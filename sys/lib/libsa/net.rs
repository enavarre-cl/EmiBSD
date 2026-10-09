/* <LICENSES> */
/*
 * Copyright (c) 1992 Regents of the University of California.
 * All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
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
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
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
 * @(#) Header: net.c,v 1.9 93/08/06 19:32:15 leres Exp  (LBL)
 */

/*
 * Copyright (c) 1993 Adam Glass
 * Copyright (c) 1992 Regents of the University of California.
 * All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
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
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
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
 */
/* </LICENSES> */

/* <CODE> */
//! `net.h` and `net.c`: the standalone network code's interface (its sizes, the packet
//! buffer convention, the timeouts), `sendrecv()`, which sends a request and waits for its
//! answer with exponential backoff, and the address conversions.
//!
//! Upstream: sys/lib/libsa/net.c @ 3ce1f3f79392, sys/lib/libsa/net.h @ 3ce1f3f79392
//!
//! A packet travels down the layers in one buffer: the C hands each layer a pointer to its
//! data and the layer writes its header in front of it ("Caller must leave room for
//! ethernet, ip and udp headers in front!!"). Here a layer takes the whole buffer `pkt` and
//! the offset `off` of its data: the data are `pkt[off..]` (the slice's end is the C's
//! `len`) and the headers go in `pkt[..off]`, which [`PacketHeader`] sizes for all three.
//!
//! ## Deviations
//! - The C's `ssize_t` routines that return -1 return an `Err`. Where the C returns -1
//!   without setting `errno`, the `Err` carries what `errno` holds then, which is what the C
//!   caller reads. `sendrecv` keeps the C's protocol: a receive routine's failure with
//!   `errno` 0 means "not the packet we wait for".
//! - The send and receive routines `sendrecv` takes are [`SendProc`] and [`RecvProc`] over
//!   the buffer convention above (the C's `void *` and size).
//! - `inet_ntoa()` and `intoa()` return the text by value ([`NetStr`]) where the C returns
//!   its `static char buf[]`. `ip_convertaddr` takes a byte string, empty for the C's NULL.
//! - The globals `net.h` declares are in `globals.rs` (`BCEA`, `MYIP`... as `static`s are
//!   named in Rust), `sockets[]` in `netif.rs`; `extern int debug` and the `NET_DEBUG`
//!   messages are not declared: no efiboot Makefile defines `NET_DEBUG`. `rarp_getipaddress`
//!   (`rarp.c`) is not compiled by any program ported here and is not declared.
//! - `getsecs()`, machine-dependent, is the program's `SaConf::getsecs`.

use core::fmt;

use crate::dev::{errno, set_errno};
use crate::hdr::endian::{htonl, ntohl};
use crate::hdr::in_::{INADDR_NONE, InAddr};
use crate::hdr::types::Time;
use crate::iodesc::IoDesc;
use crate::saerrno::Errno;
use crate::stand::{isdigit, sa_conf};

/// `BA`: the Ethernet broadcast address.
pub const BA: [u8; 6] = [0xff; 6];

/// `MAXTMO`: the longest retransmission timeout, in seconds.
pub const MAXTMO: Time = 20;
/// `MINTMO`: the first retransmission timeout, in seconds.
pub const MINTMO: Time = 2;

/// `FNAME_SIZE`: the size of the file and host names.
pub const FNAME_SIZE: usize = 128;
/// `IFNAME_SIZE`: the size of an interface name.
pub const IFNAME_SIZE: usize = 16;
/// `RECV_SIZE`: the receive buffer size ("XXX delete this").
pub const RECV_SIZE: usize = 1536;

/// `ETHER_SIZE`: the Ethernet header's size.
pub const ETHER_SIZE: usize = 14;

/// `struct packet_header`: the room to leave for the headers (14 for `struct
/// ether_header`, 20 for `struct ip`, 8 for `struct udphdr`: 42, padded to 48), `int`
/// aligned.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PacketHeader {
    /// `pad`.
    pub pad: [i32; 48 / core::mem::size_of::<i32>()],
}

/// `sizeof(struct packet_header)`: where the data of a packet buffer start.
pub const PACKET_HEADER: usize = core::mem::size_of::<PacketHeader>();

/// A send routine of `sendrecv`: send `pkt[off..]`; the number of bytes sent.
pub type SendProc = fn(d: &mut IoDesc, pkt: &mut [u8], off: usize) -> Result<usize, Errno>;

/// A receive routine of `sendrecv`: receive into `pkt[off..]`, waiting at most `tleft`
/// seconds; the number of bytes read (0 for EOF), or an `Err` with `errno` set (a real
/// failure) or 0 ("not done yet").
pub type RecvProc =
    fn(d: &mut IoDesc, pkt: &mut [u8], off: usize, tleft: Time) -> Result<usize, Errno>;

/// The text `inet_ntoa()`, `intoa()` and `ether_sprintf()` return, in place of their static
/// buffers.
#[derive(Clone, Copy)]
pub struct NetStr<const N: usize> {
    buf: [u8; N],
    start: usize,
}

