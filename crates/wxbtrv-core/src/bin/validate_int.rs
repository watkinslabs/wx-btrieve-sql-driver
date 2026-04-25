/// validate_int — standalone INT file validator
/// Usage: validate_int [dir]
/// Reads every *.INT file in [dir] (default: current dir), parses it with the same
/// logic used by mer_compat.dll, and prints a report of all key fields.
/// Exit code 0 = all parsed, non-zero = at least one parse failure.
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

// ── Minimal type mirrors (must stay in sync with state.rs) ───────────────────
// Layouts are kept identical to the runtime structs for parser compatibility,
// even when the validator itself doesn't read every field.
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct IntField {
    num: u32,
    name: String,
    native_type: i32,
    length: u32,
    offset: u32,
    field_index: Option<u32>,
    default_value: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
struct IntIndex {
    num: u32,
    field_nums: Vec<u32>,
    attrs: Vec<u16>,
    desc: Vec<bool>,
    key_len: u32,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct TableMeta {
    table_name: String,
    schema_name: String,
    db_name: String,
    record_length: u32,
    fields: Vec<IntField>,
    indexes: Vec<IntIndex>,
    recnum_col: String,
    ignore_null_values: bool,
    trim_string_fields: bool,
    translate_oem_to_ansi: bool,
    primary_index: Option<u32>,
}

// ── Parser (verbatim copy of int_meta.rs logic) ───────────────────────────────

fn parse_kv_lines(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(k) = it.next() else { continue };
        let v = it.collect::<Vec<_>>().join(" ");
        if !v.is_empty() {
            out.insert(k.to_ascii_uppercase(), v);
        }
    }
    out
}

fn parse_table_meta_from_text(text: &str) -> Option<TableMeta> {
    let kv = parse_kv_lines(text);
    let table_name = kv.get("TABLE_NAME")?.clone();
    let schema_name = kv.get("SCHEMA_NAME").cloned().unwrap_or_default();
    let db_name = kv.get("DATABASE_SPACE_NAME").cloned().unwrap_or_default();
    let record_length = kv
        .get("LOGICAL_RECORD_LENGTH")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let ignore_null_values = kv
        .get("IGNORE_NULL_VALUES")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let trim_string_fields = kv
        .get("TRIM_STRING_FIELDS")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let translate_oem_to_ansi = kv
        .get("TRANSLATE_OEM_TO_ANSI")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let primary_index: Option<u32> = kv.get("PRIMARY_INDEX").and_then(|s| s.trim().parse().ok());

    // Parse fields
    let mut fields: Vec<IntField> = Vec::new();
    let mut cur_num: Option<u32> = None;
    let mut cur_name = String::new();
    let mut cur_type: i32 = 0;
    let mut cur_len: u32 = 0;
    let mut cur_off: u32 = 0;
    let mut cur_fidx: Option<u32> = None;
    let mut cur_default: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let val: String = match line.find(char::is_whitespace) {
            Some(pos) => line[pos..].trim_start().to_string(),
            None => String::new(),
        };
        match key.to_ascii_uppercase().as_str() {
            "FIELD_NUMBER" => {
                if let Some(n) = cur_num {
                    if !cur_name.is_empty() {
                        fields.push(IntField {
                            num: n,
                            name: cur_name.clone(),
                            native_type: cur_type,
                            length: cur_len,
                            offset: cur_off,
                            field_index: cur_fidx,
                            default_value: cur_default.clone(),
                        });
                    }
                }
                cur_num = val.parse().ok();
                cur_name = String::new();
                cur_type = 0;
                cur_len = 0;
                cur_off = 0;
                cur_fidx = None;
                cur_default = None;
            }
            "FIELD_NAME" => cur_name = val,
            "FIELD_NATIVE_TYPE" => cur_type = val.parse().unwrap_or(0),
            "FIELD_NATIVE_LENGTH" => cur_len = val.parse().unwrap_or(0),
            "FIELD_NATIVE_OFFSET" => cur_off = val.parse().unwrap_or(0),
            "FIELD_INDEX" => cur_fidx = val.parse().ok(),
            "FIELD_DEFAULT_VALUE" => cur_default = if val.is_empty() { None } else { Some(val) },
            _ => {}
        }
    }
    if let Some(n) = cur_num {
        if !cur_name.is_empty() {
            fields.push(IntField {
                num: n,
                name: cur_name,
                native_type: cur_type,
                length: cur_len,
                offset: cur_off,
                field_index: cur_fidx,
                default_value: cur_default,
            });
        }
    }

    let field_map: HashMap<u32, &IntField> = fields.iter().map(|f| (f.num, f)).collect();

    // Parse INDEX_NUMBER blocks
    let mut indexes: Vec<IntIndex> = Vec::new();
    {
        let mut cur_idx_num: Option<u32> = None;
        let mut cur_fnums: Vec<u32> = Vec::new();
        let mut cur_attrs: Vec<u16> = Vec::new();
        let mut cur_descs: Vec<bool> = Vec::new();
        let mut pending_fnum: Option<u32> = None;
        let mut pending_desc: bool = false;

        let flush = |idx_num: u32,
                     fnums: &Vec<u32>,
                     attrs: &Vec<u16>,
                     descs: &Vec<bool>,
                     field_map: &HashMap<u32, &IntField>,
                     indexes: &mut Vec<IntIndex>| {
            if fnums.is_empty() {
                return;
            }
            let key_len: u32 = fnums
                .iter()
                .filter_map(|n| field_map.get(n))
                .map(|f| f.length)
                .sum();
            indexes.push(IntIndex {
                num: idx_num,
                field_nums: fnums.clone(),
                attrs: attrs.clone(),
                desc: descs.clone(),
                key_len,
            });
        };

        for raw in text.lines() {
            let line = raw.trim();
            let mut it = line.split_whitespace();
            let Some(key) = it.next() else { continue };
            let val: String = it.collect::<Vec<_>>().join(" ");
            match key.to_ascii_uppercase().as_str() {
                "INDEX_NUMBER" => {
                    if let Some(n) = cur_idx_num {
                        flush(
                            n,
                            &cur_fnums,
                            &cur_attrs,
                            &cur_descs,
                            &field_map,
                            &mut indexes,
                        );
                    }
                    cur_idx_num = val.parse().ok();
                    cur_fnums = Vec::new();
                    cur_attrs = Vec::new();
                    cur_descs = Vec::new();
                    pending_fnum = None;
                    pending_desc = false;
                }
                "INDEX_SEGMENT_FIELD" => {
                    pending_fnum = val.parse().ok();
                    pending_desc = false;
                }
                "INDEX_SEGMENT_DIRECTION" => {
                    pending_desc = val.trim().eq_ignore_ascii_case("DESCENDING");
                }
                "INDEX_SEGMENT_FLAG" => {
                    let flag: i32 = val.parse().unwrap_or(-1);
                    if let Some(fnum) = pending_fnum.take() {
                        if fnum != 0 && flag != -1 {
                            cur_fnums.push(fnum);
                            cur_attrs.push(flag as u16);
                            cur_descs.push(pending_desc);
                        }
                    }
                    pending_desc = false;
                }
                _ => {}
            }
        }
        if let Some(n) = cur_idx_num {
            flush(
                n,
                &cur_fnums,
                &cur_attrs,
                &cur_descs,
                &field_map,
                &mut indexes,
            );
        }
    }

    // Fallback: build indexes from FIELD_INDEX if no INDEX_NUMBER blocks
    if indexes.is_empty() {
        let mut by_idx: HashMap<u32, Vec<u32>> = HashMap::new();
        for f in &fields {
            if let Some(ix) = f.field_index {
                by_idx.entry(ix).or_default().push(f.num);
            }
        }
        indexes = by_idx
            .into_iter()
            .map(|(ix_num, mut fnums)| {
                fnums.sort_by_key(|n| field_map.get(n).map(|f| f.offset).unwrap_or(0));
                let key_len: u32 = fnums
                    .iter()
                    .filter_map(|n| field_map.get(n))
                    .map(|f| f.length)
                    .sum();
                let n = fnums.len();
                let attrs: Vec<u16> = fnums
                    .iter()
                    .enumerate()
                    .map(|(i, &fnum)| {
                        let native_type = field_map.get(&fnum).map(|f| f.native_type).unwrap_or(0);
                        let mut attr: u16 = 0x0103;
                        if native_type != 0 {
                            attr |= 0x0004;
                        }
                        if i < n - 1 {
                            attr |= 0x0010;
                        }
                        attr
                    })
                    .collect();
                let desc = vec![false; fnums.len()];
                IntIndex {
                    num: ix_num,
                    field_nums: fnums,
                    attrs,
                    desc,
                    key_len,
                }
            })
            .collect();
    }

    indexes.sort_by_key(|ix| ix.num);

    let recnum_col = primary_index
        .and_then(|pi| indexes.get(pi as usize))
        .and_then(|ix| ix.field_nums.first())
        .and_then(|fnum| fields.iter().find(|f| f.num == *fnum))
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "MDS_RECNUM".to_string());

    Some(TableMeta {
        table_name,
        schema_name,
        db_name,
        record_length,
        fields,
        indexes,
        recnum_col,
        ignore_null_values,
        trim_string_fields,
        translate_oem_to_ansi,
        primary_index,
    })
}

