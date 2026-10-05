//! Host tests of acpi(4)'s core that need no firmware and no namespace: the id matching, the
//! `_CRS` and `_DSD` parsing, the task queue and the fixed registers of an unmapped acpi0.
//! (The namespace walks run in the QEMU smokes; `dsdt/tests.rs` owns the namespace on the
//! host.)

use std::boxed::Box;
use std::sync::Mutex;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::dev::acpi::amltypes::AmlObj;

/// The task queue is global: the tests that use it take this lock.
static TASKQ_LOCK: Mutex<()> = Mutex::new(());

/// An acpi0 softc as autoconfiguration allocates it: all zero (the `Softc` contract).
fn zeroed_softc() -> Box<AcpiSoftc> {
    // SAFETY: all-zero is a valid `AcpiSoftc` (`unsafe impl Softc`, acpivar.rs).
    unsafe { Box::<AcpiSoftc>::new_zeroed().assume_init() }
}

/// `memset(&aaa, 0, sizeof(aaa))` without a softc.
fn aaa() -> AcpiAttachArgs {
    AcpiAttachArgs {
        aaa_name: ptr::null(),
        aaa_iot: None,
        aaa_memt: None,
        aaa_dmat: None,
        aaa_table: ptr::null_mut(),
        aaa_node: None,
        aaa_dev: ptr::null(),
        aaa_cdev: ptr::null(),
        aaa_addr: [0; 8],
        aaa_size: [0; 8],
        aaa_bst: [None; 8],
        aaa_naddr: 0,
        aaa_irq: [0; 8],
        aaa_irq_flags: [0; 8],
        aaa_nirq: 0,
    }
}

#[test]
fn hids_match_as_in_c() {
    assert_eq!(_acpi_matchhids(b"PNP0C0F", &ACPI_SKIP_HIDS), 1);
    assert_eq!(_acpi_matchhids(b"PNP0501", &ACPI_SKIP_HIDS), 0);
    assert_eq!(_acpi_matchhids(b"ACPI0007", &ACPI_QUIET_HIDS), 1);

    let node: AmlNodeRef = Rc::new(crate::dev::acpi::amltypes::AmlNode::new(None, *b"COM1\0"));
    let mut a = aaa();
    let dev = *b"PNP0700\0";
    let cdev = *b"PNP0C02\0";
    // No node: no match.
    a.aaa_dev = dev.as_ptr();
    assert_eq!(acpi_matchhids(&a, &ACPI_ISA_HIDS, "fdc"), 0);
    a.aaa_node = Some(node);
    assert_eq!(acpi_matchhids(&a, &ACPI_ISA_HIDS, "fdc"), 2);
    // The _CID matches: 1.
    a.aaa_cdev = cdev.as_ptr();
    assert_eq!(acpi_matchhids(&a, &ACPI_SKIP_HIDS, "none"), 1);
}

#[test]
fn hid_strings() {
    // EISAID("PNP0303") is 0x0303d041.
    assert_eq!(hid_string(&AmlValue::integer(0x0303_d041)), b"PNP0303");
    assert_eq!(hid_string(&AmlValue::string(b"QEMU0002")), b"QEMU0002");
    assert_eq!(hid_string(&AmlValue::buffer(&[1, 2])), b"unknown");

    let mut out = [0xffu8; 8];
    strlcpy(&mut out, b"ACPI0010-long");
    assert_eq!(&out, b"ACPI001\0");
}

