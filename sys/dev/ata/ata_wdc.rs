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

/*
 * Copyright (c) 1998, 2001 Manuel Bouyer.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 */

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum, by Onno van der Linden and by Manuel Bouyer.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! wd(4)'s block transfers through a wdc(4) channel: `wdc_ata_bio` queues a `struct
//! ata_bio` as a channel transfer, whose start hook first walks a freshly reset drive
//! through recalibration, mode setting, geometry and multi-sector setup
//! (`wdc_ata_ctrl_intr`), then issues READ/WRITE (single, multiple, DMA, 28- or 48-bit LBA
//! or CHS) commands chunk by chunk, moving the data by PIO or through the controller's DMA,
//! and completes the request back to `wddone`.
//!
//! Upstream: sys/dev/ata/ata_wdc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option HIBERNATE` is not configured: `wd_hibernate_io` (which crafts a private softc,
//!   channel and transfer in the hibernate page) is not compiled, as in a kernel without it.
//! - `wdc_ata_bio` takes the `ata_bio` as `&'static`: every caller passes its softc's
//!   `sc_wdc_bio`, which outlives the transfer.
//! - The `goto`s of `_wdc_ata_bio_start` (`again`, `do_pio`, `intr`, `timeout`),
//!   `wdc_ata_bio_intr` (`end`, `timeout`) and the `switch` fallthroughs of
//!   `wdc_ata_ctrl_intr` are labelled blocks and loops.
//! - The label fields the transfer reads (`ata_bio->lp->d_secsize`, ...) are read in place
//!   through `AtaBio::label`, without copying the label onto the stack.
//! - `WDCDEBUG_PRINT` here is `wdcdebug_print!` on `wdcdebug_wd_mask` (feature `wdcdebug`).

use core::ptr::{self, NonNull};
#[cfg(feature = "wdcdebug")]
use core::sync::atomic::AtomicI32;

use crate::dev::ata::ata::ata_dmaerr;
use crate::dev::ata::atavar::{
    AtaDriveDatas, DRIVE_DMA, DRIVE_DMAERR, DRIVE_MODE, DRIVE_UDMA, NXFER, WDC_COMPLETE,
    WDC_QUEUED, WDC_TRY_AGAIN,
};
use crate::dev::ata::wd::wddone;
use crate::dev::ata::wdvar::{
    ATA_CORR, ATA_ITSDONE, ATA_LBA, ATA_LBA48, ATA_POLL, ATA_READ, ATA_SINGLE, AtaBio, DMAMODE,
    DMAMODE_WAIT, ERR_DF, ERR_DMA, ERR_NODEV, ERROR, GEOMETRY, GEOMETRY_WAIT, MULTIMODE,
    MULTIMODE_WAIT, NOERROR, PIOMODE, PIOMODE_WAIT, READY, RECAL, RECAL_WAIT, TIMEOUT, WdSoftc,
};
use crate::dev::ic::wdc::{
    wdc_dmawait, wdc_drvp_chp, wdc_exec_xfer, wdc_free_xfer, wdc_get_xfer, wdc_input_bytes,
    wdc_output_bytes, wdc_set_drive, wdccommand, wdccommandext, wdccommandshort, wdcdebug_print,
    wdcstart,
};
use crate::dev::ic::wdcreg::{
    SET_FEATURES, WDCC_IDP, WDCC_READ, WDCC_READ_EXT, WDCC_READDMA, WDCC_READDMA_EXT,
    WDCC_READMULTI, WDCC_READMULTI_EXT, WDCC_RECAL, WDCC_SETMULTI, WDCC_WRITE, WDCC_WRITE_EXT,
    WDCC_WRITEDMA, WDCC_WRITEDMA_EXT, WDCC_WRITEMULTI, WDCC_WRITEMULTI_EXT, WDCE_ABRT, WDCE_AMNF,
    WDCE_BBK, WDCE_CRC, WDCE_IDNF, WDCE_TK0NF, WDCE_UNC, WDCS_BITS, WDCS_BSY, WDCS_CORR, WDCS_DRDY,
    WDCS_DRQ, WDCS_DWF, WDCS_ERR, WDSD_CHS, WDSD_LBA, WDSF_SET_MODE,
};
use crate::dev::ic::wdcvar::{
    C_DMA, C_POLL, C_PRIVATEXFER, C_TIMEOU, ChannelSoftc, WDC_CAPABILITY_IRQACK,
    WDC_CAPABILITY_MODE, WDC_DMA_LBA48, WDC_DMA_READ, WDC_NOSLEEP, WDC_QUIRK_NOSHORTDMA,
    WDCF_DMA_BEFORE_CMD, WDCF_DMA_WAIT, WDCF_IRQ_WAIT, WdcXfer, wait_for_drq, wait_for_ready,
    wait_for_unbusy, wdcwait,
};
use crate::kassert;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del};
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::machine::cpu::delay;
use crate::sys::errno::Errno::EINVAL;

/// `DEBUG_INTR`.
const DEBUG_INTR: i32 = 0x01;
/// `DEBUG_XFERS`.
const DEBUG_XFERS: i32 = 0x02;
/// `DEBUG_STATUS`.
#[allow(dead_code)] // the C defines it for this file too
const DEBUG_STATUS: i32 = 0x04;
/// `DEBUG_FUNCS`.
const DEBUG_FUNCS: i32 = 0x08;
/// `DEBUG_PROBE`.
#[allow(dead_code)] // the C defines it for this file too
const DEBUG_PROBE: i32 = 0x10;

/// `WDCDEBUG_WD_MASK`.
#[cfg(feature = "wdcdebug")]
const WDCDEBUG_WD_MASK: i32 = 0x00;

/// `ATA_DELAY`: 45s for a drive I/O.
const ATA_DELAY: i32 = 45000;

/// `WDC_ATA_NOERR`: drive doesn't report an error.
pub const WDC_ATA_NOERR: i32 = 0x00;
/// `WDC_ATA_RECOV`: there was a recovered error.
pub const WDC_ATA_RECOV: i32 = 0x01;
/// `WDC_ATA_ERR`: drive reports an error.
pub const WDC_ATA_ERR: i32 = 0x02;

