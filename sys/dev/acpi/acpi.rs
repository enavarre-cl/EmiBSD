/* $OpenBSD: acpi.c,v 1.458 2026/07/31 18:17:22 jan Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
 * Copyright (c) 2005 Jordan Hargrave <jordan@openbsd.org>
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

//! `dev/acpi/acpi.c`, not ported yet: only the seam the AML interpreter (`dev/acpi/dsdt.rs`)
//! calls through. Each function here has the C's signature and a visible stub body
//! (`unported!`); the port of `acpi.c` (and of `arch/amd64/amd64/acpi_machdep.c` for the
//! global lock, through the machine contract) replaces them in place.
//!
//! Upstream: sys/dev/acpi/acpi.c @ 3ce1f3f79392 (seam only; `ports.toml` keeps the file
//! `todo`)
//!
//! ## Deviations
//! - Every function is a stub until `acpi.c` is ported. What each one answers meanwhile:
//!   - `acpi_gasio`: -1 (the access failed), nothing read or written;
//!   - `acpi_addtask`: the task is not queued (and its `arg0` is not given back);
//!   - `acpi_dotask`: 0, no task ran;
//!   - `acpi_read_pmreg`: 0; `acpi_write_pmreg`: nothing;
//!   - `acpi_acquire_glk`: 1 (acquired, so `acpi_glk_enter` does not spin);
//!     `acpi_release_glk`: 0 (no waiter); both belong to `acpi_machdep.c`;
//!   - `acpi_maptable`: `None` (`Load` then reports that it cannot load the table).
//! - `acpi_poll_enabled` is defined here, as in `acpi.c`.

use core::ffi::c_void;
use core::sync::atomic::AtomicI32;

use super::acpivar::{AcpiQ, AcpiSoftc};
use crate::unported;

/// `acpi_poll_enabled`: some device asked to be polled (`ACPIDEV_POLL`) and the poll
/// timeout already runs.
pub static ACPI_POLL_ENABLED: AtomicI32 = AtomicI32::new(0);

/// `acpi_gasio(sc, iodir, iospace, address, access_size, len, buffer)`: reads or writes
/// `len` bytes of address space `iospace` (`GAS_*`) at `address`, `access_size` bytes at a
/// time; 0 or -1.
pub fn acpi_gasio(
    _sc: Option<&AcpiSoftc>,
    _iodir: i32,
    _iospace: i32,
    _address: u64,
    _access_size: i32,
    _len: i32,
    _buffer: &mut [u8],
) -> i32 {
    let _ = unported!("acpi_gasio");
    -1
}

/// `acpi_addtask(sc, handler, arg0, arg1)`: queues `handler(arg0, arg1)` for the acpi
/// thread. The handler runs exactly once, so an `arg0` that carries ownership (an
/// `Rc::into_raw`) is taken back by the handler.
pub fn acpi_addtask(
    _sc: &AcpiSoftc,
    _handler: fn(*mut c_void, i32),
    _arg0: *mut c_void,
    _arg1: i32,
) {
    let _ = unported!("acpi_addtask");
}

/// `acpi_dotask(sc)`: runs one queued task; nonzero if it ran one.
pub fn acpi_dotask(_sc: &AcpiSoftc) -> i32 {
    let _ = unported!("acpi_dotask");
    0
}

/// `acpi_read_pmreg(sc, reg, offset)`: reads fixed register `reg` (`ACPIREG_*`).
pub fn acpi_read_pmreg(_sc: &AcpiSoftc, _reg: i32, _offset: i32) -> i32 {
    let _ = unported!("acpi_read_pmreg");
    0
}

/// `acpi_write_pmreg(sc, reg, offset, regval)`.
pub fn acpi_write_pmreg(_sc: &AcpiSoftc, _reg: i32, _offset: i32, _regval: i32) {
    let _ = unported!("acpi_write_pmreg");
}

/// `acpi_acquire_glk(lock)`: takes the firmware's global lock in the FACS; nonzero when
/// it is ours (`acpi_machdep.c`).
pub fn acpi_acquire_glk(_lock: *mut u32) -> i32 {
    let _ = unported!("acpi_acquire_glk");
    1
}

/// `acpi_release_glk(lock)`: releases it; nonzero when the firmware waits for it.
pub fn acpi_release_glk(_lock: *mut u32) -> i32 {
    let _ = unported!("acpi_release_glk");
    0
}

/// `acpi_maptable(sc, addr, sig, oem, tbl, flags)`: maps and records the table at physical
/// address `addr` if its signature (and OEM ids, when given) match.
pub fn acpi_maptable(
    _sc: &AcpiSoftc,
    _addr: usize,
    _sig: Option<&[u8]>,
    _oem: Option<&[u8]>,
    _tbl: Option<&[u8]>,
    _flags: i32,
) -> Option<&'static AcpiQ> {
    let _ = unported!("acpi_maptable");
    None
}
