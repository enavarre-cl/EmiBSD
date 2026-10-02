//! Tests of `errno.rs`; see there.

use super::*;

#[test]
fn values_and_aliases() {
    assert_eq!(Errno::EPERM.as_i32(), 1);
    assert_eq!(Errno::EPROTO.as_i32(), Errno::ELAST);
    assert_eq!(Errno::EWOULDBLOCK, Errno::EAGAIN);
    assert_eq!(Errno::ERESTART.as_i32(), -1);
    assert_eq!(Errno::EJUSTRETURN.as_i32(), -2);
}

#[test]
fn from_raw_round_trips_every_variant() {
    for (name, value) in Errno::TABLE {
        let e = Errno::from_raw(*value).unwrap_or_else(|| panic!("{name} not mapped"));
        assert_eq!(e.as_i32(), *value, "{name}");
    }
    assert_eq!(Errno::from_raw(0), None);
    assert_eq!(Errno::from_raw(96), None);
    assert_eq!(Errno::from_raw(-3), None);
}

#[test]
fn errno_fits_in_an_i32_and_is_not_zero() {
    assert_eq!(core::mem::size_of::<Errno>(), 4);
    assert!(Errno::TABLE.iter().all(|(_, v)| *v != 0));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/errno.h");
    for (name, value) in Errno::TABLE {
        assert_eq!(
            crate::reftest::int(&defs, name),
            Some(i64::from(*value)),
            "{name}"
        );
    }
    for name in defs.keys().filter(|n| n.starts_with('E')) {
        let header = crate::reftest::int(&defs, name);
        match name.as_str() {
            "EWOULDBLOCK" => {
                assert_eq!(header, Some(i64::from(Errno::EWOULDBLOCK.as_i32())));
            }
            "ELAST" => assert_eq!(header, Some(i64::from(Errno::ELAST))),
            _ => assert!(
                Errno::TABLE.iter().any(|(n, _)| *n == name.as_str()),
                "{name} is in errno.h but not ported"
            ),
        }
    }
}
