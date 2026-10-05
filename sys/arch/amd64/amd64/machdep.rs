/*	$OpenBSD: machdep.c,v 1.314 2026/09/28 14:14:03 deraadt Exp $	*/
/*	$NetBSD: machdep.c,v 1.3 2003/05/07 22:58:18 fvdl Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997, 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and by Jason R. Thorpe of the Numerical Aerospace
 * Simulation Facility, NASA Ames Research Center.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1982, 1987, 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)machdep.c	7.4 (Berkeley) 6/3/91
 */
/* </LICENSES> */

//! amd64 machine-dependent setup and shutdown: `arch/amd64/amd64/machdep.c`.
//!
//! Upstream: sys/arch/amd64/amd64/machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports what the console and a panic need: the first part of
//! `init_x86_64` (message buffer, console, `boot -d`), `boot`, `delay` and the globals they
//! use (`cpureset_delay`, `lid_action`, `waittime`); M3 adds the direct map, `pmap_bootstrap`
//! and the memory clusters; M4 adds the descriptor tables (`setgate`, `unsetgate`,
//! `setregion`, `set_mem_segment`, `set_sys_segment`), the IDT (`idt`, `idt_allocmap`,
//! `cpu_init_idt`, `idt_vec_alloc`, `idt_vec_alloc_range`, `idt_vec_set`, `idt_vec_free`),
//! `x86_64_proc0_tss_ldt_init`, `splassert_check` and the GDT/TSS/IDT part of `init_x86_64`,
//! which now ends as the C does: `intr_default_setup`, `softintr_init`, `splraise(IPL_IPI)`,
//! `intr_enable`. `cpu_reset`,
//! `dumpsys`, the bootinfo parsing and the sysctl tree arrive with M4-b to M6; `kern_sig.c`
//! brought `sendsig`, `sys_sigreturn`, `copyoutfpu`, `initialize_thread_xstate` and
//! `signotify`. M11a adds the `MULTIPROCESSOR` parts: `cpu_kick`, `cpu_unidle`'s IPI,
//! `need_resched`'s kick, `signotify`'s `cpu_kick` and `boot`'s `X86_IPI_HALT` broadcast.
//! M13 adds ACPI to `boot` (`acpi_softc->sc_state = ACPI_STATE_S5`, `acpi_powerdown`),
//! `cpu_reset` (`cpuresetfn`, the keyboard controller's reset line, the triple fault),
//! `pwr_action`, and `bios_efiinfo->config_acpi` as `BIOS_EFIINFO_CONFIG_ACPI`.
//!
//! ## Deviations
//! - `cpu_sysctl` (M13, `machine::cpu::cpu_sysctl`) is ported with these gaps: `CPU_BIOS` is
//!   `EOPNOTSUPP` as in the C without `BAPIV_VECTOR` (Limine passes no boot arguments, so
//!   `bios_sysctl`'s `BIOS_DEV`, `BIOS_DISKINFO` and `BIOS_CKSUMLEN` cannot be reached);
//!   `CPU_FORCEUKBD` is compiled out as in C without `pckbc(4)` (not ported); `allowaperture`
//!   is the `#else` (`APERTURE` is not configured); `cpu_sev_guestmode` (SEV probe),
//!   `amd64_has_xcrypt` (`via_nano_setup`) and `need_retpoline` (`codepatch_replace`) keep
//!   their initial values because the code that changes them is not ported.
//! - Limine has set up long mode, paging and the direct map before `init_x86_64` runs, so the
//!   BIOS/EFI memory-map walk and the page-table work of the C version are replaced by the
//!   boot protocol (`docs/ARCHITECTURE.md`, "Boot flow"): `pmap_direct_base` is the
//!   bootloader's higher-half direct map, and the memory clusters loaded into `uvm` are the
//!   protocol's usable regions, which already exclude the kernel, the firmware and the
//!   bootloader's own data. The ISA hole and the `avail_end` bookkeeping have nothing to do.
//! - `cpu_startup` prints the memory sizes and fills the boot CPU's TSS (`cpu_enter_pages`):
//!   `version` (generated `vers.c`, M5-b), the exec and physio maps and `bufinit` are;
//!   `cpu_init_extents` and `cpu_boot_mode` (M4-b) are not there yet.
//! - The IDT is a static page (`IDT`) instead of the early page `locore0.S` reserves and the
//!   page `init_x86_64` maps at `idt_vaddr`; `idt_allocmap` is an array of atomics.
//!   `cpu_init_msrs` and the `cpu_info_full_primary` initialiser are the first lines of
//!   `init_x86_64` because there is no `locore0.S` to run them earlier.
//!   `x86_64_proc0_tss_ldt_init` loads the task register only (from `cpu_configure`, as in
//!   C): proc0's pcb is M5. The IST
//!   stacks are filled by `cpu_enter_pages` from `cpu_startup`, as in C, so an NMI or double
//!   fault before then has no stack, as in C.
//! - The message buffer is a static area (`kern/subr_log.rs`, `init_static_msgbuf`) instead of
//!   reserved physical pages, until M3.
//! - `cninit()` is replaced by `consinit()` (`consinit.rs`): no `constab[]` yet.
//! - `delay_func` is a `StaticCell<fn(i32)>` written by `delay_init`/`delay_fini` during
//!   autoconfiguration; `delay` is the C's `DELAY(x)`/`delay(x)` macro over it.
//! - `init_x86_64` keeps the bootloader's processors (`BootInfo::mp`) in `cpu.rs`'s
//!   `BOOT_MP` (`MULTIPROCESSOR`), where the C's `acpimadt`/`mpbios` read the firmware's
//!   tables during autoconfiguration (`mainbus_attach`).
//! - `cpu_kick`/`cpu_unidle` always send `X86_IPI_NOP`: `cpu_init_mwait` is not ported, so
//!   `cpu_mwait_size` is 0 and no CPU idles in `mwait`.
//! - `init_x86_64` does `locore0.S`'s CPUID probe (`cpuid_level`, `cpu_vendor`, `cpu_id`,
//!   `cpu_ebxfeature`, `cpu_ecxfeature`, `cpu_feature` with `CPUID_NXE`) before
//!   `cpu_set_vendor`: there is no `locore0.S`. The meltdown and SEV probes are not there.
//! - `boot`: under feature `qemu`, the wait for a key after "The operating system has halted"
//!   is the emulator exit with the failure status, which `xtask smoke` checks after a panic.
//! - `sendsig`/`sys_sigreturn` without the FPU (`fpu.c` is not ported): `fpu_save_len` is the
//!   `fxsave` size and `cpu_use_xsaves` is false; the save area copied out is the pcb's as it
//!   is (no `fpusave`, since `CPUPF_USERXSTATE` is never set), `initialize_thread_xstate`,
//!   `fpureset`, `fpu_cleandata` and `xrstor_user` are reported. A handler therefore runs
//!   with the interrupted code's FPU/SSE registers and `sigreturn` does not restore them.
//!   `vfs_shutdown`, `resettodr`, `if_downall`, `uvm_shutdown`, `dumpsys` and
//!   `config_suspend_all` are reported as unported when reached.
//! - `bios_efiinfo` (boot(8)'s `BOOTARG_EFIINFO`) is replaced by Limine: its `config_acpi`,
//!   the RSDP's physical address, is `BIOS_EFIINFO_CONFIG_ACPI`, from Limine's RSDP request
//!   (`BootInfo::rsdp`), for `bios_attach`.
//! - `KBCMDP` and `KBC_PULSE0` (`dev/ic/i8042reg.h`, not ported) and `IO_KBD`
//!   (`dev/isa/isareg.h`) are local constants of `cpu_reset`, whose last resort faults
//!   with `ud2` through the emptied IDT where the C divides by zero: either fault becomes
//!   the triple fault that resets the processor.

