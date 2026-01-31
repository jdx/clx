//! Tera template function and filter registration.
//!
//! This module registers all the custom functions and filters available
//! in progress job templates.

use std::collections::HashMap;
use std::time::Duration;

use tera::Tera;

use crate::{progress_bar, style};

use super::flex::{encode_progress_bar_chars, safe_prefix};
use super::format::{format_bytes, format_count, format_duration};
use super::job::{ProgressJob, ProgressStatus};
use super::output::{ProgressOutput, output};
use super::render::RenderContext;
use super::spinners::{DEFAULT_SPINNER, SPINNERS};

/// Registers all Tera functions and filters for a job.
pub fn add_tera_functions(tera: &mut Tera, ctx: &RenderContext, job: &ProgressJob) {
    let elapsed = ctx.elapsed().as_millis() as usize;
    let job_elapsed = job.start.elapsed();
    // Use operation-specific elapsed time for ETA/rate calculations after next_operation()
    let operation_elapsed_secs = job.operation_start.lock().unwrap().elapsed().as_secs_f64();
    let status = job.status.lock().unwrap().clone();
    let progress = ctx.progress;
    let width = ctx.width;

    register_time_functions(tera, job_elapsed, operation_elapsed_secs, progress, job);
    register_rate_functions(tera, progress, operation_elapsed_secs, job);
    register_progress_functions(tera, progress);
    register_spinner_function(tera, elapsed, &status);
    register_progress_bar_function(tera, progress, width);
    register_flex_filters(tera, width);
    register_style_filters(tera);
}

/// Registers elapsed() and eta() functions.
fn register_time_functions(
    tera: &mut Tera,
    job_elapsed: Duration,
    operation_elapsed_secs: f64,
    progress: Option<(usize, usize)>,
    job: &ProgressJob,
) {
    // elapsed() - time since job started
    let elapsed_str = format_duration(job_elapsed);
    tera.register_function("elapsed", move |_: &HashMap<String, tera::Value>| {
        Ok(elapsed_str.clone().into())
    });

    // eta() - estimated time remaining (uses operation-specific elapsed time for fallback)
    let smoothed_rate = *job.smoothed_rate.lock().unwrap();
    let (eta_value, eta_is_complete) =
        calculate_eta(progress, smoothed_rate, operation_elapsed_secs);
    tera.register_function("eta", move |props: &HashMap<String, tera::Value>| {
        let hide_complete = props
            .get("hide_complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if hide_complete && (eta_is_complete || eta_value.is_none()) {
            Ok("".to_string().into())
        } else {
            Ok(eta_value.clone().unwrap_or_else(|| "-".to_string()).into())
        }
    });
}

/// Calculate ETA based on progress and rate.
/// Uses operation_elapsed_secs for linear extrapolation fallback to give accurate
/// estimates after next_operation() resets the smoothed rate.
fn calculate_eta(
    progress: Option<(usize, usize)>,
    smoothed_rate: Option<f64>,
    operation_elapsed_secs: f64,
) -> (Option<String>, bool) {
    if let Some((cur, total)) = progress {
        if cur > 0 && total > 0 && cur <= total {
            let remaining_items = (total - cur) as f64;

            let remaining_secs = if let Some(rate) = smoothed_rate {
                if rate > 0.0 {
                    remaining_items / rate
                } else {
                    // Fall back to linear extrapolation using operation-specific elapsed time
                    let progress_ratio = cur as f64 / total as f64;
                    let estimated_total = operation_elapsed_secs / progress_ratio;
                    estimated_total - operation_elapsed_secs
                }
            } else {
                // No smoothed rate yet, use linear extrapolation with operation-specific time
                let progress_ratio = cur as f64 / total as f64;
                let estimated_total = operation_elapsed_secs / progress_ratio;
                estimated_total - operation_elapsed_secs
            };

            if remaining_secs > 0.0 {
                (
                    Some(format_duration(Duration::from_secs_f64(remaining_secs))),
                    false,
                )
            } else {
                (Some("0s".to_string()), true)
            }
        } else {
            (None, cur >= total)
        }
    } else {
        (None, false)
    }
}

/// Registers rate() function.
fn register_rate_functions(
    tera: &mut Tera,
    progress: Option<(usize, usize)>,
    operation_elapsed_secs: f64,
    job: &ProgressJob,
) {
    let smoothed_rate = *job.smoothed_rate.lock().unwrap();
    let rate_str = calculate_rate_string(progress, smoothed_rate, operation_elapsed_secs);
    tera.register_function("rate", move |_: &HashMap<String, tera::Value>| {
        Ok(rate_str.clone().into())
    });
}