impl<const N: usize> NetStr<N> {
    /// The text `buf[start..]`.
    pub(crate) const fn new(buf: [u8; N], start: usize) -> Self {
        Self { buf, start }
    }

    /// The text's bytes (no NUL).
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[self.start..]
    }
}

impl<const N: usize> fmt::Display for NetStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.as_bytes() {
            fmt::Write::write_char(f, char::from(b))?;
        }
        Ok(())
    }
}

impl<const N: usize> fmt::Debug for NetStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// `SAMENET(a1, a2, m)`: whether two addresses are on the same net (`m` as the C applies
/// it, to the network order values).
pub const fn samenet(a1: InAddr, a2: InAddr, m: u32) -> bool {
    (a1.s_addr & m) == (a2.s_addr & m)
}

/// The `Err` of a routine that returns -1 without setting `errno`: what `errno` holds.
pub(crate) fn fail<T>() -> Result<T, Errno> {
    Err(errno())
}

/// `sendrecv(d, sproc, sbuf, ssize, rproc, rbuf, rsize)`: send `sbuf[soff..]` and wait for
/// a reply in `rbuf[roff..]`, with exponential backoff.
///
/// The send routine must return the actual number of bytes written.
///
/// The receive routine can indicate success by returning the number of bytes read; it can
/// return 0 to indicate EOF; it can return an error with a non-zero errno to indicate
/// failure; finally, it can return an error with a zero errno to indicate it isn't done
/// yet.
pub fn sendrecv(
    d: &mut IoDesc,
    sproc: SendProc,
    sbuf: &mut [u8],
    soff: usize,
    rproc: RecvProc,
    rbuf: &mut [u8],
    roff: usize,
) -> Result<usize, Errno> {
    let ssize = sbuf.len().saturating_sub(soff);
    let mut tmo: Time = MINTMO;
    let mut tlast: Time = 0;
    let mut tleft: Time = 0;
    let mut t = getsecs();
    loop {
        if tleft <= 0 {
            if tmo >= MAXTMO {
                set_errno(Errno::ETIMEDOUT);
                return Err(Errno::ETIMEDOUT);
            }
            match sproc(d, sbuf, soff) {
                Ok(cc) if cc >= ssize => {}
                r => crate::exit::panic(format_args!(
                    "sendrecv: short write! ({} < {})",
                    r.map_or(-1, |cc| cc as isize),
                    ssize
                )),
            }

            tleft = tmo;
            tmo <<= 1;
            if tmo > MAXTMO {
                tmo = MAXTMO;
            }
            tlast = t;
        }

        // Try to get a packet and process it.
        match rproc(d, rbuf, roff, tleft) {
            // Return on data, EOF or real error.
            Ok(cc) => return Ok(cc),
            Err(_) if errno() != Errno(0) => return Err(errno()),
            // Timed out or didn't get the packet we're waiting for
            Err(_) => {}
        }
        t = getsecs();
        tleft -= t - tlast;
        tlast = t;
    }
}

/// The byte of a C string at `i`: NUL past its end.
fn at(cp: &[u8], i: usize) -> u8 {
    cp.get(i).copied().unwrap_or(0)
}

/// `inet_addr(cp)`: like `inet_addr()` in the C library, but only base-10 is accepted. The
/// value is in network order; `INADDR_NONE` when `cp` is malformed.
pub fn inet_addr(cp: &[u8]) -> u32 {
    let mut val: u64;
    let mut parts = [0u32; 4];
    let mut pp = 0usize;
    let mut i = 0usize;

    loop {
        // Collect number up to ``.''. Values are specified as for C: 0x=hex, 0=octal,
        // other=decimal.
        val = 0;
        while at(cp, i) != 0 {
            let c = at(cp, i);
            if c.is_ascii_digit() {
                val = val.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
                i += 1;
                continue;
            }
            break;
        }
        if at(cp, i) == b'.' {
            // Internet format:
            //	a.b.c.d
            //	a.b.c	(with c treated as 16-bits)
            //	a.b	(with b treated as 24 bits)
            if pp >= 3 || val > 0xff {
                return htonl(INADDR_NONE);
            }
            parts[pp] = val as u32;
            pp += 1;
            i += 1;
        } else {
            break;
        }
    }
    // Check for trailing characters.
    if at(cp, i) != 0 {
        return htonl(INADDR_NONE);
    }

    // Concoct the address according to the number of parts specified.
    match pp + 1 {
        // a -- 32 bits
        1 => {}
        // a.b -- 8.24 bits
        2 => {
            if val > 0xff_ffff {
                return htonl(INADDR_NONE);
            }
            val |= u64::from(parts[0] << 24);
        }
        // a.b.c -- 8.8.16 bits
        3 => {
            if val > 0xffff {
                return htonl(INADDR_NONE);
            }
            val |= u64::from((parts[0] << 24) | (parts[1] << 16));
        }
        // a.b.c.d -- 8.8.8.8 bits
        _ => {
            if val > 0xff {
                return htonl(INADDR_NONE);
            }
            val |= u64::from((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8));
        }
    }

    htonl(val as u32)
}

