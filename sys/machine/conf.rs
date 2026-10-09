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
//! What the device switch needs from the machine: each architecture's `conf.c`.
//!
//! OpenBSD's `<sys/conf.h>` declares `bdevsw[]`, `cdevsw[]`, `nblkdev`, `nchrdev`,
//! `chrtoblktbl[]` and the helpers `iskmemdev`, `iszerodev` and `getnulldev`, and every
//! architecture defines them in `arch/<arch>/<arch>/conf.c`, with major numbers that are ABI
//! (the `/dev` nodes `MAKEDEV(8)` creates carry them). Here each architecture writes its
//! tables in `sys/arch/<arch>/<arch>/conf.rs`, in the C's slot order, with an entry for every
//! driver that exists and `cdev_notdef()`/`bdev_notdef()` for the rest; generic code reaches
//! them only through this module (`docs/ARCHITECTURE.md`, "The device switch").
//!
//! The entries are copied out (`Cdevsw` is a handful of function pointers), so a caller
//! never holds a reference into a table that a console driver may rewrite at boot
//! (`pluartcnattach`'s `cdevsw[maj] = pluartdev`, [`cdevsw_set`]).

use core::cell::Cell;

use crate::machine::Machine;
use crate::sys::conf::{Bdevsw, Cdevsw, bdev_notdef, cdev_notdef};
use crate::sys::types::Dev;

/// A device switch table: one `Cell` per major, read by copy.
pub struct Devsw<T: Copy, const N: usize>(pub [Cell<T>; N]);

// SAFETY: the tables are written only by `cdevsw_set`, on the boot CPU during console
// attachment, before any other CPU or interrupt handler runs; afterwards they are only read.
unsafe impl<T: Copy, const N: usize> Sync for Devsw<T, N> {}

impl<T: Copy, const N: usize> Devsw<T, N> {
    /// `nitems(table)`.
    pub const fn len(&self) -> usize {
        N
    }

    /// Whether the table has no entries.
    pub const fn is_empty(&self) -> bool {
        N == 0
    }

    /// `table[maj]`, `None` past the end.
    pub fn get(&self, maj: u32) -> Option<T> {
        self.0.get(maj as usize).map(Cell::get)
    }

    /// `table[maj] = sw`; nothing past the end.
    pub fn set(&self, maj: u32, sw: T) {
        if let Some(slot) = self.0.get(maj as usize) {
            slot.set(sw);
        }
    }
}

/// The machine's device switch tables (its `conf.c`).
pub trait Conf {
    /// `nchrdev`: the number of `cdevsw[]` entries.
    fn nchrdev() -> u32;

    /// `cdevsw[maj]`, `None` past the end.
    fn cdevsw(maj: u32) -> Option<Cdevsw>;

    /// `cdevsw[maj] = sw`: a console driver taking over another driver's slot at boot.
    fn cdevsw_set(maj: u32, sw: Cdevsw);

    /// `nblkdev`: the number of `bdevsw[]` entries.
    fn nblkdev() -> u32;

    /// `bdevsw[maj]`, `None` past the end.
    fn bdevsw(maj: u32) -> Option<Bdevsw>;

    /// `chrtoblktbl[]` (`nchrtoblktbl` is its length): the block major of each character
    /// major, `NODEV` where there is none.
    fn chrtoblktbl() -> &'static [Dev];

    /// `swapdev`: the fake device of `sw.c`, used only internally to get to `swstrategy`.
    fn swapdev() -> Dev;

    /// `mem_no`: major device number of memory special file.
    fn mem_no() -> u32;

    /// `iskmemdev(dev)`: whether `dev` is `/dev/mem` or `/dev/kmem`.
    fn iskmemdev(dev: Dev) -> bool;

    /// `iszerodev(dev)`: whether `dev` is `/dev/zero`.
    fn iszerodev(dev: Dev) -> bool;

    /// `getnulldev()`: the device number of `/dev/null`.
    fn getnulldev() -> Dev;
}

/// `nchrdev` on the selected machine.
pub fn nchrdev() -> u32 {
    Machine::nchrdev()
}

/// `cdevsw[maj]` on the selected machine. A major past the table reads as an empty slot
/// (`cdev_notdef()`): the C indexes the array without a check there, which callers rule out
/// beforehand.
pub fn cdevsw(maj: u32) -> Cdevsw {
    Machine::cdevsw(maj).unwrap_or(cdev_notdef())
}

/// `cdevsw[maj] = sw` on the selected machine: only console attachment does it, on the
/// boot CPU, before the slot is used (`pluartcnattach`'s KLUDGE).
pub fn cdevsw_set(maj: u32, sw: Cdevsw) {
    Machine::cdevsw_set(maj, sw)
}

/// `nblkdev` on the selected machine.
pub fn nblkdev() -> u32 {
    Machine::nblkdev()
}

/// `bdevsw[maj]` on the selected machine; past the table, an empty slot (see [`cdevsw`]).
pub fn bdevsw(maj: u32) -> Bdevsw {
    Machine::bdevsw(maj).unwrap_or(bdev_notdef())
}

/// `chrtoblktbl[]` on the selected machine.
pub fn chrtoblktbl() -> &'static [Dev] {
    Machine::chrtoblktbl()
}

/// `swapdev` on the selected machine.
pub fn swapdev() -> Dev {
    Machine::swapdev()
}

/// `mem_no` on the selected machine.
pub fn mem_no() -> u32 {
    Machine::mem_no()
}

/// `iskmemdev` on the selected machine.
pub fn iskmemdev(dev: Dev) -> bool {
    Machine::iskmemdev(dev)
}

/// `iszerodev` on the selected machine.
pub fn iszerodev(dev: Dev) -> bool {
    Machine::iszerodev(dev)
}

/// `getnulldev` on the selected machine.
pub fn getnulldev() -> Dev {
    Machine::getnulldev()
}
/* </CODE> */
