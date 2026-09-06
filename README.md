# Claude Swap Bar for Windows

A Windows system tray app for [claude-swap](https://github.com/realiti4/claude-swap)
(`cswap`), the multi-account switcher for Claude Code. It shows the active
account's rate-limit usage in the tray tooltip and switches accounts from the
tray menu.

Design inspired by the macOS [claude-swap-bar](https://github.com/jx-grxf/claude-swap-bar).
This app delegates credential handling and usage retrieval to the cswap CLI.
Compatibility depends on its commands and JSON output; a minimum supported
cswap version has not yet been established.

## Requirements

- Windows 11. Development targets Windows x64 with the MSVC Rust toolchain.
  Windows 10 and ARM64 have not been validated for this project.
- Microsoft Edge WebView2 Runtime; see [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).
- `cswap` on `PATH`, or installed at `%USERPROFILE%\.local\bin\cswap.exe`
- At least one account registered with `cswap add`

Menu actions are English. App-authored status and error messages are Korean;
cswap status codes and error output retain their original language.

## Set up cswap

Install cswap using an existing [uv](https://docs.astral.sh/uv/getting-started/installation/)
installation, following the [upstream instructions](https://github.com/realiti4/claude-swap#installation):

```powershell
uv tool install claude-swap
cswap --version
```

Log into Claude Code with the account you want to register, then run:

```powershell
cswap add
cswap list --json
```

Repeat registration for additional accounts using the upstream instructions.
After installing cswap or changing `PATH`, restart the tray app so it receives
the updated environment. Do not attach real account JSON or credential exports
to public issues.

## Install or build

If this repository has a published release, use the Windows NSIS setup `.exe`
attached to that release. Otherwise, build from source as described below.
Installer users do not need Rust or the Tauri CLI. For unsigned installers,
Windows may show a publisher or SmartScreen warning; check the release's signing
information before installation.

### Build prerequisites

1. Install [Rust through rustup](https://www.rust-lang.org/tools/install), using
   an MSVC toolchain. Rust 1.95.0 is used for the current checks.
2. Install Microsoft C++ Build Tools with **Desktop development with C++** and
   a Windows SDK. Follow the [Tauri Windows setup](https://v2.tauri.app/start/prerequisites/#windows)
   for C++ tools and WebView2.
3. Install the Tauri v2 CLI separately:

   ```powershell
   cargo install tauri-cli --version "^2" --locked
   cargo tauri --version
   ```

Rust and Cargo alone do not install `cargo tauri`. Node.js is not required:
the frontend input is the checked-in `dist/index.html`. Python 3 is only needed
to regenerate icons.

### Build from the repository root

```powershell
cargo build --locked
cargo tauri build -- --locked
```

The first command builds the debug app. The second creates the release app and
NSIS installer under `target/release/bundle/nsis/` for the default target.
The first build needs network access for dependencies and packaging tools.
Keep `Cargo.lock` and `dist/index.html` in your checkout.

## Use

Start the installed app, or run `cargo run --locked` from the repository root.
There is no main window. Check the taskbar's hidden tray icons if the icon is
not visible. Left- or right-click opens the menu; `Quit` exits the app.

The tray icon's menu lists every managed account with its 5-hour and 7-day
usage. Click one to switch. `Rotate` moves to the next account; `Best Quota`
jumps to the one with the most headroom.

Data refreshes at startup, every five minutes, after a successful switch, and
on `Refresh now`. Opening the menu also fetches data, but updates **only the
tooltip**, so the open menu is not replaced. Account rows retain previous values
until a timer, manual refresh, or successful switch rebuilds the menu.

`Open Dashboard` launches `cswap tui` in a new console window. Accounts marked
`(자동 제외)` are excluded from automatic rotation but remain manually selectable.

`Start with Windows` writes the `ClaudeSwapBar` value under
`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.
The NSIS uninstall hook removes it on normal uninstall and preserves it during
installer updates. Actual install/upgrade/removal needs manual release testing.
There is no automatic app updater.

The app has no credential vault or dedicated log file. Account data is held in
memory for display; credential access and usage retrieval are delegated to cswap.

## Development and tests

Run from the repository root on Windows:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests use synthetic account fixtures and a fake cswap executable. No real cswap
installation or Claude account is needed. Autostart tests use unique keys under
`HKEY_CURRENT_USER\Software\ClaudeSwapBarTests`, never the real Windows `Run`
key. Keys are removed on normal exit and unwinding; forced termination may leave
harmless test keys behind.

CLI tests serialize temporary `FAKE_CSWAP_MODE` changes and restore the previous
value on success or panic. The timeout test takes about ten seconds. The
dashboard test may briefly display a console for the fake executable.

`cargo test` builds the fake example. When running only the integration target
from a clean checkout, build that example explicitly first:

```powershell
cargo build --locked --example fake_cswap
cargo test --locked --test cswap_process
```

To regenerate the checked-in icons using Python's standard library:

```powershell
python tools/make_icon.py icons
```

Automated tests do not verify live tray interactions or installer behavior.
See the [manual release checks](docs/manual-testing.md) before distribution.
Cross-platform app builds are not supported.

## Troubleshooting and limitations

The tray icon is static. Usage and error state appear only in the tooltip and
the menu.

- **No tray icon:** check the taskbar overflow. Initial discovery may take ten
  seconds if `cswap --version` stops responding.
- **cswap not found:** verify `cswap --version` in a new terminal, check its path,
  and restart the app. `Install cswap` opens the upstream installation guide.
- **Stale menu values:** use `Refresh now`. Opening the menu refreshes the tooltip
  only; cswap may also return cached usage.
- **Command failure:** verify cswap separately. CLI commands time out after ten
  seconds. Non-UTF-8 Korean output may display replacement characters.
- **Menu closes during refresh:** a timer or completed switch can still replace
  an open menu. Avoid repeated selections while a switch is in progress;
  the app does not serialize switch requests.

For bug reports, include app/Windows/cswap versions, installation method, and
reproduction steps. Remove emails, tokens, and other account details from
screenshots and command output.

## License

Copyright 2026 tkddn <tkddn204@gmail.com>.

Licensed under the [Apache License, Version 2.0](LICENSE).
Third-party dependencies remain under their respective licenses.
