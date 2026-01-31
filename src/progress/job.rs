//! Progress job types and builder.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use serde::ser::Serialize as SerializeTrait;
use tera::{Context, Tera};

use crate::Result;

use super::flex::flex;
use super::output::{ProgressOutput, output};
use super::render::{RenderContext, add_tera_template, indent, render_text_mode};
use super::spinners::DEFAULT_BODY;
use super::state::{JOBS, STOPPING, TERM_LOCK, is_disabled, notify, term};
use super::tera_setup::add_tera_functions;

/// Status of a progress job.
///
/// The status determines how the job is displayed (spinner icon, colors) and
/// whether it's considered "active" (still running).
#[derive(Debug, Default, Clone, PartialEq, strum::EnumIs)]
pub enum ProgressStatus {
    /// Hidden status - the job is not displayed.
    Hide,
    /// Paused/pending status - shows a pause indicator.
    Pending,
    /// Running status (default) - shows an animated spinner.
    #[default]
    Running,
    /// Running with a custom spinner character.
    RunningCustom(String),
    /// Done with a custom completion character.
    DoneCustom(String),
    /// Successfully completed - shows a green checkmark.
    Done,
    /// Completed with warnings - shows a yellow warning icon.
    Warn,
    /// Failed - shows a red X.
    Failed,
}

impl ProgressStatus {
    /// Returns `true` if the job is still active (running).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::RunningCustom(_))
    }
}

/// Behavior when a progress job completes.
#[derive(Debug, Default, PartialEq)]
pub enum ProgressJobDoneBehavior {
    /// Keep the job and all children visible (default).
    #[default]
    Keep,
    /// Keep the job visible but hide its children.
    Collapse,
    /// Remove the job from display entirely.
    Hide,
}

/// Builder for creating progress jobs.
#[must_use]
pub struct ProgressJobBuilder {
    body: String,
    body_text: Option<String>,
    status: ProgressStatus,
    ctx: Context,
    on_done: ProgressJobDoneBehavior,
    progress_current: Option<usize>,
    progress_total: Option<usize>,
}

impl Default for ProgressJobBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ProgressJobBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressJobBuilder")
            .field("body", &self.body)
            .field("body_text", &self.body_text)
            .field("status", &self.status)
            .field("on_done", &self.on_done)
            .field("progress_current", &self.progress_current)
            .field("progress_total", &self.progress_total)
            .finish_non_exhaustive()
    }
}

impl ProgressJobBuilder {
    /// Creates a new progress job builder with default settings.
    pub fn new() -> Self {
        Self {
            body: DEFAULT_BODY.clone(),
            body_text: None,
            status: Default::default(),
            ctx: Default::default(),
            on_done: Default::default(),
            progress_current: None,
            progress_total: None,
        }
    }

    /// Sets the Tera template for rendering the job body.
    pub fn body<S: Into<String>>(mut self, body: S) -> Self {
        self.body = body.into();
        self
    }

    /// Sets an alternative template for text output mode.
    pub fn body_text(mut self, body: Option<impl Into<String>>) -> Self {
        self.body_text = body.map(|s| s.into());
        self
    }

    /// Sets the initial status of the job.
    pub fn status(mut self, status: ProgressStatus) -> Self {
        self.status = status;
        self
    }

    /// Sets the behavior when the job completes.
    pub fn on_done(mut self, on_done: ProgressJobDoneBehavior) -> Self {
        self.on_done = on_done;
        self
    }

    /// Sets the current progress value.
    pub fn progress_current(mut self, progress_current: usize) -> Self {
        self.progress_current = Some(progress_current);
        self.prop("cur", &progress_current)
    }

    /// Sets the total progress value.
    pub fn progress_total(mut self, progress_total: usize) -> Self {
        self.progress_total = Some(progress_total);
        self.prop("total", &progress_total)
    }

    /// Sets a template property (variable).
    pub fn prop<T: SerializeTrait + ?Sized, S: Into<String>>(mut self, key: S, val: &T) -> Self {
        self.ctx.insert(key, val);
        self
    }

