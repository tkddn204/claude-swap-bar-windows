use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::model::ListResponse;

/// Windows `CREATE_NO_WINDOW`: keeps a console window from flashing when we
/// spawn `cswap.exe` from a GUI process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum CswapError {
    /// The executable could not be spawned at all.
    NotFound,
    /// The child was still running after `TIMEOUT` and was killed.
    Timeout,
    /// The child exited with a non-zero status.
    Failed { code: Option<i32>, stderr: String },
    /// The child succeeded but its stdout was not the JSON we expected.
    BadJson(String),
}

impl CswapError {
    /// A single line suitable for the tray menu.
    pub fn summary(&self) -> String {
        match self {
            CswapError::NotFound => "cswap을 찾을 수 없음".to_string(),
            CswapError::Timeout => "갱신 실패: 응답 없음 (10초 초과)".to_string(),
            CswapError::Failed { stderr, .. } if !stderr.is_empty() => {
                format!("갱신 실패: {stderr}")
            }
            // Not `{code:?}`: that renders as "종료 코드 Some(3)", leaking Rust
            // syntax into a line the user reads in the tray menu.
            CswapError::Failed { code: Some(code), .. } => {
                format!("갱신 실패: 종료 코드 {code}")
            }
            CswapError::Failed { code: None, .. } => {
                "갱신 실패: 비정상 종료".to_string()
            }
            CswapError::BadJson(_) => "갱신 실패: 응답을 해석할 수 없음".to_string(),
        }
    }
}

/// Where to send `cswap switch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// A specific slot number.
    Account(u32),
    /// The next account in rotation.
    Rotate,
    /// Whichever account has the most quota headroom.
    Best,
}

impl Switch {
    pub fn args(&self) -> Vec<String> {
        match self {
            Switch::Account(n) => vec!["switch".into(), n.to_string(), "--json".into()],
            Switch::Rotate => vec!["switch".into(), "--json".into()],
            Switch::Best => vec![
                "switch".into(),
                "--strategy".into(),
                "best".into(),
                "--json".into(),
            ],
        }
    }
}

/// Locate `cswap.exe`: `PATH` first, then the default installer location.
pub fn find_exe() -> Option<PathBuf> {
    if probe("cswap") {
        return Some(PathBuf::from("cswap"));
    }
    let fallback = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)?
        .join(".local")
        .join("bin")
        .join("cswap.exe");
    fallback.is_file().then_some(fallback)
}

/// Whether `name` can be executed at all.
///
/// Deliberately does NOT require a zero exit. `find_exe` asks whether the
/// executable is there, and a `cswap` that spawns but exits non-zero on
/// `--version` is still the one we want; routing this through `run_owned`
/// would conflate the two and report a usable `cswap` as missing.
///
/// The wait is bounded all the same. `find_exe` is called from the refresh
/// loop whenever no executable is known, and an unbounded wait there parks
/// the refresh thread with the in-flight flag set, freezing the tray.
fn probe(name: &str) -> bool {
    let mut cmd = Command::new(name);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return false,
    };

    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // It spawned, so it exists; it just did not answer in
                    // time. Reporting it missing would be the wrong answer.
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

/// Run `cswap` with `args` and return its stdout, killing it after `TIMEOUT`.
pub fn run(exe: &Path, args: &[&str]) -> Result<String, CswapError> {
    let owned: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    run_owned(exe, &owned)
}

fn run_owned(exe: &Path, args: &[String]) -> Result<String, CswapError> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|_| CswapError::NotFound)?;

    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    // Drain both pipes on their own threads. A child that fills a pipe buffer
    // while we wait on the other one would otherwise deadlock.
    //
    // Decode lossily rather than with `read_to_string`. `cswap` is a Python
    // program, and Python writes to a redirected pipe using the locale
    // encoding — cp949 on a Korean Windows install. `read_to_string` leaves
    // the buffer EMPTY on invalid UTF-8, which would turn a real error
    // message into "the child printed nothing" and erase the diagnostic the
    // menu is supposed to show. Replacement characters beat silence.
    let (out_tx, out_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = out_tx.send(String::from_utf8_lossy(&buf).into_owned());
    });
    let (err_tx, err_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let _ = err_tx.send(String::from_utf8_lossy(&buf).into_owned());
    });

    // Poll rather than waiting on a thread: a thread parked in `Child::wait`
    // cannot be joined until the child exits, so a timeout that then joined
    // that thread would block for exactly as long as it was trying to avoid.
    let deadline = std::time::Instant::now() + TIMEOUT;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CswapError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(CswapError::NotFound),
        }
    };

    // Bounded, not `recv()`. A pipe reaches EOF only once every holder of its
    // write end closes, which includes any grandchild that outlived the child
    // we just reaped — so an unbounded wait here could hang forever after the
    // timeout logic had already done its job.
    let drain = Duration::from_secs(1);
    let out = out_rx.recv_timeout(drain).unwrap_or_default();
    let err = err_rx.recv_timeout(drain).unwrap_or_default();

    if code == Some(0) {
        Ok(out)
    } else {
        Err(CswapError::Failed {
            code,
            stderr: err.lines().next().unwrap_or("").trim().to_string(),
        })
    }
}

/// Fetch and parse the account list.
pub fn list(exe: &Path) -> Result<ListResponse, CswapError> {
    let out = run(exe, &["list", "--json"])?;
    ListResponse::parse(&out).map_err(|e| CswapError::BadJson(e.to_string()))
}

/// Switch accounts. The JSON body is ignored; only success matters.
pub fn switch(exe: &Path, target: Switch) -> Result<(), CswapError> {
    run_owned(exe, &target.args()).map(|_| ())
}

/// Where to send a user who has no `cswap` installed.
pub const INSTALL_GUIDE_URL: &str = "https://github.com/realiti4/claude-swap";

/// Open a URL in whatever the user has set as their default browser.
///
/// Routed through `cmd /c start` rather than a URL-opening crate: the process
/// machinery is already here, and one more dependency to run one more shell
/// command is not a trade worth making. The empty `""` is not decoration —
/// `start` reads a first quoted argument as the console window title, so
/// omitting it would make `start` treat the URL as a title and open nothing.
///
/// `CREATE_NO_WINDOW` applies: the console `cmd` would otherwise flash is
/// ours, not the dashboard's, and there is nothing in it to see.
pub fn open_url(url: &str) -> Result<(), std::io::Error> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/c", "start", "", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut cmd);
    cmd.spawn().map(|_| ())
}

/// Launch `cswap tui` in a console window of its own.
///
/// This is the one spawn that must NOT set `CREATE_NO_WINDOW`: the dashboard
/// is a terminal UI and needs a console to draw in. We do not wait for it.
pub fn open_dashboard(exe: &Path) -> Result<(), CswapError> {
    let mut cmd = Command::new(exe);
    cmd.arg("tui");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Windows `CREATE_NEW_CONSOLE`.
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    cmd.spawn().map(|_| ()).map_err(|_| CswapError::NotFound)
}
