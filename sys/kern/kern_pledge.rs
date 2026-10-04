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
//! A process that called `pledge(2)` has `PS_PLEDGE` set and its promises in `ps_pledge`
//! (copied per thread into `p_pledge` at every system call). `mi_syscall` asks
//! [`pledge_syscall`] whether the call is covered by a promise (the `pledge_syscalls[]`
//! table); the rest of the kernel asks the narrower checks here (`pledge_namei`,
//! `pledge_ioctl`, `pledge_sysctl`, `pledge_sockopt`, ...). A violation ends in
//! [`pledge_fail`]: a line on the controlling terminal, `APLEDGE` in the accounting flags and
//! an uncatchable `SIGABRT`, or `ENOSYS` under the "error" promise.
//!
//! ## Deviations
//! - `pledge_fail` returns the `Errno` itself (it never succeeds); callers write
//!   `Err(pledge_fail(..))`. `checkzoneinfopath` returns a `bool` (true for an acceptable
//!   path) for the C's 0/-1.
//! - `KTRACE` is not configured: `parsepledges` and `pledge_fail` record nothing.
//! - Promise classes for devices this kernel does not configure are compiled out as the C's
//!   `#if` does with a zero count: `NAUDIO`, `NVIDEO`, `NDRM` (`pledge_ioctl_drm`), `NVMM`
//!   and `NPSP` (`pledge_ioctl_psp`).
//! - `net/frame.h` (`AF_FRAME`) is not ported: no such protocol exists and the "mcast"
//!   `FRAME_*_MEMBERSHIP` arm of `pledge_sockopt` is left out.
//! - `pledge_sockopt`'s `af_inet` (0, `AF_INET` or `AF_INET6`) is an `i32`; the C's
//!   fall-through from the `AF_INET`/`AF_INET6` cases into `AF_UNIX`'s `TCP_NODELAY` test is
//!   one condition. The C's IPv6 arms and the `*_IN6` ioctls are not under `#ifdef INET6`
//!   there, so they are compiled whatever the `inet6` feature says.
//! - `dev/diskmap.c` is not ported: no device switch entry has `diskmapioctl`, so the
//!   "disklabel" `DIOCMAP` case never allows.
//! - The `#ifdef CPU_CHR2BLK`, `CPU_SSE`, `CPU_ID_AA64ISAR0` and `CPU_ID_AA64ISAR1` tests of
//!   `pledge_sysctl` read the machine's `<machine/cpu.h>` names through `crate::machine`
//!   (`None` where the machine does not define them).
//! - `SMALL_KERNEL` is not configured: `checkpledgepaths` and `pledge_sysctl` are built.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::dev::biovar::{BIOCDISK, BIOCINQ, BIOCINSTALLBOOT, BIOCVOL};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_prot::groupmember;
use crate::kern::kern_sig::{sigabort, single_thread_clear, single_thread_set};
use crate::kern::subr_prf::{Str, panic, uprintf};
use crate::kern::tty_pty::{ptcopen, ptmopen};
use crate::kern::vfs_vnops::vn_ioctl;
use crate::machine::conf::{bdevsw, cdevsw};
use crate::machine::copy::copyinstr;
use crate::machine::cpu::{CPU_CHR2BLK, CPU_ID_AA64ISAR0, CPU_ID_AA64ISAR1, CPU_SSE};
use crate::net::bpf::{BIOCGSTATS, bpfopen};
use crate::net::pf_ioctl::pfopen;
use crate::net::pfvar::{
    DIOCADDRULE, DIOCGETSTATUS, DIOCKILLSRCNODES, DIOCNATLOOK, DIOCRADDTABLES, DIOCRCLRADDRS,
    DIOCRCLRTABLES, DIOCRCLRTSTATS, DIOCRGETTSTATS, DIOCRSETADDRS, DIOCXBEGIN, DIOCXCOMMIT,
};
use crate::net::route::RTF_LLINFO;
use crate::netinet::in_::{
    IP_ADD_MEMBERSHIP, IP_DROP_MEMBERSHIP, IP_IPDEFTTL, IP_MINTTL, IP_MULTICAST_IF,
    IP_MULTICAST_LOOP, IP_MULTICAST_TTL, IP_OPTIONS, IP_PORTRANGE, IP_RECVDSTADDR, IP_RECVDSTPORT,
    IP_TOS, IP_TTL, IPPROTO_IP, IPPROTO_IPV6, IPPROTO_TCP,
};
use crate::netinet::tcp::{
    TCP_INFO, TCP_MAXSEG, TCP_MD5SIG, TCP_NODELAY, TCP_NOPUSH, TCP_SACK_ENABLE,
};
use crate::netinet6::in6::{
    IPV6_DONTFRAG, IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP, IPV6_MINHOPCOUNT, IPV6_MULTICAST_HOPS,
    IPV6_MULTICAST_IF, IPV6_MULTICAST_LOOP, IPV6_PORTRANGE, IPV6_RECVDSTPORT, IPV6_RECVHOPLIMIT,
    IPV6_RECVPKTINFO, IPV6_RECVTCLASS, IPV6_TCLASS, IPV6_UNICAST_HOPS, IPV6_USE_MIN_MTU,
    IPV6_V6ONLY,
};
use crate::netinet6::in6_var::{
    SIOCAIFADDR_IN6, SIOCDIFADDR_IN6, SIOCGIFAFLAG_IN6, SIOCGIFALIFETIME_IN6, SIOCGIFDSTADDR_IN6,
    SIOCGIFINFO_IN6, SIOCGIFNETMASK_IN6, SIOCGNBRINFO_IN6,
};
use crate::sys::acct::APLEDGE;
use crate::sys::conf::{D_DISK, DevTypeOpen};
use crate::sys::dkio::{DIOCGDINFO, DIOCGPDINFO, DIOCMAP, DIOCRLDINFO, DIOCWDINFO};
use crate::sys::errno::Errno;
use crate::sys::fcntl::F_SETOWN;
use crate::sys::file::{DTYPE_DMABUF, DTYPE_PIPE, DTYPE_SOCKET, DTYPE_SYNC, DTYPE_VNODE, File};
use crate::sys::filio::{FIOCLEX, FIONBIO, FIONCLEX, FIONREAD};
use crate::sys::mman::PROT_EXEC;
use crate::sys::mount::{VFS_BCACHESTAT, VFS_GENERIC};
use crate::sys::mtio::{MTIOCGET, MTIOCTOP};
use crate::sys::namei::{BPU_LOCALTIME, BPU_ZONEINFO, BYPASSUNVEIL, Nameidata, UNVEIL_PLEDGEOPEN};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::*;
use crate::sys::proc::{
    PS_COREDUMP, PS_EXECPLEDGE, PS_PLEDGE, Proc, SINGLE_DEEP, SINGLE_UNWIND, p_hassibling,
};
use crate::sys::protosw::Protosw;
use crate::sys::socket::{
    AF_INET, AF_INET6, AF_UNIX, NET_RT_DUMP, NET_RT_FLAGS, NET_RT_IFLIST, NET_RT_IFNAMES,
    NET_RT_SOURCE, NET_RT_TABLE, PF_INET6, PF_ROUTE, SO_ERROR, SO_RCVBUF, SO_RTABLE, SO_TIMESTAMP,
    SOL_SOCKET,
};
use crate::sys::socketvar::SS_DNS;
use crate::sys::sockio::{
    SIOCAIFADDR, SIOCATMARK, SIOCDIFADDR, SIOCGIFADDR, SIOCGIFDATA, SIOCGIFDESCR, SIOCGIFFLAGS,
    SIOCGIFGMEMB, SIOCGIFGROUP, SIOCGIFMEDIA, SIOCGIFMETRIC, SIOCGIFRDOMAIN, SIOCGIFXFLAGS,
    SIOCSIFMTU,
};
use crate::sys::swap::{SWAP_NSWAP, SWAP_STATS};
use crate::sys::syscall::*;
use crate::sys::syscallargs::SysPledgeArgs;
use crate::sys::sysctl::{
    CTL_HW, CTL_KERN, CTL_MACHDEP, CTL_NET, CTL_VFS, CTL_VM, HW_MACHINE, HW_NCPU, HW_NCPUONLINE,
    HW_PAGESIZE, HW_PHYSMEM64, HW_SENSORS, HW_USERMEM64, KERN_ARGMAX, KERN_AUTOCONF_SERIAL,
    KERN_BOOTTIME, KERN_CCPU, KERN_CLOCKRATE, KERN_CONSDEV, KERN_CPTIME, KERN_CPTIME2,
    KERN_CPUSTATS, KERN_DOMAINNAME, KERN_FSCALE, KERN_HOSTNAME, KERN_MAXPARTITIONS, KERN_NGROUPS,
    KERN_OSRELEASE, KERN_OSTYPE, KERN_OSVERSION, KERN_POSIX1, KERN_PROC, KERN_PROC_ARGS,
    KERN_PROC_ARGV, KERN_PROC_CWD, KERN_PROC_ENV, KERN_RAWPARTITION, KERN_SOMAXCONN, KERN_SYSVSHM,
    KERN_VERSION,
};
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::tty::PTMGET;
use crate::sys::ttycom::{
    TIOCCBRK, TIOCCDTR, TIOCEXCL, TIOCEXT, TIOCFLUSH, TIOCGETA, TIOCGPGRP, TIOCGWINSZ, TIOCSBRK,
    TIOCSCTTY, TIOCSDTR, TIOCSETA, TIOCSETAF, TIOCSETAW, TIOCSPGRP, TIOCSTART, TIOCSTAT,
    TIOCSWINSZ, TIOCUCNTL,
};
use crate::sys::types::{Gid, Pid, Register, Uid, major};
use crate::sys::vnode::{VBAD, VBLK, VCHR, VDIR, VISTTY, Vnode};
use crate::uvm::uvmexp::{VM_LOADAVG, VM_MALLOC_CONF, VM_MAXSLP, VM_PSSTRINGS, VM_UVMEXP};

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

