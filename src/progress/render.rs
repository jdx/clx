//! Frame rendering and refresh logic for progress display.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tera::{Context, Tera};

use crate::Result;

use super::diagnostics;
use super::flex::flex;
use super::job::ProgressJob;
use super::output::{ProgressOutput, output};
use super::state::{
    JOBS, LAST_OUTPUT, LINES, REFRESH_LOCK, RENDER_CTX, STARTED, STOPPING, TERA, TERM_LOCK,
    is_disabled, is_paused, term, update_osc_progress,
};

/// Context for rendering a frame.
#[derive(Clone)]
pub struct RenderContext {
    pub start: Instant,
    pub now: Instant,
    pub width: usize,
    pub tera_ctx: Context,
    pub indent: usize,
    pub include_children: bool,
    pub progress: Option<(usize, usize)>,
}

impl Default for RenderContext {
    fn default() -> Self {
        let mut tera_ctx = Context::new();
        tera_ctx.insert("message", "");
        Self {
            start: Instant::now(),
            now: Instant::now(),
            width: term().size().1 as usize,
            tera_ctx,
            indent: 0,
            include_children: true,
            progress: None,
        }
    }
}

impl RenderContext {
    /// Returns the elapsed time since the start.
    pub fn elapsed(&self) -> Duration {
        self.now - self.start
    }
}

/// Prepares the render context for a refresh cycle.
pub(crate) fn prepare_render_context() -> RenderContext {
    let ctx = RENDER_CTX.get_or_init(|| std::sync::Mutex::new(RenderContext::default()));
    let mut ctx_guard = ctx.lock().unwrap();
    ctx_guard.now = Instant::now();
    ctx_guard.width = term().size().1 as usize;
    ctx_guard.clone()
}

/// Result of rendering all jobs to a string.
pub(crate) struct RenderedFrame {
    pub output: String,
    pub jobs: Vec<Arc<ProgressJob>>,
}

