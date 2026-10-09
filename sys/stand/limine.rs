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
//! The Limine boot protocol, base revision 6: the subset of features this kernel asks for.
//!
//! Written from the protocol specification, `PROTOCOL.md` and `include/limine.h` of
//! <https://github.com/limine-bootloader/limine-protocol> at commit `3a0526b700e3` (2026-09-25).
//! Layouts are `#[repr(C)]`; every field is a `u64` or a pointer, as in the C header. No crate
//! stands in between (`docs/ARCHITECTURE.md`, "Dependencies").
//!
//! How it works: the kernel places request structures, each starting with a magic ID, in the
//! `.requests` section of its image. Before jumping to `_start`, the bootloader scans the image,
//! fills each request's `response` pointer, and rewrites the base revision tag to say which
//! revision it honoured. Responses live in bootloader-reclaimable memory, and every pointer in
//! them is a higher-half (HHDM) address.

use core::cell::UnsafeCell;
use core::ffi::{CStr, c_char, c_void};
use core::ptr;

/// `LIMINE_COMMON_MAGIC`: the two words every request ID starts with.
pub const COMMON_MAGIC: [u64; 2] = [0xc7b1_dd30_df4c_8b88, 0x0a82_e883_a194_f07b];

/// The base revision this kernel is written against.
pub const BASE_REVISION: u64 = 6;

/// Request IDs: the feature-specific half; [`Request::new`] prepends [`COMMON_MAGIC`].
pub mod id {
    /// Bootloader Info feature.
    pub const BOOTLOADER_INFO: [u64; 2] = [0xf550_38d8_e2a1_202f, 0x2794_26fc_f5f5_9740];
    /// Entry Point feature: the kernel names its entry instead of the ELF's `e_entry`.
    pub const ENTRY_POINT: [u64; 2] = [0x13d8_6c03_5a1c_d3e1, 0x2b0c_aa89_d8f3_026a];
    /// Stack Size feature.
    pub const STACK_SIZE: [u64; 2] = [0x224e_f046_0a8e_8926, 0xe1cb_0fc2_5f46_ea3d];
    /// HHDM (Higher Half Direct Map) feature.
    pub const HHDM: [u64; 2] = [0x48dc_f1cb_8ad2_b852, 0x6398_4e95_9a98_244b];
    /// Memory Map feature.
    pub const MEMMAP: [u64; 2] = [0x67cf_3d9d_378a_806f, 0xe304_acdf_c50c_3c62];
    /// RSDP feature.
    pub const RSDP: [u64; 2] = [0xc5e7_7b6b_397e_7b43, 0x2763_7845_accd_cf3c];
    /// Executable Address feature.
    pub const EXECUTABLE_ADDRESS: [u64; 2] = [0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487];
    /// Device Tree Blob feature.
    pub const DTB: [u64; 2] = [0xb40d_db48_fb54_bac7, 0x5450_8149_3f81_ffb7];
    /// Executable Command Line feature.
    pub const EXECUTABLE_CMDLINE: [u64; 2] = [0x4b16_1536_e598_651e, 0xb390_ad4a_2f1f_303a];
    /// `LIMINE_MODULE_REQUEST`: the files `module_path:` lines of `limine.conf` loaded.
    pub const MODULE: [u64; 2] = [0x3e7e_2797_02be_32af, 0xca1c_4f3b_d128_0cee];
    /// EFI System Table feature.
    pub const EFI_SYSTEM_TABLE: [u64; 2] = [0x5ceb_a516_3eaa_f6d6, 0x0a69_8161_0cf6_5fcc];
    /// SMBIOS feature (`LIMINE_SMBIOS_REQUEST_ID`).
    pub const SMBIOS: [u64; 2] = [0x9e90_46f1_1e09_5391, 0xaa4a_520f_efbd_e5ee];
    /// Framebuffer feature (`LIMINE_FRAMEBUFFER_REQUEST_ID`).
    pub const FRAMEBUFFER: [u64; 2] = [0x9d58_27dc_d881_dd75, 0xa314_8604_f6fa_b11b];
    /// EFI Memory Map feature.
    pub const EFI_MEMMAP: [u64; 2] = [0x7df6_2a43_1d68_72d5, 0xa4fc_dfb3_e573_06c8];
    #[cfg_attr(not(feature = "multiprocessor"), allow(dead_code))] // asked by the MP kernel only
    /// MP (multiprocessor) feature.
    pub const MP: [u64; 2] = [0x95a6_7b81_9a1b_857e, 0xa0b6_1b72_3b6a_73e0];
}

