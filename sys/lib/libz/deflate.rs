/* <LICENSES> */
/* deflate.c -- compress data using the deflation algorithm
 * Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* deflate.h -- internal compression state
 * Copyright (C) 1995-2026 Jean-loup Gailly
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* zlib.h -- interface of the 'zlib' general purpose compression library
  version 1.3.2, February 17th, 2026

  Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler

  This software is provided 'as-is', without any express or implied
  warranty.  In no event will the authors be held liable for any damages
  arising from the use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not
     claim that you wrote the original software. If you use this software
     in a product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Jean-loup Gailly        Mark Adler
  jloup@gzip.org          madler@alumni.caltech.edu


  The data format used by the zlib library is described by RFCs (Request for
  Comments) 1950 to 1952 at https://datatracker.ietf.org/doc/html/rfc1950
  (zlib format), rfc1951 (deflate format) and rfc1952 (gzip format).
*/
/* </LICENSES> */

//! The deflate compressor: the compression state of a stream (`deflate.h`) and the LZ77 half of
//! the algorithm with the stream API (`deflate.c`).
//!
//! Upstream: sys/lib/libz/deflate.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `deflate.h`** (zlib licence,
//! clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full
//! (clause 3). Do not mistake it for the zlib distribution; bugs here are not the zlib authors'.
//!
//! [`DeflateState`] is `struct internal_state` of `deflate.h`, what `z_stream.state` points at
//! for a compression stream ([`InternalState::Deflate`](crate::zlib::InternalState)). The
//! Huffman half of the compressor (`trees.c`) works on the same state and lives in
//! `trees.rs`.
//!
//! ## Deviations
//! - The state has no `strm` back-pointer: every function that needs the stream takes it as a
//!   parameter next to the state.
//! - The buffers are `Vec`s from `zcalloc` and the pointers into them are offsets:
//!   `pending_out` is an index into `pending_buf`, and `sym_buf` is the offset of the symbol
//!   buffer inside `pending_buf` (the two still overlay, as in the C). The three dynamic trees
//!   and the heap are `Vec`s too, so the state itself stays small and is never built on the
//!   stack.
//! - `ct_data`'s two unions are two `u16` fields, [`CtData::fc`] (`Freq` or `Code`) and
//!   [`CtData::dl`] (`Dad` or `Len`).
//! - `tree_desc.dyn_tree` (a pointer to one of the state's own trees) is not stored: a [`Tree`]
//!   value says which tree a descriptor goes with, and `trees.rs` moves that tree out of the
//!   state while it works on it.
//! - `GZIP` is not defined (the kernel's zlib is built with `NO_GZIP`): `GZIP_STATE` does not
//!   exist and `gzhead`/`gzindex`, which only a gzip stream uses, are left out.
//! - `LIT_MEM` and `ZLIB_DEBUG` are not defined: the symbol buffer is `sym_buf` (three bytes a
//!   symbol, overlaid on `pending_buf`), `compressed_len`/`bits_sent` do not exist, and
//!   `_tr_tally_lit`/`_tr_tally_dist` are the inline versions (which, unlike `_tr_tally`, do not
//!   count `matches`).
//! - `match_available` and `slid` are `bool`; window positions and lengths are `usize`.

#![allow(non_snake_case)] // zlib's names (deflateInit2_, putShortMSB)
#![allow(non_upper_case_globals)] // zlib's names (Buf_size, configuration_table)

use alloc::vec::Vec;

use crate::trees::{
    _dist_code, _length_code, StaticTreeDesc, static_bl_desc, static_d_desc, static_l_desc,
};
use crate::zopenbsd::zcalloc;
use crate::zutil::{MAX_MATCH, MIN_MATCH};

/// `LENGTH_CODES`: number of length codes, not counting the special END_BLOCK code.
pub(crate) const LENGTH_CODES: usize = 29;

/// `LITERALS`: number of literal bytes 0..255.
pub(crate) const LITERALS: usize = 256;

/// `L_CODES`: number of Literal or Length codes, including the END_BLOCK code.
pub(crate) const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;

/// `D_CODES`: number of distance codes.
pub(crate) const D_CODES: usize = 30;

/// `BL_CODES`: number of codes used to transfer the bit lengths.
pub(crate) const BL_CODES: usize = 19;

/// `HEAP_SIZE`: maximum heap size.
pub(crate) const HEAP_SIZE: usize = 2 * L_CODES + 1;

