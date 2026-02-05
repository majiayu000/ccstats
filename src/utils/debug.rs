use std::sync::atomic::{AtomicBool, Ordering};

static PARSE_DEBUG: AtomicBool = AtomicBool::new(false);

pub fn set_parse_debug(enabled: bool) {
    PARSE_DEBUG.store(enabled, Ordering::Relaxed);
}

pub fn parse_debug_enabled() -> bool {
    PARSE_DEBUG.load(Ordering::Relaxed)
}
