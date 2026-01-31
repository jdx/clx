//! Integration tests for progress control flow.
//!
//! These tests verify the control functions like pause/resume, stop,
//! interval configuration, and output mode switching.

use clx::progress::{
    ProgressJobBuilder, ProgressOutput, ProgressStatus, flush, interval, is_paused, output, pause,
    resume, set_interval, set_output, stop, stop_clear, with_terminal_lock,
};
use std::time::Duration;

/// Set up text mode for testing (no terminal interaction needed)
fn setup() {
    set_output(ProgressOutput::Text);
}

#[test]
fn test_pause_resume() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Pause test")
        .start();

    // Initially not paused
    assert!(!is_paused());

    // Pause
    pause();
    assert!(is_paused());

    // Can still update job while paused
    job.prop("message", "Updated while paused");

    // Resume
    resume();
    assert!(!is_paused());

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_multiple_pause_resume() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Multi pause test")
        .start();

    // Multiple pause/resume cycles
    for _ in 0..5 {
        pause();
        assert!(is_paused());

        job.prop("message", "Paused");

        resume();
        assert!(!is_paused());

        job.prop("message", "Resumed");
    }

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_stop() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Stop test")
        .start();

    job.set_status(ProgressStatus::Done);

    // Stop should work without error
    stop();
}

#[test]
fn test_stop_clear() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Stop clear test")
        .start();

    // Stop and clear while still running
    stop_clear();

    // Job still exists but display is cleared
    assert!(job.is_running());
}

#[test]
fn test_flush() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Flush test")
        .start();

    // Multiple flushes should work
    flush();
    job.prop("message", "Updated");
    flush();
    job.prop("message", "Updated again");
    flush();

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_interval_configuration() {
    // Save original interval
    let original = interval();

    // Set new interval
    set_interval(Duration::from_millis(500));
    assert_eq!(interval(), Duration::from_millis(500));

    // Set another interval
    set_interval(Duration::from_millis(100));
    assert_eq!(interval(), Duration::from_millis(100));

    // Restore original
    set_interval(original);
    assert_eq!(interval(), original);
}

#[test]
fn test_output_mode_switching() {
    // Save original mode (may be Text if another test set it)
    let original = output();

    // Test UI mode setting works
    set_output(ProgressOutput::UI);
    assert_eq!(output(), ProgressOutput::UI);

    // Test Text mode setting works
    set_output(ProgressOutput::Text);
    assert_eq!(output(), ProgressOutput::Text);

    // Test switching back works
    set_output(ProgressOutput::UI);
    assert_eq!(output(), ProgressOutput::UI);

    // Restore original for other tests
    set_output(original);
}

#[test]
fn test_text_mode_output() {
    set_output(ProgressOutput::Text);

    let job = ProgressJobBuilder::new()
        .prop("message", "Text mode job")
        .start();

    // Updates in text mode should print immediately
    job.prop("message", "Updated in text mode");
    job.prop("message", "Updated again");

    job.set_status(ProgressStatus::Done);

    // Restore UI mode for other tests
    set_output(ProgressOutput::UI);
}

#[test]
fn test_text_mode_with_body_text() {
    set_output(ProgressOutput::Text);

    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message }}")
        .body_text(Some("[TEXT] {{ message }}"))
        .prop("message", "Custom text body")
        .start();

    job.set_status(ProgressStatus::Done);

    // Restore UI mode for other tests
    set_output(ProgressOutput::UI);
}

#[test]
fn test_println_during_progress() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Println test")
        .start();

    // println should work during progress
    job.println("Log message 1");
    job.println("Log message 2");
    job.println("Log message 3");

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_println_empty_string() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Empty println test")
        .start();

    // Empty string should be a no-op
    job.println("");

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_with_terminal_lock_return_value() {
    // Test that with_terminal_lock returns the closure's value
    let result = with_terminal_lock(|| 42);
    assert_eq!(result, 42);

    let result = with_terminal_lock(|| "hello".to_string());
    assert_eq!(result, "hello");

    let result: Option<i32> = with_terminal_lock(|| Some(123));
    assert_eq!(result, Some(123));
}

#[test]
fn test_with_terminal_lock_during_progress() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Lock test")
        .start();

    // Use terminal lock while progress is running
    let value = with_terminal_lock(|| {
        // Some work while holding the lock
        42
    });
    assert_eq!(value, 42);

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_pause_during_updates() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Pause during updates")
        .progress_total(100)
        .start();

    // Start updating
    for i in 0..50 {
        job.progress_current(i);
    }

    // Pause in the middle
    pause();

    // Continue updating while paused
    for i in 50..100 {
        job.progress_current(i);
    }

    // Resume
    resume();

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_ui_mode_basic() {
    // Note: UI mode will skip actual terminal operations in test environment
    // since there's no terminal attached, but it should not error
    set_output(ProgressOutput::UI);

    let job = ProgressJobBuilder::new()
        .prop("message", "UI mode job")
        .start();

    job.prop("message", "Updated");
    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_interval_affects_refresh() {
    setup();

    // Set a longer interval
    let original = interval();
    set_interval(Duration::from_millis(1000));

    let job = ProgressJobBuilder::new()
        .prop("message", "Interval test")
        .start();

    job.prop("message", "Updated");

    job.set_status(ProgressStatus::Done);

    // Restore original
    set_interval(original);
}

#[test]
fn test_control_with_children() {
    setup();

    let parent = ProgressJobBuilder::new().prop("message", "Parent").start();

    let child1 = parent.add(ProgressJobBuilder::new().prop("message", "Child 1").build());

    let child2 = parent.add(ProgressJobBuilder::new().prop("message", "Child 2").build());

    // Pause while children are running
    pause();

    child1.set_status(ProgressStatus::Done);

    // Resume
    resume();

    child2.set_status(ProgressStatus::Done);
    parent.set_status(ProgressStatus::Done);
}

#[test]
fn test_flush_after_pause_resume() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Flush after pause")
        .start();

    pause();
    job.prop("message", "Updated while paused");
    resume();

    // Flush after resume should work
    flush();

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_stop_while_paused() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Stop while paused")
        .start();

    pause();
    stop();

    // Job still exists
    assert!(job.is_running());

    // Restore pause state for other tests
    resume();
}

#[test]
fn test_stop_clear_while_paused() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Stop clear while paused")
        .start();

    pause();
    stop_clear();

    // Job still exists
    assert!(job.is_running());

    // Restore pause state for other tests
    resume();
}
