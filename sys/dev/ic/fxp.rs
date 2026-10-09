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
 * Copyright (c) 1995, David Greenman
 * All rights reserved.
 *
 * Modifications to support NetBSD:
 * Copyright (c) 1997 Jason R. Thorpe.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice unmodified, this list of conditions, and the following
 *    disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: if_fxp.c,v 1.55 1998/08/04 08:53:12 dg Exp
 */
/* </LICENSES> */

/* <CODE> */
//! fxp(4): the Intel EtherExpress PRO/100 (i82557, i82558, i82559, i82550, i82551, i82562
//! ...) Fast Ethernet driver, bus-independent part (`dev/ic/fxp.c`). `if_fxp_pci.rs`
//! attaches it.
//!
//! Upstream: sys/dev/ic/fxp.c @ 3ce1f3f79392
//!
//! The chip is driven through a command block list (CBL) for transmission and a linked list
//! of receive frame areas (RFA), each at the start of an mbuf cluster, for reception. The
//! CBL, the statistics dump area and the one-shot configuration, address, multicast and
//! microcode blocks are `struct fxp_ctrl`, one `bus_dmamem` segment. The EEPROM holds the
//! station address and the PHY description, the PHY is reached through the MDI register and
//! mii(4) (inphy(4) for the 82555 of QEMU's `i82559er`). On the 82558 and later the
//! receive bundling microcode ("CPUSaver") of `rcvbundl.h` is loaded with `loadfirmware(9)`
//! from `/etc/firmware/fxp-*` when the interface is initialised.
//!
//! ## Deviations
//! - `tx_next`, `tx_cb` and `tx_off` of `struct fxp_txsw` are computed from the slot index;
//!   `sc_cbt_cons`, `sc_cbt_prod` and `sc_cbt_prev` are indexes (`fxpvar.rs`). The DMA
//!   structures are read and written through [`Dma`] views (volatile, bounds checked),
//!   whole blocks (`fxp_cb_config`, `fxp_cb_ias`, ...) built in a local and stored once
//!   where the C stores their members one by one.
//! - The receive cluster keeps the `bus_dmamap_t` in its first bytes as in the C; the RFA
//!   sits `RFA_ALIGNMENT_FUDGE` bytes into the cluster and is reached as a [`Dma`] view.
//! - `fxp_add_rfabuf` and `fxp_attach`, `fxp_init` return `Result<(), Errno>` where the C
//!   returns 0 or 1/an errno: `fxp_add_rfabuf` fails (with `ENOBUFS`) both when there is no
//!   mbuf and when the old mbuf was recycled, the C's return of 1 in both cases.
//! - In `fxp_intr`, the loop condition `cb_command & htole16(FXP_CB_COMMAND_NOP)` is
//!   omitted: `FXP_CB_COMMAND_NOP` is 0, so the term is always false (and clippy denies an
//!   `x & 0` test).
//! - `fxp_load_ucode` refuses a firmware file of more than `MAXUCODESIZE` dwords (the C
//!   would write past the download block) with a message instead of overflowing.
//! - `bus_dmamap_load` of a receive cluster is not checked, as in the C.
//! - `fxp_int_delay`, `fxp_bundle_max`, `fxp_min_size_mask` and `tx_threshold` are atomics.
//! - `SMALL_KERNEL` is not defined (the microcode is loaded); `NBPFILTER > 0`; `DEBUG` is not
//!   defined (the `microcode loaded` line is not printed).
//! - `RAMDISK_HOOKS` is not configured, so on the ramdisk `loadfirmware` finds no
//!   `/etc/firmware/fxp-*` and prints `fxp0: error 2, could not read firmware fxp-d101s`
//!   at the first `fxp_init`, then goes on without the microcode, as OpenBSD's bsd.rd does.

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::firmload::loadfirmware;
use crate::dev::ic::fxpreg::*;
use crate::dev::ic::fxpvar::*;
use crate::dev::microcode::fxp::rcvbundl::*;
use crate::dev::mii::mii::{
    mii_attach, mii_detach, mii_down, mii_mediachg, mii_pollstat, mii_tick,
};
use crate::dev::mii::mii_physubr::mii_phy_reset;
use crate::dev::mii::miivar::{MII_OFFSET_ANY, MII_PHY_ANY, MIIF_NOISOLATE};
use crate::kern::kern_malloc::free;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, log, panic, printf};
use crate::kern::uipc_mbuf::{m_clget, m_defrag, m_free, m_freem, m_gethdr, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmamap, BusSize, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load, bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc,
    bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_NET, splassert, splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IFCAP_VLAN_MTU, IFF_ALLMULTI, IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING,
    IFF_SIMPLEX, IFF_UP, IFNAMSIZ, Ifmediareq, if_attach, if_detach, if_input,
};
use crate::net::if_ethersubr::{ether_ifattach, ether_ifdetach, ether_ioctl, ether_sprintf};
use crate::net::if_media::{
    IFM_10_T, IFM_AUTO, IFM_ETHER, IFM_INST_ANY, IFM_MANUAL, ifmedia_add, ifmedia_delete_instance,
    ifmedia_init, ifmedia_ioctl, ifmedia_match, ifmedia_set,
};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{
    ifq_clr_oactive, ifq_dequeue, ifq_empty, ifq_init_maxlen, ifq_is_oactive, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    ETHER_ADDR_LEN, ETHER_HDR_LEN, EtherMultistep, ether_first_multi, ether_next_multi,
};
use crate::sys::device::{Cfdriver, DV_IFNET, DVACT_SUSPEND, DVACT_WAKEUP, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_DEVBUF;
use crate::sys::mbuf::{M_DONTWAIT, M_EXT, MCLBYTES, MT_DATA, Mbuf, MbufList};
use crate::sys::param::PAGE_SIZE;
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA};
use crate::sys::syslog::LOG_ERR;

/// `RFA_ALIGNMENT_FUDGE`: the card DMAs the packet immediately following the RFA, and the
/// first thing in the packet is a 14-byte Ethernet header; the RFA is put 2 bytes (plus the
/// map pointer) into the cluster so that the data after the header is 32-bit aligned.
const RFA_ALIGNMENT_FUDGE: usize = 2 + size_of::<*const BusDmamap>();

/// `FXP_TXCB_MASK`: TxCB list index mask, used to do list wrap-around.
const FXP_TXCB_MASK: usize = FXP_NTXCB - 1;

/// `FXP_MAX_RX_IDLE`: maximum number of seconds that the receiver can be idle before we
/// assume it's dead and attempt to reset it by reprogramming the multicast filter. This is
/// part of a work-around for a bug in the NIC. See `fxp_stats_update`.
const FXP_MAX_RX_IDLE: i32 = 15;

/// `fxp_cb_config_template[]`: the default configuration parameters (see `FxpCbConfig` for
/// the bit definitions; `cb_command` is filled in later), as the block they make.
const fn fxp_cb_config_template() -> FxpCbConfig {
    FxpCbConfig {
        cb_status: 0,
        cb_command: 0,
        link_addr: 0xffff_ffff,
        byte_count: 0x16,
        fifo_limit: 0x08,
        adaptive_ifs: 0x00,
        ctrl0: 0x00,
        rx_dma_bytecount: 0x00,
        tx_dma_bytecount: 0x80,
        ctrl1: 0xb2,
        ctrl2: 0x03,
        mediatype: 0x01,
        void2: 0x00,
        ctrl3: 0x26,
        linear_priority: 0x00,
        interfrm_spacing: 0x60,
        void3: 0x00,
        void4: 0xf2,
        promiscuous: 0x48,
        void5: 0x00,
        void6: 0x40,
        stripping: 0xf3,
        fdx_pin: 0x00,
        multi_ia: 0x3f,
        mc_all: 0x05,
    }
}

/// `tx_threshold`: the initial transmit threshold is 64 (512 bytes). This is increased by 64
/// (512 bytes) at a time, to maximum of 192 (1536 bytes), if an underrun occurs.
static TX_THRESHOLD: AtomicI32 = AtomicI32::new(64);

/// `fxp_int_delay`: interrupts coalescing parameter.
pub static FXP_INT_DELAY_: AtomicI32 = AtomicI32::new(FXP_INT_DELAY);
/// `fxp_bundle_max`: interrupts coalescing parameter.
pub static FXP_BUNDLE_MAX_: AtomicI32 = AtomicI32::new(FXP_BUNDLE_MAX);
/// `fxp_min_size_mask`: interrupts coalescing parameter.
pub static FXP_MIN_SIZE_MASK_: AtomicI32 = AtomicI32::new(FXP_MIN_SIZE_MASK);

/// `fxp_cd`.
pub static FXP_CD: Cfdriver = Cfdriver::new(b"fxp", DV_IFNET, 0);

/// `(struct fxp_softc *)ifp->if_softc`.
pub fn fxp_softc(ifp: &Ifnet) -> &'static FxpSoftc {
    let p = ifp.if_softc.get().cast::<FxpSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("fxp: interface without its softc"));
    }
    // SAFETY: fxp_attach sets `if_softc` to its softc and installs fxp's functions only on
    // its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// The softc behind the C's `void *` argument (interrupt, timeout).
fn fxp_arg(arg: *mut c_void) -> &'static FxpSoftc {
    if arg.is_null() {
        panic(format_args!("fxp: no softc argument"));
    }
    // SAFETY: fxp_pci_attach and fxp_attach pass the softc, which is never freed while the
    // device exists.
    unsafe { &*arg.cast::<FxpSoftc>() }
}

