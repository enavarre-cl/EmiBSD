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
 * Copyright (c) 2006 Gordon Willem Klok <gklok@cogeco.ca>
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
//! bios0: `arch/amd64/amd64/bios.c`, the firmware's node below mainbus. It reads the
//! SMBIOS tables (the machine's vendor and product) and attaches the firmware's other
//! interfaces: `efi0`, `acpi0` and `mpbios0`.
//!
//! Upstream: sys/arch/amd64/amd64/bios.c @ 3ce1f3f79392
//!
//! `bios_attach` looks for the SMBIOS 2 entry point (`_SM_`, `smbios_find`) where efiboot
//! said it is (`bios_efiinfo->config_smbios`, the `SMBIOS_TABLE_GUID` configuration table;
//! under Limine its SMBIOS request, `machdep.rs`), else every 16 bytes of the BIOS area. It
//! maps the structure table read-only for good (`smbios_entry`), prints its revision, the
//! BIOS information (type 0: vendor, version, date) and the system's vendor and product, and
//! sets `hw.vendor`, `hw.product`, `hw.version`, `hw.serialno` and `hw.uuid` from the system
//! information (type 1), falling back on the base board's (type 2) (`smbios_info`).
//! `smbios_find_table` and `smbios_get_string` are what other drivers read the table with
//! (ipmi(4)'s probe of the IPMI device information, type 38). Without SMBIOS it looks for a
//! Soekris comBIOS in the BIOS area.
//!
//! On QEMU's `q35` and `pc` with OVMF, as OpenBSD 8.0 does there, the type 0 line is a bare
//! `bios0:`: OVMF puts that structure last, and `smbios_get_string` gives no string that
//! lies within 64 bytes (the caller's buffer) of the table's end.
//!
//! ## Deviations
//! - The entry point at `config_smbios` and the BIOS area (`ISA_HOLE_VADDR(SMBIOS_START)`
//!   to `SMBIOS_END`) are read through a cacheable `bus_space_map` of memory space, unmapped
//!   once read, where the C reads them through `PMAP_DIRECT_MAP` and `locore0.S`'s mapping of
//!   the ISA hole: under Limine the direct map covers only the regions of the bootloader's
//!   memory map, which need not include the firmware's tables or the BIOS area (ipmi(4)'s
//!   `scan_sig` does the same). The structure table is mapped as in the C.
//! - `smbios_find` takes the bytes it may read (a slice) and returns a copy of the entry
//!   point, not a pointer into it: an entry point cut short by the end of the area is none.
//! - The structure table is read as the byte slice of `smbios_entry.len` bytes: a search
//!   stops at its end, where the C's `while (*va++)` reads on, and a formatted area cut short
//!   by the end of the table reads as zeros past it (`smbios_struct`). `struct smbtable`
//!   keeps the C's pointers into the table.
//! - `smbios_get_string` returns the filled `dest` (`Option<&mut [u8]>`, the C's `char *` or
//!   NULL) and `fixstring` the fixed string without its NUL (the C's pointer into the same
//!   buffer). `smbios_find_table_in` and `smbios_get_string_in` are the two over a given
//!   `smbios_entry`, for the host tests.
//! - `smbios_info` gathers the strings first (`smbios_info_strings`, host-tested), then
//!   sets the `hw_*` globals and prints, in the C's order. `enqueue_randomness` is the visible
//!   stub of `dev/rnd.rs` (the entropy pool is not ported).
//! - The vendor of the type 0 line, when `fixstring` rejects it, prints `(null)`, which is
//!   what the C's kernel `printf` makes of the NULL it is given.
//! - `bios_efiinfo` is Limine's (`machdep.rs`, `BIOS_EFIINFO_CONFIG_ACPI` and
//!   `bios_efiinfo().config_smbios`): acpi0 gets the RSDP and bios0 the SMBIOS entry point
//!   the firmware gave, as on an EFI boot. `efi0` (`efi_machdep.c`) and `mpbios0`
//!   (`mpbios.c`) are not ported and are reported where the C would attach them.
//! - `struct bios_attach_args` (`<machine/biosvar.h>`) is `BiosAttachArgs` here, not in
//!   `include/biosvar.rs`: that module is shared by path with efiboot (M14), which must
//!   build it without the kernel's bus_space types.
//! - The host double compiles this file for its tests (docs/ARCHITECTURE.md, "Host tests of
//!   arch code"): the headers are reached as `super::super::include`, and what reaches the
//!   machine (the attach, the mappings, `bios_efiinfo`) is `target_os = "none"` only.

#[cfg(target_os = "none")]
use core::ffi::c_void;
use core::ptr;
use core::slice;
#[cfg(target_os = "none")]
use core::sync::atomic::Ordering;

use libkern::{StaticCell, strlcpy, strncasecmp, strnlen};

use super::super::include::biosvar::SMBIOS_SIGNATURE;
#[cfg(target_os = "none")]
use super::super::include::smbiosvar::{SMBIOS_END, SMBIOS_START};
use super::super::include::smbiosvar::{
    SMBIOS_TYPE_BASEBOARD, SMBIOS_TYPE_BIOS, SMBIOS_TYPE_EOT, SMBIOS_TYPE_SYSTEM,
    SMBIOS_UUID_NPRESENT, SMBIOS_UUID_NSET, SMBIOS_UUID_REPLEN, Smbhdr, SmbiosBoard, SmbiosEntry,
    SmbiosStructBios, SmbiosSys, Smbtable, Smbtblhdr,
};
#[cfg(target_os = "none")]
use crate::arch::amd64::amd64::bus_space::{
    BUS_SPACE_MAP_CACHEABLE, X86_BUS_SPACE_IO, X86_BUS_SPACE_MEM, X86BusSpace, bus_space_map,
    bus_space_unmap, bus_space_vaddr,
};
#[cfg(target_os = "none")]
use crate::arch::amd64::amd64::machdep::{BIOS_EFIINFO_CONFIG_ACPI, bios_efiinfo};
#[cfg(target_os = "none")]
use crate::arch::amd64::amd64::pmap::pmap_kenter_pa;
#[cfg(target_os = "none")]
use crate::arch::amd64::include::param::{NBPG, PGOFSET};
use crate::dev::rnd::enqueue_randomness;
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_sysctl::{hw_prod, hw_serial, hw_uuid, hw_vendor, hw_ver};
#[cfg(target_os = "none")]
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{Str, snprintf};
use crate::kprintf;
#[cfg(target_os = "none")]
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, UNCONF};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
#[cfg(target_os = "none")]
use crate::sys::mman::PROT_READ;
#[cfg(target_os = "none")]
use crate::sys::types::{Paddr, Vaddr};
#[cfg(target_os = "none")]
use crate::unported;
#[cfg(target_os = "none")]
use crate::uvm::uvm_km::{KD_NOWAIT, KP_NONE, KV_ANY, km_alloc};
#[cfg(target_os = "none")]
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `struct bios_attach_args` (`<machine/biosvar.h>`): what bios0 hands `acpi0`, `efi0` and
/// `mpbios0`.
#[cfg(target_os = "none")]
#[repr(C)]
pub struct BiosAttachArgs {
    /// `ba_name`: the driver to match; first, as every amd64 attach argument starts with
    /// the name (`mainbus_print`).
    pub ba_name: &'static [u8],
    /// `ba_func`.
    pub ba_func: u32,
    /// `ba_iot`.
    pub ba_iot: X86BusSpace,
    /// `ba_memt`.
    pub ba_memt: X86BusSpace,
    /// `ba_acpipbase`: the RSDP's physical address, 0 when unknown.
    pub ba_acpipbase: usize,
}

