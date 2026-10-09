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
 * Copyright (c) 1997, 1998, 1999
 *	Bill Paul <wpaul@ee.columbia.edu>.  All rights reserved.
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
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul OR THE VOICES IN HIS HEAD
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/pci/if_dc.c,v 1.43 2001/01/19 23:55:07 wpaul Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! dc(4): the DEC "tulip" clone ethernet driver, bus-independent part (`dev/ic/dc.c`).
//! `if_dc_pci.rs` attaches it.
//!
//! Upstream: sys/dev/ic/dc.c @ 3ce1f3f79392
//!
//! Supports the DEC/Intel 21143 series chips and several workalikes: the Macronix
//! 98713/98715/98725/98727/98732 PMAC, the Macronix/Lite-On 82c115 PNIC II, the Lite-On
//! 82c168/82c169 PNIC, the ASIX AX88140A and AX88141, the ADMtek AL981 and AN983, the Davicom
//! DM9100, DM9102 and DM9102A, the Accton EN1217 and EN2242, the Xircom X3201 and the
//! Conexant RS7112. The 21143 has an MII port (10/100 and NWAY autonegotiation through an
//! external PHY), a SYM port (symbol mode 100Mbps), a 10baseT port and an AUI/BNC port; the
//! SYM and 10baseT ports with the internal NWAY make a 10/100 autosensing configuration that
//! dcphy(4) drives. The workalikes use some form of MII transceiver, the Macronix chips also a
//! SYM port. The receive filter of the 21143-style chips is a setup frame the transmit engine
//! downloads; the ADMtek, ASIX and Xircom chips have filter registers (the Xircom a setup
//! frame of its own).
//!
//! QEMU's `tulip` is a 21143 (PCI 1011:0019, revision 0) with an LXT970 PHY at MII address 1.
//! On it OpenBSD 8.0 attaches `dc0` and `lxtphy0`, prints `dc0: failed to force tx to idle
//! state` twice when the interface comes up and later `dc0: watchdog timeout`, and receives
//! nothing (ping: 100% packet loss); this port does what the C does on that device.
//!
//! dc(4) is not MP-safe, as in C: its interrupt is established without `IPL_MPSAFE` and its
//! start routine is the interface's `if_start`, so the softc changes under the kernel lock, at
//! `splnet`.
//!
//! ## Deviations
//! - `dc_read_eeprom` fills a `&mut [u8]` (the C's `caddr_t`), each word stored in host order
//!   as `letoh16`/`betoh16` give it.
//! - `dc_attach` reads the Macronix/PNIC II station address offset into a zeroed 16-bit word
//!   (the C reads one EEPROM word into the low half of an uninitialised `int`).
//! - `dc_attach`'s Conexant address is the six bytes at `DC_CONEXANT_EE_NODEADDR` of the SROM
//!   (the C copies from `&sc->dc_srom + DC_CONEXANT_EE_NODEADDR`, an address computed from the
//!   softc's pointer member, not the SROM: a C bug; FreeBSD indexes the SROM).
//! - `dc_setfilt_asix` writes the second filter word from the address's last two bytes and
//!   two zero bytes (the C reads four bytes from `ac_enaddr[4]`, the last two being arpcom's
//!   zeroed padding).
//! - The SROM is read through bounds-checked accessors: a byte past `dc_sromsize` reads as 0
//!   in `dc_parse_21143_srom` and the leaf decoders, and `dc_apply_fixup` stops at the end of
//!   the SROM (the C reads past the allocation). `struct dc_mediainfo` holds offsets into the
//!   SROM (`dcreg.rs`).
//! - `dc_pnic_rx_bug_war` copies at most the five frames `dc_pnic_rx_buf` holds and scans no
//!   further back than its start (the C overruns and underruns it); its arithmetic is the
//!   pure [`dc_pnic_salvage`]. `dc_rxeof` hands `m_devget` at most the cluster's bytes (the
//!   C trusts the descriptor's length).
//! - `dc_detach` unmaps the whole list memory (the C passes `sc_listnseg`, 1, as the size).
//! - The setup frame's stand-in mbuf pointer is the flag `sd_setup` (`dcreg.rs`).
//! - `BUS_DMA_OVERRUN` is not defined by the machine and is 0, as in the C's fallback.
//! - `dc_init`, `dc_stop`, `dc_tick` and `dc_intr` take the softc (the C's `void *` for the
//!   last two); functions returning 0 or an errno return `Result<(), Errno>`; `dc_rx_resync`
//!   returns whether it moved (`EAGAIN`), `dc_mii_readreg` whether the PHY did not ack.

#![allow(non_upper_case_globals)] // the C's `DC_TYPE_987x5`, verbatim

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::ic::dcreg::*;
use crate::dev::mii::mii::{
    BMSR_MEDIAMASK, MII_ANAR, MII_ANER, MII_ANLPAR, MII_BMCR, MII_BMSR, MII_NPHY, MII_PHYIDR1,
    MII_PHYIDR2, mii_attach, mii_detach, mii_mediachg, mii_pollstat, mii_tick,
};
use crate::dev::mii::miivar::{MII_OFFSET_ANY, MII_PHY_ANY};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_DEC_21142, PCI_PRODUCT_LITEON_PNIC, PCI_VENDOR_DEC, PCI_VENDOR_LITEON,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_adj, m_clget, m_defrag, m_devget, m_freem, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusAddr, BusDmaSegment, BusDmamap, BusSize, bus_dmamap_create,
    bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IFCAP_VLAN_MTU, IFF_ALLMULTI, IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING,
    IFF_SIMPLEX, IFF_UP, IFNAMSIZ, Ifmediareq, if_attach, if_detach, if_input,
};
use crate::net::if_ethersubr::{
    ETHERBROADCASTADDR, ether_crc32_be, ether_crc32_le, ether_ifattach, ether_ifdetach,
    ether_ioctl, ether_sprintf,
};
use crate::net::if_media::{
    IFM_10_T, IFM_100_TX, IFM_ACTIVE, IFM_AUTO, IFM_ETHER, IFM_FDX, IFM_GMASK, IFM_HPNA_1,
    IFM_NONE, ifm_subtype, ifmedia_add, ifmedia_init, ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{
    ifq_clr_oactive, ifq_deq_begin, ifq_deq_commit, ifq_deq_rollback, ifq_empty, ifq_init_maxlen,
    ifq_is_oactive, ifq_len, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_CRC_LEN, ETHER_MAX_DIX_LEN, EtherMultistep,
    ether_first_multi, ether_next_multi,
};
use crate::sys::device::{Cfdriver, DV_IFNET, DVACT_RESUME, DVACT_SUSPEND, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::mbuf::{M_DONTWAIT, MCLBYTES, Mbuf, MbufList, mtod};
use crate::sys::param::PAGE_SIZE;
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA};

/// `BUS_DMA_OVERRUN`: the machine does not define it, so it is 0 (the C's fallback). The
/// Davicom DM9102's DMA engine reads beyond the end of a transfer; machines with an IOMMU
/// that traps on it would define it.
pub const BUS_DMA_OVERRUN: i32 = 0;

/// `DC_BITS_512`.
const DC_BITS_512: u32 = 9;
/// `DC_BITS_128`.
const DC_BITS_128: u32 = 7;
/// `DC_BITS_64`.
const DC_BITS_64: u32 = 6;

/// `DC_WHOLEFRAME`.
const DC_WHOLEFRAME: u32 = DC_RXSTAT_FIRSTFRAG | DC_RXSTAT_LASTFRAG;

/// `sizeof(u_int64_t)`: the headroom `dc_newbuf` leaves in front of a receive buffer.
const DC_RX_SKIP: usize = size_of::<u64>();

/// `dc_cd`.
pub static DC_CD: Cfdriver = Cfdriver::new(b"dc", DV_IFNET, 0);

/// `(struct dc_softc *)ifp->if_softc`.
pub fn dc_softc(ifp: &Ifnet) -> &'static DcSoftc {
    let p = ifp.if_softc.get().cast::<DcSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("dc: interface without its softc"));
    }
    // SAFETY: dc_attach sets `if_softc` to its softc and installs dc's functions only on its
    // own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// The softc behind the C's `void *` argument (interrupt, timeout).
fn dc_arg(arg: *mut c_void) -> &'static DcSoftc {
    if arg.is_null() {
        panic(format_args!("dc: no softc argument"));
    }
    // SAFETY: dc_pci_attach and dc_init pass the softc, which is never freed while the
    // device exists.
    unsafe { &*arg.cast::<DcSoftc>() }
}

/// The softc of the device the mii(4) callbacks name (`(struct dc_softc *)self`).
fn dc_dev_sc(dev: &Device) -> &'static DcSoftc {
    // SAFETY: mii(4) calls back with the device that attached it, dc's; its softc starts
    // with a `struct dc_softc` (`dc_pci_softc`), never freed while the device exists.
    unsafe { &*ptr::from_ref(dev.softc::<DcSoftc>()) }
}

/// The softc as the C's `void *`.
fn dc_ptr(sc: &'static DcSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// `SIO_SET(x)`.
fn sio_set(sc: &DcSoftc, x: u32) {
    sc.dc_setbit(DC_SIO, x);
}

/// `SIO_CLR(x)`.
fn sio_clr(sc: &DcSoftc, x: u32) {
    sc.dc_clrbit(DC_SIO, x);
}

/// A byte of the SROM, 0 past its end.
fn srom_byte(srom: &[u8], i: usize) -> u8 {
    srom.get(i).copied().unwrap_or(0)
}

/// `dc_delay`: about 300ns, as reads of the bus control register.
pub fn dc_delay(sc: &DcSoftc) {
    for _ in 0..(300 / 33) + 1 {
        let _ = sc.csr_read_4(DC_BUSCTL);
    }
}

/// Enter EEPROM access mode (the sequence `dc_eeprom_width`, `dc_eeprom_idle` and
/// `dc_eeprom_getword` share).
fn dc_eeprom_enter(sc: &DcSoftc) {
    sc.csr_write_4(DC_SIO, DC_SIO_EESEL);
    dc_delay(sc);
    sc.dc_setbit(DC_SIO, DC_SIO_ROMCTL_READ);
    dc_delay(sc);
    sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
    dc_delay(sc);
    sc.dc_setbit(DC_SIO, DC_SIO_EE_CS);
    dc_delay(sc);
}

/// `dc_eeprom_width`: the EEPROM's address width, from the zero bit it answers a read
/// command with (6 when it is out of range).
pub fn dc_eeprom_width(sc: &DcSoftc) {
    // Force EEPROM to idle state.
    dc_eeprom_idle(sc);

    // Enter EEPROM access mode.
    dc_eeprom_enter(sc);

    for i in (0..3).rev() {
        if 6 & (1 << i) != 0 {
            sc.dc_setbit(DC_SIO, DC_SIO_EE_DATAIN);
        } else {
            sc.dc_clrbit(DC_SIO, DC_SIO_EE_DATAIN);
        }
        dc_delay(sc);
        sc.dc_setbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
        sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
    }

    let mut i = 1;
    while i <= 12 {
        sc.dc_setbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
        if sc.csr_read_4(DC_SIO) & DC_SIO_EE_DATAOUT == 0 {
            sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
            dc_delay(sc);
            break;
        }
        sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
        i += 1;
    }

    // Turn off EEPROM access mode.
    dc_eeprom_idle(sc);

    if !(4..=12).contains(&i) {
        sc.dc_romwidth.set(6);
    } else {
        sc.dc_romwidth.set(i);
    }

    // Enter EEPROM access mode.
    dc_eeprom_enter(sc);

    // Turn off EEPROM access mode.
    dc_eeprom_idle(sc);
}

/// `dc_eeprom_idle`: force the EEPROM to its idle state.
pub fn dc_eeprom_idle(sc: &DcSoftc) {
    dc_eeprom_enter(sc);

    for _ in 0..25 {
        sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
        sc.dc_setbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
    }

    sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
    dc_delay(sc);
    sc.dc_clrbit(DC_SIO, DC_SIO_EE_CS);
    dc_delay(sc);
    sc.csr_write_4(DC_SIO, 0x00000000);
}

/// `dc_eeprom_putbyte`: send a read command and address to the EEPROM, check for ACK.
pub fn dc_eeprom_putbyte(sc: &DcSoftc, addr: i32) {
    let d = DC_EECMD_READ >> 6;

    for i in (0..3).rev() {
        if d & (1 << i) != 0 {
            sc.dc_setbit(DC_SIO, DC_SIO_EE_DATAIN);
        } else {
            sc.dc_clrbit(DC_SIO, DC_SIO_EE_DATAIN);
        }
        dc_delay(sc);
        sc.dc_setbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
        sc.dc_clrbit(DC_SIO, DC_SIO_EE_CLK);
        dc_delay(sc);
    }

    // Feed in each bit and strobe the clock.
    for i in (0..sc.dc_romwidth.get()).rev() {
        if addr & (1 << i) != 0 {
            sio_set(sc, DC_SIO_EE_DATAIN);
        } else {
            sio_clr(sc, DC_SIO_EE_DATAIN);
        }
        dc_delay(sc);
        sio_set(sc, DC_SIO_EE_CLK);
        dc_delay(sc);
        sio_clr(sc, DC_SIO_EE_CLK);
        dc_delay(sc);
    }
}

/// `dc_eeprom_getword_pnic`: read a word of data stored in the EEPROM at address `addr`.
/// The PNIC 82c168/82c169 has its own non-standard way to read the EEPROM; `dest` is left
/// alone when the read never completes.
pub fn dc_eeprom_getword_pnic(sc: &DcSoftc, addr: i32, dest: &mut u16) {
    sc.csr_write_4(DC_PN_SIOCTL, DC_PN_EEOPCODE_READ | addr as u32);

    for _ in 0..DC_TIMEOUT {
        delay(1);
        let r = sc.csr_read_4(DC_SIO);
        if r & DC_PN_SIOCTL_BUSY == 0 {
            *dest = (r & 0xFFFF) as u16;
            return;
        }
    }
}

/// `dc_eeprom_getword_xircom`: read a word of data stored in the EEPROM at address `addr`.
/// The Xircom X3201 has its own non-standard way to read the EEPROM, too.
pub fn dc_eeprom_getword_xircom(sc: &DcSoftc, addr: i32) -> u16 {
    sio_set(sc, DC_SIO_ROMSEL | DC_SIO_ROMCTL_READ);

    let mut addr = (addr * 2) as u32;
    sc.csr_write_4(DC_ROM, addr | 0x160);
    let mut dest = (sc.csr_read_4(DC_SIO) & 0xff) as u16;
    addr += 1;
    sc.csr_write_4(DC_ROM, addr | 0x160);
    dest |= ((sc.csr_read_4(DC_SIO) & 0xff) as u16) << 8;

    sio_clr(sc, DC_SIO_ROMSEL | DC_SIO_ROMCTL_READ);
    dest
}

/// `dc_eeprom_getword`: read a word of data stored in the EEPROM at address `addr`.
pub fn dc_eeprom_getword(sc: &DcSoftc, addr: i32) -> u16 {
    let mut word: u16 = 0;

    // Force EEPROM to idle state.
    dc_eeprom_idle(sc);

    // Enter EEPROM access mode.
    dc_eeprom_enter(sc);

    // Send address of word we want to read.
    dc_eeprom_putbyte(sc, addr);

    // Start reading bits from EEPROM.
    let mut i: u16 = 0x8000;
    while i != 0 {
        sio_set(sc, DC_SIO_EE_CLK);
        dc_delay(sc);
        if sc.csr_read_4(DC_SIO) & DC_SIO_EE_DATAOUT != 0 {
            word |= i;
        }
        dc_delay(sc);
        sio_clr(sc, DC_SIO_EE_CLK);
        dc_delay(sc);
        i >>= 1;
    }

    // Turn off EEPROM access mode.
    dc_eeprom_idle(sc);

    word
}

/// The bytes `dc_read_eeprom` stores for an EEPROM word: the 16-bit value `betoh16(word)`
/// (with `swap`) or `letoh16(word)`, in host order.
pub fn dc_eeprom_word_bytes(word: u16, swap: bool) -> [u8; 2] {
    let v = if swap {
        u16::from_be(word)
    } else {
        u16::from_le(word)
    };
    v.to_ne_bytes()
}

/// `dc_read_eeprom`: read `dest.len() / 2` words from the EEPROM at word `off`.
pub fn dc_read_eeprom(sc: &DcSoftc, dest: &mut [u8], off: i32, swap: bool) {
    // The C's `word`, carried over a PNIC read that never completes.
    let mut word: u16 = 0;

    for (i, w) in dest.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let addr = off + i as i32;
        if sc.dc_is_pnic() {
            dc_eeprom_getword_pnic(sc, addr, &mut word);
        } else if sc.dc_is_xircom() {
            word = dc_eeprom_getword_xircom(sc, addr);
        } else {
            word = dc_eeprom_getword(sc, addr);
        }
        *w = dc_eeprom_word_bytes(word, swap);
    }
}