/// The softc of the device the mii(4) callbacks and the autoconf hooks name
/// (`(struct fxp_softc *)self`).
fn fxp_dev_sc(dev: &Device) -> &'static FxpSoftc {
    // SAFETY: mii(4) and autoconf call back with the device fxp attached, whose softc starts
    // with a `struct fxp_softc` (`fxp_pci_softc`), never freed while the device exists.
    unsafe { &*ptr::from_ref(dev.softc::<FxpSoftc>()) }
}

/// `&sc->sc_arpcom.ac_if`.
fn fxp_ifp(sc: &'static FxpSoftc) -> &'static Ifnet {
    &sc.sc_arpcom.ac_if
}

/// `sc->txs[i].tx_next` as an index.
const fn fxp_txs_next(i: usize) -> usize {
    (i + 1) & FXP_TXCB_MASK
}

/// `fxp_lwcopy`: copies a 16-bit aligned 32-bit quantity, as two halves, into the RFA at
/// `off`.
fn fxp_lwcopy(v: u32, rfa: &Dma, off: usize) {
    rfa.wr16(off, v as u16);
    rfa.wr16(off + 2, (v >> 16) as u16);
}

/// The RFA of the cluster of `m`, `RFA_ALIGNMENT_FUDGE` bytes in.
fn fxp_rfa(m: &Mbuf) -> Dma {
    let buf = m.m_ext().ext_buf.get();
    // SAFETY: `m` is a cluster mbuf of the receive list (`fxp_add_rfabuf`), whose cluster is
    // MCLBYTES long; the RFA is the 16 bytes at the fudge offset, inside it, and is the
    // chip's as well as ours (volatile accesses).
    unsafe { Dma::new(buf.wrapping_add(RFA_ALIGNMENT_FUDGE), size_of::<FxpRfa>()) }
}

/// `*((bus_dmamap_t *)m->m_ext.ext_buf)`: the DMA map `fxp_add_rfabuf` left at the start of
/// the cluster.
fn fxp_rxmap_of(m: &Mbuf) -> &'static BusDmamap {
    let buf = m.m_ext().ext_buf.get();
    // SAFETY: every cluster in the receive list has a `&'static BusDmamap` stored by
    // `fxp_rxmap_store` in its first bytes (the cluster is at least a pointer long and
    // aligned); the map lives as long as the softc.
    unsafe { &**buf.cast::<*const BusDmamap>() }
}

/// `*((bus_dmamap_t *)m->m_ext.ext_buf) = rxmap`.
fn fxp_rxmap_store(m: &Mbuf, rxmap: &'static BusDmamap) {
    let buf = m.m_ext().ext_buf.get();
    // SAFETY: `m` has a cluster (`M_EXT`) of MCLBYTES bytes, aligned for a pointer, which
    // nothing else uses at its start (the RFA is past the fudge).
    unsafe { buf.cast::<*const BusDmamap>().write(ptr::from_ref(rxmap)) }
}

/// Loads the cluster of `m` into `rxmap` (`bus_dmamap_load(sc->sc_dmat, rxmap,
/// m->m_ext.ext_buf, m->m_ext.ext_size, NULL, BUS_DMA_NOWAIT)`); the C does not check it.
fn fxp_rxmap_load(sc: &FxpSoftc, rxmap: &BusDmamap, m: &Mbuf) {
    let ext = m.m_ext();
    // SAFETY: the cluster stays this map's until fxp_intr, fxp_stop or fxp_init unload the
    // map before handing the mbuf on or freeing it.
    let _ = unsafe {
        bus_dmamap_load(
            sc.dmat(),
            rxmap,
            ext.ext_buf.get(),
            ext.ext_size.get() as BusSize,
            None,
            BUS_DMA_NOWAIT,
        )
    };
}

/// `fxp_scb_wait`: wait for the previous command to be accepted (but not necessarily
/// completed).
pub fn fxp_scb_wait(sc: &FxpSoftc) {
    let mut i = FXP_CMD_TMO;

    while sc.csr_read_2(FXP_CSR_SCB_COMMAND) & 0xff != 0 {
        i -= 1;
        if i == 0 {
            break;
        }
        delay(2);
    }
    if i == 0 {
        printf(format_args!("{}: warning: SCB timed out\n", sc.devname()));
    }
}

/// `fxp_eeprom_shiftin`.
pub fn fxp_eeprom_shiftin(sc: &FxpSoftc, data: i32, length: i32) {
    // Shift in data.
    let mut x = 1 << (length - 1);
    while x != 0 {
        let reg = if data & x != 0 {
            FXP_EEPROM_EECS | FXP_EEPROM_EEDI
        } else {
            FXP_EEPROM_EECS
        };
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
        delay(1);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg | FXP_EEPROM_EESK);
        delay(1);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
        delay(1);
        x >>= 1;
    }
}

/// `fxp_eeprom_putword`.
pub fn fxp_eeprom_putword(sc: &FxpSoftc, offset: i32, data: u16) {
    // Erase/write enable.
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
    fxp_eeprom_shiftin(sc, 0x4, 3);
    fxp_eeprom_shiftin(sc, 0x03 << (sc.eeprom_size.get() - 2), sc.eeprom_size.get());
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
    delay(1);
    // Shift in write opcode, address, data.
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
    fxp_eeprom_shiftin(sc, FXP_EEPROM_OPC_WRITE, 3);
    fxp_eeprom_shiftin(sc, offset, sc.eeprom_size.get());
    fxp_eeprom_shiftin(sc, i32::from(data), 16);
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
    delay(1);
    // Wait for EEPROM to finish up.
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
    delay(1);
    for _ in 0..1000 {
        if sc.csr_read_2(FXP_CSR_EEPROMCONTROL) & FXP_EEPROM_EEDO != 0 {
            break;
        }
        delay(50);
    }
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
    delay(1);
    // Erase/write disable.
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
    fxp_eeprom_shiftin(sc, 0x4, 3);
    fxp_eeprom_shiftin(sc, 0, sc.eeprom_size.get());
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
    delay(1);
}

/// `fxp_write_eeprom`.
pub fn fxp_write_eeprom(sc: &FxpSoftc, data: &[u16], offset: i32, words: i32) {
    for i in 0..words {
        fxp_eeprom_putword(sc, offset + i, data[i as usize]);
    }
}

/*************************************************************
 * Operating system-specific autoconfiguration glue
 *************************************************************/

/// `fxp_activate`.
pub fn fxp_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = fxp_dev_sc(self_);
    let ifp = fxp_ifp(sc);

    match act {
        DVACT_SUSPEND => {
            if ifp.if_flags.get() & IFF_RUNNING != 0 {
                fxp_stop(sc, true, false);
            }
        }
        DVACT_WAKEUP if ifp.if_flags.get() & IFF_UP != 0 => fxp_wakeup(sc),
        _ => {}
    }
    Ok(())
}

/// `fxp_wakeup`.
pub fn fxp_wakeup(sc: &'static FxpSoftc) {
    let s = splnet();

    // force reload of the microcode
    sc.sc_flags.set(sc.sc_flags.get() & !FXPF_UCODELOADED);

    fxp_init(sc);
    splx(s);
}

/*************************************************************
 * End of operating system-specific autoconfiguration glue
 *************************************************************/

/// The part of `fxp_attach`'s `fail:` label that undoes the control structure's DMA setup.
fn fxp_attach_free_ctrl(sc: &FxpSoftc) {
    let dmat = sc.dmat();
    if let Some(map) = sc.tx_cb_map.take() {
        bus_dmamap_unload(dmat, map);
        // SAFETY: the control map `fxp_attach` created and loaded, now unloaded.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        if let Some(kva) = NonNull::new(sc.sc_ctrl.get().cast::<u8>()) {
            sc.sc_ctrl.set(ptr::null_mut());
            // SAFETY: the mapping `fxp_attach` made of this segment; nothing uses it now.
            unsafe {
                bus_dmamem_unmap(dmat, kva, size_of::<FxpCbTx>() * FXP_NTXCB);
            }
        }
        let seg = [sc.sc_cb_seg.get()];
        // SAFETY: the segment `fxp_attach` allocated, unmapped just above.
        unsafe { bus_dmamem_free(dmat, &seg[..sc.sc_cb_nseg.get().max(0) as usize]) };
    }
}

