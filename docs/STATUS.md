# Status

Milestone: **M5 in progress** (part a, clocks, done; part b1, process structures and proc0,
done; part b2, sleep/scheduler/switch, next). Updated: 2026-10-02.

Done:
- M5-b1: `proc.h` (`Proc`, `Process`, `Pgrp`, `Session`, `Tusage`, flags, states), `user.h`,
  `refcnt.h`, `resource.h`, `ucred.h`, per-arch `pcb.h`/`proc.h`/`fpu.h`/`reg.h` through
  `machine::proc`; `kern_proc.c` (lists, hashes, pools, `tfind`/`prfind`/`chgproccnt`),
  `hashinit`, `kern_kthread.c`'s deferred queue, `process_initialize` and the pid/tid
  allocators, `refcnt_*`; `main` sets `curproc` and builds process 0; `maxprocess`.
- M5-a: `sys/time.h`, `timetc.h`, `clockintr.h`, `timeout.h`, `pclock.h`, `sched.h` (clock
  subset); `kern_tc.c` (beerware, accepted), `kern_clockintr.c`, `kern_timeout.c`,
  `kern_clock.c`, `roundrobin`, `sched_init_cpu` (the binds), `itimer_update`, `profclock`,
  `libkern/random.c`, `pc_lock`; `Cpu::{CpuInfo, ClockFrame, curcpu, ci_queue, ci_schedstate,
  CLKF_*, need_resched, cpu_initclocks/startclock}`. amd64: i8254 timecounter, LAPIC timer
  (calibrated, `Xintr_lapic_ltimer`). arm64: `agtimer` from the DTB through `ampintc`.
  `main` runs `initclocks`; `selftest=clock` sees `hz` ticks in ~1 s and a timeout on both.
- Licence blocks of every ported file sit between `/* <LICENSES> */` markers (read from the
  closing one). M4 done before: traps, interrupts, GICv2/DTB, UART by interrupt.

Next (M5-b2):
- `kern_synch.c` (`tsleep`/`msleep`, `sleep_setup`/`sleep_finish`, the sleep queues,
  `wakeup_n`, `endtsleep`), the rest of `sched_bsd.c`/`kern_sched.c` (`mi_switch`,
  `setrunnable`, `setrunqueue`, `sched_chooseproc`, `sched_idle`, cpusets), `switchframe` +
  `cpu_switchto` per arch (`locore.S`/`cpuswitch.S`), `vm_machdep.c`'s `cpu_fork`,
  `uvm_uarea_alloc`, `fork1` for kernel threads, `kthread_create`, `kthread_run_deferred_queue`
  in `main`, the softclock thread; then `timeout_barrier`/`clockintr_unbind` get their sleeps.
- Exit: two kthreads ping-pong via `tsleep`/`wakeup` (`selftest=kthread`).

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.
