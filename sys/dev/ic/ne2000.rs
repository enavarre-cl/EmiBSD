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

/*-
 * Copyright (c) 1997, 1998 The NetBSD Foundation, Inc.
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
//! Common code shared by all NE2000-compatible Ethernet interfaces (`dev/ic/ne2000.c`).
//!
//! Upstream: sys/dev/ic/ne2000.c @ 3ce1f3f79392
//!
//! The board's memory is reached by programmed I/O through the ASIC's data port: `ne2000_readmem`
//! and `ne2000_writemem` set the remote DMA registers of the DP8390 and then move bytes or
//! words through `NE2000_ASIC_DATA`. `ne2000_detect` tells an NE1000 from an NE2000 by writing a
//! test pattern to the board's memory, `ne2000_attach` sets up the [`Dp8390Softc`] callbacks
//! and reads the station address.
//!
//! ## Deviations
//! - The `GWETHER` block of `ne2000_attach` (and its `delay`/reset in `ne2000_detect`), a
//!   compile-time option no OpenBSD kernel configuration defines, is not ported.
//! - The `DIAGNOSTIC` check of `ne2000_write_mbuf` is the `diagnostic` cargo feature.
//! - `ne2000_readmem` takes the destination as a slice. The C rounds the transfer up to a
//!   word and so writes a byte past an odd-length destination (it relies on mbufs being word
//!   sized); here the extra byte is read and dropped.
//! - The `BYTE_ORDER == BIG_ENDIAN` swap of `ne2000_read_hdr` is the little-endian read of
//!   the header's count.
//! - `ne2000_attach` takes the station address as an `Option<[u8; 6]>`.

use core::ptr;
use core::slice;

use crate::dev::ic::ax88190reg::AX88190_NODEID_OFFSET;
use crate::dev::ic::dp8390::{dp8390_config, dp8390_media_init, dp8390_reset};
use crate::dev::ic::dp8390reg::{
    Dp8390Ring, ED_CR_PAGE_0, ED_CR_RD0, ED_CR_RD1, ED_CR_RD2, ED_CR_STA, ED_CR_STP, ED_CR_TXP,
    ED_DCR_FT1, ED_DCR_LS, ED_DCR_WTS, ED_ISR_RDC, ED_ISR_RST, ED_P0_CR, ED_P0_CRDA0, ED_P0_CRDA1,
    ED_P0_DCR, ED_P0_ISR, ED_P0_PSTART, ED_P0_PSTOP, ED_P0_RBCR0, ED_P0_RBCR1, ED_P0_RCR,
    ED_P0_RSAR0, ED_P0_RSAR1, ED_PAGE_SHIFT, ED_RCR_INTT, ED_RCR_MON,
};
use crate::dev::ic::dp8390var::{
    DP8390_DO_AX88190_WORKAROUND, DP8390_NO_REMOTE_DMA_COMPLETE, Dp8390Softc,
};
use crate::dev::ic::ne2000reg::{NE2000_ASIC_DATA, NE2000_ASIC_RESET};
use crate::dev::ic::ne2000var::{Ne2000Softc, Ne2000Type};
use crate::kern::subr_prf::{log, panic, printf};
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusSpaceHandle, BusSpaceTag,
    bus_space_barrier, bus_space_read_1, bus_space_read_multi_1, bus_space_read_raw_multi_2,
    bus_space_write_1, bus_space_write_multi_1, bus_space_write_raw_multi_2,
};
use crate::machine::cpu::delay;
use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::sys::device::{Cfdriver, DV_IFNET};
use crate::sys::mbuf::Mbuf;
use crate::sys::syslog::LOG_WARNING;

/// `ASIC_BARRIER(asict, asich)`.
fn asic_barrier(asict: BusSpaceTag, asich: BusSpaceHandle) {
    bus_space_barrier(
        asict,
        asich,
        0,
        0x10,
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );
}

/// `ne_cd`.
pub static NE_CD: Cfdriver = Cfdriver::new(b"ne", DV_IFNET, 0);

/// `(struct ne2000_softc *)sc`: the softc a dp8390 callback belongs to.
fn ne2000_softc(sc: &Dp8390Softc) -> &Ne2000Softc {
    // SAFETY: `ne2000_attach` installs the callbacks calling this only on the `Dp8390Softc`
    // that is the first member of a `#[repr(C)]` `Ne2000Softc` (or of a softc that has one
    // first), so the pointer is also a pointer to the whole `Ne2000Softc`, which lives as long.
    unsafe { &*ptr::from_ref(sc).cast::<Ne2000Softc>() }
}

/// `nsc->sc_asict` and `nsc->sc_asich`, which the bus front end sets first.
fn asic_regs(nsc: &Ne2000Softc) -> (BusSpaceTag, BusSpaceHandle) {
    match (nsc.sc_asict.get(), nsc.sc_asich.get()) {
        (Some(t), Some(h)) => (t, h),
        _ => panic(format_args!("{}: ASIC not mapped", nsc.sc_dp8390.devname())),
    }
}

/// `ne2000_attach`: returns 0, or 1 when the board is gone or the setup failed.
pub fn ne2000_attach(nsc: &'static Ne2000Softc, myea: Option<[u8; ETHER_ADDR_LEN]>) -> i32 {
    let dsc = &nsc.sc_dp8390;
    let (nict, nich) = dsc.regs();
    let mut romdata = [0u8; 16];

    // Detect it again unless caller specified it; this gives us the memory size.
    if nsc.sc_type.get() == Ne2000Type::Unknown {
        nsc.sc_type.set(ne2000_detect(nsc));
    }

    // 8k of memory for NE1000, 16k otherwise.
    let (memsize, useword) = match nsc.sc_type.get() {
        Ne2000Type::Unknown => {
            printf(format_args!(": where did the card go?\n"));
            return 1;
        }
        Ne2000Type::Ne1000 => (8192, false),
        Ne2000Type::Ne2000
        | Ne2000Type::Ax88190 // XXX really?
        | Ne2000Type::Ax88790
        | Ne2000Type::Dl10019
        | Ne2000Type::Dl10022 => (8192 * 2, true),
    };

    nsc.sc_useword.set(i32::from(useword));

    dsc.cr_proto.set(ED_CR_RD2);
    if nsc.sc_type.get() == Ne2000Type::Ax88190 || nsc.sc_type.get() == Ne2000Type::Ax88790 {
        dsc.rcr_proto.set(ED_RCR_INTT);
        dsc.sc_flags
            .set(dsc.sc_flags.get() | DP8390_DO_AX88190_WORKAROUND);
    } else {
        dsc.rcr_proto.set(0);
    }

    // DCR gets:
    //
    //	FIFO threshold to 8, No auto-init Remote DMA, byte order=80x86.
    //
    // NE1000 gets byte-wide DMA, NE2000 gets word-wide DMA.
    dsc.dcr_reg
        .set(ED_DCR_FT1 | ED_DCR_LS | if useword { ED_DCR_WTS } else { 0 });

    dsc.test_mem.set(Some(ne2000_test_mem));
    dsc.ring_copy.set(Some(ne2000_ring_copy));
    dsc.write_mbuf.set(Some(ne2000_write_mbuf));
    dsc.read_hdr.set(Some(ne2000_read_hdr));

    // Registers are linear.
    for (i, r) in dsc.sc_reg_map.iter().enumerate() {
        r.set(i);
    }

    // NIC memory doesn't start at zero on an NE board. The start address is tied to the bus
    // width. (It happens to be computed the same way as mem size.)
    dsc.mem_start.set(memsize);

    dsc.mem_size.set(memsize);

    if let Some(myea) = myea {
        dsc.sc_arpcom.ac_enaddr.set(myea);
    } else {
        // Read the station address.
        let mut enaddr = [0u8; ETHER_ADDR_LEN];
        if nsc.sc_type.get() == Ne2000Type::Ax88190 || nsc.sc_type.get() == Ne2000Type::Ax88790 {
            // Select page 0 registers.
            dsc.nic_barrier();
            bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STA);
            dsc.nic_barrier();
            // Select word transfer.
            bus_space_write_1(nict, nich, ED_P0_DCR, ED_DCR_WTS);
            dsc.nic_barrier();
            ne2000_readmem(nsc, AX88190_NODEID_OFFSET, &mut enaddr, useword);
        } else {
            ne2000_readmem(nsc, 0, &mut romdata, useword);
            for (i, b) in enaddr.iter_mut().enumerate() {
                *b = romdata[i * if useword { 2 } else { 1 }];
            }
        }
        dsc.sc_arpcom.ac_enaddr.set(enaddr);
    }

    // Clear any pending interrupts that might have occurred above.
    dsc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_ISR, 0xff);
    dsc.nic_barrier();

    if dsc.sc_media_init.get().is_none() {
        dsc.sc_media_init.set(Some(dp8390_media_init));
    }

    if dp8390_config(dsc) != 0 {
        printf(format_args!(": setup failed\n"));
        return 1;
    }

    0
}

/// `ne2000_detect`: detect an NE-2000 or compatible. Returns a model code.
pub fn ne2000_detect(nsc: &Ne2000Softc) -> Ne2000Type {
    let dsc = &nsc.sc_dp8390;
    let (nict, nich) = dsc.regs();
    let (asict, asich) = asic_regs(nsc);
    let mut test_pattern = [0u8; 32];
    test_pattern[..29].copy_from_slice(b"THIS is A memory TEST pattern");
    let mut test_buffer = [0u8; 32];
    let mut rv = Ne2000Type::Unknown;

    let state = dsc.sc_enabled.get();
    dsc.sc_enabled.set(0);

    'out: {
        // Reset the board.
        let mut tmp = bus_space_read_1(asict, asich, NE2000_ASIC_RESET);
        asic_barrier(asict, asich);
        delay(10000);

        // I don't know if this is necessary; probably cruft leftover from Clarkson packet
        // driver code. Doesn't do a thing on the boards I've tested. -DG [note that a outb(0x84,
        // 0) seems to work here, and is non-invasive...but some boards don't seem to reset and I
        // don't have complete documentation on what the 'right' thing to do is...so we do the
        // invasive thing for now. Yuck.]
        bus_space_write_1(asict, asich, NE2000_ASIC_RESET, tmp);
        asic_barrier(asict, asich);
        delay(5000);

        // This is needed because some NE clones apparently don't reset the NIC properly (or the
        // NIC chip doesn't reset fully on power-up).
        // XXX - this makes the probe invasive! Done against my better judgement. -DLG
        bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STP);
        dsc.nic_barrier();

        delay(5000);

        // Generic probe routine for testing for the existence of a DS8390. Must be performed
        // after the NIC has just been reset. This works by looking at certain register values
        // that are guaranteed to be initialized a certain way after power-up or reset.
        //
        // Specifically:
        //
        //	Register		reset bits	set bits
        //	--------		----------	--------
        //	CR			TXP, STA	RD2, STP
        //	ISR					RST
        //	IMR			<all>
        //	DCR					LAS
        //	TCR			LB1, LB0
        //
        // We only look at CR and ISR, however, since looking at the others would require
        // changing register pages, which would be intrusive if this isn't an 8390.

        tmp = bus_space_read_1(nict, nich, ED_P0_CR);
        if tmp & (ED_CR_RD2 | ED_CR_TXP | ED_CR_STA | ED_CR_STP) != (ED_CR_RD2 | ED_CR_STP) {
            break 'out;
        }

        tmp = bus_space_read_1(nict, nich, ED_P0_ISR);
        if tmp & ED_ISR_RST != ED_ISR_RST {
            break 'out;
        }

        bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STA);
        dsc.nic_barrier();

        for _ in 0..100 {
            if bus_space_read_1(nict, nich, ED_P0_ISR) & ED_ISR_RST == ED_ISR_RST {
                // Ack the reset bit.
                bus_space_write_1(nict, nich, ED_P0_ISR, ED_ISR_RST);
                dsc.nic_barrier();
                break;
            }
            delay(100);
        }

        // Test the ability to read and write to the NIC memory. This has the side effect of
        // determining if this is an NE1000 or an NE2000.

        // This prevents packets from being stored in the NIC memory when the readmem routine
        // turns on the start bit in the CR.
        bus_space_write_1(nict, nich, ED_P0_RCR, ED_RCR_MON);
        dsc.nic_barrier();

        // Temporarily initialize DCR for byte operations.
        bus_space_write_1(nict, nich, ED_P0_DCR, ED_DCR_FT1 | ED_DCR_LS);

        bus_space_write_1(nict, nich, ED_P0_PSTART, (8192 >> ED_PAGE_SHIFT) as u8);
        bus_space_write_1(nict, nich, ED_P0_PSTOP, (16384 >> ED_PAGE_SHIFT) as u8);

        // Write a test pattern in byte mode. If this fails, then there probably isn't any
        // memory at 8k - which likely means that the board is an NE2000.
        ne2000_writemem(nsc, &test_pattern, 8192, false);
        ne2000_readmem(nsc, 8192, &mut test_buffer, false);

        if test_pattern != test_buffer {
            // not an NE1000 - try NE2000
            bus_space_write_1(nict, nich, ED_P0_DCR, ED_DCR_WTS | ED_DCR_FT1 | ED_DCR_LS);
            bus_space_write_1(nict, nich, ED_P0_PSTART, (16384 >> ED_PAGE_SHIFT) as u8);
            bus_space_write_1(nict, nich, ED_P0_PSTOP, (32768 >> ED_PAGE_SHIFT) as u8);

            // Write the test pattern in word mode. If this also fails, then we don't know what
            // this board is.
            ne2000_writemem(nsc, &test_pattern, 16384, true);
            ne2000_readmem(nsc, 16384, &mut test_buffer, true);

            if test_pattern != test_buffer {
                break 'out; // not an NE2000 either
            }

            rv = Ne2000Type::Ne2000;
        } else {
            // We're an NE1000.
            rv = Ne2000Type::Ne1000;
        }

        // Clear any pending interrupts that might have occurred above.
        dsc.nic_barrier();
        bus_space_write_1(nict, nich, ED_P0_ISR, 0xff);
    }

    dsc.sc_enabled.set(state);

    rv
}

/// `ne2000_write_mbuf`: write an mbuf chain to the destination NIC memory address using
/// programmed I/O.
pub fn ne2000_write_mbuf(sc: &'static Dp8390Softc, m: &Mbuf, buf: i32) -> i32 {
    let nsc = ne2000_softc(sc);
    let (nict, nich) = sc.regs();
    let (asict, asich) = asic_regs(nsc);
    let mut maxwait = 100; // about 120us

    let savelen = m.m_pkthdr().len.get();

    // Select page 0 registers.
    sc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STA);
    sc.nic_barrier();

    // Reset remote DMA complete flag.
    bus_space_write_1(nict, nich, ED_P0_ISR, ED_ISR_RDC);
    sc.nic_barrier();

    // Set up DMA byte count.
    bus_space_write_1(nict, nich, ED_P0_RBCR0, savelen as u8);
    bus_space_write_1(nict, nich, ED_P0_RBCR1, (savelen >> 8) as u8);

    // Set up destination address in NIC mem.
    bus_space_write_1(nict, nich, ED_P0_RSAR0, buf as u8);
    bus_space_write_1(nict, nich, ED_P0_RSAR1, (buf >> 8) as u8);

    // Set remote DMA write.
    sc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD1 | ED_CR_PAGE_0 | ED_CR_STA);
    sc.nic_barrier();

    // Transfer the mbuf chain to the NIC memory. NE2000 cards require that data be transferred
    // as words, and only words, so that case requires some extra code to patch over odd-length
    // mbufs.
    if nsc.sc_type.get() == Ne2000Type::Ne1000 {
        // NE1000s are easy.
        let mut cur = Some(m);
        while let Some(m) = cur {
            let len = m.m_len().get() as usize;
            if len != 0 {
                // SAFETY: an mbuf's data pointer addresses `m_len` valid bytes.
                let data = unsafe { slice::from_raw_parts(m.m_data().get(), len) };
                bus_space_write_multi_1(asict, asich, NE2000_ASIC_DATA, data);
            }
            cur = m.m_next().get();
        }
    } else {
        // NE2000s are a bit trickier.
        let mut savebyte = [0u8; 2];

        // Start out with no leftover data.
        let mut leftover = false;

        let mut cur = Some(m);
        while let Some(m) = cur {
            let l = m.m_len().get() as usize;
            cur = m.m_next().get();
            if l == 0 {
                continue;
            }
            // SAFETY: an mbuf's data pointer addresses `m_len` valid bytes.
            let mut data = unsafe { slice::from_raw_parts(m.m_data().get(), l) };
            while !data.is_empty() {
                if leftover {
                    // Data left over (from mbuf or realignment). Buffer the next byte, and
                    // write it and the leftover data out.
                    savebyte[1] = data[0];
                    data = &data[1..];
                    bus_space_write_raw_multi_2(asict, asich, NE2000_ASIC_DATA, &savebyte);
                    leftover = false;
                } else if !(data.as_ptr() as usize).is_multiple_of(align_of::<u16>()) {
                    // Unaligned data; buffer the next byte.
                    savebyte[0] = data[0];
                    data = &data[1..];
                    leftover = true;
                } else {
                    // Aligned data; output contiguous words as much as we can, then buffer the
                    // remaining byte, if any.
                    leftover = data.len() & 1 != 0;
                    let n = data.len() & !1;
                    bus_space_write_raw_multi_2(asict, asich, NE2000_ASIC_DATA, &data[..n]);
                    if leftover {
                        savebyte[0] = data[n];
                    }
                    data = &[];
                }
            }
        }
        if leftover {
            savebyte[1] = 0;
            bus_space_write_raw_multi_2(asict, asich, NE2000_ASIC_DATA, &savebyte);
        }
    }
    sc.nic_barrier();

    // AX88796 doesn't seem to have remote DMA complete
    if sc.sc_flags.get() & DP8390_NO_REMOTE_DMA_COMPLETE != 0 {
        return savelen;
    }

    // Wait for remote DMA to complete. This is necessary because on the transmit side, data is
    // handled internally by the NIC in bursts, and we can't start another remote DMA until
    // this one completes. Not waiting causes really bad things to happen - like the NIC
    // wedging the bus.
    while bus_space_read_1(nict, nich, ED_P0_ISR) & ED_ISR_RDC != ED_ISR_RDC {
        maxwait -= 1;
        if maxwait == 0 {
            break;
        }
        bus_space_read_1(nict, nich, ED_P0_CRDA1);
        bus_space_read_1(nict, nich, ED_P0_CRDA0);
        sc.nic_barrier();
        delay(1);
    }

    if maxwait == 0 {
        log(
            LOG_WARNING,
            format_args!("{}: remote transmit DMA failed to complete\n", sc.devname()),
        );
        dp8390_reset(sc);
    }

    savelen
}

/// `ne2000_ring_copy`: given a source and destination address, copy `dst.len()` of a packet
/// from the ring buffer into a linear destination buffer. Takes into account ring-wrap.
pub fn ne2000_ring_copy(sc: &'static Dp8390Softc, mut src: i32, dst: &mut [u8]) -> i32 {
    let nsc = ne2000_softc(sc);
    let useword = nsc.sc_useword.get() != 0;
    let mut dst = dst;

    // Does copy wrap to lower addr in ring buffer?
    if src + dst.len() as i32 > sc.mem_end.get() {
        let tmp_amount = (sc.mem_end.get() - src) as usize;

        // Copy amount up to end of NIC memory.
        let (head, tail) = dst.split_at_mut(tmp_amount);
        ne2000_readmem(nsc, src, head, useword);

        src = sc.mem_ring.get();
        dst = tail;
    }

    ne2000_readmem(nsc, src, dst, useword);

    src + dst.len() as i32
}

/// `ne2000_read_hdr`.
pub fn ne2000_read_hdr(sc: &'static Dp8390Softc, buf: i32, hdr: &mut Dp8390Ring) {
    let nsc = ne2000_softc(sc);
    let mut raw = [0u8; 4];

    ne2000_readmem(nsc, buf, &mut raw, nsc.sc_useword.get() != 0);
    hdr.rsr = raw[0];
    hdr.next_packet = raw[1];
    hdr.count = u16::from_le_bytes([raw[2], raw[3]]);
}

/// `ne2000_test_mem`: a noop.
pub fn ne2000_test_mem(_sc: &'static Dp8390Softc) -> i32 {
    0
}

/// `ne2000_readmem`: given a NIC memory source address and a host memory destination, copy
/// `dst.len()` from NIC to host using programmed i/o. The amount is rounded up to a word on
/// the NIC's side (see the Deviations).
pub fn ne2000_readmem(nsc: &Ne2000Softc, src: i32, dst: &mut [u8], useword: bool) {
    let dsc = &nsc.sc_dp8390;
    let (nict, nich) = dsc.regs();
    let (asict, asich) = asic_regs(nsc);

    // Select page 0 registers.
    dsc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STA);
    dsc.nic_barrier();

    // Round up to a word.
    let amount = (dst.len() + 1) & !1;

    // Set up DMA byte count.
    bus_space_write_1(nict, nich, ED_P0_RBCR0, amount as u8);
    bus_space_write_1(nict, nich, ED_P0_RBCR1, (amount >> 8) as u8);

    // Set up source address in NIC mem.
    bus_space_write_1(nict, nich, ED_P0_RSAR0, src as u8);
    bus_space_write_1(nict, nich, ED_P0_RSAR1, (src >> 8) as u8);

    dsc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD0 | ED_CR_PAGE_0 | ED_CR_STA);

    asic_barrier(asict, asich);
    let even = dst.len() & !1;
    if useword {
        bus_space_read_raw_multi_2(asict, asich, NE2000_ASIC_DATA, &mut dst[..even]);
        if dst.len() != even {
            let mut last = [0u8; 2];
            bus_space_read_raw_multi_2(asict, asich, NE2000_ASIC_DATA, &mut last);
            dst[even] = last[0];
        }
    } else {
        bus_space_read_multi_1(asict, asich, NE2000_ASIC_DATA, dst);
        if amount != dst.len() {
            let mut last = [0u8; 1];
            bus_space_read_multi_1(asict, asich, NE2000_ASIC_DATA, &mut last);
        }
    }
}

/// `ne2000_writemem`: stripped down routine for writing a linear buffer to NIC memory. Only
/// used in the probe routine to test the memory. `src.len()` must be even.
pub fn ne2000_writemem(nsc: &Ne2000Softc, src: &[u8], dst: i32, useword: bool) {
    let dsc = &nsc.sc_dp8390;
    let (nict, nich) = dsc.regs();
    let (asict, asich) = asic_regs(nsc);
    let mut maxwait = 100; // about 120us
    let len = src.len();

    // Select page 0 registers.
    dsc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD2 | ED_CR_PAGE_0 | ED_CR_STA);
    dsc.nic_barrier();

    // Reset remote DMA complete flag.
    bus_space_write_1(nict, nich, ED_P0_ISR, ED_ISR_RDC);
    dsc.nic_barrier();

    // Set up DMA byte count.
    bus_space_write_1(nict, nich, ED_P0_RBCR0, len as u8);
    bus_space_write_1(nict, nich, ED_P0_RBCR1, (len >> 8) as u8);

    // Set up destination address in NIC mem.
    bus_space_write_1(nict, nich, ED_P0_RSAR0, dst as u8);
    bus_space_write_1(nict, nich, ED_P0_RSAR1, (dst >> 8) as u8);

    // Set remote DMA write.
    dsc.nic_barrier();
    bus_space_write_1(nict, nich, ED_P0_CR, ED_CR_RD1 | ED_CR_PAGE_0 | ED_CR_STA);
    dsc.nic_barrier();

    asic_barrier(asict, asich);
    if useword {
        bus_space_write_raw_multi_2(asict, asich, NE2000_ASIC_DATA, src);
    } else {
        bus_space_write_multi_1(asict, asich, NE2000_ASIC_DATA, src);
    }
    asic_barrier(asict, asich);

    // Wait for remote DMA to complete. This is necessary because on the transmit side, data is
    // handled internally by the NIC in bursts, and we can't start another remote DMA until
    // this one completes. Not waiting causes really bad things to happen - like the NIC
    // wedging the bus.
    while bus_space_read_1(nict, nich, ED_P0_ISR) & ED_ISR_RDC != ED_ISR_RDC {
        maxwait -= 1;
        if maxwait == 0 {
            break;
        }
        delay(1);
    }
}

/// `ne2000_detach`.
pub fn ne2000_detach(sc: &'static Ne2000Softc, flags: i32) -> i32 {
    crate::dev::ic::dp8390::dp8390_detach(&sc.sc_dp8390, flags)
}
/* </CODE> */
