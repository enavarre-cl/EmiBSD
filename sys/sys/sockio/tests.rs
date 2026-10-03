use super::*;
use crate::sys::ioccom::{IOC_IN, IOC_INOUT, IOC_OUT, IOCPARM_MASK};
use core::mem::size_of;

/// Every command this file defines, by name.
const OURS: &[(&str, u64)] = &[
    ("SIOCATMARK", SIOCATMARK),
    ("SIOCSPGRP", SIOCSPGRP),
    ("SIOCGPGRP", SIOCGPGRP),
    ("SIOCSIFADDR", SIOCSIFADDR),
    ("SIOCGIFADDR", SIOCGIFADDR),
    ("SIOCSIFDSTADDR", SIOCSIFDSTADDR),
    ("SIOCGIFDSTADDR", SIOCGIFDSTADDR),
    ("SIOCSIFFLAGS", SIOCSIFFLAGS),
    ("SIOCGIFFLAGS", SIOCGIFFLAGS),
    ("SIOCGIFBRDADDR", SIOCGIFBRDADDR),
    ("SIOCSIFBRDADDR", SIOCSIFBRDADDR),
    ("SIOCGIFCONF", SIOCGIFCONF),
    ("SIOCGIFNETMASK", SIOCGIFNETMASK),
    ("SIOCSIFNETMASK", SIOCSIFNETMASK),
    ("SIOCGIFMETRIC", SIOCGIFMETRIC),
    ("SIOCSIFMETRIC", SIOCSIFMETRIC),
    ("SIOCDIFADDR", SIOCDIFADDR),
    ("SIOCAIFADDR", SIOCAIFADDR),
    ("SIOCGIFDATA", SIOCGIFDATA),
    ("SIOCSIFLLADDR", SIOCSIFLLADDR),
    ("SIOCADDMULTI", SIOCADDMULTI),
    ("SIOCDELMULTI", SIOCDELMULTI),
    ("SIOCSIFMEDIA", SIOCSIFMEDIA),
    ("SIOCGIFMEDIA", SIOCGIFMEDIA),
    ("SIOCGIFSFFPAGE", SIOCGIFSFFPAGE),
    ("SIOCDIFPHYADDR", SIOCDIFPHYADDR),
    ("SIOCSLIFPHYADDR", SIOCSLIFPHYADDR),
    ("SIOCGLIFPHYADDR", SIOCGLIFPHYADDR),
    ("SIOCSIFMTU", SIOCSIFMTU),
    ("SIOCGIFMTU", SIOCGIFMTU),
    ("SIOCIFCREATE", SIOCIFCREATE),
    ("SIOCIFDESTROY", SIOCIFDESTROY),
    ("SIOCIFGCLONERS", SIOCIFGCLONERS),
    ("SIOCAIFGROUP", SIOCAIFGROUP),
    ("SIOCGIFGROUP", SIOCGIFGROUP),
    ("SIOCDIFGROUP", SIOCDIFGROUP),
    ("SIOCGIFGMEMB", SIOCGIFGMEMB),
    ("SIOCGIFGATTR", SIOCGIFGATTR),
    ("SIOCSIFGATTR", SIOCSIFGATTR),
    ("SIOCGIFGLIST", SIOCGIFGLIST),
    ("SIOCSIFDESCR", SIOCSIFDESCR),
    ("SIOCGIFDESCR", SIOCGIFDESCR),
    ("SIOCSIFRTLABEL", SIOCSIFRTLABEL),
    ("SIOCGIFRTLABEL", SIOCGIFRTLABEL),
    ("SIOCSETVLAN", SIOCSETVLAN),
    ("SIOCGETVLAN", SIOCGETVLAN),
    ("SIOCSSPPPPARAMS", SIOCSSPPPPARAMS),
    ("SIOCGSPPPPARAMS", SIOCGSPPPPARAMS),
    ("SIOCDELLABEL", SIOCDELLABEL),
    ("SIOCGPWE3", SIOCGPWE3),
    ("SIOCSETLABEL", SIOCSETLABEL),
    ("SIOCGETLABEL", SIOCGETLABEL),
    ("SIOCSIFPRIORITY", SIOCSIFPRIORITY),
    ("SIOCGIFPRIORITY", SIOCGIFPRIORITY),
    ("SIOCSIFXFLAGS", SIOCSIFXFLAGS),
    ("SIOCGIFXFLAGS", SIOCGIFXFLAGS),
    ("SIOCSIFRDOMAIN", SIOCSIFRDOMAIN),
    ("SIOCGIFRDOMAIN", SIOCGIFRDOMAIN),
    ("SIOCSLIFPHYRTABLE", SIOCSLIFPHYRTABLE),
    ("SIOCGLIFPHYRTABLE", SIOCGLIFPHYRTABLE),
    ("SIOCSETKALIVE", SIOCSETKALIVE),
    ("SIOCGETKALIVE", SIOCGETKALIVE),
    ("SIOCGIFHARDMTU", SIOCGIFHARDMTU),
    ("SIOCSVNETID", SIOCSVNETID),
    ("SIOCGVNETID", SIOCGVNETID),
    ("SIOCSLIFPHYTTL", SIOCSLIFPHYTTL),
    ("SIOCGLIFPHYTTL", SIOCGLIFPHYTTL),
    ("SIOCGIFRXR", SIOCGIFRXR),
    ("SIOCIFAFATTACH", SIOCIFAFATTACH),
    ("SIOCIFAFDETACH", SIOCIFAFDETACH),
    ("SIOCSETMPWCFG", SIOCSETMPWCFG),
    ("SIOCGETMPWCFG", SIOCGETMPWCFG),
    ("SIOCDVNETID", SIOCDVNETID),
    ("SIOCSIFPAIR", SIOCSIFPAIR),
    ("SIOCGIFPAIR", SIOCGIFPAIR),
    ("SIOCSIFPARENT", SIOCSIFPARENT),
    ("SIOCGIFPARENT", SIOCGIFPARENT),
    ("SIOCDIFPARENT", SIOCDIFPARENT),
    ("SIOCSIFLLPRIO", SIOCSIFLLPRIO),
    ("SIOCGIFLLPRIO", SIOCGIFLLPRIO),
    ("SIOCGUMBINFO", SIOCGUMBINFO),
    ("SIOCSUMBPARAM", SIOCSUMBPARAM),
    ("SIOCGUMBPARAM", SIOCGUMBPARAM),
    ("SIOCSLIFPHYDF", SIOCSLIFPHYDF),
    ("SIOCGLIFPHYDF", SIOCGLIFPHYDF),
    ("SIOCSVNETFLOWID", SIOCSVNETFLOWID),
    ("SIOCGVNETFLOWID", SIOCGVNETFLOWID),
    ("SIOCSTXHPRIO", SIOCSTXHPRIO),
    ("SIOCGTXHPRIO", SIOCGTXHPRIO),
    ("SIOCSLIFPHYECN", SIOCSLIFPHYECN),
    ("SIOCGLIFPHYECN", SIOCGLIFPHYECN),
    ("SIOCSRXHPRIO", SIOCSRXHPRIO),
    ("SIOCGRXHPRIO", SIOCGRXHPRIO),
    ("SIOCSPWE3CTRLWORD", SIOCSPWE3CTRLWORD),
    ("SIOCGPWE3CTRLWORD", SIOCGPWE3CTRLWORD),
    ("SIOCSPWE3FAT", SIOCSPWE3FAT),
    ("SIOCGPWE3FAT", SIOCGPWE3FAT),
    ("SIOCSPWE3NEIGHBOR", SIOCSPWE3NEIGHBOR),
    ("SIOCGPWE3NEIGHBOR", SIOCGPWE3NEIGHBOR),
    ("SIOCDPWE3NEIGHBOR", SIOCDPWE3NEIGHBOR),
    ("SIOCSVH", SIOCSVH),
    ("SIOCGVH", SIOCGVH),
    ("SIOCSETPFSYNC", SIOCSETPFSYNC),
    ("SIOCGETPFSYNC", SIOCGETPFSYNC),
    ("SIOCSETPFLOW", SIOCSETPFLOW),
    ("SIOCGETPFLOW", SIOCGETPFLOW),
];

