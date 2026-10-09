/* <LICENSES> */
/*
 * Copyright (c) 1990, 1991, 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from the Stanford/CMU enet packet filter,
 * (net/enet.c) distributed as part of 4.3BSD, and code contributed
 * to Berkeley by Steven McCanne and Van Jacobson both of Lawrence
 * Berkeley Laboratory.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)bpf_filter.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! The filter machine of `bpf(4)`: `net/bpf_filter.c`, the interpreter `_bpf_lfilter` that
//! runs a program over a packet, and `bpf_validate`, the check a program passes before the
//! kernel accepts it (`BIOCSETF`).
//!
//! Upstream: sys/net/bpf_filter.c @ 3ce1f3f79392
//!
//! The kernel compiles only `_bpf_lfilter` and `bpf_validate`; the packet is read through a
//! [`BpfOps`] table (`bpf.c`'s `bpf_mbuf_ops` for mbuf chains).
//!
//! ## Deviations
//! - The program is a slice (`pc` and `pc_len` in one), `None` for the C's NULL program
//!   (accept all). A NULL-terminated walk (`pc_len` 0 in the C, used only by userland's
//!   `bpf_filter`) does not exist: an empty slice runs no instruction and rejects.
//! - The instruction pointer is an index; a jump that leaves the program ends the walk as
//!   the C's `pc < pcend` test does. The C's `from` counter (which a jump does not advance)
//!   is kept, with its checks.
//! - `bpf_validate` takes the program as a slice and answers `bool` (the C's `int` 0/1).
//! - The userland half (`bpf_mem_*`, `bpf_filter`, `bpf_lfilter`, `_bpf_filter`, compiled
//!   without `_KERNEL` for libpcap) is not ported; the host tests read packets through a
//!   slice-backed `BpfOps` of their own.

use core::sync::atomic::Ordering;

use crate::dev::rnd::arc4random;
use crate::net::bpf::{
    BPF_A, BPF_ABS, BPF_ADD, BPF_ALU, BPF_AND, BPF_B, BPF_DIV, BPF_H, BPF_IMM, BPF_IND, BPF_JA,
    BPF_JEQ, BPF_JGE, BPF_JGT, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_LDX, BPF_LEN, BPF_LSH,
    BPF_MAXINSNS, BPF_MEM, BPF_MEMWORDS, BPF_MISC, BPF_MOD, BPF_MSH, BPF_MUL, BPF_NEG, BPF_OR,
    BPF_RET, BPF_RND, BPF_RSH, BPF_ST, BPF_STX, BPF_SUB, BPF_TAX, BPF_TXA, BPF_W, BPF_X, BPF_XOR,
    BpfInsn, BpfOps, bpf_class, bpf_maxbufsize, bpf_mode, bpf_op, bpf_src,
};

/// `_bpf_lfilter`: executes the filter program `pc` on the packet `pkt`, read through `ops`.
/// `wirelen` is the length of the original packet. Returns how many bytes of the packet to
/// keep: 0 rejects it, `u32::MAX` (the C's `(u_int)-1`) keeps all of it.
pub fn _bpf_lfilter<P: ?Sized>(
    pc: Option<&[BpfInsn]>,
    ops: &BpfOps<P>,
    pkt: &P,
    wirelen: u32,
) -> u32 {
    let Some(prog) = pc else {
        // No filter means accept all.
        return u32::MAX;
    };
    let pc_len = prog.len() as u32;

    if pc_len > BPF_MAXINSNS {
        return 0;
    }

    let mut a: u32 = 0;
    let mut x: u32 = 0;
    let mut from: u32 = 1;
    let mut mem = [0u32; BPF_MEMWORDS];

    // A conditional jump: `k` instructions forward, rejected when it leaves the program.
    let jump = |from: u32, k: u32| from.wrapping_add(k) < pc_len;

    let mut i = 0usize;
    while let Some(insn) = prog.get(i) {
        let k = insn.k;
        match insn.code {
            c if c == BPF_RET | BPF_K => return k,

            c if c == BPF_RET | BPF_A => return a,

            c if c == BPF_LD | BPF_W | BPF_ABS => match (ops.ldw)(pkt, k) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LD | BPF_H | BPF_ABS => match (ops.ldh)(pkt, k) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LD | BPF_B | BPF_ABS => match (ops.ldb)(pkt, k) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LD | BPF_W | BPF_IND => match (ops.ldw)(pkt, x.wrapping_add(k)) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LD | BPF_H | BPF_IND => match (ops.ldh)(pkt, x.wrapping_add(k)) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LD | BPF_B | BPF_IND => match (ops.ldb)(pkt, x.wrapping_add(k)) {
                Some(v) => a = v,
                None => return 0,
            },

            c if c == BPF_LDX | BPF_B | BPF_MSH => match (ops.ldb)(pkt, k) {
                Some(v) => x = (v & 0xf) << 2,
                None => return 0,
            },

            c if c == BPF_LD | BPF_W | BPF_LEN => a = wirelen,

            c if c == BPF_LDX | BPF_W | BPF_LEN => x = wirelen,

            c if c == BPF_LD | BPF_W | BPF_RND => a = arc4random(),

            c if c == BPF_LD | BPF_IMM => a = k,

            c if c == BPF_LDX | BPF_IMM => x = k,

            c if c == BPF_LD | BPF_MEM => match mem.get(k as usize) {
                Some(&v) => a = v,
                None => return 0,
            },

            c if c == BPF_LDX | BPF_MEM => match mem.get(k as usize) {
                Some(&v) => x = v,
                None => return 0,
            },

            BPF_ST => match mem.get_mut(k as usize) {
                Some(slot) => *slot = a,
                None => return 0,
            },

            BPF_STX => match mem.get_mut(k as usize) {
                Some(slot) => *slot = x,
                None => return 0,
            },

            c if c == BPF_JMP | BPF_JA => {
                if from.checked_add(k).is_none() || !jump(from, k) {
                    return 0;
                }
                i += k as usize;
            }

            c if bpf_class(c) == BPF_JMP && is_cond_jump(c) => {
                let operand = if bpf_src(c) == BPF_X { x } else { k };
                let taken = match bpf_op(c) {
                    BPF_JGT => a > operand,
                    BPF_JGE => a >= operand,
                    BPF_JEQ => a == operand,
                    _ => a & operand != 0,
                };
                let off = u32::from(if taken { insn.jt } else { insn.jf });
                if !jump(from, off) {
                    return 0;
                }
                i += off as usize;
            }

            c if c == BPF_ALU | BPF_ADD | BPF_X => a = a.wrapping_add(x),

            c if c == BPF_ALU | BPF_SUB | BPF_X => a = a.wrapping_sub(x),

            c if c == BPF_ALU | BPF_MUL | BPF_X => a = a.wrapping_mul(x),

            c if c == BPF_ALU | BPF_DIV | BPF_X => {
                if x == 0 {
                    return 0;
                }
                a /= x;
            }

            c if c == BPF_ALU | BPF_MOD | BPF_X => {
                if x == 0 {
                    return 0;
                }
                a %= x;
            }

            c if c == BPF_ALU | BPF_AND | BPF_X => a &= x,

            c if c == BPF_ALU | BPF_OR | BPF_X => a |= x,

            c if c == BPF_ALU | BPF_XOR | BPF_X => a ^= x,

            c if c == BPF_ALU | BPF_LSH | BPF_X => a = a.checked_shl(x).unwrap_or(0),

            c if c == BPF_ALU | BPF_RSH | BPF_X => a = a.checked_shr(x).unwrap_or(0),

            c if c == BPF_ALU | BPF_ADD | BPF_K => a = a.wrapping_add(k),

            c if c == BPF_ALU | BPF_SUB | BPF_K => a = a.wrapping_sub(k),

            c if c == BPF_ALU | BPF_MUL | BPF_K => a = a.wrapping_mul(k),

            c if c == BPF_ALU | BPF_DIV | BPF_K => {
                if k == 0 {
                    return 0;
                }
                a /= k;
            }

            c if c == BPF_ALU | BPF_MOD | BPF_K => {
                if k == 0 {
                    return 0;
                }
                a %= k;
            }

            c if c == BPF_ALU | BPF_AND | BPF_K => a &= k,

            c if c == BPF_ALU | BPF_OR | BPF_K => a |= k,

            c if c == BPF_ALU | BPF_XOR | BPF_K => a ^= k,

            c if c == BPF_ALU | BPF_LSH | BPF_K => a = a.checked_shl(k).unwrap_or(0),

            c if c == BPF_ALU | BPF_RSH | BPF_K => a = a.checked_shr(k).unwrap_or(0),

            c if c == BPF_ALU | BPF_NEG => a = a.wrapping_neg(),

            c if c == BPF_MISC | BPF_TAX => x = a,

            c if c == BPF_MISC | BPF_TXA => a = x,

            _ => return 0,
        }
        i += 1;
        from = from.wrapping_add(1);
    }
    0
}

/// The eight conditional jumps: `BPF_JGT`, `BPF_JGE`, `BPF_JEQ` and `BPF_JSET`, each with a
/// `BPF_K` or a `BPF_X` operand (the C's eight `case` labels).
const fn is_cond_jump(code: u16) -> bool {
    let k_or_x = code & !BPF_X;
    k_or_x == BPF_JMP | BPF_JGT
        || k_or_x == BPF_JMP | BPF_JGE
        || k_or_x == BPF_JMP | BPF_JEQ
        || k_or_x == BPF_JMP | BPF_JSET
}

/// `bpf_validate`: whether `f` is a valid filter program. The constraints are that each jump
/// be forward and to a valid code and memory operations use valid addresses. The code must
/// terminate with either an accept or reject.
///
/// The kernel needs to be able to verify an application's filter code. Otherwise, a bogus
/// program could easily crash the system.
pub fn bpf_validate(f: &[BpfInsn]) -> bool {
    let len = f.len() as u32;

    if !(1..=BPF_MAXINSNS).contains(&len) {
        return false;
    }

    let maxbufsize = bpf_maxbufsize.load(Ordering::Relaxed) as u32;
    for (i, p) in f.iter().enumerate() {
        let code = p.code;
        match bpf_class(code) {
            BPF_RET if code == BPF_RET | BPF_K || code == BPF_RET | BPF_A => {}
            // Check that memory operations use valid addresses.
            BPF_LD | BPF_LDX if is_load(code) => match bpf_mode(code) {
                BPF_IMM => {}
                BPF_ABS | BPF_IND | BPF_MSH => {
                    // More strict check with actual packet length is done runtime.
                    if p.k >= maxbufsize {
                        return false;
                    }
                }
                BPF_MEM => {
                    if p.k as usize >= BPF_MEMWORDS {
                        return false;
                    }
                }
                BPF_LEN | BPF_RND => {}
                _ => return false,
            },
            BPF_ST | BPF_STX if code == BPF_ST || code == BPF_STX => {
                if p.k as usize >= BPF_MEMWORDS {
                    return false;
                }
            }
            BPF_JMP if code == BPF_JMP | BPF_JA || is_cond_jump(code) => {
                // Check that jumps are forward, and within the code block.
                let from = i as u32 + 1;
                match bpf_op(code) {
                    BPF_JA => match from.checked_add(p.k) {
                        Some(to) if to < len => {}
                        _ => return false,
                    },
                    BPF_JEQ | BPF_JGT | BPF_JGE | BPF_JSET => {
                        if from + u32::from(p.jt) >= len || from + u32::from(p.jf) >= len {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            BPF_ALU if is_alu(code) => match bpf_op(code) {
                BPF_ADD | BPF_SUB | BPF_MUL | BPF_OR | BPF_XOR | BPF_AND | BPF_NEG => {}
                BPF_LSH | BPF_RSH => {
                    // Check constant shifts are less than 32 bits.
                    if bpf_src(code) == BPF_K && p.k > 31 {
                        return false;
                    }
                }
                BPF_DIV | BPF_MOD => {
                    // Check for constant division by 0.
                    if bpf_src(code) == BPF_K && p.k == 0 {
                        return false;
                    }
                }
                _ => return false,
            },
            BPF_MISC if code == BPF_MISC | BPF_TAX || code == BPF_MISC | BPF_TXA => {}
            _ => return false,
        }
    }
    f.last().is_some_and(|last| bpf_class(last.code) == BPF_RET)
}

/// The fourteen load instructions `bpf_validate` accepts (the C's `case` labels).
fn is_load(code: u16) -> bool {
    [
        BPF_LD | BPF_W | BPF_ABS,
        BPF_LD | BPF_H | BPF_ABS,
        BPF_LD | BPF_B | BPF_ABS,
        BPF_LD | BPF_W | BPF_IND,
        BPF_LD | BPF_H | BPF_IND,
        BPF_LD | BPF_B | BPF_IND,
        BPF_LDX | BPF_B | BPF_MSH,
        BPF_LD | BPF_W | BPF_LEN,
        BPF_LDX | BPF_W | BPF_LEN,
        BPF_LD | BPF_W | BPF_RND,
        BPF_LD | BPF_IMM,
        BPF_LDX | BPF_IMM,
        BPF_LD | BPF_MEM,
        BPF_LDX | BPF_MEM,
    ]
    .contains(&code)
}

/// The twenty-one ALU instructions `bpf_validate` accepts (the C's `case` labels): the ten
/// binary operations with a `BPF_X` or a `BPF_K` operand, and `BPF_NEG`.
const fn is_alu(code: u16) -> bool {
    if code == BPF_ALU | BPF_NEG {
        return true;
    }
    let op = code & !BPF_X;
    op == BPF_ALU | BPF_ADD
        || op == BPF_ALU | BPF_SUB
        || op == BPF_ALU | BPF_MUL
        || op == BPF_ALU | BPF_DIV
        || op == BPF_ALU | BPF_MOD
        || op == BPF_ALU | BPF_AND
        || op == BPF_ALU | BPF_OR
        || op == BPF_ALU | BPF_XOR
        || op == BPF_ALU | BPF_LSH
        || op == BPF_ALU | BPF_RSH
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
