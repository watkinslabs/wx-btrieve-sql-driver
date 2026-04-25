//! Position / direct access ops — 22 GetPosition, 23 GetDirect, 44 GetPercent, 45 FindPercent.

use super::helpers::{clone_table_meta, posblk_key, strace, write_data};
use super::sql_helpers::{extract_key_vals, fetch_keyset_one};
use crate::constants::*;
use crate::sql::fetch_rows_positional;
use crate::state::state;
use core::ffi::c_void;

/// **Op 22 — Get Position** (`B_GET_POSITION`)
///
/// Returns a 4-byte opaque physical address of the current record. The
/// caller can save it and later pass it to Get Direct/Record (23) to
/// re-retrieve the row without an index lookup.
///
/// **Parameters**:
/// - posblk: open file handle with a physical current record.
/// - data buffer: receives the 4-byte address.
/// - data len: ≥ 4.
///
/// **Prerequisites**: file open; a physical current record must exist.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 8 no current,
/// 22 short buffer.
///
/// **Notes**: does not affect currency. We return `last_recnum` as a
/// 4-byte little-endian u32 token — callers must treat it as opaque.
pub(super) fn op_get_position(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    // Return MDS_RECNUM as a 4-byte LE u32 position token — Btrieve GetPos always returns 4 bytes.
    let hid = posblk_key(posblk as *const c_void);
    let rn = state()
        .lock()
        .ok()
        .and_then(|st| st.handles.get(&hid).and_then(|h| h.last_recnum))
        .unwrap_or(0);
    let token = (rn as u32).to_le_bytes();
    write_data(data_buf, data_len, &token);
    BTR_SUCCESS
}

/// **Op 23 — Get Direct/Record** (`B_GET_DIRECT`)
///
/// Retrieves the record at a physical address previously obtained from
/// Get Position (22) or from an extended-op `recPos` field. Bypasses the
/// index; does not validate key consistency. Also has a **Chunk** variant
/// (same opcode, `get_direct_chunk.md`) that fetches byte ranges rather
/// than the whole record, selected by a signature `0x80000000` header in
/// the data buffer; we do not implement the chunk variant.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: first 4 bytes = physical address on entry; receives
///   the record image on exit.
/// - data len: capacity in / actual record length out.
/// - key buffer: output-only key value for the chosen key path.
/// - key number: key path (0..118) to re-establish logical currency,
///   -1 for NCC (physical only), or 125 for log key.
///
/// **Prerequisites**: file open; valid 4-byte address in the buffer.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 4 deleted,
/// 6 bad key number, 9 not found, 22 short buffer, 43 invalid address.
///
/// **Notes**: key number ≥ 0 sets full currency; -1 sets only physical.
pub(super) fn op_get_direct(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    _key_buf: *mut c_void,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    if data_buf.is_null() || data_len.is_null() {
        return BTR_DATA_TOO_SHORT;
    }
    let dlen = unsafe { *data_len } as usize;
    let pos_bytes = unsafe { std::slice::from_raw_parts(data_buf as *const u8, 4.min(dlen)) };
    if pos_bytes.len() < 4 {
        return BTR_DATA_TOO_SHORT;
    }
    let recnum =
        u32::from_le_bytes([pos_bytes[0], pos_bytes[1], pos_bytes[2], pos_bytes[3]]) as i64;
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let rc = &meta.recnum_col;
    let sql = format!("SELECT TOP 1 {cols} FROM {tref} WHERE [{rc}] = {recnum}");
    strace!("op_get_direct h={} rn={} sql={}", hid, recnum, sql);
    match fetch_keyset_one(&meta, &sql) {
        Ok((rn, packed, fields)) => {
            let idx_num = state()
                .lock()
                .ok()
                .and_then(|st| st.handles.get(&hid).and_then(|h| h.get_index_num))
                .unwrap_or_else(|| meta.indexes.first().map(|ix| ix.num).unwrap_or(0));
            let last_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_last_keys = last_keys;
                    h.get_last_recnum = Some(rn);
                    h.last_recnum = Some(rn);
                }
            }
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => 9,
        Err(e) => e,
    }
}

