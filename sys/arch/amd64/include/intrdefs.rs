/* <CODE> */
//! amd64 `<machine/intrdefs.h>`: interrupt priority levels and interrupt source numbers.
//!
//! Upstream: sys/arch/amd64/include/intrdefs.h @ 3ce1f3f79392
//!
//! There are tty, network and disk drivers that use free() at interrupt time, so imp > (tty
//! | net | bio). Since run queues may be manipulated by both the statclock and tty, network,
//! and disk drivers, clock > imp. IPL_HIGH must block everything that can manipulate a run
//! queue. The level numbers are picked to fit into APIC vector priorities.
//!
//! Status: `ported`. Milestone M3 ports the levels and the source numbers; M4 the IDT gate
//! boundaries; M11a the `X86_IPI_*` numbers.

/// `IPL_NONE`: nothing.
pub const IPL_NONE: i32 = 0x0;
/// `IPL_SOFTCLOCK`: timeouts.
pub const IPL_SOFTCLOCK: i32 = 0x1;
/// `IPL_SOFTNET`: protocol stacks.
pub const IPL_SOFTNET: i32 = 0x2;
/// `IPL_BIO`: block I/O.
pub const IPL_BIO: i32 = 0x3;
/// `IPL_NET`: network.
pub const IPL_NET: i32 = 0x4;
/// `IPL_SOFTTTY`: delayed terminal handling.
pub const IPL_SOFTTTY: i32 = 0x8;
/// `IPL_TTY`: terminal.
pub const IPL_TTY: i32 = 0x9;
/// `IPL_VM`: memory allocation.
pub const IPL_VM: i32 = 0xa;
/// `IPL_AUDIO`: audio.
pub const IPL_AUDIO: i32 = 0xb;
/// `IPL_CLOCK`: clock.
pub const IPL_CLOCK: i32 = 0xc;
/// `IPL_SCHED`.
pub const IPL_SCHED: i32 = IPL_CLOCK;
/// `IPL_STATCLOCK`.
pub const IPL_STATCLOCK: i32 = IPL_CLOCK;
/// `IPL_HIGH`: everything.
pub const IPL_HIGH: i32 = 0xd;
/// `IPL_IPI`: inter-processor interrupts.
pub const IPL_IPI: i32 = 0xe;
/// `NIPL`: number of levels.
pub const NIPL: usize = 16;

/// `IPL_MPFLOOR`.
pub const IPL_MPFLOOR: i32 = IPL_TTY;
/// `IPL_MPSAFE`.
pub const IPL_MPSAFE: i32 = 0x100;
/// `IPL_WAKEUP`.
pub const IPL_WAKEUP: i32 = 0x200;

// Interrupt sharing types.

/// `IST_NONE`: none.
pub const IST_NONE: i32 = 0;
/// `IST_PULSE`: pulsed.
pub const IST_PULSE: i32 = 1;
/// `IST_EDGE`: edge-triggered.
pub const IST_EDGE: i32 = 2;
/// `IST_LEVEL`: level-triggered.
pub const IST_LEVEL: i32 = 3;

// Local APIC masks. Must not conflict with SIR_* above, and must be >= NUM_LEGACY_IRQs. Note
// that LIR_IPI must be first.

/// `LIR_IPI`.
pub const LIR_IPI: u32 = 63;
/// `LIR_TIMER`.
pub const LIR_TIMER: u32 = 62;

// Soft interrupt masks.

/// `SIR_XCALL`.
pub const SIR_XCALL: u32 = 61;
/// `SIR_CLOCK`.
pub const SIR_CLOCK: u32 = 60;
/// `SIR_NET`.
pub const SIR_NET: u32 = 59;
/// `SIR_TTY`.
pub const SIR_TTY: u32 = 58;

/// `LIR_XEN`.
pub const LIR_XEN: u32 = 57;
/// `LIR_HYPERV`.
pub const LIR_HYPERV: u32 = 56;

/// `MAX_INTR_SOURCES`: maximum # of interrupt sources per CPU. 64 to fit in one word. ioapics
/// can theoretically produce more, but it's not likely to happen. For multiple ioapics, things
/// can be routed to different CPUs.
pub const MAX_INTR_SOURCES: usize = 64;
/// `NUM_LEGACY_IRQS`.
pub const NUM_LEGACY_IRQS: usize = 16;

