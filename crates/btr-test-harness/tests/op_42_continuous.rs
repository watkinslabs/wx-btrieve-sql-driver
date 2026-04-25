use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_42_continuous_start_and_end_all() {
    h::reset_fixture();
    let mut posblk = h::new_posblk();

    // Start: key_num=0, data_buf holds comma-sep path list terminated by NUL.
    let paths = b"G:\\PACIFIC\\TEST_CUST.B\0";
    let mut data = vec![0u8; 128];
    data[..paths.len()].copy_from_slice(paths);
    let mut dlen: u32 = paths.len() as u32;
    let mut key = [0u8; 8];
    let rc = h::btrcall(42, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "continuous start rc");

    // End all: key_num=1.
    let mut data2 = vec![0u8; 1];
    let mut dlen2: u32 = 0;
    let rc = h::btrcall(42, &mut posblk, &mut data2, &mut dlen2, &mut key, 1);
    assert_eq!(rc, BTR_SUCCESS, "continuous end-all rc");
}
