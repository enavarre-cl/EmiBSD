//! Host tests of the BAR decoding, over a fake configuration space the host double serves
//! (`Machine::set_pci_conf`). [`FakePci`], [`with_fake`] and [`attach_args`] are shared with
//! `pci.rs`'s tests.

use std::boxed::Box;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use super::*;
use crate::dev::pci::pcivar::PCI_FLAGS_MEM_ENABLED;
use crate::machine::Machine;
use crate::machine::pci_machdep::{pci_decompose_tag, pci_make_tag};

/// One function's configuration space: 64 registers, and the size each BAR decodes (0 for
/// an unimplemented one) with its type bits.
#[derive(Clone)]
pub(crate) struct FakeFunc {
    pub regs: [u32; 64],
    /// For each of the six BARs: (size in bytes, type bits); a 64-bit BAR's size is given on
    /// its low register and the high register follows.
    pub bars: [(u64, u32); 6],
}

impl Default for FakeFunc {
    fn default() -> Self {
        Self {
            regs: [0; 64],
            bars: [(0, 0); 6],
        }
    }
}

/// A bus of fake functions, by tag.
#[derive(Default)]
pub(crate) struct FakePci {
    pub funcs: BTreeMap<(i32, i32, i32), FakeFunc>,
}

impl FakePci {
    /// Adds a function with the given ID and class registers; header type and the rest zero.
    pub fn add(&mut self, bus: i32, dev: i32, func: i32, id: u32, class: u32) -> &mut FakeFunc {
        let f = self.funcs.entry((bus, dev, func)).or_default();
        f.regs[0] = id;
        f.regs[2] = class;
        f
    }
}

impl FakeFunc {
    /// Sets the BHLC register's header type byte.
    pub fn hdrtype(&mut self, ty: u32) -> &mut Self {
        self.regs[3] = (self.regs[3] & !0x00ff_0000) | (ty << 16);
        self
    }

    /// A BAR at index `i` decoding `size` bytes of `typebits`, assigned `base`.
    pub fn bar(&mut self, i: usize, size: u64, typebits: u32, base: u64) -> &mut Self {
        self.bars[i] = (size, typebits);
        self.regs[4 + i] = (base as u32 & !0xf) | typebits;
        if typebits & PCI_MAPREG_MEM_TYPE_64BIT != 0 && typebits & 1 == 0 {
            self.regs[5 + i] = (base >> 32) as u32;
        }
        self
    }
}

/// The BAR (index, high half of a 64-bit one) register `reg` (a word index) belongs to.
fn bar_of(f: &FakeFunc, reg: usize) -> Option<(usize, bool)> {
    if !(4..10).contains(&reg) {
        return None;
    }
    let i = reg - 4;
    if i > 0 {
        let (size, ty) = f.bars[i - 1];
        if size != 0 && ty & 1 == 0 && ty & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
            return Some((i - 1, true));
        }
    }
    Some((i, false))
}

/// A read of the fake: all ones where there is no function.
fn fake_read(fake: &Mutex<FakePci>, tag: Pcitag, reg: i32) -> u32 {
    let fake = fake.lock().unwrap();
    match fake.funcs.get(&pci_decompose_tag(pc(), tag)) {
        Some(f) => f.regs[(reg / 4) as usize],
        None => 0xffff_ffff,
    }
}

/// A write to the fake: a BAR keeps only the address bits its size leaves writable.
fn fake_write(fake: &Mutex<FakePci>, tag: Pcitag, reg: i32, data: u32) {
    let mut fake = fake.lock().unwrap();
    let Some(f) = fake.funcs.get_mut(&pci_decompose_tag(pc(), tag)) else {
        return;
    };
    let r = (reg / 4) as usize;
    match bar_of(f, r) {
        Some((i, high)) => {
            let (size, ty) = f.bars[i];
            let mask = !(size.max(1) - 1);
            f.regs[r] = if size == 0 {
                0
            } else if high {
                data & (mask >> 32) as u32
            } else if ty & 1 != 0 {
                (data & mask as u32 & !0x3) | ty
            } else {
                (data & mask as u32 & !0xf) | ty
            };
        }
        None => f.regs[r] = data,
    }
}

/// The host's chipset tag.
pub(crate) fn pc() -> PciChipsetTag {
    Default::default()
}

/// Serialises the tests that install a configuration space.
static LOCK: Mutex<()> = Mutex::new(());

/// Installs `fake` for the duration of `f`.
pub(crate) fn with_fake<R>(fake: FakePci, f: impl FnOnce(&Arc<Mutex<FakePci>>) -> R) -> R {
    let _guard: MutexGuard<'_, ()> = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let shared = Arc::new(Mutex::new(fake));
    let (r_fake, w_fake) = (shared.clone(), shared.clone());
    Machine::set_pci_conf(Some((
        Box::new(move |tag, reg| fake_read(&r_fake, tag, reg)),
        Box::new(move |tag, reg, data| fake_write(&w_fake, tag, reg, data)),
    )));
    let r = f(&shared);
    Machine::set_pci_conf(None);
    r
}

