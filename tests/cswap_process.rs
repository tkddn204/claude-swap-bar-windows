use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use claude_swap_bar::cswap::{self, CswapError, Switch};

fn fake() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_cswap"))
}

/// `FAKE_CSWAP_MODE` is process-global, and `cargo test` runs these tests on
/// several threads of one process. Without this lock two tests would clobber
/// each other's mode between the `set_var` and the spawn.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_mode<T>(mode: &str, body: impl FnOnce() -> T) -> T {
    // A panicking test poisons the mutex; the guarded data is `()`, so
    // recovering is always safe and keeps one failure from cascading.
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("FAKE_CSWAP_MODE", mode);
    let out = body();
    std::env::remove_var("FAKE_CSWAP_MODE");
    out
}

#[test]
fn list_parses_a_successful_run() {
    let list = with_mode("ok", || cswap::list(&fake())).expect("should parse");
    assert_eq!(list.accounts.len(), 2);
    assert_eq!(list.active_account_number, Some(2));
}

#[test]
fn nonzero_exit_reports_the_first_stderr_line() {
    let err = with_mode("fail", || cswap::list(&fake())).unwrap_err();
    match err {
        CswapError::Failed { code, stderr } => {
            assert_eq!(code, Some(3));
            assert_eq!(stderr, "cswap: no accounts configured");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn unparseable_output_is_bad_json_not_a_panic() {
    let err = with_mode("badjson", || cswap::list(&fake())).unwrap_err();
    assert!(matches!(err, CswapError::BadJson(_)), "got {err:?}");
}

#[test]
fn a_hung_child_times_out_and_does_not_block_forever() {
    let started = Instant::now();
    let err = with_mode("hang", || cswap::list(&fake())).unwrap_err();
    assert!(matches!(err, CswapError::Timeout), "got {err:?}");
    assert!(
        started.elapsed().as_secs() < 20,
        "timeout took {:?}, expected roughly 10s",
        started.elapsed()
    );
}

#[test]
fn switch_passes_the_expected_arguments() {
    let args = with_mode("echo-args", || {
        cswap::run(&fake(), &["switch", "2", "--json"])
    })
    .expect("run should succeed");
    assert_eq!(args, "switch 2 --json");
}

#[test]
fn switch_variants_map_to_cswap_arguments() {
    assert_eq!(Switch::Account(7).args(), vec!["switch", "7", "--json"]);
    assert_eq!(Switch::Rotate.args(), vec!["switch", "--json"]);
    assert_eq!(
        Switch::Best.args(),
        vec!["switch", "--strategy", "best", "--json"]
    );
}

#[test]
fn a_missing_executable_is_not_found() {
    let err = cswap::list(&PathBuf::from("definitely-not-a-real-binary-xyz")).unwrap_err();
    assert!(matches!(err, CswapError::NotFound), "got {err:?}");
}
