//! `init`: the first user process of EmiBSD, standing in for `init(8)` until there is a
//! filesystem to load one from. Freestanding and static: no libc, the system calls are the
//! raw instructions with OpenBSD's convention (the number in `rax`/`x8`, the error in the
//! carry flag). The kernel loads it as a Limine module and execs it at `start_init`.
//!
//! What it does is the M6 exit criterion: writes one line through `write(2)` and leaves
//! through `exit(2)`. Since M7a it also writes to bss pages that nothing but the fault
//! handler can provide (exec maps them zero-fill and never touches them), the demand-paging
//! exit criterion.

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

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
    exit(status)
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
