/* <LICENSES> */
/*
 * Copyright (c) 2000 Manuel Bouyer.
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
//! siop(4): the driver of the Symbios/NCR 53c7xx/8xx PCI-SCSI I/O processors, a SCSI
//! adapter whose bus is a parallel SCSI bus (`siop* at pci?`, QEMU's `lsi53c895a`).
//!
//! Upstream: sys/dev/ic/siop.c @ 3ce1f3f79392
//!
//! "SYM53c7/8xx PCI-SCSI I/O Processors driver". The chip runs the SCRIPTS program of
//! `dev/microcode/siop` (copied into its on-board RAM, or into a page of host memory): a
//! scheduler of 40 slots, each a `JUMP` to the select script of a command, and a reselect
//! switch per target, lun and tag. Each command (`struct siop_cmd`) owns a 384-byte area in
//! DMA memory, `struct siop_xfer`: the tables the SCRIPTS read (messages, CDB, status,
//! scatter/gather list) and its own copy of `load_dsa`, which loads the command's address
//! into DSA and jumps to the select or reselected code. `siop_start` puts ready commands
//! into free scheduler slots and signals the chip; `siop_intr` handles the script
//! interrupts (message in, extended messages and the wide/sync negotiations, disconnect,
//! save data pointer, reselection, completion), the phase mismatches, selection timeouts
//! and bus resets; `siop_scsicmd_end` completes a command, issuing a REQUEST SENSE through
//! slot 0 after a CHECK CONDITION.
//!
//! ## Deviations
//! - `siop_cd`, `siop_switch` are [`SIOP_CD`] and [`SIOP_SWITCH`].
//! - `(struct siop_softc *)` casts of the common softc are [`siop_softc`], which checks that
//!   the device is a siop (`SIOP_CD`) before the cast: every siop attachment's softc begins
//!   with a `struct siop_softc`.
//! - The command blocks, targets, luns and lun switches are `malloc(9)`ed as in the C and
//!   reached through the accessors of `siopvar.rs`; a `malloc` that may fail
//!   (`M_NOWAIT`, `M_CANFAIL`) is checked as in the C.
//! - `siop_intr`'s `goto reset`, `goto scintr` and `goto end` are a labelled block that
//!   yields where to go; the code at `reset:` and `end:` follows it.
//! - In `siop_intr`, a DSA that points past the last `siop_xfer` of a block's page, or at a
//!   command that never carried a transfer (`xs` NULL), is taken as "no current command"
//!   (the C indexes past `cmds[]` or dereferences NULL); a reselect with a lun or a tag
//!   outside the tables is "invalid lun"/"invalid tag" (the C indexes past them).
//! - `siop_handle_qtag_reject` returns `bool` (false for the C's -1).
//! - `siop_scsiprobe` returns `Result<(), Errno>` (`ENOMEM`).
//! - The tables and the `resel[]` script of a command are written volatile
//!   (`siopvar_common.rs`); the host copy of the script (without on-board RAM) is read and
//!   written volatile.
//! - `SIOP_DEBUG` (and with it `SIOP_DEBUG_DR`, `SIOP_DEBUG_INTR`, `SIOP_DEBUG_SCHED`,
//!   `DUMP_SCRIPT` and `siop_dump_script`) and `SIOP_STATS` (`INCSTAT`, the `siop_stat_*`
//!   counters, `siop_printstats`) are not defined by the file: their code is not ported.
//!   `DIAGNOSTIC`'s checks are feature `diagnostic`.
//! - `SIOP_DEFAULT_TARGET`, which siop.c defines again under `#ifndef`, is
//!   `siopvar_common.rs`'s.
//! - No stubs: every other function of the file is ported.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::siop_common::{
    siop_clearfifo, siop_common_attach, siop_common_reset, siop_iwr, siop_ma, siop_modechange,
    siop_ppr_neg, siop_resetbus, siop_sdp, siop_sdtr_msg, siop_sdtr_neg, siop_setuptables,
    siop_update_resid, siop_update_xfer_mode, siop_wdtr_neg,
};
use crate::dev::ic::siopreg::{
    DCNTL_STD, DSTAT_ABRT, DSTAT_BF, DSTAT_DFE, DSTAT_IID, DSTAT_MDPE, DSTAT_SIR, DSTAT_SSI,
    ISTAT_ABRT, ISTAT_DIP, ISTAT_INTF, ISTAT_SIGP, ISTAT_SIP, SIOP_DCNTL, SIOP_DSA, SIOP_DSP,
    SIOP_DSPS, SIOP_DSTAT, SIOP_ISTAT, SIOP_SCRATCHA, SIOP_SFBR, SIOP_SIST0, SIOP_SSTAT1, SIST0_MA,
    SIST0_PAR, SIST0_RST, SIST0_SGE, SIST0_UDC, SIST1_SBMC, SIST1_STO, SSTAT1_PHASE_MASK,
    SSTAT1_PHASE_MSGIN, SSTAT1_PHASE_STATUS,
};
use crate::dev::ic::siopvar::{
    CmdList, SIOP_LUNF_FULL, SIOP_NTAG, SiopCbd, SiopCmd, SiopDmamem, SiopLun, SiopLunsw,
    SiopSoftc, SiopTarget, SiopXfer,
};
use crate::dev::ic::siopvar_common::{
    CMDFL_TAG, CMDFL_TIMEOUT, CMDST_ACTIVE, CMDST_DONE, CMDST_FREE, CMDST_READY, CMDST_SENSE,
    CMDST_SENSE_ACTIVE, CMDST_SENSE_DONE, SCSI_SIOP_NOCHECK, SCSI_SIOP_NOSTATUS, SF_BUS_ULTRA3,
    SF_BUS_WIDE, SF_CHIP_BE, SF_CHIP_LED0, SF_CHIP_RAM, SIOP_NEG_ACK, SIOP_NEG_MSGOUT,
    SIOP_NOOFFSET, SIOP_NSG, ScrTable, SiopCommonCmd, SiopCommonSoftc, SiopCommonTarget,
    SiopCommonXfer, TARF_DT, TARF_ISDT, TARF_ISWIDE, TARF_SYNC, TARF_TAG, TARST_ASYNC, TARST_OK,
    TARST_PROBING, TARST_SYNC_NEG, dma_get, dma_set, siop_ctoh32, siop_htoc32,
};
use crate::dev::microcode::siop::siop::{
    A_flag_data, A_int_disc, A_int_done, A_int_err, A_int_extmsgdata, A_int_extmsgin, A_int_msgin,
    A_int_resellun, A_int_reseltag, A_int_reseltarg, A_int_resfail, A_int_saveoffset,
    E_abs_lunsw_return_Used, E_abs_msgin_Used, E_ldsa_abs_data_Used, E_ldsa_abs_reselect_Used,
    E_ldsa_abs_reselected_Used, E_ldsa_abs_selected_Used, E_ldsa_abs_slot_Used, Ent_get_extmsgdata,
    Ent_ldsa_data, Ent_ldsa_reload_dsa, Ent_ldsa_select, Ent_led_off, Ent_led_on1, Ent_led_on2,
    Ent_lun_switch_entry, Ent_lunsw_return, Ent_msgin, Ent_msgin_ack, Ent_msgin_space, Ent_rdsa0,
    Ent_rdsa1, Ent_rdsa2, Ent_rdsa3, Ent_resel_tag0, Ent_resel_targ0, Ent_reselect, Ent_reselected,
    Ent_restore_scntl3, Ent_script_sched, Ent_script_sched_slot0, Ent_selected, Ent_send_msgout,
    Ent_status, Ent_tag_switch_entry, load_dsa, lun_switch, siop_led_off, siop_led_on, siop_script,
    tag_switch,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_STREAMING,
    BUS_DMA_WRITE, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusAddr, BusDmaSegment, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_unmap, bus_space_read_4, bus_space_write_4,
    bus_space_write_region_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsi_all::{
    INQUIRY, REQUEST_SENSE, SCSI_BUSY, SCSI_CHECK, SCSI_OK, SCSI_QUEUE_FULL, SID_QUAL,
    SID_QUAL_BAD_LU, ScsiGeneric, ScsiSense, ScsiSenseData, ScsiWire,
};
use crate::scsi::scsi_base::{sc_print_addr, scsi_done, scsi_iopool_init};
use crate::scsi::scsi_message::{
    MSG_EXT_PPR, MSG_EXT_SDTR, MSG_EXT_WDTR, MSG_EXTENDED, MSG_HEAD_OF_Q_TAG, MSG_IGN_WIDE_RESIDUE,
    MSG_MESSAGE_REJECT, MSG_ORDERED_Q_TAG, MSG_SIMPLE_Q_TAG,
};
use crate::scsi::scsiconf::{
    ITSDONE, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_POLL, ScsiAdapter, ScsiIo, ScsiLink, ScsiXfer,
    ScsibusAttachArgs, XS_BUSY, XS_DRIVER_STUFFUP, XS_NOERROR, XS_RESET, XS_SELTIMEOUT, XS_SENSE,
    XS_TIMEOUT, scsiprint,
};
use crate::sys::device::{Cfdriver, DV_DULL};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::param::{MAXPHYS, PAGE_SIZE, roundup};
use crate::sys::queue::TailqHead;

/// `SIOP_NCMDPB`: number of cmd descriptors per block.
pub const SIOP_NCMDPB: usize = PAGE_SIZE / size_of::<SiopXfer>();

/// `SIOP_NSLOTS`: number of scheduler slot (needs to match script).
pub const SIOP_NSLOTS: u32 = 40;

/// The bytes of `struct siop_common_xfer`, the offset of a command's `resel[]` script from
/// its DSA.
const XFER_TABLES: u32 = size_of::<SiopCommonXfer>() as u32;

/// `siop_wdtr_neg`, `siop_sdtr_neg` or `siop_ppr_neg`: a negotiation's answer.
type SiopNegFn = fn(&SiopCommonCmd) -> i32;

/// Where `siop_intr` goes after its checks: `goto reset` or `goto end`.
enum Goto {
    /// `reset:`: fatal error, reset the bus.
    Reset,
    /// `end:`: the command is done; complete it and restart the script.
    End,
}

/// `siop_cd`.
pub static SIOP_CD: Cfdriver = Cfdriver::new(b"siop", DV_DULL, 0);

/// `siop_switch`: the adapter's entry points.
pub static SIOP_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: siop_scsicmd,
    dev_minphys: None,
    dev_probe: Some(siop_scsiprobe),
    dev_free: Some(siop_scsifree),
    ioctl: None,
};

/// Structures `siop_malloc` may hand out zeroed.
///
/// # Safety
///
/// Every member of the type is valid as all-zero bytes and it has no `Drop`.
unsafe trait SiopZeroed {}

// SAFETY: `Cell`s of integers and of `Option`s of `NonNull`s, all-zero valid.
unsafe impl SiopZeroed for SiopTarget {}
// SAFETY: `Cell`s of integers and of `Option`s of references, all-zero valid.
unsafe impl SiopZeroed for SiopLun {}
// SAFETY: a queue entry (null links) and `Cell`s of integers.
unsafe impl SiopZeroed for SiopLunsw {}
// SAFETY: a queue entry (null links), a `Cell` of a raw pointer and `Cell`s of `Option`s of
// references.
unsafe impl SiopZeroed for SiopCbd {}
// SAFETY: a queue entry, the common command (`Cell`s of integers, raw pointers and
// `Option`s of references) and `Cell`s of integers and of an `Option` of a reference.
unsafe impl SiopZeroed for SiopCmd {}

/// `malloc(sizeof(T), M_DEVBUF, flags | M_ZERO)`.
fn siop_malloc<T: SiopZeroed>(flags: i32) -> Option<NonNull<T>> {
    malloc(size_of::<T>(), M_DEVBUF, flags | M_ZERO).map(NonNull::cast)
}

/// `free(p, M_DEVBUF, sizeof(T))`.
fn siop_free<T>(p: NonNull<T>) {
    free(p.cast(), M_DEVBUF, size_of::<T>());
}

/// `(struct siop_softc *)sc`: the siop softc whose common part `sc` is.
pub fn siop_softc(sc: &SiopCommonSoftc) -> &SiopSoftc {
    if !ptr::eq(sc.sc_dev.cfdata().cf_driver, &SIOP_CD) {
        panic(format_args!("{}: not a siop", sc.sc_dev.xname()));
    }
    // SAFETY: the device is a siop's (checked above), and every siop attachment's softc
    // (`siop_pci.rs`) is `#[repr(C)]` with a `SiopSoftc` first, itself headed by the
    // common softc.
    unsafe { &*ptr::from_ref(sc).cast::<SiopSoftc>() }
}

/// `&cbdp->cmds[i]`, `None` past the block's `SIOP_NCMDPB` commands.
fn siop_cbd_cmd(cbdp: &SiopCbd, i: usize) -> Option<&'static SiopCmd> {
    let cmds = cbdp.cmds.get();
    if cmds.is_null() || i >= SIOP_NCMDPB {
        return None;
    }
    // SAFETY: siop_morecbd allocates `SIOP_NCMDPB` commands at `cmds` for a block it links,
    // never freed afterwards.
    Some(unsafe { &*cmds.add(i) })
}

/// `xs->sc_link->bus->sb_adapter_softc`: the softc of the siop a link is on.
fn siop_link_softc(link: &ScsiLink) -> &'static SiopSoftc {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("siop: bus without an adapter softc"));
    }
    // SAFETY: siop_attach attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's links reach `siop_switch`; softcs are never freed.
    unsafe { &*p.cast::<SiopSoftc>().cast_const() }
}

/// `sc->targets[target]` as a `struct siop_target`, which the C dereferences unchecked.
fn siop_target_of(sc: &SiopSoftc, target: usize) -> &'static SiopTarget {
    match sc.target(target) {
        Some(t) => t,
        None => panic(format_args!(
            "{}: no state for target {target}",
            sc.sc_c.sc_dev.xname()
        )),
    }
}

/// `siop_target->siop_lun[lun]`, which the C dereferences unchecked.
fn siop_lun_of(sc: &SiopSoftc, target: usize, lun: usize) -> &'static SiopLun {
    match siop_target_of(sc, target).lun(lun) {
        Some(l) => l,
        None => panic(format_args!(
            "{}: no state for target {target} lun {lun}",
            sc.sc_c.sc_dev.xname()
        )),
    }
}

/// `siop_table_sync`: syncs a command's `struct siop_xfer`.
pub fn siop_table_sync(siop_cmd: &SiopCmd, ops: i32) {
    let sc = siop_cmd.cmd_c.sc();
    let xfers = siop_cmd.cbd().xfers();

    let offset = siop_cmd.cmd_c.dsa.get() - xfers.dva();
    bus_dmamap_sync(sc.dmat(), xfers.map(), offset, size_of::<SiopXfer>(), ops);
}

/// `siop_script_sync`: syncs the script page (without on-board RAM).
pub fn siop_script_sync(sc: &SiopSoftc, ops: i32) {
    if sc.sc_c.features.get() & SF_CHIP_RAM == 0 {
        bus_dmamap_sync(sc.sc_c.dmat(), sc.sc_c.scriptdma(), 0, PAGE_SIZE, ops);
    }
}

/// `siop_script_read`: word `offset` of the script.
pub fn siop_script_read(sc: &SiopSoftc, offset: u32) -> u32 {
    if sc.sc_c.features.get() & SF_CHIP_RAM != 0 {
        let (t, h) = sc.sc_c.ram();
        bus_space_read_4(t, h, offset as usize * 4)
    } else {
        let p = sc.sc_c.script_word(offset as usize);
        // SAFETY: a word of the script page (`script_word` checks the bounds); the chip
        // writes it behind the compiler's back, hence volatile.
        siop_ctoh32(&sc.sc_c, unsafe { ptr::read_volatile(p) })
    }
}

/// `siop_script_write`: sets word `offset` of the script.
pub fn siop_script_write(sc: &SiopSoftc, offset: u32, val: u32) {
    if sc.sc_c.features.get() & SF_CHIP_RAM != 0 {
        let (t, h) = sc.sc_c.ram();
        bus_space_write_4(t, h, offset as usize * 4, val);
    } else {
        let p = sc.sc_c.script_word(offset as usize);
        // SAFETY: as in siop_script_read; the chip reads the word by DMA.
        unsafe { ptr::write_volatile(p, siop_htoc32(&sc.sc_c, val)) };
    }
}

/// `sc->sc_c.sc_script[i] = siop_htoc32(&sc->sc_c, val)` (without on-board RAM).
fn siop_script_store(sc: &SiopSoftc, i: usize, val: u32) {
    let p = sc.sc_c.script_word(i);
    // SAFETY: as in siop_script_read.
    unsafe { ptr::write_volatile(p, siop_htoc32(&sc.sc_c, val)) };
}

/// The script's address on the bus, as the 32-bit word the SCRIPTS use.
fn siop_scriptaddr(sc: &SiopSoftc) -> u32 {
    sc.sc_c.sc_scriptaddr.get() as u32
}

/// `siop_attach`: sets the adapter up, resets the bus and the chip and attaches its
/// scsibus.
pub fn siop_attach(sc: &'static SiopSoftc) {
    if siop_common_attach(&sc.sc_c).is_err() {
        return;
    }

    sc.free_list.init();
    sc.ready_list.init();
    sc.urgent_list.init();
    sc.cmds.init();
    sc.lunsw_list.init();
    // SAFETY: siop_cmd_get and siop_cmd_put take this softc as their cookie, and the softc
    // is never freed.
    unsafe {
        scsi_iopool_init(
            &sc.iopool,
            ptr::from_ref(sc).cast_mut().cast(),
            siop_cmd_get,
            siop_cmd_put,
        )
    };
    sc.sc_currschedslot.set(0);

    // Start with one page worth of commands
    siop_morecbd(sc);

    // Do a bus reset, so that devices fall back to narrow/async
    siop_resetbus(&sc.sc_c);
    // siop_reset() will reset the chip, thus clearing pending interrupts
    siop_reset(sc);

    let mut saa = ScsibusAttachArgs::new();
    saa.saa_adapter_softc = ptr::from_ref(sc).cast_mut().cast();
    saa.saa_adapter = Some(&SIOP_SWITCH);
    saa.saa_adapter_target = sc.sc_c.sc_id.get();
    saa.saa_adapter_buswidth = if sc.sc_c.features.get() & SF_BUS_WIDE != 0 {
        16
    } else {
        8
    };
    saa.saa_luns = 8;
    saa.saa_openings = SIOP_NTAG as u16;
    saa.saa_pool = Some(&sc.iopool);
    saa.saa_quirks = 0;
    saa.saa_flags = 0;
    saa.saa_wwpn = 0;
    saa.saa_wwnn = 0;

    config_found(
        &sc.sc_c.sc_dev,
        ptr::from_mut(&mut saa).cast(),
        Some(scsiprint),
    );
}

/// `siop_reset`: resets the chip, copies and patches the script, rebuilds the reselect
/// switch of the known targets and starts the script.
pub fn siop_reset(sc: &SiopSoftc) {
    let c = &sc.sc_c;
    siop_common_reset(c);

    // copy and patch the script
    let msgin_space = siop_scriptaddr(sc).wrapping_add(Ent_msgin_space);
    if c.features.get() & SF_CHIP_RAM != 0 {
        let (t, h) = c.ram();
        bus_space_write_region_4(t, h, 0, &siop_script);
        for &j in &E_abs_msgin_Used {
            bus_space_write_4(t, h, j as usize * 4, msgin_space);
        }
        if c.features.get() & SF_CHIP_LED0 != 0 {
            bus_space_write_region_4(t, h, Ent_led_on1 as usize, &siop_led_on);
            bus_space_write_region_4(t, h, Ent_led_on2 as usize, &siop_led_on);
            bus_space_write_region_4(t, h, Ent_led_off as usize, &siop_led_off);
        }
    } else {
        for (j, &w) in siop_script.iter().enumerate() {
            siop_script_store(sc, j, w);
        }
        for &j in &E_abs_msgin_Used {
            siop_script_store(sc, j as usize, msgin_space);
        }
        if c.features.get() & SF_CHIP_LED0 != 0 {
            for (j, &w) in siop_led_on.iter().enumerate() {
                siop_script_store(sc, Ent_led_on1 as usize / 4 + j, w);
            }
            for (j, &w) in siop_led_on.iter().enumerate() {
                siop_script_store(sc, Ent_led_on2 as usize / 4 + j, w);
            }
            for (j, &w) in siop_led_off.iter().enumerate() {
                siop_script_store(sc, Ent_led_off as usize / 4 + j, w);
            }
        }
    }
    sc.script_free_lo.set(siop_script.len() as u32);
    sc.script_free_hi.set((c.ram_size.get() / 4) as u32);
    sc.sc_ntargets.set(0);

    // free used and unused lun switches
    while let Some(lunsw) = sc.lunsw_list.first() {
        // SAFETY: the first element of the list.
        unsafe { sc.lunsw_list.remove(lunsw) };
        siop_free(NonNull::from(lunsw));
    }
    sc.lunsw_list.init();
    // restore reselect switch
    let buswidth = if c.features.get() & SF_BUS_WIDE != 0 {
        16
    } else {
        8
    };
    for i in 0..buswidth {
        let Some(target) = sc.target(i) else {
            continue;
        };
        if let Some(l) = target.lunsw.get() {
            siop_free(l);
        }
        target.lunsw.set(siop_get_lunsw(sc));
        if target.lunsw.get().is_none() {
            printf(format_args!(
                "{}: can't alloc lunsw for target {i}\n",
                c.sc_dev.xname()
            ));
            break;
        }
        siop_add_reselsw(sc, i);
    }

    // start script
    if c.features.get() & SF_CHIP_RAM == 0 {
        bus_dmamap_sync(
            c.dmat(),
            c.scriptdma(),
            0,
            PAGE_SIZE,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
    }
    c.write_4(SIOP_DSP, siop_scriptaddr(sc).wrapping_add(Ent_reselect));
}

/// `CALL_SCRIPT(ent)`: restarts the SCRIPTS processor at `ent`.
fn call_script(sc: &SiopSoftc, ent: u32) {
    sc.sc_c
        .write_4(SIOP_DSP, siop_scriptaddr(sc).wrapping_add(ent));
}

/// `sc_print_addr(xs->sc_link)` when there is a transfer, else `"<adapter>: "`.
fn siop_print_addr(sc: &SiopSoftc, xs: Option<&ScsiXfer>) {
    match xs.and_then(|xs| xs.sc_link.get()) {
        Some(link) => sc_print_addr(link),
        None => {
            printf(format_args!("{}: ", sc.sc_c.sc_dev.xname()));
        }
    }
}

/// The current command of an interrupt, which a script interrupt without the 0x80 bit
/// always has (`siop_intr` resets the bus otherwise).
fn siop_intr_cmd(cmd: Option<&'static SiopCmd>) -> &'static SiopCmd {
    match cmd {
        Some(cmd) => cmd,
        None => panic(format_args!("siop_intr: no current command")),
    }
}

/// `siop_intr`'s `DIAGNOSTIC` checks of the command its DSA names: false (with the
/// message) when the command is not active, which the C then treats as no command; a
/// message when its tag slot holds another command. Always true without `diagnostic`.
fn siop_intr_diagnostic(cmd: &SiopCmd, link: &ScsiLink, dsa: u32) -> bool {
    if !cfg!(feature = "diagnostic") {
        return true;
    }
    let lun = link.lun.get();
    let status = cmd.cmd_c.status.get();
    if status != CMDST_ACTIVE && status != CMDST_SENSE_ACTIVE {
        printf(format_args!(
            "siop_cmd (lun {lun}) for DSA {dsa:#x} not active ({status})\n"
        ));
        return false;
    }
    let tag = cmd.cmd_c.tag.get();
    let active = cmd.siop_target().lun(usize::from(lun)).and_then(|l| {
        usize::try_from(tag)
            .ok()
            .and_then(|t| l.siop_tag.get(t))
            .and_then(|t| t.active.get())
    });
    if !active.is_some_and(|a| ptr::eq(a, cmd)) {
        printf(format_args!(
            "siop_cmd (lun {lun} tag {tag}) not in siop_lun active ({:p} != {:p})\n",
            cmd,
            active.map_or(ptr::null(), ptr::from_ref)
        ));
    }
    true
}

/// `siop_intr`: the interrupt handler (also polled by `siop_scsicmd`).
#[allow(non_upper_case_globals)] // the microcode's names (`A_int_*`) as match patterns
pub fn siop_intr(v: *mut c_void) -> i32 {
    // SAFETY: siop_pci_attach_common establishes this handler with the adapter's common
    // softc, the head of its `SiopSoftc`, which lives as long as the kernel; siop_scsicmd
    // polls it with the same pointer.
    let sc: &'static SiopSoftc = unsafe { &*v.cast::<SiopSoftc>().cast_const() };
    let c = &sc.sc_c;
    let xname = c.sc_dev.xname();
    let mut dstat: u8 = 0;
    let mut need_reset = false;
    let mut restart = false;

    let istat = c.read_1(SIOP_ISTAT);
    if istat & (ISTAT_INTF | ISTAT_DIP | ISTAT_SIP) == 0 {
        return 0;
    }
    if istat & ISTAT_INTF != 0 {
        printf(format_args!("INTRF\n"));
        c.write_1(SIOP_ISTAT, ISTAT_INTF);
    }
    if istat & (ISTAT_DIP | ISTAT_SIP | ISTAT_ABRT) == (ISTAT_DIP | ISTAT_ABRT) {
        // clear abort
        c.write_1(SIOP_ISTAT, 0);
    }
    // use DSA to find the current siop_cmd
    let mut siop_cmd: Option<&'static SiopCmd> = None;
    let dsa_reg = c.read_4(SIOP_DSA);
    let dsa = dsa_reg as BusAddr;
    for cbdp in sc.cmds.iter() {
        let dva = cbdp.xfers().dva();
        if dsa >= dva && dsa < dva + PAGE_SIZE {
            siop_cmd = siop_cbd_cmd(cbdp, (dsa - dva) / size_of::<SiopXfer>());
            if let Some(cmd) = siop_cmd {
                siop_table_sync(cmd, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
            }
            break;
        }
    }
    // A command that never carried a transfer has no target or lun (module deviations).
    let cur = siop_cmd
        .and_then(|cmd| {
            let xs = cmd.cmd_c.xs.get()?;
            let link = xs.sc_link.get()?;
            Some((cmd, xs, link))
        })
        .filter(|&(cmd, _, link)| siop_intr_diagnostic(cmd, link, dsa_reg));
    let mut xs: Option<&'static ScsiXfer> = None;
    let mut target: i32 = -1;
    let mut lun: i32 = -1;
    let mut tag: i32 = -1;
    let mut siop_lun: Option<&'static SiopLun> = None;
    if let Some((cmd, cxs, link)) = cur {
        xs = Some(cxs);
        target = i32::from(link.target.get());
        lun = i32::from(link.lun.get());
        tag = cmd.cmd_c.tag.get();
        siop_lun = cmd.siop_target().lun(lun as usize);
    }
    let siop_cmd: Option<&'static SiopCmd> = cur.map(|(cmd, _, _)| cmd);

    let goto: Goto = 'body: {
        if istat & ISTAT_DIP != 0 {
            dstat = c.read_1(SIOP_DSTAT);
            if dstat & DSTAT_ABRT != 0 {
                // was probably generated by a bus reset IOCTL
                if dstat & DSTAT_DFE == 0 {
                    siop_clearfifo(c);
                }
                break 'body Goto::Reset;
            }
            if dstat & DSTAT_SSI != 0 {
                printf(format_args!(
                    "single step dsp {:#010x} dsa 0x08{:x}\n",
                    c.read_4(SIOP_DSP).wrapping_sub(siop_scriptaddr(sc)),
                    c.read_4(SIOP_DSA)
                ));
                if dstat & !(DSTAT_DFE | DSTAT_SSI) == 0 && istat & ISTAT_SIP == 0 {
                    c.write_1(SIOP_DCNTL, c.read_1(SIOP_DCNTL) | DCNTL_STD);
                }
                return 1;
            }

            if dstat & !(DSTAT_SIR | DSTAT_DFE | DSTAT_SSI) != 0 {
                printf(format_args!("{xname}: DMA IRQ:"));
                if dstat & DSTAT_IID != 0 {
                    printf(format_args!(" illegal instruction"));
                }
                if dstat & DSTAT_BF != 0 {
                    printf(format_args!(" bus fault"));
                }
                if dstat & DSTAT_MDPE != 0 {
                    printf(format_args!(" parity"));
                }
                if dstat & DSTAT_DFE != 0 {
                    printf(format_args!(" DMA fifo empty"));
                } else {
                    siop_clearfifo(c);
                }
                printf(format_args!(
                    ", DSP={:#x} DSA={:#x}: ",
                    c.read_4(SIOP_DSP).wrapping_sub(siop_scriptaddr(sc)),
                    c.read_4(SIOP_DSA)
                ));
                match siop_cmd {
                    Some(cmd) => {
                        let t = cmd.tables();
                        printf(format_args!(
                            "last msg_in={:#x} status={:#x}\n",
                            t.msg_in(0),
                            siop_ctoh32(c, dma_get(&t.status))
                        ));
                    }
                    None => {
                        printf(format_args!("current DSA invalid\n"));
                    }
                }
                need_reset = true;
            }
        }
        let mut scintr = false;
        let sist: u16;
        let sstat1: u8;
        if istat & ISTAT_SIP != 0 {
            if istat & ISTAT_DIP != 0 {
                delay(10);
            }
            // Can't read sist0 & sist1 independently, or we have to insert delay
            sist = c.read_2(SIOP_SIST0);
            sstat1 = c.read_1(SIOP_SSTAT1);
            if sist & u16::from(SIST0_RST) != 0 {
                siop_handle_reset(sc);
                siop_start(sc);
                // no table to flush here
                return 1;
            }
            if sist & u16::from(SIST0_SGE) != 0 {
                siop_print_addr(sc, if siop_cmd.is_some() { xs } else { None });
                printf(format_args!("scsi gross error\n"));
                break 'body Goto::Reset;
            }
            if sist & u16::from(SIST0_MA) != 0 && !need_reset {
                if let Some(cmd) = siop_cmd {
                    // XXX Why read DSTAT again?
                    dstat = c.read_1(SIOP_DSTAT);
                    // first restore DSA, in case we were in a S/G operation.
                    c.write_4(SIOP_DSA, cmd.cmd_c.dsa.get() as u32);
                    let scratcha0 = c.read_1(SIOP_SCRATCHA);
                    match sstat1 & SSTAT1_PHASE_MASK {
                        SSTAT1_PHASE_STATUS => {
                            // previous phase may be aborted for any reason ( for example,
                            // the target has less data to transfer than requested).
                            // Compute resid and just go to status, the command should
                            // terminate.
                            if u32::from(scratcha0) & A_flag_data != 0 {
                                siop_ma(&cmd.cmd_c);
                            } else if dstat & DSTAT_DFE == 0 {
                                siop_clearfifo(c);
                            }
                            call_script(sc, Ent_status);
                            return 1;
                        }
                        SSTAT1_PHASE_MSGIN => {
                            // target may be ready to disconnect Compute resid which would
                            // be used later if a save data pointer is needed.
                            if u32::from(scratcha0) & A_flag_data != 0 {
                                siop_ma(&cmd.cmd_c);
                            } else if dstat & DSTAT_DFE == 0 {
                                siop_clearfifo(c);
                            }
                            c.write_1(SIOP_SCRATCHA, scratcha0 & !(A_flag_data as u8));
                            call_script(sc, Ent_msgin);
                            return 1;
                        }
                        _ => {}
                    }
                    printf(format_args!(
                        "{xname}: unexpected phase mismatch {}\n",
                        sstat1 & SSTAT1_PHASE_MASK
                    ));
                } else {
                    printf(format_args!("{xname}: phase mismatch without command\n"));
                }
                need_reset = true;
            }
            if sist & u16::from(SIST0_PAR) != 0 {
                // parity error, reset
                siop_print_addr(sc, if siop_cmd.is_some() { xs } else { None });
                printf(format_args!("parity error\n"));
                break 'body Goto::Reset;
            }
            if sist & (u16::from(SIST1_STO) << 8) != 0 && !need_reset {
                // selection time out, assume there's no device here
                if let (Some(cmd), Some(x)) = (siop_cmd, xs) {
                    cmd.cmd_c.status.set(CMDST_DONE);
                    x.error.set(XS_SELTIMEOUT);
                    break 'body Goto::End;
                } else {
                    printf(format_args!("{xname}: selection timeout without command\n"));
                    need_reset = true;
                }
            }
            if sist & u16::from(SIST0_UDC) != 0 {
                // unexpected disconnect. Usually the target signals a fatal condition this
                // way. Attempt to get sense.
                if let Some(cmd) = siop_cmd {
                    dma_set(&cmd.tables().status, siop_htoc32(c, u32::from(SCSI_CHECK)));
                    break 'body Goto::End;
                }
                printf(format_args!(
                    "{xname}: unexpected disconnect without command\n"
                ));
                break 'body Goto::Reset;
            }
            if sist & (u16::from(SIST1_SBMC) << 8) != 0 {
                // SCSI bus mode change
                if !siop_modechange(c) || need_reset {
                    break 'body Goto::Reset;
                }
                if istat & ISTAT_DIP != 0 && dstat & DSTAT_SIR != 0 {
                    // we have a script interrupt, it will restart the script.
                    scintr = true;
                } else {
                    // else we have to restart it ourselves, at the interrupted
                    // instruction.
                    c.write_4(SIOP_DSP, c.read_4(SIOP_DSP).wrapping_sub(8));
                    return 1;
                }
            }
            if !scintr {
                // Else it's an unhandled exception (for now).
                printf(format_args!(
                    "{xname}: unhandled scsi interrupt, sist={sist:#x} sstat1={sstat1:#x} \
                     DSA={:#x} DSP={:#x}\n",
                    c.read_4(SIOP_DSA),
                    c.read_4(SIOP_DSP).wrapping_sub(siop_scriptaddr(sc))
                ));
                if let (Some(cmd), Some(x)) = (siop_cmd, xs) {
                    cmd.cmd_c.status.set(CMDST_DONE);
                    x.error.set(XS_SELTIMEOUT);
                    break 'body Goto::End;
                }
                need_reset = true;
            }
        } else {
            sist = 0;
            sstat1 = 0;
        }
        if !scintr && need_reset {
            break 'body Goto::Reset;
        }

        // scintr:
        let mut irqcode: u32 = 0;
        if istat & ISTAT_DIP != 0 && dstat & DSTAT_SIR != 0 {
            // script interrupt
            irqcode = c.read_4(SIOP_DSPS);
            // no command, or an inactive command is only valid for a reselect interrupt
            if irqcode & 0x80 == 0 {
                let Some(cmd) = siop_cmd else {
                    printf(format_args!(
                        "{xname}: script interrupt ({irqcode:#x}) with invalid DSA !!!\n"
                    ));
                    break 'body Goto::Reset;
                };
                let status = cmd.cmd_c.status.get();
                if status != CMDST_ACTIVE && status != CMDST_SENSE_ACTIVE {
                    printf(format_args!(
                        "{xname}: command with invalid status (IRQ code {irqcode:#x} \
                         current status {status}) !\n"
                    ));
                    xs = None;
                }
            }
            match irqcode {
                A_int_err => {
                    printf(format_args!(
                        "error, DSP={:#x}\n",
                        c.read_4(SIOP_DSP).wrapping_sub(siop_scriptaddr(sc))
                    ));
                    if let Some(x) = xs {
                        x.error.set(XS_SELTIMEOUT);
                        break 'body Goto::End;
                    } else {
                        break 'body Goto::Reset;
                    }
                }
                A_int_reseltarg => {
                    printf(format_args!("{xname}: reselect with invalid target\n"));
                    break 'body Goto::Reset;
                }
                A_int_resellun => {
                    let target = usize::from(c.read_1(SIOP_SCRATCHA) & 0xf);
                    let lun = usize::from(c.read_1(SIOP_SCRATCHA + 1));
                    let tag = usize::from(c.read_1(SIOP_SCRATCHA + 2));
                    let Some(siop_target) = sc.target(target) else {
                        printf(format_args!(
                            "{xname}: reselect with invalid target {target}\n"
                        ));
                        break 'body Goto::Reset;
                    };
                    let Some(siop_lun) = siop_target
                        .siop_lun
                        .get(lun)
                        .and_then(|_| siop_target.lun(lun))
                    else {
                        printf(format_args!(
                            "{xname}: target {target} reselect with invalid lun {lun}\n"
                        ));
                        break 'body Goto::Reset;
                    };
                    let Some(siop_cmd) = siop_lun.siop_tag.get(tag).and_then(|t| t.active.get())
                    else {
                        printf(format_args!(
                            "{xname}: target {target} lun {lun} tag {tag} reselect without \
                             command\n"
                        ));
                        break 'body Goto::Reset;
                    };
                    c.write_4(
                        SIOP_DSP,
                        (siop_cmd.cmd_c.dsa.get() as u32)
                            .wrapping_add(XFER_TABLES)
                            .wrapping_add(Ent_ldsa_reload_dsa),
                    );
                    siop_table_sync(siop_cmd, BUS_DMASYNC_PREWRITE);
                    return 1;
                }
                A_int_reseltag => {
                    printf(format_args!("{xname}: reselect with invalid tag\n"));
                    break 'body Goto::Reset;
                }
                A_int_msgin => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    let tables = cmd.tables();
                    let msgin = c.read_1(SIOP_SFBR);
                    if msgin == MSG_MESSAGE_REJECT {
                        let (msg, extmsg) = if tables.msg_out(0) & 0x80 != 0 {
                            // message was part of a identify + something else. Identify
                            // shouldn't have been rejected.
                            (tables.msg_out(1), tables.msg_out(3))
                        } else {
                            (tables.msg_out(0), tables.msg_out(2))
                        };
                        if msg == MSG_MESSAGE_REJECT {
                            // MSG_REJECT  for a MSG_REJECT  !
                            siop_print_addr(sc, xs);
                            printf(format_args!("our reject message was rejected\n"));
                            break 'body Goto::Reset;
                        }
                        let st = cmd.siop_target();
                        if msg == MSG_EXTENDED && extmsg == MSG_EXT_WDTR {
                            // WDTR rejected, initiate sync
                            if st.target_c.flags.get() & TARF_SYNC == 0 {
                                st.target_c.status.set(TARST_OK);
                                siop_update_xfer_mode(c, target as usize);
                                // no table to flush here
                                call_script(sc, Ent_msgin_ack);
                                return 1;
                            }
                            st.target_c.status.set(TARST_SYNC_NEG);
                            siop_sdtr_msg(&cmd.cmd_c, 0, c.st_minsync.get(), c.maxoff.get());
                            siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                            call_script(sc, Ent_send_msgout);
                            return 1;
                        } else if msg == MSG_EXTENDED && extmsg == MSG_EXT_SDTR {
                            // sync rejected
                            st.target_c.offset.set(0);
                            st.target_c.period.set(0);
                            st.target_c.status.set(TARST_OK);
                            siop_update_xfer_mode(c, target as usize);
                            // no table to flush here
                            call_script(sc, Ent_msgin_ack);
                            return 1;
                        } else if msg == MSG_EXTENDED && extmsg == MSG_EXT_PPR {
                            // PPR negotiation rejected
                            st.target_c.offset.set(0);
                            st.target_c.period.set(0);
                            st.target_c.status.set(TARST_ASYNC);
                            st.target_c
                                .flags
                                .set(st.target_c.flags.get() & !(TARF_DT | TARF_ISDT));
                            call_script(sc, Ent_msgin_ack);
                            return 1;
                        } else if msg == MSG_SIMPLE_Q_TAG
                            || msg == MSG_HEAD_OF_Q_TAG
                            || msg == MSG_ORDERED_Q_TAG
                        {
                            if !siop_handle_qtag_reject(cmd) {
                                break 'body Goto::Reset;
                            }
                            call_script(sc, Ent_msgin_ack);
                            return 1;
                        }
                        siop_print_addr(sc, xs);
                        if msg == MSG_EXTENDED {
                            printf(format_args!(
                                "scsi message reject, extended message sent was {extmsg:#x}\n"
                            ));
                        } else {
                            printf(format_args!(
                                "scsi message reject, message sent was {msg:#x}\n"
                            ));
                        }
                        // no table to flush here
                        call_script(sc, Ent_msgin_ack);
                        return 1;
                    }
                    if msgin == MSG_IGN_WIDE_RESIDUE {
                        // use the extmsgdata table to get the second byte
                        SiopCommonXfer::set_count(&tables.t_extmsgdata, siop_htoc32(c, 1));
                        siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                        call_script(sc, Ent_get_extmsgdata);
                        return 1;
                    }
                    siop_print_addr(sc, xs);
                    printf(format_args!("unhandled message {:#x}\n", tables.msg_in(0)));
                    tables.set_msg_out(0, MSG_MESSAGE_REJECT);
                    SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(c, 1));
                    siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                    call_script(sc, Ent_send_msgout);
                    return 1;
                }
                A_int_extmsgin => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    let tables = cmd.tables();
                    let len = tables.msg_in(1);
                    if usize::from(len) > tables.msg_in.len() - 2 {
                        printf(format_args!("{xname}: extended message too big ({len})\n"));
                    }
                    SiopCommonXfer::set_count(
                        &tables.t_extmsgdata,
                        siop_htoc32(c, u32::from(len).wrapping_sub(1)),
                    );
                    siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                    call_script(sc, Ent_get_extmsgdata);
                    return 1;
                }
                A_int_extmsgdata => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    let tables = cmd.tables();
                    if tables.msg_in(0) == MSG_IGN_WIDE_RESIDUE {
                        // we got the second byte of MSG_IGN_WIDE_RESIDUE
                        if tables.msg_in(3) != 1 {
                            printf(format_args!(
                                "MSG_IGN_WIDE_RESIDUE: bad len {}\n",
                                tables.msg_in(3)
                            ));
                        }
                        match siop_iwr(&cmd.cmd_c) {
                            SIOP_NEG_MSGOUT => {
                                siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                                call_script(sc, Ent_send_msgout);
                                return 1;
                            }
                            SIOP_NEG_ACK => {
                                call_script(sc, Ent_msgin_ack);
                                return 1;
                            }
                            _ => panic(format_args!("invalid retval from siop_iwr()")),
                        }
                    }
                    let neg: Option<(SiopNegFn, &str)> = match tables.msg_in(2) {
                        MSG_EXT_WDTR => Some((siop_wdtr_neg, "siop_wdtr_neg()")),
                        MSG_EXT_SDTR => Some((siop_sdtr_neg, "siop_sdtr_neg()")),
                        // The C's message names siop_wdtr_neg() here too.
                        MSG_EXT_PPR => Some((siop_ppr_neg, "siop_wdtr_neg()")),
                        _ => None,
                    };
                    if let Some((negotiate, name)) = neg {
                        match negotiate(&cmd.cmd_c) {
                            SIOP_NEG_MSGOUT => {
                                siop_update_scntl3(sc, cmd.cmd_c.target());
                                siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                                call_script(sc, Ent_send_msgout);
                                return 1;
                            }
                            SIOP_NEG_ACK => {
                                siop_update_scntl3(sc, cmd.cmd_c.target());
                                call_script(sc, Ent_msgin_ack);
                                return 1;
                            }
                            _ => panic(format_args!("invalid retval from {name}")),
                        }
                    }
                    // send a message reject
                    tables.set_msg_out(0, MSG_MESSAGE_REJECT);
                    SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(c, 1));
                    siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                    call_script(sc, Ent_send_msgout);
                    return 1;
                }
                A_int_disc => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    let offset = usize::from(c.read_1(SIOP_SCRATCHA + 1));
                    siop_sdp(&cmd.cmd_c, offset);
                    // we start again with no offset
                    cmd.saved_offset.set(SIOP_NOOFFSET);
                    siop_table_sync(cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
                    call_script(sc, Ent_script_sched);
                    return 1;
                }
                A_int_saveoffset => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    let offset = c.read_1(SIOP_SCRATCHA + 1);
                    cmd.saved_offset.set(u32::from(offset));
                    call_script(sc, Ent_script_sched);
                    return 1;
                }
                A_int_resfail => {
                    printf(format_args!("reselect failed\n"));
                    // check if we can put some command in scheduler
                    siop_start(sc);
                    call_script(sc, Ent_script_sched);
                    return 1;
                }
                A_int_done => {
                    let cmd = siop_intr_cmd(siop_cmd);
                    if xs.is_none() {
                        printf(format_args!(
                            "{xname}: done without command, DSA={:#x}\n",
                            cmd.cmd_c.dsa.get()
                        ));
                        cmd.cmd_c.status.set(CMDST_FREE);
                        siop_start(sc);
                        call_script(sc, Ent_script_sched);
                        return 1;
                    }
                    // update resid.
                    let mut offset = u32::from(c.read_1(SIOP_SCRATCHA + 1));
                    // if we got a disconnect between the last data phase and the status
                    // phase, offset will be 0. In this case, siop_cmd->saved_offset will
                    // have the proper value if it got updated by the controller
                    if offset == 0 && cmd.saved_offset.get() != SIOP_NOOFFSET {
                        offset = cmd.saved_offset.get();
                    }
                    siop_update_resid(&cmd.cmd_c, offset as usize);
                    if cmd.cmd_c.status.get() == CMDST_SENSE_ACTIVE {
                        cmd.cmd_c.status.set(CMDST_SENSE_DONE);
                    } else {
                        cmd.cmd_c.status.set(CMDST_DONE);
                    }
                    break 'body Goto::End;
                }
                _ => {
                    printf(format_args!("unknown irqcode {irqcode:x}\n"));
                    if let Some(x) = xs {
                        x.error.set(XS_SELTIMEOUT);
                        break 'body Goto::End;
                    }
                    break 'body Goto::Reset;
                }
            }
        }
        // We can get here if ISTAT_DIP and DSTAT_DFE are the only bits set.
        // But that *SHOULDN'T* happen. It does on powerpc (at least).
        printf(format_args!(
            "{xname}: siop_intr() - we should not be here!\n   istat = {istat:#x}, dstat = \
             {dstat:#x}, sist = {sist:#x}, sstat1 = {sstat1:#x}\n   need_reset = {:x}, \
             irqcode = {irqcode:x}, siop_cmd {}\n",
            i32::from(need_reset),
            if siop_cmd.is_none() {
                "== NULL"
            } else {
                "!= NULL"
            }
        ));
        Goto::Reset // Where we should have gone in the first place!
    };

    if let Goto::Reset = goto {
        // fatal error, reset the bus
        siop_resetbus(c);
        // no table to flush here
        return 1;
    }

    // end:
    // restart the script now if command completed properly. Otherwise wait for
    // siop_scsicmd_end(), we may need to cleanup the queue
    let cmd = siop_intr_cmd(siop_cmd);
    let Some(xs) = xs else {
        panic(format_args!("siop_intr: end without a transfer"));
    };
    xs.status
        .set(siop_ctoh32(c, dma_get(&cmd.tables().status)) as u8);
    if xs.status.get() == SCSI_OK {
        call_script(sc, Ent_script_sched);
    } else {
        restart = true;
    }
    match siop_lun.and_then(|l| usize::try_from(tag).ok().and_then(|t| l.siop_tag.get(t))) {
        Some(t) => t.active.set(None),
        None => panic(format_args!(
            "{xname}: command for target {target} lun {lun} tag {tag} without state"
        )),
    }
    siop_scsicmd_end(cmd);
    siop_start(sc);
    if restart {
        call_script(sc, Ent_script_sched);
    }
    1
}

/// `siop_scsicmd_end`: completes a command: its error from the SCSI status, a REQUEST
/// SENSE after a CHECK CONDITION, a requeue after QUEUE FULL.
pub fn siop_scsicmd_end(siop_cmd: &'static SiopCmd) {
    let xs = siop_cmd.cmd_c.xs();
    let sc = siop_softc(siop_cmd.cmd_c.sc());
    let c = &sc.sc_c;
    let link = xs.link();
    let siop_lun = siop_lun_of(
        sc,
        usize::from(link.target.get()),
        usize::from(link.lun.get()),
    );

    // If the command is re-queued (SENSE, QUEUE_FULL) it must get a new timeout, so delete
    // existing timeout now.
    timeout_del(&xs.stimeout);

    match xs.status.get() {
        SCSI_OK => {
            xs.error.set(if siop_cmd.cmd_c.status.get() == CMDST_DONE {
                XS_NOERROR
            } else {
                XS_SENSE
            });
        }
        SCSI_BUSY => xs.error.set(XS_BUSY),
        SCSI_CHECK => {
            if siop_cmd.cmd_c.status.get() == CMDST_SENSE_DONE {
                // request sense on a request sense ?
                printf(format_args!("{}: request sense failed\n", c.sc_dev.xname()));
                xs.error.set(XS_DRIVER_STUFFUP);
            } else {
                siop_cmd.cmd_c.status.set(CMDST_SENSE);
            }
        }
        SCSI_QUEUE_FULL => {
            // Device didn't queue the command. We have to retry it. We insert it into the
            // urgent list, hoping to preserve order. But unfortunately, commands already in
            // the scheduler may be accepted before this one. Also remember the condition,
            // to avoid starting new commands for this device before one is done.
            siop_lun
                .lun_flags
                .set(siop_lun.lun_flags.get() | SIOP_LUNF_FULL);
            siop_cmd.cmd_c.status.set(CMDST_READY);
            siop_setuptables(&siop_cmd.cmd_c);
            siop_table_sync(siop_cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
            // SAFETY: an active command is in no list; it stays in place (its block is never
            // freed).
            unsafe { sc.urgent_list.insert_tail(siop_cmd) };
            return;
        }
        s if u32::from(s) == SCSI_SIOP_NOCHECK => {
            // don't check status, xs->error is already valid
        }
        s if u32::from(s) == SCSI_SIOP_NOSTATUS => {
            // the status byte was not updated, cmd was aborted
            xs.error.set(XS_SELTIMEOUT);
        }
        _ => xs.error.set(XS_DRIVER_STUFFUP),
    }
    let dmat = c.dmat();
    let map = siop_cmd.cmd_c.dmamap();
    if siop_cmd.cmd_c.status.get() != CMDST_SENSE_DONE
        && xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) != 0
    {
        bus_dmamap_sync(
            dmat,
            map,
            0,
            map.dm_mapsize.get(),
            if xs.flags.get() & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
        bus_dmamap_unload(dmat, map);
    }
    'out: {
        if siop_cmd.cmd_c.status.get() == CMDST_SENSE {
            // issue a request sense for this target
            let tables = siop_cmd.tables();
            let cmd = ScsiSense {
                opcode: REQUEST_SENSE,
                byte2: (link.lun.get() << 5) as u8,
                unused: [0, 0],
                length: size_of::<ScsiSenseData>() as u8,
                control: 0,
            };
            let mut xscmd = ScsiGeneric::default();
            xscmd.as_bytes_mut()[..size_of::<ScsiSense>()].copy_from_slice(cmd.as_bytes());
            dma_set(&tables.xscmd, xscmd);
            SiopCommonXfer::set_count(&tables.cmd, siop_htoc32(c, size_of::<ScsiSense>() as u32));
            siop_cmd
                .cmd_c
                .flags
                .set(siop_cmd.cmd_c.flags.get() & !CMDFL_TAG);
            // SAFETY: the command's own sense buffer in its block's DMA memory, mapped forever;
            // unloaded below or by the next siop_scsicmd_end before the map is reused.
            if let Err(error) = unsafe {
                bus_dmamap_load(
                    dmat,
                    map,
                    siop_cmd.cmd_c.sense.get().cast(),
                    size_of::<ScsiSenseData>(),
                    None,
                    BUS_DMA_NOWAIT,
                )
            } {
                printf(format_args!(
                    "{}: unable to load data DMA map (for SENSE): {}\n",
                    c.sc_dev.xname(),
                    error as i32
                ));
                xs.error.set(XS_DRIVER_STUFFUP);
                break 'out;
            }
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

            siop_setuptables(&siop_cmd.cmd_c);
            siop_table_sync(siop_cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
            // arrange for the cmd to be handled now
            // SAFETY: as for the QUEUE FULL case above.
            unsafe { sc.urgent_list.insert_head(siop_cmd) };
            return;
        } else if siop_cmd.cmd_c.status.get() == CMDST_SENSE_DONE {
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
            bus_dmamap_unload(dmat, map);
            // SAFETY: the command's sense buffer (siop_morecbd), the chip done writing it.
            let sense = unsafe { ptr::read_volatile(siop_cmd.cmd_c.sense.get()) };
            xs.sense.set(sense);
        }
    }
    // out:
    siop_lun
        .lun_flags
        .set(siop_lun.lun_flags.get() & !SIOP_LUNF_FULL);
    scsi_done(xs);
}

/// `siop_handle_qtag_reject`: handle a rejected queue tag message: the command will run
/// untagged, has to adjust the reselect script. False for the C's -1.
pub fn siop_handle_qtag_reject(siop_cmd: &'static SiopCmd) -> bool {
    let sc = siop_softc(siop_cmd.cmd_c.sc());
    let link = siop_cmd.cmd_c.xs().link();
    let target = usize::from(link.target.get());
    let lun = usize::from(link.lun.get());
    let tag = usize::from(siop_cmd.tables().msg_out(2));
    let siop_lun = siop_lun_of(sc, target, lun);

    if let Some(active) = siop_lun.siop_tag[0].active.get() {
        printf(format_args!(
            "{}: untagged command already running for target {target} lun {lun} (status \
             {})\n",
            sc.sc_c.sc_dev.xname(),
            active.cmd_c.status.get()
        ));
        return false;
    }
    // clear tag slot
    siop_lun.siop_tag[tag].active.set(None);
    // add command to non-tagged slot
    siop_lun.siop_tag[0].active.set(Some(siop_cmd));
    siop_cmd.cmd_c.tag.set(0);
    // adjust reselect script if there is one
    if siop_lun.siop_tag[0].reseloff.get() > 0 {
        siop_script_write(
            sc,
            siop_lun.siop_tag[0].reseloff.get() + 1,
            (siop_cmd.cmd_c.dsa.get() as u32)
                .wrapping_add(XFER_TABLES)
                .wrapping_add(Ent_ldsa_reload_dsa),
        );
        siop_table_sync(siop_cmd, BUS_DMASYNC_PREWRITE);
    }
    true
}

/// `siop_handle_reset`: handle a bus reset: reset chip, unqueue all active commands, free
/// all target struct and report lossage to upper layer. As the upper layer may requeue
/// immediately we have to first store all active commands in a temporary queue.
pub fn siop_handle_reset(sc: &SiopSoftc) {
    let c = &sc.sc_c;
    let reset_list: TailqHead<CmdList> = TailqHead::new();

    // scsi bus reset. reset the chip and restart the queue. Need to clean up all active
    // commands
    printf(format_args!("{}: scsi bus reset\n", c.sc_dev.xname()));
    // stop, reset and restart the chip
    siop_reset(sc);
    reset_list.init();
    // Process all commands: first commands being executed
    let buswidth = if c.features.get() & SF_BUS_WIDE != 0 {
        16
    } else {
        8
    };
    for target in 0..buswidth {
        let Some(siop_target) = sc.target(target) else {
            continue;
        };
        for lun in 0..8 {
            let Some(siop_lun) = siop_target.lun(lun) else {
                continue;
            };
            siop_lun
                .lun_flags
                .set(siop_lun.lun_flags.get() & !SIOP_LUNF_FULL);
            let ntags = if siop_target.target_c.flags.get() & TARF_TAG != 0 {
                SIOP_NTAG
            } else {
                1
            };
            for (tag, t) in siop_lun.siop_tag.iter().take(ntags).enumerate() {
                let Some(siop_cmd) = t.active.get() else {
                    continue;
                };
                t.active.set(None);
                // SAFETY: an active command is in no list; `reset_list` stays in place
                // until it is empty again, below.
                unsafe { reset_list.insert_tail(siop_cmd) };
                sc_print_addr(siop_cmd.cmd_c.xs().link());
                printf(format_args!(
                    "cmd {:p} (tag {tag}) added to reset list\n",
                    siop_cmd
                ));
            }
        }
        let t = &siop_target.target_c;
        if t.status.get() != TARST_PROBING {
            t.status.set(TARST_ASYNC);
            t.flags.set(t.flags.get() & !TARF_ISWIDE);
            t.period.set(0);
            t.offset.set(0);
            siop_update_xfer_mode(c, target);
        }
    }
    // Next commands from the urgent list
    while let Some(siop_cmd) = sc.urgent_list.first() {
        // SAFETY: the first element of the urgent list moves to the reset list, which stays
        // in place until it is empty again.
        unsafe {
            sc.urgent_list.remove(siop_cmd);
            reset_list.insert_tail(siop_cmd);
        }
        sc_print_addr(siop_cmd.cmd_c.xs().link());
        printf(format_args!(
            "cmd {:p} added to reset list from urgent list\n",
            siop_cmd
        ));
    }
    // Then commands waiting in the input list.
    while let Some(siop_cmd) = sc.ready_list.first() {
        // SAFETY: as above, from the ready list.
        unsafe {
            sc.ready_list.remove(siop_cmd);
            reset_list.insert_tail(siop_cmd);
        }
        sc_print_addr(siop_cmd.cmd_c.xs().link());
        printf(format_args!(
            "cmd {:p} added to reset list from ready list\n",
            siop_cmd
        ));
    }

    while let Some(siop_cmd) = reset_list.first() {
        // SAFETY: commands live in their blocks forever; the reference outlives the list.
        let siop_cmd: &'static SiopCmd = unsafe { &*ptr::from_ref(siop_cmd) };
        let xs = siop_cmd.cmd_c.xs();
        siop_cmd
            .cmd_c
            .flags
            .set(siop_cmd.cmd_c.flags.get() & !CMDFL_TAG);
        xs.error
            .set(if siop_cmd.cmd_c.flags.get() & CMDFL_TIMEOUT != 0 {
                XS_TIMEOUT
            } else {
                XS_RESET
            });
        xs.status.set(SCSI_SIOP_NOCHECK as u8);
        sc_print_addr(xs.link());
        printf(format_args!(
            "cmd {:p} (status {}) reset",
            siop_cmd,
            siop_cmd.cmd_c.status.get()
        ));
        let status = siop_cmd.cmd_c.status.get();
        if status == CMDST_SENSE || status == CMDST_SENSE_ACTIVE {
            siop_cmd.cmd_c.status.set(CMDST_SENSE_DONE);
        } else {
            siop_cmd.cmd_c.status.set(CMDST_DONE);
        }
        printf(format_args!(
            " with status {}, xs->error {}\n",
            siop_cmd.cmd_c.status.get(),
            xs.error.get()
        ));
        // SAFETY: the first element of the reset list.
        unsafe { reset_list.remove(siop_cmd) };
        siop_scsicmd_end(siop_cmd);
    }
}

/// `siop_cmd_get`: the pool's `io_get`: a free command, now `CMDST_READY`.
///
/// # Safety
///
/// `cookie` is a live [`SiopSoftc`] (the one `siop_attach` gave `scsi_iopool_init`).
pub unsafe fn siop_cmd_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<SiopSoftc>().cast_const() };

    // Look if a ccb is available.
    let s = splbio();
    let siop_cmd = sc.free_list.first();
    if let Some(cmd) = siop_cmd {
        // SAFETY: the first element of the free list.
        unsafe { sc.free_list.remove(cmd) };
        #[cfg(feature = "diagnostic")]
        if cmd.cmd_c.status.get() != CMDST_FREE {
            panic(format_args!("siop_scsicmd: new cmd not free"));
        }
        cmd.cmd_c.status.set(CMDST_READY);
    }
    splx(s);

    siop_cmd.map(|cmd| NonNull::from(cmd).cast())
}

/// `siop_cmd_put`: the pool's `io_put`: gives a command back to the free list.
///
/// # Safety
///
/// `cookie` is a live [`SiopSoftc`] and `io` one of its commands, from [`siop_cmd_get`]
/// and in no list.
pub unsafe fn siop_cmd_put(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<SiopSoftc>().cast_const() };
    // SAFETY: the caller's guarantee: a command of one of the blocks, never freed.
    let siop_cmd: &'static SiopCmd = unsafe { io.cast::<SiopCmd>().as_ref() };

    let s = splbio();
    siop_cmd.cmd_c.status.set(CMDST_FREE);
    // SAFETY: the command is in no list (the caller's guarantee) and stays in place.
    unsafe { sc.free_list.insert_tail(siop_cmd) };
    splx(s);
}

/// `siop_scsiprobe`: the adapter's `dev_probe`: the target's and the lun's state.
pub fn siop_scsiprobe(link: &'static ScsiLink) -> Result<(), Errno> {
    let sc = siop_link_softc(link);
    let target = usize::from(link.target.get());
    let lun = usize::from(link.lun.get());
    let xname = sc.sc_c.sc_dev.xname();

    // XXX locking

    let siop_target = match sc.target(target) {
        Some(t) => t,
        None => {
            let Some(p) = siop_malloc::<SiopTarget>(M_WAITOK | M_CANFAIL) else {
                printf(format_args!(
                    "{xname}: can't malloc memory for target {target}\n"
                ));
                return Err(Errno::ENOMEM);
            };
            // SAFETY: a fresh zeroed allocation (valid, `SiopZeroed`), kept until
            // siop_scsifree.
            let siop_target: &'static SiopTarget = unsafe { &*p.as_ptr() };

            siop_target.target_c.status.set(TARST_PROBING);
            siop_target.target_c.flags.set(0);
            siop_target
                .target_c
                .id
                .set((sc.sc_c.clock_div.get() as u32) << 24); // scntl3
            siop_target
                .target_c
                .id
                .set(siop_target.target_c.id.get() | (target as u32) << 16); // id
            // siop_target->target_c.id |= 0x0 << 8; scxfer is 0

            // get a lun switch script
            siop_target.lunsw.set(siop_get_lunsw(sc));
            if siop_target.lunsw.get().is_none() {
                printf(format_args!(
                    "{xname}: can't alloc lunsw for target {target}\n"
                ));
                siop_free(p);
                return Err(Errno::ENOMEM);
            }
            for l in &siop_target.siop_lun {
                l.set(None);
            }

            sc.sc_c.targets[target].set(Some(NonNull::from(&siop_target.target_c)));

            siop_add_reselsw(sc, target);
            siop_target
        }
    };

    if siop_target.siop_lun[lun].get().is_none() {
        let Some(l) = siop_malloc::<SiopLun>(M_WAITOK | M_CANFAIL) else {
            printf(format_args!(
                "{xname}: can't alloc siop_lun for target {target} lun {lun}\n"
            ));
            return Err(Errno::ENOMEM);
        };
        siop_target.siop_lun[lun].set(Some(l));
    }

    Ok(())
}

/// `siop_scsicmd`: the adapter's `scsi_cmd`: sets the command up, queues it and starts
/// the chip; with `SCSI_POLL`, polls the interrupt handler until the command is done.
pub fn siop_scsicmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = siop_link_softc(link);
    let target = usize::from(link.target.get());
    let lun = usize::from(link.lun.get());
    let c = &sc.sc_c;

    let siop_target = siop_target_of(sc, target);
    let Some(io) = xs.io.get() else {
        panic(format_args!("siop_scsicmd: xs {:p} without a command", xs));
    };
    // SAFETY: the transfer's opening came from this adapter's pool, whose io_get
    // (siop_cmd_get) hands out commands of the blocks, never freed.
    let siop_cmd: &'static SiopCmd = unsafe { io.cast::<SiopCmd>().as_ref() };

    // The xs may have been restarted by the scsi layer, so ensure the ccb starts in the
    // proper state.
    siop_cmd.cmd_c.status.set(CMDST_READY);

    // Always reset xs->stimeout, lest we timeout_del() with trash
    timeout_set(
        &xs.stimeout,
        siop_timeout,
        ptr::from_ref(siop_cmd).cast_mut().cast(),
    );

    siop_cmd.cmd_c.siop_target.set(c.targets[target].get());
    siop_cmd.cmd_c.xs.set(Some(xs));
    siop_cmd.cmd_c.flags.set(0);

    let tables = siop_cmd.tables();
    let cmdlen = (xs.cmdlen.get().max(0) as usize).min(size_of::<ScsiGeneric>());
    let mut xscmd = ScsiGeneric::default();
    xscmd.as_bytes_mut()[..cmdlen].copy_from_slice(&xs.cmd.get().as_bytes()[..cmdlen]);
    dma_set(&tables.xscmd, xscmd);
    SiopCommonXfer::set_count(&tables.cmd, siop_htoc32(c, xs.cmdlen.get() as u32));

    // load the DMA maps
    if xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) != 0 {
        let dmat = c.dmat();
        let map = siop_cmd.cmd_c.dmamap();
        let rw = if xs.flags.get() & SCSI_DATA_IN != 0 {
            BUS_DMA_READ
        } else {
            BUS_DMA_WRITE
        };
        // SAFETY: `set_data`'s contract: `xs.data()` is valid for `datalen` bytes and
        // reserved for this transfer until it completes, which unloads the map
        // (siop_scsicmd_end).
        if let Err(error) = unsafe {
            bus_dmamap_load(
                dmat,
                map,
                xs.data(),
                xs.datalen().max(0) as usize,
                None,
                BUS_DMA_NOWAIT | BUS_DMA_STREAMING | rw,
            )
        } {
            printf(format_args!(
                "{}: unable to load data DMA map: {}\n",
                c.sc_dev.xname(),
                error as i32
            ));
            xs.error.set(XS_DRIVER_STUFFUP);
            scsi_done(xs);
            return;
        }
        bus_dmamap_sync(
            dmat,
            map,
            0,
            map.dm_mapsize.get(),
            if xs.flags.get() & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_PREREAD
            } else {
                BUS_DMASYNC_PREWRITE
            },
        );
    }

    siop_setuptables(&siop_cmd.cmd_c);
    siop_cmd.saved_offset.set(SIOP_NOOFFSET);
    siop_table_sync(siop_cmd, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

    // Negotiate transfer parameters on first non-polling command.
    if xs.flags.get() & SCSI_POLL == 0 && siop_target.target_c.status.get() == TARST_PROBING {
        siop_target.target_c.status.set(TARST_ASYNC);
    }

    let s = splbio();
    // SAFETY: a command from the pool is in no list (siop_cmd_get took it off the free
    // list) and stays in place.
    unsafe { sc.ready_list.insert_tail(siop_cmd) };
    siop_start(sc);
    if xs.flags.get() & SCSI_POLL == 0 {
        splx(s);
        return;
    }

    // Poll for command completion.
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();
    let mut i = xs.timeout.get();
    while i > 0 {
        siop_intr(arg);
        if xs.flags.get() & ITSDONE == 0 {
            delay(1000);
            i -= 1;
            continue;
        }
        if xs.cmd.get().opcode == INQUIRY && xs.error.get() == XS_NOERROR {
            // SAFETY: the polling issuer waits inside this call for the transfer, so its
            // data (the inquiry buffer) is not touched by anyone else meanwhile.
            let device = unsafe { xs.data_slice() }.first().copied().unwrap_or(0);
            if device & SID_QUAL == SID_QUAL_BAD_LU {
                break;
            }
            // Allocate cbd's to hold maximum openings worth of commands. Do this now
            // because doing it dynamically in siop_startcmd may cause calls to bus_dma*
            // functions in interrupt context.
            for _ in (0..SIOP_NTAG).step_by(SIOP_NCMDPB) {
                siop_morecbd(sc);
            }

            // Set TARF_DT here because if it is turned off during PPR, it must STAY off!
            if lun == 0 && c.features.get() & SF_BUS_ULTRA3 != 0 {
                let t = c.target(target);
                t.flags.set(t.flags.get() | TARF_DT);
            }
            // Can't do lun 0 here, because flags are not set yet. But have to do other
            // lun's here because they never go through TARST_ASYNC.
            if lun > 0 {
                siop_add_dev(sc, target, lun);
            }
        }
        break;
    }
    if i == 0 {
        siop_timeout(ptr::from_ref(siop_cmd).cast_mut().cast());
        while xs.flags.get() & ITSDONE == 0 {
            siop_intr(arg);
        }
    }

    splx(s);
}

/// `siop_start`: puts the urgent, then the ready commands into free scheduler slots and
/// signals the script.
pub fn siop_start(sc: &SiopSoftc) {
    let c = &sc.sc_c;
    let mut newcmd = false;
    let slot0 = Ent_script_sched_slot0 / 4;

    // first make sure to read valid data
    siop_script_sync(sc, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);

    // The queue management here is a bit tricky: the script always looks at the slot from
    // first to last, so if we always use the first free slot commands can stay at the tail
    // of the queue ~forever. The algorithm used here is to restart from the head when we
    // know that the queue is empty, and only add commands after the last one. When we're
    // at the end of the queue wait for the script to clear it. The best thing to do here
    // would be to implement a circular queue, but using only 53c720 features this can be
    // "interesting". A mid-way solution could be to implement 2 queues and swap orders.
    let mut slot = sc.sc_currschedslot.get() as u32;
    // If the instruction is 0x80000000 (JUMP foo, IF FALSE) the slot is free. As this is
    // the last used slot, all previous slots are free, we can restart from 1. slot 0 is
    // reserved for request sense commands.
    if siop_script_read(sc, slot0 + slot * 2) == 0x8000_0000 {
        slot = 1;
        sc.sc_currschedslot.set(1);
    } else {
        slot += 1;
    }
    'end: {
        // first handle commands from the urgent list, then the ready list
        for doingready in [false, true] {
            let list = if doingready {
                &sc.ready_list
            } else {
                &sc.urgent_list
            };
            let mut next = list.first();
            while let Some(siop_cmd) = next {
                // SAFETY: commands live in their blocks forever.
                let siop_cmd: &'static SiopCmd = unsafe { &*ptr::from_ref(siop_cmd) };
                next = TailqHead::<CmdList>::next(siop_cmd);
                #[cfg(feature = "diagnostic")]
                {
                    let status = siop_cmd.cmd_c.status.get();
                    if status != CMDST_READY && status != CMDST_SENSE {
                        panic(format_args!("siop: non-ready cmd in ready list"));
                    }
                }
                let xs = siop_cmd.cmd_c.xs();
                let link = xs.link();
                let target = usize::from(link.target.get());
                let lun = usize::from(link.lun.get());
                let siop_lun = siop_lun_of(sc, target, lun);
                // if non-tagged command active, wait
                if siop_lun.siop_tag[0].active.get().is_some() {
                    continue;
                }
                // if we're in a queue full condition don't start a new command, unless it's
                // a request sense
                if siop_lun.lun_flags.get() & SIOP_LUNF_FULL != 0
                    && siop_cmd.cmd_c.status.get() == CMDST_READY
                {
                    continue;
                }
                // find a free tag if needed
                let tag = if siop_cmd.cmd_c.flags.get() & CMDFL_TAG != 0 {
                    match (1..SIOP_NTAG).find(|&t| siop_lun.siop_tag[t].active.get().is_none()) {
                        Some(t) => t,
                        None => continue, // no free tag
                    }
                } else {
                    0
                };
                siop_cmd.cmd_c.tag.set(tag as i32);
                // find a free scheduler slot and load it. If it's a request sense we need to
                // use slot 0.
                if siop_cmd.cmd_c.status.get() != CMDST_SENSE {
                    while slot < SIOP_NSLOTS {
                        // If cmd if 0x80000000 the slot is free
                        if siop_script_read(sc, slot0 + slot * 2) == 0x8000_0000 {
                            break;
                        }
                        slot += 1;
                    }
                    // no more free slots, no need to continue
                    if slot == SIOP_NSLOTS {
                        break 'end;
                    }
                } else {
                    slot = 0;
                    if siop_script_read(sc, slot0) != 0x8000_0000 {
                        break 'end;
                    }
                }

                let tables = siop_cmd.tables();
                // Ok, we can add the tag message
                if tag > 0 {
                    #[cfg(feature = "diagnostic")]
                    {
                        let msgcount = siop_ctoh32(c, dma_get(&tables.t_msgout).count);
                        if msgcount != 1 {
                            printf(format_args!(
                                "{}:{target}:{lun}: tag {tag} with msgcount {msgcount}\n",
                                c.sc_dev.xname()
                            ));
                        }
                    }
                    tables.set_msg_out(1, MSG_SIMPLE_Q_TAG);
                    tables.set_msg_out(2, tag as u8);
                    SiopCommonXfer::set_count(&tables.t_msgout, siop_htoc32(c, 3));
                }
                // note that we started a new command
                newcmd = true;
                // mark command as active
                match siop_cmd.cmd_c.status.get() {
                    CMDST_READY => siop_cmd.cmd_c.status.set(CMDST_ACTIVE),
                    CMDST_SENSE => siop_cmd.cmd_c.status.set(CMDST_SENSE_ACTIVE),
                    _ => panic(format_args!("siop_start: bad status")),
                }
                // SAFETY: the command is in the list being walked; `next` was taken first.
                unsafe { list.remove(siop_cmd) };
                siop_lun.siop_tag[tag].active.set(Some(siop_cmd));
                // patch scripts with DSA addr
                let dsa = siop_cmd.cmd_c.dsa.get() as u32;
                // first reselect switch, if we have an entry
                if siop_lun.siop_tag[tag].reseloff.get() > 0 {
                    siop_script_write(
                        sc,
                        siop_lun.siop_tag[tag].reseloff.get() + 1,
                        dsa.wrapping_add(XFER_TABLES)
                            .wrapping_add(Ent_ldsa_reload_dsa),
                    );
                }
                // CMD script: MOVE MEMORY addr
                let siop_xfer = siop_cmd.xfer();
                dma_set(
                    &siop_xfer.resel[E_ldsa_abs_slot_Used[0] as usize],
                    siop_htoc32(
                        c,
                        siop_scriptaddr(sc)
                            .wrapping_add(Ent_script_sched_slot0)
                            .wrapping_add(slot * 8),
                    ),
                );
                siop_table_sync(siop_cmd, BUS_DMASYNC_PREWRITE);
                // scheduler slot: JUMP ldsa_select
                siop_script_write(
                    sc,
                    slot0 + slot * 2 + 1,
                    dsa.wrapping_add(XFER_TABLES).wrapping_add(Ent_ldsa_select),
                );
                // handle timeout
                if siop_cmd.cmd_c.status.get() == CMDST_ACTIVE && xs.flags.get() & SCSI_POLL == 0 {
                    // start expire timer
                    timeout_add_msec(&xs.stimeout, xs.timeout.get().max(0) as u64);
                }
                // Change JUMP cmd so that this slot will be handled
                siop_script_write(sc, slot0 + slot * 2, 0x8008_0000);
                // if we're using the request sense slot, stop here
                if slot == 0 {
                    break 'end;
                }
                sc.sc_currschedslot.set(slot as i32);
                slot += 1;
            }
        }
    }

    // end:
    // if nothing changed no need to flush cache and wakeup script
    if !newcmd {
        return;
    }
    // make sure SCRIPT processor will read valid data
    siop_script_sync(sc, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    // Signal script it has some work to do
    c.write_1(SIOP_ISTAT, ISTAT_SIGP);
    // and wait for IRQ
}

/// `siop_timeout`: a command's timeout: resets the bus and fails every active command.
pub fn siop_timeout(v: *mut c_void) {
    // SAFETY: siop_scsicmd sets the timeout with its command, which lives in its block
    // forever.
    let siop_cmd: &'static SiopCmd = unsafe { &*v.cast::<SiopCmd>().cast_const() };
    let sc = siop_softc(siop_cmd.cmd_c.sc());
    let xs = siop_cmd.cmd_c.xs();

    // deactivate callout
    timeout_del(&xs.stimeout);

    sc_print_addr(xs.link());
    printf(format_args!(
        "timeout on SCSI command {:#x}\n",
        xs.cmd.get().opcode
    ));

    let s = splbio();
    // reset the scsi bus
    siop_resetbus(&sc.sc_c);
    siop_cmd
        .cmd_c
        .flags
        .set(siop_cmd.cmd_c.flags.get() | CMDFL_TIMEOUT);
    siop_handle_reset(sc);
    splx(s);
}

/// `siop_morecbd`: allocates a block of `SIOP_NCMDPB` commands with their tables, sense
/// buffers and data maps, and puts them on the free list.
pub fn siop_morecbd(sc: &SiopSoftc) {
    let c = &sc.sc_c;
    let xname = c.sc_dev.xname();
    let dmat = c.dmat();
    let sense_size = roundup(size_of::<ScsiSenseData>(), 16);

    // allocate a new list head
    let Some(newcbd_p) = siop_malloc::<SiopCbd>(M_NOWAIT) else {
        printf(format_args!(
            "{xname}: can't allocate memory for command descriptors head\n"
        ));
        return;
    };
    // SAFETY: a fresh zeroed allocation (`SiopZeroed`); linked into `cmds` below and never
    // freed afterwards.
    let newcbd: &'static SiopCbd = unsafe { &*newcbd_p.as_ptr() };

    // allocate cmd list
    let Some(cmds) = mallocarray(
        SIOP_NCMDPB,
        size_of::<SiopCmd>(),
        M_DEVBUF,
        M_NOWAIT | M_ZERO,
    ) else {
        printf(format_args!(
            "{xname}: can't allocate memory for command descriptors\n"
        ));
        // bad3:
        siop_free(newcbd_p);
        return;
    };
    let cmds = cmds.cast::<SiopCmd>();
    newcbd.cmds.set(cmds.as_ptr());
    let free_cmds = || {
        free(cmds.cast(), M_DEVBUF, SIOP_NCMDPB * size_of::<SiopCmd>());
        siop_free(newcbd_p);
    };

    let Some(xfers) = siop_dmamem_alloc(sc, PAGE_SIZE) else {
        printf(format_args!(
            "{xname}: unable to allocate cbd xfer DMA memory\n"
        ));
        // bad2:
        free_cmds();
        return;
    };
    newcbd.xfers.set(Some(xfers));

    let Some(sense) = siop_dmamem_alloc(sc, sense_size * SIOP_NCMDPB) else {
        printf(format_args!(
            "{xname}: unable to allocate cbd sense DMA memory\n"
        ));
        // bad1:
        // SAFETY: the area allocated above, unused.
        unsafe { siop_dmamem_free(sc, xfers) };
        free_cmds();
        return;
    };
    newcbd.sense.set(Some(sense));

    let cmd_at = |i: usize| -> &'static SiopCmd {
        // SAFETY: `i` is below SIOP_NCMDPB, the zeroed (`SiopZeroed`) commands allocated
        // above, which live as long as the block.
        unsafe { &*cmds.as_ptr().add(i) }
    };

    for i in 0..SIOP_NCMDPB {
        match bus_dmamap_create(
            dmat,
            MAXPHYS,
            SIOP_NSG as i32,
            MAXPHYS,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) {
            Ok(map) => cmd_at(i).cmd_c.dmamap_data.set(Some(map)),
            Err(error) => {
                printf(format_args!(
                    "{xname}: unable to create data DMA map for cbd: error {}\n",
                    error as i32
                ));
                // bad0:
                for j in (0..i).rev() {
                    if let Some(map) = cmd_at(j).cmd_c.dmamap_data.take() {
                        // SAFETY: a map created above, never loaded.
                        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
                    }
                }
                // SAFETY: the areas allocated above, unused.
                unsafe {
                    siop_dmamem_free(sc, sense);
                    siop_dmamem_free(sc, xfers);
                }
                free_cmds();
                return;
            }
        }
    }

    // Use two loops since bailing out above releases allocated memory
    let off: u32 = if c.features.get() & SF_CHIP_BE != 0 {
        3
    } else {
        0
    };
    let xfers_kva = xfers.kva().cast::<SiopXfer>();
    for i in 0..SIOP_NCMDPB {
        let cmd = cmd_at(i);
        cmd.cmd_c.siop_sc.set(Some(siop_common_static(sc)));
        cmd.siop_cbdp.set(Some(newcbd));
        // SAFETY: `i` is below SIOP_NCMDPB, so the xfer is inside the page of `xfers`
        // (`SIOP_NCMDPB * sizeof(struct siop_xfer)` bytes at most); zeroing it whole before
        // the reference is made, then the area lives (mapped) forever.
        let xfer: &'static SiopXfer = unsafe {
            let p = xfers_kva.as_ptr().add(i);
            ptr::write_bytes(p, 0, 1);
            &*p
        };
        cmd.cmd_c.siop_tables.set(Some(&xfer.siop_tables));
        let dsa = xfers.dva() + i * size_of::<SiopXfer>();
        cmd.cmd_c.dsa.set(dsa);
        let dsa = dsa as u32;
        cmd.cmd_c.status.set(CMDST_FREE);
        // SAFETY: `i * sense_size` is inside the sense area (`sense_size * SIOP_NCMDPB`
        // bytes), mapped forever.
        cmd.cmd_c
            .sense
            .set(unsafe { sense.kva().as_ptr().add(i * sense_size) }.cast());
        let t = &xfer.siop_tables;
        let msg_in = core::mem::offset_of!(SiopCommonXfer, msg_in) as u32;
        let status = core::mem::offset_of!(SiopCommonXfer, status) as u32;
        let xscmd = core::mem::offset_of!(SiopCommonXfer, xscmd) as u32;
        let table = |count: u32, addr: u32| ScrTable {
            count: siop_htoc32(c, count),
            addr: siop_htoc32(c, addr),
        };
        dma_set(&t.t_msgout, table(1, dsa));
        dma_set(&t.t_msgin, table(1, dsa + msg_in));
        dma_set(&t.t_extmsgin, table(2, dsa + msg_in + 1));
        dma_set(&t.t_extmsgdata, table(0, dsa + msg_in + 3));
        dma_set(&t.t_status, table(1, dsa + status + off));
        dma_set(&t.cmd, table(0, dsa + xscmd));
        // The select/reselect script
        let scr = &xfer.resel;
        for (j, &w) in load_dsa.iter().enumerate() {
            dma_set(&scr[j], siop_htoc32(c, w));
        }
        // 0x78000000 is a 'move data8 to reg'. data8 is the second octet, reg offset is the
        // third.
        dma_set(
            &scr[(Ent_rdsa0 / 4) as usize],
            siop_htoc32(c, 0x7810_0000 | ((dsa & 0x0000_00ff) << 8)),
        );
        dma_set(
            &scr[(Ent_rdsa1 / 4) as usize],
            siop_htoc32(c, 0x7811_0000 | (dsa & 0x0000_ff00)),
        );
        dma_set(
            &scr[(Ent_rdsa2 / 4) as usize],
            siop_htoc32(c, 0x7812_0000 | ((dsa & 0x00ff_0000) >> 8)),
        );
        dma_set(
            &scr[(Ent_rdsa3 / 4) as usize],
            siop_htoc32(c, 0x7813_0000 | ((dsa & 0xff00_0000) >> 16)),
        );
        let scriptaddr = siop_scriptaddr(sc);
        dma_set(
            &scr[E_ldsa_abs_reselected_Used[0] as usize],
            siop_htoc32(c, scriptaddr.wrapping_add(Ent_reselected)),
        );
        dma_set(
            &scr[E_ldsa_abs_reselect_Used[0] as usize],
            siop_htoc32(c, scriptaddr.wrapping_add(Ent_reselect)),
        );
        dma_set(
            &scr[E_ldsa_abs_selected_Used[0] as usize],
            siop_htoc32(c, scriptaddr.wrapping_add(Ent_selected)),
        );
        dma_set(
            &scr[E_ldsa_abs_data_Used[0] as usize],
            siop_htoc32(c, dsa.wrapping_add(XFER_TABLES).wrapping_add(Ent_ldsa_data)),
        );
        // JUMP foo, IF FALSE - used by MOVE MEMORY to clear the slot
        dma_set(
            &scr[(Ent_ldsa_data / 4) as usize],
            siop_htoc32(c, 0x8000_0000),
        );
        let s = splbio();
        // SAFETY: a new command, in no list, in a block that is never freed.
        unsafe { sc.free_list.insert_tail(cmd) };
        splx(s);
    }
    let s = splbio();
    // SAFETY: a new block, in no list, never freed.
    unsafe { sc.cmds.insert_tail(newcbd) };
    splx(s);
}

/// `&sc->sc_c` with the softc's lifetime.
fn siop_common_static(sc: &SiopSoftc) -> &'static SiopCommonSoftc {
    // SAFETY: every `SiopSoftc` is the head of a siop device's softc, which is never freed
    // while the kernel runs (softcs live as long as their device).
    unsafe { &*ptr::from_ref(&sc.sc_c) }
}

/// `siop_get_lunsw`: a lun switch script in the script memory, from the free list or
/// newly copied at `script_free_lo`; `None` when the memory or `malloc` runs out.
pub fn siop_get_lunsw(sc: &SiopSoftc) -> Option<NonNull<SiopLunsw>> {
    let c = &sc.sc_c;

    if sc.script_free_lo.get() + lun_switch.len() as u32 >= sc.script_free_hi.get() {
        return None;
    }
    if let Some(lunsw) = sc.lunsw_list.first() {
        // SAFETY: the first element of the list.
        unsafe { sc.lunsw_list.remove(lunsw) };
        return Some(NonNull::from(lunsw));
    }
    let p = siop_malloc::<SiopLunsw>(M_NOWAIT)?;
    // SAFETY: a fresh zeroed allocation (`SiopZeroed`).
    let lunsw = unsafe { &*p.as_ptr() };
    let free_lo = sc.script_free_lo.get();
    let lunsw_return = siop_scriptaddr(sc).wrapping_add(Ent_lunsw_return);
    if c.features.get() & SF_CHIP_RAM != 0 {
        let (t, h) = c.ram();
        bus_space_write_region_4(t, h, free_lo as usize * 4, &lun_switch);
        bus_space_write_4(
            t,
            h,
            (free_lo + E_abs_lunsw_return_Used[0]) as usize * 4,
            lunsw_return,
        );
    } else {
        for (i, &w) in lun_switch.iter().enumerate() {
            siop_script_store(sc, free_lo as usize + i, w);
        }
        siop_script_store(
            sc,
            (free_lo + E_abs_lunsw_return_Used[0]) as usize,
            lunsw_return,
        );
    }
    lunsw.lunsw_off.set(free_lo);
    lunsw.lunsw_size.set(lun_switch.len() as u32);
    sc.script_free_lo.set(free_lo + lunsw.lunsw_size.get());
    siop_script_sync(sc, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    Some(p)
}

/// `siop_add_reselsw`: adds the target's entry to the reselect switch.
pub fn siop_add_reselsw(sc: &SiopSoftc, target: usize) {
    let siop_target = siop_target_of(sc, target);

    // add an entry to resel switch
    siop_script_sync(sc, BUS_DMASYNC_POSTWRITE);
    let mut found = false;
    for i in 0..15 {
        siop_target.reseloff.set(Ent_resel_targ0 / 4 + i * 2);
        if siop_script_read(sc, siop_target.reseloff.get()) & 0xff == 0xff {
            // it's free
            // JUMP abs_foo, IF target | 0x80;
            siop_script_write(sc, siop_target.reseloff.get(), 0x800c_0080 | target as u32);
            siop_script_write(
                sc,
                siop_target.reseloff.get() + 1,
                siop_scriptaddr(sc)
                    .wrapping_add(siop_target.lunsw().lunsw_off.get() * 4)
                    .wrapping_add(Ent_lun_switch_entry),
            );
            found = true;
            break;
        }
    }
    if !found {
        // no free slot, shouldn't happen
        panic(format_args!("siop: resel switch full"));
    }

    sc.sc_ntargets.set(sc.sc_ntargets.get() + 1);
    for i in 0..8 {
        let Some(siop_lun) = siop_target.lun(i) else {
            continue;
        };
        if siop_lun.reseloff.get() > 0 {
            siop_lun.reseloff.set(0);
            for t in &siop_lun.siop_tag {
                t.reseloff.set(0);
            }
            siop_add_dev(sc, target, i);
        }
    }
    siop_update_scntl3(sc, &siop_target.target_c);
    siop_script_sync(sc, BUS_DMASYNC_PREWRITE);
}

/// `siop_update_scntl3`: patches the target's lun switch with its `SCNTL3` and `SXFER`.
pub fn siop_update_scntl3(sc: &SiopSoftc, siop_target: &SiopCommonTarget) {
    // SAFETY: the target of a siop adapter (a command's or `targets[]`'s).
    let siop_target = unsafe { SiopTarget::from_common(siop_target) };
    let off = siop_target.lunsw().lunsw_off.get() + Ent_restore_scntl3 / 4;
    let id = siop_target.target_c.id.get();
    // MOVE target->id >> 24 TO SCNTL3
    siop_script_write(sc, off, 0x7803_0000 | ((id >> 16) & 0x0000_ff00));
    // MOVE target->id >> 8 TO SXFER
    siop_script_write(sc, off + 2, 0x7805_0000 | (id & 0x0000_ff00));
    siop_script_sync(sc, BUS_DMASYNC_PREWRITE);
}

/// `siop_add_dev`: adds the lun's entry to its target's lun switch, and a tag switch for a
/// tagged target, when the script memory allows.
pub fn siop_add_dev(sc: &SiopSoftc, target: usize, lun: usize) {
    let c = &sc.sc_c;
    let siop_target = siop_target_of(sc, target);
    let siop_lun = siop_lun_of(sc, target, lun);

    if siop_lun.reseloff.get() > 0 {
        return;
    }
    let lunsw = siop_target.lunsw();
    if lunsw.lunsw_off.get() + lunsw.lunsw_size.get() < sc.script_free_lo.get() {
        // can't extend this slot. Probably not worth trying to deal with this case
        return;
    }
    // count how many free targets we still have to probe
    let buswidth: i32 = if c.features.get() & SF_BUS_WIDE != 0 {
        16
    } else {
        8
    };
    let ntargets = (buswidth - 1) - 1 - sc.sc_ntargets.get();

    // we need 8 bytes for the lun sw additional entry, and eventually sizeof(tag_switch)
    // for the tag switch entry. Keep enough free space for the free targets that could be
    // probed later. (The C computes in size_t: a negative count wraps.)
    let need = u64::from(sc.script_free_lo.get())
        + 2
        + (ntargets as i64 as u64).wrapping_mul(size_of_val(&lun_switch) as u64)
            / size_of::<u32>() as u64;
    let limit = if siop_target.target_c.flags.get() & TARF_TAG != 0 {
        u64::from(sc.script_free_hi.get()).wrapping_sub(tag_switch.len() as u64)
    } else {
        u64::from(sc.script_free_hi.get())
    };
    if need >= limit {
        // not enough space, probably not worth dealing with it. We can hold 13
        // tagged-queuing capable devices in the 4k RAM.
        return;
    }
    let free_lo = sc.script_free_lo.get();
    // INT int_resellun
    siop_script_write(sc, free_lo, 0x9808_0000);
    siop_script_write(sc, free_lo + 1, A_int_resellun);
    // Now the slot entry: JUMP abs_foo, IF lun
    siop_script_write(sc, free_lo - 2, 0x800c_0000 | lun as u32);
    siop_script_write(sc, free_lo - 1, 0);
    siop_lun.reseloff.set(free_lo - 2);
    lunsw.lunsw_size.set(lunsw.lunsw_size.get() + 2);
    sc.script_free_lo.set(free_lo + 2);
    if siop_target.target_c.flags.get() & TARF_TAG != 0 {
        // we need a tag switch
        sc.script_free_hi
            .set(sc.script_free_hi.get() - tag_switch.len() as u32);
        let free_hi = sc.script_free_hi.get();
        if c.features.get() & SF_CHIP_RAM != 0 {
            let (t, h) = c.ram();
            bus_space_write_region_4(t, h, free_hi as usize * 4, &tag_switch);
        } else {
            for (i, &w) in tag_switch.iter().enumerate() {
                siop_script_store(sc, free_hi as usize + i, w);
            }
        }
        siop_script_write(
            sc,
            siop_lun.reseloff.get() + 1,
            siop_scriptaddr(sc)
                .wrapping_add(free_hi * 4)
                .wrapping_add(Ent_tag_switch_entry),
        );

        for (i, t) in siop_lun.siop_tag.iter().enumerate() {
            t.reseloff
                .set(free_hi + (Ent_resel_tag0 / 4) + i as u32 * 2);
        }
    } else {
        // non-tag case; just work with the lun switch
        siop_lun.siop_tag[0].reseloff.set(siop_lun.reseloff.get());
    }
    siop_script_sync(sc, BUS_DMASYNC_PREWRITE);
}

/// `siop_scsifree`: the adapter's `dev_free`: frees the lun's state, and the target's when
/// it was its last lun.
pub fn siop_scsifree(link: &'static ScsiLink) {
    let sc = siop_link_softc(link);
    let target = usize::from(link.target.get());
    let lun = usize::from(link.lun.get());

    let siop_target = siop_target_of(sc, target);
    if let Some(l) = siop_target.siop_lun[lun].take() {
        siop_free(l);
    }
    // XXX compact sw entry too ?
    // check if we can free the whole target
    if siop_target.siop_lun.iter().any(|l| l.get().is_some()) {
        return;
    }
    // nothing here, free the target struct and resel switch entry
    siop_script_write(sc, siop_target.reseloff.get(), 0x800c_00ff);
    siop_script_sync(sc, BUS_DMASYNC_PREWRITE);
    if let Some(lunsw) = siop_target.lunsw.get() {
        // SAFETY: the target's lun switch, in no list while the target lives, stays in
        // place until siop_reset frees it.
        unsafe { sc.lunsw_list.insert_tail(lunsw.as_ref()) };
    }
    if let Some(t) = sc.sc_c.targets[target].take() {
        siop_free(t.cast::<SiopTarget>());
    }
    sc.sc_ntargets.set(sc.sc_ntargets.get() - 1);
}

/// `siop_dmamem_alloc`: `size` bytes of zeroed DMA memory, mapped and loaded.
pub fn siop_dmamem_alloc(sc: &SiopSoftc, size: usize) -> Option<&'static SiopDmamem> {
    let dmat = sc.sc_c.dmat();

    let mem = malloc(size_of::<SiopDmamem>(), M_DEVBUF, M_NOWAIT | M_ZERO)?;
    let sdmfree = |mem: NonNull<u8>| free(mem, M_DEVBUF, size_of::<SiopDmamem>());

    let map = match bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW) {
        Ok(map) => map,
        Err(_) => {
            sdmfree(mem);
            return None;
        }
    };
    let destroy = |mem: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        sdmfree(mem);
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let nsegs = match bus_dmamem_alloc(
        dmat,
        size,
        PAGE_SIZE,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) {
        Ok(n) => n,
        Err(_) => {
            destroy(mem);
            return None;
        }
    };

    let kva = match bus_dmamem_map(
        dmat,
        &mut segs[..nsegs],
        size,
        BUS_DMA_NOWAIT | BUS_DMA_COHERENT,
    ) {
        Ok(kva) => kva,
        Err(_) => {
            // free:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
            destroy(mem);
            return None;
        }
    };

    // SAFETY: `kva` maps `size` bytes allocated above, which stay until siop_dmamem_free
    // unloads the map first.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_NOWAIT) }.is_err() {
        // unmap:
        // SAFETY: the mapping and the segment made above, unused.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &segs[..nsegs]);
        }
        destroy(mem);
        return None;
    }

    let sdm = mem.cast::<SiopDmamem>();
    // SAFETY: a fresh allocation of `size_of::<SiopDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until siop_dmamem_free.
    unsafe {
        sdm.as_ptr().write(SiopDmamem {
            sdm_map: map,
            sdm_seg: segs[0],
            sdm_size: size,
            sdm_kva: kva,
        });
        Some(&*sdm.as_ptr())
    }
}

/// `siop_dmamem_free`.
///
/// # Safety
///
/// `sdm` came from [`siop_dmamem_alloc`], the chip no longer uses it and nothing references
/// it afterwards.
pub unsafe fn siop_dmamem_free(sc: &SiopSoftc, sdm: &'static SiopDmamem) {
    let dmat = sc.sc_c.dmat();

    bus_dmamap_unload(dmat, sdm.sdm_map);
    // SAFETY: the caller's guarantee: the area's own mapping, segment and map, unloaded.
    unsafe {
        bus_dmamem_unmap(dmat, sdm.sdm_kva, sdm.sdm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&sdm.sdm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(sdm.sdm_map));
    }
    free(NonNull::from(sdm).cast(), M_DEVBUF, size_of::<SiopDmamem>());
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `siop.rs`: the script memory management (the script copied and patched by
    // `siop_reset`, a target's lun switch, its reselect entry and SCNTL3/SXFER restore, the lun
    // entries with and without a tag switch) on an adapter whose script lives in a page of host
    // memory, as without on-board RAM; the host's `bus_space` reads 0 and drops writes.

    use std::alloc::{Layout, alloc_zeroed};
    use std::assert_eq;
    use std::boxed::Box;

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::bus::{BusDmamap, BusSpaceTag, bus_space_map};

    /// The script's bus address in these tests.
    const SCRIPTADDR: u32 = 0x1000_0000;

    /// A zeroed `T` (an `M_ZERO` allocation), leaked.
    ///
    /// # Safety
    ///
    /// All-zero must be a valid `T`.
    unsafe fn leak_zeroed<T>() -> &'static T {
        // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
        unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
    }

    /// The PCI front-end's reset, which has nothing to do here.
    fn no_pci_reset(_sc: &SiopCommonSoftc) {}

    /// A wide adapter whose script is a page of host memory at bus address `SCRIPTADDR`, reset
    /// by `siop_reset`.
    fn adapter() -> &'static SiopSoftc {
        // SAFETY: the softc is `Cell`s, queue heads, the iopool and a `Device`, valid as zero
        // bytes; the host's map is `Cell`s of integers and segments.
        let (sc, map) = unsafe { (leak_zeroed::<SiopSoftc>(), leak_zeroed::<BusDmamap>()) };
        let c = &sc.sc_c;
        c.features.set(SF_BUS_WIDE);
        c.ram_size.set(PAGE_SIZE as i32);
        c.clock_div.set(3);
        let page: &'static mut [u32] = Box::leak(std::vec![0u32; PAGE_SIZE / 4].into_boxed_slice());
        c.sc_script.set(page.as_mut_ptr());
        c.sc_scriptaddr.set(SCRIPTADDR as usize);
        c.sc_scriptdma.set(Some(map));
        c.sc_dmat.set(Some(Default::default()));
        c.sc_reset.set(Some(no_pci_reset));
        c.sc_rt.set(Some(BusSpaceTag::default()));
        // SAFETY: the host's bus space is a test double; nothing is mapped.
        c.sc_rh.set(Some(
            unsafe { bus_space_map(BusSpaceTag::default(), 0, 0x100, 0) }.unwrap(),
        ));
        siop_reset(sc);
        sc
    }

    #[test]
    fn reset_copies_and_patches_the_script() {
        let sc = adapter();
        assert_eq!(SIOP_NCMDPB, PAGE_SIZE / 384);
        for (i, &w) in siop_script.iter().enumerate() {
            if !E_abs_msgin_Used.contains(&(i as u32)) {
                assert_eq!(siop_script_read(sc, i as u32), w, "word {i}");
            }
        }
        for &j in &E_abs_msgin_Used {
            assert_eq!(siop_script_read(sc, j), SCRIPTADDR + Ent_msgin_space);
        }
        assert_eq!(sc.script_free_lo.get(), siop_script.len() as u32);
        assert_eq!(sc.script_free_hi.get(), (PAGE_SIZE / 4) as u32);
        assert_eq!(sc.sc_ntargets.get(), 0);
    }

    #[test]
    fn target_and_lun_switches() {
        // siop_get_lunsw malloc(9)s the lun switch.
        let _g = setup_real_memory();
        let sc = adapter();
        let free_lo = sc.script_free_lo.get();

        // siop_scsiprobe's part: a target with its lun switch, in the reselect switch.
        // SAFETY: a target is `Cell`s of integers and `Option`s, valid as zero bytes.
        let target = unsafe { leak_zeroed::<SiopTarget>() };
        target.target_c.id.set((3 << 24) | (2 << 16));
        target.lunsw.set(siop_get_lunsw(sc));
        let lunsw = target.lunsw();
        assert_eq!(lunsw.lunsw_off.get(), free_lo);
        assert_eq!(lunsw.lunsw_size.get(), lun_switch.len() as u32);
        assert_eq!(sc.script_free_lo.get(), free_lo + 12);
        assert_eq!(
            siop_script_read(sc, free_lo + E_abs_lunsw_return_Used[0]),
            SCRIPTADDR + Ent_lunsw_return
        );
        sc.sc_c.targets[2].set(Some(NonNull::from(&target.target_c)));
        siop_add_reselsw(sc, 2);
        let reseloff = Ent_resel_targ0 / 4;
        assert_eq!(target.reseloff.get(), reseloff);
        assert_eq!(siop_script_read(sc, reseloff), 0x800c_0082);
        assert_eq!(
            siop_script_read(sc, reseloff + 1),
            SCRIPTADDR + free_lo * 4 + Ent_lun_switch_entry
        );
        assert_eq!(sc.sc_ntargets.get(), 1);
        // The restore of SCNTL3 (3) and SXFER (0) at the head of the lun switch.
        assert_eq!(siop_script_read(sc, free_lo), 0x7803_0300);
        assert_eq!(siop_script_read(sc, free_lo + 2), 0x7805_0000);

        // lun 0, untagged: its JUMP replaces the switch's trailing INT, which moves down.
        // SAFETY: a lun is `Cell`s, valid as zero bytes.
        let lun0 = unsafe { leak_zeroed::<SiopLun>() };
        target.siop_lun[0].set(Some(NonNull::from(lun0)));
        siop_add_dev(sc, 2, 0);
        let lo = free_lo + 12;
        assert_eq!(lun0.reseloff.get(), lo - 2);
        assert_eq!(lun0.siop_tag[0].reseloff.get(), lo - 2);
        assert_eq!(siop_script_read(sc, lo - 2), 0x800c_0000);
        assert_eq!(siop_script_read(sc, lo), 0x9808_0000);
        assert_eq!(siop_script_read(sc, lo + 1), A_int_resellun);
        assert_eq!(sc.script_free_lo.get(), lo + 2);
        assert_eq!(lunsw.lunsw_size.get(), 14);
        // Twice is harmless.
        siop_add_dev(sc, 2, 0);
        assert_eq!(sc.script_free_lo.get(), lo + 2);

        // lun 1 of a tagged target: a tag switch at the top of the memory.
        target
            .target_c
            .flags
            .set(target.target_c.flags.get() | TARF_TAG);
        // SAFETY: as for lun 0.
        let lun1 = unsafe { leak_zeroed::<SiopLun>() };
        target.siop_lun[1].set(Some(NonNull::from(lun1)));
        siop_add_dev(sc, 2, 1);
        let hi = (PAGE_SIZE / 4) as u32 - tag_switch.len() as u32;
        assert_eq!(sc.script_free_hi.get(), hi);
        assert_eq!(lun1.reseloff.get(), lo);
        assert_eq!(siop_script_read(sc, lo), 0x800c_0001);
        assert_eq!(
            siop_script_read(sc, lo + 1),
            SCRIPTADDR + hi * 4 + Ent_tag_switch_entry
        );
        for (i, t) in lun1.siop_tag.iter().enumerate() {
            assert_eq!(t.reseloff.get(), hi + Ent_resel_tag0 / 4 + i as u32 * 2);
        }
        assert_eq!(siop_script_read(sc, hi + 2), tag_switch[2]);
    }
}
/* </TESTS> */
