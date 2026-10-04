//! Host tests for `nfs_vfsops.c`: the mount argument decoding (`nfs_decode_args` and its
//! timeout arithmetic), the fsinfo digestion, the `nfs_args` bytes in `mount_info`, the
//! `fs.nfs` sysctl's error paths and the operations that only return an errno.

use std::boxed::Box;

use super::*;
use crate::kern::init_main::PROC0;
use crate::nfs::nfs::Nfsstats;
use crate::nfs::xdr_subs::txdr_unsigned;
use crate::sys::mount::{NFS_ARGSVERSION, NFSMNT_AUTHERR};
use crate::sys::socket::SOCK_STREAM;

/// A mount that lives for the rest of the test run, with the defaults `mountnfs` gives.
fn leak_mount() -> &'static NfsMount {
    let nmp: &'static NfsMount = Box::leak(Box::new(NfsMount::new()));
    nmp.nm_timeo.set(1);
    nmp.nm_retry.set(NFS_RETRANS);
    nmp.nm_wsize.set(NFS_WSIZE);
    nmp.nm_rsize.set(NFS_RSIZE);
    nmp.nm_readdirsize.set(NFS_READDIRSIZE);
    nmp.nm_numgrps.set(NFS_MAXGRPS);
    nmp.nm_readahead.set(NFS_DEFRAHEAD);
    nmp.nm_acregmin.set(NFS_MINATTRTIMO as u16);
    nmp.nm_acregmax.set(NFS_MAXATTRTIMO as u16);
    nmp.nm_acdirmin.set(NFS_MINATTRTIMO as u16);
    nmp.nm_acdirmax.set(NFS_MAXATTRTIMO as u16);
    nmp
}

/// Mount arguments with every field zero but the version and the socket type.
fn args(flags: i32, sotype: i32) -> NfsArgs {
    NfsArgs {
        version: NFS_ARGSVERSION,
        addr: 0,
        addrlen: 0,
        sotype,
        proto: 0,
        fh: 0,
        fhsize: 0,
        flags,
        wsize: 0,
        rsize: 0,
        readdirsize: 0,
        timeo: 0,
        retrans: 0,
        maxgrouplist: 0,
        readahead: 0,
        leaseterm: 0,
        deadthresh: 0,
        hostname: 0,
        acregmin: 0,
        acregmax: 0,
        acdirmin: 0,
        acdirmax: 0,
    }
}

#[test]
fn timeout_is_tenths_of_a_second_in_ticks_and_clamped() {
    // 1 second at hz 100.
    assert_eq!(decode_timeo(10, 100, 100, 6000), 100);
    // (3 * 100 + 5) / 10 = 30, below the minimum.
    assert_eq!(decode_timeo(3, 100, 100, 6000), 100);
    // 20 seconds, 2000 ticks, stays; 100 seconds is capped at the maximum.
    assert_eq!(decode_timeo(200, 100, 100, 6000), 2000);
    assert_eq!(decode_timeo(1000, 100, 100, 6000), 6000);
    // The +5 rounds half a tick up: 0.1 s at hz 5 is half a tick, which makes one.
    assert_eq!(decode_timeo(1, 5, 0, 600), 1);
    assert_eq!(decode_timeo(15, 10, 1, 600), 15);
}

#[test]
fn decode_args_rounds_and_caps_the_sizes() {
    let nmp = leak_mount();
    let mut out = args(0, SOCK_DGRAM);

    // NFSv3 over UDP: 32768 is the largest datagram; sizes round down to the block size.
    let mut a = args(
        NFSMNT_NFSV3 | NFSMNT_WSIZE | NFSMNT_RSIZE | NFSMNT_READDIRSIZE,
        SOCK_DGRAM,
    );
    a.wsize = 50000;
    a.rsize = 20000;
    a.readdirsize = 3000;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_wsize.get(), NFS_MAXDGRAMDATA as i32);
    let fab = NFS_FABLKSIZE as i32;
    assert_eq!(nmp.nm_rsize.get(), 20000 & !(fab - 1));
    assert_eq!(nmp.nm_readdirsize.get(), 3000 & !(NFS_DIRBLKSIZ - 1));
    assert_eq!(out.wsize, nmp.nm_wsize.get());
    assert_eq!(out.rsize, nmp.nm_rsize.get());
    assert_eq!(out.readdirsize, nmp.nm_readdirsize.get());

    // Too small to hold a block: one block (readdir: one directory block).
    let mut a = args(
        NFSMNT_NFSV3 | NFSMNT_WSIZE | NFSMNT_RSIZE | NFSMNT_READDIRSIZE,
        SOCK_DGRAM,
    );
    a.wsize = 100;
    a.rsize = 1;
    a.readdirsize = 5;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_wsize.get(), fab);
    assert_eq!(nmp.nm_rsize.get(), fab);
    assert_eq!(nmp.nm_readdirsize.get(), NFS_DIRBLKSIZ);

    // Version 2: 8192 at most, whatever the socket; TCP v3 may use NFS_MAXDATA.
    let mut a = args(NFSMNT_WSIZE | NFSMNT_RSIZE, SOCK_STREAM);
    a.wsize = 30000;
    a.rsize = 30000;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_wsize.get(), NFS_V2MAXDATA as i32);
    assert_eq!(nmp.nm_rsize.get(), NFS_V2MAXDATA as i32);
    let mut a = args(NFSMNT_NFSV3 | NFSMNT_WSIZE, SOCK_STREAM);
    a.wsize = 60000;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_wsize.get(), 60000 & !(fab - 1));
}

