//! Host tests for the Rock Ridge walk: the extension check of the root, alternate names,
//! symbolic links, attributes, time stamps, device numbers, relocation entries and the SUSP
//! control entries (`RR`, `CE`, `ST`), over the records of a makefs image and over records
//! built here.

use std::vec::Vec;

use super::*;
use crate::isofs::cd9660::cd9660_extern::ISO_FTYPE_RRIP;
use crate::isofs::cd9660::iso::tests::{image, rec, test_mnt, test_node};
use crate::sys::stat::{S_IFCHR, S_IFDIR, S_IFLNK, S_IFREG};

/// A 733 field: the value little-endian, then big-endian.
fn both32(v: u32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&v.to_le_bytes());
    b[4..].copy_from_slice(&v.to_be_bytes());
    b
}

/// A SUSP entry of version 1.
fn susp(t: &[u8; 2], body: &[u8]) -> Vec<u8> {
    let mut e = std::vec![t[0], t[1], (4 + body.len()) as u8, 1];
    e.extend_from_slice(body);
    e
}

/// A `PX` entry (RRIP 1.10: no file serial number).
fn px(mode: u32, links: u32, uid: u32, gid: u32) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [mode, links, uid, gid] {
        b.extend_from_slice(&both32(v));
    }
    susp(b"PX", &b)
}

/// A directory record at extent 30 named `name`, with the system use entries `entries`.
fn record(name: &[u8], dir: bool, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut r = std::vec![0u8; ISO_DIRECTORY_RECORD_SIZE];
    r[2..10].copy_from_slice(&both32(30));
    r[18..25].copy_from_slice(&[126, 10, 4, 7, 36, 48, (-12i8) as u8]);
    r[25] = if dir { 2 } else { 0 };
    r[28..32].copy_from_slice(&[1, 0, 0, 1]);
    r[32] = name.len() as u8;
    r.extend_from_slice(name);
    if name.len() % 2 == 0 {
        r.push(0);
    }
    for e in entries {
        r.extend_from_slice(e);
    }
    r[0] = r.len() as u8;
    r
}

/// `cd9660_rrip_getname`: the name, its length, the inode number and the result.
fn getname(r: &[u8], imp: &IsoMnt) -> (Vec<u8>, i32, Cdino) {
    let mut out = [0u8; NAME_MAX + 1];
    let mut len = 0u16;
    let mut ino: Cdino = 77;
    let res = cd9660_rrip_getname(&rec(r), &mut out, &mut len, &mut ino, imp);
    (out[..usize::from(len)].to_vec(), res, ino)
}

/// `cd9660_rrip_getsymname`: the link and the result.
fn getsymname(r: &[u8], imp: &IsoMnt) -> (Vec<u8>, i32) {
    let mut out = [0u8; MAXPATHLEN];
    let mut len = 0u16;
    let res = cd9660_rrip_getsymname(&rec(r), &mut out, &mut len, imp);
    (out[..usize::from(len)].to_vec(), res)
}

#[test]
fn the_root_announces_rock_ridge() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    assert_eq!(cd9660_rrip_offset(&rec(&image::ROOT_DOT), imp), 0);
    assert_eq!(imp.rr_skip0, 0);

    // no SP entry: not Rock Ridge (after looking for the CD-ROM XA layout too)
    assert_eq!(cd9660_rrip_offset(&rec(&image::FILE_REC), imp), -1);
    assert_eq!(imp.rr_skip0, 15);

    // CD-ROM XA: 15 bytes of XA data before the SP entry; the ER may say RRIP_1991A
    let mut sp_er = std::vec![0x58u8; 15];
    sp_er.extend_from_slice(b"SP\x07\x01\xbe\xef\x00");
    let mut er = std::vec![10, 0, 0, 1];
    er.extend_from_slice(b"RRIP_1991A");
    sp_er.extend_from_slice(&susp(b"ER", &er));
    let mut r = image::ROOT_DOT[..34].to_vec();
    r.extend_from_slice(&sp_er);
    r[0] = r.len() as u8;
    assert_eq!(cd9660_rrip_offset(&rec(&r), imp), 0);
    assert_eq!(imp.rr_skip0, 15);

    // an extension reference to something else is not Rock Ridge
    let pos = r.len() - 10;
    r[pos..].copy_from_slice(b"SOMETHING!");
    assert_eq!(cd9660_rrip_offset(&rec(&r), imp), -1);
}

