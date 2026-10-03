/*	$OpenBSD: fpu.h,v 1.20 2024/04/14 09:59:04 kettenis Exp $	*/
/*	$NetBSD: fpu.h,v 1.1 2003/04/26 18:39:40 fvdl Exp $	*/

//! amd64 `<machine/fpu.h>`: the floating-point/"extended state" save area.
//!
//! Upstream: sys/arch/amd64/include/fpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `struct fxsave64`, `struct xstate_hdr` and `struct
//! savefpu` (the `pcb` embeds one) and the initial control words. The FPU functions
//! (`fpuinit`, `fputrap`, `fpusave`, `xrstor_*`, `xsetbv_user`) and the `xsave_mask`
//! globals arrive with user-mode threads (M6), which are the first to use the FPU.
//!
//! If the CPU supports xsave/xrstor then we use them so that we can provide AVX support.
//! Otherwise we require fxsave/fxrstor, as the SSE registers are part of the ABI for
//! passing floating point values. While fxsave/fxrstor only required 16-byte alignment for
//! the save area, xsave/xrstor requires the save area to have 64-byte alignment.

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

const _: () = {
    assert!(core::mem::size_of::<Fxsave64>() == 512);
    assert!(core::mem::size_of::<XstateHdr>() == 64);
    assert!(core::mem::align_of::<Savefpu>() == 64);
};
