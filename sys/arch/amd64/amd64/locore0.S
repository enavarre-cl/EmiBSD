
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
 * EmiBSD: assembled by global_asm! (locore0.rs) in AT&T syntax, without the C preprocessor:
 * the braced NAME placeholders are what assym.h and the headers provide in C, RELOC(x) is written
 * out as `x - KERNBASE`, the fillkpt macros are .macro, #if is .if. Deviations (locore0.rs
 * lists them): no Meltdown or SEV probe, no #VC handler beyond the termination request, no
 * pmap_direct_rand, no proc0 stack in the bootstrap tables (a 64 KiB boot stack in .bss),
 * no atdevbase relocation, and the C's `call init_x86_64; call main` is `call bootarg_main`
 * (sys/stand/bootarg.rs).
 */

	.section .text.locore0,"ax",@progbits

	.code32

	.globl	start
start:	movw	$0x1234,0x472			# warm boot

	/*
	 * Load parameters from stack
	 * (howto, bootdev, bootapiver, esym, extmem (unused), cnvmem, ac, av)
	 */
	movl	4(%esp),%eax
	movl	%eax, {BOOTHOWTO} - {KERNBASE}
	movl	8(%esp),%eax
	movl	%eax, {BOOTDEV} - {KERNBASE}

	/*
	 * Syms are placed after last load and bss of the kernel.
	 * XXX Boot ignores 2MB roundup of _end, so esyms can be < _end.
	 */
	movl	16(%esp), %eax
	testl	%eax,%eax
	jz	1f
	addl	${KERNBASE_LO},%eax
	movl	${ESYM} - {KERNBASE},%ebp
	movl	%eax,(%ebp)
	movl	${KERNBASE_HI},4(%ebp)
1:
	movl	24(%esp), %eax
	movl	%eax, {BIOSBASEMEM} - {KERNBASE}

	movl	12(%esp), %eax
	movl	%eax, {BOOTAPIVER} - {KERNBASE}

	/*
	 * Copy the boot arguments to bootinfo[] in machdep.c.
	 *
	 * We are passed the size of the data /boot passed to us in
	 * 28(%esp). We copy up to bootinfo_size bytes of data into
	 * bootinfo and report back how much we copied in bootinfo_size.
	 *
	 * machdep.c can then take action if bootinfo_size >= bootinfo[]
	 * (which would meant that we may have been passed too much data).
	 */
	movl	28(%esp), %eax
	movl	%eax, %ecx
	cmpl	{BOOTINFO_SIZE} - {KERNBASE}, %ecx	/* Too much? */
	jb	bi_size_ok
	movl	{BOOTINFO_SIZE} - {KERNBASE}, %ecx	/* Only copy this much */
bi_size_ok:
	movl	%eax, {BOOTINFO_SIZE} - {KERNBASE}	/* Report full amount */

	movl	${BOOTINFO} - {KERNBASE}, %edi		/* Destination */
	movl	32(%esp), %esi			/* Source */
	rep movsb				/* Copy this many bytes */

	/* First, reset the PSL. */
	pushl	${PSL_MBO}
	popfl

	/*
	 * Setup temporary #VC trap handler, in case we are running
	 * on an AMD CPU in SEV-ES guest mode.  Will be reset by
	 * init_x86_64().
	 */
	movl	$early_idt - {KERNBASE}, %ecx
	movl	${T_VC}, %edx
	leal	(%ecx, %edx, 8), %ecx		/* 32bit #VC IDT slot */

	pushl	%cs				/* get current %cs */
	popl	%ebx
	shll	$16, %ebx

	movl	$locore_vc_trap32 - {KERNBASE}, %eax
	andl	$0x0000ffff, %eax
	orl	%ebx, %eax			/* use current %cs */
	movl	%eax, (%ecx)

	movl	$locore_vc_trap32 - {KERNBASE}, %eax
	andl	$0xffff0000, %eax
	orl	$((0x80 | {SDT_SYS386IGT}) << 8), %eax
	movl	%eax, 4(%ecx)

	movl	$idtlc - {KERNBASE}, %eax
	lidt	(%eax)

	/* Reset debug control registers */
	xorl	%eax,%eax
	movl	%eax,%dr6
	movl	%eax,%dr7

	/* %eax still zero from above */
	cpuid
	movl	%eax, {CPUID_LEVEL} - {KERNBASE}
	movl	${CPU_VENDOR} - {KERNBASE},%ebp
	movl	%ebx,(%ebp)
	movl	%edx,4(%ebp)
	movl	%ecx,8(%ebp)
	movl	$0, 12(%ebp)

	/*
	 * The Meltdown probe (cpu_meltdown, pg_g_kern) is not here: pg_g_kern stays 0, as
	 * for an insecure CPU (locore0.rs).
	 */

	movl	$1,%eax
	cpuid
	movl	%eax, {CPU_ID} - {KERNBASE}
	movl	%ebx, {CPU_EBXFEATURE} - {KERNBASE}
	movl	%ecx, {CPU_ECXFEATURE} - {KERNBASE}
	movl	%edx, {CPU_FEATURE} - {KERNBASE}

	movl	$0x80000001, %eax
	cpuid
	andl	${CPUID_NXE}, %edx	/* other bits may clash */
	jz	cont

	/*
	 * We have NX, set pg_nx accordingly.
	 * NX bit is bit 63 (bit 31 of the second 32 bit dword) - need
	 * to use 32 bit registers here
	 */
	pushl	%edx
	movl	{PG_NX} - {KERNBASE} + 4, %edx	/* Second dword */
	orl	$0x80000000, %edx		/* Bit 31 (really 63) */
	movl	%edx, {PG_NX} - {KERNBASE} + 4
	popl	%edx
