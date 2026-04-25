use btr_test_harness as h;

#[test]
fn op_01_close_test_cust() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 64];
    let rc = h::btrcall(1, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "close should succeed");

    // After close, subsequent op should return BTR_FILE_NOT_OPEN (3).
    let mut dlen2: u32 = 256;
    let rc2 = h::btrcall(12, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(
        rc2, 3,
        "op after close should return BTR_FILE_NOT_OPEN, got {rc2}"
    );
}
