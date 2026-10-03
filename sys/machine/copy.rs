//! The `copyin(9)` family: the `<sys/systm.h>` prototypes `copyin`, `copyout`,
//! `copyinstr`, `copyoutstr` and `kcopy`, implemented by each architecture's `copy.S`
//! (`copystr.S` on arm64) with `pcb_onfault` catching the faults.
//!
//! Milestone M6 (part a) adds them to the contract; the host double copies within its one
//! address space.

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
