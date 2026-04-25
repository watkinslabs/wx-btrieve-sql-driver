use btr_test_harness as h;

#[test]
fn types_02_get_by_negative_int() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_TYPES.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // INT_VAL=-250 → row with STR_FIX="CCCC".
    key[0..4].copy_from_slice(&(-250i32).to_le_bytes());
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 1);
    assert_eq!(rc, 0, "get equal negative rc");
    assert_eq!(&data[0..10], b"CCCC      ", "matched STR_FIX");
    let int_val = i32::from_le_bytes(data[26..30].try_into().unwrap());
    assert_eq!(int_val, -250);
}
