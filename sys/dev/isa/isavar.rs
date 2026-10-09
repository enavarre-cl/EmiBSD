/* <LICENSES> */
/*-
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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

/*
 * Copyright (c) 1996 Christos Zoulas.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Christos Zoulas.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1995 Chris G. Demetriou
 * Copyright (c) 1992 Berkeley Software Design, Inc.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Berkeley Software
 *	Design, Inc.
 * 4. The name of Berkeley Software Design must not be used to endorse
 *    or promote products derived from this software without specific
 *    prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY BERKELEY SOFTWARE DESIGN, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL BERKELEY SOFTWARE DESIGN, INC. BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	BSDI Id: isavar.h,v 1.5 1992/12/01 18:06:00 karels Exp
 */
/* </LICENSES> */

/* <CODE> */
//! The ISA bus's shared definitions: `<dev/isa/isavar.h>`.
//!
//! Upstream: sys/dev/isa/isavar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `NISADMA` is 0: `isadma0 at isa?` is in GENERIC, but `isadma.c` is not ported, so the
//!   `#if NISADMA > 0` members (`iba_dmat`, `ia_dmat`, the DMA state of `struct isa_softc`)
//!   and the `ISA_DRQ_*` macros over them are left out, as for a kernel without it.
//!   `NISAPNP` is 0 too (ISA PnP is not in amd64's GENERIC): `struct isapnp_softc`, the
//!   `ISAPNP_*` register macros and the `isapnp_*` prototypes are for `isapnp.c`, which is
//!   not ported; the PnP members of `struct isa_attach_args` are kept, since the compatibility
//!   names (`ia_iobase`, `ia_irq`, ...) live in them.
//! - The `ia_*` compatibility macros (`#define ia_iobase ipa_io[0].base`) are accessor
//!   methods ([`IsaAttachArgs::ia_iobase`], [`IsaAttachArgs::set_ia_iobase`], ...), and the
//!   `cf_*` locator macros (`#define cf_iobase cf_loc[0]`) are functions of a `Cfdata`.
//! - `struct isapnp_pin`'s `flags:4, type:4` bit-fields are one byte (`flags_type`), the
//!   flags in the low nibble.
//! - The `isa_softc`'s `sc_subdevs` list and `struct isadev` are kept; nothing links an
//!   `isadev` yet (only `isapnp.c` does).

use core::ffi::c_void;

use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use crate::machine::isa_machdep::IsaChipsetTag;
use crate::queue_adapter;
use crate::sys::device::{Cfdata, Device, Softc};
use crate::sys::queue::{TailqEntry, TailqHead};

/// Most cards one ISA PnP bus may have.
pub const ISAPNP_MAX_CARDS: usize = 8;
/// Length of a PnP identifier.
pub const ISAPNP_MAX_IDENT: usize = 32;
/// Length of a PnP device class.
pub const ISAPNP_MAX_DEVCLASS: usize = 16;
/// Length of a PnP serial.
pub const ISAPNP_SERIAL_SIZE: usize = 9;

/// Memory regions of a PnP device.
pub const ISAPNP_NUM_MEM: usize = 4;
/// I/O regions of a PnP device.
pub const ISAPNP_NUM_IO: usize = 8;
/// Interrupt lines of a PnP device.
pub const ISAPNP_NUM_IRQ: usize = 16;
/// DMA lines of a PnP device.
pub const ISAPNP_NUM_DRQ: usize = 8;
/// 32-bit memory regions of a PnP device.
pub const ISAPNP_NUM_MEM32: usize = 4;

/// `IOBASEUNK`: i/o address is unknown.
pub const IOBASEUNK: i32 = -1;
/// `IRQUNK`: interrupt request line is unknown.
pub const IRQUNK: i32 = -1;
/// `DRQUNK`: DMA request line is unknown.
pub const DRQUNK: i32 = -1;
/// `MADDRUNK`: shared memory address is unknown.
pub const MADDRUNK: i32 = -1;

/// `struct isapnp_region`: one I/O or memory range.
#[derive(Clone, Copy, Default)]
pub struct IsapnpRegion {
    /// `h`: the mapping.
    pub h: Option<BusSpaceHandle>,
    /// `base`.
    pub base: u32,
    /// `minbase`.
    pub minbase: u32,
    /// `maxbase`.
    pub maxbase: u32,
    /// `length`.
    pub length: u32,
    /// `align`.
    pub align: u32,
    /// `flags`.
    pub flags: u8,
}

/// `struct isapnp_pin`: one interrupt or DMA line.
#[derive(Clone, Copy, Debug, Default)]
pub struct IsapnpPin {
    /// `num`.
    pub num: i16,
    /// `flags:4` (low nibble) and `type:4` (high nibble).
    pub flags_type: u8,
    /// `bits`.
    pub bits: u16,
}

