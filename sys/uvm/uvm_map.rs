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

/* <CODE> */
//! `uvm_map.c`: uvm map operations, and `<uvm/uvm_map.h>`: `struct vm_map` and `struct
//! vm_map_entry`.
//!
//! Upstream: sys/uvm/uvm_map.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_map.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a (part 2) ports the entry tree and the map operations:
//! `uvm_map`/`uvm_mapanon`/`uvm_unmap`, the clippers, the free-space bookkeeping with the
//! address selectors of `uvm_addr.rs`, `uvm_map_protect`/`inherit`/`immutable`/`advice`,
//! `uvm_map_pageable`, `uvm_map_extract`, `uvm_map_clean`, `uvm_map_submap`, the fork copy
//! functions and the vmspace life cycle. The fault handler (`uvm_fault_wire`,
//! `uvm_fault_unwire_locked`) is M7a part 3.
//!
//! ## Deviations
//! - `vm_map_lock` and friends are the `*_ln` functions without the `VMMAP_DEBUG`
//!   file/line arguments and `LPRINTF`; the tree checks run under feature `vmmap_debug` (and
//!   in host tests), as `option VMMAP_DEBUG`.
//! - [`uvm_map_inentry`] prints with `printf` (no controlling terminal: `uprintf`); its
//!   printf format becomes two words (`what`, `reason`).
//! - [`uvm_map_protect`] reports the `RLIMIT_DATA` check (`lim_cur` is not ported) instead
//!   of enforcing it.
//! - `uvm_map_splitentry` takes the pager reference under `KERNEL_LOCK`, as the C (M11a).
//! - `uvm_map_fill_vmmap` (`kinfo_vmentry`, sysctl) and the `ddb` printers
//!   (`uvm_map_printit`, `uvm_object_printit`, `uvm_page_printit`) wait for their
//!   subsystems; `PMAP_CHECK_COPYIN`, `SYSVSHM` and `DEADBEEF0` are not configured;
//!   `TRACEPOINT` (dt) is not configured.
//! - `UVM_MAP_CLIP_START`/`END` are [`uvm_map_clip_start_at`]/[`uvm_map_clip_end_at`].
//! - `uvm_map_fix_space` reuses a free-only entry that already starts at the address where
//!   the C would allocate a new one (a kernel map whose first entry is free space at
//!   `uvm_maxkaddr`: amd64 reserves nothing at `uvm_km_init`, its image lives outside the
//!   window); the C would collide in the address tree.
//! - `uvm_maxkaddr` starts at the compile-time `VM_MIN_KERNEL_ADDRESS` and `uvm_init` raises
//!   it to the runtime kernel-map base before `uvm_km_init` (amd64 shifts the base above its
//!   direct map).
//! - Out parameters become return values (`Result`, tuples, `Option`): `uvm_map_findspace`
//!   yields the first and last entry and the address, `uvm_map_lookup_entry` the entry only
//!   when it contains the address, `uvm_map_extract` the destination address.

use core::cell::Cell;
use core::cmp::Ordering;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering as AtomicOrdering};

use libkern::StaticCell;

use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter, rw_enter_read, rw_enter_write, rw_exit,
    rw_exit_read, rw_exit_write, rw_init, rw_init_flags,
};
use crate::kern::kern_sig::trapsignal;
use crate::kern::kern_synch::{msleep_nsec, wakeup};
use crate::kern::kern_time::ratecheck;
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put, pool_sethiwat};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::{Cpu, curproc};
use crate::machine::intr::splassert;
use crate::machine::intr::{IPL_NONE, IPL_VM};
use crate::machine::pmap::{
    MachinePmap, pmap_activate, pmap_create, pmap_deactivate, pmap_destroy, pmap_kernel,
    pmap_protect, pmap_reference, pmap_remove, pmap_remove_holes, pmap_update, pmap_wired_count,
};
use crate::machine::vmparam::VmParam;
use crate::machine::{Machine, Pmap};
use crate::sys::acct::AMAP;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_VMMAP, M_WAITOK};
use crate::sys::mman::{
    MADV_NORMAL, MADV_RANDOM, MADV_SEQUENTIAL, MAP_INHERIT_COPY, MAP_INHERIT_NONE,
    MAP_INHERIT_SHARE, MAP_INHERIT_ZERO, MCL_CURRENT, MCL_FUTURE, PROT_EXEC, PROT_NONE, PROT_READ,
    PROT_WRITE,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::PVM;
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::{PInentry, Proc, Process};
use crate::sys::queue::{SlistEntry, TailqEntry, TailqHead};
use crate::sys::rwlock::{
    RW_DOWNGRADE, RW_NOSLEEP, RW_UPGRADE, RW_WRITE, RWL_DUPOK, Rwlock, rw_write_held,
};
use crate::sys::sched::sched_pause;
use crate::sys::siginfo::{SEGV_ACCERR, Sigval};
use crate::sys::signal::SIGSEGV;
use crate::sys::systm::{COLD, INFSLP, kernel_lock, kernel_unlock};
use crate::sys::time::Timeval;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::{Vaddr, Vsize};
use crate::uvm::uvm::{
    UVM_ET_CONCEAL, UVM_ET_COPYONWRITE, UVM_ET_FREEMAPPED, UVM_ET_HOLE, UVM_ET_IMMUTABLE,
    UVM_ET_NEEDSCOPY, UVM_ET_NOFAULT, UVM_ET_OBJ, UVM_ET_STACK, UVM_ET_SUBMAP, UVM_ET_WC,
    uvm_et_iscopyonwrite, uvm_et_ishole, uvm_et_isneedscopy, uvm_et_isobj, uvm_et_isstack,
    uvm_et_issubmap,
};
use crate::uvm::uvm_addr::{
    UADDR_KBOOTSTRAP, UvmAddrState, uaddr_rnd_create, uaddr_stack_brk_create, uvm_addr_destroy,
    uvm_addr_init, uvm_addr_invoke,
};
use crate::uvm::uvm_amap::{
    AMAP_REFALL, AMAP_SHARED, amap_alloc, amap_copy, amap_cow_now, amap_flags, amap_lock,
    amap_lookup, amap_ref, amap_refs, amap_splitref, amap_unadd, amap_unlock, amap_unref,
};
use crate::uvm::uvm_anon::{VmAref, uvm_anfree};
use crate::uvm::uvm_extern::{
    PROT_MASK, UVM_FLAG_CONCEAL, UVM_FLAG_COPYONW, UVM_FLAG_FIXED, UVM_FLAG_HOLE, UVM_FLAG_NOFAULT,
    UVM_FLAG_NOMERGE, UVM_FLAG_OVERLAY, UVM_FLAG_QUERY, UVM_FLAG_SIGALTSTACK, UVM_FLAG_STACK,
    UVM_FLAG_TRYLOCK, UVM_FLAG_UNMAP, UVM_FLAG_WC, UVM_LK_ENTER, UVM_LK_EXIT, UVM_UNKNOWN_OFFSET,
    VmInherit, VmProt, Vmspace, Voff, uvm_advice, uvm_inherit, uvm_mapflag, uvm_maxprotection,
    uvm_protection,
};
use crate::uvm::uvm_fault::{uvm_fault_unwire_locked, uvm_fault_wire};
use crate::uvm::uvm_init::{UVM, UVMEXP};
use crate::uvm::uvm_km::{
    KD_NOWAIT, KP_DIRTY, KV_PAGE, is_kernel_map, kernel_map, km_alloc, uvm_km_pgremove,
    uvm_km_pgremove_intrsafe,
};
use crate::uvm::uvm_object::{UvmObject, uvm_obj_is_kern_object};
use crate::uvm::uvm_page::{PQ_ANON, uvm_pagedeactivate};
use crate::uvm::uvm_pager::{PGO_CLEANIT, PGO_DEACTIVATE, PGO_FREE};
use crate::uvm::uvm_param::{atop, ptoa, round_page, trunc_page};
use crate::uvm::uvm_pmap::{pmap_prefer_align, pmap_prefer_offset};
use crate::{kassert, kdassert, kprintf, queue_adapter, tree_adapter, unported};

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

/// `UVM_MAP_STATIC`: static map entry.
pub const UVM_MAP_STATIC: u8 = 0x01;
/// `UVM_MAP_KMEM`: from kmem entry pool.
pub const UVM_MAP_KMEM: u8 = 0x02;

/// `UVM_EXTRACT_FIXPROT`: set prot to maxprot as we go.
pub const UVM_EXTRACT_FIXPROT: u32 = 0x8;

/// `VM_MAP_KSIZE_INIT`: the kernel map will initially be this many bytes.
pub const VM_MAP_KSIZE_INIT: usize = 512 * PAGE_SIZE;
/// `VM_MAP_KSIZE_DELTA`: every time that gets cramped, we grow by at least this many bytes.
pub const VM_MAP_KSIZE_DELTA: usize = 256 * PAGE_SIZE;
/// `VM_MAP_KSIZE_ALLOCMUL`: we attempt to grow by this many times the allocation size each
/// time.
pub const VM_MAP_KSIZE_ALLOCMUL: usize = 4;

/// `VMMAP_MIN_ADDR`: auto-allocate address lower bound.
pub const VMMAP_MIN_ADDR: usize = PAGE_SIZE;

/// `UVMMAP_DEADBEEF`: the poison of an entry that is on no address tree (`DEADBEEF0` is not
/// configured).
pub const UVMMAP_DEADBEEF: usize = 0xdead_d0d0;

/// `object`: what a map entry points to (the C's `object` union).
#[derive(Clone, Copy)]
pub enum VmMapEntryObject {
    /// Neither: anonymous memory or a hole.
    None,
    /// `uvm_obj`: uvm object.
    Obj(&'static UvmObject),
    /// `sub_map`: belongs to another map.
    SubMap(&'static VmMap),
}

/// `struct vm_map_entry`: address map entries consist of start and end addresses, a VM
/// object (or sharing map) and offset into that object, and user-exported inheritance and
/// protection information. Also included is control information for virtual copy
/// operations.
///
/// Every field is guarded by the map's lock (`vm_map_lock`); the `dfree` links by whichever
/// tree, queue or dead queue the entry is on.
pub struct VmMapEntry {
    /// `daddrs.addr_entry`: address tree.
    pub addr_entry: RbtEntry,
    /// `daddrs.addr_kentry`: the static kernel entries' free list.
    pub addr_kentry: SlistEntry<VmMapEntry>,
    /// `dfree.rbtree`: link freespace tree.
    pub rbtree: RbtEntry,
    /// `dfree.tailq` / `dfree.deadq`: link freespace queue / dead entry queue.
    pub dfree_tailq: TailqEntry<VmMapEntry>,
    /// `start`: start address (`uvm_map_entry_start_copy`).
    pub start: Cell<usize>,
    /// `end`: end address.
    pub end: Cell<usize>,
    /// `guard`: bytes in guard.
    pub guard: Cell<usize>,
    /// `fspace`: free space.
    pub fspace: Cell<usize>,
    /// `object`: object I point to.
    pub object: Cell<VmMapEntryObject>,
    /// `offset`: offset into object.
    pub offset: Cell<Voff>,
    /// `aref`: anonymous overlay.
    pub aref: VmAref,
    /// `etype`: entry type.
    pub etype: Cell<i32>,
    /// `protection`: protection code.
    pub protection: Cell<VmProt>,
    /// `max_protection`: maximum protection.
    pub max_protection: Cell<VmProt>,
    /// `inheritance`: inheritance.
    pub inheritance: Cell<VmInherit>,
    /// `wired_count`: can be paged if == 0.
    pub wired_count: Cell<i32>,
    /// `advice`: madvise advice.
    pub advice: Cell<i32>,
    /// `flags`: `UVM_MAP_STATIC`, `UVM_MAP_KMEM` (`uvm_map_entry_stop_copy`).
    pub flags: Cell<u8>,
    /// `fspace_augment`: max(fspace) in subtree.
    pub fspace_augment: Cell<usize>,
}

// SAFETY: the map lock guards every field (see the struct doc).
unsafe impl Sync for VmMapEntry {}

impl VmMapEntry {
    /// A zero entry, on no tree (what the entry pools hand out with `PR_ZERO`).
    pub const fn new() -> Self {
        Self {
            addr_entry: RbtEntry::new(),
            addr_kentry: SlistEntry::new(),
            rbtree: RbtEntry::new(),
            dfree_tailq: TailqEntry::new(),
            start: Cell::new(0),
            end: Cell::new(0),
            guard: Cell::new(0),
            fspace: Cell::new(0),
            object: Cell::new(VmMapEntryObject::None),
            offset: Cell::new(0),
            aref: VmAref::new(),
            etype: Cell::new(0),
            protection: Cell::new(0),
            max_protection: Cell::new(0),
            inheritance: Cell::new(0),
            wired_count: Cell::new(0),
            advice: Cell::new(0),
            flags: Cell::new(0),
            fspace_augment: Cell::new(0),
        }
    }

    /// `object.uvm_obj`, when the entry maps an object (`UVM_ET_ISOBJ`).
    pub fn uvm_obj(&self) -> Option<&'static UvmObject> {
        match self.object.get() {
            VmMapEntryObject::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// `object.sub_map`, when the entry is a submap (`UVM_ET_ISSUBMAP`).
    pub fn sub_map(&self) -> Option<&'static VmMap> {
        match self.object.get() {
            VmMapEntryObject::SubMap(m) => Some(m),
            _ => None,
        }
    }
}

impl Default for VmMapEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// `VM_MAPENT_ISWIRED(entry)`.
pub fn vm_mapent_iswired(entry: &VmMapEntry) -> bool {
    entry.wired_count.get() != 0
}

/// `VMMAP_FREE_START(entry)`: where the free space after the entry starts.
pub fn vmmap_free_start(entry: &VmMapEntry) -> usize {
    entry.end.get() + entry.guard.get()
}

/// `VMMAP_FREE_END(entry)`: where the free space after the entry ends.
pub fn vmmap_free_end(entry: &VmMapEntry) -> usize {
    entry.end.get() + entry.guard.get() + entry.fspace.get()
}

/// `uvm_mapentry_addrcmp`: tree describing entries by address.
///
/// Addresses are unique. Entries with `start == end` may only exist if they are the first
/// entry (sorted by address) within a free-memory tree.
pub fn uvm_mapentry_addrcmp(e1: &VmMapEntry, e2: &VmMapEntry) -> Ordering {
    e1.start.get().cmp(&e2.start.get())
}

tree_adapter!(
    /// `uvm_map_addr`: the entry tree of a map, by address (`daddrs.addr_entry`), augmented
    /// with `fspace_augment` (`RBT_GENERATE_AUGMENT`).
    pub UvmMapAddr: VmMapEntry, addr_entry => RbtEntry, uvm_mapentry_addrcmp,
    augment = uvm_map_addr_augment
);

queue_adapter!(
    /// The dead entry queue's link (`dfree.deadq`).
    pub UvmMapDeadqLink: VmMapEntry, dfree_tailq => TailqEntry<VmMapEntry>
);

/// `struct uvm_map_deadq`: dead entry queue.
pub type UvmMapDeadq = TailqHead<UvmMapDeadqLink>;

queue_adapter!(
    /// `uvm.kentry_free`'s link (`daddrs.addr_kentry`).
    pub UvmKentryFree: VmMapEntry, addr_kentry => SlistEntry<VmMapEntry>
);

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
    /// \[v\] `addr`: entry tree, by addr.
    pub addr: RbtHead<UvmMapAddr>,
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
    /// `uaddr_exe`: executable selector.
    ///
    /// Special address selectors. The uaddr_exe mapping is used if: it is set and the
    /// protection of the mapping contains `PROT_EXEC`. If uaddr_exe is not used, the other
    /// mappings are checked in order of appearance. If uaddr_any is NULL, the kernel
    /// will panic.
    pub uaddr_exe: Cell<Option<&'static UvmAddrState>>,
    /// `uaddr_any`: more selectors.
    pub uaddr_any: [Cell<Option<&'static UvmAddrState>>; 4],
    /// `uaddr_brk_stack`: brk/stack selector. The uaddr_brk_stack selector will select
    /// addresses that are in the brk/stack area of the map.
    pub uaddr_brk_stack: Cell<Option<&'static UvmAddrState>>,
    // check_copyin: PMAP_CHECK_COPYIN is not configured.
    /// `lock`: non-intrsafe lock.
    pub lock: Rwlock,
    /// `mtx`: intrsafe lock.
    pub mtx: Mutex,
    /// `flags_lock`: flags lock.
    pub flags_lock: Mutex,
}

impl VmMap {
    /// A zero map, before `uvm_map_setup`.
    pub const fn new() -> Self {
        Self {
            pmap: Cell::new(ptr::null()),
            sserial: Cell::new(0),
            addr: RbtHead::new(),
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
            uaddr_exe: Cell::new(None),
            uaddr_any: [const { Cell::new(None) }; 4],
            uaddr_brk_stack: Cell::new(None),
            lock: Rwlock::new("vmmaplk"),
            mtx: Mutex::new(IPL_VM),
            flags_lock: Mutex::new(IPL_VM),
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

    /// `(struct vmspace *)map`: the vmspace this map is the first field of
    /// (`VM_MAP_ISVMSPACE`).
    pub fn vmspace(&self) -> &Vmspace {
        kassert!(self.flags.get() & VM_MAP_ISVMSPACE != 0);
        // SAFETY: `uvmspace_init` is the only setter of `VM_MAP_ISVMSPACE`, on the map that
        // is the first field of a `#[repr(C)]` `Vmspace` (offset asserted below).
        unsafe { &*ptr::from_ref(self).cast::<Vmspace>() }
    }
}

// SAFETY: the map's own locks guard its fields, as the field docs say (`v`, `f`, `a`, `I`);
// the kernel map is a static.
unsafe impl Sync for VmMap {}

const _: () = assert!(core::mem::offset_of!(Vmspace, vm_map) == 0);

impl Default for VmMap {
    fn default() -> Self {
        Self::new()
    }
}

/// `uvm_kmapent_mtx`: guards `uvm.kentry_free` and the warning rate limiter.
static UVM_KMAPENT_MTX: Mutex = Mutex::new(IPL_VM);
/// `uvm_kmapent_last_warn_time` (guarded by `uvm_kmapent_mtx`).
static UVM_KMAPENT_LAST_WARN_TIME: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));
/// `uvm_kmapent_warn_rate`.
static UVM_KMAPENT_WARN_RATE: Timeval = Timeval::new(10, 0);

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
    // LPRINTF(("map   lock: ...")): not configured.
    uvm_tree_sanity(map, "vm_map_lock_try");
    uvm_tree_size_chk(map, "vm_map_lock_try");

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
    uvm_tree_sanity(map, "vm_map_lock");
    uvm_tree_size_chk(map, "vm_map_lock");
}

/// `vm_map_lock_read(map)` (`vm_map_lock_read_ln`).
pub fn vm_map_lock_read(map: &VmMap) {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_enter_read(&map.lock);
    } else {
        mtx_enter(&map.mtx);
    }
    uvm_tree_sanity(map, "vm_map_lock_read");
    uvm_tree_size_chk(map, "vm_map_lock_read");
}

/// `vm_map_unlock(map)` (`vm_map_unlock_ln`).
pub fn vm_map_unlock(map: &VmMap) {
    kassert!(map.busy.get().is_null() || ptr::eq(map.busy.get(), curproc_ptr()));
    uvm_tree_sanity(map, "vm_map_unlock");
    uvm_tree_size_chk(map, "vm_map_unlock");
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        rw_exit(&map.lock);
    } else {
        mtx_leave(&map.mtx);
    }
}

/// `vm_map_unlock_read(map)` (`vm_map_unlock_read_ln`).
pub fn vm_map_unlock_read(map: &VmMap) {
    // XXX: RO
    uvm_tree_sanity(map, "vm_map_unlock_read");
    uvm_tree_size_chk(map, "vm_map_unlock_read");
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
    uvm_tree_sanity(map, "vm_map_upgrade");
    uvm_tree_size_chk(map, "vm_map_upgrade");
    true
}

/// `vm_map_downgrade(map)` (`vm_map_downgrade_ln`): the write lock becomes a read lock.
pub fn vm_map_downgrade(map: &VmMap) {
    uvm_tree_sanity(map, "vm_map_downgrade");
    uvm_tree_size_chk(map, "vm_map_downgrade");
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

/// `uvm_vmspace_pool`: pool for vmspace structures.
static UVM_VMSPACE_POOL: Pool = Pool::new();

/// `uvm_map_entry_pool`: pool for dynamically-allocated map entries.
static UVM_MAP_ENTRY_POOL: Pool = Pool::new();
/// `uvm_map_entry_kmem_pool`: the kernel map's entries.
static UVM_MAP_ENTRY_KMEM_POOL: Pool = Pool::new();

/// `uvm_maxkaddr`: this global represents the end of the kernel virtual address space. If
/// we want to exceed this, we must grow the kernel virtual address space dynamically.
///
/// Note, this variable is locked by kernel_map's lock.
pub static UVM_MAXKADDR: AtomicUsize =
    AtomicUsize::new(<Machine as VmParam>::VM_MIN_KERNEL_ADDRESS);

/// `kernel_map_entry[MAX_KMAPENT]`: the static pool of kernel map entries `uvm_map_init`
/// puts on `uvm.kentry_free`.
static KERNEL_MAP_ENTRY: [VmMapEntry; MAX_KMAPENT] = [const { VmMapEntry::new() }; MAX_KMAPENT];

/// `uvm_maxkaddr`'s value.
fn uvm_maxkaddr() -> usize {
    UVM_MAXKADDR.load(AtomicOrdering::Relaxed)
}

/// `UVM_MAP_REQ_WRITE(map)`: locking predicate.
fn uvm_map_req_write(map: &VmMap) {
    if map.ref_count.load(AtomicOrdering::Relaxed) > 0 {
        if map.flags.get() & VM_MAP_INTRSAFE == 0 {
            rw_assert_wrlock(&map.lock);
        } else {
            mutex_assert_locked(&map.mtx, "uvm_map_req_write");
        }
    }
}

/// Whether two optional selectors are the same one (the C's pointer compare).
fn same_uaddr(a: Option<&UvmAddrState>, b: Option<&UvmAddrState>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => ptr::eq(a, b),
        _ => false,
    }
}

/// `iter != end` for a tree walk that stops at `end` (`None`: walks to the last entry).
fn until<'a>(iter: Option<&'a VmMapEntry>, end: Option<&VmMapEntry>) -> Option<&'a VmMapEntry> {
    match (iter, end) {
        (Some(e), Some(stop)) if ptr::eq(e, stop) => None,
        _ => iter,
    }
}

/// The entry that describes `addr`, which every address inside the map has (the tree has
/// no gaps); a missing one is a corrupted tree.
fn entry_at<'m>(map: &'m VmMap, addr: usize, who: &str) -> &'m VmMapEntry {
    match uvm_map_entrybyaddr(&map.addr, addr) {
        Some(e) => e,
        None => panic(format_args!(
            "{}: map {:p} has no entry for address {:#x}",
            who,
            ptr::from_ref(map),
            addr
        )),
    }
}

/// `uvm_mapent_copy`: copy mapentry (the fields from `uvm_map_entry_start_copy` to
/// `uvm_map_entry_stop_copy`: not the links, `flags` or `fspace_augment`).
#[inline]
fn uvm_mapent_copy(src: &VmMapEntry, dst: &VmMapEntry) {
    dst.start.set(src.start.get());
    dst.end.set(src.end.get());
    dst.guard.set(src.guard.get());
    dst.fspace.set(src.fspace.get());
    dst.object.set(src.object.get());
    dst.offset.set(src.offset.get());
    dst.aref.ar_pageoff.set(src.aref.ar_pageoff.get());
    dst.aref.ar_amap.set(src.aref.ar_amap.get());
    dst.etype.set(src.etype.get());
    dst.protection.set(src.protection.get());
    dst.max_protection.set(src.max_protection.get());
    dst.inheritance.set(src.inheritance.get());
    dst.wired_count.set(src.wired_count.get());
    dst.advice.set(src.advice.get());
}

