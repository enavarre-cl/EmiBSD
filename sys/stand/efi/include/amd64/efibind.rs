/* Public Domain. */

//! Basic scalar types and the `EFIERR` macro for amd64.
//!
//! Upstream: sys/stand/efi/include/amd64/efibind.h @ 3ce1f3f79392
//!
//! The C header maps the UEFI scalar names onto `<sys/stdint.h>`. OpenBSD's arm64 efiboot
//! reaches this header too (both are LP64, 64-bit UEFI), and the compile-time checks below
//! pin the one assumption that matters, `UINTN` being 64 bits wide.
//!
//! ## Deviations
//! - The scalar aliases keep the specification's upper-case names (`UINTN`, `UINT32`, ...);
//!   `non_camel_case_types` is allowed for that reason.
//! - `VOID` is `core::ffi::c_void`; `VOID *` becomes `*mut c_void` in the other modules.
//! - `INTERFACE_DECL(x)` (a forward declaration) has no Rust counterpart and is dropped.
//! - `EFIAPI` (`__attribute((ms_abi))`) is the stable `extern "efiapi"` ABI string on every
//!   function pointer.
//! - `EFIERR(x)` is the const fn [`efierr`].

#![allow(non_camel_case_types)] // the specification's scalar names, kept verbatim

use core::ffi::c_void;

/// `INT8`: signed 8 bits.
pub type INT8 = i8;
/// `UINT8`: unsigned 8 bits.
pub type UINT8 = u8;
/// `INT16`: signed 16 bits.
pub type INT16 = i16;
/// `UINT16`: unsigned 16 bits.
pub type UINT16 = u16;
/// `INT32`: signed 32 bits.
pub type INT32 = i32;
/// `UINT32`: unsigned 32 bits.
pub type UINT32 = u32;
/// `INT64`: signed 64 bits.
pub type INT64 = i64;
/// `UINT64`: unsigned 64 bits.
pub type UINT64 = u64;

/// `VOID`: the pointee of `VOID *`.
pub type VOID = c_void;

/// `INTN`: the native signed integer (64 bits on amd64 and arm64).
pub type INTN = isize;
/// `UINTN`: the native unsigned integer (64 bits on amd64 and arm64).
pub type UINTN = usize;

/// `EFIERR(x)`: the error bit (the top bit of a `UINTN`) or-ed with `x`.
pub const fn efierr(x: UINTN) -> UINTN {
    0x8000_0000_0000_0000 | x
}

const _: () = assert!(core::mem::size_of::<UINTN>() == 8);
const _: () = assert!(core::mem::size_of::<INTN>() == 8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efierr_sets_the_top_bit() {
        assert_eq!(efierr(1), 0x8000_0000_0000_0001);
        assert_eq!(efierr(0), 1usize << 63);
    }
}
