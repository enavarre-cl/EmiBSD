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
/* </LICENSES> */

/* <CODE> */
//! FUSE, the userland file system interface: OpenBSD `sys/miscfs/fuse/` (`option FUSE` and
//! `pseudo-device fuse`, feature `fuse`).
//!
//! Headers (types): `fusefs` (`fusefs.h`), `fusefs_node` (`fusefs_node.h`); the protocol is
//! `sys::fusebuf` (`<sys/fusebuf.h>`). Files (functions): `fusebuf` (the request buffers),
//! `fuse_device` (the `/dev/fuse0` character device the daemon reads requests from and
//! writes replies to), `fuse_file`, `fuse_ihash`, `fuse_lookup`, `fuse_vfsops`,
//! `fuse_vnops`. `FUSE_DEBUG` is off, as in GENERIC: the `DPRINTF`s are not compiled.

pub mod fuse_device;
pub mod fuse_file;
pub mod fuse_ihash;
pub mod fuse_lookup;
pub mod fuse_vfsops;
pub mod fuse_vnops;
pub mod fusebuf;
pub mod fusefs;
pub mod fusefs_node;
/* </CODE> */
