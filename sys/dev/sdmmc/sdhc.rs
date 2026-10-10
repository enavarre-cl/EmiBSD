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
//! `sdhc(4)`: the SD Host Controller driver, based on the SD Host Controller Standard
//! Simplified Specification Version 1.00 (www.sdcard.org).
//!
//! Upstream: sys/dev/sdmmc/sdhc.c @ 3ce1f3f79392
//!
//! An attachment (`sdhc_pci`, ...) maps one standard register set per slot and calls
//! `sdhc_host_found`, which resets the host, reads its capabilities (spec version, base
//! clock, voltages, block length, ADMA2), allocates the ADMA2 descriptor page and attaches
//! the `sdmmc(4)` bus with the chip functions below. Commands are started by writing the
//! command register; their completion, the buffer-ready events and the DMA completion are
//! waited for by sleeping on the host's soft interrupt status (polling the status register
//! while cold), which `sdhc_intr` fills.
//!
//! ## Deviations
//! - `struct sdhc_host` is allocated by `sdhc_host_found` (`M_ZERO`) and written whole; its
//!   members are `Cell`s; `iot`/`ioh` are fixed at creation. `sdmmc` is the attached bus's
//!   device (`None` until `config_found` returns).
//! - The chip functions take the host as the `sdmmc_chipset_handle_t` the bus gives back
//!   (`sdhc_host_found` passes it); the C's 0/errno returns are `Result<(), Errno>`, and
//!   `sdhc_host_found`'s 1 is `EIO`.
//! - The 136-bit response is written byte by byte into the response words' memory, as the
//!   C's `u_char *` walk does; the PIO data path reads and writes unaligned 32-bit words.
//! - The ADMA2 descriptors are written into the descriptor page unaligned and volatile.
//! - `sdhc_shutdown` takes the softc (the C's `void *` argument).
//! - An interrupt that wants the bus (card change, card interrupt) before `config_found`
//!   has returned panics where the C passes NULL on.
//! - Not configured: `SDHC_DEBUG` (`sdhc_dump_regs` is not compiled; `DPRINTF`s are
//!   comments). `DIAGNOSTIC` is feature `diagnostic`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::sdmmc::sdhcreg::{
    SDHC_1_8V_SIGNAL_EN, SDHC_4BIT_MODE, SDHC_8BIT_MODE, SDHC_8BIT_MODE_SUPP, SDHC_64BIT_DMA_SUPP,
    SDHC_ADMA_SYSTEM_ADDR, SDHC_ADMA2_ACT_TRANS, SDHC_ADMA2_END, SDHC_ADMA2_SUPP, SDHC_ADMA2_VALID,
    SDHC_ARGUMENT, SDHC_AUTO_CMD12_ENABLE, SDHC_BLOCK_COUNT, SDHC_BLOCK_COUNT_ENABLE,
    SDHC_BLOCK_COUNT_MAX, SDHC_BLOCK_GAP_EVENT, SDHC_BLOCK_SIZE, SDHC_BUFFER_READ_ENABLE,
    SDHC_BUFFER_READ_READY, SDHC_BUFFER_WRITE_ENABLE, SDHC_BUFFER_WRITE_READY, SDHC_BUS_POWER,
    SDHC_CAPABILITIES, SDHC_CAPABILITIES2, SDHC_CARD_INSERTED, SDHC_CARD_INSERTION,
    SDHC_CARD_INTERRUPT, SDHC_CARD_REMOVAL, SDHC_CLOCK_CTL, SDHC_CMD_INHIBIT_MASK,
    SDHC_CMD_TIMEOUT_ERROR, SDHC_COMMAND, SDHC_COMMAND_COMPLETE, SDHC_COMMAND_INDEX_MASK,
    SDHC_COMMAND_INDEX_SHIFT, SDHC_CRC_CHECK_ENABLE, SDHC_DATA, SDHC_DATA_PRESENT_SELECT,
    SDHC_DATA_TIMEOUT_ERROR, SDHC_DDR50_SUPP, SDHC_DMA_ENABLE, SDHC_DMA_INTERRUPT, SDHC_DMA_SELECT,
    SDHC_DMA_SELECT_ADMA32, SDHC_DMA_SELECT_ADMA64, SDHC_EINTR_SIGNAL_EN, SDHC_EINTR_SIGNAL_MASK,
    SDHC_EINTR_STATUS, SDHC_EINTR_STATUS_EN, SDHC_EINTR_STATUS_MASK, SDHC_ERROR_INTERRUPT,
    SDHC_HIGH_SPEED, SDHC_HIGH_SPEED_SUPP, SDHC_HOST_CTL, SDHC_HOST_CTL_VERSION, SDHC_HOST_CTL2,
    SDHC_INDEX_CHECK_ENABLE, SDHC_INTCLK_ENABLE, SDHC_INTCLK_STABLE, SDHC_LED_ON,
    SDHC_MAX_BLK_LEN_512, SDHC_MAX_BLK_LEN_1024, SDHC_MAX_BLK_LEN_2048, SDHC_MAX_BLK_LEN_MASK,
    SDHC_MAX_BLK_LEN_SHIFT, SDHC_MULTI_BLOCK_MODE, SDHC_NINTR_SIGNAL_EN, SDHC_NINTR_STATUS,
    SDHC_NINTR_STATUS_EN, SDHC_NINTR_STATUS_MASK, SDHC_NO_RESPONSE, SDHC_POWER_CTL,
    SDHC_PRESENT_STATE, SDHC_READ_MODE, SDHC_RESET_ALL, SDHC_RESET_CMD, SDHC_RESET_DAT,
    SDHC_RESP_LEN_48, SDHC_RESP_LEN_48_CHK_BUSY, SDHC_RESP_LEN_136, SDHC_RESPONSE,
    SDHC_SDCLK_DIV_MAX, SDHC_SDCLK_DIV_MAX_V3, SDHC_SDCLK_ENABLE, SDHC_SOFTWARE_RESET,
    SDHC_SPEC_V3, SDHC_SPEC_VERS_4_10, SDHC_SPEC_VERS_4_20, SDHC_TIMEOUT_CTL, SDHC_TIMEOUT_MAX,
    SDHC_TRANSFER_COMPLETE, SDHC_TRANSFER_MODE, SDHC_UHS_MODE_SELECT_DDR50,
    SDHC_UHS_MODE_SELECT_MASK, SDHC_VOLTAGE_1_8V, SDHC_VOLTAGE_3_0V, SDHC_VOLTAGE_3_3V,
    SDHC_VOLTAGE_SHIFT, SDHC_VOLTAGE_SUPP_1_8V, SDHC_VOLTAGE_SUPP_3_0V, SDHC_VOLTAGE_SUPP_3_3V,
    SdhcAdma2Descriptor32, SdhcAdma2Descriptor64, sdhc_base_freq_khz, sdhc_base_freq_khz_v3,
    sdhc_sdclk_div, sdhc_sdclk_div_v3, sdhc_spec_version,
};
use crate::dev::sdmmc::sdhcvar::{
    SDHC_F_32BIT_ACCESS, SDHC_F_NO_HS_BIT, SDHC_F_NONREMOVABLE, SDHC_F_NOPWR0, SdhcSoftc,
};
use crate::dev::sdmmc::sdmmc::{sdmmc_delay, sdmmc_needs_discover};
use crate::dev::sdmmc::sdmmc_io::sdmmc_card_intr;
use crate::dev::sdmmc::sdmmc_ioreg::SD_IO_RW_EXTENDED;
use crate::dev::sdmmc::sdmmcchip::{
    SDMMC_SDCLK_OFF, SDMMC_SIGNAL_VOLTAGE_180, SDMMC_SIGNAL_VOLTAGE_330, SDMMC_TIMING_LEGACY,
    SDMMC_TIMING_MMC_DDR52, SdmmcChipFunctions, SdmmcChipsetHandle, SdmmcbusAttachArgs,
};
use crate::dev::sdmmc::sdmmcreg::{
    MMC_OCR_1_65V_1_95V, MMC_OCR_2_9V_3_0V, MMC_OCR_3_0V_3_1V, MMC_OCR_3_2V_3_3V, MMC_OCR_3_3V_3_4V,
};
use crate::dev::sdmmc::sdmmcvar::{
    SCF_CMD_READ, SCF_ITSDONE, SCF_RSP_136, SCF_RSP_BSY, SCF_RSP_CRC, SCF_RSP_IDX, SCF_RSP_PRESENT,
    SMC_CAPS_4BIT_MODE, SMC_CAPS_8BIT_MODE, SMC_CAPS_DMA, SMC_CAPS_MMC_DDR52,
    SMC_CAPS_MMC_HIGHSPEED, SMC_CAPS_NONREMOVABLE, SMC_CAPS_SD_HIGHSPEED, SdmmcCommand, splsdmmc,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_autoconf::{config_activate_children, config_found};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_WAITOK, BUS_DMA_WRITE, BUS_DMA_ZERO, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync, bus_dmamem_alloc,
    bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap, bus_space_read_1, bus_space_read_2,
    bus_space_read_4, bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::splx;
use crate::sys::device::{Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::param::{PAGE_SIZE, PWAIT};
use crate::sys::systm::COLD;
use crate::sys::time::sec_to_nsec;

/* Timeouts in seconds */

/// `SDHC_COMMAND_TIMEOUT`.
pub const SDHC_COMMAND_TIMEOUT: i32 = 1;
/// `SDHC_BUFFER_TIMEOUT`.
pub const SDHC_BUFFER_TIMEOUT: i32 = 1;
/// `SDHC_TRANSFER_TIMEOUT`.
pub const SDHC_TRANSFER_TIMEOUT: i32 = 1;
/// `SDHC_DMA_TIMEOUT`.
pub const SDHC_DMA_TIMEOUT: i32 = 3;

/* flag values */

/// `SHF_USE_DMA`.
pub const SHF_USE_DMA: i32 = 0x0001;
/// `SHF_USE_DMA64`.
pub const SHF_USE_DMA64: i32 = 0x0002;
/// `SHF_USE_32BIT_ACCESS`.
pub const SHF_USE_32BIT_ACCESS: i32 = 0x0004;

/// `struct sdhc_host`: one SD card slot's standard register set and state.
#[derive(Clone)]
pub struct SdhcHost {
    /// `sc`: host controller device.
    pub sc: &'static SdhcSoftc,
    /// `sdmmc`: generic SD/MMC device.
    pub sdmmc: Cell<Option<NonNull<Device>>>,
    /// `iot`: host register set tag.
    pub iot: BusSpaceTag,
    /// `ioh`: host register set handle.
    pub ioh: BusSpaceHandle,
    /// `version`: specification version.
    pub version: Cell<u16>,
    /// `clkbase`: base clock frequency in KHz.
    pub clkbase: Cell<u32>,
    /// `maxblklen`: maximum block length.
    pub maxblklen: Cell<i32>,
    /// `flags`: flags for this host (`SHF_*`).
    pub flags: Cell<i32>,
    /// `ocr`: OCR value from capabilities.
    pub ocr: Cell<u32>,
    /// `regs`: host controller state.
    pub regs: Cell<[u8; 14]>,
    /// `intr_status`: soft interrupt status.
    pub intr_status: Cell<u16>,
    /// `intr_error_status`: soft error status.
    pub intr_error_status: Cell<u16>,

    /// `adma_map`.
    pub adma_map: Cell<Option<&'static BusDmamap>>,
    /// `adma_segs`.
    pub adma_segs: Cell<[BusDmaSegment; 1]>,
    /// `adma2`: the descriptor page's kernel address.
    pub adma2: Cell<*mut u8>,

    /// `block_size`.
    pub block_size: Cell<u16>,
    /// `block_count`.
    pub block_count: Cell<u16>,
    /// `transfer_mode`.
    pub transfer_mode: Cell<u16>,
}

impl SdhcHost {
    /// `DEVNAME(hp->sc)`.
    fn devname(&self) -> &'static str {
        self.sc.sc_dev.xname()
    }

    /// `ISSET(hp->flags, f)`.
    fn has_flag(&self, f: i32) -> bool {
        self.flags.get() & f != 0
    }

    /// `hp->sdmmc`, the attached bus.
    fn sdmmc(&self) -> &'static Device {
        match self.sdmmc.get() {
            // SAFETY: the bus `config_found` attached below this host; devices with a
            // parent attached are never freed.
            Some(d) => unsafe { d.as_ref() },
            None => panic(format_args!("{}: no sdmmc bus", self.devname())),
        }
    }

    /// `hp->adma_map`, which `sdhc_host_found` made when it set `SHF_USE_DMA`.
    fn adma_map(&self) -> &'static BusDmamap {
        match self.adma_map.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no ADMA2 descriptor map", self.devname())),
        }
    }
}

