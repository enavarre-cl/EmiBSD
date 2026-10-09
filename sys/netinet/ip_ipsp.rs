/* <LICENSES> */
/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr),
 * Niels Provos (provos@physnet.uni-hamburg.de) and
 * Niklas Hallqvist (niklas@appli.se).
 *
 * The original version of this code was written by John Ioannidis
 * for BSD/OS in Athens, Greece, in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Additional features in 1999 by Angelos D. Keromytis and Niklas Hallqvist.
 *
 * Copyright (c) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 1999 Niklas Hallqvist.
 * Copyright (c) 2001, Angelos D. Keromytis.
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all copies of any software which is or includes a copy or
 * modification of this software.
 * You may use this code under the GNU public license if you so wish. Please
 * contribute changes back to the authors under this freer than GPL license
 * so that we may further the use of strong encryption without limitations to
 * all.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */

/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr),
 * Niels Provos (provos@physnet.uni-hamburg.de) and
 * Niklas Hallqvist (niklas@appli.se).
 *
 * The original version of this code was written by John Ioannidis
 * for BSD/OS in Athens, Greece, in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Additional features in 1999 by Angelos D. Keromytis and Niklas Hallqvist.
 *
 * Copyright (c) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 1999 Niklas Hallqvist.
 * Copyright (c) 2001, Angelos D. Keromytis.
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all copies of any software which is or includes a copy or
 * modification of this software.
 * You may use this code under the GNU public license if you so wish. Please
 * contribute changes back to the authors under this freer than GPL license
 * so that we may further the use of strong encryption without limitations to
 * all.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

/* <CODE> */
//! The IPsec security policy machinery: `<netinet/ip_ipsp.h>` (the TDB, the policies, the
//! acquires, the identities and the transform switch) and `netinet/ip_ipsp.c` (the SA
//! database: three hash tables of TDBs, SPI reservation, the lifetime timers, the identity
//! trees and the transform table `xformsw[]`).
//!
//! Upstream: sys/netinet/ip_ipsp.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_ipsp.c @ 3ce1f3f79392
//!
//! A TDB (tunnel descriptor block) is one security association. Each TDB is on three hash
//! tables: one keyed on dst/spi/sproto (`tdbh`, to find a specific TDB for input), one keyed on
//! dst/sproto (`tdbdst`, outgoing policy matching) and one keyed on src/sproto (`tdbsrc`,
//! incoming policy matching). TDBs are reference counted: the tables hold one reference, every
//! pending timeout one, every policy that caches it one, every bundle link one.
//!
//! Locks used to protect struct members (`[x]` in the field docs):
//! - I: immutable after creation; a: atomic operations; N: net lock;
//! - A: `ipsec_acquire_mtx`; F: `ipsec_flows_mtx`; P: `ipo_tdb_mtx` (policy to TDB links);
//! - D: `tdb_sadb_mtx` (the SA database); m: `tdb_mtx` (fields of a TDB); S: pfsync.
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `union sockaddr_union` is [`SockaddrUnion`], a byte image of the C union's size (28
//!   bytes, the `sockaddr_in6` member) with accessors for the `sa`, `sin` and `sin6`
//!   views (28 bytes also without `INET6`, where the C union is a `sockaddr_in`'s 16). Every byte is always initialised, so the C's `memcmp`s over
//!   `sa_len` bytes are slice comparisons.
//! - `struct sockaddr_encap` is [`SockaddrEncap`], the 48 bytes of the C structure (the radix
//!   tree key of a policy) with getters and setters named after the `sen_*` macros at the C
//!   offsets, so the padding the C `bzero`s is part of the image and `bcmp` is `==`.
//! - The TDB, policy, acquire and identity structures are pool or `malloc` items reached as
//!   `&'static T`; the members the C changes through a shared pointer are `Cell`s whose doc
//!   names the lock. Their queue and tree links use `sys/queue.rs`/`sys/tree.rs` adapters.
//! - `tdb_counters` (`struct cpumem *` from `counters_alloc`) is an array of atomics inside
//!   the TDB, not per-CPU memory: every CPU adds to the same atomics (exact, if slower),
//!   and the array lives exactly as long as the TDB.
//! - `tdb_amxkey`/`tdb_emxkey` stay `malloc(M_XDATA)` pointers beside their lengths; the key
//!   bytes are lent out by [`Tdb::tdb_amxkey`]/[`Tdb::tdb_emxkey`] (`unsafe`: the key lives
//!   until the transform's `xf_zeroize`).
//! - The three hash tables (`tdbh`, `tdbdst`, `tdbsrc`), `tdbkey`, `tdb_hashmask` and
//!   `tdb_count` are one [`TdbTables`] in a `StaticCell` reached under `tdb_sadb_mtx`; the
//!   tables are `Vec`s (`mallocarray(M_TDB)` in C), grown by `tdb_rehash` with `try_reserve`
//!   (the C's `M_NOWAIT` failure is the same `ENOMEM`).
//! - `struct xformsw` is [`Xformsw`] of `fn` pointers with Rust signatures: `xf_input` takes
//!   the packet as `&mut Option<&'static Mbuf>` and returns the next protocol or
//!   `IPPROTO_DONE`, `xf_init`/`xf_zeroize`/`xf_output` return `Result`.
//! - `struct ipsecinit` is [`IpsecInit`] with the keys as borrowed slices (empty for NULL).
//! - `gettdb()`, `gettdb_rev()` and `gettdbbysrcdst*()` are functions; every lookup returns
//!   `Option<&'static Tdb>` holding a reference, as the C's `tdb_ref`.
//! - `tdb_walk` takes the walker as a closure (`docs/C_TO_RUST.md`); its `void *arg` is
//!   whatever the closure captures.
//! - `ipsp_address` needs `inet_ntop` (`netinet/inet_ntop.c`), which is not ported: it reports
//!   itself and answers a placeholder for `AF_INET` and `AF_INET6`, as `ifa_print_all` does. It is only used
//!   by `tdb_printit` and the `ENCDEBUG` messages.
//! - `DPRINTF` is [`ipsec_dprintf!`]: active with feature `encdebug` (OpenBSD's `option
//!   ENCDEBUG`, not in GENERIC) and the `net.inet.ip.encdebug` sysctl; otherwise its
//!   arguments are type-checked and never evaluated.
//! - Not configured, each a comment at its site: `NSEC` (`sec(4)`: `sec_tdb_insert`,
//!   `sec_tdb_remove`). `INET6` is configured (feature `inet6`): `ipsp_address` and
//!   `ipsp_is_unspecified` have their `AF_INET6` cases and the `sen_ip6_*` accessors take
//!   an `In6Addr`. `NPFSYNC` is configured (`pfsync_delete_tdb`), and so is
//!   `TCP_SIGNATURE` (M9+): `XF_TCPSIGNATURE` calls the `tcp_signature_tdb_*` functions of
//!   `netinet/tcp_subr.rs`.
//! - `NET_LOCK()`/`KERNEL_LOCK()` keep the C's places; `MUTEX_ASSERT_LOCKED` is
//!   `mutex_assert_locked`, active with feature `diagnostic`.

use alloc::vec::Vec;
use core::cell::Cell;
use core::cmp::Ordering as CmpOrdering;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::crypto::xform::{AuthHash, CompAlgo, EncXform};
use crate::dev::rnd::{arc4random_buf, arc4random_uniform};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::free;
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele, refcnt_take};
use crate::kern::kern_tc::{gettime, getuptime};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set_proc};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Bitmask, panic};
use crate::machine::db_machdep::PrFn;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_var::Netstack;
use crate::net::pf_ioctl::pf_tag_unref;
use crate::net::pfkeyv2::{
    SADB_EXT_LIFETIME_HARD, SADB_EXT_LIFETIME_SOFT, SADB_SATYPE_UNSPEC, pfkeyv2_expire,
};
use crate::net::radix::RadixNode;
use crate::netinet::in_::{INADDR_ANY, IPPROTO_IPCOMP, InAddr, SockaddrIn};
use crate::netinet::ip_ah::{ah_attach, ah_init, ah_input, ah_output, ah_zeroize};
use crate::netinet::ip_esp::{esp_attach, esp_init, esp_input, esp_output, esp_zeroize};
use crate::netinet::ip_ipcomp::{
    ipcomp_attach, ipcomp_init, ipcomp_input, ipcomp_output, ipcomp_zeroize,
};
use crate::netinet::ip_ipip::{ipe4_attach, ipe4_init, ipe4_input, ipe4_zeroize};
use crate::netinet::ipsec_input::{IPSEC_KEEP_INVALID, IPSECCOUNTERS};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::in6_is_addr_unspecified;
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::sys::endian::{htonl, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_CREDENTIALS;
use crate::sys::mbuf::Mbuf;
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, SimpleqEntry, SimpleqHead, TailqEntry, TailqHead};
use crate::sys::refcnt::{DT_REFCNT_IDX_TDB, Refcnt};
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, Sockaddr};
use crate::sys::systm::{net_assert_locked, net_assert_locked_exclusive, net_lock, net_unlock};
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC, Timeout};
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::SaFamily;
use crate::{kassert, queue_adapter, tree_adapter};
use libkern::staticcell::StaticCell;

/// `DPRINTF(fmt, args...)` of the IPsec files: `printf("%s: " fmt "\n", __func__, args)`
/// when the kernel has `option ENCDEBUG` (feature `encdebug`) and `net.inet.ip.encdebug` is
/// set. Without the feature the arguments are type-checked inside a closure that is never
/// called, as `kassert!` does.
#[macro_export]
macro_rules! ipsec_dprintf {
    ($func:expr, $($arg:tt)*) => {
        #[cfg(feature = "encdebug")]
        {
            if $crate::netinet::ipsec_input::ENCDEBUG
                .load(::core::sync::atomic::Ordering::Relaxed) != 0
            {
                $crate::kern::subr_prf::printf(::core::format_args!(
                    "{}: {}\n",
                    $func,
                    ::core::format_args!($($arg)*)
                ));
            }
        }
        #[cfg(not(feature = "encdebug"))]
        {
            let _ = || {
                let _ = $func;
                let _ = ::core::format_args!($($arg)*);
            };
        }
    };
}

/// `sizeof(struct sockaddr_in6)`, the largest member of `union sockaddr_union`.
pub const SIZEOF_SOCKADDR_IN6: usize = size_of::<SockaddrIn6>();

/// `union sockaddr_union`: a `struct sockaddr`, `struct sockaddr_in` or `struct
/// sockaddr_in6`, as the bytes of the largest (see the deviations).
#[repr(C, align(4))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SockaddrUnion {
    bytes: [u8; SIZEOF_SOCKADDR_IN6],
}

impl SockaddrUnion {
    /// An all-zero union (`memset(&su, 0, sizeof(su))`).
    pub const fn new() -> Self {
        Self {
            bytes: [0; SIZEOF_SOCKADDR_IN6],
        }
    }

    /// A union holding `sin` (the rest zero).
    pub fn from_sin(sin: &SockaddrIn) -> Self {
        let mut su = Self::new();
        su.set_sin(sin);
        su
    }

    /// The bytes of the union.
    pub const fn as_bytes(&self) -> &[u8; SIZEOF_SOCKADDR_IN6] {
        &self.bytes
    }

    /// The bytes of the union, writable.
    pub fn as_bytes_mut(&mut self) -> &mut [u8; SIZEOF_SOCKADDR_IN6] {
        &mut self.bytes
    }

    /// `sa.sa_len`.
    pub const fn sa_len(&self) -> u8 {
        self.bytes[0]
    }

    /// Sets `sa.sa_len`.
    pub fn set_sa_len(&mut self, len: u8) {
        self.bytes[0] = len;
    }

    /// `sa.sa_family`.
    pub const fn sa_family(&self) -> SaFamily {
        self.bytes[1]
    }

    /// Sets `sa.sa_family`.
    pub fn set_sa_family(&mut self, family: SaFamily) {
        self.bytes[1] = family;
    }

    /// The first `sa_len` bytes (at most the union's size), what the C's `memcmp(a, b,
    /// b->sa.sa_len)` and `SipHash24_Update(.., dst, dst->sa.sa_len)` read.
    pub fn sa_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.sa_len()).min(SIZEOF_SOCKADDR_IN6)]
    }

    /// The `sa` member.
    pub fn sa(&self) -> Sockaddr {
        // SAFETY: the union is at least `size_of::<Sockaddr>()` bytes, all initialised; a
        // `Sockaddr` is integers, valid for any bytes.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<Sockaddr>()) }
    }

    /// The `sin` member.
    pub fn sin(&self) -> SockaddrIn {
        // SAFETY: as in `sa`; a `SockaddrIn` is integers too.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<SockaddrIn>()) }
    }

    /// Writes the `sin` member.
    pub fn set_sin(&mut self, sin: &SockaddrIn) {
        // SAFETY: the union holds `size_of::<SockaddrIn>()` bytes; `SockaddrIn` has no
        // padding (pinned below), so every byte written is initialised.
        unsafe { ptr::write_unaligned(self.bytes.as_mut_ptr().cast::<SockaddrIn>(), *sin) };
    }

    /// `sin.sin_addr`.
    pub fn sin_addr(&self) -> InAddr {
        self.sin().sin_addr
    }

    /// A union holding `sin6`.
    pub fn from_sin6(sin6: &SockaddrIn6) -> Self {
        let mut su = Self::new();
        su.set_sin6(sin6);
        su
    }

    /// The `sin6` member.
    pub fn sin6(&self) -> SockaddrIn6 {
        // SAFETY: the union is `size_of::<SockaddrIn6>()` bytes, all initialised; a
        // `SockaddrIn6` is integers and byte arrays, valid for any bytes.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<SockaddrIn6>()) }
    }

    /// Writes the `sin6` member.
    pub fn set_sin6(&mut self, sin6: &SockaddrIn6) {
        // SAFETY: the union holds exactly `size_of::<SockaddrIn6>()` bytes; `SockaddrIn6`
        // has no padding (28 bytes of integer fields, pinned by the size assertion).
        unsafe { ptr::write_unaligned(self.bytes.as_mut_ptr().cast::<SockaddrIn6>(), *sin6) };
    }

    /// `sin6.sin6_addr`.
    pub fn sin6_addr(&self) -> In6Addr {
        self.sin6().sin6_addr
    }

    /// Copies the first `sa_len` bytes of the socket address at `sa` over the union
    /// (`memcpy(&su.sa, sa, sa->sa_len)`), at most the union's size.
    ///
    /// # Safety
    ///
    /// `sa` points to a socket address readable for its `sa_len` bytes (or the union's size,
    /// whichever is smaller).
    pub unsafe fn copy_from_sa(&mut self, sa: *const Sockaddr) {
        // SAFETY: the caller's contract.
        let len = usize::from(unsafe { (*sa).sa_len }).min(SIZEOF_SOCKADDR_IN6);
        // SAFETY: `len` bytes are readable (the caller's contract) and fit in the union.
        unsafe { ptr::copy_nonoverlapping(sa.cast::<u8>(), self.bytes.as_mut_ptr(), len) };
    }

    /// The union as a `struct sockaddr *`, for the routing calls.
    pub fn as_sockaddr_ptr(&self) -> *const Sockaddr {
        self.bytes.as_ptr().cast()
    }

    /// The union as a writable `struct sockaddr *` (`&su.sa`).
    pub fn as_sockaddr_mut_ptr(&mut self) -> *mut Sockaddr {
        self.bytes.as_mut_ptr().cast()
    }
}

impl Default for SockaddrUnion {
    fn default() -> Self {
        Self::new()
    }
}

/// `AH_HMAC_MAX_HASHLEN`: 256 bits of authenticator for SHA512.
pub const AH_HMAC_MAX_HASHLEN: usize = 32;
/// `AH_HMAC_RPLENGTH`: 32 bits of replay counter.
pub const AH_HMAC_RPLENGTH: usize = 4;
/// `AH_HMAC_INITIAL_RPL`: replay counter initial value.
pub const AH_HMAC_INITIAL_RPL: u64 = 1;

// Authenticator lengths

/// `AH_MD5_ALEN`.
pub const AH_MD5_ALEN: usize = 16;
/// `AH_SHA1_ALEN`.
pub const AH_SHA1_ALEN: usize = 20;
/// `AH_RMD160_ALEN`.
pub const AH_RMD160_ALEN: usize = 20;
/// `AH_SHA2_256_ALEN`.
pub const AH_SHA2_256_ALEN: usize = 32;
/// `AH_SHA2_384_ALEN`.
pub const AH_SHA2_384_ALEN: usize = 48;
/// `AH_SHA2_512_ALEN`.
pub const AH_SHA2_512_ALEN: usize = 64;
/// `AH_ALEN_MAX`: keep updated.
pub const AH_ALEN_MAX: usize = 64;

// Reserved SPI numbers

/// `SPI_LOCAL_USE`.
pub const SPI_LOCAL_USE: u32 = 0;
/// `SPI_RESERVED_MIN`.
pub const SPI_RESERVED_MIN: u32 = 1;
/// `SPI_RESERVED_MAX`.
pub const SPI_RESERVED_MAX: u32 = 255;

// Reserved CPI numbers

/// `CPI_RESERVED_MIN`.
pub const CPI_RESERVED_MIN: u32 = 1;
/// `CPI_RESERVED_MAX`.
pub const CPI_RESERVED_MAX: u32 = 255;
/// `CPI_PRIVATE_MIN`.
pub const CPI_PRIVATE_MIN: u32 = 61440;
/// `CPI_PRIVATE_MAX`.
pub const CPI_PRIVATE_MAX: u32 = 65535;

// sysctl default values

