//! Tests of `param.rs`; see there.

use super::*;

#[test]
fn derived_constants() {
    assert_eq!(MAXCOMLEN, 23);
    assert_eq!(MAXLOGNAME, 32);
    assert_eq!(NGROUPS, 16);
    assert_eq!(DEV_BSIZE, 512);
    assert_eq!(BLKDEV_IOSIZE, PAGE_SIZE);
    assert_eq!(MAXPATHLEN, 1024);
    assert_eq!(MAXSYMLINKS, 32);
    assert_eq!(FSCALE, 2048);
    assert_eq!(NBPG, 4096);
    assert_eq!(PGOFSET, 0xfff);
    assert!(MAXPATHLEN <= MAXBSIZE && powerof2(MAXPATHLEN));
}

#[test]
fn block_conversions() {
    assert_eq!(ctod(1), PAGE_SIZE / DEV_BSIZE);
    assert_eq!(dtoc(ctod(3)), 3);
    assert_eq!(btodb(1024), 2);
    assert_eq!(dbtob(2), 1024);
}

#[test]
fn alignment() {
    assert_eq!(ALIGNBYTES, 7);
    assert_eq!(align(0), 0);
    assert_eq!(align(1), 8);
    assert_eq!(align(8), 8);
    assert_eq!(align(9), 16);
    assert!(aligned_pointer::<u64>(3));
}

#[test]
fn counting_and_rounding() {
    assert_eq!(howmany(0, 4), 0);
    assert_eq!(howmany(1, 4), 1);
    assert_eq!(howmany(8, 4), 2);
    assert_eq!(howmany(9, 4), 3);
    assert_eq!(roundup(0, 8), 0);
    assert_eq!(roundup(5, 8), 8);
    assert_eq!(roundup(16, 8), 16);
    assert!(powerof2(0));
    assert!(powerof2(1));
    assert!(powerof2(4096));
    assert!(!powerof2(6));
    assert!(!powerof2(usize::MAX));
}

#[test]
fn bitmaps() {
    let mut map = [0u8; 4];
    setbit(&mut map, 0);
    setbit(&mut map, 9);
    setbit(&mut map, 31);
    assert_eq!(map, [0x01, 0x02, 0x00, 0x80]);
    assert!(isset(&map, 9));
    assert!(isclr(&map, 10));
    clrbit(&mut map, 9);
    assert!(isclr(&map, 9));
    assert_eq!(map, [0x01, 0x00, 0x00, 0x80]);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let mut defs = crate::reftest::defines("sys/sys/param.h");
    defs.extend(crate::reftest::defines("sys/sys/syslimits.h"));
    let ours: &[(&str, i64)] = &[
        ("BSD", BSD as i64),
        ("BSD4_3", BSD4_3 as i64),
        ("BSD4_4", BSD4_4 as i64),
        ("OpenBSD", OpenBSD as i64),
        ("OpenBSD8_0", OpenBSD8_0 as i64),
        ("MAXCOMLEN", MAXCOMLEN as i64),
        ("MAXINTERP", MAXINTERP as i64),
        ("MAXLOGNAME", MAXLOGNAME as i64),
        ("MAXUPRC", MAXUPRC as i64),
        ("NCARGS", NCARGS as i64),
        ("NGROUPS", NGROUPS as i64),
        ("NOFILE", NOFILE as i64),
        ("NOFILE_MAX", NOFILE_MAX as i64),
        ("NOGROUP", NOGROUP as i64),
        ("MAXHOSTNAMELEN", MAXHOSTNAMELEN as i64),
        ("PSWP", PSWP as i64),
        ("PVM", PVM as i64),
        ("PINOD", PINOD as i64),
        ("PRIBIO", PRIBIO as i64),
        ("PVFS", PVFS as i64),
        ("PZERO", PZERO as i64),
        ("PSOCK", PSOCK as i64),
        ("PWAIT", PWAIT as i64),
        ("PLOCK", PLOCK as i64),
        ("PPAUSE", PPAUSE as i64),
        ("PUSER", PUSER as i64),
        ("MAXPRI", MAXPRI as i64),
        ("PRIMASK", PRIMASK as i64),
        ("PCATCH", PCATCH as i64),
        ("PNORELOCK", PNORELOCK as i64),
        ("MAXPHYS", MAXPHYS as i64),
        ("MAXBSIZE", MAXBSIZE as i64),
        ("_DEV_BSHIFT", _DEV_BSHIFT as i64),
        ("DEV_BSIZE", DEV_BSIZE as i64),
        ("DEV_BSHIFT", DEV_BSHIFT as i64),
        ("MAXPATHLEN", MAXPATHLEN as i64),
        ("MAXSYMLINKS", MAXSYMLINKS as i64),
        ("_FSHIFT", _FSHIFT as i64),
        ("FSHIFT", FSHIFT as i64),
        ("FSCALE", FSCALE as i64),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn host_double_mirrors_amd64_machine_param() {
    let mut defs = crate::reftest::defines("sys/arch/amd64/include/param.h");
    defs.extend(crate::reftest::defines("sys/arch/amd64/include/_types.h"));
    let ours: &[(&str, i64)] = &[
        ("PAGE_SHIFT", PAGE_SHIFT as i64),
        ("PAGE_SIZE", PAGE_SIZE as i64),
        ("PAGE_MASK", PAGE_MASK as i64),
        ("KERNBASE", KERNBASE as i64),
        ("UPAGES", UPAGES as i64),
        ("USPACE", USPACE as i64),
        ("USPACE_ALIGN", USPACE_ALIGN as i64),
        ("NMBCLUSTERS", NMBCLUSTERS as i64),
        ("MSGBUFSIZE", MSGBUFSIZE as i64),
        (
            "_STACKALIGNBYTES",
            <Machine as MachineParam>::STACKALIGNBYTES as i64,
        ),
        (
            "_MAX_PAGE_SHIFT",
            <Machine as MachineParam>::MAX_PAGE_SHIFT as i64,
        ),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
    assert_eq!(defs.get("MACHINE").map(|s| s.as_str()), Some("\"amd64\""));
}
