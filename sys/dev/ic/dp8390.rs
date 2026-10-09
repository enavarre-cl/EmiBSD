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
 * Device driver for National Semiconductor DS8390/WD83C690 based ethernet
 * adapters.
 *
 * Copyright (c) 1994, 1995 Charles M. Hannum.  All rights reserved.
 *
 * Copyright (C) 1993, David Greenman.  This software may be used, modified,
 * copied, distributed, and sold, in both source and binary form provided that
 * the above copyright and these terms are retained.  Under no circumstances is
 * the author responsible for the proper functioning of this software, nor does
 * the author assume any responsibility for damages incurred with its use.
 */
/* </LICENSES> */

/* <CODE> */
//! Device driver for National Semiconductor DS8390/WD83C690 based ethernet adapters
//! (`dev/ic/dp8390.c`).
//!
//! Upstream: sys/dev/ic/dp8390.c @ 3ce1f3f79392
//!
//! The bus-independent core under ne(4): `dp8390_config` makes the interface, `dp8390_init`
//! programs the chip in the order the National manual gives, packets go to the board through
//! `write_mbuf`, and `dp8390_rint` walks the receive ring in the board's memory. The bus
//! front end (`ne2000.c` here) overrides how the board's memory is reached through the
//! callbacks of [`Dp8390Softc`].
//!
//! ## Deviations
//! - `dp8390_test_mem`, `dp8390_read_hdr`, `dp8390_ring_copy` and `dp8390_write_mbuf` (the
//!   defaults for 8-bit shared memory) do the C's `bus_space_{set,read,write}_region_1` as
//!   loops over `bus_space_{read,write}_1`, which is what every architecture's region
//!   operation is for these spaces. ne(4) installs its own PIO versions.
//! - `ring_copy` takes its destination as a slice, `dp8390_enable` returns
//!   `Result<(), Errno>` and `dp8390_config` the C's 0 or 1.
//! - `NBPFILTER > 0`: `bpf_mtap` is called when `if_bpf` is set.
//! - `DIAGNOSTIC` is the `diagnostic` cargo feature; `DEBUG` (`dp8390_debug`) is not
//!   defined.
//! - The multicast filter bit of an address is [`dp8390_mcaf_bit`], so it can be tested on
//!   the host.

use core::ffi::c_void;
use core::slice;

use crate::dev::ic::dp8390reg::{
    Dp8390Ring, ED_CR_PAGE_0, ED_CR_PAGE_1, ED_CR_STA, ED_CR_STP, ED_CR_TXP, ED_DCR_FT1, ED_DCR_LS,
    ED_IMR_OVWE, ED_IMR_PRXE, ED_IMR_PTXE, ED_IMR_RXEE, ED_IMR_TXEE, ED_ISR_CNT, ED_ISR_OVW,
    ED_ISR_PRX, ED_ISR_PTX, ED_ISR_RST, ED_ISR_RXE, ED_ISR_TXE, ED_P0_BNRY, ED_P0_CNTR0,
    ED_P0_CNTR1, ED_P0_CNTR2, ED_P0_CR, ED_P0_DCR, ED_P0_IMR, ED_P0_ISR, ED_P0_NCR, ED_P0_PSTART,
    ED_P0_PSTOP, ED_P0_RBCR0, ED_P0_RBCR1, ED_P0_RCR, ED_P0_TBCR0, ED_P0_TBCR1, ED_P0_TCR,
    ED_P0_TPSR, ED_P0_TSR, ED_P1_CURR, ED_P1_MAR0, ED_P1_PAR0, ED_PAGE_MASK, ED_PAGE_SHIFT,
    ED_PAGE_SIZE, ED_RCR_AB, ED_RCR_AM, ED_RCR_AR, ED_RCR_MON, ED_RCR_PRO, ED_RCR_SEP,
    ED_RING_HDRSZ, ED_TCR_LB0, ED_TSR_ABT, ED_TXBUF_SIZE,
};
use crate::dev::ic::dp8390var::{
    DP8390_DO_AX88190_WORKAROUND, DP8390_NO_MULTI_BUFFERING, Dp8390Softc,
};
use crate::kern::subr_prf::{Str, log, panic, printf};
use crate::kern::uipc_mbuf::{m_clget, m_freem, m_get, m_gethdr, ml_enqueue};
use crate::machine::bus::{bus_space_read_1, bus_space_write_1};
use crate::machine::cpu::delay;
use crate::machine::intr::{splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IFCAP_VLAN_MTU, IFF_ALLMULTI, IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING,
    IFF_SIMPLEX, IFF_UP, IFNAMSIZ, Ifmediareq, if_attach, if_detach, if_input,
};
use crate::net::if_ethersubr::{
    ether_crc32_be, ether_ifattach, ether_ifdetach, ether_ioctl, ether_sprintf,
};
use crate::net::if_media::{
    IFM_ETHER, IFM_INST_ANY, IFM_MANUAL, IFM_NONE, ifmedia_add, ifmedia_delete_instance,
    ifmedia_init, ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{ifq_clr_oactive, ifq_dequeue, ifq_is_oactive, ifq_set_oactive};
use crate::netinet::if_ether::{
    Arpcom, ETHER_ADDR_LEN, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_MIN_LEN, EtherMultistep,
    ether_first_multi, ether_next_multi,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_DONTWAIT, M_EXT, M_PKTHDR, MCLBYTES, MHLEN, MINCLSIZE, MLEN, MT_DATA, Mbuf, MbufList,
};
use crate::sys::param::align;
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA};
use crate::sys::syslog::LOG_ERR;

