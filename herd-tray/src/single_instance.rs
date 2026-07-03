//! Single-instance guard.
//!
//! Windows: a named mutex `Global\herd-tray`; a second launch sees
//! `ERROR_ALREADY_EXISTS` and [`acquire`] returns `None` (the caller exits 0).
//! Elsewhere: a no-op guard that always acquires.

#[cfg(windows)]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;

    /// Holds the mutex handle for the lifetime of the process.
    pub struct InstanceGuard {
        handle: HANDLE,
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }

    pub fn acquire() -> Option<InstanceGuard> {
        let name: Vec<u16> = "Global\\herd-tray\0".encode_utf16().collect();
        unsafe {
            // `CreateMutexW` returns the existing handle AND sets last-error to
            // ERROR_ALREADY_EXISTS when another instance already created it.
            let handle = CreateMutexW(None, true, PCWSTR(name.as_ptr())).ok()?;
            if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(InstanceGuard { handle })
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// No-op guard on non-Windows platforms.
    pub struct InstanceGuard;

    pub fn acquire() -> Option<InstanceGuard> {
        Some(InstanceGuard)
    }
}

pub use imp::{acquire, InstanceGuard};
