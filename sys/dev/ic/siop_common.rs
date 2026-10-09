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
 * Copyright (c) 2000, 2002 Manuel Bouyer.
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
/* </LICENSES> */

/* <CODE> */
//! The code siop(4) shares with esiop: the attach-time set-up of the common softc (script
//! memory, the adapter's SCSI ID, the synchronous period range), the chip reset, the
//! per-command tables (`siop_setuptables`), the wide, synchronous and parallel-protocol
//! negotiations and their messages, the data pointer bookkeeping after a phase mismatch,
//! a disconnect or an ignore-wide-residue message, and the bus helpers.
//!
//! Upstream: sys/dev/ic/siop_common.c @ 3ce1f3f79392
//!
//! "SYM53c7/8xx PCI-SCSI I/O Processors driver". The tables a command shares with the chip
//! are read and written volatile ([`dma_get`]/[`dma_set`], `siopvar_common.rs`).
//!
//! ## Deviations
//! - `siop_common_attach` returns `Result<(), Errno>` (the C returns the `bus_dma` error).
//!   As in the C, a failure part way leaves what was allocated.
//! - The `goto reject`/`goto end`/`goto async` of the negotiations are labelled blocks.
//! - `siop_setuptables` calls siop's `siop_add_dev` through the softc's siop view
//!   (`siop_softc`, `siop.rs`), the C's `(struct siop_softc *)sc` cast: esiop, the other
//!   user of this file, is not ported.
//! - `siop_sdp`'s `bcopy` of the remaining table entries is a loop of entry copies, front to
//!   back (the C's overlapping copy moves them down the same way).
//! - `#ifdef DEBUG`, `DEBUG_DR`, `DEBUG_NEG` and `DEBUG_INTR` are `#undef`'d by the file:
//!   their `printf`s are not ported. `DIAGNOSTIC`'s check in `siop_sdp` is feature
//!   `diagnostic`.
//! - No stubs: every function of the file is ported.

use core::cell::Cell;

use crate::dev::ic::siop::{siop_add_dev, siop_softc};
use crate::dev::ic::siopreg::{
    AIPCNTL1_DIS, CTEST3_CLF, CTEST5_BOMASK, CTEST5_DFS, DCNTL_COM, DCNTL_PFEN, DT_SCF_PERIOD,
    ISTAT_SRST, SCF_PERIOD, SCID_RRE, SCNTL0_AAP, SCNTL0_ARB_MASK, SCNTL0_EPC, SCNTL1_RST,
    SCNTL3_EWS, SCNTL3_SCF_MASK, SCNTL3_SCF_SHIFT, SCNTL3_ULTRA, SCNTL4_U3EN, SIEN0_CMP, SIEN0_RSL,
    SIEN0_SEL, SIEN1_GEN, SIEN1_HTH, SIEN1_SBMC, SIOP_AIPCNTL1, SIOP_CTEST3, SIOP_CTEST5, SIOP_DBC,
    SIOP_DCNTL, SIOP_DFBC, SIOP_DFIFO, SIOP_DIEN, SIOP_GPCNTL, SIOP_ISTAT, SIOP_RESPID0, SIOP_SCID,
    SIOP_SCNTL0, SIOP_SCNTL1, SIOP_SCNTL3, SIOP_SCNTL4, SIOP_SCRATCHA, SIOP_SIEN0, SIOP_SIEN1,
    SIOP_SIST0, SIOP_SIST1, SIOP_SSTAT0, SIOP_SSTAT2, SIOP_STEST1, SIOP_STEST2, SIOP_STEST3,
    SIOP_STEST4, SIOP_STIME0, SIOP_SXFER, SSTAT0_OLF, SSTAT0_ORF, SSTAT2_OLF1, SSTAT2_ORF1,
    STEST1_DBLEN, STEST1_DBLSEL, STEST2_DIF, STEST3_HSC, STEST3_TE, STEST4_LOCK, STEST4_MODE_DIF,
    STEST4_MODE_LVD, STEST4_MODE_MASK, STEST4_MODE_SE, STIME0_SEL_SHIFT, SXFER_MO_MASK,
};
use crate::dev::ic::siopvar_common::{
    CMDFL_RESID, CMDFL_TAG, CMDST_SENSE, SCSI_SIOP_NOSTATUS, SF_BUS_ULTRA3, SF_BUS_WIDE,
    SF_CHIP_AAIP, SF_CHIP_DBLR, SF_CHIP_DFBC, SF_CHIP_FIFO, SF_CHIP_GEBUG, SF_CHIP_LED0,
    SF_CHIP_PF, SF_CHIP_QUAD, SF_CHIP_RAM, SIOP_DEFAULT_TARGET, SIOP_NEG_ACK, SIOP_NEG_MSGOUT,
    SIOP_NSG, ScrTable, SiopCommonCmd, SiopCommonSoftc, SiopCommonXfer, TARF_DT, TARF_ISDT,
    TARF_ISWIDE, TARF_SYNC, TARF_TAG, TARF_WIDE, TARST_ASYNC, TARST_OK, TARST_PPR_NEG,
    TARST_SYNC_NEG, TARST_WIDE_NEG, dma_get, dma_set, siop_ctoh32, siop_htoc32,
};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BusDmaSegment, bus_dmamap_create, bus_dmamap_load,
    bus_dmamem_alloc, bus_dmamem_map, bus_space_set_region_4,
};
use crate::machine::cpu::delay;
use crate::scsi::scsi_message::{
    MSG_EXT_PPR, MSG_EXT_PPR_LEN, MSG_EXT_PPR_PROT_DT, MSG_EXT_SDTR, MSG_EXT_SDTR_LEN,
    MSG_EXT_WDTR, MSG_EXT_WDTR_BUS_8_BIT, MSG_EXT_WDTR_BUS_16_BIT, MSG_EXT_WDTR_LEN, MSG_EXTENDED,
    MSG_MESSAGE_REJECT, msg_identify,
};
use crate::scsi::scsiconf::{SCSI_DATA_IN, SCSI_DATA_OUT, SDEV_NOSYNC, SDEV_NOTAGS, SDEV_NOWIDE};
use crate::sys::errno::Errno;
use crate::sys::param::PAGE_SIZE;

#[cfg(feature = "diagnostic")]
use crate::scsi::scsi_base::sc_print_addr;

