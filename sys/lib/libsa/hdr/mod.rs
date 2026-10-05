//! The kernel headers libsa includes, as the subsets the standalone programs use.
//!
//! OpenBSD compiles libsa with `-I${S}`: `ufs.c` includes `<ufs/ffs/fs.h>`, `loadfile.c`
//! `<sys/exec_elf.h>`, `stand.h` `<sys/stat.h>` and so on, the very headers the kernel uses.
//! Here those headers are ported in the kernel crate (`bsd`), where they are tied to kernel
//! types (`Buf`, `Inode`, `Cell`-wrapped super-blocks, the `machine` traits). libsa is a leaf
//! crate that the boot loaders link without the kernel, so the parts it needs are declared
//! again in this module, one file per header, with the C layouts (`#[repr(C)]`) and names.
//! The host tests in `tests.rs` (a dev-dependency on `bsd`) check that every layout here
//! agrees with the kernel's port of the same header (docs/ARCHITECTURE.md, "Boot loaders").

pub mod cons;
pub mod dinode;
pub mod dir;
pub mod disklabel;
pub mod endian;
pub mod ethertypes;
pub mod exec_elf;
pub mod fs;
pub mod hibernate;
pub mod if_arp;
pub mod if_ether;
pub mod in_;
pub mod ip;
pub mod ip_var;
pub mod iso;
pub mod param;
pub mod reboot;
pub mod stat;
pub mod types;
pub mod udp;
pub mod udp_var;
pub mod uuid;

#[cfg(test)]
mod tests;
