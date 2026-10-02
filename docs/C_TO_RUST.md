# C → Rust idioms

One row per settled idiom. Add a row when the idiom is decided, in the same commit as its first use.
Columns: what OpenBSD C does | what we write | why.

| C (OpenBSD) | Rust (here) | Why |
|---|---|---|
| `int f(...)` returning `0` or an errno | `fn f(...) -> Result<T, Errno>` | the type carries the contract; `?` replaces `goto out` |
| errno values (`EINVAL`, `ERESTART = -1`, `EJUSTRETURN = -2`) | `#[repr(i32)] enum Errno` in `sys/sys/errno.rs`, same names and values | ABI-visible numbers; names grep across both trees |
| `LIST_HEAD(name, type)` plus the `field` argument of every `LIST_*` macro (`queue.h`; same for SLIST, SIMPLEQ, XSIMPLEQ, TAILQ, STAILQ) | `ListHead<A>` with `A` a zero-sized `Adapter` from `queue_adapter!(A: Elem, field => ListEntry<Elem>)`; the element embeds `ListEntry<Elem>`; readers are safe, mutators `unsafe` with the C precondition as contract | the field is fixed once and type-checked; O(1) unlink, one element in several lists, no allocation, as in C |
| `SPLAY_GENERATE`, `RB_GENERATE`, `RBT_GENERATE[_AUGMENT](name, type, field, cmp[, aug])` (`tree.h`) | `tree_adapter!(A: Elem, field => RbtEntry, cmp[, augment = aug])` with `cmp` returning `Ordering`; heads `SplayHead<A>`, `RbHead<A>`, `RbtHead<A>`; `RB_*` and `RBT_*` share the `kern/subr_tree.rs` algorithm | the comparator and the hook belong to the adapter, as `struct rb_type` already bundles them in C; one red-black implementation instead of a macro copy |
| `int cmp(a, b)` returning negative, zero or positive | `fn cmp(a: &T, b: &T) -> Ordering` | the type says the three outcomes; `match` checks them all |
| `s = splhigh(); ... splx(s);` | `let _s = IplGuard::raise(Ipl::High);` | drop restores; a forgotten `splx` is impossible |
| `mtx_enter(&m); ... mtx_leave(&m);` | `let g = m.lock();` on the ported `Mutex<T>` (`kern_lock.c`, IPL-aware) | data lives inside the lock; the guard proves it is held |
| `KASSERT(x)` / `KDASSERT(x)` | `kassert!(x)` / `kdassert!(x)` behind features `diagnostic` / `debug` | same text: `kernel diagnostic assertion "x" failed: file "f", line n` |
| `panic("fmt", ...)` | `panic!("fmt", ...)` routed to `kern::subr_prf::panic` | identical message; `#[allow(clippy::panic)]` with a reason at each site |
| `printf(9)`, `log(9)`, `%b` | `kprintf!`, `kprintln!`, `log!(LOG_x, ...)`; a `Bitmask(value, "\20\1FLAG...")` `Display` helper for `%b` | Rust format strings; `%b` has no counterpart so it is a wrapper |
| `#ifdef OPTION` | `#[cfg(feature = "option")]` | one knob per `option(4)`, visible in `Cargo.toml` |
| `struct proc *p` passed down (borrowed) | `&Proc` | no ownership transfer |
| `struct proc *` in lists or long-lived | `Cell<*const Proc>` inside the list entries, `&Proc` at the API; `Arc<Process>` after M3 for `refcnt(9)` | the link is a raw pointer under the list's lock; every borrow is explicit |
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
| `static __inline` wrappers around one instruction (`inb`, `outb`, `intr_disable`, `disable_irq_daif`) | `#[inline] pub fn` (or `pub unsafe fn` when the instruction has a precondition) with `core::arch::asm!`, `options(nomem, nostack, preserves_flags)` only where each is true | the compiler inlines as GCC did; the options let it schedule around the instruction honestly |
| `int c` characters in the console and printf paths (`cnputc(int)`, `kputchar(int, ...)`, `cn_putc(dev_t, int)`) | `i32`, with `i32::from(b'x')` at the call sites | the C contract (`-1`, `0177`, a NUL that is skipped) is an `int`'s; a `u8` would hide the sentinel values |
| `printf("%s", cstr)` of a NUL-terminated byte string | `Str(&bytes)` (`kern::subr_prf::Str`), a `Display` adaptor that stops at the first NUL | kernel strings are bytes, Rust format strings want `Display` |
| a table of function pointers (`struct consdev`, `struct bus_space`, `arm_clock_func`) | a `struct` of `fn` pointers (`Option<fn>` where the C allows NULL), a `static` instance per device | the C design is a vtable; Rust spells it the same way, with no trait object or allocation |
| calling into a subsystem that is not ported yet | `unported!("uvm_map")` (`sys/kern/unported.rs`): prints once per site, yields `Errno::ENOSYS` | the gap is visible on the serial console and in the message buffer, and propagates as an error where the C returned one |
| `bus_space_tag_t` / `bus_space_handle_t` | the arch's `Tag`/`Handle` associated types of `machine::BusSpace`, aliased as `BusSpaceTag`/`BusSpaceHandle`; `bus_space_map` is `unsafe` (the caller vouches for the device), reads and writes through a handle are safe | a handle only comes from a map; the one `unsafe` is where the firmware's word is taken |
| `__builtin_frame_address(0)` | `Machine::frame_address()`, an `#[inline(always)]` `asm!` reading `rbp`/`x29` | no stable intrinsic; inlining keeps it the caller's frame |
