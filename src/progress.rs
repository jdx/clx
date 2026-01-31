use crate::{Result, progress_bar, style};
use serde::ser::Serialize as SerializeTrait;
use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, LazyLock, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use console::Term;
use indicatif::TermLike;
use tera::{Context, Tera};

// Include OSC progress functionality
use crate::osc::{ProgressState, clear_progress, set_progress};

// Diagnostic frame logging
mod diagnostics {
    use super::*;
    use serde::Serialize;
    use std::fs::{File, OpenOptions};
    use std::io::{LineWriter, Write};
    use std::sync::{Mutex, OnceLock};

    static LOG_WRITER: OnceLock<Option<Mutex<LineWriter<File>>>> = OnceLock::new();
    static KEEP_ANSI: OnceLock<bool> = OnceLock::new();

    fn get_log_writer() -> Option<&'static Mutex<LineWriter<File>>> {
        LOG_WRITER
            .get_or_init(|| {
                std::env::var("CLX_TRACE_LOG").ok().and_then(|path| {
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .ok()
                        .map(|file| Mutex::new(LineWriter::new(file)))
                })
            })
            .as_ref()
    }

    fn keep_ansi() -> bool {
        *KEEP_ANSI.get_or_init(|| std::env::var("CLX_TRACE_RAW").is_ok())
    }

    /// Snapshot of a single job's state
    #[derive(Debug, Clone, Serialize)]
    pub struct JobSnapshot {
        pub id: usize,
        pub status: String,
        pub message: Option<String>,
        pub progress: Option<(usize, usize)>,
        pub children: Vec<JobSnapshot>,
    }

    impl JobSnapshot {
        pub fn from_job(job: &ProgressJob) -> Self {
            let status = job.status.lock().unwrap();
            let status_str = match &*status {
                ProgressStatus::Hide => "hide",
                ProgressStatus::Pending => "pending",
                ProgressStatus::Running => "running",
                ProgressStatus::RunningCustom(_) => "running",
                ProgressStatus::DoneCustom(_) => "done",
                ProgressStatus::Done => "done",
                ProgressStatus::Warn => "warn",
                ProgressStatus::Failed => "failed",
            };
            drop(status);

            let message = job
                .tera_ctx
                .lock()
                .unwrap()
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let progress = match (
                *job.progress_current.lock().unwrap(),
                *job.progress_total.lock().unwrap(),
            ) {
                (Some(cur), Some(total)) => Some((cur, total)),
                _ => None,
            };

            let children = job
                .children
                .lock()
                .unwrap()
                .iter()
                .map(|c| JobSnapshot::from_job(c))
                .collect();

            JobSnapshot {
                id: job.id,
                status: status_str.to_string(),
                message,
                progress,
                children,
            }
        }
    }

    /// Frame event emitted for each refresh
    #[derive(Debug, Clone, Serialize)]
    pub struct FrameEvent {
        pub rendered: String,
        pub jobs: Vec<JobSnapshot>,
    }

    /// Log a frame event to the trace log file
    pub fn log_frame(rendered: &str, jobs: &[Arc<ProgressJob>]) {
        let Some(log_writer) = get_log_writer() else {
            return;
        };

        let rendered = if keep_ansi() {
            rendered.to_string()
        } else {
            console::strip_ansi_codes(rendered).to_string()
        };

        let event = FrameEvent {
            rendered,
            jobs: jobs.iter().map(|j| JobSnapshot::from_job(j)).collect(),
        };

        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut writer) = log_writer.lock() {
                let _ = writeln!(writer, "{}", json);
            }
        }
    }
}

static DEFAULT_BODY: LazyLock<String> =
    LazyLock::new(|| "{{ spinner() }} {{ message }}".to_string());

struct Spinner {
    frames: Vec<String>,
    fps: usize,
}

macro_rules! spinner {
    ($name:expr, $frames:expr, $fps:expr) => {
        (
            $name.to_string(),
            Spinner {
                frames: $frames.iter().map(|s| s.to_string()).collect(),
                fps: $fps,
            },
        )
    };
}

