/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * from: Id: uvm_io.c,v 1.1.2.2 1997/12/30 12:02:00 mrg Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `uvm_io.c`: uvm i/o ops, reading another address space through `kernel_map`.
//!
//! Upstream: sys/uvm/uvm_io.c @ 3ce1f3f79392
//!
//! `uvm_io` is what `kern.proc_args` (`sysctl_proc_args`) reads a victim's `ps_strings`
//! and argument strings with: each chunk of the victim's map is extracted into `kernel_map`
//! (`uvm_map_extract`), copied with `uiomove` (faulting the pages in as the kernel touches
//! them), and unmapped again.
//!
//! ## Deviations
//! - `vm_map_t` is `&VmMap`, the `struct uio *` a `&mut Uio`, the `int` error a
//!   `Result`. `uvm_unmap_remove` cannot fail on `kernel_map` (no immutable entries are
//!   checked), so its result is not consulted, as the C ignores it.

use crate::kern::kern_subr::uiomove;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::param::{MAXBSIZE, PAGE_MASK, PAGE_SIZE};
use crate::sys::uio::Uio;
use crate::uvm::uvm_amap::AMAP_REFALL;
use crate::uvm::uvm_extern::UVM_IO_FIXPROT;
use crate::uvm::uvm_km::kernel_map;
use crate::uvm::uvm_map::{
    UVM_EXTRACT_FIXPROT, UvmMapDeadq, VmMap, uvm_map_extract, uvm_unmap_detach, uvm_unmap_remove,
    vm_map_lock, vm_map_unlock,
};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `uvm_io`: perform I/O on a map. `uio_offset` is the address in `map`, `uio_resid` the
/// length; the transfer stops at `VM_MAXUSER_ADDRESS` (an EOF truncation).
///
/// The caller must hold a reference to `map` so that it does not go away meanwhile.
pub fn uvm_io(map: &VmMap, uio: &mut Uio<'_>, flags: i32) -> Result<(), Errno> {
    let maxuser = <Machine as VmParam>::VM_MAXUSER_ADDRESS;

    // Step 0: sanity checks and set up for the copy loop. Start with a large chunk; if
    // kernel_map has no room for it, it is halved.
    if uio.uio_resid == 0 {
        return Ok(());
    }
    let mut togo = uio.uio_resid;

    let mut baseva = uio.uio_offset as usize;
    let Some(endva) = baseva.checked_add(togo - 1) else {
        return Err(Errno::EIO); // wrap around
    };
    if baseva >= maxuser {
        return Err(Errno::EIO);
    }
    if endva >= maxuser {
        togo -= endva - maxuser + 1; // EOF truncate
    }
    let mut pageoffset = baseva & PAGE_MASK;
    baseva = trunc_page(baseva);
    let mut chunksz = round_page(togo + pageoffset).min(MAXBSIZE);

    let extractflags = if flags & UVM_IO_FIXPROT != 0 {
        UVM_EXTRACT_FIXPROT
    } else {
        0
    };

    // Step 1: the main loop, while there is data to move.
    while togo > 0 {
        // Step 2: extract the mappings from the map into kernel_map.
        let kva = match uvm_map_extract(map, baseva, chunksz, extractflags) {
            Ok(kva) => kva,
            Err(Errno::ENOMEM) if chunksz > PAGE_SIZE => {
                // Retry with a smaller chunk (the C's `continue` also clears pageoffset).
                chunksz = trunc_page(chunksz / 2).max(PAGE_SIZE);
                pageoffset = 0;
                continue;
            }
            Err(e) => return Err(e),
        };

        // Step 3: move a chunk of data.
        let sz = (chunksz - pageoffset).min(togo);
        // SAFETY: `kva .. kva + chunksz` was just entered into kernel_map by
        // uvm_map_extract and stays mapped until the unmap below; `pageoffset + sz` is at
        // most chunksz. The pages are faulted in through kernel_map as uiomove touches them,
        // and nothing else refers to this fresh range.
        let chunk = unsafe { core::slice::from_raw_parts_mut((kva + pageoffset) as *mut u8, sz) };
        let moved = uiomove(chunk, uio);
        togo -= sz;
        baseva += chunksz;

        // Step 4: unmap the area of kernel memory.
        let kmap = kernel_map();
        let dead_entries = UvmMapDeadq::new();
        vm_map_lock(kmap);
        let _ = uvm_unmap_remove(kmap, kva, kva + chunksz, &dead_entries, false, true, false);
        vm_map_unlock(kmap);
        uvm_unmap_detach(&dead_entries, AMAP_REFALL);

        moved?;
        pageoffset = 0;
    }

    Ok(())
}
/* </CODE> */
