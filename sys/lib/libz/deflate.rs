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

/* <CODE> */
//! The deflate compressor: the compression state of a stream (`deflate.h`) and the LZ77 half of
//! the algorithm with the stream API (`deflate.c`): `deflateInit2_`, `deflate`, `deflateEnd`
//! and the rest of zlib's compression functions.
//!
//! Upstream: sys/lib/libz/deflate.c @ 3ce1f3f79392, sys/lib/libz/deflate.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `deflate.c`/`deflate.h`** (zlib
//! licence, clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above
//! in full (clause 3). Do not mistake it for the zlib distribution; bugs here are not the zlib
//! authors'.
//!
//! The algorithm: the deflation process depends on being able to identify portions of the
//! input text which are identical to earlier input (within a sliding window trailing behind
//! the input currently being processed). The most straightforward technique turns out to be
//! the fastest for most input files: try all possible matches and select the longest. The key
//! feature is that insertions into the string dictionary are very simple and thus fast, and
//! deletions are avoided completely. Insertions are performed at each input character, whereas
//! string matches are performed only when the previous match ends. So it is preferable to
//! spend more time in matches to allow very fast string insertions and avoid deletions. The
//! matching algorithm for small strings is inspired from that of Rabin & Karp; a brute force
//! approach is used to find longer strings when a small match has been found (as in comic by
//! Jan-Mark Wams and freeze by Leonid Broukhis). The idea of lazy evaluation of matches is due
//! to Jan-Mark Wams. A previous version of zlib used the Fiala and Greene algorithm, linear
//! but slower on average and patented. References: Deutsch, "DEFLATE Compressed Data Format
//! Specification" (RFC 1951); Sedgewick, "Algorithms", p252 (Rabin-Karp); Fiala and Greene,
//! "Data Compression with Finite Windows", Comm. ACM 32,4 (1989) 490-595.
//!
//! `DeflateState` is `struct internal_state` of `deflate.h`, what `z_stream.state` points at
//! for a compression stream (`InternalState::Deflate`). The Huffman half of the compressor
//! (`trees.c`) works on the same state and lives in `trees.rs`. A public function takes the
//! state out of the stream for the duration of the call (`with_state`), so that the stream
//! and the state can be borrowed apart, and puts it back on every path.
//!
//! The kernel's caller is IPComp (`xform_ipcomp.c`): `deflateInit2(strm, Z_DEFAULT_COMPRESSION,
//! Z_DEFLATED, -12, 8, Z_DEFAULT_STRATEGY)` (a raw stream with a 4 KiB window), then
//! `deflate(strm, Z_FINISH)` with fresh output buffers until `Z_STREAM_END`, then `deflateEnd`.
//!
//! ## Deviations
//! - The state has no `strm` back-pointer: every function that needs the stream takes it as a
//!   parameter next to the state. `deflateStateCheck` checks that the stream's state is a
//!   deflate state with a valid status; `zalloc`/`zfree` are always `zcalloc`/`zcfree`
//!   (`zopenbsd.rs`), so their checks and `Z_SOLO` have no counterpart.
//! - The buffers are `Vec`s from `zcalloc` and the pointers into them are offsets:
//!   `pending_out` is an index into `pending_buf`, and `sym_buf` is the offset of the symbol
//!   buffer inside `pending_buf` (the two still overlay, as in the C). The three dynamic trees
//!   and the heap are `Vec`s too, so the state itself stays small and is never built on the
//!   stack. The buffers are zero-filled; the C leaves them uninitialised, which only matters
//!   to memory checkers (the reason for `high_water`, kept as is).
//! - `ct_data`'s two unions are two `u16` fields, `CtData::fc` (`Freq` or `Code`) and
//!   `CtData::dl` (`Dad` or `Len`).
//! - `tree_desc.dyn_tree` (a pointer to one of the state's own trees) is not stored: a `Tree`
//!   value says which tree a descriptor goes with, and `trees.rs` moves that tree out of the
//!   state while it works on it.
//! - `config.func` is a `CompressFunc` value instead of a function pointer, so that
//!   `deflateParams` compares two of them with `==`; `deflate_stored`, `deflate_fast` and
//!   `deflate_slow` are called through a `match`.
//! - The input and output pointers are the stream's slices. `deflate_stored` remembers the
//!   input slice it started with to reach the bytes it copied straight to `next_out` (the C
//!   reads them back at `next_in - used`). `deflateSetDictionary` feeds the dictionary through
//!   a temporary stream instead of pointing `next_in` at it; `total_in` still counts it, as in
//!   the C.
//! - `deflate()`'s `next_out == Z_NULL` and `next_in == Z_NULL` checks have no counterpart: a
//!   slice is never null.
//! - `GZIP` is not defined (the kernel's zlib is built with `NO_GZIP`): `GZIP_STATE`, the
//!   gzip header states of `deflate()` (`EXTRA_STATE` .. `HCRC_STATE` are still accepted by
//!   `deflateStateCheck`, as in the C, but nothing enters them), the gzip trailer, the
//!   `windowBits > 15` gzip request, `HCRC_UPDATE`, the CRC-32 in `read_buf` and
//!   `gzhead`/`gzindex` are left out. `deflateSetHeader` refuses every stream, as the C does
//!   when `wrap` cannot be 2.
//! - `LIT_MEM` and `ZLIB_DEBUG` are not defined: the symbol buffer is `sym_buf` (three bytes a
//!   symbol, overlaid on `pending_buf`), `compressed_len`/`bits_sent` and `check_match` do not
//!   exist, and `_tr_tally_lit`/`_tr_tally_dist` are the inline versions (which, unlike
//!   `_tr_tally`, do not count `matches`). `FASTEST` (level 1 only, no hash chains) and
//!   `UNALIGNED_OK` (two-byte compares in `longest_match`) are not defined either: only their
//!   `#else` sides are ported. The `sizeof(int) <= 2` path of `fill_window` and `MAXSEG_64K`
//!   are for 16-bit targets and are left out.
//! - `deflateCopy` copies `next_in` but gives the copy an empty `next_out`: two streams cannot
//!   hold the same `&mut` output buffer.
//! - `deflateGetDictionary` returns `Z_BUF_ERROR` when `dictionary` is shorter than the
//!   history it would get (the C writes past the caller's buffer).
//! - `deflatePending`, `deflateUsed` and `deflateGetDictionary` take `Option<&mut _>` for
//!   their optional output parameters; `deflateBound` and `deflateBound_z` are the same
//!   function on LP64 (`uLong` and `z_size_t` are both 64 bits).
//! - OpenBSD's copy has no `deflate_copyright` string (it keeps only the comment asking for
//!   an acknowledgement), so neither has this file.
//! - `match_available` and `slid` are `bool`; window positions and lengths are `usize`, and
//!   the C's unsigned differences that may wrap (`strstart - hash_head`, `strstart -
//!   match_start`) use `wrapping_sub`.

#![allow(non_snake_case)] // zlib's names (deflateInit2_, putShortMSB)
#![allow(non_upper_case_globals)] // zlib's names (Buf_size, configuration_table)

use alloc::vec::Vec;

use crate::adler32::adler32;
use crate::trees::{
    _dist_code, _length_code, _tr_align, _tr_flush_bits, _tr_flush_block, _tr_init,
    _tr_stored_block, StaticTreeDesc, static_bl_desc, static_d_desc, static_l_desc,
};
use crate::zconf::{MAX_MEM_LEVEL, MAX_WBITS};
use crate::zlib::{
    GzHeader, InternalState, Z_BLOCK, Z_BUF_ERROR, Z_DATA_ERROR, Z_DEFAULT_COMPRESSION,
    Z_DEFAULT_STRATEGY, Z_DEFLATED, Z_FILTERED, Z_FINISH, Z_FIXED, Z_FULL_FLUSH, Z_HUFFMAN_ONLY,
    Z_MEM_ERROR, Z_NO_FLUSH, Z_OK, Z_PARTIAL_FLUSH, Z_RLE, Z_STREAM_END, Z_STREAM_ERROR, Z_UNKNOWN,
    Z_VERSION_ERROR, ZLIB_VERSION, ZStream,
};
use crate::zopenbsd::{zcalloc, zcalloc_box, zcfree};
use crate::zutil::{
    DEF_MEM_LEVEL, ERR_MSG, ERR_RETURN, MAX_MATCH, MIN_MATCH, PRESET_DICT, zassert,
};

/// `NIL`: tail of hash chains.
const NIL: usize = 0;

/// `TOO_FAR`: matches of length 3 are discarded if their distance exceeds TOO_FAR.
const TOO_FAR: usize = 4096;

/// `MAX_STORED`: maximum stored block length in deflate format (not including header).
const MAX_STORED: usize = 65535;

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
    /// `heap[n]` are `heap[2*n]` and `heap[2*n+1]`. `heap[0]` is not used. The same heap array is used
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

/// `block_state`: what a compression function did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlockState {
    /// `need_more`: block not completed, need more input or more output.
    NeedMore,
    /// `block_done`: block flush performed.
    BlockDone,
    /// `finish_started`: finish started, need only more output at next deflate.
    FinishStarted,
    /// `finish_done`: finish done, accept no more input or output.
    FinishDone,
}

/// `compress_func`: the compression function of a level (a function pointer in the C).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CompressFunc {
    /// `deflate_stored`.
    Stored,
    /// `deflate_fast`.
    Fast,
    /// `deflate_slow`.
    Slow,
}

/// `config`: the parameters of a compression level.
struct Config {
    /// `good_length`: reduce lazy search above this match length.
    good_length: u16,
    /// `max_lazy`: do not perform lazy search above this match length.
    max_lazy: u16,
    /// `nice_length`: quit search above this match length.
    nice_length: u16,
    /// `max_chain`.
    max_chain: u16,
    /// `func`.
    func: CompressFunc,
}

impl Config {
    const fn new(
        good_length: u16,
        max_lazy: u16,
        nice_length: u16,
        max_chain: u16,
        func: CompressFunc,
    ) -> Self {
        Self {
            good_length,
            max_lazy,
            nice_length,
            max_chain,
            func,
        }
    }
}

/// `configuration_table`: values for max_lazy_match, good_match and max_chain_length,
/// depending on the desired pack level (0..9). The values given below have been tuned to
/// exclude worst case performance for pathological files. Better values may be found for
/// specific files.
///
/// Note: the deflate() code requires max_lazy >= MIN_MATCH and max_chain >= 4. For
/// deflate_fast() (levels <= 3) good is ignored and lazy has a different meaning.
static configuration_table: [Config; 10] = [
    //          good lazy nice chain
    Config::new(0, 0, 0, 0, CompressFunc::Stored), // 0: store only
    Config::new(4, 4, 8, 4, CompressFunc::Fast),   // 1: max speed, no lazy matches
    Config::new(4, 5, 16, 8, CompressFunc::Fast),  // 2
    Config::new(4, 6, 32, 32, CompressFunc::Fast), // 3
    Config::new(4, 4, 16, 16, CompressFunc::Slow), // 4: lazy matches
    Config::new(8, 16, 32, 32, CompressFunc::Slow), // 5
    Config::new(8, 16, 128, 128, CompressFunc::Slow), // 6
    Config::new(8, 32, 128, 256, CompressFunc::Slow), // 7
    Config::new(32, 128, 258, 1024, CompressFunc::Slow), // 8
    Config::new(32, 258, 258, 4096, CompressFunc::Slow), // 9: max compression
];

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

/// `RANK(f)`: rank Z_BLOCK between Z_NO_FLUSH and Z_PARTIAL_FLUSH.
const fn RANK(f: i32) -> i32 {
    (f * 2) - if f > 4 { 9 } else { 0 }
}

/// `UPDATE_HASH(s, h, c)`: update a hash value with the given input byte. IN assertion: all
/// calls to UPDATE_HASH are made with consecutive input characters, so that a running hash
/// key can be computed from the previous key instead of complete recalculation each time.
fn UPDATE_HASH(s: &DeflateState, h: usize, c: u8) -> usize {
    ((h << s.hash_shift) ^ usize::from(c)) & s.hash_mask
}

