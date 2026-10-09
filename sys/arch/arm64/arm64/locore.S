/*-
 * Copyright (c) 2012-2014 Andrew Turner
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: head/sys/arm64/arm64/locore.S 282867 2015-05-13 18:57:03Z zbb $
 */

/*
 * The ported part of locore.S (see locore.rs): what locore0.S's boot(8) entry and the
 * application processors' entry use (drop_to_el1, get_virt_delta, start_mmu, the boot page
 * tables, initstack), the signal trampoline, and cpu_hatch_secondary. The symbols written
 * as NAME between braces are filled in by locore.rs from the Rust definitions (assym.h,
 * <machine/armreg.h>, <machine/hypervisor.h>, <sys/syscall.h> in C). No RETGUARD and no
 * CODEPATCH: neither exists in this kernel (locore.rs, deviations).
 */

/*
 * If we are started in EL2, configure the required hypervisor
 * registers and drop to EL1.
 */
	.text
	.globl drop_to_el1
drop_to_el1:
	mrs	x1, CurrentEL
	lsr	x1, x1, #2
	cmp	x1, #0x2
	b.eq	2f
1:
	ret
2:
	/*
	 * Check for EL2 Host mode by testing aliasing between
	 * EL1 and EL2 accessors of the FAR_EL2 register.
	 */
	msr	hcr_el2, xzr
	isb
	mov	x2, #1
	msr	far_el1, x2
	isb
	mov	x2, #2
	msr	far_el2, x2
	isb
	mrs	x2, far_el1
	cmp	x2, #2
	b.ne	3f

	ldr	x2, .Lhcr_host
	msr	hcr_el2, x2
	isb
	b	1b

3:
	/* Configure the Hypervisor */
	ldr	x2, .Lhcr_guest
	msr	hcr_el2, x2

	/* Load the Virtualization Process ID Register */
	mrs	x2, midr_el1
	msr	vpidr_el2, x2

	/* Load the Virtualization Multiprocess ID Register */
	mrs	x2, mpidr_el1
	msr	vmpidr_el2, x2

	/* Set the bits that need to be 1 in sctlr_el1 */
	ldr	x2, .Lsctlr_res1
	msr	sctlr_el1, x2

	/* Don't trap to EL2 for exceptions */
	mov	x2, #{CPTR_RES1}
	msr	cptr_el2, x2

	/* Don't trap to EL2 for CP15 traps */
	msr	hstr_el2, xzr

	/* Enable access to the physical timers at EL1 */
	mov	x2, #{CNTHCTL_EL1}
	msr	cnthctl_el2, x2

	/* Set the counter offset to a known value */
	msr	cntvoff_el2, xzr

	/* Hypervisor trap functions (adr is the physical address: the MMU is off) */
	adr	x2, hyp_vectors
	msr	vbar_el2, x2

	mov	x2, #{PSR_EL1H_MASKED}
	msr	spsr_el2, x2

	/* Configure GICv3 CPU interface */
	mrs	x2, id_aa64pfr0_el1
	/* Extract GIC bits from the register */
	ubfx	x2, x2, #{ID_AA64PFR0_GIC_SHIFT}, #{ID_AA64PFR0_GIC_BITS}
	/* GIC[3:0] == 0001 - GIC CPU interface via special regs. supported */
	cmp	x2, #{ID_AA64PFR0_GIC_CPUIF_EN}
	b.ne	4f

	mrs	x2, icc_sre_el2
	orr	x2, x2, #{ICC_SRE_EL2_EN}	/* Enable access from insecure EL1 */
	orr	x2, x2, #{ICC_SRE_EL2_SRE}	/* Enable system registers */
	msr	icc_sre_el2, x2
4:

	/* Set the address to return to our return address */
	msr	elr_el2, x30
	isb

	eret
	dsb	nsh
	isb

	.align 3
.Lsctlr_res1:
	.quad {SCTLR_RES1}
.Lhcr_host:
	.quad {HCR_HOST}
.Lhcr_guest:
	.quad {HCR_GUEST}

	.macro	VECT_EMPTY
	.align 7
	1:	b	1b
	.endm

	.align 11
