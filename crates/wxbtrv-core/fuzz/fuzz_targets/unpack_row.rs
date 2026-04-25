#![no_main]

use libfuzzer_sys::fuzz_target;
use wxbtrv_core::record::unpack_row;
use wxbtrv_core::state::IntField;

// Btrieve native type codes — kept in sync with crates/wxbtrv-core/src/record.rs.
const TYPES: &[i32] = &[
    0,  // STRING
    1,  // INT
    2,  // FLOAT
    3,  // DATE
    4,  // TIME
    5,  // DECIMAL
    6,  // MONEY
    7,  // LOGICAL
    10, // LSTRING
    11, // ZSTRING
    12, // ZSTRING_12
    14, // AUTOINC
    15, // AUTOINCREMENT
    99, // unknown / fall-through branch
];

// Field lengths to exercise both well-formed and short slot decodes.
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
    // One pathological field at a far offset to exercise the bounds-check path.
    fields.push(IntField {
        num,
        name: "FAR".into(),
        native_type: 1,
        length: 8,
        offset: 0xFFFF_FFF0,
        field_index: None,
        default_value: None,
    });
    fields
}

fuzz_target!(|data: &[u8]| {
    let fields = synthetic_fields();
    // Contract: unpack_row must never panic, regardless of how short or
    // garbled `data` is. We discard the result — the assertion is "did not
    // panic / abort under sanitizer".
    let _ = unpack_row(&fields, data);
});
