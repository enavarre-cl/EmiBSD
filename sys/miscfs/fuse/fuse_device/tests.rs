//! Host tests for the fuse(4) device: opens and closes, a request queued by the kernel read
//! back by the daemon in libfuse's layout (header, input structure, data), replies (data,
//! errors, bad lengths, unknown IDs, notifications), `FUSE_FORGET` and `FUSE_INIT`,
//! `fb_queue` returning the daemon's answer, the cleanup of both queues, close detaching the
//! mount, and the read filter. The vfs setup is `vfs_subr/tests.rs`'s; its `vfsinit` runs
//! `fusefs_init` (the fusebuf pool, the inode hash).

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::machine::Machine;
use crate::machine::copy::AbiPod;
use crate::machine::cpu::Cpu;
use crate::miscfs::fuse::fuse_vfsops::PENDING;
use crate::miscfs::fuse::fusebuf::{TEST_DAEMON, fb_queue, fb_setup};
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::fusebuf::{
    FUSE_GETATTR, FUSE_LOOKUP, FUSE_OPEN, FUSE_READ, FuseAttrOut, FuseEntryOut, FuseForgetIn,
    FuseInitIn, FuseOpenOut, FuseReadIn, abi_bytes,
};
use crate::sys::mount::Mount;
use crate::sys::proc::THREAD_PID_OFFSET;
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// fuse(4)'s character major (`cdevsw[92]`).
pub(crate) const FUSE_MAJOR: u32 = 92;

/// Memory, the vfs (whose `vfsinit` runs `fusefs_init`), the thread as `curproc`, an empty
/// device list and counters, no simulated daemon.
pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);
    rw_obj_init();
    fuseattach(NFUSE);
    STAT_FBUFS_IN.store(0, Ordering::Relaxed);
    STAT_FBUFS_WAIT.store(0, Ordering::Relaxed);
    STAT_OPENED_FUSEDEV.store(0, Ordering::Relaxed);
    TEST_DAEMON.with(|d| d.set(None));
    (g, p)
}

/// `/dev/fuse<unit>`.
pub(crate) fn fusedev(unit: u32) -> Dev {
    makedev(FUSE_MAJOR, unit)
}

/// A mount structure for the device as `fusefs_mount` makes it, on a mount that is not a
/// real one (the device only reads `max_read`, `max_write` and `sess_init`).
pub(crate) fn fake_fmp(dev: Dev, max_read: i32) -> &'static FusefsMnt {
    let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
    Box::leak(Box::new(FusefsMnt {
        mp,
        undef_op: Cell::new(0),
        max_read,
        max_write: Cell::new(4096),
        sess_init: Cell::new(PENDING),
        allow_other: 0,
        dev,
    }))
}

/// `read(fd, buf, len)` on the device: the bytes read.
pub(crate) fn dev_read(dev: Dev, buf: &mut [u8], ioflag: i32) -> Result<usize, Errno> {
    let len = buf.len();
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: len,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    fuseread(dev, &mut uio, ioflag)?;
    Ok(len - uio.uio_resid)
}

/// `writev(fd, ...)` of `parts` on the device, one iovec each, as libfuse writes a reply.
pub(crate) fn dev_write(dev: Dev, parts: &[&[u8]]) -> Result<(), Errno> {
    let mut bufs: Vec<Vec<u8>> = parts.iter().map(|p| p.to_vec()).collect();
    let mut iov: Vec<Iovec> = bufs
        .iter_mut()
        .map(|b| Iovec {
            iov_base: b.as_mut_ptr().cast(),
            iov_len: b.len(),
        })
        .collect();
    let resid = parts.iter().map(|p| p.len()).sum();
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    fusewrite(dev, &mut uio, 0)
}

