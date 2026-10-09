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
 * Copyright (c) 1998, 2001 Manuel Bouyer.  All rights reserved.
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
 */
/* </LICENSES> */

/* <CODE> */
//! The ATA commands common to the drive drivers of a wdc(4) channel: IDENTIFY (DEVICE or
//! PACKET DEVICE) into a `struct ataparams` with its strings put in order, SET FEATURES
//! transfer mode, the DMA error accounting that decides a mode downgrade, and the error
//! register as text.
//!
//! Upstream: sys/dev/ata/ata.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `dma_alloc(9)` is not ported: the IDENTIFY buffer is `scsiconf`'s `DmaBuf`
//!   (`malloc(9)`), freed when it goes out of scope (`dma_free`).
//! - `ata_get_params` and `ata_set_mode` give their command to `wdc_exec_command` from their
//!   own frame, so `flags` must hold `AT_WAIT` or `AT_POLL` (every caller passes `at_poll`,
//!   `AT_WAIT` or `AT_POLL`); without either they panic, where the C would leave the
//!   channel's transfer pointing into a dead frame.
//! - The string fixing of `ata_get_params` is [`ata_params_fixup`]; the big-endian word
//!   swaps (`swap16_multi`) are compiled for `target_endian = "big"`, as the C's
//!   `BYTE_ORDER == BIG_ENDIAN`.
//! - `ata_perror` writes its NUL-terminated message into a byte buffer with `snprintf`, as
//!   the C; the message strings are `&str`.

use crate::dev::ata::atareg::{ATAPARAMS_LEN, Ataparams, WDC_CFG_ATAPI, WDC_CFG_ATAPI_MASK};
use crate::dev::ata::atavar::{
    AT_DF, AT_ERROR, AT_POLL, AT_READ, AT_TIMEOU, AT_WAIT, AtaDriveDatas, CMD_AGAIN, CMD_ERR,
    CMD_OK, DRIVE_ATA, DRIVE_ATAPI, NERRS_MAX, NXFER, WDC_COMPLETE, WdcCommand,
};
#[cfg(feature = "wdcdebug")]
use crate::dev::ic::wdc::wdcdebug_mask;
use crate::dev::ic::wdc::wdcdebug_print;
use crate::dev::ic::wdc::{DEBUG_FUNCS, DEBUG_PROBE, wdc_downgrade_mode, wdc_exec_command};
use crate::dev::ic::wdcreg::{
    ATAPI_IDENTIFY_DEVICE, SET_FEATURES, WDCC_IDENTIFY, WDCS_DRDY, WDSF_SET_MODE,
};
use crate::kern::subr_prf::{panic, snprintf};
use crate::scsi::scsiconf::DmaBuf;
use crate::sys::malloc::M_NOWAIT;

/// `ATAPARAMS_SIZE`.
const ATAPARAMS_SIZE: usize = 512;

/// The messages of the error register's bits, before ATA-4.
static ERRSTR0_3: [&str; 8] = [
    "address mark not found",
    "track 0 not found",
    "aborted command",
    "media change requested",
    "id not found",
    "media changed",
    "uncorrectable data error",
    "bad block detected",
];

/// The messages of the error register's bits, ATA-4 and later.
static ERRSTR4_5: [&str; 8] = [
    "",
    "no media/write protected",
    "aborted command",
    "media change requested",
    "id not found",
    "media changed",
    "uncorrectable data error",
    "interface CRC error",
];

/// The `wdc_exec_command` of a command on this frame: `flags` must make it complete before
/// it returns (see the module's deviations).
pub(crate) fn exec_on_stack(drvp: &AtaDriveDatas, wdc_c: &WdcCommand, func: &str) -> i32 {
    if !wdc_c.isset(AT_WAIT | AT_POLL) {
        panic(format_args!(
            "{}: command neither waited for nor polled",
            func
        ));
    }
    // SAFETY: `AT_WAIT` or `AT_POLL` (checked above): the command is done, or was never
    // queued, when `wdc_exec_command` returns, before `wdc_c` goes out of scope.
    unsafe { wdc_exec_command(drvp, wdc_c) }
}

