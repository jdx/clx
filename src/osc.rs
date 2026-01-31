/// OSC (Operating System Command) escape sequences for terminal integration
///
/// This module provides support for OSC escape sequences that allow terminal
/// integration features like progress bars in Ghostty, VS Code, Windows Terminal,
/// and VTE-based terminals.
use std::io::Write;
use std::sync::OnceLock;

/// Global OSC progress enable/disable flag
static OSC_ENABLED: OnceLock<bool> = OnceLock::new();

/// Configure OSC progress functionality
pub fn configure(enabled: bool) {
    OSC_ENABLED
        .set(enabled)
        .expect("OSC_ENABLED already initialized");
}

/// Check if OSC progress is enabled
pub(crate) fn is_enabled() -> bool {
    *OSC_ENABLED.get_or_init(|| true)
}

/// OSC 9;4 states for terminal progress indication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// No progress indicator (clears any existing progress)
    None,
    /// Normal progress bar with percentage (typically shows as default color, often blue/cyan)
    Normal,
    /// Error state (typically shows as red)
    Error,
    /// Indeterminate progress (spinner/activity indicator)
    Indeterminate,
    /// Warning state (typically shows as yellow)
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

/// Checks if the current terminal supports OSC 9;4 progress sequences
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

/// Sends an OSC 9;4 sequence to set terminal progress
///
/// # Arguments
/// * `state` - The progress state to display
/// * `progress` - Progress percentage (0-100), ignored if state is None or Indeterminate
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

/// Clears any terminal progress indicator
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
