/* <LICENSES> */
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

/* <CODE> */
//! `<dev/acpi/amltypes.h>`: the AML opcodes and object types, and the two structures the AML
//! interpreter (`dev/acpi/dsdt.rs`) is built on: the value (`struct aml_value`) and the
//! namespace node (`struct aml_node`).
//!
//! Upstream: sys/dev/acpi/amltypes.h @ 3ce1f3f79392
//!
//! In C a value is a `type` tag, a `length`, a reference count and a union read through the
//! `v_*` macros; nodes form a tree of `SIMPLEQ`s with `parent` pointers, and a value is freed
//! by `aml_delref` when its count drops to zero. Here a value is an [`AmlValue`] whose tag and
//! union are one enum ([`AmlObj`]) behind a `RefCell` (the C changes a value's type in place,
//! through any of the pointers that share it), shared as an [`AmlValueRef`] (`Rc`): the `Rc`
//! count is the C's `refcnt`. Nodes are [`AmlNodeRef`]s (`Rc`) owned by their parent's child
//! list, with `Weak` links back to the parent and from a value to its node, so a node the
//! interpreter deletes (`aml_delchildren`) reads as NULL through the values that outlive it,
//! which is what the C arranges by hand. Why `Rc` and not `Arc` is in `dev/acpi/dsdt.rs`
//! (`AmlGlobal`): every path to the namespace runs under the kernel lock.
//!
//! ## Deviations
//! - `struct aml_value`'s `type`, `length` and union are [`AmlObj`]; `length` is derived from
//!   the variant (the bytes of a string or buffer, the elements of a package, `aml_intlen / 8`
//!   for an integer, the stored length of an AML range), not a member `_aml_setvalue` leaves
//!   stale when it changes the type to one without a length.
//! - `refcnt` is the `Rc` strong count; `aml_addref`/`aml_delref` are `clone`/`drop`.
//! - `v_mutex` (`struct acpi_mutex *`) shares its storage with `v_mtx` in C and nothing in the
//!   tree ever stores a `struct acpi_mutex` there; a mutex value is [`AmlMtx`] only.
//! - `struct aml_waitq`, `aml_waitq_head` and the `waiters` members of `v_mtx`/`v_evt` are
//!   declared but used by no file in the tree: left out.
//! - `struct aml_node`'s `son`/`sib` `SIMPLEQ` is a `Vec` of children in insertion order
//!   ([`AmlNode::sons`]). `i2c` (`struct i2c_controller *`) is left out until `dev/i2c` is
//!   ported, so `aml_rwgsb` never finds a controller and answers `EIO`, as the C does then.
//! - `char name[5]` keeps its NUL; [`AmlNode::name`] gives the bytes before it.
//! - The `uint8_t *` pointers into AML byte code (`v_method.start`, `v_nameref`, a node's
//!   `start`) are [`AmlPtr`]s: the table's bytes and an offset.
//! - `struct acpi_gpio`'s function pointers are plain `fn`s (every GPIO driver fills all five);
//!   `struct acpi_pci`'s members that `acpi.c` fills after allocation are `Cell`s.

use alloc::rc::{Rc, Weak};
use alloc::vec::Vec;
use core::cell::{Cell, Ref, RefCell};
use core::ffi::c_void;
use core::fmt;

use super::dsdt::{AmlScope, aml_intlen};
use crate::sys::device::Device;
use crate::sys::queue::TailqEntry;

