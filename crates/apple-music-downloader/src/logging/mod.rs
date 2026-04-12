use std::env;
use std::fmt;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

static DEBUG_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    env::var("WRAPPER_LOG")
        .ok()
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| value.contains("debug"))
});

pub fn emit(level: &str, target: &str, args: fmt::Arguments<'_>) {
    if !should_emit(level) {
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

/// Keep Android shim logs on the same sink so native and Rust events stay ordered.
pub fn emit_android_log(prio: i32, target: &str, message: &str) {
    emit(
        android_log_level(prio),
        target,
        format_args!("prio={prio} {message}"),
    );
}

fn android_log_level(prio: i32) -> &'static str {
    match prio {
        ..=3 => "DEBUG",
        4 => "INFO",
        5 => "WARN",
        6.. => "ERROR",
    }
}

fn should_emit(level: &str) -> bool {
    matches!(level, "INFO" | "WARN" | "ERROR") || (*DEBUG_ENABLED && level == "DEBUG")
}

#[macro_export]
macro_rules! app_debug {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::emit("DEBUG", $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! app_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::emit("INFO", $target, format_args!($($arg)*))
    }
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
