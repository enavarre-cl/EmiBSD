/* <CODE> */
//! wsfont, the raster font list: OpenBSD `sys/dev/wsfont/`.
//!
//! `wsfont` is `wsfont.c` and `<dev/wsfont/wsfont.h>`; the font modules are the font
//! headers GENERIC builds into amd64 and arm64 kernels (Spleen 8x16, 12x24, 16x32, 32x64).

pub mod spleen12x24;
pub mod spleen16x32;
pub mod spleen32x64;
pub mod spleen8x16;
#[allow(clippy::module_inception)] // wsfont.c, the file, in the wsfont directory
pub mod wsfont;
/* </CODE> */