/// A `_CRS` buffer: an I/O port range 0x3f8..0x3ff (8 bytes), IRQ 4 without flags (so
/// edge, exclusive, active high), a fixed 32-bit memory range, a bus-number word range, and
/// the end tag.
fn crs() -> AmlValue {
    let mut b: Vec<u8> = vec![];
    b.extend_from_slice(&[0x47, 0x01, 0xf8, 0x03, 0xf8, 0x03, 0x01, 0x08]); // IO
    b.extend_from_slice(&[0x22, 0x10, 0x00]); // IRQNoFlags {4}
    b.extend_from_slice(&[0x86, 0x09, 0x00, 0x01]); // Memory32Fixed, read-write
    b.extend_from_slice(&0xfed0_0000u32.to_le_bytes());
    b.extend_from_slice(&0x400u32.to_le_bytes());
    // WordBusNumber: type 2, _MIN 0, _MAX 0xff, _LEN 0x100.
    b.extend_from_slice(&[0x88, 0x0d, 0x00, 0x02, 0x0c, 0x00]);
    for w in [0u16, 0, 0xff, 0, 0x100] {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b.extend_from_slice(&[0x79, 0x00]); // EndTag
    AmlValue::buffer(&b)
}

#[test]
fn crs_resources_fill_the_attach_args() {
    let mut a = aaa();
    aml_parse_resource(&crs(), &mut |i, r| acpi_parse_resources(i, r, &mut a));
    // The bus range is skipped; the port and the memory are recorded.
    assert_eq!(a.aaa_naddr, 2);
    assert_eq!((a.aaa_addr[0], a.aaa_size[0]), (0x3f8, 8));
    assert_eq!((a.aaa_addr[1], a.aaa_size[1]), (0xfed0_0000, 0x400));
    assert_eq!(a.aaa_nirq, 1);
    assert_eq!(a.aaa_irq[0], 4);
    assert_eq!(a.aaa_irq_flags[0], u32::from(LR_EXTIRQ_MODE));

    let mut bus = -1;
    aml_parse_resource(&crs(), &mut |i, r| acpi_getminbus(i, r, &mut bus));
    assert_eq!(bus, 0);
}

#[test]
fn dsd_properties_are_found_by_name() {
    let pkg = |v: Vec<AmlValueRef>| AmlValue::from_obj(AmlObj::Package(v));
    let prop =
        |name: &[u8], v: AmlValue| Rc::new(pkg(vec![Rc::new(AmlValue::string(name)), Rc::new(v)]));
    let dsd = pkg(vec![
        Rc::new(AmlValue::buffer(&DSD_PROP_GUID)),
        Rc::new(pkg(vec![
            prop(b"clock-frequency", AmlValue::integer(1_843_200)),
            prop(b"compatible", AmlValue::string(b"snps,dw-apb-uart")),
        ])),
    ]);
    let props = dsd_properties(&dsd, &DSD_PROP_GUID).unwrap();
    assert_eq!(
        dsd_property(&props, b"clock-frequency")
            .unwrap()
            .v_integer(),
        1_843_200
    );
    assert_eq!(
        dsd_property(&props, b"compatible").unwrap().v_string(),
        b"snps,dw-apb-uart"
    );
    assert!(dsd_property(&props, b"missing").is_none());
    // Another UUID: not device properties.
    assert!(dsd_properties(&dsd, &[0; 16]).is_none());
}

static RAN: Mutex<Vec<(usize, i32)>> = Mutex::new(Vec::new());

fn record(arg0: *mut c_void, arg1: i32) {
    RAN.lock().unwrap().push((arg0 as usize, arg1));
}

#[test]
fn tasks_run_once_in_order() {
    let _g = TASKQ_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let sc = zeroed_softc();
    RAN.lock().unwrap().clear();

    assert_eq!(acpi_dotask(&sc), 0);
    acpi_addtask(&sc, record, 1 as *mut c_void, 10);
    acpi_addtask(&sc, record, 2 as *mut c_void, 20);
    while acpi_dotask(&sc) != 0 {}
    assert_eq!(*RAN.lock().unwrap(), vec![(1, 10), (2, 20)]);
    assert_eq!(acpi_dotask(&sc), 0);
}

#[test]
fn an_unmapped_acpi0() {
    let sc = zeroed_softc();
    // No register is mapped: reads give 0, writes go nowhere.
    assert_eq!(acpi_read_pmreg(&sc, ACPIREG_PM1A_CNT, 0), 0);
    acpi_write_pmreg(&sc, ACPIREG_PM1A_CNT, 0, 0x2000);
    // Hardware-reduced ACPI is always in ACPI mode.
    sc.sc_hw_reduced.set(1);
    assert_eq!(
        acpi_read_pmreg(&sc, ACPIREG_PM1B_CNT, 0),
        i32::from(ACPI_PM1_SCI_EN)
    );
    // No acpi0 at all: the access fails.
    let mut b = [0u8; 4];
    assert_eq!(
        acpi_gasio(None, ACPI_IOREAD, GAS_SYSTEM_IOSPACE, 0xcf9, 1, 1, &mut b),
        -1
    );
    // The host double maps no firmware table.
    assert!(acpi_maptable(&sc, Paddr::new(0xe0000), None, None, None, 1).is_none());
    assert!(sc.sc_tables.iter().next().is_none());
}
