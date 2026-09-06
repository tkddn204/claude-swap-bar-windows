//! Windows "start with the user's session" toggle.
//!
//! The presence of the registry value *is* the setting, so the app keeps no
//! configuration file of its own.

use std::io;
use std::path::Path;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub const VALUE_NAME: &str = "ClaudeSwapBar";

fn open(access: u32) -> io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, access)
}

/// The command line currently registered, if any.
pub fn stored_command() -> Option<String> {
    read_command(&open(KEY_READ).ok()?)
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
    let exe = enabled.then(std::env::current_exe).transpose()?;
    write_command(&key, exe.as_deref())
}

fn read_command(key: &RegKey) -> Option<String> {
    key.get_value(VALUE_NAME).ok()
}

/// The key is supplied explicitly so tests never need the real Run key.
fn write_command(key: &RegKey, exe: Option<&Path>) -> io::Result<()> {
    if let Some(exe) = exe {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Each test owns a disposable key outside Windows startup settings.
    struct TestKey {
        key: RegKey,
        path: String,
    }

    impl TestKey {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = format!(
                r"Software\ClaudeSwapBarTests\{}-{}-{}",
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
            // A forced process exit can leave this harmless test key behind,
            // but it cannot register a program to run at the next login.
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.path);
        }
    }

    #[test]
    fn toggling_round_trips_through_the_registry() {
        let test = TestKey::new();
        assert_eq!(read_command(&test.key), None);
        write_command(&test.key, Some(Path::new(r"C:\Apps\tray.exe"))).unwrap();
        assert_eq!(
            read_command(&test.key).as_deref(),
            Some(r#""C:\Apps\tray.exe""#)
        );
        write_command(&test.key, None).unwrap();
        assert_eq!(read_command(&test.key), None);
    }

    #[test]
    fn disabling_an_absent_value_is_not_an_error() {
        let test = TestKey::new();
        write_command(&test.key, None).unwrap();
        write_command(&test.key, None).unwrap();
        assert_eq!(read_command(&test.key), None);
    }

    #[test]
    fn paths_with_spaces_and_korean_characters_are_quoted() {
        let test = TestKey::new();
        let exe = Path::new(r"C:\사용자\내 앱\tray.exe");
        write_command(&test.key, Some(exe)).unwrap();
        assert_eq!(
            read_command(&test.key).as_deref(),
            Some(r#""C:\사용자\내 앱\tray.exe""#)
        );
    }

    #[test]
    fn changing_one_key_leaves_other_keys_and_values_untouched() {
        let first = TestKey::new();
        let second = TestKey::new();
        first.key.set_value("OtherApplication", &"keep me").unwrap();
        write_command(&first.key, Some(Path::new(r"C:\first.exe"))).unwrap();
        write_command(&second.key, Some(Path::new(r"C:\second.exe"))).unwrap();
        write_command(&first.key, None).unwrap();
        assert_eq!(read_command(&first.key), None);
        assert_eq!(
            read_command(&second.key).as_deref(),
            Some(r#""C:\second.exe""#)
        );
        assert_eq!(
            first
                .key
                .get_value::<String, _>("OtherApplication")
                .unwrap(),
            "keep me"
        );
    }

    #[test]
    fn registry_write_errors_are_returned_to_the_caller() {
        let test = TestKey::new();
        write_command(&test.key, Some(Path::new(r"C:\original.exe"))).unwrap();
        let read_only = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(&test.path, KEY_READ)
            .unwrap();
        assert_eq!(
            write_command(&read_only, Some(Path::new(r"C:\replacement.exe")))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            write_command(&read_only, None).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            read_command(&test.key).as_deref(),
            Some(r#""C:\original.exe""#)
        );
    }
}
