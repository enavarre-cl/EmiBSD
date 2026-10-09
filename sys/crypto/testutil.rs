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
//! Host test helpers for `sys/crypto`: the known-answer vectors are written as hex strings,
//! and the reference-backed tests read the constant tables of the C files.

extern crate std;

use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

/// The bytes of a hex string; spaces and line breaks are ignored.
pub(crate) fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("not a hex digit: {}", b as char),
        })
        .collect();
    assert!(digits.len() % 2 == 0, "odd number of hex digits");
    digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect()
}

/// The bytes of a hex string as a fixed-size array.
pub(crate) fn hexn<const N: usize>(s: &str) -> [u8; N] {
    let v = hex(s);
    let mut a = [0u8; N];
    a.copy_from_slice(&v);
    a
}

/// The integers of the table `name` of the C file `rel` (below `$OPENBSD_SRC`): every number
/// between the `{` that follows `name[` and the closing `};`, comments dropped, in order. For
/// the reference-backed tests (`just test-ref`).
pub(crate) fn c_table(rel: &str, name: &str) -> Vec<u64> {
    let path = crate::reftest::openbsd_src().join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    // Drop the comments.
    let mut clean = std::string::String::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        match rest[i..].find("*/") {
            Some(j) => rest = &rest[i + j + 2..],
            None => rest = "",
        }
    }
    clean.push_str(rest);

    let start = clean
        .find(&std::format!("{name}["))
        .unwrap_or_else(|| panic!("{name}: not in {rel}"));
    let open = start + clean[start..].find('{').expect("no table body");
    let close = open + clean[open..].find("};").expect("no end of table");
    clean[open + 1..close]
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| {
            let t = t.trim_end_matches(['U', 'L', 'u', 'l']);
            match t.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16),
                None => t.parse(),
            }
            .unwrap_or_else(|_| panic!("{name}: cannot parse `{t}`"))
        })
        .collect()
}

/// The crypto framework's tables (`crypto_drivers`, `swcr_sessions`) are global: the tests that
/// touch them hold this lock, one at a time.
pub(crate) fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
/* </CODE> */