/// `INSERT_STRING(s, str, match_head)`: insert string `str` in the dictionary and return the
/// previous head of the hash chain (the most recent string with same hash key). IN assertion:
/// all calls to INSERT_STRING are made with consecutive input characters and the first
/// MIN_MATCH bytes of str are valid (except for the last MIN_MATCH-1 bytes of the input file).
fn INSERT_STRING(s: &mut DeflateState, str: usize) -> usize {
    s.ins_h = UPDATE_HASH(s, s.ins_h, s.window[str + (MIN_MATCH - 1)]);
    let match_head = s.head[s.ins_h];
    s.prev[str & s.w_mask] = match_head;
    s.head[s.ins_h] = str as u16;
    usize::from(match_head)
}

/// `CLEAR_HASH(s)`: initialize the hash table. prev[] will be initialized on the fly.
fn CLEAR_HASH(s: &mut DeflateState) {
    s.head[..s.hash_size].fill(NIL as u16);
    s.slid = false;
}

/// `slide_hash`: slide the hash table when sliding the window down (could be avoided with 32
/// bit values at the expense of memory usage). We slide even when level == 0 to keep the hash
/// table consistent if we switch back to level > 0 later.
fn slide_hash(s: &mut DeflateState) {
    let wsize = s.w_size;
    for p in s.head[..s.hash_size].iter_mut() {
        let m = usize::from(*p);
        *p = if m >= wsize {
            (m - wsize) as u16
        } else {
            NIL as u16
        };
    }
    for p in s.prev[..wsize].iter_mut() {
        let m = usize::from(*p);
        *p = if m >= wsize {
            (m - wsize) as u16
        } else {
            NIL as u16
        };
        // If n is not on any hash chain, prev[n] is garbage but its value will never be used.
    }
    s.slid = true;
}

/// `read_buf`: read a new buffer from the current input stream, update the adler32 (when
/// `wrap` is 1) and total number of bytes read. All deflate() input goes through this
/// function. Returns the number of bytes read, at most `buf.len()`.
fn read_buf(strm: &mut ZStream<'_>, wrap: i32, buf: &mut [u8]) -> usize {
    let len = strm.avail_in().min(buf.len());
    if len == 0 {
        return 0;
    }

    buf[..len].copy_from_slice(strm.take_in(len));
    if wrap == 1 {
        strm.adler = adler32(strm.adler, Some(&buf[..len]));
    }
    // wrap == 2 (CRC-32 of a gzip stream) only exists with GZIP.
    strm.total_in += len as u64;

    len
}

/// `fill_window`: fill the window when the lookahead becomes insufficient. Updates strstart
/// and lookahead.
///
/// IN assertion: lookahead < MIN_LOOKAHEAD. OUT assertions: strstart <=
/// window_size-MIN_LOOKAHEAD; at least one byte has been read, or avail_in == 0; reads are
/// performed for at least two bytes (required for the zip translate_eol option -- not
/// supported here).
fn fill_window(strm: &mut ZStream<'_>, s: &mut DeflateState) {
    let wsize = s.w_size;

    zassert!(s.lookahead < MIN_LOOKAHEAD, "already enough lookahead");

    loop {
        // Amount of free space at the end of the window.
        let mut more = s.window_size - s.lookahead - s.strstart;

        // (The C deals with the 64K limit of 16-bit ints here.)

        // If the window is almost full and there is insufficient lookahead, move the upper
        // half to the lower one to make room in the upper half.
        if s.strstart >= wsize + MAX_DIST(s) {
            s.window.copy_within(wsize..wsize + wsize - more, 0);
            s.match_start = s.match_start.wrapping_sub(wsize);
            s.strstart -= wsize; // we now have strstart >= MAX_DIST
            s.block_start -= wsize as i64;
            if s.insert > s.strstart {
                s.insert = s.strstart;
            }
            slide_hash(s);
            more += wsize;
        }
        if strm.avail_in() == 0 {
            break;
        }

        // If there was no sliding:
        //    strstart <= WSIZE+MAX_DIST-1 && lookahead <= MIN_LOOKAHEAD - 1 &&
        //    more == window_size - lookahead - strstart
        // => more >= window_size - (MIN_LOOKAHEAD-1 + WSIZE + MAX_DIST-1)
        // => more >= window_size - 2*WSIZE + 2
        // window_size == 2*WSIZE so more >= 2. If there was sliding, more >= WSIZE. So in all
        // cases, more >= 2.
        zassert!(more >= 2, "more < 2");

        let at = s.strstart + s.lookahead;
        let n = read_buf(strm, s.wrap, &mut s.window[at..at + more]);
        s.lookahead += n;

        // Initialize the hash value now that we have some input:
        if s.lookahead + s.insert >= MIN_MATCH {
            let mut str = s.strstart - s.insert;
            s.ins_h = usize::from(s.window[str]);
            s.ins_h = UPDATE_HASH(s, s.ins_h, s.window[str + 1]);
            while s.insert != 0 {
                s.ins_h = UPDATE_HASH(s, s.ins_h, s.window[str + MIN_MATCH - 1]);
                s.prev[str & s.w_mask] = s.head[s.ins_h];
                s.head[s.ins_h] = str as u16;
                str += 1;
                s.insert -= 1;
                if s.lookahead + s.insert < MIN_MATCH {
                    break;
                }
            }
        }
        // If the whole input has less than MIN_MATCH bytes, ins_h is garbage, but this is not
        // important since only literal bytes will be emitted.

        if !(s.lookahead < MIN_LOOKAHEAD && strm.avail_in() != 0) {
            break;
        }
    }

    // If the WIN_INIT bytes after the end of the current data have never been written, then
    // zero those bytes in order to avoid memory check reports of the use of uninitialized
    // bytes by the longest match routines. Update the high water mark for the next time
    // through here. WIN_INIT is set to MAX_MATCH since the longest match routines allow
    // scanning to strstart + MAX_MATCH, ignoring lookahead.
    if s.high_water < s.window_size {
        let curr = s.strstart + s.lookahead;

        if s.high_water < curr {
            // Previous high water mark below current data -- zero WIN_INIT bytes or up to end
            // of window, whichever is less.
            let init = (s.window_size - curr).min(WIN_INIT);
            s.window[curr..curr + init].fill(0);
            s.high_water = curr + init;
        } else if s.high_water < curr + WIN_INIT {
            // High water mark at or above current data, but below current data plus WIN_INIT
            // -- zero out to current data plus WIN_INIT, or up to end of window, whichever is
            // less.
            let init = (curr + WIN_INIT - s.high_water).min(s.window_size - s.high_water);
            s.window[s.high_water..s.high_water + init].fill(0);
            s.high_water += init;
        }
    }

    zassert!(
        s.strstart <= s.window_size - MIN_LOOKAHEAD,
        "not enough room for search"
    );
}

/// `deflateInit_`: [`deflateInit2_`] with the default method, window, memory level and
/// strategy.
pub fn deflateInit_(strm: &mut ZStream<'_>, level: i32, version: &str, stream_size: i32) -> i32 {
    deflateInit2_(
        strm,
        level,
        Z_DEFLATED,
        MAX_WBITS,
        DEF_MEM_LEVEL,
        Z_DEFAULT_STRATEGY,
        version,
        stream_size,
    )
    // To do: ignore strm->next_in if we use it as window
}

/// `deflateInit2_`: initialize `strm` for compression at `level` (0..9 or
/// `Z_DEFAULT_COMPRESSION`) with a window of `2^windowBits` bytes (9..15; negative for a raw
/// deflate stream without the zlib wrapper, 8 is taken as 9 with the zlib wrapper),
/// `memLevel` (1..9) and `strategy`. `version` and `stream_size` are what the caller was
/// compiled against ([`deflateInit2`](crate::zlib::deflateInit2) passes them).
#[allow(clippy::too_many_arguments)] // zlib's signature
pub fn deflateInit2_(
    strm: &mut ZStream<'_>,
    level: i32,
    method: i32,
    windowBits: i32,
    memLevel: i32,
    strategy: i32,
    version: &str,
    stream_size: i32,
) -> i32 {
    let mut level = level;
    let mut windowBits = windowBits;
    let mut wrap = 1;
    let my_version = ZLIB_VERSION;

    if version.bytes().next() != my_version.bytes().next()
        || stream_size != size_of::<ZStream<'_>>() as i32
    {
        return Z_VERSION_ERROR;
    }

    strm.msg = None;
    // zalloc and zfree are always zcalloc and zcfree.

    if level == Z_DEFAULT_COMPRESSION {
        level = 6;
    }

    if windowBits < 0 {
        // suppress zlib wrapper
        wrap = 0;
        if windowBits < -15 {
            return Z_STREAM_ERROR;
        }
        windowBits = -windowBits;
    }
    // windowBits > 15 asks for a gzip wrapper (wrap 2) only with GZIP.
    if !(1..=MAX_MEM_LEVEL).contains(&memLevel)
        || method != Z_DEFLATED
        || !(8..=15).contains(&windowBits)
        || !(0..=9).contains(&level)
        || !(0..=Z_FIXED).contains(&strategy)
        || (windowBits == 8 && wrap != 1)
    {
        return Z_STREAM_ERROR;
    }
    if windowBits == 8 {
        windowBits = 9; // until 256-byte window bug fixed
    }

    let w_bits = windowBits as usize;
    let hash_bits = memLevel as usize + 7;
    let lit_bufsize = 1 << (memLevel + 6); // 16K elements by default

    // We overlay pending_buf and sym_buf. This works since the average size for
    // length/distance pairs over any compressed block is assured to be 31 bits or less.
    //
    // Analysis: The longest fixed codes are a length code of 8 bits plus 5 extra bits, for
    // lengths 131 to 257. The longest fixed distance codes are 5 bits plus 13 extra bits, for
    // distances 16385 to 32768. The longest possible fixed-codes length/distance pair is then
    // 31 bits total.
    //
    // sym_buf starts one-fourth of the way into pending_buf. So there are three bytes in
    // sym_buf for every four bytes in pending_buf. Each symbol in sym_buf is three bytes --
    // two for the distance and one for the literal/length. As each symbol is consumed, the
    // pointer to the next sym_buf value to read moves forward three bytes. From that symbol,
    // up to 31 bits are written to pending_buf. The closest the written pending_buf bits gets
    // to the next sym_buf symbol to read is just before the last code is written. At that
    // time, 31*(n - 2) bits have been written, just after 24*(n - 2) bits have been consumed
    // from sym_buf. sym_buf starts at 8*n bits into pending_buf. (Note that the symbol buffer
    // fills when n - 1 symbols are written.) The closest the writing gets to what is unread
    // is then n + 14 bits. Here n is lit_bufsize, which is 16384 by default, and can range
    // from 128 to 32768.
    //
    // Therefore, at a minimum, there are 142 bits of space between what is written and what
    // is read in the overlain buffers, so the symbols cannot be overwritten by the compressed
    // data. That space is actually 139 bits, due to the three-bit fixed-code block header.
    //
    // That covers the case where either Z_FIXED is specified, forcing fixed codes, or when
    // the use of fixed codes is chosen, because that choice results in a smaller compressed
    // block than dynamic codes. That latter condition then assures that the above analysis
    // also covers all dynamic blocks. A dynamic-code block will only be chosen to be emitted
    // if it has fewer bits than a fixed-code block would for the same set of symbols.
    // Therefore its average symbol length is assured to be less than 31. So the compressed
    // data for a dynamic block also cannot overwrite the symbols from which it is being
    // constructed.
    //
    // (sym_end avoids equality with lit_bufsize*3 because of wraparound at 64K on 16 bit
    // machines and because stored blocks are restricted to 64K-1 bytes.)
    let Some(mut s) = DeflateState::alloc(w_bits, hash_bits, lit_bufsize) else {
        // The window, hash or pending buffer could not be allocated (the state itself is
        // allocated last here; the C allocates it first and fails without a message then).
        strm.state = InternalState::None;
        strm.msg = Some(ERR_MSG(Z_MEM_ERROR));
        return Z_MEM_ERROR;
    };
    s.status = INIT_STATE; // to pass state test in deflateReset()
    s.wrap = wrap;
    s.high_water = 0; // nothing written to s->window yet
    s.level = level;
    s.strategy = strategy;
    s.method = method as u8;

    let Some(s) = zcalloc_box(s) else {
        return Z_MEM_ERROR;
    };
    strm.state = InternalState::Deflate(s);

    deflateReset(strm)
}

