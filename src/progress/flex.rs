//! Flex tag processing for dynamic width content.
//!
//! This module handles the `<clx:flex>` and `<clx:flex_fill>` tags that are used
//! to truncate or pad content to fit the terminal width.

use crate::progress_bar;

/// Process flex tags in the given string.
///
/// Handles both `<clx:flex>` (truncate to fit) and `<clx:flex_fill>` (pad to fill)
/// tags based on the given width.
pub fn flex(s: &str, width: usize) -> String {
    // Fast path: no tags
    if !s.contains("<clx:flex>") && !s.contains("<clx:flex_fill>") {
        return s.to_string();
    }

    // Process repeatedly until no tags remain or no progress can be made
    let mut current = s.to_string();
    let max_passes = 8; // avoid pathological loops
    for _ in 0..max_passes {
        if !current.contains("<clx:flex>") && !current.contains("<clx:flex_fill>") {
            break;
        }

        let before = current.clone();
        current = flex_process_once(&before, width);

        if current == before {
            break;
        }
    }
    current
}

/// Process one pass of flex tags.
fn flex_process_once(s: &str, width: usize) -> String {
    // Try flex_fill first, then flex, then line-by-line fallback
    if let Some(result) = process_flex_fill_tags(s, width) {
        return result;
    }

    if let Some(result) = process_flex_tags(s, width) {
        return result;
    }

    // Fallback: process line by line for incomplete flex tags
    process_flex_line_by_line(s, width)
}

/// Process `<clx:flex_fill>` tags (pads content to fill available width).
fn process_flex_fill_tags(s: &str, width: usize) -> Option<String> {
    let flex_fill_count = s.matches("<clx:flex_fill>").count();
    if flex_fill_count < 2 {
        return None;
    }
    // Delegate to the single-line processor which has the same logic
    process_line_flex_fill(s, width)
}

/// Process `<clx:flex>` tags (truncates content to fit).
fn process_flex_tags(s: &str, width: usize) -> Option<String> {
    let flex_count = s.matches("<clx:flex>").count();
    if flex_count < 2 {
        return None;
    }

    let parts = s.splitn(3, "<clx:flex>").collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }

    let prefix = parts[0];
    let content = parts[1];
    let suffix = if parts.len() == 3 { parts[2] } else { "" };

    // Handle empty content case
    if content.is_empty() {
        let mut result = String::new();
        result.push_str(prefix);
        result.push_str(suffix);
        return Some(result);
    }

    // For multi-line content, we need to handle it specially
    let content_lines: Vec<&str> = content.lines().collect();
    let prefix_lines: Vec<&str> = prefix.lines().collect();
    let suffix_lines: Vec<&str> = suffix.lines().collect();

    // Calculate the width available on the first line
    let first_line_prefix = prefix_lines.last().unwrap_or(&"");
    let first_line_prefix_width = if prefix.ends_with('\n') {
        0
    } else {
        console::measure_text_width(first_line_prefix)
    };

    // For multi-line content, truncate more aggressively
    if content_lines.len() > 1 {
        let available_width = width.saturating_sub(first_line_prefix_width + 3);

        let mut result = String::new();
        result.push_str(prefix);

        if let Some(first_content_line) = content_lines.first() {
            if available_width > 3 {
                let truncated = console::truncate_str(first_content_line, available_width, "…");
                result.push_str(&truncated);
            } else {
                result.push('…');
            }
        } else {
            result.push_str(content);
        }

        return Some(result);
    }

    // Single line with flex tags
    let suffix_width = if suffix_lines.is_empty() {
        0
    } else {
        console::measure_text_width(suffix_lines[0])
    };
    let available_for_content = width.saturating_sub(first_line_prefix_width + suffix_width);

    if first_line_prefix_width >= width {
        return Some(console::truncate_str(prefix, width, "…").to_string());
    }

    let mut result = String::new();
    result.push_str(prefix);

    if content.starts_with("<clx:progress") {
        // Render a progress bar sized to the available space
        if let Some(pb) = render_progress_placeholder(content, available_for_content) {
            result.push_str(&pb);
            result.push_str(suffix);
            return Some(result);
        }
    }

    if available_for_content > 3 {
        result.push_str(&console::truncate_str(content, available_for_content, "…"));
        result.push_str(suffix);
    } else {
        let available = width.saturating_sub(first_line_prefix_width);
        if available > 3 {
            result.push_str(&console::truncate_str(content, available, "…"));
        }
    }

    Some(result)
}

