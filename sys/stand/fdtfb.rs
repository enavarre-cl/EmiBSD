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
//! What arm64's efiboot does with the UEFI GOP frame buffer (`efi_framebuffer()` in
//! `sys/arch/arm64/stand/efiboot/efiboot.c`), done by the Limine glue: a copy of the device
//! tree with a `simple-framebuffer` node `framebuffer` under `/chosen`, so that `simplefb`
//! attaches to it as it does under OpenBSD's boot loader (`docs/ARCHITECTURE.md`,
//! "Frame buffer under Limine").
//!
//! As in efiboot, nothing is added when `/chosen` or `/` already has a `simple-framebuffer`
//! child whose status is absent or `okay`, when the pixel format is not one of the three
//! efiboot knows (`x8b8g8r8`, `x8r8g8b8`, `r5g6b5`), or when the root's `#address-cells` or
//! `#size-cells` is above 2. Unlike efiboot, which edits the tree in place with the room its
//! copy left, this builds a new blob (header, reservation map, structure block with the node
//! spliced in before `/chosen`'s end, strings block with the new names appended) in a static
//! buffer; the bootloader's copy is not touched. A tree that does not fit keeps its original.

use bsd::machine::BootFramebuffer;

/// `FDT_MAGIC`.
const FDT_MAGIC: u32 = 0xd00d_feed;
/// `FDT_NODE_BEGIN`.
const FDT_NODE_BEGIN: u32 = 1;
/// `FDT_NODE_END`.
const FDT_NODE_END: u32 = 2;
/// `FDT_PROPERTY`.
const FDT_PROPERTY: u32 = 3;
/// `FDT_NOP`.
const FDT_NOP: u32 = 4;
/// `FDT_END`.
const FDT_END: u32 = 9;
/// The header's size (version 17).
const HEADER: usize = 40;

/// Big-endian word at byte `off`, `None` past the end.
fn be32(b: &[u8], off: usize) -> Option<u32> {
    let w = b.get(off..off + 4)?;
    Some(u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
}

/// `off` rounded up to a word.
const fn align4(off: usize) -> usize {
    (off + 3) & !3
}

/// The NUL-terminated string at `off`, without its NUL.
fn cstr(b: &[u8], off: usize) -> Option<&[u8]> {
    let s = b.get(off..)?;
    let n = s.iter().position(|&c| c == 0)?;
    Some(&s[..n])
}

/// Whether a `compatible` value (NUL-separated strings) lists `name`.
fn lists(value: &[u8], name: &[u8]) -> bool {
    value.split(|&c| c == 0).any(|s| s == name)
}

/// The pixel format efiboot names for a GOP mode, and its bytes per pixel.
fn format(fb: &BootFramebuffer) -> Option<&'static [u8]> {
    match (fb.bpp, fb.red_shift, fb.green_shift, fb.blue_shift) {
        // PixelRedGreenBlueReserved8BitPerColor
        (32, 0, 8, 16) => Some(b"x8b8g8r8"),
        // PixelBlueGreenRedReserved8BitPerColor
        (32, 16, 8, 0) => Some(b"x8r8g8b8"),
        // PixelBitMask 0xf800/0x07e0/0x001f
        (16, 11, 5, 0) if (fb.red_size, fb.green_size, fb.blue_size) == (5, 6, 5) => {
            Some(b"r5g6b5")
        }
        _ => None,
    }
}

/// What a walk of the structure block found.
struct Walk {
    /// Offset in the structure block of `/chosen`'s `FDT_NODE_END`.
    chosen_end: Option<usize>,
    /// The root's `#address-cells` and `#size-cells`.
    cells: (u32, u32),
    /// A usable `simple-framebuffer` child of `/` or `/chosen` exists.
    has_fb: bool,
}

