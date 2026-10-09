/* <LICENSES> */
/*
 * Copyright (c) 1995 Adam Glass, Gordon Ross
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
 * 3. The name of the authors may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `nfs/nfs_boot.c`: support for NFS diskless booting, specifically getting information about
//! where to boot from, what pathnames, etc.
//!
//! Upstream: sys/nfs/nfs_boot.c @ 3ce1f3f79392
//!
//! This implementation uses RARP and the bootparam RPC. We are forced to implement RPC anyway
//! (to get file handles) so we might as well take advantage of it for bootparam too.
//!
//! The diskless boot sequence goes as follows:
//! 1. Use RARP to get our interface address;
//! 2. Use RPC/bootparam/whoami to get our hostname, our IP address, and the server's IP
//!    address;
//! 3. Use RPC/bootparam/getfile to get the root path;
//! 4. Use RPC/mountd to get the root file handle;
//! 5. Use RPC/bootparam/getfile to get the swap path;
//! 6. Use RPC/mountd to get the swap file handle.
//!
//! (This happens to be the way Sun does it too.)
//!
//! ## Deviations
//! - The C has two variants: `#if !defined(NFSCLIENT) || (NETHER == 0)` stubs
//!   (`nfs_boot_init` panics, `nfs_boot_getfh` is `EOPNOTSUPP`) and the real functions in the
//!   `#else`. Both are here: the stubs without feature `nfsclient`, the real ones with it.
//!   `NETHER` is not a count here: ethernet (`if_ethersubr.rs`, `if_ether.rs`) is always
//!   built, so `NETHER == 0` never holds. `sys/conf/files` compiles the file only with
//!   `nfsclient`; the stubs are the C's code for a build that never happens, kept as the C
//!   has them.
//! - `nfsbootdevname` (a `char *` set by `setroot()` in `subr_disk.c`) is
//!   [`NFSBOOTDEVNAME`], a copy of the name, with a setter, [`set_nfsbootdevname`].
//! - The C's `hostname`/`hostnamelen`/`domainname`/`domainnamelen` are `kern_sysctl.rs`'s
//!   `HOSTNAME`, `HOSTNAMELEN`, `DOMAINNAME`, `DOMAINNAMELEN`.
//! - `bp_whoami` returns the gateway address instead of storing it through `gw_ip`;
//!   `bp_getfile`'s `serv_name` and `pathname` are slices (`MNAMELEN` and `MAXPATHLEN`
//!   bytes); `md_mount` also takes the buffer the file handle goes to (the C reaches it
//!   through `argp->fh`, which `nfs_boot_getfh` sets to the same buffer).
//! - `md_mount`'s `error` from the mount daemon (an errno number on the wire) that names no
//!   `Errno` is `EIO`.
//! - `md_mount` and `bp_whoami`/`bp_getfile` free the reply on every path (the C does too;
//!   the parsing is split into `*_reply` functions so that each owns the chain it parses).
//!   `md_mount` pulls up eight bytes before it reads the version 3 handle's length (the C
//!   reads it after checking only four).
//! - `NFS_BOOT_OPTIONS` and `NFS_BOOT_RWSIZE` (kernel config options, not defined in
//!   `GENERIC`) are not configured.

#[cfg(feature = "nfsclient")]
use core::ffi::c_void;
#[cfg(feature = "nfsclient")]
use core::ptr;
#[cfg(feature = "nfsclient")]
use core::sync::atomic::Ordering;

#[cfg(feature = "nfsclient")]
use libkern::StaticCell;

#[cfg(feature = "nfsclient")]
use crate::kern::kern_sysctl::{DOMAINNAME, DOMAINNAMELEN, HOSTNAME, HOSTNAMELEN};
use crate::kern::subr_prf::panic;
#[cfg(feature = "nfsclient")]
use crate::kern::subr_prf::{Str, printf};
#[cfg(feature = "nfsclient")]
use crate::kern::uipc_mbuf::{m_adj, m_copydata, m_freem, m_get, m_pullup};
#[cfg(feature = "nfsclient")]
use crate::kern::uipc_socket::{soclose, socreate};
#[cfg(feature = "nfsclient")]
use crate::net::if_::{IFF_UP, IFNAMSIZ, Ifreq, if_put, if_unit, ifioctl};
#[cfg(feature = "nfsclient")]
use crate::netinet::if_ether::revarpwhoami;
#[cfg(feature = "nfsclient")]
use crate::netinet::in_::{INADDR_ANY, InAddr, SockaddrIn};
#[cfg(feature = "nfsclient")]
use crate::netinet::in_var::{InAliasreq, ifatoia};
#[cfg(feature = "nfsclient")]
use crate::nfs::krpc_subr::{
    BOOTPARAM_GETFILE, BOOTPARAM_PROG, BOOTPARAM_VERS, BOOTPARAM_WHOAMI, PMAPPORT, PMAPPROC_CALLIT,
    PMAPPROG, PMAPVERS, krpc_call, krpc_portmap, xdr_inaddr_decode, xdr_inaddr_encode,
    xdr_string_decode, xdr_string_encode,
};
use crate::nfs::nfsdiskless::{NfsDiskless, NfsDlmount};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsproto::{NFS_PROG, NFS_VER2, NFS_VER3, NFSX_V2FH, NFSX_V3FHMAX};
#[cfg(feature = "nfsclient")]
use crate::nfs::rpcv2::{RPCMNT_MOUNT, RPCPROG_MNT};
#[cfg(feature = "nfsclient")]
use crate::nfs::xdr_subs::{fxdr_unsigned, txdr_unsigned};
use crate::sys::errno::Errno;
#[cfg(feature = "nfsclient")]
use crate::sys::mbuf::{M_WAIT, MT_DATA, Mbuf, mtod};
#[cfg(feature = "nfsclient")]
use crate::sys::mount::{MNAMELEN, NFSMNT_NFSV3, NfsArgs};
#[cfg(feature = "nfsclient")]
use crate::sys::param::{MAXHOSTNAMELEN, MAXPATHLEN};
use crate::sys::proc::Proc;
#[cfg(feature = "nfsclient")]
use crate::sys::socket::{AF_INET, SOCK_DGRAM};
#[cfg(feature = "nfsclient")]
use crate::sys::sockio::{SIOCAIFADDR, SIOCGIFFLAGS, SIOCSIFFLAGS};

/// `nfsbootdevname`: the name of the root device when booting from the network (a NUL-padded
/// copy of `rootdv->dv_xname`), set by `setroot()` through [`set_nfsbootdevname`].
#[cfg(feature = "nfsclient")]
static NFSBOOTDEVNAME: StaticCell<Option<[u8; IFNAMSIZ]>> = StaticCell::new(None);

/// `nfsbootdevname = rootdv->dv_xname` (`setroot()` in `subr_disk.c`): remembers the
/// network device the root file system is on.
#[cfg(feature = "nfsclient")]
pub fn set_nfsbootdevname(name: &[u8]) {
    let mut n = [0u8; IFNAMSIZ];
    let len = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    n[..len].copy_from_slice(&name[..len]);
    // SAFETY: written by `setroot` while the kernel configures itself, read by
    // `nfs_boot_init` in the same single thread of control afterwards; no reference to the
    // cell's contents is live across either.
    unsafe { NFSBOOTDEVNAME.write(Some(n)) };
}

/// The bytes of a NUL-terminated buffer before its NUL.
#[cfg(feature = "nfsclient")]
fn cstr(b: &[u8]) -> &[u8] {
    let len = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..len]
}

/// `nfs_boot_init(nd, procp)`: called with an empty `nfs_diskless` to be filled in. Finds an
/// interface, RARPs for its IP address, stuffs it, the implied broadcast address and the
/// netmask into the interface, asks the bootparam server who we are, and records the boot
/// server's address in `nd`. Panics when there is no interface or nobody answers: there is
/// nothing to boot from then.
///
/// This was moved here from `nfs_vfsops.c` because this procedure would be quite different if
/// someone decides to write (i.e.) a BOOTP version of this file (might not use RARP, etc.).
#[cfg(feature = "nfsclient")]
pub fn nfs_boot_init(nd: &mut NfsDiskless, procp: &Proc) -> Result<(), Errno> {
    // Find a network interface.
    //
    // SAFETY: `NFSBOOTDEVNAME` is written once by `setroot` before this runs (above).
    let devname = unsafe { NFSBOOTDEVNAME.read() };
    let Some(ifp) = devname.and_then(|n| if_unit(&n)) else {
        panic(format_args!("nfs_boot: no suitable interface"));
    };

    let xname = ifp.if_xname.get();
    let mut ireq = Ifreq::zeroed();
    ireq.ifr_name = xname;
    printf(format_args!(
        "nfs_boot: using interface {}, with revarp & bootparams\n",
        Str(&xname)
    ));

    // Bring up the interface.
    //
    // Get the old interface flags and or IFF_UP into them; if IFF_UP set blindly, interface
    // selection can be clobbered.
    let so = match socreate(i32::from(AF_INET), SOCK_DGRAM, 0) {
        Ok(so) => so,
        Err(e) => panic(format_args!("nfs_boot: socreate, error={}", e.as_i32())),
    };
    let sop = ptr::from_ref(so).cast::<c_void>();
    // SAFETY: `ireq` is a `struct ifreq`, what SIOCGIFFLAGS takes; `so` is a live socket.
    if let Err(e) = unsafe { ifioctl(sop, SIOCGIFFLAGS, ptr::from_mut(&mut ireq).cast(), procp) } {
        panic(format_args!("nfs_boot: GIFFLAGS, error={}", e.as_i32()));
    }
    ireq.set_ifr_flags(ireq.ifr_flags() | IFF_UP as i16);
    // SAFETY: as above, for SIOCSIFFLAGS.
    if let Err(e) = unsafe { ifioctl(sop, SIOCSIFFLAGS, ptr::from_mut(&mut ireq).cast(), procp) } {
        panic(format_args!("nfs_boot: SIFFLAGS, error={}", e.as_i32()));
    }

    // Do RARP for the interface address.
    let Ok(my_ip) = revarpwhoami(ifp) else {
        panic(format_args!(
            "reverse arp not answered by rarpd(8) or dhcpd(8)"
        ));
    };
    printf(format_args!("nfs_boot: client_addr={}\n", Ipv4(my_ip)));

    // Do enough of ifconfig(8) so that the chosen interface can talk to the servers. (just
    // set the address)
    let mut ifra = InAliasreq::zeroed();
    ifra.ifra_name = xname;
    *ifra.ifra_addr_mut() = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: my_ip,
        ..SockaddrIn::default()
    };
    // SAFETY: `ifra` is a `struct in_aliasreq`, what SIOCAIFADDR takes.
    if let Err(e) = unsafe { ifioctl(sop, SIOCAIFADDR, ptr::from_mut(&mut ifra).cast(), procp) } {
        panic(format_args!("nfs_boot: set if addr, error={}", e.as_i32()));
    }

    let _ = soclose(so, 0);

    let ifa = ifp.if_addrlist.iter().find(|ifa| {
        let sa = ifa.ifa_addr.get();
        // SAFETY: a live interface address's `ifa_addr` points at its `sockaddr`.
        !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET
    });
    let Some(ifa) = ifa else {
        panic(format_args!(
            "nfs_boot: address not configured on {}",
            Str(&xname)
        ));
    };
    if_put(ifp);

    // Get client name and gateway address. RPC: bootparam/whoami. The server address
    // returned by the WHOAMI call is used for all subsequent bootparam RPCs.
    let mut bp_sin = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: InAddr {
            s_addr: ifatoia(ifa).ia_broadaddr().get().sin_addr.s_addr,
        },
        ..SockaddrIn::default()
    };
    HOSTNAMELEN.store(MAXHOSTNAMELEN as i32, Ordering::Relaxed);

    // this returns gateway IP address
    let _gw_ip = match bp_whoami(&mut bp_sin, &my_ip) {
        Ok(gw) => gw,
        Err(e) => panic(format_args!(
            "nfs_boot: bootparam whoami, error={}",
            e.as_i32()
        )),
    };
    // SAFETY: HOSTNAME is written only by `bp_whoami` (above) and the kern.hostname sysctl,
    // neither running concurrently with this single thread of control at boot.
    let hostname = unsafe { HOSTNAME.get() };
    printf(format_args!(
        "nfs_boot: server_addr={} hostname={}\n",
        Ipv4(bp_sin.sin_addr),
        Str(hostname)
    ));

    nd.nd_boot = bp_sin;

    Ok(())
}

/// An IPv4 address in network order, for `inet_ntop`'s `%s`.
#[cfg(feature = "nfsclient")]
struct Ipv4(InAddr);

#[cfg(feature = "nfsclient")]
impl core::fmt::Display for Ipv4 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let b = self.0.s_addr.to_ne_bytes();
        write!(f, "{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    }
}

/// `nfs_boot_getfh(bpsin, key, ndmntp, retries)`: gets the file handle of the file system
/// `key` (`root` or `swap`) from the bootparam server `bpsin` (with `retries` tries: -1 is
/// forever): `ndmntp` is the output, with the mount arguments set up to describe the file
/// system, as the C does, by pointers into `ndmntp` itself.
#[cfg(feature = "nfsclient")]
pub fn nfs_boot_getfh(
    bpsin: &SockaddrIn,
    key: &[u8],
    ndmntp: &mut NfsDlmount,
    retries: i32,
) -> Result<(), Errno> {
    let mut pathname = [0u8; MAXPATHLEN];

    // Initialize mount args.
    let args = &mut ndmntp.ndm_args;
    *args = NfsDlmount::new().ndm_args;
    args.addr = ptr::from_ref(&ndmntp.ndm_saddr) as usize;
    args.addrlen = i32::from(ndmntp.ndm_saddr.sin_len);
    args.sotype = SOCK_DGRAM;
    args.fh = ndmntp.ndm_fh.as_ptr() as usize;
    args.hostname = ndmntp.ndm_host.as_ptr() as usize;
    args.flags = NFSMNT_NFSV3;

    let sin = &mut ndmntp.ndm_saddr;

    // Get server:pathname for "key" (root or swap) using RPC to bootparam/getfile
    if let Err(e) = bp_getfile(
        bpsin,
        key,
        sin,
        &mut ndmntp.ndm_host,
        &mut pathname,
        retries,
    ) {
        printf(format_args!(
            "nfs_boot: bootparam get {}: {}\n",
            Str(key),
            e.as_i32()
        ));
        return Err(e);
    }

    // Get file handle for "key" (root or swap) using RPC to mountd/mount
    if let Err(e) = md_mount(sin, cstr(&pathname), args, &mut ndmntp.ndm_fh) {
        printf(format_args!(
            "nfs_boot: mountd {}, error={}\n",
            Str(key),
            e.as_i32()
        ));
        return Err(e);
    }

    // Set port number for NFS use.
    // XXX: NFS port is always 2049, right?
    let ver = if args.flags & NFSMNT_NFSV3 != 0 {
        NFS_VER3
    } else {
        NFS_VER2
    };
    match krpc_portmap(sin, NFS_PROG, ver) {
        Ok(port) => sin.sin_port = port,
        Err(e) => {
            printf(format_args!(
                "nfs_boot: portmap NFS, error={}\n",
                e.as_i32()
            ));
            return Err(e);
        }
    }

    // Construct remote path (for getmntinfo(3))
    remote_path(&mut ndmntp.ndm_host, cstr(&pathname));

    Ok(())
}

/// The tail of `nfs_boot_getfh`: appends `:` and `pathname` to the server name in `host`,
/// cut to fit `MNAMELEN` (the remote path `getmntinfo(3)` shows).
#[cfg(feature = "nfsclient")]
fn remote_path(host: &mut [u8; MNAMELEN], pathname: &[u8]) {
    let endp = MNAMELEN - 1;
    let mut dp = cstr(host).len();
    host[dp] = b':';
    dp += 1;
    for &c in pathname {
        if dp >= endp {
            break;
        }
        host[dp] = c;
        dp += 1;
    }
    host[dp] = 0;
}

/// `bp_whoami(bpsin, my_ip, gw_ip)`: RPC bootparam/whoami: given the client IP address, gets
/// the client name (`hostname`), the domain name (`domainname`) and the gateway address (the
/// return value). The hostname and domainname are set here for convenience.
///
/// Note: `bpsin` is initialized to the broadcast address, and will be replaced with the
/// bootparam server address after this call is complete. Have to use `PMAP_PROC_CALL` to make
/// sure we get responses only from a servers that know about us (don't want to broadcast a
/// getport call).
#[cfg(feature = "nfsclient")]
fn bp_whoami(bpsin: &mut SockaddrIn, my_ip: &InAddr) -> Result<InAddr, Errno> {
    // Build request message for PMAPPROC_CALLIT: struct whoami_call (the program, version and
    // procedure to call and the length of the argument, all words) ...
    let m = m_get(M_WAIT, MT_DATA).ok_or(Errno::ENOBUFS)?;
    // ... and the encapsulated data (client IP address).
    let Some(arg) = xdr_inaddr_encode(my_ip) else {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    };
    let call: [u32; 4] = [
        txdr_unsigned(BOOTPARAM_PROG),
        txdr_unsigned(BOOTPARAM_VERS),
        txdr_unsigned(BOOTPARAM_WHOAMI),
        txdr_unsigned(arg.m_len().get()),
    ];
    // SAFETY: a fresh mbuf's data area holds `MLEN >= 16` bytes.
    unsafe { ptr::copy_nonoverlapping(call.as_ptr().cast::<u8>(), mtod::<u8>(m), 16) };
    m.m_len().set(16);
    m.m_next().set(Some(arg));

    // RPC: portmap/callit
    bpsin.sin_port = PMAPPORT.to_be();
    let mut data = Some(m);
    let mut from = None;
    krpc_call(
        bpsin,
        PMAPPROG,
        PMAPVERS,
        PMAPPROC_CALLIT,
        &mut data,
        Some(&mut from),
        -1,
    )?;

    // Parse result message.
    let r = bp_whoami_reply(data, from, bpsin);
    m_freem(from);
    r
}

/// The reply half of `bp_whoami`: the callit reply (the port and the length of the
/// encapsulated reply, then the whoami reply: hostname, domainname, gateway) in `m`, and the
/// address it came from.
#[cfg(feature = "nfsclient")]
fn bp_whoami_reply(
    m: Option<&'static Mbuf>,
    from: Option<&'static Mbuf>,
    bpsin: &mut SockaddrIn,
) -> Result<InAddr, Errno> {
    let bad = || {
        printf(format_args!("nfs_boot: bootparam_whoami: bad reply\n"));
        Errno::EBADRPC
    };
    let Some(mut m) = m else {
        return Err(bad());
    };
    // struct callit_reply: the port and the length of the encapsulated data (words).
    if (m.m_len().get() as usize) < 8 {
        m = m_pullup(m, 8).ok_or_else(bad)?;
    }
    let mut hdr = [0u8; 8];
    m_copydata(m, 0, &mut hdr);
    let port = fxdr_unsigned(u32::from_ne_bytes([hdr[0], hdr[1], hdr[2], hdr[3]])) as i16;
    // msg_len = fxdr_unsigned(u_int32_t, reply->encap_len): not used by the C either.
    m_adj(m, 8);

    // Save bootparam server address
    let Some(from) = from else {
        m_freem(m);
        return Err(bad());
    };
    let mut sin = [0u8; size_of::<SockaddrIn>()];
    if (from.m_len().get() as usize) < sin.len() {
        m_freem(m);
        return Err(bad());
    }
    m_copydata(from, 0, &mut sin);
    // sin_addr is at offset 4 of a struct sockaddr_in.
    bpsin.sin_port = (port as u16).to_be();
    bpsin.sin_addr.s_addr = u32::from_ne_bytes([sin[4], sin[5], sin[6], sin[7]]);

    // SAFETY: HOSTNAME and DOMAINNAME are written only here and by the sysctl, neither
    // running concurrently with this single thread of control at boot.
    let (hostname, domainname) = unsafe { (HOSTNAME.get_mut(), DOMAINNAME.get_mut()) };

    // client name
    let mut len = MAXHOSTNAMELEN - 1;
    HOSTNAMELEN.store(len as i32, Ordering::Relaxed);
    let Some(m1) = xdr_string_decode(m, hostname, &mut len) else {
        return Err(bad());
    };
    HOSTNAMELEN.store(len as i32, Ordering::Relaxed);

    // domain name
    let mut len = MAXHOSTNAMELEN - 1;
    DOMAINNAMELEN.store(len as i32, Ordering::Relaxed);
    let Some(m2) = xdr_string_decode(m1, domainname, &mut len) else {
        return Err(bad());
    };
    DOMAINNAMELEN.store(len as i32, Ordering::Relaxed);

    // gateway address
    let mut gw_ip = InAddr { s_addr: INADDR_ANY };
    let Some(m3) = xdr_inaddr_decode(m2, &mut gw_ip) else {
        return Err(bad());
    };

    // success
    m_freem(m3);
    Ok(gw_ip)
}

/// `bp_getfile(bpsin, key, md_sin, serv_name, pathname, retries)`: RPC bootparam/getfile:
/// given the client name and the file `key`, gets the server name (`serv_name`), the server IP
/// address (`md_sin`, a new `sockaddr_in` of it, for mountd and NFS) and the server pathname
/// (`pathname`).
#[cfg(feature = "nfsclient")]
fn bp_getfile(
    bpsin: &SockaddrIn,
    key: &[u8],
    md_sin: &mut SockaddrIn,
    serv_name: &mut [u8],
    pathname: &mut [u8],
    retries: i32,
) -> Result<(), Errno> {
    // Build request message.

    // client name (hostname)
    //
    // SAFETY: HOSTNAME is written only by `bp_whoami` and the sysctl, neither running
    // concurrently with this single thread of control at boot.
    let hostname = unsafe { HOSTNAME.get() };
    let hostnamelen = (HOSTNAMELEN.load(Ordering::Relaxed) as usize).min(hostname.len());
    let m = xdr_string_encode(&hostname[..hostnamelen]).ok_or(Errno::ENOMEM)?;

    // key name (root or swap)
    let Some(k) = xdr_string_encode(key) else {
        m_freem(m);
        return Err(Errno::ENOMEM);
    };
    m.m_next().set(Some(k));

    // RPC: bootparam/getfile
    let mut data = Some(m);
    krpc_call(
        bpsin,
        BOOTPARAM_PROG,
        BOOTPARAM_VERS,
        BOOTPARAM_GETFILE,
        &mut data,
        None,
        retries,
    )?;

    // Parse result message.
    let r = bp_getfile_reply(data, md_sin, serv_name, pathname);
    if r.is_err() {
        printf(format_args!("nfs_boot: bootparam_getfile: bad reply\n"));
        return Err(Errno::EBADRPC);
    }
    Ok(())
}

/// The reply half of `bp_getfile`: server name, server address, server pathname; `Err` for
/// a bad reply.
#[cfg(feature = "nfsclient")]
fn bp_getfile_reply(
    m: Option<&'static Mbuf>,
    md_sin: &mut SockaddrIn,
    serv_name: &mut [u8],
    pathname: &mut [u8],
) -> Result<(), ()> {
    let m = m.ok_or(())?;

    // server name
    let mut sn_len = MNAMELEN - 1;
    let m = xdr_string_decode(m, serv_name, &mut sn_len).ok_or(())?;

    // server IP address (mountd/NFS)
    let mut inaddr = InAddr { s_addr: INADDR_ANY };
    let m = xdr_inaddr_decode(m, &mut inaddr).ok_or(())?;

    // server pathname
    let mut path_len = MAXPATHLEN - 1;
    let m = xdr_string_decode(m, pathname, &mut path_len).ok_or(())?;

    // setup server socket address
    *md_sin = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: inaddr,
        ..SockaddrIn::default()
    };

    // success
    m_freem(m);
    Ok(())
}

/// `md_mount(mdsin, path, argp)`: RPC mountd/mount: given a server pathname, gets an NFS file
/// handle (into `fh`, its size in `argp.fhsize`). Also sets `mdsin.sin_port` to the mount
/// daemon's port. `mdsin` is the mountd server address. Tries mount protocol version 3 (for
/// NFSv3) and falls back to lower versions, clearing `NFSMNT_NFSV3` when it ends up with
/// less than 3.
#[cfg(feature = "nfsclient")]
fn md_mount(
    mdsin: &mut SockaddrIn,
    path: &[u8],
    argp: &mut NfsArgs,
    fh: &mut [u8; NFSX_V3FHMAX],
) -> Result<(), Errno> {
    let mut mntver: u32 = if argp.flags & NFSMNT_NFSV3 != 0 { 3 } else { 2 };
    let mut data: Option<&'static Mbuf>;
    loop {
        // A failed portmap (`continue` in the C's do-while) or a mount daemon that does not
        // speak this version (`EPROGMISMATCH`) tries the next lower version; any other
        // result of the call ends the loop.
        let retry = match krpc_portmap(mdsin, RPCPROG_MNT, mntver) {
            Err(e) => e,
            Ok(port) => {
                mdsin.sin_port = port;

                let m = xdr_string_encode(path).ok_or(Errno::ENOMEM)?;

                // Do RPC to mountd.
                data = Some(m);
                match krpc_call(
                    mdsin,
                    RPCPROG_MNT,
                    mntver,
                    RPCMNT_MOUNT,
                    &mut data,
                    None,
                    -1,
                ) {
                    Ok(()) => break,
                    Err(Errno::EPROGMISMATCH) => Errno::EPROGMISMATCH,
                    Err(e) => return Err(e), // message already freed
                }
            }
        };
        // Try lower version of mountd.
        mntver -= 1;
        if mntver < 1 {
            return Err(retry);
        }
    }

    if mntver != 3 {
        argp.flags &= !NFSMNT_NFSV3;
    }

    let Some(m) = data else {
        return Err(Errno::EBADRPC);
    };
    md_mount_reply(m, mntver, argp, fh)
}

/// The reply half of `md_mount`: the mount status and the file handle; frees the reply.
#[cfg(feature = "nfsclient")]
fn md_mount_reply(
    m: &'static Mbuf,
    mntver: u32,
    argp: &mut NfsArgs,
    fh: &mut [u8; NFSX_V3FHMAX],
) -> Result<(), Errno> {
    let mut m = m;
    // The reply might have only the errno.
    if m.m_len().get() < 4 {
        m_freem(m);
        return Err(Errno::EBADRPC);
    }
    // Have at least errno, so check that.
    let mut w = [0u8; 4];
    m_copydata(m, 0, &mut w);
    let error = fxdr_unsigned(u32::from_ne_bytes(w));
    if error != 0 {
        m_freem(m);
        return Err(Errno::from_raw(error as i32).unwrap_or(Errno::EIO));
    }

    // Have errno==0, so the fh must be there.
    let (fhoff, minlen);
    if mntver == 3 {
        if (m.m_len().get() as usize) < 8 {
            m = match m_pullup(m, 8) {
                Some(m) => m,
                None => return Err(Errno::EBADRPC),
            };
        }
        m_copydata(m, 4, &mut w);
        let fhsize = fxdr_unsigned(u32::from_ne_bytes(w));
        if fhsize as usize > NFSX_V3FHMAX {
            m_freem(m);
            return Err(Errno::EBADRPC);
        }
        argp.fhsize = fhsize as i32;
        fhoff = 8;
        minlen = 2 * size_of::<u32>() + fhsize as usize;
    } else {
        argp.fhsize = NFSX_V2FH as i32;
        fhoff = 4;
        minlen = size_of::<u32>() + NFSX_V2FH;
    }

    if (m.m_len().get() as usize) < minlen {
        m = m_pullup(m, minlen as i32).ok_or(Errno::EBADRPC)?;
    }

    m_copydata(m, fhoff, &mut fh[..argp.fhsize as usize]);

    m_freem(m);
    Ok(())
}

/// `nfs_boot_init(nd, procp)`: without `NFSCLIENT` (or without ethernet) there is nothing to
/// boot from.
#[cfg(not(feature = "nfsclient"))]
pub fn nfs_boot_init(_nd: &mut NfsDiskless, _procp: &Proc) -> Result<(), Errno> {
    panic(format_args!(
        "nfs_boot_init: NFSCLIENT not enabled in kernel"
    ));
}

/// `nfs_boot_getfh(bpsin, key, ndmntp, retries)`: without `NFSCLIENT` (or without ethernet)
/// there is nothing to boot from; the C notes that it cannot get here.
#[cfg(not(feature = "nfsclient"))]
pub fn nfs_boot_getfh(
    _bpsin: &crate::netinet::in_::SockaddrIn,
    _key: &[u8],
    _ndmntp: &mut NfsDlmount,
    _retries: i32,
) -> Result<(), Errno> {
    // can not get here
    Err(Errno::EOPNOTSUPP)
}
/* </CODE> */

