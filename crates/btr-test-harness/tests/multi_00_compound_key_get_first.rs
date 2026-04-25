use btr_test_harness as h;

#[test]
fn multi_00_compound_key_get_first() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_MULTI.B");
    assert_eq!(rc, 0, "open TEST_MULTI");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");

    // Smallest seeded compound (REGION, DEPT, SUB_CODE) tuple is
    // ('EAST','ENG ','ALPHA ').
    assert_eq!(&data[0..4], b"EAST", "REGION");
    assert_eq!(&data[4..8], b"ENG ", "DEPT");
    assert_eq!(&data[8..14], b"ALPHA ", "SUB_CODE");

    // Key buffer should hold the same 14 bytes (REGION+DEPT+SUB_CODE).
    assert_eq!(&key[0..4], b"EAST", "key REGION");
    assert_eq!(&key[4..8], b"ENG ", "key DEPT");
    assert_eq!(&key[8..14], b"ALPHA ", "key SUB_CODE");
}