/// `wdcdebug_wd_mask`: the `WDCDEBUG_PRINT` mask of this file and `wd.c`.
#[cfg(feature = "wdcdebug")]
#[allow(non_upper_case_globals)] // the C name
pub static wdcdebug_wd_mask: AtomicI32 = AtomicI32::new(WDCDEBUG_WD_MASK);

// #ifdef HIBERNATE: wd_hibernate_io (not configured).

/// The `struct ata_bio` of a transfer `wdc_ata_bio` queued (`xfer->cmd`).
fn xfer_bio(xfer: &WdcXfer) -> &'static AtaBio {
    // SAFETY: `wdc_ata_bio` sets `cmd` to its `&'static` `ata_bio`.
    unsafe { &*xfer.cmd.get().cast::<AtaBio>().cast_const() }
}

/// `ata_bio->wd`, which `__wdstart` and `wddump` set.
fn bio_wd(ata_bio: &AtaBio) -> &'static WdSoftc {
    match ata_bio.wd.get() {
        // SAFETY: `wd` is the softc whose `sc_wdc_bio` this is, attached while it has
        // transfers.
        Some(wd) => unsafe { wd.as_ref() },
        None => panic(format_args!("ata_bio: no wd")),
    }
}

/// `chp->wdc->irqack(chp)`.
fn irqack(chp: &ChannelSoftc) {
    let wdc = chp.wdc();
    match wdc.irqack.get() {
        Some(f) => f(chp),
        None => panic(format_args!("{}: no irqack", wdc.sc_dev.xname())),
    }
}

/// `wdc_ata_bio`: handles a block I/O operation. Returns `WDC_COMPLETE`, `WDC_QUEUED`, or
/// `WDC_TRY_AGAIN`. Must be called at `splbio()`.
pub fn wdc_ata_bio(drvp: &AtaDriveDatas, ata_bio: &'static AtaBio) -> i32 {
    let chp = wdc_drvp_chp(drvp);

    let Some(xfer) = wdc_get_xfer(WDC_NOSLEEP) else {
        return WDC_TRY_AGAIN;
    };
    if ata_bio.isset(ATA_POLL) {
        xfer.set(C_POLL);
    }
    if !ata_bio.isset(ATA_POLL)
        && drvp.isset(DRIVE_DMA | DRIVE_UDMA)
        && !ata_bio.isset(ATA_SINGLE)
        && (ata_bio.bcount.get() > 512 || chp.wdc().quirks.get() & WDC_QUIRK_NOSHORTDMA == 0)
    {
        xfer.set(C_DMA);
    }
    xfer.drive.set(drvp.drive.get());
    xfer.cmd.set(ptr::from_ref(ata_bio).cast_mut().cast());
    xfer.databuf.set(ata_bio.databuf.get());
    xfer.c_bcount.set(ata_bio.bcount.get() as i32);
    xfer.c_start.set(Some(wdc_ata_bio_start));
    xfer.c_intr.set(Some(wdc_ata_bio_intr));
    xfer.c_kill_xfer.set(Some(wdc_ata_bio_kill_xfer));
    wdc_exec_xfer(chp, xfer);
    if ata_bio.isset(ATA_ITSDONE) {
        WDC_COMPLETE
    } else {
        WDC_QUEUED
    }
}

/// `wdc_ata_bio_start`: the transfer's `c_start`, which starts the timeout machinery.
pub fn wdc_ata_bio_start(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let ata_bio = xfer_bio(xfer);
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wdc_ata_bio_start {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        xfer.drive.get()
    );

    // start timeout machinery
    if !ata_bio.isset(ATA_POLL) {
        let _ = timeout_add_msec(&chp.ch_timo, ATA_DELAY as u64);
    }
    _wdc_ata_bio_start(chp, xfer);
}

/// The `timeout:` exit of `_wdc_ata_bio_start`: the drive never became ready.
fn bio_start_timeout(chp: &ChannelSoftc, xfer: &WdcXfer, drvp: &AtaDriveDatas, ata_bio: &AtaBio) {
    if chp.ch_status.get() == 0xff {
        return;
    }
    let _ = printf(format_args!(
        "{}:{}:{}: not ready, st=0x{}, err=0x{:02x}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        xfer.drive.get(),
        Bitmask(u64::from(chp.ch_status.get()), WDCS_BITS),
        chp.ch_error.get()
    ));
    if wdc_ata_err(drvp, ata_bio) != WDC_ATA_ERR {
        ata_bio.error.set(TIMEOUT);
    }
    wdc_ata_bio_done(chp, xfer);
}

