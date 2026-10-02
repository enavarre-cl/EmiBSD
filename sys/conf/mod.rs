//! Kernel configuration data: OpenBSD `sys/conf/`.
//!
//! `config(8)`, the Makefiles and `newvers.sh` are replaced by Cargo features and `xtask`
//! (`docs/ARCHITECTURE.md`); what remains is the C that every kernel compiles, such as
//! `param.c`.

pub mod param;
