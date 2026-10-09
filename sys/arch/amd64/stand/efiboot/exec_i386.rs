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
 * Copyright (c) 1997-1998 Michael Shalayeff
 * Copyright (c) 1997 Tobias Weingartner
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `run_loadfile()`: hand the loaded kernel its boot arguments, leave the boot services,
//! move the kernel to its physical address and enter it in 32-bit protected mode through
//! `run_i386`; and the CPU microcode it may load first.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/exec_i386.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `run_loadfile` takes boot(8)'s command state (`cmd.bootdev`, which `ucode_load` reads
//!   from the C's global `cmd`).
//! - `SOFTRAID`'s `BOOTARG_BOOTSR` and `sr_clear_keys()` are feature `softraid`, not ported
//!   (no softraid volume can be the boot device without it).
//! - `marks` and the move are done with the addresses as integers (`u64`), as the C's
//!   `u_long` arithmetic; `delta` is the two's complement of `efi_loadaddr`.

use alloc::vec;
use core::arch::asm;
use core::sync::atomic::{AtomicPtr, Ordering};

use boot::bootarg::{BOOTARG_APIVER, addbootarg, makebootargs32};
use boot::cmd::{CmdState, cstr};
use boot::vars::DB_CONSOLE;
use efi::include::efi::*;
use libsa::cread::{close, open, read};
use libsa::fstat::fstat;
use libsa::hdr::stat::Stat;
use libsa::loadfile::{MARK_END, MARK_ENTRY, MARK_MAX, MARK_START};
use libsa::printf;
use libsa::printf::Str;
use libsa::snprintf;

use crate::biosvar::{
    BOOTARG_BOOTDUID, BOOTARG_BOOTMAC, BOOTARG_DDB, BOOTARG_LEN, BOOTARG_UCODE, BiosBootduid,
    BiosDdb, BiosUcode,
};
use crate::diskprobe::BOOTDEV_DIP;
use crate::efiboot::{EFI_LOADADDR, RUN_I386, bs, efi_cleanup, efi_makebootargs, efi_setconsdev};
use crate::libsa_md::{cpuid, cpuid_leaf};
use crate::memprobe::{CNVMEM, EXTMEM, mem_pass};

/// `bootmac`: the MAC address of the PXE boot interface, null when booted from a disk.
pub static BOOTMAC: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

/// `CPUIDEAX_SEV` (`<machine/specialreg.h>`).
const CPUIDEAX_SEV: u32 = 1 << 1;
/// `MSR_SEV_STATUS`.
const MSR_SEV_STATUS: u32 = 0xc001_0131;
/// `SEV_STAT_ENABLED`.
const SEV_STAT_ENABLED: u32 = 0x0000_0001;
/// `CR0_PG`.
const CR0_PG: u64 = 0x8000_0000;
/// `PG_RW`, `PG_PS`, `PG_FRAME` (`<machine/pte.h>`).
const PG_RW: u64 = 0x2;
const PG_PS: u64 = 0x80;
const PG_FRAME: u64 = 0x000f_ffff_ffff_f000;
/// `L4_SHIFT`.. `L1_SHIFT` and their masks.
const L4_SHIFT: u64 = 39;
const L3_SHIFT: u64 = 30;
const L2_SHIFT: u64 = 21;
const L1_SHIFT: u64 = 12;
const L4_MASK: u64 = 0x0000_ff80_0000_0000;
const L3_MASK: u64 = 0x0000_007f_c000_0000;
const L2_MASK: u64 = 0x0000_0000_3fe0_0000;
const L1_MASK: u64 = 0x0000_0000_001f_f000;
/// `PAGE_SIZE`, `PAGE_MASK`.
const PAGE_SIZE: u64 = 4096;

/// `run_i386`'s prototype: `(start, entry, howto, bootdev, apiver, end, extmem, cnvmem, ac,
/// av)`, the System V calling convention, not returning.
type RunI386 = unsafe extern "C" fn(u64, u64, i32, i32, i32, i32, i32, i32, i32, i32) -> !;

