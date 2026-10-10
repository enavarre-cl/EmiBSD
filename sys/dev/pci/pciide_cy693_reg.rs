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
 * Copyright (c) 1998 Manuel Bouyer.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! Contaq/Cypress CY82C693 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_cy693_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `cy_dma_pulse` and `cy_dma_rec` (under `#ifdef unused`) are not compiled, as in the C.
//! - `struct pciide_cy` is [`PciideCy`]; its handle is a `&'static` [`Cy82c693Handle`].
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

use crate::dev::pci::cy82c693var::Cy82c693Handle;
use core::cell::Cell;

// Registers definitions for Contaq/Cypress's CY82693U PCI IDE controller.
// Available from http://www.cypress.com/japan/prodgate/chip/cy82c693.html
// This chip has 2 PCI IDE functions, each of them has only one channel
// So there's no primary/secondary distinction in the registers defs.

// IDE control register
/// `CY_CTRL`.
pub const CY_CTRL: u32 = 0x40;
/// `CY_CTRL_RETRY`.
pub const CY_CTRL_RETRY: u32 = 0x00002000;
/// `CY_CTRL_SLAVE_PREFETCH`.
pub const CY_CTRL_SLAVE_PREFETCH: u32 = 0x00000400;
/// `CY_CTRL_POSTWRITE`.
pub const CY_CTRL_POSTWRITE: u32 = 0x00000200;
/// `CY_CTRL_PREFETCH`.
pub const fn cy_ctrl_prefetch(drive: i32) -> u32 {
    0x00000100 << (2 * drive)
}
/// `CY_CTRL_POSTWRITE_LENGTH_MASK`.
pub const CY_CTRL_POSTWRITE_LENGTH_MASK: u32 = 0x00000030;
/// `CY_CTRL_POSTWRITE_LENGTH_OFF`.
pub const CY_CTRL_POSTWRITE_LENGTH_OFF: i32 = 4;
/// `CY_CTRL_PREFETCH_LENGTH_MASK`.
pub const CY_CTRL_PREFETCH_LENGTH_MASK: u32 = 0x00000003;
/// `CY_CTRL_PREFETCH_LENGTH_OFF`.
pub const CY_CTRL_PREFETCH_LENGTH_OFF: i32 = 0;

// IDE addr setup control register
/// `CY_ADDR_CTRL`.
pub const CY_ADDR_CTRL: u32 = 0x48;
/// `CY_ADDR_CTRL_SETUP_OFF`.
pub const fn cy_addr_ctrl_setup_off(drive: i32) -> i32 {
    4 * drive
}
/// `CY_ADDR_CTRL_SETUP_MASK`.
pub const fn cy_addr_ctrl_setup_mask(drive: i32) -> u32 {
    0x00000007 << cy_addr_ctrl_setup_off(drive)
}

// command control register
/// `CY_CMD_CTRL`.
pub const CY_CMD_CTRL: i32 = 0x4c;
/// `CY_CMD_CTRL_IOW_PULSE_OFF`.
pub const fn cy_cmd_ctrl_iow_pulse_off(drive: i32) -> i32 {
    12 + 16 * drive
}
/// `CY_CMD_CTRL_IOW_REC_OFF`.
pub const fn cy_cmd_ctrl_iow_rec_off(drive: i32) -> i32 {
    8 + 16 * drive
}
/// `CY_CMD_CTRL_IOR_PULSE_OFF`.
pub const fn cy_cmd_ctrl_ior_pulse_off(drive: i32) -> i32 {
    4 + 16 * drive
}
/// `CY_CMD_CTRL_IOR_REC_OFF`.
pub const fn cy_cmd_ctrl_ior_rec_off(drive: i32) -> i32 {
    16 * drive
}

/// `cy_pio_pulse`.
pub const CY_PIO_PULSE: [u8; 5] = [9, 4, 3, 2, 2];
/// `cy_pio_rec`.
pub const CY_PIO_REC: [u8; 5] = [9, 7, 4, 2, 0];
// #ifdef unused: static int8_t cy_dma_pulse[] = {7, 2, 2}; static int8_t cy_dma_rec[] = {7, 1, 0};

// The cypress is quite weird: it uses 8-bit ISA registers to control
// DMA modes.

/// `CY_DMA_ADDR`.
pub const CY_DMA_ADDR: u32 = 0x22;
/// `CY_DMA_SIZE`.
pub const CY_DMA_SIZE: u32 = 0x2;

/// `CY_DMA_IDX`.
pub const CY_DMA_IDX: u32 = 0x00;
/// `CY_DMA_IDX_PRIMARY`.
pub const CY_DMA_IDX_PRIMARY: u32 = 0x30;
/// `CY_DMA_IDX_SECONDARY`.
pub const CY_DMA_IDX_SECONDARY: u32 = 0x31;
/// `CY_DMA_IDX_TIMEOUT`.
pub const CY_DMA_IDX_TIMEOUT: u32 = 0x32;

/// `CY_DMA_DATA`.
pub const CY_DMA_DATA: u32 = 0x01;
// Multiword DMA transfer, for CY_DMA_IDX_PRIMARY or CY_DMA_IDX_SECONDARY
/// `CY_DMA_DATA_MODE_MASK`.
pub const CY_DMA_DATA_MODE_MASK: u32 = 0x03;
/// `CY_DMA_DATA_SINGLE`.
pub const CY_DMA_DATA_SINGLE: u32 = 0x04;

// Private data
/// `struct pciide_cy`.
pub struct PciideCy {
    /// `cy_handle`.
    pub cy_handle: Cell<Option<&'static Cy82c693Handle>>,
    /// `cy_compatchan`.
    pub cy_compatchan: Cell<i32>,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_cy693_reg.h");

        crate::reftest::assert_defines!(defs; CY_CTRL, CY_CTRL_RETRY, CY_CTRL_SLAVE_PREFETCH, CY_CTRL_POSTWRITE, CY_CTRL_POSTWRITE_LENGTH_MASK, CY_CTRL_POSTWRITE_LENGTH_OFF, CY_CTRL_PREFETCH_LENGTH_MASK, CY_CTRL_PREFETCH_LENGTH_OFF, CY_ADDR_CTRL, CY_CMD_CTRL, CY_DMA_ADDR, CY_DMA_SIZE, CY_DMA_IDX, CY_DMA_IDX_PRIMARY, CY_DMA_IDX_SECONDARY, CY_DMA_IDX_TIMEOUT, CY_DMA_DATA, CY_DMA_DATA_MODE_MASK, CY_DMA_DATA_SINGLE);
    }
}
/* </TESTS> */
