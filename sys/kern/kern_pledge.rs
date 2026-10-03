/*	$OpenBSD: kern_pledge.c,v 1.369 2026/09/21 00:46:13 jan Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 Nicholas Marriott <nicm@openbsd.org>
 * Copyright (c) 2015 Theo de Raadt <deraadt@openbsd.org>
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

//! `pledge(2)`: `kern/kern_pledge.c`.
//!
//! Upstream: sys/kern/kern_pledge.c @ 3ce1f3f79392
//!
//! Status: `wip`. The promise names (`pledgereq[]`, `pledgereq_flags`), their parsing
//! (`parsepledges`) and `sys_pledge`'s checks and bookkeeping are ported. The enforcement is
//! not: the per-system-call table (`pledge_syscalls[]`) with `pledge_syscall` and
//! `pledge_fail`, and the checks the rest of the kernel calls (`pledge_namei`,
//! `pledge_ioctl`, `pledge_sysctl`, `pledge_sockopt`, `pledge_socket`, `pledge_recvfd`,
//! `pledge_sendfd`, `pledge_chown`, `pledge_adjtime`, `pledge_sendit`, `pledge_flock`,
//! `pledge_swapctl`, `pledge_fcntl`, `pledge_kill`, `pledge_protexec`, `checkpledgepaths`,
//! `checkzoneinfopath`); their callers report them for a pledged process.
//!
//! ## Deviations
//! - `sys_pledge` records the promises in `ps_pledge`/`ps_execpledge` (and `p_pledge`) but
//!   does not set `PS_PLEDGE`/`PS_EXECPLEDGE`: with the flag set every check above would be
//!   needed, and without them a pledged program (`init(8)`, `ksh(1)`) could not run.
//!   Consequently the "only permit reductions" checks, which the C makes only once
//!   `PS_PLEDGE` is set, never refuse, and `execve` does not hand execpromises over. The
//!   unveil clean-up (`unveil_destroy`) is `kern_unveil.c`'s, not ported; no process has
//!   unveiled paths.

use core::sync::atomic::Ordering;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sig::{single_thread_clear, single_thread_set};
use crate::machine::copy::copyinstr;
use crate::sys::errno::Errno;
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::*;
use crate::sys::proc::{PS_EXECPLEDGE, PS_PLEDGE, Proc, SINGLE_UNWIND};
use crate::sys::syscallargs::SysPledgeArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;

/// `pledgereq[]`: the promise names and their flags, sorted by name (`pledgereq_flags`
/// searches it by bisection).
const PLEDGEREQ: &[(&[u8], u64)] = &[
    (b"audio", PLEDGE_AUDIO),
    (b"bpf", PLEDGE_BPF),
    (b"chown", PLEDGE_CHOWN | PLEDGE_CHOWNUID),
    (b"cpath", PLEDGE_CPATH),
    (b"disklabel", PLEDGE_DISKLABEL),
    (b"dns", PLEDGE_DNS),
    (b"dpath", PLEDGE_DPATH),
    (b"drm", PLEDGE_DRM),
    (b"error", PLEDGE_ERROR),
    (b"exec", PLEDGE_EXEC),
    (b"fattr", PLEDGE_FATTR | PLEDGE_CHOWN),
    (b"flock", PLEDGE_FLOCK),
    (b"getpw", PLEDGE_GETPW),
    (b"id", PLEDGE_ID),
    (b"inet", PLEDGE_INET),
    (b"mcast", PLEDGE_MCAST),
    (b"pf", PLEDGE_PF),
    (b"proc", PLEDGE_PROC),
    (b"prot_exec", PLEDGE_PROTEXEC),
    (b"ps", PLEDGE_PS),
    (b"recvfd", PLEDGE_RECVFD),
    (b"route", PLEDGE_ROUTE),
    (b"rpath", PLEDGE_RPATH),
    (b"sendfd", PLEDGE_SENDFD),
    (b"settime", PLEDGE_SETTIME),
    (b"stdio", PLEDGE_STDIO),
    (b"tape", PLEDGE_TAPE),
    (b"tty", PLEDGE_TTY),
    (b"unix", PLEDGE_UNIX),
    (b"unveil", PLEDGE_UNVEIL),
    (b"video", PLEDGE_VIDEO),
    (b"vminfo", PLEDGE_VMINFO),
    (b"vmm", PLEDGE_VMM),
    (b"wpath", PLEDGE_WPATH),
    (b"wroute", PLEDGE_WROUTE),
];

/// `parsepledges`: the flags of a space-separated promise string at the user address
/// `promises`; `EINVAL` for an unknown promise.
pub fn parsepledges(_p: &Proc, _kname: &str, promises: usize) -> Result<u64, Errno> {
    let mut rbuf = [0u8; MAXPATHLEN];
    let rbuflen = copyinstr(promises, &mut rbuf)?;
    // KTRACE: not configured.

    let mut flags = 0u64;
    for name in rbuf[..rbuflen.saturating_sub(1)]
        .split(|&c| c == b' ')
        .filter(|name| !name.is_empty())
    {
        let f = pledgereq_flags(name);
        if f == 0 {
            return Err(Errno::EINVAL);
        }
        flags |= f;
    }
    Ok(flags)
}

/// `pledge(2)` (see the module's deviations).
pub fn sys_pledge(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPledgeArgs = sysargs(v);
    let pr = p.process();
    let mut promises = 0u64;
    let mut execpromises = 0u64;
    let mut unveil_cleanup = false;
    let upromises = uap.promises.get() as usize;
    let uexecpromises = uap.execpromises.get() as usize;

    // Check for any error in user input
    if upromises != 0 {
        promises = parsepledges(p, "pledgereq", upromises)?;
    }
    if uexecpromises != 0 {
        execpromises = parsepledges(p, "pledgeexecreq", uexecpromises)?;
    }

    mtx_enter(&pr.ps_mtx);
    let error = 'fail: {
        let flags = pr.ps_flags.load(Ordering::Relaxed);

        // Check for any error wrt current promises
        if upromises != 0 {
            // In "error" mode, ignore promise increase requests, but accept promise
            // decrease requests
            if flags & PS_PLEDGE != 0 && pr.ps_pledge.get() & PLEDGE_ERROR != 0 {
                promises &= pr.ps_pledge.get() & PLEDGE_USERSET;
            }

            // Only permit reductions
            if flags & PS_PLEDGE != 0 && (promises | pr.ps_pledge.get()) != pr.ps_pledge.get() {
                break 'fail Err(Errno::EPERM);
            }
        }
        if uexecpromises != 0 {
            // Only permit reductions
            if flags & PS_EXECPLEDGE != 0
                && (execpromises | pr.ps_execpledge.get()) != pr.ps_execpledge.get()
            {
                break 'fail Err(Errno::EPERM);
            }
        }

        // Set up promises: recorded, PS_PLEDGE not set (see the module's deviations).
        if upromises != 0 {
            pr.ps_pledge.set(promises);
            p.p_pledge.set(promises);

            if pr.ps_pledge.get()
                & (PLEDGE_RPATH
                    | PLEDGE_WPATH
                    | PLEDGE_CPATH
                    | PLEDGE_DPATH
                    | PLEDGE_EXEC
                    | PLEDGE_UNIX
                    | PLEDGE_UNVEIL)
                == 0
            {
                unveil_cleanup = true;
            }
        }
        if uexecpromises != 0 {
            pr.ps_execpledge.set(execpromises);
        }
        Ok(())
    };
    mtx_leave(&pr.ps_mtx);

    if unveil_cleanup {
        // Kill off unveil and drop unveil vnode refs if we no longer are holding any
        // path-accessing pledge. This must be done single-threaded, because another thread
        // may be in a system call sleeping in namei().
        let _ = single_thread_set(p, SINGLE_UNWIND);
        // KERNEL_LOCK(); unveil_destroy(pr): kern_unveil.c, not ported (no process has
        // unveiled paths); KERNEL_UNLOCK().
        single_thread_clear(p);
    }
    error
}

/// `pledgereq_flags`: the flags of the promise `req_name`, 0 when there is no such promise.
pub fn pledgereq_flags(req_name: &[u8]) -> u64 {
    match PLEDGEREQ.binary_search_by(|(name, _)| (*name).cmp(req_name)) {
        Ok(i) => PLEDGEREQ[i].1,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promise_names() {
        assert!(
            PLEDGEREQ.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted for bsearch"
        );
        assert_eq!(pledgereq_flags(b"stdio"), PLEDGE_STDIO);
        assert_eq!(pledgereq_flags(b"fattr"), PLEDGE_FATTR | PLEDGE_CHOWN);
        assert_eq!(pledgereq_flags(b"wroute"), PLEDGE_WROUTE);
        assert_eq!(pledgereq_flags(b"audio"), PLEDGE_AUDIO);
        assert_eq!(pledgereq_flags(b"nope"), 0);
        assert_eq!(pledgereq_flags(b""), 0);
    }
}