/// `deflateStateCheck`: check for a valid deflate stream state. Return true (the C's 1) if
/// not ok.
fn deflateStateCheck(strm: &ZStream<'_>) -> bool {
    let InternalState::Deflate(s) = &strm.state else {
        return true;
    };
    !matches!(
        s.status,
        INIT_STATE
            | EXTRA_STATE
            | NAME_STATE
            | COMMENT_STATE
            | HCRC_STATE
            | BUSY_STATE
            | FINISH_STATE
    )
}

/// Run `f` on the stream and its deflate state, taken out of the stream for the call and put
/// back after it; `err` without calling `f` if `deflateStateCheck` fails.
fn with_state<'a, R>(
    strm: &mut ZStream<'a>,
    err: R,
    f: impl FnOnce(&mut ZStream<'a>, &mut DeflateState) -> R,
) -> R {
    if deflateStateCheck(strm) {
        return err;
    }
    match core::mem::take(&mut strm.state) {
        InternalState::Deflate(mut s) => {
            let ret = f(strm, &mut s);
            strm.state = InternalState::Deflate(s);
            ret
        }
        other => {
            strm.state = other;
            err
        }
    }
}

/// The deflate state of `strm`, if `deflateStateCheck` accepts it.
fn state_ref<'s>(strm: &'s ZStream<'_>) -> Option<&'s DeflateState> {
    match &strm.state {
        InternalState::Deflate(s) if !deflateStateCheck(strm) => Some(s),
        _ => None,
    }
}

/// The deflate state of `strm`, mutable, if `deflateStateCheck` accepts it.
fn state_mut<'s>(strm: &'s mut ZStream<'_>) -> Option<&'s mut DeflateState> {
    if deflateStateCheck(strm) {
        return None;
    }
    match &mut strm.state {
        InternalState::Deflate(s) => Some(s),
        _ => None,
    }
}

/// `deflateSetDictionary`: initialize the compression dictionary from `dictionary`. Must be
/// called immediately after `deflateInit`, `deflateInit2` or `deflateReset` (any time for a
/// raw stream, as long as no input is pending in the window).
pub fn deflateSetDictionary(strm: &mut ZStream<'_>, dictionary: &[u8]) -> i32 {
    with_state(strm, Z_STREAM_ERROR, |strm, s| {
        let wrap = s.wrap;
        if wrap == 2 || (wrap == 1 && s.status != INIT_STATE) || s.lookahead != 0 {
            return Z_STREAM_ERROR;
        }

        // when using zlib wrappers, compute Adler-32 for provided dictionary
        if wrap == 1 {
            strm.adler = adler32(strm.adler, Some(dictionary));
        }
        s.wrap = 0; // avoid computing Adler-32 in read_buf

        // if dictionary would fill window, just replace the history
        let mut dictionary = dictionary;
        if dictionary.len() >= s.w_size {
            if wrap == 0 {
                // already empty otherwise
                CLEAR_HASH(s);
                s.strstart = 0;
                s.block_start = 0;
                s.insert = 0;
            }
            dictionary = &dictionary[dictionary.len() - s.w_size..]; // use the tail
        }

        // insert dictionary into window and hash
        let mut dict = ZStream::new();
        dict.next_in = dictionary;
        dict.total_in = strm.total_in;
        fill_window(&mut dict, s);
        while s.lookahead >= MIN_MATCH {
            let mut str = s.strstart;
            let mut n = s.lookahead - (MIN_MATCH - 1);
            loop {
                s.ins_h = UPDATE_HASH(s, s.ins_h, s.window[str + MIN_MATCH - 1]);
                s.prev[str & s.w_mask] = s.head[s.ins_h];
                s.head[s.ins_h] = str as u16;
                str += 1;
                n -= 1;
                if n == 0 {
                    break;
                }
            }
            s.strstart = str;
            s.lookahead = MIN_MATCH - 1;
            fill_window(&mut dict, s);
        }
        strm.total_in = dict.total_in;
        s.strstart += s.lookahead;
        s.block_start = s.strstart as i64;
        s.insert = s.lookahead;
        s.lookahead = 0;
        s.match_length = MIN_MATCH - 1;
        s.prev_length = MIN_MATCH - 1;
        s.match_available = false;
        s.wrap = wrap;
        Z_OK
    })
}

/// `deflateGetDictionary`: copy the sliding dictionary (up to the window size, the last
/// bytes compressed) to `dictionary`, if given, and its length to `dictLength`. Returns
/// `Z_BUF_ERROR` if `dictionary` is too short for it.
pub fn deflateGetDictionary(
    strm: &ZStream<'_>,
    dictionary: Option<&mut [u8]>,
    dictLength: Option<&mut u32>,
) -> i32 {
    let Some(s) = state_ref(strm) else {
        return Z_STREAM_ERROR;
    };
    let len = (s.strstart + s.lookahead).min(s.w_size);
    if let Some(dictionary) = dictionary
        && len != 0
    {
        let Some(dst) = dictionary.get_mut(..len) else {
            return Z_BUF_ERROR;
        };
        let end = s.strstart + s.lookahead;
        dst.copy_from_slice(&s.window[end - len..end]);
    }
    if let Some(dictLength) = dictLength {
        *dictLength = len as u32;
    }
    Z_OK
}

/// `deflateResetKeep`: reset the stream for a new compression without freeing it and
/// without resetting the "longest match" parameters (see `deflateReset`).
pub fn deflateResetKeep(strm: &mut ZStream<'_>) -> i32 {
    with_state(strm, Z_STREAM_ERROR, |strm, s| {
        strm.total_in = 0;
        strm.total_out = 0;
        strm.msg = None; // use zfree if we ever allocate msg dynamically
        strm.data_type = Z_UNKNOWN;

        s.pending = 0;
        s.pending_out = 0;

        if s.wrap < 0 {
            s.wrap = -s.wrap; // was made negative by deflate(..., Z_FINISH);
        }
        // wrap == 2 starts in GZIP_STATE with a CRC-32 only with GZIP.
        s.status = INIT_STATE;
        strm.adler = adler32(0, None);
        s.last_flush = -2;

        _tr_init(s);

        Z_OK
    })
}

/// `lm_init`: initialize the "longest match" routines for a new zlib stream.
fn lm_init(s: &mut DeflateState) {
    s.window_size = 2 * s.w_size;

    CLEAR_HASH(s);

    // Set the default configuration parameters:
    let config = &configuration_table[s.level as usize];
    s.max_lazy_match = u32::from(config.max_lazy);
    s.good_match = u32::from(config.good_length);
    s.nice_match = i32::from(config.nice_length);
    s.max_chain_length = u32::from(config.max_chain);

    s.strstart = 0;
    s.block_start = 0;
    s.lookahead = 0;
    s.insert = 0;
    s.match_length = MIN_MATCH - 1;
    s.prev_length = MIN_MATCH - 1;
    s.match_available = false;
    s.ins_h = 0;
}

/// `deflateReset`: reset the stream for a new compression with the same parameters,
/// without freeing and reallocating it.
pub fn deflateReset(strm: &mut ZStream<'_>) -> i32 {
    let ret = deflateResetKeep(strm);
    if ret == Z_OK
        && let Some(s) = state_mut(strm)
    {
        lm_init(s);
    }
    ret
}

/// `deflateSetHeader`: provide gzip header information for a gzip stream. The kernel's zlib
/// has no gzip streams (`NO_GZIP`: `wrap` is never 2), so this always returns
/// `Z_STREAM_ERROR`, as the C does in that build.
pub fn deflateSetHeader(strm: &mut ZStream<'_>, head: &GzHeader) -> i32 {
    // The C: `if (deflateStateCheck(strm) || strm->state->wrap != 2) return Z_STREAM_ERROR;`
    // and it would keep `head` for the gzip header. Both conditions refuse here.
    let _ = (strm, head);
    Z_STREAM_ERROR
}

/// `deflatePending`: the number of bytes of output generated but not yet delivered in
/// `next_out` (`pending`) and the number of bits not yet written out (`bits`).
pub fn deflatePending(
    strm: &ZStream<'_>,
    pending: Option<&mut u32>,
    bits: Option<&mut i32>,
) -> i32 {
    let Some(s) = state_ref(strm) else {
        return Z_STREAM_ERROR;
    };
    if let Some(bits) = bits {
        *bits = s.bi_valid;
    }
    if let Some(pending) = pending {
        match u32::try_from(s.pending) {
            Ok(p) => *pending = p,
            Err(_) => {
                *pending = u32::MAX;
                return Z_BUF_ERROR;
            }
        }
    }
    Z_OK
}

/// `deflateUsed`: the number of bits used in the last byte of the last completed deflate
/// block (1..8; 0 if no block was completed at a byte boundary yet).
pub fn deflateUsed(strm: &ZStream<'_>, bits: Option<&mut i32>) -> i32 {
    let Some(s) = state_ref(strm) else {
        return Z_STREAM_ERROR;
    };
    if let Some(bits) = bits {
        *bits = s.bi_used;
    }
    Z_OK
}

/// `deflatePrime`: insert the `bits` (0..16) low bits of `value` in the output stream.
pub fn deflatePrime(strm: &mut ZStream<'_>, bits: i32, value: i32) -> i32 {
    let Some(s) = state_mut(strm) else {
        return Z_STREAM_ERROR;
    };
    if !(0..=16).contains(&bits) || s.sym_buf < s.pending_out + ((Buf_size as usize + 7) >> 3) {
        return Z_BUF_ERROR;
    }
    let mut bits = bits;
    let mut value = value;
    loop {
        let put = (Buf_size - s.bi_valid).min(bits);
        s.bi_buf |= ((value & ((1 << put) - 1)) << s.bi_valid) as u16;
        s.bi_valid += put;
        _tr_flush_bits(s);
        value >>= put;
        bits -= put;
        if bits == 0 {
            break;
        }
    }
    Z_OK
}

/// `deflateParams`: change the compression level and strategy mid-stream. If the
/// compression function changes, the input so far is compressed and flushed first (as with
/// `Z_BLOCK`); `Z_BUF_ERROR` if there was not enough output space for that.
pub fn deflateParams(strm: &mut ZStream<'_>, level: i32, strategy: i32) -> i32 {
    with_state(strm, Z_STREAM_ERROR, |strm, s| {
        let mut level = level;
        if level == Z_DEFAULT_COMPRESSION {
            level = 6;
        }
        if !(0..=9).contains(&level) || !(0..=Z_FIXED).contains(&strategy) {
            return Z_STREAM_ERROR;
        }
        let func = configuration_table[s.level as usize].func;

        if (strategy != s.strategy || func != configuration_table[level as usize].func)
            && s.last_flush != -2
        {
            // Flush the last buffer:
            let err = deflate_(strm, s, Z_BLOCK);
            if err == Z_STREAM_ERROR {
                return err;
            }
            if strm.avail_in() != 0 || (s.strstart as i64 - s.block_start) + s.lookahead as i64 != 0
            {
                return Z_BUF_ERROR;
            }
        }
        if s.level != level {
            if s.level == 0 && s.matches != 0 {
                if s.matches == 1 {
                    slide_hash(s);
                } else {
                    CLEAR_HASH(s);
                }
                s.matches = 0;
            }
            s.level = level;
            let config = &configuration_table[level as usize];
            s.max_lazy_match = u32::from(config.max_lazy);
            s.good_match = u32::from(config.good_length);
            s.nice_match = i32::from(config.nice_length);
            s.max_chain_length = u32::from(config.max_chain);
        }
        s.strategy = strategy;
        Z_OK
    })
}

