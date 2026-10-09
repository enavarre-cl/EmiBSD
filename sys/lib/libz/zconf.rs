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

/* zconf.h -- configuration of the zlib compression library
 * Copyright (C) 1995-2026 Jean-loup Gailly, Mark Adler
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
//! zlib's build configuration: the limits of `windowBits` and `memLevel`.
//!
//! Upstream: sys/lib/libz/zconf.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `zconf.h`** (zlib licence,
//! clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full.
//!
//! ## Deviations
//! - Only the two limits are kept. The rest of the header adapts C to compilers and platforms
//!   (`Z_PREFIX` renames, `FAR`, `OF()`, `z_const`, `ZEXTERN`/`ZEXPORT`, `STDC` detection, the
//!   `Byte`/`uInt`/`uLong`/`voidpf`/`z_size_t`/`z_off_t` typedefs): the Rust side uses `u8`,
//!   `u32`, `u64`, `usize` and slices directly, and the memory-requirement note is on the two
//!   constants.

/// `MAX_MEM_LEVEL`: maximum value for `memLevel` in `deflateInit2` (9: `MAXSEG_64K` is not
/// defined). The memory requirements for deflate are `(1 << (windowBits+2)) + (1 <<
/// (memLevel+9))` bytes, 128K + 128K for the defaults, plus a few kilobytes for small objects.
pub const MAX_MEM_LEVEL: i32 = 9;

/// `MAX_WBITS`: maximum value for `windowBits` in `deflateInit2` and `inflateInit2` (a 32K
/// LZ77 window). Inflate needs `1 << windowBits` bytes, 32K, plus about 7 kilobytes.
pub const MAX_WBITS: i32 = 15;
/* </CODE> */
