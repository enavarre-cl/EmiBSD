/*	$OpenBSD: tftp.c,v 1.7 2021/10/25 15:59:46 patrick Exp $	*/
/*	$NetBSD: tftp.c,v 1.15 2003/08/18 15:45:29 dsl Exp $	 */
/*	$OpenBSD: tftp.h,v 1.4 2014/11/19 19:59:02 miod Exp $	*/
/*	$NetBSD: tftp.h,v 1.3 2003/08/07 16:32:30 agc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996
 *	Matthias Drochner.  All rights reserved.
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
 * Copyright (c) 1996
 *	Matthias Drochner.  All rights reserved.
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
 *
 */

/*	NetBSD: tftp.h,v 1.6 2000/10/18 01:35:46 dogcow Exp 	*/

/*
 * Copyright (c) 1983, 1993
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
 *	@(#)tftp.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! `tftp.h` and `tftp.c`: a simple TFTP client as a libsa file system: `tftp_open` asks the
//! server for a file and `tftp_read` acknowledges block after block, re-requesting the file
//! to seek backwards.
//!
//! Upstream: sys/lib/libsa/tftp.c @ 3ce1f3f79392, sys/lib/libsa/tftp.h @ 3ce1f3f79392
//!
//! Assumes:
//!  - the socket at `open_file->f_devdata` (here a `usize` the device's open points it at)
//!  - server host IP in global [`SERVIP`]
//!
//! Restrictions:
//!  - read only
//!  - lseek only with `SEEK_SET` or `SEEK_CUR`
//!  - no big time differences between transfers (<tftp timeout)
//!
//! ## Deviations
//! - `extern struct in_addr servip`, which the program defines (efiboot's `efipxe.c`), is
//!   libsa's [`SERVIP`] (`s_addr`, network order): libsa cannot name the program's
//!   statics. The program stores the server's address there.
//! - The socket `f_devdata` points at is a `usize` (the C's `int`), the descriptor
//!   `netif_open` returns; a NULL `f_devdata` is `ENXIO` where the C dereferences it.
//! - `struct tftp_handle` keeps the socket number, not the `struct iodesc *`, and takes the
//!   descriptor with `socktodesc` at each entry point. It owns a copy of the path, where the
//!   C keeps the caller's pointer ("we hope it's static"); a path the request cannot hold
//!   (more than 131 bytes) is `ENOENT` where the C overflows its buffer. `islastblock` is a
//!   `bool`, `validsize` a `usize`; `off` stays the C's `int`, which `tftp_seek` truncates
//!   to.
//! - `struct tftphdr` ([`Tftphdr`]) sizes the buffers; its members are read and written in
//!   the packet bytes at their offsets (`th_block` and `th_code` share `th_u`).
//! - `static int tftpport` is never written: the constant [`TFTPPORT`].
//! - The handle is a `Box` that `f_fsdata` owns (`alloc()` cannot fail here: no `ENOMEM`).
//! - `TFTP_NOTERMINATE`, `LIBSA_NO_TWIDDLE` and `NO_READDIR` are not defined (as for
//!   efiboot): `tftp_terminate`, the twiddle and `tftp_readdir` are compiled. The `DEBUG`
//!   messages are not ported: no efiboot Makefile defines `DEBUG`.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::dev::{errno, set_errno};
use crate::hdr::endian::{htons, ntohs};
use crate::hdr::stat::Stat;
use crate::hdr::types::{Off, Time};
use crate::hdr::udp::Udphdr;
use crate::iodesc::IoDesc;
use crate::net::{FNAME_SIZE, PACKET_HEADER, fail, getsecs, sendrecv};
use crate::netif::socktodesc;
use crate::netudp::{readudp, sendudp};
use crate::printf::twiddle;
use crate::saerrno::Errno;
use crate::stand::{OpenFile, SEEK_CUR, SEEK_SET};

/// `SEGSIZE`: data segment size.
pub const SEGSIZE: usize = 512;

/// `RRQ`: read request.
pub const RRQ: u16 = 0o1;
/// `WRQ`: write request.
pub const WRQ: u16 = 0o2;
/// `DATA`: data packet.
pub const DATA: u16 = 0o3;
/// `ACK`: acknowledgement.
pub const ACK: u16 = 0o4;
/// `ERROR`: error code.
pub const ERROR: u16 = 0o5;

/// `EUNDEF`: not defined.
pub const EUNDEF: u16 = 0;
/// `ENOTFOUND`: file not found.
pub const ENOTFOUND: u16 = 1;
/// `EACCESS`: access violation.
pub const EACCESS: u16 = 2;
/// `ENOSPACE`: disk full or allocation exceeded.
pub const ENOSPACE: u16 = 3;
/// `EBADOP`: illegal TFTP operation.
pub const EBADOP: u16 = 4;
/// `EBADID`: unknown transfer ID.
pub const EBADID: u16 = 5;
/// `EEXISTS`: file already exists.
pub const EEXISTS: u16 = 6;
/// `ENOUSER`: no such user.
pub const ENOUSER: u16 = 7;

/// `IPPORT_TFTP`: the TFTP server's port.
pub const IPPORT_TFTP: u16 = 69;

/// `tftpport`: the base of our local port.
pub const TFTPPORT: i32 = 2000;

/// `RSPACE`: max data packet, rounded up.
const RSPACE: usize = 520;

/// `struct tftphdr`: a TFTP packet's header (`th_data[1]` because space needed for NUL).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Tftphdr {
    /// `th_opcode`: packet type, network order.
    pub th_opcode: i16,
    /// `th_u`: `th_block` (block #), `th_code` (error code) or the first byte of
    /// `th_stuff` (request packet stuff), network order.
    pub th_u: u16,
    /// `th_data` (`th_msg`): data or error string.
    pub th_data: [u8; 1],
}

/// The offset of `th_block`/`th_code`/`th_stuff` in a TFTP packet.
const TH_U: usize = offset_of!(Tftphdr, th_u);
/// The offset of `th_data` in a TFTP packet (`t->th_data - (char *)t`).
const TH_DATA: usize = offset_of!(Tftphdr, th_data);

/// `sizeof(lastdata)`: `struct packet_header`, `struct tftphdr` and `RSPACE`.
const LASTDATA: usize = PACKET_HEADER + size_of::<Tftphdr>() + RSPACE;

/// `struct tftp_handle`: an open TFTP file.
struct TftpHandle {
    /// `iodesc`: the socket.
    iodesc: usize,
    /// `currblock`: contents of lastdata.
    currblock: i32,
    /// `islastblock`: flag.
    islastblock: bool,
    /// `validsize`: the data bytes in `lastdata`.
    validsize: usize,
    /// `off`: the file offset.
    off: i32,
    /// `path`: saved for re-requests.
    path: Vec<u8>,
    /// `lastdata`: the last block received, its TFTP header at `PACKET_HEADER`.
    lastdata: [u8; LASTDATA],
}

impl TftpHandle {
    /// The socket's descriptor.
    ///
    /// # Safety
    ///
    /// As for [`socktodesc`]: the reference ends before the entry point returns.
    unsafe fn io(&self) -> Result<&'static mut IoDesc, Errno> {
        // SAFETY: the caller's contract.
        unsafe { socktodesc(self.iodesc) }
    }
}

/// `servip`: the server's address, network order (the program sets it).
pub static SERVIP: AtomicU32 = AtomicU32::new(0);

/// `tftp_read`'s `static int tc`: blocks fetched, for the twiddle.
static TC: AtomicUsize = AtomicUsize::new(0);

/// `tftperrors[8]`: the error numbers of the TFTP error codes.
const TFTPERRORS: [Errno; 8] = [
    Errno(0), // ???
    Errno::ENOENT,
    Errno::EPERM,
    Errno::ENOSPC,
    Errno::EINVAL, // ???
    Errno::EINVAL, // ???
    Errno::EEXIST,
    Errno::EINVAL, // ???
];

/// The network order 16-bit value at `pkt[at..]`.
fn get16(pkt: &[u8], at: usize) -> u16 {
    pkt.get(at..at + 2)
        .map_or(0, |b| u16::from_ne_bytes([b[0], b[1]]))
}

/// Stores the network order 16-bit value `v` at `pkt[at..]`.
fn put16(pkt: &mut [u8], at: usize, v: u16) {
    pkt[at..at + 2].copy_from_slice(&v.to_ne_bytes());
}

/// `recvtftp(d, pkt, len, tleft)`: receive the next DATA block into `pkt[off..]`; its
/// length, or an error (`errno` 0: not the block we expect).
pub fn recvtftp(d: &mut IoDesc, pkt: &mut [u8], off: usize, tleft: Time) -> Result<usize, Errno> {
    set_errno(Errno(0));

    let n = match readudp(d, pkt, off, tleft) {
        Ok(n) if n >= 4 => n,
        _ => return fail(),
    };

    match ntohs(get16(pkt, off)) {
        DATA => {
            if u64::from(htons(get16(pkt, off + TH_U))) != d.xid {
                // Expected block?
                return fail();
            }
            if d.xid == 1 {
                // First data packet from new port.
                let uh = Udphdr::from_bytes(&pkt[off - size_of::<Udphdr>()..]).unwrap_or_default();
                d.destport = uh.uh_sport;
            } // else check uh_sport has not changed???
            Ok(n - TH_DATA)
        }
        ERROR => {
            let code = ntohs(get16(pkt, off + TH_U));
            if usize::from(code) >= TFTPERRORS.len() {
                crate::printf!("illegal tftp error {}\n", code);
                set_errno(Errno::EIO);
            } else {
                set_errno(TFTPERRORS[usize::from(code)]);
            }
            Err(errno())
        }
        _ => fail(),
    }
}

/// `tftp_makereq(h)`: send request, expect first block (or error).
fn tftp_makereq(h: &mut TftpHandle) -> Result<(), Errno> {
    let mut wbuf = [0u8; PACKET_HEADER + size_of::<Tftphdr>() + FNAME_SIZE + 6];
    let t = PACKET_HEADER;

    put16(&mut wbuf, t, htons(RRQ));
    let mut wtail = t + TH_U;
    let l = h.path.len();
    if wtail + l + 1 + 6 > wbuf.len() {
        return Err(Errno::ENOENT);
    }
    wbuf[wtail..wtail + l].copy_from_slice(&h.path);
    wtail += l + 1;
    wbuf[wtail..wtail + 6].copy_from_slice(b"octet\0");
    wtail += 6;

    // SAFETY: entry point of the TFTP code (called from tftp_open/tftp_read); the reference
    // ends with this function.
    let io = unsafe { h.io() }?;
    // h->iodesc->myport = htons(--tftpport);
    io.myport = htons((TFTPPORT + (getsecs() & 0x3ff) as i32) as u16);
    io.destport = htons(IPPORT_TFTP);
    io.xid = 1; // expected block

    let res = sendrecv(
        io,
        sendudp,
        &mut wbuf[..wtail],
        t,
        recvtftp,
        &mut h.lastdata,
        PACKET_HEADER,
    )?;

    h.currblock = 1;
    h.validsize = res;
    h.islastblock = false;
    if res < SEGSIZE {
        h.islastblock = true; // very short file
    }
    Ok(())
}

/// `tftp_getnextblock(h)`: ack block, expect next.
fn tftp_getnextblock(h: &mut TftpHandle) -> Result<(), Errno> {
    let mut wbuf = [0u8; PACKET_HEADER + size_of::<Tftphdr>()];
    let t = PACKET_HEADER;

    put16(&mut wbuf, t, htons(ACK));
    put16(&mut wbuf, t + TH_U, htons(h.currblock as u16));
    let wtail = t + TH_DATA;

    // SAFETY: as in `tftp_makereq`.
    let io = unsafe { h.io() }?;
    io.xid = (h.currblock + 1) as u64; // expected block

    let res = sendrecv(
        io,
        sendudp,
        &mut wbuf[..wtail],
        t,
        recvtftp,
        &mut h.lastdata,
        PACKET_HEADER,
    )?;
    // 0 is OK!

    h.currblock += 1;
    h.validsize = res;
    if res < SEGSIZE {
        h.islastblock = true; // EOF
    }
    Ok(())
}

/// `tftp_terminate(h)`: acknowledge the last block, or tell the server we stop.
fn tftp_terminate(h: &mut TftpHandle) {
    let mut wbuf = [0u8; PACKET_HEADER + size_of::<Tftphdr>()];
    let t = PACKET_HEADER;
    let mut wtail = t + TH_DATA;

    if h.islastblock {
        put16(&mut wbuf, t, htons(ACK));
        put16(&mut wbuf, t + TH_U, htons(h.currblock as u16));
    } else {
        put16(&mut wbuf, t, htons(ERROR));
        put16(&mut wbuf, t + TH_U, htons(ENOSPACE)); // ???
        wtail += 1; // ERROR data is a string, thus needs NUL.
    }

    // SAFETY: as in `tftp_makereq`.
    if let Ok(io) = unsafe { h.io() } {
        let _ = sendudp(io, &mut wbuf[..wtail], t);
    }
}

/// `tftp_open(path, f)`: request `path` from the server over the socket `f_devdata` points
/// at.
pub fn tftp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_devdata.is_null() {
        return Err(Errno::ENXIO);
    }
    // SAFETY: the TFTP device's open points `f_devdata` at the socket number (a `usize`),
    // which outlives the open file (this module's contract).
    let sock = unsafe { *f.f_devdata.cast::<usize>() };

    let path = &path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())];
    let mut tftpfile = Box::new(TftpHandle {
        iodesc: sock,
        currblock: 0,
        islastblock: false,
        validsize: 0,
        off: 0,
        path: path.to_vec(), // XXXXXXX we hope it's static
        lastdata: [0; LASTDATA],
    });
    {
        // SAFETY: entry point; the reference ends with this block.
        let io = unsafe { tftpfile.io() }?;
        io.destip.s_addr = SERVIP.load(Ordering::Relaxed);
    }

    tftp_makereq(&mut tftpfile)?;

    f.f_fsdata = Some(tftpfile);
    Ok(())
}

/// `tftp_read(f, addr, size, resid)`: read from the file offset into `buf`; `resid` gets
/// what was not read (the end of the file).
pub fn tftp_read(f: &mut OpenFile, buf: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    let tftpfile = f.fsdata::<TftpHandle>().ok_or(Errno::EBADF)?;
    let mut size = buf.len();
    let mut addr = 0usize;

    while size > 0 {
        let needblock = tftpfile.off / SEGSIZE as i32 + 1;

        if tftpfile.currblock > needblock {
            // seek backwards
            tftp_terminate(tftpfile);
            // Don't bother to check retval: it worked for open()
            let _ = tftp_makereq(tftpfile);
        }

        while tftpfile.currblock < needblock {
            if TC.fetch_add(1, Ordering::Relaxed).is_multiple_of(16) {
                twiddle();
            }
            // no answer
            tftp_getnextblock(tftpfile)?;
            if tftpfile.islastblock {
                break;
            }
        }

        if tftpfile.currblock == needblock {
            // The C's `int` to `size_t`: a negative offset is huge, and invalid.
            let offinblock = (tftpfile.off % SEGSIZE as i32) as isize as usize;

            if offinblock > tftpfile.validsize {
                return Err(Errno::EINVAL);
            }
            let inbuffer = tftpfile.validsize - offinblock;
            let count = size.min(inbuffer);
            let from = PACKET_HEADER + TH_DATA + offinblock;
            let Some(data) = tftpfile.lastdata.get(from..from + count) else {
                return Err(Errno::EINVAL);
            };
            buf[addr..addr + count].copy_from_slice(data);

            addr += count;
            tftpfile.off = tftpfile.off.wrapping_add(count as i32);
            size -= count;

            if tftpfile.islastblock && count == inbuffer {
                break; // EOF
            }
        } else {
            return Err(Errno::EINVAL);
        }
    }

    *resid = size;
    Ok(())
}

/// `tftp_close(f)`: tell the server and free the handle.
pub fn tftp_close(f: &mut OpenFile) -> Result<(), Errno> {
    if let Some(tftpfile) = f.fsdata::<TftpHandle>() {
        tftp_terminate(tftpfile);
    }
    f.f_fsdata = None;
    Ok(())
}

/// `tftp_write()`: read only.
pub fn tftp_write(_f: &mut OpenFile, _buf: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

/// `tftp_stat(f, sb)`: a readable file of unknown size.
pub fn tftp_stat(_f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    sb.st_mode = 0o444;
    sb.st_nlink = 1;
    sb.st_uid = 0;
    sb.st_gid = 0;
    sb.st_size = -1;

    Ok(())
}

/// `tftp_seek(f, offset, where)`: the new offset; `SEEK_SET` and `SEEK_CUR` only.
pub fn tftp_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    let tftpfile = f.fsdata::<TftpHandle>().ok_or(Errno::EBADF)?;

    match whence {
        SEEK_SET => tftpfile.off = offset as i32,
        SEEK_CUR => tftpfile.off = tftpfile.off.wrapping_add(offset as i32),
        _ => {
            set_errno(Errno::EOFFSET);
            return Err(Errno::EOFFSET);
        }
    }

    Ok(Off::from(tftpfile.off))
}

/// `tftp_readdir()`: not implemented.
pub fn tftp_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

const _: () = assert!(size_of::<Tftphdr>() == 6);
const _: () = assert!(TH_U == 2 && TH_DATA == 4);

#[cfg(test)]
mod tests;
