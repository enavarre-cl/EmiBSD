//! Host tests for `kern_pledge.c`: the promise names and their parsing, `sys_pledge`'s
//! reductions and "error" mode, the system call table, the `__pledge_open` whitelist of
//! `pledge_namei` (with `checkpledgepaths` and `checkzoneinfopath`), a few of the narrow
//! checks, and the IPv6 socket options and interface ioctls. Every pledged test thread also
//! has "error", so a violation answers `ENOSYS` instead of sending the (host) thread a
//! `SIGABRT`.

use std::boxed::Box;
use std::{assert, assert_eq};

use super::*;
use crate::kern::vfs_lookup::ndinit;
use crate::sys::namei::{LOOKUP, NiDirp};
use crate::sys::proc::Process;

/// A thread of a process pledged to `promises` (plus "error").
fn pledged(promises: u64) -> &'static Proc {
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_flags.fetch_or(PS_PLEDGE, Ordering::Relaxed);
    pr.ps_pledge
        .store(promises | PLEDGE_ERROR, Ordering::Relaxed);
    p.p_pledge.set(promises | PLEDGE_ERROR);
    p
}

/// An unpledged thread.
fn unpledged() -> &'static Proc {
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    p
}

#[test]
fn promise_names() {
    assert!(
        PLEDGEREQ.windows(2).all(|w| w[0].0 < w[1].0),
        "sorted for bsearch"
    );
    assert_eq!(pledgereq_flags(b"stdio"), PLEDGE_STDIO);
    assert_eq!(pledgereq_flags(b"fattr"), PLEDGE_FATTR | PLEDGE_CHOWN);
    assert_eq!(pledgereq_flags(b"chown"), PLEDGE_CHOWN | PLEDGE_CHOWNUID);
    assert_eq!(pledgereq_flags(b"wroute"), PLEDGE_WROUTE);
    assert_eq!(pledgereq_flags(b"audio"), PLEDGE_AUDIO);
    assert_eq!(pledgereq_flags(b"nope"), 0);
    assert_eq!(pledgereq_flags(b""), 0);
}

#[test]
fn promise_strings_parse() {
    assert_eq!(parsepledges_buf(b""), Ok(0));
    assert_eq!(parsepledges_buf(b"stdio"), Ok(PLEDGE_STDIO));
    assert_eq!(
        parsepledges_buf(b"  stdio   rpath getpw "),
        Ok(PLEDGE_STDIO | PLEDGE_RPATH | PLEDGE_GETPW)
    );
    assert_eq!(
        parsepledges_buf(b"stdio rpath wpath cpath proc exec"),
        Ok(PLEDGE_STDIO | PLEDGE_RPATH | PLEDGE_WPATH | PLEDGE_CPATH | PLEDGE_PROC | PLEDGE_EXEC)
    );
    assert_eq!(parsepledges_buf(b"stdio bogus"), Err(Errno::EINVAL));
    assert_eq!(parsepledges_buf(b"stdi"), Err(Errno::EINVAL));
}

/// `pledge(promises, execpromises)` through the system call.
fn pledge(
    p: &Proc,
    promises: Option<&core::ffi::CStr>,
    exec: Option<&core::ffi::CStr>,
) -> Result<(), Errno> {
    let addr = |s: Option<&core::ffi::CStr>| s.map_or(0, |s| s.as_ptr() as Register);
    let args: SysArgs = [addr(promises), addr(exec), 0, 0, 0, 0];
    let mut retval = [0; 2];
    sys_pledge(p, &args, &mut retval)
}

#[test]
fn pledge_only_reduces() {
    let p = unpledged();
    let pr = p.process();
    assert_eq!(
        pledge(p, Some(c"stdio rpath wpath"), Some(c"stdio rpath")),
        Ok(())
    );
    assert!(
        pr.ps_flags.load(Ordering::Relaxed) & (PS_PLEDGE | PS_EXECPLEDGE)
            == PS_PLEDGE | PS_EXECPLEDGE
    );
    assert_eq!(
        pr.ps_pledge.load(Ordering::Relaxed),
        PLEDGE_STDIO | PLEDGE_RPATH | PLEDGE_WPATH
    );
    assert_eq!(pr.ps_execpledge.get(), PLEDGE_STDIO | PLEDGE_RPATH);
    assert_eq!(
        pledge(p, Some(c"stdio rpath wpath cpath"), None),
        Err(Errno::EPERM)
    );
    assert_eq!(
        pledge(p, None, Some(c"stdio rpath wpath")),
        Err(Errno::EPERM)
    );
    assert_eq!(pledge(p, Some(c"stdio rpath"), None), Ok(()));
    assert_eq!(
        pr.ps_pledge.load(Ordering::Relaxed),
        PLEDGE_STDIO | PLEDGE_RPATH
    );
    assert_eq!(pledge(p, Some(c"stdio unknown"), None), Err(Errno::EINVAL));
    assert_eq!(
        pr.ps_pledge.load(Ordering::Relaxed),
        PLEDGE_STDIO | PLEDGE_RPATH
    );
}

