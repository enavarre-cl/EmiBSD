/*	$OpenBSD: malloc.h,v 1.127 2025/02/05 18:29:17 mvs Exp $	*/
/*	$NetBSD: malloc.h,v 1.39 1998/07/12 19:52:01 augustss Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1987, 1993
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
 *	@(#)malloc.h	8.5 (Berkeley) 5/3/95
 */
/* </LICENSES> */

//! `<sys/malloc.h>`: the kernel memory allocator's flags, types and bookkeeping structures.
//!
//! Upstream: sys/sys/malloc.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the flags, the memory types with their names, the
//! allocator constants and the `kmemstats`/`kmemusage`/`kmembuckets` structures; the
//! `CTL_KERN_MALLOC_NAMES` sysctl table and `poison_*` (`subr_poison.c`) come later.
//!
//! ## Deviations
//! - `kmemusage`'s union of `freecnt`/`pagecnt` is one `u16` with two accessor pairs.

use core::cell::Cell;

use crate::kern::kern_malloc::KfList;
use crate::sys::param::PAGE_SIZE;
use crate::sys::queue::XsimpleqHead;

/// `KERN_MALLOC_BUCKETS`.
pub const KERN_MALLOC_BUCKETS: i32 = 1;
/// `KERN_MALLOC_BUCKET`.
pub const KERN_MALLOC_BUCKET: i32 = 2;
/// `KERN_MALLOC_KMEMNAMES`.
pub const KERN_MALLOC_KMEMNAMES: i32 = 3;
/// `KERN_MALLOC_KMEMSTATS`.
pub const KERN_MALLOC_KMEMSTATS: i32 = 4;
/// `KERN_MALLOC_MAXID`.
pub const KERN_MALLOC_MAXID: i32 = 5;

// flags to malloc

/// `M_WAITOK`: may sleep.
pub const M_WAITOK: i32 = 0x0001;
/// `M_NOWAIT`: must not sleep.
pub const M_NOWAIT: i32 = 0x0002;
/// `M_CANFAIL`: fail instead of panicking when the request cannot be met.
pub const M_CANFAIL: i32 = 0x0004;
/// `M_ZERO`: zero the memory.
pub const M_ZERO: i32 = 0x0008;

// Types of memory to be allocated

