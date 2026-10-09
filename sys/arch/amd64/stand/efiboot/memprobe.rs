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
 * Copyright (c) 1997-1999 Tobias Weingartner
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
//! The memory map the kernel gets: printing it (`machine memory`), editing it (`+`, `-`,
//! `=`) and passing it (`BOOTARG_MEMMAP`).
//!
//! Upstream: sys/arch/amd64/stand/efiboot/memprobe.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `cnvmem` and `extmem` are atomics. The map is `efiboot.rs`'s `bios_memmap[128]`, which
//!   this file declares as `bios_memmap[64]` in C (the copies in `mem_limit`, `mem_delete`
//!   and `mem_add` move 64 entries' worth; `mem_add`'s "insert before" one more than the
//!   array): here the entries move within the whole array and stay inside it.

use core::sync::atomic::{AtomicU32, Ordering};

use boot::bootarg::addbootarg;
use libsa::hdr::param::PAGE_SIZE;
use libsa::printf;

use crate::biosvar::{BIOS_MAP_END, BIOS_MAP_FREE, BOOTARG_MEMMAP, BiosMemmap};
use crate::efiboot::BIOS_MEMMAP;

/// `cnvmem`: conventional memory, KB (XXX - compatibility).
pub static CNVMEM: AtomicU32 = AtomicU32::new(0);
/// `extmem`: extended memory, KB.
pub static EXTMEM: AtomicU32 = AtomicU32::new(0);

/// The map.
fn map() -> &'static mut [BiosMemmap; 128] {
    // SAFETY: efiboot is single-threaded and none of this file's callers holds another
    // reference to the map while calling it.
    unsafe { BIOS_MEMMAP.get_mut() }
}

/// The index of the `BIOS_MAP_END` entry.
fn end(map: &[BiosMemmap]) -> usize {
    map.iter()
        .position(|p| p.r#type == BIOS_MAP_END)
        .unwrap_or(map.len() - 1)
}

/// `dump_biosmem(tm)`: the regions and the totals.
pub fn dump_biosmem(tm: Option<&[BiosMemmap]>) {
    let map = map();
    let tm = tm.unwrap_or(&map[..]);
    let mut total: u32 = 0;

    for (i, p) in tm
        .iter()
        .take_while(|p| p.r#type != BIOS_MAP_END)
        .enumerate()
    {
        let (ty, addr, size) = (p.r#type, p.addr, p.size);
        printf!(
            "Region {}: type {} at {:#x} for {}KB\n",
            i,
            ty,
            addr,
            (size / 1024) as u32
        );

        if ty == BIOS_MAP_FREE {
            total = total.wrapping_add((size / 1024) as u32);
        }
    }

    printf!(
        "Low ram: {}KB  High ram: {}KB\n",
        CNVMEM.load(Ordering::Relaxed) as i32,
        EXTMEM.load(Ordering::Relaxed) as i32
    );
    printf!("Total free memory: {}KB\n", total);
}

/// `mem_limit(ml)`: drop the free memory above `ml`.
pub fn mem_limit(ml: i64) -> i32 {
    let map = map();
    let mut i = 0;
    while map[i].r#type != BIOS_MAP_END {
        let p = map[i];
        let (sp, ep) = (p.addr as i64, (p.addr + p.size) as i64);

        if p.r#type != BIOS_MAP_FREE {
            i += 1;
            continue;
        }

        if sp >= ml && ep >= ml {
            // Wholly above limit, nuke it
            map.copy_within(i + 1.., i);
            continue;
        } else if sp < ml && ep >= ml {
            map[i].size -= (ep - ml) as u64;
        }
        i += 1;
    }
    0
}

/// `mem_delete(sa, ea)`: remove `[sa, ea)` from the free memory.
pub fn mem_delete(sa: i64, ea: i64) -> i32 {
    let map = map();
    let mut i = 0;
    while map[i].r#type != BIOS_MAP_END {
        if map[i].r#type == BIOS_MAP_FREE {
            let p = map[i];
            let (sp, ep) = (p.addr as i64, (p.addr + p.size) as i64);

            // can we eat it as a whole?
            if (sa - sp) <= PAGE_SIZE as i64 && (ep - ea) <= PAGE_SIZE as i64 {
                map.copy_within(i + 1.., i);
                break;
            // eat head or legs
            } else if sa <= sp && sp < ea {
                map[i].addr = ea as u64;
                map[i].size = (ep - ea) as u64;
                break;
            } else if sa < ep && ep <= ea {
                map[i].size = (sa - sp) as u64;
                break;
            } else if sp < sa && ea < ep {
                // bite in half
                let n = map.len();
                map.copy_within(i..n - 1, i + 1);
                map[i + 1].addr = ea as u64;
                map[i + 1].size = (ep - ea) as u64;
                map[i].size = (sa - sp) as u64;
                break;
            }
        }
        i += 1;
    }
    0
}

/// `mem_add(sa, ea)`: add `[sa, ea)` to the free memory.
pub fn mem_add(sa: i64, ea: i64) -> i32 {
    let map = map();
    let mut i = 0;
    while map[i].r#type != BIOS_MAP_END {
        if map[i].r#type == BIOS_MAP_FREE {
            let p = map[i];
            let (sp, ep) = (p.addr as i64, (p.addr + p.size) as i64);

            // is it already there?
            if sp <= sa && ea <= ep {
                break;
            // join head or legs
            } else if sa < sp && sp <= ea {
                map[i].addr = sa as u64;
                map[i].size = (ep - sa) as u64;
                break;
            } else if sa <= ep && ep < ea {
                map[i].size = (ea - sp) as u64;
                break;
            } else if ea < sp {
                // insert before
                let n = map.len();
                map.copy_within(i..n - 1, i + 1);
                map[i].addr = sa as u64;
                map[i].size = (ea - sa) as u64;
                break;
            }
        }
        i += 1;
    }

    // meaning add new item at the end of the list
    if map[i].r#type == BIOS_MAP_END && i + 1 < map.len() {
        map[i + 1] = map[i];
        map[i].r#type = BIOS_MAP_FREE;
        map[i].addr = sa as u64;
        map[i].size = (ea - sa) as u64;
    }

    0
}

/// `mem_pass()`: `BOOTARG_MEMMAP`: the map, its end entry included.
pub fn mem_pass() {
    let map = map();
    let n = end(&map[..]) + 1;
    let mut bytes = alloc::vec::Vec::with_capacity(n * core::mem::size_of::<BiosMemmap>());
    for p in &map[..n] {
        bytes.extend_from_slice(p.as_bytes());
    }
    addbootarg(BOOTARG_MEMMAP, &bytes);
}
/* </CODE> */
