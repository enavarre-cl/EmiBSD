/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! The Internet protocols: OpenBSD `sys/netinet/`.
//!
//! Headers become modules as in `sys/sys` (`ip_icmp.h` → `ip_icmp.rs`). `in.h` is `in_.rs`
//! because `in` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod icmp6;
pub mod icmp_var;
pub mod if_ether;
pub mod igmp;
pub mod igmp_var;
pub mod in4_cksum;
pub mod in_;
pub mod in_cksum;
pub mod in_pcb;
pub mod in_proto;
pub mod in_systm;
pub mod in_var;
pub mod inet_nat64;
pub mod ip;
pub mod ip6;
pub mod ip_ah;
pub mod ip_divert;
pub mod ip_ecn;
pub mod ip_esp;
pub mod ip_icmp;
pub mod ip_id;
pub mod ip_input;
pub mod ip_ipcomp;
pub mod ip_ipip;
pub mod ip_ipsp;
pub mod ip_output;
pub mod ip_spd;
pub mod ip_var;
pub mod ipsec_input;
pub mod ipsec_output;
pub mod raw_ip;
pub mod tcp;
pub mod tcp_debug;
pub mod tcp_fsm;
pub mod tcp_input;
pub mod tcp_output;
pub mod tcp_seq;
pub mod tcp_subr;
pub mod tcp_timer;
pub mod tcp_usrreq;
pub mod tcp_var;
pub mod udp;
pub mod udp_usrreq;
pub mod udp_var;
/* </CODE> */
