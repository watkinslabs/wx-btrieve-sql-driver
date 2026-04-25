//! btrieve.rs — record reading from .B files for db-config (analyze-b, future data ops).
//!
//! Schema extraction (FCR parsing, schema_to_int) is in btr_types::bfile and shared
//! with btr-import. The record-walker (`read_records`, `pat_lookup`, etc.) is
//! reserved for the not-yet-wired `analyze-b` command.
#![allow(dead_code)]

pub use btr_types::bfile::{parse_schema, schema_to_int, BtrieveSchema, RecordKind};

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

fn read_at(f: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), String> {
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    f.read_exact(buf).map_err(|e| e.to_string())
}

fn u16le(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() {
        return 0;
    }
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

/// Read all data records from a .B file, calling `on_record` for each live record.
/// Returns the total number of records yielded.
pub fn read_records<F>(path: &Path, schema: &BtrieveSchema, mut on_record: F) -> Result<u32, String>
where
    F: FnMut(&[u8]) -> bool,
{
    if schema.record_kind != RecordKind::Fixed {
        return Err("variable-length records are not yet supported for data extraction".into());
    }

    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let file_len = f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;

    let ps = schema.page_size as usize;
    let prl = schema.physical_rec_length as usize;
    let rl = schema.record_length as usize;
    if prl == 0 {
        return Err("physical_rec_length is 0".into());
    }
    let recs_per_page = (ps - 6) / prl;
    let mut count = 0u32;

    if schema.is_v6 {
        for logical_page in 1..schema.page_count {
            let Some((phys_off, page_type)) = pat_lookup(&mut f, logical_page, schema, file_len)?
            else {
                continue;
            };
            if page_type != b'D' && page_type != b'V' {
                continue;
            }

            let mut page = vec![0u8; ps];
            read_at(&mut f, phys_off as u64, &mut page)?;
            if page[5] & 0x80 == 0 {
                continue;
            }

            for slot in 0..recs_per_page {
                let off = 6 + slot * prl;
                if off + prl > ps {
                    break;
                }
                let slot_data = &page[off..off + prl];
                let usage = (slot_data[0] as u16) << 8 | slot_data[1] as u16;
                if usage == 0 {
                    continue;
                }
                let rec_end = 2 + rl;
                if rec_end > slot_data.len() {
                    continue;
                }
                count += 1;
                if !on_record(&slot_data[2..rec_end]) {
                    return Ok(count);
                }
            }
        }
    } else {
        for page_num in 1..schema.page_count {
            let off = (page_num as u64) * (ps as u64);
            if off + ps as u64 > file_len {
                break;
            }
            let mut page = vec![0u8; ps];
            read_at(&mut f, off, &mut page)?;
            if page[5] & 0x80 == 0 {
                continue;
            }

            for slot in 0..recs_per_page {
                let soff = 6 + slot * prl;
                if soff + prl > ps {
                    break;
                }
                let slot_data = &page[soff..soff + prl];
                if v5_is_unused(slot_data, file_len) {
                    break;
                }
                let record = &slot_data[..rl.min(slot_data.len())];
                count += 1;
                if !on_record(record) {
                    return Ok(count);
                }
            }
        }
    }

    Ok(count)
}

fn v5_is_unused(data: &[u8], file_len: u64) -> bool {
    if data.len() < 4 {
        return true;
    }
    let ptr = ((data[0] as u64) << 24)
        | ((data[1] as u64) << 16)
        | ((data[2] as u64) << 8)
        | (data[3] as u64);
    if data[4..].iter().any(|&b| b != 0) {
        return false;
    }
    ptr < file_len
}

fn pat_lookup(
    f: &mut File,
    logical_page: u32,
    schema: &BtrieveSchema,
    file_len: u64,
) -> Result<Option<(u32, u8)>, String> {
    let ps = schema.page_size as u64;
    let pages_per_pat = (schema.page_size as u32 / 4) - 2;

    let mut lp = logical_page;
    let mut pat_start_page: u64 = 2;
    while lp > pages_per_pat {
        lp -= pages_per_pat;
        pat_start_page += schema.page_size as u64 / 4;
    }

    let pat_off = pat_start_page * ps;
    if pat_off + ps * 2 > file_len {
        return Ok(None);
    }

    let mut pat = vec![0u8; (ps as usize) * 2];
    read_at(f, pat_off, &mut pat)?;
    let (pat1, pat2) = pat.split_at(ps as usize);

    let valid1 = pat1[0] == b'P' && pat1[1] == b'P';
    let valid2 = pat2[0] == b'P' && pat2[1] == b'P';
    if !valid1 && !valid2 {
        return Ok(None);
    }

    let uc1 = u16le(pat1, 4);
    let uc2 = u16le(pat2, 4);
    let ap = if uc1 >= uc2 { pat1 } else { pat2 };

    let entry_off = (lp as usize) * 4 + 4;
    if entry_off + 4 > ps as usize {
        return Ok(None);
    }

    let e = &ap[entry_off..entry_off + 4];
    let page_type = e[1];
    let phys_page = ((e[0] as u32) << 16) | ((e[3] as u32) << 8) | (e[2] as u32);
    if phys_page == 0xFFFF_FF {
        return Ok(None);
    }

    let phys_off = phys_page * schema.page_size as u32;
    if (phys_off as u64) + ps > file_len {
        return Ok(None);
    }

    Ok(Some((phys_off, page_type)))
}
