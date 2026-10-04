//! Host tests for vnd(4): attach and the open rules of unconfigured units, `VNDIOCSET`'s
//! checks and its cleanup on failure (over testfs), and a configured unit over `memfs`, an
//! in-memory backing file: the fabricated label, reads, writes, sloppy requests, the end of
//! the disk, the label ioctls, `VNDIOCGET`, `VNDIOCCLR` and Blowfish encryption.

use std::boxed::Box;
use std::sync::Mutex;
use std::vec::Vec;
use std::{assert, assert_eq, assert_ne, vec};

use super::*;
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_subr::getnewvnode;
use crate::kern::vfs_subr::tests::testfs::setup_root;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::buf::{B_BUSY, B_DONE};
use crate::sys::disklabel::{DTYPE_VND, RAW_PART, makediskdev};
use crate::sys::stat::{S_IFBLK, S_IFCHR};
use crate::sys::vnode::{
    VREG, VT_NON, VopGetattrArgs, VopInactiveArgs, VopReadArgs, VopWriteArgs, Vops,
};

/// vnd's block major (`bdevsw[14]` on both archs).
const VND_BMAJ: u32 = 14;
/// The backing file's sectors.
const SECTORS: usize = 64;
/// `va_fsid` of memfs: major 4, not vnd's.
const MEMFS_FSID: i64 = 0x0400;
/// `va_fileid` of the backing file.
const MEMFS_INO: u64 = 42;

/// The backing file's bytes.
static FILE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn file<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    f(&mut FILE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn memfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    *ap.a_vap = Vattr::new();
    ap.a_vap.va_type = VREG;
    ap.a_vap.va_size = file(|f| f.len()) as u64;
    ap.a_vap.va_fsid = MEMFS_FSID;
    ap.a_vap.va_fileid = MEMFS_INO;
    Ok(())
}

fn memfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let mut data = file(|f| f.clone());
    let off = (uio.uio_offset as usize).min(data.len());
    let n = uio.uio_resid.min(data.len() - off);
    uiomove(&mut data[off..off + n], uio)
}

fn memfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let off = uio.uio_offset as usize;
    let mut buf = vec![0u8; uio.uio_resid];
    uiomove(&mut buf, uio)?;
    file(|f| {
        if f.len() < off + buf.len() {
            f.resize(off + buf.len(), 0);
        }
        f[off..off + buf.len()].copy_from_slice(&buf);
    });
    Ok(())
}

fn memfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    VOP_UNLOCK(ap.a_vp)
}

/// `vops` of `memfs`: no locking, the file in memory.
static MEMFS_VOPS: Vops = Vops {
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_close: Some(|_| nullop()),
    vop_inactive: Some(memfs_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_getattr: Some(memfs_getattr),
    vop_read: Some(memfs_read),
    vop_write: Some(memfs_write),
    ..Vops::EMPTY
};

/// The current thread for the VOPs' `assert_curproc`, and the `NVND` units.
fn begin(p: &'static Proc) {
    Machine::set_curproc(Machine::curcpu(), p);
    vndattach(NVND);
}

fn end() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// A busy buffer of `len` bytes for `dev`, at `blkno`, reading or writing.
fn buf(dev: Dev, blkno: Daddr, len: usize, read: bool, fill: u8) -> &'static Buf {
    let data: &'static mut Vec<u8> = Box::leak(Box::new(vec![fill; len]));
    let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
    bp.b_data.set(data.as_mut_ptr());
    bp.b_dev.set(dev);
    bp.b_blkno.set(blkno);
    bp.b_bcount.set(len as i64);
    bp.set(B_BUSY | if read { B_READ } else { B_WRITE });
    bp
}

/// The buffer's bytes.
fn bytes(bp: &Buf) -> &'static [u8] {
    // SAFETY: the test's buffer of `b_bcount` bytes, leaked.
    unsafe { core::slice::from_raw_parts(bp.b_data.get(), bp.b_bcount.get() as usize) }
}

