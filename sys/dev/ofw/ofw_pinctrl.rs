/* <LICENSES> */
/*
 * Copyright (c) 2016 Mark Kettenis
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/*
 * Copyright (c) 2016 Mark Kettenis
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! The device tree's pin controllers: `dev/ofw/ofw_pinctrl.h` and `dev/ofw/ofw_pinctrl.c`.
//!
//! Upstream: sys/dev/ofw/ofw_pinctrl.h @ 3ce1f3f79392
//! Upstream: sys/dev/ofw/ofw_pinctrl.c @ 3ce1f3f79392
//!
//! A pin controller driver registers the configuration nodes below its node; a device
//! selects its `pinctrl-N` (or `pinctrl-names`) states by phandle. No pin controller is
//! ported (QEMU's `virt` has none), so on QEMU the lookups find nothing and return -1, as
//! the C does on a board whose controller is not configured.
//!
//! ## Deviations
//! - `pinctrls` is a `static` behind a `Sync` wrapper ([`Pinctrls`]): controllers register
//!   while autoconfiguring and the list is only read after, as the C's unlocked list.
//! - The list elements are `Box`es (`malloc` with `M_DEVBUF`, never freed); the `phandles`
//!   of `pinctrl_byid` a `Vec` (`M_TEMP`).

use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;
use core::fmt::Write;

use crate::dev::ofw::fdt::{
    OF_child, OF_getindex, OF_getpropint, OF_getpropintarray, OF_getproplen, OF_peer,
};
use crate::queue_adapter;
use crate::sys::queue::{ListEntry, ListHead};

/// The `int (*)(uint32_t, void *)` a pin controller registers: apply the configuration
/// node with this phandle.
pub type PinctrlFn = fn(u32, *mut c_void) -> i32;

/// `struct pinctrl`.
pub struct Pinctrl {
    /// `pc_phandle`.
    pub pc_phandle: u32,
    /// `pc_pinctrl`.
    pub pc_pinctrl: PinctrlFn,
    /// `pc_cookie`.
    pub pc_cookie: *mut c_void,

    /// `pc_list`.
    pub pc_list: ListEntry<Pinctrl>,
}

queue_adapter!(
    /// `LIST_HEAD(, pinctrl)` through `pc_list`.
    pub PinctrlList: Pinctrl, pc_list => ListEntry<Pinctrl>
);

/// `pinctrls`, behind a wrapper that can be a `static`.
pub struct Pinctrls(ListHead<PinctrlList>);

// SAFETY: pin controllers register while autoconfiguring (kernel lock) and the list is only
// read afterwards, as the C's unlocked list.
unsafe impl Sync for Pinctrls {}

/// `pinctrls`.
pub static PINCTRLS: Pinctrls = Pinctrls(ListHead::new());

/// `pinctrl_register`: every configuration node below `node`.
pub fn pinctrl_register(node: i32, pinctrl: PinctrlFn, cookie: *mut c_void) {
    let mut node = OF_child(node);
    while node != 0 {
        pinctrl_register_child(node, pinctrl, cookie);
        node = OF_peer(node);
    }
}

/// `pinctrl_register_child`: `node` if it has a phandle, then its children.
pub fn pinctrl_register_child(node: i32, pinctrl: PinctrlFn, cookie: *mut c_void) {
    let phandle = OF_getpropint(node, b"phandle", 0);
    if phandle != 0 {
        let pc: &'static Pinctrl = Box::leak(Box::new(Pinctrl {
            pc_phandle: phandle,
            pc_pinctrl: pinctrl,
            pc_cookie: cookie,
            pc_list: ListEntry::new(),
        }));
        // SAFETY: a fresh element that lives forever.
        unsafe { PINCTRLS.0.insert_head(pc) };
    }

    let mut node = OF_child(node);
    while node != 0 {
        pinctrl_register_child(node, pinctrl, cookie);
        node = OF_peer(node);
    }
}

/// `pinctrl_byphandle`: apply the configuration node `phandle`; -1 if no controller has it.
pub fn pinctrl_byphandle(phandle: u32) -> i32 {
    if phandle == 0 {
        return -1;
    }

    match PINCTRLS.0.iter().find(|pc| pc.pc_phandle == phandle) {
        Some(pc) => (pc.pc_pinctrl)(pc.pc_phandle, pc.pc_cookie),
        None => -1,
    }
}

/// A small buffer `snprintf` writes into, truncating as the C does.
struct NameBuf {
    buf: [u8; 32],
    len: usize,
}

impl Write for NameBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() - 1 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

/// `pinctrl_byid`: apply every configuration `pinctrl-<id>` names.
pub fn pinctrl_byid(node: i32, id: i32) -> i32 {
    let mut name = NameBuf {
        buf: [0; 32],
        len: 0,
    };
    let _ = write!(name, "pinctrl-{id}");
    let pinctrl = &name.buf[..name.len];

    let len = OF_getproplen(node, pinctrl);
    if len <= 0 {
        return -1;
    }

    let mut phandles = vec![0u32; len as usize / size_of::<u32>()];
    OF_getpropintarray(node, pinctrl, &mut phandles);
    for &phandle in &phandles {
        pinctrl_byphandle(phandle);
    }
    0
}

/// `pinctrl_byname`: apply the state `pinctrl-names` calls `config`.
pub fn pinctrl_byname(node: i32, config: &[u8]) -> i32 {
    let id = OF_getindex(node, Some(config), b"pinctrl-names");
    if id < 0 {
        return -1;
    }

    pinctrl_byid(node, id)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ptr;

    use super::*;

    fn apply(phandle: u32, cookie: *mut c_void) -> i32 {
        // SAFETY: the test's cookie is a leaked `Cell<u32>`.
        unsafe { &*cookie.cast::<Cell<u32>>() }.set(phandle);
        0
    }

    #[test]
    fn byphandle_dispatches_to_the_registered_controller() {
        let last: &'static Cell<u32> = Box::leak(Box::new(Cell::new(0)));
        let pc: &'static Pinctrl = Box::leak(Box::new(Pinctrl {
            pc_phandle: 0x77,
            pc_pinctrl: apply,
            pc_cookie: ptr::from_ref(last).cast_mut().cast(),
            pc_list: ListEntry::new(),
        }));
        // SAFETY: a leaked element on no list, as pinctrl_register_child puts it.
        unsafe { PINCTRLS.0.insert_head(pc) };

        assert_eq!(pinctrl_byphandle(0x77), 0);
        assert_eq!(last.get(), 0x77);
        assert_eq!(pinctrl_byphandle(0), -1);
        assert_eq!(pinctrl_byphandle(0x78), -1);
        // No tree: no pinctrl-0 property, no pinctrl-names.
        assert_eq!(pinctrl_byid(0, 0), -1);
        assert_eq!(pinctrl_byname(0, b"default"), -1);
    }

    #[test]
    fn snprintf_truncates() {
        let mut n = NameBuf {
            buf: [0; 32],
            len: 0,
        };
        let _ = write!(n, "pinctrl-{}", 12);
        assert_eq!(&n.buf[..n.len], b"pinctrl-12");
        let _ = write!(n, "{}", "x".repeat(40));
        assert_eq!(n.len, 31);
    }
}
/* </TESTS> */
