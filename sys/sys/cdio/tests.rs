use super::*;

#[test]
fn cdioreadmsaddr_is_an_inout_int_request() {
    // _IOWR('c', 6, int): IOC_INOUT | sizeof(int) << 16 | 'c' << 8 | 6.
    assert_eq!(CDIOREADMSADDR, 0xc004_6306);
}

#[test]
fn ioctl_numbers_encode_the_argument_sizes() {
    assert_eq!(CDIOCPLAYTRACKS, 0x8004_6301);
    assert_eq!(CDIOCPLAYBLOCKS, 0x8008_6302);
    assert_eq!(CDIOCREADSUBCHANNEL, 0xc010_6303);
    assert_eq!(CDIOREADTOCHEADER, 0x4004_6304);
    assert_eq!(CDIOREADTOCENTRIES, 0xc010_6305);
    assert_eq!(CDIOCEJECT, 0x2000_6318);
    assert_eq!(CDIOCALLOW, 0x2000_6319);
    assert_eq!(CDIOCPLAYMSF, 0x8006_6319);
    assert_eq!(CDIOCLOADUNLOAD, 0x8002_631a);
    assert_eq!(DVD_READ_STRUCT, 0xc808_6400);
    assert_eq!(DVD_AUTH, 0xc010_6402);
}

#[test]
fn toc_entry_nibbles_and_msf_views() {
    let mut e = CdTocEntry::default();
    e.set_control(4);
    e.set_addr_type(1);
    assert_eq!((e.control(), e.addr_type(), e.ac), (4, 1, 0x14));
    e.addr.set_lba(0x0102_0304);
    assert_eq!(e.addr.lba(), 0x0102_0304);
    let m = MsfLba {
        addr: [0, 12, 34, 56],
    };
    assert_eq!((m.minute(), m.second(), m.frame()), (12, 34, 56));
}

#[test]
fn unions_start_with_the_type_byte() {
    let mut s = DvdStruct::zeroed();
    s.disckey().r#type = DVD_STRUCT_DISCKEY;
    assert_eq!(s.r#type(), DVD_STRUCT_DISCKEY);
    let mut a = DvdAuthinfo::zeroed();
    a.set_type(DVD_LU_SEND_AGID);
    a.lsa().agid = 3;
    assert_eq!(a.r#type(), DVD_LU_SEND_AGID);
    assert_eq!(a.lsa().agid, 3);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/cdio.h");
    crate::reftest::assert_defines!(defs;
        CD_AS_AUDIO_INVALID, CD_AS_PLAY_IN_PROGRESS, CD_AS_PLAY_PAUSED,
        CD_AS_PLAY_COMPLETED, CD_AS_PLAY_ERROR, CD_AS_NO_STATUS, CD_LBA_FORMAT,
        CD_MSF_FORMAT, CD_SUBQ_DATA, CD_CURRENT_POSITION, CD_MEDIA_CATALOG, CD_TRACK_INFO,
        CD_TRACK_LEADOUT, CD_LU_ABORT, CD_LU_UNLOAD, CD_LU_LOAD, GPCMD_READ_DVD_STRUCTURE,
        GPCMD_SEND_DVD_STRUCTURE, GPCMD_REPORT_KEY, GPCMD_SEND_KEY, DVD_STRUCT_PHYSICAL,
        DVD_STRUCT_COPYRIGHT, DVD_STRUCT_DISCKEY, DVD_STRUCT_BCA, DVD_STRUCT_MANUFACT,
        DVD_LU_SEND_AGID, DVD_HOST_SEND_CHALLENGE, DVD_LU_SEND_KEY1, DVD_LU_SEND_CHALLENGE,
        DVD_HOST_SEND_KEY2, DVD_AUTH_ESTABLISHED, DVD_AUTH_FAILURE, DVD_LU_SEND_TITLE_KEY,
        DVD_LU_SEND_ASF, DVD_INVALIDATE_AGID, DVD_LU_SEND_RPC_STATE,
        DVD_HOST_SEND_RPC_STATE, DVD_KEY_SIZE, DVD_CHALLENGE_SIZE, DVD_CPM_NO_COPYRIGHT,
        DVD_CPM_COPYRIGHTED, DVD_CP_SEC_NONE, DVD_CP_SEC_EXIST, DVD_CGMS_UNRESTRICTED,
        DVD_CGMS_SINGLE, DVD_CGMS_RESTRICTED,
    );
}
