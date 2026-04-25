//! Op 2 — Insert. Op 40 — Insert Extended.

use super::helpers::{clone_table_meta, posblk_key, strace};
use crate::constants::*;
use crate::record::unpack_row;
use crate::sql::{execute_sql, fetch_one_row};
use crate::state::state;
use core::ffi::c_void;
use core::ptr;
use core::slice;

/// Insert a single record worth of packed bytes. Factored out of op_insert
/// so op_insert_extended can call it repeatedly.
fn insert_one(hid: u32, meta: &crate::state::TableMeta, record: &[u8]) -> Result<i64, i32> {
    let cols = unpack_row(&meta.fields, record);
    let is_sql_identity_col = meta.recnum_col != "MDS_RECNUM";
    let autoinc_field = meta.fields.iter().enumerate().find(|(_, f)| {
        f.native_type == 14
            || f.native_type == 15
            || (is_sql_identity_col && f.name.eq_ignore_ascii_case(&meta.recnum_col))
    });
    let autoinc_idx = autoinc_field.map(|(i, _)| i);
    let insert_cols: Vec<_> = cols
        .iter()
        .enumerate()
        .filter(|(i, _)| !autoinc_idx.map(|ai| *i == ai).unwrap_or(false))
        .map(|(_, cv)| cv.clone())
        .collect();
    if insert_cols.is_empty() {
        return Err(BTR_DATA_TOO_SHORT);
    }
    let tref = meta.table_ref("", "");
    let col_names: String = insert_cols
        .iter()
        .map(|(n, _)| format!("[{}]", n.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");
    let col_vals: String = insert_cols
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let insert_sql = format!("INSERT INTO {} ({}) VALUES ({})", tref, col_names, col_vals);
    strace!("insert_one h={} sql={}", hid, insert_sql);
    execute_sql(&insert_sql).map_err(|_| BTR_DUPLICATE_KEY)?;
    let new_id: i64 = fetch_one_row("SELECT CAST(SCOPE_IDENTITY() AS BIGINT)", 1)
        .ok()
        .and_then(|r| r.into_iter().next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.last_recnum = Some(new_id);
            h.step_last_recnum = Some(new_id);
        }
    }
    Ok(new_id)
}

/// **Op 2 — Insert** (`B_INSERT`)
///
/// Inserts a new record. The engine (or SQL Server, here) updates every
/// key's B-tree / index. When an AUTOINCREMENT field in the record image is
/// zero, the assigned value is written back into the caller's data buffer.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: record image, ≥ fixed record length.
/// - data len: record image length.
/// - key number (not taken here): key path used to set post-insert
///   currency, or -1 for NCC Insert, or 125 for log key.
///
/// **Prerequisites**: file must be open; record length ≥ fixed-portion.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open (BTR_FILE_NOT_OPEN).
/// - 5: duplicate key on unique index (BTR_DUPLICATE_KEY).
/// - 22: data buffer shorter than fixed-portion (BTR_DATA_TOO_SHORT).
///
/// **Notes**: establishes full logical + physical currency on the new
/// record (non-NCC). The assigned SCOPE_IDENTITY() is remembered as the
/// handle's `last_recnum` for subsequent Update/Delete.
pub(super) fn op_insert(posblk: *mut c_void, data_buf: *const c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let dlen: usize = if data_len.is_null() {
        0
    } else {
        unsafe { (*data_len) as usize }
    };
    if data_buf.is_null() || dlen == 0 {
        return BTR_DATA_TOO_SHORT;
    }
    let record = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };

    let cols = unpack_row(&meta.fields, record);

    // Find the AUTOINCREMENT field.
    let is_sql_identity_col = meta.recnum_col != "MDS_RECNUM";
    let autoinc_field = meta.fields.iter().enumerate().find(|(_, f)| {
        f.native_type == 14
            || f.native_type == 15
            || (is_sql_identity_col && f.name.eq_ignore_ascii_case(&meta.recnum_col))
    });
    let (autoinc_idx, autoinc_offset, autoinc_len) = if let Some((i, f)) = autoinc_field {
        let start = f.offset as usize;
        let end = (f.offset + f.length) as usize;
        let should_skip = is_sql_identity_col && f.name.eq_ignore_ascii_case(&meta.recnum_col)
            || record
                .get(start..end)
                .map(|s| s.iter().all(|&b| b == 0))
                .unwrap_or(true);
        if should_skip {
            (Some(i), f.offset, f.length)
        } else {
            (None, 0, 0)
        }
    } else {
        (None, 0, 0)
    };

    let insert_cols: Vec<_> = cols
        .iter()
        .zip(meta.fields.iter())
        .enumerate()
        .filter(|(i, _)| !autoinc_idx.map(|ai| *i == ai).unwrap_or(false))
        .map(|(_, (cv, _))| cv.clone())
        .collect();

    if insert_cols.is_empty() {
        return BTR_DATA_TOO_SHORT;
    }
    let tref = meta.table_ref("", "");
    let col_names: String = insert_cols
        .iter()
        .map(|(n, _)| format!("[{}]", n.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");
    let col_vals: String = insert_cols
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let insert_sql = format!("INSERT INTO {} ({}) VALUES ({})", tref, col_names, col_vals);

    if autoinc_idx.is_some() {
        strace!("op_insert(autoinc) h={} sql={}", hid, insert_sql);
    } else {
        strace!("op_insert h={} sql={}", hid, insert_sql);
    }
    if execute_sql(&insert_sql).is_err() {
        return BTR_DUPLICATE_KEY;
    }
    let new_id: i64 = fetch_one_row("SELECT CAST(SCOPE_IDENTITY() AS BIGINT)", 1)
        .ok()
        .and_then(|r| r.into_iter().next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    {
        if autoinc_idx.is_some()
            && !data_buf.is_null()
            && dlen >= (autoinc_offset + autoinc_len) as usize
        {
            let id_bytes = new_id.to_le_bytes();
            let n = (autoinc_len as usize).min(8);
            unsafe {
                let dst = (data_buf as *mut u8).add(autoinc_offset as usize);
                ptr::copy_nonoverlapping(id_bytes.as_ptr(), dst, n);
            }
        }
        if let Ok(mut st) = state().lock() {
            if let Some(h) = st.handles.get_mut(&hid) {
                h.last_recnum = Some(new_id);
                h.step_last_recnum = Some(new_id);
            }
        }
        BTR_SUCCESS
    }
}

/// **Op 40 — Insert Extended** (`B_INSERT_EXTENDED`)
///
/// Batch insert: the data buffer begins with a `u16` count, followed by
/// `count` variable-length records each preceded by a `u16` length.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: input batch / output (rewritten with success count word).
/// - `data_len`: total buffer size on input.
///
/// On full success returns 0. On the first duplicate-key failure returns
/// BTR_DUPLICATE_KEY (5) with the output buffer's first word set to the
/// number of records successfully inserted so far.
///
/// Status: 0 success, 3 file not open, 5 partial (dup key), 22 bad layout.
pub(super) fn op_insert_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    if data_buf.is_null() || dlen < 2 {
        return BTR_DATA_TOO_SHORT;
    }
    let buf = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };
    let count = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    strace!("op_insert_extended h={} count={} dlen={}", hid, count, dlen);
    let mut off = 2usize;
    let mut success: u16 = 0;
    let mut last_err: Option<i32> = None;
    for i in 0..count {
        if off + 2 > buf.len() {
            last_err = Some(BTR_DATA_TOO_SHORT);
            break;
        }
        let rec_len = u16::from_le_bytes([buf[off], buf[off + 1]]) as usize;
        off += 2;
        if off + rec_len > buf.len() {
            last_err = Some(BTR_DATA_TOO_SHORT);
            break;
        }
        let record = &buf[off..off + rec_len];
        off += rec_len;
        match insert_one(hid, &meta, record) {
            Ok(_) => {
                success += 1;
            }
            Err(e) => {
                strace!(
                    "op_insert_extended h={} stop at i={} ({} ok) err={}",
                    hid,
                    i,
                    success,
                    e
                );
                last_err = Some(e);
                break;
            }
        }
    }
    // Rewrite the first word with the success count.
    unsafe {
        let p = data_buf as *mut u8;
        ptr::write_unaligned(p as *mut u16, success);
    }
    if let Some(e) = last_err {
        if success == 0 {
            return e;
        }
        return BTR_DUPLICATE_KEY;
    }
    BTR_SUCCESS
}
