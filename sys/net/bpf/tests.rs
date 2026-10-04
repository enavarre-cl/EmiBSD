use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::init_main::PROC0;
use crate::kern::uipc_mbuf::m_get;
use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::mbuf::M_DONTWAIT;
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// A two-mbuf chain holding `bytes`, split after `split` bytes.
fn chain(bytes: &[u8], split: usize) -> &'static Mbuf {
    let m = test_packet(&bytes[..split]);
    let n = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
    let rest = &bytes[split..];
    // SAFETY: a fresh mbuf has MLEN bytes at m_data; the test data is shorter.
    unsafe { ptr::copy_nonoverlapping(rest.as_ptr(), n.m_data().get(), rest.len()) };
    n.m_len().set(rest.len() as u32);
    m.m_next().set(Some(n));
    m.m_pkthdr().len.set(bytes.len() as i32);
    m
}

#[test]
fn loads_across_segments() {
    let _g = setup_net();
    let bytes: Vec<u8> = (0u8..20).collect();
    let m = chain(&bytes, 6);
    let pkt = BpfPkt::mbuf(m);
    assert_eq!(bpf_mbuf_ldw(&pkt, 4), Some(0x0405_0607));
    assert_eq!(bpf_mbuf_ldh(&pkt, 5), Some(0x0506));
    assert_eq!(bpf_mbuf_ldb(&pkt, 19), Some(19));
    assert_eq!(bpf_mbuf_ldb(&pkt, 20), None);
    assert_eq!(bpf_mbuf_ldw(&pkt, 17), None);

    // bpf_mtap_af's header in front of the chain.
    let afh = 2u32.to_be_bytes();
    let pkt = BpfPkt {
        hdr: &afh,
        body: BpfBody::Mbuf(m),
    };
    assert_eq!(bpf_mbuf_ldw(&pkt, 0), Some(2));
    assert_eq!(bpf_mbuf_ldw(&pkt, 2), Some(0x0002_0001));
    let mut all = [0u8; 24];
    bpf_mcopy(&pkt, &mut all);
    assert_eq!(&all[4..], &bytes[..]);
    m_freem(m);

    // bpf_tap_hdr's two linear buffers.
    let pkt = BpfPkt {
        hdr: &[0xaa, 0xbb],
        body: BpfBody::Buf(&[1, 2, 3]),
    };
    assert_eq!(bpf_mbuf_ldw(&pkt, 1), Some(0xbb01_0203));
    assert_eq!(pkt.segments().map(<[u8]>::len).sum::<usize>(), 5);
}

#[test]
fn names() {
    assert!(bpf_name_eq(
        b"vio0\0\0\0\0\0\0\0\0\0\0\0\0",
        b"vio0\0garbage"
    ));
    assert!(!bpf_name_eq(b"vio0", b"vio1"));
    assert!(!bpf_name_eq(b"vio", b"vio0"));
}

/// `ioctl` on the descriptor with an argument of type `T`.
fn ioctl<T: AbiPod>(dev: Dev, cmd: u64, arg: &mut T) -> Result<(), Errno> {
    let mut data = vec![0u8; size_of::<T>().max(32)];
    ioctl_ret(&mut data, arg);
    let r = bpfioctl(dev, cmd, &mut data, 0, &PROC0);
    *arg = ioctl_arg(&data);
    r
}

/// Reads the descriptor's buffer (`bufsize` bytes, as bpfread insists), without waiting.
fn read(dev: Dev, bufsize: usize) -> (Result<(), Errno>, Vec<u8>) {
    let mut buf = vec![0u8; bufsize];
    let mut iov = [Iovec::new()];
    iov[0].iov_base = buf.as_mut_ptr().cast();
    iov[0].iov_len = bufsize;
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: bufsize,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let r = bpfread(dev, &mut uio, IO_NDELAY);
    let got = bufsize - uio.uio_resid;
    buf.truncate(got);
    (r, buf)
}

