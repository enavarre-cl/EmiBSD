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
//! What the boot glue hands to the machine and the kernel: a bootloader-neutral view of the
//! loaded image and of physical memory.
//!
//! `sys/stand/` fills it from the Limine responses, and the machine's `getbootinfo` from
//! OpenBSD boot(8)'s hand-over (M14, `sys/stand/bootarg.rs`); nothing here names either. Grows with the milestones (M3 adds
//! what `uvm_page` needs; M11a the processors and the way to start them, [`BootMp`]).

use core::ffi::CStr;
use core::ptr::NonNull;

use crate::sys::reboot::{RB_ASKNAME, RB_CONFIG, RB_KDB, RB_SINGLE};
use crate::sys::types::{Paddr, Psize, Vaddr};

/// Upper bound on memory map regions kept in [`MemMap`]; boot fails loudly beyond it.
pub const MAX_REGIONS: usize = 256;

/// The most boot modules the glue keeps (`init` and the ramdisk image so far).
pub const MAX_MODULES: usize = 4;

/// A file the bootloader loaded next to the kernel (`module_path:` in `limine.conf`): the
/// `init` the kernel execs until there is a filesystem (M6) and the root file system image
/// of rd(4) (`ramdisk.ffs`, M8).
#[derive(Clone, Copy, Debug)]
pub struct BootModule {
    /// The path it was loaded from (`/init`).
    pub path: &'static CStr,
    /// The string given with it (`module_string:` in `limine.conf`), possibly empty.
    pub string: &'static CStr,
    /// Its contents, mapped for the kernel's lifetime.
    pub data: &'static [u8],
    /// The same bytes, writable: the bootloader maps modules read-write in its direct map.
    /// rd(4) takes its image through this pointer and writes to it, so a module handed to
    /// rd(4) is not read through `data` afterwards.
    pub base: *mut u8,
}

// SAFETY: a module is memory the bootloader handed over for the kernel's lifetime; `base` is
// only an address of it, and whoever writes through it (rd(4)) owns those bytes alone.
unsafe impl Send for BootModule {}
// SAFETY: as for `Send`: shared views read the path, string and length, never `base`'s bytes.
unsafe impl Sync for BootModule {}

/// What a region of physical memory holds, as the bootloader reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemKind {
    /// Free RAM.
    Usable,
    /// Reserved by firmware or hardware; never touched.
    Reserved,
    /// ACPI tables and AML; reclaimable once that data is no longer needed.
    AcpiReclaimable,
    /// ACPI non-volatile storage.
    AcpiNvs,
    /// Unreliable RAM.
    BadMemory,
    /// Bootloader data still in use (responses, page tables, the boot stack); reclaimable later.
    BootloaderReclaimable,
    /// The loaded kernel image and modules.
    KernelAndModules,
    /// A memory-mapped framebuffer.
    Framebuffer,
    /// Reserved, but mapped by the bootloader (ACPI tables, EFI runtime services).
    ReservedMapped,
    /// A type this kernel does not know; the raw value is kept.
    Unknown(u64),
}

/// One region of the physical memory map.
#[derive(Clone, Copy, Debug)]
pub struct MemRegion {
    /// First byte of the region.
    pub base: Paddr,
    /// Size in bytes.
    pub length: Psize,
    /// What the region holds.
    pub kind: MemKind,
}

/// The physical memory map, in the order the bootloader delivers it (sorted by base address).
#[derive(Clone)]
pub struct MemMap {
    regions: [MemRegion; MAX_REGIONS],
    count: usize,
}

impl MemMap {
    const EMPTY: MemRegion = MemRegion {
        base: Paddr(0),
        length: Psize(0),
        kind: MemKind::Unknown(0),
    };

    /// An empty map.
    pub const fn new() -> Self {
        Self {
            regions: [Self::EMPTY; MAX_REGIONS],
            count: 0,
        }
    }

    /// Appends a region; `false` when the map is full.
    pub fn push(&mut self, region: MemRegion) -> bool {
        if self.count == MAX_REGIONS {
            return false;
        }
        self.regions[self.count] = region;
        self.count += 1;
        true
    }

    /// The regions recorded so far.
    pub fn regions(&self) -> &[MemRegion] {
        &self.regions[..self.count]
    }

    /// Number of regions.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether no region was recorded.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Bytes in [`MemKind::Usable`] regions.
    pub fn usable_bytes(&self) -> usize {
        self.regions()
            .iter()
            .filter(|r| r.kind == MemKind::Usable)
            .map(|r| r.length.as_usize())
            .sum()
    }
}

impl Default for MemMap {
    fn default() -> Self {
        Self::new()
    }
}

