//! Host tests for the descriptor tables: the bitmap search (`find_next_zero`, `fd_used`,
//! `fd_unused`, `find_last_set`), descriptor allocation and expansion (`fdalloc`'s search,
//! `fdexpand`, past the 1024 descriptors of the internal maps), the open file life cycle
//! (`fnew`, `fdinsert`, `fd_getfile`, `fdrelease`, `closef`, `fdrop`), `finishdup`, `fdcopy`,
//! `fdprepforexec` and `fdfree`.
//!
//! The limit `fdalloc` reads needs a `curproc`, which the host's one CPU cannot give a test
//! without disturbing the others, so the tests drive `fdalloc_search` with the limit as an
//! argument.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicUsize;
use std::{assert, assert_eq, vec::Vec};

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::crget;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::event::Knote;
use crate::sys::file::Fileops;
use crate::sys::filedesc::NDENTRIES;
use crate::sys::uio::Uio;

/// Real memory, the process pools (for the credentials) and the file pools.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    filedesc_init();
    guard
}

/// A thread of a fresh process with credentials and a new descriptor table.
fn thread() -> &'static Proc {
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    let fdp = fdinit();
    pr.ps_fd.set(fdp);
    p.p_fd.set(fdp);
    p
}

/// How many times `test_close` ran.
static CLOSES: AtomicUsize = AtomicUsize::new(0);

fn test_rw(_fp: &File, _uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    Ok(())
}

fn test_ioctl(_fp: &File, _com: u64, _data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

fn test_kqfilter(_fp: &File, _kn: &Knote) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

fn test_stat(_fp: &File, _ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

fn test_close(_fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
    CLOSES.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

static TESTOPS: Fileops = Fileops {
    fo_read: test_rw,
    fo_write: test_rw,
    fo_ioctl: test_ioctl,
    fo_kqfilter: test_kqfilter,
    fo_stat: test_stat,
    fo_close: test_close,
    fo_seek: None,
};

/// `falloc` + `fdinsert` with the limit given: a new file at the lowest free descriptor,
/// the caller's extra reference dropped.
fn open_file(p: &Proc, lim: i32) -> (i32, &'static File) {
    let fdp = p.fd();
    fdplock(fdp);
    let fd = loop {
        match fdalloc_search(fdp, 0, lim, false) {
            Ok(fd) => break fd,
            Err(Errno::ENOSPC) => fdexpand(p).unwrap(),
            Err(e) => panic!("fdalloc: {e:?}"),
        }
    };
    let fp = fnew(p).unwrap();
    fref(fp);
    fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
    fp.f_type.set(DTYPE_PIPE);
    fp.f_ops.set(Some(&TESTOPS));
    fdinsert(fdp, fd, 0, fp);
    fdpunlock(fdp);
    frele(fp, p).unwrap();
    (fd, fp)
}

#[test]
fn find_next_zero_scans_words_from_want() {
    let map = [!0u32, 0xffff_00ff, 0];
    let at = |o: usize| map.get(o).copied().unwrap_or(!0);
    assert_eq!(find_next_zero(&at, 0, 96), 40);
    assert_eq!(find_next_zero(&at, 41, 96), 41);
    assert_eq!(find_next_zero(&at, 48, 96), 64);
    assert_eq!(find_next_zero(&at, 97, 96), -1);
    let full = |_| !0u32;
    assert_eq!(find_next_zero(&full, 0, 32), -1);
    // bits rounds up to whole high-map words: NDLOSLOTS(1) is 32 words.
    let one = |o: usize| if o == 31 { 0xfffe_ffff } else { !0 };
    assert_eq!(find_next_zero(&one, 0, 1), 31 * 32 + 16);
}

#[test]
fn fdinit_builds_an_empty_table() {
    let _g = setup();
    let p = thread();
    let fdp = p.fd();
    assert_eq!(fdp.fd_nfiles.load(Ordering::Relaxed), NDFILE as i32);
    assert_eq!(fdp.fd_refcnt.get(), 1);
    assert_eq!(fdp.fd_cmask.get(), S_IWGRP | S_IWOTH);
    assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 0);
    assert!((0..NDFILE).all(|fd| fdp.ofile(fd).is_none()));
    fdfree(p);
    assert!(p.p_fd.get().is_null());
}

#[test]
fn fdalloc_takes_the_lowest_free_descriptor() {
    let _g = setup();
    let p = thread();
    let fdp = p.fd();
    fdplock(fdp);
    for want in 0..3 {
        assert_eq!(fdalloc_search(fdp, 0, 1000, false), Ok(want));
    }
    assert_eq!(fdalloc_search(fdp, 7, 1000, true), Ok(7));
    assert_eq!(fdp.ofileflags(7), UF_PLEDGED);
    assert_eq!(fdp.fd_lastfile.get(), 7);
    assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 4);

    fd_unused(fdp, 1);
    assert_eq!(fdp.fd_freefile.get(), 1);
    assert_eq!(fdalloc_search(fdp, 0, 1000, false), Ok(1));
    fd_unused(fdp, 7);
    assert_eq!(fdp.fd_lastfile.get(), 2);
    assert_eq!(find_last_set(fdp, 3), 2);

    // The table is full at NDFILE: ENOSPC asks for an expansion, EMFILE is the limit.
    while fdalloc_search(fdp, 0, 1000, false).is_ok() {}
    assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), NDFILE as i32);
    assert_eq!(fdalloc_search(fdp, 0, 1000, false), Err(Errno::ENOSPC));
    assert_eq!(
        fdalloc_search(fdp, 0, NDFILE as i32, false),
        Err(Errno::EMFILE)
    );
    fdpunlock(fdp);
}

