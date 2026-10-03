//! Host tests for pipes: the circular buffer through `pipe_read`/`pipe_write` (partial
//! reads, wraparound, atomic writes of at most `PIPE_BUF` bytes, non-blocking `EAGAIN`),
//! EOF and `EPIPE` after one side is closed, the pair going back to the pool, `pipe_ioctl`,
//! `pipe_stat`, `pipe_rundown` and the filter bodies.
//!
//! The host double refuses pageable kernel memory (`km_alloc` with `kp_pageable` needs an
//! MMU), so `pipe_pair_create` fails there, which the first test checks; the others build
//! the pair as `pipe_pair_create` does, with small buffers from the test's heap, and clear
//! a pipe's buffer before closing it so that `pipe_buffer_free` does not hand it to
//! `km_free`.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::crget;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::filio::FIONBIO;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// The size of the test pipes' buffers.
const SIZE: usize = 16;

/// Real memory, the process pools (for the credentials) and the pipe pool.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    pipe_init();
    guard
}

/// A pipe pair linked as `pipe_pair_create` links it, with `SIZE`-byte heap buffers, and its
/// two files: `fds[0]` reads `pp_rpipe`, `fds[1]` reads `pp_wpipe`.
fn test_pair() -> (&'static PipePair, &'static File, &'static File) {
    let Some(mem) = pool_get(&PIPE_PAIR_POOL, PR_WAITOK | PR_ZERO) else {
        panic!("pipe_pair_pool is empty");
    };
    let raw = mem.cast::<PipePair>().as_ptr();
    // SAFETY: a fresh pool item, as in `pipe_pair_create`.
    unsafe { raw.write(PipePair::new()) };
    // SAFETY: as above.
    let pp: &'static PipePair = unsafe { &*raw };
    for (pipe, peer) in [(&pp.pp_wpipe, &pp.pp_rpipe), (&pp.pp_rpipe, &pp.pp_wpipe)] {
        pipe.pipe_pair.set(raw);
        pipe.pipe_peer.set(peer);
        pipe.pipe_lock.set(&pp.pp_lock);
        let buffer = vec![0u8; SIZE].leak();
        pipe.pipe_buffer.buffer.set(buffer.as_mut_ptr());
        pipe.pipe_buffer.size.set(SIZE as u32);
        sigio_init(&pipe.pipe_sigio);
    }
    rw_init(&pp.pp_lock, "pipelk");

    let file = |pipe: &Pipe| -> &'static File {
        let fp: &'static File = Box::leak(Box::new(File::new()));
        fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
        fp.f_type.set(DTYPE_PIPE);
        fp.f_data.set(ptr::from_ref(pipe).cast_mut().cast());
        fp.f_ops.set(Some(&PIPEOPS));
        fp.f_cred.set(crget());
        fp
    };
    (pp, file(&pp.pp_rpipe), file(&pp.pp_wpipe))
}

/// Closes a test file: its pipe's heap buffer is dropped first (see the module's notes).
fn close(fp: &File) {
    fp_pipe(fp).pipe_buffer.buffer.set(ptr::null_mut());
    assert_eq!(pipe_close(fp, None), Ok(()));
    assert!(fp.f_ops.get().is_none());
}

/// Sets or clears `FNONBLOCK`, as `FIONBIO` does in `sys_ioctl`.
fn nonblock(fp: &File, on: bool) {
    if on {
        fp.f_flag.fetch_or(FNONBLOCK as u32, Ordering::SeqCst);
    } else {
        fp.f_flag.fetch_and(!(FNONBLOCK as u32), Ordering::SeqCst);
    }
}

/// `write(2)` of `bytes` through `pipe_write`: the result and how much went in.
fn write(fp: &File, bytes: &[u8]) -> (Result<(), Errno>, usize) {
    let mut iov = [Iovec::new()];
    iov[0].iov_base = bytes.as_ptr().cast_mut().cast();
    iov[0].iov_len = bytes.len();
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: bytes.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    let error = pipe_write(fp, &mut uio, 0);
    (error, bytes.len() - uio.uio_resid)
}

/// `read(2)` of at most `n` bytes through `pipe_read`.
fn read(fp: &File, n: usize) -> (Result<(), Errno>, Vec<u8>) {
    let mut buf = vec![0u8; n];
    let mut iov = [Iovec::new()];
    iov[0].iov_base = buf.as_mut_ptr().cast();
    iov[0].iov_len = n;
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: n,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let error = pipe_read(fp, &mut uio, 0);
    let got = n - uio.uio_resid;
    buf.truncate(got);
    (error, buf)
}

