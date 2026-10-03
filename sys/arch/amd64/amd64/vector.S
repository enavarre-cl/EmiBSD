/*	$OpenBSD: vector.S,v 1.108 2026/08/19 08:56:28 hshoexer Exp $	*/
/*	$NetBSD: vector.S,v 1.5 2004/06/28 09:13:11 fvdl Exp $	*/

/*
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * The ported subset of vector.S: the trap and fault vector routines (see vector.rs). The
 * symbols written as NAME between braces are filled in by vector.rs from the Rust definitions (assym.h
 * and the trap numbers in C).
 */

/*****************************************************************************/

/*
 * Trap and fault vector routines
 *
 * On exit from the kernel to user mode, we always need to check for ASTs.  In
 * addition, we need to do this atomically; otherwise an interrupt may occur
 * which causes an AST, but it won't get processed until the next kernel entry
 * (possibly the next clock tick).  Thus, we disable interrupt before checking,
 * and only enable them again on the final `iret' or before calling the AST
 * handler.
 */

/*****************************************************************************/

.macro	TRAP	a
	pushq	$\a
	jmp	alltraps
.endm
.macro	ZTRAP	a
	pushq	$0
	TRAP	\a
.endm

/* IDTVEC(name): the entry point of vector `name', as asm.h defines it. */
.macro	IDTVEC	name
	.text
	.globl	X\name
	.type	X\name,@function
	.p2align 4, 0x90
X\name:
.endm

/* IDTVEC_ALIAS(name, sym): vector `name' is the same code as `sym'. */
.macro	IDTVEC_ALIAS	name, sym
	.globl	X\name
	.set	X\name, X\sym
.endm

IDTVEC	trap00
	ZTRAP	{T_DIVIDE}
IDTVEC	trap01
	ZTRAP	{T_TRCTRAP}
/*
 * NMIs and double faults take the special-stack path (calltrap_specstk) in the C, which
 * saves and restores GS.base and %cr3 because the trap may land in user context; without
 * user mode they go through alltraps on their IST stacks (M6).
 */
IDTVEC	trap02
	ZTRAP	{T_NMI}
IDTVEC	trap03
	ZTRAP	{T_BPTFLT}
IDTVEC	trap04
	ZTRAP	{T_OFLOW}	/* impossible: INTO instruction invalid in amd64 */
IDTVEC	trap05
	ZTRAP	{T_BOUND}	/* impossible: BOUND instruction invalid in amd64 */
IDTVEC	trap06
	ZTRAP	{T_PRIVINFLT}
IDTVEC	trap07
	ZTRAP	{T_DNA}		/* impossible: we don't do lazy FPU */
IDTVEC	trap08
	TRAP	{T_DOUBLEFLT}
IDTVEC	trap09
	ZTRAP	{T_FPOPFLT}	/* impossible: not generated on amd64 */
IDTVEC	trap0a
	TRAP	{T_TSSFLT}
IDTVEC	trap0b
	TRAP	{T_SEGNPFLT}
IDTVEC	trap0c
	TRAP	{T_STKFLT}
/*
 * The #GP handler's special cases (faults in iretq to userspace, in xrstor, in xsetbv and in
 * rdmsr_safe, resumed in their stubs) have no takers yet: M6.
 */
IDTVEC	trap0d
	TRAP	{T_PROTFLT}
IDTVEC	trap0e
	TRAP	{T_PAGEFLT}
IDTVEC	intrspurious
IDTVEC_ALIAS	trap0f, intrspurious
	iretq
IDTVEC	trap10
	ZTRAP	{T_ARITHTRAP}
IDTVEC	trap11
	TRAP	{T_ALIGNFLT}
IDTVEC	trap12
	ZTRAP	{T_MCA}
IDTVEC	trap13
	ZTRAP	{T_XMM}
IDTVEC	trap14
	ZTRAP	{T_VE}
IDTVEC	trap15
	TRAP	{T_CP}
/* #VC (trap1d) is AMD SEV: not configured. */
IDTVEC	trap1f
IDTVEC_ALIAS	trap16, trap1f
IDTVEC_ALIAS	trap17, trap1f
IDTVEC_ALIAS	trap18, trap1f
IDTVEC_ALIAS	trap19, trap1f
IDTVEC_ALIAS	trap1a, trap1f
IDTVEC_ALIAS	trap1b, trap1f
IDTVEC_ALIAS	trap1c, trap1f
IDTVEC_ALIAS	trap1d, trap1f
IDTVEC_ALIAS	trap1e, trap1f
	/* 22 - 31 reserved for future exp */
	ZTRAP	{T_RESERVED}

	.section .rodata
	.globl	Xexceptions
	.type	Xexceptions,@object
