use btr_test_harness as h;

#[test]
fn op_03_update_city() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 64];
    key[..8].copy_from_slice(b"A0003   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get equal A0003");

    // Mutate CITY (offset 38, 20 bytes) → "HOUSTON            "
    let city = b"HOUSTON             ";
    data[38..58].copy_from_slice(city);

    let mut dlen2: u32 = dlen;
    let rc = h::btrcall(3, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, 0, "update rc");

    // Re-read.
    let mut data2 = vec![0u8; 256];
    let mut dlen3: u32 = 256;
    let mut key2 = [0u8; 64];
    key2[..8].copy_from_slice(b"A0003   ");
    let rc = h::btrcall(5, &mut posblk, &mut data2, &mut dlen3, &mut key2, 0);
    assert_eq!(rc, 0, "get equal after update");
    assert!(
        data2[38..58].starts_with(b"HOUSTON"),
        "CITY not updated, got {:?}",
        String::from_utf8_lossy(&data2[38..58])
    );
}
