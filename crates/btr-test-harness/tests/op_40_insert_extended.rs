use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

fn build_record(cust_id: &[u8; 8], name: &[u8]) -> [u8; 73] {
    let mut rec = [0u8; 73];
    rec[0..8].copy_from_slice(cust_id);
    let n = name.len().min(30);
    rec[8..8 + n].copy_from_slice(&name[..n]);
    for i in 8 + n..38 {
        rec[i] = b' ';
    }
    for i in 38..58 {
        rec[i] = b' ';
    }
    rec[58] = b'Z';
    rec[59] = b'Z';
    rec[68] = 1;
    rec
}

#[test]
fn op_40_insert_extended_batch3() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Build 3 records with CUST_IDs "BATCH01 ", "BATCH02 ", "BATCH03 ".
    let r1 = build_record(b"BATCH01 ", b"BATCH ONE");
    let r2 = build_record(b"BATCH02 ", b"BATCH TWO");
    let r3 = build_record(b"BATCH03 ", b"BATCH THREE");

    // Layout: u16 count, then per-record u16 length + record bytes.
    let mut buf = Vec::new();
    buf.extend_from_slice(&3u16.to_le_bytes());
    for r in [&r1, &r2, &r3] {
        buf.extend_from_slice(&(r.len() as u16).to_le_bytes());
        buf.extend_from_slice(r);
    }
    // Pad to a comfortable size so output-rewrite has headroom.
    buf.resize(buf.len().max(1024), 0);
    let mut dlen: u32 = buf.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(40, &mut posblk, &mut buf, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "insert extended rc");

    // Verify the first 2 bytes of buf reflect success count = 3.
    let success = u16::from_le_bytes([buf[0], buf[1]]);
    assert_eq!(success, 3, "expected 3 successful inserts");

    // Verify each is gettable.
    for id in [b"BATCH01 ", b"BATCH02 ", b"BATCH03 "] {
        let (mut p2, rc) = h::fixture_open("TEST_CUST.B");
        assert_eq!(rc, 0);
        let mut data = vec![0u8; 256];
        let mut dlen2: u32 = data.len() as u32;
        let mut key2 = [0u8; 64];
        key2[..8].copy_from_slice(id);
        let rc = h::btrcall(5, &mut p2, &mut data, &mut dlen2, &mut key2, 0);
        assert_eq!(rc, BTR_SUCCESS, "get equal {:?}", std::str::from_utf8(id));
        assert_eq!(&data[0..8], id);
    }
}
