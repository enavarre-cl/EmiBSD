use super::*;
use crate::net::bpf::{BPF_MAXBUFSIZE, bpf_jump, bpf_stmt};
use std::vec;
use std::vec::Vec;

/// `bpf_mem_ldw`: userland's load over a linear buffer, for the tests.
fn mem_ldw(p: &[u8], k: u32) -> Option<u32> {
    let b = p.get(k as usize..)?.get(..4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// `bpf_mem_ldh`.
fn mem_ldh(p: &[u8], k: u32) -> Option<u32> {
    let b = p.get(k as usize..)?.get(..2)?;
    Some(u32::from(u16::from_be_bytes([b[0], b[1]])))
}

/// `bpf_mem_ldb`.
fn mem_ldb(p: &[u8], k: u32) -> Option<u32> {
    p.get(k as usize).map(|&b| u32::from(b))
}

/// `bpf_mem_ops`.
static MEM_OPS: BpfOps<[u8]> = BpfOps {
    ldw: mem_ldw,
    ldh: mem_ldh,
    ldb: mem_ldb,
};

/// `bpf_filter(pc, pkt, wirelen, buflen)` with the whole buffer captured.
fn run(prog: &[BpfInsn], pkt: &[u8]) -> u32 {
    _bpf_lfilter(Some(prog), &MEM_OPS, pkt, pkt.len() as u32)
}

/// `tcpdump -d 'ip and udp and dst port 53'` on Ethernet.
const UDP_DNS: [BpfInsn; 11] = [
    bpf_stmt(BPF_LD | BPF_H | BPF_ABS, 12),
    bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0x800, 0, 8),
    bpf_stmt(BPF_LD | BPF_B | BPF_ABS, 23),
    bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 17, 0, 6),
    bpf_stmt(BPF_LD | BPF_H | BPF_ABS, 20),
    bpf_jump(BPF_JMP | BPF_JSET | BPF_K, 0x1fff, 4, 0),
    bpf_stmt(BPF_LDX | BPF_B | BPF_MSH, 14),
    bpf_stmt(BPF_LD | BPF_H | BPF_IND, 16),
    bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 53, 0, 1),
    bpf_stmt(BPF_RET | BPF_K, 262144),
    bpf_stmt(BPF_RET | BPF_K, 0),
];

