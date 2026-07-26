//! Verifies that redraws stay anchored while the terminal changes size.
#![cfg(unix)]

use std::io::Read;
use std::process::Command;
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

    set_interval(Duration::from_millis(25));
    let job = ProgressJobBuilder::new().body(&"x".repeat(60)).start();
    thread::sleep(Duration::from_secs(4));
    job.set_status(ProgressStatus::Done);
    clx::progress::stop();

    std::process::exit(0);
}

#[test]
fn cramped_stop_child_scenario() {
    if std::env::var_os("CLX_CRAMPED_STOP_SCENARIO").is_none() {
        return;
    }

    use clx::progress::{ProgressJobBuilder, set_interval};

    set_interval(Duration::from_millis(25));
    let _job = ProgressJobBuilder::new().body(&"x".repeat(60)).start();
    thread::sleep(Duration::from_millis(1500));
    clx::progress::stop();

    std::process::exit(0);
}

#[test]
fn tmux_resize_child_scenario() {
    if std::env::var_os("CLX_TMUX_RESIZE_SCENARIO").is_none() {
        return;
    }

    use clx::progress::{ProgressJobBuilder, set_interval};

    set_interval(Duration::from_millis(25));
    let _jobs = ["TMUX_ROW_ALPHA", "TMUX_ROW_BRAVO", "TMUX_ROW_CHARLIE"].map(|label| {
        ProgressJobBuilder::new()
            .body(&format!("{{{{ spinner() }}}} {label}"))
            .start()
    });
    thread::sleep(Duration::from_secs(30));

    std::process::exit(0);
}

#[test]
fn tmux_resize_keeps_one_copy_of_each_progress_row() {
    let Some(tmux) = std::env::var_os("CLX_TMUX_BIN") else {
        return;
    };

    let socket = format!("clx-resize-test-{}", std::process::id());
    let session = "clx-resize";
    let test_binary = std::env::current_exe().expect("current_exe");
    let child_command = format!(
        "env CLX_TMUX_RESIZE_SCENARIO=1 {} --exact tmux_resize_child_scenario --nocapture",
        test_binary.display()
    );
    let cleanup = TmuxCleanup {
        binary: tmux.clone(),
        socket: socket.clone(),
    };

    let status = Command::new(&tmux)
        .args([
            "-L",
            &socket,
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-x",
            "120",
            "-y",
            "24",
            "-s",
            session,
            &child_command,
        ])
        .status()
        .expect("start tmux");
    assert!(status.success(), "tmux new-session failed");

    assert_unique_tmux_rows(&tmux, &socket, session, Duration::from_secs(10));
    for columns in (40..=110).rev().step_by(10) {
        resize_tmux(&tmux, &socket, session, columns);
        thread::sleep(Duration::from_millis(20));
    }
    assert_unique_tmux_rows(&tmux, &socket, session, Duration::from_secs(10));
    for columns in (50..=120).step_by(10) {
        resize_tmux(&tmux, &socket, session, columns);
        thread::sleep(Duration::from_millis(20));
    }
    assert_unique_tmux_rows(&tmux, &socket, session, Duration::from_secs(10));

    drop(cleanup);
}

#[test]
fn resize_resets_a_frame_that_outgrows_the_viewport() {
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
            wait_for_frames(&rx, &mut resized, 2, Duration::from_secs(3)),
            "did not redraw at {cols} columns: {}",
            String::from_utf8_lossy(&resized).escape_debug()
        );
        let frame = synchronized_frame(&resized, 1).unwrap_or(&resized);
        assert!(
            frame.windows(4).any(|window| window == b"\x1b[2J"),
            "settled resize redraw did not reset the visible viewport: {}",
            String::from_utf8_lossy(frame).escape_debug()
        );
        assert!(
            String::from_utf8_lossy(frame).contains('x'),
            "settled resize redraw did not contain the new frame: {}",
            String::from_utf8_lossy(frame).escape_debug()
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
        wait_for_frames(&rx, &mut full, 1, Duration::from_secs(3)),
        "did not reset a viewport filled by the frame: {}",
        String::from_utf8_lossy(&full).escape_debug()
    );
    let reset = first_frame(&full).unwrap_or(&full);
    assert!(
        reset.windows(4).any(|window| window == b"\x1b[2J"),
        "cramped resize did not clear the visible viewport: {}",
        String::from_utf8_lossy(reset).escape_debug()
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
        wait_for_frames(&rx, &mut cramped, 1, Duration::from_secs(3)),
        "did not clear again while the cramped terminal kept resizing: {}",
        String::from_utf8_lossy(&cramped).escape_debug()
    );
    let cramped_reset = first_frame(&cramped).unwrap_or(&cramped);
    assert!(
        cramped_reset.windows(4).any(|window| window == b"\x1b[2J")
            && !String::from_utf8_lossy(cramped_reset).contains('x'),
        "cramped resize did not remain blank: {}",
        String::from_utf8_lossy(cramped_reset).escape_debug()
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
        wait_for_frames(&rx, &mut resized, 2, Duration::from_secs(10)),
        "progress display did not redraw after resize"
    );

    child.wait().expect("wait child");
    drop(pair.master);
    reader_thread.join().expect("join reader");

    let frame = synchronized_frame(&resized, 1).unwrap_or(&resized);
    assert!(
        String::from_utf8_lossy(frame).contains('x'),
        "recovered frame did not render its output: {}",
        String::from_utf8_lossy(frame).escape_debug()
    );
}