/// `(struct dp8390_softc *)ifp->if_softc`.
fn dp8390_softc(ifp: &Ifnet) -> &'static Dp8390Softc {
    let p = ifp.if_softc.get().cast::<Dp8390Softc>().cast_const();
    if p.is_null() {
        panic(format_args!("dp8390: interface without its softc"));
    }
    // SAFETY: dp8390_config sets `if_softc` to its softc and installs the dp8390 functions
    // only on its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// The softc behind the C's `void *` argument (interrupt).
fn dp8390_arg(arg: *mut c_void) -> &'static Dp8390Softc {
    if arg.is_null() {
        panic(format_args!("dp8390: no softc argument"));
    }
    // SAFETY: the bus front end passes the softc, which is never freed while the device
    // exists.
    unsafe { &*arg.cast::<Dp8390Softc>() }
}

/// `dp8390_media_init`: standard media init routine for the dp8390.
pub fn dp8390_media_init(sc: &'static Dp8390Softc) {
    ifmedia_init(sc.sc_media(), 0, dp8390_mediachange, dp8390_mediastatus);
    ifmedia_add(
        sc.sc_media(),
        IFM_ETHER | IFM_MANUAL,
        0,
        core::ptr::null_mut(),
    );
    ifmedia_set(sc.sc_media(), IFM_ETHER | IFM_MANUAL);
}

/// `dp8390_config`: do bus-independent setup. Returns 0, or 1 when the board failed.
pub fn dp8390_config(sc: &'static Dp8390Softc) -> i32 {
    let ifp = sc.ifp();

    if sc.test_mem.get().is_none() {
        sc.test_mem.set(Some(dp8390_test_mem));
    }

    // Allocate one xmit buffer if < 16k, two buffers otherwise.
    if sc.mem_size.get() < 16384 || sc.sc_flags.get() & DP8390_NO_MULTI_BUFFERING != 0 {
        sc.txb_cnt.set(1);
    } else if sc.mem_size.get() < 8192 * 3 {
        sc.txb_cnt.set(2);
    } else {
        sc.txb_cnt.set(3);
    }

    let tx_page_start = (sc.mem_start.get() >> ED_PAGE_SHIFT) as u16;
    sc.tx_page_start.set(tx_page_start);
    sc.rec_page_start
        .set(tx_page_start + sc.txb_cnt.get() * ED_TXBUF_SIZE as u16);
    sc.rec_page_stop
        .set(tx_page_start + (sc.mem_size.get() >> ED_PAGE_SHIFT) as u16);
    sc.mem_ring.set(
        sc.mem_start.get() + ((sc.txb_cnt.get() as i32 * ED_TXBUF_SIZE as i32) << ED_PAGE_SHIFT),
    );
    sc.mem_end.set(sc.mem_start.get() + sc.mem_size.get());

    // Now zero memory and verify that it is clear.
    if sc.test_mem.get().is_some_and(|test_mem| test_mem(sc) != 0) {
        return 1;
    }

    // Set interface to stopped condition (reset).
    dp8390_stop(sc);

    // Initialize ifnet structure.
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let n = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);
    ifp.if_softc.set(core::ptr::from_ref(sc).cast_mut().cast());
    ifp.if_start.set(Some(dp8390_start));
    ifp.if_ioctl.set(Some(dp8390_ioctl));
    if ifp.if_watchdog.get().is_none() {
        ifp.if_watchdog.set(Some(dp8390_watchdog));
    }
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);

    ifp.if_capabilities.set(IFCAP_VLAN_MTU);

    // Print additional info when attached.
    printf(format_args!(
        ", address {}\n",
        Str(&ether_sprintf(&sc.sc_arpcom.ac_enaddr.get()))
    ));

    // Initialize media goo.
    if let Some(media_init) = sc.sc_media_init.get() {
        media_init(sc);
    }

    // Attach the interface.
    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);

    0
}

/// `dp8390_mediachange`: media change callback.
pub fn dp8390_mediachange(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = dp8390_softc(ifp);

    if let Some(mediachange) = sc.sc_mediachange.get() {
        return mediachange(sc);
    }

    Ok(())
}

/// `dp8390_mediastatus`: media status callback.
pub fn dp8390_mediastatus(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = dp8390_softc(ifp);

    if sc.sc_enabled.get() == 0 {
        ifmr.ifm_active = IFM_ETHER | IFM_NONE;
        ifmr.ifm_status = 0;
        return;
    }

    if let Some(mediastatus) = sc.sc_mediastatus.get() {
        mediastatus(sc, ifmr);
    }
}

