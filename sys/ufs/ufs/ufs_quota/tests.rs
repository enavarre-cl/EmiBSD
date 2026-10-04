//! Host tests for disk quotas: the limit and grace logic of `chkdqchg`/`chkiqchg`,
//! `setquota` and `setuse`, and `quotactl(2)` on an FFS image mounted on the host, with a
//! quota file laid out as `quotacheck(8)` leaves it: a user writes until `EDQUOT`, the
//! usage is read back with `Q_GETQUOTA`, and it survives `Q_SYNC`, an unmount with quotas
//! on, `Q_QUOTAOFF` and a remount.

use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_descrip::sys_close;
use crate::kern::kern_prot::crget;
use crate::kern::sys_generic::sys_write;
use crate::kern::vfs_syscalls::{sys_chown, sys_open, sys_quotactl, sys_unlink};
use crate::sys::fcntl::{O_CREAT, O_RDWR};
use crate::ufs::ffs::ffs_vfsops::tests::{
    DISK as DISKIMG, mount_root, newfs, path, read_file, setup, sys, teardown, unmount_root,
};
use crate::ufs::ufs::quota::qcmd;

const NOW: Time = 1_000_000;
const WEEK: Time = 7 * 24 * 60 * 60;

fn dqblk(bhard: u32, bsoft: u32, curb: u32, ihard: u32, isoft: u32, curi: u32) -> Dqblk {
    Dqblk {
        dqb_bhardlimit: bhard,
        dqb_bsoftlimit: bsoft,
        dqb_curblocks: curb,
        dqb_ihardlimit: ihard,
        dqb_isoftlimit: isoft,
        dqb_curinodes: curi,
        dqb_btime: 0,
        dqb_itime: 0,
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn dquot_flags_match_the_c_file() {
    let defs = crate::reftest::defines("sys/ufs/ufs/ufs_quota.c");
    for (name, value) in [
        ("DQ_LOCK", DQ_LOCK),
        ("DQ_WANT", DQ_WANT),
        ("DQ_MOD", DQ_MOD),
        ("DQ_FAKE", DQ_FAKE),
        ("DQ_BLKS", DQ_BLKS),
        ("DQ_INODS", DQ_INODS),
    ] {
        assert_eq!(
            crate::reftest::int(&defs, name),
            Some(i64::from(value)),
            "{name}"
        );
    }
    assert_eq!(crate::reftest::int(&defs, "DQUOTINC"), Some(DQUOTINC));
    let defs = crate::reftest::defines("sys/ufs/ufs/quota.h");
    assert_eq!(crate::reftest::int(&defs, "MAX_DQ_TIME"), Some(MAX_DQ_TIME));
    assert_eq!(crate::reftest::int(&defs, "MAX_IQ_TIME"), Some(MAX_IQ_TIME));
}

#[test]
fn chklimit_hard_soft_and_grace() {
    // No limits: anything goes.
    assert_eq!(chklimit(1000, 1000, 0, 0, 0, NOW), Limit::Under);
    // The hard limit is reached when the new usage gets to it (">=").
    assert_eq!(chklimit(90, 9, 100, 0, 0, NOW), Limit::Under);
    assert_eq!(chklimit(90, 10, 100, 0, 0, NOW), Limit::HardReached);
    assert_eq!(chklimit(0, 200, 100, 0, 0, NOW), Limit::HardReached);
    // Crossing the soft limit starts the grace time (allowed, with a warning).
    assert_eq!(chklimit(40, 10, 100, 50, 0, NOW), Limit::SoftCrossed);
    assert_eq!(chklimit(40, 9, 100, 50, 0, NOW), Limit::Under);
    // Over it already: allowed until the deadline, refused after it.
    let deadline = (NOW + 10) as u32;
    assert_eq!(chklimit(60, 1, 100, 50, deadline, NOW), Limit::Under);
    assert_eq!(chklimit(60, 1, 100, 50, deadline, NOW + 10), Limit::Under);
    assert_eq!(
        chklimit(60, 1, 100, 50, deadline, NOW + 11),
        Limit::GraceExpired
    );
    // The hard limit wins over the soft one.
    assert_eq!(chklimit(60, 50, 100, 50, deadline, NOW), Limit::HardReached);
    // Giving back is never refused by the soft limit's grace when under it.
    assert_eq!(
        chklimit(60, -20, 100, 50, deadline, NOW + 100),
        Limit::Under
    );
}

#[test]
fn setquota_keeps_usage_and_starts_grace() {
    // The current values come from the old dquot, not from the caller.
    let old = dqblk(0, 0, 70, 0, 0, 5);
    let (b, f) = setquota_dqblk(old, 0, 1000, dqblk(100, 50, 0, 10, 4, 0), NOW, WEEK, WEEK);
    assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (70, 5));
    assert_eq!((b.dqb_bhardlimit, b.dqb_bsoftlimit), (100, 50));
    // Over both new soft limits with none before: both grace times start now.
    assert_eq!(b.dqb_btime, (NOW + WEEK) as u32);
    assert_eq!(b.dqb_itime, (NOW + WEEK) as u32);
    assert_eq!(f, DQ_MOD);

    // Already over the old soft limit: the old grace time is kept for an id but 0.
    let mut old = dqblk(100, 50, 70, 0, 0, 0);
    old.dqb_btime = 1234;
    let (b, _) = setquota_dqblk(old, 0, 1000, dqblk(100, 60, 0, 0, 0, 0), NOW, WEEK, WEEK);
    assert_eq!(b.dqb_btime, 1234);
    // Id 0 carries the file system's grace times: they are taken from the caller.
    let mut newlim = dqblk(0, 0, 0, 0, 0, 0);
    newlim.dqb_btime = 3600;
    newlim.dqb_itime = 7200;
    let (b, f) = setquota_dqblk(old, DQ_BLKS, 0, newlim, NOW, WEEK, WEEK);
    assert_eq!((b.dqb_btime, b.dqb_itime), (3600, 7200));
    // No limits left: just usage; the warning flags clear only under a soft limit.
    assert_eq!(f, DQ_BLKS | DQ_FAKE | DQ_MOD);
    let (_, f) = setquota_dqblk(
        old,
        DQ_BLKS | DQ_FAKE,
        1000,
        dqblk(0, 80, 0, 0, 0, 0),
        NOW,
        1,
        1,
    );
    assert_eq!(f, DQ_MOD);
}

#[test]
fn setuse_sets_usage_and_starts_grace() {
    let old = dqblk(100, 50, 10, 10, 4, 1);
    let (b, f) = setuse_dqblk(
        old,
        DQ_BLKS | DQ_INODS,
        dqblk(9, 9, 60, 9, 9, 5),
        NOW,
        30,
        40,
    );
    assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (60, 5));
    // The limits are not the caller's.
    assert_eq!(
        (b.dqb_bhardlimit, b.dqb_bsoftlimit, b.dqb_isoftlimit),
        (100, 50, 4)
    );
    assert_eq!(b.dqb_btime, (NOW + 30) as u32);
    assert_eq!(b.dqb_itime, (NOW + 40) as u32);
    assert_eq!(f, DQ_BLKS | DQ_INODS | DQ_MOD);
    // Back under the soft limits: the warnings are forgotten, the times stay.
    let (b2, f) = setuse_dqblk(b, f, dqblk(0, 0, 3, 0, 0, 1), NOW + 5, 30, 40);
    assert_eq!(b2.dqb_btime, b.dqb_btime);
    assert_eq!(f, DQ_MOD);
}

