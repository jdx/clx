//! Integration tests for progress job lifecycle.
//!
//! These tests verify the full lifecycle of progress jobs from creation through
//! completion, including status transitions, child jobs, and done behaviors.

use clx::progress::{
    ProgressJobBuilder, ProgressJobDoneBehavior, ProgressOutput, ProgressStatus, set_output,
};
use std::sync::Arc;

/// Set up text mode for testing (no terminal interaction needed)
fn setup() {
    set_output(ProgressOutput::Text);
}

#[test]
fn test_basic_lifecycle() {
    setup();

    // Create and start a job
    let job = ProgressJobBuilder::new()
        .prop("message", "Test task")
        .start();

    // Should be running initially
    assert!(job.is_running());

    // Transition to done
    job.set_status(ProgressStatus::Done);
    assert!(!job.is_running());
}

#[test]
fn test_lifecycle_with_children() {
    setup();

    // Create parent job
    let parent = ProgressJobBuilder::new()
        .prop("message", "Parent task")
        .start();

    // Add child jobs
    let child1 = parent.add(ProgressJobBuilder::new().prop("message", "Child 1").build());

    let child2 = parent.add(ProgressJobBuilder::new().prop("message", "Child 2").build());

    // All should be running
    assert!(parent.is_running());
    assert!(child1.is_running());
    assert!(child2.is_running());

    // Children should be tracked
    let children = parent.children();
    assert_eq!(children.len(), 2);

    // Complete children first
    child1.set_status(ProgressStatus::Done);
    child2.set_status(ProgressStatus::Done);

    // Parent still running
    assert!(parent.is_running());
    assert!(!child1.is_running());
    assert!(!child2.is_running());

    // Complete parent
    parent.set_status(ProgressStatus::Done);
    assert!(!parent.is_running());
}

#[test]
fn test_progress_tracking() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Download")
        .progress_total(100)
        .progress_current(0)
        .start();

    // Update progress
    for i in 0..=100 {
        job.progress_current(i);
    }

    job.set_status(ProgressStatus::Done);
    assert!(!job.is_running());
}

#[test]
fn test_status_transitions() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Task")
        .status(ProgressStatus::Pending)
        .start();

    // Should start as pending
    assert!(!job.is_running());

    // Transition through states
    job.set_status(ProgressStatus::Running);
    assert!(job.is_running());

    job.set_status(ProgressStatus::Warn);
    assert!(!job.is_running());

    job.set_status(ProgressStatus::Running);
    assert!(job.is_running());

    job.set_status(ProgressStatus::Failed);
    assert!(!job.is_running());
}

#[test]
fn test_job_removal() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Temporary task")
        .start();

    assert!(job.is_running());

    // Remove the job
    job.remove();

    // Job still exists (just removed from display), but is_running still works
    assert!(job.is_running());
}

#[test]
fn test_child_removal() {
    setup();

    let parent = ProgressJobBuilder::new().prop("message", "Parent").start();

    let child = parent.add(ProgressJobBuilder::new().prop("message", "Child").build());

    assert_eq!(parent.children().len(), 1);

    // Remove child
    child.remove();

    assert_eq!(parent.children().len(), 0);
}

#[test]
fn test_done_behavior_keep() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Keep task")
        .on_done(ProgressJobDoneBehavior::Keep)
        .start();

    let child = job.add(ProgressJobBuilder::new().prop("message", "Child").build());

    child.set_status(ProgressStatus::Done);
    job.set_status(ProgressStatus::Done);

    // Children should still be accessible
    assert_eq!(job.children().len(), 1);
}

#[test]
fn test_done_behavior_hide() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Hidden task")
        .on_done(ProgressJobDoneBehavior::Hide)
        .start();

    job.set_status(ProgressStatus::Done);
    // Job should complete without error
    assert!(!job.is_running());
}

#[test]
fn test_done_behavior_collapse() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Collapse task")
        .on_done(ProgressJobDoneBehavior::Collapse)
        .start();

    let _child = job.add(ProgressJobBuilder::new().prop("message", "Child").build());

    job.set_status(ProgressStatus::Done);

    // Children should still exist in the data structure
    assert_eq!(job.children().len(), 1);
}

#[test]
fn test_deeply_nested_jobs() {
    setup();

    let level1 = ProgressJobBuilder::new().prop("message", "Level 1").start();

    let level2 = level1.add(ProgressJobBuilder::new().prop("message", "Level 2").build());

    let level3 = level2.add(ProgressJobBuilder::new().prop("message", "Level 3").build());

    let level4 = level3.add(ProgressJobBuilder::new().prop("message", "Level 4").build());

    // All should be running
    assert!(level1.is_running());
    assert!(level2.is_running());
    assert!(level3.is_running());
    assert!(level4.is_running());

    // Complete from deepest to shallowest
    level4.set_status(ProgressStatus::Done);
    level3.set_status(ProgressStatus::Done);
    level2.set_status(ProgressStatus::Done);
    level1.set_status(ProgressStatus::Done);

    assert!(!level1.is_running());
}

#[test]
fn test_multiple_top_level_jobs() {
    setup();

    let job1 = ProgressJobBuilder::new().prop("message", "Job 1").start();

    let job2 = ProgressJobBuilder::new().prop("message", "Job 2").start();

    let job3 = ProgressJobBuilder::new().prop("message", "Job 3").start();

    assert!(job1.is_running());
    assert!(job2.is_running());
    assert!(job3.is_running());

    // Complete in any order
    job2.set_status(ProgressStatus::Done);
    job1.set_status(ProgressStatus::Failed);
    job3.set_status(ProgressStatus::Done);
}

#[test]
fn test_custom_status() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Custom status")
        .start();

    // Custom running status
    job.set_status(ProgressStatus::RunningCustom("🚀".to_string()));
    assert!(job.is_running());

    // Custom done status
    job.set_status(ProgressStatus::DoneCustom("🎉".to_string()));
    assert!(!job.is_running());
}

#[test]
fn test_prop_updates() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Initial")
        .prop("count", &0)
        .start();

    // Update props
    job.prop("message", "Updated");
    job.prop("count", &42);
    job.prop("new_prop", "new value");

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_body_update() {
    setup();

    let job = ProgressJobBuilder::new()
        .body("{{ spinner() }} {{ message }}")
        .prop("message", "Initial body")
        .start();

    // Change the body template
    job.set_body("{{ spinner() }} [NEW] {{ message }}");

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_progress_clamping() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Clamping test")
        .progress_total(100)
        .start();

    // Progress should be clamped to total
    job.progress_current(150);

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_total_adjustment() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Total adjustment")
        .progress_current(80)
        .start();

    // Total should be adjusted to at least current
    job.progress_total(50);

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_hide_status() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Hidden")
        .status(ProgressStatus::Hide)
        .start();

    // Hide is not considered running
    assert!(!job.is_running());

    job.set_status(ProgressStatus::Running);
    assert!(job.is_running());

    job.set_status(ProgressStatus::Done);
}

#[test]
fn test_arc_cloning() {
    setup();

    let job: Arc<_> = ProgressJobBuilder::new()
        .prop("message", "Cloned job")
        .start();

    // Clone the Arc
    let job_clone = Arc::clone(&job);

    // Both references should work
    job.prop("message", "Updated via original");
    job_clone.prop("message", "Updated via clone");

    job.set_status(ProgressStatus::Done);

    // Both should see the same status
    assert!(!job.is_running());
    assert!(!job_clone.is_running());
}