/// `LIMINE_MEMMAP_*`: memory map entry types.
pub mod memmap_type {
    /// Usable RAM.
    pub const USABLE: u64 = 0;
    /// Reserved.
    pub const RESERVED: u64 = 1;
    /// ACPI tables, reclaimable.
    pub const ACPI_RECLAIMABLE: u64 = 2;
    /// ACPI non-volatile storage.
    pub const ACPI_NVS: u64 = 3;
    /// Bad memory.
    pub const BAD_MEMORY: u64 = 4;
    /// Bootloader data, reclaimable.
    pub const BOOTLOADER_RECLAIMABLE: u64 = 5;
    /// The executable and its modules.
    pub const EXECUTABLE_AND_MODULES: u64 = 6;
    /// A framebuffer.
    pub const FRAMEBUFFER: u64 = 7;
    /// Reserved but mapped (base revision 4 and later).
    pub const RESERVED_MAPPED: u64 = 8;
}

/// `LIMINE_BASE_REVISION(N)`: tells the bootloader which protocol revision the kernel expects.
#[repr(C)]
pub struct BaseRevision {
    magic0: u64,
    magic1: UnsafeCell<u64>,
    revision: UnsafeCell<u64>,
}

// SAFETY: the bootloader writes the two cells before the kernel's first instruction; afterwards
// they are only read.
unsafe impl Sync for BaseRevision {}

impl BaseRevision {
    const MAGIC0: u64 = 0xf956_2b2d_5c95_a6c8;
    const MAGIC1: u64 = 0x6a7b_3849_4453_6bdc;

    /// A tag requesting `revision`.
    pub const fn new(revision: u64) -> Self {
        Self {
            magic0: Self::MAGIC0,
            magic1: UnsafeCell::new(Self::MAGIC1),
            revision: UnsafeCell::new(revision),
        }
    }

    /// `LIMINE_BASE_REVISION_SUPPORTED`: the bootloader honoured the requested revision.
    pub fn supported(&self) -> bool {
        Self::read(&self.revision) == 0
    }

    /// `LIMINE_LOADED_BASE_REVISION`: the revision the bootloader actually used, which
    /// bootloaders of revision 3 and later report even when they could not honour the request.
    pub fn loaded_revision(&self) -> Option<u64> {
        let v = Self::read(&self.magic1);
        if v == Self::MAGIC1 { None } else { Some(v) }
    }

    fn read(cell: &UnsafeCell<u64>) -> u64 {
        // SAFETY: the bootloader finished writing before `_start`; the read is volatile so the
        // compiler cannot assume the value the static was initialised with.
        unsafe { ptr::read_volatile(cell.get()) }
    }
}

/// `LIMINE_REQUESTS_START_MARKER`: requests before it in the image are ignored.
#[repr(C)]
pub struct RequestsStartMarker([u64; 4]);

impl RequestsStartMarker {
    /// The marker.
    pub const fn new() -> Self {
        Self([
            0xf6b8_f4b3_9de7_d1ae,
            0xfab9_1a69_40fc_b9cf,
            0x785c_6ed0_15d3_e316,
            0x181e_920a_7852_b9d9,
        ])
    }
}

impl Default for RequestsStartMarker {
    fn default() -> Self {
        Self::new()
    }
}

/// `LIMINE_REQUESTS_END_MARKER`: requests after it in the image are ignored.
#[repr(C)]
pub struct RequestsEndMarker([u64; 2]);

impl RequestsEndMarker {
    /// The marker.
    pub const fn new() -> Self {
        Self([0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62])
    }
}

impl Default for RequestsEndMarker {
    fn default() -> Self {
        Self::new()
    }
}

/// The three members every request starts with; `R` is the response the bootloader points at.
/// Requests with extra members wrap this (see [`StackSizeRequest`]).
#[repr(C)]
pub struct Request<R> {
    id: [u64; 4],
    revision: u64,
    response: UnsafeCell<*const R>,
}

// SAFETY: the bootloader writes `response` (or leaves it null) before the kernel's first
// instruction; afterwards it is only read.
unsafe impl<R> Sync for Request<R> {}

impl<R> Request<R> {
    /// A revision-0 request for the feature whose [`id`] is given.
    pub const fn new(id: [u64; 2]) -> Self {
        Self {
            id: [COMMON_MAGIC[0], COMMON_MAGIC[1], id[0], id[1]],
            revision: 0,
            response: UnsafeCell::new(ptr::null()),
        }
    }

