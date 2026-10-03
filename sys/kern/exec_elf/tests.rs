//! Host tests for the ELF loader: the note check, the pin table (`elf_read_pintable`,
//! `elf_adjustpins`), the auxiliary vector's layout and, when `cargo xtask userland` has
//! built them, OpenBSD's own static PIE programs (`target/userland/amd64/root`): their
//! program headers, OpenBSD note, `PT_OPENBSD_SYSCALLS` table and the vmcmds a PIE load
//! produces.

use std::boxed::Box;
use std::path::PathBuf;
use std::vec::Vec;
use std::{assert, assert_eq, eprintln, fs, vec};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::exec_elf::{ELF_AUX_WORDS, EM_AMD64, NT_OPENBSD_PROF};
use crate::sys::syscall::{SYS_exit, SYS_getentropy, SYS_mmap, SYS_write};

/// A thread for the reads: an image read never looks at it.
fn test_proc() -> &'static Proc {
    Box::leak(Box::new(Proc::new()))
}

/// A note: the header, the name padded to 4 bytes, the descriptor padded to 4 bytes.
fn note(name: &[u8], ty: u32, desc: &[u8]) -> Vec<u8> {
    let mut n = Vec::new();
    n.extend_from_slice(&(name.len() as u32).to_ne_bytes());
    n.extend_from_slice(&(desc.len() as u32).to_ne_bytes());
    n.extend_from_slice(&ty.to_ne_bytes());
    n.extend_from_slice(name);
    n.resize(n.len() + elfround(name.len()) - name.len(), 0);
    n.extend_from_slice(desc);
    n.resize(n.len() + elfround(desc.len()) - desc.len(), 0);
    n
}

/// The header and the body of a note made by [`note`].
fn split(n: &[u8]) -> (ElfNote, &[u8]) {
    let Some(hdr) = read_image!(ElfNote, n, 0) else {
        panic!("short note");
    };
    (hdr, &n[size_of::<ElfNote>()..])
}

#[test]
fn note_name_knows_openbsd() {
    let n = note(b"OpenBSD\0", 1, &[0, 0, 0, 0]);
    let (hdr, body) = split(&n);
    assert_eq!(
        elf_os_pt_note_name(&hdr, body),
        Some((ELF_NOTE_NAME_OPENBSD, 1))
    );

    let n = note(b"OpenBSD\0", NT_OPENBSD_PROF, &[]);
    let (hdr, body) = split(&n);
    assert_eq!(
        elf_os_pt_note_name(&hdr, body),
        Some((ELF_NOTE_NAME_OPENBSD, NT_OPENBSD_PROF))
    );
}

#[test]
fn note_name_refuses_others() {
    for name in [&b"GNU\0"[..], b"OpenBSE\0", b"OpenBSD", b"OpenBSDx\0"] {
        let n = note(name, 1, &[0, 0, 0, 0]);
        let (hdr, body) = split(&n);
        assert_eq!(elf_os_pt_note_name(&hdr, body), None, "{name:?}");
    }
}

#[test]
fn adjustpins_rebases_offsets_only() {
    let mut pins = [0u32, 0x1010, u32::MAX, 0x1020];
    let (mut base, mut len) = (0x10_0000usize, 0x2000usize);
    elf_adjustpins(&mut base, &mut len, &mut pins, 0x1000);
    assert_eq!(pins, [0, 0x10, u32::MAX, 0x20]);
    assert_eq!((base, len), (0x10_1000, 0x1000));
}

/// `struct pinsyscalls` pairs as bytes.
fn pairs(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut v = Vec::new();
    for &(offset, sysno) in entries {
        v.extend_from_slice(&offset.to_ne_bytes());
        v.extend_from_slice(&sysno.to_ne_bytes());
    }
    v
}

/// A program header for `filesz` bytes at `offset`.
fn phdr(p_type: u32, offset: u64, filesz: u64) -> ElfPhdr {
    ElfPhdr {
        p_type,
        p_flags: PF_R,
        p_offset: offset,
        p_vaddr: 0,
        p_paddr: 0,
        p_filesz: filesz,
        p_memsz: filesz,
        p_align: 4,
    }
}

/// Gives a table `elf_read_pintable` made back.
fn free_pins(npins: i32, pins: Option<NonNull<u32>>) {
    if let Some(pins) = pins {
        free(
            pins.cast::<u8>(),
            M_PINSYSCALL,
            npins as usize * size_of::<u32>(),
        );
    }
}

