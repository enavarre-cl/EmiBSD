/*	$OpenBSD: mainbus.c,v 1.54 2025/09/16 12:18:10 hshoexer Exp $	*/
/*	$NetBSD: mainbus.c,v 1.1 2003/04/26 18:39:29 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
 */
/* </LICENSES> */

//! The amd64 root bus: `arch/amd64/amd64/mainbus.c`.
//!
//! Upstream: sys/arch/amd64/amd64/mainbus.c @ 3ce1f3f79392
//!
//! `mainbus0 at root` attaches first (`cpu_configure`'s `config_rootfound`) and attaches, in
//! the C's order, the BIOS (and through it ACPI and the MP tables), IPMI, the boot CPU when
//! nothing has attached it yet, the paravirtual bus, PCI, ISA, `vmm` and the EFI framebuffer.
//!
//! ## Deviations
//! - Only the `cpu`, `bios`, `pci` and `isa` children exist (`sys/arch/amd64/conf/ioconf.rs`);
//!   every other child GENERIC configures is reported with `unported!` where the C would
//!   probe or attach it: `ipmi_probe`, `pvbus_probe`,
//!   `vmm_enabled`, `efifb`; so are `replacemds`, `setperf_setup` and `codepatch_disable`.
//!   No PCI-ISA bridge driver (`pcib`) exists, so `isa0` attaches here, as the C does when
//!   none has.
//!   Without a MADT (`acpimadt`) or MP tables the boot CPU attaches here, as `CPU_ROLE_SP`,
//!   and `pci0` attaches here for bus 0: `acpi_haspci` stays false until `acpipci.c` is
//!   ported, as the C does on a machine whose ACPI names no PCI host bridge it drives.
//! - `MULTIPROCESSOR` (M11a): with no ACPI MADT (M13) and no `mpbios`, the processors the
//!   bootloader found (`BootInfo::mp`, kept as `BOOT_MP`) are the enumeration: mainbus
//!   attaches one `cpu` per processor, the boot processor first as `CPU_ROLE_BP`, the others
//!   as `CPU_ROLE_AP` in the bootloader's order, with the hardware ID as `cpu_apicid` and the
//!   bootloader's processor number as `cpu_acpi_proc_id`, as `acpimadt` would (its children
//!   attach at mainbus too). A uniprocessor kernel, or an MP kernel the bootloader found one
//!   processor for, attaches the boot CPU alone as `CPU_ROLE_SP`, as before.
//! - `pci0`'s attach arguments carry no extents (`sys/extent.h` is not ported, so
//!   `pci_init_extents` is reported and `pciio_ex`, `pcimem_ex`, `pcibus_ex` are NULL).
//! - `union mainbus_attach_args` has the members that exist (`mba_busname`, `mba_caa`,
//!   `mba_pba`); the others come with their buses. `mp_busses`/`mp_intrs`
//!   (`NMPBIOS`/`NACPI`) come with `mpbios`/`acpi`.

use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::arch::amd64::amd64::bios::BiosAttachArgs;
use crate::arch::amd64::amd64::bus_space::{X86_BUS_SPACE_IO, X86_BUS_SPACE_MEM};
use crate::arch::amd64::include::cpu::{CPUF_PRESENT, cpu_info_primary};
use crate::arch::amd64::include::cpuvar::{CPU_ROLE_SP, CpuAttachArgs};
use crate::arch::amd64::pci::pci_machdep::{PCI_BUS_DMA_TAG, pci_init_extents};
use crate::dev::isa::isavar::IsabusAttachArgs;
use crate::dev::pci::pci::PCI_NDOMAINS;
use crate::dev::pci::pcivar::PcibusAttachArgs;
use crate::kern::subr_autoconf::{config_found, device_mainbus};
use crate::kern::subr_prf::{Str, printf};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, UNCONF};
use crate::unported;