#[test]
fn error_mode_ignores_increases() {
    let p = unpledged();
    let pr = p.process();
    assert_eq!(pledge(p, Some(c"stdio rpath error"), None), Ok(()));
    assert_eq!(pledge(p, Some(c"stdio rpath wpath error"), None), Ok(()));
    assert_eq!(
        pr.ps_pledge.load(Ordering::Relaxed),
        PLEDGE_STDIO | PLEDGE_RPATH | PLEDGE_ERROR
    );
}

#[test]
fn the_system_call_table() {
    let p = pledged(PLEDGE_STDIO);
    let mut tval = 0;
    assert_eq!(pledge_syscall(p, SYS_getpid, &mut tval), Ok(()));
    assert_eq!(pledge_syscall(p, SYS_exit, &mut tval), Ok(()));
    assert_eq!(pledge_syscall(p, SYS_open, &mut tval), Err(Errno::EPERM));
    assert_eq!(tval, PLEDGE_RPATH | PLEDGE_WPATH);
    assert_eq!(p.p_pledge_syscall.get(), SYS_open);
    assert_eq!(pledge_syscall(p, SYS_acct, &mut tval), Err(Errno::EPERM));
    assert_eq!(tval, 0, "no promise allows acct(2)");
    assert_eq!(pledge_syscall(p, -1, &mut tval), Err(Errno::EINVAL));
    assert_eq!(
        pledge_syscall(p, SYS_MAXSYSCALL as i32, &mut tval),
        Err(Errno::EINVAL)
    );
    // p_pledge is refreshed from the process at each call.
    p.process()
        .ps_pledge
        .store(PLEDGE_STDIO | PLEDGE_RPATH, Ordering::Relaxed);
    assert_eq!(pledge_syscall(p, SYS_open, &mut tval), Ok(()));
    assert_eq!(p.p_pledge.get(), PLEDGE_STDIO | PLEDGE_RPATH);
    assert_eq!(PLEDGE_SYSCALLS[SYS_kbind as usize], PLEDGE_ALWAYS);
    assert_eq!(PLEDGE_SYSCALLS[SYS_fork as usize], PLEDGE_PROC);
    assert_eq!(
        PLEDGE_SYSCALLS[SYS_socket as usize],
        PLEDGE_INET | PLEDGE_UNIX | PLEDGE_DNS
    );
    assert_eq!(PLEDGE_SYSCALLS[SYS_sysarch as usize], PLEDGE_PROTEXEC);
    assert_eq!(PLEDGE_SYSCALLS.iter().filter(|&&f| f != 0).count(), 190);
}

#[test]
fn whitelisted_paths() {
    assert!(
        PLEDGEPATHS.windows(2).all(|w| w[0].0 < w[1].0),
        "sorted for bsearch"
    );
    assert_eq!(checkpledgepaths(b"/etc/pwd.db"), PLEDGEPATH_PWD);
    assert_eq!(checkpledgepaths(b"/dev/null"), PLEDGEPATH_NULL);
    assert_eq!(checkpledgepaths(b"/etc/spwd.db"), PLEDGEPATH_SPWD);
    assert_eq!(checkpledgepaths(b"/etc/master.passwd"), 0);
    assert_eq!(checkpledgepaths(b"/etc/pwd.db/"), 0);
    assert!(checkzoneinfopath(b"/usr/share/zoneinfo/Europe/Madrid"));
    assert!(checkzoneinfopath(b"/usr/share/zoneinfo/..foo"));
    assert!(!checkzoneinfopath(
        b"/usr/share/zoneinfo/../../etc/master.passwd"
    ));
    assert!(!checkzoneinfopath(b"/usr/share/zoneinfo/posix/.."));
    assert!(!checkzoneinfopath(b"/usr/share/zoneinfo"));
    assert!(!checkzoneinfopath(b"/etc/localtime"));
}

