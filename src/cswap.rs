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
            CswapError::Failed { code, .. } => {
                format!("갱신 실패: 종료 코드 {code:?}")
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

fn probe(name: &str) -> bool {
    let mut cmd = Command::new(name);
    cmd.arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut cmd);
    cmd.status().is_ok()
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
    let (out_tx, out_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = out_tx.send(buf);
    });
    let (err_tx, err_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        let _ = err_tx.send(buf);
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

    let out = out_rx.recv().unwrap_or_default();
    let err = err_rx.recv().unwrap_or_default();

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
