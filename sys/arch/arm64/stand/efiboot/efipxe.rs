/* <LICENSES> */
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
//! Network boot: find the PXE interface the firmware booted us through, and read files
//! from its TFTP server (`tftp0a:` names): whole, through the PXE base code's MTFTP when
//! the firmware has one, else block by block with libsa's TFTP client over the `efinet`
//! interface driver (the EFI Simple Network Protocol).
//!
//! Upstream: sys/arch/arm64/stand/efiboot/efipxe.c @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/efipxe.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct mtftp_handle` is owned by `f_fsdata`; `NET`, `PXE`, `bootip`, `boothw`, `txbuf`
//!   and `use_mtftp` are atomics and cells set by `efi_pxeprobe` and `tftpopen`; `servip`
//!   and `gateip` are libsa's (`tftp.rs`, `globals.rs`), as the C's `extern`s.
//! - `tftpdev_sock` is an `AtomicUsize` (libsa's sockets are `usize`), `usize::MAX` for the
//!   C's -1; `f_devdata` points at it, as the C.
//! - `tftpopen` gets the C's variadic unit from the device part of the name
//!   ([`devopen_args`]).
//! - The `efinet` routines take references and slices (libsa's `netif.rs`); `efinet_get`
//!   and `efinet_put` return `Err(EIO)` where the C returns -1.
//! - `PXE->Mtftp` is read as an address to test it for NULL, as the C does (the EFI type is
//!   a function pointer, which cannot be null).
//! - Not covered by a smoke yet (QEMU's PXE boot needs a TFTP-served efiboot).

use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use efi::include::efi::*;
use libkern::staticcell::StaticCell;
use libsa::dev::set_errno;
use libsa::globals::GATEIP;
use libsa::hdr::if_ether::{ETHER_ALIGN, ETHER_CRC_LEN, ETHER_HDR_LEN};
use libsa::hdr::stat::Stat;
use libsa::hdr::types::{Daddr, Off, Time};
use libsa::iodesc::IoDesc;
use libsa::net::RECV_SIZE;
use libsa::netif::{Netif, NetifDif, NetifDriver, NetifStats, netif_close, netif_open};
use libsa::saerrno::Errno;
use libsa::stand::{OpenFile, SEEK_CUR, SEEK_END, SEEK_SET};
use libsa::tftp::{SERVIP, tftp_open};

use crate::efiboot::{
    BOOTMAC, EFI_BOOTDP, bs, devopen_args, efi_device_path_depth, efi_device_path_ncmp, getsecs,
    handle_protocol,
};

/// `struct mtftp_handle`: the whole file, read at open.
struct MtftpHandle {
    /// `inbuf`: input buffer (EFI pages), null for an empty file.
    inbuf: *mut u8,
    /// `inbufsize`.
    inbufsize: usize,
    /// `inbufoff`.
    inbufoff: Off,
}

// SAFETY: efiboot is single-threaded; the buffer is the firmware's pages.
unsafe impl Send for MtftpHandle {}

/// `boothw[16]`: the hardware address of the boot interface.
static BOOTHW: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// `bootip`: our address, network order.
static BOOTIP: AtomicU32 = AtomicU32::new(0);
/// `NET`: the Simple Network Protocol of the boot interface.
static NET: AtomicPtr<EfiSimpleNetwork> = AtomicPtr::new(ptr::null_mut());
/// `PXE`: the PXE base code of the boot interface, null if we did not boot from the net.
static PXE: AtomicPtr<EfiPxeBaseCode> = AtomicPtr::new(ptr::null_mut());
/// `txbuf`: the transmit buffer (EFI pages).
static TXBUF: AtomicU64 = AtomicU64::new(0);
/// `use_mtftp`: the PXE base code reads files for us.
static USE_MTFTP: AtomicBool = AtomicBool::new(false);
/// `tftpdev_sock`: the socket of the dummy network device, `usize::MAX` for none.
static TFTPDEV_SOCK: AtomicUsize = AtomicUsize::new(usize::MAX);

/// `efinet_stats`.
static EFINET_STATS: NetifStats = NetifStats::new();
/// `efinet_ifs[]`.
static EFINET_IFS: [NetifDif; 1] = [NetifDif::new(0, 1, &EFINET_STATS, ptr::null_mut())];
/// `efinet_driver`: the Simple Network Protocol driver.
pub static EFINET_DRIVER: NetifDriver = NetifDriver {
    netif_bname: "efinet",
    netif_match: efinet_match,
    netif_probe: efinet_probe,
    netif_init: efinet_init,
    netif_get: efinet_get,
    netif_put: efinet_put,
    netif_end: efinet_end,
    netif_ifs: &EFINET_IFS,
};

