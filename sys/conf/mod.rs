//! Kernel configuration data: OpenBSD `sys/conf/`.
//!
//! `config(8)`, the Makefiles and `newvers.sh` are replaced by Cargo features and `xtask`
//! (`docs/ARCHITECTURE.md`); what remains is the C that every kernel compiles, such as
//! `param.c`, and `vers.rs`, the strings `newvers.sh` would generate (built from what
//! `build.rs` passes).

pub mod param;
pub mod swapgeneric;
pub mod vers;
