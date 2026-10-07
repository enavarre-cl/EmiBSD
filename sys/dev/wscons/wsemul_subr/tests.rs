use super::*;
use std::vec::Vec;

/// Decodes all of `bytes` in UTF-8 mode, one `wsemul_getchar` call per character, and
/// returns the characters and the errors that ended calls.
fn decode(bytes: &[u8], state: &mut WsemulInputstate) -> (Vec<u32>, Vec<Errno>) {
    let mut chars = Vec::new();
    let mut errs = Vec::new();
    let mut buf = bytes;
    while !buf.is_empty() {
        match wsemul_getchar(&mut buf, state, true) {
            Ok(()) => chars.push(state.inchar),
            Err(e) => errs.push(e),
        }
    }
    (chars, errs)
}

#[test]
fn getchar_decodes_utf8() {
    let mut st = WsemulInputstate::ZERO;
    let (chars, errs) = decode("aé€😀".as_bytes(), &mut st);
    assert_eq!(chars, [0x61, 0xe9, 0x20ac, 0x1f600]);
    assert!(errs.is_empty());
    assert_eq!(st.mbleft, 0);
}

#[test]
fn getchar_resumes_a_sequence_split_between_calls() {
    let mut st = WsemulInputstate::ZERO;
    let euro = "€".as_bytes();
    let mut buf = &euro[..2];
    assert_eq!(wsemul_getchar(&mut buf, &mut st, true), Err(Errno::EAGAIN));
    assert!(buf.is_empty());
    assert_eq!(st.mbleft, 1);
    let mut buf = &euro[2..];
    assert_eq!(wsemul_getchar(&mut buf, &mut st, true), Ok(()));
    assert_eq!(st.inchar, 0x20ac);
}

#[test]
fn getchar_rejects_ill_formed_sequences() {
    // A bad continuation byte aborts the sequence; a later character still decodes.
    let mut st = WsemulInputstate::ZERO;
    let mut buf: &[u8] = &[0xc3, 0x41, 0x42];
    assert_eq!(wsemul_getchar(&mut buf, &mut st, true), Ok(()));
    assert_eq!(st.inchar, 0x42);
    // Overlong, surrogate, past U+10FFFF, a stray continuation byte, 0xf8.
    for bad in [
        &[0xc0, 0x80][..],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0x80],
        &[0xf8],
    ] {
        let mut st = WsemulInputstate::ZERO;
        let mut buf = bad;
        assert_eq!(
            wsemul_getchar(&mut buf, &mut st, true),
            Err(Errno::EILSEQ),
            "{bad:x?}"
        );
        assert_eq!((st.inchar, st.mbleft), (0, 0));
    }
    let mut buf: &[u8] = &[];
    assert_eq!(wsemul_getchar(&mut buf, &mut st, true), Err(Errno::EAGAIN));
}

#[test]
fn getchar_without_utf8_takes_bytes() {
    let mut st = WsemulInputstate::ZERO;
    let mut buf: &[u8] = &[0xc3, 0xa9];
    assert_eq!(wsemul_getchar(&mut buf, &mut st, false), Ok(()));
    assert_eq!((st.inchar, buf.len()), (0xc3, 1));
}

#[test]
fn utf8_translate_encodes() {
    let mut out = [0u8; WSEMUL_TRANSLATE_SIZE];
    for (c, s) in [('a', "a"), ('é', "é"), ('€', "€"), ('😀', "😀")] {
        let n = wsemul_utf8_translate(c as u32, KB_US, &mut out, true);
        assert_eq!(&out[..n], s.as_bytes());
    }
    assert_eq!(wsemul_utf8_translate(0xd800, KB_US, &mut out, true), 0);
    assert_eq!(wsemul_utf8_translate(0x110000, KB_US, &mut out, true), 0);
}

#[test]
fn local_translate_follows_the_layout() {
    let mut out = [0u8; WSEMUL_TRANSLATE_SIZE];
    let mut tr = |u: u32, layout: KbdT| {
        assert_eq!(wsemul_utf8_translate(u, layout, &mut out, false), 1);
        out[0]
    };
    assert_eq!(tr(0x0410, KB_RU), 0xe1); // Cyrillic A to KOI8
    assert_eq!(tr(0x0451, KB_RU), 0xa3); // io
    assert_eq!(tr(0x0400, KB_RU), 0x00); // IE grave: no KOI8 code, low byte kept
    assert_eq!(tr(u32::from(KS_Cyrillic_GHEUKR), KB_UA), 0xbd);
    assert_eq!(tr(0x0105, KB_PL), 0xb1); // a ogonek to Latin-2
    assert_eq!(tr(0x0105, KB_LT), 0xe0); // a ogonek to Latin-7
    assert_eq!(tr(0x0105, KB_US), 0x05); // no hint: low byte
    assert_eq!(tr(u32::from(KS_L5_Gbreve), KB_TR), 0xd0);
    assert_eq!(tr(u32::from(KS_L2_caron), KB_SI), 0xb7);
    assert_eq!(tr(u32::from(KS_L7_AE), KB_LV), 0xaf);
    assert_eq!(tr(u32::from(KS_L7_dbllow9quot), KB_LT), 0xa5);
    assert_eq!(tr(0xe9, KB_FR), 0xe9);
}

/// The tables against `wsemul_subr.c`'s.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn tables_match_the_c_file() {
    let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wsemul_subr.c");
    let text = std::fs::read_to_string(path).unwrap();
    for (name, ours) in [
        ("cyrillic_to_koi8", &CYRILLIC_TO_KOI8[..]),
        ("unicode_to_latin2", &UNICODE_TO_LATIN2[..]),
        ("unicode_to_latin7", &UNICODE_TO_LATIN7[..]),
    ] {
        let start = text.find(&std::format!("{name}[] = {{")).unwrap();
        let body = &text[start..];
        let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
        let values: Vec<u8> = body
            .lines()
            .map(|l| l.split("/*").next().unwrap())
            .flat_map(|l| l.split(','))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| u8::from_str_radix(t.trim_start_matches("0x"), 16).unwrap())
            .collect();
        assert_eq!(values, ours, "{name}");
    }
}
