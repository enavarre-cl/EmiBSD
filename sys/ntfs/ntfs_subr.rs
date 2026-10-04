/*	$OpenBSD: ntfs_subr.h,v 1.12 2024/05/13 01:15:53 jsg Exp $	*/
/*	$NetBSD: ntfs_subr.h,v 1.1 2002/12/23 17:38:33 jdolecek Exp $	*/
/*	$OpenBSD: ntfs_subr.c,v 1.53 2025/01/13 13:58:41 claudio Exp $	*/
/*	$NetBSD: ntfs_subr.c,v 1.4 2003/04/10 21:37:32 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_subr.h,v 1.4 1999/05/12 09:43:02 semenu Exp
 */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko (semenu@FreeBSD.org)
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_subr.c,v 1.4 1999/05/12 09:43:01 semenu Exp
 */
/* </LICENSES> */

//! NTFS subroutines: the in-core attribute (`struct ntvattr`), the ntnode life cycle (lookup,
//! reference, lock, load from the MFT, release), fnodes, run lists, the readers of attribute
//! data (resident, in runs, compressed), directory lookup and directory reading over the
//! `$I30` index, fixups, times, and the upper-case table.
//!
//! Upstream: sys/ntfs/ntfs_subr.h @ 3ce1f3f79392, sys/ntfs/ntfs_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module, as `.h`/`.c` pairs do (`siphash.rs`).
//! - `struct ntvattr` is [`Ntvattr`]: `malloc(M_NTFSNTVATTR)`ed by `ntfs_attrtontvattr`,
//!   written once and handed around as `&'static` until `ntfs_freentvattr`; `va_vflag`,
//!   `va_vp` and `va_ip` are `Cell`s, the rest is set at construction. The union `va_d` is
//!   [`VaD`], an enumeration tagged by `NTFS_AF_INRUN` as the C's readers are: the run arrays
//!   (`va_vruncn`, `va_vruncl`, `va_vruncnt`) or the resident data (`va_datap`, which
//!   `va_a_name`, `va_a_iroot` and `va_a_ialloc` cast); [`Ntvattr::va_a_name`] and
//!   [`Ntvattr::va_a_iroot`] copy the structures out of the data ([`Packed::read`]).
//! - The functions that read data take [`Rdata`]: the C's `void *rdata` (`Mem`) or its
//!   `struct uio *uio` (`Uio`), one of which is NULL in every call, plus the offset in `rdata`
//!   where the C's advanced `data` pointer stands. Copies into `Mem` are bounded by the
//!   slice; copies out to a `uio` go through a bounce buffer (`uiomove` wants a writable
//!   source), and the C's byte-by-byte `uiomove("", 1, uio)` of a hole moves zeros 256 bytes
//!   at a time.
//! - `ntfs_findvattr` returns `Ok(true)`/`Ok(false)` for the C's 0/-1 ("not here, look in
//!   the attribute list"); `ntfs_ntvattrget`, `ntfs_ntlookup`, `ntfs_fget` and `ntfs_filesize`
//!   return what the C stores through their out-parameters; `ntfs_ntreaddir` returns the
//!   offset of the entry in `f_dirblbuf` (the C's `*riepp` points there), `None` for NULL.
//!   `ntfs_ntvattrrele` returns nothing (the C's is always 0).
//! - The `char *` attribute names are byte slices (`Option<&[u8]>` for NULL); the names
//!   `ntfs_ntlookupattr` allocates are [`AttrNameBuf`]. `ntfs_uastricmp`/`ntfs_uastrcmp` take
//!   the Unicode name as a `wchar` slice (its length is `ustrlen`).
//! - The temporary buffers (`malloc(M_TEMP)`, `M_NTFSDECOMP`) are [`KBuf`]s, zeroed when made
//!   (a Rust slice must be initialised) and freed when dropped, on every path. `f_dirblbuf`,
//!   the resident data and the upper-case table are zeroed too (`M_ZERO`).
//! - The on-disk structures are read with [`Packed::read`]: a structure past the end of its
//!   buffer reads zeros, where the C reads past the buffer. Where that would make the C loop
//!   for ever on a damaged volume, the loop stops instead: an index entry or an attribute list
//!   entry of length 0 ends its scan, an attribute list entry longer than what is left ends
//!   the list (the C dereferences NULL next), an attribute record of length 0 or past the
//!   end of its MFT record is `EINVAL` (`ntfs_loadntnode`), and the `$AttrDef` name copy
//!   stops at its 64 characters (`ntfs_mountfs`).
//! - `ntfs_runtovrun`'s `(u_int32_t)run[off] << (i << 3)` for a length field of more than 4
//!   bytes shifts by its count modulo 32, as the machines do (`wrapping_shl`; the C is
//!   undefined there). The run list is read from the bytes to the end of the MFT record; a
//!   byte past them reads as 0 (the end of the list). `run[off + sz - 1]` of a run with no
//!   offset bytes (a hole) reads the byte before, as the C does. An empty run list allocates
//!   nothing.
//! - `ntfs_readntvattr_plain` releases the buffer when `uiomove` fails (the C breaks out
//!   with it still busy).
//! - `ntfs_toupper_tab` is an `AtomicPtr` and its use count an `AtomicI32`, both changed under
//!   `ntfs_toupper_lock` as in C; `NTFS_TOUPPER` reads the table without the lock, as in C.
//! - `ntfs_parserun` and `ntfs_runtocn` are under `#if UNUSED_CODE` (never compiled): left
//!   out. `NTFS_DEBUG` is off: `ntfs_debug` and the `DPRINTF`s are left out.
//! - `ntfs_ntput`'s negative use count check is under feature `diagnostic` (`DIAGNOSTIC`), as
//!   is `ntfs_toupper_unuse`'s; `KASSERT` is `kassert!`.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_subr::{vput, vref, vrele};
use crate::ntfs::ntfs::{
    Attr, AttrAttrlist, AttrIndexalloc, AttrIndexentry, AttrIndexroot, AttrName, Cn, FTONT,
    Filerec, Fixuphdr, LOADED_NTNODE_HI, NTFS_A_ATTRLIST, NTFS_A_DATA, NTFS_A_INDX,
    NTFS_A_INDXBITMAP, NTFS_A_INDXROOT, NTFS_AF_INRUN, NTFS_BOOTINO, NTFS_FFLAG_DIR,
    NTFS_FILEMAGIC, NTFS_IEFLAG_LAST, NTFS_IEFLAG_SUBNODE, NTFS_INDXMAGIC, NTFS_IRFLAG_INDXALLOC,
    NTFS_MAXATTRNAME, NTFS_MFTINO, NTFS_SYSNODESNUM, NTFS_UPCASEINO, Ntfsino, Ntfsmount, Packed,
    VTOF, VTONT, Wchar, wchar_at,
};
use crate::ntfs::ntfs_compr::{NTFS_COMPUNIT_CL, ntfs_uncompunit};
use crate::ntfs::ntfs_ihash::{ntfs_nthashins, ntfs_nthashlookup, ntfs_nthashrem};
use crate::ntfs::ntfs_inode::{
    AttrNameBuf, FN_AATTRNAME, FN_PRELOADED, FN_VALID, Fnode, FnodeList, IN_LOADED, Ntnode,
};
use crate::ntfs::ntfs_vfsops::{VG_DONTLOADIN, VG_DONTVALIDFN, VG_EXT, ntfs_vgetex};
use crate::ntfs::ntfsmount::{NTFS_MFLAG_ALLNAMES, NTFS_MFLAG_CASEINS};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::malloc::{
    M_NTFSDIR, M_NTFSFNODE, M_NTFSNTNODE, M_NTFSNTVATTR, M_NTFSRDATA, M_NTFSRUN, M_TEMP, M_WAITOK,
    M_ZERO,
};
use crate::sys::mount::{Mount, VFS_VGET};
use crate::sys::namei::{CREATE, Componentname, ISLASTCN, RENAME};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::time::Timespec;
use crate::sys::types::{Daddr, Off};
use crate::sys::uio::Uio;
use crate::sys::vnode::{VDIR, VREG, Vnode};

/// `VA_LOADED`.
pub const VA_LOADED: u32 = 0x0001;
/// `VA_PRELOADED`.
pub const VA_PRELOADED: u32 = 0x0002;

/// `va_d`: where an attribute's data is (the module's deviations).
#[derive(Clone, Copy, Debug)]
pub enum VaD {
    /// `vrun`: a non-resident attribute's runs, `cn[i]` (0 for a hole) and `cl[i]` clusters
    /// each, `cnt` of them.
    Vrun {
        /// `va_vruncn`.
        cn: Option<NonNull<Cn>>,
        /// `va_vruncl`.
        cl: Option<NonNull<Cn>>,
        /// `va_vruncnt`.
        cnt: u64,
    },
    /// `datap`: a resident attribute's data, `va_datalen` bytes.
    Datap(Option<NonNull<u8>>),
}

/// `struct ntvattr`: an attribute of a loaded ntnode.
pub struct Ntvattr {
    /// `va_list`: the link of the ntnode's `i_valist`.
    pub va_list: ListEntry<Ntvattr>,
    /// `va_vflag`.
    pub va_vflag: Cell<u32>,
    /// `va_vp`.
    pub va_vp: Cell<Option<&'static Vnode>>,
    /// `va_ip`: the ntnode the attribute belongs to.
    pub va_ip: Cell<Option<&'static Ntnode>>,
    /// `va_flag`: `NTFS_AF_INRUN`.
    pub va_flag: u32,
    /// `va_type`: the `NTFS_A_*`.
    pub va_type: u32,
    /// `va_namelen`.
    pub va_namelen: u8,
    /// `va_name`: the name, each `wchar` cut to a `char`.
    pub va_name: [u8; NTFS_MAXATTRNAME],
    /// `va_compression`.
    pub va_compression: u32,
    /// `va_compressalg`.
    pub va_compressalg: u32,
    /// `va_datalen`.
    pub va_datalen: u64,
    /// `va_allocated`.
    pub va_allocated: u64,
    /// `va_vcnstart`.
    pub va_vcnstart: Cn,
    /// `va_vcnend`.
    pub va_vcnend: Cn,
    /// `va_index`.
    pub va_index: u16,
    /// `va_d` (the module's deviations).
    pub va_d: VaD,
}