/// `fxp_attach`: do generic parts of attach.
pub fn fxp_attach(sc: &'static FxpSoftc, intrstr: &[u8]) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();

    // Reset to a stable state.
    sc.csr_write_4(FXP_CSR_PORT, FXP_PORT_SOFTWARE_RESET);
    delay(10);

    let ctrl_size = size_of::<FxpCtrl>();
    let mut seg = [BusDmaSegment::default(); 1];
    match bus_dmamem_alloc(
        dmat,
        ctrl_size,
        PAGE_SIZE,
        0,
        &mut seg,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) {
        Ok(n) => {
            sc.sc_cb_seg.set(seg[0]);
            sc.sc_cb_nseg.set(n as i32);
        }
        Err(_) => return fxp_attach_fail(sc),
    }
    let kva = match bus_dmamem_map(
        dmat,
        &mut seg[..sc.sc_cb_nseg.get() as usize],
        ctrl_size,
        BUS_DMA_NOWAIT,
    ) {
        Ok(kva) => kva,
        Err(_) => {
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &seg[..sc.sc_cb_nseg.get() as usize]) };
            return fxp_attach_fail(sc);
        }
    };
    sc.sc_ctrl.set(kva.as_ptr().cast());
    let map = match bus_dmamap_create(dmat, ctrl_size, 1, ctrl_size, 0, BUS_DMA_NOWAIT) {
        Ok(m) => m,
        Err(_) => {
            sc.sc_ctrl.set(ptr::null_mut());
            // SAFETY: the mapping and the segment made above, no map holds them.
            unsafe {
                bus_dmamem_unmap(dmat, kva, ctrl_size);
                bus_dmamem_free(dmat, &seg[..sc.sc_cb_nseg.get() as usize]);
            }
            return fxp_attach_fail(sc);
        }
    };
    // SAFETY: the control structure stays mapped and loaded until the detach or the failure
    // path below unloads the map.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), ctrl_size, None, BUS_DMA_NOWAIT) }.is_err()
    {
        sc.sc_ctrl.set(ptr::null_mut());
        // SAFETY: as above, the map is not loaded.
        unsafe {
            bus_dmamap_destroy(dmat, NonNull::from(map));
            bus_dmamem_unmap(dmat, kva, ctrl_size);
            bus_dmamem_free(dmat, &seg[..sc.sc_cb_nseg.get() as usize]);
        }
        return fxp_attach_fail(sc);
    }
    sc.tx_cb_map.set(Some(map));

    for i in 0..FXP_NTXCB {
        match bus_dmamap_create(dmat, MCLBYTES, FXP_NTXSEG as i32, MCLBYTES, 0, 0) {
            Ok(m) => {
                sc.txs[i].tx_map.set(Some(m));
                sc.txs[i].tx_mbuf.set(None);
            }
            Err(err) => {
                printf(format_args!(
                    "{}: unable to create tx dma map {}, error {}\n",
                    sc.devname(),
                    i,
                    err as i32
                ));
                return fxp_attach_fail(sc);
            }
        }
    }

    // Pre-allocate some receive buffers.
    sc.sc_rxfree.set(0);
    for i in 0..FXP_NRFABUFS_MIN {
        match bus_dmamap_create(dmat, MCLBYTES, 1, MCLBYTES, 0, 0) {
            Ok(m) => sc.sc_rxmaps[i].set(Some(m)),
            Err(err) => {
                printf(format_args!(
                    "{}: unable to create rx dma map {}, error {}\n",
                    sc.devname(),
                    i,
                    err as i32
                ));
                return fxp_attach_fail(sc);
            }
        }
        sc.rx_bufs.set(sc.rx_bufs.get() + 1);
    }
    for _ in 0..FXP_NRFABUFS_MIN {
        if fxp_add_rfabuf(sc, None).is_err() {
            return fxp_attach_fail(sc);
        }
    }

    // Find out how large of an SEEPROM we have.
    fxp_autosize_eeprom(sc);

    // Get info about the primary PHY
    let mut data = [0u16; 1];
    fxp_read_eeprom(sc, &mut data, FXP_EEPROM_REG_PHY, 1);
    sc.phy_primary_addr.set(i32::from(data[0] & 0xff));
    sc.phy_primary_device.set(i32::from((data[0] >> 8) & 0x3f));
    sc.phy_10mbps_only.set(i32::from(data[0] >> 15));

    // Only 82558 and newer cards can do this.
    if sc.sc_revision.get() >= FXP_REV_82558_A4 {
        sc.sc_int_delay
            .set(FXP_INT_DELAY_.load(Ordering::Relaxed) as u16);
        sc.sc_bundle_max
            .set(FXP_BUNDLE_MAX_.load(Ordering::Relaxed) as u16);
        sc.sc_min_size_mask
            .set(FXP_MIN_SIZE_MASK_.load(Ordering::Relaxed) as u16);
    }
    // Read MAC address.
    let mut enaddr_w = [0u16; ETHER_ADDR_LEN / 2];
    fxp_read_eeprom(sc, &mut enaddr_w, FXP_EEPROM_REG_MAC, 3);
    let mut enaddr = [0u8; ETHER_ADDR_LEN];
    for (i, w) in enaddr_w.iter().enumerate() {
        enaddr[2 * i..2 * i + 2].copy_from_slice(&w.to_ne_bytes());
    }

    let ifp = fxp_ifp(sc);
    sc.sc_arpcom.ac_enaddr.set(enaddr);
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.xname().as_bytes();
    let n = name.len().min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);
    ifp.if_softc.set(arg);
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_ioctl.set(Some(fxp_ioctl));
    ifp.if_start.set(Some(fxp_start));
    ifp.if_watchdog.set(Some(fxp_watchdog));
    ifq_init_maxlen(&ifp.if_snd, FXP_NTXCB as u32 - 1);

    ifp.if_capabilities.set(IFCAP_VLAN_MTU);

    printf(format_args!(
        ": {}, address {}\n",
        Str(intrstr),
        Str(&ether_sprintf(&sc.sc_arpcom.ac_enaddr.get()))
    ));

    if sc.sc_flags.get() & FXPF_DISABLE_STANDBY != 0 {
        let mut data = [0u16; 1];
        fxp_read_eeprom(sc, &mut data, FXP_EEPROM_REG_ID, 1);
        if data[0] & FXP_EEPROM_REG_ID_STB != 0 {
            printf(format_args!(
                "{}: Disabling dynamic standby mode in EEPROM",
                sc.devname()
            ));
            data[0] &= !FXP_EEPROM_REG_ID_STB;
            fxp_write_eeprom(sc, &data, FXP_EEPROM_REG_ID, 1);
            printf(format_args!(", New ID 0x{:x}", data[0]));
            let mut cksum: u16 = 0;
            for i in 0..(1i32 << sc.eeprom_size.get()) - 1 {
                fxp_read_eeprom(sc, &mut data, i, 1);
                cksum = cksum.wrapping_add(data[0]);
            }
            let i = (1i32 << sc.eeprom_size.get()) - 1;
            cksum = 0xBABAu16.wrapping_sub(cksum);
            fxp_read_eeprom(sc, &mut data, i, 1);
            fxp_write_eeprom(sc, &[cksum], i, 1);
            printf(format_args!(
                ", cksum @ 0x{:x}: 0x{:x} -> 0x{:x}\n",
                i, data[0], cksum
            ));
        }
    }

    // Receiver lock-up workaround detection.
    let mut data = [0u16; 1];
    fxp_read_eeprom(sc, &mut data, FXP_EEPROM_REG_COMPAT, 1);
    if data[0] & (FXP_EEPROM_REG_COMPAT_MC10 | FXP_EEPROM_REG_COMPAT_MC100)
        != (FXP_EEPROM_REG_COMPAT_MC10 | FXP_EEPROM_REG_COMPAT_MC100)
    {
        sc.sc_flags.set(sc.sc_flags.get() | FXPF_RECV_WORKAROUND);
    }

    // Initialize our media structures and probe the MII.
    let mii = &sc.sc_mii;
    mii.mii_ifp.set(Some(ifp));
    mii.mii_readreg.set(Some(fxp_mdi_read));
    mii.mii_writereg.set(Some(fxp_mdi_write));
    mii.mii_statchg.set(Some(fxp_statchg));
    ifmedia_init(&mii.mii_media, 0, fxp_mediachange, fxp_mediastatus);
    mii_attach(
        &sc.sc_dev,
        mii,
        0xffff_ffff_u32 as i32,
        MII_PHY_ANY,
        MII_OFFSET_ANY,
        MIIF_NOISOLATE,
    );
    // If no phy found, just use auto mode
    if mii.mii_phys.first().is_none() {
        ifmedia_add(&mii.mii_media, IFM_ETHER | IFM_MANUAL, 0, ptr::null_mut());
        printf(format_args!(
            "{}: no phy found, using manual mode\n",
            sc.devname()
        ));
    }

    if ifmedia_match(&mii.mii_media, IFM_ETHER | IFM_MANUAL, 0) {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_MANUAL);
    } else if ifmedia_match(&mii.mii_media, IFM_ETHER | IFM_AUTO, 0) {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_AUTO);
    } else {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_10_T);
    }

    // Attach the interface.
    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);

    // Initialize timeout for statistics update.
    timeout_set(&sc.stats_update_to, fxp_stats_update, arg);

    Ok(())
}

/// The `fail:` label of `fxp_attach`: the control structure's DMA state and the receive
/// buffers made so far are released.
fn fxp_attach_fail(sc: &'static FxpSoftc) -> Result<(), Errno> {
    printf(format_args!("{}: Failed to malloc memory\n", sc.devname()));
    fxp_attach_free_ctrl(sc);
    let mut m = sc.rfa_headm.get();
    while let Some(mb) = m {
        let rxmap = fxp_rxmap_of(mb);
        bus_dmamap_unload(sc.dmat(), rxmap);
        sc.rxmap_put(rxmap);
        m = m_free(mb);
    }
    Err(Errno::ENOMEM)
}