/// `IPSEC_DEFAULT_EMBRYONIC_SA_TIMEOUT`: 1 minute.
pub const IPSEC_DEFAULT_EMBRYONIC_SA_TIMEOUT: i32 = 60;
/// `IPSEC_DEFAULT_PFS`.
pub const IPSEC_DEFAULT_PFS: i32 = 1;
/// `IPSEC_DEFAULT_SOFT_ALLOCATIONS`.
pub const IPSEC_DEFAULT_SOFT_ALLOCATIONS: i32 = 0;
/// `IPSEC_DEFAULT_EXP_ALLOCATIONS`.
pub const IPSEC_DEFAULT_EXP_ALLOCATIONS: i32 = 0;
/// `IPSEC_DEFAULT_SOFT_BYTES`.
pub const IPSEC_DEFAULT_SOFT_BYTES: i32 = 0;
/// `IPSEC_DEFAULT_EXP_BYTES`.
pub const IPSEC_DEFAULT_EXP_BYTES: i32 = 0;
/// `IPSEC_DEFAULT_SOFT_TIMEOUT`.
pub const IPSEC_DEFAULT_SOFT_TIMEOUT: i32 = 80000;
/// `IPSEC_DEFAULT_EXP_TIMEOUT`.
pub const IPSEC_DEFAULT_EXP_TIMEOUT: i32 = 86400;
/// `IPSEC_DEFAULT_SOFT_FIRST_USE`.
pub const IPSEC_DEFAULT_SOFT_FIRST_USE: i32 = 3600;
/// `IPSEC_DEFAULT_EXP_FIRST_USE`.
pub const IPSEC_DEFAULT_EXP_FIRST_USE: i32 = 7200;
/// `IPSEC_DEFAULT_DEF_ENC`.
pub const IPSEC_DEFAULT_DEF_ENC: &[u8] = b"aes";
/// `IPSEC_DEFAULT_DEF_AUTH`.
pub const IPSEC_DEFAULT_DEF_AUTH: &[u8] = b"hmac-sha1";
/// `IPSEC_DEFAULT_EXPIRE_ACQUIRE`.
pub const IPSEC_DEFAULT_EXPIRE_ACQUIRE: i32 = 30;
/// `IPSEC_DEFAULT_DEF_COMP`.
pub const IPSEC_DEFAULT_DEF_COMP: &[u8] = b"deflate";

/// `SENT_LEN`: `sizeof(struct sockaddr_encap)`.
pub const SENT_LEN: usize = 48;

/// `struct sockaddr_encap`: the key of a flow in the SPD, `PF_KEY` family. The union `Sen`
/// holds `Sip4` (`SENT_IP4`) or `Sip6` (`SENT_IP6`); the accessors below are the `sen_*`
/// macros (see the deviations).
#[repr(C, align(4))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SockaddrEncap {
    bytes: [u8; SENT_LEN],
}

/// Byte offsets inside `struct sockaddr_encap` (LP64, as the C compiler lays it out).
mod sen_off {
    pub const LEN: usize = 0;
    pub const FAMILY: usize = 1;
    pub const TYPE: usize = 2;
    /// `Sen` starts at 4 (4-byte aligned: `struct in_addr`).
    pub const SEN: usize = 4;
    pub const DIRECTION: usize = SEN;
    pub const IP_SRC: usize = SEN + 4;
    pub const IP_DST: usize = SEN + 8;
    pub const PROTO: usize = SEN + 12;
    pub const SPORT: usize = SEN + 14;
    pub const DPORT: usize = SEN + 16;
    pub const IP6_SRC: usize = SEN + 4;
    pub const IP6_DST: usize = SEN + 20;
    pub const IP6_PROTO: usize = SEN + 36;
    pub const IP6_SPORT: usize = SEN + 38;
    pub const IP6_DPORT: usize = SEN + 40;
}

macro_rules! sen_u8 {
    ($get:ident, $set:ident, $off:expr, $doc:literal) => {
        #[doc = $doc]
        pub const fn $get(&self) -> u8 {
            self.bytes[$off]
        }

        #[doc = concat!("Sets ", $doc)]
        pub fn $set(&mut self, v: u8) {
            self.bytes[$off] = v;
        }
    };
}

macro_rules! sen_u16 {
    ($get:ident, $set:ident, $off:expr, $doc:literal) => {
        #[doc = $doc]
        pub const fn $get(&self) -> u16 {
            u16::from_ne_bytes([self.bytes[$off], self.bytes[$off + 1]])
        }

        #[doc = concat!("Sets ", $doc)]
        pub fn $set(&mut self, v: u16) {
            self.bytes[$off..$off + 2].copy_from_slice(&v.to_ne_bytes());
        }
    };
}

macro_rules! sen_in_addr {
    ($get:ident, $set:ident, $off:expr, $doc:literal) => {
        #[doc = $doc]
        pub const fn $get(&self) -> InAddr {
            InAddr {
                s_addr: u32::from_ne_bytes([
                    self.bytes[$off],
                    self.bytes[$off + 1],
                    self.bytes[$off + 2],
                    self.bytes[$off + 3],
                ]),
            }
        }

        #[doc = concat!("Sets ", $doc)]
        pub fn $set(&mut self, v: InAddr) {
            self.bytes[$off..$off + 4].copy_from_slice(&v.s_addr.to_ne_bytes());
        }
    };
}

macro_rules! sen_in6_addr {
    ($get:ident, $set:ident, $off:expr, $doc:literal) => {
        #[doc = $doc]
        pub fn $get(&self) -> In6Addr {
            let mut a = [0u8; 16];
            a.copy_from_slice(&self.bytes[$off..$off + 16]);
            In6Addr::new(a)
        }

        #[doc = concat!("Sets ", $doc)]
        pub fn $set(&mut self, v: In6Addr) {
            self.bytes[$off..$off + 16].copy_from_slice(&v.s6_addr);
        }
    };
}

impl SockaddrEncap {
    /// An all-zero `struct sockaddr_encap` (`bzero`).
    pub const fn new() -> Self {
        Self {
            bytes: [0; SENT_LEN],
        }
    }

    /// The structure's bytes: the radix key.
    pub const fn as_bytes(&self) -> &[u8; SENT_LEN] {
        &self.bytes
    }

    sen_u8!(sen_len, set_sen_len, sen_off::LEN, "`sen_len`: length.");
    sen_u8!(
        sen_family,
        set_sen_family,
        sen_off::FAMILY,
        "`sen_family`: `PF_KEY`."
    );
    sen_u16!(
        sen_type,
        set_sen_type,
        sen_off::TYPE,
        "`sen_type`: see `SENT_*`."
    );
    sen_u8!(
        sen_direction,
        set_sen_direction,
        sen_off::DIRECTION,
        "`sen_direction` (`Sen.Sip4.Direction`)."
    );
    sen_in_addr!(
        sen_ip_src,
        set_sen_ip_src,
        sen_off::IP_SRC,
        "`sen_ip_src` (`Sen.Sip4.Src`)."
    );
    sen_in_addr!(
        sen_ip_dst,
        set_sen_ip_dst,
        sen_off::IP_DST,
        "`sen_ip_dst` (`Sen.Sip4.Dst`)."
    );
    sen_u8!(
        sen_proto,
        set_sen_proto,
        sen_off::PROTO,
        "`sen_proto` (`Sen.Sip4.Proto`)."
    );
    sen_u16!(
        sen_sport,
        set_sen_sport,
        sen_off::SPORT,
        "`sen_sport` (`Sen.Sip4.Sport`), network order."
    );
    sen_u16!(
        sen_dport,
        set_sen_dport,
        sen_off::DPORT,
        "`sen_dport` (`Sen.Sip4.Dport`), network order."
    );
    sen_u8!(
        sen_ip6_direction,
        set_sen_ip6_direction,
        sen_off::DIRECTION,
        "`sen_ip6_direction` (`Sen.Sip6.Direction`)."
    );
    sen_in6_addr!(
        sen_ip6_src,
        set_sen_ip6_src,
        sen_off::IP6_SRC,
        "`sen_ip6_src` (`Sen.Sip6.Src`)."
    );
    sen_in6_addr!(
        sen_ip6_dst,
        set_sen_ip6_dst,
        sen_off::IP6_DST,
        "`sen_ip6_dst` (`Sen.Sip6.Dst`)."
    );
    sen_u8!(
        sen_ip6_proto,
        set_sen_ip6_proto,
        sen_off::IP6_PROTO,
        "`sen_ip6_proto` (`Sen.Sip6.Proto`)."
    );
    sen_u16!(
        sen_ip6_sport,
        set_sen_ip6_sport,
        sen_off::IP6_SPORT,
        "`sen_ip6_sport` (`Sen.Sip6.Sport`)."
    );
    sen_u16!(
        sen_ip6_dport,
        set_sen_ip6_dport,
        sen_off::IP6_DPORT,
        "`sen_ip6_dport` (`Sen.Sip6.Dport`)."
    );
}

impl Default for SockaddrEncap {
    fn default() -> Self {
        Self::new()
    }
}

/// `IPSP_DIRECTION_IN`.
pub const IPSP_DIRECTION_IN: u8 = 0x1;
/// `IPSP_DIRECTION_OUT`.
pub const IPSP_DIRECTION_OUT: u8 = 0x2;

/// `struct ipsecstat`: the IPsec statistics as `net.inet.ip.ipsec-stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ipsecstat {
    /// Number of active tunnels.
    pub ipsec_tunnels: u64,
    /// Past number of tunnels.
    pub ipsec_prevtunnels: u64,
    /// Input IPsec packets.
    pub ipsec_ipackets: u64,
    /// Output IPsec packets.
    pub ipsec_opackets: u64,
    /// Input bytes.
    pub ipsec_ibytes: u64,
    /// Output bytes.
    pub ipsec_obytes: u64,
    /// Input bytes, decompressed.
    pub ipsec_idecompbytes: u64,
    /// Output bytes, uncompressed.
    pub ipsec_ouncompbytes: u64,
    /// Dropped on input.
    pub ipsec_idrops: u64,
    /// Dropped on output.
    pub ipsec_odrops: u64,
    /// Crypto processing failure.
    pub ipsec_crypto: u64,
    /// No TDB was found.
    pub ipsec_notdb: u64,
    /// Crypto error.
    pub ipsec_noxform: u64,
    /// TDBs with hardlimit excess.
    pub ipsec_exctdb: u64,
}

/// `struct ipsec_level`: the security levels of a socket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpsecLevel {
    /// Authentication level.
    pub sl_auth: u8,
    /// ESP transport level.
    pub sl_esp_trans: u8,
    /// ESP network (encapsulation) level.
    pub sl_esp_network: u8,
    /// Compression level.
    pub sl_ipcomp: u8,
}

/// `enum ipsec_counters`: one per field of [`Ipsecstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IpsecCounters {
    /// `ipsec_tunnels`.
    IpsecTunnels,
    /// `ipsec_prevtunnels`.
    IpsecPrevtunnels,
    /// `ipsec_ipackets`.
    IpsecIpackets,
    /// `ipsec_opackets`.
    IpsecOpackets,
    /// `ipsec_ibytes`.
    IpsecIbytes,
    /// `ipsec_obytes`.
    IpsecObytes,
    /// `ipsec_idecompbytes`.
    IpsecIdecompbytes,
    /// `ipsec_ouncompbytes`.
    IpsecOuncompbytes,
    /// `ipsec_idrops`.
    IpsecIdrops,
    /// `ipsec_odrops`.
    IpsecOdrops,
    /// `ipsec_crypto`.
    IpsecCrypto,
    /// `ipsec_notdb`.
    IpsecNotdb,
    /// `ipsec_noxform`.
    IpsecNoxform,
    /// `ipsec_exctdb`.
    IpsecExctdb,
    /// `ipsec_ncounters`.
    IpsecNcounters,
}

/// `ipsecstat_inc(c)`.
pub fn ipsecstat_inc(c: IpsecCounters) {
    IPSECCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ipsecstat_dec(c)`.
pub fn ipsecstat_dec(c: IpsecCounters) {
    IPSECCOUNTERS[c as usize].fetch_sub(1, Ordering::Relaxed);
}

/// `ipsecstat_add(c, v)`.
pub fn ipsecstat_add(c: IpsecCounters, v: u64) {
    IPSECCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ipsecstat_pkt(p, b, v)`: one packet of `v` bytes.
pub fn ipsecstat_pkt(p: IpsecCounters, b: IpsecCounters, v: u64) {
    IPSECCOUNTERS[p as usize].fetch_add(1, Ordering::Relaxed);
    IPSECCOUNTERS[b as usize].fetch_add(v, Ordering::Relaxed);
}

/// `SENT_IP4`: data is two `struct in_addr`. The "type" is really part of the address as far
/// as the routing system is concerned. By using only one bit in the type field for each type,
/// we sort-of make sure that different types of encapsulation addresses won't be matched
/// against the wrong type.
pub const SENT_IP4: u16 = 0x0001;
/// `SENT_IP6`.
pub const SENT_IP6: u16 = 0x0002;

/// `struct ipsec_id`: the header of an identity; `len` bytes of data follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpsecId {
    /// Subtype of data.
    pub type_: u16,
    /// Length of data following.
    pub len: i16,
}

impl IpsecId {
    /// The data following the header (`id + 1`).
    pub fn data(&self) -> &[u8] {
        let len = usize::try_from(self.len).unwrap_or(0);
        // SAFETY: an identity is allocated with its `len` bytes of data right after the
        // header (`import_identity`), and is immutable while referenced.
        unsafe { slice::from_raw_parts(ptr::from_ref(self).add(1).cast::<u8>(), len) }
    }
}

/// `struct ipsec_ids`: a pair of identities, with the flow number the SPD and the sockets
/// refer to it by. Allocated with `malloc(M_CREDENTIALS)`, shared, reference counted, freed
/// by the garbage collector `ipsp_ids_gc` some time after the last reference went.
pub struct IpsecIds {
    /// \[F\] `id_gc_list`.
    pub id_gc_list: ListEntry<IpsecIds>,
    /// \[F\] `id_node_id`.
    pub id_node_id: RbtEntry,
    /// \[F\] `id_node_flow`.
    pub id_node_flow: RbtEntry,
    /// \[I\] `id_local`.
    pub id_local: NonNull<IpsecId>,
    /// \[I\] `id_remote`.
    pub id_remote: NonNull<IpsecId>,
    /// \[I\] `id_flow`.
    pub id_flow: Cell<u32>,
    /// \[F\] `id_refcount`.
    pub id_refcount: Cell<u32>,
    /// \[F\] `id_gc_ttl`.
    pub id_gc_ttl: Cell<u32>,
}

// SAFETY: the links and counts change under `ipsec_flows_mtx`; the identities are immutable.
unsafe impl Sync for IpsecIds {}
// SAFETY: as above.
unsafe impl Send for IpsecIds {}

impl IpsecIds {
    /// A pair of identities that is in no tree yet.
    pub const fn new(id_local: NonNull<IpsecId>, id_remote: NonNull<IpsecId>) -> Self {
        Self {
            id_gc_list: ListEntry::new(),
            id_node_id: RbtEntry::new(),
            id_node_flow: RbtEntry::new(),
            id_local,
            id_remote,
            id_flow: Cell::new(0),
            id_refcount: Cell::new(0),
            id_gc_ttl: Cell::new(0),
        }
    }

    /// `id_local`.
    pub fn id_local(&self) -> &IpsecId {
        // SAFETY: \[I\] set at creation to an identity that lives as long as the pair.
        unsafe { self.id_local.as_ref() }
    }

    /// `id_remote`.
    pub fn id_remote(&self) -> &IpsecId {
        // SAFETY: as for `id_local`.
        unsafe { self.id_remote.as_ref() }
    }
}

tree_adapter!(
    /// `RBT_HEAD(ipsec_ids_tree, ipsec_ids)`: the pairs by identities (through `id_node_flow`,
    /// as the C's `RBT_GENERATE` has it).
    pub IpsecIdsTree: IpsecIds, id_node_flow => RbtEntry, ipsp_ids_cmp
);

tree_adapter!(
    /// `RBT_HEAD(ipsec_ids_flows, ipsec_ids)`: the pairs by flow number (through
    /// `id_node_id`).
    pub IpsecIdsFlows: IpsecIds, id_node_id => RbtEntry, ipsp_ids_flow_cmp
);

queue_adapter!(
    /// `LIST_HEAD(, ipsec_ids)` through `id_gc_list`: the pairs waiting to be freed.
    pub IpsecIdsGcList: IpsecIds, id_gc_list => ListEntry<IpsecIds>
);

/// `struct ipsec_acquire`: a pending request to key management for an SA, made when a
/// policy needed one that did not exist.
pub struct IpsecAcquire {
    /// `ipa_addr`.
    pub ipa_addr: SockaddrUnion,
    /// \[A\] `ipa_seq`: the `SADB_ACQUIRE` sequence number.
    pub ipa_seq: Cell<u32>,
    /// `ipa_info`.
    pub ipa_info: SockaddrEncap,
    /// `ipa_mask`.
    pub ipa_mask: SockaddrEncap,
    /// `ipa_refcnt`.
    pub ipa_refcnt: Refcnt,
    /// `ipa_timeout`.
    pub ipa_timeout: Timeout,
    /// \[A\] `ipa_policy`: back pointer.
    pub ipa_policy: Cell<Option<&'static IpsecPolicy>>,
    /// \[A\] `ipa_ipo_next`: per policy.
    pub ipa_ipo_next: TailqEntry<IpsecAcquire>,
    /// \[A\] `ipa_next`: global list.
    pub ipa_next: TailqEntry<IpsecAcquire>,
}

// SAFETY: the members change under `ipsec_acquire_mtx`; the reference count is atomic.
unsafe impl Sync for IpsecAcquire {}

queue_adapter!(
    /// `TAILQ_HEAD(ipsec_acquire_head, ipsec_acquire)` through `ipa_next`.
    pub IpsecAcquireHead: IpsecAcquire, ipa_next => TailqEntry<IpsecAcquire>
);

queue_adapter!(
    /// The acquires of one policy (`ipo_acquires`), through `ipa_ipo_next`.
    pub IpsecAcquirePolicyList: IpsecAcquire, ipa_ipo_next => TailqEntry<IpsecAcquire>
);

