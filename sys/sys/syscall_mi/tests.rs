//! Host tests for `mi_syscall`'s lock choice: the `SY_NOLOCK_DEFERRED` table against
//! `sysent`.

use super::*;
use crate::kern::init_sysent::SYSENT;

/// Every deferred entry is a `NOLOCK` system call, listed once.
#[test]
fn deferred_entries_are_nolock() {
    for (i, &code) in SY_NOLOCK_DEFERRED.iter().enumerate() {
        assert!(
            SYSENT[code as usize].sy_flags & SY_NOLOCK != 0,
            "{code} is not NOLOCK"
        );
        assert!(
            !SY_NOLOCK_DEFERRED[..i].contains(&code),
            "{code} listed twice"
        );
    }
}

/// The `NOLOCK` system calls that run without the kernel lock are the audited ones.
#[test]
fn audited_nolock_set() {
    let unlocked: std::vec::Vec<i32> = (0..SYS_MAXSYSCALL as i32)
        .filter(|&code| !syscall_lock(&SYSENT[code as usize], code as Register))
        .collect();
    let mut audited = std::vec![
        SYS_getentropy,
        SYS_getpid,
        SYS_getuid,
        SYS_geteuid,
        SYS_getppid,
        SYS_getegid,
        SYS_getgid,
        SYS_sigprocmask,
        SYS_gettimeofday,
        SYS_settimeofday,
        SYS_setitimer,
        SYS_getitimer,
        SYS_getgroups,
        SYS_futex,
        SYS_clock_gettime,
        SYS_clock_settime,
        SYS_clock_getres,
        SYS_nanosleep,
        SYS_sigsuspend,
        SYS_adjtime,
        SYS_getrlimit,
        SYS_utrace,
        SYS_issetugid,
        SYS_getresuid,
        SYS_getresgid,
        SYS_sched_yield,
        SYS_getthrid,
        SYS___thrsigdivert,
        SYS_adjfreq,
        SYS___set_tcb,
        SYS___get_tcb,
        // the `files` audit
        SYS_open,
        SYS_openat,
        SYS___pledge_open,
        SYS_stat,
        SYS_lstat,
        SYS_fstatat,
        SYS___realpath,
        SYS_dup,
        SYS_flock,
        SYS_lseek,
        SYS_kqueue,
        SYS_kqueue1,
        SYS_pipe,
        SYS_pipe2,
        SYS_getrtable,
        SYS_getdtablecount,
        // the `uvm` audit (setrlimit(2)'s RLIMIT_STACK change is mprotect's uvm_map_protect)
        SYS_munmap,
        SYS_mprotect,
        SYS_minherit,
        SYS_kbind,
        SYS_setrlimit,
        // pledge(2): ps_pledge is an atomic, the rest runs under ps_mtx (kern_pledge.c)
        SYS_pledge,
        // file I/O (vnodes, pipes, kqueues, sockets), umask(2) and fcntl(2): the `files`,
        // `net` and `netinet` audits
        SYS_read,
        SYS_write,
        SYS_close,
        SYS_fstat,
        SYS_ioctl,
        SYS_umask,
        SYS_select,
        SYS_dup2,
        SYS_fcntl,
        SYS_dup3,
        SYS_ppoll,
        SYS_pselect,
        SYS_readv,
        SYS_writev,
        SYS_pread,
        SYS_pwrite,
        SYS_preadv,
        SYS_pwritev,
        SYS_poll,
        SYS_closefrom,
        // sockets (the kern side, `net`, `netinet`), sendsyslog(2) over them
        SYS_recvmsg,
        SYS_sendmsg,
        SYS_recvfrom,
        SYS_accept,
        SYS_getpeername,
        SYS_getsockname,
        SYS_accept4,
        SYS_socket,
        SYS_connect,
        SYS_bind,
        SYS_setsockopt,
        SYS_listen,
        SYS_sendsyslog,
        SYS_recvmmsg,
        SYS_sendmmsg,
        SYS_getsockopt,
        SYS_sendto,
        SYS_shutdown,
        SYS_socketpair,
        SYS_ypconnect,
        SYS_setrtable,
        // kevent(2), mmap(2) (its file path is not ported) and sysctl(2)
        SYS_kevent,
        SYS_mmap,
        SYS_sysctl,
    ];
    audited.sort_unstable();
    assert_eq!(unlocked, audited);
    // A number past the table is locked (`sys_nosys`'s SIGSYS).
    assert!(syscall_lock(&SYSENT[0], SYS_MAXSYSCALL as Register));
}
