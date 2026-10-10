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
//! M16g: an ISO 9660 image of a directory tree, as OpenBSD's release makes its CD images with
//! `mkhybrid -a -R -T -L -l -d -D -N ... -b BOOT -c CATALOG -e EFIBOOT`
//! (`distrib/amd64/ramdisk_cd/Makefile`, `cd${OSrev}.iso`). mkhybrid is GNU code that is not in
//! the clone (`gnu/usr.sbin/mkhybrid`) and no host tool is installed for it (ROADMAP M16g), so
//! xtask writes the image itself. What each flag means here:
//!
//! - `-a`: every file of the tree goes in; `-D`: directories are never relocated (the trees
//!   are shallow; a tree deeper than ISO 9660's eight levels is refused);
//! - `-l`, `-L`, `-d`, `-N`: ISO 9660 names of up to 31 characters, leading dots allowed, no
//!   trailing period on a name without an extension and no `;1` version. A name is the file's
//!   name in upper case; a character outside ISO 9660's `d-characters` becomes `_`, and so
//!   does every dot of a file name but its last. A directory keeps its dots: OpenBSD's boot
//!   loaders find `/8.0/amd64/bsd.rd` through the path table with no Rock Ridge
//!   (`sys/lib/libsa/cd9660.c`, `pnmatch` compares the upper-cased path component with the
//!   name as it is), and OpenBSD's own CD boots `set image /8.0/amd64/bsd.rd`;
//! - `-R`: Rock Ridge (RRIP 1.09): the root's `.` record holds `SP` and, through a `CE`
//!   continuation area, the `ER` record of `RRIP_1991A` (the kernel takes Rock Ridge only
//!   after both: `cd9660_rrip_offset`); every record holds `RR`, `PX` (the file's mode, owner
//!   root:wheel, as the release is built by root) and `TF`, and `NM` with the real name;
//! - `-T`: a `TRANS.TBL` in every directory, one `F|D ISONAME realname` line per entry, so a
//!   reader without Rock Ridge can map the names back (mkhybrid's column widths are not
//!   checked: its sources are not in the clone);
//! - `-b`, `-c`, `-e`: an El Torito boot catalogue at `-c` (a file of the tree, one sector)
//!   with the BIOS image `-b` as the default no-emulation entry and the EFI image `-e` (a FAT
//!   file system) in a section of platform `0xEF`. Without `-b` (EmiBSD builds no BIOS boot
//!   programs) the EFI image is the default entry and the validation entry names the EFI
//!   platform.
//!
//! Every timestamp is the one given ([`Options::time`]), so the same tree makes the same
//! image. The volume descriptor strings (`-A`, `-P`, `-p`, `-V`) are the caller's.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::Result;

/// ISO 9660's logical block.
pub(crate) const SECTOR: usize = 2048;

/// The first sector after the system area: the primary volume descriptor.
const PVD_SECTOR: u32 = 16;

/// ISO 9660's deepest directory level (the root is level 1).
const MAX_LEVEL: usize = 8;

/// The longest name `-l` allows.
const MAX_NAME: usize = 31;

/// A directory record without its name and system use area.
const DIR_RECORD_FIXED: usize = 33;

/// The `-T` table's name.
const TRANS_TBL: &str = "TRANS.TBL";

/// RRIP 1.09's extension identifier, description and source (the `ER` record).
const ER_ID: &str = "RRIP_1991A";
const ER_DES: &str =
    "THE ROCK RIDGE INTERCHANGE PROTOCOL PROVIDES SUPPORT FOR POSIX FILE SYSTEM SEMANTICS";
const ER_SRC: &str = "PLEASE CONTACT DISC PUBLISHER FOR SPECIFICATION SOURCE.  SEE PUBLISHER \
                      IDENTIFIER IN PRIMARY VOLUME DESCRIPTOR FOR CONTACT INFORMATION.";

/// `RR` flags: which Rock Ridge records follow.
const RR_PX: u8 = 0x01;
const RR_NM: u8 = 0x08;
const RR_TF: u8 = 0x80;

/// `TF` flags: modification, access and attribute change times, 7-byte form.
const TF_FLAGS: u8 = 0x0e;

/// Directory record flags.
const FLAG_DIRECTORY: u8 = 0x02;

/// El Torito's platforms.
const PLATFORM_X86: u8 = 0x00;
const PLATFORM_EFI: u8 = 0xef;

/// The image's descriptors and boot images; paths are relative to the tree's root.
pub(crate) struct Options<'a> {
    /// `-A`: the application identifier.
    pub application: &'a str,
    /// `-P`: the publisher identifier.
    pub publisher: &'a str,
    /// `-p`: the data preparer identifier.
    pub preparer: &'a str,
    /// `-V`: the volume identifier.
    pub volume: &'a str,
    /// `-b`: the BIOS no-emulation boot image.
    pub bios_boot: Option<&'a str>,
    /// `-c`: where the boot catalogue goes (with `-b` or `-e`).
    pub catalog: Option<&'a str>,
    /// `-e`: the EFI boot image (a FAT file system).
    pub efi_boot: Option<&'a str>,
    /// Every timestamp of the image, in seconds since the epoch (UTC).
    pub time: i64,
}

