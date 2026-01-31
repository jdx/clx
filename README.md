# clx

Components for building CLI applications in Rust with rich terminal output.

## Features

- **Progress Jobs** - Hierarchical progress indicators with spinners, status tracking, and nested child jobs
- **OSC Integration** - Terminal progress bar integration for supported terminals (Ghostty, VS Code, Windows Terminal, VTE-based)
- **Styling** - Color and formatting utilities for stderr and stdout output
- **Diagnostics** - Frame logging for debugging and LLM-friendly verification

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
clx = "1"
```

## Usage

### Progress Jobs

Create progress indicators using the builder pattern:

```rust
use clx::progress::{ProgressJobBuilder, ProgressStatus, ProgressJobDoneBehavior};

// Create and start a progress job
let job = ProgressJobBuilder::new()
    .prop("message", "Processing files...")
    .on_done(ProgressJobDoneBehavior::Collapse)
    .start();

// Add child jobs for nested progress
let child = job.add(
    ProgressJobBuilder::new()
        .prop("message", "Subtask 1")
        .build()
);

// Update status when complete
child.set_status(ProgressStatus::Done);
job.set_status(ProgressStatus::Done);
```

#### Progress with Percentage

Track progress with current/total values:

```rust
let job = ProgressJobBuilder::new()
    .body("{{ spinner() }} {{ message }} {{ progress_bar(flex=true) }}")
    .prop("message", "Downloading")
    .progress_total(100)
    .progress_current(0)
    .start();

// Update progress
for i in 0..=100 {
    job.progress_current(i);
}
job.set_status(ProgressStatus::Done);
```

#### Custom Templates

Progress jobs use [Tera](https://tera.netlify.app/) templates:

```rust
let job = ProgressJobBuilder::new()
    .body("{{ spinner(name='dots') }} [{{ cur }}/{{ total }}] {{ message }}")
    .prop("message", "Building")
    .prop("cur", &0)
    .prop("total", &10)
    .start();
```

Available template functions:
- `spinner(name='...')` - Animated spinner. Available spinners:
  - Classic: `line`, `dot`, `mini_dot`, `jump`, `pulse`, `points`, `hamburger`, `ellipsis`
  - Minimal: `arrow`, `triangle`, `square`, `circle`, `bounce`, `arc`, `box_bounce`
  - Aesthetic: `star`, `hearts`, `clock`, `weather`
  - Growing: `grow_horizontal`, `grow_vertical`, `meter`
  - Emoji: `globe`, `moon`, `monkey`, `runner`, `oranges`, `smiley`
- `progress_bar(flex=true)` - Progress bar that fills available width
- `progress_bar(width=N)` - Fixed-width progress bar
- `elapsed()` - Time since job started (e.g., "1m23s")
- `eta()` - Estimated time remaining based on progress
- `rate()` - Throughput rate (e.g., "42.5/s")

Available template filters:
- `{{ content | flex }}` - Truncates content to fit available width
- `{{ content | flex_fill }}` - Pads content with spaces to fill available width (for right-aligning subsequent content)
- Color filters: `{{ text | cyan }}`, `{{ text | blue }}`, `{{ text | green }}`, `{{ text | yellow }}`, `{{ text | red }}`, `{{ text | magenta }}`
- Style filters: `{{ text | bold }}`, `{{ text | dim }}`, `{{ text | underline }}`

Tera's built-in `{% if %}` conditionals are also available for conditional rendering.

#### Right-Aligned Progress Bars

Use `flex_fill` to push content to the right edge:

```rust
let job = ProgressJobBuilder::new()
    .body("{{ spinner() }} {{ message | flex_fill }}{{ progress_bar(flex=true) }}")
    .prop("message", "Downloading")
    .progress_total(100)
    .start();
```

This produces output like:
```
⠋ Downloading                              [========>           ]
```

#### Status Types

```rust
use clx::progress::ProgressStatus;

job.set_status(ProgressStatus::Running);     // Spinner animation
job.set_status(ProgressStatus::Pending);     // Paused indicator
job.set_status(ProgressStatus::Done);        // Success checkmark
job.set_status(ProgressStatus::Failed);      // Error indicator
job.set_status(ProgressStatus::Warn);        // Warning indicator
job.set_status(ProgressStatus::Hide);        // Hidden from display
```

### OSC Terminal Progress

Automatically shows progress in terminal title bars for supported terminals:

```rust
use clx::osc;

