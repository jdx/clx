//! Global state and thread management for progress display.
//!
//! This module contains all the global statics that coordinate the progress
//! display system, including job storage, terminal locking, and the background
//! refresh thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use console::Term;

use super::job::ProgressJob;
use super::output::{ProgressOutput, output};
use super::render::{refresh, refresh_once};

// =============================================================================
// Environment Variable Controls
// =============================================================================

static ENV_NO_PROGRESS: OnceLock<bool> = OnceLock::new();
static ENV_TEXT_MODE: OnceLock<bool> = OnceLock::new();

/// Checks if an environment variable is set to a truthy value ("1" or "true").
fn check_env_bool(var_name: &str) -> bool {
    std::env::var(var_name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Returns true if progress display is disabled via `CLX_NO_PROGRESS=1` environment variable.
fn env_no_progress() -> bool {
    *ENV_NO_PROGRESS.get_or_init(|| check_env_bool("CLX_NO_PROGRESS"))
}

/// Returns true if text mode is forced via `CLX_TEXT_MODE=1` environment variable.
pub(crate) fn env_text_mode() -> bool {
    *ENV_TEXT_MODE.get_or_init(|| check_env_bool("CLX_TEXT_MODE"))
}

/// Returns whether progress display is currently disabled.
///
/// Progress is disabled when the `CLX_NO_PROGRESS` environment variable is set to `1` or `true`.
#[must_use]
pub fn is_disabled() -> bool {
    env_no_progress()
}

// =============================================================================
// Global Statics
// =============================================================================

/// Number of terminal lines currently occupied by progress output.
pub(crate) static LINES: Mutex<usize> = Mutex::new(0);

/// Global terminal lock for synchronizing output operations.
pub static TERM_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Lock to ensure only one refresh cycle runs at a time.
pub(crate) static REFRESH_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Signal to stop the background refresh thread.
pub(crate) static STOPPING: AtomicBool = AtomicBool::new(false);

/// Channel to notify the background thread of updates.
static NOTIFY: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

/// Whether the background refresh thread is currently running.
pub static STARTED: Mutex<bool> = Mutex::new(false);

/// Whether progress rendering is temporarily paused.
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Collection of all top-level progress jobs.
pub(crate) static JOBS: Mutex<Vec<Arc<ProgressJob>>> = Mutex::new(vec![]);

/// Shared Tera template engine instance.
pub(crate) static TERA: Mutex<Option<tera::Tera>> = Mutex::new(None);

/// Refresh interval for the progress display.
static INTERVAL: Mutex<Duration> = Mutex::new(Duration::from_millis(200));

/// OSC progress tracking state.
pub(crate) static LAST_OSC_PERCENTAGE: Mutex<Option<u8>> = Mutex::new(None);

/// Cache for smart refresh optimization.
pub(crate) static LAST_OUTPUT: Mutex<String> = Mutex::new(String::new());

/// Shared render context for refresh cycles.
pub(crate) static RENDER_CTX: OnceLock<Mutex<super::render::RenderContext>> = OnceLock::new();

// =============================================================================
// Terminal Resize Handling (Unix)
// =============================================================================

/// Flag indicating that a terminal resize (SIGWINCH) was received.
#[cfg(unix)]
static RESIZE_SIGNALED: AtomicBool = AtomicBool::new(false);

/// Signal handler for SIGWINCH (terminal resize).
#[cfg(unix)]
extern "C" fn handle_sigwinch(_: nix::libc::c_int) {
    RESIZE_SIGNALED.store(true, Ordering::Relaxed);
}

/// Registers the SIGWINCH signal handler for terminal resize detection.
#[cfg(unix)]
fn register_resize_handler() {
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
    let handler = SigHandler::Handler(handle_sigwinch);
    let action = SigAction::new(handler, SaFlags::SA_RESTART, SigSet::empty());
    unsafe {
        let _ = sigaction(Signal::SIGWINCH, &action);
    }
}

/// Checks and clears the resize signal flag.
#[cfg(unix)]
pub(crate) fn check_resize_signaled() -> bool {
    RESIZE_SIGNALED.swap(false, Ordering::Relaxed)
}

/// Stub for non-Unix platforms where SIGWINCH doesn't exist.
#[cfg(not(unix))]
pub(crate) fn check_resize_signaled() -> bool {
    false
}

// =============================================================================
// Terminal Access
// =============================================================================

/// Returns a reference to the shared terminal instance.
pub(crate) fn term() -> &'static Term {
    static TERM: LazyLock<Term> = LazyLock::new(Term::stderr);
    &TERM
}

// =============================================================================
// Terminal Lock
// =============================================================================

/// Executes a function while holding the global terminal lock.
///
/// Use this to synchronize your own stderr/stdout writes with the progress display
/// to prevent interleaved or corrupted output.
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

// =============================================================================
// Interval Configuration
// =============================================================================

/// Returns the current refresh interval.
#[must_use]
pub fn interval() -> Duration {
    *INTERVAL.lock().unwrap()
}

/// Sets the refresh interval for the progress display.
pub fn set_interval(interval: Duration) {
    *INTERVAL.lock().unwrap() = interval;
}

// =============================================================================
// Pause/Resume
// =============================================================================

/// Returns `true` if progress rendering is currently paused.
pub fn is_paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

/// Pauses progress rendering and clears the display.
pub fn pause() {
    PAUSED.store(true, Ordering::Relaxed);
    if *STARTED.lock().unwrap() {
        let _ = clear();
    }
}

/// Resumes progress rendering after a pause.
pub fn resume() {
    PAUSED.store(false, Ordering::Relaxed);
    if !*STARTED.lock().unwrap() {
        return;
    }
    if output() == ProgressOutput::UI {
        notify();
    }
}

// =============================================================================
// Thread Control
// =============================================================================

/// Notifies the background thread of updates.
pub(crate) fn notify() {
    if is_disabled() || STOPPING.load(Ordering::Relaxed) {
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

/// Forces an immediate refresh of the progress display.
pub fn flush() {
    if !*STARTED.lock().unwrap() {
        return;
    }
    if let Err(err) = refresh() {
        eprintln!("clx: {err:?}");
    }
}

/// Starts the background refresh thread if not already running.
fn start() {
    let mut started = STARTED.lock().unwrap();
    if *started
        || is_disabled()
        || output() == ProgressOutput::Text
        || STOPPING.load(Ordering::Relaxed)
    {
        return;
    }
    *started = true;
    drop(started);

    #[cfg(unix)]
    register_resize_handler();

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
            if check_resize_signaled() {
                LAST_OUTPUT.lock().unwrap().clear();
                continue;
            }
            notify_wait(interval());
        }
    });
}

