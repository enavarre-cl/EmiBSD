/* $OpenBSD: dsdt.h,v 1.82 2024/05/13 01:15:50 jsg Exp $ */
/* $OpenBSD: dsdt.c,v 1.281 2026/07/31 05:13:46 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
 * Copyright (c) 2005 Jordan Hargrave <jordan@openbsd.org>
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

//! The AML interpreter: `<dev/acpi/dsdt.h>` (the parser's scope, the resource descriptors,
//! the `_OSI` table) and `dev/acpi/dsdt.c` (the namespace, the value routines, the parser
//! and evaluator, operation regions and fields, and the external API the ACPI drivers call:
//! `aml_evalnode`, `aml_evalname`, `aml_evalinteger`, `aml_searchname`, `aml_find_node`,
//! `aml_walknodes`, `aml_parse_resource`, ...).
//!
//! Upstream: sys/dev/acpi/dsdt.h @ 3ce1f3f79392
//! Upstream: sys/dev/acpi/dsdt.c @ 3ce1f3f79392
//!
//! `acpi.c` loads the DSDT and the SSDTs and hands their byte code to [`acpi_parse_aml`],
//! which builds the namespace under [`aml_root`] by running the tables' top-level terms.
//! Drivers then find their devices ([`aml_find_node`], [`aml_searchname`]) and evaluate
//! names and methods ([`aml_evalname`], [`aml_evalinteger`]); methods read and write
//! hardware through operation regions, whose accesses go to the handler registered for the
//! region's address space ([`aml_register_regionspace`]; system memory, system I/O, PCI
//! configuration space and the embedded controller default to `acpi_gasio`).
//!
//! The parser is the C's: `aml_parse` decodes one opcode, parses its arguments as the
//! opcode table's argument string says (recursing for terms), executes it, and, for a term
//! list (`'T'`), keeps going in the same frame, pushing a scope for `If`/`While`/`Scope`/
//! `Device` bodies and popping it at its end. Values are [`AmlValueRef`]s (`Rc`), nodes
//! [`AmlNodeRef`]s (`Rc`), scopes `Rc<AmlScope>`; see `dev/acpi/amltypes.rs` for how the C's
//! reference counts map onto them.
//!
//! All of this state is global and not thread-safe, as in C: every path into the
//! interpreter runs under the kernel lock (see [`AmlGlobal`]).
//!
//! ## Deviations
//! - `ACPI_DEBUG` and `ACPI_MEMDEBUG` are not configured: `dnprintf`, `aml_dump`,
//!   `aml_showstack`, `aml_print_resource` and `acpi_walkmem` are compiled out, as in C.
//!   `SMALL_KERNEL` is not defined, so `aml_showvalue`, `aml_val_to_string`,
//!   `acpi_getdevlist` and the full `aml_rwgsb` are here.
//! - `aml_disasm` and `aml_disprintf` (`#ifdef DDB`) are called only by `acpidebug.c`
//!   (ddb's `machine acpi` commands): they wait for that file's port.
//! - Memory comes from Rust's allocator (`kern/rust_alloc.rs`, `M_TEMP`) instead of
//!   `acpi_os_malloc` (`M_ACPI`); `acpi_nalloc`, `struct acpi_memblock` and
//!   `_acpi_os_malloc`/`_acpi_os_free` are therefore not needed. `aml_addref`/`aml_delref`
//!   are `Rc` clones and drops. Where the C leaks a reference (`aml_compare`'s converted
//!   operand, `aml_createfield`'s converted buffer) or keeps a dangling one, the value is
//!   simply dropped or kept alive.
//! - `aml_hashopcodes` has nothing left to do: the perfect hash of `aml_table[]` is built
//!   at compile time (`AML_OPHASH`).
//! - The static buffers the C returns (`aml_nodename`, `aml_getname`, `aml_mnem`,
//!   `aml_eisaid`, `aml_val_to_string`) are returned by value.
//! - Callbacks with a `void *arg` (`aml_find_node`, `aml_walknodes`, `aml_parse_resource`,
//!   `aml_foreachpkg`) take closures; `aml_register_notify`'s callback keeps its
//!   `void *arg`, as the drivers register long-lived softcs.
//! - `union acpi_resource` is [`AcpiResource`], a view of the descriptor's bytes with one
//!   accessor per member (`crs.sr_irq_irq_mask()` for `crs->sr_irq.irq_mask`); reads past
//!   the descriptor's length give zeros, which is what `aml_mapresource`'s zeroed copy
//!   gives the C callbacks.
//! - A read past the end of an AML table reads zeros (`AmlPtr`); the C reads past it.
//!   `Buffer(n){...}` with an initializer longer than `n` copies `n` bytes (the C overruns
//!   the allocation). Shifts by 64 or more wrap the count, as the amd64 instruction does;
//!   overflowing sums wrap, as C's unsigned arithmetic does. Bit copies into a buffer stop
//!   at its end.
//! - A buffer field wider than an integer reads as a buffer and is written from a buffer
//!   (the C copies its bits into and out of the 8-byte `v_integer`, past its end).
//! - `AML_ARG_CONST` turns `Ones` (`0xFF`) into -1 through `(char)opcode`; that is amd64's
//!   signed `char`. On arm64, where `char` is unsigned, the C makes 255; here both archs
//!   make -1.
//! - Paths that dereference NULL in C (no `acpi_softc` yet, a scope without a node, an
//!   operand missing after a parse error) do nothing or use an uninitialised value
//!   instead: `aml_notify` and `acpi_poll` need acpi0, `acpi_glk_enter`/`leave` and
//!   `acpi_event_wait`'s task loop need it too (without it the event wait is the cold
//!   path's `delay` loop).
//! - `acpi_gasio`, `acpi_addtask`, `acpi_dotask`, `acpi_read_pmreg`/`acpi_write_pmreg`,
//!   `acpi_maptable` and the global lock's `acpi_acquire_glk`/`acpi_release_glk` are
//!   `acpi.c`'s and `acpi_machdep.c`'s: visible stubs in `dev/acpi/acpi.rs` until those
//!   files are ported.
//! - `aml_rwgsb` never finds an I2C controller (`struct aml_node` has no `i2c` until
//!   `dev/i2c` is ported) and answers `EIO` in the status byte, as the C does when the
//!   controller is missing.
//! - `aml_evalnode`'s `argv` is a slice (the C's `argc` is its length); `res` is an
//!   [`AmlValue`] the caller owns, as in C.

use alloc::rc::{Rc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicI64, Ordering};

use super::acpi::{
    ACPI_POLL_ENABLED, acpi_acquire_glk, acpi_addtask, acpi_dotask, acpi_gasio, acpi_maptable,
    acpi_read_pmreg, acpi_release_glk, acpi_write_pmreg,
};
use super::acpidev::{ACPIDEV_POLL, ACPIDEV_WAKEUP};
use super::acpireg::{
    ACPI_OPREG_EC, ACPI_OPREG_GPIO, ACPI_OPREG_GSB, ACPI_OPREG_PCICFG, ACPI_OPREG_SYSIO,
    ACPI_OPREG_SYSMEM, ACPI_PM1_GBL_RLS, AcpiTableHeader, GAS_EMBEDDED, GAS_PCI_CFG_SPACE,
    GAS_SYSTEM_IOSPACE, GAS_SYSTEM_MEMORY, acpi_pci_bus, acpi_pci_seg,
};
use super::acpivar::{
    ACPI_IOREAD, ACPI_IOWRITE, ACPIREG_PM1_CNT, AcpiDevlist, AcpiDevlistHead, AcpiQ, AcpiSoftc,
    acpi_softc,
};
use super::amltypes::*;
use crate::dev::pci::pcireg::{
    PCI_CLASS_BRIDGE, PCI_CLASS_REG, PCI_SUBCLASS_BRIDGE_PCI, pci_class, pci_subclass,
};
use crate::dev::pci::ppbreg::{PPB_REG_BUSINFO, ppb_businfo_secondary};
use crate::kern::kern_synch::{rwsleep, tsleep_nsec, wakeup, wakeup_one};
use crate::kern::kern_sysctl::hw_vendor;
use crate::kern::kern_tc::nanouptime;
use crate::kern::kern_timeout::timeout_add_sec;
use crate::kern::subr_prf::{Str, panic};
use crate::machine::cpu::delay;
use crate::machine::intr::{splbio, splx};
use crate::machine::pci_machdep::{pci_conf_read, pci_lookup_segment, pci_make_tag};
use crate::sys::errno::Errno;
use crate::sys::param::PWAIT;
use crate::sys::systm::{COLD, kernel_assert_locked};
use crate::sys::time::msec_to_nsec;
use crate::{kassert, kprintf};

/// `opsize(opcode)`: the bytes an opcode takes.
const fn opsize(opcode: i32) -> i32 {
    if opcode & 0xFF00 != 0 { 2 } else { 1 }
}

/// `AML_FIELD_ATTRIB` (dsdt.c's own; `AML_FIELD_RESERVED` is amltypes.h's).
const AML_FIELD_ATTRIB: i32 = 0x01;

/// `AML_REVISION`: what `Revision` answers.
pub const AML_REVISION: i64 = 0x01;
/// `AML_INTSTRLEN`.
pub const AML_INTSTRLEN: i32 = 16;
/// `AML_NAMESEG_LEN`: the length of a name segment.
pub const AML_NAMESEG_LEN: usize = 4;

/// `SRT_IRQ2`: a small resource's full first byte (`SR_TAG(SR_IRQ, 2)`).
pub const SRT_IRQ2: u8 = 0x22;
/// `SRT_IRQ3`.
pub const SRT_IRQ3: u8 = 0x23;
/// `SRT_DMA`.
pub const SRT_DMA: u8 = 0x2A;
/// `SRT_STARTDEP0`.
pub const SRT_STARTDEP0: u8 = 0x30;
/// `SRT_STARTDEP1`.
pub const SRT_STARTDEP1: u8 = 0x31;
/// `SRT_ENDDEP`.
pub const SRT_ENDDEP: u8 = 0x38;
/// `SRT_IOPORT`.
pub const SRT_IOPORT: u8 = 0x47;
/// `SRT_FIXEDPORT`.
pub const SRT_FIXEDPORT: u8 = 0x4B;
/// `SRT_ENDTAG`.
pub const SRT_ENDTAG: u8 = 0x79;

/// `SR_IRQ`: a small resource type (`AML_CRSTYPE`).
pub const SR_IRQ: i32 = 0x04;
/// `SR_DMA`.
pub const SR_DMA: i32 = 0x05;
/// `SR_STARTDEP`.
pub const SR_STARTDEP: i32 = 0x06;
/// `SR_ENDDEP`.
pub const SR_ENDDEP: i32 = 0x07;
/// `SR_IOPORT`.
pub const SR_IOPORT: i32 = 0x08;
/// `SR_FIXEDPORT`.
pub const SR_FIXEDPORT: i32 = 0x09;
/// `SR_ENDTAG`.
pub const SR_ENDTAG: i32 = 0x0F;

/// `SR_TAG(tag, len)`: byte zero of a small resource, the tag above a length in [1..7].
pub const fn sr_tag(tag: i32, len: i32) -> i32 {
    (tag << 3) + len
}

/// `LR_MEM24`: a large resource type.
pub const LR_MEM24: i32 = 0x81;
/// `LR_GENREGISTER`.
pub const LR_GENREGISTER: i32 = 0x82;
/// `LR_MEM32`.
pub const LR_MEM32: i32 = 0x85;
/// `LR_MEM32FIXED`.
pub const LR_MEM32FIXED: i32 = 0x86;
/// `LR_DWORD`.
pub const LR_DWORD: i32 = 0x87;
/// `LR_WORD`.
pub const LR_WORD: i32 = 0x88;
/// `LR_EXTIRQ`.
pub const LR_EXTIRQ: i32 = 0x89;
/// `LR_QWORD`.
pub const LR_QWORD: i32 = 0x8A;
/// `LR_GPIO`.
pub const LR_GPIO: i32 = 0x8C;
/// `LR_SERBUS`.
pub const LR_SERBUS: i32 = 0x8E;

/// `SR_IRQ_SHR`: `sr_irq.irq_flags`, shareable.
pub const SR_IRQ_SHR: u8 = 1 << 4;
/// `SR_IRQ_POLARITY`.
pub const SR_IRQ_POLARITY: u8 = 1 << 3;
/// `SR_IRQ_MODE`.
pub const SR_IRQ_MODE: u8 = 1 << 0;
/// `SR_DMA_TYP_MASK`.
pub const SR_DMA_TYP_MASK: u8 = 0x3;
/// `SR_DMA_TYP_SHIFT`.
pub const SR_DMA_TYP_SHIFT: u8 = 5;
/// `SR_DMA_BM`.
pub const SR_DMA_BM: u8 = 1 << 2;
/// `SR_DMA_SIZE_MASK`.
pub const SR_DMA_SIZE_MASK: u8 = 0x3;
/// `SR_DMA_SIZE_SHIFT`.
pub const SR_DMA_SIZE_SHIFT: u8 = 0;
/// `SR_IOPORT_DEC`.
pub const SR_IOPORT_DEC: u8 = 1 << 0;
/// `LR_EXTIRQ_SHR`.
pub const LR_EXTIRQ_SHR: u8 = 1 << 3;
/// `LR_EXTIRQ_POLARITY`.
pub const LR_EXTIRQ_POLARITY: u8 = 1 << 2;
/// `LR_EXTIRQ_MODE`.
pub const LR_EXTIRQ_MODE: u8 = 1 << 1;
/// `LR_TYPE_MEMORY`: `lr_word`/`lr_dword`/`lr_qword` `type`.
pub const LR_TYPE_MEMORY: u8 = 0;
/// `LR_TYPE_IO`.
pub const LR_TYPE_IO: u8 = 1;
/// `LR_TYPE_BUS`.
pub const LR_TYPE_BUS: u8 = 2;
/// `LR_MEMORY_TTP`.
pub const LR_MEMORY_TTP: u8 = 1 << 5;
/// `LR_IO_TTP`.
pub const LR_IO_TTP: u8 = 1 << 4;
/// `LR_GPIO_INT`: `lr_gpio.type`.
pub const LR_GPIO_INT: u8 = 0x00;
/// `LR_GPIO_IO`.
pub const LR_GPIO_IO: u8 = 0x01;
/// `LR_GPIO_SHR`: `lr_gpio.tflags`.
pub const LR_GPIO_SHR: u16 = 3 << 3;
/// `LR_GPIO_POLARITY`.
pub const LR_GPIO_POLARITY: u16 = 3 << 1;
/// `LR_GPIO_ACTHI`.
pub const LR_GPIO_ACTHI: u16 = 0 << 1;
/// `LR_GPIO_ACTLO`.
pub const LR_GPIO_ACTLO: u16 = 1 << 1;
/// `LR_GPIO_ACTBOTH`.
pub const LR_GPIO_ACTBOTH: u16 = 2 << 1;
/// `LR_GPIO_MODE`.
pub const LR_GPIO_MODE: u16 = 1 << 0;
/// `LR_GPIO_LEVEL`.
pub const LR_GPIO_LEVEL: u16 = 0;
/// `LR_GPIO_EDGE`.
pub const LR_GPIO_EDGE: u16 = 1 << 0;
/// `LR_SERBUS_I2C`: `lr_serbus.type`.
pub const LR_SERBUS_I2C: u8 = 1;

/// `ACPI_E_NOERROR`: `aml_evalnode` succeeded.
pub const ACPI_E_NOERROR: i32 = 0x00;
/// `ACPI_E_BADVALUE`: `aml_evalnode` got no node or a node without a value.
pub const ACPI_E_BADVALUE: i32 = 0x01;

/// `AML_MAX_ARG`: the arguments a method takes at most.
pub const AML_MAX_ARG: i32 = 7;
/// `AML_MAX_LOCAL`: a method's locals.
pub const AML_MAX_LOCAL: i32 = 8;

/// `AML_WALK_PRE`: `aml_walknodes` calls back before the children.
pub const AML_WALK_PRE: i32 = 0x00;
/// `AML_WALK_POST`: after them.
pub const AML_WALK_POST: i32 = 0x01;

/// `enum acpi_osi`: `OSI_UNKNOWN`, the newest Windows the firmware asked about with `_OSI`
/// (an index into [`AML_VALID_OSI`]).
pub const OSI_UNKNOWN: i32 = -1;
/// `OSI_WIN_2000`.
pub const OSI_WIN_2000: i32 = 0;
/// `OSI_WIN_XP`.
pub const OSI_WIN_XP: i32 = 1;
/// `OSI_WIN_2003`.
pub const OSI_WIN_2003: i32 = 2;
/// `OSI_WIN_2003_SP1`.
pub const OSI_WIN_2003_SP1: i32 = 3;
/// `OSI_WIN_XP_SP0`.
pub const OSI_WIN_XP_SP0: i32 = 4;
/// `OSI_WIN_XP_SP1`.
pub const OSI_WIN_XP_SP1: i32 = 5;
/// `OSI_WIN_XP_SP2`.
pub const OSI_WIN_XP_SP2: i32 = 6;
/// `OSI_WIN_XP_SP3`.
pub const OSI_WIN_XP_SP3: i32 = 7;
/// `OSI_WIN_XP_SP4`.
pub const OSI_WIN_XP_SP4: i32 = 8;
/// `OSI_WIN_VISTA`.
pub const OSI_WIN_VISTA: i32 = 9;
/// `OSI_WIN_2008`.
pub const OSI_WIN_2008: i32 = 10;
/// `OSI_WIN_VISTA_SP1`.
pub const OSI_WIN_VISTA_SP1: i32 = 11;
/// `OSI_WIN_VISTA_SP2`.
pub const OSI_WIN_VISTA_SP2: i32 = 12;
/// `OSI_WIN_7`.
pub const OSI_WIN_7: i32 = 13;
/// `OSI_WIN_8`.
pub const OSI_WIN_8: i32 = 14;
/// `OSI_WIN_8_1`.
pub const OSI_WIN_8_1: i32 = 15;
/// `OSI_WIN_10`.
pub const OSI_WIN_10: i32 = 16;
/// `OSI_WIN_10_1607`.
pub const OSI_WIN_10_1607: i32 = 17;
/// `OSI_WIN_10_1703`.
pub const OSI_WIN_10_1703: i32 = 18;
/// `OSI_WIN_10_1709`.
pub const OSI_WIN_10_1709: i32 = 19;
/// `OSI_WIN_10_1803`.
pub const OSI_WIN_10_1803: i32 = 20;
/// `OSI_WIN_10_1809`.
pub const OSI_WIN_10_1809: i32 = 21;
/// `OSI_WIN_10_1903`.
pub const OSI_WIN_10_1903: i32 = 22;
/// `OSI_WIN_10_2004`.
pub const OSI_WIN_10_2004: i32 = 23;
/// `OSI_WIN_11`.
pub const OSI_WIN_11: i32 = 24;
/// `OSI_WIN_11_22H2`.
pub const OSI_WIN_11_22H2: i32 = 25;

/// `AML_VALID_OSI` (`aml_valid_osi[]` without the NULL): the `_OSI` strings answered true,
/// indexed by `enum acpi_osi`.
pub const AML_VALID_OSI: [&[u8]; 26] = [
    b"Windows 2000",
    b"Windows 2001",
    b"Windows 2001.1",
    b"Windows 2001.1 SP1",
    b"Windows 2001 SP0",
    b"Windows 2001 SP1",
    b"Windows 2001 SP2",
    b"Windows 2001 SP3",
    b"Windows 2001 SP4",
    b"Windows 2006",
    b"Windows 2006.1",
    b"Windows 2006 SP1",
    b"Windows 2006 SP2",
    b"Windows 2009",
    b"Windows 2012",
    b"Windows 2013",
    b"Windows 2015",
    b"Windows 2016",
    b"Windows 2017",
    b"Windows 2017.2",
    b"Windows 2018",
    b"Windows 2018.2",
    b"Windows 2019",
    b"Windows 2020",
    b"Windows 2021",
    b"Windows 2022",
];

/// `struct aml_scope`: one level of the parser: a range of byte code being run, the node it
/// runs in, and, for a method, its locals, arguments and return value.
pub struct AmlScope {
    /// `sc`.
    pub sc: Option<&'static AcpiSoftc>,
    /// `pos`: the next byte; NULL once a `Return` ended a `While`.
    pub pos: Cell<Option<AmlPtr>>,
    /// `start`.
    pub start: AmlPtr,
    /// `end`.
    pub end: AmlPtr,
    /// `node`: the namespace node names are created and looked up in.
    pub node: Option<AmlNodeRef>,
    /// `parent`: the scope that pushed this one.
    pub parent: Option<Rc<AmlScope>>,
    /// `locals`: a package of `Local0`..`Local7`, made on first use.
    pub locals: RefCell<Option<AmlValueRef>>,
    /// `args`: a package of `Arg0`..`Arg6`.
    pub args: RefCell<Option<AmlValueRef>>,
    /// `retv`: what `Return` stored.
    pub retv: RefCell<Option<AmlValueRef>>,
    /// `type`: the opcode that pushed the scope (`AMLOP_METHOD`, `AMLOP_WHILE`, ...).
    pub r#type: i32,
    /// `depth`.
    pub depth: i32,
}

impl AmlScope {
    /// `scope->pos`, the end when it is NULL (the C would dereference NULL).
    fn cur(&self) -> AmlPtr {
        self.pos.get().unwrap_or(self.end)
    }

    /// `scope->pos += n`.
    fn advance(&self, n: usize) {
        self.pos.set(Some(self.cur().add(n)));
    }

    /// `scope->pos >= scope->end` (false for a NULL `pos`, as the C pointer comparison).
    fn pos_ge_end(&self) -> bool {
        matches!(self.pos.get(), Some(p) if p >= self.end)
    }
}

/// `struct aml_opcode`: an entry of the opcode table.
pub struct AmlOpcode {
    /// `opcode`.
    pub opcode: u32,
    /// `mnem`: the ASL name.
    pub mnem: &'static str,
    /// `args`: one `AML_ARG_*` letter per argument.
    pub args: &'static [u8],
}

/// Builds an `aml_table[]` entry.
const fn op(opcode: i32, mnem: &'static str, args: &'static [u8]) -> AmlOpcode {
    AmlOpcode {
        opcode: opcode as u32,
        mnem,
        args,
    }
}

/// `aml_table[]`: every opcode the parser knows, its mnemonic and its arguments.
pub static AML_TABLE: [AmlOpcode; 116] = [
    // Simple types
    op(AMLOP_ZERO, "Zero", b"c"),
    op(AMLOP_ONE, "One", b"c"),
    op(AMLOP_ONES, "Ones", b"c"),
    op(AMLOP_REVISION, "Revision", b"R"),
    op(AMLOP_BYTEPREFIX, ".Byte", b"b"),
    op(AMLOP_WORDPREFIX, ".Word", b"w"),
    op(AMLOP_DWORDPREFIX, ".DWord", b"d"),
    op(AMLOP_QWORDPREFIX, ".QWord", b"q"),
    op(AMLOP_STRINGPREFIX, ".String", b"a"),
    op(AMLOP_DEBUG, "DebugOp", b"D"),
    op(AMLOP_BUFFER, "Buffer", b"piB"),
    op(AMLOP_PACKAGE, "Package", b"pbT"),
    op(AMLOP_VARPACKAGE, "VarPackage", b"piT"),
    // Simple objects
    op(AMLOP_LOCAL0, "Local0", b"L"),
    op(AMLOP_LOCAL1, "Local1", b"L"),
    op(AMLOP_LOCAL2, "Local2", b"L"),
    op(AMLOP_LOCAL3, "Local3", b"L"),
    op(AMLOP_LOCAL4, "Local4", b"L"),
    op(AMLOP_LOCAL5, "Local5", b"L"),
    op(AMLOP_LOCAL6, "Local6", b"L"),
    op(AMLOP_LOCAL7, "Local7", b"L"),
    op(AMLOP_ARG0, "Arg0", b"A"),
    op(AMLOP_ARG1, "Arg1", b"A"),
    op(AMLOP_ARG2, "Arg2", b"A"),
    op(AMLOP_ARG3, "Arg3", b"A"),
    op(AMLOP_ARG4, "Arg4", b"A"),
    op(AMLOP_ARG5, "Arg5", b"A"),
    op(AMLOP_ARG6, "Arg6", b"A"),
    // Control flow
    op(AMLOP_IF, "If", b"piI"),
    op(AMLOP_ELSE, "Else", b"pT"),
    op(AMLOP_WHILE, "While", b"piT"),
    op(AMLOP_BREAK, "Break", b""),
    op(AMLOP_CONTINUE, "Continue", b""),
    op(AMLOP_RETURN, "Return", b"t"),
    op(AMLOP_FATAL, "Fatal", b"bdi"),
    op(AMLOP_NOP, "Nop", b""),
    op(AMLOP_BREAKPOINT, "BreakPoint", b""),
    // Arithmetic operations
    op(AMLOP_INCREMENT, "Increment", b"S"),
    op(AMLOP_DECREMENT, "Decrement", b"S"),
    op(AMLOP_ADD, "Add", b"iir"),
    op(AMLOP_SUBTRACT, "Subtract", b"iir"),
    op(AMLOP_MULTIPLY, "Multiply", b"iir"),
    op(AMLOP_DIVIDE, "Divide", b"iirr"),
    op(AMLOP_SHL, "ShiftLeft", b"iir"),
    op(AMLOP_SHR, "ShiftRight", b"iir"),
    op(AMLOP_AND, "And", b"iir"),
    op(AMLOP_NAND, "Nand", b"iir"),
    op(AMLOP_OR, "Or", b"iir"),
    op(AMLOP_NOR, "Nor", b"iir"),
    op(AMLOP_XOR, "Xor", b"iir"),
    op(AMLOP_NOT, "Not", b"ir"),
    op(AMLOP_MOD, "Mod", b"iir"),
    op(AMLOP_FINDSETLEFTBIT, "FindSetLeftBit", b"ir"),
    op(AMLOP_FINDSETRIGHTBIT, "FindSetRightBit", b"ir"),
    // Logical test operations
    op(AMLOP_LAND, "LAnd", b"ii"),
    op(AMLOP_LOR, "LOr", b"ii"),
    op(AMLOP_LNOT, "LNot", b"i"),
    op(AMLOP_LNOTEQUAL, "LNotEqual", b"tt"),
    op(AMLOP_LLESSEQUAL, "LLessEqual", b"tt"),
    op(AMLOP_LGREATEREQUAL, "LGreaterEqual", b"tt"),
    op(AMLOP_LEQUAL, "LEqual", b"tt"),
    op(AMLOP_LGREATER, "LGreater", b"tt"),
    op(AMLOP_LLESS, "LLess", b"tt"),
    // Named objects
    op(AMLOP_NAMECHAR, ".NameRef", b"n"),
    op(AMLOP_ALIAS, "Alias", b"nN"),
    op(AMLOP_NAME, "Name", b"Nt"),
    op(AMLOP_EVENT, "Event", b"N"),
    op(AMLOP_MUTEX, "Mutex", b"Nb"),
    op(AMLOP_DATAREGION, "DataRegion", b"Nttt"),
    op(AMLOP_OPREGION, "OpRegion", b"Nbii"),
    op(AMLOP_SCOPE, "Scope", b"pnT"),
    op(AMLOP_DEVICE, "Device", b"pNT"),
    op(AMLOP_POWERRSRC, "Power Resource", b"pNbwT"),
    op(AMLOP_THERMALZONE, "ThermalZone", b"pNT"),
    op(AMLOP_PROCESSOR, "Processor", b"pNbdbT"),
    op(AMLOP_METHOD, "Method", b"pNbM"),
    // Field operations
    op(AMLOP_FIELD, "Field", b"pnbF"),
    op(AMLOP_INDEXFIELD, "IndexField", b"pnnbF"),
    op(AMLOP_BANKFIELD, "BankField", b"pnnibF"),
    op(AMLOP_CREATEFIELD, "CreateField", b"tiiN"),
    op(AMLOP_CREATEQWORDFIELD, "CreateQWordField", b"tiN"),
    op(AMLOP_CREATEDWORDFIELD, "CreateDWordField", b"tiN"),
    op(AMLOP_CREATEWORDFIELD, "CreateWordField", b"tiN"),
    op(AMLOP_CREATEBYTEFIELD, "CreateByteField", b"tiN"),
    op(AMLOP_CREATEBITFIELD, "CreateBitField", b"tiN"),
    // Conversion operations
    op(AMLOP_TOINTEGER, "ToInteger", b"tr"),
    op(AMLOP_TOBUFFER, "ToBuffer", b"tr"),
    op(AMLOP_TODECSTRING, "ToDecString", b"tr"),
    op(AMLOP_TOHEXSTRING, "ToHexString", b"tr"),
    op(AMLOP_TOSTRING, "ToString", b"tir"),
    op(AMLOP_MID, "Mid", b"tiir"),
    op(AMLOP_FROMBCD, "FromBCD", b"ir"),
    op(AMLOP_TOBCD, "ToBCD", b"ir"),
    // Mutex/Signal operations
    op(AMLOP_ACQUIRE, "Acquire", b"Sw"),
    op(AMLOP_RELEASE, "Release", b"S"),
    op(AMLOP_SIGNAL, "Signal", b"S"),
    op(AMLOP_WAIT, "Wait", b"Si"),
    op(AMLOP_RESET, "Reset", b"S"),
    op(AMLOP_INDEX, "Index", b"tir"),
    op(AMLOP_DEREFOF, "DerefOf", b"t"),
    op(AMLOP_REFOF, "RefOf", b"S"),
    op(AMLOP_CONDREFOF, "CondRef", b"Sr"),
    op(AMLOP_LOADTABLE, "LoadTable", b"tttttt"),
    op(AMLOP_STALL, "Stall", b"i"),
    op(AMLOP_SLEEP, "Sleep", b"i"),
    op(AMLOP_TIMER, "Timer", b""),
    op(AMLOP_LOAD, "Load", b"nS"),
    op(AMLOP_UNLOAD, "Unload", b"t"),
    op(AMLOP_STORE, "Store", b"tS"),
    op(AMLOP_CONCAT, "Concat", b"ttr"),
    op(AMLOP_CONCATRES, "ConcatRes", b"ttt"),
    op(AMLOP_NOTIFY, "Notify", b"Si"),
    op(AMLOP_SIZEOF, "Sizeof", b"S"),
    op(AMLOP_MATCH, "Match", b"tbibii"),
    op(AMLOP_OBJECTTYPE, "ObjectType", b"S"),
    op(AMLOP_COPYOBJECT, "CopyObject", b"tS"),
];

/// `HASH_OFF`: the perfect hash's key offset.
const HASH_OFF: i32 = 6904;
/// `HASH_SIZE`.
const HASH_SIZE: usize = 179;

/// `HASH_KEY(k)`; `None` where the C's `%` of a negative key would index before the table.
const fn hash_key(k: i32) -> Option<usize> {
    let h = (k ^ HASH_OFF) % HASH_SIZE as i32;
    if h < 0 { None } else { Some(h as usize) }
}

/// `aml_ophash`, built by `aml_hashopcodes` in C: the index of each opcode's entry plus one
/// (0 for an empty slot), at its hash key. A later entry with the same key wins, as there.
static AML_OPHASH: [u8; HASH_SIZE] = {
    let mut h = [0u8; HASH_SIZE];
    let mut i = 0;
    while i < AML_TABLE.len() {
        if let Some(k) = hash_key(AML_TABLE[i].opcode as i32) {
            h[k] = (i + 1) as u8;
        }
        i += 1;
    }
    h
};

/// `union acpi_resource`: one resource descriptor of a `_CRS` buffer, as the bytes of the
/// descriptor. Each `view.member` of the C union is a method `view_member()`; a member past
/// the descriptor's bytes reads 0.
#[derive(Clone, Copy)]
pub struct AcpiResource<'a> {
    b: &'a [u8],
}

/// Defines the accessors of [`AcpiResource`]: `name: type @ offset`.
macro_rules! acpi_resource_members {
    ($($(#[$m:meta])* $name:ident: $t:ident @ $off:expr;)*) => {
        // `sr_ioport__min` is `sr_ioport._min`: the C members' names start with `_`.
        #[allow(non_snake_case)]
        impl AcpiResource<'_> {
            $(
                $(#[$m])*
                pub fn $name(&self) -> $t {
                    let mut b = [0u8; core::mem::size_of::<$t>()];
                    for (i, x) in b.iter_mut().enumerate() {
                        *x = self.byte($off + i);
                    }
                    $t::from_le_bytes(b)
                }
            )*
        }
    };
}

impl<'a> AcpiResource<'a> {
    /// A view of the descriptor in `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { b: bytes }
    }

    /// `pad[i]`: byte `i` of the descriptor, 0 past its end.
    pub fn byte(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    /// The descriptor's bytes (and, for a descriptor of 64 bytes or more, what follows it in
    /// the buffer, as the C's pointer into it reaches).
    pub fn bytes(&self) -> &'a [u8] {
        self.b
    }

    /// `lr_extirq.irq[i]`.
    pub fn lr_extirq_irq(&self, i: usize) -> u32 {
        let o = 5 + 4 * i;
        u32::from_le_bytes([
            self.byte(o),
            self.byte(o + 1),
            self.byte(o + 2),
            self.byte(o + 3),
        ])
    }
}

acpi_resource_members! {
    /// `hdr.typecode`.
    hdr_typecode: u8 @ 0;
    /// `hdr.length`: a large descriptor's length after the three header bytes.
    hdr_length: u16 @ 1;
    /// `sr_irq.irq_mask`.
    sr_irq_irq_mask: u16 @ 1;
    /// `sr_irq.irq_flags`.
    sr_irq_irq_flags: u8 @ 3;
    /// `sr_dma.channel`.
    sr_dma_channel: u8 @ 1;
    /// `sr_dma.flags`.
    sr_dma_flags: u8 @ 2;
    /// `sr_ioport.flags`.
    sr_ioport_flags: u8 @ 1;
    /// `sr_ioport._min`.
    sr_ioport__min: u16 @ 2;
    /// `sr_ioport._max`.
    sr_ioport__max: u16 @ 4;
    /// `sr_ioport._aln`.
    sr_ioport__aln: u8 @ 6;
    /// `sr_ioport._len`.
    sr_ioport__len: u8 @ 7;
    /// `sr_fioport._bas`.
    sr_fioport__bas: u16 @ 1;
    /// `sr_fioport._len`.
    sr_fioport__len: u8 @ 3;
    /// `lr_m24._info`.
    lr_m24__info: u8 @ 3;
    /// `lr_m24._min`.
    lr_m24__min: u16 @ 4;
    /// `lr_m24._max`.
    lr_m24__max: u16 @ 6;
    /// `lr_m24._aln`.
    lr_m24__aln: u16 @ 8;
    /// `lr_m24._len`.
    lr_m24__len: u16 @ 10;
    /// `lr_m32._info`.
    lr_m32__info: u8 @ 3;
    /// `lr_m32._min`.
    lr_m32__min: u32 @ 4;
    /// `lr_m32._max`.
    lr_m32__max: u32 @ 8;
    /// `lr_m32._aln`.
    lr_m32__aln: u32 @ 12;
    /// `lr_m32._len`.
    lr_m32__len: u32 @ 16;
    /// `lr_m32fixed._info`.
    lr_m32fixed__info: u8 @ 3;
    /// `lr_m32fixed._bas`.
    lr_m32fixed__bas: u32 @ 4;
    /// `lr_m32fixed._len`.
    lr_m32fixed__len: u32 @ 8;
    /// `lr_extirq.flags`.
    lr_extirq_flags: u8 @ 3;
    /// `lr_extirq.irq_count`.
    lr_extirq_irq_count: u8 @ 4;
    /// `lr_word.type`.
    lr_word_type: u8 @ 3;
    /// `lr_word.flags`.
    lr_word_flags: u8 @ 4;
    /// `lr_word.tflags`.
    lr_word_tflags: u8 @ 5;
    /// `lr_word._gra`.
    lr_word__gra: u16 @ 6;
    /// `lr_word._min`.
    lr_word__min: u16 @ 8;
    /// `lr_word._max`.
    lr_word__max: u16 @ 10;
    /// `lr_word._tra`.
    lr_word__tra: u16 @ 12;
    /// `lr_word._len`.
    lr_word__len: u16 @ 14;
    /// `lr_word.src_index`.
    lr_word_src_index: u8 @ 16;
    /// `lr_dword.type`.
    lr_dword_type: u8 @ 3;
    /// `lr_dword.flags`.
    lr_dword_flags: u8 @ 4;
    /// `lr_dword.tflags`.
    lr_dword_tflags: u8 @ 5;
    /// `lr_dword._gra`.
    lr_dword__gra: u32 @ 6;
    /// `lr_dword._min`.
    lr_dword__min: u32 @ 10;
    /// `lr_dword._max`.
    lr_dword__max: u32 @ 14;
    /// `lr_dword._tra`.
    lr_dword__tra: u32 @ 18;
    /// `lr_dword._len`.
    lr_dword__len: u32 @ 22;
    /// `lr_dword.src_index`.
    lr_dword_src_index: u8 @ 26;
    /// `lr_qword.type`.
    lr_qword_type: u8 @ 3;
    /// `lr_qword.flags`.
    lr_qword_flags: u8 @ 4;
    /// `lr_qword.tflags`.
    lr_qword_tflags: u8 @ 5;
    /// `lr_qword._gra`.
    lr_qword__gra: u64 @ 6;
    /// `lr_qword._min`.
    lr_qword__min: u64 @ 14;
    /// `lr_qword._max`.
    lr_qword__max: u64 @ 22;
    /// `lr_qword._tra`.
    lr_qword__tra: u64 @ 30;
    /// `lr_qword._len`.
    lr_qword__len: u64 @ 38;
    /// `lr_qword.src_index`.
    lr_qword_src_index: u8 @ 46;
    /// `lr_gpio.revid`.
    lr_gpio_revid: u8 @ 3;
    /// `lr_gpio.type`.
    lr_gpio_type: u8 @ 4;
    /// `lr_gpio.flags`.
    lr_gpio_flags: u16 @ 5;
    /// `lr_gpio.tflags`.
    lr_gpio_tflags: u16 @ 7;
    /// `lr_gpio._ppi`.
    lr_gpio__ppi: u8 @ 9;
    /// `lr_gpio._drs`.
    lr_gpio__drs: u16 @ 10;
    /// `lr_gpio._dbt`.
    lr_gpio__dbt: u16 @ 12;
    /// `lr_gpio.pin_off`.
    lr_gpio_pin_off: u16 @ 14;
    /// `lr_gpio.residx`.
    lr_gpio_residx: u8 @ 16;
    /// `lr_gpio.res_off`.
    lr_gpio_res_off: u16 @ 17;
    /// `lr_gpio.vd_off`.
    lr_gpio_vd_off: u16 @ 19;
    /// `lr_gpio.vd_len`.
    lr_gpio_vd_len: u16 @ 21;
    /// `lr_serbus.revid`.
    lr_serbus_revid: u8 @ 3;
    /// `lr_serbus.residx`.
    lr_serbus_residx: u8 @ 4;
    /// `lr_serbus.type`.
    lr_serbus_type: u8 @ 5;
    /// `lr_serbus.flags`.
    lr_serbus_flags: u8 @ 6;
    /// `lr_serbus.tflags`.
    lr_serbus_tflags: u16 @ 7;
    /// `lr_serbus.trevid`.
    lr_serbus_trevid: u8 @ 9;
    /// `lr_serbus.tlength`.
    lr_serbus_tlength: u16 @ 10;
    /// `lr_i2cbus.revid`.
    lr_i2cbus_revid: u8 @ 3;
    /// `lr_i2cbus.residx`.
    lr_i2cbus_residx: u8 @ 4;
    /// `lr_i2cbus.type`.
    lr_i2cbus_type: u8 @ 5;
    /// `lr_i2cbus.flags`.
    lr_i2cbus_flags: u8 @ 6;
    /// `lr_i2cbus.tflags`.
    lr_i2cbus_tflags: u16 @ 7;
    /// `lr_i2cbus.trevid`.
    lr_i2cbus_trevid: u8 @ 9;
    /// `lr_i2cbus.tlength`.
    lr_i2cbus_tlength: u16 @ 10;
    /// `lr_i2cbus._spe`.
    lr_i2cbus__spe: u32 @ 12;
    /// `lr_i2cbus._adr`.
    lr_i2cbus__adr: u16 @ 16;
}

/// Offset of `lr_word.src` in the descriptor.
pub const LR_WORD_SRC: usize = 17;
/// Offset of `lr_dword.src`.
pub const LR_DWORD_SRC: usize = 27;
/// Offset of `lr_qword.src`.
pub const LR_QWORD_SRC: usize = 47;
/// Offset of `lr_serbus.tdata`.
pub const LR_SERBUS_TDATA: usize = 12;
/// Offset of `lr_i2cbus.vdata`.
pub const LR_I2CBUS_VDATA: usize = 18;
/// `sizeof(union acpi_resource)`: its `pad[64]`.
pub const ACPI_RESOURCE_SIZE: usize = 64;

/// `AML_CRSTYPE(x)`: a large descriptor's type byte, or a small one's tag.
pub fn aml_crstype(x: &AcpiResource<'_>) -> i32 {
    let t = x.hdr_typecode();
    if t & 0x80 != 0 {
        i32::from(t)
    } else {
        i32::from(t >> 3)
    }
}

/// `AML_CRSLEN(x)`: the descriptor's length in bytes.
pub fn aml_crslen(x: &AcpiResource<'_>) -> i32 {
    let t = x.hdr_typecode();
    if t & 0x80 != 0 {
        3 + i32::from(x.hdr_length())
    } else {
        1 + i32::from(t & 0x7)
    }
}

/// `union amlpci_t`: a PCI configuration address, `addr` or its parts (little-endian:
/// `reg` in bits 0-15, `fun` 16-31, `dev` 32-39, `bus` 40-47, `seg` 48-63).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AmlpciT {
    /// `addr`.
    pub addr: u64,
}

impl AmlpciT {
    /// Replaces `width` bits at `shift` with `v`.
    fn put(&mut self, shift: u32, width: u32, v: u64) {
        let mask = ((1u64 << width) - 1) << shift;
        self.addr = (self.addr & !mask) | ((v << shift) & mask);
    }

    /// `reg`.
    pub fn reg(&self) -> u16 {
        self.addr as u16
    }

    /// `fun`.
    pub fn fun(&self) -> u16 {
        (self.addr >> 16) as u16
    }

    /// `dev`.
    pub fn dev(&self) -> u8 {
        (self.addr >> 32) as u8
    }

    /// `bus`.
    pub fn bus(&self) -> u8 {
        (self.addr >> 40) as u8
    }

    /// `seg`.
    pub fn seg(&self) -> u16 {
        (self.addr >> 48) as u16
    }

    /// `fun = v`.
    pub fn set_fun(&mut self, v: u64) {
        self.put(16, 16, v);
    }

    /// `dev = v`.
    pub fn set_dev(&mut self, v: u64) {
        self.put(32, 8, v);
    }

    /// `bus = v`.
    pub fn set_bus(&mut self, v: u64) {
        self.put(40, 8, v);
    }

    /// `seg = v`.
    pub fn set_seg(&mut self, v: u64) {
        self.put(48, 16, v);
    }
}

/// `struct aml_notify_data`: a driver's request to hear about notifications on a node.
struct AmlNotifyData {
    node: AmlNodeRef,
    pnpid: [u8; 20],
    cbarg: *mut c_void,
    cbproc: Option<AmlNotifyFn>,
    flags: i32,
}

/// The callback of `aml_register_notify`: `int (*)(struct aml_node *, int, void *)`.
pub type AmlNotifyFn = fn(&AmlNodeRef, i32, *mut c_void) -> i32;

/// The handler of an address space: `int (*)(void *cookie, int iodir, uint64_t address,
/// int size, uint64_t *value)`.
pub type AmlRegionHandler = fn(*mut c_void, i32, u64, i32, &mut u64) -> i32;

/// `struct aml_regionspace`.
#[derive(Clone, Copy)]
struct AmlRegionspace {
    cookie: *mut c_void,
    handler: Option<AmlRegionHandler>,
}

/// One entry of `struct aml_defval aml_defobj[]`: an object `aml_create_defaultobjects`
/// puts in the namespace.
struct AmlDefval {
    name: &'static [u8],
    r#type: i32,
    ival: i64,
    bval: DefBval,
    gval: bool,
}

/// The `bval` of an `aml_defobj[]` entry.
#[derive(Clone, Copy)]
enum DefBval {
    None,
    Osstring,
    Fneval(AmlFneval),
}

/// A global of the interpreter (the namespace root, the notify list, the region-space
/// table, ...).
///
/// The C keeps these in plain globals and the interpreter is not reentrant across CPUs:
/// it relies on the kernel lock. `get` asserts it (`KERNEL_ASSERT_LOCKED()`, with
/// `MULTIPROCESSOR` and `DIAGNOSTIC`) before handing out the reference, which is the only
/// way to reach the contents.
pub struct AmlGlobal<T>(T);

// SAFETY: OpenBSD reaches the namespace, its values and these globals only from the acpi
// thread (a kthread), from acpi_interrupt (established with IPL_BIO | IPL_WAKEUP, not
// IPL_MPSAFE), and from driver attach, tasks, sensor and timeout callbacks that are not
// MPSAFE; all of them run under the kernel lock (checked at the pin: no IPL_MPSAFE,
// TASKQ_MPSAFE or TIMEOUT_MPSAFE path in dev/acpi reaches aml_*). So one CPU at a time
// touches the `Rc` counts and `RefCell` flags inside; `get` asserts the lock. Host tests
// that touch the namespace serialise on a test lock instead.
unsafe impl<T> Sync for AmlGlobal<T> {}

impl<T> AmlGlobal<T> {
    /// A global holding `v`.
    pub const fn new(v: T) -> Self {
        Self(v)
    }

    /// The contents, under the kernel lock.
    pub fn get(&self) -> &T {
        kernel_assert_locked();
        &self.0
    }
}

/// `aml_intlen`: the width of an AML integer in bits, 64, or 32 for a DSDT of revision 1
/// (`acpi.c` sets it).
pub static AML_INTLEN: AtomicI32 = AtomicI32::new(64);

/// `aml_intlen`, read.
pub fn aml_intlen() -> i32 {
    AML_INTLEN.load(Ordering::Relaxed)
}

/// `aml_root`: the namespace's root, `\`. Made on first use.
static AML_ROOT: AmlGlobal<RefCell<Option<AmlNodeRef>>> = AmlGlobal::new(RefCell::new(None));

/// `aml_global_lock`: the value of `\_GL`.
static AML_GLOBAL_LOCK: AmlGlobal<RefCell<Option<AmlValueRef>>> =
    AmlGlobal::new(RefCell::new(None));

/// `aml_lastscope`: the innermost scope, for `aml_die`'s backtrace.
static AML_LASTSCOPE: AmlGlobal<RefCell<Weak<AmlScope>>> =
    AmlGlobal::new(RefCell::new(Weak::new()));

/// `aml_notify_list`.
static AML_NOTIFY_LIST: AmlGlobal<RefCell<Vec<Rc<AmlNotifyData>>>> =
    AmlGlobal::new(RefCell::new(Vec::new()));

/// The default handler of an address space `acpi_gasio` serves.
const fn defspace(handler: AmlRegionHandler) -> Cell<AmlRegionspace> {
    Cell::new(AmlRegionspace {
        cookie: ptr::null_mut(),
        handler: Some(handler),
    })
}

/// `aml_regionspace[256]`: the handler of each address space.
static AML_REGIONSPACE: AmlGlobal<[Cell<AmlRegionspace>; 256]> = AmlGlobal::new({
    let mut t = [const {
        Cell::new(AmlRegionspace {
            cookie: ptr::null_mut(),
            handler: None,
        })
    }; 256];
    t[ACPI_OPREG_SYSMEM as usize] = defspace(aml_opreg_sysmem_handler);
    t[ACPI_OPREG_SYSIO as usize] = defspace(aml_opreg_sysio_handler);
    t[ACPI_OPREG_PCICFG as usize] = defspace(aml_opreg_pcicfg_handler);
    t[ACPI_OPREG_EC as usize] = defspace(aml_opreg_ec_handler);
    t
});

/// `global_lock_count`: how deep this CPU holds the firmware's global lock.
static GLOBAL_LOCK_COUNT: AtomicI64 = AtomicI64::new(0);

/// `acpi_max_osi`: the most recent Windows the firmware asked about (`enum acpi_osi`).
pub static ACPI_MAX_OSI: AtomicI32 = AtomicI32::new(OSI_UNKNOWN);

/// `aml_error`: errors since the last `acpi_parse_aml`/`aml_evalnode`.
pub static AML_ERROR: AtomicI32 = AtomicI32::new(0);

/// `aml_busy`: nonzero while a table is being parsed.
pub static AML_BUSY: AtomicI32 = AtomicI32::new(0);

/// `odp`: the depth of `aml_parse` calls.
static ODP: AtomicI32 = AtomicI32::new(0);

/// `maxdp`: the deepest it went.
static MAXDP: AtomicI32 = AtomicI32::new(0);

/// `acpinowait` (`acpi_sleep`'s): the channel nobody wakes.
static ACPINOWAIT: AtomicI32 = AtomicI32::new(0);

/// `aml_die(...)`: `_aml_die(__FUNCTION__, __LINE__, ...)`.
macro_rules! aml_die {
    ($func:expr, $($arg:tt)*) => {
        _aml_die($func, line!(), format_args!($($arg)*))
    };
}

/// The NULL `uint8_t *` of a method without byte code (`_OSI`).
const AML_NULL: AmlPtr = AmlPtr::new(&[]);

/// The `const void *bval` of `_aml_setvalue`/`aml_allocvalue`, whose meaning depends on the
/// type being set.
#[derive(Clone)]
pub enum Bval<'a> {
    /// `NULL`.
    Null,
    /// The bytes of a buffer or string.
    Bytes(&'a [u8]),
    /// A method's `fneval`.
    Fneval(AmlFneval),
    /// A `NAMEREF`'s name in the byte code.
    Name(AmlPtr),
    /// An `OBJREF`'s target.
    Ref(AmlValueRef),
}

/// `aml_root`: the root of the namespace, made (empty, named `\`) on first use.
pub fn aml_root() -> AmlNodeRef {
    let mut root = AML_ROOT.get().borrow_mut();
    root.get_or_insert_with(|| Rc::new(AmlNode::new(None, *b"\\\0\0\0\0")))
        .clone()
}

/// `aml_global_lock`: the value of `\_GL` once `aml_create_defaultobjects` made it.
pub fn aml_global_lock() -> Option<AmlValueRef> {
    AML_GLOBAL_LOCK.get().borrow().clone()
}

/// `aml_pc(src)`: the offset of `src` in the table being run (for messages).
pub fn aml_pc(src: AmlPtr) -> i32 {
    match aml_root().start.get() {
        Some(start) => src.diff(start) as i32,
        None => src.addr() as i32,
    }
}

/// `_aml_die(fn, line, fmt, ...)`: prints the message and the arguments and locals of every
/// method being run, then panics.
pub fn _aml_die(func: &str, line: u32, args: fmt::Arguments<'_>) -> ! {
    kprintf!("{}\n", args);

    let mut root = AML_LASTSCOPE.get().borrow().upgrade();
    while let Some(r) = root {
        let Some(pos) = r.pos.get() else { break };
        kprintf!(
            "{:04x} Called: {}\n",
            aml_pc(pos),
            Str(&aml_nodename(r.node.as_ref()))
        );
        for idx in 0..AML_MAX_ARG {
            if let Some(sp) = aml_getstack(&r, AMLOP_ARG0 + idx)
                && sp.r#type() != 0
            {
                kprintf!("  arg{}: ", idx);
                aml_showvalue(Some(&sp));
            }
        }
        for idx in 0..AML_MAX_LOCAL {
            if let Some(sp) = aml_getstack(&r, AMLOP_LOCAL0 + idx)
                && sp.r#type() != 0
            {
                kprintf!("  local{}: ", idx);
                aml_showvalue(Some(&sp));
            }
        }
        root = r.parent.clone();
    }

    // XXX: don't panic (the C's own comment)
    panic(format_args!("aml_die {}:{}", func, line));
}

/// `aml_hashopcodes()`: builds the opcode hash. `AML_OPHASH` is computed at compile time,
/// so there is nothing left to do at run time; `acpi.c` still calls it.
pub fn aml_hashopcodes() {}

/// `aml_findopcode(opcode)`: the table entry of `opcode`.
pub fn aml_findopcode(opcode: i32) -> Option<&'static AmlOpcode> {
    let k = hash_key(opcode)?;
    let i = AML_OPHASH[k];
    if i == 0 {
        return None;
    }
    let hop = &AML_TABLE[usize::from(i) - 1];
    if hop.opcode as i32 == opcode {
        Some(hop)
    } else {
        None
    }
}

/// Cuts `b` to what fits a C buffer of `size` bytes with its NUL.
fn cap(mut b: Vec<u8>, size: usize) -> Vec<u8> {
    b.truncate(size - 1);
    b
}

/// `aml_mnem(opcode, pos)`: the mnemonic of `opcode`, with its immediate operand at `pos`
/// for the prefixes and names.
pub fn aml_mnem(opcode: i32, pos: Option<AmlPtr>) -> Vec<u8> {
    let Some(tab) = aml_findopcode(opcode) else {
        return b"xxx".to_vec();
    };
    let mut s = tab.mnem.as_bytes().to_vec();
    if let Some(pos) = pos {
        match opcode {
            AMLOP_STRINGPREFIX => {
                s = alloc::format!("\"{}\"", Str(pos.tail())).into_bytes();
            }
            AMLOP_BYTEPREFIX => s = alloc::format!("0x{:02x}", pos.get8()).into_bytes(),
            AMLOP_WORDPREFIX => s = alloc::format!("0x{:04x}", pos.get16()).into_bytes(),
            // The C prints a DWord's low 16 bits.
            AMLOP_DWORDPREFIX => s = alloc::format!("0x{:04x}", pos.get16()).into_bytes(),
            AMLOP_NAMECHAR => s = aml_getname(pos.tail()),
            _ => {}
        }
    }
    cap(s, 32)
}

/// `acpi_sleep(ms, reason)`: sleeps (or spins while cold) for `ms` milliseconds.
pub fn acpi_sleep(ms: i32, reason: &'static str) {
    // XXX ACPI integers are supposed to be unsigned.
    let ms = ms.max(1);

    if COLD.load(Ordering::Relaxed) {
        delay((ms as u32).saturating_mul(1000));
    } else {
        let _ = tsleep_nsec(
            ptr::from_ref(&ACPINOWAIT),
            PWAIT,
            reason,
            msec_to_nsec(ms as u64),
        );
    }
}

/// `acpi_stall(us)`.
pub fn acpi_stall(us: i32) {
    delay(us.max(0) as u32);
}

/// `aml_tstbit(pb, bit)`: bit `bit` of `pb` (its mask, as the C); 0 past the end.
pub fn aml_tstbit(pb: &[u8], bit: i32) -> u8 {
    pb.get(aml_bytepos(bit) as usize)
        .map_or(0, |b| b & aml_bitmask(bit))
}

/// `aml_setbit(pb, bit, val)`; a bit past the end is not written.
pub fn aml_setbit(pb: &mut [u8], bit: i32, val: bool) {
    if let Some(b) = pb.get_mut(aml_bytepos(bit) as usize) {
        if val {
            *b |= aml_bitmask(bit);
        } else {
            *b &= !aml_bitmask(bit);
        }
    }
}

/// `acpi_poll(arg)`: the `sc_dev_timeout` handler: queues a poll of the `ACPIDEV_POLL`
/// devices for the acpi thread and comes back in ten seconds.
pub fn acpi_poll(_arg: *mut c_void) {
    let Some(sc) = acpi_softc() else { return };

    let s = splbio();
    acpi_addtask(sc, acpi_poll_notify_task, ptr::null_mut(), 0);
    sc.sc_threadwaiting.set(0);
    wakeup(ptr::from_ref(sc));
    splx(s);

    timeout_add_sec(&sc.sc_dev_timeout, 10);
}

/// A snapshot of `aml_notify_list`, so callbacks may register more.
fn notify_list() -> Vec<Rc<AmlNotifyData>> {
    AML_NOTIFY_LIST.get().borrow().clone()
}

/// `aml_notify_task(node, notify_value)`: runs the callbacks registered on `node`.
///
/// `node` is the `Rc::into_raw` `aml_notify` queued; the task takes that reference back.
pub fn aml_notify_task(node: *mut c_void, notify_value: i32) {
    if node.is_null() {
        return;
    }
    // SAFETY: `aml_notify` queued this task with `Rc::into_raw` of the node, and
    // `acpi_addtask` runs each task exactly once, so this is the one `from_raw` of it.
    let node = unsafe { Rc::from_raw(node.cast_const().cast::<AmlNode>()) };
    for pdata in notify_list() {
        if Rc::ptr_eq(&pdata.node, &node)
            && let Some(cbproc) = pdata.cbproc
        {
            cbproc(&pdata.node, notify_value, pdata.cbarg);
        }
    }
}

/// `aml_register_notify(node, pnpid, proc, arg, flags)`: calls `proc(node, value, arg)` on
/// each `Notify` of `node` (and on `aml_notify_dev(pnpid, ...)`); `ACPIDEV_POLL` also polls
/// it every ten seconds.
pub fn aml_register_notify(
    node: &AmlNodeRef,
    pnpid: Option<&[u8]>,
    proc_: AmlNotifyFn,
    arg: *mut c_void,
    flags: i32,
) {
    let mut id = [0u8; 20];
    if let Some(pnpid) = pnpid {
        let n = pnpid
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(pnpid.len())
            .min(id.len() - 1);
        id[..n].copy_from_slice(&pnpid[..n]);
    }
    let pdata = Rc::new(AmlNotifyData {
        node: node.clone(),
        pnpid: id,
        cbarg: arg,
        cbproc: Some(proc_),
        flags,
    });
    AML_NOTIFY_LIST.get().borrow_mut().insert(0, pdata);

    if flags & ACPIDEV_POLL != 0
        && ACPI_POLL_ENABLED.load(Ordering::Relaxed) == 0
        && let Some(sc) = acpi_softc()
    {
        timeout_add_sec(&sc.sc_dev_timeout, 10);
    }
}

/// `aml_notify(node, notify_value)`: queues the callbacks of `node` for the acpi thread.
pub fn aml_notify(node: Option<&AmlNodeRef>, notify_value: i32) {
    let Some(node) = node else { return };
    let Some(sc) = acpi_softc() else { return };

    for pdata in notify_list() {
        if Rc::ptr_eq(&pdata.node, node) && pdata.flags & ACPIDEV_WAKEUP != 0 {
            sc.sc_wakeup.set(1);
        }
    }

    acpi_addtask(
        sc,
        aml_notify_task,
        Rc::into_raw(node.clone()).cast_mut().cast::<c_void>(),
        notify_value,
    );
}

/// `aml_notify_dev(pnpid, notify_value)`: runs the callbacks registered under `pnpid`.
pub fn aml_notify_dev(pnpid: Option<&[u8]>, notify_value: i32) {
    let Some(pnpid) = pnpid else { return };
    for pdata in notify_list() {
        if cstr(&pdata.pnpid) == cstr(pnpid)
            && let Some(cbproc) = pdata.cbproc
        {
            cbproc(&pdata.node, notify_value, pdata.cbarg);
        }
    }
}

/// `acpi_poll_notify_task(arg0, arg1)`: calls every `ACPIDEV_POLL` callback with 0.
pub fn acpi_poll_notify_task(_arg0: *mut c_void, _arg1: i32) {
    for pdata in notify_list() {
        if let Some(cbproc) = pdata.cbproc
            && pdata.flags & ACPIDEV_POLL != 0
        {
            cbproc(&pdata.node, 0, pdata.cbarg);
        }
    }
}

/// The bytes of `s` up to its first NUL (a C string in a buffer).
pub fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `strncmp(node->name, nameseg, AML_NAMESEG_LEN) == 0`.
fn nameseg_eq(name: &[u8; 5], seg: &[u8]) -> bool {
    for (i, &a) in name.iter().take(AML_NAMESEG_LEN).enumerate() {
        let b = seg.get(i).copied().unwrap_or(0);
        if a != b {
            return false;
        }
        if a == 0 {
            return true;
        }
    }
    true
}

/// `__aml_search(root, nameseg, create)`: the child of `root` named `nameseg` (four bytes),
/// made (with an uninitialised value) when absent and `create` is set.
pub fn __aml_search(root: Option<&AmlNodeRef>, nameseg: &[u8], create: bool) -> Option<AmlNodeRef> {
    let root = root?;
    for node in root.sons() {
        if nameseg_eq(&node.name, nameseg) {
            return Some(node);
        }
    }
    if create {
        let mut name = [0u8; 5];
        for (i, c) in name.iter_mut().take(AML_NAMESEG_LEN).enumerate() {
            *c = nameseg.get(i).copied().unwrap_or(0);
        }
        let node = Rc::new(AmlNode::new(Some(root), name));
        let value = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
        value.set_node(Some(&node));
        *node.value.borrow_mut() = Some(value);
        root.add_son(node.clone());
        return Some(node);
    }
    None
}

/// Appends `s` to `buf` as `strlcat` into a 128-byte buffer would.
fn strlcat128(buf: &mut Vec<u8>, s: &[u8]) {
    for &c in cstr(s) {
        if buf.len() >= 127 {
            break;
        }
        buf.push(c);
    }
}

/// `aml_nodename(node)`: the absolute name of `node` (`\_SB_.PCI0`).
pub fn aml_nodename(node: Option<&AmlNodeRef>) -> Vec<u8> {
    /// The C's recursion over `namebuf`, which starts with an extra character.
    fn build(node: Option<&AmlNodeRef>, buf: &mut Vec<u8>) {
        buf.clear();
        if let Some(node) = node {
            let parent = node.parent();
            build(parent.as_ref(), buf);
            let under_root = parent.is_some_and(|p| Rc::ptr_eq(&p, &aml_root()));
            if !under_root {
                strlcat128(buf, b".");
            }
            strlcat128(buf, node.name());
        }
    }
    let mut buf = Vec::new();
    build(node, &mut buf);
    if node.is_some() && !buf.is_empty() {
        buf.remove(0);
    }
    buf
}

/// `aml_getname(name)`: an encoded AML name string (`\`, `^` prefixes, a segment, a dual or
/// a multi name) as dotted text (`\_SB_.PCI0`).
pub fn aml_getname(name: &[u8]) -> Vec<u8> {
    let g = |i: usize| name.get(i).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;

    while i32::from(g(i)) == AMLOP_ROOTCHAR || i32::from(g(i)) == AMLOP_PARENTPREFIX {
        out.push(g(i));
        i += 1;
    }
    let count = match i32::from(g(i)) {
        0x00 => 0,
        AMLOP_MULTINAMEPREFIX => {
            let c = usize::from(g(i + 1));
            i += 2;
            c
        }
        AMLOP_DUALNAMEPREFIX => {
            i += 1;
            2
        }
        _ => 1,
    };
    for _ in 0..count {
        for k in 0..AML_NAMESEG_LEN {
            out.push(g(i + k));
        }
        out.push(b'.');
        i += AML_NAMESEG_LEN;
        if g(i) == b'.' {
            i += 1;
        }
    }
    // `*(--p) = 0`: drops the last '.', or the last prefix of a NULL name.
    out.pop();
    let mut out = cstr(&out).to_vec();
    out.truncate(127);
    out
}

/// `aml_delchildren(node)`: deletes every node below `node`. A value something else still
/// refers to survives, without its node.
pub fn aml_delchildren(node: Option<&AmlNodeRef>) {
    let Some(node) = node else { return };
    while let Some(onode) = node.take_first_son() {
        aml_delchildren(Some(&onode));

        // Don't delete values that have references
        let value = onode.value.borrow_mut().take();
        if let Some(value) = value
            && Rc::strong_count(&value) > 1
        {
            value.set_node(None);
        }
    }
}

/// The address of the firmware's global lock word in the FACS, if acpi0 mapped one.
fn facs_global_lock() -> Option<(&'static AcpiSoftc, *mut u32)> {
    let sc = acpi_softc()?;
    let facs = sc.sc_facs.get();
    if facs.is_null() {
        return None;
    }
    // SAFETY: `sc_facs` points at the FACS `acpi.c` mapped for good; the projection makes a
    // raw pointer to the (packed, aligned in practice) member without a reference.
    let lock = unsafe { &raw mut (*facs).global_lock };
    Some((sc, lock))
}

/// `acpi_glk_enter()`: takes the firmware's global lock, spinning until it is ours.
pub fn acpi_glk_enter() {
    // If lock is already ours, just continue.
    if GLOBAL_LOCK_COUNT.fetch_add(1, Ordering::Relaxed) != 0 {
        return;
    }
    let Some((_sc, lock)) = facs_global_lock() else {
        return;
    };

    // Spin to acquire the lock.
    let mut st = 0;
    while st == 0 {
        st = acpi_acquire_glk(lock);
        // XXX - yield/delay?
    }
}

/// `acpi_glk_leave()`: releases it, and tells the firmware if it was waiting.
pub fn acpi_glk_leave() {
    // If we are the last one, turn out the lights.
    if GLOBAL_LOCK_COUNT.fetch_sub(1, Ordering::Relaxed) - 1 != 0 {
        return;
    }
    let Some((sc, lock)) = facs_global_lock() else {
        return;
    };

    let st = acpi_release_glk(lock);
    if st == 0 {
        return;
    }

    // If pending, notify the BIOS that the lock was released by OSPM. No locking is
    // needed because nobody outside the ACPI thread is supposed to touch this register.
    let mut x = acpi_read_pmreg(sc, ACPIREG_PM1_CNT, 0);
    x |= i32::from(ACPI_PM1_GBL_RLS);
    acpi_write_pmreg(sc, ACPIREG_PM1_CNT, 0, x);
}

/// `aml_lockfield(scope, field)`: takes the global lock for a field with `LockRule` on.
pub fn aml_lockfield(_scope: Option<&Rc<AmlScope>>, field: &AmlValue) {
    let flags = field.v_field().map_or(0, |f| f.flags);
    if aml_field_lock(flags) != AML_FIELD_LOCK_ON {
        return;
    }
    acpi_glk_enter();
}

/// `aml_unlockfield(scope, field)`.
pub fn aml_unlockfield(_scope: Option<&Rc<AmlScope>>, field: &AmlValue) {
    let flags = field.v_field().map_or(0, |f| f.flags);
    if aml_field_lock(flags) != AML_FIELD_LOCK_ON {
        return;
    }
    acpi_glk_leave();
}

/// `aml_showvalue(val)`: prints a value (for `aml_die` and errors).
pub fn aml_showvalue(val: Option<&AmlValueRef>) {
    let Some(val) = val else { return };

    if let Some(node) = val.node() {
        kprintf!(" [{}]", Str(&aml_nodename(Some(&node))));
    }
    kprintf!(
        " {:p} cnt:{:02x} stk:{:02x}",
        Rc::as_ptr(val),
        Rc::strong_count(val),
        val.stack.get()
    );
    let obj = val.obj().clone();
    match obj {
        AmlObj::Integer(i) => {
            kprintf!(" integer: {:x}\n", i);
        }
        AmlObj::String(s) => {
            kprintf!(" string: {}\n", Str(&s));
        }
        AmlObj::Method(m) => {
            kprintf!(" method: {:02x}\n", m.flags);
        }
        AmlObj::Package(p) => {
            kprintf!(" package: {:02x}\n", p.len());
            for e in &p {
                aml_showvalue(Some(e));
            }
        }
        AmlObj::Buffer(b) => {
            kprintf!(" buffer: {:02x} {{", b.len());
            for (idx, x) in b.iter().enumerate() {
                kprintf!("{}{:02x}", if idx != 0 { ", " } else { "" }, x);
            }
            kprintf!("}}\n");
        }
        AmlObj::FieldUnit(f) | AmlObj::BufferField(f) => {
            kprintf!(
                " field: bitpos={:04x} bitlen={:04x} ref1:{:p} ref2:{:p} [{}]\n",
                f.bitpos,
                f.bitlen,
                f.ref1.as_ref().map_or(ptr::null(), Rc::as_ptr),
                f.ref2.as_ref().map_or(ptr::null(), Rc::as_ptr),
                Str(&aml_mnem(f.r#type, None))
            );
            if let Some(r) = &f.ref1 {
                kprintf!("  ref1: {}\n", Str(&aml_nodename(r.node().as_ref())));
            }
            if let Some(r) = &f.ref2 {
                kprintf!("  ref2: {}\n", Str(&aml_nodename(r.node().as_ref())));
            }
        }
        // v_mutex is never a struct acpi_mutex (see amltypes.rs): the C's NULL case.
        AmlObj::Mutex(_) => {
            kprintf!(" mutex:  ref: 0\n");
        }
        AmlObj::Event(_) => {
            kprintf!(" event:\n");
        }
        AmlObj::OpRegion(r) => {
            kprintf!(
                " opregion: {:02x},{:08x},{:x}\n",
                r.iospace,
                r.iobase,
                r.iolen
            );
        }
        AmlObj::NameRef(p) => {
            kprintf!(" nameref: {}\n", Str(&aml_getname(p.tail())));
        }
        AmlObj::Device => {
            kprintf!(" device:\n");
        }
        AmlObj::Processor(p) => {
            kprintf!(
                " cpu: {:02x},{:04x},{:02x}\n",
                p.proc_id,
                p.proc_addr,
                p.proc_len
            );
        }
        AmlObj::ThermZone => {
            kprintf!(" thermzone:\n");
        }
        AmlObj::PowerRsrc(p) => {
            kprintf!(" pwrrsrc: {:02x},{:02x}\n", p.pwr_level, p.pwr_order);
        }
        AmlObj::ObjRef(o) => {
            kprintf!(
                " objref: {:p} index:{:x} opcode:{}\n",
                o.r#ref.as_ref().map_or(ptr::null(), Rc::as_ptr),
                o.index,
                Str(&aml_mnem(o.r#type, None))
            );
            aml_showvalue(o.r#ref.as_ref());
        }
        other => {
            kprintf!(" !!type: {:x}\n", other.r#type());
        }
    }
}

/// `aml_val2int(rval)`: an integer, buffer or (hex) string as an integer; 0 otherwise.
pub fn aml_val2int(rval: Option<&AmlValue>) -> i64 {
    let Some(rval) = rval else { return 0 };
    let obj = rval.obj();
    match &*obj {
        AmlObj::Integer(i) => *i,
        AmlObj::Buffer(b) => {
            let mut ival = [0u8; 8];
            let bits = aml_intlen().min(b.len() as i32 * 8);
            aml_bufcpy(&mut ival, 0, b, 0, bits);
            i64::from_le_bytes(ival)
        }
        AmlObj::String(s) => aml_hextoint(s),
        _ => 0,
    }
}

/// The value of `type` with its union zeroed (`memset(&lhs->_, 0, ...)`).
fn zero_obj(r#type: i32) -> AmlObj {
    match r#type {
        AML_OBJTYPE_UNINITIALIZED => AmlObj::Uninitialized,
        AML_OBJTYPE_INTEGER => AmlObj::Integer(0),
        AML_OBJTYPE_STRING => AmlObj::String(Vec::new()),
        AML_OBJTYPE_BUFFER => AmlObj::Buffer(Vec::new()),
        AML_OBJTYPE_PACKAGE => AmlObj::Package(Vec::new()),
        AML_OBJTYPE_FIELDUNIT => AmlObj::FieldUnit(AmlField::default()),
        AML_OBJTYPE_DEVICE => AmlObj::Device,
        AML_OBJTYPE_EVENT => AmlObj::Event(0),
        AML_OBJTYPE_METHOD => AmlObj::Method(AmlMethod::default()),
        AML_OBJTYPE_MUTEX => AmlObj::Mutex(AmlMtx::default()),
        AML_OBJTYPE_OPREGION => AmlObj::OpRegion(AmlOpregion::default()),
        AML_OBJTYPE_POWERRSRC => AmlObj::PowerRsrc(AmlPowerrsrc::default()),
        AML_OBJTYPE_PROCESSOR => AmlObj::Processor(AmlProcessor::default()),
        AML_OBJTYPE_THERMZONE => AmlObj::ThermZone,
        AML_OBJTYPE_BUFFERFIELD => AmlObj::BufferField(AmlField::default()),
        AML_OBJTYPE_DDBHANDLE => AmlObj::DdbHandle(0),
        AML_OBJTYPE_DEBUGOBJ => AmlObj::DebugObj,
        AML_OBJTYPE_NAMEREF => AmlObj::NameRef(AML_NULL),
        AML_OBJTYPE_OBJREF => AmlObj::ObjRef(AmlObjref::default()),
        AML_OBJTYPE_SCOPE => AmlObj::Scope(AML_NULL, 0),
        AML_OBJTYPE_NOTARGET => AmlObj::NoTarget,
        t => AmlObj::Other(t),
    }
}

/// `strncpy(dst, src, n)` into `n` zeroed bytes.
fn strncpy(src: &[u8], n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    for (d, &s) in v.iter_mut().zip(src.iter()) {
        if s == 0 {
            break;
        }
        *d = s;
    }
    v
}

/// `_aml_setvalue(lhs, type, ival, bval)`: makes `lhs` a value of `type` built from `ival`
/// and `bval` (what they mean depends on the type). The old contents are dropped.
pub fn _aml_setvalue(lhs: &AmlValue, r#type: i32, ival: i64, bval: Bval<'_>) {
    let obj = match r#type {
        AML_OBJTYPE_INTEGER => AmlObj::Integer(ival),
        AML_OBJTYPE_METHOD => AmlObj::Method(AmlMethod {
            flags: ival as i32,
            fneval: match bval {
                Bval::Fneval(f) => Some(f),
                _ => None,
            },
            ..AmlMethod::default()
        }),
        AML_OBJTYPE_NAMEREF => AmlObj::NameRef(match bval {
            Bval::Name(p) => p,
            _ => AML_NULL,
        }),
        AML_OBJTYPE_OBJREF => AmlObj::ObjRef(AmlObjref {
            r#type: ival as i32,
            index: 0,
            r#ref: match bval {
                Bval::Ref(r) => Some(r),
                _ => None,
            },
        }),
        AML_OBJTYPE_BUFFER => {
            let n = ival.max(0) as usize;
            let mut b = vec![0u8; n];
            if let Bval::Bytes(src) = bval {
                let m = n.min(src.len());
                b[..m].copy_from_slice(&src[..m]);
            }
            AmlObj::Buffer(b)
        }
        AML_OBJTYPE_STRING => {
            let src = match bval {
                Bval::Bytes(s) => s,
                _ => &[],
            };
            let n = if ival == -1 {
                cstr(src).len()
            } else {
                ival.max(0) as usize
            };
            AmlObj::String(strncpy(src, n))
        }
        AML_OBJTYPE_PACKAGE => AmlObj::Package(
            (0..ival.max(0))
                .map(|_| aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null))
                .collect(),
        ),
        t => zero_obj(t),
    };
    lhs.set_obj(obj);
}

/// `aml_copyvalue(lhs, rhs)`: copies `rhs` into `lhs` (deeply for packages, by reference
/// for object references).
pub fn aml_copyvalue(lhs: &AmlValue, rhs: &AmlValue) {
    let rtype = rhs.r#type();
    let obj = {
        let r = rhs.obj();
        match &*r {
            AmlObj::Uninitialized => AmlObj::Uninitialized,
            AmlObj::Integer(i) => AmlObj::Integer(*i),
            // The C copies `v_mutex`, which overlays `synclvl` and `savelvl`.
            AmlObj::Mutex(m) => AmlObj::Mutex(AmlMtx {
                synclvl: m.synclvl,
                savelvl: m.savelvl,
                ..AmlMtx::default()
            }),
            AmlObj::PowerRsrc(p) => AmlObj::PowerRsrc(*p),
            AmlObj::Method(m) => AmlObj::Method(*m),
            AmlObj::Buffer(b) => AmlObj::Buffer(b.clone()),
            AmlObj::String(s) => AmlObj::String(strncpy(s, s.len())),
            AmlObj::OpRegion(o) => AmlObj::OpRegion(*o),
            AmlObj::Processor(p) => AmlObj::Processor(*p),
            AmlObj::NameRef(p) => AmlObj::NameRef(*p),
            AmlObj::Package(p) => AmlObj::Package(
                p.iter()
                    .map(|e| {
                        let v = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
                        aml_copyvalue(&v, e);
                        v
                    })
                    .collect(),
            ),
            AmlObj::ObjRef(o) => AmlObj::ObjRef(o.clone()),
            AmlObj::Device => AmlObj::Device,
            AmlObj::ThermZone => AmlObj::ThermZone,
            _ => {
                kprintf!("copyvalue: {:x}", rtype);
                zero_obj(rtype)
            }
        }
    };
    if matches!(
        rtype,
        AML_OBJTYPE_POWERRSRC | AML_OBJTYPE_PROCESSOR | AML_OBJTYPE_DEVICE | AML_OBJTYPE_THERMZONE
    ) {
        lhs.set_node(rhs.node().as_ref());
    }
    lhs.set_obj(obj);
}

/// `aml_allocvalue(type, ival, bval)`: a new value (see [`_aml_setvalue`]).
pub fn aml_allocvalue(r#type: i32, ival: i64, bval: Bval<'_>) -> AmlValueRef {
    let rv = Rc::new(AmlValue::new());
    _aml_setvalue(&rv, r#type, ival, bval);
    rv
}

/// `aml_freevalue(val)`: drops the contents, leaving an uninitialised value.
pub fn aml_freevalue(val: Option<&AmlValue>) {
    if let Some(val) = val {
        val.set_obj(AmlObj::Uninitialized);
    }
}

/// `aml_convradix(val, iradix, oradix)`: rewrites the digits of `val` in radix `iradix` as
/// digits in radix `oradix` (BCD conversions).
pub fn aml_convradix(mut val: u64, iradix: i32, oradix: i32) -> u64 {
    let (iradix, oradix) = (iradix as u64, oradix as u64);
    let mut rv: u64 = 0;
    let mut pwr: u64 = 1;
    while val != 0 {
        rv = rv.wrapping_add((val % iradix).wrapping_mul(pwr));
        val /= iradix;
        pwr = pwr.wrapping_mul(oradix);
    }
    rv
}

/// `aml_lsb(val)`: the 1-based position of the lowest set bit; 0 for 0.
pub fn aml_lsb(mut val: u64) -> i32 {
    if val == 0 {
        return 0;
    }
    let mut lsb = 1;
    while val & 0x1 == 0 {
        val >>= 1;
        lsb += 1;
    }
    lsb
}

/// `aml_msb(val)`: the 1-based position of the highest set bit; 0 for 0.
pub fn aml_msb(mut val: u64) -> i32 {
    if val == 0 {
        return 0;
    }
    let mut msb = 1;
    while val != 0x1 {
        val >>= 1;
        msb += 1;
    }
    msb
}

/// `-(cond)` of the C's logical operators: all ones for true.
fn lbool(c: bool) -> u64 {
    if c { u64::MAX } else { 0 }
}

/// `aml_evalexpr(lhs, rhs, opcode)`: the arithmetic and logical operators.
pub fn aml_evalexpr(lhs: u64, rhs: u64, opcode: i32) -> u64 {
    match opcode {
        // Math operations
        AMLOP_INCREMENT | AMLOP_ADD => lhs.wrapping_add(rhs),
        AMLOP_DECREMENT | AMLOP_SUBTRACT => lhs.wrapping_sub(rhs),
        AMLOP_MULTIPLY => lhs.wrapping_mul(rhs),
        // A zero divisor traps in C; `aml_parse` checks Divide's, Mod's yields 0 here.
        AMLOP_DIVIDE => lhs.checked_div(rhs).unwrap_or(0),
        AMLOP_MOD => lhs.checked_rem(rhs).unwrap_or(0),
        AMLOP_SHL => lhs.wrapping_shl(rhs as u32),
        AMLOP_SHR => lhs.wrapping_shr(rhs as u32),
        AMLOP_AND => lhs & rhs,
        AMLOP_NAND => !(lhs & rhs),
        AMLOP_OR => lhs | rhs,
        AMLOP_NOR => !(lhs | rhs),
        AMLOP_XOR => lhs ^ rhs,
        AMLOP_NOT => !lhs,

        // Conversion/misc
        AMLOP_FINDSETLEFTBIT => aml_msb(lhs) as u64,
        AMLOP_FINDSETRIGHTBIT => aml_lsb(lhs) as u64,
        AMLOP_TOINTEGER => lhs,
        AMLOP_FROMBCD => aml_convradix(lhs, 16, 10),
        AMLOP_TOBCD => aml_convradix(lhs, 10, 16),

        // Logical/Comparison
        AMLOP_LAND => lbool(lhs != 0 && rhs != 0),
        AMLOP_LOR => lbool(lhs != 0 || rhs != 0),
        AMLOP_LNOT => lbool(lhs == 0),
        AMLOP_LNOTEQUAL => lbool(lhs != rhs),
        AMLOP_LLESSEQUAL => lbool(lhs <= rhs),
        AMLOP_LGREATEREQUAL => lbool(lhs >= rhs),
        AMLOP_LEQUAL => lbool(lhs == rhs),
        AMLOP_LGREATER => lbool(lhs > rhs),
        AMLOP_LLESS => lbool(lhs < rhs),
        _ => 0,
    }
}

/// `aml_bufcpy(dst, dstpos, src, srcpos, len)`: copies `len` bits from bit `srcpos` of
/// `src` to bit `dstpos` of `dst`, bytewise when everything is byte-aligned. Bits outside
/// either buffer are not copied.
pub fn aml_bufcpy(dst: &mut [u8], dst_pos: i32, src: &[u8], src_pos: i32, len: i32) {
    if len <= 0 {
        return;
    }
    if aml_bytealigned(dst_pos | src_pos | len) {
        // Aligned transfer: use memcpy
        let d = aml_bytepos(dst_pos) as usize;
        let s = aml_bytepos(src_pos) as usize;
        let n = aml_bytelen(len) as usize;
        let n = n
            .min(dst.len().saturating_sub(d))
            .min(src.len().saturating_sub(s));
        if n > 0 {
            dst[d..d + n].copy_from_slice(&src[s..s + n]);
        }
        return;
    }

    // Misaligned transfer: perform bitwise copy (slow)
    for idx in 0..len {
        aml_setbit(dst, idx + dst_pos, aml_tstbit(src, idx + src_pos) != 0);
    }
}

/// `aml_walknodes(node, mode, nodecb, arg)`: calls `nodecb` on `node` and every node below
/// it, before (`AML_WALK_PRE`; a nonzero answer skips the children) or after
/// (`AML_WALK_POST`) the children.
pub fn aml_walknodes(
    node: Option<&AmlNodeRef>,
    mode: i32,
    nodecb: &mut dyn FnMut(&AmlNodeRef) -> i32,
) {
    let Some(node) = node else { return };
    if mode == AML_WALK_PRE && nodecb(node) != 0 {
        return;
    }
    for child in node.sons() {
        aml_walknodes(Some(&child), mode, nodecb);
    }
    if mode == AML_WALK_POST {
        nodecb(node);
    }
}

/// `aml_find_node(node, name, cbproc, arg)`: calls `cbproc` on every node below `node`
/// named `name`; a nonzero answer stops the search among that node's siblings.
pub fn aml_find_node(node: &AmlNodeRef, name: &[u8], cbproc: &mut dyn FnMut(&AmlNodeRef) -> i32) {
    let name = cstr(name);

    // match child of this node first before recursing
    for child in node.sons() {
        let mut nn = child.name();
        if nn.first().is_some_and(|&c| i32::from(c) == AMLOP_ROOTCHAR) {
            nn = &nn[1..];
        }
        while nn
            .first()
            .is_some_and(|&c| i32::from(c) == AMLOP_PARENTPREFIX)
        {
            nn = &nn[1..];
        }
        // Only recurse if cbproc() wants us to
        if name == nn && cbproc(&child) != 0 {
            return;
        }
    }

    for child in node.sons() {
        aml_find_node(&child, name, cbproc);
    }
}

/// `aml_parseopcode(scope)`: decodes the opcode at `scope->pos` and steps over it; an
/// embedded name is `AMLOP_NAMECHAR` and is not stepped over.
pub fn aml_parseopcode(scope: &AmlScope) -> i32 {
    let pos = scope.cur();
    if pos >= scope.end {
        return AMLOP_INVALID;
    }
    let opcode = i32::from(pos.at(0));

    // Check if this is an embedded name
    match opcode {
        AMLOP_ROOTCHAR
        | AMLOP_PARENTPREFIX
        | AMLOP_MULTINAMEPREFIX
        | AMLOP_DUALNAMEPREFIX
        | AMLOP_NAMECHAR => return AMLOP_NAMECHAR,
        _ => {}
    }
    if (i32::from(b'A')..=i32::from(b'Z')).contains(&opcode) {
        return AMLOP_NAMECHAR;
    }

    // Treat AMLOP_LNOT special. It might be a single-byte opcode, but it can also be the
    // start of a dual-byte opcode. Since AMLOP_LNOT has fixed-list arguments, it needs to
    // be followed by at least some additional bytes. So we can safely handle it together
    // with the other dual-byte opcodes.
    if opcode != AMLOP_LNOT && opcode != AMLOP_EXTPREFIX {
        scope.advance(1);
        return opcode;
    }

    if pos.add(1) >= scope.end {
        return AMLOP_INVALID;
    }
    let twocode = (i32::from(pos.at(0)) << 8) + i32::from(pos.at(1));

    // Check for single-byte AMLOP_LNOT.
    if opcode == AMLOP_LNOT
        && twocode != AMLOP_LNOTEQUAL
        && twocode != AMLOP_LLESSEQUAL
        && twocode != AMLOP_LGREATEREQUAL
    {
        scope.advance(1);
        return opcode;
    }

    scope.advance(2);
    twocode
}

/// `aml_parsename(inode, pos, &rval, create)`: decodes the name string at `pos` relative to
/// `inode` and returns the position after it and the value it names (an alias resolved;
/// a `NAMEREF` when the name does not exist and `create` is not set).
pub fn aml_parsename(
    inode: Option<&AmlNodeRef>,
    pos: AmlPtr,
    create: bool,
) -> (AmlPtr, AmlValueRef) {
    let start = pos;
    let mut pos = pos;
    let mut node = inode.cloned();

    if i32::from(pos.get8()) == AMLOP_ROOTCHAR {
        pos = pos.add(1);
        node = Some(aml_root());
    }
    while i32::from(pos.get8()) == AMLOP_PARENTPREFIX {
        pos = pos.add(1);
        node = node.and_then(|n| n.parent()).or_else(|| Some(aml_root()));
    }
    match i32::from(pos.get8()) {
        0x00 => pos = pos.add(1),
        AMLOP_MULTINAMEPREFIX => {
            let n = usize::from(pos.at(1));
            for i in 0..n {
                node = __aml_search(
                    node.as_ref(),
                    pos.add(2 + i * AML_NAMESEG_LEN).bytes(AML_NAMESEG_LEN),
                    create,
                );
            }
            pos = pos.add(2 + n * AML_NAMESEG_LEN);
        }
        AMLOP_DUALNAMEPREFIX => {
            node = __aml_search(node.as_ref(), pos.add(1).bytes(AML_NAMESEG_LEN), create);
            node = __aml_search(
                node.as_ref(),
                pos.add(1 + AML_NAMESEG_LEN).bytes(AML_NAMESEG_LEN),
                create,
            );
            pos = pos.add(1 + 2 * AML_NAMESEG_LEN);
        }
        _ => {
            // If Relative Search (pos == start), recursively go up root
            let mut relnode = node.clone();
            loop {
                node = __aml_search(relnode.as_ref(), pos.bytes(AML_NAMESEG_LEN), create);
                relnode = relnode.and_then(|r| r.parent());
                if node.is_some() || pos != start || relnode.is_none() {
                    break;
                }
            }
            pos = pos.add(AML_NAMESEG_LEN);
        }
    }

    let rval = match node {
        Some(node) => {
            let mut rval = node
                .value()
                .unwrap_or_else(|| aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null));
            // Dereference ALIAS here
            if let Some(o) = rval.v_objref()
                && o.r#type == AMLOP_ALIAS
                && let Some(r) = o.r#ref
            {
                rval = r;
            }
            rval
        }
        None => aml_allocvalue(AML_OBJTYPE_NAMEREF, 0, Bval::Name(start)),
    };
    (pos, rval)
}

/// `aml_parselength(scope)`: decodes a package length (one to four bytes) and steps over
/// it.
///
/// ```text
///  byte0    byte1    byte2    byte3
///  00xxxxxx                             : if upper bits == 00, length = xxxxxx
///  01--xxxx yyyyyyyy                    : if upper bits == 01, length = yyyyyyyyxxxx
///  10--xxxx yyyyyyyy zzzzzzzz           : if upper bits == 10, length = zzzzzzzzyyyyyyyyxxxx
///  11--xxxx yyyyyyyy zzzzzzzz wwwwwwww  : if upper bits == 11, length = wwwwwwwwzzzzzzzzyyyyyyyyxxxx
/// ```
pub fn aml_parselength(scope: &AmlScope) -> i32 {
    let next = || {
        let b = scope.cur().get8();
        scope.advance(1);
        i32::from(b)
    };
    let lcode = next();
    if lcode <= 0x3F {
        return lcode;
    }

    // lcode >= 0x40, multibyte length, get first byte of extended length
    let mut len = lcode & 0xF;
    len += next() << 4;
    if lcode >= 0x80 {
        len += next() << 12;
    }
    if lcode >= 0xC0 {
        len += next() << 20;
    }
    len
}

/// `aml_parseend(scope)`: the end of the object whose package length is at `scope->pos`
/// (stepped over), cut at the scope's end.
pub fn aml_parseend(scope: &AmlScope) -> AmlPtr {
    let pos = scope.cur();
    let len = aml_parselength(scope);
    let e = pos.add(len.max(0) as usize);
    if e > scope.end {
        return scope.end;
    }
    e
}

/// `hext[]`.
const HEXT: &[u8; 16] = b"0123456789ABCDEF";

/// `aml_eisaid(pid)`: a compressed EISA id (`_HID` integer) as its seven characters
/// (`PNP0A03`).
pub fn aml_eisaid(pid: u32) -> [u8; 7] {
    [
        b'@'.wrapping_add(((pid >> 2) & 0x1F) as u8),
        b'@'.wrapping_add((((pid << 3) & 0x18) + ((pid >> 13) & 0x7)) as u8),
        b'@'.wrapping_add(((pid >> 8) & 0x1F) as u8),
        HEXT[((pid >> 20) & 0xF) as usize],
        HEXT[((pid >> 16) & 0xF) as usize],
        HEXT[((pid >> 28) & 0xF) as usize],
        HEXT[((pid >> 24) & 0xF) as usize],
    ]
}

/// `aml_defobj[]`: the objects every namespace starts with.
static AML_DEFOBJ: [AmlDefval; 9] = [
    AmlDefval {
        name: b"_OS_\0",
        r#type: AML_OBJTYPE_STRING,
        ival: -1,
        bval: DefBval::Osstring,
        gval: false,
    },
    AmlDefval {
        name: b"_REV\0",
        r#type: AML_OBJTYPE_INTEGER,
        ival: 2,
        bval: DefBval::None,
        gval: false,
    },
    AmlDefval {
        name: b"_GL\0",
        r#type: AML_OBJTYPE_MUTEX,
        ival: 1,
        bval: DefBval::None,
        gval: true,
    },
    AmlDefval {
        name: b"_OSI\0",
        r#type: AML_OBJTYPE_METHOD,
        ival: 1,
        bval: DefBval::Fneval(aml_callosi),
        gval: false,
    },
    // Create default scopes
    AmlDefval {
        name: b"_GPE\0",
        r#type: AML_OBJTYPE_DEVICE,
        ival: 0,
        bval: DefBval::None,
        gval: false,
    },
    AmlDefval {
        name: b"_PR_\0",
        r#type: AML_OBJTYPE_DEVICE,
        ival: 0,
        bval: DefBval::None,
        gval: false,
    },
    AmlDefval {
        name: b"_SB_\0",
        r#type: AML_OBJTYPE_DEVICE,
        ival: 0,
        bval: DefBval::None,
        gval: false,
    },
    AmlDefval {
        name: b"_TZ_\0",
        r#type: AML_OBJTYPE_DEVICE,
        ival: 0,
        bval: DefBval::None,
        gval: false,
    },
    AmlDefval {
        name: b"_SI_\0",
        r#type: AML_OBJTYPE_DEVICE,
        ival: 0,
        bval: DefBval::None,
        gval: false,
    },
];

/// `aml_callosi(scope, val)`: the built-in `_OSI` method. True for the Windows versions of
/// [`AML_VALID_OSI`] (remembering the newest asked about in [`ACPI_MAX_OSI`]), and on
/// Apple hardware for "Darwin" only.
pub fn aml_callosi(scope: &Rc<AmlScope>, _val: Option<&AmlValueRef>) -> Option<AmlValueRef> {
    let fa = aml_getstack(scope, AMLOP_ARG0)
        .map(|v| v.v_string())
        .unwrap_or_default();

    // SAFETY: `hw_vendor` is written by the machine's attach code before the first table
    // is parsed, and only read afterwards.
    let vendor = unsafe { hw_vendor.read() };
    if let Some(vendor) = vendor {
        let vendor = cstr(vendor);
        if vendor == b"Apple Inc." || vendor == b"Apple Computer, Inc." {
            let result = i64::from(fa == b"Darwin");
            return Some(aml_allocvalue(AML_OBJTYPE_INTEGER, result, Bval::Null));
        }
    }

    let mut result = 0;
    for (idx, osi) in AML_VALID_OSI.iter().enumerate() {
        if fa.as_slice() == *osi {
            result = 1;
            ACPI_MAX_OSI.fetch_max(idx as i32, Ordering::Relaxed);
            break;
        }
    }
    Some(aml_allocvalue(AML_OBJTYPE_INTEGER, result, Bval::Null))
}

/// `aml_create_defaultobjects()`: starts the namespace: `\`, `_OS_`, `_REV`, `_GL`, `_OSI`
/// and the predefined scopes.
pub fn aml_create_defaultobjects() {
    let mut osstring = *b"Macrosift Windogs MT\0";
    osstring[1] = b'i';
    osstring[6] = b'o';
    osstring[15] = b'w';
    osstring[18] = b'N';

    let root = aml_root();
    while root.take_first_son().is_some() {}
    let value = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
    value.set_node(Some(&root));
    *root.value.borrow_mut() = Some(value);

    for def in &AML_DEFOBJ {
        // Allocate object value + add to namespace
        let (_, tmp) = aml_parsename(Some(&root), AmlPtr::new(def.name), true);
        let bval = match def.bval {
            DefBval::None => Bval::Null,
            DefBval::Osstring => Bval::Bytes(&osstring),
            DefBval::Fneval(f) => Bval::Fneval(f),
        };
        _aml_setvalue(&tmp, def.r#type, def.ival, bval);
        if def.gval {
            // Set root object pointer
            *AML_GLOBAL_LOCK.get().borrow_mut() = Some(tmp);
        }
    }
}

/// `aml_parse_resource(res, crs_enum, arg)`: calls `crs_enum(index, descriptor)` on each
/// descriptor of the resource template `res` up to its end tag; -1 if `res` is no buffer of
/// at least five bytes.
pub fn aml_parse_resource(
    res: &AmlValue,
    crs_enum: &mut dyn FnMut(i32, &AcpiResource<'_>) -> i32,
) -> i32 {
    let buf = match &*res.obj() {
        AmlObj::Buffer(b) if b.len() >= 5 => b.clone(),
        _ => return -1,
    };
    let len = buf.len();
    let mut off = 0;
    let mut crsidx = 0;
    while off < len {
        let crs = AcpiResource::new(&buf[off..]);
        let rlen = aml_crslen(&crs) as usize;
        if crs.hdr_typecode() == SRT_ENDTAG || rlen == 0 {
            break;
        }

        // aml_mapresource: a short descriptor is seen through a zeroed copy of its bytes.
        let crs = if rlen >= ACPI_RESOURCE_SIZE {
            crs
        } else {
            AcpiResource::new(&buf[off..(off + rlen).min(len)])
        };
        crs_enum(crsidx, &crs);
        off += rlen;
        crsidx += 1;
    }
    0
}

/// `aml_foreachpkg(pkg, start, fn, arg)`: calls `f` on each element of `pkg` from `start`.
pub fn aml_foreachpkg(pkg: &AmlValue, start: i32, f: &mut dyn FnMut(&AmlValueRef)) {
    if pkg.r#type() != AML_OBJTYPE_PACKAGE {
        return;
    }
    for idx in start.max(0)..pkg.length() {
        if let Some(e) = pkg.v_package(idx as usize) {
            f(&e);
        }
    }
}

/// `aml_fixup_node(node, arg)`: resolves the `NAMEREF`s of a node's value (and of the
/// packages in it) now that the whole namespace exists.
fn aml_fixup_node(node: &AmlNodeRef, val: Option<&AmlValueRef>) -> i32 {
    let Some(nval) = node.value() else { return 0 };
    let Some(val) = val else {
        return aml_fixup_node(node, Some(&nval));
    };
    let obj = val.obj().clone();
    match obj {
        AmlObj::NameRef(p) => {
            if let Some(n) = aml_searchname(Some(node), &aml_getname(p.tail()))
                && let Some(v) = n.value()
            {
                _aml_setvalue(
                    val,
                    AML_OBJTYPE_OBJREF,
                    i64::from(AMLOP_NAMECHAR),
                    Bval::Ref(v),
                );
            }
        }
        AmlObj::Package(p) => {
            for e in &p {
                aml_fixup_node(node, Some(e));
            }
        }
        _ => {}
    }
    0
}

/// `aml_postparse()`: resolves forward references after the tables were parsed.
pub fn aml_postparse() {
    aml_walknodes(Some(&aml_root()), AML_WALK_PRE, &mut |n| {
        aml_fixup_node(n, None)
    });
}

/// `aml_val_to_string(val)`: a buffer, string or integer as text.
pub fn aml_val_to_string(val: &AmlValue) -> Vec<u8> {
    let obj = val.obj();
    let s = match &*obj {
        AmlObj::Buffer(b) => cstr(&b[..b.len().min(255)]).to_vec(),
        AmlObj::String(s) => cstr(&s[..s.len().min(255)]).to_vec(),
        AmlObj::Integer(i) => alloc::format!("{:x}", i).into_bytes(),
        other => {
            alloc::format!("Failed to convert type {} to string!", other.r#type()).into_bytes()
        }
    };
    cap(s, 256)
}

/// `aml_findscope(scope, type, endscope)`: the innermost scope of `type`. With `endscope`
/// (`Return`, `Break`, `Continue`) every scope on the way is ended, and a `Break` also ends
/// the loop in its parent.
pub fn aml_findscope(scope: &Rc<AmlScope>, r#type: i32, endscope: i32) -> Option<Rc<AmlScope>> {
    let mut s = Some(scope.clone());
    while let Some(sc) = s {
        match endscope {
            AMLOP_RETURN => {
                sc.pos.set(Some(sc.end));
                if sc.r#type == AMLOP_WHILE {
                    sc.pos.set(None);
                }
            }
            AMLOP_CONTINUE => sc.pos.set(Some(sc.end)),
            AMLOP_BREAK => {
                sc.pos.set(Some(sc.end));
                if sc.r#type == r#type
                    && let Some(p) = &sc.parent
                {
                    p.pos.set(Some(sc.end));
                }
            }
            _ => {}
        }
        if sc.r#type == r#type {
            return Some(sc);
        }
        s = sc.parent.clone();
    }
    None
}

/// `aml_getstack(scope, opcode)`: the `LocalX`/`ArgX` of the method `scope` runs in (an
/// argument passed by reference is dereferenced).
pub fn aml_getstack(scope: &Rc<AmlScope>, opcode: i32) -> Option<AmlValueRef> {
    let scope = aml_findscope(scope, AMLOP_METHOD, 0)?;
    if (AMLOP_LOCAL0..=AMLOP_LOCAL7).contains(&opcode) {
        let locals = scope
            .locals
            .borrow_mut()
            .get_or_insert_with(|| aml_allocvalue(AML_OBJTYPE_PACKAGE, 8, Bval::Null))
            .clone();
        let sp = locals.v_package((opcode - AMLOP_LOCAL0) as usize)?;
        sp.stack.set(opcode);
        Some(sp)
    } else if (AMLOP_ARG0..=AMLOP_ARG6).contains(&opcode) {
        let args = scope
            .args
            .borrow_mut()
            .get_or_insert_with(|| aml_allocvalue(AML_OBJTYPE_PACKAGE, 7, Bval::Null))
            .clone();
        let sp = args.v_package((opcode - AMLOP_ARG0) as usize)?;
        match sp.v_objref() {
            Some(o) => o.r#ref,
            None => Some(sp),
        }
    } else {
        None
    }
}

/// `aml_pushscope(parent, range, node, type)`: a scope running the byte code of `range` (a
/// method, or an AML range); `None` for an empty range.
pub fn aml_pushscope(
    parent: Option<&Rc<AmlScope>>,
    range: &AmlValue,
    node: Option<AmlNodeRef>,
    r#type: i32,
) -> Option<Rc<AmlScope>> {
    let (start, end) = match &*range.obj() {
        AmlObj::Method(m) => (m.start.unwrap_or(AML_NULL), m.end.unwrap_or(AML_NULL)),
        AmlObj::Scope(p, len) => {
            let (s, e) = (*p, p.add((*len).max(0) as usize));
            if s == e {
                return None;
            }
            (s, e)
        }
        _ => return None,
    };
    let scope = Rc::new(AmlScope {
        sc: acpi_softc(),
        pos: Cell::new(Some(start)),
        start,
        end,
        node,
        parent: parent.cloned(),
        locals: RefCell::new(None),
        args: RefCell::new(None),
        retv: RefCell::new(None),
        r#type,
        depth: parent.map_or(0, |p| p.depth + 1),
    });
    *AML_LASTSCOPE.get().borrow_mut() = Rc::downgrade(&scope);
    Some(scope)
}

/// `aml_popscope(scope)`: ends `scope` (deleting the names a method made) and returns its
/// parent.
pub fn aml_popscope(scope: Rc<AmlScope>) -> Option<Rc<AmlScope>> {
    let nscope = scope.parent.clone();

    if scope.r#type == AMLOP_METHOD {
        aml_delchildren(scope.node.as_ref());
    }
    let locals = scope.locals.borrow_mut().take();
    aml_freevalue(locals.as_deref());
    let args = scope.args.borrow_mut().take();
    aml_freevalue(args.as_deref());
    *AML_LASTSCOPE.get().borrow_mut() = nscope.as_ref().map_or_else(Weak::new, Rc::downgrade);

    nscope
}

/// `aml_matchtest(a, b, op)`: one `Match()` comparison.
pub fn aml_matchtest(a: i64, b: i64, op: i32) -> bool {
    match op {
        AML_MATCH_TR => true,
        AML_MATCH_EQ => a == b,
        AML_MATCH_LT => a < b,
        AML_MATCH_LE => a <= b,
        AML_MATCH_GE => a >= b,
        AML_MATCH_GT => a > b,
        _ => false,
    }
}

/// `aml_match(pkg, index, op1, v1, op2, v2)`: the first element from `index` that passes
/// both comparisons, or -1.
pub fn aml_match(pkg: &AmlValue, mut index: i32, op1: i32, v1: i32, op2: i32, v2: i32) -> i32 {
    while index >= 0 && index < pkg.length() {
        let Some(e) = pkg.v_package(index as usize) else {
            break;
        };
        // Convert package value to integer
        let tmp = aml_convert(&e, AML_OBJTYPE_INTEGER, -1);

        // Perform test
        let flag = aml_matchtest(tmp.v_integer(), i64::from(v1), op1)
            && aml_matchtest(tmp.v_integer(), i64::from(v2), op2);
        if flag {
            return index;
        }
        index += 1;
    }
    -1
}

/// `aml_hextoint(str)`: the hexadecimal number at the start of `str`.
pub fn aml_hextoint(s: &[u8]) -> i64 {
    let mut v: i64 = 0;
    for &ch in s {
        let c = match ch {
            b'0'..=b'9' => ch - b'0',
            b'a'..=b'f' => ch - b'a' + 10,
            b'A'..=b'F' => ch - b'A' + 10,
            _ => break,
        };
        v = v.wrapping_shl(4).wrapping_add(i64::from(c));
    }
    v
}

/// The `length` bytes of an integer's `v_integer` (`&a->v_integer`).
fn int_bytes(a: &AmlValue) -> Vec<u8> {
    let n = a.length().clamp(0, 8) as usize;
    a.v_integer().to_le_bytes()[..n].to_vec()
}

/// `aml_tryconv(a, ctype, clen)`: `a` converted to `ctype` (`a` itself if it has that type),
/// or `None` when the conversion is not defined.
pub fn aml_tryconv(a: &AmlValueRef, ctype: i32, _clen: i32) -> Option<AmlValueRef> {
    // Object is already this type
    let atype = a.r#type();
    if atype == ctype {
        return Some(a.clone());
    }
    match ctype {
        AML_OBJTYPE_BUFFER => match atype {
            AML_OBJTYPE_INTEGER => Some(aml_allocvalue(
                AML_OBJTYPE_BUFFER,
                i64::from(a.length()),
                Bval::Bytes(&int_bytes(a)),
            )),
            AML_OBJTYPE_STRING => Some(aml_allocvalue(
                AML_OBJTYPE_BUFFER,
                i64::from(a.length()),
                Bval::Bytes(&a.v_buffer()),
            )),
            AML_OBJTYPE_BUFFERFIELD | AML_OBJTYPE_FIELDUNIT => {
                let c = aml_allocvalue(AML_OBJTYPE_BUFFER, 0, Bval::Null);
                let bitlen = a.v_field().map_or(0, |f| f.bitlen);
                aml_rwfield(a, 0, bitlen, &c, ACPI_IOREAD);
                Some(c)
            }
            _ => None,
        },
        AML_OBJTYPE_INTEGER => match atype {
            AML_OBJTYPE_BUFFER => {
                let c = aml_allocvalue(AML_OBJTYPE_INTEGER, 0, Bval::Null);
                let b = a.v_buffer();
                let mut x = [0u8; 8];
                let n = b.len().min(c.length() as usize);
                x[..n].copy_from_slice(&b[..n]);
                _aml_setvalue(&c, AML_OBJTYPE_INTEGER, i64::from_le_bytes(x), Bval::Null);
                Some(c)
            }
            AML_OBJTYPE_STRING => Some(aml_allocvalue(
                AML_OBJTYPE_INTEGER,
                aml_hextoint(&a.v_string()),
                Bval::Null,
            )),
            AML_OBJTYPE_UNINITIALIZED => Some(aml_allocvalue(AML_OBJTYPE_INTEGER, 0, Bval::Null)),
            AML_OBJTYPE_BUFFERFIELD | AML_OBJTYPE_FIELDUNIT => {
                let bitlen = a.v_field().map_or(0, |f| f.bitlen);
                if bitlen > aml_intlen() {
                    return None;
                }
                let c = aml_allocvalue(AML_OBJTYPE_INTEGER, 0, Bval::Null);
                aml_rwfield(a, 0, bitlen, &c, ACPI_IOREAD);
                Some(c)
            }
            _ => None,
        },
        AML_OBJTYPE_STRING | AML_OBJTYPE_HEXSTRING | AML_OBJTYPE_DECSTRING => match atype {
            AML_OBJTYPE_INTEGER => {
                let text = if ctype == AML_OBJTYPE_HEXSTRING {
                    alloc::format!("0x{:x}", a.v_integer())
                } else {
                    alloc::format!("{}", a.v_integer())
                };
                // snprintf into the 20 bytes of the new string.
                let text = cap(text.into_bytes(), 20);
                Some(aml_allocvalue(AML_OBJTYPE_STRING, 20, Bval::Bytes(&text)))
            }
            AML_OBJTYPE_BUFFER => Some(aml_allocvalue(
                AML_OBJTYPE_STRING,
                i64::from(a.length()),
                Bval::Bytes(&a.v_buffer()),
            )),
            AML_OBJTYPE_STRING => Some(a.clone()),
            // XXX Deal with broken Lenovo X1 BIOS.
            AML_OBJTYPE_PACKAGE => Some(aml_allocvalue(AML_OBJTYPE_STRING, 0, Bval::Null)),
            _ => None,
        },
        _ => None,
    }
}

/// `aml_convert(a, ctype, clen)`: [`aml_tryconv`], dying when it cannot.
pub fn aml_convert(a: &AmlValueRef, ctype: i32, clen: i32) -> AmlValueRef {
    match aml_tryconv(a, ctype, clen) {
        Some(c) => c,
        None => {
            aml_showvalue(Some(a));
            aml_die!(
                "aml_convert",
                "Could not convert {:x} to {:x}\n",
                a.r#type(),
                ctype
            )
        }
    }
}

/// `aml_compare(a1, a2, opcode)`: a logical comparison (`LEqual`, `LLess`, ...) of two
/// operands, the second converted to the first's type; -1 (true) or 0.
pub fn aml_compare(a1: &AmlValueRef, a2: &AmlValueRef, opcode: i32) -> i32 {
    // Convert A1 to integer, string, or buffer.
    //
    // The possible conversions listed in Table 19.6 of the ACPI spec imply that unless we
    // already got one of the three supported types, the conversion must be from field unit
    // or buffer field. In both cases, the rules (Table 19.7) state that we should convert
    // to integer if possible with buffer as a fallback.
    let t1 = a1.r#type();
    let a1 = if t1 != AML_OBJTYPE_INTEGER && t1 != AML_OBJTYPE_STRING && t1 != AML_OBJTYPE_BUFFER {
        match aml_tryconv(a1, AML_OBJTYPE_INTEGER, -1) {
            Some(cv) => cv,
            None => aml_convert(a1, AML_OBJTYPE_BUFFER, -1),
        }
    } else {
        a1.clone()
    };

    // Convert A2 to type of A1
    let a2 = aml_convert(a2, a1.r#type(), -1);
    if a1.r#type() == AML_OBJTYPE_INTEGER {
        aml_evalexpr(a1.v_integer() as u64, a2.v_integer() as u64, opcode) as i32
    } else {
        // Perform String/Buffer comparison
        let (b1, b2) = (a1.v_buffer(), a2.v_buffer());
        let n = b1.len().min(b2.len());
        let mut rc: i32 = match b1[..n].cmp(&b2[..n]) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        };
        if rc == 0 {
            // If buffers match, which one is longer
            rc = b1.len() as i32 - b2.len() as i32;
        }
        // Perform comparison against zero (as the C: unsigned)
        aml_evalexpr(i64::from(rc) as u64, 0, opcode) as i32
    }
}

/// `aml_concat(a1, a2)`: `Concatenate`: two integers into a buffer, or two buffers or
/// strings (the second converted to the first's type).
pub fn aml_concat(a1: &AmlValueRef, a2: &AmlValueRef) -> AmlValueRef {
    // Make A1 an integer, string, or buffer. Unless we already got one of these three
    // types, convert to string.
    let t1 = a1.r#type();
    let a1 = if t1 != AML_OBJTYPE_INTEGER && t1 != AML_OBJTYPE_STRING && t1 != AML_OBJTYPE_BUFFER {
        aml_convert(a1, AML_OBJTYPE_STRING, -1)
    } else {
        a1.clone()
    };

    // Convert arg2 to type of arg1
    let a2 = aml_convert(a2, a1.r#type(), -1);
    let (b1, b2) = match a1.r#type() {
        AML_OBJTYPE_INTEGER => (int_bytes(&a1), int_bytes(&a2)),
        AML_OBJTYPE_BUFFER | AML_OBJTYPE_STRING => (a1.v_buffer(), a2.v_buffer()),
        t => aml_die!(
            "aml_concat",
            "concat type mismatch {} != {}\n",
            t,
            a2.r#type()
        ),
    };
    let mut c = b1;
    c.extend_from_slice(&b2);
    let r#type = if a1.r#type() == AML_OBJTYPE_STRING {
        AmlObj::String(c)
    } else {
        AmlObj::Buffer(c)
    };
    Rc::new(AmlValue::from_obj(r#type))
}

/// `aml_concatres(a1, a2)`: `ConcatenateResTemplate`: the descriptors of two resource
/// templates and a new end tag.
pub fn aml_concatres(a1: &AmlValueRef, a2: &AmlValueRef) -> AmlValueRef {
    if a1.r#type() != AML_OBJTYPE_BUFFER || a2.r#type() != AML_OBJTYPE_BUFFER {
        aml_die!("aml_concatres", "concatres: not buffers\n");
    }

    // Walk a1, a2, get length minus end tags, concatenate buffers, add end tag
    let (mut l1, mut l2) = (0usize, 0usize);
    aml_parse_resource(a1, &mut |_, rs| {
        l1 += aml_crslen(rs) as usize; // aml_ccrlen
        0
    });
    aml_parse_resource(a2, &mut |_, rs| {
        l2 += aml_crslen(rs) as usize;
        0
    });

    // Concatenate buffers, add end tag
    let (b1, b2) = (a1.v_buffer(), a2.v_buffer());
    let mut c = Vec::with_capacity(l1 + l2 + 2);
    c.extend_from_slice(&b1[..l1.min(b1.len())]);
    c.extend_from_slice(&b2[..l2.min(b2.len())]);
    c.resize(l1 + l2, 0);
    c.extend_from_slice(&[SRT_ENDTAG, 0x00]);
    Rc::new(AmlValue::from_obj(AmlObj::Buffer(c)))
}

/// `aml_mid(src, index, length)`: `Mid`: `length` bytes of a string or buffer from `index`.
pub fn aml_mid(src: &AmlValue, mut index: i32, mut length: i32) -> AmlValueRef {
    let len = src.length();
    if index > len {
        index = len;
    }
    if index + length > len {
        length = len - index;
    }
    let b = src.v_buffer();
    let from = (index.max(0) as usize).min(b.len());
    aml_allocvalue(src.r#type(), i64::from(length), Bval::Bytes(&b[from..]))
}

/// `aml_evalhid(node, val)`: the device's `_HID`, an EISA id turned into its string; -1 when
/// it has none.
pub fn aml_evalhid(node: &AmlNodeRef, val: &AmlValue) -> i32 {
    if aml_evalname(acpi_softc(), Some(node), b"_HID", &[], Some(val)) != 0 {
        return -1;
    }

    // Integer _HID: convert to EISA ID
    if val.r#type() == AML_OBJTYPE_INTEGER {
        let id = aml_eisaid(val.v_integer() as u32);
        _aml_setvalue(val, AML_OBJTYPE_STRING, -1, Bval::Bytes(&id));
    }
    0
}

/// An address space `acpi_gasio` serves, reached through `value`'s bytes.
fn opreg_gasio(iodir: i32, space: i32, address: u64, size: i32, value: &mut u64) -> i32 {
    let mut b = value.to_le_bytes();
    let n = (size.clamp(0, 8)) as usize;
    let r = acpi_gasio(acpi_softc(), iodir, space, address, size, size, &mut b[..n]);
    *value = u64::from_le_bytes(b);
    r
}

/// `aml_opreg_sysmem_handler`: `SystemMemory` regions.
pub fn aml_opreg_sysmem_handler(
    _cookie: *mut c_void,
    iodir: i32,
    address: u64,
    size: i32,
    value: &mut u64,
) -> i32 {
    opreg_gasio(iodir, GAS_SYSTEM_MEMORY, address, size, value)
}

/// `aml_opreg_sysio_handler`: `SystemIO` regions.
pub fn aml_opreg_sysio_handler(
    _cookie: *mut c_void,
    iodir: i32,
    address: u64,
    size: i32,
    value: &mut u64,
) -> i32 {
    opreg_gasio(iodir, GAS_SYSTEM_IOSPACE, address, size, value)
}

/// `aml_opreg_pcicfg_handler`: `PCI_Config` regions.
pub fn aml_opreg_pcicfg_handler(
    _cookie: *mut c_void,
    iodir: i32,
    address: u64,
    size: i32,
    value: &mut u64,
) -> i32 {
    opreg_gasio(iodir, GAS_PCI_CFG_SPACE, address, size, value)
}

/// `aml_opreg_ec_handler`: `EmbeddedControl` regions.
pub fn aml_opreg_ec_handler(
    _cookie: *mut c_void,
    iodir: i32,
    address: u64,
    size: i32,
    value: &mut u64,
) -> i32 {
    opreg_gasio(iodir, GAS_EMBEDDED, address, size, value)
}

/// `aml_register_regionspace(node, iospace, cookie, handler)`: routes address space
/// `iospace` to `handler` and tells the firmware (`_REG(iospace, 1)` below `node`).
pub fn aml_register_regionspace(
    node: &AmlNodeRef,
    iospace: i32,
    cookie: *mut c_void,
    handler: AmlRegionHandler,
) {
    kassert!((0..256).contains(&iospace));

    let table = AML_REGIONSPACE.get();
    table[(iospace & 0xff) as usize].set(AmlRegionspace {
        cookie,
        handler: Some(handler),
    });

    // Register address space.
    let arg = [AmlValue::integer(i64::from(iospace)), AmlValue::integer(1)];
    let node = aml_searchname(Some(node), b"_REG");
    if node.is_some() {
        aml_evalnode(acpi_softc(), node.as_ref(), &arg, None);
    }
}

/// `aml_rdpciaddr(pcidev, addr)`: the PCI segment, bus, device and function of the device
/// an operation region belongs to: `_SEG`/`_ADR` of the path from the root, crossing PCI
/// bridges.
pub fn aml_rdpciaddr(pcidev: &AmlNodeRef, addr: &mut AmlpciT) -> i32 {
    // invert
    let mut path: Vec<AmlNodeRef> = Vec::new();
    let mut cur = Some(pcidev.clone());
    loop {
        let Some(n) = cur else { break };
        cur = n.parent();
        path.push(n);
        if cur.is_none() || path.len() == 10 {
            break;
        }
    }
    // What the C's `pcidev` is after the loop: the node above the path, usually NULL.
    let above = cur;

    // start from root
    addr.set_seg(0);
    let mut res: i64 = 0;
    for n in (0..path.len()).rev() {
        if aml_evalinteger(acpi_softc(), Some(&path[n]), b"_ADR", &[], &mut res) == 0 {
            addr.set_dev((res >> 16) as u64);
            addr.set_fun((res & 0xFFFF) as u64);
        } else if __aml_search(Some(&path[n]), b"_HID", false).is_some() {
            // HID device (PCI or PCIE root): eval _SEG and _BBN
            if aml_evalinteger(acpi_softc(), Some(&path[n]), b"_SEG", &[], &mut res) == 0 {
                addr.set_seg(res as u64);
                addr.set_bus(0);
                addr.set_dev(0);
                addr.set_fun(0);
            }
            if aml_evalinteger(acpi_softc(), above.as_ref(), b"_BBN", &[], &mut res) == 0 {
                addr.set_bus(res as u64);
                addr.set_dev(0);
                addr.set_fun(0);
            }
        } else {
            continue;
        }

        if n == 0 {
            break;
        }

        // an intermediate device, if it's a bridge jump busses
        let Some(pc) = pci_lookup_segment(
            i32::from(acpi_pci_seg(addr.addr)),
            i32::from(acpi_pci_bus(addr.addr)),
        ) else {
            continue;
        };
        let tag = pci_make_tag(
            pc,
            i32::from(addr.bus()),
            i32::from(addr.dev()),
            i32::from(addr.fun()),
        );
        let reg = pci_conf_read(pc, tag, PCI_CLASS_REG);
        if pci_class(reg) == PCI_CLASS_BRIDGE && pci_subclass(reg) == PCI_SUBCLASS_BRIDGE_PCI {
            let reg = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
            addr.set_bus(u64::from(ppb_businfo_secondary(reg)));
            addr.set_dev(0);
            addr.set_fun(0);
        }
    }
    0
}

/// `acpi_genio(sc, iodir, iospace, address, access_size, len, buffer)`: `len` bytes of
/// address space `iospace` at `address`, `access_size` at a time, through the space's
/// handler; 0 or the handler's error (-1 for a bad access size).
pub fn acpi_genio(
    _sc: Option<&AcpiSoftc>,
    iodir: i32,
    iospace: i32,
    address: u64,
    access_size: i32,
    len: i32,
    buffer: &mut [u8],
) -> i32 {
    let region = AML_REGIONSPACE.get()[(iospace & 0xff) as usize].get();
    let Some(handler) = region.handler else {
        return -1;
    };

    kassert!(access_size > 0 && len % access_size == 0);
    if access_size <= 0 {
        return -1;
    }

    let mut reg = 0;
    while reg < len {
        let at = reg as usize;
        let sz = access_size as usize;
        let mut value: u64 = 0;
        if iodir == ACPI_IOREAD {
            let err = handler(
                region.cookie,
                iodir,
                address.wrapping_add(reg as u64),
                access_size,
                &mut value,
            );
            if err != 0 {
                return err;
            }
            match access_size {
                1 | 2 | 4 => {
                    let b = value.to_le_bytes();
                    for (i, &x) in b.iter().take(sz).enumerate() {
                        if let Some(d) = buffer.get_mut(at + i) {
                            *d = x;
                        }
                    }
                }
                _ => {
                    kprintf!("acpi_genio: invalid access size {} on read\n", access_size);
                    return -1;
                }
            }
        } else {
            match access_size {
                1 | 2 | 4 => {
                    let mut b = [0u8; 8];
                    for (i, x) in b.iter_mut().take(sz).enumerate() {
                        *x = buffer.get(at + i).copied().unwrap_or(0);
                    }
                    value = u64::from_le_bytes(b);
                }
                _ => {
                    kprintf!("acpi_genio: invalid access size {} on write\n", access_size);
                    return -1;
                }
            }
            let err = handler(
                region.cookie,
                iodir,
                address.wrapping_add(reg as u64),
                access_size,
                &mut value,
            );
            if err != 0 {
                return err;
            }
        }
        reg += access_size;
    }
    0
}

/// The bytes a field access reads a value's bits from (`&val->v_integer` or
/// `val->v_buffer`): an integer's eight bytes, a buffer's or string's bytes, else zeros.
fn value_bits(val: &AmlValue) -> Vec<u8> {
    match &*val.obj() {
        AmlObj::Integer(i) => i.to_le_bytes().to_vec(),
        AmlObj::Buffer(b) | AmlObj::String(b) => b.clone(),
        _ => vec![0; 8],
    }
}

/// The integer held in the first eight bytes of `b`.
fn le_i64(b: &[u8]) -> i64 {
    let mut x = [0u8; 8];
    let n = b.len().min(8);
    x[..n].copy_from_slice(&b[..n]);
    i64::from_le_bytes(x)
}

/// `roundup(x, y)`.
const fn roundup(x: i32, y: i32) -> i32 {
    ((x + (y - 1)) / y) * y
}

/// `aml_rwgen(rgn, bpos, blen, val, mode, flag)`: reads (into `val`) or writes (from it)
/// `blen` bits at bit `bpos` of operation region `rgn`, with the access width and update
/// rule of `flag`.
pub fn aml_rwgen(rgn: &AmlValue, bpos: i32, blen: i32, val: &AmlValueRef, mode: i32, flag: i32) {
    let r = rgn.v_opregion().unwrap_or_default();
    let mut bpos = bpos;
    let mut blen = blen;

    // Get field access size
    let sz: i32 = match aml_field_access(flag) {
        AML_FIELD_WORDACC => 2,
        AML_FIELD_DWORDACC => 4,
        AML_FIELD_QWORDACC => 8,
        _ => 1,
    };

    let mut pi = AmlpciT {
        addr: r.iobase.wrapping_add((bpos >> 3) as u64) & !((sz - 1) as u64),
    };
    bpos += ((r.iobase & (sz - 1) as u64) << 3) as i32;
    bpos &= (sz << 3) - 1;

    if i32::from(r.iospace) == ACPI_OPREG_PCICFG {
        // Get PCI Root Address for this opregion
        if let Some(parent) = rgn.node().and_then(|n| n.parent()) {
            aml_rdpciaddr(&parent, &mut pi);
        }
    }

    let tlen = roundup(bpos + blen, sz << 3);
    let r#type = i32::from(r.iospace);

    if AML_REGIONSPACE.get()[r#type as usize]
        .get()
        .handler
        .is_none()
    {
        kprintf!("aml_rwgen: unregistered RegionSpace 0x{:x}\n", r#type);
        return;
    }

    // Allocate temporary storage
    let tmp_is_buffer = tlen > aml_intlen();
    let mut tbit = vec![0u8; ((tlen >> 3) as usize).max(8)];

    let big = blen > aml_intlen();
    let mut vbit: Vec<u8> = if mode == ACPI_IOREAD {
        // Read from a large field: create buffer; from a short field: an integer
        if big {
            vec![0u8; ((blen + 7) >> 3) as usize]
        } else {
            vec![0u8; 8]
        }
    } else if big {
        // Write to a large field.. create or convert buffer
        let v = aml_convert(val, AML_OBJTYPE_BUFFER, -1);
        if blen > v.length() << 3 {
            blen = v.length() << 3;
        }
        v.v_buffer()
    } else {
        // Write to a short field.. convert to integer
        let v = aml_convert(val, AML_OBJTYPE_INTEGER, -1);
        v.v_integer().to_le_bytes().to_vec()
    };

    let sc = acpi_softc();
    if mode == ACPI_IOREAD {
        // Read bits from opregion
        acpi_genio(sc, ACPI_IOREAD, r#type, pi.addr, sz, tlen >> 3, &mut tbit);
        aml_bufcpy(&mut vbit, 0, &tbit, bpos, blen);
        if big {
            val.set_obj(AmlObj::Buffer(vbit));
        } else {
            val.set_obj(AmlObj::Integer(le_i64(&vbit)));
        }
    } else {
        // Write bits to opregion
        if aml_field_update(flag) == AML_FIELD_PRESERVE && (bpos != 0 || blen != tlen) {
            acpi_genio(sc, ACPI_IOREAD, r#type, pi.addr, sz, tlen >> 3, &mut tbit);
        } else if aml_field_update(flag) == AML_FIELD_WRITEASONES && tmp_is_buffer {
            // (an integer temporary has a length of 0 in C: nothing is set)
            tbit.fill(0xff);
        }
        // Copy target bits, then write to region
        aml_bufcpy(&mut tbit, bpos, &vbit, 0, blen);
        acpi_genio(sc, ACPI_IOWRITE, r#type, pi.addr, sz, tlen >> 3, &mut tbit);
    }
}

/// `aml_rwgpio(conn, bpos, blen, val, mode, flag)`: a `GeneralPurposeIO` field: one pin of
/// the controller the `GpioIo` connection names.
pub fn aml_rwgpio(conn: &AmlValue, bpos: i32, blen: i32, val: &AmlValue, mode: i32, _flag: i32) {
    let b = match &*conn.obj() {
        AmlObj::Buffer(b) => b.clone(),
        _ => Vec::new(),
    };
    let crs = AcpiResource::new(&b);
    if b.len() < 5 || aml_crstype(&crs) != LR_GPIO || aml_crslen(&crs) as usize > b.len() {
        aml_die!("aml_rwgpio", "Invalid GpioIo");
    }
    if bpos != 0 || blen != 1 {
        aml_die!("aml_rwgpio", "Invalid GpioIo access");
    }

    let res_off = usize::from(crs.lr_gpio_res_off());
    let name = cstr(b.get(res_off..).unwrap_or(&[])).to_vec();
    let node = aml_searchname(conn.node().as_ref(), &name);
    let pin_off = usize::from(crs.lr_gpio_pin_off());
    let pin = u16::from_le_bytes([crs.byte(pin_off), crs.byte(pin_off + 1)]);

    let Some(gpio) = node.and_then(|n| n.gpio.get()) else {
        aml_die!("aml_rwgpio", "Could not find GpioIo pin");
    };

    if mode == ACPI_IOWRITE {
        let v = aml_val2int(Some(val)) as i32;
        (gpio.write_pin)(gpio.cookie, i32::from(pin), v);
    } else {
        let v = (gpio.read_pin)(gpio.cookie, i32::from(pin));
        _aml_setvalue(val, AML_OBJTYPE_INTEGER, i64::from(v), Bval::Null);
    }
}

/// `aml_rwgsb(conn, len, bpos, blen, val, mode, flag)`: a `GenericSerialBus` field over the
/// I2C connection `conn`. The status byte of the result is the transfer's error; without
/// `dev/i2c` no controller is ever found, so it is `EIO`, as the C answers then.
pub fn aml_rwgsb(
    conn: &AmlValue,
    len: i32,
    bpos: i32,
    _blen: i32,
    val: &AmlValue,
    mode: i32,
    flag: i32,
) {
    let b = match &*conn.obj() {
        AmlObj::Buffer(b) => b.clone(),
        _ => Vec::new(),
    };
    let crs = AcpiResource::new(&b);
    if b.len() < 5
        || aml_crstype(&crs) != LR_SERBUS
        || aml_crslen(&crs) as usize > b.len()
        || crs.lr_i2cbus_revid() != 1
        || crs.lr_i2cbus_type() != LR_SERBUS_I2C
    {
        aml_die!("aml_rwgsb", "Invalid GenericSerialBus");
    }
    if aml_field_access(flag) != AML_FIELD_BUFFERACC || bpos & 0x3 != 0 {
        aml_die!("aml_rwgsb", "Invalid GenericSerialBus access");
    }

    let at = (LR_I2CBUS_VDATA + usize::from(crs.lr_i2cbus_tlength())).saturating_sub(6);
    let name = cstr(b.get(at..).unwrap_or(&[])).to_vec();
    let mut node = aml_searchname(conn.node().as_ref(), &name);

    let (_cmdlen, buflen) = match (flag >> 6) & 0x3 {
        // Normal
        0 => match aml_field_attr(flag) {
            0x02 => (0, 0),   // AttribQuick
            0x04 => (0, 1),   // AttribSendReceive
            0x06 => (1, 1),   // AttribByte
            0x08 => (1, 2),   // AttribWord
            0x0b => (1, len), // AttribBytes
            0x0e => (0, len), // AttribRawBytes
            0x0f => {
                // AttribRawProcessBytes: XXX Not implemented yet but used by various WoA
                // laptops. Force an error status instead of a panic for now.
                node = None;
                (0, len)
            }
            _ => aml_die!("aml_rwgsb", "unsupported access type 0x{:x}", flag),
        },
        1 => (1, aml_field_attr(flag)), // AttribBytes
        2 => (0, aml_field_attr(flag)), // AttribRawBytes
        _ => aml_die!("aml_rwgsb", "unsupported access type 0x{:x}", flag),
    };

    if mode == ACPI_IOREAD {
        _aml_setvalue(val, AML_OBJTYPE_BUFFER, i64::from(buflen) + 2, Bval::Null);
    }

    // Return an error if we can't find the I2C controller that we're supposed to use for
    // this request: `struct aml_node` has no `i2c` until dev/i2c is ported.
    let _ = node;
    val.with_obj(|o| {
        if let AmlObj::Buffer(buf) = o
            && let Some(b0) = buf.first_mut()
        {
            *b0 = Errno::EIO as i32 as u8;
        }
    });
}

/// `aml_rwindexfield(fld, val, mode)`: an `IndexField`: each access unit is selected by
/// writing its offset to the index field, then read or written through the data field.
pub fn aml_rwindexfield(fld: &AmlValue, val: &AmlValueRef, mode: i32) {
    let Some(f) = fld.v_field() else { return };
    let (Some(ref1), Some(ref2)) = (f.ref1, f.ref2) else {
        return;
    };
    let mut bpos = f.bitpos;
    let mut blen = f.bitlen;

    let tmp = Rc::new(AmlValue::new());

    // Get field access size
    let sz: i32 = match aml_field_access(f.flags) {
        AML_FIELD_WORDACC => 2,
        AML_FIELD_DWORDACC => 4,
        AML_FIELD_QWORDACC => 8,
        _ => 1,
    };

    let big = blen > aml_intlen();
    if mode == ACPI_IOREAD {
        if big {
            // Read from a large field: create buffer
            _aml_setvalue(
                val,
                AML_OBJTYPE_BUFFER,
                i64::from((blen + 7) >> 3),
                Bval::Null,
            );
        } else {
            // Read from a short field: initialize integer
            _aml_setvalue(val, AML_OBJTYPE_INTEGER, 0, Bval::Null);
        }
    }
    let mut vbit = value_bits(val);
    let mut vpos = 0;

    let mut indexval = (bpos >> 3) & !(sz - 1);
    bpos -= indexval << 3;

    while blen > 0 {
        let len = blen.min((sz << 3) - bpos);

        // Write index register
        _aml_setvalue(&tmp, AML_OBJTYPE_INTEGER, i64::from(indexval), Bval::Null);
        aml_rwfield(&ref2, 0, aml_intlen(), &tmp, ACPI_IOWRITE);
        indexval += sz;

        // Read/write data register
        _aml_setvalue(&tmp, AML_OBJTYPE_INTEGER, 0, Bval::Null);
        if mode == ACPI_IOWRITE {
            let mut tbit = [0u8; 8];
            aml_bufcpy(&mut tbit, 0, &vbit, vpos, len);
            _aml_setvalue(
                &tmp,
                AML_OBJTYPE_INTEGER,
                i64::from_le_bytes(tbit),
                Bval::Null,
            );
        }
        aml_rwfield(&ref1, bpos, len, &tmp, mode);
        if mode == ACPI_IOREAD {
            aml_bufcpy(&mut vbit, vpos, &value_bits(&tmp), 0, len);
        }
        vpos += len;
        blen -= len;
        bpos = 0;
    }

    if mode == ACPI_IOREAD {
        if big {
            val.set_obj(AmlObj::Buffer(vbit));
        } else {
            val.set_obj(AmlObj::Integer(le_i64(&vbit)));
        }
    }
}

/// `aml_rwfield(fld, bpos, blen, val, mode)`: reads (into `val`) or writes (from `val`)
/// `blen` bits at bit `bpos` of field unit or buffer field `fld`.
pub fn aml_rwfield(fld: &AmlValue, bpos: i32, blen: i32, val: &AmlValueRef, mode: i32) {
    let Some(f) = fld.v_field() else { return };
    let blen = blen.min(f.bitlen);

    aml_lockfield(None, fld);
    let tmp = Rc::new(AmlValue::new());
    if f.r#type == AMLOP_INDEXFIELD {
        aml_rwindexfield(fld, val, mode);
    } else if f.r#type == AMLOP_BANKFIELD {
        _aml_setvalue(&tmp, AML_OBJTYPE_INTEGER, i64::from(f.ref3), Bval::Null);
        if let Some(ref2) = &f.ref2 {
            aml_rwfield(ref2, 0, aml_intlen(), &tmp, ACPI_IOWRITE);
        }
        if let Some(ref1) = &f.ref1 {
            aml_rwgen(ref1, f.bitpos, f.bitlen, val, mode, f.flags);
        }
    } else if f.r#type == AMLOP_FIELD {
        let iospace = f
            .ref1
            .as_ref()
            .and_then(|r| r.v_opregion())
            .map_or(0, |r| i32::from(r.iospace));
        match (iospace, &f.ref1, &f.ref2) {
            (ACPI_OPREG_GPIO, _, Some(ref2)) => aml_rwgpio(ref2, bpos, blen, val, mode, f.flags),
            (ACPI_OPREG_GSB, _, Some(ref2)) => {
                aml_rwgsb(ref2, f.ref3, f.bitpos + bpos, blen, val, mode, f.flags)
            }
            (_, Some(ref1), _) => aml_rwgen(ref1, f.bitpos + bpos, blen, val, mode, f.flags),
            _ => {}
        }
    } else if mode == ACPI_IOREAD {
        // bufferfield:read
        let src = f.ref1.as_ref().map(|r| r.v_buffer()).unwrap_or_default();
        if f.bitlen > aml_intlen() {
            let mut b = vec![0u8; ((f.bitlen + 7) >> 3) as usize];
            aml_bufcpy(&mut b, 0, &src, f.bitpos, f.bitlen);
            val.set_obj(AmlObj::Buffer(b));
        } else {
            let mut b = [0u8; 8];
            aml_bufcpy(&mut b, 0, &src, f.bitpos, f.bitlen);
            val.set_obj(AmlObj::Integer(i64::from_le_bytes(b)));
        }
    } else {
        // bufferfield:write
        let src = if f.bitlen > aml_intlen() {
            aml_convert(val, AML_OBJTYPE_BUFFER, -1).v_buffer()
        } else {
            aml_convert(val, AML_OBJTYPE_INTEGER, -1)
                .v_integer()
                .to_le_bytes()
                .to_vec()
        };
        if let Some(ref1) = &f.ref1 {
            let bitlen = f.bitlen.min(src.len() as i32 * 8);
            ref1.with_obj(|o| {
                if let AmlObj::Buffer(b) = o {
                    aml_bufcpy(b, f.bitpos, &src, 0, bitlen);
                }
            });
        }
    }
    aml_unlockfield(None, fld);
}

/// `aml_createfield(field, opcode, data, bpos, blen, index, indexval, flags)`: makes `field`
/// a field unit (`Field`, `IndexField`, `BankField`) or a buffer field over `data`.
///
/// ```text
///  Create Field Object          data            index
///    AMLOP_FIELD                n:OpRegion      NULL
///    AMLOP_INDEXFIELD           n:Field         n:Field
///    AMLOP_BANKFIELD            n:OpRegion      n:Field
///    AMLOP_CREATEFIELD          t:Buffer        NULL
///    AMLOP_CREATEBITFIELD       t:Buffer        NULL
///    AMLOP_CREATEBYTEFIELD      t:Buffer        NULL
///    AMLOP_CREATEWORDFIELD      t:Buffer        NULL
///    AMLOP_CREATEDWORDFIELD     t:Buffer        NULL
///    AMLOP_CREATEQWORDFIELD     t:Buffer        NULL
///    AMLOP_INDEX                t:Buffer        NULL
/// ```
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn aml_createfield(
    field: &AmlValue,
    opcode: i32,
    data: &AmlValueRef,
    bpos: i32,
    blen: i32,
    index: Option<&AmlValueRef>,
    indexval: i32,
    flags: i32,
) {
    let unit = opcode == AMLOP_FIELD || opcode == AMLOP_INDEXFIELD || opcode == AMLOP_BANKFIELD;
    let data = if !unit && data.r#type() != AML_OBJTYPE_BUFFER {
        aml_convert(data, AML_OBJTYPE_BUFFER, -1)
    } else {
        data.clone()
    };
    let f = AmlField {
        r#type: opcode,
        flags,
        bitpos: bpos,
        bitlen: blen,
        ref1: Some(data),
        ref2: index.cloned(),
        ref3: indexval,
    };
    field.set_obj(if unit {
        AmlObj::FieldUnit(f)
    } else {
        AmlObj::BufferField(f)
    });
}

/// `aml_parsefieldlist(mscope, opcode, flags, data, index, indexval)`: makes the named
/// fields of a `Field`/`IndexField`/`BankField` list, then pops `mscope`.
pub fn aml_parsefieldlist(
    mscope: Option<Rc<AmlScope>>,
    opcode: i32,
    flags: i32,
    data: &AmlValueRef,
    index: Option<&AmlValueRef>,
    indexval: i32,
) {
    let Some(mscope) = mscope else { return };
    let mut flags = flags;
    let mut indexval = indexval;
    let mut conn: Option<AmlValueRef> = None;
    let mut bpos = 0;

    let next = |s: &AmlScope| {
        let b = s.cur().get8();
        s.advance(1);
        i32::from(b)
    };

    while mscope.cur() < mscope.end {
        let blen = match mscope.cur().get8() {
            0x00 => {
                // ReservedField
                mscope.advance(1);
                aml_parselength(&mscope)
            }
            0x01 => {
                // AccessField
                mscope.advance(1);
                flags = next(&mscope);
                flags |= next(&mscope) << 8;
                0
            }
            0x02 => {
                // ConnectionField
                mscope.advance(1);
                let Some(c) = aml_parse(Some(&mscope), b'o', "Connection") else {
                    aml_die!("aml_parsefieldlist", "Could not parse connection");
                };
                c.set_node(mscope.node.as_ref());
                conn = Some(c);
                0
            }
            0x03 => {
                // ExtendedAccessField
                mscope.advance(1);
                flags = next(&mscope);
                flags |= next(&mscope) << 8;
                indexval = next(&mscope);
                0
            }
            _ => {
                // NamedField
                let (pos, rv) = aml_parsename(mscope.node.as_ref(), mscope.cur(), true);
                mscope.pos.set(Some(pos));
                let blen = aml_parselength(&mscope);
                aml_createfield(
                    &rv,
                    opcode,
                    data,
                    bpos,
                    blen,
                    conn.as_ref().or(index),
                    indexval,
                    flags,
                );
                blen
            }
        };
        bpos += blen;
    }
    aml_popscope(mscope);
}

/// `acpi_mutex_acquire(scope, mtx, timeout)`: `Acquire`: 0 when `scope` now owns `mtx`
/// (taking the firmware's global lock for `\_GL`), -1 when it is held and `timeout` is 0.
/// Waiting is not implemented, as in C: a held mutex with a timeout is reported acquired.
pub fn acpi_mutex_acquire(scope: &Rc<AmlScope>, mtx: &AmlValueRef, timeout: i32) -> i32 {
    let me = Rc::as_ptr(scope);
    let owner = match &*mtx.obj() {
        AmlObj::Mutex(m) => m.owner,
        _ => ptr::null(),
    };
    if owner.is_null() || ptr::eq(owner, me) {
        // We are now the owner
        mtx.with_obj(|o| {
            if let AmlObj::Mutex(m) = o {
                m.owner = me;
            }
        });
        if aml_global_lock().is_some_and(|gl| Rc::ptr_eq(&gl, mtx)) {
            acpi_glk_enter();
        }
        return 0;
    } else if timeout == 0 {
        return -1;
    }
    // Wait for mutex
    0
}

/// `acpi_mutex_release(scope, mtx)`: `Release`.
pub fn acpi_mutex_release(_scope: &Rc<AmlScope>, mtx: &AmlValueRef) {
    if aml_global_lock().is_some_and(|gl| Rc::ptr_eq(&gl, mtx)) {
        acpi_glk_leave();
    }
    mtx.with_obj(|o| {
        if let AmlObj::Mutex(m) = o {
            m.owner = ptr::null();
        }
    });
    // Wakeup waiters
}

/// `v_evt.state` of an event (0 for another type).
fn evt_state(evt: &AmlValue) -> i32 {
    match &*evt.obj() {
        AmlObj::Event(s) => *s,
        _ => 0,
    }
}

/// Sets `v_evt.state` of an event.
fn set_evt_state(evt: &AmlValue, f: impl FnOnce(i32) -> i32) {
    evt.with_obj(|o| {
        if let AmlObj::Event(s) = o {
            *s = f(*s);
        }
    });
}

/// `acpi_event_wait(scope, evt, timeout)`: `Wait`: 0 once `evt` was signalled (consuming
/// one signal), -1 after `timeout` milliseconds. The acpi thread's tasks run meanwhile.
pub fn acpi_event_wait(_scope: &Rc<AmlScope>, evt: &AmlValueRef, mut timeout: i32) -> i32 {
    // Wait for event to occur; do work in meantime
    while evt_state(evt) == 0 && timeout >= 0 {
        let sc = acpi_softc();
        if let Some(sc) = sc
            && acpi_dotask(sc) != 0
        {
            continue;
        }
        match sc {
            Some(sc) if !COLD.load(Ordering::Relaxed) => {
                let r = rwsleep(Rc::as_ptr(evt), &sc.sc_lck, PWAIT, "acpievt", 1);
                if r == Err(Errno::EWOULDBLOCK) && timeout < AML_NO_TIMEOUT {
                    timeout -= 1000 / crate::conf::param::HZ.load(Ordering::Relaxed);
                }
            }
            _ => {
                delay(1000);
                if timeout < AML_NO_TIMEOUT {
                    timeout -= 1;
                }
            }
        }
    }
    if evt_state(evt) == 0 {
        return -1;
    }
    set_evt_state(evt, |s| s - 1);
    0
}

/// `acpi_event_signal(scope, evt)`: `Signal`.
pub fn acpi_event_signal(_scope: &Rc<AmlScope>, evt: &AmlValueRef) {
    set_evt_state(evt, |s| s + 1);
    if evt_state(evt) > 0 {
        wakeup_one(Rc::as_ptr(evt));
    }
}

/// `acpi_event_reset(scope, evt)`: `Reset`.
pub fn acpi_event_reset(_scope: &Rc<AmlScope>, evt: &AmlValueRef) {
    set_evt_state(evt, |_| 0);
}

/// `aml_store(scope, lhs, ival, rhs)`: stores `rhs` (or the integer `ival` when it is NULL)
/// into the target `lhs`, with the implicit conversions of `Store`.
pub fn aml_store(
    scope: &Rc<AmlScope>,
    lhs: Option<&AmlValueRef>,
    ival: i64,
    rhs: Option<&AmlValueRef>,
) {
    // Already set
    let Some(lhs) = lhs else { return };
    if rhs.is_some_and(|r| Rc::ptr_eq(lhs, r)) || lhs.r#type() == AML_OBJTYPE_NOTARGET {
        return;
    }
    let tmp = Rc::new(AmlValue::new());
    let mut rhs = match rhs {
        Some(r) => r.clone(),
        None => {
            _aml_setvalue(&tmp, AML_OBJTYPE_INTEGER, ival, Bval::Null);
            tmp.clone()
        }
    };
    if matches!(
        rhs.r#type(),
        AML_OBJTYPE_BUFFERFIELD | AML_OBJTYPE_FIELDUNIT
    ) {
        let bitlen = rhs.v_field().map_or(0, |f| f.bitlen);
        aml_rwfield(&rhs, 0, bitlen, &tmp, ACPI_IOREAD);
        rhs = tmp.clone();
    }
    let is_local = |v: &AmlValue| (AMLOP_LOCAL0..=AMLOP_LOCAL7).contains(&v.stack.get());

    // Store to LocalX: free value
    if is_local(lhs) {
        aml_freevalue(Some(lhs));
    }

    let lhs = aml_gettgt(Some(lhs), AMLOP_STORE).unwrap_or_else(|| lhs.clone());

    // Store to LocalX: free value again
    if is_local(&lhs) {
        aml_freevalue(Some(&lhs));
    }
    match lhs.r#type() {
        AML_OBJTYPE_UNINITIALIZED => aml_copyvalue(&lhs, &rhs),
        AML_OBJTYPE_BUFFERFIELD | AML_OBJTYPE_FIELDUNIT => {
            let bitlen = lhs.v_field().map_or(0, |f| f.bitlen);
            aml_rwfield(&lhs, 0, bitlen, &rhs, ACPI_IOWRITE);
        }
        AML_OBJTYPE_DEBUGOBJ => {}
        AML_OBJTYPE_INTEGER => {
            let r = aml_convert(&rhs, AML_OBJTYPE_INTEGER, -1);
            lhs.set_obj(AmlObj::Integer(r.v_integer()));
        }
        t @ (AML_OBJTYPE_BUFFER | AML_OBJTYPE_STRING) => {
            let r = aml_convert(&rhs, t, -1);
            let rb = r.v_buffer();
            let mut lb = lhs.v_buffer();
            if lb.len() < rb.len() {
                lb = vec![0u8; rb.len()];
            }
            lb.fill(0);
            lb[..rb.len()].copy_from_slice(&rb);
            lhs.set_obj(if t == AML_OBJTYPE_STRING {
                AmlObj::String(lb)
            } else {
                AmlObj::Buffer(lb)
            });
        }
        AML_OBJTYPE_PACKAGE => {
            // Convert to LHS type, copy into LHS
            if rhs.r#type() != AML_OBJTYPE_PACKAGE {
                aml_die!("aml_store", "Copy non-package into package?");
            }
            aml_copyvalue(&lhs, &rhs);
        }
        AML_OBJTYPE_NAMEREF => {
            let name = aml_getname(lhs.v_nameref().unwrap_or(AML_NULL).tail());
            let node = __aml_searchname(scope.node.as_ref(), &name, true);
            match node.and_then(|n| n.value()) {
                Some(v) => aml_copyvalue(&v, &rhs),
                None => aml_die!("aml_store", "Could not create node {}", Str(&name)),
            }
        }
        AML_OBJTYPE_OBJREF => {
            let o = lhs.v_objref().unwrap_or_default();
            if o.r#type != AMLOP_PACKAGE {
                aml_die!("aml_store", "Package expected");
            }
            if let Some(r) = &o.r#ref {
                aml_copyvalue(r, &rhs);
            }
        }
        AML_OBJTYPE_METHOD => {
            // Method override
            if rhs.r#type() != AML_OBJTYPE_INTEGER {
                aml_die!("aml_store", "Overriding a method with a non-int?");
            }
            aml_copyvalue(&lhs, &rhs);
        }
        t => aml_die!("aml_store", "Store to default type!	 {:x}\n", t),
    }
}

/// `aml_eval(scope, my_ret, ret_type, argc, argv)`: the value of an object: a method is run
/// (with `argv`, or with arguments parsed from `scope`'s byte code when `argv` is `None`),
/// a field is read, anything else is itself.
pub fn aml_eval(
    scope: Option<&Rc<AmlScope>>,
    my_ret: &AmlValueRef,
    ret_type: u8,
    _argc: i32,
    argv: Option<&[AmlValue]>,
) -> Option<AmlValueRef> {
    let tmp = my_ret.clone();
    let mut my_ret = Some(tmp.clone());

    match tmp.r#type() {
        AML_OBJTYPE_NAMEREF => {
            let name = aml_getname(tmp.v_nameref().unwrap_or(AML_NULL).tail());
            my_ret = Some(aml_seterror(
                scope,
                format_args!("Undefined name: {}", Str(&name)),
            ));
        }
        AML_OBJTYPE_METHOD => {
            let m = tmp.v_method().unwrap_or_default();
            let ms = aml_pushscope(scope, &tmp, tmp.node(), AMLOP_METHOD)?;

            // Parse method arguments
            for idx in 0..aml_method_argcount(m.flags) {
                let Some(sp) = aml_getstack(&ms, AMLOP_ARG0 + idx) else {
                    continue;
                };
                match argv {
                    Some(argv) => {
                        if let Some(a) = argv.get(idx as usize) {
                            aml_copyvalue(&sp, a);
                        }
                    }
                    None => {
                        _aml_setvalue(
                            &sp,
                            AML_OBJTYPE_OBJREF,
                            i64::from(AMLOP_ARG0 + idx),
                            Bval::Null,
                        );
                        let r = aml_parse(scope, b't', "ARGX");
                        sp.with_obj(|o| {
                            if let AmlObj::ObjRef(o) = o {
                                o.r#ref = r;
                            }
                        });
                    }
                }
            }

            // Evaluate method scope
            aml_root().start.set(m.base);
            if let Some(fneval) = m.fneval {
                my_ret = fneval(&ms, None);
            } else {
                aml_parse(Some(&ms), b'T', "METHEVAL");
                my_ret = ms.retv.borrow_mut().take();
            }
            aml_popscope(ms);
        }
        AML_OBJTYPE_BUFFERFIELD | AML_OBJTYPE_FIELDUNIT => {
            let r = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
            let bitlen = tmp.v_field().map_or(0, |f| f.bitlen);
            aml_rwfield(&tmp, 0, bitlen, &r, ACPI_IOREAD);
            my_ret = Some(r);
        }
        _ => {}
    }
    if ret_type == b'i'
        && let Some(r) = &my_ret
        && r.r#type() != AML_OBJTYPE_INTEGER
    {
        aml_showvalue(Some(r));
        aml_die!("aml_eval", "Not Integer");
    }
    my_ret
}

/// `aml_parsesimple(scope, ch, rv)`: an immediate operand (`Revision`, `Debug`, the byte,
/// word, dword and qword constants, a string).
pub fn aml_parsesimple(scope: &AmlScope, ch: u8, rv: Option<AmlValueRef>) -> AmlValueRef {
    let rv = rv.unwrap_or_else(|| aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null));
    let pos = scope.cur();
    match ch {
        AML_ARG_REVISION => _aml_setvalue(&rv, AML_OBJTYPE_INTEGER, AML_REVISION, Bval::Null),
        AML_ARG_DEBUG => _aml_setvalue(&rv, AML_OBJTYPE_DEBUGOBJ, 0, Bval::Null),
        AML_ARG_BYTE => {
            _aml_setvalue(&rv, AML_OBJTYPE_INTEGER, i64::from(pos.get8()), Bval::Null);
            scope.advance(1);
        }
        AML_ARG_WORD => {
            _aml_setvalue(&rv, AML_OBJTYPE_INTEGER, i64::from(pos.get16()), Bval::Null);
            scope.advance(2);
        }
        AML_ARG_DWORD => {
            _aml_setvalue(&rv, AML_OBJTYPE_INTEGER, i64::from(pos.get32()), Bval::Null);
            scope.advance(4);
        }
        AML_ARG_QWORD => {
            _aml_setvalue(&rv, AML_OBJTYPE_INTEGER, pos.get64() as i64, Bval::Null);
            scope.advance(8);
        }
        AML_ARG_STRING => {
            _aml_setvalue(&rv, AML_OBJTYPE_STRING, -1, Bval::Bytes(pos.tail()));
            scope.advance(rv.length() as usize + 1);
        }
        _ => {}
    }
    rv
}

/// `aml_gettgt(val, opcode)`: the object a chain of references ends at (a reference to a
/// package element is kept for `Store`, which handles it).
pub fn aml_gettgt(val: Option<&AmlValueRef>, opcode: i32) -> Option<AmlValueRef> {
    let mut val = val.cloned();
    while let Some(v) = &val {
        let Some(o) = v.v_objref() else { break };
        // Stores into package elements need to be handled differently than other stores.
        // Return the corresponding object reference such that our caller can still
        // recognize them as such.
        if opcode == AMLOP_STORE && o.r#type == AMLOP_PACKAGE {
            return val;
        }
        val = o.r#ref;
    }
    val
}

/// `aml_seterror(scope, fmt, ...)`: reports a parse error, ends every scope up to the
/// table's, counts the error in [`AML_ERROR`], and yields an integer 0.
pub fn aml_seterror(scope: Option<&Rc<AmlScope>>, args: fmt::Arguments<'_>) -> AmlValueRef {
    let pc = scope.map_or(0, |s| aml_pc(s.cur()));
    kprintf!("### AML PARSE ERROR (0x{:x}): {}\n", pc, args);

    let mut s = scope.cloned();
    while let Some(sc) = s {
        sc.pos.set(Some(sc.end));
        s = sc.parent.clone();
    }
    AML_ERROR.fetch_add(1, Ordering::Relaxed);
    aml_allocvalue(AML_OBJTYPE_INTEGER, 0, Bval::Null)
}

/// `strncmp(a, b, n) == 0` over byte strings that may end before `n` (at a NUL or at their
/// end).
fn strneq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let (ca, cb) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
    }
    true
}

/// The byte code of a table `acpi.c` loaded: the bytes after its header.
fn table_aml(q: &AcpiQ) -> Option<(AcpiTableHeader, &'static [u8])> {
    let hdr = q.q_table.cast_const().cast::<AcpiTableHeader>();
    if hdr.is_null() {
        return None;
    }
    // SAFETY: `q_table` points at a table `acpi_maptable` mapped (or copied) for good; its
    // header is read unaligned, as the structure is packed.
    let h = unsafe { ptr::read_unaligned(hdr) };
    let hlen = core::mem::size_of::<AcpiTableHeader>();
    let len = (h.length as usize).saturating_sub(hlen);
    // SAFETY: as above; the table is `h.length` bytes long and never unmapped, so the bytes
    // after the header live as long as the kernel.
    let aml = unsafe { core::slice::from_raw_parts(hdr.cast::<u8>().add(hlen), len) };
    Some((h, aml))
}

/// `aml_loadtable(sc, signature, oemid, oemtableid, rootpath, parameterpath,
/// parameterdata)`: `LoadTable`: parses the loaded table with those ids into the namespace
/// (at `rootpath`); a DDB handle, or the integer 0 when there is no such table.
pub fn aml_loadtable(
    sc: Option<&AcpiSoftc>,
    signature: &[u8],
    oemid: &[u8],
    oemtableid: &[u8],
    rootpath: &[u8],
    parameterpath: &[u8],
    _parameterdata: &AmlValueRef,
) -> AmlValueRef {
    if !cstr(parameterpath).is_empty() {
        aml_die!(
            "aml_loadtable",
            "LoadTable: ParameterPathString unsupported"
        );
    }

    if let Some(sc) = sc {
        for entry in sc.sc_tables.iter() {
            let Some((hdr, aml)) = table_aml(entry) else {
                continue;
            };
            if strneq(&hdr.signature, signature, hdr.signature.len())
                && strneq(&hdr.oemid, oemid, hdr.oemid.len())
                && strneq(&hdr.oemtableid, oemtableid, hdr.oemtableid.len())
            {
                acpi_parse_aml(Some(sc), Some(rootpath), aml);
                return aml_allocvalue(AML_OBJTYPE_DDBHANDLE, 0, Bval::Null);
            }
        }
    }

    aml_allocvalue(AML_OBJTYPE_INTEGER, 0, Bval::Null)
}

/// `aml_load(sc, scope, rgn, ddb)`: `Load`: maps the SSDT at the start of system-memory
/// region `rgn`, stores its handle in `ddb`, and returns a scope running it.
pub fn aml_load(
    sc: Option<&AcpiSoftc>,
    scope: &Rc<AmlScope>,
    rgn: &AmlValueRef,
    ddb: &AmlValueRef,
) -> Option<Rc<AmlScope>> {
    ddb.set_obj(AmlObj::DdbHandle(0));

    let xname = sc.map_or("acpi0", |sc| sc.sc_dev.xname());
    let loaded = 'load: {
        let Some(r) = rgn.v_opregion() else {
            break 'load None;
        };
        if i32::from(r.iospace) != GAS_SYSTEM_MEMORY {
            break 'load None;
        }
        let Some(sc) = sc else { break 'load None };

        // Load SSDT from memory
        let Some(entry) = acpi_maptable(sc, r.iobase as usize, Some(b"SSDT"), None, None, 1) else {
            break 'load None;
        };
        ddb.set_obj(AmlObj::DdbHandle(i64::from(entry.q_id)));

        let Some((_, aml)) = table_aml(entry) else {
            break 'load None;
        };
        let tmp = AmlValue::from_obj(AmlObj::Scope(AmlPtr::new(aml), aml.len() as i32));
        Some(aml_pushscope(
            Some(scope),
            &tmp,
            scope.node.clone(),
            AMLOP_LOAD,
        ))
    };
    match loaded {
        Some(s) => s,
        None => {
            kprintf!(
                "{}: unable to load {}\n",
                xname,
                Str(&aml_nodename(rgn.node().as_ref()))
            );
            None
        }
    }
}

/// The argument strings of `aml_parse` that differ only in their letter.
const IFELSE_ELSE: &[u8] = b"-TbpT";
/// `IF` without an `Else`.
const IFELSE_IF: &[u8] = b"-T";

/// `aml_parse(scope, ret_type, stype)`: the parser and evaluator. Decodes and runs one term
/// at `scope->pos` and returns its value as `ret_type` asks:
///
/// - `'o'`: a data object (integer, string, buffer, package, name);
/// - `'i'`: an integer;
/// - `'t'`: a term argument (integer, string, buffer, package);
/// - `'r'`: a target (a named object, a local, an argument, or none);
/// - `'S'`: a super name (a named object, a local, an argument);
/// - `'T'`: a term list: runs terms to the end of `scope`, entering and leaving the scopes
///   of the control and named-object terms in this frame, and returns nothing.
pub fn aml_parse(scope: Option<&Rc<AmlScope>>, ret_type: u8, stype: &str) -> Option<AmlValueRef> {
    let mut scope = scope?.clone();
    if scope.pos.get().is_none() || scope.pos_ge_end() {
        return None;
    }
    if ODP.fetch_add(1, Ordering::Relaxed) > 125 {
        panic(format_args!("depth"));
    }
    MAXDP.fetch_max(ODP.load(Ordering::Relaxed), Ordering::Relaxed);
    let _ = stype;

    let mut end: Option<AmlPtr> = None;
    let iscope = scope.clone();
    let mut my_ret: Option<AmlValueRef>;
    loop {
        // --== Stage 0: Get Opcode ==--
        let start = scope.cur();
        let pc = aml_pc(start);

        let opcode = aml_parseopcode(&scope);
        let Some(htab) = aml_findopcode(opcode) else {
            // No opcode handler
            aml_die!("aml_parse", "Unknown opcode: {:04x} @ {:04x}", opcode, pc);
        };

        // --== Stage 1: Process opcode arguments ==--
        let mut opargs: [Option<AmlValueRef>; 8] = Default::default();
        let mut idx = 0;
        let mut args: &[u8] = htab.args;
        let mut ci = 0;
        let mut parse_error = false;
        my_ret = None;
        while ci < args.len() {
            let ch = args[ci];
            let mut rv: Option<AmlValueRef> = None;
            match ch {
                AML_ARG_OBJLEN => end = Some(aml_parseend(&scope)),
                AML_ARG_IFELSE => {
                    // Special Case: IF-ELSE:piTbpT or IF:piT
                    let e = end.unwrap_or(scope.end);
                    args = if i32::from(e.get8()) == AMLOP_ELSE && e < scope.end {
                        IFELSE_ELSE
                    } else {
                        IFELSE_IF
                    };
                    ci = 0;
                }

                // Complex arguments
                b's' | b'S' | AML_ARG_TARGET | AML_ARG_TERMOBJ | AML_ARG_INTEGER => {
                    if ch == AML_ARG_TARGET && i32::from(scope.cur().get8()) == AMLOP_ZERO {
                        // Special case: NULL Target
                        rv = Some(aml_allocvalue(AML_OBJTYPE_NOTARGET, 0, Bval::Null));
                        scope.advance(1);
                    } else {
                        rv = aml_parse(Some(&scope), ch, htab.mnem);
                        if rv.is_none() || AML_ERROR.load(Ordering::Relaxed) != 0 {
                            parse_error = true;
                            break;
                        }
                    }
                }

                // Simple arguments
                AML_ARG_BUFFER | AML_ARG_METHOD | AML_ARG_FIELDLIST | AML_ARG_TERMOBJLIST => {
                    let p = scope.cur();
                    let e = end.unwrap_or(p);
                    rv = Some(Rc::new(AmlValue::from_obj(AmlObj::Scope(
                        p,
                        e.diff(p) as i32,
                    ))));
                    scope.pos.set(Some(e));
                }
                AML_ARG_CONST => {
                    // `(char)opcode`: Ones is -1.
                    rv = Some(aml_allocvalue(
                        AML_OBJTYPE_INTEGER,
                        i64::from(opcode as u8 as i8),
                        Bval::Null,
                    ));
                }
                AML_ARG_CREATENAME | AML_ARG_SEARCHNAME => {
                    let (p, v) =
                        aml_parsename(scope.node.as_ref(), scope.cur(), ch == AML_ARG_CREATENAME);
                    scope.pos.set(Some(p));
                    rv = Some(v);
                }
                AML_ARG_BYTE | AML_ARG_WORD | AML_ARG_DWORD | AML_ARG_QWORD | AML_ARG_DEBUG
                | AML_ARG_STRING | AML_ARG_REVISION => rv = Some(aml_parsesimple(&scope, ch, None)),
                AML_ARG_STKLOCAL | AML_ARG_STKARG => rv = aml_getstack(&scope, opcode),
                _ => aml_die!("aml_parse", "Unknown arg type: {}\n", char::from(ch)),
            }
            if let Some(rv) = rv
                && idx < opargs.len()
            {
                opargs[idx] = Some(rv);
                idx += 1;
            }
            ci += 1;
        }

        if !parse_error {
            // --== Stage 2: Process opcode ==--
            let mut ival: i64 = 0;
            let mut mscope: Option<Rc<AmlScope>> = None;
            let a = |i: usize| -> AmlValueRef {
                opargs[i]
                    .clone()
                    .unwrap_or_else(|| Rc::new(AmlValue::new()))
            };
            let int = |i: usize| a(i).v_integer();
            let is_term = matches!(ret_type, b't' | b'i' | b'T');

            match opcode {
                AMLOP_NOP | AMLOP_BREAKPOINT => {}
                AMLOP_LOCAL0..=AMLOP_LOCAL7 | AMLOP_ARG0..=AMLOP_ARG6 => {
                    my_ret = opargs[0].clone();
                }
                AMLOP_NAMECHAR => {
                    // opargs[0] = named object (node != NULL), or nameref
                    let mut r = a(0);
                    if scope.r#type == AMLOP_PACKAGE && r.node().is_some() {
                        // Special case for package
                        r = aml_allocvalue(
                            AML_OBJTYPE_OBJREF,
                            i64::from(AMLOP_NAMECHAR),
                            Bval::Ref(a(0)),
                        );
                    } else if let Some(o) = r.v_objref() {
                        r = o.r#ref.unwrap_or(r);
                    }
                    my_ret = if matches!(ret_type, b'i' | b't' | b'T') {
                        // Return TermArg or Integer: Evaluate object
                        aml_eval(Some(&scope), &r, ret_type, 0, None)
                    } else {
                        // A non-termarg method: this should only happen with CondRef
                        Some(r)
                    };
                }

                AMLOP_ZERO | AMLOP_ONE | AMLOP_ONES | AMLOP_DEBUG | AMLOP_REVISION
                | AMLOP_BYTEPREFIX | AMLOP_WORDPREFIX | AMLOP_DWORDPREFIX | AMLOP_QWORDPREFIX
                | AMLOP_STRINGPREFIX => my_ret = opargs[0].clone(),

                AMLOP_BUFFER => {
                    // Buffer: iB => Buffer
                    my_ret = Some(aml_allocvalue(
                        AML_OBJTYPE_BUFFER,
                        int(0),
                        Bval::Bytes(&a(1).v_buffer()),
                    ));
                }
                AMLOP_PACKAGE | AMLOP_VARPACKAGE => {
                    // Package/VarPackage: bT/iT => Package
                    let pkg = aml_allocvalue(AML_OBJTYPE_PACKAGE, int(0), Bval::Null);
                    let ms = aml_pushscope(Some(&scope), &a(1), scope.node.clone(), AMLOP_PACKAGE);

                    // Recursively parse package contents
                    for i in 0..pkg.length() as usize {
                        if let Some(rv) = aml_parse(ms.as_ref(), b'o', "Package") {
                            pkg.with_obj(|o| {
                                if let AmlObj::Package(p) = o
                                    && let Some(slot) = p.get_mut(i)
                                {
                                    *slot = rv;
                                }
                            });
                        }
                    }
                    if let Some(ms) = ms {
                        aml_popscope(ms);
                    }
                    my_ret = Some(pkg);
                }

                // Math/Logical operations
                AMLOP_OR | AMLOP_ADD | AMLOP_AND | AMLOP_NAND | AMLOP_XOR | AMLOP_SHL
                | AMLOP_SHR | AMLOP_NOR | AMLOP_MOD | AMLOP_SUBTRACT | AMLOP_MULTIPLY => {
                    // XXX: iir => I
                    ival = aml_evalexpr(int(0) as u64, int(1) as u64, opcode) as i64;
                    aml_store(&scope, opargs[2].as_ref(), ival, None);
                }
                AMLOP_DIVIDE => {
                    // Divide: iirr => I
                    if int(1) == 0 {
                        my_ret = Some(aml_seterror(Some(&scope), format_args!("Divide by Zero!")));
                    } else {
                        let rem = aml_evalexpr(int(0) as u64, int(1) as u64, AMLOP_MOD) as i64;
                        ival = aml_evalexpr(int(0) as u64, int(1) as u64, AMLOP_DIVIDE) as i64;
                        aml_store(&scope, opargs[2].as_ref(), rem, None);
                        aml_store(&scope, opargs[3].as_ref(), ival, None);
                    }
                }
                AMLOP_NOT
                | AMLOP_TOBCD
                | AMLOP_FROMBCD
                | AMLOP_FINDSETLEFTBIT
                | AMLOP_FINDSETRIGHTBIT => {
                    // XXX: ir => I
                    ival = aml_evalexpr(int(0) as u64, 0, opcode) as i64;
                    aml_store(&scope, opargs[1].as_ref(), ival, None);
                }
                AMLOP_INCREMENT | AMLOP_DECREMENT => {
                    // Inc/Dec: S => I
                    let r = aml_eval(Some(&scope), &a(0), AML_ARG_INTEGER, 0, None);
                    let cur = r.as_ref().map_or(0, |r| r.v_integer());
                    ival = aml_evalexpr(cur as u64, 1, opcode) as i64;
                    aml_store(&scope, opargs[0].as_ref(), ival, None);
                    my_ret = r;
                }
                AMLOP_LNOT => {
                    // LNot: i => Bool
                    ival = aml_evalexpr(int(0) as u64, 0, opcode) as i64;
                }
                AMLOP_LOR | AMLOP_LAND => {
                    // XXX: ii => Bool
                    ival = aml_evalexpr(int(0) as u64, int(1) as u64, opcode) as i64;
                }
                AMLOP_LLESS | AMLOP_LEQUAL | AMLOP_LGREATER | AMLOP_LNOTEQUAL
                | AMLOP_LLESSEQUAL | AMLOP_LGREATEREQUAL => {
                    // XXX: tt => Bool
                    ival = i64::from(aml_compare(&a(0), &a(1), opcode));
                }

                // Reference/Store operations
                AMLOP_CONDREFOF => {
                    // CondRef: rr => I
                    ival = 0;
                    if a(0).node().is_some() {
                        // Create Object Reference
                        let rv =
                            aml_allocvalue(AML_OBJTYPE_OBJREF, i64::from(opcode), Bval::Ref(a(0)));
                        aml_store(&scope, opargs[1].as_ref(), 0, Some(&rv));

                        // Mark that we found it
                        ival = -1;
                    }
                }
                AMLOP_REFOF => {
                    // RefOf: r => ObjRef
                    my_ret = Some(aml_allocvalue(
                        AML_OBJTYPE_OBJREF,
                        i64::from(opcode),
                        Bval::Ref(a(0)),
                    ));
                }
                AMLOP_INDEX => 'index: {
                    // Index: tir => ObjRef
                    let a0 = a(0);
                    let i = int(1) as i32;
                    // Reading past the end of the array? - Ignore
                    if i >= a0.length() || i < 0 {
                        break 'index;
                    }
                    match a0.r#type() {
                        AML_OBJTYPE_PACKAGE => {
                            let e = a0.v_package(i as usize);
                            if is_term {
                                my_ret = e;
                            } else {
                                let r = aml_allocvalue(
                                    AML_OBJTYPE_OBJREF,
                                    i64::from(AMLOP_PACKAGE),
                                    e.map_or(Bval::Null, Bval::Ref),
                                );
                                r.with_obj(|o| {
                                    if let AmlObj::ObjRef(o) = o {
                                        o.index = i;
                                    }
                                });
                                my_ret = Some(r);
                            }
                        }
                        AML_OBJTYPE_BUFFER | AML_OBJTYPE_STRING | AML_OBJTYPE_INTEGER => {
                            let rv = aml_convert(&a0, AML_OBJTYPE_BUFFER, -1);
                            if is_term {
                                ival =
                                    i64::from(rv.v_buffer().get(i as usize).copied().unwrap_or(0));
                            } else {
                                let r = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
                                aml_createfield(
                                    &r,
                                    AMLOP_INDEX,
                                    &rv,
                                    8 * i,
                                    8,
                                    None,
                                    0,
                                    AML_FIELD_BYTEACC,
                                );
                                my_ret = Some(r);
                            }
                        }
                        t => aml_die!("aml_parse", "Unknown index : {:x}\n", t),
                    }
                    aml_store(&scope, opargs[2].as_ref(), ival, my_ret.as_ref());
                }
                AMLOP_DEREFOF => {
                    // DerefOf: t:ObjRef => DataRefObj
                    my_ret = match a(0).v_objref() {
                        Some(o) => o.r#ref,
                        None => opargs[0].clone(),
                    };
                }
                AMLOP_COPYOBJECT => {
                    // CopyObject: t:DataRefObj, s:implename => DataRefObj
                    my_ret = opargs[0].clone();
                    aml_freevalue(opargs[1].as_deref());
                    aml_copyvalue(&a(1), &a(0));
                }
                AMLOP_STORE => {
                    // Store: t:DataRefObj, S:upername => DataRefObj
                    my_ret = opargs[0].clone();
                    aml_store(&scope, opargs[1].as_ref(), 0, opargs[0].as_ref());
                }

                // Conversion
                AMLOP_TOINTEGER | AMLOP_TOBUFFER | AMLOP_TOHEXSTRING | AMLOP_TODECSTRING => {
                    // Source:CData, Result => Integer, Buffer or String
                    let ctype = match opcode {
                        AMLOP_TOINTEGER => AML_OBJTYPE_INTEGER,
                        AMLOP_TOBUFFER => AML_OBJTYPE_BUFFER,
                        AMLOP_TOHEXSTRING => AML_OBJTYPE_HEXSTRING,
                        _ => AML_OBJTYPE_DECSTRING,
                    };
                    let r = aml_convert(&a(0), ctype, -1);
                    aml_store(&scope, opargs[1].as_ref(), 0, Some(&r));
                    my_ret = Some(r);
                }
                AMLOP_TOSTRING => {
                    // Source:B, Length:I, Result => String
                    let r = aml_convert(&a(0), AML_OBJTYPE_STRING, int(1) as i32);
                    aml_store(&scope, opargs[2].as_ref(), 0, Some(&r));
                    my_ret = Some(r);
                }
                AMLOP_CONCAT => {
                    // Source1:CData, Source2:CData, Result => CData
                    let r = aml_concat(&a(0), &a(1));
                    aml_store(&scope, opargs[2].as_ref(), 0, Some(&r));
                    my_ret = Some(r);
                }
                AMLOP_CONCATRES => {
                    // Concat two resource buffers: buf1, buf2, result => Buffer
                    let r = aml_concatres(&a(0), &a(1));
                    aml_store(&scope, opargs[2].as_ref(), 0, Some(&r));
                    my_ret = Some(r);
                }
                AMLOP_MID => {
                    // Source:BS, Index:I, Length:I, Result => BS
                    let r = aml_mid(&a(0), int(1) as i32, int(2) as i32);
                    aml_store(&scope, opargs[3].as_ref(), 0, Some(&r));
                    my_ret = Some(r);
                }
                AMLOP_MATCH => {
                    // Match: Pkg, Op1, Val1, Op2, Val2, Index
                    ival = i64::from(aml_match(
                        &a(0),
                        int(5) as i32,
                        int(1) as i32,
                        int(2) as i32,
                        int(3) as i32,
                        int(4) as i32,
                    ));
                }
                AMLOP_SIZEOF => {
                    // Sizeof: S => i
                    ival =
                        aml_gettgt(opargs[0].as_ref(), opcode).map_or(0, |r| i64::from(r.length()));
                }
                AMLOP_OBJECTTYPE => {
                    // ObjectType: S => i
                    ival =
                        aml_gettgt(opargs[0].as_ref(), opcode).map_or(0, |r| i64::from(r.r#type()));
                }

                // Mutex/Event handlers
                AMLOP_ACQUIRE => {
                    // Acquire: Sw => Bool
                    if let Some(rv) = aml_gettgt(opargs[0].as_ref(), opcode) {
                        ival = i64::from(acpi_mutex_acquire(&scope, &rv, int(1) as i32));
                    }
                }
                AMLOP_RELEASE => {
                    // Release: S
                    if let Some(rv) = aml_gettgt(opargs[0].as_ref(), opcode) {
                        acpi_mutex_release(&scope, &rv);
                    }
                }
                AMLOP_WAIT => {
                    // Wait: Si => Bool
                    if let Some(rv) = aml_gettgt(opargs[0].as_ref(), opcode) {
                        ival = i64::from(acpi_event_wait(&scope, &rv, int(1) as i32));
                    }
                }
                AMLOP_RESET => {
                    // Reset: S
                    if let Some(rv) = aml_gettgt(opargs[0].as_ref(), opcode) {
                        acpi_event_reset(&scope, &rv);
                    }
                }
                AMLOP_SIGNAL => {
                    // Signal: S
                    if let Some(rv) = aml_gettgt(opargs[0].as_ref(), opcode) {
                        acpi_event_signal(&scope, &rv);
                    }
                }

                // Named objects
                AMLOP_NAME => {
                    // Name: Nt
                    let rv = a(0);
                    aml_freevalue(Some(&rv));
                    aml_copyvalue(&rv, &a(1));
                }
                AMLOP_ALIAS => {
                    // Alias: nN
                    let rv = a(1);
                    _aml_setvalue(&rv, AML_OBJTYPE_OBJREF, i64::from(opcode), Bval::Null);
                    let tgt = aml_gettgt(opargs[0].as_ref(), opcode);
                    rv.with_obj(|o| {
                        if let AmlObj::ObjRef(o) = o {
                            o.r#ref = tgt;
                        }
                    });
                }
                AMLOP_OPREGION => {
                    // OpRegion: Nbii
                    a(0).set_obj(AmlObj::OpRegion(AmlOpregion {
                        iospace: int(1) as u8,
                        iobase: int(2) as u64,
                        iolen: int(3) as u32,
                        flag: 0,
                    }));
                }
                AMLOP_DATAREGION => {
                    // DataTableRegion: N,t:SigStr,t:OemIDStr,t:OemTableIDStr
                    a(0).set_obj(AmlObj::OpRegion(AmlOpregion {
                        iospace: GAS_SYSTEM_MEMORY as u8,
                        ..AmlOpregion::default()
                    }));
                    aml_die!("aml_parse", "AML-DataTableRegion\n");
                }
                AMLOP_EVENT => {
                    // Event: N
                    a(0).set_obj(AmlObj::Event(0));
                }
                AMLOP_MUTEX => {
                    // Mutex: Nw
                    a(0).set_obj(AmlObj::Mutex(AmlMtx {
                        synclvl: int(1) as i32,
                        ..AmlMtx::default()
                    }));
                }
                AMLOP_SCOPE => {
                    // Scope: NT
                    let rv = a(0);
                    if let Some(p) = rv.v_nameref() {
                        kprintf!("Undefined scope: {}\n", Str(&aml_getname(p.tail())));
                    } else {
                        mscope = aml_pushscope(Some(&scope), &a(1), rv.node(), opcode);
                    }
                }
                AMLOP_DEVICE | AMLOP_THERMALZONE => {
                    // Device: NT, ThermalZone: NT
                    let rv = a(0);
                    rv.set_obj(if opcode == AMLOP_DEVICE {
                        AmlObj::Device
                    } else {
                        AmlObj::ThermZone
                    });
                    mscope = aml_pushscope(Some(&scope), &a(1), rv.node(), opcode);
                }
                AMLOP_POWERRSRC => {
                    // PowerRsrc: NbwT
                    let rv = a(0);
                    rv.set_obj(AmlObj::PowerRsrc(AmlPowerrsrc {
                        pwr_level: int(1) as u8,
                        pwr_order: int(2) as u16,
                    }));
                    mscope = aml_pushscope(Some(&scope), &a(3), rv.node(), opcode);
                }
                AMLOP_PROCESSOR => {
                    // Processor: NbdbT
                    let rv = a(0);
                    rv.set_obj(AmlObj::Processor(AmlProcessor {
                        proc_id: int(1) as u8,
                        proc_addr: int(2) as u32,
                        proc_len: int(3) as u8,
                    }));
                    mscope = aml_pushscope(Some(&scope), &a(4), rv.node(), opcode);
                }
                AMLOP_METHOD => {
                    // Method: NbM
                    let (p, len) = match &*a(2).obj() {
                        AmlObj::Scope(p, len) => (*p, *len),
                        _ => (AML_NULL, 0),
                    };
                    a(0).set_obj(AmlObj::Method(AmlMethod {
                        flags: int(1) as i32,
                        start: Some(p),
                        end: Some(p.add(len.max(0) as usize)),
                        fneval: None,
                        base: aml_root().start.get(),
                    }));
                }

                // Field objects
                AMLOP_CREATEFIELD => {
                    // Source:B, BitIndex:I, NumBits:I, FieldName
                    let rv = a(3);
                    _aml_setvalue(&rv, AML_OBJTYPE_BUFFERFIELD, 0, Bval::Null);
                    aml_createfield(&rv, opcode, &a(0), int(1) as i32, int(2) as i32, None, 0, 0);
                }
                AMLOP_CREATEBITFIELD
                | AMLOP_CREATEBYTEFIELD
                | AMLOP_CREATEWORDFIELD
                | AMLOP_CREATEDWORDFIELD
                | AMLOP_CREATEQWORDFIELD => {
                    // Source:B, BitIndex:I or ByteIndex:I, FieldName
                    let (bpos, blen, flags) = match opcode {
                        AMLOP_CREATEBITFIELD => (int(1) as i32, 1, 0),
                        AMLOP_CREATEBYTEFIELD => (int(1) as i32 * 8, 8, AML_FIELD_BYTEACC),
                        AMLOP_CREATEWORDFIELD => (int(1) as i32 * 8, 16, AML_FIELD_WORDACC),
                        AMLOP_CREATEDWORDFIELD => (int(1) as i32 * 8, 32, AML_FIELD_DWORDACC),
                        _ => (int(1) as i32 * 8, 64, AML_FIELD_QWORDACC),
                    };
                    let rv = a(2);
                    _aml_setvalue(&rv, AML_OBJTYPE_BUFFERFIELD, 0, Bval::Null);
                    aml_createfield(&rv, opcode, &a(0), bpos, blen, None, 0, flags);
                }
                AMLOP_FIELD => {
                    // Field: n:OpRegion, b:Flags, F:ieldlist
                    let ms = aml_pushscope(Some(&scope), &a(2), scope.node.clone(), opcode);
                    aml_parsefieldlist(ms, opcode, int(1) as i32, &a(0), None, 0);
                }
                AMLOP_INDEXFIELD => {
                    // IndexField: n:Index, n:Data, b:Flags, F:ieldlist
                    let ms = aml_pushscope(Some(&scope), &a(3), scope.node.clone(), opcode);
                    aml_parsefieldlist(ms, opcode, int(2) as i32, &a(1), Some(&a(0)), 0);
                }
                AMLOP_BANKFIELD => {
                    // BankField: n:OpRegion, n:Field, i:Bank, b:Flags, F:ieldlist
                    let ms = aml_pushscope(Some(&scope), &a(4), scope.node.clone(), opcode);
                    aml_parsefieldlist(
                        ms,
                        opcode,
                        int(3) as i32,
                        &a(0),
                        Some(&a(1)),
                        int(2) as i32,
                    );
                }

                // Misc functions
                AMLOP_STALL => acpi_stall(int(0) as i32),
                AMLOP_SLEEP => acpi_sleep(int(0) as i32, "amlsleep"),
                AMLOP_NOTIFY => {
                    // Notify: Si
                    let rv = aml_gettgt(opargs[0].as_ref(), opcode);
                    aml_notify(rv.and_then(|r| r.node()).as_ref(), int(1) as i32);
                }
                AMLOP_TIMER => {
                    // Timer: => i
                    let ts = nanouptime();
                    ival = ts
                        .tv_sec
                        .wrapping_mul(10_000_000)
                        .wrapping_add(ts.tv_nsec / 100);
                }
                AMLOP_FATAL => {
                    // Fatal: bdi
                    aml_die!(
                        "aml_parse",
                        "AML FATAL ERROR: {:x},{:x},{:x}\n",
                        int(0),
                        int(1),
                        int(2)
                    );
                }
                AMLOP_LOADTABLE => {
                    // LoadTable(Sig:Str, OEMID:Str, OEMTable:Str, [RootPath:Str],
                    // [ParmPath:Str], [ParmData:DataRefObj]) => DDBHandle
                    my_ret = Some(aml_loadtable(
                        acpi_softc(),
                        &a(0).v_string(),
                        &a(1).v_string(),
                        &a(2).v_string(),
                        &a(3).v_string(),
                        &a(4).v_string(),
                        &a(5),
                    ));
                }
                AMLOP_LOAD => {
                    // Load(Object:NameString, DDBHandle:SuperName)
                    mscope = aml_load(acpi_softc(), &scope, &a(0), &a(1));
                }
                AMLOP_UNLOAD => {
                    // DDBHandle
                    aml_die!("aml_parse", "Unload");
                }

                // Control Flow
                AMLOP_IF => {
                    // Arguments: iT or iTbT
                    if int(0) != 0 {
                        mscope = aml_pushscope(Some(&scope), &a(1), scope.node.clone(), AMLOP_IF);
                    } else if let Some(e) = &opargs[3] {
                        mscope = aml_pushscope(Some(&scope), e, scope.node.clone(), AMLOP_ELSE);
                    }
                }
                AMLOP_WHILE => {
                    if int(0) != 0 {
                        // Set parent position to start of WHILE
                        scope.pos.set(Some(start));
                        mscope =
                            aml_pushscope(Some(&scope), &a(1), scope.node.clone(), AMLOP_WHILE);
                    }
                }
                AMLOP_BREAK => {
                    // Break: Find While Scope parent, mark type as null
                    aml_findscope(&scope, AMLOP_WHILE, AMLOP_BREAK);
                }
                AMLOP_CONTINUE => {
                    // Find Scope.. mark all objects as invalid on way to root
                    aml_findscope(&scope, AMLOP_WHILE, AMLOP_CONTINUE);
                }
                AMLOP_RETURN => {
                    if let Some(ms) = aml_findscope(&scope, AMLOP_METHOD, AMLOP_RETURN) {
                        if ms.retv.borrow().is_some() {
                            aml_die!("aml_parse", "already allocated\n");
                        }
                        let r = aml_allocvalue(AML_OBJTYPE_UNINITIALIZED, 0, Bval::Null);
                        aml_copyvalue(&r, &a(0));
                        *ms.retv.borrow_mut() = Some(r);
                    }
                }
                _ => {
                    // may be set direct result
                    aml_die!("aml_parse", "Unknown opcode: {:x}:{}\n", opcode, htab.mnem);
                }
            }
            if let Some(ms) = mscope {
                // Change our scope to new scope
                scope = ms;
            }
            if (ret_type == b'i' || ret_type == b't') && my_ret.is_none() {
                my_ret = Some(aml_allocvalue(AML_OBJTYPE_INTEGER, ival, Bval::Null));
            }
            if ret_type == b'i'
                && let Some(r) = &my_ret
                && r.r#type() != AML_OBJTYPE_INTEGER
            {
                my_ret = Some(aml_convert(r, AML_OBJTYPE_INTEGER, -1));
            }
        }

        // End opcode: display/free arguments (parse_error:)
        drop(opargs);

        // If parsing whole scope and not done, start again
        if ret_type == b'T' {
            my_ret = None;
            while scope.pos_ge_end() && !Rc::ptr_eq(&scope, &iscope) {
                // Pop intermediate scope
                match aml_popscope(scope.clone()) {
                    Some(p) => scope = p,
                    None => break,
                }
            }
            if let Some(p) = scope.pos.get()
                && p < scope.end
            {
                continue;
            }
        }
        break;
    }

    ODP.fetch_sub(1, Ordering::Relaxed);
    my_ret
}

/// `acpi_parse_aml(sc, rootpath, start, length)`: runs the top level of a definition block
/// (the DSDT's or an SSDT's byte code, `aml`), building the namespace; 0, or -1 when the
/// parser reported errors.
pub fn acpi_parse_aml(sc: Option<&AcpiSoftc>, rootpath: Option<&[u8]>, aml: &'static [u8]) -> i32 {
    let _ = sc;
    let root = aml_root();
    if let Some(rootpath) = rootpath
        && aml_searchname(Some(&root), rootpath).is_none()
    {
        aml_die!("acpi_parse_aml", "Invalid RootPathName {}\n", Str(rootpath));
    }

    root.start.set(Some(AmlPtr::new(aml)));
    let res = AmlValue::from_obj(AmlObj::Scope(AmlPtr::new(aml), aml.len() as i32));

    // Push toplevel scope, parse AML
    AML_ERROR.store(0, Ordering::Relaxed);
    let scope = aml_pushscope(None, &res, Some(root), AMLOP_SCOPE);
    AML_BUSY.fetch_add(1, Ordering::Relaxed);
    aml_parse(scope.as_ref(), b'T', "TopLevel");
    AML_BUSY.fetch_sub(1, Ordering::Relaxed);
    if let Some(scope) = scope {
        aml_popscope(scope);
    }

    if AML_ERROR.load(Ordering::Relaxed) != 0 {
        kprintf!("error in acpi_parse_aml\n");
        return -1;
    }
    0
}

/// `aml_evalnode(sc, node, argc, argv, res)`: evaluates `node` (runs a method with the
/// arguments `argv`) and copies the result into `res`, which the caller owns. 0,
/// `ACPI_E_BADVALUE` when there is no node or value, -1 when the interpreter reported an
/// error.
pub fn aml_evalnode(
    _sc: Option<&AcpiSoftc>,
    node: Option<&AmlNodeRef>,
    argv: &[AmlValue],
    res: Option<&AmlValue>,
) -> i32 {
    if let Some(res) = res {
        res.set_obj(AmlObj::Uninitialized);
        res.set_node(None);
        res.stack.set(0);
    }
    let Some(node) = node else {
        return ACPI_E_BADVALUE;
    };
    let Some(value) = node.value() else {
        return ACPI_E_BADVALUE;
    };

    AML_ERROR.store(0, Ordering::Relaxed);
    let argv = if argv.is_empty() { None } else { Some(argv) };
    let xres = aml_eval(None, &value, b't', argv.map_or(0, |a| a.len() as i32), argv);
    if let (Some(xres), Some(res)) = (xres, res) {
        aml_copyvalue(res, &xres);
    }
    if AML_ERROR.load(Ordering::Relaxed) != 0 {
        kprintf!("error evaluating: {}\n", Str(&aml_nodename(Some(node))));
        return -1;
    }
    0
}

/// `aml_node_setval(sc, node, val)`: runs the method `node` with the one argument `val`.
pub fn aml_node_setval(sc: Option<&AcpiSoftc>, node: Option<&AmlNodeRef>, val: i64) -> i32 {
    if node.is_none() {
        return 0;
    }
    let env = [AmlValue::integer(val)];
    aml_evalnode(sc, node, &env, None)
}

/// `aml_evalname(sc, parent, name, argc, argv, res)`: [`aml_evalnode`] of `name` below
/// `parent`.
pub fn aml_evalname(
    sc: Option<&AcpiSoftc>,
    parent: Option<&AmlNodeRef>,
    name: &[u8],
    argv: &[AmlValue],
    res: Option<&AmlValue>,
) -> i32 {
    let node = aml_searchname(parent, name);
    aml_evalnode(sc, node.as_ref(), argv, res)
}

/// `aml_evalinteger(sc, parent, name, argc, argv, &ival)`: [`aml_evalname`] read as an
/// integer into `ival` (on success only).
pub fn aml_evalinteger(
    sc: Option<&AcpiSoftc>,
    parent: Option<&AmlNodeRef>,
    name: &[u8],
    argv: &[AmlValue],
    ival: &mut i64,
) -> i32 {
    let res = AmlValue::new();
    let node = aml_searchname(parent, name);
    let rc = aml_evalnode(sc, node.as_ref(), argv, Some(&res));
    if rc == 0 {
        *ival = aml_val2int(Some(&res));
        aml_freevalue(Some(&res));
    }
    rc
}

/// `__aml_searchname(root, vname, create)`: the node of the dotted name `vname` (short
/// segments padded with `_`) below `root`, or from `\` for an absolute name.
pub fn __aml_searchname(
    root: Option<&AmlNodeRef>,
    vname: &[u8],
    create: bool,
) -> Option<AmlNodeRef> {
    let name = cstr(vname);
    let mut root = root.cloned();
    let mut i = 0;
    while name.get(i).is_some_and(|&c| i32::from(c) == AMLOP_ROOTCHAR) {
        root = Some(aml_root());
        i += 1;
    }
    while i < name.len() {
        // Ugh.. we can have short names here: append '_'
        let mut nseg = *b"____";
        let mut k = 0;
        while k < AML_NAMESEG_LEN && i < name.len() && name[i] != b'.' {
            nseg[k] = name[i];
            k += 1;
            i += 1;
        }
        if name.get(i) == Some(&b'.') {
            i += 1;
        }
        root = __aml_search(root.as_ref(), &nseg, create);
    }
    root
}

/// `aml_searchname(root, vname)`: [`__aml_searchname`] without creating.
pub fn aml_searchname(root: Option<&AmlNodeRef>, vname: &[u8]) -> Option<AmlNodeRef> {
    __aml_searchname(root, vname, false)
}

/// `aml_searchrel(root, vname)`: `vname` below `root` or below the nearest ancestor that has
/// it.
pub fn aml_searchrel(root: Option<&AmlNodeRef>, vname: &[u8]) -> Option<AmlNodeRef> {
    let mut root = root.cloned();
    while let Some(r) = root {
        if let Some(res) = aml_searchname(Some(&r), vname) {
            return Some(res);
        }
        root = r.parent();
    }
    None
}

/// `acpi_getdevlist(list, root, pkg, off)`: appends to `list` the devices the elements of
/// `pkg` from `off` name (names resolved relative to `root`).
pub fn acpi_getdevlist(
    list: &mut AcpiDevlistHead,
    root: Option<&AmlNodeRef>,
    pkg: &AmlValue,
    off: i32,
) {
    for idx in off.max(0)..pkg.length() {
        let Some(mut val) = pkg.v_package(idx as usize) else {
            continue;
        };
        if let Some(p) = val.v_nameref() {
            let name = aml_getname(p.tail());
            let Some(node) = aml_searchrel(root, &name) else {
                kprintf!("acpi_getdevlist: device {} not found\n", Str(&name));
                continue;
            };
            let Some(v) = node.value() else { continue };
            val = v;
        }
        if let Some(o) = val.v_objref()
            && let Some(r) = o.r#ref
        {
            val = r;
        }
        if let Some(node) = val.node() {
            list.push(AcpiDevlist { dev_node: node });
        }
    }
}

/// `acpi_freedevlist(list)`.
pub fn acpi_freedevlist(list: &mut AcpiDevlistHead) {
    list.clear();
}

const _: () = {
    assert!(AML_TABLE.len() < u8::MAX as usize);
    assert!(opsize(AMLOP_MUTEX) == 2 && opsize(AMLOP_STORE) == 1);
    assert!(AML_FIELD_ATTRIB == AML_FIELD_ATTR__);
};

#[cfg(test)]
mod tests;
