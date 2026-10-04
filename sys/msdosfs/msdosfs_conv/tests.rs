//! Host tests of `msdosfs_conv.rs`: the time conversions both ways, the short name conversions
//! with their quirks, the long name entries, and the tables against the C file.

use super::*;
use crate::sys::dirent::MAXNAMLEN;

fn ts(tv_sec: i64, tv_nsec: i64) -> Timespec {
    Timespec { tv_sec, tv_nsec }
}

fn dirent() -> Dirent {
    Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: 0,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    }
}

fn dos(name: &[u8]) -> [u8; 11] {
    let mut dn = [0u8; 11];
    dn.copy_from_slice(name);
    dn
}

#[test]
fn unix2dostime_vectors() {
    // 1980-01-01 00:00:00, the DOS epoch.
    assert_eq!(unix2dostime(&ts(315_532_800, 0)), (0x21, 0, 0));
    // Before the epoch and after 2107 there is only the epoch.
    assert_eq!(unix2dostime(&ts(0, 0)), (0x21, 0, 0));
    assert_eq!(unix2dostime(&ts(4_354_819_198, 0)), (0x21, 0, 0));
    // 2026-10-04 12:34:57.25: the odd second goes to the hundredths.
    assert_eq!(
        unix2dostime(&ts(1_791_117_297, 250_000_000)),
        (4 | 10 << 5 | 46 << 9, 28 | 34 << 5 | 12 << 11, 125)
    );
    // 2000-02-29 23:59:58: a leap day.
    assert_eq!(
        unix2dostime(&ts(951_868_798, 0)),
        (29 | 2 << 5 | 20 << 9, 29 | 59 << 5 | 23 << 11, 0)
    );
}

#[test]
fn unix2dostime_cache_keeps_conversions_apart() {
    // Alternate times on the same day and on different days: the cache must give each its own
    // result.
    for _ in 0..3 {
        assert_eq!(unix2dostime(&ts(951_868_798, 0)).1, 29 | 59 << 5 | 23 << 11);
        assert_eq!(unix2dostime(&ts(951_868_700, 0)).1, 10 | 58 << 5 | 23 << 11);
        assert_eq!(unix2dostime(&ts(951_868_700, 0)).0, 29 | 2 << 5 | 20 << 9);
        assert_eq!(unix2dostime(&ts(1_791_117_297, 0)).0, 4 | 10 << 5 | 46 << 9);
    }
}

#[test]
fn dos2unixtime_vectors() {
    let t = dos2unixtime(0, 0x1234, 0);
    assert_eq!((t.tv_sec, t.tv_nsec), (0, 0));
    let t = dos2unixtime(0x21, 0, 0);
    assert_eq!((t.tv_sec, t.tv_nsec), (315_532_800, 0));
    let t = dos2unixtime(4 | 10 << 5 | 46 << 9, 28 | 34 << 5 | 12 << 11, 125);
    assert_eq!((t.tv_sec, t.tv_nsec), (1_791_117_297, 250_000_000));
    let t = dos2unixtime(29 | 2 << 5 | 20 << 9, 29 | 59 << 5 | 23 << 11, 0);
    assert_eq!(t.tv_sec, 951_868_798);
    // Month 0 is reported and read as January.
    assert_eq!(dos2unixtime(1, 0, 0).tv_sec, 315_532_800);
    // A corrupt month 15 does not fault.
    let _ = dos2unixtime(1 | 15 << 5, 0, 0);
}

#[test]
fn time_round_trip() {
    for &(sec, nsec) in &[
        (315_532_800i64, 0i64),
        (315_532_801, 990_000_000),
        (951_868_798, 0),
        (1_000_000_000, 120_000_000),
        (1_791_117_297, 250_000_000),
        (4_294_967_294, 0),
    ] {
        let (dd, dt, dh) = unix2dostime(&ts(sec, nsec));
        let back = dos2unixtime(u32::from(dd), u32::from(dt), u32::from(dh));
        assert_eq!((back.tv_sec, back.tv_nsec), (sec, nsec), "{sec}");
    }
    // dos2unixtime adds in 32 bits, as the C does: from 2106 on, the time wraps.
    let (dd, dt, dh) = unix2dostime(&ts(4_354_775_998, 0));
    let back = dos2unixtime(u32::from(dd), u32::from(dt), u32::from(dh));
    assert_eq!(back.tv_sec, 4_354_775_998 - (1 << 32));
}

