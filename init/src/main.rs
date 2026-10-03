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
/// `CLOCK_REALTIME`.
const CLOCK_REALTIME: usize = 0;
/// `CLOCK_MONOTONIC`.
const CLOCK_MONOTONIC: usize = 3;
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
/// `EPIPE`.
const EPIPE: usize = 32;
/// `EAGAIN`.
const EAGAIN: usize = 35;
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
/// and `getentropy`, `sched_yield`, `futex`, `utrace`, `pledge`, `acct`, `setrtable`,
/// `getrtable`, `ypconnect`, `profil` and `sendsyslog` answer as OpenBSD's do here.
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
    ok &= call(
        SYS_PLEDGE,
        c"stdio rpath wpath cpath proc exec".as_ptr() as usize,
        0,
        0,
    ) == (0, false);
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
        status = 9;
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
    if processes() {
        if write(1, b"init: processes ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 10;
    }
    if times() {
        if write(1, b"init: time ok\n").is_err() {
            status = 1;
        }
    } else {
        status = 11;
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