impl Ntvattr {
    /// `va_vruncn[0 .. va_vruncnt]`.
    pub fn va_vruncn(&self) -> &[Cn] {
        match self.va_d {
            VaD::Vrun {
                cn: Some(p), cnt, ..
            } => {
                // SAFETY: the `mallocarray`ed array of `cnt` cluster numbers
                // `ntfs_runtovrun` filled, freed only by `ntfs_freentvattr` with the
                // attribute.
                unsafe { core::slice::from_raw_parts(p.as_ptr(), cnt as usize) }
            }
            _ => &[],
        }
    }

    /// `va_vruncl[0 .. va_vruncnt]`.
    pub fn va_vruncl(&self) -> &[Cn] {
        match self.va_d {
            VaD::Vrun {
                cl: Some(p), cnt, ..
            } => {
                // SAFETY: as `va_vruncn`, for the cluster counts.
                unsafe { core::slice::from_raw_parts(p.as_ptr(), cnt as usize) }
            }
            _ => &[],
        }
    }

    /// `va_vruncnt`.
    pub fn va_vruncnt(&self) -> u64 {
        match self.va_d {
            VaD::Vrun { cnt, .. } => cnt,
            VaD::Datap(_) => 0,
        }
    }

    /// `va_datap[0 .. va_datalen]`: a resident attribute's data.
    pub fn va_datap(&self) -> &[u8] {
        match self.va_d {
            // SAFETY: the `malloc`ed copy of `va_datalen` bytes `ntfs_attrtontvattr` made
            // (zeroed first), freed only by `ntfs_freentvattr` with the attribute; nothing
            // writes it meanwhile.
            VaD::Datap(Some(p)) => unsafe {
                core::slice::from_raw_parts(p.as_ptr(), self.va_datalen as usize)
            },
            _ => &[],
        }
    }

    /// `va_a_name`: the `$FILE_NAME` value.
    pub fn va_a_name(&self) -> AttrName {
        AttrName::read(self.va_datap(), 0)
    }

    /// `va_a_iroot`: the `$INDEX_ROOT` value.
    pub fn va_a_iroot(&self) -> AttrIndexroot {
        AttrIndexroot::read(self.va_datap(), 0)
    }

    /// `va_a_ialloc`: the `$INDEX_ALLOCATION` header.
    pub fn va_a_ialloc(&self) -> AttrIndexalloc {
        AttrIndexalloc::read(self.va_datap(), 0)
    }

    /// `vap->va_ip`, which `ntfs_loadntnode` sets.
    pub fn ip(&self) -> &'static Ntnode {
        match self.va_ip.get() {
            Some(ip) => ip,
            None => panic(format_args!("ntvattr {:p}: no va_ip", self)),
        }
    }
}

queue_adapter!(
    /// The ntnode's attributes (`LIST_ENTRY(ntvattr) va_list`).
    pub NtvattrList: Ntvattr, va_list => ListEntry<Ntvattr>
);

/// A `malloc`ed, zeroed byte buffer, freed when dropped (the module's deviations).
pub struct KBuf {
    /// The allocation.
    p: NonNull<u8>,
    /// Its size.
    len: usize,
    /// Its `M_*` type.
    type_: i32,
}

impl KBuf {
    /// `malloc(len, type, M_WAITOK | M_ZERO)`.
    pub fn new(len: usize, type_: i32) -> Self {
        let Some(p) = malloc(len, type_, M_WAITOK | M_ZERO) else {
            panic(format_args!("KBuf: no memory"));
        };
        Self { p, len, type_ }
    }

    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `len` zeroed (so initialised) bytes owned by this buffer until it drops.
        unsafe { core::slice::from_raw_parts(self.p.as_ptr(), self.len) }
    }

    /// The bytes, writable.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as `as_slice`; `&mut self` makes the slice unique.
        unsafe { core::slice::from_raw_parts_mut(self.p.as_ptr(), self.len) }
    }
}

impl Drop for KBuf {
    fn drop(&mut self) {
        free(self.p, self.type_, self.len);
    }
}

/// Where attribute data goes: the C's `rdata` or its `uio` (the module's deviations).
pub enum Rdata<'a, 'b> {
    /// `rdata`: a kernel buffer.
    Mem(&'a mut [u8]),
    /// `uio`.
    Uio(&'a mut Uio<'b>),
}

impl Rdata<'_, '_> {
    /// `memcpy(data + doff, src, len)` or `uiomove(src, len, uio)`.
    fn copy(&mut self, doff: usize, src: &[u8]) -> Result<(), Errno> {
        match self {
            Rdata::Mem(d) => {
                let d = d.get_mut(doff..).unwrap_or(&mut []);
                let n = d.len().min(src.len());
                d[..n].copy_from_slice(&src[..n]);
                Ok(())
            }
            Rdata::Uio(uio) => uiomove_from(src, uio),
        }
    }

    /// `bzero(data + doff, len)`, or `len` zero bytes moved by `uiomove`.
    fn zero(&mut self, doff: usize, len: usize) -> Result<(), Errno> {
        match self {
            Rdata::Mem(d) => {
                let d = d.get_mut(doff..).unwrap_or(&mut []);
                let n = d.len().min(len);
                d[..n].fill(0);
                Ok(())
            }
            Rdata::Uio(uio) => {
                let mut zeros = [0u8; 256];
                let mut left = len;
                while left > 0 {
                    let n = left.min(zeros.len());
                    uiomove(&mut zeros[..n], uio)?;
                    left -= n;
                }
                Ok(())
            }
        }
    }
}

/// Local struct used in `ntfs_ntlookupfile()`: where to resume a full scan.
struct NtfsLookupCtx {
    /// `aoff`.
    aoff: u32,
    /// `rdsize`.
    rdsize: u32,
    /// `cn`.
    cn: Cn,
    /// `prev`.
    prev: Option<&'static NtfsLookupCtx>,
}

/// `ntfs_toupper_tab`: table for mapping Unicode chars into uppercase; it's filled upon first
/// ntfs mount, freed upon last ntfs umount.
static NTFS_TOUPPER_TAB: AtomicPtr<Wchar> = AtomicPtr::new(ptr::null_mut());
/// `ntfs_toupper_lock`.
pub static NTFS_TOUPPER_LOCK: Rwlock = Rwlock::new("ntfs_toupper");
/// `ntfs_toupper_usecount`.
static NTFS_TOUPPER_USECOUNT: AtomicI32 = AtomicI32::new(0);

/// The size of the upper-case table: 256 * 256 `wchar`s.
const NTFS_TOUPPER_SIZE: usize = 256 * 256 * size_of::<Wchar>();

/// `malloc(sizeof(T), type, M_WAITOK | M_ZERO)` holding `value`.
pub(crate) fn ntfs_alloc<T>(value: T, type_: i32) -> &'static T {
    let Some(mem) = malloc(size_of::<T>(), type_, M_WAITOK | M_ZERO) else {
        panic(format_args!("ntfs_alloc: no memory"));
    };
    kassert!((mem.as_ptr() as usize).is_multiple_of(align_of::<T>()));
    let p = mem.cast::<T>();
    // SAFETY: a fresh allocation of `T`'s size, aligned for it (malloc's buckets are at
    // least 16-byte aligned), written once here; `ntfs_dealloc` gives it back.
    unsafe {
        p.as_ptr().write(value);
        p.as_ref()
    }
}

/// `free(obj, type, 0)` of an object `ntfs_alloc` made.
///
/// # Safety
///
/// `obj` came from `ntfs_alloc` with `type_`, is unlinked from every list, and nothing uses
/// it afterwards.
pub(crate) unsafe fn ntfs_dealloc<T>(obj: &T, type_: i32) {
    free(NonNull::from(obj).cast::<u8>(), type_, size_of::<T>());
}

/// `uiomove(src, len, uio)` out of read-only bytes, through a bounce buffer: `uiomove` wants
/// a writable source.
pub fn uiomove_from(src: &[u8], uio: &mut Uio<'_>) -> Result<(), Errno> {
    let mut bounce = [0u8; 256];
    for chunk in src.chunks(bounce.len()) {
        if uio.uio_resid == 0 {
            break;
        }
        let b = &mut bounce[..chunk.len()];
        b.copy_from_slice(chunk);
        uiomove(b, uio)?;
    }
    Ok(())
}

/// `NTFS_U28(ch)`.
#[allow(non_snake_case)] // the C macro's name
pub fn NTFS_U28(ch: Wchar) -> u8 {
    if ch & 0xE0 == 0 {
        b'_'
    } else {
        (ch & 0xFF) as u8
    }
}

/// `NTFS_TOUPPER(ch)`: `ntfs_toupper_tab[(unsigned char)(ch)]`.
#[allow(non_snake_case)] // the C macro's name
pub fn NTFS_TOUPPER(ch: Wchar) -> Wchar {
    let tab = NTFS_TOUPPER_TAB.load(Ordering::Acquire);
    if tab.is_null() {
        panic(format_args!("NTFS_TOUPPER: no table"));
    }
    // SAFETY: a non-null table is the zeroed allocation of 256 * 256 `wchar`s
    // `ntfs_toupper_use` made, freed only by the last `ntfs_toupper_unuse`, after the last
    // NTFS mount that compares names is gone; the index is below 256.
    unsafe { tab.add(usize::from(ch & 0xFF)).read() }
}

/// Forget the upper-case table: the host tests reset the memory it lives in between tests.
#[cfg(test)]
pub(crate) fn ntfs_toupper_reset() {
    NTFS_TOUPPER_TAB.store(ptr::null_mut(), Ordering::Release);
    NTFS_TOUPPER_USECOUNT.store(0, Ordering::Relaxed);
}

/// Install `tab` (256 * 256 `wchar`s at least) as the upper-case table, for the host tests
/// of the comparisons.
#[cfg(test)]
pub(crate) fn ntfs_toupper_set(tab: &'static mut [Wchar]) {
    assert!(tab.len() >= 256 * 256);
    NTFS_TOUPPER_TAB.store(tab.as_mut_ptr(), Ordering::Release);
}