/// `struct ipsec_policy`: an entry of the SPD (a flow), in the radix tree of its routing
/// domain. `ipo_nodes` is first: the tree's `struct radix_node *` is cast back to the policy.
#[repr(C)]
pub struct IpsecPolicy {
    /// `ipo_nodes`: radix tree glue.
    pub ipo_nodes: [RadixNode; 2],
    /// `ipo_addr`: the flow (the radix key), written before the policy enters the tree.
    pub ipo_addr: Cell<SockaddrEncap>,
    /// `ipo_mask`: its mask (the radix mask).
    pub ipo_mask: Cell<SockaddrEncap>,
    /// `ipo_src`: local address to use.
    pub ipo_src: Cell<SockaddrUnion>,
    /// `ipo_dst`: remote gateway -- if it's zeroed: on output, we try to contact the remote
    /// host directly (if needed); on input, we accept on if the inner source is the same as
    /// the outer source address, or if transport mode was used.
    pub ipo_dst: Cell<SockaddrUnion>,
    /// \[P\] `ipo_last_searched`: timestamp of lookup.
    pub ipo_last_searched: Cell<u64>,
    /// `ipo_flags`: see `IPSP_POLICY_*`.
    pub ipo_flags: Cell<u8>,
    /// `ipo_type`: USE/ACQUIRE/...
    pub ipo_type: Cell<u8>,
    /// `ipo_sproto`: ESP/AH; if zero, use system dflts.
    pub ipo_sproto: Cell<u8>,
    /// `ipo_rdomain`.
    pub ipo_rdomain: Cell<u32>,
    /// `ipo_refcnt`.
    pub ipo_refcnt: Refcnt,
    /// \[P\] `ipo_tdb`: cached TDB entry.
    pub ipo_tdb: Cell<Option<&'static Tdb>>,
    /// `ipo_ids`.
    pub ipo_ids: Cell<Option<&'static IpsecIds>>,
    /// \[A\] `ipo_acquires`: list of acquires.
    pub ipo_acquires: TailqHead<IpsecAcquirePolicyList>,
    /// \[P\] `ipo_tdb_next`: list TDB policies.
    pub ipo_tdb_next: TailqEntry<IpsecPolicy>,
    /// `ipo_list`: list of all policies.
    pub ipo_list: TailqEntry<IpsecPolicy>,
}

// SAFETY: the members change under the locks their docs name, or under the net lock.
unsafe impl Sync for IpsecPolicy {}

impl IpsecPolicy {
    /// A zeroed policy (`pool_get(PR_ZERO)`).
    pub const fn new() -> Self {
        Self {
            ipo_nodes: [RadixNode::new(), RadixNode::new()],
            ipo_addr: Cell::new(SockaddrEncap::new()),
            ipo_mask: Cell::new(SockaddrEncap::new()),
            ipo_src: Cell::new(SockaddrUnion::new()),
            ipo_dst: Cell::new(SockaddrUnion::new()),
            ipo_last_searched: Cell::new(0),
            ipo_flags: Cell::new(0),
            ipo_type: Cell::new(0),
            ipo_sproto: Cell::new(0),
            ipo_rdomain: Cell::new(0),
            ipo_refcnt: Refcnt::new(),
            ipo_tdb: Cell::new(None),
            ipo_ids: Cell::new(None),
            ipo_acquires: TailqHead::new(),
            ipo_tdb_next: TailqEntry::new(),
            ipo_list: TailqEntry::new(),
        }
    }

    /// `ipo_addr`'s bytes as the radix key (`(caddr_t)&ipo->ipo_addr`).
    pub fn ipo_addr_key(&self) -> *const u8 {
        self.ipo_addr.as_ptr().cast_const().cast()
    }

    /// `ipo_mask`'s bytes as the radix mask.
    pub fn ipo_mask_key(&self) -> *const u8 {
        self.ipo_mask.as_ptr().cast_const().cast()
    }
}

impl Default for IpsecPolicy {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(tdb_policy_head, ipsec_policy)`: the policies that cache a TDB, through
    /// `ipo_tdb_next`.
    pub TdbPolicyHead: IpsecPolicy, ipo_tdb_next => TailqEntry<IpsecPolicy>
);

queue_adapter!(
    /// `TAILQ_HEAD(ipsec_policy_head, ipsec_policy)`: every policy, through `ipo_list`.
    pub IpsecPolicyHead: IpsecPolicy, ipo_list => TailqEntry<IpsecPolicy>
);

/// `IPSP_POLICY_NONE`: no flags set.
pub const IPSP_POLICY_NONE: u8 = 0x0000;
/// `IPSP_POLICY_STATIC`: static policy.
pub const IPSP_POLICY_STATIC: u8 = 0x0002;

/// `IPSP_IPSEC_USE`: use if existing, don't acquire.
pub const IPSP_IPSEC_USE: u8 = 0;
/// `IPSP_IPSEC_ACQUIRE`: try acquire, let packet through.
pub const IPSP_IPSEC_ACQUIRE: u8 = 1;
/// `IPSP_IPSEC_REQUIRE`: require SA.
pub const IPSP_IPSEC_REQUIRE: u8 = 2;
/// `IPSP_PERMIT`: permit traffic through.
pub const IPSP_PERMIT: u8 = 3;
/// `IPSP_DENY`: deny traffic.
pub const IPSP_DENY: u8 = 4;
/// `IPSP_IPSEC_DONTACQ`: require, but don't acquire.
pub const IPSP_IPSEC_DONTACQ: u8 = 5;

// Identity types

/// `IPSP_IDENTITY_NONE`.
pub const IPSP_IDENTITY_NONE: u16 = 0;
/// `IPSP_IDENTITY_PREFIX`.
pub const IPSP_IDENTITY_PREFIX: u16 = 1;
/// `IPSP_IDENTITY_FQDN`.
pub const IPSP_IDENTITY_FQDN: u16 = 2;
/// `IPSP_IDENTITY_USERFQDN`.
pub const IPSP_IDENTITY_USERFQDN: u16 = 3;
/// `IPSP_IDENTITY_ASN1_DN`.
pub const IPSP_IDENTITY_ASN1_DN: u16 = 4;

/// `TDBF_UNIQUE`: this should not be used by others.
pub const TDBF_UNIQUE: u32 = 0x00001;
/// `TDBF_TIMER`: absolute expiration timer in use.
pub const TDBF_TIMER: u32 = 0x00002;
/// `TDBF_BYTES`: check the byte counters.
pub const TDBF_BYTES: u32 = 0x00004;
/// `TDBF_ALLOCATIONS`: check the flows counters.
pub const TDBF_ALLOCATIONS: u32 = 0x00008;
/// `TDBF_INVALID`: this SPI is not valid yet/anymore.
pub const TDBF_INVALID: u32 = 0x00010;
/// `TDBF_FIRSTUSE`: expire after first use.
pub const TDBF_FIRSTUSE: u32 = 0x00020;
/// `TDBF_DELETED`: this TDB has already been deleted.
pub const TDBF_DELETED: u32 = 0x00040;
/// `TDBF_SOFT_TIMER`: soft expiration.
pub const TDBF_SOFT_TIMER: u32 = 0x00080;
/// `TDBF_SOFT_BYTES`: soft expiration.
pub const TDBF_SOFT_BYTES: u32 = 0x00100;
/// `TDBF_SOFT_ALLOCATIONS`: soft expiration.
pub const TDBF_SOFT_ALLOCATIONS: u32 = 0x00200;
/// `TDBF_SOFT_FIRSTUSE`: soft expiration.
pub const TDBF_SOFT_FIRSTUSE: u32 = 0x00400;
/// `TDBF_PFS`: ask for PFS from Key Mgmt.
pub const TDBF_PFS: u32 = 0x00800;
/// `TDBF_TUNNELING`: force IP-IP encapsulation.
pub const TDBF_TUNNELING: u32 = 0x01000;
/// `TDBF_USEDTUNNEL`: appended a tunnel header in past.
pub const TDBF_USEDTUNNEL: u32 = 0x10000;
/// `TDBF_UDPENCAP`: UDP encapsulation.
pub const TDBF_UDPENCAP: u32 = 0x20000;
/// `TDBF_PFSYNC`: TDB will be synced.
pub const TDBF_PFSYNC: u32 = 0x40000;
/// `TDBF_PFSYNC_RPL`: replay counter should be bumped.
pub const TDBF_PFSYNC_RPL: u32 = 0x80000;
/// `TDBF_ESN`: 64-bit sequence numbers (ESN).
pub const TDBF_ESN: u32 = 0x100000;
/// `TDBF_PFSYNC_SNAPPED`: entry is being dispatched to peer.
pub const TDBF_PFSYNC_SNAPPED: u32 = 0x200000;
/// `TDBF_IFACE`: entry policy is via `sec(4)`.
pub const TDBF_IFACE: u32 = 0x400000;

/// `TDBF_BITS`: the `%b` description of `tdb_flags`.
pub const TDBF_BITS: &[u8] = b"\x10\x01UNIQUE\x02TIMER\x03BYTES\x04ALLOCATIONS\
\x05INVALID\x06FIRSTUSE\x07DELETED\x08SOFT_TIMER\
\x09SOFT_BYTES\x0aSOFT_ALLOCATIONS\x0bSOFT_FIRSTUSE\x0cPFS\
\x0dTUNNELING\
\x11USEDTUNNEL\x12UDPENCAP\x13PFSYNC\x14PFSYNC_RPL\
\x15ESN\x16IFACE";

/// `TDB_REPLAYWASTE`.
pub const TDB_REPLAYWASTE: u32 = 32;
/// `TDB_REPLAYMAX`.
pub const TDB_REPLAYMAX: u32 = 2100 + TDB_REPLAYWASTE;
/// `howmany(TDB_REPLAYMAX, 32)`: the words of the anti-replay window (`SEEN_SIZE` in
/// `ip_esp.c`).
pub const TDB_SEEN_WORDS: usize = TDB_REPLAYMAX.div_ceil(32) as usize;

/// `enum tdb_counters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TdbCounters {
    /// Input IPsec packets.
    TdbIpackets,
    /// Output IPsec packets.
    TdbOpackets,
    /// Input bytes.
    TdbIbytes,
    /// Output bytes.
    TdbObytes,
    /// Dropped on input.
    TdbIdrops,
    /// Dropped on output.
    TdbOdrops,
    /// Input bytes, decompressed.
    TdbIdecompbytes,
    /// Output bytes, uncompressed.
    TdbOuncompbytes,
    /// `tdb_ncounters`.
    TdbNcounters,
}

/// `tdb_ncounters`.
pub const TDB_NCOUNTERS: usize = TdbCounters::TdbNcounters as usize;

/// `struct tdb`: tunnel descriptor block, one security association.
pub struct Tdb {
    /// \[D\] `tdb_hnext`: dst/spi/sproto table.
    pub tdb_hnext: Cell<Option<&'static Tdb>>,
    /// \[D\] `tdb_dnext`: dst/sproto table.
    pub tdb_dnext: Cell<Option<&'static Tdb>>,
    /// \[D\] `tdb_snext`: src/sproto table.
    pub tdb_snext: Cell<Option<&'static Tdb>>,
    /// `tdb_inext`: the SA applied before this one on input (bundles).
    pub tdb_inext: Cell<Option<&'static Tdb>>,
    /// `tdb_onext`: the SA applied after this one on output (bundles).
    pub tdb_onext: Cell<Option<&'static Tdb>>,
    /// \[N\] `tdb_walk`: temp list for tdb walker.
    pub tdb_walk: SimpleqEntry<Tdb>,

    /// `tdb_refcnt`.
    pub tdb_refcnt: Refcnt,
    /// `tdb_mtx`.
    pub tdb_mtx: Mutex,

    /// `tdb_xform`: transform to use.
    pub tdb_xform: Cell<Option<&'static Xformsw>>,
    /// `tdb_encalgxform`: enc algorithm.
    pub tdb_encalgxform: Cell<Option<&'static EncXform>>,
    /// `tdb_authalgxform`: auth algorithm.
    pub tdb_authalgxform: Cell<Option<&'static AuthHash>>,
    /// `tdb_compalgxform`: compression algo.
    pub tdb_compalgxform: Cell<Option<&'static CompAlgo>>,

    /// \[m\] `tdb_flags`: flags related to this TDB (`TDBF_*`).
    pub tdb_flags: Cell<u32>,

    /// `tdb_timer_tmo`.
    pub tdb_timer_tmo: Timeout,
    /// `tdb_first_tmo`.
    pub tdb_first_tmo: Timeout,
    /// `tdb_stimer_tmo`.
    pub tdb_stimer_tmo: Timeout,
    /// `tdb_sfirst_tmo`.
    pub tdb_sfirst_tmo: Timeout,

    /// `tdb_seq`: tracking number for PFKEY.
    pub tdb_seq: Cell<u32>,
    /// `tdb_exp_allocations`: expire after so many flows.
    pub tdb_exp_allocations: Cell<u32>,
    /// `tdb_soft_allocations`: expiration warning.
    pub tdb_soft_allocations: Cell<u32>,
    /// `tdb_cur_allocations`: total number of allocs.
    pub tdb_cur_allocations: Cell<u32>,

    /// `tdb_exp_bytes`: expire after so many bytes passed.
    pub tdb_exp_bytes: Cell<u64>,
    /// `tdb_soft_bytes`: expiration warning.
    pub tdb_soft_bytes: Cell<u64>,
    /// `tdb_cur_bytes`: current count of bytes.
    pub tdb_cur_bytes: Cell<u64>,

    /// `tdb_exp_timeout`: when does the SPI expire.
    pub tdb_exp_timeout: Cell<u64>,
    /// `tdb_soft_timeout`: send soft-expire warning.
    pub tdb_soft_timeout: Cell<u64>,
    /// `tdb_established`: when was SPI established.
    pub tdb_established: Cell<u64>,

    /// `tdb_first_use`: when was it first used.
    pub tdb_first_use: Cell<u64>,
    /// `tdb_soft_first_use`: soft warning.
    pub tdb_soft_first_use: Cell<u64>,
    /// `tdb_exp_first_use`: expire if `tdb_first_use + tdb_exp_first_use <= curtime`.
    pub tdb_exp_first_use: Cell<u64>,

    /// `tdb_last_used`: when was this SA last used.
    pub tdb_last_used: Cell<u64>,
    /// `tdb_last_marked`: last SKIPCRYPTO status change.
    pub tdb_last_marked: Cell<u64>,

    /// `tdb_counters`: stats about this TDB (see the deviations).
    pub tdb_counters: [AtomicU64; TDB_NCOUNTERS],

    /// `tdb_cryptoid`: crypto session ID.
    pub tdb_cryptoid: Cell<u64>,

    /// \[I\] `tdb_spi`: SPI, network order.
    pub tdb_spi: Cell<u32>,
    /// `tdb_amxkeylen`: raw authentication key length.
    pub tdb_amxkeylen: Cell<u16>,
    /// `tdb_emxkeylen`: raw encryption key length.
    pub tdb_emxkeylen: Cell<u16>,
    /// `tdb_ivlen`: IV length.
    pub tdb_ivlen: Cell<u16>,
    /// \[I\] `tdb_sproto`: IPsec protocol.
    pub tdb_sproto: Cell<u8>,
    /// `tdb_wnd`: replay window.
    pub tdb_wnd: Cell<u8>,
    /// `tdb_satype`: SA type (RFC2367, PF_KEY).
    pub tdb_satype: Cell<u8>,
    /// \[I\] `tdb_iface_dir`: `sec(4)` iface direction.
    pub tdb_iface_dir: Cell<u8>,

    /// \[N\] `tdb_dst`: destination address.
    pub tdb_dst: Cell<SockaddrUnion>,
    /// \[N\] `tdb_src`: source address.
    pub tdb_src: Cell<SockaddrUnion>,

    /// `tdb_amxkey`: raw authentication key (`malloc(M_XDATA)`, `tdb_amxkeylen` bytes).
    pub tdb_amxkey: Cell<*mut u8>,
    /// `tdb_emxkey`: raw encryption key (`malloc(M_XDATA)`, `tdb_emxkeylen` bytes).
    pub tdb_emxkey: Cell<*mut u8>,

    /// \[m\] `tdb_rpl`: replay counter.
    pub tdb_rpl: Cell<u64>,
    /// \[m\] `tdb_seen`: anti-replay window.
    pub tdb_seen: [Cell<u32>; TDB_SEEN_WORDS],

    /// `tdb_iv`: used for HALF-IV ESP.
    pub tdb_iv: Cell<[u8; 4]>,

    /// `tdb_ids`: src/dst ID for this SA.
    pub tdb_ids: Cell<Option<&'static IpsecIds>>,
    /// `tdb_ids_swapped`: XXX.
    pub tdb_ids_swapped: Cell<i32>,

    /// `tdb_mtu`: MTU at this point in the chain.
    pub tdb_mtu: Cell<u32>,
    /// `tdb_mtutimeout`: when to ignore this entry.
    pub tdb_mtutimeout: Cell<u64>,

    /// `tdb_udpencap_port`: peer UDP port, network order.
    pub tdb_udpencap_port: Cell<u16>,

    /// `tdb_tag`: packet filter tag.
    pub tdb_tag: Cell<u16>,
    /// `tdb_tap`: alternate `enc(4)` interface.
    pub tdb_tap: Cell<u32>,
    /// \[I\] `tdb_iface`: `sec(4)` iface.
    pub tdb_iface: Cell<u32>,

    /// \[I\] `tdb_rdomain`: routing domain.
    pub tdb_rdomain: Cell<u32>,
    /// \[I\] `tdb_rdomain_post`: change domain.
    pub tdb_rdomain_post: Cell<u32>,

    /// `tdb_filter`: what traffic is acceptable.
    pub tdb_filter: Cell<SockaddrEncap>,
    /// `tdb_filtermask`: and the mask.
    pub tdb_filtermask: Cell<SockaddrEncap>,

