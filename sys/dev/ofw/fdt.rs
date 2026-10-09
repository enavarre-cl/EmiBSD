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
 * Copyright (c) 2009 Dariusz Swiderski <sfires@sfires.net>
 * Copyright (c) 2009 Mark Kettenis <kettenis@openbsd.org>
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
//! The flattened device tree: `dev/ofw/fdt.c`, with the types of `<dev/ofw/fdt.h>` and the
//! `OF_*` accessors `<dev/ofw/openfirm.h>` declares (`fdt.c` implements them over the blob).
//!
//! Upstream: sys/dev/ofw/fdt.c @ 3ce1f3f79392
//! Upstream: sys/dev/ofw/fdt.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the read side: `fdt_check_head`, `fdt_init`,
//! `fdt_get_size`, `fdt_get_str`, the `skip_*` helpers, `fdt_node_property`,
//! `fdt_node_property_ints`/`_int`, `fdt_next_node`, `fdt_child_node`, `fdt_node_name`,
//! `fdt_find_node`, `fdt_parent_node`, `fdt_find_phandle`, `fdt_get_cells`,
//! `fdt_translate_reg`, `fdt_get_reg`, `fdt_is_compatible`, and `OF_peer`, `OF_child`,
//! `OF_parent`, `OF_finddevice`, `OF_getnodebyname`, `OF_getnodebyphandle`, `OF_getproplen`,
//! `OF_getprop`, `OF_getpropbool`, `OF_getpropint`, `OF_getpropintarray`, `OF_getpropint64`,
//! `OF_getpropint64array`, `OF_is_compatible`, `OF_is_enabled`, `OF_getindex`. The write side
//! (`fdt_finalize`, `fdt_add_str`, `fdt_node_set_property`, `fdt_node_add_property`,
//! `OF_setprop`), `fdt_next_property`/`OF_nextprop`, `OF_getpropstr`/`OF_freepropstr`,
//! `OF_translate` and the `DEBUG` printers come with the drivers that need them (M5).
//!
//! ## Deviations
//! - A node is an [`FdtNode`], a pointer into the blob (null is the C's `NULL`); an `OF_*`
//!   handle is the node's byte offset from the header, as in C (0 is none, `-1` is
//!   `OF_finddevice`'s failure). Property values and names come back as byte slices into the
//!   blob instead of a length and an out pointer; `None` is the C's `-1`.
//! - The tree is a `StaticCell`, written once by `fdt_init` on the boot CPU; the blob is only
//!   read, through unaligned big-endian reads.
//! - Names are byte strings without a NUL; the blob's strings are NUL-terminated.

#![allow(non_snake_case)] // the OF_* functions keep OpenFirmware's names

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use libkern::StaticCell;

use crate::sys::errno::Errno;

/// `struct fdt_head`: the blob's header, big-endian.
#[repr(C)]
pub struct FdtHead {
    /// `fh_magic`.
    pub fh_magic: u32,
    /// `fh_size`.
    pub fh_size: u32,
    /// `fh_struct_off`.
    pub fh_struct_off: u32,
    /// `fh_strings_off`.
    pub fh_strings_off: u32,
    /// `fh_reserve_off`.
    pub fh_reserve_off: u32,
    /// `fh_version`.
    pub fh_version: u32,
    /// `fh_comp_ver`: last compatible version.
    pub fh_comp_ver: u32,
    /// `fh_boot_cpu_id`: `fh_version >= 2`.
    pub fh_boot_cpu_id: u32,
    /// `fh_strings_size`: `fh_version >= 3`.
    pub fh_strings_size: u32,
    /// `fh_struct_size`: `fh_version >= 17`.
    pub fh_struct_size: u32,
}

/// `struct fdt`: the parsed blob.
pub struct Fdt {
    /// `header`.
    pub header: *const FdtHead,
    /// `tree`: the structure block.
    pub tree: *const u8,
    /// `strings`: the strings block.
    pub strings: *const u8,
    /// `memory`: the memory reservation block.
    pub memory: *const u8,
    /// `end`: one past the blob.
    pub end: *const u8,
    /// `version`.
    pub version: i32,
    /// `strings_size`.
    pub strings_size: i32,
    /// `struct_size`.
    pub struct_size: i32,
}

// SAFETY: written once by `fdt_init` on the boot CPU before any reader; the pointers address
// the bootloader's blob, which is never freed.
unsafe impl Sync for Fdt {}
// SAFETY: as above.
unsafe impl Send for Fdt {}

impl Fdt {
    const fn empty() -> Self {
        Self {
            header: ptr::null(),
            tree: ptr::null(),
            strings: ptr::null(),
            memory: ptr::null(),
            end: ptr::null(),
            version: 0,
            strings_size: 0,
            struct_size: 0,
        }
    }
}

/// `struct fdt_reg`: an address and size from a `reg` property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FdtReg {
    /// `addr`.
    pub addr: u64,
    /// `size`.
    pub size: u64,
}

/// A node: a pointer to its `FDT_NODE_BEGIN` token in the blob; null is no node.
pub type FdtNode = *const u32;

/// `FDT_MAGIC`.
pub const FDT_MAGIC: u32 = 0xd00d_feed;
/// `FDT_NODE_BEGIN`.
pub const FDT_NODE_BEGIN: u32 = 0x01;
/// `FDT_NODE_END`.
pub const FDT_NODE_END: u32 = 0x02;
/// `FDT_PROPERTY`.
pub const FDT_PROPERTY: u32 = 0x03;
/// `FDT_NOP`.
pub const FDT_NOP: u32 = 0x04;
/// `FDT_END`.
pub const FDT_END: u32 = 0x09;
/// `FDT_CODE_VERSION`: the newest version this parser knows.
pub const FDT_CODE_VERSION: u32 = 0x11;

