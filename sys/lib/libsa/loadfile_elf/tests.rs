//! `loadfile` on a small hand-made ELF64 "kernel": text, data with bss, a random segment
//! and a symbol table, loaded through `LOADADDR` into a buffer.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::*;
use crate::arc4::{Rc4Ctx, rc4_keysetup};
use crate::cread::close;
use crate::hdr::exec_elf::{ELFCLASS64, ELFMAG, Elf64Ehdr, Elf64Phdr, Elf64Shdr};
use crate::loadfile::{LOAD_ALL, loadfile};
use crate::testutil::{LOADBASE, add_file, output, setup};

const PADDR: u64 = 0x100_0000;
const ENTRY: u64 = 0xffff_ffff_8100_0000;

/// The ELF file and the bytes of its symbol, string and section-name tables.
fn kernel() -> (Vec<u8>, [Vec<u8>; 3]) {
    let text: Vec<u8> = (0..16).collect();
    let data = vec![0xdd; 8];
    let symtab = vec![0x5a; 24];
    let strtab = b"\0_start\0".to_vec();
    let shstrtab = b"\0.symtab\0.strtab\0.shstrtab\0.text\0".to_vec();

    let mut f = vec![0u8; 0x100];
    f.extend_from_slice(&text); // 0x100
    f.extend_from_slice(&data); // 0x110
    f.extend_from_slice(&symtab); // 0x118
    f.extend_from_slice(&strtab); // 0x130
    f.extend_from_slice(&shstrtab); // 0x138
    let shoff = f.len().next_multiple_of(8);
    f.resize(shoff, 0);

    let sh = |name, ty, off, size| Elf64Shdr {
        sh_name: name,
        sh_type: ty,
        sh_offset: off,
        sh_size: size,
        ..Default::default()
    };
    let shdrs = [
        Elf64Shdr::default(),
        sh(27, 1, 0x100, 16), // .text, PROGBITS: not copied
        sh(1, SHT_SYMTAB, 0x118, 24),
        sh(9, SHT_STRTAB, 0x130, 8),
        sh(17, SHT_STRTAB, 0x138, shstrtab.len() as u64),
    ];
    for s in &shdrs {
        f.extend_from_slice(s.as_bytes());
    }

    let mut ident = [0u8; 16];
    ident[..4].copy_from_slice(ELFMAG);
    ident[4] = ELFCLASS64;
    let ehdr = Elf64Ehdr {
        e_ident: ident,
        e_entry: ENTRY + 0x10,
        e_phoff: 64,
        e_shoff: shoff as u64,
        e_phentsize: 56,
        e_phnum: 3,
        e_shentsize: 64,
        e_shnum: shdrs.len() as u16,
        e_shstrndx: 4,
        ..Default::default()
    };
    f[..64].copy_from_slice(ehdr.as_bytes());
    let ph = |ty, flags, off, paddr, filesz, memsz| Elf64Phdr {
        p_type: ty,
        p_flags: flags,
        p_offset: off,
        p_vaddr: ENTRY + (paddr - PADDR),
        p_paddr: paddr,
        p_filesz: filesz,
        p_memsz: memsz,
        p_align: 0x1000,
    };
    let phdrs = [
        ph(PT_LOAD, PF_R | PF_X, 0x100, PADDR, 16, 16),
        ph(PT_LOAD, PF_R | PF_W, 0x110, PADDR + 0x1000, 8, 32),
        ph(PT_OPENBSD_RANDOMIZE, PF_R, 0, PADDR + 0x1010, 8, 8),
    ];
    for (i, p) in phdrs.iter().enumerate() {
        f[64 + 56 * i..64 + 56 * (i + 1)].copy_from_slice(&{
            let mut b = [0u8; 56];
            b.copy_from_slice(
                // SAFETY: `Elf64Phdr` is 56 bytes of `#[repr(C)]` integers.
                unsafe { core::slice::from_raw_parts((p as *const Elf64Phdr).cast::<u8>(), 56) },
            );
            b
        });
    }
    (f, [symtab, strtab, shstrtab])
}

#[test]
fn loads_segments_symbols_and_marks() {
    let _g = setup();
    let (file, [symtab, strtab, shstrtab]) = kernel();
    add_file("elf", Box::leak(file.into_boxed_slice()));

    let mut mem = vec![0xeeu8; 0x2000];
    let base = mem.as_mut_ptr() as u64;
    LOADBASE.store(base.wrapping_sub(PADDR), Ordering::Relaxed);
    // SAFETY: the test is single-threaded under `setup`'s lock.
    unsafe { rc4_keysetup(RANDOMCTX.get_mut(), b"seed") };

    let mut marks = [0u64; MARK_MAX];
    // SAFETY: LOADADDR maps the kernel's 0x1000000.. into `mem`, which is big enough.
    let fd = unsafe { loadfile(b"elf:/bsd", &mut marks, LOAD_ALL) }.unwrap();
    close(fd).unwrap();

    let size = 0x1060 + 320 + 24 + 8 + shstrtab.len().next_multiple_of(8) as u64;
    assert_eq!(
        output(),
        alloc::format!("16+8+24 [24+8+{}]=0x{:x}\n\r", shstrtab.len(), size)
    );
    assert_eq!(marks[MARK_START], base);
    assert_eq!(marks[MARK_ENTRY], base + 0x10);
    assert_eq!(marks[MARK_VENTRY], ENTRY + 0x10);
    assert_eq!(marks[MARK_SYM], base + 0x1020);
    assert_eq!(marks[MARK_RANDOM], base + 0x1010);
    assert_eq!(marks[MARK_ERANDOM], base + 0x1018);
    assert_eq!(marks[MARK_END], base + size);

    // text, data, the zeroed bss with the random bytes in it
    assert_eq!(mem[..16], (0..16).collect::<Vec<u8>>()[..]);
    assert_eq!(mem[0x1000..0x1008], [0xdd; 8]);
    let mut ctx = Rc4Ctx::new();
    rc4_keysetup(&mut ctx, b"seed");
    let mut rnd = [0u8; 8];
    rc4_getbytes(&mut ctx, &mut rnd);
    assert_eq!(mem[0x1010..0x1018], rnd);
    assert_eq!(mem[0x1018..0x1020], [0; 8]);
    // the ELF header the kernel gets: no program headers, sections right after it
    let ehdr = Elf64Ehdr::from_bytes(&mem[0x1020..]).unwrap();
    assert_eq!((ehdr.e_phoff, ehdr.e_phnum, ehdr.e_shoff), (0, 0, 64));
    // the section headers, the tables offset after them, and the tables
    let sh = |i: usize| Elf64Shdr::from_bytes(&mem[0x1060 + 64 * i..]).unwrap();
    assert_eq!(sh(2).sh_offset, 64 + 320);
    assert_eq!(sh(2).sh_flags & SHF_ALLOC, SHF_ALLOC);
    assert_eq!(sh(3).sh_offset, 64 + 320 + 24);
    assert_eq!(sh(1).sh_offset, 0x100);
    let tables = 0x1060 + 320;
    assert_eq!(mem[tables..tables + 24], symtab[..]);
    assert_eq!(mem[tables + 24..tables + 32], strtab[..]);
    assert_eq!(mem[tables + 32..tables + 32 + shstrtab.len()], shstrtab[..]);
}
