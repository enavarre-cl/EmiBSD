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
//! `/etc/firmware/fxp-*` for the base set (M16c): OpenBSD's `sys/dev/microcode/fxp/build.c`
//! re-expressed. `build.c` includes `rcvbundl.h` and writes each microcode array to a file
//! as little-endian dwords; here the arrays are read out of the same header (the
//! `#define NAME { 0x..., ... }` blocks) and the files are made in memory, to join the
//! base set beside `fxp-license` (`sys/dev/microcode/fxp/Makefile` installs the seven
//! files and the licence into `/etc/firmware`, mode 644, and the `base` set list names
//! them). The ramdisk lists name none (`distrib/amd64/ramdisk_cd/list`), so `bsd.rd`'s
//! fxp(4) finds no firmware, as on OpenBSD.

use super::*;

/// The firmware files of `build.c` and the macro of `rcvbundl.h` each is made from, in
/// the order `build.c` writes them.
pub(super) const FXP_FILES: [(&str, &str); 7] = [
    ("fxp-d101a", "D101_A_RCVBUNDLE_UCODE"),
    ("fxp-d101b0", "D101_B0_RCVBUNDLE_UCODE"),
    ("fxp-d101ma", "D101M_B_RCVBUNDLE_UCODE"),
    ("fxp-d101s", "D101S_RCVBUNDLE_UCODE"),
    ("fxp-d102", "D102_B_RCVBUNDLE_UCODE"),
    ("fxp-d102c", "D102_C_RCVBUNDLE_UCODE"),
    ("fxp-d102e", "D102_E_RCVBUNDLE_UCODE"),
];

/// The dwords of `#define <name> { ... }` in `text` (the C header's initialiser).
fn macro_dwords(text: &str, name: &str) -> Result<Vec<u32>> {
    let define = text
        .match_indices(name)
        .find(|(i, _)| {
            text[..*i]
                .trim_end_matches([' ', '\t'])
                .ends_with("#define")
        })
        .map(|(i, _)| i)
        .ok_or_else(|| format!("rcvbundl.h: no #define {name}"))?;
    let rest = &text[define..];
    let open = rest
        .find('{')
        .ok_or_else(|| format!("rcvbundl.h: {name}: no {{"))?;
    let close = rest
        .find('}')
        .ok_or_else(|| format!("rcvbundl.h: {name}: no }}"))?;
    rest[open + 1..close]
        .split(|c: char| c == ',' || c.is_whitespace() || c == '\\')
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.strip_prefix("0x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("rcvbundl.h: {name}: bad dword {w:?}").into())
        })
        .collect()
}

/// The seven firmware files of `build.c` out of the text of `rcvbundl.h`: name and bytes.
pub(super) fn fxp_firmware_from(text: &str) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let mut files = Vec::new();
    for (file, name) in FXP_FILES {
        let bytes = macro_dwords(text, name)?
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        files.push((file, bytes));
    }
    Ok(files)
}

/// The seven firmware files of `build.c`, from `<src>/sys/dev/microcode/fxp/rcvbundl.h`.
pub(super) fn fxp_firmware(src: &Path) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let path = src.join("sys/dev/microcode/fxp/rcvbundl.h");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    fxp_firmware_from(&text)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "#define     D101_A_RCVBUNDLE_UCODE \\\n{\\\n0x03B301BB, \\\n0x0046FFFF, \\\n}\n\
        #define D101_B0_RCVBUNDLE_UCODE \\\n{\\\n0x00000001, \\\n}\n\
        #define D101M_B_RCVBUNDLE_UCODE {0x2}\n#define D101S_RCVBUNDLE_UCODE {0x3}\n\
        #define D102_B_RCVBUNDLE_UCODE {0x4}\n#define D102_C_RCVBUNDLE_UCODE {0x5}\n\
        #define D102_E_RCVBUNDLE_UCODE {0x6}\n";

    #[test]
    fn dwords_are_written_little_endian_in_build_c_order() {
        let files = fxp_firmware_from(HEADER).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.0).collect();
        assert_eq!(
            names,
            [
                "fxp-d101a",
                "fxp-d101b0",
                "fxp-d101ma",
                "fxp-d101s",
                "fxp-d102",
                "fxp-d102c",
                "fxp-d102e"
            ]
        );
        assert_eq!(files[0].1, [0xBB, 0x01, 0xB3, 0x03, 0xFF, 0xFF, 0x46, 0x00]);
        assert_eq!(files[1].1, [1, 0, 0, 0]);
    }

    /// The real header, when the reference tree is there: seven files of whole dwords that
    /// fit the download block of `fxp_load_ucode` (`MAXUCODESIZE`, 192 dwords).
    #[test]
    fn the_reference_header_gives_seven_whole_images() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/openbsd-src");
        if !src.join("sys/dev/microcode/fxp/rcvbundl.h").is_file() {
            return;
        }
        let files = fxp_firmware(&src).unwrap();
        assert_eq!(files.len(), 7);
        for (name, bytes) in files {
            assert!(!bytes.is_empty() && bytes.len() % 4 == 0, "{name}");
            assert!(bytes.len() / 4 <= 192, "{name}");
        }
    }

    #[test]
    fn a_missing_macro_is_an_error() {
        assert!(fxp_firmware_from("#define D101_A_RCVBUNDLE_UCODE {0x1}").is_err());
    }
}
/* </TESTS> */
