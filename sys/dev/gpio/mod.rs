/* <CODE> */
//! The GPIO framework: OpenBSD `sys/dev/gpio/`.
//!
//! `gpiovar` holds the controller and attach-argument types, `gpio` the gpio(4) bus and its
//! device (M16f).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/gpio/gpio.c
pub mod gpio;
pub mod gpiovar;
/* </CODE> */
