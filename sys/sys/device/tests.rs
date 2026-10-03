//! Host tests for `<sys/device.h>`: the types and the constants (against the C header).

use super::*;

#[test]
fn devclass_values_follow_the_c_enum() {
    assert_eq!(DV_DULL as i32, 0);
    assert_eq!(DV_TTY as i32, 5);
}

#[test]
fn cfdriver_starts_without_units() {
    static CD: Cfdriver = Cfdriver::new(b"test", DV_DULL, 0);
    assert_eq!(CD.cd_ndevs.get(), 0);
    assert!(CD.cd_dev(0).is_none());
    assert!(CD.cd_dev(-1).is_none());
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/device.h");
    let ours: &[(&str, i64)] = &[
        ("DVACT_DEACTIVATE", DVACT_DEACTIVATE.into()),
        ("DVACT_QUIESCE", DVACT_QUIESCE.into()),
        ("DVACT_SUSPEND", DVACT_SUSPEND.into()),
        ("DVACT_RESUME", DVACT_RESUME.into()),
        ("DVACT_WAKEUP", DVACT_WAKEUP.into()),
        ("DVACT_POWERDOWN", DVACT_POWERDOWN.into()),
        ("DVF_ACTIVE", DVF_ACTIVE.into()),
        ("FSTATE_NOTFOUND", FSTATE_NOTFOUND.into()),
        ("FSTATE_FOUND", FSTATE_FOUND.into()),
        ("FSTATE_STAR", FSTATE_STAR.into()),
        ("FSTATE_DNOTFOUND", FSTATE_DNOTFOUND.into()),
        ("FSTATE_DSTAR", FSTATE_DSTAR.into()),
        ("DETACH_FORCE", DETACH_FORCE.into()),
        ("DETACH_QUIET", DETACH_QUIET.into()),
        ("CD_INDIRECT", CD_INDIRECT.into()),
        ("CD_SKIPHIBERNATE", CD_SKIPHIBERNATE.into()),
        ("CD_COCOVM", CD_COCOVM.into()),
        ("QUIET", QUIET.into()),
        ("UNCONF", UNCONF.into()),
        ("UNSUPP", UNSUPP.into()),
        ("SLEEP_RESUME", SLEEP_RESUME.into()),
        ("SLEEP_SUSPEND", SLEEP_SUSPEND.into()),
        ("SLEEP_HIBERNATE", SLEEP_HIBERNATE.into()),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}
