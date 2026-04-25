//! Key-ordered Get ops — 5 GetEqual, 6 GetNext, 7 GetPrev, 8 GetGreater,
//! 9 GetGE, 10 GetLess, 11 GetLE, 12 GetFirst, 13 GetLast.

use super::helpers::{
    clone_meta_and_idx, keybuf_bytes, posblk_key, strace, write_data, write_key_buf,
};
use super::sql_helpers::{
    build_continuation_where_marker, build_key_where, build_order_by_cols, extract_key_vals,
    fetch_keyset_one_with, index_col_refs, pick_index,
};
use crate::constants::*;
use crate::dialect::select_with_limit;
use crate::record::unpack_key_fields;
use crate::sql_param::SqlValue;
use crate::state::state;
use core::ffi::c_void;

/// Shared core for all key-search ops: GetEqual (5), GetGreater (8),
/// GetGE (9), GetLess (10), GetLE (11). Picks the index (by key number or
/// current index), builds an ORDER BY over its segments, emits a SQL
/// WHERE comparing the caller's key buffer against the segment columns
/// with `cmp`, and stores the resulting row + segment values as the new
/// logical position. `not_found_rc` is the key-specific not-found status.
#[allow(clippy::too_many_arguments)] // mirrors the Btrieve C-ABI signature
pub(super) fn op_get_by_key(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
    cmp: &str,
    dir: i8,
    not_found_rc: i32,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, cur_idx) = match clone_meta_and_idx(hid) {
        Some(m) => m,
        None => {
            match state().lock() {
                Err(_) => strace!("op_get_by_key FAIL: state mutex POISONED (hid={})", hid),
                Ok(st) => strace!(
                    "op_get_by_key FAIL: handle {} not found, open handles={:?}",
                    hid,
                    st.handles.keys().copied().collect::<Vec<_>>()
                ),
            }
            return BTR_FILE_NOT_OPEN;
        }
    };

    let idx = pick_index(&meta, key_num, cur_idx);
    let Some(idx) = idx else { return not_found_rc };
    let idx_num = idx.num;

    let null_wildcard = meta.ignore_null_values;
    let raw_kn = key_num as u16;
    let provided_key_len = if (raw_kn >> 8) != 0 {
        ((raw_kn & 0xFF) as usize).min(idx.key_len as usize)
    } else {
        idx.key_len as usize
    };
    let mut key_bytes = keybuf_bytes(key_buf as *const c_void, provided_key_len);
    key_bytes.resize(idx.key_len as usize, 0);
    let kf = unpack_key_fields(&meta.fields, idx, &key_bytes, null_wildcard);

    let dialect = crate::dialect::active();
    let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
        meta.fields.iter().map(|f| (f.num, f)).collect();
    let col_refs: Vec<(String, bool)> = idx
        .field_nums
        .iter()
        .zip(idx.desc.iter().copied().chain(std::iter::repeat(false)))
        .filter_map(|(n, d)| field_map.get(n).map(|f| (dialect.quote_ident(&f.name), d)))
        .collect();

    let mut params: Vec<SqlValue> = Vec::new();
    let key_cols: Vec<(String, String)> = col_refs
        .iter()
        .zip(kf)
        .filter_map(|((c, _), (_, opt_v))| {
            opt_v.map(|v| {
                params.push(v);
                (c.clone(), dialect.param_marker(params.len()))
            })
        })
        .collect();

    if null_wildcard && key_cols.is_empty() {
        return not_found_rc;
    }

    let where_clause = build_key_where(&key_cols, cmp);
    let order_by = build_order_by_cols(&col_refs, dir, false, &meta.recnum_sql_ref());
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let sql = select_with_limit(dialect, 1, &cols, &tref, &where_clause, &order_by);
    strace!("op_get_by_key h={} cmp={} sql={}", hid, cmp, sql);

    let seg_descs: Vec<bool> = col_refs.iter().map(|(_, d)| *d).collect();
    match fetch_keyset_one_with(&meta, &sql, &params) {
        Ok((recnum, packed, fields)) => {
            let last_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_index_num = Some(idx_num);
                    h.get_last_keys = last_keys;
                    h.get_last_desc = seg_descs;
                    h.get_last_recnum = Some(recnum);
                    h.get_dir = dir;
                    h.last_recnum = Some(recnum);
                }
            }
            // Spec: after a successful Get, the key buffer is updated with
            // the matched record's key field values.
            write_key_buf(key_buf, &meta, idx_num, &packed);
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => not_found_rc,
        Err(e) => {
            strace!("op_get_by_key err={} cmp={} h={}", e, cmp, hid);
            e
        }
    }
}

