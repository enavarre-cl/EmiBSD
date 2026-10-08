/* $OpenBSD: if_em_osdep.h,v 1.15 2024/10/22 21:50:02 jsg Exp $ */
/* $FreeBSD: if_em_osdep.h,v 1.11 2003/05/02 21:17:08 pdeuskar Exp $ */
/* <LICENSES> */
/**************************************************************************

Copyright (c) 2001-2006, Intel Corporation
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

 1. Redistributions of source code must retain the above copyright notice,
    this list of conditions and the following disclaimer.

 2. Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.

 3. Neither the name of the Intel Corporation nor the names of its
    contributors may be used to endorse or promote products derived from
    this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.

***************************************************************************/
/* </LICENSES> */

/* <CODE> */
//! The OS layer of em(4)'s shared code (`<dev/pci/if_em_osdep.h>`): the delays, the register
//! access macros over `bus_space(9)`, and `struct em_osdep`, the part of the softc that holds
//! the register mappings and the PCI attach arguments.
//!
//! Upstream: sys/dev/pci/if_em_osdep.h @ 3ce1f3f79392
//!
//! The C macros take a register *name* and paste `E1000_` in front of it
//! (`E1000_READ_REG(hw, CTRL)`); the functions here take the full constant instead
//! (`e1000_read_reg(hw, E1000_CTRL)`), and translate it for the 82542 exactly as the macro
//! does. All of them reach the softc's [`EmOsdep`] through `hw.back` ([`EmHw::osdep`]).
//!
//! ## Deviations
//! - The macros are functions with lowercase names (`E1000_READ_REG` is [`e1000_read_reg`],
//!   `EM_WRITE_REG_ARRAY` is [`em_write_reg_array`], ...). `E1000_READ_REG_ARRAY_DWORD` and
//!   `E1000_WRITE_REG_ARRAY_DWORD` are the same functions as `E1000_READ_REG_ARRAY` and
//!   `E1000_WRITE_REG_ARRAY`, as in the C.
//! - `DBG` is not defined: `DEBUGFUNC` and `DEBUGOUT*` expand to nothing in the C, and the
//!   shared code's calls to them are left out of the Rust. `DEBUG` is not defined either, and
//!   `EM_KASSERT` and `MSGOUT` are used by no file: none of the three has a counterpart.
//! - `struct em_osdep`'s `dev` is `Option<NonNull<Device>>` (the softc's device, `NULL` until
//!   attach sets it).

use core::ptr::NonNull;

use crate::dev::pci::if_em_hw::{EmHw, em_82543, em_translate_82542_register};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_2, bus_space_read_4,
    bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::sys::device::Device;

/// `CMD_MEM_WRT_INVALIDATE`: Memory Write and Invalidate in the PCI command register
/// (`BIT_4`).
pub const CMD_MEM_WRT_INVALIDATE: u32 = 0x0010;

/// `struct em_osdep`: the register mappings and PCI attach arguments of one em(4) device,
/// filled by if_em.c's attach and reached by the shared code through `hw->back`.
pub struct EmOsdep {
    /// `mem_bus_space_tag`: the register BAR's tag.
    pub mem_bus_space_tag: BusSpaceTag,
    /// `mem_bus_space_handle`: the register BAR's mapping.
    pub mem_bus_space_handle: BusSpaceHandle,
    /// `io_bus_space_tag`: the I/O BAR's tag (82544 and later).
    pub io_bus_space_tag: BusSpaceTag,
    /// `io_bus_space_handle`: the I/O BAR's mapping.
    pub io_bus_space_handle: BusSpaceHandle,
    /// `flash_bus_space_tag`: the ICH flash BAR's tag.
    pub flash_bus_space_tag: BusSpaceTag,
    /// `flash_bus_space_handle`: the ICH flash BAR's mapping.
    pub flash_bus_space_handle: BusSpaceHandle,
    /// `dev`: the softc's device.
    pub dev: Option<NonNull<Device>>,
    /// `em_pa`: the PCI attach arguments, for the configuration space accessors of if_em.c.
    pub em_pa: PciAttachArgs,
    /// `em_memsize`.
    pub em_memsize: BusSize,
    /// `em_membase`.
    pub em_membase: BusAddr,
    /// `em_iosize`.
    pub em_iosize: BusSize,
    /// `em_iobase`.
    pub em_iobase: BusAddr,
    /// `em_flashsize`.
    pub em_flashsize: BusSize,
    /// `em_flashbase`.
    pub em_flashbase: BusAddr,
    /// `em_flashoffset`: where the flash registers start inside the flash mapping.
    pub em_flashoffset: usize,
}

