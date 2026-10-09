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
/* </LICENSES> */

/* <CODE> */
//! The install sets (M14c): `base<rev>.tgz` and `comp<rev>.tgz` (`80` for 8.0), `bsd`,
//! `bsd.mp` (the same MULTIPROCESSOR kernel: no uniprocessor `bsd` is built for the sets),
//! `bsd.rd`, `SHA256` and `SHA256.sig`, in `target/install/<arch>/sets/`.
//!
//! OpenBSD makes a set by listing its files (`distrib/sets/lists/<set>/{mi,md.<arch>}` and,
//! for comp, `clang.<arch>`) and archiving those paths out of the installed tree
//! (`distrib/sets/maketars`: `pax -w -d`, `gzip`). The same here, from the tree this build
//! has: the programs, libraries and headers of `userland` (`target/userland/<arch>/root`) and
//! `comp` (`target/comp/<arch>/root`), the directories of `etc/mtree/4.4BSD.dist`, and the
//! files `etc/Makefile`'s `distribution-etc-root-var` installs (`etc.rs`-style table below).
//! A path a list names and this tree has not is not an error: it goes to
//! `MISSING-<set>.txt` with the reason (a table of prefixes), and the totals are printed; a
//! path the tree has and no list names stays out of the set (`EXTRA-<set>.txt`), as
//! `distrib/sets/checkflist` would flag it.
//!
//! The tar archives are written here (ustar, with the owner and group names and ids of
//! OpenBSD's `etc/master.passwd` and `etc/group`, hard links where the build linked them)
//! because the host's tar cannot make files owned by root and a device-less `/dev`.
//! `gzip(1)` is the host's (`-n`, so the archives are reproducible). `base` carries
//! `var/sysmerge/etc.tgz`, the `lists/etc` files, which `install.sub` extracts after base
//! (`etc` files stay the system administrator's: `sysmerge(8)`), and `etcsum`, their
//! checksums. `SHA256` lists the sets and kernels (`SHA256 (file) = hex`, sorted, as
//! `cksum -a sha256` prints), and `SHA256.sig` is signify's signature of it, embedding the
//! text (`signify -S -e`), made with the test key of `signify.rs`.
//!
//! What OpenBSD's lists have and this build does not is the point of the report; the
//! reasons: GPL parts of `gnu/` not in the clone (perl, cvs, texinfo, binutils' `as`, `gdb`,
//! `ld.bfd`, `objdump`, `readelf`...: the user's decision of 2026-10-05), man pages and
//! `/usr/share` data nobody built yet, and the programs `userland` does not build.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt as _;

use super::miniroot::{self, MtreeDir};
use super::ramdisk::{self, write_if_changed};
use super::*;

/// What one entry of a set is.
#[derive(Clone, Debug)]
enum Kind {
    Dir,
    /// A file of the build, by host path.
    File(PathBuf),
    /// A file made here.
    Text(Vec<u8>),
    Symlink(String),
}

/// One path of the tree: the entry and its owner.
#[derive(Clone, Debug)]
struct Ent {
    kind: Kind,
    mode: u32,
    uname: String,
    gname: String,
}

/// `./usr/bin/cc`-style paths to entries.
type Tree = BTreeMap<String, Ent>;

fn ent(kind: Kind, mode: u32, uname: &str, gname: &str) -> Ent {
    Ent {
        kind,
        mode,
        uname: uname.to_string(),
        gname: gname.to_string(),
    }
}

/// Users and groups of OpenBSD's `etc/master.passwd` and `etc/group`: name → id.
struct Ids {
    users: BTreeMap<String, u32>,
    groups: BTreeMap<String, u32>,
}

impl Ids {
    fn read(src: &Path) -> Result<Ids> {
        let mut users = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let pw = fs::read_to_string(src.join("etc/master.passwd"))
            .map_err(|e| format!("etc/master.passwd: {e}"))?;
        for l in pw.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = l.split(':').collect();
            if let (Some(n), Some(Ok(id))) = (f.first(), f.get(2).map(|s| s.parse::<u32>())) {
                users.insert((*n).to_string(), id);
            }
        }
        let gr =
            fs::read_to_string(src.join("etc/group")).map_err(|e| format!("etc/group: {e}"))?;
        for l in gr.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = l.split(':').collect();
            if let (Some(n), Some(Ok(id))) = (f.first(), f.get(2).map(|s| s.parse::<u32>())) {
                groups.insert((*n).to_string(), id);
            }
        }
        Ok(Ids { users, groups })
    }

    fn uid(&self, name: &str) -> u32 {
        self.users.get(name).copied().unwrap_or(0)
    }

    fn gid(&self, name: &str) -> u32 {
        self.groups.get(name).copied().unwrap_or(0)
    }
}

/// The reasons a path of a list is missing, by prefix (first match): the report groups by
/// them. The last row matches everything.
const REASONS: &[(&str, &str)] = &[
    (
        "./usr/share/man",
        "man pages are not built (no mandoc; the man set)",
    ),
    (
        "./usr/libdata/perl5",
        "perl is GPL/Artistic, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/perl",
        "perl is not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/cvs",
        "cvs is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/makeinfo",
        "texinfo is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/info",
        "texinfo is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/install-info",
        "texinfo is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/texi2",
        "texinfo is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/share/info",
        "texinfo is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/as",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/gdb",
        "gdb is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/ld.bfd",
        "GNU ld is GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/objdump",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/readelf",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/addr2line",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/c++filt",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/bin/gprof",
        "GNU binutils are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    (
        "./usr/libdata/ldscripts",
        "GNU ld's scripts are GPL, not in the clone (the user's decision of 2026-10-05)",
    ),
    ("./usr/share/doc", "documents are not built"),
    (
        "./usr/share/terminfo",
        "terminfo is made by tic(1) from share/termtypes, not built yet",
    ),
    (
        "./usr/share/zoneinfo",
        "zoneinfo is made by zic(8) from share/zoneinfo, not built yet",
    ),
    ("./usr/share/locale", "locale data is not built yet"),
    ("./usr/share/misc", "usr/share/misc is not built yet"),
    ("./usr/share/dict", "dictionaries are not built yet"),
    ("./usr/share", "usr/share data is not built yet"),
    (
        "./usr/libdata",
        "usr/libdata is not built yet (pkgconfig, lint)",
    ),
    (
        "./usr/local",
        "usr/local is the ports tree's directories, empty here",
    ),
    (
        "./etc/firmware",
        "firmware is fetched by fw_update(8), not built",
    ),
    (
        "./var/www",
        "httpd's files are not built (usr.sbin/httpd is not in PROGRAMS)",
    ),
    ("./var/yp", "YP is not built"),
    ("./var/nsd", "nsd is not built"),
    ("./var/unbound", "unbound is not built"),
    (
        "./usr/mdec",
        "the BIOS boot programs are not ported (boot(8) is efiboot only, M14)",
    ),
    (
        "./usr/lib/lib",
        "this library is not built (see userland.rs LIBRARIES)",
    ),
    (
        "./usr/bin",
        "this program is not built (see userland.rs PROGRAMS)",
    ),
    (
        "./usr/sbin",
        "this program is not built (see userland.rs PROGRAMS)",
    ),
    (
        "./usr/libexec",
        "this program is not built (see userland.rs PROGRAMS)",
    ),
    (
        "./sbin",
        "this program is not built (see userland.rs PROGRAMS)",
    ),
    (
        "./bin",
        "this program is not built (see userland.rs PROGRAMS)",
    ),
    ("", "not built by this build yet"),
];