/// `M_FREE`: should be on free list.
pub const M_FREE: i32 = 0;
/// `M_DEVBUF`: device driver memory.
pub const M_DEVBUF: i32 = 2;
/// `M_PCB`: protocol control blocks.
pub const M_PCB: i32 = 4;
/// `M_RTABLE`: routing tables.
pub const M_RTABLE: i32 = 5;
/// `M_PF`: packet filter structures.
pub const M_PF: i32 = 6;
/// `M_IFADDR`: interface addresses.
pub const M_IFADDR: i32 = 9;
/// `M_IFGROUP`: interface groups.
pub const M_IFGROUP: i32 = 10;
/// `M_SYSCTL`: sysctl persistent buffers.
pub const M_SYSCTL: i32 = 11;
/// `M_COUNTERS`: per-CPU counters via counters_alloc(9).
pub const M_COUNTERS: i32 = 12;
/// `M_IOCTLOPS`: ioctl data buffers.
pub const M_IOCTLOPS: i32 = 14;
/// `M_IOV`: large IOVs.
pub const M_IOV: i32 = 19;
/// `M_MOUNT`: VFS mount structs.
pub const M_MOUNT: i32 = 20;
/// `M_NFSREQ`: NFS request headers.
pub const M_NFSREQ: i32 = 22;
/// `M_NFSMNT`: NFS mount structures.
pub const M_NFSMNT: i32 = 23;
/// `M_LOG`: messages in kernel log stash.
pub const M_LOG: i32 = 24;
/// `M_VNODE`: Dynamically allocated vnodes.
pub const M_VNODE: i32 = 25;
/// `M_DQUOT`: UFS quota entries.
pub const M_DQUOT: i32 = 27;
/// `M_UFSMNT`: UFS mount structures.
pub const M_UFSMNT: i32 = 28;
/// `M_SHM`: SVID compatible shared memory segments.
pub const M_SHM: i32 = 29;
/// `M_VMMAP`: VM map structures.
pub const M_VMMAP: i32 = 30;
/// `M_SEM`: SVID compatible semaphores.
pub const M_SEM: i32 = 31;
/// `M_DIRHASH`: UFS directory hash structures.
pub const M_DIRHASH: i32 = 32;
/// `M_ACPI`: ACPI structures.
pub const M_ACPI: i32 = 33;
/// `M_VMPMAP`: VM pmap data.
pub const M_VMPMAP: i32 = 34;
/// `M_FILEDESC`: open file descriptor tables.
pub const M_FILEDESC: i32 = 39;
/// `M_SIGIO`: sigio structures.
pub const M_SIGIO: i32 = 40;
/// `M_PROC`: proc structures.
pub const M_PROC: i32 = 41;
/// `M_SUBPROC`: proc sub-structures.
pub const M_SUBPROC: i32 = 42;
/// `M_MFSNODE`: MFS vnode private part.
pub const M_MFSNODE: i32 = 46;
/// `M_NETADDR`: export host address structures.
pub const M_NETADDR: i32 = 49;
/// `M_NFSSVC`: NFS server structures.
pub const M_NFSSVC: i32 = 50;
/// `M_NFSD`: NFS server daemon structures.
pub const M_NFSD: i32 = 52;
/// `M_IPMOPTS`: internet multicast options.
pub const M_IPMOPTS: i32 = 53;
/// `M_IPMADDR`: internet multicast addresses.
pub const M_IPMADDR: i32 = 54;
/// `M_IFMADDR`: link-level multicast addresses.
pub const M_IFMADDR: i32 = 55;
/// `M_MRTABLE`: multicast routing tables.
pub const M_MRTABLE: i32 = 56;
/// `M_ISOFSMNT`: ISOFS mount structures.
pub const M_ISOFSMNT: i32 = 57;
/// `M_ISOFSNODE`: ISOFS vnode private part.
pub const M_ISOFSNODE: i32 = 58;
/// `M_MSDOSFSMNT`: MSDOS FS mount structures.
pub const M_MSDOSFSMNT: i32 = 59;
/// `M_MSDOSFSFAT`: MSDOS FS FAT tables.
pub const M_MSDOSFSFAT: i32 = 60;
/// `M_MSDOSFSNODE`: MSDOS FS vnode private part.
pub const M_MSDOSFSNODE: i32 = 61;
/// `M_TTYS`: allocated tty structures.
pub const M_TTYS: i32 = 62;
/// `M_EXEC`: argument lists & other mem used by exec.
pub const M_EXEC: i32 = 63;
/// `M_MISCFSMNT`: miscellaneous FS mount structures.
pub const M_MISCFSMNT: i32 = 64;
/// `M_FUSEFS`: FUSE FS mount structures.
pub const M_FUSEFS: i32 = 65;
/// `M_PFKEY`: pfkey data.
pub const M_PFKEY: i32 = 74;
/// `M_TDB`: transforms database.
pub const M_TDB: i32 = 75;
/// `M_XDATA`: IPsec data.
pub const M_XDATA: i32 = 76;
/// `M_PAGEDEP`: file page dependencies.
pub const M_PAGEDEP: i32 = 78;
/// `M_INODEDEP`: inode dependencies.
pub const M_INODEDEP: i32 = 79;
/// `M_NEWBLK`: new block allocation.
pub const M_NEWBLK: i32 = 80;
/// `M_INDIRDEP`: indirect block dependencies.
pub const M_INDIRDEP: i32 = 83;
/// `M_VMSWAP`: VM swap structures.
pub const M_VMSWAP: i32 = 92;
/// `M_UVMAMAP`: UVM amap and related.
pub const M_UVMAMAP: i32 = 98;
/// `M_UVMAOBJ`: UVM aobj and related.
pub const M_UVMAOBJ: i32 = 99;
/// `M_PINSYSCALL`: pinsyscall.
pub const M_PINSYSCALL: i32 = 100;
/// `M_USB`: USB general.
pub const M_USB: i32 = 101;
/// `M_USBDEV`: USB device driver.
pub const M_USBDEV: i32 = 102;
/// `M_USBHC`: USB host controller.
pub const M_USBHC: i32 = 103;
/// `M_WITNESS`: witness(4) memory.
pub const M_WITNESS: i32 = 104;
/// `M_MEMDESC`: memory range.
pub const M_MEMDESC: i32 = 105;
/// `M_CRYPTO_DATA`: crypto(9) data buffers.
pub const M_CRYPTO_DATA: i32 = 108;
/// `M_CREDENTIALS`: ipsec(4) related credentials.
pub const M_CREDENTIALS: i32 = 110;
/// `M_IP6OPT`: IPv6 options.
pub const M_IP6OPT: i32 = 123;
/// `M_IP6NDP`: IPv6 Neighbor Discovery structures.
pub const M_IP6NDP: i32 = 124;
/// `M_TEMP`: miscellaneous temporary data buffers.
pub const M_TEMP: i32 = 127;
/// `M_NTFSMNT`: NTFS mount structures.
pub const M_NTFSMNT: i32 = 128;
/// `M_NTFSNTNODE`: NTFS ntnode information.
pub const M_NTFSNTNODE: i32 = 129;
/// `M_NTFSFNODE`: NTFS fnode information.
pub const M_NTFSFNODE: i32 = 130;
/// `M_NTFSDIR`: NTFS directory buffers.
pub const M_NTFSDIR: i32 = 131;
/// `M_NTFSNTHASH`: NTFS ntnode hash tables.
pub const M_NTFSNTHASH: i32 = 132;
/// `M_NTFSNTVATTR`: NTFS file attribute information.
pub const M_NTFSNTVATTR: i32 = 133;
/// `M_NTFSRDATA`: NTFS resident data.
pub const M_NTFSRDATA: i32 = 134;
/// `M_NTFSDECOMP`: NTFS decompression temporary storage.
pub const M_NTFSDECOMP: i32 = 135;
/// `M_NTFSRUN`: NTFS vrun storage.
pub const M_NTFSRUN: i32 = 136;
/// `M_KEVENT`: kqueue(2) data structures.
pub const M_KEVENT: i32 = 137;
/// `M_SYNCACHE`: SYN cache hash array.
pub const M_SYNCACHE: i32 = 139;
/// `M_UDFMOUNT`: UDF mount structures.
pub const M_UDFMOUNT: i32 = 140;
/// `M_UDFFENTRY`: UDF file entries.
pub const M_UDFFENTRY: i32 = 141;
/// `M_UDFFID`: UDF file IDs.
pub const M_UDFFID: i32 = 142;
/// `M_AGP`: AGP memory.
pub const M_AGP: i32 = 144;
/// `M_DRM`: Direct Rendering Manager.
pub const M_DRM: i32 = 145;
/// `M_LAST`: Must be last type + 1.
pub const M_LAST: i32 = 146;

