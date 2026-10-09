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
//! The application processors' real-mode trampoline (`MULTIPROCESSOR`):
//! `arch/amd64/amd64/mptramp.S`, pulled in from the `.S` file next to this module (the file
//! keeps OpenBSD's licence blocks and layout; `{NAME}` placeholders are what `assym.h` and
//! the headers provide in C).
//!
//! Upstream: sys/arch/amd64/amd64/mptramp.S @ 3ce1f3f79392
//!
//! `map_tramps` (`machdep.rs`) copies `cpu_spinup_trampoline` to `MP_TRAMPOLINE` and the
//! data (`mp_tramp_data_start`: the far jumps, `mp_pdirpa`, the 32- and 64-bit GDTs) to
//! `MP_TRAMP_DATA`; `mp_cpu_start` (`cpu.rs`) points the warm reset vector there and sends
//! INIT and STARTUP. The processor goes from real mode to protected mode, enables PAE, long
//! mode and NX, loads the kernel's PML4 (`mp_pdirpa`, identity-mapping the two pages through
//! `pmap_prealloc_lowmem_ptps`'s tables), jumps to `cpu_spinup_finish` in the higher half,
//! finds its `cpu_info` by local APIC ID, takes its idle pcb's stack, GDT and `%cr3`, and
//! calls `cpu_hatch`.
//!
//! ## Deviations
//! - Used only when the kernel was booted by boot(8): under Limine the bootloader starts the
//!   processors (its MP request; `BootMp::start` releases them into `cpu_hatch_entry`).
//! - AT&T syntax through `global_asm!` without the C preprocessor; `GENTRY`/`END` and the
//!   `_TRMP_LABEL`/`_TRMP_DATA_LABEL`/`_TRMP_DATA_OFFSET` macros are written out at each
//!   label; `addr32` is empty, as the C defines it for clang.
//! - `x2apic_enabled` is a byte (`lapic.rs`'s `AtomicBool`), read with `movzbl` where the C
//!   reads an `int`.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::amd64::cpu::{CPU_INFO, cpu_hatch};
use crate::arch::amd64::amd64::lapic::{LOCAL_APIC, X2APIC_ENABLED};
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::i82489reg::{
    LAPIC_ID, LAPIC_ID_SHIFT, MSR_X2APIC_ID, X2APIC_ID_MASK,
};
use crate::arch::amd64::include::mpbiosvar::{MP_TRAMP_DATA, MP_TRAMPOLINE};
use crate::arch::amd64::include::param::NBPG;
use crate::arch::amd64::include::pcb::Pcb;
use crate::arch::amd64::include::psl::PSL_MBO;
use crate::arch::amd64::include::segments::{GCODE_SEL, GDATA_SEL, GDT_SIZE, SEL_KPL, gsel};
use crate::arch::amd64::include::specialreg::{
    APICBASE_ENABLE_X2APIC, CPUID_NXE, CR0_DEFAULT, CR0_PE, CR4_DEFAULT, EFER_LME, EFER_NXE,
    EFER_SCE, MSR_APICBASE, MSR_EFER,
};

global_asm!(
    include_str!("mptramp.S"),
    MP_TRAMPOLINE = const MP_TRAMPOLINE,
    MP_TRAMP_DATA = const MP_TRAMP_DATA,
    NBPG = const NBPG,
    CR0_PE = const CR0_PE,
    PSL_MBO = const PSL_MBO,
    CR4_DEFAULT = const CR4_DEFAULT,
    MSR_EFER = const MSR_EFER,
    CPUID_NXE = const CPUID_NXE,
    EFER_NXE = const EFER_NXE,
    EFER_LME = const EFER_LME,
    EFER_SCE = const EFER_SCE,
    GSEL_KDATA = const gsel(GDATA_SEL, SEL_KPL),
    GSEL_KCODE = const gsel(GCODE_SEL, SEL_KPL),
    CR0_DEFAULT = const CR0_DEFAULT,
    X2APIC_ENABLED = sym X2APIC_ENABLED,
    MSR_APICBASE = const MSR_APICBASE,
    APICBASE_ENABLE_X2APIC = const APICBASE_ENABLE_X2APIC,
    MSR_X2APIC_ID = const MSR_X2APIC_ID,
    X2APIC_ID_MASK = const X2APIC_ID_MASK,
    LOCAL_APIC = sym LOCAL_APIC,
    LAPIC_ID = const LAPIC_ID,
    LAPIC_ID_SHIFT = const LAPIC_ID_SHIFT,
    CPU_INFO = sym CPU_INFO,
    CPU_INFO_APICID = const offset_of!(CpuInfo, ci_apicid),
    CPU_INFO_IDLE_PCB = const offset_of!(CpuInfo, ci_idle_pcb),
    CPU_INFO_GDT = const offset_of!(CpuInfo, ci_gdt),
    PCB_RSP = const offset_of!(Pcb, pcb_rsp),
    PCB_RBP = const offset_of!(Pcb, pcb_rbp),
    PCB_CR3 = const offset_of!(Pcb, pcb_cr3),
    GDT_SIZE = const GDT_SIZE,
    CPU_HATCH = sym cpu_hatch,
    options(att_syntax)
);

unsafe extern "C" {
    /// `cpu_spinup_trampoline[]`: the real-mode code, copied to `MP_TRAMPOLINE`.
    pub static cpu_spinup_trampoline: [u8; 0];
    /// `cpu_spinup_trampoline_end[]`.
    pub static cpu_spinup_trampoline_end: [u8; 0];
    /// `mp_tramp_data_start[]`: the trampoline's data, copied to `MP_TRAMP_DATA`.
    pub static mp_tramp_data_start: [u8; 0];
    /// `mp_tramp_data_end[]`.
    pub static mp_tramp_data_end: [u8; 0];
    /// `mp_pdirpa`: the PML4 the trampoline loads; an address inside the copy at
    /// `MP_TRAMP_DATA`, valid only while that page is mapped.
    pub static mp_pdirpa: [u8; 0];
}
/* </CODE> */
