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
 * Copyright (c) 2017 Patrick Wildt <patrick@blueri.se>
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
 * Copyright (c) 2017 Patrick Wildt <patrick@blueri.se>
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
//! Network boot: find the PXE interface the firmware booted us through and read files from
//! its TFTP server (`tftp:` names), whole, through the PXE base code's MTFTP.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/efipxe.c @ 3ce1f3f79392,
//! sys/arch/amd64/stand/efiboot/efipxe.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct tftp_handle` is owned by `f_fsdata`; `PXE`, `bootip`, `servip` and `boothw` are
//!   cells set by `efi_pxeprobe`.
//! - `tftp_fs` (an unused second `fs_ops` for TFTP) is not declared: `conf.rs`'s table names
//!   the routines.
//! - Not tested by a smoke yet (QEMU's PXE boot needs a TFTP-served efiboot).

use alloc::boxed::Box;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use efi::include::efi::*;
use libkern::staticcell::StaticCell;
use libsa::dev::set_errno;
use libsa::hdr::stat::Stat;
use libsa::hdr::types::{Daddr, Off};
use libsa::saerrno::Errno;
use libsa::stand::{OpenFile, SEEK_CUR, SEEK_END, SEEK_SET};

use crate::diskprobe::BOOTDEV_DIP;
use crate::efiboot::{
    EFI_BOOTDP, bs, efi_device_path_depth, efi_device_path_ncmp, handle_protocol,
};
use crate::exec_i386::BOOTMAC;

/// `boothw`: the hardware address of the boot interface.
static BOOTHW: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// `bootip`, `servip`.
static SERVIP: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// `PXE`: the PXE base code of the boot interface, null if we did not boot from the net.
static PXE: AtomicPtr<EfiPxeBaseCode> = AtomicPtr::new(ptr::null_mut());

/// `efi_pxeprobe()`: TFTP initial probe. Discover the PXE handles and see whether one of
/// them did the PXE handshake we were booted through; if so, remember it (`PXE`).
pub fn efi_pxeprobe() {
    let bootdp = EFI_BOOTDP.load(Ordering::Relaxed);
    if bootdp.is_null() {
        return;
    }

    let mut guid = EFI_PXE_BASE_CODE_PROTOCOL;
    let mut nhandles: UINTN = 0;
    let mut handles: *mut EfiHandle = ptr::null_mut();
    // SAFETY: a boot service with valid pointers.
    let status = unsafe {
        (bs().LocateHandleBuffer)(
            ByProtocol,
            &mut guid,
            ptr::null_mut(),
            &mut nhandles,
            &mut handles,
        )
    };
    if status != EFI_SUCCESS {
        return;
    }

    for i in 0..nhandles {
        // SAFETY: the firmware's buffer of `nhandles` handles.
        let h = unsafe { *handles.add(i) };
        let Ok(dp0) = handle_protocol::<EfiDevicePath>(h, DEVICE_PATH_PROTOCOL) else {
            continue;
        };

        let depth = efi_device_path_depth(bootdp, MESSAGING_DEVICE_PATH);
        if depth == -1 || efi_device_path_ncmp(bootdp, dp0, depth) != 0 {
            continue;
        }

        let Ok(pxe) = handle_protocol::<EfiPxeBaseCode>(h, EFI_PXE_BASE_CODE_PROTOCOL) else {
            continue;
        };

        // SAFETY: the firmware's PXE base code protocol and its mode.
        unsafe {
            if (*pxe).Mode.is_null() {
                continue;
            }

            let dhcp = &(*(*pxe).Mode).DhcpAck.Dhcpv4;
            let servip = SERVIP.get_mut();
            servip[..4].copy_from_slice(&dhcp.BootpSiAddr);
            let boothw = BOOTHW.get_mut();
            boothw.copy_from_slice(&dhcp.BootpHwAddr);
            BOOTMAC.store(boothw.as_mut_ptr(), Ordering::Relaxed);
        }
        PXE.store(pxe, Ordering::Relaxed);

        // It is expected that bootdev_dip exists. Usually efiopen() sets the pointer.
        // Create a fake disk for the TFTP case.
        let mut dip = crate::efidev::efid_init_none();
        dip.disklabel.d_uid = [0xff; 8];
        BOOTDEV_DIP.store(Box::leak(dip), Ordering::Relaxed);
        break;
    }
}

/// `struct tftp_handle`: the whole file, read at open.
struct TftpHandle {
    /// `inbuf`: input buffer (EFI pages), null for an empty file.
    inbuf: *mut u8,
    /// `inbufsize`.
    inbufsize: usize,
    /// `inbufoff`.
    inbufoff: Off,
}

// SAFETY: efiboot is single-threaded; the buffer is the firmware's pages.
unsafe impl Send for TftpHandle {}

/// The open file's TFTP state.
fn handle(f: &mut OpenFile) -> Result<&mut TftpHandle, Errno> {
    f.fsdata::<TftpHandle>().ok_or(Errno::EBADF)
}

/// `Mtftp` with a NUL-terminated copy of `path`.
fn mtftp(
    pxe: *mut EfiPxeBaseCode,
    op: EfiPxeBaseCodeTftpOpcode,
    buf: *mut c_void,
    size: &mut u64,
    path: &[u8],
) -> EfiStatus {
    let mut name = alloc::vec::Vec::from(path);
    name.push(0);
    let mut servip: EfiIpAddress = EfiIpAddress { Addr: [0; 4] };
    // SAFETY: SERVIP is only written by efi_pxeprobe, earlier; the union is plain bytes.
    unsafe {
        let ip = SERVIP.read();
        ptr::copy_nonoverlapping(ip.as_ptr(), (&raw mut servip).cast::<u8>(), 16);
        ((*pxe).Mtftp)(
            pxe,
            op,
            buf,
            FALSE,
            size,
            ptr::null_mut(),
            &mut servip,
            name.as_mut_ptr(),
            ptr::null_mut(),
            FALSE,
        )
    }
}