/// `OFMAXPARAM` (`<dev/ofw/openfirm.h>`): the longest property name `OF_nextprop` returns.
pub const OFMAXPARAM: usize = 64;

/// `tree`: the parsed blob.
static TREE: StaticCell<Fdt> = StaticCell::new(Fdt::empty());
/// `tree_inited`.
static TREE_INITED: AtomicBool = AtomicBool::new(false);

/// The parsed tree.
fn tree() -> &'static Fdt {
    // SAFETY: written only by `fdt_init` on the boot CPU before anything reads it.
    unsafe { TREE.get() }
}

/// `betoh32(*ptr)`: one big-endian word of the blob.
fn be32(p: *const u32) -> u32 {
    // SAFETY: the callers walk the structure block of a blob `fdt_check_head` accepted, so
    // the word is inside it; the read is unaligned on purpose (the spec aligns to 4 bytes).
    u32::from_be(unsafe { ptr::read_unaligned(p) })
}

/// The NUL-terminated string at `p` as a slice without the NUL.
fn cstr(p: *const u8) -> &'static [u8] {
    let mut n = 0;
    // SAFETY: a NUL-terminated string inside the blob (a node name or a strings-block entry).
    while unsafe { ptr::read(p.add(n)) } != 0 {
        n += 1;
    }
    // SAFETY: as above: `n` bytes before the NUL, in the blob, which lives forever.
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// `strcmp(a, b) == 0` for a NUL-terminated `b` in the blob and a NUL-free `a`.
fn name_eq(a: &[u8], b: *const u8) -> bool {
    cstr(b) == a
}

/// `fdt_check_head`: the blob's version, 0 if it is not one this parser accepts.
pub fn fdt_check_head(fdt: *const u8) -> u32 {
    let fh = fdt.cast::<FdtHead>();
    let p = fdt.cast::<u32>();
    // SAFETY: the caller hands over the bootloader's blob, at least a header long.
    let (magic, version, struct_off, struct_size) = unsafe {
        (
            u32::from_be(ptr::read_unaligned(ptr::addr_of!((*fh).fh_magic))),
            u32::from_be(ptr::read_unaligned(ptr::addr_of!((*fh).fh_version))),
            u32::from_be(ptr::read_unaligned(ptr::addr_of!((*fh).fh_struct_off))),
            u32::from_be(ptr::read_unaligned(ptr::addr_of!((*fh).fh_struct_size))),
        )
    };
    if magic != FDT_MAGIC {
        return 0;
    }
    if version > FDT_CODE_VERSION {
        return 0;
    }
    let tok = skip_nops(p.wrapping_add(struct_off as usize / 4));
    if be32(tok) != FDT_NODE_BEGIN {
        return 0;
    }
    // check for end signature on version 17 blob
    if version >= 17
        && be32(p.wrapping_add(struct_off as usize / 4 + struct_size as usize / 4 - 1)) != FDT_END
    {
        return 0;
    }
    version
}

/// `fdt_init`: initializes internal structures of module. Has to be called once, preferably
/// in `machdep.c`. Returns the version, 0 when `fdt` is null or not a blob.
pub fn fdt_init(fdt: *const u8) -> i32 {
    // SAFETY: the boot CPU, once, before any reader.
    let tree = unsafe { TREE.get_mut() };
    *tree = Fdt::empty();
    TREE_INITED.store(false, Ordering::Relaxed);

    if fdt.is_null() {
        return 0;
    }
    let version = fdt_check_head(fdt);
    if version == 0 {
        return 0;
    }

    let fh = fdt.cast::<FdtHead>();
    // SAFETY: a blob `fdt_check_head` accepted, so the whole header is readable; the reads
    // are unaligned on purpose.
    let (struct_off, strings_off, reserve_off, size, strings_size, struct_size) = unsafe {
        let field = |p: *const u32| u32::from_be(ptr::read_unaligned(p)) as usize;
        (
            field(ptr::addr_of!((*fh).fh_struct_off)),
            field(ptr::addr_of!((*fh).fh_strings_off)),
            field(ptr::addr_of!((*fh).fh_reserve_off)),
            field(ptr::addr_of!((*fh).fh_size)),
            field(ptr::addr_of!((*fh).fh_strings_size)),
            field(ptr::addr_of!((*fh).fh_struct_size)),
        )
    };
    tree.header = fh;
    tree.tree = fdt.wrapping_add(struct_off);
    tree.strings = fdt.wrapping_add(strings_off);
    tree.memory = fdt.wrapping_add(reserve_off);
    tree.end = fdt.wrapping_add(size);
    tree.version = version as i32;
    tree.strings_size = strings_size as i32;
    if tree.version >= 17 {
        tree.struct_size = struct_size as i32;
    }
    TREE_INITED.store(true, Ordering::Relaxed);
    version as i32
}

/// `fdt_get_size`: return the size of the FDT.
pub fn fdt_get_size(fdt: *const u8) -> usize {
    if fdt.is_null() || fdt_check_head(fdt) == 0 {
        return 0;
    }
    // SAFETY: a blob `fdt_check_head` accepted.
    u32::from_be(unsafe { ptr::read_unaligned(ptr::addr_of!((*fdt.cast::<FdtHead>()).fh_size)) })
        as usize
}

