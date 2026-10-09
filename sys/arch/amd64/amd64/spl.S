/*
 * Copyright (c) 2003 Wasabi Systems, Inc.
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

/*
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
 * The ported subset of spl.S: Xspllower and Xdoreti (see spl.rs). The symbols written as
 * NAME between braces are filled in by spl.rs from the Rust definitions (assym.h in C).
 * The retguard and profiling prologues are not there; the retpoline through a codepatched
 * thunk is a plain indirect jump (spl.rs, deviations).
 */

	.text

/*
 * Process pending interrupts.
 *
 * Important registers:
 *   ebx - cpl
 *   r13 - address to resume loop at
 *
 * It is important that the bit scan instruction is bsr, it will get
 * the highest 2 bits (currently the IPI and clock handlers) first,
 * to avoid deadlocks where one CPU sends an IPI, another one is at
 * splipi() and defers it, lands in here via splx(), and handles
 * a lower-prio one first, which needs to take the kernel lock -->
 * the sending CPU will never see the that CPU accept the IPI
 */
	.globl	Xspllower
	.type	Xspllower,@function
	.p2align 4, 0x90
Xspllower:
	pushq	%rbx
	pushq	%r13
	movl	%edi,%ebx
	leaq	1f(%rip),%r13		# address to resume loop at
1:	endbr64
	movl	%ebx,%eax		# get cpl
	movq	%gs:{CI_IUNMASK}(,%rax,8),%rax
	cli
	andq	%gs:{CI_IPENDING},%rax	# any non-masked bits left?
	jnz	2f
	movl	%ebx,%gs:{CI_ILEVEL}
	sti
	popq	%r13
	popq	%rbx
	ret
	lfence
2:	bsrq	%rax,%rax
	btrq	%rax,%gs:{CI_IPENDING}
	movq	%gs:{CI_ISOURCES}(,%rax,8),%rax
	movq	{IS_RECURSE}(%rax),%rax
	jmp	*%rax			/* retpoline_rax */
	.size	Xspllower, . - Xspllower

/*
 * Handle return from interrupt after device handler finishes.
 *
 * Important registers:
 *   ebx - cpl to restore
 *   r13 - address to resume loop at
 */
	.globl	Xdoreti
	.type	Xdoreti,@function
	.p2align 4, 0x90
Xdoreti:
	movq	{IF_PPL}(%rsp),%rbx	# get previous priority
	decl	%gs:{CI_IDEPTH}
	leaq	1f(%rip),%r13
1:	endbr64
	movl	%ebx,%eax
	movq	%gs:{CI_IUNMASK}(,%rax,8),%rax
	cli
	andq	%gs:{CI_IPENDING},%rax
	jnz	2f
	movl	%ebx,%gs:{CI_ILEVEL}
	testb	${SEL_RPL},{TF_CS}(%rsp)
	jnz	intr_user_exit		# Check for ASTs on exit to user mode
	jmp	intr_fast_exit		/* INTRFASTEXIT */
2:	bsrq    %rax,%rax               # slow, but not worth optimizing
	btrq    %rax,%gs:{CI_IPENDING}
	movq	%gs:{CI_ISOURCES}(,%rax, 8),%rax
	movq	{IS_RESUME}(%rax),%rax
	jmp	*%rax			/* retpoline_rax */
	.size	Xdoreti, . - Xdoreti
