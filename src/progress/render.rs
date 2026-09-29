//! Frame rendering and refresh logic for progress display.

use std::borrow::Cow;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use tera::{Context, Tera};

use crate::{Result, style};

use super::diagnostics;
use super::flex::flex;
use super::job::ProgressJob;
use super::output::{ProgressOutput, output};
use super::state::{
    CRAMPED_VIEWPORT, FRAME_TRUNCATED, JOBS, LAST_OUTPUT, LINES, REFRESH_LOCK, RENDER_CTX, STARTED,
    STOPPING, SyncUpdate, TERA, TERM_LOCK, erase_rows_above, is_disabled, is_paused,
    reset_viewport, term, update_osc_progress,
};

const RESIZE_SETTLE_TIME: Duration = Duration::from_millis(100);

#[derive(Default)]
struct TerminalResizeState {
    size: Option<(u16, u16)>,
    changed_at: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
enum ResizeAction {
    None,
    ClearAndDefer,
    Defer,
    ClearAndRender,
}

impl TerminalResizeState {
    fn update(
        &mut self,
        size: (u16, u16),
        has_frame: bool,
        any_running: bool,
        now: Instant,
    ) -> ResizeAction {
        if !has_frame && self.changed_at.is_none() {
            self.size = Some(size);
            return ResizeAction::None;
        }

        if self.size != Some(size) {
            self.size = Some(size);
            if any_running {
                self.changed_at = Some(now);
                return ResizeAction::ClearAndDefer;
            }
            self.changed_at = None;
            return ResizeAction::ClearAndRender;
        }

        if let Some(changed_at) = self.changed_at {
            if any_running && now.duration_since(changed_at) < RESIZE_SETTLE_TIME {
                return ResizeAction::Defer;
            }
            self.changed_at = None;
            return ResizeAction::ClearAndRender;
        }

        ResizeAction::None
    }
}

static TERMINAL_RESIZE_STATE: LazyLock<Mutex<TerminalResizeState>> =
    LazyLock::new(|| Mutex::new(TerminalResizeState::default()));

/// Whether the terminal changed size since the last frame was drawn.
fn viewport_resized() -> bool {
    let size = term().size();
    TERMINAL_RESIZE_STATE
        .lock()
        .unwrap()
        .size
        .is_some_and(|last| last != size)
}

pub(crate) fn reset_terminal_resize_state() {
    *TERMINAL_RESIZE_STATE.lock().unwrap() = TerminalResizeState::default();
}

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
    /// One entry per line of `output`: whether the line was rendered by a job
    /// that is still running, so a frame that must be cut can keep those in view.
    pub running_lines: Vec<bool>,
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

    let mut blocks = Vec::new();
    let mut running_lines = Vec::new();
    for job in &jobs {
        let segments = job.render_segments(tera, ctx.clone())?;
        let block = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if block.is_empty() {
            continue;
        }
        for segment in &segments {
            running_lines.extend(segment.text.split('\n').map(|_| segment.running));
        }
        blocks.push(block);
    }
    let output = blocks.join("\n");

