//! Kernel-wide types and constants: OpenBSD `sys/sys/*.h`.
//!
//! Each header becomes one module here (`errno.h` → `errno.rs`, `proc.h` → `proc.rs`). Functions
//! that the corresponding `.c` file implements live in that file's module (`kern/`), as `impl`
//! blocks on the types defined here.

pub mod clockintr;
pub mod errno;
pub mod evcount;
pub mod kernel;
pub mod malloc;
pub mod mman;
pub mod msgbuf;
pub mod mutex;
pub mod param;
pub mod pclock;
pub mod pool;
pub mod queue;
pub mod reboot;
pub mod sched;
pub mod softintr;
pub mod syslimits;
pub mod syslog;
pub mod systm;
pub mod termios;
pub mod time;
pub mod timeout;
pub mod timetc;
pub mod tree;
pub mod ttydefaults;
pub mod types;