use core::arch::asm;
use core::mem::offset_of;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::autoconf::COLD;
use crate::arch::amd64::amd64::consinit::consinit;
use crate::arch::amd64::amd64::cpu::{
    CPU_EBXFEATURE, CPU_ECXFEATURE, CPU_FEATURE, CPU_ID, CPU_INFO_FULL_PRIMARY, CPU_SEV_GUESTMODE,
    CPU_VENDOR, CPUID_LEVEL, NEED_RETPOLINE, cpu_enter_pages, cpu_info_primary_init, cpu_init_msrs,
};
use crate::arch::amd64::amd64::fpu::{FPU_SAVE_LEN, XSAVE_MASK, fpuinit};
use crate::arch::amd64::amd64::identcpu::AMD64_HAS_XCRYPT;
use crate::arch::amd64::amd64::intr::{intr_default_setup, splraise};
use crate::arch::amd64::amd64::locore::lgdt;
use crate::arch::amd64::amd64::pmap::{PMAP_DIRECT_BASE, PMAP_DIRECT_END, pmap_bootstrap};
use crate::arch::amd64::amd64::tsc::{TSC_FREQUENCY, TSC_IS_INVARIANT};
use crate::arch::amd64::amd64::vector::Xexceptions;
use crate::arch::amd64::include::biosvar::{BIOS_CKSUMLEN, BIOS_DEV, BIOS_DISKINFO};
use crate::arch::amd64::include::cpu::{
    CPU_ALLOWAPERTURE, CPU_BIOS, CPU_CHR2BLK, CPU_CONSDEV, CPU_CPUFEATURE, CPU_CPUID,
    CPU_CPUVENDOR, CPU_HIBERNATEDELAY, CPU_INVARIANTTSC, CPU_KBDRESET, CPU_LIDACTION,
    CPU_PWRACTION, CPU_RETPOLINE, CPU_TSCFREQ, CPU_VMMODE, CPU_XCRYPT, CPUPF_USERSEGS,
    CPUPF_USERXSTATE, CpuInfo, CpuVendor, cpu_info_primary, curcpu,
};
use crate::arch::amd64::include::cpufunc::{intr_enable, lidt, lldt, ltr, rcr3};
use crate::arch::amd64::include::fpu::{
    Savefpu, XstateHdr, fpu_cleandata, fpureset, fpusave, xrstor_user,
};
use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::intrdefs::IPL_IPI;
use crate::arch::amd64::include::param::{PAGE_SIZE, USPACE};
use crate::arch::amd64::include::proc::MDP_IRET;
use crate::arch::amd64::include::psl::{PSL_AC, PSL_D, PSL_T, PSL_USERSET, PSL_USERSTATIC, PSL_VM};
use crate::arch::amd64::include::segments::{
    GCODE_SEL, GDATA_SEL, GDT_SIZE, GPROC0_SEL, GUCODE_SEL, GUDATA_SEL, GateDescriptor,
    MemSegmentDescriptor, NIDT, RegionDescriptor, SDT_MEMERA, SDT_MEMRWA, SDT_SYS386IGT,
    SDT_SYS386TSS, SEL_KPL, SEL_UPL, SysSegmentDescriptor, gdt_addr_mem, gdt_addr_sys, gsel,
    gsyssel, usermode,
};
use crate::arch::amd64::include::signal::Sigcontext;
use crate::arch::amd64::include::specialreg::{
    CPUID_NXE, CPUIDECX_HV, SEV_STAT_ENABLED, SEV_STAT_ES_ENABLED, SEV_STAT_SNP_ACTIVE, cpuid,
};
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::arch::amd64::include::vmparam::{VM_MAXUSER_ADDRESS, VM_PHYS_SIZE};
use crate::arch::amd64::isa::clock::{
    i8254_delay, i8254_initclocks, i8254_start_both_clocks, rtcinit, startclocks,
};
use crate::conf::vers::VERSION;
use crate::dev::cons::cn_tab;
use crate::kassert;
use crate::kern::init_main::{BOOTHOWTO, PROC0};
use crate::kern::kern_sig::{sigexit, sigonstack};
use crate::kern::kern_softintr::softintr_init;
use crate::kern::kern_sysctl::{
    sysctl_bounded_arr, sysctl_rdint, sysctl_rdquad, sysctl_rdstring, sysctl_rdstruct,
    sysctl_securelevel_int,
};
use crate::kern::subr_log::init_static_msgbuf;
use crate::kern::subr_prf::splassert_fail;
use crate::kern::subr_xxx::chrtoblk;
use crate::kern::vfs_bio::bufinit;
use crate::kprintf;
use crate::machine::bootinfo::{BootInfo, MemKind};
use crate::machine::copy::{copyin, copyin_obj, copyout, copyout_obj};
use crate::machine::cpu::curproc;
use crate::machine::db_machdep::{db_enter, db_machine_init};
use crate::machine::{Cpu, Machine};
use crate::sys::errno::Errno;
use crate::sys::exec::{ExecPackage, PsStrings};
use crate::sys::param::{NCARGS, NODEV, roundup};
use crate::sys::proc::Proc;
use crate::sys::reboot::{
    RB_DUMP, RB_HALT, RB_KDB, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::siginfo::Siginfo;
use crate::sys::signal::{SIGILL, SS_DISABLE, Sig, Sigset};
use crate::sys::signalvar::sigcantmask;
use crate::sys::syscallargs::SysSigreturnArgs;
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::PHYSMEM;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::{Paddr, Register, Vaddr};
use crate::sys::user::{Uarea, User};
use crate::unported;
use crate::uvm::uvm_extern::{EXEC_MAP, PHYS_MAP, UvmConstraintRange};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{kernel_map, kernel_map_min, uvm_km_suballoc};
use crate::uvm::uvm_map::VM_MAP_PAGEABLE;
use crate::uvm::uvm_page::{uvm_page_physload, uvm_setpagesize};
use crate::uvm::uvm_param::{atop, ptoa, trunc_page};

#[cfg(feature = "qemu")]
use crate::arch::amd64::amd64::qemu;
#[cfg(not(feature = "qemu"))]
use crate::dev::cons::{cngetc, cnpollc};
#[cfg(feature = "qemu")]
use crate::machine::ExitStatus;

/// `cpureset_delay`: milliseconds to wait before resetting, from the `CPURESET_DELAY` option
/// (0 when not configured).
pub static CPURESET_DELAY: AtomicI32 = AtomicI32::new(0);
/// `lid_action`: what closing the lid does (`machdep.lidaction`).
pub static LID_ACTION: AtomicI32 = AtomicI32::new(1);
/// `pwr_action`: what the power button does: 0 nothing, 1 power down, 2 suspend
/// (`machdep.pwraction`).
pub static PWR_ACTION: AtomicI32 = AtomicI32::new(1);
/// `kbd_reset`: keyboard reset under pcvt (`machdep.kbdreset`).
pub static KBD_RESET: AtomicI32 = AtomicI32::new(0);
/// `hibernate_delay`: hibernate delay after suspend (`machdep.hibernatedelay`).
pub static HIBERNATE_DELAY: AtomicI32 = AtomicI32::new(0);
/// `bios_efiinfo->config_acpi`: the physical address of the ACPI RSDP the firmware gave
/// (here Limine's RSDP request), 0 without one.
pub static BIOS_EFIINFO_CONFIG_ACPI: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
/// `cpuresetfn`: the reset method acpi(4) installs (`acpi_reset`), tried first by
/// `cpu_reset`.
pub static CPURESETFN: StaticCell<Option<fn()>> = StaticCell::new(None);
/// `waittime`: set once the file systems have been synced on the way down.
static WAITTIME: AtomicI32 = AtomicI32::new(-1);
/// `isa_constraint`: what ISA DMA can reach.
pub static ISA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0x00ff_ffff),
};
/// `dma_constraint`: what 32-bit DMA can reach.
pub static DMA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0xffff_ffff),
};
/// `uvm_md_constraints[]`: the machine's DMA ranges.
pub static UVM_MD_CONSTRAINTS: [&UvmConstraintRange; 2] = [&ISA_CONSTRAINT, &DMA_CONSTRAINT];

/// The interrupt descriptor table, one page (the C's `early_idt` in `locore0.S`, later a
/// page `init_x86_64` maps at `idt_vaddr`).
#[repr(C, align(4096))]
pub struct Idt(pub [GateDescriptor; NIDT]);

/// `idt`: the interrupt descriptor table. Written by `init_x86_64` and `idt_vec_set` on the
/// boot CPU; the CPUs read it.
pub static IDT: StaticCell<Idt> = StaticCell::new(Idt([const { GateDescriptor::zeroed() }; NIDT]));

/// `idt_allocmap[]`: which vectors are taken.
pub static IDT_ALLOCMAP: [AtomicBool; NIDT] = [const { AtomicBool::new(false) }; NIDT];