/// `struct bios_softc`: bios0 is its device alone.
#[cfg(target_os = "none")]
pub type BiosSoftc = Device;

/// A string `smbios_info` keeps: its `sminfo[64]`, fixed by `fixstring` and NUL-terminated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SmbiosString([u8; 64]);

impl SmbiosString {
    /// The string, without its NUL.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..strnlen(&self.0, self.0.len())]
    }
}

/// What `smbios_info` finds in the system's UUID (`hw_uuid`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SmbiosUuid {
    /// SMBIOS before 2.1 has none: `hw_uuid` is left alone.
    #[default]
    Unread,
    /// All zero: the UUID is not present (`SMBIOS_UUID_NPRESENT`, `hw_uuid` NULL).
    NotPresent,
    /// All 0xff: present but not set (`SMBIOS_UUID_NSET`, `hw_uuid` "Not Set").
    NotSet,
    /// The UUID.
    Set([u8; 16]),
}

/// The strings `smbios_info` reads, before it stores them.
#[derive(Clone, Copy, Default, Debug)]
pub struct SmbiosInfo {
    /// `smbios_board_vendor`.
    pub board_vendor: Option<SmbiosString>,
    /// `smbios_board_prod`.
    pub board_prod: Option<SmbiosString>,
    /// `smbios_board_serial`.
    pub board_serial: Option<SmbiosString>,
    /// `hw_vendor`: the system's vendor, else the board's.
    pub vendor: Option<SmbiosString>,
    /// `hw_prod`: the system's product, else the board's.
    pub prod: Option<SmbiosString>,
    /// `hw_ver`.
    pub ver: Option<SmbiosString>,
    /// `hw_serial`.
    pub serial: Option<SmbiosString>,
    /// `hw_uuid`.
    pub uuid: SmbiosUuid,
}

/// The `<machine/smbiosvar.h>` structures bios0 reads out of firmware bytes.
///
/// # Safety
///
/// Implementors are `#[repr(C, packed)]` structures of integers and integer arrays only: no
/// padding, and every bit pattern is a value.
unsafe trait SmbiosPod: Copy + Default {}

// SAFETY: packed structures of integers (`smbiosvar.rs`).
unsafe impl SmbiosPod for Smbhdr {}
// SAFETY: as above.
unsafe impl SmbiosPod for SmbiosStructBios {}
// SAFETY: as above.
unsafe impl SmbiosPod for SmbiosSys {}
// SAFETY: as above.
unsafe impl SmbiosPod for SmbiosBoard {}

/// `bios_ca`.
#[cfg(target_os = "none")]
pub static BIOS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<BiosSoftc>(),
    ca_match: Some(bios_match),
    ca_attach: bios_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `bios_cd`.
#[cfg(target_os = "none")]
pub static BIOS_CD: Cfdriver = Cfdriver::new(b"bios", DV_DULL, CD_COCOVM);

/// `smbios_entry`: the structure table bios0 mapped; written once by `bios_attach`, during
/// autoconfiguration on the boot CPU, before anything reads it.
pub static SMBIOS_ENTRY: StaticCell<SmbiosEntry> = StaticCell::new(SmbiosEntry::new());

/// `smbios_uninfo[]`: what an uninformative string starts with.
const SMBIOS_UNINFO: [&[u8]; 4] = [b"System", b"Not ", b"To be", b"SYS-"];

/// `smbios_bios_date[64]`.
pub static SMBIOS_BIOS_DATE: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_bios_version[64]`.
pub static SMBIOS_BIOS_VERSION: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_vendor[64]`.
pub static SMBIOS_BOARD_VENDOR: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_prod[64]`.
pub static SMBIOS_BOARD_PROD: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_serial[64]`.
pub static SMBIOS_BOARD_SERIAL: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);

/// The comBIOS signature of a Soekris board.
const SOEKRIS_SIGNATURE: &[u8] = b"Soekris Engineering";

/// `bios_match(parent, match, aux)`: only one.
#[cfg(target_os = "none")]
pub fn bios_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: mainbus hands bios0 its `mba_bios`, a `struct bios_attach_args`.
    let bia = unsafe { &*aux.cast_const().cast::<BiosAttachArgs>() };

    // only one
    if BIOS_CD.cd_ndevs.get() != 0 || bia.ba_name != BIOS_CD.cd_name {
        return 0;
    }
    1
}

