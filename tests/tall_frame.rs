//! Verifies frames taller than the terminal are not duplicated in the output.
#![cfg(unix)]

use std::io::Read;
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const JOBS: usize = 30;

#[test]
fn tall_final_frame_child_scenario() {
    if std::env::var_os("CLX_TALL_FINAL_FRAME_SCENARIO").is_none() {
        return;
    }

    use clx::progress::{ProgressJobBuilder, ProgressStatus, set_interval};

    set_interval(Duration::from_millis(25));
    let jobs: Vec<_> = (1..=JOBS)
        .map(|i| {
            ProgressJobBuilder::new()
                .prop("message", &format!("job-{i}"))
                .body("{{ spinner() }} {{ message }}")
                .start()
        })
        .collect();
    thread::sleep(Duration::from_millis(300));
    for job in &jobs {
        thread::sleep(Duration::from_millis(10));
        job.set_status(ProgressStatus::Done);
    }
    clx::progress::stop();

    std::process::exit(0);
}

/// Runs the scenario in a pty of the given height and returns every line the
/// terminal retained, scrollback first.
fn run_scenario(rows: u16) -> Vec<String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(std::env::current_exe().expect("current_exe"));
    cmd.args(["--exact", "tall_final_frame_child_scenario", "--nocapture"]);
    cmd.env("CLX_TALL_FINAL_FRAME_SCENARIO", "1");

    let mut child = pair.slave.spawn_command(cmd).expect("spawn child");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let reader_thread = thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0; 4096];
        while let Ok(count) = reader.read(&mut chunk) {
            if count == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..count]);
        }
        output
    });
    let status = child.wait().expect("wait child");
    drop(pair.master);
    let output = reader_thread.join().expect("join reader");

    assert!(status.success(), "child scenario failed: {status}");

    let mut parser = vt100::Parser::new(rows, 80, 1000);
    parser.process(&output);
    let screen = parser.screen_mut();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let mut lines = Vec::new();
    for offset in (1..=history).rev() {
        screen.set_scrollback(offset);
        lines.push(screen.rows(0, 80).next().unwrap_or_default());
    }
    screen.set_scrollback(0);
    lines.extend(screen.rows(0, 80));
    lines
}

fn copies_of(lines: &[String], label: &str) -> usize {
    lines
        .iter()
        .filter(|line| line.trim_end().ends_with(label))
        .count()
}

#[test]
fn final_frame_taller_than_the_terminal_is_written_once() {
    for rows in [10, JOBS as u16, JOBS as u16 + 1] {
        let lines = run_scenario(rows);
        for label in ["✔ job-1", "✔ job-15", "✔ job-30"] {
            assert_eq!(
                copies_of(&lines, label),
                1,
                "{label} at {rows} rows: {lines:#?}"
            );
        }
    }
}

#[test]
fn tmux_running_frame_child_scenario() {
    let Some(count) = std::env::var("CLX_TMUX_RUNNING_JOBS")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
    else {
        return;
    };

    use clx::progress::{ProgressJobBuilder, ProgressStatus, set_interval};

    // Finish the first `done` jobs so the running ones sit below finished rows.
    let done: usize = std::env::var("CLX_TMUX_DONE_JOBS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    set_interval(Duration::from_millis(25));
    let jobs: Vec<_> = (1..=count)
        .map(|i| {
            ProgressJobBuilder::new()
                .prop("message", &format!("job-{i}"))
                .body("{{ spinner() }} {{ message }}")
                .start()
        })
        .collect();
    for job in jobs.iter().take(done) {
        job.set_status(ProgressStatus::Done);
    }
    thread::sleep(Duration::from_secs(30));

    std::process::exit(0);
}

/// Runs `count` running jobs in a fresh 80x24 tmux pane and returns the pane
/// text, scrollback included, after the progress display has redrawn for a while.
fn tmux_pane_after_redraws(tmux: &std::ffi::OsStr, count: usize, done: usize) -> Vec<String> {
    use std::process::Command;

    let socket = format!("clx-running-test-{}-{count}-{done}", std::process::id());
    let test_binary = std::env::current_exe().expect("current_exe");
    // tmux runs the command through a shell, so quote the path.
    let child_command = format!(
        "env CLX_TMUX_RUNNING_JOBS={count} CLX_TMUX_DONE_JOBS={done} '{}' --exact tmux_running_frame_child_scenario --nocapture",
        test_binary.display().to_string().replace('\'', "'\\''")
    );
    let run = |args: &[&str]| {
        Command::new(tmux)
            .args(["-L", &socket, "-f", "/dev/null"])
            .args(args)
            .output()
            .expect("run tmux")
    };
    struct Cleanup<'a>(&'a dyn Fn());
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    let kill = || {
        let _ = run(&["kill-server"]);
    };
    let _cleanup = Cleanup(&kill);

    let started = run(&[
        "new-session",
        "-d",
        "-x",
        "80",
        "-y",
        "24",
        "-s",
        "clx-running",
        &child_command,
    ]);
    assert!(started.status.success(), "tmux new-session failed");
    // Let a few dozen redraws happen.
    thread::sleep(Duration::from_secs(2));
    let captured = run(&["capture-pane", "-p", "-t", "clx-running", "-S", "-"]);
    assert!(captured.status.success(), "tmux capture-pane failed");
    String::from_utf8_lossy(&captured.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn tmux_running_frame_leaves_no_copies_in_scrollback() {
    let Some(tmux) = std::env::var_os("CLX_TMUX_BIN") else {
        return;
    };

    // Shorter than the terminal, and as tall as it can be while still fitting:
    // every redraw used to leave a copy in history.
    for count in [10, 23] {
        let lines = tmux_pane_after_redraws(&tmux, count, 0);
        assert_eq!(copies_of(&lines, "job-1"), 1, "{count} jobs: {lines:#?}");
    }

    // Taller than the terminal: the screen used to stay blank.
    let lines = tmux_pane_after_redraws(&tmux, 30, 0);
    assert_eq!(copies_of(&lines, "job-1"), 1, "{lines:#?}");
    assert!(
        lines.iter().any(|line| line.contains("more lines")),
        "no summary of the hidden jobs: {lines:#?}"
    );
}

#[test]
fn tmux_running_job_stays_visible_below_finished_jobs() {
    let Some(tmux) = std::env::var_os("CLX_TMUX_BIN") else {
        return;
    };

    // 25 of 30 jobs are done; the five running ones are the last five rows of
    // the full frame and must not be cut in favor of the finished rows above.
    let lines = tmux_pane_after_redraws(&tmux, 30, 25);
    for label in ["job-26", "job-30"] {
        assert_eq!(copies_of(&lines, label), 1, "{label}: {lines:#?}");
    }
    assert!(
        lines.iter().any(|line| line.contains("more lines")),
        "no summary of the hidden jobs: {lines:#?}"
    );
}