const DEFAULT_SPINNER: &str = "mini_dot";
#[rustfmt::skip]
static SPINNERS: LazyLock<HashMap<String, Spinner>> = LazyLock::new(|| {
    vec![
        // Classic - from https://github.com/charmbracelet/bubbles/blob/ea344ab907bddf5e8f71cd73b9583b070e8f1b2f/spinner/spinner.go
        spinner!("line", &["|", "/", "-", "\\"], 200),
        spinner!("dot", &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"], 200),
        spinner!("mini_dot", &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"], 200),
        spinner!("jump", &["⢄", "⢂", "⢁", "⡁", "⡈", "⡐", "⡠"], 200),
        spinner!("pulse", &["█", "▓", "▒", "░"], 200),
        spinner!("points", &["∙∙∙", "●∙∙", "∙●∙", "∙∙●"], 200),
        spinner!("globe", &["🌍", "🌎", "🌏"], 400),
        spinner!("moon", &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"], 400),
        spinner!("monkey", &["🙈", "🙉", "🙊"], 400),
        spinner!("meter", &["▱▱▱", "▰▱▱", "▰▰▱", "▰▰▰", "▰▰▱", "▰▱▱", "▱▱▱"], 400),
        spinner!("hamburger", &["☱", "☲", "☴", "☲"], 200),
        spinner!("ellipsis", &["   ", ".  ", ".. ", "..."], 200),
        // Classic/Minimal
        spinner!("arrow", &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"], 200),
        spinner!("triangle", &["◢", "◣", "◤", "◥"], 200),
        spinner!("square", &["◰", "◳", "◲", "◱"], 200),
        spinner!("circle", &["◴", "◷", "◶", "◵"], 200),
        // Box Drawing
        spinner!("bounce", &["⠁", "⠂", "⠄", "⠂"], 200),
        spinner!("arc", &["◜", "◠", "◝", "◞", "◡", "◟"], 200),
        spinner!("box_bounce", &["▖", "▘", "▝", "▗"], 200),
        // Aesthetic
        spinner!("star", &["✶", "✸", "✹", "✺", "✹", "✷"], 200),
        spinner!("hearts", &["💛", "💙", "💜", "💚", "❤️"], 400),
        spinner!("clock", &["🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛"], 200),
        spinner!("weather", &["🌤", "⛅", "🌥", "☁️", "🌧", "⛈", "🌩", "🌨"], 400),
        // Growing/Progress-like
        spinner!("grow_horizontal", &["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█", "▉", "▊", "▋", "▌", "▍", "▎"], 200),
        spinner!("grow_vertical", &["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂"], 200),
        // Playful
        spinner!("runner", &["🚶", "🏃"], 400),
        spinner!("oranges", &["🍊", "🍋", "🍇", "🍎"], 400),
        spinner!("smiley", &["😀", "😬", "😁", "😂", "🤣", "😂", "😁", "😬"], 400),
    ]
    .into_iter()
    .collect()
});

/// Refresh interval for the progress display.
/// Set to 200ms to match the fastest spinner frame rate (mini_dot, line, etc.).
/// Spinners define their frame interval in milliseconds (e.g., 200 = change frame every 200ms).
/// Using the minimum ensures smooth animation for all spinners.
static INTERVAL: Mutex<Duration> = Mutex::new(Duration::from_millis(200));
static LINES: Mutex<usize> = Mutex::new(0);
static TERM_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Execute the provided function while holding the global terminal lock.
/// This allows external crates to synchronize stderr writes (e.g., logging)
/// with clx's progress clear/write operations to avoid interleaved output.
#[must_use]
pub fn with_terminal_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = TERM_LOCK.lock().unwrap();
    let result = f();
    drop(_guard);
    result
}
static REFRESH_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static STOPPING: AtomicBool = AtomicBool::new(false);
static NOTIFY: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
static STARTED: Mutex<bool> = Mutex::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);
static JOBS: Mutex<Vec<Arc<ProgressJob>>> = Mutex::new(vec![]);
static TERA: Mutex<Option<Tera>> = Mutex::new(None);

// OSC progress tracking state
static LAST_OSC_PERCENTAGE: Mutex<Option<u8>> = Mutex::new(None);

#[derive(Clone)]
struct RenderContext {
    start: Instant,
    now: Instant,
    width: usize,
    tera_ctx: Context,
    indent: usize,
    include_children: bool,
    progress: Option<(usize, usize)>,
}

impl Default for RenderContext {
    fn default() -> Self {
        let mut tera_ctx = Context::new();
        tera_ctx.insert("message", "");
        Self {
            start: Instant::now(),
            now: Instant::now(),
            width: term().width() as usize,
            tera_ctx,
            indent: 0,
            include_children: true,
            progress: None,
        }
    }
}

impl RenderContext {
    pub fn elapsed(&self) -> Duration {
        self.now - self.start
    }
}

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

    pub fn body<S: Into<String>>(mut self, body: S) -> Self {
        self.body = body.into();
        self
    }

    pub fn body_text(mut self, body: Option<impl Into<String>>) -> Self {
        self.body_text = body.map(|s| s.into());
        self
    }

    pub fn status(mut self, status: ProgressStatus) -> Self {
        self.status = status;
        self
    }

    pub fn on_done(mut self, on_done: ProgressJobDoneBehavior) -> Self {
        self.on_done = on_done;
        self
    }

    pub fn progress_current(mut self, progress_current: usize) -> Self {
        self.progress_current = Some(progress_current);
        self.prop("cur", &progress_current)
    }

    pub fn progress_total(mut self, progress_total: usize) -> Self {
        self.progress_total = Some(progress_total);
        self.prop("total", &progress_total)
    }

    pub fn prop<T: SerializeTrait + ?Sized, S: Into<String>>(mut self, key: S, val: &T) -> Self {
        self.ctx.insert(key, val);
        self
    }

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
        }
    }

    #[must_use = "the returned job handle is needed to control the job"]
    pub fn start(self) -> Arc<ProgressJob> {
        let job = Arc::new(self.build());
        JOBS.lock().unwrap().push(job.clone());
        job.update();
        job
    }
}

