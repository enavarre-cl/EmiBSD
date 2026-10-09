
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

/*
 * The symbols written as NAME between braces are filled in by copy.rs
 * (assym.h in C). SMAP_STAC/SMAP_CLAC (code patches) and retguard are not
 * here: see copy.rs.
 */

.macro	DECLARE_ONFAULT	x
	.pushsection .nofault.0, "a"
	.quad	\x
	.popsection
.endm

/*
 * Copy routines from and to userland, plus a few more. See the
 * section 9 manpages for info. Some cases can be optimized more.
 *
 * I wonder if it's worthwhile to make these use SSE2 registers.
 */

	.text
	.globl	kcopy
	.type	kcopy,@function
	.p2align 4, 0x90
kcopy:
	movq	%gs:{CI_CURPCB},%rax
	pushq	{PCB_ONFAULT}(%rax)
	leaq	copy_fault(%rip),%r11
	movq	%r11, {PCB_ONFAULT}(%rax)

	xchgq	%rdi,%rsi
	movq	%rdx,%rcx

	movq	%rdi,%rax
	subq	%rsi,%rax
	cmpq	%rcx,%rax		# overlapping?
	jb	1f
	shrq	$3,%rcx			# nope, copy forward by 64-bit words
	rep
	movsq

	movq	%rdx,%rcx
	andl	$7,%ecx			# any bytes left?
	rep
	movsb

	movq	%gs:{CI_CURPCB},%rdx
	popq	{PCB_ONFAULT}(%rdx)
	xorq	%rax,%rax
	ret
	lfence

1:	addq	%rcx,%rdi		# copy backward
	addq	%rcx,%rsi
	std
	andq	$7,%rcx			# any fractional bytes?
	decq	%rdi
	decq	%rsi
	rep
	movsb
	movq	%rdx,%rcx		# copy remainder by 64-bit words
	shrq	$3,%rcx
	subq	$7,%rsi
	subq	$7,%rdi
	rep
	movsq
	cld

	movq	%gs:{CI_CURPCB},%rdx
	popq	{PCB_ONFAULT}(%rdx)
	xorq	%rax,%rax
	ret
	lfence
	.size	kcopy, . - kcopy

	.globl	copyout
	.type	copyout,@function
	.p2align 4, 0x90
copyout:
	movq	%gs:{CI_CURPCB},%rax
	pushq	{PCB_ONFAULT}(%rax)

	xchgq	%rdi,%rsi
	movq	%rdx,%rax

	movq	%rdi,%rdx
	addq	%rax,%rdx
	jc	copy_fault
	movq	${VM_MAXUSER_ADDRESS},%r8
	cmpq	%r8,%rdx
	ja	copy_fault

	movq	%gs:{CI_CURPCB},%rdx
	leaq	copy_fault(%rip),%r11
	movq	%r11,{PCB_ONFAULT}(%rdx)
	movq	%rax,%rcx
	shrq	$3,%rcx
	rep
	movsq
	movb	%al,%cl
	shrb	$2,%cl
	andb	$1,%cl
	rep
	movsl
	movb	%al,%cl
	andb	$3,%cl
	rep
	movsb
	popq	{PCB_ONFAULT}(%rdx)
	xorl	%eax,%eax
	ret
	lfence
	.size	copyout, . - copyout

	.globl	_copyin
	.type	_copyin,@function
	.p2align 4, 0x90
_copyin:
	movq	%gs:{CI_CURPCB},%rax
	pushq	{PCB_ONFAULT}(%rax)
	leaq	copy_fault(%rip),%r11
	movq	%r11,{PCB_ONFAULT}(%rax)
	xchgq	%rdi,%rsi
	movq	%rdx,%rax

	movq	%rsi,%rdx
	addq	%rax,%rdx
	jc	copy_fault
	movq	${VM_MAXUSER_ADDRESS},%r8
	cmpq	%r8,%rdx
	ja	copy_fault