    /// Builds the progress job without starting it.
    #[must_use = "the returned ProgressJob should be used or stored"]
    pub fn build(self) -> ProgressJob {
        static ID: AtomicUsize = AtomicUsize::new(0);
        ProgressJob {
            id: ID.fetch_add(1, Ordering::Relaxed),
            body: Mutex::new(self.body),
            body_text: self.body_text,
            status: Mutex::new(self.status),
            on_done: self.on_done,
            parent: Weak::new(),
            children: Mutex::new(vec![]),
            tera_ctx: Mutex::new(self.ctx),
            progress_current: Mutex::new(self.progress_current),
            progress_total: Mutex::new(self.progress_total),
            start: Instant::now(),
            last_progress_update: Mutex::new(None),
            smoothed_rate: Mutex::new(None),
        }
    }

    /// Builds and starts the progress job as a top-level job.
    #[must_use = "the returned job handle is needed to control the job"]
    pub fn start(self) -> Arc<ProgressJob> {
        let job = Arc::new(self.build());
        JOBS.lock().unwrap().push(job.clone());
        job.update();
        job
    }
}

/// A progress job handle for updating and controlling an active progress indicator.
pub struct ProgressJob {
    pub(crate) id: usize,
    pub(crate) body: Mutex<String>,
    pub(crate) body_text: Option<String>,
    pub(crate) status: Mutex<ProgressStatus>,
    pub(crate) parent: Weak<ProgressJob>,
    pub(crate) children: Mutex<Vec<Arc<ProgressJob>>>,
    pub(crate) tera_ctx: Mutex<Context>,
    pub(crate) on_done: ProgressJobDoneBehavior,
    pub(crate) progress_current: Mutex<Option<usize>>,
    pub(crate) progress_total: Mutex<Option<usize>>,
    pub(crate) start: Instant,
    /// Last progress update time and value (for rate calculation)
    pub(crate) last_progress_update: Mutex<Option<(Instant, usize)>>,
    /// Exponentially smoothed rate (items per second)
    pub(crate) smoothed_rate: Mutex<Option<f64>>,
}

impl ProgressJob {
    /// Renders this job to a string using the given Tera engine and context.
    pub(crate) fn render(&self, tera: &mut Tera, mut ctx: RenderContext) -> Result<String> {
        let mut s = vec![];
        ctx.tera_ctx.extend(self.tera_ctx.lock().unwrap().clone());
        ctx.progress = if let (Some(progress_current), Some(progress_total)) = (
            *self.progress_current.lock().unwrap(),
            *self.progress_total.lock().unwrap(),
        ) {
            Some((progress_current, progress_total))
        } else {
            None
        };
        add_tera_functions(tera, &ctx, self);
        if !self.should_display() {
            return Ok(String::new());
        }
        let body = if output() == ProgressOutput::Text {
            self.body_text
                .clone()
                .unwrap_or(self.body.lock().unwrap().clone())
        } else {
            self.body.lock().unwrap().clone()
        };
        let name = format!("progress_{}", self.id);
        add_tera_template(tera, &name, &body)?;
        let rendered_body = tera.render(&name, &ctx.tera_ctx)?;
        let flex_width = ctx.width.saturating_sub(ctx.indent);
        let body = flex(&rendered_body, flex_width);
        s.push(body.trim_end().to_string());
        if ctx.include_children && self.should_display_children() {
            ctx.indent += 1;
            let children = self.children.lock().unwrap();
            for child in children.iter() {
                let child_output = child.render(tera, ctx.clone())?;
                if !child_output.is_empty() {
                    let child_output = indent(child_output, ctx.width - ctx.indent + 1, ctx.indent);
                    s.push(child_output);
                }
            }
        }
        Ok(s.join("\n"))
    }

    fn should_display(&self) -> bool {
        let status = self.status.lock().unwrap();
        !status.is_hide() && (status.is_active() || self.on_done != ProgressJobDoneBehavior::Hide)
    }

    fn should_display_children(&self) -> bool {
        self.status.lock().unwrap().is_active() || self.on_done == ProgressJobDoneBehavior::Keep
    }

    /// Adds a child job to this job.
    pub fn add(self: &Arc<Self>, mut job: ProgressJob) -> Arc<Self> {
        job.parent = Arc::downgrade(self);
        let job = Arc::new(job);
        self.children.lock().unwrap().push(job.clone());
        job.update();
        job
    }

    /// Removes this job from the display.
    pub fn remove(&self) {
        if let Some(parent) = self.parent.upgrade() {
            parent
                .children
                .lock()
                .unwrap()
                .retain(|child| child.id != self.id);
        } else {
            JOBS.lock().unwrap().retain(|job| job.id != self.id);
        }
    }

