use super::*;
use std::format;
use std::string::ToString;

fn cstr(buf: &[u8]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..end]).unwrap()
}

#[test]
fn snprintf_truncates_and_reports_the_full_length() {
    let mut buf = [0xaau8; 8];
    assert_eq!(snprintf(&mut buf, format_args!("{}", "0123456789")), 10);
    assert_eq!(cstr(&buf), "0123456");
    assert_eq!(buf[7], 0);

    let mut buf = [0xaau8; 8];
    assert_eq!(snprintf(&mut buf, format_args!("ab")), 2);
    assert_eq!(cstr(&buf), "ab");

    let mut empty: [u8; 0] = [];
    assert_eq!(snprintf(&mut empty, format_args!("xyz")), 3);

    let mut one = [0xaau8; 1];
    assert_eq!(vsnprintf(&mut one, format_args!("xyz")), 3);
    assert_eq!(one, [0]);
}

#[test]
fn kprintf_to_buffer_without_count_stops_at_the_terminator_slot() {
    let mut buf = [0u8; 4];
    let n = kprintf(format_args!("abcdef"), TOBUFONLY, Some(&mut buf));
    assert_eq!(n, 4, "a, b, c and the one that overflowed");
    assert_eq!(&buf[..3], b"abc");
    assert_eq!(buf[3], 0);
}

#[test]
fn kprintf_without_a_sink_counts() {
    assert_eq!(kprintf(format_args!("{:>5}", 42), TOLOG, None), 5);
}

#[test]
fn bitmask_follows_the_c_descriptor_format() {
    let desc = b"\x10\x01READ\x02WRITE\x03EXEC";
    assert_eq!(Bitmask(0x5, desc).to_string(), "5<READ,EXEC>");
    assert_eq!(Bitmask(0x0, desc).to_string(), "0");
    assert_eq!(Bitmask(0x8, desc).to_string(), "8");
    assert_eq!(Bitmask(0xff, b"\x0a\x01A\x08H").to_string(), "255<A,H>");
    assert_eq!(Bitmask(0x7, b"\x08\x81A").to_string(), "7<A>");
    assert_eq!(
        Bitmask(0x7, b"\x07").to_string(),
        "",
        "unknown base prints nothing"
    );
    assert_eq!(Bitmask(0x7, b"").to_string(), "");
}

#[test]
fn str_stops_at_nul() {
    assert_eq!(Str(b"abc\0def").to_string(), "abc");
    assert_eq!(Str(b"no nul").to_string(), "no nul");
    assert_eq!(Str(b"\xff!").to_string(), "?!");
    assert_eq!(format!("{}", Str(&[b'x'])), "x");
}

#[test]
fn constants_match_the_c() {
    assert_eq!(KPRINTF_BUFSIZE, 23);
    assert_eq!(PRINTF_FLAGS.load(Ordering::Relaxed), TOCONS | TOLOG);
    assert!(!panicstr());
}

#[test]
fn logpri_writes_the_level_to_the_log() {
    crate::kern::subr_log::init_static_msgbuf();
    let mbp = msgbufp().unwrap();
    // The message buffer is global: a test printing on another thread can interleave its
    // bytes. Retry until a window holds only ours; the expectation itself stays exact.
    let mut text = std::vec::Vec::new();
    for _ in 0..100 {
        let before = mbp.bufx();
        logpri(LOG_ERR);
        let after = mbp.bufx();
        text = (before..after)
            .map(|i| mbp.bufc()[i as usize].get())
            .collect();
        if text.len() == 3 {
            break;
        }
    }
    assert_eq!(text, b"<3>");
    let n = kprintf!("{} {}", "hello", 7);
    assert_eq!(n, 7);
    kprintln!("line");
    kprintln!();
    log!(LOG_ERR, "logged {}", 1);
    addlog(format_args!(" more\n"));
    tablefull("test");
    puts(b"puts");
    assert_eq!(putchar(i32::from(b'z')), i32::from(b'z'));
    assert!(db_printf!("db {}", 1) == 4);
    assert_eq!(vprintf(format_args!("v")), 1);
    kassert!(true);
    kdassert!(true);
}
