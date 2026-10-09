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
//! ATA and SATA support: OpenBSD `sys/dev/ata/`. `atascsi` is the SCSI to ATA translation
//! layer of the SATA host controllers (`ahci(4)`), `pmreg` the port multiplier registers;
//! `ata` holds the IDENTIFY and SET FEATURES commands of a wdc(4) channel's drives;
//! `atareg`, `atavar`, `satareg` and `wdvar` are the IDENTIFY block, the drive data and
//! commands, the SATA registers and wd(4)'s softc (M16a).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/ata/ata.c
pub mod ata;
pub mod atareg;
pub mod atascsi;
pub mod atavar;
pub mod pmreg;
pub mod satareg;
pub mod wdvar;
/* </CODE> */