/// `struct isapnp_knowndev`: a PnP identifier and the driver for it.
pub struct IsapnpKnowndev {
    /// `pnpid`.
    pub pnpid: [u8; 8],
    /// `driver`.
    pub driver: [u8; 5],
}

/// `struct isabus_attach_args`: ISA bus attach arguments.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IsabusAttachArgs {
    /// `iba_busname`: XXX should be common.
    pub iba_busname: &'static [u8],
    /// `iba_iot`: isa i/o space tag.
    pub iba_iot: BusSpaceTag,
    /// `iba_memt`: isa mem space tag.
    pub iba_memt: BusSpaceTag,
    // iba_dmat: NISADMA > 0.
    /// `iba_ic`.
    pub iba_ic: IsaChipsetTag,
}

/// `struct isa_attach_args`: ISA driver attach arguments.
#[derive(Clone, Copy)]
pub struct IsaAttachArgs {
    /// `ia_isa`: isa device.
    pub ia_isa: *mut Device,
    /// `ia_iot`: isa i/o space tag.
    pub ia_iot: BusSpaceTag,
    /// `ia_memt`: isa mem space tag.
    pub ia_memt: BusSpaceTag,
    // ia_dmat: NISADMA > 0.
    /// `ia_delaybah`: i/o handle for `delay port'.
    pub ia_delaybah: Option<BusSpaceHandle>,
    /// `ia_ic`.
    pub ia_ic: IsaChipsetTag,

    // ISA PnP configuration support. `ipa_' prefixes are used to denote PnP specific members
    // of this structure.
    /// `ipa_sibling`.
    pub ipa_sibling: *mut IsaAttachArgs,
    /// `ipa_child`.
    pub ipa_child: *mut IsaAttachArgs,
    /// `ipa_devident`.
    pub ipa_devident: [u8; ISAPNP_MAX_IDENT],
    /// `ipa_devlogic`.
    pub ipa_devlogic: [u8; ISAPNP_MAX_DEVCLASS],
    /// `ipa_devcompat`.
    pub ipa_devcompat: [u8; ISAPNP_MAX_DEVCLASS],
    /// `ipa_devclass`.
    pub ipa_devclass: [u8; ISAPNP_MAX_DEVCLASS],
    /// `ipa_pref`.
    pub ipa_pref: u8,
    /// `ipa_devnum`.
    pub ipa_devnum: u8,
    /// `ipa_nio`.
    pub ipa_nio: u8,
    /// `ipa_nirq`.
    pub ipa_nirq: u8,
    /// `ipa_ndrq`.
    pub ipa_ndrq: u8,
    /// `ipa_nmem`.
    pub ipa_nmem: u8,
    /// `ipa_nmem32`.
    pub ipa_nmem32: u8,
    /// `ipa_io`.
    pub ipa_io: [IsapnpRegion; ISAPNP_NUM_IO],
    /// `ipa_mem`.
    pub ipa_mem: [IsapnpRegion; ISAPNP_NUM_MEM],
    /// `ipa_mem32`.
    pub ipa_mem32: [IsapnpRegion; ISAPNP_NUM_MEM32],
    /// `ipa_irq`.
    pub ipa_irq: [IsapnpPin; ISAPNP_NUM_IRQ],
    /// `ipa_drq`.
    pub ipa_drq: [IsapnpPin; ISAPNP_NUM_DRQ],

    /// `ia_aux`: driver specific.
    pub ia_aux: *mut c_void,
}

impl IsaAttachArgs {
    /// `ia_iobase` (`ipa_io[0].base`).
    pub fn ia_iobase(&self) -> i32 {
        self.ipa_io[0].base as i32
    }
    /// `ia_iobase = v`.
    pub fn set_ia_iobase(&mut self, v: i32) {
        self.ipa_io[0].base = v as u32;
    }
    /// `ia_iosize` (`ipa_io[0].length`).
    pub fn ia_iosize(&self) -> i32 {
        self.ipa_io[0].length as i32
    }
    /// `ia_iosize = v`.
    pub fn set_ia_iosize(&mut self, v: i32) {
        self.ipa_io[0].length = v as u32;
    }
    /// `ia_ioh` (`ipa_io[0].h`).
    pub fn ia_ioh(&self) -> Option<BusSpaceHandle> {
        self.ipa_io[0].h
    }
    /// `ia_irq` (`ipa_irq[0].num`).
    pub fn ia_irq(&self) -> i32 {
        i32::from(self.ipa_irq[0].num)
    }
    /// `ia_irq = v`.
    pub fn set_ia_irq(&mut self, v: i32) {
        self.ipa_irq[0].num = v as i16;
    }
    /// `ia_drq` (`ipa_drq[0].num`).
    pub fn ia_drq(&self) -> i32 {
        i32::from(self.ipa_drq[0].num)
    }
    /// `ia_drq = v`.
    pub fn set_ia_drq(&mut self, v: i32) {
        self.ipa_drq[0].num = v as i16;
    }
    /// `ia_drq2` (`ipa_drq[1].num`).
    pub fn ia_drq2(&self) -> i32 {
        i32::from(self.ipa_drq[1].num)
    }
    /// `ia_drq2 = v`.
    pub fn set_ia_drq2(&mut self, v: i32) {
        self.ipa_drq[1].num = v as i16;
    }
    /// `ia_maddr` (`ipa_mem[0].base`).
    pub fn ia_maddr(&self) -> i32 {
        self.ipa_mem[0].base as i32
    }
    /// `ia_maddr = v`.
    pub fn set_ia_maddr(&mut self, v: i32) {
        self.ipa_mem[0].base = v as u32;
    }
    /// `ia_msize` (`ipa_mem[0].length`).
    pub fn ia_msize(&self) -> i32 {
        self.ipa_mem[0].length as i32
    }
    /// `ia_msize = v`.
    pub fn set_ia_msize(&mut self, v: i32) {
        self.ipa_mem[0].length = v as u32;
    }
    /// `ia_memh` (`ipa_mem[0].h`).
    pub fn ia_memh(&self) -> Option<BusSpaceHandle> {
        self.ipa_mem[0].h
    }
}