#[test]
fn fdexpand_grows_the_table_and_the_maps() {
    let _g = setup();
    let p = thread();
    let fdp = p.fd();
    fdplock(fdp);
    let mut sizes = Vec::new();
    let mut fd = 0;
    while fd < 1100 {
        match fdalloc_search(fdp, 0, 4096, false) {
            Ok(got) => {
                assert_eq!(got, fd);
                fd += 1;
            }
            Err(Errno::ENOSPC) => {
                fdexpand(p).unwrap();
                sizes.push(fdp.fd_nfiles.load(Ordering::Relaxed));
            }
            Err(e) => panic!("fdalloc: {e:?}"),
        }
    }
    assert_eq!(sizes, [50, 100, 200, 400, 800, 1600]);
    // Past 1024 descriptors the maps left the internal arrays: 2 high words, 64 low.
    assert_eq!(ndhislots(fdp.nfiles()), 2);
    assert_eq!(fdp.himap(0), !0);
    assert_eq!(fdp.lomap(31), !0);
    assert_eq!(fdp.lomap(34), (1 << (1100 - 34 * NDENTRIES)) - 1);
    assert_eq!(fdp.fd_lastfile.get(), 1099);

    // A hole is found again through the high map.
    fd_unused(fdp, 40);
    assert_eq!(fdp.himap(0) & 2, 0);
    assert_eq!(fdalloc_search(fdp, 0, 4096, false), Ok(40));
    assert_eq!(fdalloc_search(fdp, 2000, 4096, false), Err(Errno::ENOSPC));
    fdpunlock(fdp);
    fdfree(p);
}

#[test]
fn files_are_counted_shared_and_closed() {
    let _g = setup();
    let p = thread();
    let fdp = p.fd();
    let closes = CLOSES.load(Ordering::SeqCst);
    let numfiles = NUMFILES.load(Ordering::SeqCst);

    let (fd, fp) = open_file(p, 1000);
    assert_eq!(fd, 0);
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 1);
    assert!(fp.f_iflags.load(Ordering::SeqCst) & FIF_INSERTED != 0);
    assert!(ptr::eq(FILEHEAD.0.first().unwrap(), fp));
    assert_eq!(NUMFILES.load(Ordering::SeqCst), numfiles + 1);

    // fd_getfile takes a reference; fd_getfile_mode refuses a mode the file lacks.
    let got = fd_getfile(fdp, fd).unwrap();
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
    frele(got, p).unwrap();
    fp.f_flag.store(FREAD as u32, Ordering::SeqCst);
    assert!(fd_getfile_mode(fdp, fd, FWRITE).is_none());
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 1);
    assert!(fd_getfile(fdp, 5).is_none());
    assert!(fd_getfile(fdp, -1).is_none());

    // dup: finishdup puts the same file at a second descriptor.
    let mut retval = [0; 2];
    fdplock(fdp);
    let new = fdalloc_search(fdp, 3, 1000, false).unwrap();
    fdp.set_ofileflags(fd as usize, UF_EXCLOSE);
    let dup = fd_getfile(fdp, fd).unwrap();
    assert_eq!(
        finishdup(p, dup, fd, new, &mut retval, DUPF_CLOFORK),
        Ok(())
    );
    assert_eq!(retval[0], 3);
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
    assert_eq!(fdp.ofileflags(3), UF_FORKCLOSE);
    assert!(!fd_checkclosed(fdp, 3, fp));

    // fdcopy skips the close-on-fork descriptor and shares the file.
    let child = fdcopy(p.process());
    assert!(ptr::eq(child.ofile(0).unwrap(), fp));
    assert!(child.ofile(3).is_none());
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 3);

    // exec closes the close-on-exec descriptor.
    fdprepforexec(p);
    assert!(fdp.ofile(0).is_none());
    assert_eq!(fdp.ofileflags(3), 0);
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
    assert_eq!(CLOSES.load(Ordering::SeqCst), closes);

    // close(3), then the child's table goes: the last reference closes the file.
    fdplock(fdp);
    assert_eq!(fdrelease(p, 3), Ok(()));
    fdplock(fdp);
    assert_eq!(fdrelease(p, 3), Err(Errno::EBADF));
    assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 0);
    let q = thread();
    let qfd = q.fd();
    q.p_fd.set(child);
    fdfree(q);
    assert_eq!(CLOSES.load(Ordering::SeqCst), closes + 1);
    assert_eq!(NUMFILES.load(Ordering::SeqCst), numfiles);
    assert!(FILEHEAD.0.first().is_none_or(|f| !ptr::eq(f, fp)));
    q.p_fd.set(qfd);
    fdfree(q);
    fdfree(p);
}