#[test]
fn names_come_from_nm_entries() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (name, res, ino) = getname(&image::FILE_REC, imp);
    assert_eq!(name, b"m10c-iso.txt");
    assert_ne!(res & ISO_SUSP_ALTNAME, 0);
    assert_eq!(ino, 77);
    assert_eq!(
        getname(&image::MIXED_REC, imp).0,
        b"Mixed_Case-name.long.txt"
    );
    assert_eq!(getname(&image::SUB_REC, imp).0, b"sub");
    assert_eq!(getname(&image::LINK_REC, imp).0, b"link");
    // '.' and '..' are named by the record, not by NM
    assert_eq!(getname(&image::ROOT_DOTDOT, imp).0, b"..");
    // the root's '.' continues in block 24, past this mount's last block: the walk stops
    imp.volume_space_size = 24;
    assert_eq!(getname(&image::ROOT_DOT, imp).0, b".");
}

#[test]
fn names_without_nm_are_the_iso_names() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let r = record(b"README.TXT;1", false, &[px(0o100444, 1, 0, 0)]);
    let (name, res, _) = getname(&r, imp);
    assert_eq!(name, b"README.TXT;1");
    assert_eq!(res & ISO_SUSP_ALTNAME, 0);
}

#[test]
fn nm_entries_continue_and_name_dots() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let r = record(
        b"A",
        false,
        &[susp(b"NM", b"\x01abc"), susp(b"NM", b"\x00def")],
    );
    assert_eq!(getname(&r, imp).0, b"abcdef");
    let r = record(b"A", false, &[susp(b"NM", &[ISO_SUSP_CFLAG_PARENT])]);
    assert_eq!(getname(&r, imp).0, b"..");
    let r = record(b"A", false, &[susp(b"NM", &[ISO_SUSP_CFLAG_CURRENT])]);
    assert_eq!(getname(&r, imp).0, b".");
    // an NM with bad flags counts as no NM: the ISO name stands
    let r = record(b"A", false, &[susp(b"NM", b"\x40xyz")]);
    assert_eq!(getname(&r, imp).0, b"A");
}

#[test]
fn relocation_entries() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    // CL: a relocated directory's real location
    let r = record(
        b"D",
        true,
        &[susp(b"CL", &both32(40)), susp(b"NM", b"\x00d")],
    );
    let (name, res, ino) = getname(&r, imp);
    assert_eq!((name.as_slice(), ino), (b"d".as_slice(), 40 << 11));
    assert_ne!(res & ISO_SUSP_CLINK, 0);
    // RE: the relocated directory itself, which is not listed
    let r = record(b"D", true, &[susp(b"RE", &[]), susp(b"NM", b"\x00d")]);
    let (name, res, _) = getname(&r, imp);
    assert!(name.is_empty());
    assert_ne!(res & ISO_SUSP_RELDIR, 0);
}

#[test]
fn symbolic_links_gather_their_components() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (link, res) = getsymname(&image::LINK_REC, imp);
    assert_eq!(link, b"../m10c-iso.txt");
    assert_eq!(res, ISO_SUSP_SLINK);

    let sl = |body: &[u8]| record(b"L", false, &[susp(b"SL", body)]);
    let r = sl(b"\x00\x08\x00\x00\x03etc\x00\x06passwd");
    assert_eq!(getsymname(&r, imp).0, b"/etc/passwd");
    let r = sl(b"\x00\x02\x00\x00\x03bin");
    assert_eq!(getsymname(&r, imp).0, b"./bin");
    // a component continued in the next SL entry
    let r = record(
        b"L",
        false,
        &[
            susp(b"SL", b"\x01\x01\x03abc"),
            susp(b"SL", b"\x00\x00\x03def"),
        ],
    );
    assert_eq!(getsymname(&r, imp), (b"abcdef".to_vec(), ISO_SUSP_SLINK));
    // the mount point
    imp.im_mountp
        .update_stat(|sp| sp.f_mntonname[..6].copy_from_slice(b"/cdrom"));
    let r = sl(b"\x00\x10\x00\x00\x03etc");
    assert_eq!(getsymname(&r, imp).0, b"/cdrom/etc");
    // a component running past the entry is invalid
    let r = sl(b"\x00\x00\x09abc");
    assert_eq!(getsymname(&r, imp), (b"".to_vec(), 0));
}

