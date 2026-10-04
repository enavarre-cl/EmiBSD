//! Host tests of the NTFS generator: the structures a reader checks, read back from the image
//! by a small decoder of their own. The `macos_` tests (ignored: they need macOS's NTFS
//! driver) run the cross-check: `cargo test -p xtask -- --ignored macos_`.

use std::fs;
use std::process::Command;

use super::*;

fn get32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn get64(b: &[u8], o: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(v)
}

/// Undoes the update sequence fixups of a protected structure, checking every sector.
fn unprotect(b: &mut [u8]) {
    let usa = usize::from(get16(b, 4));
    let count = usize::from(get16(b, 6));
    assert_eq!(count - 1, b.len() / SECTOR, "usa count");
    let usn = get16(b, usa);
    assert_ne!(usn, 0);
    for s in 0..count - 1 {
        let end = s * SECTOR + SECTOR - 2;
        assert_eq!(get16(b, end), usn, "sector {s} lacks the usn");
        let saved = get16(b, usa + 2 + 2 * s);
        b[end..end + 2].copy_from_slice(&saved.to_le_bytes());
    }
}

/// Decodes a mapping pairs array into (clusters, first cluster or None) runs.
fn decode_runs(b: &[u8]) -> Vec<(u64, Option<u64>)> {
    let mut runs = Vec::new();
    let mut i = 0;
    let mut lcn = 0_i64;
    while b[i] != 0 {
        let (lb, ob) = (usize::from(b[i] & 0xf), usize::from(b[i] >> 4));
        i += 1;
        let signed = |s: &[u8]| -> i64 {
            let mut v = [if s[s.len() - 1] & 0x80 != 0 { 0xff } else { 0 }; 8];
            v[..s.len()].copy_from_slice(s);
            i64::from_le_bytes(v)
        };
        let len = signed(&b[i..i + lb]);
        assert!(len > 0);
        i += lb;
        if ob == 0 {
            runs.push((len as u64, None));
        } else {
            lcn += signed(&b[i..i + ob]);
            runs.push((len as u64, Some(lcn as u64)));
        }
        i += ob;
    }
    runs
}

fn image() -> Vec<u8> {
    let (label, files) = test_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    build_image(label, &files).unwrap()
}

/// Record `n` of the MFT, fixups undone.
fn rec(img: &[u8], n: usize) -> Vec<u8> {
    let o = MFT_LCN as usize * CLUSTER + n * RECORD;
    let mut r = img[o..o + RECORD].to_vec();
    unprotect(&mut r);
    r
}

/// The attributes of a record: (type, name, header offset).
fn attrs(r: &[u8]) -> Vec<(u32, String, usize)> {
    let mut out = Vec::new();
    let mut o = usize::from(get16(r, 20));
    while get32(r, o) != 0xffff_ffff {
        let len = get32(r, o + 4) as usize;
        assert!(len >= 24 && len.is_multiple_of(8));
        let nl = usize::from(r[o + 9]);
        let no = usize::from(get16(r, o + 10));
        let name: Vec<u16> = (0..nl).map(|i| get16(r, o + no + 2 * i)).collect();
        out.push((get32(r, o), String::from_utf16(&name).unwrap(), o));
        o += len;
    }
    assert_eq!(get32(r, 24) as usize, o + 8, "bytes in use");
    out
}

/// The resident value of the attribute at `o`.
fn value(r: &[u8], o: usize) -> &[u8] {
    assert_eq!(r[o + 8], 0);
    let len = get32(r, o + 16) as usize;
    let off = usize::from(get16(r, o + 20));
    &r[o + off..o + off + len]
}

/// The bytes of the attribute at `o`, resident or not.
fn contents(img: &[u8], r: &[u8], o: usize) -> Vec<u8> {
    if r[o + 8] == 0 {
        return value(r, o).to_vec();
    }
    let runs = decode_runs(&r[o + usize::from(get16(r, o + 32))..]);
    let mut v = Vec::new();
    for (len, lcn) in runs {
        match lcn {
            Some(l) => {
                v.extend_from_slice(&img[l as usize * CLUSTER..(l + len) as usize * CLUSTER])
            }
            None => v.resize(v.len() + len as usize * CLUSTER, 0),
        }
    }
    v.truncate(get64(r, o + 48) as usize);
    v
}

fn find(r: &[u8], ty: u32, name: &str) -> usize {
    attrs(r)
        .into_iter()
        .find(|a| a.0 == ty && a.1 == name)
        .map(|a| a.2)
        .unwrap()
}

#[test]
fn boot_sector_fields_and_backup() {
    let img = image();
    assert_eq!(img.len(), DEVICE_BYTES);
    assert_eq!(&img[3..11], b"NTFS    ");
    assert_eq!(get16(&img, 11), 512);
    assert_eq!(img[13], 8);
    assert_eq!(get64(&img, 40), (DEVICE_BYTES / SECTOR - 1) as u64);
    assert_eq!(get64(&img, 48), MFT_LCN);
    assert_eq!(get64(&img, 56), MFTMIRR_LCN);
    assert_eq!(img[64], 0xf6, "1 KiB records");
    assert_eq!(img[68], 1, "one cluster per index block");
    assert_eq!(&img[510..512], &[0x55, 0xaa]);
    assert_eq!(&img[..SECTOR], &img[DEVICE_BYTES - SECTOR..]);
}