/// `sdhc_functions`.
pub static SDHC_FUNCTIONS: SdmmcChipFunctions = SdmmcChipFunctions {
    host_reset: sdhc_host_reset,
    host_ocr: sdhc_host_ocr,
    host_maxblklen: sdhc_host_maxblklen,
    card_detect: sdhc_card_detect,
    bus_power: sdhc_bus_power,
    bus_clock: sdhc_bus_clock,
    bus_width: sdhc_bus_width,
    exec_command: sdhc_exec_command,
    card_intr_mask: Some(sdhc_card_intr_mask),
    card_intr_ack: Some(sdhc_card_intr_ack),
    signal_voltage: Some(sdhc_signal_voltage),
    execute_tuning: None,
    hibernate_init: Some(sdhc_hibernate_init),
};

/// `sdhc_cd`.
pub static SDHC_CD: Cfdriver = Cfdriver::new(b"sdhc", DV_DULL, 0);

/// The host a chip function is called for (`struct sdhc_host *hp = sch`).
fn sdhc_host(sch: SdmmcChipsetHandle) -> &'static SdhcHost {
    if sch.is_null() {
        panic(format_args!("sdhc: no host"));
    }
    // SAFETY: `sdhc_host_found` gives the sdmmc bus its host as the chipset handle, and a
    // host attached to a bus is never freed.
    unsafe { &*sch.cast::<SdhcHost>().cast_const() }
}

/// `HREAD4(hp, reg)`.
fn hread4(hp: &SdhcHost, reg: BusSize) -> u32 {
    bus_space_read_4(hp.iot, hp.ioh, reg)
}

/// `HWRITE4(hp, reg, val)`.
fn hwrite4(hp: &SdhcHost, reg: BusSize, val: u32) {
    bus_space_write_4(hp.iot, hp.ioh, reg, val);
}

/// `HCLR1(hp, reg, bits)`.
fn hclr1(hp: &SdhcHost, reg: BusSize, bits: u8) {
    sdhc_write_1(hp, reg, sdhc_read_1(hp, reg) & !bits);
}

/// `HCLR2(hp, reg, bits)`.
fn hclr2(hp: &SdhcHost, reg: BusSize, bits: u16) {
    sdhc_write_2(hp, reg, sdhc_read_2(hp, reg) & !bits);
}

/// `HSET1(hp, reg, bits)`.
fn hset1(hp: &SdhcHost, reg: BusSize, bits: u8) {
    sdhc_write_1(hp, reg, sdhc_read_1(hp, reg) | bits);
}

/// `HSET2(hp, reg, bits)`.
fn hset2(hp: &SdhcHost, reg: BusSize, bits: u16) {
    sdhc_write_2(hp, reg, sdhc_read_2(hp, reg) | bits);
}

/*
 * Some controllers live on a bus that only allows 32-bit
 * transactions.  In that case we use a RMW cycle for 8-bit and 16-bit
 * register writes.  However that doesn't work for the Transfer Mode
 * register as this register lives in the same 32-bit word as the
 * Command register and writing the Command register triggers SD
 * command generation.  We avoid this issue by using a shadow variable
 * for the Transfer Mode register that we write out when we write the
 * Command register.
 *
 * The Arasan controller integrated on the Broadcom SoCs
 * used in the Raspberry Pi has an interesting bug where writing the
 * same 32-bit register twice doesn't work.  This means that we lose
 * writes to the Block Sine and/or Block Count register.  We work
 * around that issue by using shadow variables as well.
 */

/// `sdhc_read_1` (`HREAD1`).
pub fn sdhc_read_1(hp: &SdhcHost, offset: BusSize) -> u8 {
    if hp.has_flag(SHF_USE_32BIT_ACCESS) {
        let reg = bus_space_read_4(hp.iot, hp.ioh, offset & !3);
        return ((reg >> ((offset & 3) * 8)) & 0xff) as u8;
    }

    bus_space_read_1(hp.iot, hp.ioh, offset)
}

/// `sdhc_read_2` (`HREAD2`).
pub fn sdhc_read_2(hp: &SdhcHost, offset: BusSize) -> u16 {
    if hp.has_flag(SHF_USE_32BIT_ACCESS) {
        let reg = bus_space_read_4(hp.iot, hp.ioh, offset & !2);
        return ((reg >> ((offset & 2) * 8)) & 0xffff) as u16;
    }

    bus_space_read_2(hp.iot, hp.ioh, offset)
}

/// `sdhc_write_1` (`HWRITE1`).
pub fn sdhc_write_1(hp: &SdhcHost, offset: BusSize, value: u8) {
    if hp.has_flag(SHF_USE_32BIT_ACCESS) {
        let mut reg = bus_space_read_4(hp.iot, hp.ioh, offset & !3);
        reg &= !(0xff << ((offset & 3) * 8));
        reg |= u32::from(value) << ((offset & 3) * 8);
        bus_space_write_4(hp.iot, hp.ioh, offset & !3, reg);
        return;
    }

    bus_space_write_1(hp.iot, hp.ioh, offset, value);
}

