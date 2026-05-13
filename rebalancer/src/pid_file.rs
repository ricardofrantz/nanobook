//! PID file management for the kill switch.
//!
//! The PID file stores the process ID of a running rebalancer instance.
//! This allows the --kill subcommand to locate and signal the running process.
//!
//! # PID File Format
//!
//! The PID file is a plain text file containing a single integer: the process ID
//! of the running rebalancer. For example:
//!
//! ```text
//! 12345
//! ```
//!
//! # Default Location
//!
//! By default, the PID file is located at `rebalancer.pid` in the working directory.
//! This can be customized by changing the `DEFAULT_PID_FILE` constant.
//!
//! # Lifecycle
//!
//! The PID file should be:
//! 1. Written when the rebalancer starts (before any orders are submitted)
//! 2. Removed when the rebalancer exits cleanly (after all orders are filled/cancelled)
//! 3. Used by the --kill subcommand to locate and signal the running process
//!
//! # Cleanup
//!
//! If the rebalancer crashes or is killed via SIGKILL, the PID file may not be
//! cleaned up automatically. In such cases, the file should be removed manually
//! before starting a new rebalancer instance to avoid conflicts.

use crate::error::{Error, Result};
use std::fs;
use std::path::Path;

/// Default PID file path.
pub const DEFAULT_PID_FILE: &str = "rebalancer.pid";

/// Write the current process ID to a PID file.
///
/// # Errors
///
/// Returns an error if the file cannot be written or if the PID cannot be obtained.
pub fn write_pid_file(path: &Path) -> Result<()> {
    let pid = std::process::id();
    fs::write(path, pid.to_string()).map_err(Error::Audit)?;
    Ok(())
}

/// Read the PID from a PID file.
///
/// # Errors
///
/// Returns an error if the file does not exist, cannot be read, or does not contain a valid PID.
pub fn read_pid_file(path: &Path) -> Result<u32> {
    let contents = fs::read_to_string(path).map_err(Error::Audit)?;
    let pid: u32 = contents
        .trim()
        .parse()
        .map_err(|_| Error::Aborted(format!("Invalid PID in file: {}", contents)))?;
    Ok(pid)
}

/// Check if a PID file exists.
pub fn pid_file_exists(path: &Path) -> bool {
    path.exists()
}

/// Remove the PID file.
///
/// # Errors
///
/// Returns an error if the file cannot be removed.
pub fn remove_pid_file(path: &Path) -> Result<()> {
    fs::remove_file(path).map_err(Error::Audit)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_and_read_pid_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        write_pid_file(path).unwrap();
        assert!(pid_file_exists(path));

        let pid = read_pid_file(path).unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn test_pid_file_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.pid");

        assert!(!pid_file_exists(&path));
        write_pid_file(&path).unwrap();
        assert!(pid_file_exists(&path));
    }

    #[test]
    fn test_remove_pid_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        write_pid_file(path).unwrap();
        assert!(pid_file_exists(path));

        remove_pid_file(path).unwrap();
        assert!(!pid_file_exists(path));
    }

    #[test]
    fn test_read_invalid_pid_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        fs::write(path, "not-a-number").unwrap();
        let result = read_pid_file(path);
        assert!(result.is_err());
    }
}
