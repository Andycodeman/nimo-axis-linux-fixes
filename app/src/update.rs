//! "Update available": asks GitHub, at most once a day, which release is the
//! latest, and remembers the answer in `update.conf` (see `hw::load_update`).
//!
//! The request carries a `User-Agent` of `nimo-control-center/<version>` and the
//! stored ETag, nothing else. Errors are silent: a failed check only means it is
//! tried again an hour later. Only the copy that owns the D-Bus name checks.

// Used by the window, notification and tray wiring, which comes next.
#![allow(dead_code)]

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// GitHub's latest release (it leaves out drafts and prereleases).
pub const RELEASES_URL: &str = "https://api.github.com/repos/Andycodeman/nimo-axis-linux-fixes/releases/latest";
/// The one-line installer, as the README gives it.
pub const INSTALL_URL: &str = "https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh";
/// What the "Update…" panel shows for copying.
pub const UPDATE_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/Andycodeman/nimo-axis-linux-fixes/main/install.sh | sh";

const USER_AGENT: &str = concat!("nimo-control-center/", env!("CARGO_PKG_VERSION"));
/// How long between checks.
pub const DAY: u64 = 24 * 60 * 60;
/// After a failed check (offline, rate-limited), try again this much later.
pub const RETRY: u64 = 60 * 60;
/// Notes shown in the banner: the first sentence of this many bullets.
const MAX_NOTES: usize = 3;
const MAX_NOTE_LEN: usize = 160;

/// Where to ask, and whether this is a test. `NIMO_UPDATE_URL` points the check
/// at another server (a VM test against a local `latest.json`); then the app
/// should check at every start instead of once a day.
pub fn endpoint() -> (String, bool) {
    match std::env::var("NIMO_UPDATE_URL") {
        Ok(url) if !url.is_empty() => (url, true),
        _ => (RELEASES_URL.to_string(), false),
    }
}

// ---- versions ------------------------------------------------------------------

/// `1.2.3` or `v1.2.3`, exactly three plain numbers. Anything else (`1.2`,
/// `1.2.3-rc1`, `+1.2.3`) is `None`.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.').map(|p| {
        if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) {
            p.parse::<u64>().ok()
        } else {
            None
        }
    });
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// Whether `latest` is a newer release than `current`. An unreadable version on
/// either side is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

// ---- the release -----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// The tag without its `v`: `1.2.4`.
    pub version: String,
    /// Short "what's new" lines for the banner.
    pub notes: Vec<String>,
    /// The release page, for "Full notes".
    pub url: String,
}

/// Reads GitHub's `releases/latest` reply. `None` if it isn't one.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = v["tag_name"].as_str()?.trim();
    if tag.is_empty() {
        return None;
    }
    Some(Release {
        version: tag.strip_prefix('v').unwrap_or(tag).to_string(),
        notes: summarize(v["body"].as_str().unwrap_or("")),
        url: v["html_url"].as_str().unwrap_or("").to_string(),
    })
}

/// "What's new" from a release body (the CHANGELOG section plus an
/// "**Install:**" footer): the first sentence of each of the first three
/// bullets, without Markdown's `` ` `` and `**`.
pub fn summarize(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| l.trim_start().strip_prefix("- ").or_else(|| l.trim_start().strip_prefix("* ")))
        .map(|b| clean(first_sentence(b)))
        .filter(|s| !s.is_empty())
        .take(MAX_NOTES)
        .collect()
}

/// Up to and including the first `.`, `!` or `?` that ends a sentence.
fn first_sentence(s: &str) -> &str {
    let s = s.trim();
    let b = s.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if matches!(c, b'.' | b'!' | b'?') && b.get(i + 1).is_none_or(|n| *n == b' ') {
            return &s[..=i];
        }
    }
    s
}

