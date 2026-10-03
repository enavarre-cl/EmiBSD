//! `init`: the first user process of EmiBSD, standing in for `init(8)` until there is a
//! filesystem to load one from. Freestanding and static: no libc, the system calls are the
//! raw instructions with OpenBSD's convention (the number in `rax`/`x8`, the error in the
//! carry flag). The kernel loads it as a Limine module and execs it at `start_init`.
//!
//! What it does is the M6 exit criterion: writes one line through `write(2)` and leaves
//! through `exit(2)`.

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

/// `SYS_exit`.
const SYS_EXIT: usize = 1;
/// `SYS_write`.
const SYS_WRITE: usize = 4;

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
    let status = match write(1, b"init: hello from user mode\n") {
        Ok(_) => 0,
        Err(_) => 1,
    };
    exit(status)
}

/// A panic has nowhere to go: leave with a distinctive status.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(99)
}
