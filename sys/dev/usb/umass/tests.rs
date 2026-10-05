use super::*;
use std::boxed::Box;

fn softc(cmd: u8) -> &'static UmassSoftc {
    // SAFETY: all-zero is a valid `UmassSoftc` (the `Softc` contract).
    let sc: &'static UmassSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
    sc.sc_cmd.set(cmd);
    sc
}

#[test]
fn ufi_trims_the_inquiry_and_pads_the_command() {
    let sc = softc(UMASS_CPROTO_UFI);
    let mut cbw = UmassBbbCbw::default();
    cbw.bCDBLength = 6;
    cbw.CBWCDB[0] = INQUIRY;
    cbw.CBWCDB[4] = 255;
    sc.cbw.set(cbw);
    sc.transfer_datalen.set(255);
    umass_adjust_transfer(sc);
    assert_eq!(sc.transfer_datalen.get(), 36);
    assert_eq!(sc.cbw.get().CBWCDB[4], 36);
    assert_eq!(sc.cbw.get().bCDBLength, UFI_COMMAND_LENGTH);
}

#[test]
fn atapi_pads_and_scsi_keeps_the_command() {
    let sc = softc(UMASS_CPROTO_ATAPI);
    sc.cbw.set(UmassBbbCbw {
        bCDBLength: 10,
        ..UmassBbbCbw::default()
    });
    umass_adjust_transfer(sc);
    assert_eq!(sc.cbw.get().bCDBLength, 12);
    let sc = softc(UMASS_CPROTO_SCSI);
    sc.cbw.set(UmassBbbCbw {
        bCDBLength: 10,
        ..UmassBbbCbw::default()
    });
    umass_adjust_transfer(sc);
    assert_eq!(sc.cbw.get().bCDBLength, 10);
}

#[test]
fn copies_stay_within_the_transfer() {
    let sc = softc(UMASS_CPROTO_SCSI);
    sc.transfer_datalen.set(512);
    assert_eq!(umass_copy_len(sc, 100), 100);
    assert_eq!(umass_copy_len(sc, 4096), 512);
    assert_eq!(umass_copy_len(sc, -1), 0);
    assert_eq!(offset_of!(UmassBbbCbw, CBWCDB), 15);
}
