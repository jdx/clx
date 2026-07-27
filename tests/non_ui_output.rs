//! Verifies that `Text` and `Quiet` modes never write terminal escapes.
//!
//! `stop()`/`stop_clear()` sit on the shutdown path of a CLI regardless of
//! output mode, so an escape emitted there lands in output the caller expects
//! to hold only its own text — for a `--quiet` run, to be empty. Escapes go to
//! the process's real stderr rather than through `eprint!`, so libtest's
//! capture cannot see them and a child process is the only way to observe what
//! was actually written.

use std::process::Command;

const SCENARIO_ENV: &str = "CLX_NON_UI_SCENARIO";

/// Child entry point. A no-op during a normal `cargo test` run; the parent
/// re-invokes it once per output mode and reads back the bytes it wrote.
#[test]
fn non_ui_shutdown_child_scenario() {
    let Some(mode) = std::env::var_os(SCENARIO_ENV) else {
        return;
    };

    use clx::progress::{ProgressJobBuilder, ProgressOutput, ProgressStatus, set_output};

    set_output(match mode.to_str() {
        Some("text") => ProgressOutput::Text,
        Some("quiet") => ProgressOutput::Quiet,
        other => panic!("unknown scenario {other:?}"),
    });

    let job = ProgressJobBuilder::new().prop("message", "working").start();
    job.prop("message", "still working");
    job.set_status(ProgressStatus::Done);

    clx::progress::stop_clear();
    clx::progress::stop();

    std::process::exit(0);
}

fn child_stderr(mode: &str) -> Vec<u8> {
    let output = Command::new(std::env::current_exe().expect("current_exe"))
        .args(["--exact", "non_ui_shutdown_child_scenario", "--nocapture"])
        .env(SCENARIO_ENV, mode)
        .output()
        .expect("spawn child");
    output.stderr
}

/// Every escape the progress renderer can emit. Text mode legitimately writes
/// its own status lines, so the assertion targets the escapes rather than
/// requiring stderr to be empty.
const ESCAPES: &[(&str, &[u8])] = &[
    ("show cursor", b"\x1b[?25h"),
    ("hide cursor", b"\x1b[?25l"),
    ("begin synchronized update", b"\x1b[?2026h"),
    ("end synchronized update", b"\x1b[?2026l"),
    ("cursor up", b"\x1b[1A"),
    ("clear to end of screen", b"\x1b[0J"),
];

fn assert_no_escapes(mode: &str, stderr: &[u8]) {
    for (name, escape) in ESCAPES {
        assert!(
            !stderr.windows(escape.len()).any(|w| w == *escape),
            "{mode} mode wrote a {name} escape to stderr: {:?}",
            String::from_utf8_lossy(stderr),
        );
    }
}

#[test]
fn text_mode_shutdown_writes_no_escapes() {
    assert_no_escapes("text", &child_stderr("text"));
}

#[test]
fn quiet_mode_writes_nothing_at_all() {
    let stderr = child_stderr("quiet");
    assert_no_escapes("quiet", &stderr);
    assert!(
        stderr.is_empty(),
        "quiet mode wrote to stderr: {:?}",
        String::from_utf8_lossy(&stderr),
    );
}
