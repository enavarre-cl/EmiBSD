/*	$OpenBSD: fpu.h,v 1.20 2024/04/14 09:59:04 kettenis Exp $	*/
/*	$NetBSD: fpu.h,v 1.1 2003/04/26 18:39:40 fvdl Exp $	*/

//! amd64 `<machine/fpu.h>`: the floating-point/"extended state" save area.
//!
//! Upstream: sys/arch/amd64/include/fpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `struct fxsave64`, `struct xstate_hdr` and `struct
//! savefpu` (the `pcb` embeds one) and the initial control words; M8 the FPU interface:
//! `fpu_cleandata`, `fpureset`, the `locore.S` routines (`xrstor_user`, `xrstor_kern`,
//! `fpusave`, `fpusavereset`) and the instruction macros (`fninit`, `fwait`, `fxsave`,
//! `ldmxcsr`, `fldcw`). `fpuinit`, `fputrap` and the globals are `amd64/fpu.rs`'s.
//! `xsetbv_user` waits for the XSAVE support (see `amd64/fpu.rs`).
//!
//! If the CPU supports xsave/xrstor then we use them so that we can provide AVX support.
//! Otherwise we require fxsave/fxrstor, as the SSE registers are part of the ABI for
//! passing floating point values. While fxsave/fxrstor only required 16-byte alignment for
//! the save area, xsave/xrstor requires the save area to have 64-byte alignment.

use core::arch::asm;
use core::ptr;

/// `struct fxsave64`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fxsave64 {
    /// `fx_fcw`.
    pub fx_fcw: u16,
    /// `fx_fsw`.
    pub fx_fsw: u16,
    /// `fx_ftw`.
    pub fx_ftw: u8,
    /// `fx_unused1`.
    pub fx_unused1: u8,
    /// `fx_fop`.
    pub fx_fop: u16,
    /// `fx_rip`.
    pub fx_rip: u64,
    /// `fx_rdp`.
    pub fx_rdp: u64,
    /// `fx_mxcsr`.
    pub fx_mxcsr: u32,
    /// `fx_mxcsr_mask`.
    pub fx_mxcsr_mask: u32,
    /// `fx_st`: 8 normal FP regs.
    pub fx_st: [[u64; 2]; 8],
    /// `fx_xmm`: 16 SSE2 registers.
    pub fx_xmm: [[u64; 2]; 16],
    /// `fx_unused3`.
    pub fx_unused3: [u8; 96],
}

/// `struct xstate_hdr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct XstateHdr {
    /// `xstate_bv`.
    pub xstate_bv: u64,
    /// `xstate_xcomp_bv`.
    pub xstate_xcomp_bv: u64,
    /// `xstate_rsrv0`.
    pub xstate_rsrv0: [u8; 8],
    /// `xstate_rsrv`.
    pub xstate_rsrv: [u8; 40],
}

/// `struct savefpu`: 64-byte aligned, as xsave requires.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct Savefpu {
    /// `fp_fxsave`: see above.
    pub fp_fxsave: Fxsave64,
    /// `fp_xstate`.
    pub fp_xstate: XstateHdr,
    /// `fp_ymm`.
    pub fp_ymm: [[u64; 2]; 16],
    /// `fp_components`: enough for AVX-512.
    pub fp_components: [u8; 1856],
}

impl Savefpu {
    /// An all-zero save area.
    pub const fn zeroed() -> Self {
        Self {
            fp_fxsave: Fxsave64 {
                fx_fcw: 0,
                fx_fsw: 0,
                fx_ftw: 0,
                fx_unused1: 0,
                fx_fop: 0,
                fx_rip: 0,
                fx_rdp: 0,
                fx_mxcsr: 0,
                fx_mxcsr_mask: 0,
                fx_st: [[0; 2]; 8],
                fx_xmm: [[0; 2]; 16],
                fx_unused3: [0; 96],
            },
            fp_xstate: XstateHdr {
                xstate_bv: 0,
                xstate_xcomp_bv: 0,
                xstate_rsrv0: [0; 8],
                xstate_rsrv: [0; 40],
            },
            fp_ymm: [[0; 2]; 16],
            fp_components: [0; 1856],
        }
    }
}

