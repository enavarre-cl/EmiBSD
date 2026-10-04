use super::*;

#[test]
fn commands_have_openbsd_values() {
    // The values the C compiler computes for _IOWR('B', n, struct ...) on LP64.
    assert_eq!(BIOCLOCATE, 0xc2c0_4200);
    assert_eq!(BIOCINQ, 0xc2d0_4220);
    assert_eq!(BIOCDISK, 0xc330_4221);
    assert_eq!(BIOCVOL, 0xc310_4222);
    assert_eq!(BIOCALARM, 0xc2c0_4223);
    assert_eq!(BIOCBLINK, 0xc2c0_4224);
    assert_eq!(BIOCSETSTATE, 0xc2d0_4225);
    assert_eq!(BIOCCREATERAID, 0xc2e8_4226);
    assert_eq!(BIOCDELETERAID, 0xc2d0_4227);
    assert_eq!(BIOCDISCIPLINE, 0xc2d8_4228);
    assert_eq!(BIOCINSTALLBOOT, 0xc2e0_4229);
    assert_eq!(BIOCPATROL, 0xc2d0_422a);
}

/// Every `BIOC_*_S` and the like: a string constant equals the C string literal.
macro_rules! assert_strings {
    ($defs:expr; $($name:ident),* $(,)?) => {{
        let mut names: std::vec::Vec<&'static str> = std::vec::Vec::new();
        $(
            names.push(stringify!($name));
            assert_eq!(
                $defs.get(stringify!($name)).map(|s| s.as_str()),
                Some(std::format!("{:?}", $name).as_str()),
                "{}",
                stringify!($name)
            );
        )*
        names
    }};
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/biovar.h");
    let mut ours = crate::reftest::assert_defines!(defs;
        BIO_MSG_COUNT, BIO_MSG_LEN, BIO_MSG_INFO, BIO_MSG_WARN, BIO_MSG_ERROR,
        BIO_STATUS_UNKNOWN, BIO_STATUS_SUCCESS, BIO_STATUS_ERROR,
        BIOC_SDONLINE, BIOC_SDOFFLINE, BIOC_SDFAILED, BIOC_SDREBUILD, BIOC_SDHOTSPARE,
        BIOC_SDUNUSED, BIOC_SDSCRUB, BIOC_SDINVALID,
        BIOC_SVONLINE, BIOC_SVOFFLINE, BIOC_SVDEGRADED, BIOC_SVBUILDING, BIOC_SVSCRUB,
        BIOC_SVREBUILD, BIOC_SVINVALID,
        BIOC_CVUNKNOWN, BIOC_CVWRITEBACK, BIOC_CVWRITETHROUGH,
        BIOC_SADISABLE, BIOC_SAENABLE, BIOC_SASILENCE, BIOC_GASTATUS, BIOC_SATEST,
        BIOC_SBUNBLINK, BIOC_SBBLINK, BIOC_SBALARM,
        BIOC_SSOTHER_UNUSED, BIOC_SSOTHER_DEVT,
        BIOC_SSONLINE, BIOC_SSOFFLINE, BIOC_SSHOTSPARE, BIOC_SSREBUILD);
    ours.extend(crate::reftest::assert_defines!(defs;
        BIOC_CRMAXLEN, BIOC_SCFORCE, BIOC_SCDEVT, BIOC_SCNOAUTOASSEMBLE, BIOC_SCBOOTABLE,
        BIOC_SOINVALID, BIOC_SOIN, BIOC_SOOUT, BIOC_SOINOUT_FAILED, BIOC_SOINOUT_OK,
        BIOC_SDCLEARMETA,
        BIOC_SPSTOP, BIOC_SPSTART, BIOC_GPSTATUS, BIOC_SPDISABLE, BIOC_SPAUTO, BIOC_SPMANUAL,
        BIOC_SPMAUTO, BIOC_SPMMANUAL, BIOC_SPMDISABLED,
        BIOC_SPSSTOPPED, BIOC_SPSREADY, BIOC_SPSACTIVE, BIOC_SPSABORTED));
    ours.extend(crate::reftest::assert_defines!(defs;
        BIOC_INQ, BIOC_DISK, BIOC_VOL, BIOC_ALARM, BIOC_BLINK, BIOC_SETSTATE,
        BIOC_CREATERAID, BIOC_DELETERAID, BIOC_DISCIPLINE, BIOC_INSTALLBOOT, BIOC_PATROL,
        BIOC_DEVLIST));
    ours.extend(assert_strings!(defs;
        BIOC_SDONLINE_S, BIOC_SDOFFLINE_S, BIOC_SDFAILED_S, BIOC_SDREBUILD_S,
        BIOC_SDHOTSPARE_S, BIOC_SDUNUSED_S, BIOC_SDSCRUB_S, BIOC_SDINVALID_S,
        BIOC_SVONLINE_S, BIOC_SVOFFLINE_S, BIOC_SVDEGRADED_S, BIOC_SVBUILDING_S,
        BIOC_SVSCRUB_S, BIOC_SVREBUILD_S, BIOC_SVINVALID_S,
        BIOC_CVUNKNOWN_S, BIOC_CVWRITEBACK_S, BIOC_CVWRITETHROUGH_S));
    // The ioctl numbers are _IOWR expressions, which the helper does not evaluate:
    // `commands_have_openbsd_values` pins their values, this pins the expressions.
    let ioctls = [
        ("BIOCLOCATE", "_IOWR('B', 0, struct bio_locate)"),
        ("BIOCINQ", "_IOWR('B', 32, struct bioc_inq)"),
        ("BIOCDISK", "_IOWR('B', 33, struct bioc_disk)"),
        ("BIOCVOL", "_IOWR('B', 34, struct bioc_vol)"),
        ("BIOCALARM", "_IOWR('B', 35, struct bioc_alarm)"),
        ("BIOCBLINK", "_IOWR('B', 36, struct bioc_blink)"),
        ("BIOCSETSTATE", "_IOWR('B', 37, struct bioc_setstate)"),
        ("BIOCCREATERAID", "_IOWR('B', 38, struct bioc_createraid)"),
        ("BIOCDELETERAID", "_IOWR('B', 39, struct bioc_deleteraid)"),
        ("BIOCDISCIPLINE", "_IOWR('B', 40, struct bioc_discipline)"),
        ("BIOCINSTALLBOOT", "_IOWR('B', 41, struct bioc_installboot)"),
        ("BIOCPATROL", "_IOWR('B', 42, struct bioc_patrol)"),
    ];
    for (name, text) in ioctls {
        assert_eq!(defs.get(name).map(|s| s.as_str()), Some(text), "{name}");
        ours.push(name);
    }
    crate::reftest::assert_complete(&defs, "BIO", &ours);
}
