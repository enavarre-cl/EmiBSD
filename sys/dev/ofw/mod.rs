/* <CODE> */
//! Open Firmware and the flattened device tree: OpenBSD `sys/dev/ofw/`.
//!
//! `fdt` is the blob parser with the `OF_*` accessors; `openfirm` the interface header;
//! `ofw_misc` the provider registries (register maps, IOMMUs).

pub mod fdt;
pub mod ofw_misc;
pub mod openfirm;
/* </CODE> */
