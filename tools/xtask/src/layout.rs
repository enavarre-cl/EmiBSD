/* <CODE> */
//! The zone markers of every `.rs` under `sys/` and `tools/` (milestone M15).
//!
//! A file is split into zones, each opened and closed by a comment line of its own, in this
//! order:
//!
//! ```text
//! /* $OpenBSD: ... */           ported files: the id lines, then
//! /* <LICENSES> */              the original notice, verbatim
//! /* </LICENSES> */
//! /* <CODE> */                  the `//!` docs, inner attributes, `use` lines, code
//! /* </CODE> */
//! /* <TESTS> */                 the tests, inline, at the end (only in files that have tests)
//! /* </TESTS> */
//! ```
//!
//! [`check`] validates one file; `cargo xtask ports check` runs it over the tree with the
//! licence policy `ports.toml` gives each path. Reading rule:
//! `sed -n '/<CODE>/,/<\/CODE>/p' file.rs`.
//!
//! Outside the zones there are only blank lines and, before the first zone, block comments (the
//! `$OpenBSD$` id lines, a "generated, do not edit" banner). Anything else is an error.
//! "Has test code" means the file declares a `mod tests` (inline); a `#[test]` or a `mod tests`
//! in the CODE zone is an error, as is a TESTS zone without a `mod tests`.

/// What a path may do with the LICENSES zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Licenses {
    /// Not the port of a C file: the zone must be absent.
    Forbidden,
    /// The port of a C file that carries no licence text upstream (`license = "none"`).
    Allowed,
    /// The port of a C file: the zone must be present.
    Required,
}

const NAMES: [&str; 3] = ["LICENSES", "CODE", "TESTS"];
const LICENSES: usize = 0;
const CODE: usize = 1;
const TESTS: usize = 2;

/// The zone a marker line opens (`Some((zone, true))`) or closes (`Some((zone, false))`).
fn marker(line: &str) -> Option<(usize, bool)> {
    let t = line.trim_end();
    NAMES.iter().position(|n| t == format!("/* <{n}> */")).map_or_else(
        || {
            NAMES
                .iter()
                .position(|n| t == format!("/* </{n}> */"))
                .map(|z| (z, false))
        },
        |z| Some((z, true)),
    )
}

/// Does `line` declare the module `tests` (`mod tests {`, `pub(crate) mod tests;`)?
fn declares_tests_mod(line: &str) -> bool {
    let t = line.trim_start();
    let t = t.strip_prefix("pub(crate) ").unwrap_or(t);
    let t = t.strip_prefix("pub ").unwrap_or(t);
    t.strip_prefix("mod tests")
        .is_some_and(|r| r.trim_start().starts_with(['{', ';']))
}

