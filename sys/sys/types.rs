/*	$OpenBSD: types.h,v 1.50 2026/03/26 21:46:24 daniel Exp $	*/
/*	$NetBSD: types.h,v 1.29 1996/11/15 22:48:25 jtc Exp $	*/

/*-
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)types.h	8.4 (Berkeley) 1/21/94
 */

/*	$OpenBSD: _types.h,v 1.10 2022/08/06 13:31:13 semarie Exp $	*/

/*-
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)types.h	8.3 (Berkeley) 1/5/94
 */

//! Kernel-wide scalar types: `<sys/types.h>` together with `<sys/_types.h>`.
//!
//! Upstream: sys/sys/types.h @ 3ce1f3f79392
//! Upstream: sys/sys/_types.h @ 3ce1f3f79392
//!
//! Widths are those of `<machine/_types.h>` on amd64 and arm64, which agree (both are LP64).
//!
//! ## Deviations
//! - `<sys/_types.h>` is folded in: its `__name_t` typedefs exist only for namespace hygiene,
//!   which Rust modules provide. Each `name_t` becomes one CamelCase alias without the `_t`
//!   (`pid_t` → [`Pid`]); see `docs/C_TO_RUST.md`.
//! - `u_char`, `u_short`, `u_int`, `u_long`, the Sys V `unchar`/`ushort`/`uint`/`ulong`, the
//!   exact-width `[u_]intN_t` and `quad_t`/`u_quad_t` are Rust primitives (`u8`, `u16`, `u32`,
//!   `u64`, `i64`, ...); no aliases are declared.
//! - `caddr_t` is `&[u8]`/`&mut [u8]` (or `NonNull<u8>` at ABI edges); `size_t`/`ssize_t` are
//!   `usize`/`isize`; `bool`, `true`, `false` are Rust's.
//! - `vaddr_t`, `paddr_t`, `vsize_t`, `psize_t` are the newtypes [`Vaddr`], [`Paddr`], [`Vsize`],
//!   [`Psize`], so the compiler keeps virtual and physical addresses apart.
//! - `major`, `minor`, `makedev` are `const fn`s over [`Dev`].
//! - `__mbstate_t`, the userland prototypes (`lseek`, `ftruncate`, `truncate`) and the forward
//!   `struct` declarations have no kernel-side counterpart.
//! - `label_t` is machine-dependent and lives in `arch/<arch>/include/_types.rs`.

/// Blocks allocated for a file (`blkcnt_t`).
pub type Blkcnt = i64;
/// Optimal block size for I/O (`blksize_t`).
pub type Blksize = i32;
/// Ticks in `CLOCKS_PER_SEC` (`clock_t`).
pub type Clock = i64;
/// `CLOCK_*` identifiers (`clockid_t`).
pub type Clockid = i32;
/// CPU id (`cpuid_t`, an `unsigned long`).
pub type Cpuid = usize;
/// Device number (`dev_t`).
pub type Dev = i32;
/// Fixed point number (`fixpt_t`).
pub type Fixpt = u32;
/// File system block count (`fsblkcnt_t`).
pub type Fsblkcnt = u64;
/// File system file count (`fsfilcnt_t`).
pub type Fsfilcnt = u64;
/// Group id (`gid_t`).
pub type Gid = u32;
/// May contain a pid, uid or gid (`id_t`).
pub type Id = u32;
/// Base type for an internet address (`in_addr_t`).
pub type InAddr = u32;
/// IP port (`in_port_t`).
pub type InPort = u16;
/// Inode number (`ino_t`).
pub type Ino = u64;
/// IPC key, for Sys V IPC (`key_t`, a `long`).
pub type Key = isize;
/// Permissions (`mode_t`).
pub type Mode = u32;
/// Link count (`nlink_t`).
pub type Nlink = u32;
/// File offset or size (`off_t`).
pub type Off = i64;
/// Process id (`pid_t`).
pub type Pid = i32;
/// Resource limit (`rlim_t`).
pub type Rlim = u64;
/// `sockaddr` address family (`sa_family_t`).
pub type SaFamily = u8;
/// Segment size (`segsz_t`).
pub type Segsz = i32;
/// Length type for network syscalls (`socklen_t`).
pub type Socklen = u32;
/// Microseconds, signed (`suseconds_t`, a `long`).
pub type Suseconds = isize;
/// Epoch time (`time_t`).
pub type Time = i64;
/// POSIX timer identifier (`timer_t`).
pub type Timer = i32;
/// User id (`uid_t`).
pub type Uid = u32;
/// Microseconds (`useconds_t`).
pub type Useconds = u32;

/// Register-sized type (`register_t`, a `long`).
pub type Register = isize;
/// 32-bit disk address (`daddr32_t`).
pub type Daddr32 = i32;
/// 64-bit disk address (`daddr_t`).
pub type Daddr = i64;