/// **Op 5 — Get Equal** (`B_GET_EQUAL`)
///
/// Retrieves the record whose value on the specified key path equals the
/// value in the key buffer. On duplicates, returns the chronologically
/// first (oldest) record in the chain. `+50` (op 55) yields Get Key Equal.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key buffer: target key value in the key's native binary format.
/// - key number: key path (0..118), or 125 for system log key.
///
/// **Prerequisites**: file open; not a data-only file.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open.
/// - 4: key value not found (BTR_KEY_NOT_FOUND).
/// - 6: invalid key number.
/// - 22: data buffer too short; buffer length set to actual record size.
///
/// **Notes**: establishes full logical + physical currency on the matched
/// record. Subsequent Get Next/Prev walks duplicates then the rest of the
/// index in order.
pub(super) fn op_get_equal(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    op_get_by_key(posblk, data_buf, data_len, key_buf, key_num, "=", 1, 4)
}

/// **Op 8 — Get Greater Than** (`B_GET_GREATER`)
///
/// Retrieves the first record whose key value is strictly greater than
/// the value in the key buffer. For descending keys "greater than"
/// reverses sense. On duplicates, returns the oldest at the matched key.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key buffer: comparison value.
/// - key number: key path.
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 4 no greater key, 6 bad key number, 9 EOF, 22 short buf.
///
/// **Notes**: establishes full logical + physical currency on the matched
/// record.
pub(super) fn op_get_greater(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    op_get_by_key(posblk, data_buf, data_len, key_buf, key_num, ">", 1, 9)
}

/// **Op 9 — Get Greater Than or Equal** (`B_GET_GE`)
///
/// Retrieves the first record whose key value is equal to or greater than
/// the key buffer value. Tries equal first, else next greater. Descending
/// keys reverse sense. On duplicates returns the oldest at the match.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key buffer: comparison value.
/// - key number: key path.
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 4 no equal-or-greater key, 6 bad key number, 9 EOF,
///   22 short buffer.
///
/// **Notes**: common "seek-to" primitive for range scans. Establishes
/// full logical + physical currency.
pub(super) fn op_get_greater_or_equal(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    op_get_by_key(posblk, data_buf, data_len, key_buf, key_num, ">=", 1, 9)
}

/// **Op 10 — Get Less Than** (`B_GET_LESS_THAN`)
///
/// Retrieves the first record whose key value is strictly less than the
/// key buffer value. On duplicates returns the chronologically last
/// (newest) at the match. Descending keys reverse sense.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key buffer: comparison value.
/// - key number: key path.
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 4 no lesser key, 6 bad key number, 9 BOF, 22 short buf.
///
/// **Notes**: establishes full logical + physical currency.
pub(super) fn op_get_less(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    op_get_by_key(posblk, data_buf, data_len, key_buf, key_num, "<", -1, 9)
}

/// **Op 11 — Get Less Than or Equal** (`B_GET_LE`)
///
/// Retrieves the first record whose key value is equal to or less than
/// the key buffer value. Tries equal first, else next lower. On
/// duplicates returns the chronologically last (newest) at the match.
/// Descending keys reverse sense.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key buffer: comparison value.
/// - key number: key path.
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 4 no equal-or-lesser key, 6 bad key number, 9 BOF,
///   22 short buffer.
///
/// **Notes**: establishes full logical + physical currency.
pub(super) fn op_get_less_or_equal(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    op_get_by_key(posblk, data_buf, data_len, key_buf, key_num, "<=", -1, 9)
}

