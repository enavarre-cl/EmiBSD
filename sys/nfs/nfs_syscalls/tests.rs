//! Host tests for `nfs_syscalls.c`: the client address check of `nfssvc_checknam`, the server's
//! socket list (`nfsrv_init`, `nfsrv_slpderef`, `nfsrv_zapsock`, `nfsrv_getslp`) and the nfsiod
//! bookkeeping.

use std::boxed::Box;

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::netinet::in_::{InAddr, SockaddrIn};
use crate::sys::mbuf::{M_WAIT, MT_SONAME};
use crate::sys::socket::AF_INET;

/// An `MT_SONAME` mbuf holding an IPv4 address with the given port (host order).
fn client(port: u16, len: u8) -> &'static Mbuf {
    let sin = SockaddrIn {
        sin_len: len,
        sin_family: AF_INET,
        sin_port: port.to_be(),
        sin_addr: InAddr {
            s_addr: u32::from_be_bytes([10, 0, 0, 1]).to_be(),
        },
        sin_zero: [0; 8],
    };
    let m = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
    // SAFETY: an mbuf's data area holds `MLEN` bytes, more than a `sockaddr_in`; the write is
    // unaligned-safe.
    unsafe { mtod::<SockaddrIn>(m).write_unaligned(sin) };
    m.m_len().set(u32::from(len));
    m
}

#[cfg(feature = "nfsserver")]
#[test]
fn client_address_must_be_ipv4_with_a_reserved_port() {
    let _g = setup();
    let sin_len = size_of::<SockaddrIn>() as u8;

    assert!(nfssvc_checknam(Some(client(700, sin_len))));
    assert!(nfssvc_checknam(Some(client(1023, sin_len))));
    // An unprivileged port, no address at all and a malformed one are refused.
    assert!(!nfssvc_checknam(Some(client(1024, sin_len))));
    assert!(!nfssvc_checknam(Some(client(40000, sin_len))));
    assert!(!nfssvc_checknam(None));
    assert!(!nfssvc_checknam(Some(client(700, sin_len - 1))));
}

#[cfg(feature = "nfsserver")]
#[test]
fn server_socket_list_bookkeeping() {
    let _g = setup();
    nfsrv_init(0);

    // The datagram socket is always the first on the list.
    let udp = nfs_udpsock();
    assert!(core::ptr::eq(
        NFSSVC_SOCKHEAD.0.first().expect("the list is not empty"),
        udp
    ));
    assert_eq!(udp.ns_flag.get(), 0);
    assert!(NFSD_HEAD.0.is_empty());

    // A stream socket: linked after it.
    let tcp = nfssvc_sock_alloc().expect("memory");
    // SAFETY: a fresh socket on no list, which stays in place until `nfsrv_slpderef` frees it.
    unsafe { NFSSVC_SOCKHEAD.0.insert_tail(tcp) };
    assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 2);

    // Valid or referenced: dereferencing keeps it.
    tcp.ns_flag.set(SLP_VALID);
    tcp.ns_sref.set(2);
    nfsrv_slpderef(tcp);
    assert_eq!(tcp.ns_sref.get(), 1);
    nfsrv_slpderef(tcp);
    assert_eq!(tcp.ns_sref.get(), 0);
    assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 2);
    // No longer valid and the last reference gone: unlinked and freed.
    tcp.ns_flag.set(0);
    tcp.ns_sref.set(1);
    nfsrv_slpderef(tcp);
    assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);

    // The datagram socket is reused, never freed.
    udp.ns_sref.set(1);
    nfsrv_slpderef(udp);
    assert!(core::ptr::eq(NFS_UDPSOCK.load(Relaxed), udp));
    assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);

    // An nfsd looks for a socket with records (NFSD_CHECKSLP): finds the valid one that has
    // `SLP_DOREC`, takes a reference and clears the flag; with none left it clears NFSD_CHECKSLP.
    let nfsd: &'static Nfsd = Box::leak(Box::new(Nfsd::new()));
    udp.ns_sref.set(0);
    udp.ns_flag.set(SLP_VALID | SLP_DOREC);
    NFSD_HEAD_FLAG.fetch_or(NFSD_CHECKSLP, Relaxed);
    assert_eq!(nfsrv_getslp(nfsd), Ok(()));
    assert!(nfsd.nfsd_slp.get().is_some_and(|s| core::ptr::eq(s, udp)));
    assert_eq!(udp.ns_sref.get(), 1);
    assert_eq!(udp.ns_flag.get(), SLP_VALID);
    // One that already has its socket is served at once.
    assert_eq!(nfsrv_getslp(nfsd), Ok(()));
    assert_eq!(udp.ns_sref.get(), 1);

    // Zapping a socket without a file only clears its flags.
    udp.ns_flag.set(SLP_VALID | SLP_NEEDQ | SLP_DOREC);
    nfsrv_zapsock(udp);
    assert_eq!(udp.ns_flag.get(), 0);

    // Terminating: every socket goes, a new datagram socket is made, the server's lists restart.
    nfsrv_init(1);
    assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);
    assert_eq!(nfs_udpsock().ns_flag.get(), 0);
    assert!(NFSD_HEAD.0.is_empty());
    assert_eq!(NFSD_HEAD_FLAG.load(Relaxed) & NFSD_CHECKSLP, 0);
}

#[cfg(feature = "nfsserver")]
#[test]
fn the_temporary_procedure_table_covers_every_procedure() {
    assert_eq!(NFSRV3_PROCS.len(), NFS_NPROCS);
}

#[cfg(feature = "nfsclient")]
#[test]
fn nfsiod_threads_are_counted_and_the_sysctl_value_follows() {
    // Nothing runs: the slots are free and the count is zero.
    assert!(NFS_ASYNCDAEMON.iter().all(|p| p.load(Relaxed).is_null()));
    assert_eq!(NFS_NUMASYNC.load(Relaxed), 0);

    // Reading (`set` false) makes `nfs_niothreads` the number of running iods, but only once
    // it has been initialised (-1 means "not yet").
    let saved = nfs_niothreads.swap(-1, Relaxed);
    nfs_getset_niothreads(false);
    assert_eq!(nfs_niothreads.load(Relaxed), -1);
    nfs_niothreads.store(4, Relaxed);
    nfs_getset_niothreads(false);
    assert_eq!(nfs_niothreads.load(Relaxed), 0);
    nfs_niothreads.store(saved, Relaxed);
}