/// `INITKMEMNAMES`: the name of each memory type, `None` for the unused numbers.
pub const INITKMEMNAMES: [Option<&str>; M_LAST as usize] = [
    Some("free"),           // 0
    None,                   // 1
    Some("devbuf"),         // 2
    None,                   // 3
    Some("pcb"),            // 4
    Some("rtable"),         // 5
    Some("pf"),             // 6
    None,                   // 7
    None,                   // 8
    Some("ifaddr"),         // 9
    Some("ifgroup"),        // 10
    Some("sysctl"),         // 11
    Some("counters"),       // 12
    None,                   // 13
    Some("ioctlops"),       // 14
    None,                   // 15
    None,                   // 16
    None,                   // 17
    None,                   // 18
    Some("iov"),            // 19
    Some("mount"),          // 20
    None,                   // 21
    Some("NFS req"),        // 22
    Some("NFS mount"),      // 23
    Some("log"),            // 24
    Some("vnodes"),         // 25
    None,                   // 26
    Some("UFS quota"),      // 27
    Some("UFS mount"),      // 28
    Some("shm"),            // 29
    Some("VM map"),         // 30
    Some("sem"),            // 31
    Some("dirhash"),        // 32
    Some("ACPI"),           // 33
    Some("VM pmap"),        // 34
    None,                   // 35
    None,                   // 36
    None,                   // 37
    None,                   // 38
    Some("file desc"),      // 39
    Some("sigio"),          // 40
    Some("proc"),           // 41
    Some("subproc"),        // 42
    None,                   // 43
    None,                   // 44
    None,                   // 45
    Some("MFS node"),       // 46
    None,                   // 47
    None,                   // 48
    Some("Export Host"),    // 49
    Some("NFS srvsock"),    // 50
    None,                   // 51
    Some("NFS daemon"),     // 52
    Some("ip_moptions"),    // 53
    Some("in_multi"),       // 54
    Some("ether_multi"),    // 55
    Some("mrt"),            // 56
    Some("ISOFS mount"),    // 57
    Some("ISOFS node"),     // 58
    Some("MSDOSFS mount"),  // 59
    Some("MSDOSFS fat"),    // 60
    Some("MSDOSFS node"),   // 61
    Some("ttys"),           // 62
    Some("exec"),           // 63
    Some("miscfs mount"),   // 64
    Some("fusefs mount"),   // 65
    None,                   // 66
    None,                   // 67
    None,                   // 68
    None,                   // 69
    None,                   // 70
    None,                   // 71
    None,                   // 72
    None,                   // 73
    Some("pfkey data"),     // 74
    Some("tdb"),            // 75
    Some("xform_data"),     // 76
    None,                   // 77
    Some("pagedep"),        // 78
    Some("inodedep"),       // 79
    Some("newblk"),         // 80
    None,                   // 81
    None,                   // 82
    Some("indirdep"),       // 83
    None,                   // 84
    None,                   // 85
    None,                   // 86
    None,                   // 87
    None,                   // 88
    None,                   // 89
    None,                   // 90
    None,                   // 91
    Some("VM swap"),        // 92
    None,                   // 93
    None,                   // 94
    None,                   // 95
    None,                   // 96
    None,                   // 97
    Some("UVM amap"),       // 98
    Some("UVM aobj"),       // 99
    Some("pinsyscall"),     // 100
    Some("USB"),            // 101
    Some("USB device"),     // 102
    Some("USB HC"),         // 103
    Some("witness"),        // 104
    Some("memdesc"),        // 105
    None,                   // 106
    None,                   // 107
    Some("crypto data"),    // 108
    None,                   // 109
    Some("IPsec creds"),    // 110
    None,                   // 111
    None,                   // 112
    None,                   // 113
    None,                   // 114
    None,                   // 115
    None,                   // 116
    None,                   // 117
    None,                   // 118
    None,                   // 119
    None,                   // 120
    None,                   // 121
    None,                   // 122
    Some("ip6_options"),    // 123
    Some("NDP"),            // 124
    None,                   // 125
    None,                   // 126
    Some("temp"),           // 127
    Some("NTFS mount"),     // 128
    Some("NTFS node"),      // 129
    Some("NTFS fnode"),     // 130
    Some("NTFS dir"),       // 131
    Some("NTFS hash"),      // 132
    Some("NTFS attr"),      // 133
    Some("NTFS data"),      // 134
    Some("NTFS decomp"),    // 135
    Some("NTFS vrun"),      // 136
    Some("kqueue"),         // 137
    None,                   // 138
    Some("SYN cache"),      // 139
    Some("UDF mount"),      // 140
    Some("UDF file entry"), // 141
    Some("UDF file id"),    // 142
    None,                   // 143
    Some("AGP Memory"),     // 144
    Some("DRM"),            // 145
];

