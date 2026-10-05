//! Media-independent interface PHYs: OpenBSD `sys/dev/mii/`.
//!
//! Only `rgephyreg` (the Realtek PHY registers em(4)'s shared code programs) is here; the
//! mii(4) layer and its PHY drivers are not ported.

pub mod rgephyreg;
