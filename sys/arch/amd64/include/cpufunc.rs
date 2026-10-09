/* <LICENSES> */
/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/cpufunc.h>`: access to the x86 instructions the kernel needs.
//!
//! Upstream: sys/arch/amd64/include/cpufunc.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M0 ports the interrupt-flag helpers only (`read_rflags`,
//! `write_rflags`, `intr_enable`, `intr_disable`, `intr_restore`). Descriptor tables, control
//! registers, MSRs, TLB and cache helpers arrive with milestones M3 and M4; the TSC
//! timecounter adds `rdtsc`, `rdtscp` and `rdtsc_lfence`.

use core::arch::asm;

/// `read_rflags`: the current `RFLAGS`.
#[inline]
pub fn read_rflags() -> u64 {
    let ef: u64;
    // SAFETY: `pushfq; pop` reads the flags through the stack and leaves it balanced.
    unsafe { asm!("pushfq", "pop {}", out(reg) ef, options(nomem, preserves_flags)) };
    ef
}

/// `write_rflags`: loads `RFLAGS` from `ef`.
///
/// # Safety
///
/// `ef` must be a value obtained from [`read_rflags`] on this CPU; it carries the interrupt
/// enable flag among others.
#[inline]
pub unsafe fn write_rflags(ef: u64) {
    // SAFETY: `push; popfq` loads the flags through the stack and leaves it balanced; the
    // caller vouches for the value.
    unsafe { asm!("push {}", "popfq", in(reg) ef, options(nomem)) };
}

/// `intr_enable`: enables interrupts.
///
/// # Safety
///
/// Only code that owns the current interrupt level may enable interrupts.
#[inline]
pub unsafe fn intr_enable() {
    // SAFETY: `sti` touches only the interrupt flag; the caller owns the level.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// `intr_disable`: disables interrupts and returns the previous `RFLAGS` for
/// [`intr_restore`].
#[inline]
pub fn intr_disable() -> u64 {
    let ef = read_rflags();
    // SAFETY: masking interrupts is always sound; it only delays their delivery.
    unsafe { asm!("cli", options(nomem, nostack)) };
    ef
}

/// `intr_restore`: restores the interrupt state saved by [`intr_disable`].
///
/// # Safety
///
/// `ef` must come from [`intr_disable`] on this CPU.
#[inline]
pub unsafe fn intr_restore(ef: u64) {
    // SAFETY: forwarded.
    unsafe { write_rflags(ef) };
}

/// `lcr0`: loads `CR0`.
///
/// # Safety
///
/// `val` must keep protected mode and paging on (`CR0_PE`, `CR0_PG`): the kernel runs in long
/// mode.
#[inline]
pub unsafe fn lcr0(val: u64) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("mov cr0, {}", in(reg) val, options(nostack, preserves_flags)) };
}

/// `rcr0`: reads `CR0`.
#[inline]
pub fn rcr0() -> u64 {
    let val: u64;
    // SAFETY: reading CR0 has no side effects.
    unsafe { asm!("mov {}, cr0", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `rcr3`: reads `CR3`, the physical address of the current PML4 (with the PCID bits).
#[inline]
pub fn rcr3() -> u64 {
    let val: u64;
    // SAFETY: reading CR3 has no side effects; the kernel runs at CPL 0, where it is allowed.
    unsafe { asm!("mov {}, cr3", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `rcr4`: reads `CR4`.
#[inline]
pub fn rcr4() -> u64 {
    let val: u64;
    // SAFETY: reading CR4 has no side effects; the kernel runs at CPL 0, where it is allowed.
    unsafe { asm!("mov {}, cr4", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `lcr4`: loads `CR4`.
///
/// # Safety
///
/// `val` must keep the paging bits the running kernel depends on (`CR4_PAE`, `CR4_PGE` as
/// set by the boot loader) and name only features the CPU has.
#[inline]
pub unsafe fn lcr4(val: u64) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("mov cr4, {}", in(reg) val, options(nostack, preserves_flags)) };
}

/// `invlpg`: invalidates the TLB entry for `addr`.
#[inline]
pub fn invlpg(addr: u64) {
    // SAFETY: dropping a TLB entry only costs a refetch; no mapping changes.
    unsafe { asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)) };
}

/// `lcr3`: loads `CR3`.
///
/// # Safety
///
/// `val` must be the physical address of a PML4 (plus PCID bits) that maps the running code
/// and stack.
#[inline]
pub unsafe fn lcr3(val: u64) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("mov cr3, {}", in(reg) val, options(nostack, preserves_flags)) };
}

/// `tlbflush`: reloads `CR3`, dropping the non-global TLB entries.
#[inline]
pub fn tlbflush() {
    // SAFETY: reloading the current CR3 changes no mapping.
    unsafe {
        asm!("mov {tmp}, cr3", "mov cr3, {tmp}", tmp = out(reg) _, options(nostack, preserves_flags))
    };
}

/// `rdmsr`: reads a model-specific register.
///
/// # Safety
///
/// `msr` must exist on this CPU; an unknown one raises `#GP`.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let (hi, lo): (u32, u32);
    // SAFETY: the caller's guarantee; reading an MSR has no side effects.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("edx") hi, out("eax") lo, options(nomem, nostack, preserves_flags))
    };
    (u64::from(hi) << 32) | u64::from(lo)
}