/// `efi_pxeprobe()`: TFTP initial probe. Discover PXE handles and try to figure out if
/// there has already been a successful PXE handshake. If so, set the PXE variable.
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

        let Ok(net) = handle_protocol::<EfiSimpleNetwork>(h, EFI_SIMPLE_NETWORK_PROTOCOL) else {
            continue;
        };

        let Ok(pxe) = handle_protocol::<EfiPxeBaseCode>(h, EFI_PXE_BASE_CODE_PROTOCOL) else {
            continue;
        };

        // SAFETY: the firmware's PXE base code protocol and its mode; `Mtftp` is read as an
        // address first, as the C tests it for NULL.
        unsafe {
            if (*pxe).Mode.is_null() {
                continue;
            }

            if ptr::addr_of!((*pxe).Mtftp).cast::<usize>().read() != 0 {
                let status = ((*pxe).Mtftp)(
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    FALSE,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    FALSE,
                );
                if status != EFI_UNSUPPORTED {
                    USE_MTFTP.store(true, Ordering::Relaxed);
                }
            }

            let dhcp = &(*(*pxe).Mode).DhcpAck.Dhcpv4;
            BOOTIP.store(u32::from_ne_bytes(dhcp.BootpYiAddr), Ordering::Relaxed);
            SERVIP.store(u32::from_ne_bytes(dhcp.BootpSiAddr), Ordering::Relaxed);
            GATEIP.store(u32::from_ne_bytes(dhcp.BootpSiAddr), Ordering::Relaxed);
            let boothw = BOOTHW.get_mut();
            boothw.copy_from_slice(&dhcp.BootpHwAddr);
            BOOTMAC.store(boothw.as_mut_ptr(), Ordering::Relaxed);
        }
        NET.store(net, Ordering::Relaxed);
        PXE.store(pxe, Ordering::Relaxed);
        break;
    }
}

/// The open file's MTFTP state.
fn handle(f: &mut OpenFile) -> Result<&mut MtftpHandle, Errno> {
    f.fsdata::<MtftpHandle>().ok_or(Errno::EBADF)
}

/// `PXE->Mtftp(PXE, op, buf, FALSE, &size, NULL, &dstip, path, NULL, FALSE)`, with a
/// NUL-terminated copy of `path` and the server's address.
fn mtftp(
    pxe: *mut EfiPxeBaseCode,
    op: EfiPxeBaseCodeTftpOpcode,
    buf: *mut c_void,
    size: &mut u64,
    path: &[u8],
) -> EfiStatus {
    let mut name = alloc::vec::Vec::from(path);
    name.push(0);
    let mut dstip: EfiIpAddress = EfiIpAddress { Addr: [0; 4] };
    let servip = SERVIP.load(Ordering::Relaxed).to_ne_bytes();
    // SAFETY: the union is plain bytes; the PXE base code is the firmware's.
    unsafe {
        ptr::copy_nonoverlapping(servip.as_ptr(), (&raw mut dstip).cast::<u8>(), 4);
        ((*pxe).Mtftp)(
            pxe,
            op,
            buf,
            FALSE,
            size,
            ptr::null_mut(),
            &mut dstip,
            name.as_mut_ptr(),
            ptr::null_mut(),
            FALSE,
        )
    }
}

