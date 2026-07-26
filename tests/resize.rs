//! Verifies that redraws stay anchored while the terminal changes size.
#![cfg(unix)]

use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";

#[test]
fn resize_child_scenario() {
    if std::env::var_os("CLX_RESIZE_PTY_SCENARIO").is_none() {
        return;
    }

    use clx::progress::{ProgressJobBuilder, ProgressStatus, set_interval};

    set_interval(Duration::from_millis(800));
    let job = ProgressJobBuilder::new().body(&"x".repeat(60)).start();
    thread::sleep(Duration::from_secs(4));
    job.set_status(ProgressStatus::Done);
    clx::progress::stop();

    std::process::exit(0);
}

#[test]
fn resize_clears_the_reflowed_frame_height() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 20,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(std::env::current_exe().expect("current_exe"));
    cmd.args(["--exact", "resize_child_scenario", "--nocapture"]);
    cmd.env("CLX_RESIZE_PTY_SCENARIO", "1");

    let mut child = pair.slave.spawn_command(cmd).expect("spawn child");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let (tx, rx) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(count) = reader.read(&mut chunk) {
            if count == 0 || tx.send(chunk[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut initial = Vec::new();
    assert!(
        wait_for_frames(&rx, &mut initial, 1, Duration::from_secs(10)),
        "progress display did not render its initial frame"
    );
    while rx.try_recv().is_ok() {}

    for cols in [30, 40] {
        pair.master
            .resize(PtySize {
                rows: 24,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize pty while dragging");

        let mut resized = Vec::new();
        assert!(
            wait_for_frames(&rx, &mut resized, 1, Duration::from_secs(3)),
            "did not redraw at {cols} columns: {}",
            String::from_utf8_lossy(&resized).escape_debug()
        );
    }

    // A frame that fills the viewport would scroll its anchored first row into
    // scrollback, so keep the existing frame until there is room.
    pair.master
        .resize(PtySize {
            rows: 3,
            cols: 20,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("resize pty to exact frame height");

    let mut full = Vec::new();
    assert!(
        !wait_for_frames(&rx, &mut full, 1, Duration::from_millis(500)),
        "redrew a frame that filled the viewport: {}",
        String::from_utf8_lossy(&full).escape_debug()
    );

    pair.master
        .resize(PtySize {
            rows: 2,
            cols: 20,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("shrink pty");

    let mut cramped = Vec::new();
    assert!(
        !wait_for_frames(&rx, &mut cramped, 1, Duration::from_secs(1)),
        "redrew a frame that could not be fully cleared: {}",
        String::from_utf8_lossy(&cramped).escape_debug()
    );

    pair.master
        .resize(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("resize pty");

    let mut resized = Vec::new();
    assert!(
        wait_for_frames(&rx, &mut resized, 1, Duration::from_secs(10)),
        "progress display did not redraw after resize"
    );

    child.wait().expect("wait child");
    drop(pair.master);
    reader_thread.join().expect("join reader");

    let frame = first_frame(&resized).unwrap_or(&resized);
    assert!(
        frame.windows(4).any(|window| window == b"\x1b[0J"),
        "redraw did not clear downward from the frame anchor: {}",
        String::from_utf8_lossy(frame).escape_debug()
    );
    assert_eq!(
        cursor_up_amount(frame),
        Some(1),
        "redraw did not return the cursor to the frame anchor: {}",
        String::from_utf8_lossy(frame).escape_debug()
    );
}

fn wait_for_frames(
    rx: &Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    expected: usize,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while occurrences(output, END) < expected {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match rx.recv_timeout(remaining) {
            Ok(chunk) => output.extend(chunk),
            Err(_) => return false,
        }
    }
    true
}

fn first_frame(output: &[u8]) -> Option<&[u8]> {
    let start = output
        .windows(BEGIN.len())
        .position(|window| window == BEGIN)?;
    let rest = &output[start + BEGIN.len()..];
    let end = rest.windows(END.len()).position(|window| window == END)?;
    Some(&rest[..end])
}

fn cursor_up_amount(frame: &[u8]) -> Option<usize> {
    frame.windows(2).enumerate().find_map(|(index, window)| {
        if window != b"\x1b[" {
            return None;
        }
        let digits = &frame[index + 2..];
        let end = digits.iter().position(|byte| *byte == b'A')?;
        if digits[..end].iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(&digits[..end]).ok()?.parse().ok()
        } else {
            None
        }
    })
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}