/// `wrmsr`: writes a model-specific register.
///
/// # Safety
///
/// `msr` must exist and `newval` must be a value it accepts; the write changes CPU behaviour.
#[inline]
pub unsafe fn wrmsr(msr: u32, newval: u64) {
    // SAFETY: the caller's guarantee.
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") newval as u32, in("edx") (newval >> 32) as u32, options(nostack, preserves_flags))
    };
}

/// `wbinvd`: writes back and invalidates the caches.
#[inline]
pub fn wbinvd() {
    // SAFETY: flushing the caches loses no data (write-back first); memory is a clobber.
    unsafe { asm!("wbinvd", options(nostack, preserves_flags)) };
}

/// `rdtsc`: reads the time stamp counter.
#[inline]
pub fn rdtsc() -> u64 {
    let (hi, lo): (u32, u32);
    // SAFETY: `rdtsc` reads the counter into edx:eax; the kernel runs at CPL 0, where it is
    // always allowed. Like the C's `asm volatile` without a memory clobber, it orders nothing.
    unsafe {
        asm!("rdtsc", out("edx") hi, out("eax") lo, options(nomem, nostack, preserves_flags))
    };
    (u64::from(hi) << 32) | u64::from(lo)
}

/// `rdtscp`: reads the time stamp counter once all earlier instructions have executed.
#[inline]
pub fn rdtscp() -> u64 {
    let (hi, lo): (u32, u32);
    // SAFETY: as for `rdtsc`; `rdtscp` also loads `IA32_TSC_AUX` into ecx, declared clobbered.
    unsafe {
        asm!("rdtscp", out("edx") hi, out("eax") lo, out("ecx") _, options(nomem, nostack, preserves_flags))
    };
    (u64::from(hi) << 32) | u64::from(lo)
}

/// `rdtsc_lfence`: reads the time stamp counter after an `lfence`, so earlier loads have
/// completed.
#[inline]
pub fn rdtsc_lfence() -> u64 {
    let (hi, lo): (u32, u32);
    // SAFETY: as for `rdtsc`; `lfence` only orders instructions and changes no state.
    unsafe {
        asm!("lfence", "rdtsc", out("edx") hi, out("eax") lo, options(nomem, nostack, preserves_flags))
    };
    (u64::from(hi) << 32) | u64::from(lo)
}

/// `wbinvd_on_all_cpus`: there is one CPU (no `MULTIPROCESSOR`).
#[cfg(not(feature = "multiprocessor"))]
pub fn wbinvd_on_all_cpus() -> i32 {
    wbinvd();
    0
}

/// `wbinvd_on_all_cpus`: with `MULTIPROCESSOR`, `cpu.c`'s (an IPI to the other CPUs).
#[cfg(feature = "multiprocessor")]
pub use crate::arch::amd64::amd64::cpu::wbinvd_on_all_cpus;

/// `clflush(addr)`: writes the cache line holding `addr` back and invalidates it in every
/// cache of the coherence domain.
#[inline]
pub fn clflush(addr: u64) {
    // SAFETY: `clflush` only writes a line back and drops it from the caches; the data stays
    // the same, so it may name any mapped address. Like the C's `"+m"` operand it is not
    // `nomem`: it must not be moved across the accesses to that line.
    unsafe { asm!("clflush [{}]", in(reg) addr, options(nostack, preserves_flags)) };
}

