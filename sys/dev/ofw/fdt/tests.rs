//! Host tests over a blob built here, shaped like QEMU `virt`'s (a GIC and a PL011).

use std::sync::Mutex;
use std::vec::Vec;
use std::{assert, assert_eq, assert_ne};

use super::*;

/// The tree is a global: one test at a time.
static LOCK: Mutex<()> = Mutex::new(());

struct Builder {
    structure: Vec<u8>,
    strings: Vec<u8>,
}

impl Builder {
    fn new() -> Self {
        Self {
            structure: Vec::new(),
            strings: Vec::new(),
        }
    }

    fn word(&mut self, w: u32) {
        self.structure.extend_from_slice(&w.to_be_bytes());
    }

    fn pad(&mut self) {
        while self.structure.len() % 4 != 0 {
            self.structure.push(0);
        }
    }

    fn begin(&mut self, name: &[u8]) {
        self.word(FDT_NODE_BEGIN);
        self.structure.extend_from_slice(name);
        self.structure.push(0);
        self.pad();
    }

    fn end(&mut self) {
        self.word(FDT_NODE_END);
    }

    fn string_off(&mut self, name: &[u8]) -> u32 {
        let off = self.strings.len() as u32;
        self.strings.extend_from_slice(name);
        self.strings.push(0);
        off
    }

    fn prop(&mut self, name: &[u8], value: &[u8]) {
        let off = self.string_off(name);
        self.word(FDT_PROPERTY);
        self.word(value.len() as u32);
        self.word(off);
        self.structure.extend_from_slice(value);
        self.pad();
    }

    fn prop_cells(&mut self, name: &[u8], cells: &[u32]) {
        let mut v = Vec::new();
        for c in cells {
            v.extend_from_slice(&c.to_be_bytes());
        }
        self.prop(name, &v);
    }

    fn finish(mut self) -> Vec<u8> {
        self.word(FDT_END);
        let header_len = 40usize;
        let reserve_off = header_len;
        let reserve_len = 16; // one empty entry
        let struct_off = reserve_off + reserve_len;
        let strings_off = struct_off + self.structure.len();
        let size = strings_off + self.strings.len();
        let mut blob = Vec::new();
        for w in [
            FDT_MAGIC,
            size as u32,
            struct_off as u32,
            strings_off as u32,
            reserve_off as u32,
            17,
            16,
            0,
            self.strings.len() as u32,
            self.structure.len() as u32,
        ] {
            blob.extend_from_slice(&w.to_be_bytes());
        }
        blob.extend_from_slice(&[0u8; 16]);
        blob.extend_from_slice(&self.structure);
        blob.extend_from_slice(&self.strings);
        blob
    }
}

/// A tree with a root, an interrupt controller, a UART under a bus with ranges, and chosen.
fn virt_like() -> Vec<u8> {
    let mut b = Builder::new();
    b.begin(b"");
    b.prop_cells(b"#address-cells", &[2]);
    b.prop_cells(b"#size-cells", &[2]);
    b.prop(b"compatible", b"linux,dummy-virt\0");
    b.begin(b"intc@8000000");
    b.prop(b"compatible", b"arm,cortex-a15-gic\0");
    b.prop_cells(
        b"reg",
        &[0, 0x0800_0000, 0, 0x10000, 0, 0x0801_0000, 0, 0x10000],
    );
    b.prop_cells(b"#interrupt-cells", &[3]);
    b.prop(b"interrupt-controller", b"");
    b.prop_cells(b"phandle", &[1]);
    b.end();
    b.begin(b"soc");
    b.prop_cells(b"#address-cells", &[1]);
    b.prop_cells(b"#size-cells", &[1]);
    // 32-bit child addresses 0x1000_0000.. map to parent 0x9000_0000..
    b.prop_cells(b"ranges", &[0x1000_0000, 0, 0x0900_0000, 0x0100_0000]);
    b.begin(b"pl011@10000000");
    b.prop(b"compatible", b"arm,pl011\0arm,primecell\0");
    b.prop_cells(b"reg", &[0x1000_0000, 0x1000]);
    b.prop_cells(b"interrupts", &[0, 1, 4]);
    b.prop_cells(b"interrupt-parent", &[1]);
    b.prop(b"status", b"okay\0");
    b.end();
    b.end();
    b.begin(b"chosen");
    b.prop(b"stdout-path", b"/soc/pl011@10000000\0");
    b.end();
    b.end();
    b.finish()
}

