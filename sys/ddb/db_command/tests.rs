use super::*;
use crate::ddb::db_lex::{db_set_line, db_test_lock};
use crate::ddb::db_output::DB_TAB_STOP_WIDTH;

/// The name of the command `db_cmd_search` found for `name` in `table`, with its kind.
fn search(name: &str, table: &'static [DbCommand]) -> (&'static str, &'static str) {
    match db_cmd_search(name.as_bytes(), table) {
        CmdSearch::Unique(c) => ("unique", c.name),
        CmdSearch::Found(c) => ("found", c.name),
        CmdSearch::None => ("none", ""),
        CmdSearch::Ambiguous => ("ambiguous", ""),
    }
}

#[test]
fn search_prefixes() {
    // a whole name wins even when longer names start with it
    assert_eq!(search("c", DB_COMMAND_TABLE), ("unique", "c"));
    assert_eq!(search("continue", DB_COMMAND_TABLE), ("unique", "continue"));
    // a unique prefix
    assert_eq!(search("cont", DB_COMMAND_TABLE), ("found", "continue"));
    assert_eq!(search("he", DB_COMMAND_TABLE), ("found", "help"));
    assert_eq!(search("reg", DB_SHOW_CMDS), ("found", "registers"));
    // two prefixes in a row are ambiguous ...
    assert_eq!(search("tr", DB_COMMAND_TABLE), ("found", "trace"));
    assert_eq!(search("wa", DB_COMMAND_TABLE), ("found", "watch"));
    assert_eq!(search("st", DB_COMMAND_TABLE), ("ambiguous", ""));
    // ... and, as in C, a third one makes it "found" again (break, bt, boot: boot)
    assert_eq!(search("b", DB_COMMAND_TABLE), ("found", "boot"));
    // not found
    assert_eq!(search("frobnicate", DB_COMMAND_TABLE), ("none", ""));
    assert_eq!(search("machine", DB_COMMAND_TABLE), ("unique", "machine"));
}

#[test]
fn run_commands() {
    let _g = db_test_lock();
    let mut last = None;

    // `set` runs with its own syntax
    db_set_line(b"set $tabstops = 4\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 4);
    assert_eq!(last.map(|c| c.name), Some("set"));
    db_set_line(b"set $tabstops 8\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 8);

    // the standard syntax sets dot and the last address; errors come back as DbError
    db_set_line(b"help 0x1234\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    assert_eq!(DB_DOT.load(Ordering::Relaxed), 0x1234);
    assert_eq!(DB_LAST_ADDR.load(Ordering::Relaxed), 0x1234);
    db_set_line(b"help 4/0\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Err(DbError));
    db_set_line(b"write/z 1 2\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Err(DbError));

    // unknown, ambiguous and bad lines print and return
    for line in [
        &b"frobnicate\n"[..],
        b"st\n",
        b"5\n",
        b"show\n",
        b"help/5\n",
    ] {
        db_set_line(line);
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    }

    // continue ends the loop; an empty line repeats it
    DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
    db_set_line(b"c\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    assert!(DB_CMD_LOOP_DONE.load(Ordering::Relaxed));
    DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
    db_set_line(b"\n");
    assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
    assert!(DB_CMD_LOOP_DONE.load(Ordering::Relaxed));
    DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ddb/db_command.h");
    for (name, value) in [
        ("CS_OWN", CS_OWN),
        ("CS_MORE", CS_MORE),
        ("CS_SET_DOT", CS_SET_DOT),
    ] {
        assert_eq!(
            crate::reftest::int(&defs, name),
            Some(i64::from(value)),
            "{name}"
        );
    }
}
