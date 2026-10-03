//! Device drivers: OpenBSD `sys/dev/`.
//!
//! Only drivers for hardware QEMU exposes are ported; others are `skipped: deferred-driver`.
//! `cons` is the console framework, `ic/` the chip drivers (`com(4)`, `pluart(4)`), `isa/` the
//! ISA bus definitions amd64 still needs.
//! `consfile` is the console-as-a-file stand-in until `/dev/console` exists (not OpenBSD
//! code, `ports.toml` `[[extra]]`).

pub mod cons;
pub mod consfile;
pub mod fdt;
pub mod ic;
pub mod isa;
pub mod ofw;
pub mod rnd;