/// Where a file's bytes come from.
enum Data {
    /// A file of the tree.
    Host(PathBuf),
    /// Made here: `TRANS.TBL`.
    Bytes(Vec<u8>),
    /// The boot catalogue, written once every extent is known.
    Catalog,
}

/// A directory's entries, or a file's data.
enum Kind {
    Dir(Vec<usize>),
    File(Data),
}

/// One entry of the tree.
struct Node {
    /// The real name (Rock Ridge's `NM`, `TRANS.TBL`'s right column).
    name: String,
    /// The ISO 9660 name.
    iso: String,
    parent: usize,
    kind: Kind,
    /// `st_mode` for Rock Ridge's `PX`.
    mode: u32,
    size: u64,
    extent: u32,
    /// A directory's number in the path table (1 for the root).
    number: u16,
}

/// The tree being written, the root at index 0.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// Reads `dir` (symbolic links refused: the release trees have none).
    fn read(dir: &Path) -> Result<Tree> {
        let mut t = Tree {
            nodes: vec![Node {
                name: String::new(),
                iso: String::new(),
                parent: 0,
                kind: Kind::Dir(Vec::new()),
                mode: 0o040755,
                size: 0,
                extent: 0,
                number: 1,
            }],
        };
        t.read_dir(0, dir, 1)?;
        Ok(t)
    }

    fn read_dir(&mut self, at: usize, dir: &Path, level: usize) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .collect::<io::Result<_>>()
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let meta =
                fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = e
                .file_name()
                .into_string()
                .map_err(|n| format!("{}: not UTF-8: {n:?}", dir.display()))?;
            let mode = meta.permissions().mode();
            if meta.is_dir() {
                if level + 1 > MAX_LEVEL {
                    return Err(
                        format!("{}: deeper than {MAX_LEVEL} levels", path.display()).into(),
                    );
                }
                let k = self.add(at, &name, mode, Kind::Dir(Vec::new()), 0);
                self.read_dir(k, &path, level + 1)?;
            } else if meta.is_file() {
                self.add(at, &name, mode, Kind::File(Data::Host(path)), meta.len());
            } else {
                return Err(format!("{}: neither a file nor a directory", path.display()).into());
            }
        }
        Ok(())
    }

    fn add(&mut self, parent: usize, name: &str, mode: u32, kind: Kind, size: u64) -> usize {
        let k = self.nodes.len();
        let dir = matches!(kind, Kind::Dir(_));
        self.nodes.push(Node {
            name: name.to_string(),
            iso: iso_name(name, dir),
            parent,
            kind,
            mode,
            size,
            extent: 0,
            number: 0,
        });
        if let Kind::Dir(children) = &mut self.nodes[parent].kind {
            children.push(k);
        }
        k
    }

    fn children(&self, k: usize) -> &[usize] {
        match &self.nodes[k].kind {
            Kind::Dir(c) => c,
            Kind::File(_) => &[],
        }
    }

    fn is_dir(&self, k: usize) -> bool {
        matches!(self.nodes[k].kind, Kind::Dir(_))
    }

    /// The node at `path` (relative, `/`-separated).
    fn lookup(&self, path: &str) -> Option<usize> {
        let mut at = 0;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            at = *self
                .children(at)
                .iter()
                .find(|&&c| self.nodes[c].name == part)?;
        }
        Some(at)
    }

    /// Sorts every directory by ISO name and refuses two entries with the same one.
    fn sort(&mut self) -> Result<()> {
        for k in 0..self.nodes.len() {
            let mut c = self.children(k).to_vec();
            c.sort_by(|&a, &b| self.nodes[a].iso.cmp(&self.nodes[b].iso));
            for w in c.windows(2) {
                if self.nodes[w[0]].iso == self.nodes[w[1]].iso {
                    return Err(format!(
                        "ISO 9660 names collide: {} and {} are both {}",
                        self.nodes[w[0]].name, self.nodes[w[1]].name, self.nodes[w[0]].iso
                    )
                    .into());
                }
            }
            if let Kind::Dir(children) = &mut self.nodes[k].kind {
                *children = c;
            }
        }
        Ok(())
    }

    /// The directories in path table order: by level, then by parent's number, then by name.
    fn path_order(&mut self) -> Vec<usize> {
        let mut order = vec![0];
        let mut i = 0;
        while i < order.len() {
            let k = order[i];
            let subdirs: Vec<usize> = self
                .children(k)
                .iter()
                .copied()
                .filter(|&c| self.is_dir(c))
                .collect();
            order.extend(subdirs);
            i += 1;
        }
        for (n, &k) in order.iter().enumerate() {
            self.nodes[k].number = (n + 1) as u16;
        }
        order
    }
}

