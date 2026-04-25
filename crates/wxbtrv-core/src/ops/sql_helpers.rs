//! SQL query builders, key/column helpers, and keyset fetch used by op handlers.

use super::helpers::strace;
use crate::record::pack_row;
use crate::sql::{fetch_one_row, fetch_with};
use crate::sql_param::SqlValue;
use crate::state::{IntField, RuntimeIndex, TableMeta};

pub(super) const STEP_CHUNK_SIZE: usize = 256;

/// Fetch one row that starts with [MDS_RECNUM] followed by all field columns
/// using bound `params` for the SQL placeholders. Returns
/// `(recnum, packed_record, raw_field_values)`.
///
/// For MDS_RECNUM tables: `SELECT [MDS_RECNUM], [F1]..[Fn]` → n+1 cols
///   (col0=recnum, cols1..n=fields).
/// For PRIMARY_INDEX tables: `SELECT [F1]..[Fn]` where F1==recnum_col → n cols
///   (col0 is both the recnum and field[0]).
pub(super) fn fetch_keyset_one_with(
    meta: &TableMeta,
    sql: &str,
    params: &[SqlValue],
) -> Result<(i64, Vec<u8>, Vec<String>), i32> {
    if meta.fields.is_empty() || meta.record_length == 0 {
        strace!("fetch_keyset_one: no fields for {}", meta.table_name);
        return Err(20);
    }
    fetch_keyset_one_inner(meta, sql, params)
}

fn fetch_keyset_one_inner(
    meta: &TableMeta,
    sql: &str,
    params: &[SqlValue],
) -> Result<(i64, Vec<u8>, Vec<String>), i32> {
    let recnum_is_field = meta
        .fields
        .iter()
        .any(|f| f.name.eq_ignore_ascii_case(&meta.recnum_col));
    let n_cols = if recnum_is_field {
        meta.fields.len()
    } else {
        1 + meta.fields.len()
    };
    let row = if params.is_empty() {
        fetch_one_row(sql, n_cols).inspect_err(|e| {
            strace!("fetch_keyset_one err={} table={}", e, meta.table_name);
        })?
    } else {
        let mut rows = fetch_with(sql, params, n_cols, 1).inspect_err(|e| {
            strace!("fetch_keyset_one err={} table={}", e, meta.table_name);
        })?;
        rows.pop().ok_or(4)?
    };
    let recnum = row[0].trim().parse::<i64>().unwrap_or(0);
    let fields: Vec<String> = if recnum_is_field {
        // row[0] is both the recnum value and field[0]'s value; row[1..] are fields[1..].
        let mut f = vec![row[0].clone()];
        f.extend_from_slice(&row[1..]);
        f
    } else {
        row[1..].to_vec()
    };
    // For wide tables, log a sample of field values so we can verify ODBC is returning data.
    if meta.fields.len() > 50 {
        let samples: Vec<String> = meta
            .fields
            .iter()
            .zip(fields.iter())
            .filter(|(_f, v)| !v.is_empty() && v.as_str() != "0" && !v.trim().is_empty())
            .take(10)
            .map(|(f, v)| format!("{}={}", f.name, v))
            .collect();
        strace!(
            "fetch_keyset_one table={} recnum={} sample_fields=[{}]",
            meta.table_name,
            recnum,
            samples.join(", ")
        );
        // Also log every integer-type field explicitly so counter values (ARSO_NUM etc.) are visible.
        for (f, v) in meta.fields.iter().zip(fields.iter()) {
            if (f.native_type == 1 || f.native_type == 14 || f.native_type == 15)
                && v.trim().parse::<i64>().unwrap_or(0) != 0
            {
                strace!(
                    "fetch_keyset_one table={} int_field {}={}",
                    meta.table_name,
                    f.name,
                    v
                );
            }
        }
    }
    let packed = pack_row(&meta.fields, &fields, meta.record_length);

    // Log SQL field values and packed bytes so we can compare against reference
    let field_dump: String = meta
        .fields
        .iter()
        .zip(fields.iter())
        .map(|(f, v)| format!("{}={:?}", f.name, v))
        .collect::<Vec<_>>()
        .join(" ");
    let hex_show = packed.len().min(32);
    let hex: String = packed[..hex_show]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");
    let hex_tail = if packed.len() > 32 {
        format!("..+{}b", packed.len() - 32)
    } else {
        String::new()
    };
    strace!(
        "fetch_keyset_one table={} recnum={} fields=[{}]",
        meta.table_name,
        recnum,
        field_dump
    );
    strace!(
        "fetch_keyset_one packed len={} [{}{}]",
        packed.len(),
        hex,
        hex_tail
    );

    Ok((recnum, packed, fields))
}