/// `MAX_BITS`: all codes must not exceed MAX_BITS bits.
pub(crate) const MAX_BITS: usize = 15;

/// `Buf_size`: size of bit buffer in bi_buf.
pub(crate) const Buf_size: i32 = 16;

/// `INIT_STATE`: zlib header -> BUSY_STATE. The stream status values follow.
pub(crate) const INIT_STATE: i32 = 42;
// GZIP_STATE (57, gzip header -> BUSY_STATE | EXTRA_STATE) exists only with GZIP.
/// `EXTRA_STATE`: gzip extra block -> NAME_STATE.
pub(crate) const EXTRA_STATE: i32 = 69;
/// `NAME_STATE`: gzip file name -> COMMENT_STATE.
pub(crate) const NAME_STATE: i32 = 73;
/// `COMMENT_STATE`: gzip comment -> HCRC_STATE.
pub(crate) const COMMENT_STATE: i32 = 91;
/// `HCRC_STATE`: gzip header CRC -> BUSY_STATE.
pub(crate) const HCRC_STATE: i32 = 103;
/// `BUSY_STATE`: deflate -> FINISH_STATE.
pub(crate) const BUSY_STATE: i32 = 113;
/// `FINISH_STATE`: stream complete.
pub(crate) const FINISH_STATE: i32 = 666;

/// `LIT_BUFS`: bytes of `pending_buf` per symbol-buffer entry (4 without `LIT_MEM`).
pub(crate) const LIT_BUFS: usize = 4;

/// `MIN_LOOKAHEAD`: minimum amount of lookahead, except at the end of the input file. See
/// `deflate.rs` for comments about the MIN_MATCH+1.
pub(crate) const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;

/// `WIN_INIT`: number of bytes after end of data in window to initialize in order to avoid
/// memory checker errors from longest match routines.
pub(crate) const WIN_INIT: usize = MAX_MATCH;

/// `ct_data`: data structure describing a single value and its code string.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct CtData {
    /// `fc`: `Freq` (frequency count) while a tree is built, `Code` (bit string) after.
    pub(crate) fc: u16,
    /// `dl`: `Dad` (father node in Huffman tree) while a tree is built, `Len` (length of bit
    /// string) after.
    pub(crate) dl: u16,
}

impl CtData {
    /// A `ct_data` with `Code` (or `Freq`) `fc` and `Len` (or `Dad`) `dl`.
    pub(crate) const fn new(fc: u16, dl: u16) -> Self {
        Self { fc, dl }
    }
}

/// Which of the state's three dynamic trees a [`TreeDesc`] describes: the C's
/// `tree_desc.dyn_tree` pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tree {
    /// `dyn_ltree`, described by `l_desc`.
    L,
    /// `dyn_dtree`, described by `d_desc`.
    D,
    /// `bl_tree`, described by `bl_desc`.
    Bl,
}

/// `tree_desc`: a dynamic tree and its static counterpart.
#[derive(Clone, Copy)]
pub(crate) struct TreeDesc {
    /// `max_code`: largest code with non zero frequency.
    pub(crate) max_code: i32,
    /// `stat_desc`: the corresponding static tree.
    pub(crate) stat_desc: &'static StaticTreeDesc,
}

/// `deflate_state` (`struct internal_state` of `deflate.h`): the state of a compression
/// stream.
pub(crate) struct DeflateState {
    /// `status`: as the name implies.
    pub(crate) status: i32,
    /// `pending_buf`: output still pending.
    pub(crate) pending_buf: Vec<u8>,
    /// `pending_buf_size`: size of pending_buf.
    pub(crate) pending_buf_size: usize,
    /// `pending_out`: index in `pending_buf` of the next pending byte to output to the stream.
    pub(crate) pending_out: usize,
    /// `pending`: nb of bytes in the pending buffer.
    pub(crate) pending: usize,
    /// `wrap`: bit 0 true for zlib, bit 1 true for gzip; negative once the trailer is written.
    pub(crate) wrap: i32,
    /// `method`: can only be DEFLATED.
    pub(crate) method: u8,
    /// `last_flush`: value of flush param for previous deflate call.
    pub(crate) last_flush: i32,

