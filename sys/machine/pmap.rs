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
//! `<machine/pmap.h>` and the machine-dependent half of `<uvm/uvm_pmap.h>` as a trait: the
//! physical map, what `uvm` asks the MMU code to do.
//!
//! Milestone M3 needs the direct map, boot-time memory stealing, page zeroing and the kernel
//! mapping entry points (`pmap_kenter_pa`, `pmap_kremove`, `pmap_extract`). M6 adds the
//! user-space side: `pmap_create`/`pmap_destroy`/`pmap_reference`, `pmap_enter`/`pmap_remove`
//! and activation. M7a (part 1) adds `pmap_page_protect` and `pmap_clear_modify`, which the
//! object layer calls; `pmap_protect`, the reference bit and `pmap_unwire` arrive with
//! `uvm_fault` (M7a part 3).

use crate::machine::Machine;
use crate::sys::errno::Errno;
use crate::sys::proc::{Proc, Process};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt, Vmspace};
use crate::uvm::uvm_page::VmPage;

/// `struct vm_page_md` of the selected architecture: the pmap's per-page data inside
/// `struct vm_page`.
pub type VmPageMd = <Machine as Pmap>::VmPageMd;

/// `struct pmap` of the selected architecture (what a `pmap_t` points at).
pub type MachinePmap = <Machine as Pmap>::Pmap;

/// The physical map interface.
pub trait Pmap {
    /// `struct vm_page_md`.
    type VmPageMd: 'static;
    /// `struct pmap`.
    type Pmap: 'static;

