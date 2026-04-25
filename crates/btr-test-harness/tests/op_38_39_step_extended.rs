use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

fn build_sne() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&16u16.to_le_bytes()); // descriptionLen
    d.push(0); // currencyConst
    d.push(0); // reserved
    d.extend_from_slice(&0u16.to_le_bytes()); // rejectCount
    d.extend_from_slice(&0u16.to_le_bytes()); // numberTerms
    d.extend_from_slice(&5u16.to_le_bytes()); // maxRecs
    d.extend_from_slice(&1u16.to_le_bytes()); // noFields
    d.extend_from_slice(&8u16.to_le_bytes()); // fieldLen
    d.extend_from_slice(&0u16.to_le_bytes()); // fieldOffset
    d
}

#[test]
fn op_38_step_next_extended_after_step_first() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    // Step First to establish physical currency.
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(33, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step first");

    let desc = build_sne();
    let mut buf = vec![0u8; 1024];
    buf[..desc.len()].copy_from_slice(&desc);
    let mut dlen2: u32 = buf.len() as u32;
    let rc = h::btrcall(38, &mut posblk, &mut buf, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step next extended rc");

    let num = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    assert!(num > 0 && num <= 5, "numReturned={}", num);
}

#[test]
fn op_39_step_prev_extended_after_step_last() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");

    let mut data = vec![0u8; 256];
    let mut dlen: u32 = data.len() as u32;
    let mut key = [0u8; 64];
    let rc = h::btrcall(34, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step last");

    let desc = build_sne();
    let mut buf = vec![0u8; 1024];
    buf[..desc.len()].copy_from_slice(&desc);
    let mut dlen2: u32 = buf.len() as u32;
    let rc = h::btrcall(39, &mut posblk, &mut buf, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "step prev extended rc");

    let num = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    assert!(num > 0 && num <= 5, "numReturned={}", num);
}
