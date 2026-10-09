/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! `cargo xtask ntfs-image <out.img> [--check]`: a minimal NTFS 3.1 volume of our own (M10d).
//!
//! EmiBSD's own tool, not OpenBSD code: OpenBSD has no NTFS writer, and the usual one
//! (ntfs-3g's mkntfs) does not build on macOS. It is written from the public description of
//! the on-disk format (the Linux-NTFS project's documentation), not from any implementation.
//! Every image it makes for the ramdisk is mounted by macOS's own read-only NTFS driver
//! (`/System/Library/Filesystems/ntfs.fs`), an independent reader, before it is used.
//!
//! The volume (`build_image`) is 4 MiB of 512-byte sectors and 4 KiB clusters, with 1 KiB
//! MFT records and 4 KiB index blocks. Cluster map:
//!
//! ```text
//! 0..2     $Boot (the boot sector, then zeros)     2       $MFT:$BITMAP
//! 3        $AttrDef (15 definitions + terminator)  4..20   $MFT (64 records)
//! 20..52   $UpCase (65,536 UTF-16 code units)      52      the root's index block
//! 53       $Bitmap                                 54..    the files' non-resident data
//! 511      $MFTMirr (records 0..3)                 512..768 $LogFile (all 0xff: empty)
//! ```
//!
//! Records 0..11 are the system files (`$MFT` .. `$Extend`); 12..15 are reserved, in use and
//! empty; 16..23 are free, as Windows leaves them; the files are records 24 and up. A file of
//! up to `RESIDENT_MAX` bytes is resident in its record, a larger one gets clusters. The root
//! directory is a large index: its `$INDEX_ROOT` holds only the end entry, which points to
//! one `INDX` block holding every name, sorted by NTFS's collation (both names upcased with
//! `$UpCase`, compared as UTF-16 code units). The backup boot sector is the device's last
//! sector, one past the volume's last counted sector.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// Bytes per sector.
pub(crate) const SECTOR: usize = 512;
/// Bytes per cluster.
pub(crate) const CLUSTER: usize = 4096;
/// Bytes per MFT record.
pub(crate) const RECORD: usize = 1024;
/// Bytes per index block.
pub(crate) const INDEX_BLOCK: usize = 4096;
/// The device size; the volume is one sector less (the backup boot sector).
pub(crate) const DEVICE_BYTES: usize = 4 << 20;
/// Clusters in the volume (`(sectors - 1) / sectors-per-cluster`).
pub(crate) const NR_CLUSTERS: u64 = ((DEVICE_BYTES / SECTOR - 1) / (CLUSTER / SECTOR)) as u64;
/// Records in `$MFT`.
pub(crate) const MFT_RECORDS: usize = 64;
/// The first record of a file of ours.
pub(crate) const FIRST_USER_RECORD: usize = 24;
/// The largest file kept resident in its record.
pub(crate) const RESIDENT_MAX: usize = 512;

/// `$Boot`'s size: the boot sector and its (here empty) boot code.
const BOOT_BYTES: usize = 2 * CLUSTER;
/// `$MFT`'s size.
const MFT_BYTES: usize = MFT_RECORDS * RECORD;
/// The records `$MFTMirr` copies.
const MIRROR_RECORDS: usize = 4;
/// `$LogFile`'s size: enough for a reader's minimum with 16 KiB log pages.
const LOGFILE_BYTES: usize = 1 << 20;
/// `$UpCase`'s size.
const UPCASE_BYTES: usize = 0x10000 * 2;
/// `$AttrDef`'s size: 15 definitions and the terminator, 160 bytes each.
const ATTRDEF_BYTES: usize = 16 * 160;
/// `$Bitmap`'s size: a bit per cluster, in whole 8-byte words.
const BITMAP_BYTES: usize = (NR_CLUSTERS as usize).div_ceil(64) * 8;

/// `$MFT:$BITMAP`.
pub(crate) const MFTBMP_LCN: u64 = 2;
/// `$AttrDef`.
pub(crate) const ATTRDEF_LCN: u64 = 3;
/// `$MFT`.
pub(crate) const MFT_LCN: u64 = 4;
/// `$UpCase`.
pub(crate) const UPCASE_LCN: u64 = MFT_LCN + (MFT_BYTES / CLUSTER) as u64;
/// The root's index block.
pub(crate) const ROOTIDX_LCN: u64 = UPCASE_LCN + (UPCASE_BYTES / CLUSTER) as u64;
/// `$Bitmap`.
pub(crate) const BITMAP_LCN: u64 = ROOTIDX_LCN + 1;
/// The first cluster of the files' data.
pub(crate) const DATA_LCN: u64 = BITMAP_LCN + 1;
/// `$MFTMirr`, in the middle of the volume.
pub(crate) const MFTMIRR_LCN: u64 = NR_CLUSTERS / 2;
/// `$LogFile`, after the mirror.
pub(crate) const LOGFILE_LCN: u64 = MFTMIRR_LCN + 1;