/// `AMLOP_ZERO`.
pub const AMLOP_ZERO: i32 = 0x00;
/// `AMLOP_ONE`.
pub const AMLOP_ONE: i32 = 0x01;
/// `AMLOP_ALIAS`.
pub const AMLOP_ALIAS: i32 = 0x06;
/// `AMLOP_NAME`.
pub const AMLOP_NAME: i32 = 0x08;
/// `AMLOP_BYTEPREFIX`.
pub const AMLOP_BYTEPREFIX: i32 = 0x0A;
/// `AMLOP_WORDPREFIX`.
pub const AMLOP_WORDPREFIX: i32 = 0x0B;
/// `AMLOP_DWORDPREFIX`.
pub const AMLOP_DWORDPREFIX: i32 = 0x0C;
/// `AMLOP_STRINGPREFIX`.
pub const AMLOP_STRINGPREFIX: i32 = 0x0D;
/// `AMLOP_QWORDPREFIX`.
pub const AMLOP_QWORDPREFIX: i32 = 0x0E;
/// `AMLOP_SCOPE`.
pub const AMLOP_SCOPE: i32 = 0x10;
/// `AMLOP_BUFFER`.
pub const AMLOP_BUFFER: i32 = 0x11;
/// `AMLOP_PACKAGE`.
pub const AMLOP_PACKAGE: i32 = 0x12;
/// `AMLOP_VARPACKAGE`.
pub const AMLOP_VARPACKAGE: i32 = 0x13;
/// `AMLOP_METHOD`.
pub const AMLOP_METHOD: i32 = 0x14;
/// `AMLOP_DUALNAMEPREFIX`.
pub const AMLOP_DUALNAMEPREFIX: i32 = 0x2E;
/// `AMLOP_MULTINAMEPREFIX`.
pub const AMLOP_MULTINAMEPREFIX: i32 = 0x2F;
/// `AMLOP_EXTPREFIX`.
pub const AMLOP_EXTPREFIX: i32 = 0x5B;
/// `AMLOP_MUTEX`.
pub const AMLOP_MUTEX: i32 = 0x5B01;
/// `AMLOP_EVENT`.
pub const AMLOP_EVENT: i32 = 0x5B02;
/// `AMLOP_CONDREFOF`.
pub const AMLOP_CONDREFOF: i32 = 0x5B12;
/// `AMLOP_CREATEFIELD`.
pub const AMLOP_CREATEFIELD: i32 = 0x5B13;
/// `AMLOP_LOADTABLE`.
pub const AMLOP_LOADTABLE: i32 = 0x5B1F;
/// `AMLOP_LOAD`.
pub const AMLOP_LOAD: i32 = 0x5B20;
/// `AMLOP_STALL`.
pub const AMLOP_STALL: i32 = 0x5B21;
/// `AMLOP_SLEEP`.
pub const AMLOP_SLEEP: i32 = 0x5B22;
/// `AMLOP_ACQUIRE`.
pub const AMLOP_ACQUIRE: i32 = 0x5B23;
/// `AMLOP_SIGNAL`.
pub const AMLOP_SIGNAL: i32 = 0x5B24;
/// `AMLOP_WAIT`.
pub const AMLOP_WAIT: i32 = 0x5B25;
/// `AMLOP_RESET`.
pub const AMLOP_RESET: i32 = 0x5B26;
/// `AMLOP_RELEASE`.
pub const AMLOP_RELEASE: i32 = 0x5B27;
/// `AMLOP_FROMBCD`.
pub const AMLOP_FROMBCD: i32 = 0x5B28;
/// `AMLOP_TOBCD`.
pub const AMLOP_TOBCD: i32 = 0x5B29;
/// `AMLOP_UNLOAD`.
pub const AMLOP_UNLOAD: i32 = 0x5B2A;
/// `AMLOP_REVISION`.
pub const AMLOP_REVISION: i32 = 0x5B30;
/// `AMLOP_DEBUG`.
pub const AMLOP_DEBUG: i32 = 0x5B31;
/// `AMLOP_FATAL`.
pub const AMLOP_FATAL: i32 = 0x5B32;
/// `AMLOP_TIMER`.
pub const AMLOP_TIMER: i32 = 0x5B33;
/// `AMLOP_OPREGION`.
pub const AMLOP_OPREGION: i32 = 0x5B80;
/// `AMLOP_FIELD`.
pub const AMLOP_FIELD: i32 = 0x5B81;
/// `AMLOP_DEVICE`.
pub const AMLOP_DEVICE: i32 = 0x5B82;
/// `AMLOP_PROCESSOR`.
pub const AMLOP_PROCESSOR: i32 = 0x5B83;
/// `AMLOP_POWERRSRC`.
pub const AMLOP_POWERRSRC: i32 = 0x5B84;
/// `AMLOP_THERMALZONE`.
pub const AMLOP_THERMALZONE: i32 = 0x5B85;
/// `AMLOP_INDEXFIELD`.
pub const AMLOP_INDEXFIELD: i32 = 0x5B86;
/// `AMLOP_BANKFIELD`.
pub const AMLOP_BANKFIELD: i32 = 0x5B87;
/// `AMLOP_DATAREGION`.
pub const AMLOP_DATAREGION: i32 = 0x5B88;
/// `AMLOP_ROOTCHAR`.
pub const AMLOP_ROOTCHAR: i32 = 0x5C;
/// `AMLOP_PARENTPREFIX`.
pub const AMLOP_PARENTPREFIX: i32 = 0x5E;
/// `AMLOP_NAMECHAR`.
pub const AMLOP_NAMECHAR: i32 = 0x5F;
/// `AMLOP_LOCAL0`.
pub const AMLOP_LOCAL0: i32 = 0x60;
/// `AMLOP_LOCAL1`.
pub const AMLOP_LOCAL1: i32 = 0x61;
/// `AMLOP_LOCAL2`.
pub const AMLOP_LOCAL2: i32 = 0x62;
/// `AMLOP_LOCAL3`.
pub const AMLOP_LOCAL3: i32 = 0x63;
/// `AMLOP_LOCAL4`.
pub const AMLOP_LOCAL4: i32 = 0x64;
/// `AMLOP_LOCAL5`.
pub const AMLOP_LOCAL5: i32 = 0x65;
/// `AMLOP_LOCAL6`.
pub const AMLOP_LOCAL6: i32 = 0x66;
/// `AMLOP_LOCAL7`.
pub const AMLOP_LOCAL7: i32 = 0x67;
/// `AMLOP_ARG0`.
pub const AMLOP_ARG0: i32 = 0x68;
/// `AMLOP_ARG1`.
pub const AMLOP_ARG1: i32 = 0x69;
/// `AMLOP_ARG2`.
pub const AMLOP_ARG2: i32 = 0x6A;
/// `AMLOP_ARG3`.
pub const AMLOP_ARG3: i32 = 0x6B;
/// `AMLOP_ARG4`.
pub const AMLOP_ARG4: i32 = 0x6C;
/// `AMLOP_ARG5`.
pub const AMLOP_ARG5: i32 = 0x6D;
/// `AMLOP_ARG6`.
pub const AMLOP_ARG6: i32 = 0x6E;
/// `AMLOP_STORE`.
pub const AMLOP_STORE: i32 = 0x70;
/// `AMLOP_REFOF`.
pub const AMLOP_REFOF: i32 = 0x71;
/// `AMLOP_ADD`.
pub const AMLOP_ADD: i32 = 0x72;
/// `AMLOP_CONCAT`.
pub const AMLOP_CONCAT: i32 = 0x73;
/// `AMLOP_SUBTRACT`.
pub const AMLOP_SUBTRACT: i32 = 0x74;
/// `AMLOP_INCREMENT`.
pub const AMLOP_INCREMENT: i32 = 0x75;
/// `AMLOP_DECREMENT`.
pub const AMLOP_DECREMENT: i32 = 0x76;
/// `AMLOP_MULTIPLY`.
pub const AMLOP_MULTIPLY: i32 = 0x77;
/// `AMLOP_DIVIDE`.
pub const AMLOP_DIVIDE: i32 = 0x78;
/// `AMLOP_SHL`.
pub const AMLOP_SHL: i32 = 0x79;
/// `AMLOP_SHR`.
pub const AMLOP_SHR: i32 = 0x7A;
/// `AMLOP_AND`.
pub const AMLOP_AND: i32 = 0x7B;
/// `AMLOP_NAND`.
pub const AMLOP_NAND: i32 = 0x7C;
/// `AMLOP_OR`.
pub const AMLOP_OR: i32 = 0x7D;
/// `AMLOP_NOR`.
pub const AMLOP_NOR: i32 = 0x7E;
/// `AMLOP_XOR`.
pub const AMLOP_XOR: i32 = 0x7F;
/// `AMLOP_NOT`.
pub const AMLOP_NOT: i32 = 0x80;
/// `AMLOP_FINDSETLEFTBIT`.
pub const AMLOP_FINDSETLEFTBIT: i32 = 0x81;
/// `AMLOP_FINDSETRIGHTBIT`.
pub const AMLOP_FINDSETRIGHTBIT: i32 = 0x82;
/// `AMLOP_DEREFOF`.
pub const AMLOP_DEREFOF: i32 = 0x83;
/// `AMLOP_CONCATRES`.
pub const AMLOP_CONCATRES: i32 = 0x84;
/// `AMLOP_MOD`.
pub const AMLOP_MOD: i32 = 0x85;
/// `AMLOP_NOTIFY`.
pub const AMLOP_NOTIFY: i32 = 0x86;
/// `AMLOP_SIZEOF`.
pub const AMLOP_SIZEOF: i32 = 0x87;
/// `AMLOP_INDEX`.
pub const AMLOP_INDEX: i32 = 0x88;
/// `AMLOP_MATCH`.
pub const AMLOP_MATCH: i32 = 0x89;
/// `AMLOP_CREATEDWORDFIELD`.
pub const AMLOP_CREATEDWORDFIELD: i32 = 0x8A;
/// `AMLOP_CREATEWORDFIELD`.
pub const AMLOP_CREATEWORDFIELD: i32 = 0x8B;
/// `AMLOP_CREATEBYTEFIELD`.
pub const AMLOP_CREATEBYTEFIELD: i32 = 0x8C;
/// `AMLOP_CREATEBITFIELD`.
pub const AMLOP_CREATEBITFIELD: i32 = 0x8D;
/// `AMLOP_OBJECTTYPE`.
pub const AMLOP_OBJECTTYPE: i32 = 0x8E;
/// `AMLOP_CREATEQWORDFIELD`.
pub const AMLOP_CREATEQWORDFIELD: i32 = 0x8F;
/// `AMLOP_LAND`.
pub const AMLOP_LAND: i32 = 0x90;
/// `AMLOP_LOR`.
pub const AMLOP_LOR: i32 = 0x91;
/// `AMLOP_LNOT`.
pub const AMLOP_LNOT: i32 = 0x92;
/// `AMLOP_LNOTEQUAL`.
pub const AMLOP_LNOTEQUAL: i32 = 0x9293;
/// `AMLOP_LLESSEQUAL`.
pub const AMLOP_LLESSEQUAL: i32 = 0x9294;
/// `AMLOP_LGREATEREQUAL`.
pub const AMLOP_LGREATEREQUAL: i32 = 0x9295;
/// `AMLOP_LEQUAL`.
pub const AMLOP_LEQUAL: i32 = 0x93;
/// `AMLOP_LGREATER`.
pub const AMLOP_LGREATER: i32 = 0x94;
/// `AMLOP_LLESS`.
pub const AMLOP_LLESS: i32 = 0x95;
/// `AMLOP_TOBUFFER`.
pub const AMLOP_TOBUFFER: i32 = 0x96;
/// `AMLOP_TODECSTRING`.
pub const AMLOP_TODECSTRING: i32 = 0x97;
/// `AMLOP_TOHEXSTRING`.
pub const AMLOP_TOHEXSTRING: i32 = 0x98;
/// `AMLOP_TOINTEGER`.
pub const AMLOP_TOINTEGER: i32 = 0x99;
/// `AMLOP_TOSTRING`.
pub const AMLOP_TOSTRING: i32 = 0x9C;
/// `AMLOP_COPYOBJECT`.
pub const AMLOP_COPYOBJECT: i32 = 0x9D;
/// `AMLOP_MID`.
pub const AMLOP_MID: i32 = 0x9E;
/// `AMLOP_CONTINUE`.
pub const AMLOP_CONTINUE: i32 = 0x9F;
/// `AMLOP_IF`.
pub const AMLOP_IF: i32 = 0xA0;
/// `AMLOP_ELSE`.
pub const AMLOP_ELSE: i32 = 0xA1;
/// `AMLOP_WHILE`.
pub const AMLOP_WHILE: i32 = 0xA2;
/// `AMLOP_NOP`.
pub const AMLOP_NOP: i32 = 0xA3;
/// `AMLOP_RETURN`.
pub const AMLOP_RETURN: i32 = 0xA4;
/// `AMLOP_BREAK`.
pub const AMLOP_BREAK: i32 = 0xA5;
/// `AMLOP_BREAKPOINT`.
pub const AMLOP_BREAKPOINT: i32 = 0xCC;
/// `AMLOP_ONES`.
pub const AMLOP_ONES: i32 = 0xFF;