    /// Returns a clone of the children jobs list.
    #[must_use]
    pub fn children(&self) -> Vec<Arc<Self>> {
        self.children.lock().unwrap().clone()
    }

    /// Returns `true` if the job is still running (active).
    pub fn is_running(&self) -> bool {
        self.status.lock().unwrap().is_active()
    }

    /// Replaces the job's Tera template body.
    pub fn set_body<S: Into<String>>(&self, body: S) {
        *self.body.lock().unwrap() = body.into();
        self.update();
    }

    /// Sets the job's status.
    pub fn set_status(&self, status: ProgressStatus) {
        let mut s = self.status.lock().unwrap();
        if *s != status {
            *s = status.clone();
            drop(s);
            self.update();
            // For terminal states, do a synchronous render
            if matches!(
                status,
                ProgressStatus::Done
                    | ProgressStatus::Failed
                    | ProgressStatus::Warn
                    | ProgressStatus::DoneCustom(_)
            ) {
                let _ = super::render::refresh_once();
            }
        }
    }

    /// Sets a template property (variable).
    pub fn prop<T: SerializeTrait + ?Sized, S: Into<String>>(&self, key: S, val: &T) {
        let mut ctx = self.tera_ctx.lock().unwrap();
        ctx.insert(key, val);
        drop(ctx);
        self.update();
    }

    /// Updates the current progress value.
    pub fn progress_current(&self, mut current: usize) {
        if let Some(total) = *self.progress_total.lock().unwrap() {
            current = current.min(total);
        }

        self.update_smoothed_rate(current);

        *self.progress_current.lock().unwrap() = Some(current);
        self.prop("cur", &current);
    }

    /// Updates the total progress value.
    pub fn progress_total(&self, mut total: usize) {
        if let Some(current) = *self.progress_current.lock().unwrap() {
            total = total.max(current);
        }
        *self.progress_total.lock().unwrap() = Some(total);
        self.prop("total", &total);
    }

    /// Increments the current progress value by the specified amount.
    pub fn increment(&self, n: usize) {
        // Hold lock throughout read-modify-write to prevent race conditions
        let mut current_guard = self.progress_current.lock().unwrap();
        let current = current_guard.unwrap_or(0);
        let mut new_current = current.saturating_add(n);

        if let Some(total) = *self.progress_total.lock().unwrap() {
            new_current = new_current.min(total);
        }

        self.update_smoothed_rate(new_current);

        *current_guard = Some(new_current);
        drop(current_guard);

        self.prop("cur", &new_current);
    }

    /// Helper to update the smoothed rate based on progress change.
    fn update_smoothed_rate(&self, current: usize) {
        let now = Instant::now();
        let mut last_update = self.last_progress_update.lock().unwrap();
        if let Some((last_time, last_value)) = *last_update {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            if elapsed > 0.001 && current > last_value {
                let items_processed = (current - last_value) as f64;
                let instantaneous_rate = items_processed / elapsed;

                const ALPHA: f64 = 0.3;
                let mut smoothed = self.smoothed_rate.lock().unwrap();
                *smoothed = Some(match *smoothed {
                    Some(old_rate) => ALPHA * instantaneous_rate + (1.0 - ALPHA) * old_rate,
                    None => instantaneous_rate,
                });
            }
        }
        *last_update = Some((now, current));
    }

    /// Sets the message property.
    pub fn message(&self, msg: &str) {
        self.prop("message", msg);
    }

    /// Triggers a display update for this job.
    pub fn update(&self) {
        if is_disabled() || STOPPING.load(Ordering::Relaxed) {
            return;
        }
        if output() == ProgressOutput::Text {
            if let Err(e) = render_text_mode(self) {
                eprintln!("clx: {e:?}");
            }
        } else {
            notify();
        }
    }

    /// Prints a line to stderr without interfering with the progress display.
    pub fn println(&self, s: &str) {
        if !s.is_empty() {
            super::state::pause();
            let output = if s.contains("<clx:flex>") {
                flex(s, term().size().1 as usize)
            } else {
                s.to_string()
            };
            let _guard = TERM_LOCK.lock().unwrap();
            let _ = term().write_line(&output);
            drop(_guard);
            super::state::resume();
        }
    }
}

impl fmt::Debug for ProgressJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProgressJob {{ id: {}, status: {:?} }}",
            self.id,
            self.status.lock().unwrap()
        )
    }
}