    // used by deflate.rs:
    /// `w_size`: LZ77 window size (32K by default).
    pub(crate) w_size: usize,
    /// `w_bits`: log2(w_size) (8..16).
    pub(crate) w_bits: usize,
    /// `w_mask`: w_size - 1.
    pub(crate) w_mask: usize,
    /// `window`: sliding window. Input bytes are read into the second half of the window, and
    /// move to the first half later to keep a dictionary of at least wSize bytes. With this
    /// organization, matches are limited to a distance of wSize-MAX_MATCH bytes, but this
    /// ensures that IO is always performed with a length multiple of the block size.
    pub(crate) window: Vec<u8>,
    /// `window_size`: actual size of window: 2*wSize.
    pub(crate) window_size: usize,
    /// `prev`: link to older string with same hash index. To limit the size of this array to
    /// 64K, this link is maintained only for the last 32K strings. An index in this array is
    /// thus a window index modulo 32K.
    pub(crate) prev: Vec<u16>,
    /// `head`: heads of the hash chains or NIL.
    pub(crate) head: Vec<u16>,
    /// `ins_h`: hash index of string to be inserted.
    pub(crate) ins_h: usize,
    /// `hash_size`: number of elements in hash table.
    pub(crate) hash_size: usize,
    /// `hash_bits`: log2(hash_size).
    pub(crate) hash_bits: usize,
    /// `hash_mask`: hash_size-1.
    pub(crate) hash_mask: usize,
    /// `hash_shift`: number of bits by which ins_h must be shifted at each input step. It must
    /// be such that after MIN_MATCH steps, the oldest byte no longer takes part in the hash
    /// key, that is: hash_shift * MIN_MATCH >= hash_bits.
    pub(crate) hash_shift: usize,
    /// `block_start`: window position at the beginning of the current output block. Gets
    /// negative when the window is moved backwards.
    pub(crate) block_start: i64,
    /// `match_length`: length of best match.
    pub(crate) match_length: usize,
    /// `prev_match`: previous match.
    pub(crate) prev_match: usize,
    /// `match_available`: set if previous match exists.
    pub(crate) match_available: bool,
    /// `strstart`: start of string to insert.
    pub(crate) strstart: usize,
    /// `match_start`: start of matching string.
    pub(crate) match_start: usize,
    /// `lookahead`: number of valid bytes ahead in window.
    pub(crate) lookahead: usize,
    /// `prev_length`: length of the best match at previous step. Matches not greater than this
    /// are discarded. This is used in the lazy match evaluation.
    pub(crate) prev_length: usize,
    /// `max_chain_length`: to speed up deflation, hash chains are never searched beyond this
    /// length. A higher limit improves compression ratio but degrades the speed.
    pub(crate) max_chain_length: u32,
    /// `max_lazy_match`: attempt to find a better match only when the current match is
    /// strictly smaller than this value. This mechanism is used only for compression levels 4
    /// and up. It is also `max_insert_length`: insert new strings in the hash table only if
    /// the match length is not greater than this length (levels 3 and below).
    pub(crate) max_lazy_match: u32,
    /// `level`: compression level (1..9).
    pub(crate) level: i32,
    /// `strategy`: favor or force Huffman coding.
    pub(crate) strategy: i32,
    /// `good_match`: use a faster search when the previous match is longer than this.
    pub(crate) good_match: u32,
    /// `nice_match`: stop searching when current match exceeds this.
    pub(crate) nice_match: i32,