/// `ata_get_params`: gets the disk's parameters (IDENTIFY) into `prms`. Returns `CMD_OK`,
/// `CMD_ERR` (the drive failed the command) or `CMD_AGAIN`.
pub fn ata_get_params(drvp: &AtaDriveDatas, flags: u8, prms: &mut Ataparams) -> i32 {
    wdcdebug_print!(wdcdebug_mask, DEBUG_FUNCS, "ata_get_params\n");

    let wdc_c = WdcCommand::new();

    if drvp.isset(DRIVE_ATA) {
        wdc_c.r_command.set(WDCC_IDENTIFY);
        wdc_c.r_st_bmask.set(WDCS_DRDY);
        wdc_c.r_st_pmask.set(0);
        wdc_c.timeout.set(3000); // 3s
    } else if drvp.isset(DRIVE_ATAPI) {
        wdc_c.r_command.set(ATAPI_IDENTIFY_DEVICE);
        wdc_c.r_st_bmask.set(0);
        wdc_c.r_st_pmask.set(0);
        wdc_c.timeout.set(10000); // 10s
    } else {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_FUNCS | DEBUG_PROBE,
            "ata_get_params: no disks\n"
        );
        return CMD_ERR;
    }

    let Some(mut tb) = DmaBuf::new(ATAPARAMS_SIZE, M_NOWAIT) else {
        return CMD_AGAIN;
    };
    wdc_c.flags.set(AT_READ | u16::from(flags));
    wdc_c.data.set(tb.bytes().as_mut_ptr());
    wdc_c.bcount.set(ATAPARAMS_SIZE as i32);

    let ret = exec_on_stack(drvp, &wdc_c, "ata_get_params");
    if ret != WDC_COMPLETE {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "ata_get_params: wdc_exec_command failed: {}\n",
            ret
        );
        return CMD_AGAIN;
    }

    if wdc_c.isset(AT_ERROR | AT_TIMEOU | AT_DF) {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "ata_get_params: IDENTIFY failed: 0x{:x}\n",
            wdc_c.flags.get()
        );

        return CMD_ERR;
    }

    let tb = tb.bytes();
    #[cfg(target_endian = "big")]
    {
        // All the fields in the params structure are 16-bit integers except for the ID
        // strings which are char strings. The 16-bit integers are currently in memory in
        // little-endian, regardless of architecture. So, they need to be swapped on
        // big-endian architectures before they are accessed through the ataparams
        // structure.
        //
        // The swaps below avoid touching the char strings.
        for w in (0..10).chain(20..23).chain(47..ATAPARAMS_SIZE / 2) {
            tb.swap(2 * w, 2 * w + 1);
        }
    }
    // Read in parameter block.
    let Ok(block) = <&[u8; ATAPARAMS_LEN]>::try_from(&*tb) else {
        return CMD_ERR;
    };
    *prms = Ataparams::from_bytes(block);

    ata_params_fixup(prms);
    CMD_OK
}

/// The string fixing of `ata_get_params`: the model, serial and revision come as
/// big-endian words; shuffle their byte order (ATAPI Mitsumi and NEC drives don't need
/// this).
pub fn ata_params_fixup(prms: &mut Ataparams) {
    // Shuffle string byte order. ATAPI Mitsumi and NEC drives don't need this.
    if prms.atap_config & WDC_CFG_ATAPI_MASK == WDC_CFG_ATAPI
        && (prms.atap_model.starts_with(b"NE") || prms.atap_model.starts_with(b"FX"))
    {
        return;
    }
    for p in prms.atap_model.as_chunks_mut::<2>().0 {
        p.swap(0, 1);
    }
    for p in prms.atap_serial.as_chunks_mut::<2>().0 {
        p.swap(0, 1);
    }
    for p in prms.atap_revision.as_chunks_mut::<2>().0 {
        p.swap(0, 1);
    }
}

/// `ata_set_mode`: SET FEATURES transfer mode `mode`. Returns `CMD_OK`, `CMD_ERR` or
/// `CMD_AGAIN`.
pub fn ata_set_mode(drvp: &AtaDriveDatas, mode: u8, flags: u8) -> i32 {
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "ata_set_mode: mode=0x{:x}, flags=0x{:x}\n",
        mode,
        flags
    );

    let wdc_c = WdcCommand::new();

    wdc_c.r_command.set(SET_FEATURES);
    wdc_c.r_st_bmask.set(0);
    wdc_c.r_st_pmask.set(0);
    wdc_c.r_features.set(WDSF_SET_MODE);
    wdc_c.r_count.set(mode);
    wdc_c.flags.set(u16::from(flags));
    wdc_c.timeout.set(1000); // 1s
    if exec_on_stack(drvp, &wdc_c, "ata_set_mode") != WDC_COMPLETE {
        return CMD_AGAIN;
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "ata_set_mode: after wdc_exec_command() wdc_c.flags=0x{:x}\n",
        wdc_c.flags.get()
    );

    if wdc_c.isset(AT_ERROR | AT_TIMEOU | AT_DF) {
        return CMD_ERR;
    }
    CMD_OK
}

