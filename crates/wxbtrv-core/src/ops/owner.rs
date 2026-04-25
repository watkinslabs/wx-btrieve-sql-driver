//! Owner ops — 29 SetOwner, 30 ClearOwner.
//!
//! SQL Server has no concept of file-level owner-password or page-level
//! encryption, so these ops are cosmetic: we stash the owner name on the
//! HandleEntry so it shows up in traces and return BTR_SUCCESS.

use super::helpers::{posblk_key, strace};
use crate::constants::*;
use crate::state::state;
use core::ffi::c_void;
use core::slice;

/// **Op 29 — Set Owner** (`B_SET_OWNER`)
///
/// Assigns an owner name to an open file. In real Btrieve this also controls
/// optional data-page encryption; we don't emulate that. The owner name is
/// stored on the handle entry for inspection only.
///
/// - `posblk`: open position block — handle id read from offset 0.
/// - `data_buf`: owner name, NUL-terminated. Up to 8 bytes (modes 0-3) or
///   24 bytes (mode 4).
/// - `data_len`: length of the owner name in the data buffer (incl. NUL).
/// - `key_num`: owner mode (0..4).
///
/// Status: 0 on success, 3 if the handle is not open.
pub(super) fn op_set_owner(
    posblk: *mut c_void,
    data_buf: *const c_void,
    data_len: *mut u32,
    key_num: i16,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    let name = if data_buf.is_null() || dlen == 0 {
        String::new()
    } else {
        let bytes = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen.min(64)) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };
    strace!(
        "op_set_owner h={} mode={} name={:?} (SQL Server — noop, stored on handle)",
        hid,
        key_num,
        name
    );
    let Ok(mut st) = state().lock() else {
        return BTR_FILE_NOT_OPEN;
    };
    let Some(h) = st.handles.get_mut(&hid) else {
        return BTR_FILE_NOT_OPEN;
    };
    h.owner_name = if name.is_empty() { None } else { Some(name) };
    BTR_SUCCESS
}

/// **Op 30 — Clear Owner** (`B_CLEAR_OWNER`)
///
/// Removes a previously-assigned owner name from an open file. For SQL-backed
/// tables this just clears the stashed owner name on the HandleEntry.
///
/// - `posblk`: open position block — handle id read from offset 0.
///
/// Status: 0 on success, 3 if the handle is not open.
pub(super) fn op_clear_owner(posblk: *mut c_void) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let Ok(mut st) = state().lock() else {
        return BTR_FILE_NOT_OPEN;
    };
    let Some(h) = st.handles.get_mut(&hid) else {
        return BTR_FILE_NOT_OPEN;
    };
    let was = h.owner_name.take();
    strace!("op_clear_owner h={} cleared={:?}", hid, was);
    BTR_SUCCESS
}