/// `proc0paddr`: proc0's u-area (its pcb; the boot stack is Limine's, see the deviations).
pub static PROC0_UAREA: Uarea = Uarea::new();

/// `proc0paddr`: proc0's `struct user`, at the bottom of its u-area.
pub fn proc0paddr() -> &'static User {
    &PROC0_UAREA.u
}
/// `cpu_set_vendor`: records `ci`'s cpuid level and maps the vendor string to an integer.
pub fn cpu_set_vendor(ci: &CpuInfo, level: u32, vendor: &[u8]) {
    ci.ci_cpuid_level.set(level);
    CPUID_LEVEL.fetch_min(level, Ordering::Relaxed);

    // map the vendor string to an integer
    let end = vendor.iter().position(|&c| c == 0).unwrap_or(vendor.len());
    ci.ci_vendor.set(match &vendor[..end] {
        b"AuthenticAMD" => CpuVendor::CPUV_AMD,
        b"GenuineIntel" => CpuVendor::CPUV_INTEL,
        b"CentaurHauls" => CpuVendor::CPUV_VIA,
        _ => CpuVendor::CPUV_UNKNOWN,
    });
}

/// The direct map covers at least this much, by the boot protocol's guarantee.
const DIRECT_MAP_MIN_SIZE: usize = 4 << 30;

/// `init_x86_64`: the first C of the kernel, called from `locore` with the machine as the
/// bootloader left it. Here: the direct map, the message buffer, the console, the pmap
/// bootstrap, the physical memory and the `boot -d` hook.
///
/// # Safety
///
/// Call once, on the boot CPU, before anything else runs, with `boot` describing the loaded
/// image.
pub unsafe fn init_x86_64(boot: &BootInfo) -> Result<(), &'static str> {
    // The C's locore0.S points GS.base at cpu_info_primary before init_x86_64, whose first
    // call is cpu_init_msrs; here the static initialiser of cpu_info_full_primary runs first,
    // then the MSRs, so curcpu() works from this point on.
    cpu_info_primary_init();
    let ci = cpu_info_primary();
    // SAFETY: the boot CPU, once, before anything reads curcpu().
    unsafe { cpu_init_msrs(ci) };

    // bios_efiinfo->config_acpi: the RSDP Limine found, as a physical address (the protocol
    // hands it over as an address in the higher-half direct map, or as a physical one).
    if let Some(rsdp) = boot.rsdp {
        let va = rsdp.as_usize();
        let pa = if va >= boot.hhdm_offset {
            va - boot.hhdm_offset
        } else {
            va
        };
        BIOS_EFIINFO_CONFIG_ACPI.store(pa as u64, Ordering::Relaxed);
    }

    // MULTIPROCESSOR: the processors the bootloader found, for mainbus_attach (the C's
    // acpimadt/mpbios read the firmware tables) and mp_cpu_start.
    // SAFETY: the boot CPU, once, before anything reads it.
    #[cfg(feature = "multiprocessor")]
    unsafe {
        crate::arch::amd64::amd64::cpu::BOOT_MP.write(boot.mp)
    };

    // locore0.S: cpuid(0) gives cpuid_level and the vendor string, cpuid(1) the signature
    // and the feature words; the NX bit of cpuid(0x80000001) is or'ed into cpu_feature
    // (pg_nx itself is the bootloader's EFER.NXE, pmap.rs).
    let (level, vb, vc, vd) = cpuid(0);
    CPUID_LEVEL.store(level, Ordering::Relaxed);
    let mut vendor = [0u8; 16];
    vendor[0..4].copy_from_slice(&vb.to_le_bytes());
    vendor[4..8].copy_from_slice(&vd.to_le_bytes());
    vendor[8..12].copy_from_slice(&vc.to_le_bytes());
    // SAFETY: the boot CPU, once, before anything reads cpu_vendor.
    unsafe { CPU_VENDOR.write(vendor) };
    let (id, ebx, ecx, edx) = cpuid(1);
    CPU_ID.store(id, Ordering::Relaxed);
    CPU_EBXFEATURE.store(ebx, Ordering::Relaxed);
    CPU_ECXFEATURE.store(ecx, Ordering::Relaxed);
    CPU_FEATURE.store(edx, Ordering::Relaxed);
    let (_, _, _, eedx) = cpuid(0x8000_0001);
    CPU_FEATURE.fetch_or(eedx & CPUID_NXE, Ordering::Relaxed);

    // SAFETY: written just above, read on the same CPU.
    cpu_set_vendor(ci, level, unsafe { CPU_VENDOR.get() });

    // The direct map is the bootloader's (see the module's deviations); the C derives
    // pmap_direct_base from L4_SLOT_DIRECT here.
    let regions = boot.memmap.regions();
    let map_end = regions
        .iter()
        .map(|r| r.base.as_usize() + r.length.as_usize())
        .max()
        .unwrap_or(0);
    PMAP_DIRECT_BASE.store(boot.hhdm_offset, Ordering::Relaxed);
    PMAP_DIRECT_END.store(
        boot.hhdm_offset + roundup(map_end, 1 << 30).max(DIRECT_MAP_MIN_SIZE),
        Ordering::Relaxed,
    );

    // cpu_init_early_vctrap and the early PTE pages: M4 and the page tables.

    init_static_msgbuf();
    consinit(); // cninit() in C

    // The memory map is the bootloader's, already flensed (see the module's deviations).
    let avail_end = regions
        .iter()
        .filter(|r| r.kind == MemKind::Usable)
        .map(|r| r.base.as_usize() + r.length.as_usize())
        .max()
        .unwrap_or(0);

    // Call pmap initialization to make new kernel address space.
    // SAFETY: once, on the boot CPU, with the direct map set above and paging on.
    let first_avail = unsafe { pmap_bootstrap(Paddr::new(0), Paddr::new(trunc_page(avail_end))) };

    // Now, load the memory clusters (which have already been flensed) into the VM system.
    for r in regions.iter().filter(|r| r.kind == MemKind::Usable) {
        let seg_start = r.base.as_usize().max(first_avail.as_usize());
        let seg_end = r.base.as_usize() + r.length.as_usize();

        if seg_start > seg_end {
            continue;
        }
        if seg_end - seg_start < PAGE_SIZE {
            continue;
        }

        PHYSMEM.fetch_add(atop(r.length.as_usize()), Ordering::Relaxed);

        uvm_page_physload(
            atop(seg_start),
            atop(seg_end),
            atop(seg_start),
            atop(seg_end),
            0,
        );
    }
    // The memory between the ISA hole and the kernel, and the message buffer pages: the map
    // has no hole to load around, and the message buffer is static (M2).

    uvm_setpagesize();

    // The idt_vaddr/idt_paddr page: the IDT is a static page here (see the module's
    // deviations). proc0paddr, the lapic page and the trampoline pages: M5, M4-b, M6.

    ci.ci_tss.set(CPU_INFO_FULL_PRIMARY.cif_tss.get());
    ci.ci_gdt
        .set(CPU_INFO_FULL_PRIMARY.cif_gdt.get().cast::<u8>());

    // make gdt gates and memory segments
    // SAFETY: the boot CPU's GDT, written once here before lgdt loads it.
    let gdt = unsafe { &mut *CPU_INFO_FULL_PRIMARY.cif_gdt.get() };
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GCODE_SEL)],
        0,
        0xfffff,
        SDT_MEMERA,
        SEL_KPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GDATA_SEL)],
        0,
        0xfffff,
        SDT_MEMRWA,
        SEL_KPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GUDATA_SEL)],
        0,
        atop(VM_MAXUSER_ADDRESS) - 1,
        SDT_MEMRWA,
        SEL_UPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GUCODE_SEL)],
        0,
        atop(VM_MAXUSER_ADDRESS) - 1,
        SDT_MEMERA,
        SEL_UPL,
        true,
        false,
        true,
    );

    // make ldt memory segments: no LDT (M6).

    let sys = gdt_addr_sys(GPROC0_SEL);
    set_sys_segment(
        &mut gdt[sys..sys + 2],
        CPU_INFO_FULL_PRIMARY.cif_tss.get() as usize,
        size_of::<X86_64Tss>() - 1,
        SDT_SYS386TSS,
        SEL_KPL,
        false,
    );

    // exceptions
    // SAFETY: the boot CPU's IDT, written here before cpu_init_idt loads it.
    let idt = unsafe { IDT.get_mut() };
    for x in 0..32usize {
        // trap2 == NMI, trap8 == double fault
        let ist = match x {
            2 => 2,
            8 => 1,
            _ => 0,
        };
        // SAFETY: Xexceptions is vector.S's table of the 32 exception entry points.
        let func = unsafe { Xexceptions[x] } as usize;
        setgate(
            &mut idt.0[x],
            func,
            ist,
            SDT_SYS386IGT,
            if x == 3 { SEL_UPL } else { SEL_KPL },
            gsel(GCODE_SEL, SEL_KPL),
        );
        IDT_ALLOCMAP[x].store(true, Ordering::Relaxed);
    }

    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(
        &mut region,
        CPU_INFO_FULL_PRIMARY.cif_gdt.get() as usize,
        (GDT_SIZE - 1) as u16,
    );
    // SAFETY: the GDT filled above has valid 64-bit kernel code and data segments at
    // GCODE_SEL/GDATA_SEL and lives forever.
    unsafe { lgdt(&region) };
    cpu_init_idt();

    intr_default_setup();

    fpuinit();

    softintr_init();
    splraise(IPL_IPI);
    // SAFETY: the IDT has every exception and legacy interrupt gate, the PIC is masked and
    // the level is IPL_IPI, so nothing can be delivered that has no handler.
    unsafe { intr_enable() };

    // BOOTARG_BOOTSR (`bios_bootsr`: the UUID and mask key of the softraid volume OpenBSD's
    // boot(8) booted from, copied into `sr_bootuuid`/`sr_bootkey` under NSOFTRAID):
    // replaced-by-limine. Limine hands no softraid key over, so `dev/softraid.rs`'s
    // `SR_BOOTUUID`/`SR_BOOTKEY` stay zero and a crypto volume is unlocked with bioctl(8).
    // The ACPI/MP tables, the memory-map and -b/-c handling of the bootinfo: M5.
    db_machine_init();
    // ddb_init() (db_sym.c, db_elf.c: the kernel's symbol table) is not ported.
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
}

