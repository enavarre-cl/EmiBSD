/*	$OpenBSD: if_media.h,v 1.47 2026/03/19 16:50:32 chris Exp $	*/
/*	$NetBSD: if_media.h,v 1.22 2000/02/17 21:53:16 sommerfeld Exp $	*/
/*	$OpenBSD: if_media.c,v 1.40 2025/07/07 02:28:50 jsg Exp $	*/
/*	$NetBSD: if_media.c,v 1.10 2000/03/13 23:52:39 soren Exp $	*/
/* <LICENSES> */
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

/*
 * Copyright (c) 1997
 *	Jonathan Stone and Jason R. Thorpe.  All rights reserved.
 *
 * This software is derived from information provided by Matt Thomas.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Jonathan Stone
 *	and Jason R. Thorpe for the NetBSD Project.
 * 4. The names of the authors may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
 * AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
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

/*
 * Copyright (c) 1997
 *	Jonathan Stone and Jason R. Thorpe.  All rights reserved.
 *
 * This software is derived from information provided by Matt Thomas.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Jonathan Stone
 *	and Jason R. Thorpe for the NetBSD Project.
 * 4. The names of the authors may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
 * AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<net/if_media.h>` and `net/if_media.c`: network interface media selection, BSD/OS
//! compatible. The media words (`IFM_*`), `struct ifmedia` (the list of media a driver
//! supports and the one selected) and the functions drivers call to fill it and to answer
//! `SIOCGIFMEDIA`/`SIOCSIFMEDIA`, plus `ifmedia_baudrate`, the interface speed of a media word.
//!
//! Upstream: sys/net/if_media.h @ 3ce1f3f79392, sys/net/if_media.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The description tables (`struct ifmedia_description`, `IFM_TYPE_DESCRIPTIONS`,
//!   `IFM_SUBTYPE_DESCRIPTIONS`, `IFM_MODE_DESCRIPTIONS`, `IFM_OPTION_DESCRIPTIONS`,
//!   `struct ifmedia_status_description`, `IFM_STATUS_DESC`, `IFM_STATUS_DESCRIPTIONS`,
//!   `IFM_STATUS_VALID_LIST`) are left out: ifconfig(8) builds them from the C header, and the
//!   kernel reads them only under `IFMEDIA_DEBUG`, which is not defined (so `ifmedia_debug`
//!   and `ifmedia_printword` are left out as well).
//! - `struct ifmedia` is made of `Cell`s, so a zeroed softc holds a valid one; the callbacks
//!   are `Option<fn>`s, `None` until `ifmedia_init`. `ifm_cur`, `ifm_list` and `ifm_nwords`
//!   change only under `ifmedia_mtx`, as in C; `ifm_mask` and `ifm_media` as in C too (the
//!   attach and `ifmedia_ioctl`). The entries are `malloc`ed and freed only by
//!   `ifmedia_delete_instance`, which clears `ifm_cur` under the mutex.
//! - `ifmedia_ioctl` is an `unsafe fn` that takes the ioctl's `data` (a `struct ifreq` for
//!   `SIOCSIFMEDIA`, a `struct ifmediareq` for `SIOCGIFMEDIA`), with `sys_ioctl`'s contract,
//!   as a driver's `if_ioctl` does; the C's `NULL` checks of its pointers become a null
//!   check of `data`. `ifmedia_add`'s `int data` is stored as `u_int`, as in C.
//! - `ifmedia_get`'s "multiple match" message is printed under feature `diagnostic`
//!   (`DIAGNOSTIC`; `IFMEDIA_DEBUG` is not defined).
//! - Functions returning 0 or an errno return `Result<(), Errno>`; `ifmedia_match` returns
//!   `bool`. `IFM_TYPE`, `IFM_SUBTYPE`, `IFM_INST`, `IFM_OPTIONS`, `IFM_MODE`,
//!   `IFM_MAKEWORD` and `IFM_TYPE_MATCH` are `const fn`s with lower-case names.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::copy::copyout;
use crate::machine::intr::IPL_NET;
use crate::net::if_::{Ifmediareq, Ifreq, if_gbps, if_kbps, if_mbps};
use crate::net::if_var::Ifnet;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IFADDR, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCSIFMEDIA};

// Ethernet

/// `IFM_ETHER`.
pub const IFM_ETHER: u64 = 0x0000_0000_0000_0100;
/// `IFM_10_T`: 10BaseT - RJ45
pub const IFM_10_T: u64 = 3;
/// `IFM_10_2`: 10Base2 - Thinnet
pub const IFM_10_2: u64 = 4;
/// `IFM_10_5`: 10Base5 - AUI
pub const IFM_10_5: u64 = 5;
/// `IFM_100_TX`: 100BaseTX - RJ45
pub const IFM_100_TX: u64 = 6;
/// `IFM_100_FX`: 100BaseFX - Fiber
pub const IFM_100_FX: u64 = 7;
/// `IFM_100_T4`: 100BaseT4 - 4 pair cat 3
pub const IFM_100_T4: u64 = 8;
/// `IFM_100_VG`: 100VG-AnyLAN
pub const IFM_100_VG: u64 = 9;
/// `IFM_100_T2`: 100BaseT2
pub const IFM_100_T2: u64 = 10;
/// `IFM_1000_SX`: 1000BaseSX - multi-mode fiber
pub const IFM_1000_SX: u64 = 11;
/// `IFM_10_STP`: 10BaseT over shielded TP
pub const IFM_10_STP: u64 = 12;
/// `IFM_10_FL`: 10BaseFL - Fiber
pub const IFM_10_FL: u64 = 13;
/// `IFM_1000_LX`: 1000baseLX - single-mode fiber
pub const IFM_1000_LX: u64 = 14;
/// `IFM_1000_CX`: 1000baseCX - 150ohm STP
pub const IFM_1000_CX: u64 = 15;
/// `IFM_1000_T`: 1000baseT - 4 pair cat 5
pub const IFM_1000_T: u64 = 16;
/// `IFM_1000_TX`: for backwards compatibility.
pub const IFM_1000_TX: u64 = IFM_1000_T;
/// `IFM_HPNA_1`: HomePNA 1.0 (1Mb/s)
pub const IFM_HPNA_1: u64 = 17;
/// `IFM_10G_LR`: 10GBase-LR - single-mode fiber
pub const IFM_10G_LR: u64 = 18;
/// `IFM_10G_SR`: 10GBase-SR - multi-mode fiber
pub const IFM_10G_SR: u64 = 19;
/// `IFM_10G_CX4`: 10GBase-CX4 - copper
pub const IFM_10G_CX4: u64 = 20;
/// `IFM_2500_SX`: 2500baseSX - multi-mode fiber
pub const IFM_2500_SX: u64 = 21;
/// `IFM_10G_T`: 10GbaseT cat 6
pub const IFM_10G_T: u64 = 22;
/// `IFM_10G_SFP_CU`: 10G SFP+ direct attached cable
pub const IFM_10G_SFP_CU: u64 = 23;
/// `IFM_10G_LRM`: 10GBase-LRM 850nm Multi-mode
pub const IFM_10G_LRM: u64 = 24;
/// `IFM_40G_CR4`: 40GBase-CR4
pub const IFM_40G_CR4: u64 = 25;
/// `IFM_40G_SR4`: 40GBase-SR4
pub const IFM_40G_SR4: u64 = 26;
/// `IFM_40G_LR4`: 40GBase-LR4
pub const IFM_40G_LR4: u64 = 27;
/// `IFM_1000_KX`: 1000Base-KX backplane
pub const IFM_1000_KX: u64 = 28;
/// `IFM_10G_KX4`: 10GBase-KX4 backplane
pub const IFM_10G_KX4: u64 = 29;
/// `IFM_10G_KR`: 10GBase-KR backplane
pub const IFM_10G_KR: u64 = 30;
/// `IFM_10G_CR1`: 10GBase-CR1 Twinax splitter
pub const IFM_10G_CR1: u64 = 31;
/// `IFM_20G_KR2`: 20GBase-KR2 backplane
pub const IFM_20G_KR2: u64 = 32;
/// `IFM_2500_KX`: 2500Base-KX backplane
pub const IFM_2500_KX: u64 = 33;
/// `IFM_2500_T`: 2500Base-T - RJ45 (NBaseT)
pub const IFM_2500_T: u64 = 34;
/// `IFM_5000_T`: 5000Base-T - RJ45 (NBaseT)
pub const IFM_5000_T: u64 = 35;
/// `IFM_1000_SGMII`: 1G media interface
pub const IFM_1000_SGMII: u64 = 36;
/// `IFM_10G_SFI`: 10G media interface
pub const IFM_10G_SFI: u64 = 37;
/// `IFM_40G_XLPPI`: 40G media interface
pub const IFM_40G_XLPPI: u64 = 38;
/// `IFM_1000_CX_SGMII`: 1000Base-CX-SGMII
pub const IFM_1000_CX_SGMII: u64 = 39;
/// `IFM_40G_KR4`: 40GBase-KR4
pub const IFM_40G_KR4: u64 = 40;
/// `IFM_10G_ER`: 10GBase-ER
pub const IFM_10G_ER: u64 = 41;
/// `IFM_100G_CR4`: 100GBase-CR4
pub const IFM_100G_CR4: u64 = 42;
/// `IFM_100G_SR4`: 100GBase-SR4
pub const IFM_100G_SR4: u64 = 43;
/// `IFM_100G_KR4`: 100GBase-KR4
pub const IFM_100G_KR4: u64 = 44;
/// `IFM_100G_LR4`: 100GBase-LR4
pub const IFM_100G_LR4: u64 = 45;
/// `IFM_56G_R4`: 56GBase-R4
pub const IFM_56G_R4: u64 = 46;
/// `IFM_25G_CR`: 25GBase-CR
pub const IFM_25G_CR: u64 = 47;
/// `IFM_25G_KR`: 25GBase-KR
pub const IFM_25G_KR: u64 = 48;
/// `IFM_25G_SR`: 25GBase-SR
pub const IFM_25G_SR: u64 = 49;
/// `IFM_50G_CR2`: 50GBase-CR2
pub const IFM_50G_CR2: u64 = 50;
/// `IFM_50G_KR2`: 50GBase-KR2
pub const IFM_50G_KR2: u64 = 51;
/// `IFM_25G_LR`: 25GBase-LR
pub const IFM_25G_LR: u64 = 52;
/// `IFM_25G_ER`: 25GBase-ER
pub const IFM_25G_ER: u64 = 53;
/// `IFM_10G_AOC`: 10G Active Optical Cable
pub const IFM_10G_AOC: u64 = 54;
/// `IFM_25G_AOC`: 25G Active Optical Cable
pub const IFM_25G_AOC: u64 = 55;
/// `IFM_40G_AOC`: 40G Active Optical Cable
pub const IFM_40G_AOC: u64 = 56;
/// `IFM_100G_AOC`: 100G Active Optical Cable
pub const IFM_100G_AOC: u64 = 57;
/// `IFM_ETH_MASTER`: master mode (1000baseT)
pub const IFM_ETH_MASTER: u64 = 0x0000_0000_0001_0000;
/// `IFM_ETH_RXPAUSE`: receive PAUSE frames
pub const IFM_ETH_RXPAUSE: u64 = 0x0000_0000_0002_0000;
/// `IFM_ETH_TXPAUSE`: transmit PAUSE frames
pub const IFM_ETH_TXPAUSE: u64 = 0x0000_0000_0004_0000;

// FDDI

/// `IFM_FDDI`.
pub const IFM_FDDI: u64 = 0x0000_0000_0000_0300;
/// `IFM_FDDI_SMF`: Single-mode fiber
pub const IFM_FDDI_SMF: u64 = 3;
/// `IFM_FDDI_MMF`: Multi-mode fiber
pub const IFM_FDDI_MMF: u64 = 4;
/// `IFM_FDDI_UTP`: CDDI / UTP
pub const IFM_FDDI_UTP: u64 = 5;
/// `IFM_FDDI_DA`: Dual attach / single attach
pub const IFM_FDDI_DA: u64 = 0x00000100;

// IEEE 802.11 Wireless

/// `IFM_IEEE80211`.
pub const IFM_IEEE80211: u64 = 0x0000_0000_0000_0400;
/// `IFM_IEEE80211_FH1`: Frequency Hopping 1Mbps
pub const IFM_IEEE80211_FH1: u64 = 3;
/// `IFM_IEEE80211_FH2`: Frequency Hopping 2Mbps
pub const IFM_IEEE80211_FH2: u64 = 4;
/// `IFM_IEEE80211_DS2`: Direct Sequence 2Mbps
pub const IFM_IEEE80211_DS2: u64 = 5;
/// `IFM_IEEE80211_DS5`: Direct Sequence 5Mbps
pub const IFM_IEEE80211_DS5: u64 = 6;
/// `IFM_IEEE80211_DS11`: Direct Sequence 11Mbps
pub const IFM_IEEE80211_DS11: u64 = 7;
/// `IFM_IEEE80211_DS1`: Direct Sequence  1Mbps
pub const IFM_IEEE80211_DS1: u64 = 8;
/// `IFM_IEEE80211_DS22`: Direct Sequence 22Mbps
pub const IFM_IEEE80211_DS22: u64 = 9;
/// `IFM_IEEE80211_OFDM6`: OFDM 6Mbps
pub const IFM_IEEE80211_OFDM6: u64 = 10;
/// `IFM_IEEE80211_OFDM9`: OFDM 9Mbps
pub const IFM_IEEE80211_OFDM9: u64 = 11;
/// `IFM_IEEE80211_OFDM12`: OFDM 12Mbps
pub const IFM_IEEE80211_OFDM12: u64 = 12;
/// `IFM_IEEE80211_OFDM18`: OFDM 18Mbps
pub const IFM_IEEE80211_OFDM18: u64 = 13;
/// `IFM_IEEE80211_OFDM24`: OFDM 24Mbps
pub const IFM_IEEE80211_OFDM24: u64 = 14;
/// `IFM_IEEE80211_OFDM36`: OFDM 36Mbps
pub const IFM_IEEE80211_OFDM36: u64 = 15;
/// `IFM_IEEE80211_OFDM48`: OFDM 48Mbps
pub const IFM_IEEE80211_OFDM48: u64 = 16;
/// `IFM_IEEE80211_OFDM54`: OFDM 54Mbps
pub const IFM_IEEE80211_OFDM54: u64 = 17;
/// `IFM_IEEE80211_OFDM72`: OFDM 72Mbps
pub const IFM_IEEE80211_OFDM72: u64 = 18;
/// `IFM_IEEE80211_HT_MCS0`: 11n MCS 0
pub const IFM_IEEE80211_HT_MCS0: u64 = 19;
/// `IFM_IEEE80211_HT_MCS1`: 11n MCS 1
pub const IFM_IEEE80211_HT_MCS1: u64 = 20;
/// `IFM_IEEE80211_HT_MCS2`: 11n MCS 2
pub const IFM_IEEE80211_HT_MCS2: u64 = 21;
/// `IFM_IEEE80211_HT_MCS3`: 11n MCS 3
pub const IFM_IEEE80211_HT_MCS3: u64 = 22;
/// `IFM_IEEE80211_HT_MCS4`: 11n MCS 4
pub const IFM_IEEE80211_HT_MCS4: u64 = 23;
/// `IFM_IEEE80211_HT_MCS5`: 11n MCS 5
pub const IFM_IEEE80211_HT_MCS5: u64 = 24;
/// `IFM_IEEE80211_HT_MCS6`: 11n MCS 6
pub const IFM_IEEE80211_HT_MCS6: u64 = 25;
/// `IFM_IEEE80211_HT_MCS7`: 11n MCS 7
pub const IFM_IEEE80211_HT_MCS7: u64 = 26;
/// `IFM_IEEE80211_HT_MCS8`: 11n MCS 8
pub const IFM_IEEE80211_HT_MCS8: u64 = 27;
/// `IFM_IEEE80211_HT_MCS9`: 11n MCS 9
pub const IFM_IEEE80211_HT_MCS9: u64 = 28;
/// `IFM_IEEE80211_HT_MCS10`: 11n MCS 10
pub const IFM_IEEE80211_HT_MCS10: u64 = 29;
/// `IFM_IEEE80211_HT_MCS11`: 11n MCS 11
pub const IFM_IEEE80211_HT_MCS11: u64 = 30;
/// `IFM_IEEE80211_HT_MCS12`: 11n MCS 12
pub const IFM_IEEE80211_HT_MCS12: u64 = 31;
/// `IFM_IEEE80211_HT_MCS13`: 11n MCS 13
pub const IFM_IEEE80211_HT_MCS13: u64 = 32;
/// `IFM_IEEE80211_HT_MCS14`: 11n MCS 14
pub const IFM_IEEE80211_HT_MCS14: u64 = 33;
/// `IFM_IEEE80211_HT_MCS15`: 11n MCS 15
pub const IFM_IEEE80211_HT_MCS15: u64 = 34;
/// `IFM_IEEE80211_HT_MCS16`: 11n MCS 16
pub const IFM_IEEE80211_HT_MCS16: u64 = 35;
/// `IFM_IEEE80211_HT_MCS17`: 11n MCS 17
pub const IFM_IEEE80211_HT_MCS17: u64 = 36;
/// `IFM_IEEE80211_HT_MCS18`: 11n MCS 18
pub const IFM_IEEE80211_HT_MCS18: u64 = 37;
/// `IFM_IEEE80211_HT_MCS19`: 11n MCS 19
pub const IFM_IEEE80211_HT_MCS19: u64 = 38;
/// `IFM_IEEE80211_HT_MCS20`: 11n MCS 20
pub const IFM_IEEE80211_HT_MCS20: u64 = 39;
/// `IFM_IEEE80211_HT_MCS21`: 11n MCS 21
pub const IFM_IEEE80211_HT_MCS21: u64 = 40;
/// `IFM_IEEE80211_HT_MCS22`: 11n MCS 22
pub const IFM_IEEE80211_HT_MCS22: u64 = 41;
/// `IFM_IEEE80211_HT_MCS23`: 11n MCS 23
pub const IFM_IEEE80211_HT_MCS23: u64 = 42;
/// `IFM_IEEE80211_HT_MCS24`: 11n MCS 24
pub const IFM_IEEE80211_HT_MCS24: u64 = 43;
/// `IFM_IEEE80211_HT_MCS25`: 11n MCS 25
pub const IFM_IEEE80211_HT_MCS25: u64 = 44;
/// `IFM_IEEE80211_HT_MCS26`: 11n MCS 26
pub const IFM_IEEE80211_HT_MCS26: u64 = 45;
/// `IFM_IEEE80211_HT_MCS27`: 11n MCS 27
pub const IFM_IEEE80211_HT_MCS27: u64 = 46;
/// `IFM_IEEE80211_HT_MCS28`: 11n MCS 28
pub const IFM_IEEE80211_HT_MCS28: u64 = 47;
/// `IFM_IEEE80211_HT_MCS29`: 11n MCS 29
pub const IFM_IEEE80211_HT_MCS29: u64 = 48;
/// `IFM_IEEE80211_HT_MCS30`: 11n MCS 30
pub const IFM_IEEE80211_HT_MCS30: u64 = 49;
/// `IFM_IEEE80211_HT_MCS31`: 11n MCS 31
pub const IFM_IEEE80211_HT_MCS31: u64 = 50;
/// `IFM_IEEE80211_HT_MCS32`: 11n MCS 32
pub const IFM_IEEE80211_HT_MCS32: u64 = 51;
/// `IFM_IEEE80211_HT_MCS33`: 11n MCS 33
pub const IFM_IEEE80211_HT_MCS33: u64 = 52;
/// `IFM_IEEE80211_HT_MCS34`: 11n MCS 34
pub const IFM_IEEE80211_HT_MCS34: u64 = 53;
/// `IFM_IEEE80211_HT_MCS35`: 11n MCS 35
pub const IFM_IEEE80211_HT_MCS35: u64 = 54;
/// `IFM_IEEE80211_HT_MCS36`: 11n MCS 36
pub const IFM_IEEE80211_HT_MCS36: u64 = 55;
/// `IFM_IEEE80211_HT_MCS37`: 11n MCS 37
pub const IFM_IEEE80211_HT_MCS37: u64 = 56;
/// `IFM_IEEE80211_HT_MCS38`: 11n MCS 38
pub const IFM_IEEE80211_HT_MCS38: u64 = 57;
/// `IFM_IEEE80211_HT_MCS39`: 11n MCS 39
pub const IFM_IEEE80211_HT_MCS39: u64 = 58;
/// `IFM_IEEE80211_HT_MCS40`: 11n MCS 40
pub const IFM_IEEE80211_HT_MCS40: u64 = 59;
/// `IFM_IEEE80211_HT_MCS41`: 11n MCS 41
pub const IFM_IEEE80211_HT_MCS41: u64 = 60;
/// `IFM_IEEE80211_HT_MCS42`: 11n MCS 42
pub const IFM_IEEE80211_HT_MCS42: u64 = 61;
/// `IFM_IEEE80211_HT_MCS43`: 11n MCS 43
pub const IFM_IEEE80211_HT_MCS43: u64 = 62;
/// `IFM_IEEE80211_HT_MCS44`: 11n MCS 44
pub const IFM_IEEE80211_HT_MCS44: u64 = 63;
/// `IFM_IEEE80211_HT_MCS45`: 11n MCS 45
pub const IFM_IEEE80211_HT_MCS45: u64 = 64;
/// `IFM_IEEE80211_HT_MCS46`: 11n MCS 46
pub const IFM_IEEE80211_HT_MCS46: u64 = 65;
/// `IFM_IEEE80211_HT_MCS47`: 11n MCS 47
pub const IFM_IEEE80211_HT_MCS47: u64 = 66;
/// `IFM_IEEE80211_HT_MCS48`: 11n MCS 48
pub const IFM_IEEE80211_HT_MCS48: u64 = 67;
/// `IFM_IEEE80211_HT_MCS49`: 11n MCS 49
pub const IFM_IEEE80211_HT_MCS49: u64 = 68;
/// `IFM_IEEE80211_HT_MCS50`: 11n MCS 50
pub const IFM_IEEE80211_HT_MCS50: u64 = 69;
/// `IFM_IEEE80211_HT_MCS51`: 11n MCS 51
pub const IFM_IEEE80211_HT_MCS51: u64 = 70;
/// `IFM_IEEE80211_HT_MCS52`: 11n MCS 52
pub const IFM_IEEE80211_HT_MCS52: u64 = 71;
/// `IFM_IEEE80211_HT_MCS53`: 11n MCS 53
pub const IFM_IEEE80211_HT_MCS53: u64 = 72;
/// `IFM_IEEE80211_HT_MCS54`: 11n MCS 54
pub const IFM_IEEE80211_HT_MCS54: u64 = 73;
/// `IFM_IEEE80211_HT_MCS55`: 11n MCS 55
pub const IFM_IEEE80211_HT_MCS55: u64 = 74;
/// `IFM_IEEE80211_HT_MCS56`: 11n MCS 56
pub const IFM_IEEE80211_HT_MCS56: u64 = 75;
/// `IFM_IEEE80211_HT_MCS57`: 11n MCS 57
pub const IFM_IEEE80211_HT_MCS57: u64 = 76;
/// `IFM_IEEE80211_HT_MCS58`: 11n MCS 58
pub const IFM_IEEE80211_HT_MCS58: u64 = 77;
/// `IFM_IEEE80211_HT_MCS59`: 11n MCS 59
pub const IFM_IEEE80211_HT_MCS59: u64 = 78;
/// `IFM_IEEE80211_HT_MCS60`: 11n MCS 60
pub const IFM_IEEE80211_HT_MCS60: u64 = 79;
/// `IFM_IEEE80211_HT_MCS61`: 11n MCS 61
pub const IFM_IEEE80211_HT_MCS61: u64 = 80;
/// `IFM_IEEE80211_HT_MCS62`: 11n MCS 62
pub const IFM_IEEE80211_HT_MCS62: u64 = 81;
/// `IFM_IEEE80211_HT_MCS63`: 11n MCS 63
pub const IFM_IEEE80211_HT_MCS63: u64 = 82;
/// `IFM_IEEE80211_HT_MCS64`: 11n MCS 64
pub const IFM_IEEE80211_HT_MCS64: u64 = 83;
/// `IFM_IEEE80211_HT_MCS65`: 11n MCS 65
pub const IFM_IEEE80211_HT_MCS65: u64 = 84;
/// `IFM_IEEE80211_HT_MCS66`: 11n MCS 66
pub const IFM_IEEE80211_HT_MCS66: u64 = 85;
/// `IFM_IEEE80211_HT_MCS67`: 11n MCS 67
pub const IFM_IEEE80211_HT_MCS67: u64 = 86;
/// `IFM_IEEE80211_HT_MCS68`: 11n MCS 68
pub const IFM_IEEE80211_HT_MCS68: u64 = 87;
/// `IFM_IEEE80211_HT_MCS69`: 11n MCS 69
pub const IFM_IEEE80211_HT_MCS69: u64 = 88;
/// `IFM_IEEE80211_HT_MCS70`: 11n MCS 70
pub const IFM_IEEE80211_HT_MCS70: u64 = 89;
/// `IFM_IEEE80211_HT_MCS71`: 11n MCS 71
pub const IFM_IEEE80211_HT_MCS71: u64 = 90;
/// `IFM_IEEE80211_HT_MCS72`: 11n MCS 72
pub const IFM_IEEE80211_HT_MCS72: u64 = 91;
/// `IFM_IEEE80211_HT_MCS73`: 11n MCS 73
pub const IFM_IEEE80211_HT_MCS73: u64 = 92;
/// `IFM_IEEE80211_HT_MCS74`: 11n MCS 74
pub const IFM_IEEE80211_HT_MCS74: u64 = 93;
/// `IFM_IEEE80211_HT_MCS75`: 11n MCS 75
pub const IFM_IEEE80211_HT_MCS75: u64 = 94;
/// `IFM_IEEE80211_HT_MCS76`: 11n MCS 76
pub const IFM_IEEE80211_HT_MCS76: u64 = 95;
/// `IFM_IEEE80211_VHT_MCS0`: 11ac MCS 0
pub const IFM_IEEE80211_VHT_MCS0: u64 = 96;
/// `IFM_IEEE80211_VHT_MCS1`: 11ac MCS 1
pub const IFM_IEEE80211_VHT_MCS1: u64 = 97;
/// `IFM_IEEE80211_VHT_MCS2`: 11ac MCS 2
pub const IFM_IEEE80211_VHT_MCS2: u64 = 98;
/// `IFM_IEEE80211_VHT_MCS3`: 11ac MCS 3
pub const IFM_IEEE80211_VHT_MCS3: u64 = 99;
/// `IFM_IEEE80211_VHT_MCS4`: 11ac MCS 4
pub const IFM_IEEE80211_VHT_MCS4: u64 = 100;
/// `IFM_IEEE80211_VHT_MCS5`: 11ac MCS 5
pub const IFM_IEEE80211_VHT_MCS5: u64 = 101;
/// `IFM_IEEE80211_VHT_MCS6`: 11ac MCS 6
pub const IFM_IEEE80211_VHT_MCS6: u64 = 102;
/// `IFM_IEEE80211_VHT_MCS7`: 11ac MCS 7
pub const IFM_IEEE80211_VHT_MCS7: u64 = 103;
/// `IFM_IEEE80211_VHT_MCS8`: 11ac MCS 8
pub const IFM_IEEE80211_VHT_MCS8: u64 = 104;
/// `IFM_IEEE80211_VHT_MCS9`: 11ac MCS 9
pub const IFM_IEEE80211_VHT_MCS9: u64 = 105;
/// `IFM_IEEE80211_HE_MCS0`: 11ax MCS 0
pub const IFM_IEEE80211_HE_MCS0: u64 = 106;
/// `IFM_IEEE80211_HE_MCS1`: 11ax MCS 1
pub const IFM_IEEE80211_HE_MCS1: u64 = 107;
/// `IFM_IEEE80211_HE_MCS2`: 11ax MCS 2
pub const IFM_IEEE80211_HE_MCS2: u64 = 108;
/// `IFM_IEEE80211_HE_MCS3`: 11ax MCS 3
pub const IFM_IEEE80211_HE_MCS3: u64 = 109;
/// `IFM_IEEE80211_HE_MCS4`: 11ax MCS 4
pub const IFM_IEEE80211_HE_MCS4: u64 = 110;
/// `IFM_IEEE80211_HE_MCS5`: 11ax MCS 5
pub const IFM_IEEE80211_HE_MCS5: u64 = 111;
/// `IFM_IEEE80211_HE_MCS6`: 11ax MCS 6
pub const IFM_IEEE80211_HE_MCS6: u64 = 112;
/// `IFM_IEEE80211_HE_MCS7`: 11ax MCS 7
pub const IFM_IEEE80211_HE_MCS7: u64 = 113;
/// `IFM_IEEE80211_HE_MCS8`: 11ax MCS 8
pub const IFM_IEEE80211_HE_MCS8: u64 = 114;
/// `IFM_IEEE80211_HE_MCS9`: 11ax MCS 9
pub const IFM_IEEE80211_HE_MCS9: u64 = 115;
/// `IFM_IEEE80211_HE_MCS10`: 11ax MCS 10
pub const IFM_IEEE80211_HE_MCS10: u64 = 116;
/// `IFM_IEEE80211_HE_MCS11`: 11ax MCS 11
pub const IFM_IEEE80211_HE_MCS11: u64 = 117;
/// `IFM_IEEE80211_ADHOC`: Operate in Adhoc mode
pub const IFM_IEEE80211_ADHOC: u64 = 0x0000_0000_0001_0000;
/// `IFM_IEEE80211_HOSTAP`: Operate in Host AP mode
pub const IFM_IEEE80211_HOSTAP: u64 = 0x0000_0000_0002_0000;
/// `IFM_IEEE80211_IBSS`: Operate in IBSS mode
pub const IFM_IEEE80211_IBSS: u64 = 0x0000_0000_0004_0000;
/// `IFM_IEEE80211_IBSSMASTER`: Operate as an IBSS master
pub const IFM_IEEE80211_IBSSMASTER: u64 = 0x0000_0000_0008_0000;
/// `IFM_IEEE80211_MONITOR`: Operate in Monitor mode
pub const IFM_IEEE80211_MONITOR: u64 = 0x0000_0000_0010_0000;
/// `IFM_IEEE80211_11A`: 5GHz, OFDM mode
pub const IFM_IEEE80211_11A: u64 = 0x0000_0001_0000_0000;
/// `IFM_IEEE80211_11B`: Direct Sequence mode
pub const IFM_IEEE80211_11B: u64 = 0x0000_0002_0000_0000;
/// `IFM_IEEE80211_11G`: 2GHz, CCK mode
pub const IFM_IEEE80211_11G: u64 = 0x0000_0003_0000_0000;
/// `IFM_IEEE80211_FH`: 2GHz, GFSK mode
pub const IFM_IEEE80211_FH: u64 = 0x0000_0004_0000_0000;
/// `IFM_IEEE80211_11N`: 11n/HT 2GHz/5GHz
pub const IFM_IEEE80211_11N: u64 = 0x0000_0008_0000_0000;
/// `IFM_IEEE80211_11AC`: 11ac/VHT 5GHz
pub const IFM_IEEE80211_11AC: u64 = 0x0000_0010_0000_0000;
/// `IFM_IEEE80211_11AX`: 11ax/HE 2GHz/5GHz
pub const IFM_IEEE80211_11AX: u64 = 0x0000_0020_0000_0000;

// Digitally multiplexed "Carrier" Serial Interfaces

/// `IFM_TDM`.
pub const IFM_TDM: u64 = 0x0000_0000_0000_0500;
/// `IFM_TDM_T1`: T1 B8ZS+ESF 24 ts
pub const IFM_TDM_T1: u64 = 3;
/// `IFM_TDM_T1_AMI`: T1 AMI+SF 24 ts
pub const IFM_TDM_T1_AMI: u64 = 4;
/// `IFM_TDM_E1`: E1 HDB3+G.703 clearchannel 32 ts
pub const IFM_TDM_E1: u64 = 5;
/// `IFM_TDM_E1_G704`: E1 HDB3+G.703+G.704 channelized 31 ts
pub const IFM_TDM_E1_G704: u64 = 6;
/// `IFM_TDM_E1_AMI`: E1 AMI+G.703 32 ts
pub const IFM_TDM_E1_AMI: u64 = 7;
/// `IFM_TDM_E1_AMI_G704`: E1 AMI+G.703+G.704 31 ts
pub const IFM_TDM_E1_AMI_G704: u64 = 8;
/// `IFM_TDM_T3`: T3 B3ZS+C-bit 672 ts
pub const IFM_TDM_T3: u64 = 9;
/// `IFM_TDM_T3_M13`: T3 B3ZS+M13 672 ts
pub const IFM_TDM_T3_M13: u64 = 10;
/// `IFM_TDM_E3`: E3 HDB3+G.751 512? ts
pub const IFM_TDM_E3: u64 = 11;
/// `IFM_TDM_E3_G751`: E3 G.751 512 ts
pub const IFM_TDM_E3_G751: u64 = 12;
/// `IFM_TDM_E3_G832`: E3 G.832 512 ts
pub const IFM_TDM_E3_G832: u64 = 13;
/// `IFM_TDM_E1_G704_CRC4`: E1 HDB3+G.703+G.704 31 ts + CRC4
pub const IFM_TDM_E1_G704_CRC4: u64 = 14;
/// `IFM_TDM_HDLC_CRC16`: Use 16-bit CRC for HDLC instead
pub const IFM_TDM_HDLC_CRC16: u64 = 0x0100;
/// `IFM_TDM_PPP`: SPPP (dumb)
pub const IFM_TDM_PPP: u64 = 0x0200;
/// `IFM_TDM_FR_ANSI`: Frame Relay + LMI ANSI "Annex D"
pub const IFM_TDM_FR_ANSI: u64 = 0x0400;
/// `IFM_TDM_FR_CISCO`: Frame Relay + LMI Cisco
pub const IFM_TDM_FR_CISCO: u64 = 0x0800;
/// `IFM_TDM_FR_ITU`: Frame Relay + LMI ITU "Q933A"
pub const IFM_TDM_FR_ITU: u64 = 0x1000;
/// `IFM_TDM_MASTER`: aka clock source internal
pub const IFM_TDM_MASTER: u64 = 0x0000_0001_0000_0000;

// Common Access Redundancy Protocol

/// `IFM_CARP`.
pub const IFM_CARP: u64 = 0x0000_0000_0000_0600;

// Shared media sub-types

/// `IFM_AUTO`: Autoselect best media
pub const IFM_AUTO: u64 = 0;
/// `IFM_MANUAL`: Jumper/dipswitch selects media
pub const IFM_MANUAL: u64 = 1;
/// `IFM_NONE`: Deselect all media
pub const IFM_NONE: u64 = 2;

// Shared options

/// `IFM_FDX`: Force full duplex
pub const IFM_FDX: u64 = 0x0000_0100_0000_0000;
/// `IFM_HDX`: Force half duplex
pub const IFM_HDX: u64 = 0x0000_0200_0000_0000;
/// `IFM_FLOW`: enable hardware flow control
pub const IFM_FLOW: u64 = 0x0000_0400_0000_0000;
/// `IFM_FLAG0`: Driver defined flag
pub const IFM_FLAG0: u64 = 0x0000_1000_0000_0000;
/// `IFM_FLAG1`: Driver defined flag
pub const IFM_FLAG1: u64 = 0x0000_2000_0000_0000;
/// `IFM_FLAG2`: Driver defined flag
pub const IFM_FLAG2: u64 = 0x0000_4000_0000_0000;
/// `IFM_LOOP`: Put hardware in loopback
pub const IFM_LOOP: u64 = 0x0000_8000_0000_0000;

// Masks

/// `IFM_NMASK`: Network type
pub const IFM_NMASK: u64 = 0x0000_0000_0000_ff00;
/// `IFM_NSHIFT`: Network type shift
pub const IFM_NSHIFT: u32 = 8;
/// `IFM_TMASK`: Media sub-type
pub const IFM_TMASK: u64 = 0x0000_0000_0000_00ff;
/// `IFM_TSHIFT`: Sub-type shift
pub const IFM_TSHIFT: u32 = 0;
/// `IFM_IMASK`: Instance
pub const IFM_IMASK: u64 = 0xff00_0000_0000_0000;
/// `IFM_ISHIFT`: Instance shift
pub const IFM_ISHIFT: u32 = 56;
/// `IFM_OMASK`: Type specific options
pub const IFM_OMASK: u64 = 0x0000_0000_ffff_0000;
/// `IFM_OSHIFT`: Specific options shift
pub const IFM_OSHIFT: u32 = 16;
/// `IFM_MMASK`: Mode
pub const IFM_MMASK: u64 = 0x0000_00ff_0000_0000;
/// `IFM_MSHIFT`: Mode shift
pub const IFM_MSHIFT: u32 = 32;
/// `IFM_GMASK`: Global options
pub const IFM_GMASK: u64 = 0x00ff_ff00_0000_0000;
/// `IFM_GSHIFT`: Global options shift
pub const IFM_GSHIFT: u32 = 40;
/// `IFM_ETH_FMASK`: Ethernet flow control mask.
pub const IFM_ETH_FMASK: u64 = IFM_FLOW | IFM_ETH_RXPAUSE | IFM_ETH_TXPAUSE;
/// `IFM_NMIN`: lowest Network type.
pub const IFM_NMIN: u64 = IFM_ETHER;
/// `IFM_NMAX`: highest Network type.
pub const IFM_NMAX: u64 = IFM_NMASK;

// Status bits

/// `IFM_AVALID`: Active bit valid
pub const IFM_AVALID: u64 = 0x0000_0000_0000_0001;
/// `IFM_ACTIVE`: Interface attached to working net
pub const IFM_ACTIVE: u64 = 0x0000_0000_0000_0002;
/// `IFM_STATUS_VALID`: mask of "status valid" bits, for ifconfig(8).
pub const IFM_STATUS_VALID: u64 = IFM_AVALID;
/// `IFM_INST_MAX`: the highest instance number.
pub const IFM_INST_MAX: u64 = ifm_inst(IFM_IMASK);
/// `IFM_INST_ANY`: every instance (`ifmedia_delete_instance`).
pub const IFM_INST_ANY: u64 = u64::MAX;

/// `ifm_change_cb_t`: the driver's media change callback.
pub type IfmChangeCbT = fn(&'static Ifnet) -> Result<(), Errno>;

/// `ifm_stat_cb_t`: the driver's media status callback.
pub type IfmStatCbT = fn(&'static Ifnet, &mut Ifmediareq);

/// `struct ifmedia_entry`: in-kernel representation of a single supported media type.
pub struct IfmediaEntry {
    /// `ifm_list`: the link on `ifm_list` of its `struct ifmedia`.
    pub ifm_list: TailqEntry<IfmediaEntry>,
    /// `ifm_media`: description of this media attachment.
    pub ifm_media: u64,
    /// `ifm_data`: for driver-specific use.
    pub ifm_data: u32,
    /// `ifm_aux`: for driver-specific use.
    pub ifm_aux: *mut c_void,
}

queue_adapter!(
    /// `TAILQ_HEAD(ifmedia_list, ifmedia_entry)`.
    pub IfmediaList: IfmediaEntry, ifm_list => TailqEntry<IfmediaEntry>
);

/// `struct ifmedia`: one of these goes into a network interface's softc structure. It is used
/// to keep general media state.
///
/// Locks used to protect members: \[M\] `ifmedia_mtx`, the interface media mutex.
pub struct Ifmedia {
    /// `ifm_mask`: mask of changes we don't care about.
    pub ifm_mask: Cell<u64>,
    /// `ifm_media`: current user-set media word.
    pub ifm_media: Cell<u64>,
    /// `ifm_cur`: \[M\] currently selected media (null for none).
    ifm_cur: Cell<*const IfmediaEntry>,
    /// `ifm_list`: \[M\] list of all supported media.
    ifm_list: TailqHead<IfmediaList>,
    /// `ifm_nwords`: \[M\] number of `ifm_list` entries.
    ifm_nwords: Cell<usize>,
    /// `ifm_change_cb`: media change driver callback.
    pub ifm_change_cb: Cell<Option<IfmChangeCbT>>,
    /// `ifm_status_cb`: media status driver callback.
    pub ifm_status_cb: Cell<Option<IfmStatCbT>>,
}

impl Ifmedia {
    /// An empty `struct ifmedia`, what a zeroed softc holds before `ifmedia_init`.
    pub const fn new() -> Self {
        Self {
            ifm_mask: Cell::new(0),
            ifm_media: Cell::new(0),
            ifm_cur: Cell::new(ptr::null()),
            ifm_list: TailqHead::new(),
            ifm_nwords: Cell::new(0),
            ifm_change_cb: Cell::new(None),
            ifm_status_cb: Cell::new(None),
        }
    }

    /// `ifm->ifm_cur`: the currently selected media, if any.
    ///
    /// Read without `ifmedia_mtx`, as the C's drivers and mii(4) read it: an entry is freed
    /// only by `ifmedia_delete_instance`, which a driver calls when it detaches the media.
    pub fn ifm_cur(&self) -> Option<&IfmediaEntry> {
        // SAFETY: a non-null `ifm_cur` is an entry of `ifm_list`, valid until
        // `ifmedia_delete_instance` clears `ifm_cur` and frees it (see above).
        unsafe { self.ifm_cur.get().as_ref() }
    }

    /// `TAILQ_FOREACH(ife, &ifm->ifm_list, ifm_list)`, the media words in the order they
    /// were added (the walk takes `ifmedia_mtx`, as the C's walks do).
    pub fn for_each(&self, mut f: impl FnMut(&IfmediaEntry)) {
        mtx_enter(&IFMEDIA_MTX);
        for ife in self.ifm_list.iter() {
            f(ife);
        }
        mtx_leave(&IFMEDIA_MTX);
    }

    /// `ifm->ifm_nwords`.
    pub fn ifm_nwords(&self) -> usize {
        self.ifm_nwords.get()
    }
}

impl Default for Ifmedia {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `ifm_cur`, `ifm_list` and `ifm_nwords` change only under `ifmedia_mtx`; the other
// members are written by the attach (`ifmedia_init`, `ifm_mask` while adding media) and by
// `ifmedia_ioctl` under the kernel lock, as in C.
unsafe impl Sync for Ifmedia {}

/// `struct ifmedia_baudrate`: the baudrate of a media word.
pub struct IfmediaBaudrate {
    /// `ifmb_word`: media word.
    pub ifmb_word: u64,
    /// `ifmb_baudrate`: corresponding baudrate.
    pub ifmb_baudrate: u64,
}

/// `ifmedia_mtx`: the interface media mutex.
pub static IFMEDIA_MTX: Mutex = Mutex::new(IPL_NET);

/// `ifmedia_baudrate_descriptions[]`: `IFM_BAUDRATE_DESCRIPTIONS`, baudrate descriptions for
/// the various media types, ending with `{ 0, 0 }`.
pub static IFMEDIA_BAUDRATE_DESCRIPTIONS: [IfmediaBaudrate; 140] = [
    br(IFM_ETHER | IFM_10_T, if_mbps(10)),
    br(IFM_ETHER | IFM_10_2, if_mbps(10)),
    br(IFM_ETHER | IFM_10_5, if_mbps(10)),
    br(IFM_ETHER | IFM_100_TX, if_mbps(100)),
    br(IFM_ETHER | IFM_100_FX, if_mbps(100)),
    br(IFM_ETHER | IFM_100_T4, if_mbps(100)),
    br(IFM_ETHER | IFM_100_VG, if_mbps(100)),
    br(IFM_ETHER | IFM_100_T2, if_mbps(100)),
    br(IFM_ETHER | IFM_1000_SX, if_mbps(1000)),
    br(IFM_ETHER | IFM_10_STP, if_mbps(10)),
    br(IFM_ETHER | IFM_10_FL, if_mbps(10)),
    br(IFM_ETHER | IFM_1000_LX, if_mbps(1000)),
    br(IFM_ETHER | IFM_1000_CX, if_mbps(1000)),
    br(IFM_ETHER | IFM_1000_T, if_mbps(1000)),
    br(IFM_ETHER | IFM_HPNA_1, if_mbps(1)),
    br(IFM_ETHER | IFM_10G_LR, if_gbps(10)),
    br(IFM_ETHER | IFM_10G_SR, if_gbps(10)),
    br(IFM_ETHER | IFM_10G_CX4, if_gbps(10)),
    br(IFM_ETHER | IFM_2500_SX, if_mbps(2500)),
    br(IFM_ETHER | IFM_10G_T, if_gbps(10)),
    br(IFM_ETHER | IFM_10G_SFP_CU, if_gbps(10)),
    br(IFM_FDDI | IFM_FDDI_SMF, if_mbps(100)),
    br(IFM_FDDI | IFM_FDDI_MMF, if_mbps(100)),
    br(IFM_FDDI | IFM_FDDI_UTP, if_mbps(100)),
    br(IFM_IEEE80211 | IFM_IEEE80211_FH1, if_mbps(1)),
    br(IFM_IEEE80211 | IFM_IEEE80211_FH2, if_mbps(2)),
    br(IFM_IEEE80211 | IFM_IEEE80211_DS1, if_mbps(1)),
    br(IFM_IEEE80211 | IFM_IEEE80211_DS2, if_mbps(2)),
    br(IFM_IEEE80211 | IFM_IEEE80211_DS5, if_mbps(5)),
    br(IFM_IEEE80211 | IFM_IEEE80211_DS11, if_mbps(11)),
    br(IFM_IEEE80211 | IFM_IEEE80211_DS22, if_mbps(22)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM6, if_mbps(6)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM9, if_mbps(9)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM12, if_mbps(12)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM18, if_mbps(18)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM24, if_mbps(24)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM36, if_mbps(36)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM48, if_mbps(48)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM54, if_mbps(54)),
    br(IFM_IEEE80211 | IFM_IEEE80211_OFDM72, if_mbps(72)),
    // These HT rates correspond to 20 MHz channel with no SGI.
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS0, if_kbps(6500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS1, if_mbps(13)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS2, if_kbps(19500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS3, if_mbps(26)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS4, if_mbps(39)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS5, if_mbps(52)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS6, if_kbps(58500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS7, if_mbps(65)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS8, if_mbps(13)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS9, if_mbps(26)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS10, if_mbps(39)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS11, if_mbps(52)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS12, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS13, if_mbps(104)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS14, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS15, if_mbps(130)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS16, if_kbps(19500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS17, if_mbps(39)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS18, if_kbps(58500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS19, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS20, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS21, if_mbps(156)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS22, if_kbps(175500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS23, if_mbps(195)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS24, if_mbps(26)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS25, if_mbps(52)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS26, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS27, if_mbps(104)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS28, if_mbps(156)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS29, if_mbps(208)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS30, if_mbps(234)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS31, if_mbps(260)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS32, if_mbps(0)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS33, if_mbps(39)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS34, if_mbps(52)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS35, if_mbps(65)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS36, if_kbps(58500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS37, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS38, if_kbps(97500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS39, if_mbps(52)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS40, if_mbps(65)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS41, if_mbps(65)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS42, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS43, if_mbps(91)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS44, if_mbps(91)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS45, if_mbps(104)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS46, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS47, if_kbps(97500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS48, if_kbps(97500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS49, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS50, if_kbps(136500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS51, if_kbps(136500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS52, if_mbps(156)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS53, if_mbps(65)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS54, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS55, if_mbps(91)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS56, if_mbps(78)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS57, if_mbps(91)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS58, if_mbps(104)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS59, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS60, if_mbps(104)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS61, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS62, if_mbps(130)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS63, if_mbps(130)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS64, if_mbps(143)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS65, if_kbps(97500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS66, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS67, if_kbps(136500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS68, if_mbps(117)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS69, if_kbps(136500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS70, if_mbps(156)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS71, if_kbps(175500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS72, if_mbps(156)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS73, if_kbps(175500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS74, if_mbps(195)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS75, if_mbps(195)),
    br(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS76, if_kbps(214500)),
    // These VHT rates correspond to 1 SS, no SGI, 40 MHz channel.
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS0, if_kbps(13500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS1, if_mbps(27)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS2, if_kbps(40500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS3, if_mbps(54)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS4, if_mbps(81)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS5, if_mbps(108)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS6, if_kbps(121500)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS7, if_mbps(135)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS8, if_mbps(162)),
    br(IFM_IEEE80211 | IFM_IEEE80211_VHT_MCS9, if_mbps(180)),
    br(IFM_TDM | IFM_TDM_T1, if_kbps(1536)),
    br(IFM_TDM | IFM_TDM_T1_AMI, if_kbps(1536)),
    br(IFM_TDM | IFM_TDM_E1, if_kbps(2048)),
    br(IFM_TDM | IFM_TDM_E1_G704, if_kbps(2048)),
    br(IFM_TDM | IFM_TDM_E1_AMI, if_kbps(2048)),
    br(IFM_TDM | IFM_TDM_E1_AMI_G704, if_kbps(2048)),
    br(IFM_TDM | IFM_TDM_T3, if_kbps(44736)),
    br(IFM_TDM | IFM_TDM_T3_M13, if_kbps(44736)),
    br(IFM_TDM | IFM_TDM_E3, if_kbps(34368)),
    br(IFM_TDM | IFM_TDM_E3_G751, if_kbps(34368)),
    br(IFM_TDM | IFM_TDM_E3_G832, if_kbps(34368)),
    br(IFM_TDM | IFM_TDM_E1_G704_CRC4, if_kbps(2048)),
    br(0, 0),
];

/// `IFM_TYPE(x)`: the network type of a media word.
pub const fn ifm_type(x: u64) -> u64 {
    x & IFM_NMASK
}

/// `IFM_SUBTYPE(x)`: the media sub-type of a media word.
pub const fn ifm_subtype(x: u64) -> u64 {
    x & IFM_TMASK
}

/// `IFM_INST(x)`: the instance of a media word.
pub const fn ifm_inst(x: u64) -> u64 {
    (x & IFM_IMASK) >> IFM_ISHIFT
}

/// `IFM_OPTIONS(x)`: the type-specific and global options of a media word.
pub const fn ifm_options(x: u64) -> u64 {
    x & (IFM_OMASK | IFM_GMASK)
}

/// `IFM_MODE(x)`: the mode of a media word.
pub const fn ifm_mode(x: u64) -> u64 {
    x & IFM_MMASK
}

/// `IFM_MAKEWORD(type, subtype, options, instance)`: a media word from its parts (the
/// operating mode is not included, as in C).
pub const fn ifm_makeword(type_: u64, subtype: u64, options: u64, instance: u64) -> u64 {
    type_ | subtype | options | (instance << IFM_ISHIFT)
}

/// `IFM_TYPE_MATCH(dt, t)`: whether the description word `dt` applies to the network type of
/// `t` (a description without a type applies to all).
pub const fn ifm_type_match(dt: u64, t: u64) -> bool {
    ifm_type(dt) == 0 || ifm_type(dt) == ifm_type(t)
}

/// `{ word, baudrate }` of `IFM_BAUDRATE_DESCRIPTIONS`.
const fn br(ifmb_word: u64, ifmb_baudrate: u64) -> IfmediaBaudrate {
    IfmediaBaudrate {
        ifmb_word,
        ifmb_baudrate,
    }
}

/// `ifmedia_init`: initialize if_media struct for a specific interface instance.
pub fn ifmedia_init(
    ifm: &Ifmedia,
    dontcare_mask: u64,
    change_callback: IfmChangeCbT,
    status_callback: IfmStatCbT,
) {
    ifm.ifm_list.init();
    ifm.ifm_nwords.set(0);
    ifm.ifm_cur.set(ptr::null());
    ifm.ifm_media.set(0);
    ifm.ifm_mask.set(dontcare_mask); // IF don't-care bits
    ifm.ifm_change_cb.set(Some(change_callback));
    ifm.ifm_status_cb.set(Some(status_callback));
}

/// `ifmedia_add`: add a media configuration to the list of supported media for a specific
/// interface instance.
///
/// The `struct ifmedia` must stay in place while it has entries (it is a softc member).
pub fn ifmedia_add(ifm: &Ifmedia, mword: u64, data: i32, aux: *mut c_void) {
    let Some(p) = malloc(size_of::<IfmediaEntry>(), M_IFADDR, M_NOWAIT) else {
        panic(format_args!("ifmedia_add: can't malloc entry"));
    };
    let entry = p.as_ptr().cast::<IfmediaEntry>();
    // SAFETY: a fresh allocation of an `IfmediaEntry`'s size, `malloc`'s alignment covering
    // every kernel type; written whole before it is borrowed.
    let entry = unsafe {
        entry.write(IfmediaEntry {
            ifm_list: TailqEntry::new(),
            ifm_media: mword,
            ifm_data: data as u32,
            ifm_aux: aux,
        });
        &*entry
    };

    mtx_enter(&IFMEDIA_MTX);
    // SAFETY: the entry is new, so in no list; it stays valid and in place until
    // `ifmedia_delete_instance` unlinks and frees it, and the head is a softc member that
    // does not move.
    unsafe { ifm.ifm_list.insert_tail(entry) };
    ifm.ifm_nwords.set(ifm.ifm_nwords.get() + 1);
    mtx_leave(&IFMEDIA_MTX);
}

/// `ifmedia_set`: set the default active media.
///
/// Called by device-specific code which is assumed to have already selected the default
/// media in hardware. We do _not_ call the media-change callback.
pub fn ifmedia_set(ifm: &Ifmedia, mut target: u64) {
    mtx_enter(&IFMEDIA_MTX);
    let mut match_ = ifmedia_get(ifm, target, ifm.ifm_mask.get());

    // If we didn't find the requested media, then we try to fall back to target-type
    // (IFM_ETHER, e.g.) | IFM_NONE. If that's not on the list, then we add it and set the
    // media to it.
    //
    // Since ifmedia_set is almost always called with IFM_AUTO or with a known-good media,
    // this really should only occur if we:
    //
    // a) didn't find any PHYs, or
    // b) didn't find an autoselect option on the PHY when the parent ethernet driver
    //    expected to.
    //
    // In either case, it makes sense to select no media.
    if match_.is_null() {
        printf(format_args!(
            "ifmedia_set: no match for 0x{:x}/0x{:x}\n",
            target,
            !ifm.ifm_mask.get()
        ));
        target = (target & IFM_NMASK) | IFM_NONE;
        match_ = ifmedia_get(ifm, target, ifm.ifm_mask.get());
        if match_.is_null() {
            mtx_leave(&IFMEDIA_MTX);
            ifmedia_add(ifm, target, 0, ptr::null_mut());
            mtx_enter(&IFMEDIA_MTX);
            match_ = ifmedia_get(ifm, target, ifm.ifm_mask.get());
            if match_.is_null() {
                mtx_leave(&IFMEDIA_MTX);
                panic(format_args!("ifmedia_set failed"));
            }
        }
    }
    ifm.ifm_cur.set(match_);
    mtx_leave(&IFMEDIA_MTX);
}

/// `ifmedia_ioctl`: device-independent media ioctl support function.
///
/// # Safety
///
/// `data` is what `sys_ioctl` hands an `if_ioctl` for `cmd`: an aligned kernel copy of a
/// `struct ifreq` for `SIOCSIFMEDIA` and of a `struct ifmediareq` for `SIOCGIFMEDIA`, or null.
pub unsafe fn ifmedia_ioctl(
    ifp: &'static Ifnet,
    data: *mut u8,
    ifm: &Ifmedia,
    cmd: u64,
) -> Result<(), Errno> {
    if data.is_null() {
        return Err(Errno::EINVAL);
    }

    let error;
    match cmd {
        // Set the current media.
        SIOCSIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` (the contract above).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            let newmedia = ifr.ifr_media();

            mtx_enter(&IFMEDIA_MTX);
            let match_ = ifmedia_get(ifm, newmedia, ifm.ifm_mask.get());
            if match_.is_null() {
                mtx_leave(&IFMEDIA_MTX);
                return Err(Errno::EINVAL);
            }

            // If no change, we're done.
            // XXX Automedia may involve software intervention.
            //     Keep going in case the connected media changed.
            //     Similarly, if best match changed (kernel debugger?).
            if ifm_subtype(newmedia) != IFM_AUTO
                && newmedia == ifm.ifm_media.get()
                && ptr::eq(match_, ifm.ifm_cur.get())
            {
                mtx_leave(&IFMEDIA_MTX);
                return Ok(());
            }

            // We found a match, now make the driver switch to it. Make sure to preserve our
            // old media type in case the driver can't switch.
            let oldentry = ifm.ifm_cur.get();
            let oldmedia = ifm.ifm_media.get();
            ifm.ifm_cur.set(match_);
            ifm.ifm_media.set(newmedia);
            mtx_leave(&IFMEDIA_MTX);

            error = match ifm.ifm_change_cb.get() {
                Some(cb) => cb(ifp),
                None => panic(format_args!("ifmedia_ioctl: no media change callback")),
            };
            if matches!(error, Err(e) if e != Errno::ENETRESET) {
                mtx_enter(&IFMEDIA_MTX);
                if ptr::eq(ifm.ifm_cur.get(), match_) {
                    ifm.ifm_cur.set(oldentry);
                    ifm.ifm_media.set(oldmedia);
                }
                mtx_leave(&IFMEDIA_MTX);
            }
        }

        // Get list of available media and current media on interface.
        SIOCGIFMEDIA => {
            // SAFETY: SIOCGIFMEDIA carries a `struct ifmediareq` (the contract above).
            let ifmr = unsafe { &mut *data.cast::<Ifmediareq>() };

            if ifmr.ifm_count < 0 {
                return Err(Errno::EINVAL);
            }

            mtx_enter(&IFMEDIA_MTX);
            let cur = ifm.ifm_cur().map_or(IFM_NONE, |e| e.ifm_media);
            ifmr.ifm_active = cur;
            ifmr.ifm_current = cur;
            ifmr.ifm_mask = ifm.ifm_mask.get();
            ifmr.ifm_status = 0;
            mtx_leave(&IFMEDIA_MTX);

            match ifm.ifm_status_cb.get() {
                Some(cb) => cb(ifp, ifmr),
                None => panic(format_args!("ifmedia_ioctl: no media status callback")),
            }

            mtx_enter(&IFMEDIA_MTX);
            let mut nwords = ifm.ifm_nwords.get();
            mtx_leave(&IFMEDIA_MTX);

            if ifmr.ifm_count == 0 {
                ifmr.ifm_count = nwords as i32;
                return Ok(());
            }

            loop {
                let ksiz = nwords * size_of::<u64>();
                let Some(kptr) = mallocarray(nwords, size_of::<u64>(), M_TEMP, M_WAITOK | M_ZERO)
                else {
                    panic(format_args!("ifmedia_ioctl: out of memory"));
                };

                mtx_enter(&IFMEDIA_MTX);
                // Media list might grow during malloc().
                if nwords < ifm.ifm_nwords.get() {
                    nwords = ifm.ifm_nwords.get();
                    mtx_leave(&IFMEDIA_MTX);
                    free(kptr, M_TEMP, ksiz);
                    continue;
                }
                // Request memory too small, set error and ifm_count.
                if (ifmr.ifm_count as usize) < ifm.ifm_nwords.get() {
                    nwords = ifm.ifm_nwords.get();
                    mtx_leave(&IFMEDIA_MTX);
                    free(kptr, M_TEMP, ksiz);
                    error = Err(Errno::E2BIG);
                    break;
                }
                // Get the media words from the interface's list.
                // SAFETY: a zeroed allocation of `nwords` words, `malloc`'s alignment covering
                // a `u64`, freed below after its last use.
                let words =
                    unsafe { core::slice::from_raw_parts_mut(kptr.as_ptr().cast::<u64>(), nwords) };
                let mut n = 0;
                for ife in ifm.ifm_list.iter() {
                    words[n] = ife.ifm_media;
                    n += 1;
                }
                nwords = n;
                kassert!(nwords == ifm.ifm_nwords.get());
                mtx_leave(&IFMEDIA_MTX);

                // SAFETY: the words were initialised above (and the allocation zeroed).
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        kptr.as_ptr().cast_const(),
                        nwords * size_of::<u64>(),
                    )
                };
                error = copyout(bytes, ifmr.ifm_ulist as usize);
                free(kptr, M_TEMP, ksiz);
                break;
            }
            ifmr.ifm_count = nwords as i32;
        }

        _ => return Err(Errno::ENOTTY),
    }

    error
}

/// `ifmedia_match`: find media entry matching a given ifm word; true if found.
pub fn ifmedia_match(ifm: &Ifmedia, target: u64, mask: u64) -> bool {
    mtx_enter(&IFMEDIA_MTX);
    let match_ = ifmedia_get(ifm, target, mask);
    mtx_leave(&IFMEDIA_MTX);

    !match_.is_null()
}

/// `ifmedia_get`: the first entry whose word equals `target` outside the don't-care `mask`;
/// null when there is none. Called with `ifmedia_mtx` held.
fn ifmedia_get(ifm: &Ifmedia, target: u64, mask: u64) -> *const IfmediaEntry {
    mutex_assert_locked(&IFMEDIA_MTX, "ifmedia_get");

    let mut match_: *const IfmediaEntry = ptr::null();
    let mask = !mask;

    for next in ifm.ifm_list.iter() {
        if (next.ifm_media & mask) == (target & mask) {
            // SAFETY: a non-null `match_` is an entry of this list, valid under the mutex.
            if let Some(m) = unsafe { match_.as_ref() } {
                #[cfg(feature = "diagnostic")]
                printf(format_args!(
                    "ifmedia_get: multiple match for 0x{:x}/0x{:x}, selected instance {}\n",
                    target,
                    mask,
                    ifm_inst(m.ifm_media) as i64
                ));
                let _ = m;
                break;
            }
            match_ = next;
        }
    }

    match_
}

/// `ifmedia_delete_instance`: delete all media for a given instance (`IFM_INST_ANY` for all).
pub fn ifmedia_delete_instance(ifm: &Ifmedia, inst: u64) {
    let ifmlist: TailqHead<IfmediaList> = TailqHead::new();

    mtx_enter(&IFMEDIA_MTX);
    for ife in ifm.ifm_list.iter() {
        if inst == IFM_INST_ANY || inst == ifm_inst(ife.ifm_media) {
            // SAFETY: `ife` is on `ifm_list` (the walk reads the next element first), and
            // goes on the local list, which stays in place until it is emptied below.
            unsafe {
                ifm.ifm_list.remove(ife);
                ifmlist.insert_tail(ife);
            }
            ifm.ifm_nwords.set(ifm.ifm_nwords.get() - 1);
        }
    }
    ifm.ifm_cur.set(ptr::null());
    mtx_leave(&IFMEDIA_MTX);

    // Do not hold mutex longer than necessary, call free() without.
    while let Some(ife) = ifmlist.first() {
        let p = NonNull::from(ife);
        // SAFETY: `ife` is the local list's first element; once unlinked nothing refers to
        // it, so its allocation (from `ifmedia_add`) may be freed.
        unsafe { ifmlist.remove(ife) };
        free(p.cast(), M_IFADDR, size_of::<IfmediaEntry>());
    }
}

/// `ifmedia_baudrate`: compute the interface `baudrate` from the media, for the interface
/// metrics (used by routing daemons); 0 when not known.
pub fn ifmedia_baudrate(mword: u64) -> u64 {
    for d in IFMEDIA_BAUDRATE_DESCRIPTIONS.iter() {
        if d.ifmb_word == 0 {
            break;
        }
        if (mword & (IFM_NMASK | IFM_TMASK)) == d.ifmb_word {
            return d.ifmb_baudrate;
        }
    }

    // Not known.
    0
}

#[cfg(test)]
mod tests;
