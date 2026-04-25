//! Get Key (+50 bias) tests — ops 55..=63.
//!
//! Each test asserts rc=0 and that *data_len is zeroed (no record data
//! returned; only the key value is updated in key_buf).

use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;

fn open() -> Box<[u8; 128]> {
    h::reset_fixture();
    let (posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");
    posblk
}

#[test]
fn op_55_get_key_equal() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(55, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 55 rc");
    assert_eq!(dlen, 0, "op 55 dlen_out should be 0");
}

#[test]
fn op_56_get_key_next() {
    let mut posblk = open();
    // Establish currency first via op 55.
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(55, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "seed op 55 rc");

    let mut dlen2: u32 = 256;
    let rc = h::btrcall(56, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 56 rc");
    assert_eq!(dlen2, 0, "op 56 dlen_out should be 0");
}

#[test]
fn op_57_get_key_prev() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(55, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "seed op 55 rc");

    let mut dlen2: u32 = 256;
    let rc = h::btrcall(57, &mut posblk, &mut data, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 57 rc");
    assert_eq!(dlen2, 0, "op 57 dlen_out should be 0");
}

#[test]
fn op_58_get_key_greater() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(58, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 58 rc");
    assert_eq!(dlen, 0, "op 58 dlen_out should be 0");
}

#[test]
fn op_59_get_key_ge() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(59, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 59 rc");
    assert_eq!(dlen, 0, "op 59 dlen_out should be 0");
}

#[test]
fn op_60_get_key_less() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(60, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 60 rc");
    assert_eq!(dlen, 0, "op 60 dlen_out should be 0");
}

#[test]
fn op_61_get_key_le() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    key[..8].copy_from_slice(b"A0005   ");
    let rc = h::btrcall(61, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 61 rc");
    assert_eq!(dlen, 0, "op 61 dlen_out should be 0");
}

#[test]
fn op_62_get_key_first() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(62, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 62 rc");
    assert_eq!(dlen, 0, "op 62 dlen_out should be 0");
}

#[test]
fn op_63_get_key_last() {
    let mut posblk = open();
    let mut data = vec![0u8; 256];
    let mut dlen: u32 = 256;
    let mut key = [0u8; 80];
    let rc = h::btrcall(63, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "op 63 rc");
    assert_eq!(dlen, 0, "op 63 dlen_out should be 0");
}
