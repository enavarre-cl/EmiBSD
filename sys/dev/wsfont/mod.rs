/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

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
