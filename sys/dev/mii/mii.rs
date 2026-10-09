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
 * Copyright (c) 1997 Manuel Bouyer.  All rights reserved.
 *
 * Modification to match BSD/OS 3.0 MII interface by Jason R. Thorpe,
 * Numerical Aerospace Simulation Facility, NASA Ames Research Center.
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

/*-
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
//! `<dev/mii/mii.h>` and `dev/mii/mii.c`: the registers common to all PHYs, and the MII bus
//! layer, which glues MII-capable network interface drivers to shareable PHY drivers. This
//! exports an interface compatible with BSD/OS 3.0's, plus some NetBSD extensions.
//!
//! Upstream: sys/dev/mii/mii.h @ 3ce1f3f79392, sys/dev/mii/mii.c @ 3ce1f3f79392
//!
//! `mii_attach` probes the PHY addresses with the driver's `mii_readreg` and attaches what
//! answers through `config_found_sm` on the `mii` attribute, whose locator is `phy`
//! (`conf/files`: `define mii {[phy = -1]}`); each ioconf lists the PHY drivers at `mii?`.
//!
//! ## Deviations
//! - The registers and their bits are `i32` (the C's `int`, what `PHY_READ` returns).
//!   `BMCR_SPEED(x)` and `BMSR_MEDIA_TO_ANAR(x)` are `const fn`s with lower-case names.
//! - `mii_mediachg` returns `Result<(), Errno>`.
//! - `mii_attach` borrows each attached child as a `MiiSoftc` (every driver at `mii` has one
//!   as its softc's head, the C's cast) and links it in with the list's `unsafe` insertion.
//! - `mii_detach` keeps the C's test of `offloc` against `MII_PHY_ANY` (both are -1).

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::mii::miivar::{
    MII_DOWN, MII_MEDIACHG, MII_OFFSET_ANY, MII_PHY_ANY, MII_POLLSTAT, MII_TICK, MIIF_INHERIT_MASK,
    MIIF_INITDONE, MiiAttachArgs, MiiData, MiiList, MiiSoftc, mii_model, mii_oui, mii_rev,
};
use crate::kern::subr_autoconf::{config_detach, config_found_sm};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::net::if_media::IFM_NONE;
use crate::sys::device::{CfMatch, DETACH_FORCE, Device, UNCONF};
use crate::sys::errno::Errno;

/// `MII_NPHY`: max # of PHYs per MII.
pub const MII_NPHY: i32 = 32;

/// `MII_COMMAND_START`.
pub const MII_COMMAND_START: i32 = 0x01;
/// `MII_COMMAND_READ`.
pub const MII_COMMAND_READ: i32 = 0x02;
/// `MII_COMMAND_WRITE`.
pub const MII_COMMAND_WRITE: i32 = 0x01;
/// `MII_COMMAND_ACK`.
pub const MII_COMMAND_ACK: i32 = 0x02;

/// `MII_BMCR`: Basic mode control register (rw).
pub const MII_BMCR: i32 = 0x00;
/// `BMCR_RESET`: reset.
pub const BMCR_RESET: i32 = 0x8000;
/// `BMCR_LOOP`: loopback.
pub const BMCR_LOOP: i32 = 0x4000;
/// `BMCR_SPEED0`: speed selection (LSB).
pub const BMCR_SPEED0: i32 = 0x2000;
/// `BMCR_AUTOEN`: autonegotiation enable.
pub const BMCR_AUTOEN: i32 = 0x1000;
/// `BMCR_PDOWN`: power down.
pub const BMCR_PDOWN: i32 = 0x0800;
/// `BMCR_ISO`: isolate.
pub const BMCR_ISO: i32 = 0x0400;
/// `BMCR_STARTNEG`: restart autonegotiation.
pub const BMCR_STARTNEG: i32 = 0x0200;
/// `BMCR_FDX`: Set duplex mode.
pub const BMCR_FDX: i32 = 0x0100;
/// `BMCR_CTEST`: collision test.
pub const BMCR_CTEST: i32 = 0x0080;
/// `BMCR_SPEED1`: speed selection (MSB).
pub const BMCR_SPEED1: i32 = 0x0040;

/// `BMCR_S10`: 10 Mb/s.
pub const BMCR_S10: i32 = 0x0000;
/// `BMCR_S100`: 100 Mb/s.
pub const BMCR_S100: i32 = BMCR_SPEED0;
/// `BMCR_S1000`: 1000 Mb/s.
pub const BMCR_S1000: i32 = BMCR_SPEED1;

/// `MII_BMSR`: Basic mode status register (ro).
pub const MII_BMSR: i32 = 0x01;
/// `BMSR_100T4`: 100 base T4 capable.
pub const BMSR_100T4: i32 = 0x8000;
/// `BMSR_100TXFDX`: 100 base Tx full duplex capable.
pub const BMSR_100TXFDX: i32 = 0x4000;
/// `BMSR_100TXHDX`: 100 base Tx half duplex capable.
pub const BMSR_100TXHDX: i32 = 0x2000;
/// `BMSR_10TFDX`: 10 base T full duplex capable.
pub const BMSR_10TFDX: i32 = 0x1000;
/// `BMSR_10THDX`: 10 base T half duplex capable.
pub const BMSR_10THDX: i32 = 0x0800;
/// `BMSR_MFPS`: MII Frame Preamble Suppression.
pub const BMSR_MFPS: i32 = 0x0040;
/// `BMSR_100T2FDX`: 100 base T2 full duplex capable.
pub const BMSR_100T2FDX: i32 = 0x0400;
/// `BMSR_100T2HDX`: 100 base T2 half duplex capable.
pub const BMSR_100T2HDX: i32 = 0x0200;
/// `BMSR_EXTSTAT`: Extended status in register 15.
pub const BMSR_EXTSTAT: i32 = 0x0100;
/// `BMSR_ACOMP`: Autonegotiation complete.
pub const BMSR_ACOMP: i32 = 0x0020;
/// `BMSR_RFAULT`: Link partner fault.
pub const BMSR_RFAULT: i32 = 0x0010;
/// `BMSR_ANEG`: Autonegotiation capable.
pub const BMSR_ANEG: i32 = 0x0008;
/// `BMSR_LINK`: Link status.
pub const BMSR_LINK: i32 = 0x0004;
/// `BMSR_JABBER`: Jabber detected.
pub const BMSR_JABBER: i32 = 0x0002;
/// `BMSR_EXTCAP`: Extended capability.
pub const BMSR_EXTCAP: i32 = 0x0001;

/// `BMSR_MEDIAMASK`.
pub const BMSR_MEDIAMASK: i32 = BMSR_100T4
    | BMSR_100TXFDX
    | BMSR_100TXHDX
    | BMSR_10TFDX
    | BMSR_10THDX
    | BMSR_100T2FDX
    | BMSR_100T2HDX;

/// `MII_PHYIDR1`: ID register 1 (ro).
pub const MII_PHYIDR1: i32 = 0x02;

/// `MII_PHYIDR2`: ID register 2 (ro).
pub const MII_PHYIDR2: i32 = 0x03;
/// `IDR2_OUILSB`: OUI LSB.
pub const IDR2_OUILSB: i32 = 0xfc00;
/// `IDR2_MODEL`: vendor model.
pub const IDR2_MODEL: i32 = 0x03f0;
/// `IDR2_REV`: vendor revision.
pub const IDR2_REV: i32 = 0x000f;

/// `MII_ANAR`: Autonegotiation advertisement (rw).
pub const MII_ANAR: i32 = 0x04;
// section 28.2.4.1 and 37.2.6.1
/// `ANAR_NP`: Next page (ro).
pub const ANAR_NP: i32 = 0x8000;
/// `ANAR_ACK`: link partner abilities acknowledged (ro).
pub const ANAR_ACK: i32 = 0x4000;
/// `ANAR_RF`: remote fault (ro).
pub const ANAR_RF: i32 = 0x2000;
/// `ANAR_FC`: local device supports PAUSE.
pub const ANAR_FC: i32 = 0x0400;
/// `ANAR_T4`: local device supports 100bT4.
pub const ANAR_T4: i32 = 0x0200;
/// `ANAR_TX_FD`: local device supports 100bTx FD.
pub const ANAR_TX_FD: i32 = 0x0100;
/// `ANAR_TX`: local device supports 100bTx.
pub const ANAR_TX: i32 = 0x0080;
/// `ANAR_10_FD`: local device supports 10bT FD.
pub const ANAR_10_FD: i32 = 0x0040;
/// `ANAR_10`: local device supports 10bT.
pub const ANAR_10: i32 = 0x0020;
/// `ANAR_CSMA`: protocol selector CSMA/CD.
pub const ANAR_CSMA: i32 = 0x0001;
/// `ANAR_PAUSE_NONE`.
pub const ANAR_PAUSE_NONE: i32 = 0 << 10;
/// `ANAR_PAUSE_SYM`.
pub const ANAR_PAUSE_SYM: i32 = 1 << 10;
/// `ANAR_PAUSE_ASYM`.
pub const ANAR_PAUSE_ASYM: i32 = 2 << 10;
/// `ANAR_PAUSE_TOWARDS`.
pub const ANAR_PAUSE_TOWARDS: i32 = 3 << 10;

/// `ANAR_X_FD`: local device supports 1000BASE-X FD.
pub const ANAR_X_FD: i32 = 0x0020;
/// `ANAR_X_HD`: local device supports 1000BASE-X HD.
pub const ANAR_X_HD: i32 = 0x0040;
/// `ANAR_X_PAUSE_NONE`.
pub const ANAR_X_PAUSE_NONE: i32 = 0 << 7;
/// `ANAR_X_PAUSE_SYM`.
pub const ANAR_X_PAUSE_SYM: i32 = 1 << 7;
/// `ANAR_X_PAUSE_ASYM`.
pub const ANAR_X_PAUSE_ASYM: i32 = 2 << 7;
/// `ANAR_X_PAUSE_TOWARDS`.
pub const ANAR_X_PAUSE_TOWARDS: i32 = 3 << 7;

/// `MII_ANLPAR`: Autonegotiation lnk partner abilities (rw).
pub const MII_ANLPAR: i32 = 0x05;
// section 28.2.4.1 and 37.2.6.1
/// `ANLPAR_NP`: Next page (ro).
pub const ANLPAR_NP: i32 = 0x8000;
/// `ANLPAR_ACK`: link partner accepted ACK (ro).
pub const ANLPAR_ACK: i32 = 0x4000;
/// `ANLPAR_RF`: remote fault (ro).
pub const ANLPAR_RF: i32 = 0x2000;
/// `ANLPAR_FC`: link partner supports PAUSE.
pub const ANLPAR_FC: i32 = 0x0400;
/// `ANLPAR_T4`: link partner supports 100bT4.
pub const ANLPAR_T4: i32 = 0x0200;
/// `ANLPAR_TX_FD`: link partner supports 100bTx FD.
pub const ANLPAR_TX_FD: i32 = 0x0100;
/// `ANLPAR_TX`: link partner supports 100bTx.
pub const ANLPAR_TX: i32 = 0x0080;
/// `ANLPAR_10_FD`: link partner supports 10bT FD.
pub const ANLPAR_10_FD: i32 = 0x0040;
/// `ANLPAR_10`: link partner supports 10bT.
pub const ANLPAR_10: i32 = 0x0020;
/// `ANLPAR_CSMA`: protocol selector CSMA/CD.
pub const ANLPAR_CSMA: i32 = 0x0001;
/// `ANLPAR_PAUSE_MASK`.
pub const ANLPAR_PAUSE_MASK: i32 = 3 << 10;
/// `ANLPAR_PAUSE_NONE`.
pub const ANLPAR_PAUSE_NONE: i32 = 0 << 10;
/// `ANLPAR_PAUSE_SYM`.
pub const ANLPAR_PAUSE_SYM: i32 = 1 << 10;
/// `ANLPAR_PAUSE_ASYM`.
pub const ANLPAR_PAUSE_ASYM: i32 = 2 << 10;
/// `ANLPAR_PAUSE_TOWARDS`.
pub const ANLPAR_PAUSE_TOWARDS: i32 = 3 << 10;

/// `ANLPAR_X_FD`: local device supports 1000BASE-X FD.
pub const ANLPAR_X_FD: i32 = 0x0020;
/// `ANLPAR_X_HD`: local device supports 1000BASE-X HD.
pub const ANLPAR_X_HD: i32 = 0x0040;
/// `ANLPAR_X_PAUSE_MASK`.
pub const ANLPAR_X_PAUSE_MASK: i32 = 3 << 7;
/// `ANLPAR_X_PAUSE_NONE`.
pub const ANLPAR_X_PAUSE_NONE: i32 = 0 << 7;
/// `ANLPAR_X_PAUSE_SYM`.
pub const ANLPAR_X_PAUSE_SYM: i32 = 1 << 7;
/// `ANLPAR_X_PAUSE_ASYM`.
pub const ANLPAR_X_PAUSE_ASYM: i32 = 2 << 7;
/// `ANLPAR_X_PAUSE_TOWARDS`.
pub const ANLPAR_X_PAUSE_TOWARDS: i32 = 3 << 7;

/// `MII_ANER`: Autonegotiation expansion (ro).
pub const MII_ANER: i32 = 0x06;
// section 28.2.4.1 and 37.2.6.1
/// `ANER_MLF`: multiple link detection fault.
pub const ANER_MLF: i32 = 0x0010;
/// `ANER_LPNP`: link parter next page-able.
pub const ANER_LPNP: i32 = 0x0008;
/// `ANER_NP`: next page-able.
pub const ANER_NP: i32 = 0x0004;
/// `ANER_PAGE_RX`: Page received.
pub const ANER_PAGE_RX: i32 = 0x0002;
/// `ANER_LPAN`: link parter autoneg-able.
pub const ANER_LPAN: i32 = 0x0001;

/// `MII_ANNP`: Autonegotiation next page.
pub const MII_ANNP: i32 = 0x07;
// section 28.2.4.1 and 37.2.6.1

/// `MII_ANLPRNP`: Autonegotiation link partner rx next page.
pub const MII_ANLPRNP: i32 = 0x08;
// section 32.5.1 and 37.2.6.1

// This is also the 1000baseT control register
/// `MII_100T2CR`: 100base-T2 control register.
pub const MII_100T2CR: i32 = 0x09;
/// `GTCR_TEST_MASK`: see 802.3ab ss. 40.6.1.1.2.
pub const GTCR_TEST_MASK: i32 = 0xe000;
/// `GTCR_MAN_MS`: enable manual master/slave control.
pub const GTCR_MAN_MS: i32 = 0x1000;
/// `GTCR_ADV_MS`: 1 = adv. master, 0 = adv. slave.
pub const GTCR_ADV_MS: i32 = 0x0800;
/// `GTCR_PORT_TYPE`: 1 = DCE, 0 = DTE (NIC).
pub const GTCR_PORT_TYPE: i32 = 0x0400;
/// `GTCR_ADV_1000TFDX`: adv. 1000baseT FDX.
pub const GTCR_ADV_1000TFDX: i32 = 0x0200;
/// `GTCR_ADV_1000THDX`: adv. 1000baseT HDX.
pub const GTCR_ADV_1000THDX: i32 = 0x0100;

// This is also the 1000baseT status register
/// `MII_100T2SR`: 100base-T2 status register.
pub const MII_100T2SR: i32 = 0x0a;
/// `GTSR_MAN_MS_FLT`: master/slave config fault.
pub const GTSR_MAN_MS_FLT: i32 = 0x8000;
/// `GTSR_MS_RES`: result: 1 = master, 0 = slave.
pub const GTSR_MS_RES: i32 = 0x4000;
/// `GTSR_LRS`: local rx status, 1 = ok.
pub const GTSR_LRS: i32 = 0x2000;
/// `GTSR_RRS`: remove rx status, 1 = ok.
pub const GTSR_RRS: i32 = 0x1000;
/// `GTSR_LP_1000TFDX`: link partner 1000baseT FDX capable.
pub const GTSR_LP_1000TFDX: i32 = 0x0800;
/// `GTSR_LP_1000THDX`: link partner 1000baseT HDX capable.
pub const GTSR_LP_1000THDX: i32 = 0x0400;
/// `GTSR_LP_ASM_DIR`: link partner asym. pause dir. capable.
pub const GTSR_LP_ASM_DIR: i32 = 0x0200;
/// `GTSR_IDLE_ERR`: IDLE error count.
pub const GTSR_IDLE_ERR: i32 = 0x00ff;

/// `MII_PSECR`: PSE control register.
pub const MII_PSECR: i32 = 0x0b;
/// `PSECR_PACTLMASK`: pair control mask.
pub const PSECR_PACTLMASK: i32 = 0x000c;
/// `PSECR_PSEENMASK`: PSE enable mask.
pub const PSECR_PSEENMASK: i32 = 0x0003;
/// `PSECR_PINOUTB`: PSE pinout Alternative B.
pub const PSECR_PINOUTB: i32 = 0x0008;
/// `PSECR_PINOUTA`: PSE pinout Alternative A.
pub const PSECR_PINOUTA: i32 = 0x0004;
/// `PSECR_FOPOWTST`: Force Power Test Mode.
pub const PSECR_FOPOWTST: i32 = 0x0002;
/// `PSECR_PSEEN`: PSE Enabled.
pub const PSECR_PSEEN: i32 = 0x0001;
/// `PSECR_PSEDIS`: PSE Disabled.
pub const PSECR_PSEDIS: i32 = 0x0000;

/// `MII_PSESR`: PSE status register.
pub const MII_PSESR: i32 = 0x0c;
/// `PSESR_PWRDENIED`: Power Denied.
pub const PSESR_PWRDENIED: i32 = 0x1000;
/// `PSESR_VALSIG`: Valid PD signature detected.
pub const PSESR_VALSIG: i32 = 0x0800;
/// `PSESR_INVALSIG`: Invalid PD signature detected.
pub const PSESR_INVALSIG: i32 = 0x0400;
/// `PSESR_SHORTCIRC`: Short circuit condition detected.
pub const PSESR_SHORTCIRC: i32 = 0x0200;
/// `PSESR_OVERLOAD`: Overload condition detected.
pub const PSESR_OVERLOAD: i32 = 0x0100;
/// `PSESR_MPSABSENT`: MPS absent condition detected.
pub const PSESR_MPSABSENT: i32 = 0x0080;
/// `PSESR_PDCLMASK`: PD Class mask.
pub const PSESR_PDCLMASK: i32 = 0x0070;
/// `PSESR_STATMASK`: PSE Status mask.
pub const PSESR_STATMASK: i32 = 0x000e;
/// `PSESR_PAIRCTABL`: PAIR Control Ability.
pub const PSESR_PAIRCTABL: i32 = 0x0001;
/// `PSESR_PDCL_4`: Class 4.
pub const PSESR_PDCL_4: i32 = 4 << 4;
/// `PSESR_PDCL_3`: Class 3.
pub const PSESR_PDCL_3: i32 = 3 << 4;
/// `PSESR_PDCL_2`: Class 2.
pub const PSESR_PDCL_2: i32 = 2 << 4;
/// `PSESR_PDCL_1`: Class 1.
pub const PSESR_PDCL_1: i32 = 1 << 4;
/// `PSESR_PDCL_0`: Class 0.
pub const PSESR_PDCL_0: i32 = 0 << 4;

/// `MII_MMDACR`: MMD access control register.
pub const MII_MMDACR: i32 = 0x0d;
/// `MMDACR_FUNCMASK`: function.
pub const MMDACR_FUNCMASK: i32 = 0xc000;
/// `MMDACR_DADDRMASK`: device address.
pub const MMDACR_DADDRMASK: i32 = 0x001f;
/// `MMDACR_FN_ADDRESS`: address.
pub const MMDACR_FN_ADDRESS: i32 = 0 << 14;
/// `MMDACR_FN_DATANPI`: data, no post increment.
pub const MMDACR_FN_DATANPI: i32 = 1 << 14;
/// `MMDACR_FN_DATAPIRW`: data, post increment on r/w.
pub const MMDACR_FN_DATAPIRW: i32 = 2 << 14;
/// `MMDACR_FN_DATAPIW`: data, post increment on wr only.
pub const MMDACR_FN_DATAPIW: i32 = 3 << 14;

/// `MII_MMDAADR`: MMD access address data register.
pub const MII_MMDAADR: i32 = 0x0e;

/// `MII_EXTSR`: Extended status register.
pub const MII_EXTSR: i32 = 0x0f;
/// `EXTSR_1000XFDX`: 1000X full-duplex capable.
pub const EXTSR_1000XFDX: i32 = 0x8000;
/// `EXTSR_1000XHDX`: 1000X half-duplex capable.
pub const EXTSR_1000XHDX: i32 = 0x4000;
/// `EXTSR_1000TFDX`: 1000T full-duplex capable.
pub const EXTSR_1000TFDX: i32 = 0x2000;
/// `EXTSR_1000THDX`: 1000T half-duplex capable.
pub const EXTSR_1000THDX: i32 = 0x1000;

/// `EXTSR_MEDIAMASK`.
pub const EXTSR_MEDIAMASK: i32 = EXTSR_1000XFDX | EXTSR_1000XHDX | EXTSR_1000TFDX | EXTSR_1000THDX;

/// `MIICF_PHY`: `cf_loc` index.
const MIICF_PHY: usize = 0;
/// `MIICF_PHY_DEFAULT`: default phy device.
const MIICF_PHY_DEFAULT: i64 = -1;

/// `BMCR_SPEED(x)`.
pub const fn bmcr_speed(x: i32) -> i32 {
    x & (BMCR_SPEED0 | BMCR_SPEED1)
}

/// `BMSR_MEDIA_TO_ANAR(x)`: convert BMSR media capabilities to ANAR bits for
/// autonegotiation. Note the shift chopps off the BMSR_ANEG bit.
pub const fn bmsr_media_to_anar(x: i32) -> i32 {
    (x & BMSR_MEDIAMASK) >> 6
}

/// `mii_attach`: helper function used by network interface drivers, attaches PHYs to the
/// network interface driver parent.
pub fn mii_attach(
    parent: &Device,
    mii: &'static MiiData,
    capmask: i32,
    phyloc: i32,
    offloc: i32,
    flags: i32,
) {
    let mut offset = 0;

    if phyloc != MII_PHY_ANY && offloc != MII_OFFSET_ANY {
        panic(format_args!("mii_attach: phyloc and offloc specified"));
    }

    let (phymin, phymax) = if phyloc == MII_PHY_ANY {
        (0, MII_NPHY - 1)
    } else {
        (phyloc, phyloc)
    };

    if mii.mii_flags.get() & MIIF_INITDONE == 0 {
        mii.mii_phys.init();
        mii.mii_flags.set(mii.mii_flags.get() | MIIF_INITDONE);
    }

    for phyno in phymin..=phymax {
        // Make sure we haven't already configured a PHY at this address. This allows
        // mii_attach() to be called multiple times.
        if mii
            .mii_phys
            .iter()
            .any(|child| child.mii_phy.get() == phyno)
        {
            // Yes, there is already something configured at this address.
            offset += 1;
            continue;
        }

        // Check to see if there is a PHY at this address. Note, many braindead PHYs report
        // 0/0 in their ID registers, so we test for media in the BMSR.
        let bmsr = mii.mii_readreg(parent, phyno, MII_BMSR);
        if bmsr == 0 || bmsr == 0xffff || (bmsr & (BMSR_MEDIAMASK | BMSR_EXTSTAT)) == 0 {
            // Assume no PHY at this address.
            continue;
        }

        // There is a PHY at this address. If we were given an `offset' locator, skip this
        // PHY if it doesn't match.
        if offloc != MII_OFFSET_ANY && offloc != offset {
            offset += 1;
            continue;
        }

        // Extract the IDs. Braindead PHYs will be handled by the `ukphy' driver, as we have
        // no ID information to match on.
        let mut ma = MiiAttachArgs {
            mii_data: mii,
            mii_phyno: phyno,
            mii_id1: mii.mii_readreg(parent, phyno, MII_PHYIDR1),
            mii_id2: mii.mii_readreg(parent, phyno, MII_PHYIDR2),
            mii_capmask: capmask,
            mii_flags: flags | (mii.mii_flags.get() & MIIF_INHERIT_MASK),
        };

        if let Some(child) = config_found_sm(
            parent,
            ptr::from_mut(&mut ma).cast(),
            Some(mii_print),
            Some(mii_submatch),
        ) {
            // SAFETY: every driver at `mii` has a `struct mii_softc` as its softc's head
            // (the C's cast); softcs are never freed while their devices exist.
            let child: &'static MiiSoftc = unsafe { child.cast::<MiiSoftc>().as_ref() };
            // Link it up in the parent's MII data.
            // SAFETY: a new device is in no PHY list; it and the parent's softc (which holds
            // the list head) stay in place until `mii_detach` unlinks it.
            unsafe { mii.mii_phys.insert_head(child) };
            child.mii_offset.set(offset);
            mii.mii_instance.set(mii.mii_instance.get() + 1);
        }
        offset += 1;
    }
}

/// `mii_detach`: detaches the PHYs at `phyloc` or `offloc` (all of them when both are any).
pub fn mii_detach(mii: &MiiData, phyloc: i32, offloc: i32) {
    if phyloc != MII_PHY_ANY && offloc != MII_PHY_ANY {
        panic(format_args!("mii_detach: phyloc and offloc specified"));
    }

    if mii.mii_flags.get() & MIIF_INITDONE == 0 {
        return;
    }

    for child in mii.mii_phys.iter() {
        if phyloc != MII_PHY_ANY || offloc != MII_OFFSET_ANY {
            if phyloc != MII_PHY_ANY && phyloc != child.mii_phy.get() {
                continue;
            }
            if offloc != MII_OFFSET_ANY && offloc != child.mii_offset.get() {
                continue;
            }
        }
        // SAFETY: `child` is on this list (the walk reads the next element first).
        unsafe { crate::sys::queue::ListHead::<MiiList>::remove(child) };
        // SAFETY: an attached device, now off the PHY list, which nothing else refers to.
        let _ = unsafe { config_detach(NonNull::from(&child.mii_dev), DETACH_FORCE) };
    }
}

/// `mii_print`.
pub fn mii_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!(
            "OUI 0x{:06x} model 0x{:04x} rev {} at {}",
            mii_oui(ma.mii_id1, ma.mii_id2),
            mii_model(ma.mii_id2),
            mii_rev(ma.mii_id2),
            Str(pnp)
        ));
    }

    printf(format_args!(" phy {}", ma.mii_phyno));
    UNCONF
}

/// `mii_submatch`: checks the `phy` locator before the driver's match.
pub fn mii_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: as in `mii_print`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    let loc = cf
        .cf_loc
        .get(MIICF_PHY)
        .copied()
        .unwrap_or(MIICF_PHY_DEFAULT);
    if i64::from(ma.mii_phyno) != loc && loc != MIICF_PHY_DEFAULT {
        return 0;
    }

    match cf.cf_attach.ca_match {
        Some(ca_match) => ca_match(parent, match_, aux),
        None => 0,
    }
}

/// `mii_mediachg`: media changed; notify all PHYs.
pub fn mii_mediachg(mii: &'static MiiData) -> Result<(), Errno> {
    mii.mii_media_status.set(0);
    mii.mii_media_active.set(IFM_NONE);

    for child in mii.mii_phys.iter() {
        // SAFETY: the PHYs on the list are attached devices, never freed while linked.
        let child: &'static MiiSoftc = unsafe { &*ptr::from_ref(child) };
        child.phy_service(mii, MII_MEDIACHG)?;
    }
    Ok(())
}

/// `mii_tick`: call the PHY tick routines, used during autonegotiation.
pub fn mii_tick(mii: &'static MiiData) {
    for child in mii.mii_phys.iter() {
        // SAFETY: as in `mii_mediachg`.
        let child: &'static MiiSoftc = unsafe { &*ptr::from_ref(child) };
        let _ = child.phy_service(mii, MII_TICK);
    }
}

/// `mii_pollstat`: get media status from PHYs.
pub fn mii_pollstat(mii: &'static MiiData) {
    mii.mii_media_status.set(0);
    mii.mii_media_active.set(IFM_NONE);

    for child in mii.mii_phys.iter() {
        // SAFETY: as in `mii_mediachg`.
        let child: &'static MiiSoftc = unsafe { &*ptr::from_ref(child) };
        let _ = child.phy_service(mii, MII_POLLSTAT);
    }
}

/// `mii_down`: inform the PHYs that the interface is down.
pub fn mii_down(mii: &'static MiiData) {
    for child in mii.mii_phys.iter() {
        // SAFETY: as in `mii_mediachg`.
        let child: &'static MiiSoftc = unsafe { &*ptr::from_ref(child) };
        let _ = child.phy_service(mii, MII_DOWN);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/mii/mii.h");
        crate::reftest::assert_defines!(defs; MII_NPHY, MII_COMMAND_START, MII_COMMAND_READ, MII_COMMAND_WRITE, MII_COMMAND_ACK, MII_BMCR, BMCR_RESET, BMCR_LOOP, BMCR_SPEED0, BMCR_AUTOEN, BMCR_PDOWN, BMCR_ISO, BMCR_STARTNEG, BMCR_FDX, BMCR_CTEST, BMCR_SPEED1, BMCR_S10, BMCR_S100, BMCR_S1000, MII_BMSR, BMSR_100T4, BMSR_100TXFDX, BMSR_100TXHDX, BMSR_10TFDX, BMSR_10THDX, BMSR_MFPS, BMSR_100T2FDX, BMSR_100T2HDX, BMSR_EXTSTAT, BMSR_ACOMP, BMSR_RFAULT, BMSR_ANEG, BMSR_LINK, BMSR_JABBER, BMSR_EXTCAP, MII_PHYIDR1, MII_PHYIDR2, IDR2_OUILSB, IDR2_MODEL, IDR2_REV, MII_ANAR, ANAR_NP, ANAR_ACK, ANAR_RF, ANAR_FC, ANAR_T4, ANAR_TX_FD, ANAR_TX, ANAR_10_FD, ANAR_10, ANAR_CSMA, ANAR_PAUSE_NONE, ANAR_PAUSE_SYM, ANAR_PAUSE_ASYM, ANAR_PAUSE_TOWARDS, ANAR_X_FD, ANAR_X_HD, ANAR_X_PAUSE_NONE, ANAR_X_PAUSE_SYM, ANAR_X_PAUSE_ASYM, ANAR_X_PAUSE_TOWARDS, MII_ANLPAR, ANLPAR_NP, ANLPAR_ACK, ANLPAR_RF, ANLPAR_FC, ANLPAR_T4, ANLPAR_TX_FD, ANLPAR_TX, ANLPAR_10_FD, ANLPAR_10, ANLPAR_CSMA, ANLPAR_PAUSE_MASK, ANLPAR_PAUSE_NONE, ANLPAR_PAUSE_SYM, ANLPAR_PAUSE_ASYM, ANLPAR_PAUSE_TOWARDS, ANLPAR_X_FD, ANLPAR_X_HD, ANLPAR_X_PAUSE_MASK, ANLPAR_X_PAUSE_NONE, ANLPAR_X_PAUSE_SYM, ANLPAR_X_PAUSE_ASYM, ANLPAR_X_PAUSE_TOWARDS, MII_ANER, ANER_MLF, ANER_LPNP, ANER_NP, ANER_PAGE_RX, ANER_LPAN, MII_ANNP, MII_ANLPRNP, MII_100T2CR, GTCR_TEST_MASK, GTCR_MAN_MS, GTCR_ADV_MS, GTCR_PORT_TYPE, GTCR_ADV_1000TFDX, GTCR_ADV_1000THDX, MII_100T2SR, GTSR_MAN_MS_FLT, GTSR_MS_RES, GTSR_LRS, GTSR_RRS, GTSR_LP_1000TFDX, GTSR_LP_1000THDX, GTSR_LP_ASM_DIR, GTSR_IDLE_ERR, MII_PSECR, PSECR_PACTLMASK, PSECR_PSEENMASK, PSECR_PINOUTB, PSECR_PINOUTA, PSECR_FOPOWTST, PSECR_PSEEN, PSECR_PSEDIS, MII_PSESR, PSESR_PWRDENIED, PSESR_VALSIG, PSESR_INVALSIG, PSESR_SHORTCIRC, PSESR_OVERLOAD, PSESR_MPSABSENT, PSESR_PDCLMASK, PSESR_STATMASK, PSESR_PAIRCTABL, PSESR_PDCL_4, PSESR_PDCL_3, PSESR_PDCL_2, PSESR_PDCL_1, PSESR_PDCL_0, MII_MMDACR, MMDACR_FUNCMASK, MMDACR_DADDRMASK, MMDACR_FN_ADDRESS, MMDACR_FN_DATANPI, MMDACR_FN_DATAPIRW, MMDACR_FN_DATAPIW, MII_MMDAADR, MII_EXTSR, EXTSR_1000XFDX, EXTSR_1000XHDX, EXTSR_1000TFDX, EXTSR_1000THDX);
        // The two multi-line masks, which the define reader does not join.
        assert_eq!(BMSR_MEDIAMASK, 0xfe00);
        assert_eq!(EXTSR_MEDIAMASK, 0xf000);
    }

    #[test]
    fn media_to_anar_drops_the_aneg_bit() {
        let bmsr = BMSR_100TXFDX | BMSR_100TXHDX | BMSR_10TFDX | BMSR_10THDX | BMSR_ANEG;
        assert_eq!(
            bmsr_media_to_anar(bmsr),
            ANAR_TX_FD | ANAR_TX | ANAR_10_FD | ANAR_10
        );
        assert_eq!(bmcr_speed(BMCR_S100 | BMCR_FDX | BMCR_AUTOEN), BMCR_S100);
    }
}
/* </TESTS> */
