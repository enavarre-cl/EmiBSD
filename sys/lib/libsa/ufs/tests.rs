//! The FFS1, FFS2 and ISO 9660 readers, `cread` and the open file table, on the images
//! `testdata/gen_fixtures.py` made with OpenBSD's makefs (the same tree in each).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::cread::{close, lseek, open, read};
use crate::hdr::stat::{S_IFDIR, S_IFMT, S_IFREG, Stat};
use crate::readdir::{closedir, opendir, readdir};
use crate::saerrno::Errno;
use crate::stand::{SEEK_SET, SOPEN_MAX};
use crate::stat::stat;
use crate::testutil::{add_fixture, output, setup};

/// The whole file `path`, read through `cread`.
fn slurp(path: &str) -> Result<Vec<u8>, Errno> {
    let fd = open(path.as_bytes(), 0)?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1000];
    loop {
        let n = read(fd, &mut buf)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    close(fd)?;
    Ok(out)
}

/// The names `opendir`/`readdir` give for `path`, sorted.
fn ls(path: &str) -> Vec<String> {
    let fd = opendir(path.as_bytes()).unwrap();
    let mut names = Vec::new();
    let mut name = [0u8; 256];
    while readdir(fd, &mut name).is_ok() {
        let end = name.iter().position(|&c| c == 0).unwrap();
        names.push(String::from_utf8(name[..end].to_vec()).unwrap());
    }
    closedir(fd);
    names.sort();
    names
}

fn big() -> Vec<u8> {
    (0..10000)
        .flat_map(|i| alloc::format!("{i:06}\n").into_bytes())
        .collect()
}

fn check_ffs(dev: &str) {
    let p = |s: &str| alloc::format!("{dev}:{s}");
    assert_eq!(slurp(&p("/hello.txt")).unwrap(), b"hello from ffs\n");
    assert_eq!(slurp(&p("//dir/deep/sub.txt")).unwrap(), b"deep file\n");
    // symbolic links, relative and absolute
    assert_eq!(slurp(&p("/link")).unwrap(), b"deep file\n");
    assert_eq!(slurp(&p("/dir/abs")).unwrap(), b"hello from ffs\n");
    // 70000 bytes in 4096-byte blocks: the single indirect block
    assert_eq!(slurp(&p("/big.dat")).unwrap(), big());
    // gzip, inflated by cread
    assert_eq!(
        slurp(&p("/hello.gz")).unwrap(),
        b"compressed hello\n".repeat(50)
    );
    assert_eq!(slurp(&p("/nothere")), Err(Errno::ENOENT));
    assert_eq!(slurp(&p("/hello.txt/x")), Err(Errno::ENOTDIR));

    let mut sb = Stat::default();
    stat(p("/etc/boot.conf").as_bytes(), &mut sb).unwrap();
    assert_eq!((sb.st_mode & S_IFMT, sb.st_mode & 0o777), (S_IFREG, 0o644));
    assert_eq!((sb.st_uid, sb.st_size), (0, 27));
    stat(p("/dir").as_bytes(), &mut sb).unwrap();
    assert_eq!(sb.st_mode & S_IFMT, S_IFDIR);

    assert_eq!(
        ls(&p("/")),
        [
            ".",
            "..",
            "big.dat",
            "dir",
            "etc",
            "hello.gz",
            "hello.txt",
            "link"
        ]
    );
    assert_eq!(ls(&p("/dir")), [".", "..", "abs", "deep"]);

    // seeking back in a compressed file starts over; forwards reads ahead
    let fd = open(p("/hello.gz").as_bytes(), 0).unwrap();
    let mut b = [0u8; 9];
    assert_eq!(lseek(fd, 17 * 3 + 11, SEEK_SET), Ok(62));
    read(fd, &mut b).unwrap();
    assert_eq!(&b, b"hello\ncom");
    assert_eq!(lseek(fd, 0, SEEK_SET), Ok(0));
    read(fd, &mut b).unwrap();
    assert_eq!(&b, b"compresse");
    close(fd).unwrap();

    // every descriptor is free again
    let fds: Vec<usize> = (0..SOPEN_MAX)
        .map(|_| open(p("/hello.txt").as_bytes(), 0).unwrap())
        .collect();
    assert_eq!(open(p("/hello.txt").as_bytes(), 0), Err(Errno::EMFILE));
    for fd in fds {
        close(fd).unwrap();
    }
}

#[test]
fn ffs1() {
    let _g = setup();
    add_fixture("ffs1", "ffs1.img.z");
    check_ffs("ffs1");
}

#[test]
fn ffs2() {
    let _g = setup();
    add_fixture("ffs2", "ffs2.img.z");
    check_ffs("ffs2");
}

#[test]
fn cd9660() {
    let _g = setup();
    add_fixture("cd", "cd9660.img.z");
    assert_eq!(slurp("cd:/hello.txt").unwrap(), b"hello from ffs\n");
    assert_eq!(slurp("cd:/dir/deep/sub.txt").unwrap(), b"deep file\n");
    assert_eq!(slurp("cd:/BIG.DAT").unwrap(), big());
    assert_eq!(slurp("cd:/missing.txt"), Err(Errno::ENOENT));
}

#[test]
fn console_output() {
    let _g = setup();
    crate::cons::cninit();
    crate::printf!("ab\tc{:>4}|{:08x}\n", 42, 0x3f8);
    crate::putchar::putchar(0o177);
    // tabs expand to the next multiple of 8; a newline gets a carriage return; DEL erases
    assert_eq!(output(), "ab      c  42|000003f8\n\r\x08 \x08");
}