#[test]
fn pintable_records_sites_duplicates_and_kbind() {
    let _guard = setup_real_memory();
    let p = test_proc();
    let image = pairs(&[(0x100, 1), (0x200, 4), (0x300, 4), (0x40, 3)]);
    let ph = phdr(PT_OPENBSD_SYSCALLS, 0, image.len() as u64);

    let (npins, pins) = elf_read_pintable(p, ExecFile::Image(&image), &ph, false, 0x1000);
    assert_eq!(npins, 5);
    let Some(table) = pins else {
        panic!("no pin table");
    };
    // SAFETY: the fresh table, `npins` entries.
    let t = unsafe { pins_slice(table, npins) };
    assert_eq!(t, &[0, 0x100, 0, 0x40, u32::MAX]);
    free_pins(npins, pins);

    // ld.so's table always allows kbind(2) from anywhere.
    let (npins, pins) = elf_read_pintable(p, ExecFile::Image(&image), &ph, true, 0x1000);
    assert_eq!(npins, SYS_kbind + 1);
    let Some(table) = pins else {
        panic!("no pin table");
    };
    // SAFETY: as above.
    let t = unsafe { pins_slice(table, npins) };
    assert_eq!(t[SYS_kbind as usize], u32::MAX);
    free_pins(npins, pins);
}

#[test]
fn pintable_refuses_bad_tables() {
    let _guard = setup_real_memory();
    let p = test_proc();
    // system call 0, a number past the table, an offset past the text, a ragged size
    for (image, len) in [
        (pairs(&[(0x10, 0)]), 0x1000),
        (pairs(&[(0x10, SYS_MAXSYSCALL as u32)]), 0x1000),
        (pairs(&[(0x2000, 1)]), 0x1000),
        (vec![0u8; 12], 0x1000),
    ] {
        let ph = phdr(PT_OPENBSD_SYSCALLS, 0, image.len() as u64);
        let (npins, pins) = elf_read_pintable(p, ExecFile::Image(&image), &ph, false, len);
        assert_eq!(npins, 0);
        assert!(pins.is_none());
    }
}

#[test]
fn read_from_image_is_bounded() {
    let p = test_proc();
    let image = [1u8, 2, 3, 4];
    let mut buf = [0u8; 2];
    assert_eq!(
        elf_read_from(p, ExecFile::Image(&image), 2, &mut buf),
        Ok(())
    );
    assert_eq!(buf, [3, 4]);
    assert_eq!(
        elf_read_from(p, ExecFile::Image(&image), 3, &mut buf),
        Err(Errno::ENOEXEC)
    );
}

#[test]
fn auxv_layout() {
    // twelve 16-byte entries, 24 stack words, au_id then four zero bytes then au_v
    assert_eq!(ELF_AUX_WORDS * size_of::<usize>(), ELF_AUX_ENTRIES * 16);
    let a = AuxInfo {
        au_id: AUX_openbsd_timekeep,
        _pad: 0,
        au_v: 0x1122_3344_5566_7788,
    };
    let b = a.to_bytes();
    assert_eq!(&b[0..4], &4000i32.to_ne_bytes());
    assert_eq!(&b[4..8], &[0, 0, 0, 0]);
    assert_eq!(&b[8..16], &0x1122_3344_5566_7788u64.to_ne_bytes());
}

/// OpenBSD's static PIE programs, when `cargo xtask userland --arch amd64` built them.
fn userland_binaries() -> Vec<(PathBuf, Vec<u8>)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/userland/amd64/root");
    let mut out = Vec::new();
    for name in ["sbin/init", "bin/ksh", "bin/echo", "bin/ls"] {
        let path = root.join(name);
        if let Ok(bytes) = fs::read(&path) {
            out.push((path, bytes));
        }
    }
    if out.is_empty() {
        eprintln!("exec_elf tests: no target/userland/amd64 binaries; run `just userland`");
    }
    out
}

#[test]
fn userland_static_pie_headers() {
    let _guard = setup_real_memory();
    let p = test_proc();
    for (path, image) in userland_binaries() {
        let Some(eh) = read_image!(ElfEhdr, &image, 0) else {
            panic!("{}: short", path.display());
        };
        assert!(is_elf(&eh), "{}", path.display());
        assert_eq!(eh.e_type, ET_DYN, "{}: static PIE", path.display());
        assert_eq!(eh.e_machine, EM_AMD64);
        assert!(eh.e_phnum <= ELF_MAX_VALID_PHDR);

        let file = ExecFile::Image(&image);
        let ph = match elf_read_phdrs(p, file, eh.e_phoff, usize::from(eh.e_phnum)) {
            Ok(ph) => ph,
            Err(e) => panic!("{}: phdrs: {e:?}", path.display()),
        };
        let has = |t: u32| ph.iter().any(|pp| pp.p_type == t);
        assert!(!has(PT_INTERP), "{}: no ld.so", path.display());
        assert!(has(PT_PHDR));
        assert!(has(PT_OPENBSD_SYSCALLS));
        assert!(has(PT_OPENBSD_RANDOMIZE));
        assert!(has(PT_GNU_RELRO));
        let base = ph.iter().position(|pp| pp.p_type == PT_LOAD);
        assert_eq!(
            base.map(|b| ph[b].p_vaddr),
            Some(0),
            "PIE base segment at 0"
        );

        // the OpenBSD note, as exec_elf_makecmds insists
        let mut pack = ExecPackage::new(b"test");
        pack.ep_image = Some(&image);
        let mut names = 0;
        assert_eq!(elf_os_pt_note(p, &mut pack, &eh, &mut names), Ok(()));
        assert_eq!(names & ELF_NOTE_NAME_OPENBSD, ELF_NOTE_NAME_OPENBSD);
    }
}