hyp_vectors:
	VECT_EMPTY	/* Synchronous EL2t */
	VECT_EMPTY	/* IRQ EL2t */
	VECT_EMPTY	/* FIQ EL2t */
	VECT_EMPTY	/* Error EL2t */

	VECT_EMPTY	/* Synchronous EL2h */
	VECT_EMPTY	/* IRQ EL2h */
	VECT_EMPTY	/* FIQ EL2h */
	VECT_EMPTY	/* Error EL2h */

	VECT_EMPTY	/* Synchronous 64-bit EL1 */
	VECT_EMPTY	/* IRQ 64-bit EL1 */
	VECT_EMPTY	/* FIQ 64-bit EL1 */
	VECT_EMPTY	/* Error 64-bit EL1 */

	VECT_EMPTY	/* Synchronous 32-bit EL1 */
	VECT_EMPTY	/* IRQ 32-bit EL1 */
	VECT_EMPTY	/* FIQ 32-bit EL1 */
	VECT_EMPTY	/* Error 32-bit EL1 */

/*
 * Get the delta between the physical address we were loaded to and the
 * virtual address we expect to run from. This is used when building the
 * initial page table.
 */
	.globl get_virt_delta
get_virt_delta:
	/* Load the physical address of virt_map */
	adr	x28, virt_map
	/* Load the virtual address of virt_map stored in virt_map */
	ldr	x29, [x28]
	/* Calculate delta between virt_map and _start */
	ldr	x27, [x28, #8]
	sub	x27, x29, x27
	/* Find VA - PA: PA' = VA' - (VA - PA) */
	sub	x29, x29, x28
	/* Calculate physical address at which we were loaded */
	sub	x28, x28, x27
	and	x28, x28, #~0x001fffff

	ret

	.align 3
virt_map:
	.quad	virt_map
	.quad	_start

	.globl start_mmu
start_mmu:
	dsb	sy

	/* Load the exception vectors */
	ldr	x2, =exception_vectors
	msr	vbar_el1, x2

	/* Load ttbr0 and ttbr1 */
	msr	ttbr0_el1, x27
	msr	ttbr1_el1, x26
	isb

	/* Clear the Monitor Debug System control register */
	msr	mdscr_el1, xzr

	/* Invalidate the TLB */
	dsb	ishst
	tlbi	vmalle1is
	dsb	ish
	isb

	ldr	x2, mair
	msr	mair_el1, x2

	/*
	 * Setup TCR according to PARange bits from ID_AA64MMFR0_EL1.
	 */
	ldr	x2, tcr
	mrs	x3, id_aa64mmfr0_el1
	bfi	x2, x3, #32, #3
	msr	tcr_el1, x2

	/* Setup SCTLR */
	ldr	x2, sctlr_set
	ldr	x3, sctlr_clear
	mrs	x1, sctlr_el1
	bic	x1, x1, x3	/* Clear the required bits */
	orr	x1, x1, x2	/* Set the required bits */
	msr	sctlr_el1, x1
	isb

	ret

	.globl switch_mmu_kernel
switch_mmu_kernel:
	dsb	sy
	/* Load ttbr1 (kernel) */
	msr	ttbr1_el1, x0
	isb

	/* Invalidate the TLB */
	dsb	ishst
	tlbi	vmalle1is
	dsb	ish
	isb

	ret

	.align 3
mair:
	/* This kernel's layout (include/pte.rs, deviations) */
	.quad	{MAIR}
tcr:
	.quad	{TCR}
sctlr_set:
	/* Bits to set */
	.quad	{SCTLR_SET}
sctlr_clear:
	/* Bits to clear */
	.quad	{SCTLR_CLEAR}

	.align 3
	.globl abort
abort:
	b abort

	.data
	.balign 8
	.global esym
esym:	.xword	end

data_align_pad:
	.space 32
	.align 12 /* 4KiB aligned */
	/*
	 * 8 initial tables (in the following order):
	 *           L2 for kernel (High addresses)
	 *           L1 for kernel
	 *           L0 for kernel (this kernel's TTBR1_EL1 walk has four levels)
	 *           L1 for the direct map (locore.rs)
	 *           L2 for identity map (Low addresses)
	 *           L1 for identity map
	 *           L0 for identity map
	 *
	 * The kernel L2 and identity map L1 and L2 tables contain two
	 * pages each such that we can map a 64MB region that straddles
	 * 1GB or 512GB (in the case of the identity map) boundary.
	 */
	.globl	pagetable
pagetable:
pagetable_l2_ttbr1:
	.space	{PAGE_SIZE} * 2
pagetable_l1_ttbr1:
	.space	{PAGE_SIZE}
	.globl	pagetable_l0_ttbr1
pagetable_l0_ttbr1:
	.space	{PAGE_SIZE}
	.globl	pagetable_l1_direct
pagetable_l1_direct:
	.space	{PAGE_SIZE}
pagetable_l2_ttbr0:
	.space	{PAGE_SIZE} * 2
pagetable_l1_ttbr0:
	.space	{PAGE_SIZE} * 2
	.globl pagetable_l0_ttbr0
pagetable_l0_ttbr0:
	.space	{PAGE_SIZE}
	.globl	pagetable_end
pagetable_end:

	.bss
	.align	4
	.globl initstack
initstack:
	.space	{INITSTACK_SIZE}
	.globl initstack_end
initstack_end:

	.text

/* The signal trampoline saves and restores the floating-point registers. */
.arch_extension	fp

	.text
	.globl	sigcode
	.type	sigcode,@function
sigcode:
	sub	sp, sp, #17 * 32
	mov	x3, sp
	stp	q0, q1, [x3], #32
	stp	q2, q3, [x3], #32
	stp	q4, q5, [x3], #32
	stp	q6, q7, [x3], #32
	stp	q8, q9, [x3], #32
	stp	q10, q11, [x3], #32
	stp	q12, q13, [x3], #32
	stp	q14, q15, [x3], #32
	stp	q16, q17, [x3], #32
	stp	q18, q19, [x3], #32
	stp	q20, q21, [x3], #32
	stp	q22, q23, [x3], #32
	stp	q24, q25, [x3], #32
	stp	q26, q27, [x3], #32
	stp	q28, q29, [x3], #32
	stp	q30, q31, [x3], #32
	mrs	x4, fpsr
	mrs	x5, fpcr
	stp	w4, w5, [x3]
	blr	lr
	mov	x3, sp
	ldp	q0, q1, [x3], #32
	ldp	q2, q3, [x3], #32
	ldp	q4, q5, [x3], #32
	ldp	q6, q7, [x3], #32
	ldp	q8, q9, [x3], #32
	ldp	q10, q11, [x3], #32
	ldp	q12, q13, [x3], #32
	ldp	q14, q15, [x3], #32
	ldp	q16, q17, [x3], #32
	ldp	q18, q19, [x3], #32
	ldp	q20, q21, [x3], #32
	ldp	q22, q23, [x3], #32
	ldp	q24, q25, [x3], #32
	ldp	q26, q27, [x3], #32
	ldp	q28, q29, [x3], #32
	ldp	q30, q31, [x3], #32
	ldp	w4, w5, [x3]
	mrs	x4, fpsr
	mrs	x5, fpcr
	add	sp, sp, #17 * 32
	mov	x0, sp
	add	x0, x0, #{SF_SC}

	mov	x8, #{SYS_SIGRETURN}
	.globl sigcodecall
sigcodecall:
	svc	0
	dsb	nsh
	isb
	.globl  sigcoderet
sigcoderet:
	.size	sigcode, . - sigcode	/* END(sigcode) */
	.global esigcode
esigcode:
	/* FALLTHROUGH */
	.globl	sigfill
sigfill:
	udf	#0
esigfill:

	.data
	/* global_asm! shares .data with the Rust objects: align the word (not in the C). */
	.balign	4
	.globl	sigfillsiz
sigfillsiz:
	.word	esigfill - sigfill

/* Back to normal kernel code. */
.arch_extension	nofp
	.text

	.if {MULTIPROCESSOR}
/*
 * An application processor's entry, which PSCI CPU_ON starts at its physical address with
 * the MMU off and x0 the context machdep.rs passed: here the cpu_info's virtual address,
 * and the kernel's TTBR1_EL1 table in ap_ttbr1 (locore.rs, deviations).
 */
	.globl cpu_hatch_secondary
cpu_hatch_secondary:
	/* Drop to EL1 */
	bl	drop_to_el1

	/* Get the virt -> phys offset */
	bl	get_virt_delta

	/* Set up CPU info */
	msr	tpidr_el1, x0

	/* Enable the mmu */
	adr	x27, .Lpagetable_l0_ttbr0
	ldr	x27, [x27]
	sub	x27, x27, x29
	adrp	x26, {AP_TTBR1}
	ldr	x26, [x26, :lo12:{AP_TTBR1}]
	bl	start_mmu

	mrs	x0, tpidr_el1
	ldr	x1, [x0, #{CI_EL1_STKEND}]
	mov	sp, x1

	adr	x1, .Lcpu_init_secondary
	ldr	x1, [x1]
	blr	x1
	b	.

	.align 3
.Lcpu_init_secondary:
	.xword	{CPU_HATCH_BOOTARG}
.Lpagetable_l0_ttbr0:
	.xword	pagetable_l0_ttbr0
	.endif