/// `_wdc_ata_bio_start`: issues the next command of the transfer (or the drive's setup
/// first), and pushes the data of a PIO write.
pub fn _wdc_ata_bio_start(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let ata_bio = xfer_bio(xfer);
    let drive = xfer.drive.get();
    let drvp = &chp.ch_drive[usize::from(drive)];
    let mut dma_flags = 0;

    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_INTR | DEBUG_XFERS,
        "_wdc_ata_bio_start {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive
    );
    // Do control operations specially.
    if drvp.state.get() < READY {
        // Actually, we want to be careful not to mess with the control state if the device
        // is currently busy, but we can assume that we never get to this point if that's
        // the case.
        //
        // at this point, we should only be in RECAL state
        if drvp.state.get() != RECAL {
            let _ = printf(format_args!(
                "{}:{}:{}: bad state {} in _wdc_ata_bio_start\n",
                chp.wdc().sc_dev.xname(),
                chp.channel.get(),
                drive,
                drvp.state.get()
            ));
            panic(format_args!("_wdc_ata_bio_start: bad state"));
        }
        xfer.c_intr.set(Some(wdc_ata_ctrl_intr));
        wdc_set_drive(chp, i32::from(drive));
        if wdcwait(chp, WDCS_DRDY, WDCS_DRDY, ATA_DELAY).is_err() {
            bio_start_timeout(chp, xfer, drvp, ata_bio);
            return;
        }
        wdccommandshort(chp, i32::from(drive), WDCC_RECAL);
        drvp.state.set(RECAL_WAIT);
        if !ata_bio.isset(ATA_POLL) {
            chp.set(WDCF_IRQ_WAIT);
        } else {
            // Wait for at last 400ns for status bit to be valid
            delay(1);
            let _ = wdc_ata_ctrl_intr(chp, xfer, 0);
        }
        return;
    }

    if xfer.isset(C_DMA) {
        if drvp.n_xfers.get() <= NXFER {
            drvp.n_xfers.set(drvp.n_xfers.get() + 1);
        }
        dma_flags = if ata_bio.isset(ATA_READ) {
            WDC_DMA_READ
        } else {
            0
        };
        if ata_bio.isset(ATA_LBA48) {
            dma_flags |= WDC_DMA_LBA48;
        }
    }
    // again:
    loop {
        'intr: {
            // When starting a multi-sector transfer, or doing single-sector transfers...
            if xfer.c_skip.get() == 0 || ata_bio.isset(ATA_SINGLE) {
                let (secsize, nsectors, ntracks) =
                    ata_bio.label(|lp| (lp.d_secsize, lp.d_nsectors, lp.d_ntracks));
                let nblks = if ata_bio.isset(ATA_SINGLE) {
                    1
                } else {
                    xfer.c_bcount.get() / secsize as i32
                };
                let (sect, cyl, head): (u8, u16, u8);
                if ata_bio.isset(ATA_LBA) {
                    let blkno = ata_bio.blkno.get();
                    sect = blkno as u8;
                    cyl = (blkno >> 8) as u16;
                    head = ((blkno >> 24) & 0x0f) as u8 | WDSD_LBA;
                } else {
                    let mut blkno = ata_bio.blkno.get() as i32;
                    let mut s = blkno % nsectors as i32;
                    s += 1; // Sectors begin with 1, not 0.
                    sect = s as u8;
                    blkno /= nsectors as i32;
                    let h = blkno % ntracks as i32;
                    blkno /= ntracks as i32;
                    cyl = blkno as u16;
                    head = h as u8 | WDSD_CHS;
                }
                'do_pio: {
                    if xfer.isset(C_DMA) {
                        ata_bio.nblks.set(i64::from(nblks));
                        ata_bio.nbytes.set(xfer.c_bcount.get());
                        let cmd = if ata_bio.isset(ATA_LBA48) {
                            if ata_bio.isset(ATA_READ) {
                                WDCC_READDMA_EXT
                            } else {
                                WDCC_WRITEDMA_EXT
                            }
                        } else if ata_bio.isset(ATA_READ) {
                            WDCC_READDMA
                        } else {
                            WDCC_WRITEDMA
                        };
                        // Init the DMA channel.
                        let wdc = chp.wdc();
                        // SAFETY: the transfer's buffer is the bio's `bcount` bytes at
                        // `databuf` (a buffer's `b_data`), of which `c_skip` are done and
                        // `nbytes` follow; it stays valid until the transfer completes,
                        // after `dma_finish`.
                        let error = unsafe {
                            wdc.dma_init(
                                chp.channel.get(),
                                i32::from(drive),
                                xfer.databuf.get().wrapping_add(xfer.c_skip.get() as usize),
                                ata_bio.nbytes.get() as usize,
                                dma_flags,
                            )
                        };
                        if let Err(error) = error {
                            if error == EINVAL {
                                // We can't do DMA on this transfer for some reason. Fall
                                // back to PIO.
                                xfer.clr(C_DMA);
                                break 'do_pio;
                            }
                            ata_bio.error.set(ERR_DMA);
                            ata_bio.r_error.set(0);
                            wdc_ata_bio_done(chp, xfer);
                            return;
                        }
                        // Initiate command
                        wdc_set_drive(chp, i32::from(drive));
                        if wait_for_ready(chp, ATA_DELAY).is_err() {
                            bio_start_timeout(chp, xfer, drvp, ata_bio);
                            return;
                        }

                        // start the DMA channel (before)
                        if chp.isset(WDCF_DMA_BEFORE_CMD) {
                            wdc.dma_start(chp.channel.get(), i32::from(drive));
                        }

                        if ata_bio.isset(ATA_LBA48) {
                            wdccommandext(
                                chp,
                                drive,
                                cmd,
                                ata_bio.blkno.get() as u64,
                                nblks as u16,
                            );
                        } else {
                            wdccommand(chp, drive, cmd, cyl, head, sect, nblks as u8, 0);
                        }

                        // start the DMA channel (after)
                        if !chp.isset(WDCF_DMA_BEFORE_CMD) {
                            wdc.dma_start(chp.channel.get(), i32::from(drive));
                        }

                        chp.set(WDCF_DMA_WAIT);
                        // wait for irq
                        break 'intr;
                    } // else not DMA
                }
                // do_pio:
                ata_bio.nblks.set(i64::from(nblks.min(ata_bio.multi.get())));
                ata_bio
                    .nbytes
                    .set(ata_bio.nblks.get() as i32 * secsize as i32);
                kassert!(nblks == 1 || !ata_bio.isset(ATA_SINGLE));
                let cmd = if ata_bio.nblks.get() > 1 {
                    if ata_bio.isset(ATA_LBA48) {
                        if ata_bio.isset(ATA_READ) {
                            WDCC_READMULTI_EXT
                        } else {
                            WDCC_WRITEMULTI_EXT
                        }
                    } else if ata_bio.isset(ATA_READ) {
                        WDCC_READMULTI
                    } else {
                        WDCC_WRITEMULTI
                    }
                } else if ata_bio.isset(ATA_LBA48) {
                    if ata_bio.isset(ATA_READ) {
                        WDCC_READ_EXT
                    } else {
                        WDCC_WRITE_EXT
                    }
                } else if ata_bio.isset(ATA_READ) {
                    WDCC_READ
                } else {
                    WDCC_WRITE
                };
                // Initiate command!
                wdc_set_drive(chp, i32::from(drive));
                if wait_for_ready(chp, ATA_DELAY).is_err() {
                    bio_start_timeout(chp, xfer, drvp, ata_bio);
                    return;
                }
                if ata_bio.isset(ATA_LBA48) {
                    wdccommandext(chp, drive, cmd, ata_bio.blkno.get() as u64, nblks as u16);
                } else {
                    wdccommand(chp, drive, cmd, cyl, head, sect, nblks as u8, 0);
                }
            } else if ata_bio.nblks.get() > 1 {
                // The number of blocks in the last stretch may be smaller.
                let secsize = ata_bio.label(|lp| lp.d_secsize);
                let nblks = xfer.c_bcount.get() / secsize as i32;
                if ata_bio.nblks.get() > i64::from(nblks) {
                    ata_bio.nblks.set(i64::from(nblks));
                    ata_bio.nbytes.set(xfer.c_bcount.get());
                }
            }
            // If this was a write and not using DMA, push the data.
            if !ata_bio.isset(ATA_READ) {
                if wait_for_drq(chp, ATA_DELAY).is_err() {
                    let _ = printf(format_args!(
                        "{}:{}:{}: timeout waiting for DRQ, st=0x{}, err=0x{:02x}\n",
                        chp.wdc().sc_dev.xname(),
                        chp.channel.get(),
                        drive,
                        Bitmask(u64::from(chp.ch_status.get()), WDCS_BITS),
                        chp.ch_error.get()
                    ));
                    if wdc_ata_err(drvp, ata_bio) != WDC_ATA_ERR {
                        ata_bio.error.set(TIMEOUT);
                    }
                    wdc_ata_bio_done(chp, xfer);
                    return;
                }
                if wdc_ata_err(drvp, ata_bio) == WDC_ATA_ERR {
                    wdc_ata_bio_done(chp, xfer);
                    return;
                }
                // SAFETY: as for the DMA above: `nbytes` bytes of the bio's buffer past the
                // `c_skip` already written, read here only.
                let data = unsafe { bio_data(xfer, ata_bio) };
                wdc_output_bytes(drvp, Some(&*data), ata_bio.nbytes.get() as u32);
            }
        }

        // intr: Wait for IRQ (either real or polled)
        if !ata_bio.isset(ATA_POLL) {
            chp.set(WDCF_IRQ_WAIT);
        } else {
            // Wait for at last 400ns for status bit to be valid
            delay(1);
            if chp.isset(WDCF_DMA_WAIT) {
                let _ = wdc_dmawait(chp, xfer, ATA_DELAY);
                chp.clr(WDCF_DMA_WAIT);
            }
            let _ = wdc_ata_bio_intr(chp, xfer, 0);
            if !ata_bio.isset(ATA_ITSDONE) {
                continue; // goto again
            }
        }
        return;
    }
}

