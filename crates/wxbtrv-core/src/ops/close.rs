//! Op 1 — Close.

use super::helpers::{posblk_key, strace};
use crate::constants::{BTR_FILE_NOT_OPEN, BTR_SUCCESS};
use crate::state::state;
use core::ffi::c_void;

/// **Op 1 — Close** (`B_CLOSE`)
///
/// Closes the file bound to the given position block and releases any
/// record/file locks this handle holds. After a successful Close the
/// position block is invalid until a fresh Open.
///
/// **Parameters**:
/// - posblk: identifies the open file (handle id at offset 0).
/// - data buffer / key buffer / key number: ignored.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open (invalid position block).
/// - 41: close disallowed — file modified inside the active txn.
///
/// **Notes**: destroys all currency on this handle. Continuous-op state on
/// the file is not affected. We only drop the in-memory handle; SQL Server
/// handles locking itself.
pub(super) fn op_close(posblk: *mut c_void) -> i32 {
    let key = posblk_key(posblk);
    let (existed, b_path) = if let Ok(mut st) = state().lock() {
        match st.handles.remove(&key) {
            Some(h) => (true, h.b_path),
            None => (false, String::new()),
        }
    } else {
        (false, String::new())
    };
    strace!(
        "op_close key={:#010x} table={} existed={}",
        key,
        b_path,
        existed
    );
    if existed {
        BTR_SUCCESS
    } else {
        BTR_FILE_NOT_OPEN
    }
}
