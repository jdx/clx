//! Formatting utilities for durations, bytes, and counts.

use std::time::Duration;

/// Formats a duration as a human-readable string.
///
/// - Under 60 seconds: "42s"
/// - Under 1 hour: "1m30s"
/// - 1 hour or more: "1h30m45s"
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m{}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

/// Formats a byte count as a human-readable string.
///
/// Uses binary prefixes (KB = 1024 bytes, MB = 1024 KB, etc.).
pub fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{} B", bytes as usize)
    }
}

/// Formats a count as a human-readable string with SI suffixes.
///
/// Uses decimal prefixes (K = 1000, M = 1,000,000, B = 1,000,000,000).
pub fn format_count(count: usize, decimals: usize) -> String {
    const K: f64 = 1_000.0;
    const M: f64 = 1_000_000.0;
    const B: f64 = 1_000_000_000.0;

    let count = count as f64;
    if count >= B {
        format!("{:.prec$}B", count / B, prec = decimals)
    } else if count >= M {
        format!("{:.prec$}M", count / M, prec = decimals)
    } else if count >= K {
        format!("{:.prec$}K", count / K, prec = decimals)
    } else {
        format!("{}", count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_duration(Duration::from_secs(1)), "1s");
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m0s");
        assert_eq!(format_duration(Duration::from_secs(61)), "1m1s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m30s");
        assert_eq!(format_duration(Duration::from_secs(3599)), "59m59s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h0m0s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h1m1s");
        assert_eq!(format_duration(Duration::from_secs(7200)), "2h0m0s");
        assert_eq!(format_duration(Duration::from_secs(86399)), "23h59m59s");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(10240), "10.0 KB");
        assert_eq!(format_bytes(1024 * 1023), "1023.0 KB");
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 + 512 * 1024), "1.5 MB");
        assert_eq!(format_bytes(100 * 1024 * 1024), "100.0 MB");
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn test_format_count() {
        // Small numbers stay as-is
        assert_eq!(format_count(0, 1), "0");
        assert_eq!(format_count(999, 1), "999");

        // Thousands
        assert_eq!(format_count(1000, 1), "1.0K");
        assert_eq!(format_count(1500, 1), "1.5K");
        assert_eq!(format_count(999_999, 1), "1000.0K");

        // Millions
        assert_eq!(format_count(1_000_000, 1), "1.0M");
        assert_eq!(format_count(1_500_000, 1), "1.5M");

        // Billions
        assert_eq!(format_count(1_000_000_000, 1), "1.0B");
        assert_eq!(format_count(2_500_000_000, 1), "2.5B");

        // Different decimal places
        assert_eq!(format_count(1_234_567, 0), "1M");
        assert_eq!(format_count(1_234_567, 2), "1.23M");
    }
}
