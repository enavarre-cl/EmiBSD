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

//! `kern_exec.c`: the exec system call: checks the executable against the exec switch,
//! builds the new address space, copies the arguments and sets the registers.
//!
//! Upstream: sys/kern/kern_exec.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports the exec switch (`execsw`), `check_exec` and
//! the body of `sys_execve` from the point where the executable is in hand, as
//! [`exec_image`], for an executable that is a memory image (the `init` Limine module).
//! `sys_execve` itself (`namei`, the `NCARGS` argument copying, `copyargs` with a real
//! `argv`/`envp`, the timekeep page, `exec_md_map`, ptrace) and `exec_free_package` come
//! with the filesystems (M7). With `kern_prot.c`, `exec_image` does the credentials part
//! (`PS_SUGIDEXEC`, `PS_SUGID`, the saved ids reset through `crcopy`, the process's copy of
//! the credentials) and `TCB_SET(p, NULL)`; with `kern_descrip.c` `fdprepforexec`; with
//! `kern_sig.c` the signal part: `single_thread_set`/`single_thread_clear` around the exec,
//! `execsigs`, the new `ps_sigcookie`, the `PS_PPWAIT` wakeup of a vfork parent and
//! `exec_sigcode_map`, which maps the signal trampoline (`machine::MachineSignal::sigcode`)
//! into the new image.
//!
//! ## Deviations
//! - `exec_image(p, name, image)` is this port's name for "`sys_execve` of a memory image":
//!   there is no vnode, so `check_exec` skips `namei`, the attribute, mount and access
//!   checks and `vn_rdwr`; `ep_hdr` is the whole image. Like `sys_execve` it returns
//!   `EJUSTRETURN` on success: `setregs` set the registers, the caller just returns to
//!   user mode.
//! - `copyargs` lays out an empty `argv`/`envp` (`argc` 0, two NULL terminators) and no
//!   auxiliary vector (static executables only); `ps_strings` is written below
//!   `vm_minsaddr` as in C; `ep_execpath` is a local (nothing copies the path out yet).
//! - `MAXTSIZ` is checked; the data limit uses `DFLDSIZ` (`limit0`) until `lim_cur` (M6-c).
//! - A memory image has no vnode attributes, so the set[ug]id branch (`VSUID`/`VSGID`,
//!   `proc_cansugid`, the stdin/stdout/stderr fix-up) is never taken: `PS_SUGID` is cleared
//!   as in the C's else branch. `cancel_all_itimers` for a `PS_SUGIDEXEC` exec is reported
//!   (`kern_time.c`); the pledge/unveil reset waits for `pledge(2)`/`unveil(2)`.
//! - The 4-clause licence (advertising clause) was accepted by the user at M2 for this
//!   project.

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::dev::rnd::arc4random_buf;
use crate::kern::exec_elf::exec_elf_makecmds;
use crate::kern::exec_subr::exec_process_vmcmds;
use crate::kern::kern_descrip::fdprepforexec;
use crate::kern::kern_exit::exit1;
use crate::kern::kern_prot::{crcopy, crfree, crhold};
use crate::kern::kern_sig::{execsigs, psignal, single_thread_clear, single_thread_set};
use crate::kern::kern_synch::wakeup;
use crate::kern::subr_prf::panic;
use crate::machine::copy::copyout;
use crate::machine::cpu::Cpu;
use crate::machine::param::MachineParam;
use crate::machine::signal::MachineSignal;
use crate::machine::tcb::tcb_set;
use crate::machine::{Machine, VmParam};
use crate::sys::acct::AFORK;
use crate::sys::errno::Errno;
use crate::sys::exec::{ExecPackage, Execsw, PsStrings};
use crate::sys::exec_elf::ElfEhdr;
use crate::sys::mman::{
    MADV_RANDOM, MAP_INHERIT_COPY, MAP_INHERIT_SHARE, PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE,
};
use crate::sys::proc::{
    EXIT_NORMAL, PS_EXEC, PS_INEXEC, PS_ISPWAIT, PS_PPWAIT, PS_SUGID, PS_SUGIDEXEC, PS_TRACED,
    PS_WAITEVENT, Proc, Process, SINGLE_DEEP, SINGLE_EXIT, SINGLE_UNWIND,
};
use crate::sys::signal::{SIGABRT, SIGTRAP};
use crate::sys::syslimits::PATH_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Register, Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_aobj::{uao_create, uao_detach, uao_reference};
use crate::uvm::uvm_extern::{UVM_FLAG_COPYONW, uvm_mapflag};
use crate::uvm::uvm_km::kernel_map;
use crate::uvm::uvm_map::{uvm_map, uvm_map_immutable, uvm_map_protect, uvmspace_exec};
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_param::{atop, round_page, trunc_page};

/// `sigobject`: the shared sigcode object, created by the first `exec_sigcode_map` and
/// referenced forever after.
static SIGOBJECT: AtomicPtr<UvmObject> = AtomicPtr::new(ptr::null_mut());
/// `sigcode_va`: where `sigobject` is mapped (read-only) in `kernel_map`.
pub static SIGCODE_VA: AtomicUsize = AtomicUsize::new(0);
/// `sigcode_sz`: the size of that mapping.
pub static SIGCODE_SZ: AtomicUsize = AtomicUsize::new(0);

