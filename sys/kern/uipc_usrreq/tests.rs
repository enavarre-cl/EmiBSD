//! Host tests for the UNIX domain and the socket layer above it, end to end through the
//! kernel functions: stream, datagram and sequenced packet pairs (`socreate`,
//! `soconnect2`, `sosend`, `soreceive`, `soshutdown`, `soclose`), the accept queue
//! (`sonewconn`, `soisconnected`, `soqremque`, `soaccept`, a listener closed with a pending
//! connection), the paths through `namei` on the test file system (`uipc_bind`,
//! `unp_connect`, `unp_nam2sun`), and descriptor passing through the system calls
//! (`socketpair`, `sendmsg`, `recvmsg` with `SCM_RIGHTS`), with the rights of a closed
//! receiver and a cycle of sockets collected by `unp_gc`.
//!
//! The tests run as the thread `vfs_subr`'s setup builds, made `curproc` (the socket layer
//! reads its credentials and descriptor table); they clear `curproc` and dequeue the
//! collector's task before they return, since the task queue outlives the reset memory.

use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_descrip::sys_close;
use crate::kern::kern_task::task_del;
use crate::kern::sys_generic::{sys_read, sys_write};
use crate::kern::uipc_mbuf::m_get;
use crate::kern::uipc_mbuf::tests::mbinit_again;
use crate::kern::uipc_socket::{
    soaccept, sobind, soclose, soconnect, soconnect2, socreate, sogetopt, soinit, soreceive,
    sosend, soshutdown,
};
use crate::kern::uipc_socket2::{
    solock_nonet, solock_shared, soqremque, sounlock_nonet, sounlock_shared,
};
use crate::kern::uipc_syscalls::{sys_recvmsg, sys_sendmsg, sys_socketpair};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::mbuf::MT_SOOPTS;
use crate::sys::resource::{RLIM_INFINITY, RLIMIT_NOFILE, Rlimit};
use crate::sys::resourcevar::Plimit;
use crate::sys::socket::{
    MSG_DONTWAIT, MSG_NOSIGNAL, MSG_TRUNC, Msghdr, SHUT_WR, SO_PEERCRED, SO_TYPE,
};
use crate::sys::socketvar::{SS_CANTRCVMORE, SS_ISCONNECTED, SS_NOFDREF};
use crate::sys::systm::SysArgs;
use crate::sys::types::Register;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

/// The vfs setup (memory, pools, a thread with a descriptor table), mbufs, the socket and
/// control block pools, empty global lists; the thread is `curproc`.
pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    mbinit_again();
    soinit();
    unp_init();
    UNP_HEAD.0.init();
    UNP_DEFERRED.0.init();
    UNP_RIGHTS.store(0, Ordering::Relaxed);
    UNP_DEFER.store(0, Ordering::Relaxed);
    UNP_GCING.store(0, Ordering::Relaxed);
    Machine::set_curproc(Machine::curcpu(), p);
    // fdalloc reads RLIMIT_NOFILE; no other limit matters here.
    let limit: &'static Plimit = std::boxed::Box::leak(std::boxed::Box::new(Plimit::new()));
    for l in &limit.pl_rlimit {
        l.set(Rlimit {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        });
    }
    limit.pl_rlimit[RLIMIT_NOFILE].set(Rlimit {
        rlim_cur: 64,
        rlim_max: 64,
    });
    p.process().ps_limit.set(limit);
    p.p_limit.set(limit);
    (g, p)
}