/// `bios_attach(parent, self, aux)`: SMBIOS, then the firmware interfaces below bios0.
#[cfg(target_os = "none")]
pub fn bios_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    let mut smbiosrev = 0;
    let mut hdr = None;

    if let Some(ei) = bios_efiinfo()
        && ei.config_smbios != 0
    {
        hdr = smbios_find_phys(ei.config_smbios as usize);
    }

    if hdr.is_none() {
        // see if we have SMBIOS extensions
        hdr = smbios_area(SMBIOS_START, SMBIOS_END - SMBIOS_START + 1, |area| {
            (0..SMBIOS_END - SMBIOS_START)
                .step_by(16)
                .find_map(|p| smbios_find(&area[p..]))
        })
        .flatten();
    }

    if let Some(hdr) = hdr {
        let (addr, size) = (hdr.addr as usize, usize::from(hdr.size));
        let mut pa = trunc_page(addr);
        let end = round_page(addr + size);
        if let Some(va) = km_alloc(end - pa, &KV_ANY, &KP_NONE, &KD_NOWAIT) {
            let mut va = va.as_ptr() as usize;

            let entry = SmbiosEntry {
                addr: (va + (addr & PGOFSET)) as *const u8,
                len: hdr.size,
                mjr: hdr.majrev,
                min: hdr.minrev,
                count: hdr.count,
            };
            // SAFETY: bios0 attaches once, during autoconfiguration on the boot CPU; no reader
            // of smbios_entry runs before it returns.
            unsafe { SMBIOS_ENTRY.write(entry) };

            while pa < end {
                // SAFETY: `va` is fresh kernel virtual space from km_alloc(kv_any, kp_none)
                // that nothing else maps; `pa` is the firmware's structure table.
                unsafe { pmap_kenter_pa(Vaddr::new(va), Paddr::new(pa), PROT_READ) };
                pa += NBPG;
                va += NBPG;
            }

            let (majrev, minrev, count) = (hdr.majrev, hdr.minrev, hdr.count);
            kprintf!(
                ": SMBIOS rev. {}.{} @ 0x{:x} ({} entries)",
                majrev,
                minrev,
                addr,
                count
            );

            smbiosrev = i32::from(majrev) * 100 + i32::from(minrev);
            if minrev < 10 {
                smbiosrev = i32::from(majrev) * 100 + i32::from(minrev) * 10;
            }

            let mut bios = Smbtable::default();
            if smbios_find_table(SMBIOS_TYPE_BIOS, &mut bios) != 0 {
                let sb: SmbiosStructBios = smbios_struct(&entry, &bios);
                let mut scratch = [0u8; 64];
                kprintf!("\n{}:", self_.xname());
                if let Some(s) = smbios_get_string(&bios, sb.vendor, &mut scratch) {
                    match fixstring(s) {
                        Some(v) => {
                            kprintf!(" vendor {}", Str(v));
                        }
                        None => {
                            kprintf!(" vendor (null)");
                        }
                    }
                }
                if let Some(s) = smbios_get_string(&bios, sb.version, &mut scratch)
                    && let Some(sminfop) = fixstring(s)
                {
                    // SAFETY: as for smbios_entry: written here, before any reader.
                    strlcpy(unsafe { SMBIOS_BIOS_VERSION.get_mut() }, sminfop);
                    kprintf!(" version \"{}\"", Str(sminfop));
                }
                if let Some(s) = smbios_get_string(&bios, sb.release, &mut scratch)
                    && let Some(sminfop) = fixstring(s)
                {
                    // SAFETY: as above.
                    strlcpy(unsafe { SMBIOS_BIOS_DATE.get_mut() }, sminfop);
                    kprintf!(" date {}", Str(sminfop));
                }
            }

            smbios_info(self_.xname());
        }
    }

    // out:
    kprintf!("\n");

    // No SMBIOS extensions, go looking for Soekris comBIOS
    if smbiosrev == 0 {
        let _ = smbios_area(SMBIOS_START, SMBIOS_END - SMBIOS_START + 1, soekris_info);
    }

    // NEFI > 0
    let _ = unported!("efi0 at bios0 (efi_machdep.c)");

    // NACPI > 0
    {
        let mut ba = BiosAttachArgs {
            ba_name: b"acpi",
            ba_func: 0,
            ba_iot: X86_BUS_SPACE_IO,
            ba_memt: X86_BUS_SPACE_MEM,
            ba_acpipbase: BIOS_EFIINFO_CONFIG_ACPI.load(Ordering::Relaxed) as usize,
        };

        let _ = config_found(self_, ptr::from_mut(&mut ba).cast(), Some(bios_print));
    }

    // NMPBIOS > 0: if (mpbios_probe(self)) config_found(self, &ba "mpbios", bios_print).
    let _ = unported!("mpbios_probe (mpbios0 at bios0, mpbios.c)");
}

/// `f` run on the `size` bytes of physical memory at `pa`, mapped for the call (see the
/// module's deviations); `None` when they cannot be mapped.
#[cfg(target_os = "none")]
fn smbios_area<R>(pa: usize, size: usize, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    // SAFETY: firmware memory (the BIOS area or the entry point the firmware named), only
    // read, and unmapped below.
    let h = unsafe { bus_space_map(X86_BUS_SPACE_MEM, pa, size, BUS_SPACE_MAP_CACHEABLE) }.ok()?;
    let va = bus_space_vaddr(X86_BUS_SPACE_MEM, h);
    // SAFETY: `size` bytes are mapped at `va` until the unmap below; nothing writes them.
    let r = f(unsafe { slice::from_raw_parts(va.cast_const(), size) });
    bus_space_unmap(X86_BUS_SPACE_MEM, h, size);
    Some(r)
}

/// `smbios_find(PMAP_DIRECT_MAP(pa))`: the entry point at physical address `pa`, read as
/// far as its length byte says (see the module's deviations).
#[cfg(target_os = "none")]
fn smbios_find_phys(pa: usize) -> Option<Smbhdr> {
    let len = smbios_area(pa, size_of::<Smbhdr>(), |p| p[5])?;
    smbios_area(pa, usize::from(len).max(size_of::<Smbhdr>()), smbios_find).flatten()
}

/// A `T` from the bytes `b` begins with, zeros past their end.
fn pod_from<T: SmbiosPod>(b: &[u8]) -> T {
    let mut v = T::default();
    let n = b.len().min(size_of::<T>());
    // SAFETY: `T` is a packed structure of integers (`SmbiosPod`): any `n <= size_of::<T>()`
    // bytes written over it leave a value; `b` and `v` do not overlap.
    unsafe { ptr::copy_nonoverlapping(b.as_ptr(), ptr::from_mut(&mut v).cast::<u8>(), n) };
    v
}

/// `smbios_find(p)`: the SMBIOS 2 entry point at the start of `p`, if its anchor, its
/// intermediate anchor (`_DMI_`) and both checksums are right.
pub fn smbios_find(p: &[u8]) -> Option<Smbhdr> {
    let hdr: Smbhdr = pod_from(p);

    if hdr.sig != SMBIOS_SIGNATURE {
        return None;
    }
    let sum = |b: &[u8]| b.iter().fold(0u8, |c, &x| c.wrapping_add(x));
    if sum(p.get(..usize::from(hdr.len))?) != 0 {
        return None;
    }
    let p = p.get(0x10..)?;
    if !p.starts_with(b"_DMI_") {
        return None;
    }
    if sum(p.get(..0xf)?) != 0 {
        return None;
    }

    Some(hdr)
}