/// `AMLOP_INVALID`: what `aml_parseopcode` returns at the end of a scope.
pub const AMLOP_INVALID: i32 = -1;

/// `AML_MATCH_TR`: the `Match()` comparison that is always true.
pub const AML_MATCH_TR: i32 = 0;
/// `AML_MATCH_EQ`: `==`.
pub const AML_MATCH_EQ: i32 = 1;
/// `AML_MATCH_LE`: `<=`.
pub const AML_MATCH_LE: i32 = 2;
/// `AML_MATCH_LT`: `<`.
pub const AML_MATCH_LT: i32 = 3;
/// `AML_MATCH_GE`: `>=`.
pub const AML_MATCH_GE: i32 = 4;
/// `AML_MATCH_GT`: `>`.
pub const AML_MATCH_GT: i32 = 5;

/// `enum aml_objecttype`: `AML_OBJTYPE_UNINITIALIZED`. The values up to `0x10` are what
/// `ObjectType()` answers; those from `0x100` are the interpreter's own.
pub const AML_OBJTYPE_UNINITIALIZED: i32 = 0;
/// `AML_OBJTYPE_INTEGER`.
pub const AML_OBJTYPE_INTEGER: i32 = 1;
/// `AML_OBJTYPE_STRING`.
pub const AML_OBJTYPE_STRING: i32 = 2;
/// `AML_OBJTYPE_BUFFER`.
pub const AML_OBJTYPE_BUFFER: i32 = 3;
/// `AML_OBJTYPE_PACKAGE`.
pub const AML_OBJTYPE_PACKAGE: i32 = 4;
/// `AML_OBJTYPE_FIELDUNIT`.
pub const AML_OBJTYPE_FIELDUNIT: i32 = 5;
/// `AML_OBJTYPE_DEVICE`.
pub const AML_OBJTYPE_DEVICE: i32 = 6;
/// `AML_OBJTYPE_EVENT`.
pub const AML_OBJTYPE_EVENT: i32 = 7;
/// `AML_OBJTYPE_METHOD`.
pub const AML_OBJTYPE_METHOD: i32 = 8;
/// `AML_OBJTYPE_MUTEX`.
pub const AML_OBJTYPE_MUTEX: i32 = 9;
/// `AML_OBJTYPE_OPREGION`.
pub const AML_OBJTYPE_OPREGION: i32 = 10;
/// `AML_OBJTYPE_POWERRSRC`.
pub const AML_OBJTYPE_POWERRSRC: i32 = 11;
/// `AML_OBJTYPE_PROCESSOR`.
pub const AML_OBJTYPE_PROCESSOR: i32 = 12;
/// `AML_OBJTYPE_THERMZONE`.
pub const AML_OBJTYPE_THERMZONE: i32 = 13;
/// `AML_OBJTYPE_BUFFERFIELD`.
pub const AML_OBJTYPE_BUFFERFIELD: i32 = 14;
/// `AML_OBJTYPE_DDBHANDLE`.
pub const AML_OBJTYPE_DDBHANDLE: i32 = 15;
/// `AML_OBJTYPE_DEBUGOBJ`.
pub const AML_OBJTYPE_DEBUGOBJ: i32 = 16;
/// `AML_OBJTYPE_NAMEREF`: a name not (yet) found in the namespace.
pub const AML_OBJTYPE_NAMEREF: i32 = 0x100;
/// `AML_OBJTYPE_OBJREF`: a reference (`RefOf`, `Index`, an alias, a method argument).
pub const AML_OBJTYPE_OBJREF: i32 = 0x101;
/// `AML_OBJTYPE_SCOPE`: a range of AML byte code.
pub const AML_OBJTYPE_SCOPE: i32 = 0x102;
/// `AML_OBJTYPE_NOTARGET`: the NULL target of an operator.
pub const AML_OBJTYPE_NOTARGET: i32 = 0x103;
/// `AML_OBJTYPE_HEXSTRING`: a conversion target only (`ToHexString`).
pub const AML_OBJTYPE_HEXSTRING: i32 = 0x104;
/// `AML_OBJTYPE_DECSTRING`: a conversion target only (`ToDecString`).
pub const AML_OBJTYPE_DECSTRING: i32 = 0x105;

