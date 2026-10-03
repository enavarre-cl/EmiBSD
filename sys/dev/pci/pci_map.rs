/*      $OpenBSD: pci_map.c,v 1.33 2023/04/13 15:07:43 miod Exp $     */
/*	$NetBSD: pci_map.c,v 1.7 2000/05/10 16:58:42 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum; by William R. Studenmund; by Jason R. Thorpe.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! PCI device mapping: `dev/pci/pci_map.c`. Decodes a device's base address registers
//! (BARs) and maps the regions they describe into bus space.
//!
//! Upstream: sys/dev/pci/pci_map.c @ 3ce1f3f79392
//!
//! Section 6.2.5.1 of the PCI specification, "Address Maps", says that firmware should
//! already have mapped the device in a reasonable way, and that a device which wants `2^n`
//! bytes hardwires the bottom `n` address bits to 0: writing all ones while the decoder is
//! disabled and reading back gives the size.
//!
//! ## Deviations
//! - The `basep`/`sizep`/`flagsp`/`typep`/`tagp`/`handlep` out pointers are return values:
//!   `obsd_pci_io_find`, `obsd_pci_mem_find` and `pci_mapreg_info` return
//!   `(base, size, flags)`, `pci_mapreg_assign` `(base, size)`, `pci_mapreg_map`
//!   `(tag, handle, base, size)`, and `pci_mapreg_probe` the type bits (`None` for an
//!   unimplemented register).
//! - `pci_mapreg_map` returns `bus_space_map`'s error where the C returns 1.
//! - `pci_mapreg_assign` cannot place a BAR the firmware left at 0: that needs the bus's
//!   extent (`sys/extent.h`, not ported, so `pa_ioex`/`pa_memex` are always NULL) and fails
//!   with `EINVAL`, the C's answer when there is no extent. `PCI_IO_START`, `PCI_IO_END`,
//!   `PCI_MEM_START` and `PCI_MEM_END` only bound that search and are not needed.
//! - The "bad request" panics happen with or without `DIAGNOSTIC`, as in C; the `DEBUG`
//!   printfs are compiled under feature `debug`.

use crate::dev::pci::pcireg::*;
use crate::dev::pci::pcivar::{PCI_FLAGS_IO_ENABLED, PCI_FLAGS_MEM_ENABLED, PciAttachArgs, Pcireg};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map};
use crate::machine::intr::{splhigh, splx};
use crate::machine::pci_machdep::{PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write};
use crate::machine::{BusSpace, Machine};
use crate::sys::errno::Errno;

/// `printf` under `DEBUG`.
macro_rules! debug_printf {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug")]
        crate::kern::subr_prf::printf(format_args!($($arg)*));
    };
}

/// `obsd_pci_io_find`: the base and size of the I/O BAR at `reg`.
pub fn obsd_pci_io_find(
    pc: PciChipsetTag,
    tag: Pcitag,
    reg: i32,
    _type_: Pcireg,
) -> Result<(BusAddr, BusSize, i32), Errno> {
    // Can't check reg >= PCI_MAPREG_END: some devices have mapping registers way out in
    // left field.
    if reg < PCI_MAPREG_START || reg & 3 != 0 {
        panic(format_args!("pci_io_find: bad request"));
    }

    // Write all 1s while the device is disabled and see what we get back.
    let s = splhigh();
    let csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    if csr & PCI_COMMAND_IO_ENABLE != 0 {
        pci_conf_write(
            pc,
            tag,
            PCI_COMMAND_STATUS_REG,
            csr & !PCI_COMMAND_IO_ENABLE,
        );
    }
    let address = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, 0xffff_ffff);
    let mask = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, address);
    if csr & PCI_COMMAND_IO_ENABLE != 0 {
        pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
    }
    splx(s);

    if PCI_MAPREG_TYPE(address) != PCI_MAPREG_TYPE_IO {
        debug_printf!("pci_io_find: expected type i/o, found mem\n");
        return Err(Errno::EINVAL);
    }

    if pci_mapreg_io_size(mask) == 0 {
        debug_printf!("pci_io_find: void region\n");
        return Err(Errno::ENOENT);
    }

    Ok((
        pci_mapreg_io_addr(address) as BusAddr,
        pci_mapreg_io_size(mask) as BusSize,
        0,
    ))
}

/// `obsd_pci_mem_find`: the base, size and prefetchability of the memory BAR at `reg`
/// (and `reg + 4` for a 64-bit one).
pub fn obsd_pci_mem_find(
    pc: PciChipsetTag,
    tag: Pcitag,
    reg: i32,
    type_: Pcireg,
) -> Result<(BusAddr, BusSize, i32), Errno> {
    let mut address1: Pcireg = 0;
    let mut mask1: Pcireg = 0xffff_ffff;

    let is64bit = pci_mapreg_mem_type(type_) == PCI_MAPREG_MEM_TYPE_64BIT;

    // Can't check reg >= PCI_MAPREG_END (see obsd_pci_io_find).
    if reg < PCI_MAPREG_START || reg & 3 != 0 {
        panic(format_args!("pci_mem_find: bad request"));
    }

    if is64bit && reg + 4 >= PCI_MAPREG_END {
        panic(format_args!("pci_mem_find: bad 64-bit request"));
    }

    // Write all 1s while the device is disabled and see what we get back.
    let s = splhigh();
    let csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    if csr & PCI_COMMAND_MEM_ENABLE != 0 {
        pci_conf_write(
            pc,
            tag,
            PCI_COMMAND_STATUS_REG,
            csr & !PCI_COMMAND_MEM_ENABLE,
        );
    }
    let address = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, PCI_MAPREG_MEM_ADDR_MASK);
    let mask = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, address);
    if is64bit {
        address1 = pci_conf_read(pc, tag, reg + 4);
        pci_conf_write(pc, tag, reg + 4, 0xffff_ffff);
        mask1 = pci_conf_read(pc, tag, reg + 4);
        pci_conf_write(pc, tag, reg + 4, address1);
    }
    if csr & PCI_COMMAND_MEM_ENABLE != 0 {
        pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
    }
    splx(s);

    if PCI_MAPREG_TYPE(address) != PCI_MAPREG_TYPE_MEM {
        debug_printf!("pci_mem_find: expected type mem, found i/o\n");
        return Err(Errno::EINVAL);
    }
    if type_ != u32::MAX && pci_mapreg_mem_type(address) != pci_mapreg_mem_type(type_) {
        debug_printf!(
            "pci_mem_find: expected mem type {:08x}, found {:08x}\n",
            pci_mapreg_mem_type(type_),
            pci_mapreg_mem_type(address)
        );
        return Err(Errno::EINVAL);
    }

    let waddress = (u64::from(address1) << 32) | u64::from(address);
    let wmask = (u64::from(mask1) << 32) | u64::from(mask);

    if (is64bit && pci_mapreg_mem64_size(wmask) == 0)
        || (!is64bit && pci_mapreg_mem_size(mask) == 0)
    {
        debug_printf!("pci_mem_find: void region\n");
        return Err(Errno::ENOENT);
    }

    match pci_mapreg_mem_type(address) {
        PCI_MAPREG_MEM_TYPE_32BIT | PCI_MAPREG_MEM_TYPE_32BIT_1M => {}
        PCI_MAPREG_MEM_TYPE_64BIT => {
            // Handle the case of a 64-bit memory register on a platform with 32-bit
            // addressing: bus_addr_t is 64-bit on every machine here, so nothing to do.
        }
        _ => {
            debug_printf!("pci_mem_find: reserved mapping register type\n");
            return Err(Errno::EINVAL);
        }
    }

    // sizeof(u_int64_t) == sizeof(bus_addr_t): the 64-bit decoding.
    let flags = if pci_mapreg_mem_prefetchable(address) {
        <Machine as BusSpace>::BUS_SPACE_MAP_PREFETCHABLE as i32
    } else {
        0
    };
    Ok((
        pci_mapreg_mem64_addr(waddress) as BusAddr,
        pci_mapreg_mem64_size(wmask) as BusSize,
        flags,
    ))
}

/// `pci_mapreg_type`: the type bits of the BAR at `reg`.
pub fn pci_mapreg_type(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> Pcireg {
    _pci_mapreg_typebits(pci_conf_read(pc, tag, reg))
}

/// `pci_mapreg_probe`: the type bits of the BAR at `reg`, `None` when the register is not
/// implemented.
pub fn pci_mapreg_probe(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> Option<Pcireg> {
    let s = splhigh();
    let csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    let decode = PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE;
    if csr & decode != 0 {
        pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr & !decode);
    }
    let address = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, 0xffff_ffff);
    let mask = pci_conf_read(pc, tag, reg);
    pci_conf_write(pc, tag, reg, address);
    if csr & decode != 0 {
        pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
    }
    splx(s);

    if mask == 0 {
        // unimplemented mapping register
        return None;
    }

    Some(_pci_mapreg_typebits(address))
}

/// `pci_mapreg_info`: the base, size and map flags of the BAR at `reg` of type `type_`.
pub fn pci_mapreg_info(
    pc: PciChipsetTag,
    tag: Pcitag,
    reg: i32,
    type_: Pcireg,
) -> Result<(BusAddr, BusSize, i32), Errno> {
    if PCI_MAPREG_TYPE(type_) == PCI_MAPREG_TYPE_IO {
        obsd_pci_io_find(pc, tag, reg, type_)
    } else {
        obsd_pci_mem_find(pc, tag, reg, type_)
    }
}

/// `pci_mapreg_assign`: the BAR's base and size, with decoding and bus mastering enabled.
pub fn pci_mapreg_assign(
    pa: &PciAttachArgs,
    reg: i32,
    type_: Pcireg,
) -> Result<(BusAddr, BusSize), Errno> {
    let (base, size, _) = pci_mapreg_info(pa.pa_pc, pa.pa_tag, reg, type_)?;
    // !__sparc64__
    if base == 0 {
        // ex = pa->pa_ioex or pa->pa_memex, then extent_alloc_subregion(ex, start, end,
        // size, size, ...): the extents are always NULL here (see the deviations), and
        // without one the BAR is disabled because it is invalid.
        return Err(Errno::EINVAL);
    }

    let mut csr = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG);
    if PCI_MAPREG_TYPE(type_) == PCI_MAPREG_TYPE_IO {
        csr |= PCI_COMMAND_IO_ENABLE;
    } else {
        csr |= PCI_COMMAND_MEM_ENABLE;
    }
    // XXX Should this only be done for devices that do DMA?
    csr |= PCI_COMMAND_MASTER_ENABLE;
    pci_conf_write(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG, csr);

    Ok((base, size))
}

/// `pci_mapreg_map`: assigns and maps the BAR at `reg`; `maxsize` (if not 0) limits the
/// mapping.
pub fn pci_mapreg_map(
    pa: &PciAttachArgs,
    reg: i32,
    type_: Pcireg,
    flags: u32,
    maxsize: BusSize,
) -> Result<(BusSpaceTag, BusSpaceHandle, BusAddr, BusSize), Errno> {
    let (base, mut size) = pci_mapreg_assign(pa, reg, type_)?;

    let tag = if PCI_MAPREG_TYPE(type_) == PCI_MAPREG_TYPE_IO {
        if pa.pa_flags & PCI_FLAGS_IO_ENABLED == 0 {
            return Err(Errno::EINVAL);
        }
        pa.pa_iot
    } else {
        if pa.pa_flags & PCI_FLAGS_MEM_ENABLED == 0 {
            return Err(Errno::EINVAL);
        }
        pa.pa_memt
    };

    // The caller can request limitation of the mapping's size.
    if maxsize != 0 && size > maxsize {
        debug_printf!(
            "pci_mapreg_map: limited PCI mapping from {:x} to {:x}\n",
            size,
            maxsize
        );
        size = maxsize;
    }

    // SAFETY: the region is the one the device's own BAR decodes, which the firmware or
    // pci_mapreg_assign placed; this driver owns the device it was attached to.
    let handle = unsafe { bus_space_map(tag, base, size, flags) }?;

    Ok((tag, handle, base, size))
}

#[cfg(test)]
pub(crate) mod tests;
