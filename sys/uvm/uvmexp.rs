/*	$OpenBSD: uvmexp.h,v 1.27 2026/03/08 17:06:10 deraadt Exp $	*/

/* <CODE> */
//! The exported VM statistics: `<uvm/uvmexp.h>` (which carries no licence block of its own
//! upstream; it was split out of `uvm_extern.h`).
//!
//! Upstream: sys/uvm/uvmexp.h @ 3ce1f3f79392
//!
//! Status: `wip`. `struct uvmexp` is complete; `uvmexp_counters` (the per-CPU counters) and
//! `atomic_load_sint` arrive with `percpu` (M5).
//!
//! ## Deviations
//! - Every field is an `AtomicI32`: the C mixes atomics (`[a]`), lock-protected fields and
//!   immutable ones; one type keeps the global sound before the locks exist, and the C's
//!   locking letters are kept in the comments.
//! - The same member list also declares [`UvmexpCopy`], the plain `#[repr(C)]` structure of
//!   `int`s that `sysctl(2)` copies out (the C copies `uvmexp` itself), and
//!   [`Uvmexp::snapshot`] fills it.

use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::sys::sysctl::SysctlPlain;

/// `struct vmmeter`.
pub const VM_METER: i32 = 1;
/// `struct loadavg`.
pub const VM_LOADAVG: i32 = 2;
/// `PSSTRINGS`.
pub const VM_PSSTRINGS: i32 = 3;
/// `struct uvmexp`.
pub const VM_UVMEXP: i32 = 4;
/// `int`.
pub const VM_SWAPENCRYPT: i32 = 5;
/// `int` - # kmem_map pages.
pub const VM_NKMEMPAGES: i32 = 6;
/// `anonmin`.
pub const VM_ANONMIN: i32 = 7;
/// `vtextmin`.
pub const VM_VTEXTMIN: i32 = 8;
/// `vnodemin`.
pub const VM_VNODEMIN: i32 = 9;
/// `maxslp`.
pub const VM_MAXSLP: i32 = 10;
/// `uspace`.
pub const VM_USPACE: i32 = 11;
/// Config for userland malloc.
pub const VM_MALLOC_CONF: i32 = 12;
/// Number of valid vm ids.
pub const VM_MAXID: i32 = 13;

