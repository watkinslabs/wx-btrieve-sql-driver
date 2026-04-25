//! Extended Get ops — 36 GetNextExtended, 37 GetPreviousExtended.
//!
//! These ops drive the same index-ordered walk as Get Next / Get Previous,
//! but return a batch of records in one call and accept an optional filter
//! descriptor. Filter translation is implemented: TERM_HEADER entries are
//! parsed into TermClauses and combined into a SQL WHERE fragment that is
//! appended to the index-ordered continuation query.

use super::helpers::{posblk_key, strace};
use super::sql_helpers::{
    build_continuation_where_marker, build_filter_where, build_order_by_cols, extract_key_vals,
    fetch_keyset_one_with, index_col_refs, parse_filter_terms, TermClause,
};
use crate::constants::*;
use crate::dialect::select_with_limit;
use crate::sql_param::SqlValue;
use crate::state::{state, TableMeta};
use core::ffi::c_void;
use core::ptr;
use core::slice;

/// Parsed Get*Extended descriptor header (input).
pub(super) struct GneDesc {
    pub description_len: u16,
    pub _currency_const: u8,
    pub reject_count: u16,
    pub number_terms: u16,
    pub max_recs: u16,
    pub field_extracts: Vec<(u16, u16)>, // (len, offset)
    pub terms: Vec<TermClause>,
}

/// Parse the GNE input descriptor. Returns None on invalid layout.
pub(super) fn parse_gne(desc: &[u8], meta: &TableMeta) -> Option<GneDesc> {
    if desc.len() < 8 {
        return None;
    }
    let description_len = u16::from_le_bytes([desc[0], desc[1]]);
    let currency_const = desc[2];
    let reject_count = u16::from_le_bytes([desc[4], desc[5]]);
    let number_terms = u16::from_le_bytes([desc[6], desc[7]]);

    let (terms, mut off) = parse_filter_terms(meta, desc, 8, number_terms as usize)?;

    // RETRIEVAL_HEADER (4 bytes)
    if off + 4 > desc.len() {
        return None;
    }
    let max_recs = u16::from_le_bytes([desc[off], desc[off + 1]]);
    let no_fields = u16::from_le_bytes([desc[off + 2], desc[off + 3]]) as usize;
    off += 4;

    // FIELD_RETRIEVAL_HEADER (4 bytes each)
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
    Some(GneDesc {
        description_len,
        _currency_const: currency_const,
        reject_count,
        number_terms,
        max_recs,
        field_extracts,
        terms,
    })
}

/// Project a record's bytes to a GNE FIELD_RETRIEVAL list. When the list is
/// empty, return the whole packed record.
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

/// Fetch the next record in index order for a given handle (direction dir=1/-1).
/// Mirrors the SQL in op_get_next / op_get_prev but returns the tuple inline.
/// `filter_terms` is the optional filter clause (parsed from the descriptor)
/// — when present, its placeholders are appended after the keyset
/// continuation params so each backend's `?` / `$N` numbering stays correct.
fn fetch_one_extended(
    hid: u32,
    meta: &TableMeta,
    dir: i8,
    filter_terms: Option<&[TermClause]>,
) -> Result<(i64, Vec<u8>, Vec<String>, u32), i32> {
    let (idx_num, last_keys, last_desc, last_rn) = {
        let Ok(st) = state().lock() else {
            return Err(BTR_FILE_NOT_OPEN);
        };
        let Some(h) = st.handles.get(&hid) else {
            return Err(BTR_FILE_NOT_OPEN);
        };
        (
            h.get_index_num,
            h.get_last_keys.clone(),
            h.get_last_desc.clone(),
            h.get_last_recnum,
        )
    };
    let idx_num = idx_num
        .or_else(|| meta.indexes.first().map(|ix| ix.num))
        .ok_or(BTR_EOF)?;
    let dialect = crate::dialect::active();
    let col_refs = index_col_refs(meta, idx_num);
    let rc = meta.recnum_sql_ref();
    let order_by = build_order_by_cols(&col_refs, dir, true, &rc);
    let tref = meta.table_ref("", "");
    let cols = meta.select_with_recnum();
    let mut params: Vec<SqlValue> = Vec::new();
    let base_where = if let (false, Some(rn)) = (last_keys.is_empty(), last_rn) {
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
        Some(build_continuation_where_marker(
            &key_cols,
            dir,
            &last_rn_marker,
            &rc,
        ))
    } else {
        None
    };
    // Append filter-term params (keyed off params.len() so markers
    // continue correctly past any keyset markers above).
    let filter_where = filter_terms
        .filter(|t| !t.is_empty())
        .map(|t| build_filter_where(t, &mut params));
    let combined = match (base_where, filter_where) {
        (Some(b), Some(f)) => format!("({}) AND ({})", b, f),
        (Some(b), None) => b,
        (None, Some(f)) => f,
        (None, None) => String::new(),
    };
    let sql = select_with_limit(dialect, 1, &cols, &tref, &combined, &order_by);
    let (recnum, packed, fields) = fetch_keyset_one_with(meta, &sql, &params)?;
    Ok((recnum, packed, fields, idx_num))
}