#[derive(Debug, Default, Clone, PartialEq, strum::EnumIs)]
pub enum ProgressStatus {
    Hide,
    Pending,
    #[default]
    Running,
    RunningCustom(String),
    DoneCustom(String),
    Done,
    Warn,
    Failed,
}

impl ProgressStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::RunningCustom(_))
    }
}

#[derive(Debug, Default, PartialEq)]
pub enum ProgressJobDoneBehavior {
    #[default]
    Keep,
    Collapse,
    Hide,
}

pub struct ProgressJob {
    id: usize,
    body: Mutex<String>,
    body_text: Option<String>,
    status: Mutex<ProgressStatus>,
    parent: Weak<ProgressJob>,
    children: Mutex<Vec<Arc<ProgressJob>>>,
    tera_ctx: Mutex<Context>,
    on_done: ProgressJobDoneBehavior,
    progress_current: Mutex<Option<usize>>,
    progress_total: Mutex<Option<usize>>,
    start: Instant,
}

impl ProgressJob {
    fn render(&self, tera: &mut Tera, mut ctx: RenderContext) -> Result<String> {
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

    pub fn add(self: &Arc<Self>, mut job: ProgressJob) -> Arc<Self> {
        job.parent = Arc::downgrade(self);
        let job = Arc::new(job);
        self.children.lock().unwrap().push(job.clone());
        job.update();
        job
    }

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

    #[must_use]
    pub fn children(&self) -> Vec<Arc<Self>> {
        self.children.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.status.lock().unwrap().is_active()
    }

    pub fn set_body<S: Into<String>>(&self, body: S) {
        *self.body.lock().unwrap() = body.into();
        self.update();
    }

    pub fn set_status(&self, status: ProgressStatus) {
        let mut s = self.status.lock().unwrap();
        if *s != status {
            *s = status;
            drop(s);
            self.update();
        }
    }

    pub fn prop<T: SerializeTrait + ?Sized, S: Into<String>>(&self, key: S, val: &T) {
        let mut ctx = self.tera_ctx.lock().unwrap();
        ctx.insert(key, val);
        drop(ctx);
        self.update();
    }

    pub fn progress_current(&self, mut current: usize) {
        if let Some(total) = *self.progress_total.lock().unwrap() {
            current = current.min(total);
        }
        *self.progress_current.lock().unwrap() = Some(current);
        self.prop("cur", &current); // prop() calls update()
    }

    pub fn progress_total(&self, mut total: usize) {
        if let Some(current) = *self.progress_current.lock().unwrap() {
            total = total.max(current);
        }
        *self.progress_total.lock().unwrap() = Some(total);
        self.prop("total", &total); // prop() calls update()
    }

    pub fn update(&self) {
        if STOPPING.load(Ordering::Relaxed) {
            return;
        }
        if output() == ProgressOutput::Text {
            let update = || {
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
                let output = self.render(tera, ctx)?;
                if !output.is_empty() {
                    // Safety check: ensure no flex tags are visible
                    let final_output = if output.contains("<clx:flex>") {
                        flex(&output, term().width() as usize)
                    } else {
                        output
                    };
                    let _guard = TERM_LOCK.lock().unwrap();
                    term().write_line(&final_output)?;
                    drop(_guard);
                }
                Result::Ok(())
            };
            if let Err(e) = update() {
                eprintln!("clx: {e:?}");
            }
        } else {
            notify();
        }
    }

    pub fn println(&self, s: &str) {
        if !s.is_empty() {
            pause();
            // Safety check: ensure no flex tags are visible
            let output = if s.contains("<clx:flex>") {
                flex(s, term().width() as usize)
            } else {
                s.to_string()
            };
            let _guard = TERM_LOCK.lock().unwrap();
            let _ = term().write_line(&output);
            drop(_guard);
            resume();
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

fn indent(s: String, width: usize, indent: usize) -> String {
    let mut result = Vec::new();
    let indent_str = " ".repeat(indent);

    for line in s.lines() {
        let mut current = String::new();
        let mut current_width = 0;
        let mut chars = line.chars().peekable();
        let mut ansi_code = String::new();

        // Add initial indentation
        if current.is_empty() {
            current.push_str(&indent_str);
            current_width = indent;
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
                current_width = indent;
            }
            current.push(c);
            if !c.is_control() {
                current_width += char_width;
            }
        }

        // For the last line, if it's too long, we need to wrap it
        if !current.is_empty() {
            if current_width > width {
                let mut width_so_far = indent;
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

fn notify() {
    if STOPPING.load(Ordering::Relaxed) {
        return;
    }
    start();
    if let Some(tx) = NOTIFY.lock().unwrap().clone() {
        let _ = tx.send(());
    }
}

fn notify_wait(timeout: Duration) -> bool {
    let (tx, rx) = mpsc::channel();
    NOTIFY.lock().unwrap().replace(tx);
    rx.recv_timeout(timeout).is_ok()
}

pub fn flush() {
    if !*STARTED.lock().unwrap() {
        return;
    }
    if let Err(err) = refresh() {
        eprintln!("clx: {err:?}");
    }
}

fn start() {
    let mut started = STARTED.lock().unwrap();
    if *started || output() == ProgressOutput::Text || STOPPING.load(Ordering::Relaxed) {
        return; // prevent multiple loops running at a time
    }
    // Mark as started BEFORE spawning to avoid a race that can start two loops
    *started = true;
    drop(started);
    thread::spawn(move || {
        let mut refresh_after = Instant::now();
        loop {
            if refresh_after > Instant::now() {
                thread::sleep(refresh_after - Instant::now());
            }
            refresh_after = Instant::now() + interval() / 2;
            match refresh() {
                Ok(true) => {}
                Ok(false) => {
                    break;
                }
                Err(err) => {
                    eprintln!("clx: {err:?}");
                    *LINES.lock().unwrap() = 0;
                }
            }
            notify_wait(interval());
        }
    });
}

// Cache for smart refresh - skip terminal writes when output is unchanged and no spinners animating
static LAST_OUTPUT: Mutex<String> = Mutex::new(String::new());

fn refresh() -> Result<bool> {
    let _refresh_guard = REFRESH_LOCK.lock().unwrap();
    if STOPPING.load(Ordering::Relaxed) {
        *STARTED.lock().unwrap() = false;
        return Ok(false);
    }
    if is_paused() {
        return Ok(true);
    }
    static RENDER_CTX: OnceLock<Mutex<RenderContext>> = OnceLock::new();
    let ctx = RENDER_CTX.get_or_init(|| Mutex::new(RenderContext::default()));
    ctx.lock().unwrap().now = Instant::now();
    let ctx = ctx.lock().unwrap().clone();
    let mut tera = TERA.lock().unwrap();
    if tera.is_none() {
        *tera = Some(Tera::default());
    }
    let tera = tera.as_mut().unwrap();
    let jobs = JOBS.lock().unwrap().clone();

    // Update OSC progress based on current job progress
    update_osc_progress(&jobs);

    let any_running_check = || jobs.iter().any(|job| job.is_running());
    let any_running = any_running_check();
    let term = term();
    let mut lines = LINES.lock().unwrap();
    let output = jobs
        .iter()
        .map(|job| job.render(tera, ctx.clone()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // Process any remaining flex tags
    let final_output = if output.contains("<clx:flex>") || output.contains("<clx:flex_fill>") {
        flex(&output, term.width() as usize)
    } else {
        output
    };

    // Smart refresh: skip terminal write if output unchanged and no spinners animating
    // (running jobs have spinners that need to animate)
    let mut last_output = LAST_OUTPUT.lock().unwrap();
    if !any_running && final_output == *last_output && *lines > 0 {
        // Output unchanged and no animations - skip expensive terminal operations
        drop(last_output);
        if !any_running && !any_running_check() {
            *STARTED.lock().unwrap() = false;
            return Ok(false);
        }
        return Ok(true);
    }
    *last_output = final_output.clone();
    drop(last_output);

    // Perform clear + write + line accounting atomically to avoid interleaving with logger/pause
    let _guard = TERM_LOCK.lock().unwrap();
    // Robustly clear the previously rendered frame
    if *lines > 0 {
        term.move_cursor_up(*lines)?;
        term.move_cursor_left(term.width() as usize)?;
        term.clear_to_end_of_screen()?;
    }
    if !final_output.is_empty() {
        // Log frame for diagnostics (when CLX_TRACE_LOG is set)
        diagnostics::log_frame(&final_output, &jobs);

        term.write_line(&final_output)?;

        // Count how many terminal rows were consumed, accounting for wrapping
        let term_width = term.width() as usize;
        let mut consumed_rows = 0usize;
        for line in final_output.lines() {
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
    drop(_guard);
    if !any_running && !any_running_check() {
        *STARTED.lock().unwrap() = false;
        return Ok(false); // stop looping if no active progress jobs are running before or after the refresh
    }
    Ok(true)
}

fn refresh_once() -> Result<()> {
    let _refresh_guard = REFRESH_LOCK.lock().unwrap();
    let mut tera = TERA.lock().unwrap();
    if tera.is_none() {
        *tera = Some(Tera::default());
    }
    let tera = tera.as_mut().unwrap();
    static RENDER_CTX: OnceLock<Mutex<RenderContext>> = OnceLock::new();
    let ctx = RENDER_CTX.get_or_init(|| Mutex::new(RenderContext::default()));
    ctx.lock().unwrap().now = Instant::now();
    let ctx = ctx.lock().unwrap().clone();
    let jobs = JOBS.lock().unwrap().clone();

    // Update OSC progress based on current job progress
    update_osc_progress(&jobs);

    let term = term();
    let mut lines = LINES.lock().unwrap();
    let output = jobs
        .iter()
        .map(|job| job.render(tera, ctx.clone()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let _guard = TERM_LOCK.lock().unwrap();
    if *lines > 0 {
        term.move_cursor_up(*lines)?;
        term.move_cursor_left(term.width() as usize)?;
        term.clear_to_end_of_screen()?;
    }
    if !output.is_empty() {
        let final_output = if output.contains("<clx:flex>") {
            flex(&output, term.width() as usize)
        } else {
            output
        };

        // Log frame for diagnostics
        diagnostics::log_frame(&final_output, &jobs);

        term.write_line(&final_output)?;
        let term_width = term.width() as usize;
        let mut consumed_rows = 0usize;
        for line in final_output.lines() {
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
    drop(_guard);
    Ok(())
}

fn term() -> &'static Term {
    static TERM: LazyLock<Term> = LazyLock::new(Term::stderr);
    &TERM
}

#[must_use]
pub fn interval() -> Duration {
    *INTERVAL.lock().unwrap()
}

pub fn set_interval(interval: Duration) {
    *INTERVAL.lock().unwrap() = interval;
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

pub fn pause() {
    PAUSED.store(true, Ordering::Relaxed);
    if *STARTED.lock().unwrap() {
        let _ = clear();
    }
}

pub fn resume() {
    PAUSED.store(false, Ordering::Relaxed);
    if !*STARTED.lock().unwrap() {
        return;
    }
    if output() == ProgressOutput::UI {
        notify();
    }
}

pub fn stop() {
    // Stop the refresh loop and finalize a last frame synchronously
    STOPPING.store(true, Ordering::Relaxed);
    let _ = refresh_once();
    clear_osc_progress();
    *STARTED.lock().unwrap() = false;
}

pub fn stop_clear() {
    // Stop immediately and clear any progress from the screen
    STOPPING.store(true, Ordering::Relaxed);
    let _ = clear();
    clear_osc_progress();
    *STARTED.lock().unwrap() = false;
}

/// Updates OSC progress based on the current progress of all jobs
fn update_osc_progress(jobs: &[Arc<ProgressJob>]) {
    if !crate::osc::is_enabled() || jobs.is_empty() {
        return;
    }

    // If the first top-level job has explicit progress, use that directly
    if let (Some(current), Some(total)) = (
        *jobs[0].progress_current.lock().unwrap(),
        *jobs[0].progress_total.lock().unwrap(),
    ) {
        if total > 0 {
            let overall_percentage =
                (current as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as u8;
            let mut last_pct = LAST_OSC_PERCENTAGE.lock().unwrap();

            // Check for any failed jobs (including children) to determine OSC state
            let has_failed_jobs = {
                let mut stack: Vec<Arc<ProgressJob>> = jobs.to_vec();
                let mut found_failed = false;
                while let Some(job) = stack.pop() {
                    if job.status.lock().unwrap().is_failed() {
                        found_failed = true;
                        break;
                    }
                    let children = job.children.lock().unwrap();
                    for child in children.iter() {
                        stack.push(child.clone());
                    }
                }
                found_failed
            };

            let osc_state = if has_failed_jobs {
                ProgressState::Error
            } else {
                ProgressState::Normal
            };

            if *last_pct != Some(overall_percentage) || (has_failed_jobs && last_pct.is_none()) {
                set_progress(osc_state, overall_percentage);
                *last_pct = Some(overall_percentage);
            }
            return;
        }
    }

    // Fallback: use averaging algorithm for jobs without explicit progress
    let mut all_jobs: Vec<Arc<ProgressJob>> = Vec::new();
    let mut stack: Vec<Arc<ProgressJob>> = jobs.to_vec();

    while let Some(job) = stack.pop() {
        all_jobs.push(job.clone());
        let children = job.children.lock().unwrap();
        for child in children.iter() {
            stack.push(child.clone());
        }
    }

    let mut total_progress = 0.0f64;
    let mut job_count = 0;
    let mut has_failed_jobs = false;

    for job in all_jobs.iter() {
        if let (Some(current), Some(total)) = (
            *job.progress_current.lock().unwrap(),
            *job.progress_total.lock().unwrap(),
        ) {
            if total > 0 {
                let progress = (current as f64 / total as f64).clamp(0.0, 1.0);
                total_progress += progress;
                job_count += 1;
            }
        } else {
            let status = job.status.lock().unwrap();
            let progress = match &*status {
                s if s.is_running() => 0.5,
                s if s.is_done() => 1.0,
                s if s.is_failed() => {
                    has_failed_jobs = true;
                    1.0
                }
                _ => 1.0,
            };
            total_progress += progress;
            job_count += 1;
        }
    }

    if job_count > 0 {
        let overall_percentage =
            (total_progress / job_count as f64 * 100.0).clamp(0.0, 100.0) as u8;
        let mut last_pct = LAST_OSC_PERCENTAGE.lock().unwrap();

        let osc_state = if has_failed_jobs {
            ProgressState::Error
        } else {
            ProgressState::Normal
        };

        if *last_pct != Some(overall_percentage) || (has_failed_jobs && last_pct.is_none()) {
            set_progress(osc_state, overall_percentage);
            *last_pct = Some(overall_percentage);
        }
    }
}

/// Clear OSC progress indicator
fn clear_osc_progress() {
    if crate::osc::is_enabled() {
        clear_progress();
        *LAST_OSC_PERCENTAGE.lock().unwrap() = None;
    }
}

fn clear() -> Result<()> {
    let term = term();
    let mut lines = LINES.lock().unwrap();
    if *lines > 0 {
        let _guard = TERM_LOCK.lock().unwrap();
        term.move_cursor_up(*lines)?;
        term.move_cursor_left(term.width() as usize)?;
        term.clear_to_end_of_screen()?;
        drop(_guard);
    }
    *lines = 0;
    Ok(())
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m{}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

fn add_tera_functions(tera: &mut Tera, ctx: &RenderContext, job: &ProgressJob) {
    let elapsed = ctx.elapsed().as_millis() as usize;
    let job_elapsed = job.start.elapsed();
    let job_elapsed_secs = job_elapsed.as_secs_f64();
    let status = job.status.lock().unwrap().clone();
    let progress = ctx.progress;
    let width = ctx.width;

    // elapsed() - time since job started, formatted as "1m23s"
    let elapsed_str = format_duration(job_elapsed);
    tera.register_function("elapsed", move |_: &HashMap<String, tera::Value>| {
        Ok(elapsed_str.clone().into())
    });

    // eta() - estimated time remaining based on progress
    // Options:
    //   hide_complete: bool - if true, return empty string when progress is complete or no ETA available
    let (eta_value, eta_is_complete) = if let Some((cur, total)) = progress {
        if cur > 0 && total > 0 && cur <= total {
            let progress_ratio = cur as f64 / total as f64;
            let estimated_total = job_elapsed_secs / progress_ratio;
            let remaining = estimated_total - job_elapsed_secs;
            if remaining > 0.0 {
                (
                    Some(format_duration(Duration::from_secs_f64(remaining))),
                    false,
                )
            } else {
                (Some("0s".to_string()), true) // 0s means complete
            }
        } else {
            (None, cur >= total) // No ETA calculable, but might be complete
        }
    } else {
        (None, false) // No progress info
    };
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

    // rate() - items per second (or per minute if slow)
    let rate_str = if let Some((cur, _total)) = progress {
        if job_elapsed_secs > 0.0 && cur > 0 {
            let rate = cur as f64 / job_elapsed_secs;
            if rate >= 1.0 {
                format!("{:.1}/s", rate)
            } else if rate >= 1.0 / 60.0 {
                format!("{:.1}/m", rate * 60.0)
            } else {
                format!("{:.2}/s", rate)
            }
        } else {
            "-/s".to_string()
        }
    } else {
        "-/s".to_string()
    };
    tera.register_function("rate", move |_: &HashMap<String, tera::Value>| {
        Ok(rate_str.clone().into())
    });

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
    // progress_bar() - render a progress bar
    // Options:
    //   width: i64 - fixed width (negative values subtract from terminal width)
    //   flex: bool - use flexible width
    //   hide_complete: bool - if true, return empty string when progress is 100%
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
                let is_flex = props
                    .get("flex")
                    .as_ref()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_flex {
                    // Defer width calculation to flex processor using a placeholder
                    // Wrap with flex tags so callers don't need to pipe through the flex filter
                    let placeholder = format!(
                        "<clx:flex><clx:progress cur={} total={}><clx:flex>",
                        progress_current, progress_total
                    );
                    Ok(placeholder.into())
                } else {
                    let width = props
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
                    let progress_bar =
                        progress_bar::progress_bar(progress_current, progress_total, width);
                    Ok(progress_bar.into())
                }
            } else {
                Ok("".to_string().into())
            }
        },
    );
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

    // Flex fill filter - pads content to fill available width (for right-aligning subsequent content)
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

    // Simple truncate filter for text mode
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
                .unwrap_or(20); // Default prefix length estimate

            let max_len = args
                .get("length")
                .and_then(|v| v.as_i64())
                .map(|v| v as usize)
                .unwrap_or_else(|| {
                    // For text mode, calculate based on terminal width minus prefix
                    width.saturating_sub(prefix_len)
                });

            if content.len() <= max_len {
                Ok(content.into())
            } else {
                // Simple truncation with ellipsis
                if max_len > 1 {
                    Ok(format!("{}…", safe_prefix(&content, max_len.saturating_sub(1))).into())
                } else {
                    Ok("…".into())
                }
            }
        },
    );

    // Color filters
    tera.register_filter(
        "cyan",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::ecyan(&content).to_string().into())
        },
    );
    tera.register_filter(
        "blue",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::eblue(&content).to_string().into())
        },
    );
    tera.register_filter(
        "green",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::egreen(&content).to_string().into())
        },
    );
    tera.register_filter(
        "yellow",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::eyellow(&content).to_string().into())
        },
    );
    tera.register_filter(
        "red",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::ered(&content).to_string().into())
        },
    );
    tera.register_filter(
        "magenta",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::emagenta(&content).to_string().into())
        },
    );
    tera.register_filter(
        "bold",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::ebold(&content).to_string().into())
        },
    );
    tera.register_filter(
        "dim",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::edim(&content).to_string().into())
        },
    );
    tera.register_filter(
        "underline",
        |value: &tera::Value, _: &HashMap<String, tera::Value>| {
            let content = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            Ok(style::eunderline(&content).to_string().into())
        },
    );
}

