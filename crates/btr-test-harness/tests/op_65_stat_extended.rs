use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

#[test]
fn op_65_stat_extended_subfn0() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 1024];
    data[0] = 0; // subfunction 0 (File Statistics)
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 8];
    let rc = h::btrcall(65, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "stat extended rc");
    assert!(
        dlen >= 32,
        "dlen_out should be reasonable (>=32), got {}",
        dlen
    );

    // Header sanity: record_length matches fixture.
    let rec_len = u16::from_le_bytes([data[0], data[1]]);
    assert_eq!(rec_len, 73);
}