fn clean(s: &str) -> String {
    let s: String = s.replace("**", "").replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_NOTE_LEN {
        return s;
    }
    let cut: String = s.chars().take(MAX_NOTE_LEN - 1).collect();
    format!("{}…", cut.trim_end())
}

// ---- asking GitHub -------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Fetched {
    /// 304: the release we already know is still the latest.
    Unchanged,
    /// 200: the latest release, and the ETag to send next time.
    Release { release: Release, etag: String },
}

/// One GET of `url`, with `If-None-Match: etag` when there is one. Blocks for up
/// to 10 seconds; call it on its own thread, never on the hardware queue.
pub fn fetch(url: &str, etag: &str) -> Result<Fetched, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut req = agent.get(url).header("Accept", "application/vnd.github+json");
    if !etag.is_empty() {
        req = req.header("If-None-Match", etag);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        304 => Ok(Fetched::Unchanged),
        200 => {
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let body = resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| e.to_string())?;
            let release = parse_release(&body).ok_or("the reply isn't a release")?;
            Ok(Fetched::Release { release, etag })
        }
        s => Err(format!("HTTP {s}")),
    }
}

// ---- what we remember ------------------------------------------------------------------

/// `update.conf`.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// The "Check for updates" switch.
    pub check: bool,
    /// Unix seconds of the last check (a failed one counts as `RETRY` before the next).
    pub last_check: u64,
    pub etag: String,
    /// The latest release's version, without `v`.
    pub latest: String,
    /// Its notes and page, kept so the banner shows offline too.
    pub notes: Vec<String>,
    pub url: String,
    /// "Skip this version".
    pub skipped: String,
    /// The version we last sent a desktop notification for.
    pub notified: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            check: true,
            last_check: 0,
            etag: String::new(),
            latest: String::new(),
            notes: Vec::new(),
            url: String::new(),
            skipped: String::new(),
            notified: String::new(),
        }
    }
}

impl State {
    /// `key=value` lines, one `note=` per note. Unknown lines are ignored.
    pub fn parse(text: &str) -> State {
        let mut s = State::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().to_string();
            match k.trim() {
                "check" => s.check = v != "0",
                "last_check" => s.last_check = v.parse().unwrap_or(0),
                "etag" => s.etag = v,
                "latest" => s.latest = v,
                "note" if !v.is_empty() => s.notes.push(v),
                "url" => s.url = v,
                "skipped" => s.skipped = v,
                "notified" => s.notified = v,
                _ => {}
            }
        }
        s
    }

    pub fn format(&self) -> String {
        let one = |v: &str| v.replace(['\n', '\r'], " ");
        let mut out = format!(
            "check={}\nlast_check={}\netag={}\nlatest={}\nurl={}\nskipped={}\nnotified={}\n",
            self.check as u8,
            self.last_check,
            one(&self.etag),
            one(&self.latest),
            one(&self.url),
            one(&self.skipped),
            one(&self.notified)
        );
        for n in &self.notes {
            out.push_str(&format!("note={}\n", one(n)));
        }
        out
    }

    /// Whether it's time to ask again. A clock that went backwards counts as due.
    pub fn due(&self, now: u64) -> bool {
        self.check && (now < self.last_check || now - self.last_check >= DAY)
    }

    /// The version to offer in the banner: newer than `current` and not skipped.
    pub fn available(&self, current: &str) -> Option<&str> {
        (is_newer(&self.latest, current) && self.latest != self.skipped).then_some(self.latest.as_str())
    }

    /// Whether to send a desktop notification: once per version, never for a
    /// skipped one. The caller sets `notified` after sending it.
    pub fn should_notify(&self, current: &str) -> bool {
        self.available(current).is_some_and(|v| v != self.notified)
    }

    /// Take in the result of `fetch` made at `now`.
    pub fn record(&mut self, result: Result<Fetched, String>, now: u64) {
        match result {
            Ok(Fetched::Unchanged) => self.last_check = now,
            Ok(Fetched::Release { release, etag }) => {
                self.last_check = now;
                self.etag = etag;
                self.latest = release.version;
                self.notes = release.notes;
                self.url = release.url;
            }
            Err(_) => self.last_check = now.saturating_sub(DAY - RETRY),
        }
    }
}