/// The ISO 9660 name of `name` (module docs: `-l -L -d -N`).
fn iso_name(name: &str, dir: bool) -> String {
    let upper: Vec<char> = name.chars().map(|c| c.to_ascii_uppercase()).collect();
    let last_dot = if dir {
        None
    } else {
        upper.iter().rposition(|&c| c == '.')
    };
    let mut s: String = upper
        .iter()
        .enumerate()
        .map(|(i, &c)| match c {
            'A'..='Z' | '0'..='9' | '_' => c,
            '.' if dir || Some(i) == last_dot => '.',
            _ => '_',
        })
        .collect();
    s.truncate(MAX_NAME);
    s
}

/// ISO 9660's both-byte-order 16-bit field (7.2.3).
fn both16(v: u16) -> [u8; 4] {
    let (l, b) = (v.to_le_bytes(), v.to_be_bytes());
    [l[0], l[1], b[0], b[1]]
}

/// ISO 9660's both-byte-order 32-bit field (7.3.3).
fn both32(v: u32) -> [u8; 8] {
    let (l, b) = (v.to_le_bytes(), v.to_be_bytes());
    [l[0], l[1], l[2], l[3], b[0], b[1], b[2], b[3]]
}

/// Civil date of a day count since 1970-01-01 (proleptic Gregorian).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// (year, month, day, hour, minute, second) of `t` in UTC.
fn ymdhms(t: i64) -> (i64, u32, u32, u32, u32, u32) {
    let (y, m, d) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400) as u32;
    (y, m, d, s / 3600, s / 60 % 60, s % 60)
}

/// A directory record's 7-byte time (9.1.5), UTC.
fn time7(t: i64) -> [u8; 7] {
    let (y, m, d, h, mi, s) = ymdhms(t);
    [
        (y - 1900) as u8,
        m as u8,
        d as u8,
        h as u8,
        mi as u8,
        s as u8,
        0,
    ]
}

/// A volume descriptor's 17-byte time (8.4.26.1), UTC.
fn time17(t: i64) -> [u8; 17] {
    let (y, m, d, h, mi, s) = ymdhms(t);
    let mut out = [0u8; 17];
    out[..16].copy_from_slice(format!("{y:04}{m:02}{d:02}{h:02}{mi:02}{s:02}00").as_bytes());
    out
}

/// A volume descriptor's absent time: sixteen `0` digits and a zero offset.
fn time17_unset() -> [u8; 17] {
    let mut out = [b'0'; 17];
    out[16] = 0;
    out
}

/// A System Use entry: signature, length, version 1, then `body`.
fn susp(sig: &[u8; 2], body: &[u8]) -> Vec<u8> {
    let mut v = vec![sig[0], sig[1], (4 + body.len()) as u8, 1];
    v.extend_from_slice(body);
    v
}

/// Rock Ridge's `PX`: mode, links, owner root and group wheel.
fn rr_px(mode: u32, nlink: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(32);
    b.extend_from_slice(&both32(mode));
    b.extend_from_slice(&both32(nlink));
    b.extend_from_slice(&both32(0));
    b.extend_from_slice(&both32(0));
    susp(b"PX", &b)
}

/// Rock Ridge's `TF`: the three times of [`TF_FLAGS`], all `t`.
fn rr_tf(t: i64) -> Vec<u8> {
    let mut b = vec![TF_FLAGS];
    for _ in 0..3 {
        b.extend_from_slice(&time7(t));
    }
    susp(b"TF", &b)
}

/// The `ER` record of RRIP 1.09.
fn rr_er() -> Vec<u8> {
    let mut b = vec![
        ER_ID.len() as u8,
        ER_DES.len() as u8,
        ER_SRC.len() as u8,
        1, // extension version
    ];
    b.extend_from_slice(ER_ID.as_bytes());
    b.extend_from_slice(ER_DES.as_bytes());
    b.extend_from_slice(ER_SRC.as_bytes());
    susp(b"ER", &b)
}

/// A directory record of `name` (`[0]` for `.`, `[1]` for `..`) with system use `su`.
fn dir_record(
    name: &[u8],
    extent: u32,
    size: u64,
    flags: u8,
    t: i64,
    su: &[u8],
) -> Result<Vec<u8>> {
    let pad = usize::from(name.len().is_multiple_of(2));
    let len = DIR_RECORD_FIXED + name.len() + pad + su.len();
    if len > 255 {
        return Err(format!(
            "directory record of {} is {len} bytes (a record holds 255)",
            String::from_utf8_lossy(name)
        )
        .into());
    }
    let size = u32::try_from(size).map_err(|_| "a file of 4 GiB or more")?;
    let mut r = Vec::with_capacity(len);
    r.push(len as u8);
    r.push(0); // extended attribute record length
    r.extend_from_slice(&both32(extent));
    r.extend_from_slice(&both32(size));
    r.extend_from_slice(&time7(t));
    r.push(flags);
    r.push(0); // file unit size
    r.push(0); // interleave gap
    r.extend_from_slice(&both16(1)); // volume sequence number
    r.push(name.len() as u8);
    r.extend_from_slice(name);
    if pad == 1 {
        r.push(0);
    }
    r.extend_from_slice(su);
    Ok(r)
}

