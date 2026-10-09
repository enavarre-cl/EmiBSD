/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
/* </LICENSES> */

/* <CODE> */
//! `cargo xtask rdsetroot [-d] [-s] [-x] KERNEL [FS]`: OpenBSD's `rdsetroot(8)`
//! (`usr.sbin/rdsetroot/rdsetroot.c`, ISC) for an ELF64 little-endian kernel, the tool that puts
//! a miniroot file system into the `rd_root_image` array of a RAMDISK kernel (`bsd.rd`).
//!
//! Re-expressed, not copied: the C walks the ELF with libelf and `mmap`s the data segment;
//! this reads the headers itself and writes the image through the file. What is the same: the
//! symbols `rd_root_size` and `rd_root_image` are looked up in `.symtab`, the segment that
//! holds them is the `PT_LOAD` containing both addresses (the file offset of a symbol is
//! `p_offset + (address - p_vaddr)`; the C's `p_paddr` arithmetic reduces to the same thing),
//! the image is read from FS (or stdin) and written at `rd_root_image`; it must fit in
//! `rd_root_size` bytes (`ramdisk too small`), which the C does not change. `-s` prints
//! `rd_root_size`, `-x` extracts the image the kernel holds (to FS or stdout), `-d` prints
//! the offsets. The kernel's `rd_root_size` is `ROOTBYTES`, the `EMIBSD_MINIROOTSIZE` the
//! kernel was built with (`sys/dev/rd.rs`, cargo feature `miniroot`).

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::Result;

/// Where `rd_root_size` and `rd_root_image` are in a kernel file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Where {
    /// File offset of the 32-bit `rd_root_size`.
    pub(crate) size_off: u64,
    /// File offset of the `rd_root_image` array.
    pub(crate) image_off: u64,
    /// Bytes from `image_off` to the end of the segment's file contents.
    pub(crate) image_room: u64,
}

fn u16_at(b: &[u8], at: usize) -> Result<u64> {
    let s = b.get(at..at + 2).ok_or("truncated ELF")?;
    Ok(u64::from(u16::from_le_bytes([s[0], s[1]])))
}

fn u32_at(b: &[u8], at: usize) -> Result<u64> {
    let s = b.get(at..at + 4).ok_or("truncated ELF")?;
    Ok(u64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]])))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    let s = b.get(at..at + 8).ok_or("truncated ELF")?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

/// The NUL-terminated name at `at` in `table`.
fn name_at(table: &[u8], at: usize) -> &[u8] {
    let rest = table.get(at..).unwrap_or(&[]);
    let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    &rest[..end]
}

/// Finds the two symbols and their file offsets in the ELF `elf` (the whole file).
pub(crate) fn locate(elf: &[u8]) -> Result<Where> {
    if elf.get(..4) != Some(b"\x7fELF") {
        return Err("not an ELF file".into());
    }
    if elf.get(4) != Some(&2) || elf.get(5) != Some(&1) {
        return Err("not a 64-bit little-endian ELF file".into());
    }
    let phoff = usize::try_from(u64_at(elf, 0x20)?)?;
    let shoff = usize::try_from(u64_at(elf, 0x28)?)?;
    let phentsize = usize::try_from(u16_at(elf, 0x36)?)?;
    let phnum = usize::try_from(u16_at(elf, 0x38)?)?;
    let shentsize = usize::try_from(u16_at(elf, 0x3a)?)?;
    let shnum = usize::try_from(u16_at(elf, 0x3c)?)?;

    // The symbol table and the string table it names.
    let mut symtab = None;
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        if u32_at(elf, sh + 4)? == 2 {
            let link = usize::try_from(u32_at(elf, sh + 0x28)?)?;
            symtab = Some((
                usize::try_from(u64_at(elf, sh + 0x18)?)?,
                usize::try_from(u64_at(elf, sh + 0x20)?)?,
                link,
            ));
            break;
        }
    }
    let (sym_off, sym_size, link) = symtab.ok_or("symbol table not found")?;
    let str_sh = shoff + link * shentsize;
    let str_off = usize::try_from(u64_at(elf, str_sh + 0x18)?)?;
    let str_size = usize::try_from(u64_at(elf, str_sh + 0x20)?)?;
    let strtab = elf
        .get(str_off..str_off + str_size)
        .ok_or("string table not found")?;

    let mut size_sym = None;
    let mut image_sym = None;
    for i in 0..sym_size / 24 {
        let s = sym_off + i * 24;
        let name = name_at(strtab, usize::try_from(u32_at(elf, s)?)?);
        if name == b"rd_root_size" {
            size_sym = Some(u64_at(elf, s + 8)?);
        } else if name == b"rd_root_image" {
            image_sym = Some(u64_at(elf, s + 8)?);
        }
    }
    let (Some(size_va), Some(image_va)) = (size_sym, image_sym) else {
        return Err("no rd_root_image symbols? (the kernel needs cargo feature `miniroot`)".into());
    };

    // The data segment: the PT_LOAD that holds the image and the size.
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32_at(elf, ph)? != 1 {
            continue;
        }
        let offset = u64_at(elf, ph + 8)?;
        let vaddr = u64_at(elf, ph + 0x10)?;
        let filesz = u64_at(elf, ph + 0x20)?;
        let inside = |va: u64| va >= vaddr && va - vaddr < filesz;
        if !inside(image_va) {
            continue;
        }
        if !inside(size_va) {
            return Err("rd_root_size not in data segment".into());
        }
        return Ok(Where {
            size_off: offset + (size_va - vaddr),
            image_off: offset + (image_va - vaddr),
            image_room: filesz - (image_va - vaddr),
        });
    }
    Err("can't locate space for rd_root_image!".into())
}