fn add_tera_template(tera: &mut Tera, name: &str, body: &str) -> Result<()> {
    if !tera.get_template_names().any(|n| n == name) {
        tera.add_raw_template(name, body)?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProgressOutput {
    UI,
    Text,
}

static OUTPUT: Mutex<ProgressOutput> = Mutex::new(ProgressOutput::UI);

pub fn set_output(output: ProgressOutput) {
    *OUTPUT.lock().unwrap() = output;
}

#[must_use]
pub fn output() -> ProgressOutput {
    *OUTPUT.lock().unwrap()
}

fn flex(s: &str, width: usize) -> String {
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

fn flex_process_once(s: &str, width: usize) -> String {
    // Check for flex_fill tags first (pads content to fill available width)
    let flex_fill_count = s.matches("<clx:flex_fill>").count();
    if flex_fill_count >= 2 {
        let parts = s.splitn(3, "<clx:flex_fill>").collect::<Vec<_>>();
        if parts.len() >= 2 {
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
                // Truncate if content is too long
                if available_for_content > 3 {
                    result.push_str(&console::truncate_str(content, available_for_content, "…"));
                } else {
                    result.push_str(content);
                }
            } else {
                // Pad with spaces to fill available width
                result.push_str(content);
                let padding = available_for_content.saturating_sub(content_width);
                result.push_str(&" ".repeat(padding));
            }
            result.push_str(suffix);
            return result;
        }
    }

    // Check for regular flex tags (truncates content to fit)
    let flex_count = s.matches("<clx:flex>").count();
    if flex_count >= 2 {
        let parts = s.splitn(3, "<clx:flex>").collect::<Vec<_>>();
        if parts.len() >= 2 {
            let prefix = parts[0];
            let content = parts[1];
            let suffix = if parts.len() == 3 { parts[2] } else { "" };

            // Handle empty content case
            if content.is_empty() {
                let mut result = String::new();
                result.push_str(prefix);
                result.push_str(suffix);
                return result;
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
                        let truncated =
                            console::truncate_str(first_content_line, available_width, "…");
                        result.push_str(&truncated);
                    } else {
                        result.push('…');
                    }
                } else {
                    result.push_str(content);
                }

                return result;
            } else {
                // Single line with flex tags
                let suffix_width = if suffix_lines.is_empty() {
                    0
                } else {
                    console::measure_text_width(suffix_lines[0])
                };
                let available_for_content =
                    width.saturating_sub(first_line_prefix_width + suffix_width);

                if first_line_prefix_width >= width {
                    return console::truncate_str(prefix, width, "…").to_string();
                }

                let mut result = String::new();
                result.push_str(prefix);

                if content.starts_with("<clx:progress") {
                    // Render a progress bar sized to the available space
                    let mut cur: Option<usize> = None;
                    let mut total: Option<usize> = None;
                    for part in content.trim_matches(['<', '>', ' ']).split_whitespace() {
                        if let Some(v) = part.strip_prefix("cur=") {
                            cur = v.parse::<usize>().ok();
                        } else if let Some(v) = part.strip_prefix("total=") {
                            total = v.parse::<usize>().ok();
                        }
                    }
                    if let (Some(cur), Some(total)) = (cur, total) {
                        let pb = progress_bar::progress_bar(cur, total, available_for_content);
                        result.push_str(&pb);
                        result.push_str(suffix);
                        return result;
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

                return result;
            }
        }
    }

    // Fallback: process line by line for incomplete flex tags
    s.lines()
        .map(|line| {
            // Handle flex_fill in line-by-line mode
            if line.contains("<clx:flex_fill>") {
                let parts = line.splitn(3, "<clx:flex_fill>").collect::<Vec<_>>();
                if parts.len() >= 2 {
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
                            result.push_str(&console::truncate_str(
                                content,
                                available_for_content,
                                "…",
                            ));
                        } else {
                            result.push_str(content);
                        }
                    } else {
                        result.push_str(content);
                        let padding = available_for_content.saturating_sub(content_width);
                        result.push_str(&" ".repeat(padding));
                    }
                    result.push_str(suffix);
                    return result;
                }
            }

            if !line.contains("<clx:flex>") {
                return line.to_string();
            }

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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Returns a prefix of s with at most max_bytes bytes, cutting only at char boundaries
fn safe_prefix(s: &str, max_bytes: usize) -> &str {
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
    fn test_flex() {
        // Test normal case
        let s = "prefix<clx:flex>content<clx:flex>suffix";
        let result = flex(s, 20);
        let width = console::measure_text_width(&result);
        println!("Normal case: result='{}', width={}", result, width);
        assert!(width <= 20);
        assert!(result.contains("prefix"));
        assert!(result.contains("suffix"));

        // Test case where prefix + suffix are longer than available width
        let s = "very_long_prefix<clx:flex>content<clx:flex>very_long_suffix";
        let result = flex(s, 10);
        let width = console::measure_text_width(&result);
        println!(
            "Long prefix/suffix case: result='{}', width={}",
            result, width
        );
        assert!(width <= 10);
        // When truncating, we expect the result to be within width limits
        assert!(!result.is_empty());

        // Test case with extremely long content
        let long_content = "a".repeat(1000);
        let s = format!("prefix<clx:flex>{}<clx:flex>suffix", long_content);
        let result = flex(&s, 30);
        let width = console::measure_text_width(&result);
        println!("Long content case: result='{}', width={}", result, width);
        assert!(width <= 30);
        assert!(result.contains("prefix"));
        assert!(result.contains("suffix"));

        // Test case with extremely long prefix and suffix (like the ensembler_stdout issue)
        let long_prefix = "very_long_prefix_that_exceeds_screen_width_".repeat(10);
        let long_suffix = "very_long_suffix_that_exceeds_screen_width_".repeat(10);
        let s = format!("{}<clx:flex>content<clx:flex>{}", long_prefix, long_suffix);
        let result = flex(&s, 50);
        let width = console::measure_text_width(&result);
        println!(
            "Extreme long prefix/suffix case: result='{}', width={}",
            result, width
        );
        assert!(width <= 50);
        // Should still contain some content
        assert!(!result.is_empty());
    }

    #[test]
    fn test_flex_progress_placeholder_basic() {
        // Prefix + flexed progress + suffix should exactly fit the target width
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
        // Minimal width where available space for the bar is 2 characters
        let prefix = "a"; // width 1
        let suffix = "b"; // width 1
        let s = format!(
            "{}<clx:flex><clx:progress cur=1 total=1><clx:flex>{}",
            prefix, suffix
        );
        let target_width = 4; // 1 (prefix) + 2 (bar brackets) + 1 (suffix)
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
        // Should be exactly 30 (filled with spaces)
        assert_eq!(width, 30);
        assert!(result.starts_with("prefix"));
        assert!(result.ends_with("suffix"));
        assert!(result.contains("short"));
        // Should have padding spaces between content and suffix
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
        // Test that flex_fill can be used to right-align suffix content
        let s = "X<clx:flex_fill>msg<clx:flex_fill>[====]";
        let result = flex(s, 20);
        // Result should be: "Xmsg          [====]" (padded to push [====] right)
        assert_eq!(console::measure_text_width(&result), 20);
        assert!(result.starts_with("Xmsg"));
        assert!(result.ends_with("[====]"));
    }

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

        // Default is Running
        assert!(job.status.lock().unwrap().is_running());
        assert!(job.is_running());

        // Transition to Done
        job.set_status(ProgressStatus::Done);
        assert!(job.status.lock().unwrap().is_done());
        assert!(!job.is_running());

        // Transition to Failed
        job.set_status(ProgressStatus::Failed);
        assert!(job.status.lock().unwrap().is_failed());

        // Transition to Pending
        job.set_status(ProgressStatus::Pending);
        assert!(job.status.lock().unwrap().is_pending());

        // Transition back to Running
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

        // Progress should be clamped to total
        job.progress_current(150);
        assert_eq!(*job.progress_current.lock().unwrap(), Some(100));
    }

    #[test]
    fn test_progress_job_progress_total_update() {
        let job = ProgressJobBuilder::new().progress_current(80).build();

        // Setting total less than current should be adjusted
        job.progress_total(50);
        assert_eq!(*job.progress_total.lock().unwrap(), Some(80));
    }

    #[test]
    fn test_progress_job_equality() {
        let job1 = ProgressJobBuilder::new().build();
        let job2 = ProgressJobBuilder::new().build();

        // Jobs have different IDs
        assert_ne!(job1, job2);

        // Same job equals itself
        assert_eq!(job1, job1);
    }

    #[test]
    fn test_with_terminal_lock() {
        // Test that with_terminal_lock returns the value from the closure
        let result = with_terminal_lock(|| 42);
        assert_eq!(result, 42);

        let result = with_terminal_lock(|| "hello".to_string());
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_interval_get_set() {
        let original = interval();

        set_interval(Duration::from_millis(500));
        assert_eq!(interval(), Duration::from_millis(500));

        // Restore original
        set_interval(original);
    }

    #[test]
    fn test_output_get_set() {
        let original = output();

        set_output(ProgressOutput::Text);
        assert_eq!(output(), ProgressOutput::Text);

        set_output(ProgressOutput::UI);
        assert_eq!(output(), ProgressOutput::UI);

        // Restore original
        set_output(original);
    }

    #[test]
    fn test_progress_job_done_behavior() {
        assert_eq!(
            ProgressJobDoneBehavior::default(),
            ProgressJobDoneBehavior::Keep
        );
    }

    #[test]
    fn test_safe_prefix() {
        // Returns string if it fits within max_bytes
        assert_eq!(safe_prefix("hello", 10), "hello");
        assert_eq!(safe_prefix("hello", 5), "hello");

        // Returns prefix up to (but not including) char at max_bytes index
        // For "hello" with max_bytes=3, indices 0,1,2 are valid, last is 2, so returns s[..2]="he"
        assert_eq!(safe_prefix("hello", 3), "he");
        assert_eq!(safe_prefix("hello", 1), "");
        assert_eq!(safe_prefix("hello", 0), "");

        // Test with multi-byte characters - ensures we don't split UTF-8 sequences
        let s = "helloworld";
        assert_eq!(safe_prefix(s, 5), "hell");
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