/// The system files' unnamed non-resident `$DATA`: (record, first cluster, bytes).
const EXTENTS: [(usize, u64, usize); 7] = [
    (0, MFT_LCN, MFT_BYTES),
    (1, MFTMIRR_LCN, MIRROR_RECORDS * RECORD),
    (2, LOGFILE_LCN, LOGFILE_BYTES),
    (4, ATTRDEF_LCN, ATTRDEF_BYTES),
    (6, BITMAP_LCN, BITMAP_BYTES),
    (7, 0, BOOT_BYTES),
    (10, UPCASE_LCN, UPCASE_BYTES),
];

/// Attribute types.
pub(crate) const AT_STANDARD_INFORMATION: u32 = 0x10;
pub(crate) const AT_FILE_NAME: u32 = 0x30;
const AT_VOLUME_NAME: u32 = 0x60;
const AT_VOLUME_INFORMATION: u32 = 0x70;
pub(crate) const AT_DATA: u32 = 0x80;
pub(crate) const AT_INDEX_ROOT: u32 = 0x90;
pub(crate) const AT_INDEX_ALLOCATION: u32 = 0xa0;
pub(crate) const AT_BITMAP: u32 = 0xb0;
const AT_END: u32 = 0xffff_ffff;

/// Record flags.
pub(crate) const MFT_RECORD_IN_USE: u16 = 0x1;
pub(crate) const MFT_RECORD_IS_DIRECTORY: u16 = 0x2;
const MFT_RECORD_IS_VIEW_INDEX: u16 = 0x8;

/// File attribute flags.
const FILE_ATTR_HIDDEN: u32 = 0x2;
const FILE_ATTR_SYSTEM: u32 = 0x4;
const FILE_ATTR_ARCHIVE: u32 = 0x20;
pub(crate) const FILE_ATTR_DIR_INDEX: u32 = 0x1000_0000;
const FILE_ATTR_VIEW_INDEX: u32 = 0x2000_0000;

/// File name namespaces.
const NAMESPACE_WIN32: u8 = 1;
const NAMESPACE_WIN32_AND_DOS: u8 = 3;

/// Index entry flags.
pub(crate) const INDEX_ENTRY_NODE: u16 = 1;
pub(crate) const INDEX_ENTRY_END: u16 = 2;

/// Collation rules.
const COLLATION_FILE_NAME: u32 = 1;
const COLLATION_NTOFS_ULONG: u32 = 0x10;
const COLLATION_NTOFS_SECURITY_HASH: u32 = 0x12;

/// The update sequence number every protected structure carries.
pub(crate) const USN: u16 = 1;
/// The records' update sequence array offset (NTFS 3.1 records keep their number at 0x2c).
const RECORD_USA_OFS: usize = 0x30;
/// An index block's update sequence array offset.
const INDX_USA_OFS: usize = 0x28;

/// Every time stamp: 2026-01-01 00:00:00 UTC in 100 ns units since 1601.
const NT_TIME: u64 = (1_767_225_600 + 11_644_473_600) * 10_000_000;
/// The volume serial number.
const SERIAL: u64 = 0x454d_4942_5344_4e54;

/// The system files of records 0..11: (record, name).
const SYSTEM_FILES: [(usize, &str); 12] = [
    (0, "$MFT"),
    (1, "$MFTMirr"),
    (2, "$LogFile"),
    (3, "$Volume"),
    (4, "$AttrDef"),
    (5, "."),
    (6, "$Bitmap"),
    (7, "$Boot"),
    (8, "$BadClus"),
    (9, "$Secure"),
    (10, "$UpCase"),
    (11, "$Extend"),
];

/// The root directory's record.
pub(crate) const FILE_ROOT: usize = 5;

/// The volume label and files of the M10d test image: `m10d-ntfs.txt` (resident) and
/// `m10d-ntfs-big.txt` (12,000 bytes in three clusters; its last line is
/// `m10d-ntfs-big-line-0499`).
pub(crate) fn test_files() -> (&'static str, Vec<(String, Vec<u8>)>) {
    let big: String = (0..500)
        .map(|i| format!("m10d-ntfs-big-line-{i:04}\n"))
        .collect();
    (
        "M10D",
        vec![
            ("m10d-ntfs.txt".to_string(), b"m10d-ntfs-42\n".to_vec()),
            ("m10d-ntfs-big.txt".to_string(), big.into_bytes()),
        ],
    )
}

