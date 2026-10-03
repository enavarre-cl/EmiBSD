//! Kernel-wide types and constants: OpenBSD `sys/sys/*.h`.
//!
//! Each header becomes one module here (`errno.h` → `errno.rs`, `proc.h` → `proc.rs`). Functions
//! that the corresponding `.c` file implements live in that file's module (`kern/`), as `impl`
//! blocks on the types defined here.

pub mod acct;
pub mod clockintr;
pub mod device;
pub mod endian;
pub mod errno;
pub mod evcount;
pub mod exec;
pub mod exec_elf;
pub mod ioccom;
pub mod kernel;
pub mod limits;
pub mod malloc;
pub mod mbuf;
pub mod mman;
pub mod msgbuf;
pub mod mutex;
pub mod param;
pub mod pclock;
pub mod pool;
pub mod proc;
pub mod queue;
pub mod reboot;
pub mod refcnt;
pub mod resource;
pub mod resourcevar;
pub mod rwlock;
pub mod sched;
pub mod signal;
pub mod socket;
pub mod sockio;
pub mod softintr;
pub mod syscall;
pub mod syscall_mi;
pub mod syscallargs;
pub mod syslimits;
pub mod syslog;
pub mod systm;
pub mod task;
pub mod termios;
pub mod time;
pub mod timeout;
pub mod timetc;
pub mod tree;
pub mod ttydefaults;
pub mod types;
pub mod ucred;
pub mod unistd;
pub mod user;
pub mod vmmeter;
