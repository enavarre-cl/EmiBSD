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

/* <CODE> */
//! The amd64 root bus: `arch/amd64/amd64/mainbus.c`.
//!
//! Upstream: sys/arch/amd64/amd64/mainbus.c @ 3ce1f3f79392
//!
//! `mainbus0 at root` attaches first (`cpu_configure`'s `config_rootfound`) and attaches, in
//! the C's order, the BIOS (and through it ACPI and the MP tables), IPMI, the boot CPU when
//! nothing has attached it yet, the paravirtual bus, PCI, ISA, `vmm` and the EFI framebuffer.
//!
//! ## Deviations
//! - Only the `cpu`, `bios`, `pci`, `isa`, `ioapic` and `efifb` children exist
//!   (`sys/arch/amd64/conf/ioconf.rs`; `ioapic` attaches here through `acpimadt`); every
//!   other child GENERIC configures is reported with `unported!` where the C would
//!   probe or attach it: `ipmi_probe`, `pvbus_probe`,
//!   `vmm_enabled`; so are `replacemds`, `setperf_setup` and `codepatch_disable`.
//!   No PCI-ISA bridge driver (`pcib`) exists, so `isa0` attaches here, as the C does when
//!   none has.
//!   Without a MADT (`acpimadt`) or MP tables the boot CPU attaches here, as `CPU_ROLE_SP`.
//!   The PCI buses attach here too, through `acpipci_attach_busses` once `acpipci` set
//!   `acpi_haspci` (M13), else `pci0` for bus 0 without MSI, as in C.
//! - Without a MADT (no `acpimadt` set `mp_busses`; a kernel without ACPI) mainbus does
//!   what `acpimadt`/`mpbios` would before the processors attach: `lapic_boot_init` at the
//!   architectural base (`LAPIC_BASE`). With a MADT, `acpimadt` did it with the table's
//!   address, as in C.
//! - `MULTIPROCESSOR` (M11a): with no ACPI MADT and no `mpbios`, the processors the
//!   bootloader found (`BootInfo::mp`, kept as `BOOT_MP`) are the enumeration: mainbus
//!   attaches one `cpu` per processor, the boot processor first as `CPU_ROLE_BP`, the others
//!   as `CPU_ROLE_AP` in the bootloader's order, with the hardware ID as `cpu_apicid` and the
//!   bootloader's processor number as `cpu_acpi_proc_id`, as `acpimadt` would (its children
//!   attach at mainbus too). Since M13 this stand-in runs only without a MADT: `acpimadt0`
//!   attaches the processors from the table (`acpimadt.rs`). A uniprocessor kernel, or an
//!   MP kernel that found one processor, attaches the boot CPU alone as `CPU_ROLE_SP`.
//! - `pci0`'s attach arguments carry no extents (`sys/extent.h` is not ported, so
//!   `pci_init_extents` is reported and `pciio_ex`, `pcimem_ex`, `pcibus_ex` are NULL).
//! - `union mainbus_attach_args` has the members that exist (`mba_busname`, `mba_caa`,
//!   `mba_pba`, `mba_iba`, `mba_eaa`, `mba_bios`); the I/O APICs' `struct apic_attach_args` is handed
//!   to `config_found` directly by `acpimadt` (`mp_attach_ioapic`); the others come with
//!   their buses.
//! - The `mp_*` globals (`NMPBIOS > 0 || NACPI > 0`) are atomics: `mp_busses`/`mp_nbusses`,
//!   `mp_intrs`/`mp_nintrs` and `mp_isa_bus`/`mp_eisa_bus` are set once by `acpimadt`
//!   while cold; [`mp_busses`], [`mp_intrs`] and [`mp_isa_bus`] read them as slices and
//!   references. `mp_verbose` is 0 (`MPVERBOSE` is not configured).

use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::arch::amd64::amd64::bios::BiosAttachArgs;
use crate::arch::amd64::amd64::bus_space::{X86_BUS_SPACE_IO, X86_BUS_SPACE_MEM};
use crate::arch::amd64::amd64::efifb::{efifb_cb_found, efifb_cnremap};
use crate::arch::amd64::amd64::lapic::lapic_boot_init;
use crate::arch::amd64::amd64::machdep::bios_efiinfo;
use crate::arch::amd64::include::cpu::{CPUF_PRESENT, cpu_info_primary};
use crate::arch::amd64::include::cpuvar::{CPU_ROLE_SP, CpuAttachArgs};
use crate::arch::amd64::include::efifbvar::EfifbAttachArgs;
use crate::arch::amd64::include::i82489reg::LAPIC_BASE;
use crate::arch::amd64::pci::pci_machdep::{PCI_BUS_DMA_TAG, pci_init_extents};
use crate::dev::isa::isavar::IsabusAttachArgs;
use crate::dev::pci::pci::PCI_NDOMAINS;
use crate::dev::pci::pcivar::PcibusAttachArgs;
use crate::kern::subr_autoconf::{config_found, device_mainbus};
use crate::kern::subr_prf::{Str, printf};
use crate::machine::mpconfig::{MpBus, MpIntrMap};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, UNCONF};
use crate::sys::types::Paddr;
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
    /// `mba_eaa` (`NEFIFB > 0`).
    pub mba_eaa: EfifbAttachArgs,
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