cont:
	orl	%edx, {CPU_FEATURE} - {KERNBASE}

	/*
	 * The AMD SEV probe (cpu_sev_guestmode, pg_crypt, the page frame masks) is not here:
	 * AMDSEV is not configured, pg_crypt stays 0 (locore0.rs).
	 */

	/*
	 * Finished with old stack; load new %esp now instead of later so we
	 * can trace this code without having to worry about the trace trap
	 * clobbering the memory test or the zeroing of the bss+bootstrap page
	 * tables.
	 *
	 * The boot program should check:
	 *	text+data <= &stack_variable - more_space_for_stack
	 *	text+data+bss+pad+space_for_page_tables <= end_of_memory
	 * Oops, the gdt is in the carcass of the boot program so clearing
	 * the rest of memory is still not possible.
	 */
	movl	$tmpstk - {KERNBASE},%esp

/*
 * Virtual address space of kernel:
 *
 * text | data | bss | [syms] | page dir | L1 ptp | L2 ptp | L3
 *			      0          1       2      3
 *
 * (EmiBSD: no proc0 kstack in the tables; the PROC0_*_OFF and TABLESIZE placeholders
 * are computed in locore0.rs.)
 */

.macro fillkpt
	pushl	%ebp				/* save */
	movl	{PG_CRYPT} - {KERNBASE} + 4, %ebp	/* C bit? */
1:	movl	%eax,(%ebx)			/* store phys addr */
	movl	%ebp,4(%ebx)			/* upper 32 bits */
	addl	$8,%ebx				/* next pte/pde */
	addl	${NBPG},%eax			/* next phys page */
	loop	1b				/* till finished */
	popl	%ebp				/* restore */
.endm

.macro fillkpt_nx
	pushl	%ebp				/* save */
	movl	{PG_NX} - {KERNBASE} + 4, %ebp	/* NX bit? */
	orl	{PG_CRYPT} - {KERNBASE} + 4, %ebp	/* C bit? */
1:	movl	%eax,(%ebx)			/* store phys addr */
	movl	%ebp,4(%ebx)			/* upper 32 bits */
	addl	$8,%ebx				/* next pte/pde */
	addl	${NBPG},%eax			/* next phys page */
	loop	1b				/* till finished */
	popl	%ebp
.endm

	/* Find end of kernel image. */
	movl	$end - {KERNBASE},%edi
	/* Save the symbols (if loaded). */
	movl	{ESYM} - {KERNBASE},%eax
	testl	%eax,%eax
	jz	1f
	subl	${KERNBASE_LO},%eax	/* XXX */
	/* Page tables must be after symbols and after kernel image. */
	cmpl	%eax,%edi
	jg	1f
	movl	%eax,%edi
