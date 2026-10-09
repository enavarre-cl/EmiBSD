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
//! Boot glue: the kernel's two entries, Limine's and OpenBSD boot(8)'s (M14), both ending in
//! [`start_kernel`].
//!
//! `_start` is where Limine jumps (its entry point request; amd64's ELF entry is
//! `locore0.S`'s `start`, for boot(8)). It checks the protocol revision, turns the
//! bootloader's responses into the bootloader-neutral [`BootInfo`] and calls
//! [`start_kernel`], which sets `boothowto` (the command line's flags and the loader's
//! `howto`), lets the machine do its earliest setup (OpenBSD's `init_x86_64` / `initarm`: the
//! message buffer and the console) and hands over to `kern::init_main::main`. Nothing outside
//! this module names a Limine type. `bootarg.rs` is the other entry: the machine's own code
//! (amd64's `locore0.S`) calls `bootarg_main`, which takes the [`BootInfo`] from
//! `Machine::getbootinfo` (boot(8)'s `bootarg` list) and calls [`start_kernel`] too.

mod bootarg;
#[cfg(target_arch = "aarch64")]
mod fdtfb;
mod limine;

use core::ptr::NonNull;

use bsd::dev::rd::rd_root_image_set;
use bsd::kern::init_main::{self, BOOTHOWTO};
use bsd::kern::subr_prf::Str;
use bsd::kprintf;
#[cfg(feature = "multiprocessor")]
use bsd::machine::BootCpu;
use bsd::machine::{
    BootFramebuffer, BootInfo, BootModule, BootMp, Cpu, EfiMemmap, Exit, ExitStatus, MAX_MODULES,
    Machine, MachineInfo, MemKind, MemMap, MemRegion,
};
use bsd::sys::types::{Paddr, Psize, Vaddr};

use limine::{
    BaseRevision, BootloaderInfoResponse, DtbResponse, EfiMemmapResponse, EfiSystemTableResponse,
    EntryPointRequest, ExecutableAddressResponse, ExecutableCmdlineResponse, FramebufferResponse,
    HhdmResponse, MemmapResponse, ModuleResponse, Request, RequestsEndMarker, RequestsStartMarker,
    RsdpResponse, SmbiosResponse, StackSizeRequest, id, memmap_type,
};

/// Boot stack for the boot CPU: the protocol's minimum, more than OpenBSD's `USPACE`.
const STACK_SIZE_BYTES: u64 = 64 * 1024;

/// Why the boot glue gave up before a console existed. Each ends in a failure exit; under QEMU
/// the exit status (`ExitStatus::Failure`) is the only trace, so the message `early_init`
/// returns is dropped here until a console-less channel (semihosting) can carry it.
#[derive(Debug)]
enum BootError {
    UnsupportedRevision,
    MissingHhdm,
    MissingMemmap,
    MissingExecutableAddress,
    TooManyRegions,
    EarlyInit,
}

// The requests. `#[used]` keeps them in the object file and `KEEP(*(.requests*))` in the linker
// script keeps them in the image; they are also all read below, from `_start`.

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new(limine::BASE_REVISION);

/// Limine enters `_start`, not the ELF entry point (amd64's is boot(8)'s `start`).
#[used]
#[unsafe(link_section = ".requests")]
static ENTRY_POINT: EntryPointRequest = EntryPointRequest::new(_start);

#[used]
#[unsafe(link_section = ".requests")]
static BOOTLOADER_INFO: Request<BootloaderInfoResponse> = Request::new(id::BOOTLOADER_INFO);

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE: StackSizeRequest = StackSizeRequest::new(STACK_SIZE_BYTES);

#[used]
#[unsafe(link_section = ".requests")]
static HHDM: Request<HhdmResponse> = Request::new(id::HHDM);

#[used]
#[unsafe(link_section = ".requests")]
static MEMMAP: Request<MemmapResponse> = Request::new(id::MEMMAP);

#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE_ADDRESS: Request<ExecutableAddressResponse> =
    Request::new(id::EXECUTABLE_ADDRESS);

#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE_CMDLINE: Request<ExecutableCmdlineResponse> =
    Request::new(id::EXECUTABLE_CMDLINE);