/// `fxp_autosize_eeprom`: figure out EEPROM size.
///
/// 559's can have either 64-word or 256-word EEPROMs, the 558 datasheet only talks about
/// 64-word EEPROMs, and the 557 datasheet talks about the existence of 16 to 256 word
/// EEPROMs.
///
/// The only known sizes are 64 and 256, where the 256 version is used by CardBus cards to
/// store CIS information.
///
/// The address is shifted in msb-to-lsb, and after the last address-bit the EEPROM is
/// supposed to output a `dummy zero' bit, after which follows the actual data. We try to
/// detect this zero, by probing the data-out bit in the EEPROM control register just after
/// having shifted in a bit. If the bit is zero, we assume we've shifted enough address bits.
/// The data-out should be tri-state, before this, which should translate to a logical one.
pub fn fxp_autosize_eeprom(sc: &FxpSoftc) {
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
    // Shift in read opcode.
    for x in (1..=3).rev() {
        let reg = if FXP_EEPROM_OPC_READ & (1 << (x - 1)) != 0 {
            FXP_EEPROM_EECS | FXP_EEPROM_EEDI
        } else {
            FXP_EEPROM_EECS
        };
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg | FXP_EEPROM_EESK);
        delay(4);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
        delay(4);
    }
    // Shift in address.
    // Wait for the dummy zero following a correct address shift.
    let mut x = 1;
    while x <= 8 {
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS | FXP_EEPROM_EESK);
        delay(4);
        if sc.csr_read_2(FXP_CSR_EEPROMCONTROL) & FXP_EEPROM_EEDO == 0 {
            break;
        }
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
        delay(4);
        x += 1;
    }
    sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
    delay(4);
    sc.eeprom_size.set(x);
}

/// `fxp_read_eeprom`: read from the serial EEPROM. Basically, you manually shift in the read
/// opcode (one bit at a time) and then shift in the address, and then you shift out the data
/// (all of this one bit at a time). The word size is 16 bits, so you have to provide the
/// address for every 16 bits of data.
pub fn fxp_read_eeprom(sc: &FxpSoftc, data: &mut [u16], offset: i32, words: i32) {
    for i in 0..words {
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, FXP_EEPROM_EECS);
        // Shift in read opcode.
        for x in (1..=3).rev() {
            let reg = if FXP_EEPROM_OPC_READ & (1 << (x - 1)) != 0 {
                FXP_EEPROM_EECS | FXP_EEPROM_EEDI
            } else {
                FXP_EEPROM_EECS
            };
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg | FXP_EEPROM_EESK);
            delay(4);
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
            delay(4);
        }
        // Shift in address.
        for x in (1..=sc.eeprom_size.get()).rev() {
            let reg = if (i + offset) & (1 << (x - 1)) != 0 {
                FXP_EEPROM_EECS | FXP_EEPROM_EEDI
            } else {
                FXP_EEPROM_EECS
            };
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg | FXP_EEPROM_EESK);
            delay(4);
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
            delay(4);
        }
        let reg = FXP_EEPROM_EECS;
        let mut word: u16 = 0;
        // Shift out data.
        for x in (1..=16).rev() {
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg | FXP_EEPROM_EESK);
            delay(4);
            if sc.csr_read_2(FXP_CSR_EEPROMCONTROL) & FXP_EEPROM_EEDO != 0 {
                word |= 1 << (x - 1);
            }
            sc.csr_write_2(FXP_CSR_EEPROMCONTROL, reg);
            delay(4);
        }
        data[i as usize] = u16::from_le(word);
        sc.csr_write_2(FXP_CSR_EEPROMCONTROL, 0);
        delay(4);
    }
}