/// The commands whose argument structures are not ported yet.
const DEFERRED_PREFIXES: &[&str] = &["SIOCBRDG", "SIOCGETVIFCNT", "SIOCGETSGCNT"];

#[test]
fn well_known_values() {
    // The numbers ifconfig(8) and friends use on OpenBSD/amd64 and arm64.
    assert_eq!(SIOCSIFADDR, 0x8020_690c);
    assert_eq!(SIOCGIFFLAGS, 0xc020_6911);
    assert_eq!(SIOCSIFFLAGS, 0x8020_6910);
    assert_eq!(SIOCAIFADDR, 0x8040_691a);
    assert_eq!(SIOCGIFCONF, 0xc010_6924);
    assert_eq!(SIOCATMARK, 0x4004_7307);
    assert_eq!(SIOCGIFMEDIA, 0xc040_6938);
}

/// The size `sizeof(t)` has for the argument types `sockio.h` names.
fn c_size(t: &str) -> usize {
    match t {
        "int" => size_of::<i32>(),
        "struct ifreq" => size_of::<Ifreq>(),
        "struct ifconf" => size_of::<Ifconf>(),
        "struct ifaliasreq" => size_of::<Ifaliasreq>(),
        "struct ifmediareq" => size_of::<Ifmediareq>(),
        "struct if_sffpage" => size_of::<IfSffpage>(),
        "struct if_laddrreq" => size_of::<IfLaddrreq>(),
        "struct if_clonereq" => size_of::<IfClonereq>(),
        "struct ifgroupreq" => size_of::<Ifgroupreq>(),
        "struct ifkalivereq" => size_of::<Ifkalivereq>(),
        "struct if_afreq" => size_of::<IfAfreq>(),
        "struct if_parent" => size_of::<IfParent>(),
        _ => panic!("unknown argument type {t}"),
    }
}

/// Every `#define SIOC... _IO*('g', n, type)` of the C: the direction, group, number and
/// argument type it names must give the value we computed.
#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/sockio.h");
    let mut seen = 0;
    for (name, text) in &defs {
        if !name.starts_with("SIOC") || DEFERRED_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let (mac, args) = text.split_once('(').expect(name);
        let args = args.trim_end().strip_suffix(')').expect(name);
        let parts: std::vec::Vec<&str> = args.split(',').map(str::trim).collect();
        let dir = match mac.trim() {
            "_IOR" => IOC_OUT,
            "_IOW" => IOC_IN,
            "_IOWR" => IOC_INOUT,
            m => panic!("{name}: {m}"),
        };
        let group = u64::from(parts[0].as_bytes()[1]);
        let num: u64 = parts[1].parse().expect(name);
        let len = c_size(parts[2]) as u64;
        let want = dir | ((len & IOCPARM_MASK) << 16) | (group << 8) | num;
        let ours = OURS.iter().find(|(n, _)| n == name);
        let Some((_, value)) = ours else {
            panic!("{name} is not ported");
        };
        assert_eq!(*value, want, "{name}");
        seen += 1;
    }
    assert_eq!(seen, OURS.len());
}