/// `splassert_check`: the `DIAGNOSTIC` level check behind `splassert`.
pub fn splassert_check(wantipl: i32, func: &str) {
    let cpl = curcpu().ci_ilevel.get();
    let floor = curcpu().ci_handled_intr_level.get();

    if cpl < wantipl {
        splassert_fail(wantipl, cpl, func);
    }
    if floor > wantipl {
        splassert_fail(wantipl, floor, func);
    }
}

/// `reset_segs`: force the userspace FS.base to be reloaded from the PCB on return from the
/// kernel, and reset the segment registers (`%ds`, `%es`, `%fs`, and `%gs`) to their expected
/// userspace value.
pub fn reset_segs() {
    // This operates like the cpu_switchto() sequence: if we haven't reset %[defg]s already,
    // do so now.
    let ci = curcpu();
    if ci.ci_pflags.get() & CPUPF_USERSEGS != 0 {
        ci.ci_pflags.set(ci.ci_pflags.get() & !CPUPF_USERSEGS);
        // SAFETY: loads the user data selector into the data segment registers and, between
        // swapgs pairs with interrupts blocked, into %gs (which zeroes the user GS.base); the
        // kernel's GS.base is back before interrupts are allowed again.
        unsafe {
            asm!(
                "mov ds, ax",
                "mov es, ax",
                "mov fs, ax",
                "cli",    // block intr when on user GS.base
                "swapgs", // swap from kernel to user GS.base
                "mov gs, ax", // set %gs to UDATA and GS.base to 0
                "swapgs", // back to kernel GS.base
                "sti",
                in("ax") gsel(GUDATA_SEL, SEL_UPL),
                options(nostack)
            );
        }
    }
}

/// `setregs`: clear registers on exec.
pub fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, _arginfo: &PsStrings) {
    initialize_thread_xstate(p);

    // To reset all registers we have to return via iretq
    p.p_md.md_flags.set(p.p_md.md_flags.get() | MDP_IRET);

    reset_segs();
    p.pcb().pcb_fsbase.set(0);

    let tf = p.p_md.md_regs.get();
    // SAFETY: `md_regs` is the thread's trap frame at the top of its u-area (`cpu_fork`),
    // which only this thread writes, with no reference to it alive here.
    unsafe {
        tf.write(Trapframe::default());
        (*tf).tf_rip = pack.ep_entry as i64;
        (*tf).tf_cs = i64::from(gsel(GUCODE_SEL, SEL_UPL));
        (*tf).tf_rflags = PSL_USERSET as i64;
        (*tf).tf_rsp = stack.as_usize() as i64;
        (*tf).tf_ss = i64::from(gsel(GUDATA_SEL, SEL_UPL));
    }
}

/// `copyin32`: copies the aligned 32-bit word at `uaddr` in; `copyin(9)` is atomic for it.
pub fn copyin32(uaddr: usize) -> Result<u32, Errno> {
    if uaddr & 0x3 != 0 {
        return Err(Errno::EFAULT);
    }

    // copyin(9) is atomic
    let mut word = [0u8; 4];
    crate::machine::copy::copyin(uaddr, &mut word)?;
    Ok(u32::from_ne_bytes(word))
}

/// `x86_64_proc0_tss_ldt_init`: loads the boot CPU's task register and clears the LDT.
pub fn x86_64_proc0_tss_ldt_init() {
    let pcb = &proc0paddr().u_pcb;
    cpu_info_primary().ci_curpcb.set(pcb);
    pcb.pcb_fsbase.set(0);
    pcb.pcb_kstack
        .set(ptr::addr_of!(PROC0_UAREA) as u64 + USPACE as u64 - 16);
    // The kernel's page tables, what cpu_switchto compares %cr3 with (see the deviations).
    pcb.pcb_cr3.set(rcr3());
    PROC0
        .p_md
        .md_regs
        .set((pcb.pcb_kstack.get() as *mut Trapframe).wrapping_sub(1));

    // SAFETY: GPROC0_SEL holds the available TSS descriptor init_x86_64 set, loaded once;
    // selector 0 means no LDT.
    unsafe {
        ltr(gsyssel(GPROC0_SEL, SEL_KPL));
        lldt(0);
    }
}

/// `cpu_startup`: machine-dependent startup code (see the module's deviations).
pub fn cpu_startup() {
    // msgbuf_vaddr / initmsgbuf: the message buffer is static (M2).

    kprintf!("{}", VERSION);
    startclocks();
    rtcinit();

    let physmem = PHYSMEM.load(Ordering::Relaxed);
    kprintf!(
        "real mem = {} ({}MB)\n",
        ptoa(physmem),
        ptoa(physmem) / 1024 / 1024
    );

    // Allocate a submap for exec arguments. This map effectively limits the number of
    // processes exec'ing at any time.
    let mut minaddr = kernel_map_min().as_usize();
    let mut maxaddr = 0;
    let exec_map = uvm_km_suballoc(
        kernel_map(),
        &mut minaddr,
        &mut maxaddr,
        16 * NCARGS,
        VM_MAP_PAGEABLE,
        false,
        None,
    );
    EXEC_MAP.store(ptr::from_ref(exec_map).cast_mut(), Ordering::Release);

    // Allocate a submap for physio
    minaddr = kernel_map_min().as_usize();
    let phys_map = uvm_km_suballoc(
        kernel_map(),
        &mut minaddr,
        &mut maxaddr,
        VM_PHYS_SIZE,
        0,
        false,
        None,
    );
    PHYS_MAP.store(ptr::from_ref(phys_map).cast_mut(), Ordering::Release);

    // cpu_init_extents: with the extents.

    let free = UVMEXP.free.load(Ordering::Relaxed).max(0) as usize;
    kprintf!(
        "avail mem = {} ({}MB)\n",
        ptoa(free),
        ptoa(free) / 1024 / 1024
    );

    bufinit();

    // cpu_boot_mode, the ISA DMA bounce pages, the microcode and TSX setup,
    // enter_shared_special_pages (the u-k maps): M4-b and M6.

    // initialize CPU0's TSS and GDT and put them in the u-k maps
    // SAFETY: once, on the boot CPU, for its own pages (the TSS is loaded; the CPU reads it
    // on the next privilege or stack switch).
    unsafe { cpu_enter_pages(&CPU_INFO_FULL_PRIMARY) };
}