#[test]
fn fixups_apply_and_undo() {
    let mut b: Vec<u8> = (0..RECORD).map(|i| (i * 7) as u8).collect();
    let orig = b.clone();
    protect(&mut b, 0x30);
    assert_eq!(get16(&b, 510), USN);
    assert_eq!(get16(&b, 1022), USN);
    unprotect(&mut b);
    // Only the header's usa fields and the array itself differ.
    assert_eq!(b[8..0x30], orig[8..0x30]);
    assert_eq!(b[0x36..], orig[0x36..]);
}

#[test]
fn records_have_magic_numbers_and_sequences() {
    let img = image();
    for n in 0..MFT_RECORDS {
        let r = rec(&img, n);
        assert_eq!(&r[0..4], b"FILE", "record {n}");
        assert_eq!(get16(&r, 4), 0x30);
        assert_eq!(get32(&r, 44) as usize, n);
        assert_eq!(get32(&r, 28) as usize, RECORD);
        let in_use = get16(&r, 22) & MFT_RECORD_IN_USE != 0;
        assert_eq!(
            in_use,
            n < 16 || (FIRST_USER_RECORD..FIRST_USER_RECORD + 2).contains(&n)
        );
        if in_use {
            assert_eq!(get16(&r, 16), seq_of(n));
            let _ = attrs(&r);
        }
    }
    let root = rec(&img, FILE_ROOT);
    assert_ne!(get16(&root, 22) & MFT_RECORD_IS_DIRECTORY, 0);
}

#[test]
fn mft_bitmap_matches_records_in_use() {
    let img = image();
    let r = rec(&img, 0);
    let bmp = contents(&img, &r, find(&r, AT_BITMAP, ""));
    assert_eq!(bmp.len(), MFT_RECORDS / 8);
    for n in 0..MFT_RECORDS {
        let bit = bmp[n / 8] & (1 << (n % 8)) != 0;
        let used = get16(&rec(&img, n), 22) & MFT_RECORD_IN_USE != 0;
        assert_eq!(bit, used, "record {n}");
    }
    let data = find(&r, AT_DATA, "");
    assert_eq!(get64(&r, data + 48) as usize, MFT_RECORDS * RECORD);
}

#[test]
fn mirror_copies_the_first_records() {
    let img = image();
    let m = MFTMIRR_LCN as usize * CLUSTER;
    let o = MFT_LCN as usize * CLUSTER;
    assert_eq!(img[m..m + 4 * RECORD], img[o..o + 4 * RECORD]);
}

#[test]
fn run_lists_decode_back_to_the_clusters_written() {
    let runs = [
        (3, Some(100)),
        (2, None),
        (200, Some(40)),
        (1, Some(1_000_000)),
    ];
    assert_eq!(decode_runs(&encode_runs(&runs)), runs);
    let img = image();
    let (_, files) = test_files();
    for (i, (_, data)) in files.iter().enumerate() {
        let r = rec(&img, FIRST_USER_RECORD + i);
        let o = find(&r, AT_DATA, "");
        assert_eq!(r[o + 8] != 0, data.len() > RESIDENT_MAX);
        assert_eq!(&contents(&img, &r, o), data);
    }
    let up = rec(&img, 10);
    let table = contents(&img, &up, find(&up, AT_DATA, ""));
    assert_eq!(table.len(), 0x20000);
    assert_eq!(get16(&table, 2 * usize::from(b'a')), u16::from(b'A'));
    assert_eq!(get16(&table, 2 * 0xe9), 0xc9);
    let ad = rec(&img, 4);
    let defs = contents(&img, &ad, find(&ad, AT_DATA, ""));
    assert_eq!(defs.len() % 160, 0);
    assert!(defs[defs.len() - 160..].iter().all(|&b| b == 0));
}

#[test]
fn cluster_bitmap_covers_every_run() {
    let img = image();
    let bm = rec(&img, 6);
    let bitmap = contents(&img, &bm, find(&bm, AT_DATA, ""));
    let bit = |c: u64| bitmap[c as usize / 8] & (1 << (c % 8)) != 0;
    for n in 0..MFT_RECORDS {
        let r = rec(&img, n);
        if get16(&r, 22) & MFT_RECORD_IN_USE == 0 {
            continue;
        }
        for (_, _, o) in attrs(&r) {
            if r[o + 8] == 0 {
                continue;
            }
            for (len, lcn) in decode_runs(&r[o + usize::from(get16(&r, o + 32))..]) {
                if let Some(l) = lcn {
                    assert!((l..l + len).all(bit), "record {n}");
                }
            }
        }
    }
    assert!(!bit(DATA_LCN + 3));
    assert!(bit(NR_CLUSTERS));
}