    Ok(RenderedFrame {
        output,
        jobs,
        running_lines,
    })
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
///
/// Returns `true` when the frame was written and `false` when a resize guard
/// deferred it.
pub(crate) fn write_frame(output: &str, frame: &RenderedFrame) -> Result<bool> {
    let jobs = &frame.jobs;
    let term = term();
    let mut lines = LINES.lock().unwrap();

    let _guard = TERM_LOCK.lock().unwrap();

    let term_size = term.size();
    let any_running = jobs.iter().any(|job| job.is_running());
    let resize_action = TERMINAL_RESIZE_STATE.lock().unwrap().update(
        term_size,
        *lines > 0,
        any_running,
        Instant::now(),
    );
    match resize_action {
        ResizeAction::ClearAndDefer => {
            let _sync = SyncUpdate::begin();
            reset_viewport(term, term_size.0 as usize)?;
            term.hide_cursor()?;
            *lines = 0;
            return Ok(false);
        }
        ResizeAction::Defer => return Ok(false),
        ResizeAction::None | ResizeAction::ClearAndRender => {}
    }

    let (term_height, term_width) = term_size;
    let term_height = term_height as usize;
    let term_width = term_width as usize;
    // A running frame that reaches the terminal height would scroll its first
    // row away (and a resize could push the anchored origin into scrollback
    // before clx receives SIGWINCH), so show the rows that fit and summarize
    // the rest. The final frame is always written in full.
    let (output, truncated) = if any_running {
        match fit_to_viewport(output, &frame.running_lines, term_width, term_height) {
            Some(fit) => fit,
            None => {
                // Not even one job fits, e.g. a single job that wraps to more
                // rows than the terminal has. Reset the visible viewport once
                // and suppress output until a frame fits again.
                let first_cramped =
                    !CRAMPED_VIEWPORT.swap(true, std::sync::atomic::Ordering::Relaxed);
                if *lines > 0 && first_cramped {
                    let _sync = SyncUpdate::begin();
                    reset_viewport(term, term_height)?;
                    term.hide_cursor()?;
                    *lines = 0;
                }
                return Ok(false);
            }
        }
    } else {
        (output.into(), false)
    };
    let output = output.as_ref();
    let output_height = rendered_height(output, term_width);

    CRAMPED_VIEWPORT.store(false, std::sync::atomic::Ordering::Relaxed);
    let _sync = SyncUpdate::begin();
    if resize_action == ResizeAction::ClearAndRender {
        // A terminal can reflow the old frame before clx observes its new
        // dimensions, moving some of that frame into inaccessible scrollback.
        // Reset the visible viewport so the replacement always starts from a
        // known position.
        reset_viewport(term, term_height)?;
        *lines = 0;
    } else if *lines > 0 {
        erase_rows_above(term, *lines)?;
    }

    if !output.is_empty() {
        diagnostics::log_frame(output, jobs);
        term.hide_cursor()?;
        term.write_line(output)?;

        *lines = output_height.max(1);
    } else {
        *lines = 0;
        term.show_cursor()?;
    }
    FRAME_TRUNCATED.store(truncated, std::sync::atomic::Ordering::Relaxed);

    Ok(true)
}

pub(crate) fn rendered_height(output: &str, width: usize) -> usize {
    output.lines().map(|line| line_height(line, width)).sum()
}

/// Rows a single line occupies, counting an empty line as one row.
fn line_height(line: &str, width: usize) -> usize {
    let visible_width = console::measure_text_width(line).max(1);
    if width == 0 {
        1
    } else {
        (visible_width - 1).checked_div(width).unwrap_or(0) + 1
    }
}

/// Cuts a running frame to at most `term_height - 1` rows, ending it with a
/// `… N more lines` row when lines were dropped.
///
/// Lines of running jobs are kept first, then the rest from the top, so a long
/// list of finished jobs cannot push the work in progress out of view.
/// `running` holds one entry per line of `output`.
///
/// Returns the frame to write and whether it was cut, or `None` when not even
/// one line fits.
fn fit_to_viewport<'a>(
    output: &'a str,
    running: &[bool],
    width: usize,
    term_height: usize,
) -> Option<(Cow<'a, str>, bool)> {
    // Leave the row below the frame for the cursor.
    let budget = term_height.saturating_sub(1);
    if rendered_height(output, width) <= budget {
        return Some((Cow::Borrowed(output), false));
    }

    let lines: Vec<&str> = output.lines().collect();
    let is_running = |i: usize| running.get(i).copied().unwrap_or(false);
    let marker_height = line_height(&format!("… {} more lines", lines.len()), width);
    let available = budget.saturating_sub(marker_height);
    let mut keep = vec![false; lines.len()];
    let mut used = 0;
    for want_running in [true, false] {
        for (i, line) in lines.iter().enumerate() {
            if is_running(i) != want_running {
                continue;
            }
            let height = line_height(line, width);
            if used + height > available {
                continue;
            }
            used += height;
            keep[i] = true;
        }
    }
    let kept = keep.iter().filter(|kept| **kept).count();
    if kept == 0 {
        return None;
    }

    let hidden = lines.len() - kept;
    let noun = if hidden == 1 { "line" } else { "lines" };
    let marker = style::edim(format!("… {hidden} more {noun}"));
    let mut fit = lines
        .iter()
        .zip(&keep)
        .filter(|(_, kept)| **kept)
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    fit.push('\n');
    fit.push_str(&marker.to_string());
    Some((Cow::Owned(fit), true))
}