/// `mtftp_open(path, f)`: read the whole file from the TFTP server.
pub fn mtftp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_dev.is_none_or(|d| d.dv_name != "tftp") {
        return Err(Errno::ENXIO);
    }

    let pxe = PXE.load(Ordering::Relaxed);
    if pxe.is_null() {
        return Err(Errno::ENXIO);
    }

    if !USE_MTFTP.load(Ordering::Relaxed) {
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
    let mut tftpfile = MtftpHandle {
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

/// `mtftp_close(f)`.
pub fn mtftp_close(f: &mut OpenFile) -> Result<(), Errno> {
    let t = handle(f)?;
    if !t.inbuf.is_null() {
        // SAFETY: the pages mtftp_open allocated.
        unsafe { (bs().FreePages)(t.inbuf as u64, efi_size_to_pages(t.inbufsize)) };
    }
    f.f_fsdata = None;
    Ok(())
}

/// `mtftp_read(f, addr, size, &resid)`.
pub fn mtftp_read(f: &mut OpenFile, addr: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
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

/// `mtftp_write`.
pub fn mtftp_write(_f: &mut OpenFile, _start: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

/// `mtftp_seek(f, offset, where)`.
pub fn mtftp_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
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

/// `mtftp_stat(f, sb)`.
pub fn mtftp_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    let t = handle(f)?;
    sb.st_mode = 0o444;
    sb.st_nlink = 1;
    sb.st_uid = 0;
    sb.st_gid = 0;
    sb.st_size = t.inbufsize as Off;
    Ok(())
}

/// `mtftp_readdir`.
pub fn mtftp_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `efitftp_open(path, f)`: overload generic TFTP implementation to check that we actually
/// have a driver.
pub fn efitftp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_dev.is_none_or(|d| d.dv_name != "tftp") {
        return Err(Errno::ENXIO);
    }

    if NET.load(Ordering::Relaxed).is_null() || PXE.load(Ordering::Relaxed).is_null() {
        return Err(Errno::ENXIO);
    }

    if USE_MTFTP.load(Ordering::Relaxed) {
        return Err(Errno::ENXIO);
    }

    tftp_open(path, f)
}

/// `tftpopen(f, unit)`: the dummy network device; without MTFTP, a socket on `efinet`.
pub fn tftpopen(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let (unit, _) = devopen_args(file);

    // No PXE set -> no PXE available
    if PXE.load(Ordering::Relaxed).is_null() {
        return Err(Errno(1));
    }

    if unit != 0 {
        return Err(Errno(1));
    }

    if !USE_MTFTP.load(Ordering::Relaxed) {
        let mut txbuf: EfiPhysicalAddress = 0;
        // SAFETY: a boot service with a valid pointer.
        let status = unsafe {
            (bs().AllocatePages)(
                AllocateAnyPages,
                EfiLoaderData,
                efi_size_to_pages(RECV_SIZE),
                &mut txbuf,
            )
        };
        if status != EFI_SUCCESS {
            return Err(Errno::ENOMEM);
        }
        TXBUF.store(txbuf, Ordering::Relaxed);

        match netif_open(b"efinet") {
            Ok(sock) => TFTPDEV_SOCK.store(sock, Ordering::Relaxed),
            Err(_) => {
                TFTPDEV_SOCK.store(usize::MAX, Ordering::Relaxed);
                // SAFETY: the pages just allocated.
                unsafe { (bs().FreePages)(txbuf, efi_size_to_pages(RECV_SIZE)) };
                return Err(Errno::ENXIO);
            }
        }

        f.f_devdata = TFTPDEV_SOCK.as_ptr().cast();
    }

    Ok(())
}

/// `tftpclose(f)`.
pub fn tftpclose(f: &mut OpenFile) -> Result<(), Errno> {
    let mut ret = Ok(());

    if !USE_MTFTP.load(Ordering::Relaxed) {
        // SAFETY: tftpopen pointed `f_devdata` at TFTPDEV_SOCK, a usize.
        let sock = unsafe { *f.f_devdata.cast::<usize>() };
        ret = netif_close(sock);
        // SAFETY: the pages tftpopen allocated.
        unsafe { (bs().FreePages)(TXBUF.load(Ordering::Relaxed), efi_size_to_pages(RECV_SIZE)) };
        TXBUF.store(0, Ordering::Relaxed);
    }

    ret
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

/// `efinet_match(nif, v)`.
fn efinet_match(_nif: &mut Netif, _v: &[u8]) -> i32 {
    1
}

/// `efinet_probe(nif, v)`: `strncmp(v, efinet_driver.netif_bname, 3)`.
fn efinet_probe(_nif: &mut Netif, v: &[u8]) -> i32 {
    let name = EFINET_DRIVER.netif_bname.as_bytes();
    for i in 0..3 {
        let (a, b) = (
            v.get(i).copied().unwrap_or(0),
            name.get(i).copied().unwrap_or(0),
        );
        if a != b {
            return -1;
        }
        if a == 0 {
            break;
        }
    }
    0
}

/// `efinet_init(desc, v)`: start the interface, receive unicast and broadcast, and fill
/// in our addresses.
fn efinet_init(desc: &mut IoDesc, _v: &[u8]) {
    let net = NET.load(Ordering::Relaxed);
    if net.is_null() {
        return;
    }

    // SAFETY: the firmware's Simple Network Protocol and its mode.
    unsafe {
        if (*(*net).Mode).State == EfiSimpleNetworkStopped && ((*net).Start)(net) != EFI_SUCCESS {
            return;
        }

        if (*(*net).Mode).State != EfiSimpleNetworkInitialized
            && ((*net).Initialize)(net, 0, 0) != EFI_SUCCESS
        {
            return;
        }

        ((*net).ReceiveFilters)(
            net,
            EFI_SIMPLE_NETWORK_RECEIVE_UNICAST | EFI_SIMPLE_NETWORK_RECEIVE_BROADCAST,
            0,
            FALSE,
            0,
            ptr::null_mut(),
        );

        let mode = &*(*net).Mode;
        desc.myea.copy_from_slice(&mode.CurrentAddress.Addr[..6]);
    }
    desc.myip.s_addr = BOOTIP.load(Ordering::Relaxed);
    desc.xid = 1;
}

/// `efinet_get(desc, pkt, len, tmo)`: a frame, waiting at most `tmo` seconds.
fn efinet_get(_desc: &mut IoDesc, pkt: &mut [u8], tmo: Time) -> Result<usize, Errno> {
    let net = NET.load(Ordering::Relaxed);
    if net.is_null() {
        return Err(Errno::EIO);
    }

    // SAFETY: the firmware's Simple Network Protocol and its mode.
    let bufsz = unsafe { (*(*net).Mode).MaxPacketSize } as usize + ETHER_HDR_LEN + ETHER_CRC_LEN;
    let mut buf = vec![0u8; bufsz + ETHER_ALIGN];
    let ptr = buf[ETHER_ALIGN..].as_mut_ptr();

    let t = getsecs();
    let mut status = EFI_NOT_READY;
    let mut pktsz: UINTN = 0;
    while (getsecs() - t) < tmo {
        pktsz = bufsz;
        // SAFETY: the protocol and a buffer of `pktsz` bytes.
        status = unsafe {
            ((*net).Receive)(
                net,
                ptr::null_mut(),
                &mut pktsz,
                ptr.cast(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == EFI_SUCCESS {
            break;
        }
        if status != EFI_NOT_READY {
            break;
        }
    }

    if status == EFI_SUCCESS {
        let n = pktsz.min(pkt.len()).min(bufsz);
        pkt[..n].copy_from_slice(&buf[ETHER_ALIGN..ETHER_ALIGN + n]);
        return Ok(n);
    }

    Err(Errno::EIO)
}

/// `efinet_put(desc, pkt, len)`: send a frame and wait until the interface is done with
/// it.
fn efinet_put(_desc: &mut IoDesc, pkt: &[u8]) -> Result<usize, Errno> {
    let net = NET.load(Ordering::Relaxed);
    if net.is_null() {
        return Err(Errno::EIO);
    }

    if pkt.len() > RECV_SIZE {
        return Err(Errno::EIO);
    }

    let txbuf = TXBUF.load(Ordering::Relaxed) as usize as *mut u8;
    // SAFETY: `txbuf` is RECV_SIZE bytes of pages tftpopen allocated; the protocol is the
    // firmware's.
    unsafe {
        ptr::copy_nonoverlapping(pkt.as_ptr(), txbuf, pkt.len());
        let mut status = ((*net).Transmit)(
            net,
            0,
            pkt.len(),
            txbuf.cast(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        if status != EFI_SUCCESS {
            return Err(Errno::EIO);
        }

        let mut buf: *mut c_void = ptr::null_mut();
        while status == EFI_SUCCESS {
            status = ((*net).GetStatus)(net, ptr::null_mut(), &mut buf);
            if !buf.is_null() {
                break;
            }
        }

        if status == EFI_SUCCESS {
            return Ok(pkt.len());
        }
    }

    Err(Errno::EIO)
}

/// `efinet_end(nif)`.
fn efinet_end(_nif: &mut Netif) {
    let net = NET.load(Ordering::Relaxed);
    if net.is_null() {
        return;
    }

    // SAFETY: the firmware's Simple Network Protocol.
    unsafe { ((*net).Shutdown)(net) };
}
/* </CODE> */
