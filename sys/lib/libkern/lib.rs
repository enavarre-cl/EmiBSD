/* <CODE> */
//! Freestanding kernel C library: OpenBSD `sys/lib/libkern` and its header `libkern.h`.
//!
//! Upstream: sys/lib/libkern/libkern.h @ 3ce1f3f79392
//!
//! One module per C file (`strlcpy.c` → `strlcpy.rs`), each function re-exported at the crate
//! root so callers write `libkern::strlcpy(..)`. Functions whose semantics `core` already
//! provides exactly (`memcpy`, `strlen`, `qsort`) are not ported; see `ports.toml`
//! (`skipped: provided-by-core`). This crate has no dependencies and must stay that way.
//!
//! String arguments are byte slices: a C string ends at its first NUL or at the end of the
//! slice, whichever comes first, and a destination's size is its slice length.
//!
//! `libkern.h`'s prototypes are the re-exports below; its `imax`/`min`/`abs` family is
//! `Ord::max`, `Ord::min` and `i32::abs`; `KASSERT`/`KDASSERT` are `kassert!`/`kdassert!` in
//! `sys/kern/subr_prf.rs`, next to the `__assert` they call (this crate cannot call into `bsd`).
//!
//! [`StaticCell`] is the one project helper here: the `static`-with-interior-mutability every
//! ported global that is neither an atomic nor behind a lock needs (`ports.toml`, `[[extra]]`).

#![no_std]

#[cfg(test)]
extern crate std;

pub mod crc32c;
pub mod explicit_bzero;
pub mod random;
pub mod scanc;
pub mod skpc;
pub mod staticcell;
pub mod strlcat;
pub mod strlcpy;
pub mod strncasecmp;
pub mod strnlen;
pub mod timingsafe_bcmp;

pub use crc32c::crc32c;
pub use explicit_bzero::explicit_bzero;
pub use random::random;
pub use scanc::scanc;
pub use skpc::skpc;
pub use staticcell::StaticCell;
pub use strlcat::strlcat;
pub use strlcpy::strlcpy;
pub use strncasecmp::strncasecmp;
pub use strnlen::strnlen;
pub use timingsafe_bcmp::timingsafe_bcmp;
/* </CODE> */