/// `cargo xtask rdsetroot`.
pub(crate) fn rdsetroot(args: &[&str]) -> Result<()> {
    let (mut debug, mut sflag, mut xflag) = (false, false, false);
    let mut files = Vec::new();
    for a in args {
        match *a {
            "-d" => debug = true,
            "-s" => sflag = true,
            "-x" => xflag = true,
            f if f.starts_with('-') && f.len() > 1 => {
                return Err(format!("rdsetroot: unknown flag {f}\n{USAGE}").into());
            }
            f => files.push(f),
        }
    }
    if (sflag && (debug || xflag || files.len() > 1)) || files.is_empty() || files.len() > 2 {
        return Err(USAGE.into());
    }
    let kernel = Path::new(files[0]);
    let mut kfile = OpenOptions::new()
        .read(true)
        .write(!xflag && !sflag)
        .open(kernel)
        .map_err(|e| format!("rdsetroot: {}: {e}", kernel.display()))?;
    // The headers and symbols sit in the first and last parts of the file; the data is
    // large (the whole kernel), so read it once.
    let mut elf = Vec::new();
    kfile.read_to_end(&mut elf)?;
    let at = locate(&elf).map_err(|e| format!("rdsetroot: {}: {e}", kernel.display()))?;
    let size_bytes = elf
        .get(usize::try_from(at.size_off)?..usize::try_from(at.size_off)? + 4)
        .ok_or("rdsetroot: rd_root_size is beyond the file")?;
    let size = u64::from(u32::from_le_bytes([
        size_bytes[0],
        size_bytes[1],
        size_bytes[2],
        size_bytes[3],
    ]));
    if debug {
        eprintln!("rd_root_size_off: {:#x}", at.size_off);
        eprintln!("rd_root_image_off: {:#x}", at.image_off);
        eprintln!("rd_root_size  val: {size:#x} ({} blocks)", size >> 9);
    }
    if sflag {
        println!("{size}");
        return Ok(());
    }
    if size > at.image_room {
        return Err(format!(
            "rdsetroot: rd_root_size {size} is beyond its segment ({} bytes)",
            at.image_room
        )
        .into());
    }
    let start = usize::try_from(at.image_off)?;
    let end = start + usize::try_from(size)?;
    if xflag {
        let image = &elf[start..end];
        match files.get(1) {
            Some(f) => std::fs::write(f, image).map_err(|e| format!("rdsetroot: {f}: {e}"))?,
            None => std::io::stdout().write_all(image)?,
        }
        return Ok(());
    }
    let mut image = Vec::new();
    match files.get(1) {
        Some(f) => {
            File::open(f)
                .map_err(|e| format!("rdsetroot: {f}: {e}"))?
                .read_to_end(&mut image)?;
        }
        None => {
            std::io::stdin().read_to_end(&mut image)?;
        }
    }
    if image.len() as u64 > size {
        return Err(format!("rdsetroot: ramdisk too small {:#x} {size:#x}", image.len()).into());
    }
    kfile.seek(SeekFrom::Start(at.image_off))?;
    kfile.write_all(&image)?;
    kfile.flush()?;
    if debug {
        eprintln!("...copied {} bytes", image.len());
    }
    Ok(())
}

