//! Host tests of the capability walkers, the bus enumeration and the power state functions,
//! over the fake configuration space of `pci_map`'s tests.

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::dev::pci::pci_map::tests::{FakePci, attach_args, pc, with_fake};
use crate::dev::pci::pcidevs::{PCI_PRODUCT_CIRRUS_CL_PD6729, PCI_VENDOR_CIRRUS};

/// A function with a capability list: power management at 0x40, MSI at 0x50 (64-bit), MSI-X
/// at 0x60 with an 8-entry table.
fn with_caps() -> FakePci {
    let mut fake = FakePci::default();
    let f = fake.add(0, 2, 0, pci_id_code(0x1af4, 0x1000), 0x0200_0000);
    f.regs[1] = PCI_STATUS_CAPLIST_SUPPORT;
    f.regs[(PCI_CAPLISTPTR_REG / 4) as usize] = 0x40;
    f.regs[0x40 / 4] = 0x5000 | PCI_CAP_PWRMGMT as u32;
    f.regs[0x44 / 4] = PCI_PMCSR_STATE_D0;
    f.regs[0x50 / 4] = PCI_MSI_MC_C64 | 0x6000 | PCI_CAP_MSI as u32;
    f.regs[0x60 / 4] = (7 << PCI_MSIX_MC_TBLSZ_SHIFT) | PCI_CAP_MSIX as u32;
    fake
}

#[test]
fn capabilities_are_found_by_walking_the_list() {
    with_fake(with_caps(), |shared| {
        let tag = pci_make_tag(pc(), 0, 2, 0);
        assert_eq!(
            pci_get_capability(pc(), tag, PCI_CAP_MSI).map(|(o, _)| o),
            Some(0x50)
        );
        let (off, reg) = pci_get_capability(pc(), tag, PCI_CAP_MSIX).unwrap();
        assert_eq!(off, 0x60);
        assert_eq!(pci_msix_mc_tblsz(reg), 7);
        assert!(pci_get_capability(pc(), tag, PCI_CAP_VPD).is_none());
        assert!(pci_get_ht_capability(pc(), tag, PCI_HT_CAP_MSI).is_none());
        assert!(pci_get_ext_capability(pc(), tag, 1).is_none());

        // MSI-X entries come from the capability, when MSI is allowed on the bus.
        let mut pa = attach_args(0, 2, 0);
        assert_eq!(pci_intr_msix_count(&pa), 0);
        pa.pa_flags |= PCI_FLAGS_MSI_ENABLED;
        assert_eq!(pci_intr_msix_count(&pa), 8);

        // A broken list (a pointer below 0x40) ends the walk.
        shared
            .lock()
            .unwrap()
            .funcs
            .get_mut(&(0, 2, 0))
            .unwrap()
            .regs[0x40 / 4] = 0x2000 | PCI_CAP_PWRMGMT as u32;
        assert!(pci_get_capability(pc(), tag, PCI_CAP_MSI).is_none());

        // No capability list at all.
        shared
            .lock()
            .unwrap()
            .funcs
            .get_mut(&(0, 2, 0))
            .unwrap()
            .regs[1] = 0;
        assert!(pci_get_capability(pc(), tag, PCI_CAP_PWRMGMT).is_none());
    });
}

#[test]
fn power_states() {
    with_fake(with_caps(), |shared| {
        let tag = pci_make_tag(pc(), 0, 2, 0);
        shared
            .lock()
            .unwrap()
            .funcs
            .get_mut(&(0, 2, 0))
            .unwrap()
            .regs[1] |= 0x7;
        assert_eq!(pci_get_powerstate(pc(), tag), PCI_PMCSR_STATE_D0 as i32);
        assert_eq!(
            pci_set_powerstate(pc(), tag, PCI_PMCSR_STATE_D3 as i32),
            PCI_PMCSR_STATE_D0 as i32
        );
        assert_eq!(pci_get_powerstate(pc(), tag), PCI_PMCSR_STATE_D3 as i32);
        // Going to D3 turned off decoding and bus mastering.
        let csr = shared.lock().unwrap().funcs[&(0, 2, 0)].regs[1];
        assert_eq!(csr & 0x7, 0);
    });
}

/// The functions `pci_enumerate_bus` probes, recorded by its match function.
static SEEN: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

fn record(pa: &PciAttachArgs) -> i32 {
    SEEN.lock().unwrap().push((pa.pa_device, pa.pa_function));
    0
}

fn first(_pa: &PciAttachArgs) -> i32 {
    1
}

fn find_storage(pa: &PciAttachArgs) -> i32 {
    i32::from(pci_class(pa.pa_class) == PCI_CLASS_MASS_STORAGE)
}

