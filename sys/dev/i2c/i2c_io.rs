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
 * Copyright (c) 2003 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Jason R. Thorpe for Wasabi Systems, Inc.
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
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
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
//! `<dev/i2c/i2c_io.h>`: the I2C operations and addresses a client of the i2c framework names.
//!
//! Upstream: sys/dev/i2c/i2c_io.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `i2c_op_t` is the newtype [`I2cOp`] over the C enum's `int`, with the constants
//!   `I2C_OP_READ`, ... as `const`s of that type; the `I2C_OP_STOP_P`, `I2C_OP_WRITE_P`,
//!   `I2C_OP_READ_P` and `I2C_OP_BLKMODE_P` macros are its methods `stop_p`, `write_p`,
//!   `read_p` and `blkmode_p`.
//! - The `#ifdef notyet` part (`i2c_ioctl_exec_t`, `I2C_IOCTL_EXEC`) is not configured: nothing
//!   in the tree defines `notyet`, and no `/dev/iic` exists to take the ioctl.

/// `i2c_addr_t`: I2C bus address.
pub type I2cAddr = u16;

/// `I2C_OPMASK_STOP`: the operation ends with a STOP.
pub const I2C_OPMASK_STOP: i32 = 1;
/// `I2C_OPMASK_WRITE`: the operation is a write.
pub const I2C_OPMASK_WRITE: i32 = 2;
/// `I2C_OPMASK_BLKMODE`: the operation is a block transfer.
pub const I2C_OPMASK_BLKMODE: i32 = 4;

/// `i2c_op_t`: a high-level I2C operation, built from the `I2C_OPMASK_*` bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cOp(pub i32);

impl I2cOp {
    /// `I2C_OP_STOP_P(x)`.
    pub const fn stop_p(self) -> bool {
        self.0 & I2C_OPMASK_STOP != 0
    }

    /// `I2C_OP_WRITE_P(x)`.
    pub const fn write_p(self) -> bool {
        self.0 & I2C_OPMASK_WRITE != 0
    }

    /// `I2C_OP_READ_P(x)`.
    pub const fn read_p(self) -> bool {
        !self.write_p()
    }

    /// `I2C_OP_BLKMODE_P(x)`.
    pub const fn blkmode_p(self) -> bool {
        self.0 & I2C_OPMASK_BLKMODE != 0
    }
}

/// `I2C_OP_READ`.
pub const I2C_OP_READ: I2cOp = I2cOp(0);
/// `I2C_OP_READ_WITH_STOP`.
pub const I2C_OP_READ_WITH_STOP: I2cOp = I2cOp(I2C_OPMASK_STOP);
/// `I2C_OP_WRITE`.
pub const I2C_OP_WRITE: I2cOp = I2cOp(I2C_OPMASK_WRITE);
/// `I2C_OP_WRITE_WITH_STOP`.
pub const I2C_OP_WRITE_WITH_STOP: I2cOp = I2cOp(I2C_OPMASK_WRITE | I2C_OPMASK_STOP);
/// `I2C_OP_READ_BLOCK`.
pub const I2C_OP_READ_BLOCK: I2cOp = I2cOp(I2C_OPMASK_BLKMODE | I2C_OPMASK_STOP);
/// `I2C_OP_WRITE_BLOCK`.
pub const I2C_OP_WRITE_BLOCK: I2cOp =
    I2cOp(I2C_OPMASK_BLKMODE | I2C_OPMASK_WRITE | I2C_OPMASK_STOP);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_values_and_predicates() {
        assert_eq!(
            [
                I2C_OP_READ.0,
                I2C_OP_READ_WITH_STOP.0,
                I2C_OP_WRITE.0,
                I2C_OP_WRITE_WITH_STOP.0,
                I2C_OP_READ_BLOCK.0,
                I2C_OP_WRITE_BLOCK.0
            ],
            [0, 1, 2, 3, 5, 7]
        );
        assert!(I2C_OP_READ.read_p() && !I2C_OP_READ.stop_p() && !I2C_OP_READ.write_p());
        assert!(I2C_OP_READ_WITH_STOP.read_p() && I2C_OP_READ_WITH_STOP.stop_p());
        assert!(I2C_OP_WRITE.write_p() && !I2C_OP_WRITE.read_p() && !I2C_OP_WRITE.stop_p());
        assert!(I2C_OP_WRITE_WITH_STOP.write_p() && I2C_OP_WRITE_WITH_STOP.stop_p());
        assert!(I2C_OP_READ_BLOCK.blkmode_p() && I2C_OP_READ_BLOCK.read_p());
        assert!(I2C_OP_WRITE_BLOCK.blkmode_p() && I2C_OP_WRITE_BLOCK.write_p());
        assert!(!I2C_OP_WRITE_WITH_STOP.blkmode_p());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn masks_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/i2c/i2c_io.h");
        for (name, value) in [
            ("I2C_OPMASK_STOP", I2C_OPMASK_STOP),
            ("I2C_OPMASK_WRITE", I2C_OPMASK_WRITE),
            ("I2C_OPMASK_BLKMODE", I2C_OPMASK_BLKMODE),
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
