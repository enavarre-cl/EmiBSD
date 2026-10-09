/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! Host-test helpers (not an OpenBSD file): a RAM disk device over the test images in
//! `testdata/`, the [`SaConf`] the tests register, and a console that records what libsa
//! prints. Tests that use libsa's global state (the open file table, the console) hold
//! [`setup`]'s guard, so they run one at a time.

extern crate std;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::cd9660::*;
use crate::hdr::cons::{CN_MIDPRI, ConsDev};
use crate::hdr::types::Daddr;
use crate::saerrno::Errno;
use crate::stand::{Devsw, F_READ, FsOps, OpenFile, SaConf, sa_conf_register};
use crate::ufs::*;
use crate::ufs2::*;

/// Serialises the tests that use libsa's globals.
static LOCK: Mutex<()> = Mutex::new(());

/// The images the RAM disk serves, by device name (`ffs1:`, `ffs2:`, `cd:`, `elf:`).
static IMAGES: Mutex<Vec<(&'static str, &'static [u8], bool)>> = Mutex::new(Vec::new());

/// What the test console printed.
pub static OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The base of the test's load area (`LOADADDR` adds it).
pub static LOADBASE: AtomicU64 = AtomicU64::new(0);

/// A RAM disk: `f_devdata` points at one (leaked).
struct RamDisk {
    data: &'static [u8],
    /// A file of its own ([`add_file`]), which `memfs` serves, not a disk.
    file: bool,
}

/// `memfs`'s open file: the offset.
struct MemFile {
    off: i64,
}

fn ram_strategy(
    devdata: *mut c_void,
    rw: i32,
    blk: Daddr,
    buf: &mut [u8],
    rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    // SAFETY: `ram_open` set `devdata` to a leaked `RamDisk`.
    let disk = unsafe { &*devdata.cast::<RamDisk>() };
    if rw != F_READ {
        return Err(Errno::EROFS);
    }
    let off = (blk as usize * 512).min(disk.data.len());
    let n = buf.len().min(disk.data.len() - off);
    buf[..n].copy_from_slice(&disk.data[off..off + n]);
    if let Some(r) = rsize {
        *r = n;
    }
    Ok(())
}

fn ram_open(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let Some(colon) = file.iter().position(|&c| c == b':') else {
        return Err(Errno::ENXIO);
    };
    let name = &file[..colon];
    let images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(&(_, data, whole)) = images.iter().find(|(n, ..)| n.as_bytes() == name) else {
        return Err(Errno::ENXIO);
    };
    f.f_devdata = Box::into_raw(Box::new(RamDisk { data, file: whole })).cast();
    *file = &file[colon + 1..];
    Ok(())
}

fn ram_close(f: &mut OpenFile) -> Result<(), Errno> {
    if !f.f_devdata.is_null() {
        // SAFETY: `ram_open` leaked this box; the slot gives it up here.
        drop(unsafe { Box::from_raw(f.f_devdata.cast::<RamDisk>()) });
        f.f_devdata = core::ptr::null_mut();
    }
    Ok(())
}

fn ram_ioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Ok(())
}

static DEVSW: [Devsw; 1] = [Devsw {
    dv_name: "RAM",
    dv_strategy: ram_strategy,
    dv_open: ram_open,
    dv_close: ram_close,
    dv_ioctl: ram_ioctl,
}];

static FILE_SYSTEM: [FsOps; 4] = [
    FsOps {
        open: ufs_open,
        close: ufs_close,
        read: ufs_read,
        write: ufs_write,
        seek: ufs_seek,
        stat: ufs_stat,
        readdir: ufs_readdir,
        fchmod: Some(ufs_fchmod),
    },
    FsOps {
        open: ufs2_open,
        close: ufs2_close,
        read: ufs2_read,
        write: ufs2_write,
        seek: ufs2_seek,
        stat: ufs2_stat,
        readdir: ufs2_readdir,
        fchmod: Some(ufs2_fchmod),
    },
    FsOps {
        open: cd9660_open,
        close: cd9660_close,
        read: cd9660_read,
        write: cd9660_write,
        seek: cd9660_seek,
        stat: cd9660_stat,
        readdir: cd9660_readdir,
        fchmod: None,
    },
    FsOps {
        open: mem_open,
        close: mem_close,
        read: mem_read,
        write: mem_write,
        seek: mem_seek,
        stat: mem_stat,
        readdir: mem_readdir,
        fchmod: None,
    },
];

