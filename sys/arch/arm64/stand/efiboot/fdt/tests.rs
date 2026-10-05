//! The tree editor on copies of the ACPI template blob (`dt_blob.rs`).

use super::*;
use crate::dt_blob::DT_BLOB_TEMPLATE;
use std::vec::Vec;

/// A copy of the template blob, and a tree on it.
fn template() -> (Vec<u8>, Fdt) {
    let mut b = DT_BLOB_TEMPLATE.to_vec();
    let mut t = Fdt::new();
    // SAFETY: `b` is a whole blob that only `t` uses; its heap buffer does not move.
    assert_eq!(unsafe { t.init(b.as_mut_ptr()) }, 17);
    (b, t)
}

/// A second tree on the blob `t` wrote, after `fdt_finalize`.
fn reread(b: &mut [u8]) -> Fdt {
    let mut t = Fdt::new();
    // SAFETY: `b` is a whole blob that only the new tree uses.
    assert_eq!(unsafe { t.init(b.as_mut_ptr()) }, 17);
    t
}

#[test]
fn check_head_rejects_what_is_not_a_blob() {
    let mut zero = [0u8; 64];
    // SAFETY: 64 readable bytes.
    assert_eq!(unsafe { fdt_check_head(zero.as_ptr()) }, 0);
    // SAFETY: as above; null is no tree.
    assert_eq!(unsafe { Fdt::new().init(core::ptr::null_mut()) }, 0);
    zero[..4].copy_from_slice(&FDT_MAGIC.to_be_bytes());
    zero[20..24].copy_from_slice(&0x12u32.to_be_bytes()); // newer than the code
    // SAFETY: as above.
    assert_eq!(unsafe { fdt_check_head(zero.as_ptr()) }, 0);
    let (b, _) = template();
    // SAFETY: a whole blob.
    assert_eq!(unsafe { fdt_get_size(b.as_ptr()) }, b.len());
}

#[test]
fn find_matches_prefixes_and_paths() {
    let (_b, t) = template();
    let root = t.next_node(None).unwrap();
    assert_eq!(t.find_node(b"/"), Some(root));
    assert_eq!(t.find_node(b"//"), Some(root));
    let serial = t.find_node(b"/serial").unwrap();
    assert_eq!(t.node_name(serial), Some(&b"serial@0"[..]));
    assert_eq!(t.find_node(b"/serial@1"), None);
    assert_eq!(t.find_node(b"chosen"), None);
    assert_eq!(t.find_node(b"/chosen/framebuffer"), None);
    assert_eq!(t.parent_node(serial), Some(root));
    assert_eq!(t.parent_node(root), None);
    assert_eq!(t.child_node(serial), None);
}

#[test]
fn add_and_set_properties_and_nodes() {
    let (mut b, mut t) = template();
    let chosen = t.find_node(b"/chosen").unwrap();
    assert_eq!(t.add_property(chosen, b"bootargs", b"sd0a:/bsd -s\0"), 1);
    assert_eq!(
        t.add_property(chosen, b"openbsd,boothowto", &2u32.to_be_bytes()),
        1
    );
    // a property that exists is set in place, longer, then shorter
    let psci = t.find_node(b"/psci").unwrap();
    assert_eq!(t.set_property(psci, b"method", b"hvc-long-name\0"), 1);
    assert_eq!(t.set_property(psci, b"method", b"hvc\0"), 1);
    assert_eq!(t.set_property(psci, b"nope", b"x\0"), 0);
    let cpus = t.find_node(b"/cpus").unwrap();
    let cpu = t.add_node(cpus, b"cpu@0").unwrap();
    assert_eq!(t.add_property(cpu, b"reg", &0u64.to_be_bytes()), 1);
    assert_eq!(t.add_property(cpu, b"msi-controller", &[]), 1);
    let cpu1 = t.add_node(cpus, b"cpu@1").unwrap();
    assert_eq!(t.add_property(cpu1, b"device_type", b"cpu\0"), 1);
    t.finalize();

    let t = reread(&mut b);
    let chosen = t.find_node(b"/chosen").unwrap();
    assert_eq!(
        t.property(chosen, b"bootargs"),
        Some(&b"sd0a:/bsd -s\0"[..])
    );
    assert_eq!(t.node_property_int(chosen, b"openbsd,boothowto"), Some(2));
    assert_eq!(
        t.property(chosen, b"stdout-path"),
        Some(&b"serial0:115200n8\0"[..])
    );
    let psci = t.find_node(b"/psci").unwrap();
    assert_eq!(t.property(psci, b"method"), Some(&b"hvc\0"[..]));
    assert_eq!(t.property(psci, b"status"), Some(&b"disabled\0"[..]));
    let cpu = t.find_node(b"/cpus/cpu@0").unwrap();
    assert_eq!(t.property(cpu, b"reg"), Some(&[0u8; 8][..]));
    let cpu1 = t.find_node(b"/cpus/cpu@1").unwrap();
    assert_eq!(t.next_node(Some(cpu)), Some(cpu1));
    assert!(t.find_node(b"/acpi").is_some());
    // the new names went to the strings block once each
    let strings = u32::from_be_bytes([b[12], b[13], b[14], b[15]]) as usize;
    let size = u32::from_be_bytes([b[32], b[33], b[34], b[35]]) as usize;
    let block = &b[strings..strings + size];
    assert_eq!(block.windows(9).filter(|w| w == b"bootargs\0").count(), 1);
    assert!(block.ends_with(b"device_type\0"));
}

#[test]
fn property_ints_and_compatible() {
    let (_b, mut t) = template();
    let gic = t.find_node(b"/interrupt-controller").unwrap();
    let mut out = [0i32; 2];
    assert_eq!(t.node_property_ints(gic, b"#interrupt-cells", &mut out), 1);
    assert_eq!(out[0], 3);
    assert_eq!(t.node_property_ints(gic, b"nope", &mut out), -1);
    assert_eq!(
        t.set_property(gic, b"compatible", b"arm,gic-400\0arm,cortex-a15-gic\0"),
        1
    );
    assert!(t.node_is_compatible(gic, b"arm,cortex-a15-gic"));
    assert!(t.node_is_compatible(gic, b"arm,gic-400"));
    assert!(!t.node_is_compatible(gic, b"arm,gic"));
}