/// **Op 12 — Get First** (`B_GET_FIRST`)
///
/// Retrieves the logical first record on the specified key path — the
/// smallest key value (largest for descending keys). On duplicates
/// returns the chronologically first (oldest) at that key.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key number: key path (125 for system data).
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 6 bad key number, 9 EOF (empty file), 22 short buffer.
///
/// **Notes**: establishes full logical + physical currency; the logical
/// previous position is "before BOF", so a subsequent Get Prev returns 9.
pub(super) fn op_get_first(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, cur_idx) = match clone_meta_and_idx(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let idx = pick_index(&meta, key_num, cur_idx);
    let Some(idx) = idx else { return BTR_EOF };
    let idx_num = idx.num;
    let col_refs = index_col_refs(&meta, idx_num);
    let seg_descs: Vec<bool> = col_refs.iter().map(|(_, d)| *d).collect();
    let dialect = crate::dialect::active();
    let order_by = build_order_by_cols(&col_refs, 1, false, &meta.recnum_sql_ref());
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let sql = select_with_limit(dialect, 1, &cols, &tref, "", &order_by);
    strace!("op_get_first h={} sql={}", hid, sql);
    match fetch_keyset_one_with(&meta, &sql, &[]) {
        Ok((recnum, packed, fields)) => {
            let last_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_index_num = Some(idx_num);
                    h.get_last_keys = last_keys;
                    h.get_last_desc = seg_descs;
                    h.get_last_recnum = Some(recnum);
                    h.get_dir = 1;
                    h.last_recnum = Some(recnum);
                }
            }
            write_key_buf(key_buf, &meta, idx_num, &packed);
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => BTR_EOF,
        Err(e) => {
            strace!("op_get_first err={} h={}", e, hid);
            e
        }
    }
}

/// **Op 13 — Get Last** (`B_GET_LAST`)
///
/// Retrieves the logical last record on the specified key path — the
/// largest key value (smallest for descending keys). On duplicates
/// returns the chronologically last (newest) at that key.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
/// - key number: key path.
///
/// **Prerequisites**: file open; not data-only.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 6 bad key number, 9 EOF (empty file), 22 short buffer.
///
/// **Notes**: establishes full logical + physical currency; the logical
/// next position is "past EOF", so a subsequent Get Next returns 9.
pub(super) fn op_get_last(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, cur_idx) = match clone_meta_and_idx(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let idx = pick_index(&meta, key_num, cur_idx);
    let Some(idx) = idx else { return BTR_EOF };
    let idx_num = idx.num;
    let col_refs = index_col_refs(&meta, idx_num);
    let seg_descs: Vec<bool> = col_refs.iter().map(|(_, d)| *d).collect();
    let dialect = crate::dialect::active();
    let order_by = build_order_by_cols(&col_refs, -1, false, &meta.recnum_sql_ref());
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let sql = select_with_limit(dialect, 1, &cols, &tref, "", &order_by);
    strace!("op_get_last h={} sql={}", hid, sql);
    match fetch_keyset_one_with(&meta, &sql, &[]) {
        Ok((recnum, packed, fields)) => {
            let last_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_index_num = Some(idx_num);
                    h.get_last_keys = last_keys;
                    h.get_last_desc = seg_descs;
                    h.get_last_recnum = Some(recnum);
                    h.get_dir = -1;
                    h.last_recnum = Some(recnum);
                }
            }
            write_key_buf(key_buf, &meta, idx_num, &packed);
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => BTR_EOF,
        Err(e) => {
            strace!("op_get_last err={} h={}", e, hid);
            e
        }
    }
}

