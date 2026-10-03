# Status

Milestone: **M7a in progress** (part 0, `kern_rwlock.c` and the `vm_map` lock, done).
M6 closed on 2026-10-03: `init` runs in user mode on both architectures, prints through
`sys_write` and exits; `just smoke` checks `init exited with status 0 (signal 0)`.
Updated: 2026-10-03.

Done:
- M7a-0: `rwlock.h`/`kern_rwlock.c` (`rw_enter`/`rw_exit` and friends, `rrw_*`, the
  `rw_obj_*` store; host tests), `rwsleep_nsec`/`rwsleep`; the `vm_map` lock is the rwlock
  with `busy`/`nbusy` and `vm_map_lock_try/lock/lock_read/unlock/unlock_read/upgrade/
  downgrade/busy/unbusy/assert_*`; `ps_lock`, `uidinfolk`, `tc_lock` and `pool_lock` are
  real rwlocks; `rw_obj_init` runs in `main`.
- M6-b2: `exec_elf.h`, `exec.h`, `signal.h` (numbers), `limits.h`; `uvm_map.c` (the map and
  vmspace life cycle, wired-page stand-ins for `uvm_map`/`uvm_fault_wire`), `vmspace`,
  `vmspace0`; `exec_elf.c` (static `ET_EXEC`), `exec_subr.c` (vmcmds over wired pages,
  `exec_setup_stack`), `kern_exec.c` (`check_exec`, `exec_image`), `sys_write` to the
  console, `start_init` + `fork1` of init, `uvm_purge`/`uvm_exit`, `uvmspace_fork/share`;
  `machine::exec` (`ELF_TARG_*`), `setregs`, `pmap_create/destroy/reference/enter/remove`,
  `pmap_proc_iflush`, `VmParam`'s user limits. amd64: user pmaps through the direct map,
  `alltraps`/`INTRENTRY` from user mode, `intr_user_exit` (`iretq`), `usertrap`,
  `EFER.SCE`. arm64: user pmaps (three-level, ASIDs), `TTBR0`/`T0SZ` switch in
  `pmap_init` with the console and `bus_space_map` moved to the kernel half,
  `udata_abort`, `cpu_icache_sync_range`, `proc_trampoline` → `syscall_return`. `init`
  carries the OpenBSD ELF note and (arm64) `-z nobtcfi`.
- M6-b1: `kern_exit.c`, the reaper, `init` as a Limine module. M6-a: generated syscall
  tables, `syscall_mi.h`, per-arch syscall entry, `copyin` family.
- M5: clocks, process structures, sleep/scheduler/switch, kernel threads. M4 and before:
  traps, interrupts, GICv2/DTB, UART by interrupt, uvm page system, direct map, console.

Next (M7a):
- `uvm_map.c` proper (the `vm_map_entry` tree, `uvm_map_setup_entries`/`uvm_map_fix_space`,
  `uvm_addr.c`'s selectors, `uvm_map`, `uvm_unmap`, `uvm_map_protect`, `uvm_map_pageable`,
  `uvmspace_fork` copying entries), `uvm_fault.c`, `uvm_amap.c`, `uvm_aobj.c`,
  `uvm_pager.c`, `uvm_mmap.c`; the pv lists on amd64 (`pmap_enter_pv`, `pmap_page_remove`,
  `pmap_protect`), `pmap_fault_fixup` on arm64; then `exec` through `uvm_map` and the
  wired-page stand-ins go away.
- Exit: `init` runs from a pageable `vmspace`; a user page fault is served by `uvm_fault`.

Then (M6-c, interleaved as needed): `kern_sig.c` (`trapsignal`, `sigexit`, `execsigs`),
`kern_descrip.c` (the file table, `fdprepforexec`), `kern_resource.c` (`lim_cur`), a real
`sys_execve` with `copyargs`, the FPU on exec, `CPUPF_USERSEGS`/FS.base on amd64.

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.
