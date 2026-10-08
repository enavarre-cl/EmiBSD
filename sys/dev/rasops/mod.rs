/* <CODE> */
//! rasops, raster operations for frame buffer consoles: OpenBSD `sys/dev/rasops/`.
//!
//! `rasops` is `rasops.c` and `<dev/rasops/rasops.h>`, the depth-independent part; the
//! depth modules (`rasops1`, `rasops4`, `rasops8`, `rasops15` for 15 and 16 bits, `rasops24`,
//! `rasops32`) fill in what each depth draws differently; `rasops_masks` and
//! `rasops_bitops` are the bit masks and the bit-blitting helpers of the sub-byte depths.

#[allow(clippy::module_inception)] // rasops.c, the file, in the rasops directory
pub mod rasops;
pub mod rasops1;
pub mod rasops15;
pub mod rasops24;
pub mod rasops32;
pub mod rasops4;
pub mod rasops8;
pub mod rasops_bitops;
pub mod rasops_masks;
/* </CODE> */