/// `union mainbus_attach_args`: what mainbus hands its children. Every member starts with the
/// bus name, which `mainbus_print` reads.
#[repr(C)]
pub union MainbusAttachArgs {
    /// `mba_busname`: first elem of all.
    pub mba_busname: &'static [u8],
    /// `mba_caa`.
    pub mba_caa: CpuAttachArgs,
    /// `mba_pba`.
    pub mba_pba: PcibusAttachArgs,
    /// `mba_iba`.
    pub mba_iba: IsabusAttachArgs,
    /// `mba_bios` (`NBIOS > 0`).
    pub mba_bios: ManuallyDrop<BiosAttachArgs>,
    // aaa_caa (ioapic), mba_iaa (ipmi), mba_pvba, mba_eaa (efifb): with their buses.
}

/// `mainbus_ca`.
pub static MAINBUS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(mainbus_match),
    ca_attach: mainbus_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `mainbus_cd`.
pub static MAINBUS_CD: Cfdriver = Cfdriver::new(b"mainbus", DV_DULL, CD_COCOVM);

/// `isa_has_been_seen`: this is set when the ISA bus is attached. If it's not set by the time
/// it's checked below, then mainbus attempts to attach an ISA.
pub static ISA_HAS_BEEN_SEEN: AtomicI32 = AtomicI32::new(0);

/// `mainbus_match`: probe for the mainbus; always succeeds.
pub fn mainbus_match(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `mainbus_attach`: attach the mainbus.
pub fn mainbus_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    printf(format_args!("\n"));

    // NEFIFB > 0
    let _ = unported!("efifb_cnremap (efifb0 at mainbus?)");

    // NBIOS > 0
    {
        let mut mba = MainbusAttachArgs {
            mba_bios: ManuallyDrop::new(BiosAttachArgs {
                ba_name: b"bios",
                ba_func: 0,
                ba_iot: X86_BUS_SPACE_IO,
                ba_memt: X86_BUS_SPACE_MEM,
                ba_acpipbase: 0,
            }),
        };
        let _ = config_found(self_, ptr::from_mut(&mut mba).cast(), Some(mainbus_print));
    }

    // NIPMI > 0
    let _ = unported!("ipmi_probe (ipmi0 at mainbus?)");

    // MULTIPROCESSOR: the processors the bootloader found stand for acpimadt0's (or
    // mpbios0's) enumeration: the boot processor first, then the others in the bootloader's
    // order (see the module's deviations). One processor attaches as CPU_ROLE_SP below.
    #[cfg(feature = "multiprocessor")]
    mainbus_attach_cpus(self_);

    if cpu_info_primary().ci_flags.load(Ordering::Relaxed) & CPUF_PRESENT == 0 {
        let mut caa = CpuAttachArgs {
            caa_name: b"cpu",
            cpu_apicid: 0,
            cpu_acpi_proc_id: 0,
            cpu_role: CPU_ROLE_SP,
            cpu_func: None,
        };

        let _ = config_found(self_, ptr::from_mut(&mut caa).cast(), Some(mainbus_print));
    }

    // All CPUs are attached, handle MDS
    let _ = unported!("replacemds (the MDS mitigation, cpu.c)");

    // NACPI > 0: if !acpi_hasprocfvs, setperf_setup(&cpu_info_primary), which identifycpu
    // sets.
    let _ = unported!("setperf_setup (identcpu.c)");

    #[cfg(feature = "multiprocessor")]
    let _ = unported!("mp_setperf_init (mp_setperf.c)");

    // NPVBUS > 0: probe first to hide the "not configured" message.
    let _ = unported!("pvbus_probe (pvbus0 at mainbus0)");

    // NPCI > 0, NACPI > 0
    if crate::dev::acpi::acpi::ACPI_HASPCI.load(Ordering::Relaxed) != 0 {
        let _ = unported!("acpipci_attach_busses (acpipci.c)");
    } else {
        pci_init_extents();

        let mut mba = MainbusAttachArgs {
            mba_pba: PcibusAttachArgs {
                pba_busname: b"pci",
                pba_iot: X86_BUS_SPACE_IO,
                pba_memt: X86_BUS_SPACE_MEM,
                pba_dmat: &PCI_BUS_DMA_TAG,
                pba_pc: None,
                pba_flags: 0,
                // pba_ioex = pciio_ex, pba_memex = pcimem_ex, pba_busex = pcibus_ex: NULL
                // (sys/extent.h).
                pba_domain: PCI_NDOMAINS.fetch_add(1, Ordering::Relaxed),
                pba_bus: 0,
                pba_bridgetag: None,
                pba_bridgeih: None,
                pba_intrswiz: 0,
                pba_intrtag: 0,
            },
        };
        let _ = config_found(self_, ptr::from_mut(&mut mba).cast(), Some(mainbus_print));
    }

    // NISA > 0
    if ISA_HAS_BEEN_SEEN.load(Ordering::Relaxed) == 0 {
        let mut mba = MainbusAttachArgs {
            mba_iba: IsabusAttachArgs {
                iba_busname: b"isa",
                iba_iot: X86_BUS_SPACE_IO,
                iba_memt: X86_BUS_SPACE_MEM,
                // NISADMA > 0: iba_dmat = &isa_bus_dma_tag (isadma: not configured).
                iba_ic: ptr::null(),
            },
        };

        let _ = config_found(self_, ptr::from_mut(&mut mba).cast(), Some(mainbus_print));
    }

    // NVMM > 0
    let _ = unported!("vmm_enabled (vmm0 at mainbus0)");

    // NEFIFB > 0
    let _ = unported!("efifb0 at mainbus? (bios_efiinfo, efifb_cb_found)");

    let _ = unported!("codepatch_disable (codepatch.c)");
}