/// `run_loadfile(marks, howto)`.
pub fn run_loadfile(cmd: &mut CmdState, marks: &mut [u64; MARK_MAX], howto: i32) {
    let dip = BOOTDEV_DIP.load(Ordering::Relaxed);
    let (bootdev, duid) = if dip.is_null() {
        (0, [0u8; 8])
    } else {
        // SAFETY: bootdev_dip points into the disk list (efiopen or efi_pxeprobe set it).
        unsafe { ((*dip).bootdev, (*dip).disklabel.d_uid) }
    };
    let mut ac = BOOTARG_LEN;
    let av = vec![0u8; ac].leak();

    efi_makebootargs();
    efi_setconsdev();
    let delta = EFI_LOADADDR.load(Ordering::Relaxed).wrapping_neg();

    let mac = BOOTMAC.load(Ordering::Relaxed);
    if !mac.is_null() {
        // SAFETY: bootmac points at efi_pxeprobe's 16-byte hardware address.
        addbootarg(BOOTARG_BOOTMAC, unsafe {
            core::slice::from_raw_parts(mac, 6)
        });
    }

    let db_console = DB_CONSOLE.load(Ordering::Relaxed);
    if db_console != -1 {
        addbootarg(BOOTARG_DDB, BiosDdb { db_console }.as_bytes());
    }

    addbootarg(BOOTARG_BOOTDUID, BiosBootduid { duid }.as_bytes());

    ucode_load(cmd);

    let entry = (marks[MARK_ENTRY] & 0x0fff_ffff).wrapping_add(delta);

    printf!("entry point at {:#x}\n", entry);

    // Sync the memory map and call ExitBootServices()
    efi_cleanup();

    // Pass memory map to the kernel
    mem_pass();

    // This code may be used both for 64bit and 32bit. Make sure the bootarg is always
    // 32bit, even on amd64.
    ac = makebootargs32(av);

    // Move the loaded kernel image to the usual place after calling ExitBootServices().
    let (start, end) = (marks[MARK_START], marks[MARK_END]);
    protect_writeable(start.wrapping_add(delta), end - start);
    // SAFETY: the kernel was loaded at `start..end` (efi_loadaddr's pages) and goes to its
    // physical address; the boot services are gone, the memory is ours. Both ranges may
    // overlap: `copy` is memmove.
    unsafe {
        core::ptr::copy(
            start as usize as *const u8,
            start.wrapping_add(delta) as usize as *mut u8,
            (end - start) as usize,
        );
    }
    for m in marks.iter_mut() {
        *m = m.wrapping_add(delta);
    }

    let run = RUN_I386.load(Ordering::Relaxed);
    // SAFETY: `run` is the copy of run_i386_start efi_main made on the heap; it takes the
    // System V arguments of `RunI386` and enters the kernel at `entry` in 32-bit mode.
    unsafe {
        let run_i386: RunI386 = core::mem::transmute::<*mut u8, RunI386>(run);
        run_i386(
            run as u64,
            entry,
            howto,
            bootdev,
            BOOTARG_APIVER,
            marks[MARK_END] as i32,
            EXTMEM.load(Ordering::Relaxed) as i32,
            CNVMEM.load(Ordering::Relaxed) as i32,
            ac as i32,
            av.as_ptr() as usize as i32,
        )
    }
}

