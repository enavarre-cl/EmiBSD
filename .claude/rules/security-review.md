# Review of security-sensitive changes

OpenBSD's security properties are inherited through LZ and must survive the redesign. These
areas are security-sensitive: `sys/crypto/`, `sys/net/pf*` (pf, pflog, pfsync, pflow),
`sys/net/if_wg*` and `wg_*` (WireGuard), IPsec (`sys/netinet/ip_ipsp*`, `ip_esp`, `ip_ah`,
`ip_ipcomp`, `ipsec_input`, `ipsec_output`, `net/pfkeyv2*`), `sys/dev/softraid_crypto`,
`sys/kern/kern_pledge`, `kern_unveil`, `exec_*` and the ELF loader, `sys/uvm/uvm_fault` and the
pmaps, copyin/copyout and the per-arch syscall entry, `dev/rnd`.

- They are redesigned last within their subsystem, and crypto, IPsec, WireGuard and softraid
  CRYPTO last of all (N8), only with extra tests: the C's test vectors as table-driven tests,
  property tests for parsers and bounds, and constant-time behaviour kept where the C has it
  (`timingsafe_*`, no data-dependent branches or indexing).
- A commit that touches one of these areas carries `Security-Review: <what was checked>`: the
  invariant reviewed (bounds, lifetime, constant time, privilege boundary), the tests run, and
  the `diff-openbsd` scenarios that cover it. The review is written before the commit, by the
  agent that did not write the change when one is available (`large-changes.md`).
- Before the commit: `just diff-openbsd` and every smoke of the area (`smoke-pf`, `smoke-wg`,
  `smoke-esp`, `smoke-ipcomp`, `smoke-softraid`, `smoke-login`, ... as the justfile names them)
  pass, and `cargo xtask lz drift --security` is empty for the area.
- No new dependency and no new algorithm in `sys/crypto/` without the user's decision; a
  primitive is re-expressed, never replaced.
- A redesign may not relax a check the C makes (a pledge, an unveil path, a bounds check, a
  privilege test); it may only make it a type. When a check becomes a type, the commit body
  says which check and which type.
- An LZ security fix (`lz-sync.md`) in one of these areas is applied, or its
  `covered-by-redesign` reason names the test or the type that makes the bug impossible.