/// `dp8390_reset`: reset interface.
pub fn dp8390_reset(sc: &'static Dp8390Softc) {
    let s = splnet();
    dp8390_stop(sc);
    dp8390_init(sc);
    splx(s);
}

/// `dp8390_stop`: take interface offline.
pub fn dp8390_stop(sc: &'static Dp8390Softc) {
    let mut n = 5000;

    // Stop everything on the interface, and select page 0 registers.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STP);
    sc.nic_barrier();

    // Wait for interface to enter stopped state, but limit # of checks to 'n' (about 5ms). It
    // shouldn't even take 5us on modern DS8390's, but just in case it's an old one.
    while sc.nic_get(ED_P0_ISR) & ED_ISR_RST == 0 {
        n -= 1;
        if n == 0 {
            break;
        }
        delay(1);
    }

    if let Some(stop_card) = sc.stop_card.get() {
        stop_card(sc);
    }
}

/// `dp8390_watchdog`: device timeout/watchdog routine. Entered if the device neglects to
/// generate an interrupt after a transmit has been started on it.
pub fn dp8390_watchdog(ifp: &'static Ifnet) {
    let sc = dp8390_softc(ifp);

    log(LOG_ERR, format_args!("{}: device timeout\n", sc.devname()));
    ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);

    dp8390_reset(sc);
}

/// `dp8390_init`: initialize device.
pub fn dp8390_init(sc: &'static Dp8390Softc) {
    let ifp = sc.ifp();
    let mut mcaf = [0u8; 8];

    // Initialize the NIC in the exact order outlined in the NS manual. This init procedure is
    // "mandatory"...don't change what or when things happen.

    // Reset transmitter flags.
    ifp.if_timer.set(0);

    sc.txb_inuse.set(0);
    sc.txb_new.set(0);
    sc.txb_next_tx.set(0);

    // Set interface for page 0, remote DMA complete, stopped.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STP);
    sc.nic_barrier();

    if sc.dcr_reg.get() & ED_DCR_LS != 0 {
        sc.nic_put(ED_P0_DCR, sc.dcr_reg.get());
    } else {
        // Set FIFO threshold to 8, No auto-init Remote DMA, byte order=80x86, byte-wide DMA
        // xfers,
        sc.nic_put(ED_P0_DCR, ED_DCR_FT1 | ED_DCR_LS);
    }

    // Clear remote byte count registers.
    sc.nic_put(ED_P0_RBCR0, 0);
    sc.nic_put(ED_P0_RBCR1, 0);

    // Tell RCR to do nothing for now.
    sc.nic_put(ED_P0_RCR, ED_RCR_MON | sc.rcr_proto.get());

    // Place NIC in internal loopback mode.
    sc.nic_put(ED_P0_TCR, ED_TCR_LB0);

    // Set lower bits of byte addressable framing to 0.
    if sc.is790.get() != 0 {
        sc.nic_put(0x09, 0);
    }

    // Initialize receive buffer ring.
    sc.nic_put(ED_P0_BNRY, sc.rec_page_start.get() as u8);
    sc.nic_put(ED_P0_PSTART, sc.rec_page_start.get() as u8);
    sc.nic_put(ED_P0_PSTOP, sc.rec_page_stop.get() as u8);

    // Enable the following interrupts: receive/transmit complete, receive/transmit error, and
    // Receiver OverWrite.
    //
    // Counter overflow and Remote DMA complete are *not* enabled.
    sc.nic_put(
        ED_P0_IMR,
        ED_IMR_PRXE | ED_IMR_PTXE | ED_IMR_RXEE | ED_IMR_TXEE | ED_IMR_OVWE,
    );

    // Clear all interrupts. A '1' in each bit position clears the corresponding flag.
    sc.nic_put(ED_P0_ISR, 0xff);

    // Program command register for page 1.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_1 | ED_CR_STP);
    sc.nic_barrier();

    // Copy out our station address.
    let enaddr = sc.sc_arpcom.ac_enaddr.get();
    for (i, b) in enaddr.iter().enumerate() {
        sc.nic_put(ED_P1_PAR0 + i, *b);
    }

    // Set multicast filter on chip.
    dp8390_getmcaf(&sc.sc_arpcom, &mut mcaf);
    for (i, b) in mcaf.iter().enumerate() {
        sc.nic_put(ED_P1_MAR0 + i, *b);
    }

    // Set current page pointer to one page after the boundary pointer, as recommended in the
    // National manual.
    sc.next_packet.set(sc.rec_page_start.get() + 1);
    sc.nic_put(ED_P1_CURR, sc.next_packet.get() as u8);

    // Program command register for page 0.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STP);
    sc.nic_barrier();

    // Accept broadcast and multicast packets by default.
    let mut i = ED_RCR_AB | ED_RCR_AM | sc.rcr_proto.get();
    if ifp.if_flags.get() & IFF_PROMISC != 0 {
        // Set promiscuous mode. Multicast filter was set earlier so that we should receive all
        // multicast packets.
        i |= ED_RCR_PRO | ED_RCR_AR | ED_RCR_SEP;
    }
    sc.nic_put(ED_P0_RCR, i);

    // Take interface out of loopback.
    sc.nic_put(ED_P0_TCR, 0);

    // Do any card-specific initialization, if applicable.
    if let Some(init_card) = sc.init_card.get() {
        init_card(sc);
    }

    // Fire up the interface.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STA);

    // Set 'running' flag, and clear output active flag.
    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    // ...and attempt to start output.
    dp8390_start(ifp);
}