/// `deflateTune`: fine-tune deflate's internal compression parameters.
pub fn deflateTune(
    strm: &mut ZStream<'_>,
    good_length: i32,
    max_lazy: i32,
    nice_length: i32,
    max_chain: i32,
) -> i32 {
    let Some(s) = state_mut(strm) else {
        return Z_STREAM_ERROR;
    };
    s.good_match = good_length as u32;
    s.max_lazy_match = max_lazy as u32;
    s.nice_match = nice_length;
    s.max_chain_length = max_chain as u32;
    Z_OK
}

/// `deflateBound_z`: an upper bound on the compressed size after deflation of `sourceLen`
/// bytes.
///
/// For the default windowBits of 15 and memLevel of 8, this function returns a close to
/// exact, as well as small, upper bound on the compressed size. This is an expansion of
/// ~0.03%, plus a small constant.
///
/// For any setting other than those defaults for windowBits and memLevel, one of two worst
/// case bounds is returned. This is at most an expansion of ~4% or ~13%, plus a small
/// constant.
///
/// Both the 0.03% and 4% derive from the overhead of stored blocks. The first one is for
/// stored blocks of 16383 bytes (memLevel == 8), whereas the second is for stored blocks of
/// 127 bytes (the worst case memLevel == 1). The expansion results from five bytes of header
/// for each stored block.
///
/// The larger expansion of 13% results from a window size less than or equal to the symbols
/// buffer size (windowBits <= memLevel + 7). In that case some of the data being compressed
/// may have slid out of the sliding window, impeding a stored block from being emitted. Then
/// the only choice is a fixed or dynamic block, where a fixed block limits the maximum
/// expansion to 9 bits per 8-bit byte, plus 10 bits for every block. The smallest block size
/// for which this can occur is 255 (memLevel == 2).
///
/// Shifts are used to approximate divisions, for speed.
pub fn deflateBound_z(strm: &ZStream<'_>, sourceLen: usize) -> usize {
    // upper bound for fixed blocks with 9-bit literals and length 255 (memLevel == 2, which
    // is the lowest that may not use stored blocks) -- ~13% overhead plus a small constant
    let mut fixedlen = sourceLen
        .wrapping_add(sourceLen >> 3)
        .wrapping_add(sourceLen >> 8)
        .wrapping_add(sourceLen >> 9)
        .wrapping_add(4);
    if fixedlen < sourceLen {
        fixedlen = usize::MAX;
    }

    // upper bound for stored blocks with length 127 (memLevel == 1) -- ~4% overhead plus a
    // small constant
    let mut storelen = sourceLen
        .wrapping_add(sourceLen >> 5)
        .wrapping_add(sourceLen >> 7)
        .wrapping_add(sourceLen >> 11)
        .wrapping_add(7);
    if storelen < sourceLen {
        storelen = usize::MAX;
    }

    // if can't get parameters, return larger bound plus a wrapper
    let Some(s) = state_ref(strm) else {
        let bound = fixedlen.max(storelen);
        return bound.saturating_add(18);
    };

    // compute wrapper length
    let wraplen = match s.wrap.abs() {
        0 => 0,                                       // raw deflate
        1 => 6 + if s.strstart != 0 { 4 } else { 0 }, // zlib wrapper
        // 2, a gzip wrapper (18 bytes and the header's fields), only with GZIP
        _ => 18, // for compiler happiness
    };

    // if not default parameters, return one of the conservative bounds
    if s.w_bits != 15 || s.hash_bits != 8 + 7 {
        let bound = if s.w_bits <= s.hash_bits && s.level != 0 {
            fixedlen
        } else {
            storelen
        };
        return bound.saturating_add(wraplen);
    }

    // default settings: return tight bound for that case -- ~0.03% overhead plus a small
    // constant
    let bound = sourceLen
        .wrapping_add(sourceLen >> 12)
        .wrapping_add(sourceLen >> 14)
        .wrapping_add(sourceLen >> 25)
        .wrapping_add(13 - 6)
        .wrapping_add(wraplen);
    if bound < sourceLen { usize::MAX } else { bound }
}

/// `deflateBound`: [`deflateBound_z`] for a `uLong` length (the same width on LP64).
pub fn deflateBound(strm: &ZStream<'_>, sourceLen: u64) -> u64 {
    let Ok(len) = usize::try_from(sourceLen) else {
        return u64::MAX;
    };
    let bound = deflateBound_z(strm, len);
    u64::try_from(bound).unwrap_or(u64::MAX)
}

/// `putShortMSB`: put a short in the pending buffer. The 16-bit value is put in MSB order. IN
/// assertion: the stream state is correct and there is enough room in pending_buf.
fn putShortMSB(s: &mut DeflateState, b: u32) {
    put_byte(s, (b >> 8) as u8);
    put_byte(s, (b & 0xff) as u8);
}

/// `flush_pending`: flush as much pending output as possible. All deflate() output, except
/// for some deflate_stored() output, goes through this function.
fn flush_pending(strm: &mut ZStream<'_>, s: &mut DeflateState) {
    _tr_flush_bits(s);
    let len = s.pending.min(strm.avail_out());
    if len == 0 {
        return;
    }

    strm.put_out(&s.pending_buf[s.pending_out..s.pending_out + len]);
    s.pending_out += len;
    strm.total_out += len as u64;
    s.pending -= len;
    if s.pending == 0 {
        s.pending_out = 0;
    }
}

/// `deflate`: compress as much data as possible from `next_in` into `next_out`, stopping
/// when the input is consumed or the output is full. `flush` is `Z_NO_FLUSH`,
/// `Z_PARTIAL_FLUSH`, `Z_SYNC_FLUSH`, `Z_FULL_FLUSH`, `Z_FINISH` or `Z_BLOCK`. Returns
/// `Z_OK` if some progress was made, `Z_STREAM_END` once all input was consumed and all
/// output flushed after `Z_FINISH`, `Z_STREAM_ERROR` for an inconsistent stream and
/// `Z_BUF_ERROR` if no progress was possible.
pub fn deflate(strm: &mut ZStream<'_>, flush: i32) -> i32 {
    if !(0..=Z_BLOCK).contains(&flush) {
        return Z_STREAM_ERROR;
    }
    with_state(strm, Z_STREAM_ERROR, |strm, s| deflate_(strm, s, flush))
}

/// The body of `deflate` once the state is checked (`deflateParams` calls it too).
fn deflate_(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> i32 {
    if s.status == FINISH_STATE && flush != Z_FINISH {
        return ERR_RETURN(strm, Z_STREAM_ERROR);
    }
    if strm.avail_out() == 0 {
        return ERR_RETURN(strm, Z_BUF_ERROR);
    }

    let old_flush = s.last_flush; // value of flush param for previous deflate call
    s.last_flush = flush;

    // Flush as much pending output as possible
    if s.pending != 0 {
        flush_pending(strm, s);
        if strm.avail_out() == 0 {
            // Since avail_out is 0, deflate will be called again with more output space, but
            // possibly with both pending and avail_in equal to zero. There won't be anything
            // to do, but this is not an error situation so make sure we return OK instead of
            // BUF_ERROR at next call of deflate:
            s.last_flush = -1;
            return Z_OK;
        }

        // Make sure there is something to do and avoid duplicate consecutive flushes. For
        // repeated and useless calls with Z_FINISH, we keep returning Z_STREAM_END instead of
        // Z_BUF_ERROR.
    } else if strm.avail_in() == 0 && RANK(flush) <= RANK(old_flush) && flush != Z_FINISH {
        return ERR_RETURN(strm, Z_BUF_ERROR);
    }

    // User must not provide more input after the first FINISH:
    if s.status == FINISH_STATE && strm.avail_in() != 0 {
        return ERR_RETURN(strm, Z_BUF_ERROR);
    }

    // Write the header
    if s.status == INIT_STATE && s.wrap == 0 {
        s.status = BUSY_STATE;
    }
    if s.status == INIT_STATE {
        // zlib header
        let mut header = (Z_DEFLATED as u32 + ((s.w_bits as u32 - 8) << 4)) << 8;
        let level_flags = if s.strategy >= Z_HUFFMAN_ONLY || s.level < 2 {
            0
        } else if s.level < 6 {
            1
        } else if s.level == 6 {
            2
        } else {
            3
        };
        header |= level_flags << 6;
        if s.strstart != 0 {
            header |= PRESET_DICT;
        }
        header += 31 - (header % 31);

        putShortMSB(s, header);

        // Save the adler32 of the preset dictionary:
        if s.strstart != 0 {
            putShortMSB(s, strm.adler >> 16);
            putShortMSB(s, strm.adler & 0xffff);
        }
        strm.adler = adler32(0, None);
        s.status = BUSY_STATE;

        // Compression must start with an empty pending buffer
        flush_pending(strm, s);
        if s.pending != 0 {
            s.last_flush = -1;
            return Z_OK;
        }
    }
    // GZIP_STATE, EXTRA_STATE, NAME_STATE, COMMENT_STATE and HCRC_STATE (the gzip header)
    // only exist with GZIP.

    // Start a new block or continue the current one.
    if strm.avail_in() != 0 || s.lookahead != 0 || (flush != Z_NO_FLUSH && s.status != FINISH_STATE)
    {
        let bstate = if s.level == 0 {
            deflate_stored(strm, s, flush)
        } else if s.strategy == Z_HUFFMAN_ONLY {
            deflate_huff(strm, s, flush)
        } else if s.strategy == Z_RLE {
            deflate_rle(strm, s, flush)
        } else {
            match configuration_table[s.level as usize].func {
                CompressFunc::Stored => deflate_stored(strm, s, flush),
                CompressFunc::Fast => deflate_fast(strm, s, flush),
                CompressFunc::Slow => deflate_slow(strm, s, flush),
            }
        };

        if bstate == BlockState::FinishStarted || bstate == BlockState::FinishDone {
            s.status = FINISH_STATE;
        }
        if bstate == BlockState::NeedMore || bstate == BlockState::FinishStarted {
            if strm.avail_out() == 0 {
                s.last_flush = -1; // avoid BUF_ERROR next call, see above
            }
            return Z_OK;
            // If flush != Z_NO_FLUSH && avail_out == 0, the next call of deflate should use
            // the same flush parameter to make sure that the flush is complete. So we don't
            // have to output an empty block here, this will be done at next call. This also
            // ensures that for a very small output buffer, we emit at most one empty block.
        }
        if bstate == BlockState::BlockDone {
            if flush == Z_PARTIAL_FLUSH {
                _tr_align(s);
            } else if flush != Z_BLOCK {
                // FULL_FLUSH or SYNC_FLUSH
                _tr_stored_block(s, None, 0, false);
                // For a full flush, this empty block will be recognized as a special marker
                // by inflate_sync().
                if flush == Z_FULL_FLUSH {
                    CLEAR_HASH(s); // forget history
                    if s.lookahead == 0 {
                        s.strstart = 0;
                        s.block_start = 0;
                        s.insert = 0;
                    }
                }
            }
            flush_pending(strm, s);
            if strm.avail_out() == 0 {
                s.last_flush = -1; // avoid BUF_ERROR at next call, see above
                return Z_OK;
            }
        }
    }

    if flush != Z_FINISH {
        return Z_OK;
    }
    if s.wrap <= 0 {
        return Z_STREAM_END;
    }

    // Write the trailer (the gzip trailer, for wrap == 2, only exists with GZIP)
    putShortMSB(s, strm.adler >> 16);
    putShortMSB(s, strm.adler & 0xffff);
    flush_pending(strm, s);
    // If avail_out is zero, the application will call deflate again to flush the rest.
    if s.wrap > 0 {
        s.wrap = -s.wrap; // write the trailer only once!
    }
    if s.pending != 0 { Z_OK } else { Z_STREAM_END }
}

/// `deflateEnd`: free the stream's state. Returns `Z_DATA_ERROR` if the stream was freed
/// prematurely (some input or output was discarded), `Z_OK` otherwise.
pub fn deflateEnd(strm: &mut ZStream<'_>) -> i32 {
    let Some(s) = state_ref(strm) else {
        return Z_STREAM_ERROR;
    };
    let status = s.status;

    // Deallocate in reverse order of allocations: the state's buffers go with it.
    if let InternalState::Deflate(s) = core::mem::take(&mut strm.state) {
        zcfree(s);
    }

    if status == BUSY_STATE {
        Z_DATA_ERROR
    } else {
        Z_OK
    }
}

/// `deflateCopy`: set `dest` to a complete copy of `source`, which must be a deflate stream.
/// `dest` gets `source`'s input and counters but an empty `next_out`.
pub fn deflateCopy<'a>(dest: &mut ZStream<'a>, source: &ZStream<'a>) -> i32 {
    let Some(ss) = state_ref(source) else {
        return Z_STREAM_ERROR;
    };

    // zmemcpy(dest, source, sizeof(z_stream)), but for next_out (a &mut is not copied)
    dest.next_in = source.next_in;
    dest.total_in = source.total_in;
    dest.next_out = &mut [];
    dest.total_out = source.total_out;
    dest.msg = source.msg;
    dest.data_type = source.data_type;
    dest.adler = source.adler;
    dest.state = InternalState::None;

    let Some(mut ds) = DeflateState::alloc(ss.w_bits, ss.hash_bits, ss.lit_bufsize) else {
        return Z_MEM_ERROR;
    };
    // following copies, as in the C, only what holds data
    ds.window[..ss.high_water].copy_from_slice(&ss.window[..ss.high_water]);
    let prev_len = if ss.slid || ss.strstart - ss.insert > ds.w_size {
        ds.w_size
    } else {
        ss.strstart - ss.insert
    };
    ds.prev[..prev_len].copy_from_slice(&ss.prev[..prev_len]);
    ds.head.copy_from_slice(&ss.head);
    let po = ss.pending_out;
    ds.pending_buf[po..po + ss.pending].copy_from_slice(&ss.pending_buf[po..po + ss.pending]);
    let sb = ss.sym_buf;
    ds.pending_buf[sb..sb + ss.sym_next].copy_from_slice(&ss.pending_buf[sb..sb + ss.sym_next]);
    ds.dyn_ltree.copy_from_slice(&ss.dyn_ltree);
    ds.dyn_dtree.copy_from_slice(&ss.dyn_dtree);
    ds.bl_tree.copy_from_slice(&ss.bl_tree);
    ds.heap.copy_from_slice(&ss.heap);

    // the rest of the state, zmemcpy(ds, ss, sizeof(deflate_state))
    let ds = DeflateState {
        window: ds.window,
        prev: ds.prev,
        head: ds.head,
        pending_buf: ds.pending_buf,
        dyn_ltree: ds.dyn_ltree,
        dyn_dtree: ds.dyn_dtree,
        bl_tree: ds.bl_tree,
        heap: ds.heap,
        ..*ss
    };
    let Some(ds) = zcalloc_box(ds) else {
        return Z_MEM_ERROR;
    };
    dest.state = InternalState::Deflate(ds);
    Z_OK
}

