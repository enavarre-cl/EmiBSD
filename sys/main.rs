/* <CODE> */
//! The kernel image. The boot path lives in `stand/` (Limine protocol hand-off).

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
mod stand;

/// Host build of the kernel image: there is nothing to run on macOS or Linux.
#[cfg(not(target_os = "none"))]
fn main() {
    eprintln!(
        "bsd: this is a kernel image. Build it with `just build`, boot it with `just run-<arch>`."
    );
    std::process::exit(2);
}
/* </CODE> */
