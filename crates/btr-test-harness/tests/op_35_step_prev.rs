use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_35_step_prev_after_step_last() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(34, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step last rc");

    let mut dlen2: u32 = 256;
    let rc = h::btrcall(35, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step prev rc");
    assert!(dlen2 > 0, "step prev should return data");
}