/// `AML_ARG_INTEGER`: an argument letter of `aml_table[]` (`'i'`).
pub const AML_ARG_INTEGER: u8 = b'i';
/// `AML_ARG_BYTE`.
pub const AML_ARG_BYTE: u8 = b'b';
/// `AML_ARG_WORD`.
pub const AML_ARG_WORD: u8 = b'w';
/// `AML_ARG_DWORD`.
pub const AML_ARG_DWORD: u8 = b'd';
/// `AML_ARG_QWORD`.
pub const AML_ARG_QWORD: u8 = b'q';
/// `AML_ARG_IMPBYTE`.
pub const AML_ARG_IMPBYTE: u8 = b'!';
/// `AML_ARG_OBJLEN`.
pub const AML_ARG_OBJLEN: u8 = b'p';
/// `AML_ARG_STRING`.
pub const AML_ARG_STRING: u8 = b'a';
/// `AML_ARG_BYTELIST`.
pub const AML_ARG_BYTELIST: u8 = b'B';
/// `AML_ARG_REVISION`.
pub const AML_ARG_REVISION: u8 = b'R';
/// `AML_ARG_METHOD`.
pub const AML_ARG_METHOD: u8 = b'M';
/// `AML_ARG_NAMESTRING`.
pub const AML_ARG_NAMESTRING: u8 = b'N';
/// `AML_ARG_NAMEREF`.
pub const AML_ARG_NAMEREF: u8 = b'n';
/// `AML_ARG_FIELDLIST`.
pub const AML_ARG_FIELDLIST: u8 = b'F';
/// `AML_ARG_FLAG`.
pub const AML_ARG_FLAG: u8 = b'f';
/// `AML_ARG_DATAOBJLIST`.
pub const AML_ARG_DATAOBJLIST: u8 = b'O';
/// `AML_ARG_DATAOBJ`.
pub const AML_ARG_DATAOBJ: u8 = b'o';
/// `AML_ARG_SIMPLENAME`.
pub const AML_ARG_SIMPLENAME: u8 = b's';
/// `AML_ARG_SUPERNAME`.
pub const AML_ARG_SUPERNAME: u8 = b'S';
/// `AML_ARG_TERMOBJLIST`.
pub const AML_ARG_TERMOBJLIST: u8 = b'T';
/// `AML_ARG_TERMOBJ`.
pub const AML_ARG_TERMOBJ: u8 = b't';
/// `AML_ARG_IFELSE`.
pub const AML_ARG_IFELSE: u8 = b'I';
/// `AML_ARG_BUFFER`.
pub const AML_ARG_BUFFER: u8 = b'B';
/// `AML_ARG_SEARCHNAME`.
pub const AML_ARG_SEARCHNAME: u8 = b'n';
/// `AML_ARG_CREATENAME`.
pub const AML_ARG_CREATENAME: u8 = b'N';
/// `AML_ARG_STKARG`.
pub const AML_ARG_STKARG: u8 = b'A';
/// `AML_ARG_STKLOCAL`.
pub const AML_ARG_STKLOCAL: u8 = b'L';
/// `AML_ARG_DEBUG`.
pub const AML_ARG_DEBUG: u8 = b'D';
/// `AML_ARG_CONST`.
pub const AML_ARG_CONST: u8 = b'c';
/// `AML_ARG_TARGET`.
pub const AML_ARG_TARGET: u8 = b'r';

/// `AML_METHOD_ARGCOUNT(v)`: the argument count in a method's flags.
pub const fn aml_method_argcount(v: i32) -> i32 {
    v & 0x7
}

/// `AML_METHOD_SERIALIZED(v)`.
pub const fn aml_method_serialized(v: i32) -> i32 {
    (v >> 3) & 0x1
}

/// `AML_METHOD_SYNCLEVEL(v)`.
pub const fn aml_method_synclevel(v: i32) -> i32 {
    (v >> 4) & 0xF
}

/// `AML_FIELD_ACCESSMASK`.
pub const AML_FIELD_ACCESSMASK: i32 = 0x0F;

/// `AML_FIELD_SETATTR(f, t, a)`.
pub const fn aml_field_setattr(f: i32, t: i32, a: i32) -> i32 {
    (f & 0xF0) | (t & 0xF) | (a << 8)
}

/// `AML_FIELD_ACCESS(v)`: the access type in a field's flags.
pub const fn aml_field_access(v: i32) -> i32 {
    v & 0xF
}

/// `AML_FIELD_ANYACC`.
pub const AML_FIELD_ANYACC: i32 = 0x0;
/// `AML_FIELD_BYTEACC`.
pub const AML_FIELD_BYTEACC: i32 = 0x1;
/// `AML_FIELD_WORDACC`.
pub const AML_FIELD_WORDACC: i32 = 0x2;
/// `AML_FIELD_DWORDACC`.
pub const AML_FIELD_DWORDACC: i32 = 0x3;
/// `AML_FIELD_QWORDACC`.
pub const AML_FIELD_QWORDACC: i32 = 0x4;
/// `AML_FIELD_BUFFERACC`.
pub const AML_FIELD_BUFFERACC: i32 = 0x5;

/// `AML_FIELD_LOCK(v)`: the lock rule in a field's flags.
pub const fn aml_field_lock(v: i32) -> i32 {
    (v >> 4) & 0x1
}

/// `AML_FIELD_LOCK_OFF`.
pub const AML_FIELD_LOCK_OFF: i32 = 0x0;
/// `AML_FIELD_LOCK_ON`.
pub const AML_FIELD_LOCK_ON: i32 = 0x1;

/// `AML_FIELD_UPDATE(v)`: the update rule in a field's flags.
pub const fn aml_field_update(v: i32) -> i32 {
    (v >> 5) & 0x3
}

/// `AML_FIELD_PRESERVE`.
pub const AML_FIELD_PRESERVE: i32 = 0x0;
/// `AML_FIELD_WRITEASONES`.
pub const AML_FIELD_WRITEASONES: i32 = 0x1;
/// `AML_FIELD_WRITEASZEROES`.
pub const AML_FIELD_WRITEASZEROES: i32 = 0x2;

/// `AML_FIELD_ATTR(v)`: the access attribute in a field's flags.
pub const fn aml_field_attr(v: i32) -> i32 {
    v >> 8
}

/// `AML_FIELD_RESERVED`.
pub const AML_FIELD_RESERVED: i32 = 0x00;
/// `AML_FIELD_ATTR__` (the C's own "XXX fix this name").
pub const AML_FIELD_ATTR__: i32 = 0x01;

/// A `uint8_t *` into AML byte code: the bytes of the table (or of the buffer the
/// interpreter loaded) and an offset into them.
///
/// The C walks raw pointers and may read a byte or two past a scope's end; here a read past
/// the end of the bytes yields 0, so a malformed table cannot make the interpreter read
/// outside them. Two pointers compare by address, as C pointers do.
#[derive(Clone, Copy)]
pub struct AmlPtr {
    blob: &'static [u8],
    off: usize,
}

impl AmlPtr {
    /// A pointer to the first byte of `blob`.
    pub const fn new(blob: &'static [u8]) -> Self {
        Self { blob, off: 0 }
    }

    /// `p + n`.
    #[must_use]
    pub const fn add(self, n: usize) -> Self {
        Self {
            blob: self.blob,
            off: self.off + n,
        }
    }

    /// `p[i]`: the byte `i` past the pointer, or 0 past the end of the bytes.
    pub fn at(self, i: usize) -> u8 {
        self.blob.get(self.off + i).copied().unwrap_or(0)
    }

    /// `aml_get8(p)`.
    pub fn get8(self) -> u8 {
        self.at(0)
    }

    /// `aml_get16(p)`: little-endian and unaligned (both machines are little-endian).
    pub fn get16(self) -> u16 {
        u16::from_le_bytes([self.at(0), self.at(1)])
    }

    /// `aml_get32(p)`.
    pub fn get32(self) -> u32 {
        u32::from_le_bytes([self.at(0), self.at(1), self.at(2), self.at(3)])
    }

    /// `aml_get64(p)`.
    pub fn get64(self) -> u64 {
        let mut b = [0u8; 8];
        for (i, x) in b.iter_mut().enumerate() {
            *x = self.at(i);
        }
        u64::from_le_bytes(b)
    }

