use crate::{IndexSegment, IntField, IntFile, IntIndex};
/// bfile.rs — Btrieve FCR schema extraction (shared between db_config and btr-import).
///
/// Parses the File Control Record of a Btrieve .B file to extract table structure:
/// record length, page size, key definitions (with offsets, lengths, types, null values).
///
/// Does NOT read data records — each tool has its own record iterator.
///
/// Reference: mbbsemu/wbtrv32 open-source Btrieve emulator.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

// ── Key attribute flags ────────────────────────────────────────────────────────
const USE_EXT_TYPE: u16 = 1 << 8;
const SEGMENTED_KEY: u16 = 1 << 4;
const OLD_BIN: u16 = 1 << 2;
const DESCENDING: u16 = 1 << 6;
const DUPLICATES: u16 = 1 << 0;

#[derive(Debug, Clone, PartialEq)]
pub enum RecordKind {
    Fixed,
    Variable,
    VariableTruncated,
}

#[derive(Debug, Clone)]
pub struct BtrieveSchema {
    pub is_v6: bool,
    pub record_length: u16,
    pub physical_rec_length: u16,
    pub page_size: u16,
    pub page_count: u32,
    pub record_count: u32,
    pub file_flags: u16,
    pub record_kind: RecordKind,
    pub keys: Vec<BtrieveKey>,
}

#[derive(Debug, Clone)]
pub struct BtrieveKey {
    pub number: u16,
    pub segments: Vec<BtrieveSeg>,
}

#[derive(Debug, Clone)]
pub struct BtrieveSeg {
    pub offset: u16,
    pub length: u16,
    pub data_type: u8,
    pub attributes: u16,
    pub null_value: u8,
    pub descending: bool,
    pub allows_dups: bool,
}

// ── helpers ────────────────────────────────────────────────────────────────────

fn u16le(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() {
        return 0;
    }
    u16::from_le_bytes([buf[off], buf[off + 1]])
}
fn u32_hi_lo(buf: &[u8], hi_off: usize, lo_off: usize) -> u32 {
    (u16le(buf, hi_off) as u32) << 16 | (u16le(buf, lo_off) as u32)
}
fn u32le(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}
fn read_at(f: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), String> {
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    f.read_exact(buf).map_err(|e| e.to_string())
}

// ── FCR parsing ────────────────────────────────────────────────────────────────

/// Parse the File Control Record of a Btrieve .B file and return its schema.
pub fn parse_schema(path: &Path) -> Result<BtrieveSchema, String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut head = [0u8; 512];
    read_at(&mut f, 0, &mut head)?;

    let is_v6 = head[0] == b'F' && head[1] == b'C' && head[2] == 0 && head[3] == 0;

    if !is_v6 {
        let vc = (head[6] as u16) << 8 | head[7] as u16;
        if !matches!(vc, 3 | 4 | 5) {
            return Err(format!(
                "not a recognised Btrieve file (v6 magic absent, v5 version code {vc:#x})"
            ));
        }
    }

    let page_size = u16le(&head, 0x08);
    if page_size < 512 || (page_size & 0x1FF) != 0 {
        return Err(format!(
            "invalid page size {page_size} (must be multiple of 512)"
        ));
    }
    let ps = page_size as usize;

    let (fcr, fcr_file_offset): (Vec<u8>, u64) = if is_v6 {
        let mut p0 = vec![0u8; ps];
        let mut p1 = vec![0u8; ps];
        read_at(&mut f, 0, &mut p0)?;
        read_at(&mut f, ps as u64, &mut p1)?;
        let uc0 = u32le(&p0, 4);
        let uc1 = u32le(&p1, 4);
        if uc0 >= uc1 {
            (p0, 0)
        } else {
            (p1, ps as u64)
        }
    } else {
        let mut p = vec![0u8; ps.max(512)];
        read_at(&mut f, 0, &mut p)?;
        (p, 0)
    };

    let record_length = u16le(&fcr, 0x16);
    let physical_rec_length = u16le(&fcr, 0x18);
    let record_count = u32_hi_lo(&fcr, 0x1A, 0x1C);
    let page_count = u32_hi_lo(&fcr, 0x26, 0x28);
    let key_count = u16le(&fcr, 0x14) as usize;
    let file_flags = if 0x106 + 1 < fcr.len() {
        u16le(&fcr, 0x106)
    } else {
        0
    };
    let var_flags = if 0x38 < fcr.len() { fcr[0x38] } else { 0 };

    let record_kind = if var_flags != 0 || (file_flags & 0x1) != 0 {
        if var_flags == 0xFD || (file_flags & 0x2) != 0 {
            RecordKind::VariableTruncated
        } else {
            RecordKind::Variable
        }
    } else {
        RecordKind::Fixed
    };

    let keys = if key_count > 0 {
        parse_key_defs(&mut f, &fcr, is_v6, key_count, fcr_file_offset)?
    } else {
        Vec::new()
    };

    Ok(BtrieveSchema {
        is_v6,
        record_length,
        physical_rec_length,
        page_size,
        page_count,
        record_count,
        file_flags,
        record_kind,
        keys,
    })
}

