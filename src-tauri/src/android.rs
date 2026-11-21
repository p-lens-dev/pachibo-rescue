//! The Android path: ぱち簿's records read off a phone's screen over adb and
//! written as a CSV P-Lens takes in at 設定 → インポート → 汎用形式.
//!
//! The phone's records never leave its own app on a non-rooted phone, and the
//! app's own export is paid-only and, on some phones, broken. What the phone
//! shows, though, can be read: this drives the app over adb and reads the
//! screen. See the `pachibo-android` crate for the reading itself.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pachibo_android::adb::{self, Adb, Waiting};
use pachibo_android::{Device as _, End, Error as ScrapeError, Walk, csv_of, rescue, survey};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::Problem;

/// A flag the screen can raise to stop a running walk, shared with the walk.
#[derive(Default)]
pub struct Cancel(pub Arc<AtomicBool>);

/// Whether the phone is being driven now. Only one survey or walk may drive
/// it at a time: two would tap over each other.
#[derive(Default)]
pub struct Driving {
    /// Raised while a survey or walk drives the phone.
    busy: Arc<AtomicBool>,
    /// Raised once the app is closing: no new survey or walk may start.
    closing: AtomicBool,
    /// Raised once the wait on closing is over: the app leaves, held back no
    /// more, even where the walk has not stopped in time.
    leaving: Arc<AtomicBool>,
}

/// The phone held for driving, let go when dropped.
struct Held(Arc<AtomicBool>);

impl Held {
    /// Hold the phone, or say it is already held (or the app is closing).
    fn take(app: &AppHandle) -> Result<Self, Problem> {
        let driving = app.state::<Driving>();
        if driving.closing.load(Ordering::Acquire) || driving.busy.swap(true, Ordering::AcqRel) {
            Err(Problem::Busy)
        } else {
            Ok(Self(driving.busy.clone()))
        }
    }
}

/// The longest the app waits on closing for a walk to stop and put the phone
/// back as it was.
const CLOSING_WAIT: Duration = Duration::from_secs(30);

/// Whether closing must wait: where the phone is being driven, the walk is
/// stopped, and the app closes once it has (it puts the phone's screen
/// setting back and writes what it read as it stops) or after
/// [`CLOSING_WAIT`], whichever comes first. The caller holds the close back
/// where this answers true.
pub fn close_after_stopping(app: &AppHandle) -> bool {
    let driving = app.state::<Driving>();
    if driving.leaving.load(Ordering::Acquire) || !driving.busy.load(Ordering::Acquire) {
        return false;
    }
    if driving.closing.swap(true, Ordering::AcqRel) {
        return true; // Already stopping, and will close.
    }
    app.state::<Cancel>().0.store(true, Ordering::Relaxed);
    let _ = app.emit("android-closing", ());
    let busy = driving.busy.clone();
    let leaving = driving.leaving.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + CLOSING_WAIT;
        while busy.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        leaving.store(true, Ordering::Release);
        app.exit(0);
    });
    true
}

impl Drop for Held {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// What went wrong reading the phone, for the screen: by kind, with the run's
/// log where one was kept.
fn problem_of(error: ScrapeError, log: Option<&Path>) -> Problem {
    let log = log.map(|path| path.display().to_string());
    let detail = match error {
        ScrapeError::AdbMissing(detail) => return Problem::AdbMissing { detail },
        ScrapeError::PhoneLost(detail) => return Problem::PhoneLost { detail, log },
        ScrapeError::Step(detail) => detail,
        ScrapeError::NoCalendar => "ぱち簿のカレンダーが開けませんでした".to_owned(),
        ScrapeError::MonthStuck { year, month } => format!("カレンダーを {year}年{month}月 にめくれませんでした"),
        ScrapeError::DayNotSelected(date) => format!("{date} の日を選べませんでした"),
        ScrapeError::RecordNotOpened(date) => format!("{date} の記録が開けませんでした"),
        ScrapeError::RecordMismatch(date) => format!("{date} の記録を開いたら、別の記録が開きました"),
        ScrapeError::NotBack(date) => format!("{date} の記録から戻ったとき、一覧が元のままではありませんでした"),
        ScrapeError::Cancelled => "中断しました".to_owned(),
    };
    Problem::ScrapeFailed { detail, log }
}

/// Seconds since the UNIX epoch, for naming and stamping logs; 0 where the
/// clock reads before it.
fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// A new log file for one run (`android-<seconds>-<what>.log` in the app's
/// log folder), headed with what was run; none where it cannot be made — a
/// run is not stopped for want of a log.
fn new_log(app: &AppHandle, what: &str) -> Option<(File, PathBuf)> {
    let dir = app.path().app_log_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("android-{}-{what}.log", unix_now()));
    let mut file = File::create(&path).ok()?;
    let version = app.package_info().version.to_string();
    writeln!(file, "pachibo-rescue {version} on {} ({}), {what}, started at UNIX {}", std::env::consts::OS, std::env::consts::ARCH, unix_now()).ok()?;
    Some((file, path))
}