Xexceptions:
	.quad	Xtrap00, Xtrap01, Xtrap02, Xtrap03
	.quad	Xtrap04, Xtrap05, Xtrap06, Xtrap07
	.quad	Xtrap08, Xtrap09, Xtrap0a, Xtrap0b
	.quad	Xtrap0c, Xtrap0d, Xtrap0e, Xtrap0f
	.quad	Xtrap10, Xtrap11, Xtrap12, Xtrap13
	.quad	Xtrap14, Xtrap15, Xtrap16, Xtrap17
	.quad	Xtrap18, Xtrap19, Xtrap1a, Xtrap1b
	.quad	Xtrap1c, Xtrap1d, Xtrap1e, Xtrap1f
	.size	Xexceptions, . - Xexceptions

/*
 * All traps go through here.  Figure out whether we're
 * a) coming from usermode and need the Meltdown mitigation before
 *    jumping to user trap handling followed by AST and
 *    return-to-userspace handling, or
 * b) coming from supervisor mode and can directly jump to kernel
 *    trap handling before returning sans AST or other handling.
 *
 * The Meltdown alltraps page (Xalltraps, the U-K text) is not here (vector.rs).
 */
	.text
	.globl	alltraps
	.type	alltraps,@function
	.p2align 4, 0x90
alltraps:
	testb	${SEL_RPL},24(%rsp)	/* trapno, err, rip, then cs */
	je	alltraps_kern
	swapgs
	movq	%rax,%gs:{CI_SCRATCH}
	jmp	alltraps_user
	.size	alltraps, . - alltraps

/*
 * Traps from supervisor mode (kernel)
 */
	.globl	alltraps_kern
	.type	alltraps_kern,@function
	.p2align 4, 0x90
alltraps_kern:
	/* TRAP_ENTRY_KERN: there's a trapno + err already on the stack. We have to
	 * move the err from its hardware location to the location we want it. */
	subq	${TF_TRAPNO},%rsp
	movq	%rcx,{TF_RCX}(%rsp)
	movq	({TF_RIP} - 8)(%rsp),%rcx
	movq	%rcx,{TF_ERR}(%rsp)
	/* INTR_SAVE_MOST_GPRS_NO_ADJ */
	movq	%r15,{TF_R15}(%rsp)
	movq	%r14,{TF_R14}(%rsp)
	movq	%r13,{TF_R13}(%rsp)
	movq	%r12,{TF_R12}(%rsp)
	movq	%r11,{TF_R11}(%rsp)
	movq	%r10,{TF_R10}(%rsp)
	movq	%r9,{TF_R9}(%rsp)
	movq	%r8,{TF_R8}(%rsp)
	movq	%rdi,{TF_RDI}(%rsp)
	movq	%rsi,{TF_RSI}(%rsp)
	movq	%rbp,{TF_RBP}(%rsp)
	leaq	{TF_RBP}(%rsp),%rbp
	movq	%rbx,{TF_RBX}(%rsp)
	movq	%rdx,{TF_RDX}(%rsp)
	movq	%rax,{TF_RAX}(%rsp)
	sti
	cld
	/* SMAP_CLAC: with CPU identification (M4-b). */
	movq	%rsp,%rdi
	call	kerntrap
	movq	$0,-8(%rsp)
	cli
	/* DIAGNOSTIC: the "SPL NOT LOWERED ON TRAP EXIT" check comes with spl(9) (M4-b). */
	jmp	intr_fast_exit
	.size	alltraps_kern, . - alltraps_kern

/*
 * Interrupt entry, call to handler, and exit (frameasm.h in C).
 *
 * The interrupt frame is set up to look like a trap frame.  This may be a
 * waste.  The only handler which needs a frame is the clock handler, and it
 * only needs a few bits.  Xdoreti() needs a trap frame for handling ASTs, but
 * it could easily convert the frame on demand.
 */

