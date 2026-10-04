//! Host tests for `scsiconf.rs`.

use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::scsi::scsi_all::{INQUIRY, ScsiInquiry};

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/scsi/scsiconf.h");
    let ours = crate::reftest::assert_defines!(defs;
        DEVID_NONE, DEVID_NAA, DEVID_EUI, DEVID_T10, DEVID_SERIAL, DEVID_WWN, DEVID_F_PRINT,
        SDEV_S_DYING, SDEV_REMOVABLE, SDEV_MEDIA_LOADED, SDEV_READONLY, SDEV_OPEN, SDEV_DBX,
        SDEV_EJECTING, SDEV_ATAPI, SDEV_UMASS, SDEV_VIRTUAL, SDEV_OWN_IOPL, SDEV_UFI,
        SDEV_AUTOSAVE, SDEV_NOSYNC, SDEV_NOWIDE, SDEV_NOTAGS, SDEV_NOSYNCCACHE, ADEV_NOSENSE,
        ADEV_LITTLETOC, ADEV_NOCAPACITY, ADEV_NODOORLOCK, SDEV_NO_ADAPTER_TARGET,
        SCSI_NOSLEEP, SCSI_POLL, SCSI_AUTOCONF, ITSDONE, SCSI_SILENT, SCSI_IGNORE_NOT_READY,
        SCSI_IGNORE_MEDIA_CHANGE, SCSI_IGNORE_ILLEGAL_REQUEST, SCSI_RESET, SCSI_DATA_IN,
        SCSI_DATA_OUT, SCSI_TARGET, SCSI_ESCAPE, SCSI_PRIVATE, SCSI_OP_TARGET, SCSI_OP_RESET,
        SCSI_OP_BDINFO, XS_NOERROR, XS_SENSE, XS_DRIVER_STUFFUP, XS_SELTIMEOUT, XS_TIMEOUT,
        XS_BUSY, XS_SHORTSENSE, XS_RESET, TEST_READY_RETRIES, SCSI_RETRIES, SCSI_REV_0,
        SCSI_REV_1, SCSI_REV_2, SCSI_REV_SPC, SCSI_REV_SPC2, SCSI_REV_SPC3, SCSI_REV_SPC4,
        SCSI_REV_SPC5,
    );
    crate::reftest::assert_complete(&defs, "XS_", &ours);
    crate::reftest::assert_complete(&defs, "SCSI_REV_", &ours);
    assert_eq!(SCSI_IOPOOL_POISON.as_ptr() as usize, 0x5c5);
}

