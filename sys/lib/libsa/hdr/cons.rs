/* <CODE> */
//! `<dev/cons.h>` for libsa: `struct consdev`, a console the boot program may use, and its
//! priorities.

use core::sync::atomic::{AtomicI32, Ordering};

use super::types::Dev;

/// `CN_DEAD`: the device does not exist.
pub const CN_DEAD: i32 = 0;
/// `CN_LOWPRI`: the device is a last resort.
pub const CN_LOWPRI: i32 = 1;
/// `CN_MIDPRI`.
pub const CN_MIDPRI: i32 = 2;
/// `CN_HIGHPRI`.
pub const CN_HIGHPRI: i32 = 3;

/// `struct consdev`: the standalone half (probe, init, getc, putc, the device and its
/// priority). `cn_getc` is called with `dev | 0x80` to poll: it then returns non-zero if a
/// character is waiting, without consuming it.
pub struct ConsDev {
    /// `cn_probe`: probe the hardware and fill in `cn_dev` and `cn_pri`.
    pub cn_probe: fn(&ConsDev),
    /// `cn_init`: turn on the device.
    pub cn_init: fn(&ConsDev),
    /// `cn_getc`: the next character, or the poll result with `0x80` in the device.
    pub cn_getc: fn(Dev) -> i32,
    /// `cn_putc`: write a character.
    pub cn_putc: fn(Dev, i32),
    /// `cn_dev`: the device number (atomic: the probe routine sets it through `&self`).
    pub cn_dev: AtomicI32,
    /// `cn_pri`: the priority the probe found (atomic, as `cn_dev`).
    pub cn_pri: AtomicI32,
}

impl ConsDev {
    /// A console entry for `constab[]`, not probed yet.
    pub const fn new(
        cn_probe: fn(&ConsDev),
        cn_init: fn(&ConsDev),
        cn_getc: fn(Dev) -> i32,
        cn_putc: fn(Dev, i32),
    ) -> Self {
        Self {
            cn_probe,
            cn_init,
            cn_getc,
            cn_putc,
            cn_dev: AtomicI32::new(0),
            cn_pri: AtomicI32::new(CN_DEAD),
        }
    }

    /// `cn_dev`.
    pub fn dev(&self) -> Dev {
        self.cn_dev.load(Ordering::Relaxed)
    }

    /// Sets `cn_dev`.
    pub fn set_dev(&self, dev: Dev) {
        self.cn_dev.store(dev, Ordering::Relaxed);
    }

    /// `cn_pri`.
    pub fn pri(&self) -> i32 {
        self.cn_pri.load(Ordering::Relaxed)
    }

    /// Sets `cn_pri`.
    pub fn set_pri(&self, pri: i32) {
        self.cn_pri.store(pri, Ordering::Relaxed);
    }
}
/* </CODE> */
