use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn emit(level: &str, target: &str, args: fmt::Arguments<'_>) {
    if !matches!(level, "INFO" | "WARN" | "ERROR") {
        return;
    }

    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    eprintln!(
        "[{:>10}.{:03}] [{level:<5}] [{thread_name}] [{target}] {args}",
        elapsed.as_secs(),
        elapsed.subsec_millis(),
    );
}

#[macro_export]
macro_rules! app_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::emit("INFO", $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! app_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::emit("WARN", $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! app_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::emit("ERROR", $target, format_args!($($arg)*))
    };
}
