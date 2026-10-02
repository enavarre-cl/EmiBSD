# Status

Milestone: **M5 in progress** (part a, clocks: MI code done, timers half wired). Updated: 2026-10-02.

Done:
- M5-a part 1: `sys/time.h`, `timetc.h`, `clockintr.h`, `timeout.h`, `pclock.h`, `sched.h`
  (clock subset) as types; `kern_tc.c` (beerware, accepted), `kern_clockintr.c`,
  `kern_timeout.c`, `kern_clock.c`, `roundrobin`, `sched_init_cpu` (the binds), `itimer_update`,
  `profclock`, `libkern/random.c`, `pc_lock`; `Cpu::{CpuInfo, ClockFrame, curcpu, ci_queue,
  ci_schedstate, CLKF_*, need_resched, cpu_initclocks/startclock}`; `main` runs
  `timeout_startup`, `clockqueue_init`, `sched_init_cpu`, `timeout_proc_init`. amd64: i8254
  timecounter, `startclocks`, LAPIC timer calibrated (`cpu0: apic clock running at`),
  `Xintr_lapic_ltimer`, `LIR_TIMER` source. Host tests: 124 (timecounters, clockintr queue,
  timer wheel).
- Licence blocks of every ported file sit between `/* <LICENSES> */` markers (read from the
  closing one). M4 done before: traps, interrupts, GICv2/DTB, UART by interrupt.

Next (M5-a part 2, then part b):
- arm64 `dev/agtimer.c` (generic timer from the DTB, `arm_clock_register`, PPI through
  `arm_intr_establish_fdt_idx`), `include/timetc.h`, `armreg.h` CNT*/CurrentEL constants;
  then wire `initclocks()` in `main`, add `selftest=clock` (ticks at `hz`, a `timeout` fires)
  to `justfile`'s smoke for both archs. The exit criterion "uptime ticks at hz" follows.
- M5-b: `proc.h` → `Proc`/`Process`, `kern_proc.c`, `kern_synch.c` (tsleep/wakeup, sleep
  queues), `sched_bsd.c`/`kern_sched.c` (the rest), `cpu_switchto`, `kern_kthread.c`, idle;
  two kthreads ping-pong via tsleep/wakeup.

Blockers:
- None. `crc32` stays `skipped: license: zlib`.