/// `pledge_namei` for a lookup of `path` that needs `nip`, as `__pledge_open` when
/// `pledgeopen`; the result and the `cn_flags` it left.
fn namei_check(p: &Proc, path: &[u8], nip: u64, pledgeopen: bool) -> (Result<(), Errno>, u64) {
    let mut ni = ndinit(LOOKUP, 0, NiDirp::Sys(path), p);
    ni.ni_pledge = nip;
    if pledgeopen {
        ni.ni_unveil = UNVEIL_PLEDGEOPEN;
    }
    let r = pledge_namei(p, &mut ni, path);
    (r, ni.ni_cnd.cn_flags)
}

#[test]
fn pledge_open_whitelist() {
    let getpw = pledged(PLEDGE_STDIO | PLEDGE_GETPW);
    // ps(1): "getpw" reads pwd.db past its unveil, never spwd.db.
    assert_eq!(
        namei_check(getpw, b"/etc/pwd.db", PLEDGE_RPATH, true),
        (Ok(()), BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(getpw, b"/etc/group", PLEDGE_RPATH, true),
        (Ok(()), BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(getpw, b"/etc/spwd.db", PLEDGE_RPATH, true),
        (Err(Errno::EPERM), 0)
    );
    // Not for writing, not without __pledge_open, not without "getpw".
    assert_eq!(
        namei_check(getpw, b"/etc/pwd.db", PLEDGE_RPATH | PLEDGE_WPATH, true).0,
        Err(Errno::ENOSYS)
    );
    assert_eq!(
        namei_check(getpw, b"/etc/pwd.db", PLEDGE_RPATH, false),
        (Err(Errno::ENOSYS), 0)
    );
    let stdio = pledged(PLEDGE_STDIO);
    assert_eq!(
        namei_check(stdio, b"/etc/pwd.db", PLEDGE_RPATH, true),
        (Err(Errno::ENOSYS), 0)
    );
    // Paths off the list.
    assert_eq!(
        namei_check(getpw, b"/etc/master.passwd", PLEDGE_RPATH, true),
        (Err(Errno::ENOSYS), 0)
    );
    // "stdio": /dev/null; /dev/tty only with "tty".
    assert_eq!(
        namei_check(stdio, b"/dev/null", PLEDGE_RPATH | PLEDGE_WPATH, true),
        (Ok(()), BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(stdio, b"/dev/tty", PLEDGE_RPATH | PLEDGE_WPATH, true).0,
        Err(Errno::ENOSYS)
    );
    let tty = pledged(PLEDGE_STDIO | PLEDGE_TTY);
    assert_eq!(
        namei_check(tty, b"/dev/tty", PLEDGE_RPATH | PLEDGE_WPATH, true),
        (Ok(()), BYPASSUNVEIL)
    );
    // "dns".
    let dns = pledged(PLEDGE_STDIO | PLEDGE_DNS);
    assert_eq!(
        namei_check(dns, b"/etc/resolv.conf", PLEDGE_RPATH, true),
        (Ok(()), BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(stdio, b"/etc/hosts", PLEDGE_RPATH, true).0,
        Err(Errno::ENOSYS)
    );
    // tzset(3), with any promises.
    assert_eq!(
        namei_check(stdio, b"/etc/localtime", PLEDGE_RPATH, true),
        (Ok(()), BPU_LOCALTIME | BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(stdio, b"/usr/share/zoneinfo/UTC", PLEDGE_RPATH, true),
        (Ok(()), BPU_ZONEINFO | BYPASSUNVEIL)
    );
    assert_eq!(
        namei_check(
            stdio,
            b"/usr/share/zoneinfo/../../etc/spwd.db",
            PLEDGE_RPATH,
            true
        )
        .0,
        Err(Errno::ENOSYS)
    );
}

#[test]
fn plain_lookups_need_the_promise() {
    let rpath = pledged(PLEDGE_STDIO | PLEDGE_RPATH);
    assert_eq!(
        namei_check(rpath, b"/etc/motd", PLEDGE_RPATH, false),
        (Ok(()), 0)
    );
    assert_eq!(
        namei_check(rpath, b"/etc/motd", PLEDGE_RPATH | PLEDGE_WPATH, false).0,
        Err(Errno::ENOSYS)
    );
    assert_eq!(
        namei_check(rpath, b"/etc/motd", 0, false).0,
        Err(Errno::ENOSYS)
    );
    let exec = pledged(PLEDGE_STDIO | PLEDGE_EXEC);
    assert_eq!(
        namei_check(exec, b"/bin/ls", PLEDGE_EXEC, false),
        (Ok(()), 0)
    );
    assert_eq!(
        namei_check(unpledged(), b"/etc/spwd.db", 0, true),
        (Ok(()), 0)
    );
}

#[test]
fn narrow_checks() {
    let stdio = pledged(PLEDGE_STDIO);
    // sysctl(2)
    assert_eq!(pledge_sysctl(stdio, &[CTL_KERN, KERN_OSTYPE], 0), Ok(()));
    assert_eq!(pledge_sysctl(stdio, &[CTL_HW, HW_PAGESIZE], 0), Ok(()));
    assert_eq!(
        pledge_sysctl(stdio, &[CTL_KERN, KERN_OSTYPE], 1),
        Err(Errno::ENOSYS)
    );
    let kproc = [CTL_KERN, KERN_PROC, 0, 0, 0, 0];
    assert_eq!(pledge_sysctl(stdio, &kproc, 0), Err(Errno::ENOSYS));
    assert_eq!(
        pledge_sysctl(pledged(PLEDGE_STDIO | PLEDGE_PS), &kproc, 0),
        Ok(())
    );
    let iflist = [
        CTL_NET,
        PF_ROUTE as i32,
        0,
        AF_INET as i32,
        NET_RT_IFLIST,
        0,
    ];
    assert_eq!(pledge_sysctl(stdio, &iflist, 0), Err(Errno::ENOSYS));
    assert_eq!(
        pledge_sysctl(pledged(PLEDGE_STDIO | PLEDGE_INET), &iflist, 0),
        Ok(())
    );
    // socket(2), kill(2), fcntl(2), flock(2)
    assert_eq!(pledge_socket(stdio, AF_INET as i32, 0), Err(Errno::ENOSYS));
    assert_eq!(
        pledge_socket(pledged(PLEDGE_INET), AF_INET as i32, 0),
        Ok(())
    );
    assert_eq!(
        pledge_socket(pledged(PLEDGE_INET), AF_INET as i32, SS_DNS),
        Err(Errno::ENOSYS)
    );
    assert_eq!(pledge_socket(stdio, -1, 0), Ok(()));
    assert_eq!(pledge_kill(stdio, 0), Ok(()));
    assert_eq!(pledge_kill(stdio, 4242), Err(Errno::ENOSYS));
    assert_eq!(pledge_fcntl(stdio, F_SETOWN), Err(Errno::ENOSYS));
    assert_eq!(pledge_flock(stdio), Err(Errno::ENOSYS));
    assert_eq!(pledge_swapctl(pledged(PLEDGE_VMINFO), SWAP_NSWAP), Ok(()));
    assert_eq!(pledge_adjtime(stdio, 0), Ok(()));
    assert_eq!(pledge_adjtime(stdio, 8), Err(Errno::EPERM));
    assert_eq!(pledge_sendit(stdio, ptr::null()), Ok(()));
    let unp = unpledged();
    assert_eq!(pledge_kill(unp, 4242), Ok(()));
    assert_eq!(pledge_sysctl(unp, &kproc, 1), Ok(()));
}

/// A protocol of `domain` (`pr_protocol` = `proto`), as `pledge_sockopt` sees it.
fn proto_of(domain: &'static crate::sys::domain::Domain, proto: i32) -> &'static Protosw {
    Box::leak(Box::new(Protosw {
        pr_protocol: proto as i16,
        ..Protosw::new(domain)
    }))
}

#[test]
fn ipv6_socket_options() {
    use crate::netinet::in_::{IPPROTO_ICMPV6, IPPROTO_UDP};
    use crate::netinet6::in6::{ICMP6_FILTER, IPV6_PKTINFO, IPV6_RECVHOPOPTS};
    use crate::netinet6::in6_proto::INET6DOMAIN;
    let raw6 = proto_of(&INET6DOMAIN, IPPROTO_ICMPV6);
    let udp6 = proto_of(&INET6DOMAIN, IPPROTO_UDP);
    let tcp6 = proto_of(&INET6DOMAIN, IPPROTO_TCP);

    // ping6's "stdio inet dns": what it may still set or get once pledged.
    let ping6 = pledged(PLEDGE_STDIO | PLEDGE_INET | PLEDGE_DNS);
    for opt in [
        IPV6_RECVPKTINFO,
        IPV6_RECVHOPLIMIT,
        IPV6_UNICAST_HOPS,
        IPV6_TCLASS,
        IPV6_DONTFRAG,
        IPV6_V6ONLY,
    ] {
        assert_eq!(pledge_sockopt(ping6, true, raw6, IPPROTO_IPV6, opt), Ok(()));
    }
    // Lots of software tries IPPROTO_IP / IP_TOS on v6 sockets.
    assert_eq!(
        pledge_sockopt(ping6, true, udp6, IPPROTO_IP, IP_TOS),
        Ok(())
    );
    assert_eq!(
        pledge_sockopt(ping6, true, tcp6, IPPROTO_TCP, TCP_NODELAY),
        Ok(())
    );
    // Not in the lists: killed (ENOSYS in "error" mode), as on OpenBSD.
    for (level, opt) in [
        (IPPROTO_ICMPV6, ICMP6_FILTER),
        (IPPROTO_IPV6, IPV6_PKTINFO),
        (IPPROTO_IPV6, IPV6_RECVHOPOPTS),
        (IPPROTO_IP, IP_TTL),
    ] {
        assert_eq!(
            pledge_sockopt(ping6, true, raw6, level, opt),
            Err(Errno::ENOSYS)
        );
    }
    // Multicast options need "mcast".
    assert_eq!(
        pledge_sockopt(ping6, true, udp6, IPPROTO_IPV6, IPV6_JOIN_GROUP),
        Err(Errno::ENOSYS)
    );
    let mcast = pledged(PLEDGE_INET | PLEDGE_MCAST);
    for opt in [
        IPV6_MULTICAST_IF,
        IPV6_MULTICAST_HOPS,
        IPV6_MULTICAST_LOOP,
        IPV6_JOIN_GROUP,
        IPV6_LEAVE_GROUP,
    ] {
        assert_eq!(pledge_sockopt(mcast, true, udp6, IPPROTO_IPV6, opt), Ok(()));
    }
    // The DNS resolver's options with "dns" alone; the rest is "inet"'s.
    let dns = pledged(PLEDGE_STDIO | PLEDGE_DNS);
    assert_eq!(
        pledge_sockopt(dns, true, udp6, IPPROTO_IPV6, IPV6_USE_MIN_MTU),
        Ok(())
    );
    assert_eq!(
        pledge_sockopt(dns, true, udp6, IPPROTO_IPV6, IPV6_RECVPKTINFO),
        Ok(())
    );
    assert_eq!(
        pledge_sockopt(dns, true, udp6, IPPROTO_IPV6, IPV6_UNICAST_HOPS),
        Err(Errno::ENOSYS)
    );
    assert_eq!(pledge_socket(ping6, AF_INET6 as i32, 0), Ok(()));
}

#[test]
fn ipv6_interface_ioctls() {
    let sock = Box::leak(Box::new(File::new()));
    sock.f_type.set(DTYPE_SOCKET);
    let route = pledged(PLEDGE_STDIO | PLEDGE_ROUTE);
    for com in [
        SIOCGIFAFLAG_IN6,
        SIOCGIFALIFETIME_IN6,
        SIOCGIFDSTADDR_IN6,
        SIOCGIFNETMASK_IN6,
        SIOCGNBRINFO_IN6,
        SIOCGIFINFO_IN6,
    ] {
        assert_eq!(pledge_ioctl(route, com, sock), Ok(()));
    }
    for com in [SIOCAIFADDR_IN6, SIOCDIFADDR_IN6] {
        assert_eq!(pledge_ioctl(route, com, sock), Err(Errno::ENOSYS));
        assert_eq!(pledge_ioctl(pledged(PLEDGE_WROUTE), com, sock), Ok(()));
    }
    assert_eq!(
        pledge_ioctl(pledged(PLEDGE_STDIO), SIOCGIFAFLAG_IN6, sock),
        Err(Errno::ENOSYS)
    );
}
