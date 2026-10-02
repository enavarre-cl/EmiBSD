//! Tests of `syslimits.rs`; see there.

use super::*;

/// Every constant with its C name, for the reference-backed test.
const TABLE: &[(&str, i64)] = &[
    ("ARG_MAX", ARG_MAX as i64),
    ("CHILD_MAX", CHILD_MAX as i64),
    ("LINK_MAX", LINK_MAX as i64),
    ("MAX_CANON", MAX_CANON as i64),
    ("MAX_INPUT", MAX_INPUT as i64),
    ("NAME_MAX", NAME_MAX as i64),
    ("NGROUPS_MAX", NGROUPS_MAX as i64),
    ("OPEN_MAX", OPEN_MAX as i64),
    ("PATH_MAX", PATH_MAX as i64),
    ("PIPE_BUF", PIPE_BUF as i64),
    ("SYMLINK_MAX", SYMLINK_MAX as i64),
    ("SYMLOOP_MAX", SYMLOOP_MAX as i64),
    ("BC_BASE_MAX", BC_BASE_MAX as i64),
    ("BC_DIM_MAX", BC_DIM_MAX as i64),
    ("BC_SCALE_MAX", BC_SCALE_MAX as i64),
    ("BC_STRING_MAX", BC_STRING_MAX as i64),
    ("COLL_WEIGHTS_MAX", COLL_WEIGHTS_MAX as i64),
    ("EXPR_NEST_MAX", EXPR_NEST_MAX as i64),
    ("LINE_MAX", LINE_MAX as i64),
    ("RE_DUP_MAX", RE_DUP_MAX as i64),
    ("SEM_VALUE_MAX", SEM_VALUE_MAX as i64),
    ("IOV_MAX", IOV_MAX as i64),
    ("NZERO", NZERO as i64),
    ("TTY_NAME_MAX", TTY_NAME_MAX as i64),
    ("LOGIN_NAME_MAX", LOGIN_NAME_MAX as i64),
    ("HOST_NAME_MAX", HOST_NAME_MAX as i64),
    ("GETENTROPY_MAX", GETENTROPY_MAX as i64),
    ("_MAXCOMLEN", _MAXCOMLEN as i64),
];

#[test]
fn a_few_values_the_kernel_relies_on() {
    assert_eq!(PATH_MAX, 1024);
    assert_eq!(SYMLINK_MAX, PATH_MAX);
    assert_eq!(_MAXCOMLEN, 24);
    assert_eq!(NGROUPS_MAX, 16);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let mut defs = crate::reftest::defines("sys/sys/syslimits.h");
    // INT_MAX / UINT_MAX come from <sys/limits.h>
    defs.extend(crate::reftest::defines("sys/sys/limits.h"));
    for (name, value) in TABLE {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
    let header = crate::reftest::defines("sys/sys/syslimits.h");
    for name in header.keys() {
        assert!(
            TABLE.iter().any(|(n, _)| *n == name.as_str()),
            "{name} is in syslimits.h but not ported"
        );
    }
}