/// Undoes what outlives the reset memory: the collector's task on `systqmp`, `curproc`.
pub(crate) fn teardown() {
    let _ = task_del(SYSTQMP, &UNP_GC_TASK);
    UNP_HEAD.0.init();
    UNP_DEFERRED.0.init();
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// `sosend` of `bytes` from kernel space; the bytes taken.
pub(crate) fn send(so: &'static Socket, bytes: &[u8], flags: i32) -> Result<usize, Errno> {
    let mut iov = [Iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: bytes.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    sosend(so, None, Some(&mut uio), None, None, flags)?;
    Ok(bytes.len() - uio.uio_resid)
}

/// `soreceive` into `buf` in kernel space; the bytes read and the flags returned.
pub(crate) fn recv(so: &'static Socket, buf: &mut [u8], flags: i32) -> Result<(usize, i32), Errno> {
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
    let mut flags = flags;
    soreceive(so, None, &mut uio, None, None, Some(&mut flags), 0)?;
    Ok((len - uio.uio_resid, flags & !MSG_DONTWAIT))
}

/// Two connected sockets of `type_`, as `socketpair(2)` makes them.
fn pair(type_: i32) -> (&'static Socket, &'static Socket) {
    let a = socreate(i32::from(AF_UNIX), type_, 0).expect("socreate");
    let b = socreate(i32::from(AF_UNIX), type_, 0).expect("socreate");
    assert_eq!(soconnect2(a, b), Ok(()));
    if type_ == SOCK_DGRAM {
        assert_eq!(soconnect2(b, a), Ok(()));
    }
    (a, b)
}

/// An `MT_SONAME` mbuf holding `sun_len`, `AF_UNIX` and `path` (no NUL added).
fn sun(path: &[u8]) -> &'static Mbuf {
    let m = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
    let len = SockaddrUn::PATH_OFFSET + path.len();
    // SAFETY: a fresh mbuf of `MLEN` bytes; the tests' paths fit.
    unsafe {
        *mtod::<u8>(m) = len as u8;
        *mtod::<u8>(m).add(1) = AF_UNIX;
        ptr::copy_nonoverlapping(path.as_ptr(), mtod::<u8>(m).add(2), path.len());
    }
    m.m_len().set(len as u32);
    m
}

#[test]
fn stream_pair_moves_bytes_both_ways_and_shuts_down() {
    let (_g, p) = setup();
    let (a, b) = pair(SOCK_STREAM);
    assert!(a.has_state(SS_ISCONNECTED) && b.has_state(SS_ISCONNECTED));
    let mut buf = [0u8; 16];

    assert_eq!(send(a, b"hello", 0), Ok(5));
    // The sender's buffer mirrors the receiver's, for back pressure.
    assert_eq!(b.so_rcv.sb_cc.get(), 5);
    assert_eq!(a.so_snd.sb_cc.get(), 5);
    assert_eq!(recv(b, &mut buf[..2], 0), Ok((2, 0)));
    assert_eq!(&buf[..2], b"he");
    assert_eq!(a.so_snd.sb_cc.get(), 3);
    assert_eq!(recv(b, &mut buf, 0), Ok((3, 0)));
    assert_eq!(&buf[..3], b"llo");
    assert_eq!(a.so_snd.sb_cc.get(), 0);
    assert_eq!(recv(b, &mut buf, MSG_DONTWAIT), Err(Errno::EWOULDBLOCK));

    assert_eq!(send(b, b"world", 0), Ok(5));
    assert_eq!(recv(a, &mut buf, 0), Ok((5, 0)));
    assert_eq!(&buf[..5], b"world");

    // socketpair(2) records the creator as both peers.
    let m = m_get(M_WAIT, MT_SOOPTS).expect("an mbuf");
    assert_eq!(sogetopt(a, SOL_SOCKET, SO_PEERCRED, m), Ok(()));
    // SAFETY: sogetopt stored the credentials.
    let cred = unsafe { mtod::<Sockpeercred>(m).read_unaligned() };
    assert_eq!(cred.uid, p.ucred().cr_uid.get());
    assert_eq!(sogetopt(b, SOL_SOCKET, SO_TYPE, m), Ok(()));
    // SAFETY: an int.
    assert_eq!(unsafe { mtod::<i32>(m).read_unaligned() }, SOCK_STREAM);
    m_freem(Some(m));

    // Shut down a's sending side: b reads EOF, a cannot send, b still can.
    assert_eq!(soshutdown(a, SHUT_WR), Ok(()));
    assert!(b.so_rcv.has_state(SS_CANTRCVMORE));
    assert_eq!(recv(b, &mut buf, 0), Ok((0, 0)));
    assert_eq!(send(a, b"x", MSG_NOSIGNAL), Err(Errno::EPIPE));
    assert_eq!(send(b, b"!", 0), Ok(1));
    assert_eq!(recv(a, &mut buf, 0), Ok((1, 0)));

    // Closing a disconnects b.
    assert_eq!(soclose(a, 0), Ok(()));
    assert!(!b.has_state(SS_ISCONNECTED));
    assert_eq!(send(b, b"?", MSG_NOSIGNAL), Err(Errno::EPIPE));
    assert_eq!(recv(b, &mut buf, 0), Ok((0, 0)));
    assert_eq!(soclose(b, 0), Ok(()));
    assert!(UNP_HEAD.0.first().is_none());
    teardown();
}

#[test]
fn datagrams_keep_their_boundaries_and_sender() {
    let (_g, _p) = setup();
    let (a, b) = pair(SOCK_DGRAM);
    let mut buf = [0u8; 16];

    assert_eq!(send(a, b"first", 0), Ok(5));
    assert_eq!(send(a, b"second", 0), Ok(6));

    // One record per read, with the sender's address (unbound: sun_noname).
    let mut from = None;
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 16,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut flags = 0;
    assert_eq!(
        soreceive(
            b,
            Some(&mut from),
            &mut uio,
            None,
            None,
            Some(&mut flags),
            0
        ),
        Ok(())
    );
    assert_eq!(16 - uio.uio_resid, 5);
    assert_eq!(&buf[..5], b"first");
    let from = from.expect("an address");
    assert_eq!(from.m_len().get() as usize, SUN_NONAME_LEN);
    // SAFETY: a socket address.
    assert_eq!(
        unsafe { mtod::<Sockaddr>(from).read_unaligned() },
        SUN_NONAME
    );
    m_freem(Some(from));

    // A short read truncates the datagram and drops the rest of it.
    assert_eq!(recv(b, &mut buf[..3], 0), Ok((3, MSG_TRUNC)));
    assert_eq!(&buf[..3], b"sec");
    assert_eq!(recv(b, &mut buf, MSG_DONTWAIT), Err(Errno::EWOULDBLOCK));

    // The pair is connected both ways.
    assert_eq!(send(b, b"back", 0), Ok(4));
    assert_eq!(recv(a, &mut buf, 0), Ok((4, 0)));
    assert_eq!(soclose(a, 0), Ok(()));
    assert_eq!(soclose(b, 0), Ok(()));
    teardown();
}

#[test]
fn seqpacket_keeps_records() {
    let (_g, _p) = setup();
    let (a, b) = pair(SOCK_SEQPACKET);
    let mut buf = [0u8; 16];

    assert_eq!(send(a, b"ab", 0), Ok(2));
    assert_eq!(send(a, b"cd", 0), Ok(2));
    assert_eq!(recv(b, &mut buf, 0), Ok((2, 0)));
    assert_eq!(&buf[..2], b"ab");
    assert_eq!(recv(b, &mut buf, 0), Ok((2, 0)));
    assert_eq!(&buf[..2], b"cd");
    assert_eq!(soclose(a, 0), Ok(()));
    assert_eq!(soclose(b, 0), Ok(()));
    teardown();
}

/// Connects `client` to the listener `l` as `unp_connect` does once `namei` found it: a new
/// socket from `sonewconn`, connected with `unp_connect2`.
fn connect_to(client: &'static Socket, l: &'static Socket) -> &'static Socket {
    solock(l);
    let so3 = sonewconn(l, 0, M_WAIT).expect("sonewconn");
    sounlock(l);
    sounlock(so3);
    solock_pair(client, so3);
    assert_eq!(unp_connect2(client, so3), Ok(()));
    sounlock_pair(client, so3);
    so3
}

