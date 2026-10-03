//! Boot-time self-tests under feature `qemu`: a project helper, not OpenBSD code.
//!
//! Each test exercises a subsystem right after `main()` brought it up and prints one line that
//! `xtask smoke` asserts (`selftest: <what> ok`). They stand in for the user-space programs
//! OpenBSD would run until there is a user space; they are compiled only with feature `qemu`
//! (and need feature `alloc` for the allocator stress).

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::conf::param::HZ;
use crate::dev::pv::if_vio::{VioCtrlState, vio_softc};
use crate::kern::kern_clock::ticks;
use crate::kern::kern_fork::NTHREADS;
use crate::kern::kern_kthread::{kthread_create, kthread_exit};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_softintr::{SoftintrHand, softintr_establish, softintr_schedule};
use crate::kern::kern_synch::{msleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_task::{
    task_add, task_del, task_set, taskq_barrier, taskq_create, taskq_destroy,
};
use crate::kern::kern_tc::{getuptime, nsecuptime};
use crate::kern::kern_timeout::timeout_del;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_pool::{pool_destroy, pool_get, pool_init, pool_put, pool_reclaim};
use crate::kern::subr_prf::Str;
use crate::kern::uipc_mbuf::{
    MBPOOL, MBUF_MEM_ALLOC, MBUF_MEM_LIMIT, MCLPOOLS, MTAGPOOL, m_adj, m_copyback, m_copydata,
    m_copym, m_dup_pkt, m_freem, m_gethdr, m_pullup, m_split,
};
use crate::kern::uipc_mbuf2::{m_tag_find, m_tag_get, m_tag_prepend};
use crate::kprintf;
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmaTag, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load, bus_dmamap_load_mbuf, bus_dmamap_load_raw, bus_dmamap_sync, bus_dmamap_unload,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_mmap,
};
use crate::machine::cons::cn_rx_intr_establish;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_NONE, IPL_TTY};
use crate::machine::pmap::{
    pmap_extract, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_map_direct, pmap_update,
};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, Ifreq, if_enqueue, if_put, if_unit, ifioctl, link_state_is_up,
};
use crate::net::if_ethersubr::ETHERBROADCASTADDR;
use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::mbuf::mtod;
use crate::sys::mbuf::{M_COPYALL, M_DONTWAIT, MT_DATA, PACKET_TAG_GRE};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::mutex::Mutex;
use crate::sys::param::{PAGE_SIZE, PWAIT};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::sockio::SIOCSIFFLAGS;
use crate::sys::systm::INFSLP;
use crate::sys::task::{SYSTQ, SYSTQMP, Task, task_pending};
use crate::sys::timeout::Timeout;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::UVM_PGA_ZERO;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{KD_NOWAIT, KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_page::{PHYS_TO_VM_PAGE, uvm_pagealloc, uvm_pagefree, vm_page_to_phys};

/// A value that is neither all zeros nor all ones.
const PATTERN: u64 = 0x5a5a_c3c3_0f0f_a5a5;

/// `selftest=trap` on the kernel command line asks for [`trap_bad_access`].
static TRAP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=uart` asks for [`uart_echo`].
static UART_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=clock` asks for [`clock_check`].
static CLOCK_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=kthread` was on the command line.
static KTHREAD_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=taskq` was on the command line.
static TASKQ_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=vio` was on the command line.
static VIO_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Reads the self-test requests off the kernel command line: `selftest=trap` asks for the
/// fatal [`trap_bad_access`], which a plain boot must not run.
pub fn parse_bootargs(cmdline: &[u8]) {
    const TRAP: &[u8] = b"selftest=trap";
    const UART: &[u8] = b"selftest=uart";
    const CLOCK: &[u8] = b"selftest=clock";
    const KTHREAD: &[u8] = b"selftest=kthread";
    const TASKQ: &[u8] = b"selftest=taskq";
    const VIO: &[u8] = b"selftest=vio";
    if cmdline.windows(TRAP.len()).any(|w| w == TRAP) {
        TRAP_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(UART.len()).any(|w| w == UART) {
        UART_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(CLOCK.len()).any(|w| w == CLOCK) {
        CLOCK_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(KTHREAD.len()).any(|w| w == KTHREAD) {
        KTHREAD_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(TASKQ.len()).any(|w| w == TASKQ) {
        TASKQ_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(VIO.len()).any(|w| w == VIO) {
        VIO_REQUESTED.store(true, Ordering::Relaxed);
    }
}

/// Whether the command line asked for [`vio_check`].
pub fn vio_requested() -> bool {
    VIO_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`taskq_check`].
pub fn taskq_requested() -> bool {
    TASKQ_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`kthread_pingpong`].
pub fn kthread_requested() -> bool {
    KTHREAD_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`clock_check`].
pub fn clock_requested() -> bool {
    CLOCK_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`uart_echo`].
pub fn uart_requested() -> bool {
    UART_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`trap_bad_access`].
pub fn trap_requested() -> bool {
    TRAP_REQUESTED.load(Ordering::Relaxed)
}

/// Maps a fresh page into kernel virtual space reserved with `km_alloc(kv_any, kp_none)`,
/// writes through the mapping, reads back through the direct map, checks `pmap_extract`
/// before and after `pmap_kremove`.
pub fn pmap_kernel_mapping() {
    let Some(vp) = km_alloc(PAGE_SIZE, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
        kprintf!("selftest: pmap kernel mapping FAILED: no kernel virtual space\n");
        return;
    };
    let va = Vaddr::new(vp.as_ptr() as usize);

    let Some(pg) = uvm_pagealloc(None, 0, None, UVM_PGA_ZERO) else {
        kprintf!("selftest: pmap kernel mapping FAILED: no page\n");
        return;
    };
    let pa = vm_page_to_phys(pg);

    // SAFETY: `va` is kernel virtual space km_alloc reserved for this test, and `pa` is the
    // page just taken from the free list.
    unsafe { pmap_kenter_pa(va, pa, PROT_READ | PROT_WRITE) };
    pmap_update(pmap_kernel());

    // SAFETY: `va` is mapped read-write to `pa`, a page this test owns.
    unsafe { ptr::write_volatile(va.as_usize() as *mut u64, PATTERN) };
    // SAFETY: the direct map covers every page the allocator hands out.
    let seen = unsafe { ptr::read_volatile(pmap_map_direct(pg).as_usize() as *const u64) };
    let probe = Vaddr::new(va.as_usize() + 0x10);
    let extracted = pmap_extract(pmap_kernel(), probe);

    // SAFETY: the range was entered just above and is not used afterwards.
    unsafe { pmap_kremove(va, Vsize::new(PAGE_SIZE)) };
    pmap_update(pmap_kernel());
    let gone = pmap_extract(pmap_kernel(), va).is_none();
    uvm_pagefree(pg);
    km_free(vp, PAGE_SIZE, &KV_ANY, &KP_NONE);

    let ok = seen == PATTERN && extracted == Some(Paddr::new(pa.as_usize() + 0x10)) && gone;
    if ok {
        kprintf!("selftest: pmap kernel mapping ok\n");
    } else {
        kprintf!(
            "selftest: pmap kernel mapping FAILED: read {:#x}, extract {:?}, unmapped {}\n",
            seen,
            extracted.map(Paddr::as_usize),
            gone
        );
    }
}

/// The pool the stress test allocates from.
static STRESS_POOL: Pool = Pool::new();

/// Fills a block with a byte pattern derived from its index and checks it back.
fn fill_and_check(p: NonNull<u8>, len: usize, seed: u8) -> bool {
    for i in 0..len {
        // SAFETY: `len` bytes at `p` are this test's.
        unsafe { ptr::write_volatile(p.as_ptr().add(i), seed.wrapping_add(i as u8)) };
    }
    (0..len).all(|i| {
        // SAFETY: as above.
        (unsafe { ptr::read_volatile(p.as_ptr().add(i)) }) == seed.wrapping_add(i as u8)
    })
}

/// Exercises `malloc(9)` directly, the Rust allocator through `Vec` and `Box`, and a pool:
/// allocate, write, verify, free, several rounds, and reports the free page count before and
/// after.
pub fn malloc_pool_stress() {
    let free_before = UVMEXP.free.load(Ordering::Relaxed);
    let mut ok = true;
    let mut mallocs = 0u32;

    // malloc(9): every bucket size up to and past MAXALLOCSAVE, with and without M_ZERO.
    let sizes: [usize; 10] = [16, 24, 100, 256, 1000, 4096, 8192, 12288, 65536, 200_000];
    for round in 0..4u8 {
        let mut blocks: [Option<NonNull<u8>>; 10] = [None; 10];
        for (i, &sz) in sizes.iter().enumerate() {
            let flags = if i % 2 == 0 {
                M_NOWAIT
            } else {
                M_NOWAIT | M_ZERO
            };
            let Some(p) = malloc(sz, M_TEMP, flags) else {
                kprintf!("selftest: malloc({}) failed in round {}\n", sz, round);
                ok = false;
                continue;
            };
            if flags & M_ZERO != 0 {
                // SAFETY: `sz` bytes at `p` are this test's.
                ok &= (0..sz).all(|k| (unsafe { ptr::read_volatile(p.as_ptr().add(k)) }) == 0);
            }
            ok &= fill_and_check(p, sz, round.wrapping_mul(7).wrapping_add(i as u8));
            blocks[i] = Some(p);
            mallocs += 1;
        }
        for (i, &sz) in sizes.iter().enumerate() {
            if let Some(p) = blocks[i] {
                ok &= fill_and_check(p, sz, round.wrapping_add(i as u8));
                free(p, M_TEMP, sz);
            }
        }
    }

    // The Rust allocator: a growing Vec and a batch of Boxes.
    let mut v: Vec<u64> = Vec::new();
    for i in 0..4096u64 {
        v.push(i.wrapping_mul(0x9e37_79b9));
    }
    ok &= v
        .iter()
        .enumerate()
        .all(|(i, &x)| x == (i as u64).wrapping_mul(0x9e37_79b9));
    let boxes: Vec<Box<[u8; 100]>> = (0..64u8).map(|i| Box::new([i; 100])).collect();
    ok &= boxes
        .iter()
        .enumerate()
        .all(|(i, b)| b.iter().all(|&x| x == i as u8));
    drop(boxes);
    drop(v);

    // A pool of 48-byte items: take 300 (more than a page's worth), check, return, repeat.
    pool_init(&STRESS_POOL, 48, 0, IPL_NONE, 0, "stress", None);
    let mut pool_ok = true;
    for round in 0..3u8 {
        let mut items: Vec<NonNull<u8>> = Vec::with_capacity(300);
        for i in 0..300u32 {
            let flags = if round == 2 {
                PR_NOWAIT | PR_ZERO
            } else {
                PR_NOWAIT
            };
            let Some(p) = pool_get(&STRESS_POOL, flags) else {
                pool_ok = false;
                break;
            };
            if flags & PR_ZERO != 0 {
                // SAFETY: a 48-byte item of the pool, this test's.
                pool_ok &= (0..48).all(|k| (unsafe { ptr::read_volatile(p.as_ptr().add(k)) }) == 0);
            }
            pool_ok &= fill_and_check(p, 48, i as u8);
            items.push(p);
        }
        pool_ok &= items.len() == 300;
        let mut seen = items.clone();
        seen.sort_unstable();
        pool_ok &= seen
            .windows(2)
            .all(|w| w[1].as_ptr() as usize - w[0].as_ptr() as usize >= 48);
        for (i, p) in items.iter().enumerate() {
            pool_ok &= fill_and_check(*p, 48, (i as u8).wrapping_add(round));
        }
        for p in items {
            pool_put(&STRESS_POOL, p);
        }
    }
    let reclaimed = pool_reclaim(&STRESS_POOL);
    pool_ok &= STRESS_POOL.pr_nout.get() == 0;
    pool_destroy(&STRESS_POOL);
    ok &= pool_ok;

    let free_after = UVMEXP.free.load(Ordering::Relaxed);
    if ok {
        kprintf!(
            "selftest: malloc/pool stress ok ({} mallocs, {} pages free before, {} after, pool pages reclaimed: {})\n",
            mallocs,
            free_before,
            free_after,
            reclaimed
        );
    } else {
        kprintf!(
            "selftest: malloc/pool stress FAILED (pool {}, {} pages free before, {} after)\n",
            pool_ok,
            free_before,
            free_after
        );
    }
}

/// Builds, copies, pulls up, splits and frees mbuf chains right after `mbinit`, so the mbuf
/// and cluster pools allocate real pages through `m_pool_allocator` on the machine, and
/// checks that every mbuf, cluster and tag goes back to its pool.
pub fn mbuf_chains() {
    let data: Vec<u8> = (0..3000u32).map(|i| (i * 7 + 3) as u8).collect();
    let mut ok = true;

    let run = || -> Option<bool> {
        let mut ok = true;
        // a packet header mbuf grown by m_copyback into clusters
        let m = m_gethdr(M_DONTWAIT, MT_DATA)?;
        m_copyback(m, 0, &data, M_DONTWAIT).ok()?;
        ok &= m.m_pkthdr().len.get() == 3000;

        // a copy shares the clusters; both read back the same bytes
        let copy = m_copym(m, 0, M_COPYALL, M_DONTWAIT)?;
        let mut back = vec![0u8; 3000];
        m_copydata(copy, 0, &mut back);
        ok &= back == data;

        // a tag survives the copy of the header
        let tag = m_tag_get(PACKET_TAG_GRE, 0, M_DONTWAIT)?;
        m_tag_prepend(m, tag);
        let dup = m_dup_pkt(m, 2, M_DONTWAIT)?;
        ok &= m_tag_find(dup, PACKET_TAG_GRE, None).is_some();

        // pull the head up, split the packet, trim both ends
        let m = m_pullup(m, 200)?;
        let tail = m_split(m, 1000, M_DONTWAIT)?;
        m_adj(tail, 10);
        m_adj(tail, -10);
        ok &= m.m_pkthdr().len.get() == 1000 && tail.m_pkthdr().len.get() == 1980;
        let mut part = vec![0u8; 1980];
        m_copydata(tail, 0, &mut part);
        ok &= part[..] == data[1010..2990];

        m_freem(m);
        m_freem(tail);
        m_freem(copy);
        m_freem(dup);
        Some(ok)
    };
    match run() {
        Some(r) => ok &= r,
        None => ok = false,
    }

    let out = MBPOOL.pr_nout.get()
        + MTAGPOOL.pr_nout.get()
        + MCLPOOLS.iter().map(|pp| pp.pr_nout.get()).sum::<u32>();
    if ok && out == 0 {
        kprintf!(
            "selftest: mbufs ok ({} KiB of mbuf memory in use, limit {} KiB)\n",
            MBUF_MEM_ALLOC.load(Ordering::Relaxed) / 1024,
            MBUF_MEM_LIMIT.load(Ordering::Relaxed) / 1024
        );
    } else {
        kprintf!("selftest: mbufs FAILED ({} items still out)\n", out);
    }
}

/// Exercises `bus_dma(9)` on the machine's own tag (amd64's `pci_bus_dma_tag`, arm64's
/// `mainbus_dma_tag`), called by `cpu_configure` once the tag exists: allocates four pages of
/// DMA memory and checks how maps of different shapes load them (coalesced into one segment,
/// split at `maxsegsz` and at a boundary, refused when the map has too few segments), loads a
/// linear buffer through the direct map and an mbuf chain, syncs, and gives everything back.
pub fn bus_dma_check(t: BusDmaTag) {
    let size = 4 * PAGE_SIZE;
    let mut failed: Option<&str> = None;
    let mut fail = |what: &'static str| {
        if failed.is_none() {
            failed = Some(what);
        }
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let rsegs = match bus_dmamem_alloc(t, size, size, 0, &mut segs, BUS_DMA_NOWAIT | BUS_DMA_ZERO) {
        Ok(n) => n,
        Err(_) => {
            kprintf!("selftest: bus_dma FAILED (bus_dmamem_alloc)\n");
            return;
        }
    };
    let seg = segs[0];
    if rsegs != 1 || seg.ds_len != size || seg.ds_addr % size != 0 {
        fail("bus_dmamem_alloc segments");
    }
    if bus_dmamem_mmap(t, &segs, PAGE_SIZE as i64, PROT_READ, 0)
        != Some(Paddr::new(seg.ds_addr + PAGE_SIZE))
    {
        fail("bus_dmamem_mmap");
    }

    // Each shape: (maxsegsz, boundary, nsegments) and the segments load_raw must produce, as
    // (offset into the memory, length).
    type Shape<'a> = (usize, usize, i32, &'a [(usize, usize)]);
    let shapes: [Shape<'_>; 3] = [
        (size, 0, 4, &[(0, size)]),
        (
            PAGE_SIZE,
            0,
            4,
            &[
                (0, PAGE_SIZE),
                (PAGE_SIZE, PAGE_SIZE),
                (2 * PAGE_SIZE, PAGE_SIZE),
                (3 * PAGE_SIZE, PAGE_SIZE),
            ],
        ),
        (
            size,
            2 * PAGE_SIZE,
            4,
            &[(0, 2 * PAGE_SIZE), (2 * PAGE_SIZE, 2 * PAGE_SIZE)],
        ),
    ];
    let mut maps = Vec::new();
    for (maxsegsz, boundary, nsegments, want) in shapes {
        let Ok(map) = bus_dmamap_create(t, size, nsegments, maxsegsz, boundary, BUS_DMA_NOWAIT)
        else {
            fail("bus_dmamap_create");
            continue;
        };
        // SAFETY: the segments stay allocated until bus_dmamem_free below, after unload.
        if unsafe { bus_dmamap_load_raw(t, map, &segs, size, BUS_DMA_NOWAIT) }.is_err() {
            fail("bus_dmamap_load_raw");
        } else {
            let got: Vec<(usize, usize)> = map.dm_segs()[..map.dm_nsegs.get() as usize]
                .iter()
                .map(|s| (s.get().ds_addr - seg.ds_addr, s.get().ds_len))
                .collect();
            if got != want || map.dm_mapsize.get() != size {
                fail("bus_dmamap_load_raw segments");
            }
        }
        bus_dmamap_unload(t, map);
        if map.dm_nsegs.get() != 0 {
            fail("bus_dmamap_unload");
        }
        maps.push(map);
    }

    // A map with one segment of a page cannot take four pages.
    if let Ok(small) = bus_dmamap_create(t, size, 1, PAGE_SIZE, 0, BUS_DMA_NOWAIT) {
        // SAFETY: as above.
        if unsafe { bus_dmamap_load_raw(t, small, &segs, size, BUS_DMA_NOWAIT) }.is_ok() {
            fail("bus_dmamap_load_raw overflow");
        }
        maps.push(small);
    } else {
        fail("bus_dmamap_create (small)");
    }

    // A linear buffer: the DMA pages through the direct map, from an odd offset.
    if let (Some(pg), Some(&whole), Some(&paged)) = (
        PHYS_TO_VM_PAGE(Paddr::new(seg.ds_addr)),
        maps.first(),
        maps.get(1),
    ) {
        let kva = pmap_map_direct(pg).as_usize() + 100;
        // SAFETY: the buffer is the DMA memory allocated above, mapped by the direct map,
        // and stays allocated until after the unload.
        let r = unsafe {
            bus_dmamap_load(
                t,
                whole,
                kva as *mut u8,
                2 * PAGE_SIZE,
                None,
                BUS_DMA_NOWAIT,
            )
        };
        if r.is_err()
            || whole.dm_nsegs.get() != 1
            || whole.dm_segs()[0].get().ds_addr != seg.ds_addr + 100
        {
            fail("bus_dmamap_load (contiguous)");
        }
        bus_dmamap_sync(
            t,
            whole,
            0,
            2 * PAGE_SIZE,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
        bus_dmamap_sync(t, whole, 0, 2 * PAGE_SIZE, BUS_DMASYNC_POSTREAD);
        bus_dmamap_unload(t, whole);

        // SAFETY: as above.
        let r = unsafe {
            bus_dmamap_load(
                t,
                paged,
                kva as *mut u8,
                2 * PAGE_SIZE,
                None,
                BUS_DMA_NOWAIT,
            )
        };
        let lens: Vec<usize> = paged.dm_segs()[..paged.dm_nsegs.get().max(0) as usize]
            .iter()
            .map(|s| s.get().ds_len)
            .collect();
        if r.is_err() || lens[..] != [PAGE_SIZE - 100, PAGE_SIZE, 100] {
            fail("bus_dmamap_load (maxsegsz)");
        }
        bus_dmamap_unload(t, paged);
    } else {
        fail("direct map");
    }

    // An mbuf chain of clusters.
    let data: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
    match (m_gethdr(M_DONTWAIT, MT_DATA), maps.first()) {
        (Some(m), Some(&whole)) => {
            if m_copyback(m, 0, &data, M_DONTWAIT).is_err() {
                fail("m_copyback");
            }
            // SAFETY: the chain is freed after the unload.
            let r = unsafe { bus_dmamap_load_mbuf(t, whole, m, BUS_DMA_NOWAIT) };
            let total: usize = whole.dm_segs()[..whole.dm_nsegs.get().max(0) as usize]
                .iter()
                .map(|s| s.get().ds_len)
                .sum();
            if r.is_err() || whole.dm_mapsize.get() != 3000 || total != 3000 {
                fail("bus_dmamap_load_mbuf");
            }
            bus_dmamap_sync(t, whole, 0, 3000, BUS_DMASYNC_PREWRITE);
            bus_dmamap_sync(t, whole, 0, 3000, BUS_DMASYNC_POSTWRITE);
            bus_dmamap_unload(t, whole);
            m_freem(m);
        }
        _ => fail("m_gethdr"),
    }

    let nmaps = maps.len();
    for map in maps {
        // SAFETY: made above, unloaded, not used again.
        unsafe { bus_dmamap_destroy(t, NonNull::from(map)) };
    }
    // SAFETY: allocated above, every map that loaded it is unloaded and destroyed.
    unsafe { bus_dmamem_free(t, &segs) };

    match failed {
        None => {
            kprintf!(
                "selftest: bus_dma ok ({} maps, {} pages at {:#x})\n",
                nmaps,
                size / PAGE_SIZE,
                seg.ds_addr
            );
        }
        Some(what) => {
            kprintf!("selftest: bus_dma FAILED ({})\n", what);
        }
    }
}

/// Reads a kernel address that is not mapped, so the kernel takes a page fault (amd64) or a
/// data abort (arm64) in supervisor mode and the trap handler prints OpenBSD's fatal trap
/// message and panics. Never returns normally: it is the M4 exit criterion, and `smoke`
/// asserts the panic.
pub fn trap_bad_access() {
    // Kernel virtual space with nothing behind it: km_alloc(kv_any, kp_none) reserves it in
    // kernel_map, whose page tables exist, and maps no page.
    let Some(vp) = km_alloc(PAGE_SIZE, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
        kprintf!("selftest: trap FAILED: no kernel virtual space\n");
        return;
    };
    let va = Vaddr::new(vp.as_ptr() as usize);
    kprintf!(
        "selftest: trap: reading unmapped kernel address {:#x}\n",
        va.as_usize()
    );
    // SAFETY: deliberately not sound: `va` is unmapped, the read faults and the trap handler
    // panics, so the value is never used.
    let seen = unsafe { ptr::read_volatile(va.as_usize() as *const u64) };
    kprintf!("selftest: trap FAILED: read {seen:#x} without a fault\n");
}

/// The line received by interrupt, filled by the hard handler, read by the soft handler.
static UART_LINE: StaticCell<[u8; 80]> = StaticCell::new([0; 80]);
/// How many bytes of `UART_LINE` are filled.
static UART_LEN: AtomicUsize = AtomicUsize::new(0);
/// Set by the soft handler when a newline arrived.
static UART_DONE: AtomicBool = AtomicBool::new(false);
/// The soft interrupt handler's handle.
static UART_SI: AtomicPtr<SoftintrHand> = AtomicPtr::new(ptr::null_mut());

/// The hard interrupt's byte sink: stores the byte and schedules the soft handler, as
/// `comintr` fills `sc_ibuf` and schedules `comsoft`.
fn uart_rx_sink(c: u8) {
    let n = UART_LEN.load(Ordering::Relaxed);
    if n < 80 {
        // SAFETY: written from the interrupt handler at IPL_TTY, read by the soft handler
        // after it is scheduled, never both at once on the one CPU.
        unsafe { UART_LINE.get_mut()[n] = c };
        UART_LEN.store(n + 1, Ordering::Relaxed);
    }
    if let Some(si) = NonNull::new(UART_SI.load(Ordering::Relaxed)) {
        softintr_schedule(si);
    }
}

/// The soft interrupt handler: echoes a complete line.
fn uart_soft(_arg: *mut core::ffi::c_void) {
    let n = UART_LEN.load(Ordering::Relaxed);
    // SAFETY: as for `uart_rx_sink`.
    let line = unsafe { &UART_LINE.get()[..n] };
    if let Some(end) = line.iter().position(|&c| c == b'\n' || c == b'\r')
        && !UART_DONE.swap(true, Ordering::Relaxed)
    {
        kprintf!("selftest: uart echo: {}\n", Str(&line[..end]));
    }
}

/// Arms the console UART's receive interrupt and a soft interrupt behind it, then waits for
/// a line to arrive and echoes it: the M4 exit criterion (`smoke` sends the line).
pub fn uart_echo() {
    let Some(si) = softintr_establish(IPL_TTY, uart_soft, ptr::null_mut()) else {
        kprintf!("selftest: uart echo FAILED: softintr_establish\n");
        return;
    };
    UART_SI.store(si.as_ptr(), Ordering::Relaxed);
    if let Err(e) = cn_rx_intr_establish(uart_rx_sink) {
        kprintf!(
            "selftest: uart echo FAILED: cn_rx_intr_establish: {:?}\n",
            e
        );
        return;
    }
    kprintf!("selftest: uart rx interrupt armed\n");
    // No clock yet: a spin count long enough for the test harness to type the line.
    let mut spins: u64 = 0;
    while !UART_DONE.load(Ordering::Relaxed) {
        core::hint::spin_loop();
        spins += 1;
        if spins == 300_000_000 {
            kprintf!(
                "selftest: uart echo FAILED: no line received ({} bytes so far)\n",
                UART_LEN.load(Ordering::Relaxed)
            );
            return;
        }
    }
}

/// Set by [`clock_timeout_fired`].
static CLOCK_TIMEOUT_FIRED: AtomicBool = AtomicBool::new(false);
/// The timeout `clock_check` arms.
static CLOCK_TIMEOUT: Timeout = Timeout::zeroed();

/// The timeout's handler: runs from `softclock`.
fn clock_timeout_fired(_arg: *mut core::ffi::c_void) {
    CLOCK_TIMEOUT_FIRED.store(true, Ordering::Relaxed);
}

/// Waits for `hz` hardclock ticks with the clock interrupt running and checks that the
/// timecounter saw about a second go by and that a `timeout(9)` armed for half a second fired:
/// the M5 exit criterion "uptime ticks at hz".
pub fn clock_check() {
    let hz = HZ.load(Ordering::Relaxed);
    let start_ticks = ticks();
    let start_ns = nsecuptime();
    timeout_set(&CLOCK_TIMEOUT, clock_timeout_fired, ptr::null_mut());
    timeout_add_msec(&CLOCK_TIMEOUT, 500);

    // A bound on the wait in case nothing ticks: a few seconds of spinning.
    let mut spins: u64 = 0;
    while ticks().wrapping_sub(start_ticks) < hz {
        core::hint::spin_loop();
        spins += 1;
        if spins == 2_000_000_000 {
            kprintf!(
                "selftest: clock FAILED: {} ticks after {} spins (hz={})\n",
                ticks().wrapping_sub(start_ticks),
                spins,
                hz
            );
            return;
        }
    }
    let elapsed_ms = nsecuptime().wrapping_sub(start_ns) / 1_000_000;
    let fired = CLOCK_TIMEOUT_FIRED.load(Ordering::Relaxed);
    // hz ticks should take a second give or take the tick the wait started in.
    if fired && (900..=1200).contains(&elapsed_ms) {
        kprintf!(
            "selftest: clock ok: {} ticks in {} ms (hz={}), timeout fired, uptime {} s\n",
            hz,
            elapsed_ms,
            hz,
            getuptime()
        );
    } else {
        kprintf!(
            "selftest: clock FAILED: {} ticks in {} ms (hz={}), timeout fired: {}\n",
            hz,
            elapsed_ms,
            hz,
            fired
        );
    }
}

/// The mutex the ping-pong threads hand the turn under (`msleep`'s interlock).
static PINGPONG_MTX: Mutex = Mutex::new(IPL_NONE);
/// Whose turn it is: even for ping, odd for pong; `2 * PINGPONG_ROUNDS` ends the game.
static PINGPONG_TURN: AtomicU32 = AtomicU32::new(0);
/// How many threads finished.
static PINGPONG_DONE: AtomicU32 = AtomicU32::new(0);
/// A channel nobody wakes: proc0's timed sleep while the reaper runs.
static PINGPONG_REAP: AtomicU32 = AtomicU32::new(0);
/// Turns per thread.
const PINGPONG_ROUNDS: u32 = 50;

/// One ping-pong thread: `arg` is 1 (ping) or 2 (pong), as an address (`fork1` would
/// replace a null one with the thread). Waits for its turn under the mutex, passes the turn,
/// wakes the other, and parks forever once the rounds are done.
fn pingpong_thread(arg: *mut core::ffi::c_void) {
    let me = (arg as usize as u32) - 1;

    mtx_enter(&PINGPONG_MTX);
    loop {
        let turn = PINGPONG_TURN.load(Ordering::Relaxed);
        if turn >= 2 * PINGPONG_ROUNDS {
            break;
        }
        if turn % 2 != me {
            let _ = msleep_nsec(
                ptr::addr_of!(PINGPONG_TURN),
                &PINGPONG_MTX,
                PWAIT,
                "pingpong",
                INFSLP,
            );
            continue;
        }
        PINGPONG_TURN.store(turn + 1, Ordering::Relaxed);
        wakeup(ptr::addr_of!(PINGPONG_TURN));
    }
    if PINGPONG_DONE.fetch_add(1, Ordering::Relaxed) + 1 == 2 {
        wakeup(ptr::addr_of!(PINGPONG_DONE));
    }
    mtx_leave(&PINGPONG_MTX);

    // Done: exit1 -> sched_exit -> idle's exit2 -> the reaper frees the thread.
    kthread_exit(0);
}

/// Creates two kernel threads that pass a turn back and forth with `msleep`/`wakeup` while
/// proc0 sleeps for them to finish: the M5 exit criterion "two kthreads ping-pong via
/// tsleep/wakeup". Every hand-over is a context switch through the run queues and the idle
/// thread. Both threads then `kthread_exit` and proc0 checks the reaper took them (M6-b).
pub fn kthread_pingpong() {
    mtx_init(&PINGPONG_MTX, IPL_NONE);
    let start_ns = nsecuptime();

    let ping_tid = match kthread_create(pingpong_thread, ptr::without_provenance_mut(1), b"ping") {
        Ok(p) => p.p_tid.get(),
        Err(e) => {
            kprintf!(
                "selftest: kthread ping-pong FAILED: kthread_create: {:?}\n",
                e
            );
            return;
        }
    };
    let pong_tid = match kthread_create(pingpong_thread, ptr::without_provenance_mut(2), b"pong") {
        Ok(p) => p.p_tid.get(),
        Err(e) => {
            kprintf!(
                "selftest: kthread ping-pong FAILED: kthread_create: {:?}\n",
                e
            );
            return;
        }
    };

    mtx_enter(&PINGPONG_MTX);
    while PINGPONG_DONE.load(Ordering::Relaxed) < 2 {
        let _ = msleep_nsec(
            ptr::addr_of!(PINGPONG_DONE),
            &PINGPONG_MTX,
            PWAIT,
            "selftest",
            INFSLP,
        );
    }
    mtx_leave(&PINGPONG_MTX);
    let elapsed_us = nsecuptime().wrapping_sub(start_ns) / 1000;

    // Give the two exits time to reach the reaper: a timed sleep (endtsleep wakes it).
    let nthreads_before = NTHREADS.load(Ordering::Relaxed);
    let _ = tsleep_nsec(ptr::addr_of!(PINGPONG_REAP), PWAIT, "reapwait", 50_000_000);
    let nthreads_after = NTHREADS.load(Ordering::Relaxed);

    let turns = PINGPONG_TURN.load(Ordering::Relaxed);
    if turns == 2 * PINGPONG_ROUNDS && nthreads_after == nthreads_before - 2 {
        kprintf!(
            "selftest: kthread ping-pong ok: {} turns between tid {} and tid {} in {} us, {} context switches, both exited and reaped ({} threads left)\n",
            turns,
            ping_tid,
            pong_tid,
            elapsed_us,
            UVMEXP.swtch.load(Ordering::Relaxed),
            nthreads_after
        );
    } else {
        kprintf!(
            "selftest: kthread ping-pong FAILED: {} turns, {} threads before the exits, {} after\n",
            turns,
            nthreads_before,
            nthreads_after
        );
    }
}

/// Serialises the task queue check's bookkeeping between the workers and proc0.
static TASKQ_MTX: Mutex = Mutex::new(IPL_NONE);
/// One bit per selftest task that ran (its argument is the bit number).
static TASKQ_RAN: AtomicU32 = AtomicU32::new(0);
/// The thread that ran each task.
static TASKQ_RUNNER: [AtomicPtr<Proc>; 4] = [const { AtomicPtr::new(ptr::null_mut()) }; 4];
/// Queued on `systq` and run.
static TASKQ_SYS_TASK: Task = Task::zeroed();
/// Queued on `systq` and deleted before it can run.
static TASKQ_DEL_TASK: Task = Task::zeroed();
/// Queued on `systqmp` and run.
static TASKQ_MP_TASK: Task = Task::zeroed();
/// Queued on a queue made by `taskq_create`, which is then destroyed.
static TASKQ_NEW_TASK: Task = Task::zeroed();

/// The selftest tasks' function: records that task `arg` ran, and on which thread.
fn taskq_selftest_task(arg: *mut core::ffi::c_void) {
    let bit = arg as usize;
    mtx_enter(&TASKQ_MTX);
    TASKQ_RUNNER[bit].store(
        curproc().map_or(ptr::null_mut(), |p| ptr::from_ref(p).cast_mut()),
        Ordering::Relaxed,
    );
    TASKQ_RAN.fetch_or(1 << bit, Ordering::Relaxed);
    wakeup(ptr::addr_of!(TASKQ_RAN));
    mtx_leave(&TASKQ_MTX);
}

/// Sleeps (at most a second per wait) until task `bit` ran; returns the name of the thread
/// that ran it, or `None`.
fn taskq_wait(bit: usize) -> Option<&'static [u8]> {
    mtx_enter(&TASKQ_MTX);
    while TASKQ_RAN.load(Ordering::Relaxed) & (1 << bit) == 0 {
        if msleep_nsec(
            ptr::addr_of!(TASKQ_RAN),
            &TASKQ_MTX,
            PWAIT,
            "tqtest",
            1_000_000_000,
        )
        .is_err()
        {
            break;
        }
    }
    mtx_leave(&TASKQ_MTX);
    // SAFETY: the thread that ran the task is still alive: the system queues' threads never
    // exit, and the created queue is destroyed (its thread exits) only after this read.
    let runner = unsafe { TASKQ_RUNNER[bit].load(Ordering::Relaxed).as_ref() }?;
    Some(runner.process().comm())
}

/// The M7b task queue check: `task_add`/`task_del` on a pending task, a task on `systq` run
/// by the `systq` thread, one on `systqmp` by its thread, a `taskq_barrier` that returns, and
/// a queue from `taskq_create` that runs a task and is destroyed (its thread exits).
pub fn taskq_check() {
    let arg = ptr::without_provenance_mut::<core::ffi::c_void>;
    task_set(&TASKQ_SYS_TASK, taskq_selftest_task, arg(0));
    task_set(&TASKQ_DEL_TASK, taskq_selftest_task, arg(1));
    task_set(&TASKQ_MP_TASK, taskq_selftest_task, arg(2));
    task_set(&TASKQ_NEW_TASK, taskq_selftest_task, arg(3));

    // proc0 does not sleep between these calls, so the systq thread cannot run in between.
    let added = task_add(SYSTQ, &TASKQ_DEL_TASK);
    let readded = task_add(SYSTQ, &TASKQ_DEL_TASK);
    let pending = task_pending(&TASKQ_DEL_TASK);
    let deleted = task_del(SYSTQ, &TASKQ_DEL_TASK);
    let redeleted = task_del(SYSTQ, &TASKQ_DEL_TASK);
    if !(added && !readded && pending && deleted && !redeleted) {
        kprintf!(
            "selftest: taskq FAILED: task_add {} {}, pending {}, task_del {} {}\n",
            added,
            readded,
            pending,
            deleted,
            redeleted
        );
        return;
    }

    let _ = task_add(SYSTQ, &TASKQ_SYS_TASK);
    let sys_runner = taskq_wait(0);
    let _ = task_add(SYSTQMP, &TASKQ_MP_TASK);
    let mp_runner = taskq_wait(2);
    taskq_barrier(SYSTQ);

    let Some(tq) = taskq_create(b"tqtest", 1, IPL_NONE, 0) else {
        kprintf!("selftest: taskq FAILED: taskq_create\n");
        return;
    };
    let _ = task_add(tq, &TASKQ_NEW_TASK);
    let new_runner = taskq_wait(3);
    let nthreads_before = NTHREADS.load(Ordering::Relaxed);
    // SAFETY: `tq` came from `taskq_create` and nothing uses it after this.
    unsafe { taskq_destroy(NonNull::from(tq)) };
    // Give the worker's exit time to reach the reaper, in timed sleeps (endtsleep wakes us),
    // a second at most.
    for _ in 0..20 {
        if NTHREADS.load(Ordering::Relaxed) != nthreads_before {
            break;
        }
        let _ = tsleep_nsec(ptr::addr_of!(TASKQ_RUNNER), PWAIT, "reapwait", 50_000_000);
    }
    let nthreads_after = NTHREADS.load(Ordering::Relaxed);

    let ran = TASKQ_RAN.load(Ordering::Relaxed);
    let ok = ran == 0b1101
        && sys_runner == Some(b"systq".as_slice())
        && mp_runner == Some(b"systqmp".as_slice())
        && new_runner == Some(b"tqtest".as_slice())
        && nthreads_after == nthreads_before - 1;
    let name = |r: Option<&'static [u8]>| Str(r.unwrap_or(b"none"));
    if ok {
        kprintf!(
            "selftest: taskq ok: systq ran in {}, task_del took back a pending task, systqmp ran in {}, barrier returned, {} ran and was destroyed ({} threads left)\n",
            name(sys_runner),
            name(mp_runner),
            name(new_runner),
            nthreads_after
        );
    } else {
        kprintf!(
            "selftest: taskq FAILED: ran {:#b}, runners {} {} {}, {} threads before the destroy, {} after\n",
            ran,
            name(sys_runner),
            name(mp_runner),
            name(new_runner),
            nthreads_before,
            nthreads_after
        );
    }
}

/// The ARP request [`vio_check`] sends: who has 10.0.2.2 (QEMU's user-mode gateway), tell
/// 10.0.2.15, from `enaddr`, broadcast; padded to the 60 bytes of a minimal frame. Built by
/// hand: `netinet/if_ether.c` (`arprequest`) is another port.
fn vio_arp_request(enaddr: &[u8; ETHER_ADDR_LEN]) -> [u8; 60] {
    let mut f = [0u8; 60];
    // Ethernet header.
    f[0..6].copy_from_slice(&ETHERBROADCASTADDR);
    f[6..12].copy_from_slice(enaddr);
    f[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    // struct arphdr: Ethernet hardware, IPv4 protocol, 6 and 4 byte addresses, a request.
    f[14..16].copy_from_slice(&1u16.to_be_bytes()); // ARPHRD_ETHER
    f[16..18].copy_from_slice(&ETHERTYPE_IP.to_be_bytes());
    f[18] = ETHER_ADDR_LEN as u8;
    f[19] = 4;
    f[20..22].copy_from_slice(&1u16.to_be_bytes()); // ARPOP_REQUEST
    // struct ether_arp: sender 10.0.2.15, target 10.0.2.2 (hardware address unknown).
    f[22..28].copy_from_slice(enaddr);
    f[28..32].copy_from_slice(&[10, 0, 2, 15]);
    f[38..42].copy_from_slice(&[10, 0, 2, 2]);
    f
}

/// The M7b network card check (`selftest=vio`): `vio0` is brought up through `ifioctl`
/// (`SIOCSIFFLAGS` with `IFF_UP`, so `vio_init` fills the receive ring and programs the
/// filter over the control queue, whose answers arrive only through the device's interrupt:
/// `cold` is over), then a broadcast ARP request for 10.0.2.2 is queued with `if_enqueue`
/// (`vio_start` sends it) and the test waits for any frame to reach the interface's input
/// queue (`ifiq_input`, where `vio_rxeof` hands frames to the stack; QEMU's slirp answers
/// from 52:55:0a:00:02:02). The receive tick is stopped first, so a frame can only come
/// in through the receive interrupt (`vio_rx_intr`); `ether_input` then drops it at the ARP
/// demux, which `netinet` is not here to take.
pub fn vio_check() {
    let Some(ifp) = if_unit(b"vio0") else {
        kprintf!("selftest: vio FAILED: no vio0\n");
        return;
    };
    let Some(p) = curproc() else {
        kprintf!("selftest: vio FAILED: no process\n");
        return;
    };

    let mut ifr = Ifreq::zeroed();
    ifr.ifr_name[..4].copy_from_slice(b"vio0");
    ifr.set_ifr_flags((ifp.if_flags.get() | IFF_UP) as i16);
    // SAFETY: `data` is a kernel `struct ifreq`, the structure SIOCSIFFLAGS encodes; there
    // is no socket, which this command does not use.
    let up = unsafe { ifioctl(ptr::null(), SIOCSIFFLAGS, ptr::from_mut(&mut ifr).cast(), p) };
    let sc = vio_softc(ifp);
    let running = ifp.if_flags.get() & IFF_RUNNING != 0;
    let ctrl = sc.sc_ctrl_inuse.get();
    if up.is_err() || !running || ctrl != VioCtrlState::FREE {
        kprintf!(
            "selftest: vio FAILED: up {:?}, running {}, control queue {:?}\n",
            up,
            running,
            ctrl
        );
        if_put(ifp);
        return;
    }
    kprintf!(
        "selftest: vio up ok: running, link {}, control queue answered\n",
        if link_state_is_up(ifp.if_link_state.get()) {
            "up"
        } else {
            "down"
        }
    );

    // From here on only the receive interrupt can call vio_rxeof.
    timeout_del(&sc.sc_rxtick);
    let ifiq = ifp.ifiq(0);
    let rx_before = ifiq.ifiq_packets.get();

    let frame = vio_arp_request(&sc.sc_ac.ac_enaddr.get());
    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        kprintf!("selftest: vio FAILED: m_gethdr\n");
        if_put(ifp);
        return;
    };
    // SAFETY: a fresh packet header mbuf has MHLEN (> 60) bytes at m_data.
    unsafe { ptr::copy_nonoverlapping(frame.as_ptr(), mtod::<u8>(m), frame.len()) };
    m.m_len().set(frame.len() as u32);
    m.m_pkthdr().len.set(frame.len() as i32);
    if let Err(e) = if_enqueue(ifp, m) {
        kprintf!("selftest: vio FAILED: if_enqueue {:?}\n", e);
        if_put(ifp);
        return;
    }

    // Wait up to two seconds, in 10 ms sleeps, for a frame.
    for _ in 0..200 {
        if ifiq.ifiq_packets.get() != rx_before {
            break;
        }
        let _ = tsleep_nsec(ptr::addr_of!(VIO_REQUESTED), PWAIT, "viotest", 10_000_000);
    }
    let frames = ifiq.ifiq_packets.get() - rx_before;
    let sent = ifp.if_snd.ifq_packets.get();
    if frames > 0 {
        kprintf!(
            "selftest: vio rx ok: sent {} frame, received {} through the rx interrupt\n",
            sent,
            frames
        );
    } else {
        kprintf!(
            "selftest: vio rx FAILED: sent {} frame, nothing received\n",
            sent
        );
    }
    if_put(ifp);
}