/// `sdhc_write_2` (`HWRITE2`).
pub fn sdhc_write_2(hp: &SdhcHost, offset: BusSize, value: u16) {
    if hp.has_flag(SHF_USE_32BIT_ACCESS) {
        match offset {
            SDHC_BLOCK_SIZE => {
                hp.block_size.set(value);
                return;
            }
            SDHC_BLOCK_COUNT => {
                hp.block_count.set(value);
                return;
            }
            SDHC_TRANSFER_MODE => {
                hp.transfer_mode.set(value);
                return;
            }
            SDHC_COMMAND => {
                bus_space_write_4(
                    hp.iot,
                    hp.ioh,
                    SDHC_BLOCK_SIZE,
                    (u32::from(hp.block_count.get()) << 16) | u32::from(hp.block_size.get()),
                );
                bus_space_write_4(
                    hp.iot,
                    hp.ioh,
                    SDHC_TRANSFER_MODE,
                    (u32::from(value) << 16) | u32::from(hp.transfer_mode.get()),
                );
                return;
            }
            _ => {}
        }

        let mut reg = bus_space_read_4(hp.iot, hp.ioh, offset & !2);
        reg &= !(0xffff << ((offset & 2) * 8));
        reg |= u32::from(value) << ((offset & 2) * 8);
        bus_space_write_4(hp.iot, hp.ioh, offset & !2, reg);
        return;
    }

    bus_space_write_2(hp.iot, hp.ioh, offset, value);
}

/// `sdhc_host_found`: called by the attachment driver. For each SD card slot there is one
/// SD host controller standard register set. (1.3)
pub fn sdhc_host_found(
    sc: &'static SdhcSoftc,
    iot: BusSpaceTag,
    ioh: BusSpaceHandle,
    _iosize: BusSize,
    usedma: bool,
    capmask: u64,
    capset: u64,
) -> Result<(), Errno> {
    // Allocate one more host structure.
    sc.sc_nhosts.set(sc.sc_nhosts.get() + 1);
    let Some(mem) = malloc(size_of::<SdhcHost>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("sdhc_host_found: M_WAITOK allocation failed"));
    };
    let hpp = mem.cast::<SdhcHost>().as_ptr();
    // Fill in the new host structure.
    // SAFETY: a fresh allocation of the host's size (malloc(9) aligns it for any type),
    // written whole before it is shared; it lives until it is freed below or, attached,
    // forever.
    let hp: &'static SdhcHost = unsafe {
        hpp.write(SdhcHost {
            sc,
            sdmmc: Cell::new(None),
            iot,
            ioh,
            version: Cell::new(0),
            clkbase: Cell::new(0),
            maxblklen: Cell::new(0),
            flags: Cell::new(if sc.sc_flags.get() & SDHC_F_32BIT_ACCESS != 0 {
                SHF_USE_32BIT_ACCESS
            } else {
                0
            }),
            ocr: Cell::new(0),
            regs: Cell::new([0; 14]),
            intr_status: Cell::new(0),
            intr_error_status: Cell::new(0),
            adma_map: Cell::new(None),
            adma_segs: Cell::new([BusDmaSegment::default(); 1]),
            adma2: Cell::new(ptr::null_mut()),
            block_size: Cell::new(0),
            block_count: Cell::new(0),
            transfer_mode: Cell::new(0),
        });
        &*hpp
    };
    let slot = sc.sc_nhosts.get() - 1;
    // SAFETY: the attachment allocated `sc_host` with room for every slot it maps, one
    // `sdhc_host_found` per slot.
    unsafe { *sc.sc_host.get().add(slot as usize) = hpp };

    let error: Result<(), Errno> = 'err: {
        // Store specification version.
        hp.version.set(sdhc_read_2(hp, SDHC_HOST_CTL_VERSION));

        // Reset the host controller and enable interrupts.
        let _ = sdhc_host_reset(ptr::from_ref(hp).cast_mut().cast());

        // Determine host capabilities.
        let mut caps = hread4(hp, SDHC_CAPABILITIES);
        caps &= !(capmask as u32);
        caps |= capset as u32;

        // Use DMA if the host system and the controller support it.
        if usedma && caps & SDHC_ADMA2_SUPP != 0 {
            hp.flags.set(hp.flags.get() | SHF_USE_DMA);
            if caps & SDHC_64BIT_DMA_SUPP != 0 {
                hp.flags.set(hp.flags.get() | SHF_USE_DMA64);
            }
        }

        // Determine the base clock frequency. (2.2.24)
        let spec = sdhc_spec_version(hp.version.get());
        let mut max_clock: u32;
        if spec >= SDHC_SPEC_V3 {
            // SDHC 3.0 supports 10-255 MHz.
            max_clock = 255000;
            if sdhc_base_freq_khz_v3(caps) != 0 {
                hp.clkbase.set(sdhc_base_freq_khz_v3(caps));
            }
        } else {
            // SDHC 1.0/2.0 supports only 10-63 MHz.
            max_clock = 63000;
            if sdhc_base_freq_khz(caps) != 0 {
                hp.clkbase.set(sdhc_base_freq_khz(caps));
            }
        }
        if hp.clkbase.get() == 0 {
            // Make sure we can clock down to 400 kHz.
            max_clock = 400 * SDHC_SDCLK_DIV_MAX_V3 as u32;
            hp.clkbase.set(sc.sc_clkbase.get());
        }
        if hp.clkbase.get() == 0 {
            // The attachment driver must tell us.
            printf(format_args!(
                "{}: base clock frequency unknown\n",
                sc.sc_dev.xname()
            ));
            break 'err Err(Errno::EIO);
        } else if hp.clkbase.get() < 10000 || hp.clkbase.get() > max_clock {
            printf(format_args!(
                "{}: base clock frequency out of range: {} MHz\n",
                sc.sc_dev.xname(),
                hp.clkbase.get() / 1000
            ));
            break 'err Err(Errno::EIO);
        }

        let (major, minor) = match spec {
            SDHC_SPEC_VERS_4_10 => (4, 10),
            SDHC_SPEC_VERS_4_20 => (4, 20),
            _ => (i32::from(spec) + 1, 0),
        };

        printf(format_args!(
            "{}: SDHC {}.{:02}, {} MHz base clock\n",
            sc.sc_dev.xname(),
            major,
            minor,
            hp.clkbase.get() / 1000
        ));

        // XXX Set the data timeout counter value according to capabilities. (2.2.15)

        // Determine SD bus voltage levels supported by the controller.
        if caps & SDHC_VOLTAGE_SUPP_1_8V != 0 {
            hp.ocr.set(hp.ocr.get() | MMC_OCR_1_65V_1_95V);
        }
        if caps & SDHC_VOLTAGE_SUPP_3_0V != 0 {
            hp.ocr
                .set(hp.ocr.get() | MMC_OCR_2_9V_3_0V | MMC_OCR_3_0V_3_1V);
        }
        if caps & SDHC_VOLTAGE_SUPP_3_3V != 0 {
            hp.ocr
                .set(hp.ocr.get() | MMC_OCR_3_2V_3_3V | MMC_OCR_3_3V_3_4V);
        }

        // Determine the maximum block length supported by the host controller. (2.2.24)
        hp.maxblklen.set(
            match (caps >> SDHC_MAX_BLK_LEN_SHIFT) & SDHC_MAX_BLK_LEN_MASK {
                SDHC_MAX_BLK_LEN_512 => 512,
                SDHC_MAX_BLK_LEN_1024 => 1024,
                SDHC_MAX_BLK_LEN_2048 => 2048,
                _ => 1,
            },
        );

        if hp.has_flag(SHF_USE_DMA) && sdhc_adma_alloc(sc, hp).is_err() {
            // adma_done:
            printf(format_args!(
                "{}: can't allocate DMA descriptor table\n",
                hp.devname()
            ));
            hp.flags.set(hp.flags.get() & !SHF_USE_DMA);
        }

        // Attach the generic SD/MMC bus driver. (The bus driver must not invoke any chipset
        // functions before it is attached.)
        let mut saa = SdmmcbusAttachArgs::new();
        saa.saa_busname = b"sdmmc";
        saa.sct = Some(&SDHC_FUNCTIONS);
        saa.sch = ptr::from_ref(hp).cast_mut().cast();
        saa.caps = SMC_CAPS_4BIT_MODE;
        saa.dmat = sc.sc_dmat.get();
        saa.dma_boundary = sc.sc_dma_boundary.get();
        if hp.has_flag(SHF_USE_DMA) {
            saa.caps |= SMC_CAPS_DMA;
        }

        if caps & SDHC_HIGH_SPEED_SUPP != 0 {
            saa.caps |= SMC_CAPS_SD_HIGHSPEED;
        }
        if caps & SDHC_HIGH_SPEED_SUPP != 0 {
            saa.caps |= SMC_CAPS_MMC_HIGHSPEED;
        }

        if spec >= SDHC_SPEC_V3 {
            let mut caps2 = hread4(hp, SDHC_CAPABILITIES2);
            caps2 &= !((capmask >> 32) as u32);
            caps2 |= (capset >> 32) as u32;

            if caps & SDHC_8BIT_MODE_SUPP != 0 {
                saa.caps |= SMC_CAPS_8BIT_MODE;
            }

            if caps2 & SDHC_DDR50_SUPP != 0 {
                saa.caps |= SMC_CAPS_MMC_DDR52;
            }
        }

        if sc.sc_flags.get() & SDHC_F_NONREMOVABLE != 0 {
            saa.caps |= SMC_CAPS_NONREMOVABLE;
        }

        hp.sdmmc.set(config_found(
            &sc.sc_dev,
            ptr::from_mut(&mut saa).cast(),
            None,
        ));
        if hp.sdmmc.get().is_none() {
            break 'err Ok(());
        }

        return Ok(());
    };

    // err:
    free(mem, M_DEVBUF, size_of::<SdhcHost>());
    // SAFETY: the slot this call filled above.
    unsafe { *sc.sc_host.get().add(slot as usize) = ptr::null_mut() };
    sc.sc_nhosts.set(sc.sc_nhosts.get() - 1);
    error
}

