//! wscons, the workstation console: OpenBSD `sys/dev/wscons/`: the keyboard (`wskbd(4)`), the
//! display (`wsdisplay(4)`) with its terminal emulations, the mux (`wsmux(4)`) between them,
//! and the event queues; the mouse (`wsmouse.c`, `wstpad.c`) is not ported.
//!
//! `wsconsio` holds the event, keyboard and display ioctl definitions of
//! `<dev/wscons/wsconsio.h>`, `wsksymdef` the keysyms and layout codes, `wsksymvar` the keymap
//! types, `wskbdvar` the interface between keyboard drivers and `wskbd(4)`, `wskbd` the
//! keyboard driver itself, `wskbdutil` its keymap and compose helpers, `wskbdraw` the XT
//! scancodes of raw mode, and `wsdisplayvar` the interface between display drivers and
//! `wsdisplay(4)`. `wsmux` and `wsmuxvar` are the mux and the event sources it merges,
//! `wsevent` and `wseventvar` the event queue a reader of `/dev/wskbd*` or `/dev/wsmux*`
//! gets.
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
pub mod wsevent;
pub mod wseventvar;
pub mod wskbd;
pub mod wskbdraw;
pub mod wskbdutil;
pub mod wskbdvar;
pub mod wsksymdef;
pub mod wsksymvar;
pub mod wsmux;
pub mod wsmuxvar;