#[test]
fn dos2unixfn_vectors() {
    let mut un = [0xaau8; 13];
    assert_eq!(dos2unixfn(b"README  TXT", &mut un, false), 10);
    assert_eq!(&un[..11], b"README.TXT\0");
    assert_eq!(dos2unixfn(b"README  TXT", &mut un, true), 10);
    assert_eq!(&un[..11], b"readme.txt\0");
    assert_eq!(dos2unixfn(b"FOO        ", &mut un, false), 3);
    assert_eq!(&un[..4], b"FOO\0");
    assert_eq!(dos2unixfn(b"ABCDEFGHIJK", &mut un, false), 12);
    assert_eq!(&un, b"ABCDEFGH.IJK\0");
    // SLOT_E5 stands for 0xe5 (code page 850), which is 0xd5 in ISO 8859-1.
    assert_eq!(dos2unixfn(&dos(b"\x05A         "), &mut un, false), 2);
    assert_eq!(&un[..3], b"\xd5A\0");
    assert_eq!(dos2unixfn(&dos(b"\x05A         "), &mut un, true), 2);
    assert_eq!(&un[..3], b"\xf5a\0");
    // A character code page 850 has and ISO 8859-1 lacks becomes '?'.
    assert_eq!(dos2unixfn(&dos(b"\xb0          "), &mut un, false), 1);
    assert_eq!(&un[..2], b"?\0");
}

#[test]
fn unix2dosfn_vectors() {
    let mut dn = [0u8; 11];
    assert_eq!(unix2dosfn(b".", &mut dn, 0), 1);
    assert_eq!(&dn, b".          ");
    assert_eq!(unix2dosfn(b".", &mut dn, 2), 0);
    assert_eq!(unix2dosfn(b"..", &mut dn, 1), 1);
    assert_eq!(&dn, b"..         ");
    assert_eq!(unix2dosfn(b"README.TXT", &mut dn, 0), 1);
    assert_eq!(&dn, b"README  TXT");
    assert_eq!(unix2dosfn(b"README.TXT", &mut dn, 2), 0);
    assert_eq!(unix2dosfn(b"readme.txt", &mut dn, 0), 2);
    assert_eq!(&dn, b"README  TXT");
    assert_eq!(unix2dosfn(b"  . .", &mut dn, 0), 0);
    assert_eq!(unix2dosfn(b"", &mut dn, 0), 0);
    // Too long: a generation number replaces the tail.
    assert_eq!(unix2dosfn(b"longfilename.txt", &mut dn, 1), 3);
    assert_eq!(&dn, b"LONGFI~1TXT");
    assert_eq!(unix2dosfn(b"longfilename.txt", &mut dn, 123), 3);
    assert_eq!(&dn, b"LONG~123TXT");
    // OpenBSD writes the '~' over the last character of a short name.
    assert_eq!(unix2dosfn(b"abc.html", &mut dn, 1), 3);
    assert_eq!(&dn, b"AB~1    HTM");
    // A character a short name cannot hold is dropped.
    assert_eq!(unix2dosfn(b"a+b", &mut dn, 2), 3);
    assert_eq!(&dn, b"A~2        ");
    // A leading dot does not start an extension.
    assert_eq!(unix2dosfn(b".profile", &mut dn, 1), 3);
    assert_eq!(&dn, b"PROFIL~1   ");
    // The first trailing dot is kept in the extension, where it cannot be held: a generation
    // number follows.
    assert_eq!(unix2dosfn(b"name.c..", &mut dn, 1), 3);
    assert_eq!(&dn, b"NAM~1   C  ");
    assert_eq!(unix2dosfn(b"name.c", &mut dn, 0), 2);
    assert_eq!(&dn, b"NAME    C  ");
    // Seven digits do not fit.
    assert_eq!(unix2dosfn(b"longfilename.txt", &mut dn, 1_000_000), 0);
    // 0xe5 in the first slot would mean a deleted entry.
    assert_eq!(unix2dosfn(b"\xd5", &mut dn, 0), 2);
    assert_eq!(dn[0], SLOT_E5);
    // Nothing left of the name: '_', which the generation number then overwrites.
    assert_eq!(unix2dosfn(b"++.txt", &mut dn, 0), 3);
    assert_eq!(&dn, b"~       TXT");
    assert_eq!(unix2dosfn(b"++.txt", &mut dn, 1), 3);
    assert_eq!(&dn, b"~1      TXT");
}

#[test]
fn win_checksum_and_slots() {
    let reference = |n: &[u8; 11]| {
        n.iter()
            .fold(0u8, |s, &c| s.rotate_right(1).wrapping_add(c))
    };
    assert_eq!(winChksum(b"README  TXT"), 115);
    assert_eq!(winChksum(b"LONGFI~1TXT"), reference(b"LONGFI~1TXT"));
    assert_eq!(winSlotCnt(b"a"), 1);
    assert_eq!(winSlotCnt(b"abcdefghijklm"), 1);
    assert_eq!(winSlotCnt(b"abcdefghijklmn"), 2);
    assert_eq!(winSlotCnt(b"abcdefghijklm. . "), 1);
    assert_eq!(winSlotCnt(&[b'x'; 255]), 20);
    assert_eq!(winSlotCnt(&[b'x'; 256]), 0);
}

/// The long name entries of `name`, in the order they lie on the disk (last slot first).
fn entries(name: &[u8], chksum: u8) -> std::vec::Vec<Winentry> {
    let cnt = winSlotCnt(name);
    let mut v = std::vec::Vec::new();
    for c in (1..=cnt).rev() {
        let mut we = Winentry::default();
        let left = unix2winfn(name, &mut we, c, chksum);
        assert_eq!(left == 0, c == cnt, "slot {c}");
        v.push(we);
    }
    v
}

