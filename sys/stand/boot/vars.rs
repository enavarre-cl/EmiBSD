/*	$OpenBSD: vars.c,v 1.17 2023/03/13 20:19:22 miod Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998-2000 Michael Shalayeff
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! boot(8)'s variables (`set howto`, `device`, `tty`, `image`, `timeout`, `db_console`), the
//! boot flags parser and the environment (`env`).
//!
//! Upstream: sys/stand/boot/vars.c @ 3ce1f3f79392
//!
//! Compiled with `BOOT_STTY`, without `DEBUG` (no `debug` variable).
//!
//! ## Deviations
//! - The variables are commands on `&mut CmdState` (see `cmd.rs`). `db_console` is the
//!   atomic [`DB_CONSOLE`]; `environ` is a byte vector capped at the C's 4096 bytes.
//! - `ttyname()`, `ttydev()` and `cnset()`'s device come from the program (`BOOT_STTY`
//!   hooks of [`BootMd`](crate::boot::BootMd)).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::staticcell::StaticCell;
use libsa::cons::cnset;
use libsa::hdr::reboot::{RB_ASKNAME, RB_CONFIG, RB_KDB, RB_SINGLE};
use libsa::hdr::types::NODEV;
use libsa::printf;
use libsa::printf::Str;
use libsa::putchar::putchar;
use libsa::strtol::strtol;

use crate::boot::{boot_md, prog_ident};
use crate::cmd::{BOOTDEVLEN, CMDT_VAR, CmdState, CmdTable, strlcpy};

/// `ASKNAME_LETTER`: `-a` asks for the root device (`n` on alpha).
const ASKNAME_LETTER: u8 = b'a';

/// `environ`'s size limit (the C `alloc(4096)`s it).
const ENVIRON_MAX: usize = 4096;

/// `cmd_set[]`: the variables of `set`.
pub static CMD_SET: [CmdTable; 6] = [
    CmdTable {
        cmd_name: "howto",
        cmd_type: CMDT_VAR,
        cmd_exec: Xhowto,
    },
    CmdTable {
        cmd_name: "device",
        cmd_type: CMDT_VAR,
        cmd_exec: Xdevice,
    },
    CmdTable {
        cmd_name: "tty",
        cmd_type: CMDT_VAR,
        cmd_exec: Xtty,
    },
    CmdTable {
        cmd_name: "image",
        cmd_type: CMDT_VAR,
        cmd_exec: Ximage,
    },
    CmdTable {
        cmd_name: "timeout",
        cmd_type: CMDT_VAR,
        cmd_exec: Xtimeout,
    },
    CmdTable {
        cmd_name: "db_console",
        cmd_type: CMDT_VAR,
        cmd_exec: Xdb_console,
    },
];

/// `db_console`: -1 unset, 0 off, 1 on (passed to the kernel as `BOOTARG_DDB`).
pub static DB_CONSOLE: AtomicI32 = AtomicI32::new(-1);

/// `environ`: `name=value` lines, `None` until the first `env name`.
pub static ENVIRON: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);

/// `set db_console [on|off]`.
#[allow(non_snake_case)] // the C's command names
pub fn Xdb_console(cmd: &mut CmdState) -> i32 {
    if cmd.argc != 2 {
        match DB_CONSOLE.load(Ordering::Relaxed) {
            0 => printf!("off\n"),
            1 => printf!("on\n"),
            _ => printf!("unset\n"),
        }
    } else {
        match cmd.arg(1).unwrap_or(b"") {
            b"0" | b"off" => DB_CONSOLE.store(0, Ordering::Relaxed),
            b"1" | b"on" => DB_CONSOLE.store(1, Ordering::Relaxed),
            _ => {}
        }
    }
    0
}

/// `set timeout [secs]`.
#[allow(non_snake_case)] // the C's command names
fn Xtimeout(cmd: &mut CmdState) -> i32 {
    if cmd.argc != 2 {
        printf!("{}\n", cmd.timeout);
    } else {
        cmd.timeout = strtol(cmd.arg(1).unwrap_or(b""), 0).0 as i32;
    }
    0
}

/// `set`: called only with no arguments: every variable and its value.
#[allow(non_snake_case)] // the C's command names
pub fn Xset(cmd: &mut CmdState) -> i32 {
    printf!("{}\n", Str(&prog_ident()));
    for ct in &CMD_SET {
        printf!("{}\t ", ct.cmd_name);
        (ct.cmd_exec)(cmd);
    }
    0
}

/// `set device [dev]`.
#[allow(non_snake_case)] // the C's command names
fn Xdevice(cmd: &mut CmdState) -> i32 {
    if cmd.argc != 2 {
        printf!("{}\n", Str(&cmd.bootdev));
    } else {
        let mut dev = [0u8; BOOTDEVLEN];
        strlcpy(&mut dev, cmd.arg(1).unwrap_or(b""));
        strlcpy(&mut cmd.bootdev, &dev);
    }
    0
}

/// `set image [path]`.
#[allow(non_snake_case)] // the C's command names
fn Ximage(cmd: &mut CmdState) -> i32 {
    if cmd.argc != 2 {
        printf!("{}\n", Str(&cmd.image));
    } else {
        let mut image = [0u8; 1024];
        strlcpy(&mut image, cmd.arg(1).unwrap_or(b""));
        strlcpy(&mut cmd.image, &image);
    }
    0
}

/// `set tty [dev]`: switch the console.
#[allow(non_snake_case)] // the C's command names
fn Xtty(cmd: &mut CmdState) -> i32 {
    let Some(stty) = boot_md().stty.as_ref() else {
        return 0;
    };
    if cmd.argc != 2 {
        printf!("{}\n", Str(&(stty.ttyname)()));
    } else {
        let name = cmd.arg(1).unwrap_or(b"");
        let dev = (stty.ttydev)(name);
        if dev == NODEV {
            printf!("{} not a console device\n", Str(name));
        } else {
            printf!("switching console to {}\n", Str(name));
            if !cnset(dev) {
                printf!("{} console not present\n", Str(name));
            } else {
                printf!("{}\n", Str(&prog_ident()));
            }
        }
    }
    0
}

/// `set howto [-acds]`.
#[allow(non_snake_case)] // the C's command names
fn Xhowto(cmd: &mut CmdState) -> i32 {
    if cmd.argc == 1 {
        if cmd.boothowto != 0 {
            putchar(i32::from(b'-'));
            if cmd.boothowto & RB_ASKNAME != 0 {
                putchar(i32::from(ASKNAME_LETTER));
            }
            if cmd.boothowto & RB_CONFIG != 0 {
                putchar(i32::from(b'c'));
            }
            if cmd.boothowto & RB_SINGLE != 0 {
                putchar(i32::from(b's'));
            }
            if cmd.boothowto & RB_KDB != 0 {
                putchar(i32::from(b'd'));
            }
        }
        putchar(i32::from(b'\n'));
    } else {
        bootparse(cmd, 1);
    }
    0
}

/// `bootparse(i)`: the `-acds` flags from `argv[i]` on into `boothowto`; 1 (and a message)
/// on a bad one.
pub fn bootparse(cmd: &mut CmdState, i: usize) -> i32 {
    let mut howto = cmd.boothowto;

    for i in i..cmd.argc {
        let cp = cmd.arg(i).unwrap_or(b"");
        if cp.first() == Some(&b'-') {
            for &c in &cp[1..] {
                match c {
                    ASKNAME_LETTER => howto |= RB_ASKNAME,
                    b'c' => howto |= RB_CONFIG,
                    b's' => howto |= RB_SINGLE,
                    b'd' => howto |= RB_KDB,
                    _ => {
                        printf!("howto: bad option: {}\n", char::from(c));
                        return 1;
                    }
                }
            }
        } else {
            printf!("boot: illegal argument {}\n", Str(cp));
            return 1;
        }
    }
    cmd.boothowto = howto;
    0
}

/// `env [name [value]]`: print the environment, or set `name=value` in it (a sequence of
/// `\n`-separated `name=value` definitions).
#[allow(non_snake_case)] // the C's command names
pub fn Xenv(cmd: &mut CmdState) -> i32 {
    // SAFETY: boot(8) is single-threaded; this is the only reference to ENVIRON.
    let environ = unsafe { ENVIRON.get_mut() };
    if cmd.argc == 1 {
        match environ {
            Some(env) => printf!("{}", Str(env)),
            None => printf!("empty\n"),
        }
        return 0;
    }

    let name = cmd.arg(1).unwrap_or(b"");
    let env = environ.get_or_insert_with(Vec::new);
    // drop the old definitions of `name`
    let mut kept = Vec::with_capacity(env.len());
    for line in env.split_inclusive(|&c| c == b'\n') {
        let var = &line[..line.iter().position(|&c| c == b'=').unwrap_or(line.len())];
        if var != name {
            kept.extend_from_slice(line);
        }
    }
    *env = kept;
    let value = if cmd.argc == 3 {
        cmd.arg(2).unwrap_or(b"")
    } else {
        b""
    };
    for &c in name.iter().chain(b"=").chain(value).chain(b"\n") {
        if env.len() + 1 < ENVIRON_MAX {
            env.push(c);
        }
    }
    0
}
/* </CODE> */
