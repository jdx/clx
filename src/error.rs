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
