//! Host tests of the keysym helpers: the upper case, the compose table, the map entries, and
//! the keymaps of `ukbdmap.rs`'s layouts built both ways (`wskbd_load_keymap` against
//! `wskbd_get_mapentry` for every key of every layout).

use super::*;
use crate::dev::usb::ukbdmap::UKBD_KEYDESCTAB;
use crate::kern::kern_malloc::free;
use crate::kern::subr_pool::tests::setup_real_memory;

/// The entries of a map `wskbd_load_keymap` built, freed after `f`.
fn with_map(layout: KbdT, f: impl FnOnce(&[WsconsKeymap])) {
    let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, layout);
    let (map, len) = wskbd_load_keymap(&md, layout).unwrap();
    // SAFETY: the map `wskbd_load_keymap` made, `len` entries.
    f(unsafe { core::slice::from_raw_parts(map.as_ptr(), len) });
    free(map.cast(), M_DEVBUF, len * size_of::<WsconsKeymap>());
}

#[test]
fn upcase_of_letters_and_function_keys() {
    assert_eq!(ksym_upcase(KS_a), KS_A);
    assert_eq!(ksym_upcase(KS_z), KS_Z);
    assert_eq!(ksym_upcase(KS_agrave), KS_Agrave);
    assert_eq!(ksym_upcase(KS_thorn), KS_THORN);
    assert_eq!(
        ksym_upcase(KS_ydiaeresis),
        KS_ydiaeresis,
        "no Latin-1 capital"
    );
    assert_eq!(ksym_upcase(KS_division), KS_division);
    assert_eq!(ksym_upcase(KS_1), KS_1);
    assert_eq!(ksym_upcase(KS_A), KS_A);
    assert_eq!(ksym_upcase(KS_f1), KS_F1);
    assert_eq!(ksym_upcase(KS_f20), KS_F20);
    assert_eq!(ksym_upcase(KS_Return), KS_Return);
}

#[test]
fn compose_table_is_sorted_and_every_pair_is_found() {
    let sorted = &COMPOSE_TAB_SORTED;
    for w in sorted.windows(2) {
        assert!(compose_tab_cmp(&w[0], &w[1]) <= 0, "{w:?}");
    }
    for e in &COMPOSE_TAB {
        assert!(sorted.contains(e));
        assert_eq!(wskbd_compose_value(&e.elem), e.result, "{e:?}");
    }
    assert_eq!(wskbd_compose_value(&[KS_a, KS_a]), KS_at);
    assert_eq!(wskbd_compose_value(&[KS_dead_acute, KS_e]), KS_eacute);
    assert_eq!(wskbd_compose_value(&[KS_dead_caron, KS_s]), KS_L2_scaron);
    assert_eq!(wskbd_compose_value(&[KS_e, KS_dead_acute]), KS_voidSymbol);
    assert_eq!(wskbd_compose_value(&[KS_x, KS_x]), KS_voidSymbol);
}

#[test]
fn fillmapentry_by_number_of_keysyms() {
    let mut m = WsconsKeymap::default();
    fillmapentry(&[], &mut m);
    assert_eq!(
        (m.group1, m.group2),
        ([KS_voidSymbol; 2], [KS_voidSymbol; 2])
    );
    fillmapentry(&[KS_b], &mut m);
    assert_eq!((m.group1, m.group2), ([KS_b, KS_B], [KS_b, KS_B]));
    fillmapentry(&[KS_1, KS_exclam], &mut m);
    assert_eq!((m.group1, m.group2), ([KS_1, KS_exclam], [KS_1, KS_exclam]));
    fillmapentry(&[KS_q, KS_Q, KS_at], &mut m);
    assert_eq!((m.group1, m.group2), ([KS_q, KS_Q], [KS_at, KS_at]));
    fillmapentry(&[KS_e, KS_E, KS_currency, KS_cent], &mut m);
    assert_eq!((m.group1, m.group2), ([KS_e, KS_E], [KS_currency, KS_cent]));
}

#[test]
fn us_keymap() {
    let _g = setup_real_memory();
    with_map(KB_US | KB_DEFAULT, |map| {
        assert_eq!(map.len(), 237, "up to KC(236)");
        assert_eq!(map[4].group1, [KS_a, KS_A]);
        assert_eq!(map[4].command, KS_voidSymbol);
        assert_eq!(map[30].group1, [KS_1, KS_exclam]);
        assert_eq!(map[40].group1[0], KS_Return);
        assert_eq!(map[41].command, KS_Cmd_Debugger);
        assert_eq!(map[41].group1[0], KS_Escape);
        assert_eq!(map[58].command, KS_Cmd_Screen0);
        assert_eq!(map[58].group1, [KS_f1, KS_F1]);
        assert_eq!(map[89].group1, [KS_KP_End, KS_KP_1]);
        assert_eq!(map[224].command, KS_Cmd1);
        assert_eq!(map[224].group1[0], KS_Control_L);
        assert_eq!(map[0].group1, [KS_voidSymbol; 2], "an unused code");
    });
}