/// `NTFS_AALPCMP(aalp, type, name, namelen)`: the attribute list entry at `off` of `buf`
/// names attribute `type_` called `name`.
fn ntfs_aalpcmp(ntmp: &Ntfsmount, buf: &[u8], off: usize, type_: u32, name: &[u8]) -> bool {
    let aalp = AttrAttrlist::read(buf, off);
    if { aalp.al_type } != type_ || usize::from(aalp.al_namelen) != name.len() {
        return false;
    }
    let mut ustr = [0 as Wchar; 255];
    let n = usize::from(aalp.al_namelen);
    for (i, u) in ustr[..n].iter_mut().enumerate() {
        *u = wchar_at(buf, off + AttrAttrlist::SIZE, i);
    }
    ntfs_uastrcmp(ntmp, &ustr[..n], name) == 0
}

/// `ntfs_ntvattrrele(vap)`: release the reference `ntfs_ntvattrget` took on the attribute's
/// ntnode.
pub fn ntfs_ntvattrrele(vap: &Ntvattr) {
    ntfs_ntrele(vap.ip());
}

/// `ntfs_findvattr`: find the attribute in the ntnode, loading it first if needed.
/// `Ok(true)` with `*vapp` set (and the ntnode referenced) when found; `Ok(false)` when not,
/// with `*lvapp` the attribute list if the ntnode has one.
pub fn ntfs_findvattr(
    ntmp: &Ntfsmount,
    ip: &'static Ntnode,
    lvapp: &mut Option<&'static Ntvattr>,
    vapp: &mut Option<&'static Ntvattr>,
    type_: u32,
    name: &[u8],
    vcn: Cn,
) -> Result<bool, Errno> {
    if ip.i_flag.get() & IN_LOADED == 0 {
        if let Err(e) = ntfs_loadntnode(ntmp, ip) {
            printf(format_args!(
                "ntfs_findvattr: FAILED TO LOAD INO: {}\n",
                ip.i_number.get()
            ));
            return Err(e);
        }
    } else {
        // Update LRU loaded list.
        // SAFETY: a loaded ntnode is on the mount's LRU (`ntfs_loadntnode`); it goes back
        // at its head at once.
        unsafe {
            ntmp.ntm_ntnodeq.remove(ip);
            ntmp.ntm_ntnodeq.insert_head(ip);
        }
    }

    *lvapp = None;
    *vapp = None;
    for vap in ip.i_valist.iter() {
        if vap.va_type == type_
            && vap.va_vcnstart <= vcn
            && vap.va_vcnend >= vcn
            && usize::from(vap.va_namelen) == name.len()
            && vap.va_name[..name.len()] == *name
        {
            *vapp = Some(vap);
            ntfs_ntref(vap.ip());
            return Ok(true);
        }
        if vap.va_type == NTFS_A_ATTRLIST {
            *lvapp = Some(vap);
        }
    }

    Ok(false)
}

/// `ntfs_ntvattrget`: search attribute specified in ntnode (load ntnode if necessary). If
/// not found but `$ATTRIBUTE_LIST` present, read it in and search through: VOP_VGET node
/// needed, and lookup through its ntnode (load if necessary). The attribute's ntnode is
/// referenced until `ntfs_ntvattrrele`.
///
/// ntnode should be locked
pub fn ntfs_ntvattrget(
    ntmp: &Ntfsmount,
    ip: &'static Ntnode,
    type_: u32,
    name: Option<&[u8]>,
    vcn: Cn,
) -> Result<&'static Ntvattr, Errno> {
    let name = name.unwrap_or(b"");
    let mut lvap = None;
    let mut vap = None;

    if ntfs_findvattr(ntmp, ip, &mut lvap, &mut vap, type_, name, vcn)?
        && let Some(v) = vap
    {
        return Ok(v);
    }

    let Some(lvap) = lvap else {
        return Err(Errno::ENOENT);
    };
    // Scan $ATTRIBUTE_LIST for requested attribute
    let mut len = lvap.va_datalen as usize;
    let mut alpool = KBuf::new(len, M_TEMP);
    let mut init = 0;
    ntfs_readntvattr_plain(
        ntmp,
        ip,
        lvap,
        0,
        len,
        &mut Rdata::Mem(alpool.as_mut_slice()),
        0,
        &mut init,
    )?;
    len = init;
    let alpool = alpool.as_slice();

    let mut aoff = 0usize;
    while len > 0 {
        let aalp = AttrAttrlist::read(alpool, aoff);
        let reclen = usize::from(aalp.reclen);
        if reclen == 0 {
            break;
        }

        let nextaoff = if len > reclen {
            Some(aoff + reclen)
        } else {
            None
        };
        len = len.saturating_sub(reclen);

        let skip = !ntfs_aalpcmp(ntmp, alpool, aoff, type_, name)
            || nextaoff.is_some_and(|n| {
                AttrAttrlist::read(alpool, n).al_vcnstart <= vcn
                    && ntfs_aalpcmp(ntmp, alpool, n, type_, name)
            });
        if skip {
            match nextaoff {
                Some(n) => {
                    aoff = n;
                    continue;
                }
                None => break,
            }
        }

        // this is not a main record, so we can't use just plain vget()
        let newvp = match ntfs_vgetex(
            ntmp.mountp(),
            aalp.al_inumber,
            NTFS_A_DATA,
            None,
            LK_EXCLUSIVE,
            VG_EXT,
        ) {
            Ok(vp) => vp,
            Err(e) => {
                printf(format_args!("ntfs_ntvattrget: CAN'T VGET INO: {}\n", {
                    aalp.al_inumber
                }));
                return Err(e);
            }
        };
        let newip = VTONT(newvp);
        // XXX have to lock ntnode
        let mut lvap2 = None;
        let mut vap2 = None;
        let found = ntfs_findvattr(ntmp, newip, &mut lvap2, &mut vap2, type_, name, vcn);
        vput(newvp);
        if let (Ok(true), Some(v)) = (found, vap2) {
            return Ok(v);
        }
        printf(format_args!("ntfs_ntvattrget: ATTRLIST ERROR.\n"));
        break;
    }

    Err(Errno::ENOENT)
}

/// `ntfs_loadntnode`: read ntnode from disk, make ntvattr list.
///
/// ntnode should be locked
pub fn ntfs_loadntnode(ntmp: &Ntfsmount, ip: &'static Ntnode) -> Result<(), Errno> {
    kassert!(ip.i_flag.get() & IN_LOADED == 0);

    if ntmp.ntm_ntnodes.get() >= LOADED_NTNODE_HI
        && let Some(oip) = ntmp.ntm_ntnodeq.last()
    {
        // SAFETY: `oip` is on the LRU (it is its last element).
        unsafe { ntmp.ntm_ntnodeq.remove(oip) };
        ntmp.ntm_ntnodes.set(ntmp.ntm_ntnodes.get() - 1);

        kassert!(oip.i_flag.get() & IN_LOADED != 0);
        oip.clr_flag(IN_LOADED);
        ntfs_freevattrs(oip);
    }

    let recsz = ntmp.ntfs_bntob(ntmp.ntm_bpmftrec.get());
    let mut mfrp = KBuf::new(usize::try_from(recsz).unwrap_or(0), M_TEMP);

    if (ip.i_number.get() as usize) < NTFS_SYSNODESNUM {
        let bn: Daddr = ntmp
            .ntfs_cntobn(ntmp.ntm_mftcn())
            .wrapping_add(Daddr::from(ntmp.ntm_bpmftrec.get()) * Daddr::from(ip.i_number.get()));

        let (bp, error) = bread(ntmp.devvp(), bn, recsz);
        if let Err(e) = error {
            printf(format_args!("ntfs_loadntnode: BREAD FAILED\n"));
            brelse(bp);
            return Err(e);
        }
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
        // the release below.
        let data = unsafe { bp.data() };
        let n = data.len().min(mfrp.len);
        mfrp.as_mut_slice()[..n].copy_from_slice(&data[..n]);
        brelse(bp);
    } else {
        let vp = ntmp.sysvn(NTFS_MFTINO);
        if let Err(e) = ntfs_readattr(
            ntmp,
            VTONT(vp),
            NTFS_A_DATA,
            None,
            Off::from(ip.i_number.get()) * Off::from(recsz),
            mfrp.len,
            &mut Rdata::Mem(mfrp.as_mut_slice()),
        ) {
            printf(format_args!("ntfs_loadntnode: ntfs_readattr failed\n"));
            return Err(e);
        }
    }

    // Check if magic and fixups are correct
    let len = mfrp.len;
    if let Err(e) = ntfs_procfixups(ntmp, NTFS_FILEMAGIC, mfrp.as_mut_slice(), len) {
        printf(format_args!(
            "ntfs_loadntnode: BAD MFT RECORD {}\n",
            ip.i_number.get()
        ));
        return Err(e);
    }

    let mfrp = mfrp.as_slice();
    let fr = Filerec::read(mfrp, 0);
    let mut off = usize::from(fr.fr_attroff);

    ip.i_valist.init();

    let mut error = Ok(());
    loop {
        let ap = Attr::read(mfrp, off);
        if { ap.a_hdr.a_type } == u32::MAX {
            break;
        }
        // An attribute past the record, or of no length (the module's deviations).
        if off + size_of::<crate::ntfs::ntfs::Attrhdr>() > mfrp.len() || { ap.a_hdr.reclen } == 0 {
            error = Err(Errno::EINVAL);
            break;
        }
        match ntfs_attrtontvattr(ntmp, mfrp, off) {
            Ok(vap) => {
                vap.va_ip.set(Some(ip));
                // SAFETY: a fresh ntvattr, on no list; it stays in place until
                // `ntfs_freevattrs` unlinks and frees it.
                unsafe { ip.i_valist.insert_head(vap) };
            }
            Err(e) => {
                error = Err(e);
                break;
            }
        }

        off += ap.a_hdr.reclen as usize;
    }
    if let Err(e) = error {
        printf(format_args!(
            "ntfs_loadntnode: failed to load attr ino: {}\n",
            ip.i_number.get()
        ));
        return Err(e);
    }

    ip.i_mainrec.set(fr.fr_mainrec);
    ip.i_nlink.set(i64::from(fr.fr_nlink));
    ip.i_frflag.set(u32::from(fr.fr_flags));

    ip.set_flag(IN_LOADED);

    // Add to loaded list.
    // SAFETY: the ntnode was not loaded, so it is on no LRU; `ntfs_ntput` or a later unload
    // takes it off before it is freed.
    unsafe { ntmp.ntm_ntnodeq.insert_head(ip) };
    ntmp.ntm_ntnodes.set(ntmp.ntm_ntnodes.get() + 1);

    Ok(())
}

