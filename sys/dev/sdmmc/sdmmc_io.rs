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
//! Routines for SD I/O cards: the SDIO power-up (CMD5), the functions' scan and setup, the
//! register accesses (CMD52 and CMD53), the attachment of the function drivers, and the
//! card interrupt handlers.
//!
//! Upstream: sys/dev/sdmmc/sdmmc_io.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C's 0/1 returns (`sdmmc_io_enable`, `sdmmc_io_init`, `sdmmc_io_set_highspeed`)
//!   are `Result<(), Errno>` with `EIO` for the 1; `sdmmc_io_function_ready` is a `bool`;
//!   `sdmmc_io_send_op_cond` returns the I/O OCR in its `Ok` (the C's `*ocrp`).
//! - The `*_multi_1` and `*_region_1` accesses take the data as a slice (the C's pointer
//!   and length); `sdmmc_io_rw_extended` and its `_subr` take a raw pointer and are
//!   `unsafe`. `sdmmc_io_rw_extended_subr` takes the function by reference: the C reads its
//!   `cur_blklen` without a NULL check, so every caller passes one.
//! - The 16- and 32-bit register accesses move the value through its native-endian bytes,
//!   as the C's `(u_char *)&data` does.
//! - `sdmmc_intr_establish` returns the handler as a `NonNull` (the C's `void *` cookie),
//!   `sdmmc_intr_disestablish` takes it back and is `unsafe`.
//! - `sdmmc_io_rw_direct` releases `sc_lock` when the card cannot be selected, as the C
//!   does, although its callers hold the lock and release it again (`External bugs` in the
//!   M16a notes); the port keeps the C's behaviour.
//! - `sdmmc_verbose` is a static atomic (0: `SDMMC_DEBUG` is not configured). `DPRINTF`s
//!   are comments.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::sdmmc::sdmmc::{
    sdmmc_add_task, sdmmc_delay, sdmmc_function_alloc, sdmmc_function_free, sdmmc_mmc_command,
    sdmmc_select_card, sdmmc_set_bus_power, sdmmc_set_relative_addr,
};
use crate::dev::sdmmc::sdmmc_cis::{sdmmc_check_cis_quirks, sdmmc_print_cis, sdmmc_read_cis};
use crate::dev::sdmmc::sdmmc_ioreg::{
    CCCR_BUS_WIDTH_1, CCCR_BUS_WIDTH_4, CCCR_BUS_WIDTH_MASK, CCCR_CTL_RES, CCCR_SPEED_EHS,
    CCCR_SPEED_MASK, CCCR_SPEED_SHS, SD_ARG_CMD52_DATA_MASK, SD_ARG_CMD52_DATA_SHIFT,
    SD_ARG_CMD52_EXCHANGE, SD_ARG_CMD52_FUNC_MASK, SD_ARG_CMD52_FUNC_SHIFT, SD_ARG_CMD52_READ,
    SD_ARG_CMD52_REG_MASK, SD_ARG_CMD52_REG_SHIFT, SD_ARG_CMD52_WRITE, SD_ARG_CMD53_BLOCK_MODE,
    SD_ARG_CMD53_FUNC_MASK, SD_ARG_CMD53_FUNC_SHIFT, SD_ARG_CMD53_INCREMENT,
    SD_ARG_CMD53_LENGTH_MASK, SD_ARG_CMD53_LENGTH_MAX, SD_ARG_CMD53_LENGTH_SHIFT,
    SD_ARG_CMD53_READ, SD_ARG_CMD53_REG_MASK, SD_ARG_CMD53_REG_SHIFT, SD_ARG_CMD53_WRITE,
    SD_IO_CCCR_BUS_WIDTH, SD_IO_CCCR_CTL, SD_IO_CCCR_FN_ENABLE, SD_IO_CCCR_FN_READY,
    SD_IO_CCCR_INT_ENABLE, SD_IO_CCCR_SPEED, SD_IO_FBR_BLOCKLEN, SD_IO_OCR_MASK,
    SD_IO_OCR_MEM_PRESENT, SD_IO_OCR_MEM_READY, SD_IO_RW_DIRECT, SD_IO_RW_EXTENDED,
    SD_IO_SEND_OP_COND, mmc_r4, sd_io_fbr_base, sd_io_ocr_num_functions, sd_r5_data,
};
use crate::dev::sdmmc::sdmmcchip::{
    SDMMC_SDCLK_25MHZ, SDMMC_SDCLK_50MHZ, SDMMC_TIMING_HIGHSPEED, SDMMC_TIMING_LEGACY,
    sdmmc_chip_bus_clock, sdmmc_chip_bus_width, sdmmc_chip_card_intr_ack,
    sdmmc_chip_card_intr_mask, sdmmc_chip_host_maxblklen, sdmmc_chip_host_ocr,
};
use crate::dev::sdmmc::sdmmcvar::{
    SCF_CMD_AC, SCF_CMD_BCR, SCF_CMD_READ, SCF_RSP_R4, SCF_RSP_R5, SDMMC_PRODUCT_INVALID,
    SDMMC_VENDOR_INVALID, SFF_ERROR, SMC_CAPS_4BIT_MODE, SMC_CAPS_DMA, SMC_CAPS_SD_HIGHSPEED,
    SMF_IO_MODE, SMF_MEM_MODE, SMF_SD_MODE, SdmmcAttachArgs, SdmmcCommand, SdmmcFunction,
    SdmmcSoftc, sdmmc_task_pending,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_exit};
