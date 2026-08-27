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