/// Convert a field's raw text value to a typed [`SqlValue`] suitable for
/// binding as a query parameter.
///
/// Type mapping:
///   1 / 14 / 15  (INT, AUTOINC, BFLOAT) → SqlValue::I64
///   2            (FLOAT)                → SqlValue::F64
///   everything else                     → SqlValue::Text (verbatim;
///                                          no trim, since SQLite TEXT
///                                          comparison is exact and
///                                          MSSQL CHAR pads on either
///                                          side of the comparison)
pub fn col_to_sql_param(field: &crate::state::IntField, val: &str) -> SqlValue {
    match field.native_type {
        1 | 14 | 15 => {
            let n = val.trim().parse::<i64>().unwrap_or(0);
            SqlValue::I64(n)
        }
        2 => {
            let f = val.trim().parse::<f64>().unwrap_or(0.0);
            SqlValue::F64(f)
        }
        _ => SqlValue::Text(val.to_string()),
    }
}

/// Append a value to `params` and return its placeholder string for the
/// active dialect. Convenience used by the parameterized op pipeline.
pub fn push_param(params: &mut Vec<SqlValue>, value: SqlValue) -> String {
    let dialect = crate::dialect::active();
    params.push(value);
    dialect.param_marker(params.len())
}

/// Pick an index by key_num. No fallback — returns None if no match.
pub(super) fn pick_index(
    meta: &TableMeta,
    key_num: i16,
    current: Option<u32>,
) -> Option<&RuntimeIndex> {
    if key_num == -1 {
        current.and_then(|n| meta.indexes.iter().find(|ix| ix.num == n))
    } else {
        let raw = key_num as u16;
        let high = (raw >> 8) as u32;
        let lower = (raw & 0xFF) as u32;

        if high == 0 {
            meta.indexes
                .iter()
                .find(|ix| ix.num == lower.wrapping_add(1))
        } else {
            meta.index_for_key_len(lower as usize)
        }
    }
}

/// Get the dialect-quoted col-refs + descending flag for all segments of an index by number.
pub(super) fn index_col_refs(meta: &TableMeta, idx_num: u32) -> Vec<(String, bool)> {
    let Some(idx) = meta.indexes.iter().find(|ix| ix.num == idx_num) else {
        return Vec::new();
    };
    let dialect = crate::dialect::active();
    let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
        meta.fields.iter().map(|f| (f.num, f)).collect();
    idx.field_nums
        .iter()
        .zip(idx.desc.iter().copied().chain(std::iter::repeat(false)))
        .filter_map(|(n, d)| field_map.get(n).map(|f| (dialect.quote_ident(&f.name), d)))
        .collect()
}

/// Extract SQL literals for all segments of `idx_num` from a fetched row's field values.
pub(super) fn extract_key_vals(
    meta: &TableMeta,
    idx_num: u32,
    field_vals: &[String],
) -> Vec<SqlValue> {
    let Some(idx) = meta.indexes.iter().find(|ix| ix.num == idx_num) else {
        return Vec::new();
    };
    let field_pos: std::collections::HashMap<u32, usize> = meta
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.num, i))
        .collect();
    idx.field_nums
        .iter()
        .filter_map(|&fnum| {
            let fi = *field_pos.get(&fnum)?;
            Some(col_to_sql_param(
                meta.fields.get(fi)?,
                field_vals.get(fi)?,
            ))
        })
        .collect()
}