/// Attach arguments for function `(bus, dev, func)` of the fake.
pub(crate) fn attach_args(bus: i32, dev: i32, func: i32) -> PciAttachArgs {
    let tag = pci_make_tag(pc(), bus, dev, func);
    PciAttachArgs {
        pa_iot: Default::default(),
        pa_memt: Default::default(),
        pa_dmat: Default::default(),
        pa_pc: pc(),
        pa_flags: PCI_FLAGS_IO_ENABLED | PCI_FLAGS_MEM_ENABLED,
        pa_domain: 0,
        pa_bus: bus as u32,
        pa_device: dev as u32,
        pa_function: func as u32,
        pa_tag: tag,
        pa_id: 0,
        pa_class: 0,
        pa_bridgetag: None,
        pa_bridgeih: None,
        pa_intrswiz: 0,
        pa_intrtag: tag,
        pa_intrpin: 0,
        pa_intrline: 0,
        pa_rawintrpin: 0,
    }
}

fn virtio_like() -> FakePci {
    let mut fake = FakePci::default();
    fake.add(0, 3, 0, 0x1000_1af4, 0x0200_0000)
        .bar(0, 0x20, PCI_MAPREG_TYPE_IO, 0xc040)
        .bar(1, 0x1000, PCI_MAPREG_TYPE_MEM, 0xfebd_1000)
        .bar(
            4,
            0x4000,
            PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT | PCI_MAPREG_MEM_PREFETCHABLE_MASK,
            0xfe00_0000,
        );
    fake
}

#[test]
fn bar_sizes_are_decoded() {
    with_fake(virtio_like(), |shared| {
        let tag = pci_make_tag(pc(), 0, 3, 0);
        // Decoding is enabled; the probe must disable and restore it.
        shared
            .lock()
            .unwrap()
            .funcs
            .get_mut(&(0, 3, 0))
            .unwrap()
            .regs[1] = 0x7;

        let io = pci_mapreg_type(pc(), tag, 0x10);
        assert_eq!(io, PCI_MAPREG_TYPE_IO);
        assert_eq!(pci_mapreg_info(pc(), tag, 0x10, io), Ok((0xc040, 0x20, 0)));

        let mem = pci_mapreg_type(pc(), tag, 0x14);
        assert_eq!(mem, PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT);
        assert_eq!(
            pci_mapreg_info(pc(), tag, 0x14, mem),
            Ok((0xfebd_1000, 0x1000, 0))
        );

        let mem64 = pci_mapreg_type(pc(), tag, 0x20);
        assert_eq!(mem64, PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT);
        let prefetchable =
            <crate::machine::Machine as crate::machine::BusSpace>::BUS_SPACE_MAP_PREFETCHABLE;
        assert_eq!(
            pci_mapreg_info(pc(), tag, 0x20, mem64),
            Ok((0xfe00_0000, 0x4000, prefetchable as i32))
        );

        // The BARs and the command register read back as before.
        let fake = shared.lock().unwrap();
        let f = &fake.funcs[&(0, 3, 0)];
        assert_eq!(f.regs[1], 0x7);
        assert_eq!(f.regs[4], 0xc041);
        assert_eq!(f.regs[5], 0xfebd_1000);
    });
}

#[test]
fn probe_type_mismatch_and_void_regions() {
    with_fake(virtio_like(), |_| {
        let tag = pci_make_tag(pc(), 0, 3, 0);
        assert_eq!(pci_mapreg_probe(pc(), tag, 0x10), Some(PCI_MAPREG_TYPE_IO));
        // BAR 2 (0x18) is not implemented.
        assert_eq!(pci_mapreg_probe(pc(), tag, 0x18), None);
        // An I/O BAR asked for as memory, and the other way round.
        assert_eq!(
            pci_mapreg_info(pc(), tag, 0x10, PCI_MAPREG_TYPE_MEM),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            pci_mapreg_info(pc(), tag, 0x14, PCI_MAPREG_TYPE_IO),
            Err(Errno::EINVAL)
        );
        // A 32-bit BAR asked for as 64-bit.
        assert_eq!(
            pci_mapreg_info(
                pc(),
                tag,
                0x14,
                PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT
            ),
            Err(Errno::EINVAL)
        );
        // An unimplemented BAR decodes nothing: a void region.
        assert_eq!(
            pci_mapreg_info(pc(), tag, 0x18, PCI_MAPREG_TYPE_MEM),
            Err(Errno::ENOENT)
        );
    });
}

#[test]
fn assign_enables_decoding_and_mastering() {
    let mut fake = virtio_like();
    fake.add(0, 4, 0, 0x1000_1af4, 0)
        .bar(0, 0x1000, PCI_MAPREG_TYPE_MEM, 0);
    with_fake(fake, |shared| {
        let pa = attach_args(0, 3, 0);
        assert_eq!(
            pci_mapreg_assign(&pa, 0x14, PCI_MAPREG_TYPE_MEM),
            Ok((0xfebd_1000, 0x1000))
        );
        let csr = shared.lock().unwrap().funcs[&(0, 3, 0)].regs[1];
        assert_eq!(csr, PCI_COMMAND_MEM_ENABLE | PCI_COMMAND_MASTER_ENABLE);

        // A BAR the firmware left at 0 needs an extent to be placed; there is none.
        let pa = attach_args(0, 4, 0);
        assert_eq!(
            pci_mapreg_assign(&pa, 0x10, PCI_MAPREG_TYPE_MEM),
            Err(Errno::EINVAL)
        );

        // pci_mapreg_map limits the size and maps through bus_space.
        let pa = attach_args(0, 3, 0);
        let (_, _, base, size) = pci_mapreg_map(&pa, 0x10, PCI_MAPREG_TYPE_IO, 0, 0x10).unwrap();
        assert_eq!((base, size), (0xc040, 0x10));
    });
}
