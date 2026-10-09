/* <LICENSES> */
/*-
 * Copyright (c) 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)cd9660_rrip.h	8.2 (Berkeley) 12/5/94
 */
/*-
 * Copyright (c) 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)cd9660_rrip.c	8.6 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! Rock Ridge Interchange Protocol: `<isofs/cd9660/cd9660_rrip.h>` (the System Use Sharing
//! Protocol entries as they are on the disc) and `isofs/cd9660/cd9660_rrip.c` (the walk over
//! a directory record's entries, and the entries' readers: POSIX attributes, device
//! numbers, symbolic links, alternate names, relocated directories, time stamps,
//! continuation areas and the extension reference).
//!
//! Upstream: sys/isofs/cd9660/cd9660_rrip.h @ 3ce1f3f79392
//! Upstream: sys/isofs/cd9660/cd9660_rrip.c @ 3ce1f3f79392
//!
//! The entries are `#[repr(C)]` structures of byte arrays (as `iso.rs` has the volume
//! descriptors), viewed over the entry's bytes; `from_bytes` failing is the C's
//! `isonum_711(p->h.length) < sizeof(*p)`, since an entry's slice is exactly its length.
//!
//! ## Deviations
//! - `RRIP_TABLE` is a `static` array without the C's empty sentinel; the `func2` walk stops
//!   at the first entry without one, as the C's `for (...; ptable->func2; ...)` does.
//! - The output name (`ana->outbuf`, `ana->outlen`) is the buffer, position and length of
//!   [`IsoRripAnalyze`] (`iso_rrip.rs`); no byte is written past the buffer, and a write
//!   that would not fit is treated as the C treats a name past `maxlen`.
//! - `cd9660_rrip_loop` reads a record's entries within the record and its buffer, and a
//!   continuation area within the block read for it: the block number, offset and length of
//!   a `CE` entry are unsigned, so a corrupt one cannot point before the buffer, which the
//!   C's `int` fields could. As in C, when a continuation area holds another `CE`, the
//!   buffer of the earlier area is not released.
//! - `hostname` is copied out of `kern_sysctl.rs`'s `HOSTNAME` (`hostnamelen` bytes); the
//!   `VOLROOT` component copies `f_mntonname` out of the mount.

use core::sync::atomic::Ordering;

use crate::isofs::cd9660::cd9660_extern::IsoMnt;
use crate::isofs::cd9660::cd9660_node::{
    IsoNode, cd9660_defattr, cd9660_deftstamp, cd9660_tstamp_conv7, cd9660_tstamp_conv17,
};
use crate::isofs::cd9660::cd9660_util::{isochar, isofntrans};
use crate::isofs::cd9660::iso::{
    Cdino, ISO_DIRECTORY_RECORD_SIZE, IsoDirectoryRecord, from_bytes_impl, isodcl, isonum_711,
    isonum_733,
};
use crate::isofs::cd9660::iso_rrip::{
    ISO_SUSP_ALTNAME, ISO_SUSP_ATTR, ISO_SUSP_CLINK, ISO_SUSP_CONT, ISO_SUSP_DEVICE,
    ISO_SUSP_EXTREF, ISO_SUSP_IDFLAG, ISO_SUSP_PLINK, ISO_SUSP_RELDIR, ISO_SUSP_SLINK,
    ISO_SUSP_STOP, ISO_SUSP_TSTAMP, IsoRripAnalyze,
};
use crate::kern::kern_sysctl::{HOSTNAME, HOSTNAMELEN};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bread, brelse};
use crate::kprintf;
use crate::sys::buf::Buf;
use crate::sys::mount::MNAMELEN;
use crate::sys::param::{DEV_BSHIFT, MAXHOSTNAMELEN, MAXPATHLEN};
use crate::sys::syslimits::NAME_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Daddr, Dev, Off, major, makedev, minor};

/// `ISO_SUSP_HEADER`.
#[repr(C)]
pub struct IsoSuspHeader {
    /// `type`.
    pub type_: [u8; isodcl(0, 1)],
    /// `length`: 711.
    pub length: [u8; isodcl(2, 2)],
    /// `version`.
    pub version: [u8; isodcl(3, 3)],
}

from_bytes_impl!(IsoSuspHeader);

/// `ISO_RRIP_ATTR`.
#[repr(C)]
pub struct IsoRripAttr {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `mode`: 733.
    pub mode: [u8; isodcl(4, 11)],
    /// `links`: 733.
    pub links: [u8; isodcl(12, 19)],
    /// `uid`: 733.
    pub uid: [u8; isodcl(20, 27)],
    /// `gid`: 733.
    pub gid: [u8; isodcl(28, 35)],
}

from_bytes_impl!(IsoRripAttr);

/// `ISO_RRIP_DEVICE`.
#[repr(C)]
pub struct IsoRripDevice {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `dev_t_high`: 733.
    pub dev_t_high: [u8; isodcl(4, 11)],
    /// `dev_t_low`: 733.
    pub dev_t_low: [u8; isodcl(12, 19)],
}

from_bytes_impl!(IsoRripDevice);

/// `ISO_SUSP_CFLAG_CONTINUE`.
pub const ISO_SUSP_CFLAG_CONTINUE: u8 = 0x01;
/// `ISO_SUSP_CFLAG_CURRENT`.
pub const ISO_SUSP_CFLAG_CURRENT: u8 = 0x02;
/// `ISO_SUSP_CFLAG_PARENT`.
pub const ISO_SUSP_CFLAG_PARENT: u8 = 0x04;
/// `ISO_SUSP_CFLAG_ROOT`.
pub const ISO_SUSP_CFLAG_ROOT: u8 = 0x08;
/// `ISO_SUSP_CFLAG_VOLROOT`.
pub const ISO_SUSP_CFLAG_VOLROOT: u8 = 0x10;
/// `ISO_SUSP_CFLAG_HOST`.
pub const ISO_SUSP_CFLAG_HOST: u8 = 0x20;

/// `ISO_RRIP_SLINK_COMPONENT`: one component of a symbolic link (its name follows).
#[repr(C)]
pub struct IsoRripSlinkComponent {
    /// `cflag`.
    pub cflag: [u8; isodcl(1, 1)],
    /// `clen`.
    pub clen: [u8; isodcl(2, 2)],
    /// `name`: XXX.
    pub name: [u8; 1],
}

/// `ISO_RRIP_SLSIZ`: the header of a component.
pub const ISO_RRIP_SLSIZ: usize = 2;

/// `ISO_RRIP_SLINK`.
#[repr(C)]
pub struct IsoRripSlink {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `flags`.
    pub flags: [u8; isodcl(4, 4)],
    /// `component`.
    pub component: [u8; isodcl(5, 5)],
}

from_bytes_impl!(IsoRripSlink);

/// `ISO_RRIP_ALTNAME`.
#[repr(C)]
pub struct IsoRripAltname {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `flags`.
    pub flags: [u8; isodcl(4, 4)],
}

from_bytes_impl!(IsoRripAltname);

/// `ISO_RRIP_CLINK`.
#[repr(C)]
pub struct IsoRripClink {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `dir_loc`: 733.
    pub dir_loc: [u8; isodcl(4, 11)],
}

from_bytes_impl!(IsoRripClink);

/// `ISO_RRIP_PLINK`.
#[repr(C)]
pub struct IsoRripPlink {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `dir_loc`: 733.
    pub dir_loc: [u8; isodcl(4, 11)],
}

from_bytes_impl!(IsoRripPlink);

/// `ISO_RRIP_RELDIR`.
#[repr(C)]
pub struct IsoRripReldir {
    /// `h`.
    pub h: IsoSuspHeader,
}

from_bytes_impl!(IsoRripReldir);

/// `ISO_SUSP_TSTAMP_FORM17`.
pub const ISO_SUSP_TSTAMP_FORM17: u8 = 0x80;
/// `ISO_SUSP_TSTAMP_FORM7`.
pub const ISO_SUSP_TSTAMP_FORM7: u8 = 0x00;
/// `ISO_SUSP_TSTAMP_CREAT`.
pub const ISO_SUSP_TSTAMP_CREAT: u8 = 0x01;
/// `ISO_SUSP_TSTAMP_MODIFY`.
pub const ISO_SUSP_TSTAMP_MODIFY: u8 = 0x02;
/// `ISO_SUSP_TSTAMP_ACCESS`.
pub const ISO_SUSP_TSTAMP_ACCESS: u8 = 0x04;
/// `ISO_SUSP_TSTAMP_ATTR`.
pub const ISO_SUSP_TSTAMP_ATTR: u8 = 0x08;
/// `ISO_SUSP_TSTAMP_BACKUP`.
pub const ISO_SUSP_TSTAMP_BACKUP: u8 = 0x10;
/// `ISO_SUSP_TSTAMP_EXPIRE`.
pub const ISO_SUSP_TSTAMP_EXPIRE: u8 = 0x20;
/// `ISO_SUSP_TSTAMP_EFFECT`.
pub const ISO_SUSP_TSTAMP_EFFECT: u8 = 0x40;

/// `ISO_RRIP_TSTAMP`.
#[repr(C)]
pub struct IsoRripTstamp {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `flags`.
    pub flags: [u8; isodcl(4, 4)],
    /// `time`.
    pub time: [u8; isodcl(5, 5)],
}

from_bytes_impl!(IsoRripTstamp);

/// `ISO_RRIP_IDFLAG`.
#[repr(C)]
pub struct IsoRripIdflag {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `flags`.
    pub flags: [u8; isodcl(4, 4)],
}

from_bytes_impl!(IsoRripIdflag);

/// `ISO_RRIP_EXTREF`.
#[repr(C)]
pub struct IsoRripExtref {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `len_id`.
    pub len_id: [u8; isodcl(4, 4)],
    /// `len_des`.
    pub len_des: [u8; isodcl(5, 5)],
    /// `len_src`.
    pub len_src: [u8; isodcl(6, 6)],
    /// `version`.
    pub version: [u8; isodcl(7, 7)],
}

from_bytes_impl!(IsoRripExtref);

/// `ISO_RRIP_OFFSET`.
#[repr(C)]
pub struct IsoRripOffset {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `check`.
    pub check: [u8; isodcl(4, 5)],
    /// `skip`.
    pub skip: [u8; isodcl(6, 6)],
}

from_bytes_impl!(IsoRripOffset);

/// `ISO_RRIP_CONT`.
#[repr(C)]
pub struct IsoRripCont {
    /// `h`.
    pub h: IsoSuspHeader,
    /// `location`.
    pub location: [u8; isodcl(4, 11)],
    /// `offset`.
    pub offset: [u8; isodcl(12, 19)],
    /// `length`.
    pub length: [u8; isodcl(20, 27)],
}

from_bytes_impl!(IsoRripCont);

/// The reader of one entry type: the entry's bytes and the analysis; returns the
/// `ISO_SUSP_*` bit it found, or 0.
type RripFunc = fn(&[u8], &mut IsoRripAnalyze<'_>) -> i32;
/// The default of one entry type, run over the record when no entry of the type was found.
type RripFunc2 = fn(&IsoDirectoryRecord<'_>, &mut IsoRripAnalyze<'_>);

/// `RRIP_TABLE`: one entry type an analysis looks for.
struct RripTable {
    /// `type`.
    type_: [u8; 2],
    /// `func`.
    func: RripFunc,
    /// `func2`.
    func2: Option<RripFunc2>,
    /// `result`.
    result: i32,
}

/// `rrip_table_analyze`: get attributes.
static RRIP_TABLE_ANALYZE: [RripTable; 6] = [
    RripTable {
        type_: *b"PX",
        func: cd9660_rrip_attr,
        func2: Some(cd9660_rrip_defattr),
        result: ISO_SUSP_ATTR,
    },
    RripTable {
        type_: *b"TF",
        func: cd9660_rrip_tstamp,
        func2: Some(cd9660_rrip_deftstamp),
        result: ISO_SUSP_TSTAMP,
    },
    RripTable {
        type_: *b"PN",
        func: cd9660_rrip_device,
        func2: None,
        result: ISO_SUSP_DEVICE,
    },
    RripTable {
        type_: *b"RR",
        func: cd9660_rrip_idflag,
        func2: None,
        result: ISO_SUSP_IDFLAG,
    },
    RripTable {
        type_: *b"CE",
        func: cd9660_rrip_cont,
        func2: None,
        result: ISO_SUSP_CONT,
    },
    RripTable {
        type_: *b"ST",
        func: cd9660_rrip_stop,
        func2: None,
        result: ISO_SUSP_STOP,
    },
];

/// `rrip_table_getname`: get alternate name.
static RRIP_TABLE_GETNAME: [RripTable; 7] = [
    RripTable {
        type_: *b"NM",
        func: cd9660_rrip_altname,
        func2: Some(cd9660_rrip_defname),
        result: ISO_SUSP_ALTNAME,
    },
    RripTable {
        type_: *b"CL",
        func: cd9660_rrip_pclink,
        func2: None,
        result: ISO_SUSP_CLINK | ISO_SUSP_PLINK,
    },
    RripTable {
        type_: *b"PL",
        func: cd9660_rrip_pclink,
        func2: None,
        result: ISO_SUSP_CLINK | ISO_SUSP_PLINK,
    },
    RripTable {
        type_: *b"RE",
        func: cd9660_rrip_reldir,
        func2: None,
        result: ISO_SUSP_RELDIR,
    },
    RripTable {
        type_: *b"RR",
        func: cd9660_rrip_idflag,
        func2: None,
        result: ISO_SUSP_IDFLAG,
    },
    RripTable {
        type_: *b"CE",
        func: cd9660_rrip_cont,
        func2: None,
        result: ISO_SUSP_CONT,
    },
    RripTable {
        type_: *b"ST",
        func: cd9660_rrip_stop,
        func2: None,
        result: ISO_SUSP_STOP,
    },
];

/// `rrip_table_getsymname`: get symbolic link.
static RRIP_TABLE_GETSYMNAME: [RripTable; 4] = [
    RripTable {
        type_: *b"SL",
        func: cd9660_rrip_slink,
        func2: None,
        result: ISO_SUSP_SLINK,
    },
    RripTable {
        type_: *b"RR",
        func: cd9660_rrip_idflag,
        func2: None,
        result: ISO_SUSP_IDFLAG,
    },
    RripTable {
        type_: *b"CE",
        func: cd9660_rrip_cont,
        func2: None,
        result: ISO_SUSP_CONT,
    },
    RripTable {
        type_: *b"ST",
        func: cd9660_rrip_stop,
        func2: None,
        result: ISO_SUSP_STOP,
    },
];

/// `rrip_table_extref`: the extension reference.
static RRIP_TABLE_EXTREF: [RripTable; 3] = [
    RripTable {
        type_: *b"ER",
        func: cd9660_rrip_extref,
        func2: None,
        result: ISO_SUSP_EXTREF,
    },
    RripTable {
        type_: *b"CE",
        func: cd9660_rrip_cont,
        func2: None,
        result: ISO_SUSP_CONT,
    },
    RripTable {
        type_: *b"ST",
        func: cd9660_rrip_stop,
        func2: None,
        result: ISO_SUSP_STOP,
    },
];

/// `ana->inop`, which the attribute analysis sets.
fn inop<'a>(ana: &IsoRripAnalyze<'a>) -> &'a IsoNode {
    match ana.inop {
        Some(ip) => ip,
        None => panic(format_args!("cd9660_rrip: analysis without a node")),
    }
}

/// `ana->outbuf -= *ana->outlen; *ana->outlen = 0`: drop the name gathered so far.
fn reset_out(ana: &mut IsoRripAnalyze<'_>) {
    ana.outpos = ana.outpos.saturating_sub(usize::from(ana.outlen));
    ana.outlen = 0;
}

/// `hostname`, `hostnamelen`.
fn hostname() -> ([u8; MAXHOSTNAMELEN], usize) {
    // SAFETY: `HOSTNAME` is written by `sysctl(2)` under `sysctl_lock` and read without a
    // lock here, as in C; a copy is taken at once.
    let name = unsafe { *HOSTNAME.get() };
    let len = HOSTNAMELEN
        .load(Ordering::Relaxed)
        .clamp(0, MAXHOSTNAMELEN as i32) as usize;
    (name, len)
}

/// `cd9660_rrip_attr`: POSIX file attribute.
fn cd9660_rrip_attr(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let Some(p) = IsoRripAttr::from_bytes(v) else {
        return 0;
    };

    inop(ana).update_inode(|ino| {
        ino.iso_mode = isonum_733(&p.mode) as u16;
        ino.iso_uid = isonum_733(&p.uid);
        ino.iso_gid = isonum_733(&p.gid);
        ino.iso_links = isonum_733(&p.links) as i16;
    });
    ana.fields &= !ISO_SUSP_ATTR;
    ISO_SUSP_ATTR
}

/// `cd9660_rrip_defattr`.
fn cd9660_rrip_defattr(isodir: &IsoDirectoryRecord<'_>, ana: &mut IsoRripAnalyze<'_>) {
    // But this is a required field!
    kprintf!("RRIP without PX field?\n");
    cd9660_defattr(isodir, inop(ana), None);
}

/// `cd9660_rrip_slink`: symbolic links.
fn cd9660_rrip_slink(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    /// The C's `goto invalid`.
    fn invalid(ana: &mut IsoRripAnalyze<'_>) -> i32 {
        ana.cont = 1;
        ana.fields = 0;
        reset_out(ana);
        0
    }

    let Some(p) = IsoRripSlink::from_bytes(v) else {
        return invalid(ana);
    };
    let pcompe = v.len();
    let mut pcomp = core::mem::offset_of!(IsoRripSlink, component);
    let mut len = usize::from(ana.outlen);
    let mut outpos = ana.outpos;
    let mut cont = ana.cont;
    let maxlen = usize::from(ana.maxlen);
    let (mntonname, mntonlen) = ana.imp.im_mountp.mntonname();
    let (host, hostlen) = hostname();

    // Gathering a Symbolic name from each component with path
    while pcomp < pcompe {
        if pcomp + ISO_RRIP_SLSIZ > pcompe {
            return invalid(ana);
        }
        let cflag = v[pcomp];
        let clen = usize::from(v[pcomp + 1]);
        if pcomp + ISO_RRIP_SLSIZ + clen > pcompe {
            return invalid(ana);
        }

        if cont == 0 && len < maxlen {
            let Some(b) = ana.outbuf.get_mut(outpos) else {
                return invalid(ana);
            };
            *b = b'/';
            outpos += 1;
            len += 1;
        }
        cont = 0;

        let mut inbuf: &[u8] = b"..";
        let wlen = match cflag {
            ISO_SUSP_CFLAG_CURRENT => 1, // Inserting Current
            ISO_SUSP_CFLAG_PARENT => 2,  // Inserting Parent
            ISO_SUSP_CFLAG_ROOT => {
                // Inserting slash for ROOT; start over from beginning(?)
                outpos = outpos.saturating_sub(len);
                len = 0;
                0
            }
            ISO_SUSP_CFLAG_VOLROOT => {
                // Inserting a mount point i.e. "/cdrom"; same as above
                outpos = outpos.saturating_sub(len);
                len = 0;
                inbuf = &mntonname[..mntonlen.min(MNAMELEN)];
                inbuf.len()
            }
            ISO_SUSP_CFLAG_HOST => {
                // Inserting hostname i.e. "kurt.tools.de"
                inbuf = &host[..hostlen];
                hostlen
            }
            ISO_SUSP_CFLAG_CONTINUE | 0 => {
                if cflag == ISO_SUSP_CFLAG_CONTINUE {
                    cont = 1;
                }
                // Inserting component
                inbuf = &v[pcomp + ISO_RRIP_SLSIZ..pcomp + ISO_RRIP_SLSIZ + clen];
                clen
            }
            _ => {
                kprintf!("RRIP with incorrect flags?");
                maxlen + 1
            }
        };

        if len + wlen > maxlen {
            // indicate error to caller
            return invalid(ana);
        }
        let Some(dst) = ana.outbuf.get_mut(outpos..outpos + wlen) else {
            return invalid(ana);
        };
        dst.copy_from_slice(&inbuf[..wlen]);
        outpos += wlen;
        len += wlen;

        pcomp += ISO_RRIP_SLSIZ + clen;
    }
    ana.outpos = outpos;
    ana.outlen = len as u16;
    ana.cont = cont;

    if isonum_711(&p.flags) == 0 {
        ana.fields &= !ISO_SUSP_SLINK;
        return ISO_SUSP_SLINK;
    }
    0
}

/// `cd9660_rrip_altname`: alternate name.
fn cd9660_rrip_altname(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    /// The C's `goto invalid`: treat as no name field.
    fn invalid(ana: &mut IsoRripAnalyze<'_>) -> i32 {
        ana.fields &= !ISO_SUSP_ALTNAME;
        reset_out(ana);
        0
    }

    let len = v.len();
    let Some(p) = IsoRripAltname::from_bytes(v) else {
        return invalid(ana);
    };
    let (host, hostlen) = hostname();
    let mut inbuf: &[u8] = b"..";
    let mut cont = 0;

    let wlen = match p.flags[0] {
        ISO_SUSP_CFLAG_CURRENT => 1, // Inserting Current
        ISO_SUSP_CFLAG_PARENT => 2,  // Inserting Parent
        ISO_SUSP_CFLAG_HOST => {
            // Inserting hostname i.e. "kurt.tools.de"
            inbuf = &host[..hostlen];
            hostlen
        }
        f @ (ISO_SUSP_CFLAG_CONTINUE | 0) => {
            if f == ISO_SUSP_CFLAG_CONTINUE {
                cont = 1;
            }
            // Inserting component
            inbuf = &v[size_of::<IsoRripAltname>()..];
            len - size_of::<IsoRripAltname>()
        }
        _ => {
            kprintf!("RRIP with incorrect NM flags?\n");
            usize::from(ana.maxlen) + 1
        }
    };

    let outlen = usize::from(ana.outlen);
    let maxlen = usize::from(ana.maxlen);
    if outlen > maxlen || wlen > maxlen - outlen {
        return invalid(ana);
    }
    let Some(dst) = ana.outbuf.get_mut(ana.outpos..ana.outpos + wlen) else {
        return invalid(ana);
    };
    dst.copy_from_slice(&inbuf[..wlen]);
    ana.outlen += wlen as u16;
    ana.outpos += wlen;

    if cont == 0 {
        ana.fields &= !ISO_SUSP_ALTNAME;
        return ISO_SUSP_ALTNAME;
    }
    0
}

/// `cd9660_rrip_defname`: the ISO 9660 name, when there is no `NM` entry.
fn cd9660_rrip_defname(isodir: &IsoDirectoryRecord<'_>, ana: &mut IsoRripAnalyze<'_>) {
    // strlcpy(ana->outbuf, "..", ana->maxlen - *ana->outlen)
    let size = i32::from(ana.maxlen) - i32::from(ana.outlen);
    if size > 0 {
        let n = (size - 1).min(2) as usize;
        let out = ana.outbuf.get_mut(ana.outpos..).unwrap_or(&mut []);
        for (i, &c) in b".."[..n].iter().chain(b"\0").enumerate() {
            if let Some(b) = out.get_mut(i) {
                *b = c;
            }
        }
    }
    match isodir.name0() {
        0 => ana.outlen = 1,
        1 => ana.outlen = 2,
        _ => {
            let name = isodir.name();
            let namelen = usize::from(isonum_711(isodir.name_len())).min(name.len());
            let out = ana.outbuf.get_mut(ana.outpos..).unwrap_or(&mut []);
            isofntrans(
                &name[..namelen],
                out,
                &mut ana.outlen,
                true,
                isonum_711(isodir.flags()) & 4 != 0,
                ana.imp.joliet_level,
            );
        }
    }
}

/// `cd9660_rrip_pclink`: parent or child link.
fn cd9660_rrip_pclink(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let Some(p) = IsoRripClink::from_bytes(v) else {
        return 0;
    };
    let ino: Cdino = isonum_733(&p.dir_loc) << ana.imp.im_bshift;
    if let Some(inump) = ana.inump.as_deref_mut() {
        *inump = ino;
    }
    ana.fields &= !(ISO_SUSP_CLINK | ISO_SUSP_PLINK);
    if p.h.type_[0] == b'C' {
        ISO_SUSP_CLINK
    } else {
        ISO_SUSP_PLINK
    }
}

/// `cd9660_rrip_reldir`: relocated directory.
fn cd9660_rrip_reldir(_v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    // special hack to make caller aware of RE field
    ana.outlen = 0;
    ana.fields = 0;
    ISO_SUSP_RELDIR | ISO_SUSP_ALTNAME | ISO_SUSP_CLINK | ISO_SUSP_PLINK
}

/// `cd9660_rrip_tstamp`: time stamps. As in C, `need` counts the whole `ISO_RRIP_TSTAMP`
/// (one byte of `time` included) before the stamps, so a `TF` entry must be a byte longer
/// than its stamps to be read.
fn cd9660_rrip_tstamp(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let len = v.len();
    let Some(p) = IsoRripTstamp::from_bytes(v) else {
        return 0;
    };
    let flags = p.flags[0];
    let tlen = if flags & ISO_SUSP_TSTAMP_FORM17 != 0 {
        17
    } else {
        7
    };
    let mut need = size_of::<IsoRripTstamp>();
    for f in [
        ISO_SUSP_TSTAMP_CREAT,
        ISO_SUSP_TSTAMP_MODIFY,
        ISO_SUSP_TSTAMP_ACCESS,
        ISO_SUSP_TSTAMP_ATTR,
    ] {
        if flags & f != 0 {
            need += tlen;
        }
    }
    if need > len {
        return 0;
    }
    let mut ptime = core::mem::offset_of!(IsoRripTstamp, time);
    let conv: fn(&[u8], &mut Timespec) -> bool = if flags & ISO_SUSP_TSTAMP_FORM17 == 0 {
        cd9660_tstamp_conv7
    } else {
        cd9660_tstamp_conv17
    };

    // Check a format of time stamp (7bytes/17bytes)
    inop(ana).update_inode(|ino| {
        if flags & ISO_SUSP_TSTAMP_CREAT != 0 {
            ptime += tlen;
        }

        if flags & ISO_SUSP_TSTAMP_MODIFY != 0 {
            conv(&v[ptime..], &mut ino.iso_mtime);
            ptime += tlen;
        } else {
            ino.iso_mtime = Timespec::new(0, 0);
        }

        if flags & ISO_SUSP_TSTAMP_ACCESS != 0 {
            conv(&v[ptime..], &mut ino.iso_atime);
            ptime += tlen;
        } else {
            ino.iso_atime = ino.iso_mtime;
        }

        if flags & ISO_SUSP_TSTAMP_ATTR != 0 {
            conv(&v[ptime..], &mut ino.iso_ctime);
        } else {
            ino.iso_ctime = ino.iso_mtime;
        }
    });
    ana.fields &= !ISO_SUSP_TSTAMP;
    ISO_SUSP_TSTAMP
}

/// `cd9660_rrip_deftstamp`.
fn cd9660_rrip_deftstamp(isodir: &IsoDirectoryRecord<'_>, ana: &mut IsoRripAnalyze<'_>) {
    cd9660_deftstamp(isodir, inop(ana), None);
}

/// `cd9660_rrip_device`: POSIX device modes.
fn cd9660_rrip_device(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let Some(p) = IsoRripDevice::from_bytes(v) else {
        return 0;
    };
    let high = isonum_733(&p.dev_t_high);
    let low = isonum_733(&p.dev_t_low) as Dev;

    let rdev = if high == 0 {
        makedev(major(low), minor(low))
    } else {
        makedev(high, minor(low))
    };
    inop(ana).update_inode(|ino| ino.iso_rdev = rdev);
    ana.fields &= !ISO_SUSP_DEVICE;
    ISO_SUSP_DEVICE
}

/// `cd9660_rrip_idflag`: flag indicating which fields are recorded.
fn cd9660_rrip_idflag(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let Some(p) = IsoRripIdflag::from_bytes(v) else {
        return 0;
    };
    // don't touch high bits
    ana.fields &= i32::from(isonum_711(&p.flags)) | !0xff;
    // special handling of RE field
    if ana.fields & ISO_SUSP_RELDIR != 0 {
        return cd9660_rrip_reldir(v, ana);
    }

    ISO_SUSP_IDFLAG
}

/// `cd9660_rrip_cont`: continuation pointer.
fn cd9660_rrip_cont(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let Some(p) = IsoRripCont::from_bytes(v) else {
        return 0;
    };
    ana.iso_ce_blk = Daddr::from(isonum_733(&p.location));
    ana.iso_ce_off = Off::from(isonum_733(&p.offset));
    ana.iso_ce_len = isonum_733(&p.length) as i32;
    ISO_SUSP_CONT
}

/// `cd9660_rrip_stop`: System Use end.
fn cd9660_rrip_stop(_v: &[u8], _ana: &mut IsoRripAnalyze<'_>) -> i32 {
    ISO_SUSP_STOP
}

/// `cd9660_rrip_extref`: extension reference.
fn cd9660_rrip_extref(v: &[u8], ana: &mut IsoRripAnalyze<'_>) -> i32 {
    let len = v.len();
    let Some(p) = IsoRripExtref::from_bytes(v) else {
        return 0;
    };
    if isonum_711(&p.version) != 1 {
        return 0;
    }
    let idlen = usize::from(isonum_711(&p.len_id));
    if idlen != 9 && idlen != 10 {
        return 0;
    }
    if len < size_of::<IsoRripExtref>() + idlen {
        return 0;
    }
    let id = &v[8..8 + idlen];
    if idlen == 9 && id != b"IEEE_1282" {
        return 0;
    }
    if idlen == 10 && id != b"IEEE_P1282" && id != b"RRIP_1991A" {
        return 0;
    }
    ana.fields &= !ISO_SUSP_EXTREF;
    ISO_SUSP_EXTREF
}

/// `cd9660_rrip_loop`: walk the SUSP entries of `isodir` (and of its continuation areas),
/// giving each entry `table` knows to its reader while `ana.fields` still wants one; then
/// run the defaults of the types not found. Returns the `ISO_SUSP_*` bits found.
fn cd9660_rrip_loop(
    isodir: &IsoDirectoryRecord<'_>,
    ana: &mut IsoRripAnalyze<'_>,
    table: &[RripTable],
) -> i32 {
    let imp = ana.imp;
    let rec = isodir.bytes();

    // Note: If name length is odd, it will be padded by 1 byte after the name
    let name_len = usize::from(isonum_711(isodir.name_len()));
    let mut pwhead = ISO_DIRECTORY_RECORD_SIZE + name_len;
    if name_len & 1 == 0 {
        pwhead += 1;
    }
    let mut c = 0u8;
    isochar(
        isodir.name(),
        pwhead - ISO_DIRECTORY_RECORD_SIZE,
        imp.joliet_level,
        &mut c,
    );

    // If it's not the '.' entry of the root dir obey SP field
    let skip = if c != 0 || isonum_733(isodir.extent()) != imp.root_extent {
        imp.rr_skip
    } else {
        imp.rr_skip0
    };
    let mut phead = pwhead.saturating_add_signed(skip as isize);
    let mut area: &[u8] = &rec[..usize::from(isonum_711(isodir.length())).min(rec.len())];

    let mut bp: Option<&'static Buf> = None;
    let mut result = 0;
    loop {
        ana.iso_ce_len = 0;
        // Note: "pend" should be more than one SUSP header
        let pend = area.len();
        while phead + size_of::<IsoSuspHeader>() <= pend {
            let len = usize::from(area[phead + 2]);
            if len < size_of::<IsoSuspHeader>() || phead + len > pend {
                break;
            }
            let entry = &area[phead..phead + len];
            if entry[3] == 1 {
                if let Some(t) = table.iter().find(|t| entry[..2] == t.type_) {
                    result |= (t.func)(entry, ana);
                }
                if ana.fields == 0 {
                    break;
                }
            }
            if result & ISO_SUSP_STOP != 0 {
                result &= !ISO_SUSP_STOP;
                break;
            }
            // move to next SUSP; hopefully this works with newer versions, too
            phead += len;
        }

        if ana.fields == 0 || ana.iso_ce_len == 0 {
            break;
        }
        let ce_len = ana.iso_ce_len;
        let ce_off = ana.iso_ce_off;
        if ana.iso_ce_blk >= i64::from(imp.volume_space_size)
            || ce_len < 0
            || ce_off + i64::from(ce_len) > i64::from(imp.logical_block_size)
        {
            // what to do now?
            break;
        }
        // As in C, an earlier continuation buffer is not released (the module's deviations).
        let (b, error) = bread(
            imp.im_devvp,
            ana.iso_ce_blk << (imp.im_bshift - DEV_BSHIFT as i32),
            imp.logical_block_size,
        );
        bp = Some(b);
        if error.is_err() {
            break;
        }
        // SAFETY: the buffer is ours (busy from `bread`) and mapped; it is released only
        // after the walk, and nothing else views it meanwhile.
        let data: &'static [u8] = unsafe { b.data() };
        let Some(ce) = data.get(ce_off as usize..(ce_off + i64::from(ce_len)) as usize) else {
            break;
        };
        area = ce;
        phead = 0;
    }
    if let Some(b) = bp {
        brelse(b);
    }

    // If we don't find the Basic SUSP stuffs, just set default value (attribute/time
    // stamp)
    for t in table.iter() {
        let Some(func2) = t.func2 else {
            break;
        };
        if t.result & result == 0 {
            func2(isodir, ana);
        }
    }

    result
}

/// `cd9660_rrip_analyze`: get attributes (POSIX attributes, time stamps, device number)
/// of the node `inop` from its record.
pub fn cd9660_rrip_analyze(isodir: &IsoDirectoryRecord<'_>, inop: &IsoNode, imp: &IsoMnt) -> i32 {
    let mut analyze = IsoRripAnalyze::new(imp, ISO_SUSP_ATTR | ISO_SUSP_TSTAMP | ISO_SUSP_DEVICE);
    analyze.inop = Some(inop);

    cd9660_rrip_loop(isodir, &mut analyze, &RRIP_TABLE_ANALYZE)
}

/// `cd9660_rrip_getname`: get alternate name into `outbuf` (`NAME_MAX` bytes), its length
/// into `outlen`; a relocated directory's location goes to `inump`.
pub fn cd9660_rrip_getname(
    isodir: &IsoDirectoryRecord<'_>,
    outbuf: &mut [u8],
    outlen: &mut u16,
    inump: &mut Cdino,
    imp: &IsoMnt,
) -> i32 {
    let mut analyze = IsoRripAnalyze::new(
        imp,
        ISO_SUSP_ALTNAME | ISO_SUSP_RELDIR | ISO_SUSP_CLINK | ISO_SUSP_PLINK,
    );
    analyze.outbuf = outbuf;
    analyze.maxlen = NAME_MAX as u16;
    analyze.inump = Some(inump);

    let namelen = usize::from(isonum_711(isodir.name_len()));
    let mut c = 0u8;
    isochar(isodir.name(), namelen, imp.joliet_level, &mut c);
    let mut tab = &RRIP_TABLE_GETNAME[..];
    if c == 0 || c == 1 {
        cd9660_rrip_defname(isodir, &mut analyze);

        analyze.fields &= !ISO_SUSP_ALTNAME;
        tab = &tab[1..];
    }

    let result = cd9660_rrip_loop(isodir, &mut analyze, tab);
    *outlen = analyze.outlen;
    result
}

/// `cd9660_rrip_getsymname`: get symbolic link into `outbuf` (`MAXPATHLEN` bytes), its
/// length into `outlen`; non-zero when a whole link was found.
pub fn cd9660_rrip_getsymname(
    isodir: &IsoDirectoryRecord<'_>,
    outbuf: &mut [u8],
    outlen: &mut u16,
    imp: &IsoMnt,
) -> i32 {
    let mut analyze = IsoRripAnalyze::new(imp, ISO_SUSP_SLINK);
    analyze.outbuf = outbuf;
    analyze.maxlen = MAXPATHLEN as u16;
    analyze.cont = 1; // don't start with a slash

    let result = cd9660_rrip_loop(isodir, &mut analyze, &RRIP_TABLE_GETSYMNAME) & ISO_SUSP_SLINK;
    *outlen = analyze.outlen;
    result
}

/// `cd9660_rrip_offset`: check for Rock Ridge Extension and return offset of its fields
/// (the `SP` entry's skip), -1 when there is none. Note: We insist on the ER field.
pub fn cd9660_rrip_offset(isodir: &IsoDirectoryRecord<'_>, imp: &mut IsoMnt) -> i32 {
    const SP: &[u8; 6] = b"SP\x07\x01\xbe\xef";

    imp.rr_skip0 = 0;
    let rec = isodir.bytes();
    let pend = usize::from(isonum_711(isodir.length())).min(rec.len());
    let mut p = ISO_DIRECTORY_RECORD_SIZE + 1;
    if p + size_of::<IsoRripOffset>() > pend {
        return -1;
    }
    if rec[p..p + 6] != *SP {
        // Maybe, it's a CDROM XA disc?
        imp.rr_skip0 = 15;
        p += 15;
        if p + size_of::<IsoRripOffset>() > pend {
            return -1;
        }
        if rec[p..p + 6] != *SP {
            return -1;
        }
    }

    let mut analyze = IsoRripAnalyze::new(imp, ISO_SUSP_EXTREF);
    if cd9660_rrip_loop(isodir, &mut analyze, &RRIP_TABLE_EXTREF) & ISO_SUSP_EXTREF == 0 {
        return -1;
    }

    i32::from(rec[p + core::mem::offset_of!(IsoRripOffset, skip)])
}

const _: () = {
    assert!(size_of::<IsoSuspHeader>() == 4);
    assert!(size_of::<IsoRripAttr>() == 36);
    assert!(size_of::<IsoRripDevice>() == 20);
    assert!(size_of::<IsoRripSlinkComponent>() == 3);
    assert!(size_of::<IsoRripSlink>() == 6);
    assert!(size_of::<IsoRripAltname>() == 5);
    assert!(size_of::<IsoRripClink>() == 12);
    assert!(size_of::<IsoRripPlink>() == 12);
    assert!(size_of::<IsoRripReldir>() == 4);
    assert!(size_of::<IsoRripTstamp>() == 6);
    assert!(size_of::<IsoRripIdflag>() == 5);
    assert!(size_of::<IsoRripExtref>() == 8);
    assert!(size_of::<IsoRripOffset>() == 7);
    assert!(size_of::<IsoRripCont>() == 28);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the Rock Ridge walk: the extension check of the root, alternate names,
    // symbolic links, attributes, time stamps, device numbers, relocation entries and the SUSP
    // control entries (`RR`, `CE`, `ST`), over the records of a makefs image and over records
    // built here.

    use std::vec::Vec;

    use super::*;
    use crate::isofs::cd9660::cd9660_extern::ISO_FTYPE_RRIP;
    use crate::isofs::cd9660::iso::tests::{image, rec, test_mnt, test_node};
    use crate::sys::stat::{S_IFCHR, S_IFDIR, S_IFLNK, S_IFREG};

    /// A 733 field: the value little-endian, then big-endian.
    fn both32(v: u32) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&v.to_le_bytes());
        b[4..].copy_from_slice(&v.to_be_bytes());
        b
    }

    /// A SUSP entry of version 1.
    fn susp(t: &[u8; 2], body: &[u8]) -> Vec<u8> {
        let mut e = std::vec![t[0], t[1], (4 + body.len()) as u8, 1];
        e.extend_from_slice(body);
        e
    }

    /// A `PX` entry (RRIP 1.10: no file serial number).
    fn px(mode: u32, links: u32, uid: u32, gid: u32) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [mode, links, uid, gid] {
            b.extend_from_slice(&both32(v));
        }
        susp(b"PX", &b)
    }

    /// A directory record at extent 30 named `name`, with the system use entries `entries`.
    fn record(name: &[u8], dir: bool, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut r = std::vec![0u8; ISO_DIRECTORY_RECORD_SIZE];
        r[2..10].copy_from_slice(&both32(30));
        r[18..25].copy_from_slice(&[126, 10, 4, 7, 36, 48, (-12i8) as u8]);
        r[25] = if dir { 2 } else { 0 };
        r[28..32].copy_from_slice(&[1, 0, 0, 1]);
        r[32] = name.len() as u8;
        r.extend_from_slice(name);
        if name.len() % 2 == 0 {
            r.push(0);
        }
        for e in entries {
            r.extend_from_slice(e);
        }
        r[0] = r.len() as u8;
        r
    }

    /// `cd9660_rrip_getname`: the name, its length, the inode number and the result.
    fn getname(r: &[u8], imp: &IsoMnt) -> (Vec<u8>, i32, Cdino) {
        let mut out = [0u8; NAME_MAX + 1];
        let mut len = 0u16;
        let mut ino: Cdino = 77;
        let res = cd9660_rrip_getname(&rec(r), &mut out, &mut len, &mut ino, imp);
        (out[..usize::from(len)].to_vec(), res, ino)
    }

    /// `cd9660_rrip_getsymname`: the link and the result.
    fn getsymname(r: &[u8], imp: &IsoMnt) -> (Vec<u8>, i32) {
        let mut out = [0u8; MAXPATHLEN];
        let mut len = 0u16;
        let res = cd9660_rrip_getsymname(&rec(r), &mut out, &mut len, imp);
        (out[..usize::from(len)].to_vec(), res)
    }

    #[test]
    fn the_root_announces_rock_ridge() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        assert_eq!(cd9660_rrip_offset(&rec(&image::ROOT_DOT), imp), 0);
        assert_eq!(imp.rr_skip0, 0);

        // no SP entry: not Rock Ridge (after looking for the CD-ROM XA layout too)
        assert_eq!(cd9660_rrip_offset(&rec(&image::FILE_REC), imp), -1);
        assert_eq!(imp.rr_skip0, 15);

        // CD-ROM XA: 15 bytes of XA data before the SP entry; the ER may say RRIP_1991A
        let mut sp_er = std::vec![0x58u8; 15];
        sp_er.extend_from_slice(b"SP\x07\x01\xbe\xef\x00");
        let mut er = std::vec![10, 0, 0, 1];
        er.extend_from_slice(b"RRIP_1991A");
        sp_er.extend_from_slice(&susp(b"ER", &er));
        let mut r = image::ROOT_DOT[..34].to_vec();
        r.extend_from_slice(&sp_er);
        r[0] = r.len() as u8;
        assert_eq!(cd9660_rrip_offset(&rec(&r), imp), 0);
        assert_eq!(imp.rr_skip0, 15);

        // an extension reference to something else is not Rock Ridge
        let pos = r.len() - 10;
        r[pos..].copy_from_slice(b"SOMETHING!");
        assert_eq!(cd9660_rrip_offset(&rec(&r), imp), -1);
    }

    #[test]
    fn names_come_from_nm_entries() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (name, res, ino) = getname(&image::FILE_REC, imp);
        assert_eq!(name, b"m10c-iso.txt");
        assert_ne!(res & ISO_SUSP_ALTNAME, 0);
        assert_eq!(ino, 77);
        assert_eq!(
            getname(&image::MIXED_REC, imp).0,
            b"Mixed_Case-name.long.txt"
        );
        assert_eq!(getname(&image::SUB_REC, imp).0, b"sub");
        assert_eq!(getname(&image::LINK_REC, imp).0, b"link");
        // '.' and '..' are named by the record, not by NM
        assert_eq!(getname(&image::ROOT_DOTDOT, imp).0, b"..");
        // the root's '.' continues in block 24, past this mount's last block: the walk stops
        imp.volume_space_size = 24;
        assert_eq!(getname(&image::ROOT_DOT, imp).0, b".");
    }

    #[test]
    fn names_without_nm_are_the_iso_names() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let r = record(b"README.TXT;1", false, &[px(0o100444, 1, 0, 0)]);
        let (name, res, _) = getname(&r, imp);
        assert_eq!(name, b"README.TXT;1");
        assert_eq!(res & ISO_SUSP_ALTNAME, 0);
    }

    #[test]
    fn nm_entries_continue_and_name_dots() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let r = record(
            b"A",
            false,
            &[susp(b"NM", b"\x01abc"), susp(b"NM", b"\x00def")],
        );
        assert_eq!(getname(&r, imp).0, b"abcdef");
        let r = record(b"A", false, &[susp(b"NM", &[ISO_SUSP_CFLAG_PARENT])]);
        assert_eq!(getname(&r, imp).0, b"..");
        let r = record(b"A", false, &[susp(b"NM", &[ISO_SUSP_CFLAG_CURRENT])]);
        assert_eq!(getname(&r, imp).0, b".");
        // an NM with bad flags counts as no NM: the ISO name stands
        let r = record(b"A", false, &[susp(b"NM", b"\x40xyz")]);
        assert_eq!(getname(&r, imp).0, b"A");
    }

    #[test]
    fn relocation_entries() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        // CL: a relocated directory's real location
        let r = record(
            b"D",
            true,
            &[susp(b"CL", &both32(40)), susp(b"NM", b"\x00d")],
        );
        let (name, res, ino) = getname(&r, imp);
        assert_eq!((name.as_slice(), ino), (b"d".as_slice(), 40 << 11));
        assert_ne!(res & ISO_SUSP_CLINK, 0);
        // RE: the relocated directory itself, which is not listed
        let r = record(b"D", true, &[susp(b"RE", &[]), susp(b"NM", b"\x00d")]);
        let (name, res, _) = getname(&r, imp);
        assert!(name.is_empty());
        assert_ne!(res & ISO_SUSP_RELDIR, 0);
    }

    #[test]
    fn symbolic_links_gather_their_components() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (link, res) = getsymname(&image::LINK_REC, imp);
        assert_eq!(link, b"../m10c-iso.txt");
        assert_eq!(res, ISO_SUSP_SLINK);

        let sl = |body: &[u8]| record(b"L", false, &[susp(b"SL", body)]);
        let r = sl(b"\x00\x08\x00\x00\x03etc\x00\x06passwd");
        assert_eq!(getsymname(&r, imp).0, b"/etc/passwd");
        let r = sl(b"\x00\x02\x00\x00\x03bin");
        assert_eq!(getsymname(&r, imp).0, b"./bin");
        // a component continued in the next SL entry
        let r = record(
            b"L",
            false,
            &[
                susp(b"SL", b"\x01\x01\x03abc"),
                susp(b"SL", b"\x00\x00\x03def"),
            ],
        );
        assert_eq!(getsymname(&r, imp), (b"abcdef".to_vec(), ISO_SUSP_SLINK));
        // the mount point
        imp.im_mountp
            .update_stat(|sp| sp.f_mntonname[..6].copy_from_slice(b"/cdrom"));
        let r = sl(b"\x00\x10\x00\x00\x03etc");
        assert_eq!(getsymname(&r, imp).0, b"/cdrom/etc");
        // a component running past the entry is invalid
        let r = sl(b"\x00\x00\x09abc");
        assert_eq!(getsymname(&r, imp), (b"".to_vec(), 0));
    }

    #[test]
    fn symbolic_links_longer_than_maxpathlen_are_refused() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let mut body = std::vec![0u8];
        for _ in 0..40 {
            body.extend_from_slice(&[0, 30]);
            body.extend_from_slice(&[b'x'; 30]);
        }
        // more than one entry's worth: split over SL entries of at most 250 bytes
        let entries: Vec<Vec<u8>> = body[1..]
            .chunks(32 * 7)
            .map(|c| {
                let mut e = std::vec![1u8];
                e.extend_from_slice(c);
                susp(b"SL", &e)
            })
            .collect();
        // a record would be too long for its length byte: walk the entries as the loop does
        let mut out = [0u8; MAXPATHLEN];
        let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_SLINK);
        ana.outbuf = &mut out;
        ana.maxlen = MAXPATHLEN as u16;
        ana.cont = 1;
        let mut res = 0;
        for e in &entries {
            res |= cd9660_rrip_slink(e, &mut ana);
            if ana.fields == 0 {
                break;
            }
        }
        assert_eq!(res, 0);
        assert_eq!(ana.outlen, 0);
        assert_eq!(ana.fields, 0);
    }

    #[test]
    fn attributes_come_from_px_and_pn() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (ip, _vp) = test_node(imp);
        let res = cd9660_rrip_analyze(&rec(&image::FILE_REC), ip, imp);
        let ino = ip.inode.get();
        assert_eq!(u32::from(ino.iso_mode), S_IFREG | 0o644);
        assert_eq!((ino.iso_links, ino.iso_uid, ino.iso_gid), (1, 0, 0));
        assert_ne!(res & ISO_SUSP_ATTR, 0);
        // makefs writes TF entries without the byte that ISO_RRIP_TSTAMP's size counts, so
        // they are not read: the times are the record's
        assert_eq!(res & ISO_SUSP_TSTAMP, 0);
        assert_eq!(ino.iso_mtime, Timespec::new(1_791_110_208, 0));

        let (ip, _vp) = test_node(imp);
        cd9660_rrip_analyze(&rec(&image::LINK_REC), ip, imp);
        assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFLNK | 0o755);

        let (ip, _vp) = test_node(imp);
        let r = record(
            b"TTY",
            false,
            &[
                px(S_IFCHR | 0o620, 1, 0, 4),
                susp(b"PN", &[both32(0), both32(makedev(5, 3) as u32)].concat()),
            ],
        );
        let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
        assert_ne!(res & ISO_SUSP_DEVICE, 0);
        assert_eq!(ip.inode.get().iso_rdev, makedev(5, 3));
        assert_eq!(ip.inode.get().iso_gid, 4);
        let r = record(
            b"TTY",
            false,
            &[susp(
                b"PN",
                &[both32(7), both32(makedev(5, 3) as u32)].concat(),
            )],
        );
        cd9660_rrip_analyze(&rec(&r), ip, imp);
        assert_eq!(ip.inode.get().iso_rdev, makedev(7, 3));
    }

    #[test]
    fn time_stamps_come_from_tf() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (ip, _vp) = test_node(imp);
        // modify and access, 7-byte form, plus the byte the C's size check counts
        let mut tf = std::vec![ISO_SUSP_TSTAMP_MODIFY | ISO_SUSP_TSTAMP_ACCESS];
        tf.extend_from_slice(&[99, 12, 31, 23, 59, 59, 0]);
        tf.extend_from_slice(&[70, 1, 1, 0, 0, 1, 0]);
        tf.push(0);
        let r = record(
            b"F",
            false,
            &[px(S_IFREG | 0o444, 1, 0, 0), susp(b"TF", &tf)],
        );
        let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
        assert_ne!(res & ISO_SUSP_TSTAMP, 0);
        let ino = ip.inode.get();
        assert_eq!(ino.iso_mtime, Timespec::new(946_684_799, 0));
        assert_eq!(ino.iso_atime, Timespec::new(1, 0));
        assert_eq!(ino.iso_ctime, ino.iso_mtime);

        // 17-byte form with the creation time first
        let mut tf =
            std::vec![ISO_SUSP_TSTAMP_FORM17 | ISO_SUSP_TSTAMP_CREAT | ISO_SUSP_TSTAMP_ATTR];
        tf.extend_from_slice(b"2000010100000000\x00");
        tf.extend_from_slice(b"2026100407364800\xf4");
        tf.push(0);
        let r = record(
            b"F",
            false,
            &[px(S_IFREG | 0o444, 1, 0, 0), susp(b"TF", &tf)],
        );
        cd9660_rrip_analyze(&rec(&r), ip, imp);
        let ino = ip.inode.get();
        assert_eq!(ino.iso_ctime, Timespec::new(1_791_110_208, 0));
        assert_eq!(ino.iso_mtime, Timespec::new(0, 0));
        assert_eq!(ino.iso_atime, ino.iso_mtime);
    }

    #[test]
    fn rr_st_and_missing_px() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (ip, _vp) = test_node(imp);
        // RR says only PX is recorded: the walk ends after it, the times are the record's
        let mut tf = std::vec![ISO_SUSP_TSTAMP_MODIFY];
        tf.extend_from_slice(&[99, 12, 31, 23, 59, 59, 0, 0]);
        let r = record(
            b"F",
            false,
            &[
                susp(b"RR", &[0x01]),
                px(S_IFREG | 0o600, 1, 1000, 10),
                susp(b"TF", &tf),
            ],
        );
        let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
        assert_eq!(res & ISO_SUSP_TSTAMP, 0);
        assert_eq!(ip.inode.get().iso_uid, 1000);
        assert_eq!(ip.inode.get().iso_mtime, Timespec::new(1_791_110_208, 0));

        // ST ends the system use area: the PX after it is not read, the defaults apply
        let (ip, _vp) = test_node(imp);
        let r = record(
            b"D",
            true,
            &[susp(b"ST", &[]), px(S_IFREG | 0o600, 1, 1000, 10)],
        );
        let res = cd9660_rrip_analyze(&rec(&r), ip, imp);
        assert_eq!(res & ISO_SUSP_ATTR, 0);
        assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o555);

        // the root's '.' keeps its PX in a continuation area this mount cannot reach
        let imp = test_mnt(ISO_FTYPE_RRIP);
        imp.volume_space_size = 24;
        let (ip, _vp) = test_node(imp);
        let res = cd9660_rrip_analyze(&rec(&image::ROOT_DOT), ip, imp);
        assert_eq!(res & ISO_SUSP_ATTR, 0);
        assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o555);
    }

    #[test]
    fn the_continuation_area_holds_the_root_attributes() {
        // the CE target of the root's '.' entry, walked as the loop walks it after the read
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (ip, _vp) = test_node(imp);
        let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_ATTR | ISO_SUSP_TSTAMP);
        ana.inop = Some(ip);
        assert_eq!(cd9660_rrip_attr(&image::ROOT_CE, &mut ana), ISO_SUSP_ATTR);
        assert_eq!(u32::from(ip.inode.get().iso_mode), S_IFDIR | 0o755);
        assert_eq!(ip.inode.get().iso_links, 4);
        let mut ana = IsoRripAnalyze::new(imp, ISO_SUSP_ATTR);
        let ce = &image::ROOT_DOT[image::ROOT_DOT.len() - 28..];
        assert_eq!(cd9660_rrip_cont(ce, &mut ana), ISO_SUSP_CONT);
        assert_eq!(
            (ana.iso_ce_blk, ana.iso_ce_off, ana.iso_ce_len),
            (24, 0, 36)
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/isofs/cd9660/cd9660_rrip.h");
        for (name, value) in [
            ("ISO_SUSP_CFLAG_CONTINUE", ISO_SUSP_CFLAG_CONTINUE),
            ("ISO_SUSP_CFLAG_CURRENT", ISO_SUSP_CFLAG_CURRENT),
            ("ISO_SUSP_CFLAG_PARENT", ISO_SUSP_CFLAG_PARENT),
            ("ISO_SUSP_CFLAG_ROOT", ISO_SUSP_CFLAG_ROOT),
            ("ISO_SUSP_CFLAG_VOLROOT", ISO_SUSP_CFLAG_VOLROOT),
            ("ISO_SUSP_CFLAG_HOST", ISO_SUSP_CFLAG_HOST),
            ("ISO_RRIP_SLSIZ", ISO_RRIP_SLSIZ as u8),
            ("ISO_SUSP_TSTAMP_FORM17", ISO_SUSP_TSTAMP_FORM17),
            ("ISO_SUSP_TSTAMP_FORM7", ISO_SUSP_TSTAMP_FORM7),
            ("ISO_SUSP_TSTAMP_CREAT", ISO_SUSP_TSTAMP_CREAT),
            ("ISO_SUSP_TSTAMP_MODIFY", ISO_SUSP_TSTAMP_MODIFY),
            ("ISO_SUSP_TSTAMP_ACCESS", ISO_SUSP_TSTAMP_ACCESS),
            ("ISO_SUSP_TSTAMP_ATTR", ISO_SUSP_TSTAMP_ATTR),
            ("ISO_SUSP_TSTAMP_BACKUP", ISO_SUSP_TSTAMP_BACKUP),
            ("ISO_SUSP_TSTAMP_EXPIRE", ISO_SUSP_TSTAMP_EXPIRE),
            ("ISO_SUSP_TSTAMP_EFFECT", ISO_SUSP_TSTAMP_EFFECT),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
