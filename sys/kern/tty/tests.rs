//! Host tests for the line discipline: canonical input (erase, kill, end of file), signals,
//! raw input, output processing, and the termios ioctls, on a tty with no driver behind it
//! (`t_oproc` NULL, `t_dev` the console's major, whose `d_stop` does nothing).

use super::*;
use crate::kern::tty_subr::qmem;
use crate::sys::proc::{Pgrp, Process};
use crate::sys::termios::{OPOST, VEOF, VINTR, VKILL};
use crate::sys::ttydefaults::{TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG};
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, UioRw, UioSeg};
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

/// Gives `q` a leaked ring of `size` characters (what `clalloc` does with `malloc`).
fn setup_q(q: &Clist, size: usize, quot: bool) {
    let cs: &'static mut [u8] = vec![0u8; size].leak();
    q.c_cs.set(cs.as_mut_ptr());
    if quot {
        let cq: &'static mut [u8] = vec![0u8; qmem(size)].leak();
        q.c_cq.set(cq.as_mut_ptr());
    }
    q.c_cn.set(size as i32);
}

/// A tty set up as a driver's open would: default termios, open, carrier on.
fn test_tty() -> &'static Tty {
    let tp: &'static Tty = Box::leak(Box::new(Tty::new()));
    tp.t_qlen.set(1024);
    setup_q(&tp.t_rawq, 1024, true);
    setup_q(&tp.t_canq, 1024, true);
    setup_q(&tp.t_outq, 1024, false);
    tp.t_dev.set(makedev(0, 0));
    ttychars(tp);
    tp.set_t_iflag(TTYDEF_IFLAG);
    tp.set_t_oflag(TTYDEF_OFLAG);
    tp.set_t_lflag(TTYDEF_LFLAG);
    tp.set_t_cflag(TTYDEF_CFLAG);
    tp.set_t_ispeed(9600);
    tp.set_t_ospeed(9600);
    ttsetwater(tp);
    tp.t_state_set(TS_ISOPEN | TS_CARR_ON);
    tp
}

fn type_in(tp: &Tty, s: &[u8]) {
    for &c in s {
        ttyinput(i32::from(c), tp);
    }
}

fn drain(q: &Clist) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let c = getc(q);
        if c < 0 {
            return out;
        }
        out.push(c as u8);
    }
}

/// A process with a group, for the ioctls (`ttioctl` reads `p->p_p`).
struct World {
    pr: Process,
    p: Proc,
    pg: Pgrp,
}

fn world() -> Box<World> {
    let w = Box::new(World {
        pr: Process::new(),
        p: Proc::new(),
        pg: Pgrp::new(),
    });
    w.pr.ps_pgrp.set(&w.pg);
    w.p.p_p.set(&w.pr);
    w
}

/// Reads up to `n` bytes through `ttread`, without waiting.
fn read(tp: &Tty, n: usize) -> (Result<(), Errno>, Vec<u8>) {
    let mut buf = vec![0u8; n];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: n,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: n,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let r = ttread(tp, &mut uio, IO_NDELAY);
    let got = n - uio.uio_resid;
    buf.truncate(got);
    (r, buf)
}

#[test]
fn canonical_input_erases_and_breaks_lines() {
    let tp = test_tty();
    type_in(tp, b"abc\x7fd\r");
    // ICRNL made the carriage return a newline, which moved the line to the canonical queue.
    assert_eq!(tp.t_rawq.c_cc.get(), 0);
    assert_eq!(drain(&tp.t_canq), b"abd\n");
    // Echo: the characters, the ECHOE rubout of 'c', and ONLCR's "\r\n".
    assert_eq!(drain(&tp.t_outq), b"abc\x08 \x08d\r\n");
}

#[test]
fn kill_rubs_out_the_whole_line() {
    let tp = test_tty();
    type_in(tp, b"xyz");
    ttyinput(i32::from(tp.t_cc(VKILL)), tp);
    assert_eq!(tp.t_rawq.c_cc.get(), 0);
    assert_eq!(tp.t_canq.c_cc.get(), 0);
    assert_eq!(drain(&tp.t_outq), b"xyz\x08 \x08\x08 \x08\x08 \x08");
}

#[test]
fn end_of_file_ends_a_read_without_being_read() {
    let tp = test_tty();
    type_in(tp, b"ab");
    ttyinput(i32::from(tp.t_cc(VEOF)), tp);
    // ^D echoes as "^D" and backs over it.
    assert_eq!(drain(&tp.t_outq), b"ab^D\x08\x08");
    let (r, got) = read(tp, 16);
    assert_eq!(r, Ok(()));
    assert_eq!(got, b"ab");
    // A ^D alone is a read of 0 bytes: end of file.
    ttyinput(i32::from(tp.t_cc(VEOF)), tp);
    let (r, got) = read(tp, 16);
    assert_eq!(r, Ok(()));
    assert!(got.is_empty());
    // Nothing left: a non-blocking read would block.
    let (r, _) = read(tp, 16);
    assert_eq!(r, Err(Errno::EWOULDBLOCK));
}

#[test]
fn a_read_stops_at_the_end_of_the_line() {
    let tp = test_tty();
    type_in(tp, b"one\ntwo\n");
    assert_eq!(read(tp, 64).1, b"one\n");
    assert_eq!(read(tp, 2).1, b"tw");
    assert_eq!(read(tp, 64).1, b"o\n");
}