/// `uvm_mapent_free_insert`: handle free-list insertion.
pub fn uvm_mapent_free_insert(map: &VmMap, uaddr: Option<&UvmAddrState>, entry: &VmMapEntry) {
    #[cfg(any(test, feature = "vmmap_debug"))]
    {
        // Boundary check. Boundaries are folded if they go on the same free list.
        let mut min = vmmap_free_start(entry);
        let max = vmmap_free_end(entry);

        while min < max {
            let bound = uvm_map_boundary(map, min, max);
            kassert!(same_uaddr(uvm_map_uaddr(map, min), uaddr));
            min = bound;
        }
    }
    kdassert!(entry.fspace.get() & PAGE_MASK == 0);
    kassert!(entry.etype.get() & UVM_ET_FREEMAPPED == 0);

    uvm_map_req_write(map);

    // Actual insert: forward to uaddr pointer.
    if let Some(uaddr) = uaddr {
        let fun = uaddr.uaddr_functions;
        if let Some(insert) = fun.uaddr_free_insert {
            insert(map, uaddr, entry);
        }
        entry.etype.set(entry.etype.get() | UVM_ET_FREEMAPPED);
    }

    // Update fspace augmentation.
    uvm_map_addr_augment(entry);
}

/// `uvm_mapent_free_remove`: handle free-list removal.
pub fn uvm_mapent_free_remove(map: &VmMap, uaddr: Option<&UvmAddrState>, entry: &VmMapEntry) {
    kassert!(entry.etype.get() & UVM_ET_FREEMAPPED != 0 || uaddr.is_none());
    kassert!(same_uaddr(uvm_map_uaddr_e(map, entry), uaddr));
    uvm_map_req_write(map);

    if let Some(uaddr) = uaddr {
        let fun = uaddr.uaddr_functions;
        if let Some(remove) = fun.uaddr_free_remove {
            remove(map, uaddr, entry);
        }
        entry.etype.set(entry.etype.get() & !UVM_ET_FREEMAPPED);
    }
}

/// `uvm_mapent_addr_insert`: handle address tree insertion.
pub fn uvm_mapent_addr_insert(map: &VmMap, entry: &VmMapEntry) {
    if !RbtHead::<UvmMapAddr>::check(entry, UVMMAP_DEADBEEF) {
        panic(format_args!(
            "uvm_mapent_addr_insert: entry still in addr list"
        ));
    }
    kdassert!(entry.start.get() <= entry.end.get());
    kdassert!(entry.start.get() & PAGE_MASK == 0 && entry.end.get() & PAGE_MASK == 0);

    // TRACEPOINT(uvm, map_insert, ...): dt is not configured.

    uvm_map_req_write(map);
    // SAFETY: the map is write-locked (asserted above when it has references) and the entry
    // is on no address tree (its links hold the poison, checked above).
    if let Some(res) = unsafe { map.addr.insert(entry) } {
        panic(format_args!(
            "uvm_mapent_addr_insert: map {:p} entry {:p} ({:#x}-{:#x} G={:#x} F={:#x}) insert collision with entry {:p} ({:#x}-{:#x} G={:#x} F={:#x})",
            ptr::from_ref(map),
            ptr::from_ref(entry),
            entry.start.get(),
            entry.end.get(),
            entry.guard.get(),
            entry.fspace.get(),
            ptr::from_ref(res),
            res.start.get(),
            res.end.get(),
            res.guard.get(),
            res.fspace.get()
        ));
    }
}

/// `uvm_mapent_addr_remove`: handle address tree removal.
pub fn uvm_mapent_addr_remove(map: &VmMap, entry: &VmMapEntry) {
    // TRACEPOINT(uvm, map_remove, ...): dt is not configured.

    uvm_map_req_write(map);
    // SAFETY: the map is write-locked and the entry is on its address tree (every caller
    // found it there).
    let res = unsafe { map.addr.remove(entry) };
    if !ptr::eq(res, entry) {
        panic(format_args!("uvm_mapent_addr_remove"));
    }
    RbtHead::<UvmMapAddr>::poison(entry, UVMMAP_DEADBEEF);
}

/// `uvm_map_reference`: add reference to a map.
///
/// - map need not be locked
pub fn uvm_map_reference(map: &VmMap) {
    map.ref_count.fetch_add(1, AtomicOrdering::Relaxed);
}

/// `uvm_map_lock_entry`: locks the entry's amap and object.
pub fn uvm_map_lock_entry(entry: &VmMapEntry) {
    if let Some(amap) = entry.aref.amap() {
        amap_lock(amap, RW_WRITE);
    }
    if let Some(uobj) = entry.uvm_obj().filter(|_| uvm_et_isobj(entry)) {
        rw_enter_write(uobj.vmobjlock());
    }
}

/// `uvm_map_unlock_entry`: the inverse of [`uvm_map_lock_entry`].
pub fn uvm_map_unlock_entry(entry: &VmMapEntry) {
    if let Some(uobj) = entry.uvm_obj().filter(|_| uvm_et_isobj(entry)) {
        rw_exit(uobj.vmobjlock());
    }
    if let Some(amap) = entry.aref.amap() {
        amap_unlock(amap);
    }
}

/// `uvmspace_dused`: calculate the dused delta.
pub fn uvmspace_dused(map: &VmMap, min: usize, max: usize) -> usize {
    kassert!(map.flags.get() & VM_MAP_ISVMSPACE != 0);
    vm_map_assert_anylock(map);

    let vm = map.vmspace();
    let stack_begin = vm.vm_maxsaddr.get().min(vm.vm_minsaddr.get());
    let stack_end = vm.vm_maxsaddr.get().max(vm.vm_minsaddr.get());

    let mut sz = 0;
    let mut min = min;
    while min != max {
        let mut lmax = max;
        if min < stack_begin && lmax > stack_begin {
            lmax = stack_begin;
        } else if min < stack_end && lmax > stack_end {
            lmax = stack_end;
        }

        if min >= stack_begin && min < stack_end {
            // nothing
        } else {
            sz += lmax - min;
        }
        min = lmax;
    }

    sz >> PAGE_SHIFT
}

/// `uvm_map_entrybyaddr`: find the entry describing the given address.
pub fn uvm_map_entrybyaddr(atree: &RbtHead<UvmMapAddr>, addr: usize) -> Option<&VmMapEntry> {
    let mut iter = atree.root();
    while let Some(e) = iter {
        if e.start.get() > addr {
            iter = RbtHead::<UvmMapAddr>::left(e);
        } else if vmmap_free_end(e) <= addr {
            iter = RbtHead::<UvmMapAddr>::right(e);
        } else {
            return Some(e);
        }
    }
    None
}

/// `DEAD_ENTRY_PUSH(deadq, entry)`: push dead entries into a linked list.
///
/// Since the linked list abuses the address tree for storage, the entry may not be linked
/// in a map.
///
/// The queue must be initialised before the first call. `uvm_unmap_detach(deadq, 0)` will
/// remove dead entries.
#[inline]
fn dead_entry_push(deadq: &UvmMapDeadq, entry: &VmMapEntry) {
    // SAFETY: the entry was just unlinked from its address tree (or never linked) and is on
    // no other dead queue; the map's lock serialises the pushers.
    unsafe { deadq.insert_tail(entry) };
}

/// `uvm_map_isavail`: test if memory starting at addr with sz bytes is free.
///
/// Fills in `start_ptr` and `end_ptr` to be the first and last entry describing the space.
/// If called with prefilled `start_ptr` and `end_ptr`, they are to be correct.
pub fn uvm_map_isavail<'m>(
    map: &'m VmMap,
    uaddr: Option<&UvmAddrState>,
    start_ptr: &mut Option<&'m VmMapEntry>,
    end_ptr: &mut Option<&'m VmMapEntry>,
    addr: usize,
    sz: usize,
) -> bool {
    if addr.wrapping_add(sz) < addr {
        return false;
    }

    vm_map_assert_anylock(map);

    // Kernel memory above uvm_maxkaddr is considered unavailable.
    if map.flags.get() & VM_MAP_ISVMSPACE == 0 && addr + sz > uvm_maxkaddr() {
        return false;
    }

    let atree = &map.addr;

    // Fill in first, last, so they point at the entries containing the first and last
    // address of the range. Note that if they are not NULL, we don't perform the lookup.
    let first = match *start_ptr {
        None => match uvm_map_entrybyaddr(atree, addr) {
            Some(e) => {
                *start_ptr = Some(e);
                e
            }
            None => return false,
        },
        Some(e) => {
            kassert!(uvm_map_entrybyaddr(atree, addr).is_some_and(|f| ptr::eq(f, e)));
            e
        }
    };
    let last = match *end_ptr {
        None => {
            if vmmap_free_end(first) >= addr + sz {
                *end_ptr = Some(first);
                first
            } else {
                match uvm_map_entrybyaddr(atree, addr + sz - 1) {
                    Some(e) => {
                        *end_ptr = Some(e);
                        e
                    }
                    None => return false,
                }
            }
        }
        Some(e) => {
            kassert!(uvm_map_entrybyaddr(atree, addr + sz - 1).is_some_and(|l| ptr::eq(l, e)));
            e
        }
    };

    // Validation.
    kdassert!(
        first.start.get() <= addr
            && vmmap_free_end(first) > addr
            && last.start.get() < addr + sz
            && vmmap_free_end(last) >= addr + sz
    );

    // Check the none of the entries intersects with <addr, addr+sz>. Also, if the entry
    // belong to uaddr_exe or uaddr_brk_stack, it is considered unavailable unless called by
    // those allocators.
    let i_end = RbtHead::<UvmMapAddr>::next(last);
    let mut iter = Some(first);
    while let Some(i) = until(iter, i_end) {
        if i.start.get() != i.end.get() && i.end.get() > addr {
            return false;
        }

        // uaddr_exe and uaddr_brk_stack may only be used by these allocators and the NULL
        // uaddr (i.e. no uaddr). Reject if this requirement is not met.
        if let Some(uaddr) = uaddr
            && let Some(free) = uvm_map_uaddr_e(map, i)
            && !ptr::eq(uaddr, free)
            && (same_uaddr(Some(free), map.uaddr_exe.get())
                || same_uaddr(Some(free), map.uaddr_brk_stack.get()))
        {
            return false;
        }
        iter = RbtHead::<UvmMapAddr>::next(i);
    }

    true
}

/// `uvm_map_findspace`: invoke each address selector until an address is found. Will not
/// invoke uaddr_exe. Yields the first and last entry and the address.
pub fn uvm_map_findspace(
    map: &VmMap,
    sz: usize,
    pmap_align: usize,
    pmap_offset: usize,
    prot: VmProt,
    hint: usize,
) -> Result<(&VmMapEntry, &VmMapEntry, usize), Errno> {
    let mut hint = hint;
    loop {
        // Allocation for sz bytes at any address, using the addr selectors in order.
        for slot in &map.uaddr_any {
            if let Ok(found) =
                uvm_addr_invoke(map, slot.get(), sz, pmap_align, pmap_offset, prot, hint)
            {
                return Ok(found);
            }
        }

        // Fall back to brk() and stack() address selectors.
        if let Ok(found) = uvm_addr_invoke(
            map,
            map.uaddr_brk_stack.get(),
            sz,
            pmap_align,
            pmap_offset,
            prot,
            hint,
        ) {
            return Ok(found);
        }

        if hint != 0 {
            hint = 0;
            continue;
        }

        return Err(Errno::ENOMEM);
    }
}

/// `uvm_map_addr_augment_get`: calculate entry augmentation value.
pub fn uvm_map_addr_augment_get(entry: &VmMapEntry) -> usize {
    let mut augment = entry.fspace.get();
    if let Some(left) = RbtHead::<UvmMapAddr>::left(entry) {
        augment = augment.max(left.fspace_augment.get());
    }
    if let Some(right) = RbtHead::<UvmMapAddr>::right(entry) {
        augment = augment.max(right.fspace_augment.get());
    }
    augment
}

/// `uvm_map_addr_augment`: update augmentation data in entry.
pub fn uvm_map_addr_augment(entry: &VmMapEntry) {
    let mut entry = Some(entry);
    while let Some(e) = entry {
        // Calculate value for augmentation.
        let augment = uvm_map_addr_augment_get(e);

        // Descend update. Once we find an entry that already has the correct value, stop,
        // since it means all its parents will use the correct value too.
        if e.fspace_augment.get() == augment {
            return;
        }
        e.fspace_augment.set(augment);
        entry = RbtHead::<UvmMapAddr>::parent(e);
    }
}

/// `uvm_mapanon`: establish a valid mapping in map for an anon.
///
/// - `*addr` and sz must be a multiple of `PAGE_SIZE`.
/// - `*addr` is ignored, except if flags contains `UVM_FLAG_FIXED`.
/// - map must be unlocked.
/// - align: align vaddr, must be a power-of-2. Align is only a hint and will be ignored if
///   the alignment fails.
pub fn uvm_mapanon(
    map: &VmMap,
    addr: &mut usize,
    sz: usize,
    align: usize,
    flags: u32,
) -> Result<(), Errno> {
    kassert!(map.flags.get() & VM_MAP_ISVMSPACE == VM_MAP_ISVMSPACE);
    kassert!(!is_kernel_map(map));
    kassert!(map.flags.get() & UVM_FLAG_HOLE as i32 == 0);
    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);
    splassert(IPL_NONE, "uvm_mapanon");
    kassert!(flags & UVM_FLAG_TRYLOCK == 0);

    // Decode parameters.
    let prot = uvm_protection(flags);
    let maxprot = uvm_maxprotection(flags);
    let dead = UvmMapDeadq::new();
    kassert!(sz & PAGE_MASK == 0);
    kassert!(align & align.wrapping_sub(1) == 0);

    // Check protection.
    if prot & maxprot != prot {
        return Err(Errno::EACCES);
    }

    // Before grabbing the lock, allocate a map entry for later use to ensure we don't wait
    // for memory while holding the vm_map_lock.
    let mut new = Some(uvm_mapent_alloc(map, flags).ok_or(Errno::ENOMEM)?);

    vm_map_lock(map);
    let error = uvm_mapanon_locked(map, addr, sz, align, flags, &dead, &mut new);
    vm_map_unlock(map);

    // Remove dead entries.
    //
    // Dead entries may be the result of merging. uvm_map_mkentry may also create dead
    // entries, when it attempts to destroy free-space entries.
    uvm_unmap_detach(&dead, 0);

    if let Some(new) = new {
        uvm_mapent_free(new);
    }
    error
}

/// [`uvm_mapanon`] between its lock and unlock (the C's body up to the `unlock` label).
fn uvm_mapanon_locked(
    map: &VmMap,
    addr: &mut usize,
    sz: usize,
    align: usize,
    flags: u32,
    dead: &UvmMapDeadq,
    new: &mut Option<&'static VmMapEntry>,
) -> Result<(), Errno> {
    // We use pmap_align and pmap_offset as alignment and offset variables.
    //
    // Because the align parameter takes precedence over pmap prefer, the pmap_align will
    // need to be set to align, with pmap_offset = 0, if pmap_prefer will not align.
    let pmap_align = align.max(PAGE_SIZE);
    let pmap_offset = 0;

    let prot = uvm_protection(flags);
    let maxprot = uvm_maxprotection(flags);
    let advice = uvm_advice(flags);
    let inherit = uvm_inherit(flags);
    let hint = trunc_page(*addr);

    let mut first = None;
    let mut last = None;
    if flags & UVM_FLAG_FIXED != 0 {
        // Fixed location.
        //
        // Note: we ignore align, pmap_prefer. Fill in first, last and *addr.
        kassert!(*addr & PAGE_MASK == 0);

        // Check that the space is available.
        if flags & UVM_FLAG_UNMAP != 0 {
            if flags & UVM_FLAG_STACK != 0
                && !uvm_map_is_stack_remappable(map, *addr, sz, flags & UVM_FLAG_SIGALTSTACK != 0)
            {
                return Err(Errno::EINVAL);
            }
            if uvm_unmap_remove(
                map,
                *addr,
                *addr + sz,
                dead,
                false,
                true,
                flags & UVM_FLAG_SIGALTSTACK == 0,
            )
            .is_err()
            {
                return Err(Errno::EPERM); // immutable entries found
            }
        }
        if !uvm_map_isavail(map, None, &mut first, &mut last, *addr, sz) {
            return Err(Errno::ENOMEM);
        }
    } else if *addr != 0
        && *addr & PAGE_MASK == 0
        && (align == 0 || *addr & (align - 1) == 0)
        && uvm_map_isavail(map, None, &mut first, &mut last, *addr, sz)
    {
        // Address used as hint.
        //
        // Note: we enforce the alignment restriction, but ignore pmap_prefer.
    } else if prot & PROT_EXEC != 0 && map.uaddr_exe.get().is_some() {
        // Run selection algorithm for executables.
        let (f, l, a) = uvm_addr_invoke(
            map,
            map.uaddr_exe.get(),
            sz,
            pmap_align,
            pmap_offset,
            prot,
            hint,
        )?;
        first = Some(f);
        last = Some(l);
        *addr = a;
    } else {
        // Update freelists from vmspace.
        uvm_map_vmspace_update(map, dead, flags);

        let (f, l, a) = uvm_map_findspace(map, sz, pmap_align, pmap_offset, prot, hint)?;
        first = Some(f);
        last = Some(l);
        *addr = a;
    }

    // Double-check if selected address doesn't cause overflow.
    if addr.wrapping_add(sz) < *addr {
        return Err(Errno::ENOMEM);
    }

    // If we only want a query, return now.
    if flags & UVM_FLAG_QUERY != 0 {
        return Ok(());
    }

    // Create new entry. first and last may be invalidated after this call.
    let (Some(first), Some(last)) = (first, last) else {
        return Err(Errno::ENOMEM);
    };
    let entry = uvm_map_mkentry(map, first, last, *addr, sz, flags, dead, new.take())
        .ok_or(Errno::ENOMEM)?;
    kdassert!(entry.start.get() == *addr && entry.end.get() == *addr + sz);
    entry.object.set(VmMapEntryObject::None);
    entry.offset.set(0);
    entry.protection.set(prot);
    entry.max_protection.set(maxprot);
    entry.inheritance.set(inherit);
    entry.wired_count.set(0);
    entry.advice.set(advice);
    if flags & UVM_FLAG_STACK != 0 {
        entry.etype.set(entry.etype.get() | UVM_ET_STACK);
        if flags & (UVM_FLAG_FIXED | UVM_FLAG_UNMAP) != 0 {
            map.sserial.set(map.sserial.get() + 1);
        }
    }
    if flags & UVM_FLAG_COPYONW != 0 {
        entry.etype.set(entry.etype.get() | UVM_ET_COPYONWRITE);
        if flags & UVM_FLAG_OVERLAY == 0 {
            entry.etype.set(entry.etype.get() | UVM_ET_NEEDSCOPY);
        }
    }
    if flags & UVM_FLAG_CONCEAL != 0 {
        entry.etype.set(entry.etype.get() | UVM_ET_CONCEAL);
    }
    if flags & UVM_FLAG_OVERLAY != 0 {
        entry.aref.ar_pageoff.set(0);
        entry
            .aref
            .ar_amap
            .set(amap_alloc(sz, M_WAITOK, false).map_or(ptr::null(), ptr::from_ref));
    }

    // Update map and process statistics.
    map.size.set(Vsize::new(map.size.get().as_usize() + sz));
    if prot != PROT_NONE {
        let vm = map.vmspace();
        vm.vm_dused
            .set(vm.vm_dused.get() + uvmspace_dused(map, *addr, *addr + sz) as i32);
    }

    Ok(())
}

/// `uvm_map`: establish a valid mapping in map.
///
/// - `*addr` and sz must be a multiple of `PAGE_SIZE`.
/// - map must be unlocked.
/// - `<uobj,uoffset>` value meanings (4 cases):
///   1. `<NULL,uoffset>`: uoffset is a hint for `PMAP_PREFER`;
///   2. `<NULL,UVM_UNKNOWN_OFFSET>`: don't `PMAP_PREFER`;
///   3. `<uobj,uoffset>`: normal mapping;
///   4. `<uobj,UVM_UNKNOWN_OFFSET>`: uvm_map finds offset based on VA.
///
///   Case 4 is for kernel mappings where we don't know the offset until we've found a
///   virtual address. Note that kernel object offsets are always relative to
///   `vm_map_min(kernel_map)`.
/// - align: align vaddr, must be a power-of-2. Align is only a hint and will be ignored if
///   the alignment fails.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_map(
    map: &VmMap,
    addr: &mut usize,
    sz: usize,
    uobj: Option<&'static UvmObject>,
    uoffset: Voff,
    align: usize,
    flags: u32,
) -> Result<(), Errno> {
    if map.flags.get() & VM_MAP_INTRSAFE == 0 {
        splassert(IPL_NONE, "uvm_map");
    } else {
        splassert(IPL_VM, "uvm_map");
    }

    // Decode parameters.
    let prot = uvm_protection(flags);
    let maxprot = uvm_maxprotection(flags);
    let dead = UvmMapDeadq::new();
    kassert!(sz & PAGE_MASK == 0);
    kassert!(align & align.wrapping_sub(1) == 0);

    // Holes are incompatible with other types of mappings.
    if flags & UVM_FLAG_HOLE != 0 {
        kassert!(
            uobj.is_none()
                && flags & UVM_FLAG_FIXED != 0
                && flags & (UVM_FLAG_OVERLAY | UVM_FLAG_COPYONW) == 0
        );
    }

    // Check protection.
    if prot & maxprot != prot {
        return Err(Errno::EACCES);
    }

    if is_kernel_map(map) && prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC) {
        panic(format_args!("uvm_map: kernel map W^X violation requested"));
    }

    // Before grabbing the lock, allocate a map entry for later use to ensure we don't wait
    // for memory while holding the vm_map_lock.
    let mut new = Some(uvm_mapent_alloc(map, flags).ok_or(Errno::ENOMEM)?);

    if flags & UVM_FLAG_TRYLOCK != 0 {
        if !vm_map_lock_try(map) {
            if let Some(new) = new {
                uvm_mapent_free(new);
            }
            return Err(Errno::EFAULT);
        }
    } else {
        vm_map_lock(map);
    }

    let error = uvm_map_locked(map, addr, sz, uobj, uoffset, align, flags, &dead, &mut new);
    vm_map_unlock(map);

    // Remove dead entries.
    //
    // Dead entries may be the result of merging. uvm_map_mkentry may also create dead
    // entries, when it attempts to destroy free-space entries.
    if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        uvm_unmap_detach_intrsafe(&dead);
    } else {
        uvm_unmap_detach(&dead, 0);
    }
    if let Some(new) = new {
        uvm_mapent_free(new);
    }
    error
}