    /// `VM_MDPAGE_INIT`: a fresh `vm_page_md`.
    const VM_MDPAGE_INIT: Self::VmPageMd;
    /// `__HAVE_PMAP_DIRECT`: physical memory is direct-mapped, so `pmap_map_direct` works.
    const HAVE_PMAP_DIRECT: bool;
    /// `PMAP_STEAL_MEMORY`: `uvm_pageboot_alloc` defers to `pmap_steal_memory`.
    const PMAP_STEAL_MEMORY: bool;
    /// `uvm_md_constraints[]`: the DMA constraint ranges of the machine, lowest first.
    const UVM_MD_CONSTRAINTS: &'static [&'static UvmConstraintRange];
    /// `dma_constraint`: the range every DMA-capable device can reach.
    const DMA_CONSTRAINT: &'static UvmConstraintRange;
    /// `PMAP_WC`: the physical-address flag of a write-combining mapping (0 where the pmap
    /// has none, as `<uvm/uvm_pmap.h>` defaults it).
    const PMAP_WC: usize;
    /// `PMAP_NOCACHE`: the physical-address flag of an uncached mapping (a frame buffer's
    /// `mmap`, M13; 0 where the pmap has none).
    const PMAP_NOCACHE: usize;
    /// No MMU behind the pmap: only the direct map is addressable (the `host` test double,
    /// whose "physical" pages are the test process's memory). `km_alloc` and `kmeminit` then
    /// serve every request through the direct map. False on every real machine.
    const PMAP_NOMMU: bool;

    /// `pmap_kernel()`: the kernel's pmap.
    fn pmap_kernel() -> &'static Self::Pmap;

    /// `pmap_create()`: a new, empty user pmap with one reference; its top-level table maps
    /// the kernel half.
    fn pmap_create() -> &'static Self::Pmap;

    /// `pmap_destroy(pmap)`: drops a reference; the last one frees the page tables and the
    /// pmap itself (the caller must have removed every mapping).
    fn pmap_destroy(pmap: &'static Self::Pmap);

    /// `pmap_reference(pmap)`: one more reference.
    fn pmap_reference(pmap: &Self::Pmap);

    /// `pmap_enter(pmap, va, pa, prot, flags)`: maps `pa` at `va` with `prot`; `flags` carries
    /// `PMAP_WIRED`, `PMAP_CANFAIL` and the access type. Fails with `ENOMEM` only under
    /// `PMAP_CANFAIL`.
    fn pmap_enter(
        pmap: &Self::Pmap,
        va: Vaddr,
        pa: Paddr,
        prot: VmProt,
        flags: i32,
    ) -> Result<(), Errno>;

    /// `pmap_remove(pmap, sva, eva)`: removes the mappings in `[sva, eva)`.
    fn pmap_remove(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr);

    /// `pmap_protect(pmap, sva, eva, prot)`: lowers the protection of the mappings in
    /// `[sva, eva)` to `prot`; `PROT_NONE` removes them.
    fn pmap_protect(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr, prot: VmProt);

    /// `pmap_wired_count(pmap)`: the number of wired pages in the pmap (`pm_stats`).
    fn pmap_wired_count(pmap: &Self::Pmap) -> i64;

    /// `pmap_resident_count(pmap)`: the number of resident pages in the pmap (`pm_stats`).
    fn pmap_resident_count(pmap: &Self::Pmap) -> i64;

    /// `pmap_unwire(pmap, va)`: clears the wired bit of the mapping at `va`.
    fn pmap_unwire(pmap: &Self::Pmap, va: Vaddr);

    /// `pmap_page_protect(pg, prot)`: lowers every mapping of `pg` to `prot`; `PROT_NONE`
    /// removes them all (what the object layer does before freeing a page).
    fn pmap_page_protect(pg: &VmPage, prot: VmProt);

    /// `pmap_clear_modify(pg)`: clears the modified bit of every mapping of `pg`; whether any
    /// was set.
    fn pmap_clear_modify(pg: &VmPage) -> bool;

    /// `pmap_clear_reference(pg)`: clears the referenced bit of every mapping of `pg`; whether
    /// any was set.
    fn pmap_clear_reference(pg: &VmPage) -> bool;

    /// `pmap_is_modified(pg)`: whether any mapping of `pg` has its modified bit set.
    fn pmap_is_modified(pg: &VmPage) -> bool;

    /// `pmap_remove_holes(vm)`: makes the MMU's unmappable holes unavailable in the map
    /// (nothing on amd64 and arm64).
    fn pmap_remove_holes(vm: &Vmspace);

    /// `pmap_proc_iflush(pr, va, len)`: makes instructions just written to `[va, va+len)` of
    /// `pr`'s address space visible to instruction fetch (an I-cache sync where the caches
    /// are not coherent).
    fn pmap_proc_iflush(pr: &Process, va: Vaddr, len: Vsize);

    /// `pmap_zero_page`: zero-fills the page.
    fn pmap_zero_page(pg: &VmPage);

    /// `pmap_copy_page`: copies `src` into `dst`.
    fn pmap_copy_page(src: &VmPage, dst: &VmPage);

    /// `pmap_steal_memory`: takes `size` bytes of physical memory out of `vm_physmem` before
    /// the page system is up, mapped and zeroed, and tells the caller the kernel virtual space
    /// that is still free (`start`, `end`).
    ///
    /// # Safety
    ///
    /// Only before `uvm_page_init` has run (`uvm.page_init_done` is false), on the boot CPU.
    unsafe fn pmap_steal_memory(
        size: Vsize,
        start: Option<&mut Vaddr>,
        end: Option<&mut Vaddr>,
    ) -> Vaddr;

    /// `pmap_virtual_space`: the kernel virtual range still free, on machines without
    /// `PMAP_STEAL_MEMORY`.
    fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr);

    /// `pmap_kenter_pa`: maps `pa` at `va` in the kernel pmap, wired, with `prot`.
    ///
    /// # Safety
    ///
    /// `va` must be kernel virtual space the caller owns and `pa` a page it may map there.
    unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt);

    /// `pmap_kremove`: removes `len` bytes of mappings made by `pmap_kenter_pa` from `va`.
    ///
    /// # Safety
    ///
    /// The range must have been mapped by `pmap_kenter_pa` and nothing may use it afterwards.
    unsafe fn pmap_kremove(va: Vaddr, len: Vsize);

    /// `pmap_extract`: the physical address `va` maps to in `pmap`, if any.
    fn pmap_extract(pmap: &Self::Pmap, va: Vaddr) -> Option<Paddr>;

    /// `pmap_update`: makes pending mapping changes visible.
    fn pmap_update(pmap: &Self::Pmap);

    /// `pmap_activate(p)`: `p`'s address space is in use by one more thread; switched in now
    /// when `p` is the running thread.
    fn pmap_activate(p: &Proc);

    /// `pmap_deactivate(p)`: the inverse, at exit.
    fn pmap_deactivate(p: &Proc);

    /// `pmap_purge(p)` (`__HAVE_PMAP_PURGE`): the last thread of a dying process is about to
    /// tear its address space down (`uvm_purge`). arm64 drops the process's ASID first;
    /// a machine without `__HAVE_PMAP_PURGE` (amd64, host) does nothing.
    fn pmap_purge(p: &Proc);

    /// `pmap_growkernel`: grows the kernel page tables to cover `maxkvaddr`; returns how far
    /// they now reach.
    fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr;

    /// `pmap_init`: the pmap module's own initialisation, once the page system is up.
    fn pmap_init();

    /// `pmap_map_direct`: the direct-map address of a page (`__HAVE_PMAP_DIRECT`).
    fn pmap_map_direct(pg: &VmPage) -> Vaddr;

    /// `pmap_unmap_direct`: the page behind a direct-map address.
    fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage>;
}

/// `pmap_kernel()` on the selected machine.
pub fn pmap_kernel() -> &'static <Machine as Pmap>::Pmap {
    Machine::pmap_kernel()
}

/// `pmap_create` on the selected machine.
pub fn pmap_create() -> &'static MachinePmap {
    Machine::pmap_create()
}

/// `pmap_destroy` on the selected machine.
pub fn pmap_destroy(pmap: &'static MachinePmap) {
    Machine::pmap_destroy(pmap)
}

/// `pmap_reference` on the selected machine.
pub fn pmap_reference(pmap: &MachinePmap) {
    Machine::pmap_reference(pmap)
}

/// `pmap_enter` on the selected machine.
pub fn pmap_enter(
    pmap: &MachinePmap,
    va: Vaddr,
    pa: Paddr,
    prot: VmProt,
    flags: i32,
) -> Result<(), Errno> {
    Machine::pmap_enter(pmap, va, pa, prot, flags)
}