/// `dp8390_xmit`: this routine actually starts the transmission on the interface.
fn dp8390_xmit(sc: &'static Dp8390Softc) {
    let ifp = sc.ifp();

    if cfg!(feature = "diagnostic") {
        if (sc.txb_next_tx.get() + sc.txb_inuse.get()) % sc.txb_cnt.get() != sc.txb_new.get() {
            panic(format_args!(
                "dp8390_xmit: desync, next_tx={} inuse={} cnt={} new={}",
                sc.txb_next_tx.get(),
                sc.txb_inuse.get(),
                sc.txb_cnt.get(),
                sc.txb_new.get()
            ));
        }

        if sc.txb_inuse.get() == 0 {
            panic(format_args!("dp8390_xmit: no packets to xmit"));
        }
    }

    let len = sc.txb_len[sc.txb_next_tx.get() as usize].get();

    // Set NIC for page 0 register access.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STA);
    sc.nic_barrier();

    // Set TX buffer start page.
    sc.nic_put(
        ED_P0_TPSR,
        (sc.tx_page_start.get() + sc.txb_next_tx.get() * ED_TXBUF_SIZE as u16) as u8,
    );

    // Set TX length.
    sc.nic_put(ED_P0_TBCR0, len as u8);
    sc.nic_put(ED_P0_TBCR1, (len >> 8) as u8);

    // Set page 0, remote DMA complete, transmit packet, and *start*.
    sc.nic_barrier();
    sc.nic_put(
        ED_P0_CR,
        sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_TXP | ED_CR_STA,
    );

    // Point to next transmit buffer slot and wrap if necessary.
    sc.txb_next_tx.set(sc.txb_next_tx.get() + 1);
    if sc.txb_next_tx.get() == sc.txb_cnt.get() {
        sc.txb_next_tx.set(0);
    }

    // Set a timer just in case we never hear from the board again.
    ifp.if_timer.set(2);
}

/// `dp8390_start`: start output on interface. We make two assumptions here: 1) that the
/// current priority is set to splnet _before_ this code is called *and* is returned to the
/// appropriate priority after return, 2) that the IFF_OACTIVE flag is checked before this
/// code is called (i.e. that the output part of the interface is idle).
pub fn dp8390_start(ifp: &'static Ifnet) {
    let sc = dp8390_softc(ifp);

    if ifp.if_flags.get() & IFF_RUNNING == 0 || ifq_is_oactive(&ifp.if_snd) {
        return;
    }

    loop {
        // See if there is room to put another packet in the buffer.
        if sc.txb_inuse.get() == sc.txb_cnt.get() {
            // No room. Indicate this to the outside world and exit.
            ifq_set_oactive(&ifp.if_snd);
            return;
        }
        let Some(m0) = ifq_dequeue(&ifp.if_snd) else {
            return;
        };

        // We need to use m->m_pkthdr.len, so require the header
        if m0.m_flags().get() & M_PKTHDR == 0 {
            panic(format_args!("dp8390_start: no header mbuf"));
        }

        // Tap off here if there is a BPF listener.
        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap(bpf, m0, BPF_DIRECTION_OUT);
        }

        // txb_new points to next open buffer slot.
        let buffer = sc.mem_start.get()
            + ((sc.txb_new.get() as i32 * ED_TXBUF_SIZE as i32) << ED_PAGE_SHIFT);

        let len = match sc.write_mbuf.get() {
            Some(write_mbuf) => write_mbuf(sc, m0, buffer),
            None => dp8390_write_mbuf(sc, m0, buffer),
        };

        m_freem(m0);
        sc.txb_len[sc.txb_new.get() as usize]
            .set(len.max((ETHER_MIN_LEN - ETHER_CRC_LEN) as i32) as u16);

        // Point to next buffer slot and wrap if necessary.
        sc.txb_new.set(sc.txb_new.get() + 1);
        if sc.txb_new.get() == sc.txb_cnt.get() {
            sc.txb_new.set(0);
        }

        // Start the first packet transmitting.
        let inuse = sc.txb_inuse.get();
        sc.txb_inuse.set(inuse + 1);
        if inuse == 0 {
            dp8390_xmit(sc);
        }

        // Loop back to the top to possibly buffer more packets.
    }
}