const USAGE: &str = "usage: cargo xtask rdsetroot [-d] KERNEL [FS]\n       \
                     cargo xtask rdsetroot -s KERNEL\n       \
                     cargo xtask rdsetroot -x KERNEL [FS]";
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny ELF64: one PT_LOAD at 0x1000 (file offset 0x100, 0x40 bytes), `.symtab` and
    /// `.strtab` with the two symbols (`rd_root_size` at 0x1000, `rd_root_image` at 0x1010).
    fn tiny_elf() -> Vec<u8> {
        let mut e = vec![0u8; 0x400];
        e[..4].copy_from_slice(b"\x7fELF");
        e[4] = 2;
        e[5] = 1;
        e[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes()); // phoff
        e[0x28..0x30].copy_from_slice(&0x200u64.to_le_bytes()); // shoff
        e[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
        e[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
        e[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        e[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
        // Program header at 0x40.
        e[0x40..0x44].copy_from_slice(&1u32.to_le_bytes());
        e[0x48..0x50].copy_from_slice(&0x100u64.to_le_bytes()); // offset
        e[0x50..0x58].copy_from_slice(&0x1000u64.to_le_bytes()); // vaddr
        e[0x58..0x60].copy_from_slice(&0x1000u64.to_le_bytes()); // paddr
        e[0x60..0x68].copy_from_slice(&0x40u64.to_le_bytes()); // filesz
        // Strings at 0x300: "\0rd_root_size\0rd_root_image\0".
        let strs = b"\0rd_root_size\0rd_root_image\0";
        e[0x300..0x300 + strs.len()].copy_from_slice(strs);
        // Symbols at 0x340: a null one and the two.
        let sym = |e: &mut Vec<u8>, i: usize, name: u32, value: u64| {
            let s = 0x340 + i * 24;
            e[s..s + 4].copy_from_slice(&name.to_le_bytes());
            e[s + 8..s + 16].copy_from_slice(&value.to_le_bytes());
        };
        sym(&mut e, 1, 1, 0x1000);
        sym(&mut e, 2, 14, 0x1010);
        // Section headers at 0x200: null, .symtab (index 1), .strtab (index 2).
        let sh = |e: &mut Vec<u8>, i: usize, ty: u32, off: u64, size: u64, link: u32| {
            let s = 0x200 + i * 64;
            e[s + 4..s + 8].copy_from_slice(&ty.to_le_bytes());
            e[s + 0x18..s + 0x20].copy_from_slice(&off.to_le_bytes());
            e[s + 0x20..s + 0x28].copy_from_slice(&size.to_le_bytes());
            e[s + 0x28..s + 0x2c].copy_from_slice(&link.to_le_bytes());
        };
        sh(&mut e, 1, 2, 0x340, 72, 2);
        sh(&mut e, 2, 3, 0x300, strs.len() as u64, 0);
        e
    }

    #[test]
    fn symbols_are_found_in_the_data_segment() {
        let at = locate(&tiny_elf()).unwrap();
        assert_eq!(
            at,
            Where {
                size_off: 0x100,
                image_off: 0x110,
                image_room: 0x30
            }
        );
    }

    #[test]
    fn a_kernel_without_the_symbols_is_refused() {
        let mut e = tiny_elf();
        e[0x301] = b'x'; // "xd_root_size"
        assert!(locate(&e).is_err());
        assert!(locate(b"not an elf").is_err());
    }

    #[test]
    fn an_image_is_written_and_read_back() {
        let dir = std::env::temp_dir().join(format!("rdsetroot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = tiny_elf();
        e[0x100..0x104].copy_from_slice(&16u32.to_le_bytes()); // rd_root_size = 16
        let k = dir.join("k");
        let fs = dir.join("fs");
        let out = dir.join("out");
        std::fs::write(&k, &e).unwrap();
        std::fs::write(&fs, b"0123456789abcdef").unwrap();
        let (ks, fss, outs) = (
            k.to_str().unwrap(),
            fs.to_str().unwrap(),
            out.to_str().unwrap(),
        );
        rdsetroot(&[ks, fss]).unwrap();
        let patched = std::fs::read(&k).unwrap();
        assert_eq!(&patched[0x110..0x120], b"0123456789abcdef");
        rdsetroot(&["-x", ks, outs]).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"0123456789abcdef");
        // Too big for rd_root_size.
        std::fs::write(&fs, b"0123456789abcdefX").unwrap();
        assert!(rdsetroot(&[ks, fss]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
/* </TESTS> */
