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
 * Copyright (c) 1993, 1994 Christopher G. Demetriou
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `exec_script.c`: the exec switch's handler for interpreted scripts (`#!`).
//!
//! Upstream: sys/kern/exec_script.c @ 3ce1f3f79392
//!
//! A file whose header starts with `#!` names its interpreter on the first line. The
//! handler parses that line ([`exec_script_parse`]), looks the interpreter up and checks it
//! with a second, recursive `check_exec` (marked `EXEC_INDIR`, so a script cannot name a
//! script), and leaves the package describing the interpreter, with a fake argument list
//! (`EXEC_HASARGL`) that `sys_execve` puts in front of the user's arguments minus `argv[0]`
//! (`EXEC_SKIPARG`): the interpreter, its one optional argument, and the script's path. A
//! script the process may not read, or a set[ug]id one, is handed over as an open
//! descriptor and named `/dev/fd/N` instead.
//!
//! ## Deviations
//! - `epp->ep_ndp` is the exec switch's third argument, as for `check_exec` (`sys/exec.rs`).
//!   The C points that nameidata at the interpreter's name, which lives in `ep_hdr`, and
//!   hands it to `check_exec` again; here the name is copied out of the header (the
//!   recursive `check_exec` reads a new header into `ep_hdr`) and looked up through a
//!   nameidata of its own with the caller's pledge, unveil and realpath buffer. On success
//!   its results (the vnode and the component name, with the interpreter's pathname
//!   buffer) are moved into the caller's nameidata and the script's pathname buffer is
//!   freed, which leaves the caller where the C's reuse leaves it; the caller's `ni_dirp`
//!   keeps the script's path, which nothing reads after the lookup.
//! - The fake argument list is `ep_fa`, a `Vec` of byte strings without their NULs; it is
//!   freed by being dropped, where the C frees each string and the array.
//! - A memory image (`ep_image`, the `init` boot module) has neither vnode nor nameidata: a
//!   `#!` header there is `ENOEXEC`, without destroying anything.
//! - The 4-clause licence (advertising clause) is kept whole; the user accepted every
//!   licence of the pinned tree (2026-10-04).

use alloc::format;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_descrip::{falloc, fdinsert, fdrelease};
use crate::kern::kern_exec::{check_exec, namei_pnbuf_put};
use crate::kern::vfs_lookup::ndinitat;
use crate::kern::vfs_vnops::{VNOPS, vn_close, vn_lock};
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::sys::errno::Errno;
use crate::sys::exec::{
    EXEC_DESTR, EXEC_HASARGL, EXEC_HASFD, EXEC_INDIR, EXEC_SKIPARG, ExecPackage,
};
use crate::sys::exec_script::{EXEC_SCRIPT_MAGIC, EXEC_SCRIPT_MAGICLEN};
use crate::sys::fcntl::{AT_FDCWD, FREAD};
use crate::sys::file::{DTYPE_VNODE, frele};
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::namei::{LOOKUP, Nameidata, NiDirp};
use crate::sys::param::{MAXINTERP, MAXPATHLEN};
use crate::sys::proc::Proc;
use crate::sys::vnode::{VREAD, VSGID, VSUID};

/// The first line of a script as `exec_script_makecmds` reads it: the interpreter and the
/// one argument that may follow it.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptLine<'h> {
    /// The interpreter's path (`shellname`); empty for a bare `#!`.
    pub shellname: &'h [u8],
    /// Everything after the interpreter and the blanks behind it (`shellarg`), as one
    /// argument, trailing blanks included; `None` when nothing follows.
    pub shellarg: Option<&'h [u8]>,
}

