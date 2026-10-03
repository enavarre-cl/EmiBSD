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
//! `argv`/`envp`, the credentials, `fdprepforexec`, `execsigs`, the signal trampoline and
//! the timekeep page, `exec_md_map`, ptrace), `exec_free_package` and `exec_sigcode_map`
//! come with the file descriptor table and the filesystems (M6-c, M7).
//!
//! ## Deviations
//! - `exec_image(p, name, image)` is this port's name for "`sys_execve` of a memory image":
//!   there is no vnode, so `check_exec` skips `namei`, the attribute, mount and access
//!   checks and `vn_rdwr`; `ep_hdr` is the whole image. Like `sys_execve` it returns
//!   `EJUSTRETURN` on success: `setregs` set the registers, the caller just returns to
//!   user mode.
//! - `copyargs` lays out an empty `argv`/`envp` (`argc` 0, two NULL terminators) and no
//!   auxiliary vector (static executables only); `ps_strings` is written below
//!   `vm_minsaddr` as in C, `ep_execpath` and the `PROT_NONE` cover of the top of the stack
//!   (`uvm_map_protect`) wait for M7a.
//! - `MAXTSIZ` is checked; the data limit uses `DFLDSIZ` (`limit0`) until `lim_cur` (M6-c).
//! - The 4-clause licence (advertising clause) was accepted by the user at M2 for this
//!   project.

use core::sync::atomic::Ordering;

use crate::kern::exec_elf::exec_elf_makecmds;
use crate::kern::exec_subr::exec_process_vmcmds;
use crate::kern::kern_exit::exit1;
use crate::machine::copy::copyout;
use crate::machine::cpu::Cpu;
use crate::machine::param::MachineParam;
use crate::machine::{Machine, VmParam};
use crate::sys::acct::AFORK;
use crate::sys::errno::Errno;
use crate::sys::exec::{ExecPackage, Execsw, PsStrings};
use crate::sys::exec_elf::ElfEhdr;
use crate::sys::proc::{EXIT_NORMAL, PS_EXEC, PS_INEXEC, Proc};
use crate::sys::signal::SIGABRT;
use crate::sys::syslimits::PATH_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Register, Vaddr};
use crate::unported;
use crate::uvm::uvm_map::uvmspace_exec;
use crate::uvm::uvm_param::{atop, round_page, trunc_page};

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
    let pr = p.process();

    // get other threads to stop (single_thread_set SINGLE_UNWIND|SINGLE_DEEP): one thread
    // until M6-c.
    pr.ps_flags.fetch_or(PS_INEXEC, Ordering::Relaxed);

    // initialize the fields of the exec package
    let mut pack = ExecPackage::new(name, image);

    // see if we can run it.
    if let Err(e) = check_exec(p, &mut pack) {
        pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
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
        pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
        return Err(Errno::ENOMEM);
    }
    // adjust "active stack depth" for process VSZ
    pack.ep_ssize = len; // maybe should go elsewhere, but...

    // we're committed: any further errors will kill the process, so kill the other threads
    // now (single_thread_set SINGLE_EXIT: M6-c).

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
    // pack.ep_execpath = vm_minsaddr - sgap - PATH_MAX, and uvm_map_protect(PROT_NONE) from
    // round_page(execpath + PATH_MAX) to vm_minsaddr: M7a.
    let _ = unported!("exec: uvm_map_protect of the stack top (M7a)");

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

    // stopprofclock, fdprepforexec, execsigs, TCB_SET, the kbind bits and the signal
    // cookie: M6-c.
    let _ = unported!("exec: fdprepforexec/execsigs/TCB_SET (M6-c)");

    // set command name & other accounting info
    pr.set_comm(name);
    pr.ps_acflag.set(pr.ps_acflag.get() & !AFORK);

    // record proc's vnode, for use by sysctl: no vnode.

    // ps_iflags (PSI_NOBTCFI, PSI_PROFILE, PSI_WXNEEDED): with the flags (M6-c).

    // If process does execve() while it has a mismatched real, effective, or saved uid/gid,
    // we set PS_SUGIDEXEC: no credentials yet (kern_prot.c, M6-c).

    pr.ps_flags.fetch_or(PS_EXEC, Ordering::Relaxed);
    // PS_PPWAIT wakeups (vfork): M6-c.

    // reset CPU time usage for the thread, but not the process
    p.p_tu.tu_runtime.set(Timespec::new(0, 0));
    for ticks in &p.p_tu.tu_ticks {
        ticks.set(0);
    }
    // pc_lock_init(&p->p_tu.tu_pcl): the lock is statically initialised.

    p.set_name(b"");

    // km_free(argp), the pathname buffers, vn_close: nothing allocated.

    // notify others that we exec'd: knote (kqueue, M6-c).

    // map the process's timekeep page, exec_elf_fixup, the signal trampoline, exec_md_map:
    // M6-c/M7.

    // setup new registers and do misc. setup.
    Machine::setregs(p, &pack, Vaddr::new(stack), &arginfo);

    // PS_TRACED → psignal(SIGTRAP): ptrace (M7).

    // p_descfd: EXEC_HASFD never set here.

    pr.ps_flags.fetch_and(!PS_INEXEC, Ordering::Relaxed);
    // single_thread_clear(p): M6-c.

    // setregs() sets up all the registers, so just 'return'
    Err(Errno::EJUSTRETURN)
}

/// `exec_abort:`: the old process is dead: kill it.
fn exec_abort(p: &Proc, pack: &mut ExecPackage<'_>) -> ! {
    // free the vmspace-creation commands, and release their references
    pack.ep_vmcmds.kill();
    // the opened file descriptor (EXEC_HASFD), the buffers: none.

    exit1(p, 0, SIGABRT, EXIT_NORMAL)
    // NOTREACHED
}
