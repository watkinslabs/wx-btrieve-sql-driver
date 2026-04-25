//! Shell init, DBU*, and Mds* module-scoped exports. Non-op entry points.
//!
//! These functions are C ABI exports called by the DOS app (and wbexec.exe);
//! pointer validity is the caller's contract. Marking each as
//! `unsafe extern "system" fn` would not change the ABI but would force every
//! Rust-internal helper that re-uses these names to wrap calls in `unsafe { }`,
//! which adds noise without changing what's actually being verified.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::ops_exports::obtrTrace;
use core::ffi::c_void;
use core::ptr;
use core::slice;
use std::sync::atomic::Ordering;
use wxbtrv_core::constants::*;
use wxbtrv_core::ops::do_init;
use wxbtrv_core::sql::{execute_sql, fetch_rows_text};
use wxbtrv_core::state::{set_err, state, INIT_COUNT, IS_INITIALIZED, LAST_ERROR};

// ── Shell init / stop ─────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "system" fn WBRQSHELLINIT(_arg: *mut c_void) -> i32 {
    if !do_init() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    INIT_COUNT.fetch_add(1, Ordering::AcqRel);
    IS_INITIALIZED.store(true, Ordering::Release);
    0
}
#[unsafe(no_mangle)]
pub extern "system" fn WBSHELLINIT(a: *mut c_void) -> i32 {
    WBRQSHELLINIT(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn WBTRVINIT(a: *mut c_void) -> i32 {
    WBRQSHELLINIT(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn WBTRVIDSTOP(_a: *mut c_void) -> i32 {
    let c = INIT_COUNT.load(Ordering::Acquire);
    if c > 0 {
        INIT_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
    if INIT_COUNT.load(Ordering::Acquire) == 0 {
        IS_INITIALIZED.store(false, Ordering::Release);
    }
    0
}
#[unsafe(no_mangle)]
pub extern "system" fn WBTRVSTOP() -> i32 {
    WBTRVIDSTOP(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "system" fn _WBRQSHELLINIT(a: *mut c_void) -> i32 {
    WBRQSHELLINIT(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn _WBSHELLINIT(a: *mut c_void) -> i32 {
    WBSHELLINIT(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn _WBTRVIDSTOP(a: *mut c_void) -> i32 {
    WBTRVIDSTOP(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn _WBTRVINIT(a: *mut c_void) -> i32 {
    WBTRVINIT(a)
}
#[unsafe(no_mangle)]
pub extern "system" fn _WBTRVSTOP() -> i32 {
    WBTRVSTOP()
}

// ── DBU* exports ─────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "system" fn DBUGetInfo(
    info_type: u32,
    buffer: *mut c_void,
    buffer_len: *mut u32,
    _r1: *mut c_void,
    _r2: *mut c_void,
    _r3: u32,
    _r4: u32,
) -> i32 {
    if buffer.is_null() || buffer_len.is_null() {
        return set_err(8020);
    }
    let cap = unsafe { *buffer_len as usize };
    if cap == 0 {
        return 0;
    }
    let text = if info_type == 3 {
        match state().lock() {
            Ok(mut s) => {
                let t = s.row_cache.get(s.row_index).cloned().unwrap_or_default();
                if s.row_index < s.row_cache.len() {
                    s.row_index += 1;
                }
                s.last_row_text = t.clone();
                t
            }
            Err(_) => String::new(),
        }
    } else {
        state()
            .lock()
            .map(|s| s.database.clone())
            .unwrap_or_default()
    };
    let bytes = text.as_bytes();
    let n = bytes.len().min(cap.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buffer as *mut u8, n);
        *(buffer as *mut u8).add(n) = 0;
        *buffer_len = n as u32;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn DBUSetInfo(
    info_type: u32,
    buffer: *mut c_void,
    buffer_len: u32,
    _r1: *mut c_void,
    _r2: u32,
) -> i32 {
    if buffer.is_null() {
        return set_err(8020);
    }
    let bytes = unsafe { slice::from_raw_parts(buffer as *const u8, buffer_len as usize) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
    match info_type {
        1 => {
            if let Ok(mut s) = state().lock() {
                s.database = text;
            }
            0
        }
        2 => {
            if let Ok(mut s) = state().lock() {
                s.last_query = text;
            }
            0
        }
        3 => {
            let q = if text.is_empty() {
                state()
                    .lock()
                    .map(|s| s.last_query.clone())
                    .unwrap_or_default()
            } else {
                text
            };
            match fetch_rows_text(&q, 256) {
                Ok(rows) => {
                    if let Ok(mut s) = state().lock() {
                        s.row_cache = rows;
                        s.row_index = 0;
                        s.last_row_text = s.row_cache.first().cloned().unwrap_or_default();
                    }
                    0
                }
                Err(e) => e,
            }
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn _DBUGetInfo(
    it: u32,
    b: *mut c_void,
    bl: *mut u32,
    r1: *mut c_void,
    r2: *mut c_void,
    r3: u32,
    r4: u32,
) -> i32 {
    DBUGetInfo(it, b, bl, r1, r2, r3, r4)
}
#[unsafe(no_mangle)]
pub extern "system" fn _DBUSetInfo(
    it: u32,
    b: *mut c_void,
    bl: u32,
    r1: *mut c_void,
    r2: u32,
) -> i32 {
    DBUSetInfo(it, b, bl, r1, r2)
}

// ── Mds* exports ─────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "system" fn MdsSetDatabase(db: *const i8) -> i32 {
    if db.is_null() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    let s = unsafe { std::ffi::CStr::from_ptr(db) }
        .to_string_lossy()
        .to_string();
    if let Ok(mut st) = state().lock() {
        st.database = s;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsGetDatabase(out: *mut i8, out_len: *mut u16) {
    if out.is_null() || out_len.is_null() {
        return;
    }
    let db = state()
        .lock()
        .map(|s| s.database.clone())
        .unwrap_or_default();
    let cap = unsafe { *out_len as usize };
    let n = db.len().min(cap.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(db.as_ptr() as *const i8, out, n);
        *out.add(n) = 0;
        *out_len = n as u16;
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsGetError(out: *mut i8, out_len: *mut u16) -> u32 {
    if out.is_null() || out_len.is_null() {
        return u32::MAX;
    }
    let last = LAST_ERROR.load(Ordering::Relaxed);
    let msg = if last == 0 {
        String::new()
    } else {
        format!("MDS error {}", last)
    };
    let cap = unsafe { *out_len as usize };
    let n = msg.len().min(cap.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(msg.as_ptr() as *const i8, out, n);
        *out.add(n) = 0;
        *out_len = n as u16;
    }
    if last == 0 {
        0
    } else {
        u32::MAX
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsAddTable(_spec: *const i8, table_desc: *const i8) -> i32 {
    if table_desc.is_null() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    let t = unsafe { std::ffi::CStr::from_ptr(table_desc) }
        .to_string_lossy()
        .to_string();
    execute_sql(&format!("CREATE TABLE {}", t))
        .map(|_| 0)
        .unwrap_or_else(|e| e)
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsRenameTable(old: *const i8, new: *const i8) -> i32 {
    if old.is_null() || new.is_null() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    let o = unsafe { std::ffi::CStr::from_ptr(old) }
        .to_string_lossy()
        .to_string();
    let n = unsafe { std::ffi::CStr::from_ptr(new) }
        .to_string_lossy()
        .to_string();
    execute_sql(&format!("EXEC sp_rename '{}', '{}';", o, n))
        .map(|_| 0)
        .unwrap_or_else(|e| e)
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsDropTable(table: *const i8) -> i32 {
    if table.is_null() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    let n = unsafe { std::ffi::CStr::from_ptr(table) }
        .to_string_lossy()
        .to_string();
    execute_sql(&format!("DROP TABLE {}", n))
        .map(|_| 0)
        .unwrap_or_else(|e| e)
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsAddFields(table: *const i8, ddl: *const i8, _flags: u32) -> i32 {
    if table.is_null() || ddl.is_null() {
        return set_err(ERR_CONTEXT_SETUP);
    }
    let t = unsafe { std::ffi::CStr::from_ptr(table) }
        .to_string_lossy()
        .to_string();
    let d = unsafe { std::ffi::CStr::from_ptr(ddl) }
        .to_string_lossy()
        .to_string();
    execute_sql(&format!("ALTER TABLE {} ADD {}", t, d))
        .map(|_| 0)
        .unwrap_or_else(|e| e)
}

#[unsafe(no_mangle)]
pub extern "system" fn MdsSetOption(option: i16, val: *mut i16) -> i32 {
    if val.is_null() {
        return set_err(ERR_INVALID_OPTION);
    }
    match option {
        1 => {
            obtrTrace.store(unsafe { *val } as i32, Ordering::Release);
            BTR_SUCCESS
        }
        2 => {
            if let Ok(mut st) = state().lock() {
                st.trim_strings = unsafe { *val } != 0;
            }
            BTR_SUCCESS
        }
        _ => set_err(ERR_INVALID_OPTION),
    }
}