/// The structure table of `e`.
fn smbios_table(e: &SmbiosEntry) -> &[u8] {
    if e.addr.is_null() {
        return &[];
    }
    // SAFETY: `addr` and `len` are the table bios_attach mapped read-only for good (or a
    // test's buffer, alive for the test).
    unsafe { slice::from_raw_parts(e.addr, usize::from(e.len)) }
}

/// The offset in the table of `e` of a pointer into it.
fn smbios_off<T>(e: &SmbiosEntry, p: *const T) -> Option<usize> {
    let (base, q) = (e.addr as usize, p as usize);
    (!e.addr.is_null() && q >= base && q < base + usize::from(e.len)).then(|| q - base)
}

/// The formatted area of the structure `st` as a `T` (see the module's deviations).
fn smbios_struct<T: SmbiosPod>(e: &SmbiosEntry, st: &Smbtable) -> T {
    match smbios_off(e, st.tblhdr) {
        Some(off) => pod_from(&smbios_table(e)[off..]),
        None => T::default(),
    }
}

/// Past the string set that starts at `va`: past its two NULs, or 2 past the table's end.
fn smbios_skip_strings(t: &[u8], mut va: usize) -> usize {
    while va + 1 < t.len() {
        if t[va] == 0 && t[va + 1] == 0 {
            break;
        }
        va += 1;
    }
    va + 2
}

/// `smbios_find_table(type, st)`: takes a caller supplied smbios struct type and a pointer
/// to a handle (`struct smbtable`), returning one if the structure is successfully located
/// and zero otherwise. Callers should take care to initialize the cookie field of the
/// smbtable structure to zero before the first invocation of this function. Multiple tables
/// of the same type can be located by repeatedly calling `smbios_find_table` with the same
/// arguments.
pub fn smbios_find_table(r#type: u8, st: &mut Smbtable) -> i32 {
    // SAFETY: written once by bios_attach, during autoconfiguration, before any caller.
    let e = unsafe { SMBIOS_ENTRY.read() };
    smbios_find_table_in(&e, r#type, st)
}

/// [`smbios_find_table`] over the table of `e`.
pub fn smbios_find_table_in(e: &SmbiosEntry, r#type: u8, st: &mut Smbtable) -> i32 {
    let t = smbios_table(e);
    let end = t.len();
    let at = |i: usize| t.get(i).copied().unwrap_or(0);
    let mut va = 0;
    let mut ret = 0;
    let mut tcount: u32 = 1;

    // The cookie field of the smtable structure is used to locate multiple instances of a
    // table of an arbitrary type. Following the successful location of a table, the type is
    // encoded as bits 0:7 of the cookie value, the offset in terms of the number of
    // structures preceding that referenced by the handle is encoded in bits 15:31.
    if (st.cookie & 0xfff) == u32::from(r#type)
        && st.cookie >> 16 != 0
        && let Some(hdr) = smbios_off(e, st.hdr)
        && at(hdr) == r#type
    {
        va = smbios_skip_strings(t, hdr + usize::from(at(hdr + 1)));
        tcount = st.cookie >> 16;
    }
    while va + size_of::<Smbtblhdr>() < end && tcount <= u32::from(e.count) {
        let hdr = va;
        if at(hdr) == r#type {
            ret = 1;
            st.hdr = e.addr.wrapping_add(hdr).cast();
            st.tblhdr = e.addr.wrapping_add(va + size_of::<Smbtblhdr>()).cast();
            st.cookie = ((tcount + 1) << 16) | u32::from(r#type);
            break;
        }
        if at(hdr) == SMBIOS_TYPE_EOT {
            break;
        }
        va = smbios_skip_strings(t, va + usize::from(at(hdr + 1)));
        tcount += 1;
    }
    ret
}

/// `smbios_get_string(st, indx, dest, len)`: the string `indx` (from 1) of the structure
/// `st`, `dest.len()` bytes of the table from its start, NUL-terminated, into `dest`; `None`
/// when there is no such string or it lies within `dest.len()` bytes of the table's end.
pub fn smbios_get_string<'a>(st: &Smbtable, indx: u8, dest: &'a mut [u8]) -> Option<&'a mut [u8]> {
    // SAFETY: as in `smbios_find_table`.
    let e = unsafe { SMBIOS_ENTRY.read() };
    smbios_get_string_in(&e, st, indx, dest)
}

/// [`smbios_get_string`] over the table of `e`.
pub fn smbios_get_string_in<'a>(
    e: &SmbiosEntry,
    st: &Smbtable,
    indx: u8,
    dest: &'a mut [u8],
) -> Option<&'a mut [u8]> {
    let t = smbios_table(e);
    let end = t.len();
    let len = dest.len();
    let hdr = smbios_off(e, st.hdr)?;

    let mut va = hdr + usize::from(t.get(hdr + 1).copied().unwrap_or(0));
    let mut i = 1;
    while va < end && i < indx && t[va] != 0 {
        while va < end && t[va] != 0 {
            va += 1;
        }
        va += 1;
        i += 1;
    }
    if i == indx && len != 0 && va + len < end {
        dest.copy_from_slice(&t[va..va + len]);
        dest[len - 1] = 0;
        return Some(dest);
    }

    None
}

/// `fixstring(s)`: `None` for an uninformative string (`smbios_uninfo[]`) or one of blanks
/// only; else the string with its leading and trailing blanks removed in place.
pub fn fixstring(s: &mut [u8]) -> Option<&[u8]> {
    for u in SMBIOS_UNINFO {
        if strncasecmp(s, u, u.len()) == 0 {
            return None;
        }
    }
    let len = strnlen(s, s.len());
    // Remove leading and trailing whitespace
    let p = s[..len].iter().take_while(|&&c| c == b' ').count();
    // Special case entire string is whitespace
    if p == len {
        return None;
    }
    let mut e = len - 1;
    while e > 0 && s[e] == b' ' {
        e -= 1;
    }
    if p > 0 || e < len - 1 {
        s.copy_within(p..=e, 0);
        s[e - p + 1] = 0;
    }

    let n = strnlen(s, s.len());
    Some(&s[..n])
}

