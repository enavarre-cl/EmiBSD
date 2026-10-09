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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
//! Routines for SD/MMC memory cards: power-up and identification (OCR, CID, CSD, RCA), the
//! SD (SCR, bus width, switch function) and MMC (EXT_CSD, bus width, timing) setup, and the
//! block reads and writes the SCSI emulation issues.
//!
//! Upstream: sys/dev/sdmmc/sdmmc_mem.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C's 0/1 returns (`sdmmc_mem_enable`'s 1, `sdmmc_decode_csd`, `sdmmc_decode_cid`)
//!   are `Result<(), Errno>` with `EIO` for the 1; the errno returns are `Result`s too.
//!   `sdmmc_mem_send_op_cond` returns the OCR in its `Ok` (the C's `*ocrp`).
//! - `sdmmc_mem_init` evaluates `sdmmc_select_card` and `sdmmc_mem_set_blocklen` and drops
//!   their result: the C stores `error = 1` and overwrites it on the next line (an
//!   unused assignment Rust rejects); the behaviour is the same.
//! - The block reads and writes take the data as a raw pointer and a length and are
//!   `unsafe` (the pointer goes to the controller in `c_data` or a DMA map).
//! - The scratch buffers of `sdmmc_mem_send_scr`, `sdmmc_mem_send_cxd_data` and
//!   `sdmmc_mem_sd_switch` are `malloc(9)`ed, as in C (`M_NOWAIT`, `ENOMEM` on failure).
//! - `switch_group0_functions[]` and `sdmmc_mmc_timings[]` are `static`s; the latter has the
//!   C's six slots (the two it does not name are 0).
//! - Not configured, as in GENERIC: `SDMMC_DEBUG` (`sdmmc_print_cid` is not compiled;
//!   `DPRINTF`s are comments) and `HIBERNATE` (`sdmmc_mem_hibernate_write` is not compiled,
//!   as in nvme.rs and ahci.rs).

use core::ptr::{self, NonNull};

use crate::dev::sdmmc::sdmmc::{
    sdmmc_app_command, sdmmc_delay, sdmmc_function_alloc, sdmmc_function_free, sdmmc_go_idle_state,
    sdmmc_mmc_command, sdmmc_select_card, sdmmc_send_if_cond, sdmmc_set_bus_power,
    sdmmc_set_relative_addr,
};
use crate::dev::sdmmc::sdmmcchip::{
    SDMMC_SDCLK_25MHZ, SDMMC_SIGNAL_VOLTAGE_180, SDMMC_TIMING_HIGHSPEED, SDMMC_TIMING_LEGACY,
    SDMMC_TIMING_MMC_DDR52, SDMMC_TIMING_MMC_HS200, SDMMC_TIMING_UHS_SDR50,
    SDMMC_TIMING_UHS_SDR104, sdmmc_chip_bus_clock, sdmmc_chip_bus_width, sdmmc_chip_execute_tuning,
    sdmmc_chip_host_maxblklen, sdmmc_chip_host_ocr, sdmmc_chip_signal_voltage,
};
use crate::dev::sdmmc::sdmmcreg::{
    EXT_CSD_BUS_WIDTH, EXT_CSD_BUS_WIDTH_1, EXT_CSD_BUS_WIDTH_4, EXT_CSD_BUS_WIDTH_4_DDR,
    EXT_CSD_BUS_WIDTH_8, EXT_CSD_BUS_WIDTH_8_DDR, EXT_CSD_CARD_TYPE, EXT_CSD_CARD_TYPE_F_26M,
    EXT_CSD_CARD_TYPE_F_52M, EXT_CSD_CARD_TYPE_F_DDR52_1_8V, EXT_CSD_CARD_TYPE_F_HS200_1_8V,
    EXT_CSD_CMD_SET_NORMAL, EXT_CSD_HS_TIMING, EXT_CSD_HS_TIMING_HS, EXT_CSD_HS_TIMING_HS200,
    EXT_CSD_SEC_COUNT, MMC_ALL_SEND_CID, MMC_CSD_CSDVER_1_0, MMC_CSD_CSDVER_2_0,
    MMC_CSD_CSDVER_EXT_CSD, MMC_CSD_MMCVER_1_0, MMC_CSD_MMCVER_1_4, MMC_CSD_MMCVER_2_0,
    MMC_CSD_MMCVER_3_1, MMC_CSD_MMCVER_4_0, MMC_OCR_ACCESS_MODE_MASK, MMC_OCR_ACCESS_MODE_SECTOR,
    MMC_OCR_HCS, MMC_OCR_MEM_READY, MMC_OCR_S18A, MMC_R1_READY_FOR_DATA, MMC_READ_BLOCK_MULTIPLE,
    MMC_READ_BLOCK_SINGLE, MMC_SEND_CSD, MMC_SEND_EXT_CSD, MMC_SEND_OP_COND, MMC_SEND_STATUS,
    MMC_SET_BLOCKLEN, MMC_STOP_TRANSMISSION, MMC_SWITCH, MMC_SWITCH_MODE_WRITE_BYTE,
    MMC_WRITE_BLOCK_MULTIPLE, MMC_WRITE_BLOCK_SINGLE, SCR_SD_BUS_WIDTHS_4BIT, SCR_SD_SPEC_VER_1_10,
    SD_ACCESS_MODE_DDR50, SD_ACCESS_MODE_SDR12, SD_ACCESS_MODE_SDR25, SD_ACCESS_MODE_SDR50,
    SD_ACCESS_MODE_SDR104, SD_APP_OP_COND, SD_APP_SEND_SCR, SD_APP_SET_BUS_WIDTH,
    SD_ARG_BUS_WIDTH_1, SD_ARG_BUS_WIDTH_4, SD_CSD_CCC_SWITCH, SD_CSD_CSDVER_1_0,
    SD_CSD_CSDVER_2_0, SD_CSD_V2_BL_LEN, SD_SEND_SWITCH_FUNC, SD_VOLTAGE_SWITCH, mmc_arg_rca,
    mmc_cid_mdt_v1, mmc_cid_mid_v1, mmc_cid_mid_v2, mmc_cid_oid_v2, mmc_cid_pnm_v1_cpy,
    mmc_cid_pnm_v2_cpy, mmc_cid_psn_v1, mmc_cid_psn_v2, mmc_cid_rev_v1, mmc_csd_capacity,
    mmc_csd_csdver, mmc_csd_mmcver, mmc_csd_read_bl_len, mmc_r1, mmc_r3, scr_sd_bus_widths,
    scr_sd_spec, scr_structure, sd_cid_mdt, sd_cid_mid, sd_cid_oid, sd_cid_pnm_cpy, sd_cid_psn,
    sd_cid_rev, sd_csd_capacity, sd_csd_ccc, sd_csd_csdver, sd_csd_read_bl_len, sd_csd_v2_capacity,
    sfunc_status_group,
};
use crate::dev::sdmmc::sdmmcvar::{
    SCF_CMD_AC, SCF_CMD_ADTC, SCF_CMD_BCR, SCF_CMD_READ, SCF_RSP_R1, SCF_RSP_R1B, SCF_RSP_R2,
    SCF_RSP_R3, SFF_ERROR, SFF_SDHC, SMC_CAPS_4BIT_MODE, SMC_CAPS_8BIT_MODE, SMC_CAPS_DMA,
    SMC_CAPS_MMC_DDR52, SMC_CAPS_MMC_HIGHSPEED, SMC_CAPS_MMC_HS200, SMC_CAPS_SD_HIGHSPEED,
    SMC_CAPS_SINGLE_ONLY, SMC_CAPS_UHS_DDR50, SMC_CAPS_UHS_SDR50, SMC_CAPS_UHS_SDR104, SMF_IO_MODE,
    SMF_MEM_MODE, SMF_SD_MODE, SMF_STOP_AFTER_MULTIPLE, SMF_UHS_MODE, SdmmcCommand, SdmmcFunction,
    SdmmcResponse, SdmmcSoftc,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit};