/// Writes the volume of `files` (name, contents), labelled `label`, to `path`.
pub(crate) fn make_ntfs_image(path: &Path, label: &str, files: &[(&str, &[u8])]) -> Result<()> {
    let image = build_image(label, files)?;
    fs::write(path, image).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// `cargo xtask ntfs-image <out> [--check]`: the M10d test volume at `out`, mounted by macOS's
/// NTFS driver with `--check`.
pub(crate) fn ntfs_image(out: &Path, check: bool) -> Result<()> {
    let (label, files) = test_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    make_ntfs_image(out, label, &files)?;
    println!("ntfs-image: {} ({DEVICE_BYTES} bytes)", out.display());
    if check {
        check_on_macos(out, &files)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Little-endian helpers.

fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn get16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn align8(n: usize) -> usize {
    (n + 7) & !7
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn utf16_bytes(s: &[u16]) -> Vec<u8> {
    s.iter().flat_map(|c| c.to_le_bytes()).collect()
}

/// A file reference: the record number and its sequence number.
pub(crate) fn mref(record: usize, seq: u16) -> u64 {
    record as u64 | (u64::from(seq) << 48)
}

/// The sequence number of record `n`: the system files carry their record number ($MFT 1),
/// the files of ours 1.
pub(crate) fn seq_of(n: usize) -> u16 {
    if (1..16).contains(&n) { n as u16 } else { 1 }
}

// ---------------------------------------------------------------------------------------------
// $UpCase and the collation of names.

/// `$UpCase`: the upper case of every UTF-16 code unit, where it is one code unit of the
/// Basic Multilingual Plane (Unicode's simple mapping); every other unit maps to itself.
pub(crate) fn upcase_table() -> Vec<u16> {
    (0..=0xffff_u32)
        .map(|c| {
            let Some(ch) = char::from_u32(c) else {
                return c as u16;
            };
            let mut up = ch.to_uppercase();
            match (up.next(), up.next()) {
                (Some(u), None) if (u as u32) <= 0xffff => u as u32 as u16,
                _ => c as u16,
            }
        })
        .collect()
}

/// NTFS's file name collation: the names upcased, compared unit by unit (a prefix first); equal
/// names then compare as they are.
pub(crate) fn collate(upcase: &[u16], a: &[u16], b: &[u16]) -> std::cmp::Ordering {
    let up = |s: &[u16]| -> Vec<u16> { s.iter().map(|&c| upcase[usize::from(c)]).collect() };
    up(a).cmp(&up(b)).then_with(|| a.cmp(b))
}

// ---------------------------------------------------------------------------------------------
// Update sequence fixups.

/// Protects a multi-sector structure: the update sequence array at `usa_ofs` gets `USN` and
/// the last two bytes of every sector, which are replaced by `USN`.
pub(crate) fn protect(b: &mut [u8], usa_ofs: usize) {
    let sectors = b.len() / SECTOR;
    put16(b, 4, usa_ofs as u16);
    put16(b, 6, (sectors + 1) as u16);
    put16(b, usa_ofs, USN);
    for s in 0..sectors {
        let end = s * SECTOR + SECTOR - 2;
        let saved = get16(b, end);
        put16(b, usa_ofs + 2 + 2 * s, saved);
        put16(b, end, USN);
    }
}

// ---------------------------------------------------------------------------------------------
// Attributes and records.

/// One attribute of a record: resident bytes, or one extent of clusters.
struct Attr {
    ty: u32,
    name: &'static str,
    resident: Option<Vec<u8>>,
    /// (first cluster or `None` for a hole, clusters, data size) when non-resident.
    extent: (Option<u64>, u64, u64),
}

/// A resident attribute.
fn res(ty: u32, name: &'static str, value: Vec<u8>) -> Attr {
    Attr {
        ty,
        name,
        resident: Some(value),
        extent: (None, 0, 0),
    }
}

/// A non-resident attribute of `size` bytes in the clusters from `lcn` (`None`: a hole).
fn ext(ty: u32, name: &'static str, lcn: Option<u64>, size: u64) -> Attr {
    let clusters = (size as usize).div_ceil(CLUSTER) as u64;
    Attr {
        ty,
        name,
        resident: None,
        extent: (lcn, clusters, size),
    }
}

/// The smallest little-endian two's complement encoding of `v`.
fn signed_bytes(v: i64) -> Vec<u8> {
    let mut n = 1;
    while n < 8 && !(-(1_i64 << (8 * n - 1))..(1_i64 << (8 * n - 1))).contains(&v) {
        n += 1;
    }
    v.to_le_bytes()[..n].to_vec()
}

/// A mapping pairs array: per run a header byte (offset bytes << 4 | length bytes), the
/// length, the cluster delta from the previous run (absent for a hole); then a zero.
pub(crate) fn encode_runs(runs: &[(u64, Option<u64>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = 0_i64;
    for &(len, lcn) in runs {
        let lb = signed_bytes(len as i64);
        let ob = lcn.map_or_else(Vec::new, |lcn| {
            let d = signed_bytes(lcn as i64 - prev);
            prev = lcn as i64;
            d
        });
        out.push(((ob.len() as u8) << 4) | lb.len() as u8);
        out.extend_from_slice(&lb);
        out.extend_from_slice(&ob);
    }
    out.push(0);
    out
}

/// The bytes of attribute `a`, instance `instance`.
fn attr_bytes(a: &Attr, instance: u16) -> Vec<u8> {
    let name = utf16_bytes(&utf16(a.name));
    let (name_off, mut b) = match &a.resident {
        Some(v) => {
            let off = align8(24 + name.len());
            let mut b = vec![0u8; align8(off + v.len())];
            put32(&mut b, 16, v.len() as u32);
            put16(&mut b, 20, off as u16);
            b[22] = u8::from(a.ty == AT_FILE_NAME); // indexed
            b[off..off + v.len()].copy_from_slice(v);
            (24, b)
        }
        None => {
            let (lcn, clusters, size) = a.extent;
            let pairs = encode_runs(&[(clusters, lcn)]);
            let off = align8(64 + name.len());
            let mut b = vec![0u8; align8(off + pairs.len())];
            b[8] = 1;
            put64(&mut b, 24, clusters - 1); // the last VCN (the first is 0)
            put16(&mut b, 32, off as u16);
            put64(&mut b, 40, clusters * CLUSTER as u64);
            put64(&mut b, 48, size);
            put64(&mut b, 56, size);
            b[off..off + pairs.len()].copy_from_slice(&pairs);
            (64, b)
        }
    };
    put32(&mut b, 0, a.ty);
    let len = b.len() as u32;
    put32(&mut b, 4, len);
    b[9] = (name.len() / 2) as u8;
    put16(&mut b, 10, name_off as u16);
    put16(&mut b, 14, instance);
    b[name_off..name_off + name.len()].copy_from_slice(&name);
    b
}

/// MFT record `n` with `flags` and `attrs` (in type, then name order), protected.
fn record(n: usize, flags: u16, attrs: &[Attr]) -> Result<Vec<u8>> {
    let mut r = vec![0u8; RECORD];
    r[0..4].copy_from_slice(b"FILE");
    put16(&mut r, 16, if flags == 0 { 0 } else { seq_of(n) });
    let links = attrs.iter().filter(|a| a.ty == AT_FILE_NAME).count() as u16;
    put16(&mut r, 18, links);
    let mut off = align8(RECORD_USA_OFS + 2 * (RECORD / SECTOR + 1));
    put16(&mut r, 20, off as u16);
    put16(&mut r, 22, flags);
    for (i, a) in attrs.iter().enumerate() {
        let b = attr_bytes(a, i as u16);
        if off + b.len() + 8 > RECORD {
            return Err(format!("ntfsgen: record {n} overflows its {RECORD} bytes").into());
        }
        r[off..off + b.len()].copy_from_slice(&b);
        off += b.len();
    }
    put32(&mut r, off, AT_END);
    put32(&mut r, 24, (off + 8) as u32);
    put32(&mut r, 28, RECORD as u32);
    put16(&mut r, 40, attrs.len() as u16);
    put32(&mut r, 44, n as u32);
    protect(&mut r, RECORD_USA_OFS);
    Ok(r)
}

/// A `$STANDARD_INFORMATION` attribute (the 72-byte NTFS 3.x form).
fn standard_information(attrs: u32) -> Attr {
    let mut v = vec![0u8; 72];
    for o in [0, 8, 16, 24] {
        put64(&mut v, o, NT_TIME);
    }
    put32(&mut v, 32, attrs);
    res(AT_STANDARD_INFORMATION, "", v)
}

/// A `$FILE_NAME` value: in the root, `name`, sizes, flags, namespace.
fn file_name(name: &[u16], alloc: u64, size: u64, attrs: u32, namespace: u8) -> Vec<u8> {
    let mut v = vec![0u8; 66 + 2 * name.len()];
    put64(&mut v, 0, mref(FILE_ROOT, seq_of(FILE_ROOT)));
    for o in [8, 16, 24, 32] {
        put64(&mut v, o, NT_TIME);
    }
    put64(&mut v, 40, alloc);
    put64(&mut v, 48, size);
    put32(&mut v, 56, attrs);
    v[64] = name.len() as u8;
    v[65] = namespace;
    v[66..].copy_from_slice(&utf16_bytes(name));
    v
}

/// An `$INDEX_ROOT` value: the indexed type, its collation and the entries' bytes.
fn index_root(ty: u32, collation: u32, large: bool, entries: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    put32(&mut v, 0, ty);
    put32(&mut v, 4, collation);
    put32(&mut v, 8, INDEX_BLOCK as u32);
    v[12] = (INDEX_BLOCK / CLUSTER) as u8;
    put32(&mut v, 16, 16);
    put32(&mut v, 20, (16 + entries.len()) as u32);
    put32(&mut v, 24, (16 + entries.len()) as u32);
    v[28] = u8::from(large);
    v.extend_from_slice(entries);
    v
}

/// An index entry: the file reference, the key, and the subnode's VCN if any.
fn index_entry(file: u64, key: &[u8], flags: u16, subnode: Option<u64>) -> Vec<u8> {
    let node = if subnode.is_some() {
        INDEX_ENTRY_NODE
    } else {
        0
    };
    let len = align8(16 + key.len()) + 8 * usize::from(node);
    let mut e = vec![0u8; len];
    put64(&mut e, 0, file);
    put16(&mut e, 8, len as u16);
    put16(&mut e, 10, key.len() as u16);
    put16(&mut e, 12, flags | node);
    e[16..16 + key.len()].copy_from_slice(key);
    if let Some(vcn) = subnode {
        put64(&mut e, len - 8, vcn);
    }
    e
}

/// The end entry of an index node without children.
fn end_entry() -> Vec<u8> {
    index_entry(0, &[], INDEX_ENTRY_END, None)
}

/// The standard attribute definitions, then the empty terminator.
fn attrdef() -> Vec<u8> {
    // (name, type, collation, flags, minimum size, maximum size); flags 0x02 indexable,
    // 0x40 always resident, 0x80 logged when non-resident.
    const DEFS: [(&str, u32, u32, u32, u64, i64); 15] = [
        ("$STANDARD_INFORMATION", 0x10, 0, 0x40, 0x30, 0x48),
        ("$ATTRIBUTE_LIST", 0x20, 0, 0x80, 0, -1),
        ("$FILE_NAME", 0x30, 1, 0x42, 0x44, 0x242),
        ("$OBJECT_ID", 0x40, 0, 0x40, 0, 0x100),
        ("$SECURITY_DESCRIPTOR", 0x50, 0, 0x80, 0, -1),
        ("$VOLUME_NAME", 0x60, 0, 0x40, 2, 0x100),
        ("$VOLUME_INFORMATION", 0x70, 0, 0x40, 0xc, 0xc),
        ("$DATA", 0x80, 0, 0, 0, -1),
        ("$INDEX_ROOT", 0x90, 0, 0x40, 0, -1),
        ("$INDEX_ALLOCATION", 0xa0, 0, 0x80, 0, -1),
        ("$BITMAP", 0xb0, 0, 0x80, 0, -1),
        ("$REPARSE_POINT", 0xc0, 0, 0x80, 0, 0x4000),
        ("$EA_INFORMATION", 0xd0, 0, 0x40, 8, 8),
        ("$EA", 0xe0, 0, 0, 0, 0x10000),
        ("$LOGGED_UTILITY_STREAM", 0x100, 0, 0x80, 0, 0x10000),
    ];
    let mut v = vec![0u8; ATTRDEF_BYTES];
    for (d, (name, ty, coll, flags, min, max)) in v.as_chunks_mut::<160>().0.iter_mut().zip(DEFS) {
        let n = utf16_bytes(&utf16(name));
        d[..n.len()].copy_from_slice(&n);
        put32(d, 128, ty);
        put32(d, 136, coll);
        put32(d, 140, flags);
        put64(d, 144, min);
        put64(d, 152, max as u64);
    }
    v
}

/// The boot sector.
fn boot_sector() -> Vec<u8> {
    let mut b = vec![0u8; SECTOR];
    b[0..3].copy_from_slice(&[0xeb, 0x52, 0x90]);
    b[3..11].copy_from_slice(b"NTFS    ");
    put16(&mut b, 11, SECTOR as u16);
    b[13] = (CLUSTER / SECTOR) as u8;
    b[21] = 0xf8; // media: a fixed disk
    put16(&mut b, 24, 63); // sectors per track
    put16(&mut b, 26, 255); // heads
    b[36] = 0x80; // drive number
    b[38] = 0x80; // extended boot signature
    put64(&mut b, 40, (DEVICE_BYTES / SECTOR - 1) as u64);
    put64(&mut b, 48, MFT_LCN);
    put64(&mut b, 56, MFTMIRR_LCN);
    // Records and index blocks: a negative n means 2^-n bytes, a positive one clusters.
    b[64] = (-(RECORD.trailing_zeros() as i8)) as u8;
    b[68] = (INDEX_BLOCK / CLUSTER) as u8;
    put64(&mut b, 72, SERIAL);
    // The boot code the jump lands on: halt forever (the volume does not boot).
    b[0x54..0x58].copy_from_slice(&[0xfa, 0xf4, 0xeb, 0xfd]);
    b[510] = 0x55;
    b[511] = 0xaa;
    b
}

/// A file of ours: its record, name, contents, and first cluster when non-resident.
struct UserFile<'a> {
    record: usize,
    name: Vec<u16>,
    data: &'a [u8],
    lcn: Option<u64>,
}

/// Checks the files' names and places the data of the large ones from `DATA_LCN`.
fn place<'a>(upcase: &[u16], files: &[(&str, &'a [u8])]) -> Result<Vec<UserFile<'a>>> {
    if FIRST_USER_RECORD + files.len() > MFT_RECORDS {
        return Err(format!("ntfsgen: at most {} files", MFT_RECORDS - FIRST_USER_RECORD).into());
    }
    let mut users = Vec::new();
    let mut next = DATA_LCN;
    let mut seen = BTreeSet::new();
    for (i, &(name, data)) in files.iter().enumerate() {
        let n16 = utf16(name);
        if n16.is_empty()
            || n16.len() > 255
            || name.starts_with('$')
            || name.contains(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|'])
        {
            return Err(format!("ntfsgen: {name:?} is not a file name of ours").into());
        }
        let up: Vec<u16> = n16.iter().map(|&c| upcase[usize::from(c)]).collect();
        if !seen.insert(up) {
            return Err(format!("ntfsgen: {name:?} is there twice (names ignore case)").into());
        }
        let mut lcn = None;
        if data.len() > RESIDENT_MAX {
            lcn = Some(next);
            next += data.len().div_ceil(CLUSTER) as u64;
            if next > MFTMIRR_LCN {
                return Err("ntfsgen: the files do not fit in the volume".into());
            }
        }
        users.push(UserFile {
            record: FIRST_USER_RECORD + i,
            name: n16,
            data,
            lcn,
        });
    }
    Ok(users)
}

/// The volume of `files` labelled `label`, `DEVICE_BYTES` long.
pub(crate) fn build_image(label: &str, files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let upcase = upcase_table();
    let label16 = utf16(label);
    if label16.is_empty() || label16.len() > 128 {
        return Err(format!("ntfsgen: label {label:?} must be 1..=128 UTF-16 units").into());
    }
    let users = place(&upcase, files)?;

    // $Bitmap: the clusters of every extent, and the bits past the volume's end.
    let mut extents: Vec<(u64, usize)> = EXTENTS.iter().map(|e| (e.1, e.2)).collect();
    extents.extend([(MFTBMP_LCN, CLUSTER), (ROOTIDX_LCN, INDEX_BLOCK)]);
    extents.extend(
        users
            .iter()
            .filter_map(|f| f.lcn.map(|l| (l, f.data.len()))),
    );
    let mut bitmap = vec![0u8; BITMAP_BYTES];
    let ranges = extents
        .iter()
        .map(|&(l, b)| l..l + b.div_ceil(CLUSTER) as u64);
    for c in ranges.flatten().chain(NR_CLUSTERS..8 * BITMAP_BYTES as u64) {
        bitmap[c as usize / 8] |= 1 << (c % 8);
    }

    // $MFT:$BITMAP: records 0..15 and the files'.
    let mut mftbmp = vec![0u8; MFT_RECORDS / 8];
    for n in (0..16).chain(users.iter().map(|f| f.record)) {
        mftbmp[n / 8] |= 1 << (n % 8);
    }

    // The root's names: the system files (with "." for the root itself) and ours, sorted.
    let sys_attrs = |n: usize| match n {
        5 | 11 => FILE_ATTR_HIDDEN | FILE_ATTR_SYSTEM | FILE_ATTR_DIR_INDEX,
        9 => FILE_ATTR_HIDDEN | FILE_ATTR_SYSTEM | FILE_ATTR_VIEW_INDEX,
        _ => FILE_ATTR_HIDDEN | FILE_ATTR_SYSTEM,
    };
    let mut names: Vec<(Vec<u16>, usize, Vec<u8>)> = Vec::new();
    for (n, name) in SYSTEM_FILES {
        let sizes = EXTENTS.iter().find(|e| e.0 == n);
        let (alloc, size) = sizes.map_or((0, 0), |e| (e.2.next_multiple_of(CLUSTER), e.2));
        let name = utf16(name);
        let (alloc, size) = (alloc as u64, size as u64);
        let key = file_name(&name, alloc, size, sys_attrs(n), NAMESPACE_WIN32_AND_DOS);
        names.push((name, n, key));
    }
    for f in &users {
        let alloc = match f.lcn {
            Some(_) => f.data.len().next_multiple_of(CLUSTER),
            None => align8(f.data.len()),
        };
        let size = f.data.len() as u64;
        let key = file_name(
            &f.name,
            alloc as u64,
            size,
            FILE_ATTR_ARCHIVE,
            NAMESPACE_WIN32,
        );
        names.push((f.name.clone(), f.record, key));
    }
    names.sort_by(|a, b| collate(&upcase, &a.0, &b.0));

    // The root's index block: every name, in a leaf.
    let mut indx = vec![0u8; INDEX_BLOCK];
    indx[0..4].copy_from_slice(b"INDX");
    let first = align8(INDX_USA_OFS + 2 * (INDEX_BLOCK / SECTOR + 1));
    let mut off = first;
    for (_, n, key) in &names {
        let e = index_entry(mref(*n, seq_of(*n)), key, 0, None);
        if off + e.len() + 16 > INDEX_BLOCK {
            return Err("ntfsgen: the names do not fit in the root's index block".into());
        }
        indx[off..off + e.len()].copy_from_slice(&e);
        off += e.len();
    }
    indx[off..off + 16].copy_from_slice(&end_entry());
    off += 16;
    // The index header (at 24): entries, bytes in use, bytes allocated; VCN 0, a leaf.
    put32(&mut indx, 24, (first - 24) as u32);
    put32(&mut indx, 28, (off - 24) as u32);
    put32(&mut indx, 32, (INDEX_BLOCK - 24) as u32);
    protect(&mut indx, INDX_USA_OFS);

    // The records: a system file's standard information and name, its data extent if it
    // has one, then what is its own; the reserved records 12..15; the files; free records.
    let fname = |n: usize| {
        let key = names.iter().find(|e| e.1 == n).map(|e| e.2.clone());
        res(AT_FILE_NAME, "", key.unwrap_or_default())
    };
    let mut records = Vec::with_capacity(MFT_RECORDS);
    for n in 0..MFT_RECORDS {
        let mut flags = MFT_RECORD_IN_USE;
        let mut attrs = Vec::new();
        if n < SYSTEM_FILES.len() {
            attrs.push(standard_information(sys_attrs(n)));
            attrs.push(fname(n));
            if let Some(&(_, lcn, size)) = EXTENTS.iter().find(|e| e.0 == n) {
                attrs.push(ext(AT_DATA, "", Some(lcn), size as u64));
            }
        }
        match n {
            0 => attrs.push(ext(AT_BITMAP, "", Some(MFTBMP_LCN), mftbmp.len() as u64)),
            3 => {
                let mut info = vec![0u8; 12];
                info[8..10].copy_from_slice(&[3, 1]); // NTFS 3.1, no flags
                attrs.push(res(AT_VOLUME_NAME, "", utf16_bytes(&label16)));
                attrs.push(res(AT_VOLUME_INFORMATION, "", info));
                attrs.push(res(AT_DATA, "", Vec::new()));
            }
            5 => {
                flags |= MFT_RECORD_IS_DIRECTORY;
                let node = index_entry(0, &[], INDEX_ENTRY_END, Some(0));
                let root = index_root(AT_FILE_NAME, COLLATION_FILE_NAME, true, &node);
                attrs.push(res(AT_INDEX_ROOT, "$I30", root));
                let size = INDEX_BLOCK as u64;
                attrs.push(ext(AT_INDEX_ALLOCATION, "$I30", Some(ROOTIDX_LCN), size));
                attrs.push(res(AT_BITMAP, "$I30", vec![1, 0, 0, 0, 0, 0, 0, 0]));
            }
            8 => {
                attrs.push(res(AT_DATA, "", Vec::new()));
                attrs.push(ext(AT_DATA, "$Bad", None, NR_CLUSTERS * CLUSTER as u64));
            }
            9 => {
                flags |= MFT_RECORD_IS_VIEW_INDEX;
                let sdh = index_root(0, COLLATION_NTOFS_SECURITY_HASH, false, &end_entry());
                let sii = index_root(0, COLLATION_NTOFS_ULONG, false, &end_entry());
                attrs.push(res(AT_DATA, "$SDS", Vec::new()));
                attrs.push(res(AT_INDEX_ROOT, "$SDH", sdh));
                attrs.push(res(AT_INDEX_ROOT, "$SII", sii));
            }
            11 => {
                flags |= MFT_RECORD_IS_DIRECTORY;
                let root = index_root(AT_FILE_NAME, COLLATION_FILE_NAME, false, &end_entry());
                attrs.push(res(AT_INDEX_ROOT, "$I30", root));
            }
            12..16 => {
                attrs.push(standard_information(FILE_ATTR_HIDDEN | FILE_ATTR_SYSTEM));
                attrs.push(res(AT_DATA, "", Vec::new()));
            }
            _ => match users.iter().find(|f| f.record == n) {
                Some(f) => {
                    attrs.push(standard_information(FILE_ATTR_ARCHIVE));
                    attrs.push(fname(n));
                    attrs.push(match f.lcn {
                        Some(lcn) => ext(AT_DATA, "", Some(lcn), f.data.len() as u64),
                        None => res(AT_DATA, "", f.data.to_vec()),
                    });
                }
                None if n >= SYSTEM_FILES.len() => flags = 0,
                None => {}
            },
        }
        records.push(record(n, flags, &attrs)?);
    }

    // Lay it all out; the backup boot sector is the device's last sector.
    let mut img = vec![0u8; DEVICE_BYTES];
    let boot = boot_sector();
    img[..SECTOR].copy_from_slice(&boot);
    img[DEVICE_BYTES - SECTOR..].copy_from_slice(&boot);
    let mut put = |lcn: u64, bytes: &[u8]| {
        let at = lcn as usize * CLUSTER;
        img[at..at + bytes.len()].copy_from_slice(bytes);
    };
    put(MFTBMP_LCN, &mftbmp);
    put(ATTRDEF_LCN, &attrdef());
    put(MFT_LCN, &records.concat());
    put(MFTMIRR_LCN, &records[..MIRROR_RECORDS].concat());
    put(UPCASE_LCN, &utf16_bytes(&upcase));
    put(ROOTIDX_LCN, &indx);
    put(BITMAP_LCN, &bitmap);
    put(LOGFILE_LCN, &vec![0xff; LOGFILE_BYTES]);
    for f in &users {
        if let Some(lcn) = f.lcn {
            put(lcn, f.data);
        }
    }
    Ok(img)
}

// ---------------------------------------------------------------------------------------------
// The macOS cross-check.

/// macOS's disk image tool.
const HDIUTIL: &str = "/usr/bin/hdiutil";
/// macOS's mount(8) and umount(8).
const MOUNT: &str = "/sbin/mount";
const UMOUNT: &str = "/sbin/umount";
/// macOS's diskutil(8), the fallback mounter.
const DISKUTIL: &str = "/usr/sbin/diskutil";
/// macOS's read-only NTFS driver.
pub(crate) const NTFS_FS: &str = "/System/Library/Filesystems/ntfs.fs";

/// An attached image, detached when dropped (on every path).
struct Attached(String);

impl Drop for Attached {
    fn drop(&mut self) {
        let ok = Command::new(HDIUTIL)
            .args(["detach", "-quiet", &self.0])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            let _ = Command::new(HDIUTIL)
                .args(["detach", "-quiet", "-force", &self.0])
                .status();
        }
    }
}

/// A mounted directory, unmounted (and removed) when dropped.
struct Mounted(PathBuf);

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new(UMOUNT).arg(&self.0).output();
        let _ = fs::remove_dir(&self.0);
    }
}

