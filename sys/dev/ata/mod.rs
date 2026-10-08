/* <CODE> */
//! ATA and SATA support: OpenBSD `sys/dev/ata/`. `atascsi` is the SCSI to ATA translation
//! layer of the SATA host controllers (`ahci(4)`), `pmreg` the port multiplier registers.
//! `wd(4)`, `ata.c` and `ata_wdc.c` (the wdc-attached ATA disks) are not ported.

pub mod atascsi;
pub mod pmreg;
/* </CODE> */