pub(crate) fn cache_written_output(last_output: &mut String, output: &str, written: bool) {
    if written {
        output.clone_into(last_output);
    }
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
    let last_output = LAST_OUTPUT.lock().unwrap();
    let lines = *LINES.lock().unwrap();
    if final_frame_is_visible(
        any_running,
        &final_output,
        &last_output,
        lines,
        FRAME_TRUNCATED.load(std::sync::atomic::Ordering::Relaxed),
        viewport_resized(),
    ) {
        drop(last_output);
        if !any_running && !any_running_check() {
            super::state::finish_frame()?;
            *STARTED.lock().unwrap() = false;
            return Ok(false);
        }
        return Ok(true);
    }
    drop(last_output);

    let written = write_frame(&final_output, &frame)?;
    cache_written_output(&mut LAST_OUTPUT.lock().unwrap(), &final_output, written);

    if !any_running && !any_running_check() {
        super::state::finish_frame()?;
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
    refresh_once_locked()
}

pub(crate) fn refresh_once_locked() -> Result<()> {
    // The background refresh can finish after a terminal status update wakes it
    // but before that update reaches its synchronous refresh. In that case the
    // final frame is already visible and finish_frame() has reset LINES, so a
    // late write would append a duplicate instead of replacing the frame.
    if !*STARTED.lock().unwrap() {
        return Ok(());
    }

    let frame = render_frame()?;
    let final_output = process_flex_output(&frame.output);
    let any_running = frame.jobs.iter().any(|job| job.is_running());
    if final_frame_is_visible(
        any_running,
        &final_output,
        &LAST_OUTPUT.lock().unwrap(),
        *LINES.lock().unwrap(),
        FRAME_TRUNCATED.load(std::sync::atomic::Ordering::Relaxed),
        viewport_resized(),
    ) {
        return Ok(());
    }
    let written = write_frame(&final_output, &frame)?;
    cache_written_output(&mut LAST_OUTPUT.lock().unwrap(), &final_output, written);

    Ok(())
}

/// Returns `true` when redrawing would only repeat a settled frame.
///
/// A running frame that was cut to fit the terminal is never settled, even when
/// the full output is unchanged: the full frame still has to be written.
///
/// A resize since the frame was drawn always needs a redraw: the terminal may
/// have reflowed the old frame before clx observed the new size.
///
/// Redrawing moves the cursor up by the frame height, which cannot reach rows
/// that already scrolled off a frame taller than the terminal. Erasing and
/// rewriting such a frame would leave the top of the old copy in scrollback
/// above the new one.
fn final_frame_is_visible(
    any_running: bool,
    output: &str,
    last_output: &str,
    lines: usize,
    truncated: bool,
    resized: bool,
) -> bool {
    !any_running && lines > 0 && output == last_output && !truncated && !resized
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

/// Adds a raw template to the Tera engine, updating it if it already exists.
pub fn add_tera_template(tera: &mut Tera, name: &str, body: &str) -> Result<()> {
    tera.add_raw_template(name, body)?;
    Ok(())
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
        let final_output = if output.contains("<clx:flex>") {
            flex(&output, term().size().1 as usize)
        } else {
            output
        };
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

    #[test]
    fn rendered_height_tracks_terminal_reflow() {
        let output = "x".repeat(60);

        assert_eq!(rendered_height(&output, 20), 3);
        assert_eq!(rendered_height(&output, 80), 1);
    }

    #[test]
    fn rendered_height_ignores_ansi_width() {
        let output = format!("\x1b[31m{}\x1b[0m", "x".repeat(20));

        assert_eq!(rendered_height(&output, 20), 1);
    }

    #[test]
    fn deferred_frame_does_not_advance_output_cache() {
        let mut last_output = "visible frame".to_string();

        cache_written_output(&mut last_output, "deferred frame", false);
        assert_eq!(last_output, "visible frame");

        cache_written_output(&mut last_output, "written frame", true);
        assert_eq!(last_output, "written frame");
    }

    fn fit_running(
        output: &str,
        running: &[bool],
        width: usize,
        height: usize,
    ) -> Option<(String, bool)> {
        fit_to_viewport(output, running, width, height)
            .map(|(frame, cut)| (console::strip_ansi_codes(&frame).into_owned(), cut))
    }

    fn fit(output: &str, width: usize, height: usize) -> Option<(String, bool)> {
        fit_running(output, &[], width, height)
    }

    #[test]
    fn segments_flag_only_the_jobs_that_are_running() {
        use super::super::{ProgressJobBuilder, ProgressJobDoneBehavior, ProgressStatus};

        let parent = ProgressJobBuilder::new().body("parent").build();
        for (body, status) in [
            ("done-1", ProgressStatus::Done),
            ("done-2", ProgressStatus::Done),
            ("active", ProgressStatus::Running),
        ] {
            let child = ProgressJobBuilder::new()
                .body(body)
                .status(status)
                .on_done(ProgressJobDoneBehavior::Keep)
                .build();
            parent.children.lock().unwrap().push(Arc::new(child));
        }

        let mut tera = Tera::default();
        let segments = parent
            .render_segments(&mut tera, RenderContext::default())
            .unwrap();
        let rendered: Vec<_> = segments
            .iter()
            .map(|segment| (segment.text.trim(), segment.running))
            .collect();
        assert_eq!(
            rendered,
            [
                ("parent", true),
                ("done-1", false),
                ("done-2", false),
                ("active", true)
            ]
        );
        let joined = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            joined,
            parent.render(&mut tera, RenderContext::default()).unwrap()
        );
    }

    #[test]
    fn running_lines_stay_visible_when_finished_ones_are_cut() {
        // Three finished jobs above the one still running.
        assert_eq!(
            fit_running("a\nb\nc\nd", &[false, false, false, true], 80, 4),
            Some(("a\nd\n… 2 more lines".to_string(), true))
        );
    }

    #[test]
    fn a_running_line_too_tall_to_fit_does_not_hide_shorter_ones() {
        let wide = "x".repeat(30);
        assert_eq!(
            fit_running(
                &format!("{wide}\nb\nc\nd"),
                &[true, true, false, false],
                20,
                3
            ),
            Some(("b\n… 3 more lines".to_string(), true))
        );
    }

    #[test]
    fn blank_lines_take_a_row_of_the_budget() {
        assert_eq!(
            fit("a\n\nb\nc", 80, 4),
            Some(("a\n\n… 2 more lines".to_string(), true))
        );
        assert_eq!(
            fit("a\n\n\nb", 80, 5),
            Some(("a\n\n\nb".to_string(), false))
        );
        assert_eq!(rendered_height("a\n\nb", 80), 3);
    }

    #[test]
    fn frame_shorter_than_the_terminal_is_unchanged() {
        assert_eq!(fit("a\nb\nc", 80, 4), Some(("a\nb\nc".to_string(), false)));
    }

    #[test]
    fn frame_reaching_the_terminal_height_is_cut_with_a_marker() {
        // Four rows leave three for the frame; one of those is the marker.
        assert_eq!(
            fit("a\nb\nc\nd", 80, 4),
            Some(("a\nb\n… 2 more lines".to_string(), true))
        );
        // A wrapped line can be the only one hidden.
        let wide = "x".repeat(30);
        assert_eq!(
            fit(&format!("a\nb\n{wide}"), 20, 4),
            Some(("a\nb\n… 1 more line".to_string(), true))
        );
    }

    #[test]
    fn wrapped_lines_count_by_their_rendered_height() {
        let wide = "x".repeat(30);
        let output = format!("a\n{wide}\nb");
        // Rows: 1 + 2 + 1 = 4, so at 5 rows the frame is one row below the limit.
        assert_eq!(fit(&output, 20, 5).map(|(_, cut)| cut), Some(false));
        assert_eq!(
            fit(&output, 20, 4),
            // The wrapped line is skipped, but the short one after it still fits.
            Some(("a\nb\n… 1 more line".to_string(), true))
        );
    }

    #[test]
    fn frame_whose_first_line_does_not_fit_is_suppressed() {
        assert_eq!(fit(&"x".repeat(60), 20, 3), None);
        assert_eq!(fit("a\nb", 80, 2), None);
    }

    #[test]
    fn settled_frame_is_not_redrawn() {
        assert!(final_frame_is_visible(
            false, "frame", "frame", 30, false, false
        ));
        // Spinners animate while a job runs.
        assert!(!final_frame_is_visible(
            true, "frame", "frame", 30, false, false
        ));
        assert!(!final_frame_is_visible(
            false, "new", "frame", 30, false, false
        ));
        // Nothing is on screen (e.g. after a resize cleared it).
        assert!(!final_frame_is_visible(
            false, "frame", "frame", 0, false, false
        ));
        // The terminal changed size since the frame was drawn.
        assert!(!final_frame_is_visible(
            false, "frame", "frame", 30, false, true
        ));
        // The screen shows a cut running frame, not the settled one.
        assert!(!final_frame_is_visible(
            false, "frame", "frame", 30, true, false
        ));
    }

    #[test]
    fn synchronous_refresh_skips_after_background_stops() {
        let previous_started = std::mem::replace(&mut *STARTED.lock().unwrap(), false);
        let mut last_output = LAST_OUTPUT.lock().unwrap();
        let previous_output = std::mem::replace(&mut *last_output, "visible final frame".into());
        drop(last_output);

        refresh_once_locked().unwrap();

        let mut last_output = LAST_OUTPUT.lock().unwrap();
        assert_eq!(*last_output, "visible final frame");
        *last_output = previous_output;
        *STARTED.lock().unwrap() = previous_started;
    }

    #[test]
    fn active_resize_clears_then_waits_for_the_size_to_settle() {
        let mut state = TerminalResizeState::default();
        let start = Instant::now();

        assert_eq!(
            state.update((24, 80), false, true, start),
            ResizeAction::None
        );
        assert_eq!(
            state.update((24, 60), true, true, start),
            ResizeAction::ClearAndDefer
        );
        assert_eq!(
            state.update(
                (24, 60),
                false,
                true,
                start + RESIZE_SETTLE_TIME - Duration::from_millis(1)
            ),
            ResizeAction::Defer
        );
        assert_eq!(
            state.update((24, 60), false, true, start + RESIZE_SETTLE_TIME),
            ResizeAction::ClearAndRender
        );
    }
}
