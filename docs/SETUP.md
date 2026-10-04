# Setup (macOS, Apple Silicon)

Verified 2026-10-02 on Darwin 25.6 with Homebrew. Linux users: `rustup`, `qemu`, `limine`, `just`
from your distribution; the rest is identical.

## Why the Homebrew `rust` formula is not enough

Homebrew's `rust` ships only the host target (`aarch64-apple-darwin`) and no `rust-lld`.
The bare-metal targets `x86_64-unknown-none` and `aarch64-unknown-none-softfloat` need both.
`rustup` manages them; in Homebrew it is keg-only because it conflicts with the `rust` formula.
(An alternative that keeps Homebrew rust, `rustup toolchain link system "$(brew --prefix rust)"`,
still lacks the bare targets. Not recommended.)

## Steps

1. Remove Homebrew rust. Nothing else depends on it (`brew uses --installed rust` is empty):
   ```sh
   brew uninstall rust
   ```
2. Install the tools:
   ```sh
   brew install rustup qemu limine just
   brew install x86_64-elf-gdb aarch64-elf-gdb   # optional, for `just gdb-*` later
   ```
3. Put rustup and cargo on `PATH`. Add to `~/.zshrc`, then open a new shell:
   ```sh
   export PATH="$HOME/.cargo/bin:$(brew --prefix rustup)/bin:$PATH"
   ```
4. Install a default toolchain once:
   ```sh
   rustup default stable
   ```
5. In the repository, the first `cargo` command installs the pinned toolchain, both bare-metal
   targets and the components listed in `rust-toolchain.toml` automatically.

## Verify

```sh
which cargo                        # ~/.cargo/bin/cargo or the rustup keg, NOT /opt/homebrew/bin
cargo --version                    # the version pinned in rust-toolchain.toml
rustup target list --installed     # x86_64-unknown-none, aarch64-unknown-none-softfloat
ls "$(brew --prefix qemu)/share/qemu/"edk2-{x86_64,aarch64}-code.fd
ls "$(brew --prefix limine)/share/limine/"BOOT{X64,AA64}.EFI
cargo check && cargo test          # host build of bsd + libkern
cargo check -p bsd --target x86_64-unknown-none
cargo check -p bsd --target aarch64-unknown-none-softfloat
just smoke                         # after milestone M0
```

## Editor

rust-analyzer works out of the box on the host target thanks to `sys/arch/host`. To analyse
arch-specific code, set `rust-analyzer.cargo.target` to one of the bare targets temporarily.
`just clippy` checks all three targets regardless of editor settings.

## What `just` expects

- `qemu-system-x86_64`, `qemu-system-aarch64` on `PATH`.
- EDK2 firmware and Limine binaries located via `brew --prefix qemu` / `brew --prefix limine`
  at runtime by `xtask` (no hardcoded paths).

## Userland toolchain (M8)