/// The size of an operation's input structure, as libfuse knows it (`<sys/fusebuf.h>`'s
/// userland table): independent of `fb_setup`'s.
pub(crate) fn op_in_len(opcode: u32) -> usize {
    use crate::sys::fusebuf::*;
    match opcode {
        FUSE_GETATTR => size_of::<FuseGetattrIn>(),
        FUSE_SETATTR => size_of::<FuseSetattrIn>(),
        FUSE_MKNOD => size_of::<FuseMknodIn>(),
        FUSE_MKDIR => size_of::<FuseMkdirIn>(),
        FUSE_RENAME => size_of::<FuseRenameIn>(),
        FUSE_LINK => size_of::<FuseLinkIn>(),
        FUSE_OPEN | FUSE_OPENDIR => size_of::<FuseOpenIn>(),
        FUSE_READ | FUSE_READDIR => size_of::<FuseReadIn>(),
        FUSE_WRITE => size_of::<FuseWriteIn>(),
        FUSE_RELEASE | FUSE_RELEASEDIR => size_of::<FuseReleaseIn>(),
        FUSE_FSYNC => size_of::<FuseFsyncIn>(),
        FUSE_FLUSH => size_of::<FuseFlushIn>(),
        FUSE_INIT => size_of::<FuseInitIn>(),
        FUSE_FORGET => size_of::<FuseForgetIn>(),
        _ => 0, // LOOKUP, READLINK, SYMLINK, UNLINK, RMDIR, STATFS, DESTROY
    }
}

/// A request as the daemon reads it: the header, the input structure's bytes, the data.
pub(crate) struct Req {
    pub(crate) hdr: FuseInHeader,
    pub(crate) op: Vec<u8>,
    pub(crate) data: Vec<u8>,
}

impl Req {
    /// The input structure `T`.
    pub(crate) fn op<T: AbiPod + Default>(&self) -> T {
        let mut v = T::default();
        abi_bytes_mut(&mut v).copy_from_slice(&self.op[..size_of::<T>()]);
        v
    }
}

/// Reads the next request from the device (`None` when there is none, `IO_NDELAY`).
pub(crate) fn read_req(dev: Dev) -> Option<Req> {
    let mut buf = std::vec![0u8; 64 * 1024];
    let n = match dev_read(dev, &mut buf, IO_NDELAY) {
        Ok(n) => n,
        Err(Errno::EAGAIN) => return None,
        Err(e) => std::panic!("fuseread: {e:?}"),
    };
    let mut hdr = FuseInHeader::default();
    abi_bytes_mut(&mut hdr).copy_from_slice(&buf[..size_of::<FuseInHeader>()]);
    assert_eq!(hdr.len as usize, n, "hdr.len is the request's length");
    let op_end = size_of::<FuseInHeader>() + op_in_len(hdr.opcode);
    Some(Req {
        hdr,
        op: buf[size_of::<FuseInHeader>()..op_end].to_vec(),
        data: buf[op_end..n].to_vec(),
    })
}

/// Writes a reply to request `unique`: `error` (negated errno), output structure, data.
pub(crate) fn reply(
    dev: Dev,
    unique: u64,
    error: i32,
    out: &[u8],
    data: &[u8],
) -> Result<(), Errno> {
    let hdr = FuseOutHeader {
        len: (size_of::<FuseOutHeader>() + out.len() + data.len()) as u32,
        error,
        unique,
    };
    dev_write(dev, &[abi_bytes(&hdr), out, data])
}

/// `fuseopen` of `/dev/fuse<unit>` for reading and writing.
pub(crate) fn open_dev(p: &Proc, unit: u32) -> Dev {
    let dev = fusedev(unit);
    fuseopen(dev, FREAD | FWRITE, 0, p).expect("fuseopen");
    dev
}

#[test]
fn open_is_exclusive_per_minor_and_close_frees() {
    let (_g, p) = setup();
    let dev0 = open_dev(p, 0);
    assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 1);
    assert_eq!(fuseopen(dev0, FREAD, 0, p), Err(Errno::EBUSY));
    assert_eq!(
        fuseopen(fusedev(1), FREAD | O_EXCL, 0, p),
        Err(Errno::EBUSY),
        "no exclusive opens"
    );
    let dev1 = open_dev(p, 1 << 8); // a clone's minor
    assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 2);
    assert!(fuse_lookup_dev(dev1).is_some_and(|fd| {
        refcnt_rele(&fd.fd_refcnt);
        fd.fd_unit.get() == 1 << 8
    }));

    fuseclose(dev0, FREAD, 0, Some(p)).expect("close");
    fuseclose(dev1, FREAD, 0, Some(p)).expect("close");
    assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 0);
    assert_eq!(fuseclose(dev0, FREAD, 0, Some(p)), Err(Errno::EBADF));
    assert_eq!(
        fuseread(dev0, &mut empty_uio(&mut [0u8; 1]), 0),
        Err(Errno::ENODEV)
    );
}