    /// The address the pointer stands for (for comparisons and `aml_pc`).
    pub fn addr(self) -> usize {
        self.blob.as_ptr() as usize + self.off
    }

    /// `p - q` in bytes.
    pub fn diff(self, q: AmlPtr) -> isize {
        self.addr().wrapping_sub(q.addr()) as isize
    }

    /// The bytes from the pointer to the end of the table (empty past the end).
    pub fn tail(self) -> &'static [u8] {
        self.blob.get(self.off..).unwrap_or(&[])
    }

    /// The `len` bytes at the pointer, cut at the end of the table.
    pub fn bytes(self, len: usize) -> &'static [u8] {
        let t = self.tail();
        &t[..len.min(t.len())]
    }
}

impl PartialEq for AmlPtr {
    fn eq(&self, other: &Self) -> bool {
        self.addr() == other.addr()
    }
}

impl Eq for AmlPtr {}

impl PartialOrd for AmlPtr {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AmlPtr {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.addr().cmp(&other.addr())
    }
}

impl fmt::Debug for AmlPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AmlPtr({:#x})", self.addr())
    }
}

/// `struct aml_value *`: a shared, reference-counted value.
pub type AmlValueRef = Rc<AmlValue>;
/// `struct aml_node *`: a namespace node.
pub type AmlNodeRef = Rc<AmlNode>;

/// `v_method.fneval`: a method the kernel implements (`_OSI`), called with the method's
/// scope instead of parsing byte code.
pub type AmlFneval = fn(&Rc<AmlScope>, Option<&AmlValueRef>) -> Option<AmlValueRef>;

/// `v_field`: a field unit (`Field`, `IndexField`, `BankField`) or a buffer field
/// (`CreateXxxField`, `Index` into a buffer).
#[derive(Clone, Default)]
pub struct AmlField {
    /// `type`: the opcode that made it (`AMLOP_FIELD`, `AMLOP_INDEX`, ...; `uint16_t`).
    pub r#type: i32,
    /// `flags`: the access flags (`AML_FIELD_*`; `uint16_t`).
    pub flags: i32,
    /// `bitpos` (`uint32_t`; every use is `int` arithmetic).
    pub bitpos: i32,
    /// `bitlen`.
    pub bitlen: i32,
    /// `ref1`: the region, the buffer, or the index field's data field.
    pub ref1: Option<AmlValueRef>,
    /// `ref2`: the index field, the bank field, or a connection buffer.
    pub ref2: Option<AmlValueRef>,
    /// `ref3`: the bank value or the access length.
    pub ref3: i32,
}

/// `v_opregion`.
#[derive(Clone, Copy, Default)]
pub struct AmlOpregion {
    /// `iospace`: the address space (`ACPI_OPREG_*`).
    pub iospace: u8,
    /// `iobase`.
    pub iobase: u64,
    /// `iolen`.
    pub iolen: u32,
    /// `flag`.
    pub flag: i32,
}

/// `v_method`.
#[derive(Clone, Copy, Default)]
pub struct AmlMethod {
    /// `flags`: argument count, serialisation and sync level.
    pub flags: i32,
    /// `start`: the first byte of the method's body.
    pub start: Option<AmlPtr>,
    /// `end`: one past its last byte.
    pub end: Option<AmlPtr>,
    /// `fneval`: the kernel's implementation of a built-in method.
    pub fneval: Option<AmlFneval>,
    /// `base`: the start of the table that defined it (for `aml_pc`).
    pub base: Option<AmlPtr>,
}

/// `v_processor`.
#[derive(Clone, Copy, Default)]
pub struct AmlProcessor {
    /// `proc_id`.
    pub proc_id: u8,
    /// `proc_addr`.
    pub proc_addr: u32,
    /// `proc_len`.
    pub proc_len: u8,
}

/// `v_objref`.
#[derive(Clone, Default)]
pub struct AmlObjref {
    /// `type`: the opcode that made the reference (`AMLOP_REFOF`, `AMLOP_INDEX`,
    /// `AMLOP_PACKAGE`, `AMLOP_ALIAS`, `AMLOP_ARG0`.., `AMLOP_NAMECHAR`, ...).
    pub r#type: i32,
    /// `index`: the element of a package reference.
    pub index: i32,
    /// `ref`: the value referred to.
    pub r#ref: Option<AmlValueRef>,
}

/// `v_powerrsrc`.
#[derive(Clone, Copy, Default)]
pub struct AmlPowerrsrc {
    /// `pwr_level`.
    pub pwr_level: u8,
    /// `pwr_order`.
    pub pwr_order: u16,
}

/// `v_mtx` (`Vmutex`): an AML `Mutex`.
#[derive(Clone, Copy)]
pub struct AmlMtx {
    /// `synclvl`.
    pub synclvl: i32,
    /// `savelvl`.
    pub savelvl: i32,
    /// `count`.
    pub count: i32,
    /// `ownername`.
    pub ownername: [u8; 5],
    /// `owner`: the scope holding it; compared by address, never dereferenced.
    pub owner: *const AmlScope,
}

impl Default for AmlMtx {
    fn default() -> Self {
        Self {
            synclvl: 0,
            savelvl: 0,
            count: 0,
            ownername: [0; 5],
            owner: core::ptr::null(),
        }
    }
}

/// The `type` of an `aml_value` and the union member it selects.
#[derive(Clone, Default)]
pub enum AmlObj {
    /// `AML_OBJTYPE_UNINITIALIZED`.
    #[default]
    Uninitialized,
    /// `AML_OBJTYPE_INTEGER`: `v_integer`.
    Integer(i64),
    /// `AML_OBJTYPE_STRING`: `v_string`, `length` bytes (the C keeps a NUL after them).
    String(Vec<u8>),
    /// `AML_OBJTYPE_BUFFER`: `v_buffer`.
    Buffer(Vec<u8>),
    /// `AML_OBJTYPE_PACKAGE`: `v_package`.
    Package(Vec<AmlValueRef>),
    /// `AML_OBJTYPE_FIELDUNIT`: `v_field`.
    FieldUnit(AmlField),
    /// `AML_OBJTYPE_DEVICE`.
    Device,
    /// `AML_OBJTYPE_EVENT`: `v_evt.state`.
    Event(i32),
    /// `AML_OBJTYPE_METHOD`: `v_method`.
    Method(AmlMethod),
    /// `AML_OBJTYPE_MUTEX`: `v_mtx`.
    Mutex(AmlMtx),
    /// `AML_OBJTYPE_OPREGION`: `v_opregion`.
    OpRegion(AmlOpregion),
    /// `AML_OBJTYPE_POWERRSRC`: `v_powerrsrc`.
    PowerRsrc(AmlPowerrsrc),
    /// `AML_OBJTYPE_PROCESSOR`: `v_processor`.
    Processor(AmlProcessor),
    /// `AML_OBJTYPE_THERMZONE`.
    ThermZone,
    /// `AML_OBJTYPE_BUFFERFIELD`: `v_field`.
    BufferField(AmlField),
    /// `AML_OBJTYPE_DDBHANDLE`: the table id in `v_integer`.
    DdbHandle(i64),
    /// `AML_OBJTYPE_DEBUGOBJ`.
    DebugObj,
    /// `AML_OBJTYPE_NAMEREF`: `v_nameref`, the encoded name in the byte code.
    NameRef(AmlPtr),
    /// `AML_OBJTYPE_OBJREF`: `v_objref`.
    ObjRef(AmlObjref),
    /// `AML_OBJTYPE_SCOPE`: `v_buffer` and `length` over AML byte code.
    Scope(AmlPtr, i32),
    /// `AML_OBJTYPE_NOTARGET`.
    NoTarget,
    /// Any other `type` number, with an empty union (`_aml_setvalue` accepts any).
    Other(i32),
}

