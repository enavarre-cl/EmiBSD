/*	$OpenBSD: segments.h,v 1.18 2025/06/27 17:23:49 bluhm Exp $	*/
/*	$NetBSD: segments.h,v 1.1 2003/04/26 18:39:47 fvdl Exp $	*/

/*-
 * Copyright (c) 1995, 1997
 *	Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1989, 1990 William F. Jolitz
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)segments.h	7.1 (Berkeley) 5/9/91
 */

//! amd64 `<machine/segments.h>`: 386 Segmentation Data Structures and definitions (William F.
//! Jolitz, 6/20/1989), adapted for NetBSD/amd64 by fvdl@wasabisystems.com.
//!
//! Upstream: sys/arch/amd64/include/segments.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the selectors, the descriptor types, the gate and
//! segment type numbers and the GDT/IDT layout; `idt_vec_alloc_range` and the user selector
//! checks are used from M6.
//!
//! ## Deviations
//! - The C's bit-field structs are one or two `u64` words here, with `const fn` builders that
//!   place each field where the hardware reads it; `GDT_ADDR_MEM`/`GDT_ADDR_SYS` are index
//!   helpers over the GDT's words.

// Selectors

/// `SEL_KPL`: kernel privilege level.
pub const SEL_KPL: u16 = 0;
/// `SEL_UPL`: user privilege level.
pub const SEL_UPL: u16 = 3;
/// `SEL_RPL`: requester's privilege level mask.
pub const SEL_RPL: u16 = 3;
/// `SEL_LDT`: local descriptor table.
pub const SEL_LDT: u16 = 4;

/// `ISPL(s)`: what is the priority level of a selector.
pub const fn ispl(s: u16) -> u16 {
    s & SEL_RPL
}

/// `ISLDT(s)`: is it local or global.
pub const fn isldt(s: u16) -> u16 {
    s & SEL_LDT
}

/// `NGDT_MEM`: the code and data descriptors come first; there are this many of them.
pub const NGDT_MEM: usize = 5;
/// `NGDT_SYS`: then the predefined TSS descriptors.
pub const NGDT_SYS: usize = 1;
/// `SYSSEL_START`: where the system descriptors start in the GDT.
pub const SYSSEL_START: usize = NGDT_MEM << 3;
/// `GDT_SIZE`.
pub const GDT_SIZE: usize = SYSSEL_START + (NGDT_SYS << 4);
/// `GDT_SYS_OFFSET`.
pub const GDT_SYS_OFFSET: usize = NGDT_MEM << 3;

/// `IDXSEL(s)`: the index of a selector, from the start of its table.
pub const fn idxsel(s: u16) -> u16 {
    (s >> 3) & 0x1fff
}

/// `GSEL(s, r)`: a global selector.
pub const fn gsel(s: u16, r: u16) -> u16 {
    (s << 3) | r
}

/// `GSYSSEL(s, r)`: a global system selector (TSS).
pub const fn gsyssel(s: u16, r: u16) -> u16 {
    ((s << 4) + SYSSEL_START as u16) | r
}

/// `LSEL(s, r)`: a local selector.
pub const fn lsel(s: u16, r: u16) -> u16 {
    s | r | SEL_LDT
}

/// `USERMODE(c, f)`: the trap came from user mode.
pub const fn usermode(c: u64) -> bool {
    ispl(c as u16) == SEL_UPL
}

/// `KERNELMODE(c, f)`: the trap came from kernel mode.
pub const fn kernelmode(c: u64) -> bool {
    ispl(c as u16) == SEL_KPL
}

/// `struct sys_segment_descriptor`: a TSS or LDT descriptor (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SysSegmentDescriptor {
    /// Limit, base (low 24 bits), type, DPL, present, limit (high), granularity, base bits
    /// 24..31.
    pub lo: u64,
    /// Base bits 32..63 and the reserved/zero fields.
    pub hi: u64,
}

