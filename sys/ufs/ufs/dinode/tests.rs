//! Tests for `<ufs/ufs/dinode.h>`: the constants and the dinode layouts against the C header.

use core::mem::offset_of;
use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::{assert_eq, format};

use super::*;

/// The `NAME: offset` pairs the header writes in the comments of `struct name { ... };`
/// (`int32_t di_db[NDADDR]; /* 40: Direct disk blocks. */`).
fn commented_offsets(text: &str, name: &str) -> BTreeMap<String, usize> {
    let start = format!("struct\t{name} {{");
    let mut out = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with(&start) || line.starts_with(&format!("struct {name} {{")) {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.starts_with("};") {
            break;
        }
        let Some((decl, comment)) = line.split_once("/*") else {
            continue;
        };
        let Some((off, _)) = comment.trim().split_once(':') else {
            continue;
        };
        let Ok(off) = off.trim().parse::<usize>() else {
            continue;
        };
        // `u_int16_t di_mode;` -> the identifier is the last word before any `[`.
        let ident = decl.trim().trim_end_matches(';');
        let ident = ident.split('[').next().unwrap_or("");
        let ident = ident.split_whitespace().last().unwrap_or("");
        if !ident.is_empty() {
            out.insert(ident.to_string(), off);
        }
    }
    out
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ufs/ufs/dinode.h");
    for (name, value) in [
        ("NXADDR", NXADDR as i64),
        ("NDADDR", NDADDR as i64),
        ("NIADDR", NIADDR as i64),
        ("IEXEC", i64::from(IEXEC)),
        ("ISVTX", i64::from(ISVTX)),
        ("ISUID", i64::from(ISUID)),
        ("IFMT", i64::from(IFMT)),
        ("IFIFO", i64::from(IFIFO)),
        ("IFDIR", i64::from(IFDIR)),
        ("IFREG", i64::from(IFREG)),
        ("IFLNK", i64::from(IFLNK)),
        ("IFWHT", i64::from(IFWHT)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
    // ROOTINO is `((ufsino_t)2)`.
    assert_eq!(
        defs.get("ROOTINO").map(String::as_str),
        Some("((ufsino_t)2)")
    );
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn dinode_layouts_match_the_offsets_in_the_c_header() {
    let path = crate::reftest::openbsd_src().join("sys/ufs/ufs/dinode.h");
    let text = std::fs::read_to_string(path).expect("dinode.h");

    let ufs1 = commented_offsets(&text, "ufs1_dinode");
    let want1 = [
        ("di_mode", offset_of!(Ufs1Dinode, di_mode)),
        ("di_nlink", offset_of!(Ufs1Dinode, di_nlink)),
        ("di_size", offset_of!(Ufs1Dinode, di_size)),
        ("di_atime", offset_of!(Ufs1Dinode, di_atime)),
        ("di_atimensec", offset_of!(Ufs1Dinode, di_atimensec)),
        ("di_mtime", offset_of!(Ufs1Dinode, di_mtime)),
        ("di_mtimensec", offset_of!(Ufs1Dinode, di_mtimensec)),
        ("di_ctime", offset_of!(Ufs1Dinode, di_ctime)),
        ("di_ctimensec", offset_of!(Ufs1Dinode, di_ctimensec)),
        ("di_db", offset_of!(Ufs1Dinode, di_db)),
        ("di_ib", offset_of!(Ufs1Dinode, di_ib)),
        ("di_flags", offset_of!(Ufs1Dinode, di_flags)),
        ("di_blocks", offset_of!(Ufs1Dinode, di_blocks)),
        ("di_gen", offset_of!(Ufs1Dinode, di_gen)),
        ("di_uid", offset_of!(Ufs1Dinode, di_uid)),
        ("di_gid", offset_of!(Ufs1Dinode, di_gid)),
        ("di_spare", offset_of!(Ufs1Dinode, di_spare)),
    ];
    for (name, off) in want1 {
        assert_eq!(ufs1.get(name), Some(&off), "ufs1_dinode.{name}");
    }

    let ufs2 = commented_offsets(&text, "ufs2_dinode");
    let want2 = [
        ("di_mode", offset_of!(Ufs2Dinode, di_mode)),
        ("di_nlink", offset_of!(Ufs2Dinode, di_nlink)),
        ("di_uid", offset_of!(Ufs2Dinode, di_uid)),
        ("di_gid", offset_of!(Ufs2Dinode, di_gid)),
        ("di_blksize", offset_of!(Ufs2Dinode, di_blksize)),
        ("di_size", offset_of!(Ufs2Dinode, di_size)),
        ("di_blocks", offset_of!(Ufs2Dinode, di_blocks)),
        ("di_atime", offset_of!(Ufs2Dinode, di_atime)),
        ("di_mtime", offset_of!(Ufs2Dinode, di_mtime)),
        ("di_ctime", offset_of!(Ufs2Dinode, di_ctime)),
        ("di_birthtime", offset_of!(Ufs2Dinode, di_birthtime)),
        ("di_mtimensec", offset_of!(Ufs2Dinode, di_mtimensec)),
        ("di_atimensec", offset_of!(Ufs2Dinode, di_atimensec)),
        ("di_ctimensec", offset_of!(Ufs2Dinode, di_ctimensec)),
        ("di_birthnsec", offset_of!(Ufs2Dinode, di_birthnsec)),
        ("di_gen", offset_of!(Ufs2Dinode, di_gen)),
        ("di_kernflags", offset_of!(Ufs2Dinode, di_kernflags)),
        ("di_flags", offset_of!(Ufs2Dinode, di_flags)),
        ("di_extsize", offset_of!(Ufs2Dinode, di_extsize)),
        ("di_extb", offset_of!(Ufs2Dinode, di_extb)),
        ("di_db", offset_of!(Ufs2Dinode, di_db)),
        ("di_ib", offset_of!(Ufs2Dinode, di_ib)),
        ("di_spare", offset_of!(Ufs2Dinode, di_spare)),
    ];
    for (name, off) in want2 {
        assert_eq!(ufs2.get(name), Some(&off), "ufs2_dinode.{name}");
    }
    assert_eq!(ufs2.len(), want2.len());
}

#[test]
fn the_ufs1_union_views_share_the_bytes() {
    let mut d = Ufs1Dinode::default();
    d.di_u = [7, 9];
    assert_eq!((d.di_ouid(), d.di_ogid()), (7, 9));
    let mut b = [0u8; 4];
    b[..2].copy_from_slice(&7u16.to_ne_bytes());
    b[2..].copy_from_slice(&9u16.to_ne_bytes());
    assert_eq!(d.di_inumber(), u32::from_ne_bytes(b));
    assert_eq!(MAXSYMLINKLEN_UFS1, 60);
    assert_eq!(MAXSYMLINKLEN_UFS2, 120);
}
