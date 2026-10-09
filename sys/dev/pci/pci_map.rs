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

/* <CODE> */
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
//! - `pci_mapreg_assign` places a BAR the firmware left at 0 in the bus's extent (M16b),
//!   bounded by the machine's `PCI_IO_START`..`PCI_MEM_END` (`<machine/pci_machdep.h>`,
//!   the C's `#ifndef` defaults on arm64); amd64's buses have no extents yet, so it never
//!   gets here (`EINVAL`, as the C without an extent).
//! - The "bad request" panics happen with or without `DIAGNOSTIC`, as in C; the `DEBUG`
//!   printfs are compiled under feature `debug`.

use crate::dev::pci::pcireg::*;
use crate::dev::pci::pcivar::{PCI_FLAGS_IO_ENABLED, PCI_FLAGS_MEM_ENABLED, PciAttachArgs, Pcireg};
use crate::kern::subr_extent::extent_alloc_subregion;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map};
use crate::machine::intr::{splhigh, splx};
use crate::machine::pci_machdep::{
    PCI_IO_END, PCI_IO_START, PCI_MEM_END, PCI_MEM_START, PciChipsetTag, Pcitag, pci_conf_read,
    pci_conf_write,
};
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
    let (mut base, size, _) = pci_mapreg_info(pa.pa_pc, pa.pa_tag, reg, type_)?;
    // !__sparc64__
    if base == 0 {
        let (ex, start, end) = if PCI_MAPREG_TYPE(type_) == PCI_MAPREG_TYPE_IO {
            (pa.pa_ioex, PCI_IO_START, PCI_IO_END)
        } else {
            (pa.pa_memex, PCI_MEM_START, PCI_MEM_END)
        };
        // disabled because of invalid BAR
        let Some(ex) = ex else {
            return Err(Errno::EINVAL);
        };
        let start = start.max(ex.ex_start);
        let end = end.min(ex.ex_end);
        let Ok(b) = extent_alloc_subregion(ex, start, end, size as u64, size as u64, 0, 0, 0)
        else {
            return Err(Errno::EINVAL);
        };
        base = b as BusAddr;

        pci_conf_write(pa.pa_pc, pa.pa_tag, reg, base as Pcireg);
        if pci_mapreg_mem_type(type_) == PCI_MAPREG_MEM_TYPE_64BIT {
            pci_conf_write(
                pa.pa_pc,
                pa.pa_tag,
                reg + 4,
                ((base as u64) >> 32) as Pcireg,
            );
        }
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests of the BAR decoding, over a fake configuration space the host double serves
    // (`Machine::set_pci_conf`). [`FakePci`], [`with_fake`] and [`attach_args`] are shared with
    // `pci.rs`'s tests.

    use std::boxed::Box;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, MutexGuard};

    use super::*;
    use crate::dev::pci::pcivar::PCI_FLAGS_MEM_ENABLED;
    use crate::machine::Machine;
    use crate::machine::pci_machdep::{pci_decompose_tag, pci_make_tag};

    /// One function's configuration space: 64 registers, and the size each BAR decodes (0 for
    /// an unimplemented one) with its type bits.
    #[derive(Clone)]
    pub(crate) struct FakeFunc {
        pub regs: [u32; 64],
        /// For each of the six BARs: (size in bytes, type bits); a 64-bit BAR's size is given on
        /// its low register and the high register follows.
        pub bars: [(u64, u32); 6],
    }

    impl Default for FakeFunc {
        fn default() -> Self {
            Self {
                regs: [0; 64],
                bars: [(0, 0); 6],
            }
        }
    }

    /// A bus of fake functions, by tag.
    #[derive(Default)]
    pub(crate) struct FakePci {
        pub funcs: BTreeMap<(i32, i32, i32), FakeFunc>,
    }

    impl FakePci {
        /// Adds a function with the given ID and class registers; header type and the rest zero.
        pub fn add(&mut self, bus: i32, dev: i32, func: i32, id: u32, class: u32) -> &mut FakeFunc {
            let f = self.funcs.entry((bus, dev, func)).or_default();
            f.regs[0] = id;
            f.regs[2] = class;
            f
        }
    }

    impl FakeFunc {
        /// Sets the BHLC register's header type byte.
        pub fn hdrtype(&mut self, ty: u32) -> &mut Self {
            self.regs[3] = (self.regs[3] & !0x00ff_0000) | (ty << 16);
            self
        }

        /// A BAR at index `i` decoding `size` bytes of `typebits`, assigned `base`.
        pub fn bar(&mut self, i: usize, size: u64, typebits: u32, base: u64) -> &mut Self {
            self.bars[i] = (size, typebits);
            self.regs[4 + i] = (base as u32 & !0xf) | typebits;
            if typebits & PCI_MAPREG_MEM_TYPE_64BIT != 0 && typebits & 1 == 0 {
                self.regs[5 + i] = (base >> 32) as u32;
            }
            self
        }
    }

    /// The BAR (index, high half of a 64-bit one) register `reg` (a word index) belongs to.
    fn bar_of(f: &FakeFunc, reg: usize) -> Option<(usize, bool)> {
        if !(4..10).contains(&reg) {
            return None;
        }
        let i = reg - 4;
        if i > 0 {
            let (size, ty) = f.bars[i - 1];
            if size != 0 && ty & 1 == 0 && ty & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
                return Some((i - 1, true));
            }
        }
        Some((i, false))
    }

    /// A read of the fake: all ones where there is no function.
    fn fake_read(fake: &Mutex<FakePci>, tag: Pcitag, reg: i32) -> u32 {
        let fake = fake.lock().unwrap();
        match fake.funcs.get(&pci_decompose_tag(pc(), tag)) {
            Some(f) => f.regs[(reg / 4) as usize],
            None => 0xffff_ffff,
        }
    }

    /// A write to the fake: a BAR keeps only the address bits its size leaves writable.
    fn fake_write(fake: &Mutex<FakePci>, tag: Pcitag, reg: i32, data: u32) {
        let mut fake = fake.lock().unwrap();
        let Some(f) = fake.funcs.get_mut(&pci_decompose_tag(pc(), tag)) else {
            return;
        };
        let r = (reg / 4) as usize;
        match bar_of(f, r) {
            Some((i, high)) => {
                let (size, ty) = f.bars[i];
                let mask = !(size.max(1) - 1);
                f.regs[r] = if size == 0 {
                    0
                } else if high {
                    data & (mask >> 32) as u32
                } else if ty & 1 != 0 {
                    (data & mask as u32 & !0x3) | ty
                } else {
                    (data & mask as u32 & !0xf) | ty
                };
            }
            None => f.regs[r] = data,
        }
    }

    /// The host's chipset tag.
    pub(crate) fn pc() -> PciChipsetTag {
        Default::default()
    }

    /// Serialises the tests that install a configuration space.
    static LOCK: Mutex<()> = Mutex::new(());

    /// Installs `fake` for the duration of `f`.
    pub(crate) fn with_fake<R>(fake: FakePci, f: impl FnOnce(&Arc<Mutex<FakePci>>) -> R) -> R {
        let _guard: MutexGuard<'_, ()> = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let shared = Arc::new(Mutex::new(fake));
        let (r_fake, w_fake) = (shared.clone(), shared.clone());
        Machine::set_pci_conf(Some((
            Box::new(move |tag, reg| fake_read(&r_fake, tag, reg)),
            Box::new(move |tag, reg, data| fake_write(&w_fake, tag, reg, data)),
        )));
        let r = f(&shared);
        Machine::set_pci_conf(None);
        r
    }

    /// Attach arguments for function `(bus, dev, func)` of the fake.
    pub(crate) fn attach_args(bus: i32, dev: i32, func: i32) -> PciAttachArgs {
        let tag = pci_make_tag(pc(), bus, dev, func);
        PciAttachArgs {
            pa_iot: Default::default(),
            pa_memt: Default::default(),
            pa_dmat: Default::default(),
            pa_pc: pc(),
            pa_flags: PCI_FLAGS_IO_ENABLED | PCI_FLAGS_MEM_ENABLED,
            pa_ioex: None,
            pa_memex: None,
            pa_pmemex: None,
            pa_busex: None,
            pa_domain: 0,
            pa_bus: bus as u32,
            pa_device: dev as u32,
            pa_function: func as u32,
            pa_tag: tag,
            pa_id: 0,
            pa_class: 0,
            pa_bridgetag: None,
            pa_bridgeih: None,
            pa_intrswiz: 0,
            pa_intrtag: tag,
            pa_intrpin: 0,
            pa_intrline: 0,
            pa_rawintrpin: 0,
        }
    }

    fn virtio_like() -> FakePci {
        let mut fake = FakePci::default();
        fake.add(0, 3, 0, 0x1000_1af4, 0x0200_0000)
            .bar(0, 0x20, PCI_MAPREG_TYPE_IO, 0xc040)
            .bar(1, 0x1000, PCI_MAPREG_TYPE_MEM, 0xfebd_1000)
            .bar(
                4,
                0x4000,
                PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT | PCI_MAPREG_MEM_PREFETCHABLE_MASK,
                0xfe00_0000,
            );
        fake
    }

    #[test]
    fn bar_sizes_are_decoded() {
        with_fake(virtio_like(), |shared| {
            let tag = pci_make_tag(pc(), 0, 3, 0);
            // Decoding is enabled; the probe must disable and restore it.
            shared
                .lock()
                .unwrap()
                .funcs
                .get_mut(&(0, 3, 0))
                .unwrap()
                .regs[1] = 0x7;

            let io = pci_mapreg_type(pc(), tag, 0x10);
            assert_eq!(io, PCI_MAPREG_TYPE_IO);
            assert_eq!(pci_mapreg_info(pc(), tag, 0x10, io), Ok((0xc040, 0x20, 0)));

            let mem = pci_mapreg_type(pc(), tag, 0x14);
            assert_eq!(mem, PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT);
            assert_eq!(
                pci_mapreg_info(pc(), tag, 0x14, mem),
                Ok((0xfebd_1000, 0x1000, 0))
            );

            let mem64 = pci_mapreg_type(pc(), tag, 0x20);
            assert_eq!(mem64, PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT);
            let prefetchable =
                <crate::machine::Machine as crate::machine::BusSpace>::BUS_SPACE_MAP_PREFETCHABLE;
            assert_eq!(
                pci_mapreg_info(pc(), tag, 0x20, mem64),
                Ok((0xfe00_0000, 0x4000, prefetchable as i32))
            );

            // The BARs and the command register read back as before.
            let fake = shared.lock().unwrap();
            let f = &fake.funcs[&(0, 3, 0)];
            assert_eq!(f.regs[1], 0x7);
            assert_eq!(f.regs[4], 0xc041);
            assert_eq!(f.regs[5], 0xfebd_1000);
        });
    }

    #[test]
    fn probe_type_mismatch_and_void_regions() {
        with_fake(virtio_like(), |_| {
            let tag = pci_make_tag(pc(), 0, 3, 0);
            assert_eq!(pci_mapreg_probe(pc(), tag, 0x10), Some(PCI_MAPREG_TYPE_IO));
            // BAR 2 (0x18) is not implemented.
            assert_eq!(pci_mapreg_probe(pc(), tag, 0x18), None);
            // An I/O BAR asked for as memory, and the other way round.
            assert_eq!(
                pci_mapreg_info(pc(), tag, 0x10, PCI_MAPREG_TYPE_MEM),
                Err(Errno::EINVAL)
            );
            assert_eq!(
                pci_mapreg_info(pc(), tag, 0x14, PCI_MAPREG_TYPE_IO),
                Err(Errno::EINVAL)
            );
            // A 32-bit BAR asked for as 64-bit.
            assert_eq!(
                pci_mapreg_info(
                    pc(),
                    tag,
                    0x14,
                    PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT
                ),
                Err(Errno::EINVAL)
            );
            // An unimplemented BAR decodes nothing: a void region.
            assert_eq!(
                pci_mapreg_info(pc(), tag, 0x18, PCI_MAPREG_TYPE_MEM),
                Err(Errno::ENOENT)
            );
        });
    }

    #[test]
    fn assign_enables_decoding_and_mastering() {
        let mut fake = virtio_like();
        fake.add(0, 4, 0, 0x1000_1af4, 0)
            .bar(0, 0x1000, PCI_MAPREG_TYPE_MEM, 0);
        with_fake(fake, |shared| {
            let pa = attach_args(0, 3, 0);
            assert_eq!(
                pci_mapreg_assign(&pa, 0x14, PCI_MAPREG_TYPE_MEM),
                Ok((0xfebd_1000, 0x1000))
            );
            let csr = shared.lock().unwrap().funcs[&(0, 3, 0)].regs[1];
            assert_eq!(csr, PCI_COMMAND_MEM_ENABLE | PCI_COMMAND_MASTER_ENABLE);

            // A BAR the firmware left at 0 needs an extent to be placed; there is none.
            let mut pa = attach_args(0, 4, 0);
            assert_eq!(
                pci_mapreg_assign(&pa, 0x10, PCI_MAPREG_TYPE_MEM),
                Err(Errno::EINVAL)
            );

            // With the bus's memory extent (its window free, the rest taken), the BAR is
            // placed at the window's first aligned address and written back.
            let storage = std::boxed::Box::leak(std::vec![0u8; 4096].into_boxed_slice());
            let ex = crate::kern::subr_extent::extent_create(
                b"test pcimem",
                0,
                u64::MAX,
                crate::sys::malloc::M_DEVBUF,
                Some(storage),
                crate::sys::extent::EX_NOWAIT | crate::sys::extent::EX_FILLED,
            )
            .unwrap();
            crate::kern::subr_extent::extent_free(
                ex,
                0x1000_0800,
                0x10_0000,
                crate::sys::extent::EX_NOWAIT,
            )
            .unwrap();
            pa.pa_memex = Some(ex);
            assert_eq!(
                pci_mapreg_assign(&pa, 0x10, PCI_MAPREG_TYPE_MEM),
                Ok((0x1000_1000, 0x1000))
            );
            assert_eq!(pci_conf_read(pa.pa_pc, pa.pa_tag, 0x10), 0x1000_1000);
            // The I/O extent is still missing.
            assert!(pci_mapreg_assign(&pa, 0x10, PCI_MAPREG_TYPE_IO).is_err());

            // pci_mapreg_map limits the size and maps through bus_space.
            let pa = attach_args(0, 3, 0);
            let (_, _, base, size) =
                pci_mapreg_map(&pa, 0x10, PCI_MAPREG_TYPE_IO, 0, 0x10).unwrap();
            assert_eq!((base, size), (0xc040, 0x10));
        });
    }
}
/* </TESTS> */