/// [`uvm_map`] between its lock and unlock (the C's body up to the `unlock` label).
#[allow(clippy::too_many_arguments)] // the C's locals
fn uvm_map_locked(
    map: &VmMap,
    addr: &mut usize,
    sz: usize,
    uobj: Option<&'static UvmObject>,
    uoffset: Voff,
    align: usize,
    flags: u32,
    dead: &UvmMapDeadq,
    new: &mut Option<&'static VmMapEntry>,
) -> Result<(), Errno> {
    // We use pmap_align and pmap_offset as alignment and offset variables.
    //
    // Because the align parameter takes precedence over pmap prefer, the pmap_align will
    // need to be set to align, with pmap_offset = 0, if pmap_prefer will not align.
    let (pmap_align, pmap_offset) = if uoffset == UVM_UNKNOWN_OFFSET {
        (align.max(PAGE_SIZE), 0)
    } else {
        let pmap_align = pmap_prefer_align().max(PAGE_SIZE);
        let pmap_offset = pmap_prefer_offset(uoffset);

        if align == 0 || (align <= pmap_align && pmap_offset & (align - 1) == 0) {
            // pmap_offset satisfies align, no change.
            (pmap_align, pmap_offset)
        } else {
            // Align takes precedence over pmap prefer.
            (align, 0)
        }
    };

    let prot = uvm_protection(flags);
    let maxprot = uvm_maxprotection(flags);
    let advice = uvm_advice(flags);
    let inherit = uvm_inherit(flags);
    let mut hint = trunc_page(*addr);
    let mut uoffset = uoffset;

    // Unset hint for kernel_map non-fixed allocations.
    if map.flags.get() & VM_MAP_ISVMSPACE == 0 && flags & UVM_FLAG_FIXED == 0 {
        hint = 0;
    }

    let mut first = None;
    let mut last = None;
    if flags & UVM_FLAG_FIXED != 0 {
        // Fixed location.
        //
        // Note: we ignore align, pmap_prefer. Fill in first, last and *addr.
        kassert!(*addr & PAGE_MASK == 0);

        // Grow pmap to include allocated address. If the growth fails, the allocation will
        // fail too.
        if map.flags.get() & VM_MAP_ISVMSPACE == 0 && uvm_maxkaddr() < *addr + sz {
            uvm_map_kmem_grow(map, dead, *addr + sz - uvm_maxkaddr(), flags);
        }

        // Check that the space is available.
        if flags & UVM_FLAG_UNMAP != 0
            && uvm_unmap_remove(map, *addr, *addr + sz, dead, false, true, true).is_err()
        {
            return Err(Errno::EPERM); // immutable entries found
        }
        if !uvm_map_isavail(map, None, &mut first, &mut last, *addr, sz) {
            return Err(Errno::ENOMEM);
        }
    } else if *addr != 0
        && *addr & PAGE_MASK == 0
        && map.flags.get() & VM_MAP_ISVMSPACE == VM_MAP_ISVMSPACE
        && (align == 0 || *addr & (align - 1) == 0)
        && uvm_map_isavail(map, None, &mut first, &mut last, *addr, sz)
    {
        // Address used as hint.
        //
        // Note: we enforce the alignment restriction, but ignore pmap_prefer.
    } else if prot & PROT_EXEC != 0 && map.uaddr_exe.get().is_some() {
        // Run selection algorithm for executables.
        let mut found = uvm_addr_invoke(
            map,
            map.uaddr_exe.get(),
            sz,
            pmap_align,
            pmap_offset,
            prot,
            hint,
        );

        // Grow kernel memory and try again.
        if found.is_err() && map.flags.get() & VM_MAP_ISVMSPACE == 0 {
            uvm_map_kmem_grow(map, dead, sz, flags);

            found = uvm_addr_invoke(
                map,
                map.uaddr_exe.get(),
                sz,
                pmap_align,
                pmap_offset,
                prot,
                hint,
            );
        }

        let (f, l, a) = found?;
        first = Some(f);
        last = Some(l);
        *addr = a;
    } else {
        // Update freelists from vmspace.
        if map.flags.get() & VM_MAP_ISVMSPACE != 0 {
            uvm_map_vmspace_update(map, dead, flags);
        }

        let mut found = uvm_map_findspace(map, sz, pmap_align, pmap_offset, prot, hint);

        // Grow kernel memory and try again.
        if found.is_err() && map.flags.get() & VM_MAP_ISVMSPACE == 0 {
            uvm_map_kmem_grow(map, dead, sz, flags);

            found = uvm_map_findspace(map, sz, pmap_align, pmap_offset, prot, hint);
        }

        let (f, l, a) = found?;
        first = Some(f);
        last = Some(l);
        *addr = a;
    }

    // Double-check if selected address doesn't cause overflow.
    if addr.wrapping_add(sz) < *addr {
        return Err(Errno::ENOMEM);
    }

    kassert!(
        map.flags.get() & VM_MAP_ISVMSPACE == VM_MAP_ISVMSPACE || uvm_maxkaddr() >= *addr + sz
    );

    // If we only want a query, return now.
    if flags & UVM_FLAG_QUERY != 0 {
        return Ok(());
    }

    if uobj.is_none() {
        uoffset = 0;
    } else if uoffset == UVM_UNKNOWN_OFFSET {
        kassert!(uobj.is_some_and(uvm_obj_is_kern_object));
        uoffset = (*addr - kernel_map().min_offset.get()) as Voff;
    }

    // Create new entry. first and last may be invalidated after this call.
    let (Some(first), Some(last)) = (first, last) else {
        return Err(Errno::ENOMEM);
    };
    let entry = uvm_map_mkentry(map, first, last, *addr, sz, flags, dead, new.take())
        .ok_or(Errno::ENOMEM)?;
    kdassert!(entry.start.get() == *addr && entry.end.get() == *addr + sz);
    entry
        .object
        .set(uobj.map_or(VmMapEntryObject::None, VmMapEntryObject::Obj));
    entry.offset.set(uoffset);
    entry.protection.set(prot);
    entry.max_protection.set(maxprot);
    entry.inheritance.set(inherit);
    entry.wired_count.set(0);
    entry.advice.set(advice);
    let mut etype = entry.etype.get();
    if flags & UVM_FLAG_STACK != 0 {
        etype |= UVM_ET_STACK;
        if flags & UVM_FLAG_UNMAP != 0 {
            map.sserial.set(map.sserial.get() + 1);
        }
    }
    if uobj.is_some() {
        etype |= UVM_ET_OBJ;
    } else if flags & UVM_FLAG_HOLE != 0 {
        etype |= UVM_ET_HOLE;
    }
    if flags & UVM_FLAG_NOFAULT != 0 {
        etype |= UVM_ET_NOFAULT;
    }
    if flags & UVM_FLAG_WC != 0 {
        etype |= UVM_ET_WC;
    }
    if flags & UVM_FLAG_COPYONW != 0 {
        etype |= UVM_ET_COPYONWRITE;
        if flags & UVM_FLAG_OVERLAY == 0 {
            etype |= UVM_ET_NEEDSCOPY;
        }
    }
    if flags & UVM_FLAG_CONCEAL != 0 {
        etype |= UVM_ET_CONCEAL;
    }
    entry.etype.set(etype);
    if flags & UVM_FLAG_OVERLAY != 0 {
        entry.aref.ar_pageoff.set(0);
        entry
            .aref
            .ar_amap
            .set(amap_alloc(sz, M_WAITOK, false).map_or(ptr::null(), ptr::from_ref));
    }

    // Update map and process statistics.
    if flags & UVM_FLAG_HOLE == 0 {
        map.size.set(Vsize::new(map.size.get().as_usize() + sz));
        if map.flags.get() & VM_MAP_ISVMSPACE != 0 && uobj.is_none() && prot != PROT_NONE {
            let vm = map.vmspace();
            vm.vm_dused
                .set(vm.vm_dused.get() + uvmspace_dused(map, *addr, *addr + sz) as i32);
        }
    }

    // Try to merge entry.
    //
    // Userland allocations are kept separated most of the time. Forego the effort of
    // merging what most of the time can't be merged and only try the merge if it concerns
    // a kernel entry.
    if flags & UVM_FLAG_NOMERGE == 0 && map.flags.get() & VM_MAP_ISVMSPACE == 0 {
        uvm_mapent_tryjoin(map, entry, dead);
    }

    Ok(())
}

/// `uvm_mapent_isjoinable`: true iff e1 and e2 can be joined together.
pub fn uvm_mapent_isjoinable(_map: &VmMap, e1: &VmMapEntry, e2: &VmMapEntry) -> bool {
    // Must be the same entry type and not have free memory between.
    if e1.etype.get() != e2.etype.get() || e1.end.get() != e2.start.get() {
        return false;
    }

    // Submaps are never joined.
    if uvm_et_issubmap(e1) {
        return false;
    }

    // Never merge wired memory.
    if vm_mapent_iswired(e1) || vm_mapent_iswired(e2) {
        return false;
    }

    // Protection, inheritance and advice must be equal.
    if e1.protection.get() != e2.protection.get()
        || e1.max_protection.get() != e2.max_protection.get()
        || e1.inheritance.get() != e2.inheritance.get()
        || e1.advice.get() != e2.advice.get()
    {
        return false;
    }

    // If uvm_object: object itself and offsets within object must match.
    if uvm_et_isobj(e1) {
        match (e1.uvm_obj(), e2.uvm_obj()) {
            (Some(o1), Some(o2)) if ptr::eq(o1, o2) => {}
            _ => return false,
        }
        if e1.offset.get() + (e1.end.get() - e1.start.get()) as Voff != e2.offset.get() {
            return false;
        }
    }

    // Cannot join shared amaps. Note: no need to lock amap to look at refs, since we
    // don't care about its exact value. If it is 1 (i.e. we have the only reference) it
    // will stay there.
    if e1.aref.amap().is_some_and(|a| amap_refs(a) != 1) {
        return false;
    }
    if e2.aref.amap().is_some_and(|a| amap_refs(a) != 1) {
        return false;
    }

    // Apparently, e1 and e2 match.
    true
}

/// `uvm_mapent_merge`: join support function.
///
/// Returns the merged entry on success.
pub fn uvm_mapent_merge<'m>(
    map: &'m VmMap,
    e1: &'m VmMapEntry,
    e2: &'m VmMapEntry,
    dead: &UvmMapDeadq,
) -> Option<&'m VmMapEntry> {
    // Merging is not supported for map entries that contain an amap in e1. This should
    // never happen anyway, because only kernel entries are merged. These do not contain
    // amaps. e2 contains no real information in its amap, so it can be erased immediately.
    kassert!(e1.aref.amap().is_none());

    // Don't drop obj reference: uvm_unmap_detach will do this for us.
    let free = uvm_map_uaddr_e(map, e1);
    uvm_mapent_free_remove(map, free, e1);

    let free = uvm_map_uaddr_e(map, e2);
    uvm_mapent_free_remove(map, free, e2);
    uvm_mapent_addr_remove(map, e2);
    e1.end.set(e2.end.get());
    e1.guard.set(e2.guard.get());
    e1.fspace.set(e2.fspace.get());
    uvm_mapent_free_insert(map, free, e1);

    dead_entry_push(dead, e2);
    Some(e1)
}

/// `uvm_mapent_tryjoin`: attempt forward and backward joining of entry.
///
/// Returns entry after joins. We are guaranteed that the amap of entry is either
/// non-existent or has never been used.
pub fn uvm_mapent_tryjoin<'m>(
    map: &'m VmMap,
    entry: &'m VmMapEntry,
    dead: &UvmMapDeadq,
) -> &'m VmMapEntry {
    let mut entry = entry;

    // Merge with previous entry.
    if let Some(other) = RbtHead::<UvmMapAddr>::prev(entry)
        && uvm_mapent_isjoinable(map, other, entry)
        && let Some(merged) = uvm_mapent_merge(map, other, entry, dead)
    {
        entry = merged;
    }

    // Merge with next entry.
    //
    // Because amap can only extend forward and the next entry probably contains sensible
    // info, only perform forward merging in the absence of an amap.
    if let Some(other) = RbtHead::<UvmMapAddr>::next(entry)
        && entry.aref.amap().is_none()
        && other.aref.amap().is_none()
        && uvm_mapent_isjoinable(map, entry, other)
        && let Some(merged) = uvm_mapent_merge(map, entry, other, dead)
    {
        entry = merged;
    }

    entry
}

/// `uvm_unmap_detach`: kill entries that are no longer in a map.
pub fn uvm_unmap_detach(deadq: &UvmMapDeadq, flags: i32) {
    while let Some(entry) = deadq.first() {
        // SAFETY: a dead entry is a pool item or a static entry, alive until
        // `uvm_mapent_free` below hands it back.
        let entry: &'static VmMapEntry = unsafe { &*ptr::from_ref(entry) };
        // Drop reference to amap, if we've got one.
        if let Some(amap) = entry.aref.amap() {
            amap_unref(
                amap,
                entry.aref.ar_pageoff.get() as usize,
                atop(entry.end.get() - entry.start.get()),
                flags & AMAP_REFALL != 0,
            );
        }

        // Drop reference to our backing object, if we've got one.
        if uvm_et_issubmap(entry) {
            // ... unlikely to happen, but play it safe
            if let Some(submap) = entry.sub_map() {
                uvm_map_deallocate(submap);
            }
        } else if uvm_et_isobj(entry)
            && let Some(uobj) = entry.uvm_obj()
            && let Some(detach) = uobj.pgops().pgo_detach
        {
            detach(uobj);
        }

        // SAFETY: `entry` is the head of `deadq`.
        unsafe { deadq.remove(entry) };
        uvm_mapent_free(entry);
    }
}

/// `uvm_unmap_detach_intrsafe`: as [`uvm_unmap_detach`] for an intrsafe map, whose entries
/// hold no amap or object.
pub fn uvm_unmap_detach_intrsafe(deadq: &UvmMapDeadq) {
    while let Some(entry) = deadq.first() {
        // SAFETY: as in `uvm_unmap_detach`.
        let entry: &'static VmMapEntry = unsafe { &*ptr::from_ref(entry) };
        kassert!(entry.aref.amap().is_none());
        kassert!(!uvm_et_issubmap(entry));
        kassert!(!uvm_et_isobj(entry));
        // SAFETY: `entry` is the head of `deadq`.
        unsafe { deadq.remove(entry) };
        uvm_mapent_free(entry);
    }
}

/// `uvm_map_mkentry`: create and insert new entry.
///
/// Returned entry contains new addresses and is inserted properly in the tree. first and
/// last are (probably) no longer valid.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_map_mkentry<'m>(
    map: &'m VmMap,
    first: &'m VmMapEntry,
    last: &'m VmMapEntry,
    addr: usize,
    sz: usize,
    flags: u32,
    dead: &UvmMapDeadq,
    new: Option<&'static VmMapEntry>,
) -> Option<&'m VmMapEntry> {
    kdassert!(sz > 0);
    kdassert!(addr + sz > addr);
    kdassert!(first.end.get() <= addr && vmmap_free_end(first) > addr);
    kdassert!(last.start.get() < addr + sz && vmmap_free_end(last) >= addr + sz);
    kdassert!(uvm_map_isavail(
        map,
        None,
        &mut Some(first),
        &mut Some(last),
        addr,
        sz
    ));
    uvm_tree_sanity(map, "uvm_map_mkentry");

    let min = addr + sz;
    let max = vmmap_free_end(last);

    // Initialize new entry.
    let entry = match new {
        None => uvm_mapent_alloc(map, flags)?,
        Some(new) => new,
    };
    entry.offset.set(0);
    entry.etype.set(0);
    entry.wired_count.set(0);
    entry.aref.ar_pageoff.set(0);
    entry.aref.ar_amap.set(ptr::null());

    entry.start.set(addr);
    entry.end.set(min);
    entry.guard.set(0);
    entry.fspace.set(0);

    vm_map_assert_wrlock(map);

    // Reset free space in first.
    let free = uvm_map_uaddr_e(map, first);
    uvm_mapent_free_remove(map, free, first);
    first.guard.set(0);
    first.fspace.set(0);

    // Remove all entries that are fully replaced. We are iterating using last in reverse
    // order.
    let mut last = last;
    while !ptr::eq(first, last) {
        let prev = RbtHead::<UvmMapAddr>::prev(last);

        kdassert!(last.start.get() == last.end.get());
        let free = uvm_map_uaddr_e(map, last);
        uvm_mapent_free_remove(map, free, last);
        uvm_mapent_addr_remove(map, last);
        dead_entry_push(dead, last);

        let Some(prev) = prev else {
            panic(format_args!("uvm_map_mkentry: ran past first"));
        };
        last = prev;
    }
    // Remove first if it is entirely inside <addr, addr+sz>.
    if first.start.get() == addr {
        uvm_mapent_addr_remove(map, first);
        dead_entry_push(dead, first);
    } else {
        uvm_map_fix_space(map, Some(first), vmmap_free_start(first), addr, flags);
    }

    // Finally, link in entry.
    uvm_mapent_addr_insert(map, entry);
    uvm_map_fix_space(map, Some(entry), min, max, flags);

    uvm_tree_sanity(map, "uvm_map_mkentry");
    Some(entry)
}

/// `uvm_mapent_alloc`: allocate a map entry.
pub fn uvm_mapent_alloc(map: &VmMap, flags: u32) -> Option<&'static VmMapEntry> {
    let pool_flags = if flags & UVM_FLAG_TRYLOCK != 0 {
        PR_NOWAIT
    } else {
        PR_WAITOK
    };

    let me: &'static VmMapEntry;
    if map.flags.get() & VM_MAP_INTRSAFE != 0 || COLD.load(AtomicOrdering::Relaxed) {
        mtx_enter(&UVM_KMAPENT_MTX);
        if UVM.kentry_free.is_empty() {
            let Some(ne) = km_alloc(PAGE_SIZE, &KV_PAGE, &KP_DIRTY, &KD_NOWAIT) else {
                panic(format_args!("uvm_mapent_alloc: cannot allocate map entry"));
            };
            let ne = ne.cast::<VmMapEntry>();
            for i in 0..PAGE_SIZE / size_of::<VmMapEntry>() {
                // SAFETY: `ne` is a fresh page, suitably aligned, that holds that many
                // entries; each is written before it is linked, and the page is never
                // freed (the C does not free them either).
                let e: &'static VmMapEntry = unsafe {
                    let p = ne.as_ptr().add(i);
                    p.write(VmMapEntry::new());
                    &*p
                };
                // SAFETY: `uvm_kmapent_mtx` is held; `e` is on no list.
                unsafe { UVM.kentry_free.insert_head(e) };
            }
            // SAFETY: `uvm_kmapent_mtx` guards the last warn time.
            let last_warn = unsafe { UVM_KMAPENT_LAST_WARN_TIME.get_mut() };
            if ratecheck(last_warn, &UVM_KMAPENT_WARN_RATE) {
                kprintf!("uvm_mapent_alloc: out of static map entries\n");
            }
        }
        let Some(first) = UVM.kentry_free.first() else {
            panic(format_args!("uvm_mapent_alloc: kentry_free is empty"));
        };
        // SAFETY: the static kernel entries and the pages above live forever.
        me = unsafe { &*ptr::from_ref(first) };
        // SAFETY: `uvm_kmapent_mtx` is held; `me` is the head of the list.
        unsafe { UVM.kentry_free.remove_head() };
        UVMEXP.kmapent.fetch_add(1, AtomicOrdering::Relaxed);
        mtx_leave(&UVM_KMAPENT_MTX);
        me.flags.set(UVM_MAP_STATIC);
    } else if is_kernel_map(map) {
        splassert(IPL_NONE, "uvm_mapent_alloc");
        let mem = pool_get(&UVM_MAP_ENTRY_KMEM_POOL, pool_flags)?;
        let p = mem.cast::<VmMapEntry>();
        // SAFETY: a fresh, suitably aligned pool item of `size_of::<VmMapEntry>()` bytes,
        // written once before anything else sees it; it lives until `uvm_mapent_free`.
        me = unsafe {
            p.as_ptr().write(VmMapEntry::new());
            p.as_ref()
        };
        me.flags.set(UVM_MAP_KMEM);
    } else {
        splassert(IPL_NONE, "uvm_mapent_alloc");
        let mem = pool_get(&UVM_MAP_ENTRY_POOL, pool_flags)?;
        let p = mem.cast::<VmMapEntry>();
        // SAFETY: as above.
        me = unsafe {
            p.as_ptr().write(VmMapEntry::new());
            p.as_ref()
        };
        me.flags.set(0);
    }

    RbtHead::<UvmMapAddr>::poison(me, UVMMAP_DEADBEEF);
    Some(me)
}

/// `uvm_mapent_free`: free map entry.
///
/// - XXX: static pool for kernel map?
pub fn uvm_mapent_free(me: &'static VmMapEntry) {
    if me.flags.get() & UVM_MAP_STATIC != 0 {
        mtx_enter(&UVM_KMAPENT_MTX);
        // SAFETY: `uvm_kmapent_mtx` is held; a freed entry is on no list.
        unsafe { UVM.kentry_free.insert_head(me) };
        UVMEXP.kmapent.fetch_sub(1, AtomicOrdering::Relaxed);
        mtx_leave(&UVM_KMAPENT_MTX);
    } else if me.flags.get() & UVM_MAP_KMEM != 0 {
        splassert(IPL_NONE, "uvm_mapent_free");
        pool_put(&UVM_MAP_ENTRY_KMEM_POOL, NonNull::from(me).cast::<u8>());
    } else {
        splassert(IPL_NONE, "uvm_mapent_free");
        pool_put(&UVM_MAP_ENTRY_POOL, NonNull::from(me).cast::<u8>());
    }
}

/// `uvm_map_lookup_entry`: find map entry at or before an address.
///
/// - map must at least be read-locked by caller
/// - the entry is returned when the address is in it; `ET_HOLE` entries are considered to
///   not contain a mapping, ergo `None` is returned for those mappings.
pub fn uvm_map_lookup_entry(map: &VmMap, address: usize) -> Option<&VmMapEntry> {
    vm_map_assert_anylock(map);

    uvm_map_entrybyaddr(&map.addr, address).filter(|entry| {
        !uvm_et_ishole(entry) && entry.start.get() <= address && entry.end.get() > address
    })
}

/// `uvm_map_inentry_sp`: stack must be in a `MAP_STACK` entry. `PROT_NONE` indicates stack
/// not yet grown -- then `uvm_map_check_region_range()` should not cache the entry because
/// growth won't be seen.
pub fn uvm_map_inentry_sp(entry: &VmMapEntry) -> i32 {
    if entry.etype.get() & UVM_ET_STACK == 0 {
        if entry.protection.get() == PROT_NONE {
            return -1; // don't update range
        }
        return 0;
    }
    1
}

/// `uvm_map_inentry_recheck`.
pub fn uvm_map_inentry_recheck(serial: u64, addr: usize, ie: &PInentry) -> bool {
    serial != ie.ie_serial || ie.ie_start == 0 || addr < ie.ie_start || addr >= ie.ie_end
}

/// `uvm_map_inentry_fix`: inside a vm_map find the reg address and verify it via function.
/// Remember low and high addresses of region if valid and return true, else return false.
pub fn uvm_map_inentry_fix(
    p: &Proc,
    ie: &mut PInentry,
    addr: usize,
    f: fn(&VmMapEntry) -> i32,
    serial: u64,
) -> bool {
    let map = &p.vmspace().vm_map;

    if addr < map.min_offset.get() || addr >= map.max_offset.get() {
        return false;
    }

    // lock map
    vm_map_lock_read(map);

    // lookup
    let Some(entry) = uvm_map_lookup_entry(map, trunc_page(addr)) else {
        vm_map_unlock_read(map);
        return false;
    };

    let ret = f(entry);
    if ret == 0 {
        vm_map_unlock_read(map);
        return false;
    } else if ret == 1 {
        ie.ie_start = entry.start.get();
        ie.ie_end = entry.end.get();
        ie.ie_serial = serial;
    } else {
        // do not update, re-check later
    }
    vm_map_unlock_read(map);
    true
}

/// `uvm_map_inentry`: checks that `addr` (the `what` register: "sp", "pc") lies in an entry
/// `f` accepts, caching the result in `ie`; on failure prints the C's message with
/// `reason` and signals the thread (see the module's deviations).
pub fn uvm_map_inentry(
    p: &Proc,
    ie: &mut PInentry,
    addr: usize,
    what: &str,
    reason: &str,
    f: fn(&VmMapEntry) -> i32,
    serial: u64,
) -> bool {
    let mut ok = true;

    if uvm_map_inentry_recheck(serial, addr, ie) {
        ok = uvm_map_inentry_fix(p, ie, addr, f, serial);
        if !ok {
            kernel_lock(); // KERNEL_LOCK()
            let pr = p.process();
            kprintf!(
                "[{}]{}/{} {}={:#x} inside {:#x}-{:#x}: {}\n",
                core::str::from_utf8(pr.comm()).unwrap_or("?"),
                pr.ps_pid.get(),
                p.p_tid.get(),
                what,
                addr,
                ie.ie_start,
                ie.ie_end.wrapping_sub(1),
                reason
            );
            pr.ps_acflag.set(pr.ps_acflag.get() | AMAP);
            let sv = Sigval::from_ptr(<Machine as Cpu>::proc_pc(p));
            trapsignal(p, SIGSEGV, 0, SEGV_ACCERR, sv);
            kernel_unlock(); // KERNEL_UNLOCK()
        }
    }
    ok
}

