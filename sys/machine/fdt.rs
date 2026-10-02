//! `<machine/fdt.h>` as a trait: what the device-tree drivers need from the machine.
//!
//! On OpenBSD arm64 `<machine/fdt.h>` declares `fdt_find_cons`, `stdout_node`, `stdout_speed`
//! and `fdt_cons_bs_tag`, which the console drivers' `*_init_cons` use to find the console
//! the bootloader named in `/chosen`. A machine without a device tree answers "no node".

use crate::dev::ofw::fdt::FdtNode;
use crate::machine::Machine;
use crate::machine::bus::BusSpaceTag;

/// The device-tree side of the machine.
pub trait Fdt {
    /// `fdt_find_cons(name)`: the node of the console `/chosen`'s `stdout-path` (or the
    /// `serial0` alias) names, if it is compatible with `name`; sets `stdout_node` and
    /// `stdout_speed` on the way.
    fn fdt_find_cons(name: &[u8]) -> FdtNode;

    /// `stdout_node`: the console's node handle, 0 before `fdt_find_cons` found it.
    fn stdout_node() -> i32;

    /// `fdt_cons_bs_tag`: the bus space tag the console is reached through.
    fn fdt_cons_bs_tag() -> BusSpaceTag;
}

/// `fdt_find_cons` on the selected machine.
pub fn fdt_find_cons(name: &[u8]) -> FdtNode {
    Machine::fdt_find_cons(name)
}

/// `stdout_node` on the selected machine.
pub fn stdout_node() -> i32 {
    Machine::stdout_node()
}

/// `fdt_cons_bs_tag` on the selected machine.
pub fn fdt_cons_bs_tag() -> BusSpaceTag {
    Machine::fdt_cons_bs_tag()
}
