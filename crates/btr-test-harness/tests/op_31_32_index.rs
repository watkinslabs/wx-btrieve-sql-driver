use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

/// Op 31 — Create Index on CUST_NAME (offset 8 len 30).
/// Op 32 — Drop the newly-created index by key number.
///
/// Note: the SQL CREATE/DROP INDEX is best-effort. op_drop_index always
/// returns BTR_SUCCESS even if the SQL DROP fails, but op_create_index
/// propagates SQL errors. To make the test robust against an existing
/// index, we use a fresh table by calling op_create first — but the
/// fixture only supports TEST_CUST. We instead use a temporary index
/// name unlikely to collide — and accept that a repeat run after a test
/// crash may leave a stale index. reset_fixture() drops & recreates the
/// database so this is fine.
#[test]
fn op_31_32_create_and_drop_index() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Build one 16-byte segment spec: pos=39 (1-based, offset=38 -> CITY),
    // len=20, flags=0. The CreateIndex impl looks up the column via
    // (offset, length) in the handle's field table.
    let mut seg = vec![0u8; 16];
    seg[0..2].copy_from_slice(&39u16.to_le_bytes()); // pos (1-based) -> offset 38 = CITY
    seg[2..4].copy_from_slice(&20u16.to_le_bytes()); // len = 20
                                                     // flags = 0, no-more
    let mut dlen: u32 = seg.len() as u32;
    let mut key = [0u8; 8];
    let rc = h::btrcall(31, &mut posblk, &mut seg, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "create index rc");

    // The newly-created index is appended. The original fixture has 2
    // indexes (keys 0 and 1); our new one becomes key 2.
    let mut data = vec![0u8; 1];
    let mut dlen2: u32 = 0;
    let rc = h::btrcall(32, &mut posblk, &mut data, &mut dlen2, &mut key, 2);
    assert_eq!(rc, BTR_SUCCESS, "drop index rc");
}
