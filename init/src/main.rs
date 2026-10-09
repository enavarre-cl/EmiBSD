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

//! `init`: the first user process of EmiBSD, standing in for `init(8)` until there is a
//! filesystem to load one from. Freestanding and static: no libc, the system calls are the
//! raw instructions with OpenBSD's convention (the number in `rax`/`x8`, the error in the
//! carry flag). The kernel loads it as a Limine module and execs it at `start_init`.
//!
//! What it does is the M6 exit criterion: writes one line through `write(2)` and leaves
//! through `exit(2)`. Since M7a it also writes to bss pages that nothing but the fault
//! handler can provide (exec maps them zero-fill and never touches them), the demand-paging
//! exit criterion. With `kern_prot.c` it checks its ids (`getpid`, `getuid`, `issetugid`)
//! and sets its thread control block, reading it back through `__get_tcb(2)` and through
//! the TLS register (`%fs` on amd64, `TPIDR_EL0` on arm64). With `kern_descrip.c` it
//! exercises its descriptors 0, 1 and 2 (the console stand-in the kernel installs) through
//! `dup`, `dup2`, `dup3`, `fcntl`, `ioctl`, `fstat`, `close`, `closefrom`,
//! `getdtablecount` and `writev`.
//! With `kern_sig.c` it installs a `SIGUSR1` handler with `sigaction(2)`, sends itself
//! the signal with `kill(2)` and checks that the handler ran and that `sigreturn(2)` brought
//! it back; then it blocks the signal with `sigprocmask(2)`, sees it pending
//! (`sigpending(2)`) and unblocks it.
//! With `kern_sysctl.c` it asks `sysctl(2)` for `kern.ostype` and `kern.osrelease`, sets and
//! reads back `kern.hostname`, and prints `init: EmiBSD 8.0` when the system identifies itself
//! as the user decided.
//! With the vfs core (`vfs_syscalls.c`) it checks that the path system calls reach `namei`
//! and fail as they must without a root file system (`ENOENT`), that `umask(2)` swaps the
//! creation mask and that the console stand-in is not a vnode (`lseek`, `fchdir`).
//! With `sys_pipe.c` it makes pipes with `pipe2(2)` and `pipe(2)`, moves bytes through them
//! (a short write, and one large enough to grow the buffer to `BIG_PIPE_SIZE`), reads EOF
//! after the writer closes, sees `EAGAIN` on an empty non-blocking pipe, and gets `EPIPE`
//! with a `SIGPIPE` (caught, then ignored) when it writes to a pipe whose reader is gone.
//! With the tty layer (`tty.c`, `kern_proc.c`'s process groups) it becomes a session leader
//! with `setsid(2)`, makes its descriptor 0 (the console's tty) its controlling terminal with
//! `TIOCSCTTY`, reads the terminal's modes with `TIOCGETA` (what `isatty(3)` asks) and
//! finds itself the terminal's foreground process group (`TIOCGPGRP`).
//! With the real `execve` (M8) it checks the stack `start_init` and `execve` gave it (`argc`
//! 1, `argv[0]` `/init`, no environment, the auxiliary vector with the page size, the entry
//! point, base 0 and the timekeep page), that `execve(2)` of a path reaches `namei`
//! (`ENOENT`), and every system call it makes passes `pin_check`: the one `syscall`/`svc`
//! instruction is pinned for every number in its `PT_OPENBSD_SYSCALLS` table.
//! With `kern_unveil.c` it checks `unveil(2)`'s arguments (an empty path, a permission string
//! too long for its buffer), that a path reaches `namei` (`ENOENT` without a root), and that
//! `unveil(NULL, NULL)` locks the table so that a later call fails with `EPERM`.
//! With the socket layer (`uipc_socket.c`, `uipc_usrreq.c`, `uipc_syscalls.c`) it makes
//! `AF_UNIX` socket pairs with `socketpair(2)`: bytes go both ways on a stream pair, which
//! `fstat(2)` and `getsockopt(2)` know for a socket, `shutdown(2)` gives the peer EOF and the
//! writer `EPIPE`; a datagram pair keeps message boundaries (a short read truncates); and
//! `sendmsg(2)`/`recvmsg(2)` pass a descriptor in an `SCM_RIGHTS` message; a forked writer's
//! stream of small writes reads back whole and in order while both run unlocked on two CPUs.
//! With `kern_event.c` it watches a pipe with `kqueue(2)`/`kevent(2)` (`EV_ADD` on the read
//! end, a write, the event with its byte count; `EV_EOF` once the writer is closed), sees the
//! same pipe through `poll(2)` and `select(2)`, and sleeps in `kevent(2)` until a one-shot
//! `EVFILT_TIMER` fires.
//! With the Internet protocols' sockets (`in_pcb.c`, `udp_usrreq.c`, `raw_ip.c`, M9a) and
//! the address the boot self-test gave the first Ethernet interface, it asks a UDP socket for
//! `vio0`'s address (`SIOCGIFADDR`, through `ifioctl` and `in_control`), pings the gateway
//! from a raw `IPPROTO_ICMP` socket as ping(8) does (`sendto(2)`, then `recvfrom(2)` with a
//! receive timeout) and reads the echo reply with its IP header, and sends a UDP datagram
//! from one socket to another bound to its own address.
//! With `wg(4)` (`if_wg.c`) it creates `wg0` through the cloner ioctl on a socket
//! (`SIOCIFCREATE`), gives it a private key with `SIOCSWG`, reads the public key back with
//! `SIOCGWG` and compares it with RFC 7748's vector for that key, then destroys it
//! (`SIOCIFDESTROY`).

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// The page size of both architectures.
const PAGE_SIZE: usize = 4096;

/// Four pages of bss: exec maps them zero-fill (`vmcmd_map_zero`) and the first write to each
/// is a user page fault that `uvm_fault` serves.
static BSS: [AtomicU8; 4 * PAGE_SIZE] = [const { AtomicU8::new(0) }; 4 * PAGE_SIZE];

/// The `.note.openbsd.ident` note every OpenBSD executable carries (`crt0`'s), which the
/// kernel's `elf_os_pt_note` insists on: `namesz` 8, `descsz` 4, type 1, "OpenBSD\0", a
/// zero descriptor.
#[unsafe(link_section = ".note.openbsd.ident")]
#[used]
static OPENBSD_IDENT: [u8; 24] = [
    8, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, b'O', b'p', b'e', b'n', b'B', b'S', b'D', 0, 0, 0, 0, 0,
];

/// `SYS_MAXSYSCALL`: one past the last system call number (`<sys/syscall.h>`).
const SYS_MAXSYSCALL: usize = 331;

/// `SYS_exit`.
const SYS_EXIT: usize = 1;
/// `SYS_read`.
const SYS_READ: usize = 3;
/// `SYS_pipe2`.
const SYS_PIPE2: usize = 101;
/// `SYS_pipe`.
const SYS_PIPE: usize = 263;
/// `SYS_write`.
const SYS_WRITE: usize = 4;
/// `SYS_close`.
const SYS_CLOSE: usize = 6;
/// `SYS_getdtablecount`.
const SYS_GETDTABLECOUNT: usize = 18;
/// `SYS_dup`.
const SYS_DUP: usize = 41;
/// `SYS_fstat`.
const SYS_FSTAT: usize = 53;
/// `SYS_ioctl`.
const SYS_IOCTL: usize = 54;
/// `SYS_dup2`.
const SYS_DUP2: usize = 90;
/// `SYS_fcntl`.
const SYS_FCNTL: usize = 92;
/// `SYS_dup3`.
const SYS_DUP3: usize = 102;
/// `SYS_writev`.
const SYS_WRITEV: usize = 121;
/// `SYS_closefrom`.
const SYS_CLOSEFROM: usize = 287;
/// `SYS_getpid`.
const SYS_GETPID: usize = 20;
/// `SYS_getuid`.
const SYS_GETUID: usize = 24;
/// `SYS_sigaction`.
const SYS_SIGACTION: usize = 46;
/// `SYS_sigprocmask`.
const SYS_SIGPROCMASK: usize = 48;
/// `SYS_sigpending`.
const SYS_SIGPENDING: usize = 52;
/// `SYS_kill`.
const SYS_KILL: usize = 122;
/// `SYS_issetugid`.
const SYS_ISSETUGID: usize = 253;
/// `SYS___set_tcb`.
const SYS___SET_TCB: usize = 329;
/// `SYS___get_tcb`.
const SYS___GET_TCB: usize = 330;
/// `SYS_sysctl`.
const SYS_SYSCTL: usize = 202;
/// `SYS_open`.
const SYS_OPEN: usize = 5;
/// `SYS_execve`.
const SYS_EXECVE: usize = 59;
/// `SYS_fork`.
const SYS_FORK: usize = 2;
/// `SYS_wait4`.
const SYS_WAIT4: usize = 11;
/// `SYS_getentropy`.
const SYS_GETENTROPY: usize = 7;
/// `SYS_acct`.
const SYS_ACCT: usize = 51;
/// `SYS_futex`.
const SYS_FUTEX: usize = 83;
/// `SYS_pledge`.
const SYS_PLEDGE: usize = 108;
/// `SYS_sendsyslog`.
const SYS_SENDSYSLOG: usize = 112;
/// `SYS_ypconnect`.
const SYS_YPCONNECT: usize = 150;
/// `SYS_profil`.
const SYS_PROFIL: usize = 175;
/// `SYS_utrace`.
const SYS_UTRACE: usize = 209;
/// `SYS_sched_yield`.
const SYS_SCHED_YIELD: usize = 298;
/// `SYS_clock_gettime`.
const SYS_CLOCK_GETTIME: usize = 87;
/// `SYS_clock_getres`.
const SYS_CLOCK_GETRES: usize = 89;
/// `SYS_nanosleep`.
const SYS_NANOSLEEP: usize = 91;
/// `SYS_gettimeofday`.
const SYS_GETTIMEOFDAY: usize = 67;
/// `SYS_setitimer`.
const SYS_SETITIMER: usize = 69;
/// `SYS_getitimer`.
const SYS_GETITIMER: usize = 70;
/// `SYS_select`.
const SYS_SELECT: usize = 71;
/// `SYS_poll`.
const SYS_POLL: usize = 252;
/// `SYS_recvmsg`.
const SYS_RECVMSG: usize = 27;
/// `SYS_sendmsg`.
const SYS_SENDMSG: usize = 28;
/// `SYS_recvfrom`.
const SYS_RECVFROM: usize = 29;
/// `SYS_getpeername`.
const SYS_GETPEERNAME: usize = 31;
/// `SYS_socket`.
const SYS_SOCKET: usize = 97;
/// `SYS_getsockopt`.
const SYS_GETSOCKOPT: usize = 118;
/// `SYS_sendto`.
const SYS_SENDTO: usize = 133;
/// `SYS_shutdown`.
const SYS_SHUTDOWN: usize = 134;
/// `SYS_socketpair`.
const SYS_SOCKETPAIR: usize = 135;
/// `AF_UNIX`.
const AF_UNIX: usize = 1;
/// `SOCK_DGRAM`.
const SOCK_DGRAM: usize = 2;
/// `SOL_SOCKET`.
const SOL_SOCKET: u32 = 0xffff;
/// `SO_TYPE`.
const SO_TYPE: usize = 0x1008;
/// `SCM_RIGHTS`.
const SCM_RIGHTS: u32 = 0x01;
/// `SHUT_WR`.
const SHUT_WR: usize = 1;
/// `MSG_DONTWAIT`, `MSG_NOSIGNAL`.
const MSG_DONTWAIT: usize = 0x80;
const MSG_NOSIGNAL: usize = 0x400;
/// `S_IFSOCK`.
const S_IFSOCK: u32 = 0o140000;
/// `SYS_kevent`.
const SYS_KEVENT: usize = 72;
/// `SYS_kqueue`.
const SYS_KQUEUE: usize = 269;
/// `CLOCK_REALTIME`.
const CLOCK_REALTIME: usize = 0;
/// `CLOCK_MONOTONIC`.
const CLOCK_MONOTONIC: usize = 3;
/// `CLOCK_UPTIME`.
const CLOCK_UPTIME: usize = 5;
/// `ITIMER_REAL`.
const ITIMER_REAL: usize = 0;
/// `SIGALRM`.
const SIGALRM: usize = 14;
/// `EINTR`.
const EINTR: usize = 4;
/// `SYS_setrtable`.
const SYS_SETRTABLE: usize = 310;
/// `SYS_getrtable`.
const SYS_GETRTABLE: usize = 311;
/// `ECHILD`.
const ECHILD: usize = 10;
/// `EPERM`.
const EPERM: usize = 1;
/// `ENOTCONN`.
const ENOTCONN: usize = 57;
/// `EAFNOSUPPORT`.
const EAFNOSUPPORT: usize = 47;
/// `SIGABRT`.
const SIGABRT: usize = 6;
/// `FUTEX_WAKE`.
const FUTEX_WAKE: usize = 2;
/// `LOG_CONS` (`<sys/syslog.h>`).
const LOG_CONS: usize = 0x02;
/// `SOCK_STREAM`.
const SOCK_STREAM: usize = 1;
/// `SYS_chdir`.
const SYS_CHDIR: usize = 12;
/// `SYS_fchdir`.
const SYS_FCHDIR: usize = 13;
/// `SYS_stat`.
const SYS_STAT: usize = 38;
/// `SYS_umask`.
const SYS_UMASK: usize = 60;
/// `SYS_lseek`.
const SYS_LSEEK: usize = 166;
/// `SYS___getcwd`.
const SYS___GETCWD: usize = 304;
/// `SYS_setsid`.
const SYS_SETSID: usize = 147;
/// `SYS_unveil`.
const SYS_UNVEIL: usize = 114;
/// `EFAULT`.
const EFAULT: usize = 14;
/// `ENAMETOOLONG`.
const ENAMETOOLONG: usize = 63;

