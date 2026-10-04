/*	$OpenBSD: tcp_subr.c,v 1.216 2025/07/18 08:39:14 mvs Exp $	*/
/*	$NetBSD: tcp_subr.c,v 1.22 1996/02/13 23:44:00 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1990, 1993
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
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */
/* </LICENSES> */

//! TCP support routines: `netinet/tcp_subr.c`.
//!
//! Upstream: sys/netinet/tcp_subr.c @ 3ce1f3f79392
//!
//! Status: `wip`. This commit carries the file's globals, which the headers' inline
//! functions (`tcpstat_inc`, `tcp_now`) read; the functions follow.
//!
//! Locks used to protect struct members in this file: \[I\] immutable after creation,
//! \[T\] `tcp_timer_mtx` (global tcp timer data structures), \[a\] atomic.
//!
//! ## Deviations
//! - The sysctl-controlled `int`s are `AtomicI32`s (the C reads them with
//!   `atomic_load_int`); `tcpcounters` (`struct cpumem *`) is a static array of atomics;
//!   `tcp_iss` is an `AtomicU32` changed under `tcp_timer_mtx` as in C. `tcp_secret` and
//!   `tcp_secret_ctx` are written once by `tcp_init` (`StaticCell`).

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64};

use libkern::StaticCell;

use crate::crypto::sha2::Sha2Ctx;
use crate::machine::intr::IPL_SOFTNET;
use crate::netinet::tcp::TCP_MSS;
use crate::netinet::tcp_timer::TCPTV_SRTTDFLT;
use crate::netinet::tcp_var::TCPS_NCOUNTERS;
use crate::sys::mutex::Mutex;
use crate::sys::param::NMBCLUSTERS;
use crate::sys::pool::Pool;

/// `tcp_timer_mtx`: \[T\] global tcp timer data structures.
pub static TCP_TIMER_MTX: Mutex = Mutex::new(IPL_SOFTNET);

// patchable/settable parameters for tcp

/// \[a\] `tcp_mssdflt`: default maximum segment size.
pub static TCP_MSSDFLT: AtomicI32 = AtomicI32::new(TCP_MSS);
/// `tcp_rttdflt`.
pub static TCP_RTTDFLT: AtomicI32 = AtomicI32::new(TCPTV_SRTTDFLT);

// values controllable via sysctl

/// \[a\] `tcp_do_rfc1323`.
pub static TCP_DO_RFC1323: AtomicI32 = AtomicI32::new(1);
/// \[a\] `tcp_do_sack`: RFC 2018 selective ACKs.
pub static TCP_DO_SACK: AtomicI32 = AtomicI32::new(1);
/// \[a\] `tcp_ack_on_push`: set to enable immediate ACK-on-PUSH.
pub static TCP_ACK_ON_PUSH: AtomicI32 = AtomicI32::new(0);
/// \[a\] `tcp_do_ecn`: RFC3168 ECN enabled/disabled? (`TCP_ECN`).
pub static TCP_DO_ECN: AtomicI32 = AtomicI32::new(0);
/// \[a\] `tcp_do_rfc3390`: increase TCP's Initial Window to 10*mss.
pub static TCP_DO_RFC3390: AtomicI32 = AtomicI32::new(2);
/// \[a\] `tcp_do_tso`: TCP segmentation offload for output.
pub static TCP_DO_TSO: AtomicI32 = AtomicI32::new(1);

/// `TCB_INITIAL_HASH_SIZE`.
const TCB_INITIAL_HASH_SIZE: i32 = 128;

/// `tcp_reass_limit`: hardlimit for `tcpqe_pool`.
pub static TCP_REASS_LIMIT: AtomicI32 = AtomicI32::new((NMBCLUSTERS / 8) as i32);
/// `tcp_sackhole_limit`: hardlimit for `sackhl_pool`.
pub static TCP_SACKHOLE_LIMIT: AtomicI32 = AtomicI32::new(32 * 1024);

/// `tcpcb_pool`.
pub static TCPCB_POOL: Pool = Pool::new();
/// `tcpqe_pool`.
pub static TCPQE_POOL: Pool = Pool::new();
/// `sackhl_pool`.
pub static SACKHL_POOL: Pool = Pool::new();

/// `tcpcounters`: tcp statistics.
pub static TCPCOUNTERS: [AtomicU64; TCPS_NCOUNTERS] = [const { AtomicU64::new(0) }; TCPS_NCOUNTERS];

/// \[I\] `tcp_secret`.
static TCP_SECRET: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// \[I\] `tcp_secret_ctx`.
static TCP_SECRET_CTX: StaticCell<Option<Sha2Ctx>> = StaticCell::new(None);
/// \[T\] `tcp_iss`: updated by timer and connection.
pub static TCP_ISS: AtomicU32 = AtomicU32::new(0);
/// \[I\] `tcp_starttime`: random offset for `tcp_now()`.
pub static TCP_STARTTIME: AtomicU64 = AtomicU64::new(0);
