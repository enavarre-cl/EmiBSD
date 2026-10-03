# Status

Milestone: **M5 in progress** (part a, clocks, done; part b, processes and the scheduler, next).
Updated: 2026-10-02.

Done:
- M5-a: `sys/time.h`, `timetc.h`, `clockintr.h`, `timeout.h`, `pclock.h`, `sched.h` (clock
  subset); `kern_tc.c` (beerware, accepted), `kern_clockintr.c`, `kern_timeout.c`,
  `kern_clock.c`, `roundrobin`, `sched_init_cpu` (the binds), `itimer_update`, `profclock`,
  `libkern/random.c`, `pc_lock`; `Cpu::{CpuInfo, ClockFrame, curcpu, ci_queue, ci_schedstate,
  CLKF_*, need_resched, cpu_initclocks/startclock}`. amd64: i8254 timecounter, LAPIC timer
  (calibrated, `Xintr_lapic_ltimer`). arm64: `agtimer` from the DTB through `ampintc`.
  `main` runs `initclocks`; `selftest=clock` sees `hz` ticks in ~1 s and a timeout on both.
- Licence blocks of every ported file sit between `/* <LICENSES> */` markers (read from the
  closing one). M4 done before: traps, interrupts, GICv2/DTB, UART by interrupt.

Next (M5-b):
- `sys/proc.h` → `Proc`/`Process`, `proc0`, `kern_proc.c`, `kern_synch.c` (`tsleep`/`wakeup`,
  the sleep queues), the rest of `sched_bsd.c`/`kern_sched.c` (`mi_switch`, `setrunnable`, run
  queues, idle), `cpu_switchto` per arch (`locore.S`/`cpuswitch.S`), `vm_machdep.c`'s
  `cpu_fork`, `kern_fork.c` (kthreads only), `kern_kthread.c`; the softclock thread, then
  `clockintr_unbind`'s barrier and `timeout_barrier` get their sleeps.
- Exit: two kthreads ping-pong via `tsleep`/`wakeup` (`selftest=kthread`).

Blockers:
- None. `crc32` stays `skipped: license: zlib`; amd64's TSC timecounter (`tsc.c`) is deferred.