fn reason(path: &str) -> &'static str {
    REASONS
        .iter()
        .find(|(p, _)| path.starts_with(p))
        .map_or("not built", |(_, why)| why)
}

/// The paths of a `lists/<set>/<file>` list (`./usr/bin/cc`), without blanks and comments.
fn read_list(src: &Path, files: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for f in files {
        let p = src.join("distrib/sets/lists").join(f);
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        out.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| l.starts_with("./") || *l == ".")
                .map(str::to_string),
        );
    }
    Ok(out)
}

/// The staged files of a `root/`: relative path (`./usr/bin/cc`) → host path, symlinks kept
/// apart.
fn walk(
    dir: &Path,
    rel: &str,
    files: &mut BTreeMap<String, PathBuf>,
    links: &mut BTreeMap<String, String>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = format!("{rel}/{}", e.file_name().to_string_lossy());
        let md = fs::symlink_metadata(e.path())?;
        if md.is_dir() {
            walk(&e.path(), &path, files, links)?;
        } else if md.file_type().is_symlink() {
            links.insert(
                path,
                fs::read_link(e.path())?.to_string_lossy().into_owned(),
            );
        } else {
            files.insert(path, e.path());
        }
    }
    Ok(())
}

/// `userland`'s ownership tables (`mode uid gid path`): path with a leading `.`.
fn read_owner_table(path: &Path) -> BTreeMap<String, (u32, u32, u32)> {
    let mut map = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(path) {
        for line in text.lines() {
            let mut w = line.split_whitespace();
            if let (Some(m), Some(u), Some(g), Some(p)) = (w.next(), w.next(), w.next(), w.next())
                && let (Ok(m), Ok(u), Ok(g)) = (
                    u32::from_str_radix(m, 8),
                    u.parse::<u32>(),
                    g.parse::<u32>(),
                )
            {
                map.insert(format!(".{p}"), (m, u, g));
            }
        }
    }
    map
}

/// The names for the ids of the ownership tables (those of this build's `ramdisk.rs`,
/// which are OpenBSD's: `root` 0, `auth` 11, ...): id → name via OpenBSD's own files.
fn names_for(ids: &Ids, id: u32, group: bool) -> String {
    let map = if group { &ids.groups } else { &ids.users };
    map.iter()
        .find(|(_, v)| **v == id)
        .map_or_else(|| id.to_string(), |(n, _)| n.clone())
}

