//! Extended Step ops — 38 StepNextExtended, 39 StepPreviousExtended.
//!
//! Same descriptor layout as Get Next/Previous Extended (see get_ext.rs), but
//! walks the physical record order (recnum column) instead of an index path.
//! Filter terms are translated into a SQL WHERE fragment and appended to the
//! physical-order batched SELECT.

use super::helpers::{posblk_key, strace};
use super::sql_helpers::{build_filter_where, parse_filter_terms, TermClause};
use crate::constants::*;
use crate::record::pack_row;
use crate::sql::fetch_rows_positional;
use crate::state::{state, TableMeta};
use core::ffi::c_void;
use core::ptr;
use core::slice;

struct SneDesc {
    description_len: u16,
    reject_count: u16,
    number_terms: u16,
    max_recs: u16,
    field_extracts: Vec<(u16, u16)>,
    terms: Vec<TermClause>,
}

fn parse_sne(desc: &[u8], meta: &TableMeta) -> Option<SneDesc> {
    if desc.len() < 8 {
        return None;
    }
    let description_len = u16::from_le_bytes([desc[0], desc[1]]);
    let reject_count = u16::from_le_bytes([desc[4], desc[5]]);
    let number_terms = u16::from_le_bytes([desc[6], desc[7]]);
    let (terms, mut off) = parse_filter_terms(meta, desc, 8, number_terms as usize)?;
    if off + 4 > desc.len() {
        return None;
    }
    let max_recs = u16::from_le_bytes([desc[off], desc[off + 1]]);
    let no_fields = u16::from_le_bytes([desc[off + 2], desc[off + 3]]) as usize;
    off += 4;
    let mut field_extracts: Vec<(u16, u16)> = Vec::with_capacity(no_fields);
    for _ in 0..no_fields {
        if off + 4 > desc.len() {
            return None;
        }
        let flen = u16::from_le_bytes([desc[off], desc[off + 1]]);
        let foff = u16::from_le_bytes([desc[off + 2], desc[off + 3]]);
        field_extracts.push((flen, foff));
        off += 4;
    }
    Some(SneDesc {
        description_len,
        reject_count,
        number_terms,
        max_recs,
        field_extracts,
        terms,
    })
}

fn project(packed: &[u8], extracts: &[(u16, u16)]) -> Vec<u8> {
    if extracts.is_empty() {
        return packed.to_vec();
    }
    let mut out = Vec::new();
    for &(len, off) in extracts {
        let start = off as usize;
        let end = start + len as usize;
        if end <= packed.len() {
            out.extend_from_slice(&packed[start..end]);
        } else {
            let have = packed.len().saturating_sub(start);
            if have > 0 {
                out.extend_from_slice(&packed[start..start + have]);
            }
            out.resize(out.len() + (len as usize - have), 0);
        }
    }
    out
}

fn fetch_step_batch(
    meta: &TableMeta,
    last_rn: Option<i64>,
    dir: i8,
    n: usize,
    extra_where: Option<&str>,
) -> Result<Vec<(i64, Vec<u8>)>, i32> {
    let rc = meta.recnum_sql_ref();
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let (ord_dir, cmp) = if dir >= 0 {
        ("ASC", ">")
    } else {
        ("DESC", "<")
    };
    let base_where = last_rn.map(|rn| format!("{} {} {}", rc, cmp, rn));
    let combined = match (base_where, extra_where) {
        (Some(b), Some(f)) => format!("({}) AND ({})", b, f),
        (Some(b), None) => b,
        (None, Some(f)) => f.to_string(),
        (None, None) => String::new(),
    };
    let sql = if combined.is_empty() {
        format!("SELECT TOP {n} {cols} FROM {tref} ORDER BY {rc} {ord_dir}")
    } else {
        format!("SELECT TOP {n} {cols} FROM {tref} WHERE {combined} ORDER BY {rc} {ord_dir}")
    };
    let rows = fetch_rows_positional(&sql, meta.fields.len() + 1, n)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.is_empty() {
            continue;
        }
        let rn: i64 = row[0].trim().parse().unwrap_or(0);
        let col_strs: Vec<String> = row[1..].to_vec();
        let packed = pack_row(&meta.fields, &col_strs, meta.record_length);
        out.push((rn, packed));
    }
    Ok(out)
}

