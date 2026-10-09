/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<machine/fdt.h>` as a trait: what the device-tree drivers need from the machine.
//!
//! On OpenBSD arm64 `<machine/fdt.h>` declares `fdt_find_cons`, `stdout_node`, `stdout_speed`
//! and `fdt_cons_bs_tag`, which the console drivers' `*_init_cons` use to find the console
//! the bootloader named in `/chosen`, `struct fdt_attach_args`, with which a device-tree node's
//! driver is attached, and the `fdt_intr_*` names of the machine's interrupt functions
//! (M12: the `interrupt-map` and MSI ones, `fdt_intr_establish_imap*` and
//! `fdt_intr_establish_msi*`, which PCI host bridges use). A
//! machine without a device tree answers "no node", never attaches anything with
//! [`FdtAttachArgs`] (its type only has the members, so the machine-independent `sys/dev/fdt`
//! drivers compile everywhere) and establishes no interrupt.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ofw::fdt::{FdtNode, FdtReg};
use crate::machine::Machine;
use crate::machine::bus::{BusDmaTag, BusSpaceTag};
use crate::machine::cpu::CpuInfo;
use crate::machine::intr::IntrFn;

/// `struct fdt_attach_args` of the selected machine.
pub type FdtAttachArgs<'a> = <Machine as Fdt>::FdtAttachArgs<'a>;

/// The `struct fdt_attach_args` of a machine without a device tree: the members, so the
/// device-tree drivers compile; nothing ever makes one.
pub struct NoFdtAttachArgs<'a> {
    /// `fa_name`.
    pub fa_name: &'a [u8],
    /// `fa_node`.
    pub fa_node: i32,
    /// `fa_iot`.
    pub fa_iot: BusSpaceTag,
    /// `fa_dmat`.
    pub fa_dmat: BusDmaTag,
    /// `fa_reg`.
    pub fa_reg: &'a [FdtReg],
    /// `fa_intr`.
    pub fa_intr: &'a [u32],
    /// `fa_acells`.
    pub fa_acells: i32,
    /// `fa_scells`.
    pub fa_scells: i32,
}

/// The device-tree side of the machine.
pub trait Fdt {
    /// `struct fdt_attach_args`: what a node's driver is attached with (`aux`). Every
    /// machine names the members `fa_name`, `fa_node`, `fa_iot`, `fa_dmat`, `fa_reg`,
    /// `fa_intr`, `fa_acells` and `fa_scells` ([`fdt_attach_args_public_members`]).
    type FdtAttachArgs<'a>;

    /// `fdt_find_cons(name)`: the node of the console `/chosen`'s `stdout-path` (or the
    /// `serial0` alias) names, if it is compatible with `name`; sets `stdout_node` and
    /// `stdout_speed` on the way.
    fn fdt_find_cons(name: &[u8]) -> FdtNode;

    /// `stdout_node`: the console's node handle, 0 before `fdt_find_cons` found it.
    fn stdout_node() -> i32;

    /// `fdt_cons_bs_tag`: the bus space tag the console is reached through.
    fn fdt_cons_bs_tag() -> BusSpaceTag;

    /// `fdt_intr_establish(node, level, func, arg, name)`: `func(arg)` for the first
    /// interrupt of `node`; the handle `intr_barrier` and `fdt_intr_disestablish` take, `None`
    /// for the C's NULL.
    fn fdt_intr_establish(
        node: i32,
        level: i32,
        func: IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>>;

    /// `fdt_intr_establish_imap_cpu(node, reg, nreg, level, ci, func, arg, name)`: the
    /// interrupt `node`'s `interrupt-map` routes the child unit address and pin `reg` (four
    /// cells) to, on `ci` (any CPU when `None`); a handle as `fdt_intr_establish`'s.
    fn fdt_intr_establish_imap_cpu(
        node: i32,
        reg: &[u32],
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>>;

    /// `fdt_intr_establish_msi_cpu(node, &addr, &data, level, ci, func, arg, name)`: an MSI
    /// through the controller `node`'s `msi-map` or `msi-parent` names; `data` goes in as
    /// the requester ID, and `addr` and `data` come back as the doorbell and the payload.
    #[allow(clippy::too_many_arguments)] // the C's signature
    fn fdt_intr_establish_msi_cpu(
        node: i32,
        addr: &mut u64,
        data: &mut u64,
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>>;

    /// `fdt_intr_disestablish(cookie)`.
    ///
    /// # Safety
    ///
    /// `cookie` came from `fdt_intr_establish*` and is not used afterwards.
    unsafe fn fdt_intr_disestablish(cookie: NonNull<c_void>);

    /// `smc_call(a0, a1, a2, a3)` (`<machine/cpufunc.h>`, `support.S`): an SMCCC call to
    /// the secure monitor; the value `x0` comes back with. A machine without the
    /// instruction answers `PSCI_NOT_SUPPORTED` (-1).
    fn smc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64;

    /// `hvc_call(a0, a1, a2, a3)`: as `smc_call`, to the hypervisor.
    fn hvc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64;

    /// `cpuresetfn = f` (`extern void (*cpuresetfn)(void)` in `machdep.c`): the function
    /// `boot(9)` calls to reset the machine. Only a driver that attaches registers one.
    fn set_cpuresetfn(f: fn());

    /// `powerdownfn = f`: the function `boot(9)` calls to power the machine off.
    fn set_powerdownfn(f: fn());

    /// `lid_action` (`extern int lid_action`, `machdep.c`): what closing the lid does
    /// (`machdep.lidaction`), which `gpiokeys` reads for a lid switch.
    fn lid_action() -> i32;
}

/// `smc_call` on the selected machine.
pub fn smc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    Machine::smc_call(a0, a1, a2, a3)
}

/// `hvc_call` on the selected machine.
pub fn hvc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    Machine::hvc_call(a0, a1, a2, a3)
}

/// `cpuresetfn = f` on the selected machine.
pub fn set_cpuresetfn(f: fn()) {
    Machine::set_cpuresetfn(f)
}

/// `powerdownfn = f` on the selected machine.
pub fn set_powerdownfn(f: fn()) {
    Machine::set_powerdownfn(f)
}

/// `lid_action` on the selected machine.
pub fn lid_action() -> i32 {
    Machine::lid_action()
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

/// `fdt_intr_establish` on the selected machine.
pub fn fdt_intr_establish(
    node: i32,
    level: i32,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::fdt_intr_establish(node, level, func, arg, name)
}

/// `fdt_intr_establish_imap` on the selected machine: as `fdt_intr_establish_imap_cpu` on
/// any CPU.
pub fn fdt_intr_establish_imap(
    node: i32,
    reg: &[u32],
    level: i32,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::fdt_intr_establish_imap_cpu(node, reg, level, None, func, arg, name)
}

/// `fdt_intr_establish_imap_cpu` on the selected machine.
pub fn fdt_intr_establish_imap_cpu(
    node: i32,
    reg: &[u32],
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::fdt_intr_establish_imap_cpu(node, reg, level, ci, func, arg, name)
}

/// `fdt_intr_establish_msi` on the selected machine: as `fdt_intr_establish_msi_cpu` on any
/// CPU.
pub fn fdt_intr_establish_msi(
    node: i32,
    addr: &mut u64,
    data: &mut u64,
    level: i32,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::fdt_intr_establish_msi_cpu(node, addr, data, level, None, func, arg, name)
}

/// `fdt_intr_establish_msi_cpu` on the selected machine.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn fdt_intr_establish_msi_cpu(
    node: i32,
    addr: &mut u64,
    data: &mut u64,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::fdt_intr_establish_msi_cpu(node, addr, data, level, ci, func, arg, name)
}

/// `fdt_intr_disestablish` on the selected machine.
///
/// # Safety
///
/// As for [`Fdt::fdt_intr_disestablish`].
pub unsafe fn fdt_intr_disestablish(cookie: NonNull<c_void>) {
    // SAFETY: forwarded.
    unsafe { Machine::fdt_intr_disestablish(cookie) }
}

/// The members machine-independent drivers read, by the names `<machine/fdt.h>` gives them.
/// Compiling it for every architecture checks that each defines them.
pub fn fdt_attach_args_public_members(
    fa: &FdtAttachArgs<'_>,
) -> (usize, i32, BusSpaceTag, BusDmaTag, usize, usize, i32, i32) {
    (
        fa.fa_name.len(),
        fa.fa_node,
        fa.fa_iot,
        fa.fa_dmat,
        fa.fa_reg.len(),
        fa.fa_intr.len(),
        fa.fa_acells,
        fa.fa_scells,
    )
}
/* </CODE> */
