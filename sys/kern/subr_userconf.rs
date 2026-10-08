/*	$OpenBSD: subr_userconf.c,v 1.48 2022/08/14 01:58:28 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996-2001 Mats O Jansson <moj@stacken.kth.se>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! UKC, the User Kernel Config: `boot -c` edits the kernel's device configuration before
//! autoconfiguration runs (`boot_config(8)`).
//!
//! Upstream: sys/kern/subr_userconf.c @ 3ce1f3f79392
//!
//! With `RB_CONFIG` in `boothowto`, the machine's `cpu_startup` calls [`user_config`], which
//! prints `User Kernel Config` and reads commands at the `UKC>` prompt until `quit` (or
//! `exit`). The commands walk the tables `config(8)` writes into `ioconf.c`: `list` prints
//! every `cfdata[]` entry, its free slots and the pseudo-devices; `find`, `enable`, `disable`
//! and `change` take a device number, a name (`vio`), a unit (`com0`) or a starred name
//! (`vio*`), and `enable`/`disable` also a locator and value (`disable irq 5`); `change` edits
//! an entry's locators and flags (or a pseudo-device's count); `add` clones an entry into a
//! free slot before a given one; `show` lists the locator names or the entries with one;
//! `base`, `lines` and `verbose` set the number base, the page length and
//! `autoconf_verbose`; `ddb` enters the debugger. A disabled entry is in state
//! `FSTATE_DNOTFOUND`/`FSTATE_DSTAR`, which `config_search` skips, so its device does not
//! attach; a disabled pseudo-device has a negative count, which `main` does not attach.
//! Every change is also recorded in `userconf_history`, as the C does.
//!
//! The editor is a [`Userconf`] over the tables (`machine::autoconf::IoconfTables`) and a
//! console ([`UkcCons`]); the kernel's is the real console, the host tests' a script.
//!
//! ## Deviations
//! - The C's globals (`userconf_base`, `userconf_maxdev`, the history, ...) are the fields of
//!   [`Userconf`], made by each `user_config` call; the kernel calls it once per boot, so
//!   nothing the C kept across calls is lost.
//! - `ioconf.c`'s arrays are not mutable statics: the machine hands them over once
//!   (`machine::autoconf::ioconf_mut`), before anything else reads them. `cf_loc` and
//!   `cf_parents` point at constant arrays, so `change` always edits a `malloc`ed copy of the
//!   locators (the C does only when another entry shares them, and frees it when nothing
//!   changed, as here), and `add` renumbers the parents (the C's `pv[]`) in copies, one per
//!   shared vector. If those copies cannot be allocated, `add` prints `out of memory.` and
//!   changes nothing (the C's `pv[]` edit cannot fail).
//! - A negative device number (`find -1`) is answered `Unknown devno`, where the C indexes
//!   `cfdata[]` before its start; so is one too large for `int`.
//! - The locator names come from per-attribute runs in `locnamp[]` (the compression
//!   `mkioconf.c`'s XXX asks for); an entry prints as many locators as both its run and its
//!   `cf_loc` have.
//! - `userconf_number` keeps the C's digit check (`cc > base`), so a digit equal to the base
//!   (`08`, `0x` is fine) is taken; and its overflow test, which only a negative number gets.

use core::fmt;
use core::sync::atomic::Ordering;

use alloc::vec::Vec;
use libkern::{GetsnCons, getsn, strncasecmp};

use crate::dev::cons::{cngetc, cnpollc};
use crate::kern::subr_autoconf::AUTOCONF_VERBOSE;
use crate::kern::subr_prf::{Str, printf};
use crate::machine::autoconf::{IoconfTables, ioconf_mut};
use crate::sys::device::{
    FSTATE_DNOTFOUND, FSTATE_DSTAR, FSTATE_FOUND, FSTATE_NOTFOUND, FSTATE_STAR,
};

/// `UC_CHANGE`.
const UC_CHANGE: u8 = b'c';
/// `UC_DISABLE`.
const UC_DISABLE: u8 = b'd';
/// `UC_ENABLE`.
const UC_ENABLE: u8 = b'e';
/// `UC_FIND`.
const UC_FIND: u8 = b'f';
/// `UC_SHOW`.
const UC_SHOW: u8 = b's';

/// `userconf_cmds[]`: each command's name and the letter `userconf_parse` dispatches on.
const USERCONF_CMDS: &[(&[u8], u8)] = &[
    (b"add", b'a'),
    (b"base", b'b'),
    (b"change", b'c'),
    (b"ddb", b'D'),
    (b"disable", b'd'),
    (b"enable", b'e'),
    (b"exit", b'q'),
    (b"find", b'f'),
    (b"help", b'h'),
    (b"list", b'l'),
    (b"lines", b'L'),
    (b"quit", b'q'),
    (b"show", b's'),
    (b"verbose", b'v'),
    (b"?", b'h'),
];

/// `sizeof(userconf_history)`.
const USERCONF_HISTSZ: usize = 1024;

/// `sizeof(userconf_cmdbuf)`, `sizeof(userconf_argbuf)`.
const USERCONF_BUFSZ: usize = 40;

/// `INT_MAX`, the limit most numbers are read with.
const INT_MAX: i64 = i32::MAX as i64;

/// `LONG_MAX`, the limit of a locator.
const LONG_MAX: i64 = i64::MAX;

/// What UKC reads from and prints on: `cngetc`, `printf`.
pub trait UkcCons: GetsnCons {
    /// `printf`.
    fn printf(&mut self, args: fmt::Arguments<'_>);
}

/// `printf(...)` on the editor's console.
macro_rules! uc_printf {
    ($uc:expr, $($arg:tt)*) => {
        $uc.cons.printf(format_args!($($arg)*))
    };
}

/// The state of one UKC session: the C's `userconf_*` globals and the tables they edit.
pub struct Userconf<'a, C: UkcCons> {
    /// The `ioconf.c` tables.
    t: IoconfTables<'a>,
    /// The console.
    cons: C,
    /// `userconf_base`: base for "large" numbers.
    base: i64,
    /// `userconf_maxdev`: the last used device slot.
    maxdev: i64,
    /// `userconf_totdev`: the last device slot.
    totdev: i64,
    /// `userconf_maxlocnames`: the last locator name used.
    maxlocnames: i64,
    /// `userconf_cnt`: line counter for `--- more ---`; -1 when not paging.
    cnt: i32,
    /// `userconf_lines`: number of lines per page.
    lines: i64,
    /// `userconf_histlen`.
    histlen: usize,
    /// `userconf_histcur`.
    histcur: usize,
    /// `userconf_history`: the commands that changed something, as text.
    history: [u8; USERCONF_HISTSZ],
}

/// The kernel's console, for [`user_config`].
struct Cons;

impl GetsnCons for Cons {
    fn cngetc(&mut self) -> i32 {
        cngetc()
    }

    fn cnputs(&mut self, s: &[u8]) {
        printf(format_args!("{}", Str(s)));
    }
}

impl UkcCons for Cons {
    fn printf(&mut self, args: fmt::Arguments<'_>) {
        printf(args);
    }
}

/// The byte at `i` of a command line, NUL past its end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// The rest of `s` from `i`.
fn from(s: &[u8], i: usize) -> &[u8] {
    s.get(i..).unwrap_or(&[])
}

/// Skips blanks (space, tab, newline) from `i`.
fn skip_blanks(s: &[u8], mut i: usize) -> usize {
    while matches!(at(s, i), b' ' | b'\t' | b'\n') {
        i += 1;
    }
    i
}

/// The length of the word at the start of `s` (up to a blank or the end).
fn word_len(s: &[u8]) -> usize {
    let mut i = 0;
    while !matches!(at(s, i), b' ' | b'\t' | b'\n' | 0) {
        i += 1;
    }
    i
}

/// `strlen(a) == len && strncasecmp(a, b, len) == 0` with `a` a name of the tables.
fn name_is(name: &[u8], s: &[u8], len: usize) -> bool {
    name.len() == len && strncasecmp(s, name, len) == 0
}

/// `userconf_number(c, &val, limit)`: a number in C notation (`0x` hex, `0` octal, decimal),
/// optionally negative, ending at a blank or the end; `Err(-1)` for a bad digit, `Err(1)` for
/// a negative number beyond `limit`.
fn userconf_number(c: &[u8], limit: i64) -> Result<i64, i32> {
    let mut i = 0;
    let mut neg = false;
    let mut base: u64 = 10;
    let mut num: u64 = 0;

    if at(c, i) == b'-' {
        neg = true;
        i += 1;
    }
    if at(c, i) == b'0' {
        base = 8;
        i += 1;
        if matches!(at(c, i), b'x' | b'X') {
            base = 16;
            i += 1;
        }
    }
    while !matches!(at(c, i), b'\n' | b'\t' | b' ' | 0) {
        let cc = match at(c, i) {
            d @ b'0'..=b'9' => d - b'0',
            d @ b'a'..=b'f' => d - b'a' + 10,
            d @ b'A'..=b'F' => d - b'A' + 10,
            _ => return Err(-1),
        };
        if u64::from(cc) > base {
            return Err(-1);
        }
        num = num.wrapping_mul(base).wrapping_add(u64::from(cc));
        i += 1;
    }

    if neg && num > limit as u64 {
        return Err(1); // overflow
    }
    Ok(if neg {
        (num as i64).wrapping_neg()
    } else {
        num as i64
    })
}

/// `userconf_device(cmd, &len, &unit, &state)`: a device name with an optional unit or `*`,
/// alone on the line; its name's length, unit and the state it selects (`FSTATE_FOUND` for
/// a bare name, `FSTATE_STAR` for `name*`, `FSTATE_NOTFOUND` for `name0`).
fn userconf_device(cmd: &[u8]) -> Option<(usize, i16, i16)> {
    let mut u: i16 = 0;
    let mut s = FSTATE_FOUND;
    let mut i = 0;

    while at(cmd, i).is_ascii_lowercase() {
        i += 1;
    }
    let l = i;
    if at(cmd, i) == b'*' {
        s = FSTATE_STAR;
        i += 1;
    } else {
        while at(cmd, i).is_ascii_digit() {
            s = FSTATE_NOTFOUND;
            u = u
                .wrapping_mul(10)
                .wrapping_add(i16::from(at(cmd, i) - b'0'));
            i += 1;
        }
    }
    i = skip_blanks(cmd, i);

    (at(cmd, i) == 0).then_some((l, u, s))
}

impl<'a, C: UkcCons> Userconf<'a, C> {
    /// A session over `t`, talking on `cons`, with the C's initial values.
    pub fn new(t: IoconfTables<'a>, cons: C) -> Self {
        Self {
            t,
            cons,
            base: 16,
            maxdev: -1,
            totdev: -1,
            maxlocnames: -1,
            cnt: -1,
            lines: 12,
            histlen: 0,
            histcur: 0,
            history: [0; USERCONF_HISTSZ],
        }
    }

    /// The console, given back.
    pub fn into_cons(self) -> C {
        self.cons
    }

    /// `userconf_history[0..userconf_histlen]`.
    pub fn history(&self) -> &[u8] {
        &self.history[..self.histlen]
    }

    /// `pdevnames_size`.
    fn pdevnames_size(&self) -> i64 {
        self.t.pdevinit.len() as i64
    }

    /// `locnames[i]`.
    fn locname(&self, i: i16) -> &'a [u8] {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.t.locnames.get(i))
            .copied()
            .unwrap_or(b"?")
    }

    /// The locator names of `cfdata[dev]`, as indices into `locnames[]`.
    fn locator_names(&self, dev: usize) -> impl Iterator<Item = i16> + '_ {
        let start = self.t.cfdata[dev].cf_locnames as usize;
        self.t
            .locnamp
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .copied()
            .take_while(|&n| n != -1)
    }

    /// `userconf_init()`: counts the used and free device slots and the locator names.
    fn userconf_init(&mut self) {
        let mut i = 0usize;
        while i < self.t.cfdata.len() && !self.t.cfdata[i].is_free() {
            self.maxdev = i as i64;
            self.totdev = i as i64;
            let ln = self.locator_names(i).max().map_or(-1, i64::from);
            if ln > self.maxlocnames {
                self.maxlocnames = ln;
            }
            crate::kassert!(self.locator_names(i).count() == self.t.cfdata[i].cf_loc.len());
            i += 1;
        }

        while i < self.t.cfdata.len() && self.t.cfdata[i].is_free() {
            self.totdev = i as i64;
            i += 1;
        }
        self.totdev -= 1;
    }

    /// `userconf_more()`: every `userconf_lines` lines while paging, waits for a key; true
    /// when it was `q`.
    fn userconf_more(&mut self) -> bool {
        let mut quit = false;
        let mut c = 0;

        if self.cnt != -1 {
            if i64::from(self.cnt) == self.lines {
                uc_printf!(self, "--- more ---");
                c = self.cons.cngetc();
                self.cnt = 0;
                uc_printf!(self, "\r            \r");
            }
            self.cnt += 1;
            if c == i32::from(b'q') || c == i32::from(b'Q') {
                quit = true;
            }
        }
        quit
    }

    /// `userconf_hist_cmd(cmd)`: starts a history record.
    fn userconf_hist_cmd(&mut self, cmd: u8) {
        self.histcur = self.histlen;
        if self.histcur < USERCONF_HISTSZ {
            self.history[self.histcur] = cmd;
            self.histcur += 1;
        }
    }

    /// `userconf_hist_int(val)`: appends ` <val>` to the record, if it fits.
    fn userconf_hist_int(&mut self, val: i64) {
        let mut buf = [0u8; USERCONF_BUFSZ];
        let n = crate::kern::subr_prf::snprintf(&mut buf, format_args!(" {val}"));
        let n = n.min(buf.len() - 1);
        if self.histcur + n < USERCONF_HISTSZ {
            self.history[self.histcur..self.histcur + n].copy_from_slice(&buf[..n]);
            self.histcur += n;
        }
    }

    /// `userconf_hist_eoc()`: ends the record.
    fn userconf_hist_eoc(&mut self) {
        if self.histcur < USERCONF_HISTSZ {
            self.history[self.histcur] = b'\n';
            self.histcur += 1;
            self.histlen = self.histcur;
        }
    }

    /// `userconf_pnum(val)`: small numbers in decimal, the rest in `userconf_base`.
    fn userconf_pnum(&mut self, val: i64) {
        if val > -2 && val < 16 {
            uc_printf!(self, "{val}");
            return;
        }
        match self.base {
            8 => uc_printf!(self, "0{:o}", val as u64),
            10 => uc_printf!(self, "{val}"),
            _ => uc_printf!(self, "0x{:x}", val as u64),
        }
    }

    /// `userconf_pdevnam(dev)`: the entry's name with its unit, `*` or `*FOUND*`.
    fn userconf_pdevnam(&mut self, dev: usize) {
        let cd = &self.t.cfdata[dev];
        let (name, fstate, unit) = (cd.cf_driver.cd_name, cd.cf_fstate.get(), cd.cf_unit.get());
        uc_printf!(self, "{}", Str(name));
        match fstate {
            FSTATE_NOTFOUND | FSTATE_DNOTFOUND => uc_printf!(self, "{unit}"),
            FSTATE_FOUND => uc_printf!(self, "*FOUND*"),
            FSTATE_STAR | FSTATE_DSTAR => uc_printf!(self, "*"),
            _ => uc_printf!(self, "*UNKNOWN*"),
        }
    }

    /// `userconf_pdev(devno)`: one line about a device slot or a pseudo-device.
    fn userconf_pdev(&mut self, devno: i64) {
        if devno > self.maxdev && devno <= self.totdev {
            uc_printf!(self, "{devno:3} free slot (for add)\n");
            return;
        }

        if devno > self.totdev && devno <= self.totdev + self.pdevnames_size() {
            let p = (devno - self.totdev - 1) as usize;
            let (name, count) = (self.t.pdevnames[p], self.t.pdevinit[p].pdev_count);
            uc_printf!(
                self,
                "{devno:3} {} count {}",
                Str(name),
                count.unsigned_abs()
            );
            if count < 1 {
                uc_printf!(self, " disable");
            }
            uc_printf!(self, " (pseudo device)\n");
            return;
        }

        if devno > self.maxdev || devno < 0 {
            uc_printf!(self, "Unknown devno (max is {})\n", self.maxdev);
            return;
        }
        let dev = devno as usize;

        uc_printf!(self, "{devno:3} ");
        self.userconf_pdevnam(dev);
        uc_printf!(self, " at");
        let parents = self.t.cfdata[dev].cf_parents;
        if parents.is_empty() {
            uc_printf!(self, " root");
        }
        let mut c = b' ';
        for &p in parents {
            uc_printf!(self, "{}", char::from(c));
            self.userconf_pdevnam(p as usize);
            c = b'|';
        }
        match self.t.cfdata[dev].cf_fstate.get() {
            FSTATE_NOTFOUND | FSTATE_FOUND | FSTATE_STAR => {}
            FSTATE_DNOTFOUND | FSTATE_DSTAR => uc_printf!(self, " disable"),
            _ => uc_printf!(self, " ???"),
        }
        let loc = self.t.cfdata[dev].cf_loc;
        let names: Vec<i16> = self.locator_names(dev).collect();
        for (&n, &l) in names.iter().zip(loc) {
            uc_printf!(self, " {} ", Str(self.locname(n)));
            self.userconf_pnum(l);
        }
        uc_printf!(self, " flags 0x{:x}\n", self.t.cfdata[dev].cf_flags as u32);
    }

    /// `userconf_attr(cmd, &val)`: the index of the locator name the line starts with.
    fn userconf_attr(&self, cmd: &[u8]) -> Option<i64> {
        let l = word_len(cmd);
        let mut attr = None;
        for i in 0..=self.maxlocnames {
            if name_is(self.locname(i as i16), cmd, l) {
                attr = Some(i);
            }
        }
        attr
    }

    /// `userconf_modify(item, &val, limit)`: prompts `item [val] ?` until the answer is empty
    /// (keep) or a number.
    fn userconf_modify(&mut self, item: &[u8], val: &mut i64, limit: i64) {
        loop {
            uc_printf!(self, "{} [", Str(item));
            self.userconf_pnum(*val);
            uc_printf!(self, "] ? ");

            let mut argbuf = [0u8; USERCONF_BUFSZ];
            let n = getsn(&mut argbuf, &mut self.cons);
            let arg = &argbuf[..n];
            let c = skip_blanks(arg, 0);

            if at(arg, c) == 0 {
                return;
            }
            match userconf_number(from(arg, c), limit) {
                Ok(a) => {
                    *val = a;
                    return;
                }
                Err(_) => uc_printf!(self, "Unknown argument\n"),
            }
        }
    }

    /// Asks `change (y/n) ?` until the answer is one of them; true for yes.
    fn ask_change(&mut self) -> bool {
        loop {
            uc_printf!(self, "change (y/n) ?");
            let c = self.cons.cngetc();
            uc_printf!(self, "\n");
            match u8::try_from(c) {
                Ok(b'y' | b'Y') => return true,
                Ok(b'n' | b'N') => return false,
                _ => {}
            }
        }
    }

    /// `userconf_change(devno)`: edits a device's locators and flags, or a pseudo-device's
    /// count.
    fn userconf_change(&mut self, devno: i64) {
        if (0..=self.maxdev).contains(&devno) {
            let dev = devno as usize;
            self.userconf_pdev(devno);

            if self.ask_change() {
                // XXX add cmd 'c' <devno>
                self.userconf_hist_cmd(b'c');
                self.userconf_hist_int(devno);

                // The locators are constant: edit a copy, kept if anything changed.
                let old = self.t.cfdata[dev].cf_loc;
                let mut lk: Vec<i64> = Vec::new();
                if lk.try_reserve_exact(old.len()).is_err() {
                    uc_printf!(self, "out of memory.\n");
                    return;
                }
                lk.extend_from_slice(old);

                let names: Vec<i16> = self.locator_names(dev).collect();
                for (&n, l) in names.iter().zip(lk.iter_mut()) {
                    self.userconf_modify(self.locname(n), l, LONG_MAX);
                    // XXX add *l
                    self.userconf_hist_int(*l);
                }
                let mut tmp = i64::from(self.t.cfdata[dev].cf_flags);
                self.userconf_modify(b"flags", &mut tmp, INT_MAX);
                self.userconf_hist_int(tmp);
                self.t.cfdata[dev].cf_flags = tmp as i32;

                if lk[..] != *old {
                    self.t.cfdata[dev].cf_loc = lk.leak();
                }

                uc_printf!(self, "{devno:3} ");
                self.userconf_pdevnam(dev);
                uc_printf!(self, " changed\n");
                self.userconf_pdev(devno);
            }
            return;
        }

        if devno > self.maxdev && devno <= self.totdev {
            uc_printf!(self, "{devno:3} can't change free slot\n");
            return;
        }

        if devno > self.totdev && devno <= self.totdev + self.pdevnames_size() {
            let p = (devno - self.totdev - 1) as usize;
            self.userconf_pdev(devno);
            if self.ask_change() {
                // XXX add cmd 'c' <devno>
                self.userconf_hist_cmd(b'c');
                self.userconf_hist_int(devno);

                let mut tmp = i64::from(self.t.pdevinit[p].pdev_count);
                self.userconf_modify(b"count", &mut tmp, INT_MAX);
                self.userconf_hist_int(tmp);
                self.t.pdevinit[p].pdev_count = tmp as i32;

                uc_printf!(self, "{devno:3} {} changed\n", Str(self.t.pdevnames[p]));
                self.userconf_pdev(devno);

                // XXX add eoc
                self.userconf_hist_eoc();
            }
            return;
        }

        uc_printf!(
            self,
            "Unknown devno (max is {})\n",
            self.totdev + self.pdevnames_size()
        );
    }

    /// `userconf_disable(devno)`.
    fn userconf_disable(&mut self, devno: i64) {
        if (0..=self.maxdev).contains(&devno) {
            let dev = devno as usize;
            let fstate = &self.t.cfdata[dev].cf_fstate;
            let mut done = false;
            match fstate.get() {
                FSTATE_NOTFOUND => fstate.set(FSTATE_DNOTFOUND),
                FSTATE_STAR => fstate.set(FSTATE_DSTAR),
                FSTATE_DNOTFOUND | FSTATE_DSTAR => done = true,
                _ => uc_printf!(self, "Error unknown state\n"),
            }

            uc_printf!(self, "{devno:3} ");
            self.userconf_pdevnam(dev);
            if done {
                uc_printf!(self, " already");
            } else {
                // XXX add cmd 'd' <devno> eoc
                self.userconf_hist_cmd(b'd');
                self.userconf_hist_int(devno);
                self.userconf_hist_eoc();
            }
            uc_printf!(self, " disabled\n");
            return;
        }

        if devno > self.maxdev && devno <= self.totdev {
            uc_printf!(self, "{devno:3} can't disable free slot\n");
            return;
        }

        if devno > self.totdev && devno <= self.totdev + self.pdevnames_size() {
            let p = (devno - self.totdev - 1) as usize;
            uc_printf!(self, "{devno:3} {}", Str(self.t.pdevnames[p]));
            if self.t.pdevinit[p].pdev_count < 1 {
                uc_printf!(self, " already ");
            } else {
                self.t.pdevinit[p].pdev_count *= -1;
                // XXX add cmd 'd' <devno> eoc
                self.userconf_hist_cmd(b'd');
                self.userconf_hist_int(devno);
                self.userconf_hist_eoc();
            }
            uc_printf!(self, " disabled\n");
            return;
        }

        uc_printf!(
            self,
            "Unknown devno (max is {})\n",
            self.totdev + self.pdevnames_size()
        );
    }

    /// `userconf_enable(devno)`.
    fn userconf_enable(&mut self, devno: i64) {
        if (0..=self.maxdev).contains(&devno) {
            let dev = devno as usize;
            let fstate = &self.t.cfdata[dev].cf_fstate;
            let mut done = false;
            match fstate.get() {
                FSTATE_DNOTFOUND => fstate.set(FSTATE_NOTFOUND),
                FSTATE_DSTAR => fstate.set(FSTATE_STAR),
                FSTATE_NOTFOUND | FSTATE_STAR => done = true,
                _ => uc_printf!(self, "Error unknown state\n"),
            }

            uc_printf!(self, "{devno:3} ");
            self.userconf_pdevnam(dev);
            if done {
                uc_printf!(self, " already");
            } else {
                // XXX add cmd 'e' <devno> eoc
                self.userconf_hist_cmd(b'e');
                self.userconf_hist_int(devno);
                self.userconf_hist_eoc();
            }
            uc_printf!(self, " enabled\n");
            return;
        }

        if devno > self.maxdev && devno <= self.totdev {
            uc_printf!(self, "{devno:3} can't enable free slot\n");
            return;
        }

        if devno > self.totdev && devno <= self.totdev + self.pdevnames_size() {
            let p = (devno - self.totdev - 1) as usize;
            uc_printf!(self, "{devno:3} {}", Str(self.t.pdevnames[p]));
            if self.t.pdevinit[p].pdev_count > 0 {
                uc_printf!(self, " already");
            } else {
                self.t.pdevinit[p].pdev_count *= -1;
                // XXX add cmd 'e' <devno> eoc
                self.userconf_hist_cmd(b'e');
                self.userconf_hist_int(devno);
                self.userconf_hist_eoc();
            }
            uc_printf!(self, " enabled\n");
            return;
        }

        uc_printf!(
            self,
            "Unknown devno (max is {})\n",
            self.totdev + self.pdevnames_size()
        );
    }

    /// `userconf_help()`.
    fn userconf_help(&mut self) {
        uc_printf!(self, "command   args                description\n");
        for &(name, cmd) in USERCONF_CMDS {
            uc_printf!(self, "{}", Str(name));
            for _ in name.len()..10 {
                uc_printf!(self, " ");
            }
            let what = match cmd {
                b'L' => "[count]             number of lines before more",
                b'a' => "dev                 add a device",
                b'b' => "8|10|16             base on large numbers",
                b'c' => "devno|dev           change devices",
                b'D' => "                    enter ddb",
                b'd' => "attr val|devno|dev  disable devices",
                b'e' => "attr val|devno|dev  enable devices",
                b'f' => "devno|dev           find devices",
                b'h' => "                    this message",
                b'l' => "                    list configuration",
                b'q' => "                    leave UKC",
                b's' => "[attr [val]]        show attributes (or devices with an attribute)",
                b'v' => "                    toggle verbose booting",
                _ => "                    don't know",
            };
            uc_printf!(self, "{what}\n");
        }
    }

    /// `userconf_list()`: every slot and pseudo-device, a page at a time.
    fn userconf_list(&mut self) {
        self.cnt = 0;
        let mut i = 0;
        while i <= self.totdev + self.pdevnames_size() {
            if self.userconf_more() {
                break;
            }
            self.userconf_pdev(i);
            i += 1;
        }
        self.cnt = -1;
    }

    /// `userconf_show()`: the locator names.
    fn userconf_show(&mut self) {
        self.cnt = 0;
        let mut i = 0;
        while i <= self.maxlocnames {
            if self.userconf_more() {
                break;
            }
            uc_printf!(self, "{}\n", Str(self.locname(i as i16)));
            i += 1;
        }
        self.cnt = -1;
    }

    /// `userconf_common_attr_val(attr, val, routine)`: applies `routine` to every device with
    /// locator `attr` (equal to `val`, or any value when `None`, which shows them).
    fn userconf_common_attr_val(&mut self, attr: i64, val: Option<i64>, routine: u8) {
        self.cnt = 0;
        let mut quit = false;
        let mut i = 0;
        while i <= self.maxdev {
            let dev = i as usize;
            let names: Vec<i16> = self.locator_names(dev).collect();
            let loc = self.t.cfdata[dev].cf_loc;
            for (&n, &l) in names.iter().zip(loc) {
                if i64::from(n) == attr {
                    match val {
                        None => {
                            quit = self.userconf_more();
                            self.userconf_pdev(i);
                        }
                        Some(v) if v == l => {
                            quit = self.userconf_more();
                            match routine {
                                UC_ENABLE => self.userconf_enable(i),
                                UC_DISABLE => self.userconf_disable(i),
                                UC_SHOW => self.userconf_pdev(i),
                                _ => {
                                    uc_printf!(self, "Unknown routine /{}/\n", char::from(routine))
                                }
                            }
                        }
                        Some(_) => {}
                    }
                }
                if quit {
                    break;
                }
            }
            if quit {
                break;
            }
            i += 1;
        }
        self.cnt = -1;
    }

    /// `userconf_show_attr(cmd)`: `show attr [val]`.
    fn userconf_show_attr(&mut self, cmd: &[u8]) {
        let l = word_len(cmd);
        let c = skip_blanks(cmd, l);
        let Some(attr) = self.userconf_attr(cmd) else {
            uc_printf!(self, "Unknown attribute\n");
            return;
        };

        if at(cmd, c) == 0 {
            self.userconf_common_attr_val(attr, None, UC_SHOW);
        } else {
            match userconf_number(from(cmd, c), INT_MAX) {
                Ok(a) => self.userconf_common_attr_val(attr, Some(a), UC_SHOW),
                Err(_) => uc_printf!(self, "Unknown argument\n"),
            }
        }
    }

    /// `userconf_common_dev(dev, len, unit, state, routine)`: applies `routine` to every
    /// entry named `dev[..len]` (any of them for `FSTATE_FOUND`, the starred ones for
    /// `FSTATE_STAR`, unit `unit` for `FSTATE_NOTFOUND`), then to the pseudo-device of that
    /// name.
    fn userconf_common_dev(&mut self, dev: &[u8], len: usize, unit: i16, state: i16, routine: u8) {
        if routine != UC_CHANGE {
            self.cnt = 0;
        }

        let mut i = 0usize;
        while i < self.t.cfdata.len() && !self.t.cfdata[i].is_free() {
            let cf = &self.t.cfdata[i];
            let fstate = cf.cf_fstate.get();
            if name_is(cf.cf_driver.cd_name, dev, len)
                && (state == FSTATE_FOUND
                    || (state == FSTATE_STAR && (fstate == FSTATE_STAR || fstate == FSTATE_DSTAR))
                    || (state == FSTATE_NOTFOUND
                        && cf.cf_unit.get() == unit
                        && (fstate == FSTATE_NOTFOUND || fstate == FSTATE_DNOTFOUND)))
            {
                if self.userconf_more() {
                    break;
                }
                match routine {
                    UC_CHANGE => self.userconf_change(i as i64),
                    UC_ENABLE => self.userconf_enable(i as i64),
                    UC_DISABLE => self.userconf_disable(i as i64),
                    UC_FIND => self.userconf_pdev(i as i64),
                    _ => uc_printf!(self, "Unknown routine /{}/\n", char::from(routine)),
                }
            }
            i += 1;
        }

        for i in 0..self.t.pdevinit.len() {
            if strncasecmp(dev, self.t.pdevnames[i], len) == 0 && state == FSTATE_FOUND {
                let devno = self.totdev + 1 + i as i64;
                match routine {
                    UC_CHANGE => self.userconf_change(devno),
                    UC_ENABLE => self.userconf_enable(devno),
                    UC_DISABLE => self.userconf_disable(devno),
                    UC_FIND => self.userconf_pdev(devno),
                    _ => uc_printf!(self, "Unknown pseudo routine /{}/\n", char::from(routine)),
                }
            }
        }

        if routine != UC_CHANGE {
            self.cnt = -1;
        }
    }

    /// `userconf_common_attr(cmd, attr, routine)`: `enable`/`disable attr val`.
    fn userconf_common_attr(&mut self, cmd: &[u8], attr: i64, routine: u8) {
        let c = skip_blanks(cmd, word_len(cmd));

        if at(cmd, c) == 0 {
            uc_printf!(self, "Value missing for attribute\n");
            return;
        }

        match userconf_number(from(cmd, c), INT_MAX) {
            Ok(a) => self.userconf_common_attr_val(attr, Some(a), routine),
            Err(_) => uc_printf!(self, "Unknown argument\n"),
        }
    }

    /// `userconf_add_read(prompt, field, dev, len, &val)`: asks for a device number (`?`
    /// lists the candidates, `q` or an empty answer gives up, `None`); for `field` `a` it
    /// must be a `dev`.
    fn userconf_add_read(
        &mut self,
        prompt: &str,
        field: u8,
        dev: &[u8],
        len: usize,
    ) -> Option<i64> {
        loop {
            uc_printf!(self, "{prompt} ? ");

            let mut argbuf = [0u8; USERCONF_BUFSZ];
            let n = getsn(&mut argbuf, &mut self.cons);
            let arg = &argbuf[..n];
            let c = skip_blanks(arg, 0);

            if at(arg, c) == 0 {
                return None;
            }
            if let Ok(a) = userconf_number(from(arg, c), INT_MAX) {
                if a > self.maxdev || a < 0 {
                    uc_printf!(self, "Unknown devno (max is {})\n", self.maxdev);
                } else if strncasecmp(dev, self.t.cfdata[a as usize].cf_driver.cd_name, len) != 0
                    && field == b'a'
                {
                    uc_printf!(self, "Not same device type\n");
                } else {
                    return Some(a);
                }
            } else if at(arg, c) == b'?' {
                self.userconf_common_dev(dev, len, 0, FSTATE_FOUND, UC_FIND);
            } else if matches!(at(arg, c), b'q' | b'Q') {
                return None;
            } else {
                uc_printf!(self, "Unknown argument\n");
            }
        }
    }

    /// `userconf_add(dev, len, unit, state)`: clones an entry of the same driver as unit
    /// `unit` (or starred) into the slot before another entry, renumbering what follows.
    fn userconf_add(&mut self, dev: &[u8], len: usize, unit: i16, state: i16) {
        if self.maxdev == self.totdev {
            uc_printf!(self, "No more space for new devices.\n");
            return;
        }

        if state == FSTATE_FOUND {
            uc_printf!(self, "Device not complete number or * is missing\n");
            return;
        }

        let configured = self.t.cfdata.iter().take_while(|cf| !cf.is_free());
        let found = configured
            .filter(|cf| name_is(cf.cf_driver.cd_name, dev, len))
            .count()
            > 0;
        if !found {
            uc_printf!(self, "No device of this type exists.\n");
            return;
        }

        let Some(orig) = self.userconf_add_read("Clone Device (DevNo, 'q' or '?')", b'a', dev, len)
        else {
            return;
        };
        let new = self.t.cfdata[orig as usize].clone();
        new.cf_unit.set(unit);
        new.cf_fstate.set(state);
        let Some(val) =
            self.userconf_add_read("Insert before Device (DevNo, 'q' or '?')", b'i', dev, len)
        else {
            return;
        };

        // The parent vectors are constant: renumber copies, one per vector, before anything
        // moves, so running out of memory changes nothing.
        let maxdev = self.maxdev as usize;
        let val_ = val as usize;
        let mut fixed: Vec<(&[i16], &'static [i16])> = Vec::new();
        for cf in &self.t.cfdata[..=maxdev] {
            let pv = cf.cf_parents;
            if !pv.iter().any(|&p| i64::from(p) >= val)
                || fixed.iter().any(|(old, _)| core::ptr::eq(*old, pv))
            {
                continue;
            }
            let mut copy: Vec<i16> = Vec::new();
            if fixed.try_reserve(1).is_err() || copy.try_reserve_exact(pv.len()).is_err() {
                uc_printf!(self, "out of memory.\n");
                return;
            }
            copy.extend(
                pv.iter()
                    .map(|&p| if i64::from(p) >= val { p + 1 } else { p }),
            );
            fixed.push((pv, copy.leak()));
        }

        // XXX add cmd 'a' <orig> <val> eoc
        self.userconf_hist_cmd(b'a');
        self.userconf_hist_int(orig);
        self.userconf_hist_int(i64::from(unit));
        self.userconf_hist_int(i64::from(state));
        self.userconf_hist_int(val);
        self.userconf_hist_eoc();

        // Insert the new record: the free slot after the last entry moves to `val`.
        self.t.cfdata[val_..=maxdev + 1].rotate_right(1);
        self.t.cfdata[val_] = new;

        // Fix indexes in pv
        for cf in &mut self.t.cfdata[..=maxdev + 1] {
            if let Some((_, copy)) = fixed
                .iter()
                .find(|(old, _)| core::ptr::eq(*old, cf.cf_parents))
            {
                cf.cf_parents = copy;
            }
        }

        // Fix indexes in cfroots
        for r in self.t.cfroots.iter_mut() {
            if *r != -1 && i64::from(*r) >= val {
                *r += 1;
            }
        }

        self.maxdev += 1;

        // Find max unit number of the device type
        let mut max_unit: i16 = -1;
        let mut star_unit: i16 = -1;
        for cf in self.t.cfdata.iter().take_while(|cf| !cf.is_free()) {
            if name_is(cf.cf_driver.cd_name, dev, len)
                && matches!(cf.cf_fstate.get(), FSTATE_NOTFOUND | FSTATE_DNOTFOUND)
            {
                max_unit = max_unit.max(cf.cf_unit.get());
                star_unit = star_unit.max(cf.cf_unit.get());
            }
        }

        // For all * entries set unit number to max+1, and update cf_starunit1 if necessary.
        max_unit += 1;
        star_unit += 1;
        for cf in self.t.cfdata.iter_mut().take_while(|cf| !cf.is_free()) {
            if name_is(cf.cf_driver.cd_name, dev, len)
                && matches!(cf.cf_fstate.get(), FSTATE_STAR | FSTATE_DSTAR)
            {
                cf.cf_unit.set(max_unit);
                if cf.cf_starunit1 < star_unit {
                    cf.cf_starunit1 = star_unit;
                }
            }
        }
        self.userconf_pdev(val);
    }

    /// `userconf_parse(cmd)`: runs one command line; true for `quit`.
    pub fn userconf_parse(&mut self, cmd: &[u8]) -> bool {
        let v = skip_blanks(cmd, 0).min(cmd.len());
        let word = from(cmd, v);
        let i = word_len(word);

        let mut k = None;
        for &(name, letter) in USERCONF_CMDS {
            if name_is(name, word, i) {
                k = Some(letter);
            }
        }

        let c = from(word, skip_blanks(word, i));
        let none = at(c, 0) == 0;

        let Some(k) = k else {
            if at(word, 0) != b'\n' {
                uc_printf!(self, "Unknown command, try help\n");
            }
            return false;
        };
        match k {
            b'L' => {
                if none {
                    uc_printf!(self, "Argument expected\n");
                } else if let Ok(a) = userconf_number(c, INT_MAX) {
                    self.lines = a;
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'a' => {
                if none {
                    uc_printf!(self, "Dev expected\n");
                } else if let Some((len, unit, state)) = userconf_device(c) {
                    self.userconf_add(c, len, unit, state);
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'b' => {
                if none {
                    uc_printf!(self, "8|10|16 expected\n");
                } else if let Ok(a) = userconf_number(c, INT_MAX) {
                    if a == 8 || a == 10 || a == 16 {
                        self.base = a;
                    } else {
                        uc_printf!(self, "8|10|16 expected\n");
                    }
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'c' => {
                if none {
                    uc_printf!(self, "DevNo or Dev expected\n");
                } else if let Ok(a) = userconf_number(c, INT_MAX) {
                    self.userconf_change(a);
                } else if let Some((len, unit, state)) = userconf_device(c) {
                    self.userconf_common_dev(c, len, unit, state, UC_CHANGE);
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'D' => crate::machine::db_machdep::db_enter(),
            b'd' | b'e' => {
                let (routine, one): (u8, fn(&mut Self, i64)) = if k == b'd' {
                    (UC_DISABLE, Self::userconf_disable)
                } else {
                    (UC_ENABLE, Self::userconf_enable)
                };
                if none {
                    uc_printf!(self, "Attr, DevNo or Dev expected\n");
                } else if let Some(a) = self.userconf_attr(c) {
                    self.userconf_common_attr(c, a, routine);
                } else if let Ok(a) = userconf_number(c, INT_MAX) {
                    one(self, a);
                } else if let Some((len, unit, state)) = userconf_device(c) {
                    self.userconf_common_dev(c, len, unit, state, routine);
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'f' => {
                if none {
                    uc_printf!(self, "DevNo or Dev expected\n");
                } else if let Ok(a) = userconf_number(c, INT_MAX) {
                    self.userconf_pdev(a);
                } else if let Some((len, unit, state)) = userconf_device(c) {
                    self.userconf_common_dev(c, len, unit, state, UC_FIND);
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'h' => self.userconf_help(),
            b'l' => {
                if none {
                    self.userconf_list();
                } else {
                    uc_printf!(self, "Unknown argument\n");
                }
            }
            b'q' => {
                // XXX add cmd 'q' eoc
                self.userconf_hist_cmd(b'q');
                self.userconf_hist_eoc();
                return true;
            }
            b's' => {
                if none {
                    self.userconf_show();
                } else {
                    self.userconf_show_attr(c);
                }
            }
            b'v' => {
                let verbose = AUTOCONF_VERBOSE.load(Ordering::Relaxed) == 0;
                AUTOCONF_VERBOSE.store(i32::from(verbose), Ordering::Relaxed);
                uc_printf!(
                    self,
                    "autoconf verbose {}abled\n",
                    if verbose { "en" } else { "dis" }
                );
            }
            _ => uc_printf!(self, "Unknown command\n"),
        }
        false
    }

    /// `user_config()`'s body: the `UKC>` loop until `quit`, with `pollc` as `cnpollc`.
    pub fn run(&mut self, pollc: fn(bool)) {
        self.userconf_init();
        uc_printf!(self, "User Kernel Config\n");

        pollc(true);
        loop {
            uc_printf!(self, "UKC> ");
            let mut cmdbuf = [0u8; USERCONF_BUFSZ];
            let n = getsn(&mut cmdbuf, &mut self.cons);
            if n > 0 && self.userconf_parse(&cmdbuf[..n]) {
                break;
            }
        }
        pollc(false);

        uc_printf!(self, "Continuing...\n");
    }
}

/// `user_config()`: the `UKC>` prompt on the console, from `cpu_startup` when the kernel was
/// booted with `-c`.
pub fn user_config() {
    // SAFETY: called once, from `cpu_startup` on the boot CPU, before autoconfiguration (or
    // anything else) reads the tables; the session, and with it the tables, ends here.
    let t = unsafe { ioconf_mut() };
    Userconf::new(t, Cons).run(cnpollc);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::device::{Cfattach, Cfdata, Cfdriver, DV_DULL, Pdevinit};
    use std::string::String;
    use std::vec::Vec;

    static T_CA: Cfattach = Cfattach {
        ca_devsize: 0,
        ca_match: None,
        ca_attach: |_, _, _| {},
        ca_detach: None,
        ca_activate: None,
    };
    static MAINBUS_CD: Cfdriver = Cfdriver::new(b"mainbus", DV_DULL, 0);
    static PCI_CD: Cfdriver = Cfdriver::new(b"pci", DV_DULL, 0);
    static COM_CD: Cfdriver = Cfdriver::new(b"com", DV_DULL, 0);
    static VIO_CD: Cfdriver = Cfdriver::new(b"vio", DV_DULL, 0);

    const LOC_COM0: &[i64] = &[0x3f8, 4];
    static LOCNAMES: [&[u8]; 5] = [b"bus", b"port", b"irq", b"dev", b"function"];
    static LOCNAMP: [i16; 9] = [-1, 0, -1, 1, 2, -1, 3, 4, -1];
    static PDEVNAMES: [&[u8]; 2] = [b"pty", b"loop"];

    /// mainbus0 at root; pci* at mainbus0; com0 and com1 (disabled) at mainbus0 with `port`
    /// and `irq`; vio* at pci*; eight free slots.
    fn table() -> Vec<Cfdata> {
        let mut t = std::vec![
            Cfdata::new(&T_CA, &MAINBUS_CD, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0),
            Cfdata::new(&T_CA, &PCI_CD, 0, FSTATE_STAR, &[-1], 0, &[0], 1, 0),
            Cfdata::new(&T_CA, &COM_CD, 0, FSTATE_NOTFOUND, LOC_COM0, 0, &[0], 3, 0),
            Cfdata::new(
                &T_CA,
                &COM_CD,
                1,
                FSTATE_DNOTFOUND,
                &[0x2f8, 3],
                0,
                &[0],
                3,
                0
            ),
            Cfdata::new(&T_CA, &VIO_CD, 0, FSTATE_STAR, &[-1, -1], 0, &[1], 6, 0),
        ];
        t.extend((0..8).map(|_| Cfdata::free()));
        t
    }

    fn pdevinit() -> Vec<Pdevinit> {
        std::vec![
            Pdevinit {
                pdev_attach: |_| {},
                pdev_count: 16,
            },
            Pdevinit {
                pdev_attach: |_| {},
                pdev_count: 1,
            },
        ]
    }

    /// The keys typed and what was printed.
    struct Script {
        keys: Vec<u8>,
        next: usize,
        out: String,
    }

    impl GetsnCons for Script {
        fn cngetc(&mut self) -> i32 {
            let k = self.keys[self.next];
            self.next += 1;
            i32::from(k)
        }

        fn cnputs(&mut self, s: &[u8]) {
            self.out.push_str(core::str::from_utf8(s).unwrap());
        }
    }

    impl UkcCons for Script {
        fn printf(&mut self, args: fmt::Arguments<'_>) {
            use core::fmt::Write;
            self.out.write_fmt(args).unwrap();
        }
    }

    /// Runs a session on `keys` over the tables; returns what it printed and the history.
    fn session(
        keys: &str,
        cfdata: &mut [Cfdata],
        cfroots: &mut [i16],
        pdev: &mut [Pdevinit],
    ) -> (String, Vec<u8>) {
        let t = IoconfTables {
            cfdata,
            cfroots,
            pdevinit: pdev,
            pdevnames: &PDEVNAMES,
            locnames: &LOCNAMES,
            locnamp: &LOCNAMP,
        };
        let cons = Script {
            keys: keys.as_bytes().to_vec(),
            next: 0,
            out: String::new(),
        };
        let mut uc = Userconf::new(t, cons);
        uc.run(|_| {});
        let hist = uc.history().to_vec();
        let cons = uc.into_cons();
        assert_eq!(cons.next, cons.keys.len(), "every key read");
        (cons.out, hist)
    }

    fn run(keys: &str) -> (String, Vec<Cfdata>, Vec<i16>, Vec<Pdevinit>, Vec<u8>) {
        let (mut cf, mut roots, mut pdev) = (table(), std::vec![0i16], pdevinit());
        let (out, hist) = session(keys, &mut cf, &mut roots, &mut pdev);
        (out, cf, roots, pdev, hist)
    }

    #[test]
    fn find_and_disable_a_device_by_name() {
        let (out, cf, _, _, hist) = run("find vio\rdisable vio\rquit\r");
        assert!(out.starts_with("User Kernel Config\nUKC> find vio\n"));
        assert!(out.contains("  4 vio* at pci* dev -1 function -1 flags 0x0\n"));
        assert!(out.contains("UKC> disable vio\n  4 vio* disabled\n"));
        assert!(out.ends_with("UKC> quit\nContinuing...\n"));
        assert_eq!(cf[4].cf_fstate.get(), FSTATE_DSTAR);
        assert_eq!(hist, b"d 4\nq\n");
    }

    #[test]
    fn list_pages_and_shows_free_slots_and_pseudo_devices() {
        // 5 entries, 7 free slots (the eighth stays the end), 2 pseudo-devices: 14 lines,
        // a `--- more ---` after 12, answered with a space.
        let (out, ..) = run("list\r quit\r");
        assert!(out.contains("  0 mainbus0 at root flags 0x0\n"));
        assert!(out.contains("  1 pci* at mainbus0 bus -1 flags 0x0\n"));
        assert!(out.contains("  2 com0 at mainbus0 port 0x3f8 irq 4 flags 0x0\n"));
        assert!(out.contains("  3 com1 at mainbus0 disable port 0x2f8 irq 3 flags 0x0\n"));
        assert!(out.contains("  5 free slot (for add)\n"));
        assert!(out.contains(" 11 free slot (for add)\n"));
        assert!(!out.contains(" 12 free slot"));
        assert!(out.contains("--- more ---\r            \r"));
        assert!(out.contains(" 12 pty count 16 (pseudo device)\n"));
        assert!(out.contains(" 13 loop count 1 (pseudo device)\n"));
    }

    #[test]
    fn enable_and_disable_by_locator_and_number() {
        let (out, cf, ..) = run("disable irq 4\renable port 0x2f8\rdisable 2\renable 7\rquit\r");
        assert!(out.contains("UKC> disable irq 4\n  2 com0 disabled\n"));
        assert!(out.contains("UKC> enable port 0x2f8\n  3 com1 enabled\n"));
        assert!(out.contains("UKC> disable 2\n  2 com0 already disabled\n"));
        assert!(out.contains("UKC> enable 7\n  7 can't enable free slot\n"));
        assert_eq!(cf[2].cf_fstate.get(), FSTATE_DNOTFOUND);
        assert_eq!(cf[3].cf_fstate.get(), FSTATE_NOTFOUND);
    }

    #[test]
    fn units_and_stars_select_entries() {
        let (out, cf, ..) = run("disable com1\rdisable com0\rfind com\rfind 99\rquit\r");
        assert!(out.contains("UKC> disable com1\n  3 com1 already disabled\n"));
        assert!(out.contains("UKC> disable com0\n  2 com0 disabled\n"));
        assert!(out.contains("  2 com0 at mainbus0 disable port 0x3f8 irq 4 flags 0x0\n"));
        assert!(out.contains("UKC> find 99\nUnknown devno (max is 4)\n"));
        assert_eq!(cf[2].cf_fstate.get(), FSTATE_DNOTFOUND);
    }

    #[test]
    fn pseudo_devices_are_disabled_by_a_negative_count() {
        let (out, _, _, pdev, hist) = run("disable pty\rfind pty\renable 12\rdisable loop\rquit\r");
        assert!(out.contains("UKC> disable pty\n 12 pty disabled\n"));
        assert!(out.contains(" 12 pty count 16 disable (pseudo device)\n"));
        assert!(out.contains("UKC> enable 12\n 12 pty enabled\n"));
        assert!(out.contains("UKC> disable loop\n 13 loop disabled\n"));
        assert_eq!(pdev[0].pdev_count, 16);
        assert_eq!(pdev[1].pdev_count, -1);
        assert_eq!(hist, b"d 12\ne 12\nd 13\nq\n");
    }

    #[test]
    fn change_edits_a_copy_of_the_locators_and_the_flags() {
        let (out, cf, ..) = run("change 2\rxy0x3e8\r\r1\rchange com0\rn quit\r");
        assert!(out.contains("change (y/n) ?\n"));
        assert!(out.contains("port [0x3f8] ? 0x3e8\nirq [4] ? \nflags [0] ? 1\n"));
        assert!(
            out.contains("  2 com0 changed\n  2 com0 at mainbus0 port 0x3e8 irq 4 flags 0x1\n")
        );
        assert_eq!(cf[2].cf_loc, &[0x3e8, 4]);
        assert_eq!(cf[2].cf_flags, 1);
        assert_eq!(LOC_COM0, &[0x3f8, 4]);
    }

    #[test]
    fn change_keeps_the_locators_when_nothing_changed() {
        let (_, cf, ..) = run("change 2\ry\r\r\rquit\r");
        assert!(core::ptr::eq(cf[2].cf_loc, LOC_COM0));
    }

    #[test]
    fn add_inserts_a_clone_and_renumbers_parents_and_roots() {
        let (mut cf, mut roots, mut pdev) = (table(), std::vec![0i16, 3], pdevinit());
        let (out, hist) = session(
            "add vio1\r?\r4\r1\rfind vio\rquit\r",
            &mut cf,
            &mut roots,
            &mut pdev,
        );
        // `?` lists the candidates.
        assert!(out.contains("Clone Device (DevNo, 'q' or '?') ? ?\n  4 vio* at pci* dev -1"));
        assert!(out.contains("Insert before Device (DevNo, 'q' or '?') ? 1\n"));
        assert!(out.contains("  1 vio1 at pci* dev -1 function -1 flags 0x0\n"));
        assert!(out.contains("  5 vio* at pci* dev -1 function -1 flags 0x0\n"));
        assert_eq!(cf[1].cf_driver.cd_name, b"vio");
        assert_eq!(cf[2].cf_driver.cd_name, b"pci");
        assert_eq!(cf[1].cf_parents, &[2]);
        assert_eq!(cf[5].cf_parents, &[2]);
        assert_eq!(cf[3].cf_parents, &[0]);
        assert_eq!(roots, [0, 4]);
        assert_eq!((cf[5].cf_unit.get(), cf[5].cf_starunit1), (2, 2));
        assert!(!cf[5].is_free() && cf[6].is_free());
        assert_eq!(hist, b"a 4 1 0 1\nq\n");
    }

    #[test]
    fn add_refuses_what_it_cannot_do() {
        let (out, cf, ..) = run("add vio\radd foo0\radd vio1\rq\rquit\r");
        assert!(out.contains("UKC> add vio\nDevice not complete number or * is missing\n"));
        assert!(out.contains("UKC> add foo0\nNo device of this type exists.\n"));
        assert!(cf[5].is_free());

        // Seven adds fill the seven usable slots.
        let keys = "add vio1\r4\r4\r".repeat(7) + "add vio1\rquit\r";
        let (out, cf, ..) = run(&keys);
        assert!(out.contains("No more space for new devices.\n"));
        assert!(!cf[11].is_free() && cf[12].is_free());
    }

    #[test]
    fn show_help_base_and_errors() {
        let (out, ..) =
            run("show\rshow irq\rshow irq 3\rbase 10\rfind 2\rbase 7\rlines\rfoo\rhelp\rquit\r");
        assert!(out.contains("UKC> show\nbus\nport\nirq\ndev\nfunction\n"));
        assert!(
            out.contains(
                "UKC> show irq\n  2 com0 at mainbus0 port 0x3f8 irq 4 flags 0x0\n  3 com1"
            )
        );
        assert!(out.contains("UKC> show irq 3\n  3 com1 at mainbus0 disable port 0x2f8 irq 3"));
        assert!(out.contains("UKC> find 2\n  2 com0 at mainbus0 port 1016 irq 4 flags 0x0\n"));
        assert!(out.contains("UKC> base 7\n8|10|16 expected\n"));
        assert!(out.contains("UKC> lines\nArgument expected\n"));
        assert!(out.contains("UKC> foo\nUnknown command, try help\n"));
        assert!(out.contains("command   args                description\n"));
        assert!(out.contains("disable   attr val|devno|dev  disable devices\n"));
        assert!(out.contains(&std::format!("?{}this message\n", " ".repeat(29))));
    }

    #[test]
    fn numbers_and_device_names_parse_as_in_c() {
        assert_eq!(userconf_number(b"0x10", INT_MAX), Ok(16));
        assert_eq!(userconf_number(b"010", INT_MAX), Ok(8));
        assert_eq!(userconf_number(b"-5 x", INT_MAX), Ok(-5));
        assert_eq!(userconf_number(b"08", INT_MAX), Ok(8)); // the C's `cc > base`
        assert_eq!(userconf_number(b"1g", INT_MAX), Err(-1));
        assert_eq!(userconf_number(b"-0x80000000", INT_MAX), Err(1));
        assert_eq!(userconf_number(b"", INT_MAX), Ok(0));
        assert_eq!(userconf_device(b"com0"), Some((3, 0, FSTATE_NOTFOUND)));
        assert_eq!(userconf_device(b"com12 "), Some((3, 12, FSTATE_NOTFOUND)));
        assert_eq!(userconf_device(b"vio*"), Some((3, 0, FSTATE_STAR)));
        assert_eq!(userconf_device(b"vio"), Some((3, 0, FSTATE_FOUND)));
        assert_eq!(userconf_device(b"vio x"), None);
    }
}
/* </TESTS> */