/// Prepares the Tera engine and renders all jobs to a string.
pub(crate) fn render_frame() -> Result<RenderedFrame> {
    let ctx = prepare_render_context();
    let mut tera = TERA.lock().unwrap();
    if tera.is_none() {
        *tera = Some(Tera::default());
    }
    let tera = tera.as_mut().unwrap();
    let jobs = JOBS.lock().unwrap().clone();

    update_osc_progress(&jobs);

    let output = jobs
        .iter()
        .map(|job| job.render(tera, ctx.clone()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(RenderedFrame { output, jobs })
}

/// Processes flex tags in the rendered output.
pub(crate) fn process_flex_output(output: &str) -> String {
    if output.contains("<clx:flex>") || output.contains("<clx:flex_fill>") {
        flex(output, term().size().1 as usize)
    } else {
        output.to_string()
    }
}

/// Writes a rendered frame to the terminal.
pub(crate) fn write_frame(output: &str, jobs: &[Arc<ProgressJob>]) -> Result<()> {
    let term = term();
    let mut lines = LINES.lock().unwrap();

    let _guard = TERM_LOCK.lock().unwrap();

    // Clear previous frame
    if *lines > 0 {
        term.move_cursor_up(*lines)?;
        term.move_cursor_left(term.size().1 as usize)?;
        term.clear_to_end_of_screen()?;
    }

    if !output.is_empty() {
        diagnostics::log_frame(output, jobs);
        term.write_line(output)?;

        // Count how many terminal rows were consumed, accounting for wrapping
        let term_width = term.size().1 as usize;
        let mut consumed_rows = 0usize;
        for line in output.lines() {
            let visible_width = console::measure_text_width(line).max(1);
            let rows = if term_width == 0 {
                1
            } else {
                (visible_width - 1) / term_width + 1
            };
            consumed_rows += rows.max(1);
        }
        *lines = consumed_rows.max(1);
    } else {
        *lines = 0;
    }

    Ok(())
}

/// Performs one refresh cycle of the progress display.
///
/// # Returns
///
/// - `Ok(true)` - Continue the refresh loop
/// - `Ok(false)` - Exit the refresh loop (no active jobs or stopping)
/// - `Err(_)` - An error occurred during rendering
pub fn refresh() -> Result<bool> {
    let _refresh_guard = REFRESH_LOCK.lock().unwrap();
    if STOPPING.load(std::sync::atomic::Ordering::Relaxed) {
        *STARTED.lock().unwrap() = false;
        return Ok(false);
    }
    if is_paused() {
        return Ok(true);
    }

    let frame = render_frame()?;
    let any_running_check = || frame.jobs.iter().any(|job| job.is_running());
    let any_running = any_running_check();

    let final_output = process_flex_output(&frame.output);

    // Smart refresh: skip terminal write if output unchanged and no spinners animating
    let mut last_output = LAST_OUTPUT.lock().unwrap();
    let lines = *LINES.lock().unwrap();
    if !any_running && final_output == *last_output && lines > 0 {
        drop(last_output);
        if !any_running && !any_running_check() {
            *STARTED.lock().unwrap() = false;
            return Ok(false);
        }
        return Ok(true);
    }
    *last_output = final_output.clone();
    drop(last_output);

    write_frame(&final_output, &frame.jobs)?;

    if !any_running && !any_running_check() {
        *STARTED.lock().unwrap() = false;
        return Ok(false);
    }
    Ok(true)
}

/// Performs one refresh cycle without loop control.
///
/// In `ProgressOutput::Text` mode this is a no-op: text mode emits a fresh
/// line for each job update, so a full-frame redraw would only repeat content
/// already on the wire (and emit cursor-movement escape codes that look like
/// garbage in non-TTY logs such as CI).
pub fn refresh_once() -> Result<()> {
    if is_disabled() || matches!(output(), ProgressOutput::Quiet | ProgressOutput::Text) {
        return Ok(());
    }
    let _refresh_guard = REFRESH_LOCK.lock().unwrap();

    let frame = render_frame()?;
    let final_output = process_flex_output(&frame.output);
    write_frame(&final_output, &frame.jobs)?;

    Ok(())
}

/// Indents a string with wrapping support.
pub fn indent(s: String, width: usize, indent_size: usize) -> String {
    let mut result = Vec::new();
    let indent_str = " ".repeat(indent_size);

    for line in s.lines() {
        let mut current = String::new();
        let mut current_width = 0;
        let mut chars = line.chars().peekable();
        let mut ansi_code = String::new();

        // Add initial indentation
        if current.is_empty() {
            current.push_str(&indent_str);
            current_width = indent_size;
        }

        while let Some(c) = chars.next() {
            // Handle ANSI escape codes
            if c == '\x1b' {
                ansi_code = String::from(c);
                while let Some(&next) = chars.peek() {
                    ansi_code.push(next);
                    chars.next();
                    if next == 'm' {
                        break;
                    }
                }
                current.push_str(&ansi_code);
                continue;
            }

            let char_width = console::measure_text_width(&c.to_string());
            let next_width = current_width + char_width;

            // Only wrap if we're not at the end of the input and the next character would exceed width
            if next_width > width && !current.trim().is_empty() && chars.peek().is_some() {
                result.push(current);
                current = format!("{}{}", indent_str, ansi_code);
                current_width = indent_size;
            }
            current.push(c);
            if !c.is_control() {
                current_width += char_width;
            }
        }

        // For the last line, if it's too long, we need to wrap it
        if !current.is_empty() {
            if current_width > width {
                let mut width_so_far = indent_size;
                let mut last_valid_pos = indent_str.len();
                let mut chars = current[indent_str.len()..].chars();

                while let Some(c) = chars.next() {
                    if !c.is_control() {
                        width_so_far += console::measure_text_width(&c.to_string());
                        if width_so_far > width {
                            break;
                        }
                    }
                    last_valid_pos = current.len() - chars.as_str().len() - 1;
                }

                let (first, second) = current.split_at(last_valid_pos + 1);
                result.push(first.to_string());
                current = format!("{}{}{}", indent_str, ansi_code, second);
            }
            result.push(current);
        }
    }

    result.join("\n")
}

/// Adds a raw template to the Tera engine if it doesn't already exist.
pub fn add_tera_template(tera: &mut Tera, name: &str, body: &str) -> Result<()> {
    if !tera.get_template_names().any(|n| n == name) {
        tera.add_raw_template(name, body)?;
    }
    Ok(())
}

/// Default cap on rendered text-mode output. A user-supplied template can
/// produce arbitrarily large content (e.g. a multi-line shell script
/// embedded in a `check =` value, or a generated command with hundreds
/// of args expanded inline) and text mode prints each render verbatim.
/// 4096 chars covers any realistic diagnostic while bounding the
/// pathological case. Override with `CLX_TEXT_MAX_LEN`.
const DEFAULT_TEXT_MODE_MAX_LEN: usize = 4096;

/// Resolve the text-mode line cap from the env, falling back to the default.
/// `CLX_TEXT_MAX_LEN=0` (or any unparseable value) disables truncation.
fn text_mode_max_len() -> usize {
    static CACHED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("CLX_TEXT_MAX_LEN")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_TEXT_MODE_MAX_LEN)
    })
}

