/*	$OpenBSD: pledge.h,v 1.55 2026/09/19 17:21:52 dv Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 Nicholas Marriott <nicm@openbsd.org>
 * Copyright (c) 2015 Theo de Raadt <deraadt@openbsd.org>
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

//! `<sys/pledge.h>`: the `pledge(2)` request bits (`PLEDGE_*`), which `namei` also carries
//! in `ni_pledge` to say what a lookup is for, and the `pledgenames[]` table.
//!
//! Upstream: sys/sys/pledge.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pledgenames[]` is a slice without the `{ 0, NULL }` terminator; it is always compiled
//!   (the C hides it behind `PLEDGENAMES`).
//! - The prototypes belong to `kern_pledge.c`, which is not ported: the callers report a
//!   pledged process with `unported!` (no process can be pledged before `sys_pledge`).

/// `PLEDGE_ALWAYS`.
pub const PLEDGE_ALWAYS: u64 = 0xffff_ffff_ffff_ffff;
/// `PLEDGE_RPATH`: allow open for read.
pub const PLEDGE_RPATH: u64 = 0x0000_0000_0000_0001;
/// `PLEDGE_WPATH`: allow open for write.
pub const PLEDGE_WPATH: u64 = 0x0000_0000_0000_0002;
/// `PLEDGE_CPATH`: allow creat, mkdir, unlink etc.
pub const PLEDGE_CPATH: u64 = 0x0000_0000_0000_0004;
/// `PLEDGE_STDIO`: operate on own pid.
pub const PLEDGE_STDIO: u64 = 0x0000_0000_0000_0008;
/// `PLEDGE_DNS`: DNS services.
pub const PLEDGE_DNS: u64 = 0x0000_0000_0000_0020;
/// `PLEDGE_INET`: `AF_INET`/`AF_INET6` sockets.
pub const PLEDGE_INET: u64 = 0x0000_0000_0000_0040;
/// `PLEDGE_FLOCK`: file locking.
pub const PLEDGE_FLOCK: u64 = 0x0000_0000_0000_0080;
/// `PLEDGE_UNIX`: `AF_UNIX` sockets.
pub const PLEDGE_UNIX: u64 = 0x0000_0000_0000_0100;
/// `PLEDGE_ID`: allow setuid, setgid, etc.
pub const PLEDGE_ID: u64 = 0x0000_0000_0000_0200;
/// `PLEDGE_TAPE`: Tape ioctl.
pub const PLEDGE_TAPE: u64 = 0x0000_0000_0000_0400;
/// `PLEDGE_GETPW`: YP enables if ypbind.lock.
pub const PLEDGE_GETPW: u64 = 0x0000_0000_0000_0800;
/// `PLEDGE_PROC`: fork, waitpid, etc.
pub const PLEDGE_PROC: u64 = 0x0000_0000_0000_1000;
/// `PLEDGE_SETTIME`: able to set/adj time/freq.
pub const PLEDGE_SETTIME: u64 = 0x0000_0000_0000_2000;
/// `PLEDGE_FATTR`: allow explicit file `st_*` mods.
pub const PLEDGE_FATTR: u64 = 0x0000_0000_0000_4000;
/// `PLEDGE_PROTEXEC`: allow use of `PROT_EXEC`.
pub const PLEDGE_PROTEXEC: u64 = 0x0000_0000_0000_8000;
/// `PLEDGE_TTY`: tty setting.
pub const PLEDGE_TTY: u64 = 0x0000_0000_0001_0000;
/// `PLEDGE_SENDFD`: `AF_UNIX` CMSG fd sending.
pub const PLEDGE_SENDFD: u64 = 0x0000_0000_0002_0000;
/// `PLEDGE_RECVFD`: `AF_UNIX` CMSG fd receiving.
pub const PLEDGE_RECVFD: u64 = 0x0000_0000_0004_0000;
/// `PLEDGE_EXEC`: execve, child is free of pledge.
pub const PLEDGE_EXEC: u64 = 0x0000_0000_0008_0000;
/// `PLEDGE_ROUTE`: routing lookups.
pub const PLEDGE_ROUTE: u64 = 0x0000_0000_0010_0000;
/// `PLEDGE_MCAST`: multicast joins.
pub const PLEDGE_MCAST: u64 = 0x0000_0000_0020_0000;
/// `PLEDGE_VMINFO`: vminfo listings.
pub const PLEDGE_VMINFO: u64 = 0x0000_0000_0040_0000;
/// `PLEDGE_PS`: ps listings.
pub const PLEDGE_PS: u64 = 0x0000_0000_0080_0000;
/// `PLEDGE_DISKLABEL`: disklabels.
pub const PLEDGE_DISKLABEL: u64 = 0x0000_0000_0200_0000;
/// `PLEDGE_PF`: pf ioctls.
pub const PLEDGE_PF: u64 = 0x0000_0000_0400_0000;
/// `PLEDGE_AUDIO`: audio ioctls.
pub const PLEDGE_AUDIO: u64 = 0x0000_0000_0800_0000;
/// `PLEDGE_DPATH`: mknod & mkfifo.
pub const PLEDGE_DPATH: u64 = 0x0000_0000_1000_0000;
/// `PLEDGE_DRM`: drm ioctls.
pub const PLEDGE_DRM: u64 = 0x0000_0000_2000_0000;
/// `PLEDGE_VMM`: vmm ioctls.
pub const PLEDGE_VMM: u64 = 0x0000_0000_4000_0000;
/// `PLEDGE_CHOWN`: chown(2) family.
pub const PLEDGE_CHOWN: u64 = 0x0000_0000_8000_0000;
/// `PLEDGE_CHOWNUID`: allow owner/group changes.
pub const PLEDGE_CHOWNUID: u64 = 0x0000_0001_0000_0000;
/// `PLEDGE_BPF`: bpf ioctl.
pub const PLEDGE_BPF: u64 = 0x0000_0002_0000_0000;
/// `PLEDGE_ERROR`: `ENOSYS` instead of kill.
pub const PLEDGE_ERROR: u64 = 0x0000_0004_0000_0000;
/// `PLEDGE_WROUTE`: interface address ioctls.
pub const PLEDGE_WROUTE: u64 = 0x0000_0008_0000_0000;
/// `PLEDGE_UNVEIL`: allow `unveil()`.
pub const PLEDGE_UNVEIL: u64 = 0x0000_0010_0000_0000;
/// `PLEDGE_VIDEO`: video ioctls.
pub const PLEDGE_VIDEO: u64 = 0x0000_0020_0000_0000;

/// `PLEDGE_USERSET`: bits outside it are used by the kernel itself to track program
/// behaviours which have been observed.
pub const PLEDGE_USERSET: u64 = 0x0fff_ffff_ffff_ffff;

/// `pledgenames[]`: the promise names `pledge(2)` parses, with their bits.
pub static PLEDGENAMES: &[(u64, &[u8])] = &[
    (PLEDGE_STDIO, b"stdio"),
    (PLEDGE_RPATH, b"rpath"),
    (PLEDGE_WPATH, b"wpath"),
    (PLEDGE_CPATH, b"cpath"),
    (PLEDGE_DPATH, b"dpath"),
    (PLEDGE_INET, b"inet"),
    (PLEDGE_MCAST, b"mcast"),
    (PLEDGE_FATTR, b"fattr"),
    (PLEDGE_CHOWNUID, b"chown"),
    (PLEDGE_FLOCK, b"flock"),
    (PLEDGE_UNIX, b"unix"),
    (PLEDGE_DNS, b"dns"),
    (PLEDGE_GETPW, b"getpw"),
    (PLEDGE_SENDFD, b"sendfd"),
    (PLEDGE_RECVFD, b"recvfd"),
    (PLEDGE_TAPE, b"tape"),
    (PLEDGE_TTY, b"tty"),
    (PLEDGE_PROC, b"proc"),
    (PLEDGE_EXEC, b"exec"),
    (PLEDGE_PROTEXEC, b"prot_exec"),
    (PLEDGE_SETTIME, b"settime"),
    (PLEDGE_PS, b"ps"),
    (PLEDGE_VMINFO, b"vminfo"),
    (PLEDGE_ID, b"id"),
    (PLEDGE_PF, b"pf"),
    (PLEDGE_ROUTE, b"route"),
    (PLEDGE_WROUTE, b"wroute"),
    (PLEDGE_AUDIO, b"audio"),
    (PLEDGE_VIDEO, b"video"),
    (PLEDGE_BPF, b"bpf"),
    (PLEDGE_UNVEIL, b"unveil"),
    (PLEDGE_ERROR, b"error"),
    (PLEDGE_DISKLABEL, b"disklabel"),
    (PLEDGE_DRM, b"drm"),
    (PLEDGE_VMM, b"vmm"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_single_bits_inside_the_user_set() {
        for &(bits, name) in PLEDGENAMES {
            assert_eq!(bits.count_ones(), 1, "{name:?}");
            assert_eq!(bits & !PLEDGE_USERSET, 0, "{name:?}");
        }
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/pledge.h");
        for (name, value) in [
            ("PLEDGE_RPATH", PLEDGE_RPATH),
            ("PLEDGE_CPATH", PLEDGE_CPATH),
            ("PLEDGE_FATTR", PLEDGE_FATTR),
            ("PLEDGE_DPATH", PLEDGE_DPATH),
            ("PLEDGE_CHOWN", PLEDGE_CHOWN),
            ("PLEDGE_CHOWNUID", PLEDGE_CHOWNUID),
            ("PLEDGE_UNVEIL", PLEDGE_UNVEIL),
            ("PLEDGE_VIDEO", PLEDGE_VIDEO),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(value as i64),
                "{name}"
            );
        }
    }
}
