# Claude Swap Bar for Windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Windows system tray app that shows the current `cswap` account's usage in a tooltip and switches accounts from a native tray menu.

**Architecture:** The app is a thin wrapper around the `cswap` CLI. A background thread runs `cswap list --json` every five minutes, parses it into an in-memory snapshot, and rebuilds the tray menu and tooltip on the main thread. Account switching shells out to `cswap switch`. No credential handling, no HTTP, no config file.

**Tech Stack:** Rust 2021, Tauri v2 (tray icon and native menu only, no WebView window), `serde`/`serde_json`, `winreg` for the autostart registry key. Icons are pre-generated static assets.

**Spec:** `docs/superpowers/specs/2026-08-27-claude-swap-bar-windows-design.md`

## Global Constraints

- Target platform is Windows only. `#[cfg(windows)]` code needs no non-Windows fallback.
- Toolchain floor: rustc 1.97.1, cargo 1.97.1 (both verified present).
- Crate versions: `tauri` 2, `tauri-build` 2, `tauri-plugin-single-instance` 2, `serde` 1, `serde_json` 1, `winreg` 0.56.
- The app never creates a WebView window in v1. `app.windows` in `tauri.conf.json` stays an empty array.
- The tray icon is static. Never call `TrayIcon::set_icon`. All state changes go to the tooltip and menu only.
- Every child process spawn except `Open Dashboard` sets the `CREATE_NO_WINDOW` flag (`0x0800_0000`) so no console window flashes.
- The `cswap` subprocess timeout is 10 seconds.
- The refresh interval is 300 seconds, a constant. No settings file exists.
- Data is considered stale after 600 seconds since the last successful refresh.
- Windows tray tooltips are capped at 127 characters. Truncate by dropping whole trailing lines.
- Menu action labels stay in English (`Rotate`, `Best Quota`, `Refresh now`, `Open Dashboard`, `Start with Windows`, `Quit`). Status and error text is Korean, matching the spec.
- Commit after every task. Use `git add <explicit paths>`, never `git add -A`.

## File Structure

| Path | Responsibility |
| --- | --- |
| `Cargo.toml` | Crate manifest and dependencies |
| `build.rs` | Calls `tauri_build::build()` |
| `tauri.conf.json` | Tauri v2 config: no windows, NSIS bundle |
| `dist/index.html` | Placeholder frontend Tauri requires; seed for the v2 popover |
| `icons/tray.ico` | Executable and installer icon (already committed) |
| `icons/tray.png` | Runtime tray icon, embedded at compile time (already committed) |
| `tools/make_icon.py` | Regenerates both icons (already committed) |
| `src/main.rs` | Tauri bootstrap, tray registration, refresh loop, menu event dispatch |
| `src/model.rs` | `cswap` JSON types, display-name and staleness helpers |
| `src/cswap.rs` | Locating and running `cswap.exe`, timeout, error taxonomy |
| `src/menu.rs` | Pure `View` to `MenuModel` and tooltip-string conversion |
| `src/autostart.rs` | Registry Run-key read, write, delete |
| `src/bin/fake_cswap.rs` | Test double binary used by `cswap.rs` integration tests |
| `tests/fixtures/*.json` | Captured `cswap list --json` shapes |

`src/cswap.rs` is the only module that touches the outside world. `model.rs`, `menu.rs`, and `autostart.rs` hold no process or network state, so they are unit-testable without a running app.

---

### Task 1: Crate skeleton with a static tray icon

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `tauri.conf.json`
- Create: `dist/index.html`
- Create: `src/main.rs`
- Test: manual smoke test (GUI; no automated coverage in this task)

**Interfaces:**
- Consumes: `icons/tray.png`, `icons/tray.ico` (already in the repo)
- Produces: a runnable binary named `claude-swap-bar` that registers a tray icon and exits cleanly on `Quit`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "claude-swap-bar"
version = "0.1.0"
edition = "2021"
description = "Windows system tray app for the cswap Claude Code account switcher"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["tray-icon", "image-png"] }
tauri-plugin-single-instance = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[target.'cfg(windows)'.dependencies]
winreg = "0.56"

[profile.release]
strip = true
lto = true
```

The `image-png` feature is what makes `tauri::include_image!` accept a `.png`. Without it the macro fails to compile.

- [ ] **Step 2: Write `build.rs`**

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 3: Write `tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Claude Swap Bar",
  "version": "0.1.0",
  "identifier": "dev.tkddn.claude-swap-bar",
  "build": {
    "frontendDist": "dist"
  },
  "app": {
    "windows": [],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/tray.ico"]
  }
}
```

- [ ] **Step 4: Write `dist/index.html`**

Tauri requires `frontendDist` to resolve to a real directory even when no window is created. This file is never loaded in v1.

```html
<!doctype html>
<meta charset="utf-8">
<title>Claude Swap Bar</title>
<p>This app runs in the system tray.</p>
```

- [ ] **Step 5: Write the minimal `src/main.rs`**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let menu = MenuBuilder::new(app).text("quit", "Quit").build()?;

            TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png"))
                .tooltip("Claude Swap Bar")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Claude Swap Bar");
}
```

The `windows_subsystem = "windows"` attribute suppresses the console window in release builds while leaving it available in debug builds for `println!` diagnostics.

- [ ] **Step 6: Build and verify it compiles**

Run: `cargo build`
Expected: compiles with no errors and no warnings. The first build downloads and compiles Tauri and takes several minutes.

`tauri_build::build()` writes a generated `gen/schemas/` directory beside the
manifest on every build. It is a build artifact, so add it to `.gitignore`
alongside `/target/`:

```
/target/
/gen/
```

- [ ] **Step 7: Manual smoke test**

Run: `cargo run`
Expected: an orange double-arrow icon appears in the Windows notification area. Hovering shows `Claude Swap Bar`. Left-clicking or right-clicking opens a menu with a single `Quit` item. Clicking `Quit` exits the process and removes the icon.

If the icon is hidden behind the notification-area overflow chevron, that is normal Windows behavior, not a defect. Drag it onto the taskbar to keep it visible during later manual tests.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock build.rs tauri.conf.json dist/index.html src/main.rs .gitignore
git commit -m "feat: register a static tray icon with a quit menu"
```

---

### Task 2: Parse `cswap list --json`

**Files:**
- Create: `src/model.rs`
- Create: `tests/fixtures/list_two_accounts.json`
- Create: `tests/fixtures/list_minimal.json`
- Create: `tests/fixtures/list_empty.json`
- Modify: `src/main.rs` (add `mod model;`)
- Test: unit tests inside `src/model.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces:
  - `model::ListResponse { schema_version: u32, active_account_number: Option<u32>, accounts: Vec<Account> }`
  - `model::Account { number: u32, email: String, alias: Option<String>, active: bool, disabled: bool, usage_status: Option<String>, usage: Option<Usage>, usage_age_seconds: Option<f64> }`
  - `model::Usage { five_hour: Option<Window>, seven_day: Option<Window>, spend: Option<Spend>, scoped: Vec<Scoped> }`
  - `model::Window { pct: f64, countdown: Option<String> }`
  - `model::Spend { used: f64, limit: f64, pct: f64 }`
  - `model::Scoped { name: String, pct: f64 }`
  - `Account::display_name(&self) -> &str`
  - `Account::is_healthy(&self) -> bool`
  - `Account::status_note(&self) -> Option<&str>`
  - `ListResponse::parse(json: &str) -> Result<ListResponse, serde_json::Error>`

