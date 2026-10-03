/* $OpenBSD: cpuswitch.S,v 1.8 2024/10/17 02:20:53 jsg Exp $ */
/*
 * Copyright (c) 2015 Dale Rahn <drahn@dalerahn.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

	.text

/*
 * cpu_switchto_asm(struct proc *oldproc, struct proc *newproc)
 * 	x0	'struct proc *' of the old context
 * 	x1	'struct proc *' of the new context
 *      x2	- may contain curcpu
 *      x3	- may contain switchframe pointer (new or old)
 *      x4	- may contain user thread pointer (TCB)
 *	x5	- may contain PCB pointer (new or old)
 *
 * The retguard cookie dance (x20/x15) is not here: see cpuswitch.rs.
 */
	.globl	cpu_switchto_asm
	.type	cpu_switchto_asm, %function
	.p2align 2
cpu_switchto_asm:
	// check if old context needs to be saved
	cmp	x0, #0
	beq	1f

	// create switchframe
	sub	sp, sp, #({SWITCHFRAME_SZ})
	mov	x3, sp
	stp	x19, x20,  [x3, #({SF_X19})]
	stp	x21, x22,  [x3, #({SF_X21})]
	stp	x23, x24,  [x3, #({SF_X23})]
	stp	x25, x26,  [x3, #({SF_X25})]
	stp	x27, x28,  [x3, #({SF_X27})]
	stp	x29, x30,  [x3, #({SF_X29})]

	mrs	x2, tpidr_el1			// load curcpu
	ldr	x5, [x2, #({CI_CURPCB})]
	str	x3, [x5, #({PCB_SP})]		// save to old pcb

1:
	mrs	x2, tpidr_el1			// load curcpu
	mov	w5, #{SONPROC}
	strb	w5, [x1, #({P_STAT})]		// Mark new on cpu
	str	x2, [x1, #({P_CPU})]
	ldr	x5, [x1, #({P_ADDR})]		// load new pcb
	str	x5, [x2, #({CI_CURPCB})]
	str	x1, [x2, #({CI_CURPROC})]

	ldr	x4, [x5, #({PCB_TCB})]
	msr	tpidr_el0, x4			// load user tls

	ldr	x19, [x5, #({PCB_SP})]		// load new stack pointer
	mov	x0, x1
	bl	pmap_setttb
	mov	x3, x19

	mov	sp, x3

	ldp	x19, x20,  [x3, #({SF_X19})]
	ldp	x21, x22,  [x3, #({SF_X21})]
	ldp	x23, x24,  [x3, #({SF_X23})]
	ldp	x25, x26,  [x3, #({SF_X25})]
	ldp	x27, x28,  [x3, #({SF_X27})]
	ldp	x29, x30,  [x3, #({SF_X29})]
	add	sp, sp, #({SWITCHFRAME_SZ})
	ret
	.size	cpu_switchto_asm, . - cpu_switchto_asm

/*
 * proc_trampoline: where a thread built by cpu_fork starts, with the switch
 * frame's sf_x19 (the function) and sf_x20 (its argument) restored. The Rust
 * side (proc_trampoline_run) runs proc_trampoline_mi and then the function;
 * the return to user mode (syscall_return) comes with M6.
 */
	.globl	proc_trampoline
	.type	proc_trampoline, %function
	.p2align 2
proc_trampoline:
	mov	x0, x19
	mov	x1, x20
	bl	proc_trampoline_run
	brk	#0
	.size	proc_trampoline, . - proc_trampoline