/// `initialize_thread_xstate`: give the thread a clean FPU state, the user state from now on
/// (`CPUPF_USERXSTATE`).
fn initialize_thread_xstate(p: &Proc) {
    // cpu_use_xsaves (xrstors, maybe_enable_user_cet): no XSAVE (amd64/fpu.rs).
    // Reset FPU state in PCB
    // SAFETY: the thread's own save area (only it touches it) and the clean one, distinct
    // and both valid `Savefpu`s; fpu_save_len is at most their size.
    unsafe {
        ptr::copy_nonoverlapping(
            fpu_cleandata().cast::<u8>(),
            p.pcb().pcb_savefpu.get().cast::<u8>(),
            FPU_SAVE_LEN.load(Ordering::Relaxed),
        );
    }

    if curcpu().ci_pflags.get() & CPUPF_USERXSTATE != 0 {
        // state in CPU is obsolete; reset it
        fpureset();
    }

    // The reset state _is_ the userspace state for this thread now
    curcpu()
        .ci_pflags
        .set(curcpu().ci_pflags.get() | CPUPF_USERXSTATE);
}

/// `copyoutfpu`: copy out the FPU state, massaging it to be usable from userspace and
/// acceptable to `xrstor_user()`.
fn copyoutfpu(sfp: &Savefpu, sp: usize, len: usize) -> Result<(), Errno> {
    // SAFETY: `Savefpu` is `repr(C)` of packed integer structures and arrays whose sizes add
    // up to its alignment multiple (no padding), so all its bytes are initialised.
    let bytes = unsafe {
        core::slice::from_raw_parts(ptr::from_ref(sfp).cast::<u8>(), size_of::<Savefpu>())
    };
    copyout(&bytes[..len], sp)?;
    if len > offset_of!(Savefpu, fp_xstate) + offset_of!(XstateHdr, xstate_bv) {
        // The xstate_bv/xstate_xcomp_bv fix-up (XFEATURE_XCR0_MASK, XFEATURE_COMPRESSED):
        // only an xsave area is longer than the fxsave one (fpu.c).
        let _ = unported!("copyoutfpu: the xstate_bv fix-up (fpu.c)");
    }
    Ok(())
}

/// `sendsig`: send an interrupt to process.
///
/// Stack is set up to allow sigcode to call routine, followed by syscall to sigreturn
/// routine below. After sigreturn resets the signal mask, the stack, and the frame pointer,
/// it returns to the user specified pc.
pub fn sendsig(
    catcher: Sig,
    sig: i32,
    mask: Sigset,
    ksip: &Siginfo,
    info: bool,
    onstack: bool,
) -> Result<(), Errno> {
    let Some(p) = curproc() else {
        return Err(Errno::EFAULT);
    };
    let pr = p.process();
    // SAFETY: `md_regs` is the current thread's trap frame on its kernel stack (the system
    // call, trap or AST entry recorded it); only this thread touches it, and no other
    // reference to it is alive while we run.
    let tf = unsafe { &mut *p.p_md.md_regs.get() };
    let mut ksc = Sigcontext {
        sc_rdi: tf.tf_rdi,
        sc_rsi: tf.tf_rsi,
        sc_rdx: tf.tf_rdx,
        sc_rcx: tf.tf_rcx,
        sc_r8: tf.tf_r8,
        sc_r9: tf.tf_r9,
        sc_r10: tf.tf_r10,
        sc_r11: tf.tf_r11,
        sc_r12: tf.tf_r12,
        sc_r13: tf.tf_r13,
        sc_r14: tf.tf_r14,
        sc_r15: tf.tf_r15,
        sc_rbx: tf.tf_rbx,
        sc_rax: tf.tf_rax,
        sc_rbp: tf.tf_rbp,
        sc_rip: tf.tf_rip,
        sc_cs: tf.tf_cs,
        sc_rflags: tf.tf_rflags,
        sc_rsp: tf.tf_rsp,
        sc_ss: tf.tf_ss,
        sc_mask: mask as i32,
        ..Sigcontext::default()
    };

    // Allocate space for the signal handler context.
    let ss = p.p_sigstk.get();
    let mut sp = if ss.ss_flags & SS_DISABLE == 0 && !sigonstack(tf.tf_rsp as usize) && onstack {
        trunc_page(ss.ss_sp + ss.ss_size)
    } else {
        (tf.tf_rsp as usize).wrapping_sub(128)
    };

    let fpu_save_len = FPU_SAVE_LEN.load(Ordering::Relaxed);
    sp = sp.wrapping_sub(fpu_save_len);
    // cpu_use_xsaves (sp &= ~63): fpu.c, not ported.
    sp &= !15; // just in case

    // Save FPU state to PCB if necessary, then copy it out
    if curcpu().ci_pflags.get() & CPUPF_USERXSTATE != 0 {
        // SAFETY: the thread's own save area; the CPU holds its state.
        unsafe { fpusave(p.pcb().pcb_savefpu.get()) };
    }
    // SAFETY: the thread's own FPU save area, which only this thread reads or writes; no
    // write to it happens while this borrow lives.
    let sfp = unsafe { &*p.pcb().pcb_savefpu.get() };
    copyoutfpu(sfp, sp, fpu_save_len)?;

    initialize_thread_xstate(p);

    ksc.sc_fpstate = sp;
    let mut sss = (size_of::<Sigcontext>() + 15) & !15;
    let mut sip = 0usize;
    if info {
        sip = sp - ((size_of::<Siginfo>() + 15) & !15);
        sss += (size_of::<Siginfo>() + 15) & !15;

        copyout_obj(ksip, sip)?;
    }
    let scp = sp - sss;

    ksc.sc_cookie = (scp as i64) ^ (pr.ps_sigcookie.get() as i64);
    copyout_obj(&ksc, scp)?;

    // Build context to run handler in.
    tf.tf_rax = catcher as i64;
    tf.tf_rdi = i64::from(sig);
    tf.tf_rsi = sip as i64;
    tf.tf_rdx = scp as i64;

    tf.tf_rip = pr.ps_sigcode.get() as i64;
    tf.tf_cs = i64::from(gsel(GUCODE_SEL, SEL_UPL));
    tf.tf_rflags &= !((PSL_T | PSL_D | PSL_VM | PSL_AC) as i64);
    tf.tf_rsp = scp as i64;
    tf.tf_ss = i64::from(gsel(GUDATA_SEL, SEL_UPL));

    Ok(())
}