/// `dc_mii_writebit`: write a bit to the MII bus (from the Macronix 98713 Application Notes
/// pp.19-21).
pub fn dc_mii_writebit(sc: &DcSoftc, bit: bool) {
    if bit {
        sc.csr_write_4(DC_SIO, DC_SIO_ROMCTL_WRITE | DC_SIO_MII_DATAOUT);
    } else {
        sc.csr_write_4(DC_SIO, DC_SIO_ROMCTL_WRITE);
    }

    sc.dc_setbit(DC_SIO, DC_SIO_MII_CLK);
    sc.dc_clrbit(DC_SIO, DC_SIO_MII_CLK);
}

/// `dc_mii_readbit`: read a bit from the MII bus.
pub fn dc_mii_readbit(sc: &DcSoftc) -> bool {
    sc.csr_write_4(DC_SIO, DC_SIO_ROMCTL_READ | DC_SIO_MII_DIR);
    let _ = sc.csr_read_4(DC_SIO);
    sc.dc_setbit(DC_SIO, DC_SIO_MII_CLK);
    sc.dc_clrbit(DC_SIO, DC_SIO_MII_CLK);
    sc.csr_read_4(DC_SIO) & DC_SIO_MII_DATAIN != 0
}

/// `dc_mii_sync`: sync the PHYs by setting data bit and strobing the clock 32 times.
pub fn dc_mii_sync(sc: &DcSoftc) {
    sc.csr_write_4(DC_SIO, DC_SIO_ROMCTL_WRITE);

    for _ in 0..32 {
        dc_mii_writebit(sc, true);
    }
}

/// `dc_mii_send`: clock a series of bits through the MII, most significant first.
pub fn dc_mii_send(sc: &DcSoftc, bits: u32, cnt: u32) {
    let mut i = 1u32 << (cnt - 1);
    while i != 0 {
        dc_mii_writebit(sc, bits & i != 0);
        i >>= 1;
    }
}

/// `dc_mii_readreg`: read a PHY register through the MII. Returns whether the PHY failed to
/// ack (the C's 1).
pub fn dc_mii_readreg(sc: &DcSoftc, frame: &mut DcMiiFrame) -> bool {
    let s = splnet();

    // Set up frame for RX.
    frame.mii_stdelim = DC_MII_STARTDELIM;
    frame.mii_opcode = DC_MII_READOP;
    frame.mii_turnaround = 0;
    frame.mii_data = 0;

    // Sync the PHYs.
    dc_mii_sync(sc);

    // Send command/address info.
    dc_mii_send(sc, u32::from(frame.mii_stdelim), 2);
    dc_mii_send(sc, u32::from(frame.mii_opcode), 2);
    dc_mii_send(sc, u32::from(frame.mii_phyaddr), 5);
    dc_mii_send(sc, u32::from(frame.mii_regaddr), 5);

    // Check for ack
    let ack = dc_mii_readbit(sc);

    // Now try reading data bits. If the ack failed, we still need to clock through 16 cycles
    // to keep the PHY(s) in sync.
    if ack {
        for _ in 0..16 {
            let _ = dc_mii_readbit(sc);
        }
    } else {
        let mut i: u16 = 0x8000;
        while i != 0 {
            if dc_mii_readbit(sc) {
                frame.mii_data |= i;
            }
            i >>= 1;
        }
    }

    // fail:
    dc_mii_writebit(sc, false);
    dc_mii_writebit(sc, false);

    splx(s);

    ack
}

/// `dc_mii_writereg`: write to a PHY register through the MII.
pub fn dc_mii_writereg(sc: &DcSoftc, frame: &mut DcMiiFrame) {
    let s = splnet();

    // Set up frame for TX.
    frame.mii_stdelim = DC_MII_STARTDELIM;
    frame.mii_opcode = DC_MII_WRITEOP;
    frame.mii_turnaround = DC_MII_TURNAROUND;

    // Sync the PHYs.
    dc_mii_sync(sc);

    dc_mii_send(sc, u32::from(frame.mii_stdelim), 2);
    dc_mii_send(sc, u32::from(frame.mii_opcode), 2);
    dc_mii_send(sc, u32::from(frame.mii_phyaddr), 5);
    dc_mii_send(sc, u32::from(frame.mii_regaddr), 5);
    dc_mii_send(sc, u32::from(frame.mii_turnaround), 2);
    dc_mii_send(sc, u32::from(frame.mii_data), 16);

    // Idle bit.
    dc_mii_writebit(sc, false);
    dc_mii_writebit(sc, false);

    splx(s);
}

/// The AL981's register for the MII register `reg` (its PHY registers are MAC registers).
fn dc_comet_reg(reg: i32) -> Option<u32> {
    match reg {
        MII_BMCR => Some(DC_AL_BMCR),
        MII_BMSR => Some(DC_AL_BMSR),
        MII_PHYIDR1 => Some(DC_AL_VENID),
        MII_PHYIDR2 => Some(DC_AL_DEVID),
        MII_ANAR => Some(DC_AL_ANAR),
        MII_ANLPAR => Some(DC_AL_LPAR),
        MII_ANER => Some(DC_AL_ANER),
        _ => None,
    }
}

/// `dc_miibus_readreg`.
pub fn dc_miibus_readreg(self_: &Device, phy: i32, reg: i32) -> i32 {
    let sc = dc_dev_sc(self_);

    // Note: both the AL981 and AN983 have internal PHYs, however the AL981 provides direct
    // access to the PHY registers while the AN983 uses a serial MII interface. The AN983's
    // MII interface is also buggy in that you can read from any MII address (0 to 31), but
    // only address 1 behaves normally. To deal with both cases, we pretend that the PHY is
    // at MII address 1.
    if sc.dc_is_admtek() && phy != DC_ADMTEK_PHYADDR {
        return 0;
    }

    // Note: the ukphy probs of the RS7112 report a PHY at MII address 0 (possibly HomePNA?)
    // and 1 (ethernet) so we only respond to correct one.
    if sc.dc_is_conexant() && phy != DC_CONEXANT_PHYADDR {
        return 0;
    }

    if sc.dc_pmode.get() != DC_PMODE_MII {
        if phy == MII_NPHY - 1 {
            return match reg {
                // Fake something to make the probe code think there's a PHY here.
                MII_BMSR => BMSR_MEDIAMASK,
                MII_PHYIDR1 if sc.dc_is_pnic() => PCI_VENDOR_LITEON as i32,
                MII_PHYIDR1 => PCI_VENDOR_DEC as i32,
                MII_PHYIDR2 if sc.dc_is_pnic() => PCI_PRODUCT_LITEON_PNIC as i32,
                MII_PHYIDR2 => PCI_PRODUCT_DEC_21142 as i32,
                _ => 0,
            };
        }
        return 0;
    }

    if sc.dc_is_pnic() {
        sc.csr_write_4(
            DC_PN_MII,
            DC_PN_MIIOPCODE_READ | (phy as u32) << 23 | (reg as u32) << 18,
        );
        for _ in 0..DC_TIMEOUT {
            delay(1);
            let rval = sc.csr_read_4(DC_PN_MII);
            if rval & DC_PN_MII_BUSY == 0 {
                let rval = rval & 0xFFFF;
                return if rval == 0xFFFF { 0 } else { rval as i32 };
            }
        }
        return 0;
    }

    if sc.dc_is_comet() {
        let Some(phy_reg) = dc_comet_reg(reg) else {
            printf(format_args!(
                "{}: phy_read: bad phy register {:x}\n",
                sc.devname(),
                reg
            ));
            return 0;
        };

        let rval = sc.csr_read_4(phy_reg) & 0x0000FFFF;

        if rval == 0xFFFF {
            return 0;
        }
        return rval as i32;
    }

    let mut frame = DcMiiFrame {
        mii_phyaddr: phy as u8,
        mii_regaddr: reg as u8,
        ..DcMiiFrame::default()
    };
    let mut phy_reg = 0;
    if sc.dc_type.get() == DC_TYPE_98713 {
        phy_reg = sc.csr_read_4(DC_NETCFG);
        sc.csr_write_4(DC_NETCFG, phy_reg & !DC_NETCFG_PORTSEL);
    }
    let _ = dc_mii_readreg(sc, &mut frame);
    if sc.dc_type.get() == DC_TYPE_98713 {
        sc.csr_write_4(DC_NETCFG, phy_reg);
    }

    i32::from(frame.mii_data)
}

/// `dc_miibus_writereg`.
pub fn dc_miibus_writereg(self_: &Device, phy: i32, reg: i32, data: i32) {
    let sc = dc_dev_sc(self_);

    if sc.dc_is_admtek() && phy != DC_ADMTEK_PHYADDR {
        return;
    }
    if sc.dc_is_conexant() && phy != DC_CONEXANT_PHYADDR {
        return;
    }

    if sc.dc_is_pnic() {
        sc.csr_write_4(
            DC_PN_MII,
            DC_PN_MIIOPCODE_WRITE | (phy as u32) << 23 | (reg as u32) << 10 | data as u32,
        );
        for _ in 0..DC_TIMEOUT {
            if sc.csr_read_4(DC_PN_MII) & DC_PN_MII_BUSY == 0 {
                break;
            }
        }
        return;
    }

    if sc.dc_is_comet() {
        let Some(phy_reg) = dc_comet_reg(reg) else {
            printf(format_args!(
                "{}: phy_write: bad phy register {:x}\n",
                sc.devname(),
                reg
            ));
            return;
        };

        sc.csr_write_4(phy_reg, data as u32);
        return;
    }

    let mut frame = DcMiiFrame {
        mii_phyaddr: phy as u8,
        mii_regaddr: reg as u8,
        mii_data: data as u16,
        ..DcMiiFrame::default()
    };

    let mut phy_reg = 0;
    if sc.dc_type.get() == DC_TYPE_98713 {
        phy_reg = sc.csr_read_4(DC_NETCFG);
        sc.csr_write_4(DC_NETCFG, phy_reg & !DC_NETCFG_PORTSEL);
    }
    dc_mii_writereg(sc, &mut frame);
    if sc.dc_type.get() == DC_TYPE_98713 {
        sc.csr_write_4(DC_NETCFG, phy_reg);
    }
}

/// `dc_miibus_statchg`: program the MAC for the media the PHY resolved.
pub fn dc_miibus_statchg(self_: &Device) {
    let sc = dc_dev_sc(self_);

    if sc.dc_is_admtek() {
        return;
    }

    let mii = &sc.sc_mii;
    let ifm = &mii.mii_media;
    if sc.dc_is_davicom() && ifm_subtype(ifm.ifm_media.get()) == IFM_HPNA_1 {
        dc_setcfg(sc, ifm.ifm_media.get());
        sc.dc_if_media.set(ifm.ifm_media.get());
    } else {
        dc_setcfg(sc, mii.mii_media_active.get());
        sc.dc_if_media.set(mii.mii_media_active.get());
    }
}

/// The hash table bit of the little-endian CRC `crc` of an address: 9 bits (512-bit table),
/// 7 on the PNIC II and the MX98715AEC-C/D/E (`DC_128BIT_HASH`), 6 on the MX98715BEC
/// (`DC_64BIT_HASH`), and the Xircom's own scheme.
pub fn dc_crc_le_bits(crc: u32, flags: u32, xircom: bool) -> u32 {
    // The hash table on the PNIC II and the MX98715AEC-C/D/E chips is only 128 bits wide.
    if flags & DC_128BIT_HASH != 0 {
        return crc & ((1 << DC_BITS_128) - 1);
    }

    // The hash table on the MX98715BEC is only 64 bits wide.
    if flags & DC_64BIT_HASH != 0 {
        return crc & ((1 << DC_BITS_64) - 1);
    }

    // Xircom's hash filtering table is different (read: weird); Xircom uses the LEAST
    // significant bits.
    if xircom {
        if crc & 0x180 == 0x180 {
            return (crc & 0x0F) + (crc & 0x70) * 3 + (14 << 4);
        } else {
            return (crc & 0x1F) + ((crc >> 1) & 0xF0) * 3 + (12 << 4);
        }
    }

    crc & ((1 << DC_BITS_512) - 1)
}