/// `PLEDGEPATH_NULL`: `/dev/null`.
const PLEDGEPATH_NULL: i32 = 1;
/// `PLEDGEPATH_TTY`: `/dev/tty`.
const PLEDGEPATH_TTY: i32 = 2;
/// `PLEDGEPATH_SPWD`: `/etc/spwd.db`.
const PLEDGEPATH_SPWD: i32 = 3;
/// `PLEDGEPATH_PWD`: `/etc/pwd.db`.
const PLEDGEPATH_PWD: i32 = 4;
/// `PLEDGEPATH_GROUP`: `/etc/group`.
const PLEDGEPATH_GROUP: i32 = 5;
/// `PLEDGEPATH_NETID`: `/etc/netid`.
const PLEDGEPATH_NETID: i32 = 6;
/// `PLEDGEPATH_RESOLVCONF`: `/etc/resolv.conf`.
const PLEDGEPATH_RESOLVCONF: i32 = 7;
/// `PLEDGEPATH_HOSTS`: `/etc/hosts`.
const PLEDGEPATH_HOSTS: i32 = 8;
/// `PLEDGEPATH_SERVICES`: `/etc/services`.
const PLEDGEPATH_SERVICES: i32 = 9;
/// `PLEDGEPATH_PROTOCOLS`: `/etc/protocols`.
const PLEDGEPATH_PROTOCOLS: i32 = 10;
/// `PLEDGEPATH_LOCALTIME`: `/etc/localtime`.
const PLEDGEPATH_LOCALTIME: i32 = 11;
/// `PLEDGEPATH_ZONEINFO`: a file under `/usr/share/zoneinfo/`, manually parsed
/// (`checkzoneinfopath`).
const PLEDGEPATH_ZONEINFO: i32 = 12;

/// `pledgepaths[]`: the paths `__pledge_open` may open, sorted by name
/// (`checkpledgepaths` bisects).
const PLEDGEPATHS: &[(&[u8], i32)] = &[
    (b"/dev/null", PLEDGEPATH_NULL),
    (b"/dev/tty", PLEDGEPATH_TTY),
    (b"/etc/group", PLEDGEPATH_GROUP),
    (b"/etc/hosts", PLEDGEPATH_HOSTS),
    (b"/etc/localtime", PLEDGEPATH_LOCALTIME),
    (b"/etc/netid", PLEDGEPATH_NETID),
    (b"/etc/protocols", PLEDGEPATH_PROTOCOLS),
    (b"/etc/pwd.db", PLEDGEPATH_PWD),
    (b"/etc/resolv.conf", PLEDGEPATH_RESOLVCONF),
    (b"/etc/services", PLEDGEPATH_SERVICES),
    (b"/etc/spwd.db", PLEDGEPATH_SPWD),
];

/// The prefix `checkzoneinfopath` accepts.
const ZONEINFO: &[u8] = b"/usr/share/zoneinfo/";

