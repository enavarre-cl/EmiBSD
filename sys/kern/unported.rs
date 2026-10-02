//! Visible stubs for subsystems that are not ported yet (`.claude/rules/scope-and-stubs.md`).
//! Not an OpenBSD file.
//!
//! A ported file that reaches into an unported one writes `unported!("uvm_map")`: the first time
//! each site runs it prints one line on the console (and into the message buffer), and it always
//! evaluates to `Errno::ENOSYS`, so the gap is both visible on the serial transcript and
//! propagated as an error where the C would have returned one.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::kern::subr_prf::printf;
use crate::sys::errno::Errno;

/// Reports the gap `name` once per `once` flag and yields `ENOSYS`. [`unported!`] is the way to
/// call it.
pub fn unported(name: &str, once: &AtomicBool) -> Errno {
    if !once.swap(true, Ordering::Relaxed) {
        printf(format_args!("unported: {name}\n"));
    }
    Errno::ENOSYS
}

/// `unported!("name")`: records that `name` is not ported yet (printed once per site) and
/// evaluates to `Errno::ENOSYS`.
#[macro_export]
macro_rules! unported {
    ($name:expr) => {{
        static ONCE: ::core::sync::atomic::AtomicBool =
            ::core::sync::atomic::AtomicBool::new(false);
        $crate::kern::unported::unported($name, &ONCE)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yields_enosys_every_time() {
        assert_eq!(unported!("test gap"), Errno::ENOSYS);
        assert_eq!(unported!("test gap"), Errno::ENOSYS);
    }
}