/* INTR_SAVE_MOST_GPRS_NO_ADJ */
.macro	INTR_SAVE_MOST_GPRS_NO_ADJ
	movq	%r15,{TF_R15}(%rsp)
	movq	%r14,{TF_R14}(%rsp)
	movq	%r13,{TF_R13}(%rsp)
	movq	%r12,{TF_R12}(%rsp)
	movq	%r11,{TF_R11}(%rsp)
	movq	%r10,{TF_R10}(%rsp)
	movq	%r9,{TF_R9}(%rsp)
	movq	%r8,{TF_R8}(%rsp)
	movq	%rdi,{TF_RDI}(%rsp)
	movq	%rsi,{TF_RSI}(%rsp)
	movq	%rbp,{TF_RBP}(%rsp)
	leaq	{TF_RBP}(%rsp),%rbp
	movq	%rbx,{TF_RBX}(%rsp)
	movq	%rdx,{TF_RDX}(%rsp)
	movq	%rax,{TF_RAX}(%rsp)
.endm

/* INTR_SAVE_GPRS */
.macro	INTR_SAVE_GPRS
	subq	${TF_TRAPNO},%rsp
	INTR_SAVE_MOST_GPRS_NO_ADJ
	movq	%rcx,{TF_RCX}(%rsp)
.endm

/*
 * We clear registers when coming from userspace to prevent
 * user-controlled values from being available for use in speculative
 * execution in the kernel.  %rsp and %rbp are the kernel values when
 * this is used, so there are only 14 to clear.  32bit operations clear
 * the register upper-halves automatically.
 */
.macro	INTR_CLEAR_GPRS
	xorl	%eax,%eax
	xorl	%ebx,%ebx
	xorl	%ecx,%ecx
	xorl	%edx,%edx
	xorl	%esi,%esi
	xorl	%edi,%edi
	xorl	%r8d,%r8d
	xorl	%r9d,%r9d
	xorl	%r10d,%r10d
	xorl	%r11d,%r11d
	xorl	%r12d,%r12d
	xorl	%r13d,%r13d
	xorl	%r14d,%r14d
	xorl	%r15d,%r15d
.endm

/*
 * Traps from userspace: TRAP_ENTRY_USER, usertrap, then intr_user_exit.
 * %rax is in CPUVAR(SCRATCH) and the kernel GS.base is loaded (alltraps).
 */
	.globl	alltraps_user
	.type	alltraps_user,@function
	.p2align 4, 0x90
alltraps_user:
	/* TRAP_ENTRY_USER */
	movq	%gs:{CI_KERN_RSP},%rax
	xchgq	%rax,%rsp
	movq	%rcx,{TF_RCX}(%rsp)
	/* copy trapno+err to the trap frame */
	movq	0(%rax),%rcx
	movq	%rcx,{TF_TRAPNO}(%rsp)
	movq	8(%rax),%rcx
	movq	%rcx,{TF_ERR}(%rsp)
	/* copy iretq frame to the trap frame */
	movq	({IRETQ_RIP}+16)(%rax),%rcx
	movq	%rcx,{TF_RIP}(%rsp)
	movq	({IRETQ_CS}+16)(%rax),%rcx
	movq	%rcx,{TF_CS}(%rsp)
	movq	({IRETQ_RFLAGS}+16)(%rax),%rcx
	movq	%rcx,{TF_RFLAGS}(%rsp)
	movq	({IRETQ_RSP}+16)(%rax),%rcx
	movq	%rcx,{TF_RSP}(%rsp)
	movq	({IRETQ_SS}+16)(%rax),%rcx
	movq	%rcx,{TF_SS}(%rsp)
	movq	%gs:{CI_SCRATCH},%rax
	INTR_SAVE_MOST_GPRS_NO_ADJ
	INTR_CLEAR_GPRS
	sti
	cld
	/* SMAP_CLAC: with CPU identification (M4-b). */
	movq	%rsp,%rdi
	call	usertrap
	movq	$0,-8(%rsp)
	cli
	jmp	intr_user_exit
	.size	alltraps_user, . - alltraps_user

/*
 * For real interrupt code paths, where we can come from userspace.
 * We only have an iretq_frame on entry. The Meltdown CR3 switch of the
 * userspace path is not here (vector.rs).
 */
.macro	INTRENTRY	label
	endbr64
	testb	${SEL_RPL},{IRETQ_CS}(%rsp)
	je	X\label\()_untramp
	swapgs
	movq	%rax,%gs:{CI_SCRATCH}
	jmp	98f
	.size	X\label, . - X\label
	.globl	X\label\()_untramp
	.type	X\label\()_untramp,@function
