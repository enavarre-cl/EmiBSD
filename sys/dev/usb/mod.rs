//! The USB stack: OpenBSD `sys/dev/usb/`.
//!
//! The machine-independent core (M12): the wire definitions and the bus driver (`usb`), the
//! driver interface (`usbdi`, `usbdi_util`), the shared structures (`usbdivar`), device
//! enumeration (`usb_subr`), DMA memory (`usb_mem`), quirks, IDs (`usbdevs`), the HID class
//! definitions (`usbhid`) and the capture headers (`usbpcap`). Host controller and device
//! drivers (`xhci`, `uhub`, `umass`, `uhidev`, ...) attach on top of it.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/usb/usb.c
pub mod usb;
pub mod usb_mem;
pub mod usb_quirks;
pub mod usb_subr;
pub mod usbdevs;
pub mod usbdi;
pub mod usbdi_util;
pub mod usbdivar;
pub mod usbhid;
pub mod usbpcap;
