# C → Rust idioms

One row per settled idiom. Add a row when the idiom is decided, in the same commit as its first use.
Columns: what OpenBSD C does | what we write | why.

| C (OpenBSD) | Rust (here) | Why |
|---|---|---|
| `int f(...)` returning `0` or an errno | `fn f(...) -> Result<T, Errno>` | the type carries the contract; `?` replaces `goto out` |
| errno values (`EINVAL`, `ERESTART = -1`, `EJUSTRETURN = -2`) | `#[repr(i32)] enum Errno` in `sys/sys/errno.rs`, same names and values | ABI-visible numbers; names grep across both trees |
| `TAILQ_*`, `LIST_*`, `SIMPLEQ_*` (`queue.h`) | `intrusive_collections::{LinkedList, SinglyLinkedList}` behind adapters in `sys/sys/queue.rs` | O(1) unlink by element, one node in several lists, no allocation; `unsafe` audited upstream |
| `RB_*` (`tree.h`) | `intrusive_collections::RBTree` behind `sys/sys/tree.rs` | same reasons |
| `s = splhigh(); ... splx(s);` | `let _s = IplGuard::raise(Ipl::High);` | drop restores; a forgotten `splx` is impossible |
| `mtx_enter(&m); ... mtx_leave(&m);` | `let g = m.lock();` on the ported `Mutex<T>` (`kern_lock.c`, IPL-aware) | data lives inside the lock; the guard proves it is held |
| `KASSERT(x)` / `KDASSERT(x)` | `kassert!(x)` / `kdassert!(x)` behind features `diagnostic` / `debug` | same text: `kernel diagnostic assertion "x" failed: file "f", line n` |
| `panic("fmt", ...)` | `panic!("fmt", ...)` routed to `kern::subr_prf::panic` | identical message; `#[allow(clippy::panic)]` with a reason at each site |
| `printf(9)`, `log(9)`, `%b` | `kprintf!`, `kprintln!`, `log!(LOG_x, ...)`; a `Bitmask(value, "\20\1FLAG...")` `Display` helper for `%b` | Rust format strings; `%b` has no counterpart so it is a wrapper |
| `#ifdef OPTION` | `#[cfg(feature = "option")]` | one knob per `option(4)`, visible in `Cargo.toml` |
| `struct proc *p` passed down (borrowed) | `&Proc` | no ownership transfer |
| `struct proc *` in lists or long-lived | `NonNull<Proc>` inside intrusive links; `Arc<Process>` after M3 for `refcnt(9)` | lifetime management is explicit |
| fields mutated under a lock through a shared pointer | `UnsafeCell<T>` field with doc `/// Protected by: <lock>` | states the invariant the C only implies |
| `vaddr_t`, `paddr_t`, `vsize_t`, `psize_t` | `Vaddr(usize)`, `Paddr(usize)`, `Vsize(usize)`, `Psize(usize)` newtypes | the compiler stops VA/PA mix-ups |
| `#define FOO_X 0x1` flag groups | `bitflags! { struct FooFlags: u32 { const X = 0x1; } }` | typed, same names, same bits |
| `volatile` MMIO | `read_volatile` / `write_volatile` behind `bus_space`-shaped accessors | no plain dereferences of device memory |
| linker symbols (`end`, `etext`) | `unsafe extern "C" { static __kernel_end: u8; }` + `addr_of!` | address-only symbols, never read as values |
| `goto out` cleanup | `?`, RAII guards, labelled blocks | same control flow, enforced release |
| `.S` files | real `.S` file + `global_asm!(include_str!("x.S"))` | diffable against OpenBSD's |
| `static` globals | `static X: Mutex<T>`, atomics, or `StaticCell<T>` (documented init-once) | `static mut` is forbidden |
| `caddr_t`, `void *` buffers | `&[u8]` / `&mut [u8]`; `NonNull<u8>` only at ABI edges | the length travels with the pointer |
| C strings (`char *`, not UTF-8) | `&[u8]` NUL-terminated or `&CStr`; never `&str` in kernel ABI | kernel strings are bytes |
| `size_t` / `ssize_t` | `usize` / `isize` | same width, same meaning |
| `<machine/param.h>`, `<machine/_types.h>` constants (`PAGE_SIZE`, `KERNBASE`, `_ALIGNBYTES`) | associated consts of `machine::MachineParam`, defined per arch in `arch/<arch>/include/{param,_types}.rs`, re-exported by `sys::param` | generic code never names an arch; the compiler proves every arch defines the whole set |
| `typedef __int32_t pid_t;` and the other scalar typedefs of `<sys/types.h>` | `pub type Pid = i32;` in `sys/sys/types.rs`: CamelCase, no `_t` | the `_t` is C namespace hygiene; the alias keeps the width and the doc comment names the C type |
| `u_int`, `u_long`, `u_int32_t`, `quad_t`, `int8_t`, ... | `u32`, `u64`, `u32`, `i64`, `i8`, ...: no alias | the primitive already states the width; both archs are LP64 |
| `#define NAME 42` in a header | `pub const NAME: usize = 42;` in that header's module, with the narrowest honest type (`usize` for sizes and counts, `i32` where C passes an `int`) | names stay grep-able; the type documents the use |
| function-like macros in capitals: `ALIGN(p)`, `MAXCOMLEN`-style arithmetic, `howmany(x, y)`, `ctod(x)` | snake-case `const fn` in the same module: `align`, `howmany`, `ctod` | untyped macros buy nothing in Rust; lowercase is the only change |
| `MIN`/`MAX`, `offsetof`, `nitems`, `SET`/`CLR`/`ISSET` | `core::cmp::{min, max}`, `core::mem::offset_of!`, `.len()`, `|=` / `&= !` / `&` | already in core |
| `const char *s` string argument | `&[u8]` ending at the first NUL or at `s.len()`, whichever comes first | no read past the slice is possible; a NUL-free slice is still a string |
| `int f(...)` that is really a boolean (`timingsafe_bcmp` returns 0 or 1) | `-> bool`, the doc stating `true` where C returns nonzero | `if f(..)` reads the same in both languages |
| constant tables (`crc32c_lookup[]`) | `const TABLE: [u32; 256] = build_table();` computed by a `const fn` from the defining polynomial; `just test-ref` compares every entry with the C header | the data is reproduced, not copied; the reference test keeps it honest |