3:	/* bcopy(%rsi, %rdi, %rax); */
	movq	%rax,%rcx
	shrq	$3,%rcx
	rep
	movsq
	movb	%al,%cl
	shrb	$2,%cl
	andb	$1,%cl
	rep
	movsl
	movb	%al,%cl
	andb	$3,%cl
	rep
	movsb

	movq	%gs:{CI_CURPCB},%rdx
	popq	{PCB_ONFAULT}(%rdx)
	xorl	%eax,%eax
	ret
	lfence
	.size	_copyin, . - _copyin

	.globl	copy_fault
	.type	copy_fault,@function
	.p2align 4, 0x90
copy_fault:
DECLARE_ONFAULT copy_fault
	cld
	movq	%gs:{CI_CURPCB},%rdx
	popq	{PCB_ONFAULT}(%rdx)
	movl	${EFAULT},%eax
	ret
	lfence
	.size	copy_fault, . - copy_fault

	.globl	copyoutstr
	.type	copyoutstr,@function
	.p2align 4, 0x90
copyoutstr:
	xchgq	%rdi,%rsi
	movq	%rdx,%r8
	movq	%rcx,%r9

	movq	%gs:{CI_CURPCB},%rax
	pushq	{PCB_ONFAULT}(%rax)
	leaq	copystr_fault(%rip),%r11
	movq	%r11,{PCB_ONFAULT}(%rax)
	/*
	 * Get min(%rdx, VM_MAXUSER_ADDRESS-%rdi).
	 */
	movq	${VM_MAXUSER_ADDRESS},%rax
	subq	%rdi,%rax
	jbe	copystr_fault			/* die if CF == 1 || ZF == 1 */
	cmpq	%rdx,%rax
	jae	1f
	movq	%rax,%rdx
	movq	%rax,%r8

1:	incq	%rdx

1:	decq	%rdx
	jz	2f
	lodsb
	stosb
	testb	%al,%al
	jnz	1b

	/* Success -- 0 byte reached. */
	decq	%rdx
	xorq	%rax,%rax
	jmp	copystr_return

2:	/* rdx is zero -- return EFAULT or ENAMETOOLONG. */
	movq	${VM_MAXUSER_ADDRESS},%r11
	cmpq	%r11,%rdi
	jae	copystr_fault
	movl	${ENAMETOOLONG},%eax
	jmp	copystr_return
	.size	copyoutstr, . - copyoutstr

	.globl	_copyinstr
	.type	_copyinstr,@function
	.p2align 4, 0x90
_copyinstr:
	xchgq	%rdi,%rsi
	movq	%rdx,%r8
	movq	%rcx,%r9

	movq	%gs:{CI_CURPCB},%rcx
	pushq	{PCB_ONFAULT}(%rcx)
	leaq	copystr_fault(%rip),%r11
	movq	%r11,{PCB_ONFAULT}(%rcx)

	/*
	 * Get min(%rdx, VM_MAXUSER_ADDRESS-%rsi).
	 */
	movq	${VM_MAXUSER_ADDRESS},%rax
	subq	%rsi,%rax
	jbe	copystr_fault			/* die if CF == 1 || ZF == 1 */
	cmpq	%rdx,%rax
	jae	1f
	movq	%rax,%rdx
	movq	%rax,%r8

1:	incq	%rdx

1:	decq	%rdx
	jz	2f
	lodsb
	stosb
	testb	%al,%al
	jnz	1b

	/* Success -- 0 byte reached. */
	decq	%rdx
	xorq	%rax,%rax
	jmp	copystr_return

2:	/* edx is zero -- return EFAULT or ENAMETOOLONG. */
	movq	${VM_MAXUSER_ADDRESS},%r11
	cmpq	%r11,%rsi
	jae	copystr_fault
	movl	${ENAMETOOLONG},%eax
	jmp	copystr_return
	.size	_copyinstr, . - _copyinstr

	.globl	copystr_fault
	.type	copystr_fault,@function
	.p2align 4, 0x90
copystr_fault:
DECLARE_ONFAULT copystr_fault
	movl	${EFAULT},%eax
copystr_return:
	/* Set *lencopied and return %eax. */
	movq	%gs:{CI_CURPCB},%rcx
	popq	{PCB_ONFAULT}(%rcx)
	testq	%r9,%r9
	jz	8f
	subq	%rdx,%r8
	movq	%r8,(%r9)
8:
	ret
	lfence
	.size	copystr_fault, . - copystr_fault
