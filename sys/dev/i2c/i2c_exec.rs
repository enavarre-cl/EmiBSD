/* <LICENSES> */
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
//! The simplified i2c client interface engine: `iic_exec` and the SMBus operations over it.
//!
//! Upstream: sys/dev/i2c/i2c_exec.c @ 3ce1f3f79392
//!
//! `iic_exec` and the SMBus routines are the preferred interface for client access to
//! I2C/SMBus, since many automated controllers do not provide access to the low-level
//! primitives of the I2C bus protocol. A controller with an `ic_exec` hook gets the whole
//! operation; otherwise the engine drives the controller's byte-level hooks.
//!
//! ## Deviations
//! - `(vcmd, cmdlen)` and `(vbuf, buflen)` are the slices `cmd` and `buf`; a read stores into
//!   `buf`, a write sends it (`buf` is `&mut` for both, as the C's `void *`).
//! - The `bad:` path of the byte-level engine ignores `iic_send_stop`'s result, as the C does.

use crate::dev::i2c::i2c_io::{I2C_OP_READ_WITH_STOP, I2C_OP_WRITE_WITH_STOP, I2cAddr, I2cOp};
use crate::dev::i2c::i2cvar::{
    I2C_F_LAST, I2C_F_READ, I2C_F_STOP, I2cTag, iic_initiate_xfer, iic_read_byte, iic_send_stop,
    iic_write_byte,
};

/// `iic_exec`: simplified I2C client interface engine.
///
/// Defers to the controller if it provides an exec function. Returns 0 on success, nonzero
/// (the controller's own value) on failure.
pub fn iic_exec(
    tag: I2cTag,
    op: I2cOp,
    addr: I2cAddr,
    cmd: &[u8],
    buf: &mut [u8],
    mut flags: i32,
) -> i32 {
    // Defer to the controller if it provides an exec function. Use it if it does.
    if let Some(exec) = tag.ic_exec.get() {
        return exec(tag.ic_cookie.get(), op, addr, cmd, buf, flags);
    }

    let error = 'xfer: {
        if !cmd.is_empty() {
            let error = iic_initiate_xfer(tag, addr, flags);
            if error != 0 {
                break 'xfer error;
            }
            for &b in cmd {
                let error = iic_write_byte(tag, b, flags);
                if error != 0 {
                    break 'xfer error;
                }
            }
        }

        if op.read_p() {
            flags |= I2C_F_READ;
        }

        let buflen = buf.len();
        for (i, byte) in buf.iter_mut().enumerate() {
            // The C counts `len` down from `buflen - 1` as it goes.
            let len = buflen - 1 - i;
            if len == 0 && op.stop_p() {
                flags |= I2C_F_STOP;
            }
            if op.read_p() {
                // Send REPEATED START.
                if len + 1 == buflen {
                    let error = iic_initiate_xfer(tag, addr, flags);
                    if error != 0 {
                        break 'xfer error;
                    }
                }
                // NACK on last byte.
                if len == 0 {
                    flags |= I2C_F_LAST;
                }
                let error = iic_read_byte(tag, byte, flags);
                if error != 0 {
                    break 'xfer error;
                }
            } else {
                // Maybe send START.
                if len + 1 == buflen && cmd.is_empty() {
                    let error = iic_initiate_xfer(tag, addr, flags);
                    if error != 0 {
                        break 'xfer error;
                    }
                }
                let error = iic_write_byte(tag, *byte, flags);
                if error != 0 {
                    break 'xfer error;
                }
            }
        }

        return 0;
    };

    // bad:
    let _ = iic_send_stop(tag, flags);
    error
}

/// `iic_smbus_write_byte`: performs an SMBus "write byte" operation.
pub fn iic_smbus_write_byte(tag: I2cTag, addr: I2cAddr, cmd: u8, val: u8, flags: i32) -> i32 {
    let mut val = val;
    iic_exec(
        tag,
        I2C_OP_WRITE_WITH_STOP,
        addr,
        &[cmd],
        core::slice::from_mut(&mut val),
        flags,
    )
}

/// `iic_smbus_read_byte`: performs an SMBus "read byte" operation.
pub fn iic_smbus_read_byte(tag: I2cTag, addr: I2cAddr, cmd: u8, valp: &mut u8, flags: i32) -> i32 {
    iic_exec(
        tag,
        I2C_OP_READ_WITH_STOP,
        addr,
        &[cmd],
        core::slice::from_mut(valp),
        flags,
    )
}