/// What `etc/Makefile`'s `distribution-etc-root-var` installs, as (target, source relative
/// to the sources, mode, owner, group). Generated files (`ttys`, `fbtab`, the examples'
/// `sysctl.conf`, the password databases, the empty logs) are `etc_generated`.
fn etc_files(arch: &str) -> Vec<(String, String, u32, &'static str, &'static str)> {
    let mut v: Vec<(String, String, u32, &'static str, &'static str)> = Vec::new();
    let mut add = |to: &str, from: &str, mode: u32, u: &'static str, g: &'static str| {
        v.push((to.to_string(), from.to_string(), mode, u, g));
    };
    // MUTABLE (644 root:wheel).
    for f in [
        "changelist",
        "daily",
        "ftpusers",
        "gettytab",
        "group",
        "ksh.kshrc",
        "locate.rc",
        "mailer.conf",
        "moduli",
        "monthly",
        "netstart",
        "newsyslog.conf",
        "ntpd.conf",
        "pf.os",
        "protocols",
        "rc",
        "rc.conf",
        "rpc",
        "services",
        "shells",
        "syslog.conf",
        "weekly",
    ] {
        add(
            &format!("./etc/{f}"),
            &format!("etc/{f}"),
            0o644,
            "root",
            "wheel",
        );
    }
    for f in ["disktab", "login.conf"] {
        add(
            &format!("./etc/{f}"),
            &format!("etc/etc.{arch}/{f}"),
            0o644,
            "root",
            "wheel",
        );
    }
    add("./etc/motd", "etc/motd", 0o664, "root", "operator");
    add("./etc/pf.conf", "etc/pf.conf", 0o600, "root", "wheel");
    add(
        "./etc/master.passwd",
        "etc/master.passwd",
        0o600,
        "root",
        "wheel",
    );
    add(
        "./var/cron/tabs/root",
        "etc/crontab",
        0o600,
        "root",
        "crontab",
    );
    add(
        "./var/nsd/etc/nsd.conf",
        "etc/nsd.conf",
        0o640,
        "root",
        "_nsd",
    );
    add(
        "./var/unbound/etc/unbound.conf",
        "etc/unbound.conf",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./dev/MAKEDEV",
        &format!("etc/etc.{arch}/MAKEDEV"),
        0o555,
        "root",
        "wheel",
    );
    for f in [
        "dot.cshrc",
        "dot.login",
        "dot.profile",
        "dot.Xdefaults",
        "dot.cvsrc",
    ] {
        let name = f.replacen("dot", "", 1);
        add(
            &format!("./root/{name}"),
            &format!("etc/root/{f}"),
            0o644,
            "root",
            "wheel",
        );
        add(
            &format!("./etc/skel/{name}"),
            &format!("etc/skel/{f}"),
            0o644,
            "root",
            "wheel",
        );
    }
    add(
        "./etc/skel/.mailrc",
        "etc/skel/dot.mailrc",
        0o644,
        "root",
        "wheel",
    );
    add("./.cshrc", "etc/root/dot.cshrc", 0o644, "root", "wheel");
    add("./.profile", "etc/root/dot.profile", 0o644, "root", "wheel");
    add(
        "./var/mail/root",
        "etc/root/root.mail",
        0o600,
        "root",
        "wheel",
    );
    add(
        "./etc/amd/master.sample",
        "etc/amd/master.sample",
        0o644,
        "root",
        "wheel",
    );
    for f in [
        "chap-secrets",
        "options",
        "options.sample",
        "chatscript.sample",
        "pap-secrets",
    ] {
        add(
            &format!("./etc/ppp/{f}"),
            &format!("etc/ppp/{f}"),
            0o600,
            "root",
            "wheel",
        );
    }
    for f in ["afrinic", "apnic", "arin", "lacnic", "ripe"] {
        add(
            &format!("./etc/rpki/{f}.tal"),
            &format!("etc/rpki/{f}.tal"),
            0o644,
            "root",
            "wheel",
        );
        add(
            &format!("./etc/rpki/{f}.constraints"),
            &format!("etc/rpki/{f}.constraints"),
            0o644,
            "root",
            "wheel",
        );
    }
    for f in [
        "acme-client.conf",
        "chio.conf",
        "dhcpd.conf",
        "exports",
        "httpd.conf",
        "ifstated.conf",
        "inetd.conf",
        "man.conf",
        "mixerctl.conf",
        "mrouted.conf",
        "ntpd.conf",
        "printcap",
        "rad.conf",
        "rbootd.conf",
        "remote",
        "sensorsd.conf",
        "wsconsctl.conf",
    ] {
        add(
            &format!("./etc/examples/{f}"),
            &format!("etc/examples/{f}"),
            0o644,
            "root",
            "wheel",
        );
    }
    for f in [
        "bgpd.conf",
        "doas.conf",
        "dvmrpd.conf",
        "eigrpd.conf",
        "hostapd.conf",
        "iked.conf",
        "ipsec.conf",
        "ldapd.conf",
        "ldpd.conf",
        "login_ldap.conf",
        "ospf6d.conf",
        "ospfd.conf",
        "pf.conf",
        "radiusd.conf",
        "rc.local",
        "rc.securelevel",
        "rc.shutdown",
        "relayd.conf",
        "ripd.conf",
        "sasyncd.conf",
        "snmpd.conf",
        "vm.conf",
        "ypldap.conf",
    ] {
        add(
            &format!("./etc/examples/{f}"),
            &format!("etc/examples/{f}"),
            0o600,
            "root",
            "wheel",
        );
    }
    add(
        "./etc/mail/aliases",
        "etc/mail/aliases",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/mail/smtpd.conf",
        "etc/mail/smtpd.conf",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/mtree/4.4BSD.dist",
        "etc/mtree/4.4BSD.dist",
        0o444,
        "root",
        "wheel",
    );
    add(
        "./etc/mtree/BSD.x11.dist",
        "etc/mtree/BSD.x11.dist",
        0o444,
        "root",
        "wheel",
    );
    add(
        "./etc/mtree/special",
        "etc/mtree/special",
        0o600,
        "root",
        "wheel",
    );
    add("./var/crash/minfree", "etc/minfree", 0o644, "root", "wheel");
    add(
        "./etc/ssh/ssh_config",
        "usr.bin/ssh/ssh_config",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/ssh/sshd_config",
        "usr.bin/ssh/sshd_config",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/ssl/openssl.cnf",
        "lib/libcrypto/openssl.cnf",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/ssl/x509v3.cnf",
        "lib/libcrypto/x509v3.cnf",
        0o644,
        "root",
        "wheel",
    );
    add(
        "./etc/ssl/cert.pem",
        "lib/libcrypto/cert.pem",
        0o644,
        "root",
        "wheel",
    );
    v
}

/// Files `etc/Makefile` makes empty with `install ... /dev/null`: (path, mode, owner, group).
const EMPTY_FILES: &[(&str, u32, &str, &str)] = &[
    ("./etc/dumpdates", 0o664, "root", "operator"),
    ("./etc/skel/.ssh/authorized_keys", 0o600, "root", "wheel"),
    ("./root/.ssh/authorized_keys", 0o600, "root", "wheel"),
    ("./var/account/acct", 0o644, "root", "wheel"),
    ("./var/cron/at.deny", 0o660, "root", "crontab"),
    ("./var/cron/cron.deny", 0o660, "root", "crontab"),
    ("./var/cron/log", 0o600, "root", "wheel"),
    ("./var/db/locate.database", 0o444, "root", "wheel"),
    (
        "./var/db/rpki-client/openbgpd",
        0o644,
        "_rpki-client",
        "wheel",
    ),
    ("./var/log/authlog", 0o640, "root", "wheel"),
    ("./var/log/daemon", 0o640, "root", "wheel"),
    ("./var/log/failedlogin", 0o600, "root", "wheel"),
    ("./var/log/ftpd", 0o640, "root", "wheel"),
    ("./var/log/lastlog", 0o644, "root", "wheel"),
    ("./var/log/lpd-errs", 0o640, "root", "wheel"),
    ("./var/log/maillog", 0o640, "root", "wheel"),
    ("./var/log/messages", 0o644, "root", "wheel"),
    ("./var/log/secure", 0o600, "root", "wheel"),
    ("./var/log/wtmp", 0o644, "root", "wheel"),
    ("./var/log/xferlog", 0o640, "root", "wheel"),
];

/// The rc.d daemon scripts (`RCDAEMONS` of `etc/Makefile`).
const RCDAEMONS: &str = "amd apmd bgpd bgplgd bootparamd bpflogd cron dhcpd dhcpleased \
    dhcp6leased dhcrelay dhcrelay6 dvmrpd eigrpd ftpd ftpproxy ftpproxy6 hostapd hotplugd httpd \
    identd ifstated iked inetd isakmpd iscsid ldapd ldattach ldomd ldpd lldpd lockd lpd mopd \
    mountd mrouted nfsd npppd nsd ntpd ospf6d ospfd pflogd portmap rad radiusd rarpd rbootd \
    relayd resolvd ripd route6d rtrd sasyncd sensorsd slowcgi slaacd smtpd sndiod snmpd spamd \
    spamlogd sshd statd syslogd tftpd tftpproxy unbound unwind vmd watchdogd wsmoused xenodm \
    ypbind ypldap ypserv";