#[test]
fn parses_a_virt_like_tree() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let blob = virt_like();
    assert_eq!(fdt_check_head(blob.as_ptr()), 17);
    assert_eq!(fdt_init(blob.as_ptr()), 17);
    assert_eq!(fdt_get_size(blob.as_ptr()), blob.len());

    let root = fdt_next_node(ptr::null());
    assert!(!root.is_null());
    assert_eq!(fdt_node_name(root), Some(&b""[..]));
    assert_eq!(fdt_find_node(b"/"), root);
    assert!(fdt_parent_node(root).is_null());

    let gic = fdt_find_node(b"/intc@8000000");
    assert!(!gic.is_null());
    assert_eq!(
        fdt_find_node(b"/intc"),
        gic,
        "a match without the unit address"
    );
    assert!(fdt_is_compatible(gic, b"arm,cortex-a15-gic"));
    assert!(!fdt_is_compatible(gic, b"arm,gic-v3"));
    assert_eq!(fdt_node_property_int(gic, b"#interrupt-cells"), Some(3));
    assert!(fdt_node_property(gic, b"interrupt-controller").is_some());
    assert!(fdt_node_property(gic, b"nonsense").is_none());
    assert_eq!(fdt_find_phandle(1), gic);
    assert!(fdt_find_phandle(7).is_null());

    let mut reg = FdtReg::default();
    assert_eq!(fdt_get_reg(gic, 0, &mut reg), Ok(()));
    assert_eq!(
        reg,
        FdtReg {
            addr: 0x0800_0000,
            size: 0x10000
        }
    );
    assert_eq!(fdt_get_reg(gic, 1, &mut reg), Ok(()));
    assert_eq!(reg.addr, 0x0801_0000);
    assert_eq!(fdt_get_reg(gic, 2, &mut reg), Err(Errno::EINVAL));

    let uart = fdt_find_node(b"/soc/pl011@10000000");
    assert!(!uart.is_null());
    assert_eq!(fdt_find_node(b"/soc/pl011"), uart);
    assert_eq!(fdt_parent_node(uart), fdt_find_node(b"/soc"));
    assert_eq!(fdt_get_cells(uart), (1, 1));
    assert_eq!(fdt_get_cells(gic), (2, 2));
    assert_eq!(fdt_get_reg(uart, 0, &mut reg), Ok(()));
    assert_eq!(
        reg,
        FdtReg {
            addr: 0x0900_0000,
            size: 0x1000
        },
        "translated through the bus's ranges"
    );
    let mut ints = [0i32; 3];
    assert_eq!(
        fdt_node_property_ints(uart, b"interrupts", &mut ints),
        Some(3)
    );
    assert_eq!(ints, [0, 1, 4]);

    assert!(fdt_find_node(b"/nowhere").is_null());
    assert!(fdt_find_node(b"relative").is_null());
    assert!(fdt_child_node(fdt_find_node(b"/chosen")).is_null());
    assert_eq!(fdt_next_node(gic), fdt_find_node(b"/soc"));
}

#[test]
fn openfirmware_handles() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let blob = virt_like();
    assert_eq!(fdt_init(blob.as_ptr()), 17);

    let root = OF_peer(0);
    assert_ne!(root, 0);
    let gic = OF_child(root);
    assert_eq!(gic, OF_finddevice(b"/intc@8000000"));
    assert_eq!(OF_parent(gic), root);
    assert_eq!(OF_peer(gic), OF_finddevice(b"/soc"));
    assert_eq!(OF_finddevice(b"/nope"), -1);
    assert_eq!(OF_getnodebyname(0, b"soc"), OF_finddevice(b"/soc"));
    assert_eq!(OF_getnodebyname(0, b"intc"), gic);
    assert_eq!(OF_getnodebyname(0, b"zzz"), 0);
    assert_eq!(OF_getnodebyphandle(1), gic);

    assert_eq!(OF_getpropint(gic, b"#interrupt-cells", 0), 3);
    assert_eq!(OF_getpropint(gic, b"missing", 42), 42);
    assert!(OF_getpropbool(gic, b"interrupt-controller"));
    assert!(!OF_getpropbool(gic, b"missing"));
    assert!(OF_is_compatible(gic, b"arm,cortex-a15-gic"));
    assert!(OF_is_enabled(gic));

    let uart = OF_finddevice(b"/soc/pl011@10000000");
    assert_eq!(OF_getproplen(uart, b"interrupts"), 12);
    let mut cells = [0u32; 4];
    assert_eq!(OF_getpropintarray(uart, b"interrupts", &mut cells), 12);
    assert_eq!(&cells[..3], &[0, 1, 4]);
    assert_eq!(OF_getpropintarray(uart, b"missing", &mut cells), -1);
    assert_eq!(OF_getpropint(uart, b"interrupt-parent", 0), 1);
    assert!(OF_is_enabled(uart));
    assert_eq!(OF_getindex(uart, Some(b"arm,primecell"), b"compatible"), 1);
    assert_eq!(OF_getindex(uart, Some(b"nope"), b"compatible"), -1);
    assert_eq!(OF_getindex(uart, None, b"compatible"), 0);

    // the synthesized "name" of a node without one
    assert_eq!(OF_getproplen(uart, b"name"), 6);
    let mut name = [0u8; 16];
    assert_eq!(OF_getprop(uart, b"name", &mut name), 6);
    assert_eq!(&name[..6], b"pl011\0");
    let mut short = [0u8; 3];
    assert_eq!(OF_getprop(uart, b"name", &mut short), 6);
    assert_eq!(&short, b"pl\0");

    let mut buf = [0u8; 64];
    let chosen = OF_finddevice(b"/chosen");
    let len = OF_getprop(chosen, b"stdout-path", &mut buf);
    assert_eq!(&buf[..len as usize], b"/soc/pl011@10000000\0");
}