/// A one-byte read uio over `b`, for the error paths that never touch it.
fn empty_uio(b: &mut [u8; 1]) -> Uio<'static> {
    let iov: &'static mut [Iovec] = Box::leak(Box::new([Iovec {
        iov_base: b.as_mut_ptr().cast(),
        iov_len: 1,
    }]));
    Uio {
        uio_iov: iov,
        uio_offset: 0,
        uio_resid: 1,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    }
}

#[test]
fn read_needs_a_mount_and_honours_ndelay() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    let mut buf = [0u8; 256];
    assert_eq!(dev_read(dev, &mut buf, IO_NDELAY), Err(Errno::ENODEV));
    let fmp = fake_fmp(dev, FUSEBUFMAXSIZE as i32);
    fuse_device_set_fmp(fmp, true);
    assert_eq!(dev_read(dev, &mut buf, IO_NDELAY), Err(Errno::EAGAIN));
    fuseclose(dev, 0, 0, Some(p)).expect("close");
    assert_eq!(
        fmp.sess_init.get(),
        0,
        "close tells the mount the daemon is gone"
    );
}

#[test]
fn a_queued_request_is_read_in_libfuse_layout_and_answered() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

    let fbuf = fb_setup(6, 7, FUSE_LOOKUP, p);
    // SAFETY: a fresh fusebuf, not queued yet.
    unsafe { fbuf.fb_dat_slice() }.copy_from_slice(b"hello\0");
    fuse_device_queue_fbuf(dev, fbuf);
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 1);

    // We get the whole fusebuf or nothing.
    let mut small = [0u8; 45];
    assert_eq!(dev_read(dev, &mut small, IO_NDELAY), Err(Errno::EINVAL));
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 1);

    let req = read_req(dev).expect("a request");
    assert_eq!(req.hdr.len, 46);
    assert_eq!(req.hdr.opcode, FUSE_LOOKUP);
    assert_eq!(req.hdr.unique, fbuf.fb_uuid());
    assert_eq!(req.hdr.nodeid, 7);
    assert_eq!(req.hdr.uid, p.ucred().cr_uid.get());
    assert_eq!(req.hdr.gid, p.ucred().cr_gid.get());
    assert_eq!(req.hdr.pid, (p.p_tid.get() + THREAD_PID_OFFSET) as u32);
    assert!(req.op.is_empty());
    assert_eq!(req.data, b"hello\0");
    assert!(fbuf.fb_dat().is_null(), "the data is freed once read");
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 1);
    assert!(read_req(dev).is_none());

    let mut entry = FuseEntryOut {
        nodeid: 9,
        ..FuseEntryOut::default()
    };
    entry.attr.mode = 0o100644;
    entry.attr.size = 13;
    reply(dev, req.hdr.unique, 0, abi_bytes(&entry), &[]).expect("reply");
    assert_eq!(fbuf.fb_err(), 0);
    assert_eq!(fbuf.op_get::<FuseEntryOut>(), entry);
    assert_eq!(fbuf.fb_len(), 0);
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
    // The reply's ID is not waited for any more.
    assert_eq!(
        reply(dev, req.hdr.unique, 0, abi_bytes(&entry), &[]),
        Err(Errno::ENOENT)
    );
    fb_delete(fbuf);
    fuseclose(dev, 0, 0, Some(p)).expect("close");
}