/// `usec_delay(x)`: busy-waits `x` microseconds.
pub fn usec_delay(x: u32) {
    delay(x);
}

/// `msec_delay(x)`: busy-waits `x` milliseconds.
pub fn msec_delay(x: u32) {
    delay(1000 * x);
}

/// `msec_delay_irq(x)`: busy-waits `x` milliseconds ("Should we be paranoid about delaying
/// in interrupt context?").
pub fn msec_delay_irq(x: u32) {
    delay(1000 * x);
}

/// `E1000_WRITE_FLUSH(hw)`: flushes posted writes by reading `STATUS`.
pub fn e1000_write_flush(hw: &EmHw) {
    let _ = e1000_read_reg(hw, crate::dev::pci::if_em_hw::E1000_STATUS);
}

/// `E1000_READ_OFFSET(hw, offset)`: reads from an absolute offset in the adapter's memory
/// space.
pub fn e1000_read_offset(hw: &EmHw, offset: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        offset as BusSize,
    )
}

/// `E1000_WRITE_OFFSET(hw, offset, value)`: writes to an absolute offset in the adapter's
/// memory space.
pub fn e1000_write_offset(hw: &EmHw, offset: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        offset as BusSize,
        value,
    );
}

/// `E1000_REG_TR(hw, reg)`: converts a register's offset to its offset in the adapter's
/// memory space (the 82542 places some registers elsewhere).
pub fn e1000_reg_tr(hw: &EmHw, reg: u32) -> u32 {
    if hw.mac_type >= em_82543 {
        reg
    } else {
        em_translate_82542_register(reg)
    }
}

/// `E1000_READ_REG(hw, reg)`: reads register `reg` (an `E1000_*` offset).
pub fn e1000_read_reg(hw: &EmHw, reg: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        e1000_reg_tr(hw, reg) as BusSize,
    )
}

/// `E1000_WRITE_REG(hw, reg, value)`: writes register `reg` (an `E1000_*` offset).
pub fn e1000_write_reg(hw: &EmHw, reg: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        e1000_reg_tr(hw, reg) as BusSize,
        value,
    );
}

/// `EM_READ_REG(hw, reg)`: reads the register at offset `reg`, untranslated.
pub fn em_read_reg(hw: &EmHw, reg: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(o.mem_bus_space_tag, o.mem_bus_space_handle, reg as BusSize)
}

/// `EM_WRITE_REG(hw, reg, value)`: writes the register at offset `reg`, untranslated.
pub fn em_write_reg(hw: &EmHw, reg: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        reg as BusSize,
        value,
    );
}

/// `EM_READ_REG_ARRAY(hw, reg, index)`: reads the 32-bit element `index` of the register
/// array at `reg`, untranslated.
pub fn em_read_reg_array(hw: &EmHw, reg: u32, index: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (reg + (index << 2)) as BusSize,
    )
}

/// `EM_WRITE_REG_ARRAY(hw, reg, index, value)`: writes the 32-bit element `index` of the
/// register array at `reg`, untranslated.
pub fn em_write_reg_array(hw: &EmHw, reg: u32, index: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (reg + (index << 2)) as BusSize,
        value,
    );
}

