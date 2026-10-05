//! Boot glue for the Limine protocol (replaces OpenBSD's `boot(8)` / `efiboot`).
//!
//! `_start` is the ELF entry point the bootloader jumps to. It checks the protocol revision,
//! turns the bootloader's responses into the bootloader-neutral [`BootInfo`], sets `boothowto`
//! from the command line, lets the machine do its earliest setup (OpenBSD's `init_x86_64` /
//! `initarm`: the message buffer and the console) and hands over to `kern::init_main::main`.
//! Nothing outside this module names a Limine type.

mod limine;

use core::ptr::NonNull;

use bsd::dev::rd::rd_root_image_set;
use bsd::kern::init_main::{self, BOOTHOWTO};
use bsd::kern::subr_prf::Str;
use bsd::kprintf;
#[cfg(feature = "multiprocessor")]
use bsd::machine::BootCpu;
use bsd::machine::{
    BootInfo, BootModule, BootMp, Cpu, EfiMemmap, Exit, ExitStatus, MAX_MODULES, Machine,
    MachineInfo, MemKind, MemMap, MemRegion,
};
use bsd::sys::types::{Paddr, Psize, Vaddr};

use limine::{
    BaseRevision, BootloaderInfoResponse, DtbResponse, EfiMemmapResponse, EfiSystemTableResponse,
    ExecutableAddressResponse, ExecutableCmdlineResponse, HhdmResponse, MemmapResponse,
    ModuleResponse, Request, RequestsEndMarker, RequestsStartMarker, RsdpResponse,
    StackSizeRequest, id, memmap_type,
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

/// Bootloader entry point, named by `ENTRY(_start)` in `arch/*/conf/kernel.ld`.
///
/// Limine enters with every general purpose register zeroed (base revision 6), so the frame
/// pointer this function saves ends the frame chain that `ddb`'s stack trace walks.
///
/// # Safety
///
/// Called exactly once by the bootloader, with the machine state the Limine protocol specifies.
#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    // SAFETY: `_start` runs once, on the boot CPU, in the protocol's entry state.
    match unsafe { boot() } {
        Ok(boot) => {
            kprintf!(
                "bsd: booted on {} by {} {}\n",
                Machine::MACHINE,
                Str(boot.bootloader_name.to_bytes()),
                Str(boot.bootloader_version.to_bytes())
            );
            kprintf!(
                "bsd: limine protocol base revision {} ({} requested), {} regions, {} MiB usable\n",
                BASE_REVISION.loaded_revision().unwrap_or(0),
                limine::BASE_REVISION,
                boot.memmap.len(),
                boot.memmap.usable_bytes() >> 20
            );
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
        // No console yet: the failure exit status is the only trace.
        Err(_) => Machine::exit(ExitStatus::Failure),
    }
}

/// Gathers the boot facts, sets `boothowto` and brings up the machine's console.
///
/// # Safety
///
/// As for `_start`.
unsafe fn boot() -> Result<BootInfo, BootError> {
    let boot = gather()?;
    BOOTHOWTO.store(boot.boothowto(), core::sync::atomic::Ordering::Relaxed);
    // boot(8)'s BOOTARG_BOOTDUID (efiboot's `openbsd,bootduid`): `setroot` finds the boot
    // disk by this label DUID, there being no `bootdev` under Limine.
    if let Some(duid) = boot.bootduid() {
        // SAFETY: the boot CPU alone, before `main` and autoconfiguration read it.
        unsafe { bsd::kern::subr_disk::BOOTDUID.write(duid) };
    }
    #[cfg(feature = "qemu")]
    bsd::kern::selftest::parse_bootargs(boot.cmdline.to_bytes());
    // SAFETY: forwarded from `_start`; `boot` describes the image the bootloader just loaded.
    unsafe { Machine::early_init(&boot) }.map_err(|_unprintable| BootError::EarlyInit)?;
    Ok(boot)
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

    Ok(BootInfo {
        bootloader_name,
        bootloader_version,
        cmdline,
        hhdm_offset: hhdm.offset as usize,
        kernel_phys: Paddr::new(addr.physical_base as usize),
        kernel_virt: Vaddr::new(addr.virtual_base as usize),
        rsdp: RSDP.response().map(|r| Vaddr::new(r.address as usize)),
        dtb: DTB
            .response()
            .and_then(|d| NonNull::new(d.dtb_ptr.cast_mut().cast::<u8>())),
        memmap,
        efi_system_table: EFI_SYSTEM_TABLE.response().and_then(|r| {
            // A higher-half address for base revision 6 (a physical one for 3 and 4).
            let addr = r.address as usize;
            let offset = hhdm.offset as usize;
            let pa = if addr >= offset { addr - offset } else { addr };
            (pa != 0).then(|| Paddr::new(pa))
        }),
        efi_memmap: EFI_MEMMAP.response().map(|r| EfiMemmap {
            map: r.memmap(),
            desc_size: r.desc_size as u32,
            desc_ver: r.desc_version as u32,
        }),
        modules,
        mp: boot_mp(),
    })
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