/// `execsw[]`: the executable formats, in the order they are tried.
pub static EXECSW: [Execsw; 1] = [Execsw {
    es_hdrsz: size_of::<ElfEhdr>(),
    es_check: exec_elf_makecmds,
}];

/// `check_exec`: prepare an execve package for an executable: run the exec switch over its
/// header to fill in the vmcmds and addresses, and check the result against the limits.
///
/// EXEC SWITCH ENTRY: locked vnode to check, exec package, proc.
/// EXEC SWITCH EXIT: ok: filled exec package. error: destructive: everything deallocated
/// except exec header; non-destructive: error code, exec header unmodified.
pub fn check_exec(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    // namei, the regular-file, attribute, mount-point, SUID/pledge and access checks,
    // VOP_OPEN and the header read: the executable is a memory image (see the module's
    // deviations); `ep_hdr` holds all of it (`ep_hdrvalid` is its length).

    // set up the vmcmds for creation of the process address space
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
        // EXEC_DESTR: no destructive format yet.
    }
    if error.is_ok() {
        // check that entry point is sane
        if epp.ep_entry > <Machine as VmParam>::VM_MAXUSER_ADDRESS {
            error = Err(Errno::ENOEXEC);
        }

        // check limits (lim_cur(RLIMIT_DATA): see the module's deviations)
        if epp.ep_tsize > <Machine as VmParam>::MAXTSIZ
            || epp.ep_dsize > <Machine as VmParam>::DFLDSIZ
        {
            error = Err(Errno::ENOMEM);
        }

        if error.is_ok() {
            return Ok(());
        }
    }

    // free any vmspace-creation commands, and release their references
    epp.ep_vmcmds.kill();
    // exec_free_package, vn_close, the pathname buffer: nothing to release.
    error
}

/// `copyargs`: copies `argc`, the argument and environment pointers and the strings to the
/// new stack at `stack`, and records where they are in `arginfo`. Here the vectors are
/// empty (see the module's deviations): `argc` 0, a NULL `argv` terminator and a NULL
/// `envp` terminator, `ps_argvstr`/`ps_envstr` pointing at each.
fn copyargs(_pack: &ExecPackage<'_>, arginfo: &mut PsStrings, stack: usize) -> Result<(), Errno> {
    let argc: Register = 0;
    let null: Register = 0;
    let mut cpp = stack;

    copyout(&argc.to_ne_bytes(), cpp)?;
    cpp += size_of::<Register>();

    arginfo.ps_argvstr = cpp;
    // (no argument strings) ... the NULL terminator
    copyout(&null.to_ne_bytes(), cpp)?;
    cpp += size_of::<Register>();

    arginfo.ps_envstr = cpp;
    // (no environment strings) ... the NULL terminator
    copyout(&null.to_ne_bytes(), cpp)?;

    // The auxiliary vector (ELF_AUX_ENTRIES): with ld.so (M7).
    Ok(())
}

