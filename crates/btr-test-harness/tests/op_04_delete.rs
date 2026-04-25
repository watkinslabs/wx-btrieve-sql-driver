use btr_test_harness as h;

#[test]
fn op_04_delete_a0010() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 64];
    key[..8].copy_from_slice(b"A0010   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get equal A0010");

    let mut dlen2: u32 = 0;
    let mut dummy = [0u8; 1];
    let mut dummy_key = [0u8; 1];
    let rc = h::btrcall(4, &mut posblk, &mut dummy, &mut dlen2, &mut dummy_key, 0);
    assert_eq!(rc, 0, "delete rc");

    // Re-read → expect BTR_KEY_NOT_FOUND (4).
    let mut data2 = vec![0u8; 256];
    let mut dlen3: u32 = 256;
    let mut key2 = [0u8; 64];
    key2[..8].copy_from_slice(b"A0010   ");
    let rc = h::btrcall(5, &mut posblk, &mut data2, &mut dlen3, &mut key2, 0);
    assert_eq!(rc, 4, "expected BTR_KEY_NOT_FOUND after delete, got {rc}");
}
