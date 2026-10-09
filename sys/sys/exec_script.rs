/* <LICENSES> */
/*
 * Copyright (c) 1994 Christopher G. Demetriou
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
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou.
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
//! `<sys/exec_script.h>`: the magic and the header size of interpreted (`#!`) scripts.
//!
//! Upstream: sys/sys/exec_script.h @ 3ce1f3f79392
//!
//! The kernel half, `exec_script_makecmds`, is in `sys/kern/exec_script.rs`, the module of
//! `exec_script.c`, which the exec switch (`kern_exec.rs`) names.
//!
//! ## Deviations
//! - `EXEC_SCRIPT_MAGIC` is a byte string (`b"#!"`), compared against the header's bytes.

use crate::sys::param::MAXINTERP;

/// `EXEC_SCRIPT_MAGIC`: what a script starts with.
pub const EXEC_SCRIPT_MAGIC: &[u8; EXEC_SCRIPT_MAGICLEN] = b"#!";
/// `EXEC_SCRIPT_MAGICLEN`: the length of the magic.
pub const EXEC_SCRIPT_MAGICLEN: usize = 2;
/// `EXEC_SCRIPT_HDRSZ`: the header the exec switch reads for a script: the magic, a space,
/// the interpreter line (`MAXINTERP`) and its newline.
pub const EXEC_SCRIPT_HDRSZ: usize = EXEC_SCRIPT_MAGICLEN + 1 + MAXINTERP + 1;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let mut defs = crate::reftest::defines("sys/sys/exec_script.h");
        // EXEC_SCRIPT_HDRSZ is written with <sys/param.h>'s MAXINTERP.
        defs.extend(crate::reftest::defines("sys/sys/param.h"));
        let int = |name| crate::reftest::int(&defs, name);
        assert_eq!(
            int("EXEC_SCRIPT_MAGICLEN"),
            Some(EXEC_SCRIPT_MAGICLEN as i64)
        );
        assert_eq!(int("EXEC_SCRIPT_HDRSZ"), Some(EXEC_SCRIPT_HDRSZ as i64));
        assert_eq!(
            defs.get("EXEC_SCRIPT_MAGIC")
                .map(std::string::String::as_str),
            Some("\"#!\"")
        );
    }
}
/* </TESTS> */