/// `dc_crc_le`: the hash table bit of `addr` (`ETHER_ADDR_LEN` bytes).
pub fn dc_crc_le(sc: &DcSoftc, addr: &[u8]) -> u32 {
    // Compute CRC for the address value.
    let crc = ether_crc32_le(&addr[..ETHER_ADDR_LEN]);

    dc_crc_le_bits(crc, sc.dc_flags.get(), sc.dc_is_xircom())
}

/// `dc_crc_be`: calculate CRC of a multicast group address, return the lower 6 bits.
pub fn dc_crc_be(addr: &[u8]) -> u32 {
    (ether_crc32_be(&addr[..ETHER_ADDR_LEN]) >> 26) & 0x0000003F
}

/// `sp[h >> 4] |= htole32(1 << (h & 0xF))`: hash bit `h` in the setup frame.
fn dc_sbuf_sethash(sc: &DcSoftc, h: u32) {
    let i = (h >> 4) as usize;
    sc.sbuf_set(i, sc.sbuf_get(i) | (1u32 << (h & 0xF)).to_le());
}

/// The start of a setup frame: takes the transmit producer slot, clears the frame and points
/// the descriptor at it (the shared head of `dc_setfilt_21143` and `dc_setfilt_xircom`).
/// Returns the slot.
fn dc_setfilt_sframe(sc: &DcSoftc) -> usize {
    let cd = &sc.dc_cdata;
    let i = cd.dc_tx_prod.get();
    cd.dc_tx_prod.set(dc_inc(i, DC_TX_LIST_CNT));
    cd.dc_tx_cnt.set(cd.dc_tx_cnt.get() + 1);
    sc.sbuf_zero();

    sc.txd_write(
        i,
        DcDescWord::Data,
        sc.list_addr().wrapping_add(DC_SBUF_OFF as u32),
    );
    sc.txd_write(
        i,
        DcDescWord::Ctl,
        DC_SFRAME_LEN as u32 | DC_TXCTL_SETUP | DC_TXCTL_TLINK | DC_FILTER_HASHPERF | DC_TXCTL_FINT,
    );

    // The C's `sd_mbuf = (struct mbuf *)&dc_sbuf[0]`.
    let sd = cd.tx(i);
    sd.sd_mbuf.set(None);
    sd.sd_setup.set(true);
    i
}

/// The multicast and broadcast hash bits of a setup frame (`dc_setfilt_21143`,
/// `dc_setfilt_xircom`), or all multicast / promiscuous.
fn dc_setfilt_hash(sc: &DcSoftc) {
    let ac = &sc.sc_arpcom;
    let ifp = &ac.ac_if;

    sc.dc_clrbit(DC_NETCFG, DC_NETCFG_RX_ALLMULTI | DC_NETCFG_RX_PROMISC);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_PROMISC);
        } else {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_ALLMULTI);
        }
    } else {
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let h = dc_crc_le(sc, &e.enm_addrlo);

            dc_sbuf_sethash(sc, h);

            enm = ether_next_multi(&mut step);
        }
    }

    // Always accept broadcast frames.
    let h = dc_crc_le(sc, &ETHERBROADCASTADDR);
    dc_sbuf_sethash(sc, h);
}

/// `dc_setfilt_21143`: 21143-style RX filter setup routine. Filter programming is done by
/// downloading a special setup frame into the TX engine. 21143, Macronix, PNIC, PNIC II and
/// Davicom chips are programmed this way.
///
/// We always program the chip using 'hash perfect' mode, i.e. one perfect address (our node
/// address) and a 512-bit hash filter for multicast frames. We also sneak the broadcast
/// address into the hash filter since we need that too.
pub fn dc_setfilt_21143(sc: &'static DcSoftc) {
    let ifp = &sc.sc_arpcom.ac_if;
    let dmat = sc.dmat();

    let i = dc_setfilt_sframe(sc);

    dc_setfilt_hash(sc);

    // Set our MAC address
    let enaddr = sc.sc_arpcom.ac_enaddr.get();
    sc.sbuf_set(39, dc_sp_field(&enaddr, 0));
    sc.sbuf_set(40, dc_sp_field(&enaddr, 1));
    sc.sbuf_set(41, dc_sp_field(&enaddr, 2));

    bus_dmamap_sync(
        dmat,
        sc.listmap(),
        DC_SBUF_OFF as BusAddr,
        (size_of::<DcListData>() - DC_SBUF_OFF) as BusSize,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    sc.txd_write(i, DcDescWord::Status, DC_TXSTAT_OWN);

    bus_dmamap_sync(
        dmat,
        sc.listmap(),
        dc_tx_list_off(i) as BusAddr,
        size_of::<DcDesc>() as BusSize,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    sc.csr_write_4(DC_TXSTART, 0xFFFFFFFF);

    // The PNIC takes an exceedingly long time to process its setup frame; wait 10ms after
    // posting the setup frame before proceeding, just so it has time to swallow its
    // medicine.
    delay(10000);

    ifp.if_timer.set(5);
}

/// `dc_setfilt_admtek`: the ADMtek's filter registers.
pub fn dc_setfilt_admtek(sc: &DcSoftc) {
    let ac = &sc.sc_arpcom;
    let ifp = &ac.ac_if;
    let mut hashes = [0u32; 2];

    sc.dc_clrbit(DC_NETCFG, DC_NETCFG_RX_ALLMULTI | DC_NETCFG_RX_PROMISC);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_PROMISC);
        } else {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_ALLMULTI);
        }
    } else {
        // now program new ones
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let h = if sc.dc_is_centaur() {
                dc_crc_le(sc, &e.enm_addrlo)
            } else {
                dc_crc_be(&e.enm_addrlo)
            };

            if h < 32 {
                hashes[0] |= 1 << h;
            } else {
                hashes[1] |= 1u32.wrapping_shl(h - 32);
            }

            enm = ether_next_multi(&mut step);
        }
    }

    // Init our MAC address
    let e = ac.ac_enaddr.get();
    sc.csr_write_4(
        DC_AL_PAR0,
        u32::from(e[3]) << 24 | u32::from(e[2]) << 16 | u32::from(e[1]) << 8 | u32::from(e[0]),
    );
    sc.csr_write_4(DC_AL_PAR1, u32::from(e[5]) << 8 | u32::from(e[4]));

    sc.csr_write_4(DC_AL_MAR0, hashes[0]);
    sc.csr_write_4(DC_AL_MAR1, hashes[1]);
}

/// `dc_setfilt_asix`: the ASIX's filter registers.
pub fn dc_setfilt_asix(sc: &DcSoftc) {
    let ac = &sc.sc_arpcom;
    let ifp = &ac.ac_if;
    let mut hashes = [0u32; 2];

    sc.dc_clrbit(
        DC_NETCFG,
        DC_NETCFG_RX_ALLMULTI | DC_AX_NETCFG_RX_BROAD | DC_NETCFG_RX_PROMISC,
    );
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    // Always accept broadcast frames.
    sc.dc_setbit(DC_NETCFG, DC_AX_NETCFG_RX_BROAD);

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_PROMISC);
        } else {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_ALLMULTI);
        }
    } else {
        // now program new ones
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let h = dc_crc_be(&e.enm_addrlo);

            if h < 32 {
                hashes[0] |= 1 << h;
            } else {
                hashes[1] |= 1 << (h - 32);
            }

            enm = ether_next_multi(&mut step);
        }
    }

    // Init our MAC address
    let e = ac.ac_enaddr.get();
    sc.csr_write_4(DC_AX_FILTIDX, DC_AX_FILTIDX_PAR0);
    sc.csr_write_4(DC_AX_FILTDATA, u32::from_ne_bytes([e[0], e[1], e[2], e[3]]));
    sc.csr_write_4(DC_AX_FILTIDX, DC_AX_FILTIDX_PAR1);
    sc.csr_write_4(DC_AX_FILTDATA, u32::from_ne_bytes([e[4], e[5], 0, 0]));

    sc.csr_write_4(DC_AX_FILTIDX, DC_AX_FILTIDX_MAR0);
    sc.csr_write_4(DC_AX_FILTDATA, hashes[0]);
    sc.csr_write_4(DC_AX_FILTIDX, DC_AX_FILTIDX_MAR1);
    sc.csr_write_4(DC_AX_FILTDATA, hashes[1]);
}

/// `dc_setfilt_xircom`: the Xircom's setup frame, with the receiver and transmitter stopped
/// around it.
pub fn dc_setfilt_xircom(sc: &DcSoftc) {
    let ifp = &sc.sc_arpcom.ac_if;

    sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_ON | DC_NETCFG_RX_ON);

    let i = dc_setfilt_sframe(sc);

    dc_setfilt_hash(sc);

    // Set our MAC address
    let enaddr = sc.sc_arpcom.ac_enaddr.get();
    sc.sbuf_set(0, dc_sp_field(&enaddr, 0));
    sc.sbuf_set(1, dc_sp_field(&enaddr, 1));
    sc.sbuf_set(2, dc_sp_field(&enaddr, 2));

    sc.dc_setbit(DC_NETCFG, DC_NETCFG_TX_ON);
    sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_ON);
    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    sc.txd_write(i, DcDescWord::Status, DC_TXSTAT_OWN);
    sc.csr_write_4(DC_TXSTART, 0xFFFFFFFF);

    // wait some time...
    delay(1000);

    ifp.if_timer.set(5);
}

/// `dc_setfilt`: program the receive filter the chip's way.
pub fn dc_setfilt(sc: &'static DcSoftc) {
    if sc.dc_is_intel()
        || sc.dc_is_macronix()
        || sc.dc_is_pnic()
        || sc.dc_is_pnicii()
        || sc.dc_is_davicom()
        || sc.dc_is_conexant()
    {
        dc_setfilt_21143(sc);
    }

    if sc.dc_is_asix() {
        dc_setfilt_asix(sc);
    }

    if sc.dc_is_admtek() {
        dc_setfilt_admtek(sc);
    }

    if sc.dc_is_xircom() {
        dc_setfilt_xircom(sc);
    }
}

/// Whether an ISR value shows the receiver stopped or waiting.
fn dc_rx_idle(isr: u32) -> bool {
    let st = isr & DC_ISR_RX_STATE;
    st == DC_RXSTATE_STOPPED || st == DC_RXSTATE_WAIT
}

/// The jabber disable `dc_setcfg` sets for an MII media (the 21143's watchdog register has a
/// write enable bit that reads as 1).
fn dc_setcfg_jabberdis(sc: &DcSoftc) {
    if sc.dc_is_intel() {
        // there's a write enable bit here that reads as 1
        let mut watchdogreg = sc.csr_read_4(DC_WATCHDOG);
        watchdogreg &= !DC_WDOG_CTLWREN;
        watchdogreg |= DC_WDOG_JABBERDIS;
        sc.csr_write_4(DC_WATCHDOG, watchdogreg);
    } else {
        sc.dc_setbit(DC_WATCHDOG, DC_WDOG_JABBERDIS);
    }
}

/// `dc_setcfg`: in order to fiddle with the 'full-duplex' and '100Mbps' bits in the netconfig
/// register, we first have to put the transmit and/or receive logic in the idle state.
pub fn dc_setcfg(sc: &DcSoftc, media: u64) {
    let mut restart = false;

    if ifm_subtype(media) == IFM_NONE {
        return;
    }

    if sc.csr_read_4(DC_NETCFG) & (DC_NETCFG_TX_ON | DC_NETCFG_RX_ON) != 0 {
        restart = true;
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_ON | DC_NETCFG_RX_ON);

        let mut isr = 0;
        let mut i = 0;
        while i < DC_TIMEOUT {
            isr = sc.csr_read_4(DC_ISR);
            if isr & DC_ISR_TX_IDLE != 0 && dc_rx_idle(isr) {
                break;
            }
            delay(10);
            i += 1;
        }

        if i == DC_TIMEOUT {
            if isr & DC_ISR_TX_IDLE == 0 && !sc.dc_is_asix() {
                printf(format_args!(
                    "{}: failed to force tx to idle state\n",
                    sc.devname()
                ));
            }
            if !dc_rx_idle(isr) && !sc.dc_has_broken_rxstate() {
                printf(format_args!(
                    "{}: failed to force rx to idle state\n",
                    sc.devname()
                ));
            }
        }
    }

    let fdx = media & IFM_GMASK == IFM_FDX;

    if ifm_subtype(media) == IFM_100_TX {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_SPEEDSEL);
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_HEARTBEAT);
        if sc.dc_pmode.get() == DC_PMODE_MII {
            dc_setcfg_jabberdis(sc);
            sc.dc_clrbit(
                DC_NETCFG,
                DC_NETCFG_PCS | DC_NETCFG_PORTSEL | DC_NETCFG_SCRAMBLER,
            );
            if sc.dc_type.get() == DC_TYPE_98713 {
                sc.dc_setbit(DC_NETCFG, DC_NETCFG_PCS | DC_NETCFG_SCRAMBLER);
            }
            if !sc.dc_is_davicom() {
                sc.dc_setbit(DC_NETCFG, DC_NETCFG_PORTSEL);
            }
            sc.dc_clrbit(DC_10BTCTRL, 0xFFFF);
            if sc.dc_is_intel() {
                dc_apply_fixup(sc, IFM_AUTO);
            }
        } else {
            if sc.dc_is_pnic() {
                sc.dc_pn_gpio_setbit(DC_PN_GPIO_SPEEDSEL);
                sc.dc_pn_gpio_setbit(DC_PN_GPIO_100TX_LOOP);
                sc.dc_setbit(DC_PN_NWAY, DC_PN_NWAY_SPEEDSEL);
            }
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_PORTSEL);
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_PCS);
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_SCRAMBLER);
            if sc.dc_is_intel() {
                dc_apply_fixup(
                    sc,
                    if fdx {
                        IFM_100_TX | IFM_FDX
                    } else {
                        IFM_100_TX
                    },
                );
            }
        }
    }

    if ifm_subtype(media) == IFM_10_T {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_SPEEDSEL);
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_HEARTBEAT);
        if sc.dc_pmode.get() == DC_PMODE_MII {
            dc_setcfg_jabberdis(sc);
            sc.dc_clrbit(
                DC_NETCFG,
                DC_NETCFG_PCS | DC_NETCFG_PORTSEL | DC_NETCFG_SCRAMBLER,
            );
            if sc.dc_type.get() == DC_TYPE_98713 {
                sc.dc_setbit(DC_NETCFG, DC_NETCFG_PCS);
            }
            if !sc.dc_is_davicom() {
                sc.dc_setbit(DC_NETCFG, DC_NETCFG_PORTSEL);
            }
            sc.dc_clrbit(DC_10BTCTRL, 0xFFFF);
            if sc.dc_is_intel() {
                dc_apply_fixup(sc, IFM_AUTO);
            }
        } else {
            if sc.dc_is_pnic() {
                sc.dc_pn_gpio_clrbit(DC_PN_GPIO_SPEEDSEL);
                sc.dc_pn_gpio_setbit(DC_PN_GPIO_100TX_LOOP);
                sc.dc_clrbit(DC_PN_NWAY, DC_PN_NWAY_SPEEDSEL);
            }
            sc.dc_clrbit(DC_NETCFG, DC_NETCFG_PORTSEL);
            sc.dc_clrbit(DC_NETCFG, DC_NETCFG_PCS);
            sc.dc_clrbit(DC_NETCFG, DC_NETCFG_SCRAMBLER);
            if sc.dc_is_intel() {
                sc.dc_clrbit(DC_SIARESET, DC_SIA_RESET);
                sc.dc_clrbit(DC_10BTCTRL, 0xFFFF);
                if fdx {
                    sc.dc_setbit(DC_10BTCTRL, 0x7F3D);
                } else {
                    sc.dc_setbit(DC_10BTCTRL, 0x7F3F);
                }
                sc.dc_setbit(DC_SIARESET, DC_SIA_RESET);
                sc.dc_clrbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
                dc_apply_fixup(sc, if fdx { IFM_10_T | IFM_FDX } else { IFM_10_T });
                delay(20000);
            }
        }
    }

    // If this is a Davicom DM9102A card with a DM9801 HomePNA PHY and we want HomePNA mode,
    // set the portsel bit to turn on the external MII port.
    if sc.dc_is_davicom() {
        if ifm_subtype(media) == IFM_HPNA_1 {
            sc.dc_setbit(DC_NETCFG, DC_NETCFG_PORTSEL);
            sc.dc_link.set(1);
        } else {
            sc.dc_clrbit(DC_NETCFG, DC_NETCFG_PORTSEL);
        }
    }

    if fdx {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_FULLDUPLEX);
        if sc.dc_pmode.get() == DC_PMODE_SYM && sc.dc_is_pnic() {
            sc.dc_setbit(DC_PN_NWAY, DC_PN_NWAY_DUPLEX);
        }
    } else {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_FULLDUPLEX);
        if sc.dc_pmode.get() == DC_PMODE_SYM && sc.dc_is_pnic() {
            sc.dc_clrbit(DC_PN_NWAY, DC_PN_NWAY_DUPLEX);
        }
    }

    if restart {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_TX_ON | DC_NETCFG_RX_ON);
    }
}

