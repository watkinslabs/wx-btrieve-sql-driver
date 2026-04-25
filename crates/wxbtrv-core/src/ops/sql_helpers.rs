//! SQL query builders, key/column helpers, and keyset fetch used by op handlers.

use super::helpers::strace;
use crate::record::pack_row;
use crate::sql::{fetch_one_row, fetch_with};
use crate::sql_param::SqlValue;
use crate::state::{IntField, RuntimeIndex, TableMeta};

pub(super) const STEP_CHUNK_SIZE: usize = 256;

/// Fetch one row that starts with [MDS_RECNUM] followed by all field columns.
/// Returns (recnum, packed_record, raw_field_values).
#[allow(dead_code)]
pub(super) fn fetch_keyset_one(
    meta: &TableMeta,
    sql: &str,
) -> Result<(i64, Vec<u8>, Vec<String>), i32> {
    if meta.fields.is_empty() || meta.record_length == 0 {
        strace!("fetch_keyset_one: no fields for {}", meta.table_name);
        return Err(20);
    }
    // For MDS_RECNUM tables: SELECT [MDS_RECNUM], [F1]..[Fn] → n+1 cols; col0=recnum, cols1..n=fields.
    // For PRIMARY_INDEX tables: SELECT [F1]..[Fn] where F1==recnum_col → n cols; col0=recnum AND field[0].
    fetch_keyset_one_inner(meta, sql, &[])
}

/// Parameterized variant of [`fetch_keyset_one`]. Used by ops on the
/// new bind-parameter pipeline; the dead-code allow goes away as ops
/// migrate over.
#[allow(dead_code)]
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

/// Format a field's raw text value as a SQL literal for WHERE / ORDER BY comparisons.
///
/// Deprecated path — used by ops still on the string-interpolation pipeline.
/// New code should use [`col_to_sql_param`] which returns a typed `SqlValue`
/// for backend-agnostic bind parameters.
pub fn col_to_sql_literal(field: &crate::state::IntField, val: &str) -> String {
    match field.native_type {
        1 | 14 | 15 => val
            .trim()
            .parse::<i64>()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| "0".to_string()),
        2 => val.trim().to_string(), // FLOAT — numeric literal
        // String fields: only trim trailing ASCII spaces (0x20).
        // Preserve everything else including special/control characters from legacy data.
        _ => {
            let trimmed = val.trim_end_matches(' ');
            format!("'{}'", trimmed.replace('\'', "''"))
        }
    }
}

/// Convert a field's raw text value to a typed [`SqlValue`] suitable for
/// binding as a query parameter. Replaces [`col_to_sql_literal`] in the
/// parameterized op pipeline.
///
/// Type mapping mirrors the legacy literal renderer:
///   1 / 14 / 15  (INT, AUTOINC, BFLOAT) → SqlValue::I64
///   2            (FLOAT)                → SqlValue::F64
///   everything else                     → SqlValue::Text (with trailing
///                                          ASCII spaces trimmed, like the
///                                          legacy renderer)
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
        _ => SqlValue::Text(val.trim_end_matches(' ').to_string()),
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