/// The current chunk of a transfer's buffer: `nbytes` bytes at `databuf + c_skip`.
///
/// # Safety
///
/// `databuf` points to the bio's buffer, of which `c_skip + nbytes` bytes are inside, and
/// nothing else touches the chunk while the slice is live.
#[allow(clippy::mut_from_ref)] // the transfer's buffer, shared by pointer as in the C
unsafe fn bio_data<'a>(xfer: &'a WdcXfer, ata_bio: &AtaBio) -> &'a mut [u8] {
    let n = usize::try_from(ata_bio.nbytes.get()).unwrap_or(0);
    let p = xfer.databuf.get().wrapping_add(xfer.c_skip.get() as usize);
    match NonNull::new(p) {
        // SAFETY: the caller's contract.
        Some(p) => unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), n) },
        None => &mut [],
    }
}

/// `wdc_ata_bio_intr`: the transfer's `c_intr`: checks the command's status, reads the
/// data of a PIO read and moves to the next chunk or completes the transfer.
pub fn wdc_ata_bio_intr(chp: &ChannelSoftc, xfer: &WdcXfer, irq: i32) -> i32 {
    let ata_bio = xfer_bio(xfer);
    let drive = xfer.drive.get();
    let drvp = &chp.ch_drive[usize::from(drive)];

    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_INTR | DEBUG_XFERS,
        "wdc_ata_bio_intr {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive
    );

    // Is it not a transfer, but a control operation?
    if drvp.state.get() < READY {
        let _ = printf(format_args!(
            "{}:{}:{}: bad state {} in wdc_ata_bio_intr\n",
            chp.wdc().sc_dev.xname(),
            chp.channel.get(),
            drive,
            drvp.state.get()
        ));
        panic(format_args!("wdc_ata_bio_intr: bad state"));
    }

    'timeout: {
        // reset on timeout. This will cause extra resets in the case of occasional lost
        // interrupts
        if xfer.isset(C_TIMEOU) {
            break 'timeout;
        }

        // Ack interrupt done by wait_for_unbusy
        if wait_for_unbusy(chp, if irq == 0 { ATA_DELAY } else { 0 }).is_err() {
            if irq != 0 {
                return 0; // IRQ was not for us
            }
            let _ = printf(format_args!(
                "{}:{}:{}: device timeout, c_bcount={}, c_skip{}\n",
                chp.wdc().sc_dev.xname(),
                chp.channel.get(),
                drive,
                xfer.c_bcount.get(),
                xfer.c_skip.get()
            ));

            break 'timeout;
        }
        let wdc = chp.wdc();
        if wdc.has(WDC_CAPABILITY_IRQACK) {
            irqack(chp);
        }

        let mut drv_err = wdc_ata_err(drvp, ata_bio);

        'end: {
            if xfer.isset(C_DMA) {
                if wdc.dma_status.get() != 0 && drv_err != WDC_ATA_ERR {
                    ata_bio.error.set(ERR_DMA);
                    drv_err = WDC_ATA_ERR;
                }
                if chp.ch_status.get() & WDCS_DRQ != 0 && drv_err != WDC_ATA_ERR {
                    let _ = printf(format_args!(
                        "{}:{}:{}: intr with DRQ (st=0x{})\n",
                        wdc.sc_dev.xname(),
                        chp.channel.get(),
                        drive,
                        Bitmask(u64::from(chp.ch_status.get()), WDCS_BITS)
                    ));
                    ata_bio.error.set(TIMEOUT);
                    drv_err = WDC_ATA_ERR;
                }
                if drv_err != WDC_ATA_ERR {
                    break 'end;
                }
                ata_dmaerr(drvp);
            }

            // if we had an error, end
            if drv_err == WDC_ATA_ERR {
                wdc_ata_bio_done(chp, xfer);
                return 1;
            }

            // If this was a read and not using DMA, fetch the data.
            if ata_bio.isset(ATA_READ) {
                if chp.ch_status.get() & WDCS_DRQ != WDCS_DRQ {
                    let _ = printf(format_args!(
                        "{}:{}:{}: read intr before drq\n",
                        wdc.sc_dev.xname(),
                        chp.channel.get(),
                        drive
                    ));
                    ata_bio.error.set(TIMEOUT);
                    wdc_ata_bio_done(chp, xfer);
                    return 1;
                }
                // SAFETY: `nbytes` bytes of the bio's buffer past the `c_skip` already read,
                // filled here only.
                let data = unsafe { bio_data(xfer, ata_bio) };
                wdc_input_bytes(drvp, Some(data), ata_bio.nbytes.get() as u32);
            }
        }
        // end:
        ata_bio.blkno.set(ata_bio.blkno.get() + ata_bio.nblks.get());
        ata_bio
            .blkdone
            .set(ata_bio.blkdone.get() + ata_bio.nblks.get());
        xfer.c_skip.set(xfer.c_skip.get() + ata_bio.nbytes.get());
        xfer.c_bcount
            .set(xfer.c_bcount.get() - ata_bio.nbytes.get());
        // See if this transfer is complete.
        if xfer.c_bcount.get() > 0 {
            if !ata_bio.isset(ATA_POLL) {
                // Start the next operation
                _wdc_ata_bio_start(chp, xfer);
            } else {
                // Let _wdc_ata_bio_start do the loop
                return 1;
            }
        } else {
            // Done with this transfer
            ata_bio.error.set(NOERROR);
            wdc_ata_bio_done(chp, xfer);
        }
        return 1;
    }

    // timeout:
    if xfer.isset(C_DMA) {
        ata_dmaerr(drvp);
    }

    ata_bio.error.set(TIMEOUT);
    wdc_ata_bio_done(chp, xfer);
    1
}

