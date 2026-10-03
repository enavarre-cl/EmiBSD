//! Network interfaces, routing and the link layer: OpenBSD `sys/net/`.
//!
//! Headers become modules as in `sys/sys` (`if_types.h` → `if_types.rs`). `if.h` is
//! `if_.rs` because `if` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod ethertypes;
pub mod if_;
pub mod if_arp;
pub mod if_dl;
pub mod if_types;
pub mod netisr;
pub mod route;