impl SysSegmentDescriptor {
    /// A descriptor with every field zero (not present).
    pub const fn zeroed() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Packs the fields: `sd_lolimit`/`sd_hilimit` from `limit`, `sd_lobase`/`sd_hibase` from
    /// `base`, `sd_type`, `sd_dpl`, `sd_p` and `sd_gran`.
    pub const fn pack(base: u64, limit: u32, type_: u8, dpl: u8, p: bool, gran: bool) -> Self {
        let lo = (limit as u64 & 0xffff)
            | ((base & 0x00ff_ffff) << 16)
            | ((type_ as u64 & 0x1f) << 40)
            | ((dpl as u64 & 0x3) << 45)
            | ((p as u64) << 47)
            | (((limit >> 16) as u64 & 0xf) << 48)
            | ((gran as u64) << 55)
            | (((base >> 24) & 0xff) << 56);
        let hi = base >> 32;
        Self { lo, hi }
    }
}

/// `struct mem_segment_descriptor`: a code or data descriptor (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MemSegmentDescriptor(pub u64);

impl MemSegmentDescriptor {
    /// Packs the fields: `sd_lolimit`/`sd_hilimit` from `limit`, `sd_lobase`/`sd_hibase` from
    /// `base`, `sd_type`, `sd_dpl`, `sd_p`, `sd_avl`, `sd_long`, `sd_def32` and `sd_gran`.
    #[allow(clippy::too_many_arguments)] // the C struct's fields
    pub const fn pack(
        base: u64,
        limit: u32,
        type_: u8,
        dpl: u8,
        p: bool,
        avl: bool,
        long: bool,
        def32: bool,
        gran: bool,
    ) -> Self {
        Self(
            (limit as u64 & 0xffff)
                | ((base & 0x00ff_ffff) << 16)
                | ((type_ as u64 & 0x1f) << 40)
                | ((dpl as u64 & 0x3) << 45)
                | ((p as u64) << 47)
                | (((limit >> 16) as u64 & 0xf) << 48)
                | ((avl as u64) << 52)
                | ((long as u64) << 53)
                | ((def32 as u64) << 54)
                | ((gran as u64) << 55)
                | (((base >> 24) & 0xff) << 56),
        )
    }
}

/// `struct gate_descriptor`: an interrupt, trap or call gate (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GateDescriptor {
    /// Offset (low 16), selector, IST, type, DPL, present, offset bits 16..31.
    pub lo: u64,
    /// Offset bits 32..63 and the reserved/zero fields.
    pub hi: u64,
}

impl GateDescriptor {
    /// A gate with every field zero (not present): `unsetgate`.
    pub const fn zeroed() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Packs the fields: `gd_looffset`/`gd_hioffset` from `offset`, `gd_selector`, `gd_ist`,
    /// `gd_type`, `gd_dpl` and `gd_p`.
    pub const fn pack(offset: u64, selector: u16, ist: u8, type_: u8, dpl: u8, p: bool) -> Self {
        let lo = (offset & 0xffff)
            | ((selector as u64) << 16)
            | ((ist as u64 & 0x7) << 32)
            | ((type_ as u64 & 0x1f) << 40)
            | ((dpl as u64 & 0x3) << 45)
            | ((p as u64) << 47)
            | (((offset >> 16) & 0xffff) << 48);
        let hi = offset >> 32;
        Self { lo, hi }
    }

    /// `gd_p`.
    pub const fn present(&self) -> bool {
        (self.lo >> 47) & 1 != 0
    }
}

/// `struct region_descriptor`: used to load gdt/idt tables before segments yet exist.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RegionDescriptor {
    /// Segment extent.
    pub rd_limit: u16,
    /// Base address.
    pub rd_base: u64,
}

// system segments and gate types