/// `siop_common_attach`: the script memory (without on-board RAM), the adapter's own
/// target and the synchronous period range of the chip's clock.
pub fn siop_common_attach(sc: &SiopCommonSoftc) -> Result<(), Errno> {
    let xname = sc.sc_dev.xname();

    // Allocate DMA-safe memory for the script and map it.
    if sc.features.get() & SF_CHIP_RAM == 0 {
        let dmat = sc.dmat();
        let mut seg = [BusDmaSegment::default(); 1];
        let rseg = match bus_dmamem_alloc(dmat, PAGE_SIZE, PAGE_SIZE, 0, &mut seg, BUS_DMA_NOWAIT) {
            Ok(n) => n,
            Err(error) => {
                printf(format_args!(
                    "{xname}: unable to allocate script DMA memory, error = {}\n",
                    error as i32
                ));
                return Err(error);
            }
        };
        let script = match bus_dmamem_map(
            dmat,
            &mut seg[..rseg],
            PAGE_SIZE,
            BUS_DMA_NOWAIT | BUS_DMA_COHERENT,
        ) {
            Ok(kva) => kva,
            Err(error) => {
                printf(format_args!(
                    "{xname}: unable to map script DMA memory, error = {}\n",
                    error as i32
                ));
                return Err(error);
            }
        };
        sc.sc_script.set(script.cast::<u32>().as_ptr());
        let map = match bus_dmamap_create(dmat, PAGE_SIZE, 1, PAGE_SIZE, 0, BUS_DMA_NOWAIT) {
            Ok(map) => map,
            Err(error) => {
                printf(format_args!(
                    "{xname}: unable to create script DMA map, error = {}\n",
                    error as i32
                ));
                return Err(error);
            }
        };
        sc.sc_scriptdma.set(Some(map));
        // SAFETY: the page mapped above, which stays mapped and loaded for the adapter's
        // life.
        let loaded =
            unsafe { bus_dmamap_load(dmat, map, script.as_ptr(), PAGE_SIZE, None, BUS_DMA_NOWAIT) };
        if let Err(error) = loaded {
            printf(format_args!(
                "{xname}: unable to load script DMA map, error = {}\n",
                error as i32
            ));
            return Err(error);
        }
        sc.sc_scriptaddr.set(map.dm_segs()[0].get().ds_addr);
        sc.ram_size.set(PAGE_SIZE as i32);
    }

    // sc->sc_link is the template for all device sc_link's for devices attached to this
    // adapter. It is passed to the upper layers in config_found().
    let buswidth: u16 = if sc.features.get() & SF_BUS_WIDE != 0 {
        16
    } else {
        8
    };
    sc.sc_id.set(u16::from(sc.read_1(SIOP_SCID)));
    if sc.sc_id.get() == 0 || sc.sc_id.get() >= buswidth {
        sc.sc_id.set(SIOP_DEFAULT_TARGET);
    }

    for t in &sc.targets {
        t.set(None);
    }

    // find min/max sync period for this chip
    sc.st_maxsync.set(0);
    sc.dt_maxsync.set(0);
    sc.st_minsync.set(255);
    sc.dt_minsync.set(255);
    let (min, max) = sync_range(&SCF_PERIOD, sc.clock_period.get());
    sc.st_minsync.set(min);
    sc.st_maxsync.set(max);
    if sc.st_maxsync.get() == 255 || sc.st_minsync.get() == 0 {
        panic(format_args!("siop: can't find my sync parameters"));
    }
    let (min, max) = sync_range(&DT_SCF_PERIOD, sc.clock_period.get());
    sc.dt_minsync.set(min);
    sc.dt_maxsync.set(max);
    if sc.dt_maxsync.get() == 255 || sc.dt_minsync.get() == 0 {
        panic(format_args!("siop: can't find my sync parameters"));
    }
    Ok(())
}

/// The smallest and largest period of `table` for the clock `clock_period`, starting
/// from 255 and 0 as `siop_common_attach` does (`(255, 0)` when the clock has none).
pub fn sync_range(table: &[crate::dev::ic::siopreg::ScfPeriod], clock_period: i32) -> (i32, i32) {
    let (mut min, mut max) = (255, 0);
    for p in table.iter().filter(|p| p.clock == clock_period) {
        if max < p.period {
            max = p.period;
        }
        if min > p.period {
            min = p.period;
        }
    }
    (min, max)
}

/// `siop_common_reset`: resets the chip and programs its registers.
pub fn siop_common_reset(sc: &SiopCommonSoftc) {
    let features = sc.features.get();

    // reset the chip
    sc.write_1(SIOP_ISTAT, ISTAT_SRST);
    delay(1000);
    sc.write_1(SIOP_ISTAT, 0);

    // init registers
    sc.write_1(SIOP_SCNTL0, SCNTL0_ARB_MASK | SCNTL0_EPC | SCNTL0_AAP);
    sc.write_1(SIOP_SCNTL1, 0);
    sc.write_1(SIOP_SCNTL3, sc.clock_div.get() as u8);
    sc.write_1(SIOP_SXFER, 0);
    sc.write_1(SIOP_DIEN, 0xff);
    // 0xff & ~(...): the complement of the byte is the C's mask.
    sc.write_1(SIOP_SIEN0, !(SIEN0_CMP | SIEN0_SEL | SIEN0_RSL));
    sc.write_1(SIOP_SIEN1, !(SIEN1_HTH | SIEN1_GEN));
    sc.write_1(SIOP_STEST2, 0);
    sc.write_1(SIOP_STEST3, STEST3_TE);
    sc.write_1(SIOP_STIME0, 0xb << STIME0_SEL_SHIFT);
    sc.write_1(SIOP_SCID, sc.sc_id.get() as u8 | SCID_RRE);
    sc.write_1(SIOP_RESPID0, (1u16 << sc.sc_id.get()) as u8);
    sc.write_1(
        SIOP_DCNTL,
        if features & SF_CHIP_PF != 0 {
            DCNTL_COM | DCNTL_PFEN
        } else {
            DCNTL_COM
        },
    );
    if features & SF_CHIP_AAIP != 0 {
        sc.write_1(SIOP_AIPCNTL1, AIPCNTL1_DIS);
    }

    // enable clock doubler or quadrupler if appropriate
    if features & (SF_CHIP_DBLR | SF_CHIP_QUAD) != 0 {
        let stest3 = sc.read_1(SIOP_STEST3);
        sc.write_1(SIOP_STEST1, STEST1_DBLEN);
        if features & SF_CHIP_QUAD != 0 {
            // wait for PPL to lock
            while sc.read_1(SIOP_STEST4) & STEST4_LOCK == 0 {
                delay(10);
            }
        } else {
            // data sheet says 20us - more won't hurt
            delay(100);
        }
        // halt scsi clock, select doubler/quad, restart clock
        sc.write_1(SIOP_STEST3, stest3 | STEST3_HSC);
        sc.write_1(SIOP_STEST1, STEST1_DBLEN | STEST1_DBLSEL);
        sc.write_1(SIOP_STEST3, stest3);
    } else {
        sc.write_1(SIOP_STEST1, 0);
    }
    if features & SF_CHIP_FIFO != 0 {
        sc.write_1(SIOP_CTEST5, sc.read_1(SIOP_CTEST5) | CTEST5_DFS);
    }
    if features & SF_CHIP_LED0 != 0 {
        // Set GPIO0 as output if software LED control is required
        sc.write_1(SIOP_GPCNTL, sc.read_1(SIOP_GPCNTL) & 0xfe);
    }
    if features & SF_BUS_ULTRA3 != 0 {
        // reset SCNTL4
        sc.write_1(SIOP_SCNTL4, 0);
    }
    sc.mode
        .set(i32::from(sc.read_1(SIOP_STEST4) & STEST4_MODE_MASK));

    // initialise the RAM. Without this we may get scsi gross errors on the 1010
    if features & SF_CHIP_RAM != 0 {
        let (t, h) = sc.ram();
        bus_space_set_region_4(t, h, 0, 0, (sc.ram_size.get() / 4) as usize);
    }
    match sc.sc_reset.get() {
        Some(reset) => reset(sc),
        None => panic(format_args!("{}: no reset callback", sc.sc_dev.xname())),
    }
}