    /// \[P\] `tdb_policy_head`.
    pub tdb_policy_head: TailqHead<TdbPolicyHead>,
    /// \[S\] `tdb_sync_entry`: pfsync tdb queue.
    pub tdb_sync_entry: TailqEntry<Tdb>,
    /// \[S\] `tdb_updates`: pfsync update counter.
    pub tdb_updates: Cell<u32>,
}

// SAFETY: the members change under the locks their docs name (or, unmarked, under the net
// lock, as in C); the counters and the reference count are atomic.
unsafe impl Sync for Tdb {}

impl Tdb {
    /// A zeroed TDB (`pool_get(PR_ZERO)`, or the C's `malloc(M_ZERO)` stand-in of
    /// `SADB_GETSPI`): no timeouts set, no links, a fresh reference count.
    pub fn new() -> Self {
        Self {
            tdb_hnext: Cell::new(None),
            tdb_dnext: Cell::new(None),
            tdb_snext: Cell::new(None),
            tdb_inext: Cell::new(None),
            tdb_onext: Cell::new(None),
            tdb_walk: SimpleqEntry::new(),
            tdb_refcnt: Refcnt::new(),
            tdb_mtx: Mutex::new(IPL_SOFTNET),
            tdb_xform: Cell::new(None),
            tdb_encalgxform: Cell::new(None),
            tdb_authalgxform: Cell::new(None),
            tdb_compalgxform: Cell::new(None),
            tdb_flags: Cell::new(0),
            tdb_timer_tmo: Timeout::zeroed(),
            tdb_first_tmo: Timeout::zeroed(),
            tdb_stimer_tmo: Timeout::zeroed(),
            tdb_sfirst_tmo: Timeout::zeroed(),
            tdb_seq: Cell::new(0),
            tdb_exp_allocations: Cell::new(0),
            tdb_soft_allocations: Cell::new(0),
            tdb_cur_allocations: Cell::new(0),
            tdb_exp_bytes: Cell::new(0),
            tdb_soft_bytes: Cell::new(0),
            tdb_cur_bytes: Cell::new(0),
            tdb_exp_timeout: Cell::new(0),
            tdb_soft_timeout: Cell::new(0),
            tdb_established: Cell::new(0),
            tdb_first_use: Cell::new(0),
            tdb_soft_first_use: Cell::new(0),
            tdb_exp_first_use: Cell::new(0),
            tdb_last_used: Cell::new(0),
            tdb_last_marked: Cell::new(0),
            tdb_counters: [const { AtomicU64::new(0) }; TDB_NCOUNTERS],
            tdb_cryptoid: Cell::new(0),
            tdb_spi: Cell::new(0),
            tdb_amxkeylen: Cell::new(0),
            tdb_emxkeylen: Cell::new(0),
            tdb_ivlen: Cell::new(0),
            tdb_sproto: Cell::new(0),
            tdb_wnd: Cell::new(0),
            tdb_satype: Cell::new(0),
            tdb_iface_dir: Cell::new(0),
            tdb_dst: Cell::new(SockaddrUnion::new()),
            tdb_src: Cell::new(SockaddrUnion::new()),
            tdb_amxkey: Cell::new(ptr::null_mut()),
            tdb_emxkey: Cell::new(ptr::null_mut()),
            tdb_rpl: Cell::new(0),
            tdb_seen: [const { Cell::new(0) }; TDB_SEEN_WORDS],
            tdb_iv: Cell::new([0; 4]),
            tdb_ids: Cell::new(None),
            tdb_ids_swapped: Cell::new(0),
            tdb_mtu: Cell::new(0),
            tdb_mtutimeout: Cell::new(0),
            tdb_udpencap_port: Cell::new(0),
            tdb_tag: Cell::new(0),
            tdb_tap: Cell::new(0),
            tdb_iface: Cell::new(0),
            tdb_rdomain: Cell::new(0),
            tdb_rdomain_post: Cell::new(0),
            tdb_filter: Cell::new(SockaddrEncap::new()),
            tdb_filtermask: Cell::new(SockaddrEncap::new()),
            tdb_policy_head: TailqHead::new(),
            tdb_sync_entry: TailqEntry::new(),
            tdb_updates: Cell::new(0),
        }
    }

    /// Sets the `TDBF_*` bits `f` in `tdb_flags`.
    pub fn set_flags(&self, f: u32) {
        self.tdb_flags.set(self.tdb_flags.get() | f);
    }

    /// Clears the `TDBF_*` bits `f` in `tdb_flags`.
    pub fn clr_flags(&self, f: u32) {
        self.tdb_flags.set(self.tdb_flags.get() & !f);
    }

    /// Whether any of the `TDBF_*` bits `f` is set.
    pub fn has_flags(&self, f: u32) -> bool {
        self.tdb_flags.get() & f != 0
    }

    /// The raw authentication key (`tdb_amxkey`, `tdb_amxkeylen` bytes), empty when none.
    ///
    /// # Safety
    ///
    /// The TDB's transform is not zeroized (`xf_zeroize` frees the key) while the slice
    /// lives: the caller holds a reference to the TDB and does not drop it meanwhile.
    pub unsafe fn tdb_amxkey(&self) -> &[u8] {
        let p = self.tdb_amxkey.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: a non-null key is a `malloc` of `tdb_amxkeylen` bytes the transform's
        // init wrote; it lives until `xf_zeroize` (the caller's contract).
        unsafe { slice::from_raw_parts(p, usize::from(self.tdb_amxkeylen.get())) }
    }

