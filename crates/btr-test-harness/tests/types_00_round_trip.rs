use btr_test_harness as h;

#[test]
fn types_00_round_trip() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_TYPES.B");
    assert_eq!(rc, 0, "open TEST_TYPES");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // Get First on key 0 = STR_FIX index ascending. Smallest is "AAAA".
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");

    // STR_FIX (0..10) "AAAA      "
    assert_eq!(&data[0..10], b"AAAA      ", "STR_FIX");

    // STR_Z (10..26): begins with "first", null-terminated within the 16-byte slot.
    assert_eq!(&data[10..15], b"first", "STR_Z prefix");
    assert!(
        data[10..26].contains(&0),
        "STR_Z slot must be null-terminated within bounds; got {:?}",
        &data[10..26]
    );

    // INT_VAL (26..30) = 100
    let int_val = i32::from_le_bytes(data[26..30].try_into().unwrap());
    assert_eq!(int_val, 100, "INT_VAL");

    // DEC_VAL (30..38) = 123456
    let dec_val = i64::from_le_bytes(data[30..38].try_into().unwrap());
    assert_eq!(dec_val, 123456, "DEC_VAL");

    // LOG_VAL (38) = 0xFF (true)
    assert_eq!(data[38], 0xFF, "LOG_VAL");

    // DATE_VAL (39..43): 2024-01-15 → [day=15, month=1, year_lo, year_hi]
    assert_eq!(data[39], 15, "DATE day");
    assert_eq!(data[40], 1, "DATE month");
    let year = data[41] as u16 | ((data[42] as u16) << 8);
    assert_eq!(year, 2024, "DATE year");

    // Now exercise unpack_row → SQL literals; verify each comes back right.
    use wxbtrv_core::record::unpack_row;
    use wxbtrv_core::state::IntField;
    let fields = [
        IntField {
            num: 1,
            name: "STR_FIX".into(),
            native_type: 0,
            length: 10,
            offset: 0,
            field_index: None,
            default_value: None,
        },
        IntField {
            num: 2,
            name: "STR_Z".into(),
            native_type: 11,
            length: 16,
            offset: 10,
            field_index: None,
            default_value: None,
        },
        IntField {
            num: 3,
            name: "INT_VAL".into(),
            native_type: 1,
            length: 4,
            offset: 26,
            field_index: None,
            default_value: None,
        },
        IntField {
            num: 4,
            name: "DEC_VAL".into(),
            native_type: 5,
            length: 8,
            offset: 30,
            field_index: None,
            default_value: None,
        },
        IntField {
            num: 5,
            name: "LOG_VAL".into(),
            native_type: 7,
            length: 1,
            offset: 38,
            field_index: None,
            default_value: None,
        },
        IntField {
            num: 6,
            name: "DATE_VAL".into(),
            native_type: 3,
            length: 4,
            offset: 39,
            field_index: None,
            default_value: None,
        },
    ];
    let rows = unpack_row(&fields, &data[..43]);
    assert_eq!(rows[0].1, "'AAAA'", "unpack STR_FIX");
    assert_eq!(rows[1].1, "'first'", "unpack STR_Z");
    assert_eq!(rows[2].1, "100", "unpack INT_VAL");
    assert_eq!(rows[3].1, "123456", "unpack DEC_VAL");
    assert_eq!(rows[4].1, "1", "unpack LOG_VAL");
    assert_eq!(rows[5].1, "'2024-01-15'", "unpack DATE_VAL");
}