/// `siop_setuptables`: prepare tables before sending a cmd.
pub fn siop_setuptables(siop_cmd: &SiopCommonCmd) {
    let sc = siop_cmd.sc();
    let xs = siop_cmd.xs();
    let link = xs.link();
    let target = usize::from(link.target.get());
    let lun = link.lun.get() as u8;
    let msgoffset = 1;
    let tables = siop_cmd.tables();
    let t = sc.target(target);
    let targ_flags = &t.flags;

    dma_set(&tables.id, siop_htoc32(sc, t.id.get()));
    for b in &tables.msg_out {
        dma_set(b, 0);
    }
    // request sense doesn't disconnect
    if siop_cmd.status.get() == CMDST_SENSE {
        tables.set_msg_out(0, msg_identify(lun, false));
    } else if sc.features.get() & SF_CHIP_GEBUG != 0 && t.flags.get() & TARF_ISWIDE == 0 {
        // 1010 bug: it seems that the 1010 has problems with reselect when not in wide
        // mode (generate false SCSI gross error). The FreeBSD sym driver has comments about
        // it but their workaround (disable SCSI gross error reporting) doesn't work with my
        // adapter. So disable disconnect when not wide.
        tables.set_msg_out(0, msg_identify(lun, false));
    } else {
        tables.set_msg_out(0, msg_identify(lun, true));
    }
    SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(sc, msgoffset as u32));
    if t.status.get() == TARST_ASYNC {
        targ_flags.set(targ_flags.get() & TARF_DT); // Save TARF_DT 'cuz we don't set it here
        let quirks = link.quirks.get();

        if quirks & SDEV_NOTAGS == 0 {
            targ_flags.set(targ_flags.get() | TARF_TAG);
        }
        if quirks & SDEV_NOWIDE == 0 && sc.features.get() & SF_BUS_WIDE != 0 {
            targ_flags.set(targ_flags.get() | TARF_WIDE);
        }
        if quirks & SDEV_NOSYNC == 0 {
            targ_flags.set(targ_flags.get() | TARF_SYNC);
        }

        if sc.features.get() & SF_CHIP_GEBUG != 0 && targ_flags.get() & TARF_WIDE == 0 {
            // 1010 workaround: can't do disconnect if not wide, so can't do tag
            targ_flags.set(targ_flags.get() & !TARF_TAG);
        }

        // Safe to call siop_add_dev() multiple times
        siop_add_dev(siop_softc(sc), target, usize::from(lun));

        if targ_flags.get() & TARF_DT != 0 && sc.mode.get() == i32::from(STEST4_MODE_LVD) {
            t.status.set(TARST_PPR_NEG);
            siop_ppr_msg(siop_cmd, msgoffset, sc.dt_minsync.get(), sc.maxoff.get());
        } else if targ_flags.get() & TARF_WIDE != 0 {
            t.status.set(TARST_WIDE_NEG);
            siop_wdtr_msg(siop_cmd, msgoffset, i32::from(MSG_EXT_WDTR_BUS_16_BIT));
        } else if targ_flags.get() & TARF_SYNC != 0 {
            t.status.set(TARST_SYNC_NEG);
            siop_sdtr_msg(
                siop_cmd,
                msgoffset,
                sc.st_minsync.get(),
                sc.maxoff.get().min(31),
            );
        } else {
            t.status.set(TARST_OK);
            siop_update_xfer_mode(sc, target);
        }
    } else if t.status.get() == TARST_OK
        && targ_flags.get() & TARF_TAG != 0
        && siop_cmd.status.get() != CMDST_SENSE
    {
        siop_cmd.flags.set(siop_cmd.flags.get() | CMDFL_TAG);
    }
    dma_set(&tables.status, siop_htoc32(sc, SCSI_SIOP_NOSTATUS)); // set invalid status

    if xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) != 0 || siop_cmd.status.get() == CMDST_SENSE
    {
        for d in &tables.data {
            dma_set(d, ScrTable::default());
        }
        let map = siop_cmd.dmamap();
        let segs = map.dm_segs();
        for (i, seg) in segs.iter().take(map.dm_nsegs.get() as usize).enumerate() {
            let seg = seg.get();
            dma_set(
                &tables.data[i],
                ScrTable {
                    count: siop_htoc32(sc, seg.ds_len as u32),
                    addr: siop_htoc32(sc, seg.ds_addr as u32),
                },
            );
        }
    }
}

