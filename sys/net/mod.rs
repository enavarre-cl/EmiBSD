//! Network interfaces, routing and the link layer: OpenBSD `sys/net/`.
//!
//! Headers become modules as in `sys/sys` (`if_types.h` → `if_types.rs`), and a `.c` file
//! is the module of its name (`ifq.c` → `ifq.rs`, with `ifq.h`). `if.h` and `if.c` are
//! `if_.rs` because `if` is a Rust keyword (`docs/C_TO_RUST.md`).

pub mod art;
pub mod ethertypes;
pub mod if_;
pub mod if_arp;
pub mod if_dl;
pub mod if_ethersubr;
pub mod if_loop;
pub mod if_media;
pub mod if_types;
pub mod if_var;
pub mod if_wg;
pub mod ifq;
pub mod netisr;
pub mod radix;
pub mod route;
pub mod rtable;
pub mod rtsock;
pub mod toeplitz;
pub mod wg_cookie;
pub mod wg_noise;
