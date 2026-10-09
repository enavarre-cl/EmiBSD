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
 * Copyright (c) 2004 Tom Cosgrove
 * Copyright (c) 1997-1999 Michael Shalayeff
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
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `machdep()`: run the probes (console, memory, disks), then under OpenBSD's vmm(4) switch
//! the console to `com0` at 115200 baud; and `check_skip_conf()`.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/machdep.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `BIOS_regs` (BIOS calls) and the `DEBUG` checkpoints (`CKPT`) have no use under UEFI and
//!   are not declared.

use libsa::cons::cnset;
use libsa::printf;

use crate::conf::PROBE_LIST;
use crate::dev_i386::{cnspeed, ttydev};
use crate::efiboot::efi_cons_getshifts;
use crate::libsa_md::{CPUIDECX_HV, cpuid};

/// `VMM_HV_SIGNATURE` (`<machine/vmmvar.h>`): vmm(4)'s CPUID signature.
const VMM_HV_SIGNATURE: &[u8; 12] = b"OpenBSDVMM58";

/// `machdep()`.
pub fn machdep() {
    // The list of probe routines is now in conf.c.
    for pr in &PROBE_LIST {
        printf!("{}:", pr.name);

        for probe in pr.probes {
            probe();
        }

        printf!("\n");
    }

    let mut vmm = false;
    let (_, _, ecx, _) = cpuid(0x1);
    if ecx & CPUIDECX_HV != 0 {
        let (_, ebx, ecx, edx) = cpuid(0x4000_0000);
        if ebx.to_ne_bytes() == VMM_HV_SIGNATURE[0..4]
            && ecx.to_ne_bytes() == VMM_HV_SIGNATURE[4..8]
            && edx.to_ne_bytes() == VMM_HV_SIGNATURE[8..12]
        {
            vmm = true;
        }
    }

    // Set console to com0/115200 by default in vmm
    if vmm {
        let dev = ttydev(b"com0");
        cnspeed(dev, 115_200);
        cnset(dev);
    }
}

/// `check_skip_conf()`: skip boot.conf if the Control "shift" key is down.
pub fn check_skip_conf() -> bool {
    (efi_cons_getshifts(0) & 0x04) != 0
}
/* </CODE> */
