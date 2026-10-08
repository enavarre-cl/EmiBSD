/* <CODE> */
//! Open Firmware and the flattened device tree: OpenBSD `sys/dev/ofw/`.
//!
//! `fdt` is the blob parser with the `OF_*` accessors; `openfirm` the interface header;
//! `ofw_gpio` and `ofw_pinctrl` the GPIO and pin controller registries, `ofw_misc` the
//! provider registries (register maps, IOMMUs) (M16f).

pub mod fdt;
pub mod ofw_gpio;
pub mod ofw_misc;
pub mod ofw_pinctrl;
pub mod openfirm;
/* </CODE> */
