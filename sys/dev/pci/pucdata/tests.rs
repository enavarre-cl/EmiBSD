use super::*;
use crate::dev::pci::pucvar::PUC_MAX_PORTS;

#[test]
fn the_table_is_whole() {
    assert_eq!(PUC_DEVS.len(), 218);
    let ports: usize = PUC_DEVS
        .iter()
        .map(|d| d.ports.iter().filter(|p| p.type_ != 0).count())
        .sum();
    assert_eq!(ports, 566);
    // The ports of an entry come first, without holes.
    for d in &PUC_DEVS {
        let n = d.ports.iter().filter(|p| p.type_ != 0).count();
        assert!(n >= 1 && n <= PUC_MAX_PORTS);
        assert!(d.ports[..n].iter().all(|p| p.type_ != 0));
        assert!(d.ports[..n].iter().all(|p| p.bar >= 0x10 && p.bar <= 0x24));
    }
}

#[test]
fn the_last_three_entries_are_qemus_serial_cards() {
    let tail = &PUC_DEVS[PUC_DEVS.len() - 3..];

    for (d, (prod, n)) in tail.iter().zip([(2u16, 1), (3, 2), (4, 4)]) {
        assert_eq!(d.rval, [0x1b36, prod, 0, 0]);
        assert_eq!(d.rmask, [0xffff, 0xffff, 0, 0]);
        for i in 0..n {
            let p = &d.ports[i];
            assert_eq!((p.bar, p.offset), (0x10, 8 * i as u16));
        }
        assert_eq!(d.ports[n].type_, 0);
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn the_table_matches_the_c() {
    let dir = crate::reftest::openbsd_src();
    let text = std::fs::read_to_string(dir.join("sys/dev/pci/pucdata.c")).unwrap();
    let defs = crate::reftest::defines("sys/dev/pci/pcidevs.h");
    let resolve = |tok: &str| -> i64 {
        let tok = tok.trim();
        if let Some(hex) = tok.strip_prefix("0x") {
            i64::from_str_radix(hex, 16).unwrap()
        } else if let Ok(n) = tok.parse() {
            n
        } else {
            crate::reftest::int(&defs, tok).unwrap_or_else(|| panic!("{tok}"))
        }
    };

    // Without the comments and line breaks: every `{ vendor, product, subvendor, subproduct }`
    // group (the one naming a PCI_VENDOR_) in order, and every `{ PUC_PORT_..., bar, offset }`.
    let mut flat = std::string::String::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("/*") {
        flat.push_str(&rest[..i]);
        rest = &rest[i + 2..];
        rest = &rest[rest.find("*/").unwrap() + 2..];
    }
    flat.push_str(rest);
    let flat = flat.replace('\n', " ");

    let mut ids = std::vec::Vec::new();
    let mut ports = std::vec::Vec::new();
    for group in flat.split('{').skip(1) {
        let Some(inner) = group.split('}').next() else {
            continue;
        };
        let f: std::vec::Vec<&str> = inner.split(',').collect();
        if f.len() >= 4 && f[0].contains("PCI_VENDOR_") {
            ids.push([resolve(f[0]), resolve(f[1]), resolve(f[2]), resolve(f[3])]);
        } else if f.len() >= 3 && f[0].trim().starts_with("PUC_PORT_") {
            let t = match f[0].trim() {
                "PUC_PORT_LPT" => PUC_PORT_LPT,
                "PUC_PORT_COM" => PUC_PORT_COM,
                "PUC_PORT_COM_MUL4" => PUC_PORT_COM_MUL4,
                "PUC_PORT_COM_MUL8" => PUC_PORT_COM_MUL8,
                "PUC_PORT_COM_MUL10" => PUC_PORT_COM_MUL10,
                "PUC_PORT_COM_MUL128" => PUC_PORT_COM_MUL128,
                "PUC_PORT_COM_XR17V35X" => PUC_PORT_COM_XR17V35X,
                other => panic!("{other}"),
            };
            ports.push((i64::from(t), resolve(f[1]), resolve(f[2])));
        }
    }

    assert_eq!(ids.len(), PUC_DEVS.len());
    for (d, id) in PUC_DEVS.iter().zip(&ids) {
        let got = [
            i64::from(d.rval[0]),
            i64::from(d.rval[1]),
            i64::from(d.rval[2]),
            i64::from(d.rval[3]),
        ];
        assert_eq!(&got, id);
    }

    let table: std::vec::Vec<(i64, i64, i64)> = PUC_DEVS
        .iter()
        .flat_map(|d| d.ports.iter().filter(|p| p.type_ != 0))
        .map(|p| (i64::from(p.type_), i64::from(p.bar), i64::from(p.offset)))
        .collect();
    assert_eq!(table, ports);
}