use crate::kern::subr_prf::printf;
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmamap, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamap_unload,
};
use crate::machine::cpu::delay;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};

/// `sdmmc_bitfield512_t`: the 512-bit switch function status, as `__bitfield` reads it.
#[repr(C, align(4))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SdmmcBitfield512 {
    /// `_bits`.
    pub _bits: [u32; 512 / 32],
}

/// One entry of `switch_group0_functions[]`.
pub struct SwitchGroup0Function {
    /// `name`.
    pub name: &'static str,
    /// `v`: the host capability that allows it.
    pub v: u32,
    /// `freq`: the bus clock (kHz).
    pub freq: i32,
}

/// `switch_group0_functions[]`: the access modes of function group 1.
pub static SWITCH_GROUP0_FUNCTIONS: [SwitchGroup0Function; 5] = [
    // Default/SDR12
    SwitchGroup0Function {
        name: "Default/SDR12",
        v: 0,
        freq: 25000,
    },
    // High-Speed/SDR25
    SwitchGroup0Function {
        name: "High-Speed/SDR25",
        v: SMC_CAPS_SD_HIGHSPEED,
        freq: 50000,
    },
    // SDR50
    SwitchGroup0Function {
        name: "SDR50",
        v: SMC_CAPS_UHS_SDR50,
        freq: 100000,
    },
    // SDR104
    SwitchGroup0Function {
        name: "SDR104",
        v: SMC_CAPS_UHS_SDR104,
        freq: 208000,
    },
    // DDR50
    SwitchGroup0Function {
        name: "DDR50",
        v: SMC_CAPS_UHS_DDR50,
        freq: 50000,
    },
];

/// `sdmmc_mmc_timings[]`: the MMC bus clock (kHz) of each timing.
pub static SDMMC_MMC_TIMINGS: [i32; 6] = {
    let mut t = [0; 6];
    t[SDMMC_TIMING_LEGACY as usize] = 26000;
    t[SDMMC_TIMING_HIGHSPEED as usize] = 52000;
    t[SDMMC_TIMING_MMC_DDR52 as usize] = 52000;
    t[SDMMC_TIMING_MMC_HS200 as usize] = 200000;
    t
};

/// `sdmmc_mem_enable`: initializes SD/MMC memory cards and memory in SDIO "combo" cards.
pub fn sdmmc_mem_enable(sc: &'static SdmmcSoftc) -> Result<(), Errno> {
    let mut ocr: u32 = 0;

    rw_assert_wrlock(&sc.sc_lock);

    // Set host mode to SD "combo" card or SD memory-only.
    sc.clr_flag(SMF_UHS_MODE);
    sc.set_flag(SMF_SD_MODE | SMF_MEM_MODE);

    // Reset memory (*must* do that before CMD55 or CMD1).
    sdmmc_go_idle_state(sc);

    // Read the SD/MMC memory OCR value by issuing CMD55 followed by ACMD41 to read the OCR
    // value from memory-only SD cards. MMC cards will not respond to CMD55 or ACMD41 and
    // this is how we distinguish them from SD cards.
    let card_ocr = loop {
        // mmc_mode:
        match sdmmc_mem_send_op_cond(sc, 0) {
            Ok(card_ocr) => break card_ocr,
            Err(_) => {
                if sc.has_flag(SMF_SD_MODE) && !sc.has_flag(SMF_IO_MODE) {
                    // Not a SD card, switch to MMC mode.
                    sc.clr_flag(SMF_SD_MODE);
                    continue;
                }
                if !sc.has_flag(SMF_SD_MODE) {
                    // DPRINTF(("%s: can't read memory OCR\n", DEVNAME(sc)));
                    return Err(Errno::EIO);
                } else {
                    // Not a "combo" card.
                    sc.clr_flag(SMF_MEM_MODE);
                    return Ok(());
                }
            }
        }
    };

    // Set the lowest voltage supported by the card and host.
    let mut host_ocr = sdmmc_chip_host_ocr(sc.sct(), sc.sch.get());
    if sdmmc_set_bus_power(sc, host_ocr, card_ocr).is_err() {
        // DPRINTF(("%s: can't supply voltage requested by card\n", DEVNAME(sc)));
        return Err(Errno::EIO);
    }

    // Tell the card(s) to enter the idle state (again).
    sdmmc_go_idle_state(sc);

    host_ocr &= card_ocr; // only allow the common voltages

    if sc.has_flag(SMF_SD_MODE) {
        if sdmmc_send_if_cond(sc, card_ocr).is_ok() {
            ocr |= MMC_OCR_HCS;
        }

        if sdmmc_chip_host_ocr(sc.sct(), sc.sch.get()) & MMC_OCR_S18A != 0 {
            ocr |= MMC_OCR_S18A;
        }
    }
    host_ocr |= ocr;

    // Send the new OCR value until all cards are ready.
    let Ok(new_ocr) = sdmmc_mem_send_op_cond(sc, host_ocr) else {
        // DPRINTF(("%s: can't send memory OCR\n", DEVNAME(sc)));
        return Err(Errno::EIO);
    };

    if sc.has_flag(SMF_SD_MODE) && new_ocr & MMC_OCR_S18A != 0 {
        // Card and host support low voltage mode, begin switch sequence.
        let mut cmd = SdmmcCommand::new();
        cmd.c_arg = 0;
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1;
        cmd.c_opcode = SD_VOLTAGE_SWITCH;
        // DPRINTF(("%s: switching card to 1.8V\n", DEVNAME(sc)));
        sdmmc_mmc_command(sc, &mut cmd)?;
        // (on failure: DPRINTF(("%s: voltage switch command failed\n", DEVNAME(sc))))

        sdmmc_mem_signal_voltage(sc, SDMMC_SIGNAL_VOLTAGE_180)?;

        sc.set_flag(SMF_UHS_MODE);
    }

    Ok(())
}

/// `sdmmc_mem_signal_voltage`: stops the clock, switches the host's signal voltage and
/// restarts the clock at SDR12.
pub fn sdmmc_mem_signal_voltage(sc: &SdmmcSoftc, signal_voltage: i32) -> Result<(), Errno> {
    // Stop the clock
    sdmmc_chip_bus_clock(sc.sct(), sc.sch.get(), 0, SDMMC_TIMING_LEGACY)?;

    delay(1000);

    // Card switch command was successful, update host controller signal voltage setting.
    // DPRINTF(("%s: switching host to %s\n", DEVNAME(sc),
    //     signal_voltage == SDMMC_SIGNAL_VOLTAGE_180 ? "1.8V" : "3.3V"));
    sdmmc_chip_signal_voltage(sc.sct(), sc.sch.get(), signal_voltage)?;

    delay(5000);

    // Switch to SDR12 timing
    sdmmc_chip_bus_clock(
        sc.sct(),
        sc.sch.get(),
        SDMMC_SDCLK_25MHZ,
        SDMMC_TIMING_LEGACY,
    )?;

    delay(1000);

    Ok(())
}