/// Declares an address newtype and its size newtype, with the arithmetic that keeps them apart.
macro_rules! address_types {
    (
        $(#[$addr_doc:meta])* $addr:ident,
        $(#[$size_doc:meta])* $size:ident
    ) => {
        $(#[$addr_doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        #[repr(transparent)]
        pub struct $addr(pub usize);

        $(#[$size_doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        #[repr(transparent)]
        pub struct $size(pub usize);

        impl $addr {
            /// Wraps a raw address.
            pub const fn new(addr: usize) -> Self {
                Self(addr)
            }

            /// The raw address.
            pub const fn as_usize(self) -> usize {
                self.0
            }
        }

        impl $size {
            /// Wraps a raw byte count.
            pub const fn new(size: usize) -> Self {
                Self(size)
            }

            /// The raw byte count.
            pub const fn as_usize(self) -> usize {
                self.0
            }
        }

        impl core::ops::Add<$size> for $addr {
            type Output = $addr;

            fn add(self, rhs: $size) -> $addr {
                $addr(self.0 + rhs.0)
            }
        }

        impl core::ops::Sub<$size> for $addr {
            type Output = $addr;

            fn sub(self, rhs: $size) -> $addr {
                $addr(self.0 - rhs.0)
            }
        }

        impl core::ops::Sub<$addr> for $addr {
            type Output = $size;

            fn sub(self, rhs: $addr) -> $size {
                $size(self.0 - rhs.0)
            }
        }

        impl core::ops::Add for $size {
            type Output = $size;

            fn add(self, rhs: $size) -> $size {
                $size(self.0 + rhs.0)
            }
        }

        impl core::ops::Sub for $size {
            type Output = $size;

            fn sub(self, rhs: $size) -> $size {
                $size(self.0 - rhs.0)
            }
        }

        impl core::fmt::Debug for $addr {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}({:#x})", stringify!($addr), self.0)
            }
        }

        impl core::fmt::Debug for $size {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}({:#x})", stringify!($size), self.0)
            }
        }

        impl core::fmt::LowerHex for $addr {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::LowerHex::fmt(&self.0, f)
            }
        }

        impl core::fmt::LowerHex for $size {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::LowerHex::fmt(&self.0, f)
            }
        }
    };
}

address_types! {
    /// A virtual address (`vaddr_t`).
    Vaddr,
    /// A size in virtual address space (`vsize_t`).
    Vsize
}

address_types! {
    /// A physical address (`paddr_t`).
    Paddr,
    /// A size in physical address space (`psize_t`).
    Psize
}

/// `major(x)`: the major number (8 bits) of a device number.
pub const fn major(dev: Dev) -> u32 {
    ((dev as u32) >> 8) & 0xff
}

/// `minor(x)`: the minor number (24 bits, stored around the major byte) of a device number.
pub const fn minor(dev: Dev) -> u32 {
    let x = dev as u32;
    (x & 0xff) | ((x & 0xffff_0000) >> 8)
}

/// `makedev(x, y)`: the device number with major `x` and minor `y`.
pub const fn makedev(major: u32, minor: u32) -> Dev {
    (((major & 0xff) << 8) | (minor & 0xff) | ((minor & 0x00ff_ff00) << 8)) as Dev
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp64_widths() {
        assert_eq!(core::mem::size_of::<Cpuid>(), 8);
        assert_eq!(core::mem::size_of::<Key>(), 8);
        assert_eq!(core::mem::size_of::<Register>(), 8);
        assert_eq!(core::mem::size_of::<Dev>(), 4);
        assert_eq!(core::mem::size_of::<Off>(), 8);
        assert_eq!(core::mem::size_of::<Vaddr>(), core::mem::size_of::<usize>());
    }

    #[test]
    fn device_numbers_round_trip() {
        let cases: &[(u32, u32)] = &[
            (0, 0),
            (4, 1),
            (0xff, 0xff),
            (12, 0x12_3456),
            (1, 0xff_ffff),
        ];
        for &(maj, min) in cases {
            let dev = makedev(maj, min);
            assert_eq!(major(dev), maj, "major of {dev:#x}");
            assert_eq!(minor(dev), min, "minor of {dev:#x}");
        }
        // bits beyond the fields are dropped
        assert_eq!(makedev(0x1ff, 0x1ff_ffff), makedev(0xff, 0xff_ffff));
        // layout: major in bits 8..16, low minor byte in 0..8, high minor bytes in 16..32
        assert_eq!(makedev(0xab, 0x12_34cd), 0x1234_abcd_u32 as Dev);
    }

    #[test]
    fn address_arithmetic_keeps_types_apart() {
        let base = Vaddr::new(0x1000);
        let size = Vsize::new(0x10);
        assert_eq!(base + size, Vaddr(0x1010));
        assert_eq!((base + size) - base, size);
        assert_eq!((base + size) - size, base);
        assert_eq!(size + size, Vsize(0x20));
        assert_eq!(Paddr::new(0x2000).as_usize(), 0x2000);
        assert_eq!(std::format!("{:?}", Paddr(0x2000)), "Paddr(0x2000)");
        assert_eq!(std::format!("{:x}", Psize(255)), "ff");
        assert_eq!(Vaddr::default(), Vaddr(0));
    }
}