/// What `VNDIOCSET` does after opening the file, over a memfs file of `SECTORS` sectors
/// (`i % 251`), then the label read of the first open: unit 0 is configured, labelled, and
/// its in-core label covers the file.
fn configure(p: &Proc, key: Option<&[u8]>) -> &'static VndSoftc {
    file(|f| *f = (0..SECTORS * DEV_BSIZE).map(|i| (i % 251) as u8).collect());
    let vp = getnewvnode(VT_NON, None, &MEMFS_VOPS).expect("a vnode");
    vp.v_type.set(VREG);
    // What vn_open(FREAD | FWRITE) counts, and vndclear's vn_close takes back.
    vp.v_writecount.set(1);
    let cred = vndsetcred(p, vp, &VndIoctl::default()).expect("credentials");

    let sc = vnd_softc(0).expect("vnd0");
    sc.sc_type.set(DTYPE_VND);
    sc.sc_secsize.set(DEV_BSIZE);
    sc.sc_ntracks.set(1);
    sc.sc_nsectors.set(100);
    sc.sc_size.set(SECTORS);
    let mut name = [0u8; VNDNLEN];
    name[..4].copy_from_slice(b"/img");
    sc.sc_file.set(name);
    if let Some(key) = key {
        let ctx: &'static mut BlfCtx = Box::leak(Box::default());
        blf_key(ctx, key);
        sc.sc_keyctx.set(Some(NonNull::from(ctx)));
    }
    sc.sc_vp.set(Some(vp));
    sc.sc_cred.set(Some(cred));
    sc.sc_flags.set(VNF_INITED);
    sc.sc_dk.dk_name.set(*b"vnd0\0\0\0\0\0\0\0\0\0\0\0\0");
    disk_attach(Some(&sc.sc_dev), &sc.sc_dk);

    // The host has no `readdisklabel`; the label vndstrategy checks against is the
    // fabricated one, published (initialised) before the read.
    let mut lp = Disklabel::zeroed();
    let raw = makediskdev(VND_BMAJ, 0, RAW_PART);
    assert_eq!(vndgetdisklabel(raw, sc, &mut lp, false), Err(Errno::ENODEV));
    assert_eq!(&lp.d_typename[..10], b"vnd device");
    assert_eq!(&lp.d_packname[..10], b"fictitious");
    assert_eq!(lp.d_type, DTYPE_VND);
    assert_eq!(
        (lp.d_secsize, lp.d_nsectors, lp.d_ntracks, lp.d_secpercyl),
        (512, 100, 1, 100)
    );
    assert_eq!(lp.d_ncylinders, 0);
    assert_eq!(dkcksum(&lp), 0);
    let incore = sc.sc_dk.label().expect("in-core label");
    assert_eq!(
        dl_getpsize(&incore.d_partitions[RAW_PART as usize]),
        SECTORS as u64
    );
    sc.sc_flags.set(sc.sc_flags.get() | VNF_HAVELABEL);
    sc
}

#[test]
fn unconfigured_units_open_raw_and_refuse_io() {
    let (_g, p) = crate::kern::vfs_subr::tests::setup();
    begin(p);

    assert_eq!(numvnd(), NVND);
    assert_eq!(vnd_softc(3).expect("vnd3").sc_dev.xname(), "vnd3");
    assert!(vnd_softc(NVND as u32).is_none());

    // The raw partition of an unconfigured unit opens (vnconfig needs it); others do not.
    let raw = makediskdev(VND_BMAJ, 1, RAW_PART);
    let sc = vnd_softc(1).expect("vnd1");
    vndopen(raw, FREAD, S_IFCHR as i32, p).expect("raw open");
    assert_eq!(sc.sc_dk.dk_copenmask.get(), 1 << RAW_PART);
    vndclose(raw, FREAD, S_IFCHR as i32, Some(p)).expect("close");
    assert_eq!(sc.sc_dk.dk_openmask.get(), 0);
    assert_eq!(
        vndopen(makediskdev(VND_BMAJ, 1, 0), FREAD, S_IFBLK as i32, p),
        Err(Errno::ENXIO)
    );
    assert_eq!(
        vndopen(makediskdev(VND_BMAJ, 4, RAW_PART), FREAD, S_IFCHR as i32, p),
        Err(Errno::ENXIO)
    );

    // No label, no I/O.
    let bp = buf(raw, 0, DEV_BSIZE, true, 0);
    vndstrategy(bp);
    assert!(bp.isset(B_DONE | B_ERROR));
    assert_eq!(bp.b_error.get(), Some(Errno::ENXIO));
    assert_eq!(bp.b_resid.get(), DEV_BSIZE);

    let mut data = vec![0u8; DISKLABEL_SIZE];
    assert_eq!(
        vndioctl(raw, DIOCGDINFO, &mut data, FREAD, p),
        Err(Errno::ENOTTY)
    );
    assert_eq!(vndioctl(raw, 0, &mut data, FREAD, p), Err(Errno::ENOTTY));
    assert_eq!(
        vndioctl(raw, VNDIOCCLR, &mut data, FREAD, p),
        Err(Errno::ENXIO)
    );

    // VNDIOCGET: unit -1 is the unit asked, a free unit has no inode.
    let mut vnu = VndUser::zeroed();
    vnu.vnu_unit = -1;
    let mut data = vec![0u8; size_of::<VndUser>()];
    ioctl_ret(&mut data, &vnu);
    vndioctl(raw, VNDIOCGET, &mut data, FREAD, p).expect("VNDIOCGET");
    let vnu = ioctl_arg::<VndUser>(&data);
    assert_eq!((vnu.vnu_unit, vnu.vnu_ino, vnu.vnu_dev), (1, 0, 0));
    for (unit, error) in [(NVND, Errno::ENXIO), (-2, Errno::EINVAL)] {
        let mut vnu = VndUser::zeroed();
        vnu.vnu_unit = unit;
        ioctl_ret(&mut data, &vnu);
        assert_eq!(vndioctl(raw, VNDIOCGET, &mut data, FREAD, p), Err(error));
    }

    assert_eq!(vndsize(raw), -1);
    assert_eq!(vnddump(raw, 0, ptr::null_mut(), 0), Err(Errno::ENXIO));
    end();
}

