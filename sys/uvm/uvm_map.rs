/*	$OpenBSD: uvm_map.h,v 1.96 2025/09/14 13:06:02 mpi Exp $	*/
/*	$NetBSD: uvm_map.h,v 1.24 2001/02/18 21:19:08 chs Exp $	*/
/*	$OpenBSD: uvm_map.c,v 1.356 2026/06/25 08:27:34 kettenis Exp $	*/
/*	$NetBSD: uvm_map.c,v 1.86 2000/11/27 08:40:03 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2011 Ariane van der Steldt <ariane@openbsd.org>
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
 *
 *
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993, The Regents of the University of California.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *	@(#)vm_map.c    8.3 (Berkeley) 1/12/94
 * from: Id: uvm_map.c,v 1.1.2.27 1998/02/07 01:16:54 chs Exp
 *
 *
 * Copyright (c) 1987, 1990 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

//! `uvm_map.c`: uvm map operations, and `<uvm/uvm_map.h>`: `struct vm_map`.
//!
//! Upstream: sys/uvm/uvm_map.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_map.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports the map and vmspace life cycle the first user
//! process needs: `uvm_map_setup`, `uvm_map_teardown`, `uvm_map_init`, `uvmspace_alloc`,
//! `uvmspace_init`, `uvmspace_share`, `uvmspace_exec`, `uvmspace_fork`, `uvmspace_addref`,
//! `uvmspace_purge` and `uvmspace_free`. The entry tree (`uvm_map_addr`), `uvm_map`,
//! `uvm_unmap`, the address selectors (`uvm_addr`), `uvm_map_protect`, `uvm_map_immutable`,
//! `uvm_map_pageable`, the fork copy functions, `uvm_map_pie`, `uvm_map_inentry` and the
//! `ddb` printers are milestone M7a.
//!
//! ## Deviations
//! - Until M7a a map has no entries: `exec` builds the address space with wired pages
//!   through [`uvm_map_enter_wired`] and [`uvm_map_write_wired`], and the map remembers
//!   those pages in its `wired` list so `uvm_map_teardown` and `uvmspace_exec` can unmap and
//!   free them. `uvm_map_setup_entries`, the selectors and the entry pools report
//!   themselves unported. A `PROT_NONE` range wires nothing: it is a reservation.
//! - `vm_map_lock` and friends are the `*_ln` functions without the `VMMAP_DEBUG` file/line
//!   arguments, `LPRINTF` and the tree sanity checks.
//! - `uvmspace_fork` cannot copy a parent's wired pages (the fork copy functions are M7a)
//!   and reports so; process 0, which has none, forks `init` fine.
//! - `PMAP_CHECK_COPYIN` is not configured: no `check_copyin` table.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter, rw_enter_read, rw_enter_write, rw_exit,
    rw_exit_read, rw_exit_write, rw_init, rw_init_flags,
};
use crate::kern::kern_synch::{msleep_nsec, wakeup};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::curproc;
use crate::machine::intr::splassert;
use crate::machine::intr::{IPL_NONE, IPL_VM};
use crate::machine::pmap::{
    MachinePmap, pmap_activate, pmap_create, pmap_deactivate, pmap_destroy, pmap_enter,
    pmap_extract, pmap_kernel, pmap_map_direct, pmap_reference, pmap_remove, pmap_remove_holes,
    pmap_update,
};
use crate::sys::errno::Errno;
use crate::sys::mman::PROT_NONE;
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::PVM;
use crate::sys::param::{PAGE_MASK, PAGE_SIZE};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::{Proc, Process};
use crate::sys::rwlock::{
    RW_DOWNGRADE, RW_NOSLEEP, RW_UPGRADE, RW_WRITE, RWL_DUPOK, Rwlock, rw_write_held,
};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_extern::{UVM_PGA_ZERO, VmProt, Vmspace};
use crate::uvm::uvm_page::{
    PG_BUSY, PG_FAKE, PHYS_TO_VM_PAGE, Pglist, uvm_pagealloc, uvm_pagefree, uvm_pageunwire,
    uvm_pagewire, uvm_pglistfree, vm_page_to_phys,
};
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED};

/// `VM_MAP_PAGEABLE`: ro: entries are pageable.
pub const VM_MAP_PAGEABLE: i32 = 0x01;
/// `VM_MAP_INTRSAFE`: ro: interrupt safe map.
pub const VM_MAP_INTRSAFE: i32 = 0x02;
/// `VM_MAP_WIREFUTURE`: rw: wire future mappings.
pub const VM_MAP_WIREFUTURE: i32 = 0x04;
/// `VM_MAP_GUARDPAGES`: rw: add guard pgs to map.
pub const VM_MAP_GUARDPAGES: i32 = 0x20;
/// `VM_MAP_ISVMSPACE`: ro: map is a vmspace.
pub const VM_MAP_ISVMSPACE: i32 = 0x40;
/// `VM_MAP_PINSYSCALL_ONCE`: rw: pinsyscall done.
pub const VM_MAP_PINSYSCALL_ONCE: i32 = 0x100;

/// `MAX_KMAPENT`: number of kernel maps and entries to statically allocate (sufficient to
/// make it to the scheduler).
pub const MAX_KMAPENT: usize = 1024;

/// `struct vm_map`: a virtual address space, the entries that describe it and the pmap that
/// backs it.
///
/// Locks used to protect struct members: `a` atomic operations, `I` immutable after creation
/// or exec(2), `v` `vm_map_lock` (this map `lock` or `mtx`), `f` flags_lock.
pub struct VmMap {
    /// \[I\] `pmap`: physical map.
    pub pmap: Cell<*const MachinePmap>,
    /// \[v\] `sserial`: # stack changes.
    pub sserial: Cell<u64>,
    // addr: the entry tree, by address (M7a).
    /// `size`: virtual size.
    pub size: Cell<Vsize>,
    /// \[a\] `ref_count`: reference count.
    pub ref_count: AtomicI32,
    /// \[f\] `flags`.
    pub flags: Cell<i32>,
    /// `timestamp`: version number.
    pub timestamp: Cell<u32>,
    /// \[f\] `busy`: thread holding map busy.
    pub busy: Cell<*const Proc>,
    /// \[f\] `nbusy`: waiters for busy.
    pub nbusy: Cell<u32>,
    /// \[I\] `min_offset`: first address in map.
    pub min_offset: Cell<usize>,
    /// \[I\] `max_offset`: last address in map.
    pub max_offset: Cell<usize>,
    /// \[v\] `b_start`: start for brk() alloc.
    pub b_start: Cell<usize>,
    /// \[v\] `b_end`: end for brk() alloc.
    pub b_end: Cell<usize>,
    /// \[v\] `s_start`: start for stack alloc.
    pub s_start: Cell<usize>,
    /// \[v\] `s_end`: end for stack alloc.
    pub s_end: Cell<usize>,
    // uaddr_exe, uaddr_any[], uaddr_brk_stack: the address selectors (M7a).
    /// `lock`: non-intrsafe lock.
    pub lock: Rwlock,
    /// `mtx`: intrsafe lock.
    pub mtx: Mutex,
    /// `flags_lock`: flags lock.
    pub flags_lock: Mutex,
    /// \[v\] The pages wired into the map by `uvm_map_enter_wired` (see the module's
    /// deviations).
    pub wired: Pglist,
}

impl VmMap {
    /// A zero map, before `uvm_map_setup`.
    pub const fn new() -> Self {
        Self {
            pmap: Cell::new(ptr::null()),
            sserial: Cell::new(0),
            size: Cell::new(Vsize::new(0)),
            ref_count: AtomicI32::new(0),
            flags: Cell::new(0),
            timestamp: Cell::new(0),
            busy: Cell::new(ptr::null()),
            nbusy: Cell::new(0),
            min_offset: Cell::new(0),
            max_offset: Cell::new(0),
            b_start: Cell::new(0),
            b_end: Cell::new(0),
            s_start: Cell::new(0),
            s_end: Cell::new(0),
            lock: Rwlock::new("vmmaplk"),
            mtx: Mutex::new(IPL_VM),
            flags_lock: Mutex::new(IPL_VM),
            wired: Pglist::new(),
        }
    }

    /// `map->pmap`: the physical map, which `uvm_map_setup` set and `uvmspace_free` clears
    /// after the last reference.
    pub fn pmap(&self) -> &'static MachinePmap {
        let pm = self.pmap.get();
        kassert!(!pm.is_null());
        // SAFETY: non-null between `uvm_map_setup` and `uvmspace_free`, when the pmap holds
        // the reference `uvmspace_init` took; `pmap_destroy` is only called by that free.
        unsafe { &*pm }
    }
}

impl Default for VmMap {
    fn default() -> Self {
        Self::new()
    }
}

/// `uvm_vmspace_pool`: the pool of `struct vmspace`.
static UVM_VMSPACE_POOL: Pool = Pool::new();

/// `vmmapbsy`: the wait message of a thread waiting for a busy map.
const VMMAPBSY: &str = "vmmapbsy";

/// `vm_map_lock_try(map)` (`vm_map_lock_try_ln`): takes the write lock without sleeping;
/// `false` when the map is busy for another thread or already locked.
pub fn vm_map_lock_try(map: &VmMap) -> bool {
    if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        if !mtx_enter_try(&map.mtx) {
            return false;
        }
    } else {
        mtx_enter(&map.flags_lock);
        let busy = map.busy.get();
        mtx_leave(&map.flags_lock);
        if !busy.is_null() && !ptr::eq(busy, curproc_ptr()) {
            return false;
        }

        if rw_enter(&map.lock, RW_WRITE | RW_NOSLEEP).is_err() {
            return false;
        }

        // to be sure, to be sure
        mtx_enter(&map.flags_lock);
        let busy = map.busy.get();
        mtx_leave(&map.flags_lock);
        if !busy.is_null() && !ptr::eq(busy, curproc_ptr()) {
            rw_exit(&map.lock);
            return false;
        }
    }

    map.timestamp.set(map.timestamp.get() + 1);
    // LPRINTF, uvm_tree_sanity, uvm_tree_size_chk: VMMAP_DEBUG, not configured.

    true
}

/// `vm_map_lock(map)` (`vm_map_lock_ln`): takes the write lock, waiting out a thread that
/// holds the map busy.
pub fn vm_map_lock(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        mtx_enter(&map.flags_lock);
        loop {
            while !map.busy.get().is_null() && !ptr::eq(map.busy.get(), curproc_ptr()) {
                map.nbusy.set(map.nbusy.get() + 1);
                let _ = msleep_nsec(
                    ptr::from_ref(&map.busy),
                    &map.flags_lock,
                    PVM,
                    VMMAPBSY,
                    INFSLP,
                );
                map.nbusy.set(map.nbusy.get() - 1);
            }
            mtx_leave(&map.flags_lock);

            rw_enter_write(&map.lock);

            // to be sure, to be sure
            mtx_enter(&map.flags_lock);
            if !map.busy.get().is_null() && !ptr::eq(map.busy.get(), curproc_ptr()) {
                // go around again
                rw_exit_write(&map.lock);
            } else {
                // we won
                break;
            }
        }
        mtx_leave(&map.flags_lock);
    } else {
        mtx_enter(&map.mtx);
    }

    if !ptr::eq(map.busy.get(), curproc_ptr()) {
        kassert!(map.busy.get().is_null());
        map.timestamp.set(map.timestamp.get() + 1);
    }
    // LPRINTF, uvm_tree_sanity, uvm_tree_size_chk: VMMAP_DEBUG, not configured.
}

/// `vm_map_lock_read(map)` (`vm_map_lock_read_ln`).
pub fn vm_map_lock_read(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_enter_read(&map.lock);
    } else {
        mtx_enter(&map.mtx);
    }
}

/// `vm_map_unlock(map)` (`vm_map_unlock_ln`).
pub fn vm_map_unlock(map: &VmMap) {
    kassert!(map.busy.get().is_null() || ptr::eq(map.busy.get(), curproc_ptr()));
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_exit(&map.lock);
    } else {
        mtx_leave(&map.mtx);
    }
}

/// `vm_map_unlock_read(map)` (`vm_map_unlock_read_ln`).
pub fn vm_map_unlock_read(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_exit_read(&map.lock);
    } else {
        mtx_leave(&map.mtx);
    }
}

/// `vm_map_upgrade(map)` (`vm_map_upgrade_ln`): the read lock becomes the write lock
/// without sleeping; `false` when another thread is in the way.
pub fn vm_map_upgrade(map: &VmMap) -> bool {
    if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        mutex_assert_locked(&map.mtx, "vm_map_upgrade");
    } else {
        mtx_enter(&map.flags_lock);
        let busy = map.busy.get();
        mtx_leave(&map.flags_lock);
        if !busy.is_null() && !ptr::eq(busy, curproc_ptr()) {
            return false;
        }

        if rw_enter(&map.lock, RW_UPGRADE | RW_NOSLEEP).is_err() {
            return false;
        }
    }

    map.timestamp.set(map.timestamp.get() + 1);
    true
}

/// `vm_map_downgrade(map)` (`vm_map_downgrade_ln`): the write lock becomes a read lock.
pub fn vm_map_downgrade(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        mutex_assert_locked(&map.mtx, "vm_map_downgrade");
    } else {
        let rv = rw_enter(&map.lock, RW_DOWNGRADE);
        kassert!(rv.is_ok());
    }
}

/// `vm_map_busy(map)` (`vm_map_busy_ln`): the write-locking thread marks the map busy, so it
/// can drop the lock and keep other lockers out.
pub fn vm_map_busy(map: &VmMap) {
    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);
    kassert!(rw_write_held(&map.lock));
    kassert!(map.busy.get().is_null());

    mtx_enter(&map.flags_lock);
    map.busy.set(curproc_ptr());
    mtx_leave(&map.flags_lock);
}

/// `vm_map_unbusy(map)` (`vm_map_unbusy_ln`).
pub fn vm_map_unbusy(map: &VmMap) {
    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);
    kassert!(ptr::eq(map.busy.get(), curproc_ptr()));

    mtx_enter(&map.flags_lock);
    let nbusy = map.nbusy.get();
    map.busy.set(ptr::null());
    mtx_leave(&map.flags_lock);

    if nbusy > 0 {
        wakeup(ptr::from_ref(&map.busy));
    }
}

/// `vm_map_assert_anylock(map)` (`vm_map_assert_anylock_ln`).
pub fn vm_map_assert_anylock(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_assert_anylock(&map.lock);
    } else {
        mutex_assert_locked(&map.mtx, "vm_map_assert_anylock");
    }
}

/// `vm_map_assert_wrlock(map)` (`vm_map_assert_wrlock_ln`).
pub fn vm_map_assert_wrlock(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        splassert(IPL_NONE, "vm_map_assert_wrlock");
        rw_assert_wrlock(&map.lock);
    } else {
        mutex_assert_locked(&map.mtx, "vm_map_assert_wrlock");
    }
}

/// `curproc` as the pointer the map's `busy` holds.
fn curproc_ptr() -> *const Proc {
    curproc().map_or(ptr::null(), ptr::from_ref)
}

/// `vm_map_modflags(map, set, clear)`: changes the map's flags under `flags_lock`.
pub fn vm_map_modflags(map: &VmMap, set: i32, clear: i32) {
    mtx_enter(&map.flags_lock);
    map.flags.set((map.flags.get() | set) & !clear);
    mtx_leave(&map.flags_lock);
}

/// Initialize map. Allocates sufficient entries to describe the free memory in the map.
pub fn uvm_map_setup(map: &VmMap, pmap: &'static MachinePmap, min: usize, max: usize, flags: i32) {
    let mut max = max;

    kassert!(min & PAGE_MASK == 0);
    kassert!(max & PAGE_MASK == 0 || max & PAGE_MASK == PAGE_MASK);

    // Update parameters. This code handles (vaddr_t)-1 and other page mask ending
    // addresses properly. We lose the top page if the full virtual address space is used.
    if max & PAGE_MASK != 0 {
        max = max.wrapping_add(1);
        if max == 0 {
            // overflow
            max -= PAGE_SIZE;
        }
    }

    // RBT_INIT(uvm_map_addr, &map->addr), the uaddr selectors: M7a.
    map.pmap.set(pmap);
    map.size.set(Vsize::new(0));
    map.ref_count.store(0, Ordering::Relaxed);
    map.min_offset.set(min);
    map.max_offset.set(max);
    // Empty brk() area by default.
    map.b_start.set(0);
    map.b_end.set(0);
    // Empty stack area by default.
    map.s_start.set(0);
    map.s_end.set(0);
    map.flags.set(flags);
    map.timestamp.set(0);
    map.busy.set(ptr::null());
    map.wired.init();
    if flags & VM_MAP_ISVMSPACE != 0 {
        rw_init_flags(&map.lock, "vmmaplk", RWL_DUPOK);
    } else {
        rw_init(&map.lock, "kmmaplk");
    }
    // mtx_init(&map->mtx, IPL_VM), mtx_init(&map->flags_lock, IPL_VM): statically
    // initialised.

    // Configure the allocators: uvm_map_setup_md(map) for a vmspace, uaddr_kbootstrap for a
    // kernel map (M7a).

    // Fill map entries. We do not need to write-lock the map here because only the current
    // thread sees it right now. Initialize ref_count to 0 above to avoid bogus triggering of
    // lock-not-held assertions.
    let _ = unported!("uvm_map_setup_entries (the entry tree, M7a)");
    map.ref_count.store(1, Ordering::Relaxed);
}

/// Destroy the map. This is the inverse operation to `uvm_map_setup`.
pub fn uvm_map_teardown(map: &VmMap) {
    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);

    vm_map_lock(map);
    // Remove entries: the breadth-first walk of the entry tree is M7a. What the map holds
    // until then is its wired pages.
    uvm_map_remove_wired(map);
    vm_map_unlock(map);

    // Remove address selectors (uvm_addr_destroy), uvm_unmap_detach(&dead_entries): M7a.
}

/// `uvm_map_init`: init mapping system at boot time. Note that we allocate and init the
/// static pool of structs `vm_map_entry` for the kernel here.
pub fn uvm_map_init() {
    // The static pool of kernel map entries, uvm_map_entry_pool, uvm_map_entry_kmem_pool and
    // uvm_addr_init(): with the entry tree (M7a).
    let _ = unported!("uvm_map_init: the map entry pools and uvm_addr_init (M7a)");

    // initialize the map-related pools.
    pool_init(
        &UVM_VMSPACE_POOL,
        size_of::<Vmspace>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "vmsppl",
        None,
    );
}

/// `uvmspace_alloc`: allocate a vmspace structure.
///
/// - structure includes vm_map and pmap
/// - XXX: no locking on this structure
/// - refcnt set to 1, rest must be init'd by caller
pub fn uvmspace_alloc(
    min: usize,
    max: usize,
    pageable: bool,
    remove_holes: bool,
) -> &'static Vmspace {
    let Some(mem) = pool_get(&UVM_VMSPACE_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("uvmspace_alloc: uvm_vmspace_pool is empty"));
    };
    let vp = mem.cast::<Vmspace>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Vmspace>()` bytes, written
    // once before anything else sees it; it lives until `uvmspace_free` returns it.
    let vm: &'static Vmspace = unsafe {
        vp.as_ptr().write(Vmspace::new());
        vp.as_ref()
    };
    uvmspace_init(vm, None, min, max, pageable, remove_holes);
    vm
}

/// `uvmspace_init`: initialize a vmspace structure.
///
/// - XXX: no locking on this structure
/// - refcnt set to 1, rest must be init'd by caller
pub fn uvmspace_init(
    vm: &Vmspace,
    pmap: Option<&'static MachinePmap>,
    min: usize,
    max: usize,
    pageable: bool,
    remove_holes: bool,
) {
    kassert!(pmap.is_none() || pmap.is_some_and(|pm| ptr::eq(pm, pmap_kernel())));

    let pmap = match pmap {
        Some(pm) => {
            pmap_reference(pm);
            pm
        }
        None => pmap_create(),
    };

    uvm_map_setup(
        &vm.vm_map,
        pmap,
        min,
        max,
        (if pageable { VM_MAP_PAGEABLE } else { 0 }) | VM_MAP_ISVMSPACE,
    );

    vm.vm_refcnt.store(1, Ordering::Relaxed);

    if remove_holes {
        pmap_remove_holes(vm);
    }
}

/// `uvmspace_share`: share a vmspace between two processes.
///
/// - used for vfork
pub fn uvmspace_share(pr: &Process) -> &'static Vmspace {
    let vm = pr.vmspace();

    uvmspace_addref(vm);
    vm
}

/// `uvmspace_exec`: the process wants to exec a new program.
///
/// - XXX: no locking on vmspace
pub fn uvmspace_exec(p: &Proc, start: usize, end: usize) {
    let pr = p.process();
    let ovm = pr.vmspace();
    let map = &ovm.vm_map;
    let mut end = end;

    kassert!(start & PAGE_MASK == 0);
    kassert!(end & PAGE_MASK == 0 || end & PAGE_MASK == PAGE_MASK);

    // pmap_unuse_final(p) before stack addresses go away: nothing on amd64 and arm64.

    // see if more than one process is using this vmspace...
    if ovm.vm_refcnt.load(Ordering::Relaxed) == 1 {
        // If pr is the only process using its vmspace then we can safely recycle that
        // vmspace for the program that is being exec'd.

        // SYSVSHM is not configured: no segments to kill.

        // POSIX 1003.1b -- "lock future mappings" is revoked when a process execs another
        // program image.
        vm_map_lock(map);
        vm_map_modflags(map, 0, VM_MAP_WIREFUTURE | VM_MAP_PINSYSCALL_ONCE);

        // now unmap the old program. Instead of attempting to keep the map valid, we simply
        // nuke all entries and ask uvm_map_setup to reinitialize the map to the new
        // boundaries. uvm_unmap_remove will actually nuke all entries for us (as in, not
        // replace them with free-memory entries). Until M7a the entries are the wired pages.
        uvm_map_remove_wired(map);

        // Nuke statistics and boundaries.
        ovm.clear_startcopy();

        if end & PAGE_MASK != 0 {
            end = end.wrapping_add(1);
            if end == 0 {
                // overflow
                end -= PAGE_SIZE;
            }
        }

        // Setup new boundaries and populate map with entries.
        map.min_offset.set(start);
        map.max_offset.set(end);
        let _ = unported!("uvmspace_exec: uvm_map_setup_entries (the entry tree, M7a)");
        vm_map_unlock(map);

        // but keep MMU holes unavailable
        pmap_remove_holes(ovm);
    } else {
        // pr's vmspace is being shared, so we can't reuse it for pr since it is still being
        // used for others. allocate a new vmspace for pr
        let nvm = uvmspace_alloc(start, end, map.flags.get() & VM_MAP_PAGEABLE != 0, true);

        // install new vmspace and drop our ref to the old one.
        pmap_deactivate(p);
        pr.ps_vmspace.set(nvm);
        p.p_vmspace.set(nvm);
        pmap_activate(p);

        uvmspace_free(ovm);
    }

    // Release dead entries: uvm_unmap_detach (M7a).
}

/// `uvmspace_addref`: add a reference to a vmspace.
pub fn uvmspace_addref(vm: &Vmspace) {
    kassert!(vm.vm_refcnt.load(Ordering::Relaxed) > 0);
    vm.vm_refcnt.fetch_add(1, Ordering::Relaxed);
}

/// `uvmspace_purge`: tears the address space down: locks the map, to wait out all other
/// references to it, and deletes all of the mappings and pages they hold.
pub fn uvmspace_purge(vm: &Vmspace) {
    // SYSVSHM is not configured: no shared memory segments to get rid of.
    uvm_map_teardown(&vm.vm_map);
}

/// `uvmspace_free`: free a vmspace data structure.
pub fn uvmspace_free(vm: &'static Vmspace) {
    if vm.vm_refcnt.fetch_sub(1, Ordering::Relaxed) - 1 == 0 {
        // Sanity check. Kernel threads never end up here and userland ones already tear
        // down there VM space in exit1().
        uvmspace_purge(vm);

        pmap_destroy(vm.vm_map.pmap());
        vm.vm_map.pmap.set(ptr::null());

        pool_put(&UVM_VMSPACE_POOL, NonNull::from(vm).cast::<u8>());
    }
}

/// `uvmspace_fork`: fork a process' main map.
///
/// - create a new vmspace for child process from parent.
/// - parent's map must not be locked.
pub fn uvmspace_fork(pr: &Process) -> &'static Vmspace {
    let vm1 = pr.vmspace();
    let old_map = &vm1.vm_map;

    vm_map_lock(old_map);

    let vm2 = uvmspace_alloc(
        old_map.min_offset.get(),
        old_map.max_offset.get(),
        old_map.flags.get() & VM_MAP_PAGEABLE != 0,
        false,
    );
    vm2.copy_startcopy_from(vm1);
    vm2.vm_dused.set(0); // Statistic managed by us.
    let new_map = &vm2.vm_map;
    vm_map_lock(new_map);

    // go entry-by-entry: uvm_mapent_forkshared/forkcopy/forkzero over the entry tree (M7a).
    // Until then a parent that exec'd holds wired pages, which cannot be copied yet.
    if !old_map.wired.is_empty() {
        let _ =
            unported!("uvmspace_fork: copying the parent's mappings (uvm_mapent_forkcopy, M7a)");
    }
    new_map
        .flags
        .set(new_map.flags.get() | (old_map.flags.get() & VM_MAP_PINSYSCALL_ONCE));

    vm_map_unlock(old_map);
    vm_map_unlock(new_map);

    // uvm_unmap_detach(&dead, 0): M7a. SYSVSHM is not configured: no shmfork.

    vm2
}

/// Wires `len` bytes of fresh zeroed pages at `va` in `map` with protection `prot` (see the
/// module's deviations: what `uvm_map` + `uvm_fault_wire` will do once the entry tree
/// exists). A `PROT_NONE` range reserves without wiring. The map must not be locked.
pub fn uvm_map_enter_wired(map: &VmMap, va: usize, len: usize, prot: VmProt) -> Result<(), Errno> {
    kassert!(va & PAGE_MASK == 0 && len & PAGE_MASK == 0);
    if va < map.min_offset.get() || len > map.max_offset.get() - va {
        return Err(Errno::EINVAL);
    }
    if prot == PROT_NONE {
        return Ok(());
    }

    let pmap = map.pmap();
    let mut off = 0;
    while off < len {
        let Some(pg) = uvm_pagealloc(None, 0, None, UVM_PGA_ZERO) else {
            return Err(Errno::ENOMEM);
        };
        pg.clear_bits(PG_BUSY | PG_FAKE);
        uvm_pagewire(pg);
        if let Err(e) = pmap_enter(
            pmap,
            Vaddr::new(va + off),
            vm_page_to_phys(pg),
            prot,
            prot | PMAP_WIRED | PMAP_CANFAIL,
        ) {
            uvm_pageunwire(pg);
            uvm_pagefree(pg);
            return Err(e);
        }
        vm_map_lock(map);
        // SAFETY: a page `uvm_pagealloc` just handed out is on no queue; the map's lock
        // guards its `wired` list.
        unsafe { map.wired.insert_tail(pg) };
        map.size
            .set(Vsize::new(map.size.get().as_usize() + PAGE_SIZE));
        vm_map_unlock(map);
        off += PAGE_SIZE;
    }
    pmap_update(pmap);
    Ok(())
}

/// Copies `src` to `va` in `map` through the direct map of the pages `uvm_map_enter_wired`
/// wired there, whatever their user protection (what `vn_rdwr` into a writable mapping does
/// in C). `EFAULT` where no page is wired.
pub fn uvm_map_write_wired(map: &VmMap, va: usize, src: &[u8]) -> Result<(), Errno> {
    let pmap = map.pmap();
    let mut done = 0;
    while done < src.len() {
        let cur = va + done;
        let Some(pa) = pmap_extract(pmap, Vaddr::new(cur)) else {
            return Err(Errno::EFAULT);
        };
        let Some(pg) = PHYS_TO_VM_PAGE(pa.trunc_page()) else {
            return Err(Errno::EFAULT);
        };
        let in_page = cur & PAGE_MASK;
        let n = (PAGE_SIZE - in_page).min(src.len() - done);
        let dst = (pmap_map_direct(pg).as_usize() + in_page) as *mut u8;
        // SAFETY: `dst` is inside the direct map of a page this map owns (wired by
        // `uvm_map_enter_wired`), writable by the kernel; `n` stays within that page and
        // within `src`.
        unsafe { ptr::copy_nonoverlapping(src[done..].as_ptr(), dst, n) };
        done += n;
    }
    Ok(())
}

/// Unmaps and frees every page `uvm_map_enter_wired` wired into `map` (the M6 stand-in for
/// `uvm_unmap_remove` over the whole map). Called with the map locked.
fn uvm_map_remove_wired(map: &VmMap) {
    if map.wired.is_empty() {
        return;
    }
    let pmap = map.pmap();
    pmap_remove(
        pmap,
        Vaddr::new(map.min_offset.get()),
        Vaddr::new(map.max_offset.get()),
    );
    pmap_update(pmap);
    for pg in map.wired.iter() {
        uvm_pageunwire(pg);
    }
    uvm_pglistfree(&map.wired);
    map.wired.init();
    map.size.set(Vsize::new(0));
}
