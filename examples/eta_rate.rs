//! Example demonstrating elapsed time, ETA, and rate template functions
//!
//! Run with: cargo run --example eta_rate

use std::{thread, time::Duration};

use clx::progress::{ProgressJobBuilder, ProgressStatus};

fn main() {
    println!("=== ETA, Elapsed, and Rate Demo ===\n");

    // Create a job with elapsed time, ETA, and rate display
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message | cyan }} [{{ elapsed() | dim }}] {{ progress_bar(flex=true) }} {{ rate() | yellow }} ETA: {{ eta() | green }}")
        .prop("message", "Processing items")
        .progress_total(50)
        .progress_current(0)
        .start();

    // Simulate work with varying speeds
    for i in 0..=50 {
        job.progress_current(i);
        // Simulate varying processing time
        let delay = if i < 10 {
            80 // Slower start
        } else if i < 30 {
            40 // Speed up
        } else {
            60 // Slow down a bit
        };
        thread::sleep(Duration::from_millis(delay));
    }
    job.set_status(ProgressStatus::Done);

    thread::sleep(Duration::from_millis(300));

    println!("\n--- Color Filters Demo ---\n");

    // Demonstrate color filters
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ status | bold }}: {{ message | cyan }} ({{ count | yellow }})")
        .prop("status", "Downloading")
        .prop("message", "package.tar.gz")
        .prop("count", "0 bytes")
        .start();

    for i in 0..=10 {
        job.prop("count", &format!("{} KB", i * 100));
        thread::sleep(Duration::from_millis(100));
    }
    job.prop("status", "Complete");
    job.set_status(ProgressStatus::Done);

    thread::sleep(Duration::from_millis(300));

    println!("\n--- Conditional Rendering Demo ---\n");

    // Demonstrate Tera conditionals (built-in)
    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message }}{% if show_details %} ({{ details | dim }}){% endif %}")
        .prop("message", "Building")
        .prop("show_details", &false)
        .start();

    thread::sleep(Duration::from_millis(500));
    job.prop("show_details", &true);
    job.prop("details", "compiling main.rs");
    thread::sleep(Duration::from_millis(500));
    job.prop("details", "linking");
    thread::sleep(Duration::from_millis(500));
    job.set_status(ProgressStatus::Done);

    thread::sleep(Duration::from_millis(100));
    println!("\n=== Demo Complete ===");
}
