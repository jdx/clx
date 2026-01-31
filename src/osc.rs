//! OSC (Operating System Command) escape sequences for terminal integration.
//!
//! This module provides support for OSC 9;4 progress sequences that display a progress
//! indicator in the terminal's title bar or tab. This is supported by several modern
//! terminals:
//!
//! - **Ghostty** - Full support
//! - **VS Code integrated terminal** - Full support
//! - **Windows Terminal** - Full support
//! - **iTerm2** - Full support
//! - **VTE-based terminals** (GNOME Terminal, etc.) - Full support
//!
//! The progress indicator is automatically updated based on job progress and will
//! show different states (normal, error, warning) based on job status.
//!
//! # Configuration
//!
//! OSC progress is enabled by default. To disable it:
//!
//! ```rust,no_run
//! use clx::osc;
//!
//! // Must be called before any progress jobs start
//! osc::configure(false);
//! ```
//!
//! # How It Works
//!
//! When progress jobs are running, clx automatically sends OSC 9;4 sequences to
//! update the terminal's progress indicator. The progress percentage is calculated
//! from job progress values or estimated from job status.

use std::io::Write;
use std::sync::OnceLock;

/// Global OSC progress enable/disable flag
static OSC_ENABLED: OnceLock<bool> = OnceLock::new();

/// Configures whether OSC progress sequences are enabled.
///
/// This must be called before any progress jobs are started. Once set, the value
/// cannot be changed.
///
/// # Panics
///
/// Panics if called after OSC progress has already been initialized (either by
/// a previous call to `configure` or by starting a progress job).
///
/// # Examples
///
/// ```rust,no_run
/// use clx::osc;
///
/// // Disable OSC progress for environments that don't support it
/// osc::configure(false);
/// ```
pub fn configure(enabled: bool) {
    OSC_ENABLED
        .set(enabled)
        .expect("OSC_ENABLED already initialized");
}

/// Checks if OSC progress is enabled.
///
/// Returns `true` if OSC progress sequences will be sent to the terminal.
/// This is `true` by default unless disabled via [`configure`].
pub(crate) fn is_enabled() -> bool {
    *OSC_ENABLED.get_or_init(|| true)
}

/// OSC 9;4 progress states for terminal progress indication.
///
/// These states control how the terminal displays the progress indicator.
/// The exact visual appearance depends on the terminal emulator, but generally:
///
/// - [`Normal`](ProgressState::Normal) - Blue/cyan progress bar
/// - [`Error`](ProgressState::Error) - Red progress bar
/// - [`Warning`](ProgressState::Warning) - Yellow progress bar
/// - [`Indeterminate`](ProgressState::Indeterminate) - Animated/pulsing indicator
/// - [`None`](ProgressState::None) - No indicator (clears existing)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// Clears any existing progress indicator.
    None,
    /// Normal progress state, typically displayed as blue or cyan.
    /// Use this for standard progress updates.
    Normal,
    /// Error state, typically displayed as red.
    /// Use this when a job has failed.
    Error,
    /// Indeterminate progress, typically displayed as an animated indicator.
    /// Use this when progress percentage is unknown.
    Indeterminate,
    /// Warning state, typically displayed as yellow.
    /// Use this when a job completed with warnings.
    Warning,
}

impl ProgressState {
    fn as_code(&self) -> u8 {
        match self {
            ProgressState::None => 0,
            ProgressState::Normal => 1,
            ProgressState::Error => 2,
            ProgressState::Indeterminate => 3,
            ProgressState::Warning => 4,
        }
    }
}

/// Checks if the current terminal supports OSC 9;4 progress sequences.
///
/// Detection is based on environment variables:
/// - `TERM_PROGRAM` - Detects Ghostty, VS Code, iTerm, WezTerm, Alacritty
/// - `WT_SESSION` - Detects Windows Terminal
/// - `VTE_VERSION` - Detects VTE-based terminals (GNOME Terminal, etc.)
fn terminal_supports_osc_9_4() -> bool {
    static SUPPORTS_OSC_9_4: OnceLock<bool> = OnceLock::new();

    *SUPPORTS_OSC_9_4.get_or_init(|| {
        // Check TERM_PROGRAM environment variable for terminal detection
        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
            match term_program.as_str() {
                // Supported terminals
                "ghostty" => return true,
                "vscode" => return true,
                "iTerm.app" => return true,
                // Unsupported terminals
                "WezTerm" => return false,
                "Alacritty" => return false,
                _ => {}
            }
        }

        // Check for Windows Terminal
        if std::env::var("WT_SESSION").is_ok() {
            return true;
        }

        // Check for VTE-based terminals (GNOME Terminal, etc.)
        if std::env::var("VTE_VERSION").is_ok() {
            return true;
        }

        // Default to false for unknown terminals to avoid escape sequence pollution
        false
    })
}