impl AmlObj {
    /// The C's `type` member.
    pub fn r#type(&self) -> i32 {
        match self {
            AmlObj::Uninitialized => AML_OBJTYPE_UNINITIALIZED,
            AmlObj::Integer(_) => AML_OBJTYPE_INTEGER,
            AmlObj::String(_) => AML_OBJTYPE_STRING,
            AmlObj::Buffer(_) => AML_OBJTYPE_BUFFER,
            AmlObj::Package(_) => AML_OBJTYPE_PACKAGE,
            AmlObj::FieldUnit(_) => AML_OBJTYPE_FIELDUNIT,
            AmlObj::Device => AML_OBJTYPE_DEVICE,
            AmlObj::Event(_) => AML_OBJTYPE_EVENT,
            AmlObj::Method(_) => AML_OBJTYPE_METHOD,
            AmlObj::Mutex(_) => AML_OBJTYPE_MUTEX,
            AmlObj::OpRegion(_) => AML_OBJTYPE_OPREGION,
            AmlObj::PowerRsrc(_) => AML_OBJTYPE_POWERRSRC,
            AmlObj::Processor(_) => AML_OBJTYPE_PROCESSOR,
            AmlObj::ThermZone => AML_OBJTYPE_THERMZONE,
            AmlObj::BufferField(_) => AML_OBJTYPE_BUFFERFIELD,
            AmlObj::DdbHandle(_) => AML_OBJTYPE_DDBHANDLE,
            AmlObj::DebugObj => AML_OBJTYPE_DEBUGOBJ,
            AmlObj::NameRef(_) => AML_OBJTYPE_NAMEREF,
            AmlObj::ObjRef(_) => AML_OBJTYPE_OBJREF,
            AmlObj::Scope(..) => AML_OBJTYPE_SCOPE,
            AmlObj::NoTarget => AML_OBJTYPE_NOTARGET,
            AmlObj::Other(t) => *t,
        }
    }

    /// The C's `length` member, as the variant determines it.
    pub fn length(&self) -> i32 {
        match self {
            AmlObj::Integer(_) => aml_intlen() >> 3,
            AmlObj::String(s) | AmlObj::Buffer(s) => s.len() as i32,
            AmlObj::Package(p) => p.len() as i32,
            AmlObj::Scope(_, len) => *len,
            _ => 0,
        }
    }
}

/// `struct aml_value`.
///
/// Its contents change in place through any of the references that share it (`Store`,
/// `_aml_setvalue`, field reads), so the members are interior-mutable; the interpreter holds
/// a borrow only for the duration of one access.
#[derive(Default)]
pub struct AmlValue {
    /// `type`, `length` and the union.
    v: RefCell<AmlObj>,
    /// `node`: the namespace node naming the value, if any.
    node: RefCell<Weak<AmlNode>>,
    /// `stack`: the `AMLOP_LOCALx` opcode of a method local.
    pub stack: Cell<i32>,
}

impl AmlValue {
    /// A zeroed value (`memset(&v, 0, sizeof(v))`): uninitialised, no node.
    pub fn new() -> Self {
        Self::default()
    }

    /// A value of `obj`.
    pub fn from_obj(obj: AmlObj) -> Self {
        Self {
            v: RefCell::new(obj),
            ..Self::default()
        }
    }

    /// An integer value, as callers of `aml_evalnode` build their arguments
    /// (`v.type = AML_OBJTYPE_INTEGER; v.v_integer = x`).
    pub fn integer(x: i64) -> Self {
        Self::from_obj(AmlObj::Integer(x))
    }

    /// A string value.
    pub fn string(s: &[u8]) -> Self {
        Self::from_obj(AmlObj::String(s.to_vec()))
    }

    /// A buffer value.
    pub fn buffer(b: &[u8]) -> Self {
        Self::from_obj(AmlObj::Buffer(b.to_vec()))
    }

    /// The contents, borrowed for one access.
    pub fn obj(&self) -> Ref<'_, AmlObj> {
        self.v.borrow()
    }

    /// Replaces the contents; the old contents are dropped once the borrow has ended.
    pub fn set_obj(&self, obj: AmlObj) {
        let old = self.v.replace(obj);
        drop(old);
    }

    /// Takes the contents out, leaving the value uninitialised.
    pub fn take_obj(&self) -> AmlObj {
        self.v.take()
    }

    /// Runs `f` on the contents, mutably. `f` must not reach this value again.
    pub fn with_obj<R>(&self, f: impl FnOnce(&mut AmlObj) -> R) -> R {
        f(&mut self.v.borrow_mut())
    }

    /// `type`.
    pub fn r#type(&self) -> i32 {
        self.v.borrow().r#type()
    }

    /// `length` (`aml_strlen`, `aml_buflen`, `aml_pkglen`).
    pub fn length(&self) -> i32 {
        self.v.borrow().length()
    }

    /// `v_integer` (`xaml_intval`): the integer, or the table id of a DDB handle; 0 for any
    /// other type.
    pub fn v_integer(&self) -> i64 {
        match &*self.v.borrow() {
            AmlObj::Integer(i) | AmlObj::DdbHandle(i) => *i,
            _ => 0,
        }
    }

    /// `v_buffer`/`v_string` (`aml_bufval`, `aml_strval`): a copy of the bytes of a buffer or
    /// a string, or of the AML range of a scope; empty for other types.
    pub fn v_buffer(&self) -> Vec<u8> {
        match &*self.v.borrow() {
            AmlObj::String(b) | AmlObj::Buffer(b) => b.clone(),
            AmlObj::Scope(p, len) => p.bytes((*len).max(0) as usize).to_vec(),
            _ => Vec::new(),
        }
    }

    /// `v_string` read as a C string: the bytes of a string or buffer up to the first NUL.
    pub fn v_string(&self) -> Vec<u8> {
        let mut b = self.v_buffer();
        if let Some(n) = b.iter().position(|&c| c == 0) {
            b.truncate(n);
        }
        b
    }

    /// `v_package[i]` (`aml_pkgval`): element `i` of a package.
    pub fn v_package(&self, i: usize) -> Option<AmlValueRef> {
        match &*self.v.borrow() {
            AmlObj::Package(p) => p.get(i).cloned(),
            _ => None,
        }
    }

    /// `v_field`: a copy of the descriptor of a field unit or a buffer field.
    pub fn v_field(&self) -> Option<AmlField> {
        match &*self.v.borrow() {
            AmlObj::FieldUnit(f) | AmlObj::BufferField(f) => Some(f.clone()),
            _ => None,
        }
    }

    /// `v_opregion`.
    pub fn v_opregion(&self) -> Option<AmlOpregion> {
        match &*self.v.borrow() {
            AmlObj::OpRegion(r) => Some(*r),
            _ => None,
        }
    }

    /// `v_method`.
    pub fn v_method(&self) -> Option<AmlMethod> {
        match &*self.v.borrow() {
            AmlObj::Method(m) => Some(*m),
            _ => None,
        }
    }

    /// `v_objref`.
    pub fn v_objref(&self) -> Option<AmlObjref> {
        match &*self.v.borrow() {
            AmlObj::ObjRef(o) => Some(o.clone()),
            _ => None,
        }
    }

    /// `v_processor`.
    pub fn v_processor(&self) -> Option<AmlProcessor> {
        match &*self.v.borrow() {
            AmlObj::Processor(p) => Some(*p),
            _ => None,
        }
    }

    /// `v_powerrsrc`.
    pub fn v_powerrsrc(&self) -> Option<AmlPowerrsrc> {
        match &*self.v.borrow() {
            AmlObj::PowerRsrc(p) => Some(*p),
            _ => None,
        }
    }

    /// `v_nameref`: the encoded name of an unresolved reference.
    pub fn v_nameref(&self) -> Option<AmlPtr> {
        match &*self.v.borrow() {
            AmlObj::NameRef(p) => Some(*p),
            _ => None,
        }
    }

    /// `node`: the node naming this value; NULL once that node was deleted.
    pub fn node(&self) -> Option<AmlNodeRef> {
        self.node.borrow().upgrade()
    }

    /// `val->node = node`.
    pub fn set_node(&self, node: Option<&AmlNodeRef>) {
        *self.node.borrow_mut() = node.map_or_else(Weak::new, Rc::downgrade);
    }
}

