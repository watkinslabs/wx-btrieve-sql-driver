use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_15_stat_test_cust() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 1024];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(15, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "stat rc");
    assert!(
        dlen >= 32,
        "stat dlen_out should be at least 32 (file header + at least one index spec), got {}",
        dlen
    );

    // File spec header.
    let record_length = u16::from_le_bytes([data[0], data[1]]);
    let num_indexes = u16::from_le_bytes([data[4], data[5]]);
    assert_eq!(record_length, 73, "record_length should be 73");
    assert_eq!(
        num_indexes, 2,
        "num_indexes should be 2 (CUST_ID + CUST_NAME)"
    );
}