/// `E1000_READ_REG_ARRAY(hw, reg, index)` (and `E1000_READ_REG_ARRAY_DWORD`): reads the
/// 32-bit element `index` of the register array `reg`.
pub fn e1000_read_reg_array(hw: &EmHw, reg: u32, index: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (e1000_reg_tr(hw, reg) + (index << 2)) as BusSize,
    )
}

/// `E1000_WRITE_REG_ARRAY(hw, reg, index, value)` (and `E1000_WRITE_REG_ARRAY_DWORD`):
/// writes the 32-bit element `index` of the register array `reg`.
pub fn e1000_write_reg_array(hw: &EmHw, reg: u32, index: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (e1000_reg_tr(hw, reg) + (index << 2)) as BusSize,
        value,
    );
}

/// `E1000_WRITE_REG_ARRAY_BYTE(hw, reg, index, value)`: writes byte `index` of the register
/// array `reg`.
pub fn e1000_write_reg_array_byte(hw: &EmHw, reg: u32, index: u32, value: u8) {
    let o = hw.osdep();
    bus_space_write_1(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (e1000_reg_tr(hw, reg) + index) as BusSize,
        value,
    );
}

/// `E1000_WRITE_REG_ARRAY_WORD(hw, reg, index, value)`: writes 16-bit element `index` of the
/// register array `reg`.
pub fn e1000_write_reg_array_word(hw: &EmHw, reg: u32, index: u32, value: u16) {
    let o = hw.osdep();
    bus_space_write_2(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        (e1000_reg_tr(hw, reg) + (index << 1)) as BusSize,
        value,
    );
}

/// `E1000_READ_ICH_FLASH_REG(hw, reg)`: reads a 32-bit ICH flash register.
pub fn e1000_read_ich_flash_reg(hw: &EmHw, reg: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
    )
}

/// `E1000_READ_ICH_FLASH_REG16(hw, reg)`: reads a 16-bit ICH flash register.
pub fn e1000_read_ich_flash_reg16(hw: &EmHw, reg: u32) -> u16 {
    let o = hw.osdep();
    bus_space_read_2(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
    )
}

/// `E1000_READ_ICH_FLASH_REG32(hw, reg)`: reads a 32-bit ICH flash register.
pub fn e1000_read_ich_flash_reg32(hw: &EmHw, reg: u32) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
    )
}

/// `E1000_WRITE_ICH_FLASH_REG8(hw, reg, value)`: writes an 8-bit ICH flash register.
pub fn e1000_write_ich_flash_reg8(hw: &EmHw, reg: u32, value: u8) {
    let o = hw.osdep();
    bus_space_write_1(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
        value,
    );
}

/// `E1000_WRITE_ICH_FLASH_REG16(hw, reg, value)`: writes a 16-bit ICH flash register.
pub fn e1000_write_ich_flash_reg16(hw: &EmHw, reg: u32, value: u16) {
    let o = hw.osdep();
    bus_space_write_2(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
        value,
    );
}

/// `E1000_WRITE_ICH_FLASH_REG32(hw, reg, value)`: writes a 32-bit ICH flash register.
pub fn e1000_write_ich_flash_reg32(hw: &EmHw, reg: u32, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.flash_bus_space_tag,
        o.flash_bus_space_handle,
        o.em_flashoffset + reg as usize,
        value,
    );
}

/// `em_io_read(hw, port)`: reads a 32-bit word of the I/O BAR.
pub fn em_io_read(hw: &EmHw, port: u64) -> u32 {
    let o = hw.osdep();
    bus_space_read_4(o.io_bus_space_tag, o.io_bus_space_handle, port as BusSize)
}

/// `em_io_write(hw, port, value)`: writes a 32-bit word of the I/O BAR.
pub fn em_io_write(hw: &EmHw, port: u64, value: u32) {
    let o = hw.osdep();
    bus_space_write_4(
        o.io_bus_space_tag,
        o.io_bus_space_handle,
        port as BusSize,
        value,
    );
}
/* </CODE> */