    /// The bootloader's response, `None` if it did not provide the feature.
    pub fn response(&self) -> Option<&R> {
        // SAFETY: written (or left null) by the bootloader before `_start`; volatile so the
        // compiler cannot assume the initial null.
        let p = unsafe { ptr::read_volatile(self.response.get()) };
        // SAFETY: a non-null response points to a valid, 8-byte aligned structure in
        // bootloader-reclaimable memory, which stays mapped and untouched until the kernel
        // reclaims it (milestone M3 at the earliest), and is never written again.
        unsafe { p.as_ref() }
    }
}

/// `struct limine_bootloader_info_response`.
#[repr(C)]
pub struct BootloaderInfoResponse {
    /// Response revision.
    pub revision: u64,
    name: *const c_char,
    version: *const c_char,
}

impl BootloaderInfoResponse {
    /// Name of the bootloader.
    pub fn name(&self) -> &CStr {
        // SAFETY: the protocol guarantees a non-null, 0-terminated ASCII string in
        // bootloader-reclaimable memory.
        unsafe { CStr::from_ptr(self.name) }
    }

    /// Version of the bootloader.
    pub fn version(&self) -> &CStr {
        // SAFETY: as for `name`.
        unsafe { CStr::from_ptr(self.version) }
    }
}

/// `struct limine_stack_size_request`: asks for a boot stack of at least `stack_size` bytes.
#[repr(C)]
pub struct StackSizeRequest {
    /// The common request members.
    pub request: Request<StackSizeResponse>,
    /// Requested stack size in bytes (also used for secondary processors).
    pub stack_size: u64,
}

impl StackSizeRequest {
    /// A request for `stack_size` bytes.
    pub const fn new(stack_size: u64) -> Self {
        Self {
            request: Request::new(id::STACK_SIZE),
            stack_size,
        }
    }
}

/// `struct limine_stack_size_response`.
#[repr(C)]
pub struct StackSizeResponse {
    /// Response revision.
    pub revision: u64,
}

/// `struct limine_entry_point_request`: the address the bootloader enters instead of the ELF
/// entry point. amd64's ELF entry is `locore0.S`'s 32-bit `start`, which boot(8) enters.
#[repr(C)]
pub struct EntryPointRequest {
    /// The common request members.
    pub request: Request<EntryPointResponse>,
    /// `entry`: the kernel's Limine entry.
    pub entry: unsafe extern "C" fn() -> !,
}

impl EntryPointRequest {
    /// A request naming `entry`.
    pub const fn new(entry: unsafe extern "C" fn() -> !) -> Self {
        Self {
            request: Request::new(id::ENTRY_POINT),
            entry,
        }
    }
}

/// `struct limine_entry_point_response`.
#[repr(C)]
pub struct EntryPointResponse {
    /// Response revision.
    pub revision: u64,
}

/// `struct limine_hhdm_response`.
#[repr(C)]
pub struct HhdmResponse {
    /// Response revision.
    pub revision: u64,
    /// Virtual address at which physical address 0 is mapped.
    pub offset: u64,
}

/// `struct limine_memmap_entry`.
#[repr(C)]
pub struct MemmapEntry {
    /// Physical start of the region.
    pub base: u64,
    /// Size in bytes.
    pub length: u64,
    /// One of the [`memmap_type`] constants (`type` in C).
    pub kind: u64,
}

/// `struct limine_memmap_response`.
#[repr(C)]
pub struct MemmapResponse {
    /// Response revision.
    pub revision: u64,
    entry_count: u64,
    entries: *const *const MemmapEntry,
}

impl MemmapResponse {
    /// The entries, sorted by base address.
    pub fn entries(&self) -> impl Iterator<Item = &MemmapEntry> + '_ {
        (0..self.entry_count as usize).map(move |i| {
            // SAFETY: `entries` points to `entry_count` non-null pointers, each to a valid
            // entry, all in bootloader-reclaimable memory that stays untouched (see
            // `Request::response`).
            unsafe { &**self.entries.add(i) }
        })
    }
}

/// `struct limine_rsdp_response`.
#[repr(C)]
pub struct RsdpResponse {
    /// Response revision.
    pub revision: u64,
    /// Address of the RSDP: virtual (HHDM) for base revision 6.
    pub address: *const c_void,
}

/// `struct limine_executable_address_response`.
#[repr(C)]
pub struct ExecutableAddressResponse {
    /// Response revision.
    pub revision: u64,
    /// Physical base address of the loaded image.
    pub physical_base: u64,
    /// Virtual base address of the loaded image.
    pub virtual_base: u64,
}

