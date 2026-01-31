//! Diagnostic frame logging for debugging progress display.
//!
//! When enabled via the `CLX_TRACE_LOG` environment variable, this module logs
//! each rendered frame as JSONL with the rendered text and structured job state.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{LineWriter, Write};
use std::sync::{Mutex, OnceLock};

use super::ProgressJob;
use std::sync::Arc;

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

/// Snapshot of a single job's state.
#[derive(Debug, Clone, Serialize)]
pub struct JobSnapshot {
    pub id: usize,
    pub status: String,
    pub message: Option<String>,
    pub progress: Option<(usize, usize)>,
    pub children: Vec<JobSnapshot>,
}

impl JobSnapshot {
    /// Create a snapshot from a ProgressJob.
    pub fn from_job(job: &ProgressJob) -> Self {
        let status = job.status.lock().unwrap();
        let status_str = match &*status {
            super::ProgressStatus::Hide => "hide",
            super::ProgressStatus::Pending => "pending",
            super::ProgressStatus::Running => "running",
            super::ProgressStatus::RunningCustom(_) => "running",
            super::ProgressStatus::DoneCustom(_) => "done",
            super::ProgressStatus::Done => "done",
            super::ProgressStatus::Warn => "warn",
            super::ProgressStatus::Failed => "failed",
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

/// Frame event emitted for each refresh.
#[derive(Debug, Clone, Serialize)]
pub struct FrameEvent {
    pub rendered: String,
    pub jobs: Vec<JobSnapshot>,
}

/// Log a frame event to the trace log file.
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
