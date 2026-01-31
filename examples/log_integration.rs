//! Example demonstrating log integration with progress display.
//!
//! Run with: cargo run --example log_integration --features log

use clx::progress::{ProgressJobBuilder, ProgressStatus, init_log_integration};
use log::{debug, error, info, warn};
use std::{thread, time::Duration};

fn main() {
    // Initialize the progress-aware logger
    // This must be called before any logging
    init_log_integration();

    info!("Starting application");

    // Create a progress job
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message }} [{{ cur }}/{{ total }}]")
        .prop("message", "Processing items")
        .progress_total(10)
        .start();

    for i in 0..10 {
        // Simulate work
        thread::sleep(Duration::from_millis(200));

        // Update progress
        job.progress_current(i + 1);

        // Log messages are automatically interleaved with progress
        // The progress display pauses, the log is written, then progress resumes
        match i {
            2 => debug!("Debug: processed item {}", i + 1),
            4 => info!("Info: halfway there!"),
            6 => warn!("Warning: item {} took longer than expected", i + 1),
            8 => error!("Error: simulated error at item {}", i + 1),
            _ => {}
        }
    }

    job.set_status(ProgressStatus::Done);
    info!("Application complete");
}
