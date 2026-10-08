/*	$OpenBSD: uipc_mbuf.c,v 1.307 2026/07/03 11:51:57 dlg Exp $	*/
/*	$NetBSD: uipc_mbuf.c,v 1.15.4.1 1996/06/13 17:11:44 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1991, 1993
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
 *	@(#)uipc_mbuf.c	8.2 (Berkeley) 1/4/94
 */

/*
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

/* <CODE> */
//! The mbuf allocator and the mbuf chain operations: `kern/uipc_mbuf.c`.
//!
//! Upstream: sys/kern/uipc_mbuf.c @ 3ce1f3f79392
//!
//! Mbufs come from `mbpool`, clusters from one of eight pools of growing size (`mclpools`),
//! both through `m_pool_allocator`, which charges every page against `nmbclust` clusters'
//! worth of memory. A cluster shared by several mbufs (`m_copym`, `m_split`) is reference
//! counted through a `struct m_ext_refs`. The file also has the chain operations (copy,
//! adjust, pull up, split, ...), the packet header helpers, the ddb printers and the
//! `mbuf_list`/`mbuf_queue` functions.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `mbstat` (`struct cpumem *`, `COUNTERS_BOOT_INITIALIZER(mbstat_boot)`) is [`mbstat()`]:
//!   the boot counters [`MBSTAT_BOOT`] until `mbcpuinit` stores the per-CPU handle (M11e,
//!   `counters_alloc_ncpus`, with the `pool_cache_init` of the mbuf, tag, ext-refs and
//!   cluster pools), on the boot CPU before the other CPUs run.
//! - The `NPF > 0` paths (`pf_mbuf_unlink_state_key`, `pf_mbuf_unlink_inpcb`,
//!   `pf_mbuf_link_state_key`, `pf_mbuf_link_inpcb`) are compiled: pf(4) is configured.
//! - `mclnames` is built at compile time from `mclsizes` with the C's two formats (`mcl%dk`,
//!   `mcl%dk%u`) instead of by `snprintf` in `mbinit`, and `m_pool_allocator.pa_pagesz` is
//!   `pool_allocator_multi`'s from the initialiser instead of copied by `mbinit`.
//! - `IFQ_DEFPRIO` and `IFQCTL_*` (`<net/if.h>`) and `CACHELINESIZE` (`<sys/percpu.h>`) are
//!   private constants here until those headers are ported. `MCLPOOLS` (`<net/if.h>`) is
//!   spelled `MCLSIZES.len()`, because the static [`MCLPOOLS`] (the C's `mclpools`) takes
//!   the name.
//! - Out parameters are return values: `m_getptr` and `m_makespace` return the offset with the
//!   mbuf, `m_microtime` returns the `timeval`. `m_copydata`, `m_copyback` and `m_devget` take
//!   slices; `m_apply` takes a closure over the bytes in place of a function and its `fstate`.
//! - The external free functions are `unsafe fn(*mut u8, u32, *mut ())` ([`MextFreeFn`]):
//!   they trust the buffer and argument `MEXTADD` stored.
//! - A `PR_WAITOK` allocation that finds no memory fails instead of sleeping (`pool_get`'s
//!   deviation, `kern/subr_pool.rs`).
//! - `m_clget` panics on a request larger than the largest cluster with or without
//!   `DIAGNOSTIC`; without it the C dereferences the NULL pool.

use core::cell::Cell;
use core::cmp::min;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

use libkern::{StaticCell, explicit_bzero};

use crate::conf::param::NMBCLUST;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_shared, refcnt_take};
use crate::kern::kern_sysctl::{sysctl_int, sysctl_rdint};
use crate::kern::kern_tc::{microboottime, microtime};
use crate::kern::subr_percpu::counters_alloc_ncpus;
use crate::kern::subr_pool::{
    POOL_ALLOCATOR_MULTI, pool_cache_init, pool_get, pool_init, pool_put, pool_set_constraints,
    pool_wakeup,
};
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::kern::uipc_mbuf2::{m_tag_copy_chain, m_tag_delete_chain};
use crate::machine::db_machdep::PrFn;
use crate::machine::intr::{IPL_NET, splnet, splx};
use crate::net::pf::{
    pf_mbuf_inp, pf_mbuf_link_inpcb, pf_mbuf_link_state_key, pf_mbuf_statekey,
    pf_mbuf_unlink_inpcb, pf_mbuf_unlink_state_key,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_BITS, M_COPYALL, M_COPYFLAGS, M_DONTWAIT, M_EOR, M_EXT, M_EXTWR, M_PKTHDR, M_TIMESTAMP,
    M_WAIT, M_ZEROIZE, MAXMCLBYTES, MCLBYTES, MCS_BITS, MEXTFREE_POOL, MHLEN, MHdr, MINCLSIZE,
    MLEN, MPF_BITS, MSIZE, MT_DATA, MT_NTYPES, MTAG_BITS, MTag, MbstatCounters, Mbuf, MbufList,
    MbufQueue, MextFreeFn, m_move_pkthdr, m_readonly, mbstat_inc, mclget, mclgetl,
    mclinitreference, mextadd, ml_empty, ml_len, mq_drops, mq_len, mtod,
};
use crate::sys::percpu::{CpumemBootMemory, CpumemPtr, counters_boot_words, counters_dec};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, Pool, PoolAllocator};
use crate::sys::refcnt::Refcnt;
use crate::sys::time::{Timeval, nsec_to_timeval, timeradd};
use crate::uvm::uvm_km::{KP_DMA_CONTIG, KP_MBUF_CONTIG};
use crate::{kassert, kdassert};

/// `IFQ_DEFPRIO` (`<net/if.h>`, not ported yet): the default packet priority.
const IFQ_DEFPRIO: u8 = 3;

/// `CACHELINESIZE` (`<sys/percpu.h>`, not ported yet).
const CACHELINESIZE: u32 = 64;

/// `IFQCTL_LEN` (`<net/if.h>`, not ported yet).
const IFQCTL_LEN: i32 = 1;
/// `IFQCTL_MAXLEN` (`<net/if.h>`, not ported yet).
const IFQCTL_MAXLEN: i32 = 2;
/// `IFQCTL_DROPS` (`<net/if.h>`, not ported yet).
const IFQCTL_DROPS: i32 = 3;

/// `struct m_ext_refs`: the reference count of a shared cluster, with the free function and
/// argument the cluster had before it was shared.
struct MExtRefs {
    /// `arg`: the original free function's argument.
    arg: Cell<*mut ()>,
    /// `free_fn`: the original free function's index.
    free_fn: Cell<u32>,
    /// `zero`: zero the buffer when the last reference goes. Only ever set, by `m_zero`.
    zero: AtomicU32,
    /// `refs`.
    refs: Refcnt,
}

/// `mbs_ncounters`: the mbuf statistics counters, the `MT_*` types first, then
/// [`MbstatCounters`].
pub const MBS_NCOUNTERS: usize = MbstatCounters::MbsNcounters as usize;

/// `COUNTERS_BOOT_MEMORY(mbstat_boot, MBSTAT_COUNT)`: the boot CPU's counters until
/// `mbcpuinit`, which keeps them as CPU 0's.
pub static MBSTAT_BOOT: CpumemBootMemory<{ counters_boot_words(MBS_NCOUNTERS) }> =
    CpumemBootMemory::new();
/// `mbstat` once `mbcpuinit` has run (see the module's deviations).
static MBSTAT: StaticCell<Option<CpumemPtr>> = StaticCell::new(None);

/// `mbpool`: the mbuf pool.
pub static MBPOOL: Pool = Pool::new();
/// `mtagpool`: the packet tag pool.
pub static MTAGPOOL: Pool = Pool::new();

/// `mclsizes[MCLPOOLS]`: the mbuf cluster pools' sizes.
pub static MCLSIZES: [u32; 8] = [
    MCLBYTES as u32,     // must be at slot 0
    MCLBYTES as u32 + 2, // ETHER_ALIGNED 2k mbufs
    4 * 1024,
    8 * 1024,
    (9 * 1024) + 128, // use more of the pool page for ETHER_ALIGNED etc
    12 * 1024,
    16 * 1024,
    64 * 1024,
];
/// `mclnames`: the cluster pools' names, NUL-terminated.
static MCLNAMES: [[u8; 16]; MCLSIZES.len()] = mclnames(&MCLSIZES);
/// `mclpools`: the mbuf cluster pools.
pub static MCLPOOLS: [Pool; MCLSIZES.len()] = [const { Pool::new() }; MCLSIZES.len()];

/// `max_linkhdr`: largest link-level header.
pub static MAX_LINKHDR: AtomicI32 = AtomicI32::new(0);
/// `max_protohdr`: largest protocol header.
pub static MAX_PROTOHDR: AtomicI32 = AtomicI32::new(0);
/// `max_hdr`: largest link+protocol header.
pub static MAX_HDR: AtomicI32 = AtomicI32::new(0);

/// `m_ext_refs_pool`.
static M_EXT_REFS_POOL: Pool = Pool::new();

/// `m_extfree_refs_fn`: the index of `m_extfree_refs`.
pub static M_EXTFREE_REFS_FN: AtomicU32 = AtomicU32::new(0);

/// \[a\] `mbuf_mem_limit`: how much memory can be allocated.
pub static MBUF_MEM_LIMIT: AtomicU64 = AtomicU64::new(0);
/// \[a\] `mbuf_mem_alloc`: how much memory has been allocated.
pub static MBUF_MEM_ALLOC: AtomicU64 = AtomicU64::new(0);

/// `m_pool_allocator`: `pool_allocator_multi` with the memory limit in front.
pub static M_POOL_ALLOCATOR: PoolAllocator = PoolAllocator {
    pa_alloc: m_pool_alloc,
    pa_free: m_pool_free,
    pa_pagesz: POOL_ALLOCATOR_MULTI.pa_pagesz,
};

/// `mextfree_fns`: the registered external storage free functions. Written by
/// `mextfree_register`, at boot or attach on one CPU, before an mbuf can name the new index.
static MEXTFREE_FNS: StaticCell<[Option<MextFreeFn>; 4]> = StaticCell::new([None; 4]);
/// `num_extfree_fns`.
static NUM_EXTFREE_FNS: AtomicU32 = AtomicU32::new(0);

/// `m_types`: the ddb names of the `MT_*` types.
pub static M_TYPES: [&str; MT_NTYPES] = ["fre", "dat", "hdr", "nam", "opt", "ftb", "ctl", "oob"];

/// Writes `v` in decimal at `at`, as `snprintf` into a 16-byte buffer would (the last byte
/// stays NUL); returns the position after it.
const fn put_decimal(buf: &mut [u8; 16], at: usize, v: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut n = 0;
    let mut v = v;
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut at = at;
    while n > 0 {
        n -= 1;
        if at < buf.len() - 1 {
            buf[at] = digits[n];
            at += 1;
        }
    }
    at
}

/// The names `mbinit` gives the cluster pools: `"mcl%dk%u"` with the low ten bits of the size
/// when they are set, `"mcl%dk"` otherwise.
const fn mclnames(sizes: &[u32; 8]) -> [[u8; 16]; 8] {
    let mut names = [[0u8; 16]; 8];
    let mut i = 0;
    while i < sizes.len() {
        let name = &mut names[i];
        name[0] = b'm';
        name[1] = b'c';
        name[2] = b'l';
        let mut at = put_decimal(name, 3, sizes[i] >> 10);
        name[at] = b'k';
        at += 1;
        let lowbits = sizes[i] & ((1 << 10) - 1);
        if lowbits != 0 {
            put_decimal(name, at, lowbits);
        }
        i += 1;
    }
    names
}

/// `mclnames[i]` as a string.
fn mclname(i: usize) -> &'static str {
    let name = &MCLNAMES[i];
    let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    core::str::from_utf8(&name[..len]).unwrap_or("mcl")
}

/// `M_DATABUF(m)`: the start of the mbuf's data storage.
fn m_databuf(m: &Mbuf) -> *mut u8 {
    if m.m_flags().get() & M_EXT != 0 {
        m.m_ext().ext_buf.get()
    } else if m.m_flags().get() & M_PKTHDR != 0 {
        m.m_pktdat()
    } else {
        m.m_dat()
    }
}

/// `M_SIZE(m)`: the size of the mbuf's data storage.
fn m_size(m: &Mbuf) -> u32 {
    if m.m_flags().get() & M_EXT != 0 {
        m.m_ext().ext_size.get()
    } else if m.m_flags().get() & M_PKTHDR != 0 {
        MHLEN as u32
    } else {
        MLEN as u32
    }
}

/// `m->m_data += by` (or `-=` with a negative `by`).
fn m_data_adj(m: &Mbuf, by: isize) {
    m.m_data().set(m.m_data().get().wrapping_offset(by));
}

/// `mbinit`: initialize the mbuf allocator.
pub fn mbinit() {
    // CTASSERT(MSIZE == sizeof(struct mbuf)): a compile-time check in sys/mbuf.rs.

    // m_pool_allocator.pa_pagesz = pool_allocator_multi.pa_pagesz: the initialiser's.
    kassert!(M_POOL_ALLOCATOR.pa_pagesz == POOL_ALLOCATOR_MULTI.pa_pagesz);

    MBUF_MEM_ALLOC.store(0, Ordering::Relaxed);

    #[cfg(feature = "diagnostic")]
    {
        if MCLSIZES[0] != MCLBYTES as u32 {
            panic(format_args!(
                "mbinit: the smallest cluster size != MCLBYTES"
            ));
        }
        if MCLSIZES[MCLSIZES.len() - 1] != MAXMCLBYTES as u32 {
            panic(format_args!(
                "mbinit: the largest cluster size != MAXMCLBYTES"
            ));
        }
    }

    m_pool_init(&MBPOOL, MSIZE as u32, 64, "mbufpl");

    pool_init(
        &MTAGPOOL,
        PACKET_TAG_POOL_ITEM,
        0,
        IPL_NET,
        0,
        "mtagpl",
        None,
    );
    pool_init(
        &M_EXT_REFS_POOL,
        size_of::<MExtRefs>(),
        CACHELINESIZE,
        IPL_NET,
        0,
        "mextrefs",
        None,
    );

    for (i, pp) in MCLPOOLS.iter().enumerate() {
        m_pool_init(pp, MCLSIZES[i], 64, mclname(i));
    }

    let error = nmbclust_update(NMBCLUST.load(Ordering::Relaxed));
    kassert!(error.is_ok());
    let _ = error;

    let _ = mextfree_register(m_extfree_pool);
    kassert!(NUM_EXTFREE_FNS.load(Ordering::Relaxed) == 1);
    M_EXTFREE_REFS_FN.store(mextfree_register(m_extfree_refs), Ordering::Relaxed);
}

