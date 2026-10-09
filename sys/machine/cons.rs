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
//! `consinit(9)`: the machine-dependent half of the console framework.
//!
//! `<dev/cons.h>` and `dev/cons.c` (ported as `dev/cons.rs`) are generic: `cn_tab`, `cnputc`,
//! `cngetc`. What each `machdep.c` (or `consinit.c`) provides is `consinit()`, declared in
//! `<sys/systm.h>`: find the console device and attach it, once. `main()` calls it early, and
//! the architectures call it even earlier, from their first C function, so a panic during boot
//! has somewhere to print.

use crate::machine::Machine;

/// The console attach each architecture provides.
pub trait Console {
    /// `consinit()`: attaches the console device. Idempotent: the second and later calls do
    /// nothing, as in every OpenBSD `machdep.c`.
    fn consinit();
}

/// `consinit()` on the selected machine.
pub fn consinit() {
    Machine::consinit()
}
/* </CODE> */
