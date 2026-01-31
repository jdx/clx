//! Error types for the clx library.
//!
//! This module provides the [`Error`] enum and [`Result`] type alias used
//! throughout the library for error handling.

use std::process::ExitStatus;
use thiserror::Error;

/// Error type for clx operations.
///
/// This enum captures all possible errors that can occur when using the clx library,
/// including I/O errors, template rendering errors, and script execution failures.
#[derive(Error, Debug)]
pub enum Error {
    /// An I/O error occurred (e.g., writing to terminal).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An error occurred while joining paths.
    #[error(transparent)]
    JoinPaths(#[from] std::env::JoinPathsError),

    /// A Unix-specific error occurred (e.g., signal handling).
    #[cfg(unix)]
    #[error(transparent)]
    Nix(#[from] nix::errno::Errno),

    /// A template rendering error occurred.
    ///
    /// This happens when a Tera template in a progress job body has invalid syntax
    /// or references undefined variables.
    #[error(transparent)]
    Tera(#[from] tera::Error),

    /// A script or command exited with a non-zero status.
    ///
    /// The first field is the script/command name, and the second is the exit status
    /// (which may be `None` if the process was killed by a signal).
    #[error("{} exited with non-zero status: {}", .0, render_exit_status(.1))]
    ScriptFailed(String, Option<ExitStatus>),
}

/// A specialized `Result` type for clx operations.
///
/// This is defined as `std::result::Result<T, clx::Error>` for convenience.
pub type Result<T> = std::result::Result<T, Error>;

fn render_exit_status(exit_status: &Option<ExitStatus>) -> String {
    match exit_status.and_then(|s| s.code()) {
        Some(exit_status) => format!("exit code {exit_status}"),
        None => "no exit status".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, ErrorKind};

    #[test]
    fn test_error_from_io_error() {
        let io_error = io::Error::new(ErrorKind::NotFound, "file not found");
        let error: Error = io_error.into();

        assert!(matches!(error, Error::Io(_)));
        assert!(error.to_string().contains("file not found"));
    }

    #[test]
    fn test_error_io_transparent() {
        // Verify transparent error message pass-through
        let io_error = io::Error::new(ErrorKind::PermissionDenied, "access denied");
        let error: Error = io_error.into();

        // The error message should be the underlying io error message
        let msg = error.to_string();
        assert!(
            msg.contains("access denied"),
            "Expected 'access denied' in: {}",
            msg
        );
    }

    #[test]
    fn test_error_from_tera_error() {
        // Create a Tera error by trying to parse invalid template syntax
        let mut tera = tera::Tera::default();
        let tera_result = tera.add_raw_template("test", "{{ invalid syntax }");
        assert!(tera_result.is_err());

        let error: Error = tera_result.unwrap_err().into();
        assert!(matches!(error, Error::Tera(_)));
    }

    #[test]
    fn test_script_failed_with_exit_code() {
        // Create a ScriptFailed error with an exit code
        // We need to use Command to get a real ExitStatus
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 42")
            .output()
            .expect("failed to execute command");

        let error = Error::ScriptFailed("test_script".to_string(), Some(output.status));
        let msg = error.to_string();

        assert!(
            msg.contains("test_script"),
            "Expected script name in: {}",
            msg
        );
        assert!(
            msg.contains("exit code 42"),
            "Expected exit code 42 in: {}",
            msg
        );
        assert!(
            msg.contains("exited with non-zero status"),
            "Expected 'exited with non-zero status' in: {}",
            msg
        );
    }

    #[test]
    fn test_script_failed_no_exit_status() {
        // ScriptFailed with None exit status (e.g., killed by signal)
        let error = Error::ScriptFailed("killed_script".to_string(), None);
        let msg = error.to_string();

        assert!(
            msg.contains("killed_script"),
            "Expected script name in: {}",
            msg
        );
        assert!(
            msg.contains("no exit status"),
            "Expected 'no exit status' in: {}",
            msg
        );
    }

    #[test]
    fn test_render_exit_status_with_code() {
        // Test the helper function with an exit code
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 1")
            .output()
            .expect("failed to execute command");

        let result = render_exit_status(&Some(output.status));
        assert_eq!(result, "exit code 1");
    }

    #[test]
    fn test_render_exit_status_none() {
        let result = render_exit_status(&None);
        assert_eq!(result, "no exit status");
    }

    #[test]
    fn test_error_debug_impl() {
        let error = Error::ScriptFailed("debug_test".to_string(), None);
        let debug_str = format!("{:?}", error);

        assert!(
            debug_str.contains("ScriptFailed"),
            "Expected 'ScriptFailed' in debug output: {}",
            debug_str
        );
        assert!(
            debug_str.contains("debug_test"),
            "Expected 'debug_test' in debug output: {}",
            debug_str
        );
    }

    #[test]
    fn test_result_type_ok() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        let result = returns_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_result_type_err() {
        fn returns_error() -> Result<i32> {
            Err(Error::ScriptFailed("test".to_string(), None))
        }

        let result = returns_error();
        assert!(result.is_err());
    }

    #[test]
    fn test_error_source_chain() {
        // Test that std::error::Error trait is implemented
        use std::error::Error as StdError;

        let io_error = io::Error::new(ErrorKind::NotFound, "underlying error");
        let error: Error = io_error.into();

        // Verify error implements std::error::Error trait
        let _: &dyn StdError = &error;

        // The error should be displayable
        let msg = error.to_string();
        assert!(!msg.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_error_from_nix_errno() {
        use nix::errno::Errno;

        let error: Error = Errno::ENOENT.into();
        assert!(matches!(error, Error::Nix(_)));

        let msg = error.to_string();
        // Errno messages vary by platform, just check it's not empty
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_script_failed_special_characters() {
        // Test with special characters in script name
        let error = Error::ScriptFailed("/path/to/script with spaces.sh".to_string(), None);
        let msg = error.to_string();

        assert!(
            msg.contains("/path/to/script with spaces.sh"),
            "Expected script path in: {}",
            msg
        );
    }

    #[test]
    fn test_script_failed_empty_name() {
        // Edge case: empty script name
        let error = Error::ScriptFailed(String::new(), None);
        let msg = error.to_string();

        assert!(
            msg.contains("exited with non-zero status"),
            "Expected error message in: {}",
            msg
        );
    }

    #[test]
    fn test_multiple_io_error_kinds() {
        // Test various IO error kinds convert properly
        let error_kinds = [
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::ConnectionRefused,
            ErrorKind::TimedOut,
            ErrorKind::InvalidInput,
        ];

        for kind in error_kinds {
            let io_error = io::Error::new(kind, "test error");
            let error: Error = io_error.into();
            assert!(matches!(error, Error::Io(_)));
        }
    }
}