/// `fdt_get_str`: retrieve string pointer from strings table.
fn fdt_get_str(num: u32) -> Option<*const u8> {
    let t = tree();
    if num > t.strings_size as u32 {
        return None;
    }
    if t.strings.is_null() {
        None
    } else {
        Some(t.strings.wrapping_add(num as usize))
    }
}

/// `skip_nops`.
fn skip_nops(mut p: *const u32) -> *const u32 {
    while be32(p) == FDT_NOP {
        p = p.wrapping_add(1);
    }
    p
}

/// `skip_property`: move forward by magic + size + nameid + rounded up property size.
fn skip_property(p: *const u32) -> *const u32 {
    let size = be32(p.wrapping_add(1)) as usize;
    skip_nops(p.wrapping_add(3 + size.div_ceil(4)))
}

/// `skip_props`.
fn skip_props(mut p: *const u32) -> *const u32 {
    while be32(p) == FDT_PROPERTY {
        p = skip_property(p);
    }
    p
}

/// `skip_node_name`: skip name, aligned to 4 bytes, this is NULL term., so must add 1.
fn skip_node_name(p: *const u32) -> *const u32 {
    let len = cstr(p.cast::<u8>()).len() + 1;
    skip_nops(p.wrapping_add(len.div_ceil(4)))
}

/// `fdt_node_property`: retrieves node property, the returned slice is inside the fdt tree,
/// so we should not modify content pointed by it directly. `None` when the node has no such
/// property.
pub fn fdt_node_property(node: FdtNode, name: &[u8]) -> Option<&'static [u8]> {
    if !TREE_INITED.load(Ordering::Relaxed) || node.is_null() {
        return None;
    }
    let mut p = node;
    if be32(p) != FDT_NODE_BEGIN {
        return None;
    }
    p = skip_node_name(p.wrapping_add(1));
    while be32(p) == FDT_PROPERTY {
        let nameid = be32(p.wrapping_add(2)); // id of name in strings table
        if let Some(s) = fdt_get_str(nameid)
            && name_eq(name, s)
        {
            let size = be32(p.wrapping_add(1)) as usize; // size of value
            // SAFETY: the value follows the three header words, inside the blob.
            return Some(unsafe {
                core::slice::from_raw_parts(p.wrapping_add(3).cast::<u8>(), size)
            });
        }
        p = skip_property(p);
    }
    None
}

/// `skip_node`: retrieves next node, skipping all the children nodes of the pointed node,
/// returns pointer to next node, no matter if it exists or not.
fn skip_node(node: FdtNode) -> *const u32 {
    let mut p = node.wrapping_add(1);
    p = skip_node_name(p);
    p = skip_props(p);
    // skip children
    while be32(p) == FDT_NODE_BEGIN {
        p = skip_node(p);
    }
    skip_nops(p.wrapping_add(1))
}

/// `fdt_next_node`: retrieves next node, skipping all the children nodes of the pointed
/// node, returns pointer to next node if exists, otherwise returns null. If passed null will
/// return first node of the tree (root).
pub fn fdt_next_node(node: FdtNode) -> FdtNode {
    if !TREE_INITED.load(Ordering::Relaxed) {
        return ptr::null();
    }
    if node.is_null() {
        let p = skip_nops(tree().tree.cast::<u32>());
        return if be32(p) == FDT_NODE_BEGIN {
            p
        } else {
            ptr::null()
        };
    }
    let mut p = node;
    if be32(p) != FDT_NODE_BEGIN {
        return ptr::null();
    }
    p = p.wrapping_add(1);
    p = skip_node_name(p);
    p = skip_props(p);
    // skip children
    while be32(p) == FDT_NODE_BEGIN {
        p = skip_node(p);
    }
    if be32(p) != FDT_NODE_END {
        return ptr::null();
    }
    p = skip_nops(p.wrapping_add(1));
    if be32(p) != FDT_NODE_BEGIN {
        return ptr::null();
    }
    p
}

/// `fdt_node_property_ints`: retrieves node property as integers and puts them in the given
/// integer array; how many it put, or `None` when there is no such property.
pub fn fdt_node_property_ints(node: FdtNode, name: &[u8], out: &mut [i32]) -> Option<usize> {
    let data = fdt_node_property(node, name)?;
    let inlen = data.len() / 4;
    if inlen == 0 {
        return None;
    }
    let n = inlen.min(out.len());
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        *slot = i32::from_be_bytes([
            data[4 * i],
            data[4 * i + 1],
            data[4 * i + 2],
            data[4 * i + 3],
        ]);
    }
    Some(n)
}

/// `fdt_node_property_int`: retrieves node property as an integer.
pub fn fdt_node_property_int(node: FdtNode, name: &[u8]) -> Option<i32> {
    let mut v = [0i32; 1];
    fdt_node_property_ints(node, name, &mut v).map(|_| v[0])
}

/// `fdt_child_node`: the first child of `node`, if any.
pub fn fdt_child_node(node: FdtNode) -> FdtNode {
    if !TREE_INITED.load(Ordering::Relaxed) || node.is_null() {
        return ptr::null();
    }
    let mut p = node;
    if be32(p) != FDT_NODE_BEGIN {
        return ptr::null();
    }
    p = p.wrapping_add(1);
    p = skip_node_name(p);
    p = skip_props(p);
    // check if there is a child node
    if be32(p) == FDT_NODE_BEGIN {
        p
    } else {
        ptr::null()
    }
}

