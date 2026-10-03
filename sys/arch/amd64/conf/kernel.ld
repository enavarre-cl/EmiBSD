/* EmiBSD amd64 linker script (Limine protocol). Identical to arm64's except OUTPUT_FORMAT. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
}

SECTIONS
{
    /* Higher half, where Limine loads non-relocatable kernels. */
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .rodata : {
        *(.rodata .rodata.*)

        /* The fault handlers copy.S declares (DECLARE_ONFAULT); the page fault handler
         * accepts a pcb_onfault only from this table. */
        . = ALIGN(8);
        __nofault_start = .;
        KEEP(*(.nofault.*))
        KEEP(*(.nofault))
        __nofault_end = .;
    } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .data : {
        *(.data .data.*)

        /* Limine requests: kept even though nothing references them by symbol. */
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
