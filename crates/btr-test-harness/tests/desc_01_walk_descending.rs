use btr_test_harness as h;

fn rank(d: &[u8]) -> i32 {
    i32::from_le_bytes(d[0..4].try_into().unwrap())
}

#[test]
fn desc_01_walk_descending() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_DESC.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");
    let mut ranks = vec![rank(&data)];

    for i in 0..4 {
        let mut dlen2: u32 = 256;
        let rc = h::btrcall(6, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
        assert_eq!(rc, 0, "get next #{} rc", i);
        ranks.push(rank(&data));
    }

    assert_eq!(ranks, vec![80, 70, 60, 50, 40], "DESC walk");
}
