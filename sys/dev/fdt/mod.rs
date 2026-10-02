//! Device-tree attachments of the generic drivers: OpenBSD `sys/dev/fdt/`.
//!
//! `pluart_fdt` finds the console PL011 (M4); the rest attach with autoconfiguration.

pub mod pluart_fdt;