- [ ] **Step 1: Write the fixtures**

These are scrubbed captures of real `cswap 0.25.0` output. Emails, organization names, and UUIDs are replaced with placeholders; the field shapes are unchanged.

`tests/fixtures/list_two_accounts.json`:

```json
{
  "schemaVersion": 1,
  "activeAccountNumber": 2,
  "accounts": [
    {
      "number": 1,
      "email": "work@example.com",
      "organizationName": "Example Org",
      "organizationUuid": "00000000-0000-0000-0000-000000000001",
      "isOrganization": true,
      "active": false,
      "usageStatus": "ok",
      "usage": {
        "fiveHour": { "pct": 0.0 },
        "sevenDay": {
          "pct": 12.0,
          "resetsAt": "2026-09-02T06:00:00.361887+00:00",
          "countdown": "6d 5h",
          "clock": "Sep 2 15:00"
        }
      },
      "usageFetchedAt": "2026-08-27T00:56:00Z",
      "usageAgeSeconds": 0.0
    },
    {
      "number": 2,
      "email": "personal@example.com",
      "organizationName": "personal@example.com's Organization",
      "organizationUuid": "00000000-0000-0000-0000-000000000002",
      "isOrganization": true,
      "active": true,
      "usageStatus": "ok",
      "usage": {
        "fiveHour": {
          "pct": 25.0,
          "resetsAt": "2026-08-27T05:10:00.369589+00:00",
          "countdown": "4h 14m",
          "clock": "14:10"
        },
        "sevenDay": {
          "pct": 55.0,
          "resetsAt": "2026-08-29T05:00:00.369612+00:00",
          "countdown": "2d 4h",
          "clock": "Aug 29 14:00",
          "expectedPct": 69.0,
          "aheadOfPace": false,
          "projectedExhaustionAt": "2026-08-30T23:47:13Z",
          "willLastToReset": true
        },
        "spend": {
          "used": 90.62,
          "limit": 200.0,
          "pct": 45.31,
          "currency": "USD"
        },
        "scoped": [
          { "pct": 0.0, "name": "Fable" }
        ]
      },
      "usageFetchedAt": "2026-08-27T00:55:58Z",
      "usageAgeSeconds": 1.6
    }
  ]
}
```

`tests/fixtures/list_minimal.json` covers an account with no usage data at all, an alias, a disabled flag, and a non-`ok` status:

```json
{
  "schemaVersion": 1,
  "activeAccountNumber": 3,
  "accounts": [
    {
      "number": 3,
      "email": "token@example.com",
      "alias": "bot",
      "active": true,
      "disabled": true,
      "usageStatus": "rate_limited"
    }
  ]
}
```

`tests/fixtures/list_empty.json` covers zero accounts and no active account:

```json
{
  "schemaVersion": 1,
  "activeAccountNumber": null,
  "accounts": []
}
```

- [ ] **Step 2: Write the failing tests**