/// `longest_match`: set match_start to the longest match starting at the given string and
/// return its length. Matches shorter or equal to prev_length are discarded, in which case
/// the result is equal to prev_length and match_start is garbage.
///
/// IN assertions: cur_match is the head of the hash chain for the current string (strstart)
/// and its distance is <= MAX_DIST, and prev_length >= 1. OUT assertion: the match length is
/// not greater than s->lookahead.
fn longest_match(s: &mut DeflateState, cur_match: usize) -> usize {
    let mut cur_match = cur_match;
    let mut chain_length = s.max_chain_length; // max hash chain length
    let scan = s.strstart; // current string
    let mut best_len = s.prev_length; // best match length so far
    let mut nice_match = s.nice_match; // stop if match long enough
    let limit = if s.strstart > MAX_DIST(s) {
        s.strstart - MAX_DIST(s)
    } else {
        NIL
    };
    // Stop when cur_match becomes <= limit. To simplify the code, we prevent matches with the
    // string of window index 0.
    let wmask = s.w_mask;
    let window = &s.window;

    let mut scan_end1 = window[scan + best_len - 1];
    let mut scan_end = window[scan + best_len];

    // The code is optimized for HASH_BITS >= 8 and MAX_MATCH-2 multiple of 16. It is easy to
    // get rid of this optimization if necessary.
    zassert!(s.hash_bits >= 8 && MAX_MATCH == 258, "Code too clever");

    // Do not waste too much time if we already have a good match:
    if s.prev_length >= s.good_match as usize {
        chain_length >>= 2;
    }
    // Do not look for matches beyond the end of the input. This is necessary to make deflate
    // deterministic.
    if nice_match as u32 as usize > s.lookahead {
        nice_match = s.lookahead as i32;
    }

    zassert!(
        s.strstart <= s.window_size - MIN_LOOKAHEAD,
        "need lookahead"
    );

    loop {
        zassert!(cur_match < s.strstart, "no future");
        let m = cur_match; // matched string

        // Skip to next match if the match length cannot increase or if the match length is
        // less than 2. Note that the checks below for insufficient lookahead only occur
        // occasionally for performance reasons. Therefore uninitialized memory will be
        // accessed, and conditional jumps will be made that depend on those values. However
        // the length of the match is limited to the lookahead, so the output of deflate is
        // not affected by the uninitialized values.
        if window[m + best_len] == scan_end
            && window[m + best_len - 1] == scan_end1
            && window[m] == window[scan]
            && window[m + 1] == window[scan + 1]
        {
            // The check at best_len - 1 can be removed because it will be made again later.
            // (This heuristic is not always a win.) It is not necessary to compare scan[2]
            // and match[2] since they are always equal when the other bytes match, given that
            // the hash keys are equal and that HASH_BITS >= 8.
            zassert!(window[scan + 2] == window[m + 2], "match[2]?");

            // Compare from scan[3] on, up to scan[MAX_MATCH]: the length is the offset of the
            // first difference, or MAX_MATCH. (The C unrolls this eight times and only then
            // checks for strend, so it may compare scan[MAX_MATCH] too; that comparison never
            // changes the length.)
            let mut len = 3;
            while len < MAX_MATCH && window[scan + len] == window[m + len] {
                len += 1;
            }

            if len > best_len {
                s.match_start = cur_match;
                best_len = len;
                if len as i32 >= nice_match {
                    break;
                }
                scan_end1 = window[scan + best_len - 1];
                scan_end = window[scan + best_len];
            }
        }

        cur_match = usize::from(s.prev[cur_match & wmask]);
        if cur_match <= limit {
            break;
        }
        chain_length = chain_length.wrapping_sub(1);
        if chain_length == 0 {
            break;
        }
    }

    if best_len <= s.lookahead {
        best_len
    } else {
        s.lookahead
    }
}

/// `FLUSH_BLOCK_ONLY(s, last)`: flush the current block, with given end-of-file flag. IN
/// assertion: strstart is set to the end of the current match.
fn FLUSH_BLOCK_ONLY(strm: &mut ZStream<'_>, s: &mut DeflateState, last: bool) {
    let buf = if s.block_start >= 0 {
        Some(s.block_start as usize)
    } else {
        None
    };
    let stored_len = (s.strstart as i64 - s.block_start) as u64;
    _tr_flush_block(s, &mut strm.data_type, buf, stored_len, last);
    s.block_start = s.strstart as i64;
    flush_pending(strm, s);
}

/// `FLUSH_BLOCK(s, last)`: the same but force premature exit if necessary (from the
/// function it is used in, with `finish_started` or `need_more`).
macro_rules! FLUSH_BLOCK {
    ($strm:expr, $s:expr, $last:expr) => {
        FLUSH_BLOCK_ONLY($strm, $s, $last);
        if $strm.avail_out() == 0 {
            return if $last {
                BlockState::FinishStarted
            } else {
                BlockState::NeedMore
            };
        }
    };
}