/// `dc_reset`: reset the chip and bring its SIA out of reset.
pub fn dc_reset(sc: &DcSoftc) {
    sc.dc_setbit(DC_BUSCTL, DC_BUSCTL_RESET);

    let mut i = 0;
    while i < DC_TIMEOUT {
        delay(10);
        if sc.csr_read_4(DC_BUSCTL) & DC_BUSCTL_RESET == 0 {
            break;
        }
        i += 1;
    }

    if sc.dc_is_asix()
        || sc.dc_is_admtek()
        || sc.dc_is_xircom()
        || sc.dc_is_intel()
        || sc.dc_is_conexant()
    {
        delay(10000);
        sc.dc_clrbit(DC_BUSCTL, DC_BUSCTL_RESET);
        i = 0;
    }

    if i == DC_TIMEOUT {
        printf(format_args!("{}: reset never completed!\n", sc.devname()));
    }

    // Wait a little while for the chip to get its brains in order.
    delay(1000);

    sc.csr_write_4(DC_IMR, 0x00000000);
    sc.csr_write_4(DC_BUSCTL, 0x00000000);
    sc.csr_write_4(DC_NETCFG, 0x00000000);

    // Bring the SIA out of reset. In some cases, it looks like failing to unreset the SIA
    // soon enough gets it into a state where it will never come out of reset until we reset
    // the whole chip again.
    if sc.dc_is_intel() {
        sc.dc_setbit(DC_SIARESET, DC_SIA_RESET);
        sc.csr_write_4(DC_10BTCTRL, 0);
        sc.csr_write_4(DC_WATCHDOG, 0);
    }

    if sc.dc_type.get() == DC_TYPE_21145 {
        dc_setcfg(sc, IFM_10_T);
    }
}

/// The words `dc_apply_fixup` writes to the watchdog/GPIO register for `len` 16-bit SROM
/// words at `off`: each shifted to the upper half; the words past the SROM's end are left
/// out.
pub fn dc_fixup_words(srom: &[u8], off: usize, len: u8) -> impl Iterator<Item = u32> + '_ {
    (0..usize::from(len)).map_while(move |i| {
        let p = off + 2 * i;
        let b = srom.get(p..p + 2)?;
        Some((u32::from(b[0]) | u32::from(b[1]) << 8) << 16)
    })
}

/// `dc_apply_fixup`: the SROM's reset and GPIO sequences of `media`, if it lists them.
pub fn dc_apply_fixup(sc: &DcSoftc, media: u64) {
    let mut m = sc.dc_mi.get();

    let mi = loop {
        let Some(p) = m else {
            return;
        };
        // SAFETY: the list holds DcMediainfo dc_decode_leaf_* allocated; they stay until
        // the device goes.
        let mi = unsafe { p.as_ref() };
        if mi.dc_media == media {
            break mi;
        }
        m = mi.dc_next;
    };

    let srom = sc.srom();
    for reg in dc_fixup_words(srom, mi.dc_reset_ptr, mi.dc_reset_len) {
        sc.csr_write_4(DC_WATCHDOG, reg);
    }

    for reg in dc_fixup_words(srom, mi.dc_gp_ptr, mi.dc_gp_len) {
        sc.csr_write_4(DC_WATCHDOG, reg);
    }
}

/// Puts a new `struct dc_mediainfo` at the head of `sc->dc_mi` (the tail of the
/// `dc_decode_leaf_*` functions); nothing when the allocation fails, as in C.
fn dc_mediainfo_add(sc: &DcSoftc, mut m: DcMediainfo) {
    let Some(p) = malloc(size_of::<DcMediainfo>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return;
    };
    let p = p.cast::<DcMediainfo>();
    m.dc_next = sc.dc_mi.get();
    // SAFETY: a fresh allocation of a `DcMediainfo`'s size, aligned as malloc(9) aligns
    // every allocation.
    unsafe { p.as_ptr().write(m) };
    sc.dc_mi.set(Some(p));
}

/// The media of an SIA block's code (`dc_decode_leaf_sia`); 0 for an unknown one.
pub fn dc_sia_media(code: u8) -> u64 {
    match code & !DC_SIA_CODE_EXT {
        DC_SIA_CODE_10BT => IFM_10_T,
        DC_SIA_CODE_10BT_FDX => IFM_10_T | IFM_FDX,
        DC_SIA_CODE_10B2 => crate::net::if_media::IFM_10_2,
        DC_SIA_CODE_10B5 => crate::net::if_media::IFM_10_5,
        _ => 0,
    }
}

/// `dc_decode_leaf_sia`: the SIA block at `off` of the SROM.
pub fn dc_decode_leaf_sia(sc: &DcSoftc, off: usize) {
    let srom = sc.srom();
    let code = srom_byte(srom, off + offset_of!(DcEblockSia, dc_sia_code));
    let un = off + offset_of!(DcEblockSia, dc_un);

    let mut m = DcMediainfo {
        dc_media: dc_sia_media(code),
        ..DcMediainfo::default()
    };

    // We need to ignore CSR13, CSR14, CSR15 for SIA mode. Things apparently already work for
    // cards that do supply Media Specific Data.
    m.dc_gp_len = 2;
    if code & DC_SIA_CODE_EXT != 0 {
        m.dc_gp_ptr = un + offset_of!(DcSiaExt, dc_sia_gpio_ctl);
    } else {
        m.dc_gp_ptr = un + offset_of!(DcSiaNoext, dc_sia_gpio_ctl);
    }

    dc_mediainfo_add(sc, m);

    sc.dc_pmode.set(DC_PMODE_SIA);
}

/// `dc_decode_leaf_sym`: the SYM block at `off` of the SROM.
pub fn dc_decode_leaf_sym(sc: &DcSoftc, off: usize) {
    let code = srom_byte(sc.srom(), off + offset_of!(DcEblockSym, dc_sym_code));

    let mut m = DcMediainfo::default();
    if code == DC_SYM_CODE_100BT {
        m.dc_media = IFM_100_TX;
    }

    if code == DC_SYM_CODE_100BT_FDX {
        m.dc_media = IFM_100_TX | IFM_FDX;
    }

    m.dc_gp_len = 2;
    m.dc_gp_ptr = off + offset_of!(DcEblockSym, dc_sym_gpio_ctl);

    dc_mediainfo_add(sc, m);

    sc.dc_pmode.set(DC_PMODE_SYM);
}

/// The `struct dc_mediainfo` of the MII block at `off` of `srom`: its GPIO sequence follows
/// the block header, its reset length and sequence follow that.
pub fn dc_leaf_mii_info(srom: &[u8], off: usize) -> DcMediainfo {
    let gpr_len = srom_byte(srom, off + offset_of!(DcEblockMii, dc_gpr_len));

    // We abuse IFM_AUTO to represent MII.
    let gp_ptr = off + size_of::<DcEblockMii>();
    let p = gp_ptr + 2 * usize::from(gpr_len);
    DcMediainfo {
        dc_media: IFM_AUTO,
        dc_gp_len: gpr_len,
        dc_gp_ptr: gp_ptr,
        dc_reset_len: srom_byte(srom, p),
        dc_reset_ptr: p + 1,
        dc_next: None,
    }
}

/// `dc_decode_leaf_mii`: the MII block at `off` of the SROM.
pub fn dc_decode_leaf_mii(sc: &DcSoftc, off: usize) {
    dc_mediainfo_add(sc, dc_leaf_mii_info(sc.srom(), off));
}

/// `dc_read_srom`: read the whole SROM (`2 << bits` bytes) into `dc_srom`.
pub fn dc_read_srom(sc: &DcSoftc, bits: i32) {
    let size = 2usize << bits;
    sc.dc_sromsize.set(size);
    let Some(p) = malloc(size, M_DEVBUF, M_NOWAIT) else {
        sc.dc_srom.set(ptr::null_mut());
        return;
    };
    // SAFETY: a fresh allocation of `size` bytes, ours alone.
    let buf = unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), size) };
    dc_read_eeprom(sc, buf, 0, false);
    sc.dc_srom.set(p.as_ptr());
}

/// The media blocks of a 21143 SROM's first leaf, as `(type, offset)`: the leaf's offset is
/// byte 27, its block count the leaf header's `dc_mcnt`, and each block's length the low 7
/// bits of its first byte (`dc_parse_21143_srom`'s walk).
pub fn dc_srom_blocks(srom: &[u8]) -> impl Iterator<Item = (u8, usize)> + '_ {
    let loff = usize::from(srom_byte(srom, 27));
    let mcnt = srom_byte(srom, loff + offset_of!(DcLeafHdr, dc_mcnt));
    let mut ptr = loff + size_of::<DcLeafHdr>() - 1;
    (0..mcnt).map(move |_| {
        let off = ptr;
        let len = srom_byte(srom, off + offset_of!(DcEblockHdr, dc_len));
        let type_ = srom_byte(srom, off + offset_of!(DcEblockHdr, dc_type));
        ptr += usize::from(len & 0x7F) + 1;
        (type_, off)
    })
}

/// `dc_parse_21143_srom`: the media of the 21143's SROM. Only use SIA and SYM media blocks
/// if no MII media block is available.
pub fn dc_parse_21143_srom(sc: &DcSoftc) {
    // Look if we got a MII media block.
    let have_mii = dc_srom_blocks(sc.srom()).any(|(t, _)| t == DC_EBLOCK_MII);

    // Do the same thing again.
    for (type_, off) in dc_srom_blocks(sc.srom()) {
        match type_ {
            DC_EBLOCK_MII => dc_decode_leaf_mii(sc, off),
            DC_EBLOCK_SIA if !have_mii => dc_decode_leaf_sia(sc, off),
            DC_EBLOCK_SYM if !have_mii => dc_decode_leaf_sym(sc, off),
            // Don't care. Yet.
            _ => {}
        }
    }
}

/// The station address from the EEPROM or the chip, by chip type (the switch of
/// `dc_attach`).
fn dc_read_enaddr(sc: &DcSoftc) -> [u8; ETHER_ADDR_LEN] {
    let mut enaddr = [0u8; ETHER_ADDR_LEN];

    match sc.dc_type.get() {
        DC_TYPE_98713 | DC_TYPE_98713A | DC_TYPE_987x5 | DC_TYPE_PNICII => {
            let mut w = [0u8; 2];
            dc_read_eeprom(sc, &mut w, DC_EE_NODEADDR_OFFSET / 2, false);
            let mac_offset = i32::from(u16::from_ne_bytes(w));
            dc_read_eeprom(sc, &mut enaddr, mac_offset / 2, false);
        }
        DC_TYPE_PNIC => dc_read_eeprom(sc, &mut enaddr, 0, true),
        DC_TYPE_DM9102 | DC_TYPE_21143 | DC_TYPE_21145 | DC_TYPE_ASIX => {
            dc_read_eeprom(sc, &mut enaddr, DC_EE_NODEADDR, false);
        }
        DC_TYPE_AL981 | DC_TYPE_AN983 => {
            let reg = sc.csr_read_4(DC_AL_PAR0);
            enaddr[0] = reg as u8;
            enaddr[1] = (reg >> 8) as u8;
            enaddr[2] = (reg >> 16) as u8;
            enaddr[3] = (reg >> 24) as u8;
            let reg = sc.csr_read_4(DC_AL_PAR1);
            enaddr[4] = reg as u8;
            enaddr[5] = (reg >> 8) as u8;
        }
        DC_TYPE_CONEXANT => {
            let off = DC_CONEXANT_EE_NODEADDR;
            if let Some(a) = sc.srom().get(off..off + ETHER_ADDR_LEN) {
                enaddr.copy_from_slice(a);
            }
        }
        DC_TYPE_XIRCOM => {
            // Some newer units have the MAC at offset 8
            dc_read_eeprom(sc, &mut enaddr, 8, false);

            if enaddr[..3] != [0x00, 0x10, 0xa4] && enaddr[..3] != [0x00, 0x80, 0xc7] {
                dc_read_eeprom(sc, &mut enaddr, 3, false);
            }
        }
        _ => dc_read_eeprom(sc, &mut enaddr, DC_EE_NODEADDR, false),
    }

    enaddr
}