Append to `src/model.rs` (the file does not exist yet; create it containing only this test module for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = include_str!("../tests/fixtures/list_two_accounts.json");
    const MINIMAL: &str = include_str!("../tests/fixtures/list_minimal.json");
    const EMPTY: &str = include_str!("../tests/fixtures/list_empty.json");

    #[test]
    fn parses_two_accounts_with_full_usage() {
        let list = ListResponse::parse(TWO).expect("fixture should parse");
        assert_eq!(list.schema_version, 1);
        assert_eq!(list.active_account_number, Some(2));
        assert_eq!(list.accounts.len(), 2);

        let active = &list.accounts[1];
        assert_eq!(active.number, 2);
        assert!(active.active);
        assert!(!active.disabled);

        let usage = active.usage.as_ref().expect("active account has usage");
        assert_eq!(usage.five_hour.as_ref().unwrap().pct, 25.0);
        assert_eq!(
            usage.five_hour.as_ref().unwrap().countdown.as_deref(),
            Some("4h 14m")
        );
        assert_eq!(usage.seven_day.as_ref().unwrap().pct, 55.0);
        let spend = usage.spend.as_ref().expect("active account has spend");
        assert_eq!(spend.used, 90.62);
        assert_eq!(spend.limit, 200.0);
        assert_eq!(usage.scoped.len(), 1);
        assert_eq!(usage.scoped[0].name, "Fable");
    }

    #[test]
    fn five_hour_window_without_countdown_parses() {
        let list = ListResponse::parse(TWO).unwrap();
        let idle = &list.accounts[0];
        let five = idle.usage.as_ref().unwrap().five_hour.as_ref().unwrap();
        assert_eq!(five.pct, 0.0);
        assert_eq!(five.countdown, None);
    }

    #[test]
    fn account_without_usage_parses() {
        let list = ListResponse::parse(MINIMAL).unwrap();
        let account = &list.accounts[0];
        assert!(account.usage.is_none());
        assert!(account.disabled);
        assert_eq!(account.usage_age_seconds, None);
    }

    #[test]
    fn empty_account_list_parses() {
        let list = ListResponse::parse(EMPTY).unwrap();
        assert!(list.accounts.is_empty());
        assert_eq!(list.active_account_number, None);
    }

    #[test]
    fn display_name_prefers_alias_over_email() {
        let list = ListResponse::parse(MINIMAL).unwrap();
        assert_eq!(list.accounts[0].display_name(), "bot");

        let list = ListResponse::parse(TWO).unwrap();
        assert_eq!(list.accounts[0].display_name(), "work@example.com");
    }

    #[test]
    fn health_follows_usage_status_verbatim() {
        let ok = ListResponse::parse(TWO).unwrap();
        assert!(ok.accounts[0].is_healthy());
        assert_eq!(ok.accounts[0].status_note(), None);

        let limited = ListResponse::parse(MINIMAL).unwrap();
        assert!(!limited.accounts[0].is_healthy());
        assert_eq!(limited.accounts[0].status_note(), Some("rate_limited"));
    }

    #[test]
    fn missing_usage_status_counts_as_unhealthy_without_a_note() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true}]}"#;
        let list = ListResponse::parse(json).unwrap();
        assert!(!list.accounts[0].is_healthy());
        assert_eq!(list.accounts[0].status_note(), None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"schemaVersion":2,"activeAccountNumber":1,"somethingNew":true,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","brandNewField":[1,2,3]}]}"#;
        let list = ListResponse::parse(json).expect("forward-compatible parse");
        assert_eq!(list.schema_version, 2);
        assert_eq!(list.accounts.len(), 1);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(ListResponse::parse("{").is_err());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Add `mod model;` to the top of `src/main.rs`, then run:

Run: `cargo test model`
Expected: FAIL. The compiler reports `cannot find type ListResponse in this scope` and similar, because only the test module exists.

There is no library target yet — the module lives in the binary crate, so
plain `cargo test model` is the right command here. Task 3 adds the library.

- [ ] **Step 4: Write the implementation**

Prepend to `src/model.rs`, above the test module:

```rust
// Transitional: until Task 3 adds a library target, this module lives in the
// binary crate, where nothing calls it yet and every public item trips
// `dead_code`. Task 3 moves it into the library and deletes this attribute.
#![allow(dead_code)]

use serde::Deserialize;

/// One `cswap list --json` response.
///
/// Every optional field is optional because `cswap` omits keys rather than
/// emitting nulls when a value is unavailable, and because new keys appear
/// across `cswap` versions. Unknown keys are ignored by default, which is
/// what keeps this forward-compatible.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse {
    pub schema_version: u32,
    #[serde(default)]
    pub active_account_number: Option<u32>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub number: u32,
    pub email: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub usage_status: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub usage_age_seconds: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
    #[serde(default)]
    pub spend: Option<Spend>,
    #[serde(default)]
    pub scoped: Vec<Scoped>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub pct: f64,
    #[serde(default)]
    pub countdown: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub used: f64,
    pub limit: f64,
    pub pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scoped {
    pub name: String,
    pub pct: f64,
}

impl ListResponse {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Account {
    /// The alias if one is set, otherwise the email address.
    pub fn display_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.email)
    }

    /// `cswap` reports `"ok"` for a usable account. Any other value, including
    /// a missing one, means we should not claim the account is fine.
    pub fn is_healthy(&self) -> bool {
        self.usage_status.as_deref() == Some("ok")
    }

    /// The raw status string when it is not `"ok"`, for display verbatim.
    ///
    /// We deliberately do not map these onto an enum: the full set of values
    /// `cswap` can emit is not documented, and inventing categories would
    /// hide states we have never seen.
    pub fn status_note(&self) -> Option<&str> {
        match self.usage_status.as_deref() {
            Some("ok") | None => None,
            Some(other) => Some(other),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test model`
Expected: PASS, 9 tests.

- [ ] **Step 6: Commit**

```bash
git add src/model.rs src/main.rs tests/fixtures/
git commit -m "feat: parse cswap list --json into typed account snapshots"
```

---

### Task 3: Run the `cswap` CLI

**Files:**
- Create: `src/cswap.rs`
- Create: `src/bin/fake_cswap.rs`
- Create: `tests/cswap_process.rs`
- Modify: `src/main.rs` (add `mod cswap;`)
- Test: `tests/cswap_process.rs`

**Interfaces:**
- Consumes: `model::ListResponse` from Task 2
- Produces:
  - `cswap::CswapError` with variants `NotFound`, `Timeout`, `Failed { code: Option<i32>, stderr: String }`, `BadJson(String)`
  - `CswapError::summary(&self) -> String` — one Korean line for the menu
  - `cswap::find_exe() -> Option<std::path::PathBuf>`
  - `cswap::run(exe: &Path, args: &[&str]) -> Result<String, CswapError>`
  - `cswap::list(exe: &Path) -> Result<model::ListResponse, CswapError>`
  - `cswap::Switch` enum with variants `Account(u32)`, `Rotate`, `Best`
  - `cswap::switch(exe: &Path, target: Switch) -> Result<(), CswapError>`

- [ ] **Step 1: Write the test double binary**

`src/bin/fake_cswap.rs`. Cargo builds this as a second binary; the integration test finds it through the `CARGO_BIN_EXE_fake_cswap` environment variable that Cargo sets automatically.

```rust
//! Test double for `cswap.exe`. Behavior is selected by `FAKE_CSWAP_MODE`.

use std::io::Write;

fn main() {
    let mode = std::env::var("FAKE_CSWAP_MODE").unwrap_or_else(|_| "ok".to_string());
    match mode.as_str() {
        "ok" => {
            print!(
                "{}",
                include_str!("../../tests/fixtures/list_two_accounts.json")
            );
        }
        "fail" => {
            let mut err = std::io::stderr();
            writeln!(err, "cswap: no accounts configured").unwrap();
            writeln!(err, "second line of noise").unwrap();
            std::process::exit(3);
        }
        "cp949-stderr" => {
            // "계정 없음" encoded as cp949 — what a Python child actually
            // writes to a redirected pipe on a Korean Windows install. These
            // bytes are not valid UTF-8.
            let bytes: &[u8] = &[0xB0, 0xE8, 0xC1, 0xA4, 0x20, 0xBE, 0xF8, 0xC0, 0xBD, 0x0A];
            std::io::stderr().write_all(bytes).unwrap();
            std::process::exit(4);
        }
        "hang" => {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        "badjson" => {
            print!("{{ not json");
        }
        "echo-args" => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            print!("{}", args.join(" "));
        }
        other => {
            eprintln!("unknown FAKE_CSWAP_MODE: {other}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 2: Write the failing integration tests**

`tests/cswap_process.rs`:

```rust
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
```

Integration tests under `tests/` can only import from a library target, so this task also converts the crate to a library plus binary. That is Step 4.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --test cswap_process`
Expected: FAIL with `unresolved import claude_swap_bar` — there is no library target yet.

- [ ] **Step 4: Add a library target**

Create `src/lib.rs`:

```rust
pub mod cswap;
pub mod menu;
pub mod model;

#[cfg(windows)]
pub mod autostart;
```

`menu` and `autostart` do not exist yet. Create empty placeholder files so the crate compiles, and delete these placeholder lines when the real modules land in Tasks 4 and 6:

- `src/menu.rs` containing only `// filled in by Task 4`
- `src/autostart.rs` containing only `// filled in by Task 6`

Add the explicit targets to `Cargo.toml` so both the library and the binary build from the same sources:

```toml
[lib]
name = "claude_swap_bar"
path = "src/lib.rs"

[[bin]]
name = "claude-swap-bar"
path = "src/main.rs"
```

Delete the transitional `#![allow(dead_code)]` attribute and its comment from
the top of `src/model.rs`. It existed only because `model` lived in the binary
crate with no callers; as a module of the library its public items are reachable
from outside the crate, so `dead_code` no longer fires. If a warning does
appear after this, that item is genuinely unused — report it rather than
re-adding the suppression.

In `src/main.rs`, delete the `mod model;` line added in Task 2. The binary does
not use `model` yet — Task 5 adds the `use claude_swap_bar::model;` import when
it actually needs the type. Leaving the `mod` declaration in place would compile
the module a second time inside the binary crate; adding an unused `use` now
would only produce a warning.

- [ ] **Step 5: Write `src/cswap.rs`**

```rust
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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test cswap_process`
Expected: PASS, 10 tests. The `a_hung_child_times_out_and_does_not_block_forever` test takes about ten seconds.

`Switch::args()` returns `Vec<String>` while the assertions compare against
`vec!["switch", "7", "--json"]`. That compiles: `Vec<A>` implements
`PartialEq<Vec<B>>` whenever `A: PartialEq<B>`, and `String: PartialEq<&str>`.

- [ ] **Step 7: Verify against the real CLI**

Run: `cargo run --bin fake_cswap` is not the point here; instead check the real binary is discoverable.

```bash
cargo test --test cswap_process -- --nocapture
cswap list --json | head -5
```

Expected: the fixture-driven tests pass, and the real `cswap list --json` prints JSON starting with `{`. If `cswap` is not on `PATH`, `find_exe` falls back to `%USERPROFILE%\.local\bin\cswap.exe`.

- [ ] **Step 8: Commit**

```bash
git add src/lib.rs src/cswap.rs src/bin/fake_cswap.rs src/menu.rs src/autostart.rs src/model.rs tests/cswap_process.rs src/main.rs Cargo.toml
git commit -m "feat: run the cswap CLI with a timeout and typed errors"
```

---

### Task 4: Build the menu model and tooltip

**Files:**
- Modify: `src/menu.rs` (replace the Task 3 placeholder)
- Test: unit tests inside `src/menu.rs`

**Interfaces:**
- Consumes: `model::ListResponse`, `model::Account` from Task 2
- Produces:
  - `menu::View<'a> { snapshot: Option<&'a ListResponse>, age_secs: Option<u64>, error: Option<&'a str>, notice: Option<&'a str> }`
  - `menu::MenuModel { notice: Option<String>, accounts: Vec<AccountRow>, refresh_label: String, tooltip: String }`
  - `menu::AccountRow { id: String, label: String, checked: bool, enabled: bool }`
  - `menu::build(view: &View) -> MenuModel`
  - `menu::TOOLTIP_MAX: usize = 127`
  - `menu::STALE_AFTER_SECS: u64 = 600`
  - Menu ids: `acct:<number>`, `rotate`, `best`, `refresh`, `dashboard`, `autostart`, `quit`, `notice`

- [ ] **Step 1: Write the failing tests**

Replace the contents of `src/menu.rs` with this test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ListResponse;

    const TWO: &str = include_str!("../tests/fixtures/list_two_accounts.json");
    const MINIMAL: &str = include_str!("../tests/fixtures/list_minimal.json");
    const EMPTY: &str = include_str!("../tests/fixtures/list_empty.json");

    fn view_of(json: &str) -> ListResponse {
        ListResponse::parse(json).unwrap()
    }

    #[test]
    fn account_rows_carry_ids_labels_and_the_active_check() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });

        assert_eq!(model.accounts.len(), 2);
        assert_eq!(model.accounts[0].id, "acct:1");
        assert_eq!(model.accounts[0].label, "1  work@example.com   5h 0%  7d 12%");
        assert!(!model.accounts[0].checked);
        assert!(model.accounts[0].enabled);

        assert_eq!(model.accounts[1].id, "acct:2");
        assert_eq!(
            model.accounts[1].label,
            "2  personal@example.com   5h 25%  7d 55%"
        );
        assert!(model.accounts[1].checked);
    }

    #[test]
    fn an_alias_replaces_the_email_and_a_bad_status_is_appended() {
        let list = view_of(MINIMAL);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.accounts[0].label, "3  bot   rate_limited");
        assert!(model.accounts[0].checked);
    }

    #[test]
    fn an_account_with_no_usage_shows_a_dash() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok"}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.accounts[0].label, "1  a@example.com   사용량 없음");
    }

    #[test]
    fn zero_accounts_produce_no_rows_and_a_notice() {
        let list = view_of(EMPTY);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(model.accounts.is_empty());
        assert_eq!(model.notice.as_deref(), Some("등록된 계정 없음"));
    }

    #[test]
    fn an_error_takes_priority_over_the_snapshot_for_the_notice() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: Some("갱신 실패: boom"),
            notice: None,
        });
        assert_eq!(model.notice.as_deref(), Some("갱신 실패: boom"));
        assert_eq!(model.accounts.len(), 2, "rows survive a failed refresh");
    }

    #[test]
    fn a_transient_notice_outranks_an_error() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: Some("갱신 실패: boom"),
            notice: Some("전환 실패: 잠김"),
        });
        assert_eq!(model.notice.as_deref(), Some("전환 실패: 잠김"));
    }

    #[test]
    fn stale_data_is_called_out_in_the_notice() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(720),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.notice.as_deref(),
            Some("사용량 정보 오래됨 (12분 전)")
        );
    }

    #[test]
    fn the_refresh_label_reports_the_last_successful_update() {
        let list = view_of(TWO);
        let at = |secs| {
            build(&View {
                snapshot: Some(&list),
                age_secs: Some(secs),
                error: None,
                notice: None,
            })
            .refresh_label
        };
        assert_eq!(at(5), "Refresh now   · 방금 갱신");
        assert_eq!(at(120), "Refresh now   · 2분 전 갱신");
        assert_eq!(at(7200), "Refresh now   · 2시간 전 갱신");

        let never = build(&View {
            snapshot: None,
            age_secs: None,
            error: None,
            notice: None,
        });
        assert_eq!(never.refresh_label, "Refresh now");
    }

    #[test]
    fn the_tooltip_describes_the_active_account() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.tooltip,
            "personal@example.com (#2)\n\
             5h  25%   리셋 4h 14m\n\
             7d  55%   리셋 2d 4h\n\
             $   90.62 / 200  (45%)"
        );
    }

    #[test]
    fn the_tooltip_omits_the_spend_line_when_there_is_no_spend() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","usage":{"fiveHour":{"pct":3.0},
            "sevenDay":{"pct":8.0,"countdown":"5d 1h"}}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.tooltip,
            "a@example.com (#1)\n5h   3%\n7d   8%   리셋 5d 1h"
        );
    }

    #[test]
    fn the_tooltip_lists_non_zero_per_model_limits() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","usage":{"fiveHour":{"pct":3.0},
            "scoped":[{"name":"Fable","pct":0.0},{"name":"Opus","pct":42.0}]}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(model.tooltip.contains("Opus  42%"));
        assert!(!model.tooltip.contains("Fable"), "0% entries are dropped");
    }

    #[test]
    fn the_tooltip_reports_errors_instead_of_stale_numbers() {
        let model = build(&View {
            snapshot: None,
            age_secs: None,
            error: Some("cswap을 찾을 수 없음"),
            notice: None,
        });
        assert_eq!(model.tooltip, "cswap을 찾을 수 없음");
    }

    #[test]
    fn the_tooltip_marks_stale_data() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(720),
            error: None,
            notice: None,
        });
        assert!(model.tooltip.starts_with("personal@example.com (#2)"));
        assert!(model.tooltip.contains("(12분 전)"));
    }

    #[test]
    fn the_tooltip_drops_whole_trailing_lines_to_fit_the_cap() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,
            "email":"a-very-long-address-for-testing-truncation@example.com",
            "active":true,"usageStatus":"ok","usage":{
            "fiveHour":{"pct":3.0,"countdown":"4h 14m"},
            "sevenDay":{"pct":8.0,"countdown":"5d 1h"},
            "spend":{"used":90.62,"limit":200.0,"pct":45.31},
            "scoped":[{"name":"Opus","pct":42.0},{"name":"Sonnet","pct":31.0}]}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(
            model.tooltip.chars().count() <= TOOLTIP_MAX,
            "tooltip was {} chars: {:?}",
            model.tooltip.chars().count(),
            model.tooltip
        );
        assert!(model.tooltip.starts_with(
            "a-very-long-address-for-testing-truncation@example.com (#1)"
        ));
        assert!(!model.tooltip.ends_with('\n'));
    }

    #[test]
    fn a_snapshot_with_no_active_account_still_produces_a_tooltip() {
        let list = view_of(EMPTY);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.tooltip, "등록된 계정 없음");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test menu`
Expected: FAIL. The compiler reports `cannot find function build in this scope` and `cannot find struct View in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/menu.rs`, above the test module:

```rust
use crate::model::{Account, ListResponse};