/// `pledge_syscalls[]`: the promises that cover each system call, indexed by number; 0 for
/// a call no promise allows, `PLEDGE_ALWAYS` for one every pledged process may make.
/// Ordered in blocks starting with least risky and most required.
static PLEDGE_SYSCALLS: [u64; SYS_MAXSYSCALL] = {
    let mut t = [0u64; SYS_MAXSYSCALL];

    // Minimum required
    t[SYS_exit as usize] = PLEDGE_ALWAYS;
    t[SYS_kbind as usize] = PLEDGE_ALWAYS;
    t[SYS___get_tcb as usize] = PLEDGE_ALWAYS;
    t[SYS___set_tcb as usize] = PLEDGE_ALWAYS;
    t[SYS_pledge as usize] = PLEDGE_ALWAYS;
    t[SYS_sendsyslog as usize] = PLEDGE_ALWAYS; // stack protector reporting
    t[SYS_thrkill as usize] = PLEDGE_ALWAYS; // raise, abort, stack pro
    t[SYS_utrace as usize] = PLEDGE_ALWAYS; // ltrace(1) from ld.so
    t[SYS_pinsyscalls as usize] = PLEDGE_ALWAYS;

    // "getting" information about self is considered safe
    t[SYS_getuid as usize] = PLEDGE_STDIO;
    t[SYS_geteuid as usize] = PLEDGE_STDIO;
    t[SYS_getresuid as usize] = PLEDGE_STDIO;
    t[SYS_getgid as usize] = PLEDGE_STDIO;
    t[SYS_getegid as usize] = PLEDGE_STDIO;
    t[SYS_getresgid as usize] = PLEDGE_STDIO;
    t[SYS_getgroups as usize] = PLEDGE_STDIO;
    t[SYS_getlogin_r as usize] = PLEDGE_STDIO;
    t[SYS_getpgrp as usize] = PLEDGE_STDIO;
    t[SYS_getpgid as usize] = PLEDGE_STDIO;
    t[SYS_getppid as usize] = PLEDGE_STDIO;
    t[SYS_getsid as usize] = PLEDGE_STDIO;
    t[SYS_getthrid as usize] = PLEDGE_STDIO;
    t[SYS_getrlimit as usize] = PLEDGE_STDIO;
    t[SYS_getrtable as usize] = PLEDGE_STDIO;
    t[SYS_gettimeofday as usize] = PLEDGE_STDIO;
    t[SYS_getdtablecount as usize] = PLEDGE_STDIO;
    t[SYS_getrusage as usize] = PLEDGE_STDIO;
    t[SYS_issetugid as usize] = PLEDGE_STDIO;
    t[SYS_clock_getres as usize] = PLEDGE_STDIO;
    t[SYS_clock_gettime as usize] = PLEDGE_STDIO;
    t[SYS_getpid as usize] = PLEDGE_STDIO;

    // Almost exclusively read-only, Very narrow subset.
    // Use of "route", "inet", "dns", "ps", or "vminfo"
    // expands access.
    t[SYS_sysctl as usize] = PLEDGE_STDIO;

    // Only available to programs compiled -pg
    t[SYS_profil as usize] = PLEDGE_STDIO;

    // Support for malloc(3) family of operations
    t[SYS_getentropy as usize] = PLEDGE_STDIO;
    t[SYS_madvise as usize] = PLEDGE_STDIO;
    t[SYS_minherit as usize] = PLEDGE_STDIO;
    t[SYS_mmap as usize] = PLEDGE_STDIO;
    t[SYS_mprotect as usize] = PLEDGE_STDIO;
    t[SYS_mimmutable as usize] = PLEDGE_STDIO;
    t[SYS_mquery as usize] = PLEDGE_STDIO;
    t[SYS_munmap as usize] = PLEDGE_STDIO;
    t[SYS_msync as usize] = PLEDGE_STDIO;
    t[SYS_break as usize] = PLEDGE_STDIO;

    t[SYS_umask as usize] = PLEDGE_STDIO;

    // read/write operations
    t[SYS_read as usize] = PLEDGE_STDIO;
    t[SYS_readv as usize] = PLEDGE_STDIO;
    t[SYS_pread as usize] = PLEDGE_STDIO;
    t[SYS_preadv as usize] = PLEDGE_STDIO;
    t[SYS_write as usize] = PLEDGE_STDIO;
    t[SYS_writev as usize] = PLEDGE_STDIO;
    t[SYS_pwrite as usize] = PLEDGE_STDIO;
    t[SYS_pwritev as usize] = PLEDGE_STDIO;
    t[SYS_recvmsg as usize] = PLEDGE_STDIO;
    t[SYS_recvmmsg as usize] = PLEDGE_STDIO;
    t[SYS_recvfrom as usize] = PLEDGE_STDIO;
    t[SYS_ftruncate as usize] = PLEDGE_STDIO;
    t[SYS_lseek as usize] = PLEDGE_STDIO;
    t[SYS_fpathconf as usize] = PLEDGE_STDIO;

    // Address selection required a network pledge ("inet",
    // "unix", "dns".
    t[SYS_sendto as usize] = PLEDGE_STDIO;

    // Address specification required a network pledge ("inet",
    // "unix", "dns".  SCM_RIGHTS requires "sendfd" or "recvfd".
    t[SYS_sendmsg as usize] = PLEDGE_STDIO;
    t[SYS_sendmmsg as usize] = PLEDGE_STDIO;

    // Common signal operations
    t[SYS_nanosleep as usize] = PLEDGE_STDIO;
    t[SYS_sigaltstack as usize] = PLEDGE_STDIO;
    t[SYS_sigprocmask as usize] = PLEDGE_STDIO;
    t[SYS_sigsuspend as usize] = PLEDGE_STDIO;
    t[SYS_sigaction as usize] = PLEDGE_STDIO;
    t[SYS_sigreturn as usize] = PLEDGE_STDIO;
    t[SYS_sigpending as usize] = PLEDGE_STDIO;
    t[SYS_getitimer as usize] = PLEDGE_STDIO;
    t[SYS_setitimer as usize] = PLEDGE_STDIO;

    // To support event driven programming.
    t[SYS_poll as usize] = PLEDGE_STDIO;
    t[SYS_ppoll as usize] = PLEDGE_STDIO;
    t[SYS_kevent as usize] = PLEDGE_STDIO;
    t[SYS_kqueue as usize] = PLEDGE_STDIO;
    t[SYS_kqueue1 as usize] = PLEDGE_STDIO;
    t[SYS_select as usize] = PLEDGE_STDIO;
    t[SYS_pselect as usize] = PLEDGE_STDIO;

    t[SYS_fstat as usize] = PLEDGE_STDIO;
    t[SYS_fsync as usize] = PLEDGE_STDIO;

    t[SYS_setsockopt as usize] = PLEDGE_STDIO; // narrow whitelist
    t[SYS_getsockopt as usize] = PLEDGE_STDIO; // narrow whitelist

    // F_SETOWN requires PLEDGE_PROC
    t[SYS_fcntl as usize] = PLEDGE_STDIO;

    t[SYS_close as usize] = PLEDGE_STDIO;
    t[SYS_dup as usize] = PLEDGE_STDIO;
    t[SYS_dup2 as usize] = PLEDGE_STDIO;
    t[SYS_dup3 as usize] = PLEDGE_STDIO;
    t[SYS_closefrom as usize] = PLEDGE_STDIO;
    t[SYS_shutdown as usize] = PLEDGE_STDIO;
    t[SYS_fchdir as usize] = PLEDGE_STDIO;

    t[SYS_pipe as usize] = PLEDGE_STDIO;
    t[SYS_pipe2 as usize] = PLEDGE_STDIO;
    t[SYS_socketpair as usize] = PLEDGE_STDIO;

    t[SYS_wait4 as usize] = PLEDGE_STDIO;
    t[SYS_waitid as usize] = PLEDGE_STDIO;

    // Can kill self with "stdio".  Killing another pid/pgid
    // requires "proc"
    t[SYS_kill as usize] = PLEDGE_STDIO;

    // FIONREAD/FIONBIO for "stdio"
    // Other ioctl are selectively allowed based upon other pledges.
    t[SYS_ioctl as usize] = PLEDGE_STDIO;

    // Path access/creation calls encounter many extensive
    // checks done during pledge_namei()
    t[SYS_open as usize] = PLEDGE_RPATH | PLEDGE_WPATH;
    t[SYS___pledge_open as usize] = PLEDGE_STDIO;
    t[SYS_stat as usize] = PLEDGE_RPATH;
    t[SYS_access as usize] = PLEDGE_RPATH;
    t[SYS_readlink as usize] = PLEDGE_RPATH;
    t[SYS___realpath as usize] = PLEDGE_RPATH;

    t[SYS_adjtime as usize] = PLEDGE_STDIO; // setting requires "settime"
    t[SYS_adjfreq as usize] = PLEDGE_SETTIME;
    t[SYS_settimeofday as usize] = PLEDGE_SETTIME;

    // Needed by threaded programs
    t[SYS___tfork as usize] = PLEDGE_STDIO;
    t[SYS_sched_yield as usize] = PLEDGE_STDIO;
    t[SYS_futex as usize] = PLEDGE_STDIO;
    t[SYS___thrsleep as usize] = PLEDGE_STDIO;
    t[SYS___thrwakeup as usize] = PLEDGE_STDIO;
    t[SYS___threxit as usize] = PLEDGE_STDIO;
    t[SYS___thrsigdivert as usize] = PLEDGE_STDIO;
    t[SYS_getthrname as usize] = PLEDGE_STDIO;
    t[SYS_setthrname as usize] = PLEDGE_STDIO;

    t[SYS_fork as usize] = PLEDGE_PROC;
    t[SYS_vfork as usize] = PLEDGE_PROC;
    t[SYS_setpgid as usize] = PLEDGE_PROC;
    t[SYS_setsid as usize] = PLEDGE_PROC;

    t[SYS_setrlimit as usize] = PLEDGE_PROC | PLEDGE_ID;
    t[SYS_getpriority as usize] = PLEDGE_PROC | PLEDGE_ID;

    t[SYS_setpriority as usize] = PLEDGE_PROC | PLEDGE_ID;

    t[SYS_setuid as usize] = PLEDGE_ID;
    t[SYS_seteuid as usize] = PLEDGE_ID;
    t[SYS_setreuid as usize] = PLEDGE_ID;
    t[SYS_setresuid as usize] = PLEDGE_ID;
    t[SYS_setgid as usize] = PLEDGE_ID;
    t[SYS_setegid as usize] = PLEDGE_ID;
    t[SYS_setregid as usize] = PLEDGE_ID;
    t[SYS_setresgid as usize] = PLEDGE_ID;
    t[SYS_setgroups as usize] = PLEDGE_ID;
    t[SYS_setlogin as usize] = PLEDGE_ID;
    t[SYS_setrtable as usize] = PLEDGE_ID;

    t[SYS_unveil as usize] = PLEDGE_UNVEIL;

    t[SYS_execve as usize] = PLEDGE_EXEC;

    t[SYS_chdir as usize] = PLEDGE_RPATH;
    t[SYS_openat as usize] = PLEDGE_RPATH | PLEDGE_WPATH;
    t[SYS_fstatat as usize] = PLEDGE_RPATH;
    t[SYS_faccessat as usize] = PLEDGE_RPATH;
    t[SYS_readlinkat as usize] = PLEDGE_RPATH;
    t[SYS_lstat as usize] = PLEDGE_RPATH;
    t[SYS_truncate as usize] = PLEDGE_WPATH;
    t[SYS_rename as usize] = PLEDGE_RPATH | PLEDGE_CPATH;
    t[SYS_rmdir as usize] = PLEDGE_CPATH;
    t[SYS_renameat as usize] = PLEDGE_CPATH;
    t[SYS_link as usize] = PLEDGE_CPATH;
    t[SYS_linkat as usize] = PLEDGE_CPATH;
    t[SYS_symlink as usize] = PLEDGE_CPATH;
    t[SYS_symlinkat as usize] = PLEDGE_CPATH;
    t[SYS_unlink as usize] = PLEDGE_CPATH;
    t[SYS_unlinkat as usize] = PLEDGE_CPATH;
    t[SYS_mkdir as usize] = PLEDGE_CPATH;
    t[SYS_mkdirat as usize] = PLEDGE_CPATH;

    t[SYS_mkfifo as usize] = PLEDGE_DPATH;
    t[SYS_mkfifoat as usize] = PLEDGE_DPATH;
    t[SYS_mknod as usize] = PLEDGE_DPATH;
    t[SYS_mknodat as usize] = PLEDGE_DPATH;

    t[SYS_revoke as usize] = PLEDGE_TTY; // also requires PLEDGE_RPATH

    t[SYS___getcwd as usize] = PLEDGE_RPATH;

    // Classify as RPATH, because these leak path information
    t[SYS_getdents as usize] = PLEDGE_RPATH;
    t[SYS_getfsstat as usize] = PLEDGE_RPATH;
    t[SYS_statfs as usize] = PLEDGE_RPATH;
    t[SYS_fstatfs as usize] = PLEDGE_RPATH;
    t[SYS_pathconf as usize] = PLEDGE_RPATH;
    t[SYS_pathconfat as usize] = PLEDGE_RPATH;

    t[SYS_utimes as usize] = PLEDGE_FATTR;
    t[SYS_futimes as usize] = PLEDGE_FATTR;
    t[SYS_utimensat as usize] = PLEDGE_FATTR;
    t[SYS_futimens as usize] = PLEDGE_FATTR;
    t[SYS_chmod as usize] = PLEDGE_FATTR;
    t[SYS_fchmod as usize] = PLEDGE_FATTR;
    t[SYS_fchmodat as usize] = PLEDGE_FATTR;
    t[SYS_chflags as usize] = PLEDGE_FATTR;
    t[SYS_chflagsat as usize] = PLEDGE_FATTR;
    t[SYS_fchflags as usize] = PLEDGE_FATTR;

    t[SYS_chown as usize] = PLEDGE_CHOWN;
    t[SYS_fchownat as usize] = PLEDGE_CHOWN;
    t[SYS_lchown as usize] = PLEDGE_CHOWN;
    t[SYS_fchown as usize] = PLEDGE_CHOWN;

    t[SYS_socket as usize] = PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS;
    t[SYS_connect as usize] = PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS;
    t[SYS_bind as usize] = PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS;
    t[SYS_getsockname as usize] = PLEDGE_STDIO;

    t[SYS_listen as usize] = PLEDGE_INET | PLEDGE_UNIX;
    t[SYS_accept4 as usize] = PLEDGE_INET | PLEDGE_UNIX;
    t[SYS_accept as usize] = PLEDGE_INET | PLEDGE_UNIX;
    t[SYS_getpeername as usize] = PLEDGE_STDIO;

    t[SYS_flock as usize] = PLEDGE_FLOCK;

    t[SYS_ypconnect as usize] = PLEDGE_GETPW;

    t[SYS_swapctl as usize] = PLEDGE_VMINFO;

    // for sysarch(*_SYNC_ICACHE) requests only
    t[SYS_sysarch as usize] = PLEDGE_PROTEXEC;

    t
};

