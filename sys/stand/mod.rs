//! Boot glue for the Limine protocol (replaces OpenBSD's `boot(8)` / `efiboot`).
//!
//! `_start` is the ELF entry point the bootloader jumps to. It checks the protocol revision,
//! turns the bootloader's responses into the bootloader-neutral [`BootInfo`], lets the machine
//! do its earliest setup (the polled console) and hands over. Until `kern/init_main.c` is ported
//! (milestone M2) the hand-over target is [`boot_main`] below: it prints the boot banner and the
//! memory map, then leaves the machine. Nothing outside this module names a Limine type.

mod limine;

use core::fmt::{self, Write};
use core::ptr::NonNull;

use bsd::machine::{
    BootInfo, Console, Cpu, Exit, ExitStatus, Machine, MachineInfo, MemKind, MemMap, MemRegion,
};
use bsd::sys::types::{Paddr, Psize, Vaddr};

use limine::{
    BOOTLOADER_INFO_ID, BaseRevision, BootloaderInfoResponse, DTB_ID, DtbResponse,
    EXECUTABLE_ADDRESS_ID, ExecutableAddressResponse, HHDM_ID, HhdmResponse, MEMMAP_ID,
    MemmapResponse, RSDP_ID, Request, RequestsEndMarker, RequestsStartMarker, RsdpResponse,
    StackSizeRequest, memmap_type,
};

/// Boot stack for the boot CPU: the protocol's minimum, more than OpenBSD's `USPACE`.
const STACK_SIZE_BYTES: u64 = 64 * 1024;

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
static BOOTLOADER_INFO: Request<BootloaderInfoResponse> = Request::new(BOOTLOADER_INFO_ID);

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE: StackSizeRequest = StackSizeRequest::new(STACK_SIZE_BYTES);

#[used]
#[unsafe(link_section = ".requests")]
static HHDM: Request<HhdmResponse> = Request::new(HHDM_ID);

#[used]
#[unsafe(link_section = ".requests")]
static MEMMAP: Request<MemmapResponse> = Request::new(MEMMAP_ID);

#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE_ADDRESS: Request<ExecutableAddressResponse> = Request::new(EXECUTABLE_ADDRESS_ID);

#[used]
#[unsafe(link_section = ".requests")]
static RSDP: Request<RsdpResponse> = Request::new(RSDP_ID);

#[used]
#[unsafe(link_section = ".requests")]
static DTB: Request<DtbResponse> = Request::new(DTB_ID);

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Why the boot glue gave up before a console existed. Each ends in a failure exit; under QEMU
/// the exit status (`ExitStatus::Failure`) is the only trace, so the message `early_init`
/// returns is dropped here until a console-less channel (semihosting, M2's `ddb`) can carry it.
#[derive(Debug)]
enum BootError {
    UnsupportedRevision,
    MissingHhdm,
    MissingMemmap,
    MissingExecutableAddress,
    TooManyRegions,
    EarlyInit,
}

/// Bootloader entry point, named by `ENTRY(_start)` in `arch/*/conf/kernel.ld`.
///
/// # Safety
///
/// Called exactly once by the bootloader, with the machine state the Limine protocol specifies.
#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    // SAFETY: `_start` runs once, on the boot CPU, in the protocol's entry state.
    match unsafe { boot() } {
        Ok(boot) => boot_main(&boot),
        // No console yet: the failure exit status is the only trace.
        Err(_) => Machine::exit(ExitStatus::Failure),
    }
}

/// Gathers the boot facts and brings up the machine's early console.
///
/// # Safety
///
/// As for `_start`.
unsafe fn boot() -> Result<BootInfo, BootError> {
    let boot = gather()?;
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
    // The stack size request has no information in its response; it only has to be present.
    let _ = STACK_SIZE.request.response();

    Ok(BootInfo {
        bootloader_name,
        bootloader_version,
        hhdm_offset: hhdm.offset as usize,
        kernel_phys: Paddr::new(addr.physical_base as usize),
        kernel_virt: Vaddr::new(addr.virtual_base as usize),
        rsdp: RSDP.response().map(|r| Vaddr::new(r.address as usize)),
        dtb: DTB
            .response()
            .and_then(|d| NonNull::new(d.dtb_ptr.cast_mut().cast::<u8>())),
        memmap,
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

/// The polled console as a `fmt::Write` sink, with `\n` sent as `\r\n`. `kprintf!` (milestone
/// M2) replaces it.
struct EarlyConsole;

impl Write for EarlyConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                Machine::putc(b'\r');
            }
            Machine::putc(b);
        }
        Ok(())
    }
}

fn text(s: &core::ffi::CStr) -> &str {
    core::str::from_utf8(s.to_bytes()).unwrap_or("?")
}

/// Milestone M0's stand-in for `kern::init_main::main`: say where we are, show what the
/// bootloader left us, leave.
fn boot_main(boot: &BootInfo) -> ! {
    let mut con = EarlyConsole;
    // Console writes cannot fail; the Results are ignored on purpose.
    let _ = writeln!(
        con,
        "bsd: booted on {} by {} {}",
        Machine::MACHINE,
        text(boot.bootloader_name),
        text(boot.bootloader_version)
    );
    let _ = writeln!(
        con,
        "bsd: limine protocol base revision {} ({} requested)",
        BASE_REVISION.loaded_revision().unwrap_or(0),
        limine::BASE_REVISION
    );
    let _ = writeln!(
        con,
        "bsd: kernel at phys {:#x} virt {:#x}, hhdm offset {:#x}",
        boot.kernel_phys, boot.kernel_virt, boot.hhdm_offset
    );
    if let Some(rsdp) = boot.rsdp {
        let _ = writeln!(con, "bsd: acpi rsdp at {:#x}", rsdp);
    }
    if let Some(dtb) = boot.dtb {
        let _ = writeln!(con, "bsd: device tree at {:p}", dtb.as_ptr());
    }
    for r in boot.memmap.regions() {
        let end = r.base.as_usize() + r.length.as_usize();
        let _ = writeln!(
            con,
            "bsd: mem {:#018x}-{:#018x} {:?}",
            r.base.as_usize(),
            end,
            r.kind
        );
    }
    let _ = writeln!(
        con,
        "bsd: {} regions, {} MiB usable",
        boot.memmap.len(),
        boot.memmap.usable_bytes() >> 20
    );
    let _ = writeln!(con, "bsd: nothing more to do until milestone M2; leaving");
    Machine::exit(ExitStatus::Success)
}
