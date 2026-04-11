use std::ffi::{CString, c_char, c_void};
use std::mem::ManuallyDrop;
use std::ptr;

use crate::error::{AppError, AppResult};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SharedPtr {
    pub obj: *mut c_void,
    pub ctrl_blk: *mut c_void,
}

impl SharedPtr {
    pub fn is_null(&self) -> bool {
        self.obj.is_null()
    }
}

unsafe impl Send for SharedPtr {}
unsafe impl Sync for SharedPtr {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct StdVector {
    pub begin: *mut c_void,
    pub end: *mut c_void,
    pub end_capacity: *mut c_void,
}

#[repr(C)]
pub union StdString {
    pub short: ShortString,
    pub long: LongString,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShortString {
    pub mark: u8,
    pub data: [u8; 23],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LongString {
    pub cap: usize,
    pub size: usize,
    pub data: *const c_char,
}

pub struct StdStringRef {
    _storage: ManuallyDrop<CString>,
    raw: StdString,
}

impl StdStringRef {
    pub fn new(value: &str) -> AppResult<Self> {
        let storage = CString::new(value)
            .map_err(|_| AppError::Native("string contains interior NUL byte".into()))?;
        let raw = StdString {
            long: LongString {
                cap: 1,
                size: value.len(),
                data: storage.as_ptr(),
            },
        };
        Ok(Self {
            _storage: ManuallyDrop::new(storage),
            raw,
        })
    }

    pub fn as_ptr(&self) -> *const StdString {
        &self.raw
    }
}

pub fn read_std_string(value: *const StdString) -> String {
    unsafe {
        let mark = ptr::read(value.cast::<u8>());
        if (mark & 1) == 0 {
            let short = &(*value).short;
            let len = usize::from(short.mark >> 1);
            String::from_utf8_lossy(&short.data[..len]).into_owned()
        } else {
            let long = &(*value).long;
            let bytes = std::slice::from_raw_parts(long.data.cast::<u8>(), long.size);
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}