#[test]
fn userland_pie_load_and_pins() {
    let _guard = setup_real_memory();
    let p = test_proc();
    for (path, image) in userland_binaries() {
        let Some(eh) = read_image!(ElfEhdr, &image, 0) else {
            panic!("short");
        };
        let file = ExecFile::Image(&image);
        let Ok(ph) = elf_read_phdrs(p, file, eh.e_phoff, usize::from(eh.e_phnum)) else {
            panic!("phdrs");
        };

        // exec_elf_makecmds' PT_LOAD loop for a PIE at a fixed base.
        let exe_base: u64 = 0x2_0000_0000;
        let mut vmcmds = ExecVmcmdSet::new();
        let base_ph = ph.iter().position(|pp| pp.p_type == PT_LOAD);
        let (mut taddr, mut tsize) = (usize::MAX, 0usize);
        for (i, pp) in ph.iter().enumerate() {
            if pp.p_type != PT_LOAD {
                continue;
            }
            let (mut addr, flags) = if Some(i) == base_ph {
                (exe_base, VMCMD_BASE)
            } else {
                (
                    pp.p_vaddr - ph[base_ph.unwrap_or(i)].p_vaddr,
                    VMCMD_RELATIVE,
                )
            };
            let (mut size, mut prot) = (0u64, 0);
            elf_load_psection(
                &mut vmcmds,
                Some(file),
                pp,
                &mut addr,
                &mut size,
                &mut prot,
                flags,
            );
            if flags != VMCMD_BASE {
                addr += exe_base;
            }
            if prot & PROT_EXEC != 0 && prot & PROT_WRITE == 0 {
                taddr = taddr.min(addr as usize);
                tsize = (addr + size) as usize - taddr;
            }
        }
        // the first command maps the base segment at the base, the rest are relative,
        // file-backed commands are page aligned in the file
        assert_eq!(vmcmds.evs_cmds[0].ev_addr, exe_base as usize);
        assert!(vmcmds.evs_cmds[0].ev_flags & VMCMD_BASE != 0);
        for cmd in &vmcmds.evs_cmds[1..] {
            assert!(cmd.ev_flags & VMCMD_RELATIVE != 0, "{}", path.display());
        }
        for cmd in &vmcmds.evs_cmds {
            if cmd.ev_proc == VmcmdProc::MapPagedvn {
                assert_eq!(cmd.ev_offset & (PAGE_SIZE - 1), 0);
                assert_eq!(cmd.ev_len & (PAGE_SIZE - 1), 0);
            }
        }

        // the pin table, rebased to the text segment as exec_elf_makecmds does
        let Some(sys_ph) = ph.iter().find(|pp| pp.p_type == PT_OPENBSD_SYSCALLS) else {
            panic!("no PT_OPENBSD_SYSCALLS");
        };
        let exe_end = taddr + tsize;
        let mut pbase = exe_base as usize;
        let mut len = exe_end - exe_base as usize;
        let (npins, pins) = elf_read_pintable(p, file, sys_ph, false, len);
        assert!(npins > SYS_write, "{}: {npins} pins", path.display());
        let Some(table) = pins else {
            panic!("no pins");
        };
        // SAFETY: the fresh table, `npins` entries.
        let t = unsafe { pins_slice(table, npins) };
        elf_adjustpins(&mut pbase, &mut len, t, (taddr - exe_base as usize) as u32);
        assert_eq!(pbase, taddr);
        // libc's startup and exit are pinned; every pinned site lies in the text
        for sysno in [SYS_exit, SYS_mmap, SYS_getentropy] {
            assert!(
                t[sysno as usize] != 0,
                "{}: syscall {sysno} pinned",
                path.display()
            );
        }
        for &pin in t.iter() {
            if pin != 0 && pin != u32::MAX {
                assert!(
                    (pin as usize) < len,
                    "{}: pin {pin:#x} in the text",
                    path.display()
                );
            }
        }
        free_pins(npins, pins);
        vmcmds.kill();
    }
}