#[test]
fn symbolic_links_longer_than_maxpathlen_are_refused() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let mut body = std::vec![0u8];
    for _ in 0..40 {
        body.extend_from_slice(&[0, 30]);
        body.extend_from_slice(&[b'x'; 30]);
    }
    // more than one entry's worth: split over SL entries of at most 250 bytes
    let entries: Vec<Vec<u8>> = body[1..]
        .chunks(32 * 7)
        .map(|c| {
            let mut e = std::vec![1u8];
            e.extend_from_slice(c);
            susp(b"SL", &e)
        })
        .collect();
    // a record would be too long for its length byte: walk the entries as the loop does
    let mut out = [0u8; MAXPATHLEN];
    let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_SLINK);
    ana.outbuf = &mut out;
    ana.maxlen = MAXPATHLEN as u16;
    ana.cont = 1;
    let mut res = 0;
    for e in &entries {
        res |= cd9660_rrip_slink(e, &mut ana);
        if ana.fields == 0 {
            break;
        }
    }
    assert_eq!(res, 0);
    assert_eq!(ana.outlen, 0);
    assert_eq!(ana.fields, 0);
}

#[test]
fn attributes_come_from_px_and_pn() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (ip, _vp) = test_node(imp);
    let res = cd9660_rrip_analyze(&rec(&image::FILE_REC), ip, imp);
    let ino = ip.inode.get();
    assert_eq!(u32::from(ino.iso_mode), S_IFREG | 0o644);
    assert_eq!((ino.iso_links, ino.iso_uid, ino.iso_gid), (1, 0, 0));
    assert_ne!(res & ISO_SUSP_ATTR, 0);
    // makefs writes TF entries without the byte that ISO_RRIP_TSTAMP's size counts, so
    // they are not read: the times are the record's
    assert_eq!(res & ISO_SUSP_TSTAMP, 0);
    assert_eq!(ino.iso_mtime, Timespec::new(1_791_110_208, 0));

    let (ip, _vp) = test_node(imp);
    cd9660_rrip_analyze(&rec(&image::LINK_REC), ip, imp);
    assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFLNK | 0o755);

    let (ip, _vp) = test_node(imp);
    let r = record(
        b"TTY",
        false,
        &[
            px(S_IFCHR | 0o620, 1, 0, 4),
            susp(b"PN", &[both32(0), both32(makedev(5, 3) as u32)].concat()),
        ],
    );
    let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
    assert_ne!(res & ISO_SUSP_DEVICE, 0);
    assert_eq!(ip.inode.get().iso_rdev, makedev(5, 3));
    assert_eq!(ip.inode.get().iso_gid, 4);
    let r = record(
        b"TTY",
        false,
        &[susp(
            b"PN",
            &[both32(7), both32(makedev(5, 3) as u32)].concat(),
        )],
    );
    cd9660_rrip_analyze(&rec(&r), ip, imp);
    assert_eq!(ip.inode.get().iso_rdev, makedev(7, 3));
}

#[test]
fn time_stamps_come_from_tf() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (ip, _vp) = test_node(imp);
    // modify and access, 7-byte form, plus the byte the C's size check counts
    let mut tf = std::vec![ISO_SUSP_TSTAMP_MODIFY | ISO_SUSP_TSTAMP_ACCESS];
    tf.extend_from_slice(&[99, 12, 31, 23, 59, 59, 0]);
    tf.extend_from_slice(&[70, 1, 1, 0, 0, 1, 0]);
    tf.push(0);
    let r = record(
        b"F",
        false,
        &[px(S_IFREG | 0o444, 1, 0, 0), susp(b"TF", &tf)],
    );
    let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
    assert_ne!(res & ISO_SUSP_TSTAMP, 0);
    let ino = ip.inode.get();
    assert_eq!(ino.iso_mtime, Timespec::new(946_684_799, 0));
    assert_eq!(ino.iso_atime, Timespec::new(1, 0));
    assert_eq!(ino.iso_ctime, ino.iso_mtime);

    // 17-byte form with the creation time first
    let mut tf = std::vec![ISO_SUSP_TSTAMP_FORM17 | ISO_SUSP_TSTAMP_CREAT | ISO_SUSP_TSTAMP_ATTR];
    tf.extend_from_slice(b"2000010100000000\x00");
    tf.extend_from_slice(b"2026100407364800\xf4");
    tf.push(0);
    let r = record(
        b"F",
        false,
        &[px(S_IFREG | 0o444, 1, 0, 0), susp(b"TF", &tf)],
    );
    cd9660_rrip_analyze(&rec(&r), ip, imp);
    let ino = ip.inode.get();
    assert_eq!(ino.iso_ctime, Timespec::new(1_791_110_208, 0));
    assert_eq!(ino.iso_mtime, Timespec::new(0, 0));
    assert_eq!(ino.iso_atime, ino.iso_mtime);
}