/// `siop_wdtr_neg`: handles a WDTR message from the target.
pub fn siop_wdtr_neg(siop_cmd: &SiopCommonCmd) -> i32 {
    let sc = siop_cmd.sc();
    let siop_target = siop_cmd.target();
    let target = usize::from(siop_cmd.xs().link().target.get());
    let tables = siop_cmd.tables();
    let t = sc.target(target);

    if siop_target.status.get() == TARST_WIDE_NEG {
        // we initiated wide negotiation
        let accepted = match tables.msg_in(3) {
            MSG_EXT_WDTR_BUS_8_BIT => {
                siop_target
                    .flags
                    .set(siop_target.flags.get() & !TARF_ISWIDE);
                t.id.set(t.id.get() & !(u32::from(SCNTL3_EWS) << 24));
                true
            }
            MSG_EXT_WDTR_BUS_16_BIT if siop_target.flags.get() & TARF_WIDE != 0 => {
                siop_target.flags.set(siop_target.flags.get() | TARF_ISWIDE);
                t.id.set(t.id.get() | (u32::from(SCNTL3_EWS) << 24));
                true
            }
            _ => false,
        };
        if !accepted {
            // hum, we got more than what we can handle, shouldn't happen. Reject, and stay
            // async
            siop_target
                .flags
                .set(siop_target.flags.get() & !TARF_ISWIDE);
            siop_target.status.set(TARST_OK);
            siop_target.offset.set(0);
            siop_target.period.set(0);
            siop_update_xfer_mode(sc, target);
            printf(format_args!(
                "{}: rejecting invalid wide negotiation from target {} ({})\n",
                sc.sc_dev.xname(),
                target,
                tables.msg_in(3)
            ));
            SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(sc, 1));
            tables.set_msg_out(0, MSG_MESSAGE_REJECT);
            return SIOP_NEG_MSGOUT;
        }
        dma_set(&tables.id, siop_htoc32(sc, t.id.get()));
        sc.write_1(SIOP_SCNTL3, (t.id.get() >> 24) as u8);
        // we now need to do sync
        if siop_target.flags.get() & TARF_SYNC != 0 {
            siop_target.status.set(TARST_SYNC_NEG);
            siop_sdtr_msg(siop_cmd, 0, sc.st_minsync.get(), sc.maxoff.get().min(31));
            SIOP_NEG_MSGOUT
        } else {
            siop_target.status.set(TARST_OK);
            siop_update_xfer_mode(sc, target);
            SIOP_NEG_ACK
        }
    } else {
        // target initiated wide negotiation
        if tables.msg_in(3) >= MSG_EXT_WDTR_BUS_16_BIT && siop_target.flags.get() & TARF_WIDE != 0 {
            siop_target.flags.set(siop_target.flags.get() | TARF_ISWIDE);
            t.id.set(t.id.get() | (u32::from(SCNTL3_EWS) << 24));
        } else {
            siop_target
                .flags
                .set(siop_target.flags.get() & !TARF_ISWIDE);
            t.id.set(t.id.get() & !(u32::from(SCNTL3_EWS) << 24));
        }
        dma_set(&tables.id, siop_htoc32(sc, t.id.get()));
        sc.write_1(SIOP_SCNTL3, (t.id.get() >> 24) as u8);
        // we did reset wide parameters, so fall back to async, but don't schedule a sync
        // neg, target should initiate it
        siop_target.status.set(TARST_OK);
        siop_target.offset.set(0);
        siop_target.period.set(0);
        siop_update_xfer_mode(sc, target);
        siop_wdtr_msg(
            siop_cmd,
            0,
            if siop_target.flags.get() & TARF_ISWIDE != 0 {
                i32::from(MSG_EXT_WDTR_BUS_16_BIT)
            } else {
                i32::from(MSG_EXT_WDTR_BUS_8_BIT)
            },
        );
        SIOP_NEG_MSGOUT
    }
}

/// `siop_ppr_neg`: handles a PPR message from the target.
pub fn siop_ppr_neg(siop_cmd: &SiopCommonCmd) -> i32 {
    let sc = siop_cmd.sc();
    let siop_target = siop_cmd.target();
    let target = usize::from(siop_cmd.xs().link().target.get());
    let tables = siop_cmd.tables();
    let t = sc.target(target);
    let xname = sc.sc_dev.xname();

    'reject: {
        if siop_target.status.get() != TARST_PPR_NEG {
            // target initiated PPR negotiation, shouldn't happen
            printf(format_args!(
                "{xname}: rejecting invalid PPR negotiation from target {target}\n"
            ));
            break 'reject;
        }

        // we initiated PPR negotiation
        let sync = i32::from(tables.msg_in(3));
        let offset = i32::from(tables.msg_in(5));
        let options = tables.msg_in(7);
        let mut scf = 0;
        let fall_back_async = || {
            siop_target.status.set(TARST_ASYNC);
            siop_target
                .flags
                .set(siop_target.flags.get() & !(TARF_DT | TARF_ISDT));
            siop_target.offset.set(0);
            siop_target.period.set(0);
        };
        if options != MSG_EXT_PPR_PROT_DT {
            // shouldn't happen
            printf(format_args!(
                "{xname}: ppr negotiation for target {target}: no DT option\n"
            ));
            fall_back_async();
            break 'reject;
        }

        if offset > sc.maxoff.get() || sync < sc.dt_minsync.get() || sync > sc.dt_maxsync.get() {
            printf(format_args!(
                "{xname}: ppr negotiation for target {target}: offset ({offset}) or sync \
                 ({sync}) out of range\n"
            ));
            // should not happen
            fall_back_async();
            break 'reject;
        } else {
            for p in DT_SCF_PERIOD.iter() {
                if sc.clock_period.get() != p.clock {
                    continue;
                }
                if p.period == sync {
                    // ok, found it. we now are sync.
                    siop_target.offset.set(offset);
                    siop_target.period.set(sync);
                    scf = p.scf;
                    siop_target.flags.set(siop_target.flags.get() | TARF_ISDT);
                }
            }
            if siop_target.flags.get() & TARF_ISDT == 0 {
                printf(format_args!(
                    "{xname}: ppr negotiation for target {target}: sync ({sync}) \
                     incompatible with adapter\n"
                ));
                // we didn't find it in our table, do async send reject msg, start
                // SDTR/WDTR neg
                fall_back_async();
                break 'reject;
            }
        }
        if tables.msg_in(6) != 1 {
            printf(format_args!(
                "{xname}: ppr negotiation for target {target}: transfer width ({}) \
                 incompatible with dt\n",
                tables.msg_in(6)
            ));
            // DT mode can only be done with wide transfers
            fall_back_async();
            break 'reject;
        }
        siop_target.flags.set(siop_target.flags.get() | TARF_ISWIDE);
        let mut id = t.id.get();
        id |= u32::from(SCNTL3_EWS) << 24;
        id &= !(u32::from(SCNTL3_SCF_MASK) << 24);
        id |= (scf as u32) << (24 + SCNTL3_SCF_SHIFT);
        id &= !(u32::from(SXFER_MO_MASK) << 8);
        id |= ((siop_target.offset.get() as u32) & u32::from(SXFER_MO_MASK)) << 8;
        id &= !0xff;
        id |= u32::from(SCNTL4_U3EN);
        t.id.set(id);
        siop_target.status.set(TARST_OK);
        siop_update_xfer_mode(sc, target);
        sc.write_1(SIOP_SCNTL3, (t.id.get() >> 24) as u8);
        sc.write_1(SIOP_SXFER, (t.id.get() >> 8) as u8);
        sc.write_1(SIOP_SCNTL4, t.id.get() as u8);
        return SIOP_NEG_ACK;
    }

    // reject:
    SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(sc, 1));
    tables.set_msg_out(0, MSG_MESSAGE_REJECT);
    SIOP_NEG_MSGOUT
}

/// The target's `id` with the synchronous `period`'s SCF and `offset` set
/// (`siop_sdtr_neg`'s "ok, found it. we now are sync.").
fn siop_sdtr_id(sc: &SiopCommonSoftc, id: u32, scf: i32, sync: i32, offset: i32) -> u32 {
    let mut id = id;
    id &= !(u32::from(SCNTL3_SCF_MASK) << 24);
    id |= (scf as u32) << (24 + SCNTL3_SCF_SHIFT);
    if sync < 25 /* Ultra */ && sc.features.get() & SF_BUS_ULTRA3 == 0 {
        id |= u32::from(SCNTL3_ULTRA) << 24;
    } else {
        id &= !(u32::from(SCNTL3_ULTRA) << 24);
    }
    id &= !(u32::from(SXFER_MO_MASK) << 8);
    id |= ((offset as u32) & u32::from(SXFER_MO_MASK)) << 8;
    id &= !0xff; // scntl4
    id
}