/// **Op 6 — Get Next** (`B_GET_NEXT`)
///
/// Retrieves the record that follows the logical current record in the
/// key path's index order. Walks a duplicate chain in insertion order
/// before advancing to the next distinct key. Descending keys walk
/// toward smaller values.
///
/// **Parameters**:
/// - posblk: open file handle; current logical position is held in
///   `get_last_keys` / `get_last_recnum`.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file open; logical currency established on the key
/// path by a prior Get Equal/First/Greater/GE/Prev/Insert with same key.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 6 bad key number, 7 different key number,
///   8 no current record, 9 EOF, 22 short buffer.
///
/// **Notes**: establishes full logical + physical currency.
pub(super) fn op_get_next(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, idx_num, last_keys, last_desc, last_rn) = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        (
            h.meta.clone(),
            h.get_index_num,
            h.get_last_keys.clone(),
            h.get_last_desc.clone(),
            h.get_last_recnum,
        )
    };
    let idx_num = idx_num.or_else(|| meta.indexes.first().map(|ix| ix.num));
    let Some(idx_num) = idx_num else {
        return BTR_EOF;
    };
    let dialect = crate::dialect::active();
    let col_refs = index_col_refs(&meta, idx_num);
    let rc = meta.recnum_sql_ref();
    let order_by = build_order_by_cols(&col_refs, 1, true, &rc);
    let tref = meta.table_ref("", "");
    let cols = meta.select_with_recnum();
    let mut params: Vec<SqlValue> = Vec::new();
    let sql = if let (false, Some(rn)) = (last_keys.is_empty(), last_rn) {
        let key_cols: Vec<(String, String, bool)> = col_refs
            .iter()
            .zip(last_keys)
            .zip(last_desc.iter().copied().chain(std::iter::repeat(false)))
            .map(|((cr, val), d)| {
                params.push(val);
                (cr.0.clone(), dialect.param_marker(params.len()), d)
            })
            .collect();
        params.push(SqlValue::I64(rn));
        let last_rn_marker = dialect.param_marker(params.len());
        let where_clause = build_continuation_where_marker(&key_cols, 1, &last_rn_marker, &rc);
        select_with_limit(dialect, 1, &cols, &tref, &where_clause, &order_by)
    } else {
        select_with_limit(dialect, 1, &cols, &tref, "", &order_by)
    };
    strace!("op_get_next h={} sql={}", hid, sql);
    match fetch_keyset_one_with(&meta, &sql, &params) {
        Ok((recnum, packed, fields)) => {
            let new_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_last_keys = new_keys;
                    h.get_last_recnum = Some(recnum);
                    h.get_dir = 1;
                    h.last_recnum = Some(recnum);
                }
            }
            write_key_buf(key_buf, &meta, idx_num, &packed);
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => BTR_EOF,
        Err(e) => {
            strace!("op_get_next err={} h={}", e, hid);
            e
        }
    }
}

/// **Op 7 — Get Previous** (`B_GET_PREVIOUS`)
///
/// Retrieves the record preceding the logical current record in the key
/// path's index order. Walks a duplicate chain backward before moving
/// to the previous distinct key. Descending keys walk toward greater
/// values.
///
/// **Parameters**:
/// - posblk: open file handle; logical position held on the handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file open; logical currency established on the
/// key path.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3 not open, 6 bad key number, 7 different key, 8 no current,
///   9 BOF, 22 short buffer.
///
/// **Notes**: establishes full logical + physical currency.
pub(super) fn op_get_prev(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, idx_num, last_keys, last_desc, last_rn) = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        (
            h.meta.clone(),
            h.get_index_num,
            h.get_last_keys.clone(),
            h.get_last_desc.clone(),
            h.get_last_recnum,
        )
    };
    let idx_num = idx_num.or_else(|| meta.indexes.first().map(|ix| ix.num));
    let Some(idx_num) = idx_num else {
        return BTR_EOF;
    };
    let dialect = crate::dialect::active();
    let col_refs = index_col_refs(&meta, idx_num);
    let rc = meta.recnum_sql_ref();
    let order_by = build_order_by_cols(&col_refs, -1, true, &rc);
    let tref = meta.table_ref("", "");
    let cols = meta.select_with_recnum();
    let mut params: Vec<SqlValue> = Vec::new();
    let sql = if let (false, Some(rn)) = (last_keys.is_empty(), last_rn) {
        let key_cols: Vec<(String, String, bool)> = col_refs
            .iter()
            .zip(last_keys)
            .zip(last_desc.iter().copied().chain(std::iter::repeat(false)))
            .map(|((cr, val), d)| {
                params.push(val);
                (cr.0.clone(), dialect.param_marker(params.len()), d)
            })
            .collect();
        params.push(SqlValue::I64(rn));
        let last_rn_marker = dialect.param_marker(params.len());
        let where_clause = build_continuation_where_marker(&key_cols, -1, &last_rn_marker, &rc);
        select_with_limit(dialect, 1, &cols, &tref, &where_clause, &order_by)
    } else {
        select_with_limit(dialect, 1, &cols, &tref, "", &order_by)
    };
    strace!("op_get_prev h={} sql={}", hid, sql);
    match fetch_keyset_one_with(&meta, &sql, &params) {
        Ok((recnum, packed, fields)) => {
            let new_keys = extract_key_vals(&meta, idx_num, &fields);
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.get_last_keys = new_keys;
                    h.get_last_recnum = Some(recnum);
                    h.get_dir = -1;
                    h.last_recnum = Some(recnum);
                }
            }
            write_key_buf(key_buf, &meta, idx_num, &packed);
            write_data(data_buf, data_len, &packed);
            0
        }
        Err(4) => BTR_EOF,
        Err(e) => {
            strace!("op_get_prev err={} h={}", e, hid);
            e
        }
    }
}
