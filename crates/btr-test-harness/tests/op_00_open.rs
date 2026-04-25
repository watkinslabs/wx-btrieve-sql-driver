use btr_test_harness as h;

#[test]
fn op_00_open_test_cust() {
    h::reset_fixture();
    let (posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open should succeed");
    let handle_id = u32::from_le_bytes([posblk[0], posblk[1], posblk[2], posblk[3]]);
    assert!(handle_id > 0, "posblk should contain a nonzero handle id");
}
