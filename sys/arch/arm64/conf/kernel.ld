/*
 * EmiBSD arm64 linker script. The same ELF boots from Limine and from OpenBSD's efiboot
 * (M14, track A3). The kernel is linked at 0xffffffff80000000 (Limine's higher half, which
 * arm64/pmap.rs adopts) with physical addresses from 0 (AT()): efiboot's LOADADDR keeps
 * the low 39 bits of p_paddr and adds its 64 MB block's address (efi_loadaddr), so the
 * image lands at the block's start. The ELF entry is the physical offset of locore0.S's
 * `_start` for the same reason (e_entry goes through LOADADDR too); Limine enters the
 * boot glue's `_start` (sys/stand/mod.rs) through its entry point request.
 */
OUTPUT_FORMAT(elf64-littleaarch64)
ENTRY(__start_phys)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
    /* boot(8) fills this segment with random data (loadfile_elf's PT_OPENBSD_RANDOMIZE). */
    openbsd_randomize PT_OPENBSD_RANDOMIZE;
}

__kernel_base = 0xffffffff80000000;

SECTIONS
{
    /* Higher half, where Limine loads non-relocatable kernels; physical = virtual - base. */
    . = __kernel_base;

    .text : AT(ADDR(.text) - __kernel_base) {
        /* locore0.S's _start first: efiboot cleans the cache from the entry point on. */
        KEEP(*(.text.locore0))
        *(.text .text.*)
    } :text

    PROVIDE(_etext = .);
    PROVIDE(etext = .);

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .rodata : AT(ADDR(.rodata) - __kernel_base) {
        *(.rodata .rodata.*)
    } :rodata

    /* dev/rnd.c's boot seed (entropy_pool0, rs_buf0) and init_main.c's __guard_local, which
     * boot(8) fills; read-only, inside the rodata segment, as in OpenBSD's kern.ldscript. */
    . = ALIGN(0x1000);
    .openbsd.randomdata : AT(ADDR(.openbsd.randomdata) - __kernel_base) {
        *(.openbsd.randomdata .openbsd.randomdata.*)
    } :openbsd_randomize :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .data : AT(ADDR(.data) - __kernel_base) {
        *(.data .data.*)

        /* Limine requests: kept even though nothing references them by symbol. */
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :data

    .bss : AT(ADDR(.bss) - __kernel_base) {
        . = ALIGN(16);
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
    } :data

    /* locore0.S zeroes __bss_start .. _end; locore.S's esym starts at end. */
    _end = .;
    PROVIDE(end = .);

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}

/* The ELF entry: _start's offset in the image (see above). */
__start_phys = _start - __kernel_base;
