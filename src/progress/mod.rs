//! Hierarchical progress indicators with spinners and template rendering.
//!
//! This module provides a flexible progress display system for CLI applications.
//! Progress jobs can be nested hierarchically, support animated spinners, and use
//! Tera templates for customizable rendering.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use clx::progress::{ProgressJobBuilder, ProgressStatus};
//!
//! // Create and start a progress job
//! let job = ProgressJobBuilder::new()
//!     .prop("message", "Processing...")
//!     .start();
//!
//! // Update the message
//! job.prop("message", "Almost done...");
//!
//! // Mark as complete
//! job.set_status(ProgressStatus::Done);
//! ```
//!
//! # Hierarchical Progress
//!
//! Jobs can have child jobs for nested progress display:
//!
//! ```rust,no_run
//! use clx::progress::{ProgressJobBuilder, ProgressStatus};
//!
//! let parent = ProgressJobBuilder::new()
//!     .prop("message", "Building project")
//!     .start();
//!
//! let child = parent.add(
//!     ProgressJobBuilder::new()
//!         .prop("message", "Compiling src/main.rs")
//!         .build()
//! );
//!
//! child.set_status(ProgressStatus::Done);
//! parent.set_status(ProgressStatus::Done);
//! ```
//!
//! # Custom Templates
//!
//! Progress jobs use [Tera](https://tera.netlify.app/) templates for rendering:
//!
//! ```rust,no_run
//! use clx::progress::ProgressJobBuilder;
//!
//! let job = ProgressJobBuilder::new()
//!     .body("{{ spinner() }} [{{ cur }}/{{ total }}] {{ message | cyan }}")
//!     .prop("message", "Building")
//!     .prop("cur", &0)
//!     .prop("total", &10)
//!     .start();
//! ```
//!
//! ## Available Template Functions
//!
//! - `spinner(name='...')` - Animated spinner (default: `mini_dot`)
//! - `progress_bar(flex=true)` - Progress bar that fills available width
//! - `progress_bar(width=N)` - Fixed-width progress bar
//! - `elapsed()` - Time since job started (e.g., "1m23s")
//! - `eta()` - Estimated time remaining
//! - `rate()` - Throughput rate (e.g., "42.5/s")
//! - `bytes()` - Progress as human-readable bytes (e.g., "5.2 MB / 10.4 MB")
//!
//! ## Available Template Filters
//!
//! - `{{ text | flex }}` - Truncates to fit available width
//! - `{{ text | flex_fill }}` - Pads to fill available width
//! - Color: `cyan`, `blue`, `green`, `yellow`, `red`, `magenta`
//! - Style: `bold`, `dim`, `underline`
//!
//! # Output Modes
//!
//! The progress system supports two output modes:
//!
//! - [`ProgressOutput::UI`] - Rich terminal UI with animations (default)
//! - [`ProgressOutput::Text`] - Simple text output for non-interactive environments
//!
//! ```rust,no_run
//! use clx::progress::{set_output, ProgressOutput};
//!
//! // Use text mode for CI environments
//! set_output(ProgressOutput::Text);
//! ```
//!
//! # Environment Variables
//!
//! The progress system can be controlled via environment variables:
//!
//! - `CLX_NO_PROGRESS=1` - Disable progress display entirely. Jobs can still be
//!   created and used, but nothing will be rendered. Useful for scripts that
//!   parse output or environments where progress causes issues.
//!
//! - `CLX_TEXT_MODE=1` - Force text mode regardless of [`set_output`] calls.
//!   Each update prints a new line instead of updating in place. Useful for CI
//!   systems and log files.
//!
//! ```bash
//! # Disable all progress display
//! CLX_NO_PROGRESS=1 ./my-program
//!
//! # Force text mode for logging
//! CLX_TEXT_MODE=1 ./my-program
//! ```
//!
//! Use [`is_disabled`] to check if progress is disabled at runtime.
//!
//! # Threading Model
//!
//! The progress system is designed for safe concurrent access from multiple threads.
//! Understanding its threading model helps when integrating with multi-threaded
//! applications or debugging synchronization issues.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Main Thread(s)                              │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
//! │  │  Worker 1   │  │  Worker 2   │  │  Worker N   │                 │
//! │  │ job.prop()  │  │ job.prop()  │  │ job.prop()  │                 │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘                 │
//! │         │                │                │                         │
//! │         ▼                ▼                ▼                         │
//! │  ┌────────────────────────────────────────────────────────────────┐│
//! │  │              JOBS (Mutex<Vec<Arc<ProgressJob>>>)               ││
//! │  │  • Stores all top-level jobs                                   ││
//! │  │  • Each job has interior mutability via Mutex                  ││
//! │  └────────────────────────────────────────────────────────────────┘│
//! │                          │                                          │
//! │                          │ notify()                                 │
//! │                          ▼                                          │
//! │  ┌────────────────────────────────────────────────────────────────┐│
//! │  │              NOTIFY (mpsc::Sender)                             ││
//! │  │  • Wakes background thread for immediate refresh               ││
//! │  └────────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Background Thread                              │
//! │  ┌────────────────────────────────────────────────────────────────┐│
//! │  │                   refresh()                                    ││
//! │  │  1. Acquire REFRESH_LOCK                                       ││
//! │  │  2. Clone JOBS snapshot                                        ││
//! │  │  3. Render all jobs via Tera                                   ││
//! │  │  4. Acquire TERM_LOCK                                          ││
//! │  │  5. Clear previous output + write new                          ││
//! │  │  6. Release TERM_LOCK                                          ││
//! │  │  7. Wait on NOTIFY or timeout (INTERVAL)                       ││
//! │  └────────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Global State
//!
//! | Static | Type | Purpose |
//! |--------|------|---------|
//! | `JOBS` | `Mutex<Vec<Arc<ProgressJob>>>` | All top-level progress jobs |
//! | `TERM_LOCK` | `Mutex<()>` | Serializes terminal write operations |
//! | `REFRESH_LOCK` | `Mutex<()>` | Prevents concurrent refresh cycles |
//! | `STARTED` | `Mutex<bool>` | Whether background thread is running |
//! | `PAUSED` | `AtomicBool` | Whether refresh is temporarily paused |
//! | `STOPPING` | `AtomicBool` | Signal to stop the background thread |
//! | `INTERVAL` | `Mutex<Duration>` | Refresh interval (default 200ms) |
//! | `NOTIFY` | `Mutex<Option<mpsc::Sender>>` | Channel to wake background thread |
//!
//! ## Background Thread Lifecycle
//!
//! 1. **Start**: First call to `notify()` spawns the background thread via `start()`
//! 2. **Loop**: Thread alternates between rendering and waiting for notifications
//! 3. **Smart Refresh**: Skips terminal writes if output unchanged and no spinners animating
//! 4. **Stop**: When no active jobs remain, thread exits automatically
//!
//! The background thread is lazy - it only starts when the first job update occurs,
//! and stops automatically when all jobs complete.
//!
//! ## Notification System
//!
//! Job updates call `notify()` which:
//! 1. Ensures the background thread is started
//! 2. Sends a message on the `NOTIFY` channel
//! 3. This wakes the background thread for immediate refresh
//!
//! Without notifications, the thread waits for `INTERVAL` between refreshes.
//!
//! ## Terminal Lock Usage
//!
//! The `TERM_LOCK` serializes all terminal output to prevent interleaved writes:
//!
//! - The background thread holds it during clear/write operations
//! - `with_terminal_lock()` lets external code acquire it for safe printing
//! - `pause()`/`resume()` clear and restore display while allowing external writes
//!
//! ## Thread Safety Guarantees
//!
//! - **Job updates are atomic**: Each field update acquires its own mutex
//! - **Display is consistent**: `REFRESH_LOCK` ensures complete render cycles
//! - **No interleaved output**: `TERM_LOCK` serializes all terminal writes
//! - **Safe concurrent access**: `Arc<ProgressJob>` can be shared across threads
//!
//! ## Text Mode
//!
//! When `ProgressOutput::Text` is active:
//! - No background thread is started
//! - Each `update()` call writes directly to stderr
//! - Useful for CI/CD, piped output, or non-terminal environments
//!
//! ## Terminal Resize Handling
//!
//! On Unix systems, the progress display automatically adapts to terminal resizes:
//! - A SIGWINCH signal handler is registered when the background thread starts
//! - When the terminal is resized, the display is immediately re-rendered
//! - The `flex` and `flex_fill` filters adapt content to the new width
//! - Progress bars with `flex=true` automatically resize
//!
//! This is handled transparently - no user code is required to enable resize support.
//!
//! ## Example: Multi-threaded Usage
//!
//! ```rust,no_run
//! use clx::progress::{ProgressJobBuilder, ProgressStatus, with_terminal_lock};
//! use std::sync::Arc;
//! use std::thread;
//!
//! // Create a job that will be shared across threads
//! let job = ProgressJobBuilder::new()
//!     .prop("message", "Processing")
//!     .progress_total(100)
//!     .start();
//!
//! // Clone Arc for each worker thread
//! let handles: Vec<_> = (0..4).map(|i| {
//!     let job = Arc::clone(&job);
//!     thread::spawn(move || {
//!         for j in 0..25 {
//!             // Safe concurrent progress updates
//!             job.progress_current(i * 25 + j);
//!
//!             // Use terminal lock for custom output
//!             with_terminal_lock(|| {
//!                 eprintln!("Worker {} completed item {}", i, j);
//!             });
//!         }
//!     })
//! }).collect();
//!
//! for h in handles {
//!     h.join().unwrap();
//! }
//!
//! job.set_status(ProgressStatus::Done);
//! ```
//!
//! ## Log Integration
//!
//! When the `log` feature is enabled, you can use the progress-aware logger
//! to seamlessly interleave log messages with progress display:
//!
//! ```rust,ignore
//! use clx::progress::{ProgressJobBuilder, ProgressStatus, init_log_integration};
//! use log::info;
//!
//! // Initialize once at startup
//! init_log_integration();
//!
//! let job = ProgressJobBuilder::new()
//!     .prop("message", "Working...")
//!     .start();
//!
//! // Log messages automatically pause/resume progress
//! info!("Starting work");
//! // ... do work ...
//! info!("Work complete");
//!
//! job.set_status(ProgressStatus::Done);
//! ```
//!
//! The logger automatically pauses progress before writing and resumes afterward,
//! preventing log output from being overwritten by progress updates.