X\label\()_untramp:	/* from kernel */
	subq	${TF_RIP},%rsp
	movq	%rcx,{TF_RCX}(%rsp)
	jmp	99f
	.p2align 4, 0x90
98:	/* from userspace */
	movq	%gs:{CI_KERN_RSP},%rax
	xchgq	%rax,%rsp
	movq	%rcx,{TF_RCX}(%rsp)
	/* copy iretq frame to the trap frame */
	movq	{IRETQ_RIP}(%rax),%rcx
	movq	%rcx,{TF_RIP}(%rsp)
	movq	{IRETQ_CS}(%rax),%rcx
	movq	%rcx,{TF_CS}(%rsp)
	movq	{IRETQ_RFLAGS}(%rax),%rcx
	movq	%rcx,{TF_RFLAGS}(%rsp)
	movq	{IRETQ_RSP}(%rax),%rcx
	movq	%rcx,{TF_RSP}(%rsp)
	movq	{IRETQ_SS}(%rax),%rcx
	movq	%rcx,{TF_SS}(%rsp)
	movq	%gs:{CI_SCRATCH},%rax
99:	INTR_SAVE_MOST_GPRS_NO_ADJ
	INTR_CLEAR_GPRS
	movq	%rax,{TF_ERR}(%rsp)
.endm

/*
 * Entry for faking up an interrupt frame after spllower() unblocks
 * a previously received interrupt.  On entry, %r13 has the %rip
 * to return to.  %r10 and %r11 are scratch.
 */
.macro	INTR_RECURSE
	endbr64
	/* fake the iretq_frame */
	movq	%rsp,%r10
	movl	%ss,%r11d
	pushq	%r11
	pushq	%r10
	pushfq
	movl	%cs,%r11d
	pushq	%r11
	pushq	%r13
	/* now do the rest of the intrframe */
	subq	$16,%rsp
	INTR_SAVE_GPRS
.endm

/*****************************************************************************/

/* The i8259 masking and acknowledgement sequences (i8259.h in C). */

.macro	voidop	num=0, icuaddr=0
.endm

.macro	i8259_asm_ack1	num
	movb	$(0x60|(\num%8)),%al	/* specific EOI */
	outb	%al,${IO_ICU1}
.endm

.macro	i8259_asm_ack2	num
	movb	$(0x60|(\num%8)),%al	/* specific EOI */
	outb	%al,${IO_ICU2}		/* do the second ICU first */
	movb	$(0x60|{IRQ_SLAVE}),%al	/* specific EOI for IRQ2 */
	outb	%al,${IO_ICU1}
.endm

.macro	i8259_asm_mask	num, icuaddr
	movb	i8259_imen+((\num)>>3)(%rip),%al
	orb	$(1<<((\num)%8)),%al
	movb	%al,i8259_imen+((\num)>>3)(%rip)
	outb	%al,$(\icuaddr+1)
.endm

.macro	i8259_asm_unmask	num, icuaddr
	movb	i8259_imen+((\num)>>3)(%rip),%al
	andb	$~(1<<((\num)%8)),%al
	movb	%al,i8259_imen+((\num)>>3)(%rip)
	outb	%al,$(\icuaddr+1)
.endm

/*
 * This macro defines the generic stub code. Its arguments modify it
 * for specific PICs.
 */
.macro	INTRSTUB	name, num, early_ack, late_ack, mask, unmask, level_mask, icuaddr
	.globl	Xrecurse_\name\()\num
	.type	Xrecurse_\name\()\num,@function
	.p2align 4, 0x90
Xrecurse_\name\()\num:
	INTR_RECURSE
	.size	Xrecurse_\name\()\num, . - Xrecurse_\name\()\num
	.globl	Xresume_\name\()\num
	.type	Xresume_\name\()\num,@function
Xresume_\name\()\num:
	endbr64
	movq	${IREENT_MAGIC},{TF_ERR}(%rsp)
	movl	%ebx,%r13d
	movq	%gs:({CI_ISOURCES} + (\num) * 8),%r14
	movl	{IS_MAXLEVEL}(%r14),%ebx
	jmp	1f
	.size	Xresume_\name\()\num, . - Xresume_\name\()\num
	.globl	Xintr_\name\()\num
	.type	Xintr_\name\()\num,@function
	.p2align 4, 0x90