/// `d` padded with spaces to `n` bytes (a volume descriptor's string field).
fn field(d: &str, n: usize) -> Result<Vec<u8>> {
    if d.len() > n {
        return Err(format!("{d:?}: longer than the {n} bytes of its field").into());
    }
    let mut v = d.as_bytes().to_vec();
    v.resize(n, b' ');
    Ok(v)
}

/// The image being laid out: the tree and every extent.
struct Image<'a> {
    tree: Tree,
    opts: &'a Options<'a>,
    /// The directories in path table order.
    dirs: Vec<usize>,
    /// The `CE` continuation area holding `ER`.
    ce_extent: u32,
    catalog: Option<usize>,
    bios_boot: Option<usize>,
    efi_boot: Option<usize>,
    path_table_size: u32,
    path_l: u32,
    path_m: u32,
    total: u32,
}

impl Image<'_> {
    /// The system use area of `k`'s own record (`dot` for its `.`/`..` records: no `NM`).
    fn su(&self, k: usize, dot: bool, root_dot: bool) -> Vec<u8> {
        let n = &self.tree.nodes[k];
        let nlink = if self.tree.is_dir(k) {
            2 + self
                .tree
                .children(k)
                .iter()
                .filter(|&&c| self.tree.is_dir(c))
                .count() as u32
        } else {
            1
        };
        let mut su = Vec::new();
        if root_dot {
            su.extend_from_slice(&susp(b"SP", &[0xbe, 0xef, 0]));
        }
        let flags = RR_PX | RR_TF | if dot { 0 } else { RR_NM };
        su.extend_from_slice(&susp(b"RR", &[flags]));
        su.extend_from_slice(&rr_px(n.mode, nlink));
        su.extend_from_slice(&rr_tf(self.opts.time));
        if !dot {
            let mut nm = vec![0u8];
            nm.extend_from_slice(n.name.as_bytes());
            su.extend_from_slice(&susp(b"NM", &nm));
        }
        if root_dot {
            let er_len = rr_er().len() as u32;
            let mut ce = Vec::with_capacity(24);
            ce.extend_from_slice(&both32(self.ce_extent));
            ce.extend_from_slice(&both32(0));
            ce.extend_from_slice(&both32(er_len));
            su.extend_from_slice(&susp(b"CE", &ce));
        }
        su
    }

    /// The records of directory `k`, each sector's worth padded so none crosses a sector.
    fn dir_bytes(&self, k: usize) -> Result<Vec<u8>> {
        let t = self.opts.time;
        let n = &self.tree.nodes[k];
        let parent = &self.tree.nodes[n.parent];
        let mut records = vec![
            dir_record(
                &[0],
                n.extent,
                n.size,
                FLAG_DIRECTORY,
                t,
                &self.su(k, true, k == 0),
            )?,
            dir_record(
                &[1],
                parent.extent,
                parent.size,
                FLAG_DIRECTORY,
                t,
                &self.su(n.parent, true, false),
            )?,
        ];
        for &c in self.tree.children(k) {
            let cn = &self.tree.nodes[c];
            let flags = if self.tree.is_dir(c) {
                FLAG_DIRECTORY
            } else {
                0
            };
            records.push(dir_record(
                cn.iso.as_bytes(),
                cn.extent,
                cn.size,
                flags,
                t,
                &self.su(c, false, false),
            )?);
        }
        let mut out = Vec::new();
        for r in records {
            let used = out.len() % SECTOR;
            if used + r.len() > SECTOR {
                out.resize(out.len() + SECTOR - used, 0);
            }
            out.extend_from_slice(&r);
        }
        out.resize(out.len().div_ceil(SECTOR).max(1) * SECTOR, 0);
        Ok(out)
    }

    /// The path table, little-endian (`L`) or big-endian (`M`).
    fn path_table(&self, big: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for &k in &self.dirs {
            let n = &self.tree.nodes[k];
            let name: &[u8] = if k == 0 { &[0] } else { n.iso.as_bytes() };
            let parent = self.tree.nodes[n.parent].number;
            out.push(name.len() as u8);
            out.push(0);
            if big {
                out.extend_from_slice(&n.extent.to_be_bytes());
                out.extend_from_slice(&parent.to_be_bytes());
            } else {
                out.extend_from_slice(&n.extent.to_le_bytes());
                out.extend_from_slice(&parent.to_le_bytes());
            }
            out.extend_from_slice(name);
            if name.len() % 2 == 1 {
                out.push(0);
            }
        }
        out
    }

    /// The primary volume descriptor.
    fn pvd(&self) -> Result<Vec<u8>> {
        let o = self.opts;
        let mut d = vec![1u8];
        d.extend_from_slice(b"CD001");
        d.push(1);
        d.push(0);
        d.extend_from_slice(&field("", 32)?); // system identifier
        d.extend_from_slice(&field(o.volume, 32)?);
        d.extend_from_slice(&[0; 8]);
        d.extend_from_slice(&both32(self.total));
        d.extend_from_slice(&[0; 32]);
        d.extend_from_slice(&both16(1)); // volume set size
        d.extend_from_slice(&both16(1)); // volume sequence number
        d.extend_from_slice(&both16(SECTOR as u16));
        d.extend_from_slice(&both32(self.path_table_size));
        d.extend_from_slice(&self.path_l.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&self.path_m.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        let root = &self.tree.nodes[0];
        // The root's record here carries no system use area (34 bytes).
        d.extend_from_slice(&dir_record(
            &[0],
            root.extent,
            root.size,
            FLAG_DIRECTORY,
            o.time,
            &[],
        )?);
        d.extend_from_slice(&field("", 128)?); // volume set identifier
        d.extend_from_slice(&field(o.publisher, 128)?);
        d.extend_from_slice(&field(o.preparer, 128)?);
        d.extend_from_slice(&field(o.application, 128)?);
        d.extend_from_slice(&field("", 37)?); // copyright file
        d.extend_from_slice(&field("", 37)?); // abstract file
        d.extend_from_slice(&field("", 37)?); // bibliographic file
        d.extend_from_slice(&time17(o.time)); // creation
        d.extend_from_slice(&time17(o.time)); // modification
        d.extend_from_slice(&time17_unset()); // expiration
        d.extend_from_slice(&time17(o.time)); // effective
        d.push(1); // file structure version
        d.resize(SECTOR, 0);
        Ok(d)
    }

    /// El Torito's boot record volume descriptor.
    fn boot_record(&self, catalog: u32) -> Vec<u8> {
        let mut d = vec![0u8];
        d.extend_from_slice(b"CD001");
        d.push(1);
        let mut id = b"EL TORITO SPECIFICATION".to_vec();
        id.resize(32, 0);
        d.extend_from_slice(&id);
        d.resize(71, 0);
        d.extend_from_slice(&catalog.to_le_bytes());
        d.resize(SECTOR, 0);
        d
    }

    /// The boot catalogue (module docs).
    fn catalog_bytes(&self) -> Result<Vec<u8>> {
        let entry = |k: usize| -> Result<[u8; 32]> {
            let n = &self.tree.nodes[k];
            let count = u16::try_from(n.size.div_ceil(512))
                .map_err(|_| format!("boot image {}: more than 65535 virtual sectors", n.name))?;
            let mut e = [0u8; 32];
            e[0] = 0x88; // bootable
            e[1] = 0; // no emulation
            // load segment 0: the default 0x7c0
            e[6..8].copy_from_slice(&count.to_le_bytes());
            e[8..12].copy_from_slice(&n.extent.to_le_bytes());
            Ok(e)
        };
        let validation = |platform: u8| {
            let mut v = [0u8; 32];
            v[0] = 1;
            v[1] = platform;
            v[30] = 0x55;
            v[31] = 0xaa;
            let sum = v.chunks(2).fold(0u16, |s, w| {
                s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
            });
            v[28..30].copy_from_slice(&0u16.wrapping_sub(sum).to_le_bytes());
            v
        };
        let mut c = Vec::with_capacity(SECTOR);
        match (self.bios_boot, self.efi_boot) {
            (Some(b), efi) => {
                c.extend_from_slice(&validation(PLATFORM_X86));
                c.extend_from_slice(&entry(b)?);
                if let Some(e) = efi {
                    let mut head = [0u8; 32];
                    head[0] = 0x91; // final section header
                    head[1] = PLATFORM_EFI;
                    head[2..4].copy_from_slice(&1u16.to_le_bytes());
                    c.extend_from_slice(&head);
                    c.extend_from_slice(&entry(e)?);
                }
            }
            (None, Some(e)) => {
                c.extend_from_slice(&validation(PLATFORM_EFI));
                c.extend_from_slice(&entry(e)?);
            }
            (None, None) => return Err("a boot catalogue with no boot image".into()),
        }
        c.resize(SECTOR, 0);
        Ok(c)
    }
}