/// `parsepledges`: the flags of the space-separated promise string at the user address
/// `promises`; `EINVAL` for an unknown promise.
pub fn parsepledges(_p: &Proc, _kname: &str, promises: usize) -> Result<u64, Errno> {
    let mut rbuf = [0u8; MAXPATHLEN];
    let rbuflen = copyinstr(promises, &mut rbuf)?;
    // KTRACE: not configured.
    parsepledges_buf(&rbuf[..rbuflen.saturating_sub(1)])
}

/// The scan of `parsepledges` over the promise string `rbuf` (without its NUL).
fn parsepledges_buf(rbuf: &[u8]) -> Result<u64, Errno> {
    let mut flags = 0u64;
    for name in rbuf.split(|&c| c == b' ').filter(|name| !name.is_empty()) {
        let f = pledgereq_flags(name);
        if f == 0 {
            return Err(Errno::EINVAL);
        }
        flags |= f;
    }
    Ok(flags)
}

/// `pledge(2)`: restrict the process to `promises` and, across `execve`, to `execpromises`
/// (a NULL one is left as it is). Promises can only be reduced.
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

        // Set up promises
        if upromises != 0 {
            pr.ps_pledge.set(promises);
            pr.ps_flags.fetch_or(PS_PLEDGE, Ordering::SeqCst);

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
            pr.ps_flags.fetch_or(PS_EXECPLEDGE, Ordering::SeqCst);
        }
        Ok(())
    };
    mtx_leave(&pr.ps_mtx);

    if unveil_cleanup {
        // Kill off unveil and drop unveil vnode refs if we no longer are holding any
        // path-accessing pledge. This must be done single-threaded, because another thread
        // may be in a system call sleeping in namei().
        let _ = single_thread_set(p, SINGLE_UNWIND);
        // KERNEL_LOCK()/KERNEL_UNLOCK(): nothing without MULTIPROCESSOR.
        crate::kern::kern_unveil::unveil_destroy(pr);
        single_thread_clear(p);
    }
    error
}

/// `pledge_syscall`: whether the pledged thread `p` may make system call `code`. On
/// `EPERM`, `*tval` holds the promises that would have allowed it (for `pledge_fail`).
pub fn pledge_syscall(p: &Proc, code: i32, tval: &mut u64) -> Result<(), Errno> {
    p.p_pledge_syscall.set(code);
    *tval = 0;

    let Some(&need) = usize::try_from(code)
        .ok()
        .and_then(|c| PLEDGE_SYSCALLS.get(c))
    else {
        return Err(Errno::EINVAL);
    };

    if need == PLEDGE_ALWAYS {
        return Ok(());
    }

    // pledge checks are per-thread
    p.p_pledge.set(p.process().ps_pledge.get());
    if p.p_pledge.get() & need != 0 {
        return Ok(());
    }

    *tval = need;
    Err(Errno::EPERM)
}