#[test]
fn replies_with_errors_bad_lengths_and_notifications() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    let fmp = fake_fmp(dev, 8);
    fuse_device_set_fmp(fmp, true);

    // Too short for a header; a notification (unique 0) is accepted and ignored.
    assert_eq!(dev_write(dev, &[&[0u8; 15]]), Err(Errno::EINVAL));
    reply(dev, 0, -1, &[], &[]).expect("notification");

    // An error reply carries nothing else.
    let a = fb_setup(0, 1, FUSE_GETATTR, p);
    let b = fb_setup(0, 1, FUSE_GETATTR, p);
    fuse_device_queue_fbuf(dev, a);
    fuse_device_queue_fbuf(dev, b);
    let ra = read_req(dev).expect("a");
    let rb = read_req(dev).expect("b");
    assert_eq!(ra.op.len(), size_of::<crate::sys::fusebuf::FuseGetattrIn>());
    // b is answered first: a is then the element before it on the wait queue.
    reply(dev, rb.hdr.unique, -(Errno::ENOENT as i32), &[], &[]).expect("error reply");
    assert_eq!(b.fb_err(), Errno::ENOENT as i32);
    assert_eq!(
        reply(dev, ra.hdr.unique, -(Errno::EACCES as i32), &[], &[1, 2]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        a.fb_err(),
        Errno::EIO as i32,
        "an error reply with data fails"
    );
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
    fb_delete(a);
    fb_delete(b);

    // A data reply: its length comes from hdr.len, bounded by max_read (8 here).
    let r = fb_setup(0, 2, FUSE_READ, p);
    r.op_set(&FuseReadIn {
        size: 8,
        ..FuseReadIn::default()
    });
    fuse_device_queue_fbuf(dev, r);
    let rr = read_req(dev).expect("read");
    assert_eq!(rr.op::<FuseReadIn>().size, 8);
    reply(dev, rr.hdr.unique, 0, &[], b"abcdef").expect("data");
    assert_eq!(r.fb_len(), 6);
    // SAFETY: the reply is in; the fusebuf is off the queues.
    assert_eq!(unsafe { r.fb_dat_slice() }, b"abcdef");
    fb_delete(r);

    let r = fb_setup(0, 2, FUSE_READ, p);
    fuse_device_queue_fbuf(dev, r);
    let rr = read_req(dev).expect("read");
    assert_eq!(
        reply(dev, rr.hdr.unique, 0, &[], b"123456789"),
        Err(Errno::EINVAL)
    );
    assert_eq!(r.fb_err(), Errno::EIO as i32, "more than max_read");
    fb_delete(r);

    // hdr.len and the bytes written disagree.
    let o = fb_setup(0, 2, FUSE_OPEN, p);
    fuse_device_queue_fbuf(dev, o);
    let ro = read_req(dev).expect("open");
    let hdr = FuseOutHeader {
        len: 16,
        error: 0,
        unique: ro.hdr.unique,
    };
    let out = FuseOpenOut::default();
    assert_eq!(
        dev_write(dev, &[abi_bytes(&hdr), abi_bytes(&out), &[0]]),
        Err(Errno::EINVAL)
    );
    assert_eq!(o.fb_err(), Errno::EIO as i32);
    fb_delete(o);

    fuseclose(dev, 0, 0, Some(p)).expect("close");
}

#[test]
fn forget_and_init_need_no_requester() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    let fmp = fake_fmp(dev, FUSEBUFMAXSIZE as i32);
    fuse_device_set_fmp(fmp, true);

    let f = fb_setup(0, 5, FUSE_FORGET, p);
    f.op_set(&FuseForgetIn { nlookup: 3 });
    fuse_device_queue_fbuf(dev, f);
    let rf = read_req(dev).expect("forget");
    assert_eq!(rf.hdr.opcode, FUSE_FORGET);
    assert_eq!(rf.op::<FuseForgetIn>().nlookup, 3);
    assert_eq!(
        STAT_FBUFS_WAIT.load(Ordering::Relaxed),
        0,
        "no reply is expected"
    );

    // A daemon of an older major version is refused: the session stays pending.
    let i = fb_setup(0, 0, FUSE_INIT, p);
    i.op_set(&FuseInitIn {
        major: FUSE_KERNEL_VERSION,
        minor: 19,
        ..FuseInitIn::default()
    });
    fuse_device_queue_fbuf(dev, i);
    let ri = read_req(dev).expect("init");
    assert_eq!(ri.op::<FuseInitIn>().major, 7);
    let old = FuseInitOut {
        major: 6,
        minor: 30,
        ..FuseInitOut::default()
    };
    assert_eq!(
        reply(dev, ri.hdr.unique, 0, abi_bytes(&old), &[]),
        Err(Errno::EINVAL)
    );
    assert_eq!(fmp.sess_init.get(), PENDING);

    let i = fb_setup(0, 0, FUSE_INIT, p);
    fuse_device_queue_fbuf(dev, i);
    let ri = read_req(dev).expect("init");
    let ok = FuseInitOut {
        major: 7,
        minor: 26,
        max_write: 0,
        ..FuseInitOut::default()
    };
    reply(dev, ri.hdr.unique, 0, abi_bytes(&ok), &[]).expect("init reply");
    assert_eq!(fmp.sess_init.get(), 1);
    assert_eq!(
        fmp.max_write.get(),
        FUSEBUFMAXSIZE as i32,
        "0 means the most"
    );
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);

    fuseclose(dev, 0, 0, Some(p)).expect("close");
}