/// `CTL_KERN` (`<sys/sysctl.h>`).
const CTL_KERN: i32 = 1;
/// `KERN_OSTYPE`.
const KERN_OSTYPE: i32 = 1;
/// `KERN_OSRELEASE`.
const KERN_OSRELEASE: i32 = 2;
/// `KERN_HOSTNAME`.
const KERN_HOSTNAME: i32 = 10;

/// `ENOENT`.
const ENOENT: usize = 2;
/// `EBADF`.
const EBADF: usize = 9;
/// `ENOTDIR`.
const ENOTDIR: usize = 20;
/// `ESPIPE`.
const ESPIPE: usize = 29;
/// `EINVAL`.
const EINVAL: usize = 22;
/// `ENOSYS`.
const ENOSYS: usize = 78;
/// `EPIPE`.
const EPIPE: usize = 32;
/// `EAGAIN`.
const EAGAIN: usize = 35;
/// `EEXIST`.
const EEXIST: usize = 17;
/// `ENXIO`.
const ENXIO: usize = 6;
/// `SIOCIFCREATE`: `_IOW('i', 122, struct ifreq)`.
const SIOCIFCREATE: usize = 0x8020_697a;
/// `SIOCIFDESTROY`: `_IOW('i', 121, struct ifreq)`.
const SIOCIFDESTROY: usize = 0x8020_6979;
/// `SIOCSWG`: `_IOWR('i', 210, struct wg_data_io)`.
const SIOCSWG: usize = 0xc020_69d2;
/// `SIOCGWG`: `_IOWR('i', 211, struct wg_data_io)`.
const SIOCGWG: usize = 0xc020_69d3;
/// `WG_INTERFACE_HAS_PUBLIC`.
const WG_INTERFACE_HAS_PUBLIC: u8 = 1 << 0;
/// `WG_INTERFACE_HAS_PRIVATE`.
const WG_INTERFACE_HAS_PRIVATE: u8 = 1 << 1;
/// `O_NONBLOCK`, `O_CLOEXEC`.
const O_NONBLOCK: usize = 0x4;
const O_CLOEXEC: usize = 0x10000;
/// `FIONREAD`: `_IOR('f', 127, int)`.
const FIONREAD: usize = 0x4004_667f;
/// `S_IFIFO`.
const S_IFIFO: u32 = 0o010000;
/// `SIGPIPE`.
const SIGPIPE: usize = 13;
/// `SIG_IGN`.
const SIG_IGN: usize = 1;
/// `F_DUPFD`, `F_GETFD`, `F_SETFD`, `F_GETFL`, `F_DUPFD_CLOEXEC`.
const F_DUPFD: usize = 0;
const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_DUPFD_CLOEXEC: usize = 10;
/// `FD_CLOEXEC`.
const FD_CLOEXEC: usize = 1;
/// `O_RDONLY`, `O_RDWR`.
const O_RDONLY: usize = 0;
const O_RDWR: usize = 2;
/// `SEEK_CUR`.
const SEEK_CUR: usize = 1;
/// `FIOCLEX`, `FIONCLEX`: `_IO('f', 1)`, `_IO('f', 2)`.
const FIOCLEX: usize = 0x2000_6601;
const FIONCLEX: usize = 0x2000_6602;
/// `TIOCSCTTY`: `_IO('t', 97)`.
const TIOCSCTTY: usize = 0x2000_7461;
/// `TIOCGETA`: `_IOR('t', 19, struct termios)`, a 44-byte `struct termios`.
const TIOCGETA: usize = 0x402c_7413;
/// `TIOCGPGRP`: `_IOR('t', 119, int)`.
const TIOCGPGRP: usize = 0x4004_7477;
/// `ICANON`, in `c_lflag`.
const ICANON: u32 = 0x0000_0100;
/// `S_IFMT`, `S_IFCHR`.
const S_IFMT: u32 = 0o170000;
const S_IFCHR: u32 = 0o020000;
/// `SIGUSR1`.
const SIGUSR1: usize = 30;
/// `SIG_BLOCK`.
const SIG_BLOCK: usize = 1;
/// `SIG_SETMASK`.
const SIG_SETMASK: usize = 3;

/// `EVFILT_READ`.
const EVFILT_READ: i16 = -1;
/// `EVFILT_TIMER`.
const EVFILT_TIMER: i16 = -7;
/// `EV_ADD`.
const EV_ADD: u16 = 0x0001;
/// `EV_ONESHOT`.
const EV_ONESHOT: u16 = 0x0010;
/// `EV_EOF`.
const EV_EOF: u16 = 0x8000;
/// `POLLIN`.
const POLLIN: i16 = 0x0001;
/// `POLLOUT`.
const POLLOUT: i16 = 0x0004;
/// `POLLHUP`.
const POLLHUP: i16 = 0x0010;

/// `struct kevent`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Kevent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: i64,
    udata: usize,
}

/// `struct pollfd`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

/// `struct sigaction`: the handler, the mask to apply while it runs, the `SA_*` flags.
#[repr(C)]
struct Sigaction {
    sa_handler: usize,
    sa_mask: u32,
    sa_flags: i32,
}

/// How many times `on_sigusr1` ran.
static HANDLED: AtomicUsize = AtomicUsize::new(0);

/// How many times `on_sigpipe` ran.
static SIGPIPES: AtomicUsize = AtomicUsize::new(0);

/// Bytes for a write larger than `PIPE_SIZE` (16384), which makes the kernel grow the pipe's
/// buffer to `BIG_PIPE_SIZE` (65536) instead of blocking.
const BIG_WRITE: usize = 20000;

/// The bytes of the large write (bss, filled with a pattern before use).
static BIG: [AtomicU8; BIG_WRITE] = [const { AtomicU8::new(0) }; BIG_WRITE];

/// The thread control block: its first word points at itself, as the TLS ABIs want, so the
/// TLS register can be checked by reading through it.
static TCB: AtomicUsize = AtomicUsize::new(0);

/// A three-argument system call: the return register and whether the carry flag (OpenBSD's
/// error indication) was set. Every system call goes through [`syscall6`], the one call site
/// the pin table names.
fn syscall3(number: usize, a: usize, b: usize, c: usize) -> (usize, bool) {
    syscall6(number, [a, b, c, 0, 0, 0])
}

/// A six-argument system call (`syscall`: the fourth argument in `r10`, as the kernel's
/// `Xsyscall` reads it).
///
/// This is the program's only system call instruction. The kernel's `pin_check` accepts a
/// system call only from the site the executable's `PT_OPENBSD_SYSCALLS` table names for
/// its number (libc's stubs each emit one `PINSYSCALL` entry); this function emits one entry
/// per system call number, all naming this instruction, into `.openbsd.syscalls`, which
/// `init.ld` puts in that segment. `inline(never)` keeps the instruction single.
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn syscall6(number: usize, a: [usize; 6]) -> (usize, bool) {
    let ret: usize;
    let carry: u8;
    // SAFETY: the `syscall` instruction with the OpenBSD register convention; the kernel
    // owns everything that happens, and clobbers only rcx and r11 besides the outputs. The
    // section directives only add data to `.openbsd.syscalls`.
    unsafe {
        asm!(
            "2:",
            "syscall",
            ".pushsection .openbsd.syscalls,\"a\"",
            ".set .Linit_pin_sysno, 1",
            ".rept {nsys}",
            ".long 2b",
            ".long .Linit_pin_sysno",
            ".set .Linit_pin_sysno, .Linit_pin_sysno + 1",
            ".endr",
            ".popsection",
            "setc {carry}",
            nsys = const SYS_MAXSYSCALL - 1,
            carry = out(reg_byte) carry,
            inlateout("rax") number => ret,
            in("rdi") a[0],
            in("rsi") a[1],
            in("rdx") a[2],
            in("r10") a[3],
            in("r8") a[4],
            in("r9") a[5],
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    (ret, carry != 0)
}

/// A six-argument system call: `svc #0`, arguments in `x0`..`x5`, followed by the
/// speculation barrier the kernel skips over (`svc_handler` adds 8 to the return address).
/// The only system call instruction, pinned for every number as on amd64.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
fn syscall6(number: usize, a: [usize; 6]) -> (usize, bool) {
    let ret: usize;
    let carry: usize;
    // SAFETY: the `svc` instruction with the OpenBSD register convention; the kernel owns
    // everything that happens and clobbers nothing but the outputs. The section directives
    // only add data to `.openbsd.syscalls`.
    unsafe {
        asm!(
            "2:",
            "svc #0",
            "dsb nsh",
            "isb",
            ".pushsection .openbsd.syscalls,\"a\"",
            ".set .Linit_pin_sysno, 1",
            ".rept {nsys}",
            ".long 2b",
            ".long .Linit_pin_sysno",
            ".set .Linit_pin_sysno, .Linit_pin_sysno + 1",
            ".endr",
            ".popsection",
            "cset {carry}, cs",
            nsys = const SYS_MAXSYSCALL - 1,
            carry = out(reg) carry,
            in("x8") number,
            inlateout("x0") a[0] => ret,
            in("x1") a[1],
            in("x2") a[2],
            in("x3") a[3],
            in("x4") a[4],
            in("x5") a[5],
            options(nostack)
        );
    }
    (ret, carry != 0)
}

/// `sysctl(2)` for a string: the bytes before the NUL, in `buf`.
fn sysctl_string<'a>(name: &[i32], buf: &'a mut [u8]) -> Option<&'a [u8]> {
    let mut len = buf.len();
    let args = [
        name.as_ptr() as usize,
        name.len(),
        buf.as_mut_ptr() as usize,
        &mut len as *mut usize as usize,
        0,
        0,
    ];
    match syscall6(SYS_SYSCTL, args) {
        (0, false) if len > 0 && len <= buf.len() && buf[len - 1] == 0 => Some(&buf[..len - 1]),
        _ => None,
    }
}