/// `struct acpi_pci`: what `acpi.c` learns about a PCI device the namespace describes.
pub struct AcpiPci {
    /// `next`: the link in `acpi.c`'s list of them.
    pub next: TailqEntry<AcpiPci>,
    /// `node`.
    pub node: RefCell<Option<AmlNodeRef>>,
    /// `device`: the attached driver, once there is one.
    pub device: Cell<Option<&'static Device>>,
    /// `sub`.
    pub sub: Cell<i32>,
    /// `seg`.
    pub seg: Cell<i32>,
    /// `bus`.
    pub bus: Cell<i32>,
    /// `dev`.
    pub dev: Cell<i32>,
    /// `fun`.
    pub fun: Cell<i32>,
    /// `_s0w`.
    pub _s0w: Cell<i32>,
    /// `_s3d`.
    pub _s3d: Cell<i32>,
    /// `_s3w`.
    pub _s3w: Cell<i32>,
    /// `_s4d`.
    pub _s4d: Cell<i32>,
    /// `_s4w`.
    pub _s4w: Cell<i32>,
    /// `d3cold`.
    pub d3cold: Cell<i32>,
}

/// The interrupt handler `acpi_gpio.intr_establish` takes: `int (*)(void *)`.
pub type AcpiGpioIntrFn = fn(*mut c_void) -> i32;

/// `struct acpi_gpio`: a GPIO controller's pin operations, registered on its node.
pub struct AcpiGpio {
    /// `cookie`: the controller's softc.
    pub cookie: *mut c_void,
    /// `read_pin(cookie, pin)`.
    pub read_pin: fn(*mut c_void, i32) -> i32,
    /// `write_pin(cookie, pin, value)`.
    pub write_pin: fn(*mut c_void, i32, i32),
    /// `intr_establish(cookie, pin, flags, ipl, func, arg)`.
    pub intr_establish: fn(*mut c_void, i32, i32, i32, AcpiGpioIntrFn, *mut c_void),
    /// `intr_enable(cookie, pin)`.
    pub intr_enable: fn(*mut c_void, i32),
    /// `intr_disable(cookie, pin)`.
    pub intr_disable: fn(*mut c_void, i32),
}

/// `struct aml_node`: a name in the ACPI namespace.
pub struct AmlNode {
    /// `parent`; empty for the root.
    parent: Weak<AmlNode>,
    /// `son`: the children, in the order they were made (`SIMPLEQ_INSERT_TAIL`).
    son: RefCell<Vec<AmlNodeRef>>,
    /// `attached`: a driver attached to the device.
    pub attached: Cell<i32>,
    /// `name`: four characters and a NUL.
    pub name: [u8; 5],
    /// `opcode`.
    pub opcode: Cell<u16>,
    /// `start`: for the root, the table being parsed (`aml_pc`).
    pub start: Cell<Option<AmlPtr>>,
    /// `end`.
    pub end: Cell<Option<AmlPtr>>,
    /// `value`: the object the name stands for.
    pub value: RefCell<Option<AmlValueRef>>,
    /// `pci`: set by `acpi.c` on PCI devices.
    pub pci: Cell<Option<&'static AcpiPci>>,
    /// `gpio`: set by a GPIO controller's driver.
    pub gpio: Cell<Option<&'static AcpiGpio>>,
}

impl AmlNode {
    /// A node named `name` below `parent`, not yet in its parent's list.
    pub fn new(parent: Option<&AmlNodeRef>, name: [u8; 5]) -> Self {
        Self {
            parent: parent.map_or_else(Weak::new, Rc::downgrade),
            son: RefCell::new(Vec::new()),
            attached: Cell::new(0),
            name,
            opcode: Cell::new(0),
            start: Cell::new(None),
            end: Cell::new(None),
            value: RefCell::new(None),
            pci: Cell::new(None),
            gpio: Cell::new(None),
        }
    }

    /// `node->parent`: NULL for the root and for a node whose parent was deleted.
    pub fn parent(&self) -> Option<AmlNodeRef> {
        self.parent.upgrade()
    }

    /// `node->name`, without the NUL.
    pub fn name(&self) -> &[u8] {
        let n = self.name.iter().position(|&c| c == 0).unwrap_or(4);
        &self.name[..n]
    }

    /// `SIMPLEQ_FOREACH(child, &node->son, sib)`: a snapshot of the children, so the caller
    /// may evaluate AML (which can add or delete nodes) while walking them.
    pub fn sons(&self) -> Vec<AmlNodeRef> {
        self.son.borrow().clone()
    }

    /// `SIMPLEQ_INSERT_TAIL(&node->son, child, sib)`.
    pub(crate) fn add_son(&self, child: AmlNodeRef) {
        self.son.borrow_mut().push(child);
    }

    /// `SIMPLEQ_FIRST` + `SIMPLEQ_REMOVE_HEAD(&node->son, sib)`.
    pub(crate) fn take_first_son(&self) -> Option<AmlNodeRef> {
        let mut son = self.son.borrow_mut();
        if son.is_empty() {
            None
        } else {
            Some(son.remove(0))
        }
    }

    /// `node->value`.
    pub fn value(&self) -> Option<AmlValueRef> {
        self.value.borrow().clone()
    }
}

/// `aml_bitmask(n)`: the bit of bit position `n` within its byte.
pub const fn aml_bitmask(n: i32) -> u8 {
    1u8 << (n & 0x7)
}

/// `aml_bitpos(n)`.
pub const fn aml_bitpos(n: i32) -> i32 {
    n & 0x7
}

/// `aml_bytepos(n)`: the byte holding bit `n`.
pub const fn aml_bytepos(n: i32) -> i32 {
    n >> 3
}

/// `aml_bytelen(n)`: the bytes `n` bits span.
pub const fn aml_bytelen(n: i32) -> i32 {
    (n + 7) >> 3
}

/// `aml_bytealigned(x)`.
pub const fn aml_bytealigned(x: i32) -> bool {
    x & 0x7 == 0
}