/// `PACKET_TAG_MAXSIZE + sizeof(struct m_tag)`: an `mtagpool` item.
const PACKET_TAG_POOL_ITEM: usize = crate::sys::mbuf::PACKET_TAG_MAXSIZE + size_of::<MTag>();

/// `mbstat`: the mbuf statistics counters (see the module's deviations).
pub fn mbstat() -> CpumemPtr {
    // SAFETY: written once by `mbcpuinit` on the boot CPU before the other CPUs run; only
    // read afterwards.
    match unsafe { MBSTAT.read() } {
        Some(cm) => cm,
        None => MBSTAT_BOOT.initializer(),
    }
}

/// `mbcpuinit`: per-CPU mbuf statistics and pool caches. Without `MULTIPROCESSOR`,
/// `counters_alloc_ncpus` keeps the boot counters and `pool_cache_init` does nothing.
pub fn mbcpuinit() {
    let cm = counters_alloc_ncpus(mbstat(), MBS_NCOUNTERS);
    // SAFETY: once, from `main` on the boot CPU before the other CPUs run (`mbstat`).
    unsafe { MBSTAT.write(Some(cm)) };

    pool_cache_init(&MBPOOL);
    pool_cache_init(&MTAGPOOL);
    pool_cache_init(&M_EXT_REFS_POOL);

    for pp in &MCLPOOLS {
        pool_cache_init(pp);
    }
}

/// `nmbclust_update`: sets the cluster limit and the memory limit derived from it.
pub fn nmbclust_update(newval: i64) -> Result<(), Errno> {
    if newval <= 0 || newval > i64::MAX / MCLBYTES as i64 {
        return Err(Errno::ERANGE);
    }
    // update the global mbuf memory limit
    NMBCLUST.store(newval, Ordering::Relaxed);
    MBUF_MEM_LIMIT.store(newval as u64 * MCLBYTES as u64, Ordering::Relaxed);

    pool_wakeup(&MBPOOL);
    for pp in MCLPOOLS.iter() {
        pool_wakeup(pp);
    }

    Ok(())
}

/// Makes the `mbpool` item `p` an mbuf with a zeroed header.
fn mbuf_from_item(p: NonNull<u8>) -> &'static Mbuf {
    let mp = p.cast::<Mbuf>().as_ptr();
    // SAFETY: an `mbpool` item is `MSIZE` bytes aligned to 64 bytes, the size and more than the
    // alignment of an `Mbuf`; the header is written before the reference is made and the data
    // area is bytes inside an `UnsafeCell`. The item is the caller's until `m_free` returns it
    // to the pool.
    unsafe {
        (&raw mut (*mp).m_hdr).write(MHdr::new());
        &*mp
    }
}

/// `m_get`: space allocation routines.
pub fn m_get(nowait: i32, type_: i32) -> Option<&'static Mbuf> {
    kassert!(type_ >= 0 && (type_ as usize) < MT_NTYPES);

    let p = pool_get(
        &MBPOOL,
        if nowait == M_WAIT {
            PR_WAITOK
        } else {
            PR_NOWAIT
        },
    )?;

    mbstat_inc(type_ as usize);

    let m = mbuf_from_item(p);
    m.m_type().set(type_ as i16);
    m.m_next().set(None);
    m.m_nextpkt().set(None);
    m.m_data().set(m.m_dat());
    m.m_flags().set(0);

    Some(m)
}

/// `m_gethdr`. ATTN: When changing anything here check `m_inithdr()` and `m_defrag()` those
/// may need to change as well.
pub fn m_gethdr(nowait: i32, type_: i32) -> Option<&'static Mbuf> {
    kassert!(type_ >= 0 && (type_ as usize) < MT_NTYPES);

    let p = pool_get(
        &MBPOOL,
        if nowait == M_WAIT {
            PR_WAITOK
        } else {
            PR_NOWAIT
        },
    )?;

    mbstat_inc(type_ as usize);

    let m = mbuf_from_item(p);
    m.m_type().set(type_ as i16);

    Some(m_inithdr(m))
}

/// `m_inithdr`: makes `m` the first mbuf of an empty packet.
pub fn m_inithdr(m: &'static Mbuf) -> &'static Mbuf {
    // keep in sync with m_gethdr
    m.m_next().set(None);
    m.m_nextpkt().set(None);
    m.m_data().set(m.m_pktdat());
    m.m_flags().set(M_PKTHDR);
    m.m_pkthdr_zero();
    m.m_pkthdr().pf.prio.set(IFQ_DEFPRIO);

    m
}

/// `m_clearhdr`: drops the tags and zeroes the packet header.
fn m_clearhdr(m: &Mbuf) {
    // delete all mbuf tags to reset the state
    m_tag_delete_chain(m);
    pf_mbuf_unlink_state_key(m);
    pf_mbuf_unlink_inpcb(m);

    m.m_pkthdr_zero();
}

/// `m_removehdr`: makes the first mbuf of a packet a plain one.
pub fn m_removehdr(m: &Mbuf) {
    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    m_clearhdr(m);
    m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
}

/// `m_resethdr`: like `m_inithdr()`, but keep any associated data and mbufs.
pub fn m_resethdr(m: &Mbuf) {
    let len = m.m_pkthdr().len.get();
    let loopcnt = m.m_pkthdr().ph_loopcnt.get();

    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    m.m_flags()
        .set(m.m_flags().get() & (M_EXT | M_PKTHDR | M_EOR | M_EXTWR | M_ZEROIZE));
    m_clearhdr(m);
    // like m_inithdr(), but keep any associated data and mbufs
    m.m_pkthdr().pf.prio.set(IFQ_DEFPRIO);
    m.m_pkthdr().len.set(len);
    m.m_pkthdr().ph_loopcnt.set(loopcnt);
}

/// `m_calchdrlen`: sets the packet length from the chain's.
pub fn m_calchdrlen(m: &Mbuf) {
    let mut plen = 0i32;

    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    let mut n = Some(m);
    while let Some(nn) = n {
        plen += nn.m_len().get() as i32;
        n = nn.m_next().get();
    }
    m.m_pkthdr().len.set(plen);
}

/// `m_getclr`: an mbuf with its data area zeroed.
pub fn m_getclr(nowait: i32, type_: i32) -> Option<&'static Mbuf> {
    let m = m_get(nowait, type_)?;
    // SAFETY: a fresh mbuf's data starts at `m_dat`, `MLEN` bytes of its own.
    unsafe { ptr::write_bytes(mtod::<u8>(m), 0, MLEN) };
    Some(m)
}

/// `m_clpool`: the smallest cluster pool for a packet of `pktlen` bytes.
pub fn m_clpool(pktlen: u32) -> Option<&'static Pool> {
    MCLPOOLS.iter().find(|pp| pktlen <= pp.pr_size.get())
}

/// `m_clget`: adds a cluster of at least `pktlen` bytes to `m`, or to a new packet header mbuf
/// when `m` is `None`.
pub fn m_clget(m: Option<&'static Mbuf>, how: i32, pktlen: u32) -> Option<&'static Mbuf> {
    let mut m0 = None;

    let Some(pp) = m_clpool(pktlen) else {
        // DIAGNOSTIC's message; without it the C dereferences the NULL pool.
        panic(format_args!("m_clget: request for {} byte cluster", pktlen));
    };

    let m = match m {
        Some(m) => m,
        None => {
            let n = m_gethdr(how, MT_DATA)?;
            m0 = Some(n);
            n
        }
    };
    let Some(buf) = pool_get(pp, if how == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) else {
        m_freem(m0);
        return None;
    };

    mextadd(
        m,
        buf.as_ptr(),
        pp.pr_size.get(),
        M_EXTWR,
        MEXTFREE_POOL,
        ptr::from_ref(pp).cast_mut().cast::<()>(),
    );
    Some(m)
}

/// `m_extfree_pool`: gives a cluster back to its pool.
///
/// # Safety
///
/// `buf` is an item of the pool `pp` points at, and nothing uses it any more.
pub unsafe fn m_extfree_pool(buf: *mut u8, _size: u32, pp: *mut ()) {
    // SAFETY: the caller's guarantee: `pp` is the `Pool` `m_clget` stored.
    let pp = unsafe { &*pp.cast_const().cast::<Pool>() };
    if let Some(buf) = NonNull::new(buf) {
        pool_put(pp, buf);
    }
}

/// `m_ext_refs_shared`: whether the shared cluster has more than one reference.
///
/// # Safety
///
/// `m` has `M_EXT` and its free function is `m_extfree_refs`, so `ext_arg` is the
/// `struct m_ext_refs` `m_extref` made.
pub unsafe fn m_ext_refs_shared(m: &Mbuf) -> bool {
    // SAFETY: the caller's guarantee; the structure lives until the last reference goes.
    let mrefs = unsafe { &*m.m_ext().ext_arg.get().cast_const().cast::<MExtRefs>() };

    refcnt_shared(&mrefs.refs)
}

/// `m_extfree_refs`: drops a reference to a shared cluster; the last one frees it with its
/// original free function.
///
/// # Safety
///
/// `arg` is the `struct m_ext_refs` of the cluster at `buf` of `size` bytes, and the caller's
/// reference is the one being dropped.
unsafe fn m_extfree_refs(buf: *mut u8, size: u32, arg: *mut ()) {
    // SAFETY: the caller's guarantee.
    let mrefs = unsafe { &*arg.cast_const().cast::<MExtRefs>() };

    if refcnt_rele(&mrefs.refs) {
        if mrefs.zero.load(Ordering::Relaxed) != 0 {
            // SAFETY: the last reference: the `size` bytes at `buf` are nobody else's.
            explicit_bzero(unsafe { slice::from_raw_parts_mut(buf, size as usize) });
        }

        kassert!(mrefs.free_fn.get() < NUM_EXTFREE_FNS.load(Ordering::Relaxed));
        kassert!(mrefs.free_fn.get() != M_EXTFREE_REFS_FN.load(Ordering::Relaxed));

        let free = mextfree_fn(mrefs.free_fn.get());
        // SAFETY: `arg` and `free_fn` are what the cluster had before `m_extref` shared it.
        unsafe { free(buf, size, mrefs.arg.get()) };

        pool_put(&M_EXT_REFS_POOL, NonNull::from(mrefs).cast::<u8>());
    }
}

/// `m_free`: frees one mbuf and returns the next in its chain.
pub fn m_free<'a>(m: impl Into<Option<&'a Mbuf>>) -> Option<&'static Mbuf> {
    let m = m.into()?;

    let s = splnet();
    counters_dec(mbstat(), m.m_type().get() as usize);
    splx(s);

    let n = m.m_next().get();
    if m.m_flags().get() & M_ZEROIZE != 0 {
        m_zero(m);
        // propagate M_ZEROIZE to the next mbuf in the chain
        if let Some(n) = n {
            n.m_flags().set(n.m_flags().get() | M_ZEROIZE);
        }
    }
    if m.m_flags().get() & M_PKTHDR != 0 {
        m_tag_delete_chain(m);
        pf_mbuf_unlink_state_key(m);
        pf_mbuf_unlink_inpcb(m);
    }
    if m.m_flags().get() & M_EXT != 0 {
        m_extfree(m);
    }

    pool_put(&MBPOOL, NonNull::from(m).cast::<u8>());

    n
}

/// `m_extref`: makes `n` share `m`'s cluster, turning the cluster into a reference counted
/// one on first share.
fn m_extref(m: &Mbuf, n: &Mbuf, how: i32) -> Result<(), Errno> {
    let refs_fn = M_EXTFREE_REFS_FN.load(Ordering::Relaxed);
    let mrefs: &MExtRefs = if m.m_ext().ext_free_fn.get() == refs_fn {
        // SAFETY: a cluster with the refs free function has its `m_ext_refs` as argument.
        unsafe { &*m.m_ext().ext_arg.get().cast_const().cast::<MExtRefs>() }
    } else {
        let Some(p) = pool_get(&M_EXT_REFS_POOL, how) else {
            return Err(Errno::ENOMEM);
        };
        let p = p.cast::<MExtRefs>().as_ptr();
        // SAFETY: a fresh item of `m_ext_refs_pool`, sized and aligned for a `MExtRefs`; it
        // is written whole before the reference is made.
        let mrefs = unsafe {
            p.write(MExtRefs {
                arg: Cell::new(m.m_ext().ext_arg.get()),
                free_fn: Cell::new(m.m_ext().ext_free_fn.get()),
                zero: AtomicU32::new(0),
                refs: Refcnt::new(),
            });
            &*p
        };
        refcnt_init(&mrefs.refs);

        m.m_ext().ext_arg.set(p.cast::<()>());
        m.m_ext().ext_free_fn.set(refs_fn);
        mrefs
    };

    refcnt_take(&mrefs.refs);

    mextadd(
        n,
        m.m_ext().ext_buf.get(),
        m.m_ext().ext_size.get(),
        m.m_flags().get() & M_EXTWR,
        refs_fn,
        ptr::from_ref(mrefs).cast_mut().cast::<()>(),
    );

    Ok(())
}