/// `struct limine_dtb_response`.
#[repr(C)]
pub struct DtbResponse {
    /// Response revision.
    pub revision: u64,
    /// Virtual (HHDM) pointer to the device tree blob.
    pub dtb_ptr: *const c_void,
}

/// `LIMINE_FRAMEBUFFER_RGB`: the one memory model, pixels as masks of a word.
pub const FRAMEBUFFER_RGB: u8 = 1;

/// `struct limine_framebuffer` (response revision 0; the video mode list of revision 1
/// follows and is not read).
#[repr(C)]
pub struct Framebuffer {
    /// Virtual (HHDM) address of the frame buffer.
    pub address: *mut c_void,
    /// Width in pixels.
    pub width: u64,
    /// Height in pixels.
    pub height: u64,
    /// Bytes per scan line.
    pub pitch: u64,
    /// Bits per pixel.
    pub bpp: u16,
    /// [`FRAMEBUFFER_RGB`].
    pub memory_model: u8,
    /// Bits of red.
    pub red_mask_size: u8,
    /// The lowest bit of red.
    pub red_mask_shift: u8,
    /// Bits of green.
    pub green_mask_size: u8,
    /// The lowest bit of green.
    pub green_mask_shift: u8,
    /// Bits of blue.
    pub blue_mask_size: u8,
    /// The lowest bit of blue.
    pub blue_mask_shift: u8,
    unused: [u8; 7],
    /// Size of the EDID blob.
    pub edid_size: u64,
    /// The monitor's EDID, if any.
    pub edid: *mut c_void,
}

/// `struct limine_framebuffer_response`.
#[repr(C)]
pub struct FramebufferResponse {
    /// Response revision.
    pub revision: u64,
    framebuffer_count: u64,
    framebuffers: *const *const Framebuffer,
}

impl FramebufferResponse {
    /// The frame buffers the bootloader set up (the firmware's GOP modes), first the one it
    /// drew on.
    pub fn framebuffers(&self) -> impl Iterator<Item = &Framebuffer> + '_ {
        (0..self.framebuffer_count as usize).map(move |i| {
            // SAFETY: `framebuffers` points to `framebuffer_count` non-null pointers, each to
            // a valid structure in bootloader-reclaimable memory that stays untouched (see
            // `Request::response`).
            unsafe { &**self.framebuffers.add(i) }
        })
    }
}

/// `struct limine_executable_cmdline_response`.
#[repr(C)]
pub struct ExecutableCmdlineResponse {
    /// Response revision.
    pub revision: u64,
    cmdline: *const c_char,
}

impl ExecutableCmdlineResponse {
    /// The command line given to the executable (`cmdline:` in `limine.conf`), possibly empty.
    pub fn cmdline(&self) -> &CStr {
        // SAFETY: the protocol guarantees a non-null, 0-terminated string in
        // bootloader-reclaimable memory.
        unsafe { CStr::from_ptr(self.cmdline) }
    }
}

/// `struct limine_efi_system_table_response`.
#[repr(C)]
pub struct EfiSystemTableResponse {
    /// Response revision.
    pub revision: u64,
    /// Address of the EFI system table: virtual (HHDM) for base revision 6.
    pub address: *const c_void,
}

/// `struct limine_smbios_response`.
#[repr(C)]
pub struct SmbiosResponse {
    /// Response revision.
    pub revision: u64,
    /// Physical address of the 32-bit (SMBIOS 2, `_SM_`) entry point, null if there is none.
    pub entry_32: *const c_void,
    /// Physical address of the 64-bit (SMBIOS 3, `_SM3_`) entry point, null if there is none.
    pub entry_64: *const c_void,
}

/// `struct limine_efi_memmap_response`.
#[repr(C)]
pub struct EfiMemmapResponse {
    /// Response revision.
    pub revision: u64,
    /// Virtual (HHDM) pointer to the firmware's memory map, as `GetMemoryMap` returned it.
    pub memmap: *const c_void,
    /// Size of the memory map in bytes.
    pub memmap_size: u64,
    /// Size of one descriptor in bytes (at least `sizeof(EFI_MEMORY_DESCRIPTOR)`).
    pub desc_size: u64,
    /// Version of the descriptors.
    pub desc_version: u64,
}

