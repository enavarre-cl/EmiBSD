use super::*;

#[test]
fn commands_have_openbsd_values() {
    // The values the C compiler computes for _IOW('m', 1, struct mtop) and friends.
    assert_eq!(MTIOCTOP, 0x8008_6d01);
    assert_eq!(MTIOCGET, 0x4020_6d02);
    assert_eq!(MTIOCIEOT, 0x2000_6d03);
    assert_eq!(MTIOCEEOT, 0x2000_6d04);
    assert_eq!(MTIOCRDSPOS, 0x4004_6d05);
    assert_eq!(MTIOCRDHPOS, 0x4004_6d06);
    assert_eq!(MTIOCSLOCATE, 0x8004_6d05);
    assert_eq!(MTIOCHLOCATE, 0x8004_6d06);
}

#[test]
fn layouts_match_the_c_structs() {
    assert_eq!(core::mem::offset_of!(Mtop, mt_count), 4);
    assert_eq!(core::mem::offset_of!(Mtget, mt_fileno), 8);
    assert_eq!(core::mem::offset_of!(Mtget, mt_mdensity), 28);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/mtio.h");
    let mut ours = crate::reftest::assert_defines!(defs;
        MTWEOF, MTFSF, MTBSF, MTFSR, MTBSR, MTREW, MTOFFL, MTNOP, MTRETEN, MTERASE, MTEOM,
        MTNBSF, MTCACHE, MTNOCACHE, MTSETBSIZ, MTSETDNSTY);
    ours.extend(crate::reftest::assert_defines!(defs;
        MT_ISTS, MT_ISHT, MT_ISTM, MT_ISMT, MT_ISUT, MT_ISCPC, MT_ISAR, MT_ISTMSCP, MT_ISCY,
        MT_ISCT, MT_ISFHP, MT_ISEXABYTE, MT_ISEXA8200, MT_ISEXA8500, MT_ISVIPER1,
        MT_ISPYTHON, MT_ISHPDAT, MT_ISWANGTEK, MT_ISCALIPER, MT_ISWTEK5099, MT_ISVIPER2525,
        MT_ISMFOUR, MT_ISTK50, MT_ISMT02, MT_DS_RDONLY, MT_DS_MOUNTED));
    ours.extend(crate::reftest::assert_defines!(defs;
        T_UNIT, T_NOREWIND, T_DENSEL, T_800BPI, T_1600BPI, T_6250BPI, T_BADBPI));
    // The ioctl numbers are _IO* expressions, which the helper does not evaluate:
    // `commands_have_openbsd_values` pins their values, this pins the expressions.
    let ioctls = [
        ("MTIOCTOP", "_IOW('m', 1, struct mtop)"),
        ("MTIOCGET", "_IOR('m', 2, struct mtget)"),
        ("MTIOCIEOT", "_IO('m', 3)"),
        ("MTIOCEEOT", "_IO('m', 4)"),
        ("MTIOCRDSPOS", "_IOR('m', 5, u_int32_t)"),
        ("MTIOCRDHPOS", "_IOR('m', 6, u_int32_t)"),
        ("MTIOCSLOCATE", "_IOW('m', 5, u_int32_t)"),
        ("MTIOCHLOCATE", "_IOW('m', 6, u_int32_t)"),
    ];
    for (name, text) in ioctls {
        assert_eq!(defs.get(name).map(|s| s.as_str()), Some(text), "{name}");
        ours.push(name);
    }
    crate::reftest::assert_complete(&defs, "MT", &ours);
    crate::reftest::assert_complete(&defs, "T_", &ours);
}