/// `inet_ntoa(ia)`: the dotted-quad text of an address.
pub fn inet_ntoa(ia: InAddr) -> NetStr<17> {
    intoa(ia.s_addr)
}

/// `intoa(addr)`: similar to `inet_ntoa()`, for a network order `in_addr_t`.
pub fn intoa(addr: u32) -> NetStr<17> {
    // sizeof(".255.255.255.255")
    let mut buf = [0u8; 17];
    let mut addr = ntohl(addr);
    let mut cp = buf.len();
    // The C's terminating NUL; `NetStr` ends before it.
    cp -= 1;
    let end = cp;

    for _ in 0..4 {
        let mut byte = addr & 0xff;
        cp -= 1;
        buf[cp] = (byte % 10) as u8 + b'0';
        byte /= 10;
        if byte > 0 {
            cp -= 1;
            buf[cp] = (byte % 10) as u8 + b'0';
            byte /= 10;
            if byte > 0 {
                cp -= 1;
                buf[cp] = byte as u8 + b'0';
            }
        }
        cp -= 1;
        buf[cp] = b'.';
        addr >>= 8;
    }

    // `cp + 1`: past the leading dot. Move the text to the front so `NetStr` can end at
    // `N`, dropping the NUL.
    let mut out = [0u8; 17];
    let text = &buf[cp + 1..end];
    let start = out.len() - text.len();
    out[start..].copy_from_slice(text);
    NetStr::new(out, start)
}

/// `number(s, &n)`: the decimal number at the front of `s` (wrapping as the C's `int`) and
/// where it ends.
fn number(s: &[u8], mut i: usize) -> (u32, usize) {
    let mut n: u32 = 0;
    while isdigit(at(s, i)) {
        n = n.wrapping_mul(10).wrapping_add(u32::from(at(s, i) - b'0'));
        i += 1;
    }
    (n, i)
}

/// `IP_ANYADDR`: what `ip_convertaddr` returns for a malformed address.
const IP_ANYADDR: u32 = 0;

/// `ip_convertaddr(p)`: the network order value of a dotted-quad address `a.b.c.d`, or
/// `IP_ANYADDR` (0) when it is empty or malformed.
pub fn ip_convertaddr(p: &[u8]) -> u32 {
    let mut addr: u32 = 0;

    if at(p, 0) == 0 {
        return IP_ANYADDR;
    }
    let (n, mut i) = number(p, 0);
    addr |= (n << 24) & 0xff00_0000;
    if at(p, i) == 0 || at(p, i) != b'.' {
        return IP_ANYADDR;
    }
    i += 1;
    let (n, mut i) = number(p, i);
    addr |= (n << 16) & 0x00ff_0000;
    if at(p, i) == 0 || at(p, i) != b'.' {
        return IP_ANYADDR;
    }
    i += 1;
    let (n, mut i) = number(p, i);
    addr |= (n << 8) & 0xff00;
    if at(p, i) == 0 || at(p, i) != b'.' {
        return IP_ANYADDR;
    }
    i += 1;
    let (n, i) = number(p, i);
    addr |= n & 0xff;
    if at(p, i) != 0 {
        return IP_ANYADDR;
    }

    htonl(addr)
}

/// `getsecs()`: seconds of the real-time clock (the program's `SaConf::getsecs`).
pub fn getsecs() -> Time {
    (sa_conf().getsecs)()
}

const _: () = assert!(PACKET_HEADER == 48);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inet_addr_parses_base_10() {
        assert_eq!(
            inet_addr(b"10.0.2.15\0"),
            u32::from_ne_bytes([10, 0, 2, 15])
        );
        assert_eq!(inet_addr(b"10.2.15"), u32::from_ne_bytes([10, 2, 0, 15]));
        assert_eq!(
            inet_addr(b"10.65535"),
            u32::from_ne_bytes([10, 0, 255, 255])
        );
        assert_eq!(inet_addr(b"167772687"), u32::from_ne_bytes([10, 0, 2, 15]));
        assert_eq!(inet_addr(b"256.1.1.1"), INADDR_NONE);
        assert_eq!(inet_addr(b"1.2.3.4.5"), INADDR_NONE);
        assert_eq!(inet_addr(b"1.2.3.256"), INADDR_NONE);
        assert_eq!(inet_addr(b"1.2.x"), INADDR_NONE);
    }

    #[test]
    fn intoa_and_ip_convertaddr() {
        let a = u32::from_ne_bytes([192, 168, 0, 255]);
        assert_eq!(intoa(a).as_bytes(), b"192.168.0.255");
        assert_eq!(intoa(0).as_bytes(), b"0.0.0.0");
        assert_eq!(inet_ntoa(InAddr { s_addr: a }).as_bytes(), b"192.168.0.255");
        assert_eq!(ip_convertaddr(b"192.168.0.255\0"), a);
        assert_eq!(ip_convertaddr(b""), 0);
        assert_eq!(ip_convertaddr(b"192.168.0"), 0);
        assert_eq!(ip_convertaddr(b"192.168.0.1x"), 0);
    }
}
/* </TESTS> */
