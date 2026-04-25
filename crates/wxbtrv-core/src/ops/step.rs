//! Step ops — 24 StepNext, 33 StepFirst, 34 StepLast, 35 StepPrev.
//! Physical-order navigation based on recnum_col.

use super::helpers::{posblk_key, strace, write_data};
use super::sql_helpers::build_step_select_n;
use super::sql_helpers::STEP_CHUNK_SIZE;
use crate::constants::*;
use crate::sql::fetch_rows_positional;
use crate::state::state;
use core::ffi::c_void;

/// Serve one row from the step cache, or fetch a new chunk if the cache is empty.
pub(super) fn step_cached(hid: u32, data_buf: *mut c_void, data_len: *mut u32, dir: i8) -> i32 {
    let cache_hit: Option<(i64, Vec<u8>)> = {
        let Ok(mut st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get_mut(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        if h.step_cache_dir == dir {
            if let Some((rn, packed)) = h.step_cache.pop_front() {
                h.step_last_recnum = Some(rn);
                h.last_recnum = Some(rn);
                Some((rn, packed))
            } else {
                None
            }
        } else {
            h.step_cache.clear();
            None
        }
    };
    if let Some((_rn, packed)) = cache_hit {
        write_data(data_buf, data_len, &packed);
        return 0;
    }

    let (meta, last_rn) = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        (h.meta.clone(), h.step_last_recnum)
    };

    let n = if meta.local_cache { STEP_CHUNK_SIZE } else { 1 };
    let sql = build_step_select_n(&meta, last_rn, dir, n);
    strace!(
        "step_cached h={} dir={} last_rn={:?} chunk={} sql={}",
        hid,
        dir,
        last_rn,
        n,
        sql
    );
    let rows = fetch_rows_positional(&sql, meta.fields.len() + 1, n);
    match rows {
        Err(e) => {
            if e == 4 {
                BTR_EOF
            } else {
                e
            }
        }
        Ok(rows) if rows.is_empty() => BTR_EOF,
        Ok(rows) => {
            let mut cache_rows = std::collections::VecDeque::new();
            for row in rows {
                if row.is_empty() {
                    continue;
                }
                let rn: i64 = row[0].trim().parse().unwrap_or(0);
                let col_strs: Vec<String> = row[1..].to_vec();
                let packed = crate::record::pack_row(&meta.fields, &col_strs, meta.record_length);
                cache_rows.push_back((rn, packed));
            }
            let (result_rn, result_packed) = match cache_rows.pop_front() {
                Some(row) => row,
                None => return BTR_EOF,
            };
            if let Ok(mut st) = state().lock() {
                if let Some(h) = st.handles.get_mut(&hid) {
                    h.step_last_recnum = Some(result_rn);
                    h.last_recnum = Some(result_rn);
                    h.step_dir = dir;
                    h.step_cache_dir = dir;
                    h.step_cache = cache_rows;
                }
            }
            write_data(data_buf, data_len, &result_packed);
            0
        }
    }
}

/// **Op 33 — Step First** (`B_STEP_FIRST`)
///
/// Retrieves the physically first record in the file. Physical order
/// reflects storage layout, not any index.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 9 empty file,
/// 22 short buffer, 84 record/page locked.
///
/// **Notes**: establishes physical currency only; logical currency is
/// destroyed. Clears any prior step cache on this handle.
pub(super) fn op_step_first(posblk: *mut c_void, data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.step_cache.clear();
            h.step_last_recnum = None;
        }
    }
    step_cached(hid, data_buf, data_len, 1)
}

/// **Op 24 — Step Next** (`B_STEP_NEXT`)
///
/// Retrieves the next record in physical storage order, independent of
/// any index. After Open the implicit "pre-first" physical position lets
/// Step Next return the first record on disk — standard recovery path
/// when indexes are corrupt (combine with Read-Only open).
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file must be open; a physical current position
/// (the initial pre-first counts).
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 8 no current,
/// 9 EOF, 22 short buffer, 84 record/page locked.
///
/// **Notes**: establishes physical currency only — logical currency is
/// destroyed. Served from an internal step cache when `local_cache` is
/// on (fetches rows in chunks for throughput).
pub(super) fn op_step_next(posblk: *mut c_void, data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    step_cached(hid, data_buf, data_len, 1)
}

/// **Op 34 — Step Last** (`B_STEP_LAST`)
///
/// Retrieves the physically last record in the file.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 9 empty file,
/// 22 short buffer, 84 record/page locked.
///
/// **Notes**: establishes physical currency only; logical currency is
/// destroyed. Clears any prior step cache on this handle.
pub(super) fn op_step_last(posblk: *mut c_void, data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.step_cache.clear();
            h.step_last_recnum = None;
        }
    }
    step_cached(hid, data_buf, data_len, -1)
}

/// **Op 35 — Step Previous** (`B_STEP_PREVIOUS`)
///
/// Retrieves the previous record in physical storage order.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives the record image.
/// - data len: capacity in / actual record length out.
///
/// **Prerequisites**: file must be open; physical current record exists.
///
/// **Returns**: 0 on success. Spec statuses: 3 not open, 8 no current,
/// 9 BOF, 22 short buffer, 84 record/page locked.
///
/// **Notes**: establishes physical currency only; logical currency is
/// destroyed. Served from the step cache when the direction matches.
pub(super) fn op_step_prev(posblk: *mut c_void, data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    step_cached(hid, data_buf, data_len, -1)
}