/// Unlink and free every attribute of `ip` (the `while ((vap = LIST_FIRST(...)))` loops).
fn ntfs_freevattrs(ip: &Ntnode) {
    while let Some(vap) = ip.i_valist.first() {
        // SAFETY: `vap` is on the list (its first element).
        unsafe { ListHead::<NtvattrList>::remove(vap) };
        // SAFETY: the attribute is off the list; it came from `ntfs_attrtontvattr`, and the
        // C frees it here with every reference to it gone or stale, as this does.
        unsafe { ntfs_freentvattr(vap) };
    }
}

/// `ntfs_ntget(ip)`: lock the ntnode and increase its use count, just opposite of
/// `ntfs_ntput()`.
pub fn ntfs_ntget(ip: &Ntnode) -> Result<(), Errno> {
    ip.i_usecount.set(ip.i_usecount.get() + 1);

    rw_enter_write(&ip.i_lock);

    Ok(())
}

/// `ntfs_ntlookup(ntmp, ino)`: search the ntnode in the hash; if found, lock it, increase its
/// use count and return it. If not in the hash allocate structure for ntnode, prefill it,
/// lock, inc count and return.
///
/// ntnode returned locked
pub fn ntfs_ntlookup(ntmp: &'static Ntfsmount, ino: Ntfsino) -> Result<&'static Ntnode, Errno> {
    loop {
        // retry:
        if let Some(ip) = ntfs_nthashlookup(ntmp.ntm_dev.get(), ino) {
            let _ = ntfs_ntget(ip);
            return Ok(ip);
        }

        let ip = ntfs_alloc(Ntnode::new(), M_NTFSNTNODE);

        // Generic initialization
        ip.i_devvp.set(ntmp.ntm_devvp.get());
        ip.i_dev.set(ntmp.ntm_dev.get());
        ip.i_number.set(ino);
        ip.i_mp.set(Some(ntmp));

        ip.i_fnlist.init();
        ip.i_valist.init();
        vref(ip.devvp());

        // init lock and lock the newborn ntnode
        rw_init(&ip.i_lock, "ntnode");
        let _ = ntfs_ntget(ip);

        if ntfs_nthashins(ip).is_err() {
            ntfs_ntput(ip);
            continue;
        }

        return Ok(ip);
    }
}

/// `ntfs_ntput(ip)`: decrement usecount of ntnode and unlock it; if usecount reach zero,
/// deallocate ntnode.
///
/// ntnode should be locked on entry, and unlocked on return.
pub fn ntfs_ntput(ip: &'static Ntnode) {
    let ntmp = ip.mp();

    ip.i_usecount.set(ip.i_usecount.get() - 1);

    #[cfg(feature = "diagnostic")]
    if ip.i_usecount.get() < 0 {
        panic(format_args!(
            "ntfs_ntput: ino: {} usecount: {} ",
            ip.i_number.get(),
            ip.i_usecount.get()
        ));
    }

    if ip.i_usecount.get() > 0 {
        rw_exit_write(&ip.i_lock);
        return;
    }

    if ip.i_fnlist.first().is_some() {
        panic(format_args!("ntfs_ntput: ntnode has fnodes"));
    }

    ntfs_nthashrem(ip);

    // Remove from loaded list.
    if ip.i_flag.get() & IN_LOADED != 0 {
        // SAFETY: a loaded ntnode is on the mount's LRU.
        unsafe { ntmp.ntm_ntnodeq.remove(ip) };
        ntmp.ntm_ntnodes.set(ntmp.ntm_ntnodes.get() - 1);
    }

    ntfs_freevattrs(ip);

    vrele(ip.devvp());
    // SAFETY: the ntnode came from `ntfs_alloc(M_NTFSNTNODE)` in `ntfs_ntlookup`; it is off
    // the hash and the LRU, has no fnodes and no attributes, and its last use is gone.
    unsafe { ntfs_dealloc(ip, M_NTFSNTNODE) };
}

/// `ntfs_ntref(ip)`: increment usecount of ntnode.
pub fn ntfs_ntref(ip: &Ntnode) {
    ip.i_usecount.set(ip.i_usecount.get() + 1);
}

/// `ntfs_ntrele(ip)`: decrement usecount of ntnode.
pub fn ntfs_ntrele(ip: &Ntnode) {
    ip.i_usecount.set(ip.i_usecount.get() - 1);

    if ip.i_usecount.get() < 0 {
        panic(format_args!(
            "ntfs_ntrele: ino: {} usecount: {} ",
            ip.i_number.get(),
            ip.i_usecount.get()
        ));
    }
}

/// `ntfs_freentvattr(vap)`: deallocate all memory allocated for ntvattr.
///
/// # Safety
///
/// `vap` came from `ntfs_attrtontvattr`, is on no list, and nothing uses it afterwards.
pub unsafe fn ntfs_freentvattr(vap: &Ntvattr) {
    match vap.va_d {
        VaD::Vrun { cn, cl, cnt } => {
            if let Some(p) = cn {
                free(p.cast(), M_NTFSRUN, cnt as usize * size_of::<Cn>());
            }
            if let Some(p) = cl {
                free(p.cast(), M_NTFSRUN, cnt as usize * size_of::<Cn>());
            }
        }
        VaD::Datap(Some(p)) => free(p, M_NTFSRDATA, vap.va_datalen as usize),
        VaD::Datap(None) => {}
    }
    // SAFETY: the caller's contract; the attribute came from `ntfs_alloc(M_NTFSNTVATTR)`.
    unsafe { ntfs_dealloc(vap, M_NTFSNTVATTR) };
}

/// `ntfs_attrtontvattr`: convert disk image of attribute (at `off` of the MFT record
/// `rec`) into ntvattr structure, runs are expanded also.
pub fn ntfs_attrtontvattr(
    ntmp: &Ntfsmount,
    rec: &[u8],
    off: usize,
) -> Result<&'static Ntvattr, Errno> {
    let rap = Attr::read(rec, off);
    let hdr = rap.a_hdr;

    let mut va_name = [0u8; NTFS_MAXATTRNAME];
    let va_namelen = hdr.a_namelen;
    if va_namelen != 0 {
        let unp = off + usize::from(hdr.a_nameoff);
        for (i, c) in va_name[..usize::from(va_namelen)].iter_mut().enumerate() {
            *c = wchar_at(rec, unp, i) as u8;
        }
    }

    let va_flag = u32::from(hdr.a_flag);
    let (va_datalen, va_allocated, va_vcnstart, va_vcnend, va_compressalg, va_d) =
        if va_flag & NTFS_AF_INRUN != 0 {
            let nr = rap.a_nr();
            let run = rec.get(off + usize::from(nr.a_dataoff)..).unwrap_or(&[]);
            let (cn, cl, cnt) = ntfs_runtovrun(run)?;
            (
                nr.a_datalen,
                nr.a_allocated,
                nr.a_vcnstart,
                nr.a_vcnend,
                u32::from(nr.a_compressalg),
                VaD::Vrun { cn, cl, cnt },
            )
        } else {
            let r = rap.a_r();
            let datalen = u64::from(r.a_datalen);
            let Some(p) = malloc(usize::from(r.a_datalen), M_NTFSRDATA, M_WAITOK | M_ZERO) else {
                panic(format_args!("ntfs_attrtontvattr: no memory"));
            };
            let src = rec.get(off + usize::from(r.a_dataoff)..).unwrap_or(&[]);
            let n = src.len().min(usize::from(r.a_datalen));
            // SAFETY: `p` is a fresh allocation of `a_datalen` bytes; at most that many are
            // copied into it from the record.
            unsafe { ptr::copy_nonoverlapping(src.as_ptr(), p.as_ptr(), n) };
            (
                datalen,
                datalen,
                0,
                ntmp.ntfs_btocn(datalen as Off),
                0,
                VaD::Datap(Some(p)),
            )
        };

    Ok(ntfs_alloc(
        Ntvattr {
            va_list: ListEntry::new(),
            va_vflag: Cell::new(0),
            va_vp: Cell::new(None),
            va_ip: Cell::new(None),
            va_flag,
            va_type: hdr.a_type,
            va_namelen,
            va_name,
            va_compression: u32::from(hdr.a_compression),
            va_compressalg,
            va_datalen,
            va_allocated,
            va_vcnstart,
            va_vcnend,
            va_index: hdr.a_index,
            va_d,
        },
        M_NTFSNTVATTR,
    ))
}