/// `fdt_node_name`: retrieves node name.
pub fn fdt_node_name(node: FdtNode) -> Option<&'static [u8]> {
    if !TREE_INITED.load(Ordering::Relaxed) || node.is_null() {
        return None;
    }
    if be32(node) != FDT_NODE_BEGIN {
        return None;
    }
    Some(cstr(node.wrapping_add(1).cast::<u8>()))
}

/// `fdt_find_node`: the node at the absolute path `name`, matching each component with or
/// without its unit address.
pub fn fdt_find_node(name: &[u8]) -> FdtNode {
    let mut node = fdt_next_node(ptr::null());
    if !TREE_INITED.load(Ordering::Relaxed) {
        return ptr::null();
    }
    if name.first() != Some(&b'/') {
        return ptr::null();
    }
    let mut p = name;
    while !p.is_empty() {
        while p.first() == Some(&b'/') {
            p = &p[1..];
        }
        if p.is_empty() {
            return node;
        }
        let q = p.iter().position(|&c| c == b'/').unwrap_or(p.len());
        let component = &p[..q];

        // Check for a complete match.
        let mut child = fdt_child_node(node);
        while !child.is_null() {
            if fdt_node_name(child) == Some(component) {
                break;
            }
            child = fdt_next_node(child);
        }
        if !child.is_null() {
            node = child;
            p = &p[q..];
            continue;
        }

        // Check for a match without the unit name.
        let mut child = fdt_child_node(node);
        while !child.is_null() {
            if let Some(s) = fdt_node_name(child)
                && s.len() > component.len()
                && &s[..component.len()] == component
                && s[component.len()] == b'@'
            {
                break;
            }
            child = fdt_next_node(child);
        }
        if !child.is_null() {
            node = child;
            p = &p[q..];
            continue;
        }

        return ptr::null(); // No match found.
    }
    node
}

/// `fdt_parent_node_recurse`.
fn fdt_parent_node_recurse(pnode: FdtNode, child: FdtNode) -> FdtNode {
    let mut node = fdt_child_node(pnode);
    while !node.is_null() && node != child {
        let tmp = fdt_parent_node_recurse(node, child);
        if !tmp.is_null() {
            return tmp;
        }
        node = fdt_next_node(node);
    }
    if node.is_null() { ptr::null() } else { pnode }
}

/// `fdt_parent_node`: the parent of `node`, null for the root.
pub fn fdt_parent_node(node: FdtNode) -> FdtNode {
    let pnode = fdt_next_node(ptr::null());
    if !TREE_INITED.load(Ordering::Relaxed) {
        return ptr::null();
    }
    if node == pnode {
        return ptr::null();
    }
    fdt_parent_node_recurse(pnode, node)
}

/// `fdt_find_phandle_recurse`.
fn fdt_find_phandle_recurse(node: FdtNode, phandle: u32) -> FdtNode {
    let data =
        fdt_node_property(node, b"phandle").or_else(|| fdt_node_property(node, b"linux,phandle"));
    if let Some(d) = data
        && d.len() == 4
        && u32::from_be_bytes([d[0], d[1], d[2], d[3]]) == phandle
    {
        return node;
    }
    let mut child = fdt_child_node(node);
    while !child.is_null() {
        let tmp = fdt_find_phandle_recurse(child, phandle);
        if !tmp.is_null() {
            return tmp;
        }
        child = fdt_next_node(child);
    }
    ptr::null()
}

/// `fdt_find_phandle`: the node with `phandle`.
pub fn fdt_find_phandle(phandle: u32) -> FdtNode {
    fdt_find_phandle_recurse(fdt_next_node(ptr::null()), phandle)
}

/// `fdt_get_cells`: the `#address-cells` and `#size-cells` in force at `node` (inherited
/// from the parents, 1 and 1 at the top).
pub fn fdt_get_cells(node: FdtNode) -> (i32, i32) {
    let parent = fdt_parent_node(node);
    let (mut ac, mut sc) = if parent.is_null() {
        (1, 1)
    } else {
        fdt_get_cells(parent)
    };
    if let Some(v) = fdt_node_property_int(node, b"#address-cells") {
        ac = v;
    }
    if let Some(v) = fdt_node_property_int(node, b"#size-cells") {
        sc = v;
    }
    (ac, sc)
}

/// One big-endian cell of a property value.
fn cell(data: &[u8], i: usize) -> u64 {
    u64::from(u32::from_be_bytes([
        data[4 * i],
        data[4 * i + 1],
        data[4 * i + 2],
        data[4 * i + 3],
    ]))
}