/// Core GetNext/PrevExtended driver.
fn run_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    dir: i8,
    op_label: &str,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        let Some(h) = st.handles.get(&hid) else {
            return BTR_FILE_NOT_OPEN;
        };
        h.meta.clone()
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
    let desc = match parse_gne(desc_view, &meta) {
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

    let filter_terms: Option<&[TermClause]> = if desc.terms.is_empty() {
        None
    } else {
        // Render-once for trace logging (params discarded — the real
        // params are bound per-call inside fetch_one_extended).
        let mut trace_params = Vec::new();
        let w = build_filter_where(&desc.terms, &mut trace_params);
        strace!("{} h={} filter_where={}", op_label, hid, w);
        Some(&desc.terms)
    };

    let max_recs = if desc.max_recs == 0 {
        u16::MAX as usize
    } else {
        desc.max_recs as usize
    };

    // Walk records.
    let mut records: Vec<(i64, Vec<u8>, Vec<String>, u32)> = Vec::new();
    let mut projected_len_total: usize = 0;
    let per_hdr = 6usize; // recLen + recPos
    let post_hdr = 2usize;

    while records.len() < max_recs {
        match fetch_one_extended(hid, &meta, dir, filter_terms) {
            Ok((rn, packed, fields, idx_num)) => {
                let proj = project(&packed, &desc.field_extracts);
                // Check output capacity budget.
                let next_total = post_hdr + projected_len_total + per_hdr + proj.len();
                if next_total > cap {
                    if records.is_empty() {
                        return BTR_DATA_TOO_SHORT;
                    }
                    break;
                }
                projected_len_total += per_hdr + proj.len();
                // Persist currency after each successful fetch.
                let new_keys = extract_key_vals(&meta, idx_num, &fields);
                if let Ok(mut st) = state().lock() {
                    if let Some(h) = st.handles.get_mut(&hid) {
                        h.get_index_num = Some(idx_num);
                        h.get_last_keys = new_keys;
                        h.get_last_recnum = Some(rn);
                        h.get_dir = dir;
                        h.last_recnum = Some(rn);
                    }
                }
                records.push((rn, proj, fields, idx_num));
            }
            Err(BTR_KEY_NOT_FOUND) | Err(BTR_EOF) => {
                break;
            }
            Err(e) => {
                strace!("{} h={} fetch err={}", op_label, hid, e);
                return e;
            }
        }
    }

    if records.is_empty() {
        // Status 64 = filter rejected all records in the remainder of the file.
        // Status 9 = plain EOF. We distinguish by whether a filter was active.
        return if filter_terms.is_some() { 64 } else { BTR_EOF };
    }

    // Write POST_BUFFER_HEADER + records into data_buf.
    let num_returned = records.len() as u16;
    let total: usize = post_hdr + projected_len_total;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&num_returned.to_le_bytes());
    for (rn, proj, _fields, _idx_num) in &records {
        let rec_len = proj.len() as u16;
        let rec_pos = (*rn).max(0) as u32;
        out.extend_from_slice(&rec_len.to_le_bytes());
        out.extend_from_slice(&rec_pos.to_le_bytes());
        out.extend_from_slice(proj);
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

/// **Op 36 — Get Next Extended** (`B_GET_NEXT_EXTENDED`)
///
/// Retrieves a batch of records following the current logical position on
/// the active index path. The input descriptor in `data_buf` specifies the
/// reject count, filter terms, max records, and fields to project.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: GNE input descriptor / output record list.
/// - `data_len`: capacity on input, bytes written on output.
///
/// Filter terms are translated to a SQL WHERE fragment appended to the
/// continuation query. AND/OR connectors follow Btrieve spec precedence
/// (AND tighter than OR).
///
/// Status: 0 success, 3 file not open, 9 EOF, 22 buffer too short, 62
/// invalid descriptor, 64 filter matched no records.
pub(super) fn op_get_next_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    run_extended(posblk, data_buf, data_len, 1, "op_get_next_extended")
}

/// **Op 37 — Get Previous Extended** (`B_GET_PREVIOUS_EXTENDED`)
///
/// Same as Get Next Extended but walks backward on the active index path.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: GNE input descriptor / output record list.
/// - `data_len`: capacity on input, bytes written on output.
///
/// Status: 0 success, 3 file not open, 9 EOF, 22 buffer too short, 62
/// invalid descriptor, 64 filter matched no records.
pub(super) fn op_get_prev_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    run_extended(posblk, data_buf, data_len, -1, "op_get_prev_extended")
}