/// `sys_sigreturn`: system call to cleanup state after a signal has been taken. Reset
/// signal mask and stack state from context left by sendsig (above). Return to previous pc
/// and psl as specified by context left by sendsig. Check carefully to make sure that the
/// user has not modified the psl to gain improper privileges or to cause a machine fault.
pub fn sys_sigreturn(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigreturnArgs = sysargs(v);
    let scp = uap.sigcntxp.get() as usize;
    let pr = p.process();

    if Machine::proc_pc(p) != pr.ps_sigcoderet.get() {
        sigexit(p, SIGILL);
        // return (EPERM): sigexit does not return.
    }

    let mut ksc: Sigcontext = copyin_obj(scp)?;

    if ksc.sc_cookie != ((scp as i64) ^ (pr.ps_sigcookie.get() as i64)) {
        sigexit(p, SIGILL);
        // return (EFAULT): sigexit does not return.
    }

    // Prevent reuse of the sigcontext cookie
    ksc.sc_cookie = 0;
    let _ = copyout(
        &ksc.sc_cookie.to_ne_bytes(),
        scp + offset_of!(Sigcontext, sc_cookie),
    );

    // SAFETY: as in `sendsig`: the current thread's trap frame, which the system call entry
    // recorded.
    let tf = unsafe { &mut *p.p_md.md_regs.get() };

    if ((ksc.sc_rflags ^ tf.tf_rflags) as u64) & PSL_USERSTATIC != 0 || !usermode(ksc.sc_cs as u64)
    {
        return Err(Errno::EINVAL);
    }

    // Current FPU state is obsolete; toss it and force a reload
    let ci = curcpu();
    if ci.ci_pflags.get() & CPUPF_USERXSTATE != 0 {
        ci.ci_pflags.set(ci.ci_pflags.get() & !CPUPF_USERXSTATE);
        fpureset();
    }

    // Copy in the FPU state to restore
    if ksc.sc_fpstate != 0 {
        // SAFETY: the thread's own FPU save area, which only this thread reads or writes,
        // viewed as the bytes `copyin` fills; any bytes are a valid save area.
        let sfp = unsafe {
            core::slice::from_raw_parts_mut(
                p.pcb().pcb_savefpu.get().cast::<u8>(),
                size_of::<Savefpu>(),
            )
        };
        let fpu_save_len = FPU_SAVE_LEN.load(Ordering::Relaxed);
        let clean = || {
            // SAFETY: the clean area and the thread's own, distinct; the byte view above is
            // not used again.
            unsafe {
                ptr::copy_nonoverlapping(
                    fpu_cleandata().cast::<u8>(),
                    p.pcb().pcb_savefpu.get().cast::<u8>(),
                    fpu_save_len,
                );
            }
        };
        if let Err(error) = copyin(ksc.sc_fpstate, &mut sfp[..fpu_save_len]) {
            clean();
            return Err(error);
        }
        // SAFETY: the thread's own save area; a state the CPU rejects raises #GP, which
        // trap0d turns into a return of 1.
        if unsafe {
            xrstor_user(
                p.pcb().pcb_savefpu.get(),
                XSAVE_MASK.load(Ordering::Relaxed),
            )
        } != 0
        {
            clean();
            return Err(Errno::EINVAL);
        }
        // maybe_enable_user_cet(p): CET (XSAVES' CET_U state) is not supported.
        ci.ci_pflags.set(ci.ci_pflags.get() | CPUPF_USERXSTATE);
    } else {
        // shouldn't happen, but handle it
        initialize_thread_xstate(p);
    }

    tf.tf_rdi = ksc.sc_rdi;
    tf.tf_rsi = ksc.sc_rsi;
    tf.tf_rdx = ksc.sc_rdx;
    tf.tf_rcx = ksc.sc_rcx;
    tf.tf_r8 = ksc.sc_r8;
    tf.tf_r9 = ksc.sc_r9;
    tf.tf_r10 = ksc.sc_r10;
    tf.tf_r11 = ksc.sc_r11;
    tf.tf_r12 = ksc.sc_r12;
    tf.tf_r13 = ksc.sc_r13;
    tf.tf_r14 = ksc.sc_r14;
    tf.tf_r15 = ksc.sc_r15;
    tf.tf_rbx = ksc.sc_rbx;
    tf.tf_rax = ksc.sc_rax;
    tf.tf_rbp = ksc.sc_rbp;
    tf.tf_rip = ksc.sc_rip;
    tf.tf_cs = ksc.sc_cs;
    tf.tf_rflags = ksc.sc_rflags;
    tf.tf_rsp = ksc.sc_rsp;
    tf.tf_ss = ksc.sc_ss;

    // Restore signal mask.
    p.p_sigmask.set(ksc.sc_mask as Sigset & !sigcantmask());

    // sigreturn() needs to return to userspace via the 'iretq' method, so that if the
    // process was interrupted (by tick, an IPI, whatever) as opposed to already being in the
    // kernel when a signal was being delivered, the process will be completely restored,
    // including the userland %rcx and %r11 registers which the 'sysretq' instruction cannot
    // restore. Also need to make sure we can handle faulting on xrstor.
    p.p_md.md_flags.set(p.p_md.md_flags.get() | MDP_IRET);

    Err(Errno::EJUSTRETURN)
}

/// `cpu_kick`: force a CPU into the kernel, whether or not it's idle (`MULTIPROCESSOR`;
/// nothing on one CPU).
pub fn cpu_kick(_ci: &CpuInfo) {
    #[cfg(feature = "multiprocessor")]
    // only need to kick other CPUs
    if !ptr::eq(_ci, curcpu()) {
        // cpu_mwait_size > 0 (MWAIT_IN_IDLE / MWAIT_KEEP_IDLING): cpu_init_mwait is not
        // ported, so there is no mwait and an IPI is needed.
        crate::arch::amd64::amd64::ipi::x86_send_ipi(
            _ci,
            crate::arch::amd64::include::intrdefs::X86_IPI_NOP,
        );
    }
}

/// `signotify`: notify the current process (p) that it has a signal pending, process as
/// soon as possible.
pub fn signotify(p: &Proc) {
    aston(p);
    if let Some(ci) = p.cpu() {
        cpu_kick(ci);
    }
}

/// `boot(9)`: halts or reboots according to `howto`.
pub fn boot(howto: i32) -> ! {
    let mut howto = howto;

    // NACPI > 0
    if howto & RB_POWERDOWN != 0
        && let Some(sc) = crate::dev::acpi::acpivar::acpi_softc()
    {
        sc.sc_state
            .set(i32::from(crate::dev::acpi::acpireg::ACPI_STATE_S5));
    }

    if howto & RB_POWERDOWN != 0 {
        LID_ACTION.store(0, Ordering::Relaxed);
    }

    if howto & RB_RESET == 0 {
        if COLD.load(Ordering::Relaxed) {
            if howto & RB_USERREQ == 0 {
                howto |= RB_HALT;
            }
        } else {
            BOOTHOWTO.store(howto, Ordering::Relaxed);
            if howto & RB_NOSYNC == 0 && WAITTIME.load(Ordering::Relaxed) < 0 {
                WAITTIME.store(0, Ordering::Relaxed);
                let _ = unported!("vfs_shutdown");

                if howto & RB_TIMEBAD == 0 {
                    let _ = unported!("resettodr");
                } else {
                    kprintf!("WARNING: not updating battery clock\n");
                }
            }
            let _ = unported!("if_downall");

            let _ = unported!("uvm_shutdown");
            // splhigh(): M4.
            COLD.store(true, Ordering::Relaxed);

            if howto & RB_DUMP != 0 {
                let _ = unported!("dumpsys");
            }
        }

        // haltsys:
        let _ = unported!("config_suspend_all (DVACT_POWERDOWN)");

        #[cfg(feature = "multiprocessor")]
        crate::arch::amd64::amd64::ipi::x86_broadcast_ipi(
            crate::arch::amd64::include::intrdefs::X86_IPI_HALT,
        );

        if howto & RB_HALT != 0 {
            // NACPI > 0 && !SMALL_KERNEL
            if crate::dev::acpi::acpi::ACPI_ENABLED.load(Ordering::Relaxed) != 0 {
                delay(500000);
                if howto & RB_POWERDOWN != 0 {
                    crate::dev::acpi::acpi::acpi_powerdown();
                }
            }
            kprintf!("\n");
            kprintf!("The operating system has halted.\n");
            kprintf!("Please press any key to reboot.\n\n");
            #[cfg(feature = "qemu")]
            {
                qemu::exit(ExitStatus::Failure)
            }
            #[cfg(not(feature = "qemu"))]
            {
                cnpollc(true); // for proper keyboard command handling
                cngetc();
                cnpollc(false);
            }
        }
    }

    // doreset:
    kprintf!("rebooting...\n");
    let d = CPURESET_DELAY.load(Ordering::Relaxed);
    if d > 0 {
        delay((d * 1000) as u32);
    }
    cpu_reset()
}

/// `IO_KBD` (`dev/isa/isareg.h`): the keyboard controller's ports.
const IO_KBD: u16 = 0x060;
/// `KBCMDP` (`dev/ic/i8042reg.h`): the keyboard controller's command port.
const KBCMDP: u16 = 4;
/// `KBC_PULSE0` (`dev/ic/i8042reg.h`): pulse output bit 0.
const KBC_PULSE0: u8 = 0xfe;

/// `cpu_reset`: resets the machine: acpi(4)'s reset register (`cpuresetfn`), then the
/// keyboard controller's reset line, then a triple fault.
pub fn cpu_reset() -> ! {
    let _ = crate::arch::amd64::include::cpufunc::intr_disable();

    // SAFETY: `acpi_attach_machdep` writes it once during autoconfiguration.
    if let Some(f) = unsafe { CPURESETFN.read() } {
        f();
    }

    // The keyboard controller has 4 random output pins, one of which is connected to the
    // RESET pin on the CPU in many PCs. We tell the keyboard controller to pulse this line
    // a couple of times.
    // SAFETY: the i8042's command port; pulsing output bit 0 resets the machine where it is
    // wired to RESET and does nothing else.
    unsafe { crate::arch::amd64::include::pio::outb(IO_KBD + KBCMDP, KBC_PULSE0) };
    delay(100000);
    // SAFETY: as above.
    unsafe { crate::arch::amd64::include::pio::outb(IO_KBD + KBCMDP, KBC_PULSE0) };
    delay(100000);

    // Try to cause a triple fault and watchdog reset by making the IDT invalid and causing
    // a fault.
    // SAFETY: interrupts are off and the machine is going down: an empty IDT turns the
    // fault below into a triple fault, which resets the processor.
    unsafe { IDT.write(Idt([const { GateDescriptor::zeroed() }; NIDT])) };
    // SAFETY: `ud2` raises #UD through the empty IDT (the C divides by zero for the same
    // effect); nothing after it runs.
    unsafe { asm!("ud2", options(nomem, nostack)) };

    Machine::halt()
}

