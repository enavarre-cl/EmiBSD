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
//! `<machine/disklabel.h>` and the machine's `disksubr.c` as a trait.
//!
//! Each architecture defines where its disk label sits (`LABELSECTOR`, `LABELOFFSET`) and how
//! many partitions it describes (`MAXPARTITIONS`) in `arch/<arch>/include/disklabel.rs`, and
//! reads and writes the label in `arch/<arch>/<arch>/disksubr.rs` (`readdisklabel`,
//! `writedisklabel`, which `<sys/disklabel.h>` declares). `sys::disklabel` re-exports the
//! constants; generic code calls the functions below.

use crate::machine::Machine;
use crate::sys::conf::DevTypeStrategy;
use crate::sys::disklabel::Disklabel;
use crate::sys::errno::Errno;
use crate::sys::types::Dev;

/// The machine's disk label location and its label I/O.
pub trait MachineDisklabel {
    /// `LABELSECTOR`: sector containing label.
    const LABELSECTOR: u64;
    /// `LABELOFFSET`: offset of label in sector.
    const LABELOFFSET: usize;
    /// `MAXPARTITIONS`: number of partitions.
    const MAXPARTITIONS: usize;

    /// `readdisklabel(dev, strat, lp, spoofonly)`: attempts to read a disk label from a
    /// device using the indicated strategy routine. The label must be partly set up before
    /// this: secpercyl, secsize and anything required for a block i/o read operation in the
    /// driver's strategy/start routines must be filled in before calling us.
    fn readdisklabel(
        dev: Dev,
        strat: DevTypeStrategy,
        lp: &mut Disklabel,
        spoofonly: bool,
    ) -> Result<(), Errno>;

    /// `writedisklabel(dev, strat, lp)`: writes the disk label back to the device after
    /// modification.
    fn writedisklabel(dev: Dev, strat: DevTypeStrategy, lp: &mut Disklabel) -> Result<(), Errno>;
}

/// `readdisklabel` on the selected machine.
pub fn readdisklabel(
    dev: Dev,
    strat: DevTypeStrategy,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    Machine::readdisklabel(dev, strat, lp, spoofonly)
}

/// `writedisklabel` on the selected machine.
pub fn writedisklabel(dev: Dev, strat: DevTypeStrategy, lp: &mut Disklabel) -> Result<(), Errno> {
    Machine::writedisklabel(dev, strat, lp)
}
/* </CODE> */