/// `__INITIAL_NPXCW__`: the i387 defaults to Intel extended precision mode and round to
/// nearest, with all exceptions masked.
pub const INITIAL_NPXCW: u16 = 0x037f;
/// `__INITIAL_MXCSR__`.
pub const INITIAL_MXCSR: u32 = 0x1f80;
/// `__INITIAL_MXCSR_MASK__`.
pub const INITIAL_MXCSR_MASK: u32 = 0xffbf;

unsafe extern "C" {
    /// `xrstor_user(addr, mask)`: loads the state at `addr`, which might not be trustable
    /// (a `sigreturn`): a #GP is caught (`trap0d`); returns 0 if it loaded, 1 if it trapped.
    /// The `fxrstor` form: `mask` is unused until XSAVE.
    pub fn xrstor_user(addr: *const Savefpu, mask: u64) -> i32;
    /// `xrstor_kern(addr, mask)`: loads the state at `addr`, assumed trusted (unaltered
    /// since the kernel saved it).
    pub fn xrstor_kern(addr: *const Savefpu, mask: u64);
    /// `fpusave(addr)`: saves the current state, but retains it in the FPU.
    pub fn fpusave(addr: *mut Savefpu);
    /// `fpusavereset(addr)`: saves the current state and resets the FPU to the initial
    /// (kernel) state.
    pub fn fpusavereset(addr: *mut Savefpu);
}

/// `fpu_cleandata`: the save area with everything reset (proc0's, filled by `cpu_init`).
pub fn fpu_cleandata() -> *mut Savefpu {
    crate::arch::amd64::amd64::machdep::proc0paddr()
        .u_pcb
        .pcb_savefpu
        .get()
}

/// `fpureset()`: loads the clean state.
pub fn fpureset() {
    let mask =
        crate::arch::amd64::amd64::fpu::XSAVE_MASK.load(core::sync::atomic::Ordering::Relaxed);
    // SAFETY: fpu_cleandata is a kernel-made save area, valid for the kernel's lifetime.
    unsafe { xrstor_kern(fpu_cleandata(), mask) };
}

/// `fninit()`.
#[inline]
pub fn fninit() {
    // SAFETY: resets the x87 unit's control, status and tag words; touches no memory.
    unsafe { asm!("fninit", options(nomem, nostack)) };
}

/// `fwait()`.
#[inline]
pub fn fwait() {
    // SAFETY: waits for pending x87 exceptions; touches no memory.
    unsafe { asm!("fwait", options(nomem, nostack)) };
}

/// `fxsave(addr)`: "should be fxsave64, but where we use this it doesn't matter". Takes the
/// `savefpu` whose `fp_fxsave` (at offset 0) it fills: `struct fxsave64` is packed here, and
/// the instruction needs the 16-byte alignment the C's `aligned(16)` local gives.
#[inline]
pub fn fxsave(addr: &mut Savefpu) {
    // SAFETY: writes the first 512 bytes of `addr`, its `fp_fxsave`, 64-byte aligned.
    unsafe { asm!("fxsave [{}]", in(reg) ptr::from_mut(addr), options(nostack)) };
}

/// `ldmxcsr(addr)`.
#[inline]
pub fn ldmxcsr(addr: &u32) {
    // SAFETY: loads MXCSR from `addr`; the callers pass a value with the reserved bits clear.
    unsafe { asm!("ldmxcsr [{}]", in(reg) ptr::from_ref(addr), options(nostack, readonly)) };
}

/// `fldcw(addr)`.
#[inline]
pub fn fldcw(addr: &u16) {
    // SAFETY: loads the x87 control word from `addr`.
    unsafe { asm!("fldcw [{}]", in(reg) ptr::from_ref(addr), options(nostack, readonly)) };
}

const _: () = {
    assert!(core::mem::size_of::<Fxsave64>() == 512);
    assert!(core::mem::size_of::<XstateHdr>() == 64);
    assert!(core::mem::align_of::<Savefpu>() == 64);
};