/// `uvm_map_is_stack_remappable`: check whether the given address range can be converted
/// to a `MAP_STACK` mapping.
///
/// Must be called with map locked.
pub fn uvm_map_is_stack_remappable(
    map: &VmMap,
    addr: usize,
    sz: usize,
    sigaltstack_check: bool,
) -> bool {
    let end = addr + sz;

    vm_map_assert_anylock(map);

    let Some(first) = uvm_map_lookup_entry(map, addr) else {
        return false;
    };

    // Check that the address range exists and is contiguous.
    let mut prev: Option<&VmMapEntry> = None;
    let mut iter = Some(first);
    while let Some(entry) = iter.filter(|e| e.start.get() < end) {
        // Make sure that we do not have holes in the range.
        if prev.is_some_and(|p| p.end.get() != entry.start.get()) {
            return false;
        }
        if entry.start.get() == entry.end.get() || uvm_et_ishole(entry) {
            return false;
        }
        if sigaltstack_check && entry.protection.get() != (PROT_READ | PROT_WRITE) {
            return false;
        }
        prev = Some(entry);
        iter = RbtHead::<UvmMapAddr>::next(entry);
    }

    true
}

/// `uvm_map_remap_as_stack`: remap the middle-pages of an existing mapping as a stack
/// range. If there exists a previous contiguous mapping with the given range
/// `[addr, addr + sz)`, with protection `PROT_READ|PROT_WRITE`, then the mapping is
/// dropped, and a new anon mapping is created and marked as a stack.
///
/// Must be called with map unlocked.
pub fn uvm_map_remap_as_stack(p: &Proc, addr: usize, sz: usize) -> Result<(), Errno> {
    let map = &p.vmspace().vm_map;
    let flags = uvm_mapflag(
        PROT_READ | PROT_WRITE,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_INHERIT_COPY,
        MADV_NORMAL,
        UVM_FLAG_STACK | UVM_FLAG_FIXED | UVM_FLAG_UNMAP | UVM_FLAG_COPYONW | UVM_FLAG_SIGALTSTACK,
    );

    let mut start = round_page(addr);
    let end = trunc_page(addr + sz);
    // MACHINE_STACK_GROWS_UP is not defined.
    if start == addr {
        start += PAGE_SIZE;
    }

    if start < map.min_offset.get() || end >= map.max_offset.get() || end < start {
        return Err(Errno::EINVAL);
    }

    // UVM_FLAG_SIGALTSTACK indicates that immutable may be bypassed, but the range is
    // checked that it is contiguous, is not a syscall mapping, and protection RW. Then, a
    // new mapping (all zero) is placed upon the region, which prevents an attacker from
    // pivoting into pre-placed MAP_STACK space.
    let len = end - start;
    uvm_mapanon(map, &mut start, len, 0, flags)
}

/// `VM_PIE_MAX_ADDR` (the default).
const VM_PIE_MAX_ADDR: usize = <Machine as VmParam>::VM_MAXUSER_ADDRESS / 4;
/// `VM_PIE_MIN_ADDR` (the default).
const VM_PIE_MIN_ADDR: usize = <Machine as VmParam>::VM_MIN_ADDRESS;
/// `VM_PIE_MIN_ALIGN` (the default).
const VM_PIE_MIN_ALIGN: usize = PAGE_SIZE;

/// `uvm_map_pie`: return a random load address for a PIE executable properly aligned.
pub fn uvm_map_pie(align: usize) -> usize {
    let align = align.max(VM_PIE_MIN_ALIGN);

    // round up to next alignment
    let min = (VM_PIE_MIN_ADDR + align - 1) & !(align - 1);

    if align >= VM_PIE_MAX_ADDR || min >= VM_PIE_MAX_ADDR {
        return align;
    }

    let space = ((VM_PIE_MAX_ADDR - min) / align).min(u32::MAX as usize);

    let addr = arc4random_uniform(space as u32) as usize * align;
    addr + min
}

/// `uvm_unmap`: removes the mappings in `[start, end)` from the map.
pub fn uvm_unmap(map: &VmMap, start: usize, end: usize) {
    kassert!(start & PAGE_MASK == 0 && end & PAGE_MASK == 0);
    let dead = UvmMapDeadq::new();
    vm_map_lock(map);
    let _ = uvm_unmap_remove(map, start, end, &dead, false, true, false);
    vm_map_unlock(map);

    if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        uvm_unmap_detach_intrsafe(&dead);
    } else {
        uvm_unmap_detach(&dead, 0);
    }
}

/// `uvm_mapent_mkfree`: mark entry as free.
///
/// entry will be put on the dead list. The free space will be merged into the previous or
/// a new entry, unless markfree is false.
pub fn uvm_mapent_mkfree<'m>(
    map: &'m VmMap,
    entry: &'m VmMapEntry,
    prev_ptr: &mut Option<&'m VmMapEntry>,
    dead: &UvmMapDeadq,
    markfree: bool,
) {
    uvm_map_req_write(map);

    let mut prev = *prev_ptr;
    if prev.is_some_and(|p| ptr::eq(p, entry)) {
        *prev_ptr = None;
        prev = None;
    }

    if prev.is_none_or(|p| vmmap_free_end(p) != entry.start.get()) {
        prev = RbtHead::<UvmMapAddr>::prev(entry);
    }

    // Entry is describing only free memory and has nothing to drain into.
    if prev.is_none() && entry.start.get() == entry.end.get() && markfree {
        *prev_ptr = Some(entry);
        return;
    }

    let addr = entry.start.get();
    let end = vmmap_free_end(entry);
    let free = uvm_map_uaddr_e(map, entry);
    uvm_mapent_free_remove(map, free, entry);
    uvm_mapent_addr_remove(map, entry);
    dead_entry_push(dead, entry);

    if markfree {
        if let Some(prev) = prev {
            let free = uvm_map_uaddr_e(map, prev);
            uvm_mapent_free_remove(map, free, prev);
        }
        *prev_ptr = uvm_map_fix_space(map, prev, addr, end, 0);
    }
}

/// `uvm_unmap_kill_entry_withlock`: unwire and release referenced amap and object from map
/// entry.
pub fn uvm_unmap_kill_entry_withlock(map: &VmMap, entry: &VmMapEntry, needlock: bool) {
    // Unwire removed map entry.
    if vm_mapent_iswired(entry) {
        entry.wired_count.set(0);
        uvm_fault_unwire_locked(map, entry.start.get(), entry.end.get());
    }

    if needlock {
        uvm_map_lock_entry(entry);
    }

    // Entry-type specific code.
    if uvm_et_ishole(entry) {
        // Nothing to be done for holes.
    } else if map.flags.get() & VM_MAP_INTRSAFE != 0 {
        kassert!(ptr::eq(map.pmap(), pmap_kernel()));

        uvm_km_pgremove_intrsafe(Vaddr::new(entry.start.get()), Vaddr::new(entry.end.get()));
    } else if let Some(uobj) = entry
        .uvm_obj()
        .filter(|uobj| uvm_et_isobj(entry) && uvm_obj_is_kern_object(uobj))
    {
        kassert!(ptr::eq(map.pmap(), pmap_kernel()));
        // Note: kernel object mappings are currently used in two ways:
        //  [1] "normal" mappings of pages in the kernel object
        //  [2] uvm_km_valloc'd allocations in which we pmap_enter in some
        //      non-kernel-object page (e.g. vmapbuf).
        //
        // for case [1], we need to remove the mapping from the pmap and then remove the
        // page from the kernel object (because, once pages in a kernel object are unmapped
        // they are no longer needed, unlike, say, a vnode where you might want the data to
        // persist until flushed out of a queue).
        //
        // for case [2], we need to remove the mapping from the pmap. there shouldn't be
        // any pages at the specified offset in the kernel object [but it doesn't hurt to
        // call uvm_km_pgremove just to be safe?]
        //
        // uvm_km_pgremove currently does the following: for pages in the kernel object
        // range: drops the swap slot, uvm_pagefree the page.
        //
        // note there is version of uvm_km_pgremove() that is used for "intrsafe" objects.
        //
        // remove mappings from pmap and drop the pages from the object. offsets are always
        // relative to vm_map_min(kernel_map).
        uvm_km_pgremove(
            uobj,
            Vaddr::new(entry.start.get()),
            Vaddr::new(entry.end.get()),
        );
    } else {
        // remove mappings the standard way.
        pmap_remove(
            map.pmap(),
            Vaddr::new(entry.start.get()),
            Vaddr::new(entry.end.get()),
        );
    }

    if needlock {
        uvm_map_unlock_entry(entry);
    }
}

/// `uvm_unmap_kill_entry`.
pub fn uvm_unmap_kill_entry(map: &VmMap, entry: &VmMapEntry) {
    uvm_unmap_kill_entry_withlock(map, entry, false);
}

/// `uvm_unmap_remove`: remove all entries from start to end.
///
/// If remove_holes, then remove `ET_HOLE` entries as well. If markfree, entry will be
/// properly marked free, otherwise, no replacement entry will be put in the tree
/// (corrupting the tree). `EPERM` when checkimmutable finds an immutable entry.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_unmap_remove(
    map: &VmMap,
    start: usize,
    end: usize,
    dead: &UvmMapDeadq,
    remove_holes: bool,
    markfree: bool,
    checkimmutable: bool,
) -> Result<(), Errno> {
    let start = start.max(map.min_offset.get());
    let end = end.min(map.max_offset.get());
    if start >= end {
        return Ok(());
    }

    vm_map_assert_wrlock(map);

    // Find first affected entry.
    let entry = entry_at(map, start, "uvm_unmap_remove");
    kdassert!(entry.start.get() <= start);

    if checkimmutable {
        let mut entry1 = Some(entry);

        // Refuse to unmap if any entries are immutable
        if entry.end.get() <= start {
            entry1 = RbtHead::<UvmMapAddr>::next(entry);
        }
        while let Some(e1) = entry1.filter(|e| e.start.get() < end) {
            kdassert!(e1.start.get() >= start);
            let next = RbtHead::<UvmMapAddr>::next(e1);
            // Treat memory holes as free space.
            if !(e1.start.get() == e1.end.get() || uvm_et_ishole(e1))
                && e1.etype.get() & UVM_ET_IMMUTABLE != 0
            {
                return Err(Errno::EPERM);
            }
            entry1 = next;
        }
    }

    let mut iter = if entry.end.get() <= start && markfree {
        RbtHead::<UvmMapAddr>::next(entry)
    } else {
        uvm_map_clip_start_at(map, entry, start);
        Some(entry)
    };

    // Iterate entries until we reach end address. prev_hint hints where the freed space
    // can be appended to.
    let mut prev_hint: Option<&VmMapEntry> = None;
    while let Some(entry) = iter.filter(|e| e.start.get() < end) {
        kdassert!(entry.start.get() >= start);
        if entry.end.get() > end || !markfree {
            uvm_map_clip_end_at(map, entry, end);
        }
        kdassert!(entry.start.get() >= start && entry.end.get() <= end);
        let next = RbtHead::<UvmMapAddr>::next(entry);

        // Don't remove holes unless asked to do so.
        if uvm_et_ishole(entry) && !remove_holes {
            prev_hint = Some(entry);
            iter = next;
            continue;
        }

        // A stack has been removed..
        if uvm_et_isstack(entry) && map.flags.get() & VM_MAP_ISVMSPACE != 0 {
            map.sserial.set(map.sserial.get() + 1);
        }

        // Kill entry.
        uvm_unmap_kill_entry_withlock(map, entry, true);

        // Update space usage.
        if map.flags.get() & VM_MAP_ISVMSPACE != 0
            && entry.uvm_obj().is_none()
            && entry.protection.get() != PROT_NONE
            && !uvm_et_ishole(entry)
        {
            let vm = map.vmspace();
            vm.vm_dused.set(
                vm.vm_dused.get() - uvmspace_dused(map, entry.start.get(), entry.end.get()) as i32,
            );
        }
        if !uvm_et_ishole(entry) {
            map.size.set(Vsize::new(
                map.size.get().as_usize() - (entry.end.get() - entry.start.get()),
            ));
        }

        // Actual removal of entry.
        uvm_mapent_mkfree(map, entry, &mut prev_hint, dead, markfree);
        iter = next;
    }

    pmap_update(map.pmap());

    #[cfg(any(test, feature = "vmmap_debug"))]
    {
        if markfree {
            let mut iter = uvm_map_entrybyaddr(&map.addr, start);
            while let Some(entry) = iter.filter(|e| e.start.get() < end) {
                kdassert!(
                    entry.end.get() <= start
                        || entry.start.get() == entry.end.get()
                        || uvm_et_ishole(entry)
                );
                iter = RbtHead::<UvmMapAddr>::next(entry);
            }
        } else {
            let mut a = start;
            while a < end {
                kdassert!(uvm_map_entrybyaddr(&map.addr, a).is_none());
                a += PAGE_SIZE;
            }
        }
    }
    Ok(())
}

/// `uvm_map_pageable_pgon`: mark all entries from first until end (exclusive) as pageable.
///
/// Lock must be exclusive on entry and will not be touched.
pub fn uvm_map_pageable_pgon(
    map: &VmMap,
    first: Option<&VmMapEntry>,
    end: Option<&VmMapEntry>,
    start_addr: usize,
    end_addr: usize,
) {
    let mut iter = first;
    while let Some(e) = until(iter, end) {
        kdassert!(e.start.get() >= start_addr && e.end.get() <= end_addr);
        if vm_mapent_iswired(e) && !uvm_et_ishole(e) {
            e.wired_count.set(0);
            uvm_fault_unwire_locked(map, e.start.get(), e.end.get());
        }
        iter = RbtHead::<UvmMapAddr>::next(e);
    }
}

/// `uvm_map_pageable_wire`: mark all entries from first until end (exclusive) as wired.
///
/// Lockflags determines the lock state on return from this function. Lock must be
/// exclusive on entry.
pub fn uvm_map_pageable_wire(
    map: &VmMap,
    first: Option<&VmMapEntry>,
    end: Option<&VmMapEntry>,
    start_addr: usize,
    end_addr: usize,
    lockflags: i32,
) -> Result<(), Errno> {
    let skip = |e: &VmMapEntry| {
        uvm_et_ishole(e) || e.start.get() == e.end.get() || e.protection.get() == PROT_NONE
    };

    // Wire pages in two passes:
    //
    // 1: holding the write lock, we create any anonymous maps that need to be created.
    //    then we clip each map entry to the region to be wired and increment its wiring
    //    count.
    //
    // 2: we mark the map busy, unlock it and call uvm_fault_wire to fault in the pages for
    //    any newly wired area (wired_count == 1).
    let mut iter = first;
    while let Some(e) = until(iter, end) {
        kdassert!(e.start.get() >= start_addr && e.end.get() <= end_addr);
        if !skip(e) {
            // Perform actions of vm_map_lookup that need the write lock: create an
            // anonymous map for copy-on-write, anonymous map for zero-fill. Skip submaps.
            if !vm_mapent_iswired(e)
                && !uvm_et_issubmap(e)
                && uvm_et_isneedscopy(e)
                && (e.protection.get() & PROT_WRITE != 0 || e.uvm_obj().is_none())
            {
                amap_copy(
                    map,
                    e,
                    M_WAITOK,
                    !uvm_et_isstack(e),
                    e.start.get(),
                    e.end.get(),
                );
            }
            e.wired_count.set(e.wired_count.get() + 1);
        }
        iter = RbtHead::<UvmMapAddr>::next(e);
    }

    // Pass 2.
    #[cfg(feature = "diagnostic")]
    let timestamp_save = map.timestamp.get();
    vm_map_busy(map);
    vm_map_unlock(map);

    let mut error = Ok(());
    let mut iter = first;
    let mut failed_at: Option<&VmMapEntry> = None;
    while let Some(e) = until(iter, end) {
        if !skip(e) && e.wired_count.get() == 1 {
            error = uvm_fault_wire(map, e.start.get(), e.end.get(), e.protection.get());
            if error.is_err() {
                failed_at = Some(e);
                break;
            }
        }
        iter = RbtHead::<UvmMapAddr>::next(e);
    }

    vm_map_lock(map);
    vm_map_unbusy(map);

    if error.is_err() {
        #[cfg(feature = "diagnostic")]
        if timestamp_save != map.timestamp.get() {
            panic(format_args!("uvm_map_pageable_wire: stale map"));
        }

        // first is no longer needed to restart loops. Use it as iterator to unwire entries
        // that were successfully wired above.
        let mut iter = first;
        while let Some(e) = until(iter, failed_at) {
            if !skip(e) {
                e.wired_count.set(e.wired_count.get() - 1);
                if !vm_mapent_iswired(e) {
                    uvm_fault_unwire_locked(map, e.start.get(), e.end.get());
                }
            }
            iter = RbtHead::<UvmMapAddr>::next(e);
        }

        // decrease counter in the rest of the entries
        let mut iter = failed_at;
        while let Some(e) = until(iter, end) {
            if !skip(e) {
                e.wired_count.set(e.wired_count.get() - 1);
            }
            iter = RbtHead::<UvmMapAddr>::next(e);
        }

        if lockflags & UVM_LK_EXIT == 0 {
            vm_map_unlock(map);
        }
        return error;
    }

    if lockflags & UVM_LK_EXIT == 0 {
        vm_map_unlock(map);
    } else {
        #[cfg(feature = "diagnostic")]
        if timestamp_save != map.timestamp.get() {
            panic(format_args!("uvm_map_pageable_wire: stale map"));
        }
    }
    Ok(())
}

/// `uvm_map_pageable`: set pageability of a range in a map.
///
/// Flags: `UVM_LK_ENTER`: map is already locked by caller; `UVM_LK_EXIT`: don't unlock
/// map on exit.
///
/// The full range must be in use (entries may not have fspace != 0). `UVM_ET_HOLE` counts
/// as unmapped.
pub fn uvm_map_pageable(
    map: &VmMap,
    start: usize,
    end: usize,
    new_pageable: bool,
    lockflags: i32,
) -> Result<(), Errno> {
    let start = trunc_page(start);
    let end = round_page(end);

    if start > end {
        return Err(Errno::EINVAL);
    }
    if start == end {
        return Ok(()); // nothing to do
    }
    if start < map.min_offset.get() {
        return Err(Errno::EFAULT); // why? see first XXX below
    }
    if end > map.max_offset.get() {
        return Err(Errno::EINVAL); // why? see second XXX below
    }

    kassert!(map.flags.get() & VM_MAP_PAGEABLE != 0);
    if lockflags & UVM_LK_ENTER == 0 {
        vm_map_lock(map);
    }

    let result = uvm_map_pageable_locked(map, start, end, new_pageable, lockflags);
    if let Err(e) = result {
        if lockflags & UVM_LK_EXIT == 0 {
            vm_map_unlock(map);
        }
        return Err(e);
    }
    Ok(())
}

/// [`uvm_map_pageable`] with the map locked; the wire path hands the lock to
/// [`uvm_map_pageable_wire`], the pageable path releases it unless `UVM_LK_EXIT`.
fn uvm_map_pageable_locked(
    map: &VmMap,
    start: usize,
    end: usize,
    new_pageable: bool,
    lockflags: i32,
) -> Result<(), Errno> {
    // Find first entry.
    //
    // Initial test on start is different, because of the different error returned. Rest
    // is tested further down.
    let first = entry_at(map, start, "uvm_map_pageable");
    if first.end.get() <= start || uvm_et_ishole(first) {
        // XXX if the first address is not mapped, it is EFAULT?
        return Err(Errno::EFAULT);
    }

    // Check that the range has no holes.
    let mut last = Some(first);
    while let Some(l) = last.filter(|l| l.start.get() < end) {
        if uvm_et_ishole(l) || (l.end.get() < end && vmmap_free_end(l) != l.end.get()) {
            // XXX unmapped memory in range, why is it EINVAL instead of EFAULT?
            return Err(Errno::EINVAL);
        }
        last = RbtHead::<UvmMapAddr>::next(l);
    }

    // Last ended at the first entry after the range. Move back one step.
    //
    // Note that last may be NULL.
    let last = match last {
        None => {
            let Some(max) = map.addr.max() else {
                return Err(Errno::EINVAL);
            };
            if max.end.get() < end {
                return Err(Errno::EINVAL);
            }
            max
        }
        Some(l) => {
            kassert!(!ptr::eq(l, first));
            let Some(prev) = RbtHead::<UvmMapAddr>::prev(l) else {
                return Err(Errno::EINVAL);
            };
            prev
        }
    };

    // Wire/unwire pages here.
    if new_pageable {
        // Mark pageable. entries that are not wired are untouched.
        if vm_mapent_iswired(first) {
            uvm_map_clip_start_at(map, first, start);
        }
        // Split last at end. Make tmp be the first entry after what is to be touched. If
        // last is not wired, don't touch it.
        let tmp = if vm_mapent_iswired(last) {
            uvm_map_clip_end_at(map, last, end);
            RbtHead::<UvmMapAddr>::next(last)
        } else {
            Some(last)
        };

        uvm_map_pageable_pgon(map, Some(first), tmp, start, end);

        if lockflags & UVM_LK_EXIT == 0 {
            vm_map_unlock(map);
        }
        Ok(())
    } else {
        // Mark entries wired. entries are always touched (because recovery needs this).
        if !vm_mapent_iswired(first) {
            uvm_map_clip_start_at(map, first, start);
        }
        // Split last at end. Make tmp be the first entry after what is to be touched. If
        // last is not wired, don't touch it.
        let tmp = if !vm_mapent_iswired(last) {
            uvm_map_clip_end_at(map, last, end);
            RbtHead::<UvmMapAddr>::next(last)
        } else {
            Some(last)
        };

        // uvm_map_pageable_wire releases the lock (unless UVM_LK_EXIT), on error too.
        match uvm_map_pageable_wire(map, Some(first), tmp, start, end, lockflags) {
            Ok(()) => Ok(()),
            Err(e) => {
                // The lock is already handled by the wire function; relock for the caller's
                // unlock when it expects the lock to still be held.
                if lockflags & UVM_LK_EXIT == 0 {
                    vm_map_lock(map);
                }
                Err(e)
            }
        }
    }
}

/// `uvm_map_pageable_all`: special case of `uvm_map_pageable` - affects all mapped
/// regions.
///
/// Map must not be locked. If no flags are specified, all regions are unwired.
pub fn uvm_map_pageable_all(map: &VmMap, flags: i32, limit: usize) -> Result<(), Errno> {
    kassert!(map.flags.get() & VM_MAP_PAGEABLE != 0);
    vm_map_lock(map);

    if flags == 0 {
        uvm_map_pageable_pgon(
            map,
            map.addr.min(),
            None,
            map.min_offset.get(),
            map.max_offset.get(),
        );

        vm_map_modflags(map, 0, VM_MAP_WIREFUTURE);
        vm_map_unlock(map);
        return Ok(());
    }

    if flags & MCL_FUTURE != 0 {
        vm_map_modflags(map, VM_MAP_WIREFUTURE, 0);
    }
    if flags & MCL_CURRENT == 0 {
        vm_map_unlock(map);
        return Ok(());
    }

    // Count number of pages in all non-wired entries. If the number exceeds the limit,
    // abort.
    let mut size = 0;
    for iter in map.addr.iter() {
        if vm_mapent_iswired(iter) || uvm_et_ishole(iter) {
            continue;
        }

        size += iter.end.get() - iter.start.get();
    }

    if atop(size) as i64 + UVMEXP.wired.load(AtomicOrdering::Relaxed) as i64
        > UVMEXP.wiredmax.load(AtomicOrdering::Relaxed) as i64
    {
        vm_map_unlock(map);
        return Err(Errno::ENOMEM);
    }

    // XXX non-pmap_wired_count case must be handled by caller (pmap_wired_count is defined
    // on amd64 and arm64).
    if limit != 0 && size + ptoa(pmap_wired_count(map.pmap()) as usize) > limit {
        vm_map_unlock(map);
        return Err(Errno::ENOMEM);
    }

    // uvm_map_pageable_wire will release lock
    uvm_map_pageable_wire(
        map,
        map.addr.min(),
        None,
        map.min_offset.get(),
        map.max_offset.get(),
        0,
    )
}