    /// The raw encryption key (`tdb_emxkey`, `tdb_emxkeylen` bytes), empty when none.
    ///
    /// # Safety
    ///
    /// As for [`Tdb::tdb_amxkey`].
    pub unsafe fn tdb_emxkey(&self) -> &[u8] {
        let p = self.tdb_emxkey.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: as in `tdb_amxkey`.
        unsafe { slice::from_raw_parts(p, usize::from(self.tdb_emxkeylen.get())) }
    }
}

impl Default for Tdb {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(, tdb)` through `tdb_walk`: `tdb_walk`'s private list.
    pub TdbWalkList: Tdb, tdb_walk => SimpleqEntry<Tdb>
);

/// `tdbstat_inc(tdb, c)`.
pub fn tdbstat_inc(tdb: &Tdb, c: TdbCounters) {
    tdb.tdb_counters[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `tdbstat_add(tdb, c, v)`.
pub fn tdbstat_add(tdb: &Tdb, c: TdbCounters, v: u64) {
    tdb.tdb_counters[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `tdbstat_pkt(tdb, pc, bc, bytes)`: one packet of `bytes` bytes.
pub fn tdbstat_pkt(tdb: &Tdb, pc: TdbCounters, bc: TdbCounters, bytes: u64) {
    tdb.tdb_counters[pc as usize].fetch_add(1, Ordering::Relaxed);
    tdb.tdb_counters[bc as usize].fetch_add(bytes, Ordering::Relaxed);
}

/// `struct tdb_ident`: what an `IPSEC_IN_DONE`/`IPSEC_OUT_DONE` packet tag carries: the SA the
/// packet went through.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TdbIdent {
    /// `spi`.
    pub spi: u32,
    /// `dst`.
    pub dst: SockaddrUnion,
    /// `proto`.
    pub proto: u8,
    /// The C compiler's padding after `proto`, explicit so the tag is all initialised.
    pub _pad: [u8; 3],
    /// `rdomain`.
    pub rdomain: u32,
}

impl TdbIdent {
    /// Reads the `struct tdb_ident` of a packet tag's data (`(struct tdb_ident *)(mtag + 1)`).
    ///
    /// # Safety
    ///
    /// `data` points to a tag's data of at least `size_of::<TdbIdent>()` bytes that a
    /// `tdb_ident` was written into.
    pub unsafe fn read(data: *const u8) -> Self {
        // SAFETY: the caller's contract; the structure is integers.
        unsafe { ptr::read_unaligned(data.cast::<TdbIdent>()) }
    }

    /// Writes `self` as a packet tag's data.
    ///
    /// # Safety
    ///
    /// `data` points to a tag's data of at least `size_of::<TdbIdent>()` writable bytes.
    pub unsafe fn write(&self, data: *mut u8) {
        // SAFETY: the caller's contract.
        unsafe { ptr::write_unaligned(data.cast::<TdbIdent>(), *self) };
    }

    /// The identity of `tdb`, as `ipsec_common_input_cb` and `ipsp_process_done` record it.
    pub fn of(tdb: &Tdb) -> Self {
        Self {
            spi: tdb.tdb_spi.get(),
            dst: tdb.tdb_dst.get(),
            proto: tdb.tdb_sproto.get(),
            _pad: [0; 3],
            rdomain: tdb.tdb_rdomain.get(),
        }
    }
}

/// `struct tdb_crypto`: the state an asynchronous crypto callback needed (unused since the
/// framework became synchronous, kept as the header has it).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TdbCrypto {
    /// `tc_dst`.
    pub tc_dst: SockaddrUnion,
    /// `tc_rpl`.
    pub tc_rpl: u64,
    /// `tc_spi`.
    pub tc_spi: u32,
    /// `tc_protoff`.
    pub tc_protoff: i32,
    /// `tc_skip`.
    pub tc_skip: i32,
    /// `tc_rdomain`.
    pub tc_rdomain: u32,
    /// `tc_proto`.
    pub tc_proto: u8,
}

/// `struct ipsecinit`: the algorithms and keys a transform's `xf_init` sets a TDB up with.
#[derive(Clone, Copy, Debug, Default)]
pub struct IpsecInit<'a> {
    /// `ii_enckey` (empty for NULL).
    pub ii_enckey: &'a [u8],
    /// `ii_authkey` (empty for NULL).
    pub ii_authkey: &'a [u8],
    /// `ii_enckeylen`.
    pub ii_enckeylen: u16,
    /// `ii_authkeylen`.
    pub ii_authkeylen: u16,
    /// `ii_encalg`.
    pub ii_encalg: u8,
    /// `ii_authalg`.
    pub ii_authalg: u8,
    /// `ii_compalg`.
    pub ii_compalg: u8,
}

// xform IDs

/// `XF_IP4`: IP inside IP.
pub const XF_IP4: u16 = 1;
/// `XF_AH`: AH.
pub const XF_AH: u16 = 2;
/// `XF_ESP`: ESP.
pub const XF_ESP: u16 = 3;
/// `XF_TCPSIGNATURE`: TCP MD5 Signature option, RFC 2358.
pub const XF_TCPSIGNATURE: u16 = 5;
/// `XF_IPCOMP`: IPCOMP.
pub const XF_IPCOMP: u16 = 6;

// xform attributes

/// `XFT_AUTH`.
pub const XFT_AUTH: u16 = 0x0001;
/// `XFT_CONF`.
pub const XFT_CONF: u16 = 0x0100;
/// `XFT_COMP`.
pub const XFT_COMP: u16 = 0x1000;

/// `IPSEC_ZEROES_SIZE`: larger than an IP6 extension hdr.
pub const IPSEC_ZEROES_SIZE: usize = 256;

/// `xf_input`: `(mp, tdb, skip, protoff, ns)`; the next protocol or `IPPROTO_DONE`.
pub type XfInputFn =
    fn(&mut Option<&'static Mbuf>, &'static Tdb, i32, i32, Option<&Netstack>) -> i32;
/// `xf_output`: `(m, tdb, skip, protoff)`; consumes the packet.
pub type XfOutputFn = fn(&'static Mbuf, &'static Tdb, i32, i32) -> Result<(), Errno>;
/// `xf_init`: `(tdb, xsp, ii)`.
pub type XfInitFn = fn(&Tdb, &'static Xformsw, &mut IpsecInit<'_>) -> Result<(), Errno>;

/// `struct xformsw`: an IPsec transform.
pub struct Xformsw {
    /// `xf_type`: unique ID of xform.
    pub xf_type: u16,
    /// `xf_flags`: flags (`XFT_*`).
    pub xf_flags: u16,
    /// `xf_name`: human-readable name.
    pub xf_name: &'static str,
    /// `xf_attach`: called at config time.
    pub xf_attach: fn() -> i32,
    /// `xf_init`.
    pub xf_init: XfInitFn,
    /// `xf_zeroize`: termination.
    pub xf_zeroize: fn(&Tdb) -> Result<(), Errno>,
    /// `xf_input`.
    pub xf_input: XfInputFn,
    /// `xf_output` (NULL for `XF_IP4`).
    pub xf_output: Option<XfOutputFn>,
}

/// `ipsec_in_use`: the number of flows in the SPD (no lookups when 0).
pub static IPSEC_IN_USE: AtomicI32 = AtomicI32::new(0);
/// `ipsec_last_added`: uptime of the last `puttdb`.
pub static IPSEC_LAST_ADDED: AtomicU64 = AtomicU64::new(0);
/// `ipsec_ids_idle`: keep free ids for 100s.
pub static IPSEC_IDS_IDLE: AtomicI32 = AtomicI32::new(100);

/// `tdb_pool`.
pub static TDB_POOL: Pool = Pool::new();

// Names for IPsec sysctl objects

/// `IPSEC_ENCDEBUG` (`IPCTL_ENCDEBUG`, 12).
pub const IPSEC_ENCDEBUG: i32 = 12;
/// `IPSEC_STATS` (`IPCTL_IPSEC_STATS`, 13).
pub const IPSEC_STATS: i32 = 13;
/// `IPSEC_EXPIRE_ACQUIRE` (14).
pub const IPSEC_EXPIRE_ACQUIRE: i32 = 14;
/// `IPSEC_EMBRYONIC_SA_TIMEOUT` (15).
pub const IPSEC_EMBRYONIC_SA_TIMEOUT: i32 = 15;
/// `IPSEC_REQUIRE_PFS` (16).
pub const IPSEC_REQUIRE_PFS: i32 = 16;
/// `IPSEC_SOFT_ALLOCATIONS` (17).
pub const IPSEC_SOFT_ALLOCATIONS: i32 = 17;
/// `IPSEC_ALLOCATIONS` (18).
pub const IPSEC_ALLOCATIONS: i32 = 18;
/// `IPSEC_SOFT_BYTES` (19).
pub const IPSEC_SOFT_BYTES: i32 = 19;
/// `IPSEC_BYTES` (20).
pub const IPSEC_BYTES: i32 = 20;
/// `IPSEC_TIMEOUT` (21).
pub const IPSEC_TIMEOUT: i32 = 21;
/// `IPSEC_SOFT_TIMEOUT` (22).
pub const IPSEC_SOFT_TIMEOUT: i32 = 22;
/// `IPSEC_SOFT_FIRSTUSE` (23).
pub const IPSEC_SOFT_FIRSTUSE: i32 = 23;
/// `IPSEC_FIRSTUSE` (24).
pub const IPSEC_FIRSTUSE: i32 = 24;
/// `IPSEC_MAXID`.
pub const IPSEC_MAXID: i32 = 25;

/// `IPSEC_ENC_AES`.
pub const IPSEC_ENC_AES: i32 = 0;
/// `IPSEC_ENC_AESCTR`.
pub const IPSEC_ENC_AESCTR: i32 = 1;
/// `IPSEC_ENC_3DES`.
pub const IPSEC_ENC_3DES: i32 = 2;
/// `IPSEC_ENC_BLOWFISH`.
pub const IPSEC_ENC_BLOWFISH: i32 = 3;
/// `IPSEC_ENC_CAST128`.
pub const IPSEC_ENC_CAST128: i32 = 4;

/// `IPSEC_AUTH_HMAC_SHA1`.
pub const IPSEC_AUTH_HMAC_SHA1: i32 = 0;
/// `IPSEC_AUTH_HMAC_RIPEMD160`.
pub const IPSEC_AUTH_HMAC_RIPEMD160: i32 = 1;
/// `IPSEC_AUTH_MD5`.
pub const IPSEC_AUTH_MD5: i32 = 2;
/// `IPSEC_AUTH_SHA2_256`.
pub const IPSEC_AUTH_SHA2_256: i32 = 3;
/// `IPSEC_AUTH_SHA2_384`.
pub const IPSEC_AUTH_SHA2_384: i32 = 4;
/// `IPSEC_AUTH_SHA2_512`.
pub const IPSEC_AUTH_SHA2_512: i32 = 5;

/// `IPSEC_COMP_DEFLATE`.
pub const IPSEC_COMP_DEFLATE: i32 = 0;

// Packet processing

/// `IPSP_DF_INHERIT`.
pub const IPSP_DF_INHERIT: i32 = -1;
/// `IPSP_DF_OFF`.
pub const IPSP_DF_OFF: i32 = 0;
/// `IPSP_DF_ON`.
pub const IPSP_DF_ON: i32 = 1;

/// `ipsec_flows_mtx`: the identity trees and their garbage list (\[F\]).
pub static IPSEC_FLOWS_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// The identity trees and the flow counter, under `ipsec_flows_mtx`.
struct IpsecFlows {
    /// \[F\] `ipsec_ids_next_flow`: may not be zero.
    ipsec_ids_next_flow: u32,
    /// \[F\] `ipsec_ids_tree`.
    ipsec_ids_tree: RbtHead<IpsecIdsTree>,
    /// \[F\] `ipsec_ids_flows`.
    ipsec_ids_flows: RbtHead<IpsecIdsFlows>,
    /// \[F\] `ipsp_ids_gc_list`.
    ipsp_ids_gc_list: ListHead<IpsecIdsGcList>,
}

// SAFETY: reached only under `ipsec_flows_mtx`.
unsafe impl Send for IpsecFlows {}

static IPSEC_FLOWS: StaticCell<IpsecFlows> = StaticCell::new(IpsecFlows {
    ipsec_ids_next_flow: 1,
    ipsec_ids_tree: RbtHead::new(),
    ipsec_ids_flows: RbtHead::new(),
    ipsp_ids_gc_list: ListHead::new(),
});

/// The identity state; `ipsec_flows_mtx` is held by the caller.
fn flows() -> &'static mut IpsecFlows {
    mutex_assert_locked(&IPSEC_FLOWS_MTX, "flows");
    // SAFETY: `ipsec_flows_mtx` is held, so this is the only access, and no caller keeps the
    // reference past `mtx_leave`.
    unsafe { IPSEC_FLOWS.get_mut() }
}

/// `ipsec_policy_head`: every policy (`TAILQ_HEAD_INITIALIZER`), under the net lock.
pub static IPSEC_POLICY_HEAD: PolicyListHead = PolicyListHead(TailqHead::new());

/// The `ipsec_policy_head` list head.
pub struct PolicyListHead(pub TailqHead<IpsecPolicyHead>);

// SAFETY: changed only under the exclusive net lock, as in C.
unsafe impl Sync for PolicyListHead {}

/// `ipsp_ids_gc_timeout`.
static IPSP_IDS_GC_TIMEOUT: Timeout = Timeout::new_flags(
    ipsp_ids_gc,
    ptr::null_mut(),
    KCLOCK_NONE,
    TIMEOUT_PROC | TIMEOUT_MPSAFE,
);

/// `xformsw[]`: the encapsulation transforms.
pub static XFORMSW: [Xformsw; 5] = [
    Xformsw {
        xf_type: XF_IP4,
        xf_flags: 0,
        xf_name: "IPv4 Simple Encapsulation",
        xf_attach: ipe4_attach,
        xf_init: ipe4_init,
        xf_zeroize: ipe4_zeroize,
        xf_input: ipe4_input,
        xf_output: None,
    },
    Xformsw {
        xf_type: XF_AH,
        xf_flags: XFT_AUTH,
        xf_name: "IPsec AH",
        xf_attach: ah_attach,
        xf_init: ah_init,
        xf_zeroize: ah_zeroize,
        xf_input: ah_input,
        xf_output: Some(ah_output),
    },
    Xformsw {
        xf_type: XF_ESP,
        xf_flags: XFT_CONF | XFT_AUTH,
        xf_name: "IPsec ESP",
        xf_attach: esp_attach,
        xf_init: esp_init,
        xf_zeroize: esp_zeroize,
        xf_input: esp_input,
        xf_output: Some(esp_output),
    },
    Xformsw {
        xf_type: XF_IPCOMP,
        xf_flags: XFT_COMP,
        xf_name: "IPcomp",
        xf_attach: ipcomp_attach,
        xf_init: ipcomp_init,
        xf_zeroize: ipcomp_zeroize,
        xf_input: ipcomp_input,
        xf_output: Some(ipcomp_output),
    },
    // TCP_SIGNATURE
    Xformsw {
        xf_type: XF_TCPSIGNATURE,
        xf_flags: XFT_AUTH,
        xf_name: "TCP MD5 Signature Option, RFC 2385",
        xf_attach: crate::netinet::tcp_subr::tcp_signature_tdb_attach,
        xf_init: crate::netinet::tcp_subr::tcp_signature_tdb_init,
        xf_zeroize: crate::netinet::tcp_subr::tcp_signature_tdb_zeroize,
        xf_input: crate::netinet::tcp_subr::tcp_signature_tdb_input,
        xf_output: Some(crate::netinet::tcp_subr::tcp_signature_tdb_output),
    },
];

/// `TDB_HASHSIZE_INIT`.
const TDB_HASHSIZE_INIT: u32 = 32;

/// `tdb_sadb_mtx`: the SA database (\[D\]).
pub static TDB_SADB_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// The SA database: `tdbkey`, `tdbh`, `tdbdst`, `tdbsrc`, `tdb_hashmask`, `tdb_count`, all
/// \[D\].
pub struct TdbTables {
    /// `tdbkey`.
    tdbkey: SiphashKey,
    /// `tdbh`: by dst/spi/sproto.
    tdbh: Vec<Option<&'static Tdb>>,
    /// `tdbdst`: by dst/sproto.
    tdbdst: Vec<Option<&'static Tdb>>,
    /// `tdbsrc`: by src/sproto.
    tdbsrc: Vec<Option<&'static Tdb>>,
    /// `tdb_hashmask`.
    tdb_hashmask: u32,
    /// `tdb_count`.
    tdb_count: i32,
}

static TDB_TABLES: StaticCell<TdbTables> = StaticCell::new(TdbTables {
    tdbkey: SiphashKey { k0: 0, k1: 0 },
    tdbh: Vec::new(),
    tdbdst: Vec::new(),
    tdbsrc: Vec::new(),
    tdb_hashmask: TDB_HASHSIZE_INIT - 1,
    tdb_count: 0,
});

/// The SA database; `tdb_sadb_mtx` is held by the caller.
fn tables() -> &'static mut TdbTables {
    mutex_assert_locked(&TDB_SADB_MTX, "tables");
    // SAFETY: `tdb_sadb_mtx` is held, so this is the only access, and no caller keeps the
    // reference past `mtx_leave` (the functions take it once and pass it down).
    unsafe { TDB_TABLES.get_mut() }
}

/// A fresh `SIPHASH_KEY` from `arc4random_buf`.
fn random_siphash_key() -> SiphashKey {
    let mut b = [0u8; 16];
    arc4random_buf(&mut b);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&b[..8]);
    k1.copy_from_slice(&b[8..]);
    SiphashKey {
        k0: u64::from_ne_bytes(k0),
        k1: u64::from_ne_bytes(k1),
    }
}

/// `ipsp_init`: the TDB pool and the empty hash tables.
pub fn ipsp_init() {
    pool_init(&TDB_POOL, size_of::<Tdb>(), 0, IPL_SOFTNET, 0, "tdb", None);

    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    t.tdbkey = random_siphash_key();
    let n = (t.tdb_hashmask + 1) as usize;
    t.tdbh = alloc::vec![None; n];
    t.tdbdst = alloc::vec![None; n];
    t.tdbsrc = alloc::vec![None; n];
    mtx_leave(&TDB_SADB_MTX);
}

impl TdbTables {
    /// `tdb_hash`: our hashing function needs to stir things with a non-zero random
    /// multiplier so we cannot be DoS-attacked via choosing of the data to hash.
    fn tdb_hash(&self, spi: u32, dst: &SockaddrUnion, proto: u8) -> u32 {
        let mut ctx = SiphashCtx::default();

        SipHash24_Init(&mut ctx, &self.tdbkey);
        SipHash24_Update(&mut ctx, &spi.to_ne_bytes());
        SipHash24_Update(&mut ctx, &[proto]);
        SipHash24_Update(&mut ctx, dst.sa_bytes());

        (SipHash24_End(&mut ctx) as u32) & self.tdb_hashmask
    }
}

/// `reserve_spi`: reserve an SPI in `[sspi, tspi]`; the SA is not valid yet though. The C
/// returns 0 with `*errval` set on failure.
pub fn reserve_spi(
    rdomain: u32,
    sspi: u32,
    tspi: u32,
    src: &SockaddrUnion,
    dst: &SockaddrUnion,
    sproto: u8,
) -> Result<u32, Errno> {
    let keep_invalid_local = IPSEC_KEEP_INVALID.load(Ordering::Relaxed);
    let (mut sspi, mut tspi) = (sspi, tspi);

    // Don't accept ranges only encompassing reserved SPIs.
    if i32::from(sproto) != IPPROTO_IPCOMP && (tspi < sspi || tspi <= SPI_RESERVED_MAX) {
        return Err(Errno::EINVAL);
    }
    if i32::from(sproto) == IPPROTO_IPCOMP
        && (tspi < sspi || tspi <= CPI_RESERVED_MAX || tspi >= CPI_PRIVATE_MIN)
    {
        return Err(Errno::EINVAL);
    }

    // Limit the range to not include reserved areas.
    if sspi <= SPI_RESERVED_MAX {
        sspi = SPI_RESERVED_MAX + 1;
    }

    // For IPCOMP the CPI is only 16 bits long, what a good idea....

    if i32::from(sproto) == IPPROTO_IPCOMP {
        if sspi >= 0x10000 {
            sspi = 0xffff;
        }
        if tspi >= 0x10000 {
            tspi = 0xffff;
        }
        if sspi > tspi {
            core::mem::swap(&mut sspi, &mut tspi);
        }
    }

    let mut nums = if sspi == tspi {
        1 // Asking for a specific SPI.
    } else {
        100 // Arbitrarily chosen
    };

    // allocate ahead of time to avoid potential sleeping race in loop
    let tdbp = tdb_alloc(rdomain);

    while nums > 0 {
        nums -= 1;
        let mut spi = if sspi == tspi {
            tspi // Specific SPI asked.
        } else {
            sspi + arc4random_uniform(tspi - sspi) // Range specified
        };

        // Don't allocate reserved SPIs.
        if (SPI_RESERVED_MIN..=SPI_RESERVED_MAX).contains(&spi) {
            continue;
        }
        spi = htonl(spi);

        // Check whether we're using this SPI already.
        if let Some(exists) = gettdb(rdomain, spi, dst, sproto) {
            tdb_unref(Some(exists));
            continue;
        }

        tdbp.tdb_spi.set(spi);
        let mut d = SockaddrUnion::new();
        d.as_bytes_mut()[..dst.sa_bytes().len()].copy_from_slice(dst.sa_bytes());
        tdbp.tdb_dst.set(d);
        let mut s = SockaddrUnion::new();
        s.as_bytes_mut()[..src.sa_bytes().len()].copy_from_slice(src.sa_bytes());
        tdbp.tdb_src.set(s);
        tdbp.tdb_sproto.set(sproto);
        tdbp.set_flags(TDBF_INVALID); // Mark SA invalid for now.
        tdbp.tdb_satype.set(SADB_SATYPE_UNSPEC);
        puttdb(tdbp);

        // Setup a "silent" expiration (since TDBF_INVALID's set).
        if keep_invalid_local > 0 {
            mtx_enter(&tdbp.tdb_mtx);
            tdbp.set_flags(TDBF_TIMER);
            tdbp.tdb_exp_timeout.set(keep_invalid_local as u64);
            if timeout_add_sec(&tdbp.tdb_timer_tmo, keep_invalid_local) {
                tdb_ref(Some(tdbp));
            }
            mtx_leave(&tdbp.tdb_mtx);
        }

        return Ok(spi);
    }

    tdb_unref(Some(tdbp));
    Err(Errno::EEXIST)
}

/// `gettdb_dir`: an IPSP SAID is really the concatenation of the SPI found in the packet, the
/// destination address of the packet and the IPsec protocol. When we receive an IPSP packet,
/// we need to look up its tunnel descriptor block, based on the SPI in the packet and the
/// destination address (which is really one of our addresses if we received the packet!).
/// `reverse` matches `tdb_rdomain_post` instead of `tdb_rdomain`. Returns a reference.
pub fn gettdb_dir(
    rdomain: u32,
    spi: u32,
    dst: &SockaddrUnion,
    proto: u8,
    reverse: bool,
) -> Option<&'static Tdb> {
    net_assert_locked("gettdb_dir");

    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    let hashval = t.tdb_hash(spi, dst, proto);

    let mut tdbp = t.tdbh[hashval as usize];
    while let Some(tp) = tdbp {
        if tp.tdb_spi.get() == spi
            && tp.tdb_sproto.get() == proto
            && ((!reverse && tp.tdb_rdomain.get() == rdomain)
                || (reverse && tp.tdb_rdomain_post.get() == rdomain))
            && sa_prefix_eq(&tp.tdb_dst.get(), dst)
        {
            break;
        }
        tdbp = tp.tdb_hnext.get();
    }

    let r = tdb_ref(tdbp);
    mtx_leave(&TDB_SADB_MTX);
    r
}

/// `memcmp(a, b, b->sa.sa_len) == 0`: the first `sa_len` bytes of `b` match `a`'s.
fn sa_prefix_eq(a: &SockaddrUnion, b: &SockaddrUnion) -> bool {
    let n = b.sa_bytes().len();
    a.as_bytes()[..n] == b.as_bytes()[..n]
}

/// `gettdb(a, b, c, d)`.
pub fn gettdb(rdomain: u32, spi: u32, dst: &SockaddrUnion, proto: u8) -> Option<&'static Tdb> {
    gettdb_dir(rdomain, spi, dst, proto, false)
}

/// `gettdb_rev(a, b, c, d)`.
pub fn gettdb_rev(rdomain: u32, spi: u32, dst: &SockaddrUnion, proto: u8) -> Option<&'static Tdb> {
    gettdb_dir(rdomain, spi, dst, proto, true)
}

/// `gettdbbysrcdst_dir`: same as `gettdb()` but compare SRC as well, so we use the `tdbsrc[]`
/// hash table. Setting `spi` to 0 matches all SPIs.
pub fn gettdbbysrcdst_dir(
    rdomain: u32,
    spi: u32,
    src: &SockaddrUnion,
    dst: &SockaddrUnion,
    proto: u8,
    reverse: bool,
) -> Option<&'static Tdb> {
    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    let hashval = t.tdb_hash(0, src, proto);

    let matches = |tp: &Tdb| {
        tp.tdb_sproto.get() == proto
            && (spi == 0 || tp.tdb_spi.get() == spi)
            && ((!reverse && tp.tdb_rdomain.get() == rdomain)
                || (reverse && tp.tdb_rdomain_post.get() == rdomain))
            && !tp.has_flags(TDBF_INVALID)
            && (tp.tdb_dst.get().sa_family() == crate::sys::socket::AF_UNSPEC
                || sa_prefix_eq(&tp.tdb_dst.get(), dst))
    };

    let mut tdbp = t.tdbsrc[hashval as usize];
    while let Some(tp) = tdbp {
        if matches(tp) && sa_prefix_eq(&tp.tdb_src.get(), src) {
            break;
        }
        tdbp = tp.tdb_snext.get();
    }
    if let Some(tp) = tdbp {
        let r = tdb_ref(Some(tp));
        mtx_leave(&TDB_SADB_MTX);
        return r;
    }

    let mut su_null = SockaddrUnion::new();
    su_null.set_sa_len(size_of::<Sockaddr>() as u8);
    let hashval = t.tdb_hash(0, &su_null, proto);

    let mut tdbp = t.tdbsrc[hashval as usize];
    while let Some(tp) = tdbp {
        if matches(tp) && tp.tdb_src.get().sa_family() == crate::sys::socket::AF_UNSPEC {
            break;
        }
        tdbp = tp.tdb_snext.get();
    }
    let r = tdb_ref(tdbp);
    mtx_leave(&TDB_SADB_MTX);
    r
}

/// `gettdbbysrcdst(a, b, c, d, e)`.
pub fn gettdbbysrcdst(
    rdomain: u32,
    spi: u32,
    src: &SockaddrUnion,
    dst: &SockaddrUnion,
    proto: u8,
) -> Option<&'static Tdb> {
    gettdbbysrcdst_dir(rdomain, spi, src, dst, proto, false)
}

/// `gettdbbysrcdst_rev(a, b, c, d, e)`.
pub fn gettdbbysrcdst_rev(
    rdomain: u32,
    spi: u32,
    src: &SockaddrUnion,
    dst: &SockaddrUnion,
    proto: u8,
) -> Option<&'static Tdb> {
    gettdbbysrcdst_dir(rdomain, spi, src, dst, proto, true)
}

/// `ipsp_aux_match`: check that IDs match. Return true if so. The `t*` range of arguments
/// contains information from TDBs; the `p*` range of arguments contains information from
/// policies or already established TDBs.
pub fn ipsp_aux_match(
    tdb: &Tdb,
    ids: Option<&IpsecIds>,
    pfilter: Option<&SockaddrEncap>,
    pfiltermask: Option<&SockaddrEncap>,
) -> bool {
    if let Some(ids) = ids {
        match tdb.tdb_ids.get() {
            Some(t) if ipsp_ids_match(t, ids) => {}
            _ => return false,
        }
    }

    // Check for filter matches.
    if let (Some(pfilter), Some(pfiltermask)) = (pfilter, pfiltermask)
        && tdb.tdb_filter.get().sen_type() != 0
    {
        // XXX We should really be doing a subnet-check (see whether the TDB-associated
        // filter is a subset of the policy's. For now, an exact match will solve most
        // problems (all this will do is make every policy get its own SAs).
        if tdb.tdb_filter.get() != *pfilter || tdb.tdb_filtermask.get() != *pfiltermask {
            return false;
        }
    }

    true
}

/// `gettdbbydst`: get an SA given the remote address, the security protocol type, and the
/// desired IDs.
pub fn gettdbbydst(
    rdomain: u32,
    dst: &SockaddrUnion,
    sproto: u8,
    ids: Option<&IpsecIds>,
    filter: Option<&SockaddrEncap>,
    filtermask: Option<&SockaddrEncap>,
) -> Option<&'static Tdb> {
    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    let hashval = t.tdb_hash(0, dst, sproto);

    let mut tdbp = t.tdbdst[hashval as usize];
    while let Some(tp) = tdbp {
        if tp.tdb_sproto.get() == sproto
            && tp.tdb_rdomain.get() == rdomain
            && !tp.has_flags(TDBF_INVALID)
            && sa_prefix_eq(&tp.tdb_dst.get(), dst)
        {
            // Check whether IDs match
            if ipsp_aux_match(tp, ids, filter, filtermask) {
                break;
            }
        }
        tdbp = tp.tdb_dnext.get();
    }

    let r = tdb_ref(tdbp);
    mtx_leave(&TDB_SADB_MTX);
    r
}

/// `gettdbbysrc`: get an SA given the source address, the security protocol type, and the
/// desired IDs.
pub fn gettdbbysrc(
    rdomain: u32,
    src: &SockaddrUnion,
    sproto: u8,
    ids: Option<&IpsecIds>,
    filter: Option<&SockaddrEncap>,
    filtermask: Option<&SockaddrEncap>,
) -> Option<&'static Tdb> {
    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    let hashval = t.tdb_hash(0, src, sproto);

    let mut tdbp = t.tdbsrc[hashval as usize];
    while let Some(tp) = tdbp {
        if tp.tdb_sproto.get() == sproto
            && tp.tdb_rdomain.get() == rdomain
            && !tp.has_flags(TDBF_INVALID)
            && sa_prefix_eq(&tp.tdb_src.get(), src)
        {
            // Check whether IDs match
            if ipsp_aux_match(tp, ids, filter, filtermask) {
                break;
            }
        }
        tdbp = tp.tdb_snext.get();
    }
    let r = tdb_ref(tdbp);
    mtx_leave(&TDB_SADB_MTX);
    r
}

/// `NBUCKETS` of `tdb_hashstats`.
const NBUCKETS: usize = 16;

/// `tdb_hashstats` (`DDB`): the chain lengths of `tdbh`.
pub fn tdb_hashstats() {
    let mut buckets = [0i32; NBUCKETS];

    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    if t.tdbh.is_empty() {
        mtx_leave(&TDB_SADB_MTX);
        crate::db_printf!("no tdb hash table\n");
        return;
    }

    for head in &t.tdbh {
        let mut cnt = 0;
        let mut tdbp = *head;
        while cnt < NBUCKETS - 1 {
            let Some(tp) = tdbp else { break };
            cnt += 1;
            tdbp = tp.tdb_hnext.get();
        }
        buckets[cnt] += 1;
    }
    mtx_leave(&TDB_SADB_MTX);

    crate::db_printf!("tdb cnt\t\tbucket cnt\n");
    for (i, &b) in buckets.iter().enumerate() {
        if b > 0 {
            crate::db_printf!(
                "{}{}\t\t{}\n",
                i,
                if i == NBUCKETS - 1 { "+" } else { "" },
                b
            );
        }
    }
}

/// A TDB link as `%p` prints it.
fn tdbptr(t: Option<&Tdb>) -> *const Tdb {
    t.map_or(ptr::null(), ptr::from_ref)
}

/// `tdb_printit` (`DDB`): prints a TDB, all of it with `full`, one line otherwise.
pub fn tdb_printit(tdb: &Tdb, full: bool, pr: PrFn) {
    macro_rules! dump {
        ($name:literal, $fmt:literal, $v:expr) => {
            pr(format_args!(concat!("{:>18}: ", $fmt, "\n"), $name, $v))
        };
    }

    if full {
        pr(format_args!("tdb at {:p}\n", tdb));
        dump!("hnext", "{:p}", tdbptr(tdb.tdb_hnext.get()));
        dump!("dnext", "{:p}", tdbptr(tdb.tdb_dnext.get()));
        dump!("snext", "{:p}", tdbptr(tdb.tdb_snext.get()));
        dump!("inext", "{:p}", tdbptr(tdb.tdb_inext.get()));
        dump!("onext", "{:p}", tdbptr(tdb.tdb_onext.get()));
        dump!(
            "xform",
            "{:p}",
            tdb.tdb_xform.get().map_or(ptr::null(), ptr::from_ref)
        );
        dump!(
            "refcnt",
            "{}",
            tdb.tdb_refcnt.r_refs.load(Ordering::Relaxed)
        );
        dump!(
            "encalgxform",
            "{:p}",
            tdb.tdb_encalgxform.get().map_or(ptr::null(), ptr::from_ref)
        );
        dump!(
            "authalgxform",
            "{:p}",
            tdb.tdb_authalgxform
                .get()
                .map_or(ptr::null(), ptr::from_ref)
        );
        dump!(
            "compalgxform",
            "{:p}",
            tdb.tdb_compalgxform
                .get()
                .map_or(ptr::null(), ptr::from_ref)
        );
        dump!(
            "flags",
            "{}",
            Bitmask(u64::from(tdb.tdb_flags.get()), TDBF_BITS)
        );
        // tdb_XXX_tmo
        dump!("seq", "{}", tdb.tdb_seq.get());
        dump!("exp_allocations", "{}", tdb.tdb_exp_allocations.get());
        dump!("soft_allocations", "{}", tdb.tdb_soft_allocations.get());
        dump!("cur_allocations", "{}", tdb.tdb_cur_allocations.get());
        dump!("exp_bytes", "{}", tdb.tdb_exp_bytes.get());
        dump!("soft_bytes", "{}", tdb.tdb_soft_bytes.get());
        dump!("cur_bytes", "{}", tdb.tdb_cur_bytes.get());
        dump!("exp_timeout", "{}", tdb.tdb_exp_timeout.get());
        dump!("soft_timeout", "{}", tdb.tdb_soft_timeout.get());
        dump!("established", "{}", tdb.tdb_established.get());
        dump!("first_use", "{}", tdb.tdb_first_use.get());
        dump!("soft_first_use", "{}", tdb.tdb_soft_first_use.get());
        dump!("exp_first_use", "{}", tdb.tdb_exp_first_use.get());
        dump!("last_used", "{}", tdb.tdb_last_used.get());
        dump!("last_marked", "{}", tdb.tdb_last_marked.get());
        // tdb_data
        dump!("cryptoid", "{}", tdb.tdb_cryptoid.get());
        dump!("tdb_spi", "{:08x}", ntohl(tdb.tdb_spi.get()));
        dump!("amxkeylen", "{}", tdb.tdb_amxkeylen.get());
        dump!("emxkeylen", "{}", tdb.tdb_emxkeylen.get());
        dump!("ivlen", "{}", tdb.tdb_ivlen.get());
        dump!("sproto", "{}", tdb.tdb_sproto.get());
        dump!("wnd", "{}", tdb.tdb_wnd.get());
        dump!("satype", "{}", tdb.tdb_satype.get());
        dump!("updates", "{}", tdb.tdb_updates.get());
        dump!("dst", "{}", ipsp_address(&tdb.tdb_dst.get()));
        dump!("src", "{}", ipsp_address(&tdb.tdb_src.get()));
        dump!("amxkey", "{:p}", tdb.tdb_amxkey.get());
        dump!("emxkey", "{:p}", tdb.tdb_emxkey.get());
        dump!("rpl", "{}", tdb.tdb_rpl.get());
        // tdb_seen
        // tdb_iv
        dump!(
            "ids",
            "{:p}",
            tdb.tdb_ids.get().map_or(ptr::null(), ptr::from_ref)
        );
        dump!("ids_swapped", "{}", tdb.tdb_ids_swapped.get());
        dump!("mtu", "{}", tdb.tdb_mtu.get());
        dump!("mtutimeout", "{}", tdb.tdb_mtutimeout.get());
        dump!("udpencap_port", "{}", ntohs(tdb.tdb_udpencap_port.get()));
        dump!("tag", "{}", tdb.tdb_tag.get());
        dump!("tap", "{}", tdb.tdb_tap.get());
        dump!("rdomain", "{}", tdb.tdb_rdomain.get());
        dump!("rdomain_post", "{}", tdb.tdb_rdomain_post.get());
        // tdb_filter
        // tdb_filtermask
        // tdb_policy_head
        // tdb_sync_entry
    } else {
        pr(format_args!("{:p}:", tdb));
        pr(format_args!(" {:08x}", ntohl(tdb.tdb_spi.get())));
        pr(format_args!(" {}", ipsp_address(&tdb.tdb_src.get())));
        pr(format_args!("->{}", ipsp_address(&tdb.tdb_dst.get())));
        pr(format_args!(":{}", tdb.tdb_sproto.get()));
        pr(format_args!(
            " #{}",
            tdb.tdb_refcnt.r_refs.load(Ordering::Relaxed)
        ));
        pr(format_args!(" {:08x}\n", tdb.tdb_flags.get()));
    }
}

/// `tdb_walk`: calls `walker(tdb, last)` on every TDB of `rdomain` until one fails. The walker
/// may sleep, so the TDBs are first collected on a private list (holding references) under
/// `tdb_sadb_mtx`, which is then released; the exclusive net lock keeps the list ours.
pub fn tdb_walk(
    rdomain: u32,
    mut walker: impl FnMut(&'static Tdb, bool) -> Result<(), Errno>,
) -> Result<(), Errno> {
    let tdblist: SimpleqHead<TdbWalkList> = SimpleqHead::new();

    // The walker may sleep. So we cannot hold the tdb_sadb_mtx while traversing the
    // tdb_hnext list. Create a new tdb_walk list with exclusive netlock protection.
    net_assert_locked_exclusive("tdb_walk");
    tdblist.init();

    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    for head in &t.tdbh {
        let mut tdbp = *head;
        while let Some(tp) = tdbp {
            tdbp = tp.tdb_hnext.get();
            if rdomain != tp.tdb_rdomain.get() {
                continue;
            }
            tdb_ref(Some(tp));
            // SAFETY: the exclusive net lock keeps `tdb_walk` links ours; the TDB is held.
            unsafe { tdblist.insert_tail(tp) };
        }
    }
    mtx_leave(&TDB_SADB_MTX);

    let mut rval = Ok(());
    while let Some(tp) = tdblist.first() {
        // SAFETY: TDBs are `tdb_pool` items that stay allocated while referenced; the list
        // holds a reference to each, dropped below.
        let tp: &'static Tdb = unsafe { &*ptr::from_ref(tp) };
        // SAFETY: `tp` is the head of our list.
        unsafe { tdblist.remove_head() };
        if rval.is_ok() {
            rval = walker(tp, tdblist.is_empty());
        }
        tdb_unref(Some(tp));
    }

    rval
}

/// The TDB a timeout was set up with (`timeout_set_proc(.., tdb_timeout, tdbp)`).
///
/// # Safety
///
/// `v` is the argument `tdb_alloc` gave the timeout: a TDB that holds a reference for the
/// pending timeout.
unsafe fn tdb_of(v: *mut c_void) -> &'static Tdb {
    // SAFETY: the caller's contract.
    unsafe { &*v.cast::<Tdb>() }
}

/// `tdb_timeout`: the hard lifetime ran out.
fn tdb_timeout(v: *mut c_void) {
    // SAFETY: set up by `tdb_alloc`; the pending timeout held a reference.
    let tdb = unsafe { tdb_of(v) };

    net_lock();
    if tdb.has_flags(TDBF_TIMER) {
        // If it's an "invalid" TDB do a silent expiration.
        if !tdb.has_flags(TDBF_INVALID) {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
        }
        tdb_delete(tdb);
    }
    // decrement refcount of the timeout argument
    tdb_unref(Some(tdb));
    net_unlock();
}

/// `tdb_firstuse`: the hard first-use lifetime ran out.
fn tdb_firstuse(v: *mut c_void) {
    // SAFETY: as in `tdb_timeout`.
    let tdb = unsafe { tdb_of(v) };

    net_lock();
    if tdb.has_flags(TDBF_SOFT_FIRSTUSE) {
        // If the TDB hasn't been used, don't renew it.
        if tdb.tdb_first_use.get() != 0 {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
        }
        tdb_delete(tdb);
    }
    // decrement refcount of the timeout argument
    tdb_unref(Some(tdb));
    net_unlock();
}

/// `tdb_addtimeouts`: arms the hard and soft lifetime timeouts the flags ask for, each with a
/// reference.
pub fn tdb_addtimeouts(tdbp: &'static Tdb) {
    mtx_enter(&tdbp.tdb_mtx);
    if tdbp.has_flags(TDBF_TIMER)
        && timeout_add_sec(&tdbp.tdb_timer_tmo, tdbp.tdb_exp_timeout.get() as i32)
    {
        tdb_ref(Some(tdbp));
    }
    if tdbp.has_flags(TDBF_SOFT_TIMER)
        && timeout_add_sec(&tdbp.tdb_stimer_tmo, tdbp.tdb_soft_timeout.get() as i32)
    {
        tdb_ref(Some(tdbp));
    }
    mtx_leave(&tdbp.tdb_mtx);
}

/// `tdb_soft_timeout`: soft expirations.
fn tdb_soft_timeout(v: *mut c_void) {
    // SAFETY: as in `tdb_timeout`.
    let tdb = unsafe { tdb_of(v) };

    net_lock();
    mtx_enter(&tdb.tdb_mtx);
    if tdb.has_flags(TDBF_SOFT_TIMER) {
        tdb.clr_flags(TDBF_SOFT_TIMER);
        mtx_leave(&tdb.tdb_mtx);
        // Soft expirations.
        let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_SOFT);
    } else {
        mtx_leave(&tdb.tdb_mtx);
    }
    // decrement refcount of the timeout argument
    tdb_unref(Some(tdb));
    net_unlock();
}

/// `tdb_soft_firstuse`.
fn tdb_soft_firstuse(v: *mut c_void) {
    // SAFETY: as in `tdb_timeout`.
    let tdb = unsafe { tdb_of(v) };

    net_lock();
    mtx_enter(&tdb.tdb_mtx);
    if tdb.has_flags(TDBF_SOFT_FIRSTUSE) {
        tdb.clr_flags(TDBF_SOFT_FIRSTUSE);
        mtx_leave(&tdb.tdb_mtx);
        // If the TDB hasn't been used, don't renew it.
        if tdb.tdb_first_use.get() != 0 {
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_SOFT);
        }
    } else {
        mtx_leave(&tdb.tdb_mtx);
    }
    // decrement refcount of the timeout argument
    tdb_unref(Some(tdb));
    net_unlock();
}

impl TdbTables {
    /// `tdb_rehash`: doubles the tables under a new key.
    fn tdb_rehash(&mut self) -> Result<(), Errno> {
        let old_hashmask = self.tdb_hashmask;
        let new_hashmask = (old_hashmask << 1) | 1;
        let n = (new_hashmask + 1) as usize;

        let mut new_tdbh: Vec<Option<&'static Tdb>> = Vec::new();
        let mut new_tdbdst: Vec<Option<&'static Tdb>> = Vec::new();
        let mut new_srcaddr: Vec<Option<&'static Tdb>> = Vec::new();
        if new_tdbh.try_reserve_exact(n).is_err()
            || new_tdbdst.try_reserve_exact(n).is_err()
            || new_srcaddr.try_reserve_exact(n).is_err()
        {
            return Err(Errno::ENOMEM);
        }
        new_tdbh.resize(n, None);
        new_tdbdst.resize(n, None);
        new_srcaddr.resize(n, None);

        self.tdb_hashmask = new_hashmask;
        self.tdbkey = random_siphash_key();

        for i in 0..=old_hashmask as usize {
            let mut tdbp = self.tdbh[i];
            while let Some(tp) = tdbp {
                let tdbnp = tp.tdb_hnext.get();
                let hashval =
                    self.tdb_hash(tp.tdb_spi.get(), &tp.tdb_dst.get(), tp.tdb_sproto.get());
                tp.tdb_hnext.set(new_tdbh[hashval as usize]);
                new_tdbh[hashval as usize] = Some(tp);
                tdbp = tdbnp;
            }

            let mut tdbp = self.tdbdst[i];
            while let Some(tp) = tdbp {
                let tdbnp = tp.tdb_dnext.get();
                let hashval = self.tdb_hash(0, &tp.tdb_dst.get(), tp.tdb_sproto.get());
                tp.tdb_dnext.set(new_tdbdst[hashval as usize]);
                new_tdbdst[hashval as usize] = Some(tp);
                tdbp = tdbnp;
            }

            let mut tdbp = self.tdbsrc[i];
            while let Some(tp) = tdbp {
                let tdbnp = tp.tdb_snext.get();
                let hashval = self.tdb_hash(0, &tp.tdb_src.get(), tp.tdb_sproto.get());
                tp.tdb_snext.set(new_srcaddr[hashval as usize]);
                new_srcaddr[hashval as usize] = Some(tp);
                tdbp = tdbnp;
            }
        }

        self.tdbh = new_tdbh;
        self.tdbdst = new_tdbdst;
        self.tdbsrc = new_srcaddr;

        Ok(())
    }
}

/// `puttdb`: add TDB in the hash table.
pub fn puttdb(tdbp: &'static Tdb) {
    mtx_enter(&TDB_SADB_MTX);
    puttdb_locked(tdbp);
    mtx_leave(&TDB_SADB_MTX);
}

/// `puttdb_locked`: `puttdb` with `tdb_sadb_mtx` held.
pub fn puttdb_locked(tdbp: &'static Tdb) {
    let t = tables();

    let mut hashval = t.tdb_hash(
        tdbp.tdb_spi.get(),
        &tdbp.tdb_dst.get(),
        tdbp.tdb_sproto.get(),
    );

    // Rehash if this tdb would cause a bucket to have more than two items and if the number
    // of tdbs exceed 10% of the bucket count. This number is arbitrarily chosen and is just a
    // measure to not keep rehashing when adding and removing tdbs which happens to always end
    // up in the same bucket, which is not uncommon when doing manual keying.
    if let Some(h) = t.tdbh[hashval as usize]
        && h.tdb_hnext.get().is_some()
        && t.tdb_count * 10 > (t.tdb_hashmask + 1) as i32
        && t.tdb_rehash().is_ok()
    {
        hashval = t.tdb_hash(
            tdbp.tdb_spi.get(),
            &tdbp.tdb_dst.get(),
            tdbp.tdb_sproto.get(),
        );
    }

    tdbp.tdb_hnext.set(t.tdbh[hashval as usize]);
    t.tdbh[hashval as usize] = Some(tdbp);

    t.tdb_count += 1;
    if tdbp.tdb_flags.get() & (TDBF_INVALID | TDBF_TUNNELING) == TDBF_TUNNELING {
        ipsecstat_inc(IpsecCounters::IpsecTunnels);
    }

    IPSEC_LAST_ADDED.store(getuptime() as u64, Ordering::Relaxed);

    if tdbp.has_flags(TDBF_IFACE) {
        // NSEC > 0: sec_tdb_insert(tdbp); not configured.
        return;
    }

    let hashval = t.tdb_hash(0, &tdbp.tdb_dst.get(), tdbp.tdb_sproto.get());
    tdbp.tdb_dnext.set(t.tdbdst[hashval as usize]);
    t.tdbdst[hashval as usize] = Some(tdbp);

    let hashval = t.tdb_hash(0, &tdbp.tdb_src.get(), tdbp.tdb_sproto.get());
    tdbp.tdb_snext.set(t.tdbsrc[hashval as usize]);
    t.tdbsrc[hashval as usize] = Some(tdbp);
}

/// `tdb_unlink`: removes a TDB from the hash tables.
pub fn tdb_unlink(tdbp: &Tdb) {
    mtx_enter(&TDB_SADB_MTX);
    tdb_unlink_locked(tdbp);
    mtx_leave(&TDB_SADB_MTX);
}

/// Unlinks `tdbp` from the chain starting at `head`, through the link `next` selects.
fn tdb_chain_unlink(
    head: &mut Option<&'static Tdb>,
    tdbp: &Tdb,
    next: impl Fn(&Tdb) -> &Cell<Option<&'static Tdb>>,
) {
    if head.is_some_and(|h| ptr::eq(h, tdbp)) {
        *head = next(tdbp).get();
    } else {
        let mut tdbpp = *head;
        while let Some(tp) = tdbpp {
            if next(tp).get().is_some_and(|n| ptr::eq(n, tdbp)) {
                next(tp).set(next(tdbp).get());
                break;
            }
            tdbpp = next(tp).get();
        }
    }
    next(tdbp).set(None);
}

/// `tdb_unlink_locked`: `tdb_unlink` with `tdb_sadb_mtx` held.
pub fn tdb_unlink_locked(tdbp: &Tdb) {
    let t = tables();

    let hashval = t.tdb_hash(
        tdbp.tdb_spi.get(),
        &tdbp.tdb_dst.get(),
        tdbp.tdb_sproto.get(),
    );
    tdb_chain_unlink(&mut t.tdbh[hashval as usize], tdbp, |x| &x.tdb_hnext);

    t.tdb_count -= 1;
    if tdbp.tdb_flags.get() & (TDBF_INVALID | TDBF_TUNNELING) == TDBF_TUNNELING {
        ipsecstat_dec(IpsecCounters::IpsecTunnels);
        ipsecstat_inc(IpsecCounters::IpsecPrevtunnels);
    }

    if tdbp.has_flags(TDBF_IFACE) {
        // NSEC > 0: sec_tdb_remove(tdbp); not configured.
        return;
    }

    let hashval = t.tdb_hash(0, &tdbp.tdb_dst.get(), tdbp.tdb_sproto.get());
    tdb_chain_unlink(&mut t.tdbdst[hashval as usize], tdbp, |x| &x.tdb_dnext);

    let hashval = t.tdb_hash(0, &tdbp.tdb_src.get(), tdbp.tdb_sproto.get());
    tdb_chain_unlink(&mut t.tdbsrc[hashval as usize], tdbp, |x| &x.tdb_snext);
}

/// `tdb_cleanspd`: drops the policies' cached references to a TDB.
pub fn tdb_cleanspd(tdbp: &Tdb) {
    mtx_enter(&crate::netinet::ip_spd::IPO_TDB_MTX);
    while let Some(ipo) = tdbp.tdb_policy_head.first() {
        // SAFETY: `ipo_tdb_mtx` is held; the policy is on this TDB's list.
        unsafe { tdbp.tdb_policy_head.remove(ipo) };
        tdb_unref(ipo.ipo_tdb.get());
        ipo.ipo_tdb.set(None);
        ipo.ipo_last_searched.set(0); // Force a re-search.
    }
    mtx_leave(&crate::netinet::ip_spd::IPO_TDB_MTX);
}

/// `tdb_unbundle`: breaks the `tdb_onext`/`tdb_inext` links of a TDB and their references.
pub fn tdb_unbundle(tdbp: &Tdb) {
    if let Some(onext) = tdbp.tdb_onext.get() {
        if onext.tdb_inext.get().is_some_and(|i| ptr::eq(i, tdbp)) {
            tdb_unref(onext.tdb_inext.get()); // to us
            onext.tdb_inext.set(None);
        }
        tdb_unref(Some(onext)); // to other
        tdbp.tdb_onext.set(None);
    }
    if let Some(inext) = tdbp.tdb_inext.get() {
        if inext.tdb_onext.get().is_some_and(|o| ptr::eq(o, tdbp)) {
            tdb_unref(inext.tdb_onext.get()); // to us
            inext.tdb_onext.set(None);
        }
        tdb_unref(Some(inext)); // to other
        tdbp.tdb_inext.set(None);
    }
}

/// `tdb_deltimeouts`: cancels the lifetime timeouts and drops their references.
pub fn tdb_deltimeouts(tdbp: &Tdb) {
    mtx_enter(&tdbp.tdb_mtx);
    tdbp.clr_flags(TDBF_FIRSTUSE | TDBF_SOFT_FIRSTUSE | TDBF_TIMER | TDBF_SOFT_TIMER);
    if timeout_del(&tdbp.tdb_timer_tmo) {
        tdb_unref(Some(tdbp));
    }
    if timeout_del(&tdbp.tdb_first_tmo) {
        tdb_unref(Some(tdbp));
    }
    if timeout_del(&tdbp.tdb_stimer_tmo) {
        tdb_unref(Some(tdbp));
    }
    if timeout_del(&tdbp.tdb_sfirst_tmo) {
        tdb_unref(Some(tdbp));
    }
    mtx_leave(&tdbp.tdb_mtx);
}

/// `tdb_ref`: takes a reference (NULL passes through).
pub fn tdb_ref(tdb: Option<&'static Tdb>) -> Option<&'static Tdb> {
    let tdb = tdb?;
    refcnt_take(&tdb.tdb_refcnt);
    Some(tdb)
}

/// `tdb_unref`: drops a reference, freeing the TDB on the last.
pub fn tdb_unref(tdb: Option<&Tdb>) {
    let Some(tdb) = tdb else {
        return;
    };
    if !refcnt_rele(&tdb.tdb_refcnt) {
        return;
    }
    tdb_free(tdb);
}

/// `tdb_delete`: takes a TDB out of service: unlinked from the tables, the policies and the
/// bundle, its timeouts cancelled; it is freed with the last reference.
pub fn tdb_delete(tdbp: &Tdb) {
    net_assert_locked("tdb_delete");

    mtx_enter(&tdbp.tdb_mtx);
    if tdbp.has_flags(TDBF_DELETED) {
        mtx_leave(&tdbp.tdb_mtx);
        return;
    }
    tdbp.set_flags(TDBF_DELETED);
    mtx_leave(&tdbp.tdb_mtx);
    tdb_unlink(tdbp);

    // cleanup SPD references
    tdb_cleanspd(tdbp);
    // release tdb_onext/tdb_inext references
    tdb_unbundle(tdbp);
    // delete timeouts and release references
    tdb_deltimeouts(tdbp);
    // release the reference for tdb_unlink()
    tdb_unref(Some(tdbp));
}

/// `tdb_alloc`: allocate a TDB and initialize a few basic fields. One reference, for the
/// tables `puttdb` will put it in.
pub fn tdb_alloc(rdomain: u32) -> &'static Tdb {
    let Some(mem) = pool_get(&TDB_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("tdb_alloc: pool_get(PR_WAITOK) failed"));
    };
    let raw = mem.cast::<Tdb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `tdb_pool` item of `size_of::<Tdb>()` bytes, written
    // once before anything else sees it; it stays allocated until `tdb_free`.
    unsafe { raw.write(Tdb::new()) };
    // SAFETY: as above.
    let tdbp: &'static Tdb = unsafe { &*raw };

    refcnt_init_trace(&tdbp.tdb_refcnt, DT_REFCNT_IDX_TDB);
    mtx_init(&tdbp.tdb_mtx, IPL_SOFTNET);
    tdbp.tdb_policy_head.init();

    // Record establishment time.
    tdbp.tdb_established.set(gettime() as u64);

    // Save routing domain
    tdbp.tdb_rdomain.set(rdomain);
    tdbp.tdb_rdomain_post.set(rdomain);

    // Initialize counters: the array is zero (see the deviations).

    // Initialize timeouts.
    let arg = raw.cast::<c_void>();
    timeout_set_proc(&tdbp.tdb_timer_tmo, tdb_timeout, arg);
    timeout_set_proc(&tdbp.tdb_first_tmo, tdb_firstuse, arg);
    timeout_set_proc(&tdbp.tdb_stimer_tmo, tdb_soft_timeout, arg);
    timeout_set_proc(&tdbp.tdb_sfirst_tmo, tdb_soft_firstuse, arg);

    tdbp
}

/// `tdb_free`: the last reference went: zeroize the transform and give the TDB back.
pub fn tdb_free(tdbp: &Tdb) {
    net_assert_locked("tdb_free");

    if let Some(xf) = tdbp.tdb_xform.get() {
        let _ = (xf.xf_zeroize)(tdbp);
        tdbp.tdb_xform.set(None);
    }

    crate::net::if_pfsync::pfsync_delete_tdb(tdbp);

    kassert!(tdbp.tdb_policy_head.is_empty());

    if let Some(ids) = tdbp.tdb_ids.get() {
        ipsp_ids_free(Some(ids));
        tdbp.tdb_ids.set(None);
    }

    pf_tag_unref(tdbp.tdb_tag.get());

    // counters_free: the counters are part of the TDB.

    kassert!(tdbp.tdb_onext.get().is_none());
    kassert!(tdbp.tdb_inext.get().is_none());

    // Remove expiration timeouts.
    kassert!(!crate::sys::timeout::timeout_pending(&tdbp.tdb_timer_tmo));
    kassert!(!crate::sys::timeout::timeout_pending(&tdbp.tdb_first_tmo));
    kassert!(!crate::sys::timeout::timeout_pending(&tdbp.tdb_stimer_tmo));
    kassert!(!crate::sys::timeout::timeout_pending(&tdbp.tdb_sfirst_tmo));

    pool_put(&TDB_POOL, NonNull::from(tdbp).cast());
}

/// `tdb_init`: do further initializations of a TDB: the transform of type `alg` sets it up.
pub fn tdb_init(tdbp: &Tdb, alg: u16, ii: &mut IpsecInit<'_>) -> Result<(), Errno> {
    for xsp in &XFORMSW {
        if xsp.xf_type == alg {
            return (xsp.xf_init)(tdbp, xsp, ii);
        }
    }

    crate::ipsec_dprintf!(
        "tdb_init",
        "no alg {} for spi {:08x}, addr {}, proto {}",
        alg,
        ntohl(tdbp.tdb_spi.get()),
        ipsp_address(&tdbp.tdb_dst.get()),
        tdbp.tdb_sproto.get()
    );

    Err(Errno::EINVAL)
}

/// `ipsp_address` (`DDB`, `ENCDEBUG`): a printable string for the address (see the
/// deviations: `inet_ntop` is not ported).
pub fn ipsp_address(sa: &SockaddrUnion) -> &'static str {
    match sa.sa_family() {
        AF_INET => {
            // inet_ntop(AF_INET, &sa->sin.sin_addr, buf, size): netinet/inet_ntop.c is not
            // ported.
            let _ = crate::unported!("inet_ntop");
            "(inet_ntop not ported)"
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            // inet_ntop(AF_INET6, &sa->sin6.sin6_addr, buf, size): netinet/inet_ntop.c is
            // not ported.
            let _ = crate::unported!("inet_ntop");
            "(inet_ntop not ported)"
        }
        _ => "(unknown address family)",
    }
}

/// `ipsp_is_unspecified`: check whether an IP{4,6} address is unspecified.
pub fn ipsp_is_unspecified(addr: SockaddrUnion) -> bool {
    match addr.sa_family() {
        AF_INET => addr.sin_addr().s_addr == INADDR_ANY,
        #[cfg(feature = "inet6")]
        AF_INET6 => in6_is_addr_unspecified(&addr.sin6_addr()),
        // 0: No family set.
        _ => true,
    }
}

/// `ipsp_ids_match`: the same pair of identities (they are shared, so compared by address).
pub fn ipsp_ids_match(a: &IpsecIds, b: &IpsecIds) -> bool {
    ptr::eq(a, b)
}

/// `ipsp_ids_insert`: enters a new pair of identities, or takes a reference to the equal one
/// already there (the caller frees its own then). `None` when every flow number is taken.
pub fn ipsp_ids_insert(ids: &'static IpsecIds) -> Option<&'static IpsecIds> {
    mtx_enter(&IPSEC_FLOWS_MTX);
    let f = flows();

    // SAFETY: `ipsec_flows_mtx` is held; `ids` is a new pair, in no tree.
    let found = unsafe { f.ipsec_ids_tree.insert(ids) };
    if let Some(found) = found {
        // SAFETY: tree elements are `'static` allocations (`malloc(M_CREDENTIALS)`).
        let found: &'static IpsecIds = unsafe { &*ptr::from_ref(found) };
        // if refcount was zero, then timeout is running
        found.id_refcount.set(found.id_refcount.get() + 1);
        if found.id_refcount.get() == 1 {
            // SAFETY: a pair with no references is on the garbage list.
            unsafe { ListHead::<IpsecIdsGcList>::remove(found) };

            if f.ipsp_ids_gc_list.is_empty() {
                timeout_del(&IPSP_IDS_GC_TIMEOUT);
            }
        }
        mtx_leave(&IPSEC_FLOWS_MTX);
        crate::ipsec_dprintf!(
            "ipsp_ids_insert",
            "ids {:p} count {}",
            found,
            found.id_refcount.get()
        );
        return Some(found);
    }