/// `wdc_ata_bio_kill_xfer`: the transfer's `c_kill_xfer`: the channel is going away.
pub fn wdc_ata_bio_kill_xfer(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let ata_bio = xfer_bio(xfer);

    let _ = timeout_del(&chp.ch_timo);
    // remove this command from xfer queue
    wdc_free_xfer(chp, xfer);

    ata_bio.set(ATA_ITSDONE);
    ata_bio.error.set(ERR_NODEV);
    ata_bio.r_error.set(WDCE_ABRT);
    if !ata_bio.isset(ATA_POLL) {
        wdcdebug_print!(wdcdebug_wd_mask, DEBUG_XFERS, "wdc_ata_done: wddone\n");
        wddone(bio_wd(ata_bio));
    }
}

/// `wdc_ata_bio_done`: completes the transfer, hands it back to wd(4) and starts the
/// channel's next one.
pub fn wdc_ata_bio_done(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let ata_bio = xfer_bio(xfer);

    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wdc_ata_bio_done {}:{}:{}: flags 0x{:x}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        xfer.drive.get(),
        xfer.c_flags.get()
    );

    if !xfer.isset(C_PRIVATEXFER) {
        let _ = timeout_del(&chp.ch_timo);
    }

    // feed back residual bcount to our caller
    ata_bio.bcount.set(i64::from(xfer.c_bcount.get()));

    // remove this command from xfer queue
    wdc_free_xfer(chp, xfer);

    ata_bio.set(ATA_ITSDONE);
    if !ata_bio.isset(ATA_POLL) {
        wdcdebug_print!(wdcdebug_wd_mask, DEBUG_XFERS, "wdc_ata_done: wddone\n");
        wddone(bio_wd(ata_bio));
    }
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wdcstart from wdc_ata_done, flags 0x{:x}\n",
        chp.ch_flags.get()
    );
    wdcstart(chp);
}

/// Where `wdc_ata_ctrl_intr` leaves its state machine.
enum CtrlExit {
    /// `goto timeout`.
    Timeout,
    /// `goto error`.
    Error,
}

