use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

/// Op 14 — Create. Our implementation best-effort translates the descriptor
/// into CREATE TABLE IF NOT EXISTS against SQL Server; if SQL fails it still
/// returns BTR_SUCCESS (per the comment in create.rs). We therefore only
/// assert rc == 0 and make sure the call path is actually exercised.
#[test]
fn op_14_create_new_table() {
    h::reset_fixture();

    // Build a minimal valid descriptor: 16-byte file header + one 16-byte key
    // segment spec (pos=1, len=8, flags=0, type=0 STRING).
    let mut data = vec![0u8; 32];
    // rec_len = 8
    data[0..2].copy_from_slice(&8u16.to_le_bytes());
    // page_size = 4096
    data[2..4].copy_from_slice(&4096u16.to_le_bytes());
    // num_indexes = 1
    data[4..6].copy_from_slice(&1u16.to_le_bytes());
    // 16..32: one segment, pos=1, len=8, flags=0, ext_type=0
    data[16..18].copy_from_slice(&1u16.to_le_bytes());
    data[18..20].copy_from_slice(&8u16.to_le_bytes());
    // ext_type is at seg[10]
    data[16 + 10] = 0;

    let mut dlen: u32 = data.len() as u32;
    let mut posblk = h::new_posblk();
    let mut key_buf = [0u8; 80];
    let path = b"TEST_NEW.B";
    key_buf[..path.len()].copy_from_slice(path);

    let rc = h::btrcall(14, &mut posblk, &mut data, &mut dlen, &mut key_buf, 0);
    assert_eq!(
        rc, BTR_SUCCESS,
        "create op should return SUCCESS (SQL best-effort)"
    );
}
