use btr_test_harness as h;

fn build_record(cust_id: &[u8; 8], cust_name: &str, city: &str, state: &[u8; 2]) -> [u8; 73] {
    let mut rec = [0u8; 73];
    rec[0..8].copy_from_slice(cust_id);
    // pad to 30
    let name_bytes = cust_name.as_bytes();
    let n = name_bytes.len().min(30);
    rec[8..8 + n].copy_from_slice(&name_bytes[..n]);
    for b in &mut rec[8 + n..38] {
        *b = b' ';
    }
    let city_bytes = city.as_bytes();
    let n = city_bytes.len().min(20);
    rec[38..38 + n].copy_from_slice(&city_bytes[..n]);
    for b in &mut rec[38 + n..58] {
        *b = b' ';
    }
    rec[58..60].copy_from_slice(state);
    // BALANCE: i64 LE = 1234
    rec[60..68].copy_from_slice(&1234_i64.to_le_bytes());
    // ACTIVE: logical 1
    rec[68] = 0xFF;
    // CREATED: 2024-11-01 [day, month, year_lo, year_hi]
    rec[69] = 1;
    rec[70] = 11;
    let y: u16 = 2024;
    rec[71] = y as u8;
    rec[72] = (y >> 8) as u8;
    rec
}

#[test]
fn op_02_insert_new_row() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut rec = build_record(b"Z9999   ", "ZULU TEST CORP", "LINCOLN", b"NE");
    let mut dlen: u32 = 73;
    let mut key = [0u8; 64];
    let rc = h::btrcall(2, &mut posblk, &mut rec, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "insert rc");

    // Now Get Equal on Z9999 — should find it.
    let mut data = vec![0u8; 256];
    let mut dlen2: u32 = 256;
    let mut key2 = [0u8; 64];
    key2[..8].copy_from_slice(b"Z9999   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen2, &mut key2, 0);
    assert_eq!(rc, 0, "get equal after insert");
    assert_eq!(&data[0..8], b"Z9999   ");
    assert!(data[8..38].starts_with(b"ZULU TEST CORP"));
}
