use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_27_unlock_after_locked_get() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Locked Get Equal (+100 lock bias on op 5 == 105).
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(105, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "locked get equal rc");

    let mut dlen2: u32 = 0;
    let rc = h::btrcall(27, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "unlock rc");
}
