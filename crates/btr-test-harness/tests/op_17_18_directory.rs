use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_17_set_directory() {
    h::reset_fixture();
    let mut posblk = h::new_posblk();
    let mut data = vec![0u8; 128];
    let mut dlen: u32 = 0;
    let mut key = [0u8; 80];
    let path = b"G:\\PACIFIC";
    key[..path.len()].copy_from_slice(path);
    key[path.len()] = 0;
    let rc = h::btrcall(17, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "set dir rc");
}

#[test]
fn op_18_get_directory_round_trip() {
    h::reset_fixture();
    let mut posblk = h::new_posblk();
    let mut data = vec![0u8; 128];
    let mut dlen: u32 = 0;
    let mut key = [0u8; 80];

    // Set.
    let path = b"G:\\PACIFIC";
    key[..path.len()].copy_from_slice(path);
    key[path.len()] = 0;
    let rc = h::btrcall(17, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "set dir");

    // Get.
    let mut data2 = vec![0u8; 128];
    let mut dlen2: u32 = data2.len() as u32;
    let mut key2 = [0u8; 80];
    let rc = h::btrcall(18, &mut posblk, &mut data2, &mut dlen2, &mut key2, 0);
    assert_eq!(rc, BTR_SUCCESS, "get dir rc");
    assert!(
        data2.starts_with(b"G:\\PACIFIC"),
        "got: {:?}",
        String::from_utf8_lossy(&data2[..dlen2 as usize])
    );
}
