# Pinned OpenBSD source

Repository: https://github.com/openbsd/src
Branch: master
Subtree: sys/ lib/ bin/ sbin/ usr.bin/ libexec/ include/ gnu/lib/libcompiler_rt/ gnu/llvm/compiler-rt/ usr.sbin/makefs/ usr.sbin/pwd_mkdb/ usr.sbin/tcpdump/ usr.sbin/portmap/ usr.sbin/quotaon/ usr.sbin/edquota/ usr.sbin/repquota/ usr.sbin/hostapd/ etc/ (sparse)
Commit: 3ce1f3f79392ae4d60ce67bea5835d517caaa2ca
Date: 2026-10-02

Machine-read by `cargo xtask ports check` (the `Commit:` line). Change it only together with
`ports.toml [meta].pinned`, in a commit titled `reference: bump OpenBSD pin to <12-hex>`.