/// `fdt_translate_reg`: translate memory address depending on parent's range.
///
/// Ranges are a way of mapping one address to another. This ranges attribute is set on a
/// node's parent. This means if a node does not have a parent, there's nothing to translate.
/// If it does have a parent and the parent does not have a ranges attribute, there's nothing
/// to translate either.
///
/// If the parent has a ranges attribute and the attribute is not empty, the node's memory
/// address has to be in one of the given ranges. This range is then used to translate the
/// memory address.
///
/// If the parent has a ranges attribute, but the attribute is empty, there's nothing to
/// translate. But it's not a translation barrier. It can be treated as a simple 1:1 mapping.
///
/// Translation does not end here. We need to check if the parent's parent also has a ranges
/// attribute and ask the same questions again.
fn fdt_translate_reg(node: FdtNode, reg: &mut FdtReg) -> Result<(), Errno> {
    // No parent, no translation.
    let parent = fdt_parent_node(node);
    if parent.is_null() {
        return Ok(());
    }

    // Extract ranges property from node. No ranges means translation barrier. Translation
    // stops here.
    let Some(range) = fdt_node_property(node, b"ranges") else {
        return Ok(());
    };
    let mut rlen = range.len() / 4;

    // Empty ranges means 1:1 mapping. Continue translation on parent.
    if rlen == 0 {
        return fdt_translate_reg(parent, reg);
    }

    // Get parent address/size width. We only support 32-bit (1) and 64-bit (2) wide
    // addresses and sizes here.
    let (pac, psc) = fdt_get_cells(parent);
    if !(1..=2).contains(&pac) || !(1..=2).contains(&psc) {
        return Err(Errno::EINVAL);
    }

    // Get our own address/size width. Again, we only support 32-bit (1) and 64-bit (2) wide
    // addresses and sizes here.
    let (ac, sc) = fdt_get_cells(node);
    if !(1..=2).contains(&ac) || !(1..=2).contains(&sc) {
        return Err(Errno::EINVAL);
    }
    let (pac, ac, sc) = (pac as usize, ac as usize, sc as usize);

    // Must have at least one range.
    let rone = pac + ac + sc;
    if rlen < rone {
        return Err(Errno::ESRCH);
    }

    // For each range.
    let mut base = 0;
    while rlen >= rone {
        let r = |i: usize| cell(range, base + i);
        // Extract from and size, so we can see if we fit.
        let mut from = r(0);
        if ac == 2 {
            from = (from << 32) + r(1);
        }
        let mut size = r(ac + pac);
        if sc == 2 {
            size = (size << 32) + r(ac + pac + 1);
        }

        // Try next, if we're not in the range.
        if reg.addr < from || reg.addr + reg.size > from + size {
            rlen -= rone;
            base += rone;
            continue;
        }

        // All good, extract to address and translate.
        let mut to = r(ac);
        if pac == 2 {
            to = (to << 32) + r(ac + 1);
        }
        reg.addr -= from;
        reg.addr += to;
        return fdt_translate_reg(parent, reg);
    }

    // To be successful, we must have returned in the for-loop.
    Err(Errno::ESRCH)
}

/// `fdt_get_reg`: parse the memory address and size of a node (entry `idx` of `reg`).
pub fn fdt_get_reg(node: FdtNode, idx: usize, reg: &mut FdtReg) -> Result<(), Errno> {
    if node.is_null() {
        return Err(Errno::EINVAL);
    }
    let parent = fdt_parent_node(node);
    if parent.is_null() {
        return Err(Errno::EINVAL);
    }

    // Get parent address/size width. We only support 32-bit (1) and 64-bit (2) wide
    // addresses and sizes here.
    let (ac, sc) = fdt_get_cells(parent);
    if !(1..=2).contains(&ac) || !(1..=2).contains(&sc) {
        return Err(Errno::EINVAL);
    }
    let (ac, sc) = (ac as usize, sc as usize);

    let data = fdt_node_property(node, b"reg").ok_or(Errno::EINVAL)?;
    let inlen = data.len() / 4;
    if inlen < (idx + 1) * (ac + sc) {
        return Err(Errno::EINVAL);
    }

    let off = idx * (ac + sc);
    reg.addr = cell(data, off);
    if ac == 2 {
        reg.addr = (reg.addr << 32) + cell(data, off + 1);
    }
    reg.size = cell(data, off + ac);
    if sc == 2 {
        reg.size = (reg.size << 32) + cell(data, off + ac + 1);
    }

    fdt_translate_reg(parent, reg)
}

/// `fdt_is_compatible`: whether `name` is one of the node's `compatible` strings.
pub fn fdt_is_compatible(node: FdtNode, name: &[u8]) -> bool {
    let Some(data) = fdt_node_property(node, b"compatible") else {
        return false;
    };
    data.split(|&c| c == 0).any(|s| s == name)
}

/// The node of an `OF_*` handle (its byte offset from the header), for callers that mix the
/// two APIs as `fdt_find_cons` does.
pub fn of_fdt_node(handle: i32) -> FdtNode {
    of_node(handle)
}

/// The node of an `OF_*` handle (its byte offset from the header).
fn of_node(handle: i32) -> FdtNode {
    tree()
        .header
        .cast::<u8>()
        .wrapping_add(handle as usize)
        .cast::<u32>()
}

/// The `OF_*` handle of a node, 0 for none.
fn of_handle(node: FdtNode) -> i32 {
    if node.is_null() {
        0
    } else {
        (node as usize - tree().header as usize) as i32
    }
}

/// `OF_peer`: the next sibling; `OF_peer(0)` is the root.
pub fn OF_peer(handle: i32) -> i32 {
    let node = if handle == 0 {
        fdt_find_node(b"/")
    } else {
        fdt_next_node(of_node(handle))
    };
    of_handle(node)
}

/// `OF_child`: the first child.
pub fn OF_child(handle: i32) -> i32 {
    of_handle(fdt_child_node(of_node(handle)))
}

/// `OF_parent`: the parent.
pub fn OF_parent(handle: i32) -> i32 {
    of_handle(fdt_parent_node(of_node(handle)))
}

/// `OF_finddevice`: the node at the path `name`, or `-1`.
pub fn OF_finddevice(name: &[u8]) -> i32 {
    let node = fdt_find_node(name);
    if node.is_null() { -1 } else { of_handle(node) }
}

