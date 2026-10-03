//! The Internet protocols: OpenBSD `sys/netinet/`.
//!
//! Headers become modules as in `sys/sys` (`ip_icmp.h` → `ip_icmp.rs`). `in.h` is `in_.rs`
//! because `in` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod if_ether;
pub mod in_;
pub mod in_systm;
pub mod ip;
pub mod ip_icmp;
pub mod ip_var;
