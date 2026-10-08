/*	$OpenBSD: fdt.c,v 1.8 2023/02/13 16:16:03 kettenis Exp $	*/
/*	$OpenBSD: fdt.h,v 1.3 2017/08/23 18:03:54 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2009 Dariusz Swiderski <sfires@sfires.net>
 * Copyright (c) 2009, 2016 Mark Kettenis <kettenis@openbsd.org>
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
//! A flattened device tree editor: find nodes and read properties, and add or change
//! properties and nodes in place, moving the rest of the blob up into its free space
//! (efiboot hands the kernel the firmware's tree with `/chosen` filled in, or one it built
//! from the ACPI tables).
//!
//! Upstream: sys/arch/arm64/stand/efiboot/fdt.c @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/fdt.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct fdt` is [`Fdt`], whose pointers are offsets from the header, and a node
//!   (`void *` in C) is an [`FdtNode`], the offset of its `FDT_NODE_BEGIN` token. The C's
//!   one tree (`tree`, `tree_inited`) is the static `TREE`, which the C-named free functions
//!   work on; the [`Fdt`] methods do the work, so the host tests run on trees of their own.
//! - Every token, name and value is read within the blob (`fh_size` bytes): what lies past
//!   it reads as zeros, which no loop takes for a token to follow; the C reads on. A NULL
//!   node (`None`) is no node: the C dereferences it.
//! - Values come back as copies (`Vec<u8>`) from the free functions, where the C returns a
//!   pointer into the tree that the next edit may move; [`Fdt::node_property`] returns the
//!   length and the offset, as the C.
//! - `fdt_node_property_int` returns the value or `None`, for the C's count of 1 or not.
//! - The `DEBUG` printers (`fdt_print_property`, `fdt_print_node`, `fdt_print_tree`) are not
//!   compiled, as in the C (efiboot does not define `DEBUG`).
//! - "FDT overflow" is libsa's `panic()`, as in the C.

use alloc::vec::Vec;
use core::ptr;

use libkern::staticcell::StaticCell;

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

/// `FDT_CODE_VERSION`.
pub const FDT_CODE_VERSION: u32 = 0x11;

/// `roundup(x, sizeof(uint32_t))`.
const fn roundup4(x: usize) -> usize {
    x.div_ceil(4) * 4
}

/// `struct fdt_head`: the blob's header, big-endian words.
#[repr(C)]
#[derive(Clone, Copy, Default)]
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
    /// `fh_boot_cpu_id`: `fh_version` >= 2.
    pub fh_boot_cpu_id: u32,
    /// `fh_strings_size`: `fh_version` >= 3.
    pub fh_strings_size: u32,
    /// `fh_struct_size`: `fh_version` >= 17.
    pub fh_struct_size: u32,
}

/// The byte offsets of the header's words.
const FH_SIZE: usize = 4;
const FH_STRUCT_OFF: usize = 8;
const FH_STRINGS_OFF: usize = 12;
const FH_RESERVE_OFF: usize = 16;
const FH_VERSION: usize = 20;
const FH_STRINGS_SIZE: usize = 32;
const FH_STRUCT_SIZE: usize = 36;

/// A node of the tree: the offset of its `FDT_NODE_BEGIN` token from the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdtNode(pub usize);

/// `struct fdt` (and `tree_inited`): the tree being worked on. The C's pointers are offsets
/// from the header.
pub struct Fdt {
    /// `header`: the blob, null when not initialised.
    header: *mut u8,
    /// `tree`: the structure block.
    tree: usize,
    /// `strings`: the strings block.
    strings: usize,
    /// `memory`: the memory reservation block.
    memory: usize,
    /// `end`: the end of the blob (`fh_size`).
    end: usize,
    /// `version`.
    version: u32,
    /// `strings_size`.
    strings_size: usize,
    /// `struct_size`.
    struct_size: usize,
    /// `tree_inited`.
    inited: bool,
}

// SAFETY: efiboot is single-threaded; the blob is only reached through the one tree.
unsafe impl Send for Fdt {}

impl Fdt {
    /// No tree (`memset(&tree, 0, ...)`).
    pub const fn new() -> Self {
        Self {
            header: ptr::null_mut(),
            tree: 0,
            strings: 0,
            memory: 0,
            end: 0,
            version: 0,
            strings_size: 0,
            struct_size: 0,
            inited: false,
        }
    }

    /// The big-endian word at `off`, 0 past the end of the blob.
    fn rd32(&self, off: usize) -> u32 {
        if self.header.is_null() || off.checked_add(4).is_none_or(|e| e > self.end) {
            return 0;
        }
        // SAFETY: `off..off + 4` lies within the blob, which `init`'s caller vouched for.
        unsafe { u32::from_be(self.header.add(off).cast::<u32>().read_unaligned()) }
    }

    /// Writes the big-endian word `v` at `off` (within the blob).
    fn wr32(&mut self, off: usize, v: u32) {
        self.wr(off, &v.to_be_bytes());
    }

    /// Writes `data` at `off`, as much of it as the blob holds.
    fn wr(&mut self, off: usize, data: &[u8]) {
        if self.header.is_null() || off >= self.end {
            return;
        }
        let n = data.len().min(self.end - off);
        // SAFETY: `off..off + n` lies within the blob, which `init`'s caller vouched for and
        // which this tree alone writes; `data` is not in the blob (callers pass copies).
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), self.header.add(off), n) };
    }

    /// The bytes `off..off + len`, cut at the end of the blob.
    fn bytes(&self, off: usize, len: usize) -> &[u8] {
        if self.header.is_null() || off >= self.end {
            return &[];
        }
        let n = len.min(self.end - off);
        // SAFETY: within the blob (see `rd32`); the slice lives as long as `&self`, which
        // no edit can run during.
        unsafe { core::slice::from_raw_parts(self.header.add(off), n) }
    }

    /// `memmove(dst, src, len)` within the blob.
    fn move_bytes(&mut self, dst: usize, src: usize, len: usize) {
        if self.header.is_null() || src.max(dst).saturating_add(len) > self.end {
            return;
        }
        // SAFETY: both ranges lie within the blob (checked); `copy` allows overlap.
        unsafe { ptr::copy(self.header.add(src), self.header.add(dst), len) };
    }

    /// `memset(off, 0, len)` within the blob.
    fn zero(&mut self, off: usize, len: usize) {
        if self.header.is_null() || off.saturating_add(len) > self.end {
            return;
        }
        // SAFETY: within the blob (checked).
        unsafe { ptr::write_bytes(self.header.add(off), 0, len) };
    }

    /// `strlen` of the NUL-terminated string at `off` (cut at the end of the blob).
    fn strlen(&self, off: usize) -> usize {
        let b = self.bytes(off, usize::MAX);
        b.iter().position(|&c| c == 0).unwrap_or(b.len())
    }

    /// `fdt_init(fdt)`: work on the blob at `fdt`; its version, or 0 (no tree) if it is not
    /// one.
    ///
    /// # Safety
    ///
    /// `fdt` is null or points at a blob whose header's `fh_size` bytes are readable and
    /// writable, and that nothing else uses while this tree works on it.
    pub unsafe fn init(&mut self, fdt: *mut u8) -> u32 {
        *self = Self::new();

        if fdt.is_null() {
            return 0;
        }

        // SAFETY: the caller's contract.
        let version = unsafe { fdt_check_head(fdt) };
        if version == 0 {
            return 0;
        }

        self.header = fdt;
        // SAFETY: `fdt_check_head` read the header, so its words are readable.
        let hdr = |off: usize| unsafe { u32::from_be(fdt.add(off).cast::<u32>().read_unaligned()) };
        self.end = hdr(FH_SIZE) as usize;
        self.tree = hdr(FH_STRUCT_OFF) as usize;
        self.strings = hdr(FH_STRINGS_OFF) as usize;
        self.memory = hdr(FH_RESERVE_OFF) as usize;
        self.version = version;
        self.strings_size = hdr(FH_STRINGS_SIZE) as usize;
        if self.version >= 17 {
            self.struct_size = hdr(FH_STRUCT_SIZE) as usize;
        }
        self.inited = true;

        version
    }

    /// `fdt_finalize()`: write the blob's sizes and offsets back into its header.
    pub fn finalize(&mut self) {
        if self.header.is_null() {
            return;
        }
        self.wr32(FH_SIZE, self.end as u32);
        self.wr32(FH_STRUCT_OFF, self.tree as u32);
        self.wr32(FH_STRINGS_OFF, self.strings as u32);
        self.wr32(FH_RESERVE_OFF, self.memory as u32);
        self.wr32(FH_STRINGS_SIZE, self.strings_size as u32);
        if self.version >= 17 {
            self.wr32(FH_STRUCT_SIZE, self.struct_size as u32);
        }
    }

    /// `fdt_get_str(num)`: the offset of string `num` of the strings block.
    fn get_str(&self, num: u32) -> Option<usize> {
        if num as usize > self.strings_size {
            return None;
        }
        (self.strings != 0).then_some(self.strings + num as usize)
    }

    /// `fdt_add_str(name)`: append `name` to the strings block; its offset there.
    fn add_str(&mut self, name: &[u8]) -> u32 {
        let len = roundup4(name.len() + 1);
        let end = self.strings + self.strings_size;

        if end + len > self.end {
            libsa::exit::panic(format_args!("FDT overflow"));
        }

        self.strings_size += len;
        self.zero(end, len);
        self.wr(end, name);

        (end - self.strings) as u32
    }

    /// `skip_nops(ptr)`.
    fn skip_nops(&self, mut ptr: usize) -> usize {
        while self.rd32(ptr) == FDT_NOP {
            ptr += 4;
        }
        ptr
    }

    /// `skip_property(ptr)`: move forward by magic + size + nameid + rounded up property
    /// size.
    fn skip_property(&self, ptr: usize) -> usize {
        let size = self.rd32(ptr + 4) as usize;
        self.skip_nops(ptr + 12 + roundup4(size))
    }

    /// `skip_props(ptr)`.
    fn skip_props(&self, mut ptr: usize) -> usize {
        while self.rd32(ptr) == FDT_PROPERTY {
            ptr = self.skip_property(ptr);
        }
        ptr
    }

    /// `skip_node_name(ptr)`: skip the name, NUL-terminated and aligned to 4 bytes.
    fn skip_node_name(&self, ptr: usize) -> usize {
        self.skip_nops(ptr + roundup4(self.strlen(ptr) + 1))
    }

    /// `fdt_node_property(node, name, &out)`: the length of the property `name` of `node`
    /// and the offset of its value; 0 when there is none. The value stays in the tree.
    pub fn node_property(&self, node: FdtNode, name: &[u8]) -> (u32, usize) {
        if !self.inited {
            return (0, 0);
        }

        let mut ptr = node.0;
        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return (0, 0);
        }

        ptr = self.skip_node_name(ptr + 4);

        while self.rd32(ptr) == FDT_PROPERTY {
            let nameid = self.rd32(ptr + 8); // id of name in strings table
            if let Some(tmp) = self.get_str(nameid)
                && self.cstr(tmp) == name
            {
                // beginning of the value, size of value
                return (self.rd32(ptr + 4), ptr + 12);
            }
            ptr = self.skip_property(ptr);
        }
        (0, 0)
    }

    /// The NUL-terminated string at `off`, without its NUL.
    fn cstr(&self, off: usize) -> &[u8] {
        let n = self.strlen(off);
        self.bytes(off, n)
    }

    /// The value of the property `name` of `node`, `None` where the C returns 0.
    pub fn property(&self, node: FdtNode, name: &[u8]) -> Option<&[u8]> {
        match self.node_property(node, name) {
            (0, _) => None,
            (len, off) => Some(self.bytes(off, len as usize)),
        }
    }

    /// `fdt_node_set_property(node, name, data, len)`: replace the value of the existing
    /// property `name`; 1 if it did, 0 if there is none.
    pub fn set_property(&mut self, node: FdtNode, name: &[u8], data: &[u8]) -> i32 {
        let end = self.strings + self.strings_size;

        if !self.inited {
            return 0;
        }

        let mut ptr = node.0;
        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return 0;
        }

        ptr = self.skip_node_name(ptr + 4);

        while self.rd32(ptr) == FDT_PROPERTY {
            let nameid = self.rd32(ptr + 8); // id of name in strings table
            let next = self.skip_property(ptr);
            if let Some(tmp) = self.get_str(nameid)
                && self.cstr(tmp) == name
            {
                let curlen = self.rd32(ptr + 4) as usize;
                let delta = roundup4(data.len()) as isize - roundup4(curlen) as isize;
                if end as isize + delta > self.end as isize {
                    libsa::exit::panic(format_args!("FDT overflow"));
                }

                self.move_bytes(
                    next.wrapping_add_signed(delta),
                    next,
                    end.saturating_sub(next),
                );
                self.struct_size = self.struct_size.wrapping_add_signed(delta);
                self.strings = self.strings.wrapping_add_signed(delta);
                self.wr32(ptr + 4, data.len() as u32);
                self.wr(ptr + 12, data);
                return 1;
            }
            ptr = next;
        }
        0
    }

    /// `fdt_node_add_property(node, name, data, len)`: set the property `name`, adding it
    /// first (empty, after the node's name) if `node` has none; 1 or 0 as `set_property`.
    pub fn add_property(&mut self, node: FdtNode, name: &[u8], data: &[u8]) -> i32 {
        let end = self.strings + self.strings_size;

        if !self.inited {
            return 0;
        }

        if self.node_property(node, name).0 == 0 {
            let mut ptr = node.0;

            if self.rd32(ptr) != FDT_NODE_BEGIN {
                return 0;
            }

            if end + 12 > self.end {
                libsa::exit::panic(format_args!("FDT overflow"));
            }

            ptr = self.skip_node_name(ptr + 4);

            self.move_bytes(ptr + 12, ptr, end.saturating_sub(ptr));
            self.struct_size += 12;
            self.strings += 12;
            self.wr32(ptr, FDT_PROPERTY);
            self.wr32(ptr + 4, 0);
            let nameoff = self.add_str(name);
            self.wr32(ptr + 8, nameoff);
        }

        self.set_property(node, name, data)
    }

    /// `fdt_node_add_node(node, name, &child)`: a new empty node `name` after the children
    /// of `node`.
    pub fn add_node(&mut self, node: FdtNode, name: &[u8]) -> Option<FdtNode> {
        let len = roundup4(name.len() + 1) + 8;
        let end = self.strings + self.strings_size;
        let mut ptr = node.0;

        if !self.inited {
            return None;
        }

        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return None;
        }

        if end + len > self.end {
            libsa::exit::panic(format_args!("FDT overflow"));
        }

        ptr = self.skip_node_name(ptr + 4);
        ptr = self.skip_props(ptr);

        // skip children
        while self.rd32(ptr) == FDT_NODE_BEGIN {
            ptr = self.skip_node(ptr);
        }

        self.move_bytes(ptr + len, ptr, end.saturating_sub(ptr));
        self.struct_size += len;
        self.strings += len;

        let child = FdtNode(ptr);
        self.wr32(ptr, FDT_NODE_BEGIN);
        self.zero(ptr + 4, len - 8);
        self.wr(ptr + 4, name);
        self.wr32(ptr + len - 4, FDT_NODE_END);

        Some(child)
    }

    /// `skip_node(node)`: the token after the node and its children, whether a node or not.
    fn skip_node(&self, node: usize) -> usize {
        let mut ptr = node + 4;

        ptr = self.skip_node_name(ptr);
        ptr = self.skip_props(ptr);

        // skip children
        while self.rd32(ptr) == FDT_NODE_BEGIN {
            ptr = self.skip_node(ptr);
        }

        self.skip_nops(ptr + 4)
    }

    /// `fdt_next_node(node)`: the next sibling of `node`, or the root for `None`.
    pub fn next_node(&self, node: Option<FdtNode>) -> Option<FdtNode> {
        if !self.inited {
            return None;
        }

        let Some(FdtNode(mut ptr)) = node else {
            let ptr = self.skip_nops(self.tree);
            return (self.rd32(ptr) == FDT_NODE_BEGIN).then_some(FdtNode(ptr));
        };

        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return None;
        }

        ptr += 4;

        ptr = self.skip_node_name(ptr);
        ptr = self.skip_props(ptr);

        // skip children
        while self.rd32(ptr) == FDT_NODE_BEGIN {
            ptr = self.skip_node(ptr);
        }

        if self.rd32(ptr) != FDT_NODE_END {
            return None;
        }

        ptr = self.skip_nops(ptr + 4);

        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return None;
        }

        Some(FdtNode(ptr))
    }

    /// `fdt_node_property_ints(node, name, out, outlen)`: the property's big-endian words
    /// into `out`; how many, or -1 if there are none.
    pub fn node_property_ints(&self, node: FdtNode, name: &[u8], out: &mut [i32]) -> i32 {
        let (len, off) = self.node_property(node, name);
        let inlen = len as usize / 4;
        if inlen == 0 {
            return -1;
        }

        let mut i = 0;
        while i < inlen && i < out.len() {
            out[i] = self.rd32(off + 4 * i) as i32;
            i += 1;
        }

        i as i32
    }

    /// `fdt_node_property_int(node, name, &out)`.
    pub fn node_property_int(&self, node: FdtNode, name: &[u8]) -> Option<u32> {
        let mut v = [0i32; 1];
        (self.node_property_ints(node, name, &mut v) == 1).then_some(v[0] as u32)
    }

    /// `fdt_child_node(node)`: the first child of `node`.
    pub fn child_node(&self, node: FdtNode) -> Option<FdtNode> {
        if !self.inited {
            return None;
        }

        let mut ptr = node.0;

        if self.rd32(ptr) != FDT_NODE_BEGIN {
            return None;
        }

        ptr += 4;

        ptr = self.skip_node_name(ptr);
        ptr = self.skip_props(ptr);
        // check if there is a child node
        (self.rd32(ptr) == FDT_NODE_BEGIN).then_some(FdtNode(ptr))
    }

    /// `fdt_node_name(node)`.
    pub fn node_name(&self, node: FdtNode) -> Option<&[u8]> {
        if !self.inited {
            return None;
        }

        if self.rd32(node.0) != FDT_NODE_BEGIN {
            return None;
        }

        Some(self.cstr(node.0 + 4))
    }

    /// `fdt_find_node(name)`: the node of the absolute path `name`, each component matched
    /// as a prefix of a child's name (`strncmp`), as the C.
    pub fn find_node(&self, name: &[u8]) -> Option<FdtNode> {
        let mut node = self.next_node(None);
        let mut p = 0;

        if !self.inited {
            return None;
        }

        if name.first() != Some(&b'/') {
            return None;
        }

        while p < name.len() {
            while name.get(p) == Some(&b'/') {
                p += 1;
            }
            if p == name.len() {
                return node;
            }
            let q = name[p..]
                .iter()
                .position(|&c| c == b'/')
                .map_or(name.len(), |i| p + i);

            let mut child = node.and_then(|n| self.child_node(n));
            while let Some(c) = child {
                if strncmp(&name[p..q], self.node_name(c).unwrap_or(&[]), q - p) == 0 {
                    node = Some(c);
                    break;
                }
                child = self.next_node(Some(c));
            }

            child?; // No match found.

            p = q;
        }

        node
    }

    /// `fdt_parent_node_recurse(pnode, child)`.
    #[allow(dead_code)] // fdt.h's interface; efiboot itself does not call it
    fn parent_node_recurse(&self, pnode: FdtNode, child: FdtNode) -> Option<FdtNode> {
        let mut node = self.child_node(pnode);

        while let Some(n) = node
            && n != child
        {
            if let Some(tmp) = self.parent_node_recurse(n, child) {
                return Some(tmp);
            }
            node = self.next_node(Some(n));
        }
        node.map(|_| pnode)
    }

    /// `fdt_parent_node(node)`.
    #[allow(dead_code)] // fdt.h's interface; efiboot itself does not call it
    pub fn parent_node(&self, node: FdtNode) -> Option<FdtNode> {
        let pnode = self.next_node(None)?;

        if !self.inited {
            return None;
        }

        if node == pnode {
            return None;
        }

        self.parent_node_recurse(pnode, node)
    }

    /// `fdt_node_is_compatible(node, name)`: `name` is one of the node's "compatible"
    /// strings.
    pub fn node_is_compatible(&self, node: FdtNode, name: &[u8]) -> bool {
        let Some(mut data) = self.property(node, b"compatible") else {
            return false;
        };
        let mut len = data.len() as isize;
        while len > 0 {
            let s = &data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())];
            if s == name {
                return true;
            }
            len -= s.len() as isize + 1;
            data = data.get(s.len() + 1..).unwrap_or(&[]);
        }
        false
    }
}

impl Default for Fdt {
    fn default() -> Self {
        Self::new()
    }
}

/// `tree`, `tree_inited`: the tree the C-named functions work on.
static TREE: StaticCell<Fdt> = StaticCell::new(Fdt::new());

/// The tree.
fn tree() -> &'static mut Fdt {
    // SAFETY: efiboot is single-threaded and no caller keeps the reference across a call to
    // another of these functions (each takes it for one method call).
    unsafe { TREE.get_mut() }
}

/// `strncmp(a, b, n)` of two names (a NUL ends either).
fn strncmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return i32::from(x) - i32::from(y);
        }
        if x == 0 {
            break;
        }
    }
    0
}

/// `fdt_check_head(fdt)`: the blob's version, or 0 if it is not a blob this code reads.
///
/// # Safety
///
/// `fdt` points at a readable header and, if it says it is a blob, at its `fh_size` bytes.
pub unsafe fn fdt_check_head(fdt: *const u8) -> u32 {
    // SAFETY: the header is readable (the caller's contract).
    let rd = |off: usize| unsafe { u32::from_be(fdt.add(off).cast::<u32>().read_unaligned()) };

    if rd(0) != FDT_MAGIC {
        return 0;
    }

    let version = rd(FH_VERSION);
    if version > FDT_CODE_VERSION {
        return 0;
    }

    let size = rd(FH_SIZE) as usize;
    let word = |off: usize| {
        if off.checked_add(4).is_some_and(|e| e <= size) {
            rd(off)
        } else {
            0
        }
    };

    let struct_off = rd(FH_STRUCT_OFF) as usize;
    let mut tok = struct_off;
    while word(tok) == FDT_NOP {
        tok += 4;
    }
    if word(tok) != FDT_NODE_BEGIN {
        return 0;
    }

    // check for end signature on version 17 blob
    if version >= 17 {
        let struct_size = rd(FH_STRUCT_SIZE) as usize;
        let last = (struct_off / 4 + struct_size / 4)
            .wrapping_sub(1)
            .wrapping_mul(4);
        if word(last) != FDT_END {
            return 0;
        }
    }

    version
}

/// `fdt_init(fdt)`: work on the blob at `fdt` from now on; its version, or 0.
///
/// # Safety
///
/// As [`Fdt::init`]: `fdt` is null or a blob that is the tree's alone from now on.
pub unsafe fn fdt_init(fdt: *mut u8) -> u32 {
    // SAFETY: the caller's contract.
    unsafe { tree().init(fdt) }
}

/// `fdt_finalize()`.
pub fn fdt_finalize() {
    tree().finalize();
}

/// `fdt_get_size(fdt)`: the size of the blob at `fdt`, 0 if it is not one.
///
/// # Safety
///
/// `fdt` is null or points at a readable header and, if it is a blob, its bytes.
pub unsafe fn fdt_get_size(fdt: *const u8) -> usize {
    if fdt.is_null() {
        return 0;
    }

    // SAFETY: the caller's contract.
    if unsafe { fdt_check_head(fdt) } == 0 {
        return 0;
    }

    // SAFETY: as above; `fh_size` is the header's second word.
    unsafe { u32::from_be(fdt.add(FH_SIZE).cast::<u32>().read_unaligned()) as usize }
}

/// Runs `f` on the tree the C-named functions work on (for `efiacpi.rs`, whose table
/// handlers take the tree they edit).
pub fn fdt_with_tree<R>(f: impl FnOnce(&mut Fdt) -> R) -> R {
    f(tree())
}

/// `fdt_next_node(node)`.
pub fn fdt_next_node(node: Option<FdtNode>) -> Option<FdtNode> {
    tree().next_node(node)
}

/// `fdt_child_node(node)`.
pub fn fdt_child_node(node: Option<FdtNode>) -> Option<FdtNode> {
    node.and_then(|n| tree().child_node(n))
}

/// `fdt_node_name(node)`: a copy of the node's name.
pub fn fdt_node_name(node: Option<FdtNode>) -> Option<Vec<u8>> {
    node.and_then(|n| tree().node_name(n).map(<[u8]>::to_vec))
}

/// `fdt_find_node(name)`.
pub fn fdt_find_node(name: &[u8]) -> Option<FdtNode> {
    tree().find_node(name)
}

/// `fdt_node_property(node, name, &out)`: a copy of the value; `None` where the C returns
/// 0 (no such property, or an empty one).
pub fn fdt_node_property(node: Option<FdtNode>, name: &[u8]) -> Option<Vec<u8>> {
    node.and_then(|n| tree().property(n, name).map(<[u8]>::to_vec))
}

/// `fdt_node_property_int(node, name, &out)`: the value, if the property has a word.
pub fn fdt_node_property_int(node: Option<FdtNode>, name: &[u8]) -> Option<u32> {
    node.and_then(|n| tree().node_property_int(n, name))
}

/// `fdt_node_property_ints(node, name, out, outlen)`.
#[allow(dead_code)] // fdt.h's interface; efiboot itself does not call it
pub fn fdt_node_property_ints(node: Option<FdtNode>, name: &[u8], out: &mut [i32]) -> i32 {
    node.map_or(-1, |n| tree().node_property_ints(n, name, out))
}

/// `fdt_node_set_property(node, name, data, len)`.
pub fn fdt_node_set_property(node: Option<FdtNode>, name: &[u8], data: &[u8]) -> i32 {
    node.map_or(0, |n| tree().set_property(n, name, data))
}

/// `fdt_node_add_property(node, name, data, len)`.
pub fn fdt_node_add_property(node: Option<FdtNode>, name: &[u8], data: &[u8]) -> i32 {
    node.map_or(0, |n| tree().add_property(n, name, data))
}

/// `fdt_node_add_node(node, name, &child)`: the new child.
pub fn fdt_node_add_node(node: Option<FdtNode>, name: &[u8]) -> Option<FdtNode> {
    node.and_then(|n| tree().add_node(n, name))
}

/// `fdt_parent_node(node)`.
#[allow(dead_code)] // fdt.h's interface; efiboot itself does not call it
pub fn fdt_parent_node(node: Option<FdtNode>) -> Option<FdtNode> {
    node.and_then(|n| tree().parent_node(n))
}

/// `fdt_node_is_compatible(node, name)`.
pub fn fdt_node_is_compatible(node: Option<FdtNode>, name: &[u8]) -> bool {
    node.is_some_and(|n| tree().node_is_compatible(n, name))
}

const _: () = assert!(core::mem::size_of::<FdtHead>() == 40);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // The tree editor on copies of the ACPI template blob (`dt_blob.rs`).

    use super::*;
    use crate::dt_blob::DT_BLOB_TEMPLATE;
    use std::vec::Vec;

    /// A copy of the template blob, and a tree on it.
    fn template() -> (Vec<u8>, Fdt) {
        let mut b = DT_BLOB_TEMPLATE.to_vec();
        let mut t = Fdt::new();
        // SAFETY: `b` is a whole blob that only `t` uses; its heap buffer does not move.
        assert_eq!(unsafe { t.init(b.as_mut_ptr()) }, 17);
        (b, t)
    }

    /// A second tree on the blob `t` wrote, after `fdt_finalize`.
    fn reread(b: &mut [u8]) -> Fdt {
        let mut t = Fdt::new();
        // SAFETY: `b` is a whole blob that only the new tree uses.
        assert_eq!(unsafe { t.init(b.as_mut_ptr()) }, 17);
        t
    }

    #[test]
    fn check_head_rejects_what_is_not_a_blob() {
        let mut zero = [0u8; 64];
        // SAFETY: 64 readable bytes.
        assert_eq!(unsafe { fdt_check_head(zero.as_ptr()) }, 0);
        // SAFETY: as above; null is no tree.
        assert_eq!(unsafe { Fdt::new().init(core::ptr::null_mut()) }, 0);
        zero[..4].copy_from_slice(&FDT_MAGIC.to_be_bytes());
        zero[20..24].copy_from_slice(&0x12u32.to_be_bytes()); // newer than the code
        // SAFETY: as above.
        assert_eq!(unsafe { fdt_check_head(zero.as_ptr()) }, 0);
        let (b, _) = template();
        // SAFETY: a whole blob.
        assert_eq!(unsafe { fdt_get_size(b.as_ptr()) }, b.len());
    }

    #[test]
    fn find_matches_prefixes_and_paths() {
        let (_b, t) = template();
        let root = t.next_node(None).unwrap();
        assert_eq!(t.find_node(b"/"), Some(root));
        assert_eq!(t.find_node(b"//"), Some(root));
        let serial = t.find_node(b"/serial").unwrap();
        assert_eq!(t.node_name(serial), Some(&b"serial@0"[..]));
        assert_eq!(t.find_node(b"/serial@1"), None);
        assert_eq!(t.find_node(b"chosen"), None);
        assert_eq!(t.find_node(b"/chosen/framebuffer"), None);
        assert_eq!(t.parent_node(serial), Some(root));
        assert_eq!(t.parent_node(root), None);
        assert_eq!(t.child_node(serial), None);
    }

    #[test]
    fn add_and_set_properties_and_nodes() {
        let (mut b, mut t) = template();
        let chosen = t.find_node(b"/chosen").unwrap();
        assert_eq!(t.add_property(chosen, b"bootargs", b"sd0a:/bsd -s\0"), 1);
        assert_eq!(
            t.add_property(chosen, b"openbsd,boothowto", &2u32.to_be_bytes()),
            1
        );
        // a property that exists is set in place, longer, then shorter
        let psci = t.find_node(b"/psci").unwrap();
        assert_eq!(t.set_property(psci, b"method", b"hvc-long-name\0"), 1);
        assert_eq!(t.set_property(psci, b"method", b"hvc\0"), 1);
        assert_eq!(t.set_property(psci, b"nope", b"x\0"), 0);
        let cpus = t.find_node(b"/cpus").unwrap();
        let cpu = t.add_node(cpus, b"cpu@0").unwrap();
        assert_eq!(t.add_property(cpu, b"reg", &0u64.to_be_bytes()), 1);
        assert_eq!(t.add_property(cpu, b"msi-controller", &[]), 1);
        let cpu1 = t.add_node(cpus, b"cpu@1").unwrap();
        assert_eq!(t.add_property(cpu1, b"device_type", b"cpu\0"), 1);
        t.finalize();

        let t = reread(&mut b);
        let chosen = t.find_node(b"/chosen").unwrap();
        assert_eq!(
            t.property(chosen, b"bootargs"),
            Some(&b"sd0a:/bsd -s\0"[..])
        );
        assert_eq!(t.node_property_int(chosen, b"openbsd,boothowto"), Some(2));
        assert_eq!(
            t.property(chosen, b"stdout-path"),
            Some(&b"serial0:115200n8\0"[..])
        );
        let psci = t.find_node(b"/psci").unwrap();
        assert_eq!(t.property(psci, b"method"), Some(&b"hvc\0"[..]));
        assert_eq!(t.property(psci, b"status"), Some(&b"disabled\0"[..]));
        let cpu = t.find_node(b"/cpus/cpu@0").unwrap();
        assert_eq!(t.property(cpu, b"reg"), Some(&[0u8; 8][..]));
        let cpu1 = t.find_node(b"/cpus/cpu@1").unwrap();
        assert_eq!(t.next_node(Some(cpu)), Some(cpu1));
        assert!(t.find_node(b"/acpi").is_some());
        // the new names went to the strings block once each
        let strings = u32::from_be_bytes([b[12], b[13], b[14], b[15]]) as usize;
        let size = u32::from_be_bytes([b[32], b[33], b[34], b[35]]) as usize;
        let block = &b[strings..strings + size];
        assert_eq!(block.windows(9).filter(|w| w == b"bootargs\0").count(), 1);
        assert!(block.ends_with(b"device_type\0"));
    }

    #[test]
    fn property_ints_and_compatible() {
        let (_b, mut t) = template();
        let gic = t.find_node(b"/interrupt-controller").unwrap();
        let mut out = [0i32; 2];
        assert_eq!(t.node_property_ints(gic, b"#interrupt-cells", &mut out), 1);
        assert_eq!(out[0], 3);
        assert_eq!(t.node_property_ints(gic, b"nope", &mut out), -1);
        assert_eq!(
            t.set_property(gic, b"compatible", b"arm,gic-400\0arm,cortex-a15-gic\0"),
            1
        );
        assert!(t.node_is_compatible(gic, b"arm,cortex-a15-gic"));
        assert!(t.node_is_compatible(gic, b"arm,gic-400"));
        assert!(!t.node_is_compatible(gic, b"arm,gic"));
    }
}
/* </TESTS> */