/// Build an ORDER BY clause for key columns.
pub fn build_order_by_cols(
    col_refs: &[(String, bool)],
    dir: i8,
    tie_break: bool,
    recnum_expr: &str,
) -> String {
    let forward = dir >= 0;
    let mut parts: Vec<String> = col_refs
        .iter()
        .map(|(c, seg_desc)| {
            let sql_asc = if *seg_desc { !forward } else { forward };
            format!("{} {}", c, if sql_asc { "ASC" } else { "DESC" })
        })
        .collect();
    if parts.is_empty() || tie_break {
        let d = if forward { "ASC" } else { "DESC" };
        parts.push(format!("{} {}", recnum_expr, d));
    }
    parts.join(", ")
}

/// Build a compound WHERE predicate for an initial key search.
///
/// `key_cols` is `(col_ref, value)` pairs; values are pushed into `params`
/// each time they're referenced in the predicate (segments appear in
/// multiple OR clauses for inequalities). Returns the SQL fragment.
pub fn build_key_where(
    key_cols: &[(String, SqlValue)],
    cmp: &str,
    params: &mut Vec<SqlValue>,
) -> String {
    if key_cols.is_empty() {
        return "1=1".to_string();
    }
    let dialect = crate::dialect::active();
    let push = |params: &mut Vec<SqlValue>, v: &SqlValue| -> String {
        params.push(v.clone());
        dialect.param_marker(params.len())
    };
    if cmp == "=" {
        return key_cols
            .iter()
            .map(|(c, v)| {
                let m = push(params, v);
                format!("{} = {}", c, m)
            })
            .collect::<Vec<_>>()
            .join(" AND ");
    }
    let strict = if cmp == ">=" || cmp == ">" { ">" } else { "<" };
    let n = key_cols.len();
    (0..n)
        .map(|i| {
            let mut parts: Vec<String> = Vec::with_capacity(i + 1);
            for kc in &key_cols[..i] {
                let m = push(params, &kc.1);
                parts.push(format!("{} = {}", kc.0, m));
            }
            let this_cmp = if i == n - 1 { cmp } else { strict };
            let m = push(params, &key_cols[i].1);
            parts.push(format!("{} {} {}", key_cols[i].0, this_cmp, m));
            format!("({})", parts.join(" AND "))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Build a continuation WHERE predicate for Get Next / Get Prev.
///
/// Pushes each value once per occurrence in the predicate (each segment
/// appears N+1-i times across the i'th OR clause and the final eq_all
/// clause). `last_rn` is pushed once.
pub fn build_continuation_where_marker(
    key_cols: &[(String, SqlValue, bool)],
    dir: i8,
    last_rn: i64,
    recnum_expr: &str,
    params: &mut Vec<SqlValue>,
) -> String {
    let dialect = crate::dialect::active();
    let forward = dir >= 0;
    let seg_cmp = |is_desc: bool| -> &'static str {
        if forward ^ is_desc {
            ">"
        } else {
            "<"
        }
    };
    let push = |params: &mut Vec<SqlValue>, v: &SqlValue| -> String {
        params.push(v.clone());
        dialect.param_marker(params.len())
    };
    if key_cols.is_empty() {
        let cmp = if forward { ">" } else { "<" };
        let m = push(params, &SqlValue::I64(last_rn));
        return format!("{} {} {}", recnum_expr, cmp, m);
    }
    let n = key_cols.len();
    let mut clauses: Vec<String> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let mut parts: Vec<String> = Vec::with_capacity(i + 1);
        for kc in &key_cols[..i] {
            let m = push(params, &kc.1);
            parts.push(format!("{} = {}", kc.0, m));
        }
        let cmp = seg_cmp(key_cols[i].2);
        let m = push(params, &key_cols[i].1);
        parts.push(format!("{} {} {}", key_cols[i].0, cmp, m));
        clauses.push(format!("({})", parts.join(" AND ")));
    }
    let mut eq_parts: Vec<String> = Vec::with_capacity(n + 1);
    for kc in key_cols {
        let m = push(params, &kc.1);
        eq_parts.push(format!("{} = {}", kc.0, m));
    }
    let rn_cmp = if forward { ">" } else { "<" };
    let rn_marker = push(params, &SqlValue::I64(last_rn));
    eq_parts.push(format!("{} {} {}", recnum_expr, rn_cmp, rn_marker));
    clauses.push(format!("({})", eq_parts.join(" AND ")));
    clauses.join(" OR ")
}

/// SELECT for Step ops: uses recnum_col for physical ordering.
/// dir=1 → forward (ASC, >), dir=-1 → backward (DESC, <).
/// Returns `(sql, params)` ready to pass to `fetch_with`. Uses the
/// active dialect's TOP/LIMIT shape.
pub(super) fn build_step_select_n_params(
    meta: &TableMeta,
    last_recnum: Option<i64>,
    dir: i8,
    n: usize,
) -> (String, Vec<SqlValue>) {
    let dialect = crate::dialect::active();
    let rc = meta.recnum_sql_ref();
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let (ord_dir, cmp) = if dir >= 0 {
        ("ASC", ">")
    } else {
        ("DESC", "<")
    };
    let order_by = format!("{rc} {ord_dir}");
    match last_recnum {
        Some(rn) => {
            let where_clause =
                format!("{rc} {cmp} {}", dialect.param_marker(1));
            let sql = crate::dialect::select_with_limit(
                dialect, n as u32, &cols, &tref, &where_clause, &order_by,
            );
            (sql, vec![SqlValue::I64(rn)])
        }
        None => {
            let sql = crate::dialect::select_with_limit(
                dialect, n as u32, &cols, &tref, "", &order_by,
            );
            (sql, vec![])
        }
    }
}

// ── Extended Get/Step filter term parsing ────────────────────────────────────

/// One filter term parsed out of a Get/Step Next/Prev Extended descriptor.
#[derive(Debug, Clone)]
pub struct TermClause {
    /// Dialect-quoted SQL column reference (e.g. `[GLACCT]` for MSSQL or
    /// `"GLACCT"` for Postgres / SQLite).
    pub col_ref: String,
    /// SQL comparison operator ("=", ">", ">=", "<", "<=", "<>").
    pub cmp: &'static str,
    /// Typed right-hand-side value, bound as a SqlValue parameter when the
    /// WHERE fragment is rendered.
    pub value: SqlValue,
    /// Connector to the next term: '&' for AND, '|' for OR, '.' for last term.
    pub connector: char,
}

/// Convert a comparison code from a TERM_HEADER into a SQL operator.
/// Codes: 0=EQ, 1=GT, 2=GE, 3=LT, 4=LE, 5=NE (1998 spec).
fn cmp_for_code(code: u8) -> Option<&'static str> {
    match code {
        0 => Some("="),
        1 => Some(">"),
        2 => Some(">="),
        3 => Some("<"),
        4 => Some("<="),
        5 => Some("<>"),
        _ => None,
    }
}

