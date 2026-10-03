//! Boot-time self-tests under feature `qemu`: a project helper, not OpenBSD code.
//!
//! Each test exercises a subsystem right after `main()` brought it up and prints one line that
//! `xtask smoke` asserts (`selftest: <what> ok`). They stand in for the user-space programs
//! OpenBSD would run until there is a user space; they are compiled only with feature `qemu`
//! (and need feature `alloc` for the allocator stress).

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use libkern::StaticCell;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::conf::param::HZ;
use crate::kern::kern_clock::ticks;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_softintr::{SoftintrHand, softintr_establish, softintr_schedule};
use crate::kern::kern_tc::{getuptime, nsecuptime};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_pool::{pool_destroy, pool_get, pool_init, pool_put, pool_reclaim};
use crate::kern::subr_prf::Str;
use crate::kprintf;
use crate::machine::cons::cn_rx_intr_establish;
use crate::machine::intr::{IPL_NONE, IPL_TTY};
use crate::machine::pmap::{
    pmap_extract, pmap_growkernel, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_map_direct,
    pmap_update,
};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::PAGE_SIZE;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::timeout::Timeout;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::UVM_PGA_ZERO;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::kernel_map_min;
use crate::uvm::uvm_page::{uvm_pagealloc, uvm_pagefree, vm_page_to_phys};

/// A value that is neither all zeros nor all ones.
const PATTERN: u64 = 0x5a5a_c3c3_0f0f_a5a5;

/// `selftest=trap` on the kernel command line asks for [`trap_bad_access`].
static TRAP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=uart` asks for [`uart_echo`].
static UART_REQUESTED: AtomicBool = AtomicBool::new(false);
/// `selftest=clock` asks for [`clock_check`].
static CLOCK_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Reads the self-test requests off the kernel command line: `selftest=trap` asks for the
/// fatal [`trap_bad_access`], which a plain boot must not run.
pub fn parse_bootargs(cmdline: &[u8]) {
    const TRAP: &[u8] = b"selftest=trap";
    const UART: &[u8] = b"selftest=uart";
    const CLOCK: &[u8] = b"selftest=clock";
    if cmdline.windows(TRAP.len()).any(|w| w == TRAP) {
        TRAP_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(UART.len()).any(|w| w == UART) {
        UART_REQUESTED.store(true, Ordering::Relaxed);
    }
    if cmdline.windows(CLOCK.len()).any(|w| w == CLOCK) {
        CLOCK_REQUESTED.store(true, Ordering::Relaxed);
    }
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

/// Maps a fresh page at the start of the kernel map, writes through the mapping, reads back
/// through the direct map, checks `pmap_extract` before and after `pmap_kremove`.
pub fn pmap_kernel_mapping() {
    let va = kernel_map_min();
    let want = Vaddr::new(va.as_usize() + 2 * PAGE_SIZE);
    let reached = pmap_growkernel(want);
    if reached < want {
        kprintf!(
            "selftest: pmap kernel mapping FAILED: pmap_growkernel reached {:#x}, wanted {:#x}\n",
            reached.as_usize(),
            want.as_usize()
        );
        return;
    }

    let Some(pg) = uvm_pagealloc(None, 0, None, UVM_PGA_ZERO) else {
        kprintf!("selftest: pmap kernel mapping FAILED: no page\n");
        return;
    };
    let pa = vm_page_to_phys(pg);

    // SAFETY: `va` is the first page of the kernel map, which nothing has allocated yet, and
    // `pa` is the page just taken from the free list.
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

/// Reads a kernel address that is not mapped, so the kernel takes a page fault (amd64) or a
/// data abort (arm64) in supervisor mode and the trap handler prints OpenBSD's fatal trap
/// message and panics. Never returns normally: it is the M4 exit criterion, and `smoke`
/// asserts the panic.
pub fn trap_bad_access() {
    // The first page of the kernel map: `pmap_kernel_mapping` mapped it and unmapped it again,
    // so the page tables exist and the leaf entry does not.
    let va = kernel_map_min();
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
