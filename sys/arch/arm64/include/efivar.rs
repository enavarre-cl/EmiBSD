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

/*
 * Copyright (c) 2022 Mark Kettenis <kettenis@openbsd.org>
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
//! arm64 `<machine/efivar.h>`: the efi(4) softc.
//!
//! Upstream: sys/arch/arm64/include/efivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `sc_todr` is a pointer to a `malloc`ed chip handle: a zero-filled softc (what
//!   `config_attach` makes) cannot hold a `TodrChipHandle`, whose functions are not
//!   nullable.
//! - `efi_enter`, `efi_leave` and `efi_jmpbuf` are defined in `efi_machdep.rs` (as in C);
//!   `efi_enter_check`, a `setjmp`, is `efi_machdep.rs`'s `efi_call`, which makes the
//!   runtime-service call through an assembly trampoline that saves the registers in
//!   `efi_jmpbuf`, so that `efi_fault` can return `EFAULT` from it.
//! - `efi_get_variable`, `efi_set_variable` and `efi_get_next_variable_name` are
//!   `dev/efi/efi.c`'s, which is not ported.

use core::cell::Cell;

use crate::arch::arm64::include::pmap::Pmap;
use crate::dev::clock_subr::TodrChipHandle;
use crate::dev::efi::efi::{EfiRuntimeServices, EfiSystemResourceTable};
use crate::sys::device::{Device, Softc};

/// `struct efi_softc`.
#[repr(C)]
pub struct EfiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_pm`: the pmap that maps the runtime services, active only while they run.
    pub sc_pm: Cell<*const Pmap>,
    /// `sc_rs`: the runtime services table (a physical address, mapped by `sc_pm`).
    pub sc_rs: Cell<*mut EfiRuntimeServices>,
    /// `sc_esrt`: the system resource table (`efi.c`).
    pub sc_esrt: Cell<*mut EfiSystemResourceTable>,
    /// `sc_psw`: the interrupt state `efi_enter` saved.
    pub sc_psw: Cell<u64>,

    /// `sc_todr`: the chip handle `todr_attach` was given (see the module's deviations).
    pub sc_todr: Cell<*const TodrChipHandle>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; the other fields are `Cell`s of raw
// pointers and an integer, all valid as zero.
unsafe impl Softc for EfiSoftc {}
/* </CODE> */
