use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

#[link(name = "wrapper_android_log_shim", kind = "static")]
unsafe extern "C" {
    fn wrapper_android_log_shim_anchor();
}

pub fn install_android_log_shim() {
    unsafe { wrapper_android_log_shim_anchor() };
}

/// The C shim resolves variadic Android log calls before crossing the FFI boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wrapper_rust_emit_android_log(
    prio: c_int,
    tag: *const c_char,
    message: *const c_char,
) {
    assert!(!tag.is_null(), "android log shim passed a null tag");
    assert!(!message.is_null(), "android log shim passed a null message");
    let target = unsafe { CStr::from_ptr(tag) }.to_string_lossy();
    let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    crate::logging::emit_android_log(prio, target.as_ref(), message.as_ref());
}
