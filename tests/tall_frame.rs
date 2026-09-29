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
    child.wait().expect("wait child");
    drop(pair.master);
    let output = reader_thread.join().expect("join reader");

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
