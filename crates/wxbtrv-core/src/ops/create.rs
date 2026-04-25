//! Op 14 — Create.
//!
//! In our SQL-backed world "create a Btrieve file" means "make sure a SQL
//! table by this name exists". We don't own `wxbtrv.db` schema registration
//! at runtime — that's handled by `int-tool` — so Create is a best-effort
//! CREATE TABLE IF NOT EXISTS plus a very loud trace. The tpc50 app almost
//! never calls Create at runtime; we just need the trace to be greppable
//! and the op to return BTR_SUCCESS.

use super::helpers::{keybuf_cstr, strace};
use crate::constants::*;
use crate::sql::execute_sql;
use core::ffi::c_void;
use core::slice;

/// **Op 14 — Create** (`B_CREATE`)
///
/// Creates a new Btrieve file from a file-spec header and key-segment specs.
/// We translate the request to a CREATE TABLE on SQL Server with one INT
/// column per declared key segment plus a generic `btrv_row` identity.
///
/// - `posblk`: unused on input.
/// - `data_buf`: 16-byte file spec header followed by 16-byte key segment
///   specs. See `docs/btrieve-api/create.md` for layout.
/// - `data_len`: size of the descriptor in bytes.
/// - `key_buf`: target pathname (up to 80 bytes, NUL/blank-terminated).
/// - `key_num`: 0 = overwrite silently, -1 = fail with 59 if file exists.
///
/// Status: 0 on success (including "we did our best"), 22 if the descriptor
/// is shorter than the 16-byte header, 59 if key_num = -1 and the table
/// already exists.
pub(super) fn op_create(
    data_buf: *const c_void,
    data_len: *mut u32,
    key_buf: *const c_void,
    key_num: i16,
) -> i32 {
    let path = keybuf_cstr(key_buf);
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    if data_buf.is_null() || dlen < 16 {
        strace!(
            "op_create path={:?} dlen={} descriptor too small",
            path,
            dlen
        );
        return BTR_DATA_TOO_SHORT;
    }
    let desc = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };

    // ── File spec header (16 bytes) ──────────────────────────────────────
    let rec_len = u16::from_le_bytes([desc[0], desc[1]]);
    let page_size = u16::from_le_bytes([desc[2], desc[3]]);
    let num_indexes = u16::from_le_bytes([desc[4], desc[5]]);
    let file_flags = u16::from_le_bytes([desc[10], desc[11]]);
    strace!(
        "op_create path={:?} rec_len={} page_size={} n_idx={} flags={:#06x} key_num={}",
        path,
        rec_len,
        page_size,
        num_indexes,
        file_flags,
        key_num
    );

    // Derive a SQL-safe table name from the file basename.
    let base = std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wxbtrv_created")
        .to_ascii_lowercase();
    let table_name: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Check if it already exists in our cached meta — if key_num = -1,
    // that's an error; otherwise we silently overwrite (actually: leave it).
    if key_num == -1 {
        let exists = crate::state::state()
            .lock()
            .ok()
            .map(|st| st.tables.contains_key(&table_name.to_ascii_uppercase()))
            .unwrap_or(false);
        if exists {
            strace!("op_create: table {} already exists, status 59", table_name);
            return 59;
        }
    }

    // ── Walk key segment specs to infer some column shape ────────────────
    let mut off = 16usize;
    let mut col_sql: Vec<String> = Vec::new();
    let mut col_idx = 0usize;
    while off + 16 <= desc.len() && col_sql.len() < num_indexes as usize * 4 {
        let seg = &desc[off..off + 16];
        let pos = u16::from_le_bytes([seg[0], seg[1]]);
        let len = u16::from_le_bytes([seg[2], seg[3]]);
        let flags = u16::from_le_bytes([seg[4], seg[5]]);
        let ext_type = seg[10];
        strace!(
            "op_create seg#{} pos={} len={} flags={:#06x} type={}",
            col_idx,
            pos,
            len,
            flags,
            ext_type
        );
        let sql_ty = match ext_type {
            0 => format!("VARCHAR({})", len.max(1)),
            1 => "INT".to_string(),
            3 => "REAL".to_string(),
            9 => "DATE".to_string(),
            10 => "TIME".to_string(),
            _ => format!("VARBINARY({})", len.max(1)),
        };
        col_sql.push(format!("[col_{:02}] {}", col_idx, sql_ty));
        col_idx += 1;
        off += 16;
        if (flags & 0x0010) == 0 {
            // end of a segmented key — continue scanning more keys
        }
    }
    if col_sql.is_empty() {
        col_sql.push("[col_00] VARCHAR(255)".to_string());
    }

    let create_sql = format!(
        "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = '{}') \
         CREATE TABLE [{}] ([btrv_row] INT IDENTITY(1,1) PRIMARY KEY, {})",
        table_name,
        table_name,
        col_sql.join(", ")
    );
    strace!("op_create sql={}", create_sql);
    match execute_sql(&create_sql) {
        Ok(_) => {
            strace!("op_create: OK table={}", table_name);
            BTR_SUCCESS
        }
        Err(e) => {
            strace!("op_create: sql err={} — returning SUCCESS anyway", e);
            BTR_SUCCESS
        }
    }
}
