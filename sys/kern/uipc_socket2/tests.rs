//! Host tests for the socket buffer routines: the accounting of `sballoc`/`sbfree` through
//! `sbappend` (with `sbcompress` folding small mbufs together), records (`sbappendrecord`,
//! `sbappendaddr`, `sbappendcontrol`, `sbdroprecord`), `sbdrop`, `sbflush`, `sbreserve`'s
//! limits and `sbcreatecontrol`'s message.

use std::boxed::Box;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::kern::uipc_mbuf::{m_freem, m_gethdr};
use crate::sys::mbuf::{MT_DATA, MT_HEADER};
use crate::sys::socket::{AF_UNIX, SCM_RIGHTS, SOL_SOCKET};

/// A data mbuf holding `bytes`; a packet header when `pkthdr`.
fn data(bytes: &[u8], pkthdr: bool) -> &'static Mbuf {
    let m = if pkthdr {
        m_gethdr(M_DONTWAIT, MT_HEADER)
    } else {
        m_get(M_DONTWAIT, MT_DATA)
    };
    let m = m.expect("an mbuf");
    // SAFETY: a fresh mbuf holds `MLEN` bytes; the tests stay below.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), mtod::<u8>(m), bytes.len()) };
    m.m_len().set(bytes.len() as u32);
    if pkthdr {
        m.m_pkthdr().len.set(bytes.len() as i32);
    }
    m
}

/// The bytes of the record `m` (through `m_next`).
fn record(m: Option<&Mbuf>) -> Vec<u8> {
    let mut v = Vec::new();
    let mut m = m;
    while let Some(mm) = m {
        // SAFETY: an mbuf holds `m_len` bytes of data.
        let b = unsafe { core::slice::from_raw_parts(mtod::<u8>(mm), mm.m_len().get() as usize) };
        v.extend_from_slice(b);
        m = mm.m_next().get();
    }
    v
}

/// A buffer of `hiwat` bytes, its mutex held.
fn sockbuf(hiwat: u64) -> &'static Sockbuf {
    let sb: &'static Sockbuf = Box::leak(Box::new(Sockbuf::new()));
    mtx_enter(&sb.sb_mtx);
    assert_eq!(sbreserve(sb, hiwat), Ok(()));
    sb
}

#[test]
fn sbappend_counts_and_compresses() {
    let _g = setup();
    let sb = sockbuf(4096);
    assert_eq!(sb.sb_hiwat.get(), 4096);
    assert_eq!(sb.sb_mbmax.get(), (3 * MAXMCLBYTES as u64).max(4096 * 8));

    sbappend(sb, Some(data(b"abc", false)));
    assert_eq!(sb.sb_cc.get(), 3);
    assert_eq!(sb.sb_datacc.get(), 3);
    assert_eq!(sb.sb_mbcnt.get(), MSIZE as u64);
    assert!(same(sb.sb_mb.get(), sb.sb_lastrecord.get()));
    assert!(same(sb.sb_mb.get(), sb.sb_mbtail.get()));

    // A small mbuf is copied into the room after the last one, and freed.
    sbappend(sb, Some(data(b"def", false)));
    assert_eq!(sb.sb_cc.get(), 6);
    assert_eq!(sb.sb_mbcnt.get(), MSIZE as u64);
    assert_eq!(record(sb.sb_mb.get()), b"abcdef");
    assert_eq!(
        sbspace_locked(sb),
        (4096 - 6i64).min(sb.sb_mbmax.get() as i64 - MSIZE as i64)
    );

    // An empty mbuf disappears.
    sbappend(sb, Some(data(b"", false)));
    assert_eq!(sb.sb_mbcnt.get(), MSIZE as u64);

    sbdrop(sb, 4);
    assert_eq!(sb.sb_cc.get(), 2);
    assert_eq!(record(sb.sb_mb.get()), b"ef");
    sbdrop(sb, 2);
    assert!(sb.sb_mb.get().is_none());
    assert!(sb.sb_mbtail.get().is_none());
    assert!(sb.sb_lastrecord.get().is_none());
    assert_eq!(
        (sb.sb_cc.get(), sb.sb_datacc.get(), sb.sb_mbcnt.get()),
        (0, 0, 0)
    );
    mtx_leave(&sb.sb_mtx);
}

