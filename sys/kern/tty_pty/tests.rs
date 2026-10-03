//! Host tests for the ptys: a master and its slave passing a line each way through the line
//! discipline, and the device switch finding the slave's major.

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::conf::cdevsw;
use crate::sys::conf::D_TTY;
use crate::sys::proc::{Pgrp, Process};
use crate::sys::uio::{Iovec, UioRw, UioSeg};
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

/// A thread in a process with a group (what the entry points read through `p`).
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

/// Moves `buf` through `f` as a kernel-space uio in direction `rw`; the bytes moved.
fn xfer(
    f: fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>,
    dev: Dev,
    rw: UioRw,
    buf: &mut [u8],
) -> (Result<(), Errno>, usize) {
    let n = buf.len();
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: n,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: n,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_procp: None,
    };
    let r = f(dev, &mut uio, IO_NDELAY);
    (r, n - uio.uio_resid)
}

fn read(f: fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>, dev: Dev) -> Vec<u8> {
    let mut buf = vec![0u8; 64];
    let (r, n) = xfer(f, dev, UioRw::UIO_READ, &mut buf);
    assert_eq!(r, Ok(()));
    buf.truncate(n);
    buf
}

#[test]
fn a_line_each_way_between_master_and_slave() {
    let _g = setup_real_memory();
    let w = world();

    // The host's switch has amd64's slots: pts at 5, ptc at 6, ptm at 81.
    assert_eq!(cdevsw(5).d_type, D_TTY);
    assert!(ptr::fn_addr_eq(cdevsw(6).d_open, ptcopen as DevTypeOpen));
    ptyattach(1);
    assert_eq!(PTS_MAJOR.load(Ordering::Relaxed), 5);
    assert_eq!(pty_getfree(), makedev(5, 0));

    let master = makedev(6, 3);
    let slave = makedev(5, 3);
    assert_eq!(ptcopen(master, FREAD | FWRITE, 0, &w.p), Ok(()));
    assert_eq!(
        ptcopen(master, FREAD | FWRITE, 0, &w.p),
        Err(Errno::EIO),
        "busy"
    );
    assert_eq!(ptsopen(slave, FREAD | FWRITE, 0, &w.p), Ok(()));
    let pti = pt_softc(3).expect("check_pty made the slot");
    assert_eq!(&pti.pty_pn.get(), b"/dev/ptyp3\0");
    assert_eq!(&pti.pty_sn.get(), b"/dev/ttyp3\0");

    // Typed on the master: the slave reads the line, the master reads the echo.
    let mut line = *b"hello\n";
    assert_eq!(
        xfer(ptcwrite, master, UioRw::UIO_WRITE, &mut line),
        (Ok(()), 6)
    );
    assert_eq!(read(ptsread, slave), b"hello\n");
    assert_eq!(read(ptcread, master), b"hello\r\n");

    // Written by the slave: output processing, then the master reads it.
    let mut out = *b"out\n";
    assert_eq!(
        xfer(ptswrite, slave, UioRw::UIO_WRITE, &mut out),
        (Ok(()), 4)
    );
    assert_eq!(read(ptcread, master), b"out\r\n");
    let mut buf = [0u8; 8];
    assert_eq!(
        xfer(ptcread, master, UioRw::UIO_READ, &mut buf).0,
        Err(Errno::EWOULDBLOCK)
    );

    // The slave is a tty: TIOCGETA answers.
    let mut t = [0u8; size_of::<Termios>()];
    assert_eq!(
        ptyioctl(slave, crate::sys::ttycom::TIOCGETA, &mut t, 0, &w.p),
        Ok(())
    );

    assert_eq!(ptsclose(slave, FREAD | FWRITE, 0, Some(&w.p)), Ok(()));
    assert_eq!(ptcclose(master, FREAD | FWRITE, 0, Some(&w.p)), Ok(()));
    assert!(pty_isfree(3));
}