    // used by trees.rs:
    /// `dyn_ltree`: literal and length tree (`HEAP_SIZE` entries).
    pub(crate) dyn_ltree: Vec<CtData>,
    /// `dyn_dtree`: distance tree (`2*D_CODES+1` entries).
    pub(crate) dyn_dtree: Vec<CtData>,
    /// `bl_tree`: Huffman tree for bit lengths (`2*BL_CODES+1` entries).
    pub(crate) bl_tree: Vec<CtData>,
    /// `l_desc`: desc. for literal tree.
    pub(crate) l_desc: TreeDesc,
    /// `d_desc`: desc. for distance tree.
    pub(crate) d_desc: TreeDesc,
    /// `bl_desc`: desc. for bit length tree.
    pub(crate) bl_desc: TreeDesc,
    /// `bl_count`: number of codes at each bit length for an optimal tree.
    pub(crate) bl_count: [u16; MAX_BITS + 1],
    /// `heap`: heap used to build the Huffman trees (`2*L_CODES+1` entries). The sons of
    /// heap[n] are heap[2*n] and heap[2*n+1]. heap[0] is not used. The same heap array is used
    /// to build all trees.
    pub(crate) heap: Vec<usize>,
    /// `heap_len`: number of elements in the heap.
    pub(crate) heap_len: usize,
    /// `heap_max`: element of largest frequency.
    pub(crate) heap_max: usize,
    /// `depth`: depth of each subtree used as tie breaker for trees of equal frequency.
    pub(crate) depth: [u8; 2 * L_CODES + 1],
    /// `sym_buf`: offset in `pending_buf` of the buffer for distances and literals/lengths.
    pub(crate) sym_buf: usize,
    /// `lit_bufsize`: size of match buffer for literals/lengths. There are 4 reasons for
    /// limiting lit_bufsize to 64K: frequencies can be kept in 16 bit counters; if compression
    /// is not successful for the first block, all input data is still in the window so a
    /// stored block can still be emitted; a stored file instead of a stored block saves 5
    /// bytes for small zip files; and creating new Huffman trees less frequently may not
    /// adapt fast to changes in the input data statistics.
    pub(crate) lit_bufsize: usize,
    /// `sym_next`: running index in symbol buffer.
    pub(crate) sym_next: usize,
    /// `sym_end`: symbol table full when sym_next reaches this.
    pub(crate) sym_end: usize,
    /// `opt_len`: bit length of current block with optimal trees.
    pub(crate) opt_len: u64,
    /// `static_len`: bit length of current block with static trees.
    pub(crate) static_len: u64,
    /// `matches`: number of string matches in current block (by `deflate_stored`, the number
    /// of hash table slides still to do).
    pub(crate) matches: u32,
    /// `insert`: bytes at end of window left to insert.
    pub(crate) insert: usize,
    /// `bi_buf`: output buffer. Bits are inserted starting at the bottom (least significant
    /// bits).
    pub(crate) bi_buf: u16,
    /// `bi_valid`: number of valid bits in bi_buf. All bits above the last valid bit are
    /// always zero.
    pub(crate) bi_valid: i32,
    /// `bi_used`: last number of used bits when going to a byte boundary.
    pub(crate) bi_used: i32,
    /// `high_water`: high water mark offset in window for initialized bytes -- bytes above
    /// this are set to zero in order to avoid memory check warnings when longest match
    /// routines access bytes past the input. This is then updated to the new high water mark.
    pub(crate) high_water: usize,
    /// `slid`: true if the hash table has been slid since it was cleared.
    pub(crate) slid: bool,
}

impl DeflateState {
    /// The allocations of `deflateInit2_` (and `deflateCopy`): a zeroed state (`zmemzero`)
    /// with a window of `2 * w_size` bytes, `prev` of `w_size` and `head` of `hash_size`
    /// entries, a `pending_buf` of `lit_bufsize * LIT_BUFS` bytes with the symbol buffer at
    /// `lit_bufsize`, and the trees and heap. `None` when any allocation fails (the C's
    /// `Z_MEM_ERROR` paths); what was allocated is freed.
    pub(crate) fn alloc(w_bits: usize, hash_bits: usize, lit_bufsize: usize) -> Option<Self> {
        let w_size = 1 << w_bits;
        let hash_size = 1 << hash_bits;
        Some(Self {
            status: 0,
            pending_buf: zcalloc(lit_bufsize * LIT_BUFS)?,
            pending_buf_size: lit_bufsize * 4,
            pending_out: 0,
            pending: 0,
            wrap: 0,
            method: 0,
            last_flush: 0,
            w_size,
            w_bits,
            w_mask: w_size - 1,
            window: zcalloc(w_size * 2)?,
            window_size: 0,
            prev: zcalloc(w_size)?,
            head: zcalloc(hash_size)?,
            ins_h: 0,
            hash_size,
            hash_bits,
            hash_mask: hash_size - 1,
            hash_shift: hash_bits.div_ceil(MIN_MATCH), // (hash_bits+MIN_MATCH-1)/MIN_MATCH
            block_start: 0,
            match_length: 0,
            prev_match: 0,
            match_available: false,
            strstart: 0,
            match_start: 0,
            lookahead: 0,
            prev_length: 0,
            max_chain_length: 0,
            max_lazy_match: 0,
            level: 0,
            strategy: 0,
            good_match: 0,
            nice_match: 0,
            dyn_ltree: zcalloc(HEAP_SIZE)?,
            dyn_dtree: zcalloc(2 * D_CODES + 1)?,
            bl_tree: zcalloc(2 * BL_CODES + 1)?,
            l_desc: TreeDesc {
                max_code: 0,
                stat_desc: &static_l_desc,
            },
            d_desc: TreeDesc {
                max_code: 0,
                stat_desc: &static_d_desc,
            },
            bl_desc: TreeDesc {
                max_code: 0,
                stat_desc: &static_bl_desc,
            },
            bl_count: [0; MAX_BITS + 1],
            heap: zcalloc(2 * L_CODES + 1)?,
            heap_len: 0,
            heap_max: 0,
            depth: [0; 2 * L_CODES + 1],
            sym_buf: lit_bufsize,
            lit_bufsize,
            sym_next: 0,
            sym_end: (lit_bufsize - 1) * 3,
            opt_len: 0,
            static_len: 0,
            matches: 0,
            insert: 0,
            bi_buf: 0,
            bi_valid: 0,
            bi_used: 0,
            high_water: 0,
            slid: false,
        })
    }

