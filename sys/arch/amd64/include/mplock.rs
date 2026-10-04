/*	$OpenBSD: mplock.h,v 1.5 2017/10/17 14:25:35 visa Exp $	*/

/* <LICENSES> */
/* public domain */
/* </LICENSES> */

//! amd64 `<machine/mplock.h>`: `__USE_MI_MPLOCK`, the machine-independent ticket lock.
//!
//! Upstream: sys/arch/amd64/include/mplock.h @ 3ce1f3f79392
//!
//! Status: `ported`. The lock is `sys/sys/mplock.rs`, its functions `kern/kern_lock.rs`.

/// `__USE_MI_MPLOCK`.
pub const USE_MI_MPLOCK: bool = true;