#[test]
fn records_with_address_and_control() {
    let _g = setup();
    let sb = sockbuf(4096);

    sbappendrecord(sb, Some(data(b"one", false)));
    sbappendrecord(sb, Some(data(b"two", false)));
    let first = sb.sb_mb.get().expect("a record");
    let second = first.m_nextpkt().get().expect("a second record");
    assert!(same(sb.sb_lastrecord.get(), Some(second)));
    assert_eq!(record(Some(second)), b"two");

    // An address record: the name, the control, then the data; only the data is "data".
    let sun_noname = [16u8, AF_UNIX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let control = sbcreatecontrol(&7i32.to_ne_bytes(), SCM_RIGHTS, SOL_SOCKET).expect("cmsg");
    let clen = u64::from(control.m_len().get());
    assert!(sbappendaddr(
        sb,
        &sun_noname,
        Some(data(b"dgram", true)),
        Some(control)
    ));
    let third = second.m_nextpkt().get().expect("a third record");
    assert_eq!(i32::from(third.m_type().get()), MT_SONAME);
    assert_eq!(third.m_len().get(), 16);
    let ctl = third.m_next().get().expect("control");
    assert_eq!(i32::from(ctl.m_type().get()), MT_CONTROL);
    assert_eq!(record(ctl.m_next().get()), b"dgram");
    assert_eq!(sb.sb_cc.get(), 3 + 3 + 16 + clen + 5);
    assert_eq!(sb.sb_datacc.get(), 3 + 3 + 5);
    assert!(same(sb.sb_lastrecord.get(), Some(third)));

    // Control and data as one record.
    let control = sbcreatecontrol(&[1, 2, 3, 4], SCM_RIGHTS, SOL_SOCKET).expect("cmsg");
    assert!(sbappendcontrol(sb, Some(data(b"x", false)), control));
    assert!(same(sb.sb_lastrecord.get(), Some(control)));

    sbdroprecord(sb);
    assert!(same(sb.sb_mb.get(), Some(second)));
    assert_eq!(sb.sb_datacc.get(), 3 + 5 + 1);
    sbflush(sb);
    assert!(sb.sb_mb.get().is_none());
    assert_eq!(sb.sb_mbcnt.get(), 0);
    mtx_leave(&sb.sb_mtx);
}

#[test]
fn appends_that_do_not_fit_fail() {
    let _g = setup();
    let sb = sockbuf(8);
    let addr = [16u8, AF_UNIX, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let m = data(b"0123456789", true);
    assert!(!sbappendaddr(sb, &addr, Some(m), None));
    let control = sbcreatecontrol(&[0; 16], SCM_RIGHTS, SOL_SOCKET).expect("cmsg");
    assert!(!sbappendcontrol(sb, Some(m), control));
    assert!(sb.sb_mb.get().is_none());
    assert_eq!(sb.sb_cc.get(), 0);
    m_freem(Some(m));
    m_freem(Some(control));
    mtx_leave(&sb.sb_mtx);
}

#[test]
fn sbreserve_limits_and_lowat() {
    let _g = setup();
    let sb = sockbuf(1024);
    sb.sb_lowat.set(4096);
    assert_eq!(sbreserve(sb, 0), Err(Errno::ENOBUFS));
    assert_eq!(sbreserve(sb, SB_MAX + 1), Err(Errno::ENOBUFS));
    assert_eq!(sbreserve(sb, 2048), Ok(()));
    assert_eq!(sb.sb_lowat.get(), 2048);
    assert_eq!(sbcheckreserve(1, 2), Ok(()));
    mtx_leave(&sb.sb_mtx);
}

#[test]
fn sbcreatecontrol_builds_one_message() {
    let _g = setup();
    let m = sbcreatecontrol(&[9, 8, 7, 6], SCM_RIGHTS, SOL_SOCKET).expect("cmsg");
    assert_eq!(i32::from(m.m_type().get()), MT_CONTROL);
    assert_eq!(m.m_len().get() as usize, cmsg_space(4));
    // SAFETY: the mbuf holds a whole message.
    let cm = unsafe { mtod::<Cmsghdr>(m).read_unaligned() };
    assert_eq!(cm.cmsg_len as usize, cmsg_len(4));
    assert_eq!((cm.cmsg_level, cm.cmsg_type), (SOL_SOCKET, SCM_RIGHTS));
    assert_eq!(
        &record(Some(m))[cmsg_align(size_of::<Cmsghdr>())..][..4],
        &[9, 8, 7, 6]
    );
    m_free(m);
}
