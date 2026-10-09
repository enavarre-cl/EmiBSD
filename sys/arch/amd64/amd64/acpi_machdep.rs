/* <LICENSES> */
/*
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
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
//! The amd64 half of acpi(4): `arch/amd64/amd64/acpi_machdep.c`. acpi0 at bios0
//! (`acpi_match`, `acpi_attach`, `acpi_probe` and its RSDP scan), the physical mappings of
//! the firmware tables (`acpi_map`/`acpi_unmap`), the register mappings of the AML
//! interpreter, the global lock's compare and swap, and `acpi_attach_machdep` (the SCI and
//! the reset hook). Generic code reaches it through `machine::acpi_machdep`.
//!
//! Upstream: sys/arch/amd64/amd64/acpi_machdep.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `acpi_intr_establish` (M13: through the I/O APIC, `NIOAPIC > 0`) leaks its
//!   `mp_intr_map` (`Box`), as the C never frees it, and returns NULL for a pin past the
//!   I/O APIC's where the C would write past `sc_pins`. The SCI does not use it:
//!   `acpi_attach_machdep` establishes it on its ISA line (`isa_intr_establish`, through the
//!   ISA bus's I/O APIC mapping), as the C does.
//! - `acpi_attach_machdep`'s wakeup trampoline (`acpi_real_mode_resume`, `acpi_tramp_data`,
//!   `acpi_pdirpa`: `acpi_wakecode.S`) and `acpi_sleep_cpu`, `acpi_resume_cpu`, `sleep_mp`
//!   and `resume_mp` (`SUSPEND`: S3 and hibernation) are not ported: the trampoline copy is
//!   reported when acpi0 attaches, and the four functions report themselves (sleeping
//!   fails with `ENOSYS`). No caller reaches them before `acpi_x86.c` and `subr_suspend.c`.
//! - `acpi_map` returns the mapping (the C fills a handle and returns 0 or `ENOMEM`).
//! - `acpi_scan` maps `sizeof(struct acpi_rsdp)` bytes past the area it searches, so that
//!   a candidate near its end is read within the mapping (the C reads on into the mapped
//!   page).

use alloc::boxed::Box;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::arch::amd64::amd64::bios::BiosAttachArgs;
use crate::arch::amd64::amd64::bus_space::{
    BusSpaceHandle, X86BusSpace, bus_space_map, bus_space_unmap,
};
use crate::arch::amd64::amd64::intr::intr_establish;
use crate::arch::amd64::amd64::ioapic::ioapic_find_bybase;
use crate::arch::amd64::amd64::machdep::CPURESETFN;
use crate::arch::amd64::amd64::pmap::{pmap_kenter_pa, pmap_kernel, pmap_kremove};
use crate::arch::amd64::include::i82093reg::{
    IOAPIC_REDLO_ACTLO, IOAPIC_REDLO_DEL_LOPRI, IOAPIC_REDLO_DEL_SHIFT, IOAPIC_REDLO_LEVEL,
};
use crate::arch::amd64::include::intrdefs::IST_EDGE;
use crate::arch::amd64::include::mpbiosreg::{
    MPS_INTPO_ACTHI, MPS_INTPO_ACTLO, MPS_INTPO_DEF, MPS_INTPO_MASK, MPS_INTPO_SHIFT,
    MPS_INTTR_DEF, MPS_INTTR_EDGE, MPS_INTTR_LEVEL, MPS_INTTR_MASK, MPS_INTTR_SHIFT,
};
use crate::arch::amd64::include::param::{NBPG, PGOFSET};
use crate::arch::amd64::isa::isa_machdep::isa_intr_establish;
use crate::arch::amd64::pci::pci_machdep::PCI_BUS_DMA_TAG;
use crate::dev::acpi::acpi::{acpi_attach_common, acpi_interrupt, acpi_reset};
use crate::dev::acpi::acpireg::{AcpiRsdp, AcpiRsdp1, RSDP_SIG};
use crate::dev::acpi::acpiutil::acpi_checksum;
use crate::dev::acpi::acpivar::{AcpiMemMap, AcpiSoftc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::dev::acpi::dsdt::{LR_EXTIRQ_MODE, LR_EXTIRQ_POLARITY};
use crate::dev::isa::isareg::IOM_BEGIN;
use crate::machine::bus::{BusAddr, BusDmaTag, BusSize};
use crate::machine::intr::{IPL_BIO, IPL_WAKEUP};
use crate::machine::isa_machdep::IST_LEVEL;
use crate::machine::mpconfig::MpIntrMap;
use crate::machine::pmap::pmap_update;
use crate::sys::device::{CfMatch, Cfattach, Device};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_km::{KD_NOWAIT, KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `ACPI_BIOS_RSDP_WINDOW_BASE`: the BIOS area searched for the RSDP.
const ACPI_BIOS_RSDP_WINDOW_BASE: usize = 0xe0000;
/// `ACPI_BIOS_RSDP_WINDOW_SIZE`.
const ACPI_BIOS_RSDP_WINDOW_SIZE: usize = 0x20000;

/// `acpi_ca`.
pub static ACPI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpiSoftc>(),
    ca_match: Some(acpi_match),
    ca_attach: acpi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpi_match(parent, match, aux)`: acpi0 at bios0, when there is an RSDP.
pub fn acpi_match(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: bios0 hands its children `struct bios_attach_args`.
    let ba = unsafe { &mut *aux.cast::<BiosAttachArgs>() };
    let cf = match_.cfdata();

    // sanity
    if ba.ba_name != cf.cf_driver.cd_name {
        return 0;
    }

    if acpi_probe(parent, ba) == 0 {
        return 0;
    }

    1
}

/// `acpi_attach(parent, self, aux)`.
pub fn acpi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `acpi_ca`'s softc is an `AcpiSoftc` (`ca_devsize`).
    let sc: &AcpiSoftc = unsafe { self_.softc() };
    // SAFETY: as in `acpi_match`.
    let ba = unsafe { &*aux.cast_const().cast::<BiosAttachArgs>() };
    // SAFETY: a softc `config_attach` allocated is never freed (acpi0 does not detach).
    let sc: &'static AcpiSoftc = unsafe { &*ptr::from_ref(sc) };

    sc.sc_iot.set(Some(ba.ba_iot));
    sc.sc_memt.set(Some(ba.ba_memt));
    sc.sc_cc_dmat.set(Some(&PCI_BUS_DMA_TAG));
    sc.sc_ci_dmat.set(Some(&PCI_BUS_DMA_TAG));

    acpi_attach_common(sc, Paddr::new(ba.ba_acpipbase));
}

/// `acpi_map(pa, len, handle)`: maps `len` bytes of physical memory at `pa` read-write into
/// fresh kernel virtual space.
pub fn acpi_map(pa: Paddr, len: usize) -> Result<AcpiMemMap, Errno> {
    let pa = pa.as_usize();
    let mut pgpa = trunc_page(pa);
    let endpa = round_page(pa + len);
    let Some(va) = km_alloc(endpa - pgpa, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    let mut va = va.as_ptr() as usize;

    let handle = AcpiMemMap {
        baseva: va,
        va: va + (pa & PGOFSET),
        vsize: endpa - pgpa,
        pa,
    };

    loop {
        // SAFETY: `va` is fresh kernel virtual space from km_alloc(kv_any, kp_none) that
        // nothing else maps; `pgpa` is firmware memory the caller names.
        unsafe { pmap_kenter_pa(Vaddr::new(va), Paddr::new(pgpa), PROT_READ | PROT_WRITE) };
        va += NBPG;
        pgpa += NBPG;
        if pgpa >= endpa {
            break;
        }
    }
    pmap_update(pmap_kernel());

    Ok(handle)
}

/// `acpi_unmap(handle)`.
pub fn acpi_unmap(handle: &AcpiMemMap) {
    // SAFETY: `acpi_map` entered these pages; the caller is done with them.
    unsafe { pmap_kremove(Vaddr::new(handle.baseva), Vsize::new(handle.vsize)) };
    pmap_update(pmap_kernel());
    if let Some(va) = NonNull::new(handle.baseva as *mut u8) {
        km_free(va, handle.vsize, &KV_ANY, &KP_NONE);
    }
}

/// `acpi_bus_space_map(t, addr, size, flags, &bsh)`: `_bus_space_map`, without the extent
/// check (which `bus_space_map` does not make here either).
///
/// # Safety
///
/// As for `bus_space_map`.
pub unsafe fn acpi_bus_space_map(
    t: X86BusSpace,
    addr: BusAddr,
    size: BusSize,
    flags: i32,
) -> Result<BusSpaceHandle, Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { bus_space_map(t, addr, size, flags as u32) }
}

/// `acpi_bus_space_unmap(t, bsh, size)`.
pub fn acpi_bus_space_unmap(t: X86BusSpace, bsh: BusSpaceHandle, size: BusSize) {
    bus_space_unmap(t, bsh, size);
}

/// `acpi_intr_establish(irq, flags, level, handler, arg, what)`: through the I/O APIC that
/// serves global interrupt `irq`; NULL without one.
pub fn acpi_intr_establish(
    irq: i32,
    flags: i32,
    level: i32,
    handler: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<c_void>> {
    // NIOAPIC > 0
    let apic = ioapic_find_bybase(irq)?;

    let mut map = MpIntrMap::new();
    map.ioapic = Some(apic);
    map.ioapic_pin = irq - apic.sc_apic_vecbase.get();
    map.bus_pin = irq;
    if flags & i32::from(LR_EXTIRQ_POLARITY) != 0 {
        map.flags |= MPS_INTPO_ACTLO << MPS_INTPO_SHIFT;
    } else {
        map.flags |= MPS_INTPO_ACTHI << MPS_INTPO_SHIFT;
    }
    if flags & i32::from(LR_EXTIRQ_MODE) != 0 {
        map.flags |= MPS_INTTR_EDGE << MPS_INTTR_SHIFT;
    } else {
        map.flags |= MPS_INTTR_LEVEL << MPS_INTTR_SHIFT;
    }

    map.redir = IOAPIC_REDLO_DEL_LOPRI << IOAPIC_REDLO_DEL_SHIFT;
    let po = (map.flags >> MPS_INTPO_SHIFT) & MPS_INTPO_MASK;
    if po == MPS_INTPO_DEF || po == MPS_INTPO_ACTLO {
        map.redir |= IOAPIC_REDLO_ACTLO;
    }
    let tr = (map.flags >> MPS_INTTR_SHIFT) & MPS_INTTR_MASK;
    if tr == MPS_INTTR_DEF || tr == MPS_INTTR_LEVEL {
        map.redir |= IOAPIC_REDLO_LEVEL;
    }

    let pin = map.ioapic_pin;
    let pp = apic.pins().get(pin as usize)?;
    let map: &'static MpIntrMap = Box::leak(Box::new(map));
    pp.ip_map.set(Some(map));

    let type_ = if flags & i32::from(LR_EXTIRQ_MODE) != 0 {
        IST_EDGE
    } else {
        IST_LEVEL
    };
    // SAFETY: an attached I/O APIC's pic is initialised before it is in `ioapics`.
    let pic = unsafe { apic.pic() };
    intr_establish(-1, pic, pin, type_, level, None, handler, arg, what).map(NonNull::cast)
}

/// `acpi_intr_disestablish(cookie)`.
///
/// # Safety
///
/// `cookie` came from an establish and is not used afterwards.
pub unsafe fn acpi_intr_disestablish(cookie: NonNull<c_void>) {
    // SAFETY: the caller's guarantee.
    unsafe { crate::arch::amd64::amd64::intr::intr_disestablish(cookie.cast()) };
}

/// `acpi_scan(handle, pa, len)`: looks for a valid RSDP in `len` bytes at `pa`, on 16-byte
/// boundaries; on success the area stays mapped in `handle` and the RSDP's address in it is
/// returned.
pub fn acpi_scan(pa: Paddr, len: usize) -> Option<(AcpiMemMap, usize)> {
    // The last candidate's RSDP extends past `len` (the C reads it from the same mapped
    // page): map that much more.
    let maplen = len + size_of::<AcpiRsdp>();
    let handle = acpi_map(pa, maplen).ok()?;
    // SAFETY: `acpi_map` mapped `maplen` bytes at `va`.
    let area = unsafe { core::slice::from_raw_parts(handle.va as *const u8, maplen) };
    let mut i = 0;
    while i < len {
        let p = &area[i..];
        i += 16;

        // is there a valid signature?
        if !p.starts_with(RSDP_SIG) {
            continue;
        }

        // is the checksum valid?
        let Some(r1) = p.get(..size_of::<AcpiRsdp1>()) else {
            continue;
        };
        if acpi_checksum(r1) != 0 {
            continue;
        }

        // check the extended checksum as needed
        // SAFETY: `r1` holds a whole (packed) `AcpiRsdp1`.
        let rsdp = unsafe { ptr::read_unaligned(r1.as_ptr().cast::<AcpiRsdp1>()) };
        if rsdp.revision == 0
            || ((2..=4).contains(&rsdp.revision)
                && p.get(..size_of::<AcpiRsdp>())
                    .is_some_and(|r| acpi_checksum(r) == 0))
        {
            return Some((handle, i - 16));
        }
    }
    acpi_unmap(&handle);

    None
}

/// `acpi_probe(parent, match, ba)`: finds the RSDP: where the parent says (EFI), in the
/// EBDA, or in the BIOS area; records it in `ba_acpipbase`; nonzero when found.
pub fn acpi_probe(_parent: Option<&Device>, ba: &mut BiosAttachArgs) -> i32 {
    // First try to scan the ACPI table passed by parent if any
    if ba.ba_acpipbase != 0 {
        if let Some((handle, _)) = acpi_scan(Paddr::new(ba.ba_acpipbase), 16) {
            acpi_unmap(&handle);
            return 1;
        }
        ba.ba_acpipbase = 0;
    }

    // Next try to find ACPI table entries in the EBDA
    let mut found = None;
    match acpi_map(Paddr::new(0), NBPG) {
        Err(_) => {
            crate::kprintf!("acpi: failed to map BIOS data area\n");
        }
        Ok(handle) => {
            // SAFETY: the first page is mapped; the EBDA segment is the word at 0x40e.
            let ebda = unsafe { ptr::read_unaligned((handle.va + 0x40e) as *const u16) };
            let ebda = usize::from(ebda) << 4;
            acpi_unmap(&handle);

            if ebda != 0 && ebda < IOM_BEGIN {
                found = acpi_scan(Paddr::new(ebda), 1024);
            }
        }
    }

    // Next try to find the ACPI table entries in the BIOS memory
    if found.is_none() {
        found = acpi_scan(
            Paddr::new(ACPI_BIOS_RSDP_WINDOW_BASE),
            ACPI_BIOS_RSDP_WINDOW_SIZE,
        );
    }

    let Some((handle, off)) = found else {
        return 0;
    };

    // havebase:
    ba.ba_acpipbase = handle.pa + off;
    acpi_unmap(&handle);

    1
}

/// `acpi_acquire_glk(lock)`: acquires the global lock. If busy, sets the pending bit; the
/// caller will wait for notification from the BIOS that the lock is available and then
/// attempt to acquire it again.
///
/// # Safety
///
/// `lock` points at the mapped FACS's `global_lock`.
pub unsafe fn acpi_acquire_glk(lock: *mut u32) -> i32 {
    // SAFETY: the caller's guarantee.
    unsafe { crate::machine::acpi_machdep::acpi_glk_cas(lock, true) }
}

/// `acpi_release_glk(lock)`: releases the global lock, returning whether there is a waiter
/// pending. If the BIOS set the pending bit, OSPM must notify the BIOS when it releases the
/// lock.
///
/// # Safety
///
/// As for [`acpi_acquire_glk`].
pub unsafe fn acpi_release_glk(lock: *mut u32) -> i32 {
    // SAFETY: the caller's guarantee.
    unsafe { crate::machine::acpi_machdep::acpi_glk_cas(lock, false) }
}

/// `acpi_attach_machdep(sc)`: the SCI on its ISA line, `cpuresetfn = acpi_reset`, and the
/// wakeup trampoline.
pub fn acpi_attach_machdep(sc: &'static AcpiSoftc) {
    // SAFETY: `acpi_attach_common` set `sc_fadt` (a copy at least as long as the structure)
    // before calling here.
    let sci_int = unsafe { (*sc.sc_fadt.get()).sci_int };

    let ih = isa_intr_establish(
        ptr::null(),
        i32::from(sci_int),
        IST_LEVEL,
        IPL_BIO | IPL_WAKEUP,
        acpi_interrupt,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.sc_dev.xname(),
    );
    sc.sc_interrupt
        .set(ih.map_or(ptr::null_mut(), |p| p.as_ptr().cast()));
    // SAFETY: written once, during autoconfiguration, before anything resets.
    unsafe { CPURESETFN.write(Some(acpi_reset)) };

    // !SMALL_KERNEL: copy acpi_real_mode_resume and acpi_tramp_data to ACPI_TRAMPOLINE and
    // ACPI_TRAMP_DATA, acpi_pdirpa = tramp_pdirpa (see the deviations).
    let _ = unported!("acpi_attach_machdep: the wakeup trampoline (acpi_wakecode.S)");
}

/// `acpi_sleep_cpu(sc, state)` (`SUSPEND`): saves the processor and enters `state`.
pub fn acpi_sleep_cpu(_sc: &AcpiSoftc, _state: i32) -> Result<(), Errno> {
    Err(unported!(
        "acpi_sleep_cpu (acpi_savecpu, acpi_wakecode.S: S3 suspend)"
    ))
}

/// `acpi_resume_cpu(sc, state)` (`SUSPEND`): repairs the interrupt hardware, clocks and
/// processor after a wake.
pub fn acpi_resume_cpu(_sc: &AcpiSoftc, _state: i32) {
    let _ = unported!("acpi_resume_cpu (S3 resume)");
}

/// `sleep_mp()` (`SUSPEND`, `MULTIPROCESSOR`): halts the other processors before sleeping.
pub fn sleep_mp() {
    let _ = unported!("sleep_mp (S3 suspend)");
}

/// `resume_mp()` (`SUSPEND`, `MULTIPROCESSOR`): restarts them after a wake.
pub fn resume_mp() {
    let _ = unported!("resume_mp (S3 resume)");
}

/// `acpi_iommu_device_map(node, dmat)`: amd64 has no ACPI IOMMU mapping here; the tag
/// stays.
pub fn acpi_iommu_device_map(_node: &AmlNodeRef, dmat: Option<BusDmaTag>) -> Option<BusDmaTag> {
    dmat
}
/* </CODE> */