/// Find the field that lives at the given record byte offset. Allows a term
/// that references a tail sub-field (a byte range inside a larger field) to
/// still match on offset equality — we only need the outer field's meta for
/// type encoding so the common case works.
fn field_at_offset(meta: &TableMeta, offset: u16) -> Option<&IntField> {
    // Exact match first.
    if let Some(f) = meta.fields.iter().find(|f| f.offset as u16 == offset) {
        return Some(f);
    }
    // Otherwise: whichever field covers this offset.
    meta.fields
        .iter()
        .find(|f| offset as u32 >= f.offset && (offset as u32) < f.offset + f.length)
}

/// Decode `value_bytes` into a typed `SqlValue` using `field`'s type
/// encoding. Mirrors record::unpack_key_fields() but operates on a single
/// raw slice.
pub(super) fn decode_term_value(field: &IntField, bytes: &[u8]) -> SqlValue {
    // Reuse the typed record decoder by constructing a synthetic one-field
    // record padded to the field's length — same encoding semantics as
    // a real record column.
    let mut record = vec![0u8; field.length as usize];
    let n = bytes.len().min(record.len());
    record[..n].copy_from_slice(&bytes[..n]);
    let synth = [IntField {
        num: field.num,
        name: field.name.clone(),
        native_type: field.native_type,
        length: field.length,
        offset: 0,
        field_index: field.field_index,
        default_value: field.default_value.clone(),
    }];
    crate::record::unpack_row_typed(&synth, &record)
        .into_iter()
        .next()
        .map(|(_, v)| v)
        .unwrap_or(SqlValue::Null)
}