#[test]
fn german_keymap_is_a_delta_over_us() {
    let _g = setup_real_memory();
    with_map(KB_DE, |map| {
        assert_eq!(map[28].group1, [KS_z, KS_Z], "y and z swap");
        assert_eq!(map[29].group1, [KS_y, KS_Y]);
        assert_eq!(map[4].group1, [KS_a, KS_A], "from the base");
        assert_eq!(map[20].group2, [KS_at, KS_at]);
        assert_eq!(map[46].group1, [KS_dead_acute, KS_dead_grave]);
    });
    // A variant over a variant: de.nodead over de over us.
    with_map(KB_DE | KB_NODEAD, |map| {
        assert_eq!(map[28].group1, [KS_z, KS_Z]);
        assert_ne!(map[46].group1[0], KS_dead_acute);
    });
}

/// Whether a layer of `layout`'s chain has two entries for key `kc`: the lookup takes the
/// first, the load the last (`fr.apple` redefines key 37), in C too.
fn defined_twice(layout: KbdT, kc: u32) -> bool {
    let mut cur = layout;
    while cur != 0 {
        let Some(mp) = find_keydesc(&UKBD_KEYDESCTAB, cur) else {
            return false;
        };
        let code = ks_keycode(kc) as KeysymT;
        if mp.map.iter().filter(|&&k| k == code).count() > 1 {
            return true;
        }
        cur = mp.base;
    }
    false
}

#[test]
fn every_layout_loads_and_matches_the_lookup_of_each_key() {
    // The lookup stops at the first layer that has the key, the load fills each layer over
    // the one below: a command of the base stays in the loaded map where a layer redefines
    // the key without one, and a key a layer has twice is its first entry for the lookup,
    // its last for the load (the C's two functions differ there too).
    let _g = setup_real_memory();
    for kd in &UKBD_KEYDESCTAB {
        with_map(kd.name, |map| {
            let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, kd.name);
            for (kc, want) in map.iter().enumerate() {
                if defined_twice(kd.name, kc as u32) {
                    continue;
                }
                let mut got = WsconsKeymap::default();
                wskbd_get_mapentry(&md, kc as i32, &mut got);
                assert_eq!(
                    (got.group1, got.group2),
                    (want.group1, want.group2),
                    "layout {:#x} key {kc}",
                    kd.name
                );
                if got.command != KS_voidSymbol {
                    assert_eq!(got.command, want.command, "layout {:#x} key {kc}", kd.name);
                }
            }
        });
    }
}

#[test]
fn unknown_layout_is_einval() {
    let _g = setup_real_memory();
    let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, KB_US);
    assert_eq!(
        wskbd_load_keymap(&md, KB_HU | KB_APPLE).err(),
        Some(Errno::EINVAL)
    );
    let mut e = WsconsKeymap::default();
    let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, KB_HU | KB_APPLE);
    wskbd_get_mapentry(&md, 4, &mut e);
    assert_eq!(e.group1, [KS_voidSymbol; 2]);
}

#[test]
fn init_keymap_is_void() {
    let _g = setup_real_memory();
    let map = wskbd_init_keymap(3).unwrap();
    // SAFETY: three entries, just made.
    let m = unsafe { core::slice::from_raw_parts(map.as_ptr(), 3) };
    for e in m {
        assert_eq!(e.command, KS_voidSymbol);
        assert_eq!(e.group2, [KS_voidSymbol; 2]);
    }
    free(map.cast(), M_DEVBUF, 3 * size_of::<WsconsKeymap>());
}

/// The compose table against the C's: same pairs, same results, same order.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn compose_table_matches_the_c() {
    let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wskbdutil.c");
    let text = std::fs::read_to_string(path).unwrap();
    let defs = crate::reftest::defines("sys/dev/wscons/wsksymdef.h");
    let ks = |name: &str| crate::reftest::int(&defs, name).unwrap() as KeysymT;
    let mut c = std::vec::Vec::new();
    for line in text.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("{ {") else {
            continue;
        };
        let names: std::vec::Vec<&str> = rest
            .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .filter(|s| s.starts_with("KS_"))
            .collect();
        assert_eq!(names.len(), 3, "{l}");
        c.push(ct(ks(names[0]), ks(names[1]), ks(names[2])));
    }
    assert_eq!(&c[..], &COMPOSE_TAB[..]);
}