impl PartialEq for ProgressJob {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ProgressJob {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_job_builder_default() {
        let builder = ProgressJobBuilder::new();
        let job = builder.build();
        assert_eq!(*job.status.lock().unwrap(), ProgressStatus::Running);
        assert!(job.progress_current.lock().unwrap().is_none());
        assert!(job.progress_total.lock().unwrap().is_none());
    }

    #[test]
    fn test_progress_job_builder_with_props() {
        let job = ProgressJobBuilder::new()
            .prop("message", "test message")
            .status(ProgressStatus::Pending)
            .progress_current(5)
            .progress_total(10)
            .on_done(ProgressJobDoneBehavior::Hide)
            .build();

        assert_eq!(*job.status.lock().unwrap(), ProgressStatus::Pending);
        assert_eq!(*job.progress_current.lock().unwrap(), Some(5));
        assert_eq!(*job.progress_total.lock().unwrap(), Some(10));
        assert_eq!(job.on_done, ProgressJobDoneBehavior::Hide);
    }

    #[test]
    fn test_progress_job_builder_body() {
        let job = ProgressJobBuilder::new()
            .body("custom template {{ message }}")
            .build();

        assert_eq!(*job.body.lock().unwrap(), "custom template {{ message }}");
    }

    #[test]
    fn test_progress_job_builder_body_text() {
        let job = ProgressJobBuilder::new()
            .body_text(Some("text mode output"))
            .build();

        assert_eq!(job.body_text, Some("text mode output".to_string()));
    }

    #[test]
    fn test_progress_status_is_active() {
        assert!(ProgressStatus::Running.is_active());
        assert!(ProgressStatus::RunningCustom("custom".to_string()).is_active());
        assert!(!ProgressStatus::Done.is_active());
        assert!(!ProgressStatus::Failed.is_active());
        assert!(!ProgressStatus::Pending.is_active());
        assert!(!ProgressStatus::Hide.is_active());
        assert!(!ProgressStatus::Warn.is_active());
        assert!(!ProgressStatus::DoneCustom("custom".to_string()).is_active());
    }

    #[test]
    fn test_progress_status_transitions() {
        let job = ProgressJobBuilder::new().build();

        assert!(job.status.lock().unwrap().is_running());
        assert!(job.is_running());

        job.set_status(ProgressStatus::Done);
        assert!(job.status.lock().unwrap().is_done());
        assert!(!job.is_running());

        job.set_status(ProgressStatus::Failed);
        assert!(job.status.lock().unwrap().is_failed());

        job.set_status(ProgressStatus::Pending);
        assert!(job.status.lock().unwrap().is_pending());

        job.set_status(ProgressStatus::Running);
        assert!(job.is_running());
    }

    #[test]
    fn test_progress_job_set_body() {
        let job = ProgressJobBuilder::new().build();
        assert_eq!(*job.body.lock().unwrap(), *DEFAULT_BODY);

        job.set_body("new body template");
        assert_eq!(*job.body.lock().unwrap(), "new body template");
    }

    #[test]
    fn test_progress_job_progress_updates() {
        let job = ProgressJobBuilder::new().progress_total(100).build();

        assert_eq!(*job.progress_total.lock().unwrap(), Some(100));
        assert!(job.progress_current.lock().unwrap().is_none());

        job.progress_current(50);
        assert_eq!(*job.progress_current.lock().unwrap(), Some(50));

        job.progress_current(150);
        assert_eq!(*job.progress_current.lock().unwrap(), Some(100));
    }

    #[test]
    fn test_progress_job_progress_total_update() {
        let job = ProgressJobBuilder::new().progress_current(80).build();

        job.progress_total(50);
        assert_eq!(*job.progress_total.lock().unwrap(), Some(80));
    }

    #[test]
    fn test_progress_job_equality() {
        let job1 = ProgressJobBuilder::new().build();
        let job2 = ProgressJobBuilder::new().build();

        assert_ne!(job1, job2);
        assert_eq!(job1, job1);
    }

    #[test]
    fn test_progress_job_done_behavior() {
        assert_eq!(
            ProgressJobDoneBehavior::default(),
            ProgressJobDoneBehavior::Keep
        );
    }

    #[test]
    fn test_progress_job_debug() {
        let job = ProgressJobBuilder::new().build();
        let debug_str = format!("{:?}", job);
        assert!(debug_str.contains("ProgressJob"));
        assert!(debug_str.contains("id:"));
        assert!(debug_str.contains("Running"));
    }
}
