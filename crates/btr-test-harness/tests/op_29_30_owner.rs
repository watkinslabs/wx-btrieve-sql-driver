use btr_test_harness as h;
use wxbtrv_core::constants::BTR_SUCCESS;
use wxbtrv_core::state::state;

#[test]
fn op_29_30_set_and_clear_owner() {
    h::reset_fixture();
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open");
    let hid = u32::from_le_bytes([posblk[0], posblk[1], posblk[2], posblk[3]]);

    // Set owner (op 29): owner name "secret\0" in data buffer.
    let mut data = vec![0u8; 16];
    let owner = b"secret\0";
    data[..owner.len()].copy_from_slice(owner);
    let mut dlen: u32 = owner.len() as u32;
    let mut key = [0u8; 8];
    let rc = h::btrcall(29, &mut posblk, &mut data, &mut dlen, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "set owner rc");

    // Verify via state inspection.
    {
        let st = state().lock().unwrap();
        let h = st.handles.get(&hid).expect("handle exists");
        assert_eq!(h.owner_name.as_deref(), Some("secret"));
    }

    // Clear owner (op 30).
    let mut data2 = vec![0u8; 1];
    let mut dlen2: u32 = 0;
    let rc = h::btrcall(30, &mut posblk, &mut data2, &mut dlen2, &mut key, 0);
    assert_eq!(rc, BTR_SUCCESS, "clear owner rc");

    {
        let st = state().lock().unwrap();
        let h = st.handles.get(&hid).expect("handle exists");
        assert!(h.owner_name.is_none(), "owner should be cleared");
    }
}
