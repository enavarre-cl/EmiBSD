/* <CODE> */
//! HID class support: OpenBSD `sys/dev/hid/`.
//!
//! `hid` is the report descriptor parser with `<dev/hid/hid.h>`; `hidkbd` is the keyboard
//! logic `ukbd(4)` and `ikbd(4)` wrap, with `hidkbdsc.h` and `hidkbdvar.h`. `hidms` is the mouse, tablet and
//! touch panel logic `ums(4)` and `uwacom(4)` wrap, with `hidmsvar.h`. `hidmt` and `hidcc`
//! (multitouch, consumer control) are not ported yet.

#[allow(clippy::module_inception)] // OpenBSD's layout: dev/hid/hid.c
pub mod hid;
pub mod hidkbd;
pub mod hidms;
pub mod hidmsvar;
/* </CODE> */
