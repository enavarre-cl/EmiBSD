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

/*
 * Copyright (c) 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/isa/pcppivar.h>`: what pcppi(4) offers its children (spkr(4)): the bell.
//!
//! Upstream: sys/dev/isa/pcppivar.h @ 3ce1f3f79392
//!
//! `pcppi_bell` lives in `dev/isa/pcppi.rs`, where the C defines it.

use core::ffi::c_void;

/// `pcppi_tag_t`: the pcppi softc, opaque to the children.
pub type PcppiTag = *mut c_void;

/// `struct pcppi_attach_args`.
#[derive(Clone, Copy, Debug)]
pub struct PcppiAttachArgs {
    /// `pa_cookie`.
    pub pa_cookie: PcppiTag,
}

/// `PCPPI_BELL_SLEEP`: synchronous; sleep for complete.
pub const PCPPI_BELL_SLEEP: i32 = 0x01;
/// `PCPPI_BELL_POLL`: synchronous; poll for complete.
pub const PCPPI_BELL_POLL: i32 = 0x02;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/pcppivar.h");
        let ours = crate::reftest::assert_defines!(defs; PCPPI_BELL_SLEEP, PCPPI_BELL_POLL);
        crate::reftest::assert_complete(&defs, "PCPPI_", &ours);
    }
}
/* </TESTS> */
