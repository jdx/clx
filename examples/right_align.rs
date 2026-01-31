//! Example demonstrating right-aligned progress bars using flex_fill
//!
//! The `flex_fill` filter pads content with spaces to fill available width,
//! which pushes subsequent content (like progress bars) to the right edge.
//!
//! Run with: cargo run --example right_align

use std::{thread, time::Duration};

use clx::progress::{ProgressJobBuilder, ProgressStatus};

fn main() {
    println!("=== Right-Aligned Progress Bar Demo ===\n");

    // Create a job with right-aligned progress bar
    // The flex_fill filter pads the message to push the progress bar right
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message | flex_fill }}{{ progress_bar(flex=true) }}")
        .prop("message", "Downloading files")
        .progress_total(100)
        .progress_current(0)
        .start();

    // Simulate progress
    for i in 0..=100 {
        job.progress_current(i);
        thread::sleep(Duration::from_millis(20));
    }
    job.set_status(ProgressStatus::Done);

    thread::sleep(Duration::from_millis(300));

    // Another example with different message lengths
    println!("\n--- Varying message lengths ---\n");

    let tasks = [
        ("Short", 50),
        ("Medium length task", 30),
        ("A much longer task description here", 40),
    ];

    for (msg, steps) in tasks {
        let job = ProgressJobBuilder::new()
            .body("{{ spinner() }} {{ message | flex_fill }}[{{ cur }}/{{ total }}]")
            .prop("message", msg)
            .progress_total(steps)
            .progress_current(0)
            .start();

        for i in 0..=steps {
            job.progress_current(i);
            job.prop("cur", &i);
            job.prop("total", &steps);
            thread::sleep(Duration::from_millis(30));
        }
        job.set_status(ProgressStatus::Done);
        thread::sleep(Duration::from_millis(200));
    }

    println!("\n=== Demo Complete ===");
    thread::sleep(Duration::from_millis(100));
}