/// The `cpu` children of a `MULTIPROCESSOR` kernel: one per processor of `BOOT_MP`, the
/// boot processor (`CPU_ROLE_BP`) first, then the application processors (`CPU_ROLE_AP`),
/// each with `mp_cpu_funcs`; nothing when the bootloader found a single processor.
#[cfg(feature = "multiprocessor")]
fn mainbus_attach_cpus(self_: &Device) {
    use crate::arch::amd64::amd64::cpu::{BOOT_MP, MP_CPU_FUNCS};
    use crate::arch::amd64::include::cpuvar::{CPU_ROLE_AP, CPU_ROLE_BP};

    // SAFETY: written once by `init_x86_64`, before autoconfiguration reads it.
    let Some(mp) = (unsafe { BOOT_MP.read() }) else {
        return;
    };
    if mp.ncpus < 2 {
        return;
    }

    let attach = |cpu: crate::machine::BootCpu, role: i32| {
        let mut caa = CpuAttachArgs {
            caa_name: b"cpu",
            cpu_apicid: cpu.hwid as i32,
            cpu_acpi_proc_id: cpu.processor_id as i32,
            cpu_role: role,
            cpu_func: Some(&MP_CPU_FUNCS),
        };
        let _ = config_found(self_, ptr::from_mut(&mut caa).cast(), Some(mainbus_print));
    };

    if let Some(bsp) = mp.cpus().find(|c| c.hwid == mp.bsp_hwid) {
        attach(bsp, CPU_ROLE_BP);
    }
    for cpu in mp.cpus().filter(|c| c.hwid != mp.bsp_hwid) {
        // acpimadt_attach counts every application processor in ncpusfound (which starts
        // at 1, the boot processor): percpu(9) sizes its per-CPU arrays with it.
        crate::kern::init_main::NCPUSFOUND.fetch_add(1, Ordering::Relaxed);
        attach(cpu, CPU_ROLE_AP);
    }
}

/// `mainbus_efifb_reattach` (`NEFIFB > 0`): attaches the EFI framebuffer again after a
/// display driver gave it up.
pub fn mainbus_efifb_reattach() {
    if device_mainbus().is_none() {
        return;
    }
    let _ = unported!("efifb0 at mainbus? (bios_efiinfo, efifb_cb_found)");
}

/// `mainbus_print`: names a child that found no driver.
pub fn mainbus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: every mainbus child's attach arguments start with the bus name
    // (`MainbusAttachArgs`, `CpuAttachArgs`), which is all this reads.
    let busname = unsafe { *aux.cast::<&'static [u8]>() };

    if let Some(pnp) = pnp {
        printf(format_args!("{} at {}", Str(busname), Str(pnp)));
    }
    if busname == b"pci" {
        // SAFETY: a "pci" child was handed `mba_pba`, a pcibus_attach_args.
        let pba = unsafe { &*aux.cast::<PcibusAttachArgs>() };
        printf(format_args!(" bus {}", pba.pba_bus));
    }

    UNCONF
}