/// The body of `sys_execve` for an executable that is a memory image (see the module's
/// deviations): checks it, replaces `p`'s address space with the new program's, lays out
/// the (empty) arguments and `ps_strings`, names the process `name` and sets the
/// registers. `Err(EJUSTRETURN)` is success.
pub fn exec_image(p: &Proc, name: &[u8], image: &[u8]) -> Result<(), Errno> {
    let mut cred = p.ucred();
    let pr = p.process();

    // Get other threads to stop, if contested return ERESTART, so the syscall is restarted
    // after halting in userret.
    if single_thread_set(p, SINGLE_UNWIND | SINGLE_DEEP).is_err() {
        return Err(Errno::ERESTART);
    }

    // Cheap solution to complicated problems. Mark this process as "leave me alone, I'm
    // execing".
    pr.ps_flags.fetch_or(PS_INEXEC, Ordering::Relaxed);

    // initialize the fields of the exec package
    let mut pack = ExecPackage::new(name, image);

    // see if we can run it.
    if let Err(e) = check_exec(p, &mut pack) {
        // freehdr:
        pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
        single_thread_clear(p);
        return Err(e);
    }

    // allocate an argument buffer, copy the arguments and the environment: nothing to
    // copy (see the module's deviations). The stack layout is the C's: the vectors, then
    // ps_strings and PATH_MAX for the execpath, below vm_minsaddr.
    let argc = 0usize;
    let envc = 0usize;
    let sgap = 0usize;
    let dp = 0usize;
    let len = ((argc + envc + 2) * size_of::<usize>()
        + size_of::<Register>()
        + dp
        + sgap
        + size_of::<PsStrings>()
        + PATH_MAX)
        & !<Machine as MachineParam>::STACKALIGNBYTES;

    if len > pack.ep_ssize {
        // in effect, compare to initial limit
        pack.ep_vmcmds.kill();
        // bad: ... freehdr:
        pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
        single_thread_clear(p);
        return Err(Errno::ENOMEM);
    }
    // adjust "active stack depth" for process VSZ
    pack.ep_ssize = len; // maybe should go elsewhere, but...

    // we're committed: any further errors will kill the process, so kill the other threads
    // now.
    let _ = single_thread_set(p, SINGLE_EXIT);

    // Clear profiling state in new image: prof_exec (M6-c).

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

    // create the new process's VM space by running the vmcmds
    #[cfg(feature = "diagnostic")]
    if pack.ep_vmcmds.evs_cmds.is_empty() {
        crate::kern::subr_prf::panic(format_args!("execve: no vmcmds"));
    }
    if exec_process_vmcmds(p, &mut pack).is_err() {
        // if an error happened, deallocate and punt
        exec_abort(p, &mut pack);
    }

    // MACHINE_STACK_GROWS_UP: neither amd64 nor arm64.
    pr.ps_strings
        .set(vm.vm_minsaddr.get() - sgap - PATH_MAX - size_of::<PsStrings>());
    let ep_execpath = vm.vm_minsaddr.get() - sgap - PATH_MAX;
    if uvm_map_protect(
        &vm.vm_map,
        round_page(ep_execpath + PATH_MAX),
        vm.vm_minsaddr.get(),
        PROT_NONE,
        0,
        true,
        false,
    )
    .is_err()
    {
        exec_abort(p, &mut pack);
    }

    // remember information about the process
    let mut arginfo = PsStrings {
        ps_nargvstr: argc as i32,
        ps_nenvstr: envc as i32,
        ..PsStrings::default()
    };

    let stack = vm.vm_minsaddr.get() - len;
    // Now copy argc, args & environ to new stack
    if copyargs(&pack, &mut arginfo, stack).is_err() {
        exec_abort(p, &mut pack);
    }

    // ps_auxinfo: with the auxiliary vector (M7).

    // copy out the process's ps_strings structure
    if copyout(&arginfo.to_bytes(), pr.ps_strings.get()).is_err() {
        exec_abort(p, &mut pack);
    }
    // the execpath (copyoutstr): no realpath yet.

    // the pin tables (ps_pin, ps_libcpin): M7.

    // stopprofclock(pr): subr_prof.c (M7); nothing profiles yet.
    fdprepforexec(p); // handle close on exec and close on fork
    execsigs(p); // reset caught signals
    tcb_set(p, 0); // reset the TCB address
    pr.ps_kbind_addr.set(0); // reset the kbind bits
    pr.ps_kbind_cookie.set(0);
    let mut cookie = [0u8; 8];
    arc4random_buf(&mut cookie);
    pr.ps_sigcookie.set(u64::from_ne_bytes(cookie));

    // set command name & other accounting info
    pr.set_comm(name);
    pr.ps_acflag.set(pr.ps_acflag.get() & !AFORK);

    // record proc's vnode, for use by sysctl: no vnode.

    // ps_iflags (PSI_NOBTCFI, PSI_PROFILE, PSI_WXNEEDED): with the flags (M6-c).

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

    // PS_EXECPLEDGE / ps_pledge and unveil_destroy: with pledge(2) and unveil(2).

    // deal with set[ug]id. An image has no vnode attributes, so no VSUID/VSGID bits and no
    // proc_cansugid: the C's else branch.
    pr.ps_flags.fetch_and(!PS_SUGID, Ordering::Relaxed);

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
        // cancel_all_itimers(): kern_time.c's interval timers (M6-c).
        let _ = unported!("exec: cancel_all_itimers (kern_time.c)");
    }

    // reset CPU time usage for the thread, but not the process
    p.p_tu.tu_runtime.set(Timespec::new(0, 0));
    for ticks in &p.p_tu.tu_ticks {
        ticks.set(0);
    }
    // pc_lock_init(&p->p_tu.tu_pcl): the lock is statically initialised.

    p.set_name(b"");

    // km_free(argp), the pathname buffers, vn_close: nothing allocated.

    // notify others that we exec'd: knote(&pr->ps_klist, NOTE_EXEC) (kern_event.c).
    let _ = unported!("exec: knote NOTE_EXEC (kern_event.c)");

    // map the process's timekeep page (exec_timekeep_map), exec_elf_fixup: M7.

    // setup new registers and do misc. setup.
    Machine::setregs(p, &pack, Vaddr::new(stack), &arginfo);

    // map the process's signal trampoline code
    if exec_sigcode_map(pr).is_err() {
        exec_abort(p, &mut pack);
    }

    // __HAVE_EXEC_MD_MAP: neither amd64 nor arm64.

    if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        psignal(p, SIGTRAP);
    }

    // p_descfd: EXEC_HASFD never set here.

    pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
    single_thread_clear(p);

    // setregs() sets up all the registers, so just 'return'
    Err(Errno::EJUSTRETURN)
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

/// `exec_abort:`: the old process is dead: kill it.
fn exec_abort(p: &Proc, pack: &mut ExecPackage<'_>) -> ! {
    // free the vmspace-creation commands, and release their references
    pack.ep_vmcmds.kill();
    // the opened file descriptor (EXEC_HASFD), the buffers: none.

    exit1(p, 0, SIGABRT, EXIT_NORMAL)
    // NOTREACHED
}
