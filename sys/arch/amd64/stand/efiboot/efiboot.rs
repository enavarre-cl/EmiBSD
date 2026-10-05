/*	$OpenBSD: efiboot.c,v 1.44 2025/09/16 05:07:33 yasuoka Exp $	*/
/*	$OpenBSD: efiboot.h,v 1.7 2025/08/27 09:08:12 jmatthew Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

//! efiboot's UEFI glue: the entry point, the EFI disks, the memory map, the consoles (EFI
//! text console, EFI serial I/O, legacy COM ports), the boot arguments made from EFI tables,
//! the clock and the EFI `machine` commands.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/efiboot.c @ 3ce1f3f79392,
//! sys/arch/amd64/stand/efiboot/efiboot.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ST`, `BS`, `RS`, `IH`, `efi_bootdp`, `heap`, `mmap_key`, `efi_loadaddr`, `run_i386` and
//!   the console state are atomics (pointers and numbers set once, or by one routine);
//!   `efi_disklist`, `bios_memmap[]`, `bios_efiinfo` and `efi_video[]` are `StaticCell`s
//!   (efiboot is single-threaded and keeps no reference across calls into the firmware's
//!   callbacks).
//! - `efi_main` registers libsa's and boot(8)'s machine-dependent tables (`conf.rs`) and
//!   hands libsa the heap (`heap.h`'s `heap_init`), what the C's link and `alloc.c`'s first
//!   call do; it then moves the stack and calls `boot` with `asm!` as the C does.
//! - `IDLE_POWEROFF` (`get_idle_timeout`, `set_idle_timeout`, `idle_poweroff`, `Xidle_efi`)
//!   is compiled with feature `softraid`, as the C's `SOFTRAID=yes` defines it; its only
//!   caller is softraid's passphrase prompt.
//! - `EFI_DEBUG` (the "Hit any key to reboot" wait in `_rtt`) is not defined, as in the C.

#![allow(non_upper_case_globals)] // the UEFI specification's enumerator names, in patterns

use alloc::vec::Vec;
use core::arch::asm;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use efi::include::efi::*;
use efi::include::eficonsctl::{
    EFI_CONSOLE_CONTROL_PROTOCOL_GUID, EfiConsoleControlProtocol, EfiConsoleControlScreenText,
};
use libkern::staticcell::StaticCell;
use libsa::cons::{cn_tab, cnischar};
use libsa::hdr::cons::{CN_LOWPRI, CN_MIDPRI, ConsDev};
use libsa::hdr::types::{Dev, Time, makedev, minor};
use libsa::printf;
use libsa::sa_alloc::{alloc, free};

use crate::biosvar::{
    BCD_MMIO, BEI_64BIT, BEI_ESRT, BIOS_MAP_ACPI, BIOS_MAP_END, BIOS_MAP_FREE, BIOS_MAP_NVS,
    BIOS_MAP_RES, BOOTARG_CONSDEV, BOOTARG_EFIINFO, BiosConsdev, BiosEfiinfo, BiosMemmap,
};
use crate::disk::DiskInfo;
use crate::efidev::{BIOS_BOOTDEV, efid_init};
use crate::memprobe::{CNVMEM, EXTMEM};
use crate::run_i386::{run_i386_size, run_i386_start};
use boot::bootarg::addbootarg;
use boot::cmd::CmdState;

/// `KERN_LOADSPACE_SIZE`: the room for the kernel `efi_memprobe` reserves.
pub const KERN_LOADSPACE_SIZE: u64 = 64 * 1024 * 1024;

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`.
const EFI_OS_INDICATIONS_BOOT_TO_FW_UI: u64 = 1;

/// `HEAP_LIMIT` (`EFI_HEAP_LIMIT` of `Makefile.common`): the heap and `alloc()` stay below.
pub const HEAP_LIMIT: u64 = 0xc0_0000;

/// `IOM_BEGIN` (`<dev/isa/isareg.h>`): the start of the ISA I/O memory hole.
const IOM_BEGIN: u64 = 0x0a_0000;
/// `IOM_END`: its end.
const IOM_END: u64 = 0x10_0000;

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

/// `struct efi_video`: the size of a text mode.
#[derive(Clone, Copy, Default)]
pub struct EfiVideo {
    /// `cols`.
    pub cols: i32,
    /// `rows`.
    pub rows: i32,
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
/// `heap`: the heap's physical address.
pub static HEAP: AtomicU64 = AtomicU64::new(0);
/// `heapsiz`.
const HEAPSIZ: u64 = 1024 * 1024;
/// `mmap_key`: the key of the last memory map read.
static MMAP_KEY: AtomicUsize = AtomicUsize::new(0);
/// `efi_loadaddr`: where the kernel is loaded before it is moved.
pub static EFI_LOADADDR: AtomicU64 = AtomicU64::new(0);
/// `run_i386`: the copy of `run_i386_start` on the heap.
pub static RUN_I386: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

/// `efi_disklist`: the disks `efi_diskprobe` found, the boot disk first.
#[allow(clippy::vec_box)] // the boxes move to `disklist` whole, keeping their addresses
pub static EFI_DISKLIST: StaticCell<Vec<alloc::boxed::Box<DiskInfo>>> = StaticCell::new(Vec::new());

/// `bios_memmap[128]`.
pub static BIOS_MEMMAP: StaticCell<[BiosMemmap; 128]> = StaticCell::new(
    [BiosMemmap {
        addr: 0,
        size: 0,
        r#type: BIOS_MAP_END,
    }; 128],
);
/// `bios_efiinfo`.
pub static BIOS_EFIINFO: StaticCell<BiosEfiinfo> = StaticCell::new(BiosEfiinfo {
    config_acpi: 0,
    config_smbios: 0,
    fb_addr: 0,
    fb_size: 0,
    fb_height: 0,
    fb_width: 0,
    fb_pixpsl: 0,
    fb_red_mask: 0,
    fb_green_mask: 0,
    fb_blue_mask: 0,
    fb_reserved_mask: 0,
    flags: 0,
    mmap_desc_ver: 0,
    mmap_desc_size: 0,
    mmap_size: 0,
    mmap_start: 0,
    system_table: 0,
    config_esrt: 0,
});
/// `bios_memmap_modified`: the memory map was edited with `machine memory`.
pub static BIOS_MEMMAP_MODIFIED: AtomicI32 = AtomicI32::new(0);

/// `conout`.
static CONOUT: AtomicPtr<SimpleTextOutputInterface> = AtomicPtr::new(ptr::null_mut());
/// `conin`.
static CONIN: AtomicPtr<SimpleInputInterface> = AtomicPtr::new(ptr::null_mut());
/// `gop`.
static GOP: AtomicPtr<EfiGraphicsOutput> = AtomicPtr::new(ptr::null_mut());
/// `efi_video[32]`.
static EFI_VIDEO: StaticCell<[EfiVideo; 32]> = StaticCell::new([EfiVideo { cols: 0, rows: 0 }; 32]);
/// `efi_cons_getc`'s `lastchar`.
static CONS_LASTCHAR: AtomicI32 = AtomicI32::new(0);

/// `com_addr`: the I/O port of the serial console, -1 for `comports[]`.
pub static COM_ADDR: AtomicI32 = AtomicI32::new(-1);
/// `com_speed`.
pub static COM_SPEED: AtomicI32 = AtomicI32::new(-1);
/// `serios[4]`.
static SERIOS: [AtomicPtr<SerialIoInterface>; 4] = [const { AtomicPtr::new(ptr::null_mut()) }; 4];
/// `comports[4]`.
const COMPORTS: [i32; 4] = [0x3f8, 0x2f8, 0x3e8, 0x2e8];
/// `efi_com_getc`'s `lastchar`.
static COM_LASTCHAR: AtomicI32 = AtomicI32::new(0);
/// `gopmode`.
static GOPMODE: AtomicI32 = AtomicI32::new(-1);

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

/// `BS->LocateHandle(ByProtocol, &guid, 0, &sz, handles)`, sized as the C does it: the
/// handles, `alloc`ed (free them with `free(handles, sz)`).
pub fn locate_handles(guid: EfiGuid) -> Result<(*mut EfiHandle, usize), EfiStatus> {
    let mut g = guid;
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
        // SAFETY: `handles` is null or came from `alloc`.
        unsafe { free(handles.cast(), sz as u32) };
        return Err(status);
    }
    Ok((handles, sz))
}

/// `efi_main(image, systab)`: the entry point (`_start`, `start_amd64.S`, calls it).
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

    efi_video_init();
    efi_heap_init();

    let dp0 = handle_protocol::<EfiLoadedImage>(image, LOADED_IMAGE_PROTOCOL).and_then(|imgp| {
        // SAFETY: the firmware's loaded image protocol of our image.
        let dev = unsafe { (*imgp).DeviceHandle };
        handle_protocol::<EfiDevicePath>(dev, DEVICE_PATH_PROTOCOL)
    });
    if let Ok(dp0) = dp0 {
        let mut dp = dp0;
        // SAFETY: the firmware's device path, a valid list of nodes ending with an end node.
        unsafe {
            while !is_device_path_end(dp) {
                let (t, st) = (device_path_type(dp), device_path_sub_type(dp));
                if t == MEDIA_DEVICE_PATH && (st == MEDIA_HARDDRIVE_DP || st == MEDIA_CDROM_DP) {
                    BIOS_BOOTDEV.store(
                        if st == MEDIA_CDROM_DP { 0x1e0 } else { 0x80 },
                        Ordering::Relaxed,
                    );
                    EFI_BOOTDP.store(dp0, Ordering::Relaxed);
                    break;
                } else if t == MESSAGING_DEVICE_PATH && st == MSG_MAC_ADDR_DP {
                    BIOS_BOOTDEV.store(0, Ordering::Relaxed);
                    EFI_BOOTDP.store(dp0, Ordering::Relaxed);
                    break;
                }
                dp = next_device_path_node(dp);
            }
        }
    }

    // allocate run_i386_start() on heap
    let size = run_i386_size();
    let p = alloc(size);
    if p.is_null() {
        libsa::exit::panic(format_args!("alloc() failed"));
    }
    // SAFETY: `p` has `size` bytes; `run_i386_start` is `size` bytes of code.
    unsafe { ptr::copy_nonoverlapping(run_i386_start(), p, size as usize) };
    RUN_I386.store(p, Ordering::Relaxed);

    // can't use sa_cleanup since printf is used after sa_cleanup()
    // SAFETY: single-threaded, before boot().
    unsafe { boot::boot::PROGNAME.write("BOOTX64") };

    // Move the stack before calling boot(). UEFI on some machines locate the stack on our
    // kernel load address.
    let stack = HEAP.load(Ordering::Relaxed) + HEAPSIZ;
    // SAFETY: `stack` is the top of the heap pages efi_heap_init allocated (the C's
    // layout); `boot_entry` never returns, so nothing of this frame is used again.
    unsafe {
        asm!(
            "mov rsp, {stack}",
            "call {boot}",
            stack = in(reg) stack - 32,
            boot = sym boot_entry,
            in("edi") BIOS_BOOTDEV.load(Ordering::Relaxed),
            options(noreturn)
        )
    }
}

/// `boot(bios_bootdev)`, called on the new stack.
extern "C" fn boot_entry(bootdev: Dev) -> ! {
    boot::boot::boot(bootdev)
}

/// `efi_cleanup()`: sync the memory map and leave the boot services.
pub fn efi_cleanup() {
    // retry once in case of failure
    for retry in (0..=1).rev() {
        let _ = efi_memprobe_internal(); // sync the current map
        // SAFETY: a boot service with our image handle and the map key just read.
        let status = unsafe {
            (bs().ExitBootServices)(IH.load(Ordering::Relaxed), MMAP_KEY.load(Ordering::Relaxed))
        };
        if status == EFI_SUCCESS {
            break;
        }
        if retry == 0 {
            libsa::exit::panic(format_args!("ExitBootServices failed ({})", status as i32));
        }
    }
}

/// `efi_diskprobe()`: every EFI block device with media that is not a partition, the one we
/// booted from first.
pub fn efi_diskprobe() {
    let mut bootdev = false;
    let mut depth = -1;

    // SAFETY: single-threaded; the only reference to the list.
    let list = unsafe { EFI_DISKLIST.get_mut() };
    list.clear();

    let (handles, sz) = match locate_handles(BLOCK_IO_PROTOCOL) {
        Ok(h) => h,
        Err(status) => {
            libsa::exit::panic(format_args!("BS->LocateHandle() returns {}", status as i32))
        }
    };

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
                status as i32
            )),
        };

        // SAFETY: the firmware's block I/O protocol and its media.
        let media = unsafe { &*(*blkio).Media };
        if media.LogicalPartition != 0 || media.MediaPresent == 0 {
            continue;
        }
        let di = efid_init(blkio);

        let mut first = false;
        if !bootdp.is_null()
            && depth != -1
            && !bootdev
            && let Ok(dp) = handle_protocol::<EfiDevicePath>(h, DEVICE_PATH_PROTOCOL)
            && efi_device_path_ncmp(bootdp, dp, depth) == 0
        {
            first = true;
            bootdev = true;
        }
        if first {
            list.insert(0, di);
        } else {
            list.push(di);
        }
    }

    // SAFETY: from `locate_handles`.
    unsafe { free(handles.cast(), sz as u32) };
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

/// `efi_heap_init()`: the heap, below `HEAP_LIMIT`.
fn efi_heap_init() {
    let mut heap: EfiPhysicalAddress = HEAP_LIMIT;
    // SAFETY: a boot service with a valid pointer.
    let status = unsafe {
        (bs().AllocatePages)(
            AllocateMaxAddress,
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

/// `efi_memprobe()`: reserve the kernel's room below 256 MB and print the free memory.
pub fn efi_memprobe() {
    let mut addr: EfiPhysicalAddress = 0x1000_0000; // Below 256MB
    // SAFETY: a boot service with a valid pointer.
    let status = unsafe {
        (bs().AllocatePages)(
            AllocateMaxAddress,
            EfiLoaderData,
            efi_size_to_pages(KERN_LOADSPACE_SIZE as usize),
            &mut addr,
        )
    };
    if status != EFI_SUCCESS {
        libsa::exit::panic(format_args!("BS->AllocatePages()"));
    }
    EFI_LOADADDR.store(addr, Ordering::Relaxed);

    printf!(" mem[");
    let error = efi_memprobe_internal();
    let mut n = 0;
    // SAFETY: single-threaded; a copy of the map.
    let map = unsafe { BIOS_MEMMAP.read() };
    for bm in map.iter().take_while(|bm| bm.r#type != BIOS_MAP_END) {
        let (ty, size) = (bm.r#type, bm.size);
        if ty == BIOS_MAP_FREE && size > 12 * 1024 {
            if n != 0 {
                printf!(" ");
            }
            n += 1;
            if size > 1024 * 1024 {
                printf!("{}M", size / 1024 / 1024);
            } else {
                printf!("{}K", size / 1024);
            }
        }
    }
    if error {
        printf!(" overflow");
    }
    printf!("]");
}

/// `efi_memprobe_internal()`: read the EFI memory map into `bios_efiinfo`, and into
/// `bios_memmap` unless the operator edited it; true for the C's `E2BIG`.
fn efi_memprobe_internal() -> bool {
    // SAFETY: single-threaded; the only reference to bios_efiinfo.
    let ei = unsafe { BIOS_EFIINFO.get_mut() };
    if ei.mmap_start != 0 {
        // SAFETY: the previous map, `alloc`ed below.
        unsafe { free(ei.mmap_start as usize as *mut u8, ei.mmap_size) };
    }

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

    MMAP_KEY.store(mapkey, Ordering::Relaxed);

    ei.mmap_desc_ver = mmver;
    ei.mmap_desc_size = mmsiz as u32;
    ei.mmap_size = siz as u32;
    ei.mmap_start = mm as u64;

    if BIOS_MEMMAP_MODIFIED.load(Ordering::Relaxed) != 0 {
        return false;
    }

    efi_update_bios_memmap()
}

/// `efi_update_bios_memmap()`: the EFI map, merged into `bios_memmap` by type; true when it
/// did not fit (`E2BIG`).
fn efi_update_bios_memmap() -> bool {
    let mut error = false;
    // SAFETY: single-threaded; the only references to the two statics.
    let (map, ei) = unsafe { (BIOS_MEMMAP.get_mut(), BIOS_EFIINFO.read()) };
    let (mut cnvmem, mut extmem) = (0u64, 0u64);

    map[0].r#type = BIOS_MAP_END;
    let n = ei.mmap_size / ei.mmap_desc_size;
    let mut mm = ei.mmap_start as usize as *const EfiMemoryDescriptor;
    for _ in 0..n {
        // SAFETY: `mm` is within the map GetMemoryMap filled (n descriptors of the size it
        // said); descriptors may be unaligned for Rust, so they are read unaligned.
        let d = unsafe { mm.read_unaligned() };
        mm = next_memory_descriptor(mm, ei.mmap_desc_size as UINTN);
        let mut bm0 = BiosMemmap {
            addr: d.PhysicalStart,
            size: d.NumberOfPages * EFI_PAGE_SIZE as u64,
            r#type: BIOS_MAP_END,
        };
        bm0.r#type = match d.Type {
            EfiReservedMemoryType
            | EfiUnusableMemory
            | EfiRuntimeServicesCode
            | EfiRuntimeServicesData => BIOS_MAP_RES,
            EfiLoaderCode
            | EfiLoaderData
            | EfiBootServicesCode
            | EfiBootServicesData
            | EfiConventionalMemory => BIOS_MAP_FREE,
            EfiACPIReclaimMemory => BIOS_MAP_ACPI,
            EfiACPIMemoryNVS => BIOS_MAP_NVS,
            // XXX Is there anything to do for EfiMemoryMappedIO
            // XXX EfiMemoryMappedIOPortSpace EfiPalCode?
            _ => BIOS_MAP_RES,
        };

        let mut i = 0;
        while map[i].r#type != BIOS_MAP_END {
            let bm = &mut map[i];
            let (addr, size, ty) = (bm.addr, bm.size, bm.r#type);
            if ty == bm0.r#type {
                if addr <= bm0.addr && bm0.addr <= addr + size {
                    bm.size = bm0.addr + bm0.size - addr;
                    break;
                } else if bm0.addr <= addr && addr <= bm0.addr + bm0.size {
                    bm.size = addr + size - bm0.addr;
                    bm.addr = bm0.addr;
                    break;
                }
            }
            i += 1;
        }
        if map[i].r#type == BIOS_MAP_END {
            if i == map.len() - 1 {
                error = true;
                break;
            }
            map[i] = bm0;
            map[i + 1].r#type = BIOS_MAP_END;
        }
    }
    for bm in map.iter().take_while(|bm| bm.r#type != BIOS_MAP_END) {
        let (addr, size) = (bm.addr, bm.size);
        if addr < IOM_BEGIN {
            // Below memory hole
            cnvmem = cnvmem.max((addr + size) / 1024);
        }
        if addr >= IOM_END && addr / 1024 == extmem + 1024 {
            // Above the memory hole
            extmem += size / 1024;
        }
    }
    CNVMEM.store(cnvmem as u32, Ordering::Relaxed);
    EXTMEM.store(extmem as u32, Ordering::Relaxed);

    error
}

/// `efi_video_init()`: the graphics output, then the text console in its biggest classic
/// mode (100x31, else 80x25).
fn efi_video_init() {
    let mut gop_guid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
    let mut nhandles: UINTN = 0;
    let mut handles: *mut EfiHandle = ptr::null_mut();
    // SAFETY: a boot service with valid pointers.
    let status = unsafe {
        (bs().LocateHandleBuffer)(
            ByProtocol,
            &mut gop_guid,
            ptr::null_mut(),
            &mut nhandles,
            &mut handles,
        )
    };
    if !efi_error(status) {
        let mut first_gop: *mut EfiGraphicsOutput = ptr::null_mut();
        let mut found = false;
        for i in 0..nhandles {
            // SAFETY: the firmware's buffer of `nhandles` handles.
            let h = unsafe { *handles.add(i) };
            let gop = handle_protocol::<EfiGraphicsOutput>(h, EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID)
                .unwrap_or(ptr::null_mut());
            GOP.store(gop, Ordering::Relaxed);
            if first_gop.is_null() {
                first_gop = gop;
            }
            if handle_protocol::<EfiDevicePath>(h, DEVICE_PATH_PROTOCOL).is_ok() {
                found = true;
                break;
            }
        }
        if !found {
            GOP.store(first_gop, Ordering::Relaxed);
        }
        // SAFETY: the firmware's pool buffer.
        unsafe { (bs().FreePool)(handles.cast()) };
    }

    let conout = st().ConOut;
    CONOUT.store(conout, Ordering::Relaxed);
    let mut con_guid = EFI_CONSOLE_CONTROL_PROTOCOL_GUID;
    let mut conctrl: *mut c_void = ptr::null_mut();
    // SAFETY: boot services with valid pointers; the protocol is the firmware's.
    unsafe {
        if (bs().LocateProtocol)(&mut con_guid, ptr::null_mut(), &mut conctrl) == EFI_SUCCESS {
            let cc = conctrl.cast::<EfiConsoleControlProtocol>();
            ((*cc).SetMode)(cc, EfiConsoleControlScreenText);
        }
    }
    let (mut mode80x25, mut mode100x31) = (-1i64, -1i64);
    // SAFETY: the firmware's text output protocol and its mode.
    let maxmode = unsafe { (*(*conout).Mode).MaxMode };
    for i in 0..maxmode.max(0) as usize {
        let (mut cols, mut rows): (UINTN, UINTN) = (0, 0);
        // SAFETY: as above.
        let status = unsafe { ((*conout).QueryMode)(conout, i, &mut cols, &mut rows) };
        if efi_error(status) {
            continue;
        }
        if mode80x25 < 0 && cols == 80 && rows == 25 {
            mode80x25 = i as i64;
        }
        if mode100x31 < 0 && cols == 100 && rows == 31 {
            mode100x31 = i as i64;
        }
        // SAFETY: single-threaded; the only reference to efi_video.
        let video = unsafe { EFI_VIDEO.get_mut() };
        if let Some(v) = video.get_mut(i) {
            v.cols = cols as i32;
            v.rows = rows as i32;
        }
    }
    // SAFETY: as above.
    unsafe {
        if mode100x31 >= 0 {
            ((*conout).SetMode)(conout, mode100x31 as UINTN);
        } else if mode80x25 >= 0 {
            ((*conout).SetMode)(conout, mode80x25 as UINTN);
        }
    }
    CONIN.store(st().ConIn, Ordering::Relaxed);
    efi_video_reset();
}

/// `efi_video_reset()`.
fn efi_video_reset() {
    let conout = CONOUT.load(Ordering::Relaxed);
    // SAFETY: the firmware's text output protocol.
    unsafe {
        ((*conout).EnableCursor)(conout, TRUE);
        ((*conout).SetAttribute)(conout, efi_text_attr(EFI_LIGHTGRAY, EFI_BLACK));
        ((*conout).ClearScreen)(conout);
    }
}

/// `efi_cons_probe(cn)`: the EFI text console, `pc0`.
pub fn efi_cons_probe(cn: &ConsDev) {
    cn.set_pri(CN_MIDPRI);
    cn.set_dev(makedev(12, 0));
    printf!(" pc{}", minor(cn.dev()));
}

/// `efi_cons_init(cp)`.
pub fn efi_cons_init(_cp: &ConsDev) {}

/// `efi_cons_getc(dev)`: the next key (waiting for one), or with `0x80` in `dev` whether one
/// is waiting (kept for the next call).
pub fn efi_cons_getc(dev: Dev) -> i32 {
    let last = CONS_LASTCHAR.load(Ordering::Relaxed);
    if last != 0 {
        if (dev & 0x80) == 0 {
            CONS_LASTCHAR.store(0, Ordering::Relaxed);
        }
        return last;
    }

    let conin = CONIN.load(Ordering::Relaxed);
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
        let mut dummy: UINTN = 0;
        // SAFETY: as above; WaitForKey is the protocol's event.
        unsafe {
            let mut ev = (*conin).WaitForKey;
            (bs().WaitForEvent)(1, &mut ev, &mut dummy);
            status = ((*conin).ReadKeyStroke)(conin, &mut key);
        }
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

    let mut buf: [CHAR16; 2] = [c as CHAR16, 0];
    let conout = CONOUT.load(Ordering::Relaxed);
    // SAFETY: the firmware's text output protocol and a NUL-terminated string.
    unsafe { ((*conout).OutputString)(conout, buf.as_mut_ptr()) };
}

/// `efi_cons_getshifts(dev)`: XXX.
pub fn efi_cons_getshifts(_dev: Dev) -> i32 {
    0
}

/// `outb(port, v)`.
fn outb(port: i32, v: u8) {
    // SAFETY: an I/O port write to a UART register; efiboot runs in ring 0.
    unsafe {
        asm!("out dx, al", in("dx") port as u16, in("al") v, options(nomem, nostack, preserves_flags))
    };
}

/// `inb(port)`.
fn inb(port: i32) -> u8 {
    let v: u8;
    // SAFETY: an I/O port read of a UART register; efiboot runs in ring 0.
    unsafe {
        asm!("in al, dx", in("dx") port as u16, out("al") v, options(nomem, nostack, preserves_flags))
    };
    v
}

/// `com_data`, `com_dlbl`, `com_dlbh`, `com_cfcr`, `com_lsr` (`<dev/ic/comreg.h>`).
const COM_DATA: i32 = 0;
const COM_DLBL: i32 = 0;
const COM_DLBH: i32 = 1;
const COM_CFCR: i32 = 3;
const COM_LSR: i32 = 5;
/// `LCR_DLAB`, `LCR_8BITS`, `LSR_RXRDY`, `LSR_TXRDY`, `COM_FREQ`, `COM_TOLERANCE`.
const LCR_DLAB: u8 = 0x80;
const LCR_8BITS: u8 = 0x03;
const LSR_RXRDY: u8 = 0x01;
const LSR_TXRDY: u8 = 0x20;
const COM_FREQ: i32 = 1_843_200;
const COM_TOLERANCE: i32 = 30;

/// The port of COM unit `unit`.
fn com_port(unit: u32) -> i32 {
    match COM_ADDR.load(Ordering::Relaxed) {
        -1 => COMPORTS[(unit as usize) & 3],
        a => a,
    }
}

/// `pio_comspeed(dev, sp)`: set the legacy UART's speed (`sp <= 0`: the current one).
pub fn pio_comspeed(dev: Dev, sp: i32) -> i32 {
    let port = com_port(minor(dev));

    if sp <= 0 {
        return COM_SPEED.load(Ordering::Relaxed);
    }
    // valid baud rate?
    if !(75..=115_200).contains(&sp) {
        return -1;
    }

    // Accepted speeds: 75 150 300 600 1200 2400 4800 9600 19200 38400 76800 and
    // 14400 28800 57600 115200
    let mut i = sp;
    while i != 75 && i != 14400 {
        if i & 1 != 0 {
            return -1;
        }
        i >>= 1;
    }

    // ripped screaming from dev/ic/com.c
    let divrnd = |n: i32, q: i32| (n * 2 / q + 1) / 2; // divide and round off
    let newsp = divrnd(COM_FREQ / 16, sp);
    if newsp <= 0 {
        return -1;
    }
    let err = (divrnd((COM_FREQ / 16) * 1000, sp * newsp) - 1000).abs();
    if err > COM_TOLERANCE {
        return -1;
    }

    let speed = COM_SPEED.load(Ordering::Relaxed);
    if speed != -1 && cn_tab().is_some_and(|cn| cn.dev() == dev) && speed != sp {
        printf!(
            "com{}: changing speed to {} baud in 5 seconds, change your terminal to match!\n\x07",
            minor(dev),
            sp
        );
        sleep(5);
    }

    outb(port + COM_CFCR, LCR_DLAB);
    outb(port + COM_DLBL, newsp as u8);
    outb(port + COM_DLBH, (newsp >> 8) as u8);
    outb(port + COM_CFCR, LCR_8BITS);
    if speed != -1 {
        printf!("\ncom{}: {} baud\n", minor(dev), sp);
    }

    COM_SPEED.store(sp, Ordering::Relaxed);
    speed
}

/// `pio_com_getc(dev)`.
pub fn pio_com_getc(dev: Dev) -> i32 {
    let port = com_port(minor(dev & 0x7f));

    if (dev & 0x80) != 0 {
        return i32::from(inb(port + COM_LSR) & LSR_RXRDY);
    }

    while (inb(port + COM_LSR) & LSR_RXRDY) == 0 {
        core::hint::spin_loop();
    }

    i32::from(inb(port + COM_DATA))
}

/// `pio_com_putc(dev, c)`.
pub fn pio_com_putc(dev: Dev, c: i32) {
    let port = com_port(minor(dev));

    while (inb(port + COM_LSR) & LSR_TXRDY) == 0 {
        core::hint::spin_loop();
    }

    outb(port + COM_DATA, c as u8);
}

/// `efi_com_probe(cn)`: the EFI serial I/O protocols of the legacy COM ports (ACPI
/// `PNP0501`, UID 0-3), `com0`..`com3`.
pub fn efi_com_probe(cn: &ConsDev) {
    cn.set_pri(CN_LOWPRI);
    cn.set_dev(makedev(8, 0));

    let Ok((handles, sz)) = locate_handles(SERIAL_IO_PROTOCOL) else {
        return;
    };

    for i in 0..sz / core::mem::size_of::<EfiHandle>() {
        // SAFETY: `handles` holds `sz` bytes of handles.
        let h = unsafe { *handles.add(i) };
        // Identify port number of the handle. This assumes ACPI UID 0-3 map to legacy
        // COM[1-4] and they use the legacy port address.
        let Ok(dp0) = handle_protocol::<EfiDevicePath>(h, DEVICE_PATH_PROTOCOL) else {
            continue;
        };
        let mut uid: i64 = -1;
        let mut dp = dp0.cast_const();
        // SAFETY: the firmware's device path; an ACPI node is an `AcpiHidDevicePath`, read
        // unaligned.
        unsafe {
            while !is_device_path_end(dp) {
                if device_path_type(dp) == ACPI_DEVICE_PATH && device_path_sub_type(dp) == ACPI_DP {
                    let acpi = dp.cast::<AcpiHidDevicePath>().read_unaligned();
                    if acpi.HID == efi_pnp_id(0x0501) {
                        uid = i64::from(acpi.UID);
                        break;
                    }
                }
                dp = next_device_path_node(dp);
            }
        }
        if uid < 0 || uid as usize >= SERIOS.len() {
            continue;
        }

        // Prepare SERIAL_IO_INTERFACE
        let Ok(serio) = handle_protocol::<SerialIoInterface>(h, SERIAL_IO_PROTOCOL) else {
            continue;
        };
        SERIOS[uid as usize].store(serio, Ordering::Relaxed);
    }
    // SAFETY: from `locate_handles`.
    unsafe { free(handles.cast(), sz as u32) };

    for (i, s) in SERIOS.iter().enumerate() {
        if !s.load(Ordering::Relaxed).is_null() {
            printf!(" com{}", i);
        }
    }
}

/// `efi_valid_com(dev)`.
pub fn efi_valid_com(dev: Dev) -> bool {
    SERIOS
        .get(minor(dev) as usize)
        .is_some_and(|s| !s.load(Ordering::Relaxed).is_null())
}

/// `comspeed(dev, sp)`: the speed of a COM console (`sp <= 0` to ask).
pub fn comspeed(dev: Dev, sp: i32) -> i32 {
    if sp <= 0 {
        return COM_SPEED.load(Ordering::Relaxed);
    }

    if !efi_valid_com(dev) {
        return pio_comspeed(dev, sp);
    }
    let serio = SERIOS[minor(dev) as usize].load(Ordering::Relaxed);

    // SAFETY: the firmware's serial I/O protocol and its mode.
    unsafe {
        let mode = &*(*serio).Mode;
        if mode.BaudRate != sp as u64 {
            let status = ((*serio).SetAttributes)(
                serio,
                sp as u64,
                mode.ReceiveFifoDepth,
                mode.Timeout,
                mode.Parity,
                mode.DataBits as u8,
                mode.StopBits,
            );
            if efi_error(status) {
                printf!(
                    "com{}: SetAttribute() failed with status={}\n",
                    minor(dev),
                    status as i32
                );
                return -1;
            }
            if COM_SPEED.load(Ordering::Relaxed) != -1 {
                printf!("\ncom{}: {} baud\n", minor(dev), sp);
            }
        }
    }

    // same as comspeed() in libsa/bioscons.c
    COM_SPEED.swap(sp, Ordering::Relaxed)
}

/// `efi_com_init(cn)`: 9600 baud by default.
pub fn efi_com_init(cn: &ConsDev) {
    if !efi_valid_com(cn.dev()) {
        // This actually happens if the machine has another serial.
        return;
    }

    if COM_SPEED.load(Ordering::Relaxed) == -1 {
        comspeed(cn.dev(), 9600); // default speed is 9600 baud
    }
}

/// `efi_com_getc(dev)`.
pub fn efi_com_getc(dev: Dev) -> i32 {
    if !efi_valid_com(dev & 0x7f) {
        return pio_com_getc(dev);
    }
    let serio = SERIOS[minor(dev & 0x7f) as usize].load(Ordering::Relaxed);

    let last = COM_LASTCHAR.load(Ordering::Relaxed);
    if last != 0 {
        if (dev & 0x80) == 0 {
            COM_LASTCHAR.store(0, Ordering::Relaxed);
        }
        return last;
    }

    let mut buf: u8 = 0;
    loop {
        let mut sz: UINTN = 1;
        // SAFETY: the firmware's serial I/O protocol, a one-byte buffer.
        let status = unsafe { ((*serio).Read)(serio, &mut sz, (&raw mut buf).cast()) };
        if status == EFI_SUCCESS && sz > 0 {
            break;
        }
        if status != EFI_TIMEOUT && efi_error(status) {
            libsa::exit::panic(format_args!(
                "Error reading from serial status={}",
                status as i32
            ));
        }
        if (dev & 0x80) != 0 {
            return 0;
        }
    }

    if (dev & 0x80) != 0 {
        COM_LASTCHAR.store(i32::from(buf), Ordering::Relaxed);
    }

    i32::from(buf)
}

/// `efi_com_putc(dev, c)`.
pub fn efi_com_putc(dev: Dev, c: i32) {
    if !efi_valid_com(dev) {
        pio_com_putc(dev, c);
        return;
    }
    let serio = SERIOS[minor(dev) as usize].load(Ordering::Relaxed);
    let mut sz: UINTN = 1;
    let mut buf = c as u8;
    // SAFETY: the firmware's serial I/O protocol, a one-byte buffer.
    unsafe { ((*serio).Write)(serio, &mut sz, (&raw mut buf).cast()) };
}

/// `efi_gop_setmode(mode)`.
fn efi_gop_setmode(mode: u32) -> EfiStatus {
    let gop = GOP.load(Ordering::Relaxed);
    // SAFETY: the firmware's graphics output protocol.
    unsafe {
        let status = ((*gop).SetMode)(gop, mode);
        if efi_error(status) || (*(*gop).Mode).Mode != mode {
            printf!("GOP SetMode() failed ({})\n", status as i32);
        }
        status
    }
}

/// `efi_makebootargs()`: `BOOTARG_EFIINFO`: the ACPI, SMBIOS and ESRT tables, the frame
/// buffer (in its biggest mode) and the system table.
pub fn efi_makebootargs() {
    // SAFETY: single-threaded; the only reference to bios_efiinfo.
    let ei = unsafe { BIOS_EFIINFO.get_mut() };

    // ACPI, BIOS configuration table
    let st = st();
    for i in 0..st.NumberOfTableEntries {
        // SAFETY: the firmware's table of `NumberOfTableEntries` entries.
        let ct = unsafe { &*st.ConfigurationTable.add(i) };
        if ct.VendorGuid == ACPI_20_TABLE_GUID {
            ei.config_acpi = ct.VendorTable as u64;
        } else if ct.VendorGuid == SMBIOS_TABLE_GUID {
            ei.config_smbios = ct.VendorTable as u64;
        } else if ct.VendorGuid == EFI_SYSTEM_RESOURCE_TABLE_GUID {
            ei.config_esrt = ct.VendorTable as u64;
        }
    }

    // Need to copy ESRT because call to ExitBootServices() frees memory of type
    // EfiBootServicesData in which ESRT resides.
    if ei.config_esrt != 0 {
        let esrt = ei.config_esrt as usize as *const EfiSystemResourceTable;
        // SAFETY: the firmware's ESRT, followed by its entries.
        let esrt_size = core::mem::size_of::<EfiSystemResourceTable>()
            + unsafe { (*esrt).FwResourceCount } as usize
                * core::mem::size_of::<EfiSystemResourceEntry>();
        let mut esrt_copy: *mut c_void = ptr::null_mut();

        // Using EfiRuntimeServicesData as it maps to BIOS_MAP_RES, while EfiLoaderData
        // becomes BIOS_MAP_FREE.
        // SAFETY: a boot service with a valid pointer; the copy has `esrt_size` bytes.
        unsafe {
            if (bs().AllocatePool)(EfiRuntimeServicesData, esrt_size, &mut esrt_copy) == EFI_SUCCESS
            {
                ptr::copy_nonoverlapping(esrt.cast::<u8>(), esrt_copy.cast::<u8>(), esrt_size);
                ei.config_esrt = esrt_copy as u64;
                ei.flags |= BEI_ESRT;
            }
        }
    }

    // Frame buffer
    let gop = GOP.load(Ordering::Relaxed);
    if !gop.is_null() {
        // SAFETY: the firmware's graphics output protocol and its modes.
        unsafe {
            if GOPMODE.load(Ordering::Relaxed) < 0 {
                let mut bestsiz: u64 = 0;
                for i in 0..(*(*gop).Mode).MaxMode {
                    let mut sz: UINTN = 0;
                    let mut gopi: *mut EfiGraphicsOutputModeInformation = ptr::null_mut();
                    let status = ((*gop).QueryMode)(gop, i, &mut sz, &mut gopi);
                    if efi_error(status) {
                        continue;
                    }
                    let gopsiz = u64::from((*gopi).HorizontalResolution)
                        * u64::from((*gopi).VerticalResolution);
                    if gopsiz > bestsiz {
                        GOPMODE.store(i as i32, Ordering::Relaxed);
                        bestsiz = gopsiz;
                    }
                }
            }
            let gopmode = GOPMODE.load(Ordering::Relaxed);
            if gopmode >= 0 && gopmode as u32 != (*(*gop).Mode).Mode {
                let curmode = (*(*gop).Mode).Mode;
                if efi_gop_setmode(gopmode as u32) != EFI_SUCCESS {
                    let _ = efi_gop_setmode(curmode);
                }
            }

            let gopi = &*(*(*gop).Mode).Info;
            match gopi.PixelFormat {
                PixelBlueGreenRedReserved8BitPerColor => {
                    ei.fb_red_mask = 0x00ff_0000;
                    ei.fb_green_mask = 0x0000_ff00;
                    ei.fb_blue_mask = 0x0000_00ff;
                    ei.fb_reserved_mask = 0xff00_0000;
                }
                PixelRedGreenBlueReserved8BitPerColor => {
                    ei.fb_red_mask = 0x0000_00ff;
                    ei.fb_green_mask = 0x0000_ff00;
                    ei.fb_blue_mask = 0x00ff_0000;
                    ei.fb_reserved_mask = 0xff00_0000;
                }
                PixelBitMask => {
                    ei.fb_red_mask = gopi.PixelInformation.RedMask;
                    ei.fb_green_mask = gopi.PixelInformation.GreenMask;
                    ei.fb_blue_mask = gopi.PixelInformation.BlueMask;
                    ei.fb_reserved_mask = gopi.PixelInformation.ReservedMask;
                }
                _ => {}
            }
            ei.fb_addr = (*(*gop).Mode).FrameBufferBase;
            ei.fb_size = (*(*gop).Mode).FrameBufferSize as u64;
            ei.fb_height = gopi.VerticalResolution;
            ei.fb_width = gopi.HorizontalResolution;
            ei.fb_pixpsl = gopi.PixelsPerScanLine;
        }
    }

    // EFI system table
    ei.system_table = ST.load(Ordering::Relaxed) as u64;
    ei.flags |= BEI_64BIT;

    addbootarg(BOOTARG_EFIINFO, ei.as_bytes());
}

/// The vendor device path used to indicate the mmio UART on AMD SoCs.
const AMDSOC_DEVPATH: EfiGuid = EfiGuid::new(
    0xe76f_d4e9,
    0x0a30,
    0x4ca9,
    [0x95, 0x40, 0xd7, 0x99, 0x53, 0x4c, 0xc4, 0xff],
);

/// `efi_setconsdev()`: `BOOTARG_CONSDEV`: the console, and the baud rate of a serial one
/// from the `ConOut` variable.
pub fn efi_setconsdev() {
    let mut cd = BiosConsdev {
        consdev: cn_tab().map_or(0, ConsDev::dev),
        conspeed: COM_SPEED.load(Ordering::Relaxed),
        consaddr: COM_ADDR.load(Ordering::Relaxed) as i64 as u64,
        ..Default::default()
    };
    let mut data = [0u8; 128];
    let mut size: UINTN = data.len();
    let mut global = EFI_GLOBAL_VARIABLE;
    let mut name: [u16; 7] = utf16("ConOut");

    // If the ConOut variable indicates we're using a serial console, use it to determine
    // the baud rate.
    // SAFETY: a runtime service with valid pointers; the variable is a device path, walked
    // within the bytes it filled, its nodes read unaligned.
    unsafe {
        let status = (rs().GetVariable)(
            name.as_mut_ptr(),
            &mut global,
            ptr::null_mut(),
            &mut size,
            data.as_mut_ptr().cast(),
        );
        if status == EFI_SUCCESS {
            let mut dp = data.as_ptr().cast::<EfiDevicePath>();
            while !is_device_path_end(dp) {
                // AMD Ryzen Embedded V1000 SoCs integrate a Synopsys DesignWare UART that is
                // not compatible with the traditional 8250 UART found on the IBM PC. Pass
                // the magic parameters to the kernel to make this UART work.
                if device_path_type(dp) == HARDWARE_DEVICE_PATH
                    && device_path_sub_type(dp) == HW_VENDOR_DP
                {
                    let vdp = dp.cast::<VendorDevicePath>();
                    if vdp.read_unaligned().Guid == AMDSOC_DEVPATH {
                        cd.consdev = makedev(8, 4);
                        cd.consaddr = vdp.add(1).cast::<u64>().read_unaligned();
                        cd.consfreq = 48_000_000;
                        cd.flags = BCD_MMIO;
                        cd.reg_width = 4;
                        cd.reg_shift = 2;
                    }
                }

                if device_path_type(dp) == MESSAGING_DEVICE_PATH
                    && device_path_sub_type(dp) == MSG_UART_DP
                {
                    let udp = dp.cast::<UartDevicePath>().read_unaligned();
                    if cd.conspeed == -1 {
                        cd.conspeed = udp.BaudRate as i32;
                    }
                }
                dp = next_device_path_node(dp);
            }
        }
    }

    addbootarg(BOOTARG_CONSDEV, cd.as_bytes());
}

/// `_rtt()`: reset the machine.
pub fn _rtt() -> ! {
    // SAFETY: a runtime service with valid arguments.
    unsafe { (rs().ResetSystem)(EfiResetCold, EFI_SUCCESS, 0, ptr::null_mut()) };
    loop {
        core::hint::spin_loop();
    }
}

/// `getsecs()`: seconds since the Epoch, from the firmware's clock.
pub fn getsecs() -> Time {
    const DAYTAB: [[i64; 14]; 2] = [
        [
            0, -1, 30, 58, 89, 119, 150, 180, 211, 242, 272, 303, 333, 364,
        ],
        [
            0, -1, 30, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365,
        ],
    ];
    let isleap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);

    // SAFETY: an all-zero EFI_TIME is a valid value of the plain struct.
    let mut t: EfiTime = unsafe { core::mem::zeroed() };
    // SAFETY: a runtime service with a valid pointer.
    unsafe { (rs().GetTime)(&mut t, ptr::null_mut()) };

    // Calc days from UNIX epoch
    let year = i64::from(t.Year);
    let mut r: Time = (year - 1970) * 365;
    for y in 1970..year {
        if isleap(y) {
            r += 1;
        }
    }
    r += DAYTAB[usize::from(isleap(year))][usize::from(t.Month).min(13)] + i64::from(t.Day);

    // Calc secs
    r *= 60 * 60 * 24;
    r += ((i64::from(t.Hour) * 60) + i64::from(t.Minute)) * 60 + i64::from(t.Second);
    if -24 * 60 < t.TimeZone && t.TimeZone < 24 * 60 {
        r += i64::from(t.TimeZone) * 60;
    }

    r
}

/// `sleep(i)`: wait `i` seconds, polling the console; non-zero if a key came.
pub fn sleep(i: u32) -> u32 {
    let t = getsecs() + Time::from(i);
    let mut intr = 0;
    while intr == 0 && getsecs() < t {
        intr = cnischar();
    }
    intr as u32
}

#[cfg(feature = "softraid")]
mod idle {
    //! `IDLE_POWEROFF` (with `SOFTRAID`): power off after a time without input.
    use super::*;

    /// `idle_name`.
    const IDLE_NAME: [u16; 13] = utf16("IdlePoweroff");
    /// `openbsd_guid`: randomly generated f948e8a9-0570-4338-ad10-29f4cf12849d.
    const OPENBSD_GUID: EfiGuid = EfiGuid::new(
        0xf948_e8a9,
        0x0570,
        0x4338,
        [0xad, 0x10, 0x29, 0xf4, 0xcf, 0x12, 0x84, 0x9d],
    );
    /// `idle_attrs`: non-volatile, boot service access, runtime service access.
    const IDLE_ATTRS: u32 = 0x1 | 0x2 | 0x4;
    /// `idle_secs`.
    static IDLE_SECS: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

    /// `get_idle_timeout()`: 0 with `IDLE_SECS` read, -1 if unset, 1 on error.
    pub fn get_idle_timeout() -> i32 {
        let mut name = IDLE_NAME;
        let mut guid = OPENBSD_GUID;
        let mut secs: u16 = 0;
        let mut sz: UINTN = 2;
        // SAFETY: a runtime service with valid pointers.
        let status = unsafe {
            (rs().GetVariable)(
                name.as_mut_ptr(),
                &mut guid,
                ptr::null_mut(),
                &mut sz,
                (&raw mut secs).cast(),
            )
        };
        if status != EFI_SUCCESS {
            if status != EFI_NOT_FOUND {
                printf!("get_idle_timeout: {}\n", status as i32);
                return 1;
            }
            return -1;
        }
        IDLE_SECS.store(secs, Ordering::Relaxed);
        0
    }

    /// `set_idle_timeout(secs)`.
    pub fn set_idle_timeout(secs: i32) -> i32 {
        let mut name = IDLE_NAME;
        let mut guid = OPENBSD_GUID;
        let mut s = secs as u16;
        IDLE_SECS.store(s, Ordering::Relaxed);
        let sz = if s > 0 { 2 } else { 0 };
        // SAFETY: a runtime service with valid pointers.
        let status = unsafe {
            (rs().SetVariable)(
                name.as_mut_ptr(),
                &mut guid,
                IDLE_ATTRS,
                sz,
                (&raw mut s).cast(),
            )
        };
        if status != EFI_SUCCESS {
            printf!("set_idle_timeout: {}\n", status as i32);
            return -1;
        }
        0
    }

    /// `idle_poweroff()`: see lib/libsa/softraid.c `sr_crypto_passphrase_decrypt()`.
    pub fn idle_poweroff() {
        if get_idle_timeout() == 0 && sleep(u32::from(IDLE_SECS.load(Ordering::Relaxed))) == 0 {
            printf!(
                "\nno input after {}s, powering off...\n",
                IDLE_SECS.load(Ordering::Relaxed)
            );
            super::Xpoweroff_efi(&mut CmdState::new());
        }
    }

    /// `machine idle [secs]`.
    #[allow(non_snake_case)] // the C's command names
    pub fn Xidle_efi(cmd: &mut CmdState) -> i32 {
        if cmd.argc >= 2 {
            let secs = libsa::strtol::strtol(cmd.arg(1).unwrap_or(b""), 10).0 as i32;
            if (0..i32::from(u16::MAX)).contains(&secs) {
                set_idle_timeout(secs);
            }
        } else if get_idle_timeout() == 0 {
            printf!("Timeout = {}s\n", IDLE_SECS.load(Ordering::Relaxed));
        }
        0
    }
}
#[cfg(feature = "softraid")]
pub use idle::Xidle_efi;

/// `machine exit`: back to the firmware.
#[allow(non_snake_case)] // the C's command names
pub fn Xexit_efi(_cmd: &mut CmdState) -> i32 {
    // SAFETY: a boot service with our image handle.
    unsafe { (bs().Exit)(IH.load(Ordering::Relaxed), 0, 0, ptr::null_mut()) };
    loop {
        core::hint::spin_loop();
    }
}

/// `machine video [mode]`: the text modes, or switch to one.
#[allow(non_snake_case)] // the C's command names
pub fn Xvideo_efi(cmd: &mut CmdState) -> i32 {
    let conout = CONOUT.load(Ordering::Relaxed);
    // SAFETY: single-threaded; a copy of the table.
    let video = unsafe { EFI_VIDEO.read() };
    // SAFETY: the firmware's text output protocol and its mode.
    unsafe {
        if cmd.argc >= 2 {
            let mode = libsa::strtol::strtol(cmd.arg(1).unwrap_or(b""), 10).0;
            if 0 <= mode && (mode as usize) < video.len() && video[mode as usize].cols > 0 {
                ((*conout).SetMode)(conout, mode as UINTN);
                efi_video_reset();
            }
        } else {
            let maxmode = (*(*conout).Mode).MaxMode.max(0) as usize;
            for (i, v) in video.iter().enumerate().take(maxmode) {
                if v.cols > 0 {
                    printf!("Mode {}: {} x {}\n", i, v.cols, v.rows);
                }
            }
            printf!("\n");
        }
        printf!("Current Mode = {}\n", (*(*conout).Mode).Mode);
    }
    0
}

/// `machine poweroff`.
#[allow(non_snake_case)] // the C's command names
pub fn Xpoweroff_efi(_cmd: &mut CmdState) -> i32 {
    // SAFETY: a runtime service with valid arguments.
    unsafe { (rs().ResetSystem)(EfiResetShutdown, EFI_SUCCESS, 0, ptr::null_mut()) };
    0
}

/// `machine gop [mode]`: the graphics modes, or switch to one.
#[allow(non_snake_case)] // the C's command names
pub fn Xgop_efi(cmd: &mut CmdState) -> i32 {
    let gop = GOP.load(Ordering::Relaxed);
    if gop.is_null() {
        printf!("No GOP found\n");
        return 0;
    }
    // SAFETY: the firmware's graphics output protocol and its modes.
    unsafe {
        let maxmode = (*(*gop).Mode).MaxMode;
        let mut sz: UINTN = 0;
        let mut gopi: *mut EfiGraphicsOutputModeInformation = ptr::null_mut();
        if cmd.argc >= 2 {
            let mode = libsa::strtol::strtol(cmd.arg(1).unwrap_or(b""), 10).0;
            if 0 <= mode && (mode as u32) < maxmode {
                let status = ((*gop).QueryMode)(gop, mode as u32, &mut sz, &mut gopi);
                if !efi_error(status) && efi_gop_setmode(mode as u32) == EFI_SUCCESS {
                    GOPMODE.store(mode as i32, Ordering::Relaxed);
                }
            }
        } else {
            for i in 0..maxmode {
                let status = ((*gop).QueryMode)(gop, i, &mut sz, &mut gopi);
                if efi_error(status) {
                    continue;
                }
                printf!(
                    "Mode {}: {} x {} (stride = {})\n",
                    i,
                    (*gopi).HorizontalResolution,
                    (*gopi).VerticalResolution,
                    (*gopi).PixelsPerScanLine
                );
            }
            printf!("\n");
        }
        printf!("Current Mode = {}\n", (*(*gop).Mode).Mode);
    }
    0
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
            printf!("Xfwsetup_efi: {}\n", status as i32);
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
            printf!("Xfwsetup_efi: {}\n", status as i32);
            return -1;
        }

        (rs().ResetSystem)(EfiResetCold, EFI_SUCCESS, 0, ptr::null_mut());
    }
    loop {
        core::hint::spin_loop();
    }
}
