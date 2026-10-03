# Status

Milestone: **M5 done**; next is M6. Updated: 2026-10-02.

Done:
- M5-b part 2 (the exit criterion): `kern_synch.c` (sleep queues, `tsleep`/`msleep`/`wakeup`,
  `refcnt_finalize`, `cond_wait`), `sched_bsd.c` (load average, `schedcpu`, `mi_switch`,
  `setrunnable`, `scheduler_start`), `kern_sched.c` (run queues, cpusets, idle thread,
  `setrunqueue`/`remrunqueue`/`sched_chooseproc`), `kern_fork.c` (`thread_new`,
  `process_new`, `fork1` for kernel threads, `proc_trampoline_mi`), `kthread_create`, the
  softclock thread, `timeout_barrier`, `kern_resource.c` (`tuagg_*`), `uvm_glue.c`
  (u-areas), `subr_xxx.c`, `vmmeter.h`, `acct.h`; `machine::cpu` grew `cpu_switchto`,
  `cpu_fork`, `clear_resched`, `cpu_unidle`, `cpu_idle_*`, `cpu_info_foreach`,
  `curcpu_mutex_level`; amd64 `cpu_switchto`/`proc_trampoline` in `locore.S`, `cpu_fork`,
  `pmap_activate`, `hlt` idle; arm64 `cpuswitch.S`, `cpu_fork`, `fpu_drop`, `pmap_setttb`,
  `wfi` idle. `selftest=kthread` (two kthreads, 100 turns over `msleep`/`wakeup`, about
  100 context switches) passes in `just smoke` on both archs. Host tests: 130.
- Earlier in M5: clocks on both archs (`selftest=clock`), `struct proc`/`process`, proc0,
  the process lists. Project renamed to EmiBSD.

Next (M6, syscalls + minimal init):
- `syscalls.master` → `xtask gen-syscalls` → `sys/sys/syscall.rs` + `init_sysent`, the
  per-arch syscall entry and `syscall_return`/`intr_user_exit` (the trampolines now panic
  after a thread function returns), `exec_elf.c`, user `uvm_map`/`uvm_fault`,
  `sys_generic.c` (`write`), `kern_exit.c` (`exit1`/`exit2`, `kthread_exit`,
  `sched_idle`'s dead list), `kern_sig.c` (`sleep_signal_check`), credentials (`crget`),
  `lim_startup`/`lim_fork`, `uvmspace_init`; a freestanding Rust `init` loaded as a Limine
  module. Exit criterion: `init` prints via `sys_write` and exits on both archs.
- Known M5 leftovers reported at boot: the u-area guard page (needs `km_alloc` from
  `kernel_map`), arm64 `pmap_setttb`'s TTBR0 switch (user pmaps), `exit2` from idle.

Blockers:
- None. `crc32` stays `skipped: license: zlib`.
