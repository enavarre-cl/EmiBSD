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
//! SD/MMC support: OpenBSD `sys/dev/sdmmc/` (M16a). `sdmmc` is the sdmmc(4) bus of a card
//! slot, with `sdmmc_mem` (memory cards), `sdmmc_io` and `sdmmc_cis` (SDIO functions) and
//! `sdmmc_scsi` (the SCSI emulation sd(4) attaches through); `sdhc` is sdhc(4), the SD Host
//! Controller Standard chip driver its attachments (`dev/pci/sdhc_pci`) call;
//! `sdmmcchip`, `sdmmcreg`, `sdmmcvar`, `sdmmc_ioreg`, `sdmmcdevs`, `sdhcreg` and `sdhcvar`
//! are their headers.

pub mod sdhc;
pub mod sdhcreg;
pub mod sdhcvar;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/sdmmc/sdmmc.c
pub mod sdmmc;
pub mod sdmmc_cis;
pub mod sdmmc_io;
pub mod sdmmc_ioreg;
pub mod sdmmc_mem;
pub mod sdmmc_scsi;
pub mod sdmmcchip;
pub mod sdmmcdevs;
pub mod sdmmcreg;
pub mod sdmmcvar;
/* </CODE> */
