/* <LICENSES> */
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
/* </LICENSES> */

/* <CODE> */
//! The `copyin(9)` family: the `<sys/systm.h>` prototypes `copyin`, `copyout`,
//! `copyinstr`, `copyoutstr` and `kcopy`, implemented by each architecture's `copy.S`
//! (`copystr.S` on arm64) with `pcb_onfault` catching the faults.
//!
//! Milestone M6 (part a) adds them to the contract; the host double copies within its one
//! address space. With `kern_sig.c`, [`copyin_obj`] and [`copyout_obj`] copy a whole ABI
//! structure (`copyin(uaddr, &sa, sizeof(sa))`) for the types marked [`AbiPod`].

use core::mem::MaybeUninit;
use core::slice;

use crate::machine::Machine;
use crate::sys::errno::Errno;

/// The copy routines between user and kernel space.
pub trait UserCopy {
    /// `copyin(uaddr, kaddr, len)`: copies `kbuf.len()` bytes from the user address `uaddr`
    /// into `kbuf`; `EFAULT` when the user range is not mapped or not user space.
    fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno>;

    /// `copyout(kaddr, uaddr, len)`: copies `kbuf` to the user address `uaddr`.
    fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno>;

    /// `copyinstr(uaddr, kaddr, len, done)`: copies a NUL-terminated string from user
    /// space, at most `kbuf.len()` bytes including the NUL; returns the bytes copied
    /// (NUL included), `ENAMETOOLONG` when the string does not fit.
    fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno>;

    /// `copyin32(uaddr, kaddr)`: atomically copies the aligned 32-bit word at `uaddr` in
    /// from user space; `EFAULT` when it is misaligned or not mapped (`futex(2)`).
    fn copyin32(uaddr: usize) -> Result<u32, Errno>;

    /// `copyoutstr(kaddr, uaddr, len, done)`: copies the NUL-terminated string at the start
    /// of `kbuf` to user space (at most `kbuf.len()` bytes); returns the bytes copied.
    fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno>;

    /// `kcopy(src, dst, len)`: a kernel-to-kernel copy that survives a page fault on either
    /// side (`EFAULT`) instead of panicking.
    ///
    /// # Safety
    ///
    /// `src` and `dst` are kernel addresses that are mapped for `len` bytes, or whose fault
    /// is the `EFAULT` the caller expects; the ranges may overlap.
    unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno>;
}

/// A structure the kernel copies to or from user space as it is in memory (`struct
/// sigaction`, `siginfo_t`, `struct sigcontext`, `struct timespec`).
///
/// # Safety
///
/// Implement only for `#[repr(C)]` types made of integers (or arrays and structures of
/// them) without implicit padding: every byte of a value is initialised, and every bit
/// pattern is a valid value.
pub unsafe trait AbiPod: Copy + 'static {}

// SAFETY: a plain `int`: four initialised bytes, any pattern valid (the argument of most tty
// and file ioctls, `*(int *)data`).
unsafe impl AbiPod for i32 {}

// SAFETY: a plain `u_int`: four initialised bytes, any pattern valid (`*(u_int *)data` of the
// bpf(4) ioctls).
unsafe impl AbiPod for u32 {}

/// `copyin(uaddr, &obj, sizeof(obj))`: copies a `T` in from the user address `uaddr`.
pub fn copyin_obj<T: AbiPod>(uaddr: usize) -> Result<T, Errno> {
    let mut obj = MaybeUninit::<T>::zeroed();
    // SAFETY: `T: AbiPod`, so the all-zero bytes are a valid `T` and every byte of it may be
    // viewed and overwritten as a `u8`; the slice covers exactly the object.
    let buf = unsafe { slice::from_raw_parts_mut(obj.as_mut_ptr().cast::<u8>(), size_of::<T>()) };
    copyin(uaddr, buf)?;
    // SAFETY: every bit pattern is a valid `T` (`AbiPod`), whatever `copyin` wrote.
    Ok(unsafe { obj.assume_init() })
}

/// `copyout(&obj, uaddr, sizeof(obj))`: copies `obj` out to the user address `uaddr`.
pub fn copyout_obj<T: AbiPod>(obj: &T, uaddr: usize) -> Result<(), Errno> {
    // SAFETY: `T: AbiPod` has no padding, so all `size_of::<T>()` bytes behind the reference
    // are initialised and may be read as `u8`s while `obj` is borrowed.
    let buf =
        unsafe { slice::from_raw_parts(core::ptr::from_ref(obj).cast::<u8>(), size_of::<T>()) };
    copyout(buf, uaddr)
}

/// `copyin` on the selected machine.
pub fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
    Machine::copyin(uaddr, kbuf)
}

/// `copyout` on the selected machine.
pub fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
    Machine::copyout(kbuf, uaddr)
}

/// `copyinstr` on the selected machine.
pub fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
    Machine::copyinstr(uaddr, kbuf)
}

/// `copyin32` on the selected machine.
pub fn copyin32(uaddr: usize) -> Result<u32, Errno> {
    Machine::copyin32(uaddr)
}

/// `copyoutstr` on the selected machine.
pub fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
    Machine::copyoutstr(kbuf, uaddr)
}

/// `kcopy` on the selected machine.
///
/// # Safety
///
/// As [`UserCopy::kcopy`].
pub unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::kcopy(src, dst, len) }
}
/* </CODE> */