/// What a read waits on the person for, for the screen.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum WaitingView {
    /// The phone went away; plug it back within so many seconds.
    Unplugged { seconds: u64 },
    /// The phone is locked; unlock it within so many seconds.
    Locked { seconds: u64 },
    /// It is back.
    Back,
}

/// The phone to drive for one run: logged to a new file (whose path is given
/// back), telling the screen when it waits on the person, and giving up
/// waiting once the run is stopped.
fn phone_for(app: &AppHandle, program: String, serial: String, what: &str, stop: Arc<AtomicBool>) -> (Adb, Option<PathBuf>) {
    let told = app.clone();
    let phone = Adb::new(program, Some(serial))
        .on_wait(move |waiting| {
            let view = match waiting {
                Waiting::Unplugged { seconds } => WaitingView::Unplugged { seconds },
                Waiting::Locked { seconds } => WaitingView::Locked { seconds },
                Waiting::Back => WaitingView::Back,
            };
            let _ = told.emit("android-waiting", view);
        })
        .stop_when(move || stop.load(Ordering::Relaxed));
    match new_log(app, what) {
        Some((file, path)) => (phone.log_to(file), Some(path)),
        None => (phone, None),
    }
}

/// Keeps the computer from sleeping on its own while held, for a reading
/// that a computer asleep would break. Only sleep from idleness is held off:
/// a lid closed or sleep chosen by hand still sleeps, and the reading then
/// fails as an unplugged phone does.
struct Awake {
    #[cfg(any(windows, target_os = "macos"))]
    _held: Option<keepawake::KeepAwake>,
}

/// Hold the computer awake, on this thread (Windows holds it per thread, so
/// this is dropped on the thread that took it). Where it cannot be held, the
/// log says so and the reading goes on.
fn stay_awake(phone: &mut Adb) -> Awake {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let held = keepawake::Builder::default().idle(true).reason("Reading ぱち簿 off the phone").app_name("pachibo-rescue").app_reverse_domain("jp.p-lens.pachibo-rescue").create();
        match held {
            Ok(held) => Awake { _held: Some(held) },
            Err(error) => {
                phone.note(&format!("the computer could not be kept from sleeping: {error}"));
                Awake { _held: None }
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        phone.note("the computer is not kept from sleeping on this system");
        Awake {}
    }
}

/// Keep the last screen read beside the run's log (`….last-screen.xml`), for
/// a walk that failed to be looked into; where it was kept.
fn keep_last_screen(phone: &Adb, log: Option<&Path>) -> Option<PathBuf> {
    let path = log?.with_extension("last-screen.xml");
    std::fs::write(&path, phone.last_screen()?).ok()?;
    Some(path)
}

/// Write any panic — something this app did not foresee — to `crash.log` in
/// the app's log folder, as well as where it would go anyway, so it can be
/// looked into on a computer where no console is shown.
pub fn log_panics(app: &AppHandle) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Where it cannot be written there is nowhere else to put it; the
        // default hook below still reports it.
        let _unwritten = std::fs::create_dir_all(&dir).and_then(|()| {
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log"))?;
            writeln!(file, "[UNIX {}] {info}\n{}", unix_now(), std::backtrace::Backtrace::force_capture())
        });
        before(info);
    }));
}

/// The bundled adb, once it has been run: its server keeps running in the
/// background after the app closes (and, on Windows, holds the app's files,
/// so an update or an uninstall could not replace them), so it is stopped on
/// exit. An adb of the person's own (`$ADB`, the PATH) is left running, as
/// other tools of theirs may be using it.
#[derive(Default)]
pub struct BundledAdb(Mutex<Option<String>>);

