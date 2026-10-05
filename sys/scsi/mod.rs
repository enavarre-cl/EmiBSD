//! The SCSI midlayer: OpenBSD `sys/scsi/`.
//!
//! `scsi_all` holds the command and data formats every SCSI device shares, `scsiconf` the
//! structures that tie an adapter, its `scsibus` and the device drivers together (links,
//! transfers, I/O pools), and `scsi_base` the transfer and pool machinery and the common
//! commands. `scsi_disk` holds the commands and mode pages of disks, `scsi_message` the
//! messages of the parallel bus, `scsi_debug` the per-link debugging bits, `sd`/`sdvar` the
//! disk driver, sd(4), and `cd` the CD-ROM driver, cd(4), with `<scsi/cd.h>`.

pub mod cd;
pub mod scsi_all;
pub mod scsi_base;
pub mod scsi_debug;
pub mod scsi_disk;
pub mod scsi_ioctl;
pub mod scsi_message;
pub mod scsiconf;
pub mod sd;
pub mod sdvar;
