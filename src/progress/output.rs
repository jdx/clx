//! Output mode configuration for progress display.

use std::sync::Mutex;

use super::state::env_text_mode;

/// Output mode for progress display.
///
/// Controls how progress jobs are rendered to the terminal.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProgressOutput {
    /// Rich terminal UI with animated spinners and in-place updates.
    ///
    /// This is the default mode. Progress is rendered with ANSI escape codes for
    /// colors and cursor movement, allowing smooth animation and in-place updates.
    UI,
    /// Simple text output for non-interactive environments.
    ///
    /// In this mode, each update is printed as a new line without ANSI escape codes
    /// or cursor manipulation. Use this for CI systems, log files, or when stdout/stderr
    /// is not a terminal.
    Text,
}

static OUTPUT: Mutex<ProgressOutput> = Mutex::new(ProgressOutput::UI);

/// Sets the output mode for progress display.
///
/// This should be called before starting any progress jobs.
///
/// # Examples
///
/// ```rust,no_run
/// use clx::progress::{set_output, ProgressOutput};
///
/// // Use text mode for CI environments
/// if std::env::var("CI").is_ok() {
///     set_output(ProgressOutput::Text);
/// }
/// ```
pub fn set_output(output: ProgressOutput) {
    *OUTPUT.lock().unwrap() = output;
}

/// Returns the current output mode.
///
/// If `CLX_TEXT_MODE=1` environment variable is set, this always returns
/// [`ProgressOutput::Text`] regardless of what was set via [`set_output`].
#[must_use]
pub fn output() -> ProgressOutput {
    // Environment variable takes precedence
    if env_text_mode() {
        return ProgressOutput::Text;
    }
    *OUTPUT.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_get_set() {
        let original = output();

        set_output(ProgressOutput::Text);
        assert_eq!(output(), ProgressOutput::Text);

        set_output(ProgressOutput::UI);
        assert_eq!(output(), ProgressOutput::UI);

        set_output(original);
    }
}
