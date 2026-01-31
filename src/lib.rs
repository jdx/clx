//! # clx
//!
//! A library for building CLI applications with rich terminal output.
//!
//! clx provides hierarchical progress indicators with spinners, OSC terminal integration,
//! and styling utilities for creating polished command-line interfaces.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use clx::progress::{ProgressJobBuilder, ProgressStatus};
//!
//! // Create and start a progress job
//! let job = ProgressJobBuilder::new()
//!     .prop("message", "Processing files...")
//!     .start();
//!
//! // Do work...
//!
//! // Mark as complete
//! job.set_status(ProgressStatus::Done);
//! ```
//!
//! ## Modules
//!
//! - [`progress`] - Hierarchical progress indicators with spinners and templates
//! - [`osc`] - OSC 9;4 terminal progress bar integration
//! - [`style`] - Color and formatting utilities for terminal output
//!
//! ## Features
//!
//! - **Progress Jobs** - Create hierarchical progress indicators with animated spinners,
//!   status tracking, and nested child jobs
//! - **Template Rendering** - Use Tera templates for customizable progress display
//! - **OSC Integration** - Automatic progress bar in terminal title bars for supported
//!   terminals (Ghostty, VS Code, Windows Terminal, VTE-based)
//! - **Styling** - Color and formatting utilities with automatic terminal detection
//! - **Thread Safety** - All progress operations are thread-safe with interior mutability

pub use error::{Error, Result};

mod error;
pub mod osc;
pub mod progress;
mod progress_bar;
pub mod style;
