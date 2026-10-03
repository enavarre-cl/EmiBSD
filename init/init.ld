/* EmiBSD init: a static, non-PIE ELF the kernel's exec maps at these addresses (M6). */
ENTRY(_start)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    note    PT_NOTE;
    data    PT_LOAD;
}

SECTIONS
{
    . = 0x400000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    .note.openbsd.ident : {
        KEEP(*(.note.openbsd.ident))
    } :rodata :note

    . = ALIGN(4096);

    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note.GNU-stack .note.gnu.*)
        *(.comment)
    }
}
