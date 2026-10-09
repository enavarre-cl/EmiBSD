/* <LICENSES> */
/*
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The terminal emulations compiled into the kernel, and their lookup by name.
//!
//! Upstream: sys/dev/wscons/wsemulconf.c @ 3ce1f3f79392
//!
//! `wsemul_conf` lists them in the C's order: `sun` (option `WSEMUL_SUN`), `vt100` (unless
//! option `WSEMUL_NO_VT100`), `dumb` (option `WSEMUL_DUMB`). The first is the default.
//! amd64 and arm64 `GENERIC` set none of these options, so the list is `vt100` alone.
//!
//! ## Deviations
//! - The options are the cargo features `wsemul_dumb` and `wsemul_no_vt100`
//!   (`sys/Cargo.toml`); `WSEMUL_SUN` has none, since `wsemul_sun.c` is not ported (only
//!   sparc64 configurations set it). The emulations themselves are compiled on every build,
//!   so their host tests run; the features only decide what the list offers.
//! - `WSEMUL_DEFAULT` (an option whose value is a name) is [`WSEMUL_DEFAULT`], `None` as in
//!   a kernel without it.
//! - The NULL-terminated array is a slice; `wsemul_pick` returns `None` for the C's NULL and
//!   takes the name as bytes (up to a NUL), `None` for a NULL name.

use crate::dev::wscons::wsconsio::WSEMUL_NAME_SIZE;
use crate::dev::wscons::wsemulvar::WsemulOps;

/// `WSEMUL_DEFAULT`: the emulation `wsemul_pick` chooses for an empty name, when the kernel
/// configuration names one; otherwise the first of the list.
pub const WSEMUL_DEFAULT: Option<&[u8]> = None;

/// `wsemul_conf`: the emulations compiled in, the default first.
static WSEMUL_CONF: &[&WsemulOps] = &[
    #[cfg(not(feature = "wsemul_no_vt100"))]
    &crate::dev::wscons::wsemul_vt100::WSEMUL_VT100_OPS,
    #[cfg(feature = "wsemul_dumb")]
    &crate::dev::wscons::wsemul_dumb::WSEMUL_DUMB_OPS,
];

/// The bytes of a C string up to its NUL (all of them when there is none).
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `wsemul_pick`: the emulation called `name`, compared over at most `WSEMUL_NAME_SIZE`
/// bytes; the default one for `None` or an empty name. `None` when there is no such
/// emulation.
pub fn wsemul_pick(name: Option<&[u8]>) -> Option<&'static WsemulOps> {
    let mut name = name.map(cstr).unwrap_or(&[]);
    if name.is_empty() {
        // default
        match WSEMUL_DEFAULT {
            Some(d) => name = d,
            None => return WSEMUL_CONF.first().copied(),
        }
    }
    let name = &name[..name.len().min(WSEMUL_NAME_SIZE)];
    WSEMUL_CONF.iter().copied().find(|ops| {
        let ours = &ops.name()[..ops.name().len().min(WSEMUL_NAME_SIZE)];
        ours == name
    })
}

/// `wsemul_getname`: the name of the `idx`th emulation, `None` past the end.
pub fn wsemul_getname(idx: i32) -> Option<&'static [u8]> {
    let idx = usize::try_from(idx).ok()?;
    WSEMUL_CONF.get(idx).map(|ops| ops.name())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_list_is_vt100() {
        let vt100 = wsemul_pick(None).unwrap();
        assert_eq!(vt100.name(), b"vt100");
        assert!(core::ptr::eq(wsemul_pick(Some(b"")).unwrap(), vt100));
        assert!(core::ptr::eq(
            wsemul_pick(Some(b"vt100\0junk")).unwrap(),
            vt100
        ));
        assert!(wsemul_pick(Some(b"vt10")).is_none());
        assert!(wsemul_pick(Some(b"sun")).is_none());
        assert_eq!(
            wsemul_pick(Some(b"dumb")).is_some(),
            cfg!(feature = "wsemul_dumb")
        );
        assert_eq!(wsemul_getname(0), Some(&b"vt100"[..]));
        assert_eq!(wsemul_getname(-1), None);
        assert_eq!(wsemul_getname(WSEMUL_CONF.len() as i32), None);
    }
}
/* </TESTS> */
