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

/// `SYS_exit`.
const SYS_EXIT: usize = 1;
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
/// `SYS_issetugid`.
const SYS_ISSETUGID: usize = 253;
/// `SYS___set_tcb`.
const SYS___SET_TCB: usize = 329;
/// `SYS___get_tcb`.
const SYS___GET_TCB: usize = 330;

/// `EBADF`.
const EBADF: usize = 9;
/// `EINVAL`.
const EINVAL: usize = 22;
/// `F_DUPFD`, `F_GETFD`, `F_SETFD`, `F_GETFL`, `F_DUPFD_CLOEXEC`.
const F_DUPFD: usize = 0;
const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_DUPFD_CLOEXEC: usize = 10;
/// `FD_CLOEXEC`.
const FD_CLOEXEC: usize = 1;
/// `O_RDWR`.
const O_RDWR: usize = 2;
/// `FIOCLEX`, `FIONCLEX`: `_IO('f', 1)`, `_IO('f', 2)`.
const FIOCLEX: usize = 0x2000_6601;
const FIONCLEX: usize = 0x2000_6602;
/// `S_IFMT`, `S_IFCHR`.
const S_IFMT: u32 = 0o170000;
const S_IFCHR: u32 = 0o020000;

/// The thread control block: its first word points at itself, as the TLS ABIs want, so the
/// TLS register can be checked by reading through it.
static TCB: AtomicUsize = AtomicUsize::new(0);

/// A three-argument system call: the return register and whether the carry flag (OpenBSD's
/// error indication) was set.
#[cfg(target_arch = "x86_64")]
fn syscall3(number: usize, a: usize, b: usize, c: usize) -> (usize, bool) {
    let ret: usize;
    let carry: u8;
    // SAFETY: the `syscall` instruction with the OpenBSD register convention; the kernel
    // owns everything that happens, and clobbers only rcx and r11 besides the outputs.
    unsafe {
        asm!(
            "syscall",
            "setc {carry}",
            carry = out(reg_byte) carry,
            inlateout("rax") number => ret,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    (ret, carry != 0)
}

/// A three-argument system call: `svc #0` followed by the speculation barrier the kernel
/// skips over (`svc_handler` adds 8 to the return address).
#[cfg(target_arch = "aarch64")]
fn syscall3(number: usize, a: usize, b: usize, c: usize) -> (usize, bool) {
    let ret: usize;
    let carry: usize;
    // SAFETY: the `svc` instruction with the OpenBSD register convention; the kernel owns
    // everything that happens and clobbers nothing but the outputs.
    unsafe {
        asm!(
            "svc #0",
            "dsb nsh",
            "isb",
            "cset {carry}, cs",
            carry = out(reg) carry,
            in("x8") number,
            inlateout("x0") a => ret,
            in("x1") b,
            in("x2") c,
            options(nostack)
        );
    }
    (ret, carry != 0)
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

/// The entry point: no stack arguments, no `ps_strings`, nothing to set up.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut status = match write(1, b"init: hello from user mode\n") {
        Ok(_) => 0,
        Err(_) => 1,
    };
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
    if !fds() {
        status = 4;
    }
    exit(status)
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