/// `SDT_SYSNULL`: system null.
pub const SDT_SYSNULL: u8 = 0;
/// `SDT_SYS286TSS`: system 286 TSS available.
pub const SDT_SYS286TSS: u8 = 1;
/// `SDT_SYSLDT`: system local descriptor table.
pub const SDT_SYSLDT: u8 = 2;
/// `SDT_SYS286BSY`: system 286 TSS busy.
pub const SDT_SYS286BSY: u8 = 3;
/// `SDT_SYS286CGT`: system 286 call gate.
pub const SDT_SYS286CGT: u8 = 4;
/// `SDT_SYSTASKGT`: system task gate.
pub const SDT_SYSTASKGT: u8 = 5;
/// `SDT_SYS286IGT`: system 286 interrupt gate.
pub const SDT_SYS286IGT: u8 = 6;
/// `SDT_SYS286TGT`: system 286 trap gate.
pub const SDT_SYS286TGT: u8 = 7;
/// `SDT_SYSNULL2`: system null again.
pub const SDT_SYSNULL2: u8 = 8;
/// `SDT_SYS386TSS`: system 386 TSS available.
pub const SDT_SYS386TSS: u8 = 9;
/// `SDT_SYSNULL3`: system null again.
pub const SDT_SYSNULL3: u8 = 10;
/// `SDT_SYS386BSY`: system 386 TSS busy.
pub const SDT_SYS386BSY: u8 = 11;
/// `SDT_SYS386CGT`: system 386 call gate.
pub const SDT_SYS386CGT: u8 = 12;
/// `SDT_SYSNULL4`: system null again.
pub const SDT_SYSNULL4: u8 = 13;
/// `SDT_SYS386IGT`: system 386 interrupt gate.
pub const SDT_SYS386IGT: u8 = 14;
/// `SDT_SYS386TGT`: system 386 trap gate.
pub const SDT_SYS386TGT: u8 = 15;

// memory segment types

/// `SDT_MEMRO`: memory read only.
pub const SDT_MEMRO: u8 = 16;
/// `SDT_MEMROA`: memory read only accessed.
pub const SDT_MEMROA: u8 = 17;
/// `SDT_MEMRW`: memory read write.
pub const SDT_MEMRW: u8 = 18;
/// `SDT_MEMRWA`: memory read write accessed.
pub const SDT_MEMRWA: u8 = 19;
/// `SDT_MEMROD`: memory read only expand dwn limit.
pub const SDT_MEMROD: u8 = 20;
/// `SDT_MEMRODA`: memory read only expand dwn limit accessed.
pub const SDT_MEMRODA: u8 = 21;
/// `SDT_MEMRWD`: memory read write expand dwn limit.
pub const SDT_MEMRWD: u8 = 22;
/// `SDT_MEMRWDA`: memory read write expand dwn limit acessed.
pub const SDT_MEMRWDA: u8 = 23;
/// `SDT_MEME`: memory execute only.
pub const SDT_MEME: u8 = 24;
/// `SDT_MEMEA`: memory execute only accessed.
pub const SDT_MEMEA: u8 = 25;
/// `SDT_MEMER`: memory execute read.
pub const SDT_MEMER: u8 = 26;
/// `SDT_MEMERA`: memory execute read accessed.
pub const SDT_MEMERA: u8 = 27;
/// `SDT_MEMEC`: memory execute only conforming.
pub const SDT_MEMEC: u8 = 28;
/// `SDT_MEMEAC`: memory execute only accessed conforming.
pub const SDT_MEMEAC: u8 = 29;
/// `SDT_MEMERC`: memory execute read conforming.
pub const SDT_MEMERC: u8 = 30;
/// `SDT_MEMERAC`: memory execute read accessed conforming.
pub const SDT_MEMERAC: u8 = 31;

// Segment Protection Exception code bits

/// `SEGEX_EXT`: recursive or externally induced.
pub const SEGEX_EXT: u64 = 0x01;
/// `SEGEX_IDT`: interrupt descriptor table.
pub const SEGEX_IDT: u64 = 0x02;
/// `SEGEX_TI`: local descriptor table.
pub const SEGEX_TI: u64 = 0x04;

// Entries in the Interrupt Descriptor Table (IDT)

/// `NIDT`.
pub const NIDT: usize = 256;
/// `NRSVIDT`: reserved entries for cpu exceptions.
pub const NRSVIDT: usize = 32;

// Entries in the Global Descriptor Table (GDT). The code and data descriptors must come
// first. There are NGDT_MEM of them. Then comes the predefined TSS descriptor. There are
// NGDT_SYS of them. The particular order of the UDATA and UCODE descriptors is required by
// the sysretq instruction.

/// `GNULL_SEL`: Null descriptor.
pub const GNULL_SEL: u16 = 0;
/// `GCODE_SEL`: Kernel code descriptor.
pub const GCODE_SEL: u16 = 1;
/// `GDATA_SEL`: Kernel data descriptor.
pub const GDATA_SEL: u16 = 2;
/// `GUDATA_SEL`: User data descriptor.
pub const GUDATA_SEL: u16 = 3;
/// `GUCODE_SEL`: User code descriptor.
pub const GUCODE_SEL: u16 = 4;
/// `GPROC0_SEL`: common TSS.
pub const GPROC0_SEL: u16 = 0;