/// The UEFI memory map the firmware returned before the bootloader exited its boot services
/// (what OpenBSD's efiboot passes as `openbsd,uefi-mmap-*` in `/chosen`).
#[derive(Clone, Copy, Debug)]
pub struct EfiMemmap {
    /// The descriptors, `desc_size` bytes apart, in bootloader memory the kernel copies
    /// before reclaiming any.
    pub map: &'static [u8],
    /// Size of one descriptor in bytes.
    pub desc_size: u32,
    /// The descriptors' version (`EFI_MEMORY_DESCRIPTOR_VERSION`).
    pub desc_ver: u32,
}

/// The linear frame buffer the firmware set up (the UEFI GOP mode the bootloader left
/// active): what OpenBSD's efiboot passes in `bios_efiinfo` (`fb_*`) on amd64 and as a
/// `simple-framebuffer` node of `/chosen` on arm64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootFramebuffer {
    /// Physical address of the first pixel.
    pub paddr: Paddr,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per scan line.
    pub pitch: u32,
    /// Bits per pixel.
    pub bpp: u16,
    /// Bits of red.
    pub red_size: u8,
    /// The lowest bit of red.
    pub red_shift: u8,
    /// Bits of green.
    pub green_size: u8,
    /// The lowest bit of green.
    pub green_shift: u8,
    /// Bits of blue.
    pub blue_size: u8,
    /// The lowest bit of blue.
    pub blue_shift: u8,
}

impl BootFramebuffer {
    /// The mask of a channel of `size` bits from bit `shift` (`fb_red_mask` and friends).
    pub const fn mask(size: u8, shift: u8) -> u32 {
        if size == 0 {
            0
        } else {
            (u32::MAX >> (32 - size as u32)) << shift
        }
    }

    /// Size in bytes: `height` lines of `pitch` bytes.
    pub const fn size(&self) -> usize {
        self.height as usize * self.pitch as usize
    }
}

/// One processor the bootloader found, the boot processor included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootCpu {
    /// The bootloader's number for it (the ACPI processor UID where there is ACPI).
    pub processor_id: u32,
    /// Its hardware ID: the local APIC ID on amd64, `MPIDR_EL1`'s affinity fields on arm64.
    pub hwid: u64,
}

/// The processors and how to start them (`MULTIPROCESSOR`): the bootloader parked every
/// application processor, and [`BootMp::start`] releases one into [`Cpu::cpu_hatch`]. This
/// replaces amd64's `mptramp.S` with its INIT/SIPI sequence and arm64's PSCI `CPU_ON` call
/// (`docs/ARCHITECTURE.md`, "Deviations").
///
/// [`Cpu::cpu_hatch`]: crate::machine::Cpu::cpu_hatch
#[derive(Clone, Copy)]
pub struct BootMp {
    /// The boot processor's hardware ID (as [`BootCpu::hwid`]).
    pub bsp_hwid: u64,
    /// How many processors there are, the boot processor included.
    pub ncpus: usize,
    /// Processor `i`, `i < ncpus`.
    pub cpu: fn(usize) -> BootCpu,
    /// Releases application processor `i` into `Cpu::cpu_hatch(arg)`, on a bootloader stack
    /// of 64 KiB with interrupts masked. Returns at once; the processor runs from then on.
    ///
    /// # Safety
    ///
    /// `i < ncpus`, `i` is not the boot processor and was not started before, and `arg` is
    /// what the machine's `cpu_hatch` expects for that processor.
    pub start: unsafe fn(usize, usize),
}

impl BootMp {
    /// The processors, the boot processor included, in the bootloader's order.
    pub fn cpus(&self) -> impl Iterator<Item = BootCpu> + '_ {
        (0..self.ncpus).map(|i| (self.cpu)(i))
    }

    /// The index of the processor whose hardware ID is `hwid`.
    pub fn index_of(&self, hwid: u64) -> Option<usize> {
        self.cpus().position(|c| c.hwid == hwid)
    }
}

impl core::fmt::Debug for BootMp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BootMp")
            .field("bsp_hwid", &self.bsp_hwid)
            .field("ncpus", &self.ncpus)
            .finish_non_exhaustive()
    }
}

