use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn emit(level: &str, target: &str, args: fmt::Arguments<'_>) {
    let effective_level = effective_level(level, target);
    if !should_emit(effective_level, target) {
        return;
    }

    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    eprintln!(
        "[{:>10}.{:03}] [{effective_level:<5}] [{thread_name}] [{target}] {args}",
        elapsed.as_secs(),
        elapsed.subsec_millis(),
    );
}

pub fn emit_android_log(prio: i32, target: &str, message: &str) {
    let scoped_target = format!("ffi::android::{target}");
    emit(
        android_log_level(prio),
        &scoped_target,
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

fn effective_level<'a>(level: &'a str, target: &str) -> &'a str {
    if target.starts_with("ffi::") && level == "INFO" {
        "DEBUG"
    } else {
        level
    }
}

fn should_emit(level: &str, target: &str) -> bool {
    match level {
        "DEBUG" => target.starts_with("ffi::"),
        "INFO" | "WARN" | "ERROR" => true,
        _ => false,
    }
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
