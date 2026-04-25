use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_23_get_direct_after_position() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get equal rc");
    assert_eq!(&data[0..8], b"A0005   ");

    // Get Position to capture the 4-byte token.
    let mut pos_buf = vec![0u8; 256];
    let mut pos_dlen: u32 = 256;
    let rc = h::btrcall(22, &mut posblk, &mut pos_buf, &mut pos_dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get position rc");
    let token = [pos_buf[0], pos_buf[1], pos_buf[2], pos_buf[3]];

    // Get Direct — pass the token in data[0..4].
    let mut data3 = vec![0u8; 256];
    data3[0..4].copy_from_slice(&token);
    let mut dlen3: u32 = 256;
    let rc = h::btrcall(23, &mut posblk, &mut data3, &mut dlen3, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get direct rc");
    assert_eq!(
        &data3[0..8],
        b"A0005   ",
        "direct fetch should return A0005"
    );
}
