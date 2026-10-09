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

/*
 * Copyright (c) 1980, 1986, 1993
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
 *	@(#)if.c	8.3 (Berkeley) 1/4/94
 * $FreeBSD: src/sys/net/if.c,v 1.185 2004/03/13 02:35:03 brooks Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! Interrupt maps: which CPU serves each of a device's interrupts (`kern/kern_intrmap.c`).
//!
//! Upstream: sys/kern/kern_intrmap.c @ 3ce1f3f79392
//!
//! Adapted (upstream) from DragonFly BSD's `if_ringmap`, generalised for every kind of
//! device. The CPUs that may take interrupts can be a subset of all CPUs: one per core (a
//! CPU whose `ci_smt_id` is not 0 is a second hardware thread and is left out). That set is
//! built again whenever `ncpus` has changed since the last one, and shared by reference
//! count between the maps made from it. `intrmap_create` picks how many interrupts a device
//! gets (at most `maxintrs`, at most one per interrupt CPU) and a grid: the CPUs are cut into
//! groups of `grid` and unit `n` of a driver starts at group `n` (wrapping), so several
//! devices of one driver spread their rings over different CPUs.
//!
//! Its users are multi-queue drivers with MSI-X (vio's queues, M13); nothing calls it yet.
//!
//! ## Deviations
//! - `__HAVE_CPU_TOPOLOGY` is defined on both machines; `ci->ci_smt_id` is
//!   `Cpu::ci_smt_id`, whose default is 0 (amd64's `cpu_topology` is not ported).
//! - `intrmap_create` returns `&'static Intrmap` (as `taskq_create` does) and
//!   `intrmap_destroy` is an `unsafe fn` taking it back; the C cannot fail (`M_WAITOK`), and
//!   a `None` from `malloc` panics as running out of `kmem_map` does.
//! - The body of `intrmap_create` after `intrmap_cpus_get` is `intrmap_create_cpus`, so the
//!   host tests can give it an interrupt CPU set of any size.
//! - `1 << (fls(nintrs) - 1)` (`libkern/fls.c`, not ported) is `1 << nintrs.ilog2()`;
//!   `nintrs` is never 0 there.
//! - `KASSERTMSG` is `kassert!` (the message is the C's comment at the site).

use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::kassert;
use crate::kern::init_main::NCPUS;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::{CpuInfo, ci_smt_id, cpu_info_foreach};
use crate::sys::device::Device;
use crate::sys::intrmap::INTRMAP_POWEROF2;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;

/// `struct intrmap_cpus`: a version of the set of CPUs that may take interrupts.
struct IntrmapCpus {
    /// `ic_refs`: the global's reference and one per map made from this set.
    ic_refs: Refcnt,
    /// `ic_count`: CPUs in the set.
    ic_count: u32,
    /// `ic_cpumap`: `ic_count` CPUs, `mallocarray`ed (`M_DEVBUF`).
    ic_cpumap: NonNull<&'static CpuInfo>,
}

impl IntrmapCpus {
    /// The CPUs of the set as a slice.
    fn cpus(&self) -> &[&'static CpuInfo] {
        // SAFETY: `ic_cpumap` holds `ic_count` initialised entries for the set's lifetime.
        unsafe { slice::from_raw_parts(self.ic_cpumap.as_ptr(), self.ic_count as usize) }
    }
}

/// `struct intrmap`: the CPU of each of a device's interrupts.
pub struct Intrmap {
    /// `im_count`: the device's interrupts.
    im_count: u32,
    /// `im_grid`: CPUs per group; unit `n` starts at group `n`.
    im_grid: u32,
    /// `im_cpus`: the CPU set the map indexes (one reference held).
    im_cpus: NonNull<IntrmapCpus>,
    /// `im_cpumap`: `im_count` indexes into `im_cpus`, `mallocarray`ed (`M_DEVBUF`).
    im_cpumap: NonNull<u32>,
}

// SAFETY: a map is written only while `intrmap_create` builds it and read-only afterwards;
// its CPU set is shared read-only, its reference count is atomic.
unsafe impl Sync for Intrmap {}

impl Intrmap {
    /// The CPU set of the map.
    fn ic(&self) -> &IntrmapCpus {
        // SAFETY: the map holds a reference on its set until `intrmap_destroy`.
        unsafe { self.im_cpus.as_ref() }
    }

    /// The map's CPU indexes as a slice.
    fn cpumap(&self) -> &[u32] {
        // SAFETY: `im_cpumap` holds `im_count` entries for the map's lifetime.
        unsafe { slice::from_raw_parts(self.im_cpumap.as_ptr(), self.im_count as usize) }
    }
}

/// `intrmap_lock`: serialises the rebuilds of `intrmap_cpus`.
static INTRMAP_LOCK: Rwlock = Rwlock::new("intrcpus");
/// `intrmap_cpus`: the current CPU set, or null before the first map. Written under
/// `INTRMAP_LOCK`.
static INTRMAP_CPUS: AtomicPtr<IntrmapCpus> = AtomicPtr::new(ptr::null_mut());
/// `intrmap_ncpu`: the `ncpus` that `intrmap_cpus` was built for. Written under
/// `INTRMAP_LOCK`.
static INTRMAP_NCPU: AtomicI32 = AtomicI32::new(0);

/// `malloc(M_WAITOK)`, which the C cannot see fail.
fn intrmap_malloc(size: usize, flags: i32) -> NonNull<u8> {
    malloc(size, M_DEVBUF, flags | M_WAITOK)
        .unwrap_or_else(|| panic(format_args!("intrmap: out of memory")))
}

/// `mallocarray(M_WAITOK)`, which the C cannot see fail.
fn intrmap_mallocarray(nmemb: usize, size: usize, flags: i32) -> NonNull<u8> {
    mallocarray(nmemb, size, M_DEVBUF, flags | M_WAITOK)
        .unwrap_or_else(|| panic(format_args!("intrmap: out of memory")))
}

/// `intrmap_cpus_put`: drops a reference on a CPU set and frees it with the last one.
///
/// # Safety
///
/// `ic` is null or a set from `intrmap_cpus_get` on which the caller holds a reference,
/// which this call consumes.
unsafe fn intrmap_cpus_put(ic: *mut IntrmapCpus) {
    // SAFETY: the caller's reference keeps the set alive until it is released here.
    let Some(icr) = (unsafe { ic.as_ref() }) else {
        return;
    };

    if refcnt_rele(&icr.ic_refs) {
        free(
            icr.ic_cpumap.cast::<u8>(),
            M_DEVBUF,
            icr.ic_count as usize * size_of::<&CpuInfo>(),
        );
        // That was the last reference: nobody else sees the set any more.
        free(
            NonNull::from(icr).cast::<u8>(),
            M_DEVBUF,
            size_of::<IntrmapCpus>(),
        );
    }
}

/// `intrmap_cpus_get`: the current set of interrupt CPUs, built again if `ncpus` changed
/// since the last one; the caller gets a reference on it.
fn intrmap_cpus_get() -> NonNull<IntrmapCpus> {
    let mut oic: *mut IntrmapCpus = ptr::null_mut();

    rw_enter_write(&INTRMAP_LOCK);
    let ncpus = NCPUS.load(Ordering::Relaxed);
    let ic: NonNull<IntrmapCpus> = if INTRMAP_NCPU.load(Ordering::Relaxed) != ncpus {
        // There's a new "version" of the set of CPUs available, so we need to figure out
        // which ones we can use for interrupts.
        let ncpus = ncpus.max(1) as usize;
        let mut cpumap =
            intrmap_mallocarray(ncpus, size_of::<&CpuInfo>(), 0).cast::<&'static CpuInfo>();
        let mut icpus = 0usize;

        cpu_info_foreach(&mut |ci| {
            // __HAVE_CPU_TOPOLOGY: a second hardware thread of a core takes no interrupts.
            if ci_smt_id(ci) > 0 || icpus == ncpus {
                return;
            }
            // SAFETY: `icpus < ncpus`, inside the array.
            unsafe { cpumap.as_ptr().add(icpus).write(ci) };
            icpus += 1;
        });

        if icpus < ncpus {
            // This is mostly about free(9) needing a size.
            let icpumap = intrmap_mallocarray(icpus.max(1), size_of::<&CpuInfo>(), 0)
                .cast::<&'static CpuInfo>();
            // SAFETY: both arrays hold at least `icpus` entries and do not overlap.
            unsafe { ptr::copy_nonoverlapping(cpumap.as_ptr(), icpumap.as_ptr(), icpus) };
            free(cpumap.cast::<u8>(), M_DEVBUF, ncpus * size_of::<&CpuInfo>());
            cpumap = icpumap;
        }

        let ic = intrmap_malloc(size_of::<IntrmapCpus>(), 0).cast::<IntrmapCpus>();
        // SAFETY: a fresh allocation of the struct's size, `malloc`'s alignment is enough.
        unsafe {
            ic.as_ptr().write(IntrmapCpus {
                ic_refs: Refcnt::new(),
                ic_count: icpus as u32,
                ic_cpumap: cpumap,
            })
        };
        // SAFETY: just written.
        refcnt_init(&unsafe { ic.as_ref() }.ic_refs);

        oic = INTRMAP_CPUS.load(Ordering::Relaxed);
        INTRMAP_CPUS.store(ic.as_ptr(), Ordering::Release); // give this ref to the global
        INTRMAP_NCPU.store(ncpus as i32, Ordering::Relaxed);
        ic
    } else {
        // SAFETY: `intrmap_ncpu` is only set together with a non-null `intrmap_cpus`.
        unsafe { NonNull::new_unchecked(INTRMAP_CPUS.load(Ordering::Relaxed)) }
    };

    // SAFETY: the global's reference keeps the set alive while the lock is held.
    refcnt_take(&unsafe { ic.as_ref() }.ic_refs); // take a ref for the caller
    rw_exit_write(&INTRMAP_LOCK);

    // SAFETY: `oic` is the global's old reference, handed over to us.
    unsafe { intrmap_cpus_put(oic) };

    ic
}

/// `intrmap_nintrs`: how many interrupts a device gets: what it asked for (0 for as many
/// as it can take), at most `maxintrs` and at most one per interrupt CPU.
fn intrmap_nintrs(ic: &IntrmapCpus, mut nintrs: u32, maxintrs: u32) -> u32 {
    // "invalid maximum interrupt count %u"
    kassert!(maxintrs > 0);

    if nintrs == 0 || nintrs > maxintrs {
        nintrs = maxintrs;
    }
    if nintrs > ic.ic_count {
        nintrs = ic.ic_count;
    }
    nintrs
}

/// `intrmap_set_grid`: places unit `unit`'s interrupts on consecutive CPUs, starting at its
/// group of `grid` CPUs.
fn intrmap_set_grid(im: &mut Intrmap, unit: u32, grid: u32) {
    let ic_count = im.ic().ic_count;

    // "invalid if_ringmap grid %u"
    kassert!(grid > 0);
    // "invalid intrmap grid %u, count %u"
    kassert!(grid >= im.im_count);
    im.im_grid = grid;

    let offset = grid.wrapping_mul(unit) % ic_count;
    // SAFETY: `im_cpumap` holds `im_count` entries and belongs to the map, borrowed
    // mutably here.
    let cpumap = unsafe { slice::from_raw_parts_mut(im.im_cpumap.as_ptr(), im.im_count as usize) };
    for (i, slot) in cpumap.iter_mut().enumerate() {
        *slot = offset + i as u32;
        // "invalid cpumap[%u] = %u, offset %u (ncpu %d)"
        kassert!(*slot < ic_count);
    }
}

/// `intrmap_create`: the interrupt map of device `dv`, for `nintrs` interrupts (0: as many
/// as useful) and at most `maxintrs`; `INTRMAP_POWEROF2` in `flags` rounds the count down to
/// a power of two.
pub fn intrmap_create(dv: &Device, nintrs: u32, maxintrs: u32, flags: u32) -> &'static Intrmap {
    let unit = dv.dv_unit.get().max(0) as u32;
    let ic = intrmap_cpus_get();
    intrmap_create_cpus(ic, unit, nintrs, maxintrs, flags)
}

/// The rest of `intrmap_create`, over the CPU set `ic` the caller holds a reference on
/// (which the map takes over).
fn intrmap_create_cpus(
    ic: NonNull<IntrmapCpus>,
    unit: u32,
    nintrs: u32,
    maxintrs: u32,
    flags: u32,
) -> &'static Intrmap {
    // SAFETY: the caller's reference keeps the set alive; the map inherits it.
    let icr = unsafe { ic.as_ref() };

    let mut nintrs = intrmap_nintrs(icr, nintrs, maxintrs);
    if flags & INTRMAP_POWEROF2 != 0 {
        nintrs = 1 << nintrs.ilog2();
    }
    let im_cpumap = intrmap_mallocarray(nintrs as usize, size_of::<u32>(), M_ZERO).cast::<u32>();
    let mut im = Intrmap {
        im_count: nintrs,
        im_grid: 0,
        im_cpus: ic,
        im_cpumap,
    };

    let ic_count = icr.ic_count;
    let mut grid = 0;
    let mut prev_grid = ic_count;
    for i in 0..ic_count {
        if ic_count % (i + 1) != 0 {
            continue;
        }

        grid = ic_count / (i + 1);
        if nintrs > grid {
            grid = prev_grid;
            break;
        }

        if nintrs > ic_count / (i + 2) {
            break;
        }
        prev_grid = grid;
    }
    intrmap_set_grid(&mut im, unit, grid);

    let p = intrmap_malloc(size_of::<Intrmap>(), M_ZERO).cast::<Intrmap>();
    // SAFETY: a fresh allocation of the struct's size; written once, read-only afterwards
    // until `intrmap_destroy` frees it.
    unsafe {
        p.as_ptr().write(im);
        p.as_ref()
    }
}

/// `intrmap_destroy`: frees an interrupt map and drops its reference on the CPU set.
///
/// # Safety
///
/// `im` came from [`intrmap_create`], is destroyed once, and is not used afterwards.
pub unsafe fn intrmap_destroy(im: &'static Intrmap) {
    let cpus = im.im_cpus;
    free(
        im.im_cpumap.cast::<u8>(),
        M_DEVBUF,
        im.im_count as usize * size_of::<u32>(),
    );
    // SAFETY: the map's reference on its set, released once (the caller destroys once).
    unsafe { intrmap_cpus_put(cpus.as_ptr()) };
    free(
        NonNull::from(im).cast::<u8>(),
        M_DEVBUF,
        size_of::<Intrmap>(),
    );
}

/// `intrmap_count`: the device's number of interrupts.
pub fn intrmap_count(im: &Intrmap) -> u32 {
    im.im_count
}

/// `intrmap_cpu`: the CPU that serves interrupt (ring) `ring`.
pub fn intrmap_cpu(im: &Intrmap, ring: u32) -> &'static CpuInfo {
    let ic = im.ic();
    // "invalid ring %u"
    kassert!(ring < im.im_count);
    let icpu = im.cpumap()[ring as usize];
    // "invalid interrupt cpu %u for ring %u (intrmap %p)"
    kassert!(icpu < ic.ic_count);
    ic.cpus()[icpu as usize]
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for interrupt maps: the count and grid choice over CPU sets of several sizes
    // (built here, as `intrmap_cpus_get` would on a machine with that many cores), and
    // `intrmap_create` over the host's one CPU.

    use core::mem::MaybeUninit;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::cpu::curcpu;

    /// A CPU set of `count` entries (all the host's CPU), holding the global's reference.
    fn fake_cpus(count: u32) -> NonNull<IntrmapCpus> {
        let cpumap = intrmap_mallocarray(count as usize, size_of::<&CpuInfo>(), 0)
            .cast::<&'static CpuInfo>();
        for i in 0..count as usize {
            // SAFETY: inside the array just allocated.
            unsafe { cpumap.as_ptr().add(i).write(curcpu()) };
        }
        let ic = intrmap_malloc(size_of::<IntrmapCpus>(), 0).cast::<IntrmapCpus>();
        // SAFETY: a fresh block of the struct's size.
        unsafe {
            ic.as_ptr().write(IntrmapCpus {
                ic_refs: Refcnt::new(),
                ic_count: count,
                ic_cpumap: cpumap,
            })
        };
        ic
    }

    /// `intrmap_create` over a set of `count` CPUs for unit `unit`: the count, grid and map.
    fn create(
        count: u32,
        unit: u32,
        nintrs: u32,
        maxintrs: u32,
        flags: u32,
    ) -> (u32, u32, Vec<u32>) {
        let ic = fake_cpus(count);
        // SAFETY: the set is alive; the map takes this second reference.
        refcnt_take(&unsafe { ic.as_ref() }.ic_refs);
        let im = intrmap_create_cpus(ic, unit, nintrs, maxintrs, flags);
        let got = (intrmap_count(im), im.im_grid, im.cpumap().to_vec());
        for ring in 0..intrmap_count(im) {
            assert!(ptr::eq(intrmap_cpu(im, ring), curcpu()));
        }
        // SAFETY: made just above and not used afterwards.
        unsafe { intrmap_destroy(im) };
        // SAFETY: the global's reference keeps the set alive.
        let ic_ref = unsafe { ic.as_ref() };
        assert_eq!(ic_ref.ic_refs.r_refs.load(Ordering::Relaxed), 1);
        // SAFETY: the global's reference, the last one: the set is freed.
        unsafe { intrmap_cpus_put(ic.as_ptr()) };
        got
    }

    #[test]
    fn rings_spread_over_the_cpus_by_unit() {
        let _g = setup_real_memory();
        // One ring per device: unit n on CPU n.
        assert_eq!(create(4, 0, 1, 8, 0), (1, 1, vec![0]));
        assert_eq!(create(4, 1, 1, 8, 0), (1, 1, vec![1]));
        // As many rings as CPUs: every unit uses them all.
        assert_eq!(create(4, 0, 4, 8, 0), (4, 4, vec![0, 1, 2, 3]));
        assert_eq!(create(8, 1, 16, 16, 0), (8, 8, (0..8).collect()));
        // Two rings: groups of two CPUs, unit 1 takes the second group.
        assert_eq!(create(4, 1, 2, 8, 0), (2, 2, vec![2, 3]));
        assert_eq!(create(6, 1, 2, 8, 0), (2, 2, vec![2, 3]));
        // Not a divisor: the grid is the whole set.
        assert_eq!(create(4, 0, 0, 3, 0), (3, 4, vec![0, 1, 2]));
    }

    #[test]
    fn counts_are_bounded_and_rounded() {
        let _g = setup_real_memory();
        // 0 asks for maxintrs; never more than the CPUs.
        assert_eq!(create(1, 5, 0, 4, 0), (1, 1, vec![0]));
        assert_eq!(create(2, 0, 9, 4, 0), (2, 2, vec![0, 1]));
        // INTRMAP_POWEROF2 rounds 3 down to 2.
        assert_eq!(create(4, 0, 0, 3, INTRMAP_POWEROF2), (2, 2, vec![0, 1]));
        assert_eq!(create(4, 3, 0, 3, INTRMAP_POWEROF2), (2, 2, vec![2, 3]));
    }

    #[test]
    fn intrmap_create_uses_the_running_cpus() {
        let _g = setup_real_memory();
        // SAFETY: a `struct device` is valid as all-zero bits (`config_make_softc` zeroes it).
        let dv: Device = unsafe { MaybeUninit::zeroed().assume_init() };
        dv.dv_unit.set(2);
        let im = intrmap_create(&dv, 0, 8, 0);
        assert_eq!(intrmap_count(im), NCPUS.load(Ordering::Relaxed) as u32);
        assert!(ptr::eq(intrmap_cpu(im, 0), curcpu()));
        let ic = im.im_cpus;
        // The same set again while ncpus has not changed.
        let im2 = intrmap_create(&dv, 1, 1, 0);
        assert_eq!(im2.im_cpus, ic);
        // SAFETY: both made above and not used afterwards.
        unsafe {
            intrmap_destroy(im);
            intrmap_destroy(im2);
        }
    }
}
/* </TESTS> */