/// The string `indx` of `st`, fixed (`smbios_get_string` into `sminfo[64]`, then
/// `fixstring`).
fn smbios_fixed(e: &SmbiosEntry, st: &Smbtable, indx: u8) -> Option<SmbiosString> {
    let mut sminfo = [0u8; 64];
    let s = fixstring(smbios_get_string_in(e, st, indx, &mut sminfo)?)?;
    let mut out = [0u8; 64];
    strlcpy(&mut out, s);
    Some(SmbiosString(out))
}

/// The strings `smbios_info` stores, from the table of `e`: `None` before SMBIOS 2 or
/// without the system information, which the specification requires among others (if it
/// is not there we do not bother with this smbios implementation).
pub fn smbios_info_strings(e: &SmbiosEntry) -> Option<SmbiosInfo> {
    if e.mjr < 2 {
        return None;
    }
    let (mut stbl, mut btbl) = (Smbtable::default(), Smbtable::default());
    if smbios_find_table_in(e, SMBIOS_TYPE_SYSTEM, &mut stbl) == 0 {
        return None;
    }
    let havebb = smbios_find_table_in(e, SMBIOS_TYPE_BASEBOARD, &mut btbl) != 0;

    let sys: SmbiosSys = smbios_struct(e, &stbl);
    let board: SmbiosBoard = smbios_struct(e, &btbl);
    let mut info = SmbiosInfo::default();
    if havebb {
        info.board_vendor = smbios_fixed(e, &btbl, board.vendor);
        info.board_prod = smbios_fixed(e, &btbl, board.product);
        info.board_serial = smbios_fixed(e, &btbl, board.serial);
    }
    // Some smbios implementations have no system vendor or product strings, some have very
    // uninformative data which is harder to work around and we must rely upon various
    // heuristics to detect this. In both cases we attempt to fall back on the base board
    // information in the perhaps naive belief that motherboard vendors will supply this
    // information.
    info.vendor = smbios_fixed(e, &stbl, sys.vendor).or_else(|| {
        havebb
            .then(|| smbios_fixed(e, &btbl, board.vendor))
            .flatten()
    });
    info.prod = smbios_fixed(e, &stbl, sys.product).or_else(|| {
        havebb
            .then(|| smbios_fixed(e, &btbl, board.product))
            .flatten()
    });
    info.ver = smbios_fixed(e, &stbl, sys.version);
    info.serial = smbios_fixed(e, &stbl, sys.serial);
    if e.mjr > 2 || (e.mjr == 2 && e.min >= 1) {
        // If the uuid value is all 0xff the uuid is present but not set, if its all 0 then
        // the uuid isn't present at all.
        let uuid = sys.uuid;
        let mut uuidf = SMBIOS_UUID_NPRESENT | SMBIOS_UUID_NSET;
        for b in uuid {
            if b != 0xff {
                uuidf &= !SMBIOS_UUID_NSET;
            }
            if b != 0 {
                uuidf &= !SMBIOS_UUID_NPRESENT;
            }
        }

        info.uuid = if uuidf & SMBIOS_UUID_NPRESENT != 0 {
            SmbiosUuid::NotPresent
        } else if uuidf & SMBIOS_UUID_NSET != 0 {
            SmbiosUuid::NotSet
        } else {
            SmbiosUuid::Set(uuid)
        };
    }

    Some(info)
}

/// `infolen = strlen(s) + 1; v = malloc(infolen, M_DEVBUF, M_NOWAIT); strlcpy(v, s,
/// infolen)`: a copy of `s` that lives as long as the kernel, `None` when memory is short.
fn smbios_kept(s: &[u8]) -> Option<&'static [u8]> {
    let infolen = strnlen(s, s.len()) + 1;
    let buf = malloc(infolen, M_DEVBUF, M_NOWAIT)?;
    // SAFETY: malloc returned `infolen` bytes that nothing else references; they are never
    // freed, as the C never frees what hw_vendor and the others point at.
    let dst: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(buf.as_ptr(), infolen) };
    strlcpy(dst, s);
    Some(dst)
}

/// `SMBIOS_UUID_REP` of `u` into `dst` (`SMBIOS_UUID_REPLEN` bytes with the NUL).
pub fn smbios_uuid_rep(u: &[u8; 16], dst: &mut [u8]) {
    let _ = snprintf(
        dst,
        format_args!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u[0],
            u[1],
            u[2],
            u[3],
            u[4],
            u[5],
            u[6],
            u[7],
            u[8],
            u[9],
            u[10],
            u[11],
            u[12],
            u[13],
            u[14],
            u[15]
        ),
    );
}

/// `smbios_info(str)`: the board's strings, `hw_vendor`, `hw_prod` (printed after `str`,
/// bios0's name, when both are known), `hw_ver`, `hw_serial` and `hw_uuid`.
pub fn smbios_info(str: &str) {
    // SAFETY: as in `smbios_find_table`.
    let e = unsafe { SMBIOS_ENTRY.read() };
    let Some(info) = smbios_info_strings(&e) else {
        return;
    };

    // SAFETY: every access below: bios0 attaches once, during autoconfiguration on the boot
    // CPU, before any process can read the hw.* sysctls and before any other reader of these
    // globals runs; the buffer of the UUID is fresh from malloc and kept, as in `smbios_kept`.
    unsafe {
        if let Some(s) = info.board_vendor {
            strlcpy(SMBIOS_BOARD_VENDOR.get_mut(), s.as_bytes());
        }
        if let Some(s) = info.board_prod {
            strlcpy(SMBIOS_BOARD_PROD.get_mut(), s.as_bytes());
        }
        if let Some(s) = info.board_serial {
            strlcpy(SMBIOS_BOARD_SERIAL.get_mut(), s.as_bytes());
        }

        if let Some(s) = info.vendor {
            hw_vendor.write(smbios_kept(s.as_bytes()));
        }
        if let Some(s) = info.prod {
            hw_prod.write(smbios_kept(s.as_bytes()));
        }
        if let (Some(v), Some(p)) = (hw_vendor.read(), hw_prod.read()) {
            kprintf!("\n{}: {} {}", str, Str(v), Str(p));
        }
        if let Some(s) = info.ver {
            hw_ver.write(smbios_kept(s.as_bytes()));
        }
        if let Some(s) = info.serial {
            for &c in s.as_bytes() {
                enqueue_randomness(u32::from(c));
            }
            hw_serial.write(smbios_kept(s.as_bytes()));
        }
        match info.uuid {
            SmbiosUuid::Unread => {}
            SmbiosUuid::NotPresent => hw_uuid.write(None),
            SmbiosUuid::NotSet => hw_uuid.write(Some(&b"Not Set"[..])),
            SmbiosUuid::Set(u) => {
                for b in u {
                    enqueue_randomness(u32::from(b));
                }
                hw_uuid.write(malloc(SMBIOS_UUID_REPLEN, M_DEVBUF, M_NOWAIT).map(|buf| {
                    let dst: &'static mut [u8] =
                        slice::from_raw_parts_mut(buf.as_ptr(), SMBIOS_UUID_REPLEN);
                    smbios_uuid_rep(&u, dst);
                    &*dst
                }));
            }
        }
    }
}