/// `dp8390_rint`: Ethernet interface receiver interrupt.
pub fn dp8390_rint(sc: &'static Dp8390Softc) {
    let ifp = sc.ifp();
    let ml = MbufList::new();

    'rx: loop {
        // Set NIC to page 1 registers to get 'current' pointer.
        sc.nic_barrier();
        sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_1 | ED_CR_STA);
        sc.nic_barrier();

        // 'sc->next_packet' is the logical beginning of the ring-buffer - i.e. it points to
        // where new data has been buffered. The 'CURR' (current) register points to the
        // logical end of the ring-buffer - i.e. it points to where additional new data will be
        // added. We loop here until the logical beginning equals the logical end (or in other
        // words, until the ring-buffer is empty).
        let current = sc.nic_get(ED_P1_CURR);
        if sc.next_packet.get() == u16::from(current) {
            break 'rx;
        }

        // Set NIC to page 0 registers to update boundary register.
        sc.nic_barrier();
        sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STA);
        sc.nic_barrier();

        loop {
            // Get pointer to this buffer's header structure.
            let packet_ptr = sc.mem_ring.get()
                + ((i32::from(sc.next_packet.get()) - i32::from(sc.rec_page_start.get()))
                    << ED_PAGE_SHIFT);

            let mut packet_hdr = Dp8390Ring::default();
            match sc.read_hdr.get() {
                Some(read_hdr) => read_hdr(sc, packet_ptr, &mut packet_hdr),
                None => dp8390_read_hdr(sc, packet_ptr, &mut packet_hdr),
            }
            let mut len = packet_hdr.count;

            // Try do deal with old, buggy chips that sometimes duplicate the low byte of the
            // length into the high byte. We do this by simply ignoring the high byte of the
            // length and always recalculating it.
            //
            // NOTE: sc->next_packet is pointing at the current packet.
            let np = i32::from(packet_hdr.next_packet);
            let cur = i32::from(sc.next_packet.get());
            let mut nlen = if np >= cur {
                (np - cur) as u8
            } else {
                ((np - i32::from(sc.rec_page_start.get()))
                    + (i32::from(sc.rec_page_stop.get()) - cur)) as u8
            };
            nlen = nlen.wrapping_sub(1);
            if (usize::from(len) & ED_PAGE_MASK) + ED_RING_HDRSZ > ED_PAGE_SIZE {
                nlen = nlen.wrapping_sub(1);
            }
            len = (len & ED_PAGE_MASK as u16) | (u16::from(nlen) << ED_PAGE_SHIFT);
            if cfg!(feature = "diagnostic") && len != packet_hdr.count {
                printf(format_args!(
                    "{}: length does not match next packet pointer\n",
                    sc.devname()
                ));
                printf(format_args!(
                    "{}: len {:04x} nlen {:04x} start {:02x} first {:02x} curr {:02x} next {:02x} stop {:02x}\n",
                    sc.devname(),
                    packet_hdr.count,
                    len,
                    sc.rec_page_start.get(),
                    sc.next_packet.get(),
                    current,
                    packet_hdr.next_packet,
                    sc.rec_page_stop.get()
                ));
            }

            // Be fairly liberal about what we allow as a "reasonable" length so that a
            // [crufty] packet will make it to BPF (and can thus be analyzed). Note that all
            // that is really important is that we have a length that will fit into one mbuf
            // cluster or less; the upper layer protocols can then figure out the length from
            // their own length field(s).
            if usize::from(len) <= MCLBYTES
                && u16::from(packet_hdr.next_packet) >= sc.rec_page_start.get()
                && u16::from(packet_hdr.next_packet) < sc.rec_page_stop.get()
            {
                // Go get packet.
                let m = dp8390_get(
                    sc,
                    packet_ptr + ED_RING_HDRSZ as i32,
                    len.wrapping_sub(ED_RING_HDRSZ as u16),
                );
                let Some(m) = m else {
                    ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
                    break 'rx;
                };
                ml_enqueue(&ml, m);
            } else {
                // Really BAD. The ring pointers are corrupted.
                log(
                    LOG_ERR,
                    format_args!(
                        "{}: NIC memory corrupt - invalid packet length {}\n",
                        sc.devname(),
                        len
                    ),
                );
                ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
                dp8390_reset(sc);
                break 'rx;
            }

            // Update next packet pointer.
            sc.next_packet.set(u16::from(packet_hdr.next_packet));

            // Update NIC boundary pointer - being careful to keep it one buffer behind (as
            // recommended by NS databook).
            let mut boundary = (sc.next_packet.get() as u8).wrapping_sub(1);
            if u16::from(boundary) < sc.rec_page_start.get() {
                boundary = (sc.rec_page_stop.get() - 1) as u8;
            }
            sc.nic_put(ED_P0_BNRY, boundary);

            if sc.next_packet.get() == u16::from(current) {
                break;
            }
        }
    }

    if_input(ifp, &ml);
}