/// `uvm_map_setup`: initialize map.
///
/// Allocates sufficient entries to describe the free memory in the map.
pub fn uvm_map_setup(map: &VmMap, pmap: &'static MachinePmap, min: usize, max: usize, flags: i32) {
    let mut max = max;

    kassert!(min & PAGE_MASK == 0);
    kassert!(max & PAGE_MASK == 0 || max & PAGE_MASK == PAGE_MASK);

    // Update parameters.
    //
    // This code handles (vaddr_t)-1 and other page mask ending addresses properly. We lose
    // the top page if the full virtual address space is used.
    if max & PAGE_MASK != 0 {
        max = max.wrapping_add(1);
        if max == 0 {
            // overflow
            max -= PAGE_SIZE;
        }
    }

    map.addr.init();
    map.uaddr_exe.set(None);
    for slot in &map.uaddr_any {
        slot.set(None);
    }
    map.uaddr_brk_stack.set(None);

    map.pmap.set(pmap);
    map.size.set(Vsize::new(0));
    map.ref_count.store(0, AtomicOrdering::Relaxed);
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
    if flags & VM_MAP_ISVMSPACE != 0 {
        rw_init_flags(&map.lock, "vmmaplk", RWL_DUPOK);
    } else {
        rw_init(&map.lock, "kmmaplk");
    }
    // mtx_init(&map->mtx, IPL_VM), mtx_init(&map->flags_lock, IPL_VM): statically
    // initialised.

    // Configure the allocators.
    if flags & VM_MAP_ISVMSPACE != 0 {
        uvm_map_setup_md(map);
    } else {
        map.uaddr_any[3].set(Some(&UADDR_KBOOTSTRAP));
    }

    // Fill map entries. We do not need to write-lock the map here because only the current
    // thread sees it right now. Initialize ref_count to 0 above to avoid bogus triggering of
    // lock-not-held assertions.
    uvm_map_setup_entries(map);
    uvm_tree_sanity(map, "uvm_map_setup");
    map.ref_count.store(1, AtomicOrdering::Relaxed);
}

/// `uvm_map_teardown`: destroy the map.
///
/// This is the inverse operation to `uvm_map_setup`.
pub fn uvm_map_teardown(map: &VmMap) {
    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);

    vm_map_lock(map);

    // Remove entries.
    //
    // The following is based on graph breadth-first search.
    //
    // In color terms: the dead_entries set contains all nodes that are reachable (i.e. both
    // the black and the grey nodes); any entry not in dead_entries is white; any entry that
    // appears in dead_entries before entry, is black, the rest is grey. The set [entry,
    // end] is also referred to as the wavefront.
    //
    // Since the tree is always a fully connected graph, the breadth-first search guarantees
    // that each vmmap_entry is visited exactly once. The vm_map is broken down in linear
    // time.
    let dead_entries = UvmMapDeadq::new();
    let mut entry = map.addr.root();
    if let Some(root) = entry {
        dead_entry_push(&dead_entries, root);
    }
    while let Some(e) = entry {
        sched_pause(r#yield);
        uvm_unmap_kill_entry(map, e);
        if let Some(tmp) = RbtHead::<UvmMapAddr>::left(e) {
            dead_entry_push(&dead_entries, tmp);
        }
        if let Some(tmp) = RbtHead::<UvmMapAddr>::right(e) {
            dead_entry_push(&dead_entries, tmp);
        }
        // Update wave-front.
        entry = UvmMapDeadq::next(e);
    }

    #[cfg(any(test, feature = "vmmap_debug"))]
    {
        let numt = map.addr.iter().count();
        let numq = dead_entries.iter().count();
        kassert!(numt == numq);
        // Let VMMAP_DEBUG checks work on an empty map if uvm_map_teardown() is called
        // twice.
        map.size.set(Vsize::new(0));
        map.min_offset.set(0);
        map.max_offset.set(0);
        map.flags.set(map.flags.get() & !VM_MAP_ISVMSPACE);
    }
    // reinit RB tree since the above removal leaves the tree corrupted.
    map.addr.init();
    vm_map_unlock(map);

    // Remove address selectors.
    uvm_addr_destroy(map.uaddr_exe.take());
    for slot in &map.uaddr_any {
        uvm_addr_destroy(slot.take());
    }
    uvm_addr_destroy(map.uaddr_brk_stack.take());

    uvm_unmap_detach(&dead_entries, 0);
}

/// `uvm_map_setup_entries`: populate map with free-memory entries.
///
/// Map must be initialized and empty.
pub fn uvm_map_setup_entries(map: &VmMap) {
    kdassert!(map.addr.is_empty());

    uvm_map_fix_space(map, None, map.min_offset.get(), map.max_offset.get(), 0);
}

/// `uvm_map_splitentry`: split entry at given address.
///
/// orig: entry that is to be split. next: a newly allocated map entry that is not linked.
/// split: address at which the split is done.
pub fn uvm_map_splitentry(map: &VmMap, orig: &VmMapEntry, next: &VmMapEntry, split: usize) {
    if split & PAGE_MASK != 0 {
        panic(format_args!(
            "uvm_map_splitentry: split address {:#x} not on page boundary!",
            split
        ));
    }
    uvm_tree_sanity(map, "uvm_map_splitentry");
    kassert!(orig.start.get() < split && vmmap_free_end(orig) > split);

    #[cfg(any(test, feature = "vmmap_debug"))]
    {
        kdassert!(map.addr.find(orig).is_some_and(|e| ptr::eq(e, orig)));
        kdassert!(!map.addr.find(next).is_some_and(|e| ptr::eq(e, next)));
    }

    // Free space will change, unlink from free space tree.
    let free = uvm_map_uaddr_e(map, orig);
    uvm_mapent_free_remove(map, free, orig);

    let adj = split - orig.start.get();

    uvm_mapent_copy(orig, next);
    if split >= orig.end.get() {
        next.etype.set(0);
        next.offset.set(0);
        next.wired_count.set(0);
        next.start.set(split);
        next.end.set(split);
        next.guard.set(0);
        next.fspace.set(vmmap_free_end(orig) - split);
        next.aref.ar_amap.set(ptr::null());
        next.aref.ar_pageoff.set(0);
        orig.guard.set(orig.guard.get().min(split - orig.end.get()));
        orig.fspace.set(split - vmmap_free_start(orig));
    } else {
        orig.fspace.set(0);
        orig.guard.set(0);
        orig.end.set(split);
        next.start.set(split);

        if next.aref.amap().is_some() {
            amap_splitref(&orig.aref, &next.aref, adj);
        }
        if uvm_et_issubmap(orig) {
            if let Some(submap) = next.sub_map() {
                uvm_map_reference(submap);
            }
            next.offset.set(next.offset.get() + adj as Voff);
        } else if uvm_et_isobj(orig) {
            if let Some(uobj) = next.uvm_obj()
                && let Some(reference) = uobj.pgops().pgo_reference
            {
                kernel_lock(); // KERNEL_LOCK()
                reference(uobj);
                kernel_unlock(); // KERNEL_UNLOCK()
            }
            next.offset.set(next.offset.get() + adj as Voff);
        }
    }

    // Link next into address tree. Link orig and next into free-space tree.
    //
    // Don't insert 'next' into the addr tree until orig has been linked, in case the
    // free-list looks at adjacent entries in the addr tree for its decisions.
    let free_before = if orig.fspace.get() > 0 {
        free
    } else {
        uvm_map_uaddr_e(map, orig)
    };
    uvm_mapent_free_insert(map, free_before, orig);
    uvm_mapent_addr_insert(map, next);
    uvm_mapent_free_insert(map, free, next);

    uvm_tree_sanity(map, "uvm_map_splitentry");
}

/// `uvm_tree_assert`: panics when `test` is false (`UVM_ASSERT`), naming the map.
#[cfg(any(test, feature = "vmmap_debug"))]
fn uvm_tree_assert(map: &VmMap, test: bool, test_str: &str, who: &str) {
    if test {
        return;
    }

    let map_special = if is_kernel_map(map) {
        " (kernel_map)"
    } else {
        ""
    };
    panic(format_args!(
        "uvm_tree_sanity {:p}{} ({}): {}",
        ptr::from_ref(map),
        map_special,
        who,
        test_str
    ));
}

/// `uvm_tree_sanity`: check that map is sane (`VMMAP_DEBUG`).
#[cfg(any(test, feature = "vmmap_debug"))]
pub fn uvm_tree_sanity(map: &VmMap, who: &str) {
    let mut addr = map.min_offset.get();
    for iter in map.addr.iter() {
        // Valid start, end. Catch overflow for end+fspace.
        uvm_tree_assert(map, iter.end.get() >= iter.start.get(), "end >= start", who);
        uvm_tree_assert(
            map,
            vmmap_free_end(iter) >= iter.end.get(),
            "free_end >= end",
            who,
        );

        // May not be empty.
        uvm_tree_assert(
            map,
            iter.start.get() < vmmap_free_end(iter),
            "start < free_end",
            who,
        );

        // Addresses for entry must lie within map boundaries.
        uvm_tree_assert(
            map,
            iter.start.get() >= map.min_offset.get()
                && vmmap_free_end(iter) <= map.max_offset.get(),
            "entry inside the map",
            who,
        );

        // Tree may not have gaps.
        uvm_tree_assert(map, iter.start.get() == addr, "no gaps", who);
        addr = vmmap_free_end(iter);

        // Free space may not cross boundaries, unless the same free list is used on both
        // sides of the border.
        let mut min = vmmap_free_start(iter);
        let max = vmmap_free_end(iter);

        while min < max {
            let bound = uvm_map_boundary(map, min, max);
            if bound == max {
                break;
            }
            uvm_tree_assert(
                map,
                same_uaddr(uvm_map_uaddr(map, bound - 1), uvm_map_uaddr(map, bound)),
                "free space on one free list",
                who,
            );
            min = bound;
        }

        let free = uvm_map_uaddr_e(map, iter);
        if free.is_some() {
            uvm_tree_assert(
                map,
                iter.etype.get() & UVM_ET_FREEMAPPED != 0,
                "free-mapped entry on a free list",
                who,
            );
        } else {
            uvm_tree_assert(
                map,
                iter.etype.get() & UVM_ET_FREEMAPPED == 0,
                "entry off the free lists",
                who,
            );
        }
    }
    uvm_tree_assert(map, addr == map.max_offset.get(), "tree ends at max", who);
}

/// `uvm_tree_sanity`: nothing without `VMMAP_DEBUG`.
#[cfg(not(any(test, feature = "vmmap_debug")))]
#[inline]
pub fn uvm_tree_sanity(_map: &VmMap, _who: &str) {}

/// `uvm_tree_size_chk`: checks the map's size against its entries (`VMMAP_DEBUG`).
#[cfg(any(test, feature = "vmmap_debug"))]
pub fn uvm_tree_size_chk(map: &VmMap, who: &str) {
    let mut size = 0;
    for iter in map.addr.iter() {
        if !uvm_et_ishole(iter) {
            size += iter.end.get() - iter.start.get();
        }
    }

    if map.size.get().as_usize() != size {
        kprintf!(
            "map size = {:#x}, should be {:#x}\n",
            map.size.get().as_usize(),
            size
        );
    }
    uvm_tree_assert(map, map.size.get().as_usize() == size, "map size", who);

    vmspace_validate(map);
}

/// `uvm_tree_size_chk`: nothing without `VMMAP_DEBUG`.
#[cfg(not(any(test, feature = "vmmap_debug")))]
#[inline]
pub fn uvm_tree_size_chk(_map: &VmMap, _who: &str) {}

/// `vmspace_validate`: this function validates the statistics on vmspace (`VMMAP_DEBUG`).
#[cfg(any(test, feature = "vmmap_debug"))]
pub fn vmspace_validate(map: &VmMap) {
    if map.flags.get() & VM_MAP_ISVMSPACE == 0 {
        return;
    }

    let vm = map.vmspace();
    let stack_begin = vm.vm_maxsaddr.get().min(vm.vm_minsaddr.get());
    let stack_end = vm.vm_maxsaddr.get().max(vm.vm_minsaddr.get());

    let mut stack = 0;
    let mut heap = 0;
    for iter in map.addr.iter() {
        let mut imin = iter.start.get();

        if uvm_et_ishole(iter) || iter.uvm_obj().is_some() || iter.protection.get() == PROT_NONE {
            continue;
        }

        // Update stack, heap. Keep in mind that (theoretically) the entries of userspace
        // and stack may be joined.
        while imin != iter.end.get() {
            // Set imax to the first boundary crossed between imin and stack addresses.
            let mut imax = iter.end.get();
            if imin < stack_begin && imax > stack_begin {
                imax = stack_begin;
            } else if imin < stack_end && imax > stack_end {
                imax = stack_end;
            }

            if imin >= stack_begin && imin < stack_end {
                stack += imax - imin;
            } else {
                heap += imax - imin;
            }
            imin = imax;
        }
    }

    heap >>= PAGE_SHIFT;
    if heap != vm.vm_dused.get() as usize {
        kprintf!("vmspace stack range: {:#x}-{:#x}\n", stack_begin, stack_end);
        panic(format_args!(
            "vmspace_validate: vmspace.vm_dused invalid, expected {} pgs, got {} pgs in map {:p}",
            heap,
            vm.vm_dused.get(),
            ptr::from_ref(map)
        ));
    }
    let _ = stack;
}

/// `vmspace_validate`: nothing without `VMMAP_DEBUG`.
#[cfg(not(any(test, feature = "vmmap_debug")))]
#[inline]
pub fn vmspace_validate(_map: &VmMap) {}

/// `uvm_map_init`: init mapping system at boot time. Note that we allocate and init the
/// static pool of structs `vm_map_entry` for the kernel here.
pub fn uvm_map_init() {
    // now set up static pool of kernel map entries ...
    // mtx_init(&uvm_kmapent_mtx, IPL_VM): statically initialised.
    UVM.kentry_free.init();
    for entry in &KERNEL_MAP_ENTRY {
        // SAFETY: boot, single thread; the static entries are on no list yet.
        unsafe { UVM.kentry_free.insert_head(entry) };
    }

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
    pool_init(
        &UVM_MAP_ENTRY_POOL,
        size_of::<VmMapEntry>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "vmmpepl",
        None,
    );
    pool_init(
        &UVM_MAP_ENTRY_KMEM_POOL,
        size_of::<VmMapEntry>(),
        0,
        IPL_VM,
        0,
        "vmmpekpl",
        None,
    );
    pool_sethiwat(&UVM_MAP_ENTRY_POOL, 8192);

    uvm_addr_init();
}

// DDB hooks (uvm_map_printit, uvm_object_printit, uvm_page_printit): with ddb.

/// `uvm_map_protect`: change map protection.
///
/// - set_max means set max_protection.
/// - map must be unlocked.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_map_protect(
    map: &VmMap,
    start: usize,
    end: usize,
    new_prot: VmProt,
    etype: i32,
    set_max: bool,
    checkimmutable: bool,
) -> Result<(), Errno> {
    kassert!(etype & !UVM_ET_STACK == 0); // only UVM_ET_STACK allowed

    if start > end {
        return Err(Errno::EINVAL);
    }
    let start = start.max(map.min_offset.get());
    let end = end.min(map.max_offset.get());
    if start >= end {
        return Ok(());
    }

    vm_map_lock(map);
    let error = uvm_map_protect_locked(map, start, end, new_prot, etype, set_max, checkimmutable);
    if etype & UVM_ET_STACK != 0 {
        map.sserial.set(map.sserial.get() + 1);
    }
    vm_map_unlock(map);
    error
}

/// [`uvm_map_protect`] with the map locked (the C's body up to the `out` label).
#[allow(clippy::too_many_arguments)] // the C's locals
fn uvm_map_protect_locked(
    map: &VmMap,
    start: usize,
    end: usize,
    new_prot: VmProt,
    etype: i32,
    set_max: bool,
    checkimmutable: bool,
) -> Result<(), Errno> {
    let mut dused = 0;
    let mut error = Ok(());

    // Set up first and last.
    // - first will contain first entry at or after start.
    let first = entry_at(map, start, "uvm_map_protect");
    let first = if first.end.get() <= start {
        RbtHead::<UvmMapAddr>::next(first)
    } else {
        Some(first)
    };

    // First, check for protection violations.
    let mut iter = first;
    while let Some(e) = iter.filter(|e| e.start.get() < end) {
        // Treat memory holes as free space.
        if e.start.get() == e.end.get() || uvm_et_ishole(e) {
            iter = RbtHead::<UvmMapAddr>::next(e);
            continue;
        }

        if checkimmutable && e.etype.get() & UVM_ET_IMMUTABLE != 0 {
            return Err(Errno::EPERM);
        }
        let old_prot = e.protection.get();
        if old_prot == PROT_NONE && new_prot != old_prot {
            dused += uvmspace_dused(map, start.max(e.start.get()), end.min(e.end.get()));
        }

        if uvm_et_issubmap(e) {
            return Err(Errno::EINVAL);
        }
        if new_prot & e.max_protection.get() != new_prot {
            return Err(Errno::EACCES);
        }
        if is_kernel_map(map) && new_prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC) {
            panic(format_args!(
                "uvm_map_protect: kernel map W^X violation requested"
            ));
        }
        iter = RbtHead::<UvmMapAddr>::next(e);
    }

    // Check limits.
    if dused > 0 && map.flags.get() & VM_MAP_ISVMSPACE != 0 {
        // vsize_t limit = lim_cur(RLIMIT_DATA); ENOMEM when ptoa(dused) exceeds it with
        // the current vm_dused: the limits are not ported (see the module's deviations).
        let _ = unported!("uvm_map_protect: the RLIMIT_DATA check (lim_cur)");
    }

    // only apply UVM_ET_STACK on a mapping changing to RW
    let etype = if etype != 0 && new_prot != (PROT_READ | PROT_WRITE) {
        0
    } else {
        etype
    };

    // Fix protections.
    let mut iter = first;
    while let Some(e) = iter.filter(|e| e.start.get() < end) {
        // Treat memory holes as free space.
        if e.start.get() == e.end.get() || uvm_et_ishole(e) {
            iter = RbtHead::<UvmMapAddr>::next(e);
            continue;
        }

        let old_prot = e.protection.get();

        // Skip adapting protection iff old and new protection are equal.
        if set_max {
            if old_prot == new_prot & old_prot && e.max_protection.get() == new_prot {
                iter = RbtHead::<UvmMapAddr>::next(e);
                continue;
            }
        } else if old_prot == new_prot {
            iter = RbtHead::<UvmMapAddr>::next(e);
            continue;
        }

        uvm_map_clip_start_at(map, e, start);
        uvm_map_clip_end_at(map, e, end);

        if set_max {
            e.max_protection.set(new_prot);
            e.protection.set(e.protection.get() & new_prot);
        } else {
            e.protection.set(new_prot);
        }
        e.etype.set(e.etype.get() | etype); // potentially add UVM_ET_STACK

        // update physical map if necessary. worry about copy-on-write here -- CHECK THIS XXX
        if e.protection.get() != old_prot {
            let mask = if uvm_et_iscopyonwrite(e) {
                !PROT_WRITE
            } else {
                PROT_MASK
            };

            if map.flags.get() & VM_MAP_ISVMSPACE != 0 {
                let vm = map.vmspace();
                if old_prot == PROT_NONE {
                    vm.vm_dused.set(
                        vm.vm_dused.get() + uvmspace_dused(map, e.start.get(), e.end.get()) as i32,
                    );
                }
                if e.protection.get() == PROT_NONE {
                    vm.vm_dused.set(
                        vm.vm_dused.get() - uvmspace_dused(map, e.start.get(), e.end.get()) as i32,
                    );
                }
            }

            // update pmap
            if e.protection.get() & mask == PROT_NONE && vm_mapent_iswired(e) {
                // TODO(ariane) this is stupid. wired_count is 0 if not wired, otherwise
                // anything larger than 0 (incremented once each time wire is called).
                // Mostly to be able to undo the damage on failure. Not the actually be a
                // wired refcounter...
                // Originally: iter->wired_count--;
                // (don't we have to unwire this in the pmap as well?)
                e.wired_count.set(0);
            }
            uvm_map_lock_entry(e);
            pmap_protect(
                map.pmap(),
                Vaddr::new(e.start.get()),
                Vaddr::new(e.end.get()),
                e.protection.get() & mask,
            );
            uvm_map_unlock_entry(e);
        }

        // If the map is configured to lock any future mappings, wire this entry now if the
        // old protection was PROT_NONE and the new protection is not PROT_NONE.
        if map.flags.get() & VM_MAP_WIREFUTURE != 0
            && !vm_mapent_iswired(e)
            && old_prot == PROT_NONE
            && new_prot != PROT_NONE
            && uvm_map_pageable(
                map,
                e.start.get(),
                e.end.get(),
                false,
                UVM_LK_ENTER | UVM_LK_EXIT,
            )
            .is_err()
        {
            // If locking the entry fails, remember the error if it's the first one. Note
            // we still continue setting the protection in the map, but it will return the
            // resource storage condition regardless.
            //
            // XXX Ignore what the actual error is, XXX just call it a resource shortage XXX
            // so that it doesn't get confused XXX what uvm_map_protect() itself would XXX
            // normally return.
            error = Err(Errno::ENOMEM);
        }
        iter = RbtHead::<UvmMapAddr>::next(e);
    }
    pmap_update(map.pmap());

    error
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

    vm.vm_refcnt.store(1, AtomicOrdering::Relaxed);

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
    let dead_entries = UvmMapDeadq::new();

    // see if more than one process is using this vmspace...
    if ovm.vm_refcnt.load(AtomicOrdering::Relaxed) == 1 {
        // If pr is the only process using its vmspace then we can safely recycle that
        // vmspace for the program that is being exec'd.

        // SYSVSHM is not configured: no segments to kill.

        // POSIX 1003.1b -- "lock future mappings" is revoked when a process execs another
        // program image.
        vm_map_lock(map);
        vm_map_modflags(map, 0, VM_MAP_WIREFUTURE | VM_MAP_PINSYSCALL_ONCE);

        // now unmap the old program
        //
        // Instead of attempting to keep the map valid, we simply nuke all entries and ask
        // uvm_map_setup to reinitialize the map to the new boundaries.
        //
        // uvm_unmap_remove will actually nuke all entries for us (as in, not replace them
        // with free-memory entries).
        let _ = uvm_unmap_remove(
            map,
            map.min_offset.get(),
            map.max_offset.get(),
            &dead_entries,
            true,
            false,
            false,
        );

        kdassert!(map.addr.is_empty());

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
        uvm_map_setup_entries(map);
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
    // PMAP_CHECK_COPYIN is not configured.

    // Release dead entries
    uvm_unmap_detach(&dead_entries, 0);
}

/// `uvmspace_addref`: add a reference to a vmspace.
pub fn uvmspace_addref(vm: &Vmspace) {
    kassert!(vm.vm_refcnt.load(AtomicOrdering::Relaxed) > 0);
    vm.vm_refcnt.fetch_add(1, AtomicOrdering::Relaxed);
}

/// `uvmspace_purge`: tears the address space down: locks the map, to wait out all other
/// references to it, and deletes all of the mappings and pages they hold.
pub fn uvmspace_purge(vm: &Vmspace) {
    // SYSVSHM is not configured: no shared memory segments to get rid of.
    uvm_map_teardown(&vm.vm_map);
}

/// `uvmspace_free`: free a vmspace data structure.
pub fn uvmspace_free(vm: &'static Vmspace) {
    if vm.vm_refcnt.fetch_sub(1, AtomicOrdering::AcqRel) - 1 == 0 {
        // Sanity check. Kernel threads never end up here and userland ones already tear
        // down there VM space in exit1().
        uvmspace_purge(vm);

        pmap_destroy(vm.vm_map.pmap());
        vm.vm_map.pmap.set(ptr::null());

        pool_put(&UVM_VMSPACE_POOL, NonNull::from(vm).cast::<u8>());
    }
}

