/*
 * EmiBSD amd64 linker script. The same ELF boots from Limine and from OpenBSD's efiboot
 * (M14). It differs from arm64's: OpenBSD's amd64 layout (conf/ld.script): the kernel is
 * linked at KERNTEXTOFF (0xffffffff81000000) with physical load addresses from 0x1000000
 * (AT()), entered by boot(8) at `start` (locore0.S, 32-bit) and by Limine at `_start`
 * (its entry point request, sys/stand/mod.rs).
 */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(start)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
    /* boot(8) fills this segment with random data (loadfile_elf's PT_OPENBSD_RANDOMIZE). */
    openbsd_randomize PT_OPENBSD_RANDOMIZE;
}

__kernel_base = 0xffffffff80000000;
__kernel_virt_base = __kernel_base + 0x1000000;
__kernel_phys_base = 0x1000000;

SECTIONS
{
    /* KERNTEXTOFF; physical = virtual - KERNBASE (locore0.S's RELOC()). */
    . = __kernel_virt_base;

    .text : AT(ADDR(.text) - __kernel_base) {
        KEEP(*(.text.locore0))
        *(.text .text.*)
    } :text

    PROVIDE(etext = .);
    _etext = .;

    . = ALIGN(0x1000);

    .rodata : AT(ADDR(.rodata) - __kernel_base) {
        __rodata_start = .;
        *(.rodata .rodata.*)

        /* The fault handlers copy.S declares (DECLARE_ONFAULT); the page fault handler
         * accepts a pcb_onfault only from this table. */
        . = ALIGN(8);
        __nofault_start = .;
        KEEP(*(.nofault.*))
        KEEP(*(.nofault))
        __nofault_end = .;
    } :rodata

    /* dev/rnd.c's boot seed (entropy_pool0, rs_buf0) and init_main.c's __guard_local, which
     * boot(8) fills; read-only, inside the rodata segment, as in OpenBSD's ld.script. */
    . = ALIGN(0x1000);
    .openbsd.randomdata : AT(ADDR(.openbsd.randomdata) - __kernel_base) {
        *(.openbsd.randomdata .openbsd.randomdata.*)
    } :rodata :openbsd_randomize

    . = ALIGN(0x1000);
    PROVIDE(erodata = .);
    _erodata = .;

    . = ALIGN(0x1000);

    .data : AT(ADDR(.data) - __kernel_base) {
        __data_start = .;
        *(.data .data.*)
        /* Not an orphan after `end`: locore0.S puts its tables right after the image. */
        *(.got .got.*)

        /* Limine requests: kept even though nothing references them by symbol. */
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :data

    . = ALIGN(0x1000);
    PROVIDE(edata = .);
    _edata = .;

    .bss : AT(ADDR(.bss) - __kernel_base) {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(0x1000);
    } :data
    __kernel_bss_end = .;

    . = ALIGN(0x200000);
    _end = .;
    PROVIDE(end = .);
    __kernel_phys_end = . - __kernel_base;

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
