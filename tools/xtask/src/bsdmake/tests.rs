use super::*;

fn make_from(text: &str) -> Make {
    let mut m = Make::new(
        Path::new("/nonexistent"),
        &[("MACHINE_CPU", "amd64".to_string())],
        &[("bsd.own.mk", "YP?= yes\n")],
    );
    let mut p = Parser {
        conds: Vec::new(),
        open_rule: None,
    };
    m.parse_text(text, "test.mk", None, &mut p).unwrap();
    assert!(p.conds.is_empty());
    m
}

#[test]
fn assignments_are_lazy_except_colon_equals() {
    let m = make_from("A= x ${B}\nB= one\nC:= ${B}\nB= two\nD+= d1\nD+= d2\nB?= three\n");
    assert_eq!(m.var("A").unwrap(), "x two");
    assert_eq!(m.var("C").unwrap(), "one");
    assert_eq!(m.var("D").unwrap(), "d1 d2");
}

#[test]
fn modifiers() {
    let m = make_from(
        "L= a.o b.o c.c\nCANCEL= read write\nCF= -Ifoo -DX -O2 -include n.h -pipe\n\
         V= Yes\nP= ___realpath _exit\n",
    );
    assert_eq!(m.expand("${L:.o=.po}").unwrap(), "a.po b.po c.c");
    assert_eq!(m.expand("${CANCEL:%=w_%.c}").unwrap(), "w_read.c w_write.c");
    assert_eq!(m.expand("${CANCEL:=.o}").unwrap(), "read.o write.o");
    assert_eq!(m.expand("${L:N*.c:R}").unwrap(), "a b");
    assert_eq!(m.expand("${L:M*.c}").unwrap(), "c.c");
    assert_eq!(m.expand("${CF:M-[ID]*}").unwrap(), "-Ifoo -DX");
    assert_eq!(m.expand("${V:L}").unwrap(), "yes");
    assert_eq!(m.expand("${P:S/^_//}").unwrap(), "__realpath exit");
    assert_eq!(m.expand("${L:R:S/$/.o/}").unwrap(), "a.o b.o c.o");
    assert_eq!(m.expand("${SRCS_${MACHINE_CPU}}x$$y").unwrap(), "x$y");
}

#[test]
fn conditionals_and_for() {
    let m = make_from(
        ".include <bsd.own.mk>\n\
         .if (${YP:L} == \"yes\")\nA= yp\n.else\nA= noyp\n.endif\n\
         .if ${MACHINE_CPU} == \"i386\" || ${MACHINE_CPU} == \"arm\"\nB= 32\n\
         .elif ${MACHINE_CPU} == \"amd64\"\nB= 64\n.else\nB= other\n.endif\n\
         .ifndef UNDEF\nC= c\n.endif\n.if !defined(UNDEF) && !empty(A)\nD= d\n.endif\n\
         ASM= a.o b.o c.o\nOVR= b.S c.S\n.for i in ${OVR}\nASM:= ${ASM:N${i:R}.o}\n.endfor\n",
    );
    assert_eq!(m.var("A").unwrap(), "yp");
    assert_eq!(m.var("B").unwrap(), "64");
    assert_eq!(m.var("C").unwrap(), "c");
    assert_eq!(m.var("D").unwrap(), "d");
    assert_eq!(m.var("ASM").unwrap(), "a.o");
}

#[test]
fn for_with_two_variables_and_dependency_only_rules() {
    let m = make_from(
        "SSLASM= aes aes-x86_64 bn x86_64-mont\n\
         .for dir f in ${SSLASM}\nSRCS+= ${dir}/${f}.S\n${f}.S: ${dir}/asm/${f}.pl\n\
         \tperl ./asm/${f}.pl > ${.TARGET}\n.endfor\n\
         includes: prereq\nprereq: obj_mac.h\n",
    );
    assert_eq!(m.var("SRCS").unwrap(), "aes/aes-x86_64.S bn/x86_64-mont.S");
    assert_eq!(m.sources_of("x86_64-mont.S"), ["bn/asm/x86_64-mont.pl"]);
    assert_eq!(m.sources_of("includes"), ["prereq"]);
    assert_eq!(m.sources_of("prereq"), ["obj_mac.h"]);
    assert!(m.rule_for("prereq").is_none());
    let mut p = Parser {
        conds: Vec::new(),
        open_rule: None,
    };
    let mut odd = make_from("");
    assert!(
        odd.parse_text(
            "L= a b c\n.for x y in ${L}\n.endfor\n",
            "t.mk",
            None,
            &mut p
        )
        .is_err()
    );
}