#[test]
fn capture_through_a_descriptor() {
    let _g = setup_net();
    let ifp = test_ifnet(b"bpftest0");
    bpfattach(&ifp.if_bpf, ifp, DLT_EN10MB, ETHER_HDR_LEN as u32);
    assert!(ifp.if_bpf.get().is_null(), "no listener yet");

    let dev = makedev(23, 7 << CLONE_SHIFT);
    assert_eq!(bpfopen(makedev(23, 1), 0, 0, &PROC0), Err(Errno::ENXIO));
    bpfopen(dev, 0, 0, &PROC0).expect("bpfopen");

    let mut name = [0u8; 32];
    name[..8].copy_from_slice(b"bpftest0");
    assert_eq!(bpfioctl(dev, BIOCSETIF, &mut name, 0, &PROC0), Ok(()));
    assert!(
        !ifp.if_bpf.get().is_null(),
        "the listener points the driver at the tap"
    );
    let mut one = 1i32;
    ioctl(dev, BIOCIMMEDIATE, &mut one).expect("BIOCIMMEDIATE");
    let mut blen = 0u32;
    ioctl(dev, BIOCGBLEN, &mut blen).expect("BIOCGBLEN");
    assert_eq!(blen, 32768);
    let mut dlt = 0u32;
    ioctl(dev, BIOCGDLT, &mut dlt).expect("BIOCGDLT");
    assert_eq!(dlt, DLT_EN10MB);

    // A frame comes in: captured behind a bpf_hdr whose length aligns the IP header.
    let frame: Vec<u8> = (0u8..60).collect();
    let m = test_packet(&frame);
    m.m_pkthdr().ph_ifidx.set(3);
    assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
    let mut n = 0i32;
    ioctl(dev, FIONREAD, &mut n).expect("FIONREAD");
    assert_eq!(n, 30 + 60);
    let (r, buf) = read(dev, blen as usize);
    assert_eq!(r, Ok(()));
    let bh: BpfHdr = ioctl_arg(&buf);
    assert_eq!((bh.bh_caplen, bh.bh_datalen, bh.bh_hdrlen), (60, 60, 30));
    assert_eq!(bh.bh_ifidx, 3);
    assert_eq!(bh.bh_flags & BPF_F_DIR_MASK, BPF_F_DIR_IN);
    assert_eq!(&buf[30..90], &frame[..]);
    // Nothing more: a non-blocking read returns nothing, successfully.
    let (r, buf) = read(dev, blen as usize);
    assert_eq!((r, buf.len()), (Ok(()), 0));
    assert_eq!(read(dev, 100).0, Err(Errno::EINVAL));

    // A read filter that keeps 4 bytes, then one that rejects; the counters see both.
    let keep4 = [bpf_stmt(BPF_RET | BPF_K, 4)];
    let mut prog = BpfProgram {
        bf_len: 1,
        _pad0: 0,
        bf_insns: keep4.as_ptr() as usize,
    };
    ioctl(dev, BIOCSETF, &mut prog).expect("BIOCSETF");
    assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
    let (_, buf) = read(dev, blen as usize);
    let bh: BpfHdr = ioctl_arg(&buf);
    assert_eq!((bh.bh_caplen, bh.bh_datalen), (4, 60));
    let reject = [bpf_stmt(BPF_RET | BPF_K, 0)];
    prog.bf_insns = reject.as_ptr() as usize;
    ioctl(dev, BIOCSETFNR, &mut prog).expect("BIOCSETFNR");
    assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
    let mut st = BpfStat::default();
    ioctl(dev, BIOCGSTATS, &mut st).expect("BIOCGSTATS");
    assert_eq!(
        st,
        BpfStat {
            bs_recv: 2,
            bs_drop: 0
        }
    );
    let bad = [bpf_stmt(BPF_LD | BPF_MEM, 99), bpf_stmt(BPF_RET | BPF_K, 0)];
    prog.bf_len = 2;
    prog.bf_insns = bad.as_ptr() as usize;
    assert_eq!(ioctl(dev, BIOCSETF, &mut prog), Err(Errno::EINVAL));

    // Without a filter, a listener with BPF_FILDROP_DROP drops the packet uncaptured.
    let mut none = BpfProgram::default();
    ioctl(dev, BIOCSETF, &mut none).expect("BIOCSETF NULL");
    let mut drop = u32::from(BPF_FILDROP_DROP);
    ioctl(dev, BIOCSFILDROP, &mut drop).expect("BIOCSFILDROP");
    assert!(bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
    // The direction filter skips outgoing packets.
    let mut out = BPF_DIRECTION_OUT;
    ioctl(dev, BIOCSDIRFILT, &mut out).expect("BIOCSDIRFILT");
    assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_OUT));
    m_freem(m);

    // Locked: only the read-only ioctls remain.
    ioctl(dev, BIOCLOCK, &mut one).expect("BIOCLOCK");
    assert_eq!(ioctl(dev, BIOCSFILDROP, &mut drop), Err(Errno::EPERM));
    let mut v = BpfVersion::default();
    ioctl(dev, BIOCVERSION, &mut v).expect("BIOCVERSION");
    assert_eq!((v.bv_major, v.bv_minor), (1, 1));

    bpfclose(dev, 0, 0, None).expect("bpfclose");
    assert!(ifp.if_bpf.get().is_null(), "the last listener is gone");
    assert!(bpfilter_lookup(7 << CLONE_SHIFT).is_none());
    bpfdetach(ifp);
    assert!(
        !BPF_IFLIST
            .iter()
            .any(|bp| bpf_name_eq(&bp.bif_name, b"bpftest0"))
    );
}