/// `ntfs_runtovrun(&cn, &cl, &cnt, run)`: expand run into more utilizable and more memory
/// eating format: the `mallocarray(M_NTFSRUN)`ed cluster numbers (0 for a hole) and lengths
/// of the runs, and their count.
#[allow(clippy::type_complexity)] // the C's three out-parameters
pub fn ntfs_runtovrun(
    run: &[u8],
) -> Result<(Option<NonNull<Cn>>, Option<NonNull<Cn>>, u64), Errno> {
    let r = |i: usize| run.get(i).copied().unwrap_or(0);

    let mut off = 0usize;
    let mut cnt = 0usize;
    while r(off) != 0 {
        off += usize::from(r(off) & 0xF) + usize::from((r(off) >> 4) & 0xF) + 1;
        cnt += 1;
    }
    if cnt == 0 {
        return Ok((None, None, 0));
    }
    let alloc = || {
        let Some(p) = mallocarray(cnt, size_of::<Cn>(), M_NTFSRUN, M_WAITOK | M_ZERO) else {
            panic(format_args!("ntfs_runtovrun: no memory"));
        };
        p.cast::<Cn>()
    };
    let cnp = alloc();
    let clp = alloc();
    // SAFETY: two fresh, zeroed allocations of `cnt` cluster numbers each (malloc aligns
    // them for a u64); the slices die before the pointers are returned.
    let (cn, cl) = unsafe {
        (
            core::slice::from_raw_parts_mut(cnp.as_ptr(), cnt),
            core::slice::from_raw_parts_mut(clp.as_ptr(), cnt),
        )
    };

    off = 0;
    let mut i = 0usize;
    let mut prev: Cn = 0;
    while r(off) != 0 {
        let mut sz = u32::from(r(off));
        off += 1;
        cl[i] = 0;

        for k in 0..(sz & 0xF) {
            cl[i] = cl[i].wrapping_add(u64::from(u32::from(r(off)).wrapping_shl(k << 3)));
            off += 1;
        }

        sz >>= 4;
        let mut tmp: u64;
        if r((off + sz as usize).wrapping_sub(1)) & 0x80 != 0 {
            tmp = u64::MAX.wrapping_shl(sz << 3);
        } else {
            tmp = 0;
        }
        for k in 0..sz {
            tmp |= u64::from(r(off)).wrapping_shl(k << 3);
            off += 1;
        }
        if tmp != 0 {
            prev = prev.wrapping_add(tmp);
            cn[i] = prev;
        } else {
            cn[i] = tmp;
        }

        i += 1;
    }
    Ok((Some(cnp), Some(clp), cnt as u64))
}

/// `ntfs_uastricmp`: compare unicode and ascii string case insens.
pub fn ntfs_uastricmp(ntmp: &Ntfsmount, ustr: &[Wchar], astr: &[u8]) -> i32 {
    let mut pos = 0usize;
    let mut i = 0usize;

    while i < ustr.len() && pos < astr.len() {
        let res = ntmp.wcmp(
            NTFS_TOUPPER(ustr[i]),
            NTFS_TOUPPER(ntmp.wget(astr, &mut pos)),
        );
        if res != 0 {
            return res;
        }
        i += 1;
    }

    if i == ustr.len() && pos == astr.len() {
        0
    } else if i == ustr.len() {
        -1
    } else {
        1
    }
}

/// `ntfs_uastrcmp`: compare unicode and ascii string case sens.
pub fn ntfs_uastrcmp(ntmp: &Ntfsmount, ustr: &[Wchar], astr: &[u8]) -> i32 {
    let mut pos = 0usize;
    let mut i = 0usize;

    while i < ustr.len() && pos < astr.len() {
        let res = ntmp.wcmp(ustr[i], ntmp.wget(astr, &mut pos));
        if res != 0 {
            return res;
        }
        i += 1;
    }

    if i == ustr.len() && pos == astr.len() {
        0
    } else if i == ustr.len() {
        -1
    } else {
        1
    }
}

/// The two attribute names are the same: both NULL, or both set and equal.
fn attrnames_eq(a: Option<&[u8]>, b: Option<&[u8]>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `ntfs_fget`: search fnode in ntnode, if not found allocate and preinitialize. A new
/// fnode takes `attrname` (`FN_AATTRNAME`).
///
/// ntnode should be locked on entry.
pub fn ntfs_fget(
    _ntmp: &Ntfsmount,
    ip: &'static Ntnode,
    attrtype: u32,
    attrname: Option<AttrNameBuf>,
) -> Result<&'static Fnode, Errno> {
    let want = attrname.as_ref().map(AttrNameBuf::bytes);
    let found = ip
        .i_fnlist
        .iter()
        .filter(|fp| attrtype == fp.f_attrtype.get() && attrnames_eq(want, fp.attrname()))
        .last();
    if let Some(fp) = found {
        return Ok(fp);
    }

    let fp = ntfs_alloc(Fnode::new(), M_NTFSFNODE);

    fp.f_ip.set(Some(ip));
    fp.f_attrname.set(attrname);
    if attrname.is_some() {
        fp.f_flag.set(fp.f_flag.get() | FN_AATTRNAME);
    }
    fp.f_attrtype.set(attrtype);

    ntfs_ntref(ip);

    // SAFETY: a fresh fnode, on no list; `ntfs_frele` unlinks it before it frees it.
    unsafe { ip.i_fnlist.insert_head(fp) };

    Ok(fp)
}

/// Free an attribute name `ntfs_ntlookupattr` made.
fn attrname_free(n: AttrNameBuf) {
    free(n.p, M_TEMP, n.len + 1);
}

/// `ntfs_frele(fp)`: deallocate fnode, remove it from ntnode's fnode list.
///
/// ntnode should be locked.
pub fn ntfs_frele(fp: &'static Fnode) {
    let ip = FTONT(fp);

    // SAFETY: the fnode is on its ntnode's list (`ntfs_fget`).
    unsafe { ListHead::<FnodeList>::remove(fp) };
    if fp.f_flag.get() & FN_AATTRNAME != 0
        && let Some(n) = fp.f_attrname.take()
    {
        attrname_free(n);
    }
    if let Some(b) = fp.f_dirblbuf.take() {
        free(b, M_NTFSDIR, fp.f_dirblbuf_len.get());
    }
    // SAFETY: the fnode came from `ntfs_alloc(M_NTFSFNODE)`, is off its list, and its vnode
    // (if any) no longer points at it.
    unsafe { ntfs_dealloc(fp, M_NTFSFNODE) };
    ntfs_ntrele(ip);
}

/// `ntfs_ntlookupattr`: lookup attribute name in format: `[[:$ATTR_TYPE]:$ATTR_NAME]`;
/// `$ATTR_TYPE` is searched in attrdefs read from `$AttrDef`. If `$ATTR_TYPE` not specified,
/// `ATTR_A_DATA` assumed.
pub fn ntfs_ntlookupattr(
    ntmp: &Ntfsmount,
    name: &[u8],
    attrtype: &mut u32,
    attrname: &mut Option<AttrNameBuf>,
) -> Result<(), Errno> {
    let mut name = name;

    if name.is_empty() {
        return Ok(());
    }

    if name[0] == b'$' {
        let sys = name;
        let mut syslen = 0usize;
        let mut skip = 0usize;
        while syslen < name.len() {
            if sys[syslen] == b':' {
                skip = 1;
                break;
            }
            syslen += 1;
        }
        name = &name[(syslen + skip).min(name.len())..];

        let found = ntmp.ad().iter().find(|adp| {
            let adname = adp.ad_name;
            usize::try_from(adp.ad_namelen).ok() == Some(syslen)
                && syslen <= adname.len()
                && sys[..syslen] == adname[..syslen]
        });
        match found {
            Some(adp) => *attrtype = adp.ad_type,
            None => return Err(Errno::ENOENT),
        }
    }

    // out:
    if !name.is_empty() {
        let Some(p) = malloc(name.len() + 1, M_TEMP, M_WAITOK | M_ZERO) else {
            panic(format_args!("ntfs_ntlookupattr: no memory"));
        };
        // SAFETY: `p` is a fresh allocation of `len + 1` bytes; the name fills the first
        // `len`, the zeroed last one is the NUL.
        unsafe { ptr::copy_nonoverlapping(name.as_ptr(), p.as_ptr(), name.len()) };
        *attrname = Some(AttrNameBuf { p, len: name.len() });
        *attrtype = NTFS_A_DATA;
    }

    Ok(())
}

/// The `cn_t` at byte `off` of `buf` (0 past its end).
fn cn_at(buf: &[u8], off: usize) -> Cn {
    let mut b = [0u8; 8];
    if let Some(src) = buf.get(off..) {
        let n = src.len().min(8);
        b[..n].copy_from_slice(&src[..n]);
    }
    u64::from_ne_bytes(b)
}

/// The name of the index entry, `ie_fname[0 .. ie_fnamelen]`.
fn ie_fname(iep: &AttrIndexentry) -> ([Wchar; 255], usize) {
    (iep.ie_fname, usize::from(iep.ie_fnamelen))
}

