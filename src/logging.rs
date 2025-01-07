use once_cell::sync::Lazy;
use std::sync::Mutex;

static LOGGING_ENABLED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(true));

pub fn enable_logging() {
    *LOGGING_ENABLED.lock().unwrap() = true;
}

pub fn disable_logging() {
    *LOGGING_ENABLED.lock().unwrap() = false;
}

pub fn is_logging_enabled() -> bool {
    *LOGGING_ENABLED.lock().unwrap()
}

#[macro_export]
macro_rules! log_status {
    ($node_id:expr, $($arg:tt)*) => {
        if network_node::logging::is_logging_enabled() {
            println!("[NODE {}] {}", $node_id, format!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! log_error {
    ($node_id:expr, $($arg:tt)*) => {
        if network_node::logging::is_logging_enabled() {
            eprintln!("[NODE {}] Error: {}", $node_id, format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enable_disable() {
        enable_logging();
        assert!(is_logging_enabled());
        disable_logging();
        assert!(!is_logging_enabled());
    }
}