/// `kern_sysctl.c` seen from user mode: the system says it is EmiBSD 8.0, and root can set
/// `kern.hostname` and read it back (the path that wires the caller's buffer under
/// `sysctl_lock`).
fn identity() -> bool {
    let mut ostype = [0u8; 32];
    let mut osrelease = [0u8; 32];
    let mut hostname = [0u8; 32];
    let new = b"emibsd";
    let name = [CTL_KERN, KERN_HOSTNAME];
    let set = syscall6(
        SYS_SYSCTL,
        [
            name.as_ptr() as usize,
            2,
            0,
            0,
            new.as_ptr() as usize,
            new.len(),
        ],
    );
    sysctl_string(&[CTL_KERN, KERN_OSTYPE], &mut ostype) == Some(b"EmiBSD")
        && sysctl_string(&[CTL_KERN, KERN_OSRELEASE], &mut osrelease) == Some(b"8.0")
        && set == (0, false)
        && sysctl_string(&name, &mut hostname) == Some(new)
}

/// `write(2)`.
fn write(fd: usize, buf: &[u8]) -> Result<usize, usize> {
    match syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) {
        (n, false) => Ok(n),
        (errno, true) => Err(errno),
    }
}

/// `exit(2)`: never returns; if it does, something is badly wrong and we spin.
fn exit(status: usize) -> ! {
    let _ = syscall3(SYS_EXIT, status, 0, 0);
    loop {
        core::hint::spin_loop();
    }
}

/// `AUX_null`: the end of the auxiliary vector (`<sys/exec_elf.h>`).
const AUX_NULL: usize = 0;
/// `AUX_phdr`.
const AUX_PHDR: usize = 3;
/// `AUX_pagesz`.
const AUX_PAGESZ: usize = 6;
/// `AUX_base`.
const AUX_BASE: usize = 7;
/// `AUX_entry`.
const AUX_ENTRY: usize = 9;
/// `AUX_openbsd_timekeep`.
const AUX_OPENBSD_TIMEKEEP: usize = 4000;

// The entry point: the stack pointer `execve` left points at `argc`, then `argv`, `envp`
// and the auxiliary vector (`copyargs`, `exec_elf_fixup`); pass it to `init_main`.
#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".globl _start",
    "_start:",
    "mov rdi, rsp",
    "and rsp, -16",
    "call {main}",
    "ud2",
    main = sym init_main,
);
#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".globl _start",
    "_start:",
    "mov x0, sp",
    "bl {main}",
    "brk #0",
    main = sym init_main,
);

/// The new stack as `execve` lays it out: `argc` 1, `argv[0]` the boot module's path,
/// no environment, and an auxiliary vector that names the page size, the entry point,
/// the program headers, base 0 (not a PIE) and the timekeep page.
fn args_and_auxv(sp: *const usize) -> bool {
    // SAFETY: `execve` wrote these words at the initial stack pointer: argc, argc argument
    // pointers and a NULL, the environment pointers and a NULL, then the auxiliary vector up
    // to `AUX_null`; all in the mapped stack.
    let word = |i: usize| unsafe { sp.add(i).read() };
    let argc = word(0);
    if argc != 1 || word(2) != 0 || word(3) != 0 {
        return false;
    }
    // SAFETY: argv[0] points at the NUL-terminated path `start_init` copied out.
    let arg0 = unsafe { core::ffi::CStr::from_ptr(word(1) as *const core::ffi::c_char) };
    if arg0.to_bytes() != b"/init" {
        return false;
    }
    let (mut pagesz, mut entry, mut phdr, mut base, mut timekeep) = (0, 0, 0, usize::MAX, 0);
    let mut i = 4;
    loop {
        let (id, v) = (word(i), word(i + 1));
        match id {
            AUX_NULL => break,
            AUX_PAGESZ => pagesz = v,
            AUX_ENTRY => entry = v,
            AUX_PHDR => phdr = v,
            AUX_BASE => base = v,
            AUX_OPENBSD_TIMEKEEP => timekeep = v,
            _ => {}
        }
        i += 2;
        if i > 4 + 2 * 12 {
            return false;
        }
    }
    let start: unsafe extern "C" fn() = _start;
    pagesz == PAGE_SIZE && entry == start as usize && phdr == 0 && base == 0 && timekeep != 0
}

unsafe extern "C" {
    /// The entry point above.
    fn _start();
}

/// `getpid(2)` from a call site of its own, which the `PT_OPENBSD_SYSCALLS` table does not
/// name: `pin_check` must kill the caller with `SIGABRT`.
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn unpinned_getpid() -> usize {
    let ret: usize;
    // SAFETY: the `syscall` instruction with the OpenBSD register convention; the kernel
    // kills the process here (the site is not pinned), which is what the caller wants.
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_GETPID => ret,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

/// `getpid(2)` from a call site of its own (see the amd64 version).
#[cfg(target_arch = "aarch64")]
#[inline(never)]
fn unpinned_getpid() -> usize {
    let ret: usize;
    // SAFETY: the `svc` instruction with the OpenBSD register convention; the kernel kills
    // the process here (the site is not pinned), which is what the caller wants.
    unsafe {
        asm!(
            "svc #0",
            "dsb nsh",
            "isb",
            in("x8") SYS_GETPID,
            lateout("x0") ret,
            options(nostack)
        );
    }
    ret
}

/// `kern_fork.c`, `kern_exit.c` and the process system calls seen from user mode: a forked
/// child exits with a status `wait4(2)` reports, a child that makes a system call from an
/// unpinned site dies of `SIGABRT` (`pin_check`), there are no more children (`ECHILD`),
/// and `getentropy`, `sched_yield`, `futex`, `utrace`, `pledge` (a bad promise only: a
/// pledge lasts for the process, see [`pledges`]), `acct`, `setrtable`, `getrtable`,
/// `ypconnect`, `profil` and `sendsyslog` answer as OpenBSD's do here.
fn processes() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut status: i32 = 0;
    let sp = &mut status as *mut i32 as usize;

    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => exit(7),
        (pid, false) => pid,
        _ => return false,
    };
    let mut ok = call(SYS_WAIT4, pid, sp, 0) == (pid, false);
    ok &= (status >> 8) & 0xff == 7 && status & 0x7f == 0;

    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => {
            unpinned_getpid();
            exit(8)
        }
        (pid, false) => pid,
        _ => return false,
    };
    ok &= call(SYS_WAIT4, pid, sp, 0) == (pid, false);
    ok &= status & 0x7f == SIGABRT as i32;
    ok &= call(SYS_WAIT4, usize::MAX, sp, 0) == (ECHILD, true);

    let mut entropy = [0u8; 32];
    ok &= call(
        SYS_GETENTROPY,
        entropy.as_mut_ptr() as usize,
        entropy.len(),
        0,
    ) == (0, false);
    ok &= entropy.iter().any(|&b| b != 0);
    ok &= call(SYS_GETENTROPY, entropy.as_mut_ptr() as usize, 257, 0) == (EINVAL, true);
    ok &= call(SYS_SCHED_YIELD, 0, 0, 0) == (0, false);
    let word: u32 = 0;
    ok &= syscall6(
        SYS_FUTEX,
        [&word as *const u32 as usize, FUTEX_WAKE, 1, 0, 0, 0],
    ) == (0, false);
    ok &= call(SYS_UTRACE, c"init".as_ptr() as usize, 0, 0) == (0, false);
    ok &= call(SYS_PLEDGE, c"stdio bogus".as_ptr() as usize, 0, 0) == (EINVAL, true);
    ok &= call(SYS_ACCT, 0, 0, 0) == (0, false);
    ok &= call(SYS_SETRTABLE, 0, 0, 0) == (0, false);
    ok &= call(SYS_GETRTABLE, 0, 0, 0) == (0, false);
    ok &= call(SYS_YPCONNECT, SOCK_STREAM, 0, 0) == (EAFNOSUPPORT, true);
    ok &= syscall6(SYS_PROFIL, [0, 0, 0, 0, 1, usize::MAX]) == (EPERM, true);
    // No syslogd(8): with LOG_CONS the message goes to the console without its priority.
    let msg = b"<13>init: sendsyslog ok";
    ok &= call(SYS_SENDSYSLOG, msg.as_ptr() as usize, msg.len(), LOG_CONS) == (ENOTCONN, true);
    ok
}

/// `kern_pledge.c` seen from user mode, in forked children since a pledge lasts for the
/// process: a child pledged to "stdio" may still call `getpid(2)` and exit with a status of
/// its choosing but cannot widen its promises (`EPERM`); calling `fork(2)`, which needs
/// "proc", kills it with an uncatchable `SIGABRT` (`pledge_fail`); under "error" the same
/// call answers `ENOSYS` instead. The parent stays unpledged.
fn pledges() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let pledge = |promises: &core::ffi::CStr| call(SYS_PLEDGE, promises.as_ptr() as usize, 0, 0);
    let mut status: i32 = 0;
    let sp = &mut status as *mut i32 as usize;

    // Allowed calls still work.
    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => {
            let mut ok = pledge(c"stdio") == (0, false);
            ok &= !call(SYS_GETPID, 0, 0, 0).1;
            ok &= pledge(c"stdio rpath") == (EPERM, true);
            exit(if ok { 21 } else { 1 })
        }
        (pid, false) => pid,
        _ => return false,
    };
    let mut ok = call(SYS_WAIT4, pid, sp, 0) == (pid, false);
    ok &= status & 0x7f == 0 && (status >> 8) & 0xff == 21;

    // A forbidden one is fatal.
    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => {
            if pledge(c"stdio") == (0, false) {
                let _ = call(SYS_FORK, 0, 0, 0);
            }
            exit(1)
        }
        (pid, false) => pid,
        _ => return false,
    };
    ok &= call(SYS_WAIT4, pid, sp, 0) == (pid, false);
    ok &= status & 0x7f == SIGABRT as i32;

    // Unless the process asked for errors.
    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => {
            let ok =
                pledge(c"stdio error") == (0, false) && call(SYS_FORK, 0, 0, 0) == (ENOSYS, true);
            exit(if ok { 22 } else { 1 })
        }
        (pid, false) => pid,
        _ => return false,
    };
    ok &= call(SYS_WAIT4, pid, sp, 0) == (pid, false);
    ok && status & 0x7f == 0 && (status >> 8) & 0xff == 22
}

/// How many times `on_sigalrm` ran.
static ALARMS: AtomicUsize = AtomicUsize::new(0);

/// The `SIGALRM` handler of [`times`].
extern "C" fn on_sigalrm(sig: i32) {
    if sig as usize == SIGALRM {
        ALARMS.fetch_add(1, Ordering::Relaxed);
    }
}

