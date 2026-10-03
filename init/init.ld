/* EmiBSD init: a static, non-PIE ELF the kernel's exec maps at these addresses (M6). */
ENTRY(_start)

/*
 * syscalls is PT_OPENBSD_SYSCALLS (0x65a3dbe9): the system call pin table the kernel's
 * exec reads into ps_pin (pin_check refuses a system call from anywhere else). Its
 * section is allocated, so it also sits in the rodata segment.
 */
PHDRS
{
    text     PT_LOAD;
    rodata   PT_LOAD;
    note     PT_NOTE;
    syscalls 0x65a3dbe9;
    data     PT_LOAD;
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

    .openbsd.syscalls : {
        KEEP(*(.openbsd.syscalls))
    } :rodata :syscalls

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