/// The user whose quota the file system test sets.
const USER: Uid = 1000;
/// Their block hard limit, in `DEV_BSIZE` units (50 KB).
const BHARD: u32 = 100;

/// A user quota file as `quotacheck(8)` leaves it for a file system where nothing belongs
/// to `USER` yet, with `edquota(8)`'s hard limit for `USER`: one `dqblk` per id up to
/// `USER`.
fn quota_file() -> Vec<u8> {
    let mut f = Vec::new();
    for id in 0..=USER {
        let b = if id == USER {
            dqblk(BHARD, 0, 0, 0, 0, 0)
        } else {
            Dqblk::default()
        };
        for w in [
            b.dqb_bhardlimit,
            b.dqb_bsoftlimit,
            b.dqb_curblocks,
            b.dqb_ihardlimit,
            b.dqb_isoftlimit,
            b.dqb_curinodes,
            b.dqb_btime,
            b.dqb_itime,
        ] {
            f.extend_from_slice(&w.to_ne_bytes());
        }
    }
    f
}

/// The `dqblk` of `id` in a quota file's bytes.
fn entry(file: &[u8], id: Uid) -> Dqblk {
    let off = id as usize * size_of::<Dqblk>();
    let w = |i: usize| {
        let o = off + 4 * i;
        u32::from_ne_bytes([file[o], file[o + 1], file[o + 2], file[o + 3]])
    };
    Dqblk {
        dqb_bhardlimit: w(0),
        dqb_bsoftlimit: w(1),
        dqb_curblocks: w(2),
        dqb_ihardlimit: w(3),
        dqb_isoftlimit: w(4),
        dqb_curinodes: w(5),
        dqb_btime: w(6),
        dqb_itime: w(7),
    }
}

/// `quotactl("/", QCMD(cmd, USRQUOTA), uid, addr)`.
fn quotactl(p: &Proc, cmd: i32, uid: Uid, addr: usize) -> Result<isize, Errno> {
    sys(
        sys_quotactl,
        p,
        &[
            path(b"/\0"),
            qcmd(cmd, USRQUOTA as i32) as usize,
            uid as usize,
            addr,
        ],
    )
}

/// `Q_GETQUOTA` of `uid`'s user quota.
fn getq(p: &Proc, uid: Uid) -> Result<Dqblk, Errno> {
    let mut b = Dqblk::default();
    quotactl(p, Q_GETQUOTA, uid, ptr::from_mut(&mut b) as usize)?;
    Ok(b)
}

