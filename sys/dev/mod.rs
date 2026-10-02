//! Device drivers: OpenBSD `sys/dev/`.
//!
//! Only drivers for hardware QEMU exposes are ported; others are `skipped: deferred-driver`.
//! `cons` is the console framework, `ic/` the chip drivers (`com(4)`, `pluart(4)`), `isa/` the
//! ISA bus definitions amd64 still needs.

pub mod cons;
pub mod ic;
pub mod isa;
