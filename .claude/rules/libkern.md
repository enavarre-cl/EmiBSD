---
paths:
  - "sys/lib/libkern/**"
---

# libkern

`sys/lib/libkern` is a separate crate because OpenBSD builds it as a separate library. It must
stay a leaf.

- `[dependencies]` stays empty. No `bsd`, no external crates. `proptest` as a dev-dependency only.
- `#![no_std]`; `#[cfg(test)] extern crate std;` is the only std.
- Functions whose semantics `core` already provides exactly (`memcpy`, `memset`, `memcmp`,
  `strlen`, `qsort`, `ffs`) are not ported: `status = "skipped"`, `notes = "provided-by-core: ..."`.
- Functions with OpenBSD-specific semantics are ported faithfully: `strlcpy`/`strlcat` return the
  length that *would* have been written; `crc32` table and polynomial; `timingsafe_bcmp` and
  `timingsafe_memcmp` in constant time (no early return, no data-dependent branches);
  `explicit_bzero` via `write_volatile`; `getsn`, `scanc`, `skpc`.
- Signatures use slices (`&[u8]`, `&mut [u8]`) where C uses pointer + length; return `usize` where
  C returns `size_t`; C strings are `&[u8]` NUL-terminated or `&CStr`, never `&str` (kernel strings
  are not UTF-8).
- Every function has table-driven tests covering the edge cases the C comments mention
  (zero-length destination, exact fit, truncation) and, where it makes sense, a property test.
- `.S` implementations under OpenBSD `arch/` are not ported here; the generic C version is.