/// Parse filter terms from a GNE / SNE descriptor. `desc` is the full input
/// buffer; `start` is the byte offset where the first TERM_HEADER lives (8
/// for the pre-GNE header). Returns the parsed terms and the byte offset
/// immediately after the last term — suitable for the caller to continue
/// parsing the RETRIEVAL_HEADER.
///
/// Returns None on malformed input (truncated buffer, unknown cmp code, or
/// a term that refers to a byte offset with no matching field).
pub fn parse_filter_terms(
    meta: &TableMeta,
    desc: &[u8],
    start: usize,
    n_terms: usize,
) -> Option<(Vec<TermClause>, usize)> {
    let mut off = start;
    let mut terms = Vec::with_capacity(n_terms);
    for i in 0..n_terms {
        if off + 7 > desc.len() {
            return None;
        }
        let _field_type = desc[off];
        let field_len = u16::from_le_bytes([desc[off + 1], desc[off + 2]]) as usize;
        let field_off = u16::from_le_bytes([desc[off + 3], desc[off + 4]]);
        let cmp_code = desc[off + 5];
        let connector_byte = desc[off + 6];
        let value_start = off + 7;
        if value_start + field_len > desc.len() {
            return None;
        }
        let value_bytes = &desc[value_start..value_start + field_len];
        let cmp = cmp_for_code(cmp_code)?;
        let field = field_at_offset(meta, field_off)?;
        // If the term's fieldLen is shorter than the whole field, we treat
        // it as the leading prefix of that field (standard Btrieve semantics
        // for comparing fixed-length strings/ints with truncated values).
        let value = decode_term_value(field, value_bytes);
        let col_ref = crate::dialect::active().quote_ident(&field.name);
        // Connector: 1=AND, 2=OR, 0=end. For the last term (i == n_terms-1)
        // always emit '.' regardless, so the caller stops the chain.
        let connector = if i + 1 == n_terms {
            '.'
        } else {
            match connector_byte {
                1 => '&',
                2 => '|',
                _ => '.',
            }
        };
        terms.push(TermClause {
            col_ref,
            cmp,
            value,
            connector,
        });
        off = value_start + field_len;
    }
    Some((terms, off))
}