/// `monitor(addr, extensions, hints)`: arms the monitor on the cache line of `addr`, which a
/// following [`mwait`] waits on.
#[inline]
pub fn monitor(addr: *const u32, extensions: u64, hints: u32) {
    // SAFETY: `monitor` only arms address-range monitoring; it reads no memory and changes
    // no flags. An address that is not write-back memory makes the monitor ineffective, not
    // unsafe.
    unsafe {
        asm!(
            "monitor",
            in("rax") addr,
            in("rcx") extensions,
            in("edx") hints,
            options(nostack, preserves_flags, readonly),
        )
    };
}

/// `mwait(extensions, hints)`: waits, in the C-state `hints` names, for a write to the
/// monitored line or an interrupt; then, as the C does, refills the return stack buffer with
/// 16 harmless entries (eight rounds of two calls whose return addresses are dropped) so that
/// no `ret` after the wait is predicted from entries another thread left.
#[inline]
pub fn mwait(extensions: u64, hints: u32) {
    // SAFETY: `mwait` waits and changes nothing; the stuffing pushes 16 return addresses
    // below `%rsp` (the kernel has no red zone) and pops them with the final `add`, leaving
    // the stack as it was; `%rcx` (the loop counter) is declared clobbered, and `loop` and
    // `add` change the flags, which are not preserved.
    unsafe {
        asm!(
            "mwait",
            "mov rcx, 8",
            ".align 16, 0x90",
            "3: call 5f",
            "4: pause",
            "lfence",
            "call 4b",
            ".align 16, 0xcc",
            "5: call 7f",
            "6: pause",
            "lfence",
            "call 6b",
            ".align 16, 0xcc",
            "7: loop 3b",
            "add rsp, 16*8",
            inout("rcx") extensions => _,
            in("eax") hints,
        )
    };
}

/// `mfence`: orders every earlier load and store before every later one.
#[inline]
pub fn mfence() {
    // SAFETY: a fence changes no state; memory is a clobber (not `nomem`), as the C's.
    unsafe { asm!("mfence", options(nostack, preserves_flags)) };
}

/// `rcr2`: reads `CR2`, the faulting address of the last page fault.
#[inline]
pub fn rcr2() -> u64 {
    let val: u64;
    // SAFETY: reading CR2 has no side effects.
    unsafe { asm!("mov {}, cr2", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `rdr6`: reads debug register 6.
#[inline]
pub fn rdr6() -> u64 {
    let val: u64;
    // SAFETY: reading a debug register has no side effects.
    unsafe { asm!("mov {}, dr6", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `rdr7`: reads debug register 7.
#[inline]
pub fn rdr7() -> u64 {
    let val: u64;
    // SAFETY: as for `rdr6`.
    unsafe { asm!("mov {}, dr7", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}

/// `lidt`: loads the interrupt descriptor table register.
///
/// # Safety
///
/// `p` must point at a region descriptor naming a valid IDT that outlives its use.
#[inline]
pub unsafe fn lidt(p: *const crate::arch::amd64::include::segments::RegionDescriptor) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("lidt [{}]", in(reg) p, options(nostack, preserves_flags)) };
}

/// `ltr`: loads the task register.
///
/// # Safety
///
/// `sel` must select an available TSS descriptor in the current GDT.
#[inline]
pub unsafe fn ltr(sel: u16) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("ltr {0:x}", in(reg) sel, options(nomem, nostack, preserves_flags)) };
}

/// `lldt`: loads the local descriptor table register (0: no LDT).
///
/// # Safety
///
/// `sel` must be 0 or select an LDT descriptor in the current GDT.
#[inline]
pub unsafe fn lldt(sel: u16) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("lldt {0:x}", in(reg) sel, options(nomem, nostack, preserves_flags)) };
}

/// `breakpoint`: `int3`, the debugger's entry.
#[inline]
pub fn breakpoint() {
    // SAFETY: a breakpoint trap; the IDT's `Xtrap03` hands it to `db_ktrap`, which returns
    // past the instruction.
    unsafe { asm!("int3", options(nomem, nostack, preserves_flags)) };
}

/// `lcr8`: writes `CR8`, the task priority register (0 lets every interrupt through).
///
/// # Safety
///
/// Changing the task priority changes which interrupts the LAPIC delivers.
#[inline]
pub unsafe fn lcr8(val: u64) {
    // SAFETY: the caller's guarantee.
    unsafe { asm!("mov cr8, {}", in(reg) val, options(nomem, nostack, preserves_flags)) };
}
/* </CODE> */
