# reference/

`openbsd-src/` is a sparse, shallow clone of OpenBSD's source mirror restricted to `sys/` (the
kernel, which is ported) and, since M8, `lib/ bin/ sbin/ usr.bin/ libexec/ include/` (the
userland, compiled unmodified), plus `gnu/lib/libcompiler_rt` and `gnu/llvm/compiler-rt`
(the compiler runtime libc needs, e.g. `__multf3` on arm64; Apache-2.0 WITH LLVM-exception),
`usr.sbin/makefs` (built for the host to make the ffs ramdisk image), `usr.sbin/pwd_mkdb`
(built for the host to make the ramdisk's password databases), `usr.sbin/tcpdump` (for the
ramdisk, M9+; added 2026-10-03) and `usr.sbin/portmap`, `quotaon`, `edquota`, `repquota` (NFS and
quotas, M10; added 2026-10-03), `usr.sbin/hostapd` (only `iapp.h`, which tcpdump includes) and
`etc/` (`master.passwd` and `group` give the ramdisk OpenBSD's uids, e.g. `_tcpdump`; both added 2026-10-04), and for M14 (the
user's pre-approval of 2026-10-04): all of `usr.sbin/`, `distrib/` (install.sub, the bsd.rd lists),
`share/`, and the comp set's toolchain: `gnu/llvm` (Apache-2.0 WITH LLVM-exception) with OpenBSD's
clang build glue `gnu/usr.bin/clang` and `gnu/lib/{libcxx,libcxxabi,libclang_rt}` (added
2026-10-05). The GPL parts of `gnu/` (binutils, gcc, texinfo, perl) stay out.
It is gitignored and **read-only**. `PINNED.md` records the exact commit.

## Clone (first time)

```sh
git clone --depth 1 --filter=blob:none --sparse https://github.com/openbsd/src.git reference/openbsd-src
git -C reference/openbsd-src sparse-checkout set sys lib bin sbin usr.bin libexec include \
    gnu/lib/libcompiler_rt gnu/llvm/compiler-rt usr.sbin etc distrib share \
    gnu/llvm gnu/usr.bin/clang gnu/lib/libcxx gnu/lib/libcxxabi gnu/lib/libclang_rt
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