/// `TRANS.TBL` of directory `k`: a line per entry (module docs).
fn trans_tbl(tree: &Tree, k: usize) -> Vec<u8> {
    let mut s = String::new();
    for &c in tree.children(k) {
        let n = &tree.nodes[c];
        let kind = if tree.is_dir(c) { 'D' } else { 'F' };
        s.push_str(&format!("{kind} {:<36}{}\n", n.iso, n.name));
    }
    s.into_bytes()
}

/// Writes the image of the tree `src` to `out` (module docs).
pub(crate) fn mkhybrid(src: &Path, out: &Path, opts: &Options<'_>) -> Result<()> {
    let mut tree = Tree::read(src)?;
    let boot_path = |p: Option<&str>, what: &str, tree: &Tree| -> Result<Option<usize>> {
        match p {
            None => Ok(None),
            Some(p) => match tree.lookup(p) {
                Some(k) if !tree.is_dir(k) => Ok(Some(k)),
                _ => Err(format!("{what} {p}: no such file in {}", src.display()).into()),
            },
        }
    };
    let bios_boot = boot_path(opts.bios_boot, "-b", &tree)?;
    let efi_boot = boot_path(opts.efi_boot, "-e", &tree)?;
    let catalog = match (opts.catalog, bios_boot.is_some() || efi_boot.is_some()) {
        (Some(c), true) => {
            let (dir, name) = c.rsplit_once('/').unwrap_or(("", c));
            let at = tree
                .lookup(dir)
                .filter(|&k| tree.is_dir(k))
                .ok_or_else(|| format!("-c {c}: no directory {dir} in {}", src.display()))?;
            if tree.lookup(c).is_some() {
                return Err(format!("-c {c}: already a file of the tree").into());
            }
            Some(tree.add(at, name, 0o100444, Kind::File(Data::Catalog), SECTOR as u64))
        }
        (None, true) => return Err("-b or -e without -c".into()),
        (_, false) => None,
    };
    // `-T`: every directory gets its table, listing the entries made so far.
    let dirs: Vec<usize> = (0..tree.nodes.len()).filter(|&k| tree.is_dir(k)).collect();
    tree.sort()?;
    for &k in &dirs {
        if tree
            .children(k)
            .iter()
            .any(|&c| tree.nodes[c].name == TRANS_TBL)
        {
            return Err(format!("{TRANS_TBL}: already in the tree").into());
        }
        let table = trans_tbl(&tree, k);
        let len = table.len() as u64;
        tree.add(k, TRANS_TBL, 0o100444, Kind::File(Data::Bytes(table)), len);
    }
    tree.sort()?;
    let order = tree.path_order();

    let mut img = Image {
        tree,
        opts,
        dirs: order,
        ce_extent: 0,
        catalog,
        bios_boot,
        efi_boot,
        path_table_size: 0,
        path_l: 0,
        path_m: 0,
        total: 0,
    };

    // Layout: descriptors, the two path tables, the directories, the continuation area, the
    // catalogue, the files.
    let mut next = PVD_SECTOR + 1;
    if img.catalog.is_some() {
        next += 1; // the boot record
    }
    next += 1; // the terminator
    img.path_table_size = img.path_table(false).len() as u32;
    let pt_sectors = (img.path_table_size as usize).div_ceil(SECTOR) as u32;
    img.path_l = next;
    next += pt_sectors;
    img.path_m = next;
    next += pt_sectors;
    // A directory's size does not depend on extents: lay them out with zeros first.
    for i in 0..img.dirs.len() {
        let k = img.dirs[i];
        let size = img.dir_bytes(k)?.len();
        img.tree.nodes[k].size = size as u64;
        img.tree.nodes[k].extent = next;
        next += (size / SECTOR) as u32;
    }
    img.ce_extent = next;
    next += 1;
    if let Some(c) = img.catalog {
        img.tree.nodes[c].extent = next;
        next += 1;
    }
    let mut files = Vec::new();
    for &d in &img.dirs {
        for &c in img.tree.children(d) {
            if !img.tree.is_dir(c) && Some(c) != img.catalog {
                files.push(c);
            }
        }
    }
    for &f in &files {
        let n = &mut img.tree.nodes[f];
        n.extent = next;
        next += u32::try_from(n.size.div_ceil(SECTOR as u64))
            .map_err(|_| format!("{}: too large", n.name))?;
    }
    img.total = next;

    // Write it.
    let _ = fs::remove_file(out);
    let mut w = fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let at = |w: &mut fs::File, sector: u32, bytes: &[u8]| -> Result<()> {
        w.seek(SeekFrom::Start(u64::from(sector) * SECTOR as u64))?;
        w.write_all(bytes)?;
        Ok(())
    };
    at(&mut w, PVD_SECTOR, &img.pvd()?)?;
    let mut vd = PVD_SECTOR + 1;
    if let Some(c) = img.catalog {
        at(&mut w, vd, &img.boot_record(img.tree.nodes[c].extent))?;
        vd += 1;
    }
    let mut term = vec![255u8];
    term.extend_from_slice(b"CD001");
    term.push(1);
    term.resize(SECTOR, 0);
    at(&mut w, vd, &term)?;
    at(&mut w, img.path_l, &img.path_table(false))?;
    at(&mut w, img.path_m, &img.path_table(true))?;
    for &k in &img.dirs {
        at(&mut w, img.tree.nodes[k].extent, &img.dir_bytes(k)?)?;
    }
    at(&mut w, img.ce_extent, &rr_er())?;
    if let Some(c) = img.catalog {
        at(&mut w, img.tree.nodes[c].extent, &img.catalog_bytes()?)?;
    }
    for &f in &files {
        let n = &img.tree.nodes[f];
        w.seek(SeekFrom::Start(u64::from(n.extent) * SECTOR as u64))?;
        match &n.kind {
            Kind::File(Data::Host(p)) => {
                let r = fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
                let copied = io::copy(&mut r.take(n.size), &mut w)?;
                if copied != n.size {
                    return Err(format!("{}: changed while the image was made", p.display()).into());
                }
            }
            Kind::File(Data::Bytes(b)) => w.write_all(b)?,
            Kind::File(Data::Catalog) | Kind::Dir(_) => {}
        }
    }
    w.set_len(u64::from(img.total) * SECTOR as u64)?;
    Ok(())
}