/// The target's `id` back to asynchronous transfers.
fn siop_async_id(id: u32) -> u32 {
    let mut id = id;
    id &= !(u32::from(SCNTL3_SCF_MASK) << 24);
    id &= !(u32::from(SCNTL3_ULTRA) << 24);
    id &= !(u32::from(SXFER_MO_MASK) << 8);
    id &= !0xff; // scntl4
    id
}

/// `siop_sdtr_neg`: handles an SDTR message from the target.
pub fn siop_sdtr_neg(siop_cmd: &SiopCommonCmd) -> i32 {
    let sc = siop_cmd.sc();
    let siop_target = siop_cmd.target();
    let target = usize::from(siop_cmd.xs().link().target.get());
    let tables = siop_cmd.tables();
    let t = sc.target(target);
    let mut send_msgout = false;

    // limit to Ultra/2 parameters, need PPR for Ultra/3
    let maxoffset = sc.maxoff.get().min(31);

    let mut sync = i32::from(tables.msg_in(3));
    let mut offset = i32::from(tables.msg_in(4));

    'end: {
        if siop_target.status.get() == TARST_SYNC_NEG {
            // we initiated sync negotiation
            siop_target.status.set(TARST_OK);
            if !(offset > maxoffset || sync < sc.st_minsync.get() || sync > sc.st_maxsync.get()) {
                for p in SCF_PERIOD.iter() {
                    if sc.clock_period.get() != p.clock {
                        continue;
                    }
                    if p.period == sync {
                        // ok, found it. we now are sync.
                        siop_target.offset.set(offset);
                        siop_target.period.set(sync);
                        t.id.set(siop_sdtr_id(sc, t.id.get(), p.scf, sync, offset));
                        break 'end;
                    }
                }
            }
            // we didn't find it in our table, do async and send reject msg
            // reject:
            send_msgout = true;
            SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(sc, 1));
            tables.set_msg_out(0, MSG_MESSAGE_REJECT);
            t.id.set(siop_async_id(t.id.get()));
            siop_target.offset.set(0);
            siop_target.period.set(0);
        } else {
            // target initiated sync neg
            if !(offset == 0 || sync > sc.st_maxsync.get()) {
                if offset > maxoffset {
                    offset = maxoffset;
                }
                if sync < sc.st_minsync.get() {
                    sync = sc.st_minsync.get();
                }
                // look for sync period
                for p in SCF_PERIOD.iter() {
                    if sc.clock_period.get() != p.clock {
                        continue;
                    }
                    if p.period == sync {
                        // ok, found it. we now are sync.
                        siop_target.offset.set(offset);
                        siop_target.period.set(sync);
                        t.id.set(siop_sdtr_id(sc, t.id.get(), p.scf, sync, offset));
                        siop_sdtr_msg(siop_cmd, 0, sync, offset);
                        send_msgout = true;
                        break 'end;
                    }
                }
            }
            // async:
            siop_target.offset.set(0);
            siop_target.period.set(0);
            t.id.set(siop_async_id(t.id.get()));
            siop_sdtr_msg(siop_cmd, 0, 0, 0);
            send_msgout = true;
        }
    }

    // end:
    if siop_target.status.get() == TARST_OK {
        siop_update_xfer_mode(sc, target);
    }
    dma_set(&tables.id, siop_htoc32(sc, t.id.get()));
    sc.write_1(SIOP_SCNTL3, (t.id.get() >> 24) as u8);
    sc.write_1(SIOP_SXFER, (t.id.get() >> 8) as u8);
    if send_msgout {
        SIOP_NEG_MSGOUT
    } else {
        SIOP_NEG_ACK
    }
}

/// `siop_sdtr_msg`: an SDTR message at `offset` of the message out buffer.
pub fn siop_sdtr_msg(siop_cmd: &SiopCommonCmd, offset: usize, ssync: i32, soff: i32) {
    let t = siop_cmd.tables();
    t.set_msg_out(offset, MSG_EXTENDED);
    t.set_msg_out(offset + 1, MSG_EXT_SDTR_LEN);
    t.set_msg_out(offset + 2, MSG_EXT_SDTR);
    t.set_msg_out(offset + 3, ssync as u8);
    t.set_msg_out(offset + 4, soff as u8);
    SiopCommonXfer::set_count(
        &t.t_msgout,
        siop_htoc32(
            siop_cmd.sc(),
            (offset + usize::from(MSG_EXT_SDTR_LEN) + 2) as u32,
        ),
    );
}

/// `siop_wdtr_msg`: a WDTR message at `offset` of the message out buffer.
pub fn siop_wdtr_msg(siop_cmd: &SiopCommonCmd, offset: usize, wide: i32) {
    let t = siop_cmd.tables();
    t.set_msg_out(offset, MSG_EXTENDED);
    t.set_msg_out(offset + 1, MSG_EXT_WDTR_LEN);
    t.set_msg_out(offset + 2, MSG_EXT_WDTR);
    t.set_msg_out(offset + 3, wide as u8);
    SiopCommonXfer::set_count(
        &t.t_msgout,
        siop_htoc32(
            siop_cmd.sc(),
            (offset + usize::from(MSG_EXT_WDTR_LEN) + 2) as u32,
        ),
    );
}

/// `siop_ppr_msg`: a PPR message at `offset` of the message out buffer.
pub fn siop_ppr_msg(siop_cmd: &SiopCommonCmd, offset: usize, ssync: i32, soff: i32) {
    let t = siop_cmd.tables();
    t.set_msg_out(offset, MSG_EXTENDED);
    t.set_msg_out(offset + 1, MSG_EXT_PPR_LEN);
    t.set_msg_out(offset + 2, MSG_EXT_PPR);
    t.set_msg_out(offset + 3, ssync as u8);
    t.set_msg_out(offset + 4, 0); // reserved
    t.set_msg_out(offset + 5, soff as u8);
    t.set_msg_out(offset + 6, 1); // wide
    t.set_msg_out(offset + 7, MSG_EXT_PPR_PROT_DT);
    SiopCommonXfer::set_count(
        &t.t_msgout,
        siop_htoc32(
            siop_cmd.sc(),
            (offset + usize::from(MSG_EXT_PPR_LEN) + 2) as u32,
        ),
    );
}

