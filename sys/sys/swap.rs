/*	$OpenBSD: swap.h,v 1.7 2013/09/30 12:02:30 millert Exp $	*/
/*	$NetBSD: swap.h,v 1.2 1998/09/13 14:46:24 christos Exp $	*/
/* <LICENSES> */

/*
 * Copyright (c) 1995, 1996, 1998 Matthew R. Green, Tobias Weingartner
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
 * 3. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
/* </LICENSES> */

/* <CODE> */
//! `<sys/swap.h>`: the `swapctl(2)` commands and the `struct swapent` it fills.
//!
//! Upstream: sys/sys/swap.h @ 3ce1f3f79392
//!
//! `kern_pledge.c`'s `pledge_swapctl` reads the commands; `sys_swapctl` (`uvm_swap.c`) is
//! not ported yet.

use crate::sys::syslimits::PATH_MAX;
use crate::sys::types::Dev;

/// `NETDEV`: network device (for nfs swap).
pub const NETDEV: Dev = -2;

/// `SWAP_ON`: begin swapping on device.
pub const SWAP_ON: i32 = 1;
/// `SWAP_OFF`: (stop swapping on device).
pub const SWAP_OFF: i32 = 2;
/// `SWAP_NSWAP`: how many swap devices?
pub const SWAP_NSWAP: i32 = 3;
/// `SWAP_STATS`: get device info.
pub const SWAP_STATS: i32 = 4;
/// `SWAP_CTL`: change priority on device.
pub const SWAP_CTL: i32 = 5;
/// `SWAP_DUMPDEV`: use this device as dump device.
pub const SWAP_DUMPDEV: i32 = 7;

/// `SWF_INUSE`: in use: we have swapped here.
pub const SWF_INUSE: i32 = 0x0000_0001;
/// `SWF_ENABLE`: enabled: we can swap here.
pub const SWF_ENABLE: i32 = 0x0000_0002;
/// `SWF_BUSY`: busy: I/O happening here.
pub const SWF_BUSY: i32 = 0x0000_0004;
/// `SWF_FAKE`: fake: still being built.
pub const SWF_FAKE: i32 = 0x0000_0008;

/// `struct swapent`: swap information returned to userland (`SWAP_STATS`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Swapent {
    /// `se_dev`: device id.
    pub se_dev: Dev,
    /// `se_flags`: flags.
    pub se_flags: i32,
    /// `se_nblks`: total blocks.
    pub se_nblks: i32,
    /// `se_inuse`: blocks in use.
    pub se_inuse: i32,
    /// `se_priority`: priority of this device.
    pub se_priority: i32,
    /// `se_path`: path name.
    pub se_path: [u8; PATH_MAX],
}

const _: () = assert!(size_of::<Swapent>() == 20 + PATH_MAX);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/swap.h");
        for (name, value) in [
            ("SWAP_ON", SWAP_ON),
            ("SWAP_OFF", SWAP_OFF),
            ("SWAP_NSWAP", SWAP_NSWAP),
            ("SWAP_STATS", SWAP_STATS),
            ("SWAP_CTL", SWAP_CTL),
            ("SWAP_DUMPDEV", SWAP_DUMPDEV),
            ("SWF_INUSE", SWF_INUSE),
            ("SWF_ENABLE", SWF_ENABLE),
            ("SWF_BUSY", SWF_BUSY),
            ("SWF_FAKE", SWF_FAKE),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
