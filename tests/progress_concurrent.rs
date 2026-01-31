//! Integration tests for concurrent access to progress jobs.
//!
//! These tests verify thread-safety of the progress system when accessed
//! from multiple threads simultaneously.

use clx::progress::{
    ProgressJobBuilder, ProgressOutput, ProgressStatus, set_output, with_terminal_lock,
};
use std::sync::Arc;
use std::thread;

/// Set up text mode for testing (no terminal interaction needed)
fn setup() {
    set_output(ProgressOutput::Text);
}

#[test]
fn test_concurrent_job_creation() {
    setup();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let job = ProgressJobBuilder::new()
                    .prop("message", &format!("Thread {} task", i))
                    .start();

                // Do some work
                thread::sleep(std::time::Duration::from_millis(10));

                job.set_status(ProgressStatus::Done);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_concurrent_progress_updates() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Concurrent progress")
        .progress_total(1000)
        .progress_current(0)
        .start();

    let job_arc = Arc::new(job);

    // Spawn multiple threads that update progress
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let job = Arc::clone(&job_arc);
            thread::spawn(move || {
                for j in 0..200 {
                    let progress = i * 200 + j;
                    // Note: progress updates from different threads may interleave,
                    // but each individual update should be atomic
                    job.progress_current(progress);
                    thread::yield_now();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    job_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_concurrent_child_additions() {
    setup();

    let parent = ProgressJobBuilder::new().prop("message", "Parent").start();

    let parent_arc = Arc::new(parent);

    // Multiple threads adding children
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let parent = Arc::clone(&parent_arc);
            thread::spawn(move || {
                for j in 0..3 {
                    let child = parent.add(
                        ProgressJobBuilder::new()
                            .prop("message", &format!("Child {}-{}", i, j))
                            .build(),
                    );
                    thread::sleep(std::time::Duration::from_millis(5));
                    child.set_status(ProgressStatus::Done);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // All children should have been added (5 threads x 3 children each)
    assert_eq!(parent_arc.children().len(), 15);

    parent_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_concurrent_prop_updates() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Initial")
        .prop("counter", &0)
        .start();

    let job_arc = Arc::new(job);

    // Multiple threads updating props
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let job = Arc::clone(&job_arc);
            thread::spawn(move || {
                for j in 0..100 {
                    job.prop("message", &format!("Thread {} iteration {}", i, j));
                    job.prop("counter", &(i * 100 + j));
                    thread::yield_now();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    job_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_terminal_lock_contention() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Lock contention test")
        .start();

    let job_arc = Arc::new(job);

    // Multiple threads acquiring terminal lock
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let job = Arc::clone(&job_arc);
            thread::spawn(move || {
                for j in 0..20 {
                    let _ = with_terminal_lock(|| {
                        // Simulate some work while holding the lock
                        let _ = &format!("Thread {} iteration {}", i, j);
                    });

                    // Also update the job
                    job.prop("message", &format!("Thread {} iteration {}", i, j));

                    thread::yield_now();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    job_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_concurrent_status_changes() {
    setup();

    // Create multiple jobs
    let jobs: Vec<_> = (0..10)
        .map(|i| {
            ProgressJobBuilder::new()
                .prop("message", &format!("Job {}", i))
                .start()
        })
        .collect();

    // Spawn threads to complete jobs in parallel
    let handles: Vec<_> = jobs
        .into_iter()
        .map(|job| {
            thread::spawn(move || {
                thread::sleep(std::time::Duration::from_millis(10));
                job.set_status(ProgressStatus::Done);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_concurrent_add_and_remove() {
    setup();

    let parent = ProgressJobBuilder::new().prop("message", "Parent").start();

    let parent_arc = Arc::new(parent);

    // Thread adding children
    let parent_add = Arc::clone(&parent_arc);
    let add_handle = thread::spawn(move || {
        for i in 0..20 {
            let _child = parent_add.add(
                ProgressJobBuilder::new()
                    .prop("message", &format!("Child {}", i))
                    .build(),
            );
            thread::sleep(std::time::Duration::from_millis(5));
        }
    });

    // Thread removing children
    let parent_remove = Arc::clone(&parent_arc);
    let remove_handle = thread::spawn(move || {
        for _ in 0..15 {
            thread::sleep(std::time::Duration::from_millis(7));
            let children = parent_remove.children();
            if let Some(child) = children.first() {
                child.remove();
            }
        }
    });

    add_handle.join().unwrap();
    remove_handle.join().unwrap();

    parent_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_concurrent_println() {
    setup();

    let job = ProgressJobBuilder::new()
        .prop("message", "Println test")
        .start();

    let job_arc = Arc::new(job);

    // Multiple threads calling println
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let job = Arc::clone(&job_arc);
            thread::spawn(move || {
                for j in 0..10 {
                    job.println(&&format!("Thread {} message {}", i, j));
                    thread::yield_now();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    job_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_parent_child_concurrent_updates() {
    setup();

    let parent = ProgressJobBuilder::new()
        .prop("message", "Parent")
        .progress_total(100)
        .start();

    let child = parent.add(
        ProgressJobBuilder::new()
            .prop("message", "Child")
            .progress_total(100)
            .build(),
    );

    let parent_arc = Arc::new(parent);
    let child_arc = Arc::new(child);

    // One thread updates parent
    let parent_clone = Arc::clone(&parent_arc);
    let parent_handle = thread::spawn(move || {
        for i in 0..=100 {
            parent_clone.progress_current(i);
            parent_clone.prop("message", &format!("Parent progress: {}", i));
            thread::yield_now();
        }
    });

    // Another thread updates child
    let child_clone = Arc::clone(&child_arc);
    let child_handle = thread::spawn(move || {
        for i in 0..=100 {
            child_clone.progress_current(i);
            child_clone.prop("message", &format!("Child progress: {}", i));
            thread::yield_now();
        }
    });

    parent_handle.join().unwrap();
    child_handle.join().unwrap();

    child_arc.set_status(ProgressStatus::Done);
    parent_arc.set_status(ProgressStatus::Done);
}

#[test]
fn test_stress_job_creation_and_completion() {
    setup();

    let handles: Vec<_> = (0..100)
        .map(|i| {
            thread::spawn(move || {
                let job = ProgressJobBuilder::new()
                    .prop("message", &format!("Stress job {}", i))
                    .start();

                // Vary the work time
                thread::sleep(std::time::Duration::from_micros((i * 10) as u64));

                job.set_status(if i % 3 == 0 {
                    ProgressStatus::Failed
                } else {
                    ProgressStatus::Done
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}
