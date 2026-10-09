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
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
 * Copyright (c) 2016 Mark Kettenis
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
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
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
 * Copyright (c) 2024 Mark Kettenis <kettenis@openbsd.org>
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
 * Copyright (c) 2008 Mark Kettenis
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
//! efiboot's UEFI glue: the entry point, the consoles, the EFI disks, the memory map, the
//! device tree handed to the kernel (the firmware's, one named by SMBIOS or `machine dtb`,
//! or one made from the ACPI tables), its `/chosen` boot arguments, the clock, the device
//! names, the random seed from the tree, and the EFI `machine` commands.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/efiboot.c @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/efiboot.h @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/efidt.h @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/libsa.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ST`, `BS`, `RS`, `IH`, `efi_bootdp`, `fdt_sys`, `fdt_override`, `smbios`, `heap`, the
//!   memory map's variables, `efi_loadaddr`, `dma_constraint`, `acpi`, `bootmac`, `timer`,
//!   `ticks` and the console state are atomics (pointers and numbers set once, or by one
//!   routine); `disklist` and `framebuffer_path` are `StaticCell`s (efiboot is
//!   single-threaded and keeps no reference across calls into the firmware's callbacks;
//!   `efi_timer` only counts).
//! - `efi_main` registers libsa's and boot(8)'s machine-dependent tables (`conf.rs`), what
//!   the C's link does. libsa's heap is handed over by `efi_heap_init` (heap.h).
//! - `devopen` gives the device's open routine the device part of the name (`sd0a`), from
//!   which [`devopen_args`] reads the unit and partition again: libsa's `dv_open` takes the
//!   name, where the C passes `(unit, part)` as variadic arguments.
//! - `efi_makebootargs`, `efi_fdt` and `fdt_load_override` take boot(8)'s command state
//!   (`cmd.bootdev`, the C's global `cmd`).
//! - Device tree values are copied out of the tree before they are written back into it
//!   (`efi_console`'s `stdout-path`); the C passes a pointer into the tree.
//! - The softraid parts (`sr_volumes` in `devboot`, `openbsd,sr-bootuuid`/`sr-bootkey` and
//!   `sr_clear_keys()` in `efi_makebootargs`, `srprobe()` in `efi_diskprobe`) need
//!   softraid_arm64.c and libsa's softraid.c, not ported (feature `softraid`): no volume is
//!   ever found, so they do nothing; with the feature on, `efi_diskprobe` says so.
//! - `efi_cons_putc` prints nothing before `efi_cons_init` sets `conout` (libsa's `cn_tab`
//!   starts at the first console, the C's at none).
//! - `EFI_DEBUG` (the "Hit any key to reboot" wait in `_rtt`) is not defined, as in the C;
//!   `DPRINTF` (libsa.h, `DEBUG`) neither.

#![allow(non_upper_case_globals)] // the UEFI specification's enumerator names, in patterns

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};

use boot::cmd::{BOOTDEVLEN, CMDT_CMD, CmdState, CmdTable, cstr};
use efi::include::efi::*;
use libkern::staticcell::StaticCell;
use libsa::cons::{cn_tab, cnset};
use libsa::cread::{lseek, open, read};
use libsa::fstat::fstat;
use libsa::hdr::cons::{CN_LOWPRI, CN_MIDPRI, ConsDev};
use libsa::hdr::disklabel::FS_BSDFFS;
use libsa::hdr::param::{MAXPATHLEN, PAGE_SIZE};
use libsa::hdr::stat::Stat;
use libsa::hdr::types::{Dev, NODEV, Time, major, makedev, minor};
use libsa::printf;
use libsa::printf::Str;
use libsa::sa_alloc::{alloc, free};
use libsa::saerrno::Errno;
use libsa::snprintf;
use libsa::stand::{O_RDONLY, OpenFile, SEEK_SET, isalpha, isdigit, sa_conf};