/// Windows caps `NOTIFYICONDATA::szTip` at 128 UTF-16 code units including the
/// terminator. We budget in characters, which is conservative enough.
pub const TOOLTIP_MAX: usize = 127;

/// A snapshot older than this is called out rather than shown as if current.
pub const STALE_AFTER_SECS: u64 = 600;

/// Everything the menu needs, with no clock or process dependency, so the
/// whole conversion stays testable.
pub struct View<'a> {
    /// The most recent successful snapshot, if we ever got one.
    pub snapshot: Option<&'a ListResponse>,
    /// Seconds since that snapshot was taken.
    pub age_secs: Option<u64>,
    /// A refresh error that is still current.
    pub error: Option<&'a str>,
    /// A one-shot message such as a failed switch. Outranks `error`.
    pub notice: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Menu id, always `acct:<number>`.
    pub id: String,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Disabled line shown above the account rows, when there is something to say.
    pub notice: Option<String>,
    pub accounts: Vec<AccountRow>,
    pub refresh_label: String,
    pub tooltip: String,
}

pub fn build(view: &View) -> MenuModel {
    let accounts = view
        .snapshot
        .map(|s| s.accounts.iter().map(account_row).collect())
        .unwrap_or_default();

    MenuModel {
        notice: notice_line(view),
        accounts,
        refresh_label: refresh_label(view.age_secs),
        tooltip: tooltip(view),
    }
}