    ids.id_refcount.set(1);
    let start_flow = f.ipsec_ids_next_flow;
    ids.id_flow.set(start_flow);

    f.ipsec_ids_next_flow = f.ipsec_ids_next_flow.wrapping_add(1);
    if f.ipsec_ids_next_flow == 0 {
        f.ipsec_ids_next_flow = 1;
    }
    // SAFETY: `ipsec_flows_mtx` is held; `ids` is in no flow tree yet.
    while unsafe { f.ipsec_ids_flows.insert(ids) }.is_some() {
        ids.id_flow.set(f.ipsec_ids_next_flow);
        f.ipsec_ids_next_flow = f.ipsec_ids_next_flow.wrapping_add(1);
        if f.ipsec_ids_next_flow == 0 {
            f.ipsec_ids_next_flow = 1;
        }
        if f.ipsec_ids_next_flow == start_flow {
            // SAFETY: `ids` was inserted in the identity tree above.
            unsafe { f.ipsec_ids_tree.remove(ids) };
            mtx_leave(&IPSEC_FLOWS_MTX);
            crate::ipsec_dprintf!(
                "ipsp_ids_insert",
                "ipsec_ids_next_flow exhausted {}",
                start_flow
            );
            return None;
        }
    }
    mtx_leave(&IPSEC_FLOWS_MTX);
    crate::ipsec_dprintf!(
        "ipsp_ids_insert",
        "new ids {:p} flow {}",
        ids,
        ids.id_flow.get()
    );
    Some(ids)
}