#[test]
fn vndiocset_checks_its_arguments_and_cleans_up() {
    let (_g, p, _mp) = setup_root();
    begin(p);

    let set = |dev: Dev, path: &[u8], secsize: usize| {
        let vio = VndIoctl {
            vnd_file: path.as_ptr() as usize,
            vnd_secsize: secsize,
            vnd_nsectors: 100,
            vnd_ntracks: 1,
            vnd_type: DTYPE_VND,
            ..VndIoctl::default()
        };
        let mut data = vec![0u8; size_of::<VndIoctl>()];
        ioctl_ret(&mut data, &vio);
        vndioctl(dev, VNDIOCSET, &mut data, FREAD, p)
    };
    let raw = makediskdev(VND_BMAJ, 0, RAW_PART);

    // Geometry eventually has to fit into label fields.
    assert_eq!(set(raw, b"/a/b\0", 0), Err(Errno::EINVAL));
    let vio = VndIoctl {
        vnd_secsize: DEV_BSIZE,
        vnd_ntracks: UINT_MAX as usize + 1,
        ..VndIoctl::default()
    };
    let mut data = vec![0u8; size_of::<VndIoctl>()];
    ioctl_ret(&mut data, &vio);
    assert_eq!(
        vndioctl(raw, VNDIOCSET, &mut data, FREAD, p),
        Err(Errno::EINVAL)
    );

    // vn_open weeds out directories.
    assert_eq!(set(raw, b"/a\0", DEV_BSIZE), Err(Errno::EISDIR));

    // testfs's fsid 99 is major 0: through a device of major 0, the file looks like it sits
    // on a vnd. The failure path unlocks and closes the file (a second try would panic on a
    // node left locked).
    let major0 = makediskdev(0, 0, RAW_PART);
    for _ in 0..2 {
        assert_eq!(set(major0, b"/a/b\0", DEV_BSIZE), Err(Errno::EINVAL));
    }
    let sc = vnd_softc(0).expect("vnd0");
    assert_eq!(sc.sc_flags.get(), 0);
    assert!(sc.sc_vp.get().is_none());

    // A configured unit is busy.
    sc.sc_flags.set(VNF_INITED);
    assert_eq!(set(raw, b"/a/b\0", DEV_BSIZE), Err(Errno::EBUSY));
    sc.sc_flags.set(0);
    end();
}