/// `uvm_mapent_clone`: clone map entry into other map.
///
/// Mapping will be placed at dstaddr, for the same length. Space must be available.
/// Reference counters are incremented.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_mapent_clone<'m>(
    dstmap: &'m VmMap,
    dstaddr: usize,
    dstlen: usize,
    off: usize,
    prot: VmProt,
    maxprot: VmProt,
    old_entry: &VmMapEntry,
    dead: &UvmMapDeadq,
    mapent_flags: u32,
    amap_share_flags: i32,
) -> Option<&'m VmMapEntry> {
    kdassert!(!uvm_et_issubmap(old_entry));

    // Create new entry (linked in on creation). Fill in first, last.
    let mut first = None;
    let mut last = None;
    if !uvm_map_isavail(dstmap, None, &mut first, &mut last, dstaddr, dstlen) {
        panic(format_args!(
            "uvm_mapent_clone: no space in map for entry in empty map"
        ));
    }
    let (Some(first), Some(last)) = (first, last) else {
        return None;
    };
    let new_entry = uvm_map_mkentry(
        dstmap,
        first,
        last,
        dstaddr,
        dstlen,
        mapent_flags,
        dead,
        None,
    )?;
    // old_entry -> new_entry
    new_entry.object.set(old_entry.object.get());
    new_entry.offset.set(old_entry.offset.get());
    new_entry
        .aref
        .ar_pageoff
        .set(old_entry.aref.ar_pageoff.get());
    new_entry.aref.ar_amap.set(old_entry.aref.ar_amap.get());
    new_entry
        .etype
        .set(new_entry.etype.get() | (old_entry.etype.get() & !UVM_ET_FREEMAPPED));
    new_entry.protection.set(prot);
    new_entry.max_protection.set(maxprot);
    new_entry.inheritance.set(old_entry.inheritance.get());
    new_entry.advice.set(old_entry.advice.get());

    // gain reference to object backing the map (can't be a submap).
    if let Some(amap) = new_entry.aref.amap() {
        new_entry
            .aref
            .ar_pageoff
            .set(new_entry.aref.ar_pageoff.get() + (off >> PAGE_SHIFT) as i32);
        amap_ref(
            amap,
            new_entry.aref.ar_pageoff.get() as usize,
            (new_entry.end.get() - new_entry.start.get()) >> PAGE_SHIFT,
            amap_share_flags,
        );
    }

    if let Some(uobj) = new_entry.uvm_obj().filter(|_| uvm_et_isobj(new_entry))
        && let Some(reference) = uobj.pgops().pgo_reference
    {
        new_entry.offset.set(new_entry.offset.get() + off as Voff);
        reference(uobj);
    }

    Some(new_entry)
}

/// `uvm_mapent_share`: clones `old_entry` into `dstmap` sharing its amap.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_mapent_share<'m>(
    dstmap: &'m VmMap,
    dstaddr: usize,
    dstlen: usize,
    off: usize,
    prot: VmProt,
    maxprot: VmProt,
    old_map: &VmMap,
    old_entry: &VmMapEntry,
    dead: &UvmMapDeadq,
) -> Option<&'m VmMapEntry> {
    // If old_entry refers to a copy-on-write region that has not yet been written to
    // (needs_copy flag is set), then we need to allocate a new amap for old_entry.
    //
    // If we do not do this, and the process owning old_entry does a copy-on write later,
    // old_entry and new_entry will refer to different memory regions, and the memory
    // between the processes is no longer shared.
    //
    // [in other words, we need to clear needs_copy]
    if uvm_et_isneedscopy(old_entry) {
        // get our own amap, clears needs_copy
        amap_copy(old_map, old_entry, M_WAITOK, false, 0, 0);
        // XXXCDC: WAITOK???
    }

    uvm_mapent_clone(
        dstmap,
        dstaddr,
        dstlen,
        off,
        prot,
        maxprot,
        old_entry,
        dead,
        0,
        AMAP_SHARED,
    )
}

/// `uvm_mapent_forkshared`: share the mapping: this means we want the old and new entries
/// to share amaps and backing objects.
pub fn uvm_mapent_forkshared<'m>(
    _new_vm: &Vmspace,
    new_map: &'m VmMap,
    old_map: &VmMap,
    old_entry: &VmMapEntry,
    dead: &UvmMapDeadq,
) -> Option<&'m VmMapEntry> {
    uvm_mapent_share(
        new_map,
        old_entry.start.get(),
        old_entry.end.get() - old_entry.start.get(),
        0,
        old_entry.protection.get(),
        old_entry.max_protection.get(),
        old_map,
        old_entry,
        dead,
    )
}

/// `uvm_mapent_forkcopy`: copy-on-write the mapping (using mmap's `MAP_PRIVATE`
/// semantics).
///
/// allocate new_entry, adjust reference counts. (note that new references are read-only).
pub fn uvm_mapent_forkcopy<'m>(
    _new_vm: &Vmspace,
    new_map: &'m VmMap,
    old_map: &VmMap,
    old_entry: &VmMapEntry,
    dead: &UvmMapDeadq,
) -> Option<&'m VmMapEntry> {
    let new_entry = uvm_mapent_clone(
        new_map,
        old_entry.start.get(),
        old_entry.end.get() - old_entry.start.get(),
        0,
        old_entry.protection.get(),
        old_entry.max_protection.get(),
        old_entry,
        dead,
        0,
        0,
    )?;

    new_entry
        .etype
        .set(new_entry.etype.get() | (UVM_ET_COPYONWRITE | UVM_ET_NEEDSCOPY));

    // the new entry will need an amap. it will either need to be copied from the old entry
    // or created from scratch (if the old entry does not have an amap). can we defer this
    // process until later (by setting "needs_copy") or do we need to copy the amap now?
    //
    // we must copy the amap now if any of the following conditions hold:
    // 1. the old entry has an amap and that amap is being shared. this means that the old
    //    (parent) process is sharing the amap with another process. if we do not clear
    //    needs_copy here we will end up in a situation where both the parent and child
    //    process are referring to the same amap with "needs_copy" set. if the parent
    //    write-faults, the fault routine will clear "needs_copy" in the parent by allocating
    //    a new amap. this is wrong because the parent is supposed to be sharing the old amap
    //    and the new amap will break that.
    //
    // 2. if the old entry has an amap and a non-zero wire count then we are going to have
    //    to call amap_cow_now to avoid page faults in the parent process. since
    //    amap_cow_now requires "needs_copy" to be clear we might as well clear it here as
    //    well.
    if let Some(old_amap) = old_entry.aref.amap()
        && (amap_flags(old_amap) & AMAP_SHARED != 0 || vm_mapent_iswired(old_entry))
    {
        amap_copy(new_map, new_entry, M_WAITOK, false, 0, 0);
        // XXXCDC: M_WAITOK ... ok?
    }

    // if the parent's entry is wired down, then the parent process does not want page
    // faults on access to that memory. this means that we cannot do copy-on-write because
    // we can't write protect the old entry. in this case we resolve all copy-on-write
    // faults now, using amap_cow_now. note that we have already allocated any needed amap
    // (above).
    if vm_mapent_iswired(old_entry) {
        // resolve all copy-on-write faults now (note that there is nothing to do if the
        // old mapping does not have an amap).
        if old_entry.aref.amap().is_some() {
            amap_cow_now(new_map, new_entry);
        }
    } else {
        let protect_child = if old_entry.aref.amap().is_some() {
            // setup mappings to trigger copy-on-write faults we must write-protect the
            // parent if it has an amap and it is not already "needs_copy"... if it is
            // already "needs_copy" then the parent has already been write-protected by a
            // previous fork operation.
            //
            // if we do not write-protect the parent, then we must be sure to write-protect
            // the child.
            if !uvm_et_isneedscopy(old_entry) {
                if old_entry.max_protection.get() & PROT_WRITE != 0 {
                    uvm_map_lock_entry(old_entry);
                    pmap_protect(
                        old_map.pmap(),
                        Vaddr::new(old_entry.start.get()),
                        Vaddr::new(old_entry.end.get()),
                        old_entry.protection.get() & !PROT_WRITE,
                    );
                    uvm_map_unlock_entry(old_entry);
                    pmap_update(old_map.pmap());
                }
                old_entry
                    .etype
                    .set(old_entry.etype.get() | UVM_ET_NEEDSCOPY);
            }

            // parent must now be write-protected
            false
        } else {
            // we only need to protect the child if the parent has write access.
            old_entry.max_protection.get() & PROT_WRITE != 0
        };

        // protect the child's mappings if necessary
        if protect_child {
            pmap_protect(
                new_map.pmap(),
                Vaddr::new(new_entry.start.get()),
                Vaddr::new(new_entry.end.get()),
                new_entry.protection.get() & !PROT_WRITE,
            );
        }
    }

    Some(new_entry)
}

/// `uvm_mapent_forkzero`: zero the mapping: the new entry will be zero initialized.
pub fn uvm_mapent_forkzero<'m>(
    _new_vm: &Vmspace,
    new_map: &'m VmMap,
    _old_map: &VmMap,
    old_entry: &VmMapEntry,
    dead: &UvmMapDeadq,
) -> Option<&'m VmMapEntry> {
    let new_entry = uvm_mapent_clone(
        new_map,
        old_entry.start.get(),
        old_entry.end.get() - old_entry.start.get(),
        0,
        old_entry.protection.get(),
        old_entry.max_protection.get(),
        old_entry,
        dead,
        0,
        0,
    )?;

    new_entry
        .etype
        .set(new_entry.etype.get() | (UVM_ET_COPYONWRITE | UVM_ET_NEEDSCOPY));

    if let Some(amap) = new_entry.aref.amap() {
        amap_unref(
            amap,
            new_entry.aref.ar_pageoff.get() as usize,
            atop(new_entry.end.get() - new_entry.start.get()),
            false,
        );
        new_entry.aref.ar_amap.set(ptr::null());
        new_entry.aref.ar_pageoff.set(0);
    }

    if uvm_et_isobj(new_entry) {
        if let Some(uobj) = new_entry.uvm_obj()
            && let Some(detach) = uobj.pgops().pgo_detach
        {
            detach(uobj);
        }
        new_entry.object.set(VmMapEntryObject::None);
        new_entry.etype.set(new_entry.etype.get() & !UVM_ET_OBJ);
    }

    Some(new_entry)
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

    // go entry-by-entry
    let dead = UvmMapDeadq::new();
    for old_entry in old_map.addr.iter() {
        if old_entry.start.get() == old_entry.end.get() {
            continue;
        }

        // first, some sanity checks on the old entry
        if uvm_et_issubmap(old_entry) {
            panic(format_args!(
                "fork: encountered a submap during fork (illegal)"
            ));
        }

        if !uvm_et_iscopyonwrite(old_entry) && uvm_et_isneedscopy(old_entry) {
            panic(format_args!(
                "fork: non-copy_on_write map entry marked needs_copy (illegal)"
            ));
        }

        // Apply inheritance.
        let new_entry = match old_entry.inheritance.get() {
            MAP_INHERIT_SHARE => uvm_mapent_forkshared(vm2, new_map, old_map, old_entry, &dead),
            MAP_INHERIT_COPY => uvm_mapent_forkcopy(vm2, new_map, old_map, old_entry, &dead),
            MAP_INHERIT_ZERO => uvm_mapent_forkzero(vm2, new_map, old_map, old_entry, &dead),
            _ => continue,
        };
        let Some(new_entry) = new_entry else {
            continue;
        };

        // Update process statistics.
        if !uvm_et_ishole(new_entry) {
            new_map.size.set(Vsize::new(
                new_map.size.get().as_usize() + (new_entry.end.get() - new_entry.start.get()),
            ));
        }
        if !uvm_et_isobj(new_entry)
            && !uvm_et_ishole(new_entry)
            && new_entry.protection.get() != PROT_NONE
        {
            vm2.vm_dused.set(
                vm2.vm_dused.get()
                    + uvmspace_dused(new_map, new_entry.start.get(), new_entry.end.get()) as i32,
            );
        }
    }
    new_map
        .flags
        .set(new_map.flags.get() | (old_map.flags.get() & VM_MAP_PINSYSCALL_ONCE));
    // PMAP_CHECK_COPYIN is not configured.

    vm_map_unlock(old_map);
    vm_map_unlock(new_map);

    // This can actually happen, if multiple entries described a space in which an entry
    // was inherited.
    uvm_unmap_detach(&dead, 0);

    // SYSVSHM is not configured: no shmfork.

    vm2
}

/// `uvm_map_hint`: return the beginning of the best area suitable for creating a new
/// mapping with "prot" protection.
pub fn uvm_map_hint(vm: &Vmspace, _prot: VmProt, minaddr: usize, maxaddr: usize) -> usize {
    // __i386__: no.

    // __LP64__.
    let mut spacing = (4usize * 1024 * 1024 * 1024).min(<Machine as VmParam>::MAXDSIZ) - 1;

    // Start malloc/mmap after the brk.
    let mut addr = vm.vm_daddr.get() + <Machine as VmParam>::BRKSIZ;
    addr = addr.max(minaddr);

    if addr < maxaddr {
        while spacing > maxaddr - addr {
            spacing >>= 1;
        }
    }
    addr += arc4random() as usize & spacing;
    round_page(addr)
}

/// `uvm_map_submap`: punch down part of a map into a submap.
///
/// - only the kernel_map is allowed to be submapped
/// - the purpose of submapping is to break up the locking granularity of a larger map
/// - the range specified must have been mapped previously with a `uvm_map()` call [with
///   uobj==NULL] to create a blank map entry in the main map. [And it had better still be
///   blank!]
/// - maps which contain submaps should never be copied or forked.
/// - to remove a submap, use `uvm_unmap()` on the main map and then
///   `uvm_map_deallocate()` the submap.
/// - main map must be unlocked.
/// - submap must have been init'd and have a zero reference count. [need not be locked as
///   we don't actually reference it]
pub fn uvm_map_submap(
    map: &VmMap,
    start: usize,
    end: usize,
    submap: &'static VmMap,
) -> Result<(), Errno> {
    if start > map.max_offset.get()
        || end > map.max_offset.get()
        || start < map.min_offset.get()
        || end < map.min_offset.get()
    {
        return Err(Errno::EINVAL);
    }

    vm_map_lock(map);

    let entry = uvm_map_lookup_entry(map, start).inspect(|entry| {
        uvm_map_clip_start_at(map, entry, start);
        uvm_map_clip_end_at(map, entry, end);
    });

    let result = match entry {
        Some(entry)
            if entry.start.get() == start
                && entry.end.get() == end
                && entry.uvm_obj().is_none()
                && entry.aref.amap().is_none()
                && !uvm_et_iscopyonwrite(entry)
                && !uvm_et_isneedscopy(entry) =>
        {
            entry.etype.set(entry.etype.get() | UVM_ET_SUBMAP);
            entry.object.set(VmMapEntryObject::SubMap(submap));
            entry.offset.set(0);
            uvm_map_reference(submap);
            Ok(())
        }
        _ => Err(Errno::EINVAL),
    };

    vm_map_unlock(map);
    result
}

/// `uvm_map_checkprot`: check protection in map.
///
/// - must allow specific protection in a fully allocated region.
/// - map must be read or write locked by caller.
pub fn uvm_map_checkprot(map: &VmMap, start: usize, end: usize, protection: VmProt) -> bool {
    vm_map_assert_anylock(map);

    if start < map.min_offset.get() || end > map.max_offset.get() || start > end {
        return false;
    }
    if start == end {
        return true;
    }

    // Iterate entries.
    let mut iter = uvm_map_entrybyaddr(&map.addr, start);
    while let Some(entry) = iter.filter(|e| e.start.get() < end) {
        // Fail if a hole is found.
        if uvm_et_ishole(entry)
            || (entry.end.get() < end && entry.end.get() != vmmap_free_end(entry))
        {
            return false;
        }

        // Check protection.
        if entry.protection.get() & protection != protection {
            return false;
        }
        iter = RbtHead::<UvmMapAddr>::next(entry);
    }
    true
}

/// `uvm_map_create`: create map.
pub fn uvm_map_create(
    pmap: &'static MachinePmap,
    min: usize,
    max: usize,
    flags: i32,
) -> Option<&'static VmMap> {
    let mem = malloc(size_of::<VmMap>(), M_VMMAP, M_WAITOK)?;
    let p = mem.cast::<VmMap>();
    // SAFETY: a fresh, suitably aligned allocation of `size_of::<VmMap>()` bytes, written
    // once before anything else sees it; it lives until `uvm_map_deallocate` frees it.
    let map: &'static VmMap = unsafe {
        p.as_ptr().write(VmMap::new());
        p.as_ref()
    };
    uvm_map_setup(map, pmap, min, max, flags);
    Some(map)
}

/// `uvm_map_deallocate`: drop reference to a map.
///
/// - caller must not lock map
/// - we will zap map if ref count goes to zero
pub fn uvm_map_deallocate(map: &'static VmMap) {
    let c = map.ref_count.fetch_sub(1, AtomicOrdering::AcqRel) - 1;
    if c > 0 {
        return;
    }

    // all references gone. unmap and free.
    //
    // No lock required: we are only one to access this map.
    let dead = UvmMapDeadq::new();
    uvm_tree_sanity(map, "uvm_map_deallocate");
    vm_map_lock(map);
    let _ = uvm_unmap_remove(
        map,
        map.min_offset.get(),
        map.max_offset.get(),
        &dead,
        true,
        false,
        false,
    );
    #[cfg(any(test, feature = "vmmap_debug"))]
    {
        // As uvm_map_teardown: let the VMMAP_DEBUG checks of the unlock pass on the emptied
        // map (the C's uvm_tree_sanity would trip on it here).
        map.size.set(Vsize::new(0));
        map.min_offset.set(0);
        map.max_offset.set(0);
    }
    vm_map_unlock(map);
    pmap_destroy(map.pmap());
    kassert!(map.addr.is_empty());
    free(NonNull::from(map).cast::<u8>(), M_VMMAP, size_of::<VmMap>());

    uvm_unmap_detach(&dead, 0);
}

/// `uvm_map_inherit`: set inheritance code for range of addrs in map.
///
/// - map must be unlocked
/// - note that the inherit code is used during a "fork". see fork code for details.
pub fn uvm_map_inherit(
    map: &VmMap,
    start: usize,
    end: usize,
    new_inheritance: VmInherit,
) -> Result<(), Errno> {
    match new_inheritance {
        MAP_INHERIT_NONE | MAP_INHERIT_COPY | MAP_INHERIT_SHARE | MAP_INHERIT_ZERO => {}
        _ => return Err(Errno::EINVAL),
    }

    if start > end {
        return Err(Errno::EINVAL);
    }
    let start = start.max(map.min_offset.get());
    let end = end.min(map.max_offset.get());
    if start >= end {
        return Ok(());
    }

    vm_map_lock(map);

    let entry = entry_at(map, start, "uvm_map_inherit");
    let entry = if entry.end.get() > start {
        uvm_map_clip_start_at(map, entry, start);
        Some(entry)
    } else {
        RbtHead::<UvmMapAddr>::next(entry)
    };

    // First check for illegal operations
    let mut entry1 = entry;
    let mut error = Ok(());
    while let Some(e1) = entry1.filter(|e| e.start.get() < end) {
        if e1.etype.get() & UVM_ET_IMMUTABLE != 0 {
            error = Err(Errno::EPERM);
            break;
        }
        if new_inheritance == MAP_INHERIT_ZERO && e1.protection.get() & PROT_WRITE == 0 {
            error = Err(Errno::EPERM);
            break;
        }
        entry1 = RbtHead::<UvmMapAddr>::next(e1);
    }

    if error.is_ok() {
        let mut iter = entry;
        while let Some(e) = iter.filter(|e| e.start.get() < end) {
            uvm_map_clip_end_at(map, e, end);
            e.inheritance.set(new_inheritance);
            iter = RbtHead::<UvmMapAddr>::next(e);
        }
    }

    vm_map_unlock(map);
    error
}

// uvm_map_check_copyin_add: PMAP_CHECK_COPYIN is not configured.

/// `uvm_map_immutable`: block mapping/mprotect for range of addrs in map.
///
/// - map must be unlocked
pub fn uvm_map_immutable(map: &VmMap, start: usize, end: usize, imut: bool) -> Result<(), Errno> {
    if start > end {
        return Err(Errno::EINVAL);
    }
    let start = start.max(map.min_offset.get());
    let end = end.min(map.max_offset.get());
    if start >= end {
        return Ok(());
    }

    vm_map_lock(map);

    let entry = entry_at(map, start, "uvm_map_immutable");
    let entry = if entry.end.get() > start {
        uvm_map_clip_start_at(map, entry, start);
        Some(entry)
    } else {
        RbtHead::<UvmMapAddr>::next(entry)
    };

    // First check for illegal operations
    let mut entry1 = entry;
    let mut error = Ok(());
    while let Some(e1) = entry1.filter(|e| e.start.get() < end) {
        if e1.inheritance.get() == MAP_INHERIT_ZERO {
            error = Err(Errno::EPERM);
            break;
        }
        entry1 = RbtHead::<UvmMapAddr>::next(e1);
    }

    if error.is_ok() {
        let mut iter = entry;
        while let Some(e) = iter.filter(|e| e.start.get() < end) {
            uvm_map_clip_end_at(map, e, end);
            if imut {
                e.etype.set(e.etype.get() | UVM_ET_IMMUTABLE);
            } else {
                e.etype.set(e.etype.get() & !UVM_ET_IMMUTABLE);
            }
            iter = RbtHead::<UvmMapAddr>::next(e);
        }
    }

    vm_map_unlock(map);
    error
}

/// `uvm_map_advice`: set advice code for range of addrs in map.
///
/// - map must be unlocked
pub fn uvm_map_advice(map: &VmMap, start: usize, end: usize, new_advice: i32) -> Result<(), Errno> {
    match new_advice {
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL => {}
        _ => return Err(Errno::EINVAL),
    }

    if start > end {
        return Err(Errno::EINVAL);
    }
    let start = start.max(map.min_offset.get());
    let end = end.min(map.max_offset.get());
    if start >= end {
        return Ok(());
    }

    vm_map_lock(map);

    let mut iter = match uvm_map_entrybyaddr(&map.addr, start) {
        Some(entry) if entry.end.get() > start => {
            uvm_map_clip_start_at(map, entry, start);
            Some(entry)
        }
        Some(entry) => RbtHead::<UvmMapAddr>::next(entry),
        None => None,
    };

    // XXXJRT: disallow holes?
    while let Some(e) = iter.filter(|e| e.start.get() < end) {
        uvm_map_clip_end_at(map, e, end);
        e.advice.set(new_advice);
        iter = RbtHead::<UvmMapAddr>::next(e);
    }

    vm_map_unlock(map);
    Ok(())
}

/// `uvm_map_extract`: extract a mapping from a map and put it somewhere in the kernel_map,
/// setting protection to max_prot. Yields the destination address.
///
/// - map should be unlocked (we will write lock it and kernel_map)
/// - start must be page aligned
/// - len must be page sized
/// - flags: `UVM_EXTRACT_FIXPROT`: set prot to maxprot as we go
///
/// Mappings are QREF's.
pub fn uvm_map_extract(
    srcmap: &VmMap,
    start: usize,
    len: usize,
    flags: u32,
) -> Result<usize, Errno> {
    let dead = UvmMapDeadq::new();
    let end = start.wrapping_add(len);

    // Sanity check on the parameters. Also, since the mapping may not contain gaps, error
    // out if the mapped area is not in source map.
    if start & PAGE_MASK != 0 || end & PAGE_MASK != 0 || end < start {
        return Err(Errno::EINVAL);
    }
    if start < srcmap.min_offset.get() || end > srcmap.max_offset.get() {
        return Err(Errno::EINVAL);
    }

    // Initialize dead entries. Handle len == 0 case.
    if len == 0 {
        return Ok(0);
    }

    // Acquire lock on srcmap.
    vm_map_lock(srcmap);

    let result = uvm_map_extract_locked(srcmap, start, len, flags, &dead);

    vm_map_unlock(srcmap);

    uvm_unmap_detach(&dead, 0);

    result
}

