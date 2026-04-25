#![no_main]

use libfuzzer_sys::fuzz_target;
use wxbtrv_core::record::pack_row;
use wxbtrv_core::state::IntField;

const TYPES: &[i32] = &[0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 14, 15, 99];
const LENGTHS: &[u32] = &[1, 2, 4, 8, 16];

fn synthetic_fields() -> Vec<IntField> {
    let mut fields = Vec::new();
    let mut offset: u32 = 0;
    let mut num: u32 = 1;
    for &t in TYPES {
        for &len in LENGTHS {
            fields.push(IntField {
                num,
                name: format!("F{}_{}", t, len),
                native_type: t,
                length: len,
                offset,
                field_index: None,
                default_value: None,
            });
            offset = offset.saturating_add(len);
            num += 1;
        }
    }
    fields
}

/// Split `data` into N synthetic UTF-8 column values. We don't care about
/// well-formed schemas — only that pack_row never panics on hostile input.
fn split_into_columns(data: &[u8], n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    let chunk = (data.len() / n.max(1)).max(1);
    for i in 0..n {
        let start = (i * chunk).min(data.len());
        let end = ((i + 1) * chunk).min(data.len());
        out.push(String::from_utf8_lossy(&data[start..end]).into_owned());
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let fields = synthetic_fields();
    let cols = split_into_columns(data, fields.len());

    // Vary the record_length so we exercise both fitting and clipping paths.
    let record_length: u32 = (data.len() as u32).wrapping_add(7) % 1024;

    // Contract: pack_row must never panic, regardless of how garbage the
    // column strings are.
    let _ = pack_row(&fields, &cols, record_length);
});