#[test]
fn a_configured_unit_reads_and_writes_its_file() {
    let (_g, p) = crate::kern::vfs_subr::tests::setup();
    begin(p);
    let sc = configure(p, None);
    let raw = makediskdev(VND_BMAJ, 0, RAW_PART);

    // Read sector 2.
    let bp = buf(raw, 2, DEV_BSIZE, true, 0xee);
    vndstrategy(bp);
    assert!(bp.isset(B_DONE));
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), 0);
    assert_eq!(
        bytes(bp),
        file(|f| f[2 * DEV_BSIZE..3 * DEV_BSIZE].to_vec())
    );

    // Write sector 3.
    let bp = buf(raw, 3, DEV_BSIZE, false, 0xab);
    vndstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert!(file(|f| f[3 * DEV_BSIZE..4 * DEV_BSIZE]
        .iter()
        .all(|&b| b == 0xab)));

    // A sloppy request: rounded up for the bounds check, done at its own size.
    let bp = buf(raw, 0, 100, true, 0xee);
    vndstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_bcount.get(), 100);
    assert_eq!(bp.b_resid.get(), 0);
    assert_eq!(bytes(bp), file(|f| f[..100].to_vec()));

    // At the end of the disk: nothing transferred, no error.
    let bp = buf(raw, SECTORS as Daddr, DEV_BSIZE, true, 0xee);
    vndstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), DEV_BSIZE);

    // The label ioctls.
    let mut data = vec![0u8; DISKLABEL_SIZE];
    vndioctl(raw, DIOCGDINFO, &mut data, FREAD, p).expect("DIOCGDINFO");
    let lp = Disklabel::from_bytes(&data);
    assert_eq!(&lp.d_typename[..10], b"vnd device");
    assert_eq!(
        dl_getpsize(&lp.d_partitions[RAW_PART as usize]),
        SECTORS as u64
    );
    let mut data = vec![0u8; size_of::<Partinfo>()];
    vndioctl(raw, DIOCGPART, &mut data, FREAD, p).expect("DIOCGPART");
    let pi = Partinfo::load(&data).expect("partinfo");
    // SAFETY: the in-core label's partition, live while the unit is configured.
    assert_eq!(dl_getpsize(unsafe { &*pi.part }), SECTORS as u64);
    let mut data = vec![0u8; DISKLABEL_SIZE];
    assert_eq!(
        vndioctl(raw, DIOCSDINFO, &mut data, FREAD, p),
        Err(Errno::EBADF)
    );

    // VNDIOCGET names the file.
    let mut vnu = VndUser::zeroed();
    vnu.vnu_unit = 0;
    let mut data = vec![0u8; size_of::<VndUser>()];
    ioctl_ret(&mut data, &vnu);
    vndioctl(raw, VNDIOCGET, &mut data, FREAD, p).expect("VNDIOCGET");
    let vnu = ioctl_arg::<VndUser>(&data);
    assert_eq!(&vnu.vnu_file[..5], b"/img\0");
    assert_eq!(vnu.vnu_ino, MEMFS_INO);
    assert_eq!(vnu.vnu_dev, MEMFS_FSID as Dev);

    // Busy while another partition, or both flavours of this one, are open.
    let blk = makediskdev(VND_BMAJ, 0, RAW_PART);
    vndopen(raw, FREAD, S_IFCHR as i32, p).expect("chr open");
    vndopen(blk, FREAD, S_IFBLK as i32, p).expect("blk open");
    let mut data = vec![0u8; size_of::<VndIoctl>()];
    assert_eq!(
        vndioctl(raw, VNDIOCCLR, &mut data, FREAD, p),
        Err(Errno::EBUSY)
    );
    vndclose(blk, FREAD, S_IFBLK as i32, Some(p)).expect("close");
    vndioctl(raw, VNDIOCCLR, &mut data, FREAD, p).expect("VNDIOCCLR");
    vndclose(raw, FREAD, S_IFCHR as i32, Some(p)).expect("close");
    assert_eq!(sc.sc_flags.get(), 0);
    assert!(sc.sc_vp.get().is_none() && sc.sc_cred.get().is_none());
    assert!(sc.sc_dk.label().is_none());
    assert_eq!(sc.sc_file.get()[0], 0);
    end();
}

#[test]
fn an_encrypted_unit_keeps_its_file_encrypted() {
    let (_g, p) = crate::kern::vfs_subr::tests::setup();
    begin(p);
    let sc = configure(p, Some(b"a vnd key"));
    let raw = makediskdev(VND_BMAJ, 0, RAW_PART);

    // The file holds ciphertext; the buffer is back in clear after the write.
    let bp = buf(raw, 1, 2 * DEV_BSIZE, false, 0x5a);
    vndstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert!(bytes(bp).iter().all(|&b| b == 0x5a));
    let on_disk = file(|f| f[DEV_BSIZE..3 * DEV_BSIZE].to_vec());
    assert_ne!(on_disk, vec![0x5au8; 2 * DEV_BSIZE]);
    // Each sector has its own IV: equal plaintext sectors differ on disk.
    assert_ne!(on_disk[..DEV_BSIZE], on_disk[DEV_BSIZE..]);

    // Reading decrypts.
    let bp = buf(raw, 2, DEV_BSIZE, true, 0);
    vndstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert!(bytes(bp).iter().all(|&b| b == 0x5a));

    // vndencrypt is its own inverse with the same sector number.
    let mut sector = on_disk[..DEV_BSIZE].to_vec();
    vndencrypt(sc, &mut sector, 1, false);
    assert!(sector.iter().all(|&b| b == 0x5a));
    vndencrypt(sc, &mut sector, 1, true);
    assert_eq!(sector, on_disk[..DEV_BSIZE]);

    // The test's context is leaked memory, not malloc's: forget it before unconfiguring.
    sc.sc_keyctx.set(None);
    let mut data = vec![0u8; size_of::<VndIoctl>()];
    vndioctl(raw, VNDIOCCLR, &mut data, FREAD, p).expect("VNDIOCCLR");
    end();
}
