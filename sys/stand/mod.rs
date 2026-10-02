//! Boot glue for the Limine protocol (replaces OpenBSD's `boot(8)` / `efiboot`).
//!
//! Milestone M0 fills this in: Limine requests, the arch-neutral `BootInfo`, and the hand-off to
//! `bsd::machine::Machine::early_init` followed by `bsd::kern::init_main::main`. Nothing outside
//! this module may name a `limine` type.

// Link the kernel library even though nothing is called yet: the `#[panic_handler]` lives there.
use bsd as _;

/// Bootloader entry point, named by `ENTRY(_start)` in `arch/*/conf/kernel.ld`.
///
/// Placeholder until M0: parks the CPU so a premature boot hangs visibly instead of executing
/// garbage.
///
/// # Safety
///
/// Called exactly once by the bootloader, with the machine state the Limine protocol specifies.
#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
