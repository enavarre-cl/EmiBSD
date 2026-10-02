/*	$OpenBSD: syslog.h,v 1.19 2023/04/27 23:16:18 gnezdo Exp $	*/
/*	$NetBSD: syslog.h,v 1.14 1996/04/03 20:46:44 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)syslog.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! Priorities and facilities of the system log: `<sys/syslog.h>`, as the kernel sees it.
//!
//! Upstream: sys/sys/syslog.h @ 3ce1f3f79392
//!
//! Priorities and facilities are encoded into one `int`: the bottom 3 bits are the priority
//! (0 to 7), the rest the facility. `log(9)`, `logpri` and `addlog` live in `kern/subr_prf.rs`,
//! `logwakeup` in `kern/subr_log.rs`.
//!
//! ## Deviations
//! - `LIOCSFD` (an `ioctl` code) arrives with `sys/ioccom.h`; `struct syslog_data`, the
//!   `SYSLOG_NAMES` tables and the userland prototypes are not kernel material.

/// Max line length.
pub const LOG_MAXLINE: usize = 8192;
/// Path of the log socket.
pub const _PATH_LOG: &str = "/dev/log";

// priorities (these are ordered)

/// System is unusable.
pub const LOG_EMERG: i32 = 0;
/// Action must be taken immediately.
pub const LOG_ALERT: i32 = 1;
/// Critical conditions.
pub const LOG_CRIT: i32 = 2;
/// Error conditions.
pub const LOG_ERR: i32 = 3;
/// Warning conditions.
pub const LOG_WARNING: i32 = 4;
/// Normal but significant condition.
pub const LOG_NOTICE: i32 = 5;
/// Informational.
pub const LOG_INFO: i32 = 6;
/// Debug-level messages.
pub const LOG_DEBUG: i32 = 7;

/// Mask to extract the priority part (internal).
pub const LOG_PRIMASK: i32 = 0x07;

// facility codes

/// Kernel messages.
pub const LOG_KERN: i32 = 0 << 3;
/// Random user-level messages.
pub const LOG_USER: i32 = 1 << 3;
/// Mail system.
pub const LOG_MAIL: i32 = 2 << 3;
/// System daemons.
pub const LOG_DAEMON: i32 = 3 << 3;
/// Security/authorization messages.
pub const LOG_AUTH: i32 = 4 << 3;
/// Messages generated internally by syslogd.
pub const LOG_SYSLOG: i32 = 5 << 3;
/// Line printer subsystem.
pub const LOG_LPR: i32 = 6 << 3;
/// Network news subsystem.
pub const LOG_NEWS: i32 = 7 << 3;
/// UUCP subsystem.
pub const LOG_UUCP: i32 = 8 << 3;
/// Clock daemon.
pub const LOG_CRON: i32 = 9 << 3;
/// Security/authorization messages (private).
pub const LOG_AUTHPRIV: i32 = 10 << 3;
/// Ftp daemon.
pub const LOG_FTP: i32 = 11 << 3;
/// Reserved for local use.
pub const LOG_LOCAL0: i32 = 16 << 3;
/// Reserved for local use.
pub const LOG_LOCAL1: i32 = 17 << 3;
/// Reserved for local use.
pub const LOG_LOCAL2: i32 = 18 << 3;
/// Reserved for local use.
pub const LOG_LOCAL3: i32 = 19 << 3;
/// Reserved for local use.
pub const LOG_LOCAL4: i32 = 20 << 3;
/// Reserved for local use.
pub const LOG_LOCAL5: i32 = 21 << 3;
/// Reserved for local use.
pub const LOG_LOCAL6: i32 = 22 << 3;
/// Reserved for local use.
pub const LOG_LOCAL7: i32 = 23 << 3;

/// Current number of facilities.
pub const LOG_NFACILITIES: i32 = 24;
/// Mask to extract the facility part.
pub const LOG_FACMASK: i32 = 0x03f8;

/// Pseudo-priority to indicate use of printf (kernel only).
pub const LOG_PRINTF: i32 = -1;

// Option flags for openlog. LOG_ODELAY no longer does anything; LOG_NDELAY is the inverse of
// what it used to be.

/// Log the pid with each message.
pub const LOG_PID: i32 = 0x01;
/// Log on the console if errors in sending.
pub const LOG_CONS: i32 = 0x02;
/// Delay open until first syslog() (default).
pub const LOG_ODELAY: i32 = 0x04;
/// Don't delay open.
pub const LOG_NDELAY: i32 = 0x08;
/// Don't wait for console forks: DEPRECATED.
pub const LOG_NOWAIT: i32 = 0x10;
/// Log to stderr as well.
pub const LOG_PERROR: i32 = 0x20;

/// `LOG_PRI(p)`: the priority part of `p`.
pub const fn log_pri(p: i32) -> i32 {
    p & LOG_PRIMASK
}

/// `LOG_FAC(p)`: the facility of `p`, as a facility number (not shifted).
pub const fn log_fac(p: i32) -> i32 {
    (p & LOG_FACMASK) >> 3
}

/// `LOG_MASK(pri)`: mask for one priority, for `setlogmask`.
pub const fn log_mask(pri: i32) -> i32 {
    1 << pri
}

/// `LOG_UPTO(pri)`: mask for all priorities through `pri`.
pub const fn log_upto(pri: i32) -> i32 {
    (1 << (pri + 1)) - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding() {
        assert_eq!(log_pri(LOG_DAEMON | LOG_WARNING), LOG_WARNING);
        assert_eq!(log_fac(LOG_DAEMON | LOG_WARNING), 3);
        assert_eq!(log_fac(LOG_LOCAL7), 23);
        assert_eq!(log_mask(LOG_ERR), 0b1000);
        assert_eq!(log_upto(LOG_ERR), 0b1111);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/syslog.h");
        let ours: &[(&str, i64)] = &[
            ("LOG_MAXLINE", LOG_MAXLINE as i64),
            ("LOG_EMERG", LOG_EMERG as i64),
            ("LOG_ALERT", LOG_ALERT as i64),
            ("LOG_CRIT", LOG_CRIT as i64),
            ("LOG_ERR", LOG_ERR as i64),
            ("LOG_WARNING", LOG_WARNING as i64),
            ("LOG_NOTICE", LOG_NOTICE as i64),
            ("LOG_INFO", LOG_INFO as i64),
            ("LOG_DEBUG", LOG_DEBUG as i64),
            ("LOG_PRIMASK", LOG_PRIMASK as i64),
            ("LOG_KERN", LOG_KERN as i64),
            ("LOG_USER", LOG_USER as i64),
            ("LOG_MAIL", LOG_MAIL as i64),
            ("LOG_DAEMON", LOG_DAEMON as i64),
            ("LOG_AUTH", LOG_AUTH as i64),
            ("LOG_SYSLOG", LOG_SYSLOG as i64),
            ("LOG_LPR", LOG_LPR as i64),
            ("LOG_NEWS", LOG_NEWS as i64),
            ("LOG_UUCP", LOG_UUCP as i64),
            ("LOG_CRON", LOG_CRON as i64),
            ("LOG_AUTHPRIV", LOG_AUTHPRIV as i64),
            ("LOG_FTP", LOG_FTP as i64),
            ("LOG_LOCAL0", LOG_LOCAL0 as i64),
            ("LOG_LOCAL7", LOG_LOCAL7 as i64),
            ("LOG_NFACILITIES", LOG_NFACILITIES as i64),
            ("LOG_FACMASK", LOG_FACMASK as i64),
            ("LOG_PRINTF", LOG_PRINTF as i64),
            ("LOG_PID", LOG_PID as i64),
            ("LOG_CONS", LOG_CONS as i64),
            ("LOG_ODELAY", LOG_ODELAY as i64),
            ("LOG_NDELAY", LOG_NDELAY as i64),
            ("LOG_NOWAIT", LOG_NOWAIT as i64),
            ("LOG_PERROR", LOG_PERROR as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