/// `pledge_fail`: the pledged thread `p` broke its promises (`code`: the promises that would
/// have allowed what it did, 0 when none would). Under "error" the answer is `ENOSYS`;
/// otherwise the process is reported on its terminal, stopped and sent an uncatchable
/// `SIGABRT`, and the answer is `error`.
pub fn pledge_fail(p: &Proc, error: Errno, code: u64) -> Errno {
    let pr = p.process();

    // Print first matching pledge
    let codes: &[u8] = if code == 0 {
        b""
    } else {
        PLEDGENAMES
            .iter()
            .find(|&&(bits, _)| bits & code != 0)
            .map_or(b"", |&(_, name)| name)
    };
    // KTRACE: not configured.

    if p.p_pledge.get() & PLEDGE_ERROR != 0 {
        return Errno::ENOSYS;
    }

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    uprintf(format_args!(
        "{}[{}]: pledge \"{}\", syscall {}\n",
        Str(pr.comm()),
        pr.ps_pid.get(),
        Str(codes),
        p.p_pledge_syscall.get()
    ));
    pr.ps_acflag.set(pr.ps_acflag.get() | APLEDGE);

    // Try to stop threads immediately, because this process is suspect
    if p_hassibling(p) {
        let _ = single_thread_set(p, SINGLE_UNWIND | SINGLE_DEEP);
    }

    // Send uncatchable SIGABRT for coredump
    sigabort(p);

    pr.ps_pledge.set(0); // Disable all PLEDGE_ flags
    // KERNEL_UNLOCK()
    error
}

/// `checkpledgepaths`: the `PLEDGEPATH_*` item of `path` (without its NUL), 0 when it is
/// not one of `pledgepaths[]`.
pub fn checkpledgepaths(path: &[u8]) -> i32 {
    match PLEDGEPATHS.binary_search_by(|&(name, _)| name.cmp(path)) {
        Ok(i) => PLEDGEPATHS[i].1,
        Err(_) => 0,
    }
}

/// `checkzoneinfopath`: whether `path` (without its NUL) names something under
/// `/usr/share/zoneinfo/` with no `..` component; `true` for the C's 0, `false` for its -1.
pub fn checkzoneinfopath(path: &[u8]) -> bool {
    if !path.starts_with(ZONEINFO) {
        return false;
    }
    // The scan starts at the prefix's own trailing '/', so a first component ".." is caught.
    let tail = &path[ZONEINFO.len() - 1..];
    !(0..tail.len())
        .any(|i| tail[i..].starts_with(b"/..") && matches!(tail.get(i + 3), None | Some(b'/')))
}

