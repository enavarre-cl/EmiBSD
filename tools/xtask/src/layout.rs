/* <CODE> */
//! The zone markers of every `.rs` under `sys/` and `tools/` (milestone M15).
//!
//! A file is split into zones, each opened and closed by a comment line of its own, in this
//! order:
//!
//! ```text
//! /* <LICENSES> */              the author's block, then the original notices, verbatim
//! /* </LICENSES> */
//! /* <CODE> */                  the `//!` docs, inner attributes, `use` lines, code
//! /* </CODE> */
//! /* <TESTS> */                 the tests, inline, at the end (only in files that have tests)
//! /* </TESTS> */
//! ```
//!
//! [`check`] validates one file's zones and [`check_author`] its author block (N0b);
//! `cargo xtask lz check` runs both over the tree with the licence policy `lineage.toml` gives
//! each path. Reading rule: `sed -n '/<CODE>/,/<\/CODE>/p' file.rs`.
//!
//! The author's block (the user's decision of 2026-10-09, `.claude/rules/scope-and-stubs.md`):
//! every LICENSES zone starts with [`AUTHOR_BLOCK`], the latest change first. In the port of a
//! C file with licence text the original notices follow it, untouched, after one blank line;
//! everywhere else it is the zone's only content. [`strip_author_block`] undoes exactly that, so `lz check` can compare a module with
//! its LZ source.
//!
//! Outside the zones there are only blank lines and, before the first zone, block comments (the
//! `$OpenBSD$` id lines, a "generated, do not edit" banner). Anything else is an error.
//! "Has test code" means the file declares a `mod tests` (inline); a `#[test]` or a `mod tests`
//! in the CODE zone is an error, as is a TESTS zone without a `mod tests`.

/// What a path's LICENSES zone holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Licenses {
    /// The port of a C file with licence text: the author's block, then its notices, whole.
    Port,
    /// Everything else (`license = "none"`, an `[[extra]]`, a file outside `lineage.toml`): the
    /// author's block alone.
    AuthorOnly,
}

/// The author's ISC block (N0b), as it sits in a LICENSES zone: the canonical text that
/// `.claude/rules/scope-and-stubs.md` quotes and `gen-syscalls` writes.
pub(crate) const AUTHOR_BLOCK: &str = concat!(
    "/*\n",
    " * Copyright (c) 2026 Emilio Navarrete Lineros <",
    "enavarre@outlook.com>\n",
    " *\n",
    " * Permission to use, copy, modify, and distribute this software for any\n",
    " * purpose with or without fee is hereby granted, provided that the above\n",
    " * copyright notice and this permission notice appear in all copies.\n",
    " *\n",
    " * THE SOFTWARE IS PROVIDED \"AS IS\" AND THE AUTHOR DISCLAIMS ALL WARRANTIES\n",
    " * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF\n",
    " * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR\n",
    " * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES\n",
    " * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN\n",
    " * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF\n",
    " * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.\n",
    " */\n",
);

/// The line that names the author: how a misplaced or duplicated block is found. Both
/// constants are split so that no line of this file is the block's own line.
const AUTHOR_LINE: &str = concat!(
    " * Copyright (c) 2026 Emilio Navarrete Lineros <",
    "enavarre@outlook.com>"
);

const NAMES: [&str; 3] = ["LICENSES", "CODE", "TESTS"];
const LICENSES: usize = 0;
const CODE: usize = 1;
const TESTS: usize = 2;

