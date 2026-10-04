//! Host tests for the NTFS subroutines that need no volume: the run list decoder, the update
//! sequence fixups, the name comparisons over the upper-case table, attribute name parsing,
//! name types and times. The mount, lookup and read paths are tested over an in-memory
//! volume in `ntfs_vnops/tests.rs`.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::ntfs::ntfs::{Bootfile, Ntvattrdef};
use crate::ntfs::ntfs_conv::{ntfs_utf8_wcmp, ntfs_utf8_wget, ntfs_utf8_wput};

/// A mount with 512-byte sectors, 2 sectors per cluster, 1 KB MFT records and the UTF-8
/// hooks (not linked to anything).
fn ntmp() -> Ntfsmount {
    let ntmp = Ntfsmount::new();
    ntmp.ntm_bootfile.set(Bootfile {
        bf_bps: 512,
        bf_spc: 2,
        bf_mftrecsz: 0xF6,
        ..Bootfile::default()
    });
    ntmp.ntm_bpmftrec.set(2);
    ntmp.ntm_wget.set(Some(ntfs_utf8_wget));
    ntmp.ntm_wput.set(Some(ntfs_utf8_wput));
    ntmp.ntm_wcmp.set(Some(ntfs_utf8_wcmp));
    ntmp
}

/// Memory (for malloc) and the serialisation of the tests that share globals.
fn setup() -> MutexGuard<'static, ()> {
    let (g, _p) = crate::kern::vfs_subr::tests::setup();
    ntfs_toupper_reset();
    g
}

/// The decoded runs: (cluster number, length) pairs; frees the arrays.
fn runs(run: &[u8]) -> Vec<(Cn, Cn)> {
    let (cn, cl, cnt) = ntfs_runtovrun(run).unwrap();
    let Some((cn, cl)) = cn.zip(cl) else {
        assert_eq!(cnt, 0);
        return Vec::new();
    };
    let n = cnt as usize;
    // SAFETY: the two arrays of `cnt` entries ntfs_runtovrun just made.
    let out = unsafe {
        let a = core::slice::from_raw_parts(cn.as_ptr(), n);
        let b = core::slice::from_raw_parts(cl.as_ptr(), n);
        a.iter().copied().zip(b.iter().copied()).collect()
    };
    free(cn.cast(), M_NTFSRUN, n * 8);
    free(cl.cast(), M_NTFSRUN, n * 8);
    out
}

#[test]
fn run_lists_decode_to_absolute_clusters() {
    let _g = setup();
    let run = [
        0x21, 0x02, 0xA0,
        0x00, // 2 clusters at 160 (two offset bytes: 0xA0 is negative alone)
        0x01, 0x01, // 1 sparse cluster
        0x11, 0x01, 0x0A, // 1 cluster at 160 + 10
        0x11, 0x03, 0xFB, // 3 clusters at 170 - 5
        0x00,
    ];
    assert_eq!(runs(&run), [(160, 2), (0, 1), (170, 1), (165, 3)]);
    assert!(runs(&[0]).is_empty());
    // The end of the bytes ends the list.
    assert_eq!(runs(&[0x11, 0x04, 0x20]), [(0x20, 4)]);
}

#[test]
fn a_hole_after_a_high_length_byte_keeps_the_c_quirk() {
    let _g = setup();
    // run[off + sz - 1] with sz == 0 reads the length byte 0x80: tmp = -1, cn = prev - 1.
    let run = [0x11, 0x02, 0x50, 0x01, 0x80, 0x00];
    assert_eq!(runs(&run), [(0x50, 2), (0x4F, 0x80)]);
}

/// A 1 KB record with `magic`, its update sequence array at 0x30, and `usn` stamped at the
/// end of each sector over the original bytes 0x1111 and 0x2222.
fn fixed_record(magic: u32, usn: u16) -> Vec<u8> {
    let mut r = vec![0u8; 1024];
    r[0..4].copy_from_slice(&magic.to_ne_bytes());
    r[4..6].copy_from_slice(&0x30u16.to_ne_bytes());
    r[6..8].copy_from_slice(&3u16.to_ne_bytes());
    r[0x30..0x32].copy_from_slice(&usn.to_ne_bytes());
    r[0x32..0x34].copy_from_slice(&0x1111u16.to_ne_bytes());
    r[0x34..0x36].copy_from_slice(&0x2222u16.to_ne_bytes());
    r[510..512].copy_from_slice(&usn.to_ne_bytes());
    r[1022..1024].copy_from_slice(&usn.to_ne_bytes());
    r
}

#[test]
fn fixups_put_back_the_sector_ends() {
    let m = ntmp();
    let mut r = fixed_record(NTFS_FILEMAGIC, 7);
    ntfs_procfixups(&m, NTFS_FILEMAGIC, &mut r, 1024).unwrap();
    assert_eq!(&r[510..512], &0x1111u16.to_ne_bytes());
    assert_eq!(&r[1022..1024], &0x2222u16.to_ne_bytes());

    let mut r = fixed_record(NTFS_INDXMAGIC, 7);
    assert_eq!(
        ntfs_procfixups(&m, NTFS_FILEMAGIC, &mut r, 1024),
        Err(Errno::EINVAL)
    );
    let mut r = fixed_record(NTFS_FILEMAGIC, 7);
    assert_eq!(
        ntfs_procfixups(&m, NTFS_FILEMAGIC, &mut r, 512),
        Err(Errno::EINVAL)
    );
    let mut r = fixed_record(NTFS_FILEMAGIC, 7);
    r[1022] ^= 1; // a torn write
    assert_eq!(
        ntfs_procfixups(&m, NTFS_FILEMAGIC, &mut r, 1024),
        Err(Errno::EINVAL)
    );
    let mut r = fixed_record(NTFS_FILEMAGIC, 7);
    r[4..6].copy_from_slice(&0xFFFFu16.to_ne_bytes()); // array past the record size
    assert_eq!(
        ntfs_procfixups(&m, NTFS_FILEMAGIC, &mut r, 1024),
        Err(Errno::EINVAL)
    );
}