/// `dc_attach`: attach the interface. Allocate softc structures, do ifmedia setup and
/// ethernet/BPF attach. On a failure it prints why and returns, leaving what it made, as in
/// C.
pub fn dc_attach(sc: &'static DcSoftc) {
    let dmat = sc.dmat();

    // Get station address from the EEPROM.
    if sc.sc_hasmac.get() == 0 {
        sc.sc_arpcom.ac_enaddr.set(dc_read_enaddr(sc));
    }
    // hasmac:

    let size = size_of::<DcListData>();
    let mut seg = [BusDmaSegment::default(); 1];
    let Ok(nseg) = bus_dmamem_alloc(
        dmat,
        size as BusSize,
        PAGE_SIZE as BusSize,
        0,
        &mut seg,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) else {
        printf(format_args!(": can't alloc list mem\n"));
        return;
    };
    sc.sc_listseg.set(seg);
    sc.sc_listnseg.set(nseg);
    let Ok(kva) = bus_dmamem_map(dmat, &mut seg[..nseg], size, BUS_DMA_NOWAIT) else {
        printf(format_args!(": can't map list mem\n"));
        return;
    };
    sc.sc_listkva.set(kva.as_ptr());
    let Ok(listmap) =
        bus_dmamap_create(dmat, size as BusSize, 1, size as BusSize, 0, BUS_DMA_NOWAIT)
    else {
        printf(format_args!(": can't alloc list map\n"));
        return;
    };
    sc.sc_listmap.set(Some(listmap));
    // SAFETY: the list memory stays mapped until dc_detach unloads the map before unmapping
    // it; the chip is not pointed at it before dc_init.
    if unsafe {
        bus_dmamap_load(
            dmat,
            listmap,
            kva.as_ptr(),
            size as BusSize,
            None,
            BUS_DMA_NOWAIT,
        )
    }
    .is_err()
    {
        printf(format_args!(": can't load list map\n"));
        return;
    }
    sc.dc_ldata.set(kva.as_ptr().cast());

    for sd in sc.dc_cdata.dc_rx_chain.iter() {
        let Ok(map) = bus_dmamap_create(
            dmat,
            MCLBYTES as BusSize,
            1,
            MCLBYTES as BusSize,
            0,
            BUS_DMA_NOWAIT,
        ) else {
            printf(format_args!(": can't create rx map\n"));
            return;
        };
        sd.sd_map.set(Some(map));
    }
    let Ok(map) = bus_dmamap_create(
        dmat,
        MCLBYTES as BusSize,
        1,
        MCLBYTES as BusSize,
        0,
        BUS_DMA_NOWAIT,
    ) else {
        printf(format_args!(": can't create rx spare map\n"));
        return;
    };
    sc.sc_rx_sparemap.set(Some(map));

    let tx_nsegs = if sc.has_flags(DC_TX_COALESCE) {
        1
    } else {
        DC_TX_LIST_CNT as i32 - 5
    };
    for sd in sc.dc_cdata.dc_tx_chain.iter() {
        let Ok(map) = bus_dmamap_create(
            dmat,
            MCLBYTES as BusSize,
            tx_nsegs,
            MCLBYTES as BusSize,
            0,
            BUS_DMA_NOWAIT,
        ) else {
            printf(format_args!(": can't create tx map\n"));
            return;
        };
        sd.sd_map.set(Some(map));
    }
    let Ok(map) = bus_dmamap_create(
        dmat,
        MCLBYTES as BusSize,
        tx_nsegs,
        MCLBYTES as BusSize,
        0,
        BUS_DMA_NOWAIT,
    ) else {
        printf(format_args!(": can't create tx spare map\n"));
        return;
    };
    sc.sc_tx_sparemap.set(Some(map));

    // A 21143 or clone chip was detected. Inform the world.
    printf(format_args!(
        ", address {}\n",
        Str(&ether_sprintf(&sc.sc_arpcom.ac_enaddr.get()))
    ));

    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    ifp.if_softc.set(dc_ptr(sc));
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_ioctl.set(Some(dc_ioctl));
    ifp.if_start.set(Some(dc_start));
    ifp.if_watchdog.set(Some(dc_watchdog));
    ifq_init_maxlen(&ifp.if_snd, DC_TX_LIST_CNT as u32 - 1);
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let n = name.len().min(IFNAMSIZ);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);

    ifp.if_capabilities.set(IFCAP_VLAN_MTU);

    // Do MII setup. If this is a 21143, check for a PHY on the MII bus after applying any
    // necessary fixups to twiddle the GPIO bits. If we don't end up finding a PHY, restore
    // the old selection (SIA only or SIA/SYM) and attach the dcphy driver instead.
    let mut tmp = 0;
    if sc.dc_is_intel() {
        dc_apply_fixup(sc, IFM_AUTO);
        tmp = sc.dc_pmode.get();
        sc.dc_pmode.set(DC_PMODE_MII);
    }

    // Setup General Purpose port mode and data so the tulip can talk to the MII. This needs
    // to be done before mii_attach so that we can actually see them.
    if sc.dc_is_xircom() {
        dc_xircom_gpio(sc);
    }

    let mii = &sc.sc_mii;
    mii.mii_ifp.set(Some(ifp));
    mii.mii_readreg.set(Some(dc_miibus_readreg));
    mii.mii_writereg.set(Some(dc_miibus_writereg));
    mii.mii_statchg.set(Some(dc_miibus_statchg));
    ifmedia_init(&mii.mii_media, 0, dc_ifmedia_upd, dc_ifmedia_sts);
    mii_attach(
        &sc.sc_dev,
        mii,
        0xffff_ffff_u32 as i32,
        MII_PHY_ANY,
        MII_OFFSET_ANY,
        0,
    );

    if sc.dc_is_intel() {
        if mii.mii_phys.first().is_none() {
            sc.dc_pmode.set(tmp);
            if sc.dc_pmode.get() != DC_PMODE_SIA {
                sc.dc_pmode.set(DC_PMODE_SYM);
            }
            sc.set_flags(DC_21143_NWAY);
            if sc.has_flags(DC_MOMENCO_BOTCH) {
                sc.dc_pmode.set(DC_PMODE_MII);
            }
            mii_attach(
                &sc.sc_dev,
                mii,
                0xffff_ffff_u32 as i32,
                MII_PHY_ANY,
                MII_OFFSET_ANY,
                0,
            );
        } else {
            // we have a PHY, so we must clear this bit
            sc.dc_flags.set(sc.dc_flags.get() & !DC_TULIP_LEDS);
        }
    }

    if mii.mii_phys.first().is_none() {
        ifmedia_add(&mii.mii_media, IFM_ETHER | IFM_NONE, 0, ptr::null_mut());
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_NONE);
        printf(format_args!("{}: MII without any PHY!\n", sc.devname()));
    } else if sc.dc_type.get() == DC_TYPE_21145 {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_10_T);
    } else {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_AUTO);
    }

    if sc.dc_is_davicom() && sc.dc_revision.get() >= DC_REVISION_DM9102A {
        ifmedia_add(&mii.mii_media, IFM_ETHER | IFM_HPNA_1, 0, ptr::null_mut());
    }

    if sc.dc_is_admtek() {
        // Set automatic TX underrun recovery for the ADMtek chips
        sc.dc_setbit(DC_AL_CR, DC_AL_CR_ATUR);
    }

    // Call MI attach routines.
    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);
}

/// The Xircom's general purpose port setup (`dc_attach` and `dc_init`).
fn dc_xircom_gpio(sc: &DcSoftc) {
    sc.csr_write_4(
        DC_SIAGP,
        DC_SIAGP_WRITE_EN | DC_SIAGP_INT1_EN | DC_SIAGP_MD_GP2_OUTPUT | DC_SIAGP_MD_GP0_OUTPUT,
    );
    delay(10);
    sc.csr_write_4(
        DC_SIAGP,
        DC_SIAGP_INT1_EN | DC_SIAGP_MD_GP2_OUTPUT | DC_SIAGP_MD_GP0_OUTPUT,
    );
    delay(10);
}

/// `dc_list_tx_init`: initialize the transmit descriptors.
pub fn dc_list_tx_init(sc: &DcSoftc) -> Result<(), Errno> {
    let cd = &sc.dc_cdata;
    let base = sc.list_addr();

    for i in 0..DC_TX_LIST_CNT {
        let next = if i == DC_TX_LIST_CNT - 1 {
            base.wrapping_add(dc_tx_list_off(0) as u32)
        } else {
            base.wrapping_add(dc_tx_list_off(i + 1) as u32)
        };
        cd.tx(i).clear_mbuf();
        sc.txd_write(i, DcDescWord::Data, 0);
        sc.txd_write(i, DcDescWord::Ctl, 0);
        sc.txd_write(i, DcDescWord::Next, next);
    }

    cd.dc_tx_prod.set(0);
    cd.dc_tx_cons.set(0);
    cd.dc_tx_cnt.set(0);

    Ok(())
}

/// `dc_list_rx_init`: initialize the RX descriptors and allocate mbufs for them. The
/// descriptors are a closed ring: the last points back to the first.
pub fn dc_list_rx_init(sc: &'static DcSoftc) -> Result<(), Errno> {
    let base = sc.list_addr();

    for i in 0..DC_RX_LIST_CNT {
        if dc_newbuf(sc, i, None) == Err(Errno::ENOBUFS) {
            return Err(Errno::ENOBUFS);
        }
        let next = if i == DC_RX_LIST_CNT - 1 {
            base.wrapping_add(dc_rx_list_off(0) as u32)
        } else {
            base.wrapping_add(dc_rx_list_off(i + 1) as u32)
        };
        sc.rxd_write(i, DcDescWord::Next, next);
    }

    sc.dc_cdata.dc_rx_prod.set(0);

    Ok(())
}

/// `dc_newbuf`: initialize an RX descriptor and attach an mbuf cluster: a new one when `m` is
/// `None`, else `m` again.
pub fn dc_newbuf(sc: &'static DcSoftc, i: usize, m: Option<&'static Mbuf>) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let cd = &sc.dc_cdata;

    let m_new = match m {
        None => {
            let Some(m_new) = m_clget(None, M_DONTWAIT, MCLBYTES as u32) else {
                return Err(Errno::ENOBUFS);
            };
            m_new.m_len().set(MCLBYTES as u32);
            m_new.m_pkthdr().len.set(MCLBYTES as i32);
            // SAFETY: the cluster stays the slot's (`sd_mbuf`) until dc_rxeof, which reuses
            // it, or dc_stop, which unloads the map before freeing it.
            if unsafe { bus_dmamap_load_mbuf(dmat, sc.rx_sparemap(), m_new, BUS_DMA_NOWAIT) }
                .is_err()
            {
                m_freem(m_new);
                return Err(Errno::ENOBUFS);
            }
            let map = cd.rx(i).map();
            cd.rx(i).sd_map.set(Some(sc.rx_sparemap()));
            sc.sc_rx_sparemap.set(Some(map));
            m_new
        }
        Some(m_new) => {
            // We're re-using a previously allocated mbuf; be sure to re-init pointers and
            // lengths to default values.
            m_new.m_len().set(MCLBYTES as u32);
            m_new.m_pkthdr().len.set(MCLBYTES as i32);
            m_new.m_data().set(m_new.m_ext().ext_buf.get());
            m_new
        }
    };

    m_adj(m_new, DC_RX_SKIP as i32);

    // If this is a PNIC chip, zero the buffer. This is part of the workaround for the
    // receive bug in the 82c168 and 82c169 chips.
    if sc.has_flags(DC_PNIC_RX_BUG_WAR) {
        // SAFETY: the mbuf's data is `m_len` bytes of its cluster.
        unsafe { ptr::write_bytes(mtod::<u8>(m_new), 0, m_new.m_len().get() as usize) };
    }

    let sd = cd.rx(i);
    let map = sd.map();
    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

    sd.sd_mbuf.set(Some(m_new));
    sd.sd_setup.set(false);
    let addr = map.dm_segs()[0].get().ds_addr as u32;
    sc.rxd_write(i, DcDescWord::Data, addr.wrapping_add(DC_RX_SKIP as u32));
    sc.rxd_write(
        i,
        DcDescWord::Ctl,
        DC_RXCTL_RLINK | ETHER_MAX_DIX_LEN as u32,
    );
    sc.rxd_write(i, DcDescWord::Status, DC_RXSTAT_OWN);

    bus_dmamap_sync(
        dmat,
        sc.listmap(),
        dc_rx_list_off(i) as BusAddr,
        size_of::<DcDesc>() as BusSize,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    Ok(())
}

/// Where `dc_pnic_rx_bug_war` finds the salvaged frame in its buffer `buf`, of which the
/// first `end` bytes are the copied buffers: scanning back from `end` to the last non-zero
/// byte (the CRC is always there), rounding off, then back by the frame's length
/// `total_len`; never before the start.
pub fn dc_pnic_salvage(buf: &[u8], end: usize, total_len: usize) -> usize {
    // Scan backwards until we hit a non-zero byte.
    let mut p = end;
    while p > 0 && buf.get(p).copied().unwrap_or(0) == 0x00 {
        p -= 1;
    }

    // Round off.
    if p & 0x3 != 0 {
        p -= 1;
    }

    // Now find the start of the frame.
    p.saturating_sub(total_len)
}