/// Validate the zones of one file; `rel` only labels the messages. Returns the errors.
pub(crate) fn check(rel: &str, src: &str, lic: Licenses) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    // (opening line, closing line) of each zone, 1-based
    let mut seen: [(Option<usize>, Option<usize>); 3] = [(None, None); 3];
    let mut cur: Option<usize> = None;
    let mut last_opened: Option<usize> = None;
    let mut in_block = false;
    let mut first_zone_seen = false;
    let mut content: [Vec<&str>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    for (i, line) in src.lines().enumerate() {
        let n = i + 1;
        match marker(line) {
            Some((z, true)) => {
                let name = NAMES[z];
                if let Some(c) = cur {
                    let cname = NAMES[c];
                    errs.push(format!("{rel}:{n}: <{name}> opened inside <{cname}>"));
                } else if seen[z].0.is_some() {
                    errs.push(format!("{rel}:{n}: <{name}> opened twice"));
                } else if last_opened.is_some_and(|l| l > z) {
                    errs.push(format!("{rel}:{n}: <{name}> is out of order"));
                }
                if cur.is_none() {
                    cur = Some(z);
                }
                seen[z].0.get_or_insert(n);
                last_opened = Some(last_opened.map_or(z, |l| l.max(z)));
                first_zone_seen = true;
            }
            Some((z, false)) => {
                let name = NAMES[z];
                if cur == Some(z) {
                    if seen[z].1.is_some() {
                        errs.push(format!("{rel}:{n}: </{name}> closed twice"));
                    }
                    seen[z].1.get_or_insert(n);
                    cur = None;
                } else {
                    errs.push(format!("{rel}:{n}: </{name}> closes a zone that is not open"));
                }
            }
            None => match cur {
                Some(z) => content[z].push(line),
                None => {
                    let t = line.trim();
                    if t.is_empty() {
                        continue;
                    }
                    if !first_zone_seen && (in_block || t.starts_with("/*")) {
                        if t.starts_with("/*") && !in_block {
                            in_block = true;
                        }
                        if in_block && t.contains("*/") {
                            in_block = false;
                        }
                        continue;
                    }
                    errs.push(format!("{rel}:{n}: text outside the zones: {t}"));
                }
            },
        }
    }
    if let Some(c) = cur {
        let name = NAMES[c];
        errs.push(format!("{rel}: <{name}> is never closed"));
    }
    for (z, name) in NAMES.iter().enumerate() {
        if seen[z].0.is_some() && seen[z].1.is_none() && cur != Some(z) {
            errs.push(format!("{rel}: <{name}> has no closing marker"));
        }
    }
    if seen[CODE].0.is_none() {
        errs.push(format!("{rel}: no <CODE> zone"));
    }
    match (lic, seen[LICENSES].0.is_some()) {
        (Licenses::Forbidden, true) => {
            errs.push(format!(
                "{rel}: <LICENSES> in a file that is not the port of a C file (no [[file]] entry)"
            ));
        }
        (Licenses::Required, false) => {
            errs.push(format!(
                "{rel}: no <LICENSES> zone (a port keeps its notice; `license = \"none\"` in \
                 ports.toml when the C file has none)"
            ));
        }
        _ => {}
    }
    for l in &content[CODE] {
        if l.trim() == "#[test]" {
            errs.push(format!("{rel}: a #[test] in the <CODE> zone (tests go in <TESTS>)"));
            break;
        }
    }
    if content[CODE].iter().any(|l| declares_tests_mod(l)) {
        errs.push(format!("{rel}: `mod tests` in the <CODE> zone (it goes in <TESTS>)"));
    }
    if seen[TESTS].0.is_some() && !content[TESTS].iter().any(|l| declares_tests_mod(l)) {
        errs.push(format!("{rel}: a <TESTS> zone without a `mod tests`"));
    }
    errs
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// A file from its lines (so no marker ever sits on a line of this file by itself).
    fn file(lines: &[&str]) -> String {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }

    const OK: [&str; 7] = [
        "/* <CODE> */",
        "//! docs",
        "fn f() {}",
        "/* </CODE> */",
        "",
        "/* <TESTS> */",
        "#[cfg(test)]",
    ];

    fn with_tests(extra: &[&str]) -> String {
        let mut v: Vec<&str> = OK.to_vec();
        v.extend_from_slice(extra);
        file(&v)
    }

    #[test]
    fn a_code_only_file_is_fine() {
        let s = file(&["/* <CODE> */", "fn f() {}", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).is_empty());
    }

    #[test]
    fn code_and_tests_are_fine() {
        let s = with_tests(&["mod tests {", "    #[test]", "    fn t() {}", "}", "/* </TESTS> */"]);
        assert_eq!(check("a.rs", &s, Licenses::Forbidden), Vec::<String>::new());
    }

    #[test]
    fn a_ported_file_has_id_lines_licences_and_code() {
        let s = file(&[
            "/*\t$OpenBSD: x.c,v 1.1 2020/01/01 00:00:00 a Exp $\t*/",
            "/* <LICENSES> */",
            "/* Copyright. */",
            "/* </LICENSES> */",
            "",
            "/* <CODE> */",
            "fn f() {}",
            "/* </CODE> */",
        ]);
        assert!(check("a.rs", &s, Licenses::Required).is_empty());
        assert!(check("a.rs", &s, Licenses::Allowed).is_empty());
        let e = check("a.rs", &s, Licenses::Forbidden);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("not the port of a C file"));
    }

    #[test]
    fn a_banner_block_comment_before_the_first_zone_is_fine() {
        let s = file(&[
            "/*",
            " * THIS FILE AUTOMATICALLY GENERATED.",
            " */",
            "/* <CODE> */",
            "/* </CODE> */",
        ]);
        assert!(check("a.rs", &s, Licenses::Forbidden).is_empty());
    }

    #[test]
    fn a_port_without_licences_is_an_error_unless_declared() {
        let s = file(&["/* <CODE> */", "/* </CODE> */"]);
        let e = check("a.rs", &s, Licenses::Required);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("no <LICENSES> zone"));
        assert!(check("a.rs", &s, Licenses::Allowed).is_empty());
    }

    #[test]
    fn code_is_required() {
        let e = check("a.rs", "fn f() {}\n", Licenses::Forbidden);
        assert!(e.iter().any(|m| m.contains("text outside the zones")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("no <CODE> zone")), "{e:?}");
    }

    #[test]
    fn text_between_or_after_zones_is_an_error() {
        let s = file(&["/* <CODE> */", "/* </CODE> */", "fn g() {}"]);
        let e = check("a.rs", &s, Licenses::Forbidden);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].starts_with("a.rs:3:"));
        // a comment after the first zone is text too
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* late */"]);
        assert_eq!(check("a.rs", &s, Licenses::Forbidden).len(), 1);
    }

    #[test]
    fn zones_open_and_close_once_in_order() {
        // CODE twice
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* <CODE> */", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("twice")));
        // TESTS before CODE
        let s = file(&[
            "/* <TESTS> */",
            "mod tests {}",
            "/* </TESTS> */",
            "/* <CODE> */",
            "/* </CODE> */",
        ]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("out of order")));
        // LICENSES after CODE
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* <LICENSES> */", "/* </LICENSES> */"]);
        assert!(check("a.rs", &s, Licenses::Allowed).iter().any(|m| m.contains("out of order")));
    }

    #[test]
    fn unclosed_nested_and_stray_markers() {
        let s = file(&["/* <CODE> */", "fn f() {}"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("never closed")));
        let s = file(&["/* <CODE> */", "/* <TESTS> */", "/* </TESTS> */", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("inside")));
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("not open")));
        let s = file(&["/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().any(|m| m.contains("not open")));
    }

    #[test]
    fn tests_belong_in_the_tests_zone() {
        let s = file(&["/* <CODE> */", "#[cfg(test)]", "mod tests {", "}", "/* </CODE> */"]);
        let e = check("a.rs", &s, Licenses::Forbidden);
        assert!(e.iter().any(|m| m.contains("`mod tests` in the <CODE> zone")), "{e:?}");
        let s = file(&["/* <CODE> */", "#[cfg(test)]", "mod tests;", "/* </CODE> */"]);
        assert!(!check("a.rs", &s, Licenses::Forbidden).is_empty());
        let s = file(&["/* <CODE> */", "#[test]", "fn t() {}", "/* </CODE> */"]);
        let e = check("a.rs", &s, Licenses::Forbidden);
        assert!(e.iter().any(|m| m.contains("#[test]")), "{e:?}");
        // an indented `mod tests` (a nested module) or a helper named like it is not flagged
        let s = file(&["/* <CODE> */", "    mod tests_helper {}", "mod testsuite {}", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).iter().all(|m| !m.contains("mod tests")));
    }

    #[test]
    fn a_tests_zone_needs_a_tests_module() {
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* <TESTS> */", "fn helper() {}", "/* </TESTS> */"]);
        let e = check("a.rs", &s, Licenses::Forbidden);
        assert!(e.iter().any(|m| m.contains("without a `mod tests`")), "{e:?}");
    }

    #[test]
    fn markers_inside_a_line_are_not_markers() {
        let s = file(&["/* <CODE> */", "let a = \"/* <TESTS> */\";", "/* </CODE> */"]);
        assert!(check("a.rs", &s, Licenses::Forbidden).is_empty());
    }

    #[test]
    fn the_tests_mod_forms() {
        assert!(declares_tests_mod("mod tests {"));
        assert!(declares_tests_mod("mod tests;"));
        assert!(declares_tests_mod("pub(crate) mod tests;"));
        assert!(declares_tests_mod("pub mod tests {"));
        assert!(!declares_tests_mod("mod testsuite {"));
        assert!(!declares_tests_mod("// mod tests {"));
    }
}
/* </TESTS> */