`just userland` (`cargo xtask userland --arch amd64|arm64`) cross-compiles OpenBSD's own C,
unmodified, from `reference/openbsd-src`: `/usr/include`, `lib/csu`, `libc.a`, `libutil.a`,
`libm.a`, `libkvm.a`, `init(8)`, `ksh(1)`, `cat(1)`, `echo(1)`, `ls(1)`, `uname(1)`, `mount(8)`, `getty(8)`,
`login(1)`, `login_passwd(8)`, `ifconfig(8)`, `ping(8)` (and `ping6`), `route(8)`, `pfctl(8)`,
`ipsecctl(8)`, `ps(1)`, `df(1)`, `fstat(1)` and `vmstat(8)` among others, and the ffs ramdisk image `ramdisk.ffs` (made by
OpenBSD's makefs(8) and pwd_mkdb(8), built for the Mac with the same clang, as are rpcgen(1),
yacc(1), which `pfctl` and `ipsecctl`'s `parse.y` need, and lex(1) for `.l` sources, whose own
`scan.l` the Mac's `/usr/bin/lex` makes since the tree has no `initscan.c`), into
`target/userland/<arch>/`. It is not part of
`just ci`. The user approved these tools on 2026-10-03; nothing else is installed for it:

| Tool | Default | Override | Use |
|---|---|---|---|
| Apple clang 21 (Xcode) | `/usr/bin/clang` | `$EMIBSD_CC` | compiles for `x86_64-unknown-openbsd` and `aarch64-unknown-openbsd` (ELF), and builds `rpcgen` and `yacc` for the Mac |
| LLD 17 | `~/.swiftly/bin/ld.lld` | `$EMIBSD_LLVM_BIN` (the directory) | links static PIE executables |
| `llvm-ar`, `llvm-ranlib`, `llvm-objcopy`, `llvm-objdump` | `~/.swiftly/bin/` | `$EMIBSD_LLVM_BIN` | archives, `install -s`, verification |

`~/.swiftly/bin` is the Swift toolchain manager's directory; its LLVM tools are proxies that
`swiftly` resolves to the selected toolchain. Any LLVM 17 or newer `ld.lld` with OpenBSD
support (it must emit `PT_OPENBSD_SYSCALLS`) and matching binutils work through
`$EMIBSD_LLVM_BIN`. `/bin/sh`, `sed` and `/usr/bin/cpp` (Apple clang's, run by `rpcgen`) are the
system's.

The OpenBSD sources come from `$OPENBSD_SRC` if set (absolute, or relative to the repository),
else `reference/openbsd-src`, else, from a git worktree, the main checkout's
`reference/openbsd-src`. They are never written to.

Output: `sysroot/usr/{include,lib}` (what OpenBSD installs in `/usr/include` and `/usr/lib`),
`obj/` (objects, per source directory), `root/{bin,sbin}` (the executables, stripped, with
ksh's `rksh` and `sh` links, `/usr/libexec/auth/login_passwd`), `host/` (`rpcgen`, `yacc`, `lex`, `makefs`,
`pwd_mkdb`, `emibsd-bcrypt`) and `licences.txt` (the licence family of
every OpenBSD file compiled or included). The build is incremental (`.d` files and the
recorded command line of every object).

### tcpdump

`lib/libpcap` is built with the other libraries (its `scanner.l` through the host-built lex)
and `usr.sbin/tcpdump` linked `-static` into `/usr/sbin`; tcpdump's Makefile reads `iapp.h`
from `usr.sbin/hostapd`, in the sparse clone since 2026-10-04. Its privsep half chroots to
`/var/empty` as `_tcpdump` (uid and gid 76, the lines of the clone's `etc/master.passwd` and
`etc/group`).

### The test image's login

The ramdisk is a test image, not a secure system: root's password is **`emibsd`**, and the
`/etc/passwd` family is in the repository's sources (`tools/xtask/src/userland/ramdisk.rs`),
so it is public. The password is hashed with OpenBSD's own `bcrypt.c` (`$2b$`, 8 rounds, what
`encrypt(1)` gives by default) and a **fixed salt** (`EmiBSD-test-salt`,
`tools/xtask/src/userland/passwd.rs`) so that `just userland` makes the same hash every time.
`pwd.db` and `spwd.db` are made by OpenBSD's own `pwd_mkdb -p`, built for the Mac, over OpenBSD's own
`lib/libc/db`. A plain boot of the image goes multi-user (`/etc/rc`, then `getty` on `tty00` and
`login:`); `-s` on the kernel command line goes single-user (`just smoke-shell`).

`libcompiler_rt` (`gnu/lib/libcompiler_rt` over `gnu/llvm/compiler-rt`, Apache-2.0 WITH
LLVM-exception, in the sparse clone since 2026-10-03) is built and linked on both archs: arm64's
`printf` `%La` (`gdtoa/hdtoa.c`) multiplies a 128-bit `long double`, which needs `__multf3`.

M9+ adds LibreSSL (`libcrypto`, `libssl`, `libtls`), `libcurses` and `libedit`, and `ftp(1)`
and `nc(1)`. Two more of the Mac's own tools take part, nothing installed: `/usr/bin/perl`
runs libcrypto's perlasm generators (amd64's assembly) and `objects.pl`, and `/usr/bin/awk`
and `sort` run libcurses's table scripts. libcurses's `make_keys` and `make_hash` are built
for the Mac with the same clang and run during the build.

### The test CA

The first `just userland` also makes a **test CA** with the Mac's `/usr/bin/openssl` (macOS's
LibreSSL 3.3.6; `$EMIBSD_OPENSSL` overrides it) in `target/userland/test-ca/`, shared by both
archs (`tools/xtask/src/userland/testca.rs`). It is a test fixture: nothing in it is secret.

| File | What |
|---|---|
| `ca.key`, `ca.pem` | the CA, `O=EmiBSD, CN=EmiBSD test CA`, serial 1 |
| `server.key`, `server.pem` | `CN=emibsd-host`, `subjectAltName=DNS:emibsd-host`, signed by the CA, serial 2 |
| `untrusted.key`, `untrusted.pem` | the same names, self-signed, serial 3: a client must refuse it |
| `openssl.cnf` | the extensions (`CA:TRUE` for the CA, `serverAuth` for the servers) |

Keys are ECDSA P-256 and every certificate is valid for ten years from the day it was made.
The CA is kept: it is made again only when one of its files is missing
(`rm -r target/userland/test-ca` and `just userland` make a new one, with new random keys, so
the image changes too). `ca.pem` is copied into the ramdisk as `/etc/ssl/emibsd-test-ca.pem`;
the guest's clock must be within the validity (it comes from the RTC: the Mac's date).