#[used]
#[unsafe(link_section = ".requests")]
static RSDP: Request<RsdpResponse> = Request::new(id::RSDP);

#[used]
#[unsafe(link_section = ".requests")]
static DTB: Request<DtbResponse> = Request::new(id::DTB);

/// The modules (`init`, M6; `ramdisk.ffs`, the image of rd(4), M8).
#[used]
#[unsafe(link_section = ".requests")]
static EFI_SYSTEM_TABLE: Request<EfiSystemTableResponse> = Request::new(id::EFI_SYSTEM_TABLE);

/// The SMBIOS entry point (M16e: amd64's bios0, as efiboot's `SMBIOS_TABLE_GUID`
/// configuration table in `bios_efiinfo->config_smbios`).
#[used]
#[unsafe(link_section = ".requests")]
static SMBIOS: Request<SmbiosResponse> = Request::new(id::SMBIOS);

/// The frame buffer the firmware's GOP set up (M13: efifb(4), simplefb).
#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER: Request<FramebufferResponse> = Request::new(id::FRAMEBUFFER);

#[used]
#[unsafe(link_section = ".requests")]
static EFI_MEMMAP: Request<EfiMemmapResponse> = Request::new(id::EFI_MEMMAP);

#[unsafe(link_section = ".requests")]
#[used]
static MODULE: Request<ModuleResponse> = Request::new(id::MODULE);

/// The application processors (`MULTIPROCESSOR` only: without the request the bootloader
/// leaves them halted, as the uniprocessor kernel expects). xAPIC mode on amd64.
#[cfg(feature = "multiprocessor")]
#[used]
#[unsafe(link_section = ".requests")]
static MP: limine::MpRequest = limine::MpRequest::new(0);

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Limine's entry point, named by the entry point request ([`ENTRY_POINT`]).
///
/// Limine enters with every general purpose register zeroed (base revision 6), so the frame
/// pointer this function saves ends the frame chain that `ddb`'s stack trace walks.
///
/// # Safety
///
/// Called exactly once by the bootloader, with the machine state the Limine protocol specifies.
///
/// Not `#[no_mangle]`: the request hands Limine its address, and the symbol `_start` is
/// arm64's boot(8) entry (`locore0.S`, OpenBSD's name), the ELF entry of that kernel.
unsafe extern "C" fn _start() -> ! {
    // The entry point request has no information in its response; it only has to be present.
    let _ = ENTRY_POINT.request.response();
    match gather() {
        // SAFETY: `_start` runs once, on the boot CPU, in the protocol's entry state, and
        // `boot` describes the image Limine just loaded.
        Ok(boot) => unsafe { start_kernel(boot, limine_banner) },
        // No console yet: the failure exit status is the only trace.
        Err(_) => Machine::exit(ExitStatus::Failure),
    }
}

/// The protocol line of the boot banner.
fn limine_banner(boot: &BootInfo) {
    kprintf!(
        "bsd: limine protocol base revision {} ({} requested), {} regions, {} MiB usable\n",
        BASE_REVISION.loaded_revision().unwrap_or(0),
        limine::BASE_REVISION,
        boot.memmap.len(),
        boot.memmap.usable_bytes() >> 20
    );
}