/// `dc_pnic_rx_bug_war`: the PNIC chip has a terrible bug in it that manifests itself during
/// periods of heavy activity. Sometimes instead of uploading one complete frame during
/// reception, it uploads what looks like the entire contents of its FIFO memory. The frame
/// we want is at the end of the whole mess, but we never know exactly how much data has been
/// uploaded, so salvaging the frame is hard.
///
/// We know there will always be somewhere between one and three extra descriptors uploaded,
/// that the desired frame will always be at the end of the total data upload, and its size,
/// from the length field of the status word in the last descriptor. So the receive buffers
/// are zeroed when they are given to the chip (`dc_newbuf`), the chip uploads frames with
/// their CRC, and the bogus frame data is gathered into a single buffer; from its end we scan
/// backwards to the first non-zero byte, the end of the received frame (the CRC is always
/// there, so a frame of zeros does not fool us), subtract the frame's size, and copy the
/// frame into the last mbuf, faking up the status word of a successful reception. The
/// performance hit is tremendous, but it beats dropping frames all the time.
pub fn dc_pnic_rx_bug_war(sc: &'static DcSoftc, idx: usize) {
    let cd = &sc.dc_cdata;
    let bufp = sc.dc_pnic_rx_buf.get();
    if bufp.is_null() {
        panic(format_args!("{}: no PNIC receive buffer", sc.devname()));
    }
    let buflen = ETHER_MAX_DIX_LEN * 5;
    // SAFETY: dc_pci_attach allocated `ETHER_MAX_DIX_LEN * 5` bytes for the PNIC; only the
    // interrupt path uses them, under the kernel lock.
    let buf = unsafe { core::slice::from_raw_parts_mut(bufp, buflen) };

    let mut i = sc.dc_pnic_rx_bug_save.get();
    buf.fill(0);
    let mut end = 0;

    // Copy all the bytes from the bogus buffers.
    let (rxstat, m) = loop {
        let rxstat = sc.rxd_read(i, DcDescWord::Status);
        let Some(m) = cd.rx(i).sd_mbuf.get() else {
            panic(format_args!(
                "{}: rx slot {i} without an mbuf",
                sc.devname()
            ));
        };
        if end + ETHER_MAX_DIX_LEN <= buflen {
            // SAFETY: a receive cluster holds at least ETHER_MAX_DIX_LEN bytes past the
            // 8 bytes dc_newbuf skips.
            let src = unsafe { core::slice::from_raw_parts(mtod::<u8>(m), ETHER_MAX_DIX_LEN) };
            buf[end..end + ETHER_MAX_DIX_LEN].copy_from_slice(src);
            end += ETHER_MAX_DIX_LEN;
        }
        // If this is the last buffer, break out.
        if i == idx || rxstat & DC_RXSTAT_LASTFRAG != 0 {
            break (rxstat, m);
        }
        let _ = dc_newbuf(sc, i, Some(m));
        i = dc_inc(i, DC_RX_LIST_CNT);
    };

    // Find the length of the actual receive frame.
    let total_len = dc_rxbytes(rxstat) as usize;

    let start = dc_pnic_salvage(buf, end, total_len);

    // Now copy the salvaged frame to the last mbuf and fake up the status word to make it
    // look like a successful frame reception.
    let _ = dc_newbuf(sc, i, Some(m));
    let n = total_len.min(buflen - start).min(m.m_len().get() as usize);
    // SAFETY: `n` bytes fit in the cluster (its `m_len`) and in the buffer.
    unsafe { ptr::copy_nonoverlapping(buf[start..].as_ptr(), mtod::<u8>(m), n) };
    sc.rxd_write(idx, DcDescWord::Status, rxstat | DC_RXSTAT_FIRSTFRAG);
}

/// `dc_rx_resync`: search the RX ring for dirty descriptors in the event that the rxeof
/// routine falls out of sync with the chip's current descriptor pointer. This may happen
/// sometimes as a result of a "no RX buffer available" condition that happens when the chip
/// consumes all of the RX buffers before the driver has a chance to process the RX ring.
/// This routine may need to be called more than once to bring the driver back in sync with
/// the chip, however we should still be getting RX DONE interrupts to drive the search for
/// new packets in the RX ring, so we should catch up eventually. Returns whether it moved
/// (the C's `EAGAIN`).
pub fn dc_rx_resync(sc: &DcSoftc) -> bool {
    let mut pos = sc.dc_cdata.dc_rx_prod.get();

    let mut i = 0;
    while i < DC_RX_LIST_CNT {
        bus_dmamap_sync(
            sc.dmat(),
            sc.listmap(),
            dc_rx_list_off(pos) as BusAddr,
            size_of::<DcDesc>() as BusSize,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        if sc.rxd_read(pos, DcDescWord::Status) & DC_RXSTAT_OWN == 0 {
            break;
        }
        pos = dc_inc(pos, DC_RX_LIST_CNT);
        i += 1;
    }

    // If the ring really is empty, then just return.
    if i == DC_RX_LIST_CNT {
        return false;
    }

    // We've fallen behind the chip: catch it.
    sc.dc_cdata.dc_rx_prod.set(pos);

    true
}

/// `dc_rxeof`: a frame has been uploaded: pass the resulting mbuf chain up to the higher
/// level protocols. Returns the frames passed up.
pub fn dc_rxeof(sc: &'static DcSoftc) -> i32 {
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let dmat = sc.dmat();
    let cd = &sc.dc_cdata;
    let ml = MbufList::new();
    let mut consumed = 0;

    let mut i = cd.dc_rx_prod.get();

    loop {
        bus_dmamap_sync(
            dmat,
            sc.listmap(),
            dc_rx_list_off(i) as BusAddr,
            size_of::<DcDesc>() as BusSize,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        let mut rxstat = sc.rxd_read(i, DcDescWord::Status);
        if rxstat & DC_RXSTAT_OWN != 0 {
            break;
        }

        let sd = cd.rx(i);
        let Some(m) = sd.sd_mbuf.get() else {
            panic(format_args!(
                "{}: rx slot {i} without an mbuf",
                sc.devname()
            ));
        };
        let mut total_len = dc_rxbytes(rxstat) as i32;

        let map = sd.map();
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);

        if sc.has_flags(DC_PNIC_RX_BUG_WAR) && rxstat & DC_WHOLEFRAME != DC_WHOLEFRAME {
            if rxstat & DC_RXSTAT_FIRSTFRAG != 0 {
                sc.dc_pnic_rx_bug_save.set(i);
            }
            if rxstat & DC_RXSTAT_LASTFRAG == 0 {
                i = dc_inc(i, DC_RX_LIST_CNT);
                continue;
            }
            dc_pnic_rx_bug_war(sc, i);
            rxstat = sc.rxd_read(i, DcDescWord::Status);
            total_len = dc_rxbytes(rxstat) as i32;
        }

        sd.clear_mbuf();

        // If an error occurs, update stats, clear the status word and leave the mbuf cluster
        // in place: it should simply get re-used next time this descriptor comes up in the
        // ring. However, don't report long frames as errors since they could be VLANs.
        if rxstat & DC_RXSTAT_RXERR != 0
            && (rxstat & DC_RXSTAT_GIANT == 0
                || rxstat
                    & (DC_RXSTAT_CRCERR
                        | DC_RXSTAT_DRIBBLE
                        | DC_RXSTAT_MIIERE
                        | DC_RXSTAT_COLLSEEN
                        | DC_RXSTAT_RUNT
                        | DC_RXSTAT_DE)
                    != 0)
        {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            if rxstat & DC_RXSTAT_COLLSEEN != 0 {
                ifp.if_collisions().set(ifp.if_collisions().get() + 1);
            }
            let _ = dc_newbuf(sc, i, Some(m));
            if rxstat & DC_RXSTAT_CRCERR != 0 {
                i = dc_inc(i, DC_RX_LIST_CNT);
                continue;
            } else {
                dc_init(sc);
                break;
            }
        }

        // No errors; receive the packet.
        total_len -= ETHER_CRC_LEN as i32;

        let len = (total_len.max(0) as usize).min(m.m_len().get() as usize);
        // SAFETY: the cluster's data, `len` bytes of its `m_len`, which the chip wrote and
        // the sync above made visible.
        let frame = unsafe { core::slice::from_raw_parts(mtod::<u8>(m), len) };
        let m0 = m_devget(frame, ETHER_ALIGN as i32);
        let _ = dc_newbuf(sc, i, Some(m));
        i = dc_inc(i, DC_RX_LIST_CNT);
        let Some(m0) = m0 else {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            continue;
        };

        consumed += 1;
        ml_enqueue(&ml, m0);
    }

    cd.dc_rx_prod.set(i);

    if_input(ifp, &ml);

    consumed
}

/// `dc_txeof`: a frame was downloaded to the chip. It's safe for us to clean up the list
/// buffers.
pub fn dc_txeof(sc: &'static DcSoftc) {
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let dmat = sc.dmat();
    let cd = &sc.dc_cdata;

    // Go through our tx list and free mbufs for those frames that have been transmitted.
    let mut idx = cd.dc_tx_cons.get();
    while idx != cd.dc_tx_prod.get() {
        let offset = dc_tx_list_off(idx) as BusAddr;
        bus_dmamap_sync(
            dmat,
            sc.listmap(),
            offset,
            size_of::<DcDesc>() as BusSize,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        let mut txstat = sc.txd_read(idx, DcDescWord::Status);

        if txstat & DC_TXSTAT_OWN != 0 {
            break;
        }

        let ctl = sc.txd_read(idx, DcDescWord::Ctl);
        if ctl & DC_TXCTL_LASTFRAG == 0 || ctl & DC_TXCTL_SETUP != 0 {
            if ctl & DC_TXCTL_SETUP != 0 {
                // Yes, the PNIC is so brain damaged that it will sometimes generate a TX
                // underrun error while DMAing the RX filter setup frame. If we detect this,
                // we have to send the setup frame again, or else the filter won't be
                // programmed correctly.
                if sc.dc_is_pnic() && txstat & DC_TXSTAT_ERRSUM != 0 {
                    dc_setfilt(sc);
                }
                cd.tx(idx).clear_mbuf();
            }
            cd.dc_tx_cnt.set(cd.dc_tx_cnt.get() - 1);
            idx = dc_inc(idx, DC_TX_LIST_CNT);
            continue;
        }

        if sc.dc_is_xircom() || sc.dc_is_conexant() {
            // XXX: Why does my Xircom taunt me so? For some reason it likes setting the
            // CARRLOST flag even when the carrier is there. wtf?! Who knows, but Conexant
            // chips have the same problem. Maybe they took lessons from Xircom.
            if sc.dc_pmode.get() == DC_PMODE_MII
                && (txstat & 0xFFFF) & !(DC_TXSTAT_ERRSUM | DC_TXSTAT_NOCARRIER) != 0
            {
                txstat &= !DC_TXSTAT_ERRSUM;
            }
        } else if sc.dc_pmode.get() == DC_PMODE_MII
            && (txstat & 0xFFFF) & !(DC_TXSTAT_ERRSUM | DC_TXSTAT_NOCARRIER | DC_TXSTAT_CARRLOST)
                != 0
        {
            txstat &= !DC_TXSTAT_ERRSUM;
        }

        if txstat & DC_TXSTAT_ERRSUM != 0 {
            ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            if txstat & DC_TXSTAT_EXCESSCOLL != 0 {
                ifp.if_collisions().set(ifp.if_collisions().get() + 1);
            }
            if txstat & DC_TXSTAT_LATECOLL != 0 {
                ifp.if_collisions().set(ifp.if_collisions().get() + 1);
            }
            if txstat & DC_TXSTAT_UNDERRUN == 0 {
                dc_init(sc);
                return;
            }
        }

        ifp.if_collisions()
            .set(ifp.if_collisions().get() + u64::from((txstat & DC_TXSTAT_COLLCNT) >> 3));

        let sd = cd.tx(idx);
        let map = sd.map();
        if map.dm_nsegs.get() != 0 {
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
            bus_dmamap_unload(dmat, map);
        }
        if let Some(m) = sd.sd_mbuf.take() {
            m_freem(m);
        }
        sd.sd_setup.set(false);

        bus_dmamap_sync(
            dmat,
            sc.listmap(),
            offset,
            size_of::<DcDesc>() as BusSize,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        cd.dc_tx_cnt.set(cd.dc_tx_cnt.get() - 1);
        idx = dc_inc(idx, DC_TX_LIST_CNT);
    }
    cd.dc_tx_cons.set(idx);

    if DC_TX_LIST_CNT - cd.dc_tx_cnt.get() > 5 {
        ifq_clr_oactive(&ifp.if_snd);
    }
    if cd.dc_tx_cnt.get() == 0 {
        ifp.if_timer.set(0);
    }
}

/// `dc_tick`: the PHY's tick, and the link the transmit routine waits for.
pub fn dc_tick(xsc: *mut c_void) {
    let sc = dc_arg(xsc);
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let mii = &sc.sc_mii;

    let s = splnet();

    if sc.has_flags(DC_REDUCED_MII_POLL) {
        if sc.has_flags(DC_21143_NWAY) {
            let r = sc.csr_read_4(DC_10BTSTAT);
            if ifm_subtype(mii.mii_media_active.get()) == IFM_100_TX && r & DC_TSTAT_LS100 != 0 {
                sc.dc_link.set(0);
                let _ = mii_mediachg(mii);
            }
            if ifm_subtype(mii.mii_media_active.get()) == IFM_10_T && r & DC_TSTAT_LS10 != 0 {
                sc.dc_link.set(0);
                let _ = mii_mediachg(mii);
            }
            if sc.dc_link.get() == 0 {
                mii_tick(mii);
            }
        } else {
            // For NICs which never report DC_RXSTATE_WAIT, we have to bite the bullet...
            if (sc.dc_has_broken_rxstate()
                || sc.csr_read_4(DC_ISR) & DC_ISR_RX_STATE == DC_RXSTATE_WAIT)
                && sc.dc_cdata.dc_tx_cnt.get() == 0
                && !sc.dc_is_asix()
            {
                mii_tick(mii);
                if mii.mii_media_status.get() & IFM_ACTIVE == 0 {
                    sc.dc_link.set(0);
                }
            }
        }
    } else {
        mii_tick(mii);
    }

    // When the init routine completes, we expect to be able to send packets right away, and
    // in fact the network code will send a gratuitous ARP the moment the init routine marks
    // the interface as running. However, even though the MAC may have been initialized,
    // there may be a delay of a few seconds before the PHY completes autonegotiation and the
    // link is brought up. Any transmissions made during that delay will be lost. Dealing with
    // this is tricky: we can't just pause in the init routine while waiting for the PHY to
    // come ready since that would bring the whole system to a screeching halt for several
    // seconds.
    //
    // What we do here is prevent the TX start routine from sending any packets until a link
    // has been established. After the interface has been initialized, the tick routine will
    // poll the state of the PHY until the IFM_ACTIVE flag is set. Until that time, packets
    // will stay in the send queue, and once the link comes up, they will be flushed out to
    // the wire.
    if sc.dc_link.get() == 0
        && mii.mii_media_status.get() & IFM_ACTIVE != 0
        && ifm_subtype(mii.mii_media_active.get()) != IFM_NONE
    {
        sc.dc_link.set(sc.dc_link.get() + 1);
        if !ifq_empty(&ifp.if_snd) {
            dc_start(ifp);
        }
    }

    if sc.has_flags(DC_21143_NWAY) && sc.dc_link.get() == 0 {
        timeout_add_msec(&sc.dc_tick_tmo, 100);
    } else {
        timeout_add_sec(&sc.dc_tick_tmo, 1);
    }

    splx(s);
}

/// `dc_tx_underrun`: a transmit underrun has occurred. Back off the transmit threshold, or
/// switch to store and forward mode if we have to.
pub fn dc_tx_underrun(sc: &'static DcSoftc) {
    if sc.dc_is_davicom() {
        dc_init(sc);
    }

    if sc.dc_is_intel() {
        // The real 21143 requires that the transmitter be idle in order to change the
        // transmit threshold or store and forward state.
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_ON);

        let mut i = 0;
        while i < DC_TIMEOUT {
            let isr = sc.csr_read_4(DC_ISR);
            if isr & DC_ISR_TX_IDLE != 0 {
                break;
            }
            delay(10);
            i += 1;
        }
        if i == DC_TIMEOUT {
            printf(format_args!(
                "{}: failed to force tx to idle state\n",
                sc.devname()
            ));
            dc_init(sc);
        }
    }

    sc.dc_txthresh.set(sc.dc_txthresh.get() + DC_TXTHRESH_INC);
    if sc.dc_txthresh.get() > DC_TXTHRESH_MAX {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_STORENFWD);
    } else {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_THRESH);
        sc.dc_setbit(DC_NETCFG, sc.dc_txthresh.get());
    }

    if sc.dc_is_intel() {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_TX_ON);
    }
}

