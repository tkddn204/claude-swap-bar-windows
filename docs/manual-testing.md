# Manual release checks

Use a disposable Windows user account or VM. Record the app commit/version,
Rust and Tauri CLI versions, cswap version, Windows version, architecture, and
installer filename with the results. Use dedicated accounts for switch tests.

## Installation and startup

- Install the NSIS package as a normal user, including a path with spaces or
  Korean characters. Confirm a tray icon appears without a main window.
- Launch a second instance and confirm that only one tray icon remains.
- Enable Start with Windows, sign out and in, and confirm automatic launch.
  Disable it and verify the app no longer starts on login.
- Upgrade with automatic launch enabled. Confirm it remains enabled and the
  Run value points to the installed executable.
- Uninstall normally, including while the app is running. Confirm that the
  current user's Run key no longer contains `ClaudeSwapBar`.
- Verify that the installed directory does not contain `fake_cswap.exe`.

## CLI integration and menu behavior

- Start without cswap installed. Confirm a visible error and a working Install
  cswap link. Install cswap, restart the app, and verify discovery succeeds.
- Check empty-account and multiple-account states with dedicated test accounts.
- Switch to an account, use Rotate and Best Quota, and compare with cswap.
  Verify a disabled account remains manually selectable.
- Open Dashboard and confirm a usable TUI console appears.
- Exercise command failure and a response exceeding ten seconds. Confirm the
  app stays responsive and displays the failure.
- Compare menu and tooltip values after menu opening and after Refresh now.
  Opening alone refreshes the tooltip; account rows update on a rebuild.
- Hold the menu open across a five-minute refresh, and reopen it while an
  account switch runs. Record popup closure or lost clicks as known risks.

## Release evidence

- Run formatting, Clippy, and the full test suite from a fresh checkout.
- Build the installer and record its SHA-256 hash and signing status.
- Claim only OS and architecture combinations actually checked.
- Redact account details from screenshots and logs. Do not publish credential
  exports, real account JSON, or local Git history backups.