/// The header half of `exec_script_makecmds`: checks the magic and that the interpreter
/// line ends in a newline within the first `MAXINTERP` bytes of the valid header `hdr`, and
/// splits the line at the first blanks (spaces or tabs) after the interpreter's name.
/// `ENOEXEC` when the file is not a script this handler takes.
pub fn exec_script_parse(hdr: &[u8]) -> Result<ScriptLine<'_>, Errno> {
    if hdr.len() < EXEC_SCRIPT_MAGICLEN || !hdr.starts_with(EXEC_SCRIPT_MAGIC) {
        return Err(Errno::ENOEXEC);
    }

    // The shell spec must be terminated by a newline and not be too large.
    let hdrlinelen = hdr.len().min(MAXINTERP);
    let area = &hdr[EXEC_SCRIPT_MAGICLEN..hdrlinelen.max(EXEC_SCRIPT_MAGICLEN)];
    let Some(nl) = area.iter().position(|&c| c == b'\n') else {
        return Err(Errno::ENOEXEC);
    };
    // The C scans the line as a string: a NUL ends it early.
    let line = &area[..nl];
    let line = &line[..line.iter().position(|&c| c == 0).unwrap_or(line.len())];

    let blank = |c: &u8| *c == b' ' || *c == b'\t';
    // strip spaces before the shell name
    let rest = &line[line.iter().position(|c| !blank(c)).unwrap_or(line.len())..];
    // collect the shell name
    let namelen = rest.iter().position(blank).unwrap_or(rest.len());
    let shellname = &rest[..namelen];
    // skip spaces before any argument; everything after the shell name is passed as ONE
    // argument, the correct (historical) behaviour.
    let after = &rest[namelen..];
    let arg = &after[after.iter().position(|c| !blank(c)).unwrap_or(after.len())..];
    Ok(ScriptLine {
        shellname,
        shellarg: (!arg.is_empty()).then_some(arg),
    })
}

/// Check if it's an executable shell script. If it is, set things up so that the script can
/// be run: the package then describes the shell that will run the script (its vmcmds come
/// from the recursive `check_exec`) and carries the shell's arguments.
///
/// `ndp` is the nameidata `check_exec` found the script with (`epp->ep_ndp`). A failure
/// after the magic matched is destructive (`EXEC_DESTR`): the script is closed and the
/// pathname buffer freed.
pub fn exec_script_makecmds(
    p: &Proc,
    epp: &mut ExecPackage<'_>,
    ndp: Option<&mut Nameidata<'_>>,
) -> Result<(), Errno> {
    // if the magic isn't that of a shell script, or we've already done shell script
    // processing for this exec, punt on it.
    if epp.ep_flags & EXEC_INDIR != 0 {
        return Err(Errno::ENOEXEC);
    }
    let line = exec_script_parse(epp.hdr())?;
    // A memory image has no vnode to hand to an interpreter (see the module's deviations).
    let (Some(ndp), Some(scriptvp)) = (ndp, epp.ep_vp) else {
        return Err(Errno::ENOEXEC);
    };
    // The names are copied: the recursive check_exec reads the shell's header into ep_hdr.
    let shellname: Vec<u8> = line.shellname.to_vec();
    let shellarg: Option<Vec<u8>> = line.shellarg.map(<[u8]>::to_vec);
    let cred = p.p_ucred.get();

    // MNT_NOSUID and STRC are already taken care of by check_exec, so we don't need to
    // worry about them now or later.
    let script_sbits = epp.ep_vap.va_mode & (VSUID | VSGID);
    let (script_uid, script_gid) = (epp.ep_vap.va_uid, epp.ep_vap.va_gid);

    // If the script isn't readable, or it's set-id, then we've gotta supply a "/dev/fd/..."
    // for the shell to read. Note that stupid shells (csh) do the wrong thing, and close
    // all open fd's when they start. That kills this method of implementing "safe" set-id
    // and x-only scripts.
    let _ = vn_lock(scriptvp, LK_EXCLUSIVE | LK_RETRY);
    let access = VOP_ACCESS(scriptvp, VREAD, cred, p);
    let _ = VOP_UNLOCK(scriptvp);
    if access == Err(Errno::EACCES) || script_sbits != 0 {
        #[cfg(feature = "diagnostic")]
        if epp.ep_flags & EXEC_HASFD != 0 {
            #[allow(clippy::panic)] // ported panic() path
            crate::kern::subr_prf::panic(format_args!(
                "exec_script_makecmds: epp already has a fd"
            ));
        }

        let fdp = p.fd();
        fdplock(fdp);
        let (fp, fd) = match falloc(p) {
            Ok(got) => got,
            Err(error) => {
                fdpunlock(fdp);
                return exec_script_fail(p, epp, ndp, scriptvp, error);
            }
        };
        epp.ep_fd = fd;
        epp.ep_flags |= EXEC_HASFD;
        fp.f_type.set(DTYPE_VNODE);
        fp.f_ops.set(Some(&VNOPS));
        fp.f_data.set(ptr::from_ref(scriptvp).cast_mut().cast());
        fp.f_flag.store(FREAD as u32, Ordering::SeqCst);
        fdinsert(fdp, fd, 0, fp);
        fdpunlock(fdp);
        let _ = frele(fp, p);
    }

    // set up the fake args list, for later: the shell, its argument, and the script.
    let mut fa: Vec<Vec<u8>> = Vec::with_capacity(3);
    let script = if epp.ep_flags & EXEC_HASFD == 0 {
        // strlcpy(*tmpsap, epp->ep_name, MAXPATHLEN)
        let name = epp.ep_name;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        name[..len.min(MAXPATHLEN - 1)].to_vec()
    } else {
        format!("/dev/fd/{}", epp.ep_fd).into_bytes()
    };

    // mark the header we have as invalid; check_exec will read the header from the new
    // executable
    epp.ep_hdrvalid = 0;

    // set up the parameters for the recursive check_exec() call
    let mut shnd = ndinitat(LOOKUP, 0, AT_FDCWD, NiDirp::Sys(&shellname), p);
    shnd.ni_pledge = ndp.ni_pledge;
    shnd.ni_unveil = ndp.ni_unveil;
    shnd.ni_cnd.cn_rpbuf = ndp.ni_cnd.cn_rpbuf;
    epp.ep_flags |= EXEC_INDIR;

    let error = match check_exec(p, epp, Some(&mut shnd)) {
        Ok(()) => {
            // note that we've clobbered the header
            epp.ep_flags |= EXEC_DESTR;

            // It succeeded. Close the script if we aren't using it any more.
            if epp.ep_flags & EXEC_HASFD == 0 {
                let _ = vn_close(scriptvp, FREAD, cred, Some(p));
            }

            // The caller's nameidata now describes the shell's lookup; free the old
            // pathname buffer (now in shnd).
            ndp.ni_dirfd = AT_FDCWD;
            ndp.ni_vp = shnd.ni_vp;
            ndp.ni_dvp = shnd.ni_dvp;
            ndp.ni_pathlen = shnd.ni_pathlen;
            ndp.ni_next = shnd.ni_next;
            ndp.ni_loopcnt = shnd.ni_loopcnt;
            ndp.ni_unveil_match = shnd.ni_unveil_match;
            core::mem::swap(&mut ndp.ni_cnd, &mut shnd.ni_cnd);
            namei_pnbuf_put(&mut shnd);

            // set things up so that the fake args list will be used.
            fa.push(shellname);
            if let Some(arg) = shellarg {
                fa.push(arg);
            }
            fa.push(script);
            epp.ep_flags |= EXEC_HASARGL | EXEC_SKIPARG;
            epp.ep_fa = fa;

            // set things up so that set-id scripts will be handled appropriately
            epp.ep_vap.va_mode |= script_sbits;
            if script_sbits & VSUID != 0 {
                epp.ep_vap.va_uid = script_uid;
            }
            if script_sbits & VSGID != 0 {
                epp.ep_vap.va_gid = script_gid;
            }
            return Ok(());
        }
        Err(error) => error,
    };

    // A failed check_exec has given back the shell's pathname buffer; this is a no-op
    // unless it kept one.
    namei_pnbuf_put(&mut shnd);
    exec_script_fail(p, epp, ndp, scriptvp, error)
}

