//! Verifies that in-place redraws are wrapped in DEC mode 2026 synchronized
//! updates so a fast terminal cannot present a half-drawn frame.
//!
//! `write_frame` never runs in the crate's other tests, which all use text
//! mode. Driving the real UI-mode render path requires a tty, so the parent
//! test spawns this same binary under a pseudo-terminal, runs a scenario in
//! the child, and inspects the raw byte stream the child wrote.
//!
//! Unix only. The guard itself is platform-independent, but this test reads
//! the escape bytes back off a pty, and ConPTY does not fit that: its master
//! does not reliably signal EOF when the child exits, and it parses and
//! re-emits the stream rather than passing sequences through verbatim.
#![cfg(unix)]

use std::io::Read;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const BEGIN: &[u8] = b"\x1b[?2026h";

/// Child entry point. A no-op during a normal `cargo test` run; the parent
/// re-invokes it with `CLX_PTY_SCENARIO=1` under a pty to produce real frames.
#[test]
fn synchronized_output_child_scenario() {
    if std::env::var_os("CLX_PTY_SCENARIO").is_none() {
        return;
    }

    use clx::progress::{ProgressJobBuilder, ProgressStatus};

    let root = ProgressJobBuilder::new().prop("message", "root").start();
    let child = ProgressJobBuilder::new().prop("message", "child").start();
    for i in 0..5 {
        std::thread::sleep(Duration::from_millis(60));
        child.prop("message", &format!("step {i}"));
        root.println(&format!("log line {i}"));
    }
    std::thread::sleep(Duration::from_millis(120));
    child.set_status(ProgressStatus::Done);
    root.set_status(ProgressStatus::Done);
    // Let the background refresh observe completion and leave its final frame.
    // Calling stop after that must not append a duplicate frame below it.
    std::thread::sleep(Duration::from_millis(250));
    eprintln!("CLX_BEFORE_STOP");
    clx::progress::stop();
    eprintln!("CLX_AFTER_STOP");

    std::process::exit(0);
}

#[test]
fn every_redraw_is_wrapped_in_a_synchronized_update() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(std::env::current_exe().expect("current_exe"));
    cmd.args([
        "--exact",
        "synchronized_output_child_scenario",
        "--nocapture",
    ]);
    cmd.env("CLX_PTY_SCENARIO", "1");

    let mut child = pair.slave.spawn_command(cmd).expect("spawn child");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("read pty");
    child.wait().expect("wait child");
    drop(pair.master);

    assert!(
        contains(&bytes, BEGIN),
        "scenario emitted no synchronized-update sequences; the render path did not run"
    );
    let after_completion = bytes
        .split(|byte| *byte == b'\n')
        .skip_while(|line| !contains(line, b"CLX_BEFORE_STOP"))
        .skip(1)
        .take_while(|line| !contains(line, b"CLX_AFTER_STOP"))
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(
        !contains(&after_completion, b"root") && !contains(&after_completion, b"step"),
        "stop redrew a frame after the background thread had already completed it: {}",
        String::from_utf8_lossy(&after_completion).escape_debug()
    );

    let mut open = false;
    for op in escapes(&bytes) {
        match op {
            Escape::Begin => {
                assert!(
                    !open,
                    "nested synchronized update: a second begin with no end"
                );
                open = true;
            }
            Escape::End => {
                assert!(open, "synchronized-update end with no matching begin");
                open = false;
            }
            Escape::Erase => assert!(
                open,
                "in-place redraw (cursor-up or clear-to-end-of-screen) outside a synchronized update"
            ),
        }
    }
    assert!(!open, "stream ended with a synchronized update left open");
}

enum Escape {
    Begin,
    End,
    Erase,
}

/// Yields the redraw-relevant CSI sequences in order: the mode-2026 begin/end
/// pair and the erase operations (cursor-up `ESC[<n>A`, clear-to-end `ESC[0J`).
/// Every other byte, including OSC sequences and the frame text, is ignored.
fn escapes(bytes: &[u8]) -> Vec<Escape> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b || bytes.get(i + 1) != Some(&b'[') {
            i += 1;
            continue;
        }
        let params_start = i + 2;
        let mut j = params_start;
        while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let final_byte = bytes[j];
        let params = &bytes[params_start..j];
        match final_byte {
            b'h' if params == b"?2026" => out.push(Escape::Begin),
            b'l' if params == b"?2026" => out.push(Escape::End),
            b'A' | b'J' => out.push(Escape::Erase),
            _ => {}
        }
        i = j + 1;
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
