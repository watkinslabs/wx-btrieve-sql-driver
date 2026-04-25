use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_26_version_bytes() {
    h::reset_fixture();
    let mut posblk = h::new_posblk();
    let mut data = vec![0u8; 6];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 8];
    let rc = h::btrcall(26, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "version rc");
    assert_eq!(
        &data[..6],
        &[0x0F, 0x06, 0x00, 0x02, 0x00, 0x00],
        "version bytes should match 6.15 emulation"
    );
}