/// `tftp_open(path, f)`: read the whole file from the TFTP server.
pub fn tftp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_dev.is_none_or(|d| d.dv_name != "TFTP") {
        return Err(Errno::ENXIO);
    }

    let pxe = PXE.load(Ordering::Relaxed);
    if pxe.is_null() {
        return Err(Errno::ENXIO);
    }
    let path = &path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())];

    let mut size: u64 = 0;
    if mtftp(
        pxe,
        EFI_PXE_BASE_CODE_TFTP_GET_FILE_SIZE,
        ptr::null_mut(),
        &mut size,
        path,
    ) != EFI_SUCCESS
    {
        return Err(Errno::ENOENT);
    }
    let mut tftpfile = TftpHandle {
        inbuf: ptr::null_mut(),
        inbufsize: size as usize,
        inbufoff: 0,
    };

    if tftpfile.inbufsize != 0 {
        let mut addr: EfiPhysicalAddress = 0;
        // SAFETY: a boot service with a valid pointer.
        let status = unsafe {
            (bs().AllocatePages)(
                AllocateAnyPages,
                EfiLoaderData,
                efi_size_to_pages(tftpfile.inbufsize),
                &mut addr,
            )
        };
        if status != EFI_SUCCESS {
            return Err(Errno::ENOMEM);
        }
        tftpfile.inbuf = addr as usize as *mut u8;

        if mtftp(
            pxe,
            EFI_PXE_BASE_CODE_TFTP_READ_FILE,
            tftpfile.inbuf.cast(),
            &mut size,
            path,
        ) != EFI_SUCCESS
        {
            return Err(Errno::ENXIO);
        }
    }
    f.f_fsdata = Some(Box::new(tftpfile));
    Ok(())
}

/// `tftp_close(f)`.
pub fn tftp_close(f: &mut OpenFile) -> Result<(), Errno> {
    let t = handle(f)?;
    if !t.inbuf.is_null() {
        // SAFETY: the pages tftp_open allocated.
        unsafe { (bs().FreePages)(t.inbuf as u64, efi_size_to_pages(t.inbufsize)) };
    }
    f.f_fsdata = None;
    Ok(())
}

/// `tftp_read(f, addr, size, &resid)`.
pub fn tftp_read(f: &mut OpenFile, addr: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    let t = handle(f)?;
    let left = t.inbufsize - t.inbufoff as usize;
    let toread = addr.len().min(left);

    if toread != 0 {
        // SAFETY: `inbuf` holds `inbufsize` bytes; `inbufoff + toread` is within them.
        unsafe {
            ptr::copy_nonoverlapping(t.inbuf.add(t.inbufoff as usize), addr.as_mut_ptr(), toread);
        }
        t.inbufoff += toread as Off;
    }

    *resid = addr.len() - toread;
    Ok(())
}

/// `tftp_write`.
pub fn tftp_write(_f: &mut OpenFile, _start: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

/// `tftp_seek(f, offset, where)`.
pub fn tftp_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    let t = handle(f)?;
    let size = t.inbufsize as Off;
    match whence {
        SEEK_CUR => {
            if t.inbufoff + offset < 0 || t.inbufoff + offset > size {
                set_errno(Errno::EOFFSET);
                return Err(Errno::EOFFSET);
            }
            t.inbufoff += offset;
            Ok(t.inbufoff)
        }
        SEEK_SET => {
            if offset < 0 || offset > size {
                set_errno(Errno::EOFFSET);
                return Err(Errno::EOFFSET);
            }
            t.inbufoff = offset;
            Ok(t.inbufoff)
        }
        SEEK_END => {
            t.inbufoff = size;
            Ok(t.inbufoff)
        }
        _ => {
            set_errno(Errno::EINVAL);
            Err(Errno::EINVAL)
        }
    }
}

/// `tftp_stat(f, sb)`.
pub fn tftp_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    let t = handle(f)?;
    sb.st_mode = 0o444;
    sb.st_nlink = 1;
    sb.st_uid = 0;
    sb.st_gid = 0;
    sb.st_size = t.inbufsize as Off;
    Ok(())
}

/// `tftp_readdir`.
pub fn tftp_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `tftpopen(f, &fname)`: the dummy TFTP network device; claims `tftp:name`.
pub fn tftpopen(_f: &mut OpenFile, fname: &mut &[u8]) -> Result<(), Errno> {
    // No PXE set -> no PXE available
    if PXE.load(Ordering::Relaxed).is_null() {
        return Err(Errno(1));
    }

    // Parse tftp:bsd into "tftp" and "bsd"
    let name = *fname;
    let Some(p) = name
        .iter()
        .position(|&c| c == b':' || c == 0)
        .filter(|&p| name[p] == b':')
    else {
        return Err(Errno(1));
    };
    // strncmp(*fname, "tftp", p - *fname)
    if !b"tftp".iter().zip(&name[..p]).all(|(a, b)| a == b) || p > 4 {
        return Err(Errno(1));
    }

    *fname = &name[p + 1..];
    Ok(())
}

/// `tftpclose(f)`.
pub fn tftpclose(_f: &mut OpenFile) -> Result<(), Errno> {
    Ok(())
}

/// `tftpioctl`.
pub fn tftpioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `tftpstrategy`.
pub fn tftpstrategy(
    _devdata: *mut c_void,
    _rw: i32,
    _blk: Daddr,
    _buf: &mut [u8],
    _rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}
/* </CODE> */