/// `wdc_ata_ctrl_intr`: implements the operations needed before read/write: recalibrate,
/// set the PIO and DMA modes, the geometry (CHS) and the multi-sector count.
pub fn wdc_ata_ctrl_intr(chp: &ChannelSoftc, xfer: &WdcXfer, irq: i32) -> i32 {
    let ata_bio = xfer_bio(xfer);
    let drive = xfer.drive.get();
    let drvp = &chp.ch_drive[usize::from(drive)];
    let mut errstring = "";
    let delay_ms = if irq == 0 { ATA_DELAY } else { 0 };
    let wdc = chp.wdc();

    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_FUNCS,
        "wdc_ata_ctrl_intr: state {}\n",
        drvp.state.get()
    );

    // A `*_WAIT` state's check: waits for DRDY, acks the interrupt, tests for an error.
    let wait = |what: &'static str, errstring: &mut &'static str| -> Result<(), CtrlExit> {
        *errstring = what;
        if wdcwait(chp, WDCS_DRDY, WDCS_DRDY, delay_ms).is_err() {
            return Err(CtrlExit::Timeout);
        }
        if wdc.has(WDC_CAPABILITY_IRQACK) {
            irqack(chp);
        }
        if chp.ch_status.get() & (WDCS_ERR | WDCS_DWF) != 0 {
            return Err(CtrlExit::Error);
        }
        Ok(())
    };

    // again:
    let exit = 'again: loop {
        let mut st = drvp.state.get();
        // The switch: each `continue` with a new `st` is a FALLTHROUGH or a goto into a
        // later case; `break` leaves the switch with a command issued.
        loop {
            match st {
                RECAL => {
                    // Should not be in this state here
                    panic(format_args!("wdc_ata_ctrl_intr: state==RECAL"));
                }

                RECAL_WAIT => {
                    if let Err(e) = wait("recal", &mut errstring) {
                        break 'again e;
                    }
                    st = PIOMODE; // FALLTHROUGH
                }

                PIOMODE => {
                    // Don't try to set modes if controller can't be adjusted, also don't try
                    // if the drive didn't report its mode, and SET FEATURES 0x08 is only for
                    // PIO mode > 2.
                    if !wdc.has(WDC_CAPABILITY_MODE)
                        || !drvp.isset(DRIVE_MODE)
                        || drvp.PIO_mode.get() <= 2
                    {
                        st = GEOMETRY; // goto geometry
                        continue;
                    }
                    wdccommand(
                        chp,
                        drvp.drive.get(),
                        SET_FEATURES,
                        0,
                        0,
                        0,
                        0x08 | drvp.PIO_mode.get(),
                        WDSF_SET_MODE,
                    );
                    drvp.state.set(PIOMODE_WAIT);
                    break;
                }

                PIOMODE_WAIT => {
                    if let Err(e) = wait("piomode", &mut errstring) {
                        break 'again e;
                    }
                    st = DMAMODE; // FALLTHROUGH
                }

                DMAMODE => {
                    if drvp.isset(DRIVE_UDMA) {
                        wdccommand(
                            chp,
                            drvp.drive.get(),
                            SET_FEATURES,
                            0,
                            0,
                            0,
                            0x40 | drvp.UDMA_mode.get(),
                            WDSF_SET_MODE,
                        );
                    } else if drvp.isset(DRIVE_DMA) {
                        wdccommand(
                            chp,
                            drvp.drive.get(),
                            SET_FEATURES,
                            0,
                            0,
                            0,
                            0x20 | drvp.DMA_mode.get(),
                            WDSF_SET_MODE,
                        );
                    } else {
                        st = GEOMETRY; // goto geometry
                        continue;
                    }
                    drvp.state.set(DMAMODE_WAIT);
                    break;
                }

                DMAMODE_WAIT => {
                    if let Err(e) = wait("dmamode", &mut errstring) {
                        break 'again e;
                    }
                    st = GEOMETRY; // FALLTHROUGH
                }

                GEOMETRY => {
                    // geometry:
                    if ata_bio.isset(ATA_LBA) {
                        st = MULTIMODE; // goto multimode
                        continue;
                    }
                    let (ncylinders, ntracks, nsectors) =
                        ata_bio.label(|lp| (lp.d_ncylinders, lp.d_ntracks, lp.d_nsectors));
                    wdccommand(
                        chp,
                        drive,
                        WDCC_IDP,
                        ncylinders as u16,
                        ntracks.wrapping_sub(1) as u8,
                        0,
                        nsectors as u8,
                        0,
                    );
                    drvp.state.set(GEOMETRY_WAIT);
                    break;
                }

                GEOMETRY_WAIT => {
                    if let Err(e) = wait("geometry", &mut errstring) {
                        break 'again e;
                    }
                    st = MULTIMODE; // FALLTHROUGH
                }

                MULTIMODE => {
                    // multimode:
                    if ata_bio.multi.get() == 1 {
                        st = READY; // goto ready
                        continue;
                    }
                    wdccommand(
                        chp,
                        drive,
                        WDCC_SETMULTI,
                        0,
                        0,
                        0,
                        ata_bio.multi.get() as u8,
                        0,
                    );
                    drvp.state.set(MULTIMODE_WAIT);
                    break;
                }

                MULTIMODE_WAIT => {
                    if let Err(e) = wait("setmulti", &mut errstring) {
                        break 'again e;
                    }
                    st = READY; // FALLTHROUGH
                }

                READY => {
                    // ready:
                    drvp.state.set(READY);
                    // The drive is usable now
                    xfer.c_intr.set(Some(wdc_ata_bio_intr));
                    _wdc_ata_bio_start(chp, xfer);
                    return 1;
                }

                _ => break,
            }
        }

        if !ata_bio.isset(ATA_POLL) {
            chp.set(WDCF_IRQ_WAIT);
            return 1;
        }
        // goto again
    };

    match exit {
        CtrlExit::Timeout => {
            if irq != 0 && !xfer.isset(C_TIMEOU) {
                return 0; // IRQ was not for us
            }
            let _ = printf(format_args!(
                "{}:{}:{}: {} timed out\n",
                wdc.sc_dev.xname(),
                chp.channel.get(),
                drive,
                errstring
            ));
            ata_bio.error.set(TIMEOUT);
            drvp.state.set(0);
            wdc_ata_bio_done(chp, xfer);
            0
        }
        CtrlExit::Error => {
            let _ = printf(format_args!(
                "{}:{}:{}: {} ",
                wdc.sc_dev.xname(),
                chp.channel.get(),
                drive,
                errstring
            ));
            if chp.ch_status.get() & WDCS_DWF != 0 {
                let _ = printf(format_args!("drive fault\n"));
                ata_bio.error.set(ERR_DF);
            } else {
                let _ = printf(format_args!("error ({:x})\n", chp.ch_error.get()));
                ata_bio.r_error.set(chp.ch_error.get());
                ata_bio.error.set(ERROR);
            }
            drvp.state.set(0);
            wdc_ata_bio_done(chp, xfer);
            1
        }
    }
}

