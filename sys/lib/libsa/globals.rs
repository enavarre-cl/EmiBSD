/*	$OpenBSD: globals.c,v 1.4 2014/07/13 15:31:20 mpi Exp $	*/
/*	$NetBSD: globals.c,v 1.3 1995/09/18 21:19:27 pk Exp $	*/
/* <LICENSES> */
/*
 *	globals.c:
 *
 *	global variables should be separate, so nothing else
 *	must be included extraneously.
 */
/* </LICENSES> */

//! The standalone network code's global variables (`net.h`'s `extern`s): the addresses
//! the boot protocol learns and the names it is given.
//!
//! Upstream: sys/lib/libsa/globals.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Rust names `static`s in upper case: `bcea` is [`BCEA`], `myip` [`MYIP`] and so on.
//! - The `struct in_addr` globals are their `s_addr` (network order) in an `AtomicU32`, and
//!   `hostnamelen`/`domainnamelen` `AtomicI32`s: a program reads and writes them without
//!   `unsafe` (efiboot's `efi_pxeprobe` stores `gateip`). The name buffers are
//!   [`StaticCell`]s, accessed as the open file table is: one reference at a time, by the
//!   single-threaded standalone program.
//! - `bcea[]`, never written, is an immutable `static`.
//! - `netmask` keeps the C's initial value, `0xffffff00`, which `SAMENET` applies to network
//!   order addresses as the C does.

use core::sync::atomic::{AtomicI32, AtomicU32};

use libkern::staticcell::StaticCell;

use crate::net::{BA, FNAME_SIZE, IFNAME_SIZE};

/// `bcea[6]`: the broadcast ethernet address.
pub static BCEA: [u8; 6] = BA;

/// `rootpath[FNAME_SIZE]`: the root mount path, `/`.
pub static ROOTPATH: StaticCell<[u8; FNAME_SIZE]> = StaticCell::new(rootpath_init());
/// `bootfile[FNAME_SIZE]`: bootp says to boot this.
pub static BOOTFILE: StaticCell<[u8; FNAME_SIZE]> = StaticCell::new([0; FNAME_SIZE]);
/// `hostname[FNAME_SIZE]`: our hostname.
pub static HOSTNAME: StaticCell<[u8; FNAME_SIZE]> = StaticCell::new([0; FNAME_SIZE]);
/// `hostnamelen`.
pub static HOSTNAMELEN: AtomicI32 = AtomicI32::new(0);
/// `domainname[FNAME_SIZE]`: our DNS domain.
pub static DOMAINNAME: StaticCell<[u8; FNAME_SIZE]> = StaticCell::new([0; FNAME_SIZE]);
/// `domainnamelen`.
pub static DOMAINNAMELEN: AtomicI32 = AtomicI32::new(0);
/// `ifname[IFNAME_SIZE]`: name of interface (e.g. "le0").
pub static IFNAME: StaticCell<[u8; IFNAME_SIZE]> = StaticCell::new([0; IFNAME_SIZE]);
/// `myip`: my ip address, network order.
pub static MYIP: AtomicU32 = AtomicU32::new(0);
/// `nameip`: DNS server ip address, network order.
pub static NAMEIP: AtomicU32 = AtomicU32::new(0);
/// `rootip`: root ip address, network order.
pub static ROOTIP: AtomicU32 = AtomicU32::new(0);
/// `swapip`: swap ip address, network order.
pub static SWAPIP: AtomicU32 = AtomicU32::new(0);
/// `gateip`: the gateway's ip address, network order ("swap ip address", says the C).
pub static GATEIP: AtomicU32 = AtomicU32::new(0);
/// `netmask`: subnet or net mask.
pub static NETMASK: AtomicU32 = AtomicU32::new(0xffff_ff00);

/// `rootpath`'s initial value, `"/"`.
const fn rootpath_init() -> [u8; FNAME_SIZE] {
    let mut p = [0; FNAME_SIZE];
    p[0] = b'/';
    p
}