/// [`uvm_map_extract`] with the source map locked (the C's body up to the `fail` label).
fn uvm_map_extract_locked(
    srcmap: &VmMap,
    start: usize,
    len: usize,
    flags: u32,
    dead: &UvmMapDeadq,
) -> Result<usize, Errno> {
    let end = start + len;

    // Lock srcmap, lookup first and last entry in <start,len>.
    let first = uvm_map_entrybyaddr(&srcmap.addr, start);

    // Check that the range is contiguous.
    let mut entry = first;
    while let Some(e) = entry.filter(|e| e.end.get() < end) {
        if vmmap_free_end(e) != e.end.get() || uvm_et_ishole(e) {
            return Err(Errno::EINVAL);
        }
        entry = RbtHead::<UvmMapAddr>::next(e);
    }
    match entry {
        None => return Err(Errno::EINVAL),
        Some(e) if uvm_et_ishole(e) => return Err(Errno::EINVAL),
        _ => {}
    }

    // Handle need-copy flag.
    let mut entry = first;
    while let Some(e) = entry.filter(|e| e.start.get() < end) {
        if uvm_et_isneedscopy(e) {
            amap_copy(srcmap, e, M_NOWAIT, !uvm_et_isstack(e), start, end);
        }
        if uvm_et_isneedscopy(e) {
            // amap_copy failure
            return Err(Errno::ENOMEM);
        }
        entry = RbtHead::<UvmMapAddr>::next(e);
    }

    // Lock destination map (kernel_map).
    let kernel_map = kernel_map();
    vm_map_lock(kernel_map);

    let result = uvm_map_extract_into_kernel(srcmap, kernel_map, first, start, len, flags, dead);

    vm_map_unlock(kernel_map);
    result
}

/// [`uvm_map_extract`] with both maps locked (the C's steps after `vm_map_lock(kernel_map)`
/// up to the `fail2` label).
fn uvm_map_extract_into_kernel(
    _srcmap: &VmMap,
    kernel_map: &VmMap,
    first: Option<&VmMapEntry>,
    start: usize,
    len: usize,
    flags: u32,
    dead: &UvmMapDeadq,
) -> Result<usize, Errno> {
    let end = start + len;

    let Ok((_, _, dstaddr)) = uvm_map_findspace(
        kernel_map,
        len,
        PAGE_SIZE.max(pmap_prefer_align()),
        pmap_prefer_offset(start as Voff),
        PROT_NONE,
        0,
    ) else {
        return Err(Errno::ENOMEM);
    };

    // We now have srcmap and kernel_map locked. dstaddr contains the destination offset in
    // dstmap.
    // step 1: start looping through map entries, performing extraction.
    let mut error = Ok(dstaddr);
    let mut entry = first;
    while let Some(e) = entry.filter(|e| e.start.get() < end) {
        kdassert!(!uvm_et_isneedscopy(e));
        if uvm_et_ishole(e) {
            entry = RbtHead::<UvmMapAddr>::next(e);
            continue;
        }

        // Calculate uvm_mapent_clone parameters.
        let mut cp_start = e.start.get();
        let cp_off;
        if cp_start < start {
            cp_off = start - cp_start;
            cp_start = start;
        } else {
            cp_off = 0;
        }
        let cp_len = e.end.get().min(end) - cp_start;

        let Some(newentry) = uvm_mapent_clone(
            kernel_map,
            cp_start - start + dstaddr,
            cp_len,
            cp_off,
            e.protection.get(),
            e.max_protection.get(),
            e,
            dead,
            flags,
            AMAP_SHARED | AMAP_REFALL,
        ) else {
            error = Err(Errno::ENOMEM);
            break;
        };
        kernel_map
            .size
            .set(Vsize::new(kernel_map.size.get().as_usize() + cp_len));

        // Figure out the best protection
        if flags & UVM_EXTRACT_FIXPROT != 0 && newentry.protection.get() != PROT_NONE {
            newentry.protection.set(newentry.max_protection.get());
        }
        newentry
            .protection
            .set(newentry.protection.get() & !PROT_EXEC);
        entry = RbtHead::<UvmMapAddr>::next(e);
    }
    pmap_update(kernel_map.pmap());

    // Unmap copied entries on failure.
    if error.is_err() {
        let _ = uvm_unmap_remove(kernel_map, dstaddr, dstaddr + len, dead, false, true, false);
    }

    error
}

/// `uvm_map_clean`: clean out a map range.
///
/// - valid flags: if `PGO_CLEANIT`: dirty pages are cleaned first; if `PGO_SYNCIO`: dirty
///   pages are written synchronously; if `PGO_DEACTIVATE`: any cached pages are deactivated
///   after clean; if `PGO_FREE`: any cached pages are freed after clean.
/// - returns an error if any part of the specified range isn't mapped
/// - never a need to flush amap layer since the anonymous memory has no permanent home,
///   but may deactivate pages there
/// - called from `sys_msync()` and `sys_madvise()`
/// - caller must not have map locked
pub fn uvm_map_clean(map: &VmMap, start: usize, end: usize, flags: i32) -> Result<(), Errno> {
    kassert!(flags & (PGO_FREE | PGO_DEACTIVATE) != (PGO_FREE | PGO_DEACTIVATE));

    if start > end || start < map.min_offset.get() || end > map.max_offset.get() {
        return Err(Errno::EINVAL);
    }

    vm_map_lock(map);
    let first = uvm_map_entrybyaddr(&map.addr, start);

    // Make a first pass to check for various conditions.
    let mut imut = false;
    let mut entry = first;
    while let Some(e) = entry.filter(|e| e.start.get() < end) {
        if e.etype.get() & UVM_ET_IMMUTABLE != 0 {
            imut = true;
        }
        if uvm_et_issubmap(e) {
            vm_map_unlock(map);
            return Err(Errno::EINVAL);
        }
        if uvm_et_issubmap(e)
            || uvm_et_ishole(e)
            || (e.end.get() < end && vmmap_free_end(e) != e.end.get())
        {
            vm_map_unlock(map);
            return Err(Errno::EFAULT);
        }
        entry = RbtHead::<UvmMapAddr>::next(e);
    }

    vm_map_busy(map);
    vm_map_unlock(map);
    let mut error = Ok(());
    let mut entry = first;
    while let Some(e) = entry.filter(|e| e.start.get() < end) {
        let amap = e.aref.amap(); // top layer
        let uobj = e.uvm_obj().filter(|_| uvm_et_isobj(e));

        // No amap cleaning necessary if: there's no amap; we're not deactivating or freeing
        // pages.
        if let Some(amap) = amap.filter(|_| flags & (PGO_DEACTIVATE | PGO_FREE) != 0) {
            if imut {
                vm_map_unbusy(map);
                return Err(Errno::EPERM);
            }

            let mut cp_start = e.start.get().max(start);
            let cp_end = e.end.get().min(end);

            amap_lock(amap, RW_WRITE);
            while cp_start != cp_end {
                let off = cp_start - e.start.get();
                cp_start += PAGE_SIZE;
                let Some(anon) = amap_lookup(&e.aref, off) else {
                    continue;
                };

                kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));
                let Some(pg) = anon.page() else {
                    continue;
                };
                kassert!(pg.flags() & PQ_ANON != 0);

                match flags & (PGO_CLEANIT | PGO_FREE | PGO_DEACTIVATE) {
                    // In these first 3 cases, we just deactivate the page.
                    f if f == PGO_CLEANIT | PGO_FREE
                        || f == PGO_CLEANIT | PGO_DEACTIVATE
                        || f == PGO_DEACTIVATE =>
                    {
                        kassert!(ptr::eq(pg.uanon.get(), anon));
                        uvm_pagedeactivate(pg);
                    }
                    f if f == PGO_FREE => {
                        // If there are multiple references to the amap, just deactivate
                        // the page.
                        if amap_refs(amap) > 1 {
                            kassert!(ptr::eq(pg.uanon.get(), anon));
                            uvm_pagedeactivate(pg);
                            continue;
                        }

                        // skip the page if it's wired
                        if pg.wire_count.get() != 0 {
                            continue;
                        }
                        amap_unadd(&e.aref, off);
                        anon.an_ref.set(anon.an_ref.get() - 1);
                        if anon.an_ref.get() == 0 {
                            uvm_anfree(anon);
                        }
                    }
                    _ => panic(format_args!("uvm_map_clean: weird flags")),
                }
            }
            amap_unlock(amap);
        }

        // flush_object:
        let cp_start = e.start.get().max(start);
        let cp_end = e.end.get().min(end);

        // flush pages if we've got a valid backing object.
        //
        // Don't PGO_FREE if we don't have write permission and don't flush if this is a
        // copy-on-write object since we can't know our permissions on it.
        if let Some(uobj) = uobj
            && (flags & PGO_FREE == 0
                || (e.max_protection.get() & PROT_WRITE != 0
                    && e.etype.get() & UVM_ET_COPYONWRITE == 0))
        {
            rw_enter_write(uobj.vmobjlock());
            let rv = match uobj.pgops().pgo_flush {
                Some(flush) => flush(
                    uobj,
                    (cp_start - e.start.get()) as Voff + e.offset.get(),
                    (cp_end - e.start.get()) as Voff + e.offset.get(),
                    flags,
                ),
                None => panic(format_args!("uvm_map_clean: object without pgo_flush")),
            };
            rw_exit(uobj.vmobjlock());

            if !rv {
                error = Err(Errno::EFAULT);
            }
        }
        entry = RbtHead::<UvmMapAddr>::next(e);
    }

    vm_map_unbusy(map);
    error
}

/// `UVM_MAP_CLIP_END(map, entry, addr)`: ensure that the entry ends at or before the
/// ending address, if it doesn't we split the entry.
///
/// - map must be locked by caller
pub fn uvm_map_clip_end_at(map: &VmMap, entry: &VmMapEntry, addr: usize) {
    kassert!(entry.start.get() < addr);
    if entry.end.get() > addr {
        uvm_map_clip_end(map, entry, addr);
    }
}

/// `uvm_map_clip_end`: `UVM_MAP_CLIP_END` implementation.
pub fn uvm_map_clip_end(map: &VmMap, entry: &VmMapEntry, addr: usize) {
    kassert!(entry.start.get() < addr && vmmap_free_end(entry) > addr);
    let Some(tmp) = uvm_mapent_alloc(map, 0) else {
        panic(format_args!("uvm_map_clip_end: no map entry"));
    };

    // Invoke splitentry.
    uvm_map_splitentry(map, entry, tmp, addr);
}

/// `UVM_MAP_CLIP_START(map, entry, addr)`: ensure that the entry begins at or after the
/// starting address, if it doesn't we split the entry.
///
/// - map must be locked by caller
pub fn uvm_map_clip_start_at(map: &VmMap, entry: &VmMapEntry, addr: usize) {
    kassert!(entry.end.get() + entry.fspace.get() > addr);
    if entry.start.get() < addr {
        uvm_map_clip_start(map, entry, addr);
    }
}

/// `uvm_map_clip_start`: `UVM_MAP_CLIP_START` implementation.
///
/// Clippers are required to not change the pointers to the entry they are clipping on.
/// Since `uvm_map_splitentry` turns the original entry into the lowest entry (address
/// wise) we do a swap between the new entry and the original entry, prior to calling
/// `uvm_map_splitentry`.
pub fn uvm_map_clip_start(map: &VmMap, entry: &VmMapEntry, addr: usize) {
    // Unlink original.
    let free = uvm_map_uaddr_e(map, entry);
    uvm_mapent_free_remove(map, free, entry);
    uvm_mapent_addr_remove(map, entry);

    // Copy entry.
    kassert!(entry.start.get() < addr && vmmap_free_end(entry) > addr);
    let Some(tmp) = uvm_mapent_alloc(map, 0) else {
        panic(format_args!("uvm_map_clip_start: no map entry"));
    };
    uvm_mapent_copy(entry, tmp);

    // Put new entry in place of original entry.
    uvm_mapent_addr_insert(map, tmp);
    uvm_mapent_free_insert(map, free, tmp);

    // Invoke splitentry.
    uvm_map_splitentry(map, tmp, entry, addr);
}

/// `uvm_map_boundfix`: boundary fixer.
#[inline]
fn uvm_map_boundfix(min: usize, max: usize, bound: usize) -> usize {
    if min < bound && max > bound {
        bound
    } else {
        max
    }
}

/// `uvm_map_uaddr`: choose free list based on address at start of free space.
///
/// The `uvm_addr_state` returned contains addr and is the first of: uaddr_exe,
/// uaddr_brk_stack, uaddr_any.
pub fn uvm_map_uaddr(map: &VmMap, addr: usize) -> Option<&'static UvmAddrState> {
    // Special case the first page, to prevent mmap from returning 0.
    if addr < VMMAP_MIN_ADDR {
        return None;
    }

    // Upper bound for kernel maps at uvm_maxkaddr.
    if map.flags.get() & VM_MAP_ISVMSPACE == 0 && addr >= uvm_maxkaddr() {
        return None;
    }

    // Is the address inside the exe-only map?
    if let Some(exe) = map.uaddr_exe.get()
        && addr >= exe.uaddr_minaddr.get()
        && addr < exe.uaddr_maxaddr.get()
    {
        return Some(exe);
    }

    // Check if the space falls inside brk/stack area.
    if (addr >= map.b_start.get() && addr < map.b_end.get())
        || (addr >= map.s_start.get() && addr < map.s_end.get())
    {
        return map
            .uaddr_brk_stack
            .get()
            .filter(|bs| addr >= bs.uaddr_minaddr.get() && addr < bs.uaddr_maxaddr.get());
    }

    // Check the other selectors.
    //
    // These selectors are only marked as the owner, if they have insert functions.
    for slot in &map.uaddr_any {
        let Some(uaddr) = slot.get() else {
            continue;
        };
        if uaddr.uaddr_functions.uaddr_free_insert.is_none() {
            continue;
        }

        if addr >= uaddr.uaddr_minaddr.get() && addr < uaddr.uaddr_maxaddr.get() {
            return Some(uaddr);
        }
    }

    None
}

/// `uvm_map_uaddr_e`: choose free list based on address at start of free space.
///
/// The `uvm_addr_state` returned contains addr and is the first of: uaddr_exe,
/// uaddr_brk_stack, uaddr_any.
pub fn uvm_map_uaddr_e(map: &VmMap, entry: &VmMapEntry) -> Option<&'static UvmAddrState> {
    uvm_map_uaddr(map, vmmap_free_start(entry))
}

/// `uvm_map_boundary`: returns the first free-memory boundary that is crossed by
/// `[min-max]`.
pub fn uvm_map_boundary(map: &VmMap, min: usize, max: usize) -> usize {
    // Never return first page.
    let mut max = uvm_map_boundfix(min, max, VMMAP_MIN_ADDR);

    // Treat the maxkaddr special, if the map is a kernel_map.
    if map.flags.get() & VM_MAP_ISVMSPACE == 0 {
        max = uvm_map_boundfix(min, max, uvm_maxkaddr());
    }

    // Check for exe-only boundaries.
    if let Some(exe) = map.uaddr_exe.get() {
        max = uvm_map_boundfix(min, max, exe.uaddr_minaddr.get());
        max = uvm_map_boundfix(min, max, exe.uaddr_maxaddr.get());
    }

    // Check for exe-only boundaries.
    if let Some(bs) = map.uaddr_brk_stack.get() {
        max = uvm_map_boundfix(min, max, bs.uaddr_minaddr.get());
        max = uvm_map_boundfix(min, max, bs.uaddr_maxaddr.get());
    }

    // Check other boundaries.
    for slot in &map.uaddr_any {
        if let Some(uaddr) = slot.get() {
            max = uvm_map_boundfix(min, max, uaddr.uaddr_minaddr.get());
            max = uvm_map_boundfix(min, max, uaddr.uaddr_maxaddr.get());
        }
    }

    // Boundaries at stack and brk() area.
    max = uvm_map_boundfix(min, max, map.s_start.get());
    max = uvm_map_boundfix(min, max, map.s_end.get());
    max = uvm_map_boundfix(min, max, map.b_start.get());
    max = uvm_map_boundfix(min, max, map.b_end.get());

    max
}

/// `uvm_map_vmspace_update`: update map allocation start and end addresses from proc
/// vmspace.
pub fn uvm_map_vmspace_update(map: &VmMap, dead: &UvmMapDeadq, flags: u32) {
    kassert!(map.flags.get() & VM_MAP_ISVMSPACE != 0);
    // KASSERT(offsetof(struct vmspace, vm_map) == 0): asserted at compile time.

    // Derive actual allocation boundaries from vmspace.
    let vm = map.vmspace();
    let b_start = vm.vm_daddr.get();
    let b_end = b_start + <Machine as VmParam>::BRKSIZ;
    let s_start = vm.vm_maxsaddr.get().min(vm.vm_minsaddr.get());
    let s_end = vm.vm_maxsaddr.get().max(vm.vm_minsaddr.get());
    #[cfg(feature = "diagnostic")]
    if b_start & PAGE_MASK != 0
        || b_end & PAGE_MASK != 0
        || s_start & PAGE_MASK != 0
        || s_end & PAGE_MASK != 0
    {
        panic(format_args!(
            "uvm_map_vmspace_update: vmspace {:p} invalid bounds: b={:#x}-{:#x} s={:#x}-{:#x}",
            ptr::from_ref(vm),
            b_start,
            b_end,
            s_start,
            s_end
        ));
    }

    if map.b_start.get() == b_start
        && map.b_end.get() == b_end
        && map.s_start.get() == s_start
        && map.s_end.get() == s_end
    {
        return;
    }

    uvm_map_freelist_update(map, dead, b_start, b_end, s_start, s_end, flags);
}

/// `uvm_map_kmem_grow`: grow kernel memory.
///
/// This function is only called for kernel maps when an allocation fails.
///
/// If the map has a gap that is large enough to accommodate alloc_sz, this function will
/// make sure `map->free` will include it.
pub fn uvm_map_kmem_grow(map: &VmMap, dead: &UvmMapDeadq, alloc_sz: usize, flags: u32) {
    // Kernel memory only.
    kassert!(map.flags.get() & VM_MAP_ISVMSPACE == 0);
    // Destroy free list.
    uvm_map_freelist_update_clear(map, dead);

    // Include the guard page in the hard minimum requirement of alloc_sz.
    let alloc_sz = if map.flags.get() & VM_MAP_GUARDPAGES != 0 {
        alloc_sz + PAGE_SIZE
    } else {
        alloc_sz
    };

    // Grow by ALLOCMUL * alloc_sz, but at least VM_MAP_KSIZE_DELTA.
    //
    // Don't handle the case where the multiplication overflows: if that happens, the
    // allocation is probably too big anyway.
    let sz = (VM_MAP_KSIZE_ALLOCMUL.wrapping_mul(alloc_sz)).max(VM_MAP_KSIZE_DELTA);

    // Walk forward until a gap large enough for alloc_sz shows up.
    //
    // We assume the kernel map has no boundaries.
    let mut end = uvm_maxkaddr().max(map.min_offset.get());
    let mut entry = uvm_map_entrybyaddr(&map.addr, end);
    while let Some(e) = entry.filter(|e| e.fspace.get() < alloc_sz) {
        entry = RbtHead::<UvmMapAddr>::next(e);
    }
    if let Some(e) = entry {
        end = vmmap_free_start(e).max(end);
        end += sz.min(map.max_offset.get() - end);
    } else {
        end = map.max_offset.get();
    }

    // Reserve pmap entries (PMAP_GROWKERNEL is defined on amd64 and arm64).
    UVM_MAXKADDR.store(
        Machine::pmap_growkernel(Vaddr::new(end)).as_usize(),
        AtomicOrdering::Relaxed,
    );

    // Rebuild free list.
    uvm_map_freelist_update_refill(map, flags);
}

/// `uvm_map_freelist_update_clear`: freelist update subfunction: unlink all entries from
/// freelists.
pub fn uvm_map_freelist_update_clear(map: &VmMap, dead: &UvmMapDeadq) {
    let mut prev: Option<&VmMapEntry> = None;
    let mut entry = map.addr.min();
    while let Some(e) = entry {
        let next = RbtHead::<UvmMapAddr>::next(e);

        let free = uvm_map_uaddr_e(map, e);
        uvm_mapent_free_remove(map, free, e);

        match prev {
            Some(p) if e.start.get() == e.end.get() => {
                p.fspace
                    .set(p.fspace.get() + (vmmap_free_end(e) - e.end.get()));
                uvm_mapent_addr_remove(map, e);
                dead_entry_push(dead, e);
            }
            _ => prev = Some(e),
        }
        entry = next;
    }
}

/// `uvm_map_freelist_update_refill`: freelist update subfunction: refill the freelists
/// with entries.
pub fn uvm_map_freelist_update_refill(map: &VmMap, flags: u32) {
    // RBT_FOREACH, where the body replaces `entry` by what uvm_map_fix_space returns and
    // the next step continues from there.
    let mut entry = map.addr.min();
    while let Some(e) = entry {
        let min = vmmap_free_start(e);
        let max = vmmap_free_end(e);
        e.fspace.set(0);

        let last = uvm_map_fix_space(map, Some(e), min, max, flags);
        entry = last.and_then(RbtHead::<UvmMapAddr>::next);
    }

    uvm_tree_sanity(map, "uvm_map_freelist_update_refill");
}

/// `uvm_map_freelist_update`: change `{a,b}_{start,end}` allocation ranges and associated
/// free lists.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_map_freelist_update(
    map: &VmMap,
    dead: &UvmMapDeadq,
    b_start: usize,
    b_end: usize,
    s_start: usize,
    s_end: usize,
    flags: u32,
) {
    kdassert!(b_end >= b_start && s_end >= s_start);
    vm_map_assert_wrlock(map);

    // Clear all free lists.
    uvm_map_freelist_update_clear(map, dead);

    // Apply new bounds.
    map.b_start.set(b_start);
    map.b_end.set(b_end);
    map.s_start.set(s_start);
    map.s_end.set(s_end);

    // Refill free lists.
    uvm_map_freelist_update_refill(map, flags);
}

/// One of the selector slots of a map (the C's `struct uvm_addr_state **which`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvmMapUaddrSlot {
    /// `map->uaddr_exe`.
    Exe,
    /// `map->uaddr_any[i]`.
    Any(usize),
    /// `map->uaddr_brk_stack`.
    BrkStack,
}

/// `uvm_map_set_uaddr`: assign a `uvm_addr_state` to the specified pointer in vm_map.
///
/// May sleep.
pub fn uvm_map_set_uaddr(
    map: &VmMap,
    which: UvmMapUaddrSlot,
    newval: Option<&'static UvmAddrState>,
) {
    let slot = match which {
        UvmMapUaddrSlot::Exe => &map.uaddr_exe,
        UvmMapUaddrSlot::Any(i) => &map.uaddr_any[i],
        UvmMapUaddrSlot::BrkStack => &map.uaddr_brk_stack,
    };

    vm_map_lock(map);
    let dead = UvmMapDeadq::new();
    uvm_map_freelist_update_clear(map, &dead);

    uvm_addr_destroy(slot.replace(newval));

    uvm_map_freelist_update_refill(map, 0);
    vm_map_unlock(map);
    uvm_unmap_detach(&dead, 0);
}

