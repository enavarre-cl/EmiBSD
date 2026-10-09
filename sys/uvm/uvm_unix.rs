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
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993 The Regents of the University of California.
 * Copyright (c) 1988 University of Utah.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 * from: Utah $Hdr: vm_unix.c 1.1 89/11/07$
 *      @(#)vm_unix.c   8.1 (Berkeley) 6/11/93
 * from: Id: uvm_unix.c,v 1.1.2.2 1997/08/25 18:52:30 chuck Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `uvm_unix.c`: traditional sbrk/grow interface to vm.
//!
//! Upstream: sys/uvm/uvm_unix.c @ 3ce1f3f79392
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `sys_obreak` prints its failure with `printf`: there is no controlling terminal for
//!   `uprintf` yet.
//! - The coredump walk is a closure-free port: the callbacks are `fn` pointers returning
//!   `Result`, the `int` error of the C.

use core::ffi::c_void;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::mman::{MADV_NORMAL, MAP_INHERIT_COPY, PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::param::PAGE_SHIFT;
use crate::sys::proc::Proc;
use crate::sys::resource::{RLIMIT_DATA, RLIMIT_STACK};
use crate::sys::resourcevar::lim_cur;
use crate::sys::syscallargs::SysObreakArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::tree::RbtHead;
use crate::sys::types::Register;
use crate::uvm::uvm::{uvm_et_isconceal, uvm_et_issubmap};
use crate::uvm::uvm_amap::amap_lookups;
use crate::uvm::uvm_anon::VmAnon;
use crate::uvm::uvm_extern::{
    UVM_FLAG_COPYONW, UVM_FLAG_FIXED, UVM_UNKNOWN_OFFSET, UvmCoredumpSetupCb, UvmCoredumpWalkCb,
    VmProt, uvm_mapflag,
};
use crate::uvm::uvm_map::{UvmMapAddr, VmMapEntry, uvm_map, uvm_unmap, vm_map_lock, vm_map_unlock};
use crate::uvm::uvm_object::{uvm_obj_is_device, uvm_obj_is_vnode};
use crate::uvm::uvm_param::{atop, ptoa, round_page};

/// `sys_obreak`: set break.
pub fn sys_obreak(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysObreakArgs = sysargs(v);
    let vm = p.vmspace();

    let base = vm.vm_daddr.get();
    let new = round_page(uap.nsize.get() as usize);
    if new < base || (new - base) as u64 > lim_cur(RLIMIT_DATA) {
        return Err(Errno::ENOMEM);
    }

    let mut old = round_page(base + ptoa(vm.vm_dsize.get() as usize));

    if new == old {
        return Ok(());
    }

    // grow or shrink?
    if new > old {
        let grow = new - old;
        if let Err(error) = uvm_map(
            &vm.vm_map,
            &mut old,
            grow,
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            uvm_mapflag(
                PROT_READ | PROT_WRITE,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_INHERIT_COPY,
                MADV_NORMAL,
                UVM_FLAG_FIXED | UVM_FLAG_COPYONW,
            ),
        ) {
            // uprintf: see the module's deviations.
            crate::kprintf!("sbrk: grow {} failed, error = {}\n", grow, error as i32);
            return Err(Errno::ENOMEM);
        }
        vm.vm_dsize.set(vm.vm_dsize.get() + atop(grow) as i32);
    } else {
        uvm_unmap(&vm.vm_map, new, old);
        vm.vm_dsize.set(vm.vm_dsize.get() - atop(old - new) as i32);
    }

    Ok(())
}

/// `uvm_grow`: enlarge the "stack segment" to include sp.
pub fn uvm_grow(p: &Proc, sp: usize) {
    let vm = p.vmspace();
    let map = &vm.vm_map;

    // For user defined stacks (from sendsig).
    if sp < vm.vm_maxsaddr.get() {
        return;
    }
    // MACHINE_STACK_GROWS_UP: neither amd64 nor arm64.

    vm_map_lock(map);

    // For common case of already allocated (from trap).
    if sp >= vm.vm_minsaddr.get() - ptoa(vm.vm_ssize.get() as usize) {
        vm_map_unlock(map);
        return;
    }

    // Really need to check vs limit and increment stack size if ok.
    let si = atop(vm.vm_minsaddr.get() - sp) as i64 - vm.vm_ssize.get() as i64;
    if vm.vm_ssize.get() as i64 + si <= atop(lim_cur(RLIMIT_STACK) as usize) as i64 {
        vm.vm_ssize.set((vm.vm_ssize.get() as i64 + si) as i32);
    }
    // out:
    vm_map_unlock(map);
}

/// `WALK_CHUNK`.
const WALK_CHUNK: usize = 32;

/// `uvm_coredump_walk_amap`.
///
/// Not all the pages in an amap may be present. When dumping core, we don't want to force
/// all the pages to be present: it's a waste of time and memory when we already know what
/// they contain (zeros) and the ELF format at least can adequately represent them as a
/// segment with memory size larger than its file size.
///
/// So, we walk the amap with calls to `amap_lookups()` and scan the resulting pointers to
/// find ranges of zero or more present pages followed by at least one absent page or the end
/// of the amap. When then pass that range to the walk callback with 'start' pointing to the
/// start of the present range, 'realend' pointing to the first absent page (or the end of
/// the entry), and 'end' pointing to the page past the last absent page (or the end of the
/// entry).
///
/// Note that if the first page of the amap is empty then the callback must be invoked with
/// 'start' == 'realend' so it can present that first range of absent pages.
pub fn uvm_coredump_walk_amap(
    entry: &VmMapEntry,
    nsegmentp: &mut i32,
    walk: UvmCoredumpWalkCb,
    cookie: *mut c_void,
) -> Result<(), Errno> {
    let mut anons: [*const VmAnon; WALK_CHUNK] = [ptr::null(); WALK_CHUNK];

    let prot = entry.protection.get();
    let mut nsegment = *nsegmentp;
    let mut start = entry.start.get();
    let entry_end = entry
        .end
        .get()
        .min(<Machine as VmParam>::VM_MAXUSER_ADDRESS);

    let mut absent = false;
    let mut realend = start;
    let mut pos = start;
    while pos < entry_end {
        let npages = ((entry_end - pos) >> PAGE_SHIFT).min(WALK_CHUNK);
        amap_lookups(&entry.aref, pos - entry.start.get(), &mut anons[..npages]);
        for (i, anon) in anons[..npages].iter().enumerate() {
            if anon.is_null() == absent {
                continue;
            }
            if !absent {
                // going from present to absent: set realend
                realend = pos + (i << PAGE_SHIFT);
                absent = true;
                continue;
            }

            // going from absent to present: invoke callback
            let end = pos + (i << PAGE_SHIFT);
            if start != end {
                walk(start, realend, end, prot, false, nsegment, cookie)?;
                nsegment += 1;
            }
            start = end;
            realend = end;
            absent = false;
        }
        pos += npages << PAGE_SHIFT;
    }

    if !absent {
        realend = entry_end;
    }
    let error = walk(start, realend, entry_end, prot, false, nsegment, cookie);
    *nsegmentp = nsegment + 1;
    error
}

/// `uvm_should_coredump`: common logic for whether a map entry should be included in a
/// coredump.
fn uvm_should_coredump(p: &Proc, entry: &VmMapEntry) -> bool {
    let pr = p.process();
    let prot = entry.protection.get();
    if prot & PROT_WRITE == 0
        && entry.aref.ar_amap.get().is_null()
        && entry.start.get() != pr.ps_sigcode.get()
        && entry.start.get() != pr.ps_timekeep.get()
    {
        return false;
    }

    // Skip ranges marked as unreadable, as uiomove(UIO_USERSPACE) will fail on them. Maybe
    // this really should be a test of entry->max_protection, but doing
    // uvm_map_extract(UVM_EXTRACT_FIXPROT) on each such page would suck.
    if prot & PROT_READ == 0 && entry.start.get() != pr.ps_sigcode.get() {
        return false;
    }

    // Skip ranges excluded from coredumps.
    if uvm_et_isconceal(entry) {
        return false;
    }

    // Don't dump mmaped devices.
    if let Some(uobj) = entry.uvm_obj()
        && uvm_obj_is_device(uobj)
    {
        return false;
    }

    if entry.start.get() >= <Machine as VmParam>::VM_MAXUSER_ADDRESS {
        return false;
    }

    true
}

/// `noop`: do nothing callback for `uvm_coredump_walk_amap()`.
fn noop(
    _start: usize,
    _realend: usize,
    _end: usize,
    _prot: VmProt,
    _isvnode: bool,
    _nsegment: i32,
    _cookie: *mut c_void,
) -> Result<(), Errno> {
    Ok(())
}

/// `uvm_coredump_walkmap`: walk the VA space for a process to identify what to write to a
/// coredump. First the number of contiguous ranges is counted, then the 'setup' callback is
/// invoked to prepare for actually recording the ranges, then the VA is walked again,
/// invoking the 'walk' callback for each range. The number of ranges walked is guaranteed to
/// match the count seen by the 'setup' callback.
pub fn uvm_coredump_walkmap(
    p: &Proc,
    setup: UvmCoredumpSetupCb,
    walk: UvmCoredumpWalkCb,
    cookie: *mut c_void,
) -> Result<(), Errno> {
    let vm = p.vmspace();
    let map = &vm.vm_map;
    let mut refed_amaps = 0;

    // Walk the map once to count the segments. If an amap is referenced more than once than
    // take *another* reference and treat the amap as exactly one segment instead of checking
    // page presence inside it. On the second pass we'll recognize which amaps we did that for
    // by the ref count being >1...and decrement it then.
    let mut nsegment = 0;
    let mut it = map.addr.min();
    while let Some(entry) = it {
        it = RbtHead::<UvmMapAddr>::next(entry);
        // should never happen for a user process
        if uvm_et_issubmap(entry) {
            panic(format_args!(
                "uvm_coredump_walkmap: user process with submap?"
            ));
        }

        if !uvm_should_coredump(p, entry) {
            continue;
        }

        if let Some(amap) = entry.aref.amap() {
            if amap.am_ref.get() == 1 {
                let _ = uvm_coredump_walk_amap(entry, &mut nsegment, noop, cookie);
                continue;
            }

            // Multiple refs currently, so take another and treat it as a single segment
            amap.am_ref.set(amap.am_ref.get() + 1);
            refed_amaps += 1;
        }

        nsegment += 1;
    }

    // Okay, we have a count in nsegment. Prepare to walk it again, then invoke the setup
    // callback.
    let mut entry = map.addr.min();
    let mut error = setup(nsegment, cookie);
    if error.is_ok() {
        // Setup went okay, so do the second walk, invoking the walk callback on the counted
        // segments and cleaning up references as we go.
        nsegment = 0;
        while let Some(e) = entry {
            if !uvm_should_coredump(p, e) {
                entry = RbtHead::<UvmMapAddr>::next(e);
                continue;
            }

            if let Some(amap) = e.aref.amap()
                && amap.am_ref.get() == 1
            {
                error = uvm_coredump_walk_amap(e, &mut nsegment, walk, cookie);
                if error.is_err() {
                    break;
                }
                entry = RbtHead::<UvmMapAddr>::next(e);
                continue;
            }

            let end = e.end.get().min(<Machine as VmParam>::VM_MAXUSER_ADDRESS);

            let isvnode = e.uvm_obj().is_some_and(uvm_obj_is_vnode);
            error = walk(
                e.start.get(),
                end,
                end,
                e.protection.get(),
                isvnode,
                nsegment,
                cookie,
            );
            if error.is_err() {
                break;
            }
            nsegment += 1;

            if let Some(amap) = e.aref.amap()
                && amap.am_ref.get() > 1
            {
                // multiple refs, so we need to drop one
                amap.am_ref.set(amap.am_ref.get() - 1);
                refed_amaps -= 1;
            }
            entry = RbtHead::<UvmMapAddr>::next(e);
        }
    }

    if error.is_err() {
        // cleanup: clean up the extra references from where we left off
        if refed_amaps > 0 {
            while let Some(e) = entry {
                entry = RbtHead::<UvmMapAddr>::next(e);
                let Some(amap) = e.aref.amap() else {
                    continue;
                };
                if amap.am_ref.get() == 1 {
                    continue;
                }
                if !uvm_should_coredump(p, e) {
                    continue;
                }
                amap.am_ref.set(amap.am_ref.get() - 1);
                let was = refed_amaps;
                refed_amaps -= 1;
                if was == 0 {
                    break;
                }
            }
        }
    }

    error
}
/* </CODE> */
