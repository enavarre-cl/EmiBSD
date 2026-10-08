/* <CODE> */
//! The SCRIPTS microcode of the Symbios/NCR 53c7xx/8xx SCSI processors: OpenBSD
//! `sys/dev/microcode/siop/`. `siop` is `siop.out`, the program siop(4) runs (the
//! microcode of osiop(4) and oosiop(4), `osiop.out` and `oosiop.out`, is not ported: those
//! drivers are not).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/microcode/siop/siop.out
pub mod siop;
/* </CODE> */
