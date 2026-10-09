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

/*-
 * Copyright (c) 2008-2010 Rui Paulo <rpaulo@FreeBSD.org>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! A simple ELF relocator: apply the image's own `RELATIVE` relocations at the address the
//! firmware loaded it, before anything uses a pointer (`_start` calls it first).
//!
//! Upstream: sys/arch/amd64/stand/efiboot/self_reloc.c @ 3ce1f3f79392
//!
//! Compiled for amd64 (`ELFSIZE` 64, `Elf64_Rela`, `R_X86_64_RELATIVE`). The code may use
//! nothing that needs relocating (no statics, no formatting, no panics): it runs before it is
//! done.

use libsa::hdr::exec_elf::{
    DT_NULL, DT_REL, DT_RELA, DT_RELAENT, DT_RELASZ, DT_RELENT, DT_RELSZ, Elf64Dyn, Elf64Rela,
    R_X86_64_NONE, R_X86_64_RELATIVE,
};

/// `self_reloc(baseaddr, dynamic)`.
///
/// # Safety
///
/// `baseaddr` is the address the image was loaded at (it was linked at 0) and `dynamic` its
/// `_DYNAMIC`, as `_start` passes them; the image is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn self_reloc(baseaddr: u64, dynamic: *const Elf64Dyn) {
    let mut relsz: u64 = 0;
    let mut relent: u64 = 0;
    let mut rel: *const Elf64Rela = core::ptr::null();

    // SAFETY: the caller's contract: `_DYNAMIC` ends with DT_NULL, and the RELA table it
    // names (offsets from 0) lies in the loaded image, as do the words it relocates.
    unsafe {
        // Find the relocation address, its size and the relocation entry.
        let mut dynp = dynamic;
        while (*dynp).d_tag != DT_NULL {
            match (*dynp).d_tag {
                DT_REL | DT_RELA => rel = (*dynp).d_un.wrapping_add(baseaddr) as *const Elf64Rela,
                DT_RELSZ | DT_RELASZ => relsz = (*dynp).d_un,
                DT_RELENT | DT_RELAENT => relent = (*dynp).d_un,
                _ => {}
            }
            dynp = dynp.add(1);
        }

        // Perform the actual relocation. We rely on the object having been linked at 0, so
        // that the difference between the load and link address is the same as the load
        // address.
        while relsz > 0 && relent > 0 {
            // ELF64_R_TYPE, inline: no call may go through the GOT yet
            match ((*rel).r_info & 0xffff_ffff) as u32 {
                R_X86_64_NONE => {
                    // No relocation needs be performed.
                }
                R_X86_64_RELATIVE => {
                    let newaddr = (*rel).r_offset.wrapping_add(baseaddr) as *mut u64;
                    // Addend relative to the base address.
                    *newaddr = baseaddr.wrapping_add((*rel).r_addend as u64);
                }
                _ => {
                    // XXX: do we need other relocations ?
                }
            }
            rel = rel.cast::<u8>().add(relent as usize).cast();
            relsz = relsz.saturating_sub(relent);
        }
    }
}
/* </CODE> */
