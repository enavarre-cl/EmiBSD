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

/*
 * Copyright (c) 2005 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Julio M. Merino Vidal, developed as part of Google's Summer of Code
 * 2005 program.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! tmpfs vnode interface for special devices: the operations of a block or character
//! device node stored in a tmpfs file system (`tmpfs_specvops`), which keep the node's
//! times and leave the device work to `spec_vnops`.
//!
//! Upstream: sys/tmpfs/tmpfs_specops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The helpers the C installs in many slots (`vop_generic_badop`) are closures, as in
//!   `spec_vops` (`docs/C_TO_RUST.md`).

use crate::kern::spec_vnops::{
    spec_advlock, spec_close, spec_fsync, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf,
    spec_read, spec_strategy, spec_write,
};
use crate::kern::vfs_default::{
    vop_generic_badop, vop_generic_bmap, vop_generic_bwrite, vop_generic_lookup, vop_generic_revoke,
};
use crate::sys::errno::Errno;
use crate::sys::vnode::{VopReadArgs, VopWriteArgs, Vops};
use crate::tmpfs::tmpfs::{TMPFS_NODE_ACCESSED, TMPFS_NODE_MODIFIED, VP_TO_TMPFS_NODE};
use crate::tmpfs::tmpfs_subr::tmpfs_update;
use crate::tmpfs::tmpfs_vnops::{
    tmpfs_access, tmpfs_getattr, tmpfs_inactive, tmpfs_islocked, tmpfs_lock, tmpfs_print,
    tmpfs_reclaim, tmpfs_setattr, tmpfs_unlock,
};

/// `tmpfs_specvops`: vnode operations vector used for special devices stored in a tmpfs
/// file system.
pub static TMPFS_SPECVOPS: Vops = Vops {
    vop_close: Some(spec_close),
    vop_access: Some(tmpfs_access),
    vop_getattr: Some(tmpfs_getattr),
    vop_setattr: Some(tmpfs_setattr),
    vop_read: Some(tmpfs_spec_read),
    vop_write: Some(tmpfs_spec_write),
    vop_fsync: Some(spec_fsync),
    vop_inactive: Some(tmpfs_inactive),
    vop_reclaim: Some(tmpfs_reclaim),
    vop_lock: Some(tmpfs_lock),
    vop_unlock: Some(tmpfs_unlock),
    vop_print: Some(tmpfs_print),
    vop_islocked: Some(tmpfs_islocked),

    // keep in sync with spec_vops
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(spec_strategy),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(spec_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `tmpfs_spec_read` (`vop_read`): mark the node accessed, then read the device.
pub fn tmpfs_spec_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    tmpfs_update(VP_TO_TMPFS_NODE(vp), TMPFS_NODE_ACCESSED);
    spec_read(ap)
}

/// `tmpfs_spec_write` (`vop_write`): mark the node modified, then write the device.
pub fn tmpfs_spec_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    tmpfs_update(VP_TO_TMPFS_NODE(vp), TMPFS_NODE_MODIFIED);
    spec_write(ap)
}
/* </CODE> */
