use btr_test_harness as h;
use wxbtrv_core::constants::BTR_FILE_NOT_OPEN;

#[test]
fn op_01_close_on_zeroed_posblk() {
    h::reset_fixture();
    // Zeroed posblk — no Open has been issued for it.
    let mut posblk = *h::new_posblk();
    let mut data = vec![0u8; 16];
    let mut dlen: u32 = 0;
    let mut key = [0u8; 16];
    let rc = h::btrcall(1, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(
        rc, BTR_FILE_NOT_OPEN,
        "close on zeroed posblk should be FILE_NOT_OPEN"
    );
}
