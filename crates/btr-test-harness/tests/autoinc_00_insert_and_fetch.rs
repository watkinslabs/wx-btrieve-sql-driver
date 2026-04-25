use btr_test_harness as h;

#[test]
fn autoinc_00_insert_and_fetch() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_AUTOINC.B");
    assert_eq!(rc, 0, "open TEST_AUTOINC");

    // Build a 24-byte record with AUTO_ID=0 (request assignment) and NAME="FOXTROT".
    let mut data = vec![0u8; 256];
    // bytes 0..4 = AUTO_ID (zero)
    let name = b"FOXTROT             "; // 20 bytes, space-padded
    data[4..24].copy_from_slice(name);
    let mut dlen: u32 = 24;
    let mut key = [0u8; 80];

    let rc = h::btrcall(2, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "insert rc");

    // Engine should have written the assigned AUTO_ID into bytes 0..4 of data.
    let assigned = i32::from_le_bytes(data[0..4].try_into().unwrap());
    assert!(assigned > 0, "AUTO_ID not assigned (got {})", assigned);

    // Now Get Equal on AUTO_ID via key 0.
    let mut data2 = vec![0u8; 256];
    let mut dlen2: u32 = 256;
    let mut key2 = [0u8; 80];
    key2[0..4].copy_from_slice(&assigned.to_le_bytes());
    let rc = h::btrcall(5, &mut posblk, &mut data2, &mut dlen2, &mut key2, 0);
    assert_eq!(rc, 0, "get equal rc");

    let read_id = i32::from_le_bytes(data2[0..4].try_into().unwrap());
    assert_eq!(read_id, assigned, "AUTO_ID round trip");
    assert_eq!(&data2[4..11], b"FOXTROT", "NAME prefix");
}