/// `kern_time.c` seen from user mode: the monotonic clock advances across a 20 ms
/// `nanosleep(2)`, the clocks have a resolution, `gettimeofday(2)` answers, and a 200 ms
/// `ITIMER_REAL` timer interrupts a long `nanosleep` with `SIGALRM` (`EINTR`, the time left
/// copied out) and is then disarmed. `select(2)` and `poll(2)` with no descriptors sleep for
/// their timeout (`sys_generic.c`).
fn times() -> bool {
    let mut a = [0i64; 2];
    let mut b = [0i64; 2];
    let mut res = [0i64; 2];
    let mut tv = [0i64; 2];
    let call = |n, x, y, z| syscall3(n, x, y, z);

    let mut ok = call(
        SYS_CLOCK_GETTIME,
        CLOCK_MONOTONIC,
        a.as_mut_ptr() as usize,
        0,
    ) == (0, false);
    let short = [0i64, 20_000_000];
    ok &= call(SYS_NANOSLEEP, short.as_ptr() as usize, 0, 0) == (0, false);
    ok &= call(
        SYS_CLOCK_GETTIME,
        CLOCK_MONOTONIC,
        b.as_mut_ptr() as usize,
        0,
    ) == (0, false);
    // nanosleep(2) measures the time with the coarse getnanouptime(9), a tick behind the
    // precise clock, so it may end a little before 20 ms of CLOCK_MONOTONIC: ask for half.
    let elapsed = (b[0] - a[0]) * 1_000_000_000 + (b[1] - a[1]);
    ok &= elapsed >= 10_000_000;
    ok &= call(
        SYS_CLOCK_GETRES,
        CLOCK_REALTIME,
        res.as_mut_ptr() as usize,
        0,
    ) == (0, false);
    ok &= res[0] == 0 && res[1] > 0;
    ok &= call(SYS_GETTIMEOFDAY, tv.as_mut_ptr() as usize, 0, 0) == (0, false);
    ok &= tv[1] >= 0 && tv[1] < 1_000_000;

    let sa = Sigaction {
        sa_handler: on_sigalrm as *const () as usize,
        sa_mask: 0,
        sa_flags: 0,
    };
    ok &= call(SYS_SIGACTION, SIGALRM, &sa as *const Sigaction as usize, 0) == (0, false);
    // it_interval 0, it_value 200 ms: long enough that the alarm cannot arrive before the
    // nanosleep below starts, even on a slow emulator.
    let itv = [0i64, 0, 0, 200_000];
    let mut old = [1i64; 4];
    ok &= call(
        SYS_SETITIMER,
        ITIMER_REAL,
        itv.as_ptr() as usize,
        old.as_mut_ptr() as usize,
    ) == (0, false);
    ok &= old == [0; 4];
    let long = [5i64, 0];
    let mut left = [0i64; 2];
    ok &= call(
        SYS_NANOSLEEP,
        long.as_ptr() as usize,
        left.as_mut_ptr() as usize,
        0,
    ) == (EINTR, true);
    ok &= ALARMS.load(Ordering::Relaxed) == 1;
    ok &= left[0] >= 4 && left[0] <= 5;
    // one-shot: nothing left to report
    let mut now = [1i64; 4];
    ok &= call(SYS_GETITIMER, ITIMER_REAL, now.as_mut_ptr() as usize, 0) == (0, false);
    ok &= now == [0; 4];

    // select(2) and poll(2) with no descriptors sleep for their timeout and find nothing.
    let tv10 = [0i64, 10_000];
    ok &= syscall6(SYS_SELECT, [0, 0, 0, 0, tv10.as_ptr() as usize, 0]) == (0, false);
    ok &= call(
        SYS_CLOCK_GETTIME,
        CLOCK_MONOTONIC,
        a.as_mut_ptr() as usize,
        0,
    ) == (0, false);
    ok &= call(SYS_POLL, 0, 0, 10) == (0, false);
    ok &= call(
        SYS_CLOCK_GETTIME,
        CLOCK_MONOTONIC,
        b.as_mut_ptr() as usize,
        0,
    ) == (0, false);
    ok &= (b[0] - a[0]) * 1_000_000_000 + (b[1] - a[1]) >= 5_000_000;
    ok
}