/// `ipsp_ids_lookup`: the pair with flow number `ipsecflowinfo`, with a reference; `None`
/// when unknown or on its way out.
pub fn ipsp_ids_lookup(ipsecflowinfo: u32) -> Option<&'static IpsecIds> {
    let key = IpsecIds::new(NonNull::dangling(), NonNull::dangling());
    key.id_flow.set(ipsecflowinfo);

    mtx_enter(&IPSEC_FLOWS_MTX);
    let f = flows();
    let found = f.ipsec_ids_flows.find(&key).map(|ids| {
        // SAFETY: tree elements are `'static` allocations.
        unsafe { &*ptr::from_ref(ids) }
    });
    let ids = match found {
        Some(ids) if ids.id_refcount.get() != 0 => {
            ids.id_refcount.set(ids.id_refcount.get() + 1);
            Some(ids)
        }
        _ => None,
    };
    mtx_leave(&IPSEC_FLOWS_MTX);

    ids
}

/// The size `import_identity` allocated an identity with.
fn ipsec_id_size(id: &IpsecId) -> usize {
    size_of::<IpsecId>() + usize::try_from(id.len).unwrap_or(0)
}

/// `ipsp_ids_gc`: free ids only from delayed timeout.
fn ipsp_ids_gc(_arg: *mut c_void) {
    mtx_enter(&IPSEC_FLOWS_MTX);
    let f = flows();

    for ids in f.ipsp_ids_gc_list.iter() {
        kassert!(ids.id_refcount.get() == 0);
        crate::ipsec_dprintf!(
            "ipsp_ids_gc",
            "ids {:p} count {}",
            ids,
            ids.id_refcount.get()
        );

        ids.id_gc_ttl.set(ids.id_gc_ttl.get().wrapping_sub(1));
        if ids.id_gc_ttl.get() > 0 {
            continue;
        }

        // SAFETY: `ipsec_flows_mtx` is held; the pair is on the garbage list and in both
        // trees, and nothing references it (the count is zero).
        unsafe {
            ListHead::<IpsecIdsGcList>::remove(ids);
            f.ipsec_ids_tree.remove(ids);
            f.ipsec_ids_flows.remove(ids);
        }
        let (local, remote) = (ids.id_local, ids.id_remote);
        let (lsz, rsz) = (
            ipsec_id_size(ids.id_local()),
            ipsec_id_size(ids.id_remote()),
        );
        free(local.cast(), M_CREDENTIALS, lsz);
        free(remote.cast(), M_CREDENTIALS, rsz);
        free(
            NonNull::from(ids).cast(),
            M_CREDENTIALS,
            size_of::<IpsecIds>(),
        );
    }

    if !f.ipsp_ids_gc_list.is_empty() {
        timeout_add_sec(&IPSP_IDS_GC_TIMEOUT, 1);
    }

    mtx_leave(&IPSEC_FLOWS_MTX);
}

/// `ipsp_ids_free`: decrements refcount, actual free happens in gc.
pub fn ipsp_ids_free(ids: Option<&'static IpsecIds>) {
    let Some(ids) = ids else {
        return;
    };

    mtx_enter(&IPSEC_FLOWS_MTX);
    let f = flows();

    // If the refcount becomes zero, then a timeout is started. This timeout must be
    // cancelled if refcount is increased from zero.
    crate::ipsec_dprintf!(
        "ipsp_ids_free",
        "ids {:p} count {}",
        ids,
        ids.id_refcount.get()
    );
    kassert!(ids.id_refcount.get() > 0);

    ids.id_refcount.set(ids.id_refcount.get() - 1);
    if ids.id_refcount.get() > 0 {
        mtx_leave(&IPSEC_FLOWS_MTX);
        return;
    }

    // Add second for the case ipsp_ids_gc() is already running and awaits netlock to be
    // released.
    ids.id_gc_ttl
        .set(IPSEC_IDS_IDLE.load(Ordering::Relaxed) as u32 + 1);

    if f.ipsp_ids_gc_list.is_empty() {
        timeout_add_sec(&IPSP_IDS_GC_TIMEOUT, 1);
    }
    // SAFETY: `ipsec_flows_mtx` is held; a pair whose count just dropped to zero is not on
    // the garbage list.
    unsafe { f.ipsp_ids_gc_list.insert_head(ids) };

    mtx_leave(&IPSEC_FLOWS_MTX);
}

/// `ipsp_id_cmp`: identities by type, length, then data.
fn ipsp_id_cmp(a: &IpsecId, b: &IpsecId) -> CmpOrdering {
    a.type_.cmp(&b.type_).then(a.len.cmp(&b.len)).then_with(|| {
        a.data()
            .cmp(&b.data()[..a.data().len().min(b.data().len())])
    })
}

/// `ipsp_ids_cmp`: pairs by remote, then local identity.
fn ipsp_ids_cmp(a: &IpsecIds, b: &IpsecIds) -> CmpOrdering {
    ipsp_id_cmp(a.id_remote(), b.id_remote()).then_with(|| ipsp_id_cmp(a.id_local(), b.id_local()))
}

/// `ipsp_ids_flow_cmp`: pairs by flow number.
fn ipsp_ids_flow_cmp(a: &IpsecIds, b: &IpsecIds) -> CmpOrdering {
    a.id_flow.get().cmp(&b.id_flow.get())
}

