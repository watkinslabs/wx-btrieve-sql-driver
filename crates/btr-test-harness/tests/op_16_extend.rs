use btr_test_harness as h;
use wxbtrv_core::constants::BTR_UNSUPPORTED_OP;

/// Op 16 (Extend) was deprecated in Btrieve 6.0+ and the dispatcher returns
/// BTR_UNSUPPORTED_OP (20) explicitly.
#[test]
fn op_16_extend_unsupported() {
    h::reset_fixture();
    let mut posblk = h::new_posblk();
    let mut data = vec![0u8; 16];
    let mut dlen: u32 = 0;
    let mut key = [0u8; 64];
    let rc = h::btrcall(16, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(
        rc, BTR_UNSUPPORTED_OP,
        "op 16 should return unsupported (20)"
    );
}
