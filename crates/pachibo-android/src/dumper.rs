//! A long-lived reader of the phone's screen.
//!
//! `uiautomator dump` starts a new process for every dump, and on some phones
//! that start alone takes seconds (on an SH-RM15, three of a dump's five). The
//! helper here (`dumper/src/pachibo/Dumper.java`, built to a dex beside it)
//! does what `uiautomator dump --compressed` does, with the phone's own
//! uiautomator.jar, but in one process that stays up for the whole read and
//! dumps on request. It is copied to `/data/local/tmp` (no app is installed)
//! and removed when the read is done.
//!
//! It speaks a line at a time over the adb shell's standard input and output:
//! `READY 1` on start (or `FAIL <why>`); `dump` answered by `OK <bytes>` and
//! that many bytes of XML, or `ERR <why>`; `settext <hex>` (the focused
//! view given that UTF-8 text, past the keyboard) answered by `OK 0` or
//! `ERR <why>`; `quit`.

use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The helper, built from `dumper/src` by `dumper/build.sh`.
pub(crate) const DEX: &[u8] = include_bytes!("../dumper/pachibo-dumper.dex");

/// Where the helper is copied to on the phone.
pub(crate) const DEX_PATH: &str = "/data/local/tmp/pachibo-dumper.dex";

/// Where the helper has uiautomator write each dump, before it is sent.
pub(crate) const XML_PATH: &str = "/data/local/tmp/pachibo-dumper.xml";

/// The shell command that starts the helper on the phone.
pub(crate) fn launch_command() -> String {
    format!("CLASSPATH={DEX_PATH}:/system/framework/uiautomator.jar exec app_process /system/bin pachibo.Dumper {XML_PATH}")
}

/// Why the helper did not give a dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fault {
    /// It said this dump failed (uiautomator threw, the screen never idle);
    /// it is still up for the next.
    Refused(String),
    /// It said something outside the protocol, did not answer in time, or
    /// is gone: it cannot be talked to any more.
    Broken(String),
}

/// A child's output as it comes, read with a time limit: a thread reads it
/// into a channel, so a reader is never stuck on a child that says nothing.
struct Incoming {
    /// Chunks as the thread reads them; closed when the output ends.
    chunks: Receiver<Vec<u8>>,
    /// What has come and not yet been taken.
    buffer: Vec<u8>,
}

