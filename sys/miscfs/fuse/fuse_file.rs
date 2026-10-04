/* $OpenBSD: fuse_file.c,v 1.12 2026/06/20 13:45:13 helg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
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

//! FUSE file handles: `FUSE_OPEN`/`FUSE_OPENDIR` and `FUSE_RELEASE`/`FUSE_RELEASEDIR` for a
//! node's handle of one kind, and the handle a read or write uses.
//!
//! Upstream: sys/miscfs/fuse/fuse_file.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `flags` is the `int` the C passes, sent as the protocol's `uint32_t`; `isdir` is a
//!   `bool`.

use crate::miscfs::fuse::fusebuf::{fb_delete, fb_queue, fb_setup};
use crate::miscfs::fuse::fusefs::FusefsMnt;
use crate::miscfs::fuse::fusefs_node::{
    FUFH_INVALID, FUFH_RDWR, FufhType, FusefsFilehandle, FusefsNode,
};
use crate::sys::errno::Errno;
use crate::sys::fusebuf::{
    FUSE_OPEN, FUSE_OPENDIR, FUSE_RELEASE, FUSE_RELEASEDIR, FuseOpenIn, FuseOpenOut, FuseReleaseIn,
};
use crate::sys::proc::Proc;

/// `fusefs_file_open`: asks the daemon to open the node (a directory when `isdir`) with
/// `flags` and records the handle it returns as the node's handle of kind `fufh_type`.
/// Nothing is sent before the session is up.
pub fn fusefs_file_open(
    fmp: &FusefsMnt,
    ip: &FusefsNode,
    fufh_type: FufhType,
    flags: i32,
    isdir: bool,
    p: &Proc,
) -> Result<(), Errno> {
    if fmp.sess_init.get() == 0 {
        return Ok(());
    }

    let fbuf = fb_setup(
        0,
        ip.i_number,
        if isdir { FUSE_OPENDIR } else { FUSE_OPEN },
        p,
    );
    fbuf.op_set(&FuseOpenIn {
        flags: flags as u32,
        ..FuseOpenIn::default()
    });

    if let Err(error) = fb_queue(fmp.dev, fbuf) {
        fb_delete(fbuf);
        return Err(error);
    }

    ip.set_fufh(
        fufh_type,
        FusefsFilehandle {
            fh_id: fbuf.op_get::<FuseOpenOut>().fh,
            fh_type: fufh_type,
        },
    );

    fb_delete(fbuf);
    Ok(())
}

/// `fusefs_file_close`: asks the daemon to release the node's handle of kind `fufh_type`
/// (the daemon's error is returned, but the handle is forgotten in any case).
pub fn fusefs_file_close(
    fmp: &FusefsMnt,
    ip: &FusefsNode,
    fufh_type: FufhType,
    flags: i32,
    isdir: bool,
    p: &Proc,
) -> Result<(), Errno> {
    let mut error = Ok(());

    if fmp.sess_init.get() != 0 {
        let fbuf = fb_setup(
            0,
            ip.i_number,
            if isdir { FUSE_RELEASEDIR } else { FUSE_RELEASE },
            p,
        );
        fbuf.op_set(&FuseReleaseIn {
            fh: ip.fufh(fufh_type).fh_id,
            flags: flags as u32,
            ..FuseReleaseIn::default()
        });

        error = fb_queue(fmp.dev, fbuf);
        // if (error && (error != ENOSYS)) DPRINTF("file close error: %d\n", error);

        fb_delete(fbuf);
    }

    ip.set_fufh(
        fufh_type,
        FusefsFilehandle {
            fh_id: u64::MAX,
            fh_type: FUFH_INVALID,
        },
    );

    error
}

/// `fusefs_fd_get`: the node's handle of kind `type`, or its read-write one when it has
/// none of that kind.
pub fn fusefs_fd_get(ip: &FusefsNode, r#type: FufhType) -> u64 {
    let t = if ip.fufh(r#type).fh_type == FUFH_INVALID {
        FUFH_RDWR
    } else {
        r#type
    };

    ip.fufh(t).fh_id
}