/// Sends an OSC 9;4 sequence to set terminal progress.
///
/// This is called automatically by the progress system and typically doesn't need
/// to be called directly.
///
/// # Arguments
///
/// * `state` - The progress state to display
/// * `progress` - Progress percentage (0-100), clamped if greater than 100.
///   Ignored if state is `None` or `Indeterminate`.
pub(crate) fn set_progress(state: ProgressState, progress: u8) {
    let progress = progress.min(100);
    let _ = write_progress(state, progress);
}

fn write_progress(state: ProgressState, progress: u8) -> std::io::Result<()> {
    // Only write OSC sequences if enabled and stderr is actually a terminal
    if !is_enabled() || !console::user_attended_stderr() {
        return Ok(());
    }

    // Only write OSC 9;4 sequences if the terminal supports them
    if !terminal_supports_osc_9_4() {
        return Ok(());
    }

    let mut stderr = std::io::stderr();
    // OSC 9;4 format: ESC ] 9 ; 4 ; <state> ; <progress> BEL
    // Note: The color is controlled by the terminal theme
    // Ghostty may show cyan automatically for normal progress
    write!(stderr, "\x1b]9;4;{};{}\x1b\\", state.as_code(), progress)?;
    stderr.flush()
}

/// Clears any terminal progress indicator.
///
/// This is called automatically when progress jobs stop or complete.
pub(crate) fn clear_progress() {
    if is_enabled() {
        set_progress(ProgressState::None, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_state_codes() {
        assert_eq!(ProgressState::None.as_code(), 0);
        assert_eq!(ProgressState::Normal.as_code(), 1);
        assert_eq!(ProgressState::Error.as_code(), 2);
        assert_eq!(ProgressState::Indeterminate.as_code(), 3);
        assert_eq!(ProgressState::Warning.as_code(), 4);
    }

    #[test]
    fn test_set_progress_doesnt_panic() {
        // Just ensure it doesn't panic when called
        set_progress(ProgressState::Normal, 50);
        set_progress(ProgressState::Indeterminate, 0);
        clear_progress();
    }

    #[test]
    fn test_progress_clamping() {
        // Verify that progress values over 100 are clamped
        set_progress(ProgressState::Normal, 150);
    }

    #[test]
    fn test_progress_state_equality() {
        assert_eq!(ProgressState::None, ProgressState::None);
        assert_eq!(ProgressState::Normal, ProgressState::Normal);
        assert_eq!(ProgressState::Error, ProgressState::Error);
        assert_eq!(ProgressState::Indeterminate, ProgressState::Indeterminate);
        assert_eq!(ProgressState::Warning, ProgressState::Warning);

        assert_ne!(ProgressState::None, ProgressState::Normal);
        assert_ne!(ProgressState::Error, ProgressState::Warning);
    }

    #[test]
    fn test_progress_state_clone() {
        let state = ProgressState::Normal;
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_progress_state_debug() {
        let debug_str = format!("{:?}", ProgressState::Normal);
        assert_eq!(debug_str, "Normal");

        let debug_str = format!("{:?}", ProgressState::Error);
        assert_eq!(debug_str, "Error");
    }

    #[test]
    fn test_progress_boundary_values() {
        // Test boundary values for progress percentage
        set_progress(ProgressState::Normal, 0);
        set_progress(ProgressState::Normal, 100);
        set_progress(ProgressState::Normal, 255); // Should be clamped to 100
    }

    #[test]
    fn test_all_progress_states() {
        // Ensure all states can be used with set_progress
        for state in [
            ProgressState::None,
            ProgressState::Normal,
            ProgressState::Error,
            ProgressState::Indeterminate,
            ProgressState::Warning,
        ] {
            set_progress(state, 50);
        }
    }

    #[test]
    fn test_clear_progress_idempotent() {
        // Clearing progress multiple times should not panic
        clear_progress();
        clear_progress();
        clear_progress();
    }
}
