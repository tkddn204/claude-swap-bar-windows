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

    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// These tests read and write the real `Run` key of the real user running
    /// them. There is no sandbox, so two properties have to hold or a test run
    /// can silently change what launches at the user's next login.
    ///
    /// First, only one test may touch the value at a time. Relying on
    /// `--test-threads=1` would leave that to whoever types the command: with
    /// threads, one test can capture its "original" while another has a
    /// mutation in flight, and then restore that wrong value over the user's
    /// real setting. That corrupts rather than merely fails.
    ///
    /// Second, the value must be put back even when an assertion fails partway
    /// through, which is exactly when a straight-line restore call is skipped.
    fn registry_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Holds the lock for the test's whole body and restores the value on drop,
    /// panics included. The lock is released only after the restore, so the
    /// next test always observes the original value.
    struct Restore {
        was_enabled: bool,
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = set(self.was_enabled);
        }
    }

    fn guard_registry() -> Restore {
        // A test that panics poisons the lock; the payload is `()`, so
        // recovering keeps one failure from cascading into the rest.
        let lock = registry_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Restore {
            was_enabled: is_enabled(),
            _lock: lock,
        }
    }

    #[test]
    fn toggling_round_trips_through_the_registry() {
        let _restore = guard_registry();

        set(true).expect("enabling should succeed");
        assert!(is_enabled(), "should read back as enabled");

        set(false).expect("disabling should succeed");
        assert!(!is_enabled(), "should read back as disabled");
    }

    #[test]
    fn disabling_an_absent_value_is_not_an_error() {
        let _restore = guard_registry();

        set(false).expect("first disable");
        set(false).expect("disabling twice must be idempotent");
    }

    #[test]
    fn the_stored_command_is_the_current_executable_in_quotes() {
        let _restore = guard_registry();

        set(true).unwrap();

        let stored = stored_command().expect("value should exist");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(stored, format!("\"{}\"", exe.display()));
    }

    #[test]
    fn the_guard_restores_the_value_after_a_panic() {
        // The guard is what stands between a failing test and the user's real
        // autostart setting, so prove it rather than assuming it. This test
        // does not take a guard of its own: the mutex is not reentrant, and the
        // guard under test acquires it inside the closure.
        let original = {
            let _probe = guard_registry();
            is_enabled()
        };

        // A deliberate panic would otherwise print a backtrace and dirty the
        // test output, which is supposed to stay pristine.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(|| {
            let _guard = guard_registry();
            set(!is_enabled()).expect("flip the value");
            panic!("simulated assertion failure");
        });
        std::panic::set_hook(previous_hook);

        assert!(panicked.is_err(), "the closure was supposed to panic");

        let _restore = guard_registry();
        assert_eq!(
            is_enabled(),
            original,
            "the guard must put back what it found even when the test panics"
        );
    }
}