/// Runs the thread as `uid` (a new credential) until the returned one is put back.
fn become_user(p: &Proc, uid: Uid) -> *const Ucred {
    let cr = crget();
    for c in [&cr.cr_uid, &cr.cr_ruid, &cr.cr_svuid] {
        c.set(uid);
    }
    for c in [&cr.cr_gid, &cr.cr_rgid, &cr.cr_svgid] {
        c.set(uid);
    }
    p.p_ucred.replace(ptr::from_ref(cr))
}

#[test]
fn quotas_on_an_ffs_image() {
    let mut img = newfs::Image::new(newfs::FFS2_4M);
    img.add_file(b"quota.user", &quota_file());
    let (_g, p) = setup(img.finish());

    let mp = mount_root(p, false);
    // Not on yet: no quota to get.
    assert_eq!(getq(p, USER), Err(Errno::EINVAL));
    quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
    assert_ne!(mp.mnt_flag.get() & MNT_QUOTA, 0);
    let b = getq(p, USER).unwrap();
    assert_eq!(
        (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
        (BHARD, 0, 0)
    );

    // A file for the user: chown moves the inode's charge to them.
    let fd = sys(
        sys_open,
        p,
        &[path(b"/f\0"), (O_RDWR | O_CREAT) as usize, 0o644],
    )
    .unwrap();
    sys(sys_close, p, &[fd as usize]).unwrap();
    sys(sys_chown, p, &[path(b"/f\0"), USER as usize, 0]).unwrap();
    assert_eq!(getq(p, USER).unwrap().dqb_curinodes, 1);

    // The user writes 8 KB blocks until the hard limit refuses one: six blocks are 96
    // sectors, a seventh would make 112 >= 100.
    let root = become_user(p, USER);
    let fd = sys(sys_open, p, &[path(b"/f\0"), O_RDWR as usize, 0]).unwrap();
    let chunk = vec![0x5au8; 8192];
    let mut written = 0;
    let error = loop {
        match sys(
            sys_write,
            p,
            &[fd as usize, chunk.as_ptr() as usize, chunk.len()],
        ) {
            Ok(n) => written += n as usize,
            Err(e) => break e,
        }
        assert!(
            written <= 1 << 20,
            "the hard limit never stopped the writes"
        );
    };
    assert_eq!(error, Errno::EDQUOT);
    assert_eq!(written, 6 * 8192);
    sys(sys_close, p, &[fd as usize]).unwrap();
    // A user may read their own quota, not somebody else's.
    let b = getq(p, USER).unwrap();
    assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (96, 1));
    assert_eq!(getq(p, 0), Err(Errno::EPERM));
    assert_eq!(quotactl(p, Q_QUOTAOFF, 0, 0), Err(Errno::EPERM));
    let user = p.p_ucred.replace(root);
    // SAFETY: `become_user`'s credential, whose reference the thread gives up.
    crfree(unsafe { &*user });

    // Q_SYNC writes the usage into the quota file.
    quotactl(p, Q_SYNC, 0, 0).unwrap();
    let e = entry(&read_file(p, b"/quota.user\0").unwrap(), USER);
    assert_eq!(
        (e.dqb_bhardlimit, e.dqb_curblocks, e.dqb_curinodes),
        (BHARD, 96, 1)
    );

    // Unmounting with quotas on turns them off (ffs_flushfiles); a remount reads the usage
    // back from the file.
    unmount_root(p, mp);
    newfs::check(&DISKIMG.lock().unwrap(), true);
    let mp = mount_root(p, false);
    assert_eq!(mp.mnt_flag.get() & MNT_QUOTA, 0);
    quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
    let b = getq(p, USER).unwrap();
    assert_eq!(
        (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
        (BHARD, 96, 1)
    );

    // Q_SETQUOTA raises the limit (the usage stays the kernel's), Q_SETUSE sets the usage.
    let mut lim = dqblk(BHARD * 2, 0, 12345, 0, 0, 999);
    quotactl(p, Q_SETQUOTA, USER, ptr::from_mut(&mut lim) as usize).unwrap();
    let b = getq(p, USER).unwrap();
    assert_eq!(
        (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
        (BHARD * 2, 96, 1)
    );
    let mut usage = dqblk(0, 0, 90, 0, 0, 1);
    quotactl(p, Q_SETUSE, USER, ptr::from_mut(&mut usage) as usize).unwrap();
    assert_eq!(getq(p, USER).unwrap().dqb_curblocks, 90);

    // Removing the file gives the blocks and the inode back.
    sys(sys_unlink, p, &[path(b"/f\0")]).unwrap();
    let b = getq(p, USER).unwrap();
    assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (0, 0));

    // Q_QUOTAOFF writes the dquots back and closes the file; a remount and Q_QUOTAON find
    // the new limit on disk.
    quotactl(p, Q_QUOTAOFF, 0, 0).unwrap();
    assert_eq!(mp.mnt_flag.get() & MNT_QUOTA, 0);
    assert_eq!(getq(p, USER), Err(Errno::EINVAL));
    unmount_root(p, mp);
    let mp = mount_root(p, false);
    quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
    let b = getq(p, USER).unwrap();
    assert_eq!(
        (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
        (BHARD * 2, 0, 0)
    );
    unmount_root(p, mp);
    newfs::check(&DISKIMG.lock().unwrap(), true);
    teardown();
}