/// Forgets every TDB and identity: the host tests start from an empty database.
#[cfg(test)]
pub(crate) fn ipsp_reset() {
    mtx_enter(&TDB_SADB_MTX);
    let t = tables();
    let n = TDB_HASHSIZE_INIT as usize;
    t.tdbh = alloc::vec![None; n];
    t.tdbdst = alloc::vec![None; n];
    t.tdbsrc = alloc::vec![None; n];
    t.tdb_hashmask = TDB_HASHSIZE_INIT - 1;
    t.tdb_count = 0;
    mtx_leave(&TDB_SADB_MTX);
    pool_init(&TDB_POOL, size_of::<Tdb>(), 0, IPL_SOFTNET, 0, "tdb", None);
    mtx_enter(&IPSEC_FLOWS_MTX);
    let f = flows();
    f.ipsec_ids_next_flow = 1;
    f.ipsec_ids_tree = RbtHead::new();
    f.ipsec_ids_flows = RbtHead::new();
    f.ipsp_ids_gc_list = ListHead::new();
    mtx_leave(&IPSEC_FLOWS_MTX);
    IPSEC_POLICY_HEAD.0.init();
    IPSEC_IN_USE.store(0, Ordering::Relaxed);
}

// LP64 sizes and offsets of the C structures.
const _: () = {
    assert!(size_of::<SockaddrUnion>() == SIZEOF_SOCKADDR_IN6);
    assert!(size_of::<SockaddrIn>() == 16);
    assert!(size_of::<SockaddrEncap>() == SENT_LEN);
    assert!(size_of::<Ipsecstat>() == IpsecCounters::IpsecNcounters as usize * 8);
    assert!(size_of::<IpsecId>() == 4);
    assert!(size_of::<TdbIdent>() == 40);
    assert!(offset_of!(TdbIdent, dst) == 4);
    assert!(offset_of!(TdbIdent, proto) == 32);
    assert!(offset_of!(TdbIdent, rdomain) == 36);
    assert!(TDB_SEEN_WORDS == 67);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the SA database: TDB lookups by SPI and destination, by destination and by
    // source, deletion, the tables growing past their first size, SPI reservation, and the
    // shared identities.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::kern_malloc::malloc;
    use crate::netinet::in_::{IPPROTO_AH, IPPROTO_ESP};
    use crate::netinet::ip_input::tests::sin;
    use crate::sys::endian::htons;
    use crate::sys::malloc::M_WAITOK;

    /// Memory, timeouts and the network (as the IPv4 tests set them up), and an empty SA database.
    fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        let g = crate::netinet::ip_input::tests::setup();
        ipsp_reset();
        g
    }

    /// A `sockaddr_in` union for `a`.
    fn su(a: [u8; 4]) -> SockaddrUnion {
        SockaddrUnion::from_sin(&sin(a))
    }

    /// A valid TDB in the tables: `spi` (host order) from `src` to `dst` with `sproto`.
    fn tdb(spi: u32, src: [u8; 4], dst: [u8; 4], sproto: i32) -> &'static Tdb {
        let t = tdb_alloc(0);
        t.tdb_spi.set(htonl(spi));
        t.tdb_src.set(su(src));
        t.tdb_dst.set(su(dst));
        t.tdb_sproto.set(sproto as u8);
        puttdb(t);
        t
    }

    const A: [u8; 4] = [10, 0, 2, 15];
    const B: [u8; 4] = [10, 0, 2, 2];
    const C: [u8; 4] = [192, 168, 77, 2];

    #[test]
    fn tdbs_are_found_by_spi_destination_and_protocol() {
        let _g = setup();
        let esp = tdb(0x1000, A, B, IPPROTO_ESP);
        let ah = tdb(0x1000, A, B, IPPROTO_AH);
        let other = tdb(0x2000, A, C, IPPROTO_ESP);

        let found = gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).expect("the ESP SA");
        assert!(ptr::eq(found, esp));
        assert_eq!(
            found.tdb_refcnt.r_refs.load(Ordering::Relaxed),
            2,
            "the lookup's reference"
        );
        tdb_unref(Some(found));
        let found = gettdb(0, htonl(0x1000), &su(B), IPPROTO_AH as u8).expect("the AH SA");
        assert!(ptr::eq(found, ah));
        tdb_unref(Some(found));

        assert!(
            gettdb(0, htonl(0x1001), &su(B), IPPROTO_ESP as u8).is_none(),
            "other SPI"
        );
        assert!(
            gettdb(0, htonl(0x1000), &su(C), IPPROTO_ESP as u8).is_none(),
            "other dst"
        );
        assert!(
            gettdb(1, htonl(0x1000), &su(B), IPPROTO_ESP as u8).is_none(),
            "other rdomain"
        );

        // By destination and by source, for the SPD.
        let by_dst = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, None, None).expect("by dst");
        assert!(ptr::eq(by_dst, other));
        tdb_unref(Some(by_dst));
        let by_src = gettdbbysrc(0, &su(A), IPPROTO_AH as u8, None, None, None).expect("by src");
        assert!(ptr::eq(by_src, ah));
        tdb_unref(Some(by_src));
        let both = gettdbbysrcdst(0, 0, &su(A), &su(C), IPPROTO_ESP as u8).expect("by src and dst");
        assert!(ptr::eq(both, other));
        tdb_unref(Some(both));

        // An invalid (larval) SA is found by SPI only.
        esp.set_flags(TDBF_INVALID);
        assert!(gettdbbydst(0, &su(B), IPPROTO_ESP as u8, None, None, None).is_none());
        let larval = gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).expect("by SPI");
        tdb_unref(Some(larval));
        esp.clr_flags(TDBF_INVALID);

        // A filter must match exactly when the SA has one.
        let mut f = SockaddrEncap::new();
        f.set_sen_type(SENT_IP4);
        f.set_sen_ip_dst(sin(C).sin_addr);
        other.tdb_filter.set(f);
        let mut g = f;
        g.set_sen_proto(1);
        assert!(gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&g), Some(&g)).is_none());
        let zero = SockaddrEncap::new();
        let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&zero))
            .expect("the filter and the (zero) masks match");
        tdb_unref(Some(t));
        let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&f));
        assert!(t.is_none(), "the SA's mask is zero, the policy's is not");
        other.tdb_filtermask.set(f);
        let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&f)).expect("match");
        tdb_unref(Some(t));

        // Deleted SAs are gone from every table (and freed with their last reference).
        for t in [esp, ah, other] {
            tdb_delete(t);
        }
        assert!(gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).is_none());
        assert!(gettdbbysrc(0, &su(A), IPPROTO_AH as u8, None, None, None).is_none());
        assert!(gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, None, None).is_none());
    }

    #[test]
    fn the_tables_grow_and_keep_every_tdb() {
        let _g = setup();
        let all: Vec<&'static Tdb> = (0..200u32)
            .map(|i| tdb(0x100 + i, A, [10, 1, (i >> 8) as u8, i as u8], IPPROTO_ESP))
            .collect();

        mtx_enter(&TDB_SADB_MTX);
        let (mask, count) = (tables().tdb_hashmask, tables().tdb_count);
        mtx_leave(&TDB_SADB_MTX);
        assert!(mask > TDB_HASHSIZE_INIT - 1, "the tables were rehashed");
        assert_eq!(count, 200);

        for (i, t) in all.iter().enumerate() {
            let i = i as u32;
            let dst = su([10, 1, (i >> 8) as u8, i as u8]);
            let f = gettdb(0, htonl(0x100 + i), &dst, IPPROTO_ESP as u8).expect("found");
            assert!(ptr::eq(f, *t));
            tdb_unref(Some(f));
            let f = gettdbbydst(0, &dst, IPPROTO_ESP as u8, None, None, None).expect("by dst");
            assert!(ptr::eq(f, *t));
            tdb_unref(Some(f));
        }

        let mut walked = 0;
        net_lock();
        tdb_walk(0, |_, _| {
            walked += 1;
            Ok(())
        })
        .expect("walk");
        net_unlock();
        assert_eq!(walked, 200);

        for t in all {
            tdb_delete(t);
        }
    }

    #[test]
    fn reserve_spi_takes_free_spis_out_of_the_range() {
        let _g = setup();

        assert_eq!(
            reserve_spi(0, 1, 255, &su(A), &su(B), IPPROTO_ESP as u8),
            Err(Errno::EINVAL),
            "only reserved SPIs"
        );
        assert_eq!(
            reserve_spi(0, 5000, 4000, &su(A), &su(B), IPPROTO_ESP as u8),
            Err(Errno::EINVAL),
            "an empty range"
        );

        let spi = reserve_spi(0, 0x4242, 0x4242, &su(A), &su(B), IPPROTO_ESP as u8).expect("free");
        assert_eq!(spi, htonl(0x4242));
        let t = gettdb(0, spi, &su(B), IPPROTO_ESP as u8).expect("reserved");
        assert!(t.has_flags(TDBF_INVALID), "larval until SADB_UPDATE");
        assert_eq!(t.tdb_satype.get(), SADB_SATYPE_UNSPEC);
        assert!(t.has_flags(TDBF_TIMER), "with the embryonic timeout");
        assert_eq!(
            reserve_spi(0, 0x4242, 0x4242, &su(A), &su(B), IPPROTO_ESP as u8),
            Err(Errno::EEXIST)
        );

        let other = reserve_spi(0, 256, 0xffff_ffff, &su(A), &su(B), IPPROTO_ESP as u8)
            .expect("a random free SPI");
        assert!(ntohl(other) > SPI_RESERVED_MAX && other != spi);
        let o = gettdb(0, other, &su(B), IPPROTO_ESP as u8).expect("reserved");

        tdb_delete(t);
        tdb_unref(Some(t));
        tdb_delete(o);
        tdb_unref(Some(o));
    }

    /// A pair of identities as `import_identities` makes them.
    fn ids(local: &[u8], remote: &[u8]) -> &'static IpsecIds {
        let id = |data: &[u8]| {
            let p = malloc(size_of::<IpsecId>() + data.len(), M_CREDENTIALS, M_WAITOK)
                .expect("malloc")
                .cast::<IpsecId>();
            // SAFETY: a fresh allocation of the header and the data.
            unsafe {
                p.as_ptr().write(IpsecId {
                    type_: IPSP_IDENTITY_FQDN,
                    len: data.len() as i16,
                });
                ptr::copy_nonoverlapping(data.as_ptr(), p.as_ptr().add(1).cast::<u8>(), data.len());
            }
            p
        };
        let p = malloc(size_of::<IpsecIds>(), M_CREDENTIALS, M_WAITOK)
            .expect("malloc")
            .cast::<IpsecIds>();
        // SAFETY: a fresh allocation of an `IpsecIds`.
        unsafe {
            p.as_ptr().write(IpsecIds::new(id(local), id(remote)));
            &*p.as_ptr()
        }
    }

    #[test]
    fn identities_are_shared_by_value_and_found_by_flow() {
        let _g = setup();

        let a = ipsp_ids_insert(ids(b"left\0", b"right\0")).expect("inserted");
        assert_eq!(a.id_refcount.get(), 1);
        let flow = a.id_flow.get();
        assert_ne!(flow, 0);

        // The same identities give the same pair, with one more reference.
        let b = ipsp_ids_insert(ids(b"left\0", b"right\0")).expect("found");
        assert!(ptr::eq(a, b));
        assert_eq!(a.id_refcount.get(), 2);

        // Other identities get another flow number.
        let c = ipsp_ids_insert(ids(b"left\0", b"elsewhere\0")).expect("inserted");
        assert!(!ptr::eq(a, c));
        assert_ne!(c.id_flow.get(), flow);

        let l = ipsp_ids_lookup(flow).expect("by flow");
        assert!(ptr::eq(l, a));
        assert_eq!(a.id_refcount.get(), 3);
        assert!(ipsp_ids_lookup(0x7777).is_none());

        // Unreferenced pairs wait for the garbage collector and are not looked up meanwhile.
        ipsp_ids_free(Some(c));
        assert!(ipsp_ids_lookup(c.id_flow.get()).is_none());
        // Taking it again cancels its collection.
        let c2 = ipsp_ids_insert(ids(b"left\0", b"elsewhere\0")).expect("found");
        assert!(ptr::eq(c, c2));
        assert_eq!(c.id_refcount.get(), 1);

        for p in [a, a, a, c] {
            ipsp_ids_free(Some(p));
        }
        assert_eq!(a.id_refcount.get(), 0);
    }

    #[test]
    fn sockaddr_encap_accessors_are_at_the_c_offsets() {
        let mut e = SockaddrEncap::new();
        e.set_sen_len(SENT_LEN as u8);
        e.set_sen_family(crate::sys::socket::PF_KEY);
        e.set_sen_type(SENT_IP4);
        e.set_sen_direction(IPSP_DIRECTION_OUT);
        e.set_sen_ip_src(sin([10, 77, 1, 0]).sin_addr);
        e.set_sen_ip_dst(sin([10, 77, 2, 0]).sin_addr);
        e.set_sen_proto(17);
        e.set_sen_sport(htons(500));
        e.set_sen_dport(htons(4500));
        let b = e.as_bytes();
        assert_eq!(b[0], 48);
        assert_eq!(b[1], crate::sys::socket::PF_KEY);
        assert_eq!(u16::from_ne_bytes([b[2], b[3]]), SENT_IP4);
        assert_eq!(b[4], IPSP_DIRECTION_OUT);
        assert_eq!(&b[8..12], &[10, 77, 1, 0]);
        assert_eq!(&b[12..16], &[10, 77, 2, 0]);
        assert_eq!(b[16], 17);
        assert_eq!(&b[18..20], &500u16.to_be_bytes());
        assert_eq!(&b[20..22], &4500u16.to_be_bytes());
        assert!(b[22..].iter().all(|&x| x == 0));
    }

    /// An IPv6 address from its eight 16-bit words.
    #[cfg(feature = "inet6")]
    fn a6(w: [u16; 8]) -> In6Addr {
        let mut a = [0u8; 16];
        for (i, w) in w.iter().enumerate() {
            a[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
        }
        In6Addr::new(a)
    }

    #[cfg(feature = "inet6")]
    #[test]
    fn sockaddr_encap_ipv6_accessors_are_at_the_c_offsets() {
        let mut e = SockaddrEncap::new();
        e.set_sen_len(SENT_LEN as u8);
        e.set_sen_family(crate::sys::socket::PF_KEY);
        e.set_sen_type(SENT_IP6);
        e.set_sen_ip6_direction(IPSP_DIRECTION_OUT);
        e.set_sen_ip6_src(a6([0xfd77, 1, 0, 0, 0, 0, 0, 1]));
        e.set_sen_ip6_dst(a6([0xfd77, 2, 0, 0, 0, 0, 0, 2]));
        e.set_sen_ip6_proto(17);
        e.set_sen_ip6_sport(htons(500));
        e.set_sen_ip6_dport(htons(4500));
        let b = e.as_bytes();
        assert_eq!(b[0], 48);
        assert_eq!(u16::from_ne_bytes([b[2], b[3]]), SENT_IP6);
        assert_eq!(b[4], IPSP_DIRECTION_OUT);
        assert_eq!(&b[8..12], &[0xfd, 0x77, 0, 1]);
        assert_eq!(b[23], 1);
        assert_eq!(&b[24..28], &[0xfd, 0x77, 0, 2]);
        assert_eq!(b[39], 2);
        assert_eq!(b[40], 17);
        assert_eq!(&b[42..44], &500u16.to_be_bytes());
        assert_eq!(&b[44..46], &4500u16.to_be_bytes());
        assert_eq!(e.sen_ip6_src(), a6([0xfd77, 1, 0, 0, 0, 0, 0, 1]));
        assert_eq!(e.sen_ip6_dst(), a6([0xfd77, 2, 0, 0, 0, 0, 0, 2]));
        assert_eq!(e.sen_ip6_proto(), 17);
        assert_eq!(e.sen_ip6_dport(), htons(4500));
    }

    #[cfg(feature = "inet6")]
    #[test]
    fn sockaddr_union_holds_a_sockaddr_in6() {
        let sin6 = SockaddrIn6 {
            sin6_len: size_of::<SockaddrIn6>() as u8,
            sin6_family: crate::sys::socket::AF_INET6,
            sin6_port: htons(500),
            sin6_addr: a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            sin6_scope_id: 3,
            ..SockaddrIn6::default()
        };
        let s6 = SockaddrUnion::from_sin6(&sin6);
        assert_eq!(s6.sa_family(), crate::sys::socket::AF_INET6);
        assert_eq!(usize::from(s6.sa_len()), 28);
        assert_eq!(s6.sa_bytes().len(), 28, "memcmp over sa_len bytes");
        assert_eq!(s6.sin6(), sin6);
        assert_eq!(s6.sin6_addr(), sin6.sin6_addr);
        // The same address is equal as bytes, another one is not.
        assert!(s6 == SockaddrUnion::from_sin6(&sin6));
        let mut other = sin6;
        other.sin6_addr = a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]);
        assert!(s6 != SockaddrUnion::from_sin6(&other));

        // The unspecified address, and the one of no family, count as unspecified; others not.
        assert!(!ipsp_is_unspecified(s6));
        other.sin6_addr = In6Addr::default();
        assert!(ipsp_is_unspecified(SockaddrUnion::from_sin6(&other)));
        assert!(ipsp_is_unspecified(SockaddrUnion::new()));
        assert!(!ipsp_is_unspecified(su(A)));
        assert!(ipsp_is_unspecified(su([0; 4])));

        // The TDB lookups work over an IPv6 destination: the SA is found by it and not by the
        // IPv4 one with the same SPI.
        let _g = setup();
        let su_v6 = s6;
        let t = tdb_alloc(0);
        t.tdb_spi.set(htonl(0x2000));
        t.tdb_src.set(SockaddrUnion::from_sin6(&other));
        t.tdb_dst.set(su_v6);
        t.tdb_sproto.set(IPPROTO_ESP as u8);
        puttdb(t);
        let found = gettdb(0, htonl(0x2000), &su_v6, IPPROTO_ESP as u8).expect("found by IPv6");
        assert!(core::ptr::eq(found, t));
        tdb_unref(Some(found));
        assert!(gettdb(0, htonl(0x2000), &su(A), IPPROTO_ESP as u8).is_none());
    }
}
/* </TESTS> */