1:
	/* Clear tables */
	movl	%edi,%esi
	addl	${PGOFSET},%esi
	andl	$~{PGOFSET},%esi

	movl	%esi,%edi
	xorl	%eax,%eax
	cld
	movl	${TABLESIZE},%ecx
	shrl	$2,%ecx
	rep
	stosl

	leal	{PROC0_PTP1_OFF}(%esi), %ebx

	/*
	 * Compute etext - KERNBASE. This can't be > 4G, or we can't deal
	 * with it anyway, since we can't load it in 32 bit mode. So use
	 * the bottom 32 bits.
	 */
	movl	$etext - {KERNBASE},%edx
	addl	${PGOFSET},%edx
	andl	$~{PGOFSET},%edx

	/*
	 * Skip the first 16 MB.
	 */
	movl	${KERNTEXTOFF_OFF},%eax
	movl	%eax,%ecx
	shrl	$({PGSHIFT}-3),%ecx	/* ((n >> PGSHIFT) << 3) for # pdes */
	addl	%ecx,%ebx

	/* Map kernel text RO, X */
	movl	%edx,%ecx
	subl	%eax,%ecx
	shrl	${PGSHIFT},%ecx
	orl	$({PG_V}|{PG_KR}),%eax
	fillkpt

	/* Map .rodata RO, NX */
	movl	$__rodata_start - {KERNBASE}, %eax
	movl	$erodata - {KERNBASE}, %ecx
	addl	${PGOFSET}, %ecx
	andl	$~{PGOFSET}, %ecx
	subl	%eax, %ecx
	shrl	${PGSHIFT}, %ecx
	orl	$({PG_V}|{PG_KR}), %eax
	fillkpt_nx

	/* Map the data and BSS sections RW, NX */
	movl	$__data_start - {KERNBASE}, %eax
	movl	$__kernel_bss_end - {KERNBASE},%ecx
	addl	${PGOFSET}, %ecx
	andl	$~{PGOFSET}, %ecx
	subl	%eax, %ecx
	shrl	${PGSHIFT},%ecx
	orl	$({PG_V}|{PG_KW}), %eax
	fillkpt_nx

	/* Map "hole" at end of BSS RO, NX */
	movl	$__kernel_bss_end - {KERNBASE}, %eax
	movl	$end - {KERNBASE}, %ecx
	addl	${PGOFSET}, %ecx
	andl	$~{PGOFSET}, %ecx
	cmpl	%eax, %ecx
	je	map_syms
	subl	%eax, %ecx
	shrl	${PGSHIFT}, %ecx
	orl	$({PG_V}|{PG_KR}), %eax
	fillkpt_nx

map_syms:
	/* Map symbol space RO, NX */
	movl	$end - {KERNBASE}, %eax
	movl	%esi, %ecx
	addl	${PGOFSET}, %ecx
	andl	$~{PGOFSET}, %ecx
	cmpl	%eax, %ecx
	je	map_tables
	subl	%eax, %ecx
	shrl	${PGSHIFT}, %ecx
	orl	$({PG_V}|{PG_KR}), %eax
	fillkpt_nx