/// `ntfs_ntlookupfile`: lookup specified node for filename, matching cnp, return fnode
/// filled.
pub fn ntfs_ntlookupfile(
    ntmp: &'static Ntfsmount,
    vp: &'static Vnode,
    cnp: &Componentname,
    vpp: &mut Option<&'static Vnode>,
) -> Result<(), Errno> {
    let fp = VTOF(vp);
    let ip = FTONT(fp);
    let mut vap: Option<&'static Ntvattr> = None; // Root attribute
    let mut cn: Cn = 0; // VCN in current attribute
    let mut rdbuf: Option<KBuf> = None; // Buffer to read directory's blocks
    let mut attrtype = NTFS_A_DATA;
    let mut attrname: Option<AttrNameBuf> = None;
    let mut fullscan = false;
    let mut lookup_ctx: Option<&'static NtfsLookupCtx> = None;

    ntfs_ntget(ip)?;

    let error: Result<(), Errno> = 'fail: {
        match ntfs_ntvattrget(ntmp, ip, NTFS_A_INDXROOT, Some(b"$I30"), 0) {
            Ok(v) => {
                vap = Some(v);
                if v.va_flag & NTFS_AF_INRUN != 0 {
                    break 'fail Err(Errno::ENOTDIR);
                }
            }
            Err(_) => break 'fail Err(Errno::ENOTDIR),
        }
        let Some(vap) = vap else {
            break 'fail Err(Errno::ENOTDIR);
        };

        // Divide file name into: foofilefoofilefoofile[:attrspec]
        // Store like this:       fname:fnamelen       [aname:anamelen]
        let name = cnp.name();
        let (fname, aname) = match name.iter().position(|&c| c == b':') {
            Some(i) => (&name[..i], Some(&name[i + 1..])),
            None => (name, None),
        };

        let blsize = vap.va_a_iroot().ir_size;

        let rd = rdbuf.insert(KBuf::new(blsize as usize, M_TEMP));

        'loop_: loop {
            let mut rdsize = vap.va_datalen as u32;

            if let Err(e) = ntfs_readattr(
                ntmp,
                ip,
                NTFS_A_INDXROOT,
                Some(b"$I30"),
                0,
                rdsize as usize,
                &mut Rdata::Mem(rd.as_mut_slice()),
            ) {
                break 'fail Err(e);
            }

            let mut aoff = AttrIndexroot::SIZE as u32;

            let error: Result<(), Errno> = loop {
                let mut iep = AttrIndexentry::read(rd.as_slice(), aoff as usize);

                'scan: while iep.ie_flag & NTFS_IEFLAG_LAST == 0 && rdsize > aoff {
                    'next: {
                        let (uname, ulen) = ie_fname(&iep);

                        // check the name - the case-insensitive check has to come first,
                        // to break from this for loop if needed, so we can dive correctly
                        let mut res = ntfs_uastricmp(ntmp, &uname[..ulen], fname);
                        if !fullscan {
                            if res > 0 {
                                break 'scan;
                            }
                            if res < 0 {
                                break 'next;
                            }
                        }

                        if iep.ie_fnametype == 0 || ntmp.ntm_flag.get() & NTFS_MFLAG_CASEINS == 0 {
                            res = ntfs_uastrcmp(ntmp, &uname[..ulen], fname);
                            if res != 0 && !fullscan {
                                break 'next;
                            }
                        }

                        // if we perform full scan, the file does not match and this is
                        // subnode, dive
                        if fullscan && res != 0 {
                            if iep.ie_flag & NTFS_IEFLAG_SUBNODE != 0 {
                                lookup_ctx = Some(ntfs_alloc(
                                    NtfsLookupCtx {
                                        aoff: aoff + u32::from(iep.reclen),
                                        rdsize,
                                        cn,
                                        prev: lookup_ctx,
                                    },
                                    M_TEMP,
                                ));
                                break 'scan;
                            } else {
                                break 'next;
                            }
                        }

                        if let Some(aname) = aname
                            && let Err(e) =
                                ntfs_ntlookupattr(ntmp, aname, &mut attrtype, &mut attrname)
                        {
                            break 'fail Err(e);
                        }

                        // Check if we've found ourselves
                        if iep.ie_number == ip.i_number.get()
                            && attrtype == fp.f_attrtype.get()
                            && attrnames_eq(
                                attrname.as_ref().map(AttrNameBuf::bytes),
                                fp.attrname(),
                            )
                        {
                            vref(vp);
                            *vpp = Some(vp);
                            break 'fail Ok(());
                        }

                        // free the buffer returned by ntfs_ntlookupattr()
                        if let Some(n) = attrname.take() {
                            attrname_free(n);
                        }

                        // vget node, but don't load it
                        let nvp = match ntfs_vgetex(
                            ntmp.mountp(),
                            iep.ie_number,
                            attrtype,
                            attrname,
                            LK_EXCLUSIVE,
                            VG_DONTLOADIN | VG_DONTVALIDFN,
                        ) {
                            Ok(nvp) => nvp,
                            Err(e) => break 'fail Err(e),
                        };

                        let nfp = VTOF(nvp);

                        if nfp.f_flag.get() & FN_VALID != 0 {
                            *vpp = Some(nvp);
                            break 'fail Ok(());
                        }

                        nfp.f_fflag.set(iep.ie_fflag as u32);
                        nfp.f_pnumber.set(iep.ie_fpnumber);
                        nfp.f_times.set(iep.ie_ftimes);

                        let default_attr =
                            nfp.f_attrtype.get() == NTFS_A_DATA && nfp.f_attrname.get().is_none();
                        let f_type =
                            if u64::from(nfp.f_fflag.get()) & NTFS_FFLAG_DIR != 0 && default_attr {
                                VDIR
                            } else {
                                VREG
                            };

                        nvp.v_type.set(f_type);

                        if default_attr {
                            // Opening default attribute
                            nfp.f_size.set(iep.ie_fsize);
                            nfp.f_allocated.set(iep.ie_fallocated);
                            nfp.f_flag.set(nfp.f_flag.get() | FN_PRELOADED);
                        } else {
                            match ntfs_filesize(ntmp, nfp) {
                                Ok((size, bytes)) => {
                                    nfp.f_size.set(size);
                                    nfp.f_allocated.set(bytes);
                                }
                                Err(e) => {
                                    vput(nvp);
                                    break 'fail Err(e);
                                }
                            }
                        }

                        nfp.f_flag.set(nfp.f_flag.get() & !FN_VALID);
                        *vpp = Some(nvp);
                        break 'fail Ok(());
                    }

                    // aoff += iep->reclen (an entry of no length ends the scan: the
                    // module's deviations)
                    if iep.reclen == 0 {
                        break 'scan;
                    }
                    aoff += u32::from(iep.reclen);
                    iep = AttrIndexentry::read(rd.as_slice(), aoff as usize);
                }

                // Dive if possible
                if iep.ie_flag & NTFS_IEFLAG_SUBNODE != 0 {
                    cn = cn_at(
                        rd.as_slice(),
                        (aoff as usize + usize::from(iep.reclen)).wrapping_sub(size_of::<Cn>()),
                    );
                    rdsize = blsize;

                    if let Err(e) = ntfs_readattr(
                        ntmp,
                        ip,
                        NTFS_A_INDX,
                        Some(b"$I30"),
                        ntmp.ntfs_cntob(cn),
                        rdsize as usize,
                        &mut Rdata::Mem(rd.as_mut_slice()),
                    ) {
                        break 'fail Err(e);
                    }

                    if let Err(e) =
                        ntfs_procfixups(ntmp, NTFS_INDXMAGIC, rd.as_mut_slice(), rdsize as usize)
                    {
                        break 'fail Err(e);
                    }

                    aoff = u32::from(AttrIndexalloc::read(rd.as_slice(), 0).ia_hdrsize) + 0x18;
                } else if let (true, Some(ctx)) = (fullscan, lookup_ctx) {
                    cn = ctx.cn;
                    aoff = ctx.aoff;
                    rdsize = ctx.rdsize;

                    if let Err(e) = ntfs_readattr(
                        ntmp,
                        ip,
                        if cn == 0 {
                            NTFS_A_INDXROOT
                        } else {
                            NTFS_A_INDX
                        },
                        Some(b"$I30"),
                        ntmp.ntfs_cntob(cn),
                        rdsize as usize,
                        &mut Rdata::Mem(rd.as_mut_slice()),
                    ) {
                        break 'fail Err(e);
                    }

                    if cn != 0
                        && let Err(e) = ntfs_procfixups(
                            ntmp,
                            NTFS_INDXMAGIC,
                            rd.as_mut_slice(),
                            rdsize as usize,
                        )
                    {
                        break 'fail Err(e);
                    }

                    lookup_ctx = ctx.prev;
                    // SAFETY: the context came from `ntfs_alloc(M_TEMP)` above and is off
                    // the chain now.
                    unsafe { ntfs_dealloc(ctx, M_TEMP) };
                } else {
                    break Err(Errno::ENOENT);
                }
            };

            if error == Err(Errno::ENOENT) {
                // perform full scan if no entry was found
                if !fullscan {
                    fullscan = true;
                    cn = 0; // need zero, used by lookup_ctx
                    continue 'loop_;
                }

                if cnp.cn_flags & ISLASTCN != 0
                    && (cnp.cn_nameiop == CREATE || cnp.cn_nameiop == RENAME)
                {
                    break 'fail Err(Errno::EJUSTRETURN);
                }
            }

            break 'fail error;
        }
    };

    // fail:
    if let Some(vap) = vap {
        ntfs_ntvattrrele(vap);
    }
    drop(rdbuf);
    if let Some(n) = attrname {
        attrname_free(n);
    }
    while let Some(tctx) = lookup_ctx {
        lookup_ctx = tctx.prev;
        // SAFETY: each context came from `ntfs_alloc(M_TEMP)` and is off the chain now.
        unsafe { ntfs_dealloc(tctx, M_TEMP) };
    }
    ntfs_ntput(ip);
    error
}

/// `ntfs_isnamepermitted`: check if name type is permitted to show.
pub fn ntfs_isnamepermitted(ntmp: &Ntfsmount, iep: &AttrIndexentry) -> bool {
    if ntmp.ntm_flag.get() & NTFS_MFLAG_ALLNAMES != 0 {
        return true;
    }

    match iep.ie_fnametype {
        2 => return false, // skipped DOS name
        0 | 1 | 3 => return true,
        t => {
            printf(format_args!(
                "ntfs_isnamepermitted: WARNING! Unknown file name type: {}\n",
                t
            ));
        }
    }
    false
}