#[test]
fn the_accept_queue() {
    let (_g, _p) = setup();
    let l = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");

    // Only a bound socket listens.
    solock_shared(l);
    assert_eq!(crate::kern::uipc_socket::solisten(l, 5), Err(Errno::EINVAL));
    sounlock_shared(l);
    l.so_options.set(l.so_options.get() | SO_ACCEPTCONN);
    l.so_qlimit.set(5);

    let client = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");
    let so3 = connect_to(client, l);
    // soisconnected moved the new socket from the partial queue to the complete one.
    assert_eq!((l.so_q0len.get(), l.so_qlen.get()), (0, 1));
    assert!(so3.has_state(SS_NOFDREF | SS_ISCONNECTED));

    // accept(2): off the queue, a file reference, the peer's name.
    solock_shared(l);
    let first = l.so_q.first().expect("a connection");
    assert!(ptr::eq(first, so3));
    solock_nonet(so3);
    assert!(soqremque(so3, 1));
    sounlock_nonet(l);
    let nam = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
    assert_eq!(soaccept(so3, nam), Ok(()));
    sounlock_shared(so3);
    assert_eq!(nam.m_len().get() as usize, SUN_NONAME_LEN);
    m_freem(Some(nam));
    assert!(!so3.has_state(SS_NOFDREF));

    let mut buf = [0u8; 8];
    assert_eq!(send(client, b"ping", 0), Ok(4));
    assert_eq!(recv(so3, &mut buf, 0), Ok((4, 0)));
    assert_eq!(send(so3, b"pong", 0), Ok(4));
    assert_eq!(recv(client, &mut buf, 0), Ok((4, 0)));
    assert_eq!(&buf[..4], b"pong");

    // A connection still on the queue is aborted with its listener.
    let client2 = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");
    let _pending = connect_to(client2, l);
    assert_eq!(l.so_qlen.get(), 1);
    assert_eq!(soclose(l, 0), Ok(()));
    assert!(!client2.has_state(SS_ISCONNECTED));
    assert_eq!(recv(client2, &mut buf, 0), Ok((0, 0)));

    for so in [client, so3, client2] {
        assert_eq!(soclose(so, 0), Ok(()));
    }
    assert!(UNP_HEAD.0.first().is_none());
    teardown();
}

