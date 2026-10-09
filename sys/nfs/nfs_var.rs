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
 * Copyright (c) 1996 Christos Zoulas.  All rights reserved.
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
 *	This product includes software developed by Christos Zoulas.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
//! `<nfs/nfs_var.h>`: the prototypes of the NFS files and two internal utility macros.
//!
//! Upstream: sys/nfs/nfs_var.h @ 3ce1f3f79392
//!
//! The prototypes are the Rust functions in the modules of their files (`nfs_bio.rs`,
//! `nfs_boot.rs`, `nfs_node.rs`, `nfs_vnops.rs`, `nfs_serv.rs`, `nfs_socket.rs`,
//! `nfs_srvcache.rs`, `nfs_srvsubs.rs`, `nfs_subs.rs`, `nfs_syscalls.rs`, `nfs_kq.rs`), with
//! the signatures documented there; what is left here are the macros.
//!
//! ## Deviations
//! - `mb_offset(m)` and `nfsm_padlen(s)` are functions with the C names.

use crate::nfs::nfsm_subs::nfsm_rndup;
use crate::sys::mbuf::{Mbuf, mtod};

/// `mb_offset(m)`: the address just past the data of `m` (`mtod(m, caddr_t) + m->m_len`).
pub fn mb_offset(m: &Mbuf) -> *mut u8 {
    mtod::<u8>(m).wrapping_add(m.m_len().get() as usize)
}

/// `nfsm_padlen(s)`: the XDR padding after `s` bytes.
pub const fn nfsm_padlen(s: usize) -> usize {
    nfsm_rndup(s) - s
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_rounds_to_words() {
        assert_eq!(nfsm_padlen(0), 0);
        assert_eq!(nfsm_padlen(1), 3);
        assert_eq!(nfsm_padlen(4), 0);
        assert_eq!(nfsm_padlen(7), 1);
    }
}
/* </TESTS> */
