/*	$OpenBSD: ecb3_enc.c,v 1.3 2013/11/18 18:49:53 brad Exp $	*/
/* <LICENSES> */
/* lib/des/ecb3_enc.c */

/* Copyright (C) 1995 Eric Young (eay@mincom.oz.au)
 * All rights reserved.
 *
 * This file is part of an SSL implementation written
 * by Eric Young (eay@mincom.oz.au).
 * The implementation was written so as to conform with Netscapes SSL
 * specification.  This library and applications are
 * FREE FOR COMMERCIAL AND NON-COMMERCIAL USE
 * as long as the following conditions are aheared to.
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.  If this code is used in a product,
 * Eric Young should be given attribution as the author of the parts used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    This product includes software developed by Eric Young (eay@mincom.oz.au)
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
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
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.]
 */
/* </LICENSES> */

//! Triple DES (EDE, three keys) of one 8-byte block: `des3_encrypt` and `des3_decrypt` of
//! `xform.c`.
//!
//! Upstream: sys/crypto/ecb3_enc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The blocks are `&DesCblock` and `&mut DesCblock` (the in-place call of `xform.c` copies
//!   the block first), the schedules `&DesKeySchedule`, and `encrypt` a `bool`.

use super::des_locl::{DesCblock, DesKeySchedule, c2l, fp, ip, l2c};
use super::ecb_enc::des_encrypt2;

/// `des_ecb3_encrypt`: encrypts (or decrypts) one block under the three key schedules: the
/// DES transform with `ks1`, the opposite with `ks2`, the first again with `ks3`.
pub fn des_ecb3_encrypt(
    input: &DesCblock,
    output: &mut DesCblock,
    ks1: &DesKeySchedule,
    ks2: &DesKeySchedule,
    ks3: &DesKeySchedule,
    encrypt: bool,
) {
    let mut l0 = c2l(&input[0..]);
    let mut l1 = c2l(&input[4..]);
    ip(&mut l0, &mut l1);
    let mut ll = [l0, l1];
    des_encrypt2(&mut ll, ks1, encrypt);
    des_encrypt2(&mut ll, ks2, !encrypt);
    des_encrypt2(&mut ll, ks3, encrypt);
    l0 = ll[0];
    l1 = ll[1];
    fp(&mut l1, &mut l0);
    l2c(l0, &mut output[0..]);
    l2c(l1, &mut output[4..]);
}

#[cfg(test)]
mod tests;
