# LZ shape -> native shape

One row per settled native idiom: what the faithful port (EmiBSD.LZ) writes to mirror the C,
what this system writes instead, and why. A row is added in the commit that first uses the
idiom, not before. `docs/C_TO_RUST.md` (C -> LZ shape) is frozen and stays the key to reading
LZ code; this table starts where it ends.

Columns: LZ shape | native shape | why.

| LZ shape (mirrors the C) | Native shape | Why |
|---|---|---|
| _(settled rows start at N1)_ | | |

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