/// Stops the progress display and renders the final state.
pub fn stop() {
    STOPPING.store(true, Ordering::Relaxed);
    let _ = refresh_once();
    clear_osc_progress();
    *STARTED.lock().unwrap() = false;
}

/// Stops the progress display and clears it from the screen.
pub fn stop_clear() {
    STOPPING.store(true, Ordering::Relaxed);
    let _ = clear();
    clear_osc_progress();
    *STARTED.lock().unwrap() = false;
}

// =============================================================================
// Job Management
// =============================================================================

/// Returns the number of top-level progress jobs currently registered.
#[must_use]
pub fn job_count() -> usize {
    JOBS.lock().unwrap().len()
}

/// Returns the number of currently active (running) progress jobs.
#[must_use]
pub fn active_jobs() -> usize {
    fn count_active(jobs: &[Arc<ProgressJob>]) -> usize {
        jobs.iter()
            .map(|job| {
                let is_active = job.status.lock().unwrap().is_active();
                let children = job.children.lock().unwrap();
                let child_count = count_active(&children);
                (if is_active { 1 } else { 0 }) + child_count
            })
            .sum()
    }
    count_active(&JOBS.lock().unwrap())
}

// =============================================================================
// Clear Display
// =============================================================================

/// Clears the progress display from the terminal.
pub(crate) fn clear() -> crate::Result<()> {
    let term = term();
    let mut lines = LINES.lock().unwrap();
    if *lines > 0 {
        let _guard = TERM_LOCK.lock().unwrap();
        term.move_cursor_up(*lines)?;
        term.move_cursor_left(term.size().1 as usize)?;
        term.clear_to_end_of_screen()?;
        drop(_guard);
    }
    *lines = 0;
    Ok(())
}

// =============================================================================
// OSC Progress
// =============================================================================

use crate::osc::{ProgressState, clear_progress, set_progress};

/// Updates OSC progress based on the current progress of all jobs.
pub(crate) fn update_osc_progress(jobs: &[Arc<ProgressJob>]) {
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

            let has_failed_jobs = check_for_failed_jobs(jobs);
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
    let (total_progress, job_count, has_failed_jobs) = calculate_average_progress(jobs);

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

fn check_for_failed_jobs(jobs: &[Arc<ProgressJob>]) -> bool {
    let mut stack: Vec<Arc<ProgressJob>> = jobs.to_vec();
    while let Some(job) = stack.pop() {
        if job.status.lock().unwrap().is_failed() {
            return true;
        }
        let children = job.children.lock().unwrap();
        for child in children.iter() {
            stack.push(child.clone());
        }
    }
    false
}

fn calculate_average_progress(jobs: &[Arc<ProgressJob>]) -> (f64, usize, bool) {
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

    (total_progress, job_count, has_failed_jobs)
}

/// Clear OSC progress indicator.
pub(crate) fn clear_osc_progress() {
    if crate::osc::is_enabled() {
        clear_progress();
        *LAST_OSC_PERCENTAGE.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_terminal_lock() {
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

        set_interval(original);
    }

    #[test]
    fn test_resize_signal_check() {
        let result = check_resize_signaled();
        assert!(!result);
    }
}
