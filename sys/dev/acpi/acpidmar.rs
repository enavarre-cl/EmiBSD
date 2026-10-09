/* <LICENSES> */
/*
 * Copyright (c) 2015 Jordan Hargrave <jordan_hargrave@hotmail.com>
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
 * Copyright (c) 2015 Jordan Hargrave <jordan_hargrave@hotmail.com>
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
//! acpidmar(4): `dev/acpi/acpidmar.c` with its header `acpidmar.h`, the DMA remapping
//! units (IOMMUs) the ACPI `DMAR` table (Intel VT-d) or `IVRS` table (AMD-Vi) describes.
//!
//! Upstream: sys/dev/acpi/acpidmar.c @ 3ce1f3f79392
//! Upstream: sys/dev/acpi/acpidmar.h @ 3ce1f3f79392
//!
//! acpi0 offers acpidmar0 every table; it takes the `DMAR` or the `IVRS` one. For VT-d it
//! maps each remapping unit (`DRHD`), allocates its root table, turns queued invalidation on
//! and records the reserved regions (`RMRR`) and ATS roots (`ATSR`); for AMD-Vi it shares one
//! 2 MB device table between the units (`IVHD`), sets up each unit's command buffer, event
//! log and completion-wait word, enables it, and records the unity ranges (`IVMD`). From
//! then on amd64's `pci_probe_device_hook` calls [`acpidmar_pci_hook`] for every PCI
//! function: the function is put in a domain (one per device while domain ids last, then a
//! shared one), its context entry (VT-d) or device table entry (AMD-Vi) points at the
//! domain's page tables, and its `pa_dmat` becomes the domain's `bus_dma` tag. Loading a map
//! through that tag allocates I/O virtual addresses from the domain's extent, maps them
//! page by page and invalidates the IOTLB for them; translation is switched on (VT-d) at the
//! first load. The first megabyte, the MSI window (mapped 1:1), the upstream bridges'
//! windows, the RMRR and IVMD ranges and, for an ISA bridge's domain, the first 16 MB (1:1)
//! stay out of the allocator. Faults are reported by the units' interrupt: an MSI through
//! the unit's own fault-event registers (`dmarpic`) for VT-d, the unit's PCI MSI for
//! AMD-Vi.
//!
//! GENERIC (amd64) has `acpidmar0 at acpi? disable`, so the driver attaches only when it is
//! enabled with `boot -c` (`UKC> enable acpidmar`).
//!
//! The driver uses x86 machine headers directly (`struct pic`, `intr_establish`, the inside
//! of `struct bus_dma_tag` and the `_bus_dma*` functions, `PMAP_DIRECT_MAP`,
//! `pmap_flush_cache`, `bios_memmap`): its items are compiled where cfg `machine_x86` is
//! set (amd64), through `machine::x86` (docs/ARCHITECTURE.md, "machine_x86"). The header's
//! registers, entries and helpers, the source-id arithmetic, the device scope parser and
//! the invalidation-range arithmetic are compiled everywhere, for the host tests.
//!
//! ## Deviations
//! - The hardware tables' entries (`struct root_entry`, `context_entry`, `pte_entry`) are
//!   `AtomicU64` words read and written `Relaxed`: several CPUs walk and extend a domain's
//!   page tables (the C rechecks under `ptlck`), and the IOMMU reads them. The header's
//!   inline helpers take `&` entries; `iommu_rmw32`/`iommu_rmw64` return the new word, and
//!   `iommu_rmw64` shifts its mask in 64 bits (the C shifts it in 32 and would clear the
//!   upper half; nothing calls it).
//! - `IOMMU_DEBUG` is not defined, as in the C: `DPRINTF` is [`dprintf!`] behind the constant
//!   [`IOMMU_DEBUG`] (false), so its arguments stay compiled. `dmar_dumpseg` and `debugme`
//!   return at once as the C's do, behind constants instead of an early `return` before
//!   unreachable code.
//! - The C's `domain_map_page` function pointer is [`domain_map_page`] dispatching on
//!   [`DOMAIN_MAP_PAGE_AMD`], set by `acpidmar_init` (VT-d) or `acpiivrs_init` (AMD-Vi).
//! - The lists (`TAILQ_*`) are `TailqHead`s of leaked `Box`es (the C's `malloc(M_WAITOK)` of
//!   structures it never frees, but `domain_remove_device`, which frees its element, and the
//!   failure paths). A unit whose `iommu_init` fails after it established its fault interrupt
//!   is not freed (the C frees it with the handler still registered); one that failed before
//!   is.
//! - `dmar_bdf` and `dom_bdf` return values that format themselves, where the C returns its
//!   static buffers; `dom_bdf` of a domain with no device yet (the MSI window's failure in
//!   `domain_create`) shows source id 0 where the C dereferences NULL.
//! - The `bus_dma` functions take Rust slices and return `Result` as amd64's `bus_dma.rs`
//!   does; `iommu_flush_tlb_segs` and `ivhd_invalidate_segs` take the segments as an iterator
//!   of `(ds_addr, ds_len)`, so the map's segments (`Cell`s) need no copy. The map cookie
//!   (`struct dmar_map_cookie`, with `dm_er`, one extent descriptor per segment) is
//!   `malloc(9)`ed as in C and hung on `_dm_cookie` (a `Cell` on amd64).
//! - Functions that return a status return `Result`: `iommu_init`, `ivhd_iommu_init`,
//!   `iommu_enable_translation`, `domain_map_page*`, `domain_load_map`; the AMD command
//!   functions return the command's slot or `EBUSY` (the C's `-EBUSY`).
//! - The firmware tables are read from acpi0's copy with bounds-checked unaligned reads; a
//!   remapping structure of length 0 ends the walk (the C would loop for ever).
//! - `bios_memmap` is boot(8)'s; under Limine there is none, so `acpidmar_rmrr` finds no
//!   reserved E820 region and `iommu_showfault` prints no `mem in e820.reserved` line.
//! - `acpidmar_ddb` (`#ifdef DDB`, 0) is an `AtomicI32`; DDB is always configured here.
//! - The `pic_mutex` of `dmarpic` (`MULTIPROCESSOR`) does not exist: amd64's `struct pic`
//!   has none (`include/pic.rs`).
//! - `_iommu_map` and `_iommu_domain` (for vmm(4), not ported) are ported but have no
//!   caller.

use alloc::boxed::Box;
#[cfg(machine_x86)]
use alloc::vec::Vec;
use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(machine_x86)]
use core::{
    cell::Cell,
    ffi::c_void,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32},
};

use super::acpireg::{AcpidmarDevpath, AcpidmarDevscope, DMAR_BRIDGE, DMAR_ENDPOINT};
#[cfg(machine_x86)]
use super::{
    acpireg::{
        AcpiDmar, AcpiIvhd, AcpiIvhdEntryAlias, AcpiIvhdEntryAll, AcpiIvhdEntryEor,
        AcpiIvhdEntryExt, AcpiIvhdEntryResvd, AcpiIvhdEntrySel, AcpiIvhdEntrySor,
        AcpiIvhdEntrySpecial, AcpiIvhdExt, AcpiIvmd, AcpiIvrs, AcpiIvrsEntryHdr, AcpiTableHeader,
        AcpidmarAtsr, AcpidmarDrhd, AcpidmarEntryHdr, AcpidmarRmrr, DMAR_ATSR, DMAR_DRHD,
        DMAR_RMRR, DMAR_SIG, IVHD_ALIAS_SEL, IVHD_ALIAS_SOR, IVHD_ALL, IVHD_COHERENT, IVHD_EOR,
        IVHD_EXT_SEL, IVHD_EXT_SOR, IVHD_HTTUNEN, IVHD_IOTLB, IVHD_ISOC, IVHD_PASSPW, IVHD_PPRSUP,
        IVHD_PREFSUP, IVHD_RESPASSPW, IVHD_RESVD, IVHD_SEL, IVHD_SOR, IVHD_SPECIAL, IVMD_EXCLRANGE,
        IVMD_UNITY, IVRS_IVHD, IVRS_IVHD_EXT, IVRS_IVMD_ALL, IVRS_IVMD_RANGE, IVRS_IVMD_SPECIFIED,
        IVRS_PASIZE_MASK, IVRS_PASIZE_SHIFT, IVRS_SIG, IVRS_VASIZE_MASK, IVRS_VASIZE_SHIFT,
    },
    acpivar::AcpiAttachArgs,
    amd_iommu::*,
};
use crate::sys::queue::{TailqEntry, TailqHead};
#[cfg(machine_x86)]
use crate::{
    dev::pci::{
        pcireg::{
            PCI_BHLC_REG, PCI_CLASS_BRIDGE, PCI_CLASS_DISPLAY, PCI_CLASS_REG,
            PCI_SUBCLASS_BRIDGE_ISA, PCI_SUBCLASS_DISPLAY_VGA, pci_class, pci_hdrtype_type,
            pci_subclass,
        },
        pcivar::PciAttachArgs,
        ppbreg::{
            PPB_REG_BUSINFO, PPB_REG_MEM, PPB_REG_PREFBASE_HI32, PPB_REG_PREFLIM_HI32,
            PPB_REG_PREFMEM, ppb_businfo_secondary, ppb_businfo_subordinate,
        },
    },
    kassert,
    kern::{
        kern_lock::{mtx_enter, mtx_init, mtx_leave},
        kern_malloc::{free, malloc},
        subr_extent::{
            extent_alloc_region, extent_alloc_subregion_with_descr, extent_create, extent_destroy,
            extent_free,
        },
        subr_prf::snprintf,
    },
    kprintf,
    machine::{
        bus::{
            BUS_DMA_64BIT, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO, BusAddr,
            BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_4,
            bus_space_write_4,
        },
        cpu::{curproc, delay},
        db_machdep::db_enter,
        intr::{IPL_HIGH, IPL_MPSAFE, IPL_NET, IPL_NONE},
        pci_machdep::{
            PciChipsetTag, Pcitag, pci_conf_read, pci_decompose_tag, pci_intr_establish,
            pci_make_tag,
        },
        pmap::{pmap_extract, pmap_kernel},
        x86,
    },
    sys::{
        device::{
            CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, DVACT_SUSPEND, Device, Softc,
        },
        errno::Errno,
        extent::{EX_CONFLICTOK, EX_NOCOALESCE, EX_NOWAIT, EX_WAITOK, Extent, ExtentRegion},
        malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK},
        mbuf::Mbuf,
        mutex::Mutex,
        param::{PAGE_SIZE, roundup},
        proc::Proc,
        types::{Off, Paddr, Vaddr, Vsize},
        uio::Uio,
    },
    uvm::{
        uvm_km::{KD_TRYLOCK, KD_WAITOK, KP_ZERO, KV_PAGE, km_alloc, km_free},
        uvm_param::{round_page, trunc_page},
    },
};

/// `IOMMU_DEBUG`: not defined (`DPRINTF` prints nothing).
pub const IOMMU_DEBUG: bool = false;

/// `VTD_STRIDE_MASK`.
pub const VTD_STRIDE_MASK: u64 = 0x1FF;
/// `VTD_STRIDE_SIZE`.
pub const VTD_STRIDE_SIZE: i32 = 9;
/// `VTD_PAGE_SIZE`.
pub const VTD_PAGE_SIZE: usize = 4096;
/// `VTD_PAGE_MASK`.
pub const VTD_PAGE_MASK: usize = 0xFFF;
/// `VTD_PTE_MASK`.
pub const VTD_PTE_MASK: u64 = 0x0000_FFFF_FFFF_F000;

/// `VTD_LEVEL0`.
pub const VTD_LEVEL0: i32 = 12;
/// `VTD_LEVEL1`.
pub const VTD_LEVEL1: i32 = 21;
/// `VTD_LEVEL2`: Minimum level supported.
pub const VTD_LEVEL2: i32 = 30;
/// `VTD_LEVEL3`: Also supported.
pub const VTD_LEVEL3: i32 = 39;
/// `VTD_LEVEL4`.
pub const VTD_LEVEL4: i32 = 48;
/// `VTD_LEVEL5`.
pub const VTD_LEVEL5: i32 = 57;

/// `DMAR_VER_REG`: 32:Arch version supported by this IOMMU.
pub const DMAR_VER_REG: usize = 0x00;
/// `DMAR_RTADDR_REG`: 64:Root entry table.
pub const DMAR_RTADDR_REG: usize = 0x20;
/// `DMAR_FEDATA_REG`: 32:Fault event interrupt data register.
pub const DMAR_FEDATA_REG: usize = 0x3c;
/// `DMAR_FEADDR_REG`: 32:Fault event interrupt addr register.
pub const DMAR_FEADDR_REG: usize = 0x40;
/// `DMAR_FEUADDR_REG`: 32:Upper address register.
pub const DMAR_FEUADDR_REG: usize = 0x44;
/// `DMAR_AFLOG_REG`: 64:Advanced Fault control.
pub const DMAR_AFLOG_REG: usize = 0x58;
/// `DMAR_PMEN_REG`: 32:Enable Protected Memory Region.
pub const DMAR_PMEN_REG: usize = 0x64;
/// `DMAR_PLMBASE_REG`: 32:PMRR Low addr.
pub const DMAR_PLMBASE_REG: usize = 0x68;
/// `DMAR_PLMLIMIT_REG`: 32:PMRR low limit.
pub const DMAR_PLMLIMIT_REG: usize = 0x6c;
/// `DMAR_PHMBASE_REG`: 64:pmrr high base addr.
pub const DMAR_PHMBASE_REG: usize = 0x70;
/// `DMAR_PHMLIMIT_REG`: 64:pmrr high limit.
pub const DMAR_PHMLIMIT_REG: usize = 0x78;
/// `DMAR_ICS_REG`: 32:Invalidation complete status register.
pub const DMAR_ICS_REG: usize = 0x9C;
/// `DMAR_IECTL_REG`: 32:Invalidation event control register.
pub const DMAR_IECTL_REG: usize = 0xa0;
/// `DMAR_IEDATA_REG`: 32:Invalidation event data register.
pub const DMAR_IEDATA_REG: usize = 0xa4;
/// `DMAR_IEADDR_REG`: 32:Invalidation event address register.
pub const DMAR_IEADDR_REG: usize = 0xa8;
/// `DMAR_IEUADDR_REG`: 32:Invalidation event upper address register.
pub const DMAR_IEUADDR_REG: usize = 0xac;
/// `DMAR_IRTA_REG`: 64:Interrupt remapping table addr register.
pub const DMAR_IRTA_REG: usize = 0xb8;
/// `DMAR_CAP_REG`: 64:Hardware supported capabilities.
pub const DMAR_CAP_REG: usize = 0x08;
/// `CAP_PI`.
pub const CAP_PI: u64 = 1 << 59;
/// `CAP_FL1GP`.
pub const CAP_FL1GP: u64 = 1 << 56;
/// `CAP_DRD`.
pub const CAP_DRD: u64 = 1 << 55;
/// `CAP_DWD`.
pub const CAP_DWD: u64 = 1 << 54;
/// `CAP_MAMV_MASK`.
pub const CAP_MAMV_MASK: u64 = 0x3F;
/// `CAP_MAMV_SHIFT`.
pub const CAP_MAMV_SHIFT: u64 = 48;
/// `CAP_NFR_MASK`.
pub const CAP_NFR_MASK: u64 = 0xFF;
/// `CAP_NFR_SHIFT`.
pub const CAP_NFR_SHIFT: u64 = 40;
/// `CAP_PSI`.
pub const CAP_PSI: u64 = 1 << 39;
/// `CAP_SLLPS_MASK`.
pub const CAP_SLLPS_MASK: u64 = 0xF;
/// `CAP_SLLPS_SHIFT`.
pub const CAP_SLLPS_SHIFT: u64 = 34;
/// `CAP_FRO_MASK`.
pub const CAP_FRO_MASK: u64 = 0x3FF;
/// `CAP_FRO_SHIFT`.
pub const CAP_FRO_SHIFT: u64 = 24;
/// `CAP_ZLR`.
pub const CAP_ZLR: u64 = 1 << 22;
/// `CAP_MGAW_MASK`.
pub const CAP_MGAW_MASK: u64 = 0x3F;
/// `CAP_MGAW_SHIFT`.
pub const CAP_MGAW_SHIFT: u64 = 16;
/// `CAP_SAGAW_MASK`.
pub const CAP_SAGAW_MASK: u64 = 0x1F;
/// `CAP_SAGAW_SHIFT`.
pub const CAP_SAGAW_SHIFT: u64 = 8;
/// `CAP_CM`.
pub const CAP_CM: u64 = 1 << 7;
/// `CAP_PHMR`.
pub const CAP_PHMR: u64 = 1 << 6;
/// `CAP_PLMR`.
pub const CAP_PLMR: u64 = 1 << 5;
/// `CAP_RWBF`.
pub const CAP_RWBF: u64 = 1 << 4;
/// `CAP_AFL`.
pub const CAP_AFL: u64 = 1 << 3;
/// `CAP_ND_MASK`.
pub const CAP_ND_MASK: u64 = 0x7;
/// `CAP_ND_SHIFT`.
pub const CAP_ND_SHIFT: u64 = 0x00;

/// `DMAR_ECAP_REG`: 64:Extended capabilities supported.
pub const DMAR_ECAP_REG: usize = 0x10;
/// `ECAP_PSS_MASK`.
pub const ECAP_PSS_MASK: u64 = 0x1F;
/// `ECAP_PSS_SHIFT`.
pub const ECAP_PSS_SHIFT: u64 = 35;
/// `ECAP_EAFS`.
pub const ECAP_EAFS: u64 = 1 << 34;
/// `ECAP_NWFS`.
pub const ECAP_NWFS: u64 = 1 << 33;
/// `ECAP_SRS`.
pub const ECAP_SRS: u64 = 1 << 31;
/// `ECAP_ERS`.
pub const ECAP_ERS: u64 = 1 << 30;
/// `ECAP_PRS`.
pub const ECAP_PRS: u64 = 1 << 29;
/// `ECAP_PASID`.
pub const ECAP_PASID: u64 = 1 << 28;
/// `ECAP_DIS`.
pub const ECAP_DIS: u64 = 1 << 27;
/// `ECAP_NEST`.
pub const ECAP_NEST: u64 = 1 << 26;
/// `ECAP_MTS`.
pub const ECAP_MTS: u64 = 1 << 25;
/// `ECAP_ECS`.
pub const ECAP_ECS: u64 = 1 << 24;
/// `ECAP_MHMV_MASK`.
pub const ECAP_MHMV_MASK: u64 = 0xF;
/// `ECAP_MHMV_SHIFT`.
pub const ECAP_MHMV_SHIFT: u64 = 0x20;
/// `ECAP_IRO_MASK`: IOTLB Register.
pub const ECAP_IRO_MASK: u64 = 0x3FF;
/// `ECAP_IRO_SHIFT`.
pub const ECAP_IRO_SHIFT: u64 = 0x8;
/// `ECAP_SC`: Snoop Control.
pub const ECAP_SC: u64 = 1 << 7;
/// `ECAP_PT`: HW Passthru.
pub const ECAP_PT: u64 = 1 << 6;
/// `ECAP_EIM`.
pub const ECAP_EIM: u64 = 1 << 4;
/// `ECAP_IR`: Interrupt remap.
pub const ECAP_IR: u64 = 1 << 3;
/// `ECAP_DT`: Device IOTLB.
pub const ECAP_DT: u64 = 1 << 2;
/// `ECAP_QI`: Queued Invalidation.
pub const ECAP_QI: u64 = 1 << 1;
/// `ECAP_C`: Coherent cache.
pub const ECAP_C: u64 = 1 << 0;

/// `DMAR_GCMD_REG`: 32:Global command register.
pub const DMAR_GCMD_REG: usize = 0x18;
/// `GCMD_TE`.
pub const GCMD_TE: u32 = 1 << 31;
/// `GCMD_SRTP`.
pub const GCMD_SRTP: u32 = 1 << 30;
/// `GCMD_SFL`.
pub const GCMD_SFL: u32 = 1 << 29;
/// `GCMD_EAFL`.
pub const GCMD_EAFL: u32 = 1 << 28;
/// `GCMD_WBF`.
pub const GCMD_WBF: u32 = 1 << 27;
/// `GCMD_QIE`.
pub const GCMD_QIE: u32 = 1 << 26;
/// `GCMD_IRE`.
pub const GCMD_IRE: u32 = 1 << 25;
/// `GCMD_SIRTP`.
pub const GCMD_SIRTP: u32 = 1 << 24;
/// `GCMD_CFI`.
pub const GCMD_CFI: u32 = 1 << 23;

/// `DMAR_GSTS_REG`: 32:Global status register.
pub const DMAR_GSTS_REG: usize = 0x1c;
/// `GSTS_TES`.
pub const GSTS_TES: u32 = 1 << 31;
/// `GSTS_RTPS`.
pub const GSTS_RTPS: u32 = 1 << 30;
/// `GSTS_FLS`.
pub const GSTS_FLS: u32 = 1 << 29;
/// `GSTS_AFLS`.
pub const GSTS_AFLS: u32 = 1 << 28;
/// `GSTS_WBFS`.
pub const GSTS_WBFS: u32 = 1 << 27;
/// `GSTS_QIES`.
pub const GSTS_QIES: u32 = 1 << 26;
/// `GSTS_IRES`.
pub const GSTS_IRES: u32 = 1 << 25;
/// `GSTS_IRTPS`.
pub const GSTS_IRTPS: u32 = 1 << 24;
/// `GSTS_CFIS`.
pub const GSTS_CFIS: u32 = 1 << 23;

/// `DMAR_CCMD_REG`: 64:Context command reg.
pub const DMAR_CCMD_REG: usize = 0x28;
/// `CCMD_ICC`.
pub const CCMD_ICC: u64 = 1 << 63;
/// `CCMD_CIRG_MASK`.
pub const CCMD_CIRG_MASK: u64 = 0x3;
/// `CCMD_CIRG_SHIFT`.
pub const CCMD_CIRG_SHIFT: u64 = 61;
/// `CCMD_CAIG_MASK`.
pub const CCMD_CAIG_MASK: u64 = 0x3;
/// `CCMD_CAIG_SHIFT`.
pub const CCMD_CAIG_SHIFT: u64 = 59;
/// `CCMD_FM_MASK`.
pub const CCMD_FM_MASK: u64 = 0x3;
/// `CCMD_FM_SHIFT`.
pub const CCMD_FM_SHIFT: u64 = 32;
/// `CCMD_SID_MASK`.
pub const CCMD_SID_MASK: u64 = 0xFFFF;
/// `CCMD_SID_SHIFT`.
pub const CCMD_SID_SHIFT: u64 = 8;
/// `CCMD_DID_MASK`.
pub const CCMD_DID_MASK: u64 = 0xFFFF;
/// `CCMD_DID_SHIFT`.
pub const CCMD_DID_SHIFT: u64 = 0;

/// `CTX_GLOBAL`: an invalidation granularity.
pub const CTX_GLOBAL: i32 = 1;
/// `CTX_DOMAIN`.
pub const CTX_DOMAIN: i32 = 2;
/// `CTX_DEVICE`.
pub const CTX_DEVICE: i32 = 3;
/// `IOTLB_GLOBAL`.
pub const IOTLB_GLOBAL: i32 = 1;
/// `IOTLB_DOMAIN`.
pub const IOTLB_DOMAIN: i32 = 2;
/// `IOTLB_PAGE`.
pub const IOTLB_PAGE: i32 = 3;

/// `CIG_GLOBAL`.
pub const CIG_GLOBAL: u64 = ccmd_cirg(CTX_GLOBAL as u64);
/// `CIG_DOMAIN`.
pub const CIG_DOMAIN: u64 = ccmd_cirg(CTX_DOMAIN as u64);
/// `CIG_DEVICE`.
pub const CIG_DEVICE: u64 = ccmd_cirg(CTX_DEVICE as u64);

/// `DMAR_FSTS_REG`: 32:Fault Status register.
pub const DMAR_FSTS_REG: usize = 0x34;
/// `FSTS_FRI_MASK`.
pub const FSTS_FRI_MASK: u32 = 0xFF;
/// `FSTS_FRI_SHIFT`.
pub const FSTS_FRI_SHIFT: u32 = 8;
/// `FSTS_PRO`.
pub const FSTS_PRO: u32 = 1 << 7;
/// `FSTS_ITE`.
pub const FSTS_ITE: u32 = 1 << 6;
/// `FSTS_ICE`.
pub const FSTS_ICE: u32 = 1 << 5;
/// `FSTS_IQE`.
pub const FSTS_IQE: u32 = 1 << 4;
/// `FSTS_APF`.
pub const FSTS_APF: u32 = 1 << 3;
/// `FSTS_APO`.
pub const FSTS_APO: u32 = 1 << 2;
/// `FSTS_PPF`.
pub const FSTS_PPF: u32 = 1 << 1;
/// `FSTS_PFO`.
pub const FSTS_PFO: u32 = 1 << 0;

/// `DMAR_FECTL_REG`: 32:Fault control register.
pub const DMAR_FECTL_REG: usize = 0x38;
/// `FECTL_IM`.
pub const FECTL_IM: u32 = 1 << 31;
/// `FECTL_IP`.
pub const FECTL_IP: u32 = 1 << 30;

/// `FRCD_HI_F`.
pub const FRCD_HI_F: u64 = 1 << (127 - 64);
/// `FRCD_HI_T`.
pub const FRCD_HI_T: u64 = 1 << (126 - 64);
/// `FRCD_HI_AT_MASK`.
pub const FRCD_HI_AT_MASK: u64 = 0x3;
/// `FRCD_HI_AT_SHIFT`.
pub const FRCD_HI_AT_SHIFT: u64 = 124 - 64;
/// `FRCD_HI_PV_MASK`.
pub const FRCD_HI_PV_MASK: u64 = 0xFFFFF;
/// `FRCD_HI_PV_SHIFT`.
pub const FRCD_HI_PV_SHIFT: u64 = 104 - 64;
/// `FRCD_HI_FR_MASK`.
pub const FRCD_HI_FR_MASK: u64 = 0xFF;
/// `FRCD_HI_FR_SHIFT`.
pub const FRCD_HI_FR_SHIFT: u64 = 96 - 64;
/// `FRCD_HI_PP`.
pub const FRCD_HI_PP: u64 = 1 << (95 - 64);

/// `FRCD_HI_SID_MASK`.
pub const FRCD_HI_SID_MASK: u64 = 0xFF;
/// `FRCD_HI_SID_SHIFT`.
pub const FRCD_HI_SID_SHIFT: u64 = 0;
/// `FRCD_HI_BUS_SHIFT`.
pub const FRCD_HI_BUS_SHIFT: u64 = 8;
/// `FRCD_HI_BUS_MASK`.
pub const FRCD_HI_BUS_MASK: u64 = 0xFF;
/// `FRCD_HI_DEV_SHIFT`.
pub const FRCD_HI_DEV_SHIFT: u64 = 3;
/// `FRCD_HI_DEV_MASK`.
pub const FRCD_HI_DEV_MASK: u64 = 0x1F;
/// `FRCD_HI_FUN_SHIFT`.
pub const FRCD_HI_FUN_SHIFT: u64 = 0;
/// `FRCD_HI_FUN_MASK`.
pub const FRCD_HI_FUN_MASK: u64 = 0x7;

/// `IOTLB_IVT`.
pub const IOTLB_IVT: u64 = 1 << 63;
/// `IOTLB_IIRG_MASK`.
pub const IOTLB_IIRG_MASK: u64 = 0x3;
/// `IOTLB_IIRG_SHIFT`.
pub const IOTLB_IIRG_SHIFT: u64 = 60;
/// `IOTLB_IAIG_MASK`.
pub const IOTLB_IAIG_MASK: u64 = 0x3;
/// `IOTLB_IAIG_SHIFT`.
pub const IOTLB_IAIG_SHIFT: u64 = 57;
/// `IOTLB_DR`.
pub const IOTLB_DR: u64 = 1 << 49;
/// `IOTLB_DW`.
pub const IOTLB_DW: u64 = 1 << 48;
/// `IOTLB_DID_MASK`.
pub const IOTLB_DID_MASK: u64 = 0xFFFF;
/// `IOTLB_DID_SHIFT`.
pub const IOTLB_DID_SHIFT: u64 = 32;

/// `IIG_GLOBAL`.
pub const IIG_GLOBAL: u64 = iotlb_iirg(IOTLB_GLOBAL as u64);
/// `IIG_DOMAIN`.
pub const IIG_DOMAIN: u64 = iotlb_iirg(IOTLB_DOMAIN as u64);
/// `IIG_PAGE`.
pub const IIG_PAGE: u64 = iotlb_iirg(IOTLB_PAGE as u64);

/// `DMAR_IQH_REG`: 64:Invalidation queue head register.
pub const DMAR_IQH_REG: usize = 0x80;
/// `DMAR_IQT_REG`: 64:Invalidation queue tail register.
pub const DMAR_IQT_REG: usize = 0x88;
/// `DMAR_IQA_REG`: 64:Invalidation queue addr register.
pub const DMAR_IQA_REG: usize = 0x90;
/// `IQA_QS_256`: 256 entries.
pub const IQA_QS_256: u64 = 0;
/// `IQA_QS_512`.
pub const IQA_QS_512: u64 = 1;
/// `IQA_QS_1K`.
pub const IQA_QS_1K: u64 = 2;
/// `IQA_QS_2K`.
pub const IQA_QS_2K: u64 = 3;
/// `IQA_QS_4K`.
pub const IQA_QS_4K: u64 = 4;
/// `IQA_QS_8K`.
pub const IQA_QS_8K: u64 = 5;
/// `IQA_QS_16K`.
pub const IQA_QS_16K: u64 = 6;
/// `IQA_QS_32K`.
pub const IQA_QS_32K: u64 = 7;

/// `ROOT_P`.
pub const ROOT_P: u64 = 1 << 0;

/// `CTX_P`.
pub const CTX_P: u64 = 1 << 0;
/// `CTX_FPD`.
pub const CTX_FPD: u64 = 1 << 1;
/// `CTX_T_MASK`.
pub const CTX_T_MASK: u64 = 0x3;
/// `CTX_T_SHIFT`.
pub const CTX_T_SHIFT: u64 = 2;
/// `CTX_T_MULTI`: a translation type.
pub const CTX_T_MULTI: i32 = 0;
/// `CTX_T_IOTLB`.
pub const CTX_T_IOTLB: i32 = 1;
/// `CTX_T_PASSTHRU`.
pub const CTX_T_PASSTHRU: i32 = 2;

/// `CTX_H_AW_MASK`.
pub const CTX_H_AW_MASK: u64 = 0x7;
/// `CTX_H_AW_SHIFT`.
pub const CTX_H_AW_SHIFT: u64 = 0;
/// `CTX_H_USER_MASK`.
pub const CTX_H_USER_MASK: u64 = 0xF;
/// `CTX_H_USER_SHIFT`.
pub const CTX_H_USER_SHIFT: u64 = 3;
/// `CTX_H_DID_MASK`.
pub const CTX_H_DID_MASK: u64 = 0xFFFF;
/// `CTX_H_DID_SHIFT`.
pub const CTX_H_DID_SHIFT: u64 = 8;

/// `PTE_P`.
pub const PTE_P: u64 = 1 << 0;
/// `PTE_R`.
pub const PTE_R: u64 = 0x00;
/// `PTE_W`.
pub const PTE_W: u64 = 1 << 1;
/// `PTE_US`.
pub const PTE_US: u64 = 1 << 2;
/// `PTE_PWT`.
pub const PTE_PWT: u64 = 1 << 3;
/// `PTE_PCD`.
pub const PTE_PCD: u64 = 1 << 4;
/// `PTE_A`.
pub const PTE_A: u64 = 1 << 5;
/// `PTE_D`.
pub const PTE_D: u64 = 1 << 6;
/// `PTE_PAT`.
pub const PTE_PAT: u64 = 1 << 7;
/// `PTE_G`.
pub const PTE_G: u64 = 1 << 8;
/// `PTE_EA`.
pub const PTE_EA: u64 = 1 << 10;
/// `PTE_XD`.
pub const PTE_XD: u64 = 1 << 63;

/// `PTE_PS`: PDE Level entry.
pub const PTE_PS: u64 = 1 << 7;

/// `QI_CTX_DID_MASK`: Invalidate Context Entry.
pub const QI_CTX_DID_MASK: u64 = 0xFFFF;
/// `QI_CTX_DID_SHIFT`.
pub const QI_CTX_DID_SHIFT: u64 = 16;
/// `QI_CTX_SID_MASK`.
pub const QI_CTX_SID_MASK: u64 = 0xFFFF;
/// `QI_CTX_SID_SHIFT`.
pub const QI_CTX_SID_SHIFT: u64 = 32;
/// `QI_CTX_FM_MASK`.
pub const QI_CTX_FM_MASK: u64 = 0x3;
/// `QI_CTX_FM_SHIFT`.
pub const QI_CTX_FM_SHIFT: u64 = 48;
/// `QI_CTX_IG_MASK`.
pub const QI_CTX_IG_MASK: u64 = 0x3;
/// `QI_CTX_IG_SHIFT`.
pub const QI_CTX_IG_SHIFT: u64 = 4;

/// `QI_CTX_IG_GLOBAL`.
pub const QI_CTX_IG_GLOBAL: u64 = (CTX_GLOBAL as u64) << QI_CTX_IG_SHIFT;
/// `QI_CTX_IG_DOMAIN`.
pub const QI_CTX_IG_DOMAIN: u64 = (CTX_DOMAIN as u64) << QI_CTX_IG_SHIFT;
/// `QI_CTX_IG_DEVICE`.
pub const QI_CTX_IG_DEVICE: u64 = (CTX_DEVICE as u64) << QI_CTX_IG_SHIFT;

/// `QI_IOTLB_DID_MASK`: Invalidate IOTLB Entry.
pub const QI_IOTLB_DID_MASK: u64 = 0xFFFF;
/// `QI_IOTLB_DID_SHIFT`.
pub const QI_IOTLB_DID_SHIFT: u64 = 16;
/// `QI_IOTLB_IG_MASK`.
pub const QI_IOTLB_IG_MASK: u64 = 0x3;
/// `QI_IOTLB_IG_SHIFT`.
pub const QI_IOTLB_IG_SHIFT: u64 = 4;
/// `QI_IOTLB_DR`.
pub const QI_IOTLB_DR: u64 = 1 << 6;
/// `QI_IOTLB_DW`.
pub const QI_IOTLB_DW: u64 = 1 << 5;

/// `QI_IOTLB_IG_GLOBAL`.
pub const QI_IOTLB_IG_GLOBAL: u64 = 1 << QI_IOTLB_IG_SHIFT;
/// `QI_IOTLB_IG_DOMAIN`.
pub const QI_IOTLB_IG_DOMAIN: u64 = 2 << QI_IOTLB_IG_SHIFT;
/// `QI_IOTLB_IG_PAGE`.
pub const QI_IOTLB_IG_PAGE: u64 = 3 << QI_IOTLB_IG_SHIFT;

/// `QI_CTX`: a queued invalidation command.
pub const QI_CTX: u64 = 0x1;
/// `QI_IOTLB`.
pub const QI_IOTLB: u64 = 0x2;
/// `QI_DEVTLB`.
pub const QI_DEVTLB: u64 = 0x3;
/// `QI_INTR`.
pub const QI_INTR: u64 = 0x4;
/// `QI_WAIT`.
pub const QI_WAIT: u64 = 0x5;
/// `QI_EXTTLB`.
pub const QI_EXTTLB: u64 = 0x6;
/// `QI_PAS`.
pub const QI_PAS: u64 = 0x7;
/// `QI_EXTDEV`.
pub const QI_EXTDEV: u64 = 0x8;

/// `VTD_FAULT_ROOT_P`: P field in root entry is 0.
pub const VTD_FAULT_ROOT_P: i32 = 0x1;
/// `VTD_FAULT_CTX_P`: P field in context entry is 0.
pub const VTD_FAULT_CTX_P: i32 = 0x2;
/// `VTD_FAULT_CTX_INVAL`: context AW/TT/SLPPTR invalid.
pub const VTD_FAULT_CTX_INVAL: i32 = 0x3;
/// `VTD_FAULT_LIMIT`: Address is outside of MGAW.
pub const VTD_FAULT_LIMIT: i32 = 0x4;
/// `VTD_FAULT_WRITE`: Address-translation fault, non-writable.
pub const VTD_FAULT_WRITE: i32 = 0x5;
/// `VTD_FAULT_READ`: Address-translation fault, non-readable.
pub const VTD_FAULT_READ: i32 = 0x6;
/// `VTD_FAULT_PTE_INVAL`: page table hw access error.
pub const VTD_FAULT_PTE_INVAL: i32 = 0x7;
/// `VTD_FAULT_ROOT_INVAL`: root table hw access error.
pub const VTD_FAULT_ROOT_INVAL: i32 = 0x8;
/// `VTD_FAULT_CTX_TBL_INVAL`: context entry hw access error.
pub const VTD_FAULT_CTX_TBL_INVAL: i32 = 0x9;
/// `VTD_FAULT_ROOT_RESERVED`: non-zero reserved field in root entry.
pub const VTD_FAULT_ROOT_RESERVED: i32 = 0xa;
/// `VTD_FAULT_CTX_RESERVED`: non-zero reserved field in context entry.
pub const VTD_FAULT_CTX_RESERVED: i32 = 0xb;
/// `VTD_FAULT_PTE_RESERVED`: non-zero reserved field in paging entry.
pub const VTD_FAULT_PTE_RESERVED: i32 = 0xc;
/// `VTD_FAULT_CTX_TT`: invalid translation type.
pub const VTD_FAULT_CTX_TT: i32 = 0xd;

/// `MSI_BASE_ADDRESS`: we don't want IOMMU to remap MSI.
pub const MSI_BASE_ADDRESS: u64 = 0xFEE0_0000;
/// `MSI_BASE_SIZE`.
pub const MSI_BASE_SIZE: u64 = 0x0010_0000;
/// `RESERVED_ADDRESS`.
pub const RESERVED_ADDRESS: u64 = 0x0000_0000;
/// `RESERVED_SIZE`.
pub const RESERVED_SIZE: u64 = 0x0010_0000;
/// `MAX_DEVFN`.
pub const MAX_DEVFN: usize = 65536;

/// `IOMMU_QI_ENTRIES`: Intel Queued Invalidation queue.
pub const IOMMU_QI_ENTRIES: i32 = 256;
/// `IOMMU_QI_SIZE`.
pub const IOMMU_QI_SIZE: u64 = IOMMU_QI_ENTRIES as u64 * size_of::<QiEntry>() as u64;
/// `IOMMU_QI_MASK`.
pub const IOMMU_QI_MASK: u64 = IOMMU_QI_SIZE - 1;

/// `IOMMU_TLB_RANGE_MAX_DESCS_INTEL`: range limit before per-domain invalidation.
pub const IOMMU_TLB_RANGE_MAX_DESCS_INTEL: u64 = 128;
/// `IOMMU_TLB_RANGE_MAX_PAGES_AMD`.
pub const IOMMU_TLB_RANGE_MAX_PAGES_AMD: u64 = 64;

/// `SID_INVALID`: Alias mapping.
pub const SID_INVALID: u32 = 0x8000_0000;

/// `DOM_DEBUG`.
pub const DOM_DEBUG: i32 = 0x1;
/// `DOM_NOMAP`.
pub const DOM_NOMAP: i32 = 0x2;

/// `IOMMU_FLAGS_CATCHALL`.
pub const IOMMU_FLAGS_CATCHALL: i32 = 0x1;
/// `IOMMU_FLAGS_BAD`.
pub const IOMMU_FLAGS_BAD: i32 = 0x2;
/// `IOMMU_FLAGS_SUSPEND`.
pub const IOMMU_FLAGS_SUSPEND: i32 = 0x4;
/// `IOMMU_FLAGS_COHERENT`.
pub const IOMMU_FLAGS_COHERENT: i32 = 0x8;

/// `DID_UNITY`.
pub const DID_UNITY: i32 = 0x1;

/// `IVHD_MAXDELAY`.
pub const IVHD_MAXDELAY: u32 = 8;

/// `_xbit(x, y)`.
pub const fn xbit(x: u64, y: u64) -> u64 {
    (x >> y) & 1
}

/// `_xfld(x, y)`: the field `y` (`y##_SHIFT`, `y##_MASK`) of `x`.
pub const fn xfld(x: u64, shift: u64, mask: u64) -> u32 {
    ((x >> shift) & mask) as u32
}

/// `VTD_AWTOLEVEL(x)`.
pub const fn vtd_awtolevel(x: i32) -> i32 {
    (x - 30) / VTD_STRIDE_SIZE
}

/// `VTD_LEVELTOAW(x)`.
pub const fn vtd_leveltoaw(x: i32) -> i32 {
    x * VTD_STRIDE_SIZE + 30
}

/// `cap_mamv(x)`.
pub const fn cap_mamv(x: u64) -> u32 {
    xfld(x, CAP_MAMV_SHIFT, CAP_MAMV_MASK)
}

/// `cap_nfr(x)`.
pub const fn cap_nfr(x: u64) -> u32 {
    xfld(x, CAP_NFR_SHIFT, CAP_NFR_MASK) + 1
}

/// `cap_sllps(x)`.
pub const fn cap_sllps(x: u64) -> u32 {
    xfld(x, CAP_SLLPS_SHIFT, CAP_SLLPS_MASK)
}

/// `cap_fro(x)`.
pub const fn cap_fro(x: u64) -> u32 {
    xfld(x, CAP_FRO_SHIFT, CAP_FRO_MASK) * 16
}

/// `cap_mgaw(x)`.
pub const fn cap_mgaw(x: u64) -> u32 {
    xfld(x, CAP_MGAW_SHIFT, CAP_MGAW_MASK) + 1
}

/// `cap_sagaw(x)`.
pub const fn cap_sagaw(x: u64) -> u32 {
    xfld(x, CAP_SAGAW_SHIFT, CAP_SAGAW_MASK)
}

/// `cap_nd(x)`: the number of domain ids.
pub const fn cap_nd(x: u64) -> i32 {
    16 << ((x & CAP_ND_MASK) << 1)
}

/// `ecap_mhmv(x)`.
pub const fn ecap_mhmv(x: u64) -> u32 {
    xfld(x, ECAP_MHMV_SHIFT, ECAP_MHMV_MASK)
}

/// `ecap_iro(x)`: the IOTLB registers' offset.
pub const fn ecap_iro(x: u64) -> u32 {
    xfld(x, ECAP_IRO_SHIFT, ECAP_IRO_MASK) * 16
}

/// `CCMD_CIRG(x)`.
pub const fn ccmd_cirg(x: u64) -> u64 {
    x << CCMD_CIRG_SHIFT
}

/// `CCMD_FM(x)`.
pub const fn ccmd_fm(x: u64) -> u64 {
    x << CCMD_FM_SHIFT
}

/// `CCMD_SID(x)`.
pub const fn ccmd_sid(x: u64) -> u64 {
    x << CCMD_SID_SHIFT
}

/// `CCMD_DID(x)`.
pub const fn ccmd_did(x: u64) -> u64 {
    x << CCMD_DID_SHIFT
}

/// `DMAR_IOTLB_REG(x)`, of a unit whose extended capabilities are `ecap`.
pub const fn dmar_iotlb_reg(ecap: u64) -> usize {
    ecap_iro(ecap) as usize + 8
}

/// `DMAR_IVA_REG(x)`.
pub const fn dmar_iva_reg(ecap: u64) -> usize {
    ecap_iro(ecap) as usize
}

/// `DMAR_FRIH_REG(x, i)`, of a unit whose capabilities are `cap`.
pub const fn dmar_frih_reg(cap: u64, i: u32) -> usize {
    (cap_fro(cap) + 16 * i + 8) as usize
}

/// `DMAR_FRIL_REG(x, i)`.
pub const fn dmar_fril_reg(cap: u64, i: u32) -> usize {
    (cap_fro(cap) + 16 * i) as usize
}

/// `IOTLB_IIRG(x)`.
pub const fn iotlb_iirg(x: u64) -> u64 {
    x << IOTLB_IIRG_SHIFT
}

/// `IOTLB_DID(x)`.
pub const fn iotlb_did(x: u64) -> u64 {
    x << IOTLB_DID_SHIFT
}

/// `QI_CTX_DID(x)`.
pub const fn qi_ctx_did(x: u64) -> u64 {
    x << QI_CTX_DID_SHIFT
}

/// `QI_CTX_SID(x)`.
pub const fn qi_ctx_sid(x: u64) -> u64 {
    x << QI_CTX_SID_SHIFT
}

/// `QI_CTX_FM(x)`.
pub const fn qi_ctx_fm(x: u64) -> u64 {
    x << QI_CTX_FM_SHIFT
}

/// `QI_IOTLB_DID(x)`.
pub const fn qi_iotlb_did(x: u64) -> u64 {
    x << QI_IOTLB_DID_SHIFT
}

/// `__EXTRACT(v, m)`: the field `m` (`m##_SHIFT`, `m##_MASK`) of `v`.
pub const fn extract(v: u32, shift: u32, mask: u32) -> u32 {
    (v >> shift) & mask
}

/// `iommu_rmw32(ov, mask, shift, nv)`: `ov` with the field at `shift` replaced by `nv`.
pub const fn iommu_rmw32(ov: u32, mask: u32, shift: u32, nv: u32) -> u32 {
    (ov & !(mask << shift)) | ((nv & mask) << shift)
}

/// `iommu_rmw64(ov, mask, shift, nv)`: the 64-bit form (see the module's deviations).
pub const fn iommu_rmw64(ov: u64, mask: u32, shift: u32, nv: u64) -> u64 {
    let m = mask as u64;
    (ov & !(m << shift)) | ((nv & m) << shift)
}

/// `mksid(b, d, f)`: the source id of bus `b`, device `d`, function `f`.
pub const fn mksid(b: i32, d: i32, f: i32) -> i32 {
    (b << 8) + (d << 3) + f
}

/// `sid_devfn(sid)`.
pub const fn sid_devfn(sid: i32) -> i32 {
    sid & 0xff
}

/// `sid_bus(sid)`.
pub const fn sid_bus(sid: i32) -> i32 {
    (sid >> 8) & 0xff
}

/// `sid_dev(sid)`.
pub const fn sid_dev(sid: i32) -> i32 {
    (sid >> 3) & 0x1f
}

/// `sid_fun(sid)`.
pub const fn sid_fun(sid: i32) -> i32 {
    sid & 0x7
}

/// Generates the `DPRINTF(lvl, ...)` of the C: printed only with [`IOMMU_DEBUG`] and
/// `acpidmar_dbg_lvl >= lvl`.
macro_rules! dprintf {
    ($lvl:expr, $($arg:tt)*) => {
        if IOMMU_DEBUG && ACPIDMAR_DBG_LVL.load(core::sync::atomic::Ordering::Relaxed) >= $lvl {
            $crate::kprintf!($($arg)*);
        }
    };
}

/// `struct root_entry`: one per bus (256 x 128 bit = 4k).
///
/// ```text
///   0        = Present
///   1:11     = Reserved
///   12:HAW-1 = Context Table Pointer
///   HAW:63   = Reserved
///   64:127   = Reserved
/// ```
#[repr(C)]
pub struct RootEntry {
    /// `lo`.
    pub lo: AtomicU64,
    /// `hi`.
    pub hi: AtomicU64,
}

impl RootEntry {
    /// An all-zero (not present) entry.
    pub const fn new() -> Self {
        Self {
            lo: AtomicU64::new(0),
            hi: AtomicU64::new(0),
        }
    }
}

impl Default for RootEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct context_entry`: one per devfn (256 x 128 bit = 4k).
///
/// ```text
///   0      = Present
///   1      = Fault Processing Disable
///   2:3    = Translation Type
///   4:11   = Reserved
///   12:63  = Second Level Page Translation
///   64:66  = Address Width (# PTE levels)
///   67:70  = Ignore
///   71     = Reserved
///   72:87  = Domain ID
///   88:127 = Reserved
/// ```
#[repr(C)]
pub struct ContextEntry {
    /// `lo`.
    pub lo: AtomicU64,
    /// `hi`.
    pub hi: AtomicU64,
}

impl ContextEntry {
    /// An all-zero (not present) entry.
    pub const fn new() -> Self {
        Self {
            lo: AtomicU64::new(0),
            hi: AtomicU64::new(0),
        }
    }
}

impl Default for ContextEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct fault_entry`.
///
/// ```text
///   0..HAW-1 = Fault address
///   HAW:63   = Reserved
///   64:71    = Source ID
///   96:103   = Fault Reason
///   104:123  = PV
///   124:125  = Address Translation type
///   126      = Type (0 = Read, 1 = Write)
///   127      = Fault bit
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaultEntry {
    /// `lo`.
    pub lo: u64,
    /// `hi`.
    pub hi: u64,
}

/// `struct pte_entry`: PTE Entry: 512 x 64-bit = 4k.
///
/// ```text
/// 5555555444444444333333333222222222111111111000000000------------
/// [PML4 ->] PDPE.1GB
/// [PML4 ->] PDPE.PDE -> PDE.2MB
/// [PML4 ->] PDPE.PDE -> PDE -> PTE
/// GAW0 = (12.20) (PTE)
/// GAW1 = (21.29) (PDE)
/// GAW2 = (30.38) (PDPE)
/// GAW3 = (39.47) (PML4)
/// GAW4 = (48.57) (n/a)
/// GAW5 = (58.63) (n/a)
/// ```
#[repr(C)]
pub struct PteEntry {
    /// `val`.
    pub val: AtomicU64,
}

impl PteEntry {
    /// `pte->val`.
    pub fn get(&self) -> u64 {
        self.val.load(Ordering::Relaxed)
    }

    /// `pte->val = v`.
    pub fn set(&self, v: u64) {
        self.val.store(v, Ordering::Relaxed);
    }
}

/// `struct qi_entry`: a Queued Invalidation entry.
///
/// ```text
///  0:3   = 01h
///  4:5   = Granularity
///  6:15  = Reserved
///  16:31 = Domain ID
///  32:47 = Source ID
///  48:49 = FM
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QiEntry {
    /// `lo`.
    pub lo: u64,
    /// `hi`.
    pub hi: u64,
}

/// `struct dmar_devlist`: one device scope entry of a remapping structure (an endpoint or
/// a bridge), with its PCI path.
pub struct DmarDevlist {
    /// `type`: `DMAR_ENDPOINT` or `DMAR_BRIDGE`.
    pub r#type: i32,
    /// `bus`: the start bus.
    pub bus: i32,
    /// `ndp`: path entries.
    pub ndp: i32,
    /// `dp`: the path from `bus`, bridge by bridge.
    pub dp: Box<[AcpidmarDevpath]>,
    /// `link`.
    pub link: TailqEntry<DmarDevlist>,
}

crate::queue_adapter!(
    /// `TAILQ_HEAD(devlist_head, dmar_devlist)`.
    pub DevlistHead: DmarDevlist, link => TailqEntry<DmarDevlist>
);

/// `struct domain_dev`: a device of a domain.
#[cfg(machine_x86)]
pub struct DomainDev {
    /// `sid`.
    pub sid: i32,
    /// `sec`.
    pub sec: i32,
    /// `sub`.
    pub sub: i32,
    /// `link`.
    pub link: TailqEntry<DomainDev>,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, domain_dev)`.
    pub DomainDevList: DomainDev, link => TailqEntry<DomainDev>
);

/// `struct domain`: Page Table Entry per domain: the I/O address space of the devices in
/// it, and the `bus_dma` tag they use.
#[cfg(machine_x86)]
pub struct Domain {
    /// `iommu`.
    pub iommu: &'static IommuSoftc,
    /// `did`: the domain id.
    pub did: i32,
    /// `gaw`.
    pub gaw: i32,
    /// `pte`: the top page table (512 entries).
    pub pte: NonNull<PteEntry>,
    /// `ptep`: its physical address.
    pub ptep: u64,
    /// `dmat`: the tag the domain's devices get as `pa_dmat`; `_cookie` is the domain.
    pub dmat: x86::BusDmaTag,
    /// `flag`: `DOM_*`.
    pub flag: Cell<i32>,
    /// `ptlck`: serializes page table growth.
    pub ptlck: Mutex,
    /// `exlck`: protects `iovamap`.
    pub exlck: Mutex,
    /// `exname`: the extent's name (allocated beside the domain).
    pub exname: &'static [u8],
    /// `iovamap`: the I/O virtual addresses in use.
    pub iovamap: &'static Extent,
    /// `devices`.
    pub devices: TailqHead<DomainDevList>,
    /// `link`.
    pub link: TailqEntry<Domain>,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, domain)`.
    pub DomainList: Domain, link => TailqEntry<Domain>
);

/// `struct ppbwin_entry`: a PCI-PCI bridge's forwarding windows.
#[cfg(machine_x86)]
pub struct PpbwinEntry {
    /// `link`.
    pub link: TailqEntry<PpbwinEntry>,
    /// `segment`.
    pub segment: i32,
    /// `sid`: bridge BDF (sid).
    pub sid: u16,
    /// `sec`: secondary bus.
    pub sec: Cell<u8>,
    /// `sub`: subordinate bus.
    pub sub: Cell<u8>,
    /// `mem_base`.
    pub mem_base: Cell<u64>,
    /// `mem_size`.
    pub mem_size: Cell<u64>,
    /// `pmem_base`.
    pub pmem_base: Cell<u64>,
    /// `pmem_size`.
    pub pmem_size: Cell<u64>,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, ppbwin_entry)`.
    pub PpbwinList: PpbwinEntry, link => TailqEntry<PpbwinEntry>
);

/// `struct ivhd_devlist` (declared, unused, in the C).
pub struct IvhdDevlist {
    /// `start_id`.
    pub start_id: i32,
    /// `end_id`.
    pub end_id: i32,
    /// `cfg`.
    pub cfg: i32,
    /// `link`.
    pub link: TailqEntry<IvhdDevlist>,
}

/// `struct rmrr_softc`: a Reserved Memory Region Reporting structure.
#[cfg(machine_x86)]
pub struct RmrrSoftc {
    /// `link`.
    pub link: TailqEntry<RmrrSoftc>,
    /// `devices`.
    pub devices: TailqHead<DevlistHead>,
    /// `segment`.
    pub segment: i32,
    /// `start`.
    pub start: Cell<u64>,
    /// `end`.
    pub end: Cell<u64>,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, rmrr_softc)`.
    pub RmrrList: RmrrSoftc, link => TailqEntry<RmrrSoftc>
);

/// `struct atsr_softc`: a Root Port ATS Capability Reporting structure.
#[cfg(machine_x86)]
pub struct AtsrSoftc {
    /// `link`.
    pub link: TailqEntry<AtsrSoftc>,
    /// `devices`.
    pub devices: TailqHead<DevlistHead>,
    /// `segment`.
    pub segment: i32,
    /// `flags`.
    pub flags: i32,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, atsr_softc)`.
    pub AtsrList: AtsrSoftc, link => TailqEntry<AtsrSoftc>
);

/// `struct ivmd_entry`: an IVMD range (unity or exclusion).
#[cfg(machine_x86)]
pub struct IvmdEntry {
    /// `link`.
    pub link: TailqEntry<IvmdEntry>,
    /// `addr`.
    pub addr: u64,
    /// `size`.
    pub size: u64,
    /// `start_id`.
    pub start_id: u16,
    /// `end_id`.
    pub end_id: u16,
    /// `flags`.
    pub flags: u8,
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, ivmd_entry)`.
    pub IvmdList: IvmdEntry, link => TailqEntry<IvmdEntry>
);

/// `struct iommu_pic`: the `struct pic` of a VT-d unit's fault interrupt (`dmarpic`), with
/// its unit; the `pic_*` functions get the `struct pic` and find the unit around it.
#[cfg(machine_x86)]
#[repr(C)]
pub struct IommuPic {
    /// `pic`: first, so a `&Pic` of `dmarpic` is the address of its `IommuPic`.
    pub pic: x86::Pic,
    /// `iommu`.
    pub iommu: Cell<*const IommuSoftc>,
}

/// `struct iommu_softc`: one DMA remapping unit (a VT-d `DRHD` or an AMD-Vi `IVHD`).
/// Written by its attach, then by the functions that hold `reg_lock` or run in
/// autoconfiguration, as in C.
#[cfg(machine_x86)]
pub struct IommuSoftc {
    /// `link`.
    pub link: TailqEntry<IommuSoftc>,
    /// `devices`: the device scope.
    pub devices: TailqHead<DevlistHead>,
    /// `id`.
    pub id: Cell<i32>,
    /// `flags`: `IOMMU_FLAGS_*`.
    pub flags: Cell<i32>,
    /// `segment`.
    pub segment: Cell<i32>,
    /// `reg_lock`.
    pub reg_lock: Mutex,
    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `cap`.
    pub cap: Cell<u64>,
    /// `ecap`.
    pub ecap: Cell<u64>,
    /// `gcmd`.
    pub gcmd: Cell<u32>,
    /// `mgaw`.
    pub mgaw: Cell<i32>,
    /// `agaw`.
    pub agaw: Cell<i32>,
    /// `ndoms`.
    pub ndoms: Cell<i32>,
    /// `root`: the root table (256 entries).
    pub root: Cell<*const RootEntry>,
    /// `ctx`: each bus's context table (256 entries).
    pub ctx: [Cell<*const ContextEntry>; 256],
    /// `intr`.
    pub intr: Cell<*mut c_void>,
    /// `pic`.
    pub pic: IommuPic,
    /// `fedata`.
    pub fedata: Cell<i32>,
    /// `feaddr`.
    pub feaddr: Cell<u64>,
    /// `rtaddr`.
    pub rtaddr: Cell<u64>,
    /// `qi_head`: Queued Invalidation.
    pub qi_head: Cell<i32>,
    /// `qi_tail`.
    pub qi_tail: Cell<i32>,
    /// `qip`.
    pub qip: Cell<u64>,
    /// `qi`.
    pub qi: Cell<*mut QiEntry>,
    /// `unity`.
    pub unity: Cell<Option<&'static Domain>>,
    /// `domains`.
    pub domains: TailqHead<DomainList>,
    /// `dte`: AMD iommu.
    pub dte: Cell<*const IvhdDte>,
    /// `cmd_tbl`.
    pub cmd_tbl: Cell<*mut u8>,
    /// `evt_tbl`.
    pub evt_tbl: Cell<*mut u8>,
    /// `cmd_tblp`.
    pub cmd_tblp: Cell<u64>,
    /// `evt_tblp`.
    pub evt_tblp: Cell<u64>,
    /// `wait_lock`: AMD completion wait.
    pub wait_lock: Mutex,
    /// `wait_tbl`.
    pub wait_tbl: Cell<*mut u64>,
    /// `wait_tblp`.
    pub wait_tblp: Cell<u64>,
    /// `wait_seq`.
    pub wait_seq: Cell<u32>,
}

#[cfg(machine_x86)]
impl IommuSoftc {
    /// A zeroed unit (`malloc(M_ZERO)`), its `dmarpic` filled in (see the module's
    /// deviations: `acpidmar_intr_establish` names only the unit).
    pub fn new() -> Self {
        Self {
            link: TailqEntry::new(),
            devices: TailqHead::new(),
            id: Cell::new(0),
            flags: Cell::new(0),
            segment: Cell::new(0),
            reg_lock: Mutex::new(IPL_HIGH),
            iot: Cell::new(None),
            ioh: Cell::new(None),
            cap: Cell::new(0),
            ecap: Cell::new(0),
            gcmd: Cell::new(0),
            mgaw: Cell::new(0),
            agaw: Cell::new(0),
            ndoms: Cell::new(0),
            root: Cell::new(ptr::null()),
            ctx: [const { Cell::new(ptr::null()) }; 256],
            intr: Cell::new(ptr::null_mut()),
            pic: IommuPic {
                pic: x86::Pic {
                    pic_name: "dmarpic",
                    pic_type: x86::PIC_MSI,
                    pic_hwmask: Some(acpidmar_msi_hwmask),
                    pic_hwunmask: Some(acpidmar_msi_hwunmask),
                    pic_addroute: Some(acpidmar_msi_addroute),
                    pic_delroute: Some(acpidmar_msi_delroute),
                    pic_allocidtvec: None,
                    pic_level_stubs: None,
                    pic_edge_stubs: Some(x86::ioapic_edge_stubs_table),
                },
                iommu: Cell::new(ptr::null()),
            },
            fedata: Cell::new(0),
            feaddr: Cell::new(0),
            rtaddr: Cell::new(0),
            qi_head: Cell::new(0),
            qi_tail: Cell::new(0),
            qip: Cell::new(0),
            qi: Cell::new(ptr::null_mut()),
            unity: Cell::new(None),
            domains: TailqHead::new(),
            dte: Cell::new(ptr::null()),
            cmd_tbl: Cell::new(ptr::null_mut()),
            evt_tbl: Cell::new(ptr::null_mut()),
            cmd_tblp: Cell::new(0),
            evt_tblp: Cell::new(0),
            wait_lock: Mutex::new(IPL_NONE),
            wait_tbl: Cell::new(ptr::null_mut()),
            wait_tblp: Cell::new(0),
            wait_seq: Cell::new(0),
        }
    }

    /// `iommu->dte != NULL`: an AMD-Vi unit.
    fn is_amd(&self) -> bool {
        !self.dte.get().is_null()
    }

    /// `&iommu->dte[sid]`.
    fn dte_at(&self, sid: i32) -> Option<&'static IvhdDte> {
        let dte = self.dte.get();
        if dte.is_null() || !(0..MAX_DEVFN as i32).contains(&sid) {
            return None;
        }
        // SAFETY: `dte` is the device table `iommu_alloc_hwdte` allocated (65536 entries,
        // never freed); `sid` is in range.
        Some(unsafe { &*dte.add(sid as usize) })
    }

    /// `iommu->root`: the root table, once `iommu_init` allocated it.
    fn root_table(&self) -> Option<&'static [RootEntry]> {
        let root = self.root.get();
        if root.is_null() {
            return None;
        }
        // SAFETY: a zeroed page of 256 root entries `iommu_init` allocated and never frees.
        Some(unsafe { core::slice::from_raw_parts(root, 256) })
    }

    /// `iommu->ctx[bus]`: the bus's context table, once `domain_map_device` allocated it.
    fn ctx_table(&self, bus: i32) -> Option<&'static [ContextEntry]> {
        let ctx = self.ctx.get(bus as usize)?.get();
        if ctx.is_null() {
            return None;
        }
        // SAFETY: a zeroed page of 256 context entries `domain_map_device` allocated and
        // never frees.
        Some(unsafe { core::slice::from_raw_parts(ctx, 256) })
    }
}

#[cfg(machine_x86)]
impl Default for IommuSoftc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(machine_x86)]
crate::queue_adapter!(
    /// `TAILQ_HEAD(, iommu_softc)`.
    pub IommuList: IommuSoftc, link => TailqEntry<IommuSoftc>
);

/// `struct acpidmar_softc`.
#[cfg(machine_x86)]
#[repr(C)]
pub struct AcpidmarSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_pc`.
    pub sc_pc: Cell<PciChipsetTag>,
    /// `sc_memt`.
    pub sc_memt: Cell<Option<BusSpaceTag>>,
    /// `sc_haw`.
    pub sc_haw: Cell<i32>,
    /// `sc_flags`.
    pub sc_flags: Cell<i32>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<crate::machine::bus::BusDmaTag>>,
    /// `sc_hwdte`.
    pub sc_hwdte: Cell<*const IvhdDte>,
    /// `sc_hwdtep`.
    pub sc_hwdtep: Cell<u64>,
    /// `sc_drhds`.
    pub sc_drhds: TailqHead<IommuList>,
    /// `sc_rmrrs`.
    pub sc_rmrrs: TailqHead<RmrrList>,
    /// `sc_atsrs`.
    pub sc_atsrs: TailqHead<AtsrList>,
    /// `sc_ivmds`.
    pub sc_ivmds: TailqHead<IvmdList>,
    /// `sc_ppbwins`.
    pub sc_ppbwins: TailqHead<PpbwinList>,
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is no chipset, no tag, null tables
// and empty lists (an `Option<BusSpaceTag>` of zeroes is a valid value, set by the attach).
#[cfg(machine_x86)]
unsafe impl Softc for AcpidmarSoftc {}

/// `struct dmar_map_cookie`: a map's extent descriptors (one per segment) and the flags it
/// was created with.
#[cfg(machine_x86)]
struct DmarMapCookie {
    /// `dm_er`.
    dm_er: NonNull<ExtentRegion>,
    /// `dm_orig_flags`.
    dm_orig_flags: i32,
}

/// `acpidmar_ddb` (`#ifdef DDB`): enter ddb on a fault.
#[cfg(machine_x86)]
static ACPIDMAR_DDB: AtomicI32 = AtomicI32::new(0);

/// `acpidmar_force_cm`.
#[cfg(machine_x86)]
static ACPIDMAR_FORCE_CM: AtomicI32 = AtomicI32::new(1);

/// `sid_flag[MAX_DEVFN]`.
#[cfg(machine_x86)]
static SID_FLAG: [AtomicU32; MAX_DEVFN] = [const { AtomicU32::new(0) }; MAX_DEVFN];

/// `acpidmar_sc`: the attached acpidmar0 (null before).
#[cfg(machine_x86)]
static ACPIDMAR_SC: AtomicPtr<AcpidmarSoftc> = AtomicPtr::new(ptr::null_mut());

/// The C's `domain_map_page` pointer: `domain_map_page_amd` when true, else
/// `domain_map_page_intel`.
#[cfg(machine_x86)]
pub static DOMAIN_MAP_PAGE_AMD: AtomicBool = AtomicBool::new(false);

/// `iommu_init`'s `static int niommu`.
#[cfg(machine_x86)]
static NIOMMU_INTEL: AtomicI32 = AtomicI32::new(0);

/// `ivhd_iommu_init`'s `static int niommu`.
#[cfg(machine_x86)]
static NIOMMU_AMD: AtomicI32 = AtomicI32::new(0);

/// `ivhd_showpage`'s `static int show`.
#[cfg(machine_x86)]
static IVHD_SHOW: AtomicI32 = AtomicI32::new(0);

/// `acpidmar_intr`'s `static struct fault_entry ofe`: the last fault shown.
#[cfg(machine_x86)]
static OFE: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// `acpidmar_ca`.
#[cfg(machine_x86)]
pub static ACPIDMAR_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpidmarSoftc>(),
    ca_match: Some(acpidmar_match),
    ca_attach: acpidmar_attach,
    ca_detach: None,
    ca_activate: Some(acpidmar_activate),
};

/// `acpidmar_cd`.
#[cfg(machine_x86)]
pub static ACPIDMAR_CD: Cfdriver = Cfdriver::new(b"acpidmar", DV_DULL, 0);

/// `vtd_faults[]`.
pub static VTD_FAULTS: [&str; 15] = [
    "Software",
    "Root Entry Not Present",    // ok (rtaddr + 4096)
    "Context Entry Not Present", // ok (no CTX_P)
    "Context Entry Invalid",     // ok (tt = 3)
    "Address Beyond MGAW",
    "Write",                // ok
    "Read",                 // ok
    "Paging Entry Invalid", // ok
    "Root Table Invalid",
    "Context Table Invalid",
    "Root Entry Reserved", // ok (root.lo |= 0x4)
    "Context Entry Reserved",
    "Paging Entry Reserved",
    "Context Entry TT",
    "Reserved",
];

/// `acpidmar_dbg_lvl` (`IOMMU_DEBUG`).
static ACPIDMAR_DBG_LVL: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// `context_set_fpd(ce, enable)`: Set fault processing enable/disable.
pub fn context_set_fpd(ce: &ContextEntry, enable: bool) {
    let mut lo = ce.lo.load(Ordering::Relaxed) & !CTX_FPD;
    if enable {
        lo |= CTX_FPD;
    }
    ce.lo.store(lo, Ordering::Relaxed);
}

/// `root_entry_is_valid(re)`: Check if root entry is valid.
pub fn root_entry_is_valid(re: &RootEntry) -> bool {
    re.lo.load(Ordering::Relaxed) & ROOT_P != 0
}

/// `context_set_present(ce)`: Set context entry present.
pub fn context_set_present(ce: &ContextEntry) {
    ce.lo.fetch_or(CTX_P, Ordering::Relaxed);
}

/// `context_set_slpte(ce, slpte)`: Set Second Level Page Table Entry PA.
pub fn context_set_slpte(ce: &ContextEntry, slpte: u64) {
    let lo = ce.lo.load(Ordering::Relaxed) & VTD_PAGE_MASK as u64;
    ce.lo
        .store(lo | (slpte & !(VTD_PAGE_MASK as u64)), Ordering::Relaxed);
}

/// `context_set_translation_type(ce, tt)`: Set translation type.
pub fn context_set_translation_type(ce: &ContextEntry, tt: i32) {
    let lo = ce.lo.load(Ordering::Relaxed) & !(CTX_T_MASK << CTX_T_SHIFT);
    ce.lo.store(
        lo | ((tt as u64 & CTX_T_MASK) << CTX_T_SHIFT),
        Ordering::Relaxed,
    );
}

/// `context_set_address_width(ce, lvl)`: Set Address Width (# of Page Table levels).
pub fn context_set_address_width(ce: &ContextEntry, lvl: i32) {
    let hi = ce.hi.load(Ordering::Relaxed) & !(CTX_H_AW_MASK << CTX_H_AW_SHIFT);
    ce.hi.store(
        hi | ((lvl as u64 & CTX_H_AW_MASK) << CTX_H_AW_SHIFT),
        Ordering::Relaxed,
    );
}

/// `context_set_domain_id(ce, did)`: Set domain ID.
pub fn context_set_domain_id(ce: &ContextEntry, did: i32) {
    let hi = ce.hi.load(Ordering::Relaxed) & !(CTX_H_DID_MASK << CTX_H_DID_SHIFT);
    ce.hi.store(
        hi | ((did as u64 & CTX_H_DID_MASK) << CTX_H_DID_SHIFT),
        Ordering::Relaxed,
    );
}

/// `context_pte(ce)`: Get Second Level Page Table PA.
pub fn context_pte(ce: &ContextEntry) -> u64 {
    ce.lo.load(Ordering::Relaxed) & !(VTD_PAGE_MASK as u64)
}

/// `context_translation_type(ce)`: Get translation type.
pub fn context_translation_type(ce: &ContextEntry) -> i32 {
    ((ce.lo.load(Ordering::Relaxed) >> CTX_T_SHIFT) & CTX_T_MASK) as i32
}

/// `context_domain_id(ce)`: Get domain ID.
pub fn context_domain_id(ce: &ContextEntry) -> i32 {
    ((ce.hi.load(Ordering::Relaxed) >> CTX_H_DID_SHIFT) & CTX_H_DID_MASK) as i32
}

/// `context_address_width(ce)`: Get Address Width.
pub fn context_address_width(ce: &ContextEntry) -> i32 {
    vtd_leveltoaw(((ce.hi.load(Ordering::Relaxed) >> CTX_H_AW_SHIFT) & CTX_H_AW_MASK) as i32)
}

/// `context_entry_is_valid(ce)`: Check if context entry is valid.
pub fn context_entry_is_valid(ce: &ContextEntry) -> bool {
    ce.lo.load(Ordering::Relaxed) & CTX_P != 0
}

/// `context_user(ce)`: User-available bits in context entry.
pub fn context_user(ce: &ContextEntry) -> i32 {
    ((ce.hi.load(Ordering::Relaxed) >> CTX_H_USER_SHIFT) & CTX_H_USER_MASK) as i32
}

/// `context_set_user(ce, v)`.
pub fn context_set_user(ce: &ContextEntry, v: i32) {
    let hi = ce.hi.load(Ordering::Relaxed) & !(CTX_H_USER_MASK << CTX_H_USER_SHIFT);
    ce.hi.store(
        hi | ((v as u64 & CTX_H_USER_MASK) << CTX_H_USER_SHIFT),
        Ordering::Relaxed,
    );
}

/// `dmar_bdf(sid)`: `segment:bus:dev.fun` of a source id (segment 0, as the C prints).
pub fn dmar_bdf(sid: i32) -> DmarBdf {
    DmarBdf(sid)
}

/// What [`dmar_bdf`] returns: formats as `%.4x:%.2x:%.2x.%x`.
#[derive(Clone, Copy, Debug)]
pub struct DmarBdf(pub i32);

impl fmt::Display for DmarBdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04x}:{:02x}:{:02x}.{:x}",
            0,
            sid_bus(self.0),
            sid_dev(self.0),
            sid_fun(self.0)
        )
    }
}

/// Reads a `T` (a packed ACPI structure) at `off` in `b`, `None` past the end.
fn rd<T: Copy>(b: &[u8], off: usize) -> Option<T> {
    let end = off.checked_add(size_of::<T>())?;
    if end > b.len() {
        return None;
    }
    // SAFETY: `off..end` is inside `b`; `T` is a packed structure of integers, valid for any
    // bytes, read unaligned.
    Some(unsafe { ptr::read_unaligned(b.as_ptr().add(off).cast::<T>()) })
}

/// `acpidmar_parse_devscope(de, off, segment, devlist)`: Create list of device scope
/// entries from ACPI table. `de` is the remapping structure (its `length` bytes), `off` where
/// its device scope entries start.
pub fn acpidmar_parse_devscope(
    de: &[u8],
    mut off: usize,
    segment: i32,
    devlist: &TailqHead<DevlistHead>,
) {
    devlist.init();
    while off < de.len() {
        let Some(ds) = rd::<AcpidmarDevscope>(de, off) else {
            break;
        };
        let start = off;
        // A zero-length entry ends the walk (the C would loop for ever).
        if ds.length == 0 {
            break;
        }
        off += usize::from(ds.length);

        // We only care about bridges and endpoints
        if ds.r#type != DMAR_ENDPOINT && ds.r#type != DMAR_BRIDGE {
            continue;
        }

        let dplen = usize::from(ds.length).saturating_sub(size_of::<AcpidmarDevscope>());
        let ndp = dplen / 2;
        let base = start + size_of::<AcpidmarDevscope>();
        let dp: Box<[AcpidmarDevpath]> = (0..ndp)
            .map(|i| {
                rd::<AcpidmarDevpath>(de, base + 2 * i).unwrap_or(AcpidmarDevpath {
                    device: 0,
                    function: 0,
                })
            })
            .collect();
        let d = Box::leak(Box::new(DmarDevlist {
            r#type: i32::from(ds.r#type),
            bus: i32::from(ds.bus),
            ndp: ndp as i32,
            dp,
            link: TailqEntry::new(),
        }));
        // SAFETY: a fresh element, leaked so it stays in place for the list's lifetime.
        unsafe { devlist.insert_tail(d) };

        if let Some(p0) = d.dp.first() {
            dprintf!(
                1,
                "  {:>8}  {:04x}:{:02x}.{:02x}.{:x} {{",
                if ds.r#type == DMAR_BRIDGE {
                    "bridge"
                } else {
                    "endpoint"
                },
                segment,
                ds.bus,
                p0.device,
                p0.function
            );
        }
        for p in d.dp.iter().skip(1) {
            dprintf!(1, " {:2x}.{:x} ", p.device, p.function);
        }
        dprintf!(1, "}}\n");
    }
}

/// `iommu_intel_pick_am(addr, end, max_am)`: the largest address mask (`2^am` pages) whose
/// size-aligned range starts at `addr` and fits before `end`; 0 invalidates exactly one 4 KB
/// page.
pub fn iommu_intel_pick_am(addr: usize, end: usize, max_am: i32) -> i32 {
    // AM=0 invalidates exactly one 4KB page
    if max_am <= 0 {
        return 0;
    }

    let mut am = max_am;
    while am > 0 {
        let bytes = (VTD_PAGE_SIZE as u64) << am;
        if (addr as u64) & bytes.wrapping_sub(1) != 0 || ((end - addr) as u64) < bytes {
            am -= 1;
            continue;
        }
        return am;
    }
    0
}

/// The page-aligned `[start, end)` of each nonempty segment `(ds_addr, ds_len)`.
fn seg_ranges<I>(segs: I) -> impl Iterator<Item = (usize, usize)> + Clone
where
    I: Iterator<Item = (usize, usize)> + Clone,
{
    segs.filter(|&(_, len)| len != 0).map(|(addr, len)| {
        (
            addr & !VTD_PAGE_MASK,
            (addr + len).next_multiple_of(VTD_PAGE_SIZE),
        )
    })
}

/// What `iommu_flush_tlb_segs` counts before choosing how to invalidate: the pages of the
/// segments and the descriptors they take with address masks up to `max_am`
/// (`npages`, `ndescs`).
pub fn iommu_flush_tlb_counts<I>(segs: I, max_am: i32) -> (u64, u64)
where
    I: Iterator<Item = (usize, usize)> + Clone,
{
    let mut npages = 0u64;
    let mut ndescs = 0u64;
    for (start, end) in seg_ranges(segs) {
        if end > start {
            npages += ((end - start) / VTD_PAGE_SIZE) as u64;
        }
        let mut addr = start;
        while addr < end {
            let am = iommu_intel_pick_am(addr, end, max_am);
            ndescs += 1;
            addr += VTD_PAGE_SIZE << am;
        }
    }
    (npages, ndescs)
}

/// `acpidmar_sc`.
#[cfg(machine_x86)]
fn acpidmar_sc() -> Option<&'static AcpidmarSoftc> {
    let sc = ACPIDMAR_SC.load(Ordering::Acquire);
    // SAFETY: set by `acpidmar_attach` to its softc, which is never freed (acpidmar does
    // not detach).
    unsafe { sc.as_ref() }
}

/// `iommu_bad(sc)`.
#[cfg(machine_x86)]
fn iommu_bad(sc: &IommuSoftc) -> bool {
    sc.flags.get() & IOMMU_FLAGS_BAD != 0
}

/// `iommu_enabled(sc)`.
#[cfg(machine_x86)]
fn iommu_enabled(sc: &IommuSoftc) -> bool {
    if sc.is_amd() {
        return true;
    }
    sc.gcmd.get() & GCMD_TE != 0
}

/// The domain whose tag `tag` is (`tag->_cookie`).
#[cfg(machine_x86)]
fn tag_domain(tag: &x86::BusDmaTag) -> &'static Domain {
    // SAFETY: only `domain_create`'s tags hold the dmar functions, and their cookie is the
    // domain, which is never freed once its tag was handed out.
    unsafe { &*tag._cookie.cast_const().cast::<Domain>() }
}

/// The page table at physical address `pa` (`PMAP_DIRECT_MAP`).
#[cfg(machine_x86)]
fn pte_table(pa: u64) -> &'static [PteEntry] {
    let va = x86::pmap_direct_map(Paddr::new(pa as usize));
    // SAFETY: `pa` is a page table page this driver allocated (`iommu_alloc_page`) and linked
    // into a domain, never freed while linked; the direct map covers it.
    unsafe { core::slice::from_raw_parts(va.as_usize() as *const PteEntry, 512) }
}

/// The domain's top page table.
#[cfg(machine_x86)]
fn dom_pte(dom: &Domain) -> &'static [PteEntry] {
    // SAFETY: the domain's top table is a zeroed page of 512 entries `domain_create`
    // allocated, never freed while the domain lives (domains live for ever once listed).
    unsafe { core::slice::from_raw_parts(dom.pte.as_ptr(), 512) }
}

/// The map's cookie (`dmam->_dm_cookie`), if `dmar_dmamap_create` made it.
#[cfg(machine_x86)]
fn map_cookie(dmam: &x86::BusDmamap) -> Option<&DmarMapCookie> {
    let mc = dmam._dm_cookie.get().cast::<DmarMapCookie>();
    // SAFETY: a dmar map's cookie is the `DmarMapCookie` `dmar_dmamap_create` allocated, alive
    // until `dmar_dmamap_destroy`.
    unsafe { mc.as_ref() }
}

/// The valid segments of `map` as `(ds_addr, ds_len)`.
#[cfg(machine_x86)]
fn map_segs(map: &x86::BusDmamap) -> impl Iterator<Item = (BusAddr, BusSize)> + Clone + '_ {
    let n = (map.dm_nsegs.get().max(0) as usize).min(map.dm_segs().len());
    map.dm_segs()[..n].iter().map(|c| {
        let s = c.get();
        (s.ds_addr, s.ds_len)
    })
}

/// `dmar_dmamap_origflags(dmam)`.
#[cfg(machine_x86)]
fn dmar_dmamap_origflags(dmam: &x86::BusDmamap) -> i32 {
    match map_cookie(dmam) {
        Some(mc) => mc.dm_orig_flags,
        None => dmam._dm_flags,
    }
}

/// `debugme(dom)`: the C returns 0 before looking at `DOM_DEBUG`.
#[cfg(machine_x86)]
fn debugme(dom: &Domain) -> bool {
    const DEBUGME: bool = false;
    DEBUGME && dom.flag.get() & DOM_DEBUG != 0
}

/// `domain_map_check(dom)`: makes sure every device of the domain has its context or
/// device table entry, and prints the first mapping of each (VT-d).
#[cfg(machine_x86)]
pub fn domain_map_check(dom: &Domain) {
    let iommu = dom.iommu;
    for dd in dom.devices.iter() {
        let _ = acpidmar_pci_attach(acpidmar_sc(), iommu.segment.get(), dd.sid, true);

        if iommu.is_amd() {
            continue;
        }

        // Check if this is the first time we are mapped
        let Some(ctx) = iommu
            .ctx_table(sid_bus(dd.sid))
            .map(|t| &t[sid_devfn(dd.sid) as usize])
        else {
            continue;
        };
        let v = context_user(ctx);
        if v != 0xA {
            kprintf!(
                "  map: {:04x}:{:02x}:{:02x}.{:x} iommu:{} did:{:04x}\n",
                iommu.segment.get(),
                sid_bus(dd.sid),
                sid_dev(dd.sid),
                sid_fun(dd.sid),
                iommu.id.get(),
                dom.did
            );
            context_set_user(ctx, 0xA);
        }
    }
}

/// `dmar_ptmap(tag, addr)`: Map a single page as passthrough - used for DRM.
#[cfg(machine_x86)]
pub fn dmar_ptmap(tag: crate::machine::bus::BusDmaTag, addr: BusAddr) {
    if acpidmar_sc().is_none() {
        return;
    }
    let dom = tag_domain(tag);
    let iommu = dom.iommu;
    domain_map_check(dom);
    if let Err(error) = domain_map_page(dom, addr as u64, addr as u64, PTE_P | PTE_R | PTE_W, false)
    {
        kprintf!(
            "{}: ptmap {:016x} failed ({})\n",
            dom_bdf(dom),
            addr,
            error as i32
        );
        return;
    }

    // Ensure the new translation is visible
    if iommu_enabled(iommu) {
        iommu_flush_write_buffer(iommu);
        iommu_flush_tlb_page(iommu, dom.did, addr);
    }
}

/// `domain_map_pthru(dom, start, end)`: Map a range of pages 1:1.
#[cfg(machine_x86)]
pub fn domain_map_pthru(dom: &Domain, mut start: u64, end: u64) -> Result<(), Errno> {
    let iommu = dom.iommu;
    let mut error = Ok(());

    domain_map_check(dom);
    while start < end {
        error = domain_map_page(dom, start, start, PTE_P | PTE_R | PTE_W, false);
        if error.is_err() {
            break;
        }
        start += VTD_PAGE_SIZE as u64;
    }
    if let Err(e) = error {
        kprintf!("{}: pthru map failed ({})\n", dom_bdf(dom), e as i32);
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(e);
    }

    // Ensure translations are visible
    if iommu_enabled(iommu) {
        iommu_flush_write_buffer(iommu);
        iommu_flush_tlb(iommu, IOTLB_DOMAIN, dom.did);
    }
    Ok(())
}

/// `domain_map_page`: the unit's flavour of `domain_map_page_intel`/`_amd` (see the
/// module's deviations).
#[cfg(machine_x86)]
pub fn domain_map_page(
    dom: &Domain,
    va: u64,
    pa: u64,
    flags: u64,
    nowait: bool,
) -> Result<(), Errno> {
    if DOMAIN_MAP_PAGE_AMD.load(Ordering::Relaxed) {
        domain_map_page_amd(dom, va, pa, flags, nowait)
    } else {
        domain_map_page_intel(dom, va, pa, flags, nowait)
    }
}

/// `domain_map_page_intel(dom, va, pa, flags, nowait)`: Map a single paddr to IOMMU paddr;
/// without `PTE_P` in `flags` it clears the mapping (allocating no table for that).
#[cfg(machine_x86)]
pub fn domain_map_page_intel(
    dom: &Domain,
    va: u64,
    pa: u64,
    flags: u64,
    nowait: bool,
) -> Result<(), Errno> {
    let iommu = dom.iommu;
    let create = flags & PTE_P != 0;

    // Only handle 4k pages for now
    let mut npte = dom_pte(dom);
    let mut lvl = iommu.agaw.get() - VTD_STRIDE_SIZE;
    while lvl >= VTD_LEVEL0 {
        let idx = ((va >> lvl) & VTD_STRIDE_MASK) as usize;
        let pte = &npte[idx];

        if lvl == VTD_LEVEL0 {
            // Level 1: Page Table
            pte.set(if create { pa | flags } else { 0 });
            iommu_flush_cache(iommu, ptr::from_ref(pte).cast(), size_of::<PteEntry>());
            return Ok(());
        }

        let mut val = pte.get();
        if val & PTE_P == 0 {
            if !create {
                return Ok(());
            }

            let Some((newva, paddr)) = iommu_alloc_page(iommu, nowait) else {
                return Err(Errno::ENOMEM);
            };

            // Avoid concurrent mapping races
            let mut newva = Some(newva);
            mtx_enter(&dom.ptlck);
            val = pte.get();
            if val & PTE_P == 0 {
                pte.set(paddr | PTE_P | PTE_R | PTE_W);
                iommu_flush_cache(iommu, ptr::from_ref(pte).cast(), size_of::<PteEntry>());
                val = pte.get();
                newva = None; // owned by the page tables
            }
            mtx_leave(&dom.ptlck);

            if let Some(v) = newva {
                iommu_free_page(v);
            }
        }

        npte = pte_table(val & VTD_PTE_MASK);
        lvl -= VTD_STRIDE_SIZE;
    }

    Ok(())
}

/// `pte_lvl(dom, pte, va, shift, flags, nowait, create, error)`: the next level's table of
/// the entry for `va` at `shift` in `pte`, allocated (with `flags`) if missing and
/// `create`. `Ok(None)` when it is missing and not created.
///
/// AMD physical address breakdown into levels:
/// ```text
/// xxxxxxxx.xxxxxxxx.xxxxxxxx.xxxxxxxx.xxxxxxxx.xxxxxxxx.xxxxxxxx.xxxxxxxx
///        5.55555555.44444444.43333333,33222222.22211111.1111----.--------
/// mode:
///  000 = none   shift
///  001 = 1 [21].12
///  010 = 2 [30].21
///  011 = 3 [39].30
///  100 = 4 [48].39
///  101 = 5 [57]
///  110 = 6
///  111 = reserved
/// ```
#[cfg(machine_x86)]
pub fn pte_lvl(
    dom: &Domain,
    pte: &[PteEntry],
    va: u64,
    shift: u32,
    flags: u64,
    nowait: bool,
    create: bool,
) -> Result<Option<&'static [PteEntry]>, Errno> {
    let iommu = dom.iommu;

    let idx = ((va >> shift) & VTD_STRIDE_MASK) as usize;
    let mut val = pte[idx].get();

    if val & PTE_P == 0 {
        // Don't allocate lower levels for an unmap
        if !create {
            return Ok(None);
        }

        let Some((newva, paddr)) = iommu_alloc_page(iommu, nowait) else {
            return Err(Errno::ENOMEM);
        };

        let mut newva = Some(newva);
        mtx_enter(&dom.ptlck);
        val = pte[idx].get();
        if val & PTE_P == 0 {
            pte[idx].set(paddr | flags);
            iommu_flush_cache(
                iommu,
                ptr::from_ref(&pte[idx]).cast(),
                size_of::<PteEntry>(),
            );
            val = pte[idx].get();
            newva = None; // owned by the page tables
        }
        mtx_leave(&dom.ptlck);

        if let Some(v) = newva {
            iommu_free_page(v);
        }
    }

    Ok(Some(pte_table(val & PTE_PADDR_MASK)))
}

/// `domain_map_page_amd(dom, va, pa, flags, nowait)`.
#[cfg(machine_x86)]
pub fn domain_map_page_amd(
    dom: &Domain,
    va: u64,
    pa: u64,
    flags: u64,
    nowait: bool,
) -> Result<(), Errno> {
    let iommu = dom.iommu;
    let create = flags & PTE_P != 0;

    // Always assume AMD levels=4
    //        39        30        21        12
    // ---------|---------|---------|---------|------------
    let pte = dom_pte(dom);
    let Some(pte) = pte_lvl(
        dom,
        pte,
        va,
        30,
        pte_nxtlvl(2) | PTE_IR | PTE_IW | PTE_P,
        nowait,
        create,
    )?
    else {
        return Ok(());
    };
    let Some(pte) = pte_lvl(
        dom,
        pte,
        va,
        21,
        pte_nxtlvl(1) | PTE_IR | PTE_IW | PTE_P,
        nowait,
        create,
    )?
    else {
        return Ok(());
    };

    let flags = if create {
        PTE_P | PTE_R | PTE_W | PTE_IW | PTE_IR | pte_nxtlvl(0)
    } else {
        0
    };

    // Level 1: Page Table
    let idx = ((va >> 12) & 0x1FF) as usize;
    pte[idx].set(if create { pa | flags } else { 0 });
    iommu_flush_cache(
        iommu,
        ptr::from_ref(&pte[idx]).cast(),
        size_of::<PteEntry>(),
    );

    Ok(())
}

/// `dmar_dumpseg(tag, nseg, segs, lbl)`: the C returns at once.
#[cfg(machine_x86)]
fn dmar_dumpseg(tag: &x86::BusDmaTag, segs: impl Iterator<Item = (BusAddr, BusSize)>, lbl: &str) {
    const DUMPSEG: bool = false;
    if !DUMPSEG {
        return;
    }
    let dom = tag_domain(tag);
    if !debugme(dom) {
        return;
    }
    kprintf!("{}: {}\n", lbl, dom_bdf(dom));
    for (addr, len) in segs {
        kprintf!("  {:016x} {:08x}\n", addr, len as u32);
    }
}

/// `domain_unload_map(dom, dmam)`: Unload mapping.
#[cfg(machine_x86)]
pub fn domain_unload_map(dom: &Domain, dmam: &x86::BusDmamap) {
    let iommu = dom.iommu;
    if iommu_bad(iommu) {
        kprintf!("unload map no iommu\n");
        return;
    }
    if dmam.dm_nsegs.get() == 0 {
        return;
    }

    // First pass: clear PTEs
    for (addr, len) in map_segs(dmam) {
        let base = trunc_page(addr);
        let end = roundup(addr + len, VTD_PAGE_SIZE);
        let alen = end - base;

        if debugme(dom) {
            kprintf!("  va:{:016x} len:{:x}\n", base, alen as u32);
        }

        for idx in (0..alen).step_by(VTD_PAGE_SIZE) {
            let _ = domain_map_page(dom, (base + idx) as u64, 0, 0, true);
        }
    }

    // Flush translations
    iommu_flush_write_buffer(iommu);
    iommu_flush_tlb_segs(iommu, dom.did, map_segs(dmam));

    // Second pass: free IOVA space
    for (addr, len) in map_segs(dmam) {
        let base = trunc_page(addr);
        let end = roundup(addr + len, VTD_PAGE_SIZE);
        let alen = end - base;

        if dom.flag.get() & DOM_NOMAP != 0 {
            kprintf!("{}: nomap {:016x}\n", dom_bdf(dom), base);
            continue;
        }

        mtx_enter(&dom.exlck);
        let error = extent_free(dom.iovamap, base as u64, alen as u64, EX_NOWAIT);
        mtx_leave(&dom.exlck);

        kassert!(error.is_ok());
        let _ = error;
    }
}

/// `domain_load_map(dom, map, flags, pteflag, fn)`: map.segs\[x\].ds_addr is modified to
/// IOMMU virtual PA.
#[cfg(machine_x86)]
pub fn domain_load_map(
    dom: &Domain,
    map: &x86::BusDmamap,
    flags: i32,
    pteflag: u64,
    _fn: &str,
) -> Result<(), Errno> {
    let iommu = dom.iommu;
    if iommu_bad(iommu) {
        return Err(Errno::ENXIO);
    }
    if !iommu_enabled(iommu) {
        // Lazy enable translation when required
        iommu_enable_translation(iommu, true)?;
    }
    let Some(mc) = map_cookie(map) else {
        return Err(Errno::EINVAL);
    };

    let mflags = dmar_dmamap_origflags(map);
    let pt_nowait = flags & BUS_DMA_NOWAIT != 0;
    let mut mapped_nsegs = 0;

    // A boundary presented to bus_dmamem_alloc() takes precedence over boundary in the map.
    let seg0 = map.seg(0).get();
    let mut boundary = seg0._ds_boundary;
    if boundary == 0 {
        boundary = map._dm_boundary;
    }
    let align = seg0._ds_align.max(VTD_PAGE_SIZE);

    let nsegs = (map.dm_nsegs.get().max(0) as usize).min(map.dm_segs().len());
    let mut result = Ok(());
    'segs: for i in 0..nsegs {
        let cell = map.seg(i);
        let mut seg = cell.get();

        let base = trunc_page(seg.ds_addr);
        let end = roundup(seg.ds_addr + seg.ds_len, VTD_PAGE_SIZE);
        let alen = end - base;
        let mut res = base as u64;

        if dom.flag.get() & DOM_NOMAP == 0 {
            // Allocate DMA Virtual Address
            let sgstart = dom.iovamap.ex_start;
            let mut sgend = dom.iovamap.ex_end;

            if mflags & x86::BUS_DMA_24BIT != 0 {
                if sgend > 0x00ff_ffff {
                    sgend = 0x00ff_ffff;
                }
            } else if mflags & BUS_DMA_64BIT == 0 && sgend > 0xffff_ffff {
                sgend = 0xffff_ffff;
            }

            mtx_enter(&dom.exlck);
            // SAFETY: descriptor `i` of the map's cookie (one per segment) is in no extent:
            // the map is not loaded (a previous load was undone by its unload); it lives as
            // long as the map.
            let r = unsafe {
                extent_alloc_subregion_with_descr(
                    dom.iovamap,
                    sgstart,
                    sgend,
                    alen as u64,
                    align as u64,
                    0,
                    boundary as u64,
                    EX_NOWAIT,
                    &*mc.dm_er.as_ptr().add(i),
                )
            };
            mtx_leave(&dom.exlck);
            match r {
                Ok(v) => res = v,
                Err(e) => {
                    result = Err(e);
                    break 'segs;
                }
            }

            if debugme(dom) {
                kprintf!(
                    "  LOADMAP: {:016x} {:x} => {:016x}\n",
                    seg.ds_addr,
                    seg.ds_len as u32,
                    res
                );
            }

            // Reassign DMA address
            seg.ds_addr = res as usize | (seg.ds_addr & VTD_PAGE_MASK);
            cell.set(seg);
        }

        mapped_nsegs = i + 1;

        for idx in (0..alen).step_by(VTD_PAGE_SIZE) {
            if let Err(e) = domain_map_page(
                dom,
                res + idx as u64,
                (base + idx) as u64,
                PTE_P | pteflag,
                pt_nowait,
            ) {
                result = Err(e);
                break 'segs;
            }
        }
    }

    if result.is_ok() {
        iommu_flush_write_buffer(iommu);
        iommu_flush_tlb_segs(iommu, dom.did, map_segs(map));
        return Ok(());
    }

    // fail:
    if mapped_nsegs > 0 {
        let save_nsegs = map.dm_nsegs.get();

        map.dm_nsegs.set(mapped_nsegs as i32);
        domain_unload_map(dom, map);
        map.dm_nsegs.set(save_nsegs);
    }
    result
}

/// `dom_bdf(dom)`: the domain's first device, unit and id.
#[cfg(machine_x86)]
pub fn dom_bdf(dom: &Domain) -> DomBdf<'_> {
    DomBdf(dom)
}

/// What [`dom_bdf`] returns: formats as `%s iommu:%d did:%.4x%s`.
#[cfg(machine_x86)]
pub struct DomBdf<'a>(&'a Domain);

#[cfg(machine_x86)]
impl fmt::Display for DomBdf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dom = self.0;
        let sid = dom.devices.first().map_or(0, |dd| dd.sid);
        write!(
            f,
            "{} iommu:{} did:{:04x}{}",
            dmar_bdf(sid),
            dom.iommu.id.get(),
            dom.did,
            if dom.did == DID_UNITY { " [unity]" } else { "" }
        )
    }
}

/// `dmar_dmamap_create`: Bus DMA Map functions.
#[cfg(machine_x86)]
fn dmar_dmamap_create(
    tag: x86::BusDmaTagT,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<x86::BusDmamapT, Errno> {
    let dmam = x86::_bus_dmamap_create(
        tag,
        size,
        nsegments,
        maxsegsz,
        boundary,
        flags | BUS_DMA_64BIT,
    )?;

    let mflags = if flags & BUS_DMA_NOWAIT != 0 {
        M_NOWAIT
    } else {
        M_WAITOK
    };
    let Some(mc) = malloc(size_of::<DmarMapCookie>(), M_DEVBUF, mflags) else {
        // SAFETY: the map was just created and is used by nobody else.
        unsafe { x86::_bus_dmamap_destroy(tag, NonNull::from(dmam)) };
        return Err(Errno::ENOMEM);
    };
    let segcnt = dmam._dm_segcnt as usize;
    let er_size = segcnt.saturating_mul(size_of::<ExtentRegion>());
    let Some(er) = malloc(er_size, M_DEVBUF, mflags) else {
        free(mc, M_DEVBUF, size_of::<DmarMapCookie>());
        // SAFETY: as above.
        unsafe { x86::_bus_dmamap_destroy(tag, NonNull::from(dmam)) };
        return Err(Errno::ENOMEM);
    };
    let er = er.cast::<ExtentRegion>();
    let mc = mc.cast::<DmarMapCookie>();
    // SAFETY: fresh allocations of the right sizes, aligned by malloc(9) for any type.
    unsafe {
        for i in 0..segcnt {
            er.as_ptr().add(i).write(ExtentRegion::new());
        }
        mc.write(DmarMapCookie {
            dm_er: er,
            dm_orig_flags: flags & !(BUS_DMA_WAITOK | BUS_DMA_NOWAIT),
        });
    }

    dmam._dm_cookie.set(mc.as_ptr().cast());
    dmar_dumpseg(tag, map_segs(dmam), "dmar_dmamap_create");
    Ok(dmam)
}

/// `dmar_dmamap_destroy`.
///
/// # Safety
///
/// As for `_bus_dmamap_destroy`: `dmam` came from [`dmar_dmamap_create`] and is not used
/// afterwards.
#[cfg(machine_x86)]
unsafe fn dmar_dmamap_destroy(tag: x86::BusDmaTagT, dmam: NonNull<x86::BusDmamap>) {
    // SAFETY: the caller's guarantee: a live map.
    let map = unsafe { dmam.as_ref() };

    dmar_dmamap_unload(tag, map);
    if let Some(mc) = NonNull::new(map._dm_cookie.get().cast::<DmarMapCookie>()) {
        // SAFETY: the cookie `dmar_dmamap_create` made; its descriptors are in no extent
        // now that the map is unloaded.
        let er = unsafe { mc.as_ref() }.dm_er;
        free(
            er.cast(),
            M_DEVBUF,
            map._dm_segcnt as usize * size_of::<ExtentRegion>(),
        );
        free(mc.cast(), M_DEVBUF, size_of::<DmarMapCookie>());
        map._dm_cookie.set(ptr::null_mut());
    }
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamap_destroy(tag, dmam) };
}

/// The common tail of the four dmar loads: maps the loaded segments into the domain, or
/// unloads the map again.
#[cfg(machine_x86)]
fn dmar_dmamap_load_common(
    tag: x86::BusDmaTagT,
    dmam: &x86::BusDmamap,
    flags: i32,
    fname: &str,
) -> Result<(), Errno> {
    let dom = tag_domain(tag);
    dmar_dumpseg(tag, map_segs(dmam), fname);
    if let Err(rc) = domain_load_map(dom, dmam, flags, PTE_R | PTE_W, fname) {
        x86::_bus_dmamap_unload(tag, dmam);
        return Err(rc);
    }
    dmar_dumpseg(tag, map_segs(dmam), fname);
    Ok(())
}

/// Clears the allocation constraints a load left in the first segment.
#[cfg(machine_x86)]
fn dmar_clear_constraints(dmam: &x86::BusDmamap) {
    // Clear stale allocation constraints
    if dmam.dm_nsegs.get() > 0 {
        let mut s = dmam.seg(0).get();
        s._ds_align = 0;
        s._ds_boundary = 0;
        dmam.seg(0).set(s);
    }
}

/// `dmar_dmamap_load`.
///
/// # Safety
///
/// As for `_bus_dmamap_load`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamap_load(
    tag: x86::BusDmaTagT,
    dmam: &x86::BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamap_load(tag, dmam, buf, buflen, p, flags) }?;
    dmar_clear_constraints(dmam);
    dmar_dmamap_load_common(tag, dmam, flags, "dmar_dmamap_load")
}

/// `dmar_dmamap_load_mbuf`.
///
/// # Safety
///
/// As for `_bus_dmamap_load_mbuf`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamap_load_mbuf(
    tag: x86::BusDmaTagT,
    dmam: &x86::BusDmamap,
    chain: &Mbuf,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamap_load_mbuf(tag, dmam, chain, flags) }?;
    dmar_clear_constraints(dmam);
    dmar_dmamap_load_common(tag, dmam, flags, "dmar_dmamap_load_mbuf")
}

/// `dmar_dmamap_load_uio`.
///
/// # Safety
///
/// As for `_bus_dmamap_load_uio`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamap_load_uio(
    tag: x86::BusDmaTagT,
    dmam: &x86::BusDmamap,
    uio: &Uio<'_>,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamap_load_uio(tag, dmam, uio, flags) }?;
    dmar_clear_constraints(dmam);
    dmar_dmamap_load_common(tag, dmam, flags, "dmar_dmamap_load_uio")
}

/// `dmar_dmamap_load_raw`.
///
/// # Safety
///
/// As for `_bus_dmamap_load_raw`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamap_load_raw(
    tag: x86::BusDmaTagT,
    dmam: &x86::BusDmamap,
    segs: &[x86::BusDmaSegment],
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamap_load_raw(tag, dmam, segs, size, flags) }?;
    // Preserve _ds_align/_ds_boundary allocation constraints
    if let Some(s0) = segs.first()
        && dmam.dm_nsegs.get() > 0
    {
        let mut s = dmam.seg(0).get();
        s._ds_align = s0._ds_align;
        s._ds_boundary = s0._ds_boundary;
        dmam.seg(0).set(s);
    }
    dmar_dmamap_load_common(tag, dmam, flags, "dmar_dmamap_load_raw")
}

/// `dmar_dmamap_unload`.
#[cfg(machine_x86)]
fn dmar_dmamap_unload(tag: x86::BusDmaTagT, dmam: &x86::BusDmamap) {
    let dom = tag_domain(tag);

    dmar_dumpseg(tag, map_segs(dmam), "dmar_dmamap_unload");
    domain_unload_map(dom, dmam);
    x86::_bus_dmamap_unload(tag, dmam);
}

/// `dmar_dmamem_alloc`.
#[cfg(machine_x86)]
fn dmar_dmamem_alloc(
    tag: x86::BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [x86::BusDmaSegment],
    flags: i32,
) -> Result<usize, Errno> {
    // Enforce device DMA mask in the IOVA allocator (domain_load_map)
    let rsegs =
        x86::_bus_dmamem_alloc(tag, size, alignment, boundary, segs, flags | BUS_DMA_64BIT)?;
    dmar_dumpseg(
        tag,
        segs[..rsegs].iter().map(|s| (s.ds_addr, s.ds_len)),
        "dmar_dmamem_alloc",
    );
    Ok(rsegs)
}

/// `dmar_dmamem_alloc_range`.
#[cfg(machine_x86)]
#[allow(clippy::too_many_arguments)] // the C's signature
fn dmar_dmamem_alloc_range(
    tag: x86::BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [x86::BusDmaSegment],
    flags: i32,
    low: BusAddr,
    high: BusAddr,
) -> Result<usize, Errno> {
    // Honor physical address range
    let rsegs =
        x86::_bus_dmamem_alloc_range(tag, size, alignment, boundary, segs, flags, low, high)?;
    dmar_dumpseg(
        tag,
        segs[..rsegs].iter().map(|s| (s.ds_addr, s.ds_len)),
        "dmar_dmamem_alloc_range",
    );
    Ok(rsegs)
}

/// `dmar_dmamem_free`.
///
/// # Safety
///
/// As for `_bus_dmamem_free`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamem_free(tag: x86::BusDmaTagT, segs: &[x86::BusDmaSegment]) {
    dmar_dumpseg(
        tag,
        segs.iter().map(|s| (s.ds_addr, s.ds_len)),
        "dmar_dmamem_free",
    );
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamem_free(tag, segs) };
}

/// `dmar_dmamem_map`.
#[cfg(machine_x86)]
fn dmar_dmamem_map(
    tag: x86::BusDmaTagT,
    segs: &mut [x86::BusDmaSegment],
    size: usize,
    flags: i32,
) -> Result<NonNull<u8>, Errno> {
    dmar_dumpseg(
        tag,
        segs.iter().map(|s| (s.ds_addr, s.ds_len)),
        "dmar_dmamem_map",
    );
    x86::_bus_dmamem_map(tag, segs, size, flags)
}

/// `dmar_dmamem_unmap`.
///
/// # Safety
///
/// As for `_bus_dmamem_unmap`.
#[cfg(machine_x86)]
unsafe fn dmar_dmamem_unmap(tag: x86::BusDmaTagT, kva: NonNull<u8>, size: usize) {
    let dom = tag_domain(tag);

    if debugme(dom) {
        kprintf!("dmamap_unmap: {}\n", dom_bdf(dom));
    }
    // SAFETY: the caller's guarantee.
    unsafe { x86::_bus_dmamem_unmap(tag, kva, size) };
}

/// `dmar_dmamem_mmap`.
#[cfg(machine_x86)]
fn dmar_dmamem_mmap(
    tag: x86::BusDmaTagT,
    segs: &[x86::BusDmaSegment],
    off: Off,
    prot: i32,
    flags: i32,
) -> Option<Paddr> {
    dmar_dumpseg(
        tag,
        segs.iter().map(|s| (s.ds_addr, s.ds_len)),
        "dmar_dmamem_mmap",
    );
    x86::_bus_dmamem_mmap(tag, segs, off, prot, flags)
}

/// `iommu_set_rtaddr(iommu, paddr)`: Intel: Set Context Root Address.
#[cfg(machine_x86)]
pub fn iommu_set_rtaddr(iommu: &IommuSoftc, paddr: u64) {
    mtx_enter(&iommu.reg_lock);
    iommu_write_8(iommu, DMAR_RTADDR_REG, paddr);
    iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get() | GCMD_SRTP);
    let mut i = 0;
    while i < 5 {
        let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
        if sts & GSTS_RTPS != 0 {
            break;
        }
        delay(10000);
        i += 1;
    }
    mtx_leave(&iommu.reg_lock);

    if i == 5 {
        kprintf!("iommu{}: set_rtaddr timeout\n", iommu.id.get());
    }
}

/// `iommu_alloc_hwdte(sc, size, paddr)`: Allocate contiguous memory for the Device Table
/// Entries; the virtual and physical addresses.
#[cfg(machine_x86)]
pub fn iommu_alloc_hwdte(sc: &AcpidmarSoftc, size: usize) -> Option<(NonNull<u8>, u64)> {
    let Some(dmat) = sc.sc_dmat.get() else {
        kprintf!("hwdte_create fails\n");
        return None;
    };

    let Ok(map) = x86::_bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT) else {
        kprintf!("hwdte_create fails\n");
        return None;
    };
    let mut seg = [x86::BusDmaSegment::default()];
    if x86::_bus_dmamem_alloc(dmat, size, 4, 0, &mut seg, BUS_DMA_NOWAIT | BUS_DMA_ZERO).is_err() {
        kprintf!("hwdte alloc fails\n");
        return None;
    }
    let Ok(vaddr) = x86::_bus_dmamem_map(dmat, &mut seg, size, BUS_DMA_NOWAIT | BUS_DMA_COHERENT)
    else {
        kprintf!("hwdte map fails\n");
        return None;
    };
    // SAFETY: `seg` is the memory just allocated and mapped, owned by this driver for ever.
    if unsafe { x86::_bus_dmamap_load_raw(dmat, map, &seg, size, BUS_DMA_NOWAIT) }.is_err() {
        kprintf!("hwdte load raw fails\n");
        return None;
    }
    Some((vaddr, map.seg(0).get().ds_addr as u64))
}

/// `iommu_alloc_page(iommu, paddr, nowait)`: COMMON: Allocate a new memory page; its
/// kernel virtual and physical addresses.
#[cfg(machine_x86)]
pub fn iommu_alloc_page(iommu: &IommuSoftc, nowait: bool) -> Option<(NonNull<u8>, u64)> {
    let kd = if nowait { &KD_TRYLOCK } else { &KD_WAITOK };
    let va = km_alloc(VTD_PAGE_SIZE, &KV_PAGE, &KP_ZERO, kd)?;
    let paddr = pmap_extract(pmap_kernel(), Vaddr::new(va.as_ptr() as usize))
        .map_or(0, |pa| pa.as_usize() as u64);

    iommu_flush_cache(iommu, va.as_ptr(), VTD_PAGE_SIZE);

    Some((va, paddr))
}

/// `iommu_free_page(va)`.
#[cfg(machine_x86)]
pub fn iommu_free_page(va: NonNull<u8>) {
    km_free(va, VTD_PAGE_SIZE, &KV_PAGE, &KP_ZERO);
}

/// `iommu_issue_qi(iommu, qi)`: Intel: Issue command via queued invalidation.
#[cfg(machine_x86)]
pub fn iommu_issue_qi(iommu: &IommuSoftc, qi: &QiEntry) {
    let q = iommu.qi.get();
    if q.is_null() {
        return;
    }
    if iommu.gcmd.get() & GCMD_QIE == 0 {
        return;
    }

    mtx_enter(&iommu.reg_lock);

    let tail = iommu.qi_tail.get();
    let next = (tail + 1) % IOMMU_QI_ENTRIES;
    let nextoff = next as u64 * size_of::<QiEntry>() as u64;

    // Avoid filling the queue: wait if next == head
    let mut h = iommu_read_8(iommu, DMAR_IQH_REG) & IOMMU_QI_MASK;
    if nextoff == h {
        for _ in 0..1000 {
            delay(10);
            h = iommu_read_8(iommu, DMAR_IQH_REG) & IOMMU_QI_MASK;
            if nextoff != h {
                break;
            }
        }
    }

    if nextoff == h {
        kprintf!("iommu{}: QI queue full\n", iommu.id.get());
        mtx_leave(&iommu.reg_lock);
        return;
    }

    // SAFETY: `qi` is the queue page of IOMMU_QI_ENTRIES entries and `tail` is below that;
    // the IOMMU reads the slot only once the tail register moves past it.
    let slot = unsafe { q.add(tail as usize) };
    // SAFETY: as above.
    unsafe { slot.write_volatile(*qi) };
    iommu_flush_cache(iommu, slot.cast_const().cast(), size_of::<QiEntry>());

    iommu.qi_tail.set(next);
    let t = nextoff;
    iommu_write_8(iommu, DMAR_IQT_REG, t);

    // Wait for the descriptor to be processed
    for _ in 0..1000 {
        h = iommu_read_8(iommu, DMAR_IQH_REG) & IOMMU_QI_MASK;
        if h == t {
            break;
        }
        delay(10);
    }
    if h != t {
        kprintf!(
            "iommu{}: QI timeout (h={:x} t={:x})\n",
            iommu.id.get(),
            h,
            t
        );
    }

    mtx_leave(&iommu.reg_lock);
}

/// `iommu_flush_tlb_qi(iommu, mode, did)`: Intel: Flush TLB entries, Queued Invalidation
/// mode.
#[cfg(machine_x86)]
pub fn iommu_flush_tlb_qi(iommu: &IommuSoftc, mode: i32, did: i32) {
    // Use queued invalidation
    let mut qi = QiEntry::default();
    match mode {
        IOTLB_GLOBAL => qi.lo = QI_IOTLB | QI_IOTLB_IG_GLOBAL,
        IOTLB_DOMAIN => qi.lo = QI_IOTLB | QI_IOTLB_IG_DOMAIN | qi_iotlb_did(did as u64),
        // Page-selective invalidation requires an address; use iommu_flush_tlb_page()
        IOTLB_PAGE => qi.lo = QI_IOTLB | QI_IOTLB_IG_DOMAIN | qi_iotlb_did(did as u64),
        _ => {}
    }
    if iommu.cap.get() & CAP_DRD != 0 {
        qi.lo |= QI_IOTLB_DR;
    }
    if iommu.cap.get() & CAP_DWD != 0 {
        qi.lo |= QI_IOTLB_DW;
    }
    iommu_issue_qi(iommu, &qi);
}

/// `iommu_flush_ctx_qi(iommu, mode, did, sid, fm)`: Intel: Flush Context entries, Queued
/// Invalidation mode.
#[cfg(machine_x86)]
pub fn iommu_flush_ctx_qi(iommu: &IommuSoftc, mode: i32, did: i32, sid: i32, fm: i32) {
    // Use queued invalidation
    let mut qi = QiEntry::default();
    match mode {
        CTX_GLOBAL => qi.lo = QI_CTX | QI_CTX_IG_GLOBAL,
        CTX_DOMAIN => qi.lo = QI_CTX | QI_CTX_IG_DOMAIN | qi_ctx_did(did as u64),
        CTX_DEVICE => {
            qi.lo = QI_CTX
                | QI_CTX_IG_DEVICE
                | qi_ctx_did(did as u64)
                | qi_ctx_sid(sid as u64)
                | qi_ctx_fm(fm as u64);
        }
        _ => {}
    }
    iommu_issue_qi(iommu, &qi);
}

/// `iommu_flush_write_buffer(iommu)`: Intel: Flush write buffers.
#[cfg(machine_x86)]
pub fn iommu_flush_write_buffer(iommu: &IommuSoftc) {
    if iommu.is_amd() {
        return;
    }
    if iommu.cap.get() & CAP_RWBF == 0 {
        return;
    }

    dprintf!(1, "writebuf\n");

    mtx_enter(&iommu.reg_lock);

    iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get() | GCMD_WBF);
    let mut i = 0;
    while i < 5 {
        let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
        if sts & GSTS_WBFS != 0 {
            break;
        }
        delay(10000);
        i += 1;
    }

    mtx_leave(&iommu.reg_lock);

    if i == 5 {
        kprintf!("write buffer flush fails\n");
    }
}

/// `iommu_flush_cache(iommu, addr, size)`: writes a table the IOMMU reads back to memory
/// when the IOMMU does not snoop the caches.
#[cfg(machine_x86)]
pub fn iommu_flush_cache(iommu: &IommuSoftc, addr: *const u8, size: usize) {
    if iommu.is_amd() {
        if iommu.flags.get() & IOMMU_FLAGS_COHERENT != 0 {
            return;
        }
        x86::pmap_flush_cache(Vaddr::new(addr as usize), Vsize::new(size));
        return;
    }
    if iommu.ecap.get() & ECAP_C == 0 {
        x86::pmap_flush_cache(Vaddr::new(addr as usize), Vsize::new(size));
    }
}

/// `iommu_flush_tlb_reg(iommu, mode, did)`: register-based IOTLB invalidation.
#[cfg(machine_x86)]
fn iommu_flush_tlb_reg(iommu: &IommuSoftc, mode: i32, did: i32) {
    let mut val = IOTLB_IVT;
    match mode {
        IOTLB_GLOBAL => val |= IIG_GLOBAL,
        IOTLB_DOMAIN => val |= IIG_DOMAIN | iotlb_did(did as u64),
        // Page-selective invalidation requires an address; use iommu_flush_tlb_page().
        IOTLB_PAGE => val |= IIG_DOMAIN | iotlb_did(did as u64),
        _ => {}
    }

    // Check for Read/Write Drain
    if iommu.cap.get() & CAP_DRD != 0 {
        val |= IOTLB_DR;
    }
    if iommu.cap.get() & CAP_DWD != 0 {
        val |= IOTLB_DW;
    }

    mtx_enter(&iommu.reg_lock);

    let reg = dmar_iotlb_reg(iommu.ecap.get());
    iommu_write_8(iommu, reg, val);
    for _ in 0..5 {
        val = iommu_read_8(iommu, reg);
        if val & IOTLB_IVT == 0 {
            break;
        }
        delay(10000);
    }
    if val & IOTLB_IVT != 0 {
        kprintf!("iommu{}: IOTLB invalidation timeout\n", iommu.id.get());
    }

    mtx_leave(&iommu.reg_lock);
}

/// `iommu_flush_tlb(iommu, mode, did)`: Flush IOMMU TLB entries globally or per-domain.
#[cfg(machine_x86)]
pub fn iommu_flush_tlb(iommu: &IommuSoftc, mode: i32, did: i32) {
    // Call AMD
    if iommu.is_amd() {
        let _ = ivhd_invalidate_domain(iommu, did);
        return;
    }
    if iommu.gcmd.get() & GCMD_QIE != 0 && !iommu.qi.get().is_null() {
        iommu_flush_tlb_qi(iommu, mode, did);
        return;
    }

    iommu_flush_tlb_reg(iommu, mode, did);
}

/// `iommu_flush_tlb_page(iommu, did, iova)`: Flush IOMMU TLB entries for a single IOVA
/// page.
#[cfg(machine_x86)]
pub fn iommu_flush_tlb_page(iommu: &IommuSoftc, did: i32, iova: BusAddr) {
    let iova = iova & !VTD_PAGE_MASK;
    if iommu.is_amd() {
        // AMD: page-selective invalidation via INVALIDATE_IOMMU_PAGES
        if ivhd_invalidate_page(iommu, did, iova).is_err() {
            let _ = ivhd_invalidate_domain(iommu, did);
        }
        return;
    }

    // Page-selective invalidation requires PSI capability
    if iommu.cap.get() & CAP_PSI == 0 {
        iommu_flush_tlb(iommu, IOTLB_DOMAIN, did);
        return;
    }

    if iommu.gcmd.get() & GCMD_QIE != 0 && !iommu.qi.get().is_null() {
        let mut qi = QiEntry {
            lo: QI_IOTLB | QI_IOTLB_IG_PAGE | qi_iotlb_did(did as u64),
            hi: 0,
        };
        if iommu.cap.get() & CAP_DRD != 0 {
            qi.lo |= QI_IOTLB_DR;
        }
        if iommu.cap.get() & CAP_DWD != 0 {
            qi.lo |= QI_IOTLB_DW;
        }

        // Invalidate exactly one 4K page. Low bits are reserved for the address mask
        // field. Keeping them zero requests a single-page invalidation.
        qi.hi = iova as u64;
        iommu_issue_qi(iommu, &qi);
        return;
    }

    let iva = iova as u64;

    let mut val = IOTLB_IVT | IIG_PAGE | iotlb_did(did as u64);
    if iommu.cap.get() & CAP_DRD != 0 {
        val |= IOTLB_DR;
    }
    if iommu.cap.get() & CAP_DWD != 0 {
        val |= IOTLB_DW;
    }

    mtx_enter(&iommu.reg_lock);

    // Program the invalidate address register (AM=0 => one page)
    iommu_write_8(iommu, dmar_iva_reg(iommu.ecap.get()), iva);

    let reg = dmar_iotlb_reg(iommu.ecap.get());
    iommu_write_8(iommu, reg, val);
    for _ in 0..5 {
        val = iommu_read_8(iommu, reg);
        if val & IOTLB_IVT == 0 {
            break;
        }
        delay(10000);
    }
    if val & IOTLB_IVT != 0 {
        kprintf!("iommu{}: IOTLB page invalidation timeout\n", iommu.id.get());
    }

    mtx_leave(&iommu.reg_lock);
}

/// `iommu_flush_tlb_segs(iommu, did, segs, nsegs)`: Intel: Flush IOTLB entries for a set
/// of IOVA ranges (AMD: the pages, or the whole domain past
/// `IOMMU_TLB_RANGE_MAX_PAGES_AMD`).
#[cfg(machine_x86)]
fn iommu_flush_tlb_segs<I>(iommu: &IommuSoftc, did: i32, segs: I)
where
    I: Iterator<Item = (BusAddr, BusSize)> + Clone,
{
    let mut max_am = cap_mamv(iommu.cap.get()) as i32;
    if max_am > 51 {
        max_am = 51;
    }
    let (npages, ndescs) = iommu_flush_tlb_counts(segs.clone(), max_am);
    if npages == 0 {
        return;
    }

    if iommu.is_amd() {
        if npages > IOMMU_TLB_RANGE_MAX_PAGES_AMD {
            let _ = ivhd_invalidate_domain(iommu, did);
            return;
        }
        ivhd_invalidate_segs(iommu, did, segs);
        return;
    }

    // No page-selective invalidation support
    if iommu.cap.get() & CAP_PSI == 0 {
        iommu_flush_tlb(iommu, IOTLB_DOMAIN, did);
        return;
    }

    // Estimate invalidation descriptor pressure using AM coalescing.
    //
    // If this range still requires too many descriptors, fall back.
    if ndescs > IOMMU_TLB_RANGE_MAX_DESCS_INTEL {
        iommu_flush_tlb(iommu, IOTLB_DOMAIN, did);
        return;
    }

    let q = iommu.qi.get();
    if iommu.gcmd.get() & GCMD_QIE != 0 && !q.is_null() {
        let mut qi = QiEntry {
            lo: QI_IOTLB | QI_IOTLB_IG_PAGE | qi_iotlb_did(did as u64),
            hi: 0,
        };
        if iommu.cap.get() & CAP_DRD != 0 {
            qi.lo |= QI_IOTLB_DR;
        }
        if iommu.cap.get() & CAP_DWD != 0 {
            qi.lo |= QI_IOTLB_DW;
        }

        // Batch queued invalidation descriptors
        mtx_enter(&iommu.reg_lock);

        let mut n = 0;
        let mut tail;
        loop {
            let h = iommu_read_8(iommu, DMAR_IQH_REG) & IOMMU_QI_MASK;
            let head = (h / size_of::<QiEntry>() as u64) as i32;
            tail = iommu.qi_tail.get();
            let nfree = if tail >= head {
                IOMMU_QI_ENTRIES - (tail - head) - 1
            } else {
                head - tail - 1
            };
            if nfree as u64 >= ndescs {
                break;
            }

            mtx_leave(&iommu.reg_lock);
            if n >= 1000 {
                kprintf!("iommu{}: QI queue full (range flush)\n", iommu.id.get());
                // Bypass QI to avoid recursion on a saturated ring
                iommu_flush_tlb_reg(iommu, IOTLB_DOMAIN, did);
                return;
            }
            delay(10);
            mtx_enter(&iommu.reg_lock);
            n += 1;
        }

        for (start, end) in seg_ranges(segs) {
            let mut addr = start;
            while addr < end {
                let am = iommu_intel_pick_am(addr, end, max_am);

                // The low bits of the address field hold the Address Mask (AM) value. AM=0
                // invalidates one 4KB page. AM>0 invalidates a power-of-two number of pages
                // for a size-aligned range.
                qi.hi = addr as u64 | am as u64;
                // SAFETY: `tail` is a slot of the queue page, free per the head check above.
                let slot = unsafe { q.add(tail as usize) };
                // SAFETY: as above.
                unsafe { slot.write_volatile(qi) };
                iommu_flush_cache(iommu, slot.cast_const().cast(), size_of::<QiEntry>());
                tail = (tail + 1) % IOMMU_QI_ENTRIES;

                addr += VTD_PAGE_SIZE << am;
            }
        }

        iommu.qi_tail.set(tail);
        let t = tail as u64 * size_of::<QiEntry>() as u64;
        iommu_write_8(iommu, DMAR_IQT_REG, t);

        // Wait for all queued invalidations to be processed
        let mut h = 0;
        for _ in 0..1000 {
            h = iommu_read_8(iommu, DMAR_IQH_REG) & IOMMU_QI_MASK;
            if h == t {
                break;
            }
            delay(10);
        }
        if h != t {
            kprintf!(
                "iommu{}: QI timeout (range h={:x} t={:x})\n",
                iommu.id.get(),
                h,
                t
            );
        }

        mtx_leave(&iommu.reg_lock);
        return;
    }

    // Register-based PSI
    let mut cmd = IOTLB_IVT | IIG_PAGE | iotlb_did(did as u64);
    if iommu.cap.get() & CAP_DRD != 0 {
        cmd |= IOTLB_DR;
    }
    if iommu.cap.get() & CAP_DWD != 0 {
        cmd |= IOTLB_DW;
    }

    mtx_enter(&iommu.reg_lock);

    let ivareg = dmar_iva_reg(iommu.ecap.get());
    let iotlbreg = dmar_iotlb_reg(iommu.ecap.get());
    for (start, end) in seg_ranges(segs) {
        let mut addr = start;
        while addr < end {
            let am = iommu_intel_pick_am(addr, end, max_am);
            let iva = addr as u64 | am as u64;

            iommu_write_8(iommu, ivareg, iva);
            iommu_write_8(iommu, iotlbreg, cmd);
            let mut sts = 0;
            for _ in 0..5 {
                sts = iommu_read_8(iommu, iotlbreg);
                if sts & IOTLB_IVT == 0 {
                    break;
                }
                delay(10000);
            }
            if sts & IOTLB_IVT != 0 {
                kprintf!("iommu{}: IOTLB page invalidation timeout\n", iommu.id.get());
            }

            addr += VTD_PAGE_SIZE << am;
        }
    }

    mtx_leave(&iommu.reg_lock);
}

/// `iommu_flush_ctx(iommu, mode, did, sid, fm)`: Intel: Flush IOMMU settings. Flushes can
/// occur globally, per domain, or per device.
#[cfg(machine_x86)]
pub fn iommu_flush_ctx(iommu: &IommuSoftc, mode: i32, did: i32, sid: i32, fm: i32) {
    if iommu.is_amd() {
        return;
    }
    if iommu.gcmd.get() & GCMD_QIE != 0 && !iommu.qi.get().is_null() {
        iommu_flush_ctx_qi(iommu, mode, did, sid, fm);
        return;
    }

    let mut val = CCMD_ICC;
    match mode {
        CTX_GLOBAL => val |= CIG_GLOBAL,
        CTX_DOMAIN => val |= CIG_DOMAIN | ccmd_did(did as u64),
        CTX_DEVICE => {
            val |= CIG_DEVICE | ccmd_did(did as u64) | ccmd_sid(sid as u64) | ccmd_fm(fm as u64);
        }
        _ => {}
    }

    mtx_enter(&iommu.reg_lock);

    iommu_write_8(iommu, DMAR_CCMD_REG, val);
    for _ in 0..5 {
        val = iommu_read_8(iommu, DMAR_CCMD_REG);
        if val & CCMD_ICC == 0 {
            break;
        }
        delay(10000);
    }
    if val & CCMD_ICC != 0 {
        kprintf!("iommu{}: CCMD invalidation timeout\n", iommu.id.get());
    }

    mtx_leave(&iommu.reg_lock);
}

/// `iommu_enable_qi(iommu, enable)`: Intel: Enable Queued Invalidation.
#[cfg(machine_x86)]
pub fn iommu_enable_qi(iommu: &IommuSoftc, enable: bool) {
    if iommu.is_amd() {
        return;
    }
    if iommu.ecap.get() & ECAP_QI == 0 {
        return;
    }

    let mut sts = 0;
    if enable {
        if iommu.qi.get().is_null() {
            kprintf!("iommu{}: QI requested but no queue\n", iommu.id.get());
            return;
        }

        iommu.gcmd.set(iommu.gcmd.get() | GCMD_QIE);

        mtx_enter(&iommu.reg_lock);

        iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
        for _ in 0..100 {
            sts = iommu_read_4(iommu, DMAR_GSTS_REG);
            if sts & GSTS_QIES != 0 {
                break;
            }
            delay(1000);
        }

        mtx_leave(&iommu.reg_lock);

        if sts & GSTS_QIES == 0 {
            kprintf!("iommu{}: enable QI timeout\n", iommu.id.get());
            // Fall back to register invalidation
            iommu.gcmd.set(iommu.gcmd.get() & !GCMD_QIE);
            mtx_enter(&iommu.reg_lock);
            iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
            mtx_leave(&iommu.reg_lock);
        }
    } else {
        iommu.gcmd.set(iommu.gcmd.get() & !GCMD_QIE);

        mtx_enter(&iommu.reg_lock);

        iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
        for _ in 0..100 {
            sts = iommu_read_4(iommu, DMAR_GSTS_REG);
            if sts & GSTS_QIES == 0 {
                break;
            }
            delay(1000);
        }

        mtx_leave(&iommu.reg_lock);

        if sts & GSTS_QIES != 0 {
            kprintf!("iommu{}: disable QI timeout\n", iommu.id.get());
        }
    }
}

/// `iommu_enable_translation(iommu, enable)`: Intel: Enable IOMMU translation.
#[cfg(machine_x86)]
pub fn iommu_enable_translation(iommu: &IommuSoftc, enable: bool) -> Result<(), Errno> {
    let mut n = 0;

    if iommu.is_amd() {
        return Ok(());
    }
    if enable {
        dprintf!(1, "iommu{}: enable translation\n", iommu.id.get());
        if IOMMU_DEBUG && ACPIDMAR_DBG_LVL.load(Ordering::Relaxed) >= 2 {
            iommu_showcfg(iommu, -1);
        }

        iommu.gcmd.set(iommu.gcmd.get() | GCMD_TE);

        // Enable translation
        mtx_enter(&iommu.reg_lock);
        iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
        loop {
            let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
            delay(10000);
            let more = n < 5 && sts & GSTS_TES == 0;
            n += 1;
            if !more {
                break;
            }
        }
        mtx_leave(&iommu.reg_lock);
        dprintf!(
            2,
            "iommu{}: translation enable loops={}\n",
            iommu.id.get(),
            n
        );

        if n >= 5 {
            kprintf!("error.. unable to initialize iommu {}\n", iommu.id.get());
            iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);

            // Disable IOMMU
            iommu.gcmd.set(iommu.gcmd.get() & !GCMD_TE);
            mtx_enter(&iommu.reg_lock);
            iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
            mtx_leave(&iommu.reg_lock);

            return Err(Errno::EIO);
        }

        iommu_flush_ctx(iommu, CTX_GLOBAL, 0, 0, 0);
        iommu_flush_tlb(iommu, IOTLB_GLOBAL, 0);
    } else {
        iommu.gcmd.set(iommu.gcmd.get() & !GCMD_TE);

        mtx_enter(&iommu.reg_lock);

        iommu_write_4(iommu, DMAR_GCMD_REG, iommu.gcmd.get());
        loop {
            let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
            delay(10000);
            let more = n < 5 && sts & GSTS_TES != 0;
            n += 1;
            if !more {
                break;
            }
        }
        mtx_leave(&iommu.reg_lock);
        dprintf!(
            2,
            "iommu{}: translation disable loops={}\n",
            iommu.id.get(),
            n
        );
    }

    Ok(())
}

/// The `cap`/`ecap` bit names `iommu_init` prints (DPRINTF): `s` when `m` is set in `v`.
#[cfg(machine_x86)]
const fn capname(v: u64, m: u64, s: &'static str) -> &'static str {
    if v & m != 0 { s } else { "" }
}

/// `iommu_init(sc, iommu, dh)`: Intel: Initialize IOMMU.
#[cfg(machine_x86)]
pub fn iommu_init(
    sc: &AcpidmarSoftc,
    iommu: &'static IommuSoftc,
    dh: &AcpidmarDrhd,
) -> Result<(), Errno> {
    let len = VTD_PAGE_SIZE;
    let address = { dh.address };

    let Some(memt) = sc.sc_memt.get() else {
        return Err(Errno::ENXIO);
    };
    // SAFETY: the DRHD's register page, which the firmware gives to this unit.
    let ioh = unsafe { bus_space_map(memt, address as BusAddr, len, 0) }?;
    iommu.ioh.set(Some(ioh));

    iommu.domains.init();
    iommu
        .id
        .set(NIOMMU_INTEL.fetch_add(1, Ordering::Relaxed) + 1);
    iommu.flags.set(i32::from(dh.flags));
    iommu.segment.set(i32::from(dh.segment));
    iommu.iot.set(Some(memt));

    iommu.cap.set(iommu_read_8(iommu, DMAR_CAP_REG));
    iommu.ecap.set(iommu_read_8(iommu, DMAR_ECAP_REG));
    iommu.ndoms.set(cap_nd(iommu.cap.get()));

    // Print Capabilities & Extended Capabilities
    let cap = iommu.cap.get();
    let ecap = iommu.ecap.get();
    dprintf!(
        0,
        "  caps: {}{}{}{}{}{}{}{}{}{}{}\n",
        capname(cap, CAP_AFL, "afl "),   // adv fault
        capname(cap, CAP_RWBF, "rwbf "), // write-buffer flush
        capname(cap, CAP_PLMR, "plmr "), // protected lo region
        capname(cap, CAP_PHMR, "phmr "), // protected hi region
        capname(cap, CAP_CM, "cm "),     // caching mode
        capname(cap, CAP_ZLR, "zlr "),   // zero-length read
        capname(cap, CAP_PSI, "psi "),   // page invalidate
        capname(cap, CAP_DWD, "dwd "),   // write drain
        capname(cap, CAP_DRD, "drd "),   // read drain
        capname(cap, CAP_FL1GP, "Gb "),  // 1Gb pages
        capname(cap, CAP_PI, "pi ")      // posted interrupts
    );
    dprintf!(
        0,
        "  ecap: {}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}\n",
        capname(ecap, ECAP_C, "c "),       // coherent
        capname(ecap, ECAP_QI, "qi "),     // queued invalidate
        capname(ecap, ECAP_DT, "dt "),     // device iotlb
        capname(ecap, ECAP_IR, "ir "),     // intr remap
        capname(ecap, ECAP_EIM, "eim "),   // x2apic
        capname(ecap, ECAP_PT, "pt "),     // passthrough
        capname(ecap, ECAP_SC, "sc "),     // snoop control
        capname(ecap, ECAP_ECS, "ecs "),   // extended context
        capname(ecap, ECAP_MTS, "mts "),   // memory type
        capname(ecap, ECAP_NEST, "nest "), // nested translations
        capname(ecap, ECAP_DIS, "dis "),   // deferred invalidation
        capname(ecap, ECAP_PASID, "pas "), // pasid
        capname(ecap, ECAP_PRS, "prs "),   // page request
        capname(ecap, ECAP_ERS, "ers "),   // execute request
        capname(ecap, ECAP_SRS, "srs "),   // supervisor request
        capname(ecap, ECAP_NWFS, "nwfs "), // no write flag
        capname(ecap, ECAP_EAFS, "eafs ")  // extended accessed flag
    );

    mtx_init(&iommu.reg_lock, IPL_HIGH);

    // Clear Interrupt Masking
    iommu_write_4(iommu, DMAR_FSTS_REG, FSTS_PFO | FSTS_PPF);

    iommu.intr.set(acpidmar_intr_establish(
        iommu,
        IPL_HIGH,
        acpidmar_intr,
        ptr::from_ref(iommu).cast_mut().cast(),
        "dmarintr",
    ));

    // Enable interrupts
    let sts = iommu_read_4(iommu, DMAR_FECTL_REG);
    iommu_write_4(iommu, DMAR_FECTL_REG, sts & !FECTL_IM);

    // Allocate root pointer
    let Some((root, paddr)) = iommu_alloc_page(iommu, false) else {
        kprintf!("iommu{}: can't allocate root pointer\n", iommu.id.get());
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(Errno::ENOMEM);
    };
    iommu.root.set(root.as_ptr().cast_const().cast());
    dprintf!(
        0,
        "Allocated root pointer: pa:{:016x} va:{:p}\n",
        paddr,
        root.as_ptr()
    );
    iommu.rtaddr.set(paddr);
    iommu_flush_write_buffer(iommu);
    iommu_set_rtaddr(iommu, paddr);

    if iommu.ecap.get() & ECAP_QI != 0 {
        // Queued Invalidation support
        match iommu_alloc_page(iommu, false) {
            None => {
                kprintf!("iommu{}: can't allocate QI queue\n", iommu.id.get());
            }
            Some((qi, qip)) => {
                iommu.qi.set(qi.as_ptr().cast());
                iommu.qip.set(qip);
                iommu.qi_head.set(0);
                iommu.qi_tail.set(0);
                iommu_write_8(iommu, DMAR_IQT_REG, 0);
                iommu_write_8(iommu, DMAR_IQA_REG, (qip & !0xfff) | IQA_QS_256);
                iommu_flush_write_buffer(iommu);
                iommu_enable_qi(iommu, true);
            }
        }
    }
    // #if 0: interrupt remapping support (ECAP_IR): DMAR_IRTA_REG = 0.

    // Calculate guest address width and supported guest widths
    iommu.mgaw.set(cap_mgaw(iommu.cap.get()) as i32);
    dprintf!(0, "gaw: {} {{ ", iommu.mgaw.get());
    for i in 0..5 {
        if cap_sagaw(iommu.cap.get()) & (1 << i) != 0 {
            let gaw = vtd_leveltoaw(i);
            dprintf!(0, "{} ", gaw);
            iommu.agaw.set(gaw);
        }
    }
    dprintf!(0, "}}\n");

    // Cache current status register bits
    let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
    if sts & GSTS_TES != 0 {
        iommu.gcmd.set(iommu.gcmd.get() | GCMD_TE);
    }
    if sts & GSTS_QIES != 0 {
        iommu.gcmd.set(iommu.gcmd.get() | GCMD_QIE);
    }
    if sts & GSTS_IRES != 0 {
        iommu.gcmd.set(iommu.gcmd.get() | GCMD_IRE);
    }
    dprintf!(0, "gcmd: {:x} preset\n", iommu.gcmd.get());
    acpidmar_intr(ptr::from_ref(iommu).cast_mut().cast());
    Ok(())
}

/// The unit's register window, once mapped.
#[cfg(machine_x86)]
fn iommu_regs(iommu: &IommuSoftc) -> Option<(BusSpaceTag, BusSpaceHandle)> {
    Some((iommu.iot.get()?, iommu.ioh.get()?))
}

/// `iommu_read_4(iommu, reg)`: Read/Write IOMMU register.
#[cfg(machine_x86)]
pub fn iommu_read_4(iommu: &IommuSoftc, reg: usize) -> u32 {
    match iommu_regs(iommu) {
        Some((t, h)) => bus_space_read_4(t, h, reg),
        None => 0,
    }
}

/// `iommu_write_4(iommu, reg, v)`.
#[cfg(machine_x86)]
pub fn iommu_write_4(iommu: &IommuSoftc, reg: usize, v: u32) {
    if let Some((t, h)) = iommu_regs(iommu) {
        bus_space_write_4(t, h, reg, v);
    }
}

/// `iommu_read_8(iommu, reg)`.
#[cfg(machine_x86)]
pub fn iommu_read_8(iommu: &IommuSoftc, reg: usize) -> u64 {
    match iommu_regs(iommu) {
        Some((t, h)) => x86::bus_space_read_8(t, h, reg),
        None => 0,
    }
}

/// `iommu_write_8(iommu, reg, v)`.
#[cfg(machine_x86)]
pub fn iommu_write_8(iommu: &IommuSoftc, reg: usize, v: u64) {
    if let Some((t, h)) = iommu_regs(iommu) {
        x86::bus_space_write_8(t, h, reg, v);
    }
}

/// `acpidmar_match_devscope(devlist, pc, sid)`: Check if a device is within a device scope:
/// `DMAR_ENDPOINT` for the device itself, `DMAR_BRIDGE` below a bridge of the scope, 0.
#[cfg(machine_x86)]
pub fn acpidmar_match_devscope(
    devlist: &TailqHead<DevlistHead>,
    pc: PciChipsetTag,
    sid: i32,
) -> i32 {
    let sbus = sid_bus(sid);
    for ds in devlist.iter() {
        let Some(p0) = ds.dp.first() else {
            continue;
        };
        let mut bus = ds.bus;
        let mut dev = i32::from(p0.device);
        let mut fun = i32::from(p0.function);
        // Walk PCI bridges in path
        for p in ds.dp.iter().skip(1) {
            let tag = pci_make_tag(pc, bus, dev, fun);
            let reg = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
            bus = ppb_businfo_secondary(reg) as i32;
            dev = i32::from(p.device);
            fun = i32::from(p.function);
        }

        // Check for device exact match
        if sid == mksid(bus, dev, fun) {
            return i32::from(DMAR_ENDPOINT);
        }

        // Check for device subtree match
        if ds.r#type == i32::from(DMAR_BRIDGE) {
            let tag = pci_make_tag(pc, bus, dev, fun);
            let reg = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
            let sec = ppb_businfo_secondary(reg) as i32;
            let sub = ppb_businfo_subordinate(reg) as i32;
            if sec <= sbus && sbus <= sub {
                return i32::from(DMAR_BRIDGE);
            }
        }
    }

    0
}

/// `domain_create(iommu, did)`: a new domain with its top page table, `bus_dma` tag and
/// I/O address extent, the first megabyte reserved and the MSI window mapped 1:1.
#[cfg(machine_x86)]
pub fn domain_create(iommu: &'static IommuSoftc, did: i32) -> Option<&'static Domain> {
    dprintf!(0, "iommu{}: create domain: {:04x}\n", iommu.id.get(), did);
    let (pte, ptep) = iommu_alloc_page(iommu, false)?;

    let mut name = [0u8; 32];
    let n = snprintf(
        &mut name,
        format_args!("did:{:x}.{:04x}", iommu.id.get(), did),
    )
    .min(name.len() - 1);
    let name: &'static mut [u8; 32] = Box::leak(Box::new(name));
    let exname: &'static [u8] = &name[..n];

    // Setup IOMMU address map
    let gaw = iommu.agaw.get().min(iommu.mgaw.get());
    let Some(iovamap) = extent_create(
        exname,
        0,
        (1u64 << gaw) - 1,
        M_DEVBUF,
        None,
        EX_WAITOK | EX_NOCOALESCE,
    ) else {
        iommu_free_page(pte);
        return None;
    };

    let dom = Box::new(Domain {
        iommu,
        did,
        gaw: 0,
        pte: pte.cast(),
        ptep,
        // Setup DMA
        dmat: x86::BusDmaTag {
            _cookie: ptr::null_mut(),
            _dmamap_create: dmar_dmamap_create,           // nop
            _dmamap_destroy: dmar_dmamap_destroy,         // um
            _dmamap_load: dmar_dmamap_load,               // lm
            _dmamap_load_mbuf: dmar_dmamap_load_mbuf,     // lm
            _dmamap_load_uio: dmar_dmamap_load_uio,       // lm
            _dmamap_load_raw: dmar_dmamap_load_raw,       // lm
            _dmamap_unload: dmar_dmamap_unload,           // um
            _dmamap_sync: x86::_bus_dmamap_sync,          // nop
            _dmamem_alloc: dmar_dmamem_alloc,             // nop
            _dmamem_alloc_range: dmar_dmamem_alloc_range, // nop
            _dmamem_free: dmar_dmamem_free,               // nop
            _dmamem_map: dmar_dmamem_map,                 // nop
            _dmamem_unmap: dmar_dmamem_unmap,             // nop
            _dmamem_mmap: dmar_dmamem_mmap,
        },
        flag: Cell::new(0),
        ptlck: Mutex::new(IPL_HIGH),
        exlck: Mutex::new(IPL_HIGH),
        exname,
        iovamap,
        devices: TailqHead::new(),
        link: TailqEntry::new(),
    });
    let p = Box::into_raw(dom);
    // SAFETY: `p` is the domain just allocated, not shared yet: the tag's cookie is the
    // domain, as the C's `dom->dmat._cookie = dom`.
    unsafe { (*p).dmat._cookie = p.cast() };
    // SAFETY: as above; from here on the domain is only reached through shared references.
    let dom: &'static Domain = unsafe { &*p };

    // Some hardware, e.g. qwx(4), can't do DMA to low addresses. In addition, PCI-PCI
    // bridges may block forwarding VGA memory addresses from the secondary to the primary
    // interface. Take the easy way out and reserve the first 1MB of the address space.
    // This also means we'll catch bugs where drivers inadvertedly do DMA to/from address
    // zero.
    let _ = extent_alloc_region(dom.iovamap, RESERVED_ADDRESS, RESERVED_SIZE, EX_WAITOK);

    // Reserve MSI address window from IOVA allocations
    let _ = extent_alloc_region(dom.iovamap, MSI_BASE_ADDRESS, MSI_BASE_SIZE, EX_WAITOK);
    mtx_init(&dom.ptlck, IPL_HIGH);
    mtx_init(&dom.exlck, IPL_HIGH);

    // Identity-map the MSI window
    if domain_map_pthru(dom, MSI_BASE_ADDRESS, MSI_BASE_ADDRESS + MSI_BASE_SIZE).is_err() {
        kprintf!(
            "iommu{}: domain {:04x}: failed to map MSI window\n",
            iommu.id.get(),
            dom.did
        );
        // SAFETY: the extent is the domain's alone and is not used again.
        unsafe { extent_destroy(dom.iovamap) };
        iommu_free_page(dom.pte.cast());
        // SAFETY: the domain is in no list and its tag was handed to nobody; nothing uses
        // it after this.
        drop(unsafe { Box::from_raw(p) });
        return None;
    }

    // SAFETY: a leaked domain, in no domain list, never freed from now on.
    unsafe { iommu.domains.insert_tail(dom) };

    Some(dom)
}

/// `domain_add_device(dom, sid)`: adds the device to the domain and maps its unity and
/// exclusion ranges (IVMD) 1:1.
#[cfg(machine_x86)]
pub fn domain_add_device(dom: &Domain, sid: i32) {
    dprintf!(
        0,
        "add {} to iommu{}.{:04x}\n",
        dmar_bdf(sid),
        dom.iommu.id.get(),
        dom.did
    );
    let ddev = Box::leak(Box::new(DomainDev {
        sid,
        sec: 0,
        sub: 0,
        link: TailqEntry::new(),
    }));
    // SAFETY: a fresh element; freed only by `domain_remove_device` after its removal.
    unsafe { dom.devices.insert_tail(ddev) };

    let Some(sc) = acpidmar_sc() else {
        return;
    };
    for ivmd in sc.sc_ivmds.iter() {
        if sid < i32::from(ivmd.start_id) || sid > i32::from(ivmd.end_id) {
            continue;
        }
        if ivmd.flags & (IVMD_EXCLRANGE | IVMD_UNITY) == 0 {
            continue;
        }
        let _ = extent_alloc_region(dom.iovamap, ivmd.addr, ivmd.size, EX_WAITOK | EX_CONFLICTOK);
        // XXX use correct R/W permission for IVMD_UNITY
        let _ = domain_map_pthru(dom, ivmd.addr, ivmd.addr + ivmd.size);
    }

    // Should set context entry here??
}

/// `domain_remove_device(dom, sid)`.
#[cfg(machine_x86)]
pub fn domain_remove_device(dom: &Domain, sid: i32) {
    for ddev in dom.devices.iter() {
        if ddev.sid == sid {
            // SAFETY: `ddev` is in this list; `domain_add_device` leaked it and it is freed
            // here, once, after its removal (the iterator already read the next element).
            unsafe {
                dom.devices.remove(ddev);
                drop(Box::from_raw(ptr::from_ref(ddev).cast_mut()));
            }
        }
    }
}

/// `domain_lookup(sc, segment, sid)`: Lookup domain by segment & source id
/// (bus.device.function); creates one, or puts the device in the catch-all domain, when the
/// device has none yet.
#[cfg(machine_x86)]
pub fn domain_lookup(
    sc: Option<&AcpidmarSoftc>,
    segment: i32,
    sid: i32,
) -> Option<&'static Domain> {
    let sc = sc?;

    // Lookup IOMMU for this device
    let mut found = None;
    for iommu in sc.sc_drhds.iter() {
        if iommu.segment.get() != segment {
            continue;
        }
        // Check for devscope match or catchall iommu
        let rc = acpidmar_match_devscope(&iommu.devices, sc.sc_pc.get(), sid);
        if rc != 0 || iommu.flags.get() & IOMMU_FLAGS_CATCHALL != 0 {
            found = Some(iommu);
            break;
        }
    }
    let Some(iommu) = found else {
        kprintf!("{}: no iommu found\n", dmar_bdf(sid));
        return None;
    };
    // SAFETY: the units in `sc_drhds` are leaked and never freed.
    let iommu: &'static IommuSoftc = unsafe { &*ptr::from_ref(iommu) };

    // Search domain devices
    for dom in iommu.domains.iter() {
        for ddev in dom.devices.iter() {
            // XXX: match all functions?
            if ddev.sid == sid {
                // SAFETY: domains are leaked and never freed once in the list.
                return Some(unsafe { &*ptr::from_ref(dom) });
            }
        }
    }
    let dom = if iommu.ndoms.get() <= 2 {
        // Running out of domains.. create catchall domain
        if iommu.unity.get().is_none() {
            iommu.unity.set(domain_create(iommu, 1));
        }
        iommu.unity.get()
    } else {
        let did = iommu.ndoms.get() - 1;
        iommu.ndoms.set(did);
        domain_create(iommu, did)
    };
    let Some(dom) = dom else {
        kprintf!("no domain here\n");
        return None;
    };

    // Add device to domain
    domain_add_device(dom, sid);

    Some(dom)
}

/// `acpidmar_ppbwin_topmost(sc, segment, bus)`: the outermost bridge whose bus range holds
/// `bus`.
#[cfg(machine_x86)]
fn acpidmar_ppbwin_topmost(sc: &AcpidmarSoftc, segment: i32, bus: i32) -> Option<&PpbwinEntry> {
    let mut best: Option<&PpbwinEntry> = None;

    for pw in sc.sc_ppbwins.iter() {
        if pw.segment != segment {
            continue;
        }
        let (sec, sub) = (i32::from(pw.sec.get()), i32::from(pw.sub.get()));
        if bus < sec || bus > sub {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => {
                sec < i32::from(b.sec.get())
                    || (sec == i32::from(b.sec.get()) && sub > i32::from(b.sub.get()))
            }
        };
        if better {
            best = Some(pw);
        }
    }

    best
}

/// `acpidmar_ppbwin_lookup(sc, segment, sid)`.
#[cfg(machine_x86)]
pub fn acpidmar_ppbwin_lookup(sc: &AcpidmarSoftc, segment: i32, sid: u16) -> Option<&PpbwinEntry> {
    sc.sc_ppbwins
        .iter()
        .find(|pw| pw.segment == segment && pw.sid == sid)
}

/// `acpidmar_ppbwin_record(sc, pc, pa, segment, sid)`: Record PCI-PCI bridge forwarding
/// windows keyed by ACPI segment. IOMMU domains for devices in segment can then exclude
/// those windows from DVA allocation.
#[cfg(machine_x86)]
pub fn acpidmar_ppbwin_record(
    sc: &AcpidmarSoftc,
    pc: PciChipsetTag,
    pa: &PciAttachArgs,
    segment: i32,
    sid: u16,
) {
    let blr = pci_conf_read(pc, pa.pa_tag, PPB_REG_BUSINFO);
    let sec = ppb_businfo_secondary(blr) as u8;
    let sub = ppb_businfo_subordinate(blr) as u8;
    if sub < sec || sub == 0 {
        return;
    }

    let pw = match acpidmar_ppbwin_lookup(sc, segment, sid) {
        Some(pw) => pw,
        None => {
            let pw = Box::leak(Box::new(PpbwinEntry {
                link: TailqEntry::new(),
                segment,
                sid,
                sec: Cell::new(0),
                sub: Cell::new(0),
                mem_base: Cell::new(0),
                mem_size: Cell::new(0),
                pmem_base: Cell::new(0),
                pmem_size: Cell::new(0),
            }));
            // SAFETY: a fresh element, leaked, never removed.
            unsafe { sc.sc_ppbwins.insert_tail(pw) };
            pw
        }
    };
    pw.sec.set(sec);
    pw.sub.set(sub);

    // Non-prefetchable memory window
    let blr = pci_conf_read(pc, pa.pa_tag, PPB_REG_MEM);
    let base = u64::from((blr & 0x0000_fff0) << 16);
    let limit = u64::from((blr & 0xfff0_0000) | 0x000f_ffff);
    let size = if limit > base { limit - base + 1 } else { 0 };
    pw.mem_base.set(base);
    pw.mem_size.set(size);

    // Prefetchable memory window
    let blr = pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFMEM);
    let mut base = u64::from((blr & 0x0000_fff0) << 16);
    let mut limit = u64::from((blr & 0xfff0_0000) | 0x000f_ffff);
    // __LP64__: Only include the high 32-bit registers when the bridge indicates a 64-bit
    // prefetchable window.
    if blr & 0x000f_000f == 0x0001_0001 {
        let hi = pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFBASE_HI32);
        base |= u64::from(hi) << 32;
        let hi = pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFLIM_HI32);
        limit |= u64::from(hi) << 32;
    }
    let size = if limit > base { limit - base + 1 } else { 0 };
    pw.pmem_base.set(base);
    pw.pmem_size.set(size);
}

/// `acpidmar_ppbwin_reserve(sc, dom, segment, bus)`: keeps the windows of the bridge above
/// `bus` out of the domain's I/O addresses.
#[cfg(machine_x86)]
pub fn acpidmar_ppbwin_reserve(sc: &AcpidmarSoftc, dom: &Domain, segment: i32, bus: i32) {
    let Some(pw) = acpidmar_ppbwin_topmost(sc, segment, bus) else {
        return;
    };

    // Reserve non-prefetchable bridge window
    if pw.mem_size.get() != 0 {
        let _ = extent_alloc_region(
            dom.iovamap,
            pw.mem_base.get(),
            pw.mem_size.get(),
            EX_WAITOK | EX_CONFLICTOK,
        );
    }

    // Reserve prefetchable bridge window
    if pw.pmem_size.get() != 0 {
        let _ = extent_alloc_region(
            dom.iovamap,
            pw.pmem_base.get(),
            pw.pmem_size.get(),
            EX_WAITOK | EX_CONFLICTOK,
        );
    }
}

/// `_iommu_map(dom, va, gpa, len)`: Map Guest Pages into IOMMU (vmm(4)): the current
/// process's pages at `va` to the I/O addresses at `gpa`.
#[cfg(machine_x86)]
pub fn _iommu_map(d: Option<&Domain>, va: Vaddr, gpa: BusAddr, len: BusSize) {
    let Some(d) = d else {
        return;
    };
    let iommu = d.iommu;
    if iommu.flags.get() & IOMMU_FLAGS_BAD != 0 {
        return;
    }
    let Some(p) = curproc() else {
        return;
    };

    // Ensure all devices in this domain have context/DTE programmed
    domain_map_check(d);

    let start = gpa;
    let (mut gpa, mut va) = (gpa, va.as_usize());
    dprintf!(1, "Mapping dma: {:x} = {:x}/{:x}\n", va, gpa, len);
    let mut i = 0;
    while i < len {
        let Some(hpa) = pmap_extract(p.vmspace().vm_map.pmap(), Vaddr::new(va)) else {
            break;
        };
        if domain_map_page(
            d,
            gpa as u64,
            hpa.as_usize() as u64,
            PTE_P | PTE_R | PTE_W,
            false,
        )
        .is_err()
        {
            break;
        }
        gpa += PAGE_SIZE;
        va += PAGE_SIZE;
        i += PAGE_SIZE;
    }

    if i != 0 && iommu_enabled(iommu) {
        iommu_flush_write_buffer(iommu);
        iommu_flush_tlb_segs(iommu, d.did, core::iter::once((start, i)));
    }
}

/// `_iommu_domain(segment, bus, dev, func, id)`: Find IOMMU for a given PCI device; `id`
/// gets its domain id.
#[cfg(machine_x86)]
pub fn _iommu_domain(
    segment: i32,
    bus: i32,
    dev: i32,
    func: i32,
    id: &mut i32,
) -> Option<&'static Domain> {
    let dom = domain_lookup(acpidmar_sc(), segment, mksid(bus, dev, func));
    if let Some(d) = dom {
        *id = d.did;
    }
    dom
}

/// `domain_map_device(dom, sid)`: points the device's context entry (VT-d; the bus's
/// context table is made on first use) or device table entry (AMD-Vi) at the domain.
#[cfg(machine_x86)]
pub fn domain_map_device(dom: &Domain, sid: i32) {
    let iommu = dom.iommu;

    let bus = sid_bus(sid);
    let devfn = sid_devfn(sid);
    // AMD attach device
    if iommu.is_amd() {
        if let Some(dte) = iommu.dte_at(sid)
            && dte.dw(0) == 0
        {
            // Setup Device Table Entry: bus.devfn
            dprintf!(
                1,
                "@@@ PCI Attach: {:04x}[{}] {:04x}\n",
                sid,
                dmar_bdf(sid),
                dom.did
            );
            dte_set_host_page_table_root_ptr(dte, dom.ptep);
            dte_set_domain(dte, dom.did as u16);
            dte_set_mode(dte, 3); // Set 3 level PTE
            dte_set_tv(dte);
            dte_set_valid(dte);
            let _ = ivhd_flush_devtab(iommu, sid);
            if IOMMU_DEBUG {
                ivhd_showdte(iommu);
            }
        }
        return;
    }

    let Some(root) = iommu.root_table() else {
        return;
    };

    // Create Bus mapping
    if !root_entry_is_valid(&root[bus as usize]) {
        let Some((va, paddr)) = iommu_alloc_page(iommu, false) else {
            iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
            kprintf!(
                "iommu{}: can't allocate context for bus {:02x}\n",
                iommu.id.get(),
                bus
            );
            return;
        };
        iommu.ctx[bus as usize].set(va.as_ptr().cast_const().cast());
        root[bus as usize]
            .lo
            .store(paddr | ROOT_P, Ordering::Relaxed);
        iommu_flush_cache(
            iommu,
            ptr::from_ref(&root[bus as usize]).cast(),
            size_of::<RootEntry>(),
        );
        dprintf!(
            0,
            "iommu{}: Allocate context for bus: {:02x} pa:{:016x} va:{:p}\n",
            iommu.id.get(),
            bus,
            paddr,
            va.as_ptr()
        );
    }

    // Create DevFn mapping
    let Some(ctxt) = iommu.ctx_table(bus) else {
        return;
    };
    let ctx = &ctxt[devfn as usize];
    if !context_entry_is_valid(ctx) {
        let tt = CTX_T_MULTI;
        let lvl = vtd_awtolevel(iommu.agaw.get());

        // Initialize context
        context_set_slpte(ctx, dom.ptep);
        context_set_translation_type(ctx, tt);
        context_set_domain_id(ctx, dom.did);
        context_set_address_width(ctx, lvl);
        context_set_present(ctx);

        // Flush it
        iommu_flush_cache(iommu, ptr::from_ref(ctx).cast(), size_of::<ContextEntry>());
        if iommu.cap.get() & CAP_CM != 0 || ACPIDMAR_FORCE_CM.load(Ordering::Relaxed) != 0 {
            iommu_flush_ctx(iommu, CTX_DEVICE, dom.did, sid, 0);
            iommu_flush_tlb(iommu, IOTLB_GLOBAL, 0);
        } else {
            iommu_flush_write_buffer(iommu);
        }
        dprintf!(
            0,
            "iommu{}: {} set context ptep:{:016x} lvl:{} did:{:04x} tt:{}\n",
            iommu.id.get(),
            dmar_bdf(sid),
            dom.ptep,
            lvl,
            dom.did,
            tt
        );
    }
}

/// `acpidmar_pci_attach(sc, segment, sid, mapctx)`: the device's domain, its context or
/// device table entry made too with `mapctx`.
#[cfg(machine_x86)]
pub fn acpidmar_pci_attach(
    sc: Option<&AcpidmarSoftc>,
    segment: i32,
    sid: i32,
    mapctx: bool,
) -> Option<&'static Domain> {
    let Some(dom) = domain_lookup(sc, segment, sid) else {
        kprintf!("no domain: {}\n", dmar_bdf(sid));
        return None;
    };

    if mapctx {
        domain_map_device(dom, sid);
    }

    Some(dom)
}

/// `acpidmar_pci_hook(pc, pa)`: amd64's `pci_probe_device_hook`: puts the device in a
/// domain, reserves its upstream bridges' windows, maps an ISA bridge's first 16 MB 1:1,
/// makes the device's entry, and gives the device the domain's `bus_dma` tag.
#[cfg(machine_x86)]
pub fn acpidmar_pci_hook(pc: PciChipsetTag, pa: &mut PciAttachArgs) {
    let Some(sc) = acpidmar_sc() else {
        // No DMAR, ignore
        return;
    };

    sc.sc_pc.set(pc);

    // Add device to our list if valid
    let (bus, dev, fun) = pci_decompose_tag(pc, pa.pa_tag);
    let sid = mksid(bus, dev, fun);
    if SID_FLAG[sid as usize & (MAX_DEVFN - 1)].load(Ordering::Relaxed) & SID_INVALID != 0 {
        return;
    }

    let reg = pci_conf_read(pc, pa.pa_tag, PCI_CLASS_REG);

    let segment = x86::acpipci_domain_to_seg(pa.pa_domain as i32);
    kassert!(segment >= 0);

    // Record PCI-PCI bridge forwarding windows
    let bhlc = pci_conf_read(pc, pa.pa_tag, PCI_BHLC_REG);
    let hdrtype = pci_hdrtype_type(bhlc); // header > class/subclass
    if hdrtype == 0x01 {
        acpidmar_ppbwin_record(sc, pc, pa, segment, sid as u16);
    }

    // Add device to domain
    let Some(dom) = acpidmar_pci_attach(Some(sc), segment, sid, false) else {
        return;
    };

    // Reserve upstream PCI-PCI bridge windows from this domain
    acpidmar_ppbwin_reserve(sc, dom, segment, bus);

    if pci_class(reg) == PCI_CLASS_DISPLAY && pci_subclass(reg) == PCI_SUBCLASS_DISPLAY_VGA {
        dom.flag.set(DOM_NOMAP);
    }
    if pci_class(reg) == PCI_CLASS_BRIDGE && pci_subclass(reg) == PCI_SUBCLASS_BRIDGE_ISA {
        // For ISA Bridges, map 0-16Mb as 1:1
        kprintf!(
            "dmar: {:04x}:{:02x}:{:02x}.{:x} mapping ISA\n",
            segment,
            bus,
            dev,
            fun
        );
        let _ = domain_map_pthru(dom, 0x00, 16 * 1024 * 1024);

        // Keep the identity mapped IOVA range out of the allocator
        let _ = extent_alloc_region(dom.iovamap, 0, 16 * 1024 * 1024, EX_WAITOK | EX_CONFLICTOK);
    }

    // Ensure this device has a programmed context/DTE at attach time so they don't need a
    // later allocation
    domain_map_device(dom, sid);

    if iommu_bad(dom.iommu) {
        return;
    }

    // Change DMA tag
    pa.pa_dmat = &dom.dmat;
}

/// `acpidmar_drhd(sc, de)`: DMA Remapping Hardware Unit.
#[cfg(machine_x86)]
pub fn acpidmar_drhd(sc: &AcpidmarSoftc, de: &[u8]) {
    let Some(drhd) = rd::<AcpidmarDrhd>(de, 0) else {
        return;
    };
    kprintf!(
        "DRHD: segment:{:04x} base:{:016x} flags:{:02x}\n",
        { drhd.segment },
        { drhd.address },
        drhd.flags
    );
    let iommu: &'static IommuSoftc = Box::leak(Box::new(IommuSoftc::new()));
    acpidmar_parse_devscope(
        de,
        size_of::<AcpidmarDrhd>(),
        i32::from(drhd.segment),
        &iommu.devices,
    );
    if iommu_init(sc, iommu, &drhd).is_err() {
        kprintf!("iommu init failed\n");
        if iommu.intr.get().is_null() {
            // SAFETY: the unit is in no list and nothing (no interrupt handler) refers to
            // it; its device scope elements stay leaked, as the C's.
            drop(unsafe { Box::from_raw(ptr::from_ref(iommu).cast_mut()) });
        }
        return;
    }

    // SAFETY: the unit is leaked and in no list yet.
    unsafe {
        if drhd.flags != 0 {
            // Catchall IOMMU goes at end of list
            sc.sc_drhds.insert_tail(iommu);
        } else {
            sc.sc_drhds.insert_head(iommu);
        }
    }
}

/// `acpidmar_rmrr(sc, de)`: Reserved Memory Region Reporting.
#[cfg(machine_x86)]
pub fn acpidmar_rmrr(sc: &AcpidmarSoftc, de: &[u8]) {
    let Some(r) = rd::<AcpidmarRmrr>(de, 0) else {
        return;
    };
    let (base, rlimit) = ({ r.base }, { r.limit });
    kprintf!(
        "RMRR: segment:{:04x} range:{:016x}-{:016x}\n",
        { r.segment },
        base,
        rlimit
    );
    if rlimit < base {
        kprintf!("  buggy BIOS\n");
        return;
    }

    let mut limit = rlimit.wrapping_add(1);
    if limit == 0 {
        limit = rlimit;
    }

    let rmrr: &'static RmrrSoftc = Box::leak(Box::new(RmrrSoftc {
        link: TailqEntry::new(),
        devices: TailqHead::new(),
        segment: i32::from(r.segment),
        start: Cell::new(trunc_page(base as usize) as u64),
        end: Cell::new(round_page(limit as usize) as u64),
    }));
    acpidmar_parse_devscope(de, size_of::<AcpidmarRmrr>(), rmrr.segment, &rmrr.devices);

    let mm: Vec<x86::BiosMemmap> = x86::bios_memmap().collect();
    for (i, im) in mm.iter().enumerate() {
        if { im.r#type } != x86::BIOS_MAP_RES {
            continue;
        }
        // Search for adjacent reserved regions
        let start = { im.addr };
        let mut end = start + { im.size };
        for jm in &mm[i + 1..] {
            if { jm.r#type } != x86::BIOS_MAP_RES || end != { jm.addr } {
                break;
            }
            end = { jm.addr } + { jm.size };
        }
        kprintf!("e820: {:016x} - {:016x}\n", start, end);
        if start <= rmrr.start.get() && rmrr.end.get() <= end {
            // Bah.. some buggy BIOS stomp outside RMRR
            kprintf!("  ** inside E820 Reserved {:016x} {:016x}\n", start, end);
            rmrr.start.set(trunc_page(start as usize) as u64);
            rmrr.end.set(round_page(end as usize) as u64);
            break;
        }
    }
    // SAFETY: a fresh element, leaked, never removed.
    unsafe { sc.sc_rmrrs.insert_tail(rmrr) };
}

/// `acpidmar_atsr(sc, de)`: Root Port ATS Reporting.
#[cfg(machine_x86)]
pub fn acpidmar_atsr(sc: &AcpidmarSoftc, de: &[u8]) {
    let Some(a) = rd::<AcpidmarAtsr>(de, 0) else {
        return;
    };
    kprintf!("ATSR: segment:{:04x} flags:{:x}\n", { a.segment }, a.flags);

    let atsr: &'static AtsrSoftc = Box::leak(Box::new(AtsrSoftc {
        link: TailqEntry::new(),
        devices: TailqHead::new(),
        segment: i32::from(a.segment),
        flags: i32::from(a.flags),
    }));
    acpidmar_parse_devscope(de, size_of::<AcpidmarAtsr>(), atsr.segment, &atsr.devices);

    // SAFETY: a fresh element, leaked, never removed.
    unsafe { sc.sc_atsrs.insert_tail(atsr) };
}

/// `acpidmar_init(sc, dmar)`: walks the DMAR table's remapping structures, then makes the
/// domains of the device scopes and maps the RMRR regions 1:1.
#[cfg(machine_x86)]
pub fn acpidmar_init(sc: &AcpidmarSoftc, dmar: &[u8]) {
    DOMAIN_MAP_PAGE_AMD.store(false, Ordering::Relaxed);
    let Some(d) = rd::<AcpiDmar>(dmar, 0) else {
        return;
    };
    kprintf!(
        ": hardware width: {}, intr_remap:{} x2apic_opt_out:{}\n",
        i32::from(d.haw) + 1,
        i32::from(d.flags & 0x1 != 0),
        i32::from(d.flags & 0x2 != 0)
    );
    sc.sc_haw.set(i32::from(d.haw) + 1);
    sc.sc_flags.set(i32::from(d.flags));

    sc.sc_drhds.init();
    sc.sc_rmrrs.init();
    sc.sc_atsrs.init();
    sc.sc_ppbwins.init();

    let mut off = size_of::<AcpiDmar>();
    while off < dmar.len() {
        let Some(h) = rd::<AcpidmarEntryHdr>(dmar, off) else {
            break;
        };
        let len = usize::from(h.length);
        if len == 0 {
            break;
        }
        let de = &dmar[off..(off + len).min(dmar.len())];
        let ty = h.r#type;
        match ty {
            DMAR_DRHD => acpidmar_drhd(sc, de),
            DMAR_RMRR => acpidmar_rmrr(sc, de),
            DMAR_ATSR => acpidmar_atsr(sc, de),
            t => {
                kprintf!("DMAR: unknown {:x}\n", t);
            }
        }
        off += len;
    }

    // Pre-create domains for iommu devices
    for iommu in sc.sc_drhds.iter() {
        for dl in iommu.devices.iter() {
            let Some(p0) = dl.dp.first() else {
                continue;
            };
            let sid = mksid(dl.bus, i32::from(p0.device), i32::from(p0.function));
            if let Some(dom) = acpidmar_pci_attach(Some(sc), iommu.segment.get(), sid, false) {
                kprintf!(
                    "{:04x}:{:02x}:{:02x}.{:x} iommu:{} did:{:04x}\n",
                    iommu.segment.get(),
                    dl.bus,
                    p0.device,
                    p0.function,
                    iommu.id.get(),
                    dom.did
                );
            }
        }
    }
    // Map passthrough pages for RMRR
    for rmrr in sc.sc_rmrrs.iter() {
        for dl in rmrr.devices.iter() {
            let Some(p0) = dl.dp.first() else {
                continue;
            };
            let sid = mksid(dl.bus, i32::from(p0.device), i32::from(p0.function));
            if let Some(dom) = acpidmar_pci_attach(Some(sc), rmrr.segment, sid, false) {
                kprintf!(
                    "{} map ident: {:016x} {:016x}\n",
                    dom_bdf(dom),
                    rmrr.start.get(),
                    rmrr.end.get()
                );
                let _ = domain_map_pthru(dom, rmrr.start.get(), rmrr.end.get());

                // Keep the identity mapped region out of the IOVA allocator
                let len = rmrr.end.get() - rmrr.start.get();
                let _ = extent_alloc_region(
                    dom.iovamap,
                    rmrr.start.get(),
                    len,
                    EX_WAITOK | EX_CONFLICTOK,
                );
            }
        }
    }
}

/// `acpiivhd_intr(ctx)`: AMD-Vi's interrupt: shows the logged events.
#[cfg(machine_x86)]
pub fn acpiivhd_intr(ctx: *mut c_void) -> i32 {
    // SAFETY: the argument `ivhd_intr_map` registered: a leaked unit.
    let iommu = unsafe { &*ctx.cast_const().cast::<IommuSoftc>() };

    if !iommu.is_amd() {
        return 0;
    }
    let _ = ivhd_poll_events(iommu);
    1
}

/// `ivhd_intr_map(iommu, devid)`: Setup interrupt for AMD: the unit's own PCI MSI.
#[cfg(machine_x86)]
pub fn ivhd_intr_map(iommu: &'static IommuSoftc, devid: i32) {
    if !iommu.intr.get().is_null() {
        return;
    }
    let ih = x86::PciIntrHandle {
        tag: pci_make_tag(None, sid_bus(devid), sid_dev(devid), sid_fun(devid)),
        line: x86::APIC_INT_VIA_MSG,
        pin: 0,
    };
    let intr = pci_intr_establish(
        None,
        ih,
        IPL_NET | IPL_MPSAFE,
        acpiivhd_intr,
        ptr::from_ref(iommu).cast_mut().cast(),
        "amd_iommu",
    );
    iommu.intr.set(intr.map_or(ptr::null_mut(), |p| p.as_ptr()));
    kprintf!("amd iommu intr: {:p}\n", iommu.intr.get());
}

/// `_dumppte(pte, lvl, va)`: prints a page table tree.
#[cfg(machine_x86)]
pub fn _dumppte(pte: &[PteEntry], lvl: i32, va: u64) {
    const PFX: [&str; 5] = ["    ", "   ", "  ", " ", ""];

    for (i, p) in pte.iter().enumerate().take(512) {
        let sh = (i as u64) << (((lvl - 1) * 9) + 12);
        let v = p.get();
        if v & PTE_P != 0 {
            let pfx = PFX.get(lvl as usize).copied().unwrap_or("");
            if lvl > 1 {
                let npte = pte_table(v & PTE_PADDR_MASK);
                kprintf!("{}lvl{}: {:016x} nxt:{}\n", pfx, lvl, v, (v >> 9) & 7);
                _dumppte(npte, lvl - 1, va | sh);
            } else {
                kprintf!("{}lvl{}: {:016x} <- {:016x} \n", pfx, lvl, v, va | sh);
            }
        }
    }
}

/// `ivhd_showpage(iommu, sid, paddr)`: the first faults show the device's entry and its
/// domain's page tables.
#[cfg(machine_x86)]
pub fn ivhd_showpage(iommu: &IommuSoftc, sid: i32, _paddr: u64) {
    if IVHD_SHOW.load(Ordering::Relaxed) > 10 {
        return;
    }
    IVHD_SHOW.fetch_add(1, Ordering::Relaxed);
    let Some(dom) = acpidmar_pci_attach(acpidmar_sc(), iommu.segment.get(), sid, false) else {
        return;
    };
    if let Some(dte) = iommu.dte_at(sid) {
        kprintf!(
            "DTE: {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}\n",
            dte.dw(0),
            dte.dw(1),
            dte.dw(2),
            dte.dw(3),
            dte.dw(4),
            dte.dw(5),
            dte.dw(6),
            dte.dw(7)
        );
    }
    _dumppte(dom_pte(dom), 3, 0);
}

/// `ivhd_show_event(iommu, evt, head)`: Display AMD IOMMU Error, then clears the event.
///
/// # Safety
///
/// `evt` is an entry of the unit's event log.
#[cfg(machine_x86)]
pub unsafe fn ivhd_show_event(iommu: &IommuSoftc, evt: *mut IvhdEvent, head: u32) {
    // SAFETY: the caller's guarantee; the IOMMU wrote the entry before moving the tail.
    let e = unsafe { evt.read_volatile() };

    // Get Device, Domain, Address and Type of event
    let sid = extract(e.dw0, EVT_SID_SHIFT, EVT_SID_MASK) as i32;
    let type_ = extract(e.dw1, EVT_TYPE_SHIFT, EVT_TYPE_MASK);
    let did = extract(e.dw1, EVT_DID_SHIFT, EVT_DID_MASK);
    let flag = extract(e.dw1, EVT_FLAG_SHIFT, EVT_FLAG_MASK);
    let address = u64::from(e.dw2) | (u64::from(e.dw3) << 32);
    let w = |m: u32, a: &'static str, b: &'static str| if e.dw1 & m != 0 { a } else { b };

    kprintf!("=== IOMMU Error[{:04x}]: ", head);
    match type_ {
        ILLEGAL_DEV_TABLE_ENTRY => {
            kprintf!(
                "illegal dev table entry dev={} addr=0x{:016x} {}, {}, {}, {}\n",
                dmar_bdf(sid),
                address,
                w(EVT_TR, "translation", "transaction"),
                w(EVT_RZ, "reserved bit", "invalid level"),
                w(EVT_RW, "write", "read"),
                w(EVT_I, "interrupt", "memory")
            );
            ivhd_showdte(iommu);
        }
        IO_PAGE_FAULT => {
            kprintf!(
                "io page fault dev={} did=0x{:04x} addr=0x{:016x}\n{}, {}, {}, {}, {}, {}\n",
                dmar_bdf(sid),
                did,
                address,
                w(EVT_TR, "translation", "transaction"),
                w(EVT_RZ, "reserved bit", "invalid level"),
                w(EVT_PE, "no perm", "perm"),
                w(EVT_RW, "write", "read"),
                w(EVT_PR, "present", "not present"),
                w(EVT_I, "interrupt", "memory")
            );
            ivhd_showdte(iommu);
            ivhd_showpage(iommu, sid, address);
        }
        DEV_TAB_HARDWARE_ERROR => {
            kprintf!(
                "device table hardware error dev={} addr=0x{:016x} {}, {}, {}\n",
                dmar_bdf(sid),
                address,
                w(EVT_TR, "translation", "transaction"),
                w(EVT_RW, "write", "read"),
                w(EVT_I, "interrupt", "memory")
            );
            ivhd_showdte(iommu);
        }
        PAGE_TAB_HARDWARE_ERROR => {
            kprintf!(
                "page table hardware error dev={} addr=0x{:016x} {}, {}, {}\n",
                dmar_bdf(sid),
                address,
                w(EVT_TR, "translation", "transaction"),
                w(EVT_RW, "write", "read"),
                w(EVT_I, "interrupt", "memory")
            );
            ivhd_showdte(iommu);
        }
        ILLEGAL_COMMAND_ERROR => {
            kprintf!("illegal command addr=0x{:016x}\n", address);
            ivhd_showcmd(iommu);
        }
        COMMAND_HARDWARE_ERROR => {
            kprintf!(
                "command hardware error addr=0x{:016x} flag=0x{:04x}\n",
                address,
                flag
            );
            ivhd_showcmd(iommu);
        }
        IOTLB_INV_TIMEOUT => {
            kprintf!(
                "iotlb invalidation timeout dev={} address=0x{:016x}\n",
                dmar_bdf(sid),
                address
            );
        }
        INVALID_DEVICE_REQUEST => {
            kprintf!(
                "invalid device request dev={} addr=0x{:016x} flag=0x{:04x}\n",
                dmar_bdf(sid),
                address,
                flag
            );
        }
        _ => {
            kprintf!("unknown type=0x{:02x}\n", type_);
        }
    }
    // Clear old event
    // SAFETY: the caller's guarantee; the entry is behind the head the driver owns.
    unsafe { evt.write_volatile(IvhdEvent::default()) };
}

/// `ivhd_poll_events(iommu)`: AMD: Process IOMMU error from hardware.
#[cfg(machine_x86)]
pub fn ivhd_poll_events(iommu: &IommuSoftc) -> i32 {
    let sz = size_of::<IvhdEvent>() as u32;
    let mut head = iommu_read_4(iommu, EVT_HEAD_REG);
    let tail = iommu_read_4(iommu, EVT_TAIL_REG);
    if head == tail {
        // No pending events
        return 0;
    }
    let tbl = iommu.evt_tbl.get();
    if tbl.is_null() {
        return 0;
    }
    while head != tail {
        // SAFETY: `head` is an offset inside the 4 KB event log, a multiple of the entry
        // size (the IOMMU's head register).
        let evt = unsafe { tbl.add((head % EVT_TBL_SIZE) as usize) }.cast::<IvhdEvent>();
        iommu_flush_cache(iommu, evt.cast_const().cast(), sz as usize);
        // SAFETY: as above.
        unsafe { ivhd_show_event(iommu, evt, head) };
        head = (head + sz) % EVT_TBL_SIZE;
    }
    iommu_write_4(iommu, EVT_HEAD_REG, head);
    0
}

/// `_ivhd_issue_command(iommu, cmd)`: AMD: Issue command to IOMMU queue; the command's slot.
#[cfg(machine_x86)]
pub fn _ivhd_issue_command(iommu: &IommuSoftc, cmd: &IvhdCommand) -> Result<i32, Errno> {
    let sz = size_of::<IvhdCommand>() as u32;
    let tbl = iommu.cmd_tbl.get();
    if tbl.is_null() {
        return Err(Errno::ENXIO);
    }

    mtx_enter(&iommu.reg_lock);

    let mut n = 0;
    let (tail, next) = loop {
        let head = iommu_read_4(iommu, CMD_HEAD_REG);
        let tail = iommu_read_4(iommu, CMD_TAIL_REG);
        let next = (tail + sz) % CMD_TBL_SIZE;
        if next != head {
            break (tail, next);
        }

        mtx_leave(&iommu.reg_lock);
        if n >= 1000 {
            kprintf!("iommu{}: IVHD cmd queue full\n", iommu.id.get());
            return Err(Errno::EBUSY);
        }
        delay(10);
        mtx_enter(&iommu.reg_lock);
        n += 1;
    };

    // SAFETY: `tail` is a 16-byte slot of the 4 KB command buffer, which the IOMMU does not
    // read until the tail register moves past it.
    let slot = unsafe { tbl.add((tail % CMD_TBL_SIZE) as usize) }.cast::<IvhdCommand>();
    // SAFETY: as above.
    unsafe { slot.write_volatile(*cmd) };
    iommu_flush_cache(iommu, slot.cast_const().cast(), sz as usize);
    iommu_write_4(iommu, CMD_TAIL_REG, next);

    mtx_leave(&iommu.reg_lock);

    Ok((tail / sz) as i32)
}

/// `ivhd_issue_command(iommu, cmd, wait)`: with `wait`, until the IOMMU has done it.
#[cfg(machine_x86)]
pub fn ivhd_issue_command(iommu: &IommuSoftc, cmd: &IvhdCommand, wait: bool) -> Result<i32, Errno> {
    let rc = _ivhd_issue_command(iommu, cmd);
    if rc.is_ok() && wait {
        ivhd_completion_wait(iommu);
    }
    rc
}

/// `ivhd_flush_devtab(iommu, devid)`: AMD: Flush changes to Device Table Entry for a
/// specific device.
#[cfg(machine_x86)]
pub fn ivhd_flush_devtab(iommu: &IommuSoftc, devid: i32) -> Result<i32, Errno> {
    let cmd = IvhdCommand {
        dw0: devid as u32,
        dw1: INVALIDATE_DEVTAB_ENTRY << CMD_SHIFT,
        ..IvhdCommand::default()
    };

    ivhd_issue_command(iommu, &cmd, true)
}

/// `ivhd_invalidate_iommu_all(iommu)`: AMD: Invalidate all IOMMU device and page tables.
#[cfg(machine_x86)]
pub fn ivhd_invalidate_iommu_all(iommu: &IommuSoftc) -> Result<i32, Errno> {
    let cmd = IvhdCommand {
        dw1: INVALIDATE_IOMMU_ALL << CMD_SHIFT,
        ..IvhdCommand::default()
    };

    ivhd_issue_command(iommu, &cmd, false)
}

/// `ivhd_invalidate_interrupt_table(iommu, devid)`: AMD: Invalidate interrupt remapping.
#[cfg(machine_x86)]
pub fn ivhd_invalidate_interrupt_table(iommu: &IommuSoftc, devid: i32) -> Result<i32, Errno> {
    let cmd = IvhdCommand {
        dw0: devid as u32,
        dw1: INVALIDATE_INTERRUPT_TABLE << CMD_SHIFT,
        ..IvhdCommand::default()
    };

    ivhd_issue_command(iommu, &cmd, false)
}

/// `ivhd_invalidate_domain(iommu, did)`: AMD: Invalidate all page tables in a domain.
#[cfg(machine_x86)]
pub fn ivhd_invalidate_domain(iommu: &IommuSoftc, did: i32) -> Result<i32, Errno> {
    let cmd = IvhdCommand {
        dw1: did as u32 | (INVALIDATE_IOMMU_PAGES << CMD_SHIFT),
        dw2: 0xFFFF_F000 | 0x3,
        dw3: 0x7FFF_FFFF,
        ..IvhdCommand::default()
    };
    ivhd_issue_command(iommu, &cmd, true)
}

/// `ivhd_completion_wait(iommu)`: AMD: Completion wait for batching. Enqueue a
/// COMPLETION_WAIT command and poll the completion variable until it is updated by the
/// IOMMU.
#[cfg(machine_x86)]
fn ivhd_completion_wait(iommu: &IommuSoftc) {
    let wv = iommu.wait_tbl.get();
    if wv.is_null() {
        kprintf!("iommu{}: missing completion wait buffer\n", iommu.id.get());
        return;
    }

    mtx_enter(&iommu.wait_lock);

    // Clear completion state before issuing the wait command
    // SAFETY: the completion page `ivhd_iommu_init` allocated; the IOMMU writes it only
    // when a wait completes, and `wait_lock` serializes the waits.
    unsafe {
        wv.write_volatile(0);
        wv.add(1).write_volatile(0);
    }
    iommu_flush_cache(iommu, wv.cast_const().cast(), size_of::<u64>() * 2);

    iommu.wait_seq.set(iommu.wait_seq.get().wrapping_add(1));
    let token = 0xfeed_c0de_0000_0000u64 | u64::from(iommu.wait_seq.get());

    let paddr = iommu.wait_tblp.get();
    let wq = IvhdCommand {
        dw0: (paddr & !0xF) as u32 | 0x1, // s = 1 (store)
        dw1: (COMPLETION_WAIT << CMD_SHIFT) | ((paddr >> 32) & 0xFFFFF) as u32,
        dw2: token as u32,
        dw3: (token >> 32) as u32,
    };

    if let Err(rc) = _ivhd_issue_command(iommu, &wq) {
        kprintf!(
            "iommu{}: completion wait enqueue failed: {}\n",
            iommu.id.get(),
            -(rc as i32)
        );
        mtx_leave(&iommu.wait_lock);
        return;
    }

    // wv[0] will be updated to token when the command completes
    let mut i = 0;
    while i < IVHD_MAXDELAY {
        iommu_flush_cache(iommu, wv.cast_const().cast(), size_of::<u64>() * 2);
        // SAFETY: as above.
        if unsafe { wv.read_volatile() } == token {
            break;
        }
        delay(10 << i);
        i += 1;
    }
    if i == IVHD_MAXDELAY {
        kprintf!("iommu{}: completion wait timeout\n", iommu.id.get());
    }

    mtx_leave(&iommu.wait_lock);
}

/// `ivhd_invalidate_page(iommu, did, iova)`: AMD: Invalidate a single 4KB page in a domain.
#[cfg(machine_x86)]
fn ivhd_invalidate_page(iommu: &IommuSoftc, did: i32, iova: BusAddr) -> Result<i32, Errno> {
    let iova = (iova & !VTD_PAGE_MASK) as u64;
    let cmd = IvhdCommand {
        dw1: did as u32 | (INVALIDATE_IOMMU_PAGES << CMD_SHIFT),
        dw2: (iova & !0xfff) as u32,
        dw3: (iova >> 32) as u32,
        ..IvhdCommand::default()
    };

    ivhd_issue_command(iommu, &cmd, true)
}

/// `ivhd_invalidate_segs(iommu, did, segs, nsegs)`: AMD: Invalidate translations for a list
/// of IOVA ranges.
#[cfg(machine_x86)]
fn ivhd_invalidate_segs<I>(iommu: &IommuSoftc, did: i32, segs: I)
where
    I: Iterator<Item = (BusAddr, BusSize)> + Clone,
{
    for (start, end) in seg_ranges(segs) {
        let mut addr = start;
        while addr < end {
            let cmd = IvhdCommand {
                dw1: did as u32 | (INVALIDATE_IOMMU_PAGES << CMD_SHIFT),
                dw2: (addr as u64 & !0xfff) as u32,
                dw3: (addr as u64 >> 32) as u32,
                ..IvhdCommand::default()
            };

            if let Err(rc) = _ivhd_issue_command(iommu, &cmd) {
                kprintf!(
                    "iommu{}: page invalidate enqueue failed: {}\n",
                    iommu.id.get(),
                    -(rc as i32)
                );
                ivhd_completion_wait(iommu);
                return;
            }
            addr += VTD_PAGE_SIZE;
        }
    }

    ivhd_completion_wait(iommu);
}

/// `ivhd_showreg(iommu)`: AMD: Display Registers.
#[cfg(machine_x86)]
pub fn ivhd_showreg(iommu: &IommuSoftc) {
    kprintf!(
        "---- dt:{:016x} cmd:{:016x} evt:{:016x} ctl:{:016x} sts:{:016x}\n",
        iommu_read_8(iommu, DEV_TAB_BASE_REG),
        iommu_read_8(iommu, CMD_BASE_REG),
        iommu_read_8(iommu, EVT_BASE_REG),
        iommu_read_8(iommu, IOMMUCTL_REG),
        iommu_read_8(iommu, IOMMUSTS_REG)
    );
    kprintf!(
        "---- cmd queue:{:016x} {:016x} evt queue:{:016x} {:016x}\n",
        iommu_read_8(iommu, CMD_HEAD_REG),
        iommu_read_8(iommu, CMD_TAIL_REG),
        iommu_read_8(iommu, EVT_HEAD_REG),
        iommu_read_8(iommu, EVT_TAIL_REG)
    );
}

/// `ivhd_checkerr(iommu)`: AMD: Generate Errors to test event handler.
#[cfg(machine_x86)]
pub fn ivhd_checkerr(iommu: &IommuSoftc) {
    let cmd = IvhdCommand {
        dw0: u32::MAX,
        dw1: u32::MAX,
        dw2: u32::MAX,
        dw3: u32::MAX,
    };

    // Generate ILLEGAL DEV TAB entry?
    if let Some(dte) = iommu.dte_at(0x2303) {
        dte.set_dw(0, u32::MAX); // invalid
        dte.set_dw(2, 0x1234); // domain
        dte.set_dw(7, u32::MAX); // reserved
    }
    let _ = ivhd_flush_devtab(iommu, 0x2303);
    let _ = ivhd_poll_events(iommu);

    // Generate ILLEGAL_COMMAND_ERROR : ok
    let _ = ivhd_issue_command(iommu, &cmd, false);
    let _ = ivhd_poll_events(iommu);

    // Generate page hardware error
}

/// `ivhd_showdte(iommu)`: AMD: Show Device Table Entry.
#[cfg(machine_x86)]
pub fn ivhd_showdte(iommu: &IommuSoftc) {
    for i in 0..MAX_DEVFN as i32 {
        let Some(dte) = iommu.dte_at(i) else {
            return;
        };
        if dte.dw(0) != 0 {
            kprintf!(
                "{:02x}:{:02x}.{:x}: {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}\n",
                i >> 8,
                (i >> 3) & 0x1F,
                i & 0x7,
                dte.dw(0),
                dte.dw(1),
                dte.dw(2),
                dte.dw(3),
                dte.dw(4),
                dte.dw(5),
                dte.dw(6),
                dte.dw(7)
            );
        }
    }
}

/// `ivhd_showcmd(iommu)`: AMD: Show command entries.
#[cfg(machine_x86)]
pub fn ivhd_showcmd(iommu: &IommuSoftc) {
    let ihd = iommu.cmd_tbl.get().cast::<IvhdCommand>();
    if ihd.is_null() {
        return;
    }
    let phd = iommu_read_8(iommu, CMD_BASE_REG) & CMD_BASE_MASK;
    for i in 0..4096 / 128 {
        // SAFETY: `i` < 32 entries of the 4 KB command buffer.
        let c = unsafe { ihd.add(i).read_volatile() };
        kprintf!(
            "{:02x}: {:016x} {:08x} {:08x} {:08x} {:08x}\n",
            i,
            phd + (i * size_of::<IvhdCommand>()) as u64,
            c.dw0,
            c.dw1,
            c.dw2,
            c.dw3
        );
    }
}

/// `ivhd_iommu_init(sc, iommu, ivhd)`: AMD: Initialize IOMMU.
#[cfg(machine_x86)]
pub fn ivhd_iommu_init(
    sc: &AcpidmarSoftc,
    iommu: &'static IommuSoftc,
    ivhd: &AcpiIvhd,
) -> Result<(), Errno> {
    let Some(memt) = sc.sc_memt.get() else {
        kprintf!("Bad pointer to iommu_init!\n");
        return Err(Errno::EINVAL);
    };
    let address = { ivhd.address };
    // SAFETY: the IVHD's register window, which the firmware gives to this unit.
    let Ok(ioh) = (unsafe { bus_space_map(memt, address as BusAddr, 0x80000, 0) }) else {
        kprintf!("Bus Space Map fails\n");
        return Err(Errno::ENXIO);
    };
    iommu.ioh.set(Some(ioh));
    iommu.domains.init();
    iommu.devices.init();

    mtx_init(&iommu.reg_lock, IPL_HIGH);
    mtx_init(&iommu.wait_lock, IPL_NONE);

    // Setup address width and number of domains
    iommu.id.set(NIOMMU_AMD.fetch_add(1, Ordering::Relaxed) + 1);
    iommu.iot.set(Some(memt));
    iommu.mgaw.set(48);
    iommu.agaw.set(48);
    iommu.flags.set(IOMMU_FLAGS_CATCHALL);
    if ivhd.flags & IVHD_COHERENT != 0 {
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_COHERENT);
    }
    iommu.segment.set(0);
    iommu.ndoms.set(256);

    kprintf!(": AMD iommu{} at 0x{:08x}\n", iommu.id.get(), address);

    iommu.ecap.set(iommu_read_8(iommu, EXTFEAT_REG));
    let ecap = iommu.ecap.get();
    let field = |shift: u64, mask: u64| ((ecap >> shift) & mask) as i32;
    dprintf!(0, "iommu{}: ecap:{:016x} ", iommu.id.get(), ecap);
    dprintf!(
        0,
        "{}{}{}{}{}{}{}{}\n",
        capname(ecap, EFR_PREFSUP, "pref "),
        capname(ecap, EFR_PPRSUP, "ppr "),
        capname(ecap, EFR_NXSUP, "nx "),
        capname(ecap, EFR_GTSUP, "gt "),
        capname(ecap, EFR_IASUP, "ia "),
        capname(ecap, EFR_GASUP, "ga "),
        capname(ecap, EFR_HESUP, "he "),
        capname(ecap, EFR_PCSUP, "pc ")
    );
    dprintf!(
        0,
        "hats:{:x} gats:{:x} glxsup:{:x} smif:{:x} smifrc:{:x} gam:{:x}\n",
        field(EFR_HATS_SHIFT, EFR_HATS_MASK),
        field(EFR_GATS_SHIFT, EFR_GATS_MASK),
        field(EFR_GLXSUP_SHIFT, EFR_GLXSUP_MASK),
        field(EFR_SMIFSUP_SHIFT, EFR_SMIFSUP_MASK),
        field(EFR_SMIFRC_SHIFT, EFR_SMIFRC_MASK),
        field(EFR_GAMSUP_SHIFT, EFR_GAMSUP_MASK)
    );

    // Turn off iommu
    let mut ov = iommu_read_8(iommu, IOMMUCTL_REG);
    iommu_write_8(
        iommu,
        IOMMUCTL_REG,
        ov & !(CTL_IOMMUEN | CTL_COHERENT | CTL_HTTUNEN | CTL_RESPASSPW | CTL_PASSPW | CTL_ISOC),
    );

    // Enable intr, mark IOMMU device as invalid for remap
    let devid = usize::from(ivhd.devid);
    SID_FLAG[devid].fetch_or(SID_INVALID, Ordering::Relaxed);
    ivhd_intr_map(iommu, devid as i32);

    // Setup command buffer with 4k buffer (128 entries)
    let Some((cmd_tbl, paddr)) = iommu_alloc_page(iommu, false) else {
        kprintf!("iommu{}: can't allocate command buffer\n", iommu.id.get());
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(Errno::ENOMEM);
    };
    iommu.cmd_tbl.set(cmd_tbl.as_ptr());
    iommu_write_8(
        iommu,
        CMD_BASE_REG,
        (paddr & CMD_BASE_MASK) | CMD_TBL_LEN_4K,
    );
    iommu_write_4(iommu, CMD_HEAD_REG, 0x00);
    iommu_write_4(iommu, CMD_TAIL_REG, 0x00);
    iommu.cmd_tblp.set(paddr);

    // Setup event log with 4k buffer (128 entries)
    let Some((evt_tbl, paddr)) = iommu_alloc_page(iommu, false) else {
        kprintf!("iommu{}: can't allocate event log\n", iommu.id.get());
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(Errno::ENOMEM);
    };
    iommu.evt_tbl.set(evt_tbl.as_ptr());
    iommu_write_8(
        iommu,
        EVT_BASE_REG,
        (paddr & EVT_BASE_MASK) | EVT_TBL_LEN_4K,
    );
    iommu_write_4(iommu, EVT_HEAD_REG, 0x00);
    iommu_write_4(iommu, EVT_TAIL_REG, 0x00);
    iommu.evt_tblp.set(paddr);

    // Completion wait buffer
    let Some((wait_tbl, paddr)) = iommu_alloc_page(iommu, false) else {
        kprintf!(
            "iommu{}: can't allocate completion wait buffer\n",
            iommu.id.get()
        );
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(Errno::ENOMEM);
    };
    iommu.wait_tbl.set(wait_tbl.as_ptr().cast());
    iommu.wait_tblp.set(paddr);
    iommu.wait_seq.set(0);

    // Setup device table
    // 1 entry per source ID (bus:device:function - 64k entries)
    if sc.sc_hwdte.get().is_null() {
        kprintf!("iommu{}: missing device table, disabling\n", iommu.id.get());
        iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_BAD);
        return Err(Errno::ENXIO);
    }
    iommu.dte.set(sc.sc_hwdte.get());
    iommu_write_8(
        iommu,
        DEV_TAB_BASE_REG,
        (sc.sc_hwdtep.get() & DEV_TAB_MASK) | DEV_TAB_LEN,
    );

    // Enable IOMMU
    ov |= CTL_IOMMUEN | CTL_EVENTLOGEN | CTL_CMDBUFEN | CTL_EVENTINTEN;
    if ivhd.flags & IVHD_COHERENT != 0 {
        ov |= CTL_COHERENT;
    }
    if ivhd.flags & IVHD_HTTUNEN != 0 {
        ov |= CTL_HTTUNEN;
    }
    if ivhd.flags & IVHD_RESPASSPW != 0 {
        ov |= CTL_RESPASSPW;
    }
    if ivhd.flags & IVHD_PASSPW != 0 {
        ov |= CTL_PASSPW;
    }
    if ivhd.flags & IVHD_ISOC != 0 {
        ov |= CTL_ISOC;
    }
    ov &= !(CTL_INVTIMEOUT_MASK << CTL_INVTIMEOUT_SHIFT);
    ov |= CTL_INVTIMEOUT_10MS << CTL_INVTIMEOUT_SHIFT;
    iommu_write_8(iommu, IOMMUCTL_REG, ov);

    let _ = ivhd_invalidate_iommu_all(iommu);

    // SAFETY: the unit is leaked and in no list yet.
    unsafe { sc.sc_drhds.insert_tail(iommu) };
    Ok(())
}

/// The `IVHD_*` flags `acpiivrs_ivhd` lists (DPRINTF).
#[cfg(machine_x86)]
fn ivhd_flags_dprintf(flags: u8) {
    let names: [(u8, &str); 8] = [
        (IVHD_PPRSUP, " PPRSup"),
        (IVHD_PREFSUP, " PreFSup"),
        (IVHD_COHERENT, " Coherent"),
        (IVHD_IOTLB, " Iotlb"),
        (IVHD_ISOC, " ISoc"),
        (IVHD_RESPASSPW, " ResPassPW"),
        (IVHD_PASSPW, " PassPW"),
        (IVHD_HTTUNEN, " HtTunEn"),
    ];
    for (bit, name) in names {
        if flags & bit != 0 {
            dprintf!(0, "{}", name);
        }
    }
    if flags != 0 {
        dprintf!(0, "\n");
    }
}

/// `acpiivrs_ivhd(sc, ivhd)`: an I/O Virtualization Hardware Definition: the extended
/// form (`IVRS_IVHD_EXT`) initializes the unit; the device entries are walked.
#[cfg(machine_x86)]
pub fn acpiivrs_ivhd(sc: &AcpidmarSoftc, ie: &[u8]) {
    let Some(ivhd) = rd::<AcpiIvhd>(ie, 0) else {
        return;
    };
    let mut all_dte = 0;
    let mut off;

    if ivhd.r#type == IVRS_IVHD_EXT {
        let Some(ext) = rd::<AcpiIvhdExt>(ie, 0) else {
            return;
        };
        dprintf!(
            0,
            "ivhd: {:02x} {:02x} {:04x} {:04x}:{} {:04x} {:016x} {:04x} {:08x} {:016x}\n",
            ext.r#type,
            ext.flags,
            { ext.length },
            { ext.segment },
            dmar_bdf(i32::from(ext.devid)),
            { ext.cap },
            { ext.address },
            { ext.info },
            { ext.attrib },
            { ext.efr }
        );
        ivhd_flags_dprintf(ext.flags);
        off = size_of::<AcpiIvhdExt>();
        let iommu: &'static IommuSoftc = Box::leak(Box::new(IommuSoftc::new()));
        let _ = ivhd_iommu_init(sc, iommu, &ivhd);
    } else {
        dprintf!(
            0,
            "ivhd: {:02x} {:02x} {:04x} {:04x}:{} {:04x} {:016x} {:04x} {:08x}\n",
            ivhd.r#type,
            ivhd.flags,
            { ivhd.length },
            { ivhd.segment },
            dmar_bdf(i32::from(ivhd.devid)),
            { ivhd.cap },
            { ivhd.address },
            { ivhd.info },
            { ivhd.feature }
        );
        ivhd_flags_dprintf(ivhd.flags);
        off = size_of::<AcpiIvhd>();
    }
    let length = usize::from(ivhd.length).min(ie.len());
    while off < length {
        let Some(ty) = rd::<u8>(ie, off) else {
            break;
        };
        match ty {
            IVHD_RESVD => {
                dprintf!(0, " RESVD\n");
                off += size_of::<AcpiIvhdEntryResvd>();
            }
            IVHD_ALL => {
                if let Some(e) = rd::<AcpiIvhdEntryAll>(ie, off) {
                    all_dte = i32::from(e.data);
                }
                dprintf!(0, " ALL {:04x}\n", all_dte);
                off += size_of::<AcpiIvhdEntryAll>();
            }
            IVHD_SEL => {
                if let Some(e) = rd::<AcpiIvhdEntrySel>(ie, off) {
                    let dte = e.data;
                    dprintf!(0, " SELECT: {} {:04x}\n", dmar_bdf(i32::from(e.devid)), dte);
                }
                off += size_of::<AcpiIvhdEntrySel>();
            }
            IVHD_SOR => {
                if let Some(e) = rd::<AcpiIvhdEntrySor>(ie, off) {
                    let dte = e.data;
                    let start = i32::from(e.devid);
                    dprintf!(0, " SOR: {} {:04x}\n", dmar_bdf(start), dte);
                }
                off += size_of::<AcpiIvhdEntrySor>();
            }
            IVHD_EOR => {
                if let Some(e) = rd::<AcpiIvhdEntryEor>(ie, off) {
                    dprintf!(0, " EOR: {}\n", dmar_bdf(i32::from(e.devid)));
                }
                off += size_of::<AcpiIvhdEntryEor>();
            }
            IVHD_ALIAS_SEL => {
                if let Some(e) = rd::<AcpiIvhdEntryAlias>(ie, off) {
                    let dte = e.data;
                    dprintf!(0, " ALIAS: src={}: ", dmar_bdf(i32::from(e.srcid)));
                    dprintf!(0, " {} {:04x}\n", dmar_bdf(i32::from(e.devid)), dte);
                }
                off += size_of::<AcpiIvhdEntryAlias>();
            }
            IVHD_ALIAS_SOR => {
                if let Some(e) = rd::<AcpiIvhdEntryAlias>(ie, off) {
                    let dte = e.data;
                    dprintf!(
                        0,
                        " ALIAS_SOR: {} {:04x} ",
                        dmar_bdf(i32::from(e.devid)),
                        dte
                    );
                    dprintf!(0, " src={}\n", dmar_bdf(i32::from(e.srcid)));
                }
                off += size_of::<AcpiIvhdEntryAlias>();
            }
            IVHD_EXT_SEL => {
                if let Some(e) = rd::<AcpiIvhdEntryExt>(ie, off) {
                    let dte = e.data;
                    dprintf!(
                        0,
                        " EXT SEL: {} {:04x} {:08x}\n",
                        dmar_bdf(i32::from(e.devid)),
                        dte,
                        { e.extdata }
                    );
                }
                off += size_of::<AcpiIvhdEntryExt>();
            }
            IVHD_EXT_SOR => {
                if let Some(e) = rd::<AcpiIvhdEntryExt>(ie, off) {
                    let dte = e.data;
                    dprintf!(
                        0,
                        " EXT SOR: {} {:04x} {:08x}\n",
                        dmar_bdf(i32::from(e.devid)),
                        dte,
                        { e.extdata }
                    );
                }
                off += size_of::<AcpiIvhdEntryExt>();
            }
            IVHD_SPECIAL => {
                dprintf!(0, " SPECIAL\n");
                off += size_of::<AcpiIvhdEntrySpecial>();
            }
            _ => {
                dprintf!(0, " 2:unknown {:x}\n", ty);
                off = length;
            }
        }
    }
}

/// `acpiivrs_ivmd(sc, ivmd)`: records an I/O Virtualization Memory Definition.
#[cfg(machine_x86)]
pub fn acpiivrs_ivmd(sc: &AcpidmarSoftc, ivmd: &AcpiIvmd) {
    let (start_id, end_id) = match ivmd.r#type {
        IVRS_IVMD_ALL => (0, 0xffff),
        IVRS_IVMD_SPECIFIED => ({ ivmd.devid }, { ivmd.devid }),
        IVRS_IVMD_RANGE => ({ ivmd.devid }, { ivmd.auxdata }),
        _ => (0, 0),
    };

    let entry = Box::leak(Box::new(IvmdEntry {
        link: TailqEntry::new(),
        addr: { ivmd.start_address },
        size: { ivmd.block_length },
        start_id,
        end_id,
        flags: ivmd.flags,
    }));

    // SAFETY: a fresh element, leaked, never removed.
    unsafe { sc.sc_ivmds.insert_tail(entry) };
}

/// `acpiivrs_init(sc, ivrs)`: allocates the shared device table and walks the IVRS table.
#[cfg(machine_x86)]
pub fn acpiivrs_init(sc: &AcpidmarSoftc, ivrs: &[u8]) {
    if sc.sc_hwdte.get().is_null() {
        match iommu_alloc_hwdte(sc, HWDTE_SIZE) {
            Some((va, pa)) => {
                sc.sc_hwdte.set(va.as_ptr().cast_const().cast());
                sc.sc_hwdtep.set(pa);
            }
            None => {
                kprintf!(
                    "{}: can't allocate HWDTE, disabling AMD IOMMU\n",
                    sc.sc_dev.xname()
                );
                return;
            }
        }
    }

    DOMAIN_MAP_PAGE_AMD.store(true, Ordering::Relaxed);
    let Some(t) = rd::<AcpiIvrs>(ivrs, 0) else {
        return;
    };
    dprintf!(0, "IVRS Version: {}\n", t.hdr.revision);
    dprintf!(
        0,
        " VA Size: {}\n",
        ({ t.ivinfo } >> IVRS_VASIZE_SHIFT) & IVRS_VASIZE_MASK
    );
    dprintf!(
        0,
        " PA Size: {}\n",
        ({ t.ivinfo } >> IVRS_PASIZE_SHIFT) & IVRS_PASIZE_MASK
    );

    sc.sc_drhds.init();
    sc.sc_rmrrs.init();
    sc.sc_atsrs.init();
    sc.sc_ppbwins.init();

    dprintf!(0, "======== IVRS\n");
    let mut off = size_of::<AcpiIvrs>();
    while off < ivrs.len() {
        let Some(h) = rd::<AcpiIvrsEntryHdr>(ivrs, off) else {
            break;
        };
        let len = usize::from(h.length);
        if len == 0 {
            break;
        }
        let ie = &ivrs[off..(off + len).min(ivrs.len())];
        match h.r#type {
            IVRS_IVHD | IVRS_IVHD_EXT => acpiivrs_ivhd(sc, ie),
            IVRS_IVMD_ALL | IVRS_IVMD_SPECIFIED | IVRS_IVMD_RANGE => {
                if let Some(ivmd) = rd::<AcpiIvmd>(ie, 0) {
                    acpiivrs_ivmd(sc, &ivmd);
                }
            }
            t => {
                dprintf!(0, "1:unknown: {:x}\n", t);
            }
        }
        off += len;
    }
    dprintf!(0, "======== End IVRS\n");
}

/// `acpiivhd_activate(iommu, act)`.
#[cfg(machine_x86)]
fn acpiivhd_activate(iommu: &IommuSoftc, act: i32) -> i32 {
    match act {
        DVACT_SUSPEND => iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_SUSPEND),
        DVACT_RESUME => iommu.flags.set(iommu.flags.get() & !IOMMU_FLAGS_SUSPEND),
        _ => {}
    }
    0
}

/// `acpidmar_activate(self, act)`: on suspend turns translation off (VT-d), on resume
/// reprograms the root table and the fault interrupt and turns it on again.
#[cfg(machine_x86)]
pub fn acpidmar_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `acpidmar_ca`'s softc is an `AcpidmarSoftc` (`ca_devsize`).
    let sc: &AcpidmarSoftc = unsafe { self_.softc() };

    kprintf!("called acpidmar_activate {} {:p}\n", act, ptr::from_ref(sc));

    match act {
        DVACT_RESUME => {
            for iommu in sc.sc_drhds.iter() {
                kprintf!("iommu{} resume\n", iommu.id.get());
                if iommu.is_amd() {
                    let _ = acpiivhd_activate(iommu, act);
                    continue;
                }
                iommu_flush_write_buffer(iommu);
                iommu_set_rtaddr(iommu, iommu.rtaddr.get());
                iommu_write_4(iommu, DMAR_FEDATA_REG, iommu.fedata.get() as u32);
                iommu_write_4(iommu, DMAR_FEADDR_REG, iommu.feaddr.get() as u32);
                iommu_write_4(iommu, DMAR_FEUADDR_REG, (iommu.feaddr.get() >> 32) as u32);
                if iommu.flags.get() & (IOMMU_FLAGS_BAD | IOMMU_FLAGS_SUSPEND)
                    == IOMMU_FLAGS_SUSPEND
                {
                    kprintf!("enable wakeup translation\n");
                    let _ = iommu_enable_translation(iommu, true);
                }
                iommu_showcfg(iommu, -1);
            }
        }
        DVACT_SUSPEND => {
            for iommu in sc.sc_drhds.iter() {
                kprintf!("iommu{} suspend\n", iommu.id.get());
                if iommu.flags.get() & IOMMU_FLAGS_BAD != 0 {
                    continue;
                }
                if iommu.is_amd() {
                    let _ = acpiivhd_activate(iommu, act);
                    continue;
                }
                iommu.flags.set(iommu.flags.get() | IOMMU_FLAGS_SUSPEND);
                let _ = iommu_enable_translation(iommu, false);
                iommu_showcfg(iommu, -1);
            }
        }
        _ => {}
    }
    Ok(())
}

/// The table acpi0 hands over in `aaa_table`: its header and its bytes.
///
/// # Safety
///
/// `aaa_table` is null or a table `acpi_maptable` copied (at least a header long, and as
/// long as its header says), alive for the kernel's lifetime.
#[cfg(machine_x86)]
unsafe fn aaa_table(aaa: &AcpiAttachArgs) -> Option<(AcpiTableHeader, &'static [u8])> {
    if aaa.aaa_table.is_null() {
        return None;
    }
    let p = aaa.aaa_table.cast_const().cast::<u8>();
    // SAFETY: the caller's guarantee.
    let hdr = unsafe { ptr::read_unaligned(p.cast::<AcpiTableHeader>()) };
    let len = ({ hdr.length } as usize).max(size_of::<AcpiTableHeader>());
    // SAFETY: the caller's guarantee: the copy is `length` bytes long.
    Some((hdr, unsafe { core::slice::from_raw_parts(p, len) }))
}

/// `acpidmar_match(parent, match, aux)`: a `DMAR` or an `IVRS` table.
#[cfg(machine_x86)]
pub fn acpidmar_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    // If we do not have a table, it is not us
    // SAFETY: `aaa_table` is null or a table acpi0 copied.
    let Some((hdr, _)) = (unsafe { aaa_table(aaa) }) else {
        return 0;
    };

    // If it is an DMAR table, we can attach
    if hdr.signature == *DMAR_SIG || hdr.signature == *IVRS_SIG {
        return 1;
    }

    0
}

/// `acpidmar_attach(parent, self, aux)`.
#[cfg(machine_x86)]
pub fn acpidmar_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `acpidmar_ca`'s softc is an `AcpidmarSoftc` (`ca_devsize`).
    let sc: &AcpidmarSoftc = unsafe { self_.softc() };
    // SAFETY: a softc `config_attach` allocated is never freed (acpidmar does not detach).
    let sc: &'static AcpidmarSoftc = unsafe { &*ptr::from_ref(sc) };
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    sc.sc_ivmds.init();

    // SAFETY: `aaa_table` is null or a table acpi0 copied and keeps.
    let Some((hdr, table)) = (unsafe { aaa_table(aaa) }) else {
        return;
    };
    sc.sc_memt.set(aaa.aaa_memt);
    sc.sc_dmat.set(aaa.aaa_dmat);
    if hdr.signature == *DMAR_SIG {
        ACPIDMAR_SC.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
        acpidmar_init(sc, table);
    }
    if hdr.signature == *IVRS_SIG {
        ACPIDMAR_SC.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
        acpiivrs_init(sc, table);
    }
}

/// The unit of a `dmarpic`.
#[cfg(machine_x86)]
fn pic_iommu(pic: &x86::Pic) -> &IommuSoftc {
    // SAFETY: `dmarpic` is only ever the `pic` member (first, `repr(C)`) of a unit's
    // `IommuPic`, whose `iommu` `acpidmar_intr_establish` set before the pic was used.
    unsafe {
        let ip = &*ptr::from_ref(pic).cast::<IommuPic>();
        &*ip.iommu.get()
    }
}

/// `acpidmar_msi_hwmask(pic, pin)`.
#[cfg(machine_x86)]
fn acpidmar_msi_hwmask(pic: &x86::Pic, _pin: i32) {
    let iommu = pic_iommu(pic);

    kprintf!("msi_hwmask\n");

    mtx_enter(&iommu.reg_lock);

    iommu_write_4(iommu, DMAR_FECTL_REG, FECTL_IM);
    let _ = iommu_read_4(iommu, DMAR_FECTL_REG);

    mtx_leave(&iommu.reg_lock);
}

/// `acpidmar_msi_hwunmask(pic, pin)`.
#[cfg(machine_x86)]
fn acpidmar_msi_hwunmask(pic: &x86::Pic, _pin: i32) {
    let iommu = pic_iommu(pic);

    kprintf!("msi_hwunmask\n");

    mtx_enter(&iommu.reg_lock);

    iommu_write_4(iommu, DMAR_FECTL_REG, 0);
    let _ = iommu_read_4(iommu, DMAR_FECTL_REG);

    mtx_leave(&iommu.reg_lock);
}

/// `acpidmar_msi_addroute(pic, ci, pin, vec, type)`: points the fault event at `vec` on
/// `ci`'s local APIC.
#[cfg(machine_x86)]
fn acpidmar_msi_addroute(pic: &x86::Pic, ci: &x86::CpuInfo, _pin: i32, vec: i32, _type: i32) {
    let iommu = pic_iommu(pic);

    mtx_enter(&iommu.reg_lock);

    iommu.fedata.set(vec);
    iommu
        .feaddr
        .set(0xfee0_0000 | (u64::from(ci.ci_apicid.get()) << 12));
    iommu_write_4(iommu, DMAR_FEDATA_REG, vec as u32);
    iommu_write_4(iommu, DMAR_FEADDR_REG, iommu.feaddr.get() as u32);
    iommu_write_4(iommu, DMAR_FEUADDR_REG, (iommu.feaddr.get() >> 32) as u32);

    mtx_leave(&iommu.reg_lock);
}

/// `acpidmar_msi_delroute(pic, ci, pin, vec, type)`.
#[cfg(machine_x86)]
fn acpidmar_msi_delroute(_pic: &x86::Pic, _ci: &x86::CpuInfo, _pin: i32, _vec: i32, _type: i32) {
    kprintf!("msi_delroute\n");
}

/// `acpidmar_intr_establish(ctx, level, func, arg, what)`: the unit's fault interrupt, an
/// edge-triggered MSI of `dmarpic`.
#[cfg(machine_x86)]
pub fn acpidmar_intr_establish(
    iommu: &'static IommuSoftc,
    level: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    what: &'static str,
) -> *mut c_void {
    iommu.pic.iommu.set(ptr::from_ref(iommu));

    x86::intr_establish(
        -1,
        &iommu.pic.pic,
        0,
        x86::IST_PULSE,
        level,
        None,
        func,
        arg,
        what,
    )
    .map_or(ptr::null_mut(), |ih| ih.as_ptr().cast())
}

/// `acpidmar_intr(ctx)`: Intel: Handle DMAR Interrupt: shows the recorded faults.
#[cfg(machine_x86)]
pub fn acpidmar_intr(ctx: *mut c_void) -> i32 {
    // SAFETY: the argument `iommu_init` registered: a leaked unit.
    let iommu = unsafe { &*ctx.cast_const().cast::<IommuSoftc>() };

    // splassert(IPL_HIGH);

    if iommu.gcmd.get() & GCMD_TE == 0 {
        return 1;
    }
    mtx_enter(&iommu.reg_lock);
    let _ = iommu_read_4(iommu, DMAR_FECTL_REG);
    let sts = iommu_read_4(iommu, DMAR_FSTS_REG);

    if sts & FSTS_PPF == 0 {
        mtx_leave(&iommu.reg_lock);
        return 1;
    }

    let nfr = cap_nfr(iommu.cap.get());
    let fro = cap_fro(iommu.cap.get()) as usize;
    let mut fri = (sts >> FSTS_FRI_SHIFT) & FSTS_FRI_MASK;
    for _ in 0..nfr {
        let hi = iommu_read_8(iommu, fro + (fri as usize * 16) + 8);
        if hi & FRCD_HI_F == 0 {
            break;
        }

        let lo = iommu_read_8(iommu, fro + (fri as usize * 16));
        let fe = FaultEntry { lo, hi };
        if OFE[1].load(Ordering::Relaxed) != fe.hi || OFE[0].load(Ordering::Relaxed) != fe.lo {
            iommu_showfault(iommu, fri as i32, &fe);
            OFE[1].store(fe.hi, Ordering::Relaxed);
            OFE[0].store(fe.lo, Ordering::Relaxed);
        }
        fri = (fri + 1) % nfr;
    }

    iommu_write_4(iommu, DMAR_FSTS_REG, FSTS_PFO | FSTS_PPF);

    mtx_leave(&iommu.reg_lock);

    1
}

/// `iommu_showpte(ptep, lvl, base)`: Intel: Show IOMMU page table entry.
#[cfg(machine_x86)]
pub fn iommu_showpte(ptep: u64, lvl: i32, base: u64) {
    let pte = pte_table(ptep);
    for (i, p) in pte.iter().enumerate() {
        let v = p.get();
        if v & PTE_P == 0 {
            continue;
        }
        let nb = base + ((i as u64) << lvl);
        let pb = v & !(VTD_PAGE_MASK as u64);
        if lvl == VTD_LEVEL0 {
            kprintf!(
                "   {:3x} {:016x} = {:016x} {}{} {}\n",
                i,
                nb,
                pb,
                if v == PTE_R { 'r' } else { ' ' },
                if v & PTE_W != 0 { 'w' } else { ' ' },
                if nb == pb { " ident" } else { "" }
            );
            if nb == pb {
                return;
            }
        } else {
            iommu_showpte(pb, lvl - VTD_STRIDE_SIZE, nb);
        }
    }
}

/// `iommu_showcfg(iommu, sid)`: Intel: Show IOMMU configuration.
#[cfg(machine_x86)]
pub fn iommu_showcfg(iommu: &IommuSoftc, _sid: i32) {
    let cmd = iommu_read_4(iommu, DMAR_GCMD_REG);
    let sts = iommu_read_4(iommu, DMAR_GSTS_REG);
    kprintf!(
        "iommu{}: flags:{} root pa:{:016x} {} {} {} {:08x} {:08x}\n",
        iommu.id.get(),
        iommu.flags.get(),
        iommu_read_8(iommu, DMAR_RTADDR_REG),
        if sts & GSTS_TES != 0 {
            "enabled"
        } else {
            "disabled"
        },
        if sts & GSTS_QIES != 0 { "qi" } else { "ccmd" },
        if sts & GSTS_IRES != 0 { "ir" } else { "" },
        cmd,
        sts
    );
    let Some(root) = iommu.root_table() else {
        return;
    };
    for (i, re) in root.iter().enumerate() {
        if !root_entry_is_valid(re) {
            continue;
        }
        let Some(ctxt) = iommu.ctx_table(i as i32) else {
            continue;
        };
        for (j, ctx) in ctxt.iter().enumerate() {
            if !context_entry_is_valid(ctx) {
                continue;
            }
            let tag: Pcitag = pci_make_tag(None, i as i32, (j >> 3) as i32, (j & 0x7) as i32);
            let clc = pci_conf_read(None, tag, 0x08) >> 8;
            kprintf!(
                "  {:02x}:{:02x}.{:x} lvl:{} did:{:04x} tt:{} ptep:{:016x} flag:{:x} cc:{:06x}\n",
                i,
                j >> 3,
                j & 7,
                context_address_width(ctx),
                context_domain_id(ctx),
                context_translation_type(ctx),
                context_pte(ctx),
                context_user(ctx),
                clc
            );
            // #if 0: dump pagetables: iommu_showpte(ctx->lo & ~VTD_PAGE_MASK,
            // iommu->agaw - VTD_STRIDE_SIZE, 0).
        }
    }
}

/// `iommu_showfault(iommu, fri, fe)`: Intel: Show IOMMU fault.
#[cfg(machine_x86)]
pub fn iommu_showfault(iommu: &IommuSoftc, fri: i32, fe: &FaultEntry) {
    if fe.hi & FRCD_HI_F == 0 {
        return;
    }
    let type_ = if fe.hi & FRCD_HI_T != 0 { 'r' } else { 'w' };
    let fr = ((fe.hi >> FRCD_HI_FR_SHIFT) & FRCD_HI_FR_MASK) as usize;
    let bus = ((fe.hi >> FRCD_HI_BUS_SHIFT) & FRCD_HI_BUS_MASK) as i32;
    let dev = ((fe.hi >> FRCD_HI_DEV_SHIFT) & FRCD_HI_DEV_MASK) as i32;
    let fun = ((fe.hi >> FRCD_HI_FUN_SHIFT) & FRCD_HI_FUN_MASK) as i32;
    let df = ((fe.hi >> FRCD_HI_FUN_SHIFT) & 0xFF) as usize;
    iommu_showcfg(iommu, mksid(bus, dev, fun));
    let mapped = match iommu.ctx_table(bus) {
        // Bus is not initialized
        None => "nobus",
        // DevFn not initialized
        Some(t) if !context_entry_is_valid(&t[df]) => "nodevfn",
        // no bus_space_map
        Some(t) if context_user(&t[df]) != 0xA => "nomap",
        // bus_space_map
        Some(_) => "mapped",
    };
    kprintf!(
        "fri{}: dmar: {:02x}:{:02x}.{:x} {} error at {:x} fr:{} [{}] iommu:{} [{}]\n",
        fri,
        bus,
        dev,
        fun,
        if type_ == 'r' { "read" } else { "write" },
        fe.lo,
        fr,
        if fr <= 13 { VTD_FAULTS[fr] } else { "unknown" },
        iommu.id.get(),
        mapped
    );
    for im in x86::bios_memmap() {
        let (addr, size) = ({ im.addr }, { im.size });
        if { im.r#type } == x86::BIOS_MAP_RES && addr <= fe.lo && fe.lo <= addr + size {
            kprintf!("mem in e820.reserved\n");
        }
    }
    // DDB
    if ACPIDMAR_DDB.load(Ordering::Relaxed) != 0 {
        db_enter();
    }
}

const _: () = {
    assert!(size_of::<RootEntry>() == 16);
    assert!(size_of::<ContextEntry>() == 16);
    assert!(size_of::<FaultEntry>() == 16);
    assert!(size_of::<PteEntry>() == 8);
    assert!(size_of::<QiEntry>() == 16);
    assert!(IOMMU_QI_SIZE == 4096);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn source_ids() {
        let sid = mksid(0x12, 0x1f, 3);
        assert_eq!(sid, 0x12fb);
        assert_eq!(sid_bus(sid), 0x12);
        assert_eq!(sid_dev(sid), 0x1f);
        assert_eq!(sid_fun(sid), 3);
        assert_eq!(sid_devfn(sid), 0xfb);
        assert_eq!(dmar_bdf(sid).to_string(), "0000:12:1f.3");
    }

    #[test]
    fn capability_fields() {
        // QEMU's intel-iommu (aw-bits 39): ND 2 (256 domains), SAGAW bit 1 (39 bits, 3
        // levels), MGAW 38 (39 bits), FRO 0x22 (0x220), NFR 0 (1 record), MAMV 0x3f, PSI.
        let cap: u64 = 0x2 | (0x2 << 8) | (38 << 16) | (0x22 << 24) | CAP_PSI | (0x3f << 48);
        assert_eq!(cap_nd(cap), 256);
        assert_eq!(cap_sagaw(cap), 2);
        assert_eq!(cap_mgaw(cap), 39);
        assert_eq!(cap_fro(cap), 0x220);
        assert_eq!(cap_nfr(cap), 1);
        assert_eq!(cap_mamv(cap), 0x3f);
        assert_eq!(dmar_frih_reg(cap, 0), 0x228);
        assert_eq!(dmar_fril_reg(cap, 1), 0x230);
        // ECAP IRO 0x20 (IOTLB registers at 0x200)
        let ecap: u64 = (0x20 << ECAP_IRO_SHIFT) | ECAP_QI;
        assert_eq!(dmar_iva_reg(ecap), 0x200);
        assert_eq!(dmar_iotlb_reg(ecap), 0x208);
        assert_eq!(vtd_leveltoaw(1), 39);
        assert_eq!(vtd_awtolevel(48), 2);
    }

    #[test]
    fn invalidation_command_fields() {
        assert_eq!(CIG_DEVICE, 3 << 61);
        assert_eq!(IIG_DOMAIN, 2 << 60);
        assert_eq!(QI_IOTLB_IG_PAGE, 0x30);
        assert_eq!(QI_CTX_IG_DOMAIN, 0x20);
        assert_eq!(qi_iotlb_did(0xfe), 0xfe << 16);
        assert_eq!(iotlb_did(0xfe), 0xfe << 32);
        assert_eq!(extract(0x5a5a_1234, 16, 0xfff), 0xa5a);
        assert_eq!(iommu_rmw32(0xffff_ffff, 0xf, 4, 0x3), 0xffff_ff3f);
        assert_eq!(iommu_rmw64(u64::MAX, 0xff, 40, 0), !(0xffu64 << 40));
        assert_eq!(xbit(0b100, 2), 1);
    }

    #[test]
    fn context_entry_fields() {
        let ce = ContextEntry::new();
        assert!(!context_entry_is_valid(&ce));
        context_set_slpte(&ce, 0x1234_5678_9000);
        context_set_translation_type(&ce, CTX_T_MULTI);
        context_set_domain_id(&ce, 0xfe);
        context_set_address_width(&ce, vtd_awtolevel(39));
        context_set_present(&ce);
        assert!(context_entry_is_valid(&ce));
        assert_eq!(context_pte(&ce), 0x1234_5678_9000);
        assert_eq!(context_domain_id(&ce), 0xfe);
        assert_eq!(context_address_width(&ce), 39);
        assert_eq!(context_translation_type(&ce), CTX_T_MULTI);
        assert_eq!(context_user(&ce), 0);
        context_set_user(&ce, 0xA);
        assert_eq!(context_user(&ce), 0xA);
        assert_eq!(context_domain_id(&ce), 0xfe);
        context_set_fpd(&ce, true);
        assert_eq!(ce.lo.load(Ordering::Relaxed) & CTX_FPD, CTX_FPD);
        context_set_fpd(&ce, false);
        assert_eq!(ce.lo.load(Ordering::Relaxed), 0x1234_5678_9000 | CTX_P);
        let re = RootEntry::new();
        assert!(!root_entry_is_valid(&re));
        re.lo.store(0x5000 | ROOT_P, Ordering::Relaxed);
        assert!(root_entry_is_valid(&re));
    }

    #[test]
    fn address_masks_cover_ranges() {
        // An aligned 64 KB range is one descriptor of AM 4.
        assert_eq!(iommu_intel_pick_am(0x10_0000, 0x11_0000, 0x3f), 4);
        // Misaligned start: one page at a time until aligned.
        assert_eq!(iommu_intel_pick_am(0x10_1000, 0x11_0000, 0x3f), 0);
        assert_eq!(iommu_intel_pick_am(0x10_2000, 0x11_0000, 0x3f), 1);
        // Capped by MAMV.
        assert_eq!(iommu_intel_pick_am(0, 0x10_0000, 2), 2);
        assert_eq!(iommu_intel_pick_am(0, 0x10_0000, 0), 0);
        // A 0x5000-byte segment at 0x103800 spans the pages 0x103000..0x109000 (6 pages):
        // descriptors 0x103000 (1 page), 0x104000 (4 pages), 0x108000 (1 page).
        let segs = [(0x10_3800usize, 0x5000usize), (0x20_0000, 0)];
        assert_eq!(iommu_flush_tlb_counts(segs.iter().copied(), 0x3f), (6, 3));
        assert_eq!(iommu_flush_tlb_counts(segs.iter().copied(), 0), (6, 6));
    }

    #[test]
    fn device_scope_is_parsed() {
        // A DRHD header (16 bytes) followed by an endpoint 00:1f.2, a bridge 00:1c.0 -> 03.1
        // (two path entries) and an IOAPIC scope, which is skipped.
        let mut de = vec![0u8; 16];
        de.extend_from_slice(&[DMAR_ENDPOINT, 8, 0, 0, 0, 0x00, 0x1f, 2]);
        de.extend_from_slice(&[DMAR_BRIDGE, 10, 0, 0, 0, 0x00, 0x1c, 0, 0x03, 1]);
        de.extend_from_slice(&[3, 8, 0, 0, 0, 0xf0, 0x1f, 0]);
        let list = TailqHead::<DevlistHead>::new();
        acpidmar_parse_devscope(&de, 16, 0, &list);
        let v: Vec<_> = list.iter().collect();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].r#type, v[0].bus, v[0].ndp), (1, 0, 1));
        assert_eq!((v[0].dp[0].device, v[0].dp[0].function), (0x1f, 2));
        assert_eq!((v[1].r#type, v[1].ndp), (2, 2));
        assert_eq!((v[1].dp[1].device, v[1].dp[1].function), (3, 1));
        // A zero-length entry ends the walk.
        let mut bad = vec![0u8; 16];
        bad.extend_from_slice(&[DMAR_ENDPOINT, 0, 0, 0, 0, 0, 0, 0]);
        let empty = TailqHead::<DevlistHead>::new();
        acpidmar_parse_devscope(&bad, 16, 0, &empty);
        assert!(empty.is_empty());
    }

    #[test]
    fn fault_reason_names() {
        assert_eq!(VTD_FAULTS[VTD_FAULT_WRITE as usize], "Write");
        assert_eq!(VTD_FAULTS[VTD_FAULT_CTX_TT as usize], "Context Entry TT");
    }
}
/* </TESTS> */
