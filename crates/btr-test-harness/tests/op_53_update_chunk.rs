use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_53_update_chunk_state_field() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Establish currency via Get Equal on A0005.
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(5, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get equal");
    assert_eq!(&data[58..60], b"WA");

    // Random descriptor: sig=0x00000000, nChunks=1, nextOff=0,
    // then (offset=58 u32, len=2 u32), then 2 bytes "XX".
    let mut desc = Vec::new();
    desc.extend_from_slice(&0u32.to_le_bytes()); // sig
    desc.extend_from_slice(&1u32.to_le_bytes()); // nChunks
    desc.extend_from_slice(&0u32.to_le_bytes()); // nextOff
    desc.extend_from_slice(&58u32.to_le_bytes()); // offset
    desc.extend_from_slice(&2u32.to_le_bytes()); // len
    desc.extend_from_slice(b"XX");

    let mut dlen2: u32 = desc.len() as u32;
    let mut key2 = [0u8; 64];
    let rc = h::btrcall(53, &mut posblk, &mut desc, &mut dlen2, &mut key2, 0);
    assert_eq!(rc, BTR_SUCCESS, "update chunk rc");

    // Re-fetch A0005 and verify STATE is now XX.
    let mut data3 = vec![0u8; 256];
    let mut dlen3: u32 = data3.len() as u32;
    let mut key3 = [0u8; 64];
    key3[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(5, &mut posblk, &mut data3, &mut dlen3, &mut key3, 0);
    assert_eq!(rc, BTR_SUCCESS, "get equal after chunk");
    assert_eq!(
        &data3[58..60],
        b"XX",
        "STATE should be updated to XX, got {:?}",
        std::str::from_utf8(&data3[58..60])
    );
}