/* <TESTS> */
#[cfg(all(test, feature = "nfsclient"))]
mod tests {
    // Host tests for `nfs_boot.c`: the parsers of the bootparam and mountd replies
    // (`bp_whoami_reply`, `bp_getfile_reply`, `md_mount_reply`) and the structures of
    // `nfsdiskless.h`. The RPCs themselves (`krpc_call`) need a network and are the smoke
    // test's.

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup;
    use crate::nfs::krpc_subr::tests::pkt;
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};

    /// The XDR encoding of a string, as `xdr_string_encode` makes it.
    fn xstr(s: &[u8]) -> Vec<u8> {
        let mut v = (s.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(s);
        v.resize(4 + s.len().div_ceil(4) * 4, 0);
        v
    }

    /// The XDR encoding of an Internet address (type 1 and a word per byte).
    fn xaddr(a: [u8; 4]) -> Vec<u8> {
        let mut v = 1u32.to_be_bytes().to_vec();
        for b in a {
            v.extend_from_slice(&u32::from(b).to_be_bytes());
        }
        v
    }

    #[test]
    fn getfile_reply_gives_the_server_its_address_and_the_path() {
        let _g = setup();
        let mut b = xstr(b"fileserver");
        b.extend(xaddr([10, 0, 2, 2]));
        b.extend(xstr(b"/export/root"));
        b.extend([9, 9]); // trailing garbage is ignored
        let m = pkt(&[&b[..7], &b[7..30], &b[30..]]);

        let mut sin = SockaddrIn::default();
        let mut name = [0xffu8; MNAMELEN];
        let mut path = [0xffu8; MAXPATHLEN];
        bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).expect("parsed");
        assert_eq!(cstr(&name), b"fileserver");
        assert_eq!(cstr(&path), b"/export/root");
        assert_eq!(sin.sin_len, 16);
        assert_eq!(sin.sin_family, AF_INET);
        assert_eq!(sin.sin_addr.s_addr, u32::from_ne_bytes([10, 0, 2, 2]));
        assert_eq!(sin.sin_port, 0);
    }

    #[test]
    fn getfile_reply_refuses_a_short_reply() {
        let _g = setup();
        let mut b = xstr(b"fileserver");
        b.extend(xaddr([10, 0, 2, 2]));
        // The path is missing.
        let m = pkt(&[&b]);
        let mut sin = SockaddrIn::default();
        let mut name = [0u8; MNAMELEN];
        let mut path = [0u8; MAXPATHLEN];
        assert!(bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).is_err());
        assert_eq!(
            sin.sin_family, 0,
            "the address is only set for a good reply"
        );
        assert!(bp_getfile_reply(None, &mut sin, &mut name, &mut path).is_err());
    }

    #[test]
    fn getfile_reply_truncates_a_long_server_name() {
        let _g = setup();
        let long = [b'h'; 120];
        let mut b = xstr(&long);
        b.extend(xaddr([1, 2, 3, 4]));
        b.extend(xstr(b"/p"));
        let m = pkt(&[&b]);
        let mut sin = SockaddrIn::default();
        let mut name = [0u8; MNAMELEN];
        let mut path = [0u8; MAXPATHLEN];
        bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).expect("parsed");
        assert_eq!(cstr(&name).len(), MNAMELEN - 1);
        assert_eq!(cstr(&path), b"/p");
    }

    #[test]
    fn whoami_reply_sets_the_server_the_names_and_the_gateway() {
        let _g = setup();
        let mut b: Vec<u8> = Vec::new();
        b.extend(0x0000_0801u32.to_be_bytes()); // the port of the bootparam server
        b.extend(0x0000_0040u32.to_be_bytes()); // the length of what follows
        b.extend(xstr(b"client"));
        b.extend(xstr(b"example.org"));
        b.extend(xaddr([10, 0, 2, 1]));
        let m = pkt(&[&b[..5], &b[5..]]);

        // The address the reply came from.
        let from = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
        let mut sa = [0u8; 16];
        sa[0] = 16;
        sa[1] = AF_INET;
        sa[4..8].copy_from_slice(&[10, 0, 2, 2]);
        // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
        unsafe { ptr::copy_nonoverlapping(sa.as_ptr(), mtod::<u8>(from), 16) };
        from.m_len().set(16);

        let mut bpsin = SockaddrIn {
            sin_len: 16,
            sin_family: AF_INET,
            sin_addr: InAddr {
                s_addr: u32::from_ne_bytes([10, 0, 2, 255]),
            },
            sin_port: 111u16.to_be(),
            ..SockaddrIn::default()
        };
        let gw = bp_whoami_reply(Some(m), Some(from), &mut bpsin).expect("parsed");
        assert_eq!(gw.s_addr, u32::from_ne_bytes([10, 0, 2, 1]));
        assert_eq!(bpsin.sin_port, 0x0801u16.to_be());
        assert_eq!(bpsin.sin_addr.s_addr, u32::from_ne_bytes([10, 0, 2, 2]));
        // SAFETY: nothing else touches the hostname statics in this test.
        let (host, dom) = unsafe { (HOSTNAME.get(), DOMAINNAME.get()) };
        assert_eq!(cstr(host), b"client");
        assert_eq!(HOSTNAMELEN.load(Ordering::Relaxed), 6);
        assert_eq!(cstr(dom), b"example.org");
        assert_eq!(DOMAINNAMELEN.load(Ordering::Relaxed), 11);
    }

    #[test]
    fn whoami_reply_refuses_a_bad_reply() {
        let _g = setup();
        let from = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
        from.m_len().set(16);
        let mut bpsin = SockaddrIn::default();
        // Too short for the callit header.
        assert_eq!(
            bp_whoami_reply(Some(pkt(&[&[0, 0, 0]])), Some(from), &mut bpsin).err(),
            Some(Errno::EBADRPC)
        );
        // The names are there, the gateway is not.
        let mut b: Vec<u8> = vec_of(&[0, 0, 0, 1, 0, 0, 0, 8]);
        b.extend(xstr(b"client"));
        b.extend(xstr(b"dom"));
        assert_eq!(
            bp_whoami_reply(Some(pkt(&[&b])), Some(from), &mut bpsin).err(),
            Some(Errno::EBADRPC)
        );
        assert_eq!(
            bp_whoami_reply(None, Some(from), &mut bpsin).err(),
            Some(Errno::EBADRPC)
        );
    }

    fn vec_of(b: &[u8]) -> Vec<u8> {
        b.to_vec()
    }

    /// The `nfs_args` `md_mount` fills.
    fn args(v3: bool) -> NfsArgs {
        let mut a = NfsDlmount::new().ndm_args;
        if v3 {
            a.flags = NFSMNT_NFSV3;
        }
        a
    }

    #[test]
    fn mount_reply_version_3_gives_a_variable_length_handle() {
        let _g = setup();
        let fh: Vec<u8> = (1..=28).collect();
        let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 28]);
        b.extend(&fh);
        // Split inside the handle's length word: it is pulled up.
        let m = pkt(&[&b[..6], &b[6..]]);
        let mut a = args(true);
        let mut out = [0xffu8; NFSX_V3FHMAX];
        md_mount_reply(m, 3, &mut a, &mut out).expect("a handle");
        assert_eq!(a.fhsize, 28);
        assert_eq!(&out[..28], &fh[..]);
        assert_eq!(out[28], 0xff, "nothing is written past the handle");
    }

    #[test]
    fn mount_reply_version_2_gives_a_32_byte_handle() {
        let _g = setup();
        let fh: Vec<u8> = (100..132).collect();
        let mut b = vec_of(&[0, 0, 0, 0]);
        b.extend(&fh);
        let m = pkt(&[&b[..10], &b[10..]]);
        let mut a = args(false);
        let mut out = [0u8; NFSX_V3FHMAX];
        md_mount_reply(m, 2, &mut a, &mut out).expect("a handle");
        assert_eq!(a.fhsize, NFSX_V2FH as i32);
        assert_eq!(&out[..32], &fh[..]);
    }

    #[test]
    fn mount_reply_errors() {
        let _g = setup();
        let mut a = args(true);
        let mut out = [0u8; NFSX_V3FHMAX];
        // Not even an errno.
        assert_eq!(
            md_mount_reply(pkt(&[&[0, 0]]), 3, &mut a, &mut out),
            Err(Errno::EBADRPC)
        );
        // The daemon refuses: its errno (EACCES), and nothing else is read.
        assert_eq!(
            md_mount_reply(pkt(&[&[0, 0, 0, 13]]), 3, &mut a, &mut out),
            Err(Errno::EACCES)
        );
        // An errno that is not one of ours.
        assert_eq!(
            md_mount_reply(pkt(&[&[0, 0, 1, 0]]), 3, &mut a, &mut out),
            Err(Errno::EIO)
        );
        // A version 3 handle that is too long.
        let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 65]);
        b.extend([0u8; 65]);
        assert_eq!(
            md_mount_reply(pkt(&[&b]), 3, &mut a, &mut out),
            Err(Errno::EBADRPC)
        );
        // A handle that the reply does not hold.
        let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 20]);
        b.extend([0u8; 5]);
        assert_eq!(
            md_mount_reply(pkt(&[&b]), 3, &mut a, &mut out),
            Err(Errno::EBADRPC)
        );
    }

    #[test]
    fn the_diskless_structures_start_out_empty() {
        let nd = NfsDiskless::new();
        assert_eq!(nd.nd_boot.sin_family, 0);
        assert_eq!(nd.nd_root.ndm_args.flags, 0);
        assert_eq!(nd.nd_swap.ndm_host[0], 0);
        assert_eq!(nd.nd_swap.ndm_fh, [0; NFSX_V3FHMAX]);
        assert!(nd.sw_vp.is_none());
    }

    #[test]
    fn the_remote_path_is_host_colon_path_cut_to_mnamelen() {
        let mut host = [0u8; MNAMELEN];
        host[..3].copy_from_slice(b"srv");
        remote_path(&mut host, b"/export/root");
        assert_eq!(cstr(&host), b"srv:/export/root");

        let mut host = [0u8; MNAMELEN];
        host[..3].copy_from_slice(b"srv");
        remote_path(&mut host, &[b'p'; 200]);
        assert_eq!(cstr(&host).len(), MNAMELEN - 1);
        assert_eq!(&host[..5], b"srv:p");
    }
}
/* </TESTS> */
