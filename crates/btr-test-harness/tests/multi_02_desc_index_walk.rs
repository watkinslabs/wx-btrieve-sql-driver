use btr_test_harness as h;

fn value_le(d: &[u8]) -> i32 {
    i32::from_le_bytes(d[14..18].try_into().unwrap())
}

#[test]
fn multi_02_desc_index_walk() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_MULTI.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    // key_num=1 → second index (VALUE DESC)
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 1);
    assert_eq!(rc, 0, "get first key1 rc");
    let mut vals = vec![value_le(&data)];

    for i in 0..3 {
        let mut dlen2: u32 = 256;
        let rc = h::btrcall(6, &mut posblk, &mut data, &mut dlen2, &mut key, 1);
        assert_eq!(rc, 0, "get next #{} rc", i);
        vals.push(value_le(&data));
    }

    // Largest seeded VALUE is 900, then 850, 750, 700.
    assert_eq!(vals[0], 900, "largest first");
    for w in vals.windows(2) {
        assert!(w[0] >= w[1], "DESC walk not descending: {:?}", vals);
    }
    assert_eq!(vals, vec![900, 850, 750, 700]);
}
