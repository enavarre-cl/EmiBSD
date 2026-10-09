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
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and Jason R. Thorpe.
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

/*-
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Ralph Campbell and Rick Macklem.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)if_lereg.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/lancereg.h>`: register description for the AMD LANCE family of Ethernet chips
//! (Am7990 LANCE ... Am79c978 PCnet-Home): the CSR and BCR numbers, the bits of the registers
//! pcn(4) uses, the initialization block mode bits and the chip id fields.
//!
//! Upstream: sys/dev/ic/lancereg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - All constants are `u32`; the function-like macros (`LE_BCNT`, `LE_C15_PORTSEL`,
//!   `LE_C80_RCVFW`, `LE_C80_XMTSP`, `LE_C80_XMTFW`, `CHIPID_MANFID`, `CHIPID_PARTID`,
//!   `CHIPID_VER`) are lower-case `const fn`s (`docs/C_TO_RUST.md`).
//! - `LE_INITADDR`, `LE_RMDADDR`, `LE_TMDADDR`, `LE_RBUFADDR` and `LE_TBUFADDR` name members of
//!   the softc of the lance(4) family (`sc_initaddr`, `sc_rmdaddr`, ...), which pcn(4) does not
//!   have; they are not ported with it.
//! - The `%b` strings (`LE_C0_BITS`) are Rust strings with the octal escapes written as `\xNN`.

/// `LEBLEN`: ETHERMTU + header + CRC.
pub const LEBLEN: u32 = 1536;
/// `LEMINSIZE`: should be 64 if mode DTCR is set.
pub const LEMINSIZE: u32 = 60;
/// `LE_BCNT(x)`: the byte count fields in descriptors are in two's complement. This does the
/// conversion for us on unsigned numbers.
pub const fn le_bcnt(x: u32) -> u32 {
    (!x).wrapping_add(1)
}
/// `LE_CSR0`: Control and status register.
pub const LE_CSR0: u32 = 0x0000;
/// `LE_CSR1`: low address of init block.
pub const LE_CSR1: u32 = 0x0001;
/// `LE_CSR2`: high address of init block.
pub const LE_CSR2: u32 = 0x0002;
/// `LE_CSR3`: Bus master and control.
pub const LE_CSR3: u32 = 0x0003;
/// `LE_CSR4`: Test and features control.
pub const LE_CSR4: u32 = 0x0004;
/// `LE_CSR5`: Extended control and Interrupt 1.
pub const LE_CSR5: u32 = 0x0005;
/// `LE_CSR6`: Rx/Tx Descriptor table length.
pub const LE_CSR6: u32 = 0x0006;
/// `LE_CSR7`: Extended control and interrupt 2.
pub const LE_CSR7: u32 = 0x0007;
/// `LE_CSR8`: Logical Address Filter 0.
pub const LE_CSR8: u32 = 0x0008;
/// `LE_CSR9`: Logical Address Filter 1.
pub const LE_CSR9: u32 = 0x0009;
/// `LE_CSR10`: Logical Address Filter 2.
pub const LE_CSR10: u32 = 0x000a;
/// `LE_CSR11`: Logical Address Filter 3.
pub const LE_CSR11: u32 = 0x000b;
/// `LE_CSR12`: Physical Address 0.
pub const LE_CSR12: u32 = 0x000c;
/// `LE_CSR13`: Physical Address 1.
pub const LE_CSR13: u32 = 0x000d;
/// `LE_CSR14`: Physical Address 2.
pub const LE_CSR14: u32 = 0x000e;
/// `LE_CSR15`: Mode.
pub const LE_CSR15: u32 = 0x000f;
/// `LE_CSR16`: Initialization Block addr lower.
pub const LE_CSR16: u32 = 0x0010;
/// `LE_CSR17`: Initialization Block addr upper.
pub const LE_CSR17: u32 = 0x0011;
/// `LE_CSR18`: Current Rx Buffer addr lower.
pub const LE_CSR18: u32 = 0x0012;
/// `LE_CSR19`: Current Rx Buffer addr upper.
pub const LE_CSR19: u32 = 0x0013;
/// `LE_CSR20`: Current Tx Buffer addr lower.
pub const LE_CSR20: u32 = 0x0014;
/// `LE_CSR21`: Current Tx Buffer addr upper.
pub const LE_CSR21: u32 = 0x0015;
/// `LE_CSR22`: Next Rx Buffer addr lower.
pub const LE_CSR22: u32 = 0x0016;
/// `LE_CSR23`: Next Rx Buffer addr upper.
pub const LE_CSR23: u32 = 0x0017;
/// `LE_CSR24`: Base addr of Rx ring lower.
pub const LE_CSR24: u32 = 0x0018;
/// `LE_CSR25`: Base addr of Rx ring upper.
pub const LE_CSR25: u32 = 0x0019;
/// `LE_CSR26`: Next Rx Desc addr lower.
pub const LE_CSR26: u32 = 0x001a;
/// `LE_CSR27`: Next Rx Desc addr upper.
pub const LE_CSR27: u32 = 0x001b;
/// `LE_CSR28`: Current Rx Desc addr lower.
pub const LE_CSR28: u32 = 0x001c;
/// `LE_CSR29`: Current Rx Desc addr upper.
pub const LE_CSR29: u32 = 0x001d;
/// `LE_CSR30`: Base addr of Tx ring lower.
pub const LE_CSR30: u32 = 0x001e;
/// `LE_CSR31`: Base addr of Tx ring upper.
pub const LE_CSR31: u32 = 0x001f;
/// `LE_CSR32`: Next Tx Desc addr lower.
pub const LE_CSR32: u32 = 0x0020;
/// `LE_CSR33`: Next Tx Desc addr upper.
pub const LE_CSR33: u32 = 0x0021;
/// `LE_CSR34`: Current Tx Desc addr lower.
pub const LE_CSR34: u32 = 0x0022;
/// `LE_CSR35`: Current Tx Desc addr upper.
pub const LE_CSR35: u32 = 0x0023;
/// `LE_CSR36`: Next Next Rx Desc addr lower.
pub const LE_CSR36: u32 = 0x0024;
/// `LE_CSR37`: Next Next Rx Desc addr upper.
pub const LE_CSR37: u32 = 0x0025;
/// `LE_CSR38`: Next Next Tx Desc addr lower.
pub const LE_CSR38: u32 = 0x0026;
/// `LE_CSR39`: Next Next Tx Desc addr upper.
pub const LE_CSR39: u32 = 0x0027;
/// `LE_CSR40`: Current Rx Byte Count.
pub const LE_CSR40: u32 = 0x0028;
/// `LE_CSR41`: Current Rx Status.
pub const LE_CSR41: u32 = 0x0029;
/// `LE_CSR42`: Current Tx Byte Count.
pub const LE_CSR42: u32 = 0x002a;
/// `LE_CSR43`: Current Tx Status.
pub const LE_CSR43: u32 = 0x002b;
/// `LE_CSR44`: Next Rx Byte Count.
pub const LE_CSR44: u32 = 0x002c;
/// `LE_CSR45`: Next Rx Status.
pub const LE_CSR45: u32 = 0x002d;
/// `LE_CSR46`: Tx Poll Time Counter.
pub const LE_CSR46: u32 = 0x002e;
/// `LE_CSR47`: Tx Polling Interval.
pub const LE_CSR47: u32 = 0x002f;
/// `LE_CSR48`: Rx Poll Time Counter.
pub const LE_CSR48: u32 = 0x0030;
/// `LE_CSR49`: Rx Polling Interval.
pub const LE_CSR49: u32 = 0x0031;
/// `LE_CSR58`: Software Style.
pub const LE_CSR58: u32 = 0x003a;
/// `LE_CSR60`: Previous Tx Desc addr lower.
pub const LE_CSR60: u32 = 0x003c;
/// `LE_CSR61`: Previous Tx Desc addr upper.
pub const LE_CSR61: u32 = 0x003d;
/// `LE_CSR62`: Previous Tx Byte Count.
pub const LE_CSR62: u32 = 0x003e;
/// `LE_CSR63`: Previous Tx Status.
pub const LE_CSR63: u32 = 0x003f;
/// `LE_CSR64`: Next Tx Buffer addr lower.
pub const LE_CSR64: u32 = 0x0040;
/// `LE_CSR65`: Next Tx Buffer addr upper.
pub const LE_CSR65: u32 = 0x0041;
/// `LE_CSR66`: Next Tx Byte Count.
pub const LE_CSR66: u32 = 0x0042;
/// `LE_CSR67`: Next Tx Status.
pub const LE_CSR67: u32 = 0x0043;
/// `LE_CSR72`: Receive Ring Counter.
pub const LE_CSR72: u32 = 0x0048;
/// `LE_CSR74`: Transmit Ring Counter.
pub const LE_CSR74: u32 = 0x004a;
/// `LE_CSR76`: Receive Ring Length.
pub const LE_CSR76: u32 = 0x004c;
/// `LE_CSR78`: Transmit Ring Length.
pub const LE_CSR78: u32 = 0x004e;
/// `LE_CSR80`: DMA Transfer Counter and FIFO Threshold Control.
pub const LE_CSR80: u32 = 0x0050;
/// `LE_CSR82`: Tx Desc addr Pointer lower.
pub const LE_CSR82: u32 = 0x0052;
/// `LE_CSR84`: DMA addr register lower.
pub const LE_CSR84: u32 = 0x0054;
/// `LE_CSR85`: DMA addr register upper.
pub const LE_CSR85: u32 = 0x0055;
/// `LE_CSR86`: Buffer Byte Counter.
pub const LE_CSR86: u32 = 0x0056;
/// `LE_CSR88`: Chip ID Register lower.
pub const LE_CSR88: u32 = 0x0058;
/// `LE_CSR89`: Chip ID Register upper.
pub const LE_CSR89: u32 = 0x0059;
/// `LE_CSR92`: Ring Length Conversion.
pub const LE_CSR92: u32 = 0x005c;
/// `LE_CSR100`: Bus Timeout.
pub const LE_CSR100: u32 = 0x0064;
/// `LE_CSR112`: Missed Frame Count.
pub const LE_CSR112: u32 = 0x0070;
/// `LE_CSR114`: Receive Collision Count.
pub const LE_CSR114: u32 = 0x0072;
/// `LE_CSR116`: OnNow Power Mode Register.
pub const LE_CSR116: u32 = 0x0074;
/// `LE_CSR122`: Advanced Feature Control.
pub const LE_CSR122: u32 = 0x007a;
/// `LE_CSR124`: Test Register 1.
pub const LE_CSR124: u32 = 0x007c;
/// `LE_CSR125`: MAC Enhanced Configuration Control.
pub const LE_CSR125: u32 = 0x007d;
/// `LE_BCR0`: Master Mode Read Active.
pub const LE_BCR0: u32 = 0x0000;
/// `LE_BCR1`: Master Mode Write Active.
pub const LE_BCR1: u32 = 0x0001;
/// `LE_BCR2`: Misc. Configuration.
pub const LE_BCR2: u32 = 0x0002;
/// `LE_BCR4`: LED0 Status.
pub const LE_BCR4: u32 = 0x0004;
/// `LE_BCR5`: LED1 Status.
pub const LE_BCR5: u32 = 0x0005;
/// `LE_BCR6`: LED2 Status.
pub const LE_BCR6: u32 = 0x0006;
/// `LE_BCR7`: LED3 Status.
pub const LE_BCR7: u32 = 0x0007;
/// `LE_BCR9`: Full-duplex Control.
pub const LE_BCR9: u32 = 0x0009;
/// `LE_BCR16`: I/O Base Address lower.
pub const LE_BCR16: u32 = 0x0010;
/// `LE_BCR17`: I/O Base Address upper.
pub const LE_BCR17: u32 = 0x0011;
/// `LE_BCR18`: Burst and Bus Control Register.
pub const LE_BCR18: u32 = 0x0012;
/// `LE_BCR19`: EEPROM Control and Status.
pub const LE_BCR19: u32 = 0x0013;
/// `LE_BCR20`: Software Style.
pub const LE_BCR20: u32 = 0x0014;
/// `LE_BCR22`: PCI Latency Register.
pub const LE_BCR22: u32 = 0x0016;
/// `LE_BCR23`: PCI Subsystem Vendor ID.
pub const LE_BCR23: u32 = 0x0017;
/// `LE_BCR24`: PCI Subsystem ID.
pub const LE_BCR24: u32 = 0x0018;
/// `LE_BCR25`: SRAM Size Register.
pub const LE_BCR25: u32 = 0x0019;
/// `LE_BCR26`: SRAM Boundary Register.
pub const LE_BCR26: u32 = 0x001a;
/// `LE_BCR27`: SRAM Interface Control Register.
pub const LE_BCR27: u32 = 0x001b;
/// `LE_BCR28`: Exp. Bus Port Addr lower.
pub const LE_BCR28: u32 = 0x001c;
/// `LE_BCR29`: Exp. Bus Port Addr upper.
pub const LE_BCR29: u32 = 0x001d;
/// `LE_BCR30`: Exp. Bus Data Port.
pub const LE_BCR30: u32 = 0x001e;
/// `LE_BCR31`: Software Timer Register.
pub const LE_BCR31: u32 = 0x001f;
/// `LE_BCR32`: PHY Control and Status Register.
pub const LE_BCR32: u32 = 0x0020;
/// `LE_BCR33`: PHY Address Register.
pub const LE_BCR33: u32 = 0x0021;
/// `LE_BCR34`: PHY Management Data Register.
pub const LE_BCR34: u32 = 0x0022;
/// `LE_BCR35`: PCI Vendor ID Register.
pub const LE_BCR35: u32 = 0x0023;
/// `LE_BCR36`: PCI Power Management Cap. Alias.
pub const LE_BCR36: u32 = 0x0024;
/// `LE_BCR37`: PCI DATA0 Alias.
pub const LE_BCR37: u32 = 0x0025;
/// `LE_BCR38`: PCI DATA1 Alias.
pub const LE_BCR38: u32 = 0x0026;
/// `LE_BCR39`: PCI DATA2 Alias.
pub const LE_BCR39: u32 = 0x0027;
/// `LE_BCR40`: PCI DATA3 Alias.
pub const LE_BCR40: u32 = 0x0028;
/// `LE_BCR41`: PCI DATA4 Alias.
pub const LE_BCR41: u32 = 0x0029;
/// `LE_BCR42`: PCI DATA5 Alias.
pub const LE_BCR42: u32 = 0x002a;
/// `LE_BCR43`: PCI DATA6 Alias.
pub const LE_BCR43: u32 = 0x002b;
/// `LE_BCR44`: PCI DATA7 Alias.
pub const LE_BCR44: u32 = 0x002c;
/// `LE_BCR45`: OnNow Pattern Matching 1.
pub const LE_BCR45: u32 = 0x002d;
/// `LE_BCR46`: OnNow Pattern Matching 2.
pub const LE_BCR46: u32 = 0x002e;
/// `LE_BCR47`: OnNow Pattern Matching 3.
pub const LE_BCR47: u32 = 0x002f;
/// `LE_BCR48`: LED4 Status.
pub const LE_BCR48: u32 = 0x0030;
/// `LE_BCR49`: PHY Select.
pub const LE_BCR49: u32 = 0x0031;

// Control and status register 0 (csr0)
/// `LE_C0_ERR`: error summary.
pub const LE_C0_ERR: u32 = 0x8000;
/// `LE_C0_BABL`: transmitter timeout error.
pub const LE_C0_BABL: u32 = 0x4000;
/// `LE_C0_CERR`: collision.
pub const LE_C0_CERR: u32 = 0x2000;
/// `LE_C0_MISS`: missed a packet.
pub const LE_C0_MISS: u32 = 0x1000;
/// `LE_C0_MERR`: memory error.
pub const LE_C0_MERR: u32 = 0x0800;
/// `LE_C0_RINT`: receiver interrupt.
pub const LE_C0_RINT: u32 = 0x0400;
/// `LE_C0_TINT`: transmitter interrupt.
pub const LE_C0_TINT: u32 = 0x0200;
/// `LE_C0_IDON`: initialization done.
pub const LE_C0_IDON: u32 = 0x0100;
/// `LE_C0_INTR`: interrupt condition.
pub const LE_C0_INTR: u32 = 0x0080;
/// `LE_C0_INEA`: interrupt enable.
pub const LE_C0_INEA: u32 = 0x0040;
/// `LE_C0_RXON`: receiver on.
pub const LE_C0_RXON: u32 = 0x0020;
/// `LE_C0_TXON`: transmitter on.
pub const LE_C0_TXON: u32 = 0x0010;
/// `LE_C0_TDMD`: transmit demand.
pub const LE_C0_TDMD: u32 = 0x0008;
/// `LE_C0_STOP`: disable all external activity.
pub const LE_C0_STOP: u32 = 0x0004;
/// `LE_C0_STRT`: enable external activity.
pub const LE_C0_STRT: u32 = 0x0002;
/// `LE_C0_INIT`: begin initialization.
pub const LE_C0_INIT: u32 = 0x0001;
/// `LE_C0_BITS`: the `%b` bit names.
pub const LE_C0_BITS: &str = "\x10\x10ERR\x0fBABL\x0eCERR\x0dMISS\x0cMERR\x0bRINT\x0aTINT\x09IDON\x08INTR\x07INEA\x06RXON\x05TXON\x04TDMD\x03STOP\x02STRT\x01INIT";

// Control and status register 3 (csr3)
/// `LE_C3_BABLM`: babble mask.
pub const LE_C3_BABLM: u32 = 0x4000;
/// `LE_C3_MISSM`: missed frame mask.
pub const LE_C3_MISSM: u32 = 0x1000;
/// `LE_C3_MERRM`: memory error mask.
pub const LE_C3_MERRM: u32 = 0x0800;
/// `LE_C3_RINTM`: receive interrupt mask.
pub const LE_C3_RINTM: u32 = 0x0400;
/// `LE_C3_TINTM`: transmit interrupt mask.
pub const LE_C3_TINTM: u32 = 0x0200;
/// `LE_C3_IDONM`: initialization done mask.
pub const LE_C3_IDONM: u32 = 0x0100;
/// `LE_C3_DXSUFLO`: disable tx stop on underflow.
pub const LE_C3_DXSUFLO: u32 = 0x0040;
/// `LE_C3_LAPPEN`: look ahead packet processing enbl.
pub const LE_C3_LAPPEN: u32 = 0x0020;
/// `LE_C3_DXMT2PD`: disable tx two part deferral.
pub const LE_C3_DXMT2PD: u32 = 0x0010;
/// `LE_C3_EMBA`: enable modified backoff algorithm.
pub const LE_C3_EMBA: u32 = 0x0008;
/// `LE_C3_BSWP`: byte swap.
pub const LE_C3_BSWP: u32 = 0x0004;
/// `LE_C3_ACON`: ALE control, eh?.
pub const LE_C3_ACON: u32 = 0x0002;
/// `LE_C3_BCON`: byte control.
pub const LE_C3_BCON: u32 = 0x0001;

// Control and status register 4 (csr4)
/// `LE_C4_EN124`: enable CSR124.
pub const LE_C4_EN124: u32 = 0x8000;
/// `LE_C4_DMAPLUS`: always set (PCnet-PCI).
pub const LE_C4_DMAPLUS: u32 = 0x4000;
/// `LE_C4_TIMER`: enable bus activity timer.
pub const LE_C4_TIMER: u32 = 0x2000;
/// `LE_C4_TXDPOLL`: disable transmit polling.
pub const LE_C4_TXDPOLL: u32 = 0x1000;
/// `LE_C4_APAD_XMT`: auto pad transmit.
pub const LE_C4_APAD_XMT: u32 = 0x0800;
/// `LE_C4_ASTRP_RCV`: auto strip receive.
pub const LE_C4_ASTRP_RCV: u32 = 0x0400;
/// `LE_C4_MFCO`: missed frame counter overflow.
pub const LE_C4_MFCO: u32 = 0x0200;
/// `LE_C4_MFCOM`: missed frame counter overflow mask.
pub const LE_C4_MFCOM: u32 = 0x0100;
/// `LE_C4_UINTCMD`: user interrupt command.
pub const LE_C4_UINTCMD: u32 = 0x0080;
/// `LE_C4_UINT`: user interrupt.
pub const LE_C4_UINT: u32 = 0x0040;
/// `LE_C4_RCVCCO`: receive collision counter overflow.
pub const LE_C4_RCVCCO: u32 = 0x0020;
/// `LE_C4_RCVCCOM`: receive collision counter overflow mask.
pub const LE_C4_RCVCCOM: u32 = 0x0010;
/// `LE_C4_TXSTRT`: transmit start status.
pub const LE_C4_TXSTRT: u32 = 0x0008;
/// `LE_C4_TXSTRTM`: transmit start mask.
pub const LE_C4_TXSTRTM: u32 = 0x0004;

// Control and status register 5 (csr5)
/// `LE_C5_TOKINTD`: transmit ok interrupt disable.
pub const LE_C5_TOKINTD: u32 = 0x8000;
/// `LE_C5_LTINTEN`: last transmit interrupt enable.
pub const LE_C5_LTINTEN: u32 = 0x4000;
/// `LE_C5_SINT`: system interrupt.
pub const LE_C5_SINT: u32 = 0x0800;
/// `LE_C5_SINTE`: system interrupt enable.
pub const LE_C5_SINTE: u32 = 0x0400;
/// `LE_C5_EXDINT`: excessive deferral interrupt.
pub const LE_C5_EXDINT: u32 = 0x0080;
/// `LE_C5_EXDINTE`: excessive deferral interrupt enbl.
pub const LE_C5_EXDINTE: u32 = 0x0040;
/// `LE_C5_MPPLBA`: magic packet physical logical broadcast accept.
pub const LE_C5_MPPLBA: u32 = 0x0020;
/// `LE_C5_MPINT`: magic packet interrupt.
pub const LE_C5_MPINT: u32 = 0x0010;
/// `LE_C5_MPINTE`: magic packet interrupt enable.
pub const LE_C5_MPINTE: u32 = 0x0008;
/// `LE_C5_MPEN`: magic packet enable.
pub const LE_C5_MPEN: u32 = 0x0004;
/// `LE_C5_MPMODE`: magic packet mode.
pub const LE_C5_MPMODE: u32 = 0x0002;
/// `LE_C5_SPND`: suspend.
pub const LE_C5_SPND: u32 = 0x0001;

// Control and status register 6 (csr6)
/// `LE_C6_TLEN`: TLEN from init block.
pub const LE_C6_TLEN: u32 = 0xf000;
/// `LE_C6_RLEN`: RLEN from init block.
pub const LE_C6_RLEN: u32 = 0x0f00;

// Control and status register 7 (csr7)
/// `LE_C7_FASTSPNDE`: fast suspend enable.
pub const LE_C7_FASTSPNDE: u32 = 0x8000;
/// `LE_C7_RDMD`: receive demand.
pub const LE_C7_RDMD: u32 = 0x2000;
/// `LE_C7_RDXPOLL`: receive disable polling.
pub const LE_C7_RDXPOLL: u32 = 0x1000;
/// `LE_C7_STINT`: software timer interrupt.
pub const LE_C7_STINT: u32 = 0x0800;
/// `LE_C7_STINTE`: software timer interrupt enable.
pub const LE_C7_STINTE: u32 = 0x0400;
/// `LE_C7_MREINT`: PHY management read error intr.
pub const LE_C7_MREINT: u32 = 0x0200;
/// `LE_C7_MREINTE`: PHY management read error intr enable.
pub const LE_C7_MREINTE: u32 = 0x0100;
/// `LE_C7_MAPINT`: PHY management auto-poll intr.
pub const LE_C7_MAPINT: u32 = 0x0080;
/// `LE_C7_MAPINTE`: PHY management auto-poll intr enable.
pub const LE_C7_MAPINTE: u32 = 0x0040;
/// `LE_C7_MCCINT`: PHY management command complete interrupt.
pub const LE_C7_MCCINT: u32 = 0x0020;
/// `LE_C7_MCCINTE`: PHY management command complete interrupt enable.
pub const LE_C7_MCCINTE: u32 = 0x0010;
/// `LE_C7_MCCIINT`: PHY management command complete internal interrupt.
pub const LE_C7_MCCIINT: u32 = 0x0008;
/// `LE_C7_MCCIINTE`: PHY management command complete internal interrupt enable.
pub const LE_C7_MCCIINTE: u32 = 0x0004;
/// `LE_C7_MIIPDTINT`: PHY management detect transition interrupt.
pub const LE_C7_MIIPDTINT: u32 = 0x0002;
/// `LE_C7_MIIPDTINTE`: PHY management detect transition interrupt enable.
pub const LE_C7_MIIPDTINTE: u32 = 0x0001;

// Control and status register 15 (csr15)
/// `LE_C15_PROM`: promiscuous mode.
pub const LE_C15_PROM: u32 = 0x8000;
/// `LE_C15_DRCVBC`: disable Rx of broadcast.
pub const LE_C15_DRCVBC: u32 = 0x4000;
/// `LE_C15_DRCVPA`: disable Rx of physical address.
pub const LE_C15_DRCVPA: u32 = 0x2000;
/// `LE_C15_DLNKTST`: disable link status.
pub const LE_C15_DLNKTST: u32 = 0x1000;
/// `LE_C15_DAPC`: disable auto-polarity correction.
pub const LE_C15_DAPC: u32 = 0x0800;
/// `LE_C15_MENDECL`: MENDEC Loopback mode.
pub const LE_C15_MENDECL: u32 = 0x0400;
/// `LE_C15_LRT`: low receive threshold (TMAU).
pub const LE_C15_LRT: u32 = 0x0200;
/// `LE_C15_TSEL`: transmit mode select (AUI).
pub const LE_C15_TSEL: u32 = 0x0200;
/// `LE_C15_PORTSEL(x)`: port select.
pub const fn le_c15_portsel(x: u32) -> u32 {
    x << 7
}
/// `LE_C15_INTL`: internal loopback.
pub const LE_C15_INTL: u32 = 0x0040;
/// `LE_C15_DRTY`: disable retry.
pub const LE_C15_DRTY: u32 = 0x0020;
/// `LE_C15_FCOLL`: force collision.
pub const LE_C15_FCOLL: u32 = 0x0010;
/// `LE_C15_DXMTFCS`: disable Tx FCS (ADD_FCS overrides).
pub const LE_C15_DXMTFCS: u32 = 0x0008;
/// `LE_C15_LOOP`: loopback enable.
pub const LE_C15_LOOP: u32 = 0x0004;
/// `LE_C15_DTX`: disable transmit.
pub const LE_C15_DTX: u32 = 0x0002;
/// `LE_C15_DRX`: disable receiver.
pub const LE_C15_DRX: u32 = 0x0001;
/// `PORTSEL_AUI`.
pub const PORTSEL_AUI: u32 = 0;
/// `PORTSEL_10T`.
pub const PORTSEL_10T: u32 = 1;
/// `PORTSEL_GPSI`.
pub const PORTSEL_GPSI: u32 = 2;
/// `PORTSEL_MII`.
pub const PORTSEL_MII: u32 = 3;
/// `PORTSEL_MASK`.
pub const PORTSEL_MASK: u32 = 3;

// control and status register 80 (csr80)
/// `LE_C80_RCVFW(x)`: Receive FIFO Watermark.
pub const fn le_c80_rcvfw(x: u32) -> u32 {
    x << 12
}
/// `LE_C80_RCVFW_MAX`.
pub const LE_C80_RCVFW_MAX: u32 = 3;
/// `LE_C80_XMTSP(x)`: Transmit Start Point.
pub const fn le_c80_xmtsp(x: u32) -> u32 {
    x << 10
}
/// `LE_C80_XMTSP_MAX`.
pub const LE_C80_XMTSP_MAX: u32 = 3;
/// `LE_C80_XMTFW(x)`: Transmit FIFO Watermark.
pub const fn le_c80_xmtfw(x: u32) -> u32 {
    x << 8
}
/// `LE_C80_XMTFW_MAX`.
pub const LE_C80_XMTFW_MAX: u32 = 3;
/// `LE_C80_DMATC`: DMA transfer counter.
pub const LE_C80_DMATC: u32 = 0x00ff;

// control and status register 116 (csr116)
/// `LE_C116_PME_EN_OVR`: PME_EN overwrite.
pub const LE_C116_PME_EN_OVR: u32 = 0x0400;
/// `LE_C116_LCDET`: link change detected.
pub const LE_C116_LCDET: u32 = 0x0200;
/// `LE_C116_LCMODE`: link change wakeup mode.
pub const LE_C116_LCMODE: u32 = 0x0100;
/// `LE_C116_PMAT`: pattern matched.
pub const LE_C116_PMAT: u32 = 0x0080;
/// `LE_C116_EMPPLBA`: magic packet physical logical broadcast accept.
pub const LE_C116_EMPPLBA: u32 = 0x0040;
/// `LE_C116_MPMAT`: magic packet match.
pub const LE_C116_MPMAT: u32 = 0x0020;
/// `LE_C116_MPPEN`: magic packet pin enable.
pub const LE_C116_MPPEN: u32 = 0x0010;
/// `LE_C116_RST_POL`: PHY_RST pin polarity.
pub const LE_C116_RST_POL: u32 = 0x0001;

// control and status register 122 (csr122)
/// `LE_C122_RCVALGN`: receive packet align.
pub const LE_C122_RCVALGN: u32 = 0x0001;

// control and status register 124 (csr124)
/// `LE_C124_RPA`: runt packet accept.
pub const LE_C124_RPA: u32 = 0x0008;

// control and status register 125 (csr125)
/// `LE_C125_IPG`: inter-packet gap.
pub const LE_C125_IPG: u32 = 0xff00;
/// `LE_C125_IFS1`: inter-frame spacing part 1.
pub const LE_C125_IFS1: u32 = 0x00ff;

// bus configuration register 0 (bcr0)
/// `LE_B0_MSRDA`: reserved locations.
pub const LE_B0_MSRDA: u32 = 0xffff;

// bus configuration register 1 (bcr1)
/// `LE_B1_MSWRA`: reserved locations.
pub const LE_B1_MSWRA: u32 = 0xffff;

// bus configuration register 2 (bcr2)
/// `LE_B2_PHYSSELEN`: enable writes to BCR18[4:3].
pub const LE_B2_PHYSSELEN: u32 = 0x2000;
/// `LE_B2_LEDPE`: LED program enable.
pub const LE_B2_LEDPE: u32 = 0x1000;
/// `LE_B2_APROMWE`: Address PROM Write Enable.
pub const LE_B2_APROMWE: u32 = 0x0100;
/// `LE_B2_INTLEVEL`: 1 == edge triggered.
pub const LE_B2_INTLEVEL: u32 = 0x0080;
/// `LE_B2_DXCVRCTL`: DXCVR control.
pub const LE_B2_DXCVRCTL: u32 = 0x0020;
/// `LE_B2_DXCVRPOL`: DXCVR polarity.
pub const LE_B2_DXCVRPOL: u32 = 0x0010;
/// `LE_B2_EADISEL`: EADI select.
pub const LE_B2_EADISEL: u32 = 0x0008;
/// `LE_B2_AWAKE`: power saving mode select.
pub const LE_B2_AWAKE: u32 = 0x0004;
/// `LE_B2_ASEL`: auto-select PORTSEL.
pub const LE_B2_ASEL: u32 = 0x0002;
/// `LE_B2_XMAUSEL`: reserved location.
pub const LE_B2_XMAUSEL: u32 = 0x0001;

// bus configuration register 4 (bcr4)

// bus configuration register 5 (bcr5)

// bus configuration register 6 (bcr6)

// bus configuration register 7 (bcr7)

// bus configuration register 48 (bcr48)
/// `LE_B4_LEDOUT`: LED output active.
pub const LE_B4_LEDOUT: u32 = 0x8000;
/// `LE_B4_LEDPOL`: LED polarity.
pub const LE_B4_LEDPOL: u32 = 0x4000;
/// `LE_B4_LEDDIS`: LED disable.
pub const LE_B4_LEDDIS: u32 = 0x2000;
/// `LE_B4_100E`: 100Mb/s enable.
pub const LE_B4_100E: u32 = 0x1000;
/// `LE_B4_MPSE`: magic packet status enable.
pub const LE_B4_MPSE: u32 = 0x0200;
/// `LE_B4_FDLSE`: full-duplex link status enable.
pub const LE_B4_FDLSE: u32 = 0x0100;
/// `LE_B4_PSE`: pulse stretcher enable.
pub const LE_B4_PSE: u32 = 0x0080;
/// `LE_B4_LNKSE`: link status enable.
pub const LE_B4_LNKSE: u32 = 0x0040;
/// `LE_B4_RCVME`: receive match status enable.
pub const LE_B4_RCVME: u32 = 0x0020;
/// `LE_B4_XMTE`: transmit status enable.
pub const LE_B4_XMTE: u32 = 0x0010;
/// `LE_B4_POWER`: power enable.
pub const LE_B4_POWER: u32 = 0x0008;
/// `LE_B4_RCVE`: receive status enable.
pub const LE_B4_RCVE: u32 = 0x0004;
/// `LE_B4_SPEED`: high speed enable.
pub const LE_B4_SPEED: u32 = 0x0002;
/// `LE_B4_COLE`: collision status enable.
pub const LE_B4_COLE: u32 = 0x0001;

// bus configuration register 9 (bcr9)
/// `LE_B9_FDRPAD`: full-duplex runt packet accept disable.
pub const LE_B9_FDRPAD: u32 = 0x0004;
/// `LE_B9_AUIFD`: AUI full-duplex.
pub const LE_B9_AUIFD: u32 = 0x0002;
/// `LE_B9_FDEN`: full-duplex enable.
pub const LE_B9_FDEN: u32 = 0x0001;

// bus configuration register 18 (bcr18)
/// `LE_B18_ROMTMG`: expansion rom timing.
pub const LE_B18_ROMTMG: u32 = 0xf000;
/// `LE_B18_NOUFLO`: no underflow on transmit.
pub const LE_B18_NOUFLO: u32 = 0x0800;
/// `LE_B18_MEMCMD`: memory read multiple enable.
pub const LE_B18_MEMCMD: u32 = 0x0200;
/// `LE_B18_EXTREQ`: extended request.
pub const LE_B18_EXTREQ: u32 = 0x0100;
/// `LE_B18_DWIO`: double-word I/O.
pub const LE_B18_DWIO: u32 = 0x0080;
/// `LE_B18_BREADE`: burst read enable.
pub const LE_B18_BREADE: u32 = 0x0040;
/// `LE_B18_BWRITE`: burst write enable.
pub const LE_B18_BWRITE: u32 = 0x0020;
/// `LE_B18_PHYSEL1`: PHYSEL 1.
pub const LE_B18_PHYSEL1: u32 = 0x0010;
/// `LE_B18_PHYSEL0`: PHYSEL 0.
pub const LE_B18_PHYSEL0: u32 = 0x0008;
/// `LE_B18_LINBC`: reserved locations.
pub const LE_B18_LINBC: u32 = 0x0007;

// bus configuration register 19 (bcr19)
/// `LE_B19_PVALID`: EEPROM status valid.
pub const LE_B19_PVALID: u32 = 0x8000;
/// `LE_B19_PREAD`: EEPROM read command.
pub const LE_B19_PREAD: u32 = 0x4000;
/// `LE_B19_EEDET`: EEPROM detect.
pub const LE_B19_EEDET: u32 = 0x2000;
/// `LE_B19_EEN`: EEPROM port enable.
pub const LE_B19_EEN: u32 = 0x0010;
/// `LE_B19_ECS`: EEPROM chip select.
pub const LE_B19_ECS: u32 = 0x0004;
/// `LE_B19_ESK`: EEPROM serial clock.
pub const LE_B19_ESK: u32 = 0x0002;
/// `LE_B19_EDI`: EEPROM data in.
pub const LE_B19_EDI: u32 = 0x0001;
/// `LE_B19_EDO`: EEPROM data out.
pub const LE_B19_EDO: u32 = 0x0001;

// bus configuration register 20 (bcr20)
/// `LE_B20_APERREN`: Advanced parity error handling.
pub const LE_B20_APERREN: u32 = 0x0400;
/// `LE_B20_CSRPCNET`: PCnet-style CSRs (0 = ILACC).
pub const LE_B20_CSRPCNET: u32 = 0x0200;
/// `LE_B20_SSIZE32`: Software Size 32-bit.
pub const LE_B20_SSIZE32: u32 = 0x0100;
/// `LE_B20_SSTYLE`: Software Style.
pub const LE_B20_SSTYLE: u32 = 0x0007;
/// `LE_B20_SSTYLE_LANCE`: LANCE/PCnet-ISA (16-bit).
pub const LE_B20_SSTYLE_LANCE: u32 = 0;
/// `LE_B20_SSTYPE_ILACC`: ILACC (32-bit).
pub const LE_B20_SSTYPE_ILACC: u32 = 1;
/// `LE_B20_SSTYLE_PCNETPCI2`: PCnet-PCI (32-bit).
pub const LE_B20_SSTYLE_PCNETPCI2: u32 = 2;
/// `LE_B20_SSTYLE_PCNETPCI3`: PCnet-PCI II (32-bit).
pub const LE_B20_SSTYLE_PCNETPCI3: u32 = 3;

// bus configuration register 25 (bcr25)
/// `LE_B25_SRAM_SIZE`: SRAM size.
pub const LE_B25_SRAM_SIZE: u32 = 0x00ff;

// bus configuration register 26 (bcr26)
/// `LE_B26_SRAM_BND`: SRAM boundary.
pub const LE_B26_SRAM_BND: u32 = 0x00ff;

// bus configuration register 27 (bcr27)
/// `LE_B27_PTRTST`: reserved for manuf. tests.
pub const LE_B27_PTRTST: u32 = 0x8000;
/// `LE_B27_LOLATRX`: low latency receive.
pub const LE_B27_LOLATRX: u32 = 0x4000;
/// `LE_B27_EBCS`: expansion bus clock source.
pub const LE_B27_EBCS: u32 = 0x0038;
/// `LE_B27_CLK_FAC`: clock factor.
pub const LE_B27_CLK_FAC: u32 = 0x0007;

// bus configuration register 28 (bcr28)
/// `LE_B28_EADDRL`: expansion port address lower.
pub const LE_B28_EADDRL: u32 = 0xffff;

// bus configuration register 29 (bcr29)
/// `LE_B29_FLASH`: flash access.
pub const LE_B29_FLASH: u32 = 0x8000;
/// `LE_B29_LAAINC`: lower address auto increment.
pub const LE_B29_LAAINC: u32 = 0x4000;
/// `LE_B29_EPADDRU`: expansion port address upper.
pub const LE_B29_EPADDRU: u32 = 0x0007;

// bus configuration register 30 (bcr30)
/// `LE_B30_EBDATA`: expansion bus data port.
pub const LE_B30_EBDATA: u32 = 0xffff;

// bus configuration register 31 (bcr31)
/// `LE_B31_STVAL`: software timer value.
pub const LE_B31_STVAL: u32 = 0xffff;

// bus configuration register 32 (bcr32)
/// `LE_B32_ANTST`: reserved for manuf. tests.
pub const LE_B32_ANTST: u32 = 0x8000;
/// `LE_B32_MIIPD`: MII PHY Detect (manuf. tests).
pub const LE_B32_MIIPD: u32 = 0x4000;
/// `LE_B32_FMDC`: fast management data clock.
pub const LE_B32_FMDC: u32 = 0x3000;
/// `LE_B32_APEP`: auto-poll PHY.
pub const LE_B32_APEP: u32 = 0x0800;
/// `LE_B32_APDW`: auto-poll dwell time.
pub const LE_B32_APDW: u32 = 0x0700;
/// `LE_B32_DANAS`: disable autonegotiation.
pub const LE_B32_DANAS: u32 = 0x0080;
/// `LE_B32_XPHYRST`: PHY reset.
pub const LE_B32_XPHYRST: u32 = 0x0040;
/// `LE_B32_XPHYANE`: PHY autonegotiation enable.
pub const LE_B32_XPHYANE: u32 = 0x0020;
/// `LE_B32_XPHYFD`: PHY full-duplex.
pub const LE_B32_XPHYFD: u32 = 0x0010;
/// `LE_B32_XPHYSP`: PHY speed.
pub const LE_B32_XPHYSP: u32 = 0x0008;
/// `LE_B32_MIIILP`: MII internal loopback.
pub const LE_B32_MIIILP: u32 = 0x0002;

// bus configuration register 33 (bcr33)
/// `LE_B33_SHADOW`: shadow enable.
pub const LE_B33_SHADOW: u32 = 0x8000;
/// `LE_B33_MII_SEL`: MII selected.
pub const LE_B33_MII_SEL: u32 = 0x4000;
/// `LE_B33_ACOMP`: internal PHY autonegotiation comp.
pub const LE_B33_ACOMP: u32 = 0x2000;
/// `LE_B33_LINK`: link status.
pub const LE_B33_LINK: u32 = 0x1000;
/// `LE_B33_FDX`: full-duplex.
pub const LE_B33_FDX: u32 = 0x0800;
/// `LE_B33_SPEED`: 1 == high speed.
pub const LE_B33_SPEED: u32 = 0x0400;
/// `LE_B33_PHYAD`: PHY address.
pub const LE_B33_PHYAD: u32 = 0x03e0;
/// `PHYAD_SHIFT`.
pub const PHYAD_SHIFT: u32 = 5;
/// `LE_B33_REGAD`: register address.
pub const LE_B33_REGAD: u32 = 0x001f;

// bus configuration register 34 (bcr34)
/// `LE_B34_MIIMD`: MII data.
pub const LE_B34_MIIMD: u32 = 0xffff;

// bus configuration register 49 (bcr49)
/// `LE_B49_PCNET`: PCnet mode - Must Be One.
pub const LE_B49_PCNET: u32 = 0x8000;
/// `LE_B49_PHYSEL_D`: PHY_SEL_Default.
pub const LE_B49_PHYSEL_D: u32 = 0x0300;
/// `LE_B49_PHYSEL_L`: PHY_SEL_Lock.
pub const LE_B49_PHYSEL_L: u32 = 0x0010;
/// `LE_B49_PHYSEL`: PHYSEL.
pub const LE_B49_PHYSEL: u32 = 0x0003;
/// `LE_MODE_PROM`: promiscuous mode.
pub const LE_MODE_PROM: u32 = 0x8000;
/// `LE_MODE_DRCVBC`: disable receive broadcast.
pub const LE_MODE_DRCVBC: u32 = 0x4000;
/// `LE_MODE_DRCVPA`: disable physical address detection.
pub const LE_MODE_DRCVPA: u32 = 0x2000;
/// `LE_MODE_DLNKTST`: disable link status.
pub const LE_MODE_DLNKTST: u32 = 0x1000;
/// `LE_MODE_DAPC`: disable automatic polarity correction.
pub const LE_MODE_DAPC: u32 = 0x0800;
/// `LE_MODE_MENDECL`: MENDEC loopback mode.
pub const LE_MODE_MENDECL: u32 = 0x0400;
/// `LE_MODE_LRTTSEL`: lower receive threshold / transmit mode selection.
pub const LE_MODE_LRTTSEL: u32 = 0x0200;
/// `LE_MODE_PSEL1`: port selection bit1.
pub const LE_MODE_PSEL1: u32 = 0x0100;
/// `LE_MODE_PSEL0`: port selection bit0.
pub const LE_MODE_PSEL0: u32 = 0x0080;
/// `LE_MODE_INTL`: internal loopback.
pub const LE_MODE_INTL: u32 = 0x0040;
/// `LE_MODE_DRTY`: disable retry.
pub const LE_MODE_DRTY: u32 = 0x0020;
/// `LE_MODE_COLL`: force a collision.
pub const LE_MODE_COLL: u32 = 0x0010;
/// `LE_MODE_DTCR`: disable transmit CRC.
pub const LE_MODE_DTCR: u32 = 0x0008;
/// `LE_MODE_LOOP`: loopback mode.
pub const LE_MODE_LOOP: u32 = 0x0004;
/// `LE_MODE_DTX`: disable transmitter.
pub const LE_MODE_DTX: u32 = 0x0002;
/// `LE_MODE_DRX`: disable receiver.
pub const LE_MODE_DRX: u32 = 0x0001;
/// `LE_MODE_NORMAL`: none of the above.
pub const LE_MODE_NORMAL: u32 = 0;
/// `CHIPID_MANFID(x)`.
pub const fn chipid_manfid(x: u32) -> u32 {
    (x >> 1) & 0x3ff
}
/// `CHIPID_PARTID(x)`.
pub const fn chipid_partid(x: u32) -> u32 {
    (x >> 12) & 0xffff
}
/// `CHIPID_VER(x)`.
pub const fn chipid_ver(x: u32) -> u32 {
    (x >> 28) & 0x7
}
/// `PARTID_Am79c960`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c960: u32 = 0x0003;
/// `PARTID_Am79c961`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c961: u32 = 0x2260;
/// `PARTID_Am79c961A`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c961A: u32 = 0x2261;
/// `PARTID_Am79c965`: yes, these....
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c965: u32 = 0x2430;
/// `PARTID_Am79c970`: ...are the same.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c970: u32 = 0x2430;
/// `PARTID_Am79c970A`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c970A: u32 = 0x2621;
/// `PARTID_Am79c971`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c971: u32 = 0x2623;
/// `PARTID_Am79c972`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c972: u32 = 0x2624;
/// `PARTID_Am79c973`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c973: u32 = 0x2625;
/// `PARTID_Am79c978`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c978: u32 = 0x2626;
/// `PARTID_Am79c975`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c975: u32 = 0x2627;
/// `PARTID_Am79c976`.
#[allow(non_upper_case_globals)] // the C name
pub const PARTID_Am79c976: u32 = 0x2628;
/* </CODE> */
