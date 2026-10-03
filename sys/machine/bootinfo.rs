//! What the boot glue hands to the machine and the kernel: a bootloader-neutral view of the
//! loaded image and of physical memory.
//!
//! `sys/stand/` fills it from the Limine responses; nothing here names Limine, so the kernel
//! could be booted by anything able to produce the same facts. Grows with the milestones (M3 adds
//! what `uvm_page` needs).

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
    /// The boot modules, in load order (`None` past the last).
    pub modules: [Option<BootModule>; MAX_MODULES],
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
            return 0;
        };
        let mut howto = 0;
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
}

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
            modules: [None; MAX_MODULES],
        };
        assert_eq!(
            boot.kernel_virt_to_phys(Vaddr::new(0xffff_ffff_8001_2345)),
            Paddr::new(0x21_2345)
        );
        assert_eq!(
            boot.hhdm(Paddr::new(0x1000)),
            Vaddr::new(0xffff_8000_0000_1000)
        );
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
            modules: [None; MAX_MODULES],
        };
        assert_eq!(boot.boothowto(), 0);
        boot.cmdline = c"-d";
        assert_eq!(boot.boothowto(), RB_KDB);
        boot.cmdline = c"bsd -sc";
        assert_eq!(boot.boothowto(), RB_SINGLE | RB_CONFIG);
        boot.cmdline = c"-a -x";
        assert_eq!(boot.boothowto(), RB_ASKNAME);
    }
}
