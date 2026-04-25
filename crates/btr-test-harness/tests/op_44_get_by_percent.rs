use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_44_get_by_percent_50() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Issue Get By Percent with 5000 (50%) in data[0..4], key_num=0.
    let mut data = vec![0u8; 256];
    data[0..4].copy_from_slice(&5000u32.to_le_bytes());
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(44, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get by percent rc");
    // Returned record should start with 'A' (all fixture CUST_IDs are A00xx).
    assert_eq!(data[0], b'A', "returned CUST_ID should start with 'A'");
    // Next 4 chars should be '0', '0', '0', digit '1'..='9' or '1' (for A0010).
    assert_eq!(
        &data[1..4],
        b"000",
        "second through fourth bytes of CUST_ID"
    );
}
