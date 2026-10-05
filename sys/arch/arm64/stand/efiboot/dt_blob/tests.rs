//! The blob's layout, and (reference test) its bytes against dtc's `dt_blob.S`.

use super::*;
use crate::fdt::Fdt;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

/// A copy of the blob, as `efi_acpi` would edit it.
fn blob() -> Vec<u8> {
    DT_BLOB_TEMPLATE.to_vec()
}

#[test]
fn header_and_blocks() {
    let b = blob();
    let w = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    assert_eq!(w(0), FDT_MAGIC);
    assert_eq!(w(4) as usize, b.len());
    assert_eq!(w(8), 0x38);
    assert_eq!(w(16), 0x28);
    assert_eq!((w(20), w(24)), (0x11, 0x10));
    assert_eq!(w(12) as usize, 0x38 + w(36) as usize);
    // the strings end where the padding starts
    assert_eq!(w(12) as usize + w(32) as usize + 16384, b.len());
    assert_eq!(&b[w(12) as usize..w(12) as usize + 6], b"model\0");
}

#[test]
fn the_tree_reads_back() {
    let mut b = blob();
    let mut t = Fdt::new();
    // SAFETY: `b` is the whole blob, used by nothing else.
    assert_eq!(unsafe { t.init(b.as_mut_ptr()) }, 0x11);
    let root = t.find_node(b"/").unwrap();
    assert_eq!(t.property(root, b"model"), Some(&b"ACPI\0"[..]));
    assert_eq!(t.node_property_int(root, b"interrupt-parent"), Some(1));
    let gic = t.find_node(b"/interrupt-controller").unwrap();
    assert_eq!(t.node_name(gic), Some(&b"interrupt-controller@0"[..]));
    assert_eq!(t.node_property_int(gic, b"phandle"), Some(1));
    assert_eq!(t.node_property(gic, b"ranges").0, 0);
    let timer = t.find_node(b"/timer").unwrap();
    assert!(t.node_is_compatible(timer, b"arm,armv8-timer"));
    assert_eq!(
        t.property(timer, b"interrupt-names"),
        Some(&b"sec-phys\0phys\0virt\0hyp-phys\0"[..])
    );
    let aliases = t.find_node(b"/aliases").unwrap();
    assert_eq!(t.property(aliases, b"serial0"), Some(&b"/serial@0\0"[..]));
    let names: Vec<_> = core::iter::successors(t.child_node(root), |&n| t.next_node(Some(n)))
        .map(|n| String::from_utf8_lossy(t.node_name(n).unwrap()).into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "chosen",
            "aliases",
            "cpus",
            "psci",
            "timer",
            "interrupt-controller@0",
            "serial@0",
            "acpi@0"
        ]
    );
}

/// `$OPENBSD_SRC` (`just test-ref`), relative paths from the workspace root.
fn openbsd_src() -> std::path::PathBuf {
    let Some(dir) = std::env::var_os("OPENBSD_SRC") else {
        panic!("OPENBSD_SRC is not set; run `just test-ref`");
    };
    let dir = std::path::PathBuf::from(dir);
    if dir.is_absolute() {
        dir
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../..")
            .join(dir)
    }
}

/// Assembles dtc's `-O asm` output: labels, `.byte` (a literal or a byte of the difference
/// of two labels), `.long`, `.asciz`, `.balign`, `.space`; two passes for the labels.
fn assemble(src: &str) -> Vec<u8> {
    let pass = |labels: &BTreeMap<String, usize>| {
        let mut out: Vec<u8> = Vec::new();
        let mut defined = BTreeMap::new();
        for line in src.lines() {
            let s = line.trim();
            if s.is_empty() || s.starts_with("/*") || s.starts_with(".globl") {
                continue;
            }
            if let Some(label) = s.strip_suffix(':') {
                defined.insert(String::from(label), out.len());
            } else if let Some(e) = s.strip_prefix(".byte") {
                let e = e.trim();
                if let Some(hex) = e.strip_prefix("0x") {
                    out.push(u8::from_str_radix(hex, 16).unwrap());
                    continue;
                }
                // ((a - b) >> n) & 0xff, or (a - b) & 0xff
                let inner = &e[e.rfind('(').unwrap() + 1..e.find(')').unwrap()];
                let (a, b) = inner.split_once(" - ").unwrap();
                let shift = e
                    .split_once(">> ")
                    .map_or(0, |(_, r)| r.split(')').next().unwrap().parse().unwrap());
                let v = labels.get(a).copied().unwrap_or(0) as i64
                    - labels.get(b).copied().unwrap_or(0) as i64;
                out.push(((v >> shift) & 0xff) as u8);
            } else if let Some(e) = s.strip_prefix(".long") {
                for x in e.split(',') {
                    let x = x.trim();
                    let v = x.strip_prefix("0x").map_or_else(
                        || x.parse::<u32>().unwrap(),
                        |h| u32::from_str_radix(h, 16).unwrap(),
                    );
                    // dtc writes only zeros here, whose byte order does not matter
                    out.extend_from_slice(&v.to_le_bytes());
                }
            } else if let Some(e) = s.strip_prefix(".asciz") {
                out.extend_from_slice(e.trim().trim_matches('"').as_bytes());
                out.push(0);
            } else if let Some(e) = s.strip_prefix(".balign") {
                let n: usize = e.split(',').next().unwrap().trim().parse().unwrap();
                while !out.len().is_multiple_of(n) {
                    out.push(0);
                }
            } else if let Some(e) = s.strip_prefix(".space") {
                let n: usize = e.split(',').next().unwrap().trim().parse().unwrap();
                out.resize(out.len() + n, 0);
            } else {
                panic!("dt_blob.S: unexpected line {s:?}");
            }
        }
        (out, defined)
    };
    let (_, labels) = pass(&BTreeMap::new());
    pass(&labels).0
}

#[test]
#[ignore = "reads the C reference tree (just test-ref)"]
fn same_bytes_as_dt_blob_s() {
    let path = openbsd_src().join("sys/arch/arm64/stand/efiboot/dt_blob.S");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let reference = assemble(&src);
    assert_eq!(reference.len(), DT_BLOB_SIZE);
    assert_eq!(blob(), reference);
}
