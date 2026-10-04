//! Host tests of `msdosfs_lookup.rs` over a FAT image mounted on the fake disk of
//! `msdosfs_vfsops.rs`'s tests: the directory search (short names, Win95 long names, volume
//! labels, the room found for new entries), `readep`/`readde`, `uniqdosname`, `createde`
//! and `removede` (within a block, across a block boundary, and growing a full directory),
//! `dosdirempty`, and the paths of `msdosfs_lookup` that need no new denode.
//!
//! Getting a denode (`deget`) locks its new vnode, which needs the vnode operations of
//! `msdosfs_vnops.c`; the directories here are denodes made by hand, as `deget` makes them.

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_subr::{getnewvnode, vrele};
use crate::msdosfs::direntry::ATTR_ARCHIVE;
use crate::msdosfs::fat::CLUST_END;
use crate::msdosfs::msdosfs_fat::fc_purge;
use crate::msdosfs::msdosfs_vfsops::msdosfs_sync;
use crate::msdosfs::msdosfs_vfsops::tests::mkfat::{self, Dir, Image};
use crate::msdosfs::msdosfs_vfsops::tests::{DISK, mount, setup, teardown, unmount};
use crate::msdosfs::msdosfsmount::vfstomsdosfs;
use crate::sys::mount::{MNT_WAIT, Mount};
use crate::sys::namei::LOOKUP;
use crate::sys::vnode::{VDIR, VT_MSDOSFS, Vnode, VopInactiveArgs, VopReclaimArgs, Vops};

/// The name of a 2-part long name entry file in the sample image.
const LONG2: &[u8] = b"a rather long name.txt";