/// `sdmmc_mem_scan`: reads the CSD and CID from all cards and assigns each card a unique
/// relative card address (RCA). CMD2 is ignored by SDIO-only cards.
pub fn sdmmc_mem_scan(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    // CMD2 is a broadcast command understood by SD cards and MMC cards. All cards begin to
    // respond to the command, but back off if another card drives the CMD line to a
    // different level. Only one card will get its entire response through. That card
    // remains silent once it has been assigned a RCA.
    for _ in 0..100 {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_ALL_SEND_CID;
        cmd.c_flags = SCF_CMD_BCR | SCF_RSP_R2;

        match sdmmc_mmc_command(sc, &mut cmd) {
            Ok(()) => {}
            Err(Errno::ETIMEDOUT) => {
                // No more cards there.
                break;
            }
            Err(_) => {
                // DPRINTF(("%s: can't read CID\n", DEVNAME(sc)));
                break;
            }
        }

        // In MMC mode, find the next available RCA.
        let mut next_rca: u16 = 1;
        if !sc.has_flag(SMF_SD_MODE) {
            for _ in sc.sf_head.iter() {
                next_rca += 1;
            }
        }

        // Allocate a sdmmc_function structure.
        let sf = sdmmc_function_alloc(sc);
        sf.rca.set(next_rca);

        // Remember the CID returned in the CMD2 response for later decoding.
        sf.raw_cid.set(cmd.c_resp);

        // Silence the card by assigning it a unique RCA, or querying it for its RCA in the
        // case of SD.
        if sdmmc_set_relative_addr(sc, sf).is_err() {
            printf(format_args!("{}: can't set mem RCA\n", sc.devname()));
            // SAFETY: just allocated, on no list, not kept anywhere.
            unsafe { sdmmc_function_free(sf) };
            break;
        }

        // (#if 0 in the C: verify that the RCA has been set by selecting the card, then
        // deselect.)

        // If this is a memory-only card, the card responding first becomes an alias for
        // SDIO function 0.
        if sc.sc_fn0.get().is_null() {
            sc.sc_fn0.set(ptr::from_ref(sf));
        }

        // SAFETY: the function is on no list and lives until `sdmmc_card_detach` frees it
        // after taking the list apart.
        unsafe { sc.sf_head.insert_tail(sf) };
    }

    // All cards are either inactive or awaiting further commands. Read the CSDs and decode
    // the raw CID for each card.
    for sf in sc.sf_head.iter() {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_SEND_CSD;
        cmd.c_arg = mmc_arg_rca(sf.rca.get());
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R2;

        if sdmmc_mmc_command(sc, &mut cmd).is_err() {
            sf.flags.set(sf.flags.get() | SFF_ERROR);
            continue;
        }

        if sdmmc_decode_csd(sc, &cmd.c_resp, sf).is_err()
            || sdmmc_decode_cid(sc, &sf.raw_cid.get(), sf).is_err()
        {
            sf.flags.set(sf.flags.get() | SFF_ERROR);
            continue;
        }

        // SDMMC_DEBUG: printf("%s: CID: ", DEVNAME(sc)); sdmmc_print_cid(&sf->cid);
    }
}

/// `sdmmc_decode_csd`.
pub fn sdmmc_decode_csd(
    sc: &SdmmcSoftc,
    resp: &SdmmcResponse,
    sf: &SdmmcFunction,
) -> Result<(), Errno> {
    let mut csd = sf.csd.get();

    if sc.has_flag(SMF_SD_MODE) {
        // CSD version 1.0 corresponds to SD system specification version 1.0 - 1.10.
        // (SanDisk, 3.5.3)
        csd.csdver = sd_csd_csdver(resp);
        match csd.csdver {
            SD_CSD_CSDVER_2_0 => {
                sf.flags.set(sf.flags.get() | SFF_SDHC);
                csd.capacity = sd_csd_v2_capacity(resp);
                csd.read_bl_len = SD_CSD_V2_BL_LEN;
            }
            SD_CSD_CSDVER_1_0 => {
                csd.capacity = sd_csd_capacity(resp);
                csd.read_bl_len = sd_csd_read_bl_len(resp);
            }
            _ => {
                printf(format_args!(
                    "{}: unknown SD CSD structure version 0x{:x}\n",
                    sc.devname(),
                    csd.csdver
                ));
                sf.csd.set(csd);
                return Err(Errno::EIO);
            }
        }
        csd.ccc = sd_csd_ccc(resp);
    } else {
        csd.csdver = mmc_csd_csdver(resp);
        if csd.csdver == MMC_CSD_CSDVER_1_0
            || csd.csdver == MMC_CSD_CSDVER_2_0
            || csd.csdver == MMC_CSD_CSDVER_EXT_CSD
        {
            csd.mmcver = mmc_csd_mmcver(resp);
            csd.capacity = mmc_csd_capacity(resp);
            csd.read_bl_len = mmc_csd_read_bl_len(resp);
        } else {
            printf(format_args!(
                "{}: unknown MMC CSD structure version 0x{:x}\n",
                sc.devname(),
                csd.csdver
            ));
            sf.csd.set(csd);
            return Err(Errno::EIO);
        }
    }
    let maxblklen = sdmmc_chip_host_maxblklen(sc.sct(), sc.sch.get());
    csd.sector_size = (1 << csd.read_bl_len).min(maxblklen);
    if csd.sector_size < (1 << csd.read_bl_len) {
        csd.capacity *= (1 << csd.read_bl_len) / csd.sector_size;
    }
    sf.csd.set(csd);

    Ok(())
}

/// `sdmmc_decode_cid`.
pub fn sdmmc_decode_cid(
    sc: &SdmmcSoftc,
    resp: &SdmmcResponse,
    sf: &SdmmcFunction,
) -> Result<(), Errno> {
    let mut cid = sf.cid.get();

    if sc.has_flag(SMF_SD_MODE) {
        cid.mid = sd_cid_mid(resp);
        cid.oid = sd_cid_oid(resp);
        sd_cid_pnm_cpy(resp, &mut cid.pnm);
        cid.rev = sd_cid_rev(resp);
        cid.psn = sd_cid_psn(resp);
        cid.mdt = sd_cid_mdt(resp);
    } else {
        match sf.csd.get().mmcver {
            MMC_CSD_MMCVER_1_0 | MMC_CSD_MMCVER_1_4 => {
                cid.mid = mmc_cid_mid_v1(resp);
                mmc_cid_pnm_v1_cpy(resp, &mut cid.pnm);
                cid.rev = mmc_cid_rev_v1(resp);
                cid.psn = mmc_cid_psn_v1(resp);
                cid.mdt = mmc_cid_mdt_v1(resp);
            }
            MMC_CSD_MMCVER_2_0 | MMC_CSD_MMCVER_3_1 | MMC_CSD_MMCVER_4_0 => {
                cid.mid = mmc_cid_mid_v2(resp);
                cid.oid = mmc_cid_oid_v2(resp);
                mmc_cid_pnm_v2_cpy(resp, &mut cid.pnm);
                cid.psn = mmc_cid_psn_v2(resp);
            }
            v => {
                printf(format_args!(
                    "{}: unknown MMC version {}\n",
                    sc.devname(),
                    v
                ));
                sf.cid.set(cid);
                return Err(Errno::EIO);
            }
        }
    }
    sf.cid.set(cid);
    Ok(())
}