#[test]
fn long_names_round_trip() {
    let name = b"A long file name.txt";
    let ck = winChksum(b"ALONGF~1TXT");
    let ents = entries(name, ck);
    assert_eq!(ents.len(), 2);
    assert_eq!(ents[0].weCnt, 2 | WIN_LAST);
    assert_eq!(ents[1].weCnt, 1);
    assert_eq!(ents[0].weAttributes, ATTR_WIN95);
    // Characters 14 to 20 and a terminator; the rest stays 0xff.
    assert_eq!(&ents[0].wePart1, b"a\0m\0e\0.\0t\0");
    assert_eq!(&ents[0].wePart2[..6], b"x\0t\0\0\0");
    assert_eq!(&ents[0].wePart2[6..], &[0xff; 6]);
    assert_eq!(&ents[0].wePart3, &[0xff; 4]);

    // winChkName walks the slots as lookup does, in disk order.
    let mut chksum = -1;
    for we in &ents {
        chksum = winChkName(name, we, chksum);
    }
    assert_eq!(chksum, i32::from(ck));
    let mut chksum = -1;
    for we in &ents {
        chksum = winChkName(b"A LONG FILE NAME.TXT", we, chksum);
    }
    assert_eq!(chksum, i32::from(ck), "case does not matter");
    let mut chksum = -1;
    for we in &ents {
        chksum = winChkName(b"A long file name.txx", we, chksum);
    }
    assert_eq!(chksum, -1);
    assert_eq!(
        winChkName(b"A long file name.txt", &ents[1], i32::from(ck) + 1),
        -1
    );

    // win2unixfn rebuilds the name for readdir.
    let mut d = dirent();
    let mut chksum = -1;
    for we in &ents {
        chksum = win2unixfn(we, &mut d, chksum);
    }
    assert_eq!(chksum, i32::from(ck));
    assert_eq!(usize::from(d.d_namlen), name.len());
    assert_eq!(&d.d_name[..name.len()], name);
}

#[test]
fn long_name_of_exactly_one_slot() {
    let name = b"abcdefghijklm";
    let mut we = Winentry::default();
    assert_eq!(unix2winfn(name, &mut we, 1, 7), 0);
    assert_eq!(we.weCnt, 1 | WIN_LAST);
    assert_eq!(&we.wePart3, b"l\0m\0");
    assert_eq!(winChkName(name, &we, -1), 7);
    assert_eq!(winChkName(b"abcdefghijkl", &we, -1), -1);
    let mut d = dirent();
    assert_eq!(win2unixfn(&we, &mut d, -1), 7);
    assert_eq!(d.d_namlen, 13);
    assert_eq!(&d.d_name[..13], name);
}

#[test]
fn win2unixfn_rejects_bad_entries() {
    let mut d = dirent();
    let mut we = Winentry::default();
    unix2winfn(b"a/b", &mut we, 1, 1);
    assert_eq!(win2unixfn(&we, &mut d, -1), -1);
    assert_eq!(d.d_name[1], 0);
    // Slot 0 and slots past 20 are impossible.
    we.weCnt = WIN_LAST;
    assert_eq!(win2unixfn(&we, &mut d, -1), -1);
    we.weCnt = WIN_LAST | 21;
    assert_eq!(win2unixfn(&we, &mut d, -1), -1);
    assert_eq!(winChkName(b"abc", &we, -1), -1);
    // A non-Latin-1 character (high byte set) is not representable.
    let mut we = Winentry::default();
    unix2winfn(b"abc", &mut we, 1, 1);
    we.wePart1[3] = 0x04;
    assert_eq!(win2unixfn(&we, &mut d, -1), -1);
    // Slot 20 of a 260-character name runs past d_name.
    let mut we = Winentry::default();
    unix2winfn(&[b'x'; 255], &mut we, 20, 1);
    assert_eq!(we.weCnt, 20 | WIN_LAST);
    assert_eq!(win2unixfn(&we, &mut d, -1), 1);
    assert_eq!(d.d_namlen, 255);
    we.wePart2[6] = b'y';
    we.wePart2[8] = b'z';
    assert_eq!(win2unixfn(&we, &mut d, -1), -1);
    assert_eq!(d.d_name[255], 0);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn tables_match_the_c_file() {
    use crate::crypto::testutil::c_table;
    let rel = "sys/msdosfs/msdosfs_conv.c";
    for (name, ours) in [
        ("unix2dos", &UNIX2DOS),
        ("dos2unix", &DOS2UNIX),
        ("u2l", &U2L),
    ] {
        let theirs = c_table(rel, name);
        let ours: std::vec::Vec<u64> = ours.iter().map(|&b| u64::from(b)).collect();
        assert_eq!(ours, theirs, "{name}");
    }
    for (name, ours) in [("regyear", &REGYEAR), ("leapyear", &LEAPYEAR)] {
        let ours: std::vec::Vec<u64> = ours.iter().map(|&d| u64::from(d)).collect();
        assert_eq!(ours, c_table(rel, name), "{name}");
    }
}
