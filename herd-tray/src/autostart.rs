//! "Start at login" toggle.
//!
//! Windows: an `HKCU\...\Run` value. Everywhere else: a no-op that reports
//! unsupported, so the menu item is simply hidden (the caller checks
//! [`supported`]).

#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result};
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "HerdTray";

    pub fn supported() -> bool {
        true
    }

    pub fn is_enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_PATH, KEY_READ)
            .and_then(|k| k.get_value::<String, _>(VALUE_NAME))
            .is_ok()
    }

    pub fn set(enabled: bool) -> Result<()> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_PATH, KEY_WRITE)
            .context("open HKCU Run key for write")?;
        if enabled {
            let exe = std::env::current_exe().context("resolve current_exe")?;
            key.set_value(VALUE_NAME, &exe.to_string_lossy().to_string())
                .context("write Run value")?;
        } else {
            let _ = key.delete_value(VALUE_NAME); // absent is fine
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use anyhow::Result;

    pub fn supported() -> bool {
        false
    }
    pub fn is_enabled() -> bool {
        false
    }
    pub fn set(_enabled: bool) -> Result<()> {
        Ok(())
    }
}

pub use imp::{is_enabled, set, supported};