/// `pledge_namei`: whether the pledged thread `p` may look up `path` (the pathname buffer,
/// without its NUL) for what `ni.ni_pledge` says. A `__pledge_open` of a whitelisted path
/// may set `BYPASSUNVEIL` (and `BPU_LOCALTIME`/`BPU_ZONEINFO`) in `ni`.
///
/// Need to make it more obvious that one cannot get through here without the right flags
/// set.
pub fn pledge_namei(p: &Proc, ni: &mut Nameidata<'_>, path: &[u8]) -> Result<(), Errno> {
    let flags = p.process().ps_flags.load(Ordering::Relaxed);
    if flags & PS_PLEDGE == 0 || flags & PS_COREDUMP != 0 {
        return Ok(());
    }
    let ple = p.p_pledge.get();
    let nip = ni.ni_pledge;

    if nip == 0 {
        return Err(pledge_fail(p, Errno::EPERM, 0));
    }

    // Doing a permitted execve()
    if nip & PLEDGE_EXEC != 0 && ple & PLEDGE_EXEC != 0 {
        return Ok(());
    }

    // In specific promise situations, __pledge_open() can open specific paths and ignores
    // rpath, wpath, or unveil restrictions. Using visibility rules, only libc calls
    // __pledge_open(). In most cases the file descriptor returned is used only a short
    // moment of time and then closed. The file descriptors are marked UF_PLEDGEOPEN and
    // various operations are prohibited.
    if ni.ni_unveil & UNVEIL_PLEDGEOPEN != 0 {
        let mut item = checkpledgepaths(path);
        if item == 0 && checkzoneinfopath(path) {
            item = PLEDGEPATH_ZONEINFO;
        }
        let rw_only = nip & !(PLEDGE_RPATH | PLEDGE_WPATH) == 0;
        let cn_flags = &mut ni.ni_cnd.cn_flags;
        match item {
            // Invalid path provided to __pledge_open
            0 => return Err(pledge_fail(p, Errno::EACCES, nip & !ple)),
            // "stdio" - for daemon(3) or other such functions
            PLEDGEPATH_NULL => {
                if rw_only {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            // "tty" - readpassphrase(3), getpass(3)
            PLEDGEPATH_TTY => {
                if ple & PLEDGE_TTY != 0 && rw_only {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            // "getpw" requirements
            PLEDGEPATH_SPWD => {
                // XXX should remove nip check!
                if ple & PLEDGE_GETPW != 0 && nip == PLEDGE_RPATH {
                    return Err(Errno::EPERM);
                }
            }
            PLEDGEPATH_PWD | PLEDGEPATH_GROUP | PLEDGEPATH_NETID => {
                if ple & PLEDGE_GETPW != 0 && nip == PLEDGE_RPATH {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            // "dns" requirements
            PLEDGEPATH_RESOLVCONF
            | PLEDGEPATH_HOSTS
            | PLEDGEPATH_SERVICES
            | PLEDGEPATH_PROTOCOLS => {
                if ple & PLEDGE_DNS != 0 && nip == PLEDGE_RPATH {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            // tzset() often happen late in programs
            PLEDGEPATH_LOCALTIME => {
                *cn_flags |= BPU_LOCALTIME;
                if nip == PLEDGE_RPATH {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            PLEDGEPATH_ZONEINFO => {
                *cn_flags |= BPU_ZONEINFO;
                if nip == PLEDGE_RPATH {
                    *cn_flags |= BYPASSUNVEIL;
                }
            }
            _ => panic(format_args!("pledgepaths table is broken")),
        }
    }

    if ni.ni_cnd.cn_flags & BYPASSUNVEIL != 0 {
        return Ok(());
    }

    // Ensure each flag of ni_pledge has counterpart allowing it in p_pledge.
    if nip & !ple != 0 {
        return Err(pledge_fail(p, Errno::EPERM, nip & !ple));
    }

    // continue into namei() which will check unveils
    Ok(())
}

/// The descriptors `pledge_recvfd` and `pledge_sendfd` let through: sockets, pipes, DMA
/// buffers, sync files and vnodes other than directories.
fn pledge_safefd(fp: &File) -> bool {
    match fp.f_type.get() {
        DTYPE_SOCKET | DTYPE_PIPE | DTYPE_DMABUF | DTYPE_SYNC => true,
        DTYPE_VNODE => fp.vnode().v_type.get() != VDIR,
        _ => false,
    }
}

/// `pledge_recvfd`: only allow reception of safe file descriptors.
pub fn pledge_recvfd(p: &Proc, fp: &File) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    if p.p_pledge.get() & PLEDGE_RECVFD == 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_RECVFD));
    }

    if pledge_safefd(fp) {
        return Ok(());
    }
    Err(Errno::EPERM)
}

/// `pledge_sendfd`: only allow sending of safe file descriptors.
pub fn pledge_sendfd(p: &Proc, fp: &File) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    if p.p_pledge.get() & PLEDGE_SENDFD == 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_SENDFD));
    }

    if pledge_safefd(fp) {
        return Ok(());
    }
    Err(pledge_fail(p, Errno::EINVAL, PLEDGE_SENDFD))
}

/// `pledge_sysctl`: the narrow set of `sysctl(2)` names a pledged thread may read (never
/// write: a `new` value is `EFAULT`), widened by "route", "ps", "vminfo", "inet", "unix",
/// "dns" and "disklabel".
pub fn pledge_sysctl(p: &Proc, mib: &[i32], new: usize) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    let pledge = p.p_pledge.get();

    if new != 0 {
        return Err(pledge_fail(p, Errno::EFAULT, 0));
    }

    let miblen = mib.len();
    let at = |i: usize| mib.get(i).copied().unwrap_or(0);
    let net_route = |len: usize| miblen == len && at(0) == CTL_NET && at(1) == PF_ROUTE as i32;
    let rt_af = || {
        let af = at(3);
        af == 0 || af == AF_INET6 as i32 || af == AF_INET as i32
    };

    // routing table observation
    if pledge & PLEDGE_ROUTE != 0 {
        if (net_route(6) || net_route(7)) && at(2) == 0 && at(4) == NET_RT_DUMP {
            return Ok(());
        }
        if net_route(6)
            && at(2) == 0
            && rt_af()
            && (at(4) == NET_RT_TABLE || at(4) == NET_RT_SOURCE)
        {
            return Ok(());
        }
        // exposes MACs
        if net_route(7)
            && at(2) == 0
            && rt_af()
            && at(4) == NET_RT_FLAGS
            && at(5) == RTF_LLINFO as i32
        {
            return Ok(());
        }
    }

    let kern2 = |name: i32| miblen == 2 && at(0) == CTL_KERN && at(1) == name;
    if pledge & (PLEDGE_PS | PLEDGE_VMINFO) != 0 {
        if kern2(KERN_FSCALE) || kern2(KERN_BOOTTIME) || kern2(KERN_CONSDEV) || kern2(KERN_CPTIME) {
            return Ok(());
        }
        // kern.cptime2, kern.cpustats
        if miblen == 3 && at(0) == CTL_KERN && (at(1) == KERN_CPTIME2 || at(1) == KERN_CPUSTATS) {
            return Ok(());
        }
    }

    if pledge & PLEDGE_PS != 0 {
        // kern.procargs.*
        if miblen == 4
            && at(0) == CTL_KERN
            && at(1) == KERN_PROC_ARGS
            && (at(3) == KERN_PROC_ARGV || at(3) == KERN_PROC_ENV)
        {
            return Ok(());
        }
        // kern.proc.*
        if miblen == 6 && at(0) == CTL_KERN && at(1) == KERN_PROC {
            return Ok(());
        }
        // kern.proc_cwd.*
        if miblen == 3 && at(0) == CTL_KERN && at(1) == KERN_PROC_CWD {
            return Ok(());
        }
        // kern.ccpu
        if kern2(KERN_CCPU) {
            return Ok(());
        }
        // vm.maxslp
        if miblen == 2 && at(0) == CTL_VM && at(1) == VM_MAXSLP {
            return Ok(());
        }
    }

    if pledge & PLEDGE_VMINFO != 0 {
        // vm.uvmexp
        if miblen == 2 && at(0) == CTL_VM && at(1) == VM_UVMEXP {
            return Ok(());
        }
        // vfs.generic.bcachestat
        if miblen == 3 && at(0) == CTL_VFS && at(1) == VFS_GENERIC && at(2) == VFS_BCACHESTAT {
            return Ok(());
        }
        // for sysconf(3)
        if miblen == 3 && at(0) == CTL_NET && at(1) == PF_INET6 as i32 {
            return Ok(());
        }
    }

    // kern.somaxconn
    if pledge & (PLEDGE_INET | PLEDGE_UNIX) != 0 && kern2(KERN_SOMAXCONN) {
        return Ok(());
    }

    // getifaddrs()
    if pledge & (PLEDGE_ROUTE | PLEDGE_INET | PLEDGE_DNS) != 0
        && net_route(6)
        && at(2) == 0
        && rt_af()
        && at(4) == NET_RT_IFLIST
    {
        return Ok(());
    }

    if pledge & PLEDGE_DISKLABEL != 0 {
        // kern.rawpartition, kern.maxpartitions
        if kern2(KERN_RAWPARTITION) || kern2(KERN_MAXPARTITIONS) {
            return Ok(());
        }
        // machdep.chr2blk
        if let Some(chr2blk) = CPU_CHR2BLK
            && miblen == 3
            && at(0) == CTL_MACHDEP
            && at(1) == chr2blk
        {
            return Ok(());
        }
    }

    // ntpd(8) to read sensors
    if miblen >= 3 && at(0) == CTL_HW && at(1) == HW_SENSORS {
        return Ok(());
    }

    // if_nameindex()
    if net_route(6) && at(2) == 0 && at(3) == 0 && at(4) == NET_RT_IFNAMES {
        return Ok(());
    }

    if miblen == 2 {
        match at(0) {
            CTL_KERN => match at(1) {
                KERN_DOMAINNAME      // getdomainname()
                | KERN_HOSTNAME      // gethostname()
                | KERN_OSTYPE        // uname()
                | KERN_OSRELEASE     // uname()
                | KERN_OSVERSION     // uname()
                | KERN_VERSION       // uname()
                | KERN_CLOCKRATE     // kern.clockrate
                | KERN_ARGMAX        // kern.argmax
                | KERN_NGROUPS       // kern.ngroups
                | KERN_SYSVSHM       // kern.sysvshm
                | KERN_POSIX1        // kern.posix1version
                | KERN_AUTOCONF_SERIAL => return Ok(()), // kern.autoconf_serial
                _ => {}
            },
            CTL_HW => match at(1) {
                HW_MACHINE          // uname()
                | HW_PAGESIZE       // getpagesize()
                | HW_PHYSMEM64      // hw.physmem
                | HW_NCPU           // hw.ncpu
                | HW_NCPUONLINE     // hw.ncpuonline
                | HW_USERMEM64 => return Ok(()), // hw.usermem
                _ => {}
            },
            CTL_VM => match at(1) {
                VM_PSSTRINGS        // setproctitle()
                | VM_LOADAVG        // vm.loadavg / getloadavg(3)
                | VM_MALLOC_CONF => return Ok(()), // vm.malloc_conf
                _ => {}
            },
            _ => {}
        }
    }

    // i386 libm tests for SSE; arm64 libcrypto inspects CPU features
    for name in [CPU_SSE, CPU_ID_AA64ISAR0, CPU_ID_AA64ISAR1]
        .into_iter()
        .flatten()
    {
        if miblen == 2 && at(0) == CTL_MACHDEP && at(1) == name {
            return Ok(());
        }
    }

    Err(pledge_fail(p, Errno::EINVAL, 0))
}

/// `pledge_chown`: without "chown", a pledged thread may only give a file to itself and to
/// a group it is in (`uid`/`gid` -1 change nothing).
pub fn pledge_chown(p: &Proc, uid: Uid, gid: Gid) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_CHOWNUID != 0 {
        return Ok(());
    }

    if uid != Uid::MAX && uid != p.ucred().cr_uid.get() {
        return Err(Errno::EPERM);
    }
    if gid != Gid::MAX && !groupmember(gid, p.ucred()) {
        return Err(Errno::EPERM);
    }
    Ok(())
}

/// `pledge_adjtime`: a pledged process may change the time only with "settime"; reading
/// it (`delta` NULL) is always allowed.
pub fn pledge_adjtime(p: &Proc, delta: usize) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_SETTIME != 0 {
        return Ok(());
    }
    if delta != 0 {
        return Err(Errno::EPERM);
    }
    Ok(())
}

/// `pledge_sendit`: a send with a destination address `to` needs a network promise.
pub fn pledge_sendit(p: &Proc, to: *const c_void) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & (PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS) != 0 {
        return Ok(()); // may use address
    }
    if to.is_null() {
        return Ok(()); // behaves just like write
    }
    Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET))
}