#[test]
fn stop_restores_cursor_after_cramped_viewport_suppression() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 20,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(std::env::current_exe().expect("current_exe"));
    cmd.args(["--exact", "cramped_stop_child_scenario", "--nocapture"]);
    cmd.env("CLX_CRAMPED_STOP_SCENARIO", "1");

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
        wait_for_frames(&rx, &mut initial, 1, Duration::from_secs(5)),
        "progress display did not render its initial frame"
    );
    pair.master
        .resize(PtySize {
            rows: 2,
            cols: 20,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("shrink pty");

    let mut stopped = Vec::new();
    assert!(
        wait_for_frames(&rx, &mut stopped, 1, Duration::from_secs(3)),
        "cramped viewport did not emit a reset"
    );
    child.wait().expect("wait child");
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
        stopped.extend(chunk);
    }
    drop(pair.master);
    reader_thread.join().expect("join reader");

    let reset = stopped
        .windows(4)
        .position(|window| window == b"\x1b[2J")
        .expect("cramped viewport did not clear");
    assert!(
        stopped[reset..]
            .windows(6)
            .any(|window| window == b"\x1b[?25h"),
        "stop did not restore cursor visibility after viewport suppression: {}",
        String::from_utf8_lossy(&stopped).escape_debug()
    );
}

struct TmuxCleanup {
    binary: std::ffi::OsString,
    socket: String,
}

impl Drop for TmuxCleanup {
    fn drop(&mut self) {
        let _ = Command::new(&self.binary)
            .args(["-L", &self.socket, "kill-server"])
            .status();
    }
}

fn resize_tmux(tmux: &std::ffi::OsStr, socket: &str, session: &str, columns: usize) {
    let status = Command::new(tmux)
        .args([
            "-L",
            socket,
            "resize-window",
            "-t",
            session,
            "-x",
            &columns.to_string(),
            "-y",
            "24",
        ])
        .status()
        .expect("resize tmux");
    assert!(status.success(), "tmux resize-window failed");
}

fn assert_unique_tmux_rows(tmux: &std::ffi::OsStr, socket: &str, session: &str, timeout: Duration) {
    const LABELS: [&str; 3] = ["TMUX_ROW_ALPHA", "TMUX_ROW_BRAVO", "TMUX_ROW_CHARLIE"];

    let deadline = Instant::now() + timeout;
    loop {
        let output = Command::new(tmux)
            .args(["-L", socket, "capture-pane", "-p", "-t", session])
            .output()
            .expect("capture tmux pane");
        assert!(output.status.success(), "tmux capture-pane failed");
        let screen = String::from_utf8_lossy(&output.stdout);
        let counts = LABELS.map(|label| screen.matches(label).count());
        if counts == [1, 1, 1] {
            // Let several stable-size refreshes run too: this catches terminals
            // that ignore an unsupported cursor save/restore sequence.
            thread::sleep(Duration::from_millis(150));
            let stable = Command::new(tmux)
                .args(["-L", socket, "capture-pane", "-p", "-t", session])
                .output()
                .expect("capture stable tmux pane");
            let stable_screen = String::from_utf8_lossy(&stable.stdout);
            let stable_counts = LABELS.map(|label| stable_screen.matches(label).count());
            if stable_counts == [1, 1, 1] {
                return;
            }
            assert!(
                stable_counts.iter().all(|count| *count <= 1),
                "progress rows accumulated in tmux:\n{stable_screen}"
            );
        }
        assert!(
            counts.iter().all(|count| *count <= 1),
            "progress rows were duplicated in tmux:\n{screen}"
        );
        assert!(
            Instant::now() < deadline,
            "progress rows did not appear in tmux:\n{screen}"
        );
        thread::sleep(Duration::from_millis(25));
    }
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
    synchronized_frame(output, 0)
}

fn synchronized_frame(output: &[u8], index: usize) -> Option<&[u8]> {
    let mut rest = output;
    for frame_index in 0..=index {
        let start = rest
            .windows(BEGIN.len())
            .position(|window| window == BEGIN)?;
        rest = &rest[start + BEGIN.len()..];
        if frame_index < index {
            let end = rest.windows(END.len()).position(|window| window == END)?;
            rest = &rest[end + END.len()..];
        }
    }
    let end = rest.windows(END.len()).position(|window| window == END)?;
    Some(&rest[..end])
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}
