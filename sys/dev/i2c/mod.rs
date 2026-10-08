/* <CODE> */
//! The I2C bus: OpenBSD `sys/dev/i2c/`.
//!
//! `i2c_io` and `i2cvar` are the headers (operations, the controller interface, the attach
//! arguments); `i2c` the bus driver (`iic* at piixpm?`, `iic* at ichiic?`), `i2c_exec` the
//! scripted client interface (`iic_exec`, the SMBus operations) and `i2c_scan` the bus scan
//! with its probe heuristics (M16e). The controllers are `ichiic(4)` and `piixpm(4)` in
//! `dev/pci/`; the chip drivers on the bus (`spdmem`, `lm`, ...) are not ported.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/i2c/i2c.c
pub mod i2c;
pub mod i2c_exec;
pub mod i2c_io;
pub mod i2c_scan;
pub mod i2cvar;
/* </CODE> */
