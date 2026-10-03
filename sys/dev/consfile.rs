//! The console as an open file: a stand-in, not OpenBSD code, until `/dev/console` exists.
//!
//! In OpenBSD the first process gets its descriptors 0, 1 and 2 from `init(8)`, which opens
//! `/dev/console` (`setctty`, `login_tty`): a vnode of the console character device, whose
//! `cdevsw` entry (`cnopen`, `cnread`, `cnwrite`, `cnioctl` in `dev/cons.c`) forwards to the
//! tty of the console driver. None of that is here before the vfs and the tty layer
//! (milestone M10), so `start_init` calls [`consfile_attach`] instead: one `struct file` of
//! type [`DTYPE_CONSFILE`] whose `fileops` talk to the polled console (`cnputc`, `cngetc`),
//! installed at descriptors 0, 1 and 2 of process 1 through the real descriptor table
//! (`falloc`, `fdinsert`, `fdalloc`), so `write(1, ...)` takes the same path as any file.
//!
//! What the stand-in does, compared with a console tty:
//! - write: every byte goes to `cnputc` (which adds the `'\r'` after `'\n'`); no output
//!   processing, no flow control.
//! - read: a minimal canonical mode: polled `cngetc` (inside `cnpollc`), `'\r'` becomes
//!   `'\n'`, each character is echoed, and the read ends at the newline or when the buffer
//!   is full. No erase/kill characters, no signals, no non-blocking reads.
//! - ioctl: `FIONBIO` and `FIOASYNC` are accepted and ignored; everything else (the
//!   `termios` calls among them) is `ENOTTY`, so `isatty(3)` says no.
//! - stat: a character device, mode `0600`, `st_rdev` the console's device number.
//! - kqueue: reported (`kern_event.c`).
//!
//! It goes away when `init` can open `/dev/console`.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::dev::cons::{cn_tab, cngetc, cnpollc, cnputc};
use crate::kern::kern_descrip::{falloc, fdalloc, fdexpand, fdinsert};
use crate::kern::kern_subr::{uiomove, ureadc};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::file::{File, Fileops, fref, frele};
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::filio::{FIOASYNC, FIONBIO};
use crate::sys::proc::Proc;
use crate::sys::stat::{S_IFCHR, S_IRUSR, S_IWUSR, Stat};
use crate::sys::uio::Uio;
use crate::unported;

/// The descriptor type of the console stand-in: outside OpenBSD's `DTYPE_*` range, so no
/// code that switches on a real type mistakes it for one.
pub const DTYPE_CONSFILE: i32 = 127;

/// How much a write moves from user space at a time.
const CONSFILE_CHUNK: usize = 256;

/// The stand-in's operations.
static CONSFILEOPS: Fileops = Fileops {
    fo_read: consfile_read,
    fo_write: consfile_write,
    fo_ioctl: consfile_ioctl,
    fo_kqfilter: consfile_kqfilter,
    fo_stat: consfile_stat,
    fo_close: consfile_close,
    fo_seek: None,
};

/// Reads a line from the console (see the module's description).
fn consfile_read(_fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    if cn_tab().is_none() {
        return Ok(()); // no console: end of file
    }
    cnpollc(true);
    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let mut c = cngetc();
        if c == i32::from(b'\r') {
            c = i32::from(b'\n');
        }
        cnputc(c);
        if let Err(e) = ureadc(c, uio) {
            error = Err(e);
            break;
        }
        if c == i32::from(b'\n') {
            break;
        }
    }
    cnpollc(false);
    error
}

/// Writes the user's bytes to the console.
fn consfile_write(_fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    let mut chunk = [0u8; CONSFILE_CHUNK];
    while uio.uio_resid > 0 {
        let n = uio.uio_resid.min(CONSFILE_CHUNK);
        uiomove(&mut chunk[..n], uio)?;
        for &c in &chunk[..n] {
            cnputc(i32::from(c));
        }
    }
    Ok(())
}

/// `FIONBIO` and `FIOASYNC` are accepted; nothing else.
fn consfile_ioctl(_fp: &File, com: u64, _data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
    match com {
        FIONBIO | FIOASYNC => Ok(()),
        _ => Err(Errno::ENOTTY),
    }
}

/// No kqueue yet.
fn consfile_kqfilter(_fp: &File, _kn: *mut c_void) -> Result<(), Errno> {
    Err(unported!("consfile: kqfilter (kern_event.c)"))
}

/// A character device owned by root, mode 0600.
fn consfile_stat(_fp: &File, ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    *ub = Stat {
        st_mode: S_IFCHR | S_IRUSR | S_IWUSR,
        st_rdev: cn_tab().map_or(0, |cp| cp.cn_dev.get()),
        st_nlink: 1,
        ..Stat::default()
    };
    Ok(())
}

/// Nothing to release.
fn consfile_close(_fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// Installs the console stand-in at descriptors 0, 1 and 2 of `p`'s (empty) table: one file
/// open for reading and writing, as `init(8)`'s `open(_PATH_CONSOLE, O_RDWR)` and
/// `login_tty`'s `dup2`s would leave it.
pub fn consfile_attach(p: &Proc) -> Result<(), Errno> {
    let fdp = p.fd();

    fdplock(fdp);
    let (fp, fd) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdpunlock(fdp);
            return Err(e);
        }
    };
    fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
    fp.f_type.set(DTYPE_CONSFILE);
    fp.f_ops.set(Some(&CONSFILEOPS));
    fdinsert(fdp, fd, 0, fp);

    let mut error = Ok(());
    for want in fd + 1..=2 {
        let new = loop {
            match fdalloc(p, want) {
                Err(Errno::ENOSPC) => {
                    if let Err(e) = fdexpand(p) {
                        break Err(e);
                    }
                }
                r => break r,
            }
        };
        match new {
            Ok(new) => {
                fref(fp);
                fdinsert(fdp, new, 0, fp);
            }
            Err(e) => {
                error = Err(e);
                break;
            }
        }
    }
    fdpunlock(fdp);

    let _ = frele(fp, p);
    error
}