#[test]
fn bind_and_connect_go_through_namei() {
    let (_g, p) = setup();
    let _mp = crate::kern::vfs_subr::tests::testfs::mount_root(p);
    let so = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");

    let bind = |path: &[u8]| {
        let nam = sun(path);
        solock_shared(so);
        let error = sobind(so, nam, p);
        sounlock_shared(so);
        m_freem(Some(nam));
        error
    };
    let connect = |path: &[u8]| {
        let nam = sun(path);
        solock(so);
        let error = soconnect(so, nam);
        sounlock(so);
        m_freem(Some(nam));
        error
    };

    // An existing name is in use; unp_nam2sun adds the missing NUL.
    assert_eq!(bind(b"/a/b"), Err(Errno::EADDRINUSE));
    assert_eq!(connect(b"/a/b"), Err(Errno::ENOTSOCK));
    assert_eq!(connect(b"/nonexistent"), Err(Errno::ENOENT));
    // A path that fills sun_path leaves no room for the NUL.
    assert_eq!(bind(&[b'x'; SUN_PATH_LEN]), Err(Errno::EINVAL));
    let inet = sun(b"/a/b");
    // SAFETY: the family byte of the address.
    unsafe { *mtod::<u8>(inet).add(1) = 2 };
    assert_eq!(unp_nam2sun(inet), Err(Errno::EAFNOSUPPORT));
    m_freem(Some(inet));
    assert!(sotounpcb(so).is_some_and(|unp| unp.unp_vnode.get().is_none()));

    assert_eq!(soclose(so, 0), Ok(()));
    teardown();
}

/// A system call's argument block from its arguments.
fn args(a: &[usize]) -> SysArgs {
    let mut v: SysArgs = [0; 6];
    for (slot, &x) in v.iter_mut().zip(a) {
        *slot = x as Register;
    }
    v
}

/// `socketpair(AF_UNIX, SOCK_STREAM, 0, sv)`.
pub(crate) fn socketpair(p: &Proc) -> [i32; 2] {
    let mut sv = [-1i32; 2];
    let mut retval = [0; 2];
    let v = args(&[
        usize::from(AF_UNIX),
        SOCK_STREAM as usize,
        0,
        sv.as_mut_ptr() as usize,
    ]);
    assert_eq!(sys_socketpair(p, &v, &mut retval), Ok(()));
    sv
}

/// `sendmsg(s, msg, 0)` of one byte with `fd` in an `SCM_RIGHTS` message.
fn send_fd(p: &Proc, s: i32, fd: i32) {
    let byte = [b'x'];
    let mut iov = [Iovec {
        iov_base: byte.as_ptr().cast_mut().cast(),
        iov_len: 1,
    }];
    let mut cbuf = vec![0u8; cmsg_space(4)];
    let cm = Cmsghdr {
        cmsg_len: cmsg_len(4) as Socklen,
        cmsg_level: SOL_SOCKET,
        cmsg_type: SCM_RIGHTS,
    };
    // SAFETY: the buffer holds a header and an int.
    unsafe {
        cbuf.as_mut_ptr().cast::<Cmsghdr>().write_unaligned(cm);
        cbuf.as_mut_ptr().add(16).cast::<i32>().write_unaligned(fd);
    }
    let msg = Msghdr {
        msg_name: ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: iov.as_mut_ptr().cast(),
        msg_iovlen: 1,
        msg_control: cbuf.as_mut_ptr().cast(),
        msg_controllen: cbuf.len() as Socklen,
        msg_flags: 0,
    };
    let mut retval = [0; 2];
    let v = args(&[s as usize, ptr::from_ref(&msg) as usize, 0]);
    assert_eq!(sys_sendmsg(p, &v, &mut retval), Ok(()));
    assert_eq!(retval[0], 1);
}

/// `close(fd)`.
pub(crate) fn close(p: &Proc, fd: i32) {
    let mut retval = [0; 2];
    assert_eq!(sys_close(p, &args(&[fd as usize]), &mut retval), Ok(()));
}

