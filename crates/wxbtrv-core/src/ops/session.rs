//! Session ops — 17 SetDir, 18 GetDir, 25 Stop, 26 Version, 27 Unlock, 28 Reset.

use super::helpers::{posblk_key, strace, write_data};
use crate::constants::*;
use crate::state::state;
use core::ffi::c_void;
use core::slice;

/// **Op 26 — Version** (`B_VERSION`)
///
/// Returns the version number(s) of the loaded MicroKernel, Requester,
/// and any intermediate layers. One or more 3-byte Version Blocks:
/// `[major, minor, revision_letter_ASCII]`.
///
/// **Parameters**:
/// - data buffer: receives the version block(s).
/// - data len: ≥ 15 per spec (room for up to five entries).
///
/// **Prerequisites**: none — session-level call, no file need be open.
///
/// **Returns**: 0 on success. Spec statuses: 20 engine not active,
/// 22 short buffer.
///
/// **Notes**: does not affect currency. We advertise Btrieve 6.15 on
/// Win32 (`0x0F 0x06 0x00 0x02 0x00 0x00`).
pub(super) fn op_version(data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    // 6 bytes: [version_lo, version_hi, revision, OS_type, reserved, reserved]
    // Emulate Btrieve 6.15 running on Win32.
    let bytes: [u8; 6] = [0x0F, 0x06, 0x00, 0x02, 0x00, 0x00];
    write_data(data_buf, data_len, &bytes);
    BTR_SUCCESS
}

/// **Op 25 — Stop** (`B_STOP`)
///
/// Stops the workstation MicroKernel: closes all open files for all
/// clients, releases all locks, and (on workstation installations)
/// unloads the engine. On client/server installations it behaves like
/// Reset (28) for the calling client.
///
/// **Parameters**: all ignored except the opcode.
///
/// **Prerequisites**: none.
///
/// **Returns**: 0 on success. Spec statuses: 33 cannot unload (files
/// still open elsewhere).
///
/// **Notes**: destroys all currency on all files. We reset the SQL
/// connection and clear every in-memory handle.
pub(super) fn op_stop() -> i32 {
    // Stop: releases all resources and unloads the MicroKernel.
    crate::sql::reset_connection();
    if let Ok(mut st) = state().lock() {
        st.handles.clear();
    }
    BTR_SUCCESS
}

/// **Op 28 — Reset** (`B_RESET`)
///
/// Releases all resources held by the calling client: aborts any pending
/// transactions, releases all record/file locks, and closes all open
/// files. Unlike Stop (25), does not unload the engine. Spec semantics
/// apply per-client; we scope to the given position block — clear the
/// handle's cached currency without closing it.
///
/// **Parameters**: spec says all ignored; we take posblk for handle scope.
///
/// **Prerequisites**: none.
///
/// **Returns**: 0 on success (always).
///
/// **Notes**: destroys all currency. If the client held a transaction,
/// Reset aborts it (spec) — we have no txn state to abort.
pub(super) fn op_reset(posblk: *mut c_void) -> i32 {
    // Reset: releases all locks and clears currency on this file. Does NOT close.
    let hid = posblk_key(posblk as *const c_void);
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.step_last_recnum = None;
            h.get_last_keys = Vec::new();
            h.get_last_recnum = None;
            h.last_recnum = None;
        }
    }
    BTR_SUCCESS
}

/// **Op 27 — Unlock** (`B_UNLOCK`)
///
/// Releases record-level locks on the file bound to the position block.
/// Key number 0 releases all single- and multiple-record locks; key
/// number 1 releases only the current single-record lock.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - key number: 0 = release all, 1 = release current single.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 77 lock param
/// out of range.
///
/// **Notes**: does not affect currency. SQL Server handles row-level
/// locking natively — we only validate the handle and return success.
pub(super) fn op_unlock(posblk: *mut c_void) -> i32 {
    // SQL Server handles row-level locking natively; no explicit unlock needed.
    let hid = posblk_key(posblk as *const c_void);
    if state()
        .lock()
        .ok()
        .and_then(|s| s.handles.contains_key(&hid).then_some(()))
        .is_none()
    {
        return BTR_FILE_NOT_OPEN;
    }
    BTR_SUCCESS
}

/// **Op 17 — Set Directory** (`B_SET_DIR`)
pub(super) fn op_set_dir(key_buf: *const c_void) -> i32 {
    if key_buf.is_null() {
        strace!("op_set_dir: null key_buf");
        return BTR_SUCCESS;
    }
    let bytes = unsafe { slice::from_raw_parts(key_buf as *const u8, 80) };
    let end = bytes
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(bytes.len());
    let raw = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let norm = raw
        .trim()
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .to_string();
    strace!("op_set_dir: cwd={:?}", norm);
    if let Ok(mut st) = state().lock() {
        st.client_cwd = if norm.is_empty() { None } else { Some(norm) };
    }
    BTR_SUCCESS
}

/// Parse the comma-separated path list that lives in the data buffer on
/// start (key=0) / end-specific (key=2) calls. The spec says the list is
/// terminated by a binary 0; individual entries are separated by commas.
fn parse_continuous_paths(data_buf: *const c_void, dlen: usize) -> Vec<String> {
    if data_buf.is_null() || dlen == 0 {
        return Vec::new();
    }
    let bytes = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };
    // Terminate at the first binary 0.
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    text.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// **Op 42 — Continuous Operation** (`B_CONTINUOUS`)
///
/// Classic Btrieve uses a delta file (`.^^^`) so tape backups can run while
/// clients keep files open. On SQL Server the backend handles point-in-time
/// snapshots directly, so there is no delta file to create or roll in — we
/// parse the requested path list (to validate the call) and return success.
///
/// - `data_buf`: on key_num=0/2, a comma-separated file path list terminated
///   by a binary 0; on key_num=1, unused.
/// - `data_len`: length of that list on input; 0 on key_num=1.
/// - `key_num`: 0 = start continuous op for listed files, 1 = end all,
///   2 = end continuous op for listed files.
///
/// Returns: 0 always on SQL Server backend (no delta-file bookkeeping).
pub(super) fn op_continuous_operation(
    data_buf: *const c_void,
    data_len: *mut u32,
    key_num: i16,
) -> i32 {
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    match key_num {
        0 => {
            let paths = parse_continuous_paths(data_buf, dlen);
            for p in &paths {
                strace!("op_continuous_op: backup registered for {}", p);
            }
            if paths.is_empty() {
                strace!("op_continuous_op: start with empty path list");
            }
        }
        1 => {
            strace!("op_continuous_op: end-all (no per-file tracking on SQL backend)");
        }
        2 => {
            let paths = parse_continuous_paths(data_buf, dlen);
            for p in &paths {
                strace!("op_continuous_op: backup ended for {}", p);
            }
            if paths.is_empty() {
                strace!("op_continuous_op: end-specific with empty path list");
            }
        }
        _ => {
            strace!("op_continuous_op: unknown key_num={}", key_num);
        }
    }
    BTR_SUCCESS
}

/// **Op 18 — Get Directory** (`B_GET_DIR`)
pub(super) fn op_get_dir(
    data_buf: *mut c_void,
    data_len: *mut u32,
    _key_buf: *const c_void,
) -> i32 {
    let cwd = state()
        .lock()
        .ok()
        .and_then(|s| s.client_cwd.clone())
        .unwrap_or_default();
    strace!("op_get_dir: returning cwd={:?}", cwd);
    let mut out = cwd.into_bytes();
    out.push(0);
    write_data(data_buf, data_len, &out);
    BTR_SUCCESS
}