/// Render a progress bar from a placeholder tag.
fn render_progress_placeholder(content: &str, available_width: usize) -> Option<String> {
    let mut cur: Option<usize> = None;
    let mut total: Option<usize> = None;
    let mut chars_encoded: Option<&str> = None;

    for part in content.trim_matches(['<', '>', ' ']).split_whitespace() {
        if let Some(v) = part.strip_prefix("cur=") {
            cur = v.parse::<usize>().ok();
        } else if let Some(v) = part.strip_prefix("total=") {
            total = v.parse::<usize>().ok();
        } else if let Some(v) = part.strip_prefix("chars=") {
            chars_encoded = Some(v);
        }
    }

    if let (Some(cur), Some(total)) = (cur, total) {
        let chars = chars_encoded
            .map(decode_progress_bar_chars)
            .unwrap_or_default();
        Some(progress_bar::progress_bar_with_chars(
            cur,
            total,
            available_width,
            &chars,
        ))
    } else {
        None
    }
}

/// Process flex tags line by line (fallback for incomplete tags).
fn process_flex_line_by_line(s: &str, width: usize) -> String {
    s.lines()
        .map(|line| {
            // Handle flex_fill in line-by-line mode
            if line.contains("<clx:flex_fill>") {
                if let Some(result) = process_line_flex_fill(line, width) {
                    return result;
                }
            }

            if !line.contains("<clx:flex>") {
                return line.to_string();
            }

            process_line_flex(line, width)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Process flex_fill on a single line.
fn process_line_flex_fill(line: &str, width: usize) -> Option<String> {
    let parts = line.splitn(3, "<clx:flex_fill>").collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }

    let prefix = parts[0];
    let content = parts[1];
    let suffix = if parts.len() == 3 { parts[2] } else { "" };

    let prefix_width = console::measure_text_width(prefix);
    let suffix_width = console::measure_text_width(suffix);
    let content_width = console::measure_text_width(content);
    let available_for_content = width.saturating_sub(prefix_width + suffix_width);

    let mut result = String::new();
    result.push_str(prefix);

    if content_width >= available_for_content {
        if available_for_content > 3 {
            result.push_str(&console::truncate_str(content, available_for_content, "…"));
        } else {
            result.push_str(content);
        }
    } else {
        result.push_str(content);
        let padding = available_for_content.saturating_sub(content_width);
        result.push_str(&" ".repeat(padding));
    }
    result.push_str(suffix);
    Some(result)
}

/// Process flex on a single line.
fn process_line_flex(line: &str, width: usize) -> String {
    let parts = line.splitn(3, "<clx:flex>").collect::<Vec<_>>();
    if parts.len() < 2 {
        return line.to_string();
    }

    let prefix = parts[0];
    let content = parts[1];
    let suffix = if parts.len() == 3 { parts[2] } else { "" };

    let prefix_width = console::measure_text_width(prefix);
    let suffix_width = console::measure_text_width(suffix);
    let available_for_content = width.saturating_sub(prefix_width + suffix_width);

    if prefix_width >= width {
        return console::truncate_str(line, width, "…").to_string();
    }

    let mut result = String::new();
    result.push_str(prefix);

    if available_for_content > 3 {
        result.push_str(&console::truncate_str(content, available_for_content, "…"));
        result.push_str(suffix);
    } else {
        let available = width.saturating_sub(prefix_width);
        if available > 3 {
            result.push_str(&console::truncate_str(content, available, "…"));
        }
    }

    result
}

/// Returns a prefix of s with at most max_bytes bytes, cutting only at char boundaries.
pub fn safe_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    match s
        .char_indices()
        .take_while(|(i, _)| *i < max_bytes)
        .map(|(i, _)| i)
        .last()
    {
        Some(last_boundary) => &s[..last_boundary],
        None => "",
    }
}

/// Encode progress bar chars for embedding in placeholder tags.
pub fn encode_progress_bar_chars(chars: &progress_bar::ProgressBarChars) -> String {
    // Simple encoding: use comma as separator and percent-encode special chars
    fn encode_part(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                ',' => "%2C".to_string(),
                '%' => "%25".to_string(),
                ' ' => "%20".to_string(),
                '<' => "%3C".to_string(),
                '>' => "%3E".to_string(),
                _ => c.to_string(),
            })
            .collect()
    }
    format!(
        "{},{},{},{},{}",
        encode_part(&chars.fill),
        encode_part(&chars.head),
        encode_part(&chars.empty),
        encode_part(&chars.left),
        encode_part(&chars.right)
    )
}