/// Runs `sh ttys.pty`, whose output `etc/Makefile` appends to `etc.<arch>/ttys`.
fn ttys(src: &Path, arch: &str) -> Result<Vec<u8>> {
    let mut text = fs::read(src.join(format!("etc/etc.{arch}/ttys")))
        .map_err(|e| format!("etc/etc.{arch}/ttys: {e}"))?;
    let pty = Command::new("/bin/sh")
        .arg(src.join("etc/ttys.pty"))
        .output()
        .map_err(|e| format!("sh ttys.pty: {e}"))?;
    if !pty.status.success() {
        return Err("sh ttys.pty failed".into());
    }
    text.extend_from_slice(&pty.stdout);
    Ok(text)
}

/// `usr/mdec/biosboot`'s placeholder (see `gather`): an `ELFCLASS32` `EM_386` executable
/// with one `PT_LOAD` segment, a 512-byte sector of text at offset 512, what installboot's
/// `loadproto` reads.
fn biosboot_placeholder() -> Vec<u8> {
    const SECTOR: usize = 512;
    let mut f = vec![0u8; 2 * SECTOR];
    // Elf32_Ehdr
    f[..16].copy_from_slice(&[0x7f, b'E', b'L', b'F', 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    f[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type: ET_EXEC
    f[18..20].copy_from_slice(&3u16.to_le_bytes()); // e_machine: EM_386
    f[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    f[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
    f[40..42].copy_from_slice(&52u16.to_le_bytes()); // e_ehsize
    f[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
    f[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    f[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
    // Elf32_Phdr
    let ph = &mut f[52..84];
    ph[0..4].copy_from_slice(&1u32.to_le_bytes()); // p_type: PT_LOAD
    ph[4..8].copy_from_slice(&(SECTOR as u32).to_le_bytes()); // p_offset
    ph[16..20].copy_from_slice(&(SECTOR as u32).to_le_bytes()); // p_filesz
    ph[20..24].copy_from_slice(&(SECTOR as u32).to_le_bytes()); // p_memsz
    ph[24..28].copy_from_slice(&5u32.to_le_bytes()); // p_flags: PF_R | PF_X
    ph[28..32].copy_from_slice(&4u32.to_le_bytes()); // p_align
    let text = b"EmiBSD placeholder: biosboot(8) is not built (UEFI only, M14)\n";
    f[SECTOR..SECTOR + text.len()].copy_from_slice(text);
    f
}

/// The tree of everything the sets can take from: the directories of
/// `etc/mtree/4.4BSD.dist`, the staged `userland` and `comp` roots, and the `etc`
/// distribution.
fn gather(
    ctx: &Ctx<'_>,
    arch: crate::boot::Arch,
    pwd_mkdb: &Path,
    scratch: &Path,
) -> Result<(Tree, Ids)> {
    let src = &ctx.src;
    let ids = Ids::read(src)?;
    let mut tree = Tree::new();

    // Directories.
    let mtree = fs::read_to_string(src.join("etc/mtree/4.4BSD.dist"))
        .map_err(|e| format!("etc/mtree/4.4BSD.dist: {e}"))?;
    for MtreeDir {
        path,
        mode,
        uname,
        gname,
    } in miniroot::parse_mtree(&mtree)
    {
        tree.insert(format!(".{path}"), ent(Kind::Dir, mode, &uname, &gname));
    }

    // The programs and libraries of `userland` and `comp`.
    let comp_root = ctx.root.join("target/comp").join(arch.name()).join("root");
    let mut tables = vec![(
        ctx.out.join("root"),
        read_owner_table(&ctx.out.join("host/owners.txt")),
    )];
    if comp_root.is_dir() {
        tables.push((
            comp_root.clone(),
            read_owner_table(
                &ctx.root
                    .join("target/comp")
                    .join(arch.name())
                    .join("owners.txt"),
            ),
        ));
    }
    for (root, owners) in tables {
        let mut files = BTreeMap::new();
        let mut links = BTreeMap::new();
        walk(&root, ".", &mut files, &mut links)?;
        for (path, host) in files {
            if tree
                .get(&path)
                .is_some_and(|e| !matches!(e.kind, Kind::Dir))
            {
                continue;
            }
            let md = fs::metadata(&host)?;
            let default = if md.mode() & 0o111 != 0 { 0o555 } else { 0o444 };
            let (mode, uid, gid) =
                owners
                    .get(&path)
                    .copied()
                    .unwrap_or((default, 0, ids.gid("bin")));
            tree.insert(
                path,
                ent(
                    Kind::File(host),
                    mode,
                    &names_for(&ids, uid, false),
                    &names_for(&ids, gid, true),
                ),
            );
        }
        for (path, target) in links {
            tree.entry(path)
                .or_insert_with(|| ent(Kind::Symlink(target), 0o755, "root", "wheel"));
        }
    }

    // `usr/mdec`: efiboot, which installboot(8) copies onto the EFI system partition, and
    // the 512-byte MBR template of fdisk(8) (no boot code: UEFI boots us).
    let loader = match arch {
        crate::boot::Arch::Amd64 => ("BOOTX64.EFI", "amd64"),
        crate::boot::Arch::Arm64 => ("BOOTAA64.EFI", "arm64"),
    };
    let built = ctx
        .root
        .join("target/efiboot")
        .join(loader.1)
        .join(loader.0);
    let loader_bytes = fs::read(&built).map_err(|e| {
        format!(
            "sets: {}: {e} (run `just efiboot-{}`)",
            built.display(),
            loader.1
        )
    })?;
    tree.insert(
        format!("./usr/mdec/{}", loader.0),
        ent(Kind::Text(loader_bytes), 0o444, "root", "wheel"),
    );
    if matches!(arch, crate::boot::Arch::Amd64) {
        // installboot(8) copies BOOTIA32.EFI too and fails without it; there is no IA32 loader
        // in this port, so it is one zero sector.
        tree.insert(
            "./usr/mdec/BOOTIA32.EFI".to_string(),
            ent(Kind::Text(vec![0u8; 512]), 0o444, "root", "wheel"),
        );
        // installboot(8) loads the BIOS partition boot record (`md_loadboot`: an ELF with
        // one load segment) before it looks at the disk, and on a GPT disk with an EFI
        // system partition never writes it. The BIOS boot programs are not built
        // (`stand/biosboot`'s Makefile wants GNU as, `-no-integrated-as`), so it is a
        // visible placeholder: an i386 ELF whose one segment is a sector of text saying so.
        // On an MBR disk installboot then fails to find biosboot's symbols, as it should.
        tree.insert(
            "./usr/mdec/biosboot".to_string(),
            ent(Kind::Text(biosboot_placeholder()), 0o444, "root", "wheel"),
        );
    }
    tree.insert(
        "./usr/mdec/mbr".to_string(),
        ent(Kind::Text(miniroot::mbr_stub()), 0o444, "root", "wheel"),
    );

    // The `etc` distribution.
    for (to, from, mode, u, g) in etc_files(arch.name()) {
        let host = src.join(&from);
        if host.is_file() {
            tree.insert(to, ent(Kind::File(host), mode, u, g));
        }
    }
    for rc in RCDAEMONS.split_whitespace() {
        let host = src.join("etc/rc.d").join(rc);
        if host.is_file() {
            tree.insert(
                format!("./etc/rc.d/{rc}"),
                ent(Kind::File(host), 0o555, "root", "wheel"),
            );
        }
    }
    tree.insert(
        "./etc/rc.d/rc.subr".to_string(),
        ent(
            Kind::File(src.join("etc/rc.d/rc.subr")),
            0o644,
            "root",
            "wheel",
        ),
    );
    for entry in fs::read_dir(src.join("etc/signify")).map_err(|e| format!("etc/signify: {e}"))? {
        let p = entry?.path();
        if p.extension().is_some_and(|e| e == "pub") {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            tree.insert(
                format!("./etc/signify/{name}"),
                ent(Kind::File(p), 0o644, "root", "wheel"),
            );
        }
    }
    for (path, mode, u, g) in EMPTY_FILES {
        tree.insert(
            (*path).to_string(),
            ent(Kind::Text(Vec::new()), *mode, u, g),
        );
    }
    tree.insert(
        "./etc/ttys".to_string(),
        ent(Kind::Text(ttys(src, arch.name())?), 0o644, "root", "wheel"),
    );
    let mut fbtab = fs::read(src.join("etc/fbtab.head"))?;
    fbtab.extend(fs::read(
        src.join(format!("etc/etc.{}/fbtab", arch.name())),
    )?);
    fbtab.extend(fs::read(src.join("etc/fbtab.tail"))?);
    tree.insert(
        "./etc/fbtab".to_string(),
        ent(Kind::Text(fbtab), 0o644, "root", "wheel"),
    );
    let mut sysctl = fs::read(src.join("etc/examples/sysctl.conf"))?;
    sysctl.extend(fs::read(
        src.join(format!("etc/etc.{}/sysctl.conf", arch.name())),
    )?);
    tree.insert(
        "./etc/examples/sysctl.conf".to_string(),
        ent(Kind::Text(sysctl), 0o644, "root", "wheel"),
    );
    tree.insert(
        "./etc/localtime".to_string(),
        ent(
            Kind::Symlink("/usr/share/zoneinfo/Canada/Mountain".to_string()),
            0o755,
            "root",
            "wheel",
        ),
    );
    tree.insert(
        "./etc/rmt".to_string(),
        ent(
            Kind::Symlink("/usr/sbin/rmt".to_string()),
            0o755,
            "root",
            "wheel",
        ),
    );
    tree.insert(
        "./var/tmp".to_string(),
        ent(Kind::Symlink("../tmp".to_string()), 0o755, "root", "wheel"),
    );

    // /usr/share/zoneinfo, by OpenBSD's zic (`zoneinfo.rs`).
    let zdir = super::zoneinfo::build(ctx)?;
    let mut zfiles = BTreeMap::new();
    let mut zlinks = BTreeMap::new();
    walk(&zdir, "./usr/share/zoneinfo", &mut zfiles, &mut zlinks)?;
    for (path, host) in zfiles {
        tree.insert(path, ent(Kind::File(host), 0o444, "root", "wheel"));
    }
    for (path, target) in zlinks {
        tree.insert(path, ent(Kind::Symlink(target), 0o755, "root", "wheel"));
    }

    // The password databases, by OpenBSD's pwd_mkdb (`pwd_mkdb -p -d DESTDIR/etc`).
    let pw = scratch.join("pw");
    let _ = fs::remove_dir_all(&pw);
    fs::create_dir_all(&pw).map_err(|e| format!("{}: {e}", pw.display()))?;
    fs::copy(src.join("etc/master.passwd"), pw.join("master.passwd"))?;
    super::passwd::make_databases(pwd_mkdb, &pw)?;
    for (name, mode, g) in [
        ("passwd", 0o644, "wheel"),
        ("pwd.db", 0o644, "wheel"),
        ("spwd.db", 0o640, "_shadow"),
    ] {
        tree.insert(
            format!("./etc/{name}"),
            ent(Kind::File(pw.join(name)), mode, "root", g),
        );
    }
    Ok((tree, ids))
}

/// A ustar header for one entry.
#[allow(clippy::too_many_arguments)] // the fields of a ustar header, one each
fn header(
    name: &str,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    typeflag: u8,
    link: &str,
    uname: &str,
    gname: &str,
    mtime: u64,
) -> Result<[u8; 512]> {
    // A long name is split at a slash into `prefix` (155) and `name` (100).
    let (prefix, base) = if name.len() <= 100 {
        ("", name)
    } else {
        let at = name
            .char_indices()
            .filter(|(i, c)| *c == '/' && *i <= 155 && name.len() - i - 1 <= 100)
            .map(|(i, _)| i)
            .next_back()
            .ok_or_else(|| format!("tar: name too long: {name}"))?;
        (&name[..at], &name[at + 1..])
    };
    if link.len() > 100 {
        return Err(format!("tar: link target too long: {name} -> {link}").into());
    }
    let mut h = [0u8; 512];
    let put = |h: &mut [u8; 512], at: usize, len: usize, text: &str| {
        let b = text.as_bytes();
        h[at..at + b.len().min(len)].copy_from_slice(&b[..b.len().min(len)]);
    };
    put(&mut h, 0, 100, base);
    put(&mut h, 100, 8, &format!("{:07o}", mode & 0o7777));
    put(&mut h, 108, 8, &format!("{:07o}", uid));
    put(&mut h, 116, 8, &format!("{:07o}", gid));
    put(&mut h, 124, 12, &format!("{size:011o}"));
    put(&mut h, 136, 12, &format!("{mtime:011o}"));
    h[156] = typeflag;
    put(&mut h, 157, 100, link);
    put(&mut h, 257, 6, "ustar");
    h[263] = b'0';
    h[264] = b'0';
    put(&mut h, 265, 32, uname);
    put(&mut h, 297, 32, gname);
    put(&mut h, 345, 155, prefix);
    // The checksum is taken with the field itself as eight spaces, and stored as six octal
    // digits, a NUL and a space.
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..154].copy_from_slice(format!("{sum:06o}").as_bytes());
    h[154] = 0;
    h[155] = b' ';
    Ok(h)
}

/// Writes `paths` of `tree` to the ustar archive `tar` (directories first as the sorted
/// names give them, hard links for files of one inode).
fn write_tar(tree: &Tree, ids: &Ids, paths: &[String], tar: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(
        fs::File::create(tar).map_err(|e| format!("{}: {e}", tar.display()))?,
    );
    let mut inodes: BTreeMap<(u64, u64), String> = BTreeMap::new();
    for p in paths {
        let Some(e) = tree.get(p) else { continue };
        let uid = ids.uid(&e.uname);
        let gid = ids.gid(&e.gname);
        let name = p.as_str();
        match &e.kind {
            Kind::Dir => {
                let n = format!("{name}/");
                out.write_all(&header(
                    &n, e.mode, uid, gid, 0, b'5', "", &e.uname, &e.gname, TIMESTAMP,
                )?)?;
            }
            Kind::Symlink(target) => {
                out.write_all(&header(
                    name, e.mode, uid, gid, 0, b'2', target, &e.uname, &e.gname, TIMESTAMP,
                )?)?;
            }
            Kind::Text(bytes) => {
                out.write_all(&header(
                    name,
                    e.mode,
                    uid,
                    gid,
                    bytes.len() as u64,
                    b'0',
                    "",
                    &e.uname,
                    &e.gname,
                    TIMESTAMP,
                )?)?;
                out.write_all(bytes)?;
                out.write_all(&vec![0u8; (512 - bytes.len() % 512) % 512])?;
            }
            Kind::File(host) => {
                let md = fs::metadata(host).map_err(|e| format!("{}: {e}", host.display()))?;
                let key = (md.dev(), md.ino());
                if let Some(first) = inodes.get(&key) {
                    out.write_all(&header(
                        name, e.mode, uid, gid, 0, b'1', first, &e.uname, &e.gname, TIMESTAMP,
                    )?)?;
                    continue;
                }
                inodes.insert(key, name.to_string());
                let size = md.len();
                out.write_all(&header(
                    name, e.mode, uid, gid, size, b'0', "", &e.uname, &e.gname, TIMESTAMP,
                )?)?;
                let mut f = fs::File::open(host).map_err(|e| format!("{}: {e}", host.display()))?;
                let copied = std::io::copy(&mut f, &mut out)?;
                if copied != size {
                    return Err(format!("{}: changed while archiving", host.display()).into());
                }
                out.write_all(&vec![0u8; (512 - (size % 512) as usize) % 512])?;
            }
        }
    }
    out.write_all(&[0u8; 1024])?;
    out.flush()?;
    Ok(())
}

/// The mtime of every archive entry: `ramdisk.rs`'s fixed timestamp, so the sets are
/// reproducible.
const TIMESTAMP: u64 = ramdisk::TIMESTAMP;

/// `gzip -n -c < TAR > TGZ` with the host's gzip.
fn gzip(tar: &Path, tgz: &Path) -> Result<()> {
    let input = fs::File::open(tar).map_err(|e| format!("{}: {e}", tar.display()))?;
    let output = fs::File::create(tgz).map_err(|e| format!("{}: {e}", tgz.display()))?;
    let status = Command::new("gzip")
        .args(["-n", "-c"])
        .stdin(input)
        .stdout(output)
        .status()
        .map_err(|e| format!("gzip: {e}"))?;
    if !status.success() {
        return Err(format!("gzip {} failed: {status}", tar.display()).into());
    }
    Ok(())
}

/// `SHA256 (name) = hex` of `file`, by the host's `shasum`.
fn sha256_line(file: &Path, name: &str) -> Result<String> {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|e| format!("shasum: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let hex = text
        .split_whitespace()
        .next()
        .filter(|h| h.len() == 64)
        .ok_or_else(|| format!("shasum {}: no digest", file.display()))?;
    Ok(format!("SHA256 ({name}) = {hex}\n"))
}

/// What a set's file made and found missing.
struct SetReport {
    name: String,
    files: usize,
    missing: Vec<String>,
    extra: Vec<String>,
    bytes: u64,
}

/// Archives the paths of the lists that `tree` has: returns the report. `claimed` collects
/// every path named by a list of the sets (for the EXTRA report).
fn make_set(
    tree: &Tree,
    ids: &Ids,
    list: &[String],
    tar: &Path,
    claimed: &mut BTreeSet<String>,
) -> Result<SetReport> {
    let listed: BTreeSet<&String> = list.iter().collect();
    // A listed name is a directory when another listed name is below it.
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut missing = Vec::new();
    let mut implicit: Vec<String> = Vec::new();
    for p in &listed {
        claimed.insert((*p).clone());
        if tree.contains_key(*p) {
            paths.insert((*p).clone());
        } else {
            let dir_prefix = format!("{p}/");
            if listed.iter().any(|q| q.starts_with(&dir_prefix)) {
                implicit.push((*p).clone());
            } else {
                missing.push((*p).clone());
            }
        }
    }
    // Directories above included files that the lists do not name are the list's job; the
    // implicit ones (named, but not in `etc/mtree`) are made here.
    let mut local = tree.clone();
    for d in &implicit {
        local
            .entry(d.clone())
            .or_insert_with(|| ent(Kind::Dir, 0o755, "root", "wheel"));
        paths.insert(d.clone());
    }
    let mut ordered: Vec<String> = paths.into_iter().collect();
    // maketars: `./usr/lib/lib*` first, then the rest, each sorted.
    ordered.sort_by_key(|p| (!p.starts_with("./usr/lib/lib"), p.clone()));
    // Drop directories no included file lives under, unless the lists name them (they do).
    write_tar(&local, ids, &ordered, tar)?;
    let files = ordered.len();
    let bytes = fs::metadata(tar).map(|m| m.len()).unwrap_or(0);
    Ok(SetReport {
        name: String::new(),
        files,
        missing,
        extra: Vec::new(),
        bytes,
    })
}

/// The public half of the test key (made on first use), for the miniroot.
pub(crate) fn test_pubkey(ctx: &Ctx<'_>) -> Result<PathBuf> {
    let rev = miniroot::os_rev(&ctx.src)?;
    let signify = super::signify::build_host_signify(ctx)?;
    Ok(super::signify::test_keys(ctx, &signify, &rev)?.0)
}

/// Where the sets of `arch` go.
pub(crate) fn sets_dir(root: &Path, arch: crate::boot::Arch) -> PathBuf {
    root.join("target/install").join(arch.name()).join("sets")
}

/// Builds the sets (module docs). `bsd` is the kernel to ship as `bsd`, `bsd_rd` the install
/// kernel, when they exist.
pub(crate) fn build(
    ctx: &Ctx<'_>,
    arch: crate::boot::Arch,
    bsd: Option<&Path>,
    bsd_rd: Option<&Path>,
) -> Result<()> {
    let src = &ctx.src;
    let rev = miniroot::os_rev(src)?;
    let dir = sets_dir(&ctx.root, arch);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let scratch = ctx.out.join("host/sets");
    fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let pwd_mkdb = super::passwd::build_pwd_mkdb(ctx)?;
    let signify = super::signify::build_host_signify(ctx)?;
    let (pubkey, secret) = super::signify::test_keys(ctx, &signify, &rev)?;

    let (mut tree, ids) = gather(ctx, arch, &pwd_mkdb, &scratch)?;

    // `var/sysmerge/etc.tgz` and `etcsum`: the `lists/etc` files of the tree.
    let etc_list = read_list(
        src,
        &["etc/mi".to_string(), format!("etc/md.{}", arch.name())],
    )?;
    let etc_present: Vec<String> = etc_list
        .iter()
        .filter(|p| tree.contains_key(*p))
        .cloned()
        .collect();
    let mut sums = String::new();
    for p in &etc_present {
        if let Some(Ent {
            kind: Kind::File(host),
            ..
        }) = tree.get(p)
        {
            let line = sha256_line(host, p)?;
            sums.push_str(&line);
        } else if let Some(Ent {
            kind: Kind::Text(bytes),
            ..
        }) = tree.get(p)
        {
            let tmp = scratch.join("etcsum-text");
            fs::write(&tmp, bytes)?;
            sums.push_str(&sha256_line(&tmp, p)?);
        }
    }
    tree.insert(
        "./var/sysmerge/etcsum".to_string(),
        ent(Kind::Text(sums.into_bytes()), 0o644, "root", "wheel"),
    );
    let etc_tar = scratch.join("etc.tar");
    let mut etc_paths: Vec<String> = etc_present.clone();
    etc_paths.sort();
    write_tar(&tree, &ids, &etc_paths, &etc_tar)?;
    let etc_tgz = scratch.join("etc.tgz");
    gzip(&etc_tar, &etc_tgz)?;
    tree.insert(
        "./var/sysmerge/etc.tgz".to_string(),
        ent(Kind::File(etc_tgz), 0o644, "root", "wheel"),
    );

    // The etc files are in the set through `etc.tgz`; they are claimed by `lists/etc`.
    let mut claimed: BTreeSet<String> = etc_list.iter().cloned().collect();
    let mut reports = Vec::new();
    let a = arch.name();
    let sets: [(&str, Vec<String>); 2] = [
        ("base", vec!["base/mi".to_string(), format!("base/md.{a}")]),
        (
            "comp",
            vec![
                "comp/mi".to_string(),
                format!("comp/md.{a}"),
                format!("comp/clang.{a}"),
                format!("comp/gcc.{a}"),
            ],
        ),
    ];
    let mut shas = Vec::new();
    for (name, lists) in &sets {
        let mut list = read_list(src, lists)?;
        if *name == "base" {
            list.push("./var/sysmerge/etc.tgz".to_string());
            list.push("./var/sysmerge/etcsum".to_string());
        }
        let tar = scratch.join(format!("{name}.tar"));
        let mut rep = make_set(&tree, &ids, &list, &tar, &mut claimed)?;
        rep.name = (*name).to_string();
        let tgz = dir.join(format!("{name}{rev}.tgz"));
        gzip(&tar, &tgz)?;
        let _ = fs::remove_file(&tar);
        shas.push(sha256_line(&tgz, &format!("{name}{rev}.tgz"))?);
        reports.push(rep);
    }
    // EXTRA: what the tree has and no list names (files only).
    let extra: Vec<String> = tree
        .iter()
        .filter(|(p, e)| !matches!(e.kind, Kind::Dir) && !claimed.contains(*p))
        .map(|(p, _)| p.clone())
        .collect();
    for r in &mut reports {
        r.extra = extra.clone();
    }

    // INSTALL.<arch>: `distrib/notes/Makefile`'s `m4 -DOSREV=... INSTALL`, with the host's m4.
    let notes = dir.join(format!("INSTALL.{a}"));
    let osrev = format!("{}.{}", &rev[..1], &rev[1..]);
    let m4 = Command::new("m4")
        .arg(format!("-DOSREV={osrev}"))
        .arg(format!("-DOSrev={rev}"))
        .arg(format!(
            "-DINCLUDE={}",
            src.join("distrib/notes").join(a).display()
        ))
        .arg(format!("-DMACHINE={a}"))
        .arg("-Uunix")
        .arg(src.join("distrib/notes/INSTALL"))
        .output();
    match m4 {
        Ok(out) if !out.stdout.is_empty() => {
            fs::write(&notes, &out.stdout).map_err(|e| format!("{}: {e}", notes.display()))?;
            shas.push(sha256_line(&notes, &format!("INSTALL.{a}"))?);
        }
        other => {
            let _ = fs::remove_file(&notes);
            println!(
                "  INSTALL.{a}: not made: the host's m4 failed on distrib/notes/INSTALL ({other:?}); the installer asks about it"
            );
        }
    }

    // The kernels. `bsd` is the MULTIPROCESSOR kernel the smokes boot, and it is `bsd.mp`
    // too: OpenBSD ships a uniprocessor `bsd` and `bsd.mp`, and install.sub insists on
    // `bsd.mp` when `hw.ncpufound` is more than 1 (`SANESETS`), installs it as `/bsd` and
    // keeps `bsd` as `/bsd.sp`.
    for (file, name) in [(bsd, "bsd"), (bsd, "bsd.mp"), (bsd_rd, "bsd.rd")] {
        if let Some(k) = file {
            let to = dir.join(name);
            let _ = fs::remove_file(&to);
            fs::copy(k, &to).map_err(|e| format!("cp {} {}: {e}", k.display(), to.display()))?;
            shas.push(sha256_line(&to, name)?);
        }
    }
    shas.sort();
    let sha_file = dir.join("SHA256");
    write_if_changed(&sha_file, &shas.concat())?;
    let sig = dir.join("SHA256.sig");
    super::signify::sign(&signify, &secret, &sha_file, &sig)?;
    let check = scratch.join("SHA256.check");
    super::signify::verify(&signify, &pubkey, &sig, &check)?;

    // The reports.
    let mut summary = String::new();
    for r in &reports {
        let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
        for m in &r.missing {
            *by_reason.entry(reason(m)).or_default() += 1;
        }
        let mut text = String::new();
        for m in &r.missing {
            text.push_str(&format!("{m}\t{}\n", reason(m)));
        }
        write_if_changed(&dir.join(format!("MISSING-{}.txt", r.name)), &text)?;
        write_if_changed(
            &dir.join(format!("EXTRA-{}.txt", r.name)),
            &r.extra.iter().map(|p| format!("{p}\n")).collect::<String>(),
        )?;
        summary.push_str(&format!(
            "  set {}{rev}.tgz: {} entries, {} bytes tar; {} listed paths not in this build \
             (MISSING-{}.txt), {} built files in no list (EXTRA-{}.txt)\n",
            r.name,
            r.files,
            r.bytes,
            r.missing.len(),
            r.name,
            r.extra.len(),
            r.name
        ));
        for (why, n) in by_reason {
            summary.push_str(&format!("      {n:5}  {why}\n"));
        }
    }
    print!("{summary}");
    println!(
        "  {}: {} signed by the test key {} (verified)",
        dir.display(),
        sig.display(),
        pubkey.display()
    );
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biosboot_placeholder_is_what_loadproto_takes() {
        let f = biosboot_placeholder();
        assert_eq!(&f[..4], b"\x7fELF");
        assert_eq!(f[4], 1); // ELFCLASS32
        assert_eq!(u16::from_le_bytes([f[44], f[45]]), 1); // e_phnum
        let phoff = u32::from_le_bytes([f[28], f[29], f[30], f[31]]) as usize;
        let off = u32::from_le_bytes(f[phoff + 4..phoff + 8].try_into().unwrap()) as usize;
        let filesz = u32::from_le_bytes(f[phoff + 16..phoff + 20].try_into().unwrap()) as usize;
        assert_eq!(filesz % 512, 0);
        assert!(f[off..off + filesz].starts_with(b"EmiBSD placeholder"));
    }

    #[test]
    fn ustar_header_checksum_and_names() {
        let h = header(
            "./usr/bin/cc",
            0o555,
            0,
            7,
            1234,
            b'0',
            "",
            "root",
            "bin",
            5,
        )
        .unwrap();
        assert_eq!(&h[257..262], b"ustar");
        assert_eq!(&h[0..12], b"./usr/bin/cc");
        assert_eq!(&h[124..135], b"00000002322");
        let mut copy = h;
        copy[148..156].copy_from_slice(b"        ");
        let sum: u32 = copy.iter().map(|&b| u32::from(b)).sum();
        assert_eq!(&h[148..154], format!("{sum:06o}").as_bytes());
        assert_eq!(h[154], 0);
        assert_eq!(h[155], b' ');
    }

    #[test]
    fn long_names_split_at_a_slash() {
        let long = format!("./{}/{}", "a".repeat(120), "b".repeat(60));
        let h = header(&long, 0o644, 0, 0, 0, b'0', "", "root", "wheel", 0).unwrap();
        assert_eq!(
            &h[345..345 + 122],
            format!("./{}", "a".repeat(120)).as_bytes()
        );
        assert_eq!(&h[0..60], "b".repeat(60).as_bytes());
        assert!(header(&"x".repeat(300), 0, 0, 0, 0, b'0', "", "r", "w", 0).is_err());
    }

    #[test]
    fn reasons_group_by_prefix() {
        assert!(reason("./usr/libdata/perl5/x.pm").contains("perl"));
        assert!(reason("./usr/bin/gdb").contains("GPL"));
        assert!(reason("./usr/share/man/man1/ls.1").contains("man"));
        assert_eq!(reason("./nonsense"), "not built by this build yet");
    }

    #[test]
    fn list_paths_are_read_and_filtered() {
        let dir = std::env::temp_dir().join(format!("emibsd-sets-{}", std::process::id()));
        fs::create_dir_all(dir.join("distrib/sets/lists/base")).unwrap();
        fs::write(
            dir.join("distrib/sets/lists/base/mi"),
            "# comment\n./bin\n./bin/cat\n\n./etc/rc\n",
        )
        .unwrap();
        let l = read_list(&dir, &["base/mi".to_string(), "base/absent".to_string()]).unwrap();
        assert_eq!(l, ["./bin", "./bin/cat", "./etc/rc"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tar_has_entries_in_order_and_hard_links() {
        let dir = std::env::temp_dir().join(format!("emibsd-sets-tar-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a"), b"hello").unwrap();
        fs::hard_link(dir.join("a"), dir.join("b")).unwrap();
        let mut tree = Tree::new();
        tree.insert(".".to_string(), ent(Kind::Dir, 0o755, "root", "wheel"));
        tree.insert(
            "./a".to_string(),
            ent(Kind::File(dir.join("a")), 0o555, "root", "bin"),
        );
        tree.insert(
            "./b".to_string(),
            ent(Kind::File(dir.join("b")), 0o555, "root", "bin"),
        );
        tree.insert(
            "./l".to_string(),
            ent(Kind::Symlink("a".to_string()), 0o755, "root", "wheel"),
        );
        let ids = Ids {
            users: BTreeMap::new(),
            groups: BTreeMap::from([("bin".to_string(), 7)]),
        };
        let tar = dir.join("t.tar");
        let paths: Vec<String> = ["./a", "./b", "./l"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        write_tar(&tree, &ids, &paths, &tar).unwrap();
        let bytes = fs::read(&tar).unwrap();
        // a: header + one block; b: a hard link header; l: a symlink header; two zero blocks.
        assert_eq!(bytes.len(), 512 * (2 + 1 + 1 + 2));
        assert_eq!(bytes[156], b'0');
        assert_eq!(&bytes[1024 + 157..1024 + 160], b"./a");
        assert_eq!(bytes[1024 + 156], b'1');
        assert_eq!(bytes[1536 + 156], b'2');
        let _ = fs::remove_dir_all(&dir);
    }
}
/* </TESTS> */
