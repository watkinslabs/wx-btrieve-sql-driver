use btr_test_harness as h;

#[test]
fn op_06_get_next_after_first() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 64];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");
    assert_eq!(&data[0..8], b"A0001   ");

    let mut dlen2: u32 = 256;
    let rc = h::btrcall(6, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, 0, "get next rc");
    assert_eq!(&data[0..8], b"A0002   ", "second record");
}
