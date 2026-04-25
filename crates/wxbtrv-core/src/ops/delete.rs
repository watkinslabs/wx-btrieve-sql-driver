//! Op 4 — Delete.

use super::helpers::{get_lock_prefix, posblk_key, strace};
use crate::constants::*;
use crate::sql::{execute_sql, lock_row, unlock_row};
use crate::state::state;
use core::ffi::c_void;

/// **Op 4 — Delete** (`B_DELETE`)
///
/// Removes the current record. Requires physical currency established by
/// a prior Get / Step / Get Direct / non-NCC Insert on this handle. The
/// engine removes the row and all its key entries; variable pages freed.
///
/// **Parameters**:
/// - posblk: identifies the open file; `last_recnum` is the target row.
/// - data buffer / data len / key buffer / key number: ignored.
///
/// **Prerequisites**: file open; physical currency present. Inside a txn,
/// the record must have been read within that same txn.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open.
/// - 8: no current record (BTR_INVALID_POS).
/// - 80: record-level conflict.
/// - 83: record was read outside the current transaction.
///
/// **Notes**: destroys physical currency. Logical currency survives in the
/// sense that the next Get Next/Prev returns the neighbors of the deleted
/// row. We lock the row via SQL before deleting.
pub(super) fn op_delete(posblk: *mut c_void) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, last_rn) = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        (h.meta.clone(), h.last_recnum)
    };
    let Some(recnum) = last_rn else {
        return BTR_INVALID_POS;
    };
    let tref = meta.table_ref("", "");
    let sql = format!(
        "DELETE FROM {} WHERE {} = {}",
        tref,
        meta.recnum_sql_ref(),
        recnum
    );
    strace!("op_delete h={} rn={} sql={}", hid, recnum, sql);
    let prefix = match get_lock_prefix(hid) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = lock_row(&prefix, recnum) {
        return e;
    }
    let rc = match execute_sql(&sql) {
        Ok(_) => BTR_SUCCESS,
        Err(e) => e,
    };
    let _ = unlock_row(&prefix, recnum);
    rc
}