#[test]
fn decode_args_retries_flags_and_attribute_cache() {
    let nmp = leak_mount();
    let mut out = args(0, SOCK_DGRAM);

    // A hard mount retries "forever" (past the clip limit), whatever it was told.
    let mut a = args(NFSMNT_RETRANS, SOCK_DGRAM);
    a.retrans = 5;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_retry.get(), NFS_MAXREXMIT + 1);
    // A soft one takes the count, capped; 1 is ignored.
    let mut a = args(NFSMNT_SOFT | NFSMNT_RETRANS, SOCK_DGRAM);
    a.retrans = 5;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_retry.get(), 5);
    a.retrans = 100_000;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_retry.get(), NFS_MAXREXMIT);
    a.retrans = 1;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_retry.get(), NFS_MAXREXMIT);
    assert_eq!(out.retrans, NFS_MAXREXMIT);

    // The kernel's own flag bits survive, the caller's are dropped.
    nmp.nm_flag.set(NFSMNT_GOTFSINFO | NFSMNT_SOFT);
    let a = args(NFSMNT_NFSV3 | NFSMNT_GOTFSINFO | NFSMNT_AUTHERR, SOCK_DGRAM);
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_flag.get(), NFSMNT_NFSV3 | NFSMNT_GOTFSINFO);

    // Group list and read-ahead are range checked.
    let mut a = args(NFSMNT_MAXGRPS | NFSMNT_READAHEAD, SOCK_DGRAM);
    a.maxgrouplist = NFS_MAXGRPS + 1;
    a.readahead = NFS_MAXRAHEAD + 1;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_numgrps.get(), NFS_MAXGRPS);
    assert_eq!(nmp.nm_readahead.get(), NFS_DEFRAHEAD);
    a.maxgrouplist = 3;
    a.readahead = 2;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(out.maxgrouplist, 3);
    assert_eq!(out.readahead, 2);

    // Attribute cache times clip to 16 bits and the minimum never exceeds the maximum.
    let mut a = args(
        NFSMNT_ACREGMIN | NFSMNT_ACREGMAX | NFSMNT_ACDIRMIN | NFSMNT_ACDIRMAX,
        SOCK_DGRAM,
    );
    a.acregmin = 100_000;
    a.acregmax = 30;
    a.acdirmin = 7;
    a.acdirmax = 1_000_000;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_acregmax.get(), 30);
    assert_eq!(nmp.nm_acregmin.get(), 30);
    assert_eq!(nmp.nm_acdirmin.get(), 7);
    assert_eq!(nmp.nm_acdirmax.get(), 0xffff);
    assert_eq!((out.acregmin, out.acregmax), (30, 30));
    assert_eq!((out.acdirmin, out.acdirmax), (7, 0xffff));
    // Negative values are ignored.
    a.acregmin = -1;
    a.acdirmax = -5;
    nfs_decode_args(nmp, &a, &mut out);
    assert_eq!(nmp.nm_acregmin.get(), 30);
    assert_eq!(nmp.nm_acdirmax.get(), 0xffff);
}

/// The fsinfo reply with the given sizes (host order).
fn fsinfo(wtpref: u32, wtmax: u32, rtpref: u32, rtmax: u32, dtpref: u32) -> Nfsv3Fsinfo {
    Nfsv3Fsinfo {
        fs_rtmax: txdr_unsigned(rtmax),
        fs_rtpref: txdr_unsigned(rtpref),
        fs_wtmax: txdr_unsigned(wtmax),
        fs_wtpref: txdr_unsigned(wtpref),
        fs_dtpref: txdr_unsigned(dtpref),
        ..Nfsv3Fsinfo::default()
    }
}

