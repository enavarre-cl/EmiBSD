//! `<machine/intr.h>` as a trait: the interrupt priority levels.
//!
//! Milestone M3 needs the `IPL_*` numbers that pools and mutexes carry around; `splraise`,
//! `spllower`, `splx`, `splassert` and the handler registration arrive with M4.

use crate::machine::Machine;

/// The interrupt priority levels of the selected architecture.
pub trait Intr {
    /// `IPL_NONE`: nothing.
    const IPL_NONE: i32;
    /// `IPL_SOFTCLOCK`: timeouts.
    const IPL_SOFTCLOCK: i32;
    /// `IPL_SOFTNET`: protocol stacks.
    const IPL_SOFTNET: i32;
    /// `IPL_SOFTTTY`: delayed terminal handling.
    const IPL_SOFTTTY: i32;
    /// `IPL_BIO`: block I/O.
    const IPL_BIO: i32;
    /// `IPL_NET`: network.
    const IPL_NET: i32;
    /// `IPL_TTY`: terminal.
    const IPL_TTY: i32;
    /// `IPL_VM`: memory allocation.
    const IPL_VM: i32;
    /// `IPL_AUDIO`: audio.
    const IPL_AUDIO: i32;
    /// `IPL_CLOCK`: clock.
    const IPL_CLOCK: i32;
    /// `IPL_SCHED`: the scheduler's level.
    const IPL_SCHED: i32;
    /// `IPL_STATCLOCK`: the statistics clock's level.
    const IPL_STATCLOCK: i32;
    /// `IPL_HIGH`: everything.
    const IPL_HIGH: i32;
    /// `IPL_IPI`: inter-processor interrupts.
    const IPL_IPI: i32;
    /// `IPL_MPFLOOR`: the lowest level that takes the kernel lock.
    const IPL_MPFLOOR: i32;
    /// `IPL_MPSAFE`: an 'mpsafe' interrupt, no kernel lock.
    const IPL_MPSAFE: i32;
    /// `IPL_WAKEUP`: a 'wakeup' interrupt.
    const IPL_WAKEUP: i32;
}

/// `IPL_NONE` on the selected machine.
pub const IPL_NONE: i32 = <Machine as Intr>::IPL_NONE;
/// `IPL_SOFTCLOCK` on the selected machine.
pub const IPL_SOFTCLOCK: i32 = <Machine as Intr>::IPL_SOFTCLOCK;
/// `IPL_SOFTNET` on the selected machine.
pub const IPL_SOFTNET: i32 = <Machine as Intr>::IPL_SOFTNET;
/// `IPL_SOFTTTY` on the selected machine.
pub const IPL_SOFTTTY: i32 = <Machine as Intr>::IPL_SOFTTTY;
/// `IPL_BIO` on the selected machine.
pub const IPL_BIO: i32 = <Machine as Intr>::IPL_BIO;
/// `IPL_NET` on the selected machine.
pub const IPL_NET: i32 = <Machine as Intr>::IPL_NET;
/// `IPL_TTY` on the selected machine.
pub const IPL_TTY: i32 = <Machine as Intr>::IPL_TTY;
/// `IPL_VM` on the selected machine.
pub const IPL_VM: i32 = <Machine as Intr>::IPL_VM;
/// `IPL_AUDIO` on the selected machine.
pub const IPL_AUDIO: i32 = <Machine as Intr>::IPL_AUDIO;
/// `IPL_CLOCK` on the selected machine.
pub const IPL_CLOCK: i32 = <Machine as Intr>::IPL_CLOCK;
/// `IPL_SCHED` on the selected machine.
pub const IPL_SCHED: i32 = <Machine as Intr>::IPL_SCHED;
/// `IPL_STATCLOCK` on the selected machine.
pub const IPL_STATCLOCK: i32 = <Machine as Intr>::IPL_STATCLOCK;
/// `IPL_HIGH` on the selected machine.
pub const IPL_HIGH: i32 = <Machine as Intr>::IPL_HIGH;
/// `IPL_IPI` on the selected machine.
pub const IPL_IPI: i32 = <Machine as Intr>::IPL_IPI;
/// `IPL_MPFLOOR` on the selected machine.
pub const IPL_MPFLOOR: i32 = <Machine as Intr>::IPL_MPFLOOR;
/// `IPL_MPSAFE` on the selected machine.
pub const IPL_MPSAFE: i32 = <Machine as Intr>::IPL_MPSAFE;
/// `IPL_WAKEUP` on the selected machine.
pub const IPL_WAKEUP: i32 = <Machine as Intr>::IPL_WAKEUP;
