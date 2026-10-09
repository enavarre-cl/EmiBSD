/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
 * Copyright (c) 1997-1999 Michael Shalayeff
 * Copyright (c) 1997 Tobias Weingartner
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
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! efiboot's `machine` commands: `comaddr`, `diskinfo`, `memory`, `video`, `gop`, `exit`,
//! `poweroff`, `fwsetup` (and `idle` with softraid).
//!
//! Upstream: sys/arch/amd64/stand/efiboot/cmd_i386.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The commands take `&mut CmdState` (see boot(8)'s `cmd.rs`). `Xregs` is `DEBUG` only, as
//!   in the C, and not ported.

use core::sync::atomic::Ordering;

use boot::cmd::{CMDT_CMD, CmdState, CmdTable};
use libsa::printf;
use libsa::strtol::strtol;
use libsa::strtoll::strtoll;

use crate::efiboot::{
    BIOS_MEMMAP_MODIFIED, COM_ADDR, Xexit_efi, Xfwsetup_efi, Xgop_efi, Xpoweroff_efi, Xvideo_efi,
};
use crate::efidev::efi_dump_diskinfo;
use crate::memprobe::{dump_biosmem, mem_add, mem_delete, mem_limit};

/// `cmd_machine[]`.
#[cfg(not(feature = "softraid"))]
pub static CMD_MACHINE: [CmdTable; 8] = [
    CmdTable {
        cmd_name: "comaddr",
        cmd_type: CMDT_CMD,
        cmd_exec: Xcomaddr,
    },
    CmdTable {
        cmd_name: "diskinfo",
        cmd_type: CMDT_CMD,
        cmd_exec: Xdiskinfo,
    },
    CmdTable {
        cmd_name: "memory",
        cmd_type: CMDT_CMD,
        cmd_exec: Xmemory,
    },
    CmdTable {
        cmd_name: "video",
        cmd_type: CMDT_CMD,
        cmd_exec: Xvideo_efi,
    },
    CmdTable {
        cmd_name: "gop",
        cmd_type: CMDT_CMD,
        cmd_exec: Xgop_efi,
    },
    CmdTable {
        cmd_name: "exit",
        cmd_type: CMDT_CMD,
        cmd_exec: Xexit_efi,
    },
    CmdTable {
        cmd_name: "poweroff",
        cmd_type: CMDT_CMD,
        cmd_exec: Xpoweroff_efi,
    },
    CmdTable {
        cmd_name: "fwsetup",
        cmd_type: CMDT_CMD,
        cmd_exec: Xfwsetup_efi,
    },
];

/// `cmd_machine[]`, with `IDLE_POWEROFF`'s `idle`.
#[cfg(feature = "softraid")]
pub static CMD_MACHINE: [CmdTable; 9] = [
    CmdTable {
        cmd_name: "comaddr",
        cmd_type: CMDT_CMD,
        cmd_exec: Xcomaddr,
    },
    CmdTable {
        cmd_name: "diskinfo",
        cmd_type: CMDT_CMD,
        cmd_exec: Xdiskinfo,
    },
    CmdTable {
        cmd_name: "memory",
        cmd_type: CMDT_CMD,
        cmd_exec: Xmemory,
    },
    CmdTable {
        cmd_name: "video",
        cmd_type: CMDT_CMD,
        cmd_exec: Xvideo_efi,
    },
    CmdTable {
        cmd_name: "gop",
        cmd_type: CMDT_CMD,
        cmd_exec: Xgop_efi,
    },
    CmdTable {
        cmd_name: "exit",
        cmd_type: CMDT_CMD,
        cmd_exec: Xexit_efi,
    },
    CmdTable {
        cmd_name: "poweroff",
        cmd_type: CMDT_CMD,
        cmd_exec: Xpoweroff_efi,
    },
    CmdTable {
        cmd_name: "idle",
        cmd_type: CMDT_CMD,
        cmd_exec: crate::efiboot::Xidle_efi,
    },
    CmdTable {
        cmd_name: "fwsetup",
        cmd_type: CMDT_CMD,
        cmd_exec: Xfwsetup_efi,
    },
];

/// `machine diskinfo`.
#[allow(non_snake_case)] // the C's command names
fn Xdiskinfo(_cmd: &mut CmdState) -> i32 {
    efi_dump_diskinfo();
    0
}

/// `machine memory [+size@addr | -size@addr | =size]...`: edit the memory map, then print
/// it.
#[allow(non_snake_case)] // the C's command names
fn Xmemory(cmd: &mut CmdState) -> i32 {
    if cmd.argc >= 2 {
        // parse the memory specs
        for i in 1..cmd.argc {
            let arg = cmd.arg(i).unwrap_or(b"");
            let Some(&op) = arg.first() else {
                continue;
            };

            let (mut size, end) = strtoll(&arg[1..], 0);
            let mut p = 1 + end;
            // Size the size
            let mult = match arg.get(p) {
                Some(b'G' | b'g') => 1024 * 1024 * 1024,
                Some(b'M' | b'm') => 1024 * 1024,
                Some(b'K' | b'k') => 1024,
                _ => 1,
            };
            if mult != 1 {
                size *= mult;
                p += 1;
            }

            // Handle (possibly non-existent) address part
            let addr = match arg.get(p) {
                Some(b'@') => strtoll(&arg[p + 1..], 0).0,
                // Adjust address if we don't need it
                _ => {
                    if op == b'=' {
                        -1
                    } else {
                        0
                    }
                }
            };

            if addr == 0 || size == 0 {
                printf!("bad language\n");
                return 0;
            }
            match op {
                b'-' => {
                    mem_delete(addr, addr + size);
                }
                b'+' => {
                    mem_add(addr, addr + size);
                }
                b'=' => {
                    mem_limit(size);
                }
                _ => {
                    printf!("bad OP\n");
                    return 0;
                }
            }
            BIOS_MEMMAP_MODIFIED.store(1, Ordering::Relaxed);
        }
    }

    dump_biosmem(None);

    0
}

/// `machine comaddr [port]`: the serial console's I/O port.
#[allow(non_snake_case)] // the C's command names
fn Xcomaddr(cmd: &mut CmdState) -> i32 {
    if cmd.argc >= 2 {
        COM_ADDR.store(
            strtol(cmd.arg(1).unwrap_or(b""), 0).0 as i32,
            Ordering::Relaxed,
        );
    }

    0
}
/* </CODE> */