/// `GDT_ADDR_MEM(s, i)`: the word of memory descriptor `i` in a GDT of `u64` words.
pub const fn gdt_addr_mem(i: u16) -> usize {
    i as usize
}

/// `GDT_ADDR_SYS(s, i)`: the first word of system descriptor `i` in a GDT of `u64` words.
pub const fn gdt_addr_sys(i: u16) -> usize {
    SYSSEL_START / 8 + (i as usize) * 2
}

/// `VALID_USER_CSEL(s)`.
pub const fn valid_user_csel(s: u16) -> bool {
    s == gsel(GUCODE_SEL, SEL_UPL)
}

/// `VALID_USER_DSEL(s)`.
pub const fn valid_user_dsel(s: u16) -> bool {
    s == gsel(GUDATA_SEL, SEL_UPL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_and_layout() {
        assert_eq!(gsel(GCODE_SEL, SEL_KPL), 0x08);
        assert_eq!(gsel(GDATA_SEL, SEL_KPL), 0x10);
        assert_eq!(gsel(GUDATA_SEL, SEL_UPL), 0x1b);
        assert_eq!(gsel(GUCODE_SEL, SEL_UPL), 0x23);
        assert_eq!(gsyssel(GPROC0_SEL, SEL_KPL), 0x28);
        assert_eq!(GDT_SIZE, 56);
        assert!(kernelmode(0x08));
        assert!(usermode(0x23));
        assert_eq!(gdt_addr_sys(GPROC0_SEL), 5);
    }

    #[test]
    fn descriptor_packing() {
        // A 64-bit kernel code segment: base 0, limit 0xfffff, page granular, long.
        let cs =
            MemSegmentDescriptor::pack(0, 0xfffff, SDT_MEMERA, 0, true, false, true, false, true);
        assert_eq!(cs.0, 0x00af_9b00_0000_ffff);
        let gate = GateDescriptor::pack(0xffff_ffff_8010_2030, 0x08, 2, SDT_SYS386IGT, 0, true);
        assert_eq!(gate.lo & 0xffff, 0x2030);
        assert_eq!((gate.lo >> 16) & 0xffff, 0x08);
        assert_eq!((gate.lo >> 32) & 0x7, 2);
        assert_eq!((gate.lo >> 40) & 0x1f, SDT_SYS386IGT as u64);
        assert!(gate.present());
        assert_eq!((gate.lo >> 48) & 0xffff, 0x8010);
        assert_eq!(gate.hi, 0xffff_ffff);
        let tss =
            SysSegmentDescriptor::pack(0xffff_ffff_8000_1000, 103, SDT_SYS386TSS, 0, true, false);
        assert_eq!(tss.lo & 0xffff, 103);
        assert_eq!((tss.lo >> 16) & 0xff_ffff, 0x00_1000);
        assert_eq!((tss.lo >> 56) & 0xff, 0x80);
        assert_eq!(tss.hi, 0xffff_ffff);
        assert_eq!(size_of::<RegionDescriptor>(), 10);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/segments.h");
        let ours: &[(&str, i64)] = &[
            ("SEL_KPL", i64::from(SEL_KPL)),
            ("SEL_UPL", i64::from(SEL_UPL)),
            ("SEL_RPL", i64::from(SEL_RPL)),
            ("SEL_LDT", i64::from(SEL_LDT)),
            ("NGDT_MEM", NGDT_MEM as i64),
            ("NGDT_SYS", NGDT_SYS as i64),
            ("SDT_SYS386TSS", i64::from(SDT_SYS386TSS)),
            ("SDT_SYS386IGT", i64::from(SDT_SYS386IGT)),
            ("SDT_MEMRWA", i64::from(SDT_MEMRWA)),
            ("SDT_MEMERA", i64::from(SDT_MEMERA)),
            ("NIDT", NIDT as i64),
            ("NRSVIDT", NRSVIDT as i64),
            ("GCODE_SEL", i64::from(GCODE_SEL)),
            ("GDATA_SEL", i64::from(GDATA_SEL)),
            ("GUDATA_SEL", i64::from(GUDATA_SEL)),
            ("GUCODE_SEL", i64::from(GUCODE_SEL)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
