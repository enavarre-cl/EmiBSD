//! The SCSI midlayer: OpenBSD `sys/scsi/`.
//!
//! `scsi_all` holds the command and data formats every SCSI device shares, `scsiconf` the
//! structures that tie an adapter, its `scsibus` and the device drivers together (links,
//! transfers, I/O pools), and `scsi_base` the transfer and pool machinery and the common
//! commands.

pub mod scsi_all;
pub mod scsi_base;
pub mod scsiconf;