/// `mp_busses`: the MP configuration's buses, NULL until a table (`acpimadt`) sets them.
pub static MP_BUSSES: AtomicPtr<MpBus> = AtomicPtr::new(ptr::null_mut());
/// `mp_nbusses`.
pub static MP_NBUSSES: AtomicI32 = AtomicI32::new(0);
/// `mp_intrs`: the local APIC interrupt mappings.
pub static MP_INTRS: AtomicPtr<MpIntrMap> = AtomicPtr::new(ptr::null_mut());
/// `mp_nintrs`.
pub static MP_NINTRS: AtomicI32 = AtomicI32::new(0);
/// `mp_isa_bus`.
pub static MP_ISA_BUS: AtomicPtr<MpBus> = AtomicPtr::new(ptr::null_mut());
/// `mp_eisa_bus`.
pub static MP_EISA_BUS: AtomicPtr<MpBus> = AtomicPtr::new(ptr::null_mut());
/// `mp_verbose`: 0 (`MPVERBOSE` is not configured).
pub static MP_VERBOSE: AtomicI32 = AtomicI32::new(0);

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
    efifb_cnremap();

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

    if mp_busses().is_none() {
        // No acpimadt0 (or mpbios0): find the LAPIC as they would (see the module's
        // deviations).
        // TODO(M14): without ACPI, mpbios.c's MP tables give the LAPIC's address.
        lapic_boot_init(Paddr::new(LAPIC_BASE));

        // MULTIPROCESSOR: the processors the bootloader found stand for acpimadt0's (or
        // mpbios0's) enumeration: the boot processor first, then the others in the
        // bootloader's order (see the module's deviations). One processor attaches as
        // CPU_ROLE_SP below.
        #[cfg(feature = "multiprocessor")]
        mainbus_attach_cpus(self_);
    }

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
        crate::arch::amd64::pci::acpipci::acpipci_attach_busses(self_);
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
                // (pci_init_extents makes none).
                pba_ioex: None,
                pba_memex: None,
                pba_pmemex: None,
                pba_busex: None,
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
    if bios_efiinfo().is_some() || efifb_cb_found() {
        let mut mba = MainbusAttachArgs {
            mba_eaa: EfifbAttachArgs { eaa_name: b"efifb" },
        };
        let _ = config_found(self_, ptr::from_mut(&mut mba).cast(), Some(mainbus_print));
    }

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
    let Some(self_) = device_mainbus() else {
        return;
    };
    // SAFETY: devices are never freed; mainbus0 lives for good.
    let self_ = unsafe { self_.as_ref() };

    if bios_efiinfo().is_some() || efifb_cb_found() {
        let mut mba = MainbusAttachArgs {
            mba_eaa: EfifbAttachArgs { eaa_name: b"efifb" },
        };
        let _ = config_found(self_, ptr::from_mut(&mut mba).cast(), Some(mainbus_print));
    }
}

/// `mainbus_print`: names a child that found no driver.
pub fn mainbus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: every mainbus child's attach arguments start with the bus name
    // (`MainbusAttachArgs`, `CpuAttachArgs`, `ApicAttachArgs`), which is all this reads.
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

/// `mp_busses` with `mp_nbusses`, `None` while no table set them.
pub fn mp_busses() -> Option<&'static [MpBus]> {
    let p = MP_BUSSES.load(Ordering::Acquire);
    if p.is_null() {
        return None;
    }
    // SAFETY: `mp_set_busses` stored a `'static` slice's start, after its length.
    Some(unsafe { core::slice::from_raw_parts(p, MP_NBUSSES.load(Ordering::Relaxed) as usize) })
}

/// `mp_isa_bus`.
pub fn mp_isa_bus() -> Option<&'static MpBus> {
    // SAFETY: `mp_set_busses` stored a `'static` reference, or nothing did.
    unsafe { MP_ISA_BUS.load(Ordering::Acquire).as_ref() }
}

/// `mp_eisa_bus` (`NEISA > 0`; nothing sets it: `acpimadt` has no EISA bus).
pub fn mp_eisa_bus() -> Option<&'static MpBus> {
    // SAFETY: as for `mp_isa_bus`.
    unsafe { MP_EISA_BUS.load(Ordering::Acquire).as_ref() }
}

/// `mp_intrs` with `mp_nintrs`; empty while no table set them.
pub fn mp_intrs() -> &'static [MpIntrMap] {
    let p = MP_INTRS.load(Ordering::Acquire);
    if p.is_null() {
        return &[];
    }
    // SAFETY: `mp_set_intrs` stored a `'static` slice's start, after its length.
    unsafe { core::slice::from_raw_parts(p, MP_NINTRS.load(Ordering::Relaxed) as usize) }
}

/// `mp_busses = busses; mp_nbusses = nitems(busses); mp_isa_bus = isa` (`acpimadt`).
pub fn mp_set_busses(busses: &'static [MpBus], isa: &'static MpBus) {
    MP_NBUSSES.store(busses.len() as i32, Ordering::Relaxed);
    MP_BUSSES.store(
        ptr::from_ref(busses).cast::<MpBus>().cast_mut(),
        Ordering::Release,
    );
    MP_ISA_BUS.store(ptr::from_ref(isa).cast_mut(), Ordering::Release);
}

/// `mp_intrs = intrs; mp_nintrs = nitems(intrs)` (`acpimadt`).
pub fn mp_set_intrs(intrs: &'static [MpIntrMap]) {
    MP_NINTRS.store(intrs.len() as i32, Ordering::Relaxed);
    MP_INTRS.store(
        ptr::from_ref(intrs).cast::<MpIntrMap>().cast_mut(),
        Ordering::Release,
    );
}
/* </CODE> */