#[test]
fn rules_paths_and_continuations() {
    let m = make_from(
        "SRCS+= a.c \\\n\tb.c\nGEN=\\t.file \"${@:R}.S\"\\n\\#include \"SYS.h\" # comment\n\
         .PATH: /x/y\nOBJ= a.o b.o\n${OBJ}: ; @echo ${GEN}\n\nh.c: helper.c\n\tsed -e 's/A/B/' $> > $@\n",
    );
    assert_eq!(m.var("SRCS").unwrap(), "a.c b.c");
    let mut locals = Locals::new();
    locals.insert("@".into(), "access.o".into());
    assert_eq!(
        m.expand_local("${GEN}", &locals).unwrap(),
        "\\t.file \"access.S\"\\n#include \"SYS.h\""
    );
    assert_eq!(m.path, vec![PathBuf::from("/x/y")]);
    assert_eq!(m.rule_for("b.o").unwrap().commands, vec!["@echo ${GEN}"]);
    let r = m.rule_for("h.c").unwrap();
    assert_eq!(r.sources, vec!["helper.c"]);
    assert_eq!(r.commands, vec!["sed -e 's/A/B/' $> > $@"]);
}

#[test]
fn unknown_constructs_fail_loudly() {
    let mut m = Make::new(Path::new("/nonexistent"), &[], &[]);
    let mut p = Parser {
        conds: Vec::new(),
        open_rule: None,
    };
    assert!(m.parse_text(".undef X\n", "t", None, &mut p).is_err());
    assert!(m.parse_text("X!= ls\n", "t", None, &mut p).is_err());
    assert!(
        m.parse_text("Y= ${X:Q}\nZ:= ${Y}\n", "t", None, &mut p)
            .is_err()
    );
    assert!(
        m.parse_text(".include <bsd.prog.mk>\n", "t", None, &mut p)
            .is_err()
    );
}

#[test]
fn glob_matches_like_make() {
    assert!(glob(b"-[ID]*", b"-I/usr/include"));
    assert!(!glob(b"-[ID]*", b"-include"));
    assert!(glob(b"*.h", b"x.h"));
    assert!(glob(b"?x[!a-c]", b"yxd"));
    assert!(!glob(b"?x[!a-c]", b"yxb"));
}

/// `.PATH` lookups match the file name exactly, even where the file system ignores case.
#[test]
fn search_is_case_exact() {
    let dir = std::env::temp_dir().join(format!("emibsd-bsdmake-case-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("a")).unwrap();
    fs::create_dir_all(dir.join("b")).unwrap();
    fs::write(dir.join("a/DwarfUnit.cpp"), "").unwrap();
    fs::write(dir.join("b/DWARFUnit.cpp"), "").unwrap();
    std::os::unix::fs::symlink(dir.join("a/DwarfUnit.cpp"), dir.join("b/Link.cpp")).unwrap();
    let mut m = Make::new(&dir, &[], &[]);
    m.add_path(&dir.join("a"));
    m.add_path(&dir.join("b"));
    assert_eq!(m.search("DWARFUnit.cpp"), Some(dir.join("b/DWARFUnit.cpp")));
    assert_eq!(m.search("DwarfUnit.cpp"), Some(dir.join("a/DwarfUnit.cpp")));
    assert_eq!(m.search("Link.cpp"), Some(dir.join("b/Link.cpp")));
    assert_eq!(m.search("dwarfunit.cpp"), None);
    let _ = fs::remove_dir_all(&dir);
}

/// A whole-line comment ends at its newline even after a backslash, as in OpenBSD's make
/// (libclangASTMatchers's Makefile); a backslash after an assignment still continues it.
#[test]
fn comment_lines_do_not_continue() {
    let m = make_from(
        "#CPPFLAGS+=\t-Ifoo \\\nCPPFLAGS+=\t-Ibar\n  # indented \\\nA= a \\\n  b # c \\\nB= x\n",
    );
    assert_eq!(m.var("CPPFLAGS").unwrap(), "-Ibar");
    assert_eq!(m.var("A").unwrap(), "a b");
    assert!(!m.defined("B"));
}
