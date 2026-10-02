//! Host test double for the machine interface. Not an OpenBSD architecture.
//!
//! Compiled whenever `target_os != "none"` so that `cargo test` runs on macOS/Linux and the
//! compiler proves the `machine` contract is complete. It prints to stdout, has no-op SPL and
//! fakes hardware with std collections. It must not grow logic: behaviour belongs in `kern/`.
//!
//! Its machine parameters mirror amd64's `<machine/param.h>` and `<machine/_types.h>`, so tests
//! see the page geometry of a real architecture (`just test-ref` checks that they stay equal).
//! Its console is a `Consdev` that writes to stdout and reads nothing, so `kprintf!` in a test
//! shows up with `--nocapture`; its bus space accepts every map, reads 0 and drops writes.

use core::cell::Cell;
use std::eprintln;
use std::io::Write;

use crate::dev::cons::{CN_LOWPRI, Consdev, set_cn_tab};
use crate::machine::bus::{BusAddr, BusSize, BusSpace};
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, MachineInfo, MachineParam};
use crate::sys::errno::Errno;
use crate::sys::param::NODEV;
use crate::sys::types::Dev;

/// The host implementation of the machine interface.
pub struct Machine;

/// The host's one bus space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostBusSpace;

/// A host bus space handle: the address that was "mapped".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostBusSpaceHandle(usize);

fn host_cngetc(_dev: Dev) -> i32 {
    0
}

fn host_cnputc(_dev: Dev, c: i32) {
    // A failed write to stdout has nowhere to be reported; the console is best effort.
    let _ = std::io::stdout().write_all(&[c as u8]);
}

fn host_cnpollc(_dev: Dev, _on: bool) {}

/// The host console device.
static HOSTCONS: Consdev = Consdev {
    cn_probe: None,
    cn_init: None,
    cn_getc: host_cngetc,
    cn_putc: host_cnputc,
    cn_pollc: host_cnpollc,
    cn_bell: None,
    cn_dev: Cell::new(NODEV),
    cn_pri: Cell::new(CN_LOWPRI),
};

impl MachineInfo for Machine {
    const MACHINE: &'static str = "host";
    const MACHINE_ARCH: &'static str = "host";
}

impl MachineParam for Machine {
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << Self::PAGE_SHIFT;
    const PAGE_MASK: usize = Self::PAGE_SIZE - 1;
    const KERNBASE: usize = 0xffff_ffff_8000_0000;
    const UPAGES: usize = 6;
    const USPACE: usize = Self::UPAGES * Self::PAGE_SIZE;
    const USPACE_ALIGN: usize = 0;
    const HAVE_USPACE_GUARD: bool = true;
    const NMBCLUSTERS: usize = 256 * 1024;
    const MSGBUFSIZE: usize = 32 * Self::PAGE_SIZE;
    const HAVE_ACPI: bool = true;
    const HAVE_FDT: bool = false;
    const ALIGNBYTES: usize = core::mem::size_of::<usize>() - 1;
    const STACKALIGNBYTES: usize = 15;
    const MAX_PAGE_SHIFT: usize = 12;

    fn aligned_pointer<T>(_p: usize) -> bool {
        true
    }
}

impl Cpu for Machine {
    unsafe fn early_init(_boot: &BootInfo) -> Result<(), &'static str> {
        Ok(())
    }

    /// The host has no CPU to park: the process ends instead.
    fn halt() -> ! {
        std::process::exit(0)
    }

    /// The host has nothing to reboot: the process ends with a failure status.
    fn boot(howto: i32) -> ! {
        let _ = std::io::stdout().flush();
        eprintln!("host: boot(howto={howto:#x})");
        std::process::exit(1)
    }

    fn delay(usec: u32) {
        std::thread::sleep(std::time::Duration::from_micros(u64::from(usec)));
    }
}

impl Console for Machine {
    fn consinit() {
        set_cn_tab(&HOSTCONS);
    }
}

impl Exit for Machine {
    fn exit(status: ExitStatus) -> ! {
        let _ = std::io::stdout().flush();
        std::process::exit(status.qemu_status() as i32)
    }
}

impl BusSpace for Machine {
    type Tag = HostBusSpace;
    type Handle = HostBusSpaceHandle;

    const BUS_SPACE_MAP_CACHEABLE: u32 = 0x01;
    const BUS_SPACE_MAP_LINEAR: u32 = 0x02;
    const BUS_SPACE_MAP_PREFETCHABLE: u32 = 0x08;

    unsafe fn bus_space_map(
        _t: Self::Tag,
        addr: BusAddr,
        _size: BusSize,
        _flags: u32,
    ) -> Result<Self::Handle, Errno> {
        Ok(HostBusSpaceHandle(addr))
    }

    fn bus_space_unmap(_t: Self::Tag, _h: Self::Handle, _size: BusSize) {}

    fn bus_space_read_1(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u8 {
        0
    }

    fn bus_space_read_2(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u16 {
        0
    }

    fn bus_space_read_4(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u32 {
        0
    }

    fn bus_space_write_1(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u8) {}

    fn bus_space_write_2(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u16) {}

    fn bus_space_write_4(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u32) {}

    fn bus_space_barrier(
        _t: Self::Tag,
        _h: Self::Handle,
        _offset: BusSize,
        _length: BusSize,
        _flags: u32,
    ) {
    }
}

impl DbMachdep for Machine {
    fn db_stack_trace_print(
        _addr: usize,
        _have_addr: bool,
        _count: usize,
        _modif: &[u8],
        pr: PrFn,
    ) {
        pr(format_args!("host: no stack trace\n"));
    }

    fn frame_address() -> usize {
        0
    }

    fn db_enter() {
        eprintln!("host: db_enter");
    }
}
