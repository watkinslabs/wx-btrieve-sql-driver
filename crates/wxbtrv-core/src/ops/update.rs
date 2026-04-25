//! Op 3 — Update. Op 53 — Update Chunk.

use super::helpers::{get_lock_prefix, posblk_key, strace};
use crate::constants::*;
use crate::dialect::select_with_limit;
use crate::record::{pack_row, unpack_row_typed};
use crate::sql::{execute_with, fetch_with, lock_row, unlock_row};
use crate::sql_param::SqlValue;
use crate::state::{state, TableMeta};
use core::ffi::c_void;
use core::slice;

/// Build a parameterized UPDATE for `meta` setting every non-autoinc field
/// to its corresponding value from the unpacked record. Returns
/// `(sql, params)`. The recnum predicate's marker is the last placeholder.
fn build_parameterized_update(
    meta: &TableMeta,
    record: &[u8],
    recnum: i64,
) -> Option<(String, Vec<SqlValue>)> {
    let dialect = crate::dialect::active();
    let cols = unpack_row_typed(&meta.fields, record);
    let rc_col = &meta.recnum_col;
    let is_sql_identity = rc_col != "MDS_RECNUM";
    let mut params: Vec<SqlValue> = Vec::new();
    let mut set_parts: Vec<String> = Vec::new();
    for ((name, value), f) in cols.into_iter().zip(meta.fields.iter()) {
        if f.native_type == 14
            || f.native_type == 15
            || (is_sql_identity && f.name.eq_ignore_ascii_case(rc_col))
        {
            continue;
        }
        params.push(value);
        set_parts.push(format!(
            "{} = {}",
            dialect.quote_ident(&name),
            dialect.param_marker(params.len())
        ));
    }
    if set_parts.is_empty() {
        return None;
    }
    params.push(SqlValue::I64(recnum));
    let where_marker = dialect.param_marker(params.len());
    let tref = meta.table_ref("", "");
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = {}",
        tref,
        set_parts.join(", "),
        meta.recnum_sql_ref(),
        where_marker
    );
    Some((sql, params))
}

/// **Op 3 — Update** (`B_UPDATE`)
///
/// Replaces the current record with a new image. Requires physical
/// currency established by a prior Get / Step / Get Direct / non-NCC
/// Insert on this handle. A plain Update may shift logical currency if
/// the key path's value changed; NCC Update (-1) leaves currency intact.
///
/// **Parameters**:
/// - posblk: identifies the open file; `last_recnum` is the target row.
/// - data buffer: new record image.
/// - data len: record length.
/// - key number (not taken here): original key path, -1 NCC, or 125 log.
///
/// **Prerequisites**: file open; physical currency established.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open.
/// - 5: duplicate key on unique index.
/// - 8: no current record (BTR_INVALID_POS).
/// - 10: modify of a non-MODIFIABLE key.
/// - 22: data buffer too short.
/// - 80: record-level conflict.
///
/// **Notes**: we lock the row via SQL before updating and release on exit.
pub(super) fn op_update(posblk: *mut c_void, data_buf: *const c_void, data_len: *mut u32) -> i32 {
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
    let dlen: usize = if data_len.is_null() {
        0
    } else {
        unsafe { (*data_len) as usize }
    };
    if data_buf.is_null() || dlen == 0 {
        return BTR_DATA_TOO_SHORT;
    }
    let record = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };

    let Some((sql, params)) = build_parameterized_update(&meta, record, recnum) else {
        return BTR_DATA_TOO_SHORT;
    };
    strace!("op_update h={} rn={} sql={}", hid, recnum, sql);
    let prefix = match get_lock_prefix(hid) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = lock_row(&prefix, recnum) {
        return e;
    }
    let rc = match execute_with(&sql, &params) {
        Ok(_) => BTR_SUCCESS,
        Err(e) => e,
    };
    let _ = unlock_row(&prefix, recnum);
    rc
}