/// `struct isadev`: a child on the bus's list.
pub struct Isadev {
    /// `id_dev`: back pointer to generic.
    pub id_dev: *mut Device,
    /// `id_bchain`: bus chain.
    pub id_bchain: TailqEntry<Isadev>,
}

queue_adapter!(
    /// `TAILQ_ENTRY(isadev) id_bchain`.
    pub IsadevBchain: Isadev, id_bchain => TailqEntry<Isadev>
);

/// `struct isa_softc`: ISA master bus.
#[repr(C)]
pub struct IsaSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_subdevs`: list of all children.
    pub sc_subdevs: TailqHead<IsadevBchain>,
    /// `sc_iot`: isa io space tag.
    pub sc_iot: core::cell::Cell<Option<BusSpaceTag>>,
    /// `sc_memt`: isa mem space tag.
    pub sc_memt: core::cell::Cell<Option<BusSpaceTag>>,
    // sc_dmat, sc_drqmap, sc_dma1h, sc_dma2h, sc_dmapgh, sc_dmamaps, sc_dmalength,
    // sc_dmareads, sc_dmafinished: NISADMA > 0.
    /// `sc_ic`.
    pub sc_ic: core::cell::Cell<Option<IsaChipsetTag>>,
    /// `sc_delaybah`: this i/o handle is used to map port 0x84, which is read to provide a
    /// 1.25us delay. This access handle is mapped in isaattach(), and exported to drivers via
    /// isa_attach_args.
    pub sc_delaybah: core::cell::Cell<Option<BusSpaceHandle>>,
}

// SAFETY: `repr(C)` with the device first; the list head is null pointers and the cells
// `None` when all-zero.
unsafe impl Softc for IsaSoftc {}

/// `cf_iobase` (`cf_loc[0]`).
pub fn cf_iobase(cf: &Cfdata) -> i64 {
    cf.cf_loc.first().copied().unwrap_or(-1)
}
/// `cf_iosize` (`cf_loc[1]`).
pub fn cf_iosize(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(1).copied().unwrap_or(0)
}
/// `cf_maddr` (`cf_loc[2]`).
pub fn cf_maddr(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(2).copied().unwrap_or(-1)
}
/// `cf_msize` (`cf_loc[3]`).
pub fn cf_msize(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(3).copied().unwrap_or(0)
}
/// `cf_irq` (`cf_loc[4]`).
pub fn cf_irq(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(4).copied().unwrap_or(-1)
}
/// `cf_drq` (`cf_loc[5]`).
pub fn cf_drq(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(5).copied().unwrap_or(-1)
}
/// `cf_drq2` (`cf_loc[6]`).
pub fn cf_drq2(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(6).copied().unwrap_or(-1)
}

// ISABUS_DMA_32BIT (BUS_DMA_BUS1): NISADMA > 0.
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/isavar.h");
        let ours: &[(&str, i64)] = &[
            ("ISAPNP_MAX_CARDS", ISAPNP_MAX_CARDS as i64),
            ("ISAPNP_MAX_IDENT", ISAPNP_MAX_IDENT as i64),
            ("ISAPNP_NUM_IO", ISAPNP_NUM_IO as i64),
            ("ISAPNP_NUM_IRQ", ISAPNP_NUM_IRQ as i64),
            ("IOBASEUNK", IOBASEUNK.into()),
            ("IRQUNK", IRQUNK.into()),
            ("DRQUNK", DRQUNK.into()),
            ("MADDRUNK", MADDRUNK.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
