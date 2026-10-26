//! App preferences kept under `HKCU\Software\ClaudeSwapBar`.
//!
//! Registry rather than a file, for the same reason as `autostart`: it needs no
//! path, survives updates, and the uninstaller can remove it in one line.

use std::io;

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

pub const KEY: &str = r"Software\ClaudeSwapBar";

pub const USAGE_ICON: &str = "ShowUsageIcon";

/// Whether to draw usage on the tray icon. On until the user turns it off.
pub fn usage_icon() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(KEY)
        .map_or(true, |key| read_usage_icon(&key))
}

pub fn set_usage_icon(on: bool) -> io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(KEY)?;
    write_usage_icon(&key, on)
}

/// A missing or unreadable value reads as the default, on.
fn read_usage_icon(key: &RegKey) -> bool {
    key.get_value::<u32, _>(USAGE_ICON).map_or(true, |v| v != 0)
}

/// The key is supplied explicitly so tests never touch the real one.
fn write_usage_icon(key: &RegKey, on: bool) -> io::Result<()> {
    key.set_value(USAGE_ICON, &u32::from(on))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Each test owns a disposable key apart from the app's real settings.
    struct TestKey {
        key: RegKey,
        path: String,
    }

    impl TestKey {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = format!(
                r"Software\ClaudeSwapBarTests\settings-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(&path)
                .expect("create isolated test key");
            Self { key, path }
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.path);
        }
    }

    #[test]
    fn an_absent_value_means_on() {
        let test = TestKey::new();
        assert!(read_usage_icon(&test.key));
    }

    #[test]
    fn the_setting_round_trips() {
        let test = TestKey::new();
        write_usage_icon(&test.key, false).unwrap();
        assert!(!read_usage_icon(&test.key));
        write_usage_icon(&test.key, true).unwrap();
        assert!(read_usage_icon(&test.key));
    }

    #[test]
    fn a_value_of_the_wrong_type_reads_as_on() {
        let test = TestKey::new();
        test.key.set_value(USAGE_ICON, &"off").unwrap();
        assert!(read_usage_icon(&test.key));
    }
}