/// The free function registered at `index`.
fn mextfree_fn(index: u32) -> MextFreeFn {
    // SAFETY: the table is written by `mextfree_register` before any mbuf names the index.
    let fns = unsafe { MEXTFREE_FNS.get() };
    match fns.get(index as usize).copied().flatten() {
        Some(f) => f,
        None => panic(format_args!("mbuf: no external free function {}", index)),
    }
}

/// `mextfree_register`: returns a number for use with `MEXTADD`. Should only be called once
/// per function. Drivers can be assured that the index will be non zero.
pub fn mextfree_register(f: MextFreeFn) -> u32 {
    let n = NUM_EXTFREE_FNS.load(Ordering::Relaxed);
    kassert!((n as usize) < 4);
    // SAFETY: registration runs at boot or attach on one CPU, before an mbuf names index `n`
    // (the C's unlocked array).
    unsafe { MEXTFREE_FNS.get_mut()[n as usize] = Some(f) };
    NUM_EXTFREE_FNS.store(n + 1, Ordering::Relaxed);
    n
}

/// `m_extfree`: drops the mbuf's external storage.
fn m_extfree(m: &Mbuf) {
    kassert!(m.m_ext().ext_free_fn.get() < NUM_EXTFREE_FNS.load(Ordering::Relaxed));
    let free = mextfree_fn(m.m_ext().ext_free_fn.get());
    // SAFETY: `MEXTADD` stored the buffer, its size and the argument with this function.
    unsafe {
        free(
            m.m_ext().ext_buf.get(),
            m.m_ext().ext_size.get(),
            m.m_ext().ext_arg.get(),
        );
    }

    m.m_flags().set(m.m_flags().get() & !(M_EXT | M_EXTWR));
}

/// `m_freem`: frees a chain and returns the next packet.
pub fn m_freem<'a>(m: impl Into<Option<&'a Mbuf>>) -> Option<&'static Mbuf> {
    let m = m.into()?;

    let n = m.m_nextpkt().get();

    let mut m = m_free(m);
    while let Some(mm) = m {
        m = m_free(mm);
    }

    n
}

/// `m_purge`: frees a list of packets.
pub fn m_purge<'a>(m: impl Into<Option<&'a Mbuf>>) {
    let Some(m) = m.into() else {
        return;
    };
    let mut m = m_freem(m);
    while let Some(mm) = m {
        m = m_freem(mm);
    }
}

/// `m_defrag`: mbuf chain defragmenter. This function uses some evil tricks to defragment an
/// mbuf chain into a single buffer without changing the mbuf pointer. This needs to know a lot
/// of the mbuf internals to make this work. The resulting mbuf is not aligned to IP header to
/// assist DMA transfers.
pub fn m_defrag(m: &Mbuf, how: i32) -> Result<(), Errno> {
    if m.m_next().get().is_none() {
        return Ok(());
    }

    kassert!(m.m_flags().get() & M_PKTHDR != 0);

    mbstat_inc(MbstatCounters::MbsDefragAlloc as usize);
    let Some(m0) = m_gethdr(how, i32::from(m.m_type().get())) else {
        return Err(Errno::ENOBUFS);
    };
    let pktlen = m.m_pkthdr().len.get();
    if pktlen > MHLEN as i32 {
        let _ = mclgetl(m0, how, pktlen as u32);
        if m0.m_flags().get() & M_EXT == 0 {
            m_free(m0);
            return Err(Errno::ENOBUFS);
        }
    }
    // SAFETY: `m0` holds `pktlen` bytes at `m_data`: its own `MHLEN`, or a cluster at least
    // that long.
    m_copydata(m, 0, unsafe {
        slice::from_raw_parts_mut(mtod::<u8>(m0), pktlen as usize)
    });
    m0.m_len().set(pktlen as u32);
    m0.m_pkthdr().len.set(pktlen);

    // free chain behind and possible ext buf on the first mbuf
    m_freem(m.m_next().get());
    m.m_next().set(None);
    if m.m_flags().get() & M_EXT != 0 {
        m_extfree(m);
    }

    // Bounce copy mbuf over to the original mbuf and set everything up. This needs to reset
    // or clear all pointers that may go into the original mbuf chain.
    if m0.m_flags().get() & M_EXT != 0 {
        m.m_ext_assign(m0);
        mclinitreference(m);
        m.m_flags()
            .set(m.m_flags().get() | (m0.m_flags().get() & (M_EXT | M_EXTWR)));
        m.m_data().set(m.m_ext().ext_buf.get());
    } else {
        m.m_data().set(m.m_pktdat());
        // SAFETY: both packet data areas are `MHLEN` bytes and `m0`'s length fits in it.
        unsafe {
            ptr::copy_nonoverlapping(mtod::<u8>(m0), mtod::<u8>(m), m0.m_len().get() as usize);
        }
    }
    m.m_len().set(m0.m_len().get());
    m.m_pkthdr().len.set(m0.m_len().get() as i32);

    m0.m_flags().set(m0.m_flags().get() & !(M_EXT | M_EXTWR)); // cluster is gone
    m_free(m0);

    Ok(())
}

// Mbuffer utility routines.

/// `m_prepend`: ensure `len` bytes of contiguous space at the beginning of the mbuf chain.
pub fn m_prepend(m: &'static Mbuf, len: i32, how: i32) -> Option<&'static Mbuf> {
    if len > MHLEN as i32 {
        panic(format_args!("mbuf prepend length too big"));
    }

    let mut m = m;
    if m_leadingspace(m) >= len {
        m_data_adj(m, -(len as isize));
        m.m_len().set(m.m_len().get() + len as u32);
    } else {
        mbstat_inc(MbstatCounters::MbsPrependAlloc as usize);
        let Some(mn) = m_get(how, i32::from(m.m_type().get())) else {
            m_freem(m);
            return None;
        };
        if m.m_flags().get() & M_PKTHDR != 0 {
            m_move_pkthdr(mn, m);
        }
        mn.m_next().set(Some(m));
        m = mn;
        m_align(m, len);
        m.m_len().set(len as u32);
    }
    if m.m_flags().get() & M_PKTHDR != 0 {
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() + len);
    }
    Some(m)
}

/// `m_copym`: make a copy of an mbuf chain starting `off` bytes from the beginning, continuing
/// for `len` bytes. If `len` is `M_COPYALL`, copy to end of mbuf. The wait parameter is a
/// choice of `M_WAIT`/`M_DONTWAIT` from caller.
pub fn m_copym(m0: &Mbuf, off: i32, len: i32, wait: i32) -> Option<&'static Mbuf> {
    if off < 0 || len < 0 {
        panic(format_args!("m_copym0: off {}, len {}", off, len));
    }
    let mut copyhdr = off == 0 && m0.m_flags().get() & M_PKTHDR != 0;
    let Some((m, off)) = m_getptr(m0, off) else {
        panic(format_args!("m_copym0: short mbuf chain"));
    };
    let mut m: Option<&Mbuf> = Some(m);
    let mut off = off;
    let mut len = len;
    let mut top: Option<&'static Mbuf> = None;
    let mut last: Option<&'static Mbuf> = None;
    while len > 0 {
        let Some(mm) = m else {
            if len != M_COPYALL {
                panic(format_args!("m_copym0: m == NULL and not COPYALL"));
            }
            break;
        };
        let Some(n) = m_get(wait, i32::from(mm.m_type().get())) else {
            // nospace:
            m_freem(top);
            return None;
        };
        // *np = n
        match last {
            None => top = Some(n),
            Some(last) => last.m_next().set(Some(n)),
        }
        if copyhdr {
            if m_dup_pkthdr(n, m0, wait).is_err() {
                m_freem(top);
                return None;
            }
            if len != M_COPYALL {
                n.m_pkthdr().len.set(len);
            }
            copyhdr = false;
        }
        n.m_len()
            .set(min(len as u32, mm.m_len().get() - off as u32));
        if mm.m_flags().get() & M_EXT != 0 {
            if m_extref(mm, n, wait).is_err() {
                m_freem(top);
                return None;
            }
            n.m_data().set(mm.m_data().get().wrapping_add(off as usize));
        } else {
            let base = if mm.m_flags().get() & M_PKTHDR != 0 {
                mm.m_pktdat()
            } else {
                mm.m_dat()
            };
            m_data_adj(n, mm.m_data().get() as isize - base as isize);
            m_data_adj(n, off as isize);
            // SAFETY: `n` sits at the same offset in an area at least as long as `mm`'s (a
            // packet header source only goes to a packet header copy, see `copyhdr`), and
            // `m_len` bytes from `off` are inside `mm`'s data.
            unsafe {
                ptr::copy_nonoverlapping(
                    mtod::<u8>(mm).add(off as usize),
                    mtod::<u8>(n),
                    n.m_len().get() as usize,
                );
            }
        }
        if len != M_COPYALL {
            len -= n.m_len().get() as i32;
        }
        off += n.m_len().get() as i32;
        #[cfg(feature = "diagnostic")]
        if off > mm.m_len().get() as i32 {
            panic(format_args!("m_copym0 overrun"));
        }
        if off == mm.m_len().get() as i32 {
            m = mm.m_next().get();
            off = 0;
        }
        last = Some(n);
    }
    top
}

/// `m_copydata`: copy data from an mbuf chain starting `off` bytes from the beginning,
/// continuing for `p.len()` bytes, into the indicated buffer.
pub fn m_copydata(m: &Mbuf, off: i32, p: &mut [u8]) {
    if off < 0 {
        panic(format_args!("m_copydata: off {} < 0", off));
    }
    let Some((m, off)) = m_getptr(m, off) else {
        panic(format_args!("m_copydata: short mbuf chain"));
    };
    let mut m: Option<&Mbuf> = Some(m);
    let mut off = off as u32;
    let mut cp = 0usize;
    let mut len = p.len();
    while len > 0 {
        let Some(mm) = m else {
            panic(format_args!("m_copydata: null mbuf"));
        };
        let count = min(mm.m_len().get() - off, len as u32) as usize;
        // SAFETY: `count` bytes from `off` are inside `mm`'s data and inside `p`; `copy` is the
        // C's `memmove`.
        unsafe {
            ptr::copy(
                mtod::<u8>(mm).add(off as usize),
                p.as_mut_ptr().add(cp),
                count,
            );
        }
        len -= count;
        cp += count;
        off = 0;
        m = mm.m_next().get();
    }
}

/// `m_copyback`: copy data from a buffer back into the indicated mbuf chain, starting `off`
/// bytes from the beginning, extending the mbuf chain if necessary. The mbuf needs to be
/// properly initialized including the setting of `m_len`.
pub fn m_copyback<'a>(
    m0: impl Into<Option<&'a Mbuf>>,
    off: i32,
    data: &[u8],
    wait: i32,
) -> Result<(), Errno> {
    let Some(m0) = m0.into() else {
        return Ok(());
    };
    let mut off = off;
    let mut len = data.len() as i32;
    let mut cp = 0usize;
    let mut totlen = 0i32;
    let mut m: &Mbuf = m0;

    let error: Result<(), Errno> = 'out: {
        loop {
            let mlen = m.m_len().get() as i32;
            if off <= mlen {
                break;
            }
            off -= mlen;
            totlen += mlen;
            let next = match m.m_next().get() {
                Some(next) => next,
                None => {
                    let Some(n) = m_get(wait, i32::from(m.m_type().get())) else {
                        break 'out Err(Errno::ENOBUFS);
                    };

                    if off + len > MLEN as i32 {
                        let _ = mclgetl(n, wait, (off + len) as u32);
                        if n.m_flags().get() & M_EXT == 0 {
                            m_free(n);
                            break 'out Err(Errno::ENOBUFS);
                        }
                    }
                    // SAFETY: `n` holds `off + len` bytes at `m_data` (MLEN or the cluster).
                    unsafe { ptr::write_bytes(mtod::<u8>(n), 0, off as usize) };
                    n.m_len().set((len + off) as u32);
                    m.m_next().set(Some(n));
                    n
                }
            };
            m = next;
        }
        while len > 0 {
            // extend last packet to be filled fully
            if m.m_next().get().is_none() && len > m.m_len().get() as i32 - off {
                let grow = min(
                    (len - (m.m_len().get() as i32 - off)) as u32,
                    m_trailingspace(m) as u32,
                );
                m.m_len().set(m.m_len().get() + grow);
            }
            let mlen = min(m.m_len().get() as i32 - off, len);
            // SAFETY: `mlen` bytes from `off` are inside `m`'s data (`m_len` covers them) and
            // inside `data` from `cp`.
            unsafe {
                ptr::copy(
                    data.as_ptr().add(cp),
                    mtod::<u8>(m).add(off as usize),
                    mlen as usize,
                );
            }
            cp += mlen as usize;
            len -= mlen;
            totlen += mlen + off;
            if len == 0 {
                break;
            }
            off = 0;

            let next = match m.m_next().get() {
                Some(next) => next,
                None => {
                    let Some(n) = m_get(wait, i32::from(m.m_type().get())) else {
                        break 'out Err(Errno::ENOBUFS);
                    };

                    if len > MLEN as i32 {
                        let _ = mclgetl(n, wait, len as u32);
                        if n.m_flags().get() & M_EXT == 0 {
                            m_free(n);
                            break 'out Err(Errno::ENOBUFS);
                        }
                    }
                    n.m_len().set(len as u32);
                    m.m_next().set(Some(n));
                    n
                }
            };
            m = next;
        }
        Ok(())
    };
    // out:
    if m0.m_flags().get() & M_PKTHDR != 0 && m0.m_pkthdr().len.get() < totlen {
        m0.m_pkthdr().len.set(totlen);
    }

    error
}