/// Truncate text-mode output to `max_len` printable characters with a
/// trailing `…`, ANSI-aware so escapes don't get split or counted toward
/// the budget. Returns the input unchanged if it fits, or `max_len == 0`
/// (the documented escape hatch for "never truncate").
fn truncate_text_mode_line(s: &str, max_len: usize) -> String {
    if max_len == 0 || console::measure_text_width(s) <= max_len {
        return s.to_string();
    }
    console::truncate_str(s, max_len, "…").into_owned()
}

/// Helper to render for text mode output.
pub fn render_text_mode(job: &ProgressJob) -> Result<()> {
    let mut ctx = RenderContext {
        include_children: false,
        ..Default::default()
    };
    ctx.tera_ctx.insert("message", "");
    let mut tera = TERA.lock().unwrap();
    if tera.is_none() {
        *tera = Some(Tera::default());
    }
    let tera = tera.as_mut().unwrap();
    let output = job.render(tera, ctx)?;
    if !output.is_empty() {
        // Safety check: ensure no flex tags are visible
        let mut final_output = if output.contains("<clx:flex>") {
            flex(&output, term().size().1 as usize)
        } else {
            output
        };
        // Cap the rendered line length so a runaway template can't dump
        // megabytes into the log on every update. See `text_mode_max_len`.
        final_output = truncate_text_mode_line(&final_output, text_mode_max_len());
        // Skip writing if this job's last text-mode line was identical. Callers
        // often update several props in a row (e.g. `message` then `cur`); each
        // call hits this path, but if the rendered line is unchanged there's no
        // information to add — emitting it again just makes CI logs noisier.
        let mut last = job.last_text_output.lock().unwrap();
        if last.as_deref() == Some(final_output.as_str()) {
            return Ok(());
        }
        *last = Some(final_output.clone());
        drop(last);
        let _guard = TERM_LOCK.lock().unwrap();
        term().write_line(&final_output)?;
        drop(_guard);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_text_mode_line_passes_short_input() {
        let s = "hello world";
        assert_eq!(truncate_text_mode_line(s, 100), "hello world");
    }

    #[test]
    fn truncate_text_mode_line_caps_long_input_with_ellipsis() {
        let s = "a".repeat(50);
        let out = truncate_text_mode_line(&s, 10);
        assert_eq!(console::measure_text_width(&out), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_text_mode_line_zero_disables_truncation() {
        let s = "a".repeat(10_000);
        assert_eq!(truncate_text_mode_line(&s, 0), s);
    }

    #[test]
    fn truncate_text_mode_line_preserves_ansi_escapes() {
        // Prefix with a color escape, then a long run of plain text; the
        // truncate should be ANSI-aware so the printable width is the cap,
        // not the byte length.
        let s = format!("\x1b[31m{}\x1b[0m", "a".repeat(50));
        let out = truncate_text_mode_line(&s, 10);
        assert_eq!(console::measure_text_width(&out), 10);
    }

    #[test]
    fn test_indent() {
        let s = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let result = indent(s.to_string(), 10, 2);
        assert_eq!(
            result,
            "  aaaaaaaa\n  aaaaaaaa\n  aaaaaaaa\n  aaaaaaaa\n  aa"
        );

        let s = "\x1b[0;31maaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let result = indent(s.to_string(), 10, 2);
        assert_eq!(
            result,
            "  \x1b[0;31maaaaaaaa\n  \x1b[0;31maaaaaaaa\n  \x1b[0;31maaaaaaaa\n  \x1b[0;31maaaaaaaa\n  \x1b[0;31maa"
        );
    }
}
