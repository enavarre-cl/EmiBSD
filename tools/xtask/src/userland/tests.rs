use super::*;

#[test]
fn licence_families_are_recognised() {
    let isc = "Permission to use, copy, modify, and distribute this software for any\n\
               * purpose with or without fee is hereby granted";
    assert_eq!(licence_families(isc), vec!["ISC"]);
    let bsd3 = "Redistribution and use in source and binary forms ...\n\
                * 3. Neither the name of the University nor";
    assert_eq!(licence_families(bsd3), vec!["BSD-3-Clause"]);
    let bsd4 = "Redistribution and use in source and binary forms\n\
                * 3. All advertising materials mentioning features";
    assert_eq!(licence_families(bsd4), vec!["BSD-4-Clause"]);
    assert_eq!(
        licence_families("This file is in the public domain."),
        vec!["public domain"]
    );
    assert_eq!(licence_families("int x;"), vec!["no licence text"]);
}

#[test]
fn compiler_builtins() {
    for s in [
        "__multf3",
        "__extenddftf2",
        "__fixtfsi",
        "__udivti3",
        "__emutls_get_address",
    ] {
        assert!(is_compiler_builtin(s), "{s}");
    }
    for s in ["_exit", "main", "__start", "__libc_init"] {
        assert!(!is_compiler_builtin(s), "{s}");
    }
}

#[test]
fn depfiles_and_locals() {
    let d = "x.o: /a/x.c /a/b.h \\\n  /a/c.h\n\n/a/b.h:\n";
    assert_eq!(
        parse_depfile(d),
        vec![
            PathBuf::from("/a/x.c"),
            PathBuf::from("/a/b.h"),
            PathBuf::from("/a/c.h")
        ]
    );
    let l = locals("___realpath.o", &[PathBuf::from("/s/helper.c")]);
    assert_eq!(l["@"], "___realpath.o");
    assert_eq!(l[".PREFIX"], "___realpath");
    assert_eq!(l[">"], "/s/helper.c");
}

/// Evaluates the real `lib/libc` Makefiles for amd64: `cargo test -p xtask -- --ignored`
/// with `$OPENBSD_SRC` naming the reference clone.
#[test]
#[ignore]
fn libc_makefile_evaluates() {
    let src = PathBuf::from(std::env::var("OPENBSD_SRC").expect("OPENBSD_SRC"));
    let curdir = src.join("lib/libc");
    let predefined = [
        ("MACHINE", "amd64".to_string()),
        ("MACHINE_ARCH", "amd64".to_string()),
        ("MACHINE_CPU", "amd64".to_string()),
        ("CFLAGS", "-O2".to_string()),
    ];
    let sys_mk = [
        ("bsd.own.mk", BSD_OWN_MK),
        ("bsd.prog.mk", BSD_PROG_MK),
        ("bsd.lib.mk", BSD_PROG_MK),
    ];
    let mut mk = Make::new(&curdir, &predefined, &sys_mk);
    mk.read(&curdir.join("Makefile")).unwrap();
    let words = |v: &str| mk.words(v).unwrap();
    assert!(words("ASM").contains(&"access.o".to_string()));
    assert!(words("HIDDEN").contains(&"read.o".to_string()));
    assert!(words("PSEUDO_NOERR").contains(&"_exit.o".to_string()));
    assert!(words("SRCS").contains(&"w_read.c".to_string()));
    assert!(words("SRCS").contains(&"md5hl.c".to_string()));
    assert!(words("CFLAGS").contains(&"-fret-clean".to_string()));
    assert!(words("CFLAGS").contains(&"-DYP".to_string()));
    assert!(mk.search("_atomic_lock.c").is_some());
    assert!(mk.rule_for("md5hl.c").is_some());
    assert!(mk.rule_for("access.o").is_some());
}