/// `m_cat`: concatenate mbuf chain `n` to `m`. `n` might be copied into `m` (when `n->m_len`
/// is small), therefore data portion of `n` could be copied into an mbuf of different mbuf
/// type. Therefore both chains should be of the same type (e.g. `MT_DATA`). Any `m_pkthdr`
/// is not updated.
pub fn m_cat(m: &Mbuf, n: Option<&'static Mbuf>) {
    let mut m = m;
    while let Some(next) = m.m_next().get() {
        m = next;
    }
    let mut n = n;
    while let Some(nn) = n {
        if m_readonly(m) || nn.m_len().get() as i32 > m_trailingspace(m) {
            // just join the two chains
            m.m_next().set(Some(nn));
            return;
        }
        // splat the data from one into the other
        // SAFETY: `m` has `nn.m_len` bytes of trailing space after its data (checked above).
        unsafe {
            ptr::copy_nonoverlapping(
                mtod::<u8>(nn),
                mtod::<u8>(m).add(m.m_len().get() as usize),
                nn.m_len().get() as usize,
            );
        }
        m.m_len().set(m.m_len().get() + nn.m_len().get());
        n = m_free(nn);
    }
}

/// `m_adj`: trims `req_len` bytes from the head of the chain, or `-req_len` bytes from the
/// tail when negative.
pub fn m_adj<'a>(mp: impl Into<Option<&'a Mbuf>>, req_len: i32) {
    let Some(mp) = mp.into() else {
        return;
    };
    let mut len = req_len;
    if len >= 0 {
        // Trim from head.
        let mut m = Some(mp);
        while let Some(mm) = m
            && len > 0
        {
            if mm.m_len().get() as i32 <= len {
                len -= mm.m_len().get() as i32;
                m_data_adj(mm, mm.m_len().get() as isize);
                mm.m_len().set(0);
                m = mm.m_next().get();
            } else {
                m_data_adj(mm, len as isize);
                mm.m_len().set(mm.m_len().get() - len as u32);
                len = 0;
            }
        }
        if mp.m_flags().get() & M_PKTHDR != 0 {
            mp.m_pkthdr()
                .len
                .set(mp.m_pkthdr().len.get() - (req_len - len));
        }
    } else {
        // Trim from tail. Scan the mbuf chain, calculating its length and finding the last
        // mbuf. If the adjustment only affects this mbuf, then just adjust and return.
        // Otherwise, rescan and truncate after the remaining size.
        len = -len;
        let mut count = 0i32;
        let mut m: &Mbuf = mp;
        loop {
            count += m.m_len().get() as i32;
            match m.m_next().get() {
                None => break,
                Some(next) => m = next,
            }
        }
        if m.m_len().get() as i32 >= len {
            m.m_len().set(m.m_len().get() - len as u32);
            if mp.m_flags().get() & M_PKTHDR != 0 {
                mp.m_pkthdr().len.set(mp.m_pkthdr().len.get() - len);
            }
            return;
        }
        count -= len;
        if count < 0 {
            count = 0;
        }
        // Correct length for chain is "count". Find the mbuf with last data, adjust its
        // length, and toss data from remaining mbufs on chain.
        if mp.m_flags().get() & M_PKTHDR != 0 {
            mp.m_pkthdr().len.set(count);
        }
        let mut m: &Mbuf = mp;
        loop {
            if m.m_len().get() as i32 >= count {
                m.m_len().set(count as u32);
                break;
            }
            count -= m.m_len().get() as i32;
            // the chain holds at least `count` bytes, so a next mbuf exists
            match m.m_next().get() {
                None => break,
                Some(next) => m = next,
            }
        }
        let mut next = m.m_next().get();
        while let Some(nn) = next {
            nn.m_len().set(0);
            next = nn.m_next().get();
        }
    }
}

/// `m_pullup`: rearrange an mbuf chain so that `len` bytes are contiguous and in the data area
/// of an mbuf (so that `mtod` will work for a structure of size `len`). Returns the resulting
/// mbuf chain on success, frees it and returns `None` on failure.
pub fn m_pullup(m0: &'static Mbuf, len: i32) -> Option<&'static Mbuf> {
    let mut len = len;

    // if len is already contig in m0, then don't do any work
    if len <= m0.m_len().get() as i32 {
        return Some(m0);
    }

    // look for some data
    let Some(mut m) = m0.m_next().get() else {
        // freem0:
        m_free(m0);
        return None;
    };

    let head0 = m_databuf(m0);
    let adj = if m0.m_len().get() == 0 {
        while m.m_len().get() == 0 {
            match m_free(m) {
                Some(next) => m = next,
                None => {
                    m_free(m0);
                    return None;
                }
            }
        }

        mtod::<u8>(m) as usize & (size_of::<usize>() - 1)
    } else {
        mtod::<u8>(m0) as usize & (size_of::<usize>() - 1)
    };

    let tail = head0 as usize + m_size(m0) as usize;
    let head = head0.wrapping_add(adj);

    let mut m0 = m0;
    if !m_readonly(m0) && len as isize <= tail as isize - head as isize {
        // we can copy everything into the first mbuf
        if m0.m_len().get() == 0 {
            m0.m_data().set(head);
        } else if len as isize > tail as isize - mtod::<u8>(m0) as isize {
            // need to memmove to make space at the end
            // SAFETY: both ranges are inside `m0`'s storage (`head + m_len <= tail`, since
            // `len > m_len` fits); `copy` is the C's `memmove`.
            unsafe {
                ptr::copy(mtod::<u8>(m0), head, m0.m_len().get() as usize);
            }
            m0.m_data().set(head);
        }
        len -= m0.m_len().get() as i32;
        mbstat_inc(MbstatCounters::MbsPullupCopy as usize);
    } else {
        // the first mbuf is too small or read-only, make a new one
        let space = adj + len as usize;

        if space > MAXMCLBYTES {
            // bad:
            m_freem(m);
            m_free(m0);
            return None;
        }

        m0.m_next().set(Some(m));
        m = m0;

        mbstat_inc(MbstatCounters::MbsPullupAlloc as usize);
        let Some(n0) = m_get(M_DONTWAIT, i32::from(m.m_type().get())) else {
            m_freem(m);
            return None;
        };
        m0 = n0;

        if space > MHLEN {
            let _ = mclgetl(m0, M_DONTWAIT, space as u32);
            if m0.m_flags().get() & M_EXT == 0 {
                m_freem(m);
                m_free(m0);
                return None;
            }
        }

        if m.m_flags().get() & M_PKTHDR != 0 {
            m_move_pkthdr(m0, m);
        }

        m0.m_len().set(0);
        m_data_adj(m0, adj as isize);
    }

    kdassert!(m_trailingspace(m0) >= len);

    loop {
        let space = min(len as u32, m.m_len().get());
        // SAFETY: `m0` has at least `len` bytes of trailing space (asserted above) and `space`
        // bytes are `m`'s data.
        unsafe {
            ptr::copy_nonoverlapping(
                mtod::<u8>(m),
                mtod::<u8>(m0).add(m0.m_len().get() as usize),
                space as usize,
            );
        }
        len -= space as i32;
        m0.m_len().set(m0.m_len().get() + space);
        m.m_len().set(m.m_len().get() - space);

        let next = if m.m_len().get() > 0 {
            m_data_adj(m, space as isize);
            Some(m)
        } else {
            m_free(m)
        };

        if len == 0 {
            m0.m_next().set(next); // link the chain back up
            return Some(m0);
        }

        match next {
            Some(next) => m = next,
            None => {
                // bad:
                m_free(m0);
                return None;
            }
        }
    }
}

/// `m_getptr`: the mbuf and offset of location `loc` in the chain; the end of the valid data
/// when `loc` is the chain's length.
pub fn m_getptr(m: &Mbuf, loc: i32) -> Option<(&Mbuf, i32)> {
    let mut m = m;
    let mut loc = loc;
    while loc >= 0 {
        // Normal end of search
        if m.m_len().get() as i32 > loc {
            return Some((m, loc));
        }
        loc -= m.m_len().get() as i32;

        match m.m_next().get() {
            None => {
                if loc == 0 {
                    // Point at the end of valid data
                    return Some((m, m.m_len().get() as i32));
                }
                return None;
            }
            Some(next) => m = next,
        }
    }

    None
}

/// `m_split`: partition an mbuf chain in two pieces, returning the tail -- all but the first
/// `len0` bytes. In case of failure, it returns `None` and attempts to restore the chain to
/// its original state.
pub fn m_split(m0: &'static Mbuf, len0: i32, wait: i32) -> Option<&'static Mbuf> {
    let mut len = len0 as u32;
    let mut olen = 0u32;

    let mut m = Some(m0);
    while let Some(mm) = m
        && len > mm.m_len().get()
    {
        len -= mm.m_len().get();
        m = mm.m_next().get();
    }
    let m = m?;
    let remain = m.m_len().get() - len;
    let n: &'static Mbuf;
    if m0.m_flags().get() & M_PKTHDR != 0 {
        n = m_gethdr(wait, i32::from(m0.m_type().get()))?;
        if m_dup_pkthdr(n, m0, wait).is_err() {
            m_freem(n);
            return None;
        }
        n.m_pkthdr().len.set(n.m_pkthdr().len.get() - len0);
        olen = m0.m_pkthdr().len.get() as u32;
        m0.m_pkthdr().len.set(len0);
        if remain == 0 {
            n.m_next().set(m.m_next().get());
            m.m_next().set(None);
            n.m_len().set(0);
            return Some(n);
        }
        if m.m_flags().get() & M_EXT == 0 && remain as usize > MHLEN {
            // m can't be the lead packet
            m_align(n, 0);
            n.m_next().set(m_split(m, len as i32, wait));
            if n.m_next().get().is_none() {
                m_free(n);
                m0.m_pkthdr().len.set(olen as i32);
                return None;
            }
            n.m_len().set(0);
            return Some(n);
        }
    } else if remain == 0 {
        let n = m.m_next().get();
        m.m_next().set(None);
        return n;
    } else {
        n = m_get(wait, i32::from(m.m_type().get()))?;
    }
    if m.m_flags().get() & M_EXT != 0 {
        if m_extref(m, n, wait).is_err() {
            m_freem(n);
            if m0.m_flags().get() & M_PKTHDR != 0 {
                m0.m_pkthdr().len.set(olen as i32);
            }
            return None;
        }
        n.m_data().set(m.m_data().get().wrapping_add(len as usize));
    } else {
        m_align(n, remain as i32);
        // SAFETY: `m_align` left `remain` bytes at `n`'s data, and `remain` bytes from `len`
        // are `m`'s data.
        unsafe {
            ptr::copy_nonoverlapping(
                mtod::<u8>(m).add(len as usize),
                mtod::<u8>(n),
                remain as usize,
            );
        }
    }
    n.m_len().set(remain);
    m.m_len().set(len);
    n.m_next().set(m.m_next().get());
    m.m_next().set(None);
    Some(n)
}

/// `m_makespace`: make space for a new header of length `hlen` at `skip` bytes into the
/// packet. When doing this we allocate new mbufs only when absolutely necessary. The mbuf
/// where the new header is to go is returned together with an offset into the mbuf. If `None`
/// is returned then the mbuf chain may have been modified; the caller is assumed to always
/// free the chain.
pub fn m_makespace(m0: &'static Mbuf, skip: i32, hlen: i32) -> Option<(&'static Mbuf, i32)> {
    kassert!(m0.m_flags().get() & M_PKTHDR != 0);
    // Limit the size of the new header to MHLEN. In case skip = 0 and the first buffer is not
    // a cluster this is the maximum space available in that mbuf. In other words this code
    // never prepends a mbuf.
    kassert!(hlen < MHLEN as i32);

    let mut skip = skip;
    let mut m = Some(m0);
    while let Some(mm) = m
        && skip > mm.m_len().get() as i32
    {
        skip -= mm.m_len().get() as i32;
        m = mm.m_next().get();
    }
    let mut m = m?;
    // At this point skip is the offset into the mbuf m where the new header should be placed.
    // Figure out if there's space to insert the new header. If so, and copying the remainder
    // makes sense then do so. Otherwise insert a new mbuf in the chain, splitting the contents
    // of m as needed.
    let remain = m.m_len().get() - skip as u32; // data to move
    let off;
    if (skip as u32) < remain && hlen <= m_leadingspace(m) {
        if skip != 0 {
            // SAFETY: `hlen` bytes of leading space precede the data; `copy` is `memmove`.
            unsafe {
                ptr::copy(
                    mtod::<u8>(m),
                    mtod::<u8>(m).wrapping_sub(hlen as usize),
                    skip as usize,
                );
            }
        }
        m_data_adj(m, -(hlen as isize));
        m.m_len().set(m.m_len().get() + hlen as u32);
        off = skip;
    } else if hlen > m_trailingspace(m) {
        if remain > 0 {
            let mut n = m_get(M_DONTWAIT, i32::from(m.m_type().get()));
            if let Some(nn) = n
                && remain as usize > MLEN
            {
                let _ = mclgetl(nn, M_DONTWAIT, remain);
                if nn.m_flags().get() & M_EXT == 0 {
                    m_free(nn);
                    n = None;
                }
            }
            let n = n?;

            // SAFETY: `n` holds `remain` bytes (MLEN or the cluster) and they are `m`'s data
            // after `skip`.
            unsafe {
                ptr::copy_nonoverlapping(
                    mtod::<u8>(m).add(skip as usize),
                    mtod::<u8>(n),
                    remain as usize,
                );
            }
            n.m_len().set(remain);
            m.m_len().set(m.m_len().get() - remain);

            n.m_next().set(m.m_next().get());
            m.m_next().set(Some(n));
        }

        if hlen <= m_trailingspace(m) {
            m.m_len().set(m.m_len().get() + hlen as u32);
            off = skip;
        } else {
            let n = m_get(M_DONTWAIT, i32::from(m.m_type().get()))?;

            n.m_len().set(hlen as u32);

            n.m_next().set(m.m_next().get());
            m.m_next().set(Some(n));

            off = 0; // header is at front ...
            m = n; // ... of new mbuf
        }
    } else {
        // Copy the remainder to the back of the mbuf so there's space to write the new header.
        if remain > 0 {
            // SAFETY: `hlen` bytes of trailing space follow the data; `copy` is `memmove`.
            unsafe {
                ptr::copy(
                    mtod::<u8>(m).add(skip as usize),
                    mtod::<u8>(m).add((skip + hlen) as usize),
                    remain as usize,
                );
            }
        }
        m.m_len().set(m.m_len().get() + hlen as u32);
        off = skip;
    }
    m0.m_pkthdr().len.set(m0.m_pkthdr().len.get() + hlen); // adjust packet length
    Some((m, off))
}

/// `m_devget`: routine to copy from device local memory into mbufs; the packet is `buf`.
pub fn m_devget(buf: &[u8], off: i32) -> Option<&'static Mbuf> {
    let mut totlen = buf.len() as i32;
    let mut off = off;
    let mut top: Option<&'static Mbuf> = None;
    let mut last: Option<&'static Mbuf> = None;
    let mut cp = 0usize;

    if off < 0 || off > MHLEN as i32 {
        return None;
    }

    let mut m = m_gethdr(M_DONTWAIT, MT_DATA)?;

    m.m_pkthdr().len.set(totlen);

    let mut len = MHLEN as i32;

    // As in C, a zero `totlen` returns NULL and keeps the header mbuf.
    while totlen > 0 {
        if let Some(top) = top {
            let Some(n) = m_get(M_DONTWAIT, MT_DATA) else {
                // As we might get called by pfkey, make sure we do not leak sensitive data.
                top.m_flags().set(top.m_flags().get() | M_ZEROIZE);
                m_freem(top);
                return None;
            };
            m = n;
            len = MLEN as i32;
        }

        if totlen + off >= MINCLSIZE as i32 {
            mclget(m, M_DONTWAIT);
            if m.m_flags().get() & M_EXT != 0 {
                len = MCLBYTES as i32;
            }
        } else {
            // Place initial small packet/header at end of mbuf.
            let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed);
            if top.is_none() && totlen + off + max_linkhdr <= len {
                m_data_adj(m, max_linkhdr as isize);
                len -= max_linkhdr;
            }
        }

        if off != 0 {
            m_data_adj(m, off as isize);
            len -= off;
            off = 0;
        }

        len = min(totlen, len);
        m.m_len().set(len as u32);
        // SAFETY: `m` has `len` bytes at `m_data` (the sizes above), `buf` has them from `cp`.
        unsafe {
            ptr::copy_nonoverlapping(buf.as_ptr().add(cp), mtod::<u8>(m), len as usize);
        }

        cp += len as usize;
        match last {
            None => top = Some(m),
            Some(last) => last.m_next().set(Some(m)),
        }
        last = Some(m);
        totlen -= len;
    }
    top
}