#[test]
fn fsinfo_shrinks_the_transfer_sizes() {
    let nmp = leak_mount();
    nmp.nm_flag.set(NFSMNT_NFSV3);
    let fab = NFS_FABLKSIZE as i32;

    // The server prefers less than the mount asks for: round the preference up to a block.
    fsinfo_apply(nmp, &fsinfo(4000, 65536, 5000, 65536, 2000));
    assert_eq!(nmp.nm_wsize.get(), (4000 + fab - 1) & !(fab - 1));
    assert_eq!(nmp.nm_rsize.get(), (5000 + fab - 1) & !(fab - 1));
    assert_eq!(nmp.nm_readdirsize.get(), 2048);
    assert_ne!(nmp.nm_flag.get() & NFSMNT_GOTFSINFO, 0);
    assert_ne!(nmp.nm_flag.get() & NFSMNT_NFSV3, 0);

    // A maximum below that rounds down; a maximum below one block is taken as it is.
    let nmp = leak_mount();
    fsinfo_apply(nmp, &fsinfo(4000, 3000, 8192, 100, 8192));
    assert_eq!(nmp.nm_wsize.get(), 3000 & !(fab - 1));
    assert_eq!(nmp.nm_rsize.get(), 100);
    // (The C compares the readdir size with the read maximum, not a maximum of its own.)
    assert_eq!(nmp.nm_readdirsize.get(), 100);

    // Larger preferences leave the sizes alone.
    let nmp = leak_mount();
    fsinfo_apply(nmp, &fsinfo(1 << 20, 1 << 20, 1 << 20, 1 << 20, 1 << 20));
    assert_eq!(nmp.nm_wsize.get(), NFS_WSIZE);
    assert_eq!(nmp.nm_rsize.get(), NFS_RSIZE);
    assert_eq!(nmp.nm_readdirsize.get(), NFS_READDIRSIZE);
    assert_ne!(nmp.nm_flag.get() & NFSMNT_GOTFSINFO, 0);
}

#[test]
fn mount_info_holds_the_args_field_by_field() {
    let mut a = args(NFSMNT_NFSV3 | NFSMNT_SOFT, SOCK_STREAM);
    a.addr = 0x1122_3344_5566_7788;
    a.addrlen = 16;
    a.proto = 6;
    a.fh = 0x99;
    a.fhsize = 28;
    a.wsize = 8192;
    a.rsize = 16384;
    a.timeo = 600;
    a.hostname = 0xdead_beef;
    a.acdirmax = 60;

    let b = nfsargs_bytes(&a);
    // The padding between `version` and `addr` and after `addrlen`... stays zero.
    assert_eq!(&b[4..8], &[0; 4]);
    let back = NfsArgs::from_bytes(&b).expect("a whole nfs_args");
    assert_eq!(nfsargs_bytes(&back), b);
    assert_eq!(back.version, NFS_ARGSVERSION);
    assert_eq!(back.addr, a.addr);
    assert_eq!(back.addrlen, 16);
    assert_eq!(back.sotype, SOCK_STREAM);
    assert_eq!(back.flags, a.flags);
    assert_eq!(back.rsize, 16384);
    assert_eq!(back.timeo, 600);
    assert_eq!(back.hostname, 0xdead_beef);
    assert_eq!(back.acdirmax, 60);

    // Through a mount's statfs.
    let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
    store_mount_args(mp, &a);
    assert_eq!(nfsargs_bytes(&mount_args(mp)), b);
}

#[test]
fn sysctl_errors_and_the_statistics_size() {
    let mut len = 0usize;

    assert_eq!(
        nfs_sysctl(&[NFS_NFSSTATS, 1], 0, &mut len, 0, 0, &PROC0),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        nfs_sysctl(&[], 0, &mut len, 0, 0, &PROC0),
        Err(Errno::EOPNOTSUPP)
    );
    assert_eq!(
        nfs_sysctl(&[99], 0, &mut len, 0, 0, &PROC0),
        Err(Errno::EOPNOTSUPP)
    );

    // No buffer: report the size of `struct nfsstats`.
    assert_eq!(
        nfs_sysctl(&[NFS_NFSSTATS], 0, &mut len, 0, 0, &PROC0),
        Ok(())
    );
    assert_eq!(len, Nfsstats::NWORDS * 8);

    // A buffer that is too small: ENOMEM and the size.
    let mut small = 8usize;
    assert_eq!(
        nfs_sysctl(&[NFS_NFSSTATS], 0x1000, &mut small, 0, 0, &PROC0),
        Err(Errno::ENOMEM)
    );
    assert_eq!(small, Nfsstats::NWORDS * 8);
}

#[test]
fn operations_that_only_fail_or_do_nothing() {
    let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
    assert_eq!(nfs_start(mp, 0, &PROC0), Ok(()));
    assert!(matches!(
        nfs_quotactl(mp, 0, 0, 0, &PROC0),
        Err(Errno::EOPNOTSUPP)
    ));
    assert!(matches!(nfs_vget(mp, 2), Err(Errno::EOPNOTSUPP)));
    assert!(matches!(
        nfs_fhtovp(mp, &Fid::default()),
        Err(Errno::EINVAL)
    ));

    assert!(NFS_VFSOPS.vfs_init.is_some());
    assert!(NFS_VFSOPS.vfs_sysctl.is_some());
}
