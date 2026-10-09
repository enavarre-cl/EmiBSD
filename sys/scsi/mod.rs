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
//! The SCSI midlayer: OpenBSD `sys/scsi/`.
//!
//! `scsi_all` holds the command and data formats every SCSI device shares, `scsiconf` the
//! structures that tie an adapter, its `scsibus` and the device drivers together (links,
//! transfers, I/O pools), and `scsi_base` the transfer and pool machinery and the common
//! commands. `scsi_disk` holds the commands and mode pages of disks, `scsi_message` the
//! messages of the parallel bus, `scsi_debug` the per-link debugging bits, `sd`/`sdvar` the
//! disk driver, sd(4), and `cd` the CD-ROM driver, cd(4), with `<scsi/cd.h>`.

pub mod cd;
pub mod scsi_all;
pub mod scsi_base;
pub mod scsi_debug;
pub mod scsi_disk;
pub mod scsi_ioctl;
pub mod scsi_message;
pub mod scsiconf;
pub mod sd;
pub mod sdvar;
/* </CODE> */