/// `m_zero`: zeroes the mbuf's storage, or marks a shared cluster to be zeroed when its last
/// reference goes.
pub fn m_zero(m: &Mbuf) {
    if m.m_flags().get() & M_EXT != 0
        && m.m_ext().ext_free_fn.get() == M_EXTFREE_REFS_FN.load(Ordering::Relaxed)
    {
        // SAFETY: the refs free function's argument is the `m_ext_refs`.
        let mrefs = unsafe { &*m.m_ext().ext_arg.get().cast_const().cast::<MExtRefs>() };

        // this variable only transitions in one direction, so if there is a race it will be
        // toward the same result and therefore there is no loss.

        mrefs.zero.store(1, Ordering::Relaxed);
        return;
    }

    // SAFETY: `M_DATABUF`/`M_SIZE` describe the mbuf's own storage.
    explicit_bzero(unsafe { slice::from_raw_parts_mut(m_databuf(m), m_size(m) as usize) });
}

/// `m_apply`: apply function `f` to the data in an mbuf chain starting `off` bytes from the
/// beginning, continuing for `len` bytes; the first error stops the walk.
pub fn m_apply(
    m: &Mbuf,
    off: i32,
    len: i32,
    mut f: impl FnMut(&[u8]) -> Result<(), Errno>,
) -> Result<(), Errno> {
    if len < 0 {
        panic(format_args!("m_apply: len {} < 0", len));
    }
    if off < 0 {
        panic(format_args!("m_apply: off {} < 0", off));
    }
    let mut m = Some(m);
    let mut off = off;
    let mut len = len;
    while off > 0 {
        let Some(mm) = m else {
            panic(format_args!("m_apply: null mbuf in skip"));
        };
        if off < mm.m_len().get() as i32 {
            break;
        }
        off -= mm.m_len().get() as i32;
        m = mm.m_next().get();
    }
    while len > 0 {
        let Some(mm) = m else {
            panic(format_args!("m_apply: null mbuf"));
        };
        let count = min(mm.m_len().get() - off as u32, len as u32);

        // SAFETY: `count` bytes from `off` are `mm`'s data.
        let data = unsafe {
            slice::from_raw_parts(
                mtod::<u8>(mm).add(off as usize).cast_const(),
                count as usize,
            )
        };
        f(data)?;

        len -= count as i32;
        off = 0;
        m = mm.m_next().get();
    }

    Ok(())
}

/// `m_leadingspace`: compute the amount of space available before the current start of data
/// in an mbuf. Read-only clusters never have space available.
pub fn m_leadingspace(m: &Mbuf) -> i32 {
    if m_readonly(m) {
        return 0;
    }
    kassert!(m.m_data().get() as usize >= m_databuf(m) as usize);
    (m.m_data().get() as usize - m_databuf(m) as usize) as i32
}

/// `m_trailingspace`: compute the amount of space available after the end of data in an mbuf.
/// Read-only clusters never have space available.
pub fn m_trailingspace(m: &Mbuf) -> i32 {
    if m_readonly(m) {
        return 0;
    }
    let end = m_databuf(m) as usize + m_size(m) as usize;
    let data_end = m.m_data().get() as usize + m.m_len().get() as usize;
    kassert!(end >= data_end);
    (end - data_end) as i32
}

/// `m_align`: set the `m_data` pointer of a newly-allocated mbuf to place an object of the
/// specified size at the end of the mbuf, longword aligned.
pub fn m_align(m: &Mbuf, len: i32) {
    const LONG: usize = size_of::<usize>();
    kassert!(len >= 0 && !m_readonly(m));
    kassert!(m.m_data().get() == m_databuf(m)); // newly-allocated check
    kassert!(((len as usize + LONG - 1) & !(LONG - 1)) <= m_size(m) as usize);

    m.m_data()
        .set(m_databuf(m).wrapping_add((m_size(m) as usize - len as usize) & !(LONG - 1)));
}

/// `m_dup_pkthdr`: duplicate mbuf pkthdr from `from` to `to`. `from` must have `M_PKTHDR`
/// set, and `to` must be empty.
pub fn m_dup_pkthdr(to: &Mbuf, from: &Mbuf, wait: i32) -> Result<(), Errno> {
    kassert!(from.m_flags().get() & M_PKTHDR != 0);

    to.m_flags().set(to.m_flags().get() & (M_EXT | M_EXTWR));
    to.m_flags()
        .set(to.m_flags().get() | (from.m_flags().get() & M_COPYFLAGS));
    to.m_pkthdr_assign(from);

    to.m_pkthdr().pf.statekey.set(core::ptr::null_mut());
    if let Some(sk) = pf_mbuf_statekey(from) {
        pf_mbuf_link_state_key(to, sk);
    }
    to.m_pkthdr().pf.inp.set(core::ptr::null_mut());
    pf_mbuf_link_inpcb(to, pf_mbuf_inp(from));

    to.m_pkthdr().ph_tags.init();

    m_tag_copy_chain(to, from, wait)?;

    if to.m_flags().get() & M_EXT == 0 {
        to.m_data().set(to.m_pktdat());
    }

    Ok(())
}

/// `m_dup_pkt`: a copy of packet `m0` in one mbuf (or cluster), its data starting `adj` bytes
/// in.
pub fn m_dup_pkt(m0: &Mbuf, adj: u32, wait: i32) -> Option<&'static Mbuf> {
    kassert!(m0.m_flags().get() & M_PKTHDR != 0);

    let len = m0.m_pkthdr().len.get() + adj as i32;
    if len > MAXMCLBYTES as i32 {
        // XXX
        return None;
    }

    let m = m_get(wait, i32::from(m0.m_type().get()))?;

    let ok = 'fail: {
        if m_dup_pkthdr(m, m0, wait).is_err() {
            break 'fail false;
        }

        if len > MHLEN as i32 {
            let _ = mclgetl(m, wait, len as u32);
            if m.m_flags().get() & M_EXT == 0 {
                break 'fail false;
            }
        }
        true
    };
    if !ok {
        m_freem(m);
        return None;
    }

    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len);
    m_adj(m, adj as i32);
    let pktlen = m0.m_pkthdr().len.get() as usize;
    // SAFETY: after the adjustment `m` has `len - adj` = `pktlen` bytes at `m_data`.
    m_copydata(m0, 0, unsafe {
        slice::from_raw_parts_mut(mtod::<u8>(m), pktlen)
    });

    Some(m)
}

/// `m_microtime`: the packet's timestamp, or the current time when it has none.
pub fn m_microtime(m: &Mbuf) -> Timeval {
    if m.m_pkthdr().csum_flags.get() & M_TIMESTAMP != 0 {
        let utv = nsec_to_timeval(m.m_pkthdr().ph_timestamp.get() as u64);
        let btv = microboottime();
        timeradd(&btv, &utv)
    } else {
        microtime()
    }
}

/// `m_pool_alloc`: a page from `pool_allocator_multi`, charged against the mbuf memory limit.
pub fn m_pool_alloc(pp: &Pool, flags: i32, slowdown: &mut i32) -> Option<NonNull<u8>> {
    let pgsize = u64::from(pp.pr_pgsize.get());

    if MBUF_MEM_ALLOC.fetch_add(pgsize, Ordering::Relaxed) + pgsize
        <= MBUF_MEM_LIMIT.load(Ordering::Relaxed)
    {
        let v = (POOL_ALLOCATOR_MULTI.pa_alloc)(pp, flags, slowdown);
        if v.is_some() {
            return v;
        }
    }

    // fail:
    mbstat_inc(MbstatCounters::MbsDrops as usize);
    MBUF_MEM_ALLOC.fetch_sub(pgsize, Ordering::Relaxed);
    None
}

/// `m_pool_free`: a page back to `pool_allocator_multi`, uncharged.
pub fn m_pool_free(pp: &Pool, v: NonNull<u8>) {
    (POOL_ALLOCATOR_MULTI.pa_free)(pp, v);

    MBUF_MEM_ALLOC.fetch_sub(u64::from(pp.pr_pgsize.get()), Ordering::Relaxed);
}

/// `m_pool_init`: an mbuf or cluster pool, DMA-reachable.
pub fn m_pool_init(pp: &'static Pool, size: u32, align: u32, wmesg: &'static str) {
    pool_init(
        pp,
        size as usize,
        align,
        IPL_NET,
        0,
        wmesg,
        Some(&M_POOL_ALLOCATOR),
    );
    pool_set_constraints(pp, &KP_DMA_CONTIG);
}

/// `m_pool_noconstraints`: lets the mbuf and cluster pools use all of memory.
pub fn m_pool_noconstraints() {
    pool_set_constraints(&MBPOOL, &KP_MBUF_CONTIG);

    for pp in MCLPOOLS.iter() {
        pool_set_constraints(pp, &KP_MBUF_CONTIG);
    }
}

/// `m_pool_used`: the mbuf memory in use, in percent of the limit.
pub fn m_pool_used() -> u32 {
    ((MBUF_MEM_ALLOC.load(Ordering::Relaxed) * 100) / MBUF_MEM_LIMIT.load(Ordering::Relaxed)) as u32
}

/// `mbuf_dma_64bit_enable`: lifts the DMA constraint when every interface can reach all of
/// memory (see the module's deviations).
pub fn mbuf_dma_64bit_enable() {
    for ifp in crate::net::if_::IFNETLIST.0.iter() {
        if ifp.if_xflags.get() & crate::net::if_::IFXF_MBUF_64BIT == 0 {
            printf(format_args!(
                "{}: restrict all mbufs to low memory\n",
                crate::kern::subr_prf::Str(&ifp.if_xname.get())
            ));
            return;
        }
    }

    printf(format_args!("enable mbufs in high memory\n"));
    m_pool_noconstraints();
}

/// The address of an optional mbuf, for `%p`.
fn mptr(m: Option<&Mbuf>) -> *const Mbuf {
    m.map_or(ptr::null(), ptr::from_ref)
}