#[test]
fn big_endian_helpers_round_trip() {
    let mut b = [0u8; 8];
    _lto2b(0x1234, &mut b);
    assert_eq!(&b[..2], &[0x12, 0x34]);
    assert_eq!(_2btol(&b), 0x1234);
    _lto3b(0x00ab_cdef, &mut b);
    assert_eq!(&b[..3], &[0xab, 0xcd, 0xef]);
    assert_eq!(_3btol(&b), 0x00ab_cdef);
    _lto4b(0xdead_beef, &mut b);
    assert_eq!(&b[..4], &[0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(_4btol(&b), 0xdead_beef);
    _lto8b(0x0102_0304_0506_0708, &mut b);
    assert_eq!(b, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(_8btol(&b), 0x0102_0304_0506_0708);
    assert_eq!(_5btol(&b), 0x01_0203_0405);
    // The high bits beyond the field are dropped, as the C's masks do.
    _lto2b(0xffff_1234, &mut b);
    assert_eq!(_2btol(&b), 0x1234);
}

/// A `devid_alloc`-shaped allocation: the header and the identifier bytes after it.
fn devid(d_type: u8, id: &[u8]) -> Vec<u8> {
    let mut v = vec![d_type, 0, 1, id.len() as u8];
    v.extend_from_slice(id);
    v
}

fn as_devid(v: &[u8]) -> &Devid {
    // SAFETY: `Devid` is four bytes with alignment 1, and the vector holds the header and
    // `d_len` bytes after it, like a `devid_alloc` allocation.
    unsafe { &*v.as_ptr().cast::<Devid>() }
}

#[test]
fn devid_cmp_compares_type_length_and_bytes() {
    let a = devid(DEVID_NAA, &[1, 2, 3, 4]);
    let b = devid(DEVID_NAA, &[1, 2, 3, 4]);
    let c = devid(DEVID_NAA, &[1, 2, 3, 5]);
    let d = devid(DEVID_EUI, &[1, 2, 3, 4]);
    let n = devid(DEVID_NONE, &[1, 2, 3, 4]);
    let n2 = devid(DEVID_NONE, &[1, 2, 3, 4]);
    // SAFETY: every argument is a `devid`-shaped allocation (see `devid`).
    unsafe {
        assert!(devid_cmp(Some(as_devid(&a)), Some(as_devid(&b))));
        assert!(!devid_cmp(Some(as_devid(&a)), Some(as_devid(&c))));
        assert!(!devid_cmp(Some(as_devid(&a)), Some(as_devid(&d))));
        assert!(!devid_cmp(Some(as_devid(&n)), Some(as_devid(&n2))));
        assert!(devid_cmp(Some(as_devid(&n)), Some(as_devid(&n))));
        assert!(!devid_cmp(None, Some(as_devid(&a))));
        assert!(!devid_cmp(Some(as_devid(&a)), None));
        assert_eq!(as_devid(&a).id(), &[1, 2, 3, 4]);
    }
}

#[test]
fn xfer_cmd_views() {
    let xs = ScsiXfer::new();
    xs.with_cmd(|cmd: &mut ScsiInquiry| {
        cmd.opcode = INQUIRY;
        _lto2b(36, &mut cmd.length);
    });
    assert_eq!(xs.cmd.get().opcode, INQUIRY);
    let inq: ScsiInquiry = xs.cmd_as();
    assert_eq!(_2btol(&inq.length), 36);

    let mut cdb = ScsiInquiry::default();
    cdb.opcode = 0x55;
    xs.set_cmd(&cdb);
    assert_eq!(xs.cmd.get().opcode, 0x55);
    // The bytes past the CDB are left alone.
    assert_eq!(xs.cmd.get().bytes[5], 0);
}

#[test]
fn xfer_data() {
    let xs = ScsiXfer::new();
    let mut buf = [0u8; 4];
    // SAFETY: `buf` outlives every use of the data below and nothing else touches it.
    unsafe { xs.set_data(buf.as_mut_ptr(), 4) };
    assert_eq!(xs.datalen(), 4);
    // SAFETY: this test is the transfer's only user.
    unsafe { xs.data_slice()[1] = 7 };
    xs.clear_data();
    // SAFETY: as above.
    assert!(unsafe { xs.data_slice() }.is_empty());
    assert_eq!(buf, [0, 7, 0, 0]);
}

#[test]
fn inquiry_revision_helpers() {
    let mut inq = crate::scsi::scsi_all::ScsiInquiryData::new();
    inq.version = 0x05 | 0x08;
    inq.response_format = 0x12;
    assert_eq!(sid_ansii_rev(&inq), SCSI_REV_SPC3);
    assert_eq!(sid_response_format(&inq), 0x02);
}

#[test]
fn new_link_uses_the_default_sense_handler() {
    let link = ScsiLink::new();
    assert!(core::ptr::fn_addr_eq(
        link.interpret_sense.get(),
        scsi_interpret_sense as fn(&'static ScsiXfer) -> Result<(), Errno>
    ));
    assert!(link.pool.get().is_none());
    assert!(link.id().is_none());
}

/*
 * scsiconf.c
 */

fn strvis(src: &[u8], dstlen: usize) -> std::vec::Vec<u8> {
    let mut dst = vec![0xaa; dstlen];
    scsi_strvis(&mut dst, src);
    let n = dst.iter().position(|&c| c == 0).expect("NUL-terminated");
    dst.truncate(n);
    dst
}

#[test]
fn strvis_trims_collapses_and_quotes() {
    assert_eq!(strvis(b"QEMU    ", 33), b"QEMU");
    assert_eq!(strvis(b"  \0QEMU  HARDDISK\xff\xff", 65), b"QEMU HARDDISK");
    assert_eq!(strvis(b"a\t\n\0b", 21), b"a b");
    assert_eq!(strvis(b"a\\b", 13), b"a\\\\b");
    assert_eq!(strvis(b"x\x01\x80y", 17), b"x\\001\\200y");
    assert_eq!(strvis(b"", 1), b"");
    assert_eq!(strvis(b"    ", 17), b"");
    // A short destination keeps what fits and its NUL.
    assert_eq!(strvis(b"abcdef", 4), b"abc");
    // No room at all: nothing is written.
    let mut none: [u8; 0] = [];
    scsi_strvis(&mut none, b"abc");
}

fn inquiry(
    device: u8,
    removable: bool,
    vendor: &[u8],
    product: &[u8],
    rev: &[u8],
) -> ScsiInquiryData {
    let mut inq = ScsiInquiryData::new();
    inq.device = device;
    inq.dev_qual2 = if removable { SID_REMOVABLE } else { 0 };
    inq.vendor.fill(b' ');
    inq.product.fill(b' ');
    inq.revision.fill(b' ');
    inq.vendor[..vendor.len()].copy_from_slice(vendor);
    inq.product[..product.len()].copy_from_slice(product);
    inq.revision[..rev.len()].copy_from_slice(rev);
    inq
}

#[test]
fn inqmatch_finds_the_quirks() {
    let inq = inquiry(T_CDROM, true, b"PLEXTOR ", b"CD-ROM PX-40TS", b"1.01");
    let (m, pri) = scsi_inqmatch(&inq, &SCSI_QUIRK_PATTERNS);
    assert_eq!(m.expect("PLEXTOR").quirks, SDEV_NOSYNC);
    assert_eq!(pri, 2 + 7 + 14 + 4);

    // A vendor pattern longer than the field runs on into the product, as the C's bcmp.
    let inq = inquiry(T_CDROM, true, b"MATSHITA", b" CR-574", b"1.06");
    let (m, pri) = scsi_inqmatch(&inq, &SCSI_QUIRK_PATTERNS);
    let m = m.expect("MATSHITA");
    assert_eq!(m.pattern.revision, b"1.06");
    assert_eq!(m.quirks, ADEV_NOCAPACITY);
    assert_eq!(pri, 2 + 15 + 4);

    // Removability and type must match.
    let inq = inquiry(T_CDROM, false, b"PLEXTOR ", b"CD-ROM PX-40TS", b"1.01");
    assert_eq!(scsi_inqmatch(&inq, &SCSI_QUIRK_PATTERNS).1, 0);
    let inq = inquiry(T_DIRECT, false, b"QEMU    ", b"QEMU HARDDISK", b"2.5+");
    let (m, pri) = scsi_inqmatch(&inq, &SCSI_QUIRK_PATTERNS);
    assert!(m.is_none());
    assert_eq!(pri, 0);
}

#[test]
fn inqmatch_prefers_the_longest_match() {
    static PATTERNS: [ScsiInquiryPattern; 3] = [
        ScsiInquiryPattern {
            r#type: T_DIRECT,
            removable: T_FIXED,
            vendor: b"",
            product: b"",
            revision: b"",
        },
        ScsiInquiryPattern {
            r#type: T_DIRECT,
            removable: T_FIXED,
            vendor: b"IBM",
            product: b"",
            revision: b"",
        },
        ScsiInquiryPattern {
            r#type: T_DIRECT,
            removable: T_REMOV,
            vendor: b"IBM",
            product: b"DCAS",
            revision: b"",
        },
    ];
    let inq = inquiry(T_DIRECT, false, b"IBM     ", b"DCAS-32160", b"S65A");
    let (m, pri) = scsi_inqmatch(&inq, &PATTERNS);
    assert!(core::ptr::eq(m.expect("IBM"), &PATTERNS[1]));
    assert_eq!(pri, 5);
    // The generic sd pattern alone gives 2.
    assert_eq!(scsi_inqmatch(&inq, &PATTERNS[..1]).1, 2);
    // Quirk table: IBM DCAS is NOTAGS.
    let (m, _) = scsi_inqmatch(&inq, &SCSI_QUIRK_PATTERNS);
    assert_eq!(m.expect("DCAS").quirks, SDEV_NOTAGS);
}

#[test]
fn devid_alloc_copy_free() {
    let _g = crate::kern::subr_pool::tests::setup_real_memory();
    let d = devid_alloc(DEVID_SERIAL, DEVID_F_PRINT, 5, b"abcdefgh").expect("devid");
    // SAFETY: a live devid_alloc allocation.
    let dr = unsafe { d.as_ref() };
    assert_eq!(
        (dr.d_type, dr.d_flags, dr.d_len, dr.d_refcount.get()),
        (4, 1, 5, 1)
    );
    // SAFETY: as above.
    assert_eq!(unsafe { dr.id() }, b"abcde");
    let d2 = devid_copy(dr);
    assert_eq!(d2, d);
    assert_eq!(dr.d_refcount.get(), 2);
    // SAFETY: the copy's reference, then the original's.
    unsafe {
        devid_free(d2);
        assert_eq!(dr.d_refcount.get(), 1);
        devid_free(d);
    }

    // Missing identifier bytes stay zero.
    let d = devid_alloc(DEVID_WWN, 0, 4, b"ab").expect("devid");
    // SAFETY: a live devid_alloc allocation, freed last.
    unsafe {
        assert_eq!(d.as_ref().id(), &[b'a', b'b', 0, 0]);
        devid_free(d);
    }
}

#[test]
fn devid_wwn_only_for_lun0_with_a_name() {
    let _g = crate::kern::subr_pool::tests::setup_real_memory();
    let link = ScsiLink::new();
    assert_eq!(scsi_devid_wwn(&link), Err(EOPNOTSUPP));
    link.node_wwn.set(0x5000_c500_1234_5678);
    link.lun.set(1);
    assert_eq!(scsi_devid_wwn(&link), Err(EOPNOTSUPP));
    link.lun.set(0);
    assert_eq!(scsi_devid_wwn(&link), Ok(()));
    let d = link.id.get().expect("an id");
    // SAFETY: the link's devid_alloc allocation, freed at the end.
    unsafe {
        assert_eq!(d.as_ref().d_type, DEVID_WWN);
        assert_eq!(
            d.as_ref().id(),
            &[0x50, 0x00, 0xc5, 0x00, 0x12, 0x34, 0x56, 0x78]
        );
        devid_free(d);
    }
}

/*
 * The probe against a fake adapter: target 0 is a disk with one LUN and VPD pages, every
 * other target times out.
 */

mod probe {
    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::cell::RefCell;
    use std::sync::MutexGuard;
    use std::sync::atomic::AtomicUsize;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::super::*;
    use crate::kern::subr_autoconf::config_init;
    use crate::kern::subr_pool::{pool_destroy, pool_init};
    use crate::machine::Machine;
    use crate::machine::intr::IPL_BIO;
    use crate::scsi::scsi_all::{INQUIRY, REPORT_LUNS, SI_EVPD};
    use crate::scsi::scsi_base::{SCSI_XFER_POOL, scsi_copy_internal_data, scsi_done};
    use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR};

    std::thread_local! {
        /// The opcodes the fake adapter got, with their target and LUN.
        static SEEN: RefCell<Vec<(u16, u16, u8)>> = const { RefCell::new(Vec::new()) };
    }

    static TDISK_ATTACHED: AtomicUsize = AtomicUsize::new(0);
    static TDISK_DETACHED: AtomicUsize = AtomicUsize::new(0);

    fn std_inquiry() -> Vec<u8> {
        let mut inq = ScsiInquiryData::new();
        inq.device = T_DIRECT;
        inq.version = SCSI_REV_SPC3;
        inq.response_format = 2;
        inq.additional_length = 31;
        inq.flags = SID_CmdQue;
        inq.vendor.copy_from_slice(b"QEMU    ");
        inq.product.copy_from_slice(b"QEMU HARDDISK   ");
        inq.revision.copy_from_slice(b"2.5+");
        inq.as_bytes()[..36].to_vec()
    }

    fn vpd(page: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![T_DIRECT, page, 0, body.len() as u8];
        v.extend_from_slice(body);
        v
    }

    fn fake_cmd(xs: &'static ScsiXfer) {
        let link = xs.link();
        let cmd = xs.cmd.get();
        SEEN.with(|s| {
            s.borrow_mut()
                .push((link.target.get(), link.lun.get(), cmd.opcode))
        });
        if link.target.get() != 0 {
            xs.error.set(XS_SELTIMEOUT);
            scsi_done(xs);
            return;
        }
        let reply = match cmd.opcode {
            INQUIRY if cmd.bytes[0] & SI_EVPD != 0 => match cmd.bytes[1] {
                SI_PG_SUPPORTED => vpd(SI_PG_SUPPORTED, &[0x00, SI_PG_SERIAL, SI_PG_DEVID]),
                // A T10 vendor designator, then an NAA one (binary), which wins.
                SI_PG_DEVID => vpd(
                    SI_PG_DEVID,
                    &[
                        0x02, 0x01, 0, 4, b'Q', b'E', b'M', b'U', // T10, ASCII
                        0x01, 0x03, 0, 8, 0x60, 1, 2, 3, 4, 5, 6, 7, // NAA, binary
                    ],
                ),
                SI_PG_SERIAL => vpd(SI_PG_SERIAL, b"SN42"),
                _ => Vec::new(),
            },
            INQUIRY => std_inquiry(),
            REPORT_LUNS => {
                let mut r = vec![0u8; 16];
                r[3] = 8; // one LUN: 0
                r
            }
            _ => Vec::new(),
        };
        if !reply.is_empty() && xs.datalen() > 0 {
            scsi_copy_internal_data(xs, &reply);
        }
        scsi_done(xs);
    }

    static FAKE_ADAPTER: ScsiAdapter = ScsiAdapter {
        scsi_cmd: fake_cmd,
        dev_minphys: None,
        dev_probe: None,
        dev_free: None,
        ioctl: None,
    };

    fn tdisk_match(_parent: Option<&Device>, _m: &CfMatch, aux: *mut c_void) -> i32 {
        // SAFETY: scsibus searches with a `ScsiAttachArgs`.
        let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
        i32::from(sa.sa_sc_link.inqdata.get().device & SID_TYPE == T_DIRECT)
    }

    fn tdisk_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
        // SAFETY: as in `tdisk_match`.
        let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
        sa.sa_sc_link
            .device_softc
            .set(Some(core::ptr::NonNull::from(self_)));
        TDISK_ATTACHED.fetch_add(1, Ordering::Relaxed);
    }

    fn tdisk_detach(_dev: &Device, _flags: i32) -> Result<(), Errno> {
        TDISK_DETACHED.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Real memory, the transfer pool, a test `ioconf` (`scsibus0` and `tdisk* at
    /// scsibus?`) and a bus on the fake adapter with `buswidth` targets.
    fn setup(buswidth: u16) -> (MutexGuard<'static, ()>, &'static ScsibusSoftc) {
        let g = crate::kern::subr_pool::tests::setup_real_memory();
        pool_init(
            &SCSI_XFER_POOL,
            size_of::<ScsiXfer>(),
            0,
            IPL_BIO,
            0,
            "scxspl",
            None,
        );
        let tdisk_ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
            ca_devsize: size_of::<Device>(),
            ca_match: Some(tdisk_match),
            ca_attach: tdisk_attach,
            ca_detach: Some(tdisk_detach),
            ca_activate: None,
        }));
        let tdisk_cd: &'static Cfdriver = Box::leak(Box::new(Cfdriver::new(b"tdisk", DV_DULL, 0)));
        let table: &'static [Cfdata] = Box::leak(Box::new([
            Cfdata::new(
                &SCSIBUS_CA,
                &SCSIBUS_CD,
                0,
                FSTATE_NOTFOUND,
                &[],
                0,
                &[],
                0,
                0,
            ),
            Cfdata::new(tdisk_ca, tdisk_cd, 0, FSTATE_STAR, &[-1, -1], 0, &[0], 0, 0),
        ]));
        // SAFETY: `setup_real_memory`'s lock serialises the tests.
        unsafe { Machine::set_ioconf(table, &[]) };
        config_init();
        TDISK_ATTACHED.store(0, Ordering::Relaxed);
        TDISK_DETACHED.store(0, Ordering::Relaxed);
        SEEN.with(|s| s.borrow_mut().clear());

        // SAFETY: a fresh zeroed allocation of the softc's layout, leaked; all-zero is a
        // valid `ScsibusSoftc` (its `Softc` impl).
        let sb = unsafe { &*alloc_zeroed(Layout::new::<ScsibusSoftc>()).cast::<ScsibusSoftc>() };
        sb.sc_dev.dv_cfdata.set(Some(&table[0]));
        let mut name = [0u8; 16];
        name[..8].copy_from_slice(b"scsibus0");
        sb.sc_dev.dv_xname.set(name);
        sb.sb_adapter.set(Some(&FAKE_ADAPTER));
        sb.sb_adapter_buswidth.set(buswidth);
        sb.sb_adapter_target.set(SDEV_NO_ADAPTER_TARGET);
        sb.sb_luns.set(8);
        sb.sb_openings.set(4);
        (g, sb)
    }

    fn teardown() {
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 0);
        pool_destroy(&SCSI_XFER_POOL);
    }

    #[test]
    fn probe_bus_attaches_the_disk_and_detach_frees_it() {
        let (_g, sb) = setup(2);

        assert_eq!(scsi_probe_bus(sb), Ok(()));
        assert_eq!(TDISK_ATTACHED.load(Ordering::Relaxed), 1);

        let link = scsi_get_link(sb, 0, 0).expect("the disk's link");
        assert!(scsi_get_link(sb, 1, 0).is_none());
        assert!(scsi_get_link(sb, 0, 1).is_none());
        assert!(link.device_softc.get().is_some());
        assert_eq!(link.flags.get() & SDEV_OWN_IOPL, SDEV_OWN_IOPL);
        assert_eq!(link.flags.get() & SDEV_REMOVABLE, 0);
        // SPC-3 with CmdQue: no NOTAGS; openings from the bus.
        assert_eq!(link.quirks.get() & SDEV_NOTAGS, 0);
        assert_eq!(link.openings.get(), 4);
        assert_eq!(&link.inqdata.get().vendor, b"QEMU    ");
        // The NAA designator of page 0x83 is the device id.
        let id = link.id().expect("a devid");
        assert_eq!(id.d_type, DEVID_NAA);
        assert_eq!(id.d_flags, 0);
        // SAFETY: the link's devid_alloc allocation.
        assert_eq!(unsafe { id.id() }, &[0x60, 1, 2, 3, 4, 5, 6, 7]);

        // REPORT LUNS was asked of target 0; target 1 timed out at INQUIRY.
        let seen = SEEN.with(|s| s.borrow().clone());
        assert!(seen.contains(&(0, 0, REPORT_LUNS)));
        assert!(seen.contains(&(1, 0, INQUIRY)));
        assert!(!seen.iter().any(|&(t, l, _)| t == 1 && l != 0));

        // Probing again finds the slot taken.
        assert_eq!(scsi_probe_lun(sb, 0, 0), Ok(()));
        assert_eq!(scsi_probe_target(sb, 1), Err(EINVAL));
        assert_eq!(scsi_probe_lun(sb, 0xffff, 0), Err(EINVAL));
        assert_eq!(TDISK_ATTACHED.load(Ordering::Relaxed), 1);

        // An open device does not detach without force.
        link.flags.set(link.flags.get() | SDEV_OPEN);
        assert_eq!(scsi_detach_lun(sb, 0, 0, 0), Err(EBUSY));
        link.flags.set(link.flags.get() & !SDEV_OPEN);
        assert_eq!(scsi_detach_lun(sb, 0, 1, 0), Err(EINVAL));

        assert_eq!(scsi_detach(sb, -1, -1, 0), Ok(()));
        assert_eq!(TDISK_DETACHED.load(Ordering::Relaxed), 1);
        assert!(sb.sc_link_list.is_empty());
        teardown();
    }

    #[test]
    fn a_device_no_driver_wants_is_not_configured() {
        let (_g, sb) = setup(1);
        // Make the disk a processor: tdisk does not match it.
        fn no_match(_p: Option<&Device>, _m: &CfMatch, _aux: *mut c_void) -> i32 {
            0
        }
        let cf = &crate::machine::autoconf::cfdata()[1];
        let ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
            ca_devsize: size_of::<Device>(),
            ca_match: Some(no_match),
            ca_attach: tdisk_attach,
            ca_detach: None,
            ca_activate: None,
        }));
        let table: &'static [Cfdata] = Box::leak(Box::new([
            Cfdata::new(
                &SCSIBUS_CA,
                &SCSIBUS_CD,
                0,
                FSTATE_NOTFOUND,
                &[],
                0,
                &[],
                0,
                0,
            ),
            Cfdata::new(ca, cf.cf_driver, 0, FSTATE_STAR, &[-1, -1], 0, &[0], 0, 0),
        ]));
        // SAFETY: the test still holds `setup_real_memory`'s lock.
        unsafe { Machine::set_ioconf(table, &[]) };
        sb.sc_dev.dv_cfdata.set(Some(&table[0]));

        assert_eq!(scsi_probe(sb, 0, 0), Ok(()));
        assert!(scsi_get_link(sb, 0, 0).is_none());
        assert_eq!(TDISK_ATTACHED.load(Ordering::Relaxed), 0);
        teardown();
    }

    #[test]
    fn submatch_checks_the_locators() {
        let (_g, sb) = setup(1);
        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        link.bus.set(Some(sb));
        link.target.set(3);
        link.lun.set(1);
        let mut inq = ScsiInquiryData::new();
        inq.device = T_DIRECT;
        link.inqdata.set(inq);
        let mut sa = ScsiAttachArgs { sa_sc_link: link };
        let aux: *mut c_void = core::ptr::from_mut(&mut sa).cast();

        let at = |loc: &'static [i64]| {
            let cf: &'static Cfdata = Box::leak(Box::new(Cfdata::new(
                crate::machine::autoconf::cfdata()[1].cf_attach,
                crate::machine::autoconf::cfdata()[1].cf_driver,
                0,
                FSTATE_STAR,
                loc,
                0,
                &[0],
                0,
                0,
            )));
            scsibussubmatch(Some(&sb.sc_dev), &CfMatch::Cfdata(cf), aux)
        };
        assert_eq!(at(&[-1, -1]), 1);
        assert_eq!(at(&[3, -1]), 1);
        assert_eq!(at(&[3, 1]), 1);
        assert_eq!(at(&[2, -1]), 0);
        assert_eq!(at(&[3, 0]), 0);
        teardown();
    }
}
