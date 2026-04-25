use btr_test_harness as h;

#[test]
fn desc_00_get_first_largest() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_DESC.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");

    let rank = i32::from_le_bytes(data[0..4].try_into().unwrap());
    assert_eq!(
        rank, 80,
        "DESC index Get First should return largest RANK_VAL"
    );
}
