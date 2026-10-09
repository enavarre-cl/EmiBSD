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
//! IPv6: OpenBSD `sys/netinet6/`.
//!
//! Headers become modules as in `sys/sys` (`in6_var.h` → `in6_var.rs`); a `.c` file with a
//! header of the same name shares its module (`in6.h` and `in6.c` → `in6.rs`). The C
//! includes `<netinet6/in6.h>` from `<netinet/in.h>` unconditionally, so this tree compiles
//! whether or not the `inet6` feature (OpenBSD's `option INET6`) is on; the feature gates the
//! `#ifdef INET6` sites outside it and the `inet6domain` registration. `ip6_mroute.c`
//! (`MROUTING`) is not configured.

pub mod dest6;
pub mod frag6;
pub mod icmp6;
pub mod in6;
pub mod in6_cksum;
pub mod in6_ifattach;
pub mod in6_pcb;
pub mod in6_proto;
pub mod in6_src;
pub mod in6_var;
pub mod ip6_divert;
pub mod ip6_forward;
pub mod ip6_id;
pub mod ip6_input;
pub mod ip6_output;
pub mod ip6_var;
pub mod ip6protosw;
pub mod mld6;
pub mod mld6_var;
pub mod nd6;
pub mod nd6_nbr;
pub mod nd6_rtr;
pub mod raw_ip6;
pub mod route6;
pub mod udp6_output;
/* </CODE> */
