//! Host tests of the layout tables: their shape, and (reference-backed) every keysym of every
//! table against the generated C.

use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use super::*;

/// Whether `k` is a key code (`KS_GROUP_Keycode`).
fn is_kc(k: KeysymT) -> bool {
    ks_group(u32::from(k)) == KS_GROUP_Keycode
}

#[test]
fn tables_are_entries_of_a_key_code_and_at_most_four_keysyms() {
    for kd in &UKBD_KEYDESCTAB {
        assert_eq!(kd.map_size as usize, kd.map.len());
        assert!(kd.map_size > 0);
        assert!(is_kc(kd.map[0]), "layout {:#x} starts with a key code", kd.name);
        let mut n = 0;
        for &k in &kd.map[1..] {
            if is_kc(k) {
                n = 0;
            } else {
                n += 1;
                assert!(n <= 5, "layout {:#x}: command + four keysyms at most", kd.name);
            }
        }
    }
}

#[test]
fn every_base_is_a_layout_and_names_are_unique() {
    let names: Vec<u32> = UKBD_KEYDESCTAB.iter().map(|k| k.name).collect();
    for (i, kd) in UKBD_KEYDESCTAB.iter().enumerate() {
        assert!(!names[..i].contains(&kd.name), "{:#x} twice", kd.name);
        assert!(kd.base == 0 || names.contains(&kd.base), "{:#x}", kd.base);
    }
    assert_eq!(UKBD_KEYDESCTAB[0].name, KB_US);
    assert_eq!(UKBD_KEYDESCTAB[0].base, 0);
    assert_eq!(kc(4), 0xe004);
}

/// The tokens of a C initialiser body: `KC(n)` and keysym names, comments dropped.
fn c_tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut s = body;
    while let Some(i) = s.find("/*") {
        let j = s[i..].find("*/").map_or(s.len(), |j| i + j + 2);
        out.extend(tokens_of(&s[..i]));
        s = &s[j..];
    }
    out.extend(tokens_of(s));
    out
}

fn tokens_of(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

/// Every table and the layout list against `ukbdmap.c`: the same keysyms, keysym by keysym,
/// with `KS_*` and `KB_*` evaluated from `wsksymdef.h`.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn tables_match_the_c() {
    let path = crate::reftest::openbsd_src().join("sys/dev/usb/ukbdmap.c");
    let text = std::fs::read_to_string(path).unwrap();
    let defs = crate::reftest::defines("sys/dev/wscons/wsksymdef.h");
    let val = |tok: &str| -> u32 {
        if let Some(n) = tok.strip_prefix("KC(").and_then(|t| t.strip_suffix(')')) {
            return ks_keycode(n.parse().unwrap());
        }
        let mut v = 0;
        for part in tok.split('|') {
            let part = part.trim();
            v |= if part == "0" {
                0
            } else {
                crate::reftest::int(&defs, part).unwrap_or_else(|| panic!("{part}")) as u32
            };
        }
        v
    };

    // The arrays, by name.
    let mut tables: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut rest = &text[..];
    while let Some(i) = rest.find("static const keysym_t ") {
        let after = &rest[i + "static const keysym_t ".len()..];
        let name = &after[..after.find('[').unwrap()];
        let open = after.find('{').unwrap();
        let close = after.find("};").unwrap();
        let body = &after[open + 1..close];
        tables.insert(name.into(), c_tokens(body).iter().map(|t| val(t)).collect());
        rest = &after[close..];
    }
    assert_eq!(tables.len(), 45);

    // The layout list.
    let start = text.find("ukbd_keydesctab[] = {").unwrap();
    let list = &text[start..start + text[start..].find("};").unwrap()];
    let mut entries = Vec::new();
    for chunk in list.split("KBD_MAP(").skip(1) {
        let args = &chunk[..chunk.find(')').unwrap()];
        let args: Vec<&str> = args.split(',').map(str::trim).collect();
        entries.push((val(args[0]), val(args[1]), args[2].to_string()));
    }
    assert_eq!(entries.len(), UKBD_KEYDESCTAB.len());
    for ((name, base, map), kd) in entries.iter().zip(&UKBD_KEYDESCTAB) {
        assert_eq!((kd.name, kd.base), (*name, *base), "{map}");
        let ours: Vec<u32> = kd.map.iter().map(|&k| u32::from(k)).collect();
        assert_eq!(&ours, &tables[map], "{map}");
    }
}