/// Whether the character device of vnode `vp` is the driver whose open routine is `open`
/// (`cdevsw[major(vp->v_rdev)].d_open == open`).
fn cdev_is(vp: &Vnode, open: DevTypeOpen) -> bool {
    vp.v_type.get() == VCHR && ptr::fn_addr_eq(cdevsw(major(vp.v_rdev())).d_open, open)
}

/// `pledge_ioctl`: the ioctls a pledged thread may issue on `fp`: `FIONREAD`, `FIONBIO`,
/// `FIOCLEX` and `FIONCLEX` always, the others by promise class and descriptor kind.
pub fn pledge_ioctl(p: &Proc, com: u64, fp: &File) -> Result<(), Errno> {
    let error = Errno::EPERM;

    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    let pledge = p.p_pledge.get();

    // The ioctl's which are always allowed.
    if matches!(com, FIONREAD | FIONBIO | FIOCLEX | FIONCLEX) {
        return Ok(());
    }

    // fp != NULL was already checked
    let is_socket = fp.f_type.get() == DTYPE_SOCKET;
    let vp = if fp.f_type.get() == DTYPE_VNODE {
        let vp = fp.vnode();
        if vp.v_type.get() == VBAD {
            return Err(Errno::ENOTTY);
        }
        Some(vp)
    } else {
        None
    };
    let vchr = vp.filter(|vp| vp.v_type.get() == VCHR);
    let istty = vp.is_some_and(|vp| vp.v_flag.get() & VISTTY != 0);

    if pledge & PLEDGE_INET != 0 && matches!(com, SIOCATMARK | SIOCGIFGROUP) && is_socket {
        return Ok(());
    }

    // NBPFILTER > 0
    if pledge & PLEDGE_BPF != 0
        && com == BIOCGSTATS
        // bpf: tcpdump privsep on ^C
        && ptr::fn_addr_eq(
            fp.ops().fo_ioctl,
            vn_ioctl as fn(&File, u64, &mut [u8], &Proc) -> Result<(), Errno>,
        )
        && vchr.is_some_and(|vp| cdev_is(vp, bpfopen as DevTypeOpen))
    {
        return Ok(());
    }

    if pledge & PLEDGE_TAPE != 0 && matches!(com, MTIOCGET | MTIOCTOP) {
        // for pax(1) and such, checking tapes...
        if let Some(vp) = vchr {
            if vp.v_flag.get() & VISTTY != 0 {
                return Err(Errno::ENOTTY);
            }
            return Ok(());
        }
    }

    // NDRM, NAUDIO: 0 (see the deviations).

    if pledge & PLEDGE_DISKLABEL != 0 {
        match com {
            DIOCGDINFO | DIOCGPDINFO | DIOCRLDINFO | DIOCWDINFO | BIOCDISK | BIOCINQ
            | BIOCINSTALLBOOT | BIOCVOL => {
                if let Some(vp) = vp {
                    let maj = major(vp.v_rdev());
                    if (vp.v_type.get() == VCHR && cdevsw(maj).d_type == D_DISK)
                        || (vp.v_type.get() == VBLK && bdevsw(maj).d_type == D_DISK)
                    {
                        return Ok(());
                    }
                }
            }
            // cdevsw[major(vp->v_rdev)].d_ioctl == diskmapioctl: dev/diskmap.c is not
            // ported, no entry has it (see the deviations).
            DIOCMAP => {}
            _ => {}
        }
    }

    // NVIDEO: 0 (see the deviations).

    // NPF > 0
    if pledge & PLEDGE_PF != 0
        && matches!(
            com,
            DIOCADDRULE
                | DIOCGETSTATUS
                | DIOCNATLOOK
                | DIOCRADDTABLES
                | DIOCRCLRADDRS
                | DIOCRCLRTABLES
                | DIOCRCLRTSTATS
                | DIOCRGETTSTATS
                | DIOCRSETADDRS
                | DIOCXBEGIN
                | DIOCXCOMMIT
                | DIOCKILLSRCNODES
        )
        && vchr.is_some_and(|vp| cdev_is(vp, pfopen as DevTypeOpen))
    {
        return Ok(());
    }

    if pledge & PLEDGE_TTY != 0 {
        let rwpath = pledge & PLEDGE_RPATH != 0 && pledge & PLEDGE_WPATH != 0;
        match com {
            // NPTY > 0
            PTMGET if rwpath && vchr.is_some_and(|vp| cdev_is(vp, ptmopen as DevTypeOpen)) => {
                return Ok(());
            }
            // vmd
            TIOCUCNTL
                if rwpath && vchr.is_some_and(|vp| cdev_is(vp, ptcopen as DevTypeOpen)) =>
            {
                return Ok(());
            }
            TIOCSPGRP | TIOCFLUSH | TIOCSTART | TIOCGPGRP | TIOCGETA | TIOCGWINSZ | TIOCSTAT
                if com != TIOCSPGRP || pledge & PLEDGE_PROC != 0 =>
            {
                // getty, telnet (TIOCFLUSH); emacs, etc (TIOCSTART); ENOTTY return for
                // non-tty (TIOCGWINSZ); csh (TIOCSTAT)
                if istty {
                    return Ok(());
                }
                return Err(Errno::ENOTTY);
            }
            TIOCSWINSZ
            | TIOCEXT   // mail, libedit ..
            | TIOCCBRK  // cu
            | TIOCSBRK  // cu
            | TIOCCDTR  // cu
            | TIOCSDTR  // cu
            | TIOCEXCL  // cu
            | TIOCSETA  // cu, ...
            | TIOCSETAW // cu, ...
            | TIOCSETAF // tcsetattr TCSAFLUSH, script
            | TIOCSCTTY // forkpty(3), login_tty(3), ...
                if istty =>
            {
                return Ok(());
            }
            _ => {}
        }
    }

    if pledge & PLEDGE_ROUTE != 0
        && matches!(
            com,
            SIOCGIFADDR
                | SIOCGIFAFLAG_IN6
                | SIOCGIFALIFETIME_IN6
                | SIOCGIFDATA
                | SIOCGIFDESCR
                | SIOCGIFFLAGS
                | SIOCGIFMETRIC
                | SIOCGIFGMEMB
                | SIOCGIFRDOMAIN
                | SIOCGIFDSTADDR_IN6
                | SIOCGIFNETMASK_IN6
                | SIOCGIFXFLAGS
                | SIOCGNBRINFO_IN6
                | SIOCGIFINFO_IN6
                | SIOCGIFMEDIA
        )
        && is_socket
    {
        return Ok(());
    }

    if pledge & PLEDGE_WROUTE != 0
        && matches!(
            com,
            SIOCAIFADDR | SIOCDIFADDR | SIOCAIFADDR_IN6 | SIOCDIFADDR_IN6 | SIOCSIFMTU
        )
        && is_socket
    {
        return Ok(());
    }

    // NVMM, NPSP: 0 (see the deviations).

    Err(pledge_fail(p, error, PLEDGE_TTY))
}