macro_rules! uvmexp_struct {
    ($( $(#[$doc:meta])* $field:ident ),* $(,)?) => {
        /// `struct uvmexp`: the VM's counters and parameters. Locks, as in the C: `a` atomic,
        /// `I` immutable after creation, `K` kernel lock, `F` `uvm_lock_fpageq`, `L`
        /// `uvm_lock_pageq`, `S` `uvm_swap_data_lock`, `p` per-CPU copies, `o` page daemon only.
        pub struct Uvmexp {
            $( $(#[$doc])* pub $field: AtomicI32, )*
        }

        impl Uvmexp {
            /// All counters at zero.
            pub const fn new() -> Self {
                Self { $( $field: AtomicI32::new(0), )* }
            }

            /// `memcpy(uexp, &uvmexp, sizeof(*uexp))`: a snapshot of every member, each read
            /// atomically, as `uvmexp_read` starts with.
            pub fn snapshot(&self) -> UvmexpCopy {
                UvmexpCopy { $( $field: self.$field.load(Ordering::Relaxed), )* }
            }
        }

        /// A plain copy of `struct uvmexp`, the C's layout (`#[repr(C)]`, every member an
        /// `int`): what `uvmexp_read` fills and `sysctl({CTL_VM, VM_UVMEXP})` copies out.
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct UvmexpCopy {
            $( $(#[$doc])* pub $field: i32, )*
        }
    };
}

uvmexp_struct! {
    // vm_page constants
    /// Size of a page (`PAGE_SIZE`): must be power of 2.
    pagesize,
    /// Page mask.
    pagemask,
    /// Page shift.
    pageshift,

    // vm_page counters
    /// \[I\] number of pages we manage.
    npages,
    /// \[aF\] number of free pages.
    free,
    /// \[aL\] # of active pages.
    active,
    /// \[aL\] # of pages that we free'd but may want back.
    inactive,
    /// \[a\] # number of pages in the process of being paged out.
    paging,
    /// \[a\] # number of wired pages.
    wired,

    /// \[aF\] number of zero'd pages.
    zeropages,
    /// \[I\] # of pages reserved for pagedaemon.
    reserve_pagedaemon,
    /// \[I\] # of pages reserved for kernel.
    reserve_kernel,
    /// \[a\] # of pages in per-CPU caches.
    percpucaches,
    /// XXX # of pages used by vnode page cache.
    vnodepages,
    /// XXX # of pages used by vtext vnodes.
    vtextpages,

    // pageout params
    /// \[I\] min number of free pages.
    freemin,
    /// \[I\] target number of free pages.
    freetarg,
    /// Target number of inactive pages.
    inactarg,
    /// \[I\] max number of wired pages.
    wiredmax,
    /// Min threshold for anon pages.
    anonmin,
    /// Min threshold for vtext pages.
    vtextmin,
    /// Min threshold for vnode pages.
    vnodemin,
    /// Min percent anon pages.
    anonminpct,
    /// Min percent vtext pages.
    vtextminpct,
    /// Min percent vnode pages.
    vnodeminpct,

    // swap
    /// \[aS\] number of configured swap devices in system.
    nswapdev,
    /// \[aS\] number of PAGE_SIZE'ed swap pages.
    swpages,
    /// \[aS\] number of swap pages in use.
    swpginuse,
    /// \[a\] number of swap pages in use, not also in RAM.
    swpgonly,
    /// \[a\] number of swap pages moved from disk to RAM.
    nswget,
    /// XXX number total of anon's in system.
    nanon,
    /// Formerly nanonneeded.
    unused05,
    /// Formerly nfreeanon.
    unused06,

    // stat counters
    /// \[p\] page fault count.
    faults,
    /// \[a\] trap count.
    traps,
    /// \[a\] interrupt count.
    intrs,
    /// Context switch count.
    swtch,
    /// \[a\] software interrupt count.
    softs,
    /// \[a\] system calls.
    syscalls,
    /// \[p\] pagein operation count (pageouts are in pdpageouts below).
    pageins,
    /// \[a\] # of pagealloc from per-CPU cache.
    pcphit,
    /// \[a\] # of times a per-CPU cache was empty.
    pcpmiss,
    /// Pages swapped in.
    pgswapin,
    /// \[a\] pages swapped out.
    pgswapout,
    /// Forks.
    forks,
    /// Forks where parent waits.
    forks_ppwait,
    /// Forks where vmspace is shared.
    forks_sharevm,
    /// \[a\] pagealloc where zero wanted and zero was available.
    pga_zerohit,
    /// \[a\] pagealloc where zero wanted and zero not available.
    pga_zeromiss,
    /// Formerly zeroaborts.
    unused09,

    // fault subcounters
    /// \[p\] # of times fault was out of ram.
    fltnoram,
    /// \[p\] # of times fault was out of anons.
    fltnoanon,
    /// \[p\] # of times fault was out of amap chunks.
    fltnoamap,
    /// \[p\] # of times fault had to wait on a page.
    fltpgwait,
    /// \[p\] # of times fault found a released page.
    fltpgrele,
    /// \[p\] # of times fault relock is a success.
    fltrelck,
    /// \[p\] # of times fault relock failed.
    fltnorelck,
    /// \[p\] # of times fault gets anon page.
    fltanget,
    /// \[p\] # of times fault retrys an anon get.
    fltanretry,
    /// \[p\] # of times fault clears "needs copy".
    fltamcopy,
    /// \[p\] # of times fault maps a neighbor anon page.
    fltnamap,
    /// \[p\] # of times fault maps a neighbor obj page.
    fltnomap,
    /// \[p\] # of times fault does a locked pgo_get.
    fltlget,
    /// \[p\] # of times fault does an unlocked get.
    fltget,
    /// \[p\] # of times fault anon (case 1a).
    flt_anon,
    /// \[p\] # of times fault anon cow (case 1b).
    flt_acow,
    /// \[p\] # of times fault is on object page (2a).
    flt_obj,
    /// \[p\] # of times fault promotes with copy (2b).
    flt_prcopy,
    /// \[p\] # of times fault promotes with zerofill (2b).
    flt_przero,
    /// \[p\] # of times fault upgrade is a success.
    fltup,
    /// \[p\] # of times fault upgrade failed.
    fltnoup,

    // daemon counters
    /// \[ao\] # of times daemon woke up.
    pdwoke,
    /// \[ao\] # of times daemon scanned for free pages.
    pdrevs,
    /// \[o\] # of times daemon called for swapout.
    pdswout,
    /// \[ao\] # of pages daemon freed since boot.
    pdfreed,
    /// \[ao\] # of pages daemon scanned since boot.
    pdscans,
    /// \[ao\] # of anonymous pages scanned by daemon.
    pdanscan,
    /// \[ao\] # of object pages scanned by daemon.
    pdobscan,
    /// \[ao\] # of pages daemon reactivated since boot.
    pdreact,
    /// \[ao\] # of times daemon found a busy page.
    pdbusy,
    /// \[ao\] # of times daemon started a pageout.
    pdpageouts,
    /// \[ao\] # of times daemon got a pending pagout.
    pdpending,
    /// \[ao\] # of pages daemon deactivates.
    pddeact,
    /// \[ao\] # of pages delayed because swap crypt busy.
    swpskip,

    /// \[a\] FPU context switches.
    fpswtch,
    /// \[a\] number of kernel map entries.
    kmapent,
}

// SAFETY: `#[repr(C)]` and nothing but `i32`s: no padding, every bit pattern valid.
unsafe impl SysctlPlain for UvmexpCopy {}

impl Default for Uvmexp {
    fn default() -> Self {
        Self::new()
    }
}

/// `enum uvm_exp_counters`: the per-CPU counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum UvmExpCounters {
    /// Page fault count.
    Faults,
    /// Pagein operation count.
    Pageins,
    /// Number of times fault was out of ram.
    FltNoram,
    /// Number of times fault was out of anons.
    FltNoanon,
    /// Number of times fault was out of amap chunks.
    FltNoamap,
    /// Number of times fault had to wait on a page.
    FltPgwait,
    /// Number of times fault found a released page.
    FltPgrele,
    /// Number of times fault relock is a success.
    FltRelck,
    /// Number of times fault relock failed.
    FltNorelck,
    /// Number of times fault gets anon page.
    FltAnget,
    /// Number of times fault retrys an anon get.
    FltAnretry,
    /// Number of times fault clears "needs copy".
    FltAmcopy,
    /// Number of times fault maps a neighbor anon page.
    FltNamap,
    /// Number of times fault maps a neighbor obj page.
    FltNomap,
    /// Number of times fault does a locked pgo_get.
    FltLget,
    /// Number of times fault does an unlocked get.
    FltGet,
    /// Number of times fault anon (case 1a).
    FltAnon,
    /// Number of times fault anon cow (case 1b).
    FltAcow,
    /// Number of times fault is on object page (2a).
    FltObj,
    /// Number of times fault promotes with copy (2b).
    FltPrcopy,
    /// Number of times fault promotes with zerofill (2b).
    FltPrzero,
    /// Number of times fault upgrade is a success.
    FltUp,
    /// Number of times fault upgrade failed.
    FltNoup,
    /// The number of counters.
    ExpNcounters,
}

/// `uvmexp_counters`: the UVM event counters. The C keeps one array per CPU
/// (`counters_alloc_ncpus`); here every CPU bumps the same relaxed atomics (`docs/C_TO_RUST.md`,
/// the `struct cpumem *` counters row), which counts the same events without a lock.
pub static UVMEXP_COUNTERS: [AtomicU64; UvmExpCounters::ExpNcounters as usize] =
    [const { AtomicU64::new(0) }; UvmExpCounters::ExpNcounters as usize];

/// `counters_inc(uvmexp_counters, c)`: bumps counter `c`.
#[inline]
pub fn counters_inc(c: UvmExpCounters) {
    UVMEXP_COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `counters_read(uvmexp_counters, c)`: the value of counter `c`.
pub fn counters_read(c: UvmExpCounters) -> u64 {
    UVMEXP_COUNTERS[c as usize].load(Ordering::Relaxed)
}

const _: () = {
    // `struct uvmexp` is 86 `int`s.
    assert!(core::mem::size_of::<UvmexpCopy>() == 86 * 4);
};
/* </CODE> */
