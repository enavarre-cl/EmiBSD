use super::*;

fn kinds(src: &str) -> Vec<(Kind, bool)> {
    scan(&lex(src), false).found
}

fn idents(src: &str) -> Vec<String> {
    lex(src)
        .into_iter()
        .filter_map(|t| match t {
            Tok::Ident(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn comments_and_strings_hide_unsafe() {
    let src = r####"
        // unsafe { line comment }
        /* unsafe /* nested unsafe */ still unsafe */
        /// doc: unsafe fn
        let a = "unsafe { in a string \" unsafe }";
        let b = r#"raw "unsafe" { }"#;
        let c = br##"raw bytes "# unsafe"##;
        let d = b"unsafe";
        let e = c"unsafe";
        let f = '"';
        let g = b'"';
        let h = '\'';
        let i = '\u{1F600}';
    "####;
    assert!(kinds(src).is_empty());
    assert!(!idents(src).contains(&"unsafe".to_string()));
}

#[test]
fn lifetimes_are_not_char_literals() {
    // Were 'a read as the start of a char literal, `unsafe` would be swallowed.
    let src = "fn f<'a>(x: &'a u8) -> &'a u8 { unsafe { g(x) } } 'outer: loop {}";
    assert_eq!(kinds(src), vec![(Kind::Block, false)]);
}

#[test]
fn raw_identifier_is_not_the_keyword() {
    assert!(kinds("let r#unsafe = 1;").is_empty());
    assert_eq!(idents("r#unsafe"), vec!["r#unsafe".to_string()]);
}

#[test]
fn numbers_and_ranges() {
    let src = "let x = 1.5e3 + 0x1F_u32 as f64; for i in 0..2 { unsafe { f(i) } }";
    assert_eq!(kinds(src), vec![(Kind::Block, false)]);
    assert!(idents(src).contains(&"i".to_string()));
}

#[test]
fn each_kind_is_classified() {
    let src = r#"
        unsafe fn a() {}
        pub(crate) const unsafe fn b() {}
        unsafe extern "C" fn c() {}
        unsafe extern "C" { fn d(); }
        unsafe impl Send for X {}
        unsafe trait T {}
        unsafe auto trait U {}
        #[unsafe(no_mangle)]
        fn e() { unsafe { f() } }
        type P = unsafe fn(u8);
        type Q = unsafe extern "C" fn();
    "#;
    let k: Vec<Kind> = kinds(src).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        k,
        vec![
            Kind::Fn,
            Kind::Fn,
            Kind::Fn,
            Kind::Other,
            Kind::Impl,
            Kind::Trait,
            Kind::Trait,
            Kind::Other,
            Kind::Block,
            Kind::Other,
            Kind::Other,
        ]
    );
}

#[test]
fn cfg_test_items_count_as_tests() {
    let src = r#"
        unsafe fn kernel() {}
        #[cfg(test)]
        mod tests {
            #[test]
            fn t() { unsafe { x() } }
        }
        #[cfg(test)]
        fn helper(a: [u8; 4]) { unsafe { y() } }
        #[cfg(any(test, feature = "debug"))]
        fn both() { unsafe { z() } }
        #[cfg(all(test, feature = "alloc"))]
        #[allow(dead_code)]
        unsafe impl Sync for W {}
        #[cfg(not(test))]
        fn not_test() { unsafe { w() } }
        #[cfg(test)]
        use std::vec::Vec;
        unsafe impl Send for V {}
    "#;
    assert_eq!(
        kinds(src),
        vec![
            (Kind::Fn, false),
            (Kind::Block, true),
            (Kind::Block, true),
            (Kind::Block, false),
            (Kind::Impl, true),
            (Kind::Block, false),
            (Kind::Impl, false),
        ]
    );
}

#[test]
fn test_mod_declarations_are_found() {
    let src = "#[cfg(test)]\nmod tests;\n#[cfg(test)]\npub(crate) mod reftest;\nmod real;\n";
    let s = scan(&lex(src), false);
    assert_eq!(
        s.test_mods,
        vec!["tests".to_string(), "reftest".to_string()]
    );
    // Inside a test file every `mod x;` is a test file.
    let s = scan(&lex("mod helpers;\nunsafe fn f() {}"), true);
    assert_eq!(s.test_mods, vec!["helpers".to_string()]);
    assert_eq!(s.found, vec![(Kind::Fn, true)]);
}

#[test]
fn inner_cfg_test_marks_the_file() {
    let s = scan(&lex("#![cfg(test)]\nunsafe fn f() {}"), false);
    assert_eq!(s.found, vec![(Kind::Fn, true)]);
}

#[test]
fn child_paths() {
    let [a, b] = child_module_paths(Path::new("sys/kern/mod.rs"), "tty");
    assert_eq!(a, Path::new("sys/kern/tty.rs"));
    assert_eq!(b, Path::new("sys/kern/tty/mod.rs"));
    let [a, _] = child_module_paths(Path::new("sys/kern/tty.rs"), "tests");
    assert_eq!(a, Path::new("sys/kern/tty/tests.rs"));
}

#[test]
fn subsystems() {
    let root = Path::new("/nonexistent");
    assert_eq!(subsystem(root, "sys/kern/tty.rs"), "kern");
    assert_eq!(
        subsystem(root, "sys/arch/amd64/amd64/pmap.rs"),
        "arch/amd64"
    );
    assert_eq!(subsystem(root, "sys/lib/libkern/strlcpy.rs"), "lib/libkern");
    assert_eq!(subsystem(root, "sys/ufs/ffs/ffs_alloc.rs"), "ufs");
    assert_eq!(subsystem(root, "sys/dev/softraid.rs"), "dev");
    assert_eq!(subsystem(root, "sys/lib.rs"), "(crate root)");
    assert_eq!(subsystem(root, "init/src/main.rs"), "init");
}

#[test]
fn status_line_is_replaced() {
    let doc = "# Status\n\nUnsafe old totals.\nNext:\n";
    let new = replace_status_line(doc, "Unsafe new.").unwrap();
    assert_eq!(new, "# Status\n\nUnsafe new.\nNext:\n");
    assert!(replace_status_line("# Status\n", "Unsafe x").is_err());
}