/// `wdc_ata_err`: what the status and error registers say of the transfer:
/// `WDC_ATA_NOERR` or `WDC_ATA_ERR` (with `ata_bio->error` set).
pub fn wdc_ata_err(drvp: &AtaDriveDatas, ata_bio: &AtaBio) -> i32 {
    let chp = wdc_drvp_chp(drvp);
    ata_bio.error.set(0);

    let status = chp.ch_status.get();
    if status == 0xff {
        ata_bio.error.set(ERR_NODEV);
        return WDC_ATA_ERR;
    }
    if status & WDCS_BSY != 0 {
        ata_bio.error.set(TIMEOUT);
        return WDC_ATA_ERR;
    }

    if status & WDCS_DWF != 0 {
        ata_bio.error.set(ERR_DF);
        return WDC_ATA_ERR;
    }

    if status & WDCS_ERR != 0 {
        ata_bio.error.set(ERROR);
        ata_bio.r_error.set(chp.ch_error.get());
        if drvp.isset(DRIVE_UDMA) && ata_bio.r_error.get() & WDCE_CRC != 0 {
            // Record the CRC error, to avoid downgrading to multiword DMA
            drvp.set(DRIVE_DMAERR);
        }
        if ata_bio.r_error.get()
            & (WDCE_BBK | WDCE_UNC | WDCE_IDNF | WDCE_ABRT | WDCE_TK0NF | WDCE_AMNF)
            != 0
        {
            return WDC_ATA_ERR;
        }
        return WDC_ATA_NOERR;
    }

    if status & WDCS_CORR != 0 {
        ata_bio.set(ATA_CORR);
    }
    WDC_ATA_NOERR
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    //! A task file emulated in memory behind a channel's register table: one ATA disk as
    //! drive 0, nothing as drive 1. The channel core, the IDENTIFY and SET FEATURES commands
    //! and the polled block transfers run on it unchanged.

    use super::*;
    use crate::dev::ata::ata::ata_get_params;
    use crate::dev::ata::atareg::{Ataparams, WDC_EXT_MODES};
    use crate::dev::ata::atavar::{AT_POLL, CMD_OK, DRIVE_ATA, DRIVE_ATAPI};
    use crate::dev::ic::wdc::{
        wdc_alloc_queue, wdc_default_lba48_write_reg, wdc_probe_caps, wdc_test_reset, wdcprobe,
    };
    use crate::dev::ic::wdcreg::{WDCC_IDENTIFY, WDCC_SETMULTI, WDCS_DSC};
    use crate::dev::ic::wdcvar::{
        ChannelSoftcVtbl, WDC_CAPABILITY_DATA16, WDC_CAPABILITY_MODE, WdcRegs, WdcSoftc,
        wdr_command, wdr_error, wdr_status,
    };
    use crate::sys::disklabel::Disklabel;
    use std::boxed::Box;
    use std::sync::Mutex;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    const NSECT: usize = 64;

    /// The emulated drive.
    struct Fake {
        regs: [u8; 8],
        status: u8,
        disk: Vec<u8>,
        /// Bytes the host reads next (IDENTIFY, READ).
        out: Vec<u8>,
        /// Where the bytes the host writes go: the LBA and how many are still expected.
        write_lba: usize,
        write_left: usize,
        written: Vec<u8>,
        /// The commands received, with their count register.
        log: Vec<(u8, u8)>,
    }

    static FAKE: Mutex<Option<Fake>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
        let mut g = FAKE.lock().unwrap_or_else(|e| e.into_inner());
        f(g.as_mut().expect("fake drive"))
    }

    fn identify() -> Vec<u8> {
        let mut p = Ataparams::zeroed();
        p.atap_config = 0x0040;
        p.atap_cylinders = 1;
        p.atap_heads = 16;
        p.atap_sectors = 63;
        p.atap_multi = 0x8010;
        p.atap_capabilities1 = crate::dev::ata::atareg::WDC_CAP_LBA;
        p.atap_extensions = WDC_EXT_MODES;
        p.atap_piomode_supp = 0x03;
        p.atap_capacity = [NSECT as u16, 0];
        // The model as the drive sends it: big-endian words.
        let model = b"EQUMH RADDSI K";
        p.atap_model[..model.len()].copy_from_slice(model);
        p.as_bytes().to_vec()
    }

    fn drive1(f: &Fake) -> bool {
        f.regs[6] & 0x10 != 0
    }

    fn lba(f: &Fake) -> usize {
        usize::from(f.regs[3])
            | usize::from(f.regs[4]) << 8
            | usize::from(f.regs[5]) << 16
            | usize::from(f.regs[6] & 0x0f) << 24
    }

    fn command(f: &mut Fake, cmd: u8) {
        let count = f.regs[2];
        f.log.push((cmd, count));
        let n = if count == 0 { 256 } else { usize::from(count) };
        match cmd {
            WDCC_IDENTIFY => {
                f.out = identify();
                f.status = WDCS_DRDY | WDCS_DSC | WDCS_DRQ;
            }
            WDCC_READ | WDCC_READMULTI => {
                let l = lba(f);
                f.out = f.disk[l * 512..(l + n) * 512].to_vec();
                f.status = WDCS_DRDY | WDCS_DSC | WDCS_DRQ;
            }
            WDCC_WRITE | WDCC_WRITEMULTI => {
                f.write_lba = lba(f);
                f.write_left = n * 512;
                f.written.clear();
                f.status = WDCS_DRDY | WDCS_DSC | WDCS_DRQ;
            }
            _ => f.status = WDCS_DRDY | WDCS_DSC,
        }
    }

    fn read_reg(_chp: &ChannelSoftc, reg: WdcRegs) -> u8 {
        with(|f| {
            if reg == wdr_status || reg == crate::dev::ic::wdcvar::wdr_altsts {
                if drive1(f) { 0xff } else { f.status }
            } else if reg == wdr_error {
                0
            } else {
                f.regs[reg.offset()]
            }
        })
    }

    fn write_reg(_chp: &ChannelSoftc, reg: WdcRegs, v: u8) {
        with(|f| {
            if reg == wdr_command {
                if !drive1(f) {
                    command(f, v);
                }
            } else if reg.isset(crate::dev::ic::wdcvar::_WDC_AUX) {
                // the control register: reset and interrupt enable, nothing to emulate
            } else {
                f.regs[reg.offset()] = v;
            }
        })
    }

    fn read_raw_2(_chp: &ChannelSoftc, data: Option<&mut [u8]>, nbytes: u32) {
        with(|f| {
            let n = nbytes as usize;
            let chunk: Vec<u8> = f.out.drain(..n.min(f.out.len())).collect();
            if let Some(d) = data {
                d[..chunk.len()].copy_from_slice(&chunk);
            }
            if f.out.is_empty() {
                f.status = WDCS_DRDY | WDCS_DSC;
            }
        })
    }

    fn write_raw_2(_chp: &ChannelSoftc, data: Option<&[u8]>, nbytes: u32) {
        with(|f| {
            let n = nbytes as usize;
            match data {
                Some(d) => f.written.extend_from_slice(&d[..n]),
                None => f.written.extend(core::iter::repeat_n(0, n)),
            }
            f.write_left -= n;
            if f.write_left == 0 {
                let l = f.write_lba * 512;
                let w = core::mem::take(&mut f.written);
                f.disk[l..l + w.len()].copy_from_slice(&w);
                f.status = WDCS_DRDY | WDCS_DSC;
            }
        })
    }

    fn unused_4_r(_chp: &ChannelSoftc, _data: Option<&mut [u8]>, _nbytes: u32) {
        panic!("32-bit transfer on a 16-bit channel");
    }

    fn unused_4_w(_chp: &ChannelSoftc, _data: Option<&[u8]>, _nbytes: u32) {
        panic!("32-bit transfer on a 16-bit channel");
    }

    static FAKE_VTBL: ChannelSoftcVtbl = ChannelSoftcVtbl {
        read_reg,
        write_reg,
        lba48_write_reg: wdc_default_lba48_write_reg,
        read_raw_multi_2: read_raw_2,
        write_raw_multi_2: write_raw_2,
        read_raw_multi_4: unused_4_r,
        write_raw_multi_4: unused_4_w,
    };

    /// A controller with one channel on the fake drive, over fresh kernel memory.
    fn setup() -> (std::sync::MutexGuard<'static, ()>, &'static ChannelSoftc) {
        let g = crate::kern::subr_pool::tests::setup_real_memory();
        wdc_test_reset();
        let disk: Vec<u8> = (0..NSECT * 512).map(|i| (i * 7 + i / 512) as u8).collect();
        *FAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Fake {
            regs: [0; 8],
            status: WDCS_DRDY | WDCS_DSC,
            disk,
            out: Vec::new(),
            write_lba: 0,
            write_left: 0,
            written: Vec::new(),
            log: Vec::new(),
        });

        // SAFETY: a `WdcSoftc` is valid as all zero bits (its `Softc` contract).
        let wdc: &'static WdcSoftc = Box::leak(unsafe { Box::new_zeroed().assume_init() });
        wdc.cap.set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_MODE);
        wdc.PIO_cap.set(4);
        let chp: &'static ChannelSoftc = Box::leak(Box::new(ChannelSoftc::new()));
        chp.wdc.set(Some(NonNull::from(wdc)));
        chp._vtbl.set(Some(&FAKE_VTBL));
        chp.ch_queue.set(wdc_alloc_queue());
        for (i, d) in chp.ch_drive.iter().enumerate() {
            d.chnl_softc.set(ptr::from_ref(chp).cast_mut().cast());
            d.drive.set(i as u8);
        }
        (g, chp)
    }

    #[test]
    fn probe_identify_and_mode_selection() {
        let (_g, chp) = setup();

        // The master answers as an ATA disk, the slave floats.
        assert_eq!(wdcprobe(chp), 0x01);
        let drvp = &chp.ch_drive[0];
        assert!(drvp.isset(DRIVE_ATA));
        assert!(!drvp.isset(DRIVE_ATAPI));

        let mut params = Ataparams::zeroed();
        assert_eq!(ata_get_params(drvp, AT_POLL as u8, &mut params), CMD_OK);
        assert_eq!(&params.atap_model[..14], b"QEMU HARDDISK ");
        assert_eq!(params.atap_heads, 16);
        assert!(chp.queue().sc_xfer.is_empty());

        // PIO 4 is set on the drive (SET FEATURES 0x0c) and kept: the controller does it.
        wdc_probe_caps(drvp, &params);
        assert_eq!(drvp.PIO_mode.get(), 4);
        assert!(drvp.isset(DRIVE_MODE));
        assert_eq!(drvp.ata_vers.get(), 2);
        assert!(with(|f| f.log.contains(&(SET_FEATURES, 0x0c))));
    }

    #[test]
    fn polled_transfers_set_up_the_drive_then_move_data() {
        let (_g, chp) = setup();
        let drvp = &chp.ch_drive[0];
        drvp.set(DRIVE_ATA | DRIVE_MODE);
        drvp.PIO_mode.set(4);

        let mut lp = Disklabel::zeroed();
        lp.d_secsize = 512;
        lp.d_nsectors = 63;
        lp.d_ntracks = 16;
        let lp: &'static mut Disklabel = Box::leak(Box::new(lp));
        let bio: &'static AtaBio = Box::leak(Box::new(AtaBio::new()));
        bio.lp.set(Some(NonNull::from(lp)));
        bio.multi.set(16);
        let buf: &'static mut [u8] = vec![0u8; 3 * 512].leak();

        // A read of three sectors at 5: recalibrate, PIO mode, multiple mode, READ MULTIPLE.
        bio.flags.set(ATA_POLL | ATA_LBA | ATA_READ);
        bio.blkno.set(5);
        bio.bcount.set(3 * 512);
        bio.databuf.set(buf.as_mut_ptr());
        assert_eq!(wdc_ata_bio(drvp, bio), WDC_COMPLETE);
        assert_eq!(bio.error.get(), NOERROR);
        assert_eq!(bio.bcount.get(), 0);
        assert_eq!(drvp.state.get(), READY);
        let expect = with(|f| f.disk[5 * 512..8 * 512].to_vec());
        assert_eq!(&buf[..], &expect[..]);
        let log = with(|f| core::mem::take(&mut f.log));
        assert_eq!(
            log,
            [
                (WDCC_RECAL, 0),
                (SET_FEATURES, 0x0c),
                (WDCC_SETMULTI, 16),
                (WDCC_READMULTI, 3),
            ]
        );

        // A one-sector write at 9, the drive ready: a plain WRITE.
        buf[..512].fill(0xa5);
        bio.flags.set(ATA_POLL | ATA_LBA);
        bio.blkno.set(9);
        bio.bcount.set(512);
        assert_eq!(wdc_ata_bio(drvp, bio), WDC_COMPLETE);
        assert_eq!(bio.error.get(), NOERROR);
        assert!(with(|f| f.disk[9 * 512..10 * 512]
            .iter()
            .all(|&b| b == 0xa5)));
        assert_eq!(with(|f| core::mem::take(&mut f.log)), [(WDCC_WRITE, 1)]);
        assert!(chp.queue().sc_xfer.is_empty());
    }
}
/* </TESTS> */
