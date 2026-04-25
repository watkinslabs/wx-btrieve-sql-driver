use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_07_get_prev_after_last() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // Get Last
    let rc = h::btrcall(13, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get last rc");
    assert_eq!(&data[0..8], b"A0010   ", "last record CUST_ID");

    // Get Previous
    let mut dlen2: u32 = 256;
    let rc = h::btrcall(7, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get prev rc");
    assert_eq!(&data[0..8], b"A0009   ", "second-to-last CUST_ID");
}