/// `delay(9)`, the C's `DELAY(x)` macro: busy-waits `usec` microseconds through
/// `delay_func`.
pub fn delay(usec: u32) {
    // SAFETY: as for `delay_is_i8254`.
    (unsafe { DELAY_FUNC.read() })(usec.min(i32::MAX as u32) as i32);
}

/// `delay_func`: `i8254_delay` until a better delay source (`tsc_delay`) calls `delay_init`.
static DELAY_FUNC: StaticCell<fn(i32)> = StaticCell::new(i8254_delay);

/// `delay_func == i8254_delay` (`lapic_calibrate_timer`).
pub fn delay_is_i8254() -> bool {
    // SAFETY: `delay_init`/`delay_fini` write it on the boot CPU during autoconfiguration;
    // readers see the static default or the value written there.
    core::ptr::fn_addr_eq(unsafe { DELAY_FUNC.read() }, i8254_delay as fn(i32))
}

/// `initclock_func`: the i8254 until `lapic_calibrate_timer` installs the LAPIC timer.
static INITCLOCK_FUNC: StaticCell<fn()> = StaticCell::new(i8254_initclocks);
/// `startclock_func`.
static STARTCLOCK_FUNC: StaticCell<fn()> = StaticCell::new(i8254_start_both_clocks);

/// `initclock_func = f`: picks the clock hardware (`lapic_calibrate_timer`).
pub fn set_initclock_func(f: fn()) {
    // SAFETY: written on the boot CPU during autoconfiguration, before `cpu_initclocks`.
    unsafe { INITCLOCK_FUNC.write(f) };
}

/// `startclock_func = f`.
pub fn set_startclock_func(f: fn()) {
    // SAFETY: as for `set_initclock_func`.
    unsafe { STARTCLOCK_FUNC.write(f) };
}

/// `initclock_func == i8254_initclocks`: whether the i8254 and the RTC drive the clocks.
pub fn initclock_is_i8254() -> bool {
    // SAFETY: read after autoconfiguration set it, or the static default.
    core::ptr::fn_addr_eq(unsafe { INITCLOCK_FUNC.read() }, i8254_initclocks as fn())
}

/// `cpu_initclocks`.
pub fn cpu_initclocks() {
    // SAFETY: as for `initclock_is_i8254`.
    (unsafe { INITCLOCK_FUNC.read() })();
}

/// `cpu_startclock`.
pub fn cpu_startclock() {
    // SAFETY: as for `initclock_is_i8254`.
    (unsafe { STARTCLOCK_FUNC.read() })();
}

/// `need_resched`: asks `ci` to reschedule; another CPU is kicked into the kernel.
pub fn need_resched(ci: &CpuInfo) {
    ci.ci_want_resched.store(1, Ordering::SeqCst);

    // There's a risk we'll be called before the idle threads start
    // SAFETY: `ci_curproc` names a thread on the CPU, hence alive (the caller holds the
    // scheduler lock, which keeps it from exiting).
    if let Some(p) = unsafe { ci.ci_curproc.get().as_ref() } {
        aston(p);
        cpu_kick(ci);
    }
}

/// `aston(p)`: `p->p_md.md_astpending = 1`.
pub fn aston(p: &Proc) {
    p.p_md.md_astpending.store(1, Ordering::Relaxed);
}

/// `clear_resched(ci)`.
pub fn clear_resched(ci: &CpuInfo) {
    ci.ci_want_resched.store(0, Ordering::SeqCst);
}

/// `cpu_unidle(ci)`: with `MULTIPROCESSOR` an IPI to wake another CPU's idle loop (no
/// `mwait`: `cpu_init_mwait` is not ported, so `MWAIT_ONLY` is never set); on one CPU the
/// idle loop sees the run queue itself.
pub fn cpu_unidle(_ci: &CpuInfo) {
    #[cfg(feature = "multiprocessor")]
    if !ptr::eq(_ci, curcpu()) {
        crate::arch::amd64::amd64::ipi::x86_send_ipi(
            _ci,
            crate::arch::amd64::include::intrdefs::X86_IPI_NOP,
        );
    }
}

/// `cpu_idle_cycle_hlt`: `sti; hlt`, what `cpu_idle_cycle_fcn` points at by default.
pub fn cpu_idle_cycle_hlt() {
    // SAFETY: enabling interrupts and halting until one arrives is what the idle thread is
    // for; `sti` takes effect after `hlt`, so no interrupt is lost in between.
    unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
}

/// `cpu_idle_cycle()`: `(*cpu_idle_cycle_fcn)()`, the `hlt` loop until a driver (acpicpu)
/// installs `mwait`.
pub fn cpu_idle_cycle() {
    cpu_idle_cycle_hlt();
}

/// `setgate`: fills an interrupt or trap gate for `func` with `ist`, `type_`, `dpl` and the
/// code selector `sel`.
pub fn setgate(gd: &mut GateDescriptor, func: usize, ist: u8, type_: u8, dpl: u16, sel: u16) {
    *gd = GateDescriptor::pack(func as u64, sel, ist, type_, dpl as u8, true);
}

/// `unsetgate`: clears a gate.
pub fn unsetgate(gd: &mut GateDescriptor) {
    *gd = GateDescriptor::zeroed();
}

/// `setregion`: fills the `lgdt`/`lidt` operand.
pub fn setregion(rd: &mut RegionDescriptor, base: usize, limit: u16) {
    rd.rd_limit = limit;
    rd.rd_base = base as u64;
}

/// `set_mem_segment`: fills a memory segment descriptor. Note that the base and limit fields
/// are ignored in long mode.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn set_mem_segment(
    sd: &mut u64,
    base: usize,
    limit: usize,
    type_: u8,
    dpl: u16,
    gran: bool,
    def32: bool,
    is64: bool,
) {
    *sd = MemSegmentDescriptor::pack(
        base as u64,
        limit as u32,
        type_,
        dpl as u8,
        true,
        false,
        is64,
        def32,
        gran,
    )
    .0;
}

/// `set_sys_segment`: fills a 16-byte system segment descriptor (`sd[0]`, `sd[1]`).
pub fn set_sys_segment(sd: &mut [u64], base: usize, limit: usize, type_: u8, dpl: u16, gran: bool) {
    let d = SysSegmentDescriptor::pack(base as u64, limit as u32, type_, dpl as u8, true, gran);
    sd[0] = d.lo;
    sd[1] = d.hi;
}

/// `cpu_init_idt`: loads this CPU's IDT register with the kernel's table.
pub fn cpu_init_idt() {
    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(
        &mut region,
        IDT.as_ptr() as usize,
        (NIDT * size_of::<GateDescriptor>() - 1) as u16,
    );
    // SAFETY: the IDT is a static page whose gates init_x86_64 filled; it lives forever.
    unsafe { lidt(&region) };
}

/// `idt_vec_alloc`: takes the first free vector in `low..=high`, or 0 if none.
pub fn idt_vec_alloc(low: i32, high: i32) -> i32 {
    for vec in low..=high {
        if !IDT_ALLOCMAP[vec as usize].swap(true, Ordering::Relaxed) {
            return vec;
        }
    }
    0
}

/// `idt_vec_alloc_range`: takes `num` (a power of two) aligned consecutive free vectors in
/// `low..=high`, or 0 if none.
pub fn idt_vec_alloc_range(low: i32, high: i32, num: i32) -> i32 {
    kassert!(num > 0 && num & (num - 1) == 0);
    let low = (low + num - 1) & !(num - 1);
    let high = ((high + 1) & !(num - 1)) - 1;

    let mut vec = low;
    while vec <= high {
        let free = (0..num).all(|i| !IDT_ALLOCMAP[(vec + i) as usize].load(Ordering::Relaxed));
        if free {
            for i in 0..num {
                IDT_ALLOCMAP[(vec + i) as usize].store(true, Ordering::Relaxed);
            }
            return vec;
        }
        vec += num;
    }
    0
}