/// What both entries do with their [`BootInfo`]: `boothowto`, boot(8)'s DUID, the machine's
/// earliest setup (the console), the boot banner (`banner` prints the entry's own line), the
/// boot modules, and `main`.
///
/// # Safety
///
/// Once, on the boot CPU, from an entry, with `boot` describing the image just loaded.
unsafe fn start_kernel(boot: BootInfo, banner: fn(&BootInfo)) -> ! {
    // SAFETY: forwarded from the entry.
    if let Err(_unprintable) = unsafe { boot_init(&boot) } {
        // No console yet: the failure exit status is the only trace.
        Machine::exit(ExitStatus::Failure)
    }
    kprintf!(
        "bsd: booted on {} by {} {}\n",
        Machine::MACHINE,
        Str(boot.bootloader_name.to_bytes()),
        Str(boot.bootloader_version.to_bytes())
    );
    banner(&boot);
    for module in boot.modules() {
        kprintf!(
            "module: {} ({} bytes){}{}\n",
            Str(module.path.to_bytes()),
            module.data.len(),
            if module.string.is_empty() { "" } else { ": " },
            Str(module.string.to_bytes())
        );
    }
    if !boot.cmdline.is_empty() {
        kprintf!("bootargs: {}\n", Str(boot.cmdline.to_bytes()));
    }
    if let Some(fb) = boot.framebuffer {
        kprintf!(
            "bsd: framebuffer {}x{}, {} bpp, {} bytes per line at {:#x}\n",
            fb.width,
            fb.height,
            fb.bpp,
            fb.pitch,
            fb.paddr.as_usize()
        );
    }
    if let Some(mp) = boot.mp {
        kprintf!(
            "bsd: {} processors, boot processor hwid {:#x}\n",
            mp.ncpus,
            mp.bsp_hwid
        );
    }
    init_main::set_init_module(boot.module(b"init").copied());
    match boot.module(b"ramdisk.ffs") {
        // SAFETY: the module is never reclaimed, is mapped read-write for the
        // kernel's lifetime, and nothing but rd(4) uses it from here on.
        // A ramdisk makes this kernel `bsd.rd`: `config bsd root on rd0a swap on rd0b`
        // (sys/conf/swapgeneric.rs); `swapconf_rdroot` runs before main reads it.
        Some(rd) => unsafe {
            rd_root_image_set(rd.base, rd.data.len());
            bsd::conf::swapgeneric::swapconf_rdroot();
        },
        None => {
            kprintf!("rd: no ramdisk module\n");
        }
    }
    init_main::main()
}

/// Sets `boothowto` and the boot DUID and brings up the machine's console.
///
/// # Safety
///
/// As for [`start_kernel`].
unsafe fn boot_init(boot: &BootInfo) -> Result<(), BootError> {
    BOOTHOWTO.store(boot.boothowto(), core::sync::atomic::Ordering::Relaxed);
    // boot(8)'s BOOTARG_BOOTDUID (efiboot's `openbsd,bootduid`; under Limine, `bootduid=` on
    // the command line): `setroot` finds the boot disk by this label DUID.
    if let Some(duid) = boot.bootduid() {
        // SAFETY: the boot CPU alone, before `main` and autoconfiguration read it.
        unsafe { bsd::kern::subr_disk::BOOTDUID.write(duid) };
    }
    #[cfg(feature = "qemu")]
    bsd::kern::selftest::parse_bootargs(boot.cmdline.to_bytes());
    // SAFETY: forwarded from the entry; `boot` describes the image just loaded.
    unsafe { Machine::early_init(boot) }.map_err(|_unprintable| BootError::EarlyInit)
}