impl Incoming {
    fn new(mut source: impl Read + Send + 'static) -> Self {
        let (sender, chunks) = channel();
        std::thread::spawn(move || {
            let mut chunk = vec![0; 65_536];
            loop {
                match source.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if sender.send(chunk.get(..read).unwrap_or_default().to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self { chunks, buffer: Vec::new() }
    }

    /// Wait (until `deadline`) for more to come.
    fn more(&mut self, deadline: Instant) -> Result<(), Fault> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.chunks.recv_timeout(left) {
            Ok(chunk) => {
                self.buffer.extend(chunk);
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => Err(Fault::Broken(format!("no answer within {}s", left.as_secs()))),
            Err(RecvTimeoutError::Disconnected) => Err(Fault::Broken("the helper's output ended".to_owned())),
        }
    }

    /// The next line, without its end.
    fn line(&mut self, deadline: Instant) -> Result<String, Fault> {
        loop {
            if let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return Ok(String::from_utf8_lossy(&line).trim_end().to_owned());
            }
            self.more(deadline)?;
        }
    }

    /// The next `count` bytes.
    fn bytes(&mut self, count: usize, deadline: Instant) -> Result<Vec<u8>, Fault> {
        while self.buffer.len() < count {
            self.more(deadline)?;
        }
        Ok(self.buffer.drain(..count).collect())
    }
}

/// What a helper's reply to `dump` comes to, from its first line: the bytes
/// of XML to come, or why there are none.
pub(crate) fn reply_of(line: &str) -> Result<usize, Fault> {
    if let Some(count) = line.strip_prefix("OK ") {
        count.trim().parse().map_err(|_| Fault::Broken(format!("a reply out of the protocol: {line}")))
    } else if let Some(why) = line.strip_prefix("ERR ") {
        Err(Fault::Refused(why.to_owned()))
    } else {
        Err(Fault::Broken(format!("a reply out of the protocol: {line}")))
    }
}

/// The helper, running on the phone.
pub(crate) struct Dumper {
    /// The adb shell it runs under.
    child: Child,
    /// Where requests go.
    requests: ChildStdin,
    /// Where replies come from.
    replies: Incoming,
    /// The end of what it has said on standard error, for a log.
    said: Arc<Mutex<String>>,
}

impl Dumper {
    /// Start the helper through `command` (an adb shell running it), and wait
    /// up to `limit` for it to say it is ready; why not, where it is not.
    pub(crate) fn start(mut command: Command, limit: Duration) -> Result<Self, String> {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("could not be run: {error}"))?;
        let (Some(requests), Some(stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("its input and output could not be taken".to_owned());
        };
        let said = Arc::new(Mutex::new(String::new()));
        let kept = Arc::clone(&said);
        std::thread::spawn(move || {
            let mut chunk = vec![0; 4096];
            while let Ok(read) = stderr.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                if let Ok(mut kept) = kept.lock() {
                    kept.push_str(&String::from_utf8_lossy(chunk.get(..read).unwrap_or_default()));
                    let excess = kept.len().saturating_sub(2048);
                    let cut = (excess..kept.len()).find(|&at| kept.is_char_boundary(at)).unwrap_or(0);
                    kept.drain(..cut);
                }
            }
        });
        let mut dumper = Self { child, requests, replies: Incoming::new(stdout), said };
        let first = dumper.replies.line(Instant::now() + limit);
        match first {
            Ok(line) if line.starts_with("READY ") => Ok(dumper),
            Ok(line) => Err(format!("it said: {line} {}", dumper.said())),
            Err(fault) => Err(format!("{fault:?} {}", dumper.said())),
        }
    }

    /// What it has said on standard error lately, on one line.
    pub(crate) fn said(&self) -> String {
        self.said.lock().map_or_else(|_| String::new(), |said| said.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// Give the view with input focus `text`, as accessibility sets it (past
    /// the keyboard, which converts what is typed), within `limit`.
    pub(crate) fn set_text(&mut self, text: &str, limit: Duration) -> Result<(), Fault> {
        let deadline = Instant::now() + limit;
        let hex = text.bytes().fold(String::with_capacity(text.len() * 2), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        self.requests.write_all(format!("settext {hex}\n").as_bytes()).and_then(|()| self.requests.flush()).map_err(|error| Fault::Broken(format!("the request could not be sent: {error}")))?;
        reply_of(&self.replies.line(deadline)?).map(|_| ())
    }

    /// One dump, within `limit`.
    pub(crate) fn dump(&mut self, limit: Duration) -> Result<String, Fault> {
        let deadline = Instant::now() + limit;
        self.requests.write_all(b"dump\n").and_then(|()| self.requests.flush()).map_err(|error| Fault::Broken(format!("the request could not be sent: {error}")))?;
        let count = reply_of(&self.replies.line(deadline)?)?;
        let bytes = self.replies.bytes(count, deadline)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

impl Drop for Dumper {
    fn drop(&mut self) {
        // Asked to leave, it lets go of the phone's one uiautomation
        // connection; one that does not is stopped.
        let _ = self.requests.write_all(b"quit\n").and_then(|()| self.requests.flush());
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_says_how_much_comes_or_why_nothing_does() {
        assert_eq!(reply_of("OK 1234"), Ok(1234));
        assert_eq!(reply_of("ERR java.lang.NullPointerException: x"), Err(Fault::Refused("java.lang.NullPointerException: x".to_owned())));
        assert!(matches!(reply_of("OK many"), Err(Fault::Broken(_))));
        assert!(matches!(reply_of("WARNING: linker: something"), Err(Fault::Broken(_))));
    }

    #[test]
    fn replies_are_read_across_chunks_and_bytes_kept_whole() {
        let mut incoming = Incoming::new(std::io::Cursor::new(b"READY 1\nOK 5\nab\ncdERR no\n".to_vec()));
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(incoming.line(deadline), Ok("READY 1".to_owned()));
        assert_eq!(incoming.line(deadline), Ok("OK 5".to_owned()));
        assert_eq!(incoming.bytes(5, deadline), Ok(b"ab\ncd".to_vec()));
        assert_eq!(incoming.line(deadline), Ok("ERR no".to_owned()));
        assert!(matches!(incoming.line(deadline), Err(Fault::Broken(_))));
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_says_nothing_is_given_up_in_time() {
        let mut silent = Command::new("sleep");
        silent.arg("30");
        let started = Instant::now();
        assert!(Dumper::start(silent, Duration::from_millis(300)).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_is_talked_to_by_its_protocol() {
        // A stand-in helper: ready, then one dump of "<hierarchy/>", then a
        // refusal, as the real one answers.
        let mut stand_in = Command::new("sh");
        stand_in.args(["-c", "echo 'READY 2'; read a; printf 'OK 12\\n<hierarchy/>'; read b; echo 'ERR busy'; read c; if [ \"$c\" = 'settext e381822c' ]; then echo 'OK 0'; else echo \"ERR got $c\"; fi; read d"]);
        let mut dumper = Dumper::start(stand_in, Duration::from_secs(5)).expect("it starts");
        assert_eq!(dumper.dump(Duration::from_secs(5)), Ok("<hierarchy/>".to_owned()));
        assert_eq!(dumper.dump(Duration::from_secs(5)), Err(Fault::Refused("busy".to_owned())));
        // "あ," as UTF-8 in hex.
        assert_eq!(dumper.set_text("あ,", Duration::from_secs(5)), Ok(()));
    }
}