/// `idt_vec_set`: points an allocated vector at `function`.
pub fn idt_vec_set(vec: i32, function: usize) {
    // Vector should be allocated, so no locking needed.
    kassert!(IDT_ALLOCMAP[vec as usize].load(Ordering::Relaxed));
    // SAFETY: the vector is allocated, so nothing else writes its gate; the CPU reads the
    // table on the next interrupt.
    let idt = unsafe { IDT.get_mut() };
    setgate(
        &mut idt.0[vec as usize],
        function,
        0,
        SDT_SYS386IGT,
        SEL_KPL,
        gsel(GCODE_SEL, SEL_KPL),
    );
}

/// `idt_vec_free`: clears a vector's gate and frees it.
pub fn idt_vec_free(vec: i32) {
    // SAFETY: as for `idt_vec_set`; the caller no longer expects the vector to fire.
    let idt = unsafe { IDT.get_mut() };
    unsetgate(&mut idt.0[vec as usize]);
    IDT_ALLOCMAP[vec as usize].store(false, Ordering::Relaxed);
}

/// `amd64_delay_quality`: the quality of the current `delay_func`.
static AMD64_DELAY_QUALITY: AtomicI32 = AtomicI32::new(0);

/// `delay_init`: makes `f` the `delay(9)` implementation if its quality beats the current one.
pub fn delay_init(f: fn(i32), fn_quality: i32) {
    if fn_quality > AMD64_DELAY_QUALITY.load(Ordering::Relaxed) {
        // SAFETY: called on the boot CPU during autoconfiguration (tsc_identify, the timer
        // drivers' attach), when nothing runs concurrently with the write.
        unsafe { DELAY_FUNC.write(f) };
        AMD64_DELAY_QUALITY.store(fn_quality, Ordering::Relaxed);
    }
}

/// `delay_fini`: goes back to `i8254_delay` if `f` is the current `delay_func`.
pub fn delay_fini(f: fn(i32)) {
    // SAFETY: as for `delay_init`.
    if core::ptr::fn_addr_eq(unsafe { DELAY_FUNC.read() }, f) {
        // SAFETY: as for `delay_init`.
        unsafe { DELAY_FUNC.write(i8254_delay) };
        AMD64_DELAY_QUALITY.store(0, Ordering::Relaxed);
    }
}

/// `bios_sysctl`: the `machdep.bios` tree. The C answers `EOPNOTSUPP` unless the boot loader
/// passed a vector of boot arguments (`bootapiver & BAPIV_VECTOR`); Limine passes none (this
/// kernel has no `bootapiver`, `bootdev` or `bios_diskinfo`), so `BIOS_DEV`, `BIOS_DISKINFO`
/// and `BIOS_CKSUMLEN` are unreachable here and every name is `EOPNOTSUPP`.
pub fn bios_sysctl(name: &[i32]) -> Result<(), Errno> {
    if name.is_empty() {
        return Err(Errno::ENOTDIR);
    }
    // bootapiver & BAPIV_VECTOR is 0 under Limine; the C's switch (BIOS_DEV, BIOS_DISKINFO,
    // BIOS_CKSUMLEN) comes after that test and cannot run without bios_diskinfo.
    let _ = (BIOS_DEV, BIOS_DISKINFO, BIOS_CKSUMLEN);
    Err(Errno::EOPNOTSUPP)
}

/// `cpuctl_vars[]`: the `machdep` integers `sysctl_bounded_arr` serves. The read-only
/// unsigned ones (`cpu_id`, `cpu_feature`) are `CPU_CPUID` and `CPU_CPUFEATURE` in
/// `cpu_sysctl`, since a `u32` is not the `int` the table points at.
static CPUCTL_VARS: [SysctlBoundedArgs; 6] = [
    SysctlBoundedArgs::new(CPU_HIBERNATEDELAY, &HIBERNATE_DELAY, 0, 86400),
    SysctlBoundedArgs::new(CPU_LIDACTION, &LID_ACTION, -1, 2),
    SysctlBoundedArgs::new(CPU_PWRACTION, &PWR_ACTION, 0, 2),
    SysctlBoundedArgs::readonly(CPU_XCRYPT, &AMD64_HAS_XCRYPT),
    SysctlBoundedArgs::readonly(CPU_INVARIANTTSC, &TSC_IS_INVARIANT),
    SysctlBoundedArgs::readonly(CPU_RETPOLINE, &NEED_RETPOLINE),
];

/// `vmmode`: the `machdep.vmmode` string (NUL-terminated) for `ecxfeature` (`cpu_ecxfeature`)
/// and `sev_guestmode` (`cpu_sev_guestmode`).
pub fn vmmode(ecxfeature: u32, sev_guestmode: i32) -> &'static [u8] {
    if ecxfeature & CPUIDECX_HV != 0 {
        if sev_guestmode & SEV_STAT_SNP_ACTIVE != 0 {
            b"SEV-SNP\0"
        } else if sev_guestmode & SEV_STAT_ES_ENABLED != 0 {
            b"SEV-ES\0"
        } else if sev_guestmode & SEV_STAT_ENABLED != 0 {
            b"SEV\0"
        } else {
            b"guest\0"
        }
    } else {
        b"host\0"
    }
}

/// `cpu_sysctl`: machine dependent system variables.
pub fn cpu_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    let Some(&first) = name.first() else {
        return Err(Errno::ENOTDIR);
    };
    match first {
        CPU_CONSDEV => {
            if name.len() != 1 {
                return Err(Errno::ENOTDIR); // overloaded
            }
            let consdev = cn_tab().map_or(NODEV, |cn| cn.cn_dev.get());
            sysctl_rdstruct(oldp, oldlenp, newp, &consdev.to_ne_bytes())
        }
        CPU_CHR2BLK => {
            if name.len() != 2 {
                return Err(Errno::ENOTDIR); // overloaded
            }
            let dev = chrtoblk(name[1]);
            sysctl_rdstruct(oldp, oldlenp, newp, &dev.to_ne_bytes())
        }
        CPU_BIOS => bios_sysctl(&name[1..]),
        CPU_CPUVENDOR => {
            // SAFETY: written once by init_x86_64 on the boot CPU before anything runs
            // that can reach sysctl(2).
            let vendor = unsafe { CPU_VENDOR.get() };
            sysctl_rdstring(oldp, oldlenp, newp, vendor)
        }
        CPU_KBDRESET => sysctl_securelevel_int(oldp, oldlenp, newp, newlen, &KBD_RESET),
        CPU_ALLOWAPERTURE => {
            if name.len() != 1 {
                return Err(Errno::ENOTDIR); // overloaded
            }
            // APERTURE is not configured: the C's #else.
            sysctl_rdint(oldp, oldlenp, newp, 0)
        }
        // CPU_FORCEUKBD: `NPCKBC > 0 && NUKBD > 0` is false (pckbc(4) is not ported), so
        // the case is compiled out as in C and falls to the table, which does not have it.
        CPU_TSCFREQ => sysctl_rdquad(
            oldp,
            oldlenp,
            newp,
            TSC_FREQUENCY.load(Ordering::Relaxed) as i64,
        ),
        CPU_VMMODE => {
            let mode = vmmode(
                CPU_ECXFEATURE.load(Ordering::Relaxed),
                CPU_SEV_GUESTMODE.load(Ordering::Relaxed),
            );
            sysctl_rdstring(oldp, oldlenp, newp, mode)
        }
        CPU_CPUID if name.len() == 1 => {
            sysctl_rdint(oldp, oldlenp, newp, CPU_ID.load(Ordering::Relaxed) as i32)
        }
        CPU_CPUFEATURE if name.len() == 1 => sysctl_rdint(
            oldp,
            oldlenp,
            newp,
            CPU_FEATURE.load(Ordering::Relaxed) as i32,
        ),
        CPU_CPUID | CPU_CPUFEATURE => Err(Errno::ENOTDIR),
        _ => sysctl_bounded_arr(&CPUCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}
