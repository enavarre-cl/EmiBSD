/* $OpenBSD: acpiutil.c,v 1.9 2020/06/17 16:16:05 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
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

//! `dev/acpi/acpiutil.c`: the checksum of the firmware tables.
//!
//! Upstream: sys/dev/acpi/acpiutil.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The table is a byte slice (the C's pointer and length). `ACPI_DEBUG` is not
//!   configured, so the "bad checksum" trace is compiled out, as in C.

/// `acpi_checksum(v, len)`: the sum of the table's bytes, modulo 256; 0 for a valid table.
pub fn acpi_checksum(v: &[u8]) -> u32 {
    let s = v.iter().fold(0u8, |s, &b| s.wrapping_add(b));

    u32::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_of_a_table() {
        assert_eq!(acpi_checksum(&[]), 0);
        assert_eq!(acpi_checksum(&[0x80, 0x80]), 0);
        assert_eq!(acpi_checksum(&[1, 2, 3]), 6);
        assert_eq!(acpi_checksum(&[0xff, 0x02]), 1);
    }
}
