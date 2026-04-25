use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_25_stop_ok() {
    h::reset_fixture();
    // Open a file first so Stop has state to clear.
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 1];
    let mut dlen: u32 = 0;
    let mut key = [0u8; 8];
    let rc = h::btrcall(25, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "stop rc");

    // After Stop, a fresh Open should still work.
    let (_, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "reopen after stop");
}
