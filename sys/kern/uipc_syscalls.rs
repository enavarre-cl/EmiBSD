/*	$OpenBSD: uipc_syscalls.c,v 1.229 2026/09/04 02:13:45 dlg Exp $	*/
/*	$NetBSD: uipc_syscalls.c,v 1.19 1996/02/09 19:00:48 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1990, 1993
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
 *	@(#)uipc_syscalls.c	8.4 (Berkeley) 2/21/94
 */
/* </LICENSES> */

//! The socket system calls: `kern/uipc_syscalls.c`.
//!
//! Upstream: sys/kern/uipc_syscalls.c @ 3ce1f3f79392
//!
//! Status: `wip`. Sockets (`uipc_socket.c`, `uipc_socket2.c`, `uipc_usrreq.c`) are not
//! ported, so the socket system calls (`socket`, `bind`, `connect`, `sendto`, `recvmsg`, ...)
//! stay `sys_nosys` in the table. Ported here: `sys_setrtable` and `sys_getrtable` (the
//! process's routing table, which `net/rtable.c` knows), and `sys_ypconnect` up to the point
//! where it needs a socket: without a YP domain name (`kern.domainname` empty, the default)
//! it fails with `EAFNOSUPPORT` as the C does; with one, the binding file lookup and the
//! socket are reported.
//!
//! ## Deviations
//! - `sys_ypconnect` with a domain name set reports the rest (`namei` of
//!   `/var/yp/binding/<domain>.2`, `VOP_ADVLOCK`, `socreate`, `soconnect`, `falloc`) and
//!   fails with `ENOSYS`: there are no sockets to connect.

use core::sync::atomic::Ordering;

use crate::kern::kern_prot::suser;
use crate::kern::kern_sysctl::{DOMAINNAME, DOMAINNAMELEN};
use crate::net::rtable::rtable_exists;
use crate::sys::errno::Errno;
use crate::sys::proc::{PS_CHROOT, Proc};
use crate::sys::socket::{SOCK_DGRAM, SOCK_STREAM};
use crate::sys::syscallargs::{SysSetrtableArgs, SysYpconnectArgs};
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::unported;

/// `setrtable(2)`: moves the process to routing table `rtableid`; only root may leave a
/// table other than 0.
pub fn sys_setrtable(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetrtableArgs = sysargs(v);
    let ps_rtableid = p.process().ps_rtableid.load(Ordering::Relaxed);

    let rtableid = uap.rtableid.get();

    if i64::from(ps_rtableid) == i64::from(rtableid) {
        return Ok(());
    }
    if ps_rtableid != 0 {
        suser(p)?;
    }
    if rtableid < 0 || !rtable_exists(rtableid as u32) {
        return Err(Errno::EINVAL);
    }

    p.process()
        .ps_rtableid
        .store(rtableid as u32, Ordering::Relaxed);
    Ok(())
}

/// `getrtable(2)`: the process's routing table.
pub fn sys_getrtable(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.process().ps_rtableid.load(Ordering::Relaxed) as i32 as Register;
    Ok(())
}

/// `ypconnect(2)`: a socket connected to the YP server `ypbind(8)` found (see the module's
/// deviations).
pub fn sys_ypconnect(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysYpconnectArgs = sysargs(v);

    // SAFETY: `domainname` is written only by `kern.domainname` under `sysctl_lock`; this is
    // a read of the bytes the C reads without a lock too.
    let domainname = unsafe { DOMAINNAME.get() };
    let len = (DOMAINNAMELEN.load(Ordering::Relaxed).max(0) as usize).min(domainname.len());
    let name = &domainname[..len];
    if name.first().is_none_or(|&c| c == 0) || name.contains(&b'/') {
        return Err(Errno::EAFNOSUPPORT);
    }

    match uap.r#type.get() {
        SOCK_STREAM | SOCK_DGRAM => {}
        _ => return Err(Errno::EAFNOSUPPORT),
    }

    if p.process().ps_flags.load(Ordering::Relaxed) & PS_CHROOT != 0 {
        return Err(Errno::EACCES);
    }

    // The binding file /var/yp/binding/<domainname>.2, its lock, the struct ypbinding in it
    // and the socket connected to the server: sockets are not ported.
    Err(unported!(
        "sys_ypconnect: the YP binding socket (socreate, soconnect: uipc_socket.c)"
    ))
}