/// `dp8390_intr`: Ethernet interface interrupt processor.
pub fn dp8390_intr(arg: *mut c_void) -> i32 {
    let sc = dp8390_arg(arg);
    let ifp = sc.ifp();

    if sc.sc_enabled.get() == 0 {
        return 0;
    }

    // Set NIC to page 0 registers.
    sc.nic_barrier();
    sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STA);
    sc.nic_barrier();

    let mut isr = sc.nic_get(ED_P0_ISR);
    if isr == 0 {
        return 0;
    }

    // Loop until there are no more new interrupts.
    loop {
        // Reset all the bits that we are 'acknowledging' by writing a '1' to each bit position
        // that was set. (Writing a '1' *clears* the bit.)
        sc.nic_put(ED_P0_ISR, isr);

        // Work around for AX88190 bug
        if sc.sc_flags.get() & DP8390_DO_AX88190_WORKAROUND != 0 {
            while sc.nic_get(ED_P0_ISR) & isr != 0 {
                sc.nic_put(ED_P0_ISR, 0);
                sc.nic_put(ED_P0_ISR, isr);
            }
        }

        // Handle transmitter interrupts. Handle these first because the receiver will reset
        // the board under some conditions.
        //
        // If the chip was reset while a packet was transmitting, it may still deliver a TX
        // interrupt. In this case, just ignore the interrupt.
        if isr & (ED_ISR_PTX | ED_ISR_TXE) != 0 && sc.txb_inuse.get() != 0 {
            let mut collisions = sc.nic_get(ED_P0_NCR) & 0x0f;

            // Check for transmit error. If a TX completed with an error, we end up throwing
            // the packet away. Really the only error that is possible is excessive
            // collisions, and in this case it is best to allow the automatic mechanisms of
            // TCP to backoff the flow. Of course, with UDP we're screwed, but this is
            // expected when a network is heavily loaded.
            if isr & ED_ISR_TXE != 0 {
                // Excessive collisions (16).
                if sc.nic_get(ED_P0_TSR) & ED_TSR_ABT != 0 && collisions == 0 {
                    // When collisions total 16, the P0_NCR will indicate 0, and the TSR_ABT
                    // is set.
                    collisions = 16;
                }

                // Update output errors counter.
                ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            } else {
                // Throw away the non-error status bits.
                //
                // XXX It may be useful to detect loss of carrier and late collisions here.
                let _ = sc.nic_get(ED_P0_TSR);
            }

            // Clear watchdog timer.
            ifp.if_timer.set(0);
            ifq_clr_oactive(&ifp.if_snd);

            // Add in total number of collisions on last transmission.
            ifp.if_collisions()
                .set(ifp.if_collisions().get() + u64::from(collisions));

            // Decrement buffer in-use count if not zero (can only be zero if a transmitter
            // interrupt occurred while not actually transmitting). If data is ready to
            // transmit, start it transmitting, otherwise defer until after handling receiver.
            sc.txb_inuse.set(sc.txb_inuse.get() - 1);
            if sc.txb_inuse.get() != 0 {
                dp8390_xmit(sc);
            }
        }

        // Handle receiver interrupts.
        if isr & (ED_ISR_PRX | ED_ISR_RXE | ED_ISR_OVW) != 0 {
            // Overwrite warning. In order to make sure that a lockup of the local DMA hasn't
            // occurred, we reset and re-init the NIC. The NSC manual suggests only a partial
            // reset/re-init is necessary - but some chips seem to want more. The DMA lockup
            // has been seen only with early rev chips - Methinks this bug was fixed in later
            // revs. -DG
            if isr & ED_ISR_OVW != 0 {
                ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
                // Stop/reset/re-init NIC.
                dp8390_reset(sc);
            } else {
                // Receiver Error. One or more of: CRC error, frame alignment error FIFO
                // overrun, or missed packet.
                if isr & ED_ISR_RXE != 0 {
                    ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
                }

                // Go get the packet(s)
                // XXX - Doing this on an error is dubious because there shouldn't be any data
                // to get (we've configured the interface to not accept packets with errors).
                match sc.recv_int.get() {
                    Some(recv_int) => recv_int(sc),
                    None => dp8390_rint(sc),
                }
            }
        }

        // If it looks like the transmitter can take more data, attempt to start output on the
        // interface. This is done after handling the receiver to give the receiver priority.
        dp8390_start(ifp);

        // Return NIC CR to standard state: page 0, remote DMA complete, start (toggling the
        // TXP bit off, even if was just set in the transmit routine, is *okay* - it is 'edge'
        // triggered from low to high).
        sc.nic_barrier();
        sc.nic_put(ED_P0_CR, sc.cr_proto.get() | ED_CR_PAGE_0 | ED_CR_STA);
        sc.nic_barrier();

        // If the Network Talley Counters overflow, read them to reset them. It appears that
        // old 8390's won't clear the ISR flag otherwise - resulting in an infinite loop.
        if isr & ED_ISR_CNT != 0 {
            let _ = sc.nic_get(ED_P0_CNTR0);
            let _ = sc.nic_get(ED_P0_CNTR1);
            let _ = sc.nic_get(ED_P0_CNTR2);
        }

        isr = sc.nic_get(ED_P0_ISR);
        if isr == 0 {
            return 1;
        }
    }
}

