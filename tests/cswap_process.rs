use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use claude_swap_bar::cswap::{self, CswapError, Switch};

/// The test double.
///
/// It lives in `examples/` rather than `src/bin/` so the Tauri bundler does
/// not ship it inside the installer. `cargo test` still builds it, and it
/// lands beside the test binary's own directory, so derive the path from
/// `current_exe` rather than `CARGO_BIN_EXE_*`, which exists only for
/// package binaries.
fn fake() -> PathBuf {
    let mut dir = std::env::current_exe().expect("test binary has a path");
    dir.pop(); // deps/
    dir.pop(); // debug/ or release/
    dir.join("examples").join("fake_cswap.exe")
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
fn run_passes_argv_through_to_the_child() {
    let args = with_mode("echo-args", || {
        cswap::run(&fake(), &["switch", "2", "--json"])
    })
    .expect("run should succeed");
    assert_eq!(args, "switch 2 --json");
}

#[test]
fn switch_sends_its_variant_args_to_the_child() {
    // Covers the composition of Switch::args with the spawn, which neither
    // `run_passes_argv_through_to_the_child` nor the pure args test reaches.
    let args = with_mode("echo-args", || {
        cswap::run(
            &fake(),
            &Switch::Best
                .args()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
    })
    .expect("run should succeed");
    assert_eq!(args, "switch --strategy best --json");

    with_mode("ok", || cswap::switch(&fake(), Switch::Account(2)))
        .expect("switch should succeed on a zero exit");
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

#[test]
fn error_summaries_are_the_lines_the_menu_shows() {
    // These strings are the whole failure UI in v1 — the tray has no other
    // way to say something went wrong — so they are worth pinning.
    assert_eq!(CswapError::NotFound.summary(), "cswap을 찾을 수 없음");
    assert_eq!(
        CswapError::Timeout.summary(),
        "갱신 실패: 응답 없음 (10초 초과)"
    );
    assert_eq!(
        CswapError::Failed {
            code: Some(3),
            stderr: "cswap: no accounts configured".to_string(),
        }
        .summary(),
        "갱신 실패: cswap: no accounts configured"
    );
    assert_eq!(
        CswapError::Failed {
            code: Some(3),
            stderr: String::new(),
        }
        .summary(),
        "갱신 실패: 종료 코드 3",
        "an empty stderr must not render the Rust debug form of the code"
    );
    assert_eq!(
        CswapError::Failed {
            code: None,
            stderr: String::new(),
        }
        .summary(),
        "갱신 실패: 비정상 종료"
    );
    assert_eq!(
        CswapError::BadJson("expected value".to_string()).summary(),
        "갱신 실패: 응답을 해석할 수 없음"
    );
}

#[test]
fn open_dashboard_spawns_without_waiting_for_the_child() {
    // "linger" stays alive past the assertion. If open_dashboard waited on
    // the child this call would block; it must return immediately.
    //
    // The threshold has to sit between "spawned and returned" and "waited for
    // the child": a spawn costs tens of milliseconds, and a wait would cost
    // linger's full 2 seconds. One second is an order of magnitude clear of
    // both. The old 5-second threshold was calibrated against a 60-second
    // child and stopped discriminating the moment linger got short — a
    // blocking wait would have finished well inside it and passed.
    //
    // The timer starts inside the closure, after `with_mode` has acquired
    // `env_lock`, not before. `a_hung_child_times_out_and_does_not_block_forever`
    // holds that lock for its own ~10s timeout; starting the clock outside
    // the closure would count time spent waiting on that unrelated lock
    // against this test's budget, failing it under `cargo test`'s default
    // parallelism whenever the two tests race for the lock.
    let elapsed = with_mode("linger", || {
        let started = Instant::now();
        cswap::open_dashboard(&fake()).expect("spawning should succeed");
        started.elapsed()
    });
    assert!(
        elapsed.as_millis() < 1_000,
        "open_dashboard blocked for {:?}; it must not wait on the child",
        elapsed
    );
}

#[test]
fn a_nonzero_exit_is_a_failure_not_a_missing_executable() {
    // `find_exe` answers whether the executable is there, not whether it
    // succeeded. Routing probe through run_owned once made a spawnable cswap
    // that exits non-zero look missing, and the app then reported
    // "cswap을 찾을 수 없음" with a usable cswap on PATH.
    let err = with_mode("fail", || cswap::list(&fake())).unwrap_err();
    assert!(
        matches!(err, CswapError::Failed { .. }),
        "a non-zero exit must surface as Failed, not NotFound; got {err:?}"
    );
}

#[test]
fn undecodable_child_output_survives_as_replacement_characters() {
    // `cswap` is a Python program, so on a Korean Windows install it writes
    // cp949 to a redirected pipe. `read_to_string` would leave the buffer
    // empty on those bytes and erase the diagnostic entirely.
    let err = with_mode("cp949-stderr", || cswap::list(&fake())).unwrap_err();
    match err {
        CswapError::Failed { stderr, .. } => {
            assert!(
                !stderr.is_empty(),
                "non-UTF-8 stderr must not vanish; got an empty string"
            );
            assert!(
                stderr.contains('\u{FFFD}'),
                "expected replacement characters, got {stderr:?}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
