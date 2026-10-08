/* <CODE> */
//! Machine-dependent code, one directory per architecture: OpenBSD `sys/arch/<arch>/`.
//!
//! Exactly one implementation is compiled in and re-exported as `current`. Nothing outside
//! `sys/arch/` and `sys/machine/` may name these modules directly.

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub mod amd64;
#[cfg(all(target_os = "none", target_arch = "aarch64"))]
pub mod arm64;
#[cfg(not(target_os = "none"))]
pub mod host;

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub use self::amd64 as current;
#[cfg(all(target_os = "none", target_arch = "aarch64"))]
pub use self::arm64 as current;
#[cfg(not(target_os = "none"))]
pub use self::host as current;
/* </CODE> */
