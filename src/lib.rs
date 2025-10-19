extern crate log;

pub use error::{Error, Result};

mod error;
pub mod osc;
pub mod progress;
pub mod progress_bar;
pub mod style;
pub mod tracing;

// Initialize tracing on module load
static _INIT: std::sync::Once = std::sync::Once::new();

fn init() {
    _INIT.call_once(|| {
        tracing::init();
    });
}