/// `AML_NO_TIMEOUT`: the `Acquire`/`Wait` timeout that never expires.
pub const AML_NO_TIMEOUT: i32 = 0xffff;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the AML types and their macros.

    use super::*;

    #[test]
    fn method_and_field_flags() {
        assert_eq!(aml_method_argcount(0x0b), 3);
        assert_eq!(aml_method_serialized(0x0b), 1);
        assert_eq!(aml_method_synclevel(0x5b), 5);
        assert_eq!(aml_field_access(0x11), AML_FIELD_BYTEACC);
        assert_eq!(aml_field_lock(0x13), AML_FIELD_LOCK_ON);
        assert_eq!(aml_field_update(0x43), AML_FIELD_WRITEASZEROES);
        assert_eq!(aml_field_attr(0x0b05), 0x0b);
        assert_eq!(aml_bitmask(13), 0x20);
        assert_eq!(aml_bytelen(9), 2);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/acpi/amltypes.h");
        crate::reftest::assert_defines!(defs;
            AMLOP_ZERO, AMLOP_ONE, AMLOP_ALIAS, AMLOP_NAME, AMLOP_BYTEPREFIX, AMLOP_WORDPREFIX,
            AMLOP_DWORDPREFIX, AMLOP_STRINGPREFIX, AMLOP_QWORDPREFIX, AMLOP_SCOPE, AMLOP_BUFFER,
            AMLOP_PACKAGE, AMLOP_VARPACKAGE, AMLOP_METHOD, AMLOP_DUALNAMEPREFIX,
            AMLOP_MULTINAMEPREFIX, AMLOP_EXTPREFIX, AMLOP_MUTEX, AMLOP_EVENT, AMLOP_CONDREFOF,
            AMLOP_CREATEFIELD, AMLOP_LOADTABLE, AMLOP_LOAD, AMLOP_STALL, AMLOP_SLEEP,
            AMLOP_ACQUIRE, AMLOP_SIGNAL, AMLOP_WAIT, AMLOP_RESET, AMLOP_RELEASE, AMLOP_FROMBCD,
            AMLOP_TOBCD, AMLOP_UNLOAD, AMLOP_REVISION, AMLOP_DEBUG, AMLOP_FATAL, AMLOP_TIMER,
            AMLOP_OPREGION, AMLOP_FIELD, AMLOP_DEVICE, AMLOP_PROCESSOR, AMLOP_POWERRSRC,
            AMLOP_THERMALZONE, AMLOP_INDEXFIELD, AMLOP_BANKFIELD, AMLOP_DATAREGION,
            AMLOP_ROOTCHAR, AMLOP_PARENTPREFIX, AMLOP_NAMECHAR, AMLOP_LOCAL0, AMLOP_LOCAL1,
            AMLOP_LOCAL2, AMLOP_LOCAL3, AMLOP_LOCAL4, AMLOP_LOCAL5, AMLOP_LOCAL6, AMLOP_LOCAL7,
            AMLOP_ARG0, AMLOP_ARG1, AMLOP_ARG2, AMLOP_ARG3, AMLOP_ARG4, AMLOP_ARG5, AMLOP_ARG6,
            AMLOP_STORE, AMLOP_REFOF, AMLOP_ADD, AMLOP_CONCAT, AMLOP_SUBTRACT, AMLOP_INCREMENT,
            AMLOP_DECREMENT, AMLOP_MULTIPLY, AMLOP_DIVIDE, AMLOP_SHL, AMLOP_SHR, AMLOP_AND,
            AMLOP_NAND, AMLOP_OR, AMLOP_NOR, AMLOP_XOR, AMLOP_NOT, AMLOP_FINDSETLEFTBIT,
            AMLOP_FINDSETRIGHTBIT, AMLOP_DEREFOF, AMLOP_CONCATRES, AMLOP_MOD, AMLOP_NOTIFY,
            AMLOP_SIZEOF, AMLOP_INDEX, AMLOP_MATCH, AMLOP_CREATEDWORDFIELD,
            AMLOP_CREATEWORDFIELD, AMLOP_CREATEBYTEFIELD, AMLOP_CREATEBITFIELD,
            AMLOP_OBJECTTYPE, AMLOP_CREATEQWORDFIELD, AMLOP_LAND, AMLOP_LOR, AMLOP_LNOT,
            AMLOP_LNOTEQUAL, AMLOP_LLESSEQUAL, AMLOP_LGREATEREQUAL, AMLOP_LEQUAL,
            AMLOP_LGREATER, AMLOP_LLESS, AMLOP_TOBUFFER, AMLOP_TODECSTRING, AMLOP_TOHEXSTRING,
            AMLOP_TOINTEGER, AMLOP_TOSTRING, AMLOP_COPYOBJECT, AMLOP_MID, AMLOP_CONTINUE,
            AMLOP_IF, AMLOP_ELSE, AMLOP_WHILE, AMLOP_NOP, AMLOP_RETURN, AMLOP_BREAK,
            AMLOP_BREAKPOINT, AMLOP_ONES, AMLOP_INVALID, AML_MATCH_TR, AML_MATCH_EQ,
            AML_MATCH_LE, AML_MATCH_LT, AML_MATCH_GE, AML_MATCH_GT, AML_FIELD_ACCESSMASK,
            AML_FIELD_ANYACC, AML_FIELD_BYTEACC, AML_FIELD_WORDACC, AML_FIELD_DWORDACC,
            AML_FIELD_QWORDACC, AML_FIELD_BUFFERACC, AML_FIELD_LOCK_OFF, AML_FIELD_LOCK_ON,
            AML_FIELD_PRESERVE, AML_FIELD_WRITEASONES, AML_FIELD_WRITEASZEROES,
            AML_FIELD_RESERVED, AML_FIELD_ATTR__, AML_NO_TIMEOUT,
        );
    }

    #[test]
    fn aml_pointers_read_zero_past_the_end() {
        static CODE: [u8; 4] = [1, 2, 3, 4];
        let p = AmlPtr::new(&CODE);
        assert_eq!(p.get16(), 0x0201);
        assert_eq!(p.add(2).get32(), 0x0403);
        assert_eq!(p.add(9).get8(), 0);
        assert!(p < p.add(1));
        assert_eq!(p.add(3).diff(p), 3);
        assert_eq!(p.add(1).bytes(8), &[2, 3, 4]);
    }

    #[test]
    fn values_and_nodes() {
        let v = AmlValue::integer(7);
        assert_eq!(
            (v.r#type(), v.v_integer(), v.length()),
            (AML_OBJTYPE_INTEGER, 7, 8)
        );
        let s = AmlValue::string(b"ab\0c");
        assert_eq!((s.length(), s.v_string()), (4, b"ab".to_vec()));
        let root = Rc::new(AmlNode::new(None, *b"\\\0\0\0\0"));
        let child = Rc::new(AmlNode::new(Some(&root), *b"CHLD\0"));
        root.add_son(child.clone());
        assert!(child.parent().is_some_and(|p| Rc::ptr_eq(&p, &root)));
        assert_eq!(child.name(), b"CHLD");
        assert_eq!(
            root.take_first_son().map(|c| Rc::ptr_eq(&c, &child)),
            Some(true)
        );
        drop(root);
        assert!(child.parent().is_none());
    }
}
/* </TESTS> */