/// `fxp_start`: start packet transmission on the interface.
pub fn fxp_start(ifp: &'static Ifnet) {
    let sc = fxp_softc(ifp);
    let mut txs = sc.sc_cbt_prod.get();
    let mut cnt = sc.sc_cbt_cnt.get();

    if ifp.if_flags.get() & IFF_RUNNING == 0 || ifq_is_oactive(&ifp.if_snd) {
        return;
    }

    let dmat = sc.dmat();
    let ctrl = sc.ctrl_dma();

    loop {
        if cnt >= (FXP_NTXCB as i32 - 2) {
            ifq_set_oactive(&ifp.if_snd);
            break;
        }

        txs = fxp_txs_next(txs);

        let Some(m0) = ifq_dequeue(&ifp.if_snd) else {
            break;
        };

        let Some(map) = sc.txs[txs].tx_map.get() else {
            panic(format_args!("{}: no tx map {txs}", sc.devname()));
        };
        // SAFETY: the mbuf stays the slot's (`tx_mbuf`) until `fxp_intr` or `fxp_stop`
        // unload the map before freeing it.
        let load = || unsafe { bus_dmamap_load_mbuf(dmat, map, m0, BUS_DMA_NOWAIT) };
        match load() {
            Ok(()) => {}
            Err(Errno::EFBIG) if m_defrag(m0, M_DONTWAIT).is_ok() && load().is_ok() => {}
            Err(_) => {
                ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
                m_freem(m0);
                // try next packet
                continue;
            }
        }

        sc.txs[txs].tx_mbuf.set(Some(m0));

        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap(bpf, m0, BPF_DIRECTION_OUT);
        }

        fxp_mbuf_sync(sc, map, BUS_DMASYNC_PREWRITE);

        let off = fxp_ctrl_tx_cb_off(txs);
        let nsegs = map.dm_nsegs.get().max(0) as usize;
        ctrl.wr8(off + offset_of!(FxpCbTx, tbd_number), nsegs as u8);
        ctrl.wr16(off + offset_of!(FxpCbTx, cb_status), 0);
        ctrl.wr16(
            off + offset_of!(FxpCbTx, cb_command),
            (FXP_CB_COMMAND_XMIT | FXP_CB_COMMAND_SF).to_le(),
        );
        ctrl.wr8(
            off + offset_of!(FxpCbTx, tx_threshold),
            TX_THRESHOLD.load(Ordering::Relaxed) as u8,
        );
        for (seg, s) in map.dm_segs().iter().take(nsegs).enumerate() {
            let s = s.get();
            let t = fxp_ctrl_tbd_off(txs) + seg * size_of::<FxpTbd>();
            ctrl.wr32(t + offset_of!(FxpTbd, tb_addr), (s.ds_addr as u32).to_le());
            ctrl.wr32(t + offset_of!(FxpTbd, tb_size), (s.ds_len as u32).to_le());
        }
        sc.txcb_sync(txs, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

        cnt += 1;
        sc.sc_cbt_prod.set(txs);
    }

    if cnt != sc.sc_cbt_cnt.get() {
        // We enqueued at least one.
        ifp.if_timer.set(5);

        txs = fxp_txs_next(sc.sc_cbt_prod.get());
        sc.sc_cbt_prod.set(txs);
        ctrl.wr16(
            fxp_ctrl_tx_cb_off(txs) + offset_of!(FxpCbTx, cb_command),
            (FXP_CB_COMMAND_I | FXP_CB_COMMAND_NOP | FXP_CB_COMMAND_S).to_le(),
        );
        sc.txcb_sync(txs, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

        let prev = sc.sc_cbt_prev.get();
        sc.txcb_sync(prev, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
        let cmd_off = fxp_ctrl_tx_cb_off(prev) + offset_of!(FxpCbTx, cb_command);
        ctrl.wr16(
            cmd_off,
            ctrl.rd16(cmd_off) & (!(FXP_CB_COMMAND_S | FXP_CB_COMMAND_I)).to_le(),
        );
        sc.txcb_sync(prev, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

        sc.sc_cbt_prev.set(txs);

        fxp_scb_wait(sc);
        fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_RESUME);

        sc.sc_cbt_cnt.set(cnt + 1);
    }
}

/// `fxp_intr`: process interface interrupts.
pub fn fxp_intr(arg: *mut c_void) -> i32 {
    let sc = fxp_arg(arg);
    let ifp = fxp_ifp(sc);
    let ml = MbufList::new();
    let mut claimed = 0;

    // If the interface isn't running, don't try to service the interrupt.. just ack it and
    // bail.
    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        let statack = sc.csr_read_2(FXP_CSR_SCB_STATUS);
        if statack != 0 {
            claimed = 1;
            sc.csr_write_2(FXP_CSR_SCB_STATUS, statack & FXP_SCB_STATACK_MASK);
        }
        return claimed;
    }

    let dmat = sc.dmat();
    let ctrl = sc.ctrl_dma();

    loop {
        let statack = sc.csr_read_2(FXP_CSR_SCB_STATUS);
        if statack & FXP_SCB_STATACK_MASK == 0 {
            break;
        }
        claimed = 1;
        let mut rnr = statack & (FXP_SCB_STATACK_RNR | FXP_SCB_STATACK_SWI) != 0;
        // First ACK all the interrupts in this pass.
        sc.csr_write_2(FXP_CSR_SCB_STATUS, statack & FXP_SCB_STATACK_MASK);

        // Free any finished transmit mbuf chains.
        if statack & (FXP_SCB_STATACK_CXTNO | FXP_SCB_STATACK_CNA) != 0 {
            let mut txcnt = sc.sc_cbt_cnt.get();
            let mut txs = sc.sc_cbt_cons.get();

            sc.txcb_sync(txs, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);

            // (The C also continues while `cb_command & htole16(FXP_CB_COMMAND_NOP)`;
            // FXP_CB_COMMAND_NOP is 0, so that term is never true.)
            while txcnt > 0
                && ctrl.rd16(fxp_ctrl_tx_cb_off(txs) + offset_of!(FxpCbTx, cb_status))
                    & FXP_CB_STATUS_C.to_le()
                    != 0
            {
                if let Some(m) = sc.txs[txs].tx_mbuf.take() {
                    if let Some(map) = sc.txs[txs].tx_map.get() {
                        fxp_mbuf_sync(sc, map, BUS_DMASYNC_POSTWRITE);
                        bus_dmamap_unload(dmat, map);
                    }
                    m_freem(m);
                }
                txcnt -= 1;
                txs = fxp_txs_next(txs);
                sc.txcb_sync(txs, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
            }
            sc.sc_cbt_cnt.set(txcnt);
            // Did we transmit any packets?
            if sc.sc_cbt_cons.get() != txs {
                ifq_clr_oactive(&ifp.if_snd);
            }
            ifp.if_timer
                .set(if sc.sc_cbt_cnt.get() != 0 { 5 } else { 0 });
            sc.sc_cbt_cons.set(txs);

            if !ifq_empty(&ifp.if_snd) {
                // Try to start more packets transmitting.
                fxp_start(ifp);
            }
        }
        // Process receiver interrupts. If a Receive Unit not ready (RNR) condition exists,
        // get whatever packets we can and re-start the receiver.
        if statack & (FXP_SCB_STATACK_FR | FXP_SCB_STATACK_RNR | FXP_SCB_STATACK_SWI) != 0 {
            // rcvloop:
            while let Some(m) = sc.rfa_headm.get() {
                let rfa = fxp_rfa(m);
                let rxmap = fxp_rxmap_of(m);
                bus_dmamap_sync(
                    dmat,
                    rxmap,
                    0,
                    MCLBYTES,
                    BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
                );

                let status = rfa.rd16(FXP_RFA_OFF_RFA_STATUS);
                if status & FXP_RFA_STATUS_C.to_le() == 0 {
                    break;
                }
                if status & FXP_RFA_STATUS_RNR.to_le() != 0 {
                    rnr = true;
                }

                // Remove first packet from the chain.
                sc.rfa_headm.set(m.m_next().get());
                m.m_next().set(None);

                // Add a new buffer to the receive chain. If this fails, the old buffer is
                // recycled instead.
                if fxp_add_rfabuf(sc, Some(m)).is_ok() {
                    let total_len =
                        u16::from_le(rfa.rd16(FXP_RFA_OFF_ACTUAL_SIZE)) & (MCLBYTES as u16 - 1);
                    if usize::from(total_len) < ETHER_HDR_LEN {
                        m_freem(m);
                        continue;
                    }
                    if rfa.rd16(FXP_RFA_OFF_RFA_STATUS) & FXP_RFA_STATUS_CRC.to_le() != 0 {
                        m_freem(m);
                        continue;
                    }

                    m.m_pkthdr().len.set(i32::from(total_len));
                    m.m_len().set(u32::from(total_len));
                    ml_enqueue(&ml, m);
                }
            }
        }
        if rnr && let Some(head) = sc.rfa_headm.get() {
            let rxmap = fxp_rxmap_of(head);
            fxp_scb_wait(sc);
            sc.csr_write_4(
                FXP_CSR_SCB_GENERAL,
                (rxmap.dm_segs()[0].get().ds_addr + RFA_ALIGNMENT_FUDGE) as u32,
            );
            fxp_scb_cmd(sc, FXP_SCB_COMMAND_RU_START);
        }
    }

    if_input(ifp, &ml);

    claimed
}

/// `fxp_stats_update`: update packet in/out/collision statistics. The i82557 doesn't allow
/// you to access these counters without doing a fairly expensive DMA to get _all_ of the
/// statistics it maintains, so we do this operation here only once per second. The
/// statistics counters in the kernel are updated from the previous dump-stats DMA and then
/// a new dump-stats DMA is started. The on-chip counters are zeroed when the DMA completes.
/// If we can't start the DMA immediately, we don't wait - we just prepare to read them again
/// next time.
pub fn fxp_stats_update(arg: *mut c_void) {
    let sc = fxp_arg(arg);
    let ifp = fxp_ifp(sc);
    let ctrl = sc.ctrl_dma();
    let st = FXP_CTRL_STATS_OFF;

    sc.stats_sync(BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
    let sp: FxpStats = ctrl.read(st);
    let coll = ifp.if_collisions();
    coll.set(coll.get() + u64::from(u32::from_le(sp.tx_total_collisions)));
    if sp.rx_good != 0 {
        sc.rx_idle_secs.set(0);
    } else if sc.sc_flags.get() & FXPF_RECV_WORKAROUND != 0 {
        sc.rx_idle_secs.set(sc.rx_idle_secs.get() + 1);
    }
    let ierr = ifp.if_ierrors();
    ierr.set(
        ierr.get()
            + u64::from(u32::from_le(sp.rx_crc_errors))
            + u64::from(u32::from_le(sp.rx_alignment_errors))
            + u64::from(u32::from_le(sp.rx_rnr_errors))
            + u64::from(u32::from_le(sp.rx_overrun_errors)),
    );
    // If any transmit underruns occurred, bump up the transmit threshold by another 512
    // bytes (64 * 8).
    if sp.tx_underruns != 0 {
        let oerr = ifp.if_oerrors();
        oerr.set(oerr.get() + u64::from(u32::from_le(sp.tx_underruns)));
        if TX_THRESHOLD.load(Ordering::Relaxed) < 192 {
            TX_THRESHOLD.fetch_add(64, Ordering::Relaxed);
        }
    }
    let s = splnet();
    // If we haven't received any packets in FXP_MAX_RX_IDLE seconds, then assume the
    // receiver has locked up and attempt to clear the condition by reprogramming the
    // multicast filter. This is a work-around for a bug in the 82557 where the receiver
    // locks up if it gets certain types of garbage in the synchronization bits prior to the
    // packet header. This bug is supposed to only occur in 10Mbps mode, but has been seen to
    // occur in 100Mbps mode as well (perhaps due to a 10/100 speed transition).
    if sc.rx_idle_secs.get() > FXP_MAX_RX_IDLE {
        sc.rx_idle_secs.set(0);
        fxp_init(sc);
        splx(s);
        return;
    }
    // If there is no pending command, start another stats dump. Otherwise punt for now.
    sc.stats_sync(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    if sc.csr_read_2(FXP_CSR_SCB_COMMAND) & 0xff == 0 {
        // Start another stats dump.
        fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_DUMPRESET);
    } else {
        // A previous command is still waiting to be accepted. Just zero our copy of the
        // stats and wait for the next timer event to update them.
        ctrl.wr32(st + offset_of!(FxpStats, tx_good), 0);
        ctrl.wr32(st + offset_of!(FxpStats, tx_underruns), 0);
        ctrl.wr32(st + offset_of!(FxpStats, tx_total_collisions), 0);

        ctrl.wr32(st + offset_of!(FxpStats, rx_good), 0);
        ctrl.wr32(st + offset_of!(FxpStats, rx_crc_errors), 0);
        ctrl.wr32(st + offset_of!(FxpStats, rx_alignment_errors), 0);
        ctrl.wr32(st + offset_of!(FxpStats, rx_rnr_errors), 0);
        ctrl.wr32(st + offset_of!(FxpStats, rx_overrun_errors), 0);
    }

    // Tick the MII clock.
    mii_tick(&sc.sc_mii);

    splx(s);
    // Schedule another timeout one second from now.
    timeout_add_sec(&sc.stats_update_to, 1);
}

/// `fxp_detach`.
pub fn fxp_detach(sc: &'static FxpSoftc) {
    let ifp = fxp_ifp(sc);

    // Get rid of our timeouts and mbufs
    fxp_stop(sc, true, true);

    // Detach any PHYs we might have.
    if sc.sc_mii.mii_phys.first().is_some() {
        mii_detach(&sc.sc_mii, MII_PHY_ANY, MII_OFFSET_ANY);
    }

    // Delete any remaining media.
    ifmedia_delete_instance(&sc.sc_mii.mii_media, IFM_INST_ANY);

    ether_ifdetach(ifp);
    if_detach(ifp);

    // SMALL_KERNEL is not defined.
    if let Some(buf) = NonNull::new(sc.sc_ucodebuf.get()) {
        sc.sc_ucodebuf.set(ptr::null_mut());
        free(buf, M_DEVBUF, sc.sc_ucodelen.get());
    }
}

/// `fxp_stop`: stop the interface. Cancels the statistics updater and resets the interface.
pub fn fxp_stop(sc: &'static FxpSoftc, drain: bool, softonly: bool) {
    let ifp = fxp_ifp(sc);
    let dmat = sc.dmat();

    // Cancel stats updater.
    timeout_del(&sc.stats_update_to);

    // Turn down interface (done early to avoid bad interactions between panics, and the
    // watchdog timer)
    ifp.if_timer.set(0);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    if !softonly {
        mii_down(&sc.sc_mii);
    }

    // Issue software reset.
    if !softonly {
        sc.csr_write_4(FXP_CSR_PORT, FXP_PORT_SELECTIVE_RESET);
        delay(10);
    }

    // Release any xmit buffers.
    for txs in &sc.txs {
        if let Some(m) = txs.tx_mbuf.take() {
            if let Some(map) = txs.tx_map.get() {
                bus_dmamap_unload(dmat, map);
            }
            m_freem(m);
        }
    }
    sc.sc_cbt_cnt.set(0);

    if drain {
        // Free all the receive buffers then reallocate/reinitialize
        let mut m = sc.rfa_headm.get();
        while let Some(mb) = m {
            let rxmap = fxp_rxmap_of(mb);
            bus_dmamap_unload(dmat, rxmap);
            sc.rxmap_put(rxmap);
            m = m_free(mb);
            sc.rx_bufs.set(sc.rx_bufs.get() - 1);
        }
        sc.rfa_headm.set(None);
        sc.rfa_tailm.set(None);
        for _ in 0..FXP_NRFABUFS_MIN {
            if fxp_add_rfabuf(sc, None).is_err() {
                // This "can't happen" - we're at splnet() and we just freed all the buffers
                // we need above.
                panic(format_args!("fxp_stop: no buffers!"));
            }
            sc.rx_bufs.set(sc.rx_bufs.get() + 1);
        }
    }
}

/// `fxp_watchdog`: watchdog/transmission transmit timeout handler. Called when a
/// transmission is started on the interface, but no interrupt is received before the
/// timeout. This usually indicates that the card has wedged for some reason.
pub fn fxp_watchdog(ifp: &'static Ifnet) {
    let sc = fxp_softc(ifp);

    log(LOG_ERR, format_args!("{}: device timeout\n", sc.devname()));
    ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);

    fxp_init(sc);
}

/// `fxp_scb_cmd`: submit a command to the i82557.
pub fn fxp_scb_cmd(sc: &FxpSoftc, cmd: u16) {
    sc.csr_write_2(FXP_CSR_SCB_COMMAND, cmd);
}

/// Waits for the command block in the union at `FXP_CTRL_U_OFF` to complete
/// (`do { DELAY(1); SYNC } while (!(status & C) && i--)`), true if it did.
fn fxp_wait_cb(sc: &FxpSoftc, sync: fn(&FxpSoftc, i32)) -> bool {
    let ctrl = sc.ctrl_dma();
    let mut i = FXP_CMD_TMO;
    loop {
        delay(1);
        sync(sc, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
        if ctrl.rd16(FXP_CTRL_U_OFF) & FXP_CB_STATUS_C.to_le() != 0 {
            break;
        }
        let more = i != 0;
        i -= 1;
        if !more {
            break;
        }
    }
    sync(sc, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
    ctrl.rd16(FXP_CTRL_U_OFF) & FXP_CB_STATUS_C.to_le() != 0
}

/// `fxp_init`.
pub fn fxp_init(sc: &'static FxpSoftc) {
    let ifp = fxp_ifp(sc);
    let dmat = sc.dmat();

    splassert(IPL_NET, "fxp_init");

    // Cancel any pending I/O
    fxp_stop(sc, false, false);

    // Initialize base of CBL and RFA memory. Loading with zero sets it up for regular linear
    // addressing.
    fxp_scb_wait(sc);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, 0);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_BASE);

    fxp_scb_wait(sc);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, 0);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_RU_BASE);

    // SMALL_KERNEL is not defined.
    fxp_load_ucode(sc);

    // Once through to set flags
    fxp_mc_setup(sc, false);

    // In order to support receiving 802.1Q VLAN frames, we have to enable "save bad frames",
    // since they are 4 bytes larger than the normal Ethernet maximum frame length. On i82558
    // and later, we have a better mechanism for this.
    let (save_bf, lrxen) = if sc.sc_revision.get() >= FXP_REV_82558_A4 {
        (false, true)
    } else {
        (true, false)
    };

    // Initialize base of dump-stats buffer.
    fxp_scb_wait(sc);
    sc.csr_write_4(
        FXP_CSR_SCB_GENERAL,
        (sc.cb_base() + FXP_CTRL_STATS_OFF) as u32,
    );
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_DUMP_ADR);

    let ctrl = sc.ctrl_dma();
    // The configuration template; this is kind of disgusting, but there are a bunch of must
    // be zero and must be one bits in this structure and this is the easiest way to
    // initialize them all to proper values.
    let mut cbp = fxp_cb_config_template();

    let prm = ifp.if_flags.get() & IFF_PROMISC != 0;
    let allm = ifp.if_flags.get() & IFF_ALLMULTI != 0;

    cbp.cb_command = (FXP_CB_COMMAND_CONFIG | FXP_CB_COMMAND_EL).to_le();

    if allm && !prm {
        cbp.mc_all |= 0x08; // accept all multicasts
    } else {
        cbp.mc_all &= !0x08; // reject all multicasts
    }

    if prm {
        cbp.promiscuous |= 1; // promiscuous mode
        cbp.ctrl2 &= !0x01; // save short packets
        cbp.stripping &= !0x01; // don't truncate rx packets
    } else {
        cbp.promiscuous &= !1; // no promiscuous mode
        cbp.ctrl2 |= 0x01; // discard short packets
        cbp.stripping |= 0x01; // truncate rx packets
    }

    if prm || save_bf {
        cbp.ctrl1 |= 0x80; // save bad frames
    } else {
        cbp.ctrl1 &= !0x80; // discard bad frames
    }

    if sc.sc_flags.get() & FXPF_MWI_ENABLE != 0 {
        cbp.ctrl0 |= 0x01; // enable PCI MWI command
    }

    if sc.phy_10mbps_only.get() == 0 {
        // interface mode
        cbp.mediatype |= 0x01;
    } else {
        cbp.mediatype &= !0x01;
    }

    if lrxen {
        // long packets
        cbp.stripping |= 0x08;
    } else {
        cbp.stripping &= !0x08;
    }

    cbp.tx_dma_bytecount = 0; // (no) tx DMA max, dma_dce = 0 ???
    cbp.ctrl1 |= 0x08; // ci_int = 1
    cbp.ctrl3 |= 0x08; // nsai
    cbp.fifo_limit = 0x08; // tx and rx fifo limit
    cbp.fdx_pin |= 0x80; // Enable full duplex setting by pin

    // Start the config command/DMA.
    fxp_scb_wait(sc);
    ctrl.write(FXP_CTRL_U_OFF, cbp);
    sc.cfg_sync(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, (sc.cb_base() + FXP_CTRL_U_OFF) as u32);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_START);
    // ...and wait for it to complete.
    if !fxp_wait_cb(sc, FxpSoftc::cfg_sync) {
        printf(format_args!("{}: config command timeout\n", sc.devname()));
        return;
    }

    // Now initialize the station address.
    let mut ias = FxpCbIas {
        cb_status: 0u16.to_le(),
        cb_command: (FXP_CB_COMMAND_IAS | FXP_CB_COMMAND_EL).to_le(),
        link_addr: 0xffff_ffffu32.to_le(),
        macaddr: [0; 6],
    };
    ias.macaddr = sc.sc_arpcom.ac_enaddr.get();

    // Start the IAS (Individual Address Setup) command/DMA.
    fxp_scb_wait(sc);
    ctrl.write(FXP_CTRL_U_OFF, ias);
    sc.ias_sync(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, (sc.cb_base() + FXP_CTRL_U_OFF) as u32);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_START);
    // ...and wait for it to complete.
    if !fxp_wait_cb(sc, FxpSoftc::ias_sync) {
        printf(format_args!("{}: IAS command timeout\n", sc.devname()));
        return;
    }

    // Again, this time really upload the multicast addresses
    fxp_mc_setup(sc, true);

    // Initialize transmit control block (TxCB) list.
    let zero = FxpCbTx {
        cb_status: 0,
        cb_command: 0,
        link_addr: 0,
        tbd_array_addr: 0,
        byte_count: 0,
        tx_threshold: 0,
        tbd_number: 0,
        tbd: [FxpTbd::default(); FXP_NTXSEG],
    };
    let base = sc.cb_base();
    for i in 0..FXP_NTXCB {
        let off = fxp_ctrl_tx_cb_off(i);
        ctrl.write(off, zero);
        ctrl.wr16(
            off + offset_of!(FxpCbTx, cb_command),
            FXP_CB_COMMAND_NOP.to_le(),
        );
        ctrl.wr32(
            off + offset_of!(FxpCbTx, link_addr),
            ((base + fxp_ctrl_tx_cb_off(fxp_txs_next(i))) as u32).to_le(),
        );
        ctrl.wr32(
            off + offset_of!(FxpCbTx, tbd_array_addr),
            ((base + fxp_ctrl_tbd_off(i)) as u32).to_le(),
        );
    }
    // Set the suspend flag on the first TxCB and start the control unit. It will execute
    // the NOP and then suspend.
    sc.sc_cbt_prev.set(0);
    sc.sc_cbt_prod.set(0);
    sc.sc_cbt_cons.set(0);
    sc.sc_cbt_cnt.set(1);
    ctrl.wr16(
        fxp_ctrl_tx_cb_off(0) + offset_of!(FxpCbTx, cb_command),
        (FXP_CB_COMMAND_NOP | FXP_CB_COMMAND_S | FXP_CB_COMMAND_I).to_le(),
    );
    sc.cb_sync_all(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

    fxp_scb_wait(sc);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, (base + fxp_ctrl_tx_cb_off(0)) as u32);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_START);

    // Initialize receiver buffer area - RFA.
    let bufs = if ifp.if_flags.get() & IFF_UP != 0 {
        FXP_NRFABUFS_MAX as i32
    } else {
        FXP_NRFABUFS_MIN as i32
    };
    if sc.rx_bufs.get() > bufs {
        while let Some(head) = sc.rfa_headm.get() {
            if sc.rx_bufs.get() <= bufs {
                break;
            }
            let rxmap = fxp_rxmap_of(head);
            bus_dmamap_unload(dmat, rxmap);
            sc.rxmap_put(rxmap);
            sc.rfa_headm.set(m_free(head));
            sc.rx_bufs.set(sc.rx_bufs.get() - 1);
        }
    } else if sc.rx_bufs.get() < bufs {
        let tmp_rx_bufs = sc.rx_bufs.get();
        for i in sc.rx_bufs.get()..bufs {
            match bus_dmamap_create(dmat, MCLBYTES, 1, MCLBYTES, 0, 0) {
                Ok(m) => sc.sc_rxmaps[i as usize].set(Some(m)),
                Err(err) => {
                    printf(format_args!(
                        "{}: unable to create rx dma map {}, error {}\n",
                        sc.devname(),
                        i,
                        err as i32
                    ));
                    break;
                }
            }
            sc.rx_bufs.set(sc.rx_bufs.get() + 1);
        }
        for _ in tmp_rx_bufs..sc.rx_bufs.get() {
            if fxp_add_rfabuf(sc, None).is_err() {
                break;
            }
        }
    }
    fxp_scb_wait(sc);

    // Set current media.
    let _ = mii_mediachg(&sc.sc_mii);

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    // Request a software generated interrupt that will be used to (re)start the RU
    // processing. If we direct the chip to start receiving from the start of queue now,
    // instead of letting the interrupt handler first process all received packets, we run the
    // risk of having it overwrite mbuf clusters while they are being processed or after they
    // have been returned to the pool.
    sc.csr_write_2(
        FXP_CSR_SCB_COMMAND,
        sc.csr_read_2(FXP_CSR_SCB_COMMAND) | FXP_SCB_INTRCNTL_REQUEST_SWI,
    );

    // Start stats updater.
    timeout_add_sec(&sc.stats_update_to, 1);
}