/// The zone a marker line opens (`Some((zone, true))`) or closes (`Some((zone, false))`).
fn marker(line: &str) -> Option<(usize, bool)> {
    let t = line.trim_end();
    NAMES
        .iter()
        .position(|n| t == format!("/* <{n}> */"))
        .map_or_else(
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
                    errs.push(format!(
                        "{rel}:{n}: </{name}> closes a zone that is not open"
                    ));
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
    if lic == Licenses::Port && seen[LICENSES].0.is_none() {
        errs.push(format!(
            "{rel}: no <LICENSES> zone (a port keeps its notice; `license = \"none\"` in \
             lineage.toml when the C file has none)"
        ));
    }
    for l in &content[CODE] {
        if l.trim() == "#[test]" {
            errs.push(format!(
                "{rel}: a #[test] in the <CODE> zone (tests go in <TESTS>)"
            ));
            break;
        }
    }
    if content[CODE].iter().any(|l| declares_tests_mod(l)) {
        errs.push(format!(
            "{rel}: `mod tests` in the <CODE> zone (it goes in <TESTS>)"
        ));
    }
    if seen[TESTS].0.is_some() && !content[TESTS].iter().any(|l| declares_tests_mod(l)) {
        errs.push(format!("{rel}: a <TESTS> zone without a `mod tests`"));
    }
    errs
}

/// The lines of the LICENSES zone of `lines` as (index of the opening marker, index of the
/// closing marker), or `None` when the file has no complete zone.
fn licenses_zone(lines: &[&str]) -> Option<(usize, usize)> {
    let open = lines
        .iter()
        .position(|l| l.trim_end() == "/* <LICENSES> */")?;
    let close = lines[open..]
        .iter()
        .position(|l| l.trim_end() == "/* </LICENSES> */")?;
    Some((open, open + close))
}

/// Validate the author's block of one file (N0b). With `required`, a file without it is an
/// error; without, only a misplaced or altered block is (the step before the block is applied).
pub(crate) fn check_author(rel: &str, src: &str, lic: Licenses, required: bool) -> Vec<String> {
    let mut errs = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let block: Vec<&str> = AUTHOR_BLOCK.lines().collect();
    let named = lines.iter().filter(|l| **l == AUTHOR_LINE).count();
    let Some((open, close)) = licenses_zone(&lines) else {
        if named > 0 && required {
            errs.push(format!(
                "{rel}: the author's block is outside a <LICENSES> zone"
            ));
        } else if required {
            errs.push(format!(
                "{rel}: no <LICENSES> zone with the author's block (scope-and-stubs.md, N0b)"
            ));
        }
        return errs;
    };
    let zone = &lines[open + 1..close];
    if named > 1 {
        errs.push(format!("{rel}: the author's block appears {named} times"));
    }
    let starts_with_block = zone.starts_with(&block);
    let after = if starts_with_block {
        &zone[block.len()..]
    } else {
        zone
    };
    match lic {
        Licenses::AuthorOnly => {
            if !(starts_with_block && after.is_empty()) {
                errs.push(format!(
                    "{rel}: the <LICENSES> zone must hold the author's block alone (only the \
                     port of a C file with licence text has other notices)"
                ));
            }
        }
        Licenses::Port => {
            if starts_with_block {
                if after.iter().all(|l| l.trim().is_empty()) {
                    errs.push(format!(
                        "{rel}: the author's block is the only one, but the C file's notice must \
                         follow it"
                    ));
                } else if after.first().is_some_and(|l| !l.trim().is_empty()) {
                    errs.push(format!(
                        "{rel}: one blank line goes between the author's block and the \
                         original notices"
                    ));
                }
            } else if named > 0 {
                errs.push(format!(
                    "{rel}: the author's block must be the first block of <LICENSES>, unaltered"
                ));
            } else if required {
                errs.push(format!(
                    "{rel}: no author's block before the original notices (scope-and-stubs.md, \
                     N0b)"
                ));
            }
        }
    }
    errs
}

/// `src` with the blank lines that start and end its LICENSES zone removed. Some LZ zones
/// begin or end with one, and rustfmt keeps at most one blank line in a row, so the blank line
/// under the author's block may be the zone's own. `lz check` compares both sides through it.
pub(crate) fn trim_licenses_edges(src: &str) -> String {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end_matches('\n')).collect();
    let Some((open, close)) = licenses_zone(&trimmed) else {
        return src.to_string();
    };
    let blank = |i: usize| trimmed[i].trim().is_empty();
    let mut out = String::with_capacity(src.len());
    for (i, l) in lines.iter().enumerate() {
        let in_zone = i > open && i < close;
        let leading = in_zone && (open + 1..=i).all(blank);
        let trailing = in_zone && (i..close).all(blank);
        if !(leading || trailing) {
            out.push_str(l);
        }
    }
    out
}