// ── Reporting helpers ─────────────────────────────────────────────────────────

fn field_type_name(t: i32) -> &'static str {
    match t {
        0 => "String",
        1 => "Integer",
        2 => "Float",
        3 => "Date",
        4 => "Time",
        7 => "Decimal",
        8 => "Money",
        9 => "Logical",
        11 => "Numeric",
        14 => "AutoInc",
        15 => "BigAutoInc",
        _ => "?",
    }
}

fn desc_str(descs: &[bool]) -> String {
    if descs.is_empty() {
        return "[]".to_string();
    }
    let parts: Vec<&str> = descs
        .iter()
        .map(|&d| if d { "DESC" } else { "ASC" })
        .collect();
    format!("[{}]", parts.join(","))
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let dir = if args.len() >= 2 {
        PathBuf::from(&args[1])
    } else {
        env::current_dir().expect("cannot get cwd")
    };

    let entries: Vec<PathBuf> = {
        let mut v: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read dir {}: {}", dir.display(), e))
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("int"))
                    .unwrap_or(false)
            })
            .collect();
        v.sort();
        v
    };

    if entries.is_empty() {
        eprintln!("No .INT files found in {}", dir.display());
        std::process::exit(1);
    }

    println!(
        "validate_int: scanning {} .INT files in {}\n",
        entries.len(),
        dir.display()
    );
    println!(
        "{:<30} {:<20} {:<8} {:<6} {:<6}  {}",
        "FILE", "TABLE", "RECNUM_COL", "PI", "NFLD", "INDEXES (num:fields:dir)"
    );
    println!("{}", "-".repeat(110));

    let mut failed = 0usize;
    let mut passed = 0usize;
    let mut issues: Vec<String> = Vec::new();

    for path in &entries {
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

        // Try reading as UTF-8; fall back to lossy
        let text = match fs::read(&path) {
            Err(e) => {
                eprintln!("FAIL  {fname}: read error: {e}");
                failed += 1;
                continue;
            }
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        };

        let Some(meta) = parse_table_meta_from_text(&text) else {
            eprintln!("FAIL  {fname}: parse returned None (missing TABLE_NAME?)");
            failed += 1;
            continue;
        };

        // Build index summary string
        let idx_summary: Vec<String> = meta
            .indexes
            .iter()
            .map(|ix| {
                let field_names: Vec<String> = ix
                    .field_nums
                    .iter()
                    .filter_map(|n| meta.fields.iter().find(|f| f.num == *n))
                    .map(|f| f.name.clone())
                    .collect();
                format!(
                    "#{}: {}{} ({}b)",
                    ix.num,
                    field_names.join("+"),
                    desc_str(&ix.desc),
                    ix.key_len
                )
            })
            .collect();

        let pi_str = meta
            .primary_index
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<30} {:<20} {:<18} {:<6} {:<6}  {}",
            fname,
            meta.table_name,
            meta.recnum_col,
            pi_str,
            meta.fields.len(),
            idx_summary.join(" | ")
        );

        // Flag any table where recnum_col is MDS_RECNUM but there IS a primary_index
        // (that would mean primary_index parsing failed)
        if meta.primary_index.is_some() && meta.recnum_col == "MDS_RECNUM" {
            let msg = format!(
                "  !! {fname}: PRIMARY_INDEX={} but recnum_col=MDS_RECNUM — index lookup failed!",
                meta.primary_index.unwrap()
            );
            println!("{msg}");
            issues.push(msg);
        }

        // Flag any index that has a DESCENDING segment but the table would ORDER by it ASC
        // (indicates a potential GetFirst/GetLast bug)
        for ix in &meta.indexes {
            if ix.desc.iter().any(|&d| d) {
                // Only flag if this is the recnum index
                let is_recnum_idx = ix
                    .field_nums
                    .first()
                    .and_then(|n| meta.fields.iter().find(|f| f.num == *n))
                    .map(|f| f.name.eq_ignore_ascii_case(&meta.recnum_col))
                    .unwrap_or(false);
                if is_recnum_idx {
                    let msg = format!(
                        "  >> {fname}: recnum index #{} has DESC segment — GetFirst will ORDER DESC",
                        ix.num
                    );
                    println!("{msg}");
                }
            }
        }

        // Detailed field dump
        println!("       Fields:");
        for f in &meta.fields {
            let default_str = f
                .default_value
                .as_deref()
                .map(|d| format!(" default={d}"))
                .unwrap_or_default();
            let idx_str = f
                .field_index
                .map(|i| format!(" idx={i}"))
                .unwrap_or_default();
            println!(
                "         [{:>2}] {:25} type={:<12} len={:<4} off={:<4}{}{}",
                f.num,
                f.name,
                field_type_name(f.native_type),
                f.length,
                f.offset,
                idx_str,
                default_str
            );
        }
        println!();

        passed += 1;
    }

    println!("{}", "=".repeat(110));
    println!(
        "Result: {passed} OK, {failed} FAILED, {} issues flagged",
        issues.len()
    );
    if !issues.is_empty() {
        println!("\nISSUES:");
        for iss in &issues {
            println!("{iss}");
        }
        std::process::exit(2);
    }
    if failed > 0 {
        std::process::exit(1);
    }
}