/// `ntfs_ntreaddir`: read ntfs dir like stream of `attr_indexentry`, not like btree of them.
/// This is done by scanning `$BITMAP:$I30` for busy clusters and reading them. Of course
/// `$INDEX_ROOT:$I30` is read before. Last read values are stored in fnode, so we can skip
/// toward record number num almost immediately. Anyway this is rather slow routine. The
/// problem is that we don't know how many records are there in `$INDEX_ALLOCATION:$I30`
/// block. Returns the offset of entry `num` in `fp`'s `f_dirblbuf`, `None` past the last.
pub fn ntfs_ntreaddir(
    ntmp: &Ntfsmount,
    fp: &'static Fnode,
    num: u32,
    _p: Option<&Proc>,
) -> Result<Option<usize>, Errno> {
    let ip = FTONT(fp);
    let mut vap: Option<&'static Ntvattr> = None; // IndexRoot attribute
    let mut bmvap: Option<&'static Ntvattr> = None; // BitMap attribute
    let mut iavap: Option<&'static Ntvattr> = None; // IndexAllocation attribute
    let mut bmp: Option<KBuf> = None; // Bitmap
    let mut cpbl: u32 = 1; // Clusters per directory block

    ntfs_ntget(ip)?;

    let error: Result<Option<usize>, Errno> = 'fail: {
        let Ok(v) = ntfs_ntvattrget(ntmp, ip, NTFS_A_INDXROOT, Some(b"$I30"), 0) else {
            break 'fail Err(Errno::ENOTDIR);
        };
        vap = Some(v);
        let iroot = v.va_a_iroot();

        if fp.f_dirblbuf.get().is_none() {
            fp.f_dirblsz.set(iroot.ir_size);
            let len = (v.va_datalen as usize).max(iroot.ir_size as usize);
            let Some(b) = malloc(len, M_NTFSDIR, M_WAITOK | M_ZERO) else {
                panic(format_args!("ntfs_ntreaddir: no memory"));
            };
            fp.f_dirblbuf.set(Some(b));
            fp.f_dirblbuf_len.set(len);
        }

        let blsize = fp.f_dirblsz.get(); // Index allocation size (2048)
        // SAFETY: the ntnode is locked (`ntfs_ntget` above), so this is the only user of
        // the fnode's buffer, and no other slice of it is alive.
        let rdbuf = unsafe { fp.dirblbuf() };

        if iroot.ir_flag & NTFS_IRFLAG_INDXALLOC != 0 {
            let Ok(bv) = ntfs_ntvattrget(ntmp, ip, NTFS_A_INDXBITMAP, Some(b"$I30"), 0) else {
                break 'fail Err(Errno::ENOTDIR);
            };
            bmvap = Some(bv);
            let b = bmp.insert(KBuf::new(bv.va_datalen as usize, M_TEMP));
            if let Err(e) = ntfs_readattr(
                ntmp,
                ip,
                NTFS_A_INDXBITMAP,
                Some(b"$I30"),
                0,
                bv.va_datalen as usize,
                &mut Rdata::Mem(b.as_mut_slice()),
            ) {
                break 'fail Err(e);
            }

            let Ok(iv) = ntfs_ntvattrget(ntmp, ip, NTFS_A_INDX, Some(b"$I30"), 0) else {
                break 'fail Err(Errno::ENOTDIR);
            };
            iavap = Some(iv);
            cpbl = ntmp.ntfs_btocn(Off::from(blsize) + ntmp.ntfs_cntob(1) - 1) as u32;
        }

        // Try use previous values
        let (mut attrnum, mut aoff, mut blnum, mut cnum) =
            if fp.f_lastdnum.get() < num && fp.f_lastdnum.get() != 0 {
                (
                    fp.f_lastdattr.get(),
                    fp.f_lastdoff.get(),
                    fp.f_lastdblnum.get(),
                    fp.f_lastdnum.get(),
                )
            } else {
                (NTFS_A_INDXROOT, AttrIndexroot::SIZE as u32, 0, 0)
            };

        loop {
            let rdsize = if attrnum == NTFS_A_INDXROOT {
                v.va_datalen as u32
            } else {
                blsize
            };
            if let Err(e) = ntfs_readattr(
                ntmp,
                ip,
                attrnum,
                Some(b"$I30"),
                ntmp.ntfs_cntob(u64::from(blnum.wrapping_mul(cpbl))),
                rdsize as usize,
                &mut Rdata::Mem(&mut *rdbuf),
            ) {
                break 'fail Err(e);
            }

            if attrnum == NTFS_A_INDX
                && let Err(e) = ntfs_procfixups(ntmp, NTFS_INDXMAGIC, rdbuf, rdsize as usize)
            {
                break 'fail Err(e);
            }
            if aoff == 0 {
                aoff = if attrnum == NTFS_A_INDX {
                    0x18 + u32::from(AttrIndexalloc::read(rdbuf, 0).ia_hdrsize)
                } else {
                    AttrIndexroot::SIZE as u32
                };
            }

            let mut iep = AttrIndexentry::read(rdbuf, aoff as usize);
            while iep.ie_flag & NTFS_IEFLAG_LAST == 0 && rdsize > aoff {
                if ntfs_isnamepermitted(ntmp, &iep) {
                    if cnum >= num {
                        fp.f_lastdnum.set(cnum);
                        fp.f_lastdoff.set(aoff);
                        fp.f_lastdblnum.set(blnum);
                        fp.f_lastdattr.set(attrnum);

                        break 'fail Ok(Some(aoff as usize));
                    }
                    cnum += 1;
                }

                // An entry of no length ends the scan (the module's deviations).
                if iep.reclen == 0 {
                    break;
                }
                aoff += u32::from(iep.reclen);
                iep = AttrIndexentry::read(rdbuf, aoff as usize);
            }

            let Some(iv) = iavap else {
                break;
            };
            if attrnum == NTFS_A_INDXROOT {
                blnum = 0;
            } else {
                blnum += 1;
            }

            let bm = bmp.as_ref().map_or(&[][..], KBuf::as_slice);
            while (ntmp.ntfs_cntob(u64::from(blnum.wrapping_mul(cpbl))) as u64) < iv.va_datalen {
                let byte = bm.get((blnum >> 3) as usize).copied().unwrap_or(0);
                if byte & (1 << (blnum & 7)) != 0 {
                    break;
                }
                blnum += 1;
            }

            attrnum = NTFS_A_INDX;
            aoff = 0;
            if ntmp.ntfs_cntob(u64::from(blnum.wrapping_mul(cpbl))) as u64 >= iv.va_datalen {
                break;
            }
        }

        fp.f_lastdnum.set(0);
        Ok(None)
    };

    // fail:
    if let Some(v) = vap {
        ntfs_ntvattrrele(v);
    }
    if let Some(v) = bmvap {
        ntfs_ntvattrrele(v);
    }
    if let Some(v) = iavap {
        ntfs_ntvattrrele(v);
    }
    drop(bmp);
    ntfs_ntput(ip);

    error
}

/// `ntfs_nttimetounix(nt)`: convert NTFS times that are in 100 ns units and begins from
/// 1601 Jan 1 into unix times.
pub fn ntfs_nttimetounix(nt: u64) -> Timespec {
    // Windows NT times are in 100 ns and from 1601 Jan 1
    Timespec {
        tv_nsec: ((nt % (1000 * 1000 * 10)) * 100) as i64,
        tv_sec: (nt / (1000 * 1000 * 10))
            .wrapping_sub(369 * 365 * 24 * 60 * 60)
            .wrapping_sub(89 * 24 * 60 * 60) as i64,
    }
}

/// `ntfs_filesize(ntmp, fp, &size, &bytes)`: get file sizes from corresponding attribute:
/// its data length and its allocated size.
///
/// ntnode under fnode should be locked.
pub fn ntfs_filesize(ntmp: &Ntfsmount, fp: &Fnode) -> Result<(u64, u64), Errno> {
    let ip = FTONT(fp);

    let vap = ntfs_ntvattrget(ntmp, ip, fp.f_attrtype.get(), fp.attrname(), 0)?;

    let bn = vap.va_allocated;
    let sz = vap.va_datalen;

    ntfs_ntvattrrele(vap);

    Ok((sz, bn))
}

/// `ntfs_readntvattr_plain`: this is one of the read routines: `rsize` bytes of attribute
/// `vap` from `roff` to `rdata` (from its byte `doff`); `*initp` counts the bytes that were
/// read from the disk (holes are not).
///
/// ntnode should be locked.
#[allow(clippy::too_many_arguments)] // the C's arguments
pub fn ntfs_readntvattr_plain(
    ntmp: &Ntfsmount,
    ip: &Ntnode,
    vap: &Ntvattr,
    roff: Off,
    rsize: usize,
    rdata: &mut Rdata<'_, '_>,
    doff: usize,
    initp: &mut usize,
) -> Result<(), Errno> {
    let mut error = Ok(());

    *initp = 0;
    if vap.va_flag & NTFS_AF_INRUN != 0 {
        let mut data = doff;

        let mut off = roff;
        let mut left = rsize;
        let mut cnt = 0usize;
        let runcn = vap.va_vruncn();
        let runcl = vap.va_vruncl();
        while left != 0 && (cnt as u64) < vap.va_vruncnt() {
            let ccn = runcn[cnt];
            let mut ccl = runcl[cnt];

            if ntmp.ntfs_cntob(ccl) < off {
                off -= ntmp.ntfs_cntob(ccl);
                cnt += 1;
                continue;
            }
            if ccn != 0 || ip.i_number.get() == NTFS_BOOTINO {
                ccl = ccl.wrapping_sub(ntmp.ntfs_btocn(off));
                let mut cn = ccn.wrapping_add(ntmp.ntfs_btocn(off));
                off = ntmp.ntfs_btocnoff(off);

                while left != 0 && ccl != 0 {
                    // Always read single clusters at a time - we need to avoid reading
                    // differently-sized blocks at the same disk offsets to avoid confusing
                    // the buffer cache.
                    let tocopy = left.min((ntmp.ntfs_cntob(1) - off) as usize);
                    let cl = ntmp.ntfs_btocl(tocopy as Off + off);
                    kassert!(cl == 1 && tocopy as Off <= ntmp.ntfs_cntob(1));

                    let (bp, e) = bread(
                        ntmp.devvp(),
                        ntmp.ntfs_cntobn(cn),
                        ntmp.ntfs_cntob(cl) as i32,
                    );
                    if let Err(e) = e {
                        brelse(bp);
                        return Err(e);
                    }
                    let res = {
                        // SAFETY: the buffer is ours (busy from bread) and mapped; the
                        // slice dies before the release below.
                        let b = unsafe { bp.data() };
                        let src = b.get(off as usize..).unwrap_or(&[]);
                        let src = &src[..tocopy.min(src.len())];
                        rdata.copy(data, src)
                    };
                    brelse(bp);
                    if let Err(e) = res {
                        error = Err(e);
                        break;
                    }
                    data += tocopy;
                    *initp += tocopy;
                    off = 0;
                    left -= tocopy;
                    cn = cn.wrapping_add(cl);
                    ccl = ccl.wrapping_sub(cl);
                }
            } else {
                let tocopy = left.min((ntmp.ntfs_cntob(ccl) - off) as usize);
                left -= tocopy;
                off = 0;
                if let Err(e) = rdata.zero(data, tocopy) {
                    error = Err(e);
                }
                data += tocopy;
            }
            cnt += 1;
            if error.is_err() {
                break;
            }
        }
        if left != 0 && error.is_ok() {
            printf(format_args!("ntfs_readntvattr_plain: POSSIBLE RUN ERROR\n"));
            error = Err(Errno::E2BIG);
        }
    } else {
        let d = vap.va_datap();
        let start = usize::try_from(roff).unwrap_or(usize::MAX).min(d.len());
        let end = start.saturating_add(rsize).min(d.len());
        error = rdata.copy(doff, &d[start..end]);
        if error.is_ok() && end - start < rsize {
            // past the resident data (the module's deviations)
            error = rdata.zero(doff + (end - start), rsize - (end - start));
        }
        *initp += rsize;
    }

    error
}