/// `iic_smbus_receive_byte`: performs an SMBus "receive byte" operation.
pub fn iic_smbus_receive_byte(tag: I2cTag, addr: I2cAddr, valp: &mut u8, flags: i32) -> i32 {
    iic_exec(
        tag,
        I2C_OP_READ_WITH_STOP,
        addr,
        &[],
        core::slice::from_mut(valp),
        flags,
    )
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::i2c::i2c_io::{I2C_OP_READ, I2C_OP_WRITE};
    use crate::dev::i2c::i2cvar::I2cController;
    use core::ffi::c_void;
    use std::boxed::Box;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// A byte-level controller that logs every call and serves reads from `data`.
    struct Log {
        calls: Vec<String>,
        data: Vec<u8>,
        fail_write_at: Option<usize>,
        writes: usize,
    }

    fn log(cookie: *mut c_void) -> &'static mut Log {
        // SAFETY: the tests pass a leaked `Log` as the cookie and run single-threaded per test.
        unsafe { &mut *cookie.cast::<Log>() }
    }

    fn start(cookie: *mut c_void, addr: I2cAddr, flags: i32) -> i32 {
        log(cookie)
            .calls
            .push(format!("start {addr:#x} {flags:#x}"));
        0
    }

    fn stop(cookie: *mut c_void, flags: i32) -> i32 {
        log(cookie).calls.push(format!("stop {flags:#x}"));
        0
    }

    fn write(cookie: *mut c_void, b: u8, flags: i32) -> i32 {
        let l = log(cookie);
        l.calls.push(format!("write {b:#x} {flags:#x}"));
        l.writes += 1;
        if l.fail_write_at == Some(l.writes) {
            5
        } else {
            0
        }
    }

    fn read(cookie: *mut c_void, b: &mut u8, flags: i32) -> i32 {
        let l = log(cookie);
        l.calls.push(format!("read {flags:#x}"));
        *b = l.data.remove(0);
        0
    }

    fn bytewise(data: &[u8], fail_write_at: Option<usize>) -> (I2cTag, &'static mut Log) {
        let l: &'static mut Log = Box::leak(Box::new(Log {
            calls: Vec::new(),
            data: data.to_vec(),
            fail_write_at,
            writes: 0,
        }));
        let ic: &'static I2cController = Box::leak(Box::new(I2cController::new()));
        ic.ic_cookie.set(core::ptr::from_mut(l).cast());
        ic.ic_initiate_xfer.set(Some(start));
        ic.ic_send_stop.set(Some(stop));
        ic.ic_write_byte.set(Some(write));
        ic.ic_read_byte.set(Some(read));
        // SAFETY: the log outlives the test; the tests read it after the engine returned.
        (ic, unsafe { &mut *core::ptr::from_mut(l) })
    }

    #[test]
    fn smbus_read_byte_writes_the_command_then_repeats_the_start() {
        let (ic, l) = bytewise(&[0xab], None);
        let mut v = 0;
        assert_eq!(iic_smbus_read_byte(ic, 0x50, 0x02, &mut v, 0), 0);
        assert_eq!(v, 0xab);
        assert_eq!(
            l.calls,
            [
                "start 0x50 0x0",
                "write 0x2 0x0",
                "start 0x50 0x5",
                "read 0x7"
            ]
        );
    }

    #[test]
    fn smbus_write_byte_is_command_then_data_with_stop() {
        let (ic, l) = bytewise(&[], None);
        assert_eq!(iic_smbus_write_byte(ic, 0x2c, 0x10, 0x77, 0), 0);
        assert_eq!(
            l.calls,
            ["start 0x2c 0x0", "write 0x10 0x0", "write 0x77 0x4"]
        );
    }

    #[test]
    fn receive_byte_has_no_command_phase() {
        let (ic, l) = bytewise(&[0x5a], None);
        let mut v = 0;
        assert_eq!(iic_smbus_receive_byte(ic, 0x50, &mut v, 0), 0);
        assert_eq!(v, 0x5a);
        assert_eq!(l.calls, ["start 0x50 0x5", "read 0x7"]);
    }

    #[test]
    fn a_multibyte_read_nacks_only_the_last_byte() {
        let (ic, l) = bytewise(&[1, 2, 3], None);
        let mut buf = [0u8; 3];
        assert_eq!(iic_exec(ic, I2C_OP_READ, 0x18, &[], &mut buf, 0), 0);
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(
            l.calls,
            ["start 0x18 0x1", "read 0x1", "read 0x1", "read 0x3"]
        );
    }

    #[test]
    fn a_failed_byte_sends_a_stop_and_returns_the_error() {
        let (ic, l) = bytewise(&[], Some(2));
        let mut buf = [9u8];
        assert_eq!(iic_exec(ic, I2C_OP_WRITE, 0x50, &[1], &mut buf, 0), 5);
        assert_eq!(
            l.calls,
            [
                "start 0x50 0x0",
                "write 0x1 0x0",
                "write 0x9 0x0",
                "stop 0x0"
            ]
        );
    }

    #[test]
    fn a_controller_exec_hook_gets_the_whole_operation() {
        fn exec(
            _cookie: *mut c_void,
            op: I2cOp,
            addr: I2cAddr,
            cmd: &[u8],
            buf: &mut [u8],
            flags: i32,
        ) -> i32 {
            buf[0] = cmd[0] ^ addr as u8;
            op.0 + flags
        }
        let ic: &'static I2cController = Box::leak(Box::new(I2cController::new()));
        ic.ic_exec.set(Some(exec));
        let mut v = 0;
        assert_eq!(iic_smbus_read_byte(ic, 0x50, 0x02, &mut v, 8), 9);
        assert_eq!(v, 0x52);
    }
}
/* </TESTS> */
