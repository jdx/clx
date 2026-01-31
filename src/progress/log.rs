//! Integration with the `log` crate for seamless logging during progress display.
//!
//! When enabled (via the `log` feature), this module provides a logger that
//! automatically pauses progress display before writing log messages and resumes
//! it afterward. This prevents log output from being overwritten by progress updates.
//!
//! # Example
//!
//! ```rust,ignore
//! use clx::progress::{ProgressJobBuilder, ProgressStatus, init_log_integration};
//! use log::{info, warn};
//!
//! // Initialize the log integration (call once at startup)
//! init_log_integration();
//!
//! let job = ProgressJobBuilder::new()
//!     .prop("message", "Working...")
//!     .start();
//!
//! // Log messages are automatically interleaved with progress
//! info!("Starting processing");
//! // ... do work ...
//! warn!("Something unexpected happened");
//!
//! job.set_status(ProgressStatus::Done);
//! ```

use super::state::{STARTED, TERM_LOCK, is_paused, pause, resume};
use crate::style;
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::io::Write;

/// A logger that integrates with the progress display system.
///
/// This logger wraps log output to ensure it doesn't interfere with
/// progress display. Before each log message, progress is paused,
/// and after the message is written, progress resumes.
pub struct ProgressLogger {
    level: LevelFilter,
    target_filter: Option<String>,
}

impl ProgressLogger {
    /// Creates a new progress-aware logger.
    ///
    /// # Arguments
    ///
    /// * `level` - The maximum log level to display
    pub fn new(level: LevelFilter) -> Self {
        Self {
            level,
            target_filter: None,
        }
    }

    /// Creates a new progress-aware logger with a target filter.
    ///
    /// Only log messages whose target starts with the given prefix will be displayed.
    ///
    /// # Arguments
    ///
    /// * `level` - The maximum log level to display
    /// * `target` - Only show logs from targets starting with this prefix
    pub fn with_target(level: LevelFilter, target: impl Into<String>) -> Self {
        Self {
            level,
            target_filter: Some(target.into()),
        }
    }

    /// Installs this logger as the global logger.
    ///
    /// # Errors
    ///
    /// Returns an error if a logger has already been set.
    pub fn init(self) -> Result<(), SetLoggerError> {
        let level = self.level;
        // Set logger first to avoid modifying max level if logger installation fails
        log::set_boxed_logger(Box::new(self))?;
        log::set_max_level(level);
        Ok(())
    }

    fn format_message(&self, record: &Record) -> String {
        let level_str = match record.level() {
            Level::Error => style::ered("ERROR").to_string(),
            Level::Warn => style::eyellow("WARN").to_string(),
            Level::Info => style::ecyan("INFO").to_string(),
            Level::Debug => style::edim("DEBUG").to_string(),
            Level::Trace => style::edim("TRACE").to_string(),
        };
        format!("{} {}", level_str, record.args())
    }
}

impl Log for ProgressLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if metadata.level() > self.level {
            return false;
        }
        if let Some(ref filter) = self.target_filter {
            metadata.target().starts_with(filter)
        } else {
            true
        }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = self.format_message(record);
        let was_paused = is_paused();

        // Pause progress if it's running
        // Track whether we actually paused to avoid race condition
        let did_pause = if !was_paused && *STARTED.lock().unwrap() {
            pause();
            true
        } else {
            false
        };

        // Write the log message with terminal lock held
        {
            let _guard = TERM_LOCK.lock().unwrap();
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{}", message);
        }

        // Resume progress only if we paused it
        if did_pause {
            resume();
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// Initializes the progress-aware logger with the default log level (Info).
///
/// This sets up a global logger that automatically pauses progress display
/// before writing log messages and resumes it afterward.
///
/// # Panics
///
/// Panics if a logger has already been initialized.
///
/// # Example
///
/// ```rust,ignore
/// use clx::progress::init_log_integration;
///
/// init_log_integration();
/// log::info!("This won't interfere with progress display!");
/// ```
pub fn init_log_integration() {
    ProgressLogger::new(LevelFilter::Info)
        .init()
        .expect("Failed to initialize logger - another logger may already be set");
}

/// Initializes the progress-aware logger with a custom log level.
///
/// # Arguments
///
/// * `level` - The maximum log level to display
///
/// # Panics
///
/// Panics if a logger has already been initialized.
pub fn init_log_integration_with_level(level: LevelFilter) {
    ProgressLogger::new(level)
        .init()
        .expect("Failed to initialize logger - another logger may already be set");
}

/// Tries to initialize the progress-aware logger, returning an error on failure.
///
/// Use this instead of [`init_log_integration`] if you want to handle the case
/// where another logger has already been set.
///
/// # Errors
///
/// Returns an error if a logger has already been set.
pub fn try_init_log_integration() -> Result<(), SetLoggerError> {
    ProgressLogger::new(LevelFilter::Info).init()
}

/// Tries to initialize the progress-aware logger with a custom level.
///
/// # Errors
///
/// Returns an error if a logger has already been set.
pub fn try_init_log_integration_with_level(level: LevelFilter) -> Result<(), SetLoggerError> {
    ProgressLogger::new(level).init()
}