/// An Ethernet frame carrying IPv4 (IHL 5 + `opts` words of options) and UDP to `dport`.
fn udp_frame(dport: u16, opts: usize, frag: u16) -> Vec<u8> {
    let mut f = vec![0u8; 14];
    f[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let mut ip = vec![0u8; 20 + 4 * opts];
    ip[0] = 0x45 + opts as u8;
    ip[6..8].copy_from_slice(&frag.to_be_bytes());
    ip[9] = 17;
    f.extend_from_slice(&ip);
    let mut udp = [0u8; 8];
    udp[0..2].copy_from_slice(&1234u16.to_be_bytes());
    udp[2..4].copy_from_slice(&dport.to_be_bytes());
    f.extend_from_slice(&udp);
    f
}

#[test]
fn canned_program_matches_dns() {
    assert_eq!(run(&UDP_DNS, &udp_frame(53, 0, 0)), 262144);
    assert_eq!(
        run(&UDP_DNS, &udp_frame(53, 2, 0)),
        262144,
        "ldxb 4*([14]&0xf)"
    );
    assert_eq!(run(&UDP_DNS, &udp_frame(80, 0, 0)), 0);
    assert_eq!(
        run(&UDP_DNS, &udp_frame(53, 0, 0x0005)),
        0,
        "a later fragment"
    );
    // ARP.
    let mut arp = udp_frame(53, 0, 0);
    arp[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
    assert_eq!(run(&UDP_DNS, &arp), 0);
    // A truncated capture: the load past the end rejects.
    assert_eq!(run(&UDP_DNS, &udp_frame(53, 0, 0)[..36]), 0);
    assert!(bpf_validate(&UDP_DNS));
}

#[test]
fn no_program_accepts_all() {
    assert_eq!(_bpf_lfilter(None, &MEM_OPS, &[][..], 0), u32::MAX);
    assert_eq!(run(&[], &[1, 2, 3]), 0);
}

#[test]
fn alu_scratch_and_len() {
    let prog = [
        bpf_stmt(BPF_LD | BPF_W | BPF_LEN, 0),
        bpf_stmt(BPF_ALU | BPF_MUL | BPF_K, 3),
        bpf_stmt(BPF_ST, 5),
        bpf_stmt(BPF_LDX | BPF_IMM, 4),
        bpf_stmt(BPF_LD | BPF_MEM, 5),
        bpf_stmt(BPF_ALU | BPF_SUB | BPF_X, 0),
        bpf_stmt(BPF_ALU | BPF_RSH | BPF_K, 1),
        bpf_stmt(BPF_RET | BPF_A, 0),
    ];
    // (10 * 3 - 4) >> 1
    assert_eq!(run(&prog, &[0; 10]), 13);
    assert!(bpf_validate(&prog));

    let neg = [
        bpf_stmt(BPF_LD | BPF_IMM, 1),
        bpf_stmt(BPF_ALU | BPF_NEG, 0),
        bpf_stmt(BPF_MISC | BPF_TAX, 0),
        bpf_stmt(BPF_LD | BPF_IMM, 7),
        bpf_stmt(BPF_MISC | BPF_TXA, 0),
        bpf_stmt(BPF_RET | BPF_A, 0),
    ];
    assert_eq!(run(&neg, &[]), u32::MAX);

    // A variable shift of 32 or more yields 0; a variable division by 0 rejects.
    let shift = [
        bpf_stmt(BPF_LD | BPF_IMM, 0xffff),
        bpf_stmt(BPF_LDX | BPF_IMM, 40),
        bpf_stmt(BPF_ALU | BPF_LSH | BPF_X, 0),
        bpf_stmt(BPF_ALU | BPF_ADD | BPF_K, 9),
        bpf_stmt(BPF_RET | BPF_A, 0),
    ];
    assert_eq!(run(&shift, &[]), 9);
    let div0 = [
        bpf_stmt(BPF_LD | BPF_IMM, 10),
        bpf_stmt(BPF_ALU | BPF_DIV | BPF_X, 0),
        bpf_stmt(BPF_RET | BPF_K, 1),
    ];
    assert_eq!(run(&div0, &[]), 0);
    let modk = [
        bpf_stmt(BPF_LD | BPF_IMM, 10),
        bpf_stmt(BPF_ALU | BPF_MOD | BPF_K, 4),
        bpf_stmt(BPF_RET | BPF_A, 0),
    ];
    assert_eq!(run(&modk, &[]), 2);
}

#[test]
fn walk_ends_off_the_program() {
    // An unchecked program whose jump leaves it rejects, as does falling off the end.
    let out = [
        bpf_jump(BPF_JMP | BPF_JA, 5, 0, 0),
        bpf_stmt(BPF_RET | BPF_K, 1),
    ];
    assert_eq!(run(&out, &[]), 0);
    assert!(!bpf_validate(&out));
    let fall = [bpf_stmt(BPF_LD | BPF_IMM, 1)];
    assert_eq!(run(&fall, &[]), 0);
    assert!(!bpf_validate(&fall));
    // An undefined opcode rejects.
    let bad = [bpf_stmt(0xff, 0), bpf_stmt(BPF_RET | BPF_K, 1)];
    assert_eq!(run(&bad, &[]), 0);
    assert!(!bpf_validate(&bad));
    // Scratch memory out of range rejects at run time and in validation.
    let mem = [bpf_stmt(BPF_LD | BPF_MEM, 16), bpf_stmt(BPF_RET | BPF_K, 1)];
    assert_eq!(run(&mem, &[]), 0);
    assert!(!bpf_validate(&mem));
}

#[test]
fn validate_rejects() {
    let ret = bpf_stmt(BPF_RET | BPF_K, 0);
    assert!(!bpf_validate(&[]));
    assert!(bpf_validate(&[ret]));
    assert!(bpf_validate(&[ret; BPF_MAXINSNS as usize]));
    assert!(!bpf_validate(&[ret; BPF_MAXINSNS as usize + 1]));
    // Constant shifts of 32 and divisions by zero.
    assert!(!bpf_validate(&[
        bpf_stmt(BPF_ALU | BPF_LSH | BPF_K, 32),
        ret
    ]));
    assert!(bpf_validate(&[
        bpf_stmt(BPF_ALU | BPF_LSH | BPF_K, 31),
        ret
    ]));
    assert!(!bpf_validate(&[
        bpf_stmt(BPF_ALU | BPF_DIV | BPF_K, 0),
        ret
    ]));
    assert!(!bpf_validate(&[
        bpf_stmt(BPF_ALU | BPF_MOD | BPF_K, 0),
        ret
    ]));
    assert!(bpf_validate(&[bpf_stmt(BPF_ALU | BPF_DIV | BPF_X, 0), ret]));
    // Conditional jumps stay inside, and the last instruction returns.
    assert!(!bpf_validate(&[
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 1, 0),
        ret
    ]));
    assert!(bpf_validate(&[
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_X, 0, 0, 0),
        ret
    ]));
    assert!(!bpf_validate(&[
        bpf_jump(BPF_JMP | BPF_JA, u32::MAX, 0, 0),
        ret
    ]));
    // Packet offsets below bpf_maxbufsize.
    let abs = BPF_MAXBUFSIZE as u32;
    assert!(!bpf_validate(&[
        bpf_stmt(BPF_LD | BPF_B | BPF_ABS, abs),
        ret
    ]));
    assert!(bpf_validate(&[
        bpf_stmt(BPF_LD | BPF_B | BPF_ABS, abs - 1),
        ret
    ]));
    assert!(!bpf_validate(&[bpf_stmt(BPF_STX, 16), ret]));
}
