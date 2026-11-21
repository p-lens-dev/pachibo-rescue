//! A [`Device`] driven over adb, the one way a computer can reach a phone's
//! screen without anything installed on the phone.
//!
//! Every adb run is bounded in time, and how it ended is sorted three ways:
//! adb itself could not be run ([`Error::AdbMissing`]), the phone could not
//! be reached ([`Error::PhoneLost`] — unplugged, offline, no longer allowing
//! this computer), or the step failed some other way ([`Error::Step`], with
//! the command, how it ended and what it said). Where the phone goes away
//! partway — a cable pulled, a loose connector — the run waits for it to
//! come back, saying so through [`Adb::on_wait`], and goes on; a step that may
//! not have landed while it was away is reported as failed, so the walk reads
//! the month again. Every run is written to the log given with
//! [`Adb::log_to`], and the last screen read is kept for a caller to save
//! where the walk fails.

use std::io::{Read as _, Write};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::dumper::{self, Dumper, Fault};
use crate::{CALENDAR_ACTIVITY, Device, Error};

/// Where on the phone `uiautomator` writes its dump, for us to read back.
const DUMP_PATH: &str = "/sdcard/pachibo-rescue.xml";

/// The setting that keeps the screen on while the phone is plugged in.
const STAY_ON: &str = "stay_on_while_plugged_in";

/// The longest any one adb run may take before it is stopped and counted as
/// failed: a half-connected phone can leave adb waiting for ever.
const RUN_LIMIT: Duration = Duration::from_secs(45);

/// How long to wait for a phone that went away to come back, and to be
/// unlocked, before giving up on it.
const RETURN_WAIT: Duration = Duration::from_secs(120);

/// The longest one dump by the helper may take: its wait for an idle screen
/// is up to ten seconds, and the dump itself a second or two.
const HELPER_DUMP_LIMIT: Duration = Duration::from_secs(30);

/// The longest the helper may take to start and say it is ready.
const HELPER_START_LIMIT: Duration = Duration::from_secs(30);

/// How long to pause before trying a failed step again, times the tries so
/// far.
const PAUSE: Duration = Duration::from_millis(900);

/// How long to give ぱち簿 brought up already running before it is read.
const LAUNCH_WAIT: Duration = Duration::from_millis(1800);

/// How long to give ぱち簿 started cold (its records loading) before it is
/// read: tapping into it too soon has crashed it.
const COLD_LAUNCH_WAIT: Duration = Duration::from_millis(5400);

/// How many times the helper may fail to start, or break, in one read before
/// the read goes on with `uiautomator dump` alone.
const HELPER_CHANCES: u32 = 3;

/// How many times to take a dump that failed (uiautomator killed, the screen
/// not idle, a file read back short) before the step counts as failed. On a
/// screen that changes while it is read, uiautomator itself can crash
/// (a `NullPointerException` in its NAF check, seen as `Killed`, exit 137);
/// the compressed dump all but avoids it, and these tries take up the rest
/// over a read of many years.
const DUMP_TRIES: u32 = 8;

/// adb as a command, ready for arguments. On Windows it is started without a
/// console window: the app has none of its own, so each adb run would
/// otherwise flash one up (and the walk runs adb thousands of times).
#[must_use]
pub fn command(program: &str) -> Command {
    #[allow(unused_mut)] // Only Windows adds to it.
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        /// `CREATE_NO_WINDOW`, from the Windows process creation flags.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// What became of one adb run.
#[derive(Debug)]
struct Ran {
    /// Whether it exited with success.
    success: bool,
    /// How it ended, in words (`exit 0`, `exit 137`, `stopped after 45s`).
    ended: String,
    /// What it wrote out.
    stdout: Vec<u8>,
    /// What it wrote as errors, trimmed.
    stderr: String,
}

/// Run a command to its end or to `limit`, whichever comes first, reading
/// its output as it goes (a dump is larger than a pipe holds). A run past the
/// limit is killed.
fn run_bounded(mut command: Command, limit: Duration) -> std::io::Result<Ran> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    });
    let err_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        sleep(Duration::from_millis(20));
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).trim().to_owned();
    Ok(match status {
        Some(status) => Ran {
            success: status.success(),
            ended: status.code().map_or_else(|| "ended by a signal".to_owned(), |code| format!("exit {code}")),
            stdout,
            stderr,
        },
        None => Ran { success: false, ended: format!("stopped after {}s with no answer", limit.as_secs()), stdout, stderr },
    })
}

