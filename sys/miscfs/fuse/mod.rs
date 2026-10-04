//! FUSE, the userland file system interface: OpenBSD `sys/miscfs/fuse/` (`option FUSE` and
//! `pseudo-device fuse`, feature `fuse`).
//!
//! Headers (types): `fusefs` (`fusefs.h`), `fusefs_node` (`fusefs_node.h`); the protocol is
//! `sys::fusebuf` (`<sys/fusebuf.h>`). Files (functions): `fusebuf` (the request buffers),
//! `fuse_device` (the `/dev/fuse0` character device the daemon reads requests from and
//! writes replies to), `fuse_file`, `fuse_ihash`, `fuse_lookup`, `fuse_vfsops`,
//! `fuse_vnops`. `FUSE_DEBUG` is off, as in GENERIC: the `DPRINTF`s are not compiled.

pub mod fuse_device;
pub mod fuse_file;
pub mod fuse_ihash;
pub mod fuse_lookup;
pub mod fuse_vfsops;
pub mod fuse_vnops;
pub mod fusebuf;
pub mod fusefs;
pub mod fusefs_node;