/// The ADMA2 descriptor page of `sdhc_host_found`: allocated, mapped and loaded into
/// `hp.adma_map`; each failure undoes the steps before it.
fn sdhc_adma_alloc(sc: &SdhcSoftc, hp: &SdhcHost) -> Result<(), Errno> {
    let Some(dmat) = sc.sc_dmat.get() else {
        return Err(Errno::EINVAL);
    };
    let mut segs = [BusDmaSegment::default(); 1];

    // Allocate ADMA2 descriptor memory
    let rseg = bus_dmamem_alloc(
        dmat,
        PAGE_SIZE,
        PAGE_SIZE,
        PAGE_SIZE,
        &mut segs,
        BUS_DMA_WAITOK | BUS_DMA_ZERO,
    )?;
    let kva = match bus_dmamem_map(
        dmat,
        &mut segs[..rseg],
        PAGE_SIZE,
        BUS_DMA_WAITOK | BUS_DMA_COHERENT,
    ) {
        Ok(kva) => kva,
        Err(e) => {
            // SAFETY: the segments just allocated, unused.
            unsafe { bus_dmamem_free(dmat, &segs[..rseg]) };
            return Err(e);
        }
    };
    let map = match bus_dmamap_create(dmat, PAGE_SIZE, 1, PAGE_SIZE, 0, BUS_DMA_WAITOK) {
        Ok(map) => map,
        Err(e) => {
            // SAFETY: the mapping and segments just made, unused.
            unsafe {
                bus_dmamem_unmap(dmat, kva, PAGE_SIZE);
                bus_dmamem_free(dmat, &segs[..rseg]);
            }
            return Err(e);
        }
    };
    // SAFETY: the page just mapped, owned by this host for as long as it exists.
    if let Err(e) = unsafe {
        bus_dmamap_load(
            dmat,
            map,
            kva.as_ptr(),
            PAGE_SIZE,
            None,
            BUS_DMA_WAITOK | BUS_DMA_WRITE,
        )
    } {
        // SAFETY: the map, mapping and segments just made, unused.
        unsafe {
            bus_dmamap_destroy(dmat, NonNull::from(map));
            bus_dmamem_unmap(dmat, kva, PAGE_SIZE);
            bus_dmamem_free(dmat, &segs[..rseg]);
        }
        return Err(e);
    }

    hp.adma_segs.set(segs);
    hp.adma2.set(kva.as_ptr());
    hp.adma_map.set(Some(map));
    Ok(())
}

/// `sdhc_activate`.
pub fn sdhc_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: the attachments' softcs begin with the `sdhc_softc`, and only they install
    // this activate function (through their own).
    let sc = unsafe { self_.softc::<SdhcSoftc>() };

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);

            // Save the host controller state.
            for n in 0..sc.sc_nhosts.get() {
                let Some(hp) = sc.host(n) else {
                    continue;
                };
                let mut regs = [0u8; 14];
                for (i, r) in regs.iter_mut().enumerate() {
                    *r = sdhc_read_1(hp, i);
                }
                hp.regs.set(regs);
            }
            rv
        }
        DVACT_RESUME => {
            // Restore the host controller state.
            for n in 0..sc.sc_nhosts.get() {
                let Some(hp) = sc.host(n) else {
                    continue;
                };
                let _ = sdhc_host_reset(ptr::from_ref(hp).cast_mut().cast());
                for (i, r) in hp.regs.get().iter().enumerate() {
                    sdhc_write_1(hp, i, *r);
                }
            }
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            sdhc_shutdown(sc);
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `sdhc_shutdown`: shutdown hook established by or called from attachment driver.
pub fn sdhc_shutdown(sc: &SdhcSoftc) {
    // XXX chip locks up if we don't disable it before reboot.
    for i in 0..sc.sc_nhosts.get() {
        if let Some(hp) = sc.host(i) {
            let _ = sdhc_host_reset(ptr::from_ref(hp).cast_mut().cast());
        }
    }
}

/// `sdhc_host_reset`: resets the host controller. Called during initialization, when cards
/// are removed, upon resume, and during error recovery.
pub fn sdhc_host_reset(sch: SdmmcChipsetHandle) -> Result<(), Errno> {
    let hp = sdhc_host(sch);

    let s = splsdmmc();

    // Disable all interrupts.
    sdhc_write_2(hp, SDHC_NINTR_SIGNAL_EN, 0);

    // Reset the entire host controller and wait up to 100ms for the controller to clear the
    // reset bit.
    if let Err(error) = sdhc_soft_reset(hp, SDHC_RESET_ALL) {
        splx(s);
        return Err(error);
    }

    // Set data timeout counter value to max for now.
    sdhc_write_1(hp, SDHC_TIMEOUT_CTL, SDHC_TIMEOUT_MAX);

    // Enable interrupts.
    let imask = SDHC_CARD_REMOVAL
        | SDHC_CARD_INSERTION
        | SDHC_BUFFER_READ_READY
        | SDHC_BUFFER_WRITE_READY
        | SDHC_DMA_INTERRUPT
        | SDHC_BLOCK_GAP_EVENT
        | SDHC_TRANSFER_COMPLETE
        | SDHC_COMMAND_COMPLETE;

    sdhc_write_2(hp, SDHC_NINTR_STATUS_EN, imask);
    sdhc_write_2(hp, SDHC_EINTR_STATUS_EN, SDHC_EINTR_STATUS_MASK);
    sdhc_write_2(hp, SDHC_NINTR_SIGNAL_EN, imask);
    sdhc_write_2(hp, SDHC_EINTR_SIGNAL_EN, SDHC_EINTR_SIGNAL_MASK);

    splx(s);
    Ok(())
}

/// `sdhc_host_ocr`.
pub fn sdhc_host_ocr(sch: SdmmcChipsetHandle) -> u32 {
    sdhc_host(sch).ocr.get()
}

/// `sdhc_host_maxblklen`.
pub fn sdhc_host_maxblklen(sch: SdmmcChipsetHandle) -> i32 {
    sdhc_host(sch).maxblklen.get()
}

/// `sdhc_card_detect`: true if the card is currently inserted.
pub fn sdhc_card_detect(sch: SdmmcChipsetHandle) -> bool {
    let hp = sdhc_host(sch);

    if let Some(card_detect) = hp.sc.sc_card_detect.get() {
        return card_detect(hp.sc);
    }

    hread4(hp, SDHC_PRESENT_STATE) & SDHC_CARD_INSERTED != 0
}

/// The `SDHC_VOLTAGE_*` level `sdhc_bus_power` selects for the `ocr` the host supports
/// (`hp_ocr`): the highest; `None` for an unsupported request.
pub fn sdhc_select_vdd(ocr: u32, hp_ocr: u32) -> Option<u8> {
    let ocr = ocr & hp_ocr;
    if ocr & (MMC_OCR_3_2V_3_3V | MMC_OCR_3_3V_3_4V) != 0 {
        Some(SDHC_VOLTAGE_3_3V)
    } else if ocr & (MMC_OCR_2_9V_3_0V | MMC_OCR_3_0V_3_1V) != 0 {
        Some(SDHC_VOLTAGE_3_0V)
    } else if ocr & MMC_OCR_1_65V_1_95V != 0 {
        Some(SDHC_VOLTAGE_1_8V)
    } else {
        None
    }
}

/// `sdhc_bus_power`: sets or changes SD bus voltage and enables or disables SD bus power.
pub fn sdhc_bus_power(sch: SdmmcChipsetHandle, ocr: u32) -> Result<(), Errno> {
    let hp = sdhc_host(sch);

    let s = splsdmmc();

    // If power is disabled, reset the host and return now.
    if ocr == 0 {
        sdhc_write_1(hp, SDHC_POWER_CTL, 0);
        splx(s);
        let _ = sdhc_host_reset(sch);
        return Ok(());
    }

    // Select the maximum voltage according to capabilities.
    let Some(vdd) = sdhc_select_vdd(ocr, hp.ocr.get()) else {
        // Unsupported voltage level requested.
        splx(s);
        return Err(Errno::EINVAL);
    };

    // Return if no change to powered bus voltage.
    if sdhc_read_1(hp, SDHC_POWER_CTL) == ((vdd << SDHC_VOLTAGE_SHIFT) | SDHC_BUS_POWER) {
        splx(s);
        return Ok(());
    }

    // Disable bus power before voltage change.
    if hp.sc.sc_flags.get() & SDHC_F_NOPWR0 == 0 {
        sdhc_write_1(hp, SDHC_POWER_CTL, 0);
    }

    // Enable bus power. Wait at least 1 ms (or 74 clocks) plus voltage ramp until power
    // rises.
    sdhc_write_1(
        hp,
        SDHC_POWER_CTL,
        (vdd << SDHC_VOLTAGE_SHIFT) | SDHC_BUS_POWER,
    );
    sdmmc_delay(10000);

    // The host system may not power the bus due to battery low, etc. In that case, the
    // host controller should clear the bus power bit.
    if sdhc_read_1(hp, SDHC_POWER_CTL) & SDHC_BUS_POWER == 0 {
        splx(s);
        return Err(Errno::ENXIO);
    }

    splx(s);
    Ok(())
}

/// `sdhc_clock_divisor`: the smallest possible base clock frequency divisor value for the
/// CLOCK_CTL register to produce `freq` (KHz) from `clkbase` (KHz); -1 if there is none.
pub fn sdhc_clock_divisor(version: u16, clkbase: u32, freq: u32) -> i32 {
    if sdhc_spec_version(version) >= SDHC_SPEC_V3 {
        if clkbase <= freq {
            return 0;
        }

        let mut div = 2;
        while div <= SDHC_SDCLK_DIV_MAX_V3 {
            if clkbase / div as u32 <= freq {
                return div / 2;
            }
            div += 2;
        }
    } else {
        let mut div = 1;
        while div <= SDHC_SDCLK_DIV_MAX {
            if clkbase / div as u32 <= freq {
                return div / 2;
            }
            div *= 2;
        }
    }

    // No divisor found.
    -1
}

/// `sdhc_bus_clock`: sets or changes SDCLK frequency or disables the SD clock.
pub fn sdhc_bus_clock(sch: SdmmcChipsetHandle, freq: i32, timing: i32) -> Result<(), Errno> {
    let hp = sdhc_host(sch);
    let sc = hp.sc;

    let s = splsdmmc();

    if let Some(pre) = sc.sc_bus_clock_pre.get() {
        pre(sc, freq, timing);
    }

    #[cfg(feature = "diagnostic")]
    {
        // Must not stop the clock if commands are in progress.
        if hread4(hp, SDHC_PRESENT_STATE) & SDHC_CMD_INHIBIT_MASK != 0 && sdhc_card_detect(sch) {
            printf(format_args!(
                "sdhc_sdclk_frequency_select: command in progress\n"
            ));
        }
    }

    let error: Result<(), Errno> = 'ret: {
        // Stop SD clock before changing the frequency.
        sdhc_write_2(hp, SDHC_CLOCK_CTL, 0);
        if freq == SDMMC_SDCLK_OFF {
            break 'ret Ok(());
        }

        if sc.sc_flags.get() & SDHC_F_NO_HS_BIT == 0 {
            if timing == SDMMC_TIMING_LEGACY {
                hclr1(hp, SDHC_HOST_CTL, SDHC_HIGH_SPEED);
            } else {
                hset1(hp, SDHC_HOST_CTL, SDHC_HIGH_SPEED);
            }
        }

        if sdhc_spec_version(hp.version.get()) >= SDHC_SPEC_V3 && timing == SDMMC_TIMING_MMC_DDR52 {
            hclr2(hp, SDHC_HOST_CTL2, SDHC_UHS_MODE_SELECT_MASK);
            hset2(hp, SDHC_HOST_CTL2, SDHC_UHS_MODE_SELECT_DDR50);
        }

        // Set the minimum base clock frequency divisor.
        let div = sdhc_clock_divisor(hp.version.get(), hp.clkbase.get(), freq as u32);
        if div < 0 {
            // Invalid base clock frequency or `freq' value.
            break 'ret Err(Errno::EINVAL);
        }
        let sdclk = if sdhc_spec_version(hp.version.get()) >= SDHC_SPEC_V3 {
            sdhc_sdclk_div_v3(div)
        } else {
            sdhc_sdclk_div(div)
        };
        sdhc_write_2(hp, SDHC_CLOCK_CTL, sdclk as u16);

        // Start internal clock. Wait 10ms for stabilization.
        hset2(hp, SDHC_CLOCK_CTL, SDHC_INTCLK_ENABLE);
        let mut timo = 1000;
        while timo > 0 {
            if sdhc_read_2(hp, SDHC_CLOCK_CTL) & SDHC_INTCLK_STABLE != 0 {
                break;
            }
            sdmmc_delay(10);
            timo -= 1;
        }
        if timo == 0 {
            break 'ret Err(Errno::ETIMEDOUT);
        }

        // Enable SD clock.
        hset2(hp, SDHC_CLOCK_CTL, SDHC_SDCLK_ENABLE);

        if let Some(post) = sc.sc_bus_clock_post.get() {
            post(sc, freq, timing);
        }

        Ok(())
    };

    // ret:
    splx(s);
    error
}

