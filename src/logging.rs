use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use chrono::Local;

static LOGGING_ENABLED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(true));
static LOG_FILE: Lazy<Mutex<Option<File>>> = Lazy::new(|| Mutex::new(None));

/// Enables logging to stdout and resets any log file redirection.
///
/// This function sets the logging status to enabled (stdout)
/// and clears any previously configured log file.
pub fn enable_logging() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = true;
    let mut file_guard = LOG_FILE.lock().expect("Failed to get LOG_FILE lock");
    *file_guard = None;
}

/// Disables logging to stdout.
///
/// This function sets the logging status to disabled.
/// It does not affect an already configured log file.
pub fn disable_logging() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = false;
}

/// Redirects log output to a file.
///
/// This function disables stdout logging and configures logging to a file
/// named "network.log". Log messages will be appended to this file.
pub fn redirect_logs_to_file() {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock") = false;
    let mut file_guard = LOG_FILE.lock().expect("Failed to get LOG_FILE lock");
    *file_guard = Some(OpenOptions::new()
        .create(true)
        .append(true)
        .open("network.log")
        .expect("Failed to open log file"));
}

/// Returns whether logging to stdout is enabled.
///
/// # Returns
///
/// `true` if logging to stdout is enabled, otherwise `false`.
pub fn is_logging_enabled() -> bool {
    *LOGGING_ENABLED.lock().expect("Failed to get LOGGING_ENABLED lock")
}

/// Checks if a log file is currently configured for logging.
///
/// # Returns
///
/// `true` if a log file is set for logging, otherwise `false`.
pub fn has_log_file() -> bool {
    LOG_FILE.lock().expect("Failed to get LOG_FILE lock").is_some()
}

/// Writes a log message to the log file if available.
///
/// The log message includes a timestamp, log level, node identifier,
/// and the provided message. If writing fails, an error is printed to stderr.
///
/// # Arguments
///
/// * `node_id` - Identifier for the node that is logging the message.
/// * `message` - The log message to be written.
/// * `is_error` - A flag indicating whether the message represents an error.
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
/// Logs a status message.
///
/// If logging to stdout is enabled, the message is printed to stdout.
/// Otherwise, if a log file is configured, the message is written to the file.
///
/// # Examples
///
/// ```
/// log_status!(1, "Node is online");
/// ```
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
/// Logs an error message.
///
/// If logging to stdout is enabled, the message is printed to stderr.
/// Otherwise, if a log file is configured, the message is written to the file as an error.
///
/// # Examples
///
/// ```
/// log_error!(1, "Failed to connect to the server");
/// ```
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
