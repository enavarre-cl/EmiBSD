//! wscons, the workstation console: OpenBSD `sys/dev/wscons/`. The keyboard half
//! `hidkbd(4)` needs, the display interface the frame buffers fill and the terminal
//! emulations are here; the rest (`wsdisplay.c`, the mux) is milestone M13.
//!
//! `wsconsio` holds the event, keyboard and display ioctl definitions of
//! `<dev/wscons/wsconsio.h>`, `wsksymdef` the keysyms and layout codes, `wsksymvar` the keymap
//! types, `wskbdvar` the interface between keyboard drivers and `wskbd(4)`, `wskbd` the
//! callbacks keyboard drivers call (stubs until M13, apart from `wskbddevprint`), and
//! `wsdisplayvar` the interface between display drivers and `wsdisplay(4)`.
//!
//! `wsemulvar` is the interface between `wsdisplay(4)` and its terminal emulations,
//! `wsemulconf` their list, `wsemul_subr` their shared UTF-8 and keysym helpers,
//! `wsemul_dumb` and `wsemul_vt100*` the emulations, `ascii`, `unicode` and
//! `wscons_features` the small headers they use, and `wscons_callbacks` the calls between
//! wsdisplay and wskbd (stubs until `wsdisplay.c` is ported).

pub mod ascii;
#[cfg(test)]
pub(crate) mod testutil;
pub mod unicode;
pub mod wscons_callbacks;
pub mod wscons_features;
pub mod wsconsio;
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