fn account_row(account: &Account) -> AccountRow {
    AccountRow {
        id: format!("acct:{}", account.number),
        label: format!(
            "{}  {}   {}",
            account.number,
            account.display_name(),
            row_detail(account)
        ),
        checked: account.active,
        // Disabled accounts are held out of auto-rotation but can still be
        // switched to by hand, so they stay clickable.
        enabled: true,
    }
}

fn row_detail(account: &Account) -> String {
    if let Some(note) = account.status_note() {
        return note.to_string();
    }
    let Some(usage) = account.usage.as_ref() else {
        return "사용량 없음".to_string();
    };
    let mut parts = Vec::new();
    if let Some(w) = usage.five_hour.as_ref() {
        parts.push(format!("5h {}%", round_pct(w.pct)));
    }
    if let Some(w) = usage.seven_day.as_ref() {
        parts.push(format!("7d {}%", round_pct(w.pct)));
    }
    if parts.is_empty() {
        "사용량 없음".to_string()
    } else {
        parts.join("  ")
    }
}

fn notice_line(view: &View) -> Option<String> {
    if let Some(notice) = view.notice {
        return Some(notice.to_string());
    }
    if let Some(error) = view.error {
        return Some(error.to_string());
    }
    match (view.snapshot, view.age_secs) {
        (Some(s), _) if s.accounts.is_empty() => Some("등록된 계정 없음".to_string()),
        (Some(_), Some(age)) if age >= STALE_AFTER_SECS => {
            Some(format!("사용량 정보 오래됨 ({})", ago(age)))
        }
        _ => None,
    }
}

fn refresh_label(age_secs: Option<u64>) -> String {
    match age_secs {
        Some(age) => format!("Refresh now   · {} 갱신", ago(age)),
        None => "Refresh now".to_string(),
    }
}

/// `"방금"`, `"2분 전"`, `"2시간 전"`.
fn ago(secs: u64) -> String {
    if secs < 60 {
        "방금".to_string()
    } else if secs < 3600 {
        format!("{}분 전", secs / 60)
    } else {
        format!("{}시간 전", secs / 3600)
    }
}

/// Percentages arrive as floats but are always shown as whole numbers.
fn round_pct(pct: f64) -> i64 {
    pct.round() as i64
}

fn tooltip(view: &View) -> String {
    if let Some(error) = view.error {
        return truncate_lines(error);
    }
    let Some(snapshot) = view.snapshot else {
        return "Claude Swap Bar".to_string();
    };
    let Some(account) = snapshot.accounts.iter().find(|a| a.active) else {
        return "등록된 계정 없음".to_string();
    };

    let stale = view
        .age_secs
        .map(|age| age >= STALE_AFTER_SECS)
        .unwrap_or(false);

    let mut lines = Vec::new();
    let head = format!("{} (#{})", account.display_name(), account.number);
    lines.push(if stale {
        format!("{head}  ({})", ago(view.age_secs.unwrap_or(0)))
    } else {
        head
    });

    if let Some(note) = account.status_note() {
        lines.push(note.to_string());
    }

    if let Some(usage) = account.usage.as_ref() {
        if let Some(w) = usage.five_hour.as_ref() {
            lines.push(window_line("5h", w.pct, w.countdown.as_deref()));
        }
        if let Some(w) = usage.seven_day.as_ref() {
            lines.push(window_line("7d", w.pct, w.countdown.as_deref()));
        }
        if let Some(s) = usage.spend.as_ref() {
            lines.push(format!(
                "$   {:.2} / {}  ({}%)",
                s.used,
                trim_float(s.limit),
                round_pct(s.pct)
            ));
        }
        for scoped in usage.scoped.iter().filter(|s| s.pct > 0.0) {
            lines.push(format!("{}  {}%", scoped.name, round_pct(scoped.pct)));
        }
    }

    truncate_lines(&lines.join("\n"))
}