/// `OF_getnodebyname`: the child of `handle` (the root for 0) called `name`, with or without
/// its unit address.
pub fn OF_getnodebyname(handle: i32, name: &[u8]) -> i32 {
    let node = if handle == 0 {
        fdt_find_node(b"/")
    } else {
        of_node(handle)
    };

    let mut child = fdt_child_node(node);
    while !child.is_null() {
        if fdt_node_name(child) == Some(name) {
            return of_handle(child);
        }
        child = fdt_next_node(child);
    }

    let len = name.len();
    let mut child = fdt_child_node(node);
    while !child.is_null() {
        if let Some(data) = fdt_node_name(child)
            && data.len() > len
            && &data[..len] == name
            && data[len] == b'@'
        {
            return of_handle(child);
        }
        child = fdt_next_node(child);
    }
    0
}

/// `OF_getnodebyphandle`.
pub fn OF_getnodebyphandle(phandle: u32) -> i32 {
    of_handle(fdt_find_phandle(phandle))
}

/// The "name" property is optional since version 16 of the flattened device tree
/// specification, so we synthesize one from the unit name of the node if it is missing.
fn synthesized_name(node: FdtNode) -> Option<&'static [u8]> {
    let name = fdt_node_name(node)?;
    let at = name.iter().position(|&c| c == b'@').unwrap_or(name.len());
    Some(&name[..at])
}

/// `OF_getproplen`: the length of a property, `-1` when there is none.
pub fn OF_getproplen(handle: i32, prop: &[u8]) -> i32 {
    let node = of_node(handle);
    match fdt_node_property(node, prop) {
        Some(data) => data.len() as i32,
        None if prop == b"name" => match synthesized_name(node) {
            Some(n) => n.len() as i32 + 1,
            None => -1,
        },
        None => -1,
    }
}

/// `OF_getprop`: copies a property into `buf` (at most `buf.len()` bytes); returns the
/// property's full length, or `-1` when there is none.
pub fn OF_getprop(handle: i32, prop: &[u8], buf: &mut [u8]) -> i32 {
    let node = of_node(handle);
    match fdt_node_property(node, prop) {
        Some(data) => {
            let n = data.len().min(buf.len());
            buf[..n].copy_from_slice(&data[..n]);
            data.len() as i32
        }
        None if prop == b"name" => match synthesized_name(node) {
            Some(name) => {
                // strlcpy, then the unit address is cut off
                let n = name.len().min(buf.len().saturating_sub(1));
                buf[..n].copy_from_slice(&name[..n]);
                if n < buf.len() {
                    buf[n] = 0;
                }
                name.len() as i32 + 1
            }
            None => -1,
        },
        None => -1,
    }
}

/// `OF_getpropbool`: whether the property exists.
pub fn OF_getpropbool(handle: i32, prop: &[u8]) -> bool {
    fdt_node_property(of_node(handle), prop).is_some()
}

/// `OF_getpropint`: a 32-bit property, or `defval`.
pub fn OF_getpropint(handle: i32, prop: &[u8], defval: u32) -> u32 {
    let mut val = [0u8; 4];
    if OF_getprop(handle, prop, &mut val) != 4 {
        return defval;
    }
    u32::from_be_bytes(val)
}

/// `OF_getpropintarray`: a property of 32-bit cells into `buf`, byte-swapped; returns the
/// property's length in bytes, or `-1` when it is missing or not a whole number of cells.
pub fn OF_getpropintarray(handle: i32, prop: &[u8], buf: &mut [u32]) -> i32 {
    let node = of_node(handle);
    let Some(data) = fdt_node_property(node, prop) else {
        return -1;
    };
    if data.len() % 4 != 0 {
        return -1;
    }
    let n = (data.len() / 4).min(buf.len());
    for (i, slot) in buf.iter_mut().enumerate().take(n) {
        *slot = cell(data, i) as u32;
    }
    data.len() as i32
}

/// `OF_getpropint64`: a 64-bit property, or `defval`.
pub fn OF_getpropint64(handle: i32, prop: &[u8], defval: u64) -> u64 {
    let mut val = [0u8; 8];
    if OF_getprop(handle, prop, &mut val) != 8 {
        return defval;
    }
    u64::from_be_bytes(val)
}

/// `OF_getpropint64array`: a property of 64-bit cells into `buf`, byte-swapped.
pub fn OF_getpropint64array(handle: i32, prop: &[u8], buf: &mut [u64]) -> i32 {
    let node = of_node(handle);
    let Some(data) = fdt_node_property(node, prop) else {
        return -1;
    };
    if data.len() % 8 != 0 {
        return -1;
    }
    let n = (data.len() / 8).min(buf.len());
    for (i, slot) in buf.iter_mut().enumerate().take(n) {
        *slot = (cell(data, 2 * i) << 32) | cell(data, 2 * i + 1);
    }
    data.len() as i32
}

/// `OF_is_compatible`.
pub fn OF_is_compatible(handle: i32, name: &[u8]) -> bool {
    fdt_is_compatible(of_node(handle), name)
}

/// `OF_is_enabled`: the node's `status` is not "disabled" or "reserved".
pub fn OF_is_enabled(handle: i32) -> bool {
    let mut status = [0u8; 32];
    if OF_getprop(handle, b"status", &mut status) > 0 {
        let s = &status[..status.iter().position(|&c| c == 0).unwrap_or(status.len())];
        if s == b"disabled" || s == b"reserved" {
            return false;
        }
    }
    true
}

