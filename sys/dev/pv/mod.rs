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
//! Paravirtual devices: OpenBSD `sys/dev/pv/`.
//!
//! `virtioreg` and `virtiovar` are the virtio headers, `virtio` the core the transports
//! (`dev/pci/virtio_pci.rs`, `dev/fdt/virtio_mmio.rs`) and the device drivers share;
//! `if_vio` is the network driver, `vio(4)`; `vioblk` the block driver, `vioblk(4)` (a SCSI
//! adapter), with its header `vioblkreg`, and `vioscsi` the SCSI host adapter driver,
//! `vioscsi(4)`, with `vioscsireg`.

pub mod if_vio;
pub mod vioblk;
pub mod vioblkreg;
pub mod vioscsi;
pub mod vioscsireg;
pub mod virtio;
pub mod virtioreg;
pub mod virtiovar;
/* </CODE> */
