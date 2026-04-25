use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_09_get_ge_exact_a0005() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(9, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get GE rc");
    assert_eq!(&data[0..8], b"A0005   ", "GE exact match");
}

#[test]
fn op_09_get_ge_between() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // "A00055  " sorts between A0005 and A0006
    key[..8].copy_from_slice(b"A00055  ");
    let rc = h::btrcall(9, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get GE between rc");
    assert_eq!(
        &data[0..8],
        b"A0006   ",
        "GE on between-key should land on A0006"
    );
}