/// `m_print` (`DDB`): prints one mbuf.
pub fn m_print(m: &Mbuf, pr: PrFn) {
    pr(format_args!("mbuf {:p}\n", m));
    pr(format_args!(
        "m_type: {}\tm_flags: {}\n",
        m.m_type().get(),
        Bitmask(u64::from(m.m_flags().get()), M_BITS)
    ));
    pr(format_args!(
        "m_next: {:p}\tm_nextpkt: {:p}\n",
        mptr(m.m_next().get()),
        mptr(m.m_nextpkt().get())
    ));
    pr(format_args!(
        "m_data: {:p}\tm_len: {}\n",
        m.m_data().get(),
        m.m_len().get()
    ));
    pr(format_args!(
        "m_dat: {:p}\tm_pktdat: {:p}\n",
        m.m_dat(),
        m.m_pktdat()
    ));
    if m.m_flags().get() & M_PKTHDR != 0 {
        let ph = m.m_pkthdr();
        pr(format_args!(
            "m_ptkhdr.ph_ifidx: {}\tm_pkthdr.len: {}\n",
            ph.ph_ifidx.get(),
            ph.len.get()
        ));
        pr(format_args!(
            "m_ptkhdr.ph_tags: {:p}\tm_pkthdr.ph_tagsset: {}\n",
            ph.ph_tags.first().map_or(ptr::null(), ptr::from_ref),
            Bitmask(u64::from(ph.ph_tagsset.get()), MTAG_BITS)
        ));
        pr(format_args!(
            "m_pkthdr.ph_flowid: {}\tm_pkthdr.ph_loopcnt: {}\n",
            ph.ph_flowid.get(),
            ph.ph_loopcnt.get()
        ));
        pr(format_args!(
            "m_pkthdr.csum_flags: {}\n",
            Bitmask(u64::from(ph.csum_flags.get()), MCS_BITS)
        ));
        pr(format_args!(
            "m_pkthdr.ether_vtag: {}\tm_ptkhdr.ph_rtableid: {}\n",
            ph.ether_vtag.get(),
            ph.ph_rtableid.get()
        ));
        pr(format_args!(
            "m_pkthdr.pf.statekey: {:p}\tm_pkthdr.pf.inp {:p}\n",
            ph.pf.statekey.get(),
            ph.pf.inp.get()
        ));
        pr(format_args!(
            "m_pkthdr.pf.qid: {}\tm_pkthdr.pf.tag: {}\n",
            ph.pf.qid.get(),
            ph.pf.tag.get()
        ));
        pr(format_args!(
            "m_pkthdr.pf.flags: {}\n",
            Bitmask(u64::from(ph.pf.flags.get()), MPF_BITS)
        ));
        pr(format_args!(
            "m_pkthdr.pf.routed: {}\tm_pkthdr.pf.prio: {}\n",
            ph.pf.routed.get(),
            ph.pf.prio.get()
        ));
    }
    if m.m_flags().get() & M_EXT != 0 {
        pr(format_args!(
            "m_ext.ext_buf: {:p}\tm_ext.ext_size: {}\n",
            m.m_ext().ext_buf.get(),
            m.m_ext().ext_size.get()
        ));
        pr(format_args!(
            "m_ext.ext_free_fn: {}\tm_ext.ext_arg: {:p}\n",
            m.m_ext().ext_free_fn.get(),
            m.m_ext().ext_arg.get()
        ));
        // if m_ext.ext_free_fn == m_extfree_refs_fn ?
    }
}

/// `m_print_chain` (`DDB`): one line per mbuf of a chain and a total.
pub fn m_print_chain(v: Option<&Mbuf>, deep: bool, pr: PrFn) {
    let mut indent = if deep { "++-" } else { "-+-" };
    let (mut chain, mut len, mut size) = (0usize, 0usize, 0usize);

    let mut m = v;
    while let Some(mm) = m {
        chain += 1;
        len += mm.m_len().get() as usize;
        size += m_size(mm) as usize;
        let t = mm.m_type().get();
        let type_ = if t >= 0 && (t as usize) < MT_NTYPES {
            M_TYPES[t as usize]
        } else {
            "???"
        };
        pr(format_args!(
            "{} mbuf {:p}, {}, off {}, len {}",
            indent,
            mm,
            type_,
            mm.m_data().get() as isize - m_databuf(mm) as isize,
            mm.m_len().get()
        ));
        if mm.m_flags().get() & M_PKTHDR != 0 {
            pr(format_args!(", pktlen {}", mm.m_pkthdr().len.get()));
        }
        if mm.m_flags().get() & M_EXT != 0 {
            pr(format_args!(", clsize {}", mm.m_ext().ext_size.get()));
        } else {
            pr(format_args!(
                ", size {}",
                if mm.m_flags().get() & M_PKTHDR != 0 {
                    MHLEN
                } else {
                    MLEN
                }
            ));
        }
        pr(format_args!("\n"));
        indent = if deep { "|+-" } else { " +-" };
        m = mm.m_next().get();
    }
    indent = if deep { "|\\-" } else { " \\-" };
    if v.is_some() {
        pr(format_args!(
            "{} total chain {}, len {}, size {}\n",
            indent, chain, len, size
        ));
    }
}

/// `m_print_packet` (`DDB`): one line per packet of a list (or every chain, when `deep`).
pub fn m_print_packet(v: Option<&Mbuf>, deep: bool, pr: PrFn) {
    let mut indent = "+--";
    let mut pkts = 0usize;

    let mut m = v;
    while let Some(mm) = m {
        let (mut chain, mut len, mut size) = (0usize, 0usize, 0usize);

        pkts += 1;
        if deep {
            m_print_chain(Some(mm), deep, pr);
            m = mm.m_nextpkt().get();
            continue;
        }
        let mut n = Some(mm);
        while let Some(nn) = n {
            chain += 1;
            len += nn.m_len().get() as usize;
            size += m_size(nn) as usize;
            n = nn.m_next().get();
        }
        pr(format_args!("{} mbuf {:p}, chain {}", indent, mm, chain));
        if mm.m_flags().get() & M_PKTHDR != 0 {
            pr(format_args!(", pktlen {}", mm.m_pkthdr().len.get()));
        }
        pr(format_args!(", len {}, size {}\n", len, size));
        m = mm.m_nextpkt().get();
    }
    indent = "\\--";
    if v.is_some() {
        pr(format_args!("{} total packets {}\n", indent, pkts));
    }
}

// mbuf lists

/// `ml_init`.
pub fn ml_init(ml: &MbufList) {
    ml.ml_head.set(None);
    ml.ml_tail.set(None);
    ml.ml_len.store(0, Ordering::Relaxed);
}

/// `ml_enqueue`: appends the packet `m`.
pub fn ml_enqueue(ml: &MbufList, m: &'static Mbuf) {
    match ml.ml_tail.get() {
        None => {
            ml.ml_head.set(Some(m));
            ml.ml_tail.set(Some(m));
        }
        Some(tail) => {
            tail.m_nextpkt().set(Some(m));
            ml.ml_tail.set(Some(m));
        }
    }

    m.m_nextpkt().set(None);
    ml.ml_len.fetch_add(1, Ordering::Relaxed);
}

/// `ml_enlist`: appends all of `mlb` to `mla` and empties `mlb`.
pub fn ml_enlist(mla: &MbufList, mlb: &MbufList) {
    if !ml_empty(mlb) {
        if ml_empty(mla) {
            mla.ml_head.set(mlb.ml_head.get());
        } else if let Some(tail) = mla.ml_tail.get() {
            tail.m_nextpkt().set(mlb.ml_head.get());
        }
        mla.ml_tail.set(mlb.ml_tail.get());
        mla.ml_len.fetch_add(ml_len(mlb), Ordering::Relaxed);

        ml_init(mlb);
    }
}

/// `ml_dequeue`: takes the first packet.
pub fn ml_dequeue(ml: &MbufList) -> Option<&'static Mbuf> {
    let m = ml.ml_head.get();
    if let Some(m) = m {
        ml.ml_head.set(m.m_nextpkt().get());
        if ml.ml_head.get().is_none() {
            ml.ml_tail.set(None);
        }

        m.m_nextpkt().set(None);
        ml.ml_len.fetch_sub(1, Ordering::Relaxed);
    }

    m
}

/// `ml_dechain`: takes all packets, still linked through `m_nextpkt`.
pub fn ml_dechain(ml: &MbufList) -> Option<&'static Mbuf> {
    let m0 = ml.ml_head.get();

    ml_init(ml);

    m0
}

/// `ml_purge`: frees every packet; returns how many there were.
pub fn ml_purge(ml: &MbufList) -> u32 {
    let mut m = ml.ml_head.get();
    while let Some(mm) = m {
        let n = mm.m_nextpkt().get();
        m_freem(mm);
        m = n;
    }

    let len = ml_len(ml);
    ml_init(ml);

    len
}

/// `ml_hdatalen`: the length of the first packet.
pub fn ml_hdatalen(ml: &MbufList) -> u32 {
    let Some(m) = ml.ml_head.get() else {
        return 0;
    };

    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    m.m_pkthdr().len.get() as u32
}

// mbuf queues

/// `mq_init`.
pub fn mq_init(mq: &MbufQueue, maxlen: u32, ipl: i32) {
    mtx_init(&mq.mq_mtx, ipl);
    ml_init(&mq.mq_list);
    mq.mq_maxlen.store(maxlen, Ordering::Relaxed);
}

/// `mq_push`: enqueues `m`, dropping the oldest packet when full; `true` when one was dropped.
pub fn mq_push(mq: &MbufQueue, m: &'static Mbuf) -> bool {
    let mut dropped = None;

    mtx_enter(&mq.mq_mtx);
    if mq_len(mq) >= mq.mq_maxlen.load(Ordering::Relaxed) {
        mq.mq_drops.fetch_add(1, Ordering::Relaxed);
        dropped = ml_dequeue(&mq.mq_list);
    }
    ml_enqueue(&mq.mq_list, m);
    mtx_leave(&mq.mq_mtx);

    if let Some(dropped) = dropped {
        m_freem(dropped);
    }

    dropped.is_some()
}

/// `mq_enqueue`: enqueues `m`, or drops it when full; `true` when it was dropped.
pub fn mq_enqueue(mq: &MbufQueue, m: &'static Mbuf) -> bool {
    let mut dropped = false;

    mtx_enter(&mq.mq_mtx);
    if mq_len(mq) < mq.mq_maxlen.load(Ordering::Relaxed) {
        ml_enqueue(&mq.mq_list, m);
    } else {
        mq.mq_drops.fetch_add(1, Ordering::Relaxed);
        dropped = true;
    }
    mtx_leave(&mq.mq_mtx);

    if dropped {
        m_freem(m);
    }

    dropped
}

/// `mq_dequeue`.
pub fn mq_dequeue(mq: &MbufQueue) -> Option<&'static Mbuf> {
    mtx_enter(&mq.mq_mtx);
    let m = ml_dequeue(&mq.mq_list);
    mtx_leave(&mq.mq_mtx);

    m
}

/// `mq_enlist`: appends `ml`, or drops all of it when the queue is full; returns how many
/// packets were dropped.
pub fn mq_enlist(mq: &MbufQueue, ml: &MbufList) -> u32 {
    let mut dropped = 0;

    mtx_enter(&mq.mq_mtx);
    if mq_len(mq) < mq.mq_maxlen.load(Ordering::Relaxed) {
        ml_enlist(&mq.mq_list, ml);
    } else {
        dropped = ml_len(ml);
        mq.mq_drops.fetch_add(dropped, Ordering::Relaxed);
    }
    mtx_leave(&mq.mq_mtx);

    if dropped != 0 {
        while let Some(m) = ml_dequeue(ml) {
            m_freem(m);
        }
    }

    dropped
}

/// `mq_delist`: moves the whole queue to `ml`.
pub fn mq_delist(mq: &MbufQueue, ml: &MbufList) {
    mtx_enter(&mq.mq_mtx);
    // *ml = mq->mq_list
    ml.ml_head.set(mq.mq_list.ml_head.get());
    ml.ml_tail.set(mq.mq_list.ml_tail.get());
    ml.ml_len.store(ml_len(&mq.mq_list), Ordering::Relaxed);
    ml_init(&mq.mq_list);
    mtx_leave(&mq.mq_mtx);
}

/// `mq_purge`: frees every queued packet; returns how many there were.
pub fn mq_purge(mq: &MbufQueue) -> u32 {
    let ml = MbufList::new();

    mq_delist(mq, &ml);

    ml_purge(&ml)
}

/// `mq_hdatalen`: the length of the first queued packet.
pub fn mq_hdatalen(mq: &MbufQueue) -> u32 {
    mtx_enter(&mq.mq_mtx);
    let hdatalen = ml_hdatalen(&mq.mq_list);
    mtx_leave(&mq.mq_mtx);

    hdatalen
}

/// `mq_set_maxlen`.
pub fn mq_set_maxlen(mq: &MbufQueue, maxlen: u32) {
    mtx_enter(&mq.mq_mtx);
    mq.mq_maxlen.store(maxlen, Ordering::Relaxed);
    mtx_leave(&mq.mq_mtx);
}