/// Build a SQL WHERE fragment from a list of filter terms.
/// Groups consecutive AND-connected terms into parenthesized chunks so that
/// OR connectors bind with correct precedence (AND tighter than OR, matching
/// SQL and Btrieve spec).
/// Render the parsed filter terms as a SQL WHERE fragment, appending each
/// term's typed value to `params`. Placeholder markers are numbered off
/// `params.len()` so the fragment composes correctly with any keyset
/// continuation params the caller has already pushed (Postgres uses `$N`
/// where N is the absolute position in the final argument list).
pub(super) fn build_filter_where(terms: &[TermClause], params: &mut Vec<SqlValue>) -> String {
    if terms.is_empty() {
        return "1=1".to_string();
    }
    let dialect = crate::dialect::active();
    let mut or_groups: Vec<Vec<String>> = vec![Vec::new()];
    for t in terms {
        params.push(t.value.clone());
        let marker = dialect.param_marker(params.len());
        let clause = format!("{} {} {}", t.col_ref, t.cmp, marker);
        or_groups.last_mut().unwrap().push(clause);
        // '&' or '.' stays in the same AND group; '|' starts a new OR.
        if t.connector == '|' {
            or_groups.push(Vec::new());
        }
    }
    let parts: Vec<String> = or_groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .map(|g| {
            if g.len() == 1 {
                g.into_iter().next().unwrap()
            } else {
                format!("({})", g.join(" AND "))
            }
        })
        .collect();
    parts.join(" OR ")
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use crate::state::IntField;

    fn make_meta() -> TableMeta {
        TableMeta {
            table_name: "T".into(),
            schema_name: "dbo".into(),
            db_name: "D".into(),
            record_length: 14,
            page_size: 4096,
            file_flags: 0,
            fields: vec![
                IntField {
                    num: 1,
                    name: "NAME".into(),
                    native_type: 0,
                    length: 10,
                    offset: 0,
                    field_index: None,
                    default_value: None,
                },
                IntField {
                    num: 2,
                    name: "ID".into(),
                    native_type: 1,
                    length: 4,
                    offset: 10,
                    field_index: None,
                    default_value: None,
                },
            ],
            indexes: vec![],
            recnum_col: "MDS_RECNUM".into(),
            ignore_null_values: true,
            trim_string_fields: true,
            translate_oem_to_ansi: false,
            primary_index: None,
            local_cache: false,
        }
    }

    #[test]
    fn parse_single_string_eq_term() {
        // header at offset 0: desc_len=0 currency=0 reject=0 nterms=1
        let mut buf = vec![0u8; 8];
        buf[6] = 1; // nterms
                    // term: fieldType=0 fieldLen=10 fieldOff=0 cmp=0(eq) conn=0
        buf.push(0);
        buf.extend_from_slice(&10u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.push(0);
        buf.push(0);
        buf.extend_from_slice(b"ALPHA     ");
        let meta = make_meta();
        let (terms, _off) = parse_filter_terms(&meta, &buf, 8, 1).unwrap();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].col_ref, "[NAME]");
        assert_eq!(terms[0].cmp, "=");
        assert_eq!(terms[0].value, SqlValue::Text("ALPHA     ".into()));
    }

    #[test]
    fn parse_int_gt_term() {
        let mut buf = vec![0u8; 8];
        buf[6] = 1;
        buf.push(1); // type
        buf.extend_from_slice(&4u16.to_le_bytes()); // len
        buf.extend_from_slice(&10u16.to_le_bytes()); // off
        buf.push(1); // cmp > (GT)
        buf.push(0); // connector end
        buf.extend_from_slice(&42i32.to_le_bytes());
        let meta = make_meta();
        let (terms, _) = parse_filter_terms(&meta, &buf, 8, 1).unwrap();
        assert_eq!(terms[0].col_ref, "[ID]");
        assert_eq!(terms[0].cmp, ">");
        assert_eq!(terms[0].value, SqlValue::I64(42));
    }

    #[test]
    fn build_where_and_or_precedence() {
        let t = |col: &str, cmp: &'static str, val: SqlValue, c: char| TermClause {
            col_ref: col.into(),
            cmp,
            value: val,
            connector: c,
        };
        // a=1 AND b=2 OR c=3 → (a=1 AND b=2) OR c=3
        let terms = vec![
            t("[A]", "=", SqlValue::I64(1), '&'),
            t("[B]", "=", SqlValue::I64(2), '|'),
            t("[C]", "=", SqlValue::I64(3), '.'),
        ];
        let mut params = Vec::new();
        // MSSQL dialect (positional `?`) is the workspace default.
        assert_eq!(
            build_filter_where(&terms, &mut params),
            "([A] = ? AND [B] = ?) OR [C] = ?"
        );
        assert_eq!(
            params,
            vec![SqlValue::I64(1), SqlValue::I64(2), SqlValue::I64(3)]
        );
    }

    #[test]
    fn build_where_all_or() {
        let t = |col: &str, val: SqlValue, c: char| TermClause {
            col_ref: col.into(),
            cmp: "=",
            value: val,
            connector: c,
        };
        let terms = vec![t("[A]", SqlValue::I64(1), '|'), t("[B]", SqlValue::I64(2), '.')];
        let mut params = Vec::new();
        assert_eq!(
            build_filter_where(&terms, &mut params),
            "[A] = ? OR [B] = ?"
        );
        assert_eq!(params, vec![SqlValue::I64(1), SqlValue::I64(2)]);
    }
}

