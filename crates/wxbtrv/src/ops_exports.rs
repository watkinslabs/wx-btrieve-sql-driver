//! C ABI exports for the Btrieve entry points and legacy data symbols.
//!
//! All real dispatch work lives in `wxbtrv_core::ops::btrcall_internal`; the
//! wrappers here exist only so the cdylib exposes the Windows-linkable C ABI.
//!
//! These exports are called from C across an FFI boundary; pointer validity
//! is the caller's contract.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use std::sync::atomic::AtomicI32;

use wxbtrv_core::ops::btrcall_internal;

// ── Data exports ─────────────────────────────────────────────────────────────
// These are legacy DLL data symbols that the original BTRVDD.DLL exported.
// `MdsSetOption` in `exports_misc.rs` reads/writes `obtrTrace`; the others are
// present purely for export-table compatibility.
#[unsafe(no_mangle)]
pub static mut obtrDriver: i32 = 1;
#[unsafe(no_mangle)]
pub static obtrTrace: AtomicI32 = AtomicI32::new(0);
#[unsafe(no_mangle)]
pub static mut obtrAvailableSQLServers: i32 = 0;
#[unsafe(no_mangle)]
pub static mut obtrAvailableSQLServerName: [u8; 256] = [0; 256];

// ── BTRCALL / BTRCALLID ──────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "system" fn BTRCALL(
    operation: u16,
    position_block: *mut c_void,
    data_buffer: *mut c_void,
    data_len: *mut u32,
    key_buffer: *mut c_void,
    key_num: i16,
    acs: *mut i8,
) -> i32 {
    unsafe {
        btrcall_internal(
            operation,
            position_block,
            data_buffer,
            data_len,
            key_buffer,
            key_num,
            acs,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn BTRCALLID(
    operation: u16,
    pb: *mut c_void,
    db: *mut c_void,
    dl: *mut u32,
    kb: *mut c_void,
    kn: i16,
    acs: *mut i8,
    _cid: *mut c_void,
) -> i32 {
    BTRCALL(operation, pb, db, dl, kb, kn, acs)
}

#[unsafe(no_mangle)]
pub extern "system" fn _BTRCALL(
    op: u16,
    pb: *mut c_void,
    db: *mut c_void,
    dl: *mut u32,
    kb: *mut c_void,
    kn: i16,
    acs: *mut i8,
) -> i32 {
    BTRCALL(op, pb, db, dl, kb, kn, acs)
}

#[unsafe(no_mangle)]
pub extern "system" fn _BTRCALLID(
    op: u16,
    pb: *mut c_void,
    db: *mut c_void,
    dl: *mut u32,
    kb: *mut c_void,
    kn: i16,
    acs: *mut i8,
    cid: *mut c_void,
) -> i32 {
    BTRCALLID(op, pb, db, dl, kb, kn, acs, cid)
}
