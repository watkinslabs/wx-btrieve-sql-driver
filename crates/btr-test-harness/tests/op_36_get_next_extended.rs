use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

/// Build a GNE descriptor: 0 filter terms, max 5 records, 1 field extract
/// (len=8, offset=0 -> CUST_ID).
fn build_gne() -> Vec<u8> {
    let mut d = Vec::new();
    // GNE_HEADER: descriptionLen u16, currencyConst u8, reserved u8,
    // rejectCount u16, numberTerms u16
    let description_len: u16 = 16; // 8 + 4 + 4
    d.extend_from_slice(&description_len.to_le_bytes());
    d.push(0); // currencyConst
    d.push(0); // reserved
    d.extend_from_slice(&0u16.to_le_bytes()); // rejectCount
    d.extend_from_slice(&0u16.to_le_bytes()); // numberTerms = 0
                                              // RETRIEVAL_HEADER: maxRecs=5, noFields=1
    d.extend_from_slice(&5u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    // FIELD_RETRIEVAL_HEADER: fieldLen=8, fieldOffset=0
    d.extend_from_slice(&8u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d
}

#[test]
fn op_36_get_next_extended_after_get_first() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Establish currency with Get First.
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get first");
    assert_eq!(&data[0..8], b"A0001   ");

    // Get Next Extended: batch of 5, project CUST_ID.
    let desc = build_gne();
    let mut buf = vec![0u8; 1024];
    buf[..desc.len()].copy_from_slice(&desc);
    let mut dlen2: u32 = buf.len() as u32;
    let rc = h::btrcall(36, &mut posblk, &mut buf, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get next extended rc");

    // POST_BUFFER_HEADER: numReturned u16 at offset 0
    let num = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    assert!(num > 0 && num <= 5, "numReturned={} expected 1..=5", num);

    // Each entry: 2 bytes recLen + 4 bytes recPos + recLen bytes of data.
    let mut off = 2usize;
    let mut ids: Vec<[u8; 8]> = Vec::new();
    for _ in 0..num {
        let rec_len = u16::from_le_bytes([buf[off], buf[off + 1]]) as usize;
        off += 2 + 4; // skip recLen + recPos
        let mut id = [0u8; 8];
        id.copy_from_slice(&buf[off..off + 8]);
        ids.push(id);
        off += rec_len;
    }
    for w in ids.windows(2) {
        assert!(
            w[0] < w[1],
            "CUST_IDs should be monotonically increasing: {:?}",
            ids
        );
    }
    // After Get First on A0001, Get Next should start at A0002.
    assert_eq!(&ids[0], b"A0002   ");
}