#[test]
fn interrupt_flushes_and_echoes() {
    let tp = test_tty();
    type_in(tp, b"partial");
    let _ = drain(&tp.t_outq);
    ttyinput(i32::from(tp.t_cc(VINTR)), tp);
    assert_eq!(tp.t_rawq.c_cc.get(), 0);
    assert_eq!(drain(&tp.t_outq), b"^C");
}

#[test]
fn raw_mode_hands_out_characters_at_once() {
    let tp = test_tty();
    tp.t_lflag_clr(ICANON | ECHO);
    type_in(tp, b"q");
    assert_eq!(tp.t_canq.c_cc.get(), 0);
    assert_eq!(tp.t_rawq.c_cc.get(), 1);
    assert_eq!(tp.t_outq.c_cc.get(), 0, "no echo");
    let w = world();
    let mut n = [0u8; 4];
    assert_eq!(ttioctl(tp, FIONREAD, &mut n, 0, &w.p), Ok(true));
    assert_eq!(i32::from_ne_bytes(n), 1);
    assert_eq!(read(tp, 8).1, b"q");
}

#[test]
fn literal_next_quotes_the_erase_character() {
    let tp = test_tty();
    type_in(tp, b"a\x16\x7f\n");
    assert_eq!(drain(&tp.t_canq), b"a\x7f\n");
}

#[test]
fn output_processing_and_tabs() {
    let tp = test_tty();
    let mut msg = *b"hi\tx\n";
    let mut iov = [Iovec {
        iov_base: msg.as_mut_ptr().cast(),
        iov_len: msg.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: msg.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    assert_eq!(ttwrite(tp, &mut uio, 0), Ok(()));
    assert_eq!(uio.uio_resid, 0);
    assert_eq!(drain(&tp.t_outq), b"hi\tx\r\n");
    assert_eq!(tp.t_column.get(), 0);
    // With OXTABS the tab becomes spaces to the next stop.
    tp.set_t_oflag(tp.t_oflag() | OXTABS);
    assert_eq!(ttyoutput(i32::from(b'\t'), tp), -1);
    assert_eq!(drain(&tp.t_outq), b"        ");
    // Without OPOST nothing is translated.
    tp.set_t_oflag(tp.t_oflag() & !OPOST);
    assert_eq!(ttyoutput(i32::from(b'\n'), tp), -1);
    assert_eq!(drain(&tp.t_outq), b"\n");
}

#[test]
fn termios_and_window_size_ioctls() {
    let tp = test_tty();
    let w = world();
    let mut t = [0u8; size_of::<Termios>()];
    assert_eq!(ttioctl(tp, TIOCGETA, &mut t, 0, &w.p), Ok(true));
    let mut termios: Termios = ioctl_arg(&t);
    assert_eq!(termios.c_lflag, TTYDEF_LFLAG);
    termios.c_lflag &= !ECHO;
    termios.c_cc[VERASE as usize] = 0x08;
    ioctl_ret(&mut t, &termios);
    assert_eq!(ttioctl(tp, TIOCSETA, &mut t, 0, &w.p), Ok(true));
    assert_eq!(tp.t_lflag() & ECHO, 0);
    assert_eq!(tp.t_cc(VERASE), 0x08);
    type_in(tp, b"ab\x08c\n");
    assert_eq!(drain(&tp.t_canq), b"ac\n");
    assert_eq!(tp.t_outq.c_cc.get(), 0, "ECHO is off");

    let ws = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let mut buf = [0u8; size_of::<Winsize>()];
    ioctl_ret(&mut buf, &ws);
    assert_eq!(ttioctl(tp, TIOCSWINSZ, &mut buf, 0, &w.p), Ok(true));
    let mut out = [0u8; size_of::<Winsize>()];
    assert_eq!(ttioctl(tp, TIOCGWINSZ, &mut out, 0, &w.p), Ok(true));
    assert_eq!(ioctl_arg::<Winsize>(&out), ws);

    // Not a tty ioctl.
    let mut pg = [0u8; 4];
    assert_eq!(ttioctl(tp, 0x2000_7400, &mut pg, 0, &w.p), Ok(false));
}

#[test]
fn speed_tables_and_water_marks() {
    let table = [
        Speedtab {
            sp_speed: 9600,
            sp_code: 12,
        },
        Speedtab {
            sp_speed: 115200,
            sp_code: 1,
        },
        Speedtab {
            sp_speed: -1,
            sp_code: -1,
        },
    ];
    assert_eq!(ttspeedtab(115200, &table), 1);
    assert_eq!(ttspeedtab(300, &table), -1);

    let tp = test_tty();
    // 9600 bps: 960 cps; low water 480 clamped to 256, high 256 + 960 clamped to 1024 - 200.
    assert_eq!(tp.t_lowat.get(), 256);
    assert_eq!(tp.t_hiwat.get(), 824);
}

#[test]
fn char_type_classes() {
    assert_eq!(cclass(i32::from(b'\n')), NEWLINE);
    assert_eq!(cclass(i32::from(b'\t')), TAB);
    assert_eq!(cclass(i32::from(b'\r')), RETURN);
    assert_eq!(cclass(0x08), BACKSPACE);
    assert_eq!(cclass(i32::from(b'a')), ORDINARY);
    assert_eq!(cclass(0x7f), CONTROL);
    assert_ne!(isalpha(i32::from(b'_')), 0);
    assert_eq!(isalpha(i32::from(b'-')), 0);
    assert_eq!(CHAR_TYPE[0x80], ORDINARY | ALPHA);
}
