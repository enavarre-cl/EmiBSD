//! Boot glue for the Limine protocol (replaces OpenBSD's `boot(8)` / `efiboot`).
//!
//! `_start` is the ELF entry point the bootloader jumps to. It checks the protocol revision,
//! turns the bootloader's responses into the bootloader-neutral [`BootInfo`], sets `boothowto`
//! from the command line, lets the machine do its earliest setup (OpenBSD's `init_x86_64` /
//! `initarm`: the message buffer and the console) and hands over to `kern::init_main::main`.
//! Nothing outside this module names a Limine type.

mod limine;

use core::ptr::NonNull;

use bsd::kern::init_main::{self, BOOTHOWTO};
use bsd::kern::subr_prf::Str;
use bsd::kprintf;
use bsd::machine::{
    BootInfo, BootModule, Cpu, Exit, ExitStatus, MAX_MODULES, Machine, MachineInfo, MemKind,
    MemMap, MemRegion,
};
use bsd::sys::types::{Paddr, Psize, Vaddr};

use limine::{
    BaseRevision, BootloaderInfoResponse, DtbResponse, ExecutableAddressResponse,
    ExecutableCmdlineResponse, HhdmResponse, MemmapResponse, ModuleResponse, Request,
    RequestsEndMarker, RequestsStartMarker, RsdpResponse, StackSizeRequest, id, memmap_type,
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

/// The modules (`init`, M6).
#[unsafe(link_section = ".requests")]
#[used]
static MODULE: Request<ModuleResponse> = Request::new(id::MODULE);

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
            init_main::set_init_module(boot.module(b"init").copied());
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
        modules,
    })
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
