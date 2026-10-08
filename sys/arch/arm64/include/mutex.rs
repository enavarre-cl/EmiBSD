/*	$OpenBSD: mutex.h,v 1.5 2018/01/25 15:06:29 mpi Exp $	*/

/* <CODE> */
//! arm64 `<machine/mutex.h>`: `__USE_MI_MUTEX`, the machine-independent mutex.
//!
//! Upstream: sys/arch/arm64/include/mutex.h @ 3ce1f3f79392
//!
//! Status: `ported`. The mutex is `sys/sys/mutex.rs`.

/// `__USE_MI_MUTEX`.
pub const USE_MI_MUTEX: bool = true;
/* </CODE> */