/// `uvm_map_fix_space`: correct space insert.
///
/// Entry must not be on any freelist. Returns the entry that describes the end of the
/// range (the one that took the last of the free space).
pub fn uvm_map_fix_space<'m>(
    map: &'m VmMap,
    entry: Option<&'m VmMapEntry>,
    min: usize,
    max: usize,
    flags: u32,
) -> Option<&'m VmMapEntry> {
    kassert!(entry.is_none_or(|e| e.etype.get() & UVM_ET_FREEMAPPED == 0));
    kdassert!(min <= max);
    kdassert!(entry.is_some_and(|e| vmmap_free_end(e) == min) || min == map.min_offset.get());

    uvm_map_req_write(map);

    // During the function, entfree will always point at the uaddr state for entry.
    let mut entry = entry;
    let mut entfree = entry.and_then(|e| uvm_map_uaddr_e(map, e));

    let mut min = min;
    while min != max {
        // Claim guard page for entry.
        if let Some(e) = entry.filter(|e| {
            map.flags.get() & VM_MAP_GUARDPAGES != 0
                && vmmap_free_end(e) == e.end.get()
                && e.start.get() != e.end.get()
        }) {
            if max - min == 2 * PAGE_SIZE {
                // If the free-space gap is exactly 2 pages, we make the guard 2 pages
                // instead of 1. Because in a guarded map, an area needs at least 2 pages
                // to allocate from: one page for the allocation and one for the guard.
                e.guard.set(2 * PAGE_SIZE);
                min = max;
            } else {
                e.guard.set(PAGE_SIZE);
                min += PAGE_SIZE;
            }
            continue;
        }

        // Handle the case where entry has a 2-page guard, but the space after entry is
        // freed.
        if let Some(e) = entry.filter(|e| e.fspace.get() == 0 && e.guard.get() > PAGE_SIZE) {
            e.guard.set(PAGE_SIZE);
            min = vmmap_free_start(e);
        }

        let lmax = uvm_map_boundary(map, min, max);
        let free = uvm_map_uaddr(map, min);

        // Entries are merged if they point at the same uvm_free(). Exception to that rule:
        // if min == uvm_maxkaddr, a new entry is started regardless (otherwise the
        // allocators will get confused).
        match entry {
            Some(e)
                if same_uaddr(free, entfree)
                    && !(map.flags.get() & VM_MAP_ISVMSPACE == 0 && min == uvm_maxkaddr()) =>
            {
                kdassert!(vmmap_free_end(e) == min);
                e.fspace.set(e.fspace.get() + (lmax - min));
            }
            Some(e) if e.start.get() == min && e.end.get() == min && e.fspace.get() == 0 => {
                // A free-only entry that already sits at min (a kernel map's first entry
                // when nothing is mapped below uvm_maxkaddr, see the module's deviations):
                // it is the new entry the C would allocate, which would collide with it.
                e.fspace.set(lmax - min);
                entfree = free;
            }
            _ => {
                // Commit entry to free list: it'll not be added to anymore. We'll start a
                // new entry and add to that entry instead.
                if let Some(e) = entry {
                    uvm_mapent_free_insert(map, entfree, e);
                }

                // New entry for new uaddr.
                let Some(e) = uvm_mapent_alloc(map, flags) else {
                    panic(format_args!("uvm_map_fix_space: no map entry"));
                };
                e.end.set(min);
                e.start.set(min);
                e.guard.set(0);
                e.fspace.set(lmax - min);
                e.object.set(VmMapEntryObject::None);
                e.offset.set(0);
                e.etype.set(0);
                e.protection.set(0);
                e.max_protection.set(0);
                e.inheritance.set(0);
                e.wired_count.set(0);
                e.advice.set(0);
                e.aref.ar_pageoff.set(0);
                e.aref.ar_amap.set(ptr::null());
                uvm_mapent_addr_insert(map, e);

                entry = Some(e);
                entfree = free;
            }
        }

        min = lmax;
    }
    // Finally put entry on the uaddr state.
    if let Some(e) = entry {
        uvm_mapent_free_insert(map, entfree, e);
    }

    entry
}

/// `uvm_map_mquery`: MQuery style of allocation.
///
/// This allocator searches forward until sufficient space is found to map the given size.
///
/// XXX: factor in offset (via pmap_prefer) and protection?
pub fn uvm_map_mquery(
    map: &VmMap,
    addr_p: &mut usize,
    sz: usize,
    offset: Voff,
    flags: u32,
) -> Result<(), Errno> {
    let mut addr = *addr_p;
    vm_map_lock_read(map);

    let error = uvm_map_mquery_locked(map, &mut addr, sz, offset, flags);

    vm_map_unlock_read(map);
    if error.is_ok() {
        *addr_p = addr;
    }
    error
}

/// [`uvm_map_mquery`] with the map read-locked (the C's body up to the `out` label).
fn uvm_map_mquery_locked(
    map: &VmMap,
    addr: &mut usize,
    sz: usize,
    offset: Voff,
    flags: u32,
) -> Result<(), Errno> {
    // Configure pmap prefer.
    let (pmap_align, pmap_offset) = if offset != UVM_UNKNOWN_OFFSET {
        (
            PAGE_SIZE.max(pmap_prefer_align()),
            pmap_prefer_offset(offset),
        )
    } else {
        (PAGE_SIZE, 0)
    };

    // Align address to pmap_prefer unless FLAG_FIXED is set.
    if flags & UVM_FLAG_FIXED == 0 && offset != UVM_UNKNOWN_OFFSET {
        let mut tmp = (*addr & !(pmap_align - 1)) | pmap_offset;
        if tmp < *addr {
            tmp += pmap_align;
        }
        *addr = tmp;
    }

    // First, check if the requested range is fully available.
    let mut entry = uvm_map_entrybyaddr(&map.addr, *addr);
    let mut last = None;
    if uvm_map_isavail(map, None, &mut entry, &mut last, *addr, sz) {
        return Ok(());
    }
    if flags & UVM_FLAG_FIXED != 0 {
        return Err(Errno::EINVAL);
    }

    // ENOMEM is the default error from here.

    // At this point, the memory at <addr, sz> is not available. The reasons are:
    // [1] it's outside the map,
    // [2] it starts in used memory (and therefore needs to move toward the first free page
    //     in entry),
    // [3] it starts in free memory but bumps into used memory.
    //
    // Note that for case [2], the forward moving is handled by the for loop below.
    match entry {
        None => {
            // [1] Outside the map.
            if *addr >= map.max_offset.get() {
                return Err(Errno::ENOMEM);
            }
            entry = map.addr.min();
        }
        Some(e) if vmmap_free_start(e) <= *addr => {
            // [3] Bumped into used memory.
            entry = RbtHead::<UvmMapAddr>::next(e);
        }
        _ => {}
    }

    // Test if the next entry is sufficient for the allocation.
    'entries: while let Some(e) = entry {
        entry = RbtHead::<UvmMapAddr>::next(e);
        if e.fspace.get() == 0 {
            continue;
        }
        *addr = vmmap_free_start(e);

        // restart: Restart address checks on address change.
        loop {
            let mut tmp = (*addr & !(pmap_align - 1)) | pmap_offset;
            if tmp < *addr {
                tmp += pmap_align;
            }
            *addr = tmp;
            if *addr >= vmmap_free_end(e) {
                continue 'entries;
            }

            // Skip brk() allocation addresses.
            if *addr + sz > map.b_start.get() && *addr < map.b_end.get() {
                if vmmap_free_end(e) > map.b_end.get() {
                    *addr = map.b_end.get();
                    continue;
                } else {
                    continue 'entries;
                }
            }
            // Skip stack allocation addresses.
            if *addr + sz > map.s_start.get() && *addr < map.s_end.get() {
                if vmmap_free_end(e) > map.s_end.get() {
                    *addr = map.s_end.get();
                    continue;
                } else {
                    continue 'entries;
                }
            }
            break;
        }

        let mut first = Some(e);
        let mut last = None;
        if uvm_map_isavail(map, None, &mut first, &mut last, *addr, sz) {
            return Ok(());
        }
    }

    Err(Errno::ENOMEM)
}

// uvm_map_fill_vmmap: kinfo_vmentry (sysctl), see the module's deviations.

// RBT_GENERATE_AUGMENT(uvm_map_addr, ...): the `UvmMapAddr` adapter above.

/// `uvm_map_setup_md`: MD code: vmspace allocator setup (the `__LP64__` version).
pub fn uvm_map_setup_md(map: &VmMap) {
    let mut min = map.min_offset.get();
    let max = map.max_offset.get();

    // Ensure the selectors will not try to manage page 0; it's too special.
    if min < VMMAP_MIN_ADDR {
        min = VMMAP_MIN_ADDR;
    }

    // "Cool stuff, not yet": the pivot selector is #if 0; "Crappy stuff, for now":
    map.uaddr_any[0].set(Some(uaddr_rnd_create(min, max)));

    // SMALL_KERNEL is not configured.
    map.uaddr_brk_stack
        .set(Some(uaddr_stack_brk_create(min, max)));
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the map operations over real memory (see `subr_pool.rs` for the
    // setup). The map, the entry tree, the selectors and the vmspace life cycle run on the
    // host's pmap double; the tree checks of `VMMAP_DEBUG` are on in tests.

    use std::sync::MutexGuard;
    use std::{assert, assert_eq, assert_ne};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::mman::MAP_INHERIT_COPY;
    use crate::uvm::uvm_amap::amap_init;
    use crate::uvm::uvm_anon::uvm_anon_init;

    const RW: VmProt = PROT_READ | PROT_WRITE;
    const RWX: VmProt = PROT_READ | PROT_WRITE | PROT_EXEC;

    /// Real memory, the lock objects, the anon and amap pools and the map pools.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        // The console, so that a kernel panic in a test says why.
        crate::machine::cons::consinit();
        rw_obj_init();
        uvm_anon_init();
        amap_init();
        uvm_map_init();
        guard
    }

    /// A vmspace laid out as `exec` leaves one: data at 256 MiB, the stack below `USRSTACK`.
    fn user_vmspace() -> &'static Vmspace {
        let vm = uvmspace_alloc(
            <Machine as VmParam>::VM_MIN_ADDRESS,
            <Machine as VmParam>::VM_MAXUSER_ADDRESS,
            true,
            false,
        );
        vm.vm_daddr.set(256 << 20);
        vm.vm_minsaddr.set(<Machine as VmParam>::USRSTACK);
        vm.vm_maxsaddr
            .set(<Machine as VmParam>::USRSTACK - <Machine as VmParam>::MAXSSIZ);
        vm
    }

    /// `uvm_mapanon` flags for a private read-write mapping, `extra` on top.
    fn anon_flags(extra: u32) -> u32 {
        uvm_mapflag(
            RW,
            RWX,
            MAP_INHERIT_COPY,
            MADV_NORMAL,
            UVM_FLAG_COPYONW | extra,
        )
    }

    /// The entry that holds `addr`, under the read lock.
    fn entry_holding(map: &VmMap, addr: usize) -> Option<(usize, usize, VmProt, i32)> {
        vm_map_lock_read(map);
        let found = uvm_map_lookup_entry(map, addr).map(|e| {
            (
                e.start.get(),
                e.end.get(),
                e.protection.get(),
                e.etype.get(),
            )
        });
        vm_map_unlock_read(map);
        found
    }

    #[test]
    fn setup_entries_cover_the_map() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        uvm_tree_sanity(map, "test");
        uvm_tree_size_chk(map, "test");
        assert_eq!(map.size.get().as_usize(), 0);
        assert!(map.addr.min().is_some());
        assert!(map.uaddr_any[0].get().is_some(), "the rnd selector");
        assert!(
            map.uaddr_brk_stack.get().is_some(),
            "the stack/brk selector"
        );
        uvmspace_free(vm);
    }

    #[test]
    fn mapanon_fixed_then_unmap() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let mut addr = 0x2000_0000;
        let len = 4 * PAGE_SIZE;

        uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
        assert_eq!(addr, 0x2000_0000);
        assert_eq!(map.size.get().as_usize(), len);
        assert_eq!(vm.vm_dused.get(), 4);
        let (start, end, prot, etype) = entry_holding(map, addr + PAGE_SIZE).expect("mapped");
        assert_eq!((start, end, prot), (addr, addr + len, RW));
        assert_ne!(etype & UVM_ET_COPYONWRITE, 0);
        assert_ne!(etype & UVM_ET_NEEDSCOPY, 0);
        assert!(
            entry_holding(map, addr + len).is_none(),
            "nothing past the end"
        );

        // The same range again is not available.
        let mut again = addr;
        assert_eq!(
            uvm_mapanon(map, &mut again, len, 0, anon_flags(UVM_FLAG_FIXED)),
            Err(Errno::ENOMEM)
        );

        uvm_unmap(map, addr, addr + len);
        assert_eq!(map.size.get().as_usize(), 0);
        assert_eq!(vm.vm_dused.get(), 0);
        assert!(entry_holding(map, addr).is_none());
        uvmspace_free(vm);
    }

    #[test]
    fn mapanon_finds_space_and_honours_a_hint() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let len = 8 * PAGE_SIZE;

        let mut a = 0;
        uvm_mapanon(map, &mut a, len, 0, anon_flags(0)).expect("first");
        assert_eq!(a & PAGE_MASK, 0);
        assert!(a >= <Machine as VmParam>::VM_MIN_ADDRESS);
        assert!(a + len <= <Machine as VmParam>::VM_MAXUSER_ADDRESS);

        let mut b = 0;
        uvm_mapanon(map, &mut b, len, 0, anon_flags(0)).expect("second");
        assert!(b + len <= a || a + len <= b, "no overlap: {a:#x} {b:#x}");

        // A free, aligned address is taken as given.
        let mut c = 0x3000_0000;
        uvm_mapanon(map, &mut c, len, 0, anon_flags(0)).expect("hinted");
        assert_eq!(c, 0x3000_0000);

        // An alignment request is honoured.
        let mut d = 0;
        uvm_mapanon(map, &mut d, len, 1 << 21, anon_flags(0)).expect("aligned");
        assert_eq!(d & ((1 << 21) - 1), 0);

        assert_eq!(map.size.get().as_usize(), 4 * len);
        vm_map_lock_read(map);
        assert!(uvm_map_checkprot(map, c, c + len, PROT_READ));
        assert!(!uvm_map_checkprot(map, c, c + len, PROT_EXEC));
        assert!(
            !uvm_map_checkprot(map, c, c + 2 * len, PROT_READ),
            "a hole fails"
        );
        vm_map_unlock_read(map);
        uvmspace_free(vm);
    }

    #[test]
    fn protect_clips_and_checks_limits() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let mut addr = 0x4000_0000;
        let len = 4 * PAGE_SIZE;
        uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");

        // Beyond max_protection.
        assert_eq!(
            uvm_map_protect(map, addr, addr + len, RWX | 0x8, 0, false, true),
            Err(Errno::EACCES)
        );

        // The middle two pages become read-only: three entries.
        uvm_map_protect(
            map,
            addr + PAGE_SIZE,
            addr + 3 * PAGE_SIZE,
            PROT_READ,
            0,
            false,
            true,
        )
        .expect("protect");
        let (s0, e0, p0, _) = entry_holding(map, addr).expect("first");
        let (s1, e1, p1, _) = entry_holding(map, addr + PAGE_SIZE).expect("middle");
        let (s2, e2, p2, _) = entry_holding(map, addr + 3 * PAGE_SIZE).expect("last");
        assert_eq!((s0, e0, p0), (addr, addr + PAGE_SIZE, RW));
        assert_eq!(
            (s1, e1, p1),
            (addr + PAGE_SIZE, addr + 3 * PAGE_SIZE, PROT_READ)
        );
        assert_eq!((s2, e2, p2), (addr + 3 * PAGE_SIZE, addr + len, RW));
        assert_eq!(map.size.get().as_usize(), len);
        assert_eq!(vm.vm_dused.get(), 4);

        // PROT_NONE drops the pages from vm_dused; back to RW restores them.
        uvm_map_protect(map, addr, addr + len, PROT_NONE, 0, false, true).expect("none");
        assert_eq!(vm.vm_dused.get(), 0);
        uvm_map_protect(map, addr, addr + len, RW, 0, false, true).expect("rw");
        assert_eq!(vm.vm_dused.get(), 4);

        uvmspace_free(vm);
    }

    #[test]
    fn immutable_entries_refuse_changes() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let mut addr = 0x5000_0000;
        let len = 2 * PAGE_SIZE;
        uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
        uvm_map_immutable(map, addr, addr + len, true).expect("immutable");

        assert_eq!(
            uvm_map_protect(map, addr, addr + len, PROT_READ, 0, false, true),
            Err(Errno::EPERM)
        );
        assert_eq!(
            uvm_map_inherit(map, addr, addr + len, MAP_INHERIT_SHARE),
            Err(Errno::EPERM)
        );
        let mut over = addr;
        assert_eq!(
            uvm_mapanon(
                map,
                &mut over,
                len,
                0,
                anon_flags(UVM_FLAG_FIXED | UVM_FLAG_UNMAP)
            ),
            Err(Errno::EPERM)
        );

        // Without the check the entries go away as usual.
        uvm_unmap(map, addr, addr + len);
        assert_eq!(map.size.get().as_usize(), 0);
        uvmspace_free(vm);
    }

    #[test]
    fn inherit_advice_and_fork() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let len = 2 * PAGE_SIZE;
        let (mut copy, mut share, mut none, mut overlay) =
            (0x6000_0000, 0x6100_0000, 0x6200_0000, 0x6300_0000);
        uvm_mapanon(map, &mut copy, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("copy");
        uvm_mapanon(map, &mut share, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("share");
        uvm_mapanon(map, &mut none, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("none");
        uvm_mapanon(
            map,
            &mut overlay,
            len,
            0,
            anon_flags(UVM_FLAG_FIXED | UVM_FLAG_OVERLAY),
        )
        .expect("overlay");
        uvm_map_inherit(map, share, share + len, MAP_INHERIT_SHARE).expect("inherit share");
        uvm_map_inherit(map, none, none + len, MAP_INHERIT_NONE).expect("inherit none");
        assert_eq!(
            uvm_map_inherit(map, none, none + len, 7),
            Err(Errno::EINVAL)
        );
        uvm_map_advice(map, copy, copy + PAGE_SIZE, MADV_RANDOM).expect("advice");
        assert_eq!(
            uvm_map_advice(map, copy, copy + len, 99),
            Err(Errno::EINVAL)
        );
        assert_eq!(map.size.get().as_usize(), 4 * len);

        let pr = Process::new();
        pr.ps_vmspace.set(vm);
        let child = uvmspace_fork(&pr);
        let cmap = &child.vm_map;

        // copy, share and overlay are inherited; none is not.
        assert_eq!(cmap.size.get().as_usize(), 3 * len);
        assert_eq!(child.vm_dused.get(), 6);
        let (_, _, _, etype) = entry_holding(cmap, copy).expect("copy inherited");
        assert_ne!(etype & UVM_ET_NEEDSCOPY, 0);
        assert!(entry_holding(cmap, share).is_some(), "share inherited");
        assert!(entry_holding(cmap, none).is_none(), "none not inherited");
        assert!(entry_holding(cmap, overlay).is_some(), "overlay inherited");
        vm_map_lock_read(cmap);
        let child_amap = uvm_map_lookup_entry(cmap, overlay)
            .and_then(|e| e.aref.amap())
            .expect("the overlay's amap is shared with the child");
        vm_map_unlock_read(cmap);
        assert_eq!(amap_refs(child_amap), 2);

        uvmspace_free(child);
        assert_eq!(amap_refs(child_amap), 1, "the child's reference is gone");
        uvmspace_free(vm);
    }

    #[test]
    fn kernel_style_map_reserves_grows_and_allocates() {
        let _g = setup();
        let min = <Machine as VmParam>::VM_MIN_KERNEL_ADDRESS;
        let max = min + (1 << 30);
        UVM_MAXKADDR.store(min, AtomicOrdering::Relaxed);
        pmap_reference(pmap_kernel());
        let map = uvm_map_create(pmap_kernel(), min, max, VM_MAP_PAGEABLE).expect("map");
        assert!(map.uaddr_any[3].get().is_some(), "the bootstrap selector");
        let kflags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, UVM_FLAG_FIXED);

        // The bootstrap reservation, as uvm_km_init makes it, grows uvm_maxkaddr.
        let mut base = min;
        let reserved = 16 * PAGE_SIZE;
        uvm_map(
            map,
            &mut base,
            reserved,
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            kflags,
        )
        .expect("reserve");
        assert_eq!(base, min);
        assert!(UVM_MAXKADDR.load(AtomicOrdering::Relaxed) >= min + reserved);

        // A non-fixed allocation lands above it, through uaddr_kbootstrap.
        let mut va = 0;
        let flags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, 0);
        uvm_map(
            map,
            &mut va,
            4 * PAGE_SIZE,
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            flags,
        )
        .expect("alloc");
        assert!(va >= min + reserved && va + 4 * PAGE_SIZE <= max, "{va:#x}");
        assert_eq!(map.size.get().as_usize(), reserved + 4 * PAGE_SIZE);

        // W^X is refused on the kernel map only; this map is not kernel_map.
        assert!(!is_kernel_map(map));

        uvm_unmap(map, va, va + 4 * PAGE_SIZE);
        assert_eq!(map.size.get().as_usize(), reserved);
        uvm_map_deallocate(map);
    }

    #[test]
    fn bestfit_switch_on_an_empty_kernel_map() {
        let _g = setup();
        let min = <Machine as VmParam>::VM_MIN_KERNEL_ADDRESS;
        let max = min + (1 << 30);
        UVM_MAXKADDR.store(min, AtomicOrdering::Relaxed);
        pmap_reference(pmap_kernel());
        let map = uvm_map_create(pmap_kernel(), min, max, VM_MAP_PAGEABLE).expect("map");

        // uvm_init's switch, with nothing reserved below uvm_maxkaddr (amd64's case).
        uvm_map_set_uaddr(
            map,
            UvmMapUaddrSlot::Any(3),
            Some(crate::uvm::uvm_addr::uaddr_bestfit_create(min, max)),
        );
        let flags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, 0);
        let mut va = 0;
        uvm_map(
            map,
            &mut va,
            2 * PAGE_SIZE,
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            flags,
        )
        .expect("alloc");
        assert!(va >= min && va + 2 * PAGE_SIZE <= max, "{va:#x}");
        let mut vb = 0;
        uvm_map(
            map,
            &mut vb,
            2 * PAGE_SIZE,
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            flags,
        )
        .expect("alloc 2");
        assert!(vb + 2 * PAGE_SIZE <= va || va + 2 * PAGE_SIZE <= vb);
        assert!(UVM_MAXKADDR.load(AtomicOrdering::Relaxed) >= va + 2 * PAGE_SIZE);
        uvm_unmap(map, va, va + 2 * PAGE_SIZE);
        uvm_unmap(map, vb, vb + 2 * PAGE_SIZE);
        assert_eq!(map.size.get().as_usize(), 0);
        uvm_map_deallocate(map);
    }

    #[test]
    fn mquery_skips_used_space() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let mut addr = 0x7000_0000;
        let len = 4 * PAGE_SIZE;
        uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");

        let mut q = addr + PAGE_SIZE;
        uvm_map_mquery(map, &mut q, PAGE_SIZE, UVM_UNKNOWN_OFFSET, 0).expect("mquery");
        assert!(q >= addr + len, "{q:#x} is past the mapping");

        let mut fixed = addr + PAGE_SIZE;
        assert_eq!(
            uvm_map_mquery(
                map,
                &mut fixed,
                PAGE_SIZE,
                UVM_UNKNOWN_OFFSET,
                UVM_FLAG_FIXED
            ),
            Err(Errno::EINVAL)
        );
        uvmspace_free(vm);
    }

    #[test]
    fn pie_addresses_stay_in_range() {
        for _ in 0..64 {
            let addr = uvm_map_pie(1 << 21);
            assert_eq!(addr & ((1 << 21) - 1), 0);
            assert!(addr >= VM_PIE_MIN_ADDR && addr < VM_PIE_MAX_ADDR + (1 << 21));
        }
    }

    #[test]
    fn exec_recycles_a_single_user_vmspace() {
        let _g = setup();
        let vm = user_vmspace();
        let map = &vm.vm_map;
        let mut addr = 0x2000_0000;
        uvm_mapanon(map, &mut addr, PAGE_SIZE, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
        let pr = Process::new();
        pr.ps_vmspace.set(vm);
        let p = Proc::new();
        p.p_p.set(&pr);
        p.p_vmspace.set(vm);

        uvmspace_exec(&p, PAGE_SIZE, 0x1000_0000);
        assert_eq!(map.min_offset.get(), PAGE_SIZE);
        assert_eq!(map.max_offset.get(), 0x1000_0000);
        assert_eq!(map.size.get().as_usize(), 0);
        assert!(entry_holding(map, addr).is_none());
        uvm_tree_sanity(map, "after exec");
        uvmspace_free(vm);
    }
}
/* </TESTS> */