/// `dc_rxeof`, and `dc_rx_resync` with `dc_rxeof` again while it moves, when nothing came
/// (the receive half of `dc_intr`).
fn dc_rx_service(sc: &'static DcSoftc) {
    if dc_rxeof(sc) == 0 {
        while dc_rx_resync(sc) {
            let _ = dc_rxeof(sc);
        }
    }
}

/// `dc_intr`: the interrupt handler.
pub fn dc_intr(arg: *mut c_void) -> i32 {
    let sc = dc_arg(arg);
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let mut claimed = 0;

    let ints = sc.csr_read_4(DC_ISR);
    if ints & DC_INTRS == 0 {
        return claimed;
    }
    if ints == 0xffffffff {
        return 0;
    }

    // Suppress unwanted interrupts
    if ifp.if_flags.get() & IFF_UP == 0 {
        if sc.csr_read_4(DC_ISR) & DC_INTRS != 0 {
            dc_stop(sc, false);
        }
        return claimed;
    }

    // Disable interrupts.
    sc.csr_write_4(DC_IMR, 0x00000000);

    loop {
        let status = sc.csr_read_4(DC_ISR);
        if status & DC_INTRS == 0 || status == 0xFFFFFFFF || ifp.if_flags.get() & IFF_RUNNING == 0 {
            break;
        }

        claimed = 1;
        sc.csr_write_4(DC_ISR, status);

        if status & DC_ISR_RX_OK != 0 {
            dc_rx_service(sc);
        }

        if status & (DC_ISR_TX_OK | DC_ISR_TX_NOBUF) != 0 {
            dc_txeof(sc);
        }

        if status & DC_ISR_TX_IDLE != 0 {
            dc_txeof(sc);
            if sc.dc_cdata.dc_tx_cnt.get() != 0 {
                sc.dc_setbit(DC_NETCFG, DC_NETCFG_TX_ON);
                sc.csr_write_4(DC_TXSTART, 0xFFFFFFFF);
            }
        }

        if status & DC_ISR_TX_UNDERRUN != 0 {
            dc_tx_underrun(sc);
        }

        if status & DC_ISR_RX_WATDOGTIMEO != 0 || status & DC_ISR_RX_NOBUF != 0 {
            dc_rx_service(sc);
        }

        if status & DC_ISR_BUS_ERR != 0 {
            dc_init(sc);
        }
    }

    // Re-enable interrupts.
    sc.csr_write_4(DC_IMR, DC_INTRS);

    if !ifq_empty(&ifp.if_snd) {
        dc_start(ifp);
    }

    claimed
}

/// `dc_encap`: encapsulate an mbuf chain in a descriptor by coupling the mbuf data pointers
/// to the fragment pointers. `map` is loaded with `m`; `idx` moves past the descriptors
/// used.
pub fn dc_encap(
    sc: &DcSoftc,
    map: &'static BusDmamap,
    m: &'static Mbuf,
    idx: &mut usize,
) -> Result<(), Errno> {
    let cd = &sc.dc_cdata;
    let mut frag = *idx;
    let mut cur = frag;
    let mut cnt = 0;

    for seg in map.dm_segs().iter().take(map.dm_nsegs.get() as usize) {
        let seg = seg.get();
        let mut ctl = DC_TXCTL_TLINK | seg.ds_len as u32;
        if cnt == 0 {
            sc.txd_write(frag, DcDescWord::Status, 0);
            ctl |= DC_TXCTL_FIRSTFRAG;
        } else {
            sc.txd_write(frag, DcDescWord::Status, DC_TXSTAT_OWN);
        }
        sc.txd_write(frag, DcDescWord::Ctl, ctl);
        sc.txd_write(frag, DcDescWord::Data, seg.ds_addr as u32);
        cur = frag;
        frag = dc_inc(frag, DC_TX_LIST_CNT);
        cnt += 1;
    }

    cd.dc_tx_cnt.set(cd.dc_tx_cnt.get() + cnt);
    let sd = cd.tx(cur);
    sd.sd_mbuf.set(Some(m));
    sd.sd_setup.set(false);
    sc.sc_tx_sparemap.set(sd.sd_map.get());
    sd.sd_map.set(Some(map));
    let or_ctl = |i: usize, bits: u32| {
        sc.txd_write(i, DcDescWord::Ctl, sc.txd_read(i, DcDescWord::Ctl) | bits)
    };
    or_ctl(cur, DC_TXCTL_LASTFRAG);
    if sc.has_flags(DC_TX_INTR_FIRSTFRAG) {
        or_ctl(*idx, DC_TXCTL_FINT);
    }
    if sc.has_flags(DC_TX_INTR_ALWAYS) {
        or_ctl(cur, DC_TXCTL_FINT);
    }
    if sc.has_flags(DC_TX_USE_TX_INTR) && cd.dc_tx_cnt.get() > 64 {
        or_ctl(cur, DC_TXCTL_FINT);
    }
    bus_dmamap_sync(
        sc.dmat(),
        map,
        0,
        map.dm_mapsize.get(),
        BUS_DMASYNC_PREWRITE,
    );

    sc.txd_write(*idx, DcDescWord::Status, DC_TXSTAT_OWN);

    *idx = frag;

    Ok(())
}

/// `dc_fits`: whether a frame loaded in `map` fits in the transmit ring at `idx`.
fn dc_fits(sc: &DcSoftc, idx: usize, map: &BusDmamap) -> bool {
    let nsegs = map.dm_nsegs.get() as usize;

    if sc.has_flags(DC_TX_ADMTEK_WAR)
        && sc.dc_cdata.dc_tx_prod.get() != idx
        && idx + nsegs >= DC_TX_LIST_CNT
    {
        return false;
    }

    sc.dc_cdata.dc_tx_cnt.get() + nsegs + 5 <= DC_TX_LIST_CNT
}

/// `dc_start`: main transmit routine. To avoid having to do mbuf copies, we put pointers to
/// the mbuf data regions directly in the transmit lists. We also save a copy of the pointers
/// since the transmit list fragment pointers are physical addresses.
pub fn dc_start(ifp: &'static Ifnet) {
    let sc = dc_softc(ifp);
    let dmat = sc.dmat();

    if sc.dc_link.get() == 0 && ifq_len(&ifp.if_snd) < 10 {
        return;
    }

    if ifq_is_oactive(&ifp.if_snd) {
        return;
    }

    let mut idx = sc.dc_cdata.dc_tx_prod.get();

    let tx_list = (
        dc_tx_list_off(0) as BusAddr,
        (size_of::<DcDesc>() * DC_TX_LIST_CNT) as BusSize,
    );
    bus_dmamap_sync(
        dmat,
        sc.listmap(),
        tx_list.0,
        tx_list.1,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );

    while let Some(m) = ifq_deq_begin(&ifp.if_snd) {
        let map = sc.tx_sparemap();
        // SAFETY: the mbuf stays the slot's (`sd_mbuf`) until dc_txeof or dc_stop unload the
        // map and free it.
        let load = |m: &Mbuf| unsafe {
            bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_NOWAIT | BUS_DMA_OVERRUN)
        };
        let loaded = match load(m) {
            Ok(()) => true,
            Err(Errno::EFBIG) => m_defrag(m, M_DONTWAIT).is_ok() && load(m).is_ok(),
            Err(_) => false,
        };
        if !loaded {
            ifq_deq_commit(&ifp.if_snd, m);
            m_freem(m);
            ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            continue;
        }

        if !dc_fits(sc, idx, map) {
            bus_dmamap_unload(dmat, map);
            ifq_deq_rollback(&ifp.if_snd, m);
            ifq_set_oactive(&ifp.if_snd);
            break;
        }

        // now we are committed to transmit the packet
        ifq_deq_commit(&ifp.if_snd, m);

        if dc_encap(sc, map, m, &mut idx).is_err() {
            m_freem(m);
            ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            continue;
        }

        // If there's a BPF listener, bounce a copy of this frame to him.
        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap(bpf, m, BPF_DIRECTION_OUT);
        }

        if sc.has_flags(DC_TX_ONE) {
            ifq_set_oactive(&ifp.if_snd);
            break;
        }
    }

    bus_dmamap_sync(
        dmat,
        sc.listmap(),
        tx_list.0,
        tx_list.1,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    if idx == sc.dc_cdata.dc_tx_prod.get() {
        return;
    }

    // Transmit
    sc.dc_cdata.dc_tx_prod.set(idx);
    if !sc.has_flags(DC_TX_POLL) {
        sc.csr_write_4(DC_TXSTART, 0xFFFFFFFF);
    }

    // Set a timeout in case the chip goes out to lunch.
    ifp.if_timer.set(5);
}

