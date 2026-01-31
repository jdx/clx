use std::sync::Once;

static INIT: Once = Once::new();

pub(crate) fn init() {
    INIT.call_once(|| {
        // Initialization hook - diagnostics are handled by the diagnostics module
    });
}
