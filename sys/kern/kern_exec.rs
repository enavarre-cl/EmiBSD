/*	$OpenBSD: kern_exec.c,v 1.275 2026/09/17 19:45:07 dgl Exp $	*/
/*	$NetBSD: kern_exec.c,v 1.75 1996/02/09 18:59:28 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 1993, 1994 Christopher G. Demetriou
 * Copyright (C) 1992 Wolfgang Solfrank.
 * Copyright (C) 1992 TooLs GmbH.
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
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `kern_exec.c`: the exec system call: finds the executable, checks it against the exec
//! switch, copies the arguments in, builds the new address space and stack, and sets the
//! registers.
//!
//! Upstream: sys/kern/kern_exec.c @ 3ce1f3f79392
//!
//! Status: `wip`. The whole file: `exec_free_package`, `check_exec` (`namei` with the
//! realpath for `AUX_openbsd_execpath`, `VOP_GETATTR`, `VOP_ACCESS`, `VOP_OPEN`, the header
//! through `vn_rdwr`), `sys_execve` (the `NCARGS` argument buffer in `exec_map`, the stack
//! gap, `ps_strings`, the execpath, the pin tables, set[ug]id executables, the pledge
//! hand-over, `exec_timekeep_map`, `exec_elf_fixup`, `setregs`, `exec_sigcode_map`),
//! `copyargs`, `exec_sigcode_map`, `exec_timekeep_map` and `stackgap_random`.
//! `exec_conf.c`'s `execsw[]` is here as [`EXECSW`] (ELF only: `exec_script.c` is not
//! ported).
//!
//! Two callers reach the body. `sys_execve` finds the executable with `namei`, as the C
//! does. `start_init` tries `initpaths[]` through `sys_execve` and, while there is no root
//! file system (every `namei` is `ENOENT`), execs the `init` boot module with
//! [`exec_image`]: the same body over an executable in kernel memory (`ExecFile::Image`),
//! with the arguments `start_init` copied out to the user stack as for `sys_execve`.
//!
//! ## Deviations
//! - [`exec_image`] is this port's name for "`sys_execve` of a memory image": `check_exec`
//!   skips `namei` and the attribute, mount, access and open checks (an image has no vnode,
//!   no set[ug]id bits, no realpath), `ep_hdr` is the image's first `exec_maxhdrsz` bytes
//!   and the vmcmds read the image (`exec_subr.rs`). It goes away when `init` is a file.
//! - The set[ug]id stdin/stdout/stderr fix-up opens `/dev/null` with `cdevvp(getnulldev())`:
//!   the device switch (`<sys/conf.h>`) is not ported, so a set[ug]id exec with one of the
//!   three descriptors closed reports it and fails (the C would succeed).
//! - `prof_exec`, `stopprofclock` (profiling, `subr_prof.c`), the `NOTE_EXEC` knote
//!   (`kern_event.c`) and `unveil_destroy` (`kern_unveil.c`) are reported or no-ops where
//!   their subsystem is missing (see each call site); `KTRACE` is not configured.
//! - The 4-clause licence (advertising clause) was accepted by the user at M2 for this
//!   project.

use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::dev::rnd::{arc4random, arc4random_buf};
use crate::kern::exec_elf::{exec_elf_fixup, exec_elf_makecmds};
use crate::kern::exec_subr::exec_process_vmcmds;
use crate::kern::kern_descrip::{closef, falloc, fd_getfile, fdprepforexec, fdrelease, fdremove};
use crate::kern::kern_exit::{exit1, pin_free};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::{crcopy, crfree, crhold, proc_cansugid};
use crate::kern::kern_sig::{execsigs, psignal, single_thread_clear, single_thread_set};
use crate::kern::kern_synch::wakeup;
use crate::kern::kern_time::cancel_all_itimers;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_getcwd::vfs_getcwd_common;
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vput, vref, vrele};
use crate::kern::vfs_syscalls::NameiBuf;
use crate::kern::vfs_vnops::{vn_close, vn_rdwr};
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_GETATTR, VOP_OPEN, VOP_UNLOCK};
use crate::machine::copy::{copyin, copyinstr, copyout, copyoutstr};
use crate::machine::cpu::Cpu;
use crate::machine::param::MachineParam;
use crate::machine::signal::MachineSignal;
use crate::machine::tcb::tcb_set;
use crate::machine::{Machine, VmParam};
use crate::sys::acct::AFORK;
use crate::sys::errno::Errno;
use crate::sys::exec::{
    EXEC_DESTR, EXEC_HASARGL, EXEC_HASFD, EXEC_INDIR, EXEC_NOBTCFI, EXEC_PROFILE, EXEC_SKIPARG,
    EXEC_WXNEEDED, ExecPackage, Execsw, PsStrings,
};
use crate::sys::exec_elf::{ELF_AUX_WORDS, ElfEhdr};
use crate::sys::fcntl::FREAD;
use crate::sys::file::frele;
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::malloc::{M_PINSYSCALL, M_TEMP, M_WAITOK};
use crate::sys::mman::{
    MADV_RANDOM, MAP_INHERIT_COPY, MAP_INHERIT_SHARE, PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE,
};
use crate::sys::mount::{MNT_NOEXEC, MNT_NOSUID};
use crate::sys::namei::{
    BYPASSUNVEIL, EXECPATH, FOLLOW, LOCKLEAF, LOOKUP, NOFOLLOW, Nameidata, NiDirp, SAVENAME,
    UNVEIL_EXEC,
};
use crate::sys::param::{MAXPATHLEN, NCARGS, PAGE_MASK};
use crate::sys::pledge::PLEDGE_EXEC;
use crate::sys::proc::{
    EXIT_NORMAL, PS_EXEC, PS_EXECPLEDGE, PS_INEXEC, PS_ISPWAIT, PS_PLEDGE, PS_PPWAIT, PS_SUGID,
    PS_SUGIDEXEC, PS_TRACED, PS_WAITEVENT, PSI_NOBTCFI, PSI_PROFILE, PSI_WXNEEDED, Proc, Process,
    SINGLE_DEEP, SINGLE_EXIT, SINGLE_UNWIND,
};
use crate::sys::resource::RLIMIT_DATA;
use crate::sys::resourcevar::lim_cur;
use crate::sys::signal::{SIGABRT, SIGTRAP};
use crate::sys::stat::{S_IXGRP, S_IXOTH, S_IXUSR};
use crate::sys::syscallargs::SysExecveArgs;
use crate::sys::syslimits::{ARG_MAX, PATH_MAX};
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::time::Timespec;
use crate::sys::timetc::{TK_VERSION, Timekeep};
use crate::sys::types::{Register, Vaddr, Vsize};
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::vnode::{VEXEC, VREG, VSGID, VSUID};
use crate::unported;
use crate::uvm::uvm_aobj::{uao_create, uao_detach, uao_reference};
use crate::uvm::uvm_extern::{KmemVaMode, KvMap, UVM_FLAG_COPYONW, uvm_mapflag};
use crate::uvm::uvm_fault::uvm_fault_wire;
use crate::uvm::uvm_km::{KD_WAITOK, KP_PAGEABLE, kernel_map, km_alloc, km_free};
use crate::uvm::uvm_map::{uvm_map, uvm_map_immutable, uvm_map_protect, uvm_unmap, uvmspace_exec};
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_param::{atop, round_page, trunc_page};

/// `sigobject`: the shared sigcode object, created by the first `exec_sigcode_map` and
/// referenced forever after.
static SIGOBJECT: AtomicPtr<UvmObject> = AtomicPtr::new(ptr::null_mut());
/// `sigcode_va`: where `sigobject` is mapped (read-only) in `kernel_map`.
pub static SIGCODE_VA: AtomicUsize = AtomicUsize::new(0);
/// `sigcode_sz`: the size of that mapping.
pub static SIGCODE_SZ: AtomicUsize = AtomicUsize::new(0);
/// `timekeep_object`: the shared timekeep object, created by the first
/// `exec_timekeep_map`.
static TIMEKEEP_OBJECT: AtomicPtr<UvmObject> = AtomicPtr::new(ptr::null_mut());
/// `timekeep`: the timekeep page in `kernel_map` (wired), which `tc_update_timekeep`
/// (`kern_tc.c`) writes; null until the first exec maps it.
pub static TIMEKEEP: AtomicPtr<Timekeep> = AtomicPtr::new(ptr::null_mut());

/// `kv_exec`: the argument buffers, from `exec_map`, waiting for space.
pub static KV_EXEC: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Exec,
    kv_align: 0,
    kv_wait: true,
    kv_singlepage: false,
};

/// `stackgap_random`: if non-zero, the upper limit of the random gap size added to the
/// fixed stack position. Must be n^2 (`kern.stackgap_random`).
#[allow(non_upper_case_globals)] // the C global; `STACKGAP_RANDOM` is the machine's constant
pub static stackgap_random: AtomicI32 =
    AtomicI32::new(<Machine as VmParam>::STACKGAP_RANDOM as i32);

/// `execsw[]` (`exec_conf.c`): the executable formats, in the order they are tried. The
/// shell script format (`exec_script.c`) is not ported.
pub static EXECSW: [Execsw; 1] = [Execsw {
    es_hdrsz: size_of::<ElfEhdr>(),
    es_check: exec_elf_makecmds,
}];

/// Free exec-package allocations owned by image-format loaders.
pub fn exec_free_package(pack: &mut ExecPackage<'_>) {
    // pool_put(&namei_pool, pack->ep_interp): the buffer gives itself back.
    pack.ep_interp = None;
    if let Some(pins) = pack.ep_pins.take() {
        free(
            pins.cast::<u8>(),
            M_PINSYSCALL,
            pack.ep_npins.max(0) as usize * size_of::<u32>(),
        );
    }
    pack.ep_npins = 0;
}

/// `pool_put(&namei_pool, ndp->ni_cnd.cn_pnbuf)`: gives back the pathname buffer a
/// `SAVENAME` lookup kept.
fn namei_pnbuf_put(ndp: &mut Nameidata<'_>) {
    if let Some(buf) = NonNull::new(ndp.ni_cnd.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    ndp.ni_cnd.cn_pnbuf = ptr::null_mut();
}

/// check exec: given an "executable" described in the exec package's namei info, see what
/// we can do with it.
///
/// ON ENTRY: exec package with appropriate namei info (`ndp`; `None` for a memory image),
/// proc pointer of exec'ing proc, NO SELF-LOCKED VNODES.
///
/// ON EXIT: error: nothing held, etc. exec header still allocated. ok: filled exec package,
/// one locked vnode.
///
/// EXEC SWITCH ENTRY: locked vnode to check, exec package, proc.
///
/// EXEC SWITCH EXIT: ok: return 0, filled exec package, one locked vnode. error:
/// destructive: everything deallocated except exec header. non-destructive: error code,
/// locked vnode, exec header unmodified.
pub fn check_exec(
    p: &Proc,
    epp: &mut ExecPackage<'_>,
    ndp: Option<&mut Nameidata<'_>>,
) -> Result<(), Errno> {
    let Some(ndp) = ndp else {
        // A memory image (see the module's deviations): no lookup, no vnode checks; the
        // header is the image's first bytes.
        let image = epp.ep_image.ok_or(Errno::ENOEXEC)?;
        let n = image.len().min(epp.ep_hdr.len());
        epp.ep_hdr[..n].copy_from_slice(&image[..n]);
        epp.ep_hdrvalid = n;
        return check_exec_switch(p, epp);
    };

    ndp.ni_cnd.cn_nameiop = LOOKUP;
    ndp.ni_cnd.cn_flags = FOLLOW | LOCKLEAF | SAVENAME | EXECPATH;
    if epp.ep_flags & EXEC_INDIR != 0 {
        ndp.ni_cnd.cn_flags |= BYPASSUNVEIL;
    }

    ndp.ni_cnd.cn_rpi = 0;

    // If realpath calculations fail, execve proceeds without the information
    let dirp: &[u8] = match ndp.ni_dirp {
        NiDirp::Sys(path) => path,
        NiDirp::User(_) => &[],
    };
    let first = dirp.first().copied().unwrap_or(0);
    if first != 0 && first != b'/' {
        let cwdlen = MAXPATHLEN * 4; // for vfs_getcwd_common
        if let Some(mem) = malloc(cwdlen, M_TEMP, M_WAITOK) {
            // SAFETY: a fresh `cwdlen`-byte allocation, freed below; the path is built
            // backwards from the NUL and only those bytes are read.
            let cwdbuf = unsafe { slice::from_raw_parts_mut(mem.as_ptr(), cwdlen) };

            // vfs_getcwd_common fills this in backwards
            let mut bp = cwdlen - 1;
            cwdbuf[bp] = 0;

            // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
            let error = match p.fd().fd_cdir.get() {
                None => Err(Errno::ENOENT), // no root file system yet
                Some(cdir) => vfs_getcwd_common(
                    cdir,
                    None,
                    Some((&mut *cwdbuf, &mut bp)),
                    (cwdlen / 2) as i32,
                    crate::sys::vnode::GETCWD_CHECK_ACCESS,
                    p,
                ),
            };

            let cwd = &cwdbuf[bp..];
            let len = cwd.iter().position(|&c| c == 0).unwrap_or(cwd.len());
            if error.is_err() || len >= MAXPATHLEN || ndp.ni_cnd.cn_rpbuf.is_null() {
                ndp.ni_cnd.cn_flags &= !EXECPATH;
            } else {
                // strlcpy(ndp->ni_cnd.cn_rpbuf, bp, MAXPATHLEN)
                // SAFETY: `cn_rpbuf` is the caller's `MAXPATHLEN`-byte realpath buffer.
                let rp = unsafe { slice::from_raw_parts_mut(ndp.ni_cnd.cn_rpbuf, MAXPATHLEN) };
                rp[..len].copy_from_slice(&cwd[..len]);
                rp[len] = 0;
                ndp.ni_cnd.cn_rpi = len;
            }
            free(mem, M_TEMP, cwdlen);
        } else {
            ndp.ni_cnd.cn_flags &= !EXECPATH;
        }
    }

    // first get the vnode
    namei(ndp)?;
    let Some(vp) = ndp.ni_vp else {
        namei_pnbuf_put(ndp);
        return Err(Errno::ENOENT);
    };
    epp.ep_vp = Some(vp);

    let cred = p.p_ucred.get();
    // bad1: free the namei pathname buffer, and put the vnode (which we don't yet have
    // open).
    let bad1 = |ndp: &mut Nameidata<'_>, error: Errno| -> Result<(), Errno> {
        namei_pnbuf_put(ndp);
        vput(vp);
        Err(error)
    };

    // check for regular file
    if vp.v_type.get() != VREG {
        return bad1(ndp, Errno::EACCES);
    }

    // get attributes
    if let Err(error) = VOP_GETATTR(vp, &mut epp.ep_vap, cred, p) {
        return bad1(ndp, error);
    }

    // Check mount point
    let mnt_flag = vp.v_mount.get().map_or(0, |mp| mp.mnt_flag.get());
    if mnt_flag & MNT_NOEXEC != 0 {
        return bad1(ndp, Errno::EACCES);
    }

    // SUID programs may not be started with execpromises
    if epp.ep_vap.va_mode & (VSUID | VSGID) != 0
        && p.process().ps_flags.load(Ordering::Relaxed) & PS_EXECPLEDGE != 0
    {
        return bad1(ndp, Errno::EACCES);
    }

    if mnt_flag & MNT_NOSUID != 0 {
        epp.ep_vap.va_mode &= !(VSUID | VSGID);
    }

    // check access. for root we have to see if any exec bit on
    if let Err(error) = VOP_ACCESS(vp, VEXEC, cred, p) {
        return bad1(ndp, error);
    }
    if epp.ep_vap.va_mode & (S_IXUSR | S_IXGRP | S_IXOTH) == 0 {
        return bad1(ndp, Errno::EACCES);
    }

    // try to open it
    if let Err(error) = VOP_OPEN(vp, FREAD, cred, p) {
        return bad1(ndp, error);
    }

    // unlock vp, we need it unlocked from here
    let _ = VOP_UNLOCK(vp);

    // now we have the file, get the exec header
    let mut resid = 0usize;
    let hdrlen = epp.ep_hdr.len();
    let error = vn_rdwr(
        UioRw::UIO_READ,
        vp,
        epp.ep_hdr.as_mut_ptr().cast(),
        hdrlen,
        0,
        UioSeg::UIO_SYSSPACE,
        0,
        cred,
        Some(&mut resid),
        Some(p),
    );
    let error = match error {
        Ok(()) => {
            epp.ep_hdrvalid = hdrlen - resid;
            match check_exec_switch(p, epp) {
                Ok(()) => return Ok(()),
                Err(e) if epp.ep_flags & EXEC_DESTR != 0 => return Err(e),
                Err(e) => {
                    // free any vmspace-creation commands, and release their references
                    epp.ep_vmcmds.kill();
                    exec_free_package(epp);
                    e
                }
            }
        }
        Err(e) => e,
    };

    // bad2: close the vnode, free the pathname buf, and punt.
    let _ = vn_close(vp, FREAD, cred, Some(p));
    namei_pnbuf_put(ndp);
    Err(error)
}

/// The exec switch half of `check_exec`: set up the vmcmds for creation of the process
/// address space, then check the result against the limits. A failure kills the vmcmds
/// and frees the package's loader allocations (not the vnode: the caller's).
fn check_exec_switch(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let mut error = Err(Errno::ENOEXEC);
    for es in &EXECSW {
        if error.is_ok() {
            break;
        }
        let newerror = (es.es_check)(p, epp);
        // make sure the first "interesting" error code is saved.
        if newerror.is_ok() || error == Err(Errno::ENOEXEC) {
            error = newerror;
        }
        if epp.ep_flags & EXEC_DESTR != 0 && error.is_err() {
            return error;
        }
    }
    if error.is_ok() {
        // check that entry point is sane
        if epp.ep_entry > <Machine as VmParam>::VM_MAXUSER_ADDRESS {
            error = Err(Errno::ENOEXEC);
        }

        // check limits
        if epp.ep_tsize > <Machine as VmParam>::MAXTSIZ
            || epp.ep_dsize as u64 > lim_cur(RLIMIT_DATA)
        {
            error = Err(Errno::ENOMEM);
        }

        if error.is_ok() {
            return Ok(());
        }
    }

    // free any vmspace-creation commands, and release their references
    epp.ep_vmcmds.kill();
    exec_free_package(epp);
    error
}

/// exec system call
pub fn sys_execve(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysExecveArgs = sysargs(v);
    execve_common(
        p,
        uap.path.get() as usize,
        uap.argp.get() as usize,
        uap.envp.get() as usize,
        None,
    )
}

/// `sys_execve` of the executable `image` held in kernel memory (see the module's
/// deviations): `path`, `argp` and `envp` are user addresses as for `execve(2)`.
/// `Err(EJUSTRETURN)` is success.
pub fn exec_image(
    p: &Proc,
    path: usize,
    argp: usize,
    envp: usize,
    image: &[u8],
) -> Result<(), Errno> {
    execve_common(p, path, argp, envp, Some(image))
}

/// Copies a NULL-terminated vector of user strings (`argv` or `envp`) at `cpp` into
/// `argbuf` from `*dp` on, at most `ARG_MAX` bytes in all; returns how many strings.
fn copy_strings(cpp: usize, argbuf: &mut [u8], dp: &mut usize) -> Result<usize, Errno> {
    let mut cpp = cpp;
    let mut n = 0;
    loop {
        let len = ARG_MAX - *dp;
        let mut word = [0u8; size_of::<usize>()];
        copyin(cpp, &mut word)?;
        let sp = usize::from_ne_bytes(word);
        if sp == 0 {
            break;
        }
        let len = match copyinstr(sp, &mut argbuf[*dp..*dp + len]) {
            Ok(len) => len,
            Err(Errno::ENAMETOOLONG) => return Err(Errno::E2BIG),
            Err(e) => return Err(e),
        };
        *dp += len;
        cpp += size_of::<usize>();
        n += 1;
    }
    Ok(n)
}

/// The body of `sys_execve`, for a path (`image` `None`) or a memory image.
fn execve_common(
    p: &Proc,
    path: usize,
    uargp: usize,
    uenvp: usize,
    image: Option<&[u8]>,
) -> Result<(), Errno> {
    let mut cred = p.ucred();
    let pr = p.process();

    // Copy into kernel for realpath-like calculations
    let pathname = NameiBuf::get()?;
    copyinstr(path, pathname.as_mut())?;

    let rpbuf = NameiBuf::get_zero()?;

    // Get other threads to stop, if contested return ERESTART, so the syscall is restarted
    // after halting in userret.
    if single_thread_set(p, SINGLE_UNWIND | SINGLE_DEEP).is_err() {
        return Err(Errno::ERESTART);
    }

    // Cheap solution to complicated problems. Mark this process as "leave me alone, I'm
    // execing".
    pr.ps_flags.fetch_or(PS_INEXEC, Ordering::Relaxed);

    let mut nid = ndinit(LOOKUP, NOFOLLOW, NiDirp::Sys(pathname.as_mut()), p);
    nid.ni_pledge = PLEDGE_EXEC;
    nid.ni_unveil = UNVEIL_EXEC;
    nid.ni_cnd.cn_rpbuf = rpbuf.as_ptr();
    nid.ni_cnd.cn_rpi = 0;

    // initialize the fields of the exec package.
    let mut pack = ExecPackage::new(pathname.as_str());
    pack.ep_image = image;

    // freehdr: the header, the pathname and the realpath buffers are dropped by the
    // caller's scope.
    let freehdr = |error: Errno| -> Result<(), Errno> {
        pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
        single_thread_clear(p);
        Err(error)
    };

    // see if we can run it.
    let ndp = if image.is_none() {
        Some(&mut nid)
    } else {
        None
    };
    if let Err(error) = check_exec(p, &mut pack, ndp) {
        return freehdr(error);
    }

    // XXX -- THE FOLLOWING SECTION NEEDS MAJOR CLEANUP

    // allocate an argument buffer
    let Some(argp) = km_alloc(NCARGS, &KV_EXEC, &KP_PAGEABLE, &KD_WAITOK) else {
        panic(format_args!("execve: argp == NULL"));
    };
    // SAFETY: a fresh `NCARGS`-byte pageable allocation from `exec_map`, ours until the
    // `km_free` on every path below; it is only written through this slice.
    let argbuf = unsafe { slice::from_raw_parts_mut(argp.as_ptr(), NCARGS) };
    let mut dp = 0usize;
    let mut argc = 0usize;

    // bad: free the vmspace-creation commands and the package, close the file, free the
    // argument buffer, then freehdr.
    let bad = |pack: &mut ExecPackage<'_>, nid: &mut Nameidata<'_>, error: Errno| {
        // free the vmspace-creation commands, and release their references
        pack.ep_vmcmds.kill();
        // kill any opened file descriptor, if necessary
        if pack.ep_flags & EXEC_HASFD != 0 {
            pack.ep_flags &= !EXEC_HASFD;
            fdplock(p.fd());
            // fdrelease unlocks p->p_fd.
            let _ = fdrelease(p, pack.ep_fd);
        }
        exec_free_package(pack);
        // close and put the exec'd file
        if let Some(vp) = pack.ep_vp {
            let _ = vn_close(vp, FREAD, ptr::from_ref(cred), Some(p));
        }
        namei_pnbuf_put(nid);
        km_free(argp, NCARGS, &KV_EXEC, &KP_PAGEABLE);
        freehdr(error)
    };

    // Copy the fake args list, if there's one, freeing it as we go. exec_script_makecmds()
    // allocates either 2 or 3 fake args bounded by MAXINTERP + MAXPATHLEN < NCARGS so no
    // overflow can happen.
    if pack.ep_flags & EXEC_HASARGL != 0 {
        for fa in core::mem::take(&mut pack.ep_fa) {
            let n = fa.iter().position(|&c| c == 0).unwrap_or(fa.len());
            argbuf[dp..dp + n].copy_from_slice(&fa[..n]);
            argbuf[dp + n] = 0;
            dp += n + 1;
            argc += 1;
        }
        pack.ep_flags &= !EXEC_HASARGL;
    }

    // Now get argv & environment
    if uargp == 0 {
        return bad(&mut pack, &mut nid, Errno::EFAULT);
    }
    let mut cpp = uargp;

    if pack.ep_flags & EXEC_SKIPARG != 0 {
        cpp += size_of::<usize>();
    }

    match copy_strings(cpp, argbuf, &mut dp) {
        Ok(n) => argc += n,
        Err(error) => return bad(&mut pack, &mut nid, error),
    }

    // must have at least one argument
    if argc == 0 {
        return bad(&mut pack, &mut nid, Errno::EINVAL);
    }

    // KTRACE (KTR_EXECARGS): not configured.

    let mut envc = 0;
    // environment does not need to be there
    if uenvp != 0 {
        match copy_strings(uenvp, argbuf, &mut dp) {
            Ok(n) => envc = n,
            Err(error) => return bad(&mut pack, &mut nid, error),
        }
        // KTRACE (KTR_EXECENV): not configured.
    }

    const STACKALIGNBYTES: usize = <Machine as MachineParam>::STACKALIGNBYTES;
    dp = (dp + STACKALIGNBYTES) & !STACKALIGNBYTES;

    // If we have enabled random stackgap, the stack itself has already been moved from a
    // random location, but is still aligned to a page boundary. Provide the lower bits of
    // random placement now.
    let sgap = if stackgap_random.load(Ordering::Relaxed) == 0 {
        0
    } else {
        let sgap = arc4random() as usize & PAGE_MASK;
        (sgap + STACKALIGNBYTES) & !STACKALIGNBYTES
    };

    // Now check if args & environ fit into new stack
    let mut len = (argc + envc + 2 + ELF_AUX_WORDS) * size_of::<usize>()
        + size_of::<i64>()
        + dp
        + sgap
        + PATH_MAX
        + size_of::<PsStrings>();

    len = (len + STACKALIGNBYTES) & !STACKALIGNBYTES;

    if len > pack.ep_ssize {
        // in effect, compare to initial limit
        return bad(&mut pack, &mut nid, Errno::ENOMEM);
    }

    // adjust "active stack depth" for process VSZ
    pack.ep_ssize = len; // maybe should go elsewhere, but...

    // we're committed: any further errors will kill the process, so kill the other threads
    // now.
    let _ = single_thread_set(p, SINGLE_EXIT);

    // Clear profiling state in new image: prof_exec(pr) (subr_prof.c, see the deviations).
    let _ = unported!("execve: prof_exec (subr_prof.c)");

    // Prepare vmspace for remapping. Note that uvmspace_exec can replace ps_vmspace!
    uvmspace_exec(
        p,
        <Machine as VmParam>::VM_MIN_ADDRESS,
        <Machine as VmParam>::VM_MAXUSER_ADDRESS,
    );

    let vm = pr.vmspace();
    // Now map address space
    vm.vm_taddr.set(trunc_page(pack.ep_taddr));
    vm.vm_tsize
        .set(atop(round_page(pack.ep_taddr + pack.ep_tsize) - trunc_page(pack.ep_taddr)) as i32);
    vm.vm_daddr.set(trunc_page(pack.ep_daddr));
    vm.vm_dsize
        .set(atop(round_page(pack.ep_daddr + pack.ep_dsize) - trunc_page(pack.ep_daddr)) as i32);
    vm.vm_dused.set(0);
    vm.vm_ssize.set(atop(round_page(pack.ep_ssize)) as i32);
    vm.vm_maxsaddr.set(pack.ep_maxsaddr);
    vm.vm_minsaddr.set(pack.ep_minsaddr);

    // exec_abort: the old process doesn't exist anymore. exit gracefully. get rid of the
    // (new) address space we have created, if any, get rid of our namei data and vnode, and
    // exit noting failure.
    let exec_abort = |pack: &mut ExecPackage<'_>, nid: &mut Nameidata<'_>| -> ! {
        uvm_unmap(
            &pr.vmspace().vm_map,
            <Machine as VmParam>::VM_MIN_ADDRESS,
            <Machine as VmParam>::VM_MAXUSER_ADDRESS,
        );
        pack.ep_vmcmds.kill();
        exec_free_package(pack);
        namei_pnbuf_put(nid);
        if let Some(vp) = pack.ep_vp {
            let _ = vn_close(vp, FREAD, ptr::from_ref(cred), Some(p));
        }
        km_free(argp, NCARGS, &KV_EXEC, &KP_PAGEABLE);
        // free_pack_abort:
        exit1(p, 0, SIGABRT, EXIT_NORMAL)
        // NOTREACHED
    };

    // create the new process's VM space by running the vmcmds
    #[cfg(feature = "diagnostic")]
    if pack.ep_vmcmds.evs_cmds.is_empty() {
        panic(format_args!("execve: no vmcmds"));
    }
    if exec_process_vmcmds(p, &mut pack).is_err() {
        // if an error happened, deallocate and punt
        exec_abort(&mut pack, &mut nid);
    }

    // MACHINE_STACK_GROWS_UP: neither amd64 nor arm64.
    pr.ps_strings
        .set(vm.vm_minsaddr.get() - sgap - PATH_MAX - size_of::<PsStrings>());
    pack.ep_execpath = vm.vm_minsaddr.get() - sgap - PATH_MAX;
    if uvm_map_protect(
        &vm.vm_map,
        round_page(pack.ep_execpath + PATH_MAX),
        vm.vm_minsaddr.get(),
        PROT_NONE,
        0,
        true,
        false,
    )
    .is_err()
    {
        exec_abort(&mut pack, &mut nid);
    }

    // remember information about the process
    let mut arginfo = PsStrings {
        ps_nargvstr: argc as i32,
        ps_nenvstr: envc as i32,
        ..PsStrings::default()
    };

    let stack = vm.vm_minsaddr.get() - len;
    // Now copy argc, args & environ to new stack
    if !copyargs(&mut pack, &mut arginfo, stack, argbuf) {
        exec_abort(&mut pack, &mut nid);
    }

    pr.ps_auxinfo.set(pack.ep_auxinfo);

    // copy out the process's ps_strings structure
    if copyout(&arginfo.to_bytes(), pr.ps_strings.get()).is_err() {
        exec_abort(&mut pack, &mut nid);
    }
    if nid.ni_cnd.cn_rpi != 0 {
        if copyoutstr(&rpbuf.as_mut()[..PATH_MAX], pack.ep_execpath).is_err() {
            exec_abort(&mut pack, &mut nid);
        }
    } else {
        pack.ep_execpath = 0;
    }

    pin_free(&pr.ps_pin);
    match pack.ep_pins.take() {
        Some(pins) if pack.ep_npins != 0 => {
            pr.ps_pin.pn_start.set(pack.ep_pinstart);
            pr.ps_pin.pn_end.set(pack.ep_pinend);
            pr.ps_pin.pn_pins.set(pins.as_ptr());
            pr.ps_pin.pn_npins.set(pack.ep_npins);
        }
        other => {
            pack.ep_pins = other;
            pr.ps_pin.pn_start.set(0);
            pr.ps_pin.pn_end.set(0);
            pr.ps_pin.pn_pins.set(ptr::null_mut());
            pr.ps_pin.pn_npins.set(0);
        }
    }
    if !pr.ps_libcpin.pn_pins.get().is_null() {
        pin_free(&pr.ps_libcpin);
        pr.ps_libcpin.pn_start.set(0);
        pr.ps_libcpin.pn_end.set(0);
        pr.ps_libcpin.pn_npins.set(0);
    }

    // stopprofclock(pr): stop profiling (subr_prof.c, see the deviations).
    fdprepforexec(p); // handle close on exec and close on fork
    execsigs(p); // reset caught signals
    tcb_set(p, 0); // reset the TCB address
    pr.ps_kbind_addr.set(0); // reset the kbind bits
    pr.ps_kbind_cookie.set(0);
    let mut cookie = [0u8; 8];
    arc4random_buf(&mut cookie);
    pr.ps_sigcookie.set(u64::from_ne_bytes(cookie));

    // set command name & other accounting info
    pr.set_comm(exec_comm(&nid, pathname.as_str()));
    pr.ps_acflag.set(pr.ps_acflag.get() & !AFORK);

    // record proc's vnode, for use by sysctl
    let otvp = pr.ps_textvp.get();
    if let Some(vp) = pack.ep_vp {
        vref(vp);
    }
    pr.ps_textvp.set(pack.ep_vp);
    if let Some(otvp) = otvp {
        vrele(otvp);
    }

    let mut iflags = pr.ps_iflags.get() & !(PSI_NOBTCFI | PSI_PROFILE | PSI_WXNEEDED);
    if pack.ep_flags & EXEC_NOBTCFI != 0 {
        iflags |= PSI_NOBTCFI;
    }
    if pack.ep_flags & EXEC_PROFILE != 0 {
        iflags |= PSI_PROFILE;
    }
    if pack.ep_flags & EXEC_WXNEEDED != 0 {
        iflags |= PSI_WXNEEDED;
    }
    pr.ps_iflags.set(iflags);

    pr.ps_flags.fetch_or(PS_EXEC, Ordering::Relaxed);
    if pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0 {
        pr.ps_flags.fetch_and(!PS_PPWAIT, Ordering::Relaxed);
        // SAFETY: a process's parent is live while the child is.
        if let Some(pptr) = unsafe { pr.ps_pptr.get().as_ref() } {
            pptr.ps_flags.fetch_and(!PS_ISPWAIT, Ordering::Relaxed);
            pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
            wakeup(ptr::from_ref(pptr));
        }
    }

    // If process does execve() while it has a mismatched real, effective, or saved uid/gid,
    // we set PS_SUGIDEXEC.
    if cred.cr_uid.get() != cred.cr_ruid.get()
        || cred.cr_uid.get() != cred.cr_svuid.get()
        || cred.cr_gid.get() != cred.cr_rgid.get()
        || cred.cr_gid.get() != cred.cr_svgid.get()
    {
        pr.ps_flags.fetch_or(PS_SUGIDEXEC, Ordering::Relaxed);
    } else {
        pr.ps_flags.fetch_and(!PS_SUGIDEXEC, Ordering::Relaxed);
    }

    if pr.ps_flags.load(Ordering::Relaxed) & PS_EXECPLEDGE != 0 {
        p.p_pledge.set(pr.ps_execpledge.get());
        pr.ps_pledge.set(pr.ps_execpledge.get());
        pr.ps_flags.fetch_or(PS_PLEDGE, Ordering::Relaxed);
    } else {
        pr.ps_flags.fetch_and(!PS_PLEDGE, Ordering::Relaxed);
        p.p_pledge.set(0);
        pr.ps_pledge.set(0);
        // Clear our unveil paths out so the child starts afresh: unveil_destroy(pr) and
        // ps_uvdone = 0 (kern_unveil.c, not ported: no process has unveiled paths).
    }

    // deal with set[ug]id. MNT_NOEXEC has already been used to disable s[ug]id.
    let attr = pack.ep_vap;
    if attr.va_mode & (VSUID | VSGID) != 0 && proc_cansugid(p) {
        pr.ps_flags
            .fetch_or(PS_SUGID | PS_SUGIDEXEC, Ordering::Relaxed);

        // KTRACE: not configured.
        cred = crcopy(cred);
        p.p_ucred.set(cred);
        if attr.va_mode & VSUID != 0 {
            cred.cr_uid.set(attr.va_uid);
        }
        if attr.va_mode & VSGID != 0 {
            cred.cr_gid.set(attr.va_gid);
        }

        // For set[ug]id processes, a few caveats apply to stdin, stdout, and stderr.
        let mut error = Ok(());
        let fdp = p.fd();
        fdplock(fdp);
        for i in 0..3 {
            // NOTE - This will never return NULL because of immature fds. The file
            // descriptor table is not shared because we're suid.
            let fp = match fd_getfile(fdp, i) {
                Some(fp) => fp,
                None => {
                    // Ensure that stdin, stdout, and stderr are already allocated. We do
                    // not want userland to accidentally allocate descriptors in this range
                    // which has implied meaning to libc.
                    let (fp, indx) = match falloc(p) {
                        Ok(r) => r,
                        Err(e) => {
                            error = Err(e);
                            break;
                        }
                    };
                    #[cfg(feature = "diagnostic")]
                    if indx != i {
                        panic(format_args!("sys_execve: falloc indx != i"));
                    }
                    // cdevvp(getnulldev(), &vp), VOP_OPEN, the DTYPE_VNODE file: the device
                    // switch is not ported (see the module's deviations).
                    fdremove(fdp, indx);
                    let _ = closef(fp, p);
                    error = Err(unported!("sys_execve: cdevvp(getnulldev()) (<sys/conf.h>)"));
                    break;
                }
            };
            let _ = frele(fp, p);
        }
        fdpunlock(fdp);
        if error.is_err() {
            exec_abort(&mut pack, &mut nid);
        }
    } else {
        pr.ps_flags.fetch_and(!PS_SUGID, Ordering::Relaxed);
    }

    // Reset the saved ugids and update the process's copy of the creds if the creds have
    // been changed
    if cred.cr_uid.get() != cred.cr_svuid.get() || cred.cr_gid.get() != cred.cr_svgid.get() {
        // make sure we have unshared ucreds
        cred = crcopy(cred);
        p.p_ucred.set(cred);
        cred.cr_svuid.set(cred.cr_uid.get());
        cred.cr_svgid.set(cred.cr_gid.get());
    }

    if !ptr::eq(pr.ps_ucred.get(), cred) {
        let ocred = pr.ucred();
        crhold(cred);
        pr.ps_ucred.set(cred);
        crfree(ocred);
    }

    if pr.ps_flags.load(Ordering::Relaxed) & PS_SUGIDEXEC != 0 {
        cancel_all_itimers();
    }

    // reset CPU time usage for the thread, but not the process
    p.p_tu.tu_runtime.set(Timespec::new(0, 0));
    for ticks in &p.p_tu.tu_ticks {
        ticks.set(0);
    }
    // pc_lock_init(&p->p_tu.tu_pcl): the lock is statically initialised.

    p.set_name(b"");

    km_free(argp, NCARGS, &KV_EXEC, &KP_PAGEABLE);

    namei_pnbuf_put(&mut nid);
    if let Some(vp) = pack.ep_vp {
        let _ = vn_close(vp, FREAD, ptr::from_ref(cred), Some(p));
    }

    // notify others that we exec'd: knote(&pr->ps_klist, NOTE_EXEC) (kern_event.c).
    let _ = unported!("exec: knote NOTE_EXEC (kern_event.c)");

    // free_pack_abort: the package and the pathname buffers are dropped; exit noting
    // failure.
    let free_pack_abort = || -> ! { exit1(p, 0, SIGABRT, EXIT_NORMAL) };

    // map the process's timekeep page, needs to be before exec_elf_fixup
    if exec_timekeep_map(pr).is_err() {
        free_pack_abort();
    }

    // setup new registers and do misc. setup.
    if exec_elf_fixup(p, &mut pack).is_err() {
        free_pack_abort();
    }
    Machine::setregs(p, &pack, Vaddr::new(stack), &arginfo);

    // map the process's signal trampoline code
    if exec_sigcode_map(pr).is_err() {
        free_pack_abort();
    }

    // __HAVE_EXEC_MD_MAP: neither amd64 nor arm64.

    if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        psignal(p, SIGTRAP);
    }

    // free(pack.ep_hdr), the pathname and realpath buffers: dropped on return.

    p.p_descfd.set(255);
    if pack.ep_flags & EXEC_HASFD != 0 && pack.ep_fd < 255 {
        p.p_descfd.set(pack.ep_fd as u8);
    }

    pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
    single_thread_clear(p);

    // setregs() sets up all the registers, so just 'return'
    Err(Errno::EJUSTRETURN)
}

/// The command name `strlcpy(pr->ps_comm, nid.ni_cnd.cn_nameptr, ...)` copies: the last
/// component `namei` left in `cn_nameptr`, or the path's last component for a memory image.
fn exec_comm<'b>(nid: &Nameidata<'_>, path: &'b [u8]) -> &'b [u8] {
    let ptr = nid.ni_cnd.cn_nameptr;
    let pnbuf = nid.ni_cnd.cn_pnbuf;
    if !ptr.is_null() && !pnbuf.is_null() {
        let start = ptr as usize - pnbuf as usize;
        if start < MAXPATHLEN {
            // SAFETY: `cn_nameptr` points into `cn_pnbuf`, the `MAXPATHLEN`-byte buffer
            // namei kept (`SAVENAME`), which lives until `namei_pnbuf_put`.
            let rest = unsafe { slice::from_raw_parts(ptr, MAXPATHLEN - start) };
            let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
            // SAFETY: as above; the slice is copied by `set_comm` before the buffer goes.
            return unsafe { slice::from_raw_parts(ptr, n) };
        }
    }
    path.rsplit(|&c| c == b'/').next().unwrap_or(path)
}

/// `copyargs`: copies `argc`, the argument and environment pointers and strings (from the
/// kernel buffer `argp`) to the new stack at `stack`, records where they are in `arginfo`
/// and where the auxiliary vector goes in `pack.ep_auxinfo`. `false` when a copy failed.
pub fn copyargs(
    pack: &mut ExecPackage<'_>,
    arginfo: &mut PsStrings,
    stack: usize,
    argp: &[u8],
) -> bool {
    const PTR: usize = size_of::<usize>();
    let mut cpp = stack;
    let mut argc = arginfo.ps_nargvstr as i64;
    let mut envc = arginfo.ps_nenvstr;
    let nullp = 0usize;

    if copyout(&argc.to_ne_bytes(), cpp).is_err() {
        return false;
    }
    cpp += PTR;

    let mut dp = cpp + (argc as usize + envc as usize + 2 + ELF_AUX_WORDS) * PTR;
    let mut sp = 0usize;

    // XXX don't copy them out, remap them!
    arginfo.ps_argvstr = cpp; // remember location of argv for later

    let one = |cpp: &mut usize, dp: &mut usize, sp: &mut usize| -> bool {
        if copyout(&dp.to_ne_bytes(), *cpp).is_err() {
            return false;
        }
        *cpp += PTR;
        let end = (*sp + ARG_MAX).min(argp.len());
        match copyoutstr(&argp[*sp..end], *dp) {
            Ok(len) => {
                *sp += len;
                *dp += len;
                true
            }
            Err(_) => false,
        }
    };

    while argc > 0 {
        argc -= 1;
        if !one(&mut cpp, &mut dp, &mut sp) {
            return false;
        }
    }

    if copyout(&nullp.to_ne_bytes(), cpp).is_err() {
        return false;
    }
    cpp += PTR;

    arginfo.ps_envstr = cpp; // remember location of envp for later

    while envc > 0 {
        envc -= 1;
        if !one(&mut cpp, &mut dp, &mut sp) {
            return false;
        }
    }

    if copyout(&nullp.to_ne_bytes(), cpp).is_err() {
        return false;
    }
    cpp += PTR;

    pack.ep_auxinfo = cpp; // remember where auxinfo will be placed
    true
}

/// `exec_sigcode_map`: map the signal trampoline into `pr`'s new address space, creating
/// the shared `sigobject` the first time.
pub fn exec_sigcode_map(pr: &Process) -> Result<(), Errno> {
    let sigcode = <Machine as MachineSignal>::sigcode();
    let sz = sigcode.len();

    // If we don't have a sigobject yet, create one.
    //
    // sigobject is an anonymous memory object (just like SYSV shared memory) that we keep a
    // permanent reference to and that we map in all processes that need this sigcode. The
    // creation is simple, we create an object, map it in kernel space, copy out the sigcode
    // to it and map it PROT_READ such that the coredump code can write it out into core
    // dumps. Then we map it with PROT_EXEC into the process just the way sys_mmap would map
    // it.
    // SAFETY: a non-null `sigobject` is the aobj created below, which is never freed.
    let existing = unsafe { SIGOBJECT.load(Ordering::Acquire).as_ref() };
    let sigobject: &'static UvmObject = match existing {
        Some(obj) => obj,
        None => {
            let sigfill = <Machine as MachineSignal>::sigfill();

            // permanent reference
            let Some(obj) = uao_create(Vsize::new(sz), 0) else {
                panic(format_args!("can't create sigobject"));
            };

            let mut va = 0usize;
            if uvm_map(
                kernel_map(),
                &mut va,
                round_page(sz),
                Some(obj),
                0,
                0,
                uvm_mapflag(
                    PROT_READ | PROT_WRITE,
                    PROT_READ | PROT_WRITE,
                    MAP_INHERIT_SHARE,
                    MADV_RANDOM,
                    0,
                ),
            )
            .is_err()
            {
                panic(format_args!("can't map sigobject"));
            }

            let mut off = 0;
            let mut left = round_page(sz);
            while left != 0 {
                let chunk = left.min(sigfill.len());
                // SAFETY: `[va, va + round_page(sz))` is the fresh, writable kernel mapping of
                // `sigobject` made above; its pages are faulted in on first touch.
                unsafe { ptr::copy_nonoverlapping(sigfill.as_ptr(), (va + off) as *mut u8, chunk) };
                left -= chunk;
                off += sigfill.len();
            }
            // SAFETY: as above; `sz <= round_page(sz)`.
            unsafe { ptr::copy_nonoverlapping(sigcode.as_ptr(), va as *mut u8, sz) };

            if uvm_map_protect(
                kernel_map(),
                va,
                round_page(va + sz),
                PROT_READ,
                0,
                false,
                false,
            )
            .is_err()
            {
                panic(format_args!("can't write-protect sigobject"));
            }

            SIGCODE_VA.store(va, Ordering::Relaxed);
            SIGCODE_SZ.store(round_page(sz), Ordering::Relaxed);
            SIGOBJECT.store(ptr::from_ref(obj).cast_mut(), Ordering::Release);
            obj
        }
    };

    pr.ps_sigcode.set(0); // no hint
    uao_reference(sigobject);
    let map = &pr.vmspace().vm_map;
    let mut addr = pr.ps_sigcode.get();
    if uvm_map(
        map,
        &mut addr,
        round_page(sz),
        Some(sigobject),
        0,
        0,
        uvm_mapflag(
            PROT_EXEC,
            PROT_READ | PROT_WRITE | PROT_EXEC,
            MAP_INHERIT_COPY,
            MADV_RANDOM,
            UVM_FLAG_COPYONW,
        ),
    )
    .is_err()
    {
        uao_detach(sigobject);
        return Err(Errno::ENOMEM);
    }
    pr.ps_sigcode.set(addr);
    let _ = uvm_map_immutable(map, addr, addr + round_page(sz), true);

    // Calculate PC at point of sigreturn entry
    pr.ps_sigcoderet
        .set(addr + <Machine as MachineSignal>::sigcoderet());

    Ok(())
}

/// `exec_timekeep_map`: map the shared timekeep page into `pr`'s new address space,
/// creating (and wiring) the kernel's `timekeep` the first time.
pub fn exec_timekeep_map(pr: &Process) -> Result<(), Errno> {
    let timekeep_sz = round_page(size_of::<Timekeep>());

    // Similar to the sigcode object
    // SAFETY: a non-null `timekeep_object` is the aobj created below, never freed.
    let existing = unsafe { TIMEKEEP_OBJECT.load(Ordering::Acquire).as_ref() };
    let object: &'static UvmObject = match existing {
        Some(obj) => obj,
        None => {
            let mut va = 0usize;

            let Some(obj) = uao_create(Vsize::new(timekeep_sz), 0) else {
                return Err(Errno::ENOMEM);
            };
            uao_reference(obj);

            if uvm_map(
                kernel_map(),
                &mut va,
                timekeep_sz,
                Some(obj),
                0,
                0,
                uvm_mapflag(
                    PROT_READ | PROT_WRITE,
                    PROT_READ | PROT_WRITE,
                    MAP_INHERIT_SHARE,
                    MADV_RANDOM,
                    0,
                ),
            )
            .is_err()
            {
                uao_detach(obj);
                return Err(Errno::ENOMEM);
            }
            if uvm_fault_wire(kernel_map(), va, va + timekeep_sz, PROT_READ | PROT_WRITE).is_err() {
                uvm_unmap(kernel_map(), va, va + timekeep_sz);
                uao_detach(obj);
                return Err(Errno::ENOMEM);
            }

            let tk = va as *mut Timekeep;
            // SAFETY: `va` is the fresh, wired, page-aligned kernel mapping made above, large
            // enough for a `Timekeep`; nothing else knows it until the store below.
            unsafe {
                tk.write(Timekeep::default());
                (*tk).tk_version = TK_VERSION;
            }
            TIMEKEEP.store(tk, Ordering::Release);
            TIMEKEEP_OBJECT.store(ptr::from_ref(obj).cast_mut(), Ordering::Release);
            obj
        }
    };

    pr.ps_timekeep.set(0); // no hint
    uao_reference(object);
    let map = &pr.vmspace().vm_map;
    let mut addr = pr.ps_timekeep.get();
    if uvm_map(
        map,
        &mut addr,
        timekeep_sz,
        Some(object),
        0,
        0,
        uvm_mapflag(PROT_READ, PROT_READ, MAP_INHERIT_COPY, MADV_RANDOM, 0),
    )
    .is_err()
    {
        uao_detach(object);
        return Err(Errno::ENOMEM);
    }
    pr.ps_timekeep.set(addr);
    let _ = uvm_map_immutable(map, addr, addr + timekeep_sz, true);

    Ok(())
}

#[cfg(test)]
mod tests;