/// Whether what adb said means the phone cannot be reached, rather than that
/// a step on it failed.
#[must_use]
pub fn says_phone_lost(said: &str) -> bool {
    let said = said.to_ascii_lowercase();
    [
        "no devices/emulators found",
        "device offline",
        "device unauthorized",
        "device still authorizing",
        "device still connecting",
        "error: closed",
        "protocol fault",
        "connection reset",
        "broken pipe",
    ]
    .iter()
    .any(|mark| said.contains(mark))
        || (said.contains("device") && said.contains("not found"))
}

/// Why a read waits on the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    /// The phone went away (unplugged, or the connection dropped): it is
    /// waited for, up to so many seconds.
    Unplugged {
        /// How long it is waited for.
        seconds: u64,
    },
    /// The phone is locked: it is waited for to be unlocked, up to so many
    /// seconds.
    Locked {
        /// How long it is waited for.
        seconds: u64,
    },
    /// It is back, and the read goes on.
    Back,
}

/// Whether one adb run may be run again where it failed: reading is, a tap
/// is not (a tap that landed and then reported failure would be a second
/// tap, which in a record's editor could change it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Again {
    /// Safe to run again.
    Repeatable,
    /// Run once; where it may not have landed, report it as failed.
    Once,
}

/// A phone driven over adb.
///
/// Reading keeps the phone's screen on while it is plugged in; the setting the
/// phone had before is put back when this is dropped.
pub struct Adb {
    /// The adb program to run.
    program: String,
    /// The phone's serial, where one of several is meant.
    serial: Option<String>,
    /// How long to wait after a tap, back or swipe before going on. Short:
    /// every dump waits for the screen to be idle (a second without change)
    /// before it reads, so this only gives the step time to begin.
    settle: Duration,
    /// The stay-on setting the phone had before we changed it, to put back.
    stay_on_before: Option<String>,
    /// Where each run is written, where wanted.
    log: Option<Box<dyn Write + Send>>,
    /// When this phone was taken up, for the log's times.
    began: Instant,
    /// Told when a read waits on the person (a phone to plug back, to unlock).
    on_wait: Option<Box<dyn FnMut(Waiting) + Send>>,
    /// Asked while waiting: where it answers true, waiting stops.
    stopped: Option<Box<dyn Fn() -> bool + Send>>,
    /// The last screen read, for a caller to keep where the walk fails.
    last_screen: Option<String>,
    /// Whether a phone that went away is being waited for now: a phone lost
    /// again while it is readied is not waited for afresh (which would stretch
    /// the wait without end), but ends the wait.
    awaiting: bool,
    /// The long-lived screen reader on the phone, while it is up.
    helper: Option<Dumper>,
    /// Whether the helper has been copied to the phone.
    helper_copied: bool,
    /// How many times the helper has failed to start or broken.
    helper_failures: u32,
}

impl std::fmt::Debug for Adb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Adb").field("program", &self.program).field("serial", &self.serial).finish_non_exhaustive()
    }
}

impl Adb {
    /// A phone reached through `program`, the one with `serial` where given.
    #[must_use]
    pub fn new(program: impl Into<String>, serial: Option<String>) -> Self {
        Self {
            program: program.into(),
            serial,
            settle: Duration::from_millis(300),
            stay_on_before: None,
            log: None,
            began: Instant::now(),
            on_wait: None,
            stopped: None,
            last_screen: None,
            awaiting: false,
            helper: None,
            helper_copied: false,
            helper_failures: 0,
        }
    }

    /// Write every adb run, and every note of the walk, to `log`.
    #[must_use]
    pub fn log_to(mut self, log: impl Write + Send + 'static) -> Self {
        self.log = Some(Box::new(log));
        self
    }

    /// Be told when a read waits on the person.
    #[must_use]
    pub fn on_wait(mut self, told: impl FnMut(Waiting) + Send + 'static) -> Self {
        self.on_wait = Some(Box::new(told));
        self
    }

    /// Stop waiting (for a phone to come back, or be unlocked) once `stopped`
    /// answers true.
    #[must_use]
    pub fn stop_when(mut self, stopped: impl Fn() -> bool + Send + 'static) -> Self {
        self.stopped = Some(Box::new(stopped));
        self
    }