/// `struct kmemstats`: the statistics of one memory type (`KMEMSTATS`).
pub struct Kmemstats {
    /// # of packets of this type currently in use.
    pub ks_inuse: Cell<i64>,
    /// Total packets of this type ever allocated.
    pub ks_calls: Cell<i64>,
    /// Total memory held in bytes.
    pub ks_memuse: Cell<i64>,
    /// Number of times blocked for hitting limit.
    pub ks_limblocks: Cell<u16>,
    /// Maximum number ever used.
    pub ks_maxused: Cell<i64>,
    /// Most that are allowed to exist.
    pub ks_limit: Cell<i64>,
    /// Sizes of this thing that are allocated.
    pub ks_size: Cell<i64>,
    /// Spare.
    pub ks_spare: Cell<i64>,
}

// SAFETY: guarded by `malloc_mtx` (M5); the boot CPU is alone until then.
unsafe impl Sync for Kmemstats {}

impl Kmemstats {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            ks_inuse: Cell::new(0),
            ks_calls: Cell::new(0),
            ks_memuse: Cell::new(0),
            ks_limblocks: Cell::new(0),
            ks_maxused: Cell::new(0),
            ks_limit: Cell::new(0),
            ks_size: Cell::new(0),
            ks_spare: Cell::new(0),
        }
    }
}