use crate::kern::kern_synch::{nowake, tsleep_nsec};
use crate::kern::subr_autoconf::{config_detach, config_found_sm};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmamap, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamap_unload,
};
use crate::machine::intr::{splhigh, splx};
use crate::queue_adapter;
use crate::scsi::scsiconf::ScsibusAttachArgs;
use crate::sys::device::{CfMatch, DETACH_FORCE, Device, QUIET, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::param::PPAUSE;
use crate::sys::queue::TailqEntry;
use crate::sys::time::sec_to_nsec;

/// `struct sdmmc_intr_handler`: an SDIO card interrupt handler.
pub struct SdmmcIntrHandler {
    /// `ih_softc`.
    pub ih_softc: &'static SdmmcSoftc,
    /// `ih_name`.
    pub ih_name: &'static str,
    /// `ih_fun`.
    pub ih_fun: fn(*mut c_void) -> i32,
    /// `ih_arg`.
    pub ih_arg: *mut c_void,
    /// `entry`: the link in `sc_intrq`.
    pub entry: TailqEntry<SdmmcIntrHandler>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, sdmmc_intr_handler)`, through `entry`.
    pub SdmmcIntrHandlerList: SdmmcIntrHandler, entry => TailqEntry<SdmmcIntrHandler>
);

/// `sdmmc_verbose`: print each function's CIS (1 under `SDMMC_DEBUG`, not configured).
pub static SDMMC_VERBOSE: AtomicI32 = AtomicI32::new(0);

/// `sdmmc_io_enable`: initializes SD I/O card functions (before memory cards). The host
/// system and controller must support card interrupts in order to use I/O functions.
pub fn sdmmc_io_enable(sc: &'static SdmmcSoftc) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // Set host mode to SD "combo" card.
    sc.set_flag(SMF_SD_MODE | SMF_IO_MODE | SMF_MEM_MODE);

    // Reset I/O functions.
    sdmmc_io_reset(sc);

    // Read the I/O OCR value, determine the number of I/O functions and whether memory is
    // also present (a "combo card") by issuing CMD5. SD memory-only and MMC cards do not
    // respond to CMD5.
    let Ok(mut card_ocr) = sdmmc_io_send_op_cond(sc, 0) else {
        // No SDIO card; switch to SD memory-only mode.
        sc.clr_flag(SMF_IO_MODE);
        return Ok(());
    };

    // Parse the additional bits in the I/O OCR value.
    if card_ocr & SD_IO_OCR_MEM_PRESENT == 0 {
        // SDIO card without memory (not a "combo card").
        // DPRINTF(("%s: no memory present\n", DEVNAME(sc)));
        sc.clr_flag(SMF_MEM_MODE);
    }
    sc.sc_function_count.set(sd_io_ocr_num_functions(card_ocr));
    if sc.sc_function_count.get() == 0 {
        // Useless SDIO card without any I/O functions.
        // DPRINTF(("%s: no I/O functions\n", DEVNAME(sc)));
        sc.clr_flag(SMF_IO_MODE);
        return Ok(());
    }
    card_ocr &= SD_IO_OCR_MASK;

    // Set the lowest voltage supported by the card and host.
    let host_ocr = sdmmc_chip_host_ocr(sc.sct(), sc.sch.get());
    if sdmmc_set_bus_power(sc, host_ocr, card_ocr).is_err() {
        printf(format_args!(
            "{}: can't supply voltage requested by card\n",
            sc.devname()
        ));
        return Err(Errno::EIO);
    }

    // Send the new OCR value until all cards are ready.
    if sdmmc_io_send_op_cond(sc, host_ocr).is_err() {
        printf(format_args!("{}: can't send I/O OCR\n", sc.devname()));
        return Err(Errno::EIO);
    }
    Ok(())
}

/// `sdmmc_io_scan`: allocates `sdmmc_function` structures for SD card I/O function
/// (including function 0).
pub fn sdmmc_io_scan(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    let sf0 = sdmmc_function_alloc(sc);
    sf0.number.set(0);
    if sdmmc_set_relative_addr(sc, sf0).is_err() {
        printf(format_args!("{}: can't set I/O RCA\n", sc.devname()));
        // SAFETY: just allocated, on no list, not kept anywhere.
        unsafe { sdmmc_function_free(sf0) };
        return;
    }
    sc.sc_fn0.set(ptr::from_ref(sf0));
    // SAFETY: the function is on no list and lives until `sdmmc_card_detach` frees it after
    // taking the list apart.
    unsafe { sc.sf_head.insert_tail(sf0) };

    // Verify that the RCA has been set by selecting the card.
    if sdmmc_select_card(sc, Some(sf0)).is_err() {
        printf(format_args!(
            "{}: can't select I/O RCA {}\n",
            sc.devname(),
            sf0.rca.get()
        ));
        sf0.flags.set(sf0.flags.get() | SFF_ERROR);
        return;
    }

    let cookies = sc.sc_cookies.get();
    for i in 1..=sc.sc_function_count.get() {
        let sf = sdmmc_function_alloc(sc);
        sf.number.set(i);
        sf.rca.set(sf0.rca.get());
        sf.cookie
            .set(cookies.get(i as usize).copied().unwrap_or(ptr::null_mut()));

        // SAFETY: as for function 0.
        unsafe { sc.sf_head.insert_tail(sf) };
    }
}

/// `sdmmc_io_init`: initializes SDIO card functions.
pub fn sdmmc_io_init(sc: &'static SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let mut cis = sf.cis.get();
    let read = sdmmc_read_cis(sf, &mut cis);
    sf.cis.set(cis);
    if read.is_err() {
        printf(format_args!("{}: can't read CIS\n", sc.devname()));
        sf.flags.set(sf.flags.get() | SFF_ERROR);
        return Err(Errno::EIO);
    }

    sdmmc_check_cis_quirks(sf);

    if SDMMC_VERBOSE.load(Ordering::Relaxed) != 0 {
        sdmmc_print_cis(sf);
    }

    if sf.number.get() == 0 {
        if sc.has_caps(SMC_CAPS_SD_HIGHSPEED) && sdmmc_io_set_highspeed(sf, true).is_ok() {
            let _ = sdmmc_chip_bus_clock(
                sc.sct(),
                sc.sch.get(),
                SDMMC_SDCLK_50MHZ,
                SDMMC_TIMING_HIGHSPEED,
            );
            if sc.has_caps(SMC_CAPS_4BIT_MODE) {
                sdmmc_io_set_bus_width(sf, 4);
                let _ = sdmmc_chip_bus_width(sc.sct(), sc.sch.get(), 4);
            }
        } else {
            let _ = sdmmc_chip_bus_clock(
                sc.sct(),
                sc.sch.get(),
                SDMMC_SDCLK_25MHZ,
                SDMMC_TIMING_LEGACY,
            );
        }
    }

    Ok(())
}

/// `sdmmc_io_function_ready`: indicates whether the function is ready to operate.
pub fn sdmmc_io_function_ready(sf: &SdmmcFunction) -> bool {
    let sc = sf.sc;

    rw_assert_wrlock(&sc.sc_lock);

    if sf.number.get() == 0 {
        return true; // FN0 is always ready
    }

    let rv = sdmmc_io_read_1(sc.fn0(), SD_IO_CCCR_FN_READY);

    rv & (1 << sf.number.get()) != 0
}

/// `sdmmc_io_function_enable`: enables the I/O function; `Ok` if the function was enabled
/// successfully.
pub fn sdmmc_io_function_enable(sf: &SdmmcFunction) -> Result<(), Errno> {
    let sc = sf.sc;
    let mut retry = 5;

    rw_assert_wrlock(&sc.sc_lock);

    if sf.number.get() == 0 {
        return Ok(()); // FN0 is always enabled
    }

    let sf0 = sc.fn0();
    let mut rv = sdmmc_io_read_1(sf0, SD_IO_CCCR_FN_ENABLE);
    rv |= 1 << sf.number.get();
    sdmmc_io_write_1(sf0, SD_IO_CCCR_FN_ENABLE, rv);

    // while (!sdmmc_io_function_ready(sf) && retry-- > 0) tsleep_nsec(...);
    while !sdmmc_io_function_ready(sf) {
        let r = retry;
        retry -= 1;
        if r <= 0 {
            break;
        }
        let _ = tsleep_nsec(nowake(), PPAUSE, "pause", sec_to_nsec(1));
    }
    if retry >= 0 {
        Ok(())
    } else {
        Err(Errno::ETIMEDOUT)
    }
}

/// `sdmmc_io_function_disable`: disables the I/O function.
pub fn sdmmc_io_function_disable(sf: &SdmmcFunction) {
    let sc = sf.sc;

    rw_assert_wrlock(&sc.sc_lock);

    if sf.number.get() == 0 {
        return; // FN0 is always enabled
    }

    let sf0 = sc.fn0();
    let mut rv = sdmmc_io_read_1(sf0, SD_IO_CCCR_FN_ENABLE);
    rv &= !(1 << sf.number.get());
    sdmmc_io_write_1(sf0, SD_IO_CCCR_FN_ENABLE, rv);
}

/// `sdmmc_io_attach`: attaches the drivers of the I/O functions.
pub fn sdmmc_io_attach(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    for sf in sc.sf_head.iter() {
        if sf.number.get() < 1 {
            continue;
        }

        let mut saa = SdmmcAttachArgs {
            saa: ScsibusAttachArgs::new(),
            sf: Some(sf),
        };

        sf.child.set(config_found_sm(
            &sc.sc_dev,
            ptr::from_mut(&mut saa).cast(),
            Some(sdmmc_print),
            Some(sdmmc_submatch),
        ));
    }
}

/// `sdmmc_submatch`: skips the scsibus, it is configured directly.
pub fn sdmmc_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();

    // Skip the scsibus, it is configured directly.
    if cf.cf_driver.cd_name == b"scsibus" {
        return 0;
    }

    cf.cf_attach
        .ca_match
        .map_or(0, |ca_match| ca_match(parent, match_, aux))
}

/// `sdmmc_print`: the attach line of an I/O function's driver, or of one not configured.
pub fn sdmmc_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `sdmmc_io_attach` passes its `sdmmc_attach_args`, which name the function.
    let sa = unsafe { &*aux.cast::<SdmmcAttachArgs>().cast_const() };
    let Some(sf) = sa.sf else {
        panic(format_args!("sdmmc_print: no function"));
    };
    let cis = sf.sc.fn0().cis.get();

    if let Some(pnp) = pnp {
        if sf.number.get() == 0 {
            return QUIET;
        }

        let mut i = 0;
        while i < 4 {
            let Some(info) = cis.info(i) else {
                break;
            };
            printf(format_args!(
                "{}{}",
                if i != 0 { ", " } else { "\"" },
                Str(info)
            ));
            i += 1;
        }
        if i != 0 {
            printf(format_args!("\""));
        }

        if cis.manufacturer != SDMMC_VENDOR_INVALID || cis.product != SDMMC_PRODUCT_INVALID {
            printf(format_args!("{}", if i != 0 { " " } else { "" }));
            if cis.manufacturer != SDMMC_VENDOR_INVALID {
                printf(format_args!(
                    "manufacturer 0x{:04x}{}",
                    cis.manufacturer,
                    if cis.product == SDMMC_PRODUCT_INVALID {
                        ""
                    } else {
                        ", "
                    }
                ));
            }
            if cis.product != SDMMC_PRODUCT_INVALID {
                printf(format_args!("product 0x{:04x}", cis.product));
            }
        }
        printf(format_args!(" at {}", Str(pnp)));
    }
    printf(format_args!(" function {}", sf.number.get()));

    if pnp.is_none() {
        let mut i = 0;
        while i < 3 {
            let Some(info) = cis.info(i) else {
                break;
            };
            printf(format_args!(
                "{}{}",
                if i != 0 { ", " } else { " \"" },
                Str(info)
            ));
            i += 1;
        }
        if i != 0 {
            printf(format_args!("\""));
        }
    }
    UNCONF
}

/// `sdmmc_io_detach`: detaches the drivers of the I/O functions.
pub fn sdmmc_io_detach(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    for sf in sc.sf_head.iter() {
        if let Some(child) = sf.child.get() {
            // SAFETY: the child `sdmmc_io_attach` attached, still attached.
            let _ = unsafe { config_detach(child, DETACH_FORCE) };
            sf.child.set(None);
        }
    }

    kassert!(sc.sc_intrq.is_empty());
}

/// `sdmmc_io_rw_direct`: CMD52, one register byte of `sf` (function 0 for `None`) read
/// into or written from `*datap`.
pub fn sdmmc_io_rw_direct(
    sc: &SdmmcSoftc,
    sf: Option<&SdmmcFunction>,
    reg: i32,
    datap: &mut u8,
    arg: u32,
) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // Make sure the card is selected.
    if let Err(error) = sdmmc_select_card(sc, sf) {
        rw_exit(&sc.sc_lock);
        return Err(error);
    }

    let arg = sdmmc_io_cmd52_arg(sf.map_or(0, |sf| sf.number.get()), reg, *datap, arg);

    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = SD_IO_RW_DIRECT;
    cmd.c_arg = arg;
    cmd.c_flags = SCF_CMD_AC | SCF_RSP_R5;

    let error = sdmmc_mmc_command(sc, &mut cmd);
    *datap = sd_r5_data(&cmd.c_resp);

    error
}

/// The CMD52 argument: `arg` (read, write, exchange) with the function, register and data
/// fields.
pub const fn sdmmc_io_cmd52_arg(number: i32, reg: i32, data: u8, arg: u32) -> u32 {
    let mut arg = arg;
    arg |= (number as u32 & SD_ARG_CMD52_FUNC_MASK) << SD_ARG_CMD52_FUNC_SHIFT;
    arg |= (reg as u32 & SD_ARG_CMD52_REG_MASK) << SD_ARG_CMD52_REG_SHIFT;
    arg |= (data as u32 & SD_ARG_CMD52_DATA_MASK) << SD_ARG_CMD52_DATA_SHIFT;
    arg
}

/// The CMD53 argument: `arg` with the function, register and length fields.
pub const fn sdmmc_io_cmd53_arg(number: i32, reg: i32, len: i32, arg: u32) -> u32 {
    let mut arg = arg;
    arg |= (number as u32 & SD_ARG_CMD53_FUNC_MASK) << SD_ARG_CMD53_FUNC_SHIFT;
    arg |= (reg as u32 & SD_ARG_CMD53_REG_MASK) << SD_ARG_CMD53_REG_SHIFT;
    arg |= (len as u32 & SD_ARG_CMD53_LENGTH_MASK) << SD_ARG_CMD53_LENGTH_SHIFT;
    arg
}

/// `sdmmc_io_rw_extended_subr`: CMD53. Useful values of `arg` to pass in are either
/// `SD_ARG_CMD53_READ` or `SD_ARG_CMD53_WRITE`. `SD_ARG_CMD53_INCREMENT` may be ORed into
/// `arg` to access successive register locations instead of accessing the same register
/// many times. `SD_ARG_CMD53_BLOCK_MODE` may be ORed into `arg` to indicate that the length
/// is a number of blocks.
///
/// # Safety
///
/// `datap` points to the transfer's bytes (`len` bytes, or `len` blocks of
/// `cur_blklen`), writable for a read, that nothing else uses until the call returns;
/// `dmap`, when given, has them loaded.
pub unsafe fn sdmmc_io_rw_extended_subr(
    sc: &SdmmcSoftc,
    sf: &SdmmcFunction,
    dmap: Option<&'static BusDmamap>,
    reg: i32,
    datap: *mut u8,
    len: i32,
    arg: u32,
) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // (#if 0 in the C: make sure the card is selected.)

    let arg = sdmmc_io_cmd53_arg(sf.number.get(), reg, len, arg);

    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = SD_IO_RW_EXTENDED;
    cmd.c_arg = arg;
    cmd.c_flags = SCF_CMD_AC | SCF_RSP_R5;
    cmd.c_dmamap = dmap;
    cmd.c_data = datap;
    let blklen = sf.cur_blklen.get() as i32;
    if arg & SD_ARG_CMD53_BLOCK_MODE != 0 {
        cmd.c_datalen = len * blklen;
        cmd.c_blklen = blklen;
    } else {
        cmd.c_datalen = len;
        cmd.c_blklen = len.min(blklen);
    }

    if arg & SD_ARG_CMD53_WRITE == 0 {
        cmd.c_flags |= SCF_CMD_READ;
    }

    sdmmc_mmc_command(sc, &mut cmd)
}

/// `sdmmc_io_rw_extended`: CMD53, through the slot's DMA map when the host does DMA.
///
/// # Safety
///
/// As for [`sdmmc_io_rw_extended_subr`].
pub unsafe fn sdmmc_io_rw_extended(
    sc: &SdmmcSoftc,
    sf: &SdmmcFunction,
    reg: i32,
    datap: *mut u8,
    len: i32,
    arg: u32,
) -> Result<(), Errno> {
    let mut datalen = len;

    if !sc.has_caps(SMC_CAPS_DMA) {
        // SAFETY: the caller's guarantee.
        return unsafe { sdmmc_io_rw_extended_subr(sc, sf, None, reg, datap, len, arg) };
    }

    if arg & SD_ARG_CMD53_BLOCK_MODE != 0 {
        datalen = len * sf.cur_blklen.get() as i32;
    }

    let read = arg & SD_ARG_CMD53_WRITE == 0;

    let dmap = sc.dmap();
    // SAFETY: the caller's guarantee: the buffer stays valid until it is unloaded below.
    unsafe {
        bus_dmamap_load(
            sc.dmat(),
            dmap,
            datap,
            datalen as usize,
            None,
            BUS_DMA_NOWAIT | if read { BUS_DMA_READ } else { BUS_DMA_WRITE },
        )?
    };

    bus_dmamap_sync(
        sc.dmat(),
        dmap,
        0,
        datalen as usize,
        if read {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    // SAFETY: the caller's guarantee; the buffer is loaded into `dmap`.
    let error = unsafe { sdmmc_io_rw_extended_subr(sc, sf, Some(dmap), reg, datap, len, arg) };
    if error.is_ok() {
        bus_dmamap_sync(
            sc.dmat(),
            dmap,
            0,
            datalen as usize,
            if read {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
    }

    // unload:
    bus_dmamap_unload(sc.dmat(), dmap);

    // out:
    error
}

/// `sdmmc_io_read_1`.
pub fn sdmmc_io_read_1(sf: &SdmmcFunction, reg: i32) -> u8 {
    let mut data: u8 = 0;

    rw_assert_wrlock(&sf.sc.sc_lock);

    let _ = sdmmc_io_rw_direct(sf.sc, Some(sf), reg, &mut data, SD_ARG_CMD52_READ);
    data
}

/// `sdmmc_io_write_1`.
pub fn sdmmc_io_write_1(sf: &SdmmcFunction, reg: i32, data: u8) {
    let mut data = data;

    rw_assert_wrlock(&sf.sc.sc_lock);

    let _ = sdmmc_io_rw_direct(sf.sc, Some(sf), reg, &mut data, SD_ARG_CMD52_WRITE);
}

/// `sdmmc_io_read_2`.
pub fn sdmmc_io_read_2(sf: &SdmmcFunction, reg: i32) -> u16 {
    let mut data = [0u8; 2];

    rw_assert_wrlock(&sf.sc.sc_lock);

    // SAFETY: two writable bytes on this frame.
    let _ = unsafe {
        sdmmc_io_rw_extended_subr(
            sf.sc,
            sf,
            None,
            reg,
            data.as_mut_ptr(),
            2,
            SD_ARG_CMD53_READ | SD_ARG_CMD53_INCREMENT,
        )
    };
    u16::from_ne_bytes(data)
}

/// `sdmmc_io_write_2`.
pub fn sdmmc_io_write_2(sf: &SdmmcFunction, reg: i32, data: u16) {
    let mut data = data.to_ne_bytes();

    rw_assert_wrlock(&sf.sc.sc_lock);

    // SAFETY: two bytes on this frame.
    let _ = unsafe {
        sdmmc_io_rw_extended_subr(
            sf.sc,
            sf,
            None,
            reg,
            data.as_mut_ptr(),
            2,
            SD_ARG_CMD53_WRITE | SD_ARG_CMD53_INCREMENT,
        )
    };
}

/// `sdmmc_io_read_4`.
pub fn sdmmc_io_read_4(sf: &SdmmcFunction, reg: i32) -> u32 {
    let mut data = [0u8; 4];

    rw_assert_wrlock(&sf.sc.sc_lock);

    // SAFETY: four writable bytes on this frame.
    let _ = unsafe {
        sdmmc_io_rw_extended_subr(
            sf.sc,
            sf,
            None,
            reg,
            data.as_mut_ptr(),
            4,
            SD_ARG_CMD53_READ | SD_ARG_CMD53_INCREMENT,
        )
    };
    u32::from_ne_bytes(data)
}

/// `sdmmc_io_write_4`.
pub fn sdmmc_io_write_4(sf: &SdmmcFunction, reg: i32, data: u32) {
    let mut data = data.to_ne_bytes();

    rw_assert_wrlock(&sf.sc.sc_lock);

    // SAFETY: four bytes on this frame.
    let _ = unsafe {
        sdmmc_io_rw_extended_subr(
            sf.sc,
            sf,
            None,
            reg,
            data.as_mut_ptr(),
            4,
            SD_ARG_CMD53_WRITE | SD_ARG_CMD53_INCREMENT,
        )
    };
}

/// The block loop of the `*_multi_1` and `*_region_1` accesses: whole blocks first (at
/// most `SD_ARG_CMD53_LENGTH_MAX` per command), then the rest in byte mode; `increment`
/// advances the register with the data.
fn sdmmc_io_rw_blocks(
    sf: &SdmmcFunction,
    reg: i32,
    data: &mut [u8],
    dir: u32,
    increment: bool,
) -> Result<(), Errno> {
    let blklen = sf.cur_blklen.get() as i32;
    let inc = if increment { SD_ARG_CMD53_INCREMENT } else { 0 };
    let mut reg = reg;
    let mut off = 0usize;
    let mut datalen = data.len() as i32;

    rw_assert_wrlock(&sf.sc.sc_lock);

    while datalen >= blklen {
        let blocks = (datalen / blklen).min(SD_ARG_CMD53_LENGTH_MAX);
        // SAFETY: `blocks * blklen` bytes from `off` lie inside `data`, which this call
        // borrows mutably.
        unsafe {
            sdmmc_io_rw_extended(
                sf.sc,
                sf,
                reg,
                data.as_mut_ptr().add(off),
                blocks,
                dir | inc | SD_ARG_CMD53_BLOCK_MODE,
            )?
        };
        if increment {
            reg += blocks * blklen;
        }
        off += (blocks * blklen) as usize;
        datalen -= blocks * blklen;
    }

    if datalen != 0 {
        // SAFETY: the last `datalen` bytes of `data`.
        unsafe {
            sdmmc_io_rw_extended(
                sf.sc,
                sf,
                reg,
                data.as_mut_ptr().add(off),
                datalen,
                dir | inc,
            )
        }
    } else {
        Ok(())
    }
}

/// `sdmmc_io_read_multi_1`: reads `data.len()` bytes from one register.
pub fn sdmmc_io_read_multi_1(sf: &SdmmcFunction, reg: i32, data: &mut [u8]) -> Result<(), Errno> {
    sdmmc_io_rw_blocks(sf, reg, data, SD_ARG_CMD53_READ, false)
}

/// `sdmmc_io_write_multi_1`: writes `data` to one register.
pub fn sdmmc_io_write_multi_1(sf: &SdmmcFunction, reg: i32, data: &mut [u8]) -> Result<(), Errno> {
    sdmmc_io_rw_blocks(sf, reg, data, SD_ARG_CMD53_WRITE, false)
}

/// `sdmmc_io_read_region_1`: reads `data.len()` bytes from successive registers.
pub fn sdmmc_io_read_region_1(sf: &SdmmcFunction, reg: i32, data: &mut [u8]) -> Result<(), Errno> {
    sdmmc_io_rw_blocks(sf, reg, data, SD_ARG_CMD53_READ, true)
}

/// `sdmmc_io_write_region_1`: writes `data` to successive registers.
pub fn sdmmc_io_write_region_1(sf: &SdmmcFunction, reg: i32, data: &mut [u8]) -> Result<(), Errno> {
    sdmmc_io_rw_blocks(sf, reg, data, SD_ARG_CMD53_WRITE, true)
}

/// `sdmmc_io_xchg`: CMD52 read-after-write.
pub fn sdmmc_io_xchg(
    sc: &SdmmcSoftc,
    sf: Option<&SdmmcFunction>,
    reg: i32,
    datap: &mut u8,
) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    sdmmc_io_rw_direct(
        sc,
        sf,
        reg,
        datap,
        SD_ARG_CMD52_WRITE | SD_ARG_CMD52_EXCHANGE,
    )
}

/// `sdmmc_io_reset`: resets the I/O functions of the card.
pub fn sdmmc_io_reset(sc: &SdmmcSoftc) {
    let mut data = CCCR_CTL_RES;

    rw_assert_wrlock(&sc.sc_lock);

    if sdmmc_io_rw_direct(sc, None, SD_IO_CCCR_CTL, &mut data, SD_ARG_CMD52_WRITE).is_ok() {
        sdmmc_delay(100000);
    }
}

/// `sdmmc_io_send_op_cond`: gets or sets the card's I/O OCR value (SDIO); the OCR of the
/// last response on success.
pub fn sdmmc_io_send_op_cond(sc: &SdmmcSoftc, ocr: u32) -> Result<u32, Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // If we change the OCR value, retry the command until the OCR we receive in response
    // has the "CARD BUSY" bit set, meaning that all cards are ready for identification.
    let mut cmd = SdmmcCommand::new();
    let mut error: Result<(), Errno> = Ok(());
    for _ in 0..100 {
        cmd = SdmmcCommand::new();
        cmd.c_opcode = SD_IO_SEND_OP_COND;
        cmd.c_arg = ocr;
        cmd.c_flags = SCF_CMD_BCR | SCF_RSP_R4;

        error = sdmmc_mmc_command(sc, &mut cmd);
        if error.is_err() {
            break;
        }
        if mmc_r4(&cmd.c_resp) & SD_IO_OCR_MEM_READY != 0 || ocr == 0 {
            break;
        }
        error = Err(Errno::ETIMEDOUT);
        sdmmc_delay(10000);
    }
    error.map(|()| mmc_r4(&cmd.c_resp))
}

/// `sdmmc_intr_enable`: enables the function's card interrupt.
pub fn sdmmc_intr_enable(sf: &SdmmcFunction) {
    let sc = sf.sc;

    rw_assert_wrlock(&sc.sc_lock);

    let sf0 = sc.fn0();
    let mut imask = sdmmc_io_read_1(sf0, SD_IO_CCCR_INT_ENABLE);
    imask |= 1 << sf.number.get();
    sdmmc_io_write_1(sf0, SD_IO_CCCR_INT_ENABLE, imask);
}

/// `sdmmc_intr_disable`: disables the function's card interrupt.
pub fn sdmmc_intr_disable(sf: &SdmmcFunction) {
    let sc = sf.sc;

    rw_assert_wrlock(&sc.sc_lock);

    let sf0 = sc.fn0();
    let mut imask = sdmmc_io_read_1(sf0, SD_IO_CCCR_INT_ENABLE);
    imask &= !(1 << sf.number.get());
    sdmmc_io_write_1(sf0, SD_IO_CCCR_INT_ENABLE, imask);
}

/// `(struct sdmmc_softc *)sdmmc`.
fn sdmmc_io_softc(sdmmc: &Device) -> &'static SdmmcSoftc {
    // SAFETY: the callers pass the sdmmc device itself (a function driver's parent, or the
    // host controller's `hp->sdmmc`), whose softc is an `SdmmcSoftc`; softcs are never
    // freed while the device exists.
    unsafe { &*ptr::from_ref(sdmmc.softc::<SdmmcSoftc>()) }
}

/// `sdmmc_intr_establish`: establishes a handler for the SDIO card interrupt. Because the
/// interrupt may be shared with different SDIO functions, multiple handlers can be
/// established.
pub fn sdmmc_intr_establish(
    sdmmc: &Device,
    fun: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<SdmmcIntrHandler>> {
    let sc = sdmmc_io_softc(sdmmc);

    sc.sct().card_intr_mask?;

    let ih = malloc(
        size_of::<SdmmcIntrHandler>(),
        M_DEVBUF,
        M_WAITOK | M_CANFAIL | M_ZERO,
    )?
    .cast::<SdmmcIntrHandler>();

    // SAFETY: a fresh allocation of the handler's size, written whole before it is linked.
    let ihr = unsafe {
        ih.as_ptr().write(SdmmcIntrHandler {
            ih_softc: sc,
            ih_name: name,
            ih_fun: fun,
            ih_arg: arg,
            entry: TailqEntry::new(),
        });
        ih.as_ref()
    };

    let s = splhigh();
    if sc.sc_intrq.is_empty() {
        sdmmc_intr_enable(sc.fn0());
        sdmmc_chip_card_intr_mask(sc.sct(), sc.sch.get(), 1);
    }
    // SAFETY: the handler is on no queue and lives until `sdmmc_intr_disestablish` unlinks
    // and frees it.
    unsafe { sc.sc_intrq.insert_tail(ihr) };
    splx(s);
    Some(ih)
}

/// `sdmmc_intr_disestablish`: disestablishes the given handler.
///
/// # Safety
///
/// `cookie` came from [`sdmmc_intr_establish`] and is not used afterwards.
pub unsafe fn sdmmc_intr_disestablish(cookie: NonNull<SdmmcIntrHandler>) {
    // SAFETY: the caller's guarantee: an established handler.
    let ih = unsafe { cookie.as_ref() };
    let sc = ih.ih_softc;

    if sc.sct().card_intr_mask.is_none() {
        return;
    }

    let s = splhigh();
    // SAFETY: the handler is on `sc_intrq` (established).
    unsafe { sc.sc_intrq.remove(ih) };
    if sc.sc_intrq.is_empty() {
        sdmmc_chip_card_intr_mask(sc.sct(), sc.sch.get(), 0);
        sdmmc_intr_disable(sc.fn0());
    }
    splx(s);

    free(cookie.cast(), M_DEVBUF, size_of::<SdmmcIntrHandler>());
}

/// `sdmmc_card_intr`: queues the call of the established SDIO card interrupt handlers.
/// The host controller must call this function from its own interrupt handler to handle an
/// SDIO interrupt from the card.
pub fn sdmmc_card_intr(sdmmc: &Device) {
    let sc = sdmmc_io_softc(sdmmc);

    if sc.sct().card_intr_mask.is_none() {
        return;
    }

    if !sdmmc_task_pending(&sc.sc_intr_task) {
        sdmmc_add_task(sc, &sc.sc_intr_task);
    }
}

/// `sdmmc_intr_task`: calls the established handlers, then acknowledges the interrupt.
pub fn sdmmc_intr_task(arg: *mut c_void) {
    if arg.is_null() {
        panic(format_args!("sdmmc_intr_task: no softc"));
    }
    // SAFETY: `sdmmc_attach` gives the task its softc, never freed while the device exists.
    let sc = unsafe { &*arg.cast::<SdmmcSoftc>().cast_const() };

    let mut s = splhigh();
    for ih in sc.sc_intrq.iter() {
        splx(s);

        // XXX examine return value and do evcount stuff
        let _ = (ih.ih_fun)(ih.ih_arg);

        s = splhigh();
    }
    sdmmc_chip_card_intr_ack(sc.sct(), sc.sch.get());
    splx(s);
}

/// `sdmmc_io_set_blocklen`: sets the function's block length (`0` for the default).
pub fn sdmmc_io_set_blocklen(sf: &SdmmcFunction, blklen: u32) {
    let sc = sf.sc;
    let mut blklen = blklen;

    rw_assert_wrlock(&sc.sc_lock);

    let maxblklen = sdmmc_chip_host_maxblklen(sc.sct(), sc.sch.get());
    if blklen > maxblklen as u32 {
        return;
    }

    if blklen == 0 {
        blklen = 512.min(maxblklen) as u32;
    }

    let sf0 = sc.fn0();
    let fbr = sd_io_fbr_base(sf.number.get()) + SD_IO_FBR_BLOCKLEN;
    sdmmc_io_write_1(sf0, fbr, (blklen & 0xff) as u8);
    sdmmc_io_write_1(sf0, fbr + 1, ((blklen >> 8) & 0xff) as u8);
    sf.cur_blklen.set(blklen);
}

/// `sdmmc_io_set_bus_width`: the card side of a bus width change (CCCR).
pub fn sdmmc_io_set_bus_width(sf: &SdmmcFunction, width: i32) {
    rw_assert_wrlock(&sf.sc.sc_lock);
    let mut rv = sdmmc_io_read_1(sf, SD_IO_CCCR_BUS_WIDTH);
    rv &= !CCCR_BUS_WIDTH_MASK;
    if width == 4 {
        rv |= CCCR_BUS_WIDTH_4;
    } else {
        rv |= CCCR_BUS_WIDTH_1;
    }
    sdmmc_io_write_1(sf, SD_IO_CCCR_BUS_WIDTH, rv);
}

/// `sdmmc_io_set_highspeed`: enables or disables high speed; fails when the card does not
/// support it.
pub fn sdmmc_io_set_highspeed(sf: &SdmmcFunction, enable: bool) -> Result<(), Errno> {
    rw_assert_wrlock(&sf.sc.sc_lock);

    let mut rv = sdmmc_io_read_1(sf, SD_IO_CCCR_SPEED);
    if enable && rv & CCCR_SPEED_SHS == 0 {
        return Err(Errno::EIO);
    }
    rv &= !CCCR_SPEED_MASK;
    if enable {
        rv |= CCCR_SPEED_EHS;
    }
    sdmmc_io_write_1(sf, SD_IO_CCCR_SPEED, rv);
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd52_and_cmd53_arguments() {
        // Write 0x08 to CCCR 0x06 of function 0 (sdmmc_io_reset).
        assert_eq!(
            sdmmc_io_cmd52_arg(0, SD_IO_CCCR_CTL, CCCR_CTL_RES, SD_ARG_CMD52_WRITE),
            0x8000_0c08
        );
        // Read register 0x1ffff of function 7: the fields are masked.
        assert_eq!(
            sdmmc_io_cmd52_arg(7, 0x3ffff, 0, SD_ARG_CMD52_READ),
            0x7000_0000 | (0x1ffff << 9)
        );
        // A 4-byte incrementing read of register 0x10 of function 1.
        assert_eq!(
            sdmmc_io_cmd53_arg(1, 0x10, 4, SD_ARG_CMD53_READ | SD_ARG_CMD53_INCREMENT),
            0x1400_2004
        );
        // 512 blocks wrap to 0 in the 9-bit count, as in the C.
        assert_eq!(
            sdmmc_io_cmd53_arg(1, 0, 512, 0) & SD_ARG_CMD53_LENGTH_MASK,
            0
        );
    }
}
/* </TESTS> */
