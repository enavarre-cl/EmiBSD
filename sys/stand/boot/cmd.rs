/* <LICENSES> */
/*
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
 */

/*
 * Copyright (c) 1997 Michael Shalayeff
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
//! boot(8)'s command interpreter: the `boot>` prompt, `boot.conf`, and the commands
//! `boot`, `echo`, `env`, `help`, `hexdump`, `ls`, `machine`, `reboot`, `set`, `stty` and
//! `time`.
//!
//! Upstream: sys/stand/boot/cmd.c @ 3ce1f3f79392, sys/stand/boot/cmd.h @ 3ce1f3f79392
//!
//! Compiled as efiboot compiles it: `MACHINE_CMD` (the program's `machine` table),
//! `BOOT_STTY`, `CHECK_SKIP_CONF`, no `INSECURE`, no `DEBUG`.
//!
//! ## Deviations
//! - `struct cmd_state cmd` (boot.c's global) is passed to the commands as `&mut CmdState`:
//!   a command is `fn(&mut CmdState) -> i32`. `cmd_buf` is the state's [`CmdState::buf`],
//!   and `argv[]` keeps the word positions in it ([`CmdState::arg`]); `argv[0]` is the
//!   command's name.
//! - The command tables are slices (no `{NULL, 0}` terminator); `cmd.cmd` is an index-free
//!   `Option<&CmdTable>`.
//! - `getsecs()`, `ttyname()`, `ttydev()`, `cnspeed()` and the `machine` table are the
//!   program's, reached through [`boot_md`](crate::boot::boot_md).
//! - `Xhexdump` reads the memory the operator names, as the C does: an `unsafe` slice.

use libsa::cons::cnischar;
use libsa::ctime::ctime;
use libsa::dev::errno;
use libsa::exit::exit;
use libsa::getchar::getchar;
use libsa::hdr::param::MAXPATHLEN;
use libsa::hdr::stat::{S_IFMT, S_IROTH, S_ISGID, S_ISTXT, S_ISUID, S_IWOTH, S_IXOTH, Stat};
use libsa::hdr::types::{NODEV, Time};
use libsa::hexdump::hexdump;
use libsa::printf;
use libsa::printf::Str;
use libsa::putchar::putchar;
use libsa::readdir::{closedir, opendir, readdir};
use libsa::stand::isdigit;
use libsa::stat::stat;
use libsa::strerror::strerror;
use libsa::strtoll::strtoll;
use libsa::{cread, fstat::fstat, snprintf};

use crate::boot::boot_md;
use crate::vars::{CMD_SET, Xenv, Xset, bootparse};

/// `CMD_BUFF_SIZE`: the command line.
pub const CMD_BUFF_SIZE: usize = 133;
/// `BOOTDEVLEN`: the boot device's name.
pub const BOOTDEVLEN: usize = 1024;

/// `CMDT_CMD`: a command.
pub const CMDT_CMD: u8 = 0;
/// `CMDT_VAR`: a variable of `set`.
pub const CMDT_VAR: u8 = 1;
/// `CMDT_SET`: `set`, whose argument is a variable.
pub const CMDT_SET: u8 = 2;
/// `CMDT_MDC`: `machine`, whose argument is a machine-dependent command.
pub const CMDT_MDC: u8 = 3;

/// `CTRL(c)`.
const fn ctrl(c: u8) -> i32 {
    (c & 0x1f) as i32
}

/// `struct cmd_table`: one command.
pub struct CmdTable {
    /// `cmd_name`.
    pub cmd_name: &'static str,
    /// `cmd_type`: `CMDT_*`.
    pub cmd_type: u8,
    /// `cmd_exec`: run it; non-zero to boot.
    pub cmd_exec: fn(&mut CmdState) -> i32,
}

/// `struct cmd_state`: the boot device, the kernel, the flags, and the command being run.
pub struct CmdState {
    /// `bootdev`: the device (NUL-terminated).
    pub bootdev: [u8; BOOTDEVLEN],
    /// `image`: the kernel's path (NUL-terminated).
    pub image: [u8; MAXPATHLEN - 16],
    /// `boothowto`: the `RB_*` flags.
    pub boothowto: i32,
    /// `conf`: /etc/boot.conf normally.
    pub conf: &'static [u8],
    /// `timeout`: seconds before the prompt boots by itself.
    pub timeout: i32,
    /// `path`: buffer for pathname compose (NUL-terminated).
    pub path: [u8; MAXPATHLEN],
    /// `cmd`: the command being run.
    pub cmd: Option<&'static CmdTable>,
    /// `argc`.
    pub argc: usize,
    /// `argv[1..argc]`, as positions in `buf` (start, end).
    argv: [(usize, usize); 8],
    /// `argv[0]`: the command's name.
    pub argv0: &'static str,
    /// `cmd_buf`: the line being run (NUL-terminated words once parsed).
    pub buf: [u8; CMD_BUFF_SIZE],
}

impl CmdState {
    /// The state boot() starts from: everything empty.
    pub const fn new() -> Self {
        Self {
            bootdev: [0; BOOTDEVLEN],
            image: [0; MAXPATHLEN - 16],
            boothowto: 0,
            conf: b"",
            timeout: 0,
            path: [0; MAXPATHLEN],
            cmd: None,
            argc: 0,
            argv: [(0, 0); 8],
            argv0: "",
            buf: [0; CMD_BUFF_SIZE],
        }
    }

    /// `cmd.argv[i]`: argument `i` (1 and up), or `None` past `argc` (the C's NULL).
    pub fn arg(&self, i: usize) -> Option<&[u8]> {
        if i == 0 || i >= self.argc {
            return None;
        }
        let (s, e) = self.argv[i];
        Some(&self.buf[s..e])
    }
}

impl Default for CmdState {
    fn default() -> Self {
        Self::new()
    }
}

/// `cmd_table[]`.
pub static CMD_TABLE: [CmdTable; 12] = [
    CmdTable {
        cmd_name: "#",
        cmd_type: CMDT_CMD,
        cmd_exec: Xnop,
    }, // XXX must be first
    CmdTable {
        cmd_name: "boot",
        cmd_type: CMDT_CMD,
        cmd_exec: Xboot,
    },
    CmdTable {
        cmd_name: "echo",
        cmd_type: CMDT_CMD,
        cmd_exec: Xecho,
    },
    CmdTable {
        cmd_name: "env",
        cmd_type: CMDT_CMD,
        cmd_exec: Xenv,
    },
    CmdTable {
        cmd_name: "help",
        cmd_type: CMDT_CMD,
        cmd_exec: Xhelp,
    },
    CmdTable {
        cmd_name: "hexdump",
        cmd_type: CMDT_CMD,
        cmd_exec: Xhexdump,
    },
    CmdTable {
        cmd_name: "ls",
        cmd_type: CMDT_CMD,
        cmd_exec: Xls,
    },
    CmdTable {
        cmd_name: "machine",
        cmd_type: CMDT_MDC,
        cmd_exec: Xmachine,
    },
    CmdTable {
        cmd_name: "reboot",
        cmd_type: CMDT_CMD,
        cmd_exec: Xreboot,
    },
    CmdTable {
        cmd_name: "set",
        cmd_type: CMDT_SET,
        cmd_exec: Xset,
    },
    CmdTable {
        cmd_name: "stty",
        cmd_type: CMDT_CMD,
        cmd_exec: Xstty,
    },
    CmdTable {
        cmd_name: "time",
        cmd_type: CMDT_CMD,
        cmd_exec: Xtime,
    },
];

/// `doboot`: the first unknown word boots (the `#` entry's `Xnop`).
static DOBOOT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// The bytes of a NUL-terminated buffer, up to the NUL.
pub fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// `strlcpy(dst, src, sizeof(dst))`.
pub fn strlcpy(dst: &mut [u8], src: &[u8]) {
    let src = cstr(src);
    if dst.is_empty() {
        return;
    }
    let n = src.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

/// `getcmd()`: read a line at the prompt and run it.
pub fn getcmd(cmd: &mut CmdState) -> i32 {
    cmd.cmd = None;

    let timeout = cmd.timeout;
    let mut buf = [0u8; CMD_BUFF_SIZE];
    if readline(cmd, &mut buf, timeout) == 0 {
        cmd.cmd = Some(&CMD_TABLE[0]);
    }
    cmd.buf = buf;

    docmd(cmd)
}

/// `read_conf()`: run the commands of `boot.conf`; 1 if one of them boots, 0 when it ends,
/// -1 when there is no (usable) file.
pub fn read_conf(cmd: &mut CmdState) -> i32 {
    if let Some(check) = boot_md().check_skip_conf
        && check()
    {
        printf!("boot.conf processing skipped at operator request\n");
        cmd.timeout = 0;
        return -1; // Pretend file wasn't found
    }

    let conf = cmd.conf;
    qualify(cmd, conf);
    let fd = match cread::open(cstr(&cmd.path), 0) {
        Ok(fd) => fd,
        Err(e) => {
            if e != libsa::saerrno::Errno::ENOENT && e != libsa::saerrno::Errno::ENXIO {
                printf!("open({}): {}\n", Str(&cmd.path), strerror(errno()));
                return 0;
            }
            return -1;
        }
    };

    let mut sb = Stat::default();
    let _ = fstat(fd, &mut sb);
    if sb.st_uid != 0 || (sb.st_mode & 2) != 0 {
        printf!("non-secure {}, will not proceed\n", Str(&cmd.path));
        let _ = cread::close(fd);
        return -1;
    }

    let mut rc;
    loop {
        let mut p = 0;
        cmd.cmd = None;
        cmd.buf = [0; CMD_BUFF_SIZE];
        loop {
            let mut c = [0u8; 1];
            rc = match cread::read(fd, &mut c) {
                Ok(n) => n as i32,
                Err(_) => -1,
            };
            if rc > 0 {
                cmd.buf[p] = c[0];
                p += 1;
            }
            if !(rc > 0 && c[0] != b'\n' && p < CMD_BUFF_SIZE) {
                break;
            }
        }

        if rc < 0 {
            // Error from read()
            printf!("{}: {}\n", Str(&cmd.path), strerror(errno()));
            break;
        }

        if rc == 0 {
            // eof from read()
            if p != 0 {
                // Line w/o trailing \n
                if p < CMD_BUFF_SIZE {
                    cmd.buf[p] = 0;
                }
                rc = docmd(cmd);
                break;
            }
        } else {
            // rc > 0, read a char
            p -= 1; // Get back to last character

            if cmd.buf[p] != b'\n' {
                // Line was too long
                printf!("{}: line too long\n", Str(&cmd.path));

                // Don't want to run the truncated command
                rc = -1;
            }
            cmd.buf[p] = 0;
        }
        if rc <= 0 {
            break;
        }
        rc = docmd(cmd);
        if rc != 0 {
            break;
        }
    }

    let _ = cread::close(fd);
    rc
}

/// `docmd()`: parse `cmd.buf` (unless `cmd.cmd` is already set) and run the command; what
/// it returns (non-zero to boot).
pub fn docmd(cmd: &mut CmdState) -> i32 {
    let mut p: Option<usize> = None;
    let mut ct: &'static CmdTable = &CMD_TABLE[0];

    cmd.argc = 1;
    if cmd.cmd.is_none() {
        // command
        let mut q = 0;
        while cmd.buf[q] == b' ' || cmd.buf[q] == b'\t' {
            q += 1;
        }
        if cmd.buf[q] == b'#' || cmd.buf[q] == 0 {
            // comment or empty string
            return 0;
        }
        let mut cs: Option<&'static [CmdTable]> = None;
        cmd.argv[cmd.argc] = (q, q); // in case it's shortcut boot
        let (found, next) = whatcmd(&CMD_TABLE, &mut cmd.buf, q);
        cmd.argv[cmd.argc].1 = word_end(&cmd.buf, q);
        p = next;
        match found {
            None => {
                cmd.argc += 1;
                ct = &CMD_TABLE[0];
            }
            Some(t) => {
                ct = t;
                if t.cmd_type == CMDT_SET && p.is_some() {
                    cs = Some(&CMD_SET);
                } else if t.cmd_type == CMDT_MDC && p.is_some() {
                    cs = boot_md().cmd_machine;
                }
            }
        }

        if let (Some(table), Some(pp)) = (cs, p) {
            let (sub, next) = whatcmd(table, &mut cmd.buf, pp);
            p = next;
            let Some(sub) = sub else {
                printf!("{}: syntax error\n", ct.cmd_name);
                return 0;
            };
            ct = sub;
        }
        cmd.cmd = Some(ct);
    } else if let Some(c) = cmd.cmd {
        ct = c;
    }

    cmd.argv0 = ct.cmd_name;
    while let Some(pp) = p {
        if cmd.argc + 1 >= cmd.argv.len() {
            break;
        }
        let next = nextword(&mut cmd.buf, pp);
        cmd.argv[cmd.argc] = (pp, word_end(&cmd.buf, pp));
        cmd.argc += 1;
        p = next;
    }

    let exec = cmd.cmd.map_or(ct.cmd_exec, |c| c.cmd_exec);
    exec(cmd)
}

/// The end of the NUL-terminated word at `p`.
fn word_end(buf: &[u8], p: usize) -> usize {
    p + cstr(&buf[p..]).len()
}

/// `whatcmd(&ct, p)`: the command of `table` the word at `p` abbreviates (the first one),
/// and the next word.
fn whatcmd(
    table: &'static [CmdTable],
    buf: &mut [u8; CMD_BUFF_SIZE],
    p: usize,
) -> (Option<&'static CmdTable>, Option<usize>) {
    let q = nextword(buf, p);
    let word = cstr(&buf[p..]);
    let l = word.len();
    let found = table.iter().find(|t| {
        // strncmp(p, cmd_name, l): the first l bytes, a shorter name ending at its NUL
        let name = t.cmd_name.as_bytes();
        (0..l).all(|i| name.get(i).copied().unwrap_or(0) == word[i])
    });
    (found, q)
}

/// `readline(buf, n, to)`: a line from the console, with erase and kill, into `buf`; after
/// `to` seconds (if positive) with nothing typed, `boot`. Its length.
fn readline(cmd: &mut CmdState, buf: &mut [u8; CMD_BUFF_SIZE], to: i32) -> usize {
    let n = buf.len();
    let mut p = 0;

    // Only do timeout if greater than 0
    if to > 0 {
        let tt: Time = (boot_md().getsecs)() + Time::from(to);
        while cnischar() == 0 && (boot_md().getsecs)() < tt {
            core::hint::spin_loop();
        }

        if cnischar() == 0 {
            strlcpy(&mut buf[..5], b"boot");
            putchar(i32::from(b'\n'));
            return 4;
        }
    } else {
        while cnischar() == 0 {
            core::hint::spin_loop();
        }
    }

    // User has typed something.  Turn off timeouts.
    cmd.timeout = 0;

    loop {
        let ch = getchar();
        match ch {
            c if c == ctrl(b'u') => {
                while p > 0 {
                    putchar(0o177);
                    p -= 1;
                }
            }
            0x0a | 0x0d => {
                buf[p] = 0;
                break;
            }
            0x08 | 0o177 => {
                if p > 0 {
                    putchar(0o177);
                    p -= 1;
                }
            }
            c => {
                if (i32::from(b' ')..0o177).contains(&c) {
                    if p < n - 1 {
                        buf[p] = c as u8;
                        p += 1;
                    } else {
                        putchar(0o007);
                        putchar(0o177);
                    }
                }
            }
        }
    }

    p
}

/// `nextword(p)`: put a NUL after the word at `p`; the position of the next word, `None` if
/// there is none.
pub fn nextword(buf: &mut [u8], mut p: usize) -> Option<usize> {
    // skip blanks
    while p < buf.len() && buf[p] != 0 && buf[p] != b'\t' && buf[p] != b' ' {
        p += 1;
    }
    if p < buf.len() && buf[p] != 0 {
        buf[p] = 0;
        p += 1;
        while p < buf.len() && (buf[p] == b'\t' || buf[p] == b' ') {
            p += 1;
        }
    }
    if p >= buf.len() || buf[p] == 0 {
        None
    } else {
        Some(p)
    }
}

/// `print_help(ct)`.
fn print_help(table: &[CmdTable]) {
    for ct in table {
        printf!(" {}", ct.cmd_name);
    }
    putchar(i32::from(b'\n'));
}

/// `help`.
#[allow(non_snake_case)] // the C's command names
fn Xhelp(cmd: &mut CmdState) -> i32 {
    printf!("commands:");
    print_help(&CMD_TABLE);
    Xmachine(cmd)
}

/// `hexdump addr size`.
#[allow(non_snake_case)] // the C's command names
fn Xhexdump(cmd: &mut CmdState) -> i32 {
    if cmd.argc != 3 {
        printf!("hexdump addr size\n");
        return 0;
    }

    let mut val = [0i64; 2];
    for (i, v) in val.iter_mut().enumerate() {
        let a = cmd.arg(i + 1).unwrap_or(b"");
        let (n, end) = strtoll(a, 0);
        if a.is_empty() || end != a.len() {
            printf!(
                "bad '{}' in \"{}\"\n",
                char::from(a.get(end).copied().unwrap_or(0)),
                Str(a)
            );
            return 0;
        }
        *v = n;
    }
    // SAFETY: none in general: as the C does, this shows whatever memory the operator names,
    // and a bad address faults. The boot program runs identity-mapped with no protection.
    let mem = unsafe { core::slice::from_raw_parts(val[0] as usize as *const u8, val[1] as usize) };
    hexdump(mem);
    0
}

/// `machine`: the machine-dependent commands.
#[allow(non_snake_case)] // the C's command names
fn Xmachine(_cmd: &mut CmdState) -> i32 {
    printf!("machine:");
    print_help(boot_md().cmd_machine.unwrap_or(&[]));
    0
}

/// `echo args`.
#[allow(non_snake_case)] // the C's command names
fn Xecho(cmd: &mut CmdState) -> i32 {
    for i in 1..cmd.argc {
        printf!("{} ", Str(cmd.arg(i).unwrap_or(b"")));
    }
    putchar(i32::from(b'\n'));
    0
}

/// `stty [dev [speed]]`.
#[allow(non_snake_case)] // the C's command names
fn Xstty(cmd: &mut CmdState) -> i32 {
    let Some(stty) = boot_md().stty.as_ref() else {
        return 0;
    };
    if cmd.argc == 1 {
        printf!(
            "{} speed is {}\n",
            Str(&(stty.ttyname)()),
            (stty.cnspeed)(0, -1)
        );
        return 0;
    }
    let name = cmd.arg(1).unwrap_or(b"");
    let dev = (stty.ttydev)(name);
    if dev == NODEV {
        printf!("{} not a console device\n", Str(name));
        return 0;
    }

    if cmd.argc == 2 {
        printf!("{} speed is {}\n", Str(name), (stty.cnspeed)(dev, -1));
    } else {
        let mut sp = 0;
        for &c in cmd
            .arg(2)
            .unwrap_or(b"")
            .iter()
            .take_while(|&&c| isdigit(c))
        {
            sp = sp * 10 + i32::from(c - b'0');
        }
        (stty.cnspeed)(dev, sp);
    }
    0
}

/// `time`.
#[allow(non_snake_case)] // the C's command names
fn Xtime(cmd: &mut CmdState) -> i32 {
    let tt = (boot_md().getsecs)();

    if cmd.argc == 1 {
        printf!("{}", ctime(tt));
    }

    0
}

/// `ls [dir]`.
#[allow(non_snake_case)] // the C's command names
fn Xls(cmd: &mut CmdState) -> i32 {
    let mut sb = Stat::default();
    let target: [u8; MAXPATHLEN] = {
        let mut t = [0u8; MAXPATHLEN];
        strlcpy(&mut t, cmd.arg(1).unwrap_or(b"/."));
        t
    };
    qualify(cmd, cstr(&target));
    if stat(cstr(&cmd.path), &mut sb).is_err() {
        printf!("stat({}): {}\n", Str(&cmd.path), strerror(errno()));
        return 0;
    }

    if (sb.st_mode & S_IFMT) != libsa::hdr::stat::S_IFDIR {
        let path = cmd.path;
        ls(cstr(&path), &sb);
    } else {
        let fd = match opendir(cstr(&cmd.path)) {
            Ok(fd) => fd,
            Err(_) => {
                printf!("opendir({}): {}\n", Str(&cmd.path), strerror(errno()));
                return 0;
            }
        };

        // no strlen in lib !!!
        let mut p = cstr(&cmd.path).len();
        cmd.path[p] = b'/';
        p += 1;
        cmd.path[p] = 0;

        while readdir(fd, &mut cmd.path[p..]).is_ok() {
            if stat(cstr(&cmd.path), &mut sb).is_err() {
                printf!("stat({}): {}\n", Str(&cmd.path), strerror(errno()));
            } else {
                let path = cmd.path;
                ls(cstr(&path[p..]), &sb);
            }
        }
        closedir(fd);
    }
    0
}

/// `lsrwx(mode, s)`: three permission letters.
fn lsrwx(mode: u32, s: &[u8; 2]) {
    putchar(i32::from(if mode & S_IROTH != 0 { b'r' } else { b'-' }));
    putchar(i32::from(if mode & S_IWOTH != 0 { b'w' } else { b'-' }));
    putchar(i32::from(if mode & S_IXOTH != 0 { s[0] } else { s[1] }));
}

/// `ls(name, sb)`: one line of `ls`.
fn ls(name: &[u8], sb: &Stat) {
    let m = sb.st_mode;
    putchar(i32::from(
        b"-fc-d-b---l-s-w-"[((m & S_IFMT) >> 12) as usize],
    ));
    lsrwx(m >> 6, if m & S_ISUID != 0 { b"sS" } else { b"x-" });
    lsrwx(m >> 3, if m & S_ISGID != 0 { b"sS" } else { b"x-" });
    lsrwx(m, if m & S_ISTXT != 0 { b"tT" } else { b"x-" });

    printf!(
        " {},{}\t{}\t{}\n",
        sb.st_uid,
        sb.st_gid,
        sb.st_size as u64,
        Str(name)
    );
}

/// `#` (and the shortcut boot): boot the first time, nothing later.
#[allow(non_snake_case)] // the C's command names
fn Xnop(cmd: &mut CmdState) -> i32 {
    if DOBOOT.swap(false, core::sync::atomic::Ordering::Relaxed) {
        return Xboot(cmd);
    }
    0
}

/// `boot [image] [-acds]`.
#[allow(non_snake_case)] // the C's command names
fn Xboot(cmd: &mut CmdState) -> i32 {
    if cmd.argc > 1 && cmd.arg(1).is_some_and(|a| a.first() != Some(&b'-')) {
        let mut target = [0u8; MAXPATHLEN];
        strlcpy(&mut target, cmd.arg(1).unwrap_or(b""));
        qualify(cmd, cstr(&target));
        if bootparse(cmd, 2) != 0 {
            return 0;
        }
    } else {
        if bootparse(cmd, 1) != 0 {
            return 0;
        }
        let (bootdev, image) = (cmd.bootdev, cmd.image);
        snprintf!(&mut cmd.path, "{}:{}", Str(&bootdev), Str(&image));
    }

    1
}

/// `qualify(name)`: `name` with the boot device in front unless it names one; into
/// `cmd.path`.
pub fn qualify(cmd: &mut CmdState, name: &[u8]) {
    let name = cstr(name);
    if name.contains(&b':') {
        strlcpy(&mut cmd.path, name);
    } else {
        let bootdev = cmd.bootdev;
        snprintf!(&mut cmd.path, "{}:{}", Str(&bootdev), Str(name));
    }
}

/// `reboot`.
#[allow(non_snake_case)] // the C's command names
fn Xreboot(_cmd: &mut CmdState) -> i32 {
    printf!("Rebooting...\n");
    exit()
}

/// `upgrade()`: is there an executable `/bsd.upgrade`?
pub fn upgrade(cmd: &mut CmdState) -> bool {
    let mut sb = Stat::default();

    qualify(cmd, b"/bsd.upgrade");
    if stat(cstr(&cmd.path), &mut sb).is_err() {
        return false;
    }
    if (sb.st_mode & libsa::hdr::stat::S_IXUSR) == 0 {
        printf!("/bsd.upgrade is not u+x\n");
        return false;
    }
    true
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // boot(8)'s command line: words, abbreviations, `set` variables, flags and the shortcut
    // boot of a bare kernel name.

    use super::*;
    use crate::boot::{BootMd, boot_md_register};
    use crate::vars::bootparse;

    static MD: BootMd = BootMd {
        machine: "test",
        version: "0",
        machdep: || {},
        devboot: |_, _| {},
        run_loadfile: |_, _, _| {},
        getsecs: || 0,
        cmd_machine: None,
        check_skip_conf: None,
        mdrandom: None,
        fwrandom: None,
        bootdev_has_hibernate: None,
        stty: None,
    };

    /// A state with boot device `hd0a` and image `/bsd`, and `line` in the buffer.
    fn state(line: &str) -> CmdState {
        boot_md_register(&MD);
        let mut cmd = CmdState::new();
        strlcpy(&mut cmd.bootdev, b"hd0a");
        strlcpy(&mut cmd.image, b"/bsd");
        cmd.buf[..line.len()].copy_from_slice(line.as_bytes());
        cmd
    }

    #[test]
    fn nextword_splits_on_blanks() {
        let mut buf = *b"ls \t /etc\0\0";
        assert_eq!(nextword(&mut buf, 0), Some(5));
        assert_eq!(&buf[..3], b"ls\0");
        assert_eq!(nextword(&mut buf, 5), None);
    }

    #[test]
    fn set_variables_and_flags() {
        let mut cmd = state("set timeout 7");
        assert_eq!(docmd(&mut cmd), 0);
        assert_eq!(cmd.timeout, 7);

        let mut cmd = state("se im /bsd.rd");
        assert_eq!(docmd(&mut cmd), 0);
        assert_eq!(cstr(&cmd.image), b"/bsd.rd");

        let mut cmd = state("set howto -sd");
        assert_eq!(docmd(&mut cmd), 0);
        assert_eq!(
            cmd.boothowto,
            libsa::hdr::reboot::RB_SINGLE | libsa::hdr::reboot::RB_KDB
        );

        let mut cmd = state("set nothing");
        assert_eq!(docmd(&mut cmd), 0); // "set: syntax error"

        let mut cmd = state("# a comment");
        assert_eq!(docmd(&mut cmd), 0);
    }

    #[test]
    fn boot_and_its_shortcut() {
        // `b` abbreviates `boot`, which composes the path from the device and the image
        let mut cmd = state("b -c");
        assert_eq!(docmd(&mut cmd), 1);
        assert_eq!(cstr(&cmd.path), b"hd0a:/bsd");
        assert_eq!(cmd.boothowto, libsa::hdr::reboot::RB_CONFIG);

        // an unknown first word boots it, once
        let mut cmd = state("hd1a:/bsd.sp -s");
        assert_eq!(docmd(&mut cmd), 1);
        assert_eq!(cstr(&cmd.path), b"hd1a:/bsd.sp");
        assert_eq!(cmd.boothowto, libsa::hdr::reboot::RB_SINGLE);
        let mut cmd = state("/bsd");
        assert_eq!(docmd(&mut cmd), 0);

        let mut cmd = state("boot /bsd -x");
        assert_eq!(docmd(&mut cmd), 0); // "howto: bad option: x"
        assert_eq!(bootparse(&mut state("boot -a"), 1), 0);
    }
}
/* </TESTS> */
