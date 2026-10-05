//! boot(8)'s machine-independent part (`sys/stand/boot`): the main loop, the `boot>` prompt
//! and `boot.conf`, the commands and variables, and the boot arguments handed to the kernel.
//!
//! OpenBSD compiles these files into every boot program (`.PATH: ${S}/stand/boot`); here
//! they are a crate the boot programs link, as libsa is. The program registers its
//! machine-dependent routines ([`boot::boot_md_register`]) and calls [`boot::boot`].

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod boot;
pub mod bootarg;
pub mod cmd;
pub mod vars;
