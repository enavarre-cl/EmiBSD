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

/*
 * Copyright (c) 2013-2015 Stefan Fritsch <sf@sfritsch.de>
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
//! `viocon(4)`: the virtio console (`viocon* at virtio?`), a tty per port.
//!
//! Upstream: sys/dev/pv/viocon.c @ 3ce1f3f79392
//!
//! Neither GENERIC configures it (amd64's has the line commented out, `#viocon* at
//! virtio?`, and arm64's has none). The module compiles on every build (so clippy and the
//! host tests cover it); the cargo feature `viocon` stands for uncommenting that line: amd64's
//! `ioconf.rs` then has the entry and `conf.rs` cdevsw 94 its entry points (`NVIOCON` 1);
//! without it the cdevsw entry is undefined, as `NVIOCON` 0 makes it (`docs/ARCHITECTURE.md`,
//! "Cargo features").
//!
//! One port (port 0, queues 0 and 1; `VIRTIO_CONSOLE_F_MULTIPORT` is not negotiated). The
//! receive queue is kept full of [`BUFSIZE`]-byte buffers; its interrupt schedules a soft
//! interrupt that hands the bytes to the line discipline and refills the queue. Output takes
//! up to [`BUFSIZE`] bytes of the tty's output queue per transmit buffer; a full queue marks
//! the tty busy until the transmit interrupt frees buffers.
//!
//! ## Deviations
//! - The softc and the port are `Cell`s (all-zero valid, as the C's `M_ZERO` allocations);
//!   `sc_ports` is the `malloc`ed array of port pointers, `vp_rx`/`vp_tx` point into the
//!   `malloc`ed `sc_vqs`, `vp_tty` is `ttymalloc`'s (never freed: no detach).
//! - `dev2sc`/`dev2port` return `Option`s: the entry points answer `ENXIO` where the C would
//!   dereference a missing unit or port (`vioconclose`, `vioconread`, `vioconwrite`,
//!   `vioconioctl`, `viocontty`), and `vioconstart`/`vioconhwiflow` do nothing.
//! - The queue names (`"p%drx"`, `"p%dtx"`) are formatted into a stack buffer as the C's
//!   `snprintf` into `char name[6]`.
//! - `VIOCON_DEBUG`'s `DPRINTF`s are compiled behind `if VIOCON_DEBUG > 0`; the `NOTYET`
//!   members (`vp_host_open`, `vp_guest_open`, `vp_is_console`) are not compiled, as in C.
//! - The return values: `viocon_port_create` returns `Result` (`ENOMEM`; its other failures
//!   panic, as the C's `err:` does), `vioconparam`, `vioconopen`, ... return
//!   `Result<(), Errno>`; `vioconioctl`'s two `>= 0` tests are the `Ok(true)` of the line
//!   discipline's and `ttioctl`'s results.

use core::cell::Cell;
use core::ffi::c_void;
use core::fmt::Write as _;
use core::ptr::{self, NonNull};

use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_check_vq, virtio_dequeue, virtio_dequeue_commit,
    virtio_enqueue_commit, virtio_enqueue_p, virtio_enqueue_prep, virtio_enqueue_reserve,
    virtio_notify, virtio_start_vq_intr, virtio_stop_vq_intr,
};
use crate::dev::pv::virtioreg::PCI_PRODUCT_VIRTIO_CONSOLE;
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue,
    virtio_has_feature, virtio_negotiate_features, virtio_read_device_config_2,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_softintr::{SoftintrHand, softintr_establish, softintr_schedule};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::tty::{ttioctl, ttsetwater, ttwakeupwr, ttychars, ttyclose, ttymalloc};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::q_to_b;
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmamap, bus_dmamap_create, bus_dmamap_load,
    bus_dmamap_sync, bus_dmamem_alloc, bus_dmamem_map,
};
use crate::machine::intr::{IPL_TTY, splassert, spltty, splx};
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_TTY, DVF_ACTIVE, Device, Softc,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::termios::{CLOCAL, CRTSCTS, Termios};
use crate::sys::tty::{
    TS_BUSY, TS_FLUSH, TS_ISOPEN, TS_TIMEOUT, TS_TTSTOP, TS_WOPEN, TS_XCLUDE, TTHIWATMINSPACE, Tty,
};
use crate::sys::ttydefaults::{TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;

// features
/// `VIRTIO_CONSOLE_F_SIZE`.
pub const VIRTIO_CONSOLE_F_SIZE: u64 = 1 << 0;
/// `VIRTIO_CONSOLE_F_MULTIPORT`.
pub const VIRTIO_CONSOLE_F_MULTIPORT: u64 = 1 << 1;
/// `VIRTIO_CONSOLE_F_EMERG_WRITE`.
pub const VIRTIO_CONSOLE_F_EMERG_WRITE: u64 = 1 << 2;

// config space
/// `VIRTIO_CONSOLE_COLS`: 16 bits.
pub const VIRTIO_CONSOLE_COLS: i32 = 0;
/// `VIRTIO_CONSOLE_ROWS`: 16 bits.
pub const VIRTIO_CONSOLE_ROWS: i32 = 2;
/// `VIRTIO_CONSOLE_MAX_NR_PORTS`: 32 bits.
pub const VIRTIO_CONSOLE_MAX_NR_PORTS: i32 = 4;
/// `VIRTIO_CONSOLE_EMERG_WR`: 32 bits.
pub const VIRTIO_CONSOLE_EMERG_WR: i32 = 8;

/// `VIOCON_DEBUG`.
const VIOCON_DEBUG: i32 = 0;

/// `viocon_feature_names[]` with `VIRTIO_DEBUG`.
static VIOCON_FEATURE_NAMES_DEBUG: [VirtioFeatureName; 3] = [
    VirtioFeatureName {
        bit: VIRTIO_CONSOLE_F_SIZE,
        name: "Size",
    },
    VirtioFeatureName {
        bit: VIRTIO_CONSOLE_F_MULTIPORT,
        name: "MultiPort",
    },
    VirtioFeatureName {
        bit: VIRTIO_CONSOLE_F_EMERG_WRITE,
        name: "EmergWrite",
    },
];

// struct virtio_console_control's events
/// `VIRTIO_CONSOLE_DEVICE_READY`.
pub const VIRTIO_CONSOLE_DEVICE_READY: u16 = 0;
/// `VIRTIO_CONSOLE_PORT_ADD`.
pub const VIRTIO_CONSOLE_PORT_ADD: u16 = 1;
/// `VIRTIO_CONSOLE_PORT_REMOVE`.
pub const VIRTIO_CONSOLE_PORT_REMOVE: u16 = 2;
/// `VIRTIO_CONSOLE_PORT_READY`.
pub const VIRTIO_CONSOLE_PORT_READY: u16 = 3;
/// `VIRTIO_CONSOLE_CONSOLE_PORT`.
pub const VIRTIO_CONSOLE_CONSOLE_PORT: u16 = 4;
/// `VIRTIO_CONSOLE_RESIZE`.
pub const VIRTIO_CONSOLE_RESIZE: u16 = 5;
/// `VIRTIO_CONSOLE_PORT_OPEN`.
pub const VIRTIO_CONSOLE_PORT_OPEN: u16 = 6;
/// `VIRTIO_CONSOLE_PORT_NAME`.
pub const VIRTIO_CONSOLE_PORT_NAME: u16 = 7;

/// `BUFSIZE`: the bytes of one receive or transmit buffer.
pub const BUFSIZE: usize = 128;

/// `VIOCONUNIT(x)`.
#[inline]
pub const fn vioconunit(x: Dev) -> u32 {
    minor(x) >> 4
}

/// `VIOCONPORT(x)`.
#[inline]
pub const fn vioconport(x: Dev) -> u32 {
    minor(x) & 0x0f
}

/// `struct virtio_console_control`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioConsoleControl {
    /// `id`: port number.
    pub id: u32,
    /// `event`: `VIRTIO_CONSOLE_*`.
    pub event: u16,
    /// `value`.
    pub value: u16,
}

/// `struct virtio_console_control_resize`: yes, the order is different than in config space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioConsoleControlResize {
    /// `rows`.
    pub rows: u16,
    /// `cols`.
    pub cols: u16,
}

/// `struct viocon_port`.
///
/// Allocated zeroed (`M_ZERO`): every member is valid as all-zero bits.
pub struct VioconPort {
    /// `vp_sc`.
    pub vp_sc: Cell<*const VioconSoftc>,
    /// `vp_rx`.
    pub vp_rx: Cell<*const Virtqueue>,
    /// `vp_tx`.
    pub vp_tx: Cell<*const Virtqueue>,
    /// `vp_si`.
    pub vp_si: Cell<Option<NonNull<SoftintrHand>>>,
    /// `vp_tty`.
    pub vp_tty: Cell<*const Tty>,
    /// `vp_name`.
    pub vp_name: Cell<*const u8>,
    /// `vp_dmaseg`.
    pub vp_dmaseg: [Cell<BusDmaSegment>; 1],
    /// `vp_dmamap`.
    pub vp_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `vp_iflow`: rx flow control.
    pub vp_iflow: Cell<bool>,
    /// `vp_rows`.
    pub vp_rows: Cell<u16>,
    /// `vp_cols`.
    pub vp_cols: Cell<u16>,
    /// `vp_rx_buf`.
    pub vp_rx_buf: Cell<*mut u8>,
    /// `vp_tx_buf`.
    pub vp_tx_buf: Cell<*mut u8>,
}

impl VioconPort {
    /// `vp->vp_sc`.
    fn sc(&self) -> &'static VioconSoftc {
        // SAFETY: `viocon_port_create` sets it to the softc, which lives for good.
        match unsafe { self.vp_sc.get().as_ref() } {
            Some(sc) => sc,
            None => panic(format_args!("viocon: port without a softc")),
        }
    }

    /// `vp->vp_rx`.
    fn rx(&self) -> &'static Virtqueue {
        // SAFETY: a queue of the softc's `sc_vqs`, allocated for good.
        match unsafe { self.vp_rx.get().as_ref() } {
            Some(vq) => vq,
            None => panic(format_args!("viocon: port without its rx queue")),
        }
    }

    /// `vp->vp_tx`.
    fn tx(&self) -> &'static Virtqueue {
        // SAFETY: as for `rx`.
        match unsafe { self.vp_tx.get().as_ref() } {
            Some(vq) => vq,
            None => panic(format_args!("viocon: port without its tx queue")),
        }
    }

    /// `vp->vp_tty`.
    fn tty(&self) -> &'static Tty {
        // SAFETY: `ttymalloc`'s, never freed.
        match unsafe { self.vp_tty.get().as_ref() } {
            Some(tp) => tp,
            None => panic(format_args!("viocon: port without a tty")),
        }
    }

    /// `vp->vp_dmamap`.
    fn dmamap(&self) -> &'static BusDmamap {
        match self.vp_dmamap.get() {
            Some(map) => map,
            None => panic(format_args!("viocon: port without a dmamap")),
        }
    }

    /// `vp->vp_tx_buf - vp->vp_rx_buf`: where the transmit buffers start in the map.
    fn tx_off(&self) -> usize {
        self.vp_tx_buf.get() as usize - self.vp_rx_buf.get() as usize
    }
}

/// `struct viocon_softc`.
#[repr(C)]
pub struct VioconSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,

    /// `sc_c_vq_rx`: the control queues (F_MULTIPORT, not used).
    pub sc_c_vq_rx: Cell<*const Virtqueue>,
    /// `sc_c_vq_tx`.
    pub sc_c_vq_tx: Cell<*const Virtqueue>,

    /// `sc_max_ports`.
    pub sc_max_ports: Cell<u32>,
    /// `sc_ports`: `sc_max_ports` port pointers, `malloc`ed.
    pub sc_ports: Cell<*mut *mut VioconPort>,

    /// `sc_dmamap`.
    pub sc_dmamap: Cell<Option<&'static BusDmamap>>,
}

impl VioconSoftc {
    /// `sc->sc_virtio`.
    fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("viocon: no virtio softc")),
        }
    }

    /// `sc->sc_ports[i]`, when the port exists.
    fn port(&self, i: usize) -> Option<&'static VioconPort> {
        let ports = self.sc_ports.get();
        if ports.is_null() || i >= self.sc_max_ports.get() as usize {
            return None;
        }
        // SAFETY: `sc_ports` holds `sc_max_ports` pointers, null or to a port allocated for
        // good by `viocon_port_create`.
        unsafe { (*ports.add(i)).as_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a raw pointer or an `Option` of a reference.
unsafe impl Softc for VioconSoftc {}

/// `viocon_ca`.
pub static VIOCON_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VioconSoftc>(),
    ca_match: Some(viocon_match),
    ca_attach: viocon_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `viocon_cd`.
pub static VIOCON_CD: Cfdriver = Cfdriver::new(b"viocon", DV_TTY, CD_COCOVM);

/// `viocon_feature_names`, empty unless `VIRTIO_DEBUG`.
fn viocon_feature_names() -> &'static [VirtioFeatureName] {
    if VIRTIO_DEBUG > 0 {
        &VIOCON_FEATURE_NAMES_DEBUG
    } else {
        &[]
    }
}

/// `DPRINTF`.
fn dprintf(args: core::fmt::Arguments<'_>) {
    if VIOCON_DEBUG > 0 {
        printf(args);
    }
}

/// `dev2sc`: `viocon_cd.cd_devs[VIOCONUNIT(dev)]`.
fn dev2sc(dev: Dev) -> Option<&'static VioconSoftc> {
    let d = VIOCON_CD.cd_dev(vioconunit(dev) as i32)?;
    // SAFETY: `viocon_cd`'s devices were made from `viocon_ca` (`VioconSoftc`) and live for
    // good (no detach).
    Some(unsafe { d.as_ref().softc::<VioconSoftc>() })
}

/// `dev2port`: `dev2sc(dev)->sc_ports[VIOCONPORT(dev)]`.
fn dev2port(dev: Dev) -> Option<&'static VioconPort> {
    dev2sc(dev)?.port(vioconport(dev) as usize)
}

/// `(struct viocon_softc *)vsc->sc_child`.
fn viocon_child(vsc: &VirtioSoftc) -> &'static VioconSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("viocon: virtio without its child"));
    }
    // SAFETY: viocon_attach stored its own device, the head of a `VioconSoftc` that lives as
    // long as the kernel.
    unsafe { &*c.cast::<VioconSoftc>() }
}

/// `viocon_match`: the virtio console.
pub fn viocon_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };
    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_CONSOLE)
}

/// `viocon_attach`.
pub fn viocon_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `viocon_ca`, whose softc is a `VioconSoftc`, never freed.
    let sc: &'static VioconSoftc = unsafe { &*ptr::from_ref(self_.softc::<VioconSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `viocon* at virtio?`: the parent's softc begins with the `struct virtio_softc`.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();
    let maxports: usize = 1;
    let vqs_size = 2 * (maxports + 1) * size_of::<Virtqueue>();
    let ports_size = maxports * size_of::<*mut VioconPort>();

    if !vsc.sc_child.get().is_null() {
        panic(format_args!("already attached to something else"));
    }
    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_TTY);
    sc.sc_virtio.set(Some(vsc));
    sc.sc_max_ports.set(maxports as u32);

    let vqs = malloc(vqs_size, M_DEVBUF, M_WAITOK | M_CANFAIL | M_ZERO);
    let ports = malloc(ports_size, M_DEVBUF, M_WAITOK | M_CANFAIL | M_ZERO);
    vsc.sc_vqs
        .set(vqs.map_or(ptr::null_mut(), |p| p.as_ptr().cast()));
    sc.sc_ports
        .set(ports.map_or(ptr::null_mut(), |p| p.as_ptr().cast()));

    let attached = 'err: {
        if vqs.is_none() || ports.is_none() {
            printf(format_args!("\nviocon_attach: Cannot allocate memory\n"));
            break 'err false;
        }

        vsc.sc_driver_features.set(VIRTIO_CONSOLE_F_SIZE);
        if virtio_negotiate_features(vsc, Some(viocon_feature_names())).is_err() {
            break 'err false;
        }

        printf(format_args!("\n"));
        dprintf(format_args!("viocon_attach: softc: {:p}\n", sc));
        if viocon_port_create(sc, 0).is_err() {
            printf(format_args!("\nviocon_attach: viocon_port_create failed\n"));
            break 'err false;
        }
        if virtio_attach_finish(vsc, va).is_err() {
            break 'err false;
        }
        if let Some(vp) = sc.port(0) {
            viocon_rx_fill(vp);
        }
        true
    };
    if attached {
        return;
    }

    // err:
    vsc.sc_child.set(VIRTIO_CHILD_ERROR);
    vsc.sc_vqs.set(ptr::null_mut());
    vsc.sc_nvqs.set(0);
    if let Some(p) = vqs {
        free(p, M_DEVBUF, vqs_size);
    }
    sc.sc_ports.set(ptr::null_mut());
    if let Some(p) = ports {
        free(p, M_DEVBUF, ports_size);
    }
}

/// `viocon_port_create`: port `portidx`'s queues, buffers and tty.
pub fn viocon_port_create(sc: &'static VioconSoftc, portidx: usize) -> Result<(), Errno> {
    let vsc = sc.virtio();

    let Some(vpp) = malloc(
        size_of::<VioconPort>(),
        M_DEVBUF,
        M_WAITOK | M_CANFAIL | M_ZERO,
    ) else {
        return Err(Errno::ENOMEM);
    };
    let vpp = vpp.cast::<VioconPort>();
    // SAFETY: `sc_ports` holds `sc_max_ports` slots and `portidx` is below it (the attach
    // creates port 0 of 1).
    unsafe { *sc.sc_ports.get().add(portidx) = vpp.as_ptr() };
    // SAFETY: a zeroed allocation of a `VioconPort` (all-zero valid), never freed.
    let vp: &'static VioconPort = unsafe { vpp.as_ref() };
    vp.vp_sc.set(ptr::from_ref(sc));
    dprintf(format_args!("viocon_port_create: vp: {:p}\n", vp));

    let rxidx = if portidx == 0 { 0 } else { 2 * (portidx + 1) };
    let txidx = rxidx + 1;

    'err: {
        let mut name = NameBuf::default();
        let _ = write!(name, "p{portidx}rx");
        if virtio_alloc_vq(vsc, vsc.vq(rxidx), rxidx as i32, 1, name.as_str()).is_err() {
            printf(format_args!("\nCan't alloc {} virtqueue\n", name.as_str()));
            break 'err;
        }
        vp.vp_rx.set(vsc.vq(rxidx));
        vp.rx().vq_done.set(Some(viocon_rx_intr));
        vp.vp_si.set(softintr_establish(
            IPL_TTY,
            viocon_rx_soft,
            ptr::from_ref(vp).cast_mut().cast(),
        ));
        dprintf(format_args!("viocon_port_create: rx: {:p}\n", vp.rx()));

        let mut name = NameBuf::default();
        let _ = write!(name, "p{portidx}tx");
        if virtio_alloc_vq(vsc, vsc.vq(txidx), txidx as i32, 1, name.as_str()).is_err() {
            printf(format_args!("\nCan't alloc {} virtqueue\n", name.as_str()));
            break 'err;
        }
        vp.vp_tx.set(vsc.vq(txidx));
        vp.tx().vq_done.set(Some(viocon_tx_intr));
        dprintf(format_args!("viocon_port_create: tx: {:p}\n", vp.tx()));

        vsc.sc_nvqs.set(vsc.sc_nvqs.get() + 2);

        let allocsize = (vp.rx().vq_num.get() + vp.tx().vq_num.get()) as usize * BUFSIZE;

        let Ok(map) = bus_dmamap_create(
            vsc.dmat(),
            allocsize,
            1,
            allocsize,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) else {
            break 'err;
        };
        vp.vp_dmamap.set(Some(map));
        let mut segs = [BusDmaSegment::default()];
        let Ok(nsegs) = bus_dmamem_alloc(
            vsc.dmat(),
            allocsize,
            8,
            0,
            &mut segs,
            BUS_DMA_NOWAIT | BUS_DMA_ZERO,
        ) else {
            break 'err;
        };
        vp.vp_dmaseg[0].set(segs[0]);
        let Ok(kva) = bus_dmamem_map(vsc.dmat(), &mut segs[..nsegs], allocsize, BUS_DMA_NOWAIT)
        else {
            break 'err;
        };
        // SAFETY: `kva` maps `allocsize` bytes of DMA memory kept for good.
        if unsafe {
            bus_dmamap_load(
                vsc.dmat(),
                map,
                kva.as_ptr(),
                allocsize,
                None,
                BUS_DMA_NOWAIT,
            )
        }
        .is_err()
        {
            break 'err;
        }
        vp.vp_rx_buf.set(kva.as_ptr());
        // XXX use only a small circular tx buffer instead of many BUFSIZE buffers?
        // SAFETY: the transmit buffers follow the receive ones inside the allocation.
        vp.vp_tx_buf
            .set(unsafe { kva.as_ptr().add(vp.rx().vq_num.get() as usize * BUFSIZE) });

        if virtio_has_feature(vsc, VIRTIO_CONSOLE_F_SIZE) {
            vp.vp_cols
                .set(virtio_read_device_config_2(vsc, VIRTIO_CONSOLE_COLS));
            vp.vp_rows
                .set(virtio_read_device_config_2(vsc, VIRTIO_CONSOLE_ROWS));
        }

        let tp = ttymalloc(1_000_000);
        tp.t_oproc.set(Some(vioconstart));
        tp.t_param.set(Some(vioconparam));
        tp.t_hwiflow.set(Some(vioconhwiflow));
        tp.t_dev
            .set(((sc.sc_dev.dv_unit.get() as u32) << 4 | portidx as u32) as Dev);
        vp.vp_tty.set(tp);
        dprintf(format_args!("viocon_port_create: tty: {:p}\n", tp));

        virtio_start_vq_intr(vsc, vp.rx());
        virtio_start_vq_intr(vsc, vp.tx());

        return Ok(());
    }
    // err:
    panic(format_args!("viocon_port_create failed"));
}

/// `char name[6]` for `snprintf`.
#[derive(Default)]
struct NameBuf {
    /// The bytes.
    buf: [u8; 6],
    /// How many are used (at most 5: `snprintf` keeps one for the NUL).
    len: usize,
}

impl NameBuf {
    /// The formatted name.
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for NameBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.len + 1 < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

/// `viocon_tx_drain`: takes the sent buffers off `vq`; how many.
pub fn viocon_tx_drain(vp: &VioconPort, vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let mut ndone = 0;

    splassert(IPL_TTY, "viocon_tx_drain");
    while let Ok((slot, _len)) = virtio_dequeue(vsc, vq) {
        bus_dmamap_sync(
            vsc.dmat(),
            vp.dmamap(),
            vp.tx_off() + slot as usize * BUFSIZE,
            BUFSIZE,
            BUS_DMASYNC_POSTREAD,
        );
        virtio_dequeue_commit(vq, slot);
        ndone += 1;
    }
    ndone
}

/// `viocon_tx_intr`: the transmit queue's `vq_done`.
pub fn viocon_tx_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viocon_child(vsc);
    let portidx = ((vq.vq_index.get() - 1) / 2) as usize;
    let Some(vp) = sc.port(portidx) else {
        panic(format_args!("viocon: no port {portidx}"));
    };
    let tp = vp.tty();

    splassert(IPL_TTY, "viocon_tx_intr");
    let ndone = viocon_tx_drain(vp, vq);
    if ndone != 0 && tp.t_state_isset(TS_BUSY) {
        tp.t_state_clr(TS_BUSY);
        (linesw(tp).l_start)(tp);
    }

    1
}

/// `viocon_rx_fill`: puts every free receive buffer on the queue.
pub fn viocon_rx_fill(vp: &VioconPort) {
    let vq = vp.rx();
    let vsc = vp.sc().virtio();
    let mut ndone = 0;

    let r = loop {
        let slot = match virtio_enqueue_prep(vq) {
            Ok(slot) => slot,
            Err(e) => break Err(e),
        };
        if virtio_enqueue_reserve(vq, slot, 1).is_err() {
            break Ok(());
        }
        bus_dmamap_sync(
            vsc.dmat(),
            vp.dmamap(),
            slot as usize * BUFSIZE,
            BUFSIZE,
            BUS_DMASYNC_PREREAD,
        );
        virtio_enqueue_p(
            vq,
            slot,
            vp.dmamap(),
            slot as usize * BUFSIZE,
            BUFSIZE,
            false,
        );
        virtio_enqueue_commit(vsc, vq, slot, false);
        ndone += 1;
    };
    kassert!(matches!(r, Ok(()) | Err(Errno::EAGAIN)));
    if ndone > 0 {
        virtio_notify(vsc, vq);
    }
}

/// `viocon_rx_intr`: the receive queue's `vq_done`: the soft interrupt does the work.
pub fn viocon_rx_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viocon_child(vsc);
    let portidx = ((vq.vq_index.get() - 1) / 2) as usize;
    let Some(vp) = sc.port(portidx) else {
        panic(format_args!("viocon: no port {portidx}"));
    };

    if let Some(si) = vp.vp_si.get() {
        softintr_schedule(si);
    }
    1
}

/// `viocon_rx_soft`: hands the received bytes to the line discipline and refills the queue.
pub fn viocon_rx_soft(arg: *mut c_void) {
    // SAFETY: established with the port, allocated for good.
    let vp = unsafe { &*arg.cast_const().cast::<VioconPort>() };
    let vq = vp.rx();
    let vsc = vq.owner();
    let tp = vp.tty();

    while !vp.vp_iflow.get() {
        let Ok((slot, len)) = virtio_dequeue(vsc, vq) else {
            break;
        };
        bus_dmamap_sync(
            vsc.dmat(),
            vp.dmamap(),
            slot as usize * BUFSIZE,
            BUFSIZE,
            BUS_DMASYNC_POSTREAD,
        );
        let base = slot as usize * BUFSIZE;
        for i in 0..len.max(0) as usize {
            // SAFETY: the receive buffers are `vq_num * BUFSIZE` bytes of the mapped area;
            // the device wrote `len` (at most BUFSIZE) bytes of slot `slot`, read volatile.
            let c = unsafe { ptr::read_volatile(vp.vp_rx_buf.get().add(base + i)) };
            (linesw(tp).l_rint)(i32::from(c), tp);
        }
        virtio_dequeue_commit(vq, slot);
    }

    viocon_rx_fill(vp);
}

/// `vioconstart`: puts the tty's output on the transmit queue.
pub fn vioconstart(tp: &Tty) {
    let (Some(sc), Some(vp)) = (dev2sc(tp.t_dev.get()), dev2port(tp.t_dev.get())) else {
        return;
    };
    let vsc = sc.virtio();
    let vq = vp.tx();

    let s = spltty();

    'out: {
        let ndone = viocon_tx_drain(vp, vq);
        if tp.t_state_isset(TS_BUSY) {
            if ndone > 0 {
                tp.t_state_clr(TS_BUSY);
            } else {
                break 'out;
            }
        }
        if tp.t_state_isset(TS_TIMEOUT | TS_TTSTOP) {
            break 'out;
        }

        if tp.t_outq.c_cc.get() == 0 {
            break 'out;
        }
        let mut ndone = 0;

        let mut ret = Ok(0);
        while tp.t_outq.c_cc.get() > 0 {
            ret = virtio_enqueue_prep(vq);
            let slot = match ret {
                Err(Errno::EAGAIN) => break,
                Err(_) => panic(format_args!("vioconstart: virtio_enqueue_prep")),
                Ok(slot) => slot,
            };
            let r = virtio_enqueue_reserve(vq, slot, 1);
            kassert!(r.is_ok());
            let off = vp.tx_off() + slot as usize * BUFSIZE;
            // SAFETY: slot `slot`'s transmit buffer, BUFSIZE bytes of the mapped area, which
            // only this function writes, at spltty.
            let buf =
                unsafe { core::slice::from_raw_parts_mut(vp.vp_rx_buf.get().add(off), BUFSIZE) };
            let cnt = q_to_b(&tp.t_outq, buf);
            bus_dmamap_sync(vsc.dmat(), vp.dmamap(), off, cnt, BUS_DMASYNC_PREWRITE);
            virtio_enqueue_p(vq, slot, vp.dmamap(), off, cnt, true);
            virtio_enqueue_commit(vsc, vq, slot, false);
            ndone += 1;
        }
        if ret == Err(Errno::EAGAIN) {
            tp.t_state_set(TS_BUSY);
        }
        if ndone > 0 {
            virtio_notify(vsc, vq);
        }
        ttwakeupwr(tp);
    }
    splx(s);
}

/// `vioconhwiflow`: stops or restarts the receive queue for the line discipline.
pub fn vioconhwiflow(tp: &Tty, stop: i32) -> i32 {
    let Some(vp) = dev2port(tp.t_dev.get()) else {
        return 1;
    };
    let vsc = vp.sc().virtio();

    let s = spltty();
    vp.vp_iflow.set(stop != 0);
    if stop != 0 {
        virtio_stop_vq_intr(vsc, vp.rx());
    } else {
        virtio_start_vq_intr(vsc, vp.rx());
        virtio_check_vq(vsc, vp.rx());
    }
    splx(s);
    1
}

/// `vioconparam`: takes the speeds and flags as they are and starts output.
pub fn vioconparam(tp: &Tty, t: &Termios) -> Result<(), Errno> {
    tp.set_t_ispeed(t.c_ispeed);
    tp.set_t_ospeed(t.c_ospeed);
    tp.set_t_cflag(t.c_cflag);

    vioconstart(tp);
    Ok(())
}

/// `vioconopen`.
pub fn vioconopen(dev: Dev, _flag: i32, _mode: i32, p: &Proc) -> Result<(), Errno> {
    let unit = vioconunit(dev) as i32;
    let port = vioconport(dev);

    if unit >= VIOCON_CD.cd_ndevs.get() {
        return Err(Errno::ENXIO);
    }
    let Some(sc) = dev2sc(dev) else {
        return Err(Errno::ENXIO);
    };
    if sc.sc_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return Err(Errno::ENXIO);
    }

    let s = spltty();
    if port >= sc.sc_max_ports.get() {
        splx(s);
        return Err(Errno::ENXIO);
    }
    let Some(vp) = sc.port(port as usize) else {
        splx(s);
        return Err(Errno::ENXIO);
    };
    let tp = vp.tty();
    splx(s);

    if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_WOPEN);
        ttychars(tp);
        tp.set_t_ispeed(1_000_000);
        tp.set_t_ospeed(1_000_000);
        tp.set_t_cflag(TTYDEF_CFLAG | CLOCAL | CRTSCTS);
        tp.set_t_iflag(TTYDEF_IFLAG);
        tp.set_t_oflag(TTYDEF_OFLAG);
        tp.set_t_lflag(TTYDEF_LFLAG);
        if vp.vp_cols.get() != 0 {
            let mut ws = tp.t_winsize.get();
            ws.ws_col = vp.vp_cols.get();
            ws.ws_row = vp.vp_rows.get();
            tp.t_winsize.set(ws);
        }

        let s = spltty();
        let _ = vioconparam(tp, &tp.t_termios.get());
        ttsetwater(tp);
        splx(s);
    } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
        return Err(Errno::EBUSY);
    }

    (linesw(tp).l_open)(dev, tp, p)
}

/// `vioconclose`.
pub fn vioconclose(dev: Dev, flag: i32, _mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let Some(vp) = dev2port(dev) else {
        return Err(Errno::ENXIO);
    };
    let tp = vp.tty();

    if !tp.t_state_isset(TS_ISOPEN) {
        return Ok(());
    }

    let _ = (linesw(tp).l_close)(tp, flag, p);
    let s = spltty();
    tp.t_state_clr(TS_BUSY | TS_FLUSH);
    let _ = ttyclose(tp);
    splx(s);

    Ok(())
}

/// `vioconread`.
pub fn vioconread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(vp) = dev2port(dev) else {
        return Err(Errno::ENXIO);
    };
    let tp = vp.tty();

    (linesw(tp).l_read)(tp, uio, flag)
}

/// `vioconwrite`.
pub fn vioconwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(vp) = dev2port(dev) else {
        return Err(Errno::ENXIO);
    };
    let tp = vp.tty();

    (linesw(tp).l_write)(tp, uio, flag)
}

/// `viocontty`.
pub fn viocontty(dev: Dev) -> Option<&'static Tty> {
    Some(dev2port(dev)?.tty())
}

/// `vioconstop`: a busy port's pending output is to be flushed.
pub fn vioconstop(tp: &Tty, _flag: i32) -> Result<(), Errno> {
    let s = spltty();
    if tp.t_state_isset(TS_BUSY) && !tp.t_state_isset(TS_TTSTOP) {
        tp.t_state_set(TS_FLUSH);
    }
    splx(s);
    Ok(())
}

/// `vioconioctl`.
pub fn vioconioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let Some(vp) = dev2port(dev) else {
        return Err(Errno::ENXIO);
    };
    let tp = vp.tty();

    if (linesw(tp).l_ioctl)(tp, cmd, data, flag, p)? {
        return Ok(());
    }
    if ttioctl(tp, cmd, data, flag, p)? {
        return Ok(());
    }
    Err(Errno::ENOTTY)
}

// CTASSERT(BUFSIZE < TTHIWATMINSPACE)
const _: () = assert!((BUFSIZE as i32) < TTHIWATMINSPACE);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of viocon(4): the minor number's unit and port, and the queue names.

    use std::assert_eq;

    use super::*;

    #[test]
    fn minor_splits_into_unit_and_port() {
        assert_eq!(vioconunit(0x00), 0);
        assert_eq!(vioconport(0x00), 0);
        assert_eq!(vioconunit(0x13), 1);
        assert_eq!(vioconport(0x13), 3);
        assert_eq!(vioconport(0x0f), 15);
    }

    #[test]
    fn queue_names_fit_six_bytes() {
        let mut n = NameBuf::default();
        let _ = write!(n, "p{}rx", 0);
        assert_eq!(n.as_str(), "p0rx");
        let mut n = NameBuf::default();
        let _ = write!(n, "p{}tx", 12);
        // snprintf into char[6] keeps five characters.
        assert_eq!(n.as_str(), "p12tx");
        let mut n = NameBuf::default();
        let _ = write!(n, "p{}tx", 123);
        assert_eq!(n.as_str(), "p123t");
    }
}
/* </TESTS> */