#[test]
fn root_index_is_sorted_by_collation() {
    let img = image();
    let root = rec(&img, FILE_ROOT);
    let ir = value(&root, find(&root, AT_INDEX_ROOT, "$I30"));
    assert_eq!(get32(ir, 0), AT_FILE_NAME);
    assert_eq!(get32(ir, 8) as usize, INDEX_BLOCK);
    assert_eq!(ir[28], 1, "large index");
    let e = 32;
    assert_eq!(get16(ir, e + 12), INDEX_ENTRY_NODE | INDEX_ENTRY_END);
    assert_eq!(get64(ir, e + usize::from(get16(ir, e + 8)) - 8), 0, "VCN 0");

    let o = ROOTIDX_LCN as usize * CLUSTER;
    let mut ia = img[o..o + INDEX_BLOCK].to_vec();
    assert_eq!(&ia[0..4], b"INDX");
    unprotect(&mut ia);
    let mut p = 24 + get32(&ia, 24) as usize;
    let end = 24 + get32(&ia, 28) as usize;
    let upcase = upcase_table();
    let mut names: Vec<Vec<u16>> = Vec::new();
    loop {
        assert!(p < end);
        let flags = get16(&ia, p + 12);
        if flags & INDEX_ENTRY_END != 0 {
            break;
        }
        let key = p + 16;
        let nl = usize::from(ia[key + 64]);
        names.push((0..nl).map(|i| get16(&ia, key + 66 + 2 * i)).collect());
        let file = get64(&ia, p) & 0xffff_ffff_ffff;
        let r = rec(&img, file as usize);
        assert_ne!(get16(&r, 22) & MFT_RECORD_IN_USE, 0);
        let is_dir = get16(&r, 22) & MFT_RECORD_IS_DIRECTORY != 0;
        assert_eq!(get32(&ia, key + 56) & FILE_ATTR_DIR_INDEX != 0, is_dir);
        p += usize::from(get16(&ia, p + 8));
    }
    assert_eq!(names.len(), 14);
    for w in names.windows(2) {
        assert_eq!(collate(&upcase, &w[0], &w[1]), std::cmp::Ordering::Less);
    }
    let text: Vec<String> = names
        .iter()
        .map(|n| String::from_utf16(n).unwrap())
        .collect();
    assert_eq!(text[0], "$AttrDef");
    assert_eq!(text[11], ".");
    assert_eq!(text[12..], ["m10d-ntfs-big.txt", "m10d-ntfs.txt"]);
}

#[test]
fn volume_is_ntfs_3_1_with_its_label() {
    let img = image();
    let v = rec(&img, 3);
    let info = value(&v, find(&v, 0x70, ""));
    assert_eq!((info[8], info[9]), (3, 1));
    let label = value(&v, find(&v, 0x60, ""));
    assert_eq!(label, &[b'M', 0, b'1', 0, b'0', 0, b'D', 0]);
}

#[test]
fn collation_upcases_first() {
    let up = upcase_table();
    let w = |s: &str| utf16(s);
    assert_eq!(collate(&up, &w("abc"), &w("ABD")), std::cmp::Ordering::Less);
    assert_eq!(
        collate(&up, &w("$MFT"), &w("$MFTMirr")),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        collate(&up, &w("Zeta"), &w("alpha")),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn bad_input_is_refused() {
    assert!(build_image("L", &[("a/b", b"x")]).is_err());
    assert!(build_image("L", &[("a", b"x"), ("A", b"y")]).is_err());
    assert!(build_image("", &[]).is_err());
    let big = vec![0u8; 3 << 20];
    assert!(build_image("L", &[("big", &big)]).is_err());
}

/// Writes `img` to a file of its own in the temporary directory, runs the macOS check on it
/// and makes sure nothing stays attached.
fn macos_check(tag: &str, img: &[u8]) -> Result<()> {
    let path =
        std::env::temp_dir().join(format!("emibsd-ntfsgen-{tag}-{}.img", std::process::id()));
    fs::write(&path, img).unwrap();
    let (_, files) = test_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    let r = check_on_macos(&path, &files);
    let info = Command::new(HDIUTIL).arg("info").output().unwrap();
    let attached = String::from_utf8_lossy(&info.stdout).contains(&*path.to_string_lossy());
    fs::remove_file(&path).unwrap();
    assert!(!attached, "{} left attached", path.display());
    r
}

#[test]
#[ignore = "needs macOS's NTFS driver"]
fn macos_reads_the_test_volume() {
    macos_check("good", &image()).unwrap();
}

/// A volume whose file differs from what the check expects, and one macOS cannot mount,
/// fail the check, and both are detached.
#[test]
#[ignore = "needs macOS's NTFS driver"]
fn macos_check_fails_cleanly() {
    let mut img = image();
    let at = img
        .windows(13)
        .position(|w| w == b"m10d-ntfs-42\n")
        .unwrap();
    img[at] = b'M';
    let e = macos_check("differs", &img).unwrap_err().to_string();
    assert!(e.contains("not the 13 written"), "{e}");
    let mut img = image();
    img[3..11].copy_from_slice(b"NOTNTFS ");
    assert!(macos_check("broken", &img).is_err());
}