/// The first `pat` at or after `from` in `area`, wholly inside it.
fn soekris_find(area: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    (from..(area.len() + 1).saturating_sub(pat.len())).find(|&p| area[p..].starts_with(pat))
}

/// The Soekris comBIOS scan of the BIOS area `area` (`SMBIOS_START` to `SMBIOS_END`):
/// `hw_vendor` from its signature and, after it, `hw_prod` from "net6501", as that's the only
/// Soekris platform that can run amd64.
#[cfg(target_os = "none")]
fn soekris_info(area: &[u8]) {
    let Some(p) = soekris_find(area, 0, SOEKRIS_SIGNATURE) else {
        return;
    };
    // SAFETY: as in `smbios_info`: bios0's attach, before any reader of hw_vendor.
    unsafe { hw_vendor.write(smbios_kept(SOEKRIS_SIGNATURE)) };
    // SAFETY: as above.
    if unsafe { hw_vendor.read() }.is_none() {
        return;
    }
    // Search only for "net6501" in the comBIOS
    if let Some(q) = soekris_find(area, p + SOEKRIS_SIGNATURE.len(), b"net6501") {
        // SAFETY: as above.
        unsafe { hw_prod.write(smbios_kept(&area[q..q + 7])) };
    }
}

/// `bios_print(aux, pnp)`: names a child that found no driver.
#[cfg(target_os = "none")]
pub fn bios_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: bios0 hands its children `struct bios_attach_args`.
    let ba = unsafe { &*aux.cast_const().cast::<BiosAttachArgs>() };

    if let Some(pnp) = pnp {
        kprintf!("{} at {}", Str(ba.ba_name), Str(pnp));
    }
    UNCONF
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::super::super::include::smbiosvar::{SMBIOS_TYPE_IPMIDEV, SMBIOS_TYPE_MEMDEV};
    use super::*;
    use std::vec::Vec;

    /// One structure: its header, formatted area and string set.
    fn structure(r#type: u8, handle: u16, formatted: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut v = std::vec![r#type, (4 + formatted.len()) as u8];
        v.extend_from_slice(&handle.to_le_bytes());
        v.extend_from_slice(formatted);
        for s in strings {
            v.extend_from_slice(s.as_bytes());
            v.push(0);
        }
        if strings.is_empty() {
            v.push(0);
        }
        v.push(0);
        v
    }

    /// Type 1, the system information, with these string indices and this UUID.
    fn system(vendor: u8, product: u8, version: u8, serial: u8, uuid: [u8; 16]) -> Vec<u8> {
        let mut f = std::vec![vendor, product, version, serial];
        f.extend_from_slice(&uuid);
        f.extend_from_slice(&[6, 0, 0]); // wakeup: power switch; no SKU, no family
        f
    }

    /// The table OVMF hands a q35 guest (type 0 last, as there), its structure count.
    fn qemu_table(uuid: [u8; 16], with_ipmi: bool) -> (Vec<u8>, u16) {
        let mut t = Vec::new();
        t.extend(structure(
            SMBIOS_TYPE_SYSTEM,
            0x100,
            &system(1, 2, 3, 0, uuid),
            &["QEMU", "Standard PC (Q35 + ICH9, 2009)", "pc-q35-11.1"],
        ));
        t.extend(structure(
            3,
            0x300,
            &[1, 1, 2, 0, 0],
            &["QEMU", "pc-q35-11.1"],
        ));
        t.extend(structure(
            SMBIOS_TYPE_MEMDEV,
            0x1100,
            &[0; 17],
            &["DIMM 0", "QEMU"],
        ));
        let mut n = 3;
        if with_ipmi {
            // KCS, IPMI 2.0, BMC at 0x20, I/O port 0xca2 (BAR format), byte spacing.
            let mut f = std::vec![1, 0x20, 0x20, 0];
            f.extend_from_slice(&0xca3u64.to_le_bytes());
            f.extend_from_slice(&[0, 0]);
            t.extend(structure(SMBIOS_TYPE_IPMIDEV, 0x2600, &f, &[]));
            n += 1;
        }
        let mut bios = std::vec![1, 2, 0x00, 0xe8, 3, 0];
        bios.extend_from_slice(&[0; 12]); // characteristics, extensions
        bios.extend_from_slice(&[0, 0, 0xff, 0xff]); // releases
        t.extend(structure(
            SMBIOS_TYPE_BIOS,
            0,
            &bios,
            &["EFI Development Kit II / OVMF", "0.0.0", "02/06/2015"],
        ));
        t.extend(structure(SMBIOS_TYPE_EOT, 0x7f00, &[], &[]));
        (t, n + 2)
    }

    /// The handle of the structure `st` found.
    fn handle(st: &Smbtable) -> u16 {
        // SAFETY: `hdr` points into the test's table.
        let h = unsafe { st.hdr.read_unaligned() };
        h.handle
    }

    fn entry(t: &[u8], mjr: u8, min: u8, count: u16) -> SmbiosEntry {
        SmbiosEntry {
            mjr,
            min,
            addr: t.as_ptr(),
            len: t.len() as u16,
            count,
        }
    }

    fn fixed(s: &str) -> Option<std::string::String> {
        let mut b = [0u8; 64];
        b[..s.len()].copy_from_slice(s.as_bytes());
        fixstring(&mut b).map(|f| std::string::String::from_utf8_lossy(f).into_owned())
    }

    fn string(s: &Option<SmbiosString>) -> Option<&str> {
        s.as_ref()
            .map(|s| core::str::from_utf8(s.as_bytes()).unwrap_or("?"))
    }

    /// An SMBIOS 2.8 entry point with both checksums right.
    fn entry_point(addr: u32, size: u16, count: u16) -> [u8; 0x1f] {
        let mut p = [0u8; 0x1f];
        p[..4].copy_from_slice(b"_SM_");
        p[5] = 0x1f;
        p[6] = 2;
        p[7] = 8;
        p[0x10..0x15].copy_from_slice(b"_DMI_");
        p[0x16..0x18].copy_from_slice(&size.to_le_bytes());
        p[0x18..0x1c].copy_from_slice(&addr.to_le_bytes());
        p[0x1c..0x1e].copy_from_slice(&count.to_le_bytes());
        p[0x1e] = 0x28;
        let sum = |b: &[u8]| b.iter().fold(0u8, |c, &x| c.wrapping_add(x));
        p[0x15] = 0u8.wrapping_sub(sum(&p[0x10..0x1f]));
        p[4] = 0u8.wrapping_sub(sum(&p));
        p
    }

    #[test]
    fn entry_point_needs_both_anchors_and_checksums() {
        assert_eq!(SMBIOS_SIGNATURE, u32::from_le_bytes(*b"_SM_"));
        let p = entry_point(0x3f58_7000, 0x1a3, 9);
        let hdr = smbios_find(&p).expect("a valid entry point");
        let (addr, size, count, majrev, minrev) =
            (hdr.addr, hdr.size, hdr.count, hdr.majrev, hdr.minrev);
        assert_eq!(
            (addr, size, count, majrev, minrev),
            (0x3f58_7000, 0x1a3, 9, 2, 8)
        );

        // In a longer area, at its start only.
        let mut area = std::vec![0u8; 64];
        area[16..16 + 0x1f].copy_from_slice(&p);
        assert!(smbios_find(&area).is_none());
        assert!(smbios_find(&area[16..]).is_some());

        for (i, v) in [(0, b'X'), (4, p[4] ^ 1), (0x12, b'X'), (0x15, p[0x15] ^ 1)] {
            let mut q = p;
            q[i] = v;
            assert!(smbios_find(&q).is_none(), "byte {i:#x}");
        }
        // An SMBIOS 3 anchor is not one, as in the C (amd64's bios.c reads `_SM_` only).
        let mut q = p;
        q[..5].copy_from_slice(b"_SM3_");
        assert!(smbios_find(&q).is_none());
        // Cut short by the end of the area.
        assert!(smbios_find(&p[..0x1e]).is_none());
        assert!(smbios_find(&[]).is_none());
    }

    #[test]
    fn find_table_walks_types_and_resumes_from_the_cookie() {
        let mut t = Vec::new();
        t.extend(structure(SMBIOS_TYPE_MEMDEV, 0x1100, &[0; 17], &["DIMM 0"]));
        t.extend(structure(
            SMBIOS_TYPE_SYSTEM,
            0x100,
            &system(1, 0, 0, 0, [0; 16]),
            &["A"],
        ));
        t.extend(structure(SMBIOS_TYPE_MEMDEV, 0x1101, &[0; 17], &["DIMM 1"]));
        t.extend(structure(SMBIOS_TYPE_EOT, 0x7f00, &[], &[]));
        t.extend(structure(
            SMBIOS_TYPE_MEMDEV,
            0x1102,
            &[0; 17],
            &["after the end"],
        ));
        let e = entry(&t, 2, 8, 5);

        let mut st = Smbtable::default();
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_MEMDEV, &mut st), 1);
        assert_eq!(st.hdr.cast::<u8>(), t.as_ptr());
        assert_eq!(st.tblhdr.cast::<u8>(), t.as_ptr().wrapping_add(4));
        assert_eq!(st.cookie, (2 << 16) | u32::from(SMBIOS_TYPE_MEMDEV));
        assert_eq!(handle(&st), 0x1100);

        // The same type again: the next one, then none (the end-of-table structure stops it).
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_MEMDEV, &mut st), 1);
        assert_eq!(handle(&st), 0x1101);
        assert_eq!(st.cookie >> 16, 4);
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_MEMDEV, &mut st), 0);

        // A cookie of another type starts over.
        let mut sys = Smbtable::default();
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_SYSTEM, &mut sys), 1);
        let mut again = sys;
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_MEMDEV, &mut again), 1);
        assert_eq!(handle(&again), 0x1100);

        // The count bounds the walk; an empty table has nothing.
        let mut st = Smbtable::default();
        assert_eq!(
            smbios_find_table_in(&entry(&t, 2, 8, 2), SMBIOS_TYPE_MEMDEV, &mut st),
            1
        );
        assert_eq!(
            smbios_find_table_in(&entry(&t, 2, 8, 2), SMBIOS_TYPE_MEMDEV, &mut st),
            0
        );
        assert_eq!(
            smbios_find_table_in(
                &SmbiosEntry::new(),
                SMBIOS_TYPE_BIOS,
                &mut Smbtable::default()
            ),
            0
        );
    }

    #[test]
    fn strings_by_index_and_the_end_of_table_rule() {
        let (t, n) = qemu_table([0; 16], false);
        let e = entry(&t, 2, 8, n);
        let mut sys = Smbtable::default();
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_SYSTEM, &mut sys), 1);

        let mut d = [0u8; 64];
        let s = smbios_get_string_in(&e, &sys, 2, &mut d).expect("string 2");
        assert_eq!(&s[..strnlen(s, 64)], b"Standard PC (Q35 + ICH9, 2009)");
        assert!(smbios_get_string_in(&e, &sys, 0, &mut d).is_none());
        // One past the last string is the empty string at the set's end, as in the C (which
        // `fixstring` then rejects); two past is none.
        let s = smbios_get_string_in(&e, &sys, 4, &mut d).expect("the set's end");
        assert_eq!(s[0], 0);
        assert!(fixstring(s).is_none());
        assert!(smbios_get_string_in(&e, &sys, 5, &mut d).is_none());

        // OVMF's type 0 is last: its strings are within 64 bytes of the end, so none is
        // given (OpenBSD 8.0 prints a bare "bios0:" there), but a smaller buffer gets them.
        let mut bios = Smbtable::default();
        assert_eq!(smbios_find_table_in(&e, SMBIOS_TYPE_BIOS, &mut bios), 1);
        let sb: SmbiosStructBios = smbios_struct(&e, &bios);
        let (vendor, version, release) = (sb.vendor, sb.version, sb.release);
        assert_eq!((vendor, version, release), (1, 2, 3));
        for i in [vendor, version, release] {
            assert!(smbios_get_string_in(&e, &bios, i, &mut d).is_none());
        }
        let mut small = [0u8; 8];
        let s = smbios_get_string_in(&e, &bios, release, &mut small).expect("date");
        assert_eq!(&s[..], b"02/06/2\0");
    }

    #[test]
    fn fixstring_trims_and_rejects_placeholders() {
        assert_eq!(fixed("QEMU").as_deref(), Some("QEMU"));
        assert_eq!(fixed("  Standard PC  ").as_deref(), Some("Standard PC"));
        assert_eq!(fixed("pc-q35-11.1   ").as_deref(), Some("pc-q35-11.1"));
        assert_eq!(fixed("   x").as_deref(), Some("x"));
        assert_eq!(fixed("To be filled by O.E.M.").as_deref(), None);
        assert_eq!(fixed("TO BE FILLED").as_deref(), None);
        assert_eq!(fixed("System Product Name").as_deref(), None);
        assert_eq!(fixed("Not Specified").as_deref(), None);
        assert_eq!(fixed("SYS-1234").as_deref(), None);
        assert_eq!(fixed("Nothing").as_deref(), Some("Nothing"));
        assert_eq!(fixed("    ").as_deref(), None);
        assert_eq!(fixed("").as_deref(), None);
        // A buffer with no NUL is a string as long as the buffer.
        let mut b = [b'a'; 4];
        assert_eq!(fixstring(&mut b), Some(&b"aaaa"[..]));
    }

    #[test]
    fn info_of_qemu_and_the_board_fallback() {
        let (t, n) = qemu_table([0; 16], true);
        let info = smbios_info_strings(&entry(&t, 2, 8, n)).expect("system information");
        assert_eq!(string(&info.vendor), Some("QEMU"));
        assert_eq!(string(&info.prod), Some("Standard PC (Q35 + ICH9, 2009)"));
        assert_eq!(string(&info.ver), Some("pc-q35-11.1"));
        assert_eq!(string(&info.serial), None);
        assert_eq!(info.uuid, SmbiosUuid::NotPresent);
        assert!(info.board_vendor.is_none());

        // The UUID: all 0xff is "Not Set"; read only from SMBIOS 2.1 on.
        let (t, n) = qemu_table([0xff; 16], false);
        let info = smbios_info_strings(&entry(&t, 2, 8, n)).expect("system information");
        assert_eq!(info.uuid, SmbiosUuid::NotSet);
        let u: [u8; 16] = core::array::from_fn(|i| i as u8 * 0x11);
        let (t, n) = qemu_table(u, false);
        assert_eq!(
            smbios_info_strings(&entry(&t, 2, 8, n)).map(|i| i.uuid),
            Some(SmbiosUuid::Set(u))
        );
        assert_eq!(
            smbios_info_strings(&entry(&t, 2, 0, n)).map(|i| i.uuid),
            Some(SmbiosUuid::Unread)
        );
        assert!(smbios_info_strings(&entry(&t, 1, 9, n)).is_none());

        // Placeholder system strings fall back on the base board's.
        let mut t = Vec::new();
        t.extend(structure(
            SMBIOS_TYPE_SYSTEM,
            0x100,
            &system(1, 2, 0, 3, [0; 16]),
            &["To Be Filled By O.E.M.", "System Product Name", "  S123  "],
        ));
        t.extend(structure(
            SMBIOS_TYPE_BASEBOARD,
            0x200,
            &[1, 2, 0, 3, 0, 0, 0, 0, 0, 0, 0],
            &["ACME", "Board 9", "B-77"],
        ));
        t.extend(structure(SMBIOS_TYPE_EOT, 0x7f00, &[], &[]));
        // Room after the end: the last strings are within 64 bytes of it otherwise.
        t.extend_from_slice(&[0; 64]);
        let info = smbios_info_strings(&entry(&t, 2, 8, 3)).expect("system information");
        assert_eq!(string(&info.vendor), Some("ACME"));
        assert_eq!(string(&info.prod), Some("Board 9"));
        assert_eq!(string(&info.serial), Some("S123"));
        assert_eq!(string(&info.ver), None);
        assert_eq!(string(&info.board_vendor), Some("ACME"));
        assert_eq!(string(&info.board_prod), Some("Board 9"));
        assert_eq!(string(&info.board_serial), Some("B-77"));

        // No system information: nothing.
        let mut t = structure(SMBIOS_TYPE_BASEBOARD, 0x200, &[1; 11], &["ACME"]);
        t.extend(structure(SMBIOS_TYPE_EOT, 0x7f00, &[], &[]));
        assert!(smbios_info_strings(&entry(&t, 2, 8, 2)).is_none());
    }

    #[test]
    fn uuid_representation() {
        let u: [u8; 16] = core::array::from_fn(|i| i as u8 * 0x11);
        let mut b = [0u8; SMBIOS_UUID_REPLEN];
        smbios_uuid_rep(&u, &mut b);
        assert_eq!(&b[..36], b"00112233-4455-6677-8899-aabbccddeeff");
        assert_eq!(b[36], 0);
    }

    #[test]
    fn soekris_signature_then_product() {
        let mut area = std::vec![0u8; 256];
        area[10..17].copy_from_slice(b"net6501");
        area[40..40 + SOEKRIS_SIGNATURE.len()].copy_from_slice(SOEKRIS_SIGNATURE);
        area[100..107].copy_from_slice(b"net6501");
        let p = soekris_find(&area, 0, SOEKRIS_SIGNATURE).expect("signature");
        assert_eq!(p, 40);
        assert_eq!(
            soekris_find(&area, p + SOEKRIS_SIGNATURE.len(), b"net6501"),
            Some(100)
        );
        // At the very end of the area, and cut short by it.
        let n = area.len();
        area[n - 7..].copy_from_slice(b"net6501");
        assert_eq!(soekris_find(&area, 101, b"net6501"), Some(n - 7));
        assert_eq!(soekris_find(&area[..n - 1], 101, b"net6501"), None);
        assert_eq!(soekris_find(&[], 0, SOEKRIS_SIGNATURE), None);
    }
}
/* </TESTS> */
