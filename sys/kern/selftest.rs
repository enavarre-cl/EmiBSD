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
//! Boot-time self-tests under feature `qemu`: a project helper, not OpenBSD code.
//!
//! Each test exercises a subsystem right after `main()` brought it up and prints one line that
//! `xtask smoke` asserts (`selftest: <what> ok`). They stand in for the user-space programs
//! OpenBSD would run until there is a user space; they are compiled only with feature `qemu`
//! (and need feature `alloc` for the allocator stress).
//!
//! The network self-test of every default boot ([`ping_gateway`], which also configures the
//! first Ethernet interface) runs after `cpu_boot_secondary_processors` and the per-CPU
//! checks (M13): OpenBSD configures its interfaces from netstart(8), with every processor
//! running, and a driver whose queue interrupts sit on the application processors (vmx(4)
//! through intrmap(9)) calls `intr_barrier` from its `init`, which waits for those CPUs to go
//! through the scheduler. Its output lines are unchanged.

use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use libkern::StaticCell;

use crate::conf::param::HZ;
use crate::dev::cons::cn_tab;
use crate::dev::pv::if_vio::{VioCtrlState, vio_softc};
use crate::dev::rasops::rasops::{RASOPS_CMAP, RasopsInfo};
use crate::dev::wscons::wsdisplayvar::{
    WSATTR_HILIT, WSATTR_WSCOLORS, WSCOL_BLUE, WSCOL_WHITE, WsemuldisplaydevAttachArgs,
};
use crate::kern::init_main::PROC0;
use crate::kern::kern_clock::ticks;
use crate::kern::kern_fork::NTHREADS;
use crate::kern::kern_kthread::{kthread_create, kthread_exit};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{msleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_task::{
    task_add, task_del, task_set, taskq_barrier, taskq_create, taskq_destroy,
};
use crate::kern::kern_tc::{getuptime, nsecuptime};
use crate::kern::kern_timeout::timeout_del;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_pool::{
    pool_cache_init, pool_cache_pool_info, pool_destroy, pool_get, pool_init, pool_put,
    pool_reclaim,
};
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
use crate::machine::conf::cdevsw;
use crate::machine::copy::copyin;
#[cfg(feature = "multiprocessor")]
use crate::machine::cpu::CpuInfo;
use crate::machine::cpu::{cpu_number, curproc};
use crate::machine::intr::{IPL_NONE, IPL_VM};
use crate::machine::pmap::{
    MachinePmap, pmap_activate, pmap_deactivate, pmap_enter, pmap_extract, pmap_kenter_pa,
    pmap_kernel, pmap_kremove, pmap_map_direct, pmap_remove, pmap_update,
};
use crate::machine::{Machine, VmParam};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, Ifreq, if_enqueue, if_put, if_unit, ifioctl, link_state_is_up,
};
use crate::net::if_ethersubr::ETHERBROADCASTADDR;
use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::mbuf::mtod;
use crate::sys::mbuf::{M_COPYALL, M_DONTWAIT, MT_DATA, PACKET_TAG_GRE};
use crate::sys::mman::{MADV_NORMAL, MAP_INHERIT_COPY, PROT_READ, PROT_WRITE};
use crate::sys::mutex::Mutex;
use crate::sys::param::{NODEV, PAGE_SIZE, PWAIT};
use crate::sys::pool::{KinfoPool, PR_NOWAIT, PR_RWLOCK, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::sockio::SIOCSIFFLAGS;
use crate::sys::stat::S_IFCHR;
use crate::sys::systm::INFSLP;
use crate::sys::task::{SYSTQ, SYSTQMP, Task, task_pending};
use crate::sys::timeout::Timeout;
use crate::sys::types::{Paddr, Vaddr, Vsize, major};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::IO_NDELAY;
use crate::uvm::uvm_extern::{
    PROT_MASK, UVM_FLAG_COPYONW, UVM_FLAG_FIXED, UVM_PGA_ZERO, UVM_PLA_NOWAIT, UVM_PLA_WAITOK,
    UVM_PLA_ZERO, Vmspace, uvm_mapflag,
};
use crate::uvm::uvm_fault::{VM_FAULT_INVALID, uvm_fault};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{KD_NOWAIT, KD_WAITOK, KP_NONE, KP_PAGEABLE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_map::{uvm_mapanon, uvm_unmap, uvmspace_alloc, uvmspace_free};
use crate::uvm::uvm_page::{
    PHYS_TO_VM_PAGE, Pglist, uvm_pagealloc, uvm_pagefree, uvm_pglistalloc, uvm_pglistfree,
    vm_page_to_phys,
};
use crate::uvm::uvm_pmap::PMAP_WIRED;

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
/// `selftest=mpstress` was on the command line.
static MPSTRESS_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=fb` asks for [`fb_check`].
static FB_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=wscons` asks for [`wscons_grid`].
static WSCONS_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=tpm` asks for `tpm_selftest` (`dev/acpi/tpm.rs`).
static TPM_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The first frame buffer that attached (its device name and the attach arguments it would
/// hand `wsdisplay`), for [`fb_check`].
struct FbAttached {
    /// The device's name (`efifb0`, `simplefb0`).
    xname: [u8; 16],
    /// What `config_found` would hand the `wsdisplay` child.
    aa: Option<WsemuldisplaydevAttachArgs>,
}

// SAFETY: written once while cold by the frame buffer's attach, read by `fb_check` after
// autoconfiguration; the pointers it holds are the driver's, which live for good.
unsafe impl Send for FbAttached {}

/// What [`fb_attached`] recorded.
static FB_ATTACHED: StaticCell<FbAttached> = StaticCell::new(FbAttached {
    xname: [0; 16],
    aa: None,
});

/// The text [`fb_check`] draws.
const FB_TEXT: &[u8] = b"EmiBSD M13";

/// Reads the self-test requests off the kernel command line: `selftest=trap` asks for the
/// fatal [`trap_bad_access`], which a plain boot must not run.
pub fn parse_bootargs(cmdline: &[u8]) {
    const TRAP: &[u8] = b"selftest=trap";
    const UART: &[u8] = b"selftest=uart";
    const CLOCK: &[u8] = b"selftest=clock";
    const KTHREAD: &[u8] = b"selftest=kthread";
    const TASKQ: &[u8] = b"selftest=taskq";
    const VIO: &[u8] = b"selftest=vio";
    const MPSTRESS: &[u8] = b"selftest=mpstress";
    const FB: &[u8] = b"selftest=fb";
    const WSCONS: &[u8] = b"selftest=wscons";
    const TPM: &[u8] = b"selftest=tpm";
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
    if cmdline.windows(MPSTRESS.len()).any(|w| w == MPSTRESS) {
        MPSTRESS_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(FB.len()).any(|w| w == FB) {
        FB_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(WSCONS.len()).any(|w| w == WSCONS) {
        WSCONS_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(TPM.len()).any(|w| w == TPM) {
        TPM_REQUESTED.store(true, Ordering::Relaxed);
    }
}

/// Whether the command line asked for [`fb_check`].
pub fn fb_requested() -> bool {
    FB_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`wscons_grid`].
pub fn wscons_requested() -> bool {
    WSCONS_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for `tpm_selftest` (`dev/acpi/tpm.rs`, M16e): a
/// `TPM2_SelfTest` sent through tpm0's own command path, whose response code `smoke-tpm`
/// checks.
pub fn tpm_requested() -> bool {
    TPM_REQUESTED.load(Ordering::Relaxed)
}

/// Whether the command line asked for [`mpstress`].
pub fn mpstress_requested() -> bool {
    MPSTRESS_REQUESTED.load(Ordering::Relaxed)
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

/// The word at physical address `pa`, read through the direct map; `None` when `pa` is not a
/// managed page.
fn read_phys(pa: Paddr) -> Option<u64> {
    let off = pa.as_usize() & (PAGE_SIZE - 1);
    let pg = PHYS_TO_VM_PAGE(Paddr::new(pa.as_usize() - off))?;
    let va = pmap_map_direct(pg).as_usize() + off;
    // SAFETY: the direct map covers every managed page; a word-aligned read of RAM.
    Some(unsafe { ptr::read_volatile(va as *const u64) })
}

/// Pageable kernel memory, as a pipe's buffer: `km_alloc(kv_any, kp_pageable)` faults its
/// pages in with `pmap_enter(pmap_kernel())` and `km_free` takes them out with
/// `pmap_remove(pmap_kernel())` and frees them; the next allocation tends to get the same
/// addresses back. Every word written through the new mapping must land in the page
/// `pmap_extract` reports, not in a freed page a stale TLB entry still points at.
fn pmap_reuse_kernel() -> Result<(), &'static str> {
    const NPAGES: usize = 4;
    const ROUNDS: u64 = 8;

    for round in 0..ROUNDS {
        let Some(buf) = km_alloc(NPAGES * PAGE_SIZE, &KV_ANY, &KP_PAGEABLE, &KD_WAITOK) else {
            return Err("km_alloc");
        };
        let mut result = Ok(());
        for i in 0..NPAGES {
            let va = buf.as_ptr() as usize + i * PAGE_SIZE;
            let pattern = PATTERN ^ (round << 8 | i as u64);
            // SAFETY: a page of the pageable allocation just made; the write faults it in.
            unsafe { ptr::write_volatile(va as *mut u64, pattern) };
            let seen = pmap_extract(pmap_kernel(), Vaddr::new(va)).and_then(read_phys);
            if seen != Some(pattern) {
                kprintf!(
                    "selftest: pmap reuse: round {} page {}: wrote {:#x}, the mapped page holds {:?}\n",
                    round,
                    i,
                    pattern,
                    seen
                );
                result = Err("kernel page");
            }
        }
        km_free(buf, NPAGES * PAGE_SIZE, &KV_ANY, &KP_PAGEABLE);
        result?;
    }
    Ok(())
}

/// A user page mapped, unmapped (which frees the page-table pages that held it, on amd64)
/// and replaced by another at the same address: `copyin` must read the new page.
fn pmap_reuse_user(pm: &MachinePmap) -> Result<(), &'static str> {
    let uva = <Machine as VmParam>::VM_MIN_ADDRESS + 0x1000_0000;
    let mut pages = [None; 2];
    let mut result = Ok(());

    for (round, slot) in pages.iter_mut().enumerate() {
        let Some(pg) = uvm_pagealloc(None, 0, None, UVM_PGA_ZERO) else {
            result = Err("uvm_pagealloc");
            break;
        };
        *slot = Some(pg);
        let pattern = PATTERN ^ (0x100 + round as u64);
        // SAFETY: a fresh page through the direct map, this test's.
        unsafe { ptr::write_volatile(pmap_map_direct(pg).as_usize() as *mut u64, pattern) };

        let pa = vm_page_to_phys(pg);
        if pmap_enter(pm, Vaddr::new(uva), pa, PROT_READ, PROT_READ | PMAP_WIRED).is_err() {
            result = Err("pmap_enter");
            break;
        }
        pmap_update(pm);
        let mut word = [0u8; 8];
        let read = copyin(uva, &mut word).map(|()| u64::from_ne_bytes(word));
        let extracted = pmap_extract(pm, Vaddr::new(uva));
        pmap_remove(pm, Vaddr::new(uva), Vaddr::new(uva + PAGE_SIZE));
        pmap_update(pm);
        if read != Ok(pattern) || extracted != Some(pa) {
            kprintf!(
                "selftest: pmap reuse: user round {}: copyin {:?}, extract {:?}, want {:#x} at {:#x}\n",
                round,
                read,
                extracted.map(Paddr::as_usize),
                pattern,
                pa.as_usize()
            );
            result = Err("user page");
            break;
        }
    }

    for pg in pages.into_iter().flatten() {
        uvm_pagefree(pg);
    }
    result
}

/// Maps, unmaps and maps again kernel and user pages while a user pmap is loaded, as a
/// process closing and reopening pipes does, and checks every mapping against what was
/// entered (`selftest: pmap reuse ok`). proc0 borrows a fresh address space for the test,
/// switching in and out of it as `uvmspace_exec` does.
pub fn pmap_reuse() {
    let Some(p) = curproc() else {
        kprintf!("selftest: pmap reuse FAILED: no curproc\n");
        return;
    };
    let ovm = p.vmspace();
    let vm = uvmspace_alloc(
        <Machine as VmParam>::VM_MIN_ADDRESS,
        <Machine as VmParam>::VM_MAXUSER_ADDRESS,
        true,
        true,
    );
    pmap_deactivate(p);
    p.p_vmspace.set(vm);
    pmap_activate(p);

    let result = pmap_reuse_kernel().and_then(|()| pmap_reuse_user(vm.vm_map.pmap()));

    pmap_deactivate(p);
    p.p_vmspace.set(ovm);
    pmap_activate(p);
    uvmspace_free(vm);

    match result {
        Ok(()) => {
            kprintf!("selftest: pmap reuse ok\n");
        }
        Err(what) => {
            kprintf!("selftest: pmap reuse FAILED: {}\n", what);
        }
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

/// The buffer cache after `bufinit`: anonymous buffers (`geteblk`) mapped in the buffer arena
/// are written and read back through `b_data`, several at once, then freed; the counters come
/// back to where they were.
pub fn buffer_cache() {
    use crate::kern::vfs_bio::{BCSTATS, brelse, geteblk};

    let numbufs = BCSTATS.numbufs.load(Ordering::Relaxed);
    let numbufpages = BCSTATS.numbufpages.load(Ordering::Relaxed);
    let mut ok = true;

    let sizes = [
        crate::sys::param::MAXPHYS,
        3 * PAGE_SIZE,
        PAGE_SIZE,
        5 * PAGE_SIZE,
    ];
    let bufs: Vec<_> = sizes.iter().map(|&sz| geteblk(sz)).collect();
    for (i, bp) in bufs.iter().enumerate() {
        // SAFETY: `geteblk` returned the buffer busy and mapped, for this test alone.
        let data = unsafe { bp.data() };
        ok &= data.len() == sizes[i];
        for (j, b) in data.iter_mut().enumerate() {
            *b = (i * 31 + j) as u8;
        }
    }
    for (i, bp) in bufs.iter().enumerate() {
        // SAFETY: as above.
        let data = unsafe { bp.data() };
        ok &= data
            .iter()
            .enumerate()
            .all(|(j, &b)| b == (i * 31 + j) as u8);
    }
    for bp in bufs {
        brelse(bp);
    }
    ok &= BCSTATS.numbufs.load(Ordering::Relaxed) == numbufs;
    ok &= BCSTATS.numbufpages.load(Ordering::Relaxed) == numbufpages;

    if ok {
        kprintf!(
            "selftest: buffer cache ok ({} kva slots, {} pages at most)\n",
            BCSTATS.kvaslots.load(Ordering::Relaxed),
            crate::conf::param::bufpages.load(Ordering::Relaxed)
        );
    } else {
        kprintf!("selftest: buffer cache FAILED\n");
    }
}

/// The pager map: three busy pages, filled through the direct map, are mapped together in a
/// pager segment (`uvm_pagermapin`, the vnode pager's path for a cluster) and read back
/// through it; a single page uses the direct map. Then everything is given back.
pub fn pager_map() {
    use crate::uvm::uvm_page::{uvm_pagealloc, uvm_pagefree};
    use crate::uvm::uvm_pager::{UVMPAGER_MAPIN_READ, uvm_pagermapin, uvm_pagermapout};

    let mut pages: Vec<&'static crate::uvm::uvm_page::VmPage> = Vec::new();
    for _ in 0..3 {
        match uvm_pagealloc(None, 0, None, 0) {
            Some(pg) => pages.push(pg),
            None => {
                kprintf!("selftest: pager map FAILED (uvm_pagealloc)\n");
                return;
            }
        }
    }
    for (i, pg) in pages.iter().enumerate() {
        pg.set_bits(crate::uvm::uvm_page::PG_BUSY);
        let va = pmap_map_direct(pg).as_usize();
        // SAFETY: a page this test just allocated, reached through the direct map.
        unsafe { ptr::write_bytes(va as *mut u8, 0x40 + i as u8, PAGE_SIZE) };
    }
    let pps: Vec<*const crate::uvm::uvm_page::VmPage> =
        pages.iter().map(|pg| ptr::from_ref(*pg)).collect();

    let mut ok = true;
    let kva = uvm_pagermapin(&pps, 3, UVMPAGER_MAPIN_READ);
    if kva == 0 {
        kprintf!("selftest: pager map FAILED (uvm_pagermapin)\n");
        ok = false;
    } else {
        for i in 0..3 {
            // SAFETY: `kva` maps the three pages in order (uvm_pagermapin).
            let b = unsafe { ptr::read_volatile((kva + i * PAGE_SIZE + 17) as *const u8) };
            ok &= b == 0x40 + i as u8;
        }
        uvm_pagermapout(kva, 3);
    }
    let one = uvm_pagermapin(&pps[2..], 1, UVMPAGER_MAPIN_READ);
    // SAFETY: the single page's mapping (the direct map).
    ok &= one != 0 && unsafe { ptr::read_volatile(one as *const u8) } == 0x42;
    uvm_pagermapout(one, 1);

    for pg in pages {
        pg.clear_bits(crate::uvm::uvm_page::PG_BUSY);
        uvm_pagefree(pg);
    }
    if ok {
        kprintf!("selftest: pager map ok\n");
    } else {
        kprintf!("selftest: pager map FAILED\n");
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

/// Unmasks interrupts on the boot CPU for a self-test that runs from `main` before the first
/// context switch and waits for an interrupt: the CPU may still mask them there, since
/// agintc(4)'s attach puts back the state it found, as in C (ampintc(4)'s enables them), and
/// no thread trampoline has run yet (M16f).
fn early_intr_enable() {
    // SAFETY: `cpu_configure` has set up the interrupt controller, and `main` may be
    // interrupted here (the level is `spl0`'s after `cpu_configure`).
    unsafe { crate::machine::cpu::intr_enable() };
}

/// Opens the console's tty through the device switch, as `/dev/console` would, then reads
/// it without blocking until the line discipline hands out a line (the receive interrupt
/// fills `sc_ibuf`, the soft interrupt runs `ttyinput`) and echoes it: the M4 exit
/// criterion (`smoke` sends the line), now through the tty layer.
pub fn uart_echo() {
    let Some(cp) = cn_tab() else {
        kprintf!("selftest: uart echo FAILED: no console\n");
        return;
    };
    let dev = cp.cn_dev.get();
    if dev == NODEV {
        kprintf!("selftest: uart echo FAILED: the console is not a tty\n");
        return;
    }
    let cdev = cdevsw(major(dev));
    if let Err(e) = (cdev.d_open)(dev, FREAD | FWRITE | FNONBLOCK, S_IFCHR as i32, &PROC0) {
        kprintf!("selftest: uart echo FAILED: open: {:?}\n", e);
        return;
    }
    // The receive interrupt is the point of the test.
    early_intr_enable();
    kprintf!("selftest: uart rx interrupt armed\n");

    let mut line = [0u8; 80];
    let mut len = 0;
    // No clock yet: a spin count long enough for the test harness to type the line.
    let mut spins: u64 = 0;
    while len < line.len() && !line[..len].contains(&b'\n') {
        let rest = &mut line[len..];
        let mut iov = [Iovec {
            iov_base: rest.as_mut_ptr().cast(),
            iov_len: rest.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: rest.len(),
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let resid = uio.uio_resid;
        match (cdev.d_read)(dev, &mut uio, IO_NDELAY) {
            Ok(()) | Err(Errno::EWOULDBLOCK) => len += resid - uio.uio_resid,
            Err(e) => {
                kprintf!("selftest: uart echo FAILED: read: {:?}\n", e);
                return;
            }
        }
        core::hint::spin_loop();
        spins += 1;
        if spins == 300_000_000 {
            kprintf!(
                "selftest: uart echo FAILED: no line received ({} bytes so far)\n",
                len
            );
            return;
        }
    }
    let end = line[..len].iter().position(|&c| c == b'\n').unwrap_or(len);
    kprintf!("selftest: uart echo: {}\n", Str(&line[..end]));
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
    // The clock interrupts are the point of the test.
    early_intr_enable();
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
/// No turn taken yet, in [`PINGPONG_CPU`].
const PINGPONG_NOCPU: u32 = u32::MAX;
/// The CPU (`ci_cpuid`) each thread (ping, pong) took its last turn on.
static PINGPONG_CPU: [AtomicU32; 2] = [const { AtomicU32::new(PINGPONG_NOCPU) }; 2];

/// With `MULTIPROCESSOR` and two CPUs running, pegs ping (`me` 0) to the first running CPU
/// and pong (`me` 1) to the second, so every turn crosses CPUs.
#[cfg(feature = "multiprocessor")]
fn pingpong_peg(me: u32) {
    use crate::kern::kern_sched::sched_peg_curproc;
    use crate::machine::cpu::{CpuInfo, cpu_info_foreach, cpu_is_running};

    let mut running: [Option<&'static CpuInfo>; 2] = [None; 2];
    let mut n = 0;
    cpu_info_foreach(&mut |ci| {
        if n < running.len() && cpu_is_running(ci) {
            running[n] = Some(ci);
            n += 1;
        }
    });
    if n == running.len()
        && let Some(ci) = running[me as usize]
    {
        sched_peg_curproc(ci);
    }
}

/// One ping-pong thread: `arg` is 1 (ping) or 2 (pong), as an address (`fork1` would
/// replace a null one with the thread). Waits for its turn under the mutex, passes the turn,
/// wakes the other, and parks forever once the rounds are done.
fn pingpong_thread(arg: *mut core::ffi::c_void) {
    let me = (arg as usize as u32) - 1;

    #[cfg(feature = "multiprocessor")]
    pingpong_peg(me);

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
        PINGPONG_CPU[me as usize].store(cpu_number(), Ordering::Relaxed);
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
    // The thread count before the two exist: after their exits are reaped it is back here.
    let nthreads_start = NTHREADS.load(Ordering::Relaxed);

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

    // Give the two exits time to reach the reaper: timed sleeps (endtsleep wakes each), 50 ms
    // at a time and at most 2 s in all, until the count is back. A loaded host (parallel
    // smokes) once left the second exit unreaped after a single 50 ms sleep. The reaper may
    // already have taken them by now, so the count is compared with the one from before
    // their creation, not with one read here.
    let nthreads_before = NTHREADS.load(Ordering::Relaxed);
    let mut nthreads_after = nthreads_before;
    for _ in 0..40 {
        let _ = tsleep_nsec(ptr::addr_of!(PINGPONG_REAP), PWAIT, "reapwait", 50_000_000);
        nthreads_after = NTHREADS.load(Ordering::Relaxed);
        if nthreads_after <= nthreads_start {
            break;
        }
    }

    let turns = PINGPONG_TURN.load(Ordering::Relaxed);
    let cpus = [
        PINGPONG_CPU[0].load(Ordering::Relaxed),
        PINGPONG_CPU[1].load(Ordering::Relaxed),
    ];
    let across = cpus[0] != cpus[1] && !cpus.contains(&PINGPONG_NOCPU);
    if turns == 2 * PINGPONG_ROUNDS && nthreads_after == nthreads_start && across {
        kprintf!(
            "selftest: kthread ping-pong ok: {} turns between tid {} and tid {} in {} us, {} context switches, both exited and reaped ({} threads left), across cpu{} and cpu{}\n",
            turns,
            ping_tid,
            pong_tid,
            elapsed_us,
            UVMEXP.swtch.load(Ordering::Relaxed),
            nthreads_after,
            cpus[0],
            cpus[1]
        );
    } else if turns == 2 * PINGPONG_ROUNDS && nthreads_after == nthreads_start {
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
            "selftest: kthread ping-pong FAILED: {} turns, {} threads at the start, {} before the reap wait, {} after\n",
            turns,
            nthreads_start,
            nthreads_before,
            nthreads_after
        );
    }
}

/// `MULTIPROCESSOR`: right after `cpu_boot_secondary_processors`, how many CPUs report
/// `CPU_IS_RUNNING` (`selftest: N cpus running`).
#[cfg(feature = "multiprocessor")]
pub fn cpus_running() {
    use crate::machine::cpu::{cpu_info_foreach, cpu_is_running};

    let mut n = 0u32;
    cpu_info_foreach(&mut |ci| {
        if cpu_is_running(ci) {
            n += 1;
        }
    });
    kprintf!("selftest: {} cpus running\n", n);
}

/// What [`clockintr_percpu`] sleeps on; nothing wakes it.
#[cfg(feature = "multiprocessor")]
static CLOCKINTR_PERCPU_WCHAN: u8 = 0;

/// `MULTIPROCESSOR` (M11b): every running CPU dispatches its own clock interrupts, reading
/// the uptime through the timecounter there, and the uptime never goes back on any of them
/// (`kern_clockintr.rs`'s check under feature `qemu`). Waits up to two seconds for each CPU
/// to check 20 more uptimes, then prints a line per CPU and `selftest: clockintr on N cpus
/// ok, uptime monotonic on each`.
#[cfg(feature = "multiprocessor")]
pub fn clockintr_percpu() {
    use crate::kern::kern_clockintr::uptime_check_counts;
    use crate::machine::cpu::{Cpu, cpu_info_foreach, cpu_is_running};

    const MORE: u64 = 20;

    // (cpu_number, checks so far) for every running CPU.
    let mut cpus: Vec<(u32, u64)> = Vec::new();
    cpu_info_foreach(&mut |ci| {
        if cpu_is_running(ci) {
            let id = Machine::ci_cpuid(ci);
            cpus.push((id, uptime_check_counts(id).0));
        }
    });
    for _ in 0..200 {
        if cpus
            .iter()
            .all(|&(id, base)| uptime_check_counts(id).0 >= base + MORE)
        {
            break;
        }
        let _ = tsleep_nsec(
            ptr::addr_of!(CLOCKINTR_PERCPU_WCHAN),
            PWAIT,
            "clkpcpu",
            10_000_000,
        );
    }

    let mut ticking = 0;
    let mut behind_total = 0;
    for &(id, base) in &cpus {
        let (checks, behind) = uptime_check_counts(id);
        kprintf!(
            "selftest: cpu{} clockintr: {} uptime checks, {} behind\n",
            id,
            checks - base,
            behind
        );
        if checks >= base + MORE {
            ticking += 1;
        }
        behind_total += behind;
    }
    if ticking == cpus.len() && behind_total == 0 {
        kprintf!(
            "selftest: clockintr on {} cpus ok, uptime monotonic on each\n",
            ticking
        );
    } else {
        kprintf!(
            "selftest: clockintr FAILED: {} of {} cpus ticking, {} uptimes behind\n",
            ticking,
            cpus.len(),
            behind_total
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

/// The address the ping self-test gives the interface: QEMU's user-mode network (slirp)
/// assigns 10.0.2.15 to its guest.
const PING_ADDR: [u8; 4] = [10, 0, 2, 15];
/// The gateway and the address pinged: slirp's router, 10.0.2.2, answers ICMP echo.
const PING_GATEWAY: [u8; 4] = [10, 0, 2, 2];
/// The echo payload, as ping(8)'s default.
const PING_DATALEN: usize = 56;
/// How long the self-test waits for the reply: 30 tries of 100 ms.
const PING_TRIES: u32 = 30;
/// What the ping self-test sleeps on (nothing wakes it: the sleeps time out).
static PING_WCHAN: u8 = 0;

/// A `sockaddr_in` for `a`.
fn ping_sin(a: [u8; 4]) -> crate::netinet::in_::SockaddrIn {
    crate::netinet::in_::SockaddrIn {
        sin_len: size_of::<crate::netinet::in_::SockaddrIn>() as u8,
        sin_family: crate::sys::socket::AF_INET,
        sin_addr: crate::netinet::in_::InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..Default::default()
    }
}

/// The M7b network check, on every default boot: what `ifconfig vio0 10.0.2.15/24 up`,
/// `route add default 10.0.2.2` and `ping -c 1 10.0.2.2` do, from the kernel. The first
/// Ethernet interface (`IFT_ETHER` on `ifnetlist`) gets 10.0.2.15/24 through `ifioctl`
/// (`SIOCAIFADDR`, which reaches `in_ioctl`, then `SIOCSIFFLAGS` with `IFF_UP`), a default
/// route through `rtrequest(RTM_ADD)`, and an ICMP echo request goes to 10.0.2.2 through
/// `ip_output`, which resolves the gateway with ARP. The test then sleeps until `icmp_input`
/// counts an echo reply (`icps_inhist[ICMP_ECHOREPLY]`), three seconds at most.
pub fn ping_gateway() {
    use crate::net::if_::{IFF_UP, IFNETLIST, ifioctl};
    use crate::net::if_types::IFT_ETHER;
    use crate::net::route::{
        RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_GATEWAY, RTF_STATIC, RTM_ADD, RtAddrinfo, rtfree,
        rtrequest,
    };
    use crate::netinet::icmp_var::IcmpstatCounters;
    use crate::netinet::in_::{IPPROTO_ICMP, sintosa};
    use crate::netinet::in_var::InAliasreq;
    use crate::netinet::ip::{Ip, MAXTTL};
    use crate::netinet::ip_icmp::{ICMP_ECHO, ICMP_ECHOREPLY, ICMPCOUNTERS, IcmpPkt};
    use crate::netinet::ip_output::ip_output;
    use crate::netinet::ip_var::mtod_ip_store;
    use crate::sys::endian::htons;
    use crate::sys::mbuf::{M_ICMP_CSUM_OUT, MHLEN};
    use crate::sys::sockio::{SIOCAIFADDR, SIOCSIFFLAGS};
    use crate::sys::systm::{net_lock, net_unlock};

    let Some(ifp) = IFNETLIST
        .0
        .iter()
        .find(|ifp| ifp.if_type.get() == IFT_ETHER)
    else {
        kprintf!("selftest: ping skipped: no interface\n");
        return;
    };
    let xname = ifp.if_xname.get();
    let Some(p) = curproc() else {
        kprintf!("selftest: ping 10.0.2.2: FAILED: no process context\n");
        return;
    };
    // ARP reads an expiry time of 0 as "permanent" (`arpresolve`), and a route's expiry is the
    // uptime in seconds when it was made: as on any OpenBSD system, networking is configured
    // after the first second of uptime.
    while getuptime() == 0 {
        let _ = tsleep_nsec(ptr::addr_of!(PING_WCHAN), PWAIT, "uptime", 100_000_000);
    }

    // ifconfig <if> inet 10.0.2.15/24
    let mut ifra = InAliasreq::zeroed();
    ifra.ifra_name = xname;
    *ifra.ifra_addr_mut() = ping_sin(PING_ADDR);
    ifra.ifra_mask = ping_sin([255, 255, 255, 0]);
    // SAFETY: `ifra` is a `struct in_aliasreq`, what SIOCAIFADDR takes; the socket is the
    // kernel's own (NULL).
    let error = unsafe { ifioctl(ptr::null(), SIOCAIFADDR, ptr::from_mut(&mut ifra).cast(), p) };
    if let Err(e) = error {
        kprintf!(
            "selftest: ping 10.0.2.2: FAILED: SIOCAIFADDR on {}: error {}\n",
            Str(&xname),
            e as i32
        );
        return;
    }

    // ... up
    let mut ifr = crate::net::if_::Ifreq::zeroed();
    ifr.ifr_name = xname;
    ifr.set_ifr_flags((ifp.if_flags.get() | IFF_UP) as i16);
    // SAFETY: `ifr` is a `struct ifreq`, what SIOCSIFFLAGS takes.
    let error = unsafe { ifioctl(ptr::null(), SIOCSIFFLAGS, ptr::from_mut(&mut ifr).cast(), p) };
    if let Err(e) = error {
        kprintf!(
            "selftest: ping 10.0.2.2: FAILED: SIOCSIFFLAGS on {}: error {}\n",
            Str(&xname),
            e as i32
        );
        return;
    }

    // route add default 10.0.2.2
    let mut dst = ping_sin([0, 0, 0, 0]);
    let mut mask = ping_sin([0, 0, 0, 0]);
    let mut gw = ping_sin(PING_GATEWAY);
    let mut info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = sintosa(&mut dst);
    info.rti_info[RTAX_NETMASK] = sintosa(&mut mask);
    info.rti_info[RTAX_GATEWAY] = sintosa(&mut gw);
    info.rti_flags = RTF_GATEWAY | RTF_STATIC;
    net_lock();
    // The interface address whose subnet holds the gateway (route(8)'s rtm_getifa).
    // SAFETY: a local `sockaddr_in`.
    info.rti_ifa = unsafe { crate::net::if_::ifaof_ifpforaddr(sintosa(&mut gw), ifp) };
    let mut rt = None;
    // SAFETY: the addresses are locals that live across the call.
    let error = unsafe { rtrequest(RTM_ADD, &mut info, 0, Some(&mut rt), 0) };
    rtfree(rt);
    net_unlock();
    if let Err(e) = error {
        kprintf!(
            "selftest: ping 10.0.2.2: FAILED: default route: error {}\n",
            e as i32
        );
        return;
    }

    // ping -c 1 10.0.2.2: an echo request with a skeletal IP header for ip_output.
    let before = ICMPCOUNTERS[IcmpstatCounters::IcpsInhist as usize + usize::from(ICMP_ECHOREPLY)]
        .load(Ordering::Relaxed);
    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        kprintf!("selftest: ping 10.0.2.2: FAILED: no mbuf\n");
        return;
    };
    let hlen = size_of::<Ip>();
    let len = hlen + 8 + PING_DATALEN;
    // Leave room in front for the link header, as a socket's send would.
    let lead = MHLEN - len;
    m.m_data().set(m.m_data().get().wrapping_add(lead & !7));
    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len as i32);
    m.m_pkthdr().ph_rtableid.set(0);
    let ip = Ip {
        ip_len: htons(len as u16),
        ip_ttl: MAXTTL,
        ip_p: IPPROTO_ICMP as u8,
        ip_dst: ping_sin(PING_GATEWAY).sin_addr,
        ..Ip::default()
    };
    mtod_ip_store(m, &ip);
    let icp = IcmpPkt::of(m, hlen);
    icp.set_icmp_type(ICMP_ECHO);
    icp.set_icmp_code(0);
    icp.set_icmp_cksum(0);
    icp.set_icmp_id(htons(0x4242));
    icp.set_icmp_seq(htons(1));
    for i in 0..PING_DATALEN {
        // SAFETY: the mbuf holds `len` bytes at its data, the payload after the headers.
        unsafe { *crate::sys::mbuf::mtod::<u8>(m).add(hlen + 8 + i) = i as u8 };
    }
    // The checksum is computed on output, as icmp_send asks for it.
    m.m_pkthdr().csum_flags.set(M_ICMP_CSUM_OUT);

    net_lock();
    let error = ip_output(m, None, None, 0, None, None, 0);
    net_unlock();
    if let Err(e) = error {
        kprintf!(
            "selftest: ping 10.0.2.2: FAILED: ip_output: error {}\n",
            e as i32
        );
        return;
    }

    let counter =
        &ICMPCOUNTERS[IcmpstatCounters::IcpsInhist as usize + usize::from(ICMP_ECHOREPLY)];
    for _ in 0..PING_TRIES {
        if counter.load(Ordering::Relaxed) != before {
            kprintf!("selftest: ping 10.0.2.2: echo reply received\n");
            return;
        }
        let _ = tsleep_nsec(ptr::addr_of!(PING_WCHAN), PWAIT, "ping", 100_000_000);
    }
    kprintf!(
        "selftest: ping 10.0.2.2: FAILED: no echo reply on {} within {} ms\n",
        Str(&xname),
        PING_TRIES * 100
    );
}

/// The ARP request [`vio_check`] sends: who has 10.0.2.2 (QEMU's user-mode gateway), tell
/// 10.0.2.15, from `enaddr`, broadcast; padded to the 60 bytes of a minimal frame. Built by
/// hand, so the check needs no IPv4 address (`arprequest` sends from one).
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
/// in through the receive interrupt (`vio_rx_intr`); `ether_input` then hands it to
/// `arpinput`, which drops it: `vio0` has no IPv4 address yet.
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

/// rd(4) and the disk layer: `rd0a` opens through the block device switch (which reads the
/// image's disklabel through `rdstrategy`), and the FFS superblock is read through the
/// switch's strategy: `rd0: <N> bytes, ffs magic ok` names the image's size, or `rd: no
/// ramdisk module` (from `stand`) and `rd0: no image` when the boot image carries none (a
/// checkout without `just userland`).
pub fn rd_check() {
    use crate::dev::rd::rd_root_size;
    use crate::kern::vfs_bio::{biowait, brelse, geteblk};
    use crate::machine::autoconf::nam2blk;
    use crate::machine::conf::bdevsw;
    use crate::sys::buf::{B_BUSY, B_DONE, B_ERROR, B_INVAL, B_RAW, B_READ, B_WRITE};
    use crate::sys::disklabel::makediskdev;
    use crate::sys::stat::S_IFBLK;

    // `<ufs/ffs/fs.h>`: where the superblock lives and its magic numbers. The file system
    // itself is the ffs port's; only these numbers are needed to recognise the image.
    const SBLOCKSIZE: usize = 8192;
    const FS_MAGIC_OFFSET: usize = 1372;
    const SBLOCKS: [(i64, u32, &str); 2] = [
        (65536, 0x1954_0119, "ffs2"), // SBLOCK_UFS2, FS_UFS2_MAGIC
        (8192, 0x0001_1954, "ffs1"),  // SBLOCK_UFS1, FS_UFS1_MAGIC
    ];

    let size = rd_root_size();
    if size == 0 {
        kprintf!("rd0: no image\n");
        return;
    }
    let Some(maj) = nam2blk()
        .iter()
        .find(|n| n.name == b"rd")
        .map(|n| n.maj as u32)
    else {
        kprintf!("rd0: no block major\n");
        return;
    };
    let dev = makediskdev(maj, 0, 0); // rd0a
    let sw = bdevsw(maj);
    if let Err(e) = (sw.d_open)(dev, FREAD, S_IFBLK as i32, &PROC0) {
        kprintf!("rd0: {} bytes, open rd0a: error {}\n", size, e as i32);
        return;
    }

    let bp = geteblk(SBLOCKSIZE);
    bp.b_dev.set(dev);
    let mut found = None;
    for (sblock, magic, name) in SBLOCKS {
        bp.b_blkno.set(sblock / 512);
        bp.b_bcount.set(SBLOCKSIZE as i64);
        bp.b_error.set(None);
        bp.clr(B_READ | B_WRITE | B_DONE | B_ERROR);
        bp.set(B_BUSY | B_READ | B_RAW);
        (sw.d_strategy)(bp);
        if biowait(bp).is_err() || bp.b_resid.get() != 0 {
            continue;
        }
        // SAFETY: `geteblk` returned the buffer busy and mapped, for this test alone.
        let data = unsafe { bp.data() };
        let at = &data[FS_MAGIC_OFFSET..FS_MAGIC_OFFSET + 4];
        if u32::from_ne_bytes([at[0], at[1], at[2], at[3]]) == magic {
            found = Some((sblock, name));
            break;
        }
    }
    bp.set(B_INVAL);
    brelse(bp);
    let _ = (sw.d_close)(dev, FREAD, S_IFBLK as i32, Some(&PROC0));

    match found {
        Some((sblock, name)) => {
            kprintf!(
                "rd0: {} bytes, ffs magic ok ({} superblock at {})\n",
                size,
                name,
                sblock
            );
        }
        None => {
            kprintf!("rd0: {} bytes, no ffs superblock on rd0a\n", size);
        }
    }
}

// selftest=mpstress: M11a's exit test, pool(9) and uvm_pmemrange on every CPU at once, and
// M11e's uvm stress (page faults, amaps and anons, the page queues, the pmap locks and the
// TLB shootdowns).

/// The most CPUs the stress runs a thread on.
const MPSTRESS_MAXCPUS: usize = 64;
/// Pool rounds per thread.
const MPSTRESS_POOL_ROUNDS: u32 = 1500;
/// Items taken from each pool per round.
const MPSTRESS_POOL_ITEMS: usize = 24;
/// Page list rounds per thread.
const MPSTRESS_PMR_ROUNDS: u32 = 600;
/// Page lists held at once per round.
const MPSTRESS_PMR_LISTS: usize = 6;
/// uvm rounds per thread.
const MPSTRESS_UVM_ROUNDS: u32 = 1000;
/// Pages per thread per uvm round, at most, in pageable kernel memory and in the thread's
/// slice of the shared anonymous map.
const MPSTRESS_UVM_PAGES: usize = 4;
/// Exchange slots per pool, through which items cross CPUs.
const MPSTRESS_XCHG: usize = 32;
/// The item sizes of the stress pools: a cache item's minimum, a mid size, and one that
/// needs the multi-page allocator.
const MPSTRESS_SIZES: [usize; 3] = [32, 256, 2048];

/// The stress pools: 32 bytes at `IPL_VM` (mutex), 256 bytes `PR_RWLOCK` (rwlock, only
/// `PR_WAITOK` gets), 2048 bytes `PR_WAITOK` (`pool_allocator_multi_ni`, which takes the
/// kernel lock).
static MPSTRESS_POOLS: [Pool; 3] = [const { Pool::new() }; 3];
/// Items in transit between CPUs, per pool.
static MPSTRESS_SLOTS: [[AtomicPtr<u8>; MPSTRESS_XCHG]; 3] =
    [const { [const { AtomicPtr::new(ptr::null_mut()) }; MPSTRESS_XCHG] }; 3];
/// The barrier's interlock.
static MPSTRESS_MTX: Mutex = Mutex::new(IPL_NONE);
/// The phase the threads may run: 0 wait, 1 pool, 2 pmemrange, 3 uvm, 4 exit.
static MPSTRESS_PHASE: AtomicU32 = AtomicU32::new(0);
/// Threads arrived at each barrier (ready, pool done, pmemrange done, uvm done).
static MPSTRESS_ARRIVED: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
/// Threads that took their last step before exiting.
static MPSTRESS_EXITED: AtomicU32 = AtomicU32::new(0);
/// The first failure, an index into [`MPSTRESS_MSG`] (`usize::MAX`: none).
static MPSTRESS_FAILED: AtomicUsize = AtomicUsize::new(usize::MAX);
/// How many threads run.
static MPSTRESS_NTHREADS: AtomicU32 = AtomicU32::new(0);
/// The CPU each thread is pegged to (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
static MPSTRESS_CPUS: [AtomicPtr<CpuInfo>; MPSTRESS_MAXCPUS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; MPSTRESS_MAXCPUS];
/// The CPU number each thread saw itself on.
static MPSTRESS_RAN_ON: [AtomicU32; MPSTRESS_MAXCPUS] =
    [const { AtomicU32::new(u32::MAX) }; MPSTRESS_MAXCPUS];
/// Counters: pool gets, `PR_NOWAIT` gets refused, items that crossed CPUs, page lists,
/// pages, page lists refused (`UVM_PLA_NOWAIT`), pageable kernel pages faulted, anonymous
/// pages faulted, slices unmapped and mapped again.
static MPSTRESS_STATS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
/// `uvmexp.free` when the pmemrange phase started and when it ended.
static MPSTRESS_FREE: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
/// The uvm phase's start and end (`nsecuptime`).
static MPSTRESS_UVM_NS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
/// `uvmexp.pcphit` and `uvmexp.pcpmiss` (the per-CPU page caches) when the uvm phase started
/// and when it ended.
static MPSTRESS_PCP: [[AtomicI32; 2]; 2] = [const { [const { AtomicI32::new(0) }; 2] }; 2];
/// The address space whose anonymous map the uvm phase's threads share.
static MPSTRESS_VM: AtomicPtr<Vmspace> = AtomicPtr::new(ptr::null_mut());

/// The failure messages: the pool's first, two, then the pmemrange's three, then the CPU's,
/// then the uvm phase's five.
static MPSTRESS_MSG: [&str; 11] = [
    "pool_get(PR_WAITOK) returned nothing",
    "a pool item's contents changed while it was out",
    "a page list's page is outside its constraint or misaligned",
    "a page's contents changed while it was allocated",
    "uvm_pglistalloc(UVM_PLA_WAITOK) failed",
    "a thread ran on the wrong CPU",
    "a pageable kernel page does not hold what was written through its mapping",
    "km_alloc(kp_pageable) failed",
    "an anonymous page is not mapped where it was faulted in, or does not hold its tag",
    "uvm_fault on the shared anonymous map failed",
    "the shared anonymous map (or a thread's slice of it) could not be mapped",
];

/// Records the first failure.
fn mpstress_fail(which: usize) {
    let _ =
        MPSTRESS_FAILED.compare_exchange(usize::MAX, which, Ordering::Relaxed, Ordering::Relaxed);
}

/// A small per-thread random stream (xorshift).
fn mpstress_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// The words of a pool item of `size` bytes.
fn mpstress_words(p: NonNull<u8>, size: usize) -> &'static [AtomicU64] {
    // SAFETY: `size` bytes of an item this test holds, 8-byte aligned (pool items are).
    unsafe { core::slice::from_raw_parts(p.as_ptr().cast::<AtomicU64>(), size / 8) }
}

/// Fills an item with `tag`: word `k` holds `tag ^ k`.
fn mpstress_fill(p: NonNull<u8>, size: usize, tag: u64) {
    for (k, w) in mpstress_words(p, size).iter().enumerate() {
        w.store(tag ^ k as u64, Ordering::Relaxed);
    }
}

/// Whether an item still holds the tag in its first word everywhere.
fn mpstress_check(p: NonNull<u8>, size: usize) -> bool {
    let words = mpstress_words(p, size);
    let tag = words[0].load(Ordering::Relaxed);
    words
        .iter()
        .enumerate()
        .all(|(k, w)| w.load(Ordering::Relaxed) == tag ^ k as u64)
}

/// Waits at barrier `b` until `PHASE` reaches `next`; the last thread to arrive runs `last`
/// and moves the phase on.
fn mpstress_barrier(b: usize, next: u32, last: fn()) {
    let n = MPSTRESS_NTHREADS.load(Ordering::Relaxed);
    mtx_enter(&MPSTRESS_MTX);
    if MPSTRESS_ARRIVED[b].fetch_add(1, Ordering::Relaxed) + 1 == n {
        last();
        MPSTRESS_PHASE.store(next, Ordering::Release);
        wakeup(ptr::addr_of!(MPSTRESS_PHASE));
    }
    while MPSTRESS_PHASE.load(Ordering::Acquire) < next {
        let _ = msleep_nsec(
            ptr::addr_of!(MPSTRESS_PHASE),
            &MPSTRESS_MTX,
            PWAIT,
            "mpstress",
            INFSLP,
        );
    }
    mtx_leave(&MPSTRESS_MTX);
}

/// The end of the pool phase, on the last thread to finish it: the items left in transit go
/// back, and the pmemrange phase starts from the free page count read now.
fn mpstress_pool_done() {
    for (pi, pp) in MPSTRESS_POOLS.iter().enumerate() {
        for slot in &MPSTRESS_SLOTS[pi] {
            if let Some(p) = NonNull::new(slot.swap(ptr::null_mut(), Ordering::AcqRel)) {
                if !mpstress_check(p, MPSTRESS_SIZES[pi]) {
                    mpstress_fail(1);
                }
                pool_put(pp, p);
            }
        }
    }
    MPSTRESS_FREE[0].store(UVMEXP.free.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// The end of the pmemrange phase, on the last thread: every thread is parked, so nothing
/// else allocates or frees pages for the test. The uvm phase starts.
fn mpstress_pmr_done() {
    MPSTRESS_FREE[1].store(UVMEXP.free.load(Ordering::Relaxed), Ordering::Relaxed);
    MPSTRESS_PCP[0][0].store(UVMEXP.pcphit.load(Ordering::Relaxed), Ordering::Relaxed);
    MPSTRESS_PCP[0][1].store(UVMEXP.pcpmiss.load(Ordering::Relaxed), Ordering::Relaxed);
    MPSTRESS_UVM_NS[0].store(nsecuptime(), Ordering::Relaxed);
}

/// The end of the uvm phase, on the last thread.
fn mpstress_uvm_done() {
    MPSTRESS_UVM_NS[1].store(nsecuptime(), Ordering::Relaxed);
    MPSTRESS_PCP[1][0].store(UVMEXP.pcphit.load(Ordering::Relaxed), Ordering::Relaxed);
    MPSTRESS_PCP[1][1].store(UVMEXP.pcpmiss.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Nothing to do at the start barrier.
fn mpstress_nothing() {}

/// The pool phase of one thread: each round takes items from the three pools (`PR_WAITOK`
/// and `PR_NOWAIT`), tags them, passes some to other CPUs through the exchange slots and
/// takes theirs, checks every item and gives them all back.
fn mpstress_pool_phase(me: usize, rand: &mut u64) {
    let mut held: [[Option<NonNull<u8>>; MPSTRESS_POOL_ITEMS]; 3] =
        [[None; MPSTRESS_POOL_ITEMS]; 3];
    for round in 0..MPSTRESS_POOL_ROUNDS {
        for (pi, pp) in MPSTRESS_POOLS.iter().enumerate() {
            let size = MPSTRESS_SIZES[pi];
            for (i, h) in held[pi].iter_mut().enumerate() {
                let waitok = pi == 1 || mpstress_rand(rand) & 1 == 0;
                let flags = if waitok { PR_WAITOK } else { PR_NOWAIT };
                let flags = if i % 5 == 0 { flags | PR_ZERO } else { flags };
                let Some(p) = pool_get(pp, flags) else {
                    if waitok {
                        mpstress_fail(0);
                    } else {
                        MPSTRESS_STATS[1].fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                };
                MPSTRESS_STATS[0].fetch_add(1, Ordering::Relaxed);
                if flags & PR_ZERO != 0
                    && mpstress_words(p, size)
                        .iter()
                        .any(|w| w.load(Ordering::Relaxed) != 0)
                {
                    mpstress_fail(1);
                }
                let tag = ((me as u64) << 56) | (u64::from(round) << 32) | ((pi * 64 + i) as u64);
                mpstress_fill(p, size, tag);
                *h = Some(p);
            }
        }

        // Hand some items to the other CPUs and take theirs.
        for (pi, items) in held.iter_mut().enumerate() {
            for h in items.iter_mut().take(4) {
                let slot = &MPSTRESS_SLOTS[pi][(mpstress_rand(rand) as usize) % MPSTRESS_XCHG];
                let theirs =
                    slot.swap(h.map_or(ptr::null_mut(), NonNull::as_ptr), Ordering::AcqRel);
                if NonNull::new(theirs).is_some() {
                    MPSTRESS_STATS[2].fetch_add(1, Ordering::Relaxed);
                }
                *h = NonNull::new(theirs);
            }
        }

        // Check everything and give it back, in a shuffled order.
        for (pi, items) in held.iter_mut().enumerate() {
            let start = (mpstress_rand(rand) as usize) % MPSTRESS_POOL_ITEMS;
            for k in 0..MPSTRESS_POOL_ITEMS {
                if let Some(p) = items[(start + k) % MPSTRESS_POOL_ITEMS].take() {
                    if !mpstress_check(p, MPSTRESS_SIZES[pi]) {
                        mpstress_fail(1);
                    }
                    pool_put(&MPSTRESS_POOLS[pi], p);
                }
            }
        }
    }
}

/// The constraints the pmemrange phase asks for: (low, high, alignment, boundary, maxseg)
/// with the sizes in pages taken from the round.
const MPSTRESS_PMR_CONSTRAINTS: [(usize, usize, usize, usize, i32); 5] = [
    (0, usize::MAX, 0, 0, 0),
    (0, 0xffff_ffff, 0, 0, 1),
    (0, usize::MAX, 16 * PAGE_SIZE, 0, 1),
    (0, 0xffff_ffff, 0, 8 * PAGE_SIZE, 0),
    (0, usize::MAX, 4 * PAGE_SIZE, 0, 2),
];

/// The pmemrange phase of one thread: each round holds several page lists of varied sizes
/// and constraints at once, writes a tag into every page through the direct map, checks the
/// pages lie inside their constraint, checks every tag, and frees the lists.
fn mpstress_pmr_phase(me: usize, rand: &mut u64) {
    let lists: [Pglist; MPSTRESS_PMR_LISTS] = [const { Pglist::new() }; MPSTRESS_PMR_LISTS];
    for l in &lists {
        l.init();
    }
    for round in 0..MPSTRESS_PMR_ROUNDS {
        for (li, pgl) in lists.iter().enumerate() {
            let (low, high, align, boundary, maxseg) =
                MPSTRESS_PMR_CONSTRAINTS[(mpstress_rand(rand) as usize) % 5];
            let npages = 1 + (mpstress_rand(rand) as usize) % 12;
            let npages = if boundary != 0 { npages.min(8) } else { npages };
            let maxseg = if maxseg == 0 { npages as i32 } else { maxseg };
            let npages = if maxseg == 2 { npages.min(8) } else { npages };
            let waitok = li % 3 == 0;
            let mut flags = if waitok {
                UVM_PLA_WAITOK
            } else {
                UVM_PLA_NOWAIT
            };
            if li % 4 == 1 {
                flags |= UVM_PLA_ZERO;
            }
            if uvm_pglistalloc(
                npages * PAGE_SIZE,
                Paddr::new(low),
                Paddr::new(high),
                Paddr::new(align),
                Paddr::new(boundary),
                pgl,
                maxseg,
                flags,
            )
            .is_err()
            {
                if waitok {
                    mpstress_fail(4);
                } else {
                    MPSTRESS_STATS[5].fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            MPSTRESS_STATS[3].fetch_add(1, Ordering::Relaxed);

            let mut n = 0usize;
            for (k, pg) in pgl.iter().enumerate() {
                let pa = vm_page_to_phys(pg).as_usize();
                if pa < low
                    || pa + (PAGE_SIZE - 1) > high
                    || (k == 0 && align != 0 && !pa.is_multiple_of(align))
                {
                    mpstress_fail(2);
                }
                let va = pmap_map_direct(pg).as_usize();
                // SAFETY: the page is this test's; its direct-map address covers PAGE_SIZE
                // bytes.
                let words =
                    unsafe { core::slice::from_raw_parts(va as *const AtomicU64, PAGE_SIZE / 8) };
                if flags & UVM_PLA_ZERO != 0 && words.iter().any(|w| w.load(Ordering::Relaxed) != 0)
                {
                    mpstress_fail(3);
                }
                let tag = ((me as u64) << 56) | (u64::from(round) << 32) | ((li * 64 + k) as u64);
                for step in [0, 1, 255, 511] {
                    words[step].store(tag ^ step as u64, Ordering::Relaxed);
                }
                n += 1;
            }
            MPSTRESS_STATS[4].fetch_add(n as u64, Ordering::Relaxed);
        }

        for (li, pgl) in lists.iter().enumerate() {
            for (k, pg) in pgl.iter().enumerate() {
                let va = pmap_map_direct(pg).as_usize();
                // SAFETY: as above.
                let words =
                    unsafe { core::slice::from_raw_parts(va as *const AtomicU64, PAGE_SIZE / 8) };
                let tag = ((me as u64) << 56) | (u64::from(round) << 32) | ((li * 64 + k) as u64);
                if [0, 1, 255, 511]
                    .iter()
                    .any(|&s| words[s].load(Ordering::Relaxed) != tag ^ s as u64)
                {
                    mpstress_fail(3);
                }
            }
            if !pgl.is_empty() {
                // uvm_pmr_freepageq takes every page off the list.
                uvm_pglistfree(pgl);
            }
        }
    }
}

/// Where the shared anonymous map of the uvm phase starts.
fn mpstress_uva() -> usize {
    <Machine as VmParam>::VM_MIN_ADDRESS + 0x1000_0000
}

/// The flags of the shared anonymous map and of each slice mapped again: private
/// (copy-on-write) anonymous memory, read and write, at the address given.
fn mpstress_uvm_flags() -> u32 {
    uvm_mapflag(
        PROT_READ | PROT_WRITE,
        PROT_MASK,
        MAP_INHERIT_COPY,
        MADV_NORMAL,
        UVM_FLAG_COPYONW | UVM_FLAG_FIXED,
    )
}

/// Writes `v` into the first word of the managed page at `pa`, through the direct map;
/// false when `pa` is not a managed page.
fn mpstress_write_phys(pa: Paddr, v: u64) -> bool {
    let Some(pg) = PHYS_TO_VM_PAGE(pa) else {
        return false;
    };
    // SAFETY: the direct map covers every managed page; a word-aligned write into a page this
    // thread's slice owns.
    unsafe { ptr::write_volatile(pmap_map_direct(pg).as_usize() as *mut u64, v) };
    true
}

/// The uvm phase of one thread. Each round faults pageable kernel memory in through the trap
/// path (`km_alloc(kp_pageable)` written through its own mapping: `uvm_fault` on
/// `kernel_map`, the kernel object, `pmap_enter` of the kernel pmap, without the kernel
/// lock), checks every page through its mapping and through the page `pmap_extract` reports,
/// and frees it (`km_free`: `pmap_remove` of the kernel pmap, which shoots the range down on
/// every CPU, and `uvm_km_pgremove`; a CPU left with a stale TLB entry reads another thread's
/// tag the next time the addresses come back). Then it faults its slice of the shared
/// anonymous map in with `uvm_fault` (the map lock, `amap_copy`, anons, the page queues,
/// `pmap_enter` under the user pmap's lock), tags each page through the direct map, faults
/// again and checks that the mapping and the tags held, and unmaps and maps the slice again
/// (`uvm_unmap`, `uvm_mapanon(UVM_FLAG_FIXED)`).
fn mpstress_uvm_phase(me: usize, rand: &mut u64) {
    // SAFETY: `mpstress` made the address space before starting the threads and frees it
    // after they all exited.
    let Some(vm) = (unsafe { MPSTRESS_VM.load(Ordering::Acquire).as_ref() }) else {
        mpstress_fail(10);
        return;
    };
    let map = &vm.vm_map;
    let pm = map.pmap();
    let size = MPSTRESS_UVM_PAGES * PAGE_SIZE;
    let slice = mpstress_uva() + me * size;

    for round in 0..MPSTRESS_UVM_ROUNDS {
        let tag = ((me as u64) << 56) | (u64::from(round) << 32);

        // Pageable kernel memory.
        let npages = 1 + (mpstress_rand(rand) as usize) % MPSTRESS_UVM_PAGES;
        let Some(buf) = km_alloc(npages * PAGE_SIZE, &KV_ANY, &KP_PAGEABLE, &KD_WAITOK) else {
            mpstress_fail(7);
            continue;
        };
        for i in 0..npages {
            let va = buf.as_ptr() as usize + i * PAGE_SIZE;
            // SAFETY: the first and last words of a page of the pageable allocation just
            // made; the first write faults it in.
            unsafe {
                ptr::write_volatile(va as *mut u64, tag ^ i as u64);
                ptr::write_volatile((va + PAGE_SIZE - 8) as *mut u64, !(tag ^ i as u64));
            }
        }
        for i in 0..npages {
            let va = buf.as_ptr() as usize + i * PAGE_SIZE;
            let want = tag ^ i as u64;
            // SAFETY: as above.
            let (first, last) = unsafe {
                (
                    ptr::read_volatile(va as *const u64),
                    ptr::read_volatile((va + PAGE_SIZE - 8) as *const u64),
                )
            };
            let phys = pmap_extract(pmap_kernel(), Vaddr::new(va)).and_then(read_phys);
            if first != want || last != !want || phys != Some(want) {
                mpstress_fail(6);
            }
        }
        km_free(buf, npages * PAGE_SIZE, &KV_ANY, &KP_PAGEABLE);
        MPSTRESS_STATS[6].fetch_add(npages as u64, Ordering::Relaxed);

        // The thread's slice of the shared anonymous map.
        let mut pas = [None; MPSTRESS_UVM_PAGES];
        for (i, slot) in pas.iter_mut().enumerate() {
            let va = slice + i * PAGE_SIZE;
            if uvm_fault(map, va, VM_FAULT_INVALID, PROT_READ | PROT_WRITE).is_err() {
                mpstress_fail(9);
                continue;
            }
            *slot = pmap_extract(pm, Vaddr::new(va));
            if !slot.is_some_and(|pa| mpstress_write_phys(pa, tag ^ (0x100 + i as u64))) {
                mpstress_fail(8);
            }
        }
        for (i, pa) in pas.iter().enumerate() {
            let va = slice + i * PAGE_SIZE;
            if uvm_fault(map, va, VM_FAULT_INVALID, PROT_READ).is_err() {
                mpstress_fail(9);
                continue;
            }
            let now = pmap_extract(pm, Vaddr::new(va));
            if pa.is_none()
                || now != *pa
                || now.and_then(read_phys) != Some(tag ^ (0x100 + i as u64))
            {
                mpstress_fail(8);
            }
        }
        MPSTRESS_STATS[7].fetch_add(MPSTRESS_UVM_PAGES as u64, Ordering::Relaxed);

        uvm_unmap(map, slice, slice + size);
        let mut addr = slice;
        if uvm_mapanon(map, &mut addr, size, 0, mpstress_uvm_flags()).is_err() || addr != slice {
            mpstress_fail(10);
            return;
        }
        MPSTRESS_STATS[8].fetch_add(1, Ordering::Relaxed);
    }
}

/// One stress thread: `arg` is its index plus one. Pegged to its CPU (`MULTIPROCESSOR`), it
/// drops the kernel lock (pool(9) and uvm_pmemrange are MPSAFE), waits for the start, runs
/// the pool phase, waits for every thread, runs the pmemrange phase, waits, runs the uvm
/// phase, waits again, and exits.
fn mpstress_thread(arg: *mut core::ffi::c_void) {
    let me = arg.addr() - 1;

    #[cfg(feature = "multiprocessor")]
    {
        let ci = MPSTRESS_CPUS[me].load(Ordering::Relaxed);
        // SAFETY: a non-null entry is a `cpu_info` the boot code made, alive forever.
        if let Some(ci) = unsafe { ci.as_ref() } {
            crate::kern::kern_sched::sched_peg_curproc(ci);
        }
    }
    crate::sys::systm::kernel_unlock();
    MPSTRESS_RAN_ON[me].store(cpu_number(), Ordering::Relaxed);

    let mut rand = 0x9e37_79b9_7f4a_7c15u64 ^ ((me as u64 + 1) << 17);

    mpstress_barrier(0, 1, mpstress_nothing);
    mpstress_pool_phase(me, &mut rand);
    mpstress_barrier(1, 2, mpstress_pool_done);
    mpstress_pmr_phase(me, &mut rand);
    mpstress_barrier(2, 3, mpstress_pmr_done);
    mpstress_uvm_phase(me, &mut rand);
    mpstress_barrier(3, 4, mpstress_uvm_done);

    if MPSTRESS_RAN_ON[me].load(Ordering::Relaxed) != cpu_number() {
        mpstress_fail(5);
    }

    crate::sys::systm::kernel_lock();
    mtx_enter(&MPSTRESS_MTX);
    MPSTRESS_EXITED.fetch_add(1, Ordering::Relaxed);
    wakeup(ptr::addr_of!(MPSTRESS_EXITED));
    mtx_leave(&MPSTRESS_MTX);
    kthread_exit(0);
}

/// `selftest=mpstress` (M11a's exit test): one kernel thread per running CPU, each pegged to
/// its CPU and started together, hammers three shared pools with per-CPU caches
/// (`pool_cache_init`), then `uvm_pglistalloc`/`uvm_pglistfree`, then uvm's fault path
/// (M11e: pageable kernel memory faulted in through the trap, and a shared anonymous map);
/// prints `selftest: mpstress pool ok (<N> cpus, ...)`, `selftest: mpstress pmemrange ok
/// (<N> cpus, ...)` and `selftest: mpstress uvm ok (<N> cpus, ...)`, or a FAILED line;
/// returns whether all three passed. The uniprocessor kernel runs
/// the same with one thread.
pub fn mpstress() -> bool {
    #[cfg(feature = "multiprocessor")]
    let ncpus = {
        use crate::machine::cpu::{cpu_info_foreach, cpu_is_running};
        let mut n = 0usize;
        cpu_info_foreach(&mut |ci| {
            if n < MPSTRESS_MAXCPUS && cpu_is_running(ci) {
                MPSTRESS_CPUS[n].store(ptr::from_ref(ci).cast_mut(), Ordering::Relaxed);
                n += 1;
            }
        });
        n.max(1)
    };
    #[cfg(not(feature = "multiprocessor"))]
    let ncpus = 1usize;
    MPSTRESS_NTHREADS.store(ncpus as u32, Ordering::Relaxed);
    mtx_init(&MPSTRESS_MTX, IPL_NONE);

    pool_init(
        &MPSTRESS_POOLS[0],
        MPSTRESS_SIZES[0],
        0,
        IPL_VM,
        0,
        "mpstress32",
        None,
    );
    pool_init(
        &MPSTRESS_POOLS[1],
        MPSTRESS_SIZES[1],
        0,
        IPL_NONE,
        PR_WAITOK | PR_RWLOCK,
        "mpstress256",
        None,
    );
    pool_init(
        &MPSTRESS_POOLS[2],
        MPSTRESS_SIZES[2],
        0,
        IPL_NONE,
        PR_WAITOK,
        "mpstress2k",
        None,
    );
    for pp in &MPSTRESS_POOLS {
        pool_cache_init(pp);
    }
    // The uvm phase's shared anonymous map: one slice of MPSTRESS_UVM_PAGES pages per thread.
    let vm = uvmspace_alloc(
        <Machine as VmParam>::VM_MIN_ADDRESS,
        <Machine as VmParam>::VM_MAXUSER_ADDRESS,
        true,
        true,
    );
    let mut uva = mpstress_uva();
    if uvm_mapanon(
        &vm.vm_map,
        &mut uva,
        ncpus * MPSTRESS_UVM_PAGES * PAGE_SIZE,
        0,
        mpstress_uvm_flags(),
    )
    .is_ok()
        && uva == mpstress_uva()
    {
        MPSTRESS_VM.store(ptr::from_ref(vm).cast_mut(), Ordering::Release);
    }
    let free_start = UVMEXP.free.load(Ordering::Relaxed);
    let start_ns = nsecuptime();

    for i in 0..ncpus {
        if let Err(e) = kthread_create(
            mpstress_thread,
            ptr::without_provenance_mut(i + 1),
            b"mpstress",
        ) {
            kprintf!("selftest: mpstress FAILED: kthread_create: {:?}\n", e);
            return false;
        }
    }

    mtx_enter(&MPSTRESS_MTX);
    while MPSTRESS_EXITED.load(Ordering::Relaxed) < ncpus as u32 {
        let _ = msleep_nsec(
            ptr::addr_of!(MPSTRESS_EXITED),
            &MPSTRESS_MTX,
            PWAIT,
            "mpstress",
            INFSLP,
        );
    }
    mtx_leave(&MPSTRESS_MTX);
    let elapsed_ms = nsecuptime().wrapping_sub(start_ns) / 1_000_000;

    // Every item is back: the pools' own count plus the caches' must be zero; then the caches
    // go and every page of the three pools must be reclaimable.
    let mut pool_ok = true;
    let mut cache_gets = 0u64;
    let mut pages_reclaimed = 0u32;
    for pp in &MPSTRESS_POOLS {
        let mut pi = KinfoPool {
            pr_nout: pp.pr_nout.get(),
            ..KinfoPool::default()
        };
        pool_cache_pool_info(pp, &mut pi);
        cache_gets += pi.pr_nget;
        pool_ok &= pi.pr_nout == 0;
        #[cfg(feature = "multiprocessor")]
        crate::kern::subr_pool::pool_cache_destroy(pp);
        pages_reclaimed += pp.pr_npages.get();
        pool_reclaim(pp);
        pool_ok &= pp.pr_npages.get() == 0 && pp.pr_nitems.get() == 0;
    }

    MPSTRESS_VM.store(ptr::null_mut(), Ordering::Relaxed);
    uvmspace_free(vm);

    let stats: [u64; 9] = core::array::from_fn(|i| MPSTRESS_STATS[i].load(Ordering::Relaxed));
    let failed = MPSTRESS_FAILED.load(Ordering::Relaxed);
    let why = MPSTRESS_MSG.get(failed).copied();
    let cpus_seen = (0..ncpus)
        .map(|i| MPSTRESS_RAN_ON[i].load(Ordering::Relaxed))
        .fold(0u64, |m, c| m | (1u64 << (c % 64)))
        .count_ones();
    let pool_failed = matches!(failed, 0 | 1 | 5);
    let pool_passed = pool_ok && !pool_failed && cpus_seen as usize == ncpus;
    if pool_passed {
        kprintf!(
            "selftest: mpstress pool ok ({} cpus, {} gets, {} through the per-cpu caches, {} items exchanged between threads, {} PR_NOWAIT refused, {} pages reclaimed, {} ms)\n",
            ncpus,
            stats[0],
            cache_gets,
            stats[2],
            stats[1],
            pages_reclaimed,
            elapsed_ms
        );
    } else {
        kprintf!(
            "selftest: mpstress pool FAILED ({} cpus, {} cpus seen, items back and pages reclaimable: {}, {})\n",
            ncpus,
            cpus_seen,
            pool_ok,
            why.unwrap_or("-")
        );
    }

    let (free_b, free_e) = (
        MPSTRESS_FREE[0].load(Ordering::Relaxed),
        MPSTRESS_FREE[1].load(Ordering::Relaxed),
    );
    let pmr_failed = matches!(failed, 2..=4);
    let pmr_passed = free_b == free_e && !pmr_failed;
    if pmr_passed {
        kprintf!(
            "selftest: mpstress pmemrange ok ({} cpus, {} page lists, {} pages, {} UVM_PLA_NOWAIT refused, {} pages free before and after; {} free at the start, {} at the end)\n",
            ncpus,
            stats[3],
            stats[4],
            stats[5],
            free_b,
            free_start,
            UVMEXP.free.load(Ordering::Relaxed)
        );
    } else {
        kprintf!(
            "selftest: mpstress pmemrange FAILED ({} cpus, {} pages free before, {} after, {})\n",
            ncpus,
            free_b,
            free_e,
            why.unwrap_or("-")
        );
    }

    let uvm_failed = matches!(failed, 6..=10);
    let uvm_ms = MPSTRESS_UVM_NS[1]
        .load(Ordering::Relaxed)
        .wrapping_sub(MPSTRESS_UVM_NS[0].load(Ordering::Relaxed))
        / 1_000_000;
    let pcp = |i: usize| {
        MPSTRESS_PCP[1][i]
            .load(Ordering::Relaxed)
            .wrapping_sub(MPSTRESS_PCP[0][i].load(Ordering::Relaxed))
    };
    if !uvm_failed {
        kprintf!(
            "selftest: mpstress uvm ok ({} cpus, {} pageable kernel pages and {} anonymous pages faulted in, {} slices unmapped and mapped again, per-cpu page caches {} hits {} misses, {} ms)\n",
            ncpus,
            stats[6],
            stats[7],
            stats[8],
            pcp(0),
            pcp(1),
            uvm_ms
        );
    } else {
        kprintf!(
            "selftest: mpstress uvm FAILED ({} cpus, {})\n",
            ncpus,
            why.unwrap_or("-")
        );
    }

    pool_passed && pmr_passed && !uvm_failed
}

/// Records the attach arguments of a frame buffer (efifb(4), simplefb) where it would
/// `config_found` its `wsdisplay` child, which is not ported yet; the first one wins.
pub fn fb_attached(dev: &Device, aa: &WsemuldisplaydevAttachArgs) {
    // SAFETY: autoconfiguration runs on one CPU while cold; `fb_check` reads later.
    let fb = unsafe { FB_ATTACHED.get_mut() };
    if fb.aa.is_some() {
        return;
    }
    let name = dev.xname().as_bytes();
    let n = name.len().min(fb.xname.len() - 1);
    fb.xname[..n].copy_from_slice(&name[..n]);
    fb.aa = Some(*aa);
}

/// `selftest=fb`: draws [`FB_TEXT`] on the frame buffer that attached, as wsdisplay would:
/// a screen from `alloc_screen`, made visible with `show_screen`, then the screen type's
/// emulops (`mapchar`, `pack_attr`, `putchar`) on it, in bright white on blue at row 1,
/// column 2. It prints where the text is in pixels and how many pixels are the foreground
/// colour (the glyphs' set bits), so that `xtask` can check a screenshot
/// (`--screenshot-after`, tools/xtask/src/hwopts.rs). The access cookie is the driver's
/// `rasops_info` (efifb and simplefb both pass it), read for the grid's origin.
pub fn fb_check() {
    // SAFETY: written while cold by `fb_attached`, which is done.
    let fb = unsafe { FB_ATTACHED.get() };
    let Some(aa) = fb.aa else {
        kprintf!("selftest: fb: no frame buffer attached\n");
        return;
    };
    let xname = Str(&fb.xname);
    let cookie = aa.accesscookie;
    // SAFETY: the drivers' screen lists are statics or softc members that live for good.
    let list = unsafe { &*aa.scrdata };
    // SAFETY: as above.
    let Some(&descr) = (unsafe { list.screens() }).first() else {
        kprintf!("selftest: fb: {}: no screen type\n", xname);
        return;
    };
    // SAFETY: as above.
    let descr = unsafe { &*descr };
    // SAFETY: `textops` is the driver's rasops_info's operation table.
    let ops = unsafe { *descr.textops };
    // SAFETY: efifb and simplefb pass their rasops_info as the access cookie.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    let (Some(alloc_screen), Some(show_screen)) =
        (aa.accessops.alloc_screen, aa.accessops.show_screen)
    else {
        kprintf!("selftest: fb: {}: no alloc_screen/show_screen\n", xname);
        return;
    };
    let (Some(mapchar), Some(pack_attr), Some(putchar)) = (ops.mapchar, ops.pack_attr, ops.putchar)
    else {
        kprintf!("selftest: fb: {}: incomplete emulops\n", xname);
        return;
    };

    let mut scr = ptr::null_mut();
    let (mut curx, mut cury, mut defattr) = (0, 0, 0);
    // SAFETY: the access cookie and a screen type of the driver's list, as wsdisplay pairs
    // them; the screen is then the emulops' cookie.
    let r = unsafe {
        alloc_screen(cookie, descr, &mut scr, &mut curx, &mut cury, &mut defattr).and_then(|()| {
            show_screen(cookie, scr, 0, None, ptr::null_mut())?;
            pack_attr(scr, WSCOL_WHITE, WSCOL_BLUE, WSATTR_WSCOLORS | WSATTR_HILIT)
        })
    };
    let attr = match r {
        Ok(attr) => attr,
        Err(e) => {
            kprintf!("selftest: fb: {}: screen setup failed: {:?}\n", xname, e);
            return;
        }
    };

    let (row, col) = (1, 2);
    let font = ri.font();
    let (fw, fh) = (font.fontwidth as usize, font.fontheight as usize);
    let stride = font.stride as usize;
    let mut ink = 0u32;
    for (i, &c) in FB_TEXT.iter().enumerate() {
        let mut g = 0;
        // SAFETY: the screen made above, with the emulops it was made for.
        let drawn = unsafe {
            let _ = mapchar(scr, i32::from(c), &mut g);
            putchar(scr, row, col + i as i32, g, attr)
        };
        if let Err(e) = drawn {
            kprintf!("selftest: fb: {}: putchar failed: {:?}\n", xname, e);
            return;
        }
        if c != b' ' {
            for line in ri
                .glyph(g - font.firstchar as u32)
                .chunks_exact(stride)
                .take(fh)
            {
                for x in 0..fw {
                    if line[x / 8] & (0x80 >> (x % 8)) != 0 {
                        ink += 1;
                    }
                }
            }
        }
    }

    let fg = &RASOPS_CMAP[15 * 3..16 * 3];
    let bg = &RASOPS_CMAP[WSCOL_BLUE as usize * 3..WSCOL_BLUE as usize * 3 + 3];
    kprintf!(
        "selftest: fb text at x={} y={} w={} h={} ink={} fg={:02x}{:02x}{:02x} bg={:02x}{:02x}{:02x}\n",
        ri.ri_xorigin.get() as usize + col as usize * fw,
        ri.ri_yorigin.get() as usize + row as usize * fh,
        FB_TEXT.len() * fw,
        fh,
        ink,
        fg[0],
        fg[1],
        fg[2],
        bg[0],
        bg[1],
        bg[2]
    );
    kprintf!(
        "selftest: fb drew '{}' on {} ({}x{}, {} bpp, {}x{} font)\n",
        Str(FB_TEXT),
        xname,
        ri.ri_width.get(),
        ri.ri_height.get(),
        ri.ri_depth.get(),
        fw,
        fh
    );
}

/// `selftest=wscons`: where the first frame buffer's `wsdisplay` screens draw, for a
/// screenshot of text written to `/dev/ttyC0` (`just smoke-wscons`): the pixel origin of
/// the character grid, the cell size and the grid's size, from the driver's `rasops_info`
/// (the access cookie, as in [`fb_check`]). Draws nothing.
pub fn wscons_grid() {
    // SAFETY: written while cold by `fb_attached`, which is done.
    let fb = unsafe { FB_ATTACHED.get() };
    let Some(aa) = fb.aa else {
        kprintf!("selftest: wscons: no frame buffer attached\n");
        return;
    };
    // SAFETY: efifb and simplefb pass their rasops_info as the access cookie.
    let ri = unsafe { &*aa.accesscookie.cast::<RasopsInfo>() };
    let font = ri.font();
    kprintf!(
        "selftest: wscons grid x={} y={} cw={} ch={} cols={} rows={} on {}\n",
        ri.ri_xorigin.get(),
        ri.ri_yorigin.get(),
        font.fontwidth,
        font.fontheight,
        ri.ri_cols.get(),
        ri.ri_rows.get(),
        Str(&fb.xname)
    );
}
/* </CODE> */