/// Turns the bootloader's responses into a [`BootInfo`].
fn gather() -> Result<BootInfo, BootError> {
    if !BASE_REVISION.supported() {
        return Err(BootError::UnsupportedRevision);
    }
    let hhdm = HHDM.response().ok_or(BootError::MissingHhdm)?;
    let addr = EXECUTABLE_ADDRESS
        .response()
        .ok_or(BootError::MissingExecutableAddress)?;
    let map = MEMMAP.response().ok_or(BootError::MissingMemmap)?;

    let mut memmap = MemMap::new();
    for e in map.entries() {
        let region = MemRegion {
            base: Paddr::new(e.base as usize),
            length: Psize::new(e.length as usize),
            kind: mem_kind(e.kind),
        };
        if !memmap.push(region) {
            return Err(BootError::TooManyRegions);
        }
    }

    let (bootloader_name, bootloader_version) = match BOOTLOADER_INFO.response() {
        Some(info) => (info.name(), info.version()),
        None => (c"unknown", c"unknown"),
    };
    let cmdline = EXECUTABLE_CMDLINE
        .response()
        .map_or(c"", ExecutableCmdlineResponse::cmdline);
    // The stack size request has no information in its response; it only has to be present.
    let _ = STACK_SIZE.request.response();

    let mut modules: [Option<BootModule>; MAX_MODULES] = [None; MAX_MODULES];
    if let Some(resp) = MODULE.response() {
        for (slot, file) in modules.iter_mut().zip(resp.modules()) {
            *slot = Some(BootModule {
                path: file.path(),
                string: file.string(),
                data: file.data(),
                base: file.address(),
            });
        }
    }

    let framebuffer = boot_framebuffer(hhdm.offset as usize);
    let dtb = DTB
        .response()
        .and_then(|d| NonNull::new(d.dtb_ptr.cast_mut().cast::<u8>()));
    #[cfg(target_arch = "aarch64")]
    let dtb = match (dtb, framebuffer) {
        (Some(dtb), Some(fb)) => Some(chosen_framebuffer(dtb, &fb).unwrap_or(dtb)),
        _ => dtb,
    };

    Ok(BootInfo {
        bootloader_name,
        bootloader_version,
        cmdline,
        hhdm_offset: hhdm.offset as usize,
        kernel_phys: Paddr::new(addr.physical_base as usize),
        kernel_virt: Vaddr::new(addr.virtual_base as usize),
        rsdp: RSDP.response().map(|r| Vaddr::new(r.address as usize)),
        dtb,
        memmap,
        efi_system_table: EFI_SYSTEM_TABLE.response().and_then(|r| {
            // A higher-half address for base revision 6 (a physical one for 3 and 4).
            let addr = r.address as usize;
            let offset = hhdm.offset as usize;
            let pa = if addr >= offset { addr - offset } else { addr };
            (pa != 0).then(|| Paddr::new(pa))
        }),
        smbios: SMBIOS.response().and_then(|r| {
            // The 32-bit entry point, the one efiboot passes (SMBIOS_TABLE_GUID); physical
            // since base revision 3, made so if it ever comes as a higher-half address.
            let addr = r.entry_32 as usize;
            let offset = hhdm.offset as usize;
            let pa = if addr >= offset { addr - offset } else { addr };
            (pa != 0).then(|| Paddr::new(pa))
        }),
        efi_memmap: EFI_MEMMAP.response().map(|r| EfiMemmap {
            map: r.memmap(),
            desc_size: r.desc_size as u32,
            desc_ver: r.desc_version as u32,
        }),
        framebuffer,
        modules,
        mp: boot_mp(),
        howto: 0,
        duid: None,
    })
}

/// The first RGB frame buffer of the framebuffer response, its address made physical.
fn boot_framebuffer(hhdm_offset: usize) -> Option<BootFramebuffer> {
    let fb = FRAMEBUFFER
        .response()?
        .framebuffers()
        .find(|f| f.memory_model == limine::FRAMEBUFFER_RGB)?;
    // A higher-half address for base revision 6.
    let va = fb.address as usize;
    let pa = if va >= hhdm_offset {
        va - hhdm_offset
    } else {
        va
    };
    Some(BootFramebuffer {
        paddr: Paddr::new(pa),
        width: fb.width as u32,
        height: fb.height as u32,
        pitch: fb.pitch as u32,
        bpp: fb.bpp,
        red_size: fb.red_mask_size,
        red_shift: fb.red_mask_shift,
        green_size: fb.green_mask_size,
        green_shift: fb.green_mask_shift,
        blue_size: fb.blue_mask_size,
        blue_shift: fb.blue_mask_shift,
    })
}

/// The copy of the device tree with efiboot's `/chosen/framebuffer` node (`fdtfb.rs`).
#[cfg(target_arch = "aarch64")]
static FDT_COPY: libkern::StaticCell<[u8; FDT_COPY_SIZE]> =
    libkern::StaticCell::new([0; FDT_COPY_SIZE]);

/// Room for the device tree copy: QEMU's `virt` tree is about 8 KiB of structure and strings.
#[cfg(target_arch = "aarch64")]
const FDT_COPY_SIZE: usize = 256 * 1024;