/// A simulated daemon answering every queued request with `FuseOpenOut { fh: 77 }`, or
/// with `EACCES` for `FUSE_GETATTR`.
fn open_daemon(dev: Dev) {
    while let Some(req) = read_req(dev) {
        if req.hdr.opcode == FUSE_GETATTR {
            reply(dev, req.hdr.unique, -(Errno::EACCES as i32), &[], &[]).expect("reply");
        } else {
            let out = FuseOpenOut {
                fh: 77,
                ..FuseOpenOut::default()
            };
            reply(dev, req.hdr.unique, 0, abi_bytes(&out), &[]).expect("reply");
        }
    }
}

#[test]
fn fb_queue_returns_the_daemons_answer() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);
    TEST_DAEMON.with(|d| d.set(Some(open_daemon)));

    let o = fb_setup(0, 3, FUSE_OPEN, p);
    assert_eq!(fb_queue(dev, o), Ok(()));
    assert_eq!(o.op_get::<FuseOpenOut>().fh, 77);
    fb_delete(o);

    let g = fb_setup(0, 3, FUSE_GETATTR, p);
    assert_eq!(fb_queue(dev, g), Err(Errno::EACCES));
    assert_eq!(g.op_get::<FuseAttrOut>(), FuseAttrOut::default());
    fb_delete(g);
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);

    TEST_DAEMON.with(|d| d.set(None));
    fuseclose(dev, 0, 0, Some(p)).expect("close");
}

#[test]
fn cleanup_fails_every_queued_request() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

    let fbufs: Vec<&'static Fusebuf> = (0..5).map(|i| fb_setup(0, i, FUSE_GETATTR, p)).collect();
    for f in &fbufs {
        fuse_device_queue_fbuf(dev, f);
    }
    // Two of them read: on the wait queue.
    read_req(dev).expect("one");
    read_req(dev).expect("two");
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 3);
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 2);

    fuse_device_cleanup(dev);
    for f in &fbufs {
        assert_eq!(f.fb_err(), Errno::ENXIO as i32);
    }
    assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
    assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
    let fd = fuse_lookup_dev(dev).expect("open");
    assert!(fd.fd_fbufs_in.is_empty() && fd.fd_fbufs_wait.is_empty());
    refcnt_rele(&fd.fd_refcnt);
    assert!(read_req(dev).is_none());
    for f in fbufs {
        fb_delete(f);
    }
    fuseclose(dev, 0, 0, Some(p)).expect("close");
}

#[test]
fn the_read_filter_fires_while_a_request_waits() {
    let (_g, p) = setup();
    let dev = open_dev(p, 0);
    fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

    let kn: &'static Knote = Box::leak(Box::new(Knote::new()));
    kn.kn_filter().set(EVFILT_READ);
    fusekqfilter(dev, kn).expect("kqfilter");
    assert!(
        kn.kn_fop
            .get()
            .is_some_and(|f| ptr::eq(f, &FUSE_RD_FILTOPS))
    );
    let fd = kn_fused(kn);

    let event = || {
        rw_enter_write(&fd.fd_lock);
        let e = filt_fuse_read(kn, 0);
        rw_exit_write(&fd.fd_lock);
        e
    };
    assert!(!event());
    filt_fuse_rdetach(kn);
    assert!(fd.fd_rklist.kl_list.is_empty());

    // Queueing activates the knotes on the list (`knote_locked`), which needs a kqueue: the
    // knote is attached again once the request waits.
    let f = fb_setup(0, 1, FUSE_GETATTR, p);
    fuse_device_queue_fbuf(dev, f);
    fusekqfilter(dev, kn).expect("kqfilter");
    assert!(event());
    read_req(dev).expect("the request");
    assert!(!event());
    filt_fuse_rdetach(kn);
    reply(dev, f.fb_uuid(), 0, abi_bytes(&FuseAttrOut::default()), &[]).expect("reply");
    fb_delete(f);

    let wk: &'static Knote = Box::leak(Box::new(Knote::new()));
    wk.kn_filter().set(EVFILT_WRITE);
    fusekqfilter(dev, wk).expect("write filter: always true");
    let bad: &'static Knote = Box::leak(Box::new(Knote::new()));
    bad.kn_filter().set(crate::sys::event::EVFILT_VNODE);
    assert_eq!(fusekqfilter(dev, bad), Err(Errno::EINVAL));

    fuseclose(dev, 0, 0, Some(p)).expect("close");
}
