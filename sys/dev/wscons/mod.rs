//! wscons, the workstation console: OpenBSD `sys/dev/wscons/`. The keyboard half
//! `hidkbd(4)` needs, the display interface the frame buffers fill, `wsdisplay(4)` and the
//! terminal emulations are here; the rest (`wskbd.c`, the mux, `wsevent.c`, the mouse) is
//! milestone M13's keyboard step.
//!
//! `wsconsio` holds the event, keyboard and display ioctl definitions of
//! `<dev/wscons/wsconsio.h>`, `wsksymdef` the keysyms and layout codes, `wsksymvar` the keymap
//! types, `wskbdvar` the interface between keyboard drivers and `wskbd(4)`, `wskbd` the
//! callbacks keyboard drivers call (stubs until M13, apart from `wskbddevprint`), and
//! `wsdisplayvar` the interface between display drivers and `wsdisplay(4)`.
//!
//! `wsdisplay` is `wsdisplay(4)` itself (virtual screens, their ttys `ttyC*`, the console
//! output, screen switching, the `wsmoused(8)` selection), `wsdisplay_compat_usl` its USL
//! `VT_*`/`KD*` ioctls (`WSDISPLAY_COMPAT_USL`) and `wsdisplay_usl_io` their definitions.
//!
//! `wsemulvar` is the interface between `wsdisplay(4)` and its terminal emulations,
//! `wsemulconf` their list, `wsemul_subr` their shared UTF-8 and keysym helpers,
//! `wsemul_dumb` and `wsemul_vt100*` the emulations, `ascii`, `unicode` and
//! `wscons_features` the small headers they use, and `wscons_callbacks` the calls between
//! wsdisplay and wskbd.

pub mod ascii;
#[cfg(test)]
pub(crate) mod testutil;
pub mod unicode;
pub mod wscons_callbacks;
pub mod wscons_features;
pub mod wsconsio;
pub mod wsdisplay;
pub mod wsdisplay_compat_usl;
pub mod wsdisplay_usl_io;
pub mod wsdisplayvar;
pub mod wsemul_dumb;
pub mod wsemul_subr;
pub mod wsemul_vt100;
pub mod wsemul_vt100_chars;
pub mod wsemul_vt100_keys;
pub mod wsemul_vt100_subr;
pub mod wsemul_vt100var;
pub mod wsemulconf;
pub mod wsemulvar;
pub mod wskbd;
pub mod wskbdvar;
pub mod wsksymdef;
pub mod wsksymvar;