/// `pmap_remove` on the selected machine.
pub fn pmap_remove(pmap: &MachinePmap, sva: Vaddr, eva: Vaddr) {
    Machine::pmap_remove(pmap, sva, eva)
}

/// `pmap_protect` on the selected machine.
pub fn pmap_protect(pmap: &MachinePmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
    Machine::pmap_protect(pmap, sva, eva, prot)
}

/// `pmap_wired_count` on the selected machine.
pub fn pmap_wired_count(pmap: &MachinePmap) -> i64 {
    Machine::pmap_wired_count(pmap)
}

/// `pmap_resident_count` on the selected machine.
pub fn pmap_resident_count(pmap: &MachinePmap) -> i64 {
    Machine::pmap_resident_count(pmap)
}

/// `pmap_unwire` on the selected machine.
pub fn pmap_unwire(pmap: &MachinePmap, va: Vaddr) {
    Machine::pmap_unwire(pmap, va)
}

/// `pmap_page_protect` on the selected machine.
pub fn pmap_page_protect(pg: &VmPage, prot: VmProt) {
    Machine::pmap_page_protect(pg, prot)
}

/// `pmap_clear_modify` on the selected machine.
pub fn pmap_clear_modify(pg: &VmPage) -> bool {
    Machine::pmap_clear_modify(pg)
}

/// `pmap_clear_reference` on the selected machine.
pub fn pmap_clear_reference(pg: &VmPage) -> bool {
    Machine::pmap_clear_reference(pg)
}

/// `pmap_is_modified` on the selected machine.
pub fn pmap_is_modified(pg: &VmPage) -> bool {
    Machine::pmap_is_modified(pg)
}

/// `pmap_remove_holes` on the selected machine.
pub fn pmap_remove_holes(vm: &Vmspace) {
    Machine::pmap_remove_holes(vm)
}

/// `pmap_proc_iflush` on the selected machine.
pub fn pmap_proc_iflush(pr: &Process, va: Vaddr, len: Vsize) {
    Machine::pmap_proc_iflush(pr, va, len)
}

/// `pmap_zero_page` on the selected machine.
pub fn pmap_zero_page(pg: &VmPage) {
    Machine::pmap_zero_page(pg)
}

/// `pmap_copy_page` on the selected machine.
pub fn pmap_copy_page(src: &VmPage, dst: &VmPage) {
    Machine::pmap_copy_page(src, dst)
}

/// `pmap_steal_memory` on the selected machine.
///
/// # Safety
///
/// As for [`Pmap::pmap_steal_memory`].
pub unsafe fn pmap_steal_memory(
    size: Vsize,
    start: Option<&mut Vaddr>,
    end: Option<&mut Vaddr>,
) -> Vaddr {
    // SAFETY: forwarded.
    unsafe { Machine::pmap_steal_memory(size, start, end) }
}

/// `pmap_virtual_space` on the selected machine.
pub fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
    Machine::pmap_virtual_space(start, end)
}

/// `pmap_kenter_pa` on the selected machine.
///
/// # Safety
///
/// As for [`Pmap::pmap_kenter_pa`].
pub unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt) {
    // SAFETY: forwarded.
    unsafe { Machine::pmap_kenter_pa(va, pa, prot) }
}

/// `pmap_kremove` on the selected machine.
///
/// # Safety
///
/// As for [`Pmap::pmap_kremove`].
pub unsafe fn pmap_kremove(va: Vaddr, len: Vsize) {
    // SAFETY: forwarded.
    unsafe { Machine::pmap_kremove(va, len) }
}

/// `pmap_extract` on the selected machine.
pub fn pmap_extract(pmap: &<Machine as Pmap>::Pmap, va: Vaddr) -> Option<Paddr> {
    Machine::pmap_extract(pmap, va)
}

/// `pmap_activate` on the selected machine.
pub fn pmap_activate(p: &Proc) {
    Machine::pmap_activate(p)
}

/// `pmap_deactivate` on the selected machine.
pub fn pmap_deactivate(p: &Proc) {
    Machine::pmap_deactivate(p)
}

/// `pmap_purge` on the selected machine (`__HAVE_PMAP_PURGE`; nothing where it is not).
pub fn pmap_purge(p: &Proc) {
    Machine::pmap_purge(p)
}

/// `pmap_update` on the selected machine.
pub fn pmap_update(pmap: &<Machine as Pmap>::Pmap) {
    Machine::pmap_update(pmap)
}

/// `pmap_growkernel` on the selected machine.
pub fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
    Machine::pmap_growkernel(maxkvaddr)
}

/// `pmap_init` on the selected machine.
pub fn pmap_init() {
    Machine::pmap_init()
}

/// `pmap_map_direct` on the selected machine.
pub fn pmap_map_direct(pg: &VmPage) -> Vaddr {
    Machine::pmap_map_direct(pg)
}

/// `pmap_unmap_direct` on the selected machine.
pub fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage> {
    Machine::pmap_unmap_direct(va)
}
/* </CODE> */