/// `fail:` in `exec_script_makecmds`: marks the header clobbered, kills the descriptor or
/// closes the script, frees the script's pathname buffer and the vmcmds, and returns
/// `error`. The fake argument list, not handed over yet, is dropped by the caller's scope.
fn exec_script_fail(
    p: &Proc,
    epp: &mut ExecPackage<'_>,
    ndp: &mut Nameidata<'_>,
    scriptvp: &'static crate::sys::vnode::Vnode,
    error: Errno,
) -> Result<(), Errno> {
    // note that we've clobbered the header
    epp.ep_flags |= EXEC_DESTR;

    // kill the opened file descriptor, else close the file
    if epp.ep_flags & EXEC_HASFD != 0 {
        epp.ep_flags &= !EXEC_HASFD;
        fdplock(p.fd());
        // fdrelease() unlocks p->p_fd.
        let _ = fdrelease(p, epp.ep_fd);
    } else {
        let _ = vn_close(scriptvp, FREAD, p.p_ucred.get(), Some(p));
    }

    namei_pnbuf_put(ndp);

    // free any vmspace-creation commands, and release their references
    epp.ep_vmcmds.kill();

    Err(error)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(hdr: &[u8]) -> Result<(&[u8], Option<&[u8]>), Errno> {
        exec_script_parse(hdr).map(|l| (l.shellname, l.shellarg))
    }

    #[test]
    fn plain_interpreter() {
        assert_eq!(parse(b"#!/bin/sh\necho hi\n"), Ok((&b"/bin/sh"[..], None)));
    }

    #[test]
    fn blanks_around_the_name_are_skipped() {
        assert_eq!(parse(b"#! \t/bin/sh \t\n"), Ok((&b"/bin/sh"[..], None)));
    }

    #[test]
    fn everything_after_the_name_is_one_argument() {
        assert_eq!(
            parse(b"#!/usr/bin/env  perl -w \nprint 1;\n"),
            Ok((&b"/usr/bin/env"[..], Some(&b"perl -w "[..])))
        );
        assert_eq!(
            parse(b"#!/bin/ksh\t-e\n"),
            Ok((&b"/bin/ksh"[..], Some(&b"-e"[..])))
        );
    }

    #[test]
    fn bare_magic_names_an_empty_interpreter() {
        assert_eq!(parse(b"#!\n"), Ok((&b""[..], None)));
        assert_eq!(parse(b"#!   \n"), Ok((&b""[..], None)));
    }

    #[test]
    fn a_nul_ends_the_line() {
        assert_eq!(parse(b"#!/bin/sh\0 -x\n"), Ok((&b"/bin/sh"[..], None)));
        assert_eq!(
            parse(b"#!/bin/sh -x\0y\n"),
            Ok((&b"/bin/sh"[..], Some(&b"-x"[..])))
        );
    }

    #[test]
    fn not_a_script() {
        assert_eq!(parse(b""), Err(Errno::ENOEXEC));
        assert_eq!(parse(b"#"), Err(Errno::ENOEXEC));
        assert_eq!(parse(b"\x7fELF\x02\x01\x01"), Err(Errno::ENOEXEC));
        assert_eq!(parse(b"# !/bin/sh\n"), Err(Errno::ENOEXEC));
    }

    #[test]
    fn the_newline_must_come_within_maxinterp_bytes() {
        // No newline in the valid header.
        assert_eq!(parse(b"#!/bin/sh"), Err(Errno::ENOEXEC));
        // The newline as the last byte the C looks at (index MAXINTERP - 1) ...
        let mut hdr = std::vec![b'a'; MAXINTERP + 4];
        hdr[..3].copy_from_slice(b"#!/");
        hdr[MAXINTERP - 1] = b'\n';
        let (name, arg) = parse(&hdr).unwrap();
        assert_eq!(name.len(), MAXINTERP - 1 - EXEC_SCRIPT_MAGICLEN);
        assert_eq!(arg, None);
        // ... and one byte later: too long.
        hdr[MAXINTERP - 1] = b'a';
        hdr[MAXINTERP] = b'\n';
        assert_eq!(parse(&hdr), Err(Errno::ENOEXEC));
    }

    #[test]
    fn the_header_is_large_enough_for_a_script() {
        assert!(crate::sys::exec::exec_maxhdrsz() >= crate::sys::exec_script::EXEC_SCRIPT_HDRSZ);
    }

    #[test]
    fn a_carriage_return_is_part_of_the_name() {
        assert_eq!(parse(b"#!/bin/sh\r\n"), Ok((&b"/bin/sh\r"[..], None)));
    }

    #[test]
    fn only_the_valid_part_of_the_header_counts() {
        // ep_hdrvalid bytes: a newline beyond them is not seen.
        let mut hdr = std::vec![b'a'; crate::sys::exec_script::EXEC_SCRIPT_HDRSZ];
        hdr[..3].copy_from_slice(b"#!/");
        hdr[20] = b'\n';
        assert_eq!(parse(&hdr[..20]), Err(Errno::ENOEXEC));
        assert!(parse(&hdr[..21]).is_ok());
        // A newline at the very end of the header is past MAXINTERP.
        hdr[20] = b'a';
        let last = hdr.len() - 1;
        hdr[last] = b'\n';
        assert_eq!(parse(&hdr), Err(Errno::ENOEXEC));
    }

    #[test]
    fn scripts_come_before_elf_in_the_exec_switch() {
        use crate::kern::kern_exec::EXECSW;
        use crate::sys::exec::{ExecMakecmdsFcn, exec_maxhdrsz};
        // exec_conf.c's order: the script handler first, then ELF.
        assert_eq!(EXECSW.len(), 2);
        assert_eq!(
            EXECSW[0].es_hdrsz,
            crate::sys::exec_script::EXEC_SCRIPT_HDRSZ
        );
        assert!(core::ptr::fn_addr_eq(
            EXECSW[0].es_check,
            exec_script_makecmds as ExecMakecmdsFcn
        ));
        // init_exec: exec_maxhdrsz is the largest es_hdrsz.
        assert_eq!(
            Some(exec_maxhdrsz()),
            EXECSW.iter().map(|e| e.es_hdrsz).max()
        );
    }
}
/* </TESTS> */
