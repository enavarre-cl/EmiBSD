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
 * Copyright (c) 2019 Alexander Bluhm <bluhm@openbsd.org>
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
//! `hexdump()`: bytes in hex and ASCII, sixteen per line, for boot(8)'s `hexdump` command.
//!
//! Upstream: sys/lib/libsa/hexdump.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The memory is a slice; the addresses printed are its bytes' addresses, as in C.

use crate::printf;

/// `hexdump(addr, size)`.
pub fn hexdump(mem: &[u8]) {
    let base = mem.as_ptr() as usize;
    for (n, line) in mem.chunks(16).enumerate() {
        printf!("{:08x}  ", base + 16 * n);
        for byte in 0..16 {
            match line.get(byte) {
                Some(b) => printf!("{:02x} ", b),
                None => printf!("   "),
            }
            if byte == 7 {
                printf!(" ");
            }
        }
        printf!(" |");
        for &b in line {
            if (b' '..=b'~').contains(&b) {
                printf!("{}", char::from(b));
            } else {
                printf!(".");
            }
        }
        printf!("|\n");
    }
    printf!("{:08x}\n", base + mem.len());
}
/* </CODE> */