/// `deflate_stored`: copy without compression as much as possible from the input stream,
/// return the current block state.
///
/// In case deflateParams() is used to later switch to a non-zero compression level,
/// s->matches (otherwise unused when storing) keeps track of the number of hash table slides
/// to perform. If s->matches is 1, then one hash table slide will be done when switching. If
/// s->matches is 2, the maximum value allowed here, then the hash table will be cleared,
/// since two or more slides is the same as a clear.
///
/// deflate_stored() is written to minimize the number of times an input byte is copied. It
/// is most efficient with large input and output buffers, which maximizes the opportunities
/// to have a single copy from next_in to next_out.
fn deflate_stored(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> BlockState {
    // Smallest worthy block size when not flushing or finishing. By default this is 32K. This
    // can be as small as 507 bytes for memLevel == 1. For large input and output buffers, the
    // stored block size will be larger.
    let mut min_block = (s.pending_buf_size - 5).min(s.w_size);

    // Copy as many min_block or larger stored blocks directly to next_out as possible. If
    // flushing, copy the remaining available input to next_out as stored blocks, if there is
    // enough space.
    let mut last = false;
    let mut len;
    let mut left;
    let mut have;
    let next_in = strm.next_in; // what `next_in - used` reaches back into
    let mut used = strm.avail_in();
    loop {
        // Set len to the maximum size block that we can copy directly with the available
        // input data and output space. Set left to how much of that would be copied from
        // what's left in the window.
        len = MAX_STORED; // maximum deflate stored block length
        have = (s.bi_valid as usize + 42) >> 3; // bytes in header
        if strm.avail_out() < have {
            break; // need room for header
        }
        // maximum stored block length that will fit in avail_out:
        have = strm.avail_out() - have;
        left = (s.strstart as i64 - s.block_start) as usize; // window bytes
        if len > left + strm.avail_in() {
            len = left + strm.avail_in(); // limit len to the input
        }
        if len > have {
            len = have; // limit len to the output
        }

        // If the stored block would be less than min_block in length, or if unable to copy
        // all of the available input when flushing, then try copying to the window and the
        // pending buffer instead. Also don't write an empty block when flushing -- deflate()
        // does that.
        if len < min_block
            && ((len == 0 && flush != Z_FINISH)
                || flush == Z_NO_FLUSH
                || len != left + strm.avail_in())
        {
            break;
        }

        // Make a dummy stored block in pending to get the header bytes, including any pending
        // bits. This also updates the debugging counts.
        last = flush == Z_FINISH && len == left + strm.avail_in();
        _tr_stored_block(s, None, 0, last);

        // Replace the lengths in the dummy stored block with len.
        let p = s.pending;
        s.pending_buf[p - 4] = len as u8;
        s.pending_buf[p - 3] = (len >> 8) as u8;
        s.pending_buf[p - 2] = !len as u8;
        s.pending_buf[p - 1] = (!len >> 8) as u8;

        // Write the stored block header bytes.
        flush_pending(strm, s);

        // Copy uncompressed bytes from the window to next_out.
        if left != 0 {
            if left > len {
                left = len;
            }
            let bs = s.block_start as usize;
            strm.put_out(&s.window[bs..bs + left]);
            strm.total_out += left as u64;
            s.block_start += left as i64;
            len -= left;
        }

        // Copy uncompressed bytes directly from next_in to next_out, updating the check
        // value.
        if len != 0 {
            let out = core::mem::take(&mut strm.next_out);
            let (direct, rest) = out.split_at_mut(len);
            read_buf(strm, s.wrap, direct);
            strm.next_out = rest;
            strm.total_out += len as u64;
        }
        if last {
            break;
        }
    }

    // Update the sliding window with the last s->w_size bytes of the copied data, or append
    // all of the copied data to the existing window if less than s->w_size bytes were copied.
    // Also update the number of bytes to insert in the hash tables, in the event that
    // deflateParams() switches to a non-zero compression level.
    used -= strm.avail_in(); // number of input bytes directly copied
    if used != 0 {
        // If any input was used, then no unused input remains in the window, therefore
        // s->block_start == s->strstart.
        if used >= s.w_size {
            // supplant the previous history
            s.matches = 2; // clear hash
            s.window[..s.w_size].copy_from_slice(&next_in[used - s.w_size..used]);
            s.strstart = s.w_size;
            s.insert = s.strstart;
        } else {
            if s.window_size - s.strstart <= used {
                // Slide the window down.
                s.strstart -= s.w_size;
                s.window.copy_within(s.w_size..s.w_size + s.strstart, 0);
                if s.matches < 2 {
                    s.matches += 1; // add a pending slide_hash()
                }
                if s.insert > s.strstart {
                    s.insert = s.strstart;
                }
            }
            s.window[s.strstart..s.strstart + used].copy_from_slice(&next_in[..used]);
            s.strstart += used;
            s.insert += used.min(s.w_size - s.insert);
        }
        s.block_start = s.strstart as i64;
    }
    if s.high_water < s.strstart {
        s.high_water = s.strstart;
    }

    // If the last block was written to next_out, then done.
    if last {
        s.bi_used = 8;
        return BlockState::FinishDone;
    }

    // If flushing and all input has been consumed, then done.
    if flush != Z_NO_FLUSH
        && flush != Z_FINISH
        && strm.avail_in() == 0
        && s.strstart as i64 == s.block_start
    {
        return BlockState::BlockDone;
    }

    // Fill the window with any remaining input.
    have = s.window_size - s.strstart;
    if strm.avail_in() > have && s.block_start >= s.w_size as i64 {
        // Slide the window down.
        s.block_start -= s.w_size as i64;
        s.strstart -= s.w_size;
        s.window.copy_within(s.w_size..s.w_size + s.strstart, 0);
        if s.matches < 2 {
            s.matches += 1; // add a pending slide_hash()
        }
        have += s.w_size; // more space now
        if s.insert > s.strstart {
            s.insert = s.strstart;
        }
    }
    if have > strm.avail_in() {
        have = strm.avail_in();
    }
    if have != 0 {
        let at = s.strstart;
        read_buf(strm, s.wrap, &mut s.window[at..at + have]);
        s.strstart += have;
        s.insert += have.min(s.w_size - s.insert);
    }
    if s.high_water < s.strstart {
        s.high_water = s.strstart;
    }

    // There was not enough avail_out to write a complete worthy or flushed stored block to
    // next_out. Write a stored block to pending instead, if we have enough input for a worthy
    // block, or if flushing and there is enough room for the remaining input as a stored
    // block in the pending buffer.
    have = (s.bi_valid as usize + 42) >> 3; // bytes in header
    // maximum stored block length that will fit in pending:
    have = (s.pending_buf_size - have).min(MAX_STORED);
    min_block = have.min(s.w_size);
    left = (s.strstart as i64 - s.block_start) as usize;
    if left >= min_block
        || ((left != 0 || flush == Z_FINISH)
            && flush != Z_NO_FLUSH
            && strm.avail_in() == 0
            && left <= have)
    {
        len = left.min(have);
        last = flush == Z_FINISH && strm.avail_in() == 0 && len == left;
        _tr_stored_block(s, Some(s.block_start as usize), len, last);
        s.block_start += len as i64;
        flush_pending(strm, s);
    }

    // We've done all we can with the available input and output.
    if last {
        s.bi_used = 8;
        BlockState::FinishStarted
    } else {
        BlockState::NeedMore
    }
}

/// `deflate_fast`: compress as much as possible from the input stream, return the current
/// block state. This function does not perform lazy evaluation of matches and inserts new
/// strings in the dictionary only for unmatched strings or for short matches. It is used
/// only for the fast compression options.
fn deflate_fast(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> BlockState {
    loop {
        // Make sure that we always have enough lookahead, except at the end of the input
        // file. We need MAX_MATCH bytes for the next match, plus MIN_MATCH bytes to insert
        // the string following the next match.
        if s.lookahead < MIN_LOOKAHEAD {
            fill_window(strm, s);
            if s.lookahead < MIN_LOOKAHEAD && flush == Z_NO_FLUSH {
                return BlockState::NeedMore;
            }
            if s.lookahead == 0 {
                break; // flush the current block
            }
        }

        // Insert the string window[strstart .. strstart + 2] in the dictionary, and set
        // hash_head to the head of the hash chain:
        let mut hash_head = NIL; // head of the hash chain
        if s.lookahead >= MIN_MATCH {
            hash_head = INSERT_STRING(s, s.strstart);
        }

        // Find the longest match, discarding those <= prev_length. At this point we have
        // always match_length < MIN_MATCH
        if hash_head != NIL && s.strstart.wrapping_sub(hash_head) <= MAX_DIST(s) {
            // To simplify the code, we prevent matches with the string of window index 0 (in
            // particular we have to avoid a match of the string with itself at the start of
            // the input file).
            s.match_length = longest_match(s, hash_head);
            // longest_match() sets match_start
        }
        let bflush; // set if current block must be flushed
        if s.match_length >= MIN_MATCH {
            bflush = _tr_tally_dist(s, s.strstart - s.match_start, s.match_length - MIN_MATCH);

            s.lookahead -= s.match_length;

            // Insert new strings in the hash table only if the match length is not too
            // large. This saves time but degrades compression.
            if s.match_length <= s.max_lazy_match as usize // max_insert_length
                && s.lookahead >= MIN_MATCH
            {
                s.match_length -= 1; // string at strstart already in table
                loop {
                    s.strstart += 1;
                    INSERT_STRING(s, s.strstart);
                    // strstart never exceeds WSIZE-MAX_MATCH, so there are always MIN_MATCH
                    // bytes ahead.
                    s.match_length -= 1;
                    if s.match_length == 0 {
                        break;
                    }
                }
                s.strstart += 1;
            } else {
                s.strstart += s.match_length;
                s.match_length = 0;
                s.ins_h = usize::from(s.window[s.strstart]);
                s.ins_h = UPDATE_HASH(s, s.ins_h, s.window[s.strstart + 1]);
                // If lookahead < MIN_MATCH, ins_h is garbage, but it does not matter since it
                // will be recomputed at next deflate call.
            }
        } else {
            // No match, output a literal byte
            bflush = _tr_tally_lit(s, s.window[s.strstart]);
            s.lookahead -= 1;
            s.strstart += 1;
        }
        if bflush {
            FLUSH_BLOCK!(strm, s, false);
        }
    }
    s.insert = s.strstart.min(MIN_MATCH - 1);
    if flush == Z_FINISH {
        FLUSH_BLOCK!(strm, s, true);
        return BlockState::FinishDone;
    }
    if s.sym_next != 0 {
        FLUSH_BLOCK!(strm, s, false);
    }
    BlockState::BlockDone
}

/// `deflate_slow`: same as above, but achieves better compression. We use a lazy evaluation
/// for matches: a match is finally adopted only if there is no better match at the next
/// window position.
fn deflate_slow(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> BlockState {
    // Process the input block.
    loop {
        // Make sure that we always have enough lookahead, except at the end of the input
        // file. We need MAX_MATCH bytes for the next match, plus MIN_MATCH bytes to insert
        // the string following the next match.
        if s.lookahead < MIN_LOOKAHEAD {
            fill_window(strm, s);
            if s.lookahead < MIN_LOOKAHEAD && flush == Z_NO_FLUSH {
                return BlockState::NeedMore;
            }
            if s.lookahead == 0 {
                break; // flush the current block
            }
        }

        // Insert the string window[strstart .. strstart + 2] in the dictionary, and set
        // hash_head to the head of the hash chain:
        let mut hash_head = NIL; // head of hash chain
        if s.lookahead >= MIN_MATCH {
            hash_head = INSERT_STRING(s, s.strstart);
        }

        // Find the longest match, discarding those <= prev_length.
        s.prev_length = s.match_length;
        s.prev_match = s.match_start;
        s.match_length = MIN_MATCH - 1;

        if hash_head != NIL
            && s.prev_length < s.max_lazy_match as usize
            && s.strstart.wrapping_sub(hash_head) <= MAX_DIST(s)
        {
            // To simplify the code, we prevent matches with the string of window index 0 (in
            // particular we have to avoid a match of the string with itself at the start of
            // the input file).
            s.match_length = longest_match(s, hash_head);
            // longest_match() sets match_start

            if s.match_length <= 5
                && (s.strategy == Z_FILTERED
                    || (s.match_length == MIN_MATCH
                        && s.strstart.wrapping_sub(s.match_start) > TOO_FAR))
            {
                // If prev_match is also MIN_MATCH, match_start is garbage but we will ignore
                // the current match anyway.
                s.match_length = MIN_MATCH - 1;
            }
        }
        // If there was a match at the previous step and the current match is not better,
        // output the previous match:
        if s.prev_length >= MIN_MATCH && s.match_length <= s.prev_length {
            // Do not insert strings in hash table beyond this.
            let max_insert = (s.strstart + s.lookahead).wrapping_sub(MIN_MATCH);

            let bflush =
                _tr_tally_dist(s, s.strstart - 1 - s.prev_match, s.prev_length - MIN_MATCH);

            // Insert in hash table all strings up to the end of the match. strstart - 1 and
            // strstart are already inserted. If there is not enough lookahead, the last two
            // strings are not inserted in the hash table.
            s.lookahead -= s.prev_length - 1;
            s.prev_length -= 2;
            loop {
                s.strstart += 1;
                if s.strstart <= max_insert {
                    INSERT_STRING(s, s.strstart);
                }
                s.prev_length -= 1;
                if s.prev_length == 0 {
                    break;
                }
            }
            s.match_available = false;
            s.match_length = MIN_MATCH - 1;
            s.strstart += 1;

            if bflush {
                FLUSH_BLOCK!(strm, s, false);
            }
        } else if s.match_available {
            // If there was no match at the previous position, output a single literal. If
            // there was a match but the current match is longer, truncate the previous match
            // to a single literal.
            let bflush = _tr_tally_lit(s, s.window[s.strstart - 1]);
            if bflush {
                FLUSH_BLOCK_ONLY(strm, s, false);
            }
            s.strstart += 1;
            s.lookahead -= 1;
            if strm.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        } else {
            // There is no previous match to compare with, wait for the next step to decide.
            s.match_available = true;
            s.strstart += 1;
            s.lookahead -= 1;
        }
    }
    zassert!(flush != Z_NO_FLUSH, "no flush?");
    if s.match_available {
        _tr_tally_lit(s, s.window[s.strstart - 1]);
        s.match_available = false;
    }
    s.insert = s.strstart.min(MIN_MATCH - 1);
    if flush == Z_FINISH {
        FLUSH_BLOCK!(strm, s, true);
        return BlockState::FinishDone;
    }
    if s.sym_next != 0 {
        FLUSH_BLOCK!(strm, s, false);
    }
    BlockState::BlockDone
}

/// `deflate_rle`: for Z_RLE, simply look for runs of bytes, generate matches only of
/// distance one. Do not maintain a hash table. (It will be regenerated if this run of
/// deflate switches away from Z_RLE.)
fn deflate_rle(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> BlockState {
    loop {
        // Make sure that we always have enough lookahead, except at the end of the input
        // file. We need MAX_MATCH bytes for the longest run, plus one for the unrolled loop.
        if s.lookahead <= MAX_MATCH {
            fill_window(strm, s);
            if s.lookahead <= MAX_MATCH && flush == Z_NO_FLUSH {
                return BlockState::NeedMore;
            }
            if s.lookahead == 0 {
                break; // flush the current block
            }
        }

        // See how many times the previous byte repeats
        s.match_length = 0;
        if s.lookahead >= MIN_MATCH && s.strstart > 0 {
            let window = &s.window;
            let at = s.strstart;
            let prev = window[at - 1]; // byte at distance one to match
            if prev == window[at] && prev == window[at + 1] && prev == window[at + 2] {
                // The run goes on up to MAX_MATCH bytes from strstart (the C's unrolled loop
                // may look at window[strstart + MAX_MATCH] too, without effect).
                let mut len = 3;
                while len < MAX_MATCH && window[at + len] == prev {
                    len += 1;
                }
                s.match_length = len.min(s.lookahead);
            }
        }

        // Emit match if have run of MIN_MATCH or longer, else emit literal
        let bflush; // set if current block must be flushed
        if s.match_length >= MIN_MATCH {
            bflush = _tr_tally_dist(s, 1, s.match_length - MIN_MATCH);

            s.lookahead -= s.match_length;
            s.strstart += s.match_length;
            s.match_length = 0;
        } else {
            // No match, output a literal byte
            bflush = _tr_tally_lit(s, s.window[s.strstart]);
            s.lookahead -= 1;
            s.strstart += 1;
        }
        if bflush {
            FLUSH_BLOCK!(strm, s, false);
        }
    }
    s.insert = 0;
    if flush == Z_FINISH {
        FLUSH_BLOCK!(strm, s, true);
        return BlockState::FinishDone;
    }
    if s.sym_next != 0 {
        FLUSH_BLOCK!(strm, s, false);
    }
    BlockState::BlockDone
}

/// `deflate_huff`: for Z_HUFFMAN_ONLY, do not look for matches. Do not maintain a hash table.
/// (It will be regenerated if this run of deflate switches away from Huffman.)
fn deflate_huff(strm: &mut ZStream<'_>, s: &mut DeflateState, flush: i32) -> BlockState {
    loop {
        // Make sure that we have a literal to write.
        if s.lookahead == 0 {
            fill_window(strm, s);
            if s.lookahead == 0 {
                if flush == Z_NO_FLUSH {
                    return BlockState::NeedMore;
                }
                break; // flush the current block
            }
        }

        // Output a literal byte
        s.match_length = 0;
        let bflush = _tr_tally_lit(s, s.window[s.strstart]);
        s.lookahead -= 1;
        s.strstart += 1;
        if bflush {
            FLUSH_BLOCK!(strm, s, false);
        }
    }
    s.insert = 0;
    if flush == Z_FINISH {
        FLUSH_BLOCK!(strm, s, true);
        return BlockState::FinishDone;
    }
    if s.sym_next != 0 {
        FLUSH_BLOCK!(strm, s, false);
    }
    BlockState::BlockDone
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the deflate compressor.
    //
    // The vectors in `testdata/deflate_*` come from zlib 1.2.12 (the macOS system libz, which
    // Python's zlib module uses), driven through the same calls as `run` below. To regenerate
    // them: `/usr/bin/python3 sys/lib/libz/testdata/gen_deflate.py`. `deflate_vectors.txt`
    // holds the length and CRC-32 of every output (and `total_in`/`adler` at the end); three
    // complete outputs are in `.bin` files. `make_inputs` builds the inputs exactly as the
    // script does; the `input` lines check that.

    use super::*;
    use crate::crc32::crc32;
    use crate::zlib::{deflateInit, deflateInit2};
    use std::format;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    const VECTORS: &str = include_str!("testdata/deflate_vectors.txt");
    const TEXT: &[u8] = include_bytes!("testdata/deflate_text.txt");
    const IPCOMP_TEXT: &[u8] = include_bytes!("testdata/deflate_ipcomp_text.bin");
    const ZLIB_TEXT_L9: &[u8] = include_bytes!("testdata/deflate_zlib_text_l9.bin");
    const RAW_TEXT_L1: &[u8] = include_bytes!("testdata/deflate_raw_text_l1.bin");

    /// The "large" output buffer of the vectors.
    const BIG: usize = 1 << 18;

    /// The generator's pseudo-random sequence: x = x * 1103515245 + 12345 mod 2^31.
    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = (self.0.wrapping_mul(1_103_515_245).wrapping_add(12345)) & 0x7fff_ffff;
            self.0
        }

        fn byte(&mut self) -> u8 {
            (self.next() >> 16) as u8
        }
    }

    /// The inputs of `gen_deflate.py`'s `make_inputs`.
    fn make_inputs() -> Vec<(&'static str, Vec<u8>)> {
        let mut rep = Vec::new();
        for _ in 0..1500 {
            rep.extend_from_slice(b"0123456789abcdef");
        }
        rep.extend(std::iter::repeat_n(b'z', 7000));
        for _ in 0..900 {
            rep.extend_from_slice(b"hello, world. ");
        }
        let mut g = Lcg(1);
        let rand: Vec<u8> = (0..30000).map(|_| g.byte()).collect();
        let mut mixed = Vec::new();
        let mut g = Lcg(7);
        let mut i = 0usize;
        while mixed.len() < 102_400 {
            match i % 4 {
                0 => {
                    let start = (i * 37) % TEXT.len();
                    let end = (start + 3000).min(TEXT.len());
                    mixed.extend_from_slice(&TEXT[start..end]);
                }
                1 => mixed.extend((0..2000).map(|_| g.byte())),
                2 => mixed.extend(std::iter::repeat_n((i * 7) as u8, 500 + (i * 13) % 3000)),
                _ => mixed.extend((0..4000).map(|_| b'a' + ((g.next() >> 16) % 4) as u8)),
            }
            i += 1;
        }
        mixed.truncate(102_400);
        vec![
            ("empty", Vec::new()),
            ("one", b"a".to_vec()),
            ("text", TEXT.to_vec()),
            ("rep", rep),
            ("rand", rand),
            ("mixed", mixed),
        ]
    }

    fn input<'i>(inputs: &'i [(&str, Vec<u8>)], name: &str) -> &'i [u8] {
        &inputs.iter().find(|(n, _)| *n == name).unwrap().1
    }

    /// What the driver does between `deflateInit2` and `deflateEnd`.
    #[derive(Debug, Clone, Copy)]
    enum Schedule {
        /// All input with `Z_FINISH`.
        Finish,
        /// Input up to `at` with `flush`, the rest with `Z_FINISH`.
        Flush(i32, usize),
        /// Input up to `at` with `Z_NO_FLUSH`, `deflateParams(level, strategy)`, the rest with
        /// `Z_FINISH`.
        Params(usize, i32, i32),
    }

    /// The output buffers handed to the stream: one buffer cut into `chunk`-sized pieces, one
    /// piece per call, and which bytes of each piece the call wrote.
    struct Slots<'a> {
        rest: &'a mut [u8],
        offset: usize,
        chunk: usize,
        used: Vec<(usize, usize)>,
    }

    impl<'a> Slots<'a> {
        fn give(&mut self, strm: &mut ZStream<'a>) {
            let (piece, rest) = core::mem::take(&mut self.rest).split_at_mut(self.chunk);
            self.rest = rest;
            strm.next_out = piece;
        }

        fn taken(&mut self, strm: &ZStream<'a>) {
            self.used.push((self.offset, self.chunk - strm.avail_out()));
            self.offset += self.chunk;
        }
    }

    /// `deflate(flush)` over `input` with fresh output buffers until a call leaves room in its
    /// buffer (and, for `Z_FINISH`, returns `Z_STREAM_END`).
    fn feed<'a>(strm: &mut ZStream<'a>, slots: &mut Slots<'a>, input: &'a [u8], flush: i32) {
        strm.next_in = input;
        loop {
            slots.give(strm);
            let ret = deflate(strm, flush);
            assert!(
                matches!(ret, Z_OK | Z_STREAM_END | Z_BUF_ERROR),
                "deflate: {ret}"
            );
            slots.taken(strm);
            if strm.avail_out() != 0 {
                if flush == Z_FINISH {
                    assert_eq!(ret, Z_STREAM_END);
                }
                break;
            }
        }
        assert_eq!(strm.avail_in(), 0);
    }

    /// The result of one compression.
    struct Output {
        bytes: Vec<u8>,
        total_in: u64,
        adler: u32,
        bound: usize,
    }

    /// Compress `data` the way `gen_deflate.py`'s `run` does.
    fn run(
        data: &[u8],
        (level, wbits, memlevel, strategy): (i32, i32, i32, i32),
        schedule: Schedule,
        chunk: usize,
        dictionary: Option<&[u8]>,
    ) -> Output {
        let pieces = (data.len() + data.len() / 8 + 1024) / chunk + 8;
        let mut space = vec![0u8; pieces * chunk];
        let (used, total_in, adler, bound);
        {
            let mut slots = Slots {
                rest: &mut space,
                offset: 0,
                chunk,
                used: Vec::new(),
            };
            let mut strm = ZStream::new();
            let ret = deflateInit2(&mut strm, level, Z_DEFLATED, wbits, memlevel, strategy);
            assert_eq!(ret, Z_OK);
            if let Some(d) = dictionary {
                assert_eq!(deflateSetDictionary(&mut strm, d), Z_OK);
            }
            bound = deflateBound_z(&strm, data.len());
            match schedule {
                Schedule::Finish => feed(&mut strm, &mut slots, data, Z_FINISH),
                Schedule::Flush(flush, at) => {
                    feed(&mut strm, &mut slots, &data[..at], flush);
                    feed(&mut strm, &mut slots, &data[at..], Z_FINISH);
                }
                Schedule::Params(at, l, s) => {
                    feed(&mut strm, &mut slots, &data[..at], Z_NO_FLUSH);
                    slots.give(&mut strm);
                    assert_eq!(deflateParams(&mut strm, l, s), Z_OK);
                    slots.taken(&strm);
                    feed(&mut strm, &mut slots, &data[at..], Z_FINISH);
                }
            }
            total_in = strm.total_in;
            adler = strm.adler;
            assert_eq!(
                strm.total_out,
                slots.used.iter().map(|&(_, n)| n as u64).sum::<u64>()
            );
            assert_eq!(deflateEnd(&mut strm), Z_OK);
            used = slots.used;
        }
        let mut bytes = Vec::new();
        for (at, n) in used {
            bytes.extend_from_slice(&space[at..at + n]);
        }
        Output {
            bytes,
            total_in,
            adler,
            bound,
        }
    }

    /// One line of `deflate_vectors.txt`.
    struct Case<'v> {
        line: &'v str,
        input: &'v str,
        params: (i32, i32, i32, i32),
        schedule: Schedule,
        chunk: usize,
        dict: Option<usize>,
        out_len: usize,
        out_crc: u32,
        total_in: u64,
        adler: u32,
    }

    fn cases() -> Vec<Case<'static>> {
        let mut cases = Vec::new();
        for line in VECTORS.lines() {
            let f: Vec<&str> = line.split('|').collect();
            if line.starts_with('#') || f[0] == "input" {
                continue;
            }
            let num = |i: usize| -> i32 { f[i].parse().unwrap() };
            let s: Vec<&str> = f[6].split(':').collect();
            let schedule = match s[0] {
                "finish" => Schedule::Finish,
                "params" => Schedule::Params(
                    s[1].parse().unwrap(),
                    s[2].parse().unwrap(),
                    s[3].parse().unwrap(),
                ),
                name => {
                    let flush = match name {
                        "partial" => Z_PARTIAL_FLUSH,
                        "sync" => crate::zlib::Z_SYNC_FLUSH,
                        "full" => Z_FULL_FLUSH,
                        "block" => Z_BLOCK,
                        _ => panic!("schedule {name}"),
                    };
                    Schedule::Flush(flush, s[1].parse().unwrap())
                }
            };
            cases.push(Case {
                line,
                input: f[1],
                params: (num(2), num(3), num(4), num(5)),
                schedule,
                chunk: f[7].parse().unwrap(),
                dict: (f[8] != "-").then(|| f[8].split(':').nth(1).unwrap().parse().unwrap()),
                out_len: f[9].parse().unwrap(),
                out_crc: u32::from_str_radix(f[10], 16).unwrap(),
                total_in: f[11].parse().unwrap(),
                adler: u32::from_str_radix(f[12], 16).unwrap(),
            });
        }
        cases
    }

    fn run_case(inputs: &[(&str, Vec<u8>)], c: &Case<'_>) -> Output {
        let dict = c.dict.map(|n| &TEXT[..n]);
        run(input(inputs, c.input), c.params, c.schedule, c.chunk, dict)
    }

    #[test]
    fn inputs_match_the_generator() {
        let inputs = make_inputs();
        let mut seen = 0;
        for line in VECTORS.lines().filter(|l| l.starts_with("input|")) {
            let f: Vec<&str> = line.split('|').collect();
            let data = input(&inputs, f[1]);
            assert_eq!(data.len(), f[2].parse::<usize>().unwrap(), "{line}");
            assert_eq!(
                crc32(0, data),
                u32::from_str_radix(f[3], 16).unwrap(),
                "{line}"
            );
            seen += 1;
        }
        assert_eq!(seen, inputs.len());
    }

    #[test]
    fn outputs_match_zlib_1_2_12() {
        let inputs = make_inputs();
        let cases = cases();
        assert!(cases.len() > 400);
        let mut failures = Vec::<String>::new();
        for c in &cases {
            let out = run_case(&inputs, c);
            let got = (
                out.bytes.len(),
                crc32(0, &out.bytes),
                out.total_in,
                out.adler,
            );
            if got != (c.out_len, c.out_crc, c.total_in, c.adler) {
                failures.push(format!(
                    "{} -> got len {} crc {:08x} total_in {} adler {:08x}",
                    c.line, got.0, got.1, got.2, got.3
                ));
            }
            // zlib.h: the bound holds when all input is compressed with Z_FINISH (other flushes
            // may exceed it).
            if matches!(c.schedule, Schedule::Finish) {
                assert!(
                    out.bound >= out.bytes.len(),
                    "deflateBound too small: {}",
                    c.line
                );
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} cases differ:\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn complete_outputs_match_byte_for_byte() {
        let inputs = make_inputs();
        let text = input(&inputs, "text");
        let ipcomp = run(
            text,
            (Z_DEFAULT_COMPRESSION, -12, 8, Z_DEFAULT_STRATEGY),
            Schedule::Finish,
            BIG,
            None,
        );
        assert_eq!(ipcomp.bytes, IPCOMP_TEXT);
        let zlib9 = run(
            text,
            (9, 15, 8, Z_DEFAULT_STRATEGY),
            Schedule::Finish,
            BIG,
            None,
        );
        assert_eq!(zlib9.bytes, ZLIB_TEXT_L9);
        let raw1 = run(
            text,
            (1, -15, 8, Z_DEFAULT_STRATEGY),
            Schedule::Finish,
            BIG,
            None,
        );
        assert_eq!(raw1.bytes, RAW_TEXT_L1);
    }

    /// The IPComp path of `xform_ipcomp.c`: a raw stream with a 4 KiB window, `Z_FINISH`
    /// repeatedly with fresh 16-byte output buffers: `Z_OK` while a buffer fills up, then
    /// `Z_STREAM_END`; `deflateEnd` is `Z_OK`.
    #[test]
    fn ipcomp_finish_with_small_buffers() {
        let mut space = [0u8; 2048];
        let mut written = 0;
        {
            let mut strm = ZStream::new();
            let ret = deflateInit2(
                &mut strm,
                Z_DEFAULT_COMPRESSION,
                Z_DEFLATED,
                -12,
                8,
                Z_DEFAULT_STRATEGY,
            );
            assert_eq!(ret, Z_OK);
            strm.next_in = TEXT;
            let mut rest: &mut [u8] = &mut space;
            let mut calls = 0;
            loop {
                let (piece, r) = core::mem::take(&mut rest).split_at_mut(16);
                rest = r;
                strm.next_out = piece;
                let ret = deflate(&mut strm, Z_FINISH);
                calls += 1;
                written += 16 - strm.avail_out();
                if ret == Z_STREAM_END {
                    break;
                }
                assert_eq!(ret, Z_OK);
                assert_eq!(strm.avail_out(), 0, "Z_OK with room left");
            }
            assert!(calls > 50);
            assert_eq!(strm.total_in, TEXT.len() as u64);
            assert_eq!(strm.total_out, written as u64);
            // more Z_FINISH calls keep returning Z_STREAM_END
            let mut extra = [0u8; 4];
            strm.next_out = &mut extra;
            assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
            assert_eq!(deflateEnd(&mut strm), Z_OK);
        }
        assert_eq!(&space[..written], IPCOMP_TEXT);
    }

    #[test]
    fn bad_parameters_are_refused() {
        let mut strm = ZStream::new();
        let bad = [
            (10, Z_DEFLATED, 15, 8, 0),
            (-2, Z_DEFLATED, 15, 8, 0),
            (6, 7, 15, 8, 0),
            (6, Z_DEFLATED, 16, 8, 0), // gzip wrapper: not configured (NO_GZIP)
            (6, Z_DEFLATED, 31, 8, 0),
            (6, Z_DEFLATED, 7, 8, 0),
            (6, Z_DEFLATED, -8, 8, 0), // 8 only with the zlib wrapper
            (6, Z_DEFLATED, -16, 8, 0),
            (6, Z_DEFLATED, 15, 0, 0),
            (6, Z_DEFLATED, 15, 10, 0),
            (6, Z_DEFLATED, 15, 8, 5),
            (6, Z_DEFLATED, 15, 8, -1),
        ];
        for (level, method, wbits, mem, strategy) in bad {
            let ret = deflateInit2(&mut strm, level, method, wbits, mem, strategy);
            assert_eq!(
                ret, Z_STREAM_ERROR,
                "{level} {method} {wbits} {mem} {strategy}"
            );
        }
        let ret = deflateInit2_(
            &mut strm,
            6,
            Z_DEFLATED,
            15,
            8,
            0,
            "2.0",
            size_of::<ZStream<'_>>() as i32,
        );
        assert_eq!(ret, Z_VERSION_ERROR);
        let ret = deflateInit_(&mut strm, 6, ZLIB_VERSION, 4);
        assert_eq!(ret, Z_VERSION_ERROR);
        // a stream that was never initialised
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_ERROR);
        assert_eq!(deflateEnd(&mut strm), Z_STREAM_ERROR);
        assert_eq!(deflateReset(&mut strm), Z_STREAM_ERROR);
        assert_eq!(deflateParams(&mut strm, 1, 0), Z_STREAM_ERROR);
        assert_eq!(deflateBound(&strm, 1000), 1000 + 125 + 3 + 1 + 4 + 18);
    }

    #[test]
    fn stream_errors() {
        let mut out = [0u8; 256];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit(&mut strm, Z_DEFAULT_COMPRESSION), Z_OK);
        assert_eq!(
            deflateSetHeader(&mut strm, &GzHeader::default()),
            Z_STREAM_ERROR
        );
        assert_eq!(deflate(&mut strm, Z_BLOCK + 1), Z_STREAM_ERROR);
        assert_eq!(deflate(&mut strm, -1), Z_STREAM_ERROR);
        // no output space
        strm.next_in = b"hello";
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_BUF_ERROR);
        assert_eq!(strm.msg, Some("buffer error"));
        strm.next_out = &mut out;
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
        // nothing to do: a second Z_NO_FLUSH without input is a buffer error
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_BUF_ERROR);
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        // no more input after Z_FINISH, and no other flush
        strm.next_in = b"more";
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_BUF_ERROR);
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_STREAM_ERROR);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        assert_eq!(deflateEnd(&mut strm), Z_STREAM_ERROR);
    }

    #[test]
    fn end_while_busy_is_a_data_error() {
        let mut out = [0u8; 64];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit(&mut strm, 6), Z_OK);
        strm.next_in = TEXT;
        strm.next_out = &mut out;
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
        assert_eq!(deflateEnd(&mut strm), Z_DATA_ERROR);
        assert!(matches!(strm.state, InternalState::None));
    }

    #[test]
    fn reset_gives_the_same_output() {
        let mut a = [0u8; 2048];
        let mut b = [0u8; 2048];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, 15, 8, 0), Z_OK);
        strm.next_in = TEXT;
        strm.next_out = &mut a;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        let n = strm.total_out as usize;
        assert_eq!(deflateReset(&mut strm), Z_OK);
        assert_eq!((strm.total_in, strm.total_out, strm.adler), (0, 0, 1));
        strm.next_in = TEXT;
        strm.next_out = &mut b;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        assert_eq!(strm.total_out as usize, n);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        assert_eq!(a[..n], b[..n]);
    }

    #[test]
    fn copy_continues_like_the_original() {
        let mut out1 = [0u8; 4096];
        let mut out2 = [0u8; 4096];
        let (half1, half2) = TEXT.split_at(TEXT.len() / 2);
        let mut head = [0u8; 4096];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit2(&mut strm, 9, Z_DEFLATED, -15, 8, 0), Z_OK);
        strm.next_in = half1;
        strm.next_out = &mut head;
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
        let mut copy = ZStream::new();
        assert_eq!(deflateCopy(&mut copy, &strm), Z_OK);
        assert_eq!(copy.avail_out(), 0);
        assert_eq!(copy.total_in, strm.total_in);
        for (s, out) in [(&mut strm, &mut out1), (&mut copy, &mut out2)] {
            s.next_in = half2;
            s.next_out = out;
            assert_eq!(deflate(s, Z_FINISH), Z_STREAM_END);
        }
        assert_eq!(strm.total_out, copy.total_out);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        assert_eq!(deflateEnd(&mut copy), Z_OK);
        assert_eq!(out1, out2);
    }

    #[test]
    fn dictionary_round_trips_through_get_dictionary() {
        let mut out = [0u8; 4096];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, 9, 8, 0), Z_OK);
        // longer than the 512-byte window: only the tail is kept
        assert_eq!(deflateSetDictionary(&mut strm, TEXT), Z_OK);
        assert_eq!(strm.adler, crate::adler32::adler32(1, Some(TEXT)));
        let mut dict = [0u8; 512];
        let mut len = 0;
        assert_eq!(
            deflateGetDictionary(&strm, Some(&mut dict), Some(&mut len)),
            Z_OK
        );
        assert_eq!(len, 512);
        assert_eq!(dict[..], TEXT[TEXT.len() - 512..]);
        let mut short = [0u8; 100];
        assert_eq!(
            deflateGetDictionary(&strm, Some(&mut short), None),
            Z_BUF_ERROR
        );
        strm.next_in = b"abc";
        strm.next_out = &mut out;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        // a zlib stream takes a dictionary only before the first deflate
        assert_eq!(deflateReset(&mut strm), Z_OK);
        strm.next_in = b"abc";
        assert_eq!(deflate(&mut strm, Z_NO_FLUSH), Z_OK);
        assert_eq!(deflateSetDictionary(&mut strm, b"abc"), Z_STREAM_ERROR);
        assert_eq!(deflateEnd(&mut strm), Z_DATA_ERROR);
    }

    #[test]
    fn pending_prime_used_and_tune() {
        let mut out = [0u8; 4096];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit2(&mut strm, 6, Z_DEFLATED, -15, 8, 0), Z_OK);
        // a raw stream may start with bits of the caller's own
        assert_eq!(deflatePrime(&mut strm, 17, 0), Z_BUF_ERROR);
        assert_eq!(deflatePrime(&mut strm, 3, 0b101), Z_OK);
        assert_eq!(deflatePrime(&mut strm, 10, 0x3ff), Z_OK);
        let (mut pending, mut bits) = (0, 0);
        assert_eq!(
            deflatePending(&strm, Some(&mut pending), Some(&mut bits)),
            Z_OK
        );
        assert_eq!((pending, bits), (1, 5));
        assert_eq!(deflateTune(&mut strm, 4, 4, 8, 4), Z_OK);
        strm.next_in = TEXT;
        strm.next_out = &mut out;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        let mut used = 0;
        assert_eq!(deflateUsed(&strm, Some(&mut used)), Z_OK);
        assert!((1..=8).contains(&used));
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        assert_eq!(out[0], 0b1111_1101);
    }

    #[test]
    fn bound_formulas() {
        let mut strm = ZStream::new();
        // the default parameters get the tight bound
        assert_eq!(deflateInit(&mut strm, 6), Z_OK);
        assert_eq!(
            deflateBound_z(&strm, 100_000),
            100_000 + 24 + 6 + 13 - 6 + 6
        );
        assert_eq!(deflateBound(&strm, u64::MAX), u64::MAX);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        // IPComp's: window 2^12 <= hash bits 15, level != 0: the fixed-block bound, raw
        assert_eq!(deflateInit2(&mut strm, -1, Z_DEFLATED, -12, 8, 0), Z_OK);
        assert_eq!(deflateBound_z(&strm, 1000), 1000 + 125 + 3 + 1 + 4);
        assert_eq!(deflateEnd(&mut strm), Z_OK);
    }
}
/* </TESTS> */