#[test]
fn ioctl_numbers() {
    // LP64 values, as OpenBSD/amd64 and arm64 compute them.
    assert_eq!(BIOCGBLEN, 0x4004_4266);
    assert_eq!(BIOCSBLEN, 0xc004_4266);
    assert_eq!(BIOCSETF, 0x8010_4267);
    assert_eq!(BIOCFLUSH, 0x2000_4268);
    assert_eq!(BIOCGETIF, 0x4020_426b);
    assert_eq!(BIOCSRTIMEOUT, 0x8010_426d);
    assert_eq!(BIOCGSTATS, 0x4008_426f);
    assert_eq!(BIOCVERSION, 0x4004_4271);
    assert_eq!(BIOCGDLTLIST, 0xc010_427b);
    assert_eq!(BIOCSETFNR, 0x8010_427f);
}

#[test]
fn header_alignment() {
    assert_eq!(bpf_wordalign(0), 0);
    assert_eq!(bpf_wordalign(1), 4);
    assert_eq!(bpf_wordalign(28), 28);
    // An Ethernet tap: the network header after hdrlen + 14 bytes lands on a longword.
    assert_eq!(bpf_wordalign(14 + SIZEOF_BPF_HDR) - 14, 30);
    assert_eq!(bpf_class(BPF_JMP | BPF_JEQ | BPF_K), BPF_JMP);
    assert_eq!(bpf_mode(BPF_LDX | BPF_B | BPF_MSH), BPF_MSH);
    assert_eq!(bpf_op(BPF_ALU | BPF_XOR | BPF_X), BPF_XOR);
    assert_eq!(bpf_src(BPF_ALU | BPF_XOR | BPF_X), BPF_X);
    assert_eq!(bpf_size(BPF_LD | BPF_H | BPF_ABS), BPF_H);
    assert_eq!(bpf_rval(BPF_RET | BPF_A), BPF_A);
    assert_eq!(bpf_miscop(BPF_MISC | BPF_TXA), BPF_TXA);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/bpf.h");
    let dlt = assert_defines!(defs;
        DLT_NULL, DLT_EN10MB, DLT_EN3MB, DLT_AX25, DLT_PRONET, DLT_CHAOS, DLT_IEEE802,
        DLT_ARCNET, DLT_SLIP, DLT_PPP, DLT_FDDI, DLT_ATM_RFC1483, DLT_LOOP, DLT_ENC, DLT_RAW,
        DLT_SLIP_BSDOS, DLT_PPP_BSDOS, DLT_PFSYNC, DLT_PPP_SERIAL, DLT_PPP_ETHER, DLT_C_HDLC,
        DLT_IEEE802_11, DLT_PFLOG, DLT_IEEE802_11_RADIO, DLT_USER0, DLT_USER1, DLT_USER2,
        DLT_USER3, DLT_USER4, DLT_USER5, DLT_USER6, DLT_USER7, DLT_USER8, DLT_USER9,
        DLT_USER10, DLT_USER11, DLT_USER12, DLT_USER13, DLT_USER14, DLT_USER15, DLT_USBPCAP,
        DLT_MPLS, DLT_OPENFLOW);
    assert_complete(&defs, "DLT_", &dlt);
    assert_defines!(defs;
        BPF_RELEASE, BPF_MAXINSNS, BPF_MAXBUFSIZE, BPF_MINBUFSIZE, BPF_MAJOR_VERSION,
        BPF_MINOR_VERSION, BPF_FILDROP_PASS, BPF_FILDROP_CAPTURE, BPF_FILDROP_DROP,
        BPF_F_PRI_MASK, BPF_F_FLOWID, BPF_F_DIR_SHIFT, BPF_LD, BPF_LDX, BPF_ST, BPF_STX,
        BPF_ALU, BPF_JMP, BPF_RET, BPF_MISC, BPF_W, BPF_H, BPF_B, BPF_IMM, BPF_ABS, BPF_IND,
        BPF_MEM, BPF_LEN, BPF_MSH, BPF_RND, BPF_ADD, BPF_SUB, BPF_MUL, BPF_DIV, BPF_OR,
        BPF_AND, BPF_LSH, BPF_RSH, BPF_NEG, BPF_MOD, BPF_XOR, BPF_JA, BPF_JEQ, BPF_JGT,
        BPF_JGE, BPF_JSET, BPF_K, BPF_X, BPF_A, BPF_TAX, BPF_TXA, BPF_MEMWORDS);
}