/// `IDT_INTR_LOW`: the first IDT vector the allocator hands out.
pub const IDT_INTR_LOW: i32 = 0x20 + NUM_LEGACY_IRQS as i32;
/// `IDT_INTR_HIGH`: the last.
pub const IDT_INTR_HIGH: i32 = 0xef;

/// `X86_IPI_HALT`: the inter-processor interrupt bits of `ci_ipis` (`x86_send_ipi`).
pub const X86_IPI_HALT: u32 = 0x0000_0001;
/// `X86_IPI_NOP`.
pub const X86_IPI_NOP: u32 = 0x0000_0002;
/// `X86_IPI_VMCLEAR_VMM`.
pub const X86_IPI_VMCLEAR_VMM: u32 = 0x0000_0004;
/// `X86_IPI_PCTR`.
pub const X86_IPI_PCTR: u32 = 0x0000_0010;
/// `X86_IPI_MTRR`.
pub const X86_IPI_MTRR: u32 = 0x0000_0020;
/// `X86_IPI_SETPERF`.
pub const X86_IPI_SETPERF: u32 = 0x0000_0040;
/// `X86_IPI_DDB`.
pub const X86_IPI_DDB: u32 = 0x0000_0080;
/// `X86_IPI_START_VMM`.
pub const X86_IPI_START_VMM: u32 = 0x0000_0100;
/// `X86_IPI_STOP_VMM`.
pub const X86_IPI_STOP_VMM: u32 = 0x0000_0200;
/// `X86_IPI_WBINVD`.
pub const X86_IPI_WBINVD: u32 = 0x0000_0400;
/// `X86_IPI_XCALL`.
pub const X86_IPI_XCALL: u32 = 0x0000_0800;

/// `X86_NIPI`: the size of `ipifunc[]`.
pub const X86_NIPI: usize = 13;

/// `IREENT_MAGIC`: what `tf_err` holds in a frame faked up by `Xrecurse_*`/`Xresume_*`.
pub const IREENT_MAGIC: i64 = 0x1804_1969;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/intrdefs.h");
        let ours: &[(&str, i64)] = &[
            ("IPL_NONE", i64::from(IPL_NONE)),
            ("IPL_SOFTCLOCK", i64::from(IPL_SOFTCLOCK)),
            ("IPL_SOFTNET", i64::from(IPL_SOFTNET)),
            ("IPL_BIO", i64::from(IPL_BIO)),
            ("IPL_NET", i64::from(IPL_NET)),
            ("IPL_SOFTTTY", i64::from(IPL_SOFTTTY)),
            ("IPL_TTY", i64::from(IPL_TTY)),
            ("IPL_VM", i64::from(IPL_VM)),
            ("IPL_AUDIO", i64::from(IPL_AUDIO)),
            ("IPL_CLOCK", i64::from(IPL_CLOCK)),
            ("IPL_HIGH", i64::from(IPL_HIGH)),
            ("IPL_IPI", i64::from(IPL_IPI)),
            ("NIPL", NIPL as i64),
            ("IPL_MPSAFE", i64::from(IPL_MPSAFE)),
            ("IPL_WAKEUP", i64::from(IPL_WAKEUP)),
            ("MAX_INTR_SOURCES", MAX_INTR_SOURCES as i64),
            ("NUM_LEGACY_IRQS", NUM_LEGACY_IRQS as i64),
            ("X86_IPI_HALT", i64::from(X86_IPI_HALT)),
            ("X86_IPI_NOP", i64::from(X86_IPI_NOP)),
            ("X86_IPI_VMCLEAR_VMM", i64::from(X86_IPI_VMCLEAR_VMM)),
            ("X86_IPI_PCTR", i64::from(X86_IPI_PCTR)),
            ("X86_IPI_MTRR", i64::from(X86_IPI_MTRR)),
            ("X86_IPI_SETPERF", i64::from(X86_IPI_SETPERF)),
            ("X86_IPI_DDB", i64::from(X86_IPI_DDB)),
            ("X86_IPI_START_VMM", i64::from(X86_IPI_START_VMM)),
            ("X86_IPI_STOP_VMM", i64::from(X86_IPI_STOP_VMM)),
            ("X86_IPI_WBINVD", i64::from(X86_IPI_WBINVD)),
            ("X86_IPI_XCALL", i64::from(X86_IPI_XCALL)),
            ("X86_NIPI", X86_NIPI as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
