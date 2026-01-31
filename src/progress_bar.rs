use crate::style;

/// Characters used to render a progress bar.
#[derive(Debug, Clone)]
pub struct ProgressBarChars {
    /// Character for filled portion (default: "=")
    pub fill: String,
    /// Character for the leading edge/head (default: ">")
    pub head: String,
    /// Character for empty portion (default: " ")
    pub empty: String,
    /// Left bracket (default: "[")
    pub left: String,
    /// Right bracket (default: "]")
    pub right: String,
}

impl Default for ProgressBarChars {
    fn default() -> Self {
        Self {
            fill: "=".to_string(),
            head: ">".to_string(),
            empty: " ".to_string(),
            left: "[".to_string(),
            right: "]".to_string(),
        }
    }
}

impl ProgressBarChars {
    /// Creates a new ProgressBarChars with block-style characters.
    pub fn blocks() -> Self {
        Self {
            fill: "█".to_string(),
            head: "▓".to_string(),
            empty: "░".to_string(),
            left: "".to_string(),
            right: "".to_string(),
        }
    }

    /// Creates a new ProgressBarChars with thin block-style characters.
    pub fn thin() -> Self {
        Self {
            fill: "━".to_string(),
            head: "╸".to_string(),
            empty: "─".to_string(),
            left: "".to_string(),
            right: "".to_string(),
        }
    }
}

pub(crate) fn progress_bar_with_chars(
    progress_current: usize,
    progress_total: usize,
    width: usize,
    chars: &ProgressBarChars,
) -> String {
    let bracket_width =
        console::measure_text_width(&chars.left) + console::measure_text_width(&chars.right);
    let inner_width = width.saturating_sub(bracket_width);

    let progress = if progress_total > 0 {
        progress_current as f64 / progress_total as f64
    } else {
        0.0
    };
    let filled_length = (inner_width as f64 * progress).round() as usize;

    let bar_content = if progress >= 1.0 {
        chars.fill.repeat(inner_width)
    } else if filled_length > 0 {
        let fill_part = chars.fill.repeat(filled_length.saturating_sub(1));
        let empty_part = chars
            .empty
            .repeat(inner_width.saturating_sub(filled_length));
        format!("{}{}{}", fill_part, chars.head, empty_part)
    } else {
        chars.empty.repeat(inner_width)
    };

    style::edim(format!("{}{}{}", chars.left, bar_content, chars.right)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar_default() {
        let bar = progress_bar_with_chars(50, 100, 12, &ProgressBarChars::default());
        // Should contain brackets and some fill
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
    }

    #[test]
    fn test_progress_bar_complete() {
        let bar = progress_bar_with_chars(100, 100, 12, &ProgressBarChars::default());
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
    }

    #[test]
    fn test_progress_bar_empty() {
        let bar = progress_bar_with_chars(0, 100, 12, &ProgressBarChars::default());
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
    }

    #[test]
    fn test_progress_bar_custom_chars() {
        let chars = ProgressBarChars::blocks();
        let bar = progress_bar_with_chars(50, 100, 10, &chars);
        // Blocks style has no brackets
        assert!(!bar.contains('['));
    }

    #[test]
    fn test_progress_bar_chars_presets() {
        let _ = ProgressBarChars::default();
        let _ = ProgressBarChars::blocks();
        let _ = ProgressBarChars::thin();
    }
}