/// `siop_ma`: compute how much of the current table didn't get handled when a phase
/// mismatch occurs.
pub fn siop_ma(siop_cmd: &SiopCommonCmd) {
    let sc = siop_cmd.sc();
    let xs = siop_cmd.xs();

    if xs.flags.get() & (SCSI_DATA_OUT | SCSI_DATA_IN) == 0 {
        return; // no valid data transfer
    }

    let offset = usize::from(sc.read_1(SIOP_SCRATCHA + 1));
    if offset >= SIOP_NSG {
        printf(format_args!(
            "{}: bad offset in siop_sdp ({offset})\n",
            sc.sc_dev.xname()
        ));
        return;
    }
    let mut dbc = (sc.read_4(SIOP_DBC) & 0x00ff_ffff) as i32;
    if xs.flags.get() & SCSI_DATA_OUT != 0 {
        if sc.features.get() & SF_CHIP_DFBC != 0 {
            dbc += i32::from(sc.read_2(SIOP_DFBC));
        } else {
            // need to account stale data in FIFO
            let mut dfifo = i32::from(sc.read_1(SIOP_DFIFO));
            if sc.features.get() & SF_CHIP_FIFO != 0 {
                dfifo |= i32::from(sc.read_1(SIOP_CTEST5) & CTEST5_BOMASK) << 8;
                dbc += (dfifo - (dbc & 0x3ff)) & 0x3ff;
            } else {
                dbc += (dfifo - (dbc & 0x7f)) & 0x7f;
            }
        }
        let sstat = sc.read_1(SIOP_SSTAT0);
        if sstat & SSTAT0_OLF != 0 {
            dbc += 1;
        }
        if sstat & SSTAT0_ORF != 0 && sc.features.get() & SF_CHIP_DFBC == 0 {
            dbc += 1;
        }
        if siop_cmd.target().flags.get() & TARF_ISWIDE != 0 {
            let sstat = sc.read_1(SIOP_SSTAT2);
            if sstat & SSTAT2_OLF1 != 0 {
                dbc += 1;
            }
            if sstat & SSTAT2_ORF1 != 0 && sc.features.get() & SF_CHIP_DFBC == 0 {
                dbc += 1;
            }
        }
        // clear the FIFO
        sc.write_1(SIOP_CTEST3, sc.read_1(SIOP_CTEST3) | CTEST3_CLF);
    }
    siop_cmd.flags.set(siop_cmd.flags.get() | CMDFL_RESID);
    siop_cmd.resid.set(dbc);
}

/// `siop_sdp`: save data pointer. We do this by adjusting the tables to point at the
/// beginning of the data not yet transferred. `offset` points to the first table with
/// untransferred data.
pub fn siop_sdp(siop_cmd: &SiopCommonCmd, offset: usize) {
    let sc = siop_cmd.sc();
    let xs = siop_cmd.xs();

    if xs.flags.get() & (SCSI_DATA_OUT | SCSI_DATA_IN) == 0 {
        return; // no data pointers to save
    }

    // offset == SIOP_NSG may be a valid condition if we get a Save data pointer when the
    // xfer is done. Just ignore the Save data pointer in this case
    if offset == SIOP_NSG {
        return;
    }
    #[cfg(feature = "diagnostic")]
    if offset > SIOP_NSG {
        sc_print_addr(xs.link());
        printf(format_args!("offset {offset} > {SIOP_NSG}\n"));
        panic(format_args!("siop_sdp: offset"));
    }
    let tables = siop_cmd.tables();

    // before doing that we decrease resid from the amount of data which has been
    // transferred.
    siop_update_resid(siop_cmd, offset);

    // First let see if we have a resid from a phase mismatch. If so, we have to adjust the
    // table at offset to remove transferred data.
    if siop_cmd.flags.get() & CMDFL_RESID != 0 {
        siop_cmd.flags.set(siop_cmd.flags.get() & !CMDFL_RESID);
        let table = &tables.data[offset];
        let t = dma_get(table);
        let resid = siop_cmd.resid.get() as u32;
        // "cut" already transferred data from this table
        dma_set(
            table,
            ScrTable {
                addr: siop_htoc32(
                    sc,
                    siop_ctoh32(sc, t.addr)
                        .wrapping_add(siop_ctoh32(sc, t.count))
                        .wrapping_sub(resid),
                ),
                count: siop_htoc32(sc, resid),
            },
        );
    }

    // now we can remove entries which have been transferred. We just move the entries
    // with data left at the beginning of the tables
    for i in 0..SIOP_NSG - offset {
        dma_set(&tables.data[i], dma_get(&tables.data[offset + i]));
    }
}

/// `siop_update_resid`: update resid. First account for the table entries which have been
/// fully completed.
pub fn siop_update_resid(siop_cmd: &SiopCommonCmd, offset: usize) {
    let sc = siop_cmd.sc();
    let xs = siop_cmd.xs();

    if xs.flags.get() & (SCSI_DATA_OUT | SCSI_DATA_IN) == 0 {
        return; // no data to transfer
    }
    let tables = siop_cmd.tables();

    for d in tables.data.iter().take(offset) {
        let count = siop_ctoh32(sc, dma_get(d).count) as usize;
        xs.resid.set(xs.resid.get().wrapping_sub(count));
    }
    // if CMDFL_RESID is set, the last table (pointed by offset) is a partial transfers. If
    // not, offset points to the entry following the last full transfer.
    if siop_cmd.flags.get() & CMDFL_RESID != 0 {
        let table = dma_get(&tables.data[offset]);
        let done = siop_ctoh32(sc, table.count).wrapping_sub(siop_cmd.resid.get() as u32);
        xs.resid.set(xs.resid.get().wrapping_sub(done as usize));
    }
}

/// `siop_iwr`: handle ignore wide residue messages.
pub fn siop_iwr(siop_cmd: &SiopCommonCmd) -> i32 {
    let sc = siop_cmd.sc();
    let tables = siop_cmd.tables();

    // if target isn't wide, reject
    if siop_cmd.target().flags.get() & TARF_ISWIDE == 0 {
        SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(sc, 1));
        tables.set_msg_out(0, MSG_MESSAGE_REJECT);
        return SIOP_NEG_MSGOUT;
    }
    // get index of current command in table
    let mut offset = i32::from(sc.read_1(SIOP_SCRATCHA + 1));
    // if the current table did complete, we're now pointing at the next one. Go back one if
    // we didn't see a phase mismatch.
    if siop_cmd.flags.get() & CMDFL_RESID == 0 {
        offset -= 1;
    }
    let table: &Cell<ScrTable> = match usize::try_from(offset)
        .ok()
        .and_then(|o| tables.data.get(o))
    {
        Some(t) => t,
        None => panic(format_args!("siop_iwr: bad table offset {offset}")),
    };

    if siop_cmd.flags.get() & CMDFL_RESID == 0 {
        if siop_ctoh32(sc, dma_get(table).count) & 1 != 0 {
            // we really got the number of bytes we expected
            SIOP_NEG_ACK
        } else {
            // now we really had a short xfer, by one byte. handle it just as if we had a
            // phase mismatch (there is a resid of one for this table). Update scratcha1 to
            // reflect the fact that this xfer isn't complete.
            siop_cmd.flags.set(siop_cmd.flags.get() | CMDFL_RESID);
            siop_cmd.resid.set(1);
            sc.write_1(SIOP_SCRATCHA + 1, offset as u8);
            SIOP_NEG_ACK
        }
    } else {
        // we already have a short xfer for this table; it's just one byte less than we
        // though it was
        siop_cmd.resid.set(siop_cmd.resid.get() - 1);
        SIOP_NEG_ACK
    }
}