/// Calculate rate string for display.
/// Uses operation_elapsed_secs for average rate fallback to give accurate
/// rates after next_operation() resets the smoothed rate.
fn calculate_rate_string(
    progress: Option<(usize, usize)>,
    smoothed_rate: Option<f64>,
    operation_elapsed_secs: f64,
) -> String {
    if let Some((cur, _total)) = progress {
        let rate = smoothed_rate.unwrap_or_else(|| {
            if operation_elapsed_secs > 0.0 && cur > 0 {
                cur as f64 / operation_elapsed_secs
            } else {
                0.0
            }
        });
        if rate >= 1.0 {
            format!("{:.1}/s", rate)
        } else if rate >= 1.0 / 60.0 {
            format!("{:.1}/m", rate * 60.0)
        } else if rate > 0.0 {
            format!("{:.2}/s", rate)
        } else {
            "-/s".to_string()
        }
    } else {
        "-/s".to_string()
    }
}

/// Registers bytes(), percentage(), and count_format() functions.
fn register_progress_functions(tera: &mut Tera, progress: Option<(usize, usize)>) {
    // bytes() - show progress as human-readable bytes
    // Options:
    //   hide_complete: bool - if true, return empty string when progress is 100%
    //   total: bool - if false, show only current bytes without "/ total" (default: true)
    let bytes_is_complete = progress.map(|(cur, total)| cur >= total).unwrap_or(false);
    tera.register_function("bytes", move |props: &HashMap<String, tera::Value>| {
        let hide_complete = props
            .get("hide_complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let show_total = props.get("total").and_then(|v| v.as_bool()).unwrap_or(true);
        if hide_complete && bytes_is_complete {
            return Ok("".to_string().into());
        }
        if let Some((cur, total)) = progress {
            if show_total {
                Ok(format!("{} / {}", format_bytes(cur), format_bytes(total)).into())
            } else {
                Ok(format_bytes(cur).into())
            }
        } else {
            Ok("".to_string().into())
        }
    });

    // percentage() - show progress as percentage
    let percentage_is_complete = progress.map(|(cur, total)| cur >= total).unwrap_or(false);
    tera.register_function("percentage", move |props: &HashMap<String, tera::Value>| {
        let hide_complete = props
            .get("hide_complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if hide_complete && percentage_is_complete {
            return Ok("".to_string().into());
        }
        if let Some((cur, total)) = progress {
            if total > 0 {
                let pct = (cur as f64 / total as f64) * 100.0;
                let decimals = props
                    .get("decimals")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    .clamp(0, 20) as usize;
                Ok(format!("{:.prec$}%", pct, prec = decimals).into())
            } else {
                Ok("0%".to_string().into())
            }
        } else {
            Ok("".to_string().into())
        }
    });

    // count_format() - show a number in human-readable format
    tera.register_function(
        "count_format",
        move |props: &HashMap<String, tera::Value>| {
            let value = props
                .get("value")
                .and_then(|v| v.as_i64())
                .map(|v| v.max(0) as usize)
                .or_else(|| progress.map(|(cur, _)| cur));

            if let Some(n) = value {
                let decimals = props
                    .get("decimals")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1)
                    .clamp(0, 20) as usize;
                Ok(format_count(n, decimals).into())
            } else {
                Ok("".to_string().into())
            }
        },
    );
}

/// Registers the spinner() function.
fn register_spinner_function(tera: &mut Tera, elapsed: usize, status: &ProgressStatus) {
    let status = status.clone();
    tera.register_function(
        "spinner",
        move |props: &HashMap<String, tera::Value>| match status {
            ProgressStatus::Running if output() == ProgressOutput::Text => {
                Ok(" ".to_string().into())
            }
            ProgressStatus::Hide => Ok(" ".to_string().into()),
            ProgressStatus::Pending => Ok(style::eyellow("⏸").dim().to_string().into()),
            ProgressStatus::Running => {
                let name = props
                    .get("name")
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or(DEFAULT_SPINNER);
                let spinner = SPINNERS.get(name).expect("spinner not found");
                let frame_index = (elapsed / spinner.fps) % spinner.frames.len();
                let frame = spinner.frames[frame_index].clone();
                Ok(style::eblue(frame).to_string().into())
            }
            ProgressStatus::Done => Ok(style::egreen("✔").bright().to_string().into()),
            ProgressStatus::Failed => Ok(style::ered("✗").to_string().into()),
            ProgressStatus::RunningCustom(ref s) => Ok(s.clone().into()),
            ProgressStatus::DoneCustom(ref s) => Ok(s.clone().into()),
            ProgressStatus::Warn => Ok(style::eyellow("⚠").to_string().into()),
        },
    );
}

