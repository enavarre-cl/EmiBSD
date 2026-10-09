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
//! libsa: the standalone library OpenBSD's boot programs link (`sys/lib/libsa`).
//!
//! The device and file system switches, the open file table, the console, `printf`, the
//! allocator, the FFS1/FFS2 and ISO 9660 readers, gzip-transparent reading (`cread`), the
//! ELF kernel loader, and the network stack the network boot programs compile (`netif`,
//! `ether`, `arp`, `netudp`, `tftp`). A boot program (efiboot: `sys/arch/amd64/stand/efiboot`) registers its
//! tables and machine-dependent routines with [`stand::sa_conf_register`], installs
//! [`sa_alloc::SaAlloc`] as its global allocator and calls [`sa_alloc::heap_init`].
//!
//! A leaf crate, as OpenBSD builds libsa as a library of its own: it depends on libkern and
//! libz only (the boot programs take `inflate.c`, `crc32.c`, `adler32.c` from `${S}/lib/libz`
//! and `strlcpy.c` and the 64-bit division helpers from `${S}/lib/libkern` through `.PATH`;
//! `mem*`/`str*` are Rust's own). The kernel headers it reads are in [`hdr`].
//!
//! Built as efiboot builds it: with `__INTERNAL_LIBSA_CREAD` (`open`/`close`/`read`/`lseek`
//! decompress; the plain ones are `oopen`...), `SMALL`, `SLOW`, without `SOFTRAID` (see
//! `docs/ARCHITECTURE.md`, "Boot loaders").

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arc4;
pub mod arp;
pub mod cd9660;
pub mod close;
pub mod closeall;
pub mod cons;
pub mod cread;
pub mod ctime;
pub mod dev;
pub mod disklabel;
pub mod dkcksum;
pub mod ether;
pub mod exit;
pub mod fchmod;
pub mod fstat;
pub mod getchar;
pub mod globals;
pub mod hdr;
pub mod hexdump;
pub mod in_cksum;
pub mod iodesc;
pub mod loadfile;
pub mod loadfile_elf;
pub mod lseek;
pub mod net;
pub mod netif;
pub mod netudp;
pub mod open;
pub mod printf;
pub mod putchar;
pub mod read;
pub mod readdir;
/// `alloc.c`; the module is not named `alloc`, which is Rust's allocation crate.
#[path = "alloc.rs"]
pub mod sa_alloc;
pub mod saerrno;
pub mod snprintf;
pub mod stand;
pub mod stat;
pub mod strerror;
pub mod strtol;
pub mod strtoll;
pub mod tftp;
pub mod ufs;
pub mod ufs2;

#[cfg(test)]
pub(crate) mod testutil;
/* </CODE> */
