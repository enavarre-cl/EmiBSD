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
| An intrusive link as `Cell<*const T>` (`queue.h`/`tree.h` entries and heads): null compared by hand, each dereference its own `unsafe` block with "a linked element is valid" as its comment | `Link<T> = Cell<Option<NonNull<T>>>` (`sys/sys/tree.rs`): links are written only from references (`NonNull::from`, so they carry the element's provenance) and followed in one function (`splay_follow`) whose `SAFETY:` is the module's invariant (a link is `None` or points at a live element); the `unsafe` mutators' contract keeps linked elements valid and in place, and a removed element is left holding no links, so readers stay safe on it. The algorithms are safe code on `&Elem` | One soundness argument instead of one comment per dereference; an absent link is `None`, matched instead of compared; a stale link of an unlinked element cannot be followed into memory it no longer owns |
| A C algorithm over raw links (`struct rb_entry *`) ported as `*const Entry` locals, an `unsafe fn e(p) -> &Entry` and an `unsafe` block per step; a generic root that any `rb_type` may walk; the mirrored left/right halves written twice | A typed handle, `RbNode<'a, T>` (`sys/kern/subr_tree.rs`): a `Copy` wrapper of the link, typed by the tree's `RbType`, made only from the element's own entry (address arithmetic by `map_addr`, which keeps the element's provenance) or from a link read out of a tree of the same type; its accessors (`left`, `parent`, `set_child`, `color`, ...) are safe, and the module's two dereferences (`RbNode::entry`, `rb_e2n`) carry the soundness argument. The root is `RbTree<T>`, typed by its `RbType`; the mirrored halves are one path over a `Side` | The algorithm becomes safe code with the C's control flow; a link of one tree cannot be read with another type's offset; a missing node the C would dereference as null is a `None` the code must handle (here, a panic naming the broken structure) |

## Candidates (not settled; each becomes a row when first used)

- `Cell<T>` on every field the C mutates behind a shared pointer -> the fields owned by the
  lock's guard (`Mutex<State>`), or `&mut` where the call graph proves exclusivity. Why: the
  lock proves the aliasing the `Cell` only promises.
- `Cell<*const T>` links in `queue.rs`/`tree.rs` entries -> owned intrusive handles with a typed
  owner, or `Arc`/`Weak` where the C's refcount discipline already exists (`refcnt(9)`). Why:
  the pointer's lifetime becomes the type's.
- C-shaped out parameters (`&mut T` filled on success, an `Errno` returned) -> `Result<T, Errno>`
  returning the value. Why: a value that exists only on success is a `Result`.
- `*mut u8` + length at an internal API -> `&[u8]` / `&mut [u8]` all the way to the copyin/
  copyout edge. Why: bounds are the slice's.
- `unported!("x")` for an inherited gap -> unchanged until the redesign closes it; then gone
  with its `## Deviations` line. Why: gaps stay visible, never widened.
- `unsafe impl Send/Sync` as a marker on a C-shaped struct -> the struct made of `Send`/`Sync`
  parts, the marker gone. Why: the compiler proves it.
- A `#[repr(C)]` struct shared with userland -> unchanged, ever. Why: it is the ABI.
