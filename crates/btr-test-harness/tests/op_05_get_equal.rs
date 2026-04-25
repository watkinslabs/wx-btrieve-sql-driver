use btr_test_harness as h;

#[test]
fn op_05_get_equal_a0005() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 64];
    // CUST_ID is CHAR(8). Load "A0005   " (space padded).
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get equal rc");
    assert_eq!(&data[0..8], b"A0005   ", "CUST_ID mismatch");
    assert!(
        data[8..38].starts_with(b"ECHO SOFTWARE"),
        "CUST_NAME does not begin with ECHO SOFTWARE, got {:?}",
        String::from_utf8_lossy(&data[8..38]),
    );
}