/// `ucode_load()`: the CPU's microcode update from `/etc/firmware`, if there is one.
fn ucode_load(cmd: &CmdState) {
    let (_, b, c, d) = cpuid(0);
    let mut vendor = [0u8; 12];
    vendor[..4].copy_from_slice(&b.to_ne_bytes());
    vendor[4..8].copy_from_slice(&d.to_ne_bytes());
    vendor[8..].copy_from_slice(&c.to_ne_bytes());
    let intel = &vendor == b"GenuineIntel";
    let amd = &vendor == b"AuthenticAMD";
    if !intel && !amd {
        return;
    }

    let (signature, _, _, _) = cpuid(1);
    let mut family = (signature >> 8) & 0x0f;
    let mut model = (signature >> 4) & 0x0f;
    if family == 0x6 || family == 0xf {
        family += (signature >> 20) & 0xff;
        model += ((signature >> 16) & 0x0f) << 4;
    }
    let stepping = signature & 0x0f;

    let mut path = [0u8; 128];
    let bootdev = Str(cstr(&cmd.bootdev));
    if intel {
        snprintf!(
            &mut path,
            "{}:/etc/firmware/intel/{:02x}-{:02x}-{:02x}",
            bootdev,
            family,
            model,
            stepping
        );
    } else if family < 0x10 {
        return;
    } else if family <= 0x14 {
        snprintf!(&mut path, "{}:/etc/firmware/amd/microcode_amd.bin", bootdev);
    } else {
        snprintf!(
            &mut path,
            "{}:/etc/firmware/amd/microcode_amd_fam{:02x}h.bin",
            bootdev,
            family
        );
    }

    let Ok(fd) = open(cstr(&path), 0) else {
        return;
    };

    let mut sb = Stat::default();
    if fstat(fd, &mut sb).is_err() {
        return;
    }

    let buflen = sb.st_size as usize;
    let mut addr: EfiPhysicalAddress = 16 * 1024 * 1024;
    // SAFETY: a boot service with a valid pointer.
    let status = unsafe {
        (bs().AllocatePages)(
            AllocateMaxAddress,
            EfiLoaderData,
            efi_size_to_pages(buflen),
            &mut addr,
        )
    };
    if status != EFI_SUCCESS {
        printf!("cannot allocate memory for ucode\n");
        return;
    }
    // SAFETY: the pages just allocated, `buflen` bytes and more.
    let buf = unsafe { core::slice::from_raw_parts_mut(addr as usize as *mut u8, buflen) };

    if read(fd, buf) != Ok(buflen) {
        let _ = close(fd);
        return;
    }

    let uc = BiosUcode {
        uc_addr: addr,
        uc_size: buflen as u64,
    };
    addbootarg(BOOTARG_UCODE, uc.as_bytes());

    let _ = close(fd);
}

/// `detect_sev()`: 0 when AMD SEV is enabled (`-ENODEV` otherwise).
fn detect_sev() -> i32 {
    // check whether we have SEV feature cpuid leaf
    let (max_ex_leaf, b, c, d) = cpuid(0x8000_0000);
    let mut vendor = [0u8; 12];
    vendor[..4].copy_from_slice(&b.to_ne_bytes());
    vendor[4..8].copy_from_slice(&d.to_ne_bytes());
    vendor[8..].copy_from_slice(&c.to_ne_bytes());
    if &vendor != b"AuthenticAMD" || max_ex_leaf < 0x8000_001f {
        return -19;
    }

    let (sev_feat, _, _, _) = cpuid_leaf(0x8000_001f, 0);
    // check that SEV is supported
    if (sev_feat & CPUIDEAX_SEV) == 0 {
        return -19;
    }

    let (sev_status, _dummy): (u32, u32);
    // SAFETY: the CPU advertised SEV, whose status MSR exists; rdmsr only reads it.
    unsafe {
        asm!("rdmsr", in("ecx") MSR_SEV_STATUS, out("eax") sev_status, out("edx") _dummy,
            options(nomem, nostack, preserves_flags));
    }
    // check whether SEV is enabled
    if (sev_status & SEV_STAT_ENABLED) == 0 {
        return -19;
    }

    0
}

/// `protect_writeable(addr, len)`: under SEV the firmware maps memory read-only; make the
/// kernel's destination writable in the current page tables.
fn protect_writeable(addr: u64, len: u64) {
    let end = addr + len;

    if detect_sev() == 0 {
        return;
    }

    let cr0: u64;
    // SAFETY: reading %cr0 has no side effect.
    unsafe { asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags)) };
    if (cr0 & CR0_PG) == 0 {
        return;
    }
    let cr3: u64;
    // SAFETY: reading %cr3 has no side effect.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };

    // SAFETY: the firmware's page tables, identity mapped, as the C walks them.
    unsafe {
        let mut a = addr & !(PAGE_SIZE - 1);
        while a < end {
            let mut p = cr3 as usize as *mut u64;
            let levels = [
                (L4_MASK, L4_SHIFT),
                (L3_MASK, L3_SHIFT),
                (L2_MASK, L2_SHIFT),
                (L1_MASK, L1_SHIFT),
            ];
            for (k, (mask, shift)) in levels.iter().enumerate() {
                let e = p.add(((a & mask) >> shift) as usize);
                if (*e & PG_RW) == 0 {
                    *e |= PG_RW;
                }
                if k == 3 || (*e & PG_PS) != 0 {
                    break;
                }
                p = (*e & PG_FRAME) as usize as *mut u64;
            }
            a += PAGE_SIZE;
        }

        // tlb flush
        asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
    }
}
/* </CODE> */