/// `dc_init`: (re)start the chip: reset, bus and threshold setup, the rings, the filter, the
/// media, and the tick.
pub fn dc_init(sc: &'static DcSoftc) {
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let mii = &sc.sc_mii;

    let s = splnet();

    // Cancel pending I/O and free all RX/TX buffers.
    dc_stop(sc, false);
    dc_reset(sc);

    // Set cache alignment and burst length.
    if sc.dc_is_asix() || sc.dc_is_davicom() {
        sc.csr_write_4(DC_BUSCTL, 0);
    } else {
        sc.csr_write_4(DC_BUSCTL, DC_BUSCTL_MRME | DC_BUSCTL_MRLE);
    }
    // Evenly share the bus between receive and transmit process.
    if sc.dc_is_intel() {
        sc.dc_setbit(DC_BUSCTL, DC_BUSCTL_ARBITRATION);
    }
    if sc.dc_is_davicom() || sc.dc_is_intel() {
        sc.dc_setbit(DC_BUSCTL, DC_BURSTLEN_USECA);
    } else {
        sc.dc_setbit(DC_BUSCTL, DC_BURSTLEN_16LONG);
    }
    if sc.has_flags(DC_TX_POLL) {
        sc.dc_setbit(DC_BUSCTL, DC_TXPOLL_1);
    }
    match sc.dc_cachesize.get() {
        32 => sc.dc_setbit(DC_BUSCTL, DC_CACHEALIGN_32LONG),
        16 => sc.dc_setbit(DC_BUSCTL, DC_CACHEALIGN_16LONG),
        8 => sc.dc_setbit(DC_BUSCTL, DC_CACHEALIGN_8LONG),
        _ => sc.dc_setbit(DC_BUSCTL, DC_CACHEALIGN_NONE),
    }

    if sc.has_flags(DC_TX_STORENFWD) || sc.dc_txthresh.get() > DC_TXTHRESH_MAX {
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_STORENFWD);
    } else {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_STORENFWD);
        sc.dc_setbit(DC_NETCFG, sc.dc_txthresh.get());
    }

    sc.dc_setbit(DC_NETCFG, DC_NETCFG_NO_RXCRC);
    sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_BACKOFF);

    if sc.dc_is_macronix() || sc.dc_is_pnicii() {
        // The app notes for the 98713 and 98715A say that in order to have the chips operate
        // properly, a magic number must be written to CSR16. Macronix does not document the
        // meaning of these bits so there's no way to know exactly what they do. The 98713 has
        // a magic number all its own; the rest all use a different one.
        sc.dc_clrbit(DC_MX_MAGICPACKET, 0xFFFF0000);
        if sc.dc_type.get() == DC_TYPE_98713 {
            sc.dc_setbit(DC_MX_MAGICPACKET, DC_MX_MAGIC_98713);
        } else {
            sc.dc_setbit(DC_MX_MAGICPACKET, DC_MX_MAGIC_98715);
        }
    }

    if sc.dc_is_xircom() {
        dc_xircom_gpio(sc);
    }

    sc.dc_clrbit(DC_NETCFG, DC_NETCFG_TX_THRESH);
    sc.dc_setbit(DC_NETCFG, DC_TXTHRESH_MIN);

    // Init circular RX list.
    if dc_list_rx_init(sc) == Err(Errno::ENOBUFS) {
        printf(format_args!(
            "{}: initialization failed: no memory for rx buffers\n",
            sc.devname()
        ));
        dc_stop(sc, false);
        splx(s);
        return;
    }

    // Init tx descriptors.
    let _ = dc_list_tx_init(sc);

    // Sync down both lists initialized.
    let listmap = sc.listmap();
    bus_dmamap_sync(
        sc.dmat(),
        listmap,
        0,
        listmap.dm_mapsize.get(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    // Load the address of the RX list.
    let base = sc.list_addr();
    sc.csr_write_4(DC_RXADDR, base.wrapping_add(dc_rx_list_off(0) as u32));
    sc.csr_write_4(DC_TXADDR, base.wrapping_add(dc_tx_list_off(0) as u32));

    // Enable interrupts.
    sc.csr_write_4(DC_IMR, DC_INTRS);
    sc.csr_write_4(DC_ISR, 0xFFFFFFFF);

    // Enable transmitter.
    sc.dc_setbit(DC_NETCFG, DC_NETCFG_TX_ON);

    // If this is an Intel 21143 and we're not using the MII port, program the LED control
    // pins so we get link and activity indications.
    if sc.has_flags(DC_TULIP_LEDS) {
        sc.csr_write_4(
            DC_WATCHDOG,
            DC_WDOG_CTLWREN | DC_WDOG_LINK | DC_WDOG_ACTIVITY,
        );
        sc.csr_write_4(DC_WATCHDOG, 0);
    }

    // Load the RX/multicast filter. We do this sort of late because the filter programming
    // scheme on the 21143 and some clones requires DMAing a setup frame via the TX engine,
    // and we need the transmitter enabled for that.
    dc_setfilt(sc);

    // Enable receiver.
    sc.dc_setbit(DC_NETCFG, DC_NETCFG_RX_ON);
    sc.csr_write_4(DC_RXSTART, 0xFFFFFFFF);

    let _ = mii_mediachg(mii);
    dc_setcfg(sc, sc.dc_if_media.get());

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    splx(s);

    timeout_set(&sc.dc_tick_tmo, dc_tick, dc_ptr(sc));

    if ifm_subtype(mii.mii_media.ifm_media.get()) == IFM_HPNA_1 {
        sc.dc_link.set(1);
    } else if sc.has_flags(DC_21143_NWAY) {
        timeout_add_msec(&sc.dc_tick_tmo, 100);
    } else {
        timeout_add_sec(&sc.dc_tick_tmo, 1);
    }
}

/// `dc_ifmedia_upd`: set media options.
pub fn dc_ifmedia_upd(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = dc_softc(ifp);
    let mii = &sc.sc_mii;
    let _ = mii_mediachg(mii);

    let ifm = &mii.mii_media;

    if sc.dc_is_davicom() && ifm_subtype(ifm.ifm_media.get()) == IFM_HPNA_1 {
        dc_setcfg(sc, ifm.ifm_media.get());
    } else {
        sc.dc_link.set(0);
    }

    Ok(())
}

/// `dc_ifmedia_sts`: report current media status.
pub fn dc_ifmedia_sts(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = dc_softc(ifp);
    let mii = &sc.sc_mii;
    mii_pollstat(mii);
    let ifm = &mii.mii_media;
    if sc.dc_is_davicom() && ifm_subtype(ifm.ifm_media.get()) == IFM_HPNA_1 {
        ifmr.ifm_active = ifm.ifm_media.get();
        ifmr.ifm_status = 0;
        return;
    }
    ifmr.ifm_active = mii.mii_media_active.get();
    ifmr.ifm_status = mii.mii_media_status.get();
}

/// `dc_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn dc_ioctl(ifp: &'static Ifnet, command: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = dc_softc(ifp);
    let mut error = Ok(());

    let s = splnet();

    match command {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                dc_init(sc);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    sc.dc_txthresh.set(0);
                    dc_init(sc);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                dc_stop(sc, false);
            }
        }
        SIOCGIFMEDIA | SIOCSIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
            // ifmediareq` (this function's contract).
            error = unsafe { ifmedia_ioctl(ifp, data, &sc.sc_mii.mii_media, command) };
        }
        _ => {
            // SAFETY: this function's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_arpcom, command, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            dc_setfilt(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

/// `dc_watchdog`: the transmit timeout: count it, say so, and start again.
pub fn dc_watchdog(ifp: &'static Ifnet) {
    let sc = dc_softc(ifp);

    ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
    printf(format_args!("{}: watchdog timeout\n", sc.devname()));

    dc_init(sc);

    if !ifq_empty(&ifp.if_snd) {
        dc_start(ifp);
    }
}

/// `dc_stop`: stop the adapter and free any mbufs allocated to the RX and TX lists; with
/// `softonly`, the chip is left alone.
pub fn dc_stop(sc: &'static DcSoftc, softonly: bool) {
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let dmat = sc.dmat();
    let cd = &sc.dc_cdata;

    ifp.if_timer.set(0);

    timeout_del(&sc.dc_tick_tmo);

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    if !softonly {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_RX_ON | DC_NETCFG_TX_ON);

        let tx_idle =
            |isr: u32| isr & DC_ISR_TX_IDLE != 0 || isr & DC_ISR_TX_STATE == DC_TXSTATE_RESET;
        let rx_stopped = |isr: u32| isr & DC_ISR_RX_STATE == DC_RXSTATE_STOPPED;

        let mut isr = 0;
        let mut i = 0;
        while i < DC_TIMEOUT {
            isr = sc.csr_read_4(DC_ISR);
            if tx_idle(isr) && rx_stopped(isr) {
                break;
            }
            delay(10);
            i += 1;
        }

        if i == DC_TIMEOUT {
            if !tx_idle(isr) && !sc.dc_is_asix() && !sc.dc_is_davicom() {
                printf(format_args!(
                    "{}: failed to force tx to idle state\n",
                    sc.devname()
                ));
            }
            if !rx_stopped(isr) && !sc.dc_has_broken_rxstate() {
                printf(format_args!(
                    "{}: failed to force rx to idle state\n",
                    sc.devname()
                ));
            }
        }

        sc.csr_write_4(DC_IMR, 0x00000000);
        sc.csr_write_4(DC_TXADDR, 0x00000000);
        sc.csr_write_4(DC_RXADDR, 0x00000000);
        sc.dc_link.set(0);
    }

    // Free data in the RX lists.
    for sd in cd.dc_rx_chain.iter() {
        let map = sd.map();
        if map.dm_nsegs.get() != 0 {
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
            bus_dmamap_unload(dmat, map);
        }
        if let Some(m) = sd.sd_mbuf.take() {
            m_freem(m);
        }
        sd.sd_setup.set(false);
    }
    sc.rx_list_zero();

    // Free the TX list buffers.
    for (i, sd) in cd.dc_tx_chain.iter().enumerate() {
        let map = sd.map();
        if map.dm_nsegs.get() != 0 {
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
            bus_dmamap_unload(dmat, map);
        }
        if sd.has_mbuf() {
            if sc.txd_read(i, DcDescWord::Ctl) & DC_TXCTL_SETUP != 0 {
                sd.clear_mbuf();
                continue;
            }
            if let Some(m) = sd.sd_mbuf.take() {
                m_freem(m);
            }
            sd.clear_mbuf();
        }
    }
    sc.tx_list_zero();

    let listmap = sc.listmap();
    bus_dmamap_sync(
        dmat,
        listmap,
        0,
        listmap.dm_mapsize.get(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );
}

/// `dc_activate`: stop on suspend, start again on resume.
pub fn dc_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = dc_dev_sc(self_);
    let ifp = &sc.sc_arpcom.ac_if;

    match act {
        DVACT_SUSPEND if ifp.if_flags.get() & IFF_RUNNING != 0 => dc_stop(sc, false),
        DVACT_RESUME if ifp.if_flags.get() & IFF_UP != 0 => dc_init(sc),
        _ => {}
    }
    Ok(())
}

/// `dc_detach`: undo `dc_attach`.
pub fn dc_detach(sc: &'static DcSoftc) -> Result<(), Errno> {
    let ifp: &'static Ifnet = &sc.sc_arpcom.ac_if;
    let dmat = sc.dmat();

    dc_stop(sc, true);

    if sc.sc_mii.mii_phys.first().is_some() {
        mii_detach(&sc.sc_mii, MII_PHY_ANY, MII_OFFSET_ANY);
    }

    if let Some(p) = NonNull::new(sc.dc_srom.get()) {
        sc.dc_srom.set(ptr::null_mut());
        free(p, M_DEVBUF, sc.dc_sromsize.get());
    }

    let destroy = |map: Option<&'static BusDmamap>| {
        if let Some(map) = map {
            // SAFETY: a map dc_attach created, unloaded by dc_stop; its slot forgets it.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        }
    };
    for sd in sc.dc_cdata.dc_rx_chain.iter() {
        destroy(sd.sd_map.take());
    }
    destroy(sc.sc_rx_sparemap.take());
    for sd in sc.dc_cdata.dc_tx_chain.iter() {
        destroy(sd.sd_map.take());
    }
    destroy(sc.sc_tx_sparemap.take());

    // XXX bus_dmamap_sync
    let listmap = sc.listmap();
    bus_dmamap_unload(dmat, listmap);
    sc.dc_ldata.set(ptr::null_mut());
    if let Some(kva) = NonNull::new(sc.sc_listkva.take()) {
        // SAFETY: the list memory dc_attach mapped; its map is unloaded and the chip
        // stopped.
        unsafe { bus_dmamem_unmap(dmat, kva, size_of::<DcListData>()) };
    }
    sc.sc_listmap.set(None);
    // SAFETY: the list map dc_attach created, unloaded above.
    unsafe { bus_dmamap_destroy(dmat, NonNull::from(listmap)) };
    let segs = sc.sc_listseg.get();
    // SAFETY: the list memory's segment, unmapped above.
    unsafe { bus_dmamem_free(dmat, &segs[..sc.sc_listnseg.get()]) };

    ether_ifdetach(ifp);
    if_detach(ifp);
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_bits_by_table_width() {
        let crc = 0xDEAD_BEEF;
        assert_eq!(dc_crc_le_bits(crc, 0, false), crc & 0x1ff);
        assert_eq!(dc_crc_le_bits(crc, DC_128BIT_HASH, false), crc & 0x7f);
        assert_eq!(dc_crc_le_bits(crc, DC_64BIT_HASH, false), crc & 0x3f);
        // Xircom: bits 7 and 8 both set.
        assert_eq!(
            dc_crc_le_bits(0x1A5, 0, true),
            (0x1A5 & 0x0F) + (0x1A5 & 0x70) * 3 + (14 << 4)
        );
        assert_eq!(
            dc_crc_le_bits(0x0A5, 0, true),
            (0x0A5 & 0x1F) + ((0x0A5 >> 1) & 0xF0) * 3 + (12 << 4)
        );
    }

    #[test]
    fn broadcast_hash_lands_in_the_setup_frame() {
        // The 21143's hash bit for ff:ff:ff:ff:ff:ff, which every setup frame carries: a word
        // index below the perfect address at words 39..41.
        let h = dc_crc_le_bits(ether_crc32_le(&ETHERBROADCASTADDR), 0, false);
        assert_eq!(h, 255);
        assert!((h >> 4) < 39);
    }

    #[test]
    fn eeprom_words_in_host_order() {
        let w = 0x1234u16;
        assert_eq!(
            dc_eeprom_word_bytes(w, false),
            u16::from_le(w).to_ne_bytes()
        );
        assert_eq!(dc_eeprom_word_bytes(w, true), u16::from_be(w).to_ne_bytes());
        // On a little-endian host the station address comes out in EEPROM byte order.
        #[cfg(target_endian = "little")]
        assert_eq!(dc_eeprom_word_bytes(0x5452, false), [0x52, 0x54]);
    }

    #[test]
    fn srom_leaf_walk_and_mii_block() {
        // A 21143 SROM whose first leaf (at 30) has two blocks: an MII block (type 1) with a
        // GPIO sequence of 1 word and a reset sequence of 2 words, then a SYM block (type 4).
        let mut srom = [0u8; 128];
        srom[27] = 30;
        srom[30] = 0x00; // dc_conntype
        srom[31] = 0x08;
        srom[32] = 2; // dc_mcnt
        // block 0 at 33: len 0x80|9 (extended), type MII, phy 1, gpr_len 1, gpr, reset_len 2.
        srom[33] = 0x80 | 9;
        srom[34] = DC_EBLOCK_MII;
        srom[35] = 1;
        srom[36] = 1;
        srom[37] = 0xAA;
        srom[38] = 0xBB;
        srom[39] = 2;
        srom[40] = 0x01;
        srom[41] = 0x02;
        srom[42] = 0x03;
        srom[43] = 0x04;
        // block 1 at 33 + 9 + 1 = 43: type SYM.
        srom[43] = 0x80 | 7;
        srom[44] = DC_EBLOCK_SYM;

        let blocks: std::vec::Vec<_> = dc_srom_blocks(&srom).collect();
        assert_eq!(blocks, [(DC_EBLOCK_MII, 33), (DC_EBLOCK_SYM, 43)]);

        let mi = dc_leaf_mii_info(&srom, 33);
        assert_eq!(mi.dc_media, IFM_AUTO);
        assert_eq!((mi.dc_gp_ptr, mi.dc_gp_len), (37, 1));
        assert_eq!((mi.dc_reset_ptr, mi.dc_reset_len), (40, 2));
        let gp: std::vec::Vec<_> = dc_fixup_words(&srom, mi.dc_gp_ptr, mi.dc_gp_len).collect();
        assert_eq!(gp, [0xBBAA_0000]);
        // The reset sequence ends past block 1's header byte, as the C reads it.
        let rst: std::vec::Vec<_> =
            dc_fixup_words(&srom, mi.dc_reset_ptr, mi.dc_reset_len).collect();
        assert_eq!(rst, [0x0201_0000, (0x80 | 7) << 24 | 0x03 << 16]);
    }

    #[test]
    fn srom_reads_stop_at_its_end() {
        // A leaf offset past the SROM: no blocks; fixup words past the end are dropped.
        let mut srom = [0u8; 32];
        srom[27] = 200;
        assert_eq!(dc_srom_blocks(&srom).count(), 0);
        assert_eq!(dc_fixup_words(&srom, 30, 3).count(), 1);
    }

    #[test]
    fn sia_media_codes() {
        assert_eq!(dc_sia_media(DC_SIA_CODE_10BT), IFM_10_T);
        assert_eq!(
            dc_sia_media(DC_SIA_CODE_10BT_FDX | DC_SIA_CODE_EXT),
            IFM_10_T | IFM_FDX
        );
        assert_eq!(dc_sia_media(0x3f), 0);
    }

    #[test]
    fn pnic_salvage_finds_the_frame_end() {
        let mut buf = [0u8; 64];
        // A 10-byte frame ending at byte 23 (its CRC's last byte non-zero).
        buf[23] = 0x5a;
        // Scan from 32 back to 23; 23 is not aligned, so one back: 22; minus 10.
        assert_eq!(dc_pnic_salvage(&buf, 32, 10), 12);
        // Never before the start.
        assert_eq!(dc_pnic_salvage(&buf, 32, 40), 0);
        // All zeros: stops at the start.
        assert_eq!(dc_pnic_salvage(&[0u8; 8], 8, 4), 0);
    }
}
/* </TESTS> */