/// `ata_dmaerr`: counts a DMA error and downgrades the drive's mode when there are too many.
pub fn ata_dmaerr(drvp: &AtaDriveDatas) {
    // Downgrade decision: if we get NERRS_MAX in NXFER. We start with n_dmaerrs set to
    // NERRS_MAX-1 so that the first error within the first NXFER ops will immediately
    // trigger a downgrade. If we got an error and n_xfers is bigger than NXFER reset
    // counters.
    drvp.n_dmaerrs.set(drvp.n_dmaerrs.get().wrapping_add(1));
    if drvp.n_dmaerrs.get() >= NERRS_MAX && drvp.n_xfers.get() <= NXFER {
        let _ = wdc_downgrade_mode(drvp);
        drvp.n_dmaerrs.set(NERRS_MAX - 1);
        drvp.n_xfers.set(0);
        return;
    }
    if drvp.n_xfers.get() > NXFER {
        drvp.n_dmaerrs.set(1); // just got an error
        drvp.n_xfers.set(1); // restart counting from this error
    }
}

/// `ata_perror`: the error register `errno` as text in `buf` (NUL-terminated).
pub fn ata_perror(drvp: &AtaDriveDatas, errno: i32, buf: &mut [u8]) {
    let errstr = if drvp.ata_vers.get() >= 4 {
        &ERRSTR4_5
    } else {
        &ERRSTR0_3
    };
    let mut sep = "";
    let mut len = 0;

    if errno == 0 {
        let _ = snprintf(buf, format_args!("error not notified"));
    }

    for (i, msg) in errstr.iter().enumerate() {
        if errno & (1 << i) != 0 {
            let start = len.min(buf.len());
            let _ = snprintf(&mut buf[start..], format_args!("{}{}", sep, msg));
            len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            sep = ", ";
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::subr_prf::Str;
    use std::format;

    #[test]
    fn identify_strings_come_in_byte_order() {
        let mut p = Ataparams::zeroed();
        p.atap_model[..14].copy_from_slice(b"EQUMH RADDSI K");
        p.atap_serial[..4].copy_from_slice(b"MQ00");
        p.atap_revision[..2].copy_from_slice(b"12");
        ata_params_fixup(&mut p);
        assert_eq!(&p.atap_model[..14], b"QEMU HARDDISK ");
        assert_eq!(&p.atap_serial[..4], b"QM00");
        assert_eq!(&p.atap_revision[..2], b"21");
    }

    #[test]
    fn nec_and_mitsumi_atapi_strings_stay() {
        let mut p = Ataparams::zeroed();
        p.atap_config = WDC_CFG_ATAPI | 0x0580;
        p.atap_model[..4].copy_from_slice(b"NEC ");
        ata_params_fixup(&mut p);
        assert_eq!(&p.atap_model[..4], b"NEC ");
        // An ATA drive's are swapped whatever they say.
        let mut p = Ataparams::zeroed();
        p.atap_model[..4].copy_from_slice(b"NEC ");
        ata_params_fixup(&mut p);
        assert_eq!(&p.atap_model[..4], b"EN C");
    }

    #[test]
    fn error_register_text() {
        let d = AtaDriveDatas::new();
        let mut buf = [0u8; 256];
        ata_perror(&d, 0x04 | 0x40, &mut buf);
        assert_eq!(
            format!("{}", Str(&buf)),
            "aborted command, uncorrectable data error"
        );
        d.ata_vers.set(4);
        let mut buf = [0u8; 256];
        ata_perror(&d, 0x80, &mut buf);
        assert_eq!(format!("{}", Str(&buf)), "interface CRC error");
        let mut buf = [0u8; 256];
        ata_perror(&d, 0, &mut buf);
        assert_eq!(format!("{}", Str(&buf)), "error not notified");
    }

    #[test]
    fn dma_errors_count_towards_a_downgrade() {
        let d = AtaDriveDatas::new();
        // Past NXFER transfers an error restarts the count.
        d.n_xfers.set(NXFER + 5);
        d.n_dmaerrs.set(2);
        ata_dmaerr(&d);
        assert_eq!((d.n_dmaerrs.get(), d.n_xfers.get()), (1, 1));
        // Below NERRS_MAX nothing else happens.
        ata_dmaerr(&d);
        assert_eq!((d.n_dmaerrs.get(), d.n_xfers.get()), (2, 1));
    }
}
/* </TESTS> */