fn wide(s: &str) -> Vec<Wchar> {
    s.encode_utf16().collect()
}

#[test]
fn names_compare_with_and_without_case() {
    let _g = setup();
    let tab: Vec<Wchar> = (0..=0xFFFFu16)
        .map(|c| {
            if (0x61..=0x7a).contains(&c) {
                c - 0x20
            } else {
                c
            }
        })
        .collect();
    ntfs_toupper_set(Box::leak(tab.into_boxed_slice()));
    let m = ntmp();

    assert_eq!(ntfs_uastrcmp(&m, &wide("hello"), b"hello"), 0);
    assert!(ntfs_uastrcmp(&m, &wide("hello"), b"HELLO") > 0);
    assert_eq!(ntfs_uastricmp(&m, &wide("hello"), b"HELLO"), 0);
    assert!(ntfs_uastricmp(&m, &wide("abc"), b"abd") < 0);
    // A prefix is smaller, a longer name larger.
    assert_eq!(ntfs_uastricmp(&m, &wide("ab"), b"abc"), -1);
    assert_eq!(ntfs_uastricmp(&m, &wide("abc"), b"ab"), 1);
    assert_eq!(ntfs_uastrcmp(&m, &wide("é"), "é".as_bytes()), 0);
    assert_eq!(NTFS_TOUPPER(u16::from(b'q')), u16::from(b'Q'));
    // Only the low byte indexes the table, as in C.
    assert_eq!(NTFS_TOUPPER(0x0161), u16::from(b'A'));
    assert_eq!(NTFS_U28(0x41), b'A');
    assert_eq!(NTFS_U28(0x10), b'_');
    ntfs_toupper_reset();
}

#[test]
fn attribute_names_parse_against_attrdef() {
    let _g = setup();
    let m = ntmp();
    let mut defs = Vec::new();
    for (name, t) in [
        (&b"$DATA"[..], NTFS_A_DATA),
        (b"$INDEX_ROOT", NTFS_A_INDXROOT),
    ] {
        let mut ad_name = [0u8; 0x40];
        ad_name[..name.len()].copy_from_slice(name);
        defs.push(Ntvattrdef {
            ad_name,
            ad_namelen: name.len() as i32,
            ad_type: t,
        });
    }
    let defs = Box::leak(defs.into_boxed_slice());
    m.ntm_ad.set(NonNull::new(defs.as_mut_ptr()));
    m.ntm_adnum.set(2);

    let parse = |s: &[u8]| {
        let mut t = 0xdead;
        let mut n = None;
        let r = ntfs_ntlookupattr(&m, s, &mut t, &mut n);
        let name = n.map(|n: AttrNameBuf| {
            let v = n.bytes().to_vec();
            attrname_free(n);
            v
        });
        (r, t, name)
    };
    assert_eq!(parse(b""), (Ok(()), 0xdead, None));
    assert_eq!(parse(b"$DATA"), (Ok(()), NTFS_A_DATA, None));
    assert_eq!(parse(b"$INDEX_ROOT"), (Ok(()), NTFS_A_INDXROOT, None));
    assert_eq!(
        parse(b"$INDEX_ROOT:foo"),
        (Ok(()), NTFS_A_DATA, Some(b"foo".to_vec()))
    );
    assert_eq!(
        parse(b"stream"),
        (Ok(()), NTFS_A_DATA, Some(b"stream".to_vec()))
    );
    assert_eq!(parse(b"$BOGUS"), (Err(Errno::ENOENT), 0xdead, None));
}

#[test]
fn times_count_from_1601_in_100ns() {
    let t = ntfs_nttimetounix(116_444_736_000_000_000);
    assert_eq!((t.tv_sec, t.tv_nsec), (0, 0));
    let t = ntfs_nttimetounix(116_444_736_000_000_000 + 10_000_000 * 1_000_000_000 + 1234);
    assert_eq!((t.tv_sec, t.tv_nsec), (1_000_000_000, 123_400));
    // Before 1970 the C's unsigned arithmetic wraps to a negative second.
    assert_eq!(ntfs_nttimetounix(0).tv_sec, -11_644_473_600);
}

#[test]
fn dos_names_are_hidden_unless_allnames() {
    let m = ntmp();
    let mut iep = AttrIndexentry::read(&[], 0);
    for (t, shown) in [(0, true), (1, true), (2, false), (3, true), (7, false)] {
        iep.ie_fnametype = t;
        assert_eq!(ntfs_isnamepermitted(&m, &iep), shown, "type {}", t);
    }
    m.ntm_flag.set(NTFS_MFLAG_ALLNAMES);
    iep.ie_fnametype = 2;
    assert!(ntfs_isnamepermitted(&m, &iep));
}