/// `sdhc_bus_width`.
pub fn sdhc_bus_width(sch: SdmmcChipsetHandle, width: i32) -> Result<(), Errno> {
    let hp = sdhc_host(sch);

    if width != 1 && width != 4 && width != 8 {
        return Err(Errno::EINVAL);
    }

    let s = splsdmmc();

    let mut reg = sdhc_read_1(hp, SDHC_HOST_CTL);
    reg &= !SDHC_4BIT_MODE;
    if sdhc_spec_version(hp.version.get()) >= SDHC_SPEC_V3 {
        reg &= !SDHC_8BIT_MODE;
    }
    if width == 4 {
        reg |= SDHC_4BIT_MODE;
    } else if width == 8 {
        kassert!(sdhc_spec_version(hp.version.get()) >= SDHC_SPEC_V3);
        reg |= SDHC_8BIT_MODE;
    }
    sdhc_write_1(hp, SDHC_HOST_CTL, reg);

    splx(s);

    Ok(())
}

/// `sdhc_card_intr_mask`.
pub fn sdhc_card_intr_mask(sch: SdmmcChipsetHandle, enable: i32) {
    let hp = sdhc_host(sch);

    if enable != 0 {
        hset2(hp, SDHC_NINTR_STATUS_EN, SDHC_CARD_INTERRUPT);
        hset2(hp, SDHC_NINTR_SIGNAL_EN, SDHC_CARD_INTERRUPT);
    } else {
        hclr2(hp, SDHC_NINTR_SIGNAL_EN, SDHC_CARD_INTERRUPT);
        hclr2(hp, SDHC_NINTR_STATUS_EN, SDHC_CARD_INTERRUPT);
    }
}

/// `sdhc_card_intr_ack`.
pub fn sdhc_card_intr_ack(sch: SdmmcChipsetHandle) {
    let hp = sdhc_host(sch);

    hset2(hp, SDHC_NINTR_STATUS_EN, SDHC_CARD_INTERRUPT);
}

