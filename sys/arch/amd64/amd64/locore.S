/*	$OpenBSD: locore.S,v 1.151 2025/08/02 07:33:28 sf Exp $	*/
/*	$NetBSD: locore.S,v 1.13 2004/03/25 18:33:17 drochner Exp $	*/

/*
 * Copyright-o-rama!
 */

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
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
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

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)locore.s	7.3 (Berkeley) 5/13/91
 */

/*
 * The ported subset of locore.S: lgdt and intr_fast_exit (see locore.rs). The symbols
 * written as NAME between braces are filled in by locore.rs from the Rust definitions (assym.h in C).
 */

	.text

/*
 * void lgdt(struct region_descriptor *rdp);
 * Change the global descriptor table.
 */
	.globl	lgdt
	.type	lgdt,@function
	.p2align 4, 0x90
lgdt:
	/* Reload the descriptor table. */
	movq	%rdi,%rax
	lgdt	(%rax)
	/* Flush the prefetch q. */
	jmp	1f
	nop
1:	/* Reload "stale" selectors. */
	movl	${GSEL_KDATA},%eax
	movl	%eax,%ds
	movl	%eax,%es
	movl	%eax,%ss
	/* Reload code selector by doing intersegment return. */
	popq	%rax
	pushq	${GSEL_KCODE}
	pushq	%rax
	lretq
	.size	lgdt, . - lgdt

/*
 * Return to the kernel from a trap or interrupt: restore the registers of the trap frame and
 * iretq. The DIAGNOSTIC check that interrupts are blocked ("not blocked" warning) comes with
 * the interrupt stubs (M4-b).
 */
	.globl	intr_fast_exit
	.type	intr_fast_exit,@function
	.p2align 4, 0x90
intr_fast_exit:
	movq	{TF_RDI}(%rsp),%rdi
	movq	{TF_RSI}(%rsp),%rsi
	movq	{TF_R8}(%rsp),%r8
	movq	{TF_R9}(%rsp),%r9
	movq	{TF_R10}(%rsp),%r10
	movq	{TF_R12}(%rsp),%r12
	movq	{TF_R13}(%rsp),%r13
	movq	{TF_R14}(%rsp),%r14
	movq	{TF_R15}(%rsp),%r15
	movq	{TF_RBP}(%rsp),%rbp
	movq	{TF_RBX}(%rsp),%rbx
	movq	{TF_RDX}(%rsp),%rdx
	movq	{TF_RCX}(%rsp),%rcx
	movq	{TF_R11}(%rsp),%r11
	movq	{TF_RAX}(%rsp),%rax
	addq	${TF_RIP},%rsp
	iretq
	.size	intr_fast_exit, . - intr_fast_exit

/*
 * intr_user_exit: the AST check and return to user mode. There is no user mode
 * yet (M6), so reaching this is a bug, reported through Rust (locore.rs).
 */
	.globl	intr_user_exit
	.type	intr_user_exit,@function
	.p2align 4, 0x90
intr_user_exit:
	movq	%rsp,%rdi
	call	intr_user_exit_unported
	ud2
	.size	intr_user_exit, . - intr_user_exit

/*
 * cpu_switchto(struct proc *old, struct proc *new): switch from old to new.
 * The subset for kernel threads: the stack pointers and %cr3; the FPU/"extended
 * state" reset, the user segment reset, the Meltdown CR3 bookkeeping, the RSB
 * refill and retguard come with user mode (see locore.rs).
 */
	.globl	cpu_switchto
	.type	cpu_switchto,@function
	.p2align 4, 0x90
cpu_switchto:
	pushq	%rbx
	pushq	%rbp
	pushq	%r12
	pushq	%r13
	pushq	%r14
	pushq	%r15

	movq	%rdi,%r13
	movq	%rsi,%r12

	/* Record new proc. */
	movb	${SONPROC},{P_STAT}(%r12)	# p->p_stat = SONPROC
	movq	%gs:{CI_SELF},%rcx		# SET_CURPROC(%r12,%rcx)
	movq	%r12,%gs:{CI_CURPROC}
	movq	%rcx,{P_CPU}(%r12)

	/* If old proc exited, don't bother. */
	testq	%r13,%r13
	jz	switch_exited

	/*
	 * Save old context.
	 *
	 * Registers:
	 *   %r13 - old proc, then old pcb
	 *   %r12 - new proc
	 */
	movq	{P_ADDR}(%r13),%r13

	/* Save stack pointers. */
	movq	%rsp,{PCB_RSP}(%r13)
	movq	%rbp,{PCB_RBP}(%r13)

switch_exited:
	/*
	 * Restore saved context.
	 *
	 * Registers:
	 *   %rax, %rdi - scratch
	 *   %r12 - new process
	 *   %r13 - new pcb
	 */
	movq	{P_ADDR}(%r12),%r13

	/* No interrupts while loading new state. */
	cli

	/* Restore stack pointers. */
	movq	{PCB_RSP}(%r13),%rsp
	movq	{PCB_RBP}(%r13),%rbp

	/* don't switch cr3 to the same thing it already was */
	movq	{PCB_CR3}(%r13),%rax
	movq	%cr3,%rdi
	xorq	%rax,%rdi
	testq	%rdi,%rdi
	jz	.Lsame_cr3
	movq	%rax,%cr3
.Lsame_cr3:
	movq	%r13,%gs:{CI_CURPCB}		# SET_CURPCB(%r13)

	/* Interrupts are okay again. */
	sti
	popq	%r15
	popq	%r14
	popq	%r13
	popq	%r12
	popq	%rbp
	popq	%rbx
	ret
	.size	cpu_switchto, . - cpu_switchto

/*
 * proc_trampoline: where a thread built by cpu_fork starts, with the switch
 * frame's sf_r12 (the function) and sf_r13 (its argument) in %r12 and %r13.
 * The Rust side (proc_trampoline_run) runs proc_trampoline_mi and then the
 * function; the return to user mode (syscall_return) comes with M6.
 */
	.globl	proc_trampoline
	.type	proc_trampoline,@function
	.p2align 4, 0x90
proc_trampoline:
	movq	%r12,%rdi
	movq	%r13,%rsi
	call	proc_trampoline_run
	ud2
	.size	proc_trampoline, . - proc_trampoline