impl Default for Kmemstats {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct kmemusage`: array of descriptors that describe the contents of each page.
pub struct Kmemusage {
    /// Bucket index.
    pub ku_indx: Cell<i16>,
    /// `ku_un`: for small allocations, free pieces in page (`freecnt`); for large
    /// allocations, pages alloced (`pagecnt`).
    ku_un: Cell<u16>,
}

// SAFETY: as for `Kmemstats`.
unsafe impl Sync for Kmemusage {}

impl Kmemusage {
    /// An unused page.
    pub const fn new() -> Self {
        Self {
            ku_indx: Cell::new(0),
            ku_un: Cell::new(0),
        }
    }

    /// `ku_freecnt`.
    pub fn ku_freecnt(&self) -> u16 {
        self.ku_un.get()
    }

    /// Sets `ku_freecnt`.
    pub fn set_ku_freecnt(&self, n: u16) {
        self.ku_un.set(n);
    }

    /// `ku_pagecnt`.
    pub fn ku_pagecnt(&self) -> u16 {
        self.ku_un.get()
    }

    /// Sets `ku_pagecnt`.
    pub fn set_ku_pagecnt(&self, n: u16) {
        self.ku_un.set(n);
    }
}

impl Default for Kmemusage {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct kmembuckets`: set of buckets for each size of memory block that is retained.
pub struct Kmembuckets {
    /// List of free blocks.
    pub kb_freelist: XsimpleqHead<KfList>,
    /// Total calls to allocate this size.
    pub kb_calls: Cell<u64>,
    /// Total number of blocks allocated.
    pub kb_total: Cell<u64>,
    /// # of free elements in this bucket.
    pub kb_totalfree: Cell<u64>,
    /// # of elements in this sized allocation.
    pub kb_elmpercl: Cell<u64>,
    /// High water mark.
    pub kb_highwat: Cell<u64>,
    /// Over high water mark and could free.
    pub kb_couldfree: Cell<u64>,
}

// SAFETY: as for `Kmemstats`.
unsafe impl Sync for Kmembuckets {}

impl Kmembuckets {
    /// An empty bucket.
    pub const fn new() -> Self {
        Self {
            kb_freelist: XsimpleqHead::new(),
            kb_calls: Cell::new(0),
            kb_total: Cell::new(0),
            kb_totalfree: Cell::new(0),
            kb_elmpercl: Cell::new(0),
            kb_highwat: Cell::new(0),
            kb_couldfree: Cell::new(0),
        }
    }
}

impl Default for Kmembuckets {
    fn default() -> Self {
        Self::new()
    }
}

// Constants for setting the parameters of the kernel memory allocator.
//
// 2 ** MINBUCKET is the smallest unit of memory that will be allocated. It must be at least
// large enough to hold a pointer.
//
// Units of memory less or equal to MAXALLOCSAVE will permanently allocate physical memory;
// requests for these size pieces of memory are quite fast. Allocations greater than
// MAXALLOCSAVE must always allocate and free physical memory; requests for these size
// allocations should be done infrequently as they will be slow.
//
// Constraints: PAGE_SIZE <= MAXALLOCSAVE <= 2 ** (MINBUCKET + 14), and MAXALLOCSIZE must be a
// power of two.

/// `MINBUCKET`: 4 => min allocation of 16 bytes.
pub const MINBUCKET: u32 = 4;

/// `MINALLOCSIZE`.
pub const MINALLOCSIZE: usize = 1 << MINBUCKET;
/// `MAXALLOCSAVE`.
pub const MAXALLOCSAVE: usize = 2 * PAGE_SIZE;
/// `MALLOC_MAX`.
pub const MALLOC_MAX: usize = 65535 * PAGE_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_numbers() {
        assert_eq!(INITKMEMNAMES[M_FREE as usize], Some("free"));
        assert_eq!(INITKMEMNAMES[M_TEMP as usize], Some("temp"));
        assert_eq!(INITKMEMNAMES[M_DRM as usize], Some("DRM"));
        assert_eq!(INITKMEMNAMES[1], None);
        assert_eq!(INITKMEMNAMES[143], None);
        assert_eq!(INITKMEMNAMES.len(), M_LAST as usize);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/malloc.h");
        let ours: &[(&str, i64)] = &[
            ("M_WAITOK", i64::from(M_WAITOK)),
            ("M_NOWAIT", i64::from(M_NOWAIT)),
            ("M_CANFAIL", i64::from(M_CANFAIL)),
            ("M_ZERO", i64::from(M_ZERO)),
            ("M_DEVBUF", i64::from(M_DEVBUF)),
            ("M_TEMP", i64::from(M_TEMP)),
            ("M_VMPMAP", i64::from(M_VMPMAP)),
            ("M_LAST", i64::from(M_LAST)),
            ("MINBUCKET", i64::from(MINBUCKET)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