/// `ntfs_readattr_plain`: this is one of read routines: `rsize` bytes of the attribute
/// `attrnum`/`attrname` from `roff`, over the records that map them.
#[allow(clippy::too_many_arguments)] // the C's arguments
pub fn ntfs_readattr_plain(
    ntmp: &Ntfsmount,
    ip: &'static Ntnode,
    attrnum: u32,
    attrname: Option<&[u8]>,
    roff: Off,
    rsize: usize,
    rdata: &mut Rdata<'_, '_>,
    doff: usize,
    initp: &mut usize,
) -> Result<(), Errno> {
    let mut off = roff;
    let mut left = rsize;
    let mut data = doff;
    let mut error = Ok(());
    *initp = 0;

    while left != 0 {
        let vap = ntfs_ntvattrget(ntmp, ip, attrnum, attrname, ntmp.ntfs_btocn(off))?;
        let toread = left.min((ntmp.ntfs_cntob(vap.va_vcnend.wrapping_add(1)) - off) as usize);
        let mut init = 0;
        if let Err(e) = ntfs_readntvattr_plain(
            ntmp,
            ip,
            vap,
            off - ntmp.ntfs_cntob(vap.va_vcnstart),
            toread,
            rdata,
            data,
            &mut init,
        ) {
            printf(format_args!(
                "ntfs_readattr_plain: ntfs_readntvattr_plain failed: o: {}, s: {}\n",
                off, toread
            ));
            printf(format_args!(
                "ntfs_readattr_plain: attrib: {} - {}\n",
                vap.va_vcnstart, vap.va_vcnend
            ));
            ntfs_ntvattrrele(vap);
            error = Err(e);
            break;
        }
        ntfs_ntvattrrele(vap);
        left -= toread;
        off += toread as Off;
        data += toread;
        *initp += init;
    }

    error
}

/// `ntfs_readattr`: this is one of read routines: `rsize` bytes of the attribute from
/// `roff` to `rdata`, decompressed if the attribute is compressed.
pub fn ntfs_readattr(
    ntmp: &Ntfsmount,
    ip: &'static Ntnode,
    attrnum: u32,
    attrname: Option<&[u8]>,
    roff: Off,
    rsize: usize,
    rdata: &mut Rdata<'_, '_>,
) -> Result<(), Errno> {
    let mut error = Ok(());
    let mut init = 0;

    let vap = ntfs_ntvattrget(ntmp, ip, attrnum, attrname, 0)?;

    if roff as u64 > vap.va_datalen || (roff as u64).wrapping_add(rsize as u64) > vap.va_datalen {
        printf(format_args!(
            "ntfs_readattr: offset too big: {} ({}) > {}\n",
            roff,
            roff.wrapping_add(rsize as Off),
            vap.va_datalen
        ));
        ntfs_ntvattrrele(vap);
        return Err(Errno::E2BIG);
    }
    if vap.va_compression != 0 && vap.va_compressalg != 0 {
        let unit = ntmp.ntfs_cntob(NTFS_COMPUNIT_CL) as usize;
        let mut data = 0usize;
        let mut left = rsize;

        let mut cup = KBuf::new(unit, crate::sys::malloc::M_NTFSDECOMP);
        let mut uup = KBuf::new(unit, crate::sys::malloc::M_NTFSDECOMP);

        let mut cn = ntmp.ntfs_btocn(roff) & !(NTFS_COMPUNIT_CL - 1);
        let mut off = roff - ntmp.ntfs_cntob(cn);

        while left != 0 {
            if let Err(e) = ntfs_readattr_plain(
                ntmp,
                ip,
                attrnum,
                attrname,
                ntmp.ntfs_cntob(cn),
                unit,
                &mut Rdata::Mem(cup.as_mut_slice()),
                0,
                &mut init,
            ) {
                error = Err(e);
                break;
            }

            let tocopy = left.min(unit - off as usize);

            let o = off as usize;
            if init == unit {
                error = rdata.copy(data, &cup.as_slice()[o..o + tocopy]);
            } else if init == 0 {
                error = rdata.zero(data, tocopy);
            } else {
                if let Err(e) = ntfs_uncompunit(ntmp, uup.as_mut_slice(), cup.as_slice()) {
                    error = Err(e);
                    break;
                }
                error = rdata.copy(data, &uup.as_slice()[o..o + tocopy]);
            }
            if error.is_err() {
                break;
            }

            left -= tocopy;
            data += tocopy;
            off = off + tocopy as Off - unit as Off;
            cn += NTFS_COMPUNIT_CL;
        }
    } else {
        error = ntfs_readattr_plain(
            ntmp, ip, attrnum, attrname, roff, rsize, rdata, 0, &mut init,
        );
    }
    ntfs_ntvattrrele(vap);
    error
}

/// `ntfs_procfixups`: process fixup routine on given buffer: check the record's magic and
/// put back the last two bytes of each sector from the update sequence array.
pub fn ntfs_procfixups(
    ntmp: &Ntfsmount,
    magic: u32,
    buf: &mut [u8],
    len: usize,
) -> Result<(), Errno> {
    let fhp = Fixuphdr::read(buf, 0);
    let bps = usize::from(ntmp.ntm_bps());

    if { fhp.fh_magic } != magic {
        printf(format_args!(
            "ntfs_procfixups: magic doesn't match: {:08x} != {:08x}\n",
            { fhp.fh_magic },
            magic
        ));
        return Err(Errno::EINVAL);
    }
    if (i64::from(fhp.fh_fnum) - 1) * bps as i64 != len as i64 {
        printf(format_args!(
            "ntfs_procfixups: bad fixups number: {} for {} bytes block\n",
            { fhp.fh_fnum },
            len
        ));
        return Err(Errno::EINVAL);
    }
    if usize::from(fhp.fh_foff)
        >= usize::from(ntmp.ntm_spc()) * usize::from(ntmp.ntm_mftrecsz()) * bps
    {
        printf(format_args!("ntfs_procfixups: invalid offset: {:x}", {
            fhp.fh_foff
        }));
        return Err(Errno::EINVAL);
    }
    let rd = |buf: &[u8], o: usize| -> u16 {
        match buf.get(o..o + 2) {
            Some(b) => u16::from_ne_bytes([b[0], b[1]]),
            None => 0,
        }
    };
    let mut fxp = usize::from(fhp.fh_foff);
    let mut cfxp = bps.wrapping_sub(2);
    let fixup = rd(buf, fxp);
    fxp += 2;
    for i in 1..usize::from(fhp.fh_fnum) {
        if rd(buf, cfxp) != fixup {
            printf(format_args!("ntfs_procfixups: fixup {} doesn't match\n", i));
            return Err(Errno::EINVAL);
        }
        let v = rd(buf, fxp);
        if let Some(b) = buf.get_mut(cfxp..cfxp + 2) {
            b.copy_from_slice(&v.to_ne_bytes());
        }
        cfxp += bps;
        fxp += 2;
    }
    Ok(())
}

/// `ntfs_toupper_use`: if the `ntfs_toupper_tab[]` is filled already, just raise use count;
/// otherwise read the data from the filesystem we are currently mounting.
pub fn ntfs_toupper_use(mp: &'static Mount, ntmp: &Ntfsmount, _p: &Proc) -> Result<(), Errno> {
    let mut error = Ok(());

    // get exclusive access
    rw_enter_write(&NTFS_TOUPPER_LOCK);

    // only read the translation data from a file if it hasn't been read already
    if NTFS_TOUPPER_TAB.load(Ordering::Acquire).is_null() {
        // Read in Unicode lowercase -> uppercase translation file. XXX for now, just the
        // first 256 entries are used anyway, so don't bother reading more
        let Some(tab) = malloc(NTFS_TOUPPER_SIZE, M_NTFSRDATA, M_WAITOK | M_ZERO) else {
            panic(format_args!("ntfs_toupper_use: no memory"));
        };
        NTFS_TOUPPER_TAB.store(tab.as_ptr().cast::<Wchar>(), Ordering::Release);

        error = VFS_VGET(mp, u64::from(NTFS_UPCASEINO)).and_then(|vp| {
            // SAFETY: the table is `NTFS_TOUPPER_SIZE` zeroed bytes just allocated, written
            // only here, under `ntfs_toupper_lock`; nothing reads it until the lock drops.
            let t = unsafe { core::slice::from_raw_parts_mut(tab.as_ptr(), NTFS_TOUPPER_SIZE) };
            let e = ntfs_readattr(
                ntmp,
                VTONT(vp),
                NTFS_A_DATA,
                None,
                0,
                NTFS_TOUPPER_SIZE,
                &mut Rdata::Mem(t),
            );
            vput(vp);
            e
        });
    }

    // out:
    NTFS_TOUPPER_USECOUNT.fetch_add(1, Ordering::Relaxed);
    rw_exit_write(&NTFS_TOUPPER_LOCK);
    error
}

/// `ntfs_toupper_unuse`: lower the use count and if it reaches zero, free the memory tied by
/// toupper table.
pub fn ntfs_toupper_unuse(_p: Option<&Proc>) {
    // get exclusive access
    rw_enter_write(&NTFS_TOUPPER_LOCK);

    let count = NTFS_TOUPPER_USECOUNT.fetch_sub(1, Ordering::Relaxed) - 1;
    if count == 0 {
        let tab = NTFS_TOUPPER_TAB.swap(ptr::null_mut(), Ordering::AcqRel);
        if let Some(t) = NonNull::new(tab) {
            free(t.cast(), M_NTFSRDATA, NTFS_TOUPPER_SIZE);
        }
    }
    #[cfg(feature = "diagnostic")]
    if count < 0 {
        panic(format_args!(
            "ntfs_toupper_unuse(): use count negative: {}",
            count
        ));
    }

    // release the lock
    rw_exit_write(&NTFS_TOUPPER_LOCK);
}

#[cfg(test)]
mod tests;