/// Get the SQL col-refs (bracketed) + descending flag for all segments of an index by number.
pub(super) fn index_col_refs(meta: &TableMeta, idx_num: u32) -> Vec<(String, bool)> {
    let Some(idx) = meta.indexes.iter().find(|ix| ix.num == idx_num) else {
        return Vec::new();
    };
    let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
        meta.fields.iter().map(|f| (f.num, f)).collect();
    idx.field_nums
        .iter()
        .zip(idx.desc.iter().copied().chain(std::iter::repeat(false)))
        .filter_map(|(n, d)| {
            field_map
                .get(n)
                .map(|f| (format!("[{}]", f.name.replace(']', "]]")), d))
        })
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
pub fn build_key_where(key_cols: &[(String, String)], cmp: &str) -> String {
    if key_cols.is_empty() {
        return "1=1".to_string();
    }
    if cmp == "=" {
        return key_cols
            .iter()
            .map(|(c, v)| format!("{} = {}", c, v))
            .collect::<Vec<_>>()
            .join(" AND ");
    }
    let strict = if cmp == ">=" || cmp == ">" { ">" } else { "<" };
    let n = key_cols.len();
    (0..n)
        .map(|i| {
            let mut parts: Vec<String> = (0..i)
                .map(|j| format!("{} = {}", key_cols[j].0, key_cols[j].1))
                .collect();
            let this_cmp = if i == n - 1 { cmp } else { strict };
            parts.push(format!("{} {} {}", key_cols[i].0, this_cmp, key_cols[i].1));
            format!("({})", parts.join(" AND "))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Build continuation WHERE predicate for GetNext/Prev.
///
/// The `key_cols` value-string and `last_rn` are both rendered as literals
/// — the legacy pipeline. New code should use
/// [`build_continuation_where_marker`] which expects placeholder markers
/// (e.g. `"?"` or `"$1"`) and a parallel param vec maintained by the caller.
pub fn build_continuation_where(
    key_cols: &[(String, String, bool)],
    dir: i8,
    last_rn: i64,
    recnum_expr: &str,
) -> String {
    build_continuation_where_marker(key_cols, dir, &last_rn.to_string(), recnum_expr)
}

/// Parameterized variant of [`build_continuation_where`]. Each tuple's
/// second element is a backend-specific placeholder marker, and `last_rn`
/// is also a marker. The caller keeps the parallel param vec.
pub fn build_continuation_where_marker(
    key_cols: &[(String, String, bool)],
    dir: i8,
    last_rn_marker: &str,
    recnum_expr: &str,
) -> String {
    let forward = dir >= 0;
    let seg_cmp = |is_desc: bool| -> &'static str {
        if forward ^ is_desc {
            ">"
        } else {
            "<"
        }
    };
    if key_cols.is_empty() {
        let cmp = if forward { ">" } else { "<" };
        return format!("{} {} {}", recnum_expr, cmp, last_rn_marker);
    }
    let n = key_cols.len();
    let mut clauses: Vec<String> = (0..n)
        .map(|i| {
            let mut parts: Vec<String> = (0..i)
                .map(|j| format!("{} = {}", key_cols[j].0, key_cols[j].1))
                .collect();
            let cmp = seg_cmp(key_cols[i].2);
            parts.push(format!("{} {} {}", key_cols[i].0, cmp, key_cols[i].1));
            format!("({})", parts.join(" AND "))
        })
        .collect();
    let eq_all = key_cols
        .iter()
        .map(|(c, v, _)| format!("{} = {}", c, v))
        .collect::<Vec<_>>()
        .join(" AND ");
    let rn_cmp = if forward { ">" } else { "<" };
    clauses.push(format!(
        "({} AND {} {} {})",
        eq_all, recnum_expr, rn_cmp, last_rn_marker
    ));
    clauses.join(" OR ")
}

/// SELECT for Step ops: uses recnum_col for physical ordering.
/// dir=1 → forward (ASC, >), dir=-1 → backward (DESC, <).
///
/// Deprecated path. Use [`build_step_select_n_params`] for the
/// parameterized pipeline. Kept until all callers migrate; a final
/// sweep at the end of 2b removes it along with the other legacy helpers.
#[allow(dead_code)]
pub(super) fn build_step_select_n(
    meta: &TableMeta,
    last_recnum: Option<i64>,
    dir: i8,
    n: usize,
) -> String {
    let rc = meta.recnum_sql_ref();
    let cols = meta.select_with_recnum();
    let tref = meta.table_ref("", "");
    let (ord_dir, cmp) = if dir >= 0 {
        ("ASC", ">")
    } else {
        ("DESC", "<")
    };
    match last_recnum {
        Some(rn) => format!(
            "SELECT TOP {n} {cols} FROM {tref} WHERE {rc} {cmp} {rn} ORDER BY {rc} {ord_dir}",
        ),
        None => format!("SELECT TOP {n} {cols} FROM {tref} ORDER BY {rc} {ord_dir}",),
    }
}

/// Parameterized variant of [`build_step_select_n`]. Returns
/// `(sql, params)` ready to pass to `fetch_with`. Uses the active
/// dialect's TOP/LIMIT shape.
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
    /// Bracketed SQL column reference e.g. "[GLACCT]".
    pub col_ref: String,
    /// SQL comparison operator ("=", ">", ">=", "<", "<=", "<>").
    pub cmp: &'static str,
    /// SQL literal for the right-hand side (escaped / quoted as needed).
    pub literal: String,
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

/// Decode `value_bytes` into a SQL literal using `field`'s type encoding.
/// Mirrors record::unpack_key_fields() but operates on a single raw slice.
pub(super) fn decode_term_literal(field: &IntField, bytes: &[u8]) -> String {
    // Build a padded slice of the field's length so unpack_key_fields-style
    // decoders work. We reuse record::unpack_row by constructing a synthetic
    // one-field record, which guarantees identical encoding semantics.
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
    let out = crate::record::unpack_row(&synth, &record);
    out.into_iter()
        .next()
        .map(|(_, lit)| lit)
        .unwrap_or_else(|| "NULL".to_string())
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
        let literal = decode_term_literal(field, value_bytes);
        let col_ref = format!("[{}]", field.name.replace(']', "]]"));
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
            literal,
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
pub(super) fn build_filter_where(terms: &[TermClause]) -> String {
    if terms.is_empty() {
        return "1=1".to_string();
    }
    let mut or_groups: Vec<Vec<String>> = vec![Vec::new()];
    for t in terms {
        let clause = format!("{} {} {}", t.col_ref, t.cmp, t.literal);
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
        assert_eq!(terms[0].literal, "'ALPHA'");
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
        assert_eq!(terms[0].literal, "42");
    }

    #[test]
    fn build_where_and_or_precedence() {
        let t = |col: &str, cmp: &'static str, lit: &str, c: char| TermClause {
            col_ref: col.into(),
            cmp,
            literal: lit.into(),
            connector: c,
        };
        // a=1 AND b=2 OR c=3 → (a=1 AND b=2) OR c=3
        let terms = vec![
            t("[A]", "=", "1", '&'),
            t("[B]", "=", "2", '|'),
            t("[C]", "=", "3", '.'),
        ];
        assert_eq!(
            build_filter_where(&terms),
            "([A] = 1 AND [B] = 2) OR [C] = 3"
        );
    }

    #[test]
    fn build_where_all_or() {
        let t = |col: &str, lit: &str, c: char| TermClause {
            col_ref: col.into(),
            cmp: "=",
            literal: lit.into(),
            connector: c,
        };
        let terms = vec![t("[A]", "1", '|'), t("[B]", "2", '.')];
        assert_eq!(build_filter_where(&terms), "[A] = 1 OR [B] = 2");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(c, v)| (c.to_string(), v.to_string()))
            .collect()
    }

    /// Build Vec<(col, val, is_desc)> for build_continuation_where tests (all asc).
    fn cols3(pairs: &[(&str, &str)]) -> Vec<(String, String, bool)> {
        pairs
            .iter()
            .map(|(c, v)| (c.to_string(), v.to_string(), false))
            .collect()
    }

    fn col_refs(names: &[&str]) -> Vec<(String, bool)> {
        names.iter().map(|n| (n.to_string(), false)).collect()
    }

    // ── build_key_where ───────────────────────────────────────────────────────

    #[test]
    fn key_where_equal_single() {
        let c = cols(&[("[ID]", "42")]);
        assert_eq!(build_key_where(&c, "="), "[ID] = 42");
    }

    #[test]
    fn key_where_equal_compound() {
        let c = cols(&[("[A]", "'foo'"), ("[B]", "5"), ("[C]", "'bar'")]);
        assert_eq!(
            build_key_where(&c, "="),
            "[A] = 'foo' AND [B] = 5 AND [C] = 'bar'"
        );
    }

    #[test]
    fn key_where_gt_single() {
        let c = cols(&[("[ID]", "10")]);
        assert_eq!(build_key_where(&c, ">"), "([ID] > 10)");
    }

    #[test]
    fn key_where_gt_compound_two() {
        // For [A, B] > [v1, v2]: (A > v1) OR (A = v1 AND B > v2)
        let c = cols(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_key_where(&c, ">");
        assert_eq!(w, "([A] > 'x') OR ([A] = 'x' AND [B] > 5)");
    }

    #[test]
    fn key_where_gte_uses_gte_on_last_segment() {
        let c = cols(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_key_where(&c, ">=");
        assert_eq!(w, "([A] > 'x') OR ([A] = 'x' AND [B] >= 5)");
    }

    #[test]
    fn key_where_lt_compound() {
        let c = cols(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_key_where(&c, "<");
        assert_eq!(w, "([A] < 'x') OR ([A] = 'x' AND [B] < 5)");
    }

    #[test]
    fn key_where_lte_uses_lte_on_last_segment() {
        let c = cols(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_key_where(&c, "<=");
        assert_eq!(w, "([A] < 'x') OR ([A] = 'x' AND [B] <= 5)");
    }

    #[test]
    fn key_where_three_segment_gt() {
        let c = cols(&[("[A]", "1"), ("[B]", "2"), ("[C]", "3")]);
        let w = build_key_where(&c, ">");
        assert_eq!(
            w,
            "([A] > 1) OR ([A] = 1 AND [B] > 2) OR ([A] = 1 AND [B] = 2 AND [C] > 3)"
        );
    }

    // ── build_continuation_where ──────────────────────────────────────────────

    // NOTE: `build_continuation_where` and `build_order_by_cols` take the
    // recnum column as an ALREADY-BRACKETED identifier (matching the production
    // caller in get.rs and friends, which pass `meta.recnum_sql_ref()`). The
    // column name itself is configurable per-table via `RECNUM_COLUMN` in
    // wxbtrv.db — it is NOT hardcoded. These tests use a generic `[pk_col]`
    // placeholder to prove the function is column-name-agnostic. Dedicated
    // back-compat tests at the bottom of this module verify that both our
    // native `btrv_row` and the legacy third-party `MDS_RECNUM` names flow
    // through unchanged.

    #[test]
    fn continuation_empty_keys_uses_recnum_only() {
        let w = build_continuation_where(&[], 1, 99, "[pk_col]");
        assert_eq!(w, "[pk_col] > 99");
    }

    #[test]
    fn continuation_single_key_forward() {
        let c = cols3(&[("[ID]", "10")]);
        let w = build_continuation_where(&c, 1, 99, "[pk_col]");
        assert_eq!(w, "([ID] > 10) OR ([ID] = 10 AND [pk_col] > 99)");
    }

    #[test]
    fn continuation_single_key_backward() {
        let c = cols3(&[("[ID]", "10")]);
        let w = build_continuation_where(&c, -1, 99, "[pk_col]");
        assert_eq!(w, "([ID] < 10) OR ([ID] = 10 AND [pk_col] < 99)");
    }

    #[test]
    fn continuation_compound_forward() {
        let c = cols3(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_continuation_where(&c, 1, 42, "[pk_col]");
        assert_eq!(
            w,
            "([A] > 'x') OR ([A] = 'x' AND [B] > 5) \
             OR ([A] = 'x' AND [B] = 5 AND [pk_col] > 42)"
        );
    }

    #[test]
    fn continuation_compound_backward() {
        let c = cols3(&[("[A]", "'x'"), ("[B]", "5")]);
        let w = build_continuation_where(&c, -1, 42, "[pk_col]");
        assert_eq!(
            w,
            "([A] < 'x') OR ([A] = 'x' AND [B] < 5) \
             OR ([A] = 'x' AND [B] = 5 AND [pk_col] < 42)"
        );
    }

    // ── build_order_by_cols ───────────────────────────────────────────────────

    #[test]
    fn order_by_asc_no_tiebreak() {
        let refs = col_refs(&["[A]", "[B]"]);
        assert_eq!(
            build_order_by_cols(&refs, 1, false, "MDS_RECNUM"),
            "[A] ASC, [B] ASC"
        );
    }

    #[test]
    fn order_by_asc_with_tiebreak() {
        let refs = col_refs(&["[A]", "[B]"]);
        assert_eq!(
            build_order_by_cols(&refs, 1, true, "[pk_col]"),
            "[A] ASC, [B] ASC, [pk_col] ASC"
        );
    }

    #[test]
    fn order_by_desc_with_tiebreak() {
        let refs = col_refs(&["[A]", "[B]"]);
        assert_eq!(
            build_order_by_cols(&refs, -1, true, "[pk_col]"),
            "[A] DESC, [B] DESC, [pk_col] DESC"
        );
    }

    #[test]
    fn order_by_empty_cols_always_recnum() {
        let refs: Vec<(String, bool)> = vec![];
        assert_eq!(
            build_order_by_cols(&refs, 1, false, "[pk_col]"),
            "[pk_col] ASC"
        );
        assert_eq!(
            build_order_by_cols(&refs, -1, false, "[pk_col]"),
            "[pk_col] DESC"
        );
    }

    // ── Back-compat for configurable recnum column names ─────────────────────
    // The Btrieve runtime supports any column as the row identity; the column
    // name is set per-table via `RECNUM_COLUMN` in wxbtrv.db. These tests prove
    // both the legacy third-party name (`MDS_RECNUM`) and our own native name
    // (`btrv_row`) flow through the builder functions unchanged — neither is
    // baked in, both are just strings.

    #[test]
    fn recnum_col_honors_legacy_mds_recnum() {
        let w = build_continuation_where(&[], 1, 99, "[MDS_RECNUM]");
        assert_eq!(w, "[MDS_RECNUM] > 99");
        let refs: Vec<(String, bool)> = vec![];
        let ob = build_order_by_cols(&refs, 1, false, "[MDS_RECNUM]");
        assert_eq!(ob, "[MDS_RECNUM] ASC");
    }

    #[test]
    fn recnum_col_honors_native_btrv_row() {
        let w = build_continuation_where(&[], 1, 99, "[btrv_row]");
        assert_eq!(w, "[btrv_row] > 99");
        let refs: Vec<(String, bool)> = vec![];
        let ob = build_order_by_cols(&refs, 1, false, "[btrv_row]");
        assert_eq!(ob, "[btrv_row] ASC");
    }

    #[test]
    fn recnum_col_honors_arbitrary_name() {
        // Prove it's fully configurable — even an unrelated column name works.
        let w = build_continuation_where(&[], -1, 42, "[my_custom_pk]");
        assert_eq!(w, "[my_custom_pk] < 42");
    }

    // ── col_to_sql_literal ────────────────────────────────────────────────────

    #[test]
    fn literal_integer_type() {
        use crate::state::IntField;
        let f = IntField {
            num: 1,
            name: "x".into(),
            native_type: 1,
            length: 4,
            offset: 0,
            field_index: None,
            default_value: None,
        };
        assert_eq!(col_to_sql_literal(&f, " 42 "), "42");
        assert_eq!(col_to_sql_literal(&f, "bad"), "0");
    }

    #[test]
    fn literal_string_type_escapes_quotes() {
        use crate::state::IntField;
        let f = IntField {
            num: 1,
            name: "x".into(),
            native_type: 0,
            length: 10,
            offset: 0,
            field_index: None,
            default_value: None,
        };
        assert_eq!(col_to_sql_literal(&f, "o'brien"), "'o''brien'");
        // col_to_sql_literal only trims trailing ASCII space — leading characters
        // (including spaces and non-printables like 0x0F) are preserved verbatim
        // because some legacy records carry meaningful leading control bytes.
        assert_eq!(col_to_sql_literal(&f, "  abc  "), "'  abc'");
    }
}

// ── Property tests ───────────────────────────────────────────────────────────
//
// These cover invariants of the WHERE/ORDER BY builder functions across
// randomly-generated inputs. They live in their own module so they can pull in
// proptest only as a dev-dependency.

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::state::IntField;
    use proptest::collection::vec as pvec;
    use proptest::prelude::*;

    fn col_name() -> impl Strategy<Value = String> {
        "[A-Z_]{3,8}".prop_map(|s| format!("[{}]", s))
    }

    fn val_lit() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<i32>().prop_map(|n| n.to_string()),
            "[a-z]{1,5}".prop_map(|s| format!("'{}'", s)),
        ]
    }

    fn col_val_pair() -> impl Strategy<Value = (String, String)> {
        (col_name(), val_lit())
    }

    fn col_val_desc() -> impl Strategy<Value = (String, String, bool)> {
        (col_name(), val_lit(), any::<bool>())
    }

    fn col_ref_pair() -> impl Strategy<Value = (String, bool)> {
        (col_name(), any::<bool>())
    }

    fn dir_strategy() -> impl Strategy<Value = i8> {
        prop_oneof![Just(1i8), Just(-1i8)]
    }

    // ── build_key_where ──────────────────────────────────────────────────

    proptest! {
        #[test]
        fn prop_key_where_eq_is_and_chain(cols in pvec(col_val_pair(), 1..=5)) {
            let w = build_key_where(&cols, "=");
            // result = "c1 = v1 AND c2 = v2 AND ..." with exactly cols.len()-1 " AND " separators.
            let parts: Vec<&str> = w.split(" AND ").collect();
            prop_assert_eq!(parts.len(), cols.len());
            for (part, (c, v)) in parts.iter().zip(cols.iter()) {
                prop_assert_eq!(*part, format!("{} = {}", c, v));
            }
            prop_assert!(!w.contains(" OR "));
        }

        #[test]
        fn prop_key_where_compound_or_groups(
            cols in pvec(col_val_pair(), 1..=5),
            cmp in prop_oneof![Just(">"), Just(">="), Just("<"), Just("<=")],
        ) {
            let w = build_key_where(&cols, cmp);
            // Top-level OR groups: count occurrences of " OR " between top-level
            // parens. Each group is "(...)" so split by ") OR (" and the result
            // length must equal cols.len().
            let n = cols.len();
            // Strip outer parens by splitting on ") OR (" which only ever appears
            // between top-level groups (since the inner clauses use AND, not OR).
            let groups: Vec<&str> = w.split(") OR (").collect();
            prop_assert_eq!(groups.len(), n);

            // Segment i (0-indexed) must have eq-prefix for j<i and the target cmp
            // for the i-th segment. Last segment uses the requested cmp; earlier
            // ones use the strict ">" or "<".
            let strict = if cmp == ">=" || cmp == ">" { ">" } else { "<" };
            for (i, group) in groups.iter().enumerate() {
                // Strip leading "(" from group 0 and trailing ")" from last.
                let g = group.trim_start_matches('(').trim_end_matches(')');
                let parts: Vec<&str> = g.split(" AND ").collect();
                prop_assert_eq!(parts.len(), i + 1);
                // Equality prefix.
                for j in 0..i {
                    prop_assert_eq!(parts[j], &*format!("{} = {}", cols[j].0, cols[j].1));
                }
                let this_cmp = if i == n - 1 { cmp } else { strict };
                prop_assert_eq!(parts[i], &*format!("{} {} {}", cols[i].0, this_cmp, cols[i].1));
            }
        }

        #[test]
        fn prop_key_where_no_doubled_connectors(
            cols in pvec(col_val_pair(), 1..=5),
            cmp in prop_oneof![Just("="), Just(">"), Just(">="), Just("<"), Just("<=")],
        ) {
            let w = build_key_where(&cols, cmp);
            prop_assert!(!w.contains(" OR OR "));
            prop_assert!(!w.contains(" AND AND "));
            prop_assert!(!w.contains(" OR AND "));
            prop_assert!(!w.contains(" AND OR "));
        }

        #[test]
        fn prop_key_where_eq_single_no_parens(cv in col_val_pair()) {
            let w = build_key_where(&[cv.clone()], "=");
            prop_assert_eq!(w, format!("{} = {}", cv.0, cv.1));
        }
    }

    // ── build_continuation_where ─────────────────────────────────────────

    proptest! {
        #[test]
        fn prop_continuation_empty_keys_recnum_only(
            dir in dir_strategy(),
            rn in any::<i64>(),
            rc in "[A-Z_]{3,8}".prop_map(|s| format!("[{}]", s)),
        ) {
            let w = build_continuation_where(&[], dir, rn, &rc);
            let cmp = if dir >= 0 { ">" } else { "<" };
            prop_assert_eq!(w, format!("{} {} {}", rc, cmp, rn));
        }

        #[test]
        fn prop_continuation_single_key_forward(
            cv in col_val_pair(),
            rn in any::<i64>(),
        ) {
            let cols = vec![(cv.0.clone(), cv.1.clone(), false)];
            let w = build_continuation_where(&cols, 1, rn, "[RN]");
            let expected = format!(
                "({} > {}) OR ({} = {} AND [RN] > {})",
                cv.0, cv.1, cv.0, cv.1, rn
            );
            prop_assert_eq!(w, expected);
        }

        #[test]
        fn prop_continuation_single_key_backward(
            cv in col_val_pair(),
            rn in any::<i64>(),
        ) {
            let cols = vec![(cv.0.clone(), cv.1.clone(), false)];
            let w = build_continuation_where(&cols, -1, rn, "[RN]");
            let expected = format!(
                "({} < {}) OR ({} = {} AND [RN] < {})",
                cv.0, cv.1, cv.0, cv.1, rn
            );
            prop_assert_eq!(w, expected);
        }

        #[test]
        fn prop_continuation_group_count_is_n_plus_one(
            cols in pvec(col_val_desc(), 1..=5),
            dir in dir_strategy(),
            rn in any::<i64>(),
        ) {
            let w = build_continuation_where(&cols, dir, rn, "[RN]");
            // Top-level OR groups (each parenthesized) count = cols.len() + 1
            // because we always append the ([eq_all] AND [RN] cmp rn) clause.
            let groups: Vec<&str> = w.split(") OR (").collect();
            prop_assert_eq!(groups.len(), cols.len() + 1);
        }
    }

    // ── build_order_by_cols ──────────────────────────────────────────────

    proptest! {
        #[test]
        fn prop_order_by_asc_all_asc(
            refs in pvec("[A-Z_]{3,8}".prop_map(|s| (format!("[{}]", s), false)), 1..=5),
            tie in any::<bool>(),
        ) {
            let s = build_order_by_cols(&refs, 1, tie, "[RN]");
            // Every comma-separated part must end in " ASC".
            for part in s.split(", ") {
                prop_assert!(part.ends_with(" ASC"), "part did not end in ASC: {:?}", part);
            }
        }

        #[test]
        fn prop_order_by_desc_all_desc(
            refs in pvec("[A-Z_]{3,8}".prop_map(|s| (format!("[{}]", s), false)), 1..=5),
            tie in any::<bool>(),
        ) {
            let s = build_order_by_cols(&refs, -1, tie, "[RN]");
            for part in s.split(", ") {
                prop_assert!(part.ends_with(" DESC"), "part did not end in DESC: {:?}", part);
            }
        }

        #[test]
        fn prop_order_by_dir_flip_swaps_asc_desc(
            refs in pvec(col_ref_pair(), 1..=5),
            tie in any::<bool>(),
        ) {
            let f = build_order_by_cols(&refs, 1, tie, "[RN]");
            let b = build_order_by_cols(&refs, -1, tie, "[RN]");
            // Swap ASC<->DESC in `f` (use placeholder to avoid double-swap).
            let swapped = f.replace(" ASC", " \x01").replace(" DESC", " ASC").replace(" \x01", " DESC");
            prop_assert_eq!(swapped, b);
        }

        #[test]
        fn prop_order_by_tiebreak_appends_recnum(
            refs in pvec(col_ref_pair(), 1..=5),
            dir in dir_strategy(),
        ) {
            let with_tb = build_order_by_cols(&refs, dir, true, "[RN]");
            let without_tb = build_order_by_cols(&refs, dir, false, "[RN]");
            let parts_with: Vec<&str> = with_tb.split(", ").collect();
            let parts_without: Vec<&str> = without_tb.split(", ").collect();
            prop_assert_eq!(parts_with.len(), refs.len() + 1);
            prop_assert_eq!(parts_without.len(), refs.len());
            // Last part of `with_tb` is the recnum.
            prop_assert!(parts_with.last().unwrap().starts_with("[RN] "));
        }

        #[test]
        fn prop_order_by_empty_always_appends_recnum(dir in dir_strategy(), tie in any::<bool>()) {
            let s = build_order_by_cols(&[], dir, tie, "[RN]");
            let d = if dir >= 0 { "ASC" } else { "DESC" };
            prop_assert_eq!(s, format!("[RN] {}", d));
        }
    }

    // ── col_to_sql_literal ───────────────────────────────────────────────

    fn int_field() -> IntField {
        IntField {
            num: 1,
            name: "F".into(),
            native_type: 1,
            length: 4,
            offset: 0,
            field_index: None,
            default_value: None,
        }
    }

    fn string_field() -> IntField {
        IntField {
            num: 1,
            name: "F".into(),
            native_type: 0,
            length: 16,
            offset: 0,
            field_index: None,
            default_value: None,
        }
    }

    proptest! {
        #[test]
        fn prop_int_literal_is_decimal_or_zero(n in any::<i64>()) {
            let f = int_field();
            let s = col_to_sql_literal(&f, &n.to_string());
            prop_assert_eq!(s, n.to_string());
        }

        #[test]
        fn prop_int_literal_garbage_yields_zero(g in "[a-z]{1,8}") {
            let f = int_field();
            let s = col_to_sql_literal(&f, &g);
            prop_assert_eq!(s, "0");
        }

        #[test]
        fn prop_string_literal_always_quoted(s in "[\\x20-\\x7e]{0,16}") {
            let f = string_field();
            let lit = col_to_sql_literal(&f, &s);
            prop_assert!(lit.starts_with('\''));
            prop_assert!(lit.ends_with('\''));
        }

        #[test]
        fn prop_string_literal_quote_balance(s in "[\\x20-\\x7e]{0,16}") {
            let f = string_field();
            let lit = col_to_sql_literal(&f, &s);
            // Must be wrapped in single quotes; strip them and verify any
            // single quote inside is properly doubled (SQL escape).
            prop_assert!(lit.starts_with('\''));
            prop_assert!(lit.ends_with('\''));
            prop_assert!(lit.len() >= 2);
            let interior = &lit[1..lit.len() - 1];
            let bytes = interior.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    prop_assert!(
                        i + 1 < bytes.len() && bytes[i + 1] == b'\'',
                        "unescaped quote at byte {} in literal {:?}", i, lit
                    );
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
    }
}