/// The files of the image at `iso` whose ISO 9660 names the boot loaders' lookup finds: for
/// each `path`, its extent and size, found as `sys/lib/libsa/cd9660.c` finds a file (the
/// type M path table for the directories, upper-cased names). For checks and tests.
pub(crate) fn libsa_lookup(iso: &[u8], path: &str) -> Option<(u32, u32)> {
    let sector = |n: u32| iso.get(n as usize * SECTOR..(n as usize + 1) * SECTOR);
    let mut s = PVD_SECTOR;
    let pvd = loop {
        let d = sector(s)?;
        if &d[1..6] != b"CD001" || d[0] == 255 {
            return None;
        }
        if d[0] == 1 {
            break d;
        }
        s += 1;
    };
    let pt_size = u32::from_le_bytes(pvd[132..136].try_into().ok()?) as usize;
    let pt_m = u32::from_be_bytes(pvd[148..152].try_into().ok()?) as usize;
    let pt = iso.get(pt_m * SECTOR..pt_m * SECTOR + pt_size)?;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let (dirs, file) = parts.split_at(parts.len().checked_sub(1)?);
    // Walk the path table: entry numbers from 1, the parent of each.
    let mut entries = Vec::new();
    let mut p = 0;
    while p + 8 <= pt.len() {
        let len = usize::from(pt[p]);
        let extent = u32::from_be_bytes(pt[p + 2..p + 6].try_into().ok()?);
        let parent = u16::from_be_bytes(pt[p + 6..p + 8].try_into().ok()?);
        let name = pt.get(p + 8..p + 8 + len)?;
        entries.push((name, extent, parent));
        p += (8 + len).div_ceil(2) * 2;
    }
    let mut cur = 1u16;
    let mut extent = entries.first()?.1;
    for d in dirs {
        let want = d.to_ascii_uppercase();
        let (i, e) = entries
            .iter()
            .enumerate()
            .find(|(_, e)| e.2 == cur && e.0 == want.as_bytes())?;
        cur = (i + 1) as u16;
        extent = e.1;
    }
    // Scan the directory's records for the file.
    let want = file.first()?.to_ascii_uppercase();
    let dir = sector(extent)?;
    let dsize = u32::from_le_bytes(dir[10..14].try_into().ok()?) as usize;
    let mut off = 0;
    while off < dsize {
        let blk = iso.get(extent as usize * SECTOR + off..)?;
        let len = usize::from(blk[0]);
        if len == 0 {
            off = (off / SECTOR + 1) * SECTOR;
            continue;
        }
        let nlen = usize::from(blk[32]);
        let name = blk.get(33..33 + nlen)?;
        if blk[25] & 6 == 0 && name == want.as_bytes() {
            let ext = u32::from_le_bytes(blk[2..6].try_into().ok()?);
            let size = u32::from_le_bytes(blk[10..14].try_into().ok()?);
            return Some((ext, size));
        }
        off += len;
    }
    None
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emibsd-iso9660-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A tree as `cd80.iso`'s: `8.0/amd64/{bsd.rd,eficdboot}`, `etc/boot.conf`.
    fn cd_tree(dir: &Path) {
        fs::create_dir_all(dir.join("8.0/amd64")).unwrap();
        fs::create_dir_all(dir.join("etc")).unwrap();
        fs::write(dir.join("etc/boot.conf"), "set image /8.0/amd64/bsd.rd\n").unwrap();
        fs::write(dir.join("8.0/amd64/bsd.rd"), vec![0x42u8; 5000]).unwrap();
        fs::write(dir.join("8.0/amd64/eficdboot"), vec![0xefu8; 358_400]).unwrap();
    }

    fn opts() -> Options<'static> {
        Options {
            application: "app",
            publisher: "pub",
            preparer: "prep",
            volume: "VOL",
            bios_boot: None,
            catalog: Some("8.0/amd64/boot.catalog"),
            efi_boot: Some("8.0/amd64/eficdboot"),
            time: 1_790_985_600,
        }
    }

    #[test]
    fn names() {
        assert_eq!(iso_name("bsd.rd", false), "BSD.RD");
        assert_eq!(iso_name("8.0", true), "8.0");
        assert_eq!(iso_name("a.b.c", false), "A_B.C");
        assert_eq!(iso_name("boot-conf", false), "BOOT_CONF");
        assert_eq!(iso_name(".profile", false), ".PROFILE");
        assert_eq!(iso_name(&"x".repeat(40), false).len(), MAX_NAME);
    }

    #[test]
    fn times() {
        // 2026-10-03 00:00:00 UTC.
        assert_eq!(time7(1_790_985_600), [126, 10, 3, 0, 0, 0, 0]);
        assert_eq!(&time17(1_790_985_600)[..16], b"2026100300000000");
        assert_eq!(ymdhms(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(ymdhms(951_782_400), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn image_reads_back() {
        let dir = scratch("tree");
        cd_tree(&dir);
        let out = dir.with_extension("iso");
        mkhybrid(&dir, &out, &opts()).unwrap();
        let iso = fs::read(&out).unwrap();
        assert_eq!(iso.len() % SECTOR, 0);
        // libsa's lookup finds the kernel and boot.conf by their upper-case names.
        let (ext, size) = libsa_lookup(&iso, "/8.0/amd64/bsd.rd").unwrap();
        assert_eq!(size, 5000);
        assert_eq!(iso[ext as usize * SECTOR], 0x42);
        let (ext, size) = libsa_lookup(&iso, "/etc/boot.conf").unwrap();
        let at = ext as usize * SECTOR;
        assert_eq!(
            &iso[at..at + size as usize],
            b"set image /8.0/amd64/bsd.rd\n"
        );
        assert!(libsa_lookup(&iso, "/8.0/amd64/TRANS.TBL").is_some());
        assert!(libsa_lookup(&iso, "/8.0/amd64/nothere").is_none());
        // The boot record names the catalogue, whose validation entry sums to zero and
        // whose default entry is the EFI image.
        let br = &iso[17 * SECTOR..18 * SECTOR];
        assert_eq!(br[0], 0);
        assert_eq!(&br[7..30], b"EL TORITO SPECIFICATION");
        let cat = u32::from_le_bytes(br[71..75].try_into().unwrap()) as usize;
        let c = &iso[cat * SECTOR..cat * SECTOR + 64];
        assert_eq!(c[1], PLATFORM_EFI);
        let sum = c[..32].chunks(2).fold(0u16, |s, w| {
            s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        });
        assert_eq!(sum, 0);
        assert_eq!(c[32], 0x88);
        assert_eq!(u16::from_le_bytes([c[38], c[39]]), 700);
        let (efi, _) = libsa_lookup(&iso, "/8.0/amd64/eficdboot").unwrap();
        assert_eq!(u32::from_le_bytes(c[40..44].try_into().unwrap()), efi);
        // The catalogue is a file of its directory too.
        assert_eq!(
            libsa_lookup(&iso, "/8.0/amd64/boot.catalog").unwrap().0 as usize,
            cat
        );
        // Rock Ridge: SP first in the root's `.`, then a CE to the ER.
        let pvd = &iso[16 * SECTOR..17 * SECTOR];
        let root = u32::from_le_bytes(pvd[158..162].try_into().unwrap()) as usize;
        let dot = &iso[root * SECTOR..];
        assert_eq!(&dot[34..40], b"SP\x07\x01\xbe\xef");
        let len = usize::from(dot[0]);
        let ce = dot[34..len].windows(2).position(|w| w == b"CE").unwrap() + 34;
        let blk = u32::from_le_bytes(dot[ce + 4..ce + 8].try_into().unwrap()) as usize;
        assert_eq!(&iso[blk * SECTOR..blk * SECTOR + 2], b"ER");
        assert_eq!(&iso[blk * SECTOR + 8..blk * SECTOR + 18], ER_ID.as_bytes());
        // The real names are there for Rock Ridge readers.
        let text = String::from_utf8_lossy(&iso);
        assert!(text.contains("NM\x0b\x01\x00bsd.rd"));
        assert!(text.contains("F BSD.RD"));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn bios_and_efi_sections() {
        let dir = scratch("both");
        cd_tree(&dir);
        fs::write(dir.join("8.0/amd64/cdbr"), vec![0xf4u8; 2048]).unwrap();
        let out = dir.with_extension("iso");
        let o = Options {
            bios_boot: Some("8.0/amd64/cdbr"),
            ..opts()
        };
        mkhybrid(&dir, &out, &o).unwrap();
        let iso = fs::read(&out).unwrap();
        let cat = u32::from_le_bytes(iso[17 * SECTOR + 71..17 * SECTOR + 75].try_into().unwrap())
            as usize;
        let c = &iso[cat * SECTOR..cat * SECTOR + 128];
        assert_eq!(c[1], PLATFORM_X86);
        assert_eq!(c[32], 0x88);
        assert_eq!(u16::from_le_bytes([c[38], c[39]]), 4);
        assert_eq!((c[64], c[65]), (0x91, PLATFORM_EFI));
        assert_eq!(c[96], 0x88);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn refuses_collisions_and_missing_boot_images() {
        let dir = scratch("bad");
        fs::write(dir.join("a-b"), "1").unwrap();
        fs::write(dir.join("a_b"), "2").unwrap();
        let out = dir.with_extension("iso");
        let o = Options {
            catalog: None,
            efi_boot: None,
            ..opts()
        };
        assert!(mkhybrid(&dir, &out, &o).is_err());
        fs::remove_file(dir.join("a_b")).unwrap();
        assert!(mkhybrid(&dir, &out, &o).is_ok());
        assert!(mkhybrid(&dir, &out, &opts()).is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&out);
    }
}
/* </TESTS> */
