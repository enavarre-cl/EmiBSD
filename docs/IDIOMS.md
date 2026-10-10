# LZ shape -> native shape

One row per settled native idiom: what the faithful port (EmiBSD.LZ) writes to mirror the C,
what this system writes instead, and why. A row is added in the commit that first uses the
idiom, not before. `docs/C_TO_RUST.md` (C -> LZ shape) is frozen and stays the key to reading
LZ code; this table starts where it ends.

Columns: LZ shape | native shape | why.

| LZ shape (mirrors the C) | Native shape | Why |
|---|---|---|
| A C status code returned as `i32` (zlib's `Z_OK`, `Z_STREAM_END`, `Z_NEED_DICT`, `Z_BUF_ERROR`, ...) and compared by every caller; a mode passed as an `i32` constant (`Z_FINISH`) | `Result<Status, Error>`: the `Ok` carries the non-error statuses (`Result<(), Error>` when success is only `Z_OK`), the `Err` a typed error enum; each type has `code()` for the C value, and the `Z_*` constants stay as those values; the mode is an enum (`Flush`) with its own `code()` (`sys/lib/libz/zlib.rs`) | `?` and `match` replace the comparisons; an error cannot be mistaken for progress, and an out-of-range mode cannot be built. Callers that keep a status integer of their own (libsa's `cread`) take `.code()` |
| A hash or MAC context struct and C-named free functions over it: `X_Init(&mut ctx)`, `X_Update(&mut ctx, data)`, `X_Final(&mut out, &mut ctx)` (`MD5Init`, `SHA256Final`, `SipHash24_End`, `blake2s_final`) | A type with methods: `XCtx::new()` (or `new(key)` for a keyed one) is `Init`, `update(&mut self, &[u8])` is `Update`, `finalize(&mut self) -> [u8; N]` is `Final` and returns the digest; `finalize` wipes the context in place where the C does `explicit_bzero(ctx)`, so it takes `&mut self`, not `self` (a by-value `self` would wipe a copy). `Default` is the wiped state, not a started hash (`sys/crypto/md5.rs`). A parameter the C passes on every call but fixes per use becomes a const parameter of the type (SipHash's round counts: `SiphashCtx<C, D>`, `sys/crypto/siphash.rs`; BLAKE2s's digest length: `Blake2sState<N>`, whose range check is a `const` assertion) | The context cannot be updated before it is started or used across two algorithms, the digest is a value instead of an out parameter whose length must match, and a context cannot change its fixed parameters midway |
| A key schedule in a `Copy` struct (`[u32; 32]`, `AesCtx`, ...), wiped only when the framework `explicit_bzero`s the session | the schedule's own type, built by its constructor and `impl Drop` zeroing its words through `crate::crypto::wipe` (so it is not `Copy`); scratch secrets are still wiped where the C wipes them | The key cannot outlive its owner on any path (an early return, a replaced `Kschedule`, a freed session), without new `unsafe`; the stores go through `black_box`, so the optimizer keeps them. A move can leave a copy on the stack, as the C's by-value copies do |
| `Cell<*const T>` (`struct type *`) and `Cell<*const Cell<*const T>>` (`struct type **`) in a list entry or head, each read through its own `unsafe { p.as_ref() }`, with `NULL` checked by hand or not at all | a typed intrusive link: a private type, `Link<P>` = `Cell<Option<NonNull<P>>>` (`None` is `NULL`), whose fields only the module's mutators write; its invariant (a link of a head or of a linked element names a live object) is stated once on the type and the dereference lives in one method (`Link::target`); the `_FOREACH_SAFE` walks share one cursor that holds the pending position as `NonNull`. What the C turns into a `NULL` dereference is a `None` arm that panics (the row below; `sys/sys/queue.rs`) | One soundness argument instead of one per read (queue.rs: 49 `unsafe` blocks to 6), and the null case cannot be forgotten; same size and auto traits as the raw pointer, so the structures that embed the entries do not change |
| `container_of` from a pointer taken from a field reference (`&elem.entry.next as *const _`, stored as the C's `tqh_last`/`tqe_prev`), stepping back with `unsafe` pointer arithmetic | the stored back pointer is made with the whole element's provenance (`NonNull::from(elem).with_addr(field.addr())`, `link_of`), the step back is safe `wrapping_byte_sub` by the adapter's offset plus the field's (`container_of`), and only the dereference of the result is `unsafe` (`sys/sys/queue.rs`) | A pointer derived from a field reference may not reach the rest of the element; with the element's provenance it may, and the arithmetic needs no `unsafe` |
| A pointer stored as an integer and turned back (`p as usize`, `(cookie ^ v) as *const T`: `XSIMPLEQ_XOR`) | `p.expose_provenance()` to encode and `ptr::with_exposed_provenance(_mut)` to decode (`sys/sys/queue.rs`) | Rust's explicit API for the round trip: the decoded pointer is one the compiler allows to be dereferenced, and the intent shows |
| A broken precondition that the C turns into a fault (a `NULL` dereference in `TAILQ_REMOVE`, `SIMPLEQ_REMOVE_HEAD` of an empty queue), mirrored in LZ by dereferencing a null raw pointer | an `Option` match whose `None` arm calls a `#[cold]` module function that is `panic(9)` in the kernel and `std::panic!` under `cfg(test)` (`queue_panic`, `sys/sys/queue.rs`), in every configuration; the message names the operation | The fault stays a fault, with a message instead of a trap, and never becomes a silent no-op; the host's `panic(9)` ends the test process (`boot` exits), so the test build unwinds and `#[should_panic]` can test each case |

## Candidates (not settled; each becomes a row when first used)

- `Cell<T>` on every field the C mutates behind a shared pointer -> the fields owned by the
  lock's guard (`Mutex<State>`), or `&mut` where the call graph proves exclusivity. Why: the
  lock proves the aliasing the `Cell` only promises.
- `Cell<*const T>` links in `tree.rs` entries -> owned intrusive handles with a typed owner,
  or `Arc`/`Weak` where the C's refcount discipline already exists (`refcnt(9)`). Why: the
  pointer's lifetime becomes the type's. (`queue.rs`'s links are settled: "a typed intrusive
  link" above.)
- C-shaped out parameters (`&mut T` filled on success, an `Errno` returned) -> `Result<T, Errno>`
  returning the value. Why: a value that exists only on success is a `Result`.
- `*mut u8` + length at an internal API -> `&[u8]` / `&mut [u8]` all the way to the copyin/
  copyout edge. Why: bounds are the slice's.
- `unported!("x")` for an inherited gap -> unchanged until the redesign closes it; then gone
  with its `## Deviations` line. Why: gaps stay visible, never widened.
- `unsafe impl Send/Sync` as a marker on a C-shaped struct -> the struct made of `Send`/`Sync`
  parts, the marker gone. Why: the compiler proves it.
- A `#[repr(C)]` struct shared with userland -> unchanged, ever. Why: it is the ABI.