mod diagnostics;
mod flex;
mod format;
mod job;
mod output;
mod render;
mod spinners;
mod state;
mod tera_setup;

#[cfg(feature = "log")]
mod log;

// Re-export public API
pub use job::{ProgressJob, ProgressJobBuilder, ProgressJobDoneBehavior, ProgressStatus};
pub use output::{ProgressOutput, output, set_output};
pub use state::{
    active_jobs, flush, interval, is_disabled, is_paused, job_count, pause, resume, set_interval,
    stop, stop_clear, with_terminal_lock,
};

#[cfg(feature = "log")]
pub use log::{
    ProgressLogger, init_log_integration, init_log_integration_with_level, try_init_log_integration,
    try_init_log_integration_with_level,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tera::Context;

    // ==================== Template Helper Function Tests ====================

    /// Helper to create a RenderContext for testing template functions
    fn test_render_context(progress: Option<(usize, usize)>) -> render::RenderContext {
        use std::time::Instant;
        let now = Instant::now();
        render::RenderContext {
            start: now,
            now,
            width: 80,
            tera_ctx: Context::new(),
            indent: 0,
            include_children: false,
            progress,
        }
    }

    /// Helper to render a template with a job
    fn render_template(job: &ProgressJob, ctx: &render::RenderContext) -> String {
        let mut tera = tera::Tera::default();
        tera_setup::add_tera_functions(&mut tera, ctx, job);
        tera.add_raw_template("body", &job.body.lock().unwrap())
            .unwrap();
        tera.render("body", &ctx.tera_ctx).unwrap()
    }

    #[test]
    fn test_template_elapsed_renders() {
        let job = ProgressJobBuilder::new().body("{{ elapsed() }}").build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "0s");
    }

    #[test]
    fn test_template_eta_no_progress() {
        let job = ProgressJobBuilder::new().body("{{ eta() }}").build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "-");
    }

    #[test]
    fn test_template_eta_with_progress() {
        let job = ProgressJobBuilder::new()
            .body("{{ eta() }}")
            .progress_current(50)
            .progress_total(100)
            .build();
        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);
        assert!(
            result.ends_with('s') || result.ends_with('m') || result == "-" || result == "0s",
            "Expected duration format, got: {}",
            result
        );
    }

    #[test]
    fn test_template_eta_hide_complete() {
        let job = ProgressJobBuilder::new()
            .body("{{ eta(hide_complete=true) }}")
            .progress_current(100)
            .progress_total(100)
            .build();
        let ctx = test_render_context(Some((100, 100)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn test_template_rate_no_progress() {
        let job = ProgressJobBuilder::new().body("{{ rate() }}").build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "-/s");
    }

    #[test]
    fn test_template_rate_with_smoothed_rate() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(100)
            .progress_total(200)
            .build();
        *job.smoothed_rate.lock().unwrap() = Some(10.0);
        let ctx = test_render_context(Some((100, 200)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "10.0/s");
    }

    #[test]
    fn test_template_rate_slow() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(1)
            .progress_total(100)
            .build();
        *job.smoothed_rate.lock().unwrap() = Some(0.5);
        let ctx = test_render_context(Some((1, 100)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "30.0/m");
    }

    #[test]
    fn test_template_rate_very_slow() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(1)
            .progress_total(100)
            .build();
        *job.smoothed_rate.lock().unwrap() = Some(0.01);
        let ctx = test_render_context(Some((1, 100)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "0.01/s");
    }

    #[test]
    fn test_template_bytes_no_progress() {
        let job = ProgressJobBuilder::new().body("{{ bytes() }}").build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn test_template_bytes_with_progress() {
        let job = ProgressJobBuilder::new()
            .body("{{ bytes() }}")
            .progress_current(1024 * 512)
            .progress_total(1024 * 1024)
            .build();
        let ctx = test_render_context(Some((1024 * 512, 1024 * 1024)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "512.0 KB / 1.0 MB");
    }

    #[test]
    fn test_template_bytes_hide_complete() {
        let job = ProgressJobBuilder::new()
            .body("{{ bytes(hide_complete=true) }}")
            .progress_current(1024)
            .progress_total(1024)
            .build();
        let ctx = test_render_context(Some((1024, 1024)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn test_template_spinner_running() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::Running)
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert!(
            result.contains("\x1b[") || !result.is_empty(),
            "Expected spinner output, got: {:?}",
            result
        );
    }

    #[test]
    fn test_template_spinner_done() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::Done)
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert!(
            result.contains('✔'),
            "Expected checkmark, got: {:?}",
            result
        );
    }

    #[test]
    fn test_template_spinner_failed() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::Failed)
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert!(result.contains('✗'), "Expected X mark, got: {:?}", result);
    }

    #[test]
    fn test_template_spinner_pending() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::Pending)
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert!(
            result.contains('⏸'),
            "Expected pause symbol, got: {:?}",
            result
        );
    }

    #[test]
    fn test_template_spinner_warn() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::Warn)
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert!(
            result.contains('⚠'),
            "Expected warning symbol, got: {:?}",
            result
        );
    }

    #[test]
    fn test_template_spinner_custom() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::RunningCustom("🔥".to_string()))
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "🔥");
    }

    #[test]
    fn test_template_spinner_done_custom() {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }}")
            .status(ProgressStatus::DoneCustom("🎉".to_string()))
            .build();
        let ctx = test_render_context(None);
        let result = render_template(&job, &ctx);
        assert_eq!(result, "🎉");
    }

    #[test]
    fn test_template_progress_bar_basic() {
        let job = ProgressJobBuilder::new()
            .body("{{ progress_bar(width=20) }}")
            .progress_current(5)
            .progress_total(10)
            .build();
        let ctx = test_render_context(Some((5, 10)));
        let result = render_template(&job, &ctx);
        assert!(result.contains('[') && result.contains(']'));
        assert!(result.contains('━') || result.contains('=') || result.contains('#'));
    }

    #[test]
    fn test_template_progress_bar_hide_complete() {
        let job = ProgressJobBuilder::new()
            .body("{{ progress_bar(width=20, hide_complete=true) }}")
            .progress_current(10)
            .progress_total(10)
            .build();
        let ctx = test_render_context(Some((10, 10)));
        let result = render_template(&job, &ctx);
        assert_eq!(result, "");
    }

    // ==================== ETA/Rate Smoothing Tests ====================

    #[test]
    fn test_smoothed_rate_initial_value() {
        let job = ProgressJobBuilder::new().progress_total(100).build();
        assert!(job.smoothed_rate.lock().unwrap().is_none());

        job.progress_current(10);
        std::thread::sleep(Duration::from_millis(10));
        job.progress_current(20);

        let rate = job.smoothed_rate.lock().unwrap();
        assert!(rate.is_some(), "Expected smoothed rate after second update");
        let rate_value = rate.unwrap();
        assert!(
            rate_value > 0.0,
            "Expected positive rate, got {}",
            rate_value
        );
    }

    #[test]
    fn test_smoothed_rate_exponential_moving_average() {
        let job = ProgressJobBuilder::new().progress_total(1000).build();

        job.progress_current(0);
        std::thread::sleep(Duration::from_millis(10));

        job.progress_current(100);
        std::thread::sleep(Duration::from_millis(10));

        let rate1 = job.smoothed_rate.lock().unwrap().unwrap();

        job.progress_current(200);
        std::thread::sleep(Duration::from_millis(10));

        let rate2 = job.smoothed_rate.lock().unwrap().unwrap();

        job.progress_current(300);

        let rate3 = job.smoothed_rate.lock().unwrap().unwrap();

        assert!(rate1 > 0.0);
        assert!(rate2 > 0.0);
        assert!(rate3 > 0.0);
    }

    #[test]
    fn test_smoothed_rate_no_update_on_backwards_progress() {
        let job = ProgressJobBuilder::new().progress_total(100).build();

        job.progress_current(0);
        std::thread::sleep(Duration::from_millis(10));

        job.progress_current(50);
        let rate_after_forward = *job.smoothed_rate.lock().unwrap();

        assert!(
            rate_after_forward.is_some(),
            "Expected rate to be set after forward progress"
        );

        std::thread::sleep(Duration::from_millis(10));

        job.progress_current(30);
        let rate_after_attempt = *job.smoothed_rate.lock().unwrap();

        assert_eq!(rate_after_forward, rate_after_attempt);
    }

    #[test]
    fn test_smoothed_rate_no_update_on_tiny_elapsed_time() {
        let job = ProgressJobBuilder::new().progress_total(100).build();

        job.progress_current(0);
        job.progress_current(10);
        job.progress_current(20);

        let _rate = job.smoothed_rate.lock().unwrap();
    }

    #[test]
    fn test_increment_updates_smoothed_rate() {
        let job = ProgressJobBuilder::new().progress_total(100).build();

        assert!(job.smoothed_rate.lock().unwrap().is_none());

        job.increment(10);
        std::thread::sleep(Duration::from_millis(10));

        job.increment(10);

        let rate = job.smoothed_rate.lock().unwrap();
        assert!(rate.is_some(), "Expected smoothed rate after increments");
    }

    #[test]
    fn test_smoothed_rate_affects_eta_calculation() {
        let job = ProgressJobBuilder::new()
            .body("{{ eta() }}")
            .progress_current(50)
            .progress_total(100)
            .build();

        *job.smoothed_rate.lock().unwrap() = Some(10.0);

        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);

        assert_eq!(result, "5s");
    }

    #[test]
    fn test_smoothed_rate_affects_rate_display() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(50)
            .progress_total(100)
            .build();

        *job.smoothed_rate.lock().unwrap() = Some(42.5);

        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);

        assert_eq!(result, "42.5/s");
    }

    #[test]
    fn test_eta_fallback_to_linear_extrapolation() {
        let job = ProgressJobBuilder::new()
            .body("{{ eta() }}")
            .progress_current(50)
            .progress_total(100)
            .build();

        assert!(job.smoothed_rate.lock().unwrap().is_none());

        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);

        assert!(
            result.ends_with('s') || result.ends_with('m') || result == "-" || result == "0s",
            "Expected valid ETA format, got: {}",
            result
        );
    }

    #[test]
    fn test_rate_fallback_to_average() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(100)
            .progress_total(200)
            .build();

        assert!(job.smoothed_rate.lock().unwrap().is_none());

        let ctx = test_render_context(Some((100, 200)));
        let result = render_template(&job, &ctx);

        assert!(
            result.contains("/s") || result.contains("/m"),
            "Expected rate format, got: {}",
            result
        );
    }

    #[test]
    fn test_eta_with_zero_smoothed_rate() {
        let job = ProgressJobBuilder::new()
            .body("{{ eta() }}")
            .progress_current(50)
            .progress_total(100)
            .build();

        *job.smoothed_rate.lock().unwrap() = Some(0.0);

        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);

        assert!(
            result.ends_with('s') || result.ends_with('m') || result == "-" || result == "0s",
            "Expected valid ETA format, got: {}",
            result
        );
    }

    #[test]
    fn test_rate_with_zero_smoothed_rate() {
        let job = ProgressJobBuilder::new()
            .body("{{ rate() }}")
            .progress_current(50)
            .progress_total(100)
            .build();

        *job.smoothed_rate.lock().unwrap() = Some(0.0);

        let ctx = test_render_context(Some((50, 100)));
        let result = render_template(&job, &ctx);

        assert_eq!(result, "-/s");
    }

    #[test]
    fn test_job_count_and_active_jobs() {
        let initial_count = job_count();
        let initial_active = active_jobs();

        let job = ProgressJobBuilder::new().prop("message", "test").start();

        assert_eq!(job_count(), initial_count + 1);
        assert_eq!(active_jobs(), initial_active + 1);

        job.set_status(ProgressStatus::Done);
        assert_eq!(job_count(), initial_count + 1);
        assert_eq!(active_jobs(), initial_active);

        job.remove();
        assert_eq!(job_count(), initial_count);
    }
}
