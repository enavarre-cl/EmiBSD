use super::*;
use crate::reftest::{assert_complete, assert_defines};

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
