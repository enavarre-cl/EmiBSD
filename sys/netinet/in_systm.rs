/*	$OpenBSD: in_systm.h,v 1.7 2014/07/13 13:57:56 mpi Exp $	*/
/*	$NetBSD: in_systm.h,v 1.8 1995/04/13 06:29:22 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)in_systm.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Miscellaneous internetwork definitions for the kernel: `<netinet/in_systm.h>`.
//!
//! Upstream: sys/netinet/in_systm.h @ 3ce1f3f79392
//!
//! Network types: values of these types are in network byte order.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - The typedefs drop C's lowercase spelling for CamelCase (`n_short` → [`NShort`]), as the
//!   `_t` typedefs of `<sys/types.h>` do.
//! - `iptime()` comes with `netinet/ip_icmp.c`, which defines it.

/// `n_short`: a 16-bit value in network byte order.
pub type NShort = u16;
/// `n_long`: a 32-bit value in network byte order.
pub type NLong = u32;
/// `n_time`: ms since 00:00 GMT, in network byte order.
pub type NTime = u32;
