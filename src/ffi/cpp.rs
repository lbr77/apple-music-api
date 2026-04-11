use std::ffi::c_void;

#[link(name = "wrapper_cpp_callbacks", kind = "static")]
unsafe extern "C" {
    fn wrapper_make_end_lease_callback(cb: unsafe extern "C" fn(i32)) -> *mut c_void;
    fn wrapper_make_playback_error_callback(cb: unsafe extern "C" fn(*mut c_void)) -> *mut c_void;
    fn wrapper_free_end_lease_callback(ptr: *mut c_void);
    fn wrapper_free_playback_error_callback(ptr: *mut c_void);
}

pub struct EndLeaseCallback {
    ptr: *mut c_void,
}

impl EndLeaseCallback {
    pub fn new(callback: unsafe extern "C" fn(i32)) -> Self {
        let ptr = unsafe { wrapper_make_end_lease_callback(callback) };
        Self { ptr }
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for EndLeaseCallback {
    fn drop(&mut self) {
        unsafe { wrapper_free_end_lease_callback(self.ptr) };
    }
}

pub struct PlaybackErrorCallback {
    ptr: *mut c_void,
}

impl PlaybackErrorCallback {
    pub fn new(callback: unsafe extern "C" fn(*mut c_void)) -> Self {
        let ptr = unsafe { wrapper_make_playback_error_callback(callback) };
        Self { ptr }
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for PlaybackErrorCallback {
    fn drop(&mut self) {
        unsafe { wrapper_free_playback_error_callback(self.ptr) };
    }
}