/// `fxp_mediachange`: change media according to request.
pub fn fxp_mediachange(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = fxp_softc(ifp);
    let mii = &sc.sc_mii;

    if mii.mii_instance.get() != 0 {
        for miisc in mii.mii_phys.iter() {
            // SAFETY: the PHYs on the list are attached devices, never freed while linked.
            let miisc = unsafe { &*ptr::from_ref(miisc) };
            mii_phy_reset(miisc);
        }
    }
    let _ = mii_mediachg(mii);
    Ok(())
}

/// `fxp_mediastatus`: notify the world which media we're using.
pub fn fxp_mediastatus(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = fxp_softc(ifp);

    mii_pollstat(&sc.sc_mii);
    ifmr.ifm_status = sc.sc_mii.mii_media_status.get();
    ifmr.ifm_active = sc.sc_mii.mii_media_active.get();
}

/// `fxp_add_rfabuf`: add a buffer to the end of the RFA buffer list. `Ok` if successful,
/// `Err` for failure (the C's 1). A failure results in adding the `oldm` (if any) on to the
/// end of the list - tossing out its old contents and recycling it. The RFA struct is stuck
/// at the beginning of mbuf cluster and the data pointer is fixed up to point just past it.
pub fn fxp_add_rfabuf(sc: &FxpSoftc, oldm: Option<&'static Mbuf>) -> Result<(), Errno> {
    let newm = match m_gethdr(M_DONTWAIT, MT_DATA) {
        Some(m) => match m_clget(Some(m), M_DONTWAIT, MCLBYTES as u32) {
            Some(m) if m.m_flags().get() & M_EXT != 0 => Some(m),
            _ => {
                m_freem(m);
                None
            }
        },
        None => None,
    };

    let (m, rxmap) = match (newm, oldm) {
        (None, None) => return Err(Errno::ENOBUFS),
        (None, Some(old)) => {
            old.m_data().set(old.m_ext().ext_buf.get());
            (old, fxp_rxmap_of(old))
        }
        (Some(m), None) => {
            let rxmap = sc.rxmap_get();
            fxp_rxmap_store(m, rxmap);
            fxp_rxmap_load(sc, rxmap, m);
            (m, rxmap)
        }
        (Some(m), Some(old)) => {
            let rxmap = fxp_rxmap_of(old);
            bus_dmamap_unload(sc.dmat(), rxmap);
            fxp_rxmap_load(sc, rxmap, m);
            fxp_rxmap_store(m, rxmap);
            (m, rxmap)
        }
    };

    // Move the data pointer up so that the incoming data packet will be 32-bit aligned.
    m.m_data()
        .set(m.m_data().get().wrapping_add(RFA_ALIGNMENT_FUDGE));

    // Get a pointer to the base of the mbuf cluster and move data start past it.
    let rfa = fxp_rfa(m);
    m.m_data()
        .set(m.m_data().get().wrapping_add(size_of::<FxpRfa>()));
    rfa.wr16(
        FXP_RFA_OFF_SIZE,
        ((MCLBYTES - size_of::<FxpRfa>() - RFA_ALIGNMENT_FUDGE) as u16).to_le(),
    );

    // Initialize the rest of the RFA. Note that since the RFA is misaligned, we cannot store
    // values directly. Instead, we use an optimized, inline copy.
    rfa.wr16(FXP_RFA_OFF_RFA_STATUS, 0);
    rfa.wr16(FXP_RFA_OFF_RFA_CONTROL, FXP_RFA_CONTROL_EL.to_le());
    rfa.wr16(FXP_RFA_OFF_ACTUAL_SIZE, 0);

    fxp_lwcopy(0xffff_ffff, &rfa, FXP_RFA_OFF_LINK_ADDR);
    fxp_lwcopy(0xffff_ffff, &rfa, FXP_RFA_OFF_RBD_ADDR);

    bus_dmamap_sync(
        sc.dmat(),
        rxmap,
        0,
        MCLBYTES,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    // If there are other buffers already on the list, attach this one to the end by fixing
    // up the tail to point to this one.
    if let Some(tail) = sc.rfa_headm.get().and(sc.rfa_tailm.get()) {
        tail.m_next().set(Some(m));
        let v = ((rxmap.dm_segs()[0].get().ds_addr + RFA_ALIGNMENT_FUDGE) as u32).to_le();
        let trfa = fxp_rfa(tail);
        fxp_lwcopy(v, &trfa, FXP_RFA_OFF_LINK_ADDR);
        trfa.wr16(
            FXP_RFA_OFF_RFA_CONTROL,
            trfa.rd16(FXP_RFA_OFF_RFA_CONTROL) & (!FXP_RFA_CONTROL_EL).to_le(),
        );
        // XXX we only need to sync the control struct
        bus_dmamap_sync(
            sc.dmat(),
            fxp_rxmap_of(tail),
            0,
            MCLBYTES,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
    } else {
        sc.rfa_headm.set(Some(m));
    }

    sc.rfa_tailm.set(Some(m));

    // `m == oldm`: the old buffer was recycled.
    if oldm.is_some_and(|o| ptr::eq(o, m)) {
        Err(Errno::ENOBUFS)
    } else {
        Ok(())
    }
}

/// `fxp_mdi_read`.
pub fn fxp_mdi_read(self_: &Device, phy: i32, reg: i32) -> i32 {
    let sc = fxp_dev_sc(self_);
    let mut count = FXP_CMD_TMO;

    sc.csr_write_4(
        FXP_CSR_MDICONTROL,
        (FXP_MDI_READ << 26) | ((reg as u32) << 16) | ((phy as u32) << 21),
    );

    let mut value;
    loop {
        value = sc.csr_read_4(FXP_CSR_MDICONTROL);
        if value & 0x1000_0000 != 0 {
            break;
        }
        let more = count != 0;
        count -= 1;
        if !more {
            break;
        }
        delay(10);
    }

    if count <= 0 {
        printf(format_args!("{}: fxp_mdi_read: timed out\n", sc.devname()));
    }

    (value & 0xffff) as i32
}

/// `fxp_statchg`: nothing to do.
pub fn fxp_statchg(_self: &Device) {}

/// `fxp_mdi_write`.
pub fn fxp_mdi_write(self_: &Device, phy: i32, reg: i32, value: i32) {
    let sc = fxp_dev_sc(self_);
    let mut count = FXP_CMD_TMO;

    sc.csr_write_4(
        FXP_CSR_MDICONTROL,
        (FXP_MDI_WRITE << 26)
            | ((reg as u32) << 16)
            | ((phy as u32) << 21)
            | (value as u32 & 0xffff),
    );

    while sc.csr_read_4(FXP_CSR_MDICONTROL) & 0x1000_0000 == 0 {
        let more = count != 0;
        count -= 1;
        if !more {
            break;
        }
        delay(10);
    }

    if count <= 0 {
        printf(format_args!("{}: fxp_mdi_write: timed out\n", sc.devname()));
    }
}

/// `fxp_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the command
/// encodes.
pub unsafe fn fxp_ioctl(ifp: &'static Ifnet, command: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = fxp_softc(ifp);
    let mut error = Ok(());

    let s = splnet();

    match command {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                fxp_init(sc);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    fxp_init(sc);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                fxp_stop(sc, true, false);
            }
        }
        SIOCSIFMEDIA | SIOCGIFMEDIA => {
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
            fxp_init(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

/// `fxp_mc_setup`: program the multicast filter.
///
/// We have an artificial restriction that the multicast setup command must be the first
/// command in the chain, so we take steps to ensure this. By requiring this, it allows us to
/// keep up the performance of the pre-initialized command ring (esp. link pointers) by not
/// actually inserting the mcsetup command in the ring - i.e. its link pointer points to the
/// TxCB ring, but the mcsetup descriptor itself is not part of it. We then can do 'CU_START'
/// on the mcsetup descriptor and have it lead into the regular TxCB ring when it completes.
///
/// This function must be called at splnet.
pub fn fxp_mc_setup(sc: &FxpSoftc, doit: bool) {
    let ifp = fxp_ifp_ref(sc);
    let ac = &sc.sc_arpcom;
    let ctrl = sc.ctrl_dma();
    let mut nmcasts: usize = 0;

    splassert(IPL_NET, "fxp_mc_setup");

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if ifp.if_flags.get() & IFF_PROMISC != 0
        || ac.ac_multirangecnt.get() > 0
        || ac.ac_multicnt.get() >= MAXMCADDR as i32
    {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
    } else {
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let mut a = [0u8; 6];
            a.copy_from_slice(&e.enm_addrlo[..ETHER_ADDR_LEN]);
            ctrl.write(
                FXP_CTRL_U_OFF + offset_of!(FxpCbMcs, mc_addr) + nmcasts * 6,
                a,
            );

            nmcasts += 1;

            enm = ether_next_multi(&mut step);
        }
    }

    if !doit {
        return;
    }

    // Initialize multicast setup descriptor.
    ctrl.wr16(
        FXP_CTRL_U_OFF + offset_of!(FxpCbMcs, cb_status),
        0u16.to_le(),
    );
    ctrl.wr16(
        FXP_CTRL_U_OFF + offset_of!(FxpCbMcs, cb_command),
        (FXP_CB_COMMAND_MCAS | FXP_CB_COMMAND_EL).to_le(),
    );
    ctrl.wr32(
        FXP_CTRL_U_OFF + offset_of!(FxpCbMcs, link_addr),
        0xffff_ffffu32.to_le(),
    );
    ctrl.wr16(
        FXP_CTRL_U_OFF + offset_of!(FxpCbMcs, mc_cnt),
        ((nmcasts * ETHER_ADDR_LEN) as u16).to_le(),
    );

    // Wait until command unit is not active. This should never be the case when nothing is
    // queued, but make sure anyway.
    let mut i = FXP_CMD_TMO;
    while sc.csr_read_2(FXP_CSR_SCB_STATUS) & FXP_SCB_CUS_MASK != FXP_SCB_CUS_IDLE {
        let more = i != 0;
        i -= 1;
        if !more {
            break;
        }
        delay(1);
    }

    if sc.csr_read_2(FXP_CSR_SCB_STATUS) & FXP_SCB_CUS_MASK != FXP_SCB_CUS_IDLE {
        printf(format_args!(
            "{}: timeout waiting for CU ready\n",
            sc.devname()
        ));
        return;
    }

    // Start the multicast setup command.
    fxp_scb_wait(sc);
    sc.mcs_sync(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, (sc.cb_base() + FXP_CTRL_U_OFF) as u32);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_START);

    if !fxp_wait_cb(sc, FxpSoftc::mcs_sync) {
        printf(format_args!(
            "{}: multicast command timeout\n",
            sc.devname()
        ));
    }
}

/// `&sc->sc_arpcom.ac_if` for a softc that is not `'static` (inside the attach chain).
fn fxp_ifp_ref(sc: &FxpSoftc) -> &Ifnet {
    &sc.sc_arpcom.ac_if
}

/// `struct ucode`: the microcode of one chip revision.
pub struct Ucode {
    /// `revision`.
    pub revision: u32,
    /// `int_delay_offset`.
    pub int_delay_offset: usize,
    /// `bundle_max_offset`.
    pub bundle_max_offset: usize,
    /// `min_size_mask_offset`.
    pub min_size_mask_offset: usize,
    /// `uname`: the file name under `/etc/firmware`.
    pub uname: &'static str,
}

/// `ucode_table[]` (without the `{ 0, ..., NULL }` terminator).
pub static UCODE_TABLE: [Ucode; 8] = [
    Ucode {
        revision: FXP_REV_82558_A4,
        int_delay_offset: D101_CPUSAVER_DWORD,
        bundle_max_offset: 0,
        min_size_mask_offset: 0,
        uname: "fxp-d101a",
    },
    Ucode {
        revision: FXP_REV_82558_B0,
        int_delay_offset: D101_CPUSAVER_DWORD,
        bundle_max_offset: 0,
        min_size_mask_offset: 0,
        uname: "fxp-d101b0",
    },
    Ucode {
        revision: FXP_REV_82559_A0,
        int_delay_offset: D101M_CPUSAVER_DWORD,
        bundle_max_offset: D101M_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D101M_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d101ma",
    },
    Ucode {
        revision: FXP_REV_82559S_A,
        int_delay_offset: D101S_CPUSAVER_DWORD,
        bundle_max_offset: D101S_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D101S_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d101s",
    },
    Ucode {
        revision: FXP_REV_82550,
        int_delay_offset: D102_B_CPUSAVER_DWORD,
        bundle_max_offset: D102_B_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D102_B_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d102",
    },
    Ucode {
        revision: FXP_REV_82550_C,
        int_delay_offset: D102_C_CPUSAVER_DWORD,
        bundle_max_offset: D102_C_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D102_C_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d102c",
    },
    Ucode {
        revision: FXP_REV_82551_F,
        int_delay_offset: D102_E_CPUSAVER_DWORD,
        bundle_max_offset: D102_E_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D102_E_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d102e",
    },
    Ucode {
        revision: FXP_REV_82551_10,
        int_delay_offset: D102_E_CPUSAVER_DWORD,
        bundle_max_offset: D102_E_CPUSAVER_BUNDLE_MAX_DWORD,
        min_size_mask_offset: D102_E_CPUSAVER_MIN_SIZE_DWORD,
        uname: "fxp-d102e",
    },
];

/// `fxp_load_ucode`.
pub fn fxp_load_ucode(sc: &FxpSoftc) {
    if sc.sc_flags.get() & FXPF_NOUCODE != 0 {
        return;
    }

    let Some(uc) = UCODE_TABLE
        .iter()
        .find(|uc| sc.sc_revision.get() == uc.revision)
    else {
        sc.sc_flags.set(sc.sc_flags.get() | FXPF_NOUCODE);
        return; // no ucode for this chip is found
    };

    if sc.sc_ucodebuf.get().is_null() {
        if sc.sc_revision.get() == FXP_REV_82550_C {
            // 82550C without the server extensions locks up with the microcode patch.
            let mut data = [0u16; 1];
            fxp_read_eeprom(sc, &mut data, FXP_EEPROM_REG_COMPAT, 1);
            if data[0] & FXP_EEPROM_REG_COMPAT_SRV == 0 {
                sc.sc_flags.set(sc.sc_flags.get() | FXPF_NOUCODE);
                return;
            }
        }

        match loadfirmware(uc.uname) {
            Ok((buf, len)) => {
                sc.sc_ucodebuf.set(buf.as_ptr());
                sc.sc_ucodelen.set(len);
            }
            Err(error) => {
                printf(format_args!(
                    "{}: error {}, could not read firmware {}\n",
                    sc.devname(),
                    error as i32,
                    uc.uname
                ));
                return;
            }
        }
    }

    // reloadit:
    if sc.sc_flags.get() & FXPF_UCODELOADED != 0 {
        return;
    }

    let ndwords = sc.sc_ucodelen.get() / size_of::<u32>();
    if ndwords > MAXUCODESIZE {
        printf(format_args!(
            "{}: firmware {} is too large ({} bytes)\n",
            sc.devname(),
            uc.uname,
            sc.sc_ucodelen.get()
        ));
        return;
    }

    let ctrl = sc.ctrl_dma();
    let u = FXP_CTRL_U_OFF;
    ctrl.wr16(u + offset_of!(FxpCbUcode, cb_status), 0);
    ctrl.wr16(
        u + offset_of!(FxpCbUcode, cb_command),
        (FXP_CB_COMMAND_UCODE | FXP_CB_COMMAND_EL).to_le(),
    );
    ctrl.wr32(u + offset_of!(FxpCbUcode, link_addr), 0xffff_ffff); // (no) next command
    let uoff = u + offset_of!(FxpCbUcode, ucode);
    for i in 0..ndwords {
        // SAFETY: `sc_ucodebuf` holds `sc_ucodelen` bytes (`loadfirmware`), `i` is below
        // their dword count; the buffer is read unaligned-safe.
        let v = unsafe { sc.sc_ucodebuf.get().cast::<u32>().add(i).read_unaligned() };
        ctrl.wr32(uoff + 4 * i, v);
    }

    // `*((u_int16_t *)&cbp->ucode[off]) = htole16(v)`: the low half of the dword.
    if uc.int_delay_offset != 0 {
        let d = sc.sc_int_delay.get();
        ctrl.wr16(uoff + 4 * uc.int_delay_offset, (d + d / 2).to_le());
    }

    if uc.bundle_max_offset != 0 {
        ctrl.wr16(
            uoff + 4 * uc.bundle_max_offset,
            sc.sc_bundle_max.get().to_le(),
        );
    }

    if uc.min_size_mask_offset != 0 {
        ctrl.wr16(
            uoff + 4 * uc.min_size_mask_offset,
            sc.sc_min_size_mask.get().to_le(),
        );
    }

    sc.ucode_sync(BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

    // Download the ucode to the chip.
    fxp_scb_wait(sc);
    sc.csr_write_4(FXP_CSR_SCB_GENERAL, (sc.cb_base() + u) as u32);
    fxp_scb_cmd(sc, FXP_SCB_COMMAND_CU_START);

    // ...and wait for it to complete.
    let mut i = FXP_CMD_TMO;
    loop {
        delay(2);
        sc.ucode_sync(BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);
        if ctrl.rd16(u) & FXP_CB_STATUS_C.to_le() != 0 {
            break;
        }
        i -= 1;
        if i == 0 {
            break;
        }
    }
    if i == 0 {
        printf(format_args!(
            "{}: timeout loading microcode\n",
            sc.devname()
        ));
        return;
    }
    sc.sc_flags.set(sc.sc_flags.get() | FXPF_UCODELOADED);

    // DEBUG is not defined: no "microcode loaded" line.
}
/* </CODE> */
