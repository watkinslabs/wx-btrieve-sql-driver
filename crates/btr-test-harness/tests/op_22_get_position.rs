use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_22_get_position_after_equal() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get equal rc");

    // Get Position
    let mut data2 = vec![0u8; 256];
    let mut dlen2: u32 = 256;
    let rc = h::btrcall(22, &mut posblk, &mut data2, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get position rc");
    let pos = u32::from_le_bytes([data2[0], data2[1], data2[2], data2[3]]);
    assert!(pos != 0, "position token should be nonzero");
}