/// `sysctl_mq` (`!SMALL_KERNEL`): the `IFQCTL_*` nodes of a queue. `oldp` and `newp` are user
/// addresses, 0 for NULL (`kern/kern_sysctl.rs`).
pub fn sysctl_mq(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    mq: &MbufQueue,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [mib] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *mib {
        IFQCTL_LEN => sysctl_rdint(oldp, oldlenp, newp, mq_len(mq) as i32),
        IFQCTL_MAXLEN => {
            let oldval = mq.mq_maxlen.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval as i32);
            let error = sysctl_int(oldp, oldlenp, newp, newlen, &newval);
            let newval = newval.into_inner() as u32;
            if error.is_ok() && oldval != newval {
                mq_set_maxlen(mq, newval);
            }
            error
        }
        IFQCTL_DROPS => sysctl_rdint(oldp, oldlenp, newp, mq_drops(mq) as i32),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

const _: () = {
    // mclsizes[0] must be MCLBYTES and the last MAXMCLBYTES (mbinit's DIAGNOSTIC checks).
    assert!(MCLSIZES[0] == MCLBYTES as u32);
    assert!(MCLSIZES[MCLSIZES.len() - 1] == MAXMCLBYTES as u32);
    // M_COPYALL is an int in C.
    assert!(M_COPYALL > 0);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the mbuf allocator, the chain operations, `mbuf_list`/`mbuf_queue` and the
    // packet tags (`uipc_mbuf2.rs`), over real memory (see `subr_pool.rs` for the setup).

    use std::string::String;
    use std::sync::{Mutex as StdMutex, MutexGuard};
    use std::vec::Vec;
    use std::{assert, assert_eq, format, vec};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::kern::uipc_mbuf2::{
        m_pulldown, m_tag_delete, m_tag_find, m_tag_first, m_tag_get, m_tag_next, m_tag_prepend,
    };
    use crate::sys::mbuf::{
        MT_HEADER, PACKET_TAG_DLT, PACKET_TAG_GRE, PACKET_TAG_TUNNEL, ml_empty, mq_empty, mq_full,
    };

    /// Real memory, the console (so that a kernel panic in a test says why) and a fresh `mbinit`.
    pub(crate) fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        mbinit_again();
        guard
    }

    /// `mbinit` over memory a test has just reset (`setup_real_memory`), for the tests of other
    /// files that need mbufs.
    pub(crate) fn mbinit_again() {
        // mbinit registers its two free functions again.
        NUM_EXTFREE_FNS.store(0, Ordering::Relaxed);
        mbinit();
        // The host's test memory lies above amd64's 4 GiB DMA constraint, which the host double
        // mirrors; lift it as mbuf_dma_64bit_enable would.
        m_pool_noconstraints();
    }

    /// The bytes the tests write: position `i` holds a value that differs from its neighbours.
    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + 3) as u8).collect()
    }

    /// The chain's data length.
    fn chain_len(m: &Mbuf) -> usize {
        let mut len = 0;
        let mut n = Some(m);
        while let Some(nn) = n {
            len += nn.m_len().get() as usize;
            n = nn.m_next().get();
        }
        len
    }

    /// The number of mbufs in the chain.
    fn chain_count(m: &Mbuf) -> usize {
        let mut count = 0;
        let mut n = Some(m);
        while let Some(nn) = n {
            count += 1;
            n = nn.m_next().get();
        }
        count
    }

    /// The chain's data.
    fn contents(m: &Mbuf) -> Vec<u8> {
        let mut v = vec![0u8; chain_len(m)];
        m_copydata(m, 0, &mut v);
        v
    }

    /// A packet of mbufs holding `lens` bytes each, filled with `pattern`.
    fn packet(lens: &[usize]) -> &'static Mbuf {
        let data = pattern(lens.iter().sum());
        let mut off = 0;
        let mut top: Option<&'static Mbuf> = None;
        let mut last: Option<&'static Mbuf> = None;
        for &len in lens {
            let m = if top.is_none() {
                m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf")
            } else {
                m_get(M_DONTWAIT, MT_DATA).expect("an mbuf")
            };
            if len > m_trailingspace(m) as usize {
                assert!(mclgetl(m, M_DONTWAIT, len as u32).is_some(), "a cluster");
            }
            m.m_len().set(len as u32);
            m_copyback(m, 0, &data[off..off + len], M_DONTWAIT).expect("copyback");
            off += len;
            match last {
                None => top = Some(m),
                Some(last) => last.m_next().set(Some(m)),
            }
            last = Some(m);
        }
        let top = top.expect("at least one mbuf");
        top.m_pkthdr().len.set(off as i32);
        top
    }

    #[test]
    fn get_gethdr_and_free() {
        let _g = setup();

        let m = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
        assert_eq!(m.m_data().get(), m.m_dat());
        assert_eq!(m.m_flags().get(), 0);
        assert_eq!(m.m_len().get(), 0);
        assert_eq!(i32::from(m.m_type().get()), MT_DATA);
        assert_eq!(m_leadingspace(m), 0);
        assert_eq!(m_trailingspace(m), MLEN as i32);

        let h = m_gethdr(M_WAIT, MT_HEADER).expect("a header mbuf");
        assert_eq!(h.m_flags().get(), M_PKTHDR);
        assert_eq!(h.m_data().get(), h.m_pktdat());
        assert_eq!(h.m_pkthdr().pf.prio.get(), IFQ_DEFPRIO);
        assert_eq!(h.m_pkthdr().len.get(), 0);
        assert!(h.m_pkthdr().ph_tags.first().is_none());
        assert_eq!(m_trailingspace(h), MHLEN as i32);
        assert_eq!(MBPOOL.pr_nout.get(), 2);

        h.m_next().set(Some(m));
        assert!(ptr::eq(m_free(h).expect("the next mbuf"), m));
        assert!(m_free(m).is_none());
        assert!(m_free(None).is_none());
        assert_eq!(MBPOOL.pr_nout.get(), 0);

        let c = m_getclr(M_DONTWAIT, MT_DATA).expect("a zeroed mbuf");
        c.m_len().set(MLEN as u32);
        assert_eq!(contents(c), vec![0u8; MLEN]);
        m_freem(c);
    }

    #[test]
    fn cluster_pools_have_the_c_names_and_sizes() {
        let _g = setup();

        let names: Vec<&str> = (0..MCLSIZES.len()).map(mclname).collect();
        assert_eq!(
            names,
            [
                "mcl2k", "mcl2k2", "mcl4k", "mcl8k", "mcl9k128", "mcl12k", "mcl16k", "mcl64k"
            ]
        );
        for (pp, &size) in MCLPOOLS.iter().zip(MCLSIZES.iter()) {
            assert!(pp.pr_size.get() >= size);
        }
        assert!(ptr::eq(m_clpool(1500).expect("a pool"), &MCLPOOLS[0]));
        assert!(ptr::eq(m_clpool(2049).expect("a pool"), &MCLPOOLS[1]));
        assert!(ptr::eq(m_clpool(9000).expect("a pool"), &MCLPOOLS[4]));
        assert!(m_clpool(64 * 1024 + 1).is_none());
        assert_eq!(nmbclust_update(0), Err(Errno::ERANGE));
        // Raise the limit rather than lower it: tests running at the same time allocate mbufs,
        // and a limit below what they hold would fail their allocations.
        let raised = 2 * crate::sys::param::NMBCLUSTERS as i64;
        assert_eq!(nmbclust_update(raised), Ok(()));
        assert_eq!(
            MBUF_MEM_LIMIT.load(Ordering::Relaxed),
            raised as u64 * MCLBYTES as u64
        );
        nmbclust_update(crate::sys::param::NMBCLUSTERS as i64).expect("the default limit");
    }

    #[test]
    fn clusters_come_and_go() {
        let _g = setup();

        let m = m_clget(None, M_DONTWAIT, 9000).expect("a cluster packet");
        assert_eq!(
            m.m_flags().get() & (M_EXT | M_EXTWR | M_PKTHDR),
            M_EXT | M_EXTWR | M_PKTHDR
        );
        assert_eq!(m.m_ext().ext_size.get(), MCLPOOLS[4].pr_size.get());
        assert_eq!(m.m_data().get(), m.m_ext().ext_buf.get());
        assert!(!m_readonly(m));
        assert_eq!(MCLPOOLS[4].pr_nout.get(), 1);
        assert!(MBUF_MEM_ALLOC.load(Ordering::Relaxed) > 0);
        assert!(m_pool_used() < 100);
        m_freem(m);
        assert_eq!(MCLPOOLS[4].pr_nout.get(), 0);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn copydata_and_copyback_round_trip() {
        let _g = setup();

        let m = packet(&[100, 50, 200]);
        let data = pattern(350);
        assert_eq!(contents(m), data);

        let mut part = vec![0u8; 100];
        m_copydata(m, 120, &mut part);
        assert_eq!(part, &data[120..220]);

        // across the first two mbufs
        let patch: Vec<u8> = (0..40).map(|i| 200 + i as u8).collect();
        m_copyback(m, 90, &patch, M_DONTWAIT).expect("copyback");
        let mut want = data.clone();
        want[90..130].copy_from_slice(&patch);
        assert_eq!(contents(m), want);
        assert_eq!(m.m_pkthdr().len.get(), 350);

        // past the end: the gap is zeroed and the packet grows
        let tail = pattern(300);
        m_copyback(m, 360, &tail, M_DONTWAIT).expect("copyback past the end");
        want.extend_from_slice(&[0u8; 10]);
        want.extend_from_slice(&tail);
        assert_eq!(contents(m), want);
        assert_eq!(m.m_pkthdr().len.get(), 660);
        m_calchdrlen(m);
        assert_eq!(m.m_pkthdr().len.get(), 660);

        m_freem(m);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
        assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
    }

    #[test]
    fn adj_trims_the_head_and_the_tail() {
        let _g = setup();

        let m = packet(&[100, 50, 200]);
        let data = pattern(350);

        m_adj(m, 120);
        assert_eq!(m.m_pkthdr().len.get(), 230);
        assert_eq!(m.m_len().get(), 0);
        assert_eq!(m.m_next().get().expect("second").m_len().get(), 30);
        assert_eq!(contents(m), &data[120..]);

        // within the last mbuf only
        m_adj(m, -10);
        assert_eq!(m.m_pkthdr().len.get(), 220);
        assert_eq!(contents(m), &data[120..340]);

        // across mbufs
        m_adj(m, -200);
        assert_eq!(m.m_pkthdr().len.get(), 20);
        assert_eq!(contents(m), &data[120..140]);

        m_adj(None, 5);
        m_freem(m);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn pullup_makes_the_head_contiguous() {
        let _g = setup();

        let data = pattern(260);
        let m = packet(&[10, 50, 200]);
        let m = m_pullup(m, 40).expect("pulled up");
        assert!(m.m_len().get() >= 40);
        assert_eq!(m.m_flags().get() & M_PKTHDR, M_PKTHDR);
        assert_eq!(m.m_pkthdr().len.get(), 260);
        assert_eq!(contents(m), data);

        // more than a header mbuf holds: a new mbuf with a cluster leads the chain
        let m = m_pullup(m, 200).expect("pulled up into a cluster");
        assert!(m.m_len().get() >= 200);
        assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
        assert_eq!(m.m_pkthdr().len.get(), 260);
        assert_eq!(contents(m), data);

        // longer than the chain: the chain is freed
        assert!(m_pullup(m, 1000).is_none());
        assert_eq!(MBPOOL.pr_nout.get(), 0);
        assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
    }

    #[test]
    fn split_plain_and_shared_clusters() {
        let _g = setup();

        let data = pattern(350);
        let m = packet(&[100, 50, 200]);
        let tail = m_split(m, 120, M_DONTWAIT).expect("a tail");
        assert_eq!(m.m_pkthdr().len.get(), 120);
        assert_eq!(contents(m), &data[..120]);
        assert_eq!(tail.m_flags().get() & M_PKTHDR, M_PKTHDR);
        assert_eq!(tail.m_pkthdr().len.get(), 230);
        assert_eq!(contents(tail), &data[120..]);
        m_freem(m);
        m_freem(tail);

        // a cluster is shared, not copied
        let data = pattern(1000);
        let m = packet(&[1000]);
        assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
        let tail = m_split(m, 300, M_DONTWAIT).expect("a tail");
        assert_eq!(tail.m_flags().get() & M_PKTHDR, M_PKTHDR);
        // the header mbuf of the tail points into the same cluster
        let shared = tail;
        assert_eq!(chain_count(tail), 1);
        assert_eq!(shared.m_ext().ext_buf.get(), m.m_ext().ext_buf.get());
        assert_eq!(
            shared.m_data().get() as usize - m.m_data().get() as usize,
            300
        );
        assert!(m_readonly(m) && m_readonly(shared));
        assert_eq!(m_trailingspace(m), 0);
        assert_eq!(contents(m), &data[..300]);
        assert_eq!(contents(tail), &data[300..]);
        assert_eq!(MCLPOOLS[0].pr_nout.get(), 1);
        m_freem(m);
        assert_eq!(
            MCLPOOLS[0].pr_nout.get(),
            1,
            "the tail still holds the cluster"
        );
        assert!(!m_readonly(shared), "the last reference is writable again");
        m_freem(tail);
        assert_eq!(MCLPOOLS[0].pr_nout.get(), 0);
        assert_eq!(M_EXT_REFS_POOL.pr_nout.get(), 0);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn prepend_uses_leading_space_then_a_new_mbuf() {
        let _g = setup();

        let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
        m_align(m, 20);
        m.m_len().set(20);
        m_copyback(m, 0, &pattern(20), M_DONTWAIT).expect("copyback");
        m.m_pkthdr().len.set(20);
        let room = m_leadingspace(m);
        assert_eq!(room, (MHLEN - 24) as i32);

        let m = m_prepend(m, 8, M_DONTWAIT).expect("prepended in place");
        assert_eq!(chain_count(m), 1);
        assert_eq!(m.m_len().get(), 28);
        assert_eq!(m.m_pkthdr().len.get(), 28);

        let big = m_leadingspace(m) + 1;
        let first = m;
        let m = m_prepend(m, big, M_DONTWAIT).expect("prepended in a new mbuf");
        assert!(!ptr::eq(m, first));
        assert!(ptr::eq(m.m_next().get().expect("the old head"), first));
        assert_eq!(m.m_flags().get() & M_PKTHDR, M_PKTHDR);
        assert_eq!(first.m_flags().get() & M_PKTHDR, 0);
        assert_eq!(m.m_len().get(), big as u32);
        assert_eq!(m.m_pkthdr().len.get(), 28 + big);
        let mut tail = vec![0u8; 20];
        m_copydata(m, 8 + big, &mut tail);
        assert_eq!(tail, pattern(20));
        m_freem(m);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn cat_copies_small_data_and_links_large() {
        let _g = setup();

        let a = packet(&[30]);
        let b = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
        b.m_len().set(10);
        m_copyback(b, 0, &[9u8; 10], M_DONTWAIT).expect("copyback");
        m_cat(a, Some(b));
        assert_eq!(chain_count(a), 1, "the data was copied, b freed");
        let mut want = pattern(30);
        want.extend_from_slice(&[9u8; 10]);
        assert_eq!(contents(a), want);

        let big = packet(&[200]);
        m_removehdr(big);
        m_cat(a, Some(big));
        assert_eq!(chain_count(a), 2, "too big to copy: linked");
        m_freem(a);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn copym_copies_and_shares() {
        let _g = setup();

        let data = pattern(1250);
        let m = packet(&[100, 150, 1000]);
        let copy = m_copym(m, 0, M_COPYALL, M_DONTWAIT).expect("a copy");
        assert_eq!(copy.m_flags().get() & M_PKTHDR, M_PKTHDR);
        assert_eq!(copy.m_pkthdr().len.get(), 1250);
        assert_eq!(contents(copy), data);
        assert_eq!(MCLPOOLS[0].pr_nout.get(), 1, "the cluster is shared");

        let part = m_copym(m, 50, 200, M_DONTWAIT).expect("a partial copy");
        assert_eq!(part.m_flags().get() & M_PKTHDR, 0);
        assert_eq!(contents(part), &data[50..250]);

        m_freem(m);
        m_freem(part);
        assert_eq!(MCLPOOLS[0].pr_nout.get(), 1);
        m_freem(copy);
        assert_eq!(MCLPOOLS[0].pr_nout.get(), 0);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn defrag_and_dup_pkt() {
        let _g = setup();

        let data = pattern(600);
        let m = packet(&[100, 300, 200]);
        m_defrag(m, M_DONTWAIT).expect("defragmented");
        assert_eq!(chain_count(m), 1);
        assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
        assert_eq!(m.m_pkthdr().len.get(), 600);
        assert_eq!(contents(m), data);

        let d = m_dup_pkt(m, 2, M_DONTWAIT).expect("a duplicate");
        assert_eq!(contents(d), data);
        assert_eq!(d.m_pkthdr().len.get(), 600);
        assert_eq!(
            d.m_data().get() as usize - d.m_ext().ext_buf.get() as usize,
            2
        );

        m_freem(m);
        m_freem(d);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
        assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
    }

    #[test]
    fn getptr_apply_and_devget() {
        let _g = setup();

        let data = pattern(350);
        let m = packet(&[100, 50, 200]);
        let (n, off) = m_getptr(m, 120).expect("inside");
        assert_eq!(n.m_len().get(), 50);
        assert_eq!(off, 20);
        let (n, off) = m_getptr(m, 350).expect("the end");
        assert_eq!((n.m_len().get(), off), (200, 200));
        assert!(m_getptr(m, 351).is_none());

        let mut sum = 0u64;
        m_apply(m, 90, 100, |b| {
            sum += b.iter().map(|&x| u64::from(x)).sum::<u64>();
            Ok(())
        })
        .expect("applied");
        assert_eq!(
            sum,
            data[90..190].iter().map(|&x| u64::from(x)).sum::<u64>()
        );
        assert_eq!(
            m_apply(m, 0, 10, |_| Err(Errno::EINVAL)),
            Err(Errno::EINVAL)
        );
        m_freem(m);

        MAX_LINKHDR.store(16, Ordering::Relaxed);
        let small = m_devget(&data[..60], 0).expect("a small packet");
        assert_eq!(small.m_pkthdr().len.get(), 60);
        assert_eq!(m_leadingspace(small), 16);
        assert_eq!(contents(small), &data[..60]);
        m_freem(small);

        let frame: Vec<u8> = pattern(1514);
        let big = m_devget(&frame, 2).expect("a cluster packet");
        assert_eq!(big.m_flags().get() & M_EXT, M_EXT);
        assert_eq!(contents(big), frame);
        m_freem(big);
        MAX_LINKHDR.store(0, Ordering::Relaxed);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn pulldown_and_makespace() {
        let _g = setup();

        let data = pattern(260);
        let m = packet(&[10, 50, 200]);
        let n = m_pulldown(m, 5, 30, None).expect("pulled down");
        assert!(n.m_len().get() >= 30);
        let mut got = vec![0u8; 30];
        m_copydata(n, 0, &mut got);
        assert_eq!(got, &data[5..35]);
        assert_eq!(contents(m), data);

        let mut off = -1;
        let n = m_pulldown(m, 100, 20, Some(&mut off)).expect("already contiguous");
        assert!(n.m_len().get() as i32 - off >= 20);
        m_copydata(n, off, &mut got[..20]);
        assert_eq!(&got[..20], &data[100..120]);
        m_freem(m);

        let m = packet(&[40]);
        let (n, off) = m_makespace(m, 20, 8).expect("space");
        assert!(ptr::eq(n, m));
        assert_eq!(m.m_pkthdr().len.get(), 48);
        let all = contents(m);
        assert_eq!(&all[..off as usize], &pattern(40)[..20]);
        assert_eq!(&all[off as usize + 8..], &pattern(40)[20..]);
        m_freem(m);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    #[test]
    fn mbuf_lists_and_queues() {
        let _g = setup();

        let ml = MbufList::new();
        assert!(ml_empty(&ml));
        let pkts: Vec<&'static Mbuf> = (0..3).map(|_| packet(&[10])).collect();
        for &p in &pkts {
            ml_enqueue(&ml, p);
        }
        assert_eq!(ml_len(&ml), 3);
        assert_eq!(ml_hdatalen(&ml), 10);
        assert_eq!(ml.iter().count(), 3);
        assert!(ptr::eq(ml_dequeue(&ml).expect("first"), pkts[0]));
        assert_eq!(ml_len(&ml), 2);

        let other = MbufList::new();
        ml_enqueue(&other, pkts[0]);
        ml_enlist(&ml, &other);
        assert!(ml_empty(&other));
        let order: Vec<*const Mbuf> = ml.iter().map(ptr::from_ref).collect();
        assert_eq!(
            order,
            [pkts[1], pkts[2], pkts[0]].map(ptr::from_ref).to_vec()
        );
        assert_eq!(ml_purge(&ml), 3);
        assert!(ml_empty(&ml));
        assert_eq!(MBPOOL.pr_nout.get(), 0);

        let mq = MbufQueue::new(0, IPL_NET);
        mq_init(&mq, 2, IPL_NET);
        assert!(mq_empty(&mq));
        assert!(!mq_enqueue(&mq, packet(&[20])));
        assert!(!mq_enqueue(&mq, packet(&[30])));
        assert!(mq_full(&mq));
        assert!(mq_enqueue(&mq, packet(&[40])), "dropped when full");
        assert_eq!(mq_drops(&mq), 1);
        assert_eq!(mq_hdatalen(&mq), 20);
        assert!(mq_push(&mq, packet(&[50])), "the oldest is dropped");
        assert_eq!(mq_drops(&mq), 2);
        assert_eq!(mq_hdatalen(&mq), 30);

        let batch = MbufList::new();
        ml_enqueue(&batch, packet(&[60]));
        assert_eq!(mq_enlist(&mq, &batch), 1, "full: the batch is dropped");
        mq_set_maxlen(&mq, 4);
        ml_enqueue(&batch, packet(&[70]));
        assert_eq!(mq_enlist(&mq, &batch), 0);
        assert_eq!(mq_len(&mq), 3);

        let out = MbufList::new();
        mq_delist(&mq, &out);
        assert!(mq_empty(&mq));
        assert_eq!(ml_len(&out), 3);
        let m = ml_dequeue(&out).expect("a packet");
        assert_eq!(m.m_pkthdr().len.get(), 30);
        m_freem(m);
        ml_purge(&out);
        assert!(mq_dequeue(&mq).is_none());
        assert!(!mq_enqueue(&mq, packet(&[80])));
        assert_eq!(mq_purge(&mq), 1);
        assert_eq!(MBPOOL.pr_nout.get(), 0);

        let name = [IFQCTL_LEN, 0];
        let mut len = 0;
        assert_eq!(
            sysctl_mq(&name, 0, &mut len, 0, 0, &mq),
            Err(Errno::ENOTDIR)
        );
        assert_eq!(
            sysctl_mq(&[99], 0, &mut len, 0, 0, &mq),
            Err(Errno::EOPNOTSUPP)
        );

        // The nodes through the kern_sysctl.c helpers: a length, a new maximum, the drops.
        let mut val = [0u8; 4];
        let mut len = val.len();
        let oldp = val.as_mut_ptr() as usize;
        assert_eq!(sysctl_mq(&[IFQCTL_LEN], oldp, &mut len, 0, 0, &mq), Ok(()));
        assert_eq!((len, i32::from_ne_bytes(val)), (4, mq_len(&mq) as i32));
        let newmax = 7i32.to_ne_bytes();
        let newp = newmax.as_ptr() as usize;
        assert_eq!(
            sysctl_mq(&[IFQCTL_MAXLEN], 0, &mut len, newp, 4, &mq),
            Ok(())
        );
        assert_eq!(mq.mq_maxlen.load(Ordering::Relaxed), 7);
        assert_eq!(
            sysctl_mq(&[IFQCTL_DROPS], oldp, &mut len, newp, 4, &mq),
            Err(Errno::EPERM)
        );
    }

    #[test]
    fn tags_are_added_found_copied_and_deleted() {
        let _g = setup();

        let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
        assert!(m_tag_find(m, PACKET_TAG_GRE, None).is_none());
        assert!(m_tag_get(PACKET_TAG_GRE, -1, M_DONTWAIT).is_none());

        let gre = m_tag_get(PACKET_TAG_GRE, 4, M_DONTWAIT).expect("a tag");
        // SAFETY: the tag item has PACKET_TAG_MAXSIZE bytes of data after the structure.
        unsafe { ptr::copy_nonoverlapping([1u8, 2, 3, 4].as_ptr(), gre.data(), 4) };
        m_tag_prepend(m, gre);
        let dlt = m_tag_get(PACKET_TAG_DLT, 0, M_DONTWAIT).expect("a tag");
        m_tag_prepend(m, dlt);
        let gre2 = m_tag_get(PACKET_TAG_GRE, 0, M_DONTWAIT).expect("a tag");
        m_tag_prepend(m, gre2);
        assert_eq!(
            m.m_pkthdr().ph_tagsset.get(),
            PACKET_TAG_GRE | PACKET_TAG_DLT
        );
        assert_eq!(MTAGPOOL.pr_nout.get(), 3);

        // newest first
        let first = m_tag_find(m, PACKET_TAG_GRE, None).expect("found");
        assert!(ptr::eq(first, gre2));
        let second = m_tag_find(m, PACKET_TAG_GRE, Some(first)).expect("found again");
        assert!(ptr::eq(second, gre));
        assert!(m_tag_find(m, PACKET_TAG_TUNNEL, None).is_none());
        assert!(ptr::eq(m_tag_first(m).expect("first"), gre2));
        assert!(ptr::eq(m_tag_next(m, gre2).expect("next"), dlt));

        // a duplicate header copies the chain in order
        let d = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
        m_dup_pkthdr(d, m, M_DONTWAIT).expect("duplicated");
        assert_eq!(MTAGPOOL.pr_nout.get(), 6);
        let ids: Vec<u16> = d
            .m_pkthdr()
            .ph_tags
            .iter()
            .map(|t| t.m_tag_id.get())
            .collect();
        assert_eq!(ids, [PACKET_TAG_GRE, PACKET_TAG_DLT, PACKET_TAG_GRE]);
        let copied = m_tag_find(d, PACKET_TAG_GRE, m_tag_find(d, PACKET_TAG_GRE, None))
            .expect("the copied data tag");
        let mut bytes = [0u8; 4];
        // SAFETY: the copy has the 4 data bytes m_tag_copy wrote.
        unsafe { ptr::copy_nonoverlapping(copied.data(), bytes.as_mut_ptr(), 4) };
        assert_eq!(bytes, [1, 2, 3, 4]);
        m_freem(d);
        assert_eq!(MTAGPOOL.pr_nout.get(), 3);

        // SAFETY: `dlt` is on `m`'s list.
        unsafe { m_tag_delete(m, dlt) };
        assert_eq!(m.m_pkthdr().ph_tagsset.get(), PACKET_TAG_GRE);
        // SAFETY: `gre2` and `gre` are on `m`'s list.
        unsafe {
            m_tag_delete(m, gre2);
            m_tag_delete(m, gre);
        }
        assert_eq!(m.m_pkthdr().ph_tagsset.get(), 0);
        assert!(m_tag_find(m, PACKET_TAG_GRE, None).is_none());

        m_tag_prepend(
            m,
            m_tag_get(PACKET_TAG_TUNNEL, 8, M_DONTWAIT).expect("a tag"),
        );
        m_resethdr(m);
        assert!(m.m_pkthdr().ph_tags.first().is_none());
        assert_eq!(m.m_pkthdr().pf.prio.get(), IFQ_DEFPRIO);
        m_freem(m);
        assert_eq!(MTAGPOOL.pr_nout.get(), 0);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }

    static PRINTED: StdMutex<String> = StdMutex::new(String::new());

    fn collect(args: core::fmt::Arguments<'_>) {
        PRINTED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_str(&format!("{args}"));
    }

    #[test]
    fn ddb_printers() {
        let _g = setup();

        PRINTED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let m = packet(&[100, 2000]);
        m_print(m, collect);
        m_print_chain(Some(m), false, collect);
        m_print_packet(Some(m), false, collect);
        let out = PRINTED.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert!(out.contains("m_type: 1\tm_flags: 2<M_PKTHDR>\n"), "{out}");
        assert!(out.contains("m_pkthdr.len: 2100\n"), "{out}");
        assert!(
            out.contains(", dat, off 0, len 100, pktlen 2100, size 144\n"),
            "{out}"
        );
        assert!(out.contains(", clsize 2048\n"), "{out}");
        assert!(
            out.contains(" \\- total chain 2, len 2100, size 2192\n"),
            "{out}"
        );
        assert!(
            out.contains(", chain 2, pktlen 2100, len 2100, size 2192\n"),
            "{out}"
        );
        assert!(out.contains("\\-- total packets 1\n"), "{out}");
        m_freem(m);
    }

    #[test]
    fn zero_clears_the_data_area() {
        let _g = setup();

        let m = packet(&[64]);
        m_zero(m);
        assert_eq!(contents(m), vec![0u8; 64]);
        m.m_flags().set(m.m_flags().get() | M_ZEROIZE);
        m_freem(m);
        assert_eq!(MBPOOL.pr_nout.get(), 0);
    }
}
/* </TESTS> */
