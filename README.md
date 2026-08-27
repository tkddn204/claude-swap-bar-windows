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
