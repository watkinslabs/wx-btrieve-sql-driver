use btr_test_harness as h;

#[test]
fn multi_03_partial_key_get_ge() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_MULTI.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // Just REGION filled — DEPT and SUB_CODE are zero (null wildcard).
    key[0..4].copy_from_slice(b"SOUT");
    let rc = h::btrcall(9, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get GE rc");

    // Smallest in SOUT is SOUT/ENG /ALPHA  (rank by compound key).
    assert_eq!(&data[0..4], b"SOUT");
    assert_eq!(&data[4..8], b"ENG ");
    assert_eq!(&data[8..14], b"ALPHA ");
}
