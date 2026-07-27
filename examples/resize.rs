//! Long-running terminal resize demo.
//!
//! Run with: cargo run --example resize

use std::{thread, time::Duration};

use clx::progress::{ProgressJobBuilder, ProgressJobDoneBehavior, ProgressStatus, set_interval};

const MESSAGES: [&str; 6] = [
    "fetching package metadata from the configured registry",
    "resolving a deliberately long dependency description",
    "downloading an archive with enough text to wrap when narrow",
    "verifying checksums and artifact provenance",
    "linking executables into the destination directory",
    "updating the final lockfile and installation manifest",
];

fn main() {
    eprintln!("Resize this terminal freely; the demo runs for about 20 seconds.");

    set_interval(Duration::from_millis(50));
    let root = ProgressJobBuilder::new()
        .body("{{ spinner() }} resize demo  {{ progress_bar(flex=true) }}  {{ cur }}/{{ total }}")
        .body_text(Some("resize demo {{ cur }}/{{ total }}"))
        .progress_total(400)
        .progress_current(0)
        .on_done(ProgressJobDoneBehavior::Collapse)
        .start();

    let children = MESSAGES
        .iter()
        .map(|message| {
            root.add(
                ProgressJobBuilder::new()
                    .prop("message", message)
                    .on_done(ProgressJobDoneBehavior::Collapse)
                    .build(),
            )
        })
        .collect::<Vec<_>>();

    for tick in 0..=400 {
        root.progress_current(tick);
        let child = tick % children.len();
        children[child].prop("message", &format!("{} · item {tick:03}", MESSAGES[child]));
        thread::sleep(Duration::from_millis(50));
    }

    for child in children {
        child.set_status(ProgressStatus::Done);
    }
    root.set_status(ProgressStatus::Done);
    clx::progress::stop();
}