/// A zeroed `malloc(9)` scratch buffer for a command's data (`M_NOWAIT | M_ZERO`).
fn sdmmc_mem_scratch(len: usize) -> Result<NonNull<u8>, Errno> {
    malloc(len, M_DEVBUF, M_NOWAIT | M_ZERO).ok_or(Errno::ENOMEM)
}

/// `sdmmc_mem_send_scr`: reads the SD Configuration Register (ACMD51) into `scr`, as the
/// card sends it (big-endian).
pub fn sdmmc_mem_send_scr(sc: &SdmmcSoftc, scr: &mut [u32; 2]) -> Result<(), Errno> {
    let datalen: usize = 8;

    let ptr = sdmmc_mem_scratch(datalen)?;

    let mut cmd = SdmmcCommand::new();
    cmd.c_data = ptr.as_ptr();
    cmd.c_datalen = datalen as i32;
    cmd.c_blklen = datalen as i32;
    cmd.c_arg = 0;
    cmd.c_flags = SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1;
    cmd.c_opcode = SD_APP_SEND_SCR;

    let error = sdmmc_app_command(sc, &mut cmd);
    if error.is_ok() {
        // SAFETY: the scratch buffer holds `datalen` bytes, the command is over.
        let bytes = unsafe { core::slice::from_raw_parts(ptr.as_ptr(), datalen) };
        for (w, b) in scr.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *w = u32::from_ne_bytes(*b);
        }
    }

    free(ptr, M_DEVBUF, datalen);

    error
}

/// `sdmmc_mem_decode_scr`.
pub fn sdmmc_mem_decode_scr(
    _sc: &SdmmcSoftc,
    raw_scr: &[u32; 2],
    sf: &SdmmcFunction,
) -> Result<(), Errno> {
    let resp = sdmmc_scr_response(raw_scr);

    let ver = scr_structure(&resp);
    let mut scr = sf.scr.get();
    scr.sd_spec = scr_sd_spec(&resp);
    scr.bus_width = scr_sd_bus_widths(&resp);
    sf.scr.set(scr);

    // DPRINTF(("%s: %s: %08x%08x ver=%d, spec=%d, bus width=%d\n", DEVNAME(sc), __func__,
    //     resp[1], resp[0], ver, sf->scr.sd_spec, sf->scr.bus_width));

    if ver != 0 {
        // DPRINTF(("%s: unknown SCR structure version: %d\n", DEVNAME(sc), ver));
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// The SCR as a response `MMC_RSP_BITS` reads ("change the raw SCR to a response").
pub fn sdmmc_scr_response(raw_scr: &[u32; 2]) -> SdmmcResponse {
    let mut resp: SdmmcResponse = [0; 4];
    resp[0] = u32::from_be(raw_scr[1]) >> 8; // LSW
    resp[1] = u32::from_be(raw_scr[0]); // MSW
    resp[0] |= (resp[1] & 0xff) << 24;
    resp[1] >>= 8;
    resp
}

/// `sdmmc_mem_send_cxd_data`: reads a register block (EXT_CSD) into `data`.
pub fn sdmmc_mem_send_cxd_data(sc: &SdmmcSoftc, opcode: u16, data: &mut [u8]) -> Result<(), Errno> {
    let datalen = data.len();

    let ptr = sdmmc_mem_scratch(datalen)?;

    let mut cmd = SdmmcCommand::new();
    cmd.c_data = ptr.as_ptr();
    cmd.c_datalen = datalen as i32;
    cmd.c_blklen = datalen as i32;
    cmd.c_opcode = opcode;
    cmd.c_arg = 0;
    cmd.c_flags = SCF_CMD_ADTC | SCF_CMD_READ;
    if opcode == MMC_SEND_EXT_CSD {
        cmd.c_flags |= SCF_RSP_R1;
    } else {
        cmd.c_flags |= SCF_RSP_R2;
    }

    let error = sdmmc_mmc_command(sc, &mut cmd);
    if error.is_ok() {
        // SAFETY: the scratch buffer holds `datalen` bytes, the command is over.
        data.copy_from_slice(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), datalen) });
    }

    free(ptr, M_DEVBUF, datalen);

    error
}

/// `sdmmc_mem_set_bus_width`: ACMD6, then the host's bus width.
pub fn sdmmc_mem_set_bus_width(sf: &SdmmcFunction, width: i32) -> Result<(), Errno> {
    let sc = sf.sc;
    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = SD_APP_SET_BUS_WIDTH;
    cmd.c_flags = SCF_RSP_R1 | SCF_CMD_AC;

    cmd.c_arg = match width {
        1 => SD_ARG_BUS_WIDTH_1,
        4 => SD_ARG_BUS_WIDTH_4,
        _ => return Err(Errno::EINVAL),
    };

    sdmmc_app_command(sc, &mut cmd)?;
    sdmmc_chip_bus_width(sc.sct(), sc.sch.get(), width)
}

/// `sdmmc_mem_sd_switch`: CMD6 in `mode` (0 check, 1 switch) for `function` of `group`;
/// the 512-bit status lands in `status`.
pub fn sdmmc_mem_sd_switch(
    sf: &SdmmcFunction,
    mode: i32,
    group: i32,
    function: i32,
    status: &mut SdmmcBitfield512,
) -> Result<(), Errno> {
    let sc = sf.sc;
    let statlen: usize = 64;

    if sf.scr.get().sd_spec >= SCR_SD_SPEC_VER_1_10 && sf.csd.get().ccc & SD_CSD_CCC_SWITCH == 0 {
        return Err(Errno::EINVAL);
    }

    if group <= 0 || group > 6 || !(0..=15).contains(&function) {
        return Err(Errno::EINVAL);
    }

    let ptr = sdmmc_mem_scratch(statlen)?;

    let mut cmd = SdmmcCommand::new();
    cmd.c_data = ptr.as_ptr();
    cmd.c_datalen = statlen as i32;
    cmd.c_blklen = statlen as i32;
    cmd.c_opcode = SD_SEND_SWITCH_FUNC;
    cmd.c_arg = sdmmc_switch_arg(mode, group, function);
    cmd.c_flags = SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1;

    let error = sdmmc_mmc_command(sc, &mut cmd);
    if error.is_ok() {
        // SAFETY: the scratch buffer holds `statlen` bytes, the command is over.
        let bytes = unsafe { core::slice::from_raw_parts(ptr.as_ptr(), statlen) };
        for (w, b) in status._bits.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *w = u32::from_ne_bytes(*b);
        }
        sdmmc_be512_to_bitfield512(status);
    }

    free(ptr, M_DEVBUF, statlen);

    error
}