// ---- running the installer --------------------------------------------------------------

/// Terminals to try, in order, and the arguments that come before the command.
const TERMINALS: &[(&str, &[&str])] = &[
    ("xdg-terminal-exec", &[]),
    ("x-terminal-emulator", &["-e"]),
    ("konsole", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("kgx", &["--"]),
    ("xfce4-terminal", &["-x"]),
    ("foot", &[]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
];

/// The shell script the terminal runs: the one-line installer with curl, or wget
/// where there's no curl (stock Ubuntu), then waits so the output stays readable.
pub fn install_script() -> String {
    format!(
        "if command -v curl >/dev/null 2>&1; then curl -fsSL {INSTALL_URL} | sh; \
         else wget -qO- {INSTALL_URL} | sh; fi; echo; printf 'Press Enter to close. '; read _"
    )
}

/// The command line that opens the first installed terminal from `TERMINALS`
/// (looked up in `path`, a `PATH` value) running `script`. `None` if there's none.
pub fn terminal_command(path: &OsStr, script: &str) -> Option<Vec<String>> {
    TERMINALS.iter().find_map(|(name, pre)| {
        let found = std::env::split_paths(path).any(|dir| is_executable(&dir.join(name)));
        found.then(|| {
            let mut argv = vec![name.to_string()];
            argv.extend(pre.iter().map(|a| a.to_string()));
            argv.extend(["sh".to_string(), "-c".to_string(), script.to_string()]);
            argv
        })
    })
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    const FIXTURE: &str = include_str!("../testdata/latest-release.json");

    #[test]
    fn versions() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v10.0.42"), Some((10, 0, 42)));
        assert_eq!(parse_version(" v1.2.3\n"), Some((1, 2, 3)));
        for bad in ["v1.2", "1.2.3.4", "v1.2.3-rc1", "1.2.3+build", "garbage", "", "v", "1..3", "+1.2.3", "1.-2.3", "V1.2.3"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer() {
        assert!(is_newer("v1.2.4", "1.2.3"));
        assert!(is_newer("1.3.0", "1.2.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.10.0", "1.9.0"), "compared as numbers, not text");
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.2", "1.2.3"));
        assert!(!is_newer("v1.3.0-rc1", "1.2.3"));
        assert!(!is_newer("garbage", "1.2.3"));
        assert!(!is_newer("1.3.0", "dev"));
        assert!(!is_newer("", "1.2.3"));
    }

    #[test]
    fn release_from_fixture() {
        let r = parse_release(FIXTURE).unwrap();
        assert_eq!(r.version, "1.2.3");
        assert_eq!(r.url, "https://github.com/Andycodeman/nimo-axis-linux-fixes/releases/tag/v1.2.3");
        assert_eq!(
            r.notes,
            [
                "Only one copy of NIMO Control Center runs.",
                "The app restarts itself after an upgrade.",
                "On CachyOS and Arch the app starts straight after installing, without logging out first.",
            ]
        );
    }

    #[test]
    fn not_releases() {
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release("{}"), None);
        assert_eq!(parse_release(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(parse_release(r#"{"tag_name":"v9.0.0","draft":true}"#), None);
        assert_eq!(parse_release(r#"{"tag_name":"v9.0.0","prerelease":true}"#), None);
        let r = parse_release(r#"{"tag_name":"v9.0.0"}"#).unwrap();
        assert_eq!((r.version.as_str(), r.notes.len(), r.url.as_str()), ("9.0.0", 0, ""));
    }

    #[test]
    fn summary() {
        let body = "\n- First **bold** `code` bit. Second sentence.\n- Uses e.g.x and v1.2 fine. More.\n\
                    - No full stop here\n- A fourth one.\n\n**Install:** `curl … | sh`\nor download it.\n";
        assert_eq!(summarize(body), ["First bold code bit.", "Uses e.g.x and v1.2 fine.", "No full stop here"]);
        assert!(summarize("**Install:** `curl … | sh`\n").is_empty());
        let long = format!("- {}", "word ".repeat(100));
        let s = &summarize(&long)[0];
        assert_eq!(s.chars().count(), MAX_NOTE_LEN);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn state_round_trip() {
        let s = State {
            check: false,
            last_check: 1_795_000_000,
            etag: "W/\"abc\"".into(),
            latest: "1.3.0".into(),
            notes: vec!["One.".into(), "Two = three.".into()],
            url: "https://example.invalid/r".into(),
            skipped: "1.2.9".into(),
            notified: "1.3.0".into(),
        };
        assert_eq!(State::parse(&s.format()), s);
        assert_eq!(State::parse(""), State::default());
        assert!(State::default().check, "on by default");
        let odd = State { notes: vec!["line\nbreak".into()], ..State::default() };
        assert_eq!(State::parse(&odd.format()).notes, ["line break"]);
        assert_eq!(State::parse("check=1\nbogus\nnew_key=x\nlast_check=nope\n"), State::default());
    }

    #[test]
    fn throttle() {
        let mut s = State { last_check: 1000, ..State::default() };
        assert!(!s.due(1000));
        assert!(!s.due(1000 + DAY - 1));
        assert!(s.due(1000 + DAY));
        assert!(s.due(999), "clock went back");
        assert!(State::default().due(1_795_000_000), "never checked");
        s.check = false;
        assert!(!s.due(1000 + DAY * 10), "switched off");
    }

    #[test]
    fn recording() {
        let now = 10 * DAY;
        let mut s = State::default();
        s.record(Err("offline".into()), now);
        assert!(!s.due(now + RETRY - 1));
        assert!(s.due(now + RETRY), "a failed check is retried an hour later");

        let release = Release { version: "1.3.0".into(), notes: vec!["New.".into()], url: "u".into() };
        s.record(Ok(Fetched::Release { release, etag: "e1".into() }), now);
        assert_eq!((s.latest.as_str(), s.etag.as_str(), s.last_check), ("1.3.0", "e1", now));
        assert!(!s.due(now + DAY - 1));

        s.record(Ok(Fetched::Unchanged), now + DAY);
        assert_eq!((s.latest.as_str(), s.etag.as_str(), s.last_check), ("1.3.0", "e1", now + DAY));
        assert_eq!(s.notes, ["New."]);

        s.record(Err("HTTP 403".into()), now + 2 * DAY);
        assert_eq!(s.latest, "1.3.0", "a failure keeps what we knew");
    }

    #[test]
    fn notify_and_skip() {
        let mut s = State { latest: "1.3.0".into(), ..State::default() };
        assert_eq!(s.available("1.2.3"), Some("1.3.0"));
        assert!(s.should_notify("1.2.3"));
        assert!(!s.should_notify("1.3.0"), "already running it");
        assert_eq!(s.available("1.3.0"), None);

        s.notified = "1.3.0".into();
        assert!(!s.should_notify("1.2.3"), "once per version");
        assert_eq!(s.available("1.2.3"), Some("1.3.0"), "the banner still shows");

        s.latest = "1.3.1".into();
        assert!(s.should_notify("1.2.3"), "a newer one notifies again");

        s.skipped = "1.3.1".into();
        assert_eq!(s.available("1.2.3"), None);
        assert!(!s.should_notify("1.2.3"));

        s.latest = "1.4.0".into();
        assert!(s.should_notify("1.2.3"), "skipping one version doesn't skip the next");

        s.latest = "1.4.0-rc1".into();
        assert_eq!(s.available("1.2.3"), None);
    }

    /// A one-shot HTTP server on 127.0.0.1 that answers `reply` and hands back
    /// the request's header lines.
    fn serve_once(reply: String) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/releases/latest", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut lines = Vec::new();
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    break;
                }
                lines.push(line);
            }
            conn.write_all(reply.as_bytes()).unwrap();
            lines
        });
        (url, handle)
    }

    fn header<'a>(lines: &'a [String], name: &str) -> Option<&'a str> {
        lines.iter().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    #[test]
    fn fetch_200() {
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: W/\"e42\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{FIXTURE}",
            FIXTURE.len()
        );
        let (url, server) = serve_once(reply);
        let got = fetch(&url, "").unwrap();
        let lines = server.join().unwrap();
        let Fetched::Release { release, etag } = got else { panic!("{got:?}") };
        assert_eq!((release.version.as_str(), etag.as_str()), ("1.2.3", "W/\"e42\""));
        assert_eq!(header(&lines, "user-agent"), Some(USER_AGENT));
        assert_eq!(header(&lines, "if-none-match"), None);
        assert_eq!(header(&lines, "cookie"), None);
    }

    #[test]
    fn fetch_304() {
        let (url, server) = serve_once("HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into());
        assert_eq!(fetch(&url, "W/\"e42\""), Ok(Fetched::Unchanged));
        let lines = server.join().unwrap();
        assert_eq!(header(&lines, "if-none-match"), Some("W/\"e42\""));
    }

    #[test]
    fn fetch_errors() {
        let body = r#"{"message":"API rate limit exceeded"}"#;
        let reply = format!("HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let (url, server) = serve_once(reply);
        assert_eq!(fetch(&url, ""), Err("HTTP 403".into()));
        server.join().unwrap();

        let reply = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let (url, server) = serve_once(reply);
        assert!(fetch(&url, "").is_err());
        server.join().unwrap();

        // Nothing listening.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(fetch(&format!("http://127.0.0.1:{port}/"), "").is_err());
    }

    /// The real API over TLS: `cargo test --release -- --ignored live_github`.
    #[test]
    #[ignore = "needs the network"]
    fn live_github() {
        let Ok(Fetched::Release { release, etag }) = fetch(RELEASES_URL, "") else { panic!("no release") };
        assert!(parse_version(&release.version).is_some(), "{}", release.version);
        assert!(!etag.is_empty());
        assert_eq!(fetch(RELEASES_URL, &etag), Ok(Fetched::Unchanged));
    }

    #[test]
    fn terminals() {
        let dir = std::env::temp_dir().join(format!("nimo-cc-test-{}-terms", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let make = |p: &Path, mode: u32| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let path = std::env::join_paths([&a, &b]).unwrap();

        assert_eq!(terminal_command(&path, "x"), None);
        make(&a.join("gnome-terminal"), 0o644);
        assert_eq!(terminal_command(&path, "x"), None, "not executable");
        make(&b.join("kitty"), 0o755);
        assert_eq!(terminal_command(&path, "x").unwrap(), ["kitty", "sh", "-c", "x"]);
        make(&b.join("konsole"), 0o755);
        assert_eq!(terminal_command(&path, "x").unwrap(), ["konsole", "-e", "sh", "-c", "x"], "earlier in the list wins");
        make(&a.join("xdg-terminal-exec"), 0o755);
        assert_eq!(terminal_command(&path, "x").unwrap(), ["xdg-terminal-exec", "sh", "-c", "x"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_falls_back_to_wget() {
        let s = install_script();
        assert!(s.contains(&format!("curl -fsSL {INSTALL_URL} | sh")));
        assert!(s.contains(&format!("wget -qO- {INSTALL_URL} | sh")));
        assert!(s.starts_with("if command -v curl"), "`curl … | sh || wget …` would never reach wget");
        assert!(UPDATE_COMMAND.contains(INSTALL_URL));
    }
}
