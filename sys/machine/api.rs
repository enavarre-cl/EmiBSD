//! Traits every architecture implements.
//!
//! Grows with the milestones: M0 adds `Cpu`, `Console` and `Exit`; M3 adds `Pmap`; M4 adds
//! `Intr`/`Spl` and trap frames. Keep each trait small and named after the OpenBSD header or
//! `(9)` interface it stands in for.

/// Identity of the running architecture.
pub trait MachineInfo {
    /// The architecture name as OpenBSD spells it: `"amd64"` or `"arm64"`; `"host"` for the
    /// test double.
    const MACHINE: &'static str;
}