/// The CMD6 argument: the mode in bit 31, `function` in its group's nibble, every other
/// group's nibble 0xf ("no change").
pub const fn sdmmc_switch_arg(mode: i32, group: i32, function: i32) -> u32 {
    let gsft = ((group - 1) << 2) as u32;
    (((mode != 0) as u32) << 31) | ((function as u32) << gsft) | (0x00ff_ffff & !(0xf << gsft))
}

/// `sdmmc_mem_mmc_switch`: CMD6 (MMC): writes `value` into EXT_CSD byte `index`.
pub fn sdmmc_mem_mmc_switch(
    sf: &SdmmcFunction,
    set: u8,
    index: u8,
    value: u8,
) -> Result<(), Errno> {
    let sc = sf.sc;
    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = MMC_SWITCH;
    cmd.c_arg = (MMC_SWITCH_MODE_WRITE_BYTE << 24)
        | (u32::from(index) << 16)
        | (u32::from(value) << 8)
        | u32::from(set);
    cmd.c_flags = SCF_RSP_R1B | SCF_CMD_AC;

    sdmmc_mmc_command(sc, &mut cmd)
}

/// `sdmmc_mem_init`: initializes a SD/MMC memory card.
pub fn sdmmc_mem_init(sc: &'static SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // The C sets `error = 1` when either fails and overwrites it below.
    let _ = sdmmc_select_card(sc, Some(sf)).and_then(|()| sdmmc_mem_set_blocklen(sc, sf));

    if sc.has_flag(SMF_SD_MODE) {
        sdmmc_mem_sd_init(sc, sf)
    } else {
        sdmmc_mem_mmc_init(sc, sf)
    }
}

/// `sdmmc_be512_to_bitfield512`: makes a 512-bit big-endian quantity
/// `__bitfield()`-compatible.
pub fn sdmmc_be512_to_bitfield512(buf: &mut SdmmcBitfield512) {
    let bitswords = buf._bits.len();
    for i in 0..bitswords / 2 {
        let tmp0 = buf._bits[i];
        let tmp1 = buf._bits[bitswords - 1 - i];
        buf._bits[i] = u32::from_be(tmp1);
        buf._bits[bitswords - 1 - i] = u32::from_be(tmp0);
    }
}

/// `sdmmc_mem_select_transfer_mode`: the fastest access mode both sides support.
pub fn sdmmc_mem_select_transfer_mode(sc: &SdmmcSoftc, support_func: i32) -> i32 {
    if sc.has_flag(SMF_UHS_MODE) {
        if sc.has_caps(SMC_CAPS_UHS_SDR104) && support_func & (1 << SD_ACCESS_MODE_SDR104) != 0 {
            return SD_ACCESS_MODE_SDR104;
        }
        if sc.has_caps(SMC_CAPS_UHS_DDR50) && support_func & (1 << SD_ACCESS_MODE_DDR50) != 0 {
            return SD_ACCESS_MODE_DDR50;
        }
        if sc.has_caps(SMC_CAPS_UHS_SDR50) && support_func & (1 << SD_ACCESS_MODE_SDR50) != 0 {
            return SD_ACCESS_MODE_SDR50;
        }
    }
    if sc.has_caps(SMC_CAPS_SD_HIGHSPEED) && support_func & (1 << SD_ACCESS_MODE_SDR25) != 0 {
        return SD_ACCESS_MODE_SDR25;
    }
    SD_ACCESS_MODE_SDR12
}

/// `sdmmc_mem_execute_tuning`.
pub fn sdmmc_mem_execute_tuning(sc: &SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    let timing = if sc.has_flag(SMF_SD_MODE) {
        if !sc.has_flag(SMF_UHS_MODE) {
            return Ok(());
        }

        match sf.csd.get().tran_speed {
            100000 => SDMMC_TIMING_UHS_SDR50,
            208000 => SDMMC_TIMING_UHS_SDR104,
            _ => return Ok(()),
        }
    } else {
        match sf.csd.get().tran_speed {
            200000 => SDMMC_TIMING_MMC_HS200,
            _ => return Ok(()),
        }
    };

    // DPRINTF(("%s: execute tuning for timing %d\n", DEVNAME(sc), timing));

    sdmmc_chip_execute_tuning(sc.sct(), sc.sch.get(), timing)
}

/// `sdmmc_mem_sd_init`: the SD card's SCR, bus width and access mode.
pub fn sdmmc_mem_sd_init(sc: &'static SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    let mut support_func: i32 = 0;
    let mut status = SdmmcBitfield512::default(); // Switch Function Status
    let mut raw_scr = [0u32; 2];

    // All SD cards are supposed to support Default Speed mode with frequencies up to 25
    // MHz. Bump up the clock frequency now as data transfers don't seem to work on the
    // Realtek RTS5229 host controller if it is running at a low clock frequency. Reading
    // the SCR requires a data transfer.
    if let Err(e) = sdmmc_chip_bus_clock(
        sc.sct(),
        sc.sch.get(),
        SDMMC_SDCLK_25MHZ,
        SDMMC_TIMING_LEGACY,
    ) {
        printf(format_args!("{}: can't change bus clock\n", sc.devname()));
        return Err(e);
    }

    if let Err(e) = sdmmc_mem_send_scr(sc, &mut raw_scr) {
        printf(format_args!("{}: SD_SEND_SCR send failed\n", sc.devname()));
        return Err(e);
    }
    sdmmc_mem_decode_scr(sc, &raw_scr, sf)?;

    if sc.has_caps(SMC_CAPS_4BIT_MODE) && sf.scr.get().bus_width & SCR_SD_BUS_WIDTHS_4BIT != 0 {
        // DPRINTF(("%s: change bus width\n", DEVNAME(sc)));
        if let Err(e) = sdmmc_mem_set_bus_width(sf, 4) {
            printf(format_args!("{}: can't change bus width\n", sc.devname()));
            return Err(e);
        }
    }

    let mut best_func = 0;
    if sf.scr.get().sd_spec >= SCR_SD_SPEC_VER_1_10 && sf.csd.get().ccc & SD_CSD_CCC_SWITCH != 0 {
        // DPRINTF(("%s: switch func mode 0\n", DEVNAME(sc)));
        if let Err(e) = sdmmc_mem_sd_switch(sf, 0, 1, 0, &mut status) {
            printf(format_args!(
                "{}: switch func mode 0 failed\n",
                sc.devname()
            ));
            return Err(e);
        }

        support_func = sfunc_status_group(&status._bits, 1);

        if !sc.has_flag(SMF_UHS_MODE)
            && (support_func & (1 << SD_ACCESS_MODE_SDR50) != 0
                || support_func & (1 << SD_ACCESS_MODE_DDR50) != 0
                || support_func & (1 << SD_ACCESS_MODE_SDR104) != 0)
        {
            // XXX UHS-I card started in 1.8V mode, switch now
            if let Err(e) = sdmmc_mem_signal_voltage(sc, SDMMC_SIGNAL_VOLTAGE_180) {
                printf(format_args!(
                    "{}: failed to recover UHS card\n",
                    sc.devname()
                ));
                return Err(e);
            }
            sc.set_flag(SMF_UHS_MODE);
        }

        // (DPRINTF of each mode the card supports:
        //  for i in switch_group0_functions: "%s: card supports mode %s\n")

        best_func = sdmmc_mem_select_transfer_mode(sc, support_func);

        // DPRINTF(("%s: using mode %s\n", DEVNAME(sc),
        //     switch_group0_functions[best_func].name));
    }

    if best_func != 0 {
        // DPRINTF(("%s: switch func mode 1(func=%d)\n", DEVNAME(sc), best_func));
        if let Err(e) = sdmmc_mem_sd_switch(sf, 1, 1, best_func, &mut status) {
            printf(format_args!(
                "{}: switch func mode 1 failed: group 1 function {}(0x{:2x})\n",
                sc.devname(),
                best_func,
                support_func
            ));
            return Err(e);
        }
        let mut csd = sf.csd.get();
        csd.tran_speed = SWITCH_GROUP0_FUNCTIONS[best_func as usize].freq;
        sf.csd.set(csd);

        // Wait 400KHz x 8 clock (2.5us * 8 + slop)
        delay(25);

        // change bus clock
        if let Err(e) = sdmmc_chip_bus_clock(
            sc.sct(),
            sc.sch.get(),
            csd.tran_speed,
            SDMMC_TIMING_HIGHSPEED,
        ) {
            printf(format_args!("{}: can't change bus clock\n", sc.devname()));
            return Err(e);
        }

        // execute tuning (UHS)
        if let Err(e) = sdmmc_mem_execute_tuning(sc, sf) {
            printf(format_args!("{}: can't execute SD tuning\n", sc.devname()));
            return Err(e);
        }
    }

    Ok(())
}