/// **Op 53 — Update Chunk** (`B_UPDATE_CHUNK`)
///
/// Updates byte ranges within the current record without sending the whole
/// record image. Supports all three chunk descriptor flavors:
///
/// - **Random** (sig `0x00000000`): N independently positioned chunks.
/// - **Rectangle** (sig `0x80000000` — high bit set): N equally spaced
///   equally sized chunks, described by `numChunks`, `chunkSize`,
///   `stride (nextOffset)`, `firstOffset` and `numChunks * chunkSize`
///   bytes of data.
/// - **Truncate** (sig `0xFFFFFFFE`): truncate the record to the given
///   new length. Since our SQL-backed records are fixed-length, truncating
///   below the fixed portion returns status 28; at/above it is a no-op.
///
/// Strategy: fetch the current record via `last_recnum`, pack it, apply
/// the chunks to the packed buffer, then feed the mutated record through
/// the same unpack+UPDATE path as op_update.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: chunk descriptor.
/// - `data_len`: total descriptor + chunk data size.
/// - `key_num`: unused by us.
///
/// Status: 0 success, 3 file not open, 8 no current record, 22 data too
/// short, 28 truncation below minimum, 62 invalid / unknown chunk
/// descriptor, 103 chunk offset beyond end of record.
pub(super) fn op_update_chunk(
    posblk: *mut c_void,
    data_buf: *const c_void,
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
    let Some(recnum) = last_rn else {
        return BTR_INVALID_POS;
    };
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    if data_buf.is_null() || dlen < 12 {
        return BTR_DATA_TOO_SHORT;
    }
    let desc = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };
    let signature = u32::from_le_bytes([desc[0], desc[1], desc[2], desc[3]]);

    // Parse the descriptor into a unified list of (record_offset, data_slice).
    // Rectangle is flattened into N random chunks of chunkSize at the same
    // stride; truncate is handled specially below.
    enum Flavor {
        Random,
        Rectangle,
        Truncate,
    }
    let flavor = match signature {
        0x0000_0000 => Flavor::Random,
        // Rectangle: high bit set. Spec references both 0x80000000 and
        // 0x80000001; accept either by masking.
        s if s & 0x8000_0000 != 0 && s != 0xFFFF_FFFE => Flavor::Rectangle,
        // Truncate: spec signature 0xFFFFFFFE; also commonly written as
        // 0x40000000 in older Btrieve docs. Accept either.
        0xFFFF_FFFE | 0x4000_0000 => Flavor::Truncate,
        _ => {
            strace!(
                "op_update_chunk h={} unknown signature={:#010x}",
                hid,
                signature
            );
            return 62;
        }
    };

    // Handle TRUNCATE as an early return — it only needs a length check,
    // no record fetch/update (our records are fixed-length).
    if let Flavor::Truncate = flavor {
        if desc.len() < 8 {
            return BTR_DATA_TOO_SHORT;
        }
        let new_len = u32::from_le_bytes([desc[4], desc[5], desc[6], desc[7]]) as u32;
        strace!(
            "op_update_chunk h={} TRUNCATE new_len={} rec_len={}",
            hid,
            new_len,
            meta.record_length
        );
        if new_len < meta.record_length {
            // Truncation below the fixed record length is rejected per spec.
            return 28;
        }
        // At or above fixed length: no-op for our backend.
        return BTR_SUCCESS;
    }

    let mut chunks: Vec<(usize, usize)> = Vec::new();
    let data_start: usize = match flavor {
        Flavor::Random => {
            let n_chunks = u32::from_le_bytes([desc[4], desc[5], desc[6], desc[7]]) as usize;
            let next_off = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]) as usize;
            strace!(
                "op_update_chunk h={} RANDOM rn={} n_chunks={} next_off={}",
                hid,
                recnum,
                n_chunks,
                next_off
            );
            // Parse (offset, length) headers; chunk data follows after the last header.
            let mut hdr_off = 12usize;
            chunks.reserve(n_chunks);
            for _ in 0..n_chunks {
                if hdr_off + 8 > desc.len() {
                    return 62;
                }
                let off = u32::from_le_bytes([
                    desc[hdr_off],
                    desc[hdr_off + 1],
                    desc[hdr_off + 2],
                    desc[hdr_off + 3],
                ]) as usize;
                let len = u32::from_le_bytes([
                    desc[hdr_off + 4],
                    desc[hdr_off + 5],
                    desc[hdr_off + 6],
                    desc[hdr_off + 7],
                ]) as usize;
                chunks.push((off, len));
                hdr_off += 8;
            }
            hdr_off
        }
        Flavor::Rectangle => {
            // Layout: sig(4) numChunks(4) chunkSize(4) stride(4) firstOffset(4)
            //         then numChunks*chunkSize bytes of data.
            if desc.len() < 20 {
                return BTR_DATA_TOO_SHORT;
            }
            let n_chunks = u32::from_le_bytes([desc[4], desc[5], desc[6], desc[7]]) as usize;
            let chunk_sz = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]) as usize;
            let stride = u32::from_le_bytes([desc[12], desc[13], desc[14], desc[15]]) as usize;
            let first_off = u32::from_le_bytes([desc[16], desc[17], desc[18], desc[19]]) as usize;
            strace!(
                "op_update_chunk h={} RECTANGLE rn={} n={} sz={} stride={} first={}",
                hid,
                recnum,
                n_chunks,
                chunk_sz,
                stride,
                first_off
            );
            chunks.reserve(n_chunks);
            for i in 0..n_chunks {
                chunks.push((first_off + i * stride, chunk_sz));
            }
            20
        }
        Flavor::Truncate => unreachable!(),
    };
    let mut data_ptr = data_start;

    // Fetch current record.
    let cols = meta.select_with_recnum();
    let dialect = crate::dialect::active();
    let tref = meta.table_ref("", "");
    let where_marker = dialect.param_marker(1);
    let where_clause = format!("{} = {}", meta.recnum_sql_ref(), where_marker);
    let sql_sel = select_with_limit(dialect, 1, &cols, &tref, &where_clause, "");
    let recnum_is_field = meta
        .fields
        .iter()
        .any(|f| f.name.eq_ignore_ascii_case(&meta.recnum_col));
    let n_cols = if recnum_is_field {
        meta.fields.len()
    } else {
        1 + meta.fields.len()
    };
    let row = match fetch_with(&sql_sel, &[SqlValue::I64(recnum)], n_cols, 1) {
        Ok(mut rs) => match rs.pop() {
            Some(r) => r,
            None => {
                strace!("op_update_chunk h={} no row", hid);
                return BTR_INVALID_POS;
            }
        },
        Err(e) => {
            strace!("op_update_chunk h={} fetch err={}", hid, e);
            return BTR_INVALID_POS;
        }
    };
    let field_vals: Vec<String> = if recnum_is_field {
        let mut f = vec![row[0].clone()];
        f.extend_from_slice(&row[1..]);
        f
    } else {
        row[1..].to_vec()
    };
    let mut packed = pack_row(&meta.fields, &field_vals, meta.record_length);

    // Apply chunks.
    for (off, len) in &chunks {
        if data_ptr + len > desc.len() {
            return BTR_DATA_TOO_SHORT;
        }
        if off + len > packed.len() {
            // Extending VAR_RECS is not meaningfully supported here.
            strace!(
                "op_update_chunk h={} chunk off={} len={} exceeds rec_len {}",
                hid,
                off,
                len,
                packed.len()
            );
            return 103;
        }
        packed[*off..*off + *len].copy_from_slice(&desc[data_ptr..data_ptr + *len]);
        data_ptr += *len;
    }

    // Push the mutated record via a parameterized UPDATE — same path as op_update.
    let Some((sql_upd, params)) = build_parameterized_update(&meta, &packed, recnum) else {
        return BTR_DATA_TOO_SHORT;
    };
    strace!("op_update_chunk h={} sql={}", hid, sql_upd);
    let prefix = match get_lock_prefix(hid) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = lock_row(&prefix, recnum) {
        return e;
    }
    let rc = match execute_with(&sql_upd, &params) {
        Ok(_) => BTR_SUCCESS,
        Err(e) => e,
    };
    let _ = unlock_row(&prefix, recnum);
    rc
}
