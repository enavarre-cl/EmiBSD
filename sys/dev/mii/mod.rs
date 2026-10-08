/* <CODE> */
//! Media-independent interface PHYs: OpenBSD `sys/dev/mii/`.
//!
//! The mii(4) layer (`mii.c`, `mii_physubr.c`, `<dev/mii/mii.h>`, `<dev/mii/miivar.h>`), the
//! PHY drivers the GENERICs attach at `mii?` that are ported (`rlphy`, `rgephy`, `ukphy` with
//! `ukphy_subr`), the ids they match (`miidevs.h`) and `rgephyreg`.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/mii/mii.c
pub mod mii;
pub mod mii_physubr;
pub mod miidevs;
pub mod miivar;
pub mod rgephy;
pub mod rgephyreg;
pub mod rlphy;
pub mod ukphy;
pub mod ukphy_subr;
/* </CODE> */