/// `OF_getindex`: the index of `entry` in the string list property `prop`, `-1` when it is
/// not there; 0 when `entry` is `None`.
pub fn OF_getindex(handle: i32, entry: Option<&[u8]>, prop: &[u8]) -> i32 {
    let Some(entry) = entry else {
        return 0;
    };
    let Some(names) = fdt_node_property(of_node(handle), prop) else {
        return -1;
    };
    if names.is_empty() {
        return -1;
    }
    for (idx, name) in names.split(|&c| c == 0).enumerate() {
        if name == entry {
            return idx as i32;
        }
    }
    -1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests over a blob built here, shaped like QEMU `virt`'s (a GIC and a PL011).

    use std::sync::Mutex;
    use std::vec::Vec;
    use std::{assert, assert_eq, assert_ne};

    use super::*;

    /// The tree is a global: one test at a time.
    static LOCK: Mutex<()> = Mutex::new(());

    struct Builder {
        structure: Vec<u8>,
        strings: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                structure: Vec::new(),
                strings: Vec::new(),
            }
        }

        fn word(&mut self, w: u32) {
            self.structure.extend_from_slice(&w.to_be_bytes());
        }

        fn pad(&mut self) {
            while self.structure.len() % 4 != 0 {
                self.structure.push(0);
            }
        }

        fn begin(&mut self, name: &[u8]) {
            self.word(FDT_NODE_BEGIN);
            self.structure.extend_from_slice(name);
            self.structure.push(0);
            self.pad();
        }

        fn end(&mut self) {
            self.word(FDT_NODE_END);
        }

        fn string_off(&mut self, name: &[u8]) -> u32 {
            let off = self.strings.len() as u32;
            self.strings.extend_from_slice(name);
            self.strings.push(0);
            off
        }

        fn prop(&mut self, name: &[u8], value: &[u8]) {
            let off = self.string_off(name);
            self.word(FDT_PROPERTY);
            self.word(value.len() as u32);
            self.word(off);
            self.structure.extend_from_slice(value);
            self.pad();
        }

        fn prop_cells(&mut self, name: &[u8], cells: &[u32]) {
            let mut v = Vec::new();
            for c in cells {
                v.extend_from_slice(&c.to_be_bytes());
            }
            self.prop(name, &v);
        }

        fn finish(mut self) -> Vec<u8> {
            self.word(FDT_END);
            let header_len = 40usize;
            let reserve_off = header_len;
            let reserve_len = 16; // one empty entry
            let struct_off = reserve_off + reserve_len;
            let strings_off = struct_off + self.structure.len();
            let size = strings_off + self.strings.len();
            let mut blob = Vec::new();
            for w in [
                FDT_MAGIC,
                size as u32,
                struct_off as u32,
                strings_off as u32,
                reserve_off as u32,
                17,
                16,
                0,
                self.strings.len() as u32,
                self.structure.len() as u32,
            ] {
                blob.extend_from_slice(&w.to_be_bytes());
            }
            blob.extend_from_slice(&[0u8; 16]);
            blob.extend_from_slice(&self.structure);
            blob.extend_from_slice(&self.strings);
            blob
        }
    }

    /// A tree with a root, an interrupt controller, a UART under a bus with ranges, and chosen.
    fn virt_like() -> Vec<u8> {
        let mut b = Builder::new();
        b.begin(b"");
        b.prop_cells(b"#address-cells", &[2]);
        b.prop_cells(b"#size-cells", &[2]);
        b.prop(b"compatible", b"linux,dummy-virt\0");
        b.begin(b"intc@8000000");
        b.prop(b"compatible", b"arm,cortex-a15-gic\0");
        b.prop_cells(
            b"reg",
            &[0, 0x0800_0000, 0, 0x10000, 0, 0x0801_0000, 0, 0x10000],
        );
        b.prop_cells(b"#interrupt-cells", &[3]);
        b.prop(b"interrupt-controller", b"");
        b.prop_cells(b"phandle", &[1]);
        b.end();
        b.begin(b"soc");
        b.prop_cells(b"#address-cells", &[1]);
        b.prop_cells(b"#size-cells", &[1]);
        // 32-bit child addresses 0x1000_0000.. map to parent 0x9000_0000..
        b.prop_cells(b"ranges", &[0x1000_0000, 0, 0x0900_0000, 0x0100_0000]);
        b.begin(b"pl011@10000000");
        b.prop(b"compatible", b"arm,pl011\0arm,primecell\0");
        b.prop_cells(b"reg", &[0x1000_0000, 0x1000]);
        b.prop_cells(b"interrupts", &[0, 1, 4]);
        b.prop_cells(b"interrupt-parent", &[1]);
        b.prop(b"status", b"okay\0");
        b.end();
        b.end();
        b.begin(b"chosen");
        b.prop(b"stdout-path", b"/soc/pl011@10000000\0");
        b.end();
        b.end();
        b.finish()
    }

    #[test]
    fn parses_a_virt_like_tree() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let blob = virt_like();
        assert_eq!(fdt_check_head(blob.as_ptr()), 17);
        assert_eq!(fdt_init(blob.as_ptr()), 17);
        assert_eq!(fdt_get_size(blob.as_ptr()), blob.len());

        let root = fdt_next_node(ptr::null());
        assert!(!root.is_null());
        assert_eq!(fdt_node_name(root), Some(&b""[..]));
        assert_eq!(fdt_find_node(b"/"), root);
        assert!(fdt_parent_node(root).is_null());

        let gic = fdt_find_node(b"/intc@8000000");
        assert!(!gic.is_null());
        assert_eq!(
            fdt_find_node(b"/intc"),
            gic,
            "a match without the unit address"
        );
        assert!(fdt_is_compatible(gic, b"arm,cortex-a15-gic"));
        assert!(!fdt_is_compatible(gic, b"arm,gic-v3"));
        assert_eq!(fdt_node_property_int(gic, b"#interrupt-cells"), Some(3));
        assert!(fdt_node_property(gic, b"interrupt-controller").is_some());
        assert!(fdt_node_property(gic, b"nonsense").is_none());
        assert_eq!(fdt_find_phandle(1), gic);
        assert!(fdt_find_phandle(7).is_null());

        let mut reg = FdtReg::default();
        assert_eq!(fdt_get_reg(gic, 0, &mut reg), Ok(()));
        assert_eq!(
            reg,
            FdtReg {
                addr: 0x0800_0000,
                size: 0x10000
            }
        );
        assert_eq!(fdt_get_reg(gic, 1, &mut reg), Ok(()));
        assert_eq!(reg.addr, 0x0801_0000);
        assert_eq!(fdt_get_reg(gic, 2, &mut reg), Err(Errno::EINVAL));

        let uart = fdt_find_node(b"/soc/pl011@10000000");
        assert!(!uart.is_null());
        assert_eq!(fdt_find_node(b"/soc/pl011"), uart);
        assert_eq!(fdt_parent_node(uart), fdt_find_node(b"/soc"));
        assert_eq!(fdt_get_cells(uart), (1, 1));
        assert_eq!(fdt_get_cells(gic), (2, 2));
        assert_eq!(fdt_get_reg(uart, 0, &mut reg), Ok(()));
        assert_eq!(
            reg,
            FdtReg {
                addr: 0x0900_0000,
                size: 0x1000
            },
            "translated through the bus's ranges"
        );
        let mut ints = [0i32; 3];
        assert_eq!(
            fdt_node_property_ints(uart, b"interrupts", &mut ints),
            Some(3)
        );
        assert_eq!(ints, [0, 1, 4]);

        assert!(fdt_find_node(b"/nowhere").is_null());
        assert!(fdt_find_node(b"relative").is_null());
        assert!(fdt_child_node(fdt_find_node(b"/chosen")).is_null());
        assert_eq!(fdt_next_node(gic), fdt_find_node(b"/soc"));
    }

    #[test]
    fn openfirmware_handles() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let blob = virt_like();
        assert_eq!(fdt_init(blob.as_ptr()), 17);

        let root = OF_peer(0);
        assert_ne!(root, 0);
        let gic = OF_child(root);
        assert_eq!(gic, OF_finddevice(b"/intc@8000000"));
        assert_eq!(OF_parent(gic), root);
        assert_eq!(OF_peer(gic), OF_finddevice(b"/soc"));
        assert_eq!(OF_finddevice(b"/nope"), -1);
        assert_eq!(OF_getnodebyname(0, b"soc"), OF_finddevice(b"/soc"));
        assert_eq!(OF_getnodebyname(0, b"intc"), gic);
        assert_eq!(OF_getnodebyname(0, b"zzz"), 0);
        assert_eq!(OF_getnodebyphandle(1), gic);

        assert_eq!(OF_getpropint(gic, b"#interrupt-cells", 0), 3);
        assert_eq!(OF_getpropint(gic, b"missing", 42), 42);
        assert!(OF_getpropbool(gic, b"interrupt-controller"));
        assert!(!OF_getpropbool(gic, b"missing"));
        assert!(OF_is_compatible(gic, b"arm,cortex-a15-gic"));
        assert!(OF_is_enabled(gic));

        let uart = OF_finddevice(b"/soc/pl011@10000000");
        assert_eq!(OF_getproplen(uart, b"interrupts"), 12);
        let mut cells = [0u32; 4];
        assert_eq!(OF_getpropintarray(uart, b"interrupts", &mut cells), 12);
        assert_eq!(&cells[..3], &[0, 1, 4]);
        assert_eq!(OF_getpropintarray(uart, b"missing", &mut cells), -1);
        assert_eq!(OF_getpropint(uart, b"interrupt-parent", 0), 1);
        assert!(OF_is_enabled(uart));
        assert_eq!(OF_getindex(uart, Some(b"arm,primecell"), b"compatible"), 1);
        assert_eq!(OF_getindex(uart, Some(b"nope"), b"compatible"), -1);
        assert_eq!(OF_getindex(uart, None, b"compatible"), 0);

        // the synthesized "name" of a node without one
        assert_eq!(OF_getproplen(uart, b"name"), 6);
        let mut name = [0u8; 16];
        assert_eq!(OF_getprop(uart, b"name", &mut name), 6);
        assert_eq!(&name[..6], b"pl011\0");
        let mut short = [0u8; 3];
        assert_eq!(OF_getprop(uart, b"name", &mut short), 6);
        assert_eq!(&short, b"pl\0");

        let mut buf = [0u8; 64];
        let chosen = OF_finddevice(b"/chosen");
        let len = OF_getprop(chosen, b"stdout-path", &mut buf);
        assert_eq!(&buf[..len as usize], b"/soc/pl011@10000000\0");
    }
}
/* </TESTS> */
