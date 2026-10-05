//! Host tests for `scsi_message.rs`: the message classes and the identify byte, and a
//! reference test of the defines.

use super::*;
use crate::reftest;

#[test]
fn message_classes_and_identify() {
    assert!(is1bytemsg(MSG_CMDCOMPLETE) && is1bytemsg(MSG_MESSAGE_REJECT));
    assert!(is1bytemsg(msg_identify(3, true)) && !is1bytemsg(MSG_EXTENDED));
    assert!(is2bytemsg(MSG_SIMPLE_Q_TAG) && is2bytemsg(MSG_IGN_WIDE_RESIDUE));
    assert!(isextmsg(MSG_EXTENDED) && !isextmsg(MSG_SAVEDATAPOINTER));
    assert_eq!(msg_identify(0, false), 0x80);
    assert_eq!(msg_identify(5, true), 0xc5);
    assert!(msg_isidentify(0xc5) && !msg_isidentify(MSG_DISCONNECT));
}

#[test]
#[ignore = "reads the C reference (just test-ref)"]
fn defines_match_the_reference() {
    let defs = reftest::defines("sys/scsi/scsi_message.h");
    let ours: &[(&str, u8)] = &[
        ("MSG_CMDCOMPLETE", MSG_CMDCOMPLETE),
        ("MSG_EXTENDED", MSG_EXTENDED),
        ("MSG_SAVEDATAPOINTER", MSG_SAVEDATAPOINTER),
        ("MSG_DISCONNECT", MSG_DISCONNECT),
        ("MSG_MESSAGE_REJECT", MSG_MESSAGE_REJECT),
        ("MSG_TERM_IO_PROC", MSG_TERM_IO_PROC),
        ("MSG_QAS_REQUEST", MSG_QAS_REQUEST),
        ("MSG_SIMPLE_Q_TAG", MSG_SIMPLE_Q_TAG),
        ("MSG_ORDERED_Q_TAG", MSG_ORDERED_Q_TAG),
        ("MSG_IGN_WIDE_RESIDUE", MSG_IGN_WIDE_RESIDUE),
        ("MSG_IDENTIFYFLAG", MSG_IDENTIFYFLAG),
        ("MSG_IDENTIFY_DISCFLAG", MSG_IDENTIFY_DISCFLAG),
        ("MSG_IDENTIFY_LUNMASK", MSG_IDENTIFY_LUNMASK),
        ("MSG_EXT_SDTR", MSG_EXT_SDTR),
        ("MSG_EXT_SDTR_LEN", MSG_EXT_SDTR_LEN),
        ("MSG_EXT_WDTR", MSG_EXT_WDTR),
        ("MSG_EXT_WDTR_LEN", MSG_EXT_WDTR_LEN),
        ("MSG_EXT_WDTR_BUS_16_BIT", MSG_EXT_WDTR_BUS_16_BIT),
        ("MSG_EXT_PPR", MSG_EXT_PPR),
        ("MSG_EXT_PPR_LEN", MSG_EXT_PPR_LEN),
        ("MSG_EXT_PPR_PCOMP_EN", MSG_EXT_PPR_PCOMP_EN),
        ("MSG_EXT_PPR_PROT_DT", MSG_EXT_PPR_PROT_DT),
        ("MSG_EXT_PPR_PROT_IUS", MSG_EXT_PPR_PROT_IUS),
    ];
    for &(name, v) in ours {
        assert_eq!(reftest::int(&defs, name), Some(i64::from(v)), "{name}");
    }
}