/// A thread for the `fo_ioctl`/`fo_stat` signatures; neither reads it.
fn thread() -> &'static Proc {
    Box::leak(Box::new(Proc::new()))
}

#[test]
fn pair_create_fails_without_pageable_memory() {
    let _g = setup();
    let out = PIPE_PAIR_POOL.pr_nout.get();
    // The host's km_alloc refuses kp_pageable: the first buffer fails, both pipes are
    // destroyed and the pair goes back to the pool.
    assert!(pipe_pair_create().is_none());
    assert_eq!(PIPE_PAIR_POOL.pr_nout.get(), out);
}

#[test]
fn write_then_read_in_pieces() {
    let _g = setup();
    let (pp, rf, wf) = test_pair();

    assert_eq!(write(wf, b"hello"), (Ok(()), 5));
    assert_eq!(pp.pp_rpipe.pipe_buffer.cnt.get(), 5);
    assert_eq!(read(rf, 3), (Ok(()), b"hel".to_vec()));
    // Some data was read: the read returns what there was instead of waiting for more.
    assert_eq!(read(rf, 10), (Ok(()), b"lo".to_vec()));
    // An empty buffer resets its pointers.
    let pb = &pp.pp_rpipe.pipe_buffer;
    assert_eq!((pb.cnt.get(), pb.r#in.get(), pb.out.get()), (0, 0, 0));

    nonblock(rf, true);
    assert_eq!(read(rf, 1), (Err(Errno::EAGAIN), Vec::new()));
    assert!(!pp.pp_rpipe.has_state(PIPE_LOCK));
    assert_eq!(pp.pp_rpipe.pipe_busy.get(), 0);

    // The other direction is independent.
    assert_eq!(write(rf, b"back"), (Ok(()), 4));
    assert_eq!(read(wf, 8), (Ok(()), b"back".to_vec()));

    close(wf);
    close(rf);
}

#[test]
fn write_wraps_around_the_buffer() {
    let _g = setup();
    let (pp, rf, wf) = test_pair();
    let pb = &pp.pp_rpipe.pipe_buffer;

    assert_eq!(write(wf, b"0123456789"), (Ok(()), 10));
    assert_eq!(read(rf, 4), (Ok(()), b"0123".to_vec()));
    assert_eq!((pb.cnt.get(), pb.r#in.get(), pb.out.get()), (6, 10, 4));
    // Six bytes fit before the end, two go to the start.
    assert_eq!(write(wf, b"abcdefgh"), (Ok(()), 8));
    assert_eq!((pb.cnt.get(), pb.r#in.get(), pb.out.get()), (14, 2, 4));
    // The read stops at the end of the buffer, then continues at its start.
    assert_eq!(read(rf, 14), (Ok(()), b"456789abcdefgh".to_vec()));
    assert_eq!(pb.cnt.get(), 0);

    close(rf);
    close(wf);
}

#[test]
fn small_writes_are_atomic() {
    let _g = setup();
    let (pp, rf, wf) = test_pair();
    nonblock(wf, true);

    assert_eq!(write(wf, b"0123456789"), (Ok(()), 10));
    // Eight bytes (at most PIPE_BUF) do not fit in the six left: nothing is written.
    assert_eq!(write(wf, b"abcdefgh"), (Err(Errno::EAGAIN), 0));
    assert_eq!(pp.pp_rpipe.pipe_buffer.cnt.get(), 10);
    assert_eq!(read(rf, SIZE), (Ok(()), b"0123456789".to_vec()));

    // A write larger than PIPE_BUF is not atomic: it fills the buffer, then would block.
    let big = [b'x'; PIPE_BUF + 88];
    assert_eq!(write(wf, &big), (Err(Errno::EAGAIN), SIZE));
    assert_eq!(pp.pp_rpipe.pipe_buffer.cnt.get(), SIZE as u32);
    assert_eq!(pp.pp_rpipe.pipe_busy.get(), 0);

    close(rf);
    close(wf);
}

#[test]
fn eof_after_the_writer_closes() {
    let _g = setup();
    let out = PIPE_PAIR_POOL.pr_nout.get();
    let (pp, rf, wf) = test_pair();

    assert_eq!(write(wf, b"bye"), (Ok(()), 3));
    close(wf);
    assert!(pp.pp_rpipe.has_state(PIPE_EOF));
    assert!(pp.pp_rpipe.pipe_peer.get().is_null());

    // What was written is still read, then EOF: 0 bytes, no error, no waiting.
    assert_eq!(read(rf, 8), (Ok(()), b"bye".to_vec()));
    assert_eq!(read(rf, 8), (Ok(()), Vec::new()));
    // Nobody reads what rf would write.
    assert_eq!(write(rf, b"x"), (Err(Errno::EPIPE), 0));

    // The second close returns the pair to the pool.
    close(rf);
    assert_eq!(PIPE_PAIR_POOL.pr_nout.get(), out);
}

#[test]
fn epipe_after_the_reader_closes() {
    let _g = setup();
    let out = PIPE_PAIR_POOL.pr_nout.get();
    let (pp, rf, wf) = test_pair();

    close(rf);
    assert!(pp.pp_wpipe.has_state(PIPE_EOF));
    assert_eq!(write(wf, b"lost"), (Err(Errno::EPIPE), 0));
    // The reverse direction sees EOF.
    assert_eq!(read(wf, 4), (Ok(()), Vec::new()));

    close(wf);
    assert_eq!(PIPE_PAIR_POOL.pr_nout.get(), out);
}

#[test]
fn ioctl_and_stat() {
    let _g = setup();
    let (pp, rf, wf) = test_pair();
    let p = thread();

    assert_eq!(write(wf, b"abc"), (Ok(()), 3));
    let mut data = [0u8; 8];
    assert_eq!(pipe_ioctl(rf, FIONREAD, &mut data, p), Ok(()));
    assert_eq!(int_arg(&data), 3);

    set_int_arg(&mut data, 1);
    assert_eq!(pipe_ioctl(rf, FIOASYNC, &mut data, p), Ok(()));
    assert!(pp.pp_rpipe.has_state(PIPE_ASYNC));
    set_int_arg(&mut data, 0);
    assert_eq!(pipe_ioctl(rf, FIOASYNC, &mut data, p), Ok(()));
    assert!(!pp.pp_rpipe.has_state(PIPE_ASYNC));
    // sys_ioctl handles FIONBIO itself; the pipe does not.
    assert_eq!(pipe_ioctl(rf, FIONBIO, &mut data, p), Err(Errno::ENOTTY));

    let mut st = Stat::default();
    assert_eq!(pipe_stat(rf, &mut st, p), Ok(()));
    assert_eq!(st.st_mode, S_IFIFO);
    assert_eq!(
        (st.st_size, st.st_blksize, st.st_blocks),
        (3, SIZE as i32, 1)
    );
    assert_eq!((st.st_uid, st.st_gid, st.st_nlink), (0, 0, 0));

    close(rf);
    close(wf);
}

#[test]
fn rundown_and_filters() {
    let _g = setup();
    let (pp, rf, wf) = test_pair();
    let (r, w) = (&pp.pp_rpipe, &pp.pp_wpipe);

    assert_eq!(write(wf, b"abc"), (Ok(()), 3));

    rw_enter_write(&pp.pp_lock);
    // Readable with 3 bytes; the write side has 16 bytes of room, less than PIPE_BUF.
    let rd = filt_piperead(r, false);
    assert_eq!((rd.active, rd.kn_data, rd.eof), (true, Some(3), false));
    let wr = filt_pipewrite(w, false);
    assert_eq!((wr.active, wr.kn_data), (false, Some(13)));
    assert_eq!(filt_pipeexcept(r, true), PipeFilter::default());

    w.pipe_busy.set(1);
    w.set_state(PIPE_WANTD | PIPE_WANTR);
    assert!(!pipe_rundown(w));
    w.pipe_busy.set(0);
    assert!(pipe_rundown(w));
    assert!(!w.has_state(PIPE_WANTD | PIPE_WANTR));
    rw_exit_write(&pp.pp_lock);

    close(rf);
    rw_enter_write(&pp.pp_lock);
    let wr = filt_pipewrite(w, true);
    assert_eq!(
        wr,
        PipeFilter {
            active: true,
            kn_data: Some(0),
            eof: true,
            hup: true
        }
    );
    let ex = filt_pipeexcept(w, true);
    assert!(ex.active && ex.hup);
    assert!(!filt_pipeexcept(w, false).active);
    rw_exit_write(&pp.pp_lock);

    close(wf);
}