/// The adb program to use: `$ADB` where set, else the one bundled with the app
/// (released builds carry Android Platform Tools as a resource), else `adb` on
/// the PATH (where a developer has it). The bundled one is remembered, to stop
/// its server on exit.
fn adb_program(app: &AppHandle) -> String {
    if let Ok(path) = std::env::var("ADB")
        && !path.is_empty()
    {
        return path;
    }
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("binaries").join(if cfg!(windows) { "adb.exe" } else { "adb" });
        if bundled.is_file()
            && let Some(path) = bundled.to_str()
        {
            if let Ok(mut used) = app.state::<BundledAdb>().0.lock() {
                *used = Some(path.to_owned());
            }
            return path.to_owned();
        }
    }
    "adb".to_owned()
}

/// Stop the bundled adb's server, where the app ran it; for when the app
/// exits.
pub fn stop_bundled_adb(app: &AppHandle) {
    let used = app.state::<BundledAdb>().0.lock().ok().and_then(|mut used| used.take());
    if let Some(program) = used {
        let _ = adb::command(&program).arg("kill-server").output();
    }
}

/// Run adb with the given arguments, giving its output, or `None` where adb
/// itself could not be run (not installed).
fn run(program: &str, args: &[&str]) -> Option<std::process::Output> {
    adb::command(program).args(args).output().ok()
}

/// A connected phone as the screen shows it, for the person to confirm it is
/// the right one and ready.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    serial: String,
    model: String,
    has_pachibo: bool,
    awake: bool,
    /// Whether the phone is in airplane mode, as a read is best done: ぱち簿
    /// then loads no ads while it is driven (adb over USB needs no network).
    airplane: bool,
}

/// What adb sees, for the screen to guide the person.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Phones {
    /// adb could not be run — it is not installed or bundled.
    NoAdb,
    /// No phone is connected.
    None,
    /// A phone is connected but has not allowed this computer (the person must
    /// tap 許可 on the phone).
    Unauthorized,
    /// A phone is ready (or nearly: it may still want ぱち簿, or waking).
    Ready { phone: PhoneView },
}

/// A property read off the phone, trimmed; empty where it could not be read.
fn prop(program: &str, serial: &str, name: &str) -> String {
    run(program, &["-s", serial, "shell", "getprop", name])
        .filter(|output| output.status.success())
        .map_or_else(String::new, |output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// What adb sees now: for the screen to guide the person to a ready phone.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands a command its AppHandle by value.
pub fn android_phones(app: AppHandle) -> Phones {
    let program = adb_program(&app);
    let Some(listing) = run(&program, &["devices"]) else {
        return Phones::NoAdb;
    };
    if !listing.status.success() {
        return Phones::NoAdb;
    }
    let text = String::from_utf8_lossy(&listing.stdout);
    let mut authorized: Option<String> = None;
    let mut any_unauthorized = false;
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        if let (Some(serial), Some(state)) = (parts.next(), parts.next()) {
            if state == "device" && authorized.is_none() {
                authorized = Some(serial.to_owned());
            } else if state == "unauthorized" {
                any_unauthorized = true;
            }
        }
    }
    match authorized {
        None if any_unauthorized => Phones::Unauthorized,
        None => Phones::None,
        Some(serial) => {
            let model = prop(&program, &serial, "ro.product.model");
            let has_pachibo = run(&program, &["-s", &serial, "shell", "pm", "path", pachibo_android::PACKAGE])
                .is_some_and(|output| output.status.success() && output.stdout.starts_with(b"package:"));
            let awake = screen_on(&program, &serial);
            let airplane = run(&program, &["-s", &serial, "shell", "settings", "get", "global", "airplane_mode_on"])
                .is_some_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "1");
            Phones::Ready { phone: PhoneView { serial, model, has_pachibo, awake, airplane } }
        }
    }
}

/// Whether the phone's screen is on (so taps will land).
fn screen_on(program: &str, serial: &str) -> bool {
    run(program, &["-s", serial, "shell", "dumpsys", "power"])
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.contains("mWakefulness=Awake") || text.contains("Display Power: state=ON")
        })
}

/// Progress for the screen, as each month is reached.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressView {
    year: i32,
    month: u32,
    records: usize,
}

/// What a quick look found, for the screen to tell the person the size of the
/// job before it starts; the count is unknown where the phone did not give it
/// plainly.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurveyView {
    records: Option<usize>,
}