/// `sdmmc_mem_mmc_init`: the MMC card's EXT_CSD, bus width, timing and size.
pub fn sdmmc_mem_mmc_init(sc: &'static SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    let mut ext_csd = [0u8; 512];
    let mut speed = 20000;
    let mut timing = SDMMC_TIMING_LEGACY;

    if let Err(e) = sdmmc_chip_bus_clock(sc.sct(), sc.sch.get(), speed, timing) {
        printf(format_args!("{}: can't change bus clock\n", sc.devname()));
        return Err(e);
    }

    if sf.csd.get().mmcver >= MMC_CSD_MMCVER_4_0 {
        // read EXT_CSD
        if let Err(e) = sdmmc_mem_send_cxd_data(sc, MMC_SEND_EXT_CSD, &mut ext_csd) {
            sf.flags.set(sf.flags.get() | SFF_ERROR);
            printf(format_args!("{}: can't read EXT_CSD\n", sc.devname()));
            return Err(e);
        }

        let card_type = ext_csd[EXT_CSD_CARD_TYPE as usize];

        if card_type & EXT_CSD_CARD_TYPE_F_HS200_1_8V != 0 && sc.has_caps(SMC_CAPS_MMC_HS200) {
            speed = 200000;
            timing = SDMMC_TIMING_MMC_HS200;
        } else if card_type & EXT_CSD_CARD_TYPE_F_DDR52_1_8V != 0 && sc.has_caps(SMC_CAPS_MMC_DDR52)
        {
            speed = 52000;
            timing = SDMMC_TIMING_MMC_DDR52;
        } else if card_type & EXT_CSD_CARD_TYPE_F_52M != 0 && sc.has_caps(SMC_CAPS_MMC_HIGHSPEED) {
            speed = 52000;
            timing = SDMMC_TIMING_HIGHSPEED;
        } else if card_type & EXT_CSD_CARD_TYPE_F_26M != 0 {
            speed = 26000;
        } else {
            printf(format_args!(
                "{}: unknown CARD_TYPE 0x{:x}\n",
                sc.devname(),
                ext_csd[EXT_CSD_CARD_TYPE as usize]
            ));
        }

        let (width, mut value) = if sc.has_caps(SMC_CAPS_8BIT_MODE) {
            (8, EXT_CSD_BUS_WIDTH_8)
        } else if sc.has_caps(SMC_CAPS_4BIT_MODE) {
            (4, EXT_CSD_BUS_WIDTH_4)
        } else {
            (1, EXT_CSD_BUS_WIDTH_1)
        };

        if width != 1 {
            sdmmc_mem_mmc_switch(sf, EXT_CSD_CMD_SET_NORMAL, EXT_CSD_BUS_WIDTH, value)?;
            // (on failure: DPRINTF(("%s: can't change bus width (%d bit)\n", ...)))
            // The C keeps the chip's answer in `error`, which the bus clock change below
            // overwrites: a failure here is not returned.
            let _ = sdmmc_chip_bus_width(sc.sct(), sc.sch.get(), width);

            // XXXX: need bus test? (using by CMD14 & CMD19)
            sdmmc_delay(10000);
        }

        if timing != SDMMC_TIMING_LEGACY {
            match timing {
                SDMMC_TIMING_MMC_HS200 => value = EXT_CSD_HS_TIMING_HS200,
                SDMMC_TIMING_MMC_DDR52 | SDMMC_TIMING_HIGHSPEED => value = EXT_CSD_HS_TIMING_HS,
                _ => {}
            }

            // switch to high speed timing
            if let Err(e) =
                sdmmc_mem_mmc_switch(sf, EXT_CSD_CMD_SET_NORMAL, EXT_CSD_HS_TIMING, value)
            {
                printf(format_args!("{}: can't change timing\n", sc.devname()));
                return Err(e);
            }

            sdmmc_delay(10000);
        }

        kassert!((timing as usize) < SDMMC_MMC_TIMINGS.len());
        let mut csd = sf.csd.get();
        csd.tran_speed = SDMMC_MMC_TIMINGS[timing as usize];
        sf.csd.set(csd);

        if timing != SDMMC_TIMING_LEGACY {
            // read EXT_CSD again
            if let Err(e) = sdmmc_mem_send_cxd_data(sc, MMC_SEND_EXT_CSD, &mut ext_csd) {
                printf(format_args!("{}: can't re-read EXT_CSD\n", sc.devname()));
                return Err(e);
            }
            if ext_csd[EXT_CSD_HS_TIMING as usize] != value {
                printf(format_args!("{}, HS_TIMING set failed\n", sc.devname()));
                return Err(Errno::EINVAL);
            }
        }

        if let Err(e) = sdmmc_chip_bus_clock(sc.sct(), sc.sch.get(), speed, SDMMC_TIMING_HIGHSPEED)
        {
            printf(format_args!("{}: can't change bus clock\n", sc.devname()));
            return Err(e);
        }

        if timing == SDMMC_TIMING_MMC_DDR52 {
            match width {
                4 => value = EXT_CSD_BUS_WIDTH_4_DDR,
                8 => value = EXT_CSD_BUS_WIDTH_8_DDR,
                _ => {}
            }

            if let Err(e) =
                sdmmc_mem_mmc_switch(sf, EXT_CSD_CMD_SET_NORMAL, EXT_CSD_BUS_WIDTH, value)
            {
                printf(format_args!("{}: can't switch to DDR\n", sc.devname()));
                return Err(e);
            }

            sdmmc_delay(10000);

            if let Err(e) =
                sdmmc_chip_signal_voltage(sc.sct(), sc.sch.get(), SDMMC_SIGNAL_VOLTAGE_180)
            {
                printf(format_args!(
                    "{}: can't switch signalling voltage\n",
                    sc.devname()
                ));
                return Err(e);
            }

            if let Err(e) = sdmmc_chip_bus_clock(sc.sct(), sc.sch.get(), speed, timing) {
                printf(format_args!("{}: can't change bus clock\n", sc.devname()));
                return Err(e);
            }

            sdmmc_delay(10000);
        }

        let off = EXT_CSD_SEC_COUNT as usize;
        let sectors = u32::from_le_bytes([
            ext_csd[off],
            ext_csd[off + 1],
            ext_csd[off + 2],
            ext_csd[off + 3],
        ]);

        if sectors > (2u32 * 1024 * 1024 * 1024) / 512 {
            sf.flags.set(sf.flags.get() | SFF_SDHC);
            let mut csd = sf.csd.get();
            csd.capacity = sectors as i32;
            sf.csd.set(csd);
        }

        if timing == SDMMC_TIMING_MMC_HS200 {
            // execute tuning (HS200)
            if let Err(e) = sdmmc_mem_execute_tuning(sc, sf) {
                printf(format_args!("{}: can't execute MMC tuning\n", sc.devname()));
                return Err(e);
            }
        }
    }

    Ok(())
}

