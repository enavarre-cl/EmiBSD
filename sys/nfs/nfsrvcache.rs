/*	$OpenBSD: nfsrvcache.h,v 1.8 2013/11/26 20:41:27 beck Exp $	*/
/*	$NetBSD: nfsrvcache.h,v 1.10 1996/02/18 11:54:08 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)nfsrvcache.h	8.3 (Berkeley) 3/30/95
 */
/* </LICENSES> */

//! `<nfs/nfsrvcache.h>`: definitions for the server recent request cache.
//!
//! Upstream: sys/nfs/nfsrvcache.h @ 3ce1f3f79392
//!
//! Cache entries are `malloc(9)` items of `nfs_srvcache.c` that live on its LRU list and
//! hash chains, handed around as `&'static NfsrvCache` with `Cell` members.
//!
//! ## Deviations
//! - The unions `rc_un` and `rc_haddr` are their members side by side: `rc_reply` and
//!   `rc_status` (selected by `RC_REPMBUF`/`RC_REPSTATUS`), and a [`Nethostaddr`]
//!   (`RC_INETADDR`/`RC_NAM`); the C's accessor macros `rc_inetaddr`/`rc_nam` are methods.

use core::cell::Cell;

use crate::nfs::nfs::Nethostaddr;
use crate::queue_adapter;
use crate::sys::mbuf::Mbuf;
use crate::sys::queue::{ListEntry, TailqEntry};

/// `NFSRVCACHESIZ`.
pub const NFSRVCACHESIZ: usize = 2048;

/// `RC_UNUSED`: cache entry states.
pub const RC_UNUSED: u8 = 0;
/// `RC_INPROG`.
pub const RC_INPROG: u8 = 1;
/// `RC_DONE`.
pub const RC_DONE: u8 = 2;

/// `RC_DROPIT`: return values.
pub const RC_DROPIT: i32 = 0;
/// `RC_REPLY`.
pub const RC_REPLY: i32 = 1;
/// `RC_DOIT`.
pub const RC_DOIT: i32 = 2;
/// `RC_CHECKIT`.
pub const RC_CHECKIT: i32 = 3;

/// `RC_LOCKED`: flag bits.
pub const RC_LOCKED: u8 = 0x01;
/// `RC_WANTED`.
pub const RC_WANTED: u8 = 0x02;
/// `RC_REPSTATUS`.
pub const RC_REPSTATUS: u8 = 0x04;
/// `RC_REPMBUF`.
pub const RC_REPMBUF: u8 = 0x08;
/// `RC_INETADDR`.
pub const RC_INETADDR: u8 = 0x20;
/// `RC_NAM`.
pub const RC_NAM: u8 = 0x40;

/// `struct nfsrvcache`: one recent request.
pub struct NfsrvCache {
    /// `rc_lru`: LRU chain.
    pub rc_lru: TailqEntry<NfsrvCache>,
    /// `rc_hash`: hash chain.
    pub rc_hash: ListEntry<NfsrvCache>,
    /// `rc_xid`: rpc id number (a raw XDR word).
    pub rc_xid: Cell<u32>,
    /// `rc_reply` (`rc_un.ru_repmb`): reply mbuf list (`RC_REPMBUF`).
    pub rc_reply: Cell<Option<&'static Mbuf>>,
    /// `rc_status` (`rc_un.ru_repstat`): reply status (`RC_REPSTATUS`).
    pub rc_status: Cell<i32>,
    /// `rc_haddr`: host address.
    pub rc_haddr: Cell<Nethostaddr>,
    /// `rc_proc`: rpc proc number.
    pub rc_proc: Cell<u16>,
    /// `rc_state`: current state of request (`RC_UNUSED`, ...).
    pub rc_state: Cell<u8>,
    /// `rc_flag`: flag bits.
    pub rc_flag: Cell<u8>,
}

// SAFETY: the cache is changed under the kernel lock, as in C; the kernel runs one CPU.
unsafe impl Sync for NfsrvCache {}

impl NfsrvCache {
    /// An entry with every member cleared (`malloc(M_ZERO)`).
    pub const fn new() -> Self {
        Self {
            rc_lru: TailqEntry::new(),
            rc_hash: ListEntry::new(),
            rc_xid: Cell::new(0),
            rc_reply: Cell::new(None),
            rc_status: Cell::new(0),
            rc_haddr: Cell::new(Nethostaddr {
                had_inetaddr: 0,
                had_nam: None,
            }),
            rc_proc: Cell::new(0),
            rc_state: Cell::new(RC_UNUSED),
            rc_flag: Cell::new(0),
        }
    }

    /// `rc_inetaddr` (`rc_haddr.had_inetaddr`).
    pub fn rc_inetaddr(&self) -> u32 {
        self.rc_haddr.get().had_inetaddr
    }

    /// Stores `rc_inetaddr`.
    pub fn set_rc_inetaddr(&self, a: u32) {
        let mut h = self.rc_haddr.get();
        h.had_inetaddr = a;
        self.rc_haddr.set(h);
    }

    /// `rc_nam` (`rc_haddr.had_nam`).
    pub fn rc_nam(&self) -> Option<&'static Mbuf> {
        self.rc_haddr.get().had_nam
    }

    /// Stores `rc_nam`.
    pub fn set_rc_nam(&self, m: Option<&'static Mbuf>) {
        let mut h = self.rc_haddr.get();
        h.had_nam = m;
        self.rc_haddr.set(h);
    }
}

impl Default for NfsrvCache {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, nfsrvcache)`: the cache's LRU list, through `rc_lru`.
    pub RcLru: NfsrvCache, rc_lru => TailqEntry<NfsrvCache>
);

queue_adapter!(
    /// `LIST_HEAD(nfsrvhash, nfsrvcache)`: a hash chain of the cache, through `rc_hash`.
    pub RcHash: NfsrvCache, rc_hash => ListEntry<NfsrvCache>
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/nfs/nfsrvcache.h");
        let ours: &[(&str, i64)] = &[
            ("NFSRVCACHESIZ", NFSRVCACHESIZ as i64),
            ("RC_UNUSED", RC_UNUSED.into()),
            ("RC_INPROG", RC_INPROG.into()),
            ("RC_DONE", RC_DONE.into()),
            ("RC_DROPIT", RC_DROPIT.into()),
            ("RC_REPLY", RC_REPLY.into()),
            ("RC_DOIT", RC_DOIT.into()),
            ("RC_CHECKIT", RC_CHECKIT.into()),
            ("RC_LOCKED", RC_LOCKED.into()),
            ("RC_WANTED", RC_WANTED.into()),
            ("RC_REPSTATUS", RC_REPSTATUS.into()),
            ("RC_REPMBUF", RC_REPMBUF.into()),
            ("RC_INETADDR", RC_INETADDR.into()),
            ("RC_NAM", RC_NAM.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