    /// Give the view with input focus `text`, for a tool that fills in
    /// ぱち簿's screens (the seeder of test records). It is set as
    /// accessibility sets it, past the keyboard: typed keys would go through
    /// the phone's Japanese keyboard, which converts them. Needs the screen
    /// reader, which is started where it is not up.
    ///
    /// # Errors
    /// [`Error::Step`] where the screen reader cannot be had or the view
    /// would not take the text.
    pub fn set_text(&mut self, text: &str) -> Result<(), Error> {
        if self.helper.is_none() {
            let helper = self.start_helper().map_err(|why| Error::Step(format!("the screen reader would not start: {why}")))?;
            self.helper = Some(helper);
        }
        let Some(helper) = self.helper.as_mut() else {
            return Err(Error::Step("the screen reader is not up".to_owned()));
        };
        let set = helper.set_text(text, HELPER_DUMP_LIMIT);
        match set {
            Ok(()) => {
                self.write_log(&format!("screen reader: settext {} chars -> ok", text.chars().count()));
                sleep(self.settle);
                Ok(())
            }
            Err(Fault::Refused(why)) => Err(Error::Step(format!("the text was not taken: {why}"))),
            Err(Fault::Broken(why)) => {
                self.helper = None;
                Err(Error::Step(format!("the screen reader broke: {why}")))
            }
        }
    }

    /// The last screen read, as `uiautomator` described it.
    #[must_use]
    pub fn last_screen(&self) -> Option<&str> {
        self.last_screen.as_deref()
    }

    /// Write a line to the log, stamped with the time since this phone was
    /// taken up. A log that cannot be written is let go, not let stop the read.
    fn write_log(&mut self, line: &str) {
        let at = self.began.elapsed();
        if let Some(log) = &mut self.log {
            let written = writeln!(log, "[{:>8.3}s] {line}", at.as_secs_f64()).and_then(|()| log.flush());
            if written.is_err() {
                self.log = None;
            }
        }
    }

    /// Tell whoever listens that the read waits on the person, and log it.
    fn tell(&mut self, waiting: Waiting) {
        self.write_log(&format!("waiting: {waiting:?}"));
        if let Some(told) = &mut self.on_wait {
            told(waiting);
        }
    }

    /// Whether the caller has asked to stop.
    fn is_stopped(&self) -> bool {
        self.stopped.as_ref().is_some_and(|stopped| stopped())
    }

    /// adb with `-s <serial>` where one is meant, and these arguments.
    fn adb(&self, args: &[&str]) -> Command {
        let mut command = command(&self.program);
        if let Some(serial) = &self.serial {
            command.args(["-s", serial]);
        }
        command.args(args);
        command
    }

    /// Run adb once, bounded in time, logged, and sorted: its output where it
    /// succeeded, else what kind of failure it was.
    fn run_once(&mut self, args: &[&str]) -> Result<Vec<u8>, Error> {
        let shown = format!("adb {}", args.join(" "));
        let started = Instant::now();
        let ran = run_bounded(self.adb(args), RUN_LIMIT);
        let took = started.elapsed().as_secs_f64();
        match ran {
            Err(error) => {
                self.write_log(&format!("{shown} -> could not be run ({took:.2}s): {error}"));
                Err(Error::AdbMissing(format!("{}: {error}", self.program)))
            }
            Ok(ran) if ran.success => {
                self.write_log(&format!("{shown} -> ok ({took:.2}s, {} bytes)", ran.stdout.len()));
                Ok(ran.stdout)
            }
            Ok(ran) => {
                let out = String::from_utf8_lossy(&ran.stdout).trim().to_owned();
                let said = if ran.stderr.is_empty() { out } else { ran.stderr };
                self.write_log(&format!("{shown} -> {} ({took:.2}s): {said}", ran.ended));
                if says_phone_lost(&said) {
                    Err(Error::PhoneLost(said))
                } else {
                    Err(Error::Step(format!("{shown}: {}{}", ran.ended, if said.is_empty() { String::new() } else { format!(": {said}") })))
                }
            }
        }
    }

    /// Run adb, waiting for the phone where it has gone away: a repeatable
    /// run is run again once it is back; a run that is not is reported as
    /// failed, as it may not have landed.
    fn run(&mut self, args: &[&str], again: Again) -> Result<Vec<u8>, Error> {
        loop {
            match self.run_once(args) {
                Err(Error::PhoneLost(said)) if !self.awaiting => {
                    self.await_return(&said)?;
                    if again == Again::Once {
                        return Err(Error::Step(format!("adb {}: the phone was away ({said}) and is back; the step may not have landed", args.join(" "))));
                    }
                }
                ran => return ran,
            }
        }
    }