/// `src` without the author's block, as it was before the block was added: from a zone with
/// other notices, the block and the blank line under it; a zone that held only the block, with
/// its markers and the blank line after it. Any other text is returned unchanged.
pub(crate) fn strip_author_block(src: &str) -> String {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end_matches('\n')).collect();
    let block: Vec<&str> = AUTHOR_BLOCK.lines().collect();
    let Some((open, close)) = licenses_zone(&trimmed) else {
        return src.to_string();
    };
    if !trimmed[open + 1..close].starts_with(&block) {
        return src.to_string();
    }
    let end = open + 1 + block.len();
    let (from, to) = if end == close {
        // the zone held only the block: drop the zone and the blank line after it
        let after = close + 1;
        let to = if trimmed.get(after).is_some_and(|l| l.is_empty()) {
            after + 1
        } else {
            after
        };
        (open, to)
    } else if trimmed[end].is_empty() {
        (open + 1, end + 1)
    } else {
        return src.to_string();
    };
    let mut out = String::with_capacity(src.len());
    for (i, l) in lines.iter().enumerate() {
        if i < from || i >= to {
            out.push_str(l);
        }
    }
    out
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
        assert!(check("a.rs", &s, Licenses::AuthorOnly).is_empty());
    }

    #[test]
    fn code_and_tests_are_fine() {
        let s = with_tests(&[
            "mod tests {",
            "    #[test]",
            "    fn t() {}",
            "}",
            "/* </TESTS> */",
        ]);
        assert_eq!(
            check("a.rs", &s, Licenses::AuthorOnly),
            Vec::<String>::new()
        );
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
        assert!(check("a.rs", &s, Licenses::Port).is_empty());
        assert!(check("a.rs", &s, Licenses::AuthorOnly).is_empty());
        // the zone's content is check_author's business
        let e = check_author("a.rs", &s, Licenses::AuthorOnly, false);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("author's block alone"));
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
        assert!(check("a.rs", &s, Licenses::AuthorOnly).is_empty());
    }

    #[test]
    fn a_port_without_licences_is_an_error_unless_declared() {
        let s = file(&["/* <CODE> */", "/* </CODE> */"]);
        let e = check("a.rs", &s, Licenses::Port);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("no <LICENSES> zone"));
        assert!(check("a.rs", &s, Licenses::AuthorOnly).is_empty());
    }

    #[test]
    fn code_is_required() {
        let e = check("a.rs", "fn f() {}\n", Licenses::AuthorOnly);
        assert!(
            e.iter().any(|m| m.contains("text outside the zones")),
            "{e:?}"
        );
        assert!(e.iter().any(|m| m.contains("no <CODE> zone")), "{e:?}");
    }

    #[test]
    fn text_between_or_after_zones_is_an_error() {
        let s = file(&["/* <CODE> */", "/* </CODE> */", "fn g() {}"]);
        let e = check("a.rs", &s, Licenses::AuthorOnly);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].starts_with("a.rs:3:"));
        // a comment after the first zone is text too
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* late */"]);
        assert_eq!(check("a.rs", &s, Licenses::AuthorOnly).len(), 1);
    }

    #[test]
    fn zones_open_and_close_once_in_order() {
        // CODE twice
        let s = file(&[
            "/* <CODE> */",
            "/* </CODE> */",
            "/* <CODE> */",
            "/* </CODE> */",
        ]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("twice"))
        );
        // TESTS before CODE
        let s = file(&[
            "/* <TESTS> */",
            "mod tests {}",
            "/* </TESTS> */",
            "/* <CODE> */",
            "/* </CODE> */",
        ]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("out of order"))
        );
        // LICENSES after CODE
        let s = file(&[
            "/* <CODE> */",
            "/* </CODE> */",
            "/* <LICENSES> */",
            "/* </LICENSES> */",
        ]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("out of order"))
        );
    }

    #[test]
    fn unclosed_nested_and_stray_markers() {
        let s = file(&["/* <CODE> */", "fn f() {}"]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("never closed"))
        );
        let s = file(&[
            "/* <CODE> */",
            "/* <TESTS> */",
            "/* </TESTS> */",
            "/* </CODE> */",
        ]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("inside"))
        );
        let s = file(&["/* <CODE> */", "/* </CODE> */", "/* </CODE> */"]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("not open"))
        );
        let s = file(&["/* </CODE> */"]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .any(|m| m.contains("not open"))
        );
    }

    #[test]
    fn tests_belong_in_the_tests_zone() {
        let s = file(&[
            "/* <CODE> */",
            "#[cfg(test)]",
            "mod tests {",
            "}",
            "/* </CODE> */",
        ]);
        let e = check("a.rs", &s, Licenses::AuthorOnly);
        assert!(
            e.iter()
                .any(|m| m.contains("`mod tests` in the <CODE> zone")),
            "{e:?}"
        );
        let s = file(&[
            "/* <CODE> */",
            "#[cfg(test)]",
            "mod tests;",
            "/* </CODE> */",
        ]);
        assert!(!check("a.rs", &s, Licenses::AuthorOnly).is_empty());
        let s = file(&["/* <CODE> */", "#[test]", "fn t() {}", "/* </CODE> */"]);
        let e = check("a.rs", &s, Licenses::AuthorOnly);
        assert!(e.iter().any(|m| m.contains("#[test]")), "{e:?}");
        // an indented `mod tests` (a nested module) or a helper named like it is not flagged
        let s = file(&[
            "/* <CODE> */",
            "    mod tests_helper {}",
            "mod testsuite {}",
            "/* </CODE> */",
        ]);
        assert!(
            check("a.rs", &s, Licenses::AuthorOnly)
                .iter()
                .all(|m| !m.contains("mod tests"))
        );
    }

    #[test]
    fn a_tests_zone_needs_a_tests_module() {
        let s = file(&[
            "/* <CODE> */",
            "/* </CODE> */",
            "/* <TESTS> */",
            "fn helper() {}",
            "/* </TESTS> */",
        ]);
        let e = check("a.rs", &s, Licenses::AuthorOnly);
        assert!(
            e.iter().any(|m| m.contains("without a `mod tests`")),
            "{e:?}"
        );
    }

    #[test]
    fn markers_inside_a_line_are_not_markers() {
        let s = file(&[
            "/* <CODE> */",
            "let a = \"/* <TESTS> */\";",
            "/* </CODE> */",
        ]);
        assert!(check("a.rs", &s, Licenses::AuthorOnly).is_empty());
    }

    /// A port with an original notice, before and after the author's block is added.
    fn port(with_author: bool) -> String {
        let mut s = String::from("/* <LICENSES> */\n");
        if with_author {
            s.push_str(AUTHOR_BLOCK);
            s.push('\n');
        }
        s.push_str("/*\n * Copyright (c) 1990 Someone\n */\n");
        s.push_str("/* </LICENSES> */\n\n/* <CODE> */\nfn f() {}\n/* </CODE> */\n");
        s
    }

    /// A file with no notice of its own, before and after the author's block is added.
    fn author_only(with_author: bool) -> String {
        let mut s = String::new();
        if with_author {
            s.push_str("/* <LICENSES> */\n");
            s.push_str(AUTHOR_BLOCK);
            s.push_str("/* </LICENSES> */\n\n");
        }
        s.push_str("/* <CODE> */\nfn f() {}\n/* </CODE> */\n");
        s
    }

    #[test]
    fn the_author_block_after_the_notices_or_alone_is_fine() {
        for required in [false, true] {
            let p = port(true);
            assert!(check("a.rs", &p, Licenses::Port).is_empty());
            assert!(check_author("a.rs", &p, Licenses::Port, required).is_empty());
            let a = author_only(true);
            assert!(check("a.rs", &a, Licenses::AuthorOnly).is_empty());
            assert!(check_author("a.rs", &a, Licenses::AuthorOnly, required).is_empty());
        }
    }

    #[test]
    fn a_missing_author_block_is_an_error_only_when_required() {
        assert!(check_author("a.rs", &port(false), Licenses::Port, false).is_empty());
        assert!(check_author("a.rs", &author_only(false), Licenses::AuthorOnly, false).is_empty());
        let e = check_author("a.rs", &port(false), Licenses::Port, true);
        assert!(e[0].contains("no author's block"), "{e:?}");
        let e = check_author("a.rs", &author_only(false), Licenses::AuthorOnly, true);
        assert!(e[0].contains("no <LICENSES> zone"), "{e:?}");
    }

    #[test]
    fn a_misplaced_or_altered_author_block_is_an_error() {
        // after the original notice
        let s = port(false).replacen(
            " */\n/* </LICENSES> */",
            &format!(" */\n\n{AUTHOR_BLOCK}/* </LICENSES> */"),
            1,
        );
        let e = check_author("a.rs", &s, Licenses::Port, false);
        assert!(e.iter().any(|m| m.contains("must be the first")), "{e:?}");
        // alone in a port
        let e = check_author("a.rs", &author_only(true), Licenses::Port, false);
        assert!(e.iter().any(|m| m.contains("must follow it")), "{e:?}");
        // no blank line between
        let s = port(true).replacen(" */\n\n/*", " */\n/*", 1);
        let e = check_author("a.rs", &s, Licenses::Port, false);
        assert!(e.iter().any(|m| m.contains("one blank line")), "{e:?}");
        // altered text
        let s = port(true).replacen("hereby granted", "granted", 1);
        let e = check_author("a.rs", &s, Licenses::Port, true);
        assert!(e.iter().any(|m| m.contains("unaltered")), "{e:?}");
        // twice
        let s = author_only(true).replacen(
            "/* </LICENSES> */",
            &format!("{AUTHOR_BLOCK}/* </LICENSES> */"),
            1,
        );
        let e = check_author("a.rs", &s, Licenses::AuthorOnly, false);
        assert!(e.iter().any(|m| m.contains("2 times")), "{e:?}");
        // outside the zone
        let s = format!("{AUTHOR_BLOCK}{}", author_only(false));
        let e = check_author("a.rs", &s, Licenses::AuthorOnly, true);
        assert!(e.iter().any(|m| m.contains("outside")), "{e:?}");
        // before the block is required, a banner block is left for the script to move
        assert!(check_author("a.rs", &s, Licenses::AuthorOnly, false).is_empty());
    }

    #[test]
    fn strip_author_block_undoes_exactly_the_addition() {
        assert_eq!(strip_author_block(&port(true)), port(false));
        assert_eq!(strip_author_block(&author_only(true)), author_only(false));
        // a banner before the zone stays where it was
        let banner = "/* generated, do not edit */\n";
        assert_eq!(
            strip_author_block(&format!("{banner}{}", author_only(true))),
            format!("{banner}{}", author_only(false))
        );
        // nothing to strip: unchanged
        assert_eq!(strip_author_block(&port(false)), port(false));
        assert_eq!(strip_author_block(&author_only(false)), author_only(false));
        // an altered block is not stripped
        let altered = port(true).replacen("hereby granted", "granted", 1);
        assert_eq!(strip_author_block(&altered), altered);
    }

    #[test]
    fn trim_licenses_edges_drops_only_the_zone_s_leading_and_trailing_blanks() {
        let s = "/* <LICENSES> */\n\n/* a */\n\n/* b */\n\n\n/* </LICENSES> */\n\n/* <CODE> */\n\n/* </CODE> */\n";
        assert_eq!(
            trim_licenses_edges(s),
            "/* <LICENSES> */\n/* a */\n\n/* b */\n/* </LICENSES> */\n\n/* <CODE> */\n\n/* </CODE> */\n"
        );
        assert_eq!(trim_licenses_edges(&port(false)), port(false));
    }

    #[test]
    fn strip_then_trim_matches_a_zone_that_began_with_a_blank_line() {
        // LZ's zone opens with a blank line; the block goes above it without another one
        let lz = "/* <LICENSES> */\n\n/* a */\n/* </LICENSES> */\n\n/* <CODE> */\n/* </CODE> */\n";
        let native = lz.replacen(
            "/* <LICENSES> */\n",
            &format!("/* <LICENSES> */\n{AUTHOR_BLOCK}"),
            1,
        );
        assert!(check_author("a.rs", &native, Licenses::Port, true).is_empty());
        assert_eq!(
            trim_licenses_edges(&strip_author_block(&native)),
            trim_licenses_edges(lz)
        );
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