fn run_step_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    dir: i8,
    op_label: &str,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let (meta, mut last_rn) = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        (h.meta.clone(), h.step_last_recnum)
    };

    let cap = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    if data_buf.is_null() || cap < 8 {
        return BTR_DATA_TOO_SHORT;
    }
    let desc_view = unsafe { slice::from_raw_parts(data_buf as *const u8, cap) };
    let desc = match parse_sne(desc_view, &meta) {
        Some(d) => d,
        None => {
            strace!("{} h={} invalid descriptor", op_label, hid);
            return 62;
        }
    };
    strace!(
        "{} h={} dir={} desc_len={} reject={} terms={} max_recs={} extract_fields={}",
        op_label,
        hid,
        dir,
        desc.description_len,
        desc.reject_count,
        desc.number_terms,
        desc.max_recs,
        desc.field_extracts.len()
    );

    let filter_where = if desc.terms.is_empty() {
        None
    } else {
        let w = build_filter_where(&desc.terms);
        strace!("{} h={} filter_where={}", op_label, hid, w);
        Some(w)
    };

    let max_recs = if desc.max_recs == 0 {
        256
    } else {
        (desc.max_recs as usize).min(1024)
    };
    let batch = match fetch_step_batch(&meta, last_rn, dir, max_recs, filter_where.as_deref()) {
        Ok(b) => b,
        Err(4) => return BTR_EOF,
        Err(e) => {
            strace!("{} h={} fetch err={}", op_label, hid, e);
            return e;
        }
    };
    if batch.is_empty() {
        return BTR_EOF;
    }

    // Build output buffer honoring capacity.
    let per_hdr = 6usize;
    let mut out = Vec::with_capacity(cap.min(4096));
    out.extend_from_slice(&[0u8, 0u8]); // placeholder for numReturned
    let mut num_returned: u16 = 0;
    for (rn, packed) in &batch {
        let proj = project(packed, &desc.field_extracts);
        let add = per_hdr + proj.len();
        if out.len() + add > cap {
            break;
        }
        let rec_len = proj.len() as u16;
        let rec_pos = (*rn).max(0) as u32;
        out.extend_from_slice(&rec_len.to_le_bytes());
        out.extend_from_slice(&rec_pos.to_le_bytes());
        out.extend_from_slice(&proj);
        num_returned += 1;
        last_rn = Some(*rn);
    }
    if num_returned == 0 {
        return BTR_DATA_TOO_SHORT;
    }
    out[0..2].copy_from_slice(&num_returned.to_le_bytes());

    // Update physical currency on the handle.
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.step_last_recnum = last_rn;
            h.last_recnum = last_rn;
            h.step_dir = dir;
            h.step_cache.clear();
        }
    }

    unsafe {
        ptr::copy_nonoverlapping(out.as_ptr(), data_buf as *mut u8, out.len());
        *data_len = out.len() as u32;
    }
    strace!(
        "{} h={} returned={} total_bytes={}",
        op_label,
        hid,
        num_returned,
        out.len()
    );
    BTR_SUCCESS
}

/// **Op 38 — Step Next Extended** (`B_STEP_NEXT_EXT`)
///
/// Returns a batch of records from the physical position following the
/// current one, optionally filtered and field-projected. Establishes only
/// physical currency (no key buffer update).
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: input descriptor / output record list.
/// - `data_len`: capacity on input, bytes written on output.
///
/// Filter terms are translated into a SQL WHERE fragment appended to the
/// physical-order batched SELECT.
/// Status: 0 success, 3 file not open, 9 EOF, 22 buffer too short, 62
/// invalid descriptor.
pub(super) fn op_step_next_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    run_step_extended(posblk, data_buf, data_len, 1, "op_step_next_extended")
}

/// **Op 39 — Step Previous Extended** (`B_STEP_PREV_EXT`)
///
/// Same as Step Next Extended but walks physical order backward.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: input descriptor / output record list.
/// - `data_len`: capacity on input, bytes written on output.
///
/// Filter terms are translated into a SQL WHERE fragment appended to the
/// physical-order batched SELECT.
/// Status: 0 success, 3 file not open, 9 EOF, 22 buffer too short, 62
/// invalid descriptor.
pub(super) fn op_step_prev_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    run_step_extended(posblk, data_buf, data_len, -1, "op_step_prev_extended")
}