// Disable OSC progress (must be called before any progress jobs start)
osc::configure(false);
```

### Terminal Lock

Synchronize output with progress display:

```rust
use clx::progress::with_terminal_lock;

// Write to stderr without interfering with progress display
with_terminal_lock(|| {
    eprintln!("Log message");
});
```

### Text Mode

For non-interactive environments:

```rust
use clx::progress::{set_output, ProgressOutput};

set_output(ProgressOutput::Text);  // Simple text output
set_output(ProgressOutput::UI);    // Rich terminal UI (default)
```

### Diagnostics

Enable frame logging to capture what users see:

```bash
CLX_TRACE_LOG=frames.jsonl cargo run --example progress
```

To preserve ANSI escape codes in the output (useful for debugging color/styling issues):

```bash
CLX_TRACE_LOG=frames.jsonl CLX_TRACE_RAW=1 cargo run --example progress
```

Each line in the log file is a JSON object with:
- `rendered` - The exact text displayed (ANSI codes stripped by default, or raw if `CLX_TRACE_RAW` is set)
- `jobs` - Structured array of job states (id, status, message, progress, children)

Example output:
```json
{"rendered":"✔ Task 1\n⠋ Task 2 [5/10]","jobs":[{"id":0,"status":"done","message":"Task 1","progress":null,"children":[]},{"id":1,"status":"running","message":"Task 2","progress":[5,10],"children":[]}]}
```

This is useful for:
- Debugging progress display issues
- Automated testing of CLI output
- LLM-based verification of user-visible behavior

#### Using LLMs to Debug Progress Display

The JSONL diagnostic format is designed to be easily parsed by LLMs. When you encounter issues with your progress display, you can capture a diagnostic log and share it with an LLM for analysis.

**Capture diagnostic output:**

```bash
CLX_TRACE_LOG=debug.jsonl cargo run --bin myapp
```

**Share with an LLM:**

Provide the contents of `debug.jsonl` along with your code and a description of the issue. The LLM can analyze:

- **Rendered output** - What users actually see on screen (with ANSI codes stripped for readability)
- **Job hierarchy** - Parent/child relationships between progress jobs
- **Status transitions** - How job statuses change over time (pending → running → done)
- **Progress values** - Current/total progress values and whether they update correctly
- **Timing issues** - Whether jobs appear/disappear in the expected order

**Example prompt:**

```
I'm using clx for progress display but the nested jobs aren't appearing correctly.
Here's my code:
[paste your code]

Here's the diagnostic output:
[paste contents of debug.jsonl]

Can you identify why the child jobs aren't visible?
```

**What LLMs can help diagnose:**

- Jobs that never transition from `pending` to `running`
- Child jobs not properly nested under parents
- Progress bars not updating (progress values stay static)
- Jobs completing in wrong order
- Template rendering issues (missing variables, malformed output)
- Flex/truncation problems (content not fitting terminal width)

The structured `jobs` array provides machine-readable state that complements the human-readable `rendered` field, making it straightforward for LLMs to correlate what the code intended with what users actually see.

## API Overview

### `clx::progress`

| Type | Description |
|------|-------------|
| `ProgressJobBuilder` | Builder for creating progress jobs |
| `ProgressJob` | Active progress job handle |
| `ProgressStatus` | Job status enum (Running, Done, Failed, etc.) |
| `ProgressJobDoneBehavior` | What to do when job completes (Keep, Collapse, Hide) |
| `ProgressOutput` | Output mode (UI, Text) |

| Function | Description |
|----------|-------------|
| `with_terminal_lock(f)` | Execute function with terminal lock held |
| `set_output(mode)` | Set output mode |
| `output()` | Get current output mode |
| `set_interval(duration)` | Set refresh interval |
| `interval()` | Get current refresh interval |
| `flush()` | Force refresh |
| `stop()` | Stop progress display |
| `stop_clear()` | Stop and clear progress display |

### `clx::osc`

| Type | Description |
|------|-------------|
| `ProgressState` | OSC progress state (None, Normal, Error, Indeterminate, Warning) |

| Function | Description |
|----------|-------------|
| `configure(enabled)` | Enable/disable OSC progress |

## Examples

Run the included examples:

```bash
cargo run --example progress      # Basic progress demo
cargo run --example styling       # Styling demo
cargo run --example osc_progress  # OSC progress demo
cargo run --example right_align   # Right-aligned progress bars
```

## License

MIT