/// `dp8390_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the command
/// encodes.
pub unsafe fn dp8390_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = dp8390_softc(ifp);
    let mut error = Ok(());

    let s = splnet();

    match cmd {
        SIOCSIFADDR => {
            error = dp8390_enable(sc);
            if error.is_ok() {
                ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
                if ifp.if_flags.get() & IFF_RUNNING == 0 {
                    dp8390_init(sc);
                }
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    error = dp8390_enable(sc);
                    if error.is_ok() {
                        dp8390_init(sc);
                    }
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                dp8390_stop(sc);
                ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
                dp8390_disable(sc);
            }
        }
        SIOCGIFMEDIA | SIOCSIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
            // ifmediareq` (this function's contract).
            error = unsafe { ifmedia_ioctl(ifp, data, sc.sc_media(), cmd) };
        }
        _ => {
            // SAFETY: this function's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_arpcom, cmd, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            dp8390_stop(sc);
            dp8390_init(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

/// The bit of the 64-bit multicast filter an address with big-endian CRC `crc` selects: the
/// 6 most significant bits of the CRC, whose high 3 select the byte and low 3 the bit.
pub fn dp8390_mcaf_bit(af: &mut [u8; 8], crc: u32) {
    // Just want the 6 most significant bits.
    let crc = (crc >> 26) as usize;

    // Turn on the corresponding bit in the filter.
    af[crc >> 3] |= 1 << (crc & 0x7);
}

/// `dp8390_getmcaf`: compute the multicast address filter from the list of multicast
/// addresses we need to listen to.
pub fn dp8390_getmcaf(ac: &Arpcom, af: &mut [u8; 8]) {
    let ifp = &ac.ac_if;

    // Set up multicast address filter by passing all multicast addresses through a crc
    // generator, and then using the high order 6 bits as an index into the 64 bit logical
    // address filter. The high order bit selects the word, while the rest of the bits select
    // the bit within the word.

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        *af = [0xff; 8];
        return;
    }
    *af = [0; 8];
    let mut step = EtherMultistep { e_enm: None };
    let mut enm = ether_first_multi(&mut step, ac);
    while let Some(e) = enm {
        dp8390_mcaf_bit(af, ether_crc32_be(&e.enm_addrlo[..ETHER_ADDR_LEN]));

        enm = ether_next_multi(&mut step);
    }
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);
}

/// `dp8390_get`: copy data from receive buffer to a new mbuf chain allocating mbufs as
/// needed. Return pointer to first mbuf in chain. `src` is the pointer in dp8390 ring buffer
/// and `total_len` the amount of data to copy.
pub fn dp8390_get(
    sc: &'static Dp8390Softc,
    mut src: i32,
    mut total_len: u16,
) -> Option<&'static Mbuf> {
    let m0 = m_gethdr(M_DONTWAIT, MT_DATA)?;
    m0.m_pkthdr().len.set(i32::from(total_len));
    let mut len = MHLEN;
    let mut m = m0;

    while total_len > 0 {
        if usize::from(total_len) >= MINCLSIZE {
            let ok = m_clget(Some(m), M_DONTWAIT, MCLBYTES as u32).is_some()
                && m.m_flags().get() & M_EXT != 0;
            if !ok {
                m_freem(m0);
                return None;
            }
            len = MCLBYTES;
        }

        // Make sure the data after the Ethernet header is aligned.
        if core::ptr::eq(m, m0) {
            let data = m.m_data().get();
            let off = align(data as usize + ETHER_HDR_LEN) - ETHER_HDR_LEN - data as usize;
            m.m_data().set(data.wrapping_add(off));
            len -= off;
        }

        len = usize::from(total_len).min(len);
        m.m_len().set(len as u32);
        // SAFETY: `m` has `len` bytes of room at its data pointer (a header mbuf's MHLEN or a
        // cluster's MCLBYTES, less the alignment offset taken from `len` above).
        let dst = unsafe { slice::from_raw_parts_mut(m.m_data().get(), len) };
        src = match sc.ring_copy.get() {
            Some(ring_copy) => ring_copy(sc, src, dst),
            None => dp8390_ring_copy(sc, src, dst),
        };

        total_len -= len as u16;
        if total_len > 0 {
            let Some(newm) = m_get(M_DONTWAIT, MT_DATA) else {
                m_freem(m0);
                return None;
            };
            len = MLEN;
            m.m_next().set(Some(newm));
            m = newm;
        }
    }

    Some(m0)
}

/// Default driver support functions.
///
/// NOTE: all support functions assume 8-bit shared memory.
///
/// `dp8390_test_mem`: zero NIC buffer memory and verify that it is clear.
fn dp8390_test_mem(sc: &'static Dp8390Softc) -> i32 {
    let (buft, bufh) = sc.bufs();
    let start = sc.mem_start.get() as usize;

    for i in 0..sc.mem_size.get() as usize {
        bus_space_write_1(buft, bufh, start + i, 0);
    }

    for i in 0..sc.mem_size.get() as usize {
        if bus_space_read_1(buft, bufh, start + i) != 0 {
            printf(format_args!(
                ": failed to clear NIC buffer at offset {:x} - check configuration\n",
                start + i
            ));
            return 1;
        }
    }

    0
}