/// `pledge_sockopt`: the socket options a pledged thread may get (`set` false) or set on a
/// socket of protocol `pr`.
pub fn pledge_sockopt(
    p: &Proc,
    set: bool,
    pr: &Protosw,
    level: i32,
    optname: i32,
) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    let pledge = p.p_pledge.get();

    // the meaning of level and optname is scoped to the protocol handler, which in turn is
    // scoped by an address family. there are exceptions though.
    //
    // optnames at the SOL_SOCKET level apply regardless of the address family and protocol,
    // so those variables are ignored for that level.
    //
    // similarly, an address family may have a level that applies to all protocols, eg,
    // optnames at the level of IPPROTO_IP in the the AF_INET family apply to all protocols.
    //
    // some protocols are implemented in multiple address families. eg, the IPPROTO_TCP
    // protocol and it's associated IPPROTO_TCP level operates under both the AF_INET and
    // AF_INET6 address families, and should be handled for both.
    let af = pr.pr_domain.dom_family;
    let proto = i32::from(pr.pr_protocol);
    // AF_INET or AF_INET6, 0 for the other families.
    let af_inet = if af == AF_INET as i32 || af == AF_INET6 as i32 {
        af
    } else {
        0
    };

    // Always allow these, which are too common to reject
    if level == SOL_SOCKET && matches!(optname, SO_RCVBUF | SO_ERROR) {
        return Ok(());
    }

    // some software assumes all streams are tcp (AF_UNIX)
    if (af_inet != 0 || af == AF_UNIX as i32) && level == IPPROTO_TCP && optname == TCP_NODELAY {
        return Ok(());
    }

    if af == AF_INET as i32 && level == IPPROTO_IP && optname == IP_TOS {
        return Ok(());
    }
    if af == AF_INET6 as i32 {
        match level {
            IPPROTO_IPV6 if optname == IPV6_TCLASS => return Ok(()),
            // Lots of software tries IPPROTO_IP / IP_TOS on v6 sockets
            IPPROTO_IP if optname == IP_TOS => return Ok(()),
            _ => {}
        }
    }

    if pledge & PLEDGE_WROUTE != 0 && level == SOL_SOCKET && optname == SO_RTABLE {
        return Ok(());
    }

    // PLEDGE_MCAST over AF_FRAME (FRAME_ADD_MEMBERSHIP, FRAME_DEL_MEMBERSHIP): see the
    // deviations.

    if pledge & (PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS) == 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET));
    }
    // In use by some service libraries
    if level == SOL_SOCKET && optname == SO_TIMESTAMP {
        return Ok(());
    }

    // DNS resolver may do these requests
    if pledge & PLEDGE_DNS != 0
        && af == AF_INET6 as i32
        && level == IPPROTO_IPV6
        && matches!(optname, IPV6_RECVPKTINFO | IPV6_USE_MIN_MTU)
    {
        return Ok(());
    }

    if pledge & (PLEDGE_INET | PLEDGE_UNIX) == 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET));
    }
    if level == SOL_SOCKET {
        if optname == SO_RTABLE {
            return Err(pledge_fail(p, Errno::EINVAL, PLEDGE_WROUTE));
        }
        return Ok(());
    }

    if pledge & PLEDGE_INET == 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET));
    }
    if af_inet == 0 {
        // af must be AF_INET or AF_INET6 after this point
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET));
    }

    if proto == IPPROTO_TCP
        && level == IPPROTO_TCP
        && matches!(
            optname,
            TCP_MD5SIG | TCP_SACK_ENABLE | TCP_MAXSEG | TCP_NOPUSH | TCP_INFO
        )
    {
        return Ok(());
    }

    if af_inet == AF_INET as i32 && level == IPPROTO_IP {
        match optname {
            IP_OPTIONS if !set => return Ok(()),
            IP_TTL | IP_MINTTL | IP_IPDEFTTL | IP_PORTRANGE | IP_RECVDSTADDR | IP_RECVDSTPORT => {
                return Ok(());
            }
            IP_MULTICAST_IF | IP_MULTICAST_TTL | IP_MULTICAST_LOOP | IP_ADD_MEMBERSHIP
            | IP_DROP_MEMBERSHIP
                if pledge & PLEDGE_MCAST != 0 =>
            {
                return Ok(());
            }
            _ => {}
        }
    } else if af_inet == AF_INET6 as i32 && level == IPPROTO_IPV6 {
        match optname {
            IPV6_DONTFRAG | IPV6_UNICAST_HOPS | IPV6_MINHOPCOUNT | IPV6_RECVHOPLIMIT
            | IPV6_PORTRANGE | IPV6_RECVPKTINFO | IPV6_RECVDSTPORT | IPV6_RECVTCLASS
            | IPV6_V6ONLY => return Ok(()),
            IPV6_MULTICAST_IF | IPV6_MULTICAST_HOPS | IPV6_MULTICAST_LOOP | IPV6_JOIN_GROUP
            | IPV6_LEAVE_GROUP
                if pledge & PLEDGE_MCAST != 0 =>
            {
                return Ok(());
            }
            _ => {}
        }
    }

    Err(pledge_fail(p, Errno::EPERM, PLEDGE_INET))
}

/// `pledge_socket`: a socket of `domain` (-1: any, for `accept`) or one created with
/// `SOCK_DNS` (`state` has `SS_DNS`) needs the matching promise.
pub fn pledge_socket(p: &Proc, domain: i32, state: u32) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    let pledge = p.p_pledge.get();

    if state & SS_DNS != 0 {
        if pledge & PLEDGE_DNS != 0 {
            return Ok(());
        }
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_DNS));
    }

    let need = match domain {
        -1 => return Ok(()), // accept on any domain
        d if d == AF_INET as i32 || d == AF_INET6 as i32 => PLEDGE_INET,
        d if d == AF_UNIX as i32 => PLEDGE_UNIX,
        _ => return Err(pledge_fail(p, Errno::EINVAL, PLEDGE_INET)),
    };
    if pledge & need != 0 {
        return Ok(());
    }
    Err(pledge_fail(p, Errno::EPERM, need))
}

/// `pledge_flock`: file locking needs "flock".
pub fn pledge_flock(p: &Proc) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_FLOCK != 0 {
        return Ok(());
    }
    Err(pledge_fail(p, Errno::EPERM, PLEDGE_FLOCK))
}

/// `pledge_swapctl`: "vminfo" may only count and list the swap devices.
pub fn pledge_swapctl(p: &Proc, cmd: i32) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_VMINFO != 0 && matches!(cmd, SWAP_NSWAP | SWAP_STATS) {
        return Ok(());
    }

    Err(pledge_fail(p, Errno::EPERM, PLEDGE_VMINFO))
}

/// `pledgereq_flags`: the flags of the promise `req_name`, 0 when there is no such promise.
pub fn pledgereq_flags(req_name: &[u8]) -> u64 {
    match PLEDGEREQ.binary_search_by(|(name, _)| (*name).cmp(req_name)) {
        Ok(i) => PLEDGEREQ[i].1,
        Err(_) => 0,
    }
}

/// `pledge_fcntl`: `F_SETOWN` needs "proc".
pub fn pledge_fcntl(p: &Proc, cmd: i32) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_PROC == 0 && cmd == F_SETOWN {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_PROC));
    }

    Ok(())
}

/// `pledge_kill`: without "proc" a pledged thread may only signal its own process (`pid` 0
/// or its own).
pub fn pledge_kill(p: &Proc, pid: Pid) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_PROC != 0 {
        return Ok(());
    }

    if pid == 0 || pid == p.process().ps_pid.get() {
        return Ok(());
    }

    Err(pledge_fail(p, Errno::EPERM, PLEDGE_PROC))
}

/// `pledge_protexec`: `PROT_EXEC` mappings need "prot_exec" once `kbind(2)` has run.
pub fn pledge_protexec(p: &Proc, prot: i32) -> Result<(), Errno> {
    let pr = p.process();
    if pr.ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }

    // Before kbind(2) call, ld.so and crt may create EXEC mappings
    if pr.ps_kbind_addr.get() == 0 && pr.ps_kbind_cookie.get() == 0 {
        return Ok(());
    }

    if p.p_pledge.get() & PLEDGE_PROTEXEC == 0 && prot & PROT_EXEC != 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_PROTEXEC));
    }

    Ok(())
}

#[cfg(test)]
mod tests;
