/* <LICENSES> */
/*-
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Juergen Hannken-Illjes.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/wscons/wsksymvar.h>`: the keymap types of wscons: a keysym, a keyboard layout code,
//! one entry of a keymap, and the table that describes a keyboard's layouts.
//!
//! Upstream: sys/dev/wscons/wsksymvar.h @ 3ce1f3f79392
//!
//! A keyboard driver owns a [`WskbdMapdata`]: its table of [`WsconsKeydesc`]s (one per layout,
//! each the keymap of a code or a delta over a base layout) and the layout in force.
//! `wskbd(4)` turns key codes into keysyms through it.
//!
//! ## Deviations
//! - `keysym_t` and `kbd_t` are [`KeysymT`] (`u16`) and [`KbdT`] (`u32`).
//! - `wscons_keydesc`'s `map` and `map_size` are one slice, `map`; the table of descriptions
//!   (`const struct wscons_keydesc *keydesc`, ended by an entry whose `name` is 0) is a slice
//!   without that last entry, as `cfdata[]` is in `sys/sys/device.rs`. `map_size` stays as the
//!   C's field, equal to `map.len()`.
//! - `wskbd_mapdata.layout` is an `AtomicU32`: a driver changes it at attach (`hidkbd` writes
//!   `ukbd_keymapdata.layout`) through a shared `static`.
//! - The prototypes at the end of the header (`wskbd_get_mapentry`, `wskbd_init_keymap`,
//!   `wskbd_load_keymap`, `wskbd_compose_value`) belong to `wskbdutil.c`; they are
//!   `wskbdutil.rs`'s.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::dev::wscons::wsksymdef::{KB_DEFAULT, KB_METAESC, KB_NOENCODING};

/// `keysym_t`: a keysym, a 16-bit Unicode code point or a private symbol.
pub type KeysymT = u16;

/// `kbd_t`: a keyboard layout: an 8-bit encoding and a 24-bit variant.
pub type KbdT = u32;

/// `KB_HANDLEDBYWSKBD`: layout variant bits ignored by mapping code.
pub const KB_HANDLEDBYWSKBD: KbdT = KB_METAESC | KB_DEFAULT | KB_NOENCODING;

/// `struct wscons_keymap`: the symbols one key stands for.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsconsKeymap {
    /// `command`: the command symbol (with the Alt-Gr/command modifier).
    pub command: KeysymT,
    /// `group1`: the symbols of group 1 (plain, shifted).
    pub group1: [KeysymT; 2],
    /// `group2`: the symbols of group 2.
    pub group2: [KeysymT; 2],
}

/// `struct wscons_keydesc`: the description of one layout.
#[derive(Clone, Copy, Debug)]
pub struct WsconsKeydesc {
    /// `name`: name of this map.
    pub name: KbdT,
    /// `base`: map this one is based on.
    pub base: KbdT,
    /// `map_size`: size of map.
    pub map_size: i32,
    /// `map`: the map itself.
    pub map: &'static [KeysymT],
}

/// `struct wskbd_mapdata`: a keyboard's table of layouts and the one in force.
pub struct WskbdMapdata {
    /// `keydesc`: the layouts the keyboard knows.
    pub keydesc: &'static [WsconsKeydesc],
    /// `layout`: the layout in force (`KB_*`).
    layout: AtomicU32,
}

impl WskbdMapdata {
    /// A map table for `keydesc` with layout `layout`.
    pub const fn new(keydesc: &'static [WsconsKeydesc], layout: KbdT) -> Self {
        Self {
            keydesc,
            layout: AtomicU32::new(layout),
        }
    }

    /// `layout`.
    pub fn layout(&self) -> KbdT {
        self.layout.load(Ordering::Relaxed)
    }

    /// `layout = l`.
    pub fn set_layout(&self, l: KbdT) {
        self.layout.store(l, Ordering::Relaxed);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::wscons::wsksymdef::{KB_DE, KB_US};

    #[test]
    fn handled_by_wskbd_is_the_three_attach_bits() {
        assert_eq!(KB_HANDLEDBYWSKBD, 0x8000_00a0);
    }

    #[test]
    fn mapdata_layout_is_settable_through_a_shared_reference() {
        static MAP: WskbdMapdata = WskbdMapdata::new(&[], KB_US);
        assert_eq!(MAP.layout(), KB_US);
        MAP.set_layout(KB_DE);
        assert_eq!(MAP.layout(), KB_DE);
        assert!(MAP.keydesc.is_empty());
    }

    #[test]
    fn keymap_entry_is_five_keysyms() {
        assert_eq!(core::mem::size_of::<WsconsKeymap>(), 10);
    }
}
/* </TESTS> */