/// The DOS name `unix2dosfn` makes of `name` with generation number `gen`.
fn dosname(name: &[u8], r#gen: u32) -> [u8; 11] {
    let mut dn = [0u8; 11];
    assert_ne!(unix2dosfn(name, &mut dn, r#gen), 0);
    dn
}

/// A short entry of a file with no data.
fn empty_file(short: &[u8; 11]) -> [u8; 32] {
    mkfat::short_entry(short, ATTR_ARCHIVE, 0, 0)
}

/// The sample root directory, FAT12, 512-byte blocks (16 entries each):
///
/// | offset | entry |
/// |---|---|
/// | 0 | volume label `EMIBSD` |
/// | 32 | `README.TXT` (short name only) |
/// | 64, 96 | `m10c-fat.txt`: one long name entry, short `M10C-FATTXT` |
/// | 128 | directory `SUBDIR` |
/// | 160, 192, 224 | `LONG2`: two long name entries, short `ARATHE~1TXT` |
/// | 256, 288, 320 | deleted |
/// | 352 | `AFTER.TXT` |
/// | 384... | never used |
fn sample() -> (Vec<u8>, u32) {
    let mut img = Image::new(mkfat::FAT12_1M);
    let mut label = mkfat::short_entry(b"EMIBSD     ", ATTR_VOLUME, 0, 0);
    label[24..26].fill(0);
    img.put(Dir::Root, &[label]);
    assert_eq!(
        img.add_file(Dir::Root, None, b"README  TXT", b"read me\n"),
        32
    );
    assert_eq!(
        img.add_file(Dir::Root, Some(b"m10c-fat.txt"), b"M10C-FATTXT", b"fat\n"),
        96
    );
    let sub = img.mkdir(Dir::Root, b"SUBDIR     ", 1);
    assert_eq!(
        img.add_file(Dir::Root, Some(LONG2), &dosname(LONG2, 1), b"long\n"),
        224
    );
    let mut deleted = empty_file(b"GONE    TXT");
    deleted[0] = SLOT_DELETED;
    img.put(Dir::Root, &[deleted, deleted, deleted]);
    img.put(Dir::Root, &[empty_file(b"AFTER   TXT")]);
    (img.finish(), sub)
}

/// The denode of the directory starting at cluster `start` (the root directory for
/// `MSDOSFSROOT`), filled in as `deget` fills it, without a vnode.
fn dirnode(pmp: &'static Msdosfsmount, start: u32) -> &'static Denode {
    let dep: &'static Denode = Box::leak(Box::new(Denode::new()));
    dep.de_pmp.set(Some(pmp));
    dep.de_Attributes.set(ATTR_DIRECTORY);
    dep.de_StartCluster.set(start);
    dep.de_refcnt.set(1);
    fc_purge(dep, 0);
    if start == MSDOSFSROOT {
        dep.de_FileSize
            .set(pmp.pm_rootdirsize.get() * u32::from(pmp.pm_BytesPerSec()));
    } else {
        let mut size = 0;
        assert_eq!(
            pcbmap(dep, CLUST_END, None, Some(&mut size), None),
            Err(Errno::E2BIG)
        );
        dep.de_FileSize.set(de_cn2off(pmp, size));
    }
    dep
}

/// A component name for `name`.
fn cn(p: &Proc, name: &'static [u8], nameiop: u64, flags: u64) -> Componentname {
    let mut cnp = Componentname::new();
    cnp.cn_nameiop = nameiop;
    cnp.cn_flags = flags;
    cnp.cn_proc = p;
    cnp.cn_cred = p.p_ucred.get();
    cnp.cn_nameptr = name.as_ptr();
    cnp.cn_namelen = name.len() as i64;
    cnp
}

/// `msdosfs_lookup_search` for `name` in `dp`.
fn search(
    p: &Proc,
    dp: &Denode,
    name: &'static [u8],
    nameiop: u64,
    flags: u64,
) -> Result<Search, Errno> {
    let cnp = cn(p, name, nameiop, flags);
    msdosfs_lookup_search(
        dp,
        dp.de_StartCluster.get() == MSDOSFSROOT,
        &cnp,
        nameiop,
        flags,
    )
}

/// The `n`th 32-byte entry of the root directory on the disk.
fn root_entry(off: u32) -> [u8; 32] {
    let img = Image::new(mkfat::FAT12_1M);
    let o = img.root_off() + off as usize;
    DISK.lock().unwrap()[o..o + 32].try_into().unwrap()
}

/// The mount and its root directory's denode.
fn mounted(p: &'static Proc) -> (&'static Mount, &'static Msdosfsmount, &'static Denode) {
    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    (mp, pmp, dirnode(pmp, MSDOSFSROOT))
}

#[test]
fn search_finds_short_and_long_names() {
    let (disk, sub) = sample();
    let readme_clust = {
        let e = &disk[Image::new(mkfat::FAT12_1M).root_off() + 32..][..32];
        u32::from(u16::from_le_bytes([e[26], e[27]]))
    };
    let (_g, p) = setup(disk);
    let (mp, _pmp, root) = mounted(p);

    let file = |blkoff, scn| Search::Found {
        isadir: false,
        scn,
        cluster: MSDOSFSROOT,
        blkoff,
    };
    // A short name, in any case.
    assert_eq!(
        search(p, root, b"README.TXT", LOOKUP, ISLASTCN),
        Ok(file(32, readme_clust))
    );
    assert_eq!(root.de_fndoffset.get(), 32);
    assert_eq!(root.de_fndcnt.get(), 0);
    assert_eq!(
        search(p, root, b"readme.txt", LOOKUP, ISLASTCN),
        Ok(file(32, readme_clust))
    );
    // A long name, by its long name entry or its short name; a rename may reuse the long
    // name entries found with a good checksum.
    let Ok(Search::Found { blkoff, .. }) = search(p, root, b"m10c-fat.txt", RENAME, ISLASTCN)
    else {
        panic!("m10c-fat.txt not found");
    };
    assert_eq!(blkoff, 96);
    assert_eq!(root.de_fndcnt.get(), 1);
    let Ok(Search::Found { blkoff, .. }) = search(p, root, b"M10C-FAT.TXT", LOOKUP, 0) else {
        panic!("M10C-FAT.TXT not found");
    };
    assert_eq!(blkoff, 96);
    // Trailing dots and blanks do not count.
    let Ok(Search::Found { blkoff, .. }) = search(p, root, b"m10c-fat.txt. ", LOOKUP, 0) else {
        panic!("m10c-fat.txt. not found");
    };
    assert_eq!(blkoff, 96);
    // A long name whose short name is a generated one is found only by its long name.
    let Ok(Search::Found { blkoff, .. }) = search(p, root, LONG2, LOOKUP, 0) else {
        panic!("{LONG2:?} not found");
    };
    assert_eq!(blkoff, 224);
    assert_eq!(root.de_fndoffset.get(), 224);
    assert!(matches!(
        search(p, root, b"a rather long name.tx", LOOKUP, 0),
        Ok(Search::NotFound { .. })
    ));
    // A directory: its own "." entry is its denode.
    assert_eq!(
        search(p, root, b"subdir", LOOKUP, 0),
        Ok(Search::Found {
            isadir: true,
            scn: sub,
            cluster: sub,
            blkoff: 0
        })
    );
    // The volume label is not a file.
    assert!(matches!(
        search(p, root, b"EMIBSD", LOOKUP, 0),
        Ok(Search::NotFound { .. })
    ));
    assert!(matches!(
        search(p, root, b"nothere", LOOKUP, 0),
        Ok(Search::NotFound { .. })
    ));
    // "." and ".." of the root directory are faked.
    let root_found = Search::Found {
        isadir: true,
        scn: MSDOSFSROOT,
        cluster: MSDOSFSROOT,
        blkoff: MSDOSFSROOT_OFS,
    };
    assert_eq!(search(p, root, b".", LOOKUP, 0), Ok(root_found));
    assert_eq!(search(p, root, b"..", LOOKUP, ISDOTDOT), Ok(root_found));
    // In a subdirectory they are real entries; ".." of a child of the root is the root.
    let subdp = dirnode(_pmp, sub);
    assert_eq!(
        search(p, subdp, b"..", LOOKUP, ISDOTDOT),
        Ok(Search::Found {
            isadir: true,
            scn: MSDOSFSROOT,
            cluster: MSDOSFSROOT,
            blkoff: MSDOSFSROOT_OFS
        })
    );
    // A name made of dots and blanks has no DOS name.
    assert_eq!(search(p, root, b"...", LOOKUP, 0), Err(Errno::EINVAL));

    unmount(p, mp);
    teardown();
}

#[test]
fn search_finds_room_for_new_entries() {
    let (disk, _) = sample();
    let (_g, p) = setup(disk);
    let (mp, pmp, root) = mounted(p);
    let create = |name: &'static [u8]| search(p, root, name, CREATE, ISLASTCN);

    // One long name entry and the DOS entry: the last two deleted slots.
    assert_eq!(
        create(b"x.txt"),
        Ok(Search::NotFound {
            slotoffset: 288,
            wincnt: 2
        })
    );
    // Three slots: the whole run of deleted ones.
    assert_eq!(
        create(b"a much longer name.txt"),
        Ok(Search::NotFound {
            slotoffset: 320,
            wincnt: 3
        })
    );
    // Four slots do not fit there: the first never used slot and the three after it, the
    // DOS entry last.
    assert_eq!(
        create(b"a name that needs four slots.txt"),
        Ok(Search::NotFound {
            slotoffset: 384 + 3 * 32,
            wincnt: 4
        })
    );
    // A DOS name needs only one.
    assert_eq!(
        create(b"NEW.TXT"),
        Ok(Search::NotFound {
            slotoffset: 256,
            wincnt: 1
        })
    );
    // Not a creation: no slot is looked for.
    assert_eq!(
        search(p, root, b"x.txt", LOOKUP, ISLASTCN),
        Ok(Search::NotFound {
            slotoffset: 0,
            wincnt: 2
        })
    );
    // Mounted with short names only, long names take one slot.
    pmp.pm_flags
        .set(pmp.pm_flags.get() | MSDOSFSMNT_SHORTNAME as u32);
    assert_eq!(
        create(b"a much longer name.txt"),
        Ok(Search::NotFound {
            slotoffset: 256,
            wincnt: 1
        })
    );
    // ... and the long name entries are not read: a long name is not found.
    assert!(matches!(
        search(p, root, LONG2, LOOKUP, 0),
        Ok(Search::NotFound { .. })
    ));

    unmount(p, mp);
    teardown();
}

#[test]
fn readep_and_readde() {
    let (disk, sub) = sample();
    let (_g, p) = setup(disk);
    let (mp, pmp, _root) = mounted(p);

    let (bp, off) = readep(pmp, MSDOSFSROOT, 96).unwrap();
    // SAFETY: the buffer is busy for the test (from `readep`) and mapped.
    let e = *Direntry::at(unsafe { bp.data() }, off);
    brelse(bp);
    assert_eq!(&e.name11(), b"M10C-FATTXT");
    assert_eq!(bp.b_bcount.get(), 512);

    let dep: &'static Denode = Box::leak(Box::new(Denode::new()));
    dep.de_pmp.set(Some(pmp));
    dep.de_dirclust.set(sub);
    dep.de_diroffset.set(32);
    let (bp, off) = readde(dep).unwrap();
    assert_eq!(off, 32);
    // SAFETY: as above.
    let e = *Direntry::at(unsafe { bp.data() }, off);
    brelse(bp);
    assert_eq!(&e.name11(), b"..         ");
    assert_eq!(e.deAttributes, ATTR_DIRECTORY);

    unmount(p, mp);
    teardown();
}

#[test]
fn readep_shortens_the_last_root_directory_block() {
    // 2 KB clusters and a root directory of one 512-byte block.
    let params = mkfat::Params {
        fat: 12,
        size: 1 << 20,
        bps: 512,
        spc: 4,
        rde: 16,
    };
    let mut img = Image::new(params);
    img.add_file(Dir::Root, None, b"ONLY    TXT", b"x");
    let (_g, p) = setup(img.finish());
    let mp = mount(p, true);
    let pmp = vfstomsdosfs(mp);
    assert_eq!(pmp.pm_rootdirsize.get(), 1);
    assert_eq!(pmp.pm_bpcluster.get(), 2048);
    let (bp, off) = readep(pmp, MSDOSFSROOT, 0).unwrap();
    assert_eq!(bp.b_bcount.get(), 512);
    // SAFETY: the buffer is busy for the test (from `readep`) and mapped.
    let e = *Direntry::at(unsafe { bp.data() }, off);
    brelse(bp);
    assert_eq!(&e.name11(), b"ONLY    TXT");
    unmount(p, mp);
    teardown();
}

#[test]
fn uniqdosname_skips_taken_names() {
    let (disk, _) = sample();
    let (_g, p) = setup(disk);
    let (mp, _pmp, root) = mounted(p);

    // LONG2's first generated name is taken.
    let mut short = [0u8; 11];
    uniqdosname(root, &cn(p, LONG2, CREATE, ISLASTCN), &mut short).unwrap();
    assert_eq!(short, dosname(LONG2, 2));
    // A new name keeps its first one.
    uniqdosname(
        root,
        &cn(p, b"another long name.txt", CREATE, ISLASTCN),
        &mut short,
    )
    .unwrap();
    assert_eq!(short, dosname(b"another long name.txt", 1));
    // Dots and blanks alone make no name.
    assert_eq!(
        uniqdosname(root, &cn(p, b". .", CREATE, ISLASTCN), &mut short),
        Err(Errno::EINVAL)
    );

    unmount(p, mp);
    teardown();
}

/// Creates the entry for `name` in `dp` where a `CREATE` lookup puts it (as
/// `msdosfs_create` does with a stack denode): the DOS entry's directory offset.
fn create_entry(p: &Proc, dp: &'static Denode, name: &'static [u8]) -> u32 {
    let cnp = cn(p, name, CREATE, ISLASTCN);
    let Ok(Search::NotFound { slotoffset, wincnt }) = msdosfs_lookup_search(
        dp,
        dp.de_StartCluster.get() == MSDOSFSROOT,
        &cnp,
        CREATE,
        ISLASTCN,
    ) else {
        panic!("{name:?} exists");
    };
    dp.de_fndoffset.set(slotoffset);
    dp.de_fndcnt.set(wincnt - 1);

    let tmpl = Denode::new();
    tmpl.de_pmp.set(dp.de_pmp.get());
    let mut short = [0u8; 11];
    uniqdosname(dp, &cnp, &mut short).unwrap();
    tmpl.de_Name.set(short);
    tmpl.de_Attributes.set(ATTR_ARCHIVE);
    tmpl.de_MDate.set(0x21);
    createde(&tmpl, dp, None, &cnp).unwrap();
    slotoffset
}

/// Removes the entry of `name` from `dp`, as `msdosfs_remove` does after a `DELETE` lookup.
fn remove_entry(p: &Proc, dp: &Denode, name: &'static [u8]) {
    assert!(matches!(
        search(p, dp, name, DELETE, ISLASTCN),
        Ok(Search::Found { .. })
    ));
    let victim = Denode::new();
    victim.de_refcnt.set(1);
    removede(dp, &victim).unwrap();
    assert_eq!(victim.de_refcnt.get(), 0);
}

#[test]
fn createde_and_removede_in_one_block() {
    let (disk, _) = sample();
    let (_g, p) = setup(disk);
    let (mp, _pmp, root) = mounted(p);
    let name: &'static [u8] = b"a much longer name.txt";

    assert_eq!(create_entry(p, root, name), 320);
    // The DOS entry where the lookup said, its two long name entries before it.
    let short = dosname(name, 1);
    let e = root_entry(320);
    assert_eq!(&e[..11], &short);
    assert_eq!(e[11], ATTR_ARCHIVE);
    let lfn = mkfat::lfn_entries(name, &short);
    assert_eq!(root_entry(256), lfn[0]);
    assert_eq!(root_entry(288), lfn[1]);
    assert_eq!(root.de_fndoffset.get(), 256);
    // It is found by its long name.
    let Ok(Search::Found { blkoff, .. }) = search(p, root, name, LOOKUP, 0) else {
        panic!("not created");
    };
    assert_eq!(blkoff, 320);

    remove_entry(p, root, name);
    for off in [256, 288, 320] {
        assert_eq!(root_entry(off)[0], SLOT_DELETED, "offset {off}");
    }
    // The entry before the long name entries is another file's, and stays.
    assert_eq!(&root_entry(224)[..11], &dosname(LONG2, 1));
    assert!(matches!(
        search(p, root, name, LOOKUP, 0),
        Ok(Search::NotFound { .. })
    ));
    assert!(matches!(
        search(p, root, LONG2, LOOKUP, 0),
        Ok(Search::Found { .. })
    ));

    unmount(p, mp);
    teardown();
}

#[test]
fn createde_and_removede_across_blocks() {
    // 14 used entries: the next three slots are 448, 480 (block 0) and 512 (block 1).
    let mut img = Image::new(mkfat::FAT12_1M);
    for i in 0..14u8 {
        let mut n = *b"F00     TXT";
        n[1] = b'0' + i / 10;
        n[2] = b'0' + i % 10;
        img.put(Dir::Root, &[empty_file(&n)]);
    }
    let (_g, p) = setup(img.finish());
    let (mp, _pmp, root) = mounted(p);
    let name: &'static [u8] = b"a much longer name.txt";

    assert_eq!(create_entry(p, root, name), 512);
    let short = dosname(name, 1);
    let lfn = mkfat::lfn_entries(name, &short);
    assert_eq!(&root_entry(512)[..11], &short);
    assert_eq!(root_entry(448), lfn[0]);
    assert_eq!(root_entry(480), lfn[1]);
    let Ok(Search::Found { blkoff, .. }) = search(p, root, name, LOOKUP, 0) else {
        panic!("not created");
    };
    assert_eq!(blkoff, 512);

    remove_entry(p, root, name);
    for off in [448, 480, 512] {
        assert_eq!(root_entry(off)[0], SLOT_DELETED, "offset {off}");
    }
    assert_eq!(&root_entry(416)[..3], b"F13");

    unmount(p, mp);
    teardown();
}

#[test]
fn createde_grows_a_full_directory_and_dosdirempty() {
    let mut img = Image::new(mkfat::FAT12_1M);
    let sub = img.mkdir(Dir::Root, b"SUBDIR     ", 1);
    let full = img.mkdir(Dir::Root, b"FULL       ", 1);
    // "." and "..", then 14 files: the 512-byte cluster is full.
    for i in 0..14u8 {
        let mut n = *b"F00     TXT";
        n[1] = b'0' + i / 10;
        n[2] = b'0' + i % 10;
        img.put(Dir::Clust(full), &[empty_file(&n)]);
    }
    let disk = img.finish();
    let (_g, p) = setup(disk);
    let (mp, pmp, _root) = mounted(p);

    let subdp = dirnode(pmp, sub);
    assert!(dosdirempty(subdp));
    let fulldp = dirnode(pmp, full);
    assert!(!dosdirempty(fulldp));
    assert_eq!(fulldp.de_FileSize.get(), 512);

    // A deleted entry does not make a directory non-empty.
    create_entry(p, subdp, b"x.txt");
    assert!(!dosdirempty(subdp));
    remove_entry(p, subdp, b"x.txt");
    assert!(dosdirempty(subdp));

    // No room: the new entry and its long name entry go to a new cluster.
    let free = pmp.pm_freeclustercount.get();
    assert_eq!(create_entry(p, fulldp, b"x.txt"), 544);
    assert_eq!(fulldp.de_FileSize.get(), 1024);
    assert_eq!(pmp.pm_freeclustercount.get(), free - 1);
    let Ok(Search::Found {
        cluster, blkoff, ..
    }) = search(p, fulldp, b"x.txt", LOOKUP, 0)
    else {
        panic!("not created");
    };
    assert_eq!(blkoff, 32);
    assert_ne!(cluster, full);
    msdosfs_sync(mp, MNT_WAIT, 0, p.p_ucred.get(), p).unwrap();
    {
        // The chain on the disk: the directory's cluster, then the new one.
        let d = DISK.lock().unwrap();
        let mut img = Image::new(mkfat::FAT12_1M);
        img.disk.copy_from_slice(&d);
        assert_eq!(img.fat_get(full), cluster);
        assert_eq!(img.fat_get(cluster), 0xfff);
        let o = img.clust_off(cluster);
        assert_eq!(&img.disk[o + 32..o + 43], b"X       TXT");
        assert_eq!(img.disk[o + 11], 0x0f);
    }

    unmount(p, mp);
    teardown();
}

fn test_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    VOP_UNLOCK(ap.a_vp)
}

fn test_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    ap.a_vp.v_data.set(core::ptr::null_mut());
    Ok(())
}

/// The operations `msdosfs_lookup` uses on the directory it searches, until the port of
/// `msdosfs_vnops.c`: every access allowed, locks that always succeed.
static TEST_VOPS: Vops = Vops {
    vop_access: Some(|_| nullop()),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_inactive: Some(test_inactive),
    vop_reclaim: Some(test_reclaim),
    ..Vops::EMPTY
};

/// A vnode of `mp` for the directory denode `dp`.
fn dirvnode(mp: &'static Mount, dp: &'static Denode) -> &'static Vnode {
    let vp = getnewvnode(VT_MSDOSFS, Some(mp), &TEST_VOPS).unwrap();
    vp.v_type.set(VDIR);
    vp.v_data.set(core::ptr::from_ref(dp).cast_mut().cast());
    dp.de_vnode.set(Some(vp));
    vp
}

