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
 * There is no user mode yet (M6): a trap whose CS has the user RPL takes the kernel path
 * too, and kerntrap reports the mode it came from.
 */
	.text
	.globl	alltraps
	.type	alltraps,@function
	.p2align 4, 0x90
alltraps:
	testb	${SEL_RPL},24(%rsp)	/* trapno, err, rip, then cs */
	je	alltraps_kern
	/* Traps from userspace: TRAP_ENTRY_USER, usertrap, intr_user_exit (M6). */
	jmp	alltraps_kern
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
	/* sti: interrupts stay as they were until the interrupt stubs exist (M4-b). */
	cld
	/* SMAP_CLAC: with CPU identification (M4-b). */
	movq	%rsp,%rdi
	call	kerntrap
	movq	$0,-8(%rsp)
	cli
	/* DIAGNOSTIC: the "SPL NOT LOWERED ON TRAP EXIT" check comes with spl(9) (M4-b). */
	jmp	intr_fast_exit
	.size	alltraps_kern, . - alltraps_kern