pub(crate) static CONSTAB: [ConsDev; 1] = [ConsDev::new(
    |cp| cp.set_pri(CN_MIDPRI),
    |_| {},
    |_| 0,
    |_, c| {
        OUTPUT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(c as u8)
    },
)];

/// `devopen`: the first device that claims the name.
fn devopen<'a>(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno> {
    let mut file = fname;
    for dv in &DEVSW {
        if (dv.dv_open)(f, &mut file).is_ok() {
            f.f_dev = Some(dv);
            return Ok(file);
        }
    }
    Err(Errno::ENXIO)
}

static CONF: SaConf = SaConf {
    file_system: &FILE_SYSTEM,
    devsw: &DEVSW,
    constab: &CONSTAB,
    devopen,
    rtt: || panic!("_rtt"),
    loadaddr: |a, offset| {
        ((a.wrapping_add(offset)) & 0xfff_ffff) + LOADBASE.load(Ordering::Relaxed)
    },
    netif_drivers: &[],
    getsecs: || 0,
};

/// Registers the test configuration and takes the lock on libsa's globals.
pub fn setup() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    sa_conf_register(&CONF);
    crate::printf::DONOTTWIDDLE.store(true, Ordering::Relaxed);
    OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    guard
}

/// Serves `data` as the device `name:`.
pub fn add_image(name: &'static str, data: &'static [u8]) {
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    images.retain(|(n, ..)| *n != name);
    images.push((name, data, false));
}

/// Serves `data` as the one file of the device `name:` (any path opens it).
pub fn add_file(name: &'static str, data: &'static [u8]) {
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    images.retain(|(n, ..)| *n != name);
    images.push((name, data, true));
}

/// The RAM disk of an open file.
fn disk(f: &OpenFile) -> &'static RamDisk {
    // SAFETY: `ram_open` set `f_devdata` to a leaked `RamDisk`, freed only by `ram_close`.
    unsafe { &*f.f_devdata.cast::<RamDisk>() }
}

fn mem_open(_path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_devdata.is_null() || !disk(f).file {
        return Err(Errno::EINVAL);
    }
    f.f_fsdata = Some(Box::new(MemFile { off: 0 }));
    Ok(())
}

fn mem_close(f: &mut OpenFile) -> Result<(), Errno> {
    f.f_fsdata = None;
    Ok(())
}

fn mem_read(f: &mut OpenFile, buf: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    let data = disk(f).data;
    let mf = f.fsdata::<MemFile>().ok_or(Errno::EBADF)?;
    let off = (mf.off as usize).min(data.len());
    let n = buf.len().min(data.len() - off);
    buf[..n].copy_from_slice(&data[off..off + n]);
    mf.off += n as i64;
    *resid = buf.len() - n;
    Ok(())
}

fn mem_write(_f: &mut OpenFile, _buf: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

fn mem_seek(f: &mut OpenFile, offset: i64, whence: i32) -> Result<i64, Errno> {
    let mf = f.fsdata::<MemFile>().ok_or(Errno::EBADF)?;
    match whence {
        crate::stand::SEEK_SET => mf.off = offset,
        crate::stand::SEEK_CUR => mf.off += offset,
        _ => return Err(Errno::EINVAL),
    }
    Ok(mf.off)
}

fn mem_stat(f: &mut OpenFile, sb: &mut crate::hdr::stat::Stat) -> Result<(), Errno> {
    sb.st_mode = crate::hdr::stat::S_IFREG | 0o644;
    sb.st_size = disk(f).data.len() as i64;
    Ok(())
}

fn mem_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::ENOTDIR)
}

/// The zlib-compressed fixture `testdata/<file>`, inflated, served as `name:`.
pub fn add_fixture(name: &'static str, file: &str) {
    let path = std::format!("{}/testdata/{file}", env!("CARGO_MANIFEST_DIR"));
    let z = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out = vec![0u8; 4 << 20];
    let mut strm = libz::ZStream::new();
    assert_eq!(libz::inflateInit(&mut strm), libz::Z_OK);
    strm.next_in = &z;
    strm.next_out = &mut out;
    assert_eq!(libz::inflate(&mut strm, libz::Z_FINISH), libz::Z_STREAM_END);
    let n = strm.total_out as usize;
    libz::inflateEnd(&mut strm);
    out.truncate(n);
    add_image(name, Box::leak(out.into_boxed_slice()));
}

/// What the console printed since [`setup`], as text.
pub fn output() -> std::string::String {
    let out = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    std::string::String::from_utf8_lossy(&out).into_owned()
}
/* </CODE> */