Xintr_\name\()\num:
	INTRENTRY	intr_\name\()\num
	movq	%gs:({CI_ISOURCES} + (\num) * 8),%r14
	\mask	\num, \icuaddr		/* mask it in hardware */
	\early_ack	\num		/* and allow other intrs */
	incl	uvmexp+{V_INTR}(%rip)	/* statistical info */
	testq	%r14,%r14
	jz	9f			/* stray */
	movl	{IS_MAXLEVEL}(%r14),%ebx
	movl	%gs:{CI_ILEVEL},%r13d
	cmpl	%ebx,%r13d
	jae	10f			/* currently masked; hold it */
1:
	movq	%r13,{IF_PPL}(%rsp)
	movl	%ebx,%gs:{CI_ILEVEL}
	sti
	cld
	/* SMAP_CLAC: with CPU identification (M4-b). */
	incl	%gs:{CI_IDEPTH}
	movq	{IS_HANDLERS}(%r14),%rbx
6:	/* loop, walking chain of handlers */
	movl	{IH_LEVEL}(%rbx),%r12d
	cmpl	%r13d,%r12d
	jle	7f
	movl	%r12d,%gs:{CI_ILEVEL}
	movq	%rbx, %rsi
	movq	%rsp, %rdi
	call	intr_handler		/* call it */
	movq	$0,-8(%rsp)
	orl	%eax,%eax		/* should it be counted? */
	jz	4f			/* no, skip it */
	incq	{IH_COUNT}(%rbx)	/* count the intrs */
	cmpl	$0,intr_shared_edge(%rip)
	jne	4f			/* if no shared edges ... */
	orl	%eax,%eax		/* 1 means stop trying */
	jns	5f
4:	movq	{IH_NEXT}(%rbx),%rbx	/* next handler in chain */
	testq	%rbx,%rbx
	jnz	6b
5:	/* successfully handled */
	cli
	\unmask	\num, \icuaddr		/* unmask it in hardware */
	\late_ack	\num
	sti
	jmp	Xdoreti			/* lower spl and do ASTs */
7:	/* current IPL > handler's ih_level */
	cli
	movq	$(1 << \num),%rax
	orq     %rax,%gs:{CI_IPENDING}
	\level_mask	\num, \icuaddr
	\late_ack	\num
	sti
	jmp	Xdoreti			/* lower spl and do ASTs */
10:	/* currently masked */
	cli
	movq	$(1 << \num),%rax
	orq	%rax,%gs:{CI_IPENDING}
	\level_mask	\num, \icuaddr
	\late_ack	\num
	jmp	intr_fast_exit		/* INTRFASTEXIT */
9:	/* spurious interrupt */
	\unmask	\num, \icuaddr
	\late_ack	\num
	testb	${SEL_RPL},{TF_CS}(%rsp)
	jnz	intr_user_exit
	jmp	intr_fast_exit		/* INTRFASTEXIT */
	.size	Xintr_\name\()\num\()_untramp, . - Xintr_\name\()\num\()_untramp
.endm

INTRSTUB legacy,0,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,1,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,2,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,3,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,4,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,5,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,6,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}
INTRSTUB legacy,7,i8259_asm_ack1,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU1}

INTRSTUB legacy,8,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,9,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,10,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,11,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,12,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,13,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,14,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}
INTRSTUB legacy,15,i8259_asm_ack2,voidop,i8259_asm_mask,i8259_asm_unmask,voidop,{IO_ICU2}

/* NIOAPIC > 0: the ioapic_edge/ioapic_level stubs come with the IOAPIC (M5). */

	.section .rodata
	.globl i8259_stubs
	.type i8259_stubs,@object
i8259_stubs:
	.quad Xintr_legacy0, Xrecurse_legacy0, Xresume_legacy0
	.quad Xintr_legacy1, Xrecurse_legacy1, Xresume_legacy1
	.quad Xintr_legacy2, Xrecurse_legacy2, Xresume_legacy2
	.quad Xintr_legacy3, Xrecurse_legacy3, Xresume_legacy3
	.quad Xintr_legacy4, Xrecurse_legacy4, Xresume_legacy4
	.quad Xintr_legacy5, Xrecurse_legacy5, Xresume_legacy5
	.quad Xintr_legacy6, Xrecurse_legacy6, Xresume_legacy6
	.quad Xintr_legacy7, Xrecurse_legacy7, Xresume_legacy7
	.quad Xintr_legacy8, Xrecurse_legacy8, Xresume_legacy8
	.quad Xintr_legacy9, Xrecurse_legacy9, Xresume_legacy9
	.quad Xintr_legacy10, Xrecurse_legacy10, Xresume_legacy10
	.quad Xintr_legacy11, Xrecurse_legacy11, Xresume_legacy11
	.quad Xintr_legacy12, Xrecurse_legacy12, Xresume_legacy12
	.quad Xintr_legacy13, Xrecurse_legacy13, Xresume_legacy13
	.quad Xintr_legacy14, Xrecurse_legacy14, Xresume_legacy14
	.quad Xintr_legacy15, Xrecurse_legacy15, Xresume_legacy15
	.size i8259_stubs, . - i8259_stubs
	.text

