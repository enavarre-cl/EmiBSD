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
//! Device drivers: OpenBSD `sys/dev/`.
//!
//! Only drivers for hardware QEMU exposes are ported; others are `skipped: deferred-driver`.
//! `cons` is the console framework, `ic/` the chip drivers (`com(4)`, `pluart(4)`), `isa/` the
//! ISA bus definitions amd64 still needs.
//! `i2c/` is the I2C bus (`iic(4)`, M16e).
//! `consfile` is the console-as-a-file stand-in until `/dev/console` exists (not OpenBSD
//! code, `ports.toml` `[[extra]]`).
//! ISA bus definitions amd64 still needs, `pci/` the PCI bus, `puc/` the port drivers of
//! `puc(4)` (`com_puc`), `pv/` the paravirtual devices (`virtio(4)`), `rasops/` the raster
//! operations frame buffers draw text with, `wsfont/` their fonts.

pub mod acpi;
pub mod ata;
pub mod audio;
pub mod audio_if;
pub mod bio;
pub mod biovar;
pub mod clock_subr;
pub mod cons;
pub mod consfile;
pub mod diskmap;
pub mod efi;
pub mod fdt;
pub mod firmload;
pub mod gpio;
pub mod hid;
pub mod i2c;
pub mod ic;
pub mod ipmi;
pub mod ipmivar;
pub mod isa;
pub mod microcode;
pub mod midi;
pub mod midi_if;
pub mod midivar;
pub mod mii;
pub mod mulaw;
pub mod ofw;
pub mod pci;
pub mod pckbc;
pub mod puc;
pub mod pv;
pub mod rasops;
pub mod rd;
pub mod rnd;
pub mod sdmmc;
pub mod softraid;
pub mod softraid_concat;
pub mod softraid_crypto;
pub mod softraid_raid0;
pub mod softraid_raid1;
pub mod softraid_raid1c;
pub mod softraid_raid5;
pub mod softraid_raid6;
pub mod softraidvar;
pub mod usb;
pub mod vnd;
pub mod vndioctl;
pub mod wscons;
pub mod wsfont;
/* </CODE> */