/// arm64: what efiboot's `efi_framebuffer()` does, a `simple-framebuffer` node for the GOP
/// frame buffer in `/chosen`, in a copy of the tree (`fdtfb.rs`); `None` keeps the
/// bootloader's tree.
#[cfg(target_arch = "aarch64")]
fn chosen_framebuffer(dtb: NonNull<u8>, fb: &BootFramebuffer) -> Option<NonNull<u8>> {
    // SAFETY: the bootloader's tree starts with its 40-byte header (`fdt_check_head` reads
    // the same bytes later); `totalsize` is the second big-endian word.
    let total = unsafe {
        let h = core::slice::from_raw_parts(dtb.as_ptr(), 8);
        u32::from_be_bytes([h[4], h[5], h[6], h[7]]) as usize
    };
    // SAFETY: the tree is `totalsize` bytes of bootloader memory that stays untouched.
    let src = unsafe { core::slice::from_raw_parts(dtb.as_ptr(), total) };
    // SAFETY: `_start` runs once, on the boot CPU, before anything reads the copy.
    let out = unsafe { FDT_COPY.get_mut() };
    fdtfb::add_framebuffer(src, fb, out)?;
    NonNull::new(out.as_mut_ptr())
}

/// The processors from the MP response, `None` without one (or without `MULTIPROCESSOR`).
fn boot_mp() -> Option<BootMp> {
    #[cfg(feature = "multiprocessor")]
    if let Some(r) = MP.request.response() {
        return Some(BootMp {
            bsp_hwid: r.bsp_hwid(),
            ncpus: r.cpu_count(),
            cpu: mp_cpu,
            start: mp_start,
        });
    }
    None
}

/// [`BootMp::cpu`]: processor `i` of the MP response.
#[cfg(feature = "multiprocessor")]
fn mp_cpu(i: usize) -> BootCpu {
    match MP.request.response().and_then(|r| r.cpu(i)) {
        Some(info) => BootCpu {
            processor_id: info.processor_id,
            hwid: info.hwid(),
        },
        None => BootCpu {
            processor_id: u32::MAX,
            hwid: u64::MAX,
        },
    }
}

/// [`BootMp::start`]: hands processor `i` the argument, then the address of [`ap_start`].
///
/// # Safety
///
/// As [`BootMp::start`] states.
#[cfg(feature = "multiprocessor")]
unsafe fn mp_start(i: usize, arg: usize) {
    use core::sync::atomic::Ordering;
    let Some(info) = MP.request.response().and_then(|r| r.cpu(i)) else {
        return;
    };
    info.extra_argument.store(arg as u64, Ordering::Relaxed);
    // The protocol: an atomic write of the address releases the parked processor; Release
    // orders the argument (and everything the boot processor prepared) before it.
    let entry: unsafe extern "C" fn(*const limine::MpInfo) -> ! = ap_start;
    info.goto_address
        .store(entry as usize as u64, Ordering::Release);
}

/// Where an application processor enters the kernel, on the 64 KiB stack the bootloader gave
/// it (the stack size request covers the application processors too), with the bootloader's
/// page tables, interrupts masked, and `info` its MP structure.
///
/// # Safety
///
/// Only the bootloader jumps here, once per processor [`mp_start`] released.
#[cfg(feature = "multiprocessor")]
unsafe extern "C" fn ap_start(info: *const limine::MpInfo) -> ! {
    use core::sync::atomic::Ordering;
    // SAFETY: the protocol passes the processor's own structure, which stays mapped.
    let arg = unsafe { (*info).extra_argument.load(Ordering::Acquire) } as usize;
    // SAFETY: `arg` is what the machine passed to `mp_start` for this processor.
    unsafe { Machine::cpu_hatch(arg) }
}

fn mem_kind(raw: u64) -> MemKind {
    match raw {
        memmap_type::USABLE => MemKind::Usable,
        memmap_type::RESERVED => MemKind::Reserved,
        memmap_type::ACPI_RECLAIMABLE => MemKind::AcpiReclaimable,
        memmap_type::ACPI_NVS => MemKind::AcpiNvs,
        memmap_type::BAD_MEMORY => MemKind::BadMemory,
        memmap_type::BOOTLOADER_RECLAIMABLE => MemKind::BootloaderReclaimable,
        memmap_type::EXECUTABLE_AND_MODULES => MemKind::KernelAndModules,
        memmap_type::FRAMEBUFFER => MemKind::Framebuffer,
        memmap_type::RESERVED_MAPPED => MemKind::ReservedMapped,
        other => MemKind::Unknown(other),
    }
}
/* </CODE> */
