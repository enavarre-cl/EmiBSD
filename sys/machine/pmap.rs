//! `<machine/pmap.h>` and the machine-dependent half of `<uvm/uvm_pmap.h>` as a trait: the
//! physical map, what `uvm` asks the MMU code to do.
//!
//! Milestone M3 needs the direct map, boot-time memory stealing, page zeroing and the kernel
//! mapping entry points (`pmap_kenter_pa`, `pmap_kremove`, `pmap_extract`). The user-space side
//! (`pmap_enter`, `pmap_remove`, `pmap_create`, activation, protection and attribute bits)
//! arrives with M6.

use crate::machine::Machine;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt};
use crate::uvm::uvm_page::VmPage;

/// `struct vm_page_md` of the selected architecture: the pmap's per-page data inside
/// `struct vm_page`.
pub type VmPageMd = <Machine as Pmap>::VmPageMd;

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

    /// `pmap_kernel()`: the kernel's pmap.
    fn pmap_kernel() -> &'static Self::Pmap;

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
