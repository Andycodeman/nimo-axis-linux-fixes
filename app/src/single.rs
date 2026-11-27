//! One copy of the app per login.
//!
//! The first copy owns a name on the session bus. A later launch asks it to
//! show its window and exits; a later `--tray` launch just exits. When an
//! upgrade replaces the app's file, the running copy runs the new file in its
//! own place (same process, same groups).

use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const NAME: &str = "io.github.andycodeman.NimoControlCenter";
const INTERFACE: &str = "io.github.andycodeman.NimoControlCenter";
const PATH: &str = "/io/github/andycodeman/NimoControlCenter";

pub enum Request {
    /// Show the window.
    Activate,
    Quit,
}

struct Service {
    handler: Box<dyn Fn(Request) + Send + Sync>,
}

#[zbus::interface(name = "io.github.andycodeman.NimoControlCenter")]
impl Service {
    fn activate(&self) {
        (self.handler)(Request::Activate);
    }

    fn quit(&self) {
        (self.handler)(Request::Quit);
    }
}

pub enum Claim {
    /// This is the only copy. Keep the `Instance`: it holds the name.
    Owner(Instance),
    /// Another copy is running; it was asked to show its window if `show` was set.
    Running,
}

pub struct Instance {
    conn: zbus::blocking::Connection,
    name: String,
}

/// Take the bus name `name`, or hand over to the copy that has it.
pub fn claim(name: &str, show: bool, handler: impl Fn(Request) + Send + Sync + 'static) -> zbus::Result<Claim> {
    let conn = zbus::blocking::connection::Builder::session()?
        .serve_at(PATH, Service { handler: Box::new(handler) })?
        .build()?;
    // Without DoNotQueue the bus queues us behind the owner and reports success.
    match conn.request_name_with_flags(name, zbus::fdo::RequestNameFlags::DoNotQueue.into()) {
        Ok(_) => Ok(Claim::Owner(Instance { conn, name: name.into() })),
        Err(zbus::Error::NameTaken) => {
            if show {
                let shown = zbus::blocking::Connection::session()
                    .and_then(|c| c.call_method(Some(name), PATH, Some(INTERFACE), "Activate", &()).map(|_| ()));
                if let Err(e) = shown {
                    eprintln!("NIMO Control Center is already running, but didn't answer ({e})");
                }
            }
            Ok(Claim::Running)
        }
        Err(e) => Err(e),
    }
}

impl Instance {
    /// Give the name back and turn this process into `exe` (it only returns if that fails).
    pub fn replace_with(&self, exe: &Path, args: &[&str]) -> std::io::Error {
        let _ = self.conn.release_name(self.name.as_str());
        let e = Command::new(exe).args(args).exec();
        let _ = self.conn.request_name(self.name.as_str());
        e
    }
}

/// The app's file when an upgrade has replaced it: /proc/self/exe then reads
/// "/usr/bin/nimo-control-center (deleted)".
fn replaced(exe: &Path) -> Option<PathBuf> {
    exe.to_str()?.strip_suffix(" (deleted)").map(PathBuf::from)
}

/// Watches for an upgrade replacing the app's file.
#[derive(Default)]
pub struct Upgrade {
    /// The new file as last seen: inode, size, modification time.
    seen: Option<(u64, u64, i64, i64)>,
    /// A new file that didn't run.
    broken: Option<(u64, u64, i64, i64)>,
}

impl Upgrade {
    /// Call every couple of seconds. Gives the new file's path once it has
    /// stayed unchanged for one call (the installer has finished writing it) and
    /// it runs with `--version`.
    pub fn check(&mut self) -> Option<PathBuf> {
        let path = replaced(&std::env::current_exe().ok()?)?;
        let m = std::fs::metadata(&path).ok()?;
        let file = Some((m.ino(), m.size(), m.mtime(), m.mtime_nsec()));
        if file == self.broken {
            return None;
        }
        if file != self.seen {
            self.seen = file;
            return None;
        }
        let runs = Command::new(&path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if runs {
            Some(path)
        } else {
            eprintln!("{} was replaced, but the new file doesn't run; keeping this copy", path.display());
            self.broken = file;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_file() {
        assert_eq!(
            replaced(Path::new("/usr/bin/nimo-control-center (deleted)")),
            Some(PathBuf::from("/usr/bin/nimo-control-center"))
        );
        assert_eq!(replaced(Path::new("/usr/bin/nimo-control-center")), None);
        assert_eq!(replaced(Path::new("/opt/my apps/nimo (deleted)")), Some(PathBuf::from("/opt/my apps/nimo")));
    }

    /// Needs a session bus of its own: `dbus-run-session -- cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn second_copy_hands_over() {
        use std::sync::mpsc;
        let name = "io.github.andycodeman.NimoControlCenter.Test";
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let first = match claim(name, false, move |r| {
            let _ = tx.lock().unwrap().send(matches!(r, Request::Activate));
        }) {
            Ok(Claim::Owner(i)) => i,
            Ok(Claim::Running) => panic!("name already owned"),
            Err(e) => panic!("no session bus: {e}"),
        };
        // A tray launch leaves the first copy alone.
        assert!(matches!(claim(name, false, |_| {}), Ok(Claim::Running)));
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
        // A window launch asks it to show its window.
        assert!(matches!(claim(name, true, |_| {}), Ok(Claim::Running)));
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(true));
        // Once the name is given back, the next launch gets it.
        assert!(first.conn.release_name(name).unwrap());
        assert!(matches!(claim(name, false, |_| {}), Ok(Claim::Owner(_))));
    }
}