/// `siop_clearfifo`: clears the DMA FIFO.
pub fn siop_clearfifo(sc: &SiopCommonSoftc) {
    let mut timeout = 0;
    let ctest3 = sc.read_1(SIOP_CTEST3);

    sc.write_1(SIOP_CTEST3, ctest3 | CTEST3_CLF);
    while sc.read_1(SIOP_CTEST3) & CTEST3_CLF != 0 {
        delay(1);
        timeout += 1;
        if timeout > 1000 {
            printf(format_args!("clear fifo failed\n"));
            sc.write_1(SIOP_CTEST3, sc.read_1(SIOP_CTEST3) & !CTEST3_CLF);
            return;
        }
    }
}

/// `siop_modechange`: follows a SCSI bus mode change (single-ended, LVD, differential);
/// returns whether the new mode is valid.
pub fn siop_modechange(sc: &SiopCommonSoftc) -> bool {
    let xname = sc.sc_dev.xname();
    for _retry in 0..5 {
        // datasheet says to wait 100ms and re-read SIST1, to check that DIFFSENSE is
        // stable. We may delay() 5 times for 100ms at interrupt time; hopefully this will
        // not happen often.
        delay(100_000);
        let _sist0 = sc.read_1(SIOP_SIST0);
        let sist1 = sc.read_1(SIOP_SIST1);
        if sist1 & SIEN1_SBMC != 0 {
            continue; // we got an irq again
        }
        sc.mode
            .set(i32::from(sc.read_1(SIOP_STEST4) & STEST4_MODE_MASK));
        let stest2 = sc.read_1(SIOP_STEST2);
        match sc.mode.get() as u8 {
            STEST4_MODE_DIF => {
                printf(format_args!("{xname}: switching to differential mode\n"));
                sc.write_1(SIOP_STEST2, stest2 | STEST2_DIF);
            }
            STEST4_MODE_SE => {
                printf(format_args!("{xname}: switching to single-ended mode\n"));
                sc.write_1(SIOP_STEST2, stest2 & !STEST2_DIF);
            }
            STEST4_MODE_LVD => {
                printf(format_args!("{xname}: switching to LVD mode\n"));
                sc.write_1(SIOP_STEST2, stest2 & !STEST2_DIF);
            }
            _ => {
                printf(format_args!(
                    "{xname}: invalid SCSI mode {:#x}\n",
                    sc.mode.get()
                ));
                return false;
            }
        }
        return true;
    }
    printf(format_args!(
        "{xname}: timeout waiting for DIFFSENSE to stabilise\n"
    ));
    false
}

/// `siop_resetbus`: pulses the SCSI bus reset line.
pub fn siop_resetbus(sc: &SiopCommonSoftc) {
    let scntl1 = sc.read_1(SIOP_SCNTL1);
    sc.write_1(SIOP_SCNTL1, scntl1 | SCNTL1_RST);
    // minimum 25 us, more time won't hurt
    delay(100);
    sc.write_1(SIOP_SCNTL1, scntl1);
}

/// The transfer rate `siop_update_xfer_mode` prints for a synchronous period.
pub fn siop_period_mhz(period: i32) -> &'static str {
    match period {
        9 => "80.0",  //   12.5ns cycle
        10 => "40.0", //  25  ns cycle
        12 => "20.0", //  48  ns cycle
        18 => "13.3", //  72  ns cycle
        25 => "10.0", // 100  ns cycle
        37 => "6.67", // 118  ns cycle
        50 => "5.0",  // 200  ns cycle
        75 => "3.33", // 300  ns cycle
        _ => "??",
    }
}