/// `dp8390_read_hdr`: read a packet header from the ring, given the source offset.
fn dp8390_read_hdr(sc: &'static Dp8390Softc, src: i32, hdrp: &mut Dp8390Ring) {
    let (buft, bufh) = sc.bufs();
    let src = src as usize;

    // The byte count includes a 4 byte header that was added by the NIC.
    hdrp.rsr = bus_space_read_1(buft, bufh, src);
    hdrp.next_packet = bus_space_read_1(buft, bufh, src + 1);
    hdrp.count = u16::from(bus_space_read_1(buft, bufh, src + 2))
        | (u16::from(bus_space_read_1(buft, bufh, src + 3)) << 8);
}

/// `dp8390_ring_copy`: copy `dst.len()` bytes from a packet in the ring buffer to a linear
/// destination buffer, given a source offset. Takes into account ring-wrap.
fn dp8390_ring_copy(sc: &'static Dp8390Softc, mut src: i32, dst: &mut [u8]) -> i32 {
    let (buft, bufh) = sc.bufs();
    let mut dst = dst;

    // Does copy wrap to lower addr in ring buffer?
    if src + dst.len() as i32 > sc.mem_end.get() {
        let tmp_amount = (sc.mem_end.get() - src) as usize;

        // Copy amount up to end of NIC memory.
        let (head, tail) = dst.split_at_mut(tmp_amount);
        for (i, b) in head.iter_mut().enumerate() {
            *b = bus_space_read_1(buft, bufh, src as usize + i);
        }

        src = sc.mem_ring.get();
        dst = tail;
    }
    for (i, b) in dst.iter_mut().enumerate() {
        *b = bus_space_read_1(buft, bufh, src as usize + i);
    }

    src + dst.len() as i32
}

/// `dp8390_write_mbuf`: copy a packet from an mbuf to the transmit buffer on the card.
///
/// Currently uses an extra buffer/extra memory copy, unless the whole packet fits in one
/// mbuf.
fn dp8390_write_mbuf(sc: &'static Dp8390Softc, m: &Mbuf, buf: i32) -> i32 {
    let (buft, bufh) = sc.bufs();
    let mut buf = buf as usize;
    let mut totlen = 0;

    let mut cur = Some(m);
    while let Some(m) = cur {
        let len = m.m_len().get() as usize;
        if len > 0 {
            // SAFETY: an mbuf's data pointer addresses `m_len` valid bytes.
            let data = unsafe { slice::from_raw_parts(m.m_data().get(), len) };
            for (i, b) in data.iter().enumerate() {
                bus_space_write_1(buft, bufh, buf + i, *b);
            }
            totlen += len;
            buf += len;
        }
        cur = m.m_next().get();
    }

    totlen as i32
}

/// `dp8390_enable`: enable power on the interface.
pub fn dp8390_enable(sc: &'static Dp8390Softc) -> Result<(), Errno> {
    if sc.sc_enabled.get() == 0 && sc.sc_enable.get().is_some_and(|enable| enable(sc) != 0) {
        printf(format_args!("{}: device enable failed\n", sc.devname()));
        return Err(Errno::EIO);
    }

    sc.sc_enabled.set(1);
    Ok(())
}

/// `dp8390_disable`: disable power on the interface.
pub fn dp8390_disable(sc: &'static Dp8390Softc) {
    if let (true, Some(disable)) = (sc.sc_enabled.get() != 0, sc.sc_disable.get()) {
        disable(sc);
        sc.sc_enabled.set(0);
    }
}

/// `dp8390_detach`.
pub fn dp8390_detach(sc: &'static Dp8390Softc, _flags: i32) -> i32 {
    let ifp = sc.ifp();

    // dp8390_disable() checks sc->sc_enabled
    dp8390_disable(sc);

    if let Some(media_fini) = sc.sc_media_fini.get() {
        media_fini(sc);
    }

    // Delete all remaining media.
    ifmedia_delete_instance(sc.sc_media(), IFM_INST_ANY);

    ether_ifdetach(ifp);
    if_detach(ifp);

    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcaf_bit_uses_the_six_high_bits_of_the_crc() {
        let mut af = [0u8; 8];
        dp8390_mcaf_bit(&mut af, 0);
        assert_eq!(af, [1, 0, 0, 0, 0, 0, 0, 0]);

        // 0b111111 << 26: byte 7, bit 7.
        let mut af = [0u8; 8];
        dp8390_mcaf_bit(&mut af, 0xfc00_0000);
        assert_eq!(af, [0, 0, 0, 0, 0, 0, 0, 0x80]);

        // 0b001010 = 10: byte 1, bit 2; low bits of the CRC do not matter.
        let mut af = [0u8; 8];
        dp8390_mcaf_bit(&mut af, (10 << 26) | 0x03ff_ffff);
        assert_eq!(af, [0, 4, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn ring_header_is_four_bytes() {
        assert_eq!(size_of::<Dp8390Ring>(), ED_RING_HDRSZ);
    }

    #[test]
    fn buffer_layout_constants() {
        assert_eq!(ED_PAGE_SIZE, 1 << ED_PAGE_SHIFT);
        assert_eq!(ED_PAGE_MASK, ED_PAGE_SIZE - 1);
        assert_eq!(ED_TXBUF_SIZE * ED_PAGE_SIZE, 1536);
    }
}
/* </TESTS> */