/// A bus PciSoftc for the tests (zeroed, then given its tags), leaked.
fn test_bus() -> &'static PciSoftc {
    // SAFETY: all-zero is a valid PciSoftc (its Softc contract).
    let sc: &'static PciSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<PciSoftc>() }));
    sc.sc_devs.init();
    sc.sc_iot.set(Some(Default::default()));
    sc.sc_memt.set(Some(Default::default()));
    sc.sc_dmat.set(Some(Default::default()));
    sc.sc_pc.set(Some(pc()));
    sc.sc_maxndevs.set(32);
    sc
}

#[test]
fn enumeration_follows_headers_and_quirks() {
    let mut fake = FakePci::default();
    // dev 0: a single-function host bridge.
    fake.add(0, 0, 0, pci_id_code(0x8086, 0x29c0), 0x0600_0000);
    // dev 1: multi-function, functions 0 and 2 present.
    fake.add(0, 1, 0, pci_id_code(0x8086, 0x2918), 0x0601_0000)
        .hdrtype(0x80);
    fake.add(0, 1, 2, pci_id_code(0x8086, 0x2922), 0x0106_0100);
    // dev 4: says multi-function but is a MONOFUNCTION quirk; function 1 must not be seen.
    fake.add(
        0,
        4,
        0,
        pci_id_code(PCI_VENDOR_CIRRUS, PCI_PRODUCT_CIRRUS_CL_PD6729),
        0x0605_0000,
    )
    .hdrtype(0x80);
    fake.add(0, 4, 1, pci_id_code(0x1234, 0x5678), 0);
    // dev 5: vendor 0, skipped.
    fake.add(0, 5, 0, 0, 0);
    // dev 6: header type 3, skipped.
    fake.add(0, 6, 0, pci_id_code(0x1234, 0x1), 0).hdrtype(3);

    with_fake(fake, |_| {
        let sc = test_bus();
        SEEN.lock().unwrap().clear();
        assert_eq!(pci_enumerate_bus(sc, Some(record), None), 0);
        assert_eq!(SEEN.lock().unwrap()[..], [(0, 0), (1, 0), (1, 2), (4, 0)]);

        // A match function that answers stops the walk and fills the arguments.
        let mut pa = attach_args(0, 0, 0);
        assert_eq!(pci_enumerate_bus(sc, Some(find_storage), Some(&mut pa)), 1);
        assert_eq!((pa.pa_device, pa.pa_function), (1, 2));
        assert_eq!(pci_subclass(pa.pa_class), PCI_SUBCLASS_MASS_STORAGE_SATA);
        assert_eq!(pa.pa_flags & PCI_FLAGS_MEM_ENABLED, PCI_FLAGS_MEM_ENABLED);

        assert_eq!(pci_requester_id(pc(), pa.pa_tag), (1 << 3) | 2);
        let ids = [PciMatchid {
            pm_vid: 0x8086,
            pm_pid: 0x2922,
        }];
        assert_eq!(pci_matchbyid(&pa, &ids), 1);
        assert_eq!(pci_matchbyid(&attach_args(0, 0, 0), &ids), 0);
    });
}

#[test]
fn interrupt_pins_are_swizzled_behind_a_bridge() {
    let mut fake = FakePci::default();
    fake.add(1, 3, 0, pci_id_code(0x1af4, 0x1000), 0).regs[(PCI_INTERRUPT_REG / 4) as usize] =
        (PCI_INTERRUPT_PIN_A << PCI_INTERRUPT_PIN_SHIFT) | 11;
    with_fake(fake, |_| {
        let sc = test_bus();
        sc.sc_bus.set(1);
        static BRIDGE: HostTag = HostTag::new();
        sc.sc_bridgetag.set(Some(BRIDGE.get()));
        sc.sc_intrswiz.set(1);
        let mut pa = attach_args(0, 0, 0);
        assert_eq!(pci_enumerate_bus(sc, Some(first), Some(&mut pa)), 1);
        // Pin A of device 3 behind a bridge with swizzle 1: ((1 + 4 - 1) % 4) + 1 = pin A.
        assert_eq!(pa.pa_rawintrpin, PCI_INTERRUPT_PIN_A as u8);
        assert_eq!(pa.pa_intrswiz, 4);
        assert_eq!(pa.pa_intrpin, 1);
        assert_eq!(pa.pa_intrline, 11);
    });
}

/// A `'static` pcitag for a bridge.
struct HostTag(std::sync::OnceLock<Pcitag>);

impl HostTag {
    const fn new() -> Self {
        Self(std::sync::OnceLock::new())
    }

    fn get(&'static self) -> &'static Pcitag {
        self.0.get_or_init(|| pci_make_tag(pc(), 0, 1, 0))
    }
}