    /// Run a shell command on the phone, giving its output as text.
    fn shell(&mut self, args: &[&str], again: Again) -> Result<String, Error> {
        let mut full = vec!["shell"];
        full.extend_from_slice(args);
        Ok(String::from_utf8_lossy(&self.run(&full, again)?).into_owned())
    }

    /// Run a repeatable shell command, trying again where it fails as a step
    /// (not where the phone is gone, nor adb missing), up to `tries` times.
    fn shell_tried(&mut self, args: &[&str], tries: u32) -> Result<String, Error> {
        let mut last = Error::Step(format!("adb shell {}: not run", args.join(" ")));
        for attempt in 0..tries.max(1) {
            if attempt > 0 {
                sleep(PAUSE * attempt);
            }
            match self.shell(args, Again::Repeatable) {
                Ok(said) => return Ok(said),
                Err(error) if error.is_final() => return Err(error),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    /// Wait for a phone that went away to come back (and be unlocked), up to
    /// [`RETURN_WAIT`]; [`Error::PhoneLost`] where it does not, or the
    /// caller stops.
    fn await_return(&mut self, said: &str) -> Result<(), Error> {
        self.awaiting = true;
        let returned = self.await_return_now(said);
        self.awaiting = false;
        returned
    }

    /// [`Adb::await_return`], while `awaiting` is raised.
    fn await_return_now(&mut self, said: &str) -> Result<(), Error> {
        self.tell(Waiting::Unplugged { seconds: RETURN_WAIT.as_secs() });
        let deadline = Instant::now() + RETURN_WAIT;
        loop {
            if self.is_stopped() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::PhoneLost(said.to_owned()));
            }
            sleep(Duration::from_secs(1));
            let state = run_bounded(self.adb(&["get-state"]), Duration::from_secs(10));
            if state.is_ok_and(|ran| ran.success && String::from_utf8_lossy(&ran.stdout).trim() == "device") {
                break;
            }
        }
        self.write_log("the phone is back");
        self.ready_screen(deadline)?;
        self.tell(Waiting::Back);
        Ok(())
    }

    /// Whether the phone's lock screen is up.
    fn locked(&mut self) -> Result<bool, Error> {
        let window = self.shell(&["dumpsys", "window"], Again::Repeatable)?;
        Ok(["mKeyguardShowing=true", "isKeyguardShowing=true", "mDreamingLockscreen=true", "mShowingLockscreen=true"].iter().any(|mark| window.contains(mark)))
    }

    /// Wake the screen, keep it on while plugged in, and wait (up to
    /// `deadline`) for the lock screen to be dismissed: a locked screen takes
    /// no taps.
    fn ready_screen(&mut self, deadline: Instant) -> Result<(), Error> {
        self.shell_tried(&["input", "keyevent", "KEYCODE_WAKEUP"], 2)?;
        self.keep_awake()?;
        if !self.locked()? {
            return Ok(());
        }
        let seconds = deadline.saturating_duration_since(Instant::now()).as_secs();
        self.tell(Waiting::Locked { seconds });
        loop {
            if self.is_stopped() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::Step("the phone stayed locked".to_owned()));
            }
            sleep(Duration::from_secs(1));
            if !self.locked()? {
                self.tell(Waiting::Back);
                sleep(PAUSE);
                return Ok(());
            }
        }
    }

    /// Keep the screen on while plugged in, remembering the setting before.
    fn keep_awake(&mut self) -> Result<(), Error> {
        if self.stay_on_before.is_none() {
            let before = self.shell_tried(&["settings", "get", "global", STAY_ON], 3)?;
            let before = before.trim();
            let before = if before.is_empty() || before == "null" { "0".to_owned() } else { before.to_owned() };
            self.write_log(&format!("stay-on setting before: {before}"));
            self.stay_on_before = Some(before);
        }
        // 7 = on while plugged into AC, USB or wireless power.
        self.shell_tried(&["settings", "put", "global", STAY_ON, "7"], 3)?;
        Ok(())
    }

    /// Take a dump and read it back, once: the text where it is a whole one.
    /// By the helper where it is up (or can be started), else by
    /// `uiautomator dump`.
    fn dump_once(&mut self) -> Result<String, Error> {
        let text = match self.dump_by_helper() {
            Some(dumped) => dumped?,
            None => self.dump_by_uiautomator()?,
        };
        if !(text.contains("<hierarchy") && text.trim_end().ends_with("</hierarchy>")) {
            return Err(Error::Step(format!("the screen read back incomplete ({} bytes)", text.len())));
        }
        Ok(text)
    }

    /// A dump by the helper: none where the helper cannot be had (it would
    /// not start, or broke), so the dump is taken the old way.
    fn dump_by_helper(&mut self) -> Option<Result<String, Error>> {
        if self.helper_failures >= HELPER_CHANCES {
            return None;
        }
        if self.helper.is_none() {
            match self.start_helper() {
                Ok(helper) => {
                    self.write_log("the screen reader is up");
                    self.helper = Some(helper);
                }
                Err(why) => {
                    self.helper_failure(&format!("the screen reader would not start: {why}"));
                    return None;
                }
            }
        }
        let started = Instant::now();
        let dumped = self.helper.as_mut()?.dump(HELPER_DUMP_LIMIT);
        let took = started.elapsed().as_secs_f64();
        match dumped {
            Ok(text) => {
                self.write_log(&format!("screen reader: dump -> ok ({took:.2}s, {} bytes)", text.len()));
                Some(Ok(text))
            }
            Err(Fault::Refused(why)) => {
                self.write_log(&format!("screen reader: dump -> failed ({took:.2}s): {why}"));
                Some(Err(Error::Step(format!("the screen could not be read: {why}"))))
            }
            Err(Fault::Broken(why)) => {
                let said = self.helper.as_ref().map_or_else(String::new, Dumper::said);
                self.helper = None;
                self.helper_failure(&format!("the screen reader broke ({took:.2}s): {why} {said}"));
                None
            }
        }
    }

    /// Note that the helper failed, and stop any of it left on the phone (it
    /// would hold the one uiautomation connection a phone allows).
    fn helper_failure(&mut self, what: &str) {
        self.helper_failures += 1;
        self.write_log(&format!("{what} (failure {} of {HELPER_CHANCES})", self.helper_failures));
        let _ = self.run_once(&["shell", "pkill", "-f", "pachibo.Dumper"]);
        if self.helper_failures >= HELPER_CHANCES {
            self.write_log("going on with uiautomator dump alone");
        }
    }

    /// Copy the helper to the phone (once), stop any left from before, and
    /// start it.
    fn start_helper(&mut self) -> Result<Dumper, String> {
        if !self.helper_copied {
            let local = std::env::temp_dir().join(format!("pachibo-dumper-{}.dex", std::process::id()));
            std::fs::write(&local, dumper::DEX).map_err(|error| format!("could not be written here: {error}"))?;
            let local_path = local.to_string_lossy().into_owned();
            let copied = self.run(&["push", &local_path, dumper::DEX_PATH], Again::Repeatable);
            let _ = std::fs::remove_file(&local);
            copied.map_err(|error| format!("could not be copied to the phone: {error}"))?;
            self.helper_copied = true;
        }
        let _ = self.run_once(&["shell", "pkill", "-f", "pachibo.Dumper"]);
        Dumper::start(self.adb(&["shell", "-T", &dumper::launch_command()]), HELPER_START_LIMIT)
    }

    /// A dump by `uiautomator dump`, a new process each time.
    fn dump_by_uiautomator(&mut self) -> Result<String, Error> {
        // `uiautomator` can fail (the screen still moving, the process
        // killed) yet exit 0, leaving an old dump in place; so the old one is
        // removed first, and what it says checked. `--compressed` leaves out
        // the views that only lay others out: everything read here is kept
        // (checked against real dumps), and on the analysis screen, whose ad
        // and chart change as it is read, the full dump crashed uiautomator
        // in four reads of six where the compressed one did in none.
        let said = self.shell(&["rm", "-f", DUMP_PATH, ";", "uiautomator", "dump", "--compressed", DUMP_PATH], Again::Repeatable)?;
        if !said.contains("dumped to") {
            return Err(Error::Step(format!("uiautomator dump: {}", said.trim())));
        }
        let bytes = self.run(&["exec-out", "cat", DUMP_PATH], Again::Repeatable)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

impl Drop for Adb {
    fn drop(&mut self) {
        if self.helper.take().is_some() {
            self.write_log("the screen reader is stopped");
        }
        if self.helper_copied {
            match self.run_once(&["shell", "rm", "-f", dumper::DEX_PATH, dumper::XML_PATH]) {
                Ok(_) => self.write_log("the screen reader is removed from the phone"),
                Err(error) => self.write_log(&format!("the screen reader could not be removed from the phone: {error}")),
            }
        }
        if let Some(before) = self.stay_on_before.take() {
            match self.run_once(&["shell", "settings", "put", "global", STAY_ON, &before]) {
                Ok(_) => self.write_log(&format!("stay-on setting put back to {before}")),
                Err(error) => self.write_log(&format!("stay-on setting could not be put back to {before}: {error}")),
            }
        }
    }
}

impl Device for Adb {
    fn launch_calendar(&mut self) -> Result<(), Error> {
        // Wake it, keep it awake while it is read, and have it unlocked: a
        // sleeping or locked screen takes no taps.
        self.ready_screen(Instant::now() + RETURN_WAIT)?;
        // The app is brought up as it is, not stopped and started afresh:
        // tapping into ぱち簿 just after a cold start (its records still
        // loading) has crashed it. The walk turns to the month it wants, so
        // where the app was left does not matter; and where the app was not
        // running and so starts cold, it is given longer to load.
        let said = self.shell_tried(&["am", "start", "-W", "-n", CALENDAR_ACTIVITY], 3)?;
        let cold = said.contains("COLD");
        sleep(if cold { COLD_LAUNCH_WAIT } else { LAUNCH_WAIT });
        Ok(())
    }

    fn restart(&mut self) -> Result<(), Error> {
        // ぱち簿 keeps its records as it saves them; the walk only reads, and
        // starts afresh only between months.
        self.shell_tried(&["am", "force-stop", crate::PACKAGE], 3)?;
        self.launch_calendar()
    }

    fn dump(&mut self) -> Result<String, Error> {
        let mut last = Error::Step("uiautomator dump: not run".to_owned());
        for attempt in 0..DUMP_TRIES {
            if attempt > 0 {
                self.write_log(&format!("taking the dump again (try {} of {DUMP_TRIES}): {last}", attempt + 1));
                sleep(PAUSE * attempt);
            }
            match self.dump_once() {
                Ok(text) => {
                    self.last_screen = Some(text.clone());
                    return Ok(text);
                }
                Err(error) if error.is_final() => return Err(error),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<(), Error> {
        self.shell(&["input", "tap", &x.to_string(), &y.to_string()], Again::Once)?;
        sleep(self.settle);
        Ok(())
    }

    fn back(&mut self) -> Result<(), Error> {
        self.shell(&["input", "keyevent", "KEYCODE_BACK"], Again::Once)?;
        sleep(self.settle);
        Ok(())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), millis: u32) -> Result<(), Error> {
        let numbers = [from.0, from.1, to.0, to.1].map(|n| n.to_string());
        let [x1, y1, x2, y2] = &numbers;
        self.shell(&["input", "swipe", x1, y1, x2, y2, &millis.to_string()], Again::Once)?;
        sleep(self.settle);
        Ok(())
    }

    fn note(&mut self, what: &str) {
        self.write_log(&format!("note: {what}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_gone_is_told_from_a_step_that_failed() {
        assert!(says_phone_lost("adb: no devices/emulators found"));
        assert!(says_phone_lost("error: device offline"));
        assert!(says_phone_lost("adb: device '354961116684007' not found"));
        assert!(says_phone_lost("error: device unauthorized.\nThis adb server's $ADB_VENDOR_KEYS is not set"));
        assert!(says_phone_lost("error: closed"));
        assert!(!says_phone_lost("Killed"));
        assert!(!says_phone_lost("ERROR: could not get idle state."));
        assert!(!says_phone_lost("Error: Activity not started, unable to resolve Intent"));
    }

    #[cfg(unix)]
    #[test]
    fn a_run_past_its_limit_is_stopped() {
        let mut slow = Command::new("sleep");
        slow.arg("5");
        let started = Instant::now();
        let ran = run_bounded(slow, Duration::from_millis(200)).expect("sleep runs");
        assert!(!ran.success);
        assert!(ran.ended.contains("stopped"), "{}", ran.ended);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn a_large_output_is_read_whole() {
        let mut large = Command::new("head");
        large.args(["-c", "300000", "/dev/zero"]);
        let ran = run_bounded(large, Duration::from_secs(10)).expect("head runs");
        assert!(ran.success);
        assert_eq!(ran.stdout.len(), 300_000);
    }

    #[test]
    fn a_program_that_is_not_there_is_adb_missing() {
        let mut adb = Adb::new("/nonexistent/adb-for-pachibo-rescue", None);
        assert!(matches!(adb.dump(), Err(Error::AdbMissing(_))));
    }
}