/// Count every record in one quick look (ぱち簿's lifetime total, off the
/// analysis screen), so the screen can say roughly how long the reading will
/// take without first walking every month.
#[tauri::command]
pub async fn android_survey(app: AppHandle, serial: String) -> Result<SurveyView, Problem> {
    let held = Held::take(&app)?;
    // A reading starts un-cancelled here, with its survey; `android_cancel`
    // (or closing the app) raises this flag, for the survey or the walk after.
    let flag = app.state::<Cancel>().0.clone();
    flag.store(false, Ordering::Relaxed);
    let program = adb_program(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let _held = held;
        let (mut phone, log) = phone_for(&app, program, serial, "survey", flag.clone());
        let _awake = stay_awake(&mut phone);
        match survey(&mut phone, || flag.load(Ordering::Relaxed)) {
            Ok(found) => Ok(SurveyView { records: found.records }),
            Err(ScrapeError::Cancelled) => Err(Problem::Cancelled),
            Err(error) => {
                phone.note(&format!("survey failed: {error}"));
                keep_last_screen(&phone, log.as_deref());
                Err(problem_of(error, log.as_deref()))
            }
        }
    })
    .await
    .map_err(|error| Problem::Broken { detail: error.to_string() })?
}

/// Stop a running walk: it returns what it has gathered so far.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands a command its State by value.
pub fn android_cancel(cancel: tauri::State<'_, Cancel>) {
    cancel.0.store(true, Ordering::Relaxed);
}

/// How a walk ended, for the screen.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EndView {
    /// It read back to where the records end.
    Finished,
    /// The person stopped it.
    Cancelled,
    /// It could not go on; the records read before are written all the same.
    Failed { problem: Problem },
}

/// What a walk wrote, for the screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueView {
    /// How many records were written.
    count: usize,
    /// How the walk ended.
    end: EndView,
    /// The months (`2026-03`) whose results did not add up to the month's
    /// total, so may be missing a record.
    unsure_months: Vec<String>,
    /// The run's log, where one was kept.
    log: Option<String>,
}

/// Read every record off the phone and write them as a CSV at `out`.
///
/// Off the main thread: the walk taps and reads the phone over many minutes.
/// Progress goes to the screen as `android-progress` events. However the walk
/// ends, every record it read is written; only a walk that read none and
/// failed is an error.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Each is one thing the screen tells the walk.
pub async fn android_rescue(app: AppHandle, serial: String, out: String, deep: bool, expected: Option<usize>, year: i32, month: u32) -> Result<RescueView, Problem> {
    let held = Held::take(&app)?;
    // Not lowered here: the survey before the walk lowered it, and a stop
    // asked for since (as the survey ended) still stands.
    let flag = app.state::<Cancel>().0.clone();
    let program = adb_program(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let _held = held;
        let (mut phone, log) = phone_for(&app, program, serial, "rescue", flag.clone());
        let _awake = stay_awake(&mut phone);
        let walk = Walk { deep, expected, start: Some((year, month)), ..Walk::default() };
        let outcome = rescue(
            &mut phone,
            walk,
            |progress| {
                let _ = app.emit("android-progress", ProgressView { year: progress.year, month: progress.month, records: progress.records });
            },
            || flag.load(Ordering::Relaxed),
        );
        phone.note(&format!("the walk ended: {:?}, {} records", outcome.end, outcome.records.len()));
        if matches!(outcome.end, End::Failed(_)) {
            keep_last_screen(&phone, log.as_deref());
        }
        drop(phone); // Put the phone's screen setting back before saying it is done.
        let end = match outcome.end {
            End::Finished => EndView::Finished,
            End::Cancelled => EndView::Cancelled,
            End::Failed(error) if outcome.records.is_empty() => return Err(problem_of(error, log.as_deref())),
            End::Failed(error) => EndView::Failed { problem: problem_of(error, log.as_deref()) },
        };
        std::fs::write(&out, csv_of(&outcome.records)).map_err(|error| Problem::WriteFailed { path: out.clone(), detail: error.to_string() })?;
        let unsure_months = outcome.unsure_months.iter().map(|(year, month)| format!("{year:04}-{month:02}")).collect();
        let log = log.map(|path| path.display().to_string());
        Ok(RescueView { count: outcome.records.len(), end, unsure_months, log })
    })
    .await
    .map_err(|error| Problem::Broken { detail: error.to_string() })?
}