The test servers are the Mac's `openssl s_server`, run by `cargo xtask smoke` and `smoke2` for
the length of the run with `--https-server DIR:PORT:MODE` (`tools/xtask/src/https.rs`): in
`DIR` (relative to the repository), listening on `PORT`, as `trusted` (`-WWW`: `GET /f`
answers the file `DIR/f`; `server.pem`), `untrusted` (`-WWW` with `untrusted.pem`) or `echo`
(`server.pem`; what the client sends comes back). The guest reaches them as
`emibsd-host:PORT`: on QEMU's user network 10.0.2.2 is the Mac, and slirp turns a connection
to it into one to the Mac's 127.0.0.1, so no port forwarding is configured. `just smoke-https`
uses ports 8443 (trusted), 8444 (echo) and 8445 (untrusted); they must be free.
`cargo test -p xtask -- --ignored servers_answer` checks the three modes against
`openssl s_client` on the Mac alone.

## e2fsprogs (M10d)

`just smoke-ext2fs` checks the ext2 file system the guest made with OpenBSD's
`newfs_ext2fs(8)` a second time, on the Mac, with an independent implementation: e2fsprogs'
`e2fsck -fn` must find it clean (exit status 0) and `debugfs -R 'cat PATH'` must read the guest's
files back (`cargo xtask e2fsck`, `tools/xtask/src/e2fs.rs`). Install it once (the user did on
2026-10-04):

```sh
brew install e2fsprogs
```

The formula is keg-only (macOS has no ext2 of its own, but Homebrew keeps it off `PATH`), so
xtask never looks in `PATH` or a fixed directory: it runs `brew --prefix e2fsprogs` and uses
`<prefix>/sbin/e2fsck` and `<prefix>/sbin/debugfs`. When the formula is missing, the command
fails with an `xtask: e2fsprogs not found ...` line naming it. Both tools only read the disk
image (`target/disk-<arch>-ext2fs.img`): e2fsck runs with `-n`, and they reach the ext2
partition through e2fsprogs' `image?offset=BYTES` syntax, the offset found from the image's
MBR and OpenBSD disklabel, so nothing is copied or written.

## NTFS check (M10d)

`just userland` for amd64 makes `/root/images/ntfs.img` with our own generator and checks it
with macOS's own read-only NTFS driver, nothing installed: `/usr/bin/hdiutil attach -imagekey
diskimage-class=CRawDiskImage -nomount`, then `/usr/sbin/diskutil mount readOnly -mountPoint`
(macOS 26 has no `mount_ntfs` for `/sbin/mount -t ntfs`, which is tried first), as the user,
no `sudo`. The image is always unmounted and detached afterwards. A Mac without
`/System/Library/Filesystems/ntfs.fs` fails with an `xtask: ... not found` line; set
`EMIBSD_NTFS_CHECK=0` there to skip the check (a warning is printed). `cargo xtask ntfs-image
OUT --check` and `cargo test -p xtask -- --ignored macos_` run the same check alone.

## Two VMs (M9b, M9c)

`cargo xtask smoke2 --arch amd64|arm64 --kernel K ...` boots two VMs of one arch at once, for
the WireGuard and IPsec tunnels. Same tools as `smoke`; nothing new to install. Each VM has
`vio0` on QEMU's user-mode network (as in `smoke`) and `vio1` on a private link to the other
VM: QEMU's `dgram` netdev, a pair of UDP sockets on `127.0.0.1` (free ports picked per run).
The MACs are distinct: A has `52:54:00:aa:00:01` (vio0) and `52:54:00:bb:00:01` (vio1), B has
`...:02`. Each VM has its own image (`target/emibsd-<arch>-a.img`, `-b`) and EDK2 variable
store (`target/edk2-<arch>-a-vars.fd`, `-b`), rebuilt on every run.

Scripts are per VM, with the `smoke --until-seen` semantics (each `send` waits for its trigger
line, after the previous send): `--a-send-after L --a-send T`, `--a-expect L`, and the same
with `--b-`; `--both-*` lines are put in front of each VM's own. The run passes when both VMs
are done (a VM that is done stays up for the other one); a VM that is not done after
`--timeout SECS` (default 180) fails it and both transcripts are printed
(`--show-transcripts` prints them always). `just smoke-link` boots two VMs per arch, logs in as
root on both, gives `vio1` the addresses `192.168.77.1` and `.2` and pings across the link.
`just smoke-wg` builds a `wg0` tunnel on top (`10.77.0.1`/`.2`, RFC 7748's keys) with
`ifconfig(8)` and pings through it. Both need `just userland` first and are part of
`just smoke`. An ESP tunnel (M9c) uses the same outer addresses, with `ipsecctl -f` over a
file `echo`ed into place as further `--a-send`/`--b-send` pairs.
