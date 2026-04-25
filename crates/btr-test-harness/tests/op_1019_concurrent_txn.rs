use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

fn build_record(cust_id: &[u8; 8], name: &[u8]) -> Vec<u8> {
    let mut rec = vec![0u8; 73];
    rec[0..8].copy_from_slice(cust_id);
    let n = name.len().min(30);
    rec[8..8 + n].copy_from_slice(&name[..n]);
    for i in 8 + n..38 {
        rec[i] = b' ';
    }
    for i in 38..58 {
        rec[i] = b' ';
    }
    rec[58] = b'C';
    rec[59] = b'C';
    rec[68] = 1;
    rec
}

#[test]
fn op_1019_concurrent_begin_commit_persists() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut dummy_data = vec![0u8; 1];
    let mut dlen: u32 = 0;
    let mut dummy_key = [0u8; 8];

    // Begin Concurrent Transaction (op 1019 — READ COMMITTED).
    let rc = h::btrcall(
        1019,
        &mut posblk,
        &mut dummy_data,
        &mut dlen,
        &mut dummy_key,
        0,
    );
    assert_eq!(rc, BTR_SUCCESS, "begin concurrent txn rc");

    // Insert a row inside the concurrent txn.
    let mut rec = build_record(b"CNC001  ", b"CONCURRENT");
    let mut dlen_ins: u32 = rec.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(2, &mut posblk, &mut rec, &mut dlen_ins, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "insert inside concurrent txn rc");

    // End Transaction (op 20, commit).
    let rc = h::btrcall(
        20,
        &mut posblk,
        &mut dummy_data,
        &mut dlen,
        &mut dummy_key,
        0,
    );
    assert_eq!(rc, BTR_SUCCESS, "end txn rc");

    // Verify persistence via a fresh open.
    let (mut posblk2, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "reopen");
    let mut data = vec![0u8; 256];
    let mut dlen2: u32 = data.len() as u32;
    let mut key2 = [0u8; 64];
    key2[..8].copy_from_slice(b"CNC001  ");
    let rc = h::btrcall(5, &mut posblk2, &mut data, &mut dlen2, &mut key2, 0);
    assert_eq!(rc, BTR_SUCCESS, "get equal after commit");
    assert_eq!(&data[0..8], b"CNC001  ");
}