    /// The descriptor of tree `t` (`&s->l_desc`, `&s->d_desc`, `&s->bl_desc`).
    pub(crate) fn desc(&mut self, t: Tree) -> &mut TreeDesc {
        match t {
            Tree::L => &mut self.l_desc,
            Tree::D => &mut self.d_desc,
            Tree::Bl => &mut self.bl_desc,
        }
    }

    /// The dynamic tree `t` (what `desc->dyn_tree` points at).
    pub(crate) fn dyn_tree(&mut self, t: Tree) -> &mut Vec<CtData> {
        match t {
            Tree::L => &mut self.dyn_ltree,
            Tree::D => &mut self.dyn_dtree,
            Tree::Bl => &mut self.bl_tree,
        }
    }
}

/// `put_byte`: output a byte on the stream. IN assertion: there is enough room in
/// pending_buf.
pub(crate) fn put_byte(s: &mut DeflateState, c: u8) {
    s.pending_buf[s.pending] = c;
    s.pending += 1;
}

/// `MAX_DIST(s)`: in order to simplify the code, match distances are limited to MAX_DIST
/// instead of WSIZE.
pub(crate) fn MAX_DIST(s: &DeflateState) -> usize {
    s.w_size - MIN_LOOKAHEAD
}

/// `d_code(dist)`: mapping from a distance to a distance code. `dist` is the distance - 1.
/// `_dist_code[256]` and `_dist_code[257]` are never used.
pub(crate) fn d_code(dist: usize) -> usize {
    if dist < 256 {
        usize::from(_dist_code[dist])
    } else {
        usize::from(_dist_code[256 + (dist >> 7)])
    }
}

/// `_tr_tally_lit(s, c, flush)`: the inline version of `_tr_tally` for a literal. Returns
/// `flush`: true if the current block must be flushed.
pub(crate) fn _tr_tally_lit(s: &mut DeflateState, c: u8) -> bool {
    let sb = s.sym_buf;
    s.pending_buf[sb + s.sym_next] = 0;
    s.pending_buf[sb + s.sym_next + 1] = 0;
    s.pending_buf[sb + s.sym_next + 2] = c;
    s.sym_next += 3;
    s.dyn_ltree[usize::from(c)].fc += 1;
    s.sym_next == s.sym_end
}

/// `_tr_tally_dist(s, distance, length, flush)`: the inline version of `_tr_tally` for a
/// match of `length - MIN_MATCH` at `distance`. Returns `flush`. Unlike `_tr_tally` it does
/// not count `matches`.
pub(crate) fn _tr_tally_dist(s: &mut DeflateState, distance: usize, length: usize) -> bool {
    let len = length as u8;
    let dist = distance as u16;
    let sb = s.sym_buf;
    s.pending_buf[sb + s.sym_next] = dist as u8;
    s.pending_buf[sb + s.sym_next + 1] = (dist >> 8) as u8;
    s.pending_buf[sb + s.sym_next + 2] = len;
    s.sym_next += 3;
    let dist = usize::from(dist.wrapping_sub(1));
    s.dyn_ltree[usize::from(_length_code[usize::from(len)]) + LITERALS + 1].fc += 1;
    s.dyn_dtree[d_code(dist)].fc += 1;
    s.sym_next == s.sym_end
}