fn parse_key_defs(
    f: &mut File,
    fcr: &[u8],
    is_v6: bool,
    key_count: usize,
    fcr_offset: u64,
) -> Result<Vec<BtrieveKey>, String> {
    const KD_LEN: u64 = 0x1E;

    let key_starts: Vec<u64> = if is_v6 {
        if fcr.len() < 0x7A {
            return Err("FCR too short for KAT offset".into());
        }
        let kat_rel = fcr[0x78] as u64 | (fcr[0x79] as u64) << 8;
        let kat_abs = fcr_offset + kat_rel;
        let mut kat = vec![0u8; key_count * 2];
        read_at(f, kat_abs, &mut kat)?;
        (0..key_count)
            .map(|i| fcr_offset + u16le(&kat, i * 2) as u64)
            .collect()
    } else {
        (0..key_count)
            .map(|i| 0x110u64 + i as u64 * KD_LEN)
            .collect()
    };

    let mut keys = Vec::new();
    for (key_num, &start_off) in key_starts.iter().enumerate() {
        let mut segments = Vec::new();
        let mut seg_off = start_off;

        loop {
            let mut kd = [0u8; 0x1E];
            if read_at(f, seg_off, &mut kd).is_err() {
                break;
            }

            let attrs = u16le(&kd, 0x8);
            let raw_pos = u16le(&kd, 0x14);
            let length = u16le(&kd, 0x16);
            let null_val = kd[0x1D];

            let offset = if is_v6 {
                raw_pos.saturating_sub(2)
            } else {
                raw_pos
            };

            let data_type: u8 = if attrs & USE_EXT_TYPE != 0 {
                kd[0x1C]
            } else if attrs & OLD_BIN != 0 {
                0x0E
            } else {
                0x00
            };

            segments.push(BtrieveSeg {
                offset,
                length,
                data_type,
                attributes: attrs,
                null_value: null_val,
                descending: attrs & DESCENDING != 0,
                allows_dups: attrs & DUPLICATES != 0,
            });

            if attrs & SEGMENTED_KEY == 0 {
                break;
            }
            seg_off += KD_LEN;
        }

        keys.push(BtrieveKey {
            number: key_num as u16,
            segments,
        });
    }

    Ok(keys)
}

// ── IntFile generation ─────────────────────────────────────────────────────────

/// Build an `IntFile` skeleton from a parsed `BtrieveSchema`.
///
/// Only key-segment fields are populated (with auto-generated names like `K1_SEG1`).
/// Non-key fields must be added manually via `db_config add-field`.
pub fn schema_to_int(schema: &BtrieveSchema, table_name: &str, source_file: &str) -> IntFile {
    let mut fields: Vec<IntField> = Vec::new();
    let mut indexes: Vec<IntIndex> = Vec::new();
    let mut field_num = 1u32;

    for key in &schema.keys {
        let mut segs: Vec<IndexSegment> = Vec::new();
        for seg in &key.segments {
            let existing = fields
                .iter()
                .find(|f| f.offset == seg.offset as u32 && f.length == seg.length as u32);
            let fnum = if let Some(f) = existing {
                f.num
            } else {
                let name = format!("K{}_SEG{}", key.number + 1, segs.len() + 1);
                fields.push(IntField {
                    num: field_num,
                    name,
                    native_type: seg.data_type as i32,
                    length: seg.length as u32,
                    offset: seg.offset as u32,
                    field_index: Some(key.number as u32 + 1),
                    default_value: None,
                });
                let n = field_num;
                field_num += 1;
                n
            };
            segs.push(IndexSegment {
                field_num: fnum,
                attrs: seg.attributes,
                descending: seg.descending,
                null_value: seg.null_value,
            });
        }
        if !segs.is_empty() {
            let num_segments = segs.len() as u32;
            indexes.push(IntIndex {
                num: key.number as u32 + 1,
                num_segments,
                segments: segs,
            });
        }
    }

    fields.sort_by_key(|f| f.offset);

    IntFile {
        table_name: table_name.to_string(),
        schema_name: String::new(),
        db_name: String::new(),
        record_length: schema.record_length as u32,
        page_size: schema.page_size,
        file_flags: schema.file_flags,
        ignore_null_values: false,
        trim_string_fields: false,
        translate_oem_to_ansi: false,
        primary_index: None,
        local_cache: false,
        driver_name: String::new(),
        server_name: String::new(),
        permanent_int: false,
        number_df_fields: 0,
        source_file: source_file.to_string(),
        source_path: String::new(),
        source_dir: String::new(),
        fields,
        indexes,
    }
}

/// How many record bytes are covered by the auto-generated key fields.
pub fn covered_bytes(schema: &BtrieveSchema) -> u32 {
    let mut covered = std::collections::HashSet::new();
    for key in &schema.keys {
        for seg in &key.segments {
            for b in seg.offset..seg.offset + seg.length {
                covered.insert(b);
            }
        }
    }
    covered.len() as u32
}