/*
 * Soft interrupt handlers. The retpoline_r13 thunk (a CODEPATCHed Spectre
 * mitigation) is a plain indirect jump: codepatch.c is not ported.
 */
	.globl	Xsofttty
	.type	Xsofttty,@function
	.p2align 4, 0x90
Xsofttty:
	endbr64
	movl	${IPL_SOFTTTY},%gs:{CI_ILEVEL}
	sti
	incl	%gs:{CI_IDEPTH}
	movl	${SOFTINTR_TTY},%edi
	call	dosoftint
	movq	$0,-8(%rsp)
	decl	%gs:{CI_IDEPTH}
	jmp	*%r13			/* retpoline_r13 */
	.size	Xsofttty, . - Xsofttty

	.globl	Xsoftnet
	.type	Xsoftnet,@function
	.p2align 4, 0x90
Xsoftnet:
	endbr64
	movl	${IPL_SOFTNET},%gs:{CI_ILEVEL}
	sti
	incl	%gs:{CI_IDEPTH}
	movl	${SOFTINTR_NET},%edi
	call	dosoftint
	movq	$0,-8(%rsp)
	decl	%gs:{CI_IDEPTH}
	jmp	*%r13			/* retpoline_r13 */
	.size	Xsoftnet, . - Xsoftnet

	.globl	Xsoftclock
	.type	Xsoftclock,@function
	.p2align 4, 0x90
Xsoftclock:
	endbr64
	movl	${IPL_SOFTCLOCK},%gs:{CI_ILEVEL}
	sti
	incl	%gs:{CI_IDEPTH}
	movl	${SOFTINTR_CLOCK},%edi
	call	dosoftint
	movq	$0,-8(%rsp)
	decl	%gs:{CI_IDEPTH}
	jmp	*%r13			/* retpoline_r13 */
	.size	Xsoftclock, . - Xsoftclock

/*
 * Local APIC timer interrupt. The handler takes the frame by pointer (%rsi),
 * where the C passes `struct intrframe' by value; the EOI is the MMIO write
 * (the x2APIC CODEPATCH is not ported).
 */
	.globl	Xrecurse_lapic_ltimer
	.type	Xrecurse_lapic_ltimer,@function
	.p2align 4, 0x90
Xrecurse_lapic_ltimer:
	INTR_RECURSE
	jmp	1f
	.size	Xrecurse_lapic_ltimer, . - Xrecurse_lapic_ltimer
	.globl	Xintr_lapic_ltimer
	.type	Xintr_lapic_ltimer,@function
	.p2align 4, 0x90
Xintr_lapic_ltimer:
	INTRENTRY	intr_lapic_ltimer
	movl	$0,local_apic+{LAPIC_EOI}(%rip)
	movl	%gs:{CI_ILEVEL},%ebx
	cmpl	${IPL_CLOCK},%ebx
	jae	2f
	.size	Xintr_lapic_ltimer, . - Xintr_lapic_ltimer
	.globl	Xresume_lapic_ltimer
	.type	Xresume_lapic_ltimer,@function
Xresume_lapic_ltimer:
	endbr64
1:
	incl	%gs:{CI_IDEPTH}
	movl	${IPL_CLOCK},%gs:{CI_ILEVEL}
	sti
	cld
	/* SMAP_CLAC: with CPU identification (M4-b). */
	movq	%rbx,{IF_PPL}(%rsp)
	xorq	%rdi,%rdi
	movq	%rsp,%rsi
	call	lapic_clockintr
	movq	$0,-8(%rsp)
	jmp	Xdoreti
2:
	movq	$(1 << {LIR_TIMER}),%rax
	orq	%rax,%gs:{CI_IPENDING}
	jmp	intr_fast_exit		/* INTRFASTEXIT */
	.size	Xresume_lapic_ltimer, . - Xresume_lapic_ltimer
