# Status

Milestone: **M6 in progress** (part a, the system call plumbing, done; part b, user address
spaces and exec, next). Updated: 2026-10-02.

Done:
- M6-a: `cargo xtask gen-syscalls` (`syscalls.master` → `sys/sys/syscall.rs`,
  `syscallargs.rs`, `kern/init_sysent.rs`, `kern/syscalls.rs`; `--check` in `just ci`),
  `struct sysent`/`SCARG` (`systm.rs`), `syscall_mi.h` (`mi_syscall`, `mi_syscall_return`,
  `mi_child_return`, `mi_ast`, `pin_check`), `kern_sig.c`'s `sys_nosys` and `userret`,
  `refreshcreds`; amd64 `Xsyscall` with the AST loop and `sysretq`, `syscall()`, `ast()`,
  `child_return`, `copy.S` with the `.nofault` table and `pcb_onfault` recovery in
  `kpageflttrap`, `MSR_LSTAR`; arm64 `handle_el0_*`/`do_ast`/`syscall_return`,
  `do_el0_sync` (`svc` only), `svc_handler`, `ast`, `copy.S`/`copystr.S`, `pcb_onfault`
  recovery in `kdata_abort`; the `machine::copy` contract (`copyin`/`copyout`/`copyinstr`/
  `copyoutstr`/`kcopy`) on all three machines. Nothing runs in user mode yet: the paths are
  inert until M6-b.
- M5 done before: clocks, processes, scheduler, kernel threads (`selftest=kthread`).

Next (M6-b, user address spaces and exec):
- `vmspace` (`uvm_extern.h`), `uvmspace_init/alloc/free`, user `pmap_create/destroy/enter`
  on both archs, `pmap_activate` with a real user pmap and `cpu_switchto`'s user bits
  (`ci_kern_rsp`, `ci_proc_pmap`, segment resets, `TTBR0`), `cpu_fork` with a user stack,
  the trap-from-user paths (amd64 `TRAP_ENTRY_USER`/`INTRENTRY`'s user branch,
  `intr_user_exit`; arm64 `udata_abort`), `setregs`, `exec_elf.c` + `kern_exec.c` for a
  static ELF, the Limine module carrying a freestanding Rust `init`, `start_init`,
  `kern_exit.c` (`exit1`/`exit2`, `kthread_exit`), `sys_exit`; decision pending: port
  `uvm_map`/`uvm_fault` (M6 as planned, 7000+ lines) or wire the first process with wired
  mappings and defer them.
- M6-c: `sys_write` to the console for fds 1/2 until the file table exists; the exit
  criterion "init prints via sys_write and exits".

Blockers:
- None. `crc32` stays `skipped: license: zlib`.
