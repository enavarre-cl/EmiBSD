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
//! `iodesc.h`: the I/O descriptor of a network socket of the standalone network code: the
//! two ends' addresses and ports, the transaction id and the interface it uses.
//!
//! Upstream: sys/lib/libsa/iodesc.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct netif *io_netif` is the interface itself, `Option<Netif>` (`None` for the C's
//!   NULL, a free socket): `netif_select` returns the chosen interface by value where the C
//!   returns its `static struct netif best_if`, which every open socket would then share.

use crate::hdr::in_::InAddr;
use crate::netif::Netif;

/// `struct iodesc`: a network socket.
#[derive(Clone, Copy, Debug, Default)]
pub struct IoDesc {
    /// `destip`: destination ip address, network order.
    pub destip: InAddr,
    /// `myip`: local ip address, network order.
    pub myip: InAddr,
    /// `destport`: destination port, network order.
    pub destport: u16,
    /// `myport`: local port, network order.
    pub myport: u16,
    /// `xid`: transaction identification (for TFTP, the block expected next).
    pub xid: u64,
    /// `myea`: my ethernet address.
    pub myea: [u8; 6],
    /// `io_netif`: the interface; `None` while the socket is free.
    pub io_netif: Option<Netif>,
}

impl IoDesc {
    /// A free socket (the C's zeroed `struct iodesc`).
    pub const fn new() -> Self {
        Self {
            destip: InAddr { s_addr: 0 },
            myip: InAddr { s_addr: 0 },
            destport: 0,
            myport: 0,
            xid: 0,
            myea: [0; 6],
            io_netif: None,
        }
    }
}
/* </CODE> */