use crate::disk::{DISKINFO_FLAG_GOODLABEL, DiskInfo};
use crate::efiacpi::efi_acpi;
use crate::efidev::{check_hibernate, efid_init};
use crate::efipxe::efi_pxeprobe;
use crate::fdt::{
    FdtNode, fdt_child_node, fdt_finalize, fdt_find_node, fdt_get_size, fdt_init, fdt_next_node,
    fdt_node_add_node, fdt_node_add_property, fdt_node_is_compatible, fdt_node_name,
    fdt_node_property, fdt_node_property_int, fdt_node_set_property,
};
use crate::smbios::{HW_PROD, HW_VENDOR, smbios_init};

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`.
const EFI_OS_INDICATIONS_BOOT_TO_FW_UI: u64 = 1;

/// `heapsiz`: the heap's size.
pub const HEAPSIZ: u64 = 1024 * 1024;

/// `EFI_DT_FIXUP_PROTOCOL_GUID` (efidt.h).
const EFI_DT_FIXUP_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0xe617_d64c,
    0xfe08,
    0x46da,
    [0xf4, 0xdc, 0xbb, 0xd5, 0x87, 0x0c, 0x73, 0x00],
);
/// `EFI_DT_APPLY_FIXUPS`.
const EFI_DT_APPLY_FIXUPS: u32 = 0x0000_0001;
/// `EFI_DT_RESERVE_MEMORY`.
const EFI_DT_RESERVE_MEMORY: u32 = 0x0000_0002;

/// `MAXDEVNAME`.
const MAXDEVNAME: usize = 16;

/// `FW_PATH`: where `efi_fdt` looks for a device tree named by SMBIOS.
const FW_PATH: &str = "/etc/firmware/dtb/";

/// `L"..."`: a NUL-terminated UTF-16 string of an ASCII one.
const fn utf16<const N: usize>(s: &str) -> [u16; N] {
    let b = s.as_bytes();
    let mut out = [0u16; N];
    let mut i = 0;
    while i < b.len() && i < N - 1 {
        out[i] = b[i] as u16;
        i += 1;
    }
    out
}

/// `EFI_DT_FIXUP` (efidt.h).
#[allow(non_snake_case)] // the specification's parameter names
type EfiDtFixup = unsafe extern "efiapi" fn(
    This: *mut EfiDtFixupProtocol,
    Fdt: *mut c_void,
    BufferSize: *mut UINTN,
    Flags: UINT32,
) -> EfiStatus;

/// `EFI_DT_FIXUP_PROTOCOL` (efidt.h).
#[repr(C)]
#[allow(non_snake_case)] // the specification's member names
struct EfiDtFixupProtocol {
    Revision: UINT64,
    Fixup: EfiDtFixup,
}

/// `struct smbios_dtb`: a device tree for a machine SMBIOS names.
struct SmbiosDtb {
    vendor: &'static str,
    prod: &'static str,
    dtb: &'static str,
}

/// `EFI_SYSTEM_TABLE *ST`.
static ST: AtomicPtr<EfiSystemTable> = AtomicPtr::new(ptr::null_mut());
/// `EFI_BOOT_SERVICES *BS`.
static BS: AtomicPtr<EfiBootServices> = AtomicPtr::new(ptr::null_mut());
/// `EFI_RUNTIME_SERVICES *RS`.
static RS: AtomicPtr<EfiRuntimeServices> = AtomicPtr::new(ptr::null_mut());
/// `EFI_HANDLE IH`: our image.
pub static IH: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// `efi_bootdp`: the device path of the device we were loaded from.
pub static EFI_BOOTDP: AtomicPtr<EfiDevicePath> = AtomicPtr::new(ptr::null_mut());
/// `fdt_sys`: the firmware's device tree.
static FDT_SYS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `fdt_override`: the device tree `machine dtb` (or SMBIOS) loaded.
static FDT_OVERRIDE: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `fdt_override_size`.
static FDT_OVERRIDE_SIZE: AtomicUsize = AtomicUsize::new(0);
/// `smbios`: the firmware's SMBIOS entry point.
static SMBIOS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

/// `heap`: the heap's physical address.
pub static HEAP: AtomicU64 = AtomicU64::new(0);
/// `mmap`: the last memory map read.
static MMAP: AtomicPtr<EfiMemoryDescriptor> = AtomicPtr::new(ptr::null_mut());
/// `mmap_key`.
static MMAP_KEY: AtomicUsize = AtomicUsize::new(0);
/// `mmap_ndesc`.
static MMAP_NDESC: AtomicUsize = AtomicUsize::new(0);
/// `mmap_descsiz`.
static MMAP_DESCSIZ: AtomicUsize = AtomicUsize::new(0);
/// `mmap_version`.
static MMAP_VERSION: AtomicU32 = AtomicU32::new(0);

/// `conout`.
static CONOUT: AtomicPtr<SimpleTextOutputInterface> = AtomicPtr::new(ptr::null_mut());
/// `conin`.
static CONIN: AtomicPtr<SimpleInputInterface> = AtomicPtr::new(ptr::null_mut());
/// `efi_cons_getc`'s `lastchar`.
static CONS_LASTCHAR: AtomicI32 = AtomicI32::new(0);

/// `serial`, `framebuffer`: the device majors for these don't match the ones used by the
/// kernel. That's fine. They're just used as an index into the cdevs array and never passed
/// on to the kernel.
const SERIAL: Dev = makedev(1, 0);
const FRAMEBUFFER: Dev = makedev(2, 0);

/// `framebuffer_path[128]`: the path of the framebuffer node (NUL-terminated).
static FRAMEBUFFER_PATH: StaticCell<[u8; 128]> = StaticCell::new([0; 128]);

/// `disklist`: the disks `efi_diskprobe` found, the boot disk first.
#[allow(clippy::vec_box)] // a box keeps its address, which bootdev_dip and f_devdata hold
pub static DISKLIST: StaticCell<Vec<Box<DiskInfo>>> = StaticCell::new(Vec::new());
/// `bootdev_dip`: the disk we booted from, then the one opened last.
pub static BOOTDEV_DIP: AtomicPtr<DiskInfo> = AtomicPtr::new(ptr::null_mut());

/// `dma_constraint[2]`: the lowest and highest address devices can reach, big-endian.
pub static DMA_CONSTRAINT: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(u64::MAX)];

/// `acpi`: `machine acpi` asked for the device tree made from the ACPI tables.
static ACPI: AtomicBool = AtomicBool::new(false);
/// `bootmac`: the MAC address of the PXE boot interface (6 bytes), null when booted from a
/// disk.
pub static BOOTMAC: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

/// `efi_loadaddr`: the 64 MB block, at a 2 MB boundary, the kernel is loaded into.
pub static EFI_LOADADDR: AtomicU64 = AtomicU64::new(0);

/// `timer`: the event that ticks every second.
static TIMER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// `ticks`: seconds since `efi_timer_init`.
static TICKS: AtomicI32 = AtomicI32::new(0);

/// `cdevs[]`: the console names.
const CDEVS: [&str; 3] = ["cons", "com", "fb"];

/// `smbios_dtb[]`: keep the list below sorted by vendor.
static SMBIOS_DTB: [SmbiosDtb; 19] = [
    SmbiosDtb {
        vendor: "ASUS",
        prod: "ASUS Vivobook S 15 S5507QA",
        dtb: "qcom/x1e80100-asus-vivobook-s15.dtb",
    },
    SmbiosDtb {
        vendor: "ASUS",
        prod: "ASUS Zenbook A14 UX3407QA",
        dtb: "qcom/x1p42100-asus-zenbook-a14.dtb",
    },
    SmbiosDtb {
        vendor: "ASUS",
        prod: "ASUS Zenbook A14 UX3407RA",
        dtb: "qcom/x1e80100-asus-zenbook-a14.dtb",
    },
    SmbiosDtb {
        vendor: "Dell",
        prod: "Inspiron 14 Plus 7441",
        dtb: "qcom/x1e80100-dell-inspiron-14-plus-7441.dtb",
    },
    SmbiosDtb {
        vendor: "Dell",
        prod: "Latitude 7455",
        dtb: "qcom/x1e80100-dell-latitude-7455.dtb",
    },
    SmbiosDtb {
        vendor: "Dell",
        prod: "XPS 13 9345",
        dtb: "qcom/x1e80100-dell-xps13-9345.dtb",
    },
    SmbiosDtb {
        vendor: "HONOR",
        prod: "MRO-XXX",
        dtb: "qcom/x1e80100-honor-magicbook-art-14.dtb",
    },
    SmbiosDtb {
        vendor: "HP",
        prod: "HP EliteBook Ultra G1q",
        dtb: "qcom/x1e80100-hp-elitebook-ultra-g1q.dtb",
    },
    SmbiosDtb {
        vendor: "HP",
        prod: "HP OmniBook X Laptop 14-fe0xxx",
        dtb: "qcom/x1e80100-hp-omnibook-x14.dtb",
    },
    SmbiosDtb {
        vendor: "HP",
        prod: "HP OmniBook X Laptop 14-fe1xxx",
        dtb: "qcom/x1p42100-hp-omnibook-x14.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "21BX",
        dtb: "qcom/sc8280xp-lenovo-thinkpad-x13s.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "21BY",
        dtb: "qcom/sc8280xp-lenovo-thinkpad-x13s.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "21N1",
        dtb: "qcom/x1e78100-lenovo-thinkpad-t14s.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "21N2",
        dtb: "qcom/x1e78100-lenovo-thinkpad-t14s.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "21NH",
        dtb: "qcom/x1p42100-lenovo-thinkbook-16.dtb",
    },
    SmbiosDtb {
        vendor: "LENOVO",
        prod: "83ED",
        dtb: "qcom/x1e80100-lenovo-yoga-slim7x.dtb",
    },
    SmbiosDtb {
        vendor: "Microsoft Corporation",
        prod: "Windows Dev Kit 2023",
        dtb: "qcom/sc8280xp-microsoft-blackrock.dtb",
    },
    SmbiosDtb {
        vendor: "Qualcomm",
        prod: "CRD",
        dtb: "qcom/x1e80100-crd.dtb",
    },
    SmbiosDtb {
        vendor: "SAMSUNG",
        prod: "Galaxy Book4 Edge",
        dtb: "qcom/x1e80100-samsung-galaxy-book4-edge.dtb",
    },
];

/// `cmd_machine[]`: the `machine` commands.
pub static CMD_MACHINE: [CmdTable; 5] = [
    CmdTable {
        cmd_name: "acpi",
        cmd_type: CMDT_CMD,
        cmd_exec: Xacpi_efi,
    },
    CmdTable {
        cmd_name: "dtb",
        cmd_type: CMDT_CMD,
        cmd_exec: Xdtb_efi,
    },
    CmdTable {
        cmd_name: "exit",
        cmd_type: CMDT_CMD,
        cmd_exec: Xexit_efi,
    },
    CmdTable {
        cmd_name: "fwsetup",
        cmd_type: CMDT_CMD,
        cmd_exec: Xfwsetup_efi,
    },
    CmdTable {
        cmd_name: "poweroff",
        cmd_type: CMDT_CMD,
        cmd_exec: Xpoweroff_efi,
    },
];

/// The system table.
pub fn st() -> &'static EfiSystemTable {
    // SAFETY: efi_main set ST from the firmware's argument before anything else runs; the
    // system table stays valid for the life of the boot program.
    unsafe { &*ST.load(Ordering::Relaxed) }
}

/// The boot services (until `ExitBootServices`).
pub fn bs() -> &'static EfiBootServices {
    // SAFETY: as for `st`; the C uses BS the same way, and not after efi_cleanup.
    unsafe { &*BS.load(Ordering::Relaxed) }
}

/// The runtime services.
pub fn rs() -> &'static EfiRuntimeServices {
    // SAFETY: as for `st`.
    unsafe { &*RS.load(Ordering::Relaxed) }
}

/// `BS->HandleProtocol(handle, &guid, &iface)`.
pub fn handle_protocol<T>(handle: EfiHandle, guid: EfiGuid) -> Result<*mut T, EfiStatus> {
    let mut g = guid;
    let mut iface: *mut c_void = ptr::null_mut();
    // SAFETY: a boot service with valid pointers to locals.
    let status = unsafe { (bs().HandleProtocol)(handle, &mut g, &mut iface) };
    if status == EFI_SUCCESS {
        Ok(iface.cast())
    } else {
        Err(status)
    }
}

/// `efi_main(image, systab)`: the entry point (`_start`, `start.S`, calls it).
#[unsafe(no_mangle)]
pub extern "C" fn efi_main(image: EfiHandle, systab: *mut EfiSystemTable) -> EfiStatus {
    ST.store(systab, Ordering::Relaxed);
    // SAFETY: the firmware hands us a valid system table.
    unsafe {
        BS.store((*systab).BootServices, Ordering::Relaxed);
        RS.store((*systab).RuntimeServices, Ordering::Relaxed);
    }
    IH.store(image, Ordering::Relaxed);
    libsa::stand::sa_conf_register(&crate::conf::SA_CONF);
    boot::boot::boot_md_register(&crate::conf::BOOT_MD);

    // disable reset by watchdog after 5 minutes
    // SAFETY: a boot service with valid arguments.
    unsafe { (bs().SetWatchdogTimer)(0, 0, 0, ptr::null_mut()) };

    let dp = handle_protocol::<EfiLoadedImage>(image, LOADED_IMAGE_PROTOCOL).and_then(|imgp| {
        // SAFETY: the firmware's loaded image protocol of our image.
        let dev = unsafe { (*imgp).DeviceHandle };
        handle_protocol::<EfiDevicePath>(dev, DEVICE_PATH_PROTOCOL)
    });
    if let Ok(dp) = dp {
        EFI_BOOTDP.store(dp, Ordering::Relaxed);
    }

    let st = st();
    for i in 0..st.NumberOfTableEntries {
        // SAFETY: the firmware's configuration table has NumberOfTableEntries entries.
        let ct = unsafe { &*st.ConfigurationTable.add(i) };
        if ct.VendorGuid == FDT_TABLE_GUID {
            FDT_SYS.store(ct.VendorTable.cast(), Ordering::Relaxed);
        }
        if ct.VendorGuid == SMBIOS_TABLE_GUID {
            SMBIOS.store(ct.VendorTable.cast(), Ordering::Relaxed);
        }
        if ct.VendorGuid == SMBIOS3_TABLE_GUID {
            SMBIOS.store(ct.VendorTable.cast(), Ordering::Relaxed);
        }
    }
    // SAFETY: the firmware's device tree (or null), which efiboot alone edits now.
    unsafe { fdt_init(FDT_SYS.load(Ordering::Relaxed)) };

    // SAFETY: single-threaded, before boot().
    unsafe { boot::boot::PROGNAME.write("BOOTAA64") };

    boot::boot::boot(0)
}

/// `efi_cons_probe(cn)`: the EFI console, `cons0`.
pub fn efi_cons_probe(cn: &ConsDev) {
    cn.set_pri(CN_MIDPRI);
    cn.set_dev(makedev(0, 0));
}

/// `efi_cons_init(cp)`.
pub fn efi_cons_init(_cp: &ConsDev) {
    CONIN.store(st().ConIn, Ordering::Relaxed);
    CONOUT.store(st().ConOut, Ordering::Relaxed);
}

/// `efi_cons_getc(dev)`: the next key (polling for one), or with `0x80` in `dev` whether
/// one is waiting (kept for the next call).
pub fn efi_cons_getc(dev: Dev) -> i32 {
    let last = CONS_LASTCHAR.load(Ordering::Relaxed);
    if last != 0 {
        if (dev & 0x80) == 0 {
            CONS_LASTCHAR.store(0, Ordering::Relaxed);
        }
        return last;
    }

    let conin = CONIN.load(Ordering::Relaxed);
    if conin.is_null() {
        return 0;
    }
    let mut key = EfiInputKey {
        ScanCode: 0,
        UnicodeChar: 0,
    };
    // SAFETY: the firmware's text input protocol.
    let mut status = unsafe { ((*conin).ReadKeyStroke)(conin, &mut key) };
    while status == EFI_NOT_READY || key.UnicodeChar == 0 {
        if (dev & 0x80) != 0 {
            return 0;
        }
        // XXX The implementation of WaitForEvent() in U-boot is broken and never returns:
        // the C polls (its BS->WaitForEvent call is #if 0).
        // SAFETY: as above.
        status = unsafe { ((*conin).ReadKeyStroke)(conin, &mut key) };
    }

    if (dev & 0x80) != 0 {
        CONS_LASTCHAR.store(i32::from(key.UnicodeChar), Ordering::Relaxed);
    }

    i32::from(key.UnicodeChar)
}

/// `efi_cons_putc(dev, c)`.
#[allow(clippy::only_used_in_recursion)] // the signature is `cn_putc`'s
pub fn efi_cons_putc(dev: Dev, c: i32) {
    if c == i32::from(b'\n') {
        efi_cons_putc(dev, i32::from(b'\r'));
    }

    let conout = CONOUT.load(Ordering::Relaxed);
    if conout.is_null() {
        return;
    }
    let mut buf: [CHAR16; 2] = [c as CHAR16, 0];
    // SAFETY: the firmware's text output protocol and a NUL-terminated string.
    unsafe { ((*conout).OutputString)(conout, buf.as_mut_ptr()) };
}

/// `efi_com_probe(cn)`: the serial console (through the EFI console), `com0`.
pub fn efi_com_probe(cn: &ConsDev) {
    cn.set_pri(CN_LOWPRI);
    cn.set_dev(SERIAL);
}

/// `efi_com_init(cn)`.
pub fn efi_com_init(cn: &ConsDev) {
    efi_cons_init(cn);
}

/// `efi_com_getc(dev)`.
pub fn efi_com_getc(dev: Dev) -> i32 {
    efi_cons_getc(dev)
}

/// `efi_com_putc(dev, c)`.
pub fn efi_com_putc(dev: Dev, c: i32) {
    efi_cons_putc(dev, c);
}

/// `efi_fb_probe(cn)`: the framebuffer console (through the EFI console), `fb0`.
pub fn efi_fb_probe(cn: &ConsDev) {
    cn.set_pri(CN_LOWPRI);
    cn.set_dev(FRAMEBUFFER);
}

/// `efi_fb_init(cn)`.
pub fn efi_fb_init(cn: &ConsDev) {
    efi_cons_init(cn);
}

/// `efi_fb_getc(dev)`.
pub fn efi_fb_getc(dev: Dev) -> i32 {
    efi_cons_getc(dev)
}

/// `efi_fb_putc(dev, c)`.
pub fn efi_fb_putc(dev: Dev, c: i32) {
    efi_cons_putc(dev, c);
}

/// `efi_heap_init()`: the heap, anywhere.
fn efi_heap_init() {
    let mut heap: EfiPhysicalAddress = 0;
    // SAFETY: a boot service with a valid pointer.
    let status = unsafe {
        (bs().AllocatePages)(
            AllocateAnyPages,
            EfiLoaderData,
            efi_size_to_pages(HEAPSIZ as usize),
            &mut heap,
        )
    };
    if status != EFI_SUCCESS {
        libsa::exit::panic(format_args!("BS->AllocatePages()"));
    }
    HEAP.store(heap, Ordering::Relaxed);
    // SAFETY: the pages are ours (EfiLoaderData) and nothing else uses them; libsa's
    // allocator gets them before its first allocation (heap.h's heap_init).
    unsafe { crate::heap::heap_init() };
}

/// `efi_diskprobe()`: every EFI block device with media that is not a partition, the one we
/// booted from first; then print them (`sd0*`...).
pub fn efi_diskprobe() {
    let mut bootdev = false;
    let mut depth = -1;

    // SAFETY: single-threaded; the only reference to the list.
    let list = unsafe { DISKLIST.get_mut() };
    list.clear();

    let mut g = BLOCK_IO_PROTOCOL;
    let mut sz: UINTN = 0;
    // SAFETY: a boot service with valid pointers; a null buffer asks for the size.
    let mut status = unsafe {
        (bs().LocateHandle)(
            ByProtocol,
            &mut g,
            ptr::null_mut(),
            &mut sz,
            ptr::null_mut(),
        )
    };
    let mut handles: *mut EfiHandle = ptr::null_mut();
    if status == EFI_BUFFER_TOO_SMALL {
        handles = alloc(sz as u32).cast();
        // SAFETY: `handles` has room for `sz` bytes.
        status =
            unsafe { (bs().LocateHandle)(ByProtocol, &mut g, ptr::null_mut(), &mut sz, handles) };
    }
    if handles.is_null() || efi_error(status) {
        return;
    }

    let bootdp = EFI_BOOTDP.load(Ordering::Relaxed);
    if !bootdp.is_null() {
        depth = efi_device_path_depth(bootdp, MEDIA_DEVICE_PATH);
    }

    // U-Boot incorrectly represents devices with a single MEDIA_DEVICE_PATH component. In
    // that case include that component into the matching, otherwise we'll blindly select
    // the first device.
    if depth == 0 {
        depth = 1;
    }

    for i in 0..sz / core::mem::size_of::<EfiHandle>() {
        // SAFETY: `handles` holds `sz` bytes of handles.
        let h = unsafe { *handles.add(i) };
        let blkio = match handle_protocol::<EfiBlockIo>(h, BLOCK_IO_PROTOCOL) {
            Ok(b) => b,
            Err(status) => libsa::exit::panic(format_args!(
                "BS->HandleProtocol() returns {}",
                status as i64
            )),
        };

        // SAFETY: the firmware's block I/O protocol and its media.
        let media = unsafe { &*(*blkio).Media };
        if media.LogicalPartition != 0 || media.MediaPresent == 0 {
            continue;
        }
        let mut di = efid_init(blkio);

        if !bootdp.is_null()
            && depth != -1
            && !bootdev
            && let Ok(dp) = handle_protocol::<EfiDevicePath>(h, DEVICE_PATH_PROTOCOL)
            && efi_device_path_ncmp(bootdp, dp, depth) == 0
        {
            BOOTDEV_DIP.store(&mut *di, Ordering::Relaxed);
            bootdev = true;
            check_hibernate(&mut di);
            list.insert(0, di);
            continue;
        }
        check_hibernate(&mut di);
        list.push(di);
    }

    // SAFETY: from `alloc` above.
    unsafe { free(handles.cast(), sz as u32) };

    // Print available disks and probe for softraid.
    printf!("disks:");
    let boot = BOOTDEV_DIP.load(Ordering::Relaxed).cast_const();
    for (i, di) in list.iter().enumerate() {
        let star = if ptr::eq(&**di, boot) { "*" } else { "" };
        printf!(" sd{}{}", i, star);
    }
    srprobe();
    printf!("\n");
}

/// `srprobe()` (softraid_arm64.c): not ported; no softraid volume is found.
fn srprobe() {
    #[cfg(feature = "softraid")]
    printf!(" (softraid: not ported)");
}

/// `efi_device_path_depth(dp, dptype)`: the number of nodes up to, but not including, the
/// first node of the specified type.
pub fn efi_device_path_depth(dp: *const EfiDevicePath, dptype: u8) -> i32 {
    let mut dp = dp;
    let mut i = 0;
    // SAFETY: a firmware device path, ending with an end node.
    unsafe {
        while !is_device_path_end(dp) {
            if device_path_type(dp) == dptype {
                return i;
            }
            dp = next_device_path_node(dp);
            i += 1;
        }
    }
    i
}

/// `efi_device_path_ncmp(dpa, dpb, deptn)`: compare the first `deptn` nodes.
pub fn efi_device_path_ncmp(
    dpa: *const EfiDevicePath,
    dpb: *const EfiDevicePath,
    deptn: i32,
) -> i32 {
    let (mut dpa, mut dpb) = (dpa, dpb);
    // SAFETY: firmware device paths, ending with end nodes; a node is
    // `device_path_node_length` bytes.
    unsafe {
        for _ in 0..deptn {
            let (ea, eb) = (is_device_path_end(dpa), is_device_path_end(dpb));
            if ea || eb {
                return if ea && eb {
                    0
                } else if ea {
                    -1
                } else {
                    1
                };
            }
            let (la, lb) = (device_path_node_length(dpa), device_path_node_length(dpb));
            let cmp = la as i32 - lb as i32;
            if cmp != 0 {
                return cmp;
            }
            let a = core::slice::from_raw_parts(dpa.cast::<u8>(), la);
            let b = core::slice::from_raw_parts(dpb.cast::<u8>(), la);
            if let Some((x, y)) = a.iter().zip(b).find(|(x, y)| x != y) {
                return i32::from(*x) - i32::from(*y);
            }
            dpa = next_device_path_node(dpa);
            dpb = next_device_path_node(dpb);
        }
    }
    0
}

/// The first "simple-framebuffer" child of `node` that is enabled: its name.
fn enabled_framebuffer(node: Option<FdtNode>) -> Option<Vec<u8>> {
    let mut child = fdt_child_node(node);
    while let Some(c) = child {
        if fdt_node_is_compatible(Some(c), b"simple-framebuffer") {
            let status = fdt_node_property(Some(c), b"status");
            if status.as_deref().is_none_or(|s| cstr(s) == b"okay") {
                return Some(fdt_node_name(Some(c)).unwrap_or_default());
            }
        }
        child = fdt_next_node(Some(c));
    }
    None
}

/// `efi_framebuffer()`: a "simple-framebuffer" node under `/chosen` for the GOP's frame
/// buffer, unless the tree has one.
pub fn efi_framebuffer() {
    // SAFETY: single-threaded; the only reference.
    let path = unsafe { FRAMEBUFFER_PATH.get_mut() };

    // Don't create a "simple-framebuffer" node if we already have one. Besides "/chosen",
    // we also check under "/" since that is where the Raspberry Pi firmware puts it.
    if let Some(name) = enabled_framebuffer(fdt_find_node(b"/chosen")) {
        libkern::strlcpy::strlcpy(path, b"/chosen/");
        libkern::strlcat::strlcat(path, &name);
        return;
    }
    if let Some(name) = enabled_framebuffer(fdt_find_node(b"/")) {
        libkern::strlcpy::strlcpy(path, b"/");
        libkern::strlcat::strlcat(path, &name);
        return;
    }

    let mut guid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
    let mut gop: *mut c_void = ptr::null_mut();
    // SAFETY: a boot service with valid pointers.
    let status = unsafe { (bs().LocateProtocol)(&mut guid, ptr::null_mut(), &mut gop) };
    if status != EFI_SUCCESS {
        return;
    }
    let gop = gop.cast::<EfiGraphicsOutput>();

    // Paranoia!
    // SAFETY: the firmware's graphics output protocol, its mode and the mode's
    // information, each checked for null before it is read.
    let (mode, info) = unsafe {
        if gop.is_null() || (*gop).Mode.is_null() || (*(*gop).Mode).Info.is_null() {
            return;
        }
        (*(*gop).Mode, *(*(*gop).Mode).Info)
    };

    let (format, pxsize): (&[u8], u32) = match info.PixelFormat {
        PixelRedGreenBlueReserved8BitPerColor => (b"x8b8g8r8", 4),
        PixelBlueGreenRedReserved8BitPerColor => (b"x8r8g8b8", 4),
        PixelBitMask
            if info.PixelInformation.RedMask == 0xf800
                && info.PixelInformation.GreenMask == 0x07e0
                && info.PixelInformation.BlueMask == 0x001f =>
        {
            (b"r5g6b5", 2)
        }
        f => {
            if f == PixelBitMask {
                printf!("Unsupported PixelInformation bitmasks\n");
            }
            printf!(
                "Unsupported PixelFormat {}, not adding \"simple-framebuffer\" DT node\n",
                f
            );
            return;
        }
    };

    let base = mode.FrameBufferBase;
    let size = mode.FrameBufferSize as u64;
    let width = info.HorizontalResolution.to_be_bytes();
    let height = info.VerticalResolution.to_be_bytes();
    let stride = (info.PixelsPerScanLine * pxsize).to_be_bytes();

    let node = fdt_find_node(b"/");
    let acells = fdt_node_property_int(node, b"#address-cells").unwrap_or(1) as usize;
    let scells = fdt_node_property_int(node, b"#size-cells").unwrap_or(1) as usize;
    if acells > 2 || scells > 2 {
        return;
    }
    let mut reg = [0u32; 4];
    if acells >= 1 {
        reg[0] = base as u32;
    }
    if acells == 2 {
        reg[1] = reg[0];
        reg[0] = (base >> 32) as u32;
    }
    if scells >= 1 {
        reg[acells] = size as u32;
    }
    if scells == 2 {
        reg[acells + 1] = reg[acells];
        reg[acells] = (size >> 32) as u32;
    }
    let reg: Vec<u8> = reg[..acells + scells]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();

    let node = fdt_find_node(b"/chosen");
    let child = fdt_node_add_node(node, b"framebuffer");
    fdt_node_add_property(child, b"status", b"okay\0");
    let mut f = format.to_vec();
    f.push(0);
    fdt_node_add_property(child, b"format", &f);
    fdt_node_add_property(child, b"stride", &stride);
    fdt_node_add_property(child, b"height", &height);
    fdt_node_add_property(child, b"width", &width);
    fdt_node_add_property(child, b"reg", &reg);
    fdt_node_add_property(child, b"compatible", b"simple-framebuffer\0");

    libkern::strlcpy::strlcpy(path, b"/chosen/framebuffer");
}

/// `efi_console()`: point `/chosen/stdout-path` at the console's node (a serial port by its
/// alias, or the framebuffer).
pub fn efi_console() {
    let Some(cn) = cn_tab() else {
        return;
    };
    if major(cn.dev()) == major(SERIAL) {
        // Construct alias and resolve it.
        let mut alias = [0u8; 16];
        let n = snprintf!(&mut alias, "serial{}", minor(cn.dev())).min(15);
        let node = fdt_find_node(b"/aliases");
        let Some(serial_path) = fdt_node_property(node, &alias[..n]) else {
            return;
        };

        // Point stdout-path at the serial node.
        let node = fdt_find_node(b"/chosen");
        let mut v = cstr(&serial_path).to_vec();
        v.push(0);
        fdt_node_add_property(node, b"stdout-path", &v);
    } else if major(cn.dev()) == major(FRAMEBUFFER) {
        // SAFETY: single-threaded; a copy of the path.
        let path = unsafe { FRAMEBUFFER_PATH.read() };
        if path[0] == 0 {
            return;
        }

        // Point stdout-path at the framebuffer node.
        let node = fdt_find_node(b"/chosen");
        let mut v = cstr(&path).to_vec();
        v.push(0);
        fdt_node_add_property(node, b"stdout-path", &v);
    }
}

/// `efi_dma_constraint()`: `/chosen/openbsd,dma-constraint`, from `/soc`'s "dma-ranges" or
/// what some SoCs do not say.
pub fn efi_dma_constraint() {
    let node = fdt_find_node(b"/");
    let pacells = fdt_node_property_int(node, b"#address-cells").unwrap_or(1) as usize;
    let pscells = fdt_node_property_int(node, b"#size-cells").unwrap_or(1) as usize;
    if pacells > 2 || pscells > 2 {
        return;
    }

    let node = fdt_find_node(b"/soc");
    if node.is_some() {
        let acells =
            fdt_node_property_int(node, b"#address-cells").unwrap_or(pacells as u32) as usize;
        let scells = fdt_node_property_int(node, b"#size-cells").unwrap_or(pscells as u32) as usize;
        if acells > 2 || scells > 2 {
            return;
        }

        let prop = fdt_node_property(node, b"dma-ranges").unwrap_or_default();
        let cell = |i: usize| {
            u64::from(u32::from_be_bytes([
                prop[4 * i],
                prop[4 * i + 1],
                prop[4 * i + 2],
                prop[4 * i + 3],
            ]))
        };
        if prop.len() == (acells + pacells + scells) * 4 {
            let mut base = cell(acells);
            if pacells == 2 {
                base = (base << 32) | cell(acells + 1);
            }
            let mut size = cell(acells + pacells);
            if scells == 2 {
                size = (size << 32) | cell(acells + pacells + 1);
            }

            DMA_CONSTRAINT[0].store(base.to_be(), Ordering::Relaxed);
            DMA_CONSTRAINT[1].store(
                base.wrapping_add(size).wrapping_sub(1).to_be(),
                Ordering::Relaxed,
            );
        }
    }

    // Some SoC's have DMA constraints that aren't explicitly advertised.
    let node = fdt_find_node(b"/");
    if fdt_node_is_compatible(node, b"brcm,bcm2711") {
        DMA_CONSTRAINT[1].store(0x3bff_ffffu64.to_be(), Ordering::Relaxed);
    }
    if [
        &b"rockchip,rk3528"[..],
        b"rockchip,rk3566",
        b"rockchip,rk3568",
        b"rockchip,rk3576",
        b"rockchip,rk3588",
        b"rockchip,rk3588s",
    ]
    .iter()
    .any(|c| fdt_node_is_compatible(node, c))
    {
        DMA_CONSTRAINT[1].store(0xffff_ffffu64.to_be(), Ordering::Relaxed);
    }
    if fdt_node_is_compatible(node, b"qcom,sc8280xp")
        || fdt_node_is_compatible(node, b"qcom,x1e80100")
    {
        DMA_CONSTRAINT[1].store(0xffff_ffffu64.to_be(), Ordering::Relaxed);
    }

    // Pass DMA constraint.
    let node = fdt_find_node(b"/chosen");
    let mut v = [0u8; 16];
    v[..8].copy_from_slice(&DMA_CONSTRAINT[0].load(Ordering::Relaxed).to_ne_bytes());
    v[8..].copy_from_slice(&DMA_CONSTRAINT[1].load(Ordering::Relaxed).to_ne_bytes());
    fdt_node_add_property(node, b"openbsd,dma-constraint", &v);
}

/// `efi_makebootargs(bootargs, howto)`: the device tree handed to the kernel, in pages of
/// its own with a page of room, `/chosen` filled in (`bootargs`, `openbsd,boothowto`, the
/// boot disk's DUID, the EFI system table, room for the memory map, the framebuffer, the
/// console, the DMA constraint); null if there is none.
pub fn efi_makebootargs(cmd: &CmdState, bootargs: &[u8], howto: i32) -> *mut u8 {
    let zero = [0u8; 8];
    let uefi_system_table = (ST.load(Ordering::Relaxed) as u64).to_be_bytes();
    let boothowto = (howto as u32).to_be_bytes();

    let mut fdt = efi_fdt(cmd);
    if fdt.is_null() || ACPI.load(Ordering::Relaxed) {
        fdt = efi_acpi();
    }

    // SAFETY: a device tree from the firmware, a file or the template, or null.
    let size = unsafe { fdt_get_size(fdt) };
    if size == 0 {
        return ptr::null_mut();
    }

    let len = (size + PAGE_SIZE as usize).div_ceil(PAGE_SIZE as usize) * PAGE_SIZE as usize;
    let mut addr: EfiPhysicalAddress = 0;
    // SAFETY: a boot service with a valid pointer.
    let status = unsafe {
        (bs().AllocatePages)(
            AllocateAnyPages,
            EfiLoaderData,
            efi_size_to_pages(len),
            &mut addr,
        )
    };
    if status == EFI_SUCCESS {
        let dst = addr as usize as *mut u8;
        // SAFETY: `len` bytes of fresh pages, more than the `size` bytes of the tree; the
        // header's `fh_size` (bytes 4..8) becomes the new size.
        unsafe {
            ptr::copy_nonoverlapping(fdt, dst, size);
            dst.add(4)
                .cast::<u32>()
                .write_unaligned((len as u32).to_be());
        }
        fdt = dst;
    }

    // SAFETY: the tree, in our pages or the firmware's, now efiboot's alone.
    if unsafe { fdt_init(fdt) } == 0 {
        return ptr::null_mut();
    }

    // Create common nodes which might not exist when using mach dtb
    if fdt_find_node(b"/aliases").is_none() {
        fdt_node_add_node(fdt_find_node(b"/"), b"aliases");
    }
    if fdt_find_node(b"/chosen").is_none() {
        fdt_node_add_node(fdt_find_node(b"/"), b"chosen");
    }

    let node = fdt_find_node(b"/chosen");
    let mut args = cstr(bootargs).to_vec();
    args.push(0);
    fdt_node_add_property(node, b"bootargs", &args);
    fdt_node_add_property(node, b"openbsd,boothowto", &boothowto);

    // Pass DUID of the boot disk.
    let dip = BOOTDEV_DIP.load(Ordering::Relaxed);
    if !dip.is_null() {
        // SAFETY: bootdev_dip points into the disk list.
        let bootduid = unsafe { (*dip).disklabel.d_uid };
        if bootduid != zero {
            fdt_node_add_property(node, b"openbsd,bootduid", &bootduid);
        }

        // `bootdev_dip->sr_vol` (openbsd,sr-bootuuid, openbsd,sr-bootkey): always null,
        // softraid is not ported.
    }

    // sr_clear_keys(): softraid is not ported, there are no keys.

    // Pass netboot interface address.
    let mac = BOOTMAC.load(Ordering::Relaxed);
    if !mac.is_null() {
        // SAFETY: bootmac points at efi_pxeprobe's hardware address (16 bytes).
        let mac = unsafe { core::slice::from_raw_parts(mac, 6) }.to_vec();
        fdt_node_add_property(node, b"openbsd,bootmac", &mac);
    }

    // Pass EFI system table.
    fdt_node_add_property(node, b"openbsd,uefi-system-table", &uefi_system_table);

    // Placeholders for EFI memory map.
    fdt_node_add_property(node, b"openbsd,uefi-mmap-start", &zero);
    fdt_node_add_property(node, b"openbsd,uefi-mmap-size", &zero[..4]);
    fdt_node_add_property(node, b"openbsd,uefi-mmap-desc-size", &zero[..4]);
    fdt_node_add_property(node, b"openbsd,uefi-mmap-desc-ver", &zero[..4]);

    efi_framebuffer();
    efi_console();
    efi_dma_constraint();

    fdt_finalize();

    fdt
}

/// `efi_updatefdt()`: the final memory map into `/chosen`.
pub fn efi_updatefdt() {
    let ndesc = MMAP_NDESC.load(Ordering::Relaxed);
    let descsiz = MMAP_DESCSIZ.load(Ordering::Relaxed);
    let uefi_mmap_start = (MMAP.load(Ordering::Relaxed) as u64).to_be_bytes();
    let uefi_mmap_size = ((ndesc * descsiz) as u32).to_be_bytes();
    let uefi_mmap_desc_size = (descsiz as u32).to_be_bytes();
    let uefi_mmap_desc_ver = MMAP_VERSION.load(Ordering::Relaxed).to_be_bytes();

    let node = fdt_find_node(b"/chosen");
    if node.is_none() {
        return;
    }

    // Pass EFI memory map.
    fdt_node_set_property(node, b"openbsd,uefi-mmap-start", &uefi_mmap_start);
    fdt_node_set_property(node, b"openbsd,uefi-mmap-size", &uefi_mmap_size);
    fdt_node_set_property(node, b"openbsd,uefi-mmap-desc-size", &uefi_mmap_desc_size);
    fdt_node_set_property(node, b"openbsd,uefi-mmap-desc-ver", &uefi_mmap_desc_ver);

    fdt_finalize();
}

/// `machdep()`: the console, the heap, SMBIOS, the kernel's 64 MB, the clock, the disks and
/// the network.
pub fn machdep() {
    libsa::cons::cninit();
    efi_heap_init();
    // SAFETY: the firmware's SMBIOS entry point, or null.
    unsafe { smbios_init(SMBIOS.load(Ordering::Relaxed)) };

    // The kernel expects to be loaded into a block of memory aligned on a 2MB boundary. We
    // allocate a block of 64MB of memory, which gives us plenty of room for growth.
    let mut addr: EfiPhysicalAddress = 0;
    if efi_memprobe_find(
        efi_size_to_pages(64 * 1024 * 1024),
        0x20_0000,
        EfiLoaderCode,
        &mut addr,
    ) != EFI_SUCCESS
    {
        printf!("Can't allocate memory\n");
    }
    EFI_LOADADDR.store(addr, Ordering::Relaxed);

    efi_timer_init();
    efi_diskprobe();
    efi_pxeprobe();
}

/// `efi_cleanup()`: stop the clock, sync the memory map into the tree and leave the boot
/// services.
pub fn efi_cleanup() {
    efi_timer_cleanup();

    // retry once in case of failure
    for retry in (0..=1).rev() {
        efi_memprobe_internal(); // sync the current map
        efi_updatefdt();
        // SAFETY: a boot service with our image handle and the map key just read.
        let status = unsafe {
            (bs().ExitBootServices)(IH.load(Ordering::Relaxed), MMAP_KEY.load(Ordering::Relaxed))
        };
        if status == EFI_SUCCESS {
            break;
        }
        if retry == 0 {
            libsa::exit::panic(format_args!("ExitBootServices failed ({})", status as i64));
        }
    }
}

/// `_rtt()`: reset the machine.
pub fn _rtt() -> ! {
    // SAFETY: a runtime service with valid arguments.
    unsafe { (rs().ResetSystem)(EfiResetCold, EFI_SUCCESS, 0, ptr::null_mut()) };
    loop {
        core::hint::spin_loop();
    }
}

/// `efi_timer(event, context)`: one more second.
///
/// U-Boot only implements the GetTime() Runtime Service if it has been configured with
/// CONFIG_DM_RTC. Most board configurations don't include that option, so we can't use it
/// to implement our boot prompt timeout. Instead we use timer events to simulate a clock
/// that ticks ever second.
///
/// # Safety
///
/// Called by the firmware, as the notify function of `timer`.
unsafe extern "efiapi" fn efi_timer(_event: EfiEvent, _context: *mut c_void) {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// `efi_timer_init()`: a periodic timer event of one second.
fn efi_timer_init() {
    let mut timer: EfiEvent = ptr::null_mut();
    // SAFETY: boot services with valid arguments; `efi_timer` has the notify signature.
    let mut status = unsafe {
        (bs().CreateEvent)(
            EVT_TIMER | EVT_NOTIFY_SIGNAL,
            TPL_CALLBACK,
            Some(efi_timer),
            ptr::null_mut(),
            &mut timer,
        )
    };
    TIMER.store(timer, Ordering::Relaxed);
    if status == EFI_SUCCESS {
        // SAFETY: the event just created.
        status = unsafe { (bs().SetTimer)(timer, TimerPeriodic, 10_000_000) };
    }
    if efi_error(status) {
        printf!("Can't create timer\n");
    }
}

/// `efi_timer_cleanup()`.
fn efi_timer_cleanup() {
    // SAFETY: the event efi_timer_init created.
    unsafe { (bs().CloseEvent)(TIMER.load(Ordering::Relaxed)) };
}

/// `getsecs()`: the seconds the timer counted.
pub fn getsecs() -> Time {
    Time::from(TICKS.load(Ordering::Relaxed))
}

/// `devboot(dev, p)`: the name of the boot device: `tftp0a` without a boot disk, `esp0a`
/// when it has no BSD label, else `sd<N>a` (or the softraid volume it is a chunk of).
pub fn devboot(_dev: Dev, p: &mut [u8; BOOTDEVLEN]) {
    let dip = BOOTDEV_DIP.load(Ordering::Relaxed);
    if dip.is_null() {
        libkern::strlcpy::strlcpy(&mut p[..7], b"tftp0a");
        return;
    }

    // SAFETY: bootdev_dip points into the disk list.
    let d = unsafe { &*dip };

    // If there is no BSD disklabel on the boot device, boot from the ESP instead.
    if (d.flags & DISKINFO_FLAG_GOODLABEL) == 0 {
        libkern::strlcpy::strlcpy(&mut p[..6], b"esp0a");
        return;
    }

    // SAFETY: single-threaded; a shared look at the list.
    let list = unsafe { DISKLIST.get() };
    let sd_boot_vol = list
        .iter()
        .position(|di| ptr::eq(&**di, dip))
        .unwrap_or(list.len());

    // Determine the partition type for the 'a' partition of the boot device.
    let part_type = d.disklabel.d_partitions[0].p_fstype;

    // See if we booted from a disk that is a member of a bootable softraid volume: the
    // volumes are softraid_arm64.c's, not ported, so `sr_volumes` is empty and none is.
    let sr_boot_vol: i32 = -1;

    if sr_boot_vol != -1 && part_type != FS_BSDFFS {
        libkern::strlcpy::strlcpy(&mut p[..5], b"sr0a");
        p[2] = b'0'.wrapping_add(sr_boot_vol as u8);
        return;
    }

    libkern::strlcpy::strlcpy(&mut p[..5], b"sd0a");
    p[2] = b'0'.wrapping_add(sd_boot_vol as u8);
}

/// `cnspeed(dev, sp)`.
pub fn cnspeed(_dev: Dev, _sp: i32) -> i32 {
    115_200
}

/// `ttyname(fd)`: the console's name.
pub fn ttyname() -> [u8; 8] {
    let mut buf = [0u8; 8];
    if let Some(cn) = cn_tab() {
        let dev = cn.dev();
        snprintf!(
            &mut buf,
            "{}{}",
            CDEVS.get(major(dev) as usize).unwrap_or(&""),
            minor(dev)
        );
    }
    buf
}

/// `ttydev(name)`: the device of a console name (`cons0`, `com0`, `fb0`), `NODEV` if none.
pub fn ttydev(name: &[u8]) -> Dev {
    let name = cstr(name);
    let digits = name.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == name.len() {
        return NODEV;
    }
    let (prefix, num) = name.split_at(name.len() - digits);
    // the C reads the digits backwards: "12" gives unit 21
    let mut unit: i32 = -1;
    for &c in num.iter().rev() {
        unit = (if unit < 0 { 0 } else { unit * 10 }) + i32::from(c - b'0');
    }
    for (i, d) in CDEVS.iter().enumerate() {
        // strncmp(name, cdevs[i], no - name + 1)
        let d = d.as_bytes();
        if (0..prefix.len()).all(|k| d.get(k).copied().unwrap_or(0) == prefix[k]) {
            return makedev(i as u32, unit as u32);
        }
    }
    NODEV
}

/// What `devparse` finds in a name: the device's index in `devsw[]`, the unit, the
/// partition, and where the file name starts.
struct DevSpec {
    dev: usize,
    unit: u32,
    part: u32,
    file: usize,
}

/// `devparse(fname, &dev, &unit, &part, &file)`: parse a device spec,
/// `[A-Za-z]*[0-9]*[A-Za-z]:file` (dev, uint, part); without one, the first device's unit
/// 0, partition `a`.
fn devparse(fname: &[u8]) -> Result<DevSpec, Errno> {
    let at = |i: usize| fname.get(i).copied().unwrap_or(0);
    let mut spec = DevSpec {
        dev: 0,
        unit: 0, // default to wd0a
        part: 0,
        file: 0,
    };

    if let Some(s) = fname.iter().position(|&c| c == b':') {
        let devlen = s;
        if devlen > MAXDEVNAME {
            return Err(Errno::EINVAL);
        }

        // extract device name
        let mut i = 0;
        while isalpha(at(i)) && i < devlen {
            i += 1;
        }
        let devname = &fname[..i];

        if !isdigit(at(i)) {
            return Err(Errno::EUNIT);
        }

        // device number
        let mut u: u32 = 0;
        while isdigit(at(i)) && i < devlen {
            u = u.wrapping_mul(10).wrapping_add(u32::from(at(i) - b'0'));
            i += 1;
        }

        if !isalpha(at(i)) {
            return Err(Errno::EPART);
        }

        // partition number
        let mut p = 0;
        if i < devlen {
            p = u32::from(at(i)).wrapping_sub(u32::from(b'a'));
            i += 1;
        }

        if i != devlen {
            return Err(Errno::ENXIO);
        }

        // check device name
        let Some(d) = sa_conf()
            .devsw
            .iter()
            .position(|dp| dp.dv_name.as_bytes() == devname)
        else {
            return Err(Errno::ENXIO);
        };

        spec.unit = u;
        spec.part = p;
        spec.dev = d;
        spec.file = s + 1;
    }

    Ok(spec)
}

/// `devopen(f, fname, &file)`: open the device the name gives; the file name after it.
pub fn devopen<'a>(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno> {
    let spec = devparse(fname)?;
    let file = &fname[spec.file..];

    let dp = &sa_conf().devsw[spec.dev];
    f.f_dev = Some(dp);

    if dp.dv_name != "tftp" {
        // Clear bootmac, to signal that we loaded this file from a non-network device.
        BOOTMAC.store(ptr::null_mut(), Ordering::Relaxed);
    }

    // the device's (unit, part): its open routine reads them from the device part
    let mut devpart = &fname[..spec.file.saturating_sub(1)];
    (dp.dv_open)(f, &mut devpart)?;
    Ok(file)
}

/// The `(unit, part)` arguments `devopen` gives a device's open routine: read again from
/// the device part of the name (`sd0a`), as `devparse` reads them; `(0, 0)` for none.
pub fn devopen_args(devpart: &[u8]) -> (u32, u32) {
    let at = |i: usize| devpart.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while isalpha(at(i)) {
        i += 1;
    }
    let mut unit: u32 = 0;
    while isdigit(at(i)) {
        unit = unit.wrapping_mul(10).wrapping_add(u32::from(at(i) - b'0'));
        i += 1;
    }
    let part = if i < devpart.len() {
        u32::from(at(i)).wrapping_sub(u32::from(b'a'))
    } else {
        0
    };
    (unit, part)
}

/// `efi_memprobe_internal()`: read the EFI memory map (into `alloc()`ed memory).
fn efi_memprobe_internal() {
    let old = MMAP.load(Ordering::Relaxed);
    let oldsize = MMAP_NDESC.load(Ordering::Relaxed) * MMAP_DESCSIZ.load(Ordering::Relaxed);
    // SAFETY: the previous map, `alloc`ed below (or null).
    unsafe { free(old.cast(), oldsize as u32) };

    let (mut siz, mut mapkey, mut mmsiz, mut mmver): (UINTN, UINTN, UINTN, UINT32) = (0, 0, 0, 0);
    // SAFETY: a boot service with valid pointers; a null map asks for the size.
    let status = unsafe {
        (bs().GetMemoryMap)(
            &mut siz,
            ptr::null_mut(),
            &mut mapkey,
            &mut mmsiz,
            &mut mmver,
        )
    };
    if status != EFI_BUFFER_TOO_SMALL {
        libsa::exit::panic(format_args!("cannot get the size of memory map"));
    }
    let mm = alloc(siz as u32).cast::<EfiMemoryDescriptor>();
    // SAFETY: `mm` has room for `siz` bytes.
    let status = unsafe { (bs().GetMemoryMap)(&mut siz, mm, &mut mapkey, &mut mmsiz, &mut mmver) };
    if status != EFI_SUCCESS {
        libsa::exit::panic(format_args!("cannot get the memory map"));
    }
    let n = siz / mmsiz.max(1);
    MMAP.store(mm, Ordering::Relaxed);
    MMAP_KEY.store(mapkey, Ordering::Relaxed);
    MMAP_NDESC.store(n, Ordering::Relaxed);
    MMAP_DESCSIZ.store(mmsiz, Ordering::Relaxed);
    MMAP_VERSION.store(mmver, Ordering::Relaxed);
}

/// `efi_memprobe_find(pages, align, type, &addr)`: allocate `pages` pages at an `align`
/// boundary in conventional memory. 64-bit ARMs can have a much wider memory mapping, as in
/// somewhere after the 32-bit region. To cope with our alignment requirement, use the
/// memory table to find a place where we can fit.
fn efi_memprobe_find(
    pages: UINTN,
    align: UINTN,
    ty: EfiMemoryType,
    addr: &mut EfiPhysicalAddress,
) -> EfiStatus {
    if align < EFI_PAGE_SIZE {
        return EFI_INVALID_PARAMETER;
    }

    efi_memprobe_internal(); // sync the current map

    let mut mm = MMAP.load(Ordering::Relaxed).cast_const();
    let descsiz = MMAP_DESCSIZ.load(Ordering::Relaxed);
    for _ in 0..MMAP_NDESC.load(Ordering::Relaxed) {
        // SAFETY: `mm` is within the map GetMemoryMap filled (ndesc descriptors of the size
        // it said); descriptors may be unaligned for Rust, so they are read unaligned.
        let d = unsafe { mm.read_unaligned() };
        mm = next_memory_descriptor(mm, descsiz);
        if d.Type != EfiConventionalMemory {
            continue;
        }

        if d.NumberOfPages < pages as u64 {
            continue;
        }

        for j in 0..d.NumberOfPages {
            if d.NumberOfPages - j < pages as u64 {
                break;
            }

            let mut paddr = d.PhysicalStart + j * EFI_PAGE_SIZE as u64;
            if paddr & (align as u64 - 1) != 0 {
                continue;
            }

            // SAFETY: a boot service with a valid pointer.
            if unsafe { (bs().AllocatePages)(AllocateAddress, ty, pages, &mut paddr) }
                == EFI_SUCCESS
            {
                *addr = paddr;
                return EFI_SUCCESS;
            }
        }
    }
    EFI_OUT_OF_RESOURCES
}

/// `mdrandom(buf, buflen)`: mix the tree's `/chosen` "rng-seed" and "kaslr-seed" into
/// `buf`; 0 if there was one.
pub fn mdrandom(buf: &mut [u8]) -> i32 {
    let mut ret = -1;

    let node = fdt_find_node(b"/chosen");
    if node.is_none() {
        return -1;
    }

    for name in [&b"rng-seed"[..], b"kaslr-seed"] {
        if let Some(random) = fdt_node_property(node, name) {
            for (i, b) in buf.iter_mut().enumerate() {
                *b ^= random[i % random.len()];
            }
            ret = 0;
        }
    }

    ret
}

/// `efi_fdt()`: the device tree to use: `machine dtb`'s, else one named for the machine
/// SMBIOS describes, else the firmware's.
pub fn efi_fdt(cmd: &CmdState) -> *mut u8 {
    // 'mach dtb' has precedence
    let over = FDT_OVERRIDE.load(Ordering::Relaxed);
    if !over.is_null() {
        return over;
    }

    // Return system provided one
    // SAFETY: single-threaded; copies of the strings smbios_init found.
    let (vendor, prod) = unsafe { (HW_VENDOR.get().clone(), HW_PROD.get().clone()) };
    let (Some(vendor), Some(prod)) = (vendor, prod) else {
        return FDT_SYS.load(Ordering::Relaxed);
    };

    for d in &SMBIOS_DTB {
        if vendor.starts_with(d.vendor.as_bytes()) && prod.starts_with(d.prod.as_bytes()) {
            let mut dtb = [0u8; 256];
            snprintf!(&mut dtb, "{}{}", FW_PATH, d.dtb);
            fdt_load_override(cmd, Some(cstr(&dtb)));
            // TODO: find a better mechanism
            cnset(ttydev(b"fb0"));
            break;
        }
    }

    let over = FDT_OVERRIDE.load(Ordering::Relaxed);
    if over.is_null() {
        FDT_SYS.load(Ordering::Relaxed)
    } else {
        over
    }
}

/// `fdt_load_override(file)`: load the device tree `file` from the boot device (applying
/// the firmware's fixups), or drop the loaded one for `None`.
pub fn fdt_load_override(cmd: &CmdState, file: Option<&[u8]>) -> i32 {
    let over = FDT_OVERRIDE.load(Ordering::Relaxed);
    let Some(file) = file else {
        if !over.is_null() {
            // SAFETY: the pages a previous call allocated.
            unsafe {
                (bs().FreePages)(
                    over as u64,
                    efi_size_to_pages(FDT_OVERRIDE_SIZE.load(Ordering::Relaxed)),
                )
            };
            FDT_OVERRIDE.store(ptr::null_mut(), Ordering::Relaxed);
            // SAFETY: the firmware's tree, efiboot's alone.
            unsafe { fdt_init(FDT_SYS.load(Ordering::Relaxed)) };
        }
        return 0;
    };

    let mut path = [0u8; MAXPATHLEN];
    snprintf!(&mut path, "{}:{}", Str(&cmd.bootdev), Str(file));

    let fd = match open(cstr(&path), O_RDONLY) {
        Ok(fd) => fd,
        Err(_) => {
            printf!("cannot open {}\n", Str(&path));
            return 0;
        }
    };
    let mut sb = Stat::default();
    if fstat(fd, &mut sb).is_err() {
        printf!("cannot open {}\n", Str(&path));
        return 0;
    }
    let mut dt_size = sb.st_size as usize;
    let mut addr: EfiPhysicalAddress;
    loop {
        addr = 0;
        if efi_memprobe_find(
            efi_size_to_pages(dt_size),
            PAGE_SIZE as usize,
            EfiLoaderData,
            &mut addr,
        ) != EFI_SUCCESS
        {
            printf!("cannot allocate memory for {}\n", Str(&path));
            return 0;
        }
        // SAFETY: the pages just allocated hold at least `dt_size` bytes.
        let dst = unsafe {
            core::slice::from_raw_parts_mut(addr as usize as *mut u8, sb.st_size as usize)
        };
        if read(fd, dst) != Ok(sb.st_size as usize) {
            printf!("cannot read from {}\n", Str(&path));
            return 0;
        }

        let mut guid = EFI_DT_FIXUP_PROTOCOL_GUID;
        let mut dt_fixup: *mut c_void = ptr::null_mut();
        // SAFETY: a boot service with valid pointers.
        let status = unsafe { (bs().LocateProtocol)(&mut guid, ptr::null_mut(), &mut dt_fixup) };
        if status == EFI_SUCCESS {
            let dt_fixup = dt_fixup.cast::<EfiDtFixupProtocol>();
            let mut sz: UINTN = dt_size;
            // SAFETY: the firmware's fixup protocol and our buffer of `sz` bytes.
            let status = unsafe {
                ((*dt_fixup).Fixup)(
                    dt_fixup,
                    addr as usize as *mut c_void,
                    &mut sz,
                    EFI_DT_APPLY_FIXUPS | EFI_DT_RESERVE_MEMORY,
                )
            };
            if status == EFI_BUFFER_TOO_SMALL {
                // SAFETY: the pages allocated above.
                unsafe { (bs().FreePages)(addr, efi_size_to_pages(dt_size)) };
                let _ = lseek(fd, 0, SEEK_SET);
                dt_size = sz;
                continue;
            }
            if status != EFI_SUCCESS {
                libsa::exit::panic(format_args!("DT fixup failed: {:#x}", status));
            }
        }
        break;
    }

    // SAFETY: our pages, holding the tree just read.
    if unsafe { fdt_init(addr as usize as *mut u8) } == 0 {
        printf!("invalid device tree\n");
        // SAFETY: the pages allocated above.
        unsafe { (bs().FreePages)(addr, efi_size_to_pages(dt_size)) };
        return 0;
    }

    let over = FDT_OVERRIDE.load(Ordering::Relaxed);
    if !over.is_null() {
        // SAFETY: the pages a previous call allocated.
        unsafe {
            (bs().FreePages)(
                over as u64,
                efi_size_to_pages(FDT_OVERRIDE_SIZE.load(Ordering::Relaxed)),
            )
        };
        FDT_OVERRIDE.store(ptr::null_mut(), Ordering::Relaxed);
    }

    FDT_OVERRIDE.store(addr as usize as *mut u8, Ordering::Relaxed);
    FDT_OVERRIDE_SIZE.store(dt_size, Ordering::Relaxed);
    0
}

/// `machine acpi`: hand the kernel the device tree made from the ACPI tables.
#[allow(non_snake_case)] // the C's command names
pub fn Xacpi_efi(_cmd: &mut CmdState) -> i32 {
    ACPI.store(true, Ordering::Relaxed);
    0
}

/// `machine dtb [file]`: load a device tree, or drop the loaded one.
#[allow(non_snake_case)] // the C's command names
pub fn Xdtb_efi(cmd: &mut CmdState) -> i32 {
    if cmd.argc == 1 {
        fdt_load_override(cmd, None);
        return 0;
    }

    if cmd.argc != 2 {
        printf!("dtb file\n");
        return 0;
    }

    let file = cmd.arg(1).map(<[u8]>::to_vec).unwrap_or_default();
    fdt_load_override(cmd, Some(&file))
}

/// `machine exit`: back to the firmware.
#[allow(non_snake_case)] // the C's command names
pub fn Xexit_efi(_cmd: &mut CmdState) -> i32 {
    // SAFETY: a boot service with our image handle.
    unsafe { (bs().Exit)(IH.load(Ordering::Relaxed), 0, 0, ptr::null_mut()) };
    loop {
        core::hint::spin_loop();
    }
}

/// `machine fwsetup`: reboot into the firmware's setup.
#[allow(non_snake_case)] // the C's command names
pub fn Xfwsetup_efi(_cmd: &mut CmdState) -> i32 {
    let mut osind: u64 = 0;
    let mut osind_size: UINTN = 8;
    let osind_attrs: u32 = 0x1 | 0x2 | 0x4;
    let mut global = EFI_GLOBAL_VARIABLE;
    let mut supported: [u16; 24] = utf16("OsIndicationsSupported");
    let mut indications: [u16; 14] = utf16("OsIndications");

    // SAFETY: runtime services with valid pointers.
    unsafe {
        let status = (rs().GetVariable)(
            supported.as_mut_ptr(),
            &mut global,
            ptr::null_mut(),
            &mut osind_size,
            (&raw mut osind).cast(),
        );
        if status == EFI_NOT_FOUND {
            printf!("not supported on this machine.\n");
            return -1;
        } else if status != EFI_SUCCESS {
            printf!("Xfwsetup_efi: {}\n", status as i64);
            return -1;
        }

        if (osind & EFI_OS_INDICATIONS_BOOT_TO_FW_UI) == 0 {
            printf!("not supported on this machine.\n");
            return -1;
        }

        osind = EFI_OS_INDICATIONS_BOOT_TO_FW_UI;
        let status = (rs().SetVariable)(
            indications.as_mut_ptr(),
            &mut global,
            osind_attrs,
            8,
            (&raw mut osind).cast(),
        );
        if status != EFI_SUCCESS {
            printf!("Xfwsetup_efi: {}\n", status as i64);
            return -1;
        }

        (rs().ResetSystem)(EfiResetCold, EFI_SUCCESS, 0, ptr::null_mut());
    }
    loop {
        core::hint::spin_loop();
    }
}

/// `machine poweroff`.
#[allow(non_snake_case)] // the C's command names
pub fn Xpoweroff_efi(_cmd: &mut CmdState) -> i32 {
    // SAFETY: a runtime service with valid arguments.
    unsafe { (rs().ResetSystem)(EfiResetShutdown, EFI_SUCCESS, 0, ptr::null_mut()) };
    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_names() {
        assert_eq!(ttydev(b"fb0\0"), makedev(2, 0));
        assert_eq!(ttydev(b"com12"), makedev(1, 21));
        assert_eq!(ttydev(b"cons0"), makedev(0, 0));
        assert_eq!(ttydev(b"tty0"), NODEV);
        assert_eq!(ttydev(b"0"), NODEV);
    }

    #[test]
    fn device_parts() {
        assert_eq!(devopen_args(b"sd0a"), (0, 0));
        assert_eq!(devopen_args(b"sd12d"), (12, 3));
        assert_eq!(devopen_args(b"esp0a"), (0, 0));
        assert_eq!(devopen_args(b""), (0, 0));
        assert_eq!(devopen_args(b"tftp0"), (0, 0));
    }
}
/* </TESTS> */