#[test]
fn descriptors_pass_through_scm_rights() {
    let (_g, p) = setup();
    let [a0, a1] = socketpair(p);
    let [b0, b1] = socketpair(p);
    let fp_b0 = crate::kern::kern_descrip::fd_getfile(p.fd(), b0).expect("b0");

    send_fd(p, a0, b0);
    assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 1);

    let mut byte = [0u8; 1];
    let mut iov = [Iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    }];
    let mut cbuf = vec![0u8; 64];
    let mut msg = Msghdr {
        msg_name: ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: iov.as_mut_ptr().cast(),
        msg_iovlen: 1,
        msg_control: cbuf.as_mut_ptr().cast(),
        msg_controllen: cbuf.len() as Socklen,
        msg_flags: 0,
    };
    let mut retval = [0; 2];
    let v = args(&[a1 as usize, ptr::from_mut(&mut msg) as usize, 0]);
    assert_eq!(sys_recvmsg(p, &v, &mut retval), Ok(()));
    assert_eq!(retval[0], 1);
    assert_eq!(byte, [b'x']);
    assert_eq!(msg.msg_controllen as usize, cmsg_len(4));
    // SAFETY: recvmsg copied a header and an int out.
    let (cm, newfd) = unsafe {
        (
            cbuf.as_ptr().cast::<Cmsghdr>().read_unaligned(),
            cbuf.as_ptr().add(16).cast::<i32>().read_unaligned(),
        )
    };
    assert_eq!((cm.cmsg_level, cm.cmsg_type), (SOL_SOCKET, SCM_RIGHTS));
    assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
    let fp_new = crate::kern::kern_descrip::fd_getfile(p.fd(), newfd).expect("the new fd");
    assert!(ptr::eq(fp_new, fp_b0));
    let _ = frele(fp_new, p);
    let _ = frele(fp_b0, p);

    // The new descriptor is b0: what it writes, b1 reads.
    let mut retval = [0; 2];
    let w = args(&[newfd as usize, b"via".as_ptr() as usize, 3]);
    assert_eq!(sys_write(p, &w, &mut retval), Ok(()));
    let mut got = [0u8; 8];
    let r = args(&[b1 as usize, got.as_mut_ptr() as usize, got.len()]);
    assert_eq!(sys_read(p, &r, &mut retval), Ok(()));
    assert_eq!(&got[..retval[0] as usize], b"via");

    for fd in [a0, a1, b0, b1, newfd] {
        close(p, fd);
    }
    assert!(UNP_HEAD.0.first().is_none());
    teardown();
}

#[test]
fn rights_never_received_are_collected() {
    let (_g, p) = setup();
    let [a0, a1] = socketpair(p);
    let [b0, b1] = socketpair(p);
    let fp_b0 = crate::kern::kern_descrip::fd_getfile(p.fd(), b0).expect("b0");
    let count = |fp: &File| fp.f_count.load(Ordering::SeqCst);
    // The table's reference and ours.
    assert_eq!(count(fp_b0), 2);

    send_fd(p, a0, b0);
    assert_eq!(count(fp_b0), 3);

    // The receiver goes away with the message in its buffer: the file is handed to the
    // collector, which closes the message's reference.
    close(p, a1);
    assert!(UNP_DEFERRED.0.first().is_some());
    unp_gc(ptr::null_mut());
    assert!(UNP_DEFERRED.0.first().is_none());
    assert_eq!(count(fp_b0), 2);
    assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
    let _ = frele(fp_b0, p);

    for fd in [a0, b0, b1] {
        close(p, fd);
    }
    assert!(UNP_HEAD.0.first().is_none());
    teardown();
}

#[test]
fn a_cycle_of_sockets_is_collected() {
    let (_g, p) = setup();
    let [a0, a1] = socketpair(p);

    // a1's own descriptor goes into a1's receive buffer; then both descriptors close. a1
    // is now referenced only by the message in its own buffer.
    send_fd(p, a0, a1);
    close(p, a0);
    close(p, a1);
    let live: Vec<_> = UNP_HEAD.0.iter().collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].unp_msgcount.get(), 1);

    // The first pass finds the cycle and discards the buffer; the second closes the file.
    unp_gc(ptr::null_mut());
    assert!(UNP_DEFERRED.0.first().is_some());
    unp_gc(ptr::null_mut());
    assert!(UNP_HEAD.0.first().is_none());
    assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
    teardown();
}
