//! Boot-time self-tests under feature `qemu`: a project helper, not OpenBSD code.
//!
//! Each test exercises a subsystem right after `main()` brought it up and prints one line that
//! `xtask smoke` asserts (`selftest: <what> ok`). They stand in for the user-space programs
//! OpenBSD would run until there is a user space; they are compiled only with feature `qemu`.

use core::ptr;

use crate::kprintf;
use crate::machine::pmap::{
    pmap_extract, pmap_growkernel, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_map_direct,
    pmap_update,
};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::PAGE_SIZE;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::UVM_PGA_ZERO;
use crate::uvm::uvm_km::kernel_map_min;
use crate::uvm::uvm_page::{uvm_pagealloc, uvm_pagefree, vm_page_to_phys};

/// A value that is neither all zeros nor all ones.
const PATTERN: u64 = 0x5a5a_c3c3_0f0f_a5a5;

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