/// `sdmmc_mem_send_op_cond`: gets or sets the card's memory OCR value (SD or MMC); the OCR
/// of the last response on success.
pub fn sdmmc_mem_send_op_cond(sc: &SdmmcSoftc, ocr: u32) -> Result<u32, Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // If we change the OCR value, retry the command until the OCR we receive in response
    // has the "CARD BUSY" bit set, meaning that all cards are ready for identification.
    let mut cmd = SdmmcCommand::new();
    let mut error: Result<(), Errno> = Ok(());
    for _ in 0..100 {
        cmd = SdmmcCommand::new();
        cmd.c_arg = ocr;
        cmd.c_flags = SCF_CMD_BCR | SCF_RSP_R3;

        if sc.has_flag(SMF_SD_MODE) {
            cmd.c_opcode = SD_APP_OP_COND;
            error = sdmmc_app_command(sc, &mut cmd);
        } else {
            cmd.c_arg &= !MMC_OCR_ACCESS_MODE_MASK;
            cmd.c_arg |= MMC_OCR_ACCESS_MODE_SECTOR;
            cmd.c_opcode = MMC_SEND_OP_COND;
            error = sdmmc_mmc_command(sc, &mut cmd);
        }
        if error.is_err() {
            break;
        }
        if mmc_r3(&cmd.c_resp) & MMC_OCR_MEM_READY != 0 || ocr == 0 {
            break;
        }
        error = Err(Errno::ETIMEDOUT);
        sdmmc_delay(10000);
    }
    error.map(|()| mmc_r3(&cmd.c_resp))
}

/// `sdmmc_mem_set_blocklen`: sets the read block length appropriately for this card,
/// according to the card CSD register value.
pub fn sdmmc_mem_set_blocklen(sc: &SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = MMC_SET_BLOCKLEN;
    cmd.c_arg = sf.csd.get().sector_size as u32;
    cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1;
    // DPRINTF(("%s: read_bl_len=%d sector_size=%d\n", DEVNAME(sc),
    //     1 << sf->csd.read_bl_len, sf->csd.sector_size));

    sdmmc_mmc_command(sc, &mut cmd)
}

/// The block address argument of a read or write: the block number on a high capacity
/// card, the byte offset (`blkno << 9`) on the others.
fn sdmmc_mem_blkarg(sf: &SdmmcFunction, blkno: i32) -> u32 {
    if sf.flags.get() & SFF_SDHC != 0 {
        blkno as u32
    } else {
        (blkno << 9) as u32
    }
}

/// Polls CMD13 until the card says it is ready for data ("XXX time out": none, as in C).
fn sdmmc_mem_wait_ready(sc: &SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    loop {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_SEND_STATUS;
        cmd.c_arg = mmc_arg_rca(sf.rca.get());
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1;
        sdmmc_mmc_command(sc, &mut cmd)?;
        // XXX time out
        if mmc_r1(&cmd.c_resp) & MMC_R1_READY_FOR_DATA != 0 {
            return Ok(());
        }
    }
}

/// `sdmmc_mem_read_block_subr`.
///
/// # Safety
///
/// `data` points to `datalen` writable bytes (loaded into `dmap` when it is given) that
/// nothing else uses until the call returns.
pub unsafe fn sdmmc_mem_read_block_subr(
    sf: &SdmmcFunction,
    dmap: Option<&'static BusDmamap>,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let sc = sf.sc;

    sdmmc_select_card(sc, Some(sf))?;

    let mut cmd = SdmmcCommand::new();
    cmd.c_data = data;
    cmd.c_datalen = datalen as i32;
    cmd.c_blklen = sf.csd.get().sector_size;
    cmd.c_opcode = if (cmd.c_datalen / cmd.c_blklen) > 1 {
        MMC_READ_BLOCK_MULTIPLE
    } else {
        MMC_READ_BLOCK_SINGLE
    };
    cmd.c_arg = sdmmc_mem_blkarg(sf, blkno);
    cmd.c_flags = SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1;
    cmd.c_dmamap = dmap;

    sdmmc_mmc_command(sc, &mut cmd)?;

    if sc.has_flag(SMF_STOP_AFTER_MULTIPLE) && cmd.c_opcode == MMC_READ_BLOCK_MULTIPLE {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_STOP_TRANSMISSION;
        cmd.c_arg = mmc_arg_rca(sf.rca.get());
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1B;
        sdmmc_mmc_command(sc, &mut cmd)?;
    }

    sdmmc_mem_wait_ready(sc, sf)
}

/// `sdmmc_mem_single_read_block`: one sector at a time.
///
/// # Safety
///
/// As for [`sdmmc_mem_read_block_subr`].
pub unsafe fn sdmmc_mem_single_read_block(
    sf: &SdmmcFunction,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let ss = sf.csd.get().sector_size as usize;
    for i in 0..datalen / ss {
        // SAFETY: sector `i` lies inside the caller's `datalen` bytes.
        unsafe { sdmmc_mem_read_block_subr(sf, None, blkno + i as i32, data.add(i * ss), ss)? };
    }

    Ok(())
}

/// `sdmmc_mem_read_block`: reads `datalen` bytes from sector `blkno` into `data`.
///
/// # Safety
///
/// `data` points to `datalen` writable bytes that nothing else uses until the call returns.
pub unsafe fn sdmmc_mem_read_block(
    sf: &SdmmcFunction,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let sc = sf.sc;

    rw_enter_write(&sc.sc_lock);

    let error = if sc.has_caps(SMC_CAPS_SINGLE_ONLY) {
        // SAFETY: the caller's guarantee.
        unsafe { sdmmc_mem_single_read_block(sf, blkno, data, datalen) }
    } else if !sc.has_caps(SMC_CAPS_DMA) {
        // SAFETY: the caller's guarantee.
        unsafe { sdmmc_mem_read_block_subr(sf, None, blkno, data, datalen) }
    } else {
        // DMA transfer
        let dmap = sc.dmap();
        // SAFETY: the caller's guarantee: the buffer stays valid until it is unloaded below.
        match unsafe {
            bus_dmamap_load(
                sc.dmat(),
                dmap,
                data,
                datalen,
                None,
                BUS_DMA_NOWAIT | BUS_DMA_READ,
            )
        } {
            Err(e) => Err(e),
            Ok(()) => {
                bus_dmamap_sync(sc.dmat(), dmap, 0, datalen, BUS_DMASYNC_PREREAD);

                // SAFETY: the caller's guarantee; the buffer is loaded into `dmap`.
                let error =
                    unsafe { sdmmc_mem_read_block_subr(sf, Some(dmap), blkno, data, datalen) };
                if error.is_ok() {
                    bus_dmamap_sync(sc.dmat(), dmap, 0, datalen, BUS_DMASYNC_POSTREAD);
                }
                // unload:
                bus_dmamap_unload(sc.dmat(), dmap);
                error
            }
        }
    };

    // out:
    rw_exit(&sc.sc_lock);
    error
}