/// Walks the structure block `st` with the strings block `strs`.
fn walk(st: &[u8], strs: &[u8]) -> Option<Walk> {
    let mut w = Walk {
        chosen_end: None,
        cells: (1, 1),
        has_fb: false,
    };
    let mut off = 0;
    let mut depth = 0usize;
    // Per depth: in /chosen (or at depth 2, a child of /), and the node's compatible/status.
    let mut in_chosen = false;
    let (mut compat_fb, mut status_ok) = (false, true);

    loop {
        let tok = be32(st, off)?;
        off += 4;
        match tok {
            FDT_NODE_BEGIN => {
                let name = cstr(st, off)?;
                off = align4(off + name.len() + 1);
                depth += 1;
                if depth == 2 && (name == b"chosen" || name.starts_with(b"chosen@")) {
                    in_chosen = true;
                }
                compat_fb = false;
                status_ok = true;
            }
            FDT_NODE_END => {
                // A child of / (depth 2) or of /chosen (depth 3) that is a frame buffer.
                if (depth == 2 || (depth == 3 && in_chosen)) && compat_fb && status_ok {
                    w.has_fb = true;
                }
                if depth == 2 && in_chosen {
                    w.chosen_end = Some(off - 4);
                    in_chosen = false;
                }
                compat_fb = false;
                depth = depth.checked_sub(1)?;
            }
            FDT_PROPERTY => {
                let len = be32(st, off)? as usize;
                let nameoff = be32(st, off + 4)? as usize;
                let value = st.get(off + 8..off + 8 + len)?;
                off = align4(off + 8 + len);
                let name = cstr(strs, nameoff)?;
                let int = || {
                    (len == 4).then(|| u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
                };
                if depth == 1 && name == b"#address-cells" {
                    w.cells.0 = int().unwrap_or(1);
                } else if depth == 1 && name == b"#size-cells" {
                    w.cells.1 = int().unwrap_or(1);
                } else if name == b"compatible" && lists(value, b"simple-framebuffer") {
                    compat_fb = true;
                } else if name == b"status" {
                    status_ok = cstr(value, 0) == Some(b"okay");
                }
            }
            FDT_NOP => {}
            FDT_END => return Some(w),
            _ => return None,
        }
    }
}

/// Appends `bytes` at `*at` in `out`.
fn put(out: &mut [u8], at: &mut usize, bytes: &[u8]) -> Option<()> {
    out.get_mut(*at..*at + bytes.len())?.copy_from_slice(bytes);
    *at += bytes.len();
    Some(())
}

/// Builds in `out` the tree `dtb` with a `/chosen/framebuffer` node for `fb`; returns the
/// new tree's size, `None` when nothing is to be added (or it does not fit).
pub fn add_framebuffer(dtb: &[u8], fb: &BootFramebuffer, out: &mut [u8]) -> Option<usize> {
    if be32(dtb, 0)? != FDT_MAGIC || be32(dtb, 20)? < 16 {
        return None;
    }
    let off_st = be32(dtb, 8)? as usize;
    let off_strs = be32(dtb, 12)? as usize;
    let off_rsv = be32(dtb, 16)? as usize;
    let size_strs = be32(dtb, 32)? as usize;
    let size_st = be32(dtb, 36)? as usize;
    let st = dtb.get(off_st..off_st + size_st)?;
    let strs = dtb.get(off_strs..off_strs + size_strs)?;

    let w = walk(st, strs)?;
    // Don't create a "simple-framebuffer" node if we already have one.
    if w.has_fb {
        return None;
    }
    let chosen_end = w.chosen_end?;
    let format = format(fb)?;
    let (acells, scells) = w.cells;
    if acells > 2 || scells > 2 {
        return None;
    }

    // The reservation map: 16-byte entries up to an all-zero one.
    let mut rsv_len = 0;
    loop {
        let e = dtb.get(off_rsv + rsv_len..off_rsv + rsv_len + 16)?;
        rsv_len += 16;
        if e.iter().all(|&b| b == 0) {
            break;
        }
    }

    // The new names, appended to the strings block.
    let names: [&[u8]; 7] = [
        b"status",
        b"format",
        b"stride",
        b"height",
        b"width",
        b"reg",
        b"compatible",
    ];
    let mut nameoffs = [0u32; 7];
    let mut new_strs = size_strs;
    for (i, n) in names.iter().enumerate() {
        nameoffs[i] = new_strs as u32;
        new_strs += n.len() + 1;
    }

    // reg: the address and the size in the root's cells.
    let base = fb.paddr.as_usize() as u64;
    let size = fb.size() as u64;
    let mut reg = [0u8; 16];
    let mut rl = 0;
    for (v, cells) in [(base, acells), (size, scells)] {
        if cells == 2 {
            reg[rl..rl + 8].copy_from_slice(&v.to_be_bytes());
            rl += 8;
        } else if cells == 1 {
            reg[rl..rl + 4].copy_from_slice(&(v as u32).to_be_bytes());
            rl += 4;
        }
    }

    let mut node = [0u8; 256];
    let mut nl = 0;
    {
        let n = &mut node;
        let at = &mut nl;
        put(n, at, &FDT_NODE_BEGIN.to_be_bytes())?;
        put(n, at, b"framebuffer\0")?; // 12 bytes, already aligned
        let prop = |n: &mut [u8; 256], at: &mut usize, i: usize, v: &[u8]| -> Option<()> {
            put(n, at, &FDT_PROPERTY.to_be_bytes())?;
            put(n, at, &(v.len() as u32).to_be_bytes())?;
            put(n, at, &nameoffs[i].to_be_bytes())?;
            put(n, at, v)?;
            *at = align4(*at);
            Some(())
        };
        let mut fmt = [0u8; 16];
        fmt[..format.len()].copy_from_slice(format);
        prop(n, at, 0, b"okay\0")?;
        prop(n, at, 1, &fmt[..format.len() + 1])?;
        prop(n, at, 2, &fb.pitch.to_be_bytes())?;
        prop(n, at, 3, &fb.height.to_be_bytes())?;
        prop(n, at, 4, &fb.width.to_be_bytes())?;
        prop(n, at, 5, &reg[..rl])?;
        prop(n, at, 6, b"simple-framebuffer\0")?;
        put(n, at, &FDT_NODE_END.to_be_bytes())?;
    }

    // header | reservation map | structure | strings
    let new_off_rsv = HEADER;
    let new_off_st = new_off_rsv + rsv_len;
    let new_size_st = size_st + nl;
    let new_off_strs = new_off_st + new_size_st;
    let total = new_off_strs + new_strs;
    if total > out.len() {
        return None;
    }

    let mut at = 0;
    put(out, &mut at, dtb.get(..HEADER)?)?;
    put(out, &mut at, dtb.get(off_rsv..off_rsv + rsv_len)?)?;
    put(out, &mut at, &st[..chosen_end])?;
    put(out, &mut at, &node[..nl])?;
    put(out, &mut at, &st[chosen_end..])?;
    put(out, &mut at, strs)?;
    for n in names {
        put(out, &mut at, n)?;
        put(out, &mut at, &[0])?;
    }

    for (off, v) in [
        (4, total),
        (8, new_off_st),
        (12, new_off_strs),
        (16, new_off_rsv),
        (32, new_strs),
        (36, new_size_st),
    ] {
        out[off..off + 4].copy_from_slice(&(v as u32).to_be_bytes());
    }

    Some(total)
}
/* </CODE> */
