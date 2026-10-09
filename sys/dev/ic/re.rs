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
 * Copyright (c) 1997, 1998-2003
 *	Bill Paul <wpaul@windriver.com>.  All rights reserved.
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
 */

/*
 * Copyright (c) 2005 Peter Valchev <pvalchev@openbsd.org>
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
//! re(4): the Realtek 8139C+/8169/8169S/8110S/8168/8101 driver, bus-independent part
//! (`dev/ic/re.c`, with the prototypes of `<dev/ic/revar.h>`). `if_re_pci.rs` attaches it.
//!
//! Upstream: sys/dev/ic/re.c @ 3ce1f3f79392, sys/dev/ic/revar.h @ 3ce1f3f79392
//!
//! The driver uses the C+ descriptor rings of these chips: one transmit and one receive ring
//! of `struct rl_desc` in `bus_dma(9)` memory, checksum offload, simulated (timer based)
//! interrupt moderation, the EEPROM or the ID registers for the station address, and the
//! mii(4) layer for the PHY (on the 8139C+ the PHY registers are MAC registers that
//! `re_miibus_readreg` maps; the gigabit chips go through `RL_PHYAR`). QEMU's `rtl8139` is
//! an 8139C+ (`RL_HWREV_8139CPLUS`).
//!
//! ## Deviations
//! - Locking as in C, made explicit: `re_start` runs on whatever CPU serves the send queue,
//!   `re_intr` is established `IPL_MPSAFE` (it takes the kernel lock only around `re_init`),
//!   the ioctls, the tick and `re_init`/`re_stop` run under the kernel lock. `rl_flags` and
//!   the transmit producer/consumer indices are atomics (`rtl81x9reg.rs`); each
//!   transmit slot belongs to `re_start` or `re_txeof` by those indices, as in C; the receive
//!   ring is the interrupt's, and `re_init`/`re_stop`'s with the interrupt barred.
//! - The descriptor rings are DMA memory reached through raw pointers with bounds-checked
//!   accessors; every access reads or writes a whole descriptor, volatile (`|=` on a
//!   descriptor is a read, a change and a write).
//! - `kstat(4)` is not configured (`NKSTAT` 0): `re_kstat_attach`, `re_kstat_detach`,
//!   `re_kstat_read`, `re_kstat_copy`, `struct re_kstats`, `re_kstats_tpl`,
//!   `struct re_kstat_softc` and the `RE_DTCCR_*` registers are compiled out, as in such a C
//!   kernel; comments mark the call sites.
//! - `vlan(4)` is not configured: the `#if NVLAN > 0` blocks are behind the constant
//!   [`NVLAN`] (0), so `IFCAP_VLAN_HWTAGGING` is not offered and tags are neither inserted
//!   nor taken from the descriptors. `SMALL_KERNEL` is not defined (`re_wol` is here).
//! - `RE_DEBUG` is not defined: `redebug` and `DPRINTF` are left out.
//! - `re_encap`'s software checksum for oversized frames on `RL_FLAG_JUMBOV2` chips builds no
//!   stack mbuf: `re_jumbo_cksum` sums the IP header and the payload at their offsets in the
//!   chain (`in4_cksum`) and writes them back with `m_copyback`, what `in_cksum` and
//!   `in_delayed_cksum` do on the C's `mh`.
//! - `re_read_eeprom` fills a `&mut [u16]` (the C's `caddr_t` of words);
//!   `re_eeprom_getword` returns the word.
//! - `re_init`'s ID register writes read the address through an 8-byte zeroed copy (the
//!   C's union reads two bytes past the address).
//! - `re_init` acknowledges `rl_intrs` in `RL_ISR` before `re_setup_intr`, where the C does
//!   it after. `re_setup_intr` starts the simulated moderation timer (`RL_TIMERINT` 0x400,
//!   about 31 us at 33 MHz), whose one expiry is the only interrupt `RL_INTRS_TIMER`
//!   enables; when it expires before the C's acknowledgement (QEMU's 8139C+ runs it in
//!   virtual time, and on arm64 one boot in three did), the acknowledgement clears it, no
//!   interrupt comes and transmissions wait for the watchdog. Acknowledging first keeps the
//!   C's intent (no stale status) without the race. It is harmless: a status bit pending
//!   from before `re_init` carries no work, since `re_init` has just stopped the chip and
//!   rebuilt both rings, and a bit set between the acknowledgement and `re_setup_intr` is
//!   new work that now raises its interrupt where the C would have cleared it. The loss is
//!   the device's, not the kernel's interrupt path: re(4) is on INTx (level) on both archs,
//!   established at attach long before `re_init`, which runs at `splnet`; clearing the bit
//!   deasserts the line, so no interrupt is left for any controller to deliver.
//! - `re_newbuf` unloads the cluster's map before freeing it when the descriptor is still
//!   the chip's (the C frees the mbuf and leaves the map loaded).
//! - The revision switch of `re_attach`, with its fall-throughs, is `re_hwrev_flags`, which
//!   returns the flags and the maximum MTU of a chip.
//! - Functions returning 0 or an errno return `Result<(), Errno>`; `re_attach` too (the C's
//!   1 is an `Err`); `re_rxeof`/`re_txeof` return `bool`, `re_encap` the slots it used.
//! - `re_intr`, `re_tick` and `re_txstart` take the softc as the C's `void *`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::ic::rtl81x9reg::*;
use crate::dev::mii::mii::{
    BMCR_ISO, BMCR_LOOP, MII_ANAR, MII_ANER, MII_ANLPAR, MII_BMCR, MII_BMSR, MII_PHYIDR1,
    MII_PHYIDR2,
};
use crate::dev::mii::mii::{
    mii_attach, mii_detach, mii_down, mii_mediachg, mii_pollstat, mii_tick,
};
use crate::dev::mii::miivar::{MII_OFFSET_ANY, MII_PHY_ANY, MIIF_DOPAUSE};
use crate::dev::pci::pcidevs::PCI_PRODUCT_REALTEK_RT8101E;
use crate::kassert;
use crate::kern::kern_sysctl::{hw_prod, hw_vendor};
use crate::kern::kern_task::task_set;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_clget, m_copyback, m_copydata, m_defrag, m_freem, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BusAddr, BusDmaSegment, BusDmamap, BusSize, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load, bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc,
    bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{intr_barrier, splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap_ether};
use crate::net::if_::{
    IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_UDPv4, IFCAP_VLAN_HWTAGGING, IFCAP_VLAN_MTU,
    IFCAP_WOL, IFF_ALLMULTI, IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING, IFF_SIMPLEX,
    IFF_UP, IFNAMSIZ, IFXF_MBUF_64BIT, IFXF_MPSAFE, Ifmediareq, Ifreq, if_attach, if_detach,
    if_rxr_get, if_rxr_init, if_rxr_ioctl, if_rxr_livelocked,
};
use crate::net::if_ethersubr::{
    ether_crc32_be, ether_ifattach, ether_ifdetach, ether_ioctl, ether_sprintf,
};
use crate::net::if_media::{
    IFM_10_T, IFM_100_TX, IFM_1000_T, IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETHER, IFM_IMASK,
    IFM_INST_ANY, IFM_NONE, ifm_subtype, ifmedia_add, ifmedia_delete_instance, ifmedia_init,
    ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::{Ifnet, if_rxr_inuse, if_rxr_put};
use crate::net::ifq::{
    Ifqueue, ifiq_input, ifq_barrier, ifq_clr_oactive, ifq_dequeue, ifq_init_maxlen,
    ifq_is_oactive, ifq_purge, ifq_restart, ifq_serialize, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    ETHER_ADDR_LEN, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_VLAN_ENCAP_LEN, EtherMultistep,
    ether_first_multi, ether_next_multi,
};
use crate::netinet::in_::{IPPROTO_ICMP, IPPROTO_TCP, IPPROTO_UDP};
use crate::netinet::in4_cksum::in4_cksum;
use crate::sys::device::{Cfdriver, DV_IFNET, Device};
use crate::sys::endian::swap32;
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_DONTWAIT, M_IPV4_CSUM_IN_OK, M_IPV4_CSUM_OUT, M_PKTHDR, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT,
    M_UDP_CSUM_IN_OK, M_UDP_CSUM_OUT, M_VLANTAG, Mbuf, MbufList,
};
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCGIFRXR, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA};
use crate::sys::systm::{kernel_lock, kernel_unlock};

/// `NVLAN`: `vlan(4)` is not configured in this kernel.
pub const NVLAN: i32 = 0;

/// `sizeof(struct ip)`.
const IP_HDR_LEN: usize = 20;
/// `offsetof(struct ip, ip_sum)`.
const IP_SUM_OFFSET: usize = 10;
/// `offsetof(struct tcphdr, th_sum)` (`netinet/tcp.h`, not ported).
const TCPHDR_TH_SUM: usize = 16;
/// `offsetof(struct udphdr, uh_sum)` (`netinet/udp.h`, not ported).
const UDPHDR_UH_SUM: usize = 6;
/// `offsetof(struct icmp, icmp_cksum)`.
const ICMP_CKSUM_OFFSET: usize = 2;

/// `struct re_revision`: a chip id and its name.
pub struct ReRevision {
    /// `re_chipid`.
    pub re_chipid: u32,
    /// `re_name`.
    pub re_name: &'static str,
}

/// `re_cd`.
pub static RE_CD: Cfdriver = Cfdriver::new(b"re", DV_IFNET, 0);

/// `re_revisions[]` (without the `{ 0, NULL }` terminator).
pub static RE_REVISIONS: [ReRevision; 40] = [
    rev(RL_HWREV_8100, "RTL8100"),
    rev(RL_HWREV_8100E, "RTL8100E"),
    rev(RL_HWREV_8100E_SPIN2, "RTL8100E 2"),
    rev(RL_HWREV_8101, "RTL8101"),
    rev(RL_HWREV_8101E, "RTL8101E"),
    rev(RL_HWREV_8102E, "RTL8102E"),
    rev(RL_HWREV_8106E, "RTL8106E"),
    rev(RL_HWREV_8401E, "RTL8401E"),
    rev(RL_HWREV_8402, "RTL8402"),
    rev(RL_HWREV_8411, "RTL8411"),
    rev(RL_HWREV_8411B, "RTL8411B"),
    rev(RL_HWREV_8102EL, "RTL8102EL"),
    rev(RL_HWREV_8102EL_SPIN1, "RTL8102EL 1"),
    rev(RL_HWREV_8103E, "RTL8103E"),
    rev(RL_HWREV_8110S, "RTL8110S"),
    rev(RL_HWREV_8139CPLUS, "RTL8139C+"),
    rev(RL_HWREV_8168B_SPIN1, "RTL8168 1"),
    rev(RL_HWREV_8168B_SPIN2, "RTL8168 2"),
    rev(RL_HWREV_8168B_SPIN3, "RTL8168 3"),
    rev(RL_HWREV_8168C, "RTL8168C/8111C"),
    rev(RL_HWREV_8168C_SPIN2, "RTL8168C/8111C"),
    rev(RL_HWREV_8168CP, "RTL8168CP/8111CP"),
    rev(RL_HWREV_8168F, "RTL8168F/8111F"),
    rev(RL_HWREV_8168G, "RTL8168G/8111G"),
    rev(RL_HWREV_8168GU, "RTL8168GU/8111GU"),
    rev(RL_HWREV_8168H, "RTL8168H/8111H"),
    rev(RL_HWREV_8105E, "RTL8105E"),
    rev(RL_HWREV_8105E_SPIN1, "RTL8105E"),
    rev(RL_HWREV_8168D, "RTL8168D/8111D"),
    rev(RL_HWREV_8168DP, "RTL8168DP/8111DP"),
    rev(RL_HWREV_8168E, "RTL8168E/8111E"),
    rev(RL_HWREV_8168E_VL, "RTL8168E/8111E-VL"),
    rev(RL_HWREV_8168EP, "RTL8168EP/8111EP"),
    rev(RL_HWREV_8168FP, "RTL8168FP/8111FP"),
    rev(RL_HWREV_8169, "RTL8169"),
    rev(RL_HWREV_8169_8110SB, "RTL8169/8110SB"),
    rev(RL_HWREV_8169_8110SBL, "RTL8169SBL"),
    rev(RL_HWREV_8169_8110SCd, "RTL8169/8110SCd"),
    rev(RL_HWREV_8169_8110SCe, "RTL8169/8110SCe"),
    rev(RL_HWREV_8169S, "RTL8169S"),
];

/// `{ chipid, name }` of `re_revisions[]`.
const fn rev(re_chipid: u32, re_name: &'static str) -> ReRevision {
    ReRevision { re_chipid, re_name }
}

/// `(struct rl_softc *)ifp->if_softc`.
pub fn re_softc(ifp: &Ifnet) -> &'static RlSoftc {
    let p = ifp.if_softc.get().cast::<RlSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("re: interface without its softc"));
    }
    // SAFETY: re_attach sets `if_softc` to its softc and installs re's functions only on its
    // own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// The softc behind the C's `void *` argument (interrupt, timeout, task).
fn re_arg(arg: *mut c_void) -> &'static RlSoftc {
    if arg.is_null() {
        panic(format_args!("re: no softc argument"));
    }
    // SAFETY: re_pci_attach and re_attach pass the softc, which is never freed while the
    // device exists.
    unsafe { &*arg.cast::<RlSoftc>() }
}

/// The softc of the device the mii(4) callbacks name (`(struct rl_softc *)dev`).
fn re_dev_sc(dev: &Device) -> &'static RlSoftc {
    // SAFETY: mii(4) calls back with the device that attached it, re's; its softc starts
    // with a `struct rl_softc` (`re_pci_softc`), never freed while the device exists.
    unsafe { &*ptr::from_ref(dev.softc::<RlSoftc>()) }
}

/// `&sc->sc_arpcom.ac_if`.
fn re_ifp(sc: &'static RlSoftc) -> &'static Ifnet {
    &sc.sc_arpcom.ac_if
}

/// `EE_SET(x)`.
fn ee_set(sc: &RlSoftc, x: u32) {
    sc.csr_write_1(RL_EECMD, sc.csr_read_1(RL_EECMD) | x);
}

/// `EE_CLR(x)`.
fn ee_clr(sc: &RlSoftc, x: u32) {
    sc.csr_write_1(RL_EECMD, sc.csr_read_1(RL_EECMD) & !x);
}

/// `RL_FRAMELEN(mtu)`.
pub const fn rl_framelen(mtu: i32) -> i32 {
    mtu + ETHER_HDR_LEN as i32 + ETHER_CRC_LEN as i32 + ETHER_VLAN_ENCAP_LEN as i32
}

/// `re_set_bufaddr`: a descriptor's buffer address, little-endian, split in two words.
#[inline]
fn re_set_bufaddr(d: &mut RlDesc, addr: BusAddr) {
    d.rl_bufaddr_lo = (addr as u32).to_le();
    d.rl_bufaddr_hi = if size_of::<BusAddr>() == size_of::<u64>() {
        ((addr as u64 >> 32) as u32).to_le()
    } else {
        0
    };
}

/// `re_eeprom_putbyte`: send a read command and address to the EEPROM, check for ACK.
pub fn re_eeprom_putbyte(sc: &RlSoftc, addr: i32) {
    let d = addr | (RL_9346_READ as i32) << sc.rl_eewidth.get();

    // Feed in each bit and strobe the clock.
    let mut i = 1i32 << (sc.rl_eewidth.get() + 3);
    while i != 0 {
        if d & i != 0 {
            ee_set(sc, RL_EE_DATAIN);
        } else {
            ee_clr(sc, RL_EE_DATAIN);
        }
        delay(100);
        ee_set(sc, RL_EE_CLK);
        delay(150);
        ee_clr(sc, RL_EE_CLK);
        delay(100);
        i >>= 1;
    }
}

/// `re_eeprom_getword`: read a word of data stored in the EEPROM at address `addr`.
pub fn re_eeprom_getword(sc: &RlSoftc, addr: i32) -> u16 {
    let mut word: u16 = 0;

    // Send address of word we want to read.
    re_eeprom_putbyte(sc, addr);

    // Start reading bits from EEPROM.
    let mut i: u16 = 0x8000;
    while i != 0 {
        ee_set(sc, RL_EE_CLK);
        delay(100);
        if sc.csr_read_1(RL_EECMD) & RL_EE_DATAOUT != 0 {
            word |= i;
        }
        ee_clr(sc, RL_EE_CLK);
        delay(100);
        i >>= 1;
    }

    word
}

/// `re_read_eeprom`: read a sequence of words from the EEPROM.
pub fn re_read_eeprom(sc: &RlSoftc, dest: &mut [u16], off: i32) {
    sc.csr_setbit_1(RL_EECMD, RL_EEMODE_PROGRAM);

    delay(100);

    for (i, w) in dest.iter_mut().enumerate() {
        sc.csr_setbit_1(RL_EECMD, RL_EE_SEL);
        *w = re_eeprom_getword(sc, off + i as i32);
        sc.csr_clrbit_1(RL_EECMD, RL_EE_SEL);
    }

    sc.csr_clrbit_1(RL_EECMD, RL_EEMODE_PROGRAM);
}

/// `re_gmii_readreg`: a PHY register of the gigabit chips, through `RL_PHYAR`.
pub fn re_gmii_readreg(self_: &Device, phy: i32, reg: i32) -> i32 {
    let sc = re_dev_sc(self_);

    if phy != 7 {
        return 0;
    }

    // Let the rgephy driver read the GMEDIASTAT register

    if reg == RL_GMEDIASTAT as i32 {
        return sc.csr_read_1(RL_GMEDIASTAT) as i32;
    }

    sc.csr_write_4(RL_PHYAR, (reg as u32) << 16);

    let mut rval = 0;
    let mut i = 0;
    while i < RL_PHY_TIMEOUT {
        rval = sc.csr_read_4(RL_PHYAR);
        if rval & RL_PHYAR_BUSY != 0 {
            break;
        }
        delay(25);
        i += 1;
    }

    if i == RL_PHY_TIMEOUT {
        printf(format_args!("{}: PHY read failed\n", sc.devname()));
        return 0;
    }

    delay(20);

    (rval & RL_PHYAR_PHYDATA) as i32
}

/// `re_gmii_writereg`.
pub fn re_gmii_writereg(dev: &Device, _phy: i32, reg: i32, data: i32) {
    let sc = re_dev_sc(dev);

    sc.csr_write_4(
        RL_PHYAR,
        ((reg as u32) << 16) | (data as u32 & RL_PHYAR_PHYDATA) | RL_PHYAR_BUSY,
    );

    let mut i = 0;
    while i < RL_PHY_TIMEOUT {
        let rval = sc.csr_read_4(RL_PHYAR);
        if rval & RL_PHYAR_BUSY == 0 {
            break;
        }
        delay(25);
        i += 1;
    }

    if i == RL_PHY_TIMEOUT {
        printf(format_args!("{}: PHY write failed\n", sc.devname()));
    }

    delay(20);
}

/// The 8139C+ MAC register that stands for MII register `reg`, if any.
fn re_8139_reg(reg: i32) -> Option<u32> {
    match reg {
        MII_BMCR => Some(RL_BMCR),
        MII_BMSR => Some(RL_BMSR),
        MII_ANAR => Some(RL_ANAR),
        MII_ANER => Some(RL_ANER),
        MII_ANLPAR => Some(RL_LPAR),
        _ => None,
    }
}

/// `re_miibus_readreg`: mii(4)'s `mii_readreg`.
pub fn re_miibus_readreg(dev: &Device, phy: i32, reg: i32) -> i32 {
    let sc = re_dev_sc(dev);
    let s = splnet();

    if sc.sc_hwrev.get() != RL_HWREV_8139CPLUS {
        let rval = re_gmii_readreg(dev, phy, reg) as u16;
        splx(s);
        return i32::from(rval);
    }

    // Pretend the internal PHY is only at address 0
    if phy != 0 {
        splx(s);
        return 0;
    }
    let re8139_reg = match re_8139_reg(reg) {
        Some(r) => r,
        None if reg == MII_PHYIDR1 || reg == MII_PHYIDR2 => {
            splx(s);
            return 0;
        }
        // Allow the rlphy driver to read the media status register. If we have a link
        // partner which does not support NWAY, this is the register which will tell us the
        // results of parallel detection.
        None if reg == RL_MEDIASTAT as i32 => {
            let rval = sc.csr_read_1(RL_MEDIASTAT) as u16;
            splx(s);
            return i32::from(rval);
        }
        None => {
            printf(format_args!(
                "{}: bad phy register {:x}\n",
                sc.devname(),
                reg
            ));
            splx(s);
            return 0;
        }
    };
    let mut rval = sc.csr_read_2(re8139_reg) as u16;
    if re8139_reg == RL_BMCR {
        // 8139C+ has different bit layout.
        rval &= !((BMCR_LOOP | BMCR_ISO) as u16);
    }
    splx(s);
    i32::from(rval)
}

/// `re_miibus_writereg`: mii(4)'s `mii_writereg`.
pub fn re_miibus_writereg(dev: &Device, phy: i32, reg: i32, mut data: i32) {
    let sc = re_dev_sc(dev);
    let s = splnet();

    if sc.sc_hwrev.get() != RL_HWREV_8139CPLUS {
        re_gmii_writereg(dev, phy, reg, data);
        splx(s);
        return;
    }

    // Pretend the internal PHY is only at address 0
    if phy != 0 {
        splx(s);
        return;
    }
    let re8139_reg = match re_8139_reg(reg) {
        Some(r) => {
            if r == RL_BMCR {
                // 8139C+ has different bit layout.
                data &= !(BMCR_LOOP | BMCR_ISO);
            }
            r
        }
        None if reg == MII_PHYIDR1 || reg == MII_PHYIDR2 => {
            splx(s);
            return;
        }
        None => {
            printf(format_args!(
                "{}: bad phy register {:x}\n",
                sc.devname(),
                reg
            ));
            splx(s);
            return;
        }
    };
    sc.csr_write_2(re8139_reg, data as u32);
    splx(s);
}

/// `re_miibus_statchg`: mii(4)'s `mii_statchg`: whether the link is up at a speed the MAC
/// does (`RL_FLAG_LINK`).
pub fn re_miibus_statchg(dev: &Device) {
    let sc = re_dev_sc(dev);
    let ifp = re_ifp(sc);
    let mii = &sc.sc_mii;

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        return;
    }

    sc.clr_flags(RL_FLAG_LINK);
    if (mii.mii_media_status.get() & (IFM_ACTIVE | IFM_AVALID)) == (IFM_ACTIVE | IFM_AVALID) {
        match ifm_subtype(mii.mii_media_active.get()) {
            IFM_10_T | IFM_100_TX => sc.set_flags(RL_FLAG_LINK),
            IFM_1000_T if sc.flags() & RL_FLAG_FASTETHER == 0 => sc.set_flags(RL_FLAG_LINK),
            _ => {}
        }
    }

    // Realtek controllers do not provide an interface to Tx/Rx MACs for resolved speed,
    // duplex and flow-control parameters.
}

/// `re_iff`: the receive filter: our address, broadcast, the multicast hash or all
/// multicast, promiscuous.
pub fn re_iff(sc: &'static RlSoftc) {
    let ifp = re_ifp(sc);
    let ac = &sc.sc_arpcom;
    let mut hashes = [0u32; 2];

    let mut rxfilt = sc.csr_read_4(RL_RXCFG);
    rxfilt &= !(RL_RXCFG_RX_ALLPHYS | RL_RXCFG_RX_BROAD | RL_RXCFG_RX_INDIV | RL_RXCFG_RX_MULTI);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    // Always accept frames destined to our station address. Always accept broadcast frames.
    rxfilt |= RL_RXCFG_RX_INDIV | RL_RXCFG_RX_BROAD;

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        rxfilt |= RL_RXCFG_RX_MULTI;
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            rxfilt |= RL_RXCFG_RX_ALLPHYS;
        }
        hashes = [0xFFFF_FFFF; 2];
    } else {
        rxfilt |= RL_RXCFG_RX_MULTI;
        // Program new filter.
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let h = ether_crc32_be(&e.enm_addrlo[..ETHER_ADDR_LEN]) >> 26;

            if h < 32 {
                hashes[0] |= 1 << h;
            } else {
                hashes[1] |= 1 << (h - 32);
            }

            enm = ether_next_multi(&mut step);
        }
    }

    // For some unfathomable reason, Realtek decided to reverse the order of the multicast
    // hash registers in the PCI Express parts. This means we have to write the hash pattern
    // in reverse order for those devices.
    if sc.flags() & RL_FLAG_PCIE != 0 {
        sc.csr_write_4(RL_MAR0, swap32(hashes[1]));
        sc.csr_write_4(RL_MAR4, swap32(hashes[0]));
    } else {
        sc.csr_write_4(RL_MAR0, hashes[0]);
        sc.csr_write_4(RL_MAR4, hashes[1]);
    }

    sc.csr_write_4(RL_RXCFG, rxfilt);
}

/// `re_reset`.
pub fn re_reset(sc: &RlSoftc) {
    sc.csr_write_1(RL_COMMAND, RL_CMD_RESET);

    let mut i = 0;
    while i < RL_TIMEOUT {
        delay(10);
        if sc.csr_read_1(RL_COMMAND) & RL_CMD_RESET == 0 {
            break;
        }
        i += 1;
    }
    if i == RL_TIMEOUT {
        printf(format_args!("{}: reset never completed!\n", sc.devname()));
    }

    if sc.flags() & RL_FLAG_MACRESET != 0 {
        sc.csr_write_1(RL_LDPS, 1);
    }
}

/// The chip flags and maximum MTU of `re_attach`'s revision switch.
#[allow(non_upper_case_globals)] // RL_HWREV_8169_8110SCd/SCe, the C's names
fn re_hwrev_flags(hwrev: u32, product: u16) -> (u32, Option<u32>) {
    let mut f = 0;
    let mtu = match hwrev {
        RL_HWREV_8139CPLUS => {
            f |= RL_FLAG_FASTETHER | RL_FLAG_AUTOPAD;
            Some(RL_MTU)
        }
        RL_HWREV_8100E | RL_HWREV_8100E_SPIN2 | RL_HWREV_8101E => {
            f |= RL_FLAG_PHYWAKE | RL_FLAG_FASTETHER;
            Some(RL_MTU)
        }
        RL_HWREV_8103E | RL_HWREV_8102E | RL_HWREV_8102EL | RL_HWREV_8102EL_SPIN1 => {
            if hwrev == RL_HWREV_8103E {
                f |= RL_FLAG_MACSLEEP;
            }
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_FASTETHER
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD;
            Some(RL_MTU)
        }
        RL_HWREV_8401E | RL_HWREV_8105E | RL_HWREV_8105E_SPIN1 | RL_HWREV_8106E => {
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PHYWAKE_PM
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_FASTETHER
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD;
            Some(RL_MTU)
        }
        RL_HWREV_8402 => {
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PHYWAKE_PM
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_FASTETHER
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_CMDSTOP_WAIT_TXQ;
            Some(RL_MTU)
        }
        RL_HWREV_8168B_SPIN1 | RL_HWREV_8168B_SPIN2 | RL_HWREV_8168B_SPIN3 => {
            if hwrev != RL_HWREV_8168B_SPIN3 {
                f |= RL_FLAG_WOLRXENB;
            }
            f |= RL_FLAG_PHYWAKE | RL_FLAG_MACSTAT;
            Some(RL_MTU)
        }
        RL_HWREV_8168C_SPIN2 | RL_HWREV_8168C | RL_HWREV_8168CP => {
            if hwrev == RL_HWREV_8168C_SPIN2 {
                f |= RL_FLAG_MACSLEEP;
            }
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_6K)
        }
        RL_HWREV_8168D => {
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PHYWAKE_PM
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_9K)
        }
        RL_HWREV_8168DP => {
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_WAIT_TXPOLL
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_9K)
        }
        RL_HWREV_8168E => {
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PHYWAKE_PM
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_9K)
        }
        RL_HWREV_8168E_VL => {
            f |= RL_FLAG_EARLYOFF
                | RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_CMDSTOP_WAIT_TXQ
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_6K)
        }
        RL_HWREV_8168F | RL_HWREV_8411 => {
            if hwrev == RL_HWREV_8168F {
                f |= RL_FLAG_EARLYOFF;
            }
            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_JUMBOV2
                | RL_FLAG_CMDSTOP_WAIT_TXQ
                | RL_FLAG_WOL_MANLINK;
            Some(RL_JUMBO_MTU_9K)
        }
        RL_HWREV_8168EP | RL_HWREV_8168FP | RL_HWREV_8168G | RL_HWREV_8168GU | RL_HWREV_8168H
        | RL_HWREV_8411B => {
            let mtu = if u32::from(product) == PCI_PRODUCT_REALTEK_RT8101E {
                // RTL8106EUS
                f |= RL_FLAG_FASTETHER;
                RL_MTU
            } else {
                f |= RL_FLAG_JUMBOV2 | RL_FLAG_WOL_MANLINK;
                RL_JUMBO_MTU_9K
            };

            f |= RL_FLAG_PHYWAKE
                | RL_FLAG_PAR
                | RL_FLAG_DESCV2
                | RL_FLAG_MACSTAT
                | RL_FLAG_CMDSTOP
                | RL_FLAG_AUTOPAD
                | RL_FLAG_CMDSTOP_WAIT_TXQ
                | RL_FLAG_EARLYOFFV2
                | RL_FLAG_RXDV_GATED;
            Some(mtu)
        }
        RL_HWREV_8169_8110SB
        | RL_HWREV_8169_8110SBL
        | RL_HWREV_8169_8110SCd
        | RL_HWREV_8169_8110SCe
        | RL_HWREV_8169
        | RL_HWREV_8169S
        | RL_HWREV_8110S => {
            if matches!(
                hwrev,
                RL_HWREV_8169_8110SB
                    | RL_HWREV_8169_8110SBL
                    | RL_HWREV_8169_8110SCd
                    | RL_HWREV_8169_8110SCe
            ) {
                f |= RL_FLAG_PHYWAKE;
            }
            f |= RL_FLAG_MACRESET;
            Some(RL_JUMBO_MTU_7K)
        }
        _ => None,
    };
    (f, mtu)
}

/// `re_attach`: attach the interface. Allocate softc structures, do ifmedia setup and
/// ethernet/BPF attach. `Err` is the C's 1: the bus front-end then releases what it set up.
pub fn re_attach(sc: &'static RlSoftc, intrstr: &[u8]) -> Result<(), Errno> {
    let bus_dma64 = if sc.flags() & RL_FLAG_PCIE != 0 {
        BUS_DMA_64BIT
    } else {
        0
    };
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();

    sc.sc_hwrev.set(sc.csr_read_4(RL_TXCFG) & RL_TXCFG_HWREV);

    let (flags, max_mtu) = re_hwrev_flags(sc.sc_hwrev.get(), sc.sc_product.get());
    sc.set_flags(flags);
    if let Some(mtu) = max_mtu {
        sc.rl_max_mtu.set(mtu as i32);
    }

    if sc.sc_hwrev.get() == RL_HWREV_8139CPLUS {
        sc.rl_cfg0.set(RL_8139_CFG0);
        sc.rl_cfg1.set(RL_8139_CFG1);
        sc.rl_cfg2.set(0);
        sc.rl_cfg3.set(RL_8139_CFG3);
        sc.rl_cfg4.set(RL_8139_CFG4);
        sc.rl_cfg5.set(RL_8139_CFG5);
    } else {
        sc.rl_cfg0.set(RL_CFG0);
        sc.rl_cfg1.set(RL_CFG1);
        sc.rl_cfg2.set(RL_CFG2);
        sc.rl_cfg3.set(RL_CFG3);
        sc.rl_cfg4.set(RL_CFG4);
        sc.rl_cfg5.set(RL_CFG5);
    }

    // Reset the adapter.
    re_reset(sc);

    sc.rl_tx_time.set(5); // 125us
    sc.rl_rx_time.set(2); // 50us
    if sc.flags() & RL_FLAG_PCIE != 0 {
        sc.rl_sim_time.set(75); // 75us
    } else {
        sc.rl_sim_time.set(125); // 125us
    }
    sc.rl_imtype.set(RL_IMTYPE_SIM as i32); // simulated interrupt moderation

    if sc.sc_hwrev.get() == RL_HWREV_8139CPLUS {
        sc.rl_bus_speed.set(33); // XXX
    } else if sc.flags() & RL_FLAG_PCIE != 0 {
        sc.rl_bus_speed.set(125);
    } else {
        let cfg2 = sc.csr_read_1(sc.rl_cfg2.get());
        match cfg2 & RL_CFG2_PCI_MASK {
            RL_CFG2_PCI_33MHZ => sc.rl_bus_speed.set(33),
            RL_CFG2_PCI_66MHZ => sc.rl_bus_speed.set(66),
            _ => {
                printf(format_args!(
                    "{}: unknown bus speed, assume 33MHz\n",
                    sc.devname()
                ));
                sc.rl_bus_speed.set(33);
            }
        }

        if cfg2 & RL_CFG2_PCI_64BIT != 0 {
            sc.set_flags(RL_FLAG_PCI64);
        }
    }

    re_config_imtype(sc, sc.rl_imtype.get());

    let mut eaddr = [0u8; ETHER_ADDR_LEN];
    if sc.flags() & RL_FLAG_PAR != 0 {
        // XXX Should have a better way to extract station address from EEPROM.
        for (i, b) in eaddr.iter_mut().enumerate() {
            *b = sc.csr_read_1(RL_IDR0 + i as u32) as u8;
        }
    } else {
        let mut re_did = [0u16; 1];
        sc.rl_eewidth.set(RL_9356_ADDR_LEN as i32);
        re_read_eeprom(sc, &mut re_did, 0);
        if re_did[0] != 0x8129 {
            sc.rl_eewidth.set(RL_9346_ADDR_LEN as i32);
        }

        // Get station address from the EEPROM.
        let mut as_ = [0u16; ETHER_ADDR_LEN / 2];
        re_read_eeprom(sc, &mut as_, RL_EE_EADDR as i32);
        for (i, w) in as_.iter().enumerate() {
            eaddr[2 * i..2 * i + 2].copy_from_slice(&u16::from_le(*w).to_ne_bytes());
        }
    }

    // Set RX length mask, TX poll request register and descriptor count.
    let ld = &sc.rl_ldata;
    if sc.sc_hwrev.get() == RL_HWREV_8139CPLUS {
        sc.rl_rxlenmask.set(RL_RDESC_STAT_FRAGLEN);
        sc.rl_txstart.set(RL_TXSTART);
        ld.rl_tx_desc_cnt.set(RL_8139_TX_DESC_CNT as i32);
        ld.rl_rx_desc_cnt.set(RL_8139_RX_DESC_CNT as i32);
        ld.rl_tx_ndescs.set(RL_8139_NTXSEGS as i32);
    } else {
        sc.rl_rxlenmask.set(RL_RDESC_STAT_GFRAGLEN);
        sc.rl_txstart.set(RL_GTXSTART);
        ld.rl_tx_desc_cnt.set(RL_8169_TX_DESC_CNT as i32);
        ld.rl_rx_desc_cnt.set(RL_8169_RX_DESC_CNT as i32);
        ld.rl_tx_ndescs.set(RL_8169_NTXSEGS as i32);
    }

    sc.sc_arpcom.ac_enaddr.set(eaddr);

    let re_name = RE_REVISIONS
        .iter()
        .rev()
        .find(|rr| rr.re_chipid == sc.sc_hwrev.get())
        .map(|rr| rr.re_name);

    match re_name {
        None => printf(format_args!(
            ": unknown ASIC (0x{:04x})",
            sc.sc_hwrev.get() >> 16
        )),
        Some(name) => printf(format_args!(
            ": {} (0x{:04x})",
            name,
            sc.sc_hwrev.get() >> 16
        )),
    };

    printf(format_args!(
        ", {}, address {}\n",
        Str(intrstr),
        Str(&ether_sprintf(&sc.sc_arpcom.ac_enaddr.get()))
    ));

    re_attach_dma(sc, bus_dma64)?;

    let ifp = re_ifp(sc);
    ifp.if_softc.set(arg);
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let n = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_xflags.set(IFXF_MPSAFE);
    if bus_dma64 != 0 {
        ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_MBUF_64BIT);
    }
    ifp.if_ioctl.set(Some(re_ioctl));
    ifp.if_qstart.set(Some(re_start));
    ifp.if_watchdog.set(Some(re_watchdog));
    ifp.if_hardmtu.set(sc.rl_max_mtu.get() as u32);
    ifq_init_maxlen(&ifp.if_snd, ld.rl_tx_desc_cnt.get() as u32);

    let mut caps = IFCAP_VLAN_MTU | IFCAP_CSUM_TCPv4 | IFCAP_CSUM_UDPv4;

    // RTL8168/8111C generates wrong IP checksummed frame if the packet has IP options so
    // disable TX IP checksum offloading.
    match sc.sc_hwrev.get() {
        RL_HWREV_8168C | RL_HWREV_8168C_SPIN2 | RL_HWREV_8168CP => {}
        _ => caps |= IFCAP_CSUM_IPv4,
    }

    if NVLAN > 0 {
        caps |= IFCAP_VLAN_HWTAGGING;
    }

    // SMALL_KERNEL is not defined.
    caps |= IFCAP_WOL;
    ifp.if_capabilities.set(caps);
    ifp.if_wol.set(Some(re_wol));
    let _ = re_wol(ifp, false);

    timeout_set(&sc.timer_handle, re_tick, arg);
    task_set(&sc.rl_start, re_txstart, arg);

    // Take PHY out of power down mode.
    if sc.flags() & RL_FLAG_PHYWAKE_PM != 0 {
        sc.csr_write_1(RL_PMCH, sc.csr_read_1(RL_PMCH) | 0x80);
        if sc.sc_hwrev.get() == RL_HWREV_8401E {
            sc.csr_write_1(0xD1, sc.csr_read_1(0xD1) & !0x08);
        }
    }
    if sc.flags() & RL_FLAG_PHYWAKE != 0 {
        re_gmii_writereg(&sc.sc_dev, 1, 0x1f, 0);
        re_gmii_writereg(&sc.sc_dev, 1, 0x0e, 0);
    }

    // Do MII setup
    let mii = &sc.sc_mii;
    mii.mii_ifp.set(Some(ifp));
    mii.mii_readreg.set(Some(re_miibus_readreg));
    mii.mii_writereg.set(Some(re_miibus_writereg));
    mii.mii_statchg.set(Some(re_miibus_statchg));
    ifmedia_init(&mii.mii_media, IFM_IMASK, re_ifmedia_upd, re_ifmedia_sts);
    mii_attach(
        &sc.sc_dev,
        mii,
        0xffff_ffff_u32 as i32,
        MII_PHY_ANY,
        MII_OFFSET_ANY,
        MIIF_DOPAUSE,
    );
    if mii.mii_phys.first().is_none() {
        printf(format_args!("{}: no PHY found!\n", sc.devname()));
        ifmedia_add(&mii.mii_media, IFM_ETHER | IFM_NONE, 0, ptr::null_mut());
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_NONE);
    } else {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_AUTO);
    }

    // Call MI attach routine.
    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);

    // NKSTAT > 0: re_kstat_attach(sc); kstat(4) is not configured.

    Ok(())
}

/// The DMA part of `re_attach`: the transmit ring and its buffer maps, the receive ring (with
/// the transmit pad after it) and its buffer maps; on a failure, what was made is undone
/// (the C's `fail_*` labels) and the error returned.
fn re_attach_dma(sc: &'static RlSoftc, bus_dma64: i32) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;
    let dev = sc.devname();

    // fail_0..fail_3 for the TX ring, fail_4 for its buffer maps.
    let destroy_tx_maps = || {
        // Destroy DMA maps for TX buffers.
        for q in ld.rl_txq.iter() {
            if let Some(map) = q.txq_dmamap.take() {
                // SAFETY: a map made below for this slot and loaded with nothing.
                unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
            }
        }
    };

    // Allocate DMA'able memory for the TX ring
    let mut seg = [BusDmaSegment::default(); 1];
    let nseg = match bus_dmamem_alloc(
        dmat,
        sc.rl_tx_list_sz(),
        RL_RING_ALIGN as BusSize,
        0,
        &mut seg,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO | bus_dma64,
    ) {
        Ok(n) => n,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't allocate tx listseg, error = {}\n",
                error as i32
            ));
            return Err(error);
        }
    };
    ld.rl_tx_listseg.set(seg[0]);
    ld.rl_tx_listnseg.set(nseg as i32);
    let free_tx = |seg: &[BusDmaSegment]| {
        // SAFETY: called only with the TX ring's segment, allocated above and not mapped.
        unsafe { bus_dmamem_free(dmat, seg) }
    };

    // Load the map for the TX ring.
    let txkva = match bus_dmamem_map(
        dmat,
        &mut seg[..nseg],
        sc.rl_tx_list_sz(),
        BUS_DMA_COHERENT | BUS_DMA_NOWAIT,
    ) {
        Ok(kva) => kva,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't map tx list, error = {}\n",
                error as i32
            ));
            free_tx(&seg[..nseg]);
            return Err(error);
        }
    };
    ld.rl_tx_list.set(txkva.as_ptr().cast());
    let unmap_tx = || {
        ld.rl_tx_list.set(ptr::null_mut());
        // SAFETY: the TX ring's mapping, made above; no map holds it any more.
        unsafe { bus_dmamem_unmap(dmat, txkva, sc.rl_tx_list_sz()) };
    };

    let txmap = match bus_dmamap_create(
        dmat,
        sc.rl_tx_list_sz(),
        1,
        sc.rl_tx_list_sz(),
        0,
        bus_dma64,
    ) {
        Ok(m) => m,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't create tx list map, error = {}\n",
                error as i32
            ));
            unmap_tx();
            free_tx(&seg[..nseg]);
            return Err(error);
        }
    };
    ld.rl_tx_list_map.set(Some(txmap));
    let destroy_txmap = || {
        ld.rl_tx_list_map.set(None);
        // SAFETY: the TX ring's map, made above, unloaded.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(txmap)) };
    };

    // SAFETY: the TX ring's mapping stays until re_attach's failure path unmaps it after
    // unloading the map; the controller is not pointed at it before re_init.
    if let Err(error) = unsafe {
        bus_dmamap_load(
            dmat,
            txmap,
            txkva.as_ptr(),
            sc.rl_tx_list_sz(),
            None,
            BUS_DMA_NOWAIT,
        )
    } {
        printf(format_args!(
            "{dev}: can't load tx list, error = {}\n",
            error as i32
        ));
        destroy_txmap();
        unmap_tx();
        free_tx(&seg[..nseg]);
        return Err(error);
    }
    let fail_tx = || {
        destroy_tx_maps();
        // Free DMA'able memory for the TX ring.
        bus_dmamap_unload(dmat, txmap);
        destroy_txmap();
        unmap_tx();
        free_tx(&[ld.rl_tx_listseg.get()][..nseg]);
    };

    // Create DMA maps for TX buffers
    for q in ld.rl_txq.iter().take(ld.rl_tx_desc_cnt.get() as usize) {
        match bus_dmamap_create(
            dmat,
            RL_JUMBO_FRAMELEN as BusSize,
            ld.rl_tx_ndescs.get(),
            RL_JUMBO_FRAMELEN as BusSize,
            0,
            bus_dma64,
        ) {
            Ok(m) => q.txq_dmamap.set(Some(m)),
            Err(error) => {
                printf(format_args!("{dev}: can't create DMA map for TX\n"));
                fail_tx();
                return Err(error);
            }
        }
    }

    // Allocate DMA'able memory for the RX ring
    let mut rseg = [BusDmaSegment::default(); 1];
    let rnseg = match bus_dmamem_alloc(
        dmat,
        sc.rl_rx_dmamem_sz(),
        RL_RING_ALIGN as BusSize,
        0,
        &mut rseg,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO | bus_dma64,
    ) {
        Ok(n) => n,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't allocate rx listnseg, error = {}\n",
                error as i32
            ));
            fail_tx();
            return Err(error);
        }
    };
    ld.rl_rx_listseg.set(rseg[0]);
    ld.rl_rx_listnseg.set(rnseg as i32);
    let free_rx = |seg: &[BusDmaSegment]| {
        // SAFETY: called only with the RX ring's segment, allocated above and not mapped.
        unsafe { bus_dmamem_free(dmat, seg) }
    };

    // Load the map for the RX ring.
    let rxkva = match bus_dmamem_map(
        dmat,
        &mut rseg[..rnseg],
        sc.rl_rx_dmamem_sz(),
        BUS_DMA_COHERENT | BUS_DMA_NOWAIT,
    ) {
        Ok(kva) => kva,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't map rx list, error = {}\n",
                error as i32
            ));
            free_rx(&rseg[..rnseg]);
            fail_tx();
            return Err(error);
        }
    };
    ld.rl_rx_list.set(rxkva.as_ptr().cast());
    let unmap_rx = || {
        ld.rl_rx_list.set(ptr::null_mut());
        // SAFETY: the RX ring's mapping, made above; no map holds it any more.
        unsafe { bus_dmamem_unmap(dmat, rxkva, sc.rl_rx_dmamem_sz()) };
    };

    let rxmap = match bus_dmamap_create(
        dmat,
        sc.rl_rx_dmamem_sz(),
        1,
        sc.rl_rx_dmamem_sz(),
        0,
        bus_dma64,
    ) {
        Ok(m) => m,
        Err(error) => {
            printf(format_args!(
                "{dev}: can't create rx list map, error = {}\n",
                error as i32
            ));
            unmap_rx();
            free_rx(&rseg[..rnseg]);
            fail_tx();
            return Err(error);
        }
    };
    ld.rl_rx_list_map.set(Some(rxmap));
    let destroy_rxmap = || {
        ld.rl_rx_list_map.set(None);
        // SAFETY: the RX ring's map, made above, unloaded.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(rxmap)) };
    };

    // SAFETY: as for the TX ring.
    if let Err(error) = unsafe {
        bus_dmamap_load(
            dmat,
            rxmap,
            rxkva.as_ptr(),
            sc.rl_rx_dmamem_sz(),
            None,
            BUS_DMA_NOWAIT,
        )
    } {
        printf(format_args!(
            "{dev}: can't load rx list, error = {}\n",
            error as i32
        ));
        destroy_rxmap();
        unmap_rx();
        free_rx(&rseg[..rnseg]);
        fail_tx();
        return Err(error);
    }

    // Create DMA maps for RX buffers
    let framelen = rl_framelen(sc.rl_max_mtu.get()) as BusSize;
    for r in ld.rl_rxsoft.iter().take(ld.rl_rx_desc_cnt.get() as usize) {
        match bus_dmamap_create(dmat, framelen, 1, framelen, 0, bus_dma64) {
            Ok(m) => r.rxs_dmamap.set(Some(m)),
            Err(error) => {
                printf(format_args!("{dev}: can't create DMA map for RX\n"));
                // Destroy DMA maps for RX buffers.
                for r in ld.rl_rxsoft.iter() {
                    if let Some(map) = r.rxs_dmamap.take() {
                        // SAFETY: a map made above for this slot, loaded with nothing.
                        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
                    }
                }
                // Free DMA'able memory for the RX ring.
                bus_dmamap_unload(dmat, rxmap);
                destroy_rxmap();
                unmap_rx();
                free_rx(&rseg[..rnseg]);
                fail_tx();
                return Err(error);
            }
        }
    }

    Ok(())
}

/// `re_detach`.
pub fn re_detach(sc: &'static RlSoftc) {
    let ifp = re_ifp(sc);

    // NKSTAT > 0: re_kstat_detach(sc); kstat(4) is not configured.

    // Remove timeout handler
    timeout_del(&sc.timer_handle);

    // Detach PHY
    if sc.sc_mii.mii_phys.first().is_some() {
        mii_detach(&sc.sc_mii, MII_PHY_ANY, MII_OFFSET_ANY);
    }

    // Delete media stuff
    ifmedia_delete_instance(&sc.sc_mii.mii_media, IFM_INST_ANY);
    ether_ifdetach(ifp);
    if_detach(ifp);
}

/// `re_newbuf`: a cluster for the receive descriptor at the producer index.
pub fn re_newbuf(sc: &'static RlSoftc) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;
    let framelen = rl_framelen(sc.rl_max_mtu.get());

    let Some(m) = m_clget(None, M_DONTWAIT, framelen as u32) else {
        return Err(Errno::ENOBUFS);
    };

    // Initialize mbuf length fields and fixup alignment so that the frame payload is
    // longword aligned on strict alignment archs.
    m.m_len().set(framelen as u32);
    m.m_pkthdr().len.set(framelen);
    m.m_data()
        .set(m.m_data().get().wrapping_add(RE_ETHER_ALIGN as usize));

    let idx = ld.rl_rx_prodidx.get() as u32;
    let rxs = ld.rxsoft(idx);
    let map = rxs.map();
    // SAFETY: the cluster stays the slot's (`rxs_mbuf`) until re_rxeof or re_stop unload the
    // map before handing it on or freeing it.
    if unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_READ | BUS_DMA_NOWAIT) }.is_err() {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

    sc.rl_rxdescsync(idx, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
    let mut d = ld.rxd_get(idx);
    let cmdstat = u32::from_le(d.rl_cmdstat);
    sc.rl_rxdescsync(idx, BUS_DMASYNC_PREREAD);
    if cmdstat & RL_RDESC_STAT_OWN != 0 {
        printf(format_args!(
            "{}: tried to map busy RX descriptor\n",
            sc.devname()
        ));
        bus_dmamap_unload(dmat, map);
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    rxs.rxs_mbuf.set(Some(m));

    d.rl_vlanctl = 0;
    let seg = map.dm_segs()[0].get();
    let mut cmdstat = seg.ds_len as u32;
    if idx == ld.rl_rx_desc_cnt.get() as u32 - 1 {
        cmdstat |= RL_RDESC_CMD_EOR;
    }
    re_set_bufaddr(&mut d, seg.ds_addr);
    d.rl_cmdstat = cmdstat.to_le();
    ld.rxd_set(idx, d);
    sc.rl_rxdescsync(idx, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    cmdstat |= RL_RDESC_CMD_OWN;
    d.rl_cmdstat = cmdstat.to_le();
    ld.rxd_set(idx, d);
    sc.rl_rxdescsync(idx, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

    ld.rl_rx_prodidx.set(sc.rl_next_rx_desc(idx) as i32);

    Ok(())
}

/// `re_tx_list_init`.
pub fn re_tx_list_init(sc: &RlSoftc) -> Result<(), Errno> {
    let ld = &sc.rl_ldata;

    for i in 0..ld.rl_tx_desc_cnt.get() as u32 {
        ld.txd_set(i, RlDesc::default());
    }
    for q in ld.rl_txq.iter().take(ld.rl_tx_desc_cnt.get() as usize) {
        q.txq_mbuf.set(None);
    }

    let map = ld.tx_list_map();
    bus_dmamap_sync(
        sc.dmat(),
        map,
        0,
        map.dm_mapsize.get(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );
    ld.rl_txq_prodidx.store(0, Ordering::Relaxed);
    ld.rl_txq_considx.store(0, Ordering::Relaxed);
    ld.rl_tx_free.set(ld.rl_tx_desc_cnt.get());
    ld.rl_tx_nextfree.set(0);

    Ok(())
}

/// `re_rx_list_init`.
pub fn re_rx_list_init(sc: &'static RlSoftc) -> Result<(), Errno> {
    let ld = &sc.rl_ldata;

    for i in 0..ld.rl_rx_desc_cnt.get() as u32 {
        ld.rxd_set(i, RlDesc::default());
    }

    ld.rl_rx_prodidx.set(0);
    ld.rl_rx_considx.set(0);
    sc.rl_head.set(None);
    sc.rl_tail.set(None);

    let cnt = ld.rl_rx_desc_cnt.get() as u32;
    ld.with_rx_ring(|r| if_rxr_init(r, 2, cnt - 1));
    re_rx_list_fill(sc);

    Ok(())
}

/// `re_rx_list_fill`.
pub fn re_rx_list_fill(sc: &'static RlSoftc) {
    let ld = &sc.rl_ldata;
    let cnt = ld.rl_rx_desc_cnt.get() as u32;

    let mut slots = ld.with_rx_ring(|r| if_rxr_get(r, cnt));
    while slots > 0 {
        if re_newbuf(sc) == Err(Errno::ENOBUFS) {
            break;
        }
        slots -= 1;
    }
    ld.with_rx_ring(|r| if_rxr_put(r, slots));
}

/// `re_rxeof`: RX handler for C+ and 8169. For the gigE chips, we support the reception of
/// jumbo frames that have been fragmented across multiple 2K mbuf cluster buffers.
pub fn re_rxeof(sc: &'static RlSoftc) -> bool {
    let ml = MbufList::new();
    let ifp = re_ifp(sc);
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;
    let framelen = rl_framelen(sc.rl_max_mtu.get());
    let mut rx = false;

    let mut i = ld.rl_rx_considx.get() as u32;
    while if_rxr_inuse(&ld.rl_rx_ring.get()) > 0 {
        sc.rl_rxdescsync(i, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
        let cur_rx = ld.rxd_get(i);
        let mut rxstat = u32::from_le(cur_rx.rl_cmdstat);
        let rxvlan = u32::from_le(cur_rx.rl_vlanctl);
        sc.rl_rxdescsync(i, BUS_DMASYNC_PREREAD);
        if rxstat & RL_RDESC_STAT_OWN != 0 {
            break;
        }
        let total_len = (rxstat & sc.rl_rxlenmask.get()) as i32;
        let rxs = ld.rxsoft(i);
        let Some(m) = rxs.rxs_mbuf.take() else {
            panic(format_args!("{}: no mbuf in rx slot {i}", sc.devname()));
        };
        ld.with_rx_ring(|r| if_rxr_put(r, 1));
        rx = true;

        // Invalidate the RX mbuf and unload its map

        let map = rxs.map();
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
        bus_dmamap_unload(dmat, map);

        let next = sc.rl_next_rx_desc(i);

        if sc.flags() & RL_FLAG_JUMBOV2 != 0
            && (rxstat & (RL_RDESC_STAT_SOF | RL_RDESC_STAT_EOF))
                != (RL_RDESC_STAT_SOF | RL_RDESC_STAT_EOF)
        {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            m_freem(m);
            i = next;
            continue;
        } else if rxstat & RL_RDESC_STAT_EOF == 0 {
            m.m_len().set(framelen as u32);
            match sc.rl_tail.get() {
                None => {
                    sc.rl_head.set(Some(m));
                    sc.rl_tail.set(Some(m));
                }
                Some(tail) => {
                    m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
                    tail.m_next().set(Some(m));
                    sc.rl_tail.set(Some(m));
                }
            }
            i = next;
            continue;
        }

        // NOTE: for the 8139C+, the frame length field is always 12 bits in size, but for
        // the gigE chips, it is 13 bits (since the max RX frame length is 16K).
        // Unfortunately, all 32 bits in the status word were already used, so to make room
        // for the extra length bit, Realtek took out the 'frame alignment error' bit and
        // shifted the other status bits over one slot. The OWN, EOR, FS and LS bits are still
        // in the same places. We have already extracted the frame length and checked the OWN
        // bit, so rather than using an alternate bit mapping, we shift the status bits one
        // space to the right so we can evaluate them using the 8169 status as though it was
        // in the same format as that of the 8139C+.
        if sc.sc_hwrev.get() != RL_HWREV_8139CPLUS {
            rxstat >>= 1;
        }

        // if total_len > 2^13-1, both _RXERRSUM and _GIANT will be set, but if CRC is clear,
        // it will still be a valid frame.
        if rxstat & RL_RDESC_STAT_RXERRSUM != 0
            && !(rxstat & RL_RDESC_STAT_RXERRSUM != 0
                && !(total_len > 8191 && (rxstat & RL_RDESC_STAT_ERRS) == RL_RDESC_STAT_GIANT))
        {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            // If this is part of a multi-fragment packet, discard all the pieces.
            if let Some(head) = sc.rl_head.take() {
                m_freem(head);
                sc.rl_tail.set(None);
            }
            m_freem(m);
            i = next;
            continue;
        }

        let m = if let Some(head) = sc.rl_head.get() {
            let mut len = total_len % framelen;
            if len == 0 {
                len = framelen;
            }
            m.m_len().set(len as u32);
            let Some(tail) = sc.rl_tail.get() else {
                panic(format_args!("{}: rx chain without a tail", sc.devname()));
            };
            // Special case: if there's 4 bytes or less in this buffer, the mbuf can be
            // discarded: the last 4 bytes is the CRC, which we don't care about anyway.
            if len <= ETHER_CRC_LEN as i32 {
                tail.m_len()
                    .set(tail.m_len().get() - (ETHER_CRC_LEN as u32 - len as u32));
                m_freem(m);
            } else {
                m.m_len().set(len as u32 - ETHER_CRC_LEN as u32);
                m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
                tail.m_next().set(Some(m));
            }
            sc.rl_head.set(None);
            sc.rl_tail.set(None);
            head.m_pkthdr().len.set(total_len - ETHER_CRC_LEN as i32);
            head
        } else {
            let len = total_len - ETHER_CRC_LEN as i32;
            m.m_pkthdr().len.set(len);
            m.m_len().set(len as u32);
            m
        };

        // Do RX checksumming

        let csum = &m.m_pkthdr().csum_flags;
        if sc.flags() & RL_FLAG_DESCV2 != 0 {
            // Check IP header checksum
            if rxvlan & RL_RDESC_IPV4 != 0 && rxstat & RL_RDESC_STAT_IPSUMBAD == 0 {
                csum.set(csum.get() | M_IPV4_CSUM_IN_OK);
            }

            // Check TCP/UDP checksum
            if rxvlan & (RL_RDESC_IPV4 | RL_RDESC_IPV6) != 0
                && ((rxstat & RL_RDESC_STAT_TCP != 0 && rxstat & RL_RDESC_STAT_TCPSUMBAD == 0)
                    || (rxstat & RL_RDESC_STAT_UDP != 0 && rxstat & RL_RDESC_STAT_UDPSUMBAD == 0))
            {
                csum.set(csum.get() | M_TCP_CSUM_IN_OK | M_UDP_CSUM_IN_OK);
            }
        } else {
            // Check IP header checksum
            if rxstat & RL_RDESC_STAT_PROTOID != 0 && rxstat & RL_RDESC_STAT_IPSUMBAD == 0 {
                csum.set(csum.get() | M_IPV4_CSUM_IN_OK);
            }

            // Check TCP/UDP checksum
            if (rl_tcppkt(rxstat) && rxstat & RL_RDESC_STAT_TCPSUMBAD == 0)
                || (rl_udppkt(rxstat) && rxstat & RL_RDESC_STAT_UDPSUMBAD == 0)
            {
                csum.set(csum.get() | M_TCP_CSUM_IN_OK | M_UDP_CSUM_IN_OK);
            }
        }
        if NVLAN > 0 && rxvlan & RL_RDESC_VLANCTL_TAG != 0 {
            m.m_pkthdr()
                .ether_vtag
                .set(u16::from_be((rxvlan & RL_RDESC_VLANCTL_DATA) as u16));
            m.m_flags().set(m.m_flags().get() | M_VLANTAG);
        }

        ml_enqueue(&ml, m);
        i = next;
    }

    if ifiq_input(&ifp.if_rcv, &ml) {
        ld.with_rx_ring(if_rxr_livelocked);
    }

    ld.rl_rx_considx.set(i as i32);
    re_rx_list_fill(sc);

    rx
}

/// `re_txeof`: reclaims the transmitted frames; whether any was.
pub fn re_txeof(sc: &'static RlSoftc) -> bool {
    let ifp = re_ifp(sc);
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;
    let mut free = 0;

    let prod = ld.rl_txq_prodidx.load(Ordering::Acquire);
    let mut cons = ld.rl_txq_considx.load(Ordering::Relaxed);

    while prod != cons {
        let txq = ld.txq(cons);

        let idx = txq.txq_descidx.get() as u32;
        sc.rl_txdescsync(idx, BUS_DMASYNC_POSTREAD);
        let txstat = u32::from_le(ld.txd_get(idx).rl_cmdstat);
        sc.rl_txdescsync(idx, BUS_DMASYNC_PREREAD);
        if txstat & RL_TDESC_CMD_OWN != 0 {
            free = 2;
            break;
        }

        let map = txq.map();
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
        bus_dmamap_unload(dmat, map);
        if let Some(m) = txq.txq_mbuf.take() {
            m_freem(m);
        }

        if txstat & (RL_TDESC_STAT_EXCESSCOL | RL_TDESC_STAT_COLCNT) != 0 {
            ifp.if_collisions().set(ifp.if_collisions().get() + 1);
        }
        if txstat & RL_TDESC_STAT_TXERRSUM != 0 {
            ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
        }

        cons = sc.rl_next_tx_desc(idx);
        free = 1;
    }

    if free == 0 {
        return false;
    }

    ld.rl_txq_considx.store(cons, Ordering::Release);

    // Some chips will ignore a second TX request issued while an existing transmission is in
    // progress. If the transmitter goes idle but there are still packets waiting to be sent,
    // we need to restart the channel here to flush them out. This only seems to be required
    // with the PCIe devices.
    if ifq_is_oactive(&ifp.if_snd) {
        ifq_restart(&ifp.if_snd);
    } else if free == 2 {
        ifq_serialize(&ifp.if_snd, &sc.rl_start);
    } else {
        ifp.if_timer.set(0);
    }

    true
}

/// `re_tick`: the one-second timer: the PHY's tick and the link state.
pub fn re_tick(xsc: *mut c_void) {
    let sc = re_arg(xsc);
    let mii = &sc.sc_mii;

    let s = splnet();

    mii_tick(mii);

    if sc.flags() & RL_FLAG_LINK == 0 {
        re_miibus_statchg(&sc.sc_dev);
    }

    splx(s);

    timeout_add_sec(&sc.timer_handle, 1);
}

/// `re_intr`: the interrupt handler.
pub fn re_intr(arg: *mut c_void) -> i32 {
    let sc = re_arg(arg);
    let ifp = re_ifp(sc);
    let mut claimed = 0;

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        return 0;
    }

    // Disable interrupts.
    sc.csr_write_2(RL_IMR, 0);

    let mut rx = false;
    let mut tx = false;
    let status = sc.csr_read_2(RL_ISR);
    // If the card has gone away the read returns 0xffff.
    if status == 0xffff {
        return 0;
    }
    if status != 0 {
        sc.csr_write_2(RL_ISR, status);
    }

    if status & RL_ISR_TIMEOUT_EXPIRED != 0 {
        claimed = 1;
    }

    if status & RL_INTRS_CPLUS != 0 {
        if status & (u32::from(sc.rl_rx_ack.get()) | RL_ISR_RX_ERR | RL_ISR_FIFO_OFLOW) != 0 {
            rx |= re_rxeof(sc);
            claimed = 1;
        }

        if status & (u32::from(sc.rl_tx_ack.get()) | RL_ISR_TX_ERR) != 0 {
            tx |= re_txeof(sc);
            claimed = 1;
        }

        if status & RL_ISR_SYSTEM_ERR != 0 {
            kernel_lock();
            let _ = re_init(ifp);
            kernel_unlock();
            claimed = 1;
        }
    }

    if sc.rl_imtype.get() == RL_IMTYPE_SIM as i32 {
        if sc.rl_timerintr.get() != 0 {
            if !(tx || rx) {
                // Nothing needs to be processed, fallback to use TX/RX interrupts.
                re_setup_intr(sc, true, RL_IMTYPE_NONE as i32);

                // Recollect, mainly to avoid the possible race introduced by changing
                // interrupt masks.
                re_rxeof(sc);
                re_txeof(sc);
            } else {
                sc.csr_write_4(RL_TIMERCNT, 1); // reload
            }
        } else if tx || rx {
            // Assume that using simulated interrupt moderation (hardware timer based) could
            // reduce the interrupt rate.
            re_setup_intr(sc, true, RL_IMTYPE_SIM as i32);
        }
    }

    sc.csr_write_2(RL_IMR, u32::from(sc.rl_intrs.get()));

    claimed
}

/// `re_jumbo_cksum`: the checksums `re_encap` computes in software for a frame longer than
/// `RL_MTU` on a `RL_FLAG_JUMBOV2` chip: the C's `in_cksum` of the IP header and
/// `in_delayed_cksum` of the payload over a stack mbuf that starts past the Ethernet
/// header, here at their offsets in the chain.
fn re_jumbo_cksum(m: &Mbuf) {
    let eoff = ETHER_HDR_LEN;
    let mut iph = [0u8; IP_HDR_LEN];
    m_copydata(m, eoff as i32, &mut iph);
    let flags = m.m_pkthdr().csum_flags.get();

    if flags & M_IPV4_CSUM_OUT != 0 {
        // ip->ip_sum = in_cksum(&mh, sizeof(struct ip))
        let sum = in4_cksum(m, 0, eoff as i32, IP_HDR_LEN as i32);
        let _ = m_copyback(
            m,
            (eoff + IP_SUM_OFFSET) as i32,
            &sum.to_ne_bytes(),
            M_NOWAIT,
        );
    }
    if flags & (M_TCP_CSUM_OUT | M_UDP_CSUM_OUT) != 0 {
        // in_delayed_cksum(&mh)
        let hl = usize::from(iph[0] & 0x0f) << 2;
        let p = i32::from(iph[9]);
        let off = eoff + hl;
        let mut csum = in4_cksum(m, 0, off as i32, m.m_pkthdr().len.get() - off as i32);
        if csum == 0 && p == IPPROTO_UDP {
            csum = 0xffff;
        }
        let field = match p {
            IPPROTO_TCP => TCPHDR_TH_SUM,
            IPPROTO_UDP => UDPHDR_UH_SUM,
            IPPROTO_ICMP => ICMP_CKSUM_OFFSET,
            _ => return,
        };
        let _ = m_copyback(m, (off + field) as i32, &csum.to_ne_bytes(), M_NOWAIT);
    }
}

/// `re_encap`: maps `m` into descriptors from `idx`; the descriptors used, 0 when the frame
/// cannot be mapped (the caller frees it).
pub fn re_encap(sc: &RlSoftc, idx: u32, m: &'static Mbuf) -> u32 {
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;
    let mut vlanctl: u32 = 0;
    let mut csum_flags: u32 = 0;
    let out = M_IPV4_CSUM_OUT | M_TCP_CSUM_OUT | M_UDP_CSUM_OUT;

    // Set up checksum offload. Note: checksum offload bits must appear in all descriptors
    // of a multi-descriptor transmit attempt. This is according to testing done with an
    // 8169 chip. This is a requirement.

    // Set RL_TDESC_CMD_IPCSUM if any checksum offloading is requested. Otherwise,
    // RL_TDESC_CMD_TCPCSUM/RL_TDESC_CMD_UDPCSUM does not take affect.

    let csum = &m.m_pkthdr().csum_flags;
    if sc.flags() & RL_FLAG_JUMBOV2 != 0
        && m.m_pkthdr().len.get() > RL_MTU as i32
        && csum.get() & out != 0
    {
        re_jumbo_cksum(m);
        csum.set(csum.get() & !out);
    }

    if csum.get() & out != 0 {
        if sc.flags() & RL_FLAG_DESCV2 != 0 {
            vlanctl |= RL_TDESC_CMD_IPCSUMV2;
            if csum.get() & M_TCP_CSUM_OUT != 0 {
                vlanctl |= RL_TDESC_CMD_TCPCSUMV2;
            }
            if csum.get() & M_UDP_CSUM_OUT != 0 {
                vlanctl |= RL_TDESC_CMD_UDPCSUMV2;
            }
        } else {
            csum_flags |= RL_TDESC_CMD_IPCSUM;
            if csum.get() & M_TCP_CSUM_OUT != 0 {
                csum_flags |= RL_TDESC_CMD_TCPCSUM;
            }
            if csum.get() & M_UDP_CSUM_OUT != 0 {
                csum_flags |= RL_TDESC_CMD_UDPCSUM;
            }
        }
    }

    let txq = ld.txq(idx);
    let map: &BusDmamap = txq.map();

    // SAFETY: the mbuf stays the slot's (`txq_mbuf`) until re_txeof or re_stop unload the
    // map before freeing it.
    match unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_WRITE | BUS_DMA_NOWAIT) } {
        Ok(()) => {}
        Err(Errno::EFBIG) => {
            if m_defrag(m, M_DONTWAIT).is_err() {
                return 0;
            }
            // SAFETY: as above.
            let r = unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_WRITE | BUS_DMA_NOWAIT) };
            if r.is_err() {
                return 0;
            }
        }
        Err(_) => return 0,
    }

    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREWRITE);

    let dm_nsegs = map.dm_nsegs.get() as u32;
    let mut nsegs = dm_nsegs;
    let mut pad = false;

    // With some of the Realtek chips, using the checksum offload support in conjunction with
    // the autopadding feature results in the transmission of corrupt frames. For example, if
    // we need to send a really small IP fragment that's less than 60 bytes in size, and IP
    // header checksumming is enabled, the resulting ethernet frame that appears on the wire
    // will have garbled payload. To work around this, if TX IP checksum offload is enabled,
    // we always manually pad short frames out to the minimum ethernet frame size.
    if sc.flags() & RL_FLAG_AUTOPAD == 0
        && m.m_pkthdr().len.get() < RL_IP4CSUMTX_PADLEN as i32
        && csum.get() & M_IPV4_CSUM_OUT != 0
    {
        pad = true;
        nsegs += 1;
    }

    // Set up hardware VLAN tagging. Note: vlan tag info must appear in all descriptors of a
    // multi-descriptor transmission attempt.
    if NVLAN > 0 && m.m_flags().get() & M_VLANTAG != 0 {
        vlanctl |= u32::from(m.m_pkthdr().ether_vtag.get().swap_bytes()) | RL_TDESC_VLANCTL_TAG;
    }

    // Map the segment array into descriptors. Note that we set the start-of-frame and
    // end-of-frame markers for either TX or RX, but they really only have meaning in the TX
    // case. (In the RX case, it's the chip that tells us where packets begin and end.) We
    // also keep track of the end of the ring and set the end-of-ring bits as needed, and we
    // set the ownership bits in all except the very first descriptor. (The caller will set
    // this descriptor later when it start transmission or reception.)
    let last_desc = ld.rl_tx_desc_cnt.get() as u32 - 1;
    let mut curidx = idx;
    let mut lastidx = idx;
    let mut cmdstat = RL_TDESC_CMD_SOF;

    for seg in map.dm_segs().iter().take(dm_nsegs as usize) {
        let seg = seg.get();
        sc.rl_txdescsync(curidx, BUS_DMASYNC_POSTWRITE);

        let mut d = ld.txd_get(curidx);
        d.rl_vlanctl = vlanctl.to_le();
        re_set_bufaddr(&mut d, seg.ds_addr);
        cmdstat |= csum_flags | seg.ds_len as u32;

        if curidx == last_desc {
            cmdstat |= RL_TDESC_CMD_EOR;
        }

        d.rl_cmdstat = cmdstat.to_le();
        ld.txd_set(curidx, d);

        sc.rl_txdescsync(curidx, BUS_DMASYNC_PREWRITE);

        lastidx = curidx;
        cmdstat = RL_TDESC_CMD_OWN;
        curidx = sc.rl_next_tx_desc(curidx);
    }

    if pad {
        sc.rl_txdescsync(curidx, BUS_DMASYNC_POSTWRITE);

        let mut d = ld.txd_get(curidx);
        d.rl_vlanctl = vlanctl.to_le();
        re_set_bufaddr(&mut d, sc.rl_txpaddaddr());
        cmdstat = csum_flags
            | RL_TDESC_CMD_OWN
            | RL_TDESC_CMD_EOF
            | (RL_IP4CSUMTX_PADLEN + 1 - m.m_pkthdr().len.get() as u32);

        if curidx == last_desc {
            cmdstat |= RL_TDESC_CMD_EOR;
        }

        d.rl_cmdstat = cmdstat.to_le();
        ld.txd_set(curidx, d);

        sc.rl_txdescsync(curidx, BUS_DMASYNC_PREWRITE);

        lastidx = curidx;
    }

    // d is already pointing at the last descriptor
    let mut d = ld.txd_get(lastidx);
    d.rl_cmdstat |= RL_TDESC_CMD_EOF.to_le();
    ld.txd_set(lastidx, d);

    // Transfer ownership of packet to the chip.
    sc.rl_txdescsync(curidx, BUS_DMASYNC_POSTWRITE);
    let mut d = ld.txd_get(idx);
    d.rl_cmdstat |= RL_TDESC_CMD_OWN.to_le();
    ld.txd_set(idx, d);
    sc.rl_txdescsync(curidx, BUS_DMASYNC_PREWRITE);

    // update info of TX queue and descriptors
    txq.txq_mbuf.set(Some(m));
    txq.txq_descidx.set(lastidx as i32);

    nsegs
}

/// `re_txstart`: `rl_start`'s function: kick the transmitter.
pub fn re_txstart(xsc: *mut c_void) {
    let sc = re_arg(xsc);

    sc.csr_write_1(sc.rl_txstart.get(), RL_TXSTART_START);
}

/// `re_start`: main transmit routine for C+ and gigE NICs.
pub fn re_start(ifq: &'static Ifqueue) {
    let Some(ifp) = ifq.ifq_if.get() else {
        return;
    };
    let sc = re_softc(ifp);
    let ld = &sc.rl_ldata;
    let cnt = ld.rl_tx_desc_cnt.get() as u32;
    let mut post = false;

    if sc.flags() & RL_FLAG_LINK == 0 {
        ifq_purge(ifq);
        return;
    }

    let mut free = ld.rl_txq_considx.load(Ordering::Acquire);
    let mut idx = ld.rl_txq_prodidx.load(Ordering::Relaxed);
    if free <= idx {
        free += cnt;
    }
    free -= idx;

    loop {
        if free < ld.rl_tx_ndescs.get() as u32 + 2 {
            ifq_set_oactive(ifq);
            break;
        }

        let Some(m) = ifq_dequeue(ifq) else {
            break;
        };

        let used = re_encap(sc, idx, m);
        if used == 0 {
            m_freem(m);
            continue;
        }

        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap_ether(bpf, m, BPF_DIRECTION_OUT);
        }

        kassert!(used <= free);
        free -= used;

        idx += used;
        if idx >= cnt {
            idx -= cnt;
        }

        post = true;
    }

    if !post {
        return;
    }

    ifp.if_timer.set(5);
    ld.rl_txq_prodidx.store(idx, Ordering::Release);
    ifq_serialize(ifq, &sc.rl_start);
}

/// `re_init`: (re)starts the controller.
pub fn re_init(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = re_softc(ifp);

    let s = splnet();

    // Cancel pending I/O and free all RX/TX buffers.
    re_stop(ifp);

    // Put controller into known state.
    re_reset(sc);

    // Enable C+ RX and TX mode, as well as VLAN stripping and RX checksum offload. We must
    // configure the C+ register before all others.
    let mut cfg = RL_CPLUSCMD_TXENB | RL_CPLUSCMD_PCI_MRW | RL_CPLUSCMD_RXCSUM_ENB;

    if ifp.if_capabilities.get() & IFCAP_VLAN_HWTAGGING != 0 {
        cfg |= RL_CPLUSCMD_VLANSTRIP;
    }

    if sc.flags() & RL_FLAG_MACSTAT != 0 {
        cfg |= RL_CPLUSCMD_MACSTAT_DIS;
    } else {
        cfg |= RL_CPLUSCMD_RXENB;
    }

    sc.csr_write_2(RL_CPLUS_CMD, cfg);

    // Init our MAC address. Even though the chipset documentation doesn't mention it, we
    // need to enter "Config register write enable" mode to modify the ID registers.
    let mut eaddr = [0u8; 8];
    eaddr[..ETHER_ADDR_LEN].copy_from_slice(&sc.sc_arpcom.ac_enaddr.get());
    sc.csr_write_1(RL_EECMD, RL_EEMODE_WRITECFG);
    let w4 = u32::from_ne_bytes([eaddr[4], eaddr[5], eaddr[6], eaddr[7]]);
    let w0 = u32::from_ne_bytes([eaddr[0], eaddr[1], eaddr[2], eaddr[3]]);
    sc.csr_write_4(RL_IDR4, w4.to_le());
    sc.csr_write_4(RL_IDR0, w0.to_le());
    // Default on PC Engines APU1 is to have all LEDs off unless there is network activity.
    // Override to provide a link status LED.
    if sc.sc_hwrev.get() == RL_HWREV_8168E {
        // SAFETY: `hw_vendor`/`hw_prod` are written once by the machine's attach, which
        // precedes any interface init.
        let (vendor, prod) = unsafe { (hw_vendor.read(), hw_prod.read()) };
        if vendor == Some(&b"PC Engines"[..]) && prod == Some(&b"APU"[..]) {
            sc.csr_setbit_1(RL_CFG4, RL_CFG4_CUSTOM_LED);
            sc.csr_write_1(RL_LEDSEL, RL_LED_LINK | RL_LED_ACT << 4);
        }
    }
    // Protect config register again
    sc.csr_write_1(RL_EECMD, RL_EEMODE_OFF);

    if sc.flags() & RL_FLAG_JUMBOV2 != 0 {
        re_set_jumbo(sc);
    }

    // For C+ mode, initialize the RX descriptors and mbufs.
    let _ = re_rx_list_init(sc);
    let _ = re_tx_list_init(sc);

    // Load the addresses of the RX and TX lists into the chip.
    let rx_addr = sc.rl_ldata.rx_list_map().dm_segs()[0].get().ds_addr as u64;
    let tx_addr = sc.rl_ldata.tx_list_map().dm_segs()[0].get().ds_addr as u64;
    sc.csr_write_4(RL_RXLIST_ADDR_HI, rl_addr_hi(rx_addr));
    sc.csr_write_4(RL_RXLIST_ADDR_LO, rl_addr_lo(rx_addr));

    sc.csr_write_4(RL_TXLIST_ADDR_HI, rl_addr_hi(tx_addr));
    sc.csr_write_4(RL_TXLIST_ADDR_LO, rl_addr_lo(tx_addr));

    if sc.flags() & RL_FLAG_RXDV_GATED != 0 {
        sc.csr_write_4(RL_MISC, sc.csr_read_4(RL_MISC) & !0x0008_0000);
    }

    // Set the initial TX and RX configuration.
    sc.csr_write_4(RL_TXCFG, RL_TXCFG_CONFIG);

    sc.csr_write_1(RL_EARLY_TX_THRESH, 16);

    let mut rxcfg = RL_RXCFG_CONFIG;
    if sc.flags() & RL_FLAG_EARLYOFF != 0 {
        rxcfg |= RL_RXCFG_EARLYOFF;
    } else if sc.flags() & RL_FLAG_EARLYOFFV2 != 0 {
        rxcfg |= RL_RXCFG_EARLYOFFV2;
    }
    sc.csr_write_4(RL_RXCFG, rxcfg);

    // Enable transmit and receive.
    sc.csr_write_1(RL_COMMAND, RL_CMD_TX_ENB | RL_CMD_RX_ENB);

    // Program promiscuous mode and multicast filters.
    re_iff(sc);

    // Enable interrupts. The C acknowledges `rl_intrs` after re_setup_intr; here it is
    // acknowledged first (with the mask re_config_imtype gives), so the simulated moderation
    // timer re_setup_intr starts cannot expire into the acknowledgement (see the
    // deviations).
    re_config_imtype(sc, sc.rl_imtype.get());
    sc.csr_write_2(RL_ISR, u32::from(sc.rl_intrs.get()));
    re_setup_intr(sc, true, sc.rl_imtype.get());

    // Start RX/TX process.
    sc.csr_write_4(RL_MISSEDPKT, 0);

    // For 8169 gigE NICs, set the max allowed RX packet size so we can receive jumbo frames.
    if sc.sc_hwrev.get() != RL_HWREV_8139CPLUS {
        if sc.flags() & RL_FLAG_PCIE != 0 && sc.flags() & RL_FLAG_JUMBOV2 == 0 {
            sc.csr_write_2(RL_MAXRXPKTLEN, RE_RX_DESC_BUFLEN);
        } else {
            sc.csr_write_2(RL_MAXRXPKTLEN, 16383);
        }
    }

    sc.csr_write_1(
        sc.rl_cfg1.get(),
        sc.csr_read_1(sc.rl_cfg1.get()) | RL_CFG1_DRVLOAD,
    );

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    splx(s);

    sc.clr_flags(RL_FLAG_LINK);
    let _ = mii_mediachg(&sc.sc_mii);

    timeout_add_sec(&sc.timer_handle, 1);

    Ok(())
}

/// `re_ifmedia_upd`: set media options.
pub fn re_ifmedia_upd(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = re_softc(ifp);

    mii_mediachg(&sc.sc_mii)
}

/// `re_ifmedia_sts`: report current media status.
pub fn re_ifmedia_sts(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = re_softc(ifp);

    mii_pollstat(&sc.sc_mii);
    ifmr.ifm_active = sc.sc_mii.mii_media_active.get();
    ifmr.ifm_status = sc.sc_mii.mii_media_status.get();
}

/// `re_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn re_ioctl(ifp: &'static Ifnet, command: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = re_softc(ifp);
    let mut error = Ok(());

    let s = splnet();

    match command {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                let _ = re_init(ifp);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    let _ = re_init(ifp);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                re_stop(ifp);
            }
        }
        SIOCGIFMEDIA | SIOCSIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
            // ifmediareq` (this function's contract).
            error = unsafe { ifmedia_ioctl(ifp, data, &sc.sc_mii.mii_media, command) };
        }
        SIOCGIFRXR => {
            // SAFETY: SIOCGIFRXR carries a `struct ifreq` (this function's contract).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            error = if_rxr_ioctl(
                ifr.ifr_data() as usize,
                None,
                rl_framelen(sc.rl_max_mtu.get()) as u32,
                &sc.rl_ldata.rl_rx_ring.get(),
            );
        }
        _ => {
            // SAFETY: this function's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_arpcom, command, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            re_iff(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

/// `re_watchdog`.
pub fn re_watchdog(ifp: &'static Ifnet) {
    let sc = re_softc(ifp);
    let s = splnet();
    printf(format_args!("{}: watchdog timeout\n", sc.devname()));

    let _ = re_init(ifp);

    splx(s);
}

/// `re_stop`: stop the adapter and free any mbufs allocated to the RX and TX lists.
pub fn re_stop(ifp: &'static Ifnet) {
    let sc = re_softc(ifp);
    let dmat = sc.dmat();
    let ld = &sc.rl_ldata;

    ifp.if_timer.set(0);
    sc.clr_flags(RL_FLAG_LINK);
    sc.rl_timerintr.set(0);

    timeout_del(&sc.timer_handle);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);

    // Disable accepting frames to put RX MAC into idle state. Otherwise it's possible to get
    // frames while stop command execution is in progress and controller can DMA the frame to
    // already freed RX buffer during that period.
    sc.csr_write_4(
        RL_RXCFG,
        sc.csr_read_4(RL_RXCFG)
            & !(RL_RXCFG_RX_ALLPHYS | RL_RXCFG_RX_BROAD | RL_RXCFG_RX_INDIV | RL_RXCFG_RX_MULTI),
    );

    if sc.flags() & RL_FLAG_WAIT_TXPOLL != 0 {
        let mut i = RL_TIMEOUT;
        while i > 0 {
            if sc.csr_read_1(sc.rl_txstart.get()) & RL_TXSTART_START == 0 {
                break;
            }
            delay(20);
            i -= 1;
        }
        if i == 0 {
            printf(format_args!(
                "{}: stopping TX poll timed out!\n",
                sc.devname()
            ));
        }
        sc.csr_write_1(RL_COMMAND, 0x00);
    } else if sc.flags() & RL_FLAG_CMDSTOP != 0 {
        sc.csr_write_1(RL_COMMAND, RL_CMD_STOPREQ | RL_CMD_TX_ENB | RL_CMD_RX_ENB);
        if sc.flags() & RL_FLAG_CMDSTOP_WAIT_TXQ != 0 {
            let mut i = RL_TIMEOUT;
            while i > 0 {
                if sc.csr_read_4(RL_TXCFG) & RL_TXCFG_QUEUE_EMPTY != 0 {
                    break;
                }
                delay(100);
                i -= 1;
            }
            if i == 0 {
                printf(format_args!("{}: stopping TXQ timed out!\n", sc.devname()));
            }
        }
    } else {
        sc.csr_write_1(RL_COMMAND, 0x00);
    }
    delay(1000);
    sc.csr_write_2(RL_IMR, 0x0000);
    sc.csr_write_2(RL_ISR, 0xFFFF);

    if let Some(ih) = NonNull::new(sc.sc_ih.get()) {
        intr_barrier(ih);
    }
    ifq_barrier(&ifp.if_snd);

    ifq_clr_oactive(&ifp.if_snd);
    mii_down(&sc.sc_mii);

    if let Some(head) = sc.rl_head.take() {
        m_freem(head);
        sc.rl_tail.set(None);
    }

    // Free the TX list buffers.
    for q in ld.rl_txq.iter().take(ld.rl_tx_desc_cnt.get() as usize) {
        if let Some(m) = q.txq_mbuf.take() {
            bus_dmamap_unload(dmat, q.map());
            m_freem(m);
        }
    }

    // Free the RX list buffers.
    for r in ld.rl_rxsoft.iter().take(ld.rl_rx_desc_cnt.get() as usize) {
        if let Some(m) = r.rxs_mbuf.take() {
            bus_dmamap_unload(dmat, r.map());
            m_freem(m);
        }
    }
}

/// `re_setup_hw_im`: hardware interrupt moderation.
pub fn re_setup_hw_im(sc: &RlSoftc) {
    kassert!(sc.flags() & RL_FLAG_HWIM != 0);

    // Interrupt moderation
    //
    // 0xABCD
    // A - unknown (maybe TX related)
    // B - TX timer (unit: 25us)
    // C - unknown (maybe RX related)
    // D - RX timer (unit: 25us)
    //
    //
    // re(4)'s interrupt moderation is actually controlled by two variables, like most other
    // NICs (bge, bnx etc.)
    // o  timer
    // o  number of packets [P]
    //
    // The logic relationship between these two variables is similar to other NICs too:
    // if (timer expire || packets > [P])
    //     Interrupt is delivered
    //
    // Currently we only know how to set 'timer', but not 'number of packets', which should
    // be ~30, as far as I tested (sink ~900Kpps, interrupt rate is 30KHz)
    sc.csr_write_2(
        RL_IM,
        rl_im_rxtime(sc.rl_rx_time.get() as u32)
            | rl_im_txtime(sc.rl_tx_time.get() as u32)
            | RL_IM_MAGIC,
    );
}

/// `re_disable_hw_im`.
pub fn re_disable_hw_im(sc: &RlSoftc) {
    if sc.flags() & RL_FLAG_HWIM != 0 {
        sc.csr_write_2(RL_IM, 0);
    }
}

/// `re_setup_sim_im`: simulated interrupt moderation with the chip's timer.
pub fn re_setup_sim_im(sc: &RlSoftc) {
    if sc.sc_hwrev.get() == RL_HWREV_8139CPLUS {
        sc.csr_write_4(RL_TIMERINT, 0x400); // XXX
    } else {
        // Datasheet says tick decreases at bus speed, but it seems the clock runs a little
        // bit faster, so we do some compensation here.
        let nticks = (sc.rl_sim_time.get() * sc.rl_bus_speed.get() * 8) / 5;
        sc.csr_write_4(RL_TIMERINT_8169, nticks as u32);
    }
    sc.csr_write_4(RL_TIMERCNT, 1); // reload
    sc.rl_timerintr.set(1);
}

/// `re_disable_sim_im`.
pub fn re_disable_sim_im(sc: &RlSoftc) {
    if sc.sc_hwrev.get() == RL_HWREV_8139CPLUS {
        sc.csr_write_4(RL_TIMERINT, 0);
    } else {
        sc.csr_write_4(RL_TIMERINT_8169, 0);
    }
    sc.rl_timerintr.set(0);
}

/// `re_config_imtype`: the interrupts to enable and acknowledge for `imtype`.
pub fn re_config_imtype(sc: &RlSoftc, imtype: i32) {
    match imtype as u32 {
        RL_IMTYPE_HW | RL_IMTYPE_NONE => {
            if imtype as u32 == RL_IMTYPE_HW {
                kassert!(sc.flags() & RL_FLAG_HWIM != 0);
            }
            sc.rl_intrs.set(RL_INTRS_CPLUS as u16);
            sc.rl_rx_ack
                .set((RL_ISR_RX_OK | RL_ISR_FIFO_OFLOW | RL_ISR_RX_OVERRUN) as u16);
            sc.rl_tx_ack.set(RL_ISR_TX_OK as u16);
        }

        RL_IMTYPE_SIM => {
            sc.rl_intrs.set(RL_INTRS_TIMER as u16);
            sc.rl_rx_ack.set(RL_ISR_TIMEOUT_EXPIRED as u16);
            sc.rl_tx_ack.set(RL_ISR_TIMEOUT_EXPIRED as u16);
        }

        _ => panic(format_args!("{}: unknown imtype {}", sc.devname(), imtype)),
    }
}

/// `re_set_jumbo`.
pub fn re_set_jumbo(sc: &RlSoftc) {
    sc.csr_write_1(RL_EECMD, RL_EEMODE_WRITECFG);
    sc.csr_write_1(RL_CFG3, sc.csr_read_1(RL_CFG3) | RL_CFG3_JUMBO_EN0);

    match sc.sc_hwrev.get() {
        RL_HWREV_8168DP => {}
        RL_HWREV_8168E => {
            sc.csr_write_1(RL_CFG4, sc.csr_read_1(RL_CFG4) | RL_CFG4_8168E_JUMBO_EN1);
        }
        _ => {
            sc.csr_write_1(RL_CFG4, sc.csr_read_1(RL_CFG4) | RL_CFG4_JUMBO_EN1);
        }
    }

    sc.csr_write_1(RL_EECMD, RL_EEMODE_OFF);
}

/// `re_setup_intr`.
pub fn re_setup_intr(sc: &RlSoftc, enable_intrs: bool, imtype: i32) {
    re_config_imtype(sc, imtype);

    if enable_intrs {
        sc.csr_write_2(RL_IMR, u32::from(sc.rl_intrs.get()));
    } else {
        sc.csr_write_2(RL_IMR, 0);
    }

    match imtype as u32 {
        RL_IMTYPE_NONE => {
            re_disable_sim_im(sc);
            re_disable_hw_im(sc);
        }

        RL_IMTYPE_HW => {
            kassert!(sc.flags() & RL_FLAG_HWIM != 0);
            re_disable_sim_im(sc);
            re_setup_hw_im(sc);
        }

        RL_IMTYPE_SIM => {
            re_disable_hw_im(sc);
            re_setup_sim_im(sc);
        }

        _ => panic(format_args!("{}: unknown imtype {}", sc.devname(), imtype)),
    }
}

/// `re_wol`: wake on LAN (magic packet only).
pub fn re_wol(ifp: &'static Ifnet, enable: bool) -> Result<(), Errno> {
    let sc = re_softc(ifp);

    if enable {
        if sc.csr_read_1(sc.rl_cfg1.get()) & RL_CFG1_PME == 0 {
            printf(format_args!(
                "{}: power management is disabled, cannot do WOL\n",
                sc.devname()
            ));
            return Err(Errno::ENOTSUP);
        }
        if sc.csr_read_1(sc.rl_cfg2.get()) & RL_CFG2_AUXPWR == 0 {
            printf(format_args!(
                "{}: no auxiliary power, cannot do WOL from D3 (power-off) state\n",
                sc.devname()
            ));
        }
    }

    re_iff(sc);

    // Temporarily enable write to configuration registers.
    sc.csr_write_1(RL_EECMD, RL_EEMODE_WRITECFG);

    // Always disable all wake events except magic packet.
    let mut val = sc.csr_read_1(sc.rl_cfg5.get());
    val &= !(RL_CFG5_WOL_UCAST | RL_CFG5_WOL_MCAST | RL_CFG5_WOL_BCAST);
    sc.csr_write_1(sc.rl_cfg5.get(), val);

    let mut val = sc.csr_read_1(sc.rl_cfg3.get());
    if enable {
        val |= RL_CFG3_WOL_MAGIC;
        val &= !RL_CFG3_WOL_LINK;
    } else {
        val &= !(RL_CFG3_WOL_MAGIC | RL_CFG3_WOL_LINK);
    }
    sc.csr_write_1(sc.rl_cfg3.get(), val);

    sc.csr_write_1(RL_EECMD, RL_EEMODE_OFF);

    Ok(())
}

// NKSTAT > 0: RE_DTCCR_CMD, RE_DTCCR_LO, RE_DTCCR_HI, struct re_kstats, re_kstats_tpl,
// struct re_kstat_softc, re_kstat_read, re_kstat_copy, re_kstat_attach and
// re_kstat_detach; kstat(4) is not configured.
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_8139cplus_flags() {
        let (f, mtu) = re_hwrev_flags(RL_HWREV_8139CPLUS, 0x8139);
        assert_eq!(f, RL_FLAG_FASTETHER | RL_FLAG_AUTOPAD);
        assert_eq!(mtu, Some(RL_MTU));
        let (f, mtu) = re_hwrev_flags(RL_HWREV_8168B_SPIN1, 0);
        assert_eq!(f, RL_FLAG_WOLRXENB | RL_FLAG_PHYWAKE | RL_FLAG_MACSTAT);
        assert_eq!(mtu, Some(RL_MTU));
        assert_eq!(re_hwrev_flags(0x1234_0000, 0), (0, None));
        // RL_HWREV_8169 is 0.
        assert_eq!(
            re_hwrev_flags(RL_HWREV_8169, 0),
            (RL_FLAG_MACRESET, Some(RL_JUMBO_MTU_7K))
        );
        assert_eq!(rl_framelen(1500), 1500 + 14 + 4 + 4);
    }

    #[test]
    fn bufaddr_splits_little_endian() {
        let mut d = RlDesc::default();
        re_set_bufaddr(&mut d, 0x1_2345_6780);
        assert_eq!(u32::from_le(d.rl_bufaddr_lo), 0x2345_6780);
        assert_eq!(u32::from_le(d.rl_bufaddr_hi), 1);
    }

    #[test]
    fn revision_names() {
        let name = |id| {
            RE_REVISIONS
                .iter()
                .rev()
                .find(|r| r.re_chipid == id)
                .map(|r| r.re_name)
        };
        assert_eq!(name(RL_HWREV_8139CPLUS), Some("RTL8139C+"));
        assert_eq!(name(RL_HWREV_8168C_SPIN2), Some("RTL8168C/8111C"));
        assert_eq!(name(0x1234), None);
    }
}
/* </TESTS> */
