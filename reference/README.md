# reference/

`openbsd-src/` is a sparse, shallow clone of OpenBSD's source mirror restricted to `sys/` (the
kernel, which is ported) and, since M8, `lib/ bin/ sbin/ usr.bin/ libexec/ include/` (the
userland, compiled unmodified), plus `gnu/lib/libcompiler_rt` and `gnu/llvm/compiler-rt`
(the compiler runtime libc needs, e.g. `__multf3` on arm64; Apache-2.0 WITH LLVM-exception).
It is gitignored and **read-only**. `PINNED.md` records the exact commit.

## Clone (first time)

```sh
git clone --depth 1 --filter=blob:none --sparse https://github.com/openbsd/src.git reference/openbsd-src
git -C reference/openbsd-src sparse-checkout set sys lib bin sbin usr.bin libexec include \
    gnu/lib/libcompiler_rt gnu/llvm/compiler-rt
git -C reference/openbsd-src log -1 --format='%H %cs'      # -> PINNED.md Commit: and Date:
```

Then set `Commit:` and `Date:` in `PINNED.md`, set `[meta].pinned` in `ports.toml` to the same
hash, and run `cargo xtask ports check`.

## Reproduce an exact pin on another machine

```sh
git -C reference/openbsd-src fetch --depth 1 origin <hash>
git -C reference/openbsd-src checkout --detach <hash>
```

## Update the pin

Deliberate, never automatic. See `docs/PORTING.md`, "Bumping the reference pin".

## Why shallow and sparse

`sys/` alone is hundreds of megabytes; the full history is gigabytes. `--depth 1` means `git log`
inside the clone is useless, so drift is detected by blob hash (`cargo xtask ports drift`), not by
history.