/// Registers the progress_bar() function.
fn register_progress_bar_function(tera: &mut Tera, progress: Option<(usize, usize)>, width: usize) {
    tera.register_function(
        "progress_bar",
        move |props: &HashMap<String, tera::Value>| {
            if let Some((progress_current, progress_total)) = progress {
                let hide_complete = props
                    .get("hide_complete")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if hide_complete && progress_current >= progress_total {
                    return Ok("".to_string().into());
                }

                let chars = build_progress_bar_chars(props);

                let is_flex = props
                    .get("flex")
                    .as_ref()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_flex {
                    let chars_encoded = encode_progress_bar_chars(&chars);
                    let placeholder = format!(
                        "<clx:flex><clx:progress cur={} total={} chars={}><clx:flex>",
                        progress_current, progress_total, chars_encoded
                    );
                    Ok(placeholder.into())
                } else {
                    let bar_width = props
                        .get("width")
                        .as_ref()
                        .and_then(|v| v.as_i64())
                        .map(|v| {
                            if v < 0 {
                                width - (-v as usize)
                            } else {
                                v as usize
                            }
                        })
                        .unwrap_or(width);
                    let progress_bar = progress_bar::progress_bar_with_chars(
                        progress_current,
                        progress_total,
                        bar_width,
                        &chars,
                    );
                    Ok(progress_bar.into())
                }
            } else {
                Ok("".to_string().into())
            }
        },
    );
}

/// Build progress bar characters from template props.
fn build_progress_bar_chars(
    props: &HashMap<String, tera::Value>,
) -> progress_bar::ProgressBarChars {
    // Check for preset style first
    if let Some(style) = props.get("style").and_then(|v| v.as_str()) {
        match style {
            "blocks" => return progress_bar::ProgressBarChars::blocks(),
            "thin" => return progress_bar::ProgressBarChars::thin(),
            _ => {}
        }
    }

    // Build from individual character options
    let mut chars = progress_bar::ProgressBarChars::default();
    if let Some(fill) = props.get("fill").and_then(|v| v.as_str()) {
        chars.fill = fill.to_string();
    }
    if let Some(head) = props.get("head").and_then(|v| v.as_str()) {
        chars.head = head.to_string();
    }
    if let Some(empty) = props.get("empty").and_then(|v| v.as_str()) {
        chars.empty = empty.to_string();
    }
    if let Some(left) = props.get("left").and_then(|v| v.as_str()) {
        chars.left = left.to_string();
    }
    if let Some(right) = props.get("right").and_then(|v| v.as_str()) {
        chars.right = right.to_string();
    }
    chars
}

/// Registers flex and flex_fill filters.
fn register_flex_filters(tera: &mut Tera, width: usize) {
    // flex filter - truncates content to fit
    tera.register_filter(
        "flex",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(format!("<clx:flex>{}<clx:flex>", content).into())
        },
    );

    // flex_fill filter - pads content to fill available width
    tera.register_filter(
        "flex_fill",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(format!("<clx:flex_fill>{}<clx:flex_fill>", content).into())
        },
    );

    // truncate_text filter - simple truncation for text mode
    tera.register_filter(
        "truncate_text",
        move |value: &tera::Value, args: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());

            let prefix_len = args
                .get("prefix_len")
                .and_then(|v| v.as_i64())
                .map(|v| v as usize)
                .unwrap_or(20);

            let max_len = args
                .get("length")
                .and_then(|v| v.as_i64())
                .map(|v| v as usize)
                .unwrap_or_else(|| width.saturating_sub(prefix_len));

            if content.len() <= max_len {
                Ok(content.into())
            } else if max_len > 1 {
                Ok(format!("{}…", safe_prefix(&content, max_len.saturating_sub(1))).into())
            } else {
                Ok("…".into())
            }
        },
    );
}

/// Registers color and style filters.
fn register_style_filters(tera: &mut Tera) {
    // Helper to create a style filter
    macro_rules! register_style_filter {
        ($tera:expr, $name:literal, $style_fn:path) => {
            $tera.register_filter(
                $name,
                |value: &tera::Value, _: &HashMap<String, tera::Value>| {
                    let content = value
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| value.to_string());
                    Ok($style_fn(&content).to_string().into())
                },
            );
        };
    }

    register_style_filter!(tera, "cyan", style::ecyan);
    register_style_filter!(tera, "blue", style::eblue);
    register_style_filter!(tera, "green", style::egreen);
    register_style_filter!(tera, "yellow", style::eyellow);
    register_style_filter!(tera, "red", style::ered);
    register_style_filter!(tera, "magenta", style::emagenta);
    register_style_filter!(tera, "bold", style::ebold);
    register_style_filter!(tera, "dim", style::edim);
    register_style_filter!(tera, "underline", style::eunderline);
}
