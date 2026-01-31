# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Build (never use --release)
cargo test               # Run all tests
cargo test test_name     # Run a single test
cargo clippy             # Lint
cargo run --example progress      # Run progress demo
cargo run --example styling       # Run styling demo
cargo run --example osc_progress  # Run OSC progress demo
cargo run --example right_align   # Run right-align demo
```

## Diagnostics

```bash
CLX_TRACE_LOG=frames.jsonl cargo run --example progress  # Capture frame output
CLX_TRACE_LOG=f.jsonl CLX_TRACE_RAW=1 cargo run ...      # Keep ANSI codes
```

## Architecture

clx is a library for building CLI applications with rich terminal output. The core modules are:

- **progress** (`src/progress.rs`) - Hierarchical progress indicators with spinners. Uses Tera templates for rendering job bodies. Key types: `ProgressJobBuilder` (builder pattern), `ProgressJob` (active job handle), `ProgressStatus` (Running/Done/Failed/etc).

- **osc** (`src/osc.rs`) - OSC 9;4 terminal progress bar integration. Detects supported terminals (Ghostty, VS Code, Windows Terminal, VTE-based) via `TERM_PROGRAM`, `WT_SESSION`, `VTE_VERSION` env vars.

- **style** (`src/style.rs`) - Color/formatting utilities. Functions prefixed with `e` (ecyan, ered, ebold) are for stderr; `n` prefix (ncyan, nred) for stdout.

- **progress_bar** (`src/progress_bar.rs`) - Internal progress bar rendering.

### Progress System Design

The progress system uses a background thread that refreshes the display at 200ms intervals. Jobs are stored in a global `JOBS` vec and rendered hierarchically. Template rendering uses Tera with custom functions (`spinner()`, `progress_bar()`) and filters (`flex`, `flex_fill`).

The `flex` filter truncates content to fit terminal width. The `flex_fill` filter pads content with spaces to push subsequent content right. These work via placeholder tags (`<clx:flex>`, `<clx:flex_fill>`) that get processed after template rendering.

### Diagnostics Module

Located in `src/progress.rs` as `mod diagnostics`. Logs each rendered frame as JSONL with `rendered` (display text) and `jobs` (structured state). Controlled by `CLX_TRACE_LOG` env var. Set `CLX_TRACE_RAW=1` to preserve ANSI codes.