map_tables:
	/* Map the bootstrap tables RW, NX */
	movl	%esi, %edx
	leal	({PG_V}|{PG_KW})(%edx),%eax
	movl	${TABLESIZE},%ecx
	shrl	${PGSHIFT},%ecx
	fillkpt_nx

	/* SEV-ES's GHCB remap: AMDSEV is not configured (cpu_sev_guestmode is 0). */

	/* Map ISA I/O mem (later atdevbase) RW, NX */
	movl	$({IOM_BEGIN}|{PG_V}|{PG_KW}/*|PG_N*/),%eax
	movl	$({IOM_SIZE}>>{PGSHIFT}),%ecx
	fillkpt_nx

	/* Set up level 2 pages (RWX) */
	leal	{PROC0_PTP2_OFF}(%esi),%ebx
	leal	{PROC0_PTP1_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	$({NKL2_KIMG_ENTRIES}+1),%ecx
	fillkpt

.if {L2_SLOT_KERNBASE} > 0
	/* If needed, set up L2 entries for actual kernel mapping (RWX) */
	leal	({PROC0_PTP2_OFF}+ {L2_SLOT_KERNBASE}*8)(%esi),%ebx
	leal	{PROC0_PTP1_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	$({NKL2_KIMG_ENTRIES}+1),%ecx
	fillkpt
.endif

	/* Set up level 3 pages (RWX) */
	leal	{PROC0_PTP3_OFF}(%esi),%ebx
	leal	{PROC0_PTP2_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NKL3_KIMG_ENTRIES},%ecx
	fillkpt

.if {L3_SLOT_KERNBASE} > 0
	/* If needed, set up L3 entries for actual kernel mapping (RWX) */
	leal	({PROC0_PTP3_OFF}+ {L3_SLOT_KERNBASE}*8)(%esi),%ebx
	leal	{PROC0_PTP2_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NKL3_KIMG_ENTRIES},%ecx
	fillkpt
.endif

	/* Set up top level entries for identity mapping (RWX) */
	leal	{PROC0_PML4_OFF}(%esi),%ebx
	leal	{PROC0_PTP3_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NKL4_KIMG_ENTRIES},%ecx
	fillkpt

	/* Set up top level entries for actual kernel mapping (RWX) */
	leal	({PROC0_PML4_OFF} + {L4_SLOT_KERNBASE}*8)(%esi),%ebx
	leal	{PROC0_PTP3_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NKL4_KIMG_ENTRIES},%ecx
	fillkpt

	/*
	 * Map the first 4 GB with the direct map. We'll map the rest
	 * in pmap_bootstrap. But we always need the first 4GB during
	 * bootstrap. The direct map is mapped RW, NX. We also change
	 * the permissions on the 2MB pages corresponding to the kernel
	 * PAs to RO to prevent someone writing to the kernel area
	 * via the direct map.
	 */
	leal	{PROC0_DMP2_OFF}(%esi), %ebx
	xorl	%eax, %eax
	movl	$({NDML2_ENTRIES} * {NPDPG}), %ecx
1:	orl	$({PG_V}|{PG_KW}|{PG_PS}), %eax
	orl	{PG_G_KERN} - {KERNBASE}, %eax
	cmpl	$__kernel_phys_base, %eax
	jl	store_pte
	cmpl	$__kernel_phys_end, %eax
	jg	store_pte
	andl	$(~{PG_KW}), %eax
store_pte:
	movl	%eax, (%ebx)
	pushl	%ebp
	movl	{PG_NX} - {KERNBASE} + 4, %ebp
	orl	{PG_CRYPT} - {KERNBASE} + 4, %ebp
	movl	%ebp, 4(%ebx)
	popl	%ebp
	addl	$8, %ebx
	addl	${NBPD_L2}, %eax
	loop	1b

	leal	{PROC0_DMP3_OFF}(%esi), %ebx
	leal	{PROC0_DMP2_OFF}(%esi), %eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NDML2_ENTRIES}, %ecx
	fillkpt_nx

	/* No pmap_direct_rand: the direct map starts at PDIR_SLOT_DIRECT itself. */
	leal	({PROC0_PML4_OFF} + {PDIR_SLOT_DIRECT} * 8)(%esi), %ebx
	leal	{PROC0_DMP3_OFF}(%esi), %eax
	orl	$({PG_V}|{PG_KW}), %eax
	movl	${NDML3_ENTRIES}, %ecx
	fillkpt_nx

	/* Install recursive top level PDE */
	leal	({PROC0_PML4_OFF} + {PDIR_SLOT_PTE}*8)(%esi),%ebx
	leal	{PROC0_PML4_OFF}(%esi),%eax
	orl	$({PG_V}|{PG_KW}),%eax
	movl	%eax,(%ebx)
	pushl	%ebp
	movl	{PG_NX} - {KERNBASE} + 4, %ebp
	orl	{PG_CRYPT} - {KERNBASE} + 4, %ebp
	movl	%ebp, 4(%ebx)
	popl	%ebp

	/*
	 * Startup checklist:
	 * 1. Enable PAE (and SSE while here).
	 */
	movl	%cr4,%eax
	orl	${CR4_DEFAULT},%eax
	movl	%eax,%cr4

	/*
	 * 2. Set Long Mode Enable in EFER. Also enable the
	 *    syscall extensions and NX (if available).
	 */
	movl	${MSR_EFER},%ecx
	rdmsr
	movl	%eax,%ebx
	xorl	%eax,%eax	/* XXX */
	orl	$({EFER_LME}|{EFER_SCE}),%eax
	/* If set, preserve LMA */
	testl	${EFER_LMA},%ebx
	jz	efer_nxe
	orl	${EFER_LMA},%eax
efer_nxe:
	movl	{PG_NX} - {KERNBASE} + 4, %ebx
	cmpl	$0, %ebx
	je	write_efer
	orl	${EFER_NXE}, %eax
write_efer:
	wrmsr

	/*
	 * 3. Load %cr3 with pointer to PML4.
	 */
	movl	%esi,%eax
	movl	%eax,%cr3

	/*
	 * 4. Enable paging and the rest of it.
	 */
	movl	%cr0,%eax
	orl	${CR0_DEFAULT},%eax
	movl	%eax,%cr0
	jmp	compat
compat:

	/*
	 * 5.
	 * Not quite done yet, we're now in a compatibility segment,
	 * in legacy mode. We must jump to a long mode segment.
	 * Need to set up a temporary GDT with a long mode segment
	 * in it to do that.
	 */

	movl	$gdt64 - {KERNBASE},%eax
	lgdt	(%eax)
	movl	$farjmp64 - {KERNBASE},%eax
	ljmp	*(%eax)

.code64
longmode:
	/*
	 * 6.
	 * Finally, we're in long mode. However, we're still
	 * in the identity mapped area (could not jump out
	 * of that earlier because it would have been a > 32bit
	 * jump). We can do that now, so here we go.
	 */
	movabsq	$longmode_hi,%rax
	jmp	*%rax
longmode_hi:
	/*
	 * We have arrived.
	 * There's no need anymore for the identity mapping in low
	 * memory, remove it.
	 */
	movabsq	${KERNBASE},%r8

.if {L2_SLOT_KERNBASE} > 0
	movq	$({NKL2_KIMG_ENTRIES}+1),%rcx
	leaq	{PROC0_PTP2_OFF}(%rsi),%rbx
	addq	%r8, %rbx
1:	movq	$0 ,(%rbx)
	addq	$8,%rbx
	loop	1b
.endif

.if {L3_SLOT_KERNBASE} > 0
	movq	${NKL3_KIMG_ENTRIES},%rcx
	leaq	{PROC0_PTP3_OFF}(%rsi),%rbx
	addq	%r8, %rbx
1:	movq	$0 ,(%rbx)
	addq	$8,%rbx
	loop	1b
.endif

	movq	${NKL4_KIMG_ENTRIES},%rcx
	leaq	{PROC0_PML4_OFF}(%rsi),%rbx	# old, phys address of PML4
	addq	%r8, %rbx			# new, virtual address of PML4
1:	movq	$0, (%rbx)
	addq	$8,%rbx
	loop	1b

	/* atdevbase is the direct map's ISA hole here (bus_space.rs): nothing to relocate. */

	/* Record start of symbols */
	movq	$__kernel_bss_end, {SSYM}(%rip)

	/*
	 * Set up bootstrap stack: the kernel's boot stack (locore0_bootstk, 64 KiB as
	 * Limine's), not proc0's u-area: proc0paddr is machdep.rs's static PROC0_UAREA and
	 * x86_64_proc0_tss_ldt_init sets its pcb_cr3 from %cr3.
	 */
	leaq	locore0_bootstk_end(%rip),%rsp

	xorq	%rbp,%rbp		# mark end of frames

	xorw	%ax,%ax
	movw	%ax,%gs
	movw	%ax,%fs

	leaq	{TABLESIZE}(%rsi),%rdi
	subq	$({NBPG}*5), %rdi

	/* init_x86_64 and main: the boot glue's entry for boot(8) (sys/stand/bootarg.rs). */
	call	bootarg_main
	ud2

	.code32
	.globl	locore_vc_trap32
locore_vc_trap32:
	/* !AMDSEV: graceful termination */

.Lterminate32:
	movl	${MSR_PROTO_TERMINATION_REQ}, %eax
	movl	${MSR_SEV_GHCB}, %ecx
	wrmsr
	.byte	0xf3, 0x0f, 0x01, 0xd9	/* rep vmmcall */
.Lterm_loop32:
	hlt
	jmp	.Lterm_loop32
	.code64

	/* No .codepatch: there is no codepatch here. */

	.data
idtlc:					/* temporary locore IDT */
	.word	early_idt_end-early_idt-1
	.long	early_idt - {KERNBASE}
	.align 64, 0xcc

early_idt:
	.rept	{NIDT}
	.quad	0x0000000000000000
	.quad	0x0000000000000000
	.endr
early_idt_end:

gdt64:
	.word	gdt64_end-gdt64_start-1
	.quad	gdt64_start - {KERNBASE}
	.align 64, 0xcc

gdt64_start:
	.quad 0x0000000000000000	/* always empty */
	.quad 0x00af9a000000ffff	/* kernel CS */
	.quad 0x00cf92000000ffff	/* kernel DS */
gdt64_end:

farjmp64:
	.long	longmode - {KERNBASE}
	.word	{GSEL_KCODE}

	.align 8, 0xcc
	.space 512
tmpstk:

	.bss
	.balign	16
locore0_bootstk:
	.space	{BOOTSTACK_SIZE}
locore0_bootstk_end:

	.text
