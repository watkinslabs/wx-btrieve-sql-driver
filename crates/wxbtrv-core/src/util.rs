use crate::constants::ERR_NOT_LOADED;
use crate::state::{set_err, IS_INITIALIZED};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// Return the directory containing wxbtrv.dll, if determinable.
#[cfg(windows)]
pub fn dll_dir() -> Option<PathBuf> {
    unsafe extern "system" {
        fn GetModuleHandleA(lp: *const u8) -> *mut u8;
        fn GetModuleFileNameA(hm: *mut u8, buf: *mut u8, n: u32) -> u32;
    }
    unsafe {
        let name = b"wxbtrv.dll\0";
        let hm = GetModuleHandleA(name.as_ptr());
        if hm.is_null() {
            return None;
        }
        let mut buf = [0u8; 512];
        let len = GetModuleFileNameA(hm, buf.as_mut_ptr(), 512) as usize;
        if len == 0 {
            return None;
        }
        let s = String::from_utf8_lossy(&buf[..len]).into_owned();
        PathBuf::from(s).parent().map(|p| p.to_path_buf())
    }
}
#[cfg(not(windows))]
pub fn dll_dir() -> Option<PathBuf> {
    None
}

pub fn ensure_init() -> Result<(), i32> {
    if IS_INITIALIZED.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(set_err(ERR_NOT_LOADED))
    }
}