/// Decode progress bar chars from placeholder tag encoding.
pub fn decode_progress_bar_chars(encoded: &str) -> progress_bar::ProgressBarChars {
    // Single-pass decode to avoid issues with sequences like %252C
    fn decode_part(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                match hex.as_str() {
                    "2C" => result.push(','),
                    "20" => result.push(' '),
                    "3C" => result.push('<'),
                    "3E" => result.push('>'),
                    "25" => result.push('%'),
                    _ => {
                        result.push('%');
                        result.push_str(&hex);
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }
    let parts: Vec<&str> = encoded.splitn(5, ',').collect();
    if parts.len() >= 5 {
        progress_bar::ProgressBarChars {
            fill: decode_part(parts[0]),
            head: decode_part(parts[1]),
            empty: decode_part(parts[2]),
            left: decode_part(parts[3]),
            right: decode_part(parts[4]),
        }
    } else {
        progress_bar::ProgressBarChars::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex() {
        // Test normal case
        let s = "prefix<clx:flex>content<clx:flex>suffix";
        let result = flex(s, 20);
        let width = console::measure_text_width(&result);
        assert!(width <= 20);
        assert!(result.contains("prefix"));
        assert!(result.contains("suffix"));

        // Test case where prefix + suffix are longer than available width
        let s = "very_long_prefix<clx:flex>content<clx:flex>very_long_suffix";
        let result = flex(s, 10);
        let width = console::measure_text_width(&result);
        assert!(width <= 10);
        assert!(!result.is_empty());

        // Test case with extremely long content
        let long_content = "a".repeat(1000);
        let s = format!("prefix<clx:flex>{}<clx:flex>suffix", long_content);
        let result = flex(&s, 30);
        let width = console::measure_text_width(&result);
        assert!(width <= 30);
        assert!(result.contains("prefix"));
        assert!(result.contains("suffix"));

        // Test case with extremely long prefix and suffix
        let long_prefix = "very_long_prefix_that_exceeds_screen_width_".repeat(10);
        let long_suffix = "very_long_suffix_that_exceeds_screen_width_".repeat(10);
        let s = format!("{}<clx:flex>content<clx:flex>{}", long_prefix, long_suffix);
        let result = flex(&s, 50);
        let width = console::measure_text_width(&result);
        assert!(width <= 50);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_flex_progress_placeholder_basic() {
        let s = "prefix<clx:flex><clx:progress cur=5 total=10><clx:flex>suffix";
        let target_width = 50;
        let result = flex(s, target_width);
        let width = console::measure_text_width(&result);
        assert_eq!(width, target_width);
        assert!(result.contains('[') && result.contains(']'));
        assert!(!result.contains("<clx:progress"));
    }

    #[test]
    fn test_flex_progress_placeholder_min_width() {
        let prefix = "a";
        let suffix = "b";
        let s = format!(
            "{}<clx:flex><clx:progress cur=1 total=1><clx:flex>{}",
            prefix, suffix
        );
        let target_width = 4;
        let result = flex(&s, target_width);
        let width = console::measure_text_width(&result);
        assert_eq!(width, target_width);
        assert!(!result.contains("<clx:progress"));
    }

    #[test]
    fn test_flex_fill() {
        // Test that flex_fill pads content to fill available width
        let s = "prefix<clx:flex_fill>short<clx:flex_fill>suffix";
        let result = flex(s, 30);
        let width = console::measure_text_width(&result);
        assert_eq!(width, 30);
        assert!(result.starts_with("prefix"));
        assert!(result.ends_with("suffix"));
        assert!(result.contains("short"));
        assert!(result.contains("     ")); // multiple spaces

        // Test flex_fill with long content (should truncate)
        let s =
            "pre<clx:flex_fill>this is very long content that needs truncation<clx:flex_fill>end";
        let result = flex(s, 20);
        let width = console::measure_text_width(&result);
        assert!(width <= 20);
        assert!(result.starts_with("pre"));
    }

    #[test]
    fn test_flex_fill_right_align() {
        let s = "X<clx:flex_fill>msg<clx:flex_fill>[====]";
        let result = flex(s, 20);
        assert_eq!(console::measure_text_width(&result), 20);
        assert!(result.starts_with("Xmsg"));
        assert!(result.ends_with("[====]"));
    }

    #[test]
    fn test_safe_prefix() {
        assert_eq!(safe_prefix("hello", 10), "hello");
        assert_eq!(safe_prefix("hello", 5), "hello");
        assert_eq!(safe_prefix("hello", 3), "he");
        assert_eq!(safe_prefix("hello", 1), "");
        assert_eq!(safe_prefix("hello", 0), "");

        let s = "helloworld";
        assert_eq!(safe_prefix(s, 5), "hell");
    }

    #[test]
    fn test_encode_decode_progress_bar_chars() {
        let chars = progress_bar::ProgressBarChars {
            fill: "█".to_string(),
            head: "▓".to_string(),
            empty: " ".to_string(),
            left: "[".to_string(),
            right: "]".to_string(),
        };

        let encoded = encode_progress_bar_chars(&chars);
        let decoded = decode_progress_bar_chars(&encoded);

        assert_eq!(decoded.fill, chars.fill);
        assert_eq!(decoded.head, chars.head);
        assert_eq!(decoded.empty, chars.empty);
        assert_eq!(decoded.left, chars.left);
        assert_eq!(decoded.right, chars.right);
    }

    #[test]
    fn test_encode_decode_special_chars() {
        let chars = progress_bar::ProgressBarChars {
            fill: "|".to_string(),
            head: "|".to_string(),
            empty: " ".to_string(),
            left: "|".to_string(),
            right: "|".to_string(),
        };

        let encoded = encode_progress_bar_chars(&chars);
        let decoded = decode_progress_bar_chars(&encoded);

        assert_eq!(decoded.fill, chars.fill);
        assert_eq!(decoded.head, chars.head);
        assert_eq!(decoded.empty, chars.empty);
        assert_eq!(decoded.left, chars.left);
        assert_eq!(decoded.right, chars.right);
    }

    #[test]
    fn test_encode_decode_angle_brackets() {
        let chars = progress_bar::ProgressBarChars {
            fill: "=".to_string(),
            head: ">".to_string(),
            empty: " ".to_string(),
            left: "<".to_string(),
            right: ">".to_string(),
        };

        let encoded = encode_progress_bar_chars(&chars);
        let decoded = decode_progress_bar_chars(&encoded);

        assert_eq!(decoded.fill, chars.fill);
        assert_eq!(decoded.head, chars.head);
        assert_eq!(decoded.empty, chars.empty);
        assert_eq!(decoded.left, chars.left);
        assert_eq!(decoded.right, chars.right);
    }
}
