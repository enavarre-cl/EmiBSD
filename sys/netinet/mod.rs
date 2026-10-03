//! The Internet protocols: OpenBSD `sys/netinet/`.
//!
//! Headers become modules as in `sys/sys` (`ip_icmp.h` → `ip_icmp.rs`). `in.h` is `in_.rs`
//! because `in` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod icmp_var;
pub mod if_ether;
pub mod in4_cksum;
pub mod in_;
pub mod in_cksum;
pub mod in_pcb;
pub mod in_proto;
pub mod in_systm;
pub mod in_var;
pub mod ip;
pub mod ip_icmp;
pub mod ip_id;
pub mod ip_input;
pub mod ip_output;
pub mod ip_var;
pub mod raw_ip;
