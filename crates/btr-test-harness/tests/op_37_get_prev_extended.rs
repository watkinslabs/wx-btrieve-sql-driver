use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

fn build_gne() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&16u16.to_le_bytes()); // descriptionLen
    d.push(0);
    d.push(0);
    d.extend_from_slice(&0u16.to_le_bytes()); // rejectCount
    d.extend_from_slice(&0u16.to_le_bytes()); // numberTerms
    d.extend_from_slice(&5u16.to_le_bytes()); // maxRecs
    d.extend_from_slice(&1u16.to_le_bytes()); // noFields
    d.extend_from_slice(&8u16.to_le_bytes()); // fieldLen
    d.extend_from_slice(&0u16.to_le_bytes()); // fieldOffset
    d
}

#[test]
fn op_37_get_prev_extended_after_get_last() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(13, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get last");
    assert_eq!(&data[0..8], b"A0010   ");

    let desc = build_gne();
    let mut buf = vec![0u8; 1024];
    buf[..desc.len()].copy_from_slice(&desc);
    let mut dlen2: u32 = buf.len() as u32;
    let rc = h::btrcall(37, &mut posblk, &mut buf, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "get prev extended rc");

    let num = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    assert!(num > 0 && num <= 5, "numReturned={}", num);

    let mut off = 2usize;
    let mut ids: Vec<[u8; 8]> = Vec::new();
    for _ in 0..num {
        let rec_len = u16::from_le_bytes([buf[off], buf[off + 1]]) as usize;
        off += 6;
        let mut id = [0u8; 8];
        id.copy_from_slice(&buf[off..off + 8]);
        ids.push(id);
        off += rec_len;
    }
    // Monotonically decreasing.
    for w in ids.windows(2) {
        assert!(
            w[0] > w[1],
            "CUST_IDs should be monotonically decreasing: {:?}",
            ids
        );
    }
    assert_eq!(&ids[0], b"A0009   ");
}
