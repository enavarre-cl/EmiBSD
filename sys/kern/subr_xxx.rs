/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)subr_xxx.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Miscellaneous trivial functions, including many that are often inline-expanded or done
//! in assembler: `kern/subr_xxx.c`.
//!
//! Upstream: sys/kern/subr_xxx.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports `enodev`, `enxio`, `eopnotsupp`, `nullop`
//! and `assertwaitok`; the device switch (M8) brings `bdevsw_lookup`, `chrtoblk` and
//! `blktochr`.
//!
//! ## Deviations
//! - The error stubs return `Result<(), Errno>` like every other error path.
//! - `bdevsw_lookup` returns a copy of the entry (`machine::conf`), the C a pointer into the
//!   table.

use core::sync::atomic::Ordering;

use crate::kern::init_main::DB_ACTIVE;
use crate::kern::subr_prf::panicstr;
use crate::machine::Machine;
use crate::machine::conf::{bdevsw, chrtoblktbl, nblkdev, nchrdev};
use crate::machine::cpu::Cpu;
use crate::machine::intr::{IPL_NONE, splassert};
use crate::sys::conf::Bdevsw;
use crate::sys::errno::Errno;
use crate::sys::param::NODEV;
use crate::sys::smr::smr_assert_noncritical;
use crate::sys::types::{Dev, major, makedev, minor};

/// `enodev`: unsupported device function (e.g. writing to read-only device).
pub fn enodev() -> Result<(), Errno> {
    Err(Errno::ENODEV)
}

/// `enxio`: unconfigured device function; driver not configured.
pub fn enxio() -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `eopnotsupp`: return error for operation not supported on a specific object or file type.
pub fn eopnotsupp() -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `nullop`: generic null operation, always returns success.
pub fn nullop() -> Result<(), Errno> {
    Ok(())
}

/// `bdevsw_lookup`: the block device switch entry of `dev`.
pub fn bdevsw_lookup(dev: Dev) -> Bdevsw {
    bdevsw(major(dev))
}

/// `chrtoblk`: convert a character device number to a block device number.
pub fn chrtoblk(dev: Dev) -> Dev {
    let tbl = chrtoblktbl();
    if major(dev) >= nchrdev() || major(dev) as usize >= tbl.len() {
        return NODEV;
    }
    let blkmaj = tbl[major(dev) as usize];
    if blkmaj == NODEV {
        return NODEV;
    }
    makedev(blkmaj as u32, minor(dev))
}

/// `blktochr`: convert a block device number to a character device number.
pub fn blktochr(dev: Dev) -> Dev {
    let blkmaj = major(dev);

    if blkmaj >= nblkdev() {
        return NODEV;
    }
    for (i, &b) in chrtoblktbl().iter().enumerate() {
        if blkmaj as Dev == b {
            return makedev(i as u32, minor(dev));
        }
    }
    NODEV
}

/// `assertwaitok`: check that we're in a context where it's okay to sleep.
pub fn assertwaitok() {
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    splassert(IPL_NONE, "assertwaitok");
    smr_assert_noncritical(); // SMR_ASSERT_NONCRITICAL()
    #[cfg(feature = "diagnostic")]
    if Machine::curcpu_mutex_level() != 0 {
        crate::kern::subr_prf::panic(format_args!(
            "assertwaitok: non-zero mutex count: {}",
            Machine::curcpu_mutex_level()
        ));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = Machine::curcpu_mutex_level;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::conf::{cdevsw, getnulldev, iskmemdev, iszerodev};
    use crate::sys::conf::{D_CLONE, D_TTY};

    #[test]
    fn the_device_switch_answers_by_major() {
        // The host's switch: the console and the ptys are ttys, slot 22 is filedesc.
        assert_eq!(cdevsw(0).d_type, D_TTY);
        assert_eq!(cdevsw(6).d_type, D_TTY);
        assert_eq!(cdevsw(22).d_flags & D_CLONE, 0);
        // Past the table: an empty slot.
        assert_eq!(cdevsw(10_000).d_type, 0);
        assert_eq!(bdevsw_lookup(makedev(10_000, 0)).d_type, 0);
        // No block devices: nothing converts.
        assert_eq!(chrtoblk(makedev(13, 1)), NODEV);
        assert_eq!(blktochr(makedev(4, 1)), NODEV);
        assert!(iskmemdev(makedev(2, 1)));
        assert!(!iskmemdev(makedev(2, 2)));
        assert!(iszerodev(makedev(2, 12)));
        assert_eq!(getnulldev(), makedev(2, 2));
        assert_eq!(enxio(), Err(Errno::ENXIO));
    }
}
/* </TESTS> */
