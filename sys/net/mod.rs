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
//! Network interfaces, routing and the link layer: OpenBSD `sys/net/`.
//!
//! Headers become modules as in `sys/sys` (`if_types.h` → `if_types.rs`), and a `.c` file
//! is the module of its name (`ifq.c` → `ifq.rs`, with `ifq.h`). `if.h` and `if.c` are
//! `if_.rs` because `if` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod art;
pub mod bpf;
pub mod bpf_filter;
pub mod bpfdesc;
pub mod ethertypes;
pub mod fq_codel;
pub mod hfsc;
pub mod if_;
pub mod if_arp;
pub mod if_dl;
pub mod if_enc;
pub mod if_ethersubr;
pub mod if_loop;
pub mod if_media;
pub mod if_pflog;
pub mod if_pflow;
pub mod if_pfsync;
pub mod if_types;
pub mod if_var;
pub mod if_wg;
pub mod ifq;
pub mod netisr;
pub mod pf;
pub mod pf_if;
pub mod pf_ioctl;
pub mod pf_lb;
pub mod pf_norm;
pub mod pf_osfp;
pub mod pf_ruleset;
pub mod pf_syncookies;
pub mod pf_table;
pub mod pfkeyv2;
pub mod pfkeyv2_convert;
pub mod pfkeyv2_parsemessage;
pub mod pfvar;
pub mod pfvar_priv;
pub mod radix;
pub mod route;
pub mod rtable;
pub mod rtsock;
pub mod toeplitz;
pub mod wg_cookie;
pub mod wg_noise;
/* </CODE> */
