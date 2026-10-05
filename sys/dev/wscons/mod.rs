//! wscons, the workstation console: OpenBSD `sys/dev/wscons/`. Only the keyboard half
//! `hidkbd(4)` needs is here; the rest is milestone M13.
//!
//! `wsconsio` holds the event and keyboard ioctl definitions of `<dev/wscons/wsconsio.h>`,
//! `wsksymdef` the keysyms and layout codes, `wsksymvar` the keymap types, `wskbdvar` the
//! interface between keyboard drivers and `wskbd(4)`, `wskbd` the callbacks keyboard drivers
//! call (stubs until M13, apart from `wskbddevprint`).

pub mod wsconsio;
pub mod wskbd;
pub mod wskbdvar;
pub mod wsksymdef;
pub mod wsksymvar;