impl EfiMemmapResponse {
    /// The memory map's bytes.
    pub fn memmap(&self) -> &'static [u8] {
        // SAFETY: the protocol guarantees `memmap_size` bytes at `memmap`, in
        // bootloader-reclaimable memory the kernel reads before reclaiming any.
        unsafe { core::slice::from_raw_parts(self.memmap.cast::<u8>(), self.memmap_size as usize) }
    }
}

/// `struct limine_mp_request`: asks the bootloader to start the application processors and
/// park them until the kernel hands each a `goto_address`.
#[repr(C)]
pub struct MpRequest {
    /// The common request members.
    pub request: Request<MpResponse>,
    /// `LIMINE_MP_REQUEST_X86_64_X2APIC` (bit 0) asks for x2APIC mode on x86-64; 0 here.
    pub flags: u64,
}

#[cfg_attr(not(feature = "multiprocessor"), allow(dead_code))] // made by the MP kernel only
impl MpRequest {
    /// A request with `flags`.
    pub const fn new(flags: u64) -> Self {
        Self {
            request: Request::new(id::MP),
            flags,
        }
    }
}

/// `struct limine_mp_info` (x86-64 layout): one processor.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct MpInfo {
    /// ACPI processor UID, as in the MADT.
    pub processor_id: u32,
    /// Local APIC ID, as in the MADT.
    pub lapic_id: u32,
    reserved: u64,
    /// Where the parked processor jumps once this is written (atomically), with `%rdi`
    /// pointing at this structure; it polls the word until then.
    pub goto_address: core::sync::atomic::AtomicU64,
    /// Free for the kernel; written before `goto_address`.
    pub extra_argument: core::sync::atomic::AtomicU64,
}

/// `struct limine_mp_info` (AArch64 layout): one processor.
#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
pub struct MpInfo {
    /// ACPI processor UID, as in the MADT (the boot glue's index without ACPI).
    pub processor_id: u32,
    reserved1: u32,
    /// The processor's `MPIDR_EL1`.
    pub mpidr: u64,
    reserved: u64,
    /// Where the parked processor jumps once this is written (atomically), with `x0`
    /// pointing at this structure; it polls the word until then.
    pub goto_address: core::sync::atomic::AtomicU64,
    /// Free for the kernel; written before `goto_address`.
    pub extra_argument: core::sync::atomic::AtomicU64,
}

#[cfg_attr(not(feature = "multiprocessor"), allow(dead_code))] // read by the MP kernel only
impl MpInfo {
    /// The hardware ID: the local APIC ID on x86-64, `MPIDR_EL1` on AArch64.
    pub fn hwid(&self) -> u64 {
        #[cfg(target_arch = "x86_64")]
        return u64::from(self.lapic_id);
        #[cfg(not(target_arch = "x86_64"))]
        return self.mpidr;
    }
}

/// `struct limine_mp_response` (x86-64 layout).
#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct MpResponse {
    /// Response revision.
    pub revision: u64,
    /// `LIMINE_MP_RESPONSE_X86_64_X2APIC` (bit 0) when x2APIC was enabled.
    pub flags: u32,
    /// The boot processor's local APIC ID.
    pub bsp_lapic_id: u32,
    cpu_count: u64,
    cpus: *const *const MpInfo,
}

/// `struct limine_mp_response` (AArch64 layout).
#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
pub struct MpResponse {
    /// Response revision.
    pub revision: u64,
    /// Always 0.
    pub flags: u64,
    /// The boot processor's `MPIDR_EL1`.
    pub bsp_mpidr: u64,
    cpu_count: u64,
    cpus: *const *const MpInfo,
}

#[cfg_attr(not(feature = "multiprocessor"), allow(dead_code))] // read by the MP kernel only
impl MpResponse {
    /// The boot processor's hardware ID (see [`MpInfo::hwid`]).
    pub fn bsp_hwid(&self) -> u64 {
        #[cfg(target_arch = "x86_64")]
        return u64::from(self.bsp_lapic_id);
        #[cfg(not(target_arch = "x86_64"))]
        return self.bsp_mpidr;
    }

    /// How many processors there are, the boot processor included.
    pub fn cpu_count(&self) -> usize {
        self.cpu_count as usize
    }

    /// Processor `i`; `None` past the last.
    pub fn cpu(&self, i: usize) -> Option<&'static MpInfo> {
        if i >= self.cpu_count() {
            return None;
        }
        // SAFETY: `cpus` points to `cpu_count` non-null pointers, each to a processor's
        // structure, in bootloader-reclaimable memory the kernel never reclaims (only usable
        // memory goes to uvm); the parked processors poll their `goto_address` there.
        unsafe { (*self.cpus.add(i)).as_ref() }
    }
}