#[test]
fn rr_st_and_missing_px() {
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (ip, _vp) = test_node(imp);
    // RR says only PX is recorded: the walk ends after it, the times are the record's
    let mut tf = std::vec![ISO_SUSP_TSTAMP_MODIFY];
    tf.extend_from_slice(&[99, 12, 31, 23, 59, 59, 0, 0]);
    let r = record(
        b"F",
        false,
        &[
            susp(b"RR", &[0x01]),
            px(S_IFREG | 0o600, 1, 1000, 10),
            susp(b"TF", &tf),
        ],
    );
    let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
    assert_eq!(res & ISO_SUSP_TSTAMP, 0);
    assert_eq!(ip.inode.get().iso_uid, 1000);
    assert_eq!(ip.inode.get().iso_mtime, Timespec::new(1_791_110_208, 0));

    // ST ends the system use area: the PX after it is not read, the defaults apply
    let (ip, _vp) = test_node(imp);
    let r = record(
        b"D",
        true,
        &[susp(b"ST", &[]), px(S_IFREG | 0o600, 1, 1000, 10)],
    );
    let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
    assert_eq!(res & ISO_SUSP_ATTR, 0);
    assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o555);

    // the root's '.' keeps its PX in a continuation area this mount cannot reach
    let imp = test_mnt(ISO_FTYPE_RRIP);
    imp.volume_space_size = 24;
    let (ip, _vp) = test_node(imp);
    let res = cd9660_rrip_analyze(&rec(&image::ROOT_DOT), ip, imp);
    assert_eq!(res & ISO_SUSP_ATTR, 0);
    assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o555);
}

#[test]
fn the_continuation_area_holds_the_root_attributes() {
    // the CE target of the root's '.' entry, walked as the loop walks it after the read
    let imp = test_mnt(ISO_FTYPE_RRIP);
    let (ip, _vp) = test_node(imp);
    let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_ATTR | ISO_SUSP_TSTAMP);
    ana.inop = Some(ip);
    assert_eq!(cd9660_rrip_attr(&image::ROOT_CE, &mut ana), ISO_SUSP_ATTR);
    assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o755);
    assert_eq!(ip.inode.get().iso_links, 4);
    let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_ATTR);
    let ce = &image::ROOT_DOT[image::ROOT_DOT.len() - 28..];
    assert_eq!(cd9660_rrip_cont(ce, &mut ana), ISO_SUSP_CONT);
    assert_eq!(
        (ana.iso_ce_blk, ana.iso_ce_off, ana.iso_ce_len),
        (24, 0, 36)
    );
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/isofs/cd9660/cd9660_rrip.h");
    for (name, value) in [
        ("ISO_SUSP_CFLAG_CONTINUE", ISO_SUSP_CFLAG_CONTINUE),
        ("ISO_SUSP_CFLAG_CURRENT", ISO_SUSP_CFLAG_CURRENT),
        ("ISO_SUSP_CFLAG_PARENT", ISO_SUSP_CFLAG_PARENT),
        ("ISO_SUSP_CFLAG_ROOT", ISO_SUSP_CFLAG_ROOT),
        ("ISO_SUSP_CFLAG_VOLROOT", ISO_SUSP_CFLAG_VOLROOT),
        ("ISO_SUSP_CFLAG_HOST", ISO_SUSP_CFLAG_HOST),
        ("ISO_RRIP_SLSIZ", ISO_RRIP_SLSIZ as u8),
        ("ISO_SUSP_TSTAMP_FORM17", ISO_SUSP_TSTAMP_FORM17),
        ("ISO_SUSP_TSTAMP_FORM7", ISO_SUSP_TSTAMP_FORM7),
        ("ISO_SUSP_TSTAMP_CREAT", ISO_SUSP_TSTAMP_CREAT),
        ("ISO_SUSP_TSTAMP_MODIFY", ISO_SUSP_TSTAMP_MODIFY),
        ("ISO_SUSP_TSTAMP_ACCESS", ISO_SUSP_TSTAMP_ACCESS),
        ("ISO_SUSP_TSTAMP_ATTR", ISO_SUSP_TSTAMP_ATTR),
        ("ISO_SUSP_TSTAMP_BACKUP", ISO_SUSP_TSTAMP_BACKUP),
        ("ISO_SUSP_TSTAMP_EXPIRE", ISO_SUSP_TSTAMP_EXPIRE),
        ("ISO_SUSP_TSTAMP_EFFECT", ISO_SUSP_TSTAMP_EFFECT),
    ] {
        assert_eq!(
            crate::reftest::int(&defs, name),
            Some(i64::from(value)),
            "{name}"
        );
    }
}
