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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // The network stack end to end: a fake interface driver whose other end is an in-memory
    // ARP responder and TFTP server, and libsa's `netif`, `ether`, `arp`, `netudp` and `tftp`
    // talking to it, directly and through `open()`/`read()`/`lseek()`.

    extern crate std;

    use core::ffi::c_void;
    use core::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::vec;

    use super::*;
    use crate::arp::arp_num;
    use crate::globals::GATEIP;
    use crate::hdr::in_::InAddr;
    use crate::netif::{Netif, NetifDif, NetifDriver, NetifStats, netif_close, netif_open};
    use crate::stand::{Devsw, FsOps, SaConf, sa_conf_register};
    use crate::testutil;

    const CLIENT_EA: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    const SERVER_EA: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x02];
    const CLIENT_IP: [u8; 4] = [10, 0, 2, 15];
    const SERVER_IP: [u8; 4] = [10, 0, 2, 2];
    /// The server's transfer port (its TID).
    const SERVER_TID: u16 = 3000;

    /// The other end of the wire.
    struct Server {
        /// Frames on their way to the client.
        rx: VecDeque<Vec<u8>>,
        /// Requests the server answers: file name and contents.
        files: Vec<(&'static [u8], Vec<u8>)>,
        /// The file being transferred.
        file: Vec<u8>,
        /// The client's port.
        client_port: u16,
        /// Read requests seen.
        rrqs: usize,
        /// ARP requests for the server's address answered.
        arps: usize,
        /// ARP replies the client sent to the server's request.
        arp_replies: usize,
        /// ACKs seen, by block.
        acks: Vec<u16>,
        /// ERROR packets the client sent (it stopped a transfer).
        client_errors: usize,
        /// Before this block, ask the client who it is (ARP).
        arp_before_block: Option<u16>,
        /// Send the DATA blocks without a UDP checksum.
        no_udp_sum: bool,
        /// Answer read requests with this TFTP error code instead.
        error_code: Option<u16>,
    }

    static SERVER: Mutex<Server> = Mutex::new(Server {
        rx: VecDeque::new(),
        files: Vec::new(),
        file: Vec::new(),
        client_port: 0,
        rrqs: 0,
        arps: 0,
        arp_replies: 0,
        acks: Vec::new(),
        client_errors: 0,
        arp_before_block: None,
        no_udp_sum: false,
        error_code: None,
    });

    /// The fake clock: a second per call.
    static CLOCK: AtomicI64 = AtomicI64::new(1000);

    fn server() -> std::sync::MutexGuard<'static, Server> {
        SERVER.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The Internet checksum, written independently of `in_cksum`: the bytes to store.
    fn cksum(b: &[u8]) -> [u8; 2] {
        let mut sum: u32 = 0;
        for w in b.chunks(2) {
            sum += u32::from(w[0]) << 8 | u32::from(*w.get(1).unwrap_or(&0));
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        (!(sum as u16)).to_be_bytes()
    }

    fn ether(dst: [u8; 6], etype: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&dst);
        f.extend_from_slice(&SERVER_EA);
        f.extend_from_slice(&etype.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    fn arp(op: u16, sha: [u8; 6], spa: [u8; 4], tha: [u8; 6], tpa: [u8; 4]) -> Vec<u8> {
        let mut a = vec![0, 1, 0x08, 0x00, 6, 4];
        a.extend_from_slice(&op.to_be_bytes());
        a.extend_from_slice(&sha);
        a.extend_from_slice(&spa);
        a.extend_from_slice(&tha);
        a.extend_from_slice(&tpa);
        a
    }

    /// A UDP datagram from the server to the client, in an Ethernet frame.
    fn udp_frame(sport: u16, dport: u16, payload: &[u8], with_sum: bool) -> Vec<u8> {
        let ulen = (8 + payload.len()) as u16;
        let mut ip = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 17, 0, 0];
        ip[2..4].copy_from_slice(&(20 + ulen).to_be_bytes());
        ip.extend_from_slice(&SERVER_IP);
        ip.extend_from_slice(&CLIENT_IP);
        let s = cksum(&ip);
        ip[10..12].copy_from_slice(&s);

        let mut udp = Vec::new();
        udp.extend_from_slice(&sport.to_be_bytes());
        udp.extend_from_slice(&dport.to_be_bytes());
        udp.extend_from_slice(&ulen.to_be_bytes());
        udp.extend_from_slice(&[0, 0]);
        udp.extend_from_slice(payload);
        if with_sum {
            let mut pseudo = Vec::new();
            pseudo.extend_from_slice(&SERVER_IP);
            pseudo.extend_from_slice(&CLIENT_IP);
            pseudo.extend_from_slice(&[0, 17]);
            pseudo.extend_from_slice(&ulen.to_be_bytes());
            pseudo.extend_from_slice(&udp);
            let s = cksum(&pseudo);
            udp[6..8].copy_from_slice(&s);
        }
        ip.extend_from_slice(&udp);
        ether(CLIENT_EA, 0x0800, &ip)
    }

    impl Server {
        fn nblocks(&self) -> u16 {
            (self.file.len() / 512 + 1) as u16
        }

        fn send_block(&mut self, n: u16) {
            if self.arp_before_block == Some(n) {
                let req = arp(1, SERVER_EA, SERVER_IP, [0; 6], CLIENT_IP);
                self.rx.push_back(ether([0xff; 6], 0x0806, &req));
            }
            let from = (usize::from(n) - 1) * 512;
            let to = (from + 512).min(self.file.len());
            let mut p = vec![0, 3];
            p.extend_from_slice(&n.to_be_bytes());
            p.extend_from_slice(&self.file[from..to]);
            let f = udp_frame(SERVER_TID, self.client_port, &p, !self.no_udp_sum);
            self.rx.push_back(f);
        }

        /// The server's side of a frame the client sent.
        fn input(&mut self, f: &[u8]) {
            assert_eq!(&f[6..12], &CLIENT_EA, "the client's source address");
            match u16::from_be_bytes([f[12], f[13]]) {
                0x0806 => {
                    let a = &f[14..];
                    let op = u16::from_be_bytes([a[6], a[7]]);
                    if op == 1 && a[24..28] == SERVER_IP {
                        assert_eq!(&f[0..6], &[0xff; 6], "requests are broadcast");
                        assert_eq!(&a[8..14], &CLIENT_EA);
                        assert_eq!(&a[14..18], &CLIENT_IP);
                        self.arps += 1;
                        let rep = arp(2, SERVER_EA, SERVER_IP, CLIENT_EA, CLIENT_IP);
                        self.rx.push_back(ether(CLIENT_EA, 0x0806, &rep));
                    } else if op == 2 {
                        assert_eq!(&f[0..6], &SERVER_EA, "the reply goes to the asker");
                        assert_eq!(&a[8..14], &CLIENT_EA);
                        assert_eq!(&a[14..18], &CLIENT_IP);
                        assert_eq!(&a[18..24], &SERVER_EA);
                        assert_eq!(&a[24..28], &SERVER_IP);
                        assert_eq!(f.len(), 14 + 46, "padded to 46 bytes");
                        self.arp_replies += 1;
                    }
                }
                0x0800 => {
                    assert_eq!(&f[0..6], &SERVER_EA);
                    let ip = &f[14..];
                    assert_eq!(cksum(&ip[..20]), [0, 0], "the IP checksum");
                    assert_eq!(ip[0], 0x45);
                    assert_eq!(ip[9], 17);
                    assert_eq!(ip[8], 4, "ip_ttl is IP_TTL");
                    assert_eq!(&ip[12..16], &CLIENT_IP);
                    assert_eq!(&ip[16..20], &SERVER_IP);
                    let iplen = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
                    assert_eq!(iplen, ip.len());
                    let udp = &ip[20..iplen];
                    let ulen = u16::from_be_bytes([udp[4], udp[5]]);
                    assert_eq!(usize::from(ulen), udp.len());
                    let mut pseudo = Vec::new();
                    pseudo.extend_from_slice(&CLIENT_IP);
                    pseudo.extend_from_slice(&SERVER_IP);
                    pseudo.extend_from_slice(&[0, 17]);
                    pseudo.extend_from_slice(&ulen.to_be_bytes());
                    pseudo.extend_from_slice(udp);
                    assert_eq!(cksum(&pseudo), [0, 0], "the UDP checksum");
                    let sport = u16::from_be_bytes([udp[0], udp[1]]);
                    let dport = u16::from_be_bytes([udp[2], udp[3]]);
                    let t = &udp[8..];
                    let op = u16::from_be_bytes([t[0], t[1]]);
                    if dport == 69 {
                        assert_eq!(op, 1, "RRQ");
                        let name_end = 2 + t[2..].iter().position(|&c| c == 0).unwrap();
                        let name = &t[2..name_end];
                        assert_eq!(&t[name_end + 1..], b"octet\0");
                        self.rrqs += 1;
                        self.client_port = sport;
                        if let Some(code) = self.error_code {
                            let mut p = vec![0, 5];
                            p.extend_from_slice(&code.to_be_bytes());
                            p.extend_from_slice(b"no\0");
                            let f = udp_frame(SERVER_TID, sport, &p, true);
                            self.rx.push_back(f);
                            return;
                        }
                        match self.files.iter().find(|(n, _)| *n == name) {
                            Some((_, data)) => {
                                self.file = data.clone();
                                self.send_block(1);
                            }
                            None => {
                                let mut p = vec![0, 5, 0, 1];
                                p.extend_from_slice(b"File not found\0");
                                let f = udp_frame(SERVER_TID, sport, &p, true);
                                self.rx.push_back(f);
                            }
                        }
                    } else {
                        assert_eq!(dport, SERVER_TID, "the client answers the server's TID");
                        assert_eq!(sport, self.client_port);
                        match op {
                            4 => {
                                let n = u16::from_be_bytes([t[2], t[3]]);
                                self.acks.push(n);
                                if n < self.nblocks() {
                                    self.send_block(n + 1);
                                }
                            }
                            5 => self.client_errors += 1,
                            _ => panic!("unexpected TFTP opcode {op}"),
                        }
                    }
                }
                t => panic!("unexpected ether type {t:#x}"),
            }
        }
    }

    fn fake_match(_nif: &mut Netif, _hint: &[u8]) -> i32 {
        1
    }

    fn fake_probe(_nif: &mut Netif, hint: &[u8]) -> i32 {
        if hint.starts_with(b"fake") { 0 } else { -1 }
    }

    fn fake_init(desc: &mut IoDesc, _hint: &[u8]) {
        desc.myea = CLIENT_EA;
        desc.myip = InAddr {
            s_addr: u32::from_ne_bytes(CLIENT_IP),
        };
        desc.xid = 1;
    }

    fn fake_get(_desc: &mut IoDesc, pkt: &mut [u8], _timo: Time) -> Result<usize, Errno> {
        let Some(f) = server().rx.pop_front() else {
            return Err(Errno::EIO);
        };
        let n = f.len().min(pkt.len());
        pkt[..n].copy_from_slice(&f[..n]);
        Ok(n)
    }

    fn fake_put(_desc: &mut IoDesc, pkt: &[u8]) -> Result<usize, Errno> {
        server().input(pkt);
        Ok(pkt.len())
    }

    fn fake_end(_nif: &mut Netif) {}

    static FAKE_STATS: NetifStats = NetifStats::new();
    static FAKE_IFS: [NetifDif; 1] = [NetifDif::new(0, 1, &FAKE_STATS, core::ptr::null_mut())];
    static FAKE_DRIVER: NetifDriver = NetifDriver {
        netif_bname: "fake",
        netif_match: fake_match,
        netif_probe: fake_probe,
        netif_init: fake_init,
        netif_get: fake_get,
        netif_put: fake_put,
        netif_end: fake_end,
        netif_ifs: &FAKE_IFS,
    };
    static DRIVERS: [&NetifDriver; 1] = [&FAKE_DRIVER];

    /// The socket the `tftp` device opened (efiboot's `tftpdev_sock`).
    static TFTPDEV_SOCK: AtomicUsize = AtomicUsize::new(usize::MAX);

    fn tftpdev_open(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
        let Some(rest) = file.strip_prefix(b"tftp:") else {
            return Err(Errno::ENXIO);
        };
        let sock = netif_open(b"fake")?;
        TFTPDEV_SOCK.store(sock, Ordering::Relaxed);
        f.f_devdata = TFTPDEV_SOCK.as_ptr().cast::<c_void>();
        *file = rest;
        Ok(())
    }

    fn tftpdev_close(_f: &mut OpenFile) -> Result<(), Errno> {
        netif_close(TFTPDEV_SOCK.load(Ordering::Relaxed))
    }

    fn tftpdev_strategy(
        _devdata: *mut c_void,
        _rw: i32,
        _blk: crate::hdr::types::Daddr,
        _buf: &mut [u8],
        _rsize: Option<&mut usize>,
    ) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    fn tftpdev_ioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    static DEVSW: [Devsw; 1] = [Devsw {
        dv_name: "tftp",
        dv_strategy: tftpdev_strategy,
        dv_open: tftpdev_open,
        dv_close: tftpdev_close,
        dv_ioctl: tftpdev_ioctl,
    }];

    static FILE_SYSTEM: [FsOps; 1] = [FsOps {
        open: tftp_open,
        close: tftp_close,
        read: tftp_read,
        write: tftp_write,
        seek: tftp_seek,
        stat: tftp_stat,
        readdir: tftp_readdir,
        fchmod: None,
    }];

    fn devopen<'a>(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno> {
        let mut file = fname;
        (DEVSW[0].dv_open)(f, &mut file)?;
        f.f_dev = Some(&DEVSW[0]);
        Ok(file)
    }

    static NET_CONF: SaConf = SaConf {
        file_system: &FILE_SYSTEM,
        devsw: &DEVSW,
        constab: &testutil::CONSTAB,
        devopen,
        rtt: || panic!("_rtt"),
        loadaddr: |a, offset| a.wrapping_add(offset),
        netif_drivers: &DRIVERS,
        getsecs: || CLOCK.fetch_add(1, Ordering::Relaxed),
    };

    /// A file of `len` bytes that tells its offsets apart.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    /// Takes libsa's globals, registers the network configuration and a fresh server serving
    /// `files`, and empties the ARP cache.
    fn net_setup(files: Vec<(&'static [u8], Vec<u8>)>) -> std::sync::MutexGuard<'static, ()> {
        let guard = testutil::setup();
        sa_conf_register(&NET_CONF);
        SERVIP.store(u32::from_ne_bytes(SERVER_IP), Ordering::Relaxed);
        // As efiboot's `efi_pxeprobe`: the gateway is the server.
        GATEIP.store(u32::from_ne_bytes(SERVER_IP), Ordering::Relaxed);
        arp_num.store(1, Ordering::Relaxed);
        let mut s = server();
        s.rx.clear();
        s.files = files;
        s.file.clear();
        s.rrqs = 0;
        s.arps = 0;
        s.arp_replies = 0;
        s.acks.clear();
        s.client_errors = 0;
        s.arp_before_block = None;
        s.no_udp_sum = false;
        s.error_code = None;
        drop(s);
        guard
    }

    /// An open file on the socket `sock` (what the `tftp` device's open leaves).
    fn open_file(sock: &'static AtomicUsize) -> OpenFile {
        let mut f = OpenFile::new();
        f.f_devdata = sock.as_ptr().cast::<c_void>();
        f
    }

    #[test]
    fn tftp_reads_a_file_end_to_end() {
        let data = pattern(1300);
        let _g = net_setup(vec![(b"bsd", data.clone())]);
        static SOCK: AtomicUsize = AtomicUsize::new(0);
        SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
        let mut f = open_file(&SOCK);

        tftp_open(b"bsd\0", &mut f).unwrap();
        {
            let s = server();
            assert_eq!(s.arps, 1, "the client asked who has the server");
            assert_eq!(s.rrqs, 1);
        }

        let mut out = Vec::new();
        let mut buf = [0u8; 300];
        loop {
            let mut resid = 0;
            tftp_read(&mut f, &mut buf, &mut resid).unwrap();
            out.extend_from_slice(&buf[..buf.len() - resid]);
            if resid != 0 {
                break;
            }
        }
        assert_eq!(out, data);

        let mut sb = Stat::default();
        tftp_stat(&mut f, &mut sb).unwrap();
        assert_eq!((sb.st_mode, sb.st_size), (0o444, -1));
        assert_eq!(tftp_write(&mut f, &[], &mut 0), Err(Errno::EROFS));
        assert_eq!(tftp_readdir(&mut f, None), Err(Errno::EROFS));
        assert_eq!(tftp_seek(&mut f, 0, 2), Err(Errno::EOFFSET));

        tftp_close(&mut f).unwrap();
        assert_eq!(
            server().acks,
            [1, 2, 3],
            "the last block is acknowledged at close"
        );
        netif_close(SOCK.load(Ordering::Relaxed)).unwrap();
        assert_eq!(netif_close(SOCK.load(Ordering::Relaxed)), Err(Errno::EBADF));
    }

    #[test]
    fn tftp_through_open_read_and_lseek() {
        use crate::close::oclose;
        use crate::lseek::olseek;
        use crate::open::oopen;
        use crate::read::oread;

        // A multiple of SEGSIZE: the last block is empty.
        let data = pattern(1024);
        let _g = net_setup(vec![(b"/bsd.rd", data.clone())]);

        let fd = oopen(b"tftp:/bsd.rd", 0).unwrap();
        let mut buf = vec![0u8; 700];
        assert_eq!(oread(fd, &mut buf).unwrap(), 700);
        assert_eq!(buf, data[..700]);

        // Seek backwards: the client stops the transfer and asks again.
        assert_eq!(olseek(fd, 100, SEEK_SET).unwrap(), 100);
        let mut all = vec![0u8; 2000];
        assert_eq!(oread(fd, &mut all).unwrap(), 924);
        assert_eq!(all[..924], data[100..]);
        {
            let s = server();
            assert_eq!(s.rrqs, 2);
            assert_eq!(s.client_errors, 1, "the abandoned transfer was told so");
        }
        assert_eq!(oread(fd, &mut all).unwrap(), 0);
        oclose(fd).unwrap();
        assert_eq!(*server().acks.last().unwrap(), 3);
    }

    #[test]
    fn tftp_answers_arp_and_reads_unchecksummed_blocks() {
        let data = pattern(2000);
        let _g = net_setup(vec![(b"bsd", data.clone())]);
        {
            let mut s = server();
            s.arp_before_block = Some(2);
            s.no_udp_sum = true;
        }
        static SOCK: AtomicUsize = AtomicUsize::new(0);
        SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
        let mut f = open_file(&SOCK);
        tftp_open(b"bsd", &mut f).unwrap();
        let mut buf = vec![0u8; 2000];
        let mut resid = 0;
        tftp_read(&mut f, &mut buf, &mut resid).unwrap();
        assert_eq!((resid, &buf[..]), (0, &data[..]));
        assert_eq!(
            server().arp_replies,
            1,
            "readudp answered the server's ARP request"
        );
        tftp_close(&mut f).unwrap();
        netif_close(SOCK.load(Ordering::Relaxed)).unwrap();
    }

    #[test]
    fn tftp_errors() {
        let _g = net_setup(vec![]);
        static SOCK: AtomicUsize = AtomicUsize::new(0);
        SOCK.store(netif_open(b"fake").unwrap(), Ordering::Relaxed);
        let mut f = open_file(&SOCK);

        assert_eq!(tftp_open(b"nonexistent", &mut f), Err(Errno::ENOENT));
        server().error_code = Some(9);
        assert_eq!(tftp_open(b"x", &mut f), Err(Errno::EIO));
        assert!(testutil::output().contains("illegal tftp error 9"));
        server().error_code = Some(6);
        assert_eq!(tftp_open(b"x", &mut f), Err(Errno::EEXIST));
        assert!(f.f_fsdata.is_none());

        let long = [b'a'; 200];
        assert_eq!(
            tftp_open(&long, &mut f),
            Err(Errno::ENOENT),
            "too long a path"
        );
        netif_close(SOCK.load(Ordering::Relaxed)).unwrap();

        // A hint no driver probes.
        assert_eq!(netif_open(b"nope"), Err(Errno::EINVAL));
        assert!(testutil::output().contains("netboot: couldn't probe fake0"));
    }

    #[test]
    fn sendrecv_times_out() {
        let _g = net_setup(vec![]);
        let mut d = IoDesc::new();
        static SENT: AtomicUsize = AtomicUsize::new(0);
        SENT.store(0, Ordering::Relaxed);
        let mut sbuf = [0u8; 4];
        let mut rbuf = [0u8; 4];
        let r = sendrecv(
            &mut d,
            |_, pkt, off| {
                SENT.fetch_add(1, Ordering::Relaxed);
                Ok(pkt.len() - off)
            },
            &mut sbuf,
            0,
            |_, _, _, _| {
                crate::dev::set_errno(Errno(0));
                Err(Errno(0))
            },
            &mut rbuf,
            0,
        );
        assert_eq!(r, Err(Errno::ETIMEDOUT));
        assert_eq!(errno(), Errno::ETIMEDOUT);
        // Timeouts of 2, 4, 8 and 16 seconds, then MAXTMO.
        assert_eq!(SENT.load(Ordering::Relaxed), 4);
    }
}
/* </TESTS> */