// Layouts match the C header: these are the sizes `sizeof` reports there.
const _: () = {
    use core::mem::size_of;
    assert!(size_of::<MpRequest>() == 56);
    #[cfg(target_arch = "x86_64")]
    assert!(size_of::<MpInfo>() == 32);
    #[cfg(not(target_arch = "x86_64"))]
    assert!(size_of::<MpInfo>() == 40);
    #[cfg(target_arch = "x86_64")]
    assert!(size_of::<MpResponse>() == 32);
    #[cfg(not(target_arch = "x86_64"))]
    assert!(size_of::<MpResponse>() == 40);
    assert!(size_of::<BaseRevision>() == 24);
    assert!(size_of::<RequestsStartMarker>() == 32);
    assert!(size_of::<RequestsEndMarker>() == 16);
    assert!(size_of::<Request<HhdmResponse>>() == 48);
    assert!(size_of::<StackSizeRequest>() == 56);
    assert!(size_of::<BootloaderInfoResponse>() == 24);
    assert!(size_of::<HhdmResponse>() == 16);
    assert!(size_of::<MemmapEntry>() == 24);
    assert!(size_of::<MemmapResponse>() == 24);
    assert!(size_of::<RsdpResponse>() == 16);
    assert!(size_of::<ExecutableAddressResponse>() == 24);
    assert!(size_of::<DtbResponse>() == 16);
    assert!(size_of::<ExecutableCmdlineResponse>() == 16);
    assert!(size_of::<EfiSystemTableResponse>() == 16);
    assert!(size_of::<EfiMemmapResponse>() == 40);
};

/// `struct limine_uuid`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Uuid {
    /// `a`.
    pub a: u32,
    /// `b`.
    pub b: u16,
    /// `c`.
    pub c: u16,
    /// `d`.
    pub d: [u8; 8],
}

/// `struct limine_file`: a loaded file (the executable or a module).
#[repr(C)]
pub struct File {
    /// Revision of the structure.
    pub revision: u64,
    address: *const u8,
    size: u64,
    path: *const c_char,
    string: *const c_char,
    /// `media_type`: `LIMINE_MEDIA_TYPE_*`.
    pub media_type: u64,
    unused: u32,
    /// `tftp_ip`.
    pub tftp_ip: u32,
    /// `tftp_port`.
    pub tftp_port: u32,
    /// `partition_index`.
    pub partition_index: u32,
    /// `mbr_disk_id`.
    pub mbr_disk_id: u32,
    /// `gpt_disk_uuid`.
    pub gpt_disk_uuid: Uuid,
    /// `gpt_part_uuid`.
    pub gpt_part_uuid: Uuid,
    /// `part_uuid`.
    pub part_uuid: Uuid,
}

impl File {
    /// The file's contents, in the higher half direct map; the memory is "executable and
    /// modules" in the memory map, never reclaimed.
    pub fn data(&self) -> &'static [u8] {
        // SAFETY: the protocol guarantees `address` points at `size` readable bytes that stay
        // mapped and untouched for the kernel's lifetime.
        unsafe { core::slice::from_raw_parts(self.address, self.size as usize) }
    }

    /// The file's first byte, writable: the same memory as [`File::data`], which the
    /// bootloader maps read-write in the direct map.
    pub fn address(&self) -> *mut u8 {
        self.address.cast_mut()
    }

    /// The path the file was loaded from (`/init`).
    pub fn path(&self) -> &'static CStr {
        // SAFETY: the protocol guarantees a non-null, 0-terminated string that stays mapped.
        unsafe { CStr::from_ptr(self.path) }
    }

    /// The string given with the module (`module_string:`), possibly empty.
    pub fn string(&self) -> &'static CStr {
        // SAFETY: as for `path`.
        unsafe { CStr::from_ptr(self.string) }
    }
}

/// `struct limine_module_response`.
#[repr(C)]
pub struct ModuleResponse {
    /// Response revision.
    pub revision: u64,
    module_count: u64,
    modules: *const *const File,
}

impl ModuleResponse {
    /// The loaded modules, in `limine.conf` order.
    pub fn modules(&self) -> impl Iterator<Item = &File> + '_ {
        (0..self.module_count as usize).map(move |i| {
            // SAFETY: `modules` points to `module_count` non-null pointers, each to a valid
            // file structure, all in bootloader-reclaimable memory that stays untouched (see
            // `Request::response`).
            unsafe { &**self.modules.add(i) }
        })
    }
}
/* </CODE> */