/// Runs `cmd`, returning its standard output, or an error naming it with its output.
fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{cmd:?} failed ({}):\n{text}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )
        .into())
    }
}

/// Mounts `img` read-only with macOS's NTFS driver and checks that the root holds exactly
/// `files` (besides the `$` system files) with their contents.
pub(crate) fn check_on_macos(img: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    for tool in [HDIUTIL, MOUNT, UMOUNT, DISKUTIL, NTFS_FS] {
        if !Path::new(tool).exists() {
            return Err(format!(
                "{tool} not found: the NTFS image check needs macOS's own NTFS driver \
                 (EMIBSD_NTFS_CHECK=0 skips it, docs/SETUP.md)"
            )
            .into());
        }
    }
    let attach = output(
        Command::new(HDIUTIL)
            .args([
                "attach",
                "-imagekey",
                "diskimage-class=CRawDiskImage",
                "-nomount",
            ])
            .arg(img),
    )?;
    let dev = attach
        .split_whitespace()
        .next()
        .filter(|d| d.starts_with("/dev/disk"))
        .ok_or_else(|| {
            format!(
                "hdiutil attach {}: no /dev/disk in {attach:?}",
                img.display()
            )
        })?
        .to_string();
    let attached = Attached(dev.clone());

    let stem = img.file_stem().unwrap_or_default().to_string_lossy();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("emibsd-ntfs-check-{pid}-{stem}"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mounted = Mounted(dir.clone());
    let plain = output(
        Command::new(MOUNT)
            .args(["-t", "ntfs", "-o", "rdonly", &dev])
            .arg(&dir),
    );
    if let Err(e) = plain {
        output(
            Command::new(DISKUTIL)
                .args(["mount", "readOnly", "-mountPoint"])
                .arg(&dir)
                .arg(&dev),
        )
        .map_err(|e2| {
            format!(
                "macOS's NTFS driver did not mount {}:\n{e}\n{e2}",
                img.display()
            )
        })?;
    }

    let mut listed = BTreeSet::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('$') && !name.starts_with('.') {
            listed.insert(name);
        }
    }
    let expected: BTreeSet<String> = files.iter().map(|(n, _)| (*n).to_string()).collect();
    if listed != expected {
        return Err(format!(
            "{}: macOS lists {listed:?}, the image holds {expected:?}",
            img.display()
        )
        .into());
    }
    for (name, data) in files {
        let path = dir.join(name);
        let got = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if got != *data {
            return Err(format!(
                "{}: macOS reads {} bytes of {name}, not the {} written",
                img.display(),
                got.len(),
                data.len()
            )
            .into());
        }
    }
    drop(mounted);
    drop(attached);
    println!(
        "ntfs-image: macOS's NTFS driver mounted {} and read back {} file(s)",
        img.display(),
        files.len()
    );
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the NTFS generator: the structures a reader checks, read back from the image
    // by a small decoder of their own. The `macos_` tests (ignored: they need macOS's NTFS
    // driver) run the cross-check: `cargo test -p xtask -- --ignored macos_`.

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
}
/* </TESTS> */