/// Facts about the loaded image and the machine, gathered by the boot glue before anything else
/// runs.
pub struct BootInfo {
    /// Name of the bootloader, `"unknown"` if it did not say.
    pub bootloader_name: &'static CStr,
    /// Version of the bootloader, `"unknown"` if it did not say.
    pub bootloader_version: &'static CStr,
    /// The kernel command line the bootloader was given (`boot(8)`-style flags such as `-d`),
    /// empty if none.
    pub cmdline: &'static CStr,
    /// Offset of the higher-half direct map: physical address `p` is mapped at `p + hhdm_offset`
    /// for every region the bootloader chose to map.
    pub hhdm_offset: usize,
    /// Physical address the kernel image was loaded at.
    pub kernel_phys: Paddr,
    /// Virtual address the kernel image was linked (and mapped) at.
    pub kernel_virt: Vaddr,
    /// The ACPI RSDP, as a higher-half virtual address, if the firmware has ACPI.
    pub rsdp: Option<Vaddr>,
    /// The flattened device tree, if the firmware provides one.
    pub dtb: Option<NonNull<u8>>,
    /// The physical memory map.
    pub memmap: MemMap,
    /// Physical address of the UEFI system table, when the machine booted through UEFI (what
    /// OpenBSD's efiboot passes as `openbsd,uefi-system-table`).
    pub efi_system_table: Option<Paddr>,
    /// Physical address of the SMBIOS 2 (`_SM_`) entry point, when the firmware has one (what
    /// OpenBSD's amd64 efiboot passes as `bios_efiinfo->config_smbios`).
    pub smbios: Option<Paddr>,
    /// The UEFI memory map, when the machine booted through UEFI.
    pub efi_memmap: Option<EfiMemmap>,
    /// The firmware's linear frame buffer, when there is a display.
    pub framebuffer: Option<BootFramebuffer>,
    /// The boot modules, in load order (`None` past the last).
    pub modules: [Option<BootModule>; MAX_MODULES],
    /// The processors, when the kernel is built `MULTIPROCESSOR` and the bootloader found them.
    pub mp: Option<BootMp>,
    /// `boothowto` as the boot loader passed it (boot(8)'s `howto` argument on amd64); 0
    /// under Limine, whose flags come from the command line.
    pub howto: i32,
    /// The boot disk's label DUID as the boot loader passed it (`BOOTARG_BOOTDUID`); `None`
    /// under Limine, which names it on the command line (`bootduid=`).
    pub duid: Option<[u8; 8]>,
}

