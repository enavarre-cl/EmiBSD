//! `<machine/bus.h>` as a trait: the `bus_space(9)` methods a machine-independent driver uses to
//! reach its registers.
//!
//! Each architecture defines what a tag and a handle are (amd64 distinguishes port and memory
//! space, arm64 maps everything), the map flags it understands, and the accessors. The free
//! functions below carry the C names, so a driver reads like its C original; they dispatch to the
//! selected [`Machine`]. `bus_dma(9)` arrives with milestone M6.
//!
//! The `bus_space_map` method is `unsafe`: the caller asserts that the range is a device this
//! driver owns, which is what the firmware tables (ACPI, FDT) or a configured address guarantee
//! in OpenBSD. Reads and writes through a handle returned by it are safe.

use crate::machine::Machine;
use crate::sys::errno::Errno;

/// `bus_addr_t`: an address on the bus.
pub type BusAddr = usize;
/// `bus_size_t`: a size or offset on the bus.
pub type BusSize = usize;

/// `BUS_SPACE_BARRIER_READ`: force a read barrier.
pub const BUS_SPACE_BARRIER_READ: u32 = 0x01;
/// `BUS_SPACE_BARRIER_WRITE`: force a write barrier.
pub const BUS_SPACE_BARRIER_WRITE: u32 = 0x02;

/// `bus_space_tag_t`: the selected architecture's tag type.
pub type BusSpaceTag = <Machine as BusSpace>::Tag;
/// `bus_space_handle_t`: the selected architecture's handle type.
pub type BusSpaceHandle = <Machine as BusSpace>::Handle;

/// The `bus_space(9)` access methods.
pub trait BusSpace {
    /// `bus_space_tag_t`: which bus space (port I/O, memory, a particular bus).
    type Tag: Copy;
    /// `bus_space_handle_t`: a mapped region, as returned by `bus_space_map`.
    type Handle: Copy;

    /// `BUS_SPACE_MAP_CACHEABLE`: the region may be cached.
    const BUS_SPACE_MAP_CACHEABLE: u32;
    /// `BUS_SPACE_MAP_LINEAR`: the region must be linear (`bus_space_vaddr` works on it).
    const BUS_SPACE_MAP_LINEAR: u32;
    /// `BUS_SPACE_MAP_PREFETCHABLE`: the region may be prefetched.
    const BUS_SPACE_MAP_PREFETCHABLE: u32;

    /// `bus_space_map`: maps `size` bytes at `addr` in the space `t` names.
    ///
    /// # Safety
    ///
    /// `[addr, addr + size)` must be a device region this driver owns, as the firmware or the
    /// configured console address guarantees; mapping arbitrary memory as a device is undefined.
    unsafe fn bus_space_map(
        t: Self::Tag,
        addr: BusAddr,
        size: BusSize,
        flags: u32,
    ) -> Result<Self::Handle, Errno>;

    /// `bus_space_unmap`: releases a mapping made by `bus_space_map`.
    fn bus_space_unmap(t: Self::Tag, h: Self::Handle, size: BusSize);

    /// `bus_space_read_1`.
    fn bus_space_read_1(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u8;
    /// `bus_space_read_2`.
    fn bus_space_read_2(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u16;
    /// `bus_space_read_4`.
    fn bus_space_read_4(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u32;
    /// `bus_space_write_1`.
    fn bus_space_write_1(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u8);
    /// `bus_space_write_2`.
    fn bus_space_write_2(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u16);
    /// `bus_space_write_4`.
    fn bus_space_write_4(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u32);

    /// `bus_space_barrier`: orders accesses to `[offset, offset + length)` of `h` according to
    /// `flags` ([`BUS_SPACE_BARRIER_READ`], [`BUS_SPACE_BARRIER_WRITE`]).
    fn bus_space_barrier(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        length: BusSize,
        flags: u32,
    );
}

/// `bus_space_map(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusSpace::bus_space_map`].
pub unsafe fn bus_space_map(
    t: BusSpaceTag,
    addr: BusAddr,
    size: BusSize,
    flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_space_map(t, addr, size, flags) }
}

/// `bus_space_unmap(9)` on the selected machine.
pub fn bus_space_unmap(t: BusSpaceTag, h: BusSpaceHandle, size: BusSize) {
    Machine::bus_space_unmap(t, h, size)
}

/// `bus_space_read_1(9)`.
pub fn bus_space_read_1(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u8 {
    Machine::bus_space_read_1(t, h, offset)
}

/// `bus_space_read_2(9)`.
pub fn bus_space_read_2(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u16 {
    Machine::bus_space_read_2(t, h, offset)
}

/// `bus_space_read_4(9)`.
pub fn bus_space_read_4(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u32 {
    Machine::bus_space_read_4(t, h, offset)
}

/// `bus_space_write_1(9)`.
pub fn bus_space_write_1(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u8) {
    Machine::bus_space_write_1(t, h, offset, value)
}

/// `bus_space_write_2(9)`.
pub fn bus_space_write_2(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u16) {
    Machine::bus_space_write_2(t, h, offset, value)
}

/// `bus_space_write_4(9)`.
pub fn bus_space_write_4(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u32) {
    Machine::bus_space_write_4(t, h, offset, value)
}

/// `bus_space_barrier(9)`.
pub fn bus_space_barrier(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    length: BusSize,
    flags: u32,
) {
    Machine::bus_space_barrier(t, h, offset, length, flags)
}
