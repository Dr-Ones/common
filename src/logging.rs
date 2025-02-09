use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use chrono::Local;

static LOGGING_ENABLED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(true));
static LOG_FILE: Lazy<Mutex<Option<File>>> = Lazy::new(|| Mutex::new(None));

pub fn enable_logging() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = true;
    let mut file_guard = LOG_FILE.lock().expect("Failed to get LOG_FILE lock");
    *file_guard = None;
}

pub fn disable_logging() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = false;
}

pub fn redirect_logs_to_file() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = false;
    let mut file_guard = LOG_FILE.lock().expect("Failed to get LOG_FILE lock");
    *file_guard = Some(OpenOptions::new()
        .create(true)
        .append(true)
        .open("network.log")
        .expect("Failed to open log file"));
}

pub fn is_logging_enabled() -> bool {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock")
}

pub fn has_log_file() -> bool {
    LOG_FILE.lock().expect("Failed to get LOG_FILE lock").is_some()
}

pub fn write_to_log(node_id: u8, message: String, is_error: bool) {
    if let Some(file) = LOG_FILE.lock().expect("Failed to get LOG_FILE lock").as_mut() {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let level = if is_error { "ERROR" } else { "INFO" };
        let log_line = format!("[{}] [{:5}] [NODE {}] {}\n", 
            timestamp, level, node_id, message);
        
        if let Err(e) = file.write_all(log_line.as_bytes()) {
            eprintln!("Failed to write to log file: {}", e);
        }
    }
}

#[macro_export]
macro_rules! log_status {
    ($node_id:expr, $($arg:tt)*) => {
        if $crate::logging::is_logging_enabled() {
            println!("[NODE {}] {}", $node_id, format!($($arg)*));
        } else if $crate::logging::has_log_file() {
            $crate::logging::write_to_log($node_id, format!($($arg)*), false);
        }
    };
}

#[macro_export]
macro_rules! log_error {
    ($node_id:expr, $($arg:tt)*) => {
        if $crate::logging::is_logging_enabled() {
            eprintln!("[NODE {}] Error: {}", $node_id, format!($($arg)*));
        } else if $crate::logging::has_log_file() {
            $crate::logging::write_to_log($node_id, format!($($arg)*), true);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_enable_disable() {
        enable_logging();
        assert!(is_logging_enabled());
        disable_logging();
        assert!(!is_logging_enabled());
        assert!(!has_log_file());
    }

    #[test]
    fn test_disable_with_file() {
        let log_path = Path::new("network.log");
        if log_path.exists() {
            fs::remove_file(log_path).expect("Failed to remove log file");
        }

        redirect_logs_to_file();
        assert!(!is_logging_enabled());
        assert!(has_log_file());
        write_to_log(1, "Test message".to_string(), false);
        assert!(log_path.exists());
        fs::remove_file(log_path).expect("Failed to remove log file");
    }
}