impl BootInfo {
    /// The boot modules.
    pub fn modules(&self) -> impl Iterator<Item = &BootModule> + '_ {
        self.modules.iter().flatten()
    }

    /// The module whose path's last component is `name` (`b"init"` for `/init`).
    pub fn module(&self, name: &[u8]) -> Option<&BootModule> {
        self.modules().find(|m| {
            let path = m.path.to_bytes();
            path.rsplit(|&c| c == b'/').next() == Some(name)
        })
    }

    /// Physical address of a virtual address inside the kernel image.
    pub fn kernel_virt_to_phys(&self, va: Vaddr) -> Paddr {
        Paddr::new(va.as_usize() - self.kernel_virt.as_usize() + self.kernel_phys.as_usize())
    }

    /// The higher-half direct-map alias of a physical address. Only valid for regions the
    /// bootloader mapped (see [`MemKind`]).
    pub fn hhdm(&self, pa: Paddr) -> Vaddr {
        Vaddr::new(pa.as_usize() + self.hhdm_offset)
    }

    /// `boothowto` from the command line, parsed as arm64's `initarm` parses `bootargs`:
    /// everything from the first `-` on is `boot(8)` flag letters (`a` asks for the root
    /// device, `c` enters the device configuration, `d` the debugger, `s` single user). Unknown
    /// letters are ignored here; the C prints them, which needs a console that does not exist
    /// yet at this point.
    pub fn boothowto(&self) -> i32 {
        let bytes = self.cmdline.to_bytes();
        let Some(start) = bytes.iter().position(|&b| b == b'-') else {
            return self.howto;
        };
        let mut howto = self.howto;
        for &c in &bytes[start..] {
            howto |= match c {
                b'a' => RB_ASKNAME,
                b'c' => RB_CONFIG,
                b'd' => RB_KDB,
                b's' => RB_SINGLE,
                _ => 0,
            };
        }
        howto
    }

    /// The boot disk's DUID that boot(8) hands over (`BOOTARG_BOOTDUID` on amd64, the
    /// `openbsd,bootduid` property efiboot puts in the device tree on arm64): under Limine,
    /// a `bootduid=` word of the command line with the 16 hexadecimal digits of the
    /// label's `d_uid`, as `duid_format` prints them. `None` without one or with a malformed
    /// one. Flags go after it: everything from the first `-` on is read as flag letters
    /// ([`BootInfo::boothowto`]), the `d` of a `bootduid=` word included.
    pub fn bootduid(&self) -> Option<[u8; 8]> {
        if self.duid.is_some() {
            return self.duid;
        }
        let word = self
            .cmdline
            .to_bytes()
            .split(|c| c.is_ascii_whitespace())
            .find_map(|w| w.strip_prefix(b"bootduid="))?;
        if word.len() != 16 {
            return None;
        }
        let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let mut duid = [0u8; 8];
        for (i, pair) in word.chunks(2).enumerate() {
            duid[i] = digit(pair[0])? << 4 | digit(pair[1])?;
        }
        Some(duid)
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memmap_push_and_sum() {
        let mut m = MemMap::new();
        assert!(m.is_empty());
        for i in 0..MAX_REGIONS {
            let kind = if i % 2 == 0 {
                MemKind::Usable
            } else {
                MemKind::Reserved
            };
            assert!(m.push(MemRegion {
                base: Paddr::new(i * 0x1000),
                length: Psize::new(0x1000),
                kind,
            }));
        }
        assert_eq!(m.len(), MAX_REGIONS);
        assert!(!m.push(MemRegion {
            base: Paddr::new(0),
            length: Psize::new(0),
            kind: MemKind::Usable,
        }));
        assert_eq!(m.usable_bytes(), MAX_REGIONS / 2 * 0x1000);
        assert_eq!(m.regions()[1].kind, MemKind::Reserved);
    }

    #[test]
    fn address_translation() {
        let boot = BootInfo {
            bootloader_name: c"test",
            bootloader_version: c"0",
            cmdline: c"",
            hhdm_offset: 0xffff_8000_0000_0000,
            kernel_phys: Paddr::new(0x20_0000),
            kernel_virt: Vaddr::new(0xffff_ffff_8000_0000),
            rsdp: None,
            dtb: None,
            memmap: MemMap::new(),
            efi_system_table: None,
            smbios: None,
            efi_memmap: None,
            framebuffer: None,
            modules: [None; MAX_MODULES],
            mp: None,
            howto: 0,
            duid: None,
        };
        assert_eq!(
            boot.kernel_virt_to_phys(Vaddr::new(0xffff_ffff_8001_2345)),
            Paddr::new(0x21_2345)
        );
        assert_eq!(BootFramebuffer::mask(8, 16), 0x00ff_0000);
        assert_eq!(BootFramebuffer::mask(5, 11), 0xf800);
        assert_eq!(BootFramebuffer::mask(0, 3), 0);
        assert_eq!(
            boot.hhdm(Paddr::new(0x1000)),
            Vaddr::new(0xffff_8000_0000_1000)
        );
    }

    #[test]
    fn boot_mp_lookup() {
        fn cpu(i: usize) -> BootCpu {
            BootCpu {
                processor_id: i as u32,
                hwid: 0x100 + i as u64,
            }
        }
        // SAFETY: never called by the test.
        unsafe fn start(_i: usize, _arg: usize) {}
        let mp = BootMp {
            bsp_hwid: 0x100,
            ncpus: 4,
            cpu,
            start,
        };
        assert_eq!(mp.cpus().count(), 4);
        assert_eq!(mp.index_of(0x102), Some(2));
        assert_eq!(mp.index_of(0x104), None);
    }

    #[test]
    fn boot_flags() {
        let mut boot = BootInfo {
            bootloader_name: c"test",
            bootloader_version: c"0",
            cmdline: c"",
            hhdm_offset: 0,
            kernel_phys: Paddr::new(0),
            kernel_virt: Vaddr::new(0),
            rsdp: None,
            dtb: None,
            memmap: MemMap::new(),
            efi_system_table: None,
            smbios: None,
            efi_memmap: None,
            framebuffer: None,
            modules: [None; MAX_MODULES],
            mp: None,
            howto: 0,
            duid: None,
        };
        assert_eq!(boot.boothowto(), 0);
        boot.cmdline = c"-d";
        assert_eq!(boot.boothowto(), RB_KDB);
        boot.cmdline = c"bsd -sc";
        assert_eq!(boot.boothowto(), RB_SINGLE | RB_CONFIG);
        boot.cmdline = c"-a -x";
        assert_eq!(boot.boothowto(), RB_ASKNAME);
        assert_eq!(boot.bootduid(), None);
        boot.cmdline = c"bootduid=4e564d45524f4f54";
        assert_eq!(boot.bootduid(), Some(*b"NVMEROOT"));
        assert_eq!(boot.boothowto(), 0);
        boot.cmdline = c"bootduid=4e564d45524f4f54 -s";
        assert_eq!(boot.bootduid(), Some(*b"NVMEROOT"));
        assert_eq!(boot.boothowto(), RB_SINGLE);
        boot.cmdline = c"bootduid=4e564d45524f4f5 bootduid=4e564d45524f4fxx";
        assert_eq!(boot.bootduid(), None);
        // boot(8)'s own: its howto is or'ed with the command line's, its DUID wins.
        boot.howto = RB_KDB;
        boot.duid = Some(*b"EFIBOOT0");
        assert_eq!(boot.boothowto(), RB_KDB);
        assert_eq!(boot.bootduid(), Some(*b"EFIBOOT0"));
        boot.cmdline = c"-s";
        assert_eq!(boot.boothowto(), RB_KDB | RB_SINGLE);
    }
}
/* </TESTS> */