/// **Op 44 — Get By Percentage** (`B_GET_BY_PERCENTAGE`)
///
/// Retrieves the record at approximately a specified percentile through
/// the file, either by physical order or by a key path. Percentages
/// are 0..10000 representing 0.00..100.00%. Used for scrollbar/random
/// sampling in O(log n) time. (Our implementation returns the *current*
/// record's percentile given last_recnum.)
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: 4-byte LE percentage; also receives the result.
/// - data len: capacity in / bytes written out.
/// - key number: key path 0..118, or -1 for physical percentile.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 6 bad key,
/// 9 EOF, 22 short buffer.
///
/// **Notes**: we compute `(rows with recnum ≤ last_recnum) / COUNT(*)`
/// and return it as a LE u32 scaled to 10000.
pub(super) fn op_get_percent(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
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
    let tref = meta.table_ref("", "");
    let rc = meta.recnum_sql_ref();
    let (before, total) = if let Some(rn) = last_rn {
        let sql_b = format!("SELECT COUNT(*) FROM {tref} WHERE {rc} <= {rn}");
        let sql_t = format!("SELECT COUNT(*) FROM {tref}");
        let b: i64 = fetch_rows_positional(&sql_b, 1, 1)
            .ok()
            .and_then(|r| r.into_iter().next())
            .and_then(|r| r.into_iter().next())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let t: i64 = fetch_rows_positional(&sql_t, 1, 1)
            .ok()
            .and_then(|r| r.into_iter().next())
            .and_then(|r| r.into_iter().next())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1)
            .max(1);
        (b, t)
    } else {
        (0, 1)
    };
    let pct = ((before * 10000) / total) as u32;
    strace!(
        "op_get_percent h={} before={} total={} pct={}",
        hid,
        before,
        total,
        pct
    );
    let bytes = pct.to_le_bytes();
    write_data(data_buf, data_len, &bytes);
    0
}

/// **Op 45 — Find Percentage** (`B_FIND_PERCENTAGE`)
///
/// Inverse of Get By Percentage: given either a 4-byte physical address
/// or a key value, returns the percentile position of that record in the
/// file. Percentile is a 4-byte LE integer 0..10000.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: input physical address (physical form) or unused;
///   receives the 4-byte percentile on exit.
/// - data len: ≥ 4.
/// - key buffer: input key value when addressing by key.
/// - key number: key path (≥ 0) or -1 for physical.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 4 key not found,
/// 6 bad key number, 9 EOF (empty file), 22 short buffer, 43 bad address.
///
/// **Notes**: does not affect currency. Our impl treats the 4-byte input
/// as a percentage and fetches the row at that offset (used as a
/// percentile-to-recnum helper by the app).
pub(super) fn op_find_percent(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };

    let pct: u32 = if !data_buf.is_null() && !data_len.is_null() {
        let len = unsafe { *data_len } as usize;
        if len >= 4 {
            let b = unsafe { core::slice::from_raw_parts(data_buf as *const u8, 4) };
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            0
        }
    } else {
        0
    };

    let tref = meta.table_ref("", "");
    let rc = meta.recnum_sql_ref();

    let count_sql = format!("SELECT COUNT(*) FROM {tref}");
    let total: i64 = fetch_rows_positional(&count_sql, 1, 1)
        .ok()
        .and_then(|r| r.into_iter().next())
        .and_then(|r| r.into_iter().next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    if total == 0 {
        return BTR_EOF;
    }

    let offset = ((pct as i64 * total) / 10000).min(total - 1).max(0);
    let cols = meta.select_with_recnum();
    let sql = if offset == 0 {
        format!("SELECT TOP 1 {cols} FROM {tref} ORDER BY {rc} ASC")
    } else {
        format!(
            "SELECT TOP 1 {cols} FROM \
             (SELECT {cols}, ROW_NUMBER() OVER (ORDER BY {rc} ASC) AS _rn FROM {tref}) _t \
             WHERE _rn > {offset} ORDER BY _rn ASC"
        )
    };
    strace!(
        "op_find_percent h={} pct={} offset={} sql={}",
        hid,
        pct,
        offset,
        sql
    );

    match fetch_keyset_one(&meta, &sql) {
        Ok((recnum, packed, _)) => {
            write_data(data_buf, data_len, &packed);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.step_last_recnum = Some(recnum);
                    h.last_recnum = Some(recnum);
                }
            }
            0
        }
        Err(4) => BTR_EOF,
        Err(e) => e,
    }
}
