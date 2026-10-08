/*	$OpenBSD: pic.h,v 1.8 2024/01/19 18:38:16 kettenis Exp $	*/
/*	$NetBSD: pic.h,v 1.1 2003/02/26 21:26:11 fvdl Exp $	*/

/* <CODE> */
//! amd64 `<machine/pic.h>`: structure common to all PIC softcs.
//!
//! Upstream: sys/arch/amd64/include/pic.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct pic` and the `PIC_*` types; `i8259_pic` lives in
//! `amd64/i8259.rs` and `softintr_pic` in `amd64/intr.rs`; `local_pic` (the LAPIC) and
//! `msi_pic` come with their drivers.
//!
//! ## Deviations
//! - `pic_dev` (`struct device`, autoconfiguration) is reduced to its name, `pic_name`.
//! - `pic_mutex` is `MULTIPROCESSOR` only.
//! - The stub tables live in assembly (`vector.S`), so they are reached through a function
//!   returning the slice instead of a pointer in the struct.

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::intr::Intrstub;

/// `pic_addroute`/`pic_delroute`: `(pic, ci, pin, idtvec, type)`.
pub type PicRouteFn = fn(&Pic, &CpuInfo, i32, i32, i32);
/// `pic_allocidtvec`: `(pic, pin, low, high)`, 0 when none is free.
pub type PicAllocIdtvecFn = fn(&Pic, i32, i32, i32) -> i32;

/// `struct pic`.
pub struct Pic {
    /// `pic_dev.dv_xname`.
    pub pic_name: &'static str,
    /// `pic_type`: `PIC_*`.
    pub pic_type: i32,
    /// `pic_hwmask(pic, pin)`.
    pub pic_hwmask: Option<fn(&Pic, i32)>,
    /// `pic_hwunmask(pic, pin)`.
    pub pic_hwunmask: Option<fn(&Pic, i32)>,
    /// `pic_addroute(pic, ci, pin, idtvec, type)`.
    pub pic_addroute: Option<PicRouteFn>,
    /// `pic_delroute(pic, ci, pin, idtvec, type)`.
    pub pic_delroute: Option<PicRouteFn>,
    /// `pic_allocidtvec(pic, pin, low, high)`.
    pub pic_allocidtvec: Option<PicAllocIdtvecFn>,
    /// `pic_level_stubs`: the stubs for level-triggered pins.
    pub pic_level_stubs: Option<fn() -> &'static [Intrstub]>,
    /// `pic_edge_stubs`: the stubs for edge-triggered pins.
    pub pic_edge_stubs: Option<fn() -> &'static [Intrstub]>,
}

/// `PIC_I8259`.
pub const PIC_I8259: i32 = 0;
/// `PIC_IOAPIC`.
pub const PIC_IOAPIC: i32 = 1;
/// `PIC_LAPIC`.
pub const PIC_LAPIC: i32 = 2;
/// `PIC_MSI`.
pub const PIC_MSI: i32 = 3;
/// `PIC_SOFT`.
pub const PIC_SOFT: i32 = 4;
/* </CODE> */
