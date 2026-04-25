use btr_test_harness as h;

fn region(d: &[u8]) -> &[u8] {
    &d[0..4]
}
fn dept(d: &[u8]) -> &[u8] {
    &d[4..8]
}
fn sub(d: &[u8]) -> &[u8] {
    &d[8..14]
}

#[test]
fn multi_01_compound_key_walk() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_MULTI.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(12, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, 0, "get first rc");
    let mut rows: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = Vec::new();
    rows.push((
        region(&data).to_vec(),
        dept(&data).to_vec(),
        sub(&data).to_vec(),
    ));

    for i in 0..3 {
        let mut dlen2: u32 = 256;
        let rc = h::btrcall(6, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
        assert_eq!(rc, 0, "get next #{} rc", i);
        rows.push((
            region(&data).to_vec(),
            dept(&data).to_vec(),
            sub(&data).to_vec(),
        ));
    }

    // Verify ascending compound order.
    for w in rows.windows(2) {
        assert!(w[0] <= w[1], "rows out of order: {:?} > {:?}", w[0], w[1]);
    }

    // First two seeded EAST rows are:
    //   EAST/ENG /ALPHA  and EAST/ENG /BETA
    assert_eq!(rows[0].0, b"EAST");
    assert_eq!(rows[0].1, b"ENG ");
    assert_eq!(rows[0].2, b"ALPHA ");
}