/// `siop_update_xfer_mode`: prints the target's transfer mode.
pub fn siop_update_xfer_mode(sc: &SiopCommonSoftc, target: usize) {
    let siop_target = sc.target(target);
    let flags = siop_target.flags.get();

    printf(format_args!(
        "{}: target {} now using {}{}{} bit ",
        sc.sc_dev.xname(),
        target,
        if flags & TARF_TAG != 0 { "tagged " } else { "" },
        if flags & TARF_ISDT != 0 { "DT " } else { "" },
        if flags & TARF_ISWIDE != 0 { 16 } else { 8 }
    ));

    if siop_target.offset.get() == 0 {
        printf(format_args!("async "));
    } else {
        printf(format_args!(
            "{} MHz {} REQ/ACK offset ",
            siop_period_mhz(siop_target.period.get()),
            siop_target.offset.get()
        ));
    }

    printf(format_args!("xfers\n"));

    if sc.features.get() & SF_CHIP_GEBUG != 0 && siop_target.flags.get() & TARF_ISWIDE == 0 {
        // 1010 workaround: can't do disconnect if not wide, so can't do tag
        siop_target.flags.set(siop_target.flags.get() & !TARF_TAG);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `siop_common.rs`: the negotiation messages, the synchronous period range,
    // the save-data-pointer and residual arithmetic, and a target-initiated SDTR, on a command
    // whose tables live in host memory (the host's `bus_space` reads 0 and drops writes).

    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::{assert_eq, vec};

    use super::*;
    use crate::dev::ic::siopreg::{SCF_PERIOD, ScfPeriod};
    use crate::dev::ic::siopvar_common::{SiopCommonTarget, TARF_ISWIDE};
    use crate::machine::bus::{BusSpaceTag, bus_space_map};
    use crate::scsi::scsi_message::MSG_EXT_SDTR_LEN;
    use crate::scsi::scsiconf::{ScsiLink, ScsiXfer};

    /// A zeroed `T` (an `M_ZERO` allocation), leaked.
    ///
    /// # Safety
    ///
    /// All-zero must be a valid `T`.
    unsafe fn leak_zeroed<T>() -> &'static T {
        // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
        unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
    }

    /// An adapter with the 53c895A's clock and offset, its registers "mapped", and a command
    /// for target 0 lun 0 whose transfer has `flags`.
    fn command(flags: i32) -> &'static SiopCommonCmd {
        // SAFETY: the softc, the target, the tables and the command are `Cell`s of integers,
        // pointers, `Option`s and bus tags, and a `Device`: all valid as zero bytes.
        let (sc, target, tables, cmd) = unsafe {
            (
                leak_zeroed::<SiopCommonSoftc>(),
                leak_zeroed::<SiopCommonTarget>(),
                leak_zeroed::<SiopCommonXfer>(),
                leak_zeroed::<SiopCommonCmd>(),
            )
        };
        sc.clock_period.set(62);
        sc.maxoff.set(31);
        let (min, max) = sync_range(&SCF_PERIOD, 62);
        sc.st_minsync.set(min);
        sc.st_maxsync.set(max);
        sc.sc_rt.set(Some(BusSpaceTag::default()));
        // SAFETY: the host's bus space is a test double; nothing is mapped.
        sc.sc_rh.set(Some(
            unsafe { bus_space_map(BusSpaceTag::default(), 0, 0x100, 0) }.unwrap(),
        ));
        sc.targets[0].set(Some(core::ptr::NonNull::from(target)));

        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
        xs.sc_link.set(Some(link));
        xs.flags.set(flags);

        cmd.siop_sc.set(Some(sc));
        cmd.siop_target.set(sc.targets[0].get());
        cmd.xs.set(Some(xs));
        cmd.siop_tables.set(Some(tables));
        cmd
    }

    fn msg_out(cmd: &SiopCommonCmd, n: usize) -> std::vec::Vec<u8> {
        (0..n).map(|i| cmd.tables().msg_out(i)).collect()
    }

    #[test]
    fn negotiation_messages() {
        let cmd = command(0);
        siop_sdtr_msg(cmd, 1, 10, 31);
        assert_eq!(
            msg_out(cmd, 6)[1..],
            [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 10, 31]
        );
        assert_eq!(dma_get(&cmd.tables().t_msgout).count, 6);

        siop_wdtr_msg(cmd, 0, 1);
        assert_eq!(
            msg_out(cmd, 4),
            [MSG_EXTENDED, MSG_EXT_WDTR_LEN, MSG_EXT_WDTR, 1]
        );
        assert_eq!(dma_get(&cmd.tables().t_msgout).count, 4);

        siop_ppr_msg(cmd, 0, 9, 62);
        assert_eq!(
            msg_out(cmd, 8),
            [
                MSG_EXTENDED,
                MSG_EXT_PPR_LEN,
                MSG_EXT_PPR,
                9,
                0,
                62,
                1,
                MSG_EXT_PPR_PROT_DT
            ]
        );
        assert_eq!(dma_get(&cmd.tables().t_msgout).count, 8);
    }

    #[test]
    fn sync_range_per_clock() {
        assert_eq!(sync_range(&SCF_PERIOD, 62), (10, 25));
        assert_eq!(sync_range(&DT_SCF_PERIOD, 62), (9, 25));
        assert_eq!(sync_range(&SCF_PERIOD, 250), (25, 75));
        assert_eq!(sync_range(&SCF_PERIOD, 125), (12, 50));
        let none: [ScfPeriod; 0] = [];
        assert_eq!(sync_range(&none, 62), (255, 0));
        assert_eq!(siop_period_mhz(10), "40.0");
        assert_eq!(siop_period_mhz(11), "??");
    }

    #[test]
    fn sdp_cuts_the_partial_table_and_moves_the_rest_down() {
        let cmd = command(SCSI_DATA_IN);
        let t = cmd.tables();
        let rows = [(4096, 0x10_0000), (4096, 0x20_0000), (2048, 0x30_0000)];
        for (i, &(count, addr)) in rows.iter().enumerate() {
            dma_set(&t.data[i], ScrTable { count, addr });
        }
        cmd.xs().resid.set(10240);
        // A phase mismatch left 1000 bytes of the second table untransferred.
        cmd.flags.set(CMDFL_RESID);
        cmd.resid.set(1000);

        siop_sdp(cmd, 1);

        assert_eq!(cmd.xs().resid.get(), 10240 - 4096 - 3096);
        assert_eq!(cmd.flags.get() & CMDFL_RESID, 0);
        assert_eq!(
            dma_get(&t.data[0]),
            ScrTable {
                count: 1000,
                addr: 0x20_0000 + 4096 - 1000
            }
        );
        assert_eq!(
            dma_get(&t.data[1]),
            ScrTable {
                count: 2048,
                addr: 0x30_0000
            }
        );
        assert_eq!(dma_get(&t.data[2]), ScrTable::default());

        // Without data, nothing moves; at SIOP_NSG, neither.
        let cmd = command(0);
        dma_set(&cmd.tables().data[0], ScrTable { count: 5, addr: 6 });
        siop_sdp(cmd, 0);
        assert_eq!(dma_get(&cmd.tables().data[0]).count, 5);
        let cmd = command(SCSI_DATA_OUT);
        dma_set(&cmd.tables().data[0], ScrTable { count: 5, addr: 6 });
        siop_sdp(cmd, SIOP_NSG);
        assert_eq!(dma_get(&cmd.tables().data[0]).count, 5);
    }

    #[test]
    fn update_resid_counts_whole_tables() {
        let cmd = command(SCSI_DATA_OUT);
        for d in cmd.tables().data.iter().take(3) {
            dma_set(
                d,
                ScrTable {
                    count: 512,
                    addr: 0,
                },
            );
        }
        cmd.xs().resid.set(1536);
        siop_update_resid(cmd, 2);
        assert_eq!(cmd.xs().resid.get(), 512);
        siop_update_resid(cmd, 1);
        assert_eq!(cmd.xs().resid.get(), 0);
    }

    #[test]
    fn target_initiated_sdtr_is_answered_at_the_closest_period() {
        let cmd = command(0);
        let sc = cmd.sc();
        let t = cmd.target();
        t.status.set(TARST_OK);
        t.id.set(0x0300_0000); // clock_div 3 in SCNTL3
        let msg_in = [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 25, 40];
        for (i, &b) in msg_in.iter().enumerate() {
            dma_set(&cmd.tables().msg_in[i], b);
        }

        assert_eq!(siop_sdtr_neg(cmd), SIOP_NEG_MSGOUT);
        // Period 25 at clock 62 is SCF 5; the offset is cut to 31; not Ultra (period >= 25).
        assert_eq!((t.period.get(), t.offset.get()), (25, 31));
        let id = sc.target(0).id.get();
        assert_eq!(
            (id >> 24) & u32::from(SCNTL3_SCF_MASK),
            5 << SCNTL3_SCF_SHIFT
        );
        assert_eq!((id >> 8) & u32::from(SXFER_MO_MASK), 31);
        assert_eq!(dma_get(&cmd.tables().id), id);
        // The answer: the SDTR with the period and offset we can do.
        assert_eq!(
            msg_out(cmd, 5),
            [MSG_EXTENDED, MSG_EXT_SDTR_LEN, MSG_EXT_SDTR, 25, 31]
        );

        // A wide answer to a target that cannot be wide is rejected by siop_iwr.
        t.flags.set(t.flags.get() & !TARF_ISWIDE);
        assert_eq!(siop_iwr(cmd), SIOP_NEG_MSGOUT);
        assert_eq!(msg_out(cmd, 1), vec![MSG_MESSAGE_REJECT]);
    }
}
/* </TESTS> */