#[test]
fn msdosfs_lookup_paths_without_new_denodes() {
    let (disk, sub) = sample();
    let (_g, p) = setup(disk);
    let (mp, pmp, root) = mounted(p);
    let rootvp = dirvnode(mp, root);
    rootvp.v_flag.set(rootvp.v_flag.get() | VROOT);
    let subdp = dirnode(pmp, sub);
    let subvp = dirvnode(mp, subdp);

    let lookup = |dvp: &'static Vnode, name: &'static [u8], op: u64, flags: u64| {
        let mut cnp = cn(p, name, op, flags);
        let mut vpp = None;
        let r = msdosfs_lookup(&mut VopLookupArgs {
            a_dvp: dvp,
            a_vpp: &mut vpp,
            a_cnp: &mut cnp,
        });
        (r, vpp, cnp.cn_flags)
    };

    // Not there.
    let (r, vpp, _) = lookup(rootvp, b"nothere", LOOKUP, ISLASTCN);
    assert_eq!(r, Err(Errno::ENOENT));
    assert!(vpp.is_none());
    // Not there, to be created: where it goes, the directory left locked.
    let (r, vpp, flags) = lookup(rootvp, b"x.txt", CREATE, ISLASTCN | LOCKPARENT);
    assert_eq!(r, Err(Errno::EJUSTRETURN));
    assert!(vpp.is_none());
    assert_eq!(root.de_fndoffset.get(), 288);
    assert_eq!(root.de_fndcnt.get(), 1);
    assert_ne!(flags & SAVENAME, 0);
    assert_eq!(flags & PDIRUNLOCK, 0);
    // Without LOCKPARENT the directory is unlocked.
    let (_, _, flags) = lookup(rootvp, b"x.txt", CREATE, ISLASTCN);
    assert_ne!(flags & PDIRUNLOCK, 0);
    // The root directory cannot be deleted or renamed over.
    let (r, _, _) = lookup(rootvp, b".", DELETE, ISLASTCN);
    assert_eq!(r, Err(Errno::EROFS));
    let (r, _, _) = lookup(rootvp, b"..", RENAME, ISLASTCN | WANTPARENT);
    assert_eq!(r, Err(Errno::EROFS));
    // "." of a subdirectory is the directory itself.
    let (r, vpp, _) = lookup(subvp, b".", LOOKUP, ISLASTCN);
    assert_eq!(r, Ok(()));
    assert!(vpp.is_some_and(|vp| core::ptr::eq(vp, subvp)));
    vrele(subvp);
    let (r, vpp, _) = lookup(subvp, b".", DELETE, ISLASTCN);
    assert_eq!(r, Ok(()));
    assert!(vpp.is_some_and(|vp| core::ptr::eq(vp, subvp)));
    vrele(subvp);
    let (r, _, _) = lookup(subvp, b".", RENAME, ISLASTCN | WANTPARENT);
    assert_eq!(r, Err(Errno::EISDIR));
    // A file is not a directory to search.
    let file = dirnode(pmp, sub);
    file.de_Attributes.set(ATTR_ARCHIVE);
    let filevp = dirvnode(mp, file);
    let (r, _, _) = lookup(filevp, b"x", LOOKUP, ISLASTCN);
    assert_eq!(r, Err(Errno::ENOTDIR));

    for vp in [rootvp, subvp, filevp] {
        vrele(vp);
    }
    unmount(p, mp);
    teardown();
}
