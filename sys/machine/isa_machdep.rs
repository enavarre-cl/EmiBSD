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
/* </LICENSES> */

/* <CODE> */
//! `<machine/isa_machdep.h>` as a trait: what the ISA bus (`dev/isa/isa.c`) and its drivers
//! (`com_isa.c`) need from the machine.
//!
//! amd64 has an ISA bus behind its legacy 8259s (`arch/amd64/isa/isa_machdep.c`); arm64 has
//! none, and answers as a machine without one would: no interrupt can be established, no
//! line is free.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::machine::Machine;
use crate::sys::device::Device;

/// The ISA side of the machine.
pub trait IsaMachdep {
    /// `isa_chipset_tag_t`.
    type IsaChipsetTag: Copy;

    /// `IST_NONE`: none (`<machine/intr.h>`'s interrupt sharing types).
    const IST_NONE: i32;
    /// `IST_PULSE`: pulsed.
    const IST_PULSE: i32;
    /// `IST_EDGE`: edge-triggered.
    const IST_EDGE: i32;
    /// `IST_LEVEL`: level-triggered.
    const IST_LEVEL: i32;

    /// `isa_attach_hook(parent, self, iba)`: the machine's look at the ISA bus when it
    /// attaches.
    fn isa_attach_hook(parent: Option<&Device>, self_: &Device);

    /// `isa_intr_check(ic, irq, type)`: 0 when `irq` cannot take a `type` interrupt, 1 when
    /// it is shared, 2 when it is free.
    fn isa_intr_check(ic: Self::IsaChipsetTag, irq: i32, type_: i32) -> i32;

    /// `isa_intr_establish(ic, irq, type, level, fun, arg, what)`: the handler's cookie, NULL
    /// when it cannot be established.
    fn isa_intr_establish(
        ic: Self::IsaChipsetTag,
        irq: i32,
        type_: i32,
        level: i32,
        ih_fun: fn(*mut c_void) -> i32,
        ih_arg: *mut c_void,
        ih_what: &'static str,
    ) -> Option<NonNull<c_void>>;
}

/// `isa_chipset_tag_t` on the selected machine.
pub type IsaChipsetTag = <Machine as IsaMachdep>::IsaChipsetTag;

/// `IST_NONE` on the selected machine.
pub const IST_NONE: i32 = <Machine as IsaMachdep>::IST_NONE;
/// `IST_PULSE` on the selected machine.
pub const IST_PULSE: i32 = <Machine as IsaMachdep>::IST_PULSE;
/// `IST_EDGE` on the selected machine.
pub const IST_EDGE: i32 = <Machine as IsaMachdep>::IST_EDGE;
/// `IST_LEVEL` on the selected machine.
pub const IST_LEVEL: i32 = <Machine as IsaMachdep>::IST_LEVEL;

/// `isa_attach_hook` on the selected machine.
pub fn isa_attach_hook(parent: Option<&Device>, self_: &Device) {
    Machine::isa_attach_hook(parent, self_)
}

/// `isa_intr_check` on the selected machine.
pub fn isa_intr_check(ic: IsaChipsetTag, irq: i32, type_: i32) -> i32 {
    Machine::isa_intr_check(ic, irq, type_)
}

/// `isa_intr_establish` on the selected machine.
pub fn isa_intr_establish(
    ic: IsaChipsetTag,
    irq: i32,
    type_: i32,
    level: i32,
    ih_fun: fn(*mut c_void) -> i32,
    ih_arg: *mut c_void,
    ih_what: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::isa_intr_establish(ic, irq, type_, level, ih_fun, ih_arg, ih_what)
}
/* </CODE> */
