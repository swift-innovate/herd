//! Child-gateway supervision.
//!
//! When the gateway is not already running, the tray launches `herd serve` and
//! watches it. The binary is looked up beside the tray exe first (the shipped
//! layout), then left to `PATH`.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Child, Command};

/// Platform file name of the gateway binary.
pub fn herd_binary_name() -> &'static str {
    if cfg!(windows) {
        "herd.exe"
    } else {
        "herd"
    }
}

/// Resolve the gateway binary: prefer one sitting next to the tray exe (the
/// shipped layout), otherwise fall back to a bare name for `PATH` resolution.
pub fn locate_herd() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join(herd_binary_name());
            if beside.is_file() {
                return beside;
            }
        }
    }
    PathBuf::from(herd_binary_name())
}

/// Owns the spawned gateway child (if any) and answers liveness queries.
#[derive(Default)]
pub struct Supervisor {
    child: Option<Child>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn `herd serve`. Replaces any previous child handle.
    pub fn spawn(&mut self) -> Result<()> {
        let path = locate_herd();
        let child = Command::new(&path)
            .arg("serve")
            .spawn()
            .with_context(|| format!("spawn gateway: {}", path.display()))?;
        self.child = Some(child);
        Ok(())
    }

    /// Whether the supervised child is currently running. `false` when we never
    /// spawned one, or it has exited. Reaps the exit status so no zombie lingers.
    pub fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_status)) => {
                    self.child = None; // exited — reap
                    false
                }
                Ok(None) => true, // still running
                Err(_) => false,
            },
            None => false,
        }
    }

    /// Kill the supervised child (best-effort) and reap it.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_matches_platform() {
        let name = herd_binary_name();
        if cfg!(windows) {
            assert_eq!(name, "herd.exe");
        } else {
            assert_eq!(name, "herd");
        }
    }

    #[test]
    fn fresh_supervisor_is_not_alive() {
        let mut s = Supervisor::new();
        assert!(!s.is_alive(), "no child spawned yet");
    }

    #[test]
    fn locate_returns_a_path_ending_in_the_binary_name() {
        let p = locate_herd();
        assert!(p.to_string_lossy().ends_with(herd_binary_name()));
    }
}