/// `sdmmc_mem_write_block_subr`.
///
/// # Safety
///
/// `data` points to `datalen` readable bytes (loaded into `dmap` when it is given) that
/// nothing changes until the call returns.
pub unsafe fn sdmmc_mem_write_block_subr(
    sf: &SdmmcFunction,
    dmap: Option<&'static BusDmamap>,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let sc = sf.sc;

    sdmmc_select_card(sc, Some(sf))?;

    let mut cmd = SdmmcCommand::new();
    cmd.c_data = data;
    cmd.c_datalen = datalen as i32;
    cmd.c_blklen = sf.csd.get().sector_size;
    cmd.c_opcode = if (cmd.c_datalen / cmd.c_blklen) > 1 {
        MMC_WRITE_BLOCK_MULTIPLE
    } else {
        MMC_WRITE_BLOCK_SINGLE
    };
    cmd.c_arg = sdmmc_mem_blkarg(sf, blkno);
    cmd.c_flags = SCF_CMD_ADTC | SCF_RSP_R1;
    cmd.c_dmamap = dmap;

    sdmmc_mmc_command(sc, &mut cmd)?;

    if sc.has_flag(SMF_STOP_AFTER_MULTIPLE) && cmd.c_opcode == MMC_WRITE_BLOCK_MULTIPLE {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_STOP_TRANSMISSION;
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1B;
        sdmmc_mmc_command(sc, &mut cmd)?;
    }

    sdmmc_mem_wait_ready(sc, sf)
}

/// `sdmmc_mem_single_write_block`: one sector at a time.
///
/// # Safety
///
/// As for [`sdmmc_mem_write_block_subr`].
pub unsafe fn sdmmc_mem_single_write_block(
    sf: &SdmmcFunction,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let ss = sf.csd.get().sector_size as usize;
    for i in 0..datalen / ss {
        // SAFETY: sector `i` lies inside the caller's `datalen` bytes.
        unsafe { sdmmc_mem_write_block_subr(sf, None, blkno + i as i32, data.add(i * ss), ss)? };
    }

    Ok(())
}

/// `sdmmc_mem_write_block`: writes `datalen` bytes from `data` to sector `blkno`.
///
/// # Safety
///
/// `data` points to `datalen` readable bytes that nothing changes until the call returns.
pub unsafe fn sdmmc_mem_write_block(
    sf: &SdmmcFunction,
    blkno: i32,
    data: *mut u8,
    datalen: usize,
) -> Result<(), Errno> {
    let sc = sf.sc;

    rw_enter_write(&sc.sc_lock);

    let error = if sc.has_caps(SMC_CAPS_SINGLE_ONLY) {
        // SAFETY: the caller's guarantee.
        unsafe { sdmmc_mem_single_write_block(sf, blkno, data, datalen) }
    } else if !sc.has_caps(SMC_CAPS_DMA) {
        // SAFETY: the caller's guarantee.
        unsafe { sdmmc_mem_write_block_subr(sf, None, blkno, data, datalen) }
    } else {
        // DMA transfer
        let dmap = sc.dmap();
        // SAFETY: the caller's guarantee: the buffer stays valid until it is unloaded below.
        match unsafe {
            bus_dmamap_load(
                sc.dmat(),
                dmap,
                data,
                datalen,
                None,
                BUS_DMA_NOWAIT | BUS_DMA_WRITE,
            )
        } {
            Err(e) => Err(e),
            Ok(()) => {
                bus_dmamap_sync(sc.dmat(), dmap, 0, datalen, BUS_DMASYNC_PREWRITE);

                // SAFETY: the caller's guarantee; the buffer is loaded into `dmap`.
                let error =
                    unsafe { sdmmc_mem_write_block_subr(sf, Some(dmap), blkno, data, datalen) };
                if error.is_ok() {
                    bus_dmamap_sync(sc.dmat(), dmap, 0, datalen, BUS_DMASYNC_POSTWRITE);
                }
                // unload:
                bus_dmamap_unload(sc.dmat(), dmap);
                error
            }
        }
    };

    // out:
    rw_exit(&sc.sc_lock);
    error
}

// HIBERNATE: not configured (sdmmc_mem_hibernate_write).
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_scr_becomes_a_response() {
        // QEMU's SCR: 0x02b5_8000_0000_0000 big-endian: structure 0, SD spec 2 (bits 59:56),
        // bus widths 0x5 (bits 51:48).
        let raw: [u8; 8] = [0x02, 0xb5, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00];
        let raw_scr = [
            u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]),
            u32::from_ne_bytes([raw[4], raw[5], raw[6], raw[7]]),
        ];
        let resp = sdmmc_scr_response(&raw_scr);
        assert_eq!(scr_structure(&resp), 0);
        assert_eq!(scr_sd_spec(&resp), 2);
        assert_eq!(scr_sd_bus_widths(&resp), 0x5);
        assert_ne!(scr_sd_bus_widths(&resp) & SCR_SD_BUS_WIDTHS_4BIT, 0);
    }

    #[test]
    fn the_switch_status_is_reversed_for_bitfield() {
        // Byte 13 of the big-endian status (bits 399:392) and byte 12 (407:400): group 1's
        // support bits 415:400 are bytes 12..13.
        let mut be = [0u8; 64];
        be[12] = 0x80;
        be[13] = 0x03; // high-speed and default supported
        let mut st = SdmmcBitfield512::default();
        for (w, b) in st._bits.iter_mut().zip(be.as_chunks::<4>().0) {
            *w = u32::from_ne_bytes(*b);
        }
        sdmmc_be512_to_bitfield512(&mut st);
        assert_eq!(sfunc_status_group(&st._bits, 1), 0x8003);
    }

    #[test]
    fn switch_arguments() {
        // Check mode, group 1, function 0: every other group "no change".
        assert_eq!(sdmmc_switch_arg(0, 1, 0), 0x00ff_fff0);
        // Switch mode, group 1, function 1 (high speed).
        assert_eq!(sdmmc_switch_arg(1, 1, 1), 0x80ff_fff1);
        assert_eq!(sdmmc_switch_arg(1, 2, 3), 0x80ff_ff3f);
    }

    #[test]
    fn the_timing_tables() {
        assert_eq!(SDMMC_MMC_TIMINGS, [26000, 52000, 0, 0, 52000, 200000]);
        assert_eq!(SWITCH_GROUP0_FUNCTIONS[1].freq, 50000);
        assert_eq!(SWITCH_GROUP0_FUNCTIONS[1].v, SMC_CAPS_SD_HIGHSPEED);
    }
}
/* </TESTS> */
