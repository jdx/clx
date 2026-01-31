//! Example demonstrating OSC terminal progress integration
//!
//! OSC 9;4 is a terminal escape sequence that shows progress in the terminal's
//! title bar or tab. This is supported by:
//! - Ghostty
//! - VS Code integrated terminal
//! - Windows Terminal
//! - VTE-based terminals (GNOME Terminal, etc.)
//!
//! Run with: cargo run --example osc_progress
//!
//! Note: If your terminal doesn't support OSC 9;4, the progress will still
//! display in the terminal but won't appear in the title bar.

use std::{thread, time::Duration};

use clx::progress::{ProgressJobBuilder, ProgressStatus};

fn main() {
    println!("=== OSC Terminal Progress Demo ===\n");

    // To disable OSC progress entirely, uncomment this line:
    // clx::osc::configure(false);

    println!("Starting progress job with OSC integration...");
    println!("Watch your terminal's title bar or tab for progress!\n");

    // Create a job with explicit progress tracking
    // OSC progress is automatically derived from progress_current/progress_total
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message }} [{{ cur }}/{{ total }}] {{ progress_bar(flex=true) }}")
        .prop("message", "Downloading")
        .progress_total(100)
        .progress_current(0)
        .start();

    // Simulate progress
    for i in 0..=100 {
        job.progress_current(i);
        job.prop("cur", &i);
        thread::sleep(Duration::from_millis(30));
    }

    job.set_status(ProgressStatus::Done);
    println!("\nPhase 1 complete!\n");

    thread::sleep(Duration::from_millis(500));

    // Demonstrate different progress states via job status
    println!("Demonstrating status-based OSC states...\n");

    // Normal progress
    let job2 = ProgressJobBuilder::new()
        .prop("message", "Processing files")
        .progress_total(50)
        .progress_current(0)
        .start();

    for i in 0..=25 {
        job2.progress_current(i);
        thread::sleep(Duration::from_millis(50));
    }

    // Simulate a failure - OSC will show error state
    println!("Simulating error state...");
    job2.set_status(ProgressStatus::Failed);
    thread::sleep(Duration::from_secs(1));

    // Start a new successful job
    let job3 = ProgressJobBuilder::new()
        .prop("message", "Retrying")
        .progress_total(50)
        .progress_current(0)
        .start();

    for i in 0..=50 {
        job3.progress_current(i);
        thread::sleep(Duration::from_millis(30));
    }
    job3.set_status(ProgressStatus::Done);

    println!("\n=== Demo Complete ===");
    println!("The OSC progress indicator should now be cleared.");

    // Progress is automatically cleared when all jobs complete
    thread::sleep(Duration::from_millis(100));
}