/// `sdhc_signal_voltage`.
pub fn sdhc_signal_voltage(sch: SdmmcChipsetHandle, signal_voltage: i32) -> Result<(), Errno> {
    let hp = sdhc_host(sch);

    if let Some(signal_voltage_fn) = hp.sc.sc_signal_voltage.get() {
        return signal_voltage_fn(hp.sc, signal_voltage);
    }

    if sdhc_spec_version(hp.version.get()) < SDHC_SPEC_V3 {
        return Err(Errno::EINVAL);
    }

    match signal_voltage {
        SDMMC_SIGNAL_VOLTAGE_180 => hset2(hp, SDHC_HOST_CTL2, SDHC_1_8V_SIGNAL_EN),
        SDMMC_SIGNAL_VOLTAGE_330 => hclr2(hp, SDHC_HOST_CTL2, SDHC_1_8V_SIGNAL_EN),
        _ => return Err(Errno::EINVAL),
    }

    // Regulator output shall be stable within 5 ms.
    sdmmc_delay(5000);

    // Host controller clears this bit if 1.8V signalling fails.
    if signal_voltage == SDMMC_SIGNAL_VOLTAGE_180
        && sdhc_read_2(hp, SDHC_HOST_CTL2) & SDHC_1_8V_SIGNAL_EN == 0
    {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `sdhc_wait_state`: waits up to 100 ms for the present state bits `mask` to read `value`.
pub fn sdhc_wait_state(hp: &SdhcHost, mask: u32, value: u32) -> Result<(), Errno> {
    for _ in 0..10 {
        if hread4(hp, SDHC_PRESENT_STATE) & mask == value {
            return Ok(());
        }
        sdmmc_delay(10000);
    }
    // DPRINTF(0,("%s: timeout waiting for %x (state=%b)\n", DEVNAME(hp->sc), value, state,
    //     SDHC_PRESENT_STATE_BITS));
    Err(Errno::ETIMEDOUT)
}

/// `sdhc_exec_command`: runs `cmd` to completion (or failure), its data transfer included.
pub fn sdhc_exec_command(sch: SdmmcChipsetHandle, cmd: &mut SdmmcCommand) {
    let hp = sdhc_host(sch);

    // Start the MMC command, or mark `cmd' as failed and return.
    if let Err(error) = sdhc_start_command(hp, cmd) {
        cmd.c_error = Some(error);
        cmd.c_flags |= SCF_ITSDONE;
        return;
    }

    // Wait until the command phase is done, or until the command is marked done for any
    // other reason.
    if sdhc_wait_intr(hp, SDHC_COMMAND_COMPLETE, SDHC_COMMAND_TIMEOUT) == 0 {
        cmd.c_error = Some(Errno::ETIMEDOUT);
        cmd.c_flags |= SCF_ITSDONE;
        return;
    }

    // The host controller removes bits [0:7] from the response data (CRC) and we pass the
    // data up unchanged to the bus driver (without padding).
    if cmd.c_error.is_none() && cmd.c_flags & SCF_RSP_PRESENT != 0 {
        if cmd.c_flags & SCF_RSP_136 != 0 {
            let mut bytes = [0u8; 16];
            for (b, w) in bytes
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(cmd.c_resp.iter())
            {
                *b = w.to_ne_bytes();
            }
            for (i, b) in bytes.iter_mut().take(15).enumerate() {
                *b = sdhc_read_1(hp, SDHC_RESPONSE + i);
            }
            for (w, b) in cmd.c_resp.iter_mut().zip(bytes.as_chunks::<4>().0) {
                *w = u32::from_ne_bytes(*b);
            }
        } else {
            cmd.c_resp[0] = hread4(hp, SDHC_RESPONSE);
        }
    }

    // If the command has data to transfer in any direction, execute the transfer now.
    if cmd.c_error.is_none() && !cmd.c_data.is_null() {
        sdhc_transfer_data(hp, cmd);
    }

    // Turn off the LED.
    hclr1(hp, SDHC_HOST_CTL, SDHC_LED_ON);

    // DPRINTF(1,("%s: cmd %u done (flags=%#x error=%d)\n", DEVNAME(hp->sc), cmd->c_opcode,
    //     cmd->c_flags, cmd->c_error));
    cmd.c_flags |= SCF_ITSDONE;
}

/// The transfer mode register of a command (2.2.5).
pub fn sdhc_transfer_mode(cmd: &SdmmcCommand, blkcount: u16, use_dma: bool) -> u16 {
    let mut mode: u16 = 0;
    if cmd.c_flags & SCF_CMD_READ != 0 {
        mode |= SDHC_READ_MODE;
    }
    if blkcount > 0 {
        mode |= SDHC_BLOCK_COUNT_ENABLE;
        if blkcount > 1 {
            mode |= SDHC_MULTI_BLOCK_MODE;
            if cmd.c_opcode != SD_IO_RW_EXTENDED {
                mode |= SDHC_AUTO_CMD12_ENABLE;
            }
        }
    }
    if cmd.c_dmamap.is_some() && cmd.c_datalen > 0 && use_dma {
        mode |= SDHC_DMA_ENABLE;
    }
    mode
}

/// The command register of a command (2.2.6).
pub fn sdhc_command_reg(cmd: &SdmmcCommand) -> u16 {
    let mut command = (cmd.c_opcode & SDHC_COMMAND_INDEX_MASK) << SDHC_COMMAND_INDEX_SHIFT;

    if cmd.c_flags & SCF_RSP_CRC != 0 {
        command |= SDHC_CRC_CHECK_ENABLE;
    }
    if cmd.c_flags & SCF_RSP_IDX != 0 {
        command |= SDHC_INDEX_CHECK_ENABLE;
    }
    if !cmd.c_data.is_null() {
        command |= SDHC_DATA_PRESENT_SELECT;
    }

    if cmd.c_flags & SCF_RSP_PRESENT == 0 {
        command |= SDHC_NO_RESPONSE;
    } else if cmd.c_flags & SCF_RSP_136 != 0 {
        command |= SDHC_RESP_LEN_136;
    } else if cmd.c_flags & SCF_RSP_BSY != 0 {
        command |= SDHC_RESP_LEN_48_CHK_BUSY;
    } else {
        command |= SDHC_RESP_LEN_48;
    }
    command
}

/// `sdhc_start_command`: programs the registers of `cmd` and starts it.
pub fn sdhc_start_command(hp: &SdhcHost, cmd: &SdmmcCommand) -> Result<(), Errno> {
    let sc = hp.sc;
    let mut blksize: u16 = 0;
    let mut blkcount: u16 = 0;

    // DPRINTF(1,("%s: start cmd %u arg=%#x data=%p dlen=%d flags=%#x\n", DEVNAME(hp->sc),
    //     cmd->c_opcode, cmd->c_arg, cmd->c_data, cmd->c_datalen, cmd->c_flags));

    // The maximum block length for commands should be the minimum of the host buffer size
    // and the card buffer size. (1.7.2)

    // Fragment the data into proper blocks.
    if cmd.c_datalen > 0 {
        let bs = cmd.c_datalen.min(cmd.c_blklen);
        blksize = bs as u16;
        blkcount = (cmd.c_datalen / bs) as u16;
        if cmd.c_datalen % bs > 0 {
            // XXX: Split this command. (1.7.4)
            printf(format_args!(
                "{}: data not a multiple of {} bytes\n",
                hp.devname(),
                blksize
            ));
            return Err(Errno::EINVAL);
        }
    }

    // Check limit imposed by 9-bit block count. (1.7.2)
    if blkcount > SDHC_BLOCK_COUNT_MAX {
        printf(format_args!("{}: too much data\n", hp.devname()));
        return Err(Errno::EINVAL);
    }

    // Prepare transfer mode register value. (2.2.5)
    let mode = sdhc_transfer_mode(cmd, blkcount, hp.has_flag(SHF_USE_DMA));

    // Prepare command register value. (2.2.6)
    let command = sdhc_command_reg(cmd);

    // Wait until command and data inhibit bits are clear. (1.5)
    sdhc_wait_state(hp, SDHC_CMD_INHIBIT_MASK, 0)?;

    let s = splsdmmc();

    // Alert the user not to remove the card.
    hset1(hp, SDHC_HOST_CTL, SDHC_LED_ON);

    // Set DMA start address if SHF_USE_DMA is set.
    match cmd.c_dmamap {
        Some(dmap) if hp.has_flag(SHF_USE_DMA) => {
            let nsegs = dmap.dm_nsegs.get() as usize;
            let adma2 = hp.adma2.get();
            let dma64 = hp.has_flag(SHF_USE_DMA64);
            for (seg, s) in dmap.dm_segs().iter().take(nsegs).enumerate() {
                let s = s.get();
                let paddr = s.ds_addr as u64;
                let len: u16 = if s.ds_len == 65536 {
                    0
                } else {
                    s.ds_len as u16
                };
                let mut attr = SDHC_ADMA2_VALID | SDHC_ADMA2_ACT_TRANS;
                if seg == nsegs - 1 {
                    attr |= SDHC_ADMA2_END;
                }

                if dma64 {
                    let d = SdhcAdma2Descriptor64 {
                        attribute: attr.to_le(),
                        length: len.to_le(),
                        address_lo: ((paddr & 0xffff_ffff) as u32).to_le(),
                        address_hi: ((paddr >> 32) as u32).to_le(),
                    };
                    // SAFETY: the descriptor page (PAGE_SIZE bytes, mapped while the host
                    // exists) holds far more than `SDMMC_MAXNSEGS + 1` descriptors.
                    unsafe {
                        ptr::write_volatile(adma2.cast::<SdhcAdma2Descriptor64>().add(seg), d)
                    };
                } else {
                    let d = SdhcAdma2Descriptor32 {
                        attribute: attr.to_le(),
                        length: len.to_le(),
                        address: (paddr as u32).to_le(),
                    };
                    // SAFETY: as above.
                    unsafe {
                        ptr::write_volatile(adma2.cast::<SdhcAdma2Descriptor32>().add(seg), d)
                    };
                }
            }

            // The terminating descriptor: attribute 0.
            // SAFETY: as above; the attribute is the descriptor's first member.
            unsafe {
                if dma64 {
                    ptr::write_volatile(
                        adma2
                            .cast::<SdhcAdma2Descriptor64>()
                            .add(nsegs)
                            .cast::<u16>(),
                        0u16.to_le(),
                    );
                } else {
                    ptr::write_volatile(
                        adma2
                            .cast::<SdhcAdma2Descriptor32>()
                            .add(nsegs)
                            .cast::<u16>(),
                        0u16.to_le(),
                    );
                }
            }

            let Some(dmat) = sc.sc_dmat.get() else {
                panic(format_args!("{}: no DMA tag", hp.devname()));
            };
            bus_dmamap_sync(dmat, hp.adma_map(), 0, PAGE_SIZE, BUS_DMASYNC_PREWRITE);

            hclr1(hp, SDHC_HOST_CTL, SDHC_DMA_SELECT);
            if dma64 {
                hset1(hp, SDHC_HOST_CTL, SDHC_DMA_SELECT_ADMA64);
            } else {
                hset1(hp, SDHC_HOST_CTL, SDHC_DMA_SELECT_ADMA32);
            }

            let table = hp.adma_map().dm_segs()[0].get().ds_addr;
            hwrite4(hp, SDHC_ADMA_SYSTEM_ADDR, table as u32);
        }
        _ => hclr1(hp, SDHC_HOST_CTL, SDHC_DMA_SELECT),
    }

    // DPRINTF(1,("%s: cmd=%#x mode=%#x blksize=%d blkcount=%d\n", DEVNAME(hp->sc), command,
    //     mode, blksize, blkcount));

    // We're starting a new command, reset state.
    hp.intr_status.set(0);

    // Start a CPU data transfer. Writing to the high order byte of the SDHC_COMMAND
    // register triggers the SD command. (1.5)
    sdhc_write_2(hp, SDHC_TRANSFER_MODE, mode);
    sdhc_write_2(hp, SDHC_BLOCK_SIZE, blksize);
    sdhc_write_2(hp, SDHC_BLOCK_COUNT, blkcount);
    hwrite4(hp, SDHC_ARGUMENT, cmd.c_arg);
    sdhc_write_2(hp, SDHC_COMMAND, command);

    splx(s);
    Ok(())
}

/// `sdhc_transfer_data`: moves the command's data, by DMA or through the buffer port.
pub fn sdhc_transfer_data(hp: &SdhcHost, cmd: &mut SdmmcCommand) {
    let sc = hp.sc;
    let mut datap = cmd.c_data;

    let error: Result<(), Errno> = 'done: {
        if cmd.c_dmamap.is_some() {
            let mut error = Ok(());
            loop {
                let status = sdhc_wait_intr(
                    hp,
                    SDHC_DMA_INTERRUPT | SDHC_TRANSFER_COMPLETE,
                    SDHC_DMA_TIMEOUT,
                );
                if status & SDHC_TRANSFER_COMPLETE != 0 {
                    break;
                }
                if status == 0 {
                    error = Err(Errno::ETIMEDOUT);
                    break;
                }
            }

            if let (Some(dmat), Some(map)) = (sc.sc_dmat.get(), hp.adma_map.get()) {
                bus_dmamap_sync(dmat, map, 0, PAGE_SIZE, BUS_DMASYNC_POSTWRITE);
            }
            break 'done error;
        }

        let mask = if cmd.c_flags & SCF_CMD_READ != 0 {
            SDHC_BUFFER_READ_ENABLE
        } else {
            SDHC_BUFFER_WRITE_ENABLE
        };
        let mut error = Ok(());
        let mut datalen = cmd.c_datalen;

        // DPRINTF(1,("%s: resp=%#x datalen=%d\n", DEVNAME(hp->sc), MMC_R1(cmd->c_resp),
        //     datalen));

        // (SDHC_DEBUG: the CMD52/53 error response flags printf.)

        while datalen > 0 {
            if sdhc_wait_intr(
                hp,
                SDHC_BUFFER_READ_READY | SDHC_BUFFER_WRITE_READY,
                SDHC_BUFFER_TIMEOUT,
            ) == 0
            {
                error = Err(Errno::ETIMEDOUT);
                break;
            }

            if let Err(e) = sdhc_wait_state(hp, mask, mask) {
                error = Err(e);
                break;
            }

            let i = datalen.min(cmd.c_blklen);
            // SAFETY: `c_data` holds `c_datalen` bytes (the command's contract) and `datap`
            // walks them, `i` bytes at a time.
            unsafe {
                if cmd.c_flags & SCF_CMD_READ != 0 {
                    sdhc_read_data(hp, datap, i);
                } else {
                    sdhc_write_data(hp, datap, i);
                }
                datap = datap.add(i as usize);
            }

            datalen -= i;
        }

        if error.is_ok() && sdhc_wait_intr(hp, SDHC_TRANSFER_COMPLETE, SDHC_TRANSFER_TIMEOUT) == 0 {
            error = Err(Errno::ETIMEDOUT);
        }
        error
    };

    // done:
    if let Err(e) = error {
        cmd.c_error = Some(e);
    }
    cmd.c_flags |= SCF_ITSDONE;

    // DPRINTF(1,("%s: data transfer done (error=%d)\n", DEVNAME(hp->sc), cmd->c_error));
}

/// `sdhc_read_data`: reads `datalen` bytes from the buffer data port.
///
/// # Safety
///
/// `datap` points to `datalen` writable bytes.
pub unsafe fn sdhc_read_data(hp: &SdhcHost, datap: *mut u8, datalen: i32) {
    let mut datap = datap;
    let mut datalen = datalen;
    while datalen > 3 {
        // SAFETY: four of the caller's bytes, written unaligned.
        unsafe {
            ptr::write_unaligned(datap.cast::<u32>(), hread4(hp, SDHC_DATA));
            datap = datap.add(4);
        }
        datalen -= 4;
    }
    if datalen > 0 {
        let mut rv = hread4(hp, SDHC_DATA);
        loop {
            // SAFETY: one of the caller's remaining bytes.
            unsafe {
                *datap = (rv & 0xff) as u8;
                datap = datap.add(1);
            }
            rv >>= 8;
            datalen -= 1;
            if datalen <= 0 {
                break;
            }
        }
    }
}

/// `sdhc_write_data`: writes `datalen` bytes to the buffer data port.
///
/// # Safety
///
/// `datap` points to `datalen` readable bytes.
pub unsafe fn sdhc_write_data(hp: &SdhcHost, datap: *mut u8, datalen: i32) {
    let mut datap = datap.cast_const();
    let mut datalen = datalen;
    while datalen > 3 {
        // DPRINTF(3,("%08x\n", *(u_int32_t *)datap));
        // SAFETY: four of the caller's bytes, read unaligned.
        unsafe {
            hwrite4(hp, SDHC_DATA, ptr::read_unaligned(datap.cast::<u32>()));
            datap = datap.add(4);
        }
        datalen -= 4;
    }
    if datalen > 0 {
        // SAFETY: the caller's remaining one to three bytes.
        let rv = unsafe {
            let mut rv = u32::from(*datap);
            if datalen > 1 {
                rv |= u32::from(*datap.add(1)) << 8;
            }
            if datalen > 2 {
                rv |= u32::from(*datap.add(2)) << 16;
            }
            rv
        };
        // DPRINTF(3,("rv %08x\n", rv));
        hwrite4(hp, SDHC_DATA, rv);
    }
}

/// `sdhc_soft_reset`: prepares for another command.
pub fn sdhc_soft_reset(hp: &SdhcHost, mask: u8) -> Result<(), Errno> {
    // DPRINTF(1,("%s: software reset reg=%#x\n", DEVNAME(hp->sc), mask));

    sdhc_write_1(hp, SDHC_SOFTWARE_RESET, mask);
    let mut timo = 10;
    while timo > 0 {
        if sdhc_read_1(hp, SDHC_SOFTWARE_RESET) & mask == 0 {
            break;
        }
        sdmmc_delay(10000);
        sdhc_write_1(hp, SDHC_SOFTWARE_RESET, 0);
        timo -= 1;
    }
    if timo == 0 {
        // DPRINTF(1,("%s: timeout reg=%#x\n", DEVNAME(hp->sc),
        //     HREAD1(hp, SDHC_SOFTWARE_RESET)));
        sdhc_write_1(hp, SDHC_SOFTWARE_RESET, 0);
        return Err(Errno::ETIMEDOUT);
    }

    Ok(())
}

/// `sdhc_wait_intr_cold`: polls the interrupt status for `mask` (or an error) for up to
/// `secs` seconds, while interrupts cannot be taken.
pub fn sdhc_wait_intr_cold(hp: &SdhcHost, mask: u16, secs: i32) -> u16 {
    let mask = mask | SDHC_ERROR_INTERRUPT;
    let mut usecs = secs * 1000000;
    let mut status = hp.intr_status.get();
    while status & mask == 0 {
        status = sdhc_read_2(hp, SDHC_NINTR_STATUS);
        if status & SDHC_NINTR_STATUS_MASK != 0 {
            sdhc_write_2(hp, SDHC_NINTR_STATUS, status);
            if status & SDHC_ERROR_INTERRUPT != 0 {
                let error = sdhc_read_2(hp, SDHC_EINTR_STATUS);
                sdhc_write_2(hp, SDHC_EINTR_STATUS, error);
                hp.intr_status.set(hp.intr_status.get() | status);

                if error & (SDHC_CMD_TIMEOUT_ERROR | SDHC_DATA_TIMEOUT_ERROR) != 0 {
                    break;
                }
            }

            if status
                & (SDHC_BUFFER_READ_READY
                    | SDHC_BUFFER_WRITE_READY
                    | SDHC_COMMAND_COMPLETE
                    | SDHC_TRANSFER_COMPLETE)
                != 0
            {
                hp.intr_status.set(hp.intr_status.get() | status);
                break;
            }

            if status & SDHC_CARD_INTERRUPT != 0 {
                hset2(hp, SDHC_NINTR_STATUS_EN, SDHC_CARD_INTERRUPT);
            }

            continue;
        }

        delay(1);
        let u = usecs;
        usecs -= 1;
        if u == 0 {
            status |= SDHC_ERROR_INTERRUPT;
            break;
        }
    }

    hp.intr_status.set(hp.intr_status.get() & !(status & mask));
    status & mask
}

/// `sdhc_wait_intr`: sleeps until the soft interrupt status has a bit of `mask`; 0 on a
/// timeout or an error interrupt (after resetting the command and data lines).
pub fn sdhc_wait_intr(hp: &SdhcHost, mask: u16, secs: i32) -> u16 {
    if COLD.load(Ordering::Relaxed) {
        return sdhc_wait_intr_cold(hp, mask, secs);
    }

    let mask = mask | SDHC_ERROR_INTERRUPT;

    let s = splsdmmc();
    let mut status = hp.intr_status.get() & mask;
    while status == 0 {
        if tsleep_nsec(
            ptr::from_ref(&hp.intr_status),
            PWAIT,
            "hcintr",
            sec_to_nsec(secs as u64),
        ) == Err(Errno::EWOULDBLOCK)
        {
            status |= SDHC_ERROR_INTERRUPT;
            break;
        }
        status = hp.intr_status.get() & mask;
    }
    hp.intr_status.set(hp.intr_status.get() & !status);

    // DPRINTF(2,("%s: intr status %#x error %#x\n", DEVNAME(hp->sc), status,
    //     hp->intr_error_status));

    // Command timeout has higher priority than command complete.
    if status & SDHC_ERROR_INTERRUPT != 0 {
        hp.intr_error_status.set(0);
        let _ = sdhc_soft_reset(hp, SDHC_RESET_DAT | SDHC_RESET_CMD);
        status = 0;
    }

    splx(s);
    status
}

/// `sdhc_intr`: established by attachment driver at interrupt priority `IPL_SDMMC`.
pub fn sdhc_intr(arg: *mut c_void) -> i32 {
    if arg.is_null() {
        return 0;
    }
    // SAFETY: the attachment establishes the interrupt with its softc, which begins with the
    // `sdhc_softc` and is never freed while the device exists.
    let sc = unsafe { &*arg.cast::<SdhcSoftc>().cast_const() };
    let mut done = 0;

    // We got an interrupt, but we don't know from which slot.
    for host in 0..sc.sc_nhosts.get() {
        let Some(hp) = sc.host(host) else {
            continue;
        };

        // Find out which interrupts are pending.
        let status = sdhc_read_2(hp, SDHC_NINTR_STATUS);
        if status & SDHC_NINTR_STATUS_MASK == 0 {
            continue; // no interrupt for us
        }

        // Acknowledge the interrupts we are about to handle.
        sdhc_write_2(hp, SDHC_NINTR_STATUS, status);
        // DPRINTF(2,("%s: interrupt status=%b\n", DEVNAME(hp->sc), status,
        //     SDHC_NINTR_STATUS_BITS));

        // Claim this interrupt.
        done = 1;

        // Service error interrupts.
        if status & SDHC_ERROR_INTERRUPT != 0 {
            // Acknowledge error interrupts.
            let error = sdhc_read_2(hp, SDHC_EINTR_STATUS);
            sdhc_write_2(hp, SDHC_EINTR_STATUS, error);
            // DPRINTF(2,("%s: error interrupt, status=%b\n", DEVNAME(hp->sc), error,
            //     SDHC_EINTR_STATUS_BITS));

            if error & (SDHC_CMD_TIMEOUT_ERROR | SDHC_DATA_TIMEOUT_ERROR) != 0 {
                hp.intr_error_status.set(hp.intr_error_status.get() | error);
                hp.intr_status.set(hp.intr_status.get() | status);
                wakeup(ptr::from_ref(&hp.intr_status));
            }
        }

        // Wake up the sdmmc event thread to scan for cards.
        if status & (SDHC_CARD_REMOVAL | SDHC_CARD_INSERTION) != 0 {
            sdmmc_needs_discover(hp.sdmmc());
        }

        // Wake up the blocking process to service command related interrupt(s).
        if status
            & (SDHC_BUFFER_READ_READY
                | SDHC_BUFFER_WRITE_READY
                | SDHC_COMMAND_COMPLETE
                | SDHC_TRANSFER_COMPLETE)
            != 0
        {
            hp.intr_status.set(hp.intr_status.get() | status);
            wakeup(ptr::from_ref(&hp.intr_status));
        }

        // Service SD card interrupts.
        if status & SDHC_CARD_INTERRUPT != 0 {
            // DPRINTF(0,("%s: card interrupt\n", DEVNAME(hp->sc)));
            hclr2(hp, SDHC_NINTR_STATUS_EN, SDHC_CARD_INTERRUPT);
            sdmmc_card_intr(hp.sdmmc());
        }
    }
    done
}

/// `sdhc_needs_discover`: has every slot's bus look for cards again.
pub fn sdhc_needs_discover(sc: &SdhcSoftc) {
    for host in 0..sc.sc_nhosts.get() {
        if let Some(hp) = sc.host(host) {
            sdmmc_needs_discover(hp.sdmmc());
        }
    }
}

// SDHC_DEBUG: not configured (sdhc_dump_regs).

/// `sdhc_hibernate_init`: copies the host into the fake softc the hibernate code runs the
/// chip with.
///
/// # Safety
///
/// `fake_softc` points to writable memory with room for an [`SdhcHost`], suitably aligned.
pub unsafe fn sdhc_hibernate_init(
    sch: SdmmcChipsetHandle,
    fake_softc: *mut c_void,
) -> Result<(), Errno> {
    let hp = sdhc_host(sch);
    // SAFETY: the caller's guarantee.
    unsafe { fake_softc.cast::<SdhcHost>().write(hp.clone()) };

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::sdmmc::sdhcreg::SDHC_SPEC_V2;
    use crate::dev::sdmmc::sdmmcreg::{MMC_READ_BLOCK_MULTIPLE, MMC_SEND_CSD};
    use crate::dev::sdmmc::sdmmcvar::{SCF_CMD_AC, SCF_CMD_ADTC, SCF_RSP_R1, SCF_RSP_R2};

    #[test]
    fn clock_divisors() {
        // QEMU's 52 MHz v2 controller: 400 kHz needs /256 (div 128), 25 MHz /4 (div 2),
        // 50 MHz /2 (div 1), 52 MHz /1 (div 0).
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V2, 52000, 400), 128);
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V2, 52000, 25000), 2);
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V2, 52000, 50000), 1);
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V2, 52000, 52000), 0);
        // Too slow for a v2 divisor.
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V2, 200000, 100), -1);
        // v3: even divisors up to 2046.
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V3, 200000, 400), 250);
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V3, 200000, 200000), 0);
        assert_eq!(sdhc_clock_divisor(SDHC_SPEC_V3, 200000, 50000), 2);
    }

    #[test]
    fn bus_voltage_selection() {
        let host = MMC_OCR_3_2V_3_3V | MMC_OCR_3_3V_3_4V;
        assert_eq!(sdhc_select_vdd(host, host), Some(SDHC_VOLTAGE_3_3V));
        assert_eq!(sdhc_select_vdd(MMC_OCR_1_65V_1_95V, host), None);
        let host = host | MMC_OCR_2_9V_3_0V | MMC_OCR_1_65V_1_95V;
        assert_eq!(
            sdhc_select_vdd(MMC_OCR_2_9V_3_0V, host),
            Some(SDHC_VOLTAGE_3_0V)
        );
    }

    #[test]
    fn command_and_mode_registers() {
        let mut cmd = SdmmcCommand::new();
        cmd.c_opcode = MMC_SEND_CSD;
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R2;
        // Index 9, CRC check, 136-bit response.
        assert_eq!(sdhc_command_reg(&cmd), (9 << 8) | (1 << 3) | 1);
        assert_eq!(sdhc_transfer_mode(&cmd, 0, true), 0);

        let mut buf = [0u8; 4];
        cmd.c_opcode = MMC_READ_BLOCK_MULTIPLE;
        cmd.c_flags = SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1;
        cmd.c_data = buf.as_mut_ptr();
        cmd.c_datalen = 4096;
        // Index 18, index and CRC checks, data present, 48-bit response.
        assert_eq!(sdhc_command_reg(&cmd), (18 << 8) | 0x3a);
        // A PIO multi-block read: read, count, multi, auto CMD12; no DMA without a map.
        assert_eq!(
            sdhc_transfer_mode(&cmd, 8, true),
            SDHC_READ_MODE
                | SDHC_BLOCK_COUNT_ENABLE
                | SDHC_MULTI_BLOCK_MODE
                | SDHC_AUTO_CMD12_ENABLE
        );
        // CMD53 does not get the automatic CMD12.
        cmd.c_opcode = SD_IO_RW_EXTENDED;
        assert_eq!(
            sdhc_transfer_mode(&cmd, 2, false) & SDHC_AUTO_CMD12_ENABLE,
            0
        );
    }
}
/* </TESTS> */