/// The program, called by `_start` with the initial stack pointer.
extern "C" fn init_main(sp: *const usize) -> ! {
    let mut status = match write(1, b"init: hello from user mode\n") {
        Ok(_) => 0,
        Err(_) => 1,
    };
    if args_and_auxv(sp) {
        if write(1, b"init: argv and auxv ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 9;
    }
    if demand_zero_bss() {
        if write(1, b"init: demand-zero bss ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 2;
    }
    if ids_and_tcb() {
        if write(1, b"init: ids and tcb ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 3;
    }
    if signals() {
        if write(1, b"init: signals ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 5;
    }
    if identity() {
        if write(1, b"init: EmiBSD 8.0\n").is_err() {
            status = 1;
        }
    } else {
        status = 6;
    }
    if vfs() {
        if write(1, b"init: vfs ok (no root file system)\n").is_err() {
            status = 1;
        }
    } else {
        status = 7;
    }
    if tty() {
        if write(1, b"init: tty ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 23;
    }
    if !fds() {
        status = 4;
    }
    if pipes() {
        if write(1, b"init: pipes ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 8;
    }
    if sockets() {
        if write(1, b"init: sockets ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 13;
    }
    if wg() {
        if write(1, b"init: wg ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 16;
    }
    if kqueues() {
        if write(1, b"init: kqueue ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 14;
    }
    if inet() {
        if write(1, b"init: inet sockets ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 15;
    }
    if pfkey() {
        if write(1, b"init: pfkey ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 17;
    }
    if tcp() {
        if write(1, b"init: tcp ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 18;
    }
    if processes() {
        if write(1, b"init: processes ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 10;
    }
    if pledges() {
        if write(1, b"init: pledge ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 19;
    }
    if times() {
        if write(1, b"init: time ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 11;
    }
    if uptime_monotonic() {
        if write(1, b"init: uptime monotonic ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 20;
    }
    // Last: it locks unveil(2) for this process.
    if unveil() {
        if write(1, b"init: unveil ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 12;
    }
    exit(status)
}

/// The uptime clock never steps back: `CLOCK_UPTIME` read in a tight loop for about a
/// second (amd64's i8254 timecounter wrapped every 27 ms and could lose a period when a
/// windup came late; the TSC and arm64's generic timer cannot).
fn uptime_monotonic() -> bool {
    let read = |ts: &mut [i64; 2]| {
        syscall3(SYS_CLOCK_GETTIME, CLOCK_UPTIME, ts.as_mut_ptr() as usize, 0) == (0, false)
    };
    let nsec = |ts: &[i64; 2]| ts[0] * 1_000_000_000 + ts[1];
    let mut ts = [0i64; 2];
    if !read(&mut ts) {
        return false;
    }
    let start = nsec(&ts);
    let mut last = start;
    while last - start < 1_000_000_000 {
        if !read(&mut ts) {
            return false;
        }
        let now = nsec(&ts);
        if now < last {
            return false;
        }
        last = now;
    }
    true
}

/// `unveil(2)` (`vfs_syscalls.c`'s `sys_unveil`, `kern_unveil.c`) before a root file system
/// exists: the argument checks come first (`EINVAL` for an empty path, `ENAMETOOLONG` for
/// permissions longer than four characters, `EFAULT` for a NULL path), a real path reaches
/// `namei` (`ENOENT`), and `unveil(NULL, NULL)` locks the table: the next call is `EPERM`.
fn unveil() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let unveil = |path: usize, perms: usize| call(SYS_UNVEIL, path, perms, 0);
    let r = c"r".as_ptr() as usize;
    let mut ok = unveil(c"".as_ptr() as usize, r) == (EINVAL, true);
    ok &= unveil(c"/".as_ptr() as usize, c"rwxcr".as_ptr() as usize) == (ENAMETOOLONG, true);
    ok &= unveil(0, r) == (EFAULT, true);
    ok &= unveil(c"/".as_ptr() as usize, r) == (ENOENT, true);
    ok &= unveil(c"/etc/rc".as_ptr() as usize, c"rw".as_ptr() as usize) == (ENOENT, true);
    ok &= unveil(0, 0) == (0, false);
    ok && unveil(c"/".as_ptr() as usize, r) == (EPERM, true)
}

/// `vfs_syscalls.c` seen from user mode before a root file system exists: every path ends in
/// `namei`'s `ENOENT`, no descriptor is left behind by a failed `open`, the creation mask is
/// the one `fdinit` set (022), and the console stand-in cannot seek nor be a directory.
fn vfs() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut st = [0u64; 16];
    let mut cwd = [0u8; 64];
    let mut ok = call(SYS_OPEN, c"/etc/rc".as_ptr() as usize, O_RDONLY, 0) == (ENOENT, true);
    ok &= call(
        SYS_STAT,
        c"/".as_ptr() as usize,
        st.as_mut_ptr() as usize,
        0,
    ) == (ENOENT, true);
    ok &= call(SYS_CHDIR, c"/".as_ptr() as usize, 0, 0) == (ENOENT, true);
    ok &= call(SYS___GETCWD, cwd.as_mut_ptr() as usize, cwd.len(), 0) == (ENOENT, true);
    let argv = [c"init".as_ptr() as usize, 0];
    ok &= call(
        SYS_EXECVE,
        c"/sbin/init".as_ptr() as usize,
        argv.as_ptr() as usize,
        0,
    ) == (ENOENT, true);
    ok &= call(SYS_UMASK, 0o077, 0, 0) == (0o022, false);
    ok &= call(SYS_UMASK, 0o022, 0, 0) == (0o077, false);
    ok &= call(SYS_LSEEK, 1, 0, SEEK_CUR) == (ESPIPE, true);
    ok &= call(SYS_FCHDIR, 1, 0, 0) == (ENOTDIR, true);
    ok && call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false)
}

/// `tty.c` and the process groups seen from user mode: `setsid(2)` makes a session and a
/// group named after the process (a second call fails, it already leads one), the console
/// becomes the session's controlling terminal, answers `TIOCGETA` with canonical input on,
/// and reports the new group as its foreground group.
fn tty() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let (pid, _) = call(SYS_GETPID, 0, 0, 0);
    let mut ok = call(SYS_SETSID, 0, 0, 0) == (pid, false);
    ok &= call(SYS_SETSID, 0, 0, 0) == (EPERM, true);
    ok &= call(SYS_IOCTL, 0, TIOCSCTTY, 0) == (0, false);
    let mut termios = [0u32; 11];
    ok &= call(SYS_IOCTL, 0, TIOCGETA, termios.as_mut_ptr() as usize) == (0, false);
    ok &= termios[3] & ICANON != 0;
    let mut pgrp: i32 = 0;
    ok &= call(SYS_IOCTL, 0, TIOCGPGRP, &mut pgrp as *mut i32 as usize) == (0, false);
    ok && pgrp as usize == pid
}

/// `kern_descrip.c` seen from user mode. Descriptors 0, 1 and 2 are one console file; the
/// duplicates take the lowest free numbers, carry their own close-on-exec flag and share
/// the file. The last check writes "init: fds ok" through a duplicate with `writev`.
fn fds() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut ok = call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false);
    ok &= call(SYS_DUP, 1, 0, 0) == (3, false);
    ok &= call(SYS_DUP2, 3, 10, 0) == (10, false);
    ok &= call(SYS_FCNTL, 10, F_GETFD, 0) == (0, false);
    ok &= call(SYS_FCNTL, 10, F_SETFD, FD_CLOEXEC) == (0, false);
    ok &= call(SYS_FCNTL, 10, F_GETFD, 0) == (FD_CLOEXEC, false);
    ok &= call(SYS_FCNTL, 3, F_DUPFD, 5) == (5, false);
    ok &= call(SYS_FCNTL, 3, F_DUPFD_CLOEXEC, 5) == (6, false);
    ok &= call(SYS_FCNTL, 6, F_GETFD, 0) == (FD_CLOEXEC, false);
    ok &= call(SYS_FCNTL, 1, F_GETFL, 0) == (O_RDWR, false);
    ok &= call(SYS_DUP3, 3, 3, 0) == (EINVAL, true);
    ok &= call(SYS_IOCTL, 5, FIOCLEX, 0) == (0, false);
    ok &= call(SYS_FCNTL, 5, F_GETFD, 0) == (FD_CLOEXEC, false);
    ok &= call(SYS_IOCTL, 5, FIONCLEX, 0) == (0, false);
    ok &= call(SYS_FCNTL, 5, F_GETFD, 0) == (0, false);
    ok &= call(SYS_CLOSE, 5, 0, 0) == (0, false);
    ok &= call(SYS_CLOSE, 5, 0, 0) == (EBADF, true);
    ok &= call(SYS_GETDTABLECOUNT, 0, 0, 0) == (6, false);

    let mut st = [0u64; 16];
    ok &= call(SYS_FSTAT, 3, st.as_mut_ptr() as usize, 0) == (0, false);
    ok &= (st[0] as u32) & S_IFMT == S_IFCHR;

    ok &= call(SYS_CLOSEFROM, 4, 0, 0) == (0, false);
    ok &= call(SYS_GETDTABLECOUNT, 0, 0, 0) == (4, false);
    ok &= call(SYS_WRITE, 10, b"x".as_ptr() as usize, 1) == (EBADF, true);

    if ok {
        let (a, b) = (b"init: fds", b" ok\n");
        let iov = [a.as_ptr() as usize, a.len(), b.as_ptr() as usize, b.len()];
        ok &= call(SYS_WRITEV, 3, iov.as_ptr() as usize, 2) == (a.len() + b.len(), false);
    }
    ok &= call(SYS_CLOSE, 3, 0, 0) == (0, false);
    ok && call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false)
}

/// `pipe2(2)` (or `pipe(2)` with `flags` `None`): the two descriptors.
fn pipe(flags: Option<usize>) -> Option<[usize; 2]> {
    let mut fds = [0i32; 2];
    let fdp = fds.as_mut_ptr() as usize;
    let r = match flags {
        Some(flags) => syscall3(SYS_PIPE2, fdp, flags, 0),
        None => syscall3(SYS_PIPE, fdp, 0, 0),
    };
    (r == (0, false)).then_some([fds[0] as usize, fds[1] as usize])
}

/// The `SIGPIPE` handler.
extern "C" fn on_sigpipe(sig: i32) {
    if sig as usize == SIGPIPE {
        SIGPIPES.fetch_add(1, Ordering::Relaxed);
    }
}

/// `sys_pipe.c` seen from user mode: bytes written at one end are read at the other, in
/// pieces and across the growth of the buffer; an empty non-blocking pipe says `EAGAIN`;
/// the reader sees EOF once the writer is closed; a writer whose reader is gone gets `EPIPE`
/// and `SIGPIPE`, which kills nobody when it is caught or ignored.
fn pipes() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut buf = [0u8; 16];
    let mut st = [0u64; 16];
    let mut nread = 0u32;

    // pipe2 with O_CLOEXEC: the two lowest free descriptors, both close-on-exec.
    let Some([r, w]) = pipe(Some(O_CLOEXEC)) else {
        return false;
    };
    let mut ok = (r, w) == (3, 4);
    ok &= call(SYS_FCNTL, r, F_GETFD, 0) == (FD_CLOEXEC, false);
    ok &= call(SYS_FCNTL, w, F_GETFD, 0) == (FD_CLOEXEC, false);
    ok &= call(SYS_FSTAT, r, st.as_mut_ptr() as usize, 0) == (0, false);
    ok &= (st[0] as u32) & S_IFMT == S_IFIFO;

    // A short write, read back in two pieces.
    ok &= write(w, b"ping!") == Ok(5);
    ok &= call(SYS_IOCTL, r, FIONREAD, &mut nread as *mut u32 as usize) == (0, false);
    ok &= nread == 5;
    ok &= call(SYS_READ, r, buf.as_mut_ptr() as usize, 2) == (2, false) && buf[..2] == *b"pi";
    ok &= call(SYS_READ, r, buf.as_mut_ptr() as usize, buf.len()) == (3, false);
    ok &= buf[..3] == *b"ng!";

    // A write larger than PIPE_SIZE grows the buffer and is read back intact.
    for (i, b) in BIG.iter().enumerate() {
        b.store((i % 251) as u8, Ordering::Relaxed);
    }
    ok &= call(SYS_WRITE, w, BIG.as_ptr() as usize, BIG_WRITE) == (BIG_WRITE, false);
    let mut got = 0;
    while ok && got < BIG_WRITE {
        let mut chunk = [0u8; 512];
        match call(SYS_READ, r, chunk.as_mut_ptr() as usize, chunk.len()) {
            (n, false) if n > 0 => {
                for (j, &c) in chunk[..n].iter().enumerate() {
                    ok &= c == ((got + j) % 251) as u8;
                }
                got += n;
            }
            _ => ok = false,
        }
    }

    // EOF once the writer is gone.
    ok &= call(SYS_CLOSE, w, 0, 0) == (0, false);
    ok &= call(SYS_READ, r, buf.as_mut_ptr() as usize, buf.len()) == (0, false);
    ok &= call(SYS_CLOSE, r, 0, 0) == (0, false);

    // An empty non-blocking pipe.
    let Some([r, w]) = pipe(Some(O_NONBLOCK)) else {
        return false;
    };
    ok &= call(SYS_READ, r, buf.as_mut_ptr() as usize, buf.len()) == (EAGAIN, true);
    ok &= call(SYS_FCNTL, r, F_GETFD, 0) == (0, false);
    ok &= call(SYS_CLOSE, r, 0, 0) == (0, false);
    ok &= call(SYS_CLOSE, w, 0, 0) == (0, false);

    // No reader: EPIPE and SIGPIPE, caught by a handler, then ignored.
    let catch = Sigaction {
        sa_handler: on_sigpipe as *const () as usize,
        sa_mask: 0,
        sa_flags: 0,
    };
    let ignore = Sigaction {
        sa_handler: SIG_IGN,
        sa_mask: 0,
        sa_flags: 0,
    };
    let Some([r, w]) = pipe(None) else {
        return false;
    };
    ok &= call(SYS_CLOSE, r, 0, 0) == (0, false);
    ok &= call(
        SYS_SIGACTION,
        SIGPIPE,
        &catch as *const Sigaction as usize,
        0,
    ) == (0, false);
    ok &= write(w, b"lost") == Err(EPIPE);
    ok &= SIGPIPES.load(Ordering::Relaxed) == 1;
    ok &= call(
        SYS_SIGACTION,
        SIGPIPE,
        &ignore as *const Sigaction as usize,
        0,
    ) == (0, false);
    ok &= write(w, b"lost") == Err(EPIPE);
    ok &= SIGPIPES.load(Ordering::Relaxed) == 1;
    ok &= call(SYS_SIGPENDING, 0, 0, 0) == (0, false);
    ok &= call(SYS_CLOSE, w, 0, 0) == (0, false);

    ok && call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false)
}

/// `struct msghdr`: the C layout (`#[repr(C)]` pads it the same way).
#[repr(C)]
struct Msghdr {
    msg_name: usize,
    msg_namelen: u32,
    msg_iov: usize,
    msg_iovlen: u32,
    msg_control: usize,
    msg_controllen: u32,
    msg_flags: i32,
}

/// `socketpair(AF_UNIX, type, 0)`: the two descriptors.
fn socketpair(type_: usize) -> Option<[usize; 2]> {
    let mut sv = [0i32; 2];
    let r = syscall6(
        SYS_SOCKETPAIR,
        [AF_UNIX, type_, 0, sv.as_mut_ptr() as usize, 0, 0],
    );
    (r == (0, false)).then_some([sv[0] as usize, sv[1] as usize])
}

/// The socket layer seen from user mode: `AF_UNIX` socket pairs (stream and datagram),
/// `shutdown(2)`, the socket options and names, and a descriptor passed with `SCM_RIGHTS`.
fn sockets() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut buf = [0u8; 16];
    let mut st = [0u64; 16];

    // A stream pair: the two lowest free descriptors, bytes both ways.
    let Some([a, b]) = socketpair(SOCK_STREAM) else {
        return false;
    };
    let mut ok = (a, b) == (3, 4);
    ok &= write(a, b"ping") == Ok(4);
    ok &= call(SYS_READ, b, buf.as_mut_ptr() as usize, buf.len()) == (4, false);
    ok &= buf[..4] == *b"ping";
    ok &= write(b, b"pong!") == Ok(5);
    ok &= call(SYS_READ, a, buf.as_mut_ptr() as usize, buf.len()) == (5, false);
    ok &= buf[..5] == *b"pong!";
    // Nothing to read: a non-blocking receive says EAGAIN.
    ok &= syscall6(
        SYS_RECVFROM,
        [b, buf.as_mut_ptr() as usize, buf.len(), MSG_DONTWAIT, 0, 0],
    ) == (EAGAIN, true);

    // fstat(2) and getsockopt(2) know a socket; getpeername(2) an unbound peer.
    ok &= call(SYS_FSTAT, a, st.as_mut_ptr() as usize, 0) == (0, false);
    ok &= (st[0] as u32) & S_IFMT == S_IFSOCK;
    let mut val: i32 = 0;
    let mut len: u32 = 4;
    ok &= syscall6(
        SYS_GETSOCKOPT,
        [
            a,
            SOL_SOCKET as usize,
            SO_TYPE,
            &mut val as *mut i32 as usize,
            &mut len as *mut u32 as usize,
            0,
        ],
    ) == (0, false);
    ok &= val as usize == SOCK_STREAM && len == 4;
    let mut name = [0u8; 16];
    let mut namelen: u32 = name.len() as u32;
    ok &= call(
        SYS_GETPEERNAME,
        a,
        name.as_mut_ptr() as usize,
        &mut namelen as *mut u32 as usize,
    ) == (0, false);
    ok &= namelen == 16 && name[1] as usize == AF_UNIX;

    // shutdown(2): the peer reads EOF, the writer gets EPIPE (no SIGPIPE asked).
    ok &= call(SYS_SHUTDOWN, a, SHUT_WR, 0) == (0, false);
    ok &= call(SYS_READ, b, buf.as_mut_ptr() as usize, buf.len()) == (0, false);
    ok &= syscall6(
        SYS_SENDTO,
        [a, b"x".as_ptr() as usize, 1, MSG_NOSIGNAL, 0, 0],
    ) == (EPIPE, true);

    // A datagram pair keeps the boundaries; a short read truncates the datagram.
    let Some([c, d]) = socketpair(SOCK_DGRAM) else {
        return false;
    };
    ok &= write(c, b"first") == Ok(5);
    ok &= write(c, b"second") == Ok(6);
    ok &= call(SYS_READ, d, buf.as_mut_ptr() as usize, buf.len()) == (5, false);
    ok &= buf[..5] == *b"first";
    ok &= call(SYS_READ, d, buf.as_mut_ptr() as usize, 3) == (3, false);
    ok &= buf[..3] == *b"sec";
    ok &= syscall6(
        SYS_RECVFROM,
        [d, buf.as_mut_ptr() as usize, buf.len(), MSG_DONTWAIT, 0, 0],
    ) == (EAGAIN, true);

    // SCM_RIGHTS: b goes over the datagram pair and comes back as a new descriptor, which
    // writes to a.
    let mut cmsg = [20u32, SOL_SOCKET, SCM_RIGHTS, 0, b as u32, 0];
    let iov = [b"!".as_ptr() as usize, 1];
    let msg = Msghdr {
        msg_name: 0,
        msg_namelen: 0,
        msg_iov: iov.as_ptr() as usize,
        msg_iovlen: 1,
        msg_control: cmsg.as_mut_ptr() as usize,
        msg_controllen: 24,
        msg_flags: 0,
    };
    ok &= call(SYS_SENDMSG, c, &msg as *const Msghdr as usize, 0) == (1, false);
    let mut rcmsg = [0u32; 8];
    let riov = [buf.as_mut_ptr() as usize, buf.len()];
    let mut rmsg = Msghdr {
        msg_name: 0,
        msg_namelen: 0,
        msg_iov: riov.as_ptr() as usize,
        msg_iovlen: 1,
        msg_control: rcmsg.as_mut_ptr() as usize,
        msg_controllen: 32,
        msg_flags: 0,
    };
    ok &= call(SYS_RECVMSG, d, &mut rmsg as *mut Msghdr as usize, 0) == (1, false);
    ok &= buf[0] == b'!' && rmsg.msg_controllen == 20;
    ok &= rcmsg[..3] == [20, SOL_SOCKET, SCM_RIGHTS];
    let newfd = rcmsg[4] as usize;
    ok &= newfd == 7;
    ok &= write(newfd, b"fd") == Ok(2);
    ok &= call(SYS_READ, a, buf.as_mut_ptr() as usize, buf.len()) == (2, false);
    ok &= buf[..2] == *b"fd";

    // socket(2) of the local domain, closed at once.
    let (s, err) = call(SYS_SOCKET, AF_UNIX, SOCK_STREAM, 0);
    ok &= !err && s == 8;

    for fd in [a, b, c, d, newfd, s] {
        ok &= call(SYS_CLOSE, fd, 0, 0) == (0, false);
    }
    ok &= stream_order();
    ok && call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false)
}

/// Pages the receiver of [`stream_order`] reads into: the first write to each is a page
/// fault inside `soreceive`'s copy, which runs without the receive buffer's mutex.
static STREAM_PAGES: [AtomicU8; 64 * PAGE_SIZE] = [const { AtomicU8::new(0) }; 64 * PAGE_SIZE];

/// A stream pair loses and reorders nothing while a writer and a reader on other CPUs race:
/// a forked child writes `STREAM_WORDS` 4-byte sequence numbers, one `write(2)` each, and
/// this process reads them back 4 bytes at a time, as tcpdump(8)'s privilege separation
/// exchanges its commands. Each small write lands in the tail mbuf of the receive buffer
/// (`sbcompress`) while `soreceive` may be copying out of that very mbuf; a reader that
/// trusted the mbuf length it saw before the copy freed the appended bytes (M11e).
fn stream_order() -> bool {
    const STREAM_WORDS: u32 = 40000;
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let Some([a, b]) = socketpair(SOCK_STREAM) else {
        return false;
    };
    let pid = match call(SYS_FORK, 0, 0, 0) {
        (0, false) => {
            call(SYS_CLOSE, b, 0, 0);
            for i in 0..STREAM_WORDS {
                if write(a, &i.to_le_bytes()) != Ok(4) {
                    exit(1);
                }
            }
            exit(0)
        }
        (pid, false) => pid,
        _ => return false,
    };
    let mut ok = call(SYS_CLOSE, a, 0, 0) == (0, false);
    for i in 0..STREAM_WORDS {
        let i = i as usize;
        let off = (i % 64) * PAGE_SIZE + (i / 64 % (PAGE_SIZE / 4)) * 4;
        let word = &STREAM_PAGES[off..off + 4];
        let mut got = 0;
        while ok && got < 4 {
            match call(SYS_READ, b, word[got..].as_ptr() as usize, 4 - got) {
                (n, false) if n > 0 => got += n,
                _ => ok = false,
            }
        }
        let mut v = [0u8; 4];
        for (x, w) in v.iter_mut().zip(word) {
            *x = w.load(Ordering::Relaxed);
        }
        ok &= u32::from_le_bytes(v) as usize == i;
        if !ok {
            break;
        }
    }
    // Closing the reader first stops a writer still blocked on a full buffer (EPIPE).
    ok &= call(SYS_CLOSE, b, 0, 0) == (0, false);
    let mut status: i32 = 0;
    ok &= call(SYS_WAIT4, pid, &mut status as *mut i32 as usize, 0) == (pid, false);
    ok && status == 0
}

/// `struct wg_interface_io` (`<net/if_wg.h>`), without its peers.
#[repr(C)]
#[derive(Default)]
struct WgInterfaceIo {
    i_flags: u8,
    _pad0: u8,
    i_port: u16,
    i_rtable: i32,
    i_public: [u8; 32],
    i_private: [u8; 32],
    i_peers_count: usize,
}

/// `struct wg_data_io`.
#[repr(C)]
struct WgDataIo {
    wgd_name: [u8; 16],
    wgd_size: usize,
    wgd_interface: usize,
}

/// RFC 7748, 6.1: Alice's private key.
const ALICE_PRIVATE: [u8; 32] = [
    0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
    0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
];
/// RFC 7748, 6.1: Alice's public key, X25519 of the private key and the base point.
const ALICE_PUBLIC: [u8; 32] = [
    0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54, 0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e, 0xf7, 0x5a,
    0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4, 0xeb, 0xa4, 0xa9, 0x8e, 0xaa, 0x9b, 0x4e, 0x6a,
];

/// `wg(4)` from user mode, as `ifconfig wg0 create wgkey ...` drives it: the interface ioctls
/// go through any socket (`soo_ioctl` hands group `'i'` to `ifioctl`), here a local datagram
/// socket. `wg0` is created (twice: `EEXIST`), keyed with `SIOCSWG`, its public key read back
/// with `SIOCGWG` and checked against RFC 7748, and destroyed (then `SIOCGWG` is `ENXIO`).
fn wg() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let (s, err) = call(SYS_SOCKET, AF_UNIX, SOCK_DGRAM, 0);
    if err {
        return false;
    }
    let mut ifr = [0u8; 32];
    ifr[..3].copy_from_slice(b"wg0");
    let mut name = [0u8; 16];
    name[..3].copy_from_slice(b"wg0");

    let mut ok = call(SYS_IOCTL, s, SIOCIFCREATE, ifr.as_mut_ptr() as usize) == (0, false);
    ok &= call(SYS_IOCTL, s, SIOCIFCREATE, ifr.as_mut_ptr() as usize) == (EEXIST, true);

    let mut iface = WgInterfaceIo {
        i_flags: WG_INTERFACE_HAS_PRIVATE,
        i_private: ALICE_PRIVATE,
        ..WgInterfaceIo::default()
    };
    let mut data = WgDataIo {
        wgd_name: name,
        wgd_size: size_of::<WgInterfaceIo>(),
        wgd_interface: &mut iface as *mut WgInterfaceIo as usize,
    };
    ok &= call(SYS_IOCTL, s, SIOCSWG, &mut data as *mut WgDataIo as usize) == (0, false);

    let mut out = WgInterfaceIo::default();
    let mut data = WgDataIo {
        wgd_name: name,
        wgd_size: size_of::<WgInterfaceIo>(),
        wgd_interface: &mut out as *mut WgInterfaceIo as usize,
    };
    ok &= call(SYS_IOCTL, s, SIOCGWG, &mut data as *mut WgDataIo as usize) == (0, false);
    ok &= data.wgd_size == size_of::<WgInterfaceIo>();
    let both = WG_INTERFACE_HAS_PUBLIC | WG_INTERFACE_HAS_PRIVATE;
    ok &= out.i_flags & both == both;
    ok &= out.i_public == ALICE_PUBLIC;
    ok &= out.i_peers_count == 0;

    ok &= call(SYS_IOCTL, s, SIOCIFDESTROY, ifr.as_mut_ptr() as usize) == (0, false);
    ok &= call(SYS_IOCTL, s, SIOCGWG, &mut data as *mut WgDataIo as usize) == (ENXIO, true);
    ok &= call(SYS_CLOSE, s, 0, 0) == (0, false);
    ok
}

/// `kevent(kq, changes, nchanges, events, nevents, timeout)`: the number of events.
fn kevent(
    kq: usize,
    changes: &[Kevent],
    events: &mut [Kevent],
    timeout: Option<&[i64; 2]>,
) -> (usize, bool) {
    syscall6(
        SYS_KEVENT,
        [
            kq,
            changes.as_ptr() as usize,
            changes.len(),
            events.as_mut_ptr() as usize,
            events.len(),
            timeout.map_or(0, |t| t.as_ptr() as usize),
        ],
    )
}

/// `kern_event.c` seen from user mode: a `kqueue(2)` reports a pipe becoming readable (with
/// its byte count and the caller's `udata`) and its writer going away (`EV_EOF`); `poll(2)`
/// and `select(2)` see the same pipe readable and writable; `kevent(2)` without a timeout
/// sleeps until a one-shot `EVFILT_TIMER` fires, which then is gone. The socket filters
/// (`uipc_socket.c`) do the same for a stream socket pair ([`socket_events`]).
fn kqueues() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let zero = [0i64, 0];
    let mut ev = [Kevent::default(); 2];
    let mut buf = [0u8; 8];

    let Some([r, w]) = pipe(None) else {
        return false;
    };
    let (kq, failed) = call(SYS_KQUEUE, 0, 0, 0);
    if failed {
        return false;
    }

    // EV_ADD on the read end: registered, nothing pending yet.
    let add = [Kevent {
        ident: r,
        filter: EVFILT_READ,
        flags: EV_ADD,
        udata: 0x5eed,
        ..Kevent::default()
    }];
    let mut ok = kevent(kq, &add, &mut [], None) == (0, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (0, false);

    // A write makes it readable: one event, with the byte count and the udata.
    ok &= write(w, b"kq!") == Ok(3);
    ok &= kevent(kq, &[], &mut ev, None) == (1, false);
    ok &= ev[0].ident == r && ev[0].filter == EVFILT_READ;
    ok &= ev[0].data == 3 && ev[0].udata == 0x5eed && ev[0].flags & EV_EOF == 0;

    // poll(2): the read end is readable, the write end writable.
    let mut pfds = [
        Pollfd {
            fd: r as i32,
            events: POLLIN,
            revents: 0,
        },
        Pollfd {
            fd: w as i32,
            events: POLLOUT,
            revents: 0,
        },
    ];
    ok &= call(SYS_POLL, pfds.as_mut_ptr() as usize, 2, 0) == (2, false);
    ok &= pfds[0].revents == POLLIN && pfds[1].revents == POLLOUT;

    // select(2): the same, through fd sets.
    let mut rset = [1u32 << r];
    let mut wset = [1u32 << w];
    let tv0 = [0i64, 0];
    let nd = w.max(r) + 1;
    ok &= syscall6(
        SYS_SELECT,
        [
            nd,
            rset.as_mut_ptr() as usize,
            wset.as_mut_ptr() as usize,
            0,
            tv0.as_ptr() as usize,
            0,
        ],
    ) == (2, false);
    ok &= rset[0] == 1 << r && wset[0] == 1 << w;

    // Drained: nothing pending.
    ok &= call(SYS_READ, r, buf.as_mut_ptr() as usize, buf.len()) == (3, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (0, false);

    // A one-shot 20 ms timer: kevent(2) without a timeout sleeps until it fires.
    let timer = [Kevent {
        ident: 1,
        filter: EVFILT_TIMER,
        flags: EV_ADD | EV_ONESHOT,
        data: 20,
        ..Kevent::default()
    }];
    ok &= kevent(kq, &timer, &mut ev[..1], None) == (1, false);
    ok &= ev[0].ident == 1 && ev[0].filter == EVFILT_TIMER && ev[0].data == 1;
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (0, false);

    // The writer goes away: EOF on the read end, POLLHUP for poll(2).
    ok &= call(SYS_CLOSE, w, 0, 0) == (0, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (1, false);
    ok &= ev[0].ident == r && ev[0].flags & EV_EOF != 0;
    let mut pfd = [Pollfd {
        fd: r as i32,
        events: POLLIN,
        revents: 0,
    }];
    ok &= call(SYS_POLL, pfd.as_mut_ptr() as usize, 1, 0) == (1, false);
    ok &= pfd[0].revents == POLLIN | POLLHUP;

    ok &= call(SYS_CLOSE, r, 0, 0) == (0, false);
    ok &= socket_events(kq);
    ok &= call(SYS_CLOSE, kq, 0, 0) == (0, false);
    ok && call(SYS_GETDTABLECOUNT, 0, 0, 0) == (3, false)
}

/// The socket half of [`kqueues`] on the kqueue `kq`: a stream pair's read knote fires on
/// the peer's write (with the byte count) and on its close (`EV_EOF`); `poll(2)` sees one
/// end readable and the other writable, then the hang-up.
fn socket_events(kq: usize) -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let zero = [0i64, 0];
    let mut ev = [Kevent::default(); 2];
    let mut buf = [0u8; 8];

    let Some([a, b]) = socketpair(SOCK_STREAM) else {
        return false;
    };

    // EV_ADD of EVFILT_READ on a: registered, nothing to read yet.
    let add = [Kevent {
        ident: a,
        filter: EVFILT_READ,
        flags: EV_ADD,
        udata: 0x50c,
        ..Kevent::default()
    }];
    let mut ok = kevent(kq, &add, &mut [], None) == (0, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (0, false);

    // b writes: the knote fires (sowakeup's knote_locked) with the byte count.
    ok &= write(b, b"sock") == Ok(4);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (1, false);
    ok &= ev[0].ident == a && ev[0].filter == EVFILT_READ;
    ok &= ev[0].data == 4 && ev[0].udata == 0x50c && ev[0].flags & EV_EOF == 0;

    // poll(2): a is readable, b writable.
    let mut pfds = [
        Pollfd {
            fd: a as i32,
            events: POLLIN,
            revents: 0,
        },
        Pollfd {
            fd: b as i32,
            events: POLLOUT,
            revents: 0,
        },
    ];
    ok &= call(SYS_POLL, pfds.as_mut_ptr() as usize, 2, 0) == (2, false);
    ok &= pfds[0].revents == POLLIN && pfds[1].revents == POLLOUT;

    // Drained: nothing pending, and poll(2) finds a not readable.
    ok &= call(SYS_READ, a, buf.as_mut_ptr() as usize, buf.len()) == (4, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (0, false);
    ok &= call(SYS_POLL, pfds.as_mut_ptr() as usize, 1, 0) == (0, false);
    ok &= pfds[0].revents == 0;

    // b goes away: EOF on a's knote, POLLHUP for poll(2) (a is disconnected).
    ok &= call(SYS_CLOSE, b, 0, 0) == (0, false);
    ok &= kevent(kq, &[], &mut ev, Some(&zero)) == (1, false);
    ok &= ev[0].ident == a && ev[0].flags & EV_EOF != 0;
    ok &= call(SYS_POLL, pfds.as_mut_ptr() as usize, 1, 0) == (1, false);
    ok &= pfds[0].revents == POLLIN | POLLHUP;

    ok && call(SYS_CLOSE, a, 0, 0) == (0, false)
}

/// `AF_INET`.
const AF_INET: usize = 2;
/// `SOCK_RAW`.
const SOCK_RAW: usize = 3;
/// `IPPROTO_ICMP`.
const IPPROTO_ICMP: usize = 1;
/// `SO_RCVTIMEO`.
const SO_RCVTIMEO: usize = 0x1006;
/// `SIOCGIFADDR`: `_IOWR('i', 33, struct ifreq)`.
const SIOCGIFADDR: usize = 0xc020_6921;
/// `bind(2)`.
const SYS_BIND: usize = 104;
/// `setsockopt(2)`.
const SYS_SETSOCKOPT: usize = 105;
/// The guest's address under QEMU's user network, which the boot self-test configures.
const INET_ADDR: [u8; 4] = [10, 0, 2, 15];
/// QEMU's gateway, which answers ICMP echo.
const INET_GATEWAY: [u8; 4] = [10, 0, 2, 2];

/// A `struct sockaddr_in` for `addr:port`, as its 16 bytes.
fn sockaddr_in(addr: [u8; 4], port: u16) -> [u8; 16] {
    let mut sin = [0u8; 16];
    sin[0] = 16;
    sin[1] = AF_INET as u8;
    sin[2..4].copy_from_slice(&port.to_be_bytes());
    sin[4..8].copy_from_slice(&addr);
    sin
}

/// The Internet sockets from user mode: an interface address through a UDP socket's
/// `ioctl(2)`, an ICMP echo to the gateway and its reply through a raw socket, and a UDP
/// datagram between two sockets of this host.
fn inet() -> bool {
    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut ok = true;

    // ioctl(SIOCGIFADDR) on a UDP socket: the address of vio0.
    let (u1, err) = call(SYS_SOCKET, AF_INET, SOCK_DGRAM, 0);
    if err {
        return false;
    }
    let mut ifr = [0u8; 32];
    ifr[..4].copy_from_slice(b"vio0");
    ok &= call(SYS_IOCTL, u1, SIOCGIFADDR, ifr.as_mut_ptr() as usize) == (0, false);
    ok &= ifr[16..24] == sockaddr_in(INET_ADDR, 0)[..8];

    // ping -c 1 10.0.2.2: an echo request from a raw ICMP socket, the reply read back with
    // its IP header (at most three seconds).
    let (r, err) = call(SYS_SOCKET, AF_INET, SOCK_RAW, IPPROTO_ICMP);
    if err {
        return false;
    }
    let tv = [3u64, 0];
    ok &= syscall6(
        SYS_SETSOCKOPT,
        [
            r,
            SOL_SOCKET as usize,
            SO_RCVTIMEO,
            tv.as_ptr() as usize,
            16,
            0,
        ],
    ) == (0, false);
    // type 8 (echo), code 0, checksum, id 0x4d39, sequence 1, eight bytes of data.
    let mut echo = [
        8u8, 0, 0, 0, 0x4d, 0x39, 0, 1, b'E', b'm', b'i', b'B', b'S', b'D', b'!', 0,
    ];
    let mut sum = 0u32;
    for w in echo.chunks(2) {
        sum += u32::from(u16::from_be_bytes([w[0], w[1]]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    echo[2..4].copy_from_slice(&(!(sum as u16)).to_be_bytes());
    let gw = sockaddr_in(INET_GATEWAY, 0);
    ok &= syscall6(
        SYS_SENDTO,
        [
            r,
            echo.as_ptr() as usize,
            echo.len(),
            0,
            gw.as_ptr() as usize,
            16,
        ],
    ) == (echo.len(), false);
    let mut buf = [0u8; 128];
    let mut from = [0u8; 16];
    let mut fromlen: u32 = 16;
    let (n, err) = syscall6(
        SYS_RECVFROM,
        [
            r,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            from.as_mut_ptr() as usize,
            &mut fromlen as *mut u32 as usize,
        ],
    );
    ok &= !err && n == 20 + echo.len();
    ok &= buf[0] == 0x45 && buf[9] == IPPROTO_ICMP as u8 && buf[12..16] == INET_GATEWAY;
    ok &= buf[20] == 0 && buf[24..28] == echo[4..8] && buf[28..36] == echo[8..];
    ok &= fromlen == 16 && from[4..8] == INET_GATEWAY;

    // A UDP datagram from one socket to another bound to our address.
    let (u2, err) = call(SYS_SOCKET, AF_INET, SOCK_DGRAM, 0);
    if err {
        return false;
    }
    let to = sockaddr_in(INET_ADDR, 7777);
    ok &= call(SYS_BIND, u1, to.as_ptr() as usize, 16) == (0, false);
    ok &= syscall6(
        SYS_SETSOCKOPT,
        [
            u1,
            SOL_SOCKET as usize,
            SO_RCVTIMEO,
            tv.as_ptr() as usize,
            16,
            0,
        ],
    ) == (0, false);
    ok &= syscall6(
        SYS_SENDTO,
        [
            u2,
            b"udp!".as_ptr() as usize,
            4,
            0,
            to.as_ptr() as usize,
            16,
        ],
    ) == (4, false);
    let mut fromlen: u32 = 16;
    let (n, err) = syscall6(
        SYS_RECVFROM,
        [
            u1,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            from.as_mut_ptr() as usize,
            &mut fromlen as *mut u32 as usize,
        ],
    );
    ok &= !err && n == 4 && buf[..4] == *b"udp!";
    ok &= from[4..8] == INET_ADDR && from[2..4] != [0, 0];

    for fd in [u1, r, u2] {
        ok &= call(SYS_CLOSE, fd, 0, 0) == (0, false);
    }
    ok
}

/// TCP over the loopback from user mode (`netinet/tcp_*.c`): what `ifconfig lo0 inet
/// 127.0.0.1/8 up` does, then a listener on 127.0.0.1, a non-blocking connect(2) to it
/// completed with poll(2) (SYN, SYN-ACK through the SYN cache, ACK), accept(2), a line each
/// way, a half close (the FIN reads as end of file on the other side) and both closes. Every
/// wait is bounded (poll timeouts, `SO_RCVTIMEO`), so a stuck handshake or FIN fails the
/// check instead of hanging the boot.
fn tcp() -> bool {
    /// `SYS_listen`, `SYS_connect`, `SYS_accept`.
    const SYS_LISTEN: usize = 106;
    const SYS_CONNECT: usize = 98;
    const SYS_ACCEPT: usize = 30;
    /// `F_SETFL`.
    const F_SETFL: usize = 4;
    /// `SO_ERROR`.
    const SO_ERROR: usize = 0x1007;
    /// `EINPROGRESS`.
    const EINPROGRESS: usize = 36;
    /// `SIOCAIFADDR` (`_IOW('i', 26, struct in_aliasreq)`), `SIOCGIFFLAGS`, `SIOCSIFFLAGS`.
    const SIOCAIFADDR: usize = 0x8040_691a;
    const SIOCGIFFLAGS: usize = 0xc020_6911;
    const SIOCSIFFLAGS: usize = 0x8020_6910;
    /// `IFF_UP`.
    const IFF_UP: u16 = 0x1;
    /// 127.0.0.1.
    const LOOPBACK: [u8; 4] = [127, 0, 0, 1];
    /// The listener's port.
    const PORT: u16 = 7778;

    let call = |n, a, b, c| syscall3(n, a, b, c);
    let mut ok = true;

    let (l, err) = call(SYS_SOCKET, AF_INET, SOCK_STREAM, 0);
    if err {
        return false;
    }

    // ifconfig lo0 inet 127.0.0.1/8 up
    let mut ifra = [0u8; 64];
    ifra[..3].copy_from_slice(b"lo0");
    ifra[16..32].copy_from_slice(&sockaddr_in(LOOPBACK, 0));
    ifra[48..64].copy_from_slice(&sockaddr_in([255, 0, 0, 0], 0));
    let (_, err) = call(SYS_IOCTL, l, SIOCAIFADDR, ifra.as_mut_ptr() as usize);
    ok &= !err;
    let mut ifr = [0u8; 32];
    ifr[..3].copy_from_slice(b"lo0");
    ok &= call(SYS_IOCTL, l, SIOCGIFFLAGS, ifr.as_mut_ptr() as usize) == (0, false);
    let flags = u16::from_ne_bytes([ifr[16], ifr[17]]) | IFF_UP;
    ifr[16..18].copy_from_slice(&flags.to_ne_bytes());
    ok &= call(SYS_IOCTL, l, SIOCSIFFLAGS, ifr.as_mut_ptr() as usize) == (0, false);

    // listen(2) on 127.0.0.1:7778
    let sin = sockaddr_in(LOOPBACK, PORT);
    ok &= call(SYS_BIND, l, sin.as_ptr() as usize, 16) == (0, false);
    ok &= call(SYS_LISTEN, l, 5, 0) == (0, false);

    // A non-blocking connect(2): EINPROGRESS, then writable within three seconds, no error.
    let (c, err) = call(SYS_SOCKET, AF_INET, SOCK_STREAM, 0);
    if err {
        return false;
    }
    ok &= call(SYS_FCNTL, c, F_SETFL, O_NONBLOCK) == (0, false);
    ok &= call(SYS_CONNECT, c, sin.as_ptr() as usize, 16) == (EINPROGRESS, true);
    let mut pfd = [Pollfd {
        fd: c as i32,
        events: POLLOUT,
        revents: 0,
    }];
    ok &= call(SYS_POLL, pfd.as_mut_ptr() as usize, 1, 3000) == (1, false);
    ok &= pfd[0].revents & POLLOUT != 0;
    let mut soerr = 0u32;
    let mut len = 4u32;
    ok &= syscall6(
        SYS_GETSOCKOPT,
        [
            c,
            SOL_SOCKET as usize,
            SO_ERROR,
            &mut soerr as *mut u32 as usize,
            &mut len as *mut u32 as usize,
            0,
        ],
    ) == (0, false);
    ok &= soerr == 0;
    ok &= call(SYS_FCNTL, c, F_SETFL, 0) == (0, false);

    // accept(2) once the listener is readable.
    let mut pfd = [Pollfd {
        fd: l as i32,
        events: POLLIN,
        revents: 0,
    }];
    ok &= call(SYS_POLL, pfd.as_mut_ptr() as usize, 1, 3000) == (1, false);
    let mut from = [0u8; 16];
    let mut fromlen = 16u32;
    let (a, err) = call(
        SYS_ACCEPT,
        l,
        from.as_mut_ptr() as usize,
        &mut fromlen as *mut u32 as usize,
    );
    if err {
        return false;
    }
    ok &= fromlen == 16 && from[4..8] == LOOPBACK && from[2..4] != [0, 0];

    let tv = [3u64, 0];
    for fd in [a, c] {
        ok &= syscall6(
            SYS_SETSOCKOPT,
            [
                fd,
                SOL_SOCKET as usize,
                SO_RCVTIMEO,
                tv.as_ptr() as usize,
                16,
                0,
            ],
        ) == (0, false);
    }
    // Reads `want` bytes from `fd` (the stream may deliver them in pieces).
    let read_all = |fd: usize, want: &[u8]| {
        let mut buf = [0u8; 16];
        let mut got = 0;
        while got < want.len() {
            let (n, err) = call(
                SYS_READ,
                fd,
                buf[got..].as_mut_ptr() as usize,
                want.len() - got,
            );
            if err || n == 0 {
                return false;
            }
            got += n;
        }
        buf[..got] == *want
    };

    // A line each way.
    ok &= write(c, b"tcp syn ok\n") == Ok(11);
    ok &= read_all(a, b"tcp syn ok\n");
    ok &= write(a, b"tcp ack ok\n") == Ok(11);
    ok &= read_all(c, b"tcp ack ok\n");

    // shutdown(SHUT_WR): the FIN reads as end of file; then the other side closes.
    ok &= call(SYS_SHUTDOWN, c, SHUT_WR, 0) == (0, false);
    let mut buf = [0u8; 4];
    ok &= call(SYS_READ, a, buf.as_mut_ptr() as usize, buf.len()) == (0, false);
    ok &= call(SYS_CLOSE, a, 0, 0) == (0, false);
    ok &= call(SYS_READ, c, buf.as_mut_ptr() as usize, buf.len()) == (0, false);
    for fd in [c, l] {
        ok &= call(SYS_CLOSE, fd, 0, 0) == (0, false);
    }
    ok
}

/// `PF_KEY` (`pseudo_AF_KEY`).
const PF_KEY: usize = 30;
/// `PF_KEY_V2`.
const PF_KEY_V2: u8 = 2;
/// `SADB_REGISTER`.
const SADB_REGISTER: u8 = 7;
/// `SADB_SATYPE_ESP`.
const SADB_SATYPE_ESP: u8 = 2;
/// `SADB_EXT_SUPPORTED_AUTH`, the first extension of the `SADB_REGISTER` reply.
const SADB_EXT_SUPPORTED_AUTH: u16 = 14;

/// `net/pfkeyv2.c` from user mode, as `isakmpd(8)` and `ipsecctl(8)` start: a PF_KEY socket
/// (`socket(PF_KEY, SOCK_RAW, PF_KEY_V2)`) registers for ESP with `SADB_REGISTER` and reads
/// back the kernel's answer, which lists the supported algorithms.
fn pfkey() -> bool {
    let (s, err) = syscall3(SYS_SOCKET, PF_KEY, SOCK_RAW, usize::from(PF_KEY_V2));
    if err {
        return false;
    }
    let (pid, _) = syscall3(SYS_GETPID, 0, 0, 0);
    // struct sadb_msg: version, type, errno, satype, len (in 8-byte words), reserved, seq, pid.
    let mut msg = [0u8; 16];
    msg[0] = PF_KEY_V2;
    msg[1] = SADB_REGISTER;
    msg[3] = SADB_SATYPE_ESP;
    msg[4..6].copy_from_slice(&2u16.to_ne_bytes());
    msg[8..12].copy_from_slice(&0x4242u32.to_ne_bytes());
    msg[12..16].copy_from_slice(&(pid as u32).to_ne_bytes());
    let mut ok = syscall3(SYS_WRITE, s, msg.as_ptr() as usize, msg.len()) == (msg.len(), false);

    let mut buf = [0u8; 512];
    let (n, err) = syscall6(
        SYS_RECVFROM,
        [s, buf.as_mut_ptr() as usize, buf.len(), MSG_DONTWAIT, 0, 0],
    );
    ok &= !err && n > msg.len();
    ok &= buf[0] == PF_KEY_V2 && buf[1] == SADB_REGISTER && buf[2] == 0;
    ok &= usize::from(u16::from_ne_bytes([buf[4], buf[5]])) * 8 == n;
    ok &= buf[8..16] == msg[8..16];
    ok &= u16::from_ne_bytes([buf[18], buf[19]]) == SADB_EXT_SUPPORTED_AUTH;

    ok && syscall3(SYS_CLOSE, s, 0, 0) == (0, false)
}

/// The `SIGUSR1` handler, entered through the kernel's signal trampoline.
extern "C" fn on_sigusr1(sig: i32) {
    if sig as usize == SIGUSR1 {
        HANDLED.fetch_add(1, Ordering::Relaxed);
    }
}

/// `kern_sig.c` seen from user mode: a caught signal runs its handler on the way back from
/// the system call that sent it, and `sigreturn(2)` resumes the interrupted code with its
/// registers (the system call's return value and error flag included); a blocked signal
/// stays pending until it is unblocked.
fn signals() -> bool {
    let bit = 1usize << (SIGUSR1 - 1);
    let sa = Sigaction {
        sa_handler: on_sigusr1 as *const () as usize,
        sa_mask: 0,
        sa_flags: 0,
    };
    let mut ok =
        syscall3(SYS_SIGACTION, SIGUSR1, &sa as *const Sigaction as usize, 0) == (0, false);
    let mut osa = Sigaction {
        sa_handler: 0,
        sa_mask: 0,
        sa_flags: 0,
    };
    ok &= syscall3(
        SYS_SIGACTION,
        SIGUSR1,
        0,
        &mut osa as *mut Sigaction as usize,
    ) == (0, false);
    ok &= osa.sa_handler == on_sigusr1 as *const () as usize;

    let (pid, _) = syscall3(SYS_GETPID, 0, 0, 0);
    ok &= syscall3(SYS_KILL, pid, SIGUSR1, 0) == (0, false);
    ok &= HANDLED.load(Ordering::Relaxed) == 1;

    // Blocked: pending, not delivered, until the mask lets it through.
    ok &= syscall3(SYS_SIGPROCMASK, SIG_BLOCK, bit, 0) == (0, false);
    ok &= syscall3(SYS_KILL, pid, SIGUSR1, 0) == (0, false);
    ok &= HANDLED.load(Ordering::Relaxed) == 1;
    ok &= syscall3(SYS_SIGPENDING, 0, 0, 0) == (bit, false);
    ok &= syscall3(SYS_SIGPROCMASK, SIG_SETMASK, 0, 0) == (bit, false);
    ok && HANDLED.load(Ordering::Relaxed) == 2
}

/// `kern_prot.c` seen from user mode: init is pid 1, root, not set-id; the TCB set with
/// `__set_tcb(2)` comes back from `__get_tcb(2)` and is in the TLS register.
fn ids_and_tcb() -> bool {
    let mut ok = syscall3(SYS_GETPID, 0, 0, 0) == (1, false);
    ok &= syscall3(SYS_GETUID, 0, 0, 0) == (0, false);
    ok &= syscall3(SYS_ISSETUGID, 0, 0, 0) == (0, false);

    let tcb = &TCB as *const AtomicUsize as usize;
    TCB.store(tcb, Ordering::Relaxed);
    ok &= syscall3(SYS___SET_TCB, tcb, 0, 0) == (0, false);
    ok &= syscall3(SYS___GET_TCB, 0, 0, 0) == (tcb, false);
    ok && tls_register() == tcb
}

/// The TCB as the hardware sees it: the first word at `%fs:0`, which the kernel's FS.base
/// restore makes `TCB`'s own address.
#[cfg(target_arch = "x86_64")]
fn tls_register() -> usize {
    let tcb: usize;
    // SAFETY: reads one word through %fs. FS.base is the TCB set above, a static that is
    // mapped; if the kernel failed to load it, it is 0 and the read faults, which ends init
    // with a signal the smoke test reports.
    unsafe { asm!("mov {}, fs:[0]", out(reg) tcb, options(nostack, readonly, preserves_flags)) };
    tcb
}

/// The TCB as the hardware sees it: `TPIDR_EL0`.
#[cfg(target_arch = "aarch64")]
fn tls_register() -> usize {
    let tcb: usize;
    // SAFETY: reads the user thread pointer register; no memory is touched.
    unsafe { asm!("mrs {}, tpidr_el0", out(reg) tcb, options(nomem, nostack, preserves_flags)) };
    tcb
}

/// Each bss page reads as zero, then holds what was written to it.
fn demand_zero_bss() -> bool {
    let mut ok = true;
    for (page, byte) in BSS.iter().step_by(PAGE_SIZE).enumerate() {
        ok &= byte.load(Ordering::Relaxed) == 0;
        byte.store(page as u8 + 1, Ordering::Relaxed);
    }
    for (page, byte) in BSS.iter().step_by(PAGE_SIZE).enumerate() {
        ok &= byte.load(Ordering::Relaxed) == page as u8 + 1;
    }
    ok
}

/// A panic has nowhere to go: leave with a distinctive status.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(99)
}