fn window_line(label: &str, pct: f64, countdown: Option<&str>) -> String {
    let head = format!("{label}  {:>2}%", round_pct(pct));
    match countdown {
        Some(c) => format!("{head}   리셋 {c}"),
        None => head,
    }
}

/// `200.0` renders as `200`, `12.5` as `12.5`.
fn trim_float(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Drop whole trailing lines until the text fits `TOOLTIP_MAX`.
fn truncate_lines(text: &str) -> String {
    if text.chars().count() <= TOOLTIP_MAX {
        return text.to_string();
    }
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.len() > 1 {
        lines.pop();
        let candidate = lines.join("\n");
        if candidate.chars().count() <= TOOLTIP_MAX {
            return candidate;
        }
    }
    // A single line that is still too long: cut it at the character boundary.
    text.chars().take(TOOLTIP_MAX).collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test menu`
Expected: PASS, 15 tests.

The exact label strings in the tests were written against this formatting. If a test fails on spacing, fix the implementation to match the test, not the other way around — the tests encode the spec's tooltip layout.

- [ ] **Step 5: Commit**

```bash
git add src/menu.rs
git commit -m "feat: derive tray menu rows and tooltip text from a snapshot"
```

---

### Task 5: Wire the tray to live data

**Files:**
- Modify: `src/main.rs` (replace the Task 1 body)
- Test: manual smoke test, plus the existing suites must stay green

**Interfaces:**
- Consumes: `cswap::{find_exe, list, switch, CswapError, Switch}`, `menu::{build, View, MenuModel}`, `model::ListResponse`
- Produces: a working app. No new public API.

- [ ] **Step 1: Replace `src/main.rs`**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use claude_swap_bar::cswap::{self, CswapError, Switch};
use claude_swap_bar::menu::{self, MenuModel, View};
use claude_swap_bar::model::ListResponse;

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Everything the refresh loop and the menu builder share.
struct State {
    exe: Option<PathBuf>,
    snapshot: Option<ListResponse>,
    fetched_at: Option<Instant>,
    error: Option<String>,
    /// One-shot message, cleared by the next successful refresh.
    notice: Option<String>,
    /// True while a `cswap list` call is in flight, so requests coalesce.
    /// Only ever touched while the mutex is held.
    busy: bool,
}

impl State {
    fn view(&self) -> MenuModel {
        menu::build(&View {
            snapshot: self.snapshot.as_ref(),
            age_secs: self.fetched_at.map(|t| t.elapsed().as_secs()),
            error: self.error.as_deref(),
            notice: self.notice.as_deref(),
        })
    }
}

type Shared = Arc<Mutex<State>>;

fn main() {
    let state: Shared = Arc::new(Mutex::new(State {
        exe: cswap::find_exe(),
        snapshot: None,
        fetched_at: None,
        error: None,
        notice: None,
        busy: false,
    }));

    if state.lock().unwrap().exe.is_none() {
        state.lock().unwrap().error = Some(CswapError::NotFound.summary());
    }

    let for_setup = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch just nudges the running instance to refresh.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Some(state) = app.try_state::<Shared>() {
                    refresh(&app, &state.inner().clone());
                }
            });
        }))
        .setup(move |app| {
            app.manage(for_setup.clone());

            let menu = build_menu(app.handle(), &for_setup.lock().unwrap().view())?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png"))
                .tooltip("Claude Swap Bar")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .build(app)?;

            let handle = app.handle().clone();
            tray.on_menu_event(move |app, event| {
                on_menu_event(app, event.id().as_ref().to_string());
            });

            // First refresh immediately, then every REFRESH_INTERVAL.
            let loop_handle = handle.clone();
            let loop_state = for_setup.clone();
            std::thread::spawn(move || loop {
                refresh(&loop_handle, &loop_state);
                std::thread::sleep(REFRESH_INTERVAL);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Claude Swap Bar");
}

/// Rebuild the native menu from a `MenuModel`.
fn build_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(app);

    if let Some(notice) = model.notice.as_ref() {
        let item = MenuItem::with_id(app, "notice", notice, false, None::<&str>)?;
        builder = builder.item(&item).separator();
    }

    let mut rows = Vec::new();
    for row in &model.accounts {
        rows.push(CheckMenuItem::with_id(
            app,
            &row.id,
            &row.label,
            row.enabled,
            row.checked,
            None::<&str>,
        )?);
    }
    for row in &rows {
        builder = builder.item(row);
    }
    if !rows.is_empty() {
        builder = builder.separator();
    }

    let autostart_on = claude_swap_bar::autostart::is_enabled();
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;

    builder
        .text("rotate", "Rotate")
        .text("best", "Best Quota")
        .separator()
        .text("refresh", &model.refresh_label)
        .text("dashboard", "Open Dashboard")
        .item(&autostart)
        .separator()
        .text("quit", "Quit")
        .build()
}

/// Push the current state into the tray icon: menu plus tooltip.
fn render(app: &AppHandle, state: &Shared) {
    let model = state.lock().unwrap().view();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(tray) = app.tray_by_id("main") {
            if let Ok(menu) = build_menu(&app, &model) {
                let _ = tray.set_menu(Some(menu));
            }
            let _ = tray.set_tooltip(Some(&model.tooltip));
        }
    });
}

/// Fetch a fresh snapshot. Returns immediately if one is already in flight.
fn refresh(app: &AppHandle, state: &Shared) {
    let exe = {
        let mut guard = state.lock().unwrap();
        if guard.busy {
            return;
        }
        guard.busy = true;
        guard.exe.clone()
    };

    let outcome = match exe {
        Some(exe) => cswap::list(&exe),
        None => Err(CswapError::NotFound),
    };

    {
        let mut guard = state.lock().unwrap();
        match outcome {
            Ok(list) => {
                guard.snapshot = Some(list);
                guard.fetched_at = Some(Instant::now());
                guard.error = None;
                guard.notice = None;
            }
            Err(err) => {
                guard.error = Some(err.summary());
            }
        }
        guard.busy = false;
    }

    render(app, state);
}

fn on_menu_event(app: &AppHandle, id: String) {
    let Some(state) = app.try_state::<Shared>().map(|s| s.inner().clone()) else {
        return;
    };
    let app = app.clone();

    match id.as_str() {
        "quit" => app.exit(0),
        "notice" => {}
        "refresh" => {
            std::thread::spawn(move || refresh(&app, &state));
        }
        "rotate" => spawn_switch(app, state, Switch::Rotate),
        "best" => spawn_switch(app, state, Switch::Best),
        "dashboard" => {
            // Filled in by Task 7.
        }
        "autostart" => {
            // Filled in by Task 6.
        }
        other => {
            if let Some(num) = other.strip_prefix("acct:").and_then(|n| n.parse().ok()) {
                spawn_switch(app, state, Switch::Account(num));
            }
        }
    }
}

fn spawn_switch(app: AppHandle, state: Shared, target: Switch) {
    std::thread::spawn(move || {
        let exe = state.lock().unwrap().exe.clone();
        let result = match exe {
            Some(exe) => cswap::switch(&exe, target),
            None => Err(CswapError::NotFound),
        };
        if let Err(err) = result {
            state.lock().unwrap().notice = Some(format!("전환 실패: {}", err.summary()));
            render(&app, &state);
            return;
        }
        refresh(&app, &state);
    });
}
```

- [ ] **Step 2: Stub `autostart::is_enabled` so this compiles**

Task 6 writes the real module. For now put this in `src/autostart.rs`, replacing the placeholder comment:

```rust
/// Replaced by the real registry implementation in Task 6.
pub fn is_enabled() -> bool {
    false
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. If `run_on_main_thread` reports a lifetime error, the closure is capturing `model` by reference; make sure `model` is moved (it is a `MenuModel`, which is `Send`).

- [ ] **Step 4: Verify the existing suites still pass**

Run: `cargo test`
Expected: PASS. Tasks 2, 3, and 4 tests all still green.

- [ ] **Step 5: Manual smoke test**

Run: `cargo run`

Expected, checked one at a time:

1. Within a couple of seconds the tooltip changes from `Claude Swap Bar` to the active account's name and usage numbers.
2. The menu lists every account from `cswap list`, with a check on the active one.
3. Clicking a non-active account switches to it. Within a few seconds the check mark moves and the tooltip changes. Confirm independently with `cswap status` in a terminal.
4. `Rotate` moves to the next account.
5. `Best Quota` moves to the account with the most headroom.
6. `Refresh now` shows a relative time that resets to `방금 갱신` after clicking.
7. No console window flashes at any point.
8. `Quit` exits.

- [ ] **Step 6: Verify the failure path**

Temporarily rename the executable so it cannot be found:

```bash
mv ~/.local/bin/cswap.exe ~/.local/bin/cswap.exe.hidden
cargo run
```

Expected: the tooltip reads `cswap을 찾을 수 없음` and the menu's top line says the same. No panic, no empty tray icon.

Restore it:

```bash
mv ~/.local/bin/cswap.exe.hidden ~/.local/bin/cswap.exe
```

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/autostart.rs
git commit -m "feat: refresh account state on a timer and switch from the menu"
```

---

### Task 6: Start with Windows

**Files:**
- Modify: `src/autostart.rs` (replace the Task 5 stub)
- Modify: `src/main.rs` (wire the `autostart` menu id)
- Test: unit tests inside `src/autostart.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces:
  - `autostart::VALUE_NAME: &str = "ClaudeSwapBar"`
  - `autostart::is_enabled() -> bool`
  - `autostart::set(enabled: bool) -> std::io::Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to `src/autostart.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The registry is global to the user, so this test restores whatever it
    /// found. Running it does not permanently change the user's autostart.
    #[test]
    fn toggling_round_trips_through_the_registry() {
        let original = is_enabled();

        set(true).expect("enabling should succeed");
        assert!(is_enabled(), "should read back as enabled");

        set(false).expect("disabling should succeed");
        assert!(!is_enabled(), "should read back as disabled");

        set(original).expect("restoring should succeed");
        assert_eq!(is_enabled(), original);
    }

    #[test]
    fn disabling_an_absent_value_is_not_an_error() {
        let original = is_enabled();
        set(false).expect("first disable");
        set(false).expect("disabling twice must be idempotent");
        set(original).expect("restore");
    }

    #[test]
    fn the_stored_command_is_the_current_executable_in_quotes() {
        let original = is_enabled();
        set(true).unwrap();

        let stored = stored_command().expect("value should exist");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(stored, format!("\"{}\"", exe.display()));

        set(original).unwrap();
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test autostart`
Expected: FAIL with `cannot find function set in this scope` and `cannot find function stored_command in this scope`.

- [ ] **Step 3: Write the implementation**

Replace the stub at the top of `src/autostart.rs`:

```rust
//! Windows "start with the user's session" toggle.
//!
//! The presence of the registry value *is* the setting, so the app keeps no
//! configuration file of its own.

use std::io;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub const VALUE_NAME: &str = "ClaudeSwapBar";

fn open(access: u32) -> io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, access)
}

/// The command line currently registered, if any.
pub fn stored_command() -> Option<String> {
    open(KEY_READ).ok()?.get_value(VALUE_NAME).ok()
}

pub fn is_enabled() -> bool {
    stored_command().is_some()
}

/// Register or unregister the current executable.
///
/// Removing a value that is already absent is treated as success so the menu
/// toggle is idempotent.
pub fn set(enabled: bool) -> io::Result<()> {
    let key = open(KEY_READ | KEY_WRITE)?;
    if enabled {
        let exe = std::env::current_exe()?;
        // Quoted so a path containing spaces survives the shell.
        key.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()))
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test autostart -- --test-threads=1`
Expected: PASS, 3 tests. Single-threaded because all three tests mutate the same registry value.

- [ ] **Step 5: Wire the menu item**

In `src/main.rs`, replace the `"autostart"` arm of `on_menu_event`:

```rust
        "autostart" => {
            let want = !claude_swap_bar::autostart::is_enabled();
            if let Err(err) = claude_swap_bar::autostart::set(want) {
                state.lock().unwrap().notice =
                    Some(format!("시작프로그램 설정 실패: {err}"));
            }
            render(&app, &state);
        }
```

- [ ] **Step 6: Manual verification**

Run: `cargo run`

1. Open the menu. `Start with Windows` is unchecked.
2. Click it. Reopen the menu: it is now checked.
3. In a terminal, run `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v ClaudeSwapBar`. Expected: the value exists and holds the quoted path to the built executable.
4. Click the item again. The value is gone from the registry.

Note the registered path points at `target\debug\claude-swap-bar.exe` during development. Users toggling it from an installed build get the installed path, because the value is written from `current_exe()` at click time.

- [ ] **Step 7: Commit**

```bash
git add src/autostart.rs src/main.rs
git commit -m "feat: toggle start-with-Windows through the Run registry key"
```

---

### Task 7: Open Dashboard and release packaging

**Files:**
- Modify: `src/main.rs` (the `dashboard` menu id)
- Modify: `src/cswap.rs` (add `open_dashboard`)
- Create: `README.md`
- Modify: `tests/cswap_process.rs` (one new test)
- Test: `tests/cswap_process.rs` plus a manual packaging check

**Interfaces:**
- Consumes: `cswap::find_exe`
- Produces: `cswap::open_dashboard(exe: &Path) -> Result<(), CswapError>`

- [ ] **Step 1: Write the failing test**

Append to `tests/cswap_process.rs`:

```rust
#[test]
fn open_dashboard_spawns_without_waiting_for_the_child() {
    // "hang" sleeps for a minute. If open_dashboard waited on the child this
    // call would block; it must return immediately.
    let started = Instant::now();
    with_mode("hang", || cswap::open_dashboard(&fake()))
        .expect("spawning should succeed");
    assert!(
        started.elapsed().as_secs() < 5,
        "open_dashboard blocked for {:?}",
        started.elapsed()
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test cswap_process open_dashboard`
Expected: FAIL with `cannot find function open_dashboard in module cswap`.

- [ ] **Step 3: Implement `open_dashboard`**

Append to `src/cswap.rs`:

```rust
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test cswap_process`
Expected: PASS, 11 tests.

- [ ] **Step 5: Wire the menu item**

In `src/main.rs`, replace the `"dashboard"` arm of `on_menu_event`:

```rust
        "dashboard" => {
            let exe = state.lock().unwrap().exe.clone();
            if let Some(exe) = exe {
                if let Err(err) = cswap::open_dashboard(&exe) {
                    state.lock().unwrap().notice =
                        Some(format!("대시보드 실행 실패: {}", err.summary()));
                    render(&app, &state);
                }
            }
        }
```

- [ ] **Step 6: Write `README.md`**

```markdown
# Claude Swap Bar for Windows

A Windows system tray app for [claude-swap](https://github.com/realiti4/claude-swap)
(`cswap`), the multi-account switcher for Claude Code. It shows the active
account's rate-limit usage in the tray tooltip and switches accounts from the
tray menu.

Design inspired by the macOS [claude-swap-bar](https://github.com/jx-grxf/claude-swap-bar).
This app takes a different approach internally: rather than reimplementing
credential storage and usage polling, it wraps the `cswap` CLI, so it stays
correct as `cswap` evolves.

## Requirements

- Windows 10 or 11
- `cswap` on `PATH`, or installed at `%USERPROFILE%\.local\bin\cswap.exe`
- At least one account registered with `cswap add`

## Install

Download the installer from the releases page, or build from source:

```
cargo tauri build
```

The installer lands in `target/release/bundle/nsis/`.

## Use

The tray icon's menu lists every managed account with its 5-hour and 7-day
usage. Click one to switch. `Rotate` moves to the next account; `Best Quota`
jumps to the one with the most headroom.

Usage refreshes every five minutes and whenever you click `Refresh now`.
`Open Dashboard` launches `cswap tui` in a new console window.

`Start with Windows` registers the app under the current user's `Run` key.

## Development

```
cargo test          # unit and integration tests
cargo run           # run the tray app
python tools/make_icon.py icons   # regenerate the icon assets
```

## Limitations

The tray icon is static. Usage and error state appear only in the tooltip and
the menu.
```

- [ ] **Step 7: Build the release bundle**

Run: `cargo tauri build`

If the Tauri CLI is not installed, install it first:

```bash
cargo install tauri-cli --version "^2"
```

Expected: an NSIS installer under `target/release/bundle/nsis/` and a standalone `target/release/claude-swap-bar.exe`.

- [ ] **Step 8: Verify the packaged build**

1. Run `target/release/claude-swap-bar.exe` directly. The tray icon appears with no console window.
2. Check the executable's icon in Explorer: it shows the orange double-arrow, not the default Rust or Tauri icon.
3. Install from the NSIS installer, launch from the Start menu, and confirm switching works.
4. Uninstall.

- [ ] **Step 9: Commit**

```bash
git add src/cswap.rs src/main.rs tests/cswap_process.rs README.md
git commit -m "feat: open the cswap dashboard and document the release build"
```

---

## Plan Self-Review

Checked against `docs/superpowers/specs/2026-08-27-claude-swap-bar-windows-design.md`:

| Spec section | Covered by |
| --- | --- |
| 1.1 v1 scope | Tasks 1, 5, 6, 7 |
| 2 Tech stack | Task 1 |
| 3 CLI wrapper architecture | Task 3 |
| 3.1 cswap commands used | Task 3 (`list`, `switch`, `switch --strategy best`), Task 7 (`tui`) |
| 4 Module structure | Task 1 (`main`), 2 (`model`), 3 (`cswap`), 4 (`menu`), 6 (`autostart`) |
| 10 Test count expectations | Task 2: 9, Task 3: 10 (11 after Task 7), Task 4: 15, Task 6: 3 |
| 4.1 Module contracts | Task 3 (exe lookup, `CREATE_NO_WINDOW`, 10s timeout), Task 2 (optional fields), Task 4 (pure conversion), Task 6 (registry) |
| 5 Data flow, refresh coalescing | Task 5 (`busy` flag, `run_on_main_thread`) |
| 6 Static icon | Task 1 (`include_image!`, never `set_icon`) |
| 6.1 Tooltip layout and 127-char cap | Task 4 |
| 7 Menu layout, aliases, disabled accounts | Task 4 (model) and Task 5 (native menu) |
| 8 Failure states | Task 3 (error taxonomy), Task 4 (notice priority), Task 5 (manual failure-path check) |
| 9 No config file, single instance | Task 5 (single-instance plugin), Task 6 (registry as the only persisted state) |
| 10 Test strategy | Tasks 2, 3, 4, 6 automated; Tasks 1, 5, 6, 7 manual QA |
| 11 Build and packaging | Task 7 |
| 12 v2 roadmap | Out of scope by design |

Type consistency: `Switch::args()` returns `Vec<String>` and is asserted against string literals in Task 3 with the noted fallback form. `menu::build` takes `&View` and returns `MenuModel` in both Task 4 and Task 5. `autostart::is_enabled()` is stubbed in Task 5 Step 2 and replaced in Task 6 Step 3 with the same signature. `CswapError::summary()` is defined in Task 3 and consumed in Tasks 5 and 7.

Known ordering wrinkle: Task 3 introduces `src/lib.rs` and placeholder `src/menu.rs` and `src/autostart.rs` files so the crate compiles before Tasks 4 and 6 fill them in. This is deliberate — Rust integration tests under `tests/` require a library target, and adding one later would force rework in every earlier task.
