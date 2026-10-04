//! Host tests for the vnode operations that need no disc: `setattr`'s refusals,
//! `pathconf`, the kqueue filters, and the merging of associated files in `readdir`
//! (`iso_shipdir`/`iso_uiodir`) into a kernel buffer.

use std::vec::Vec;

use super::*;
use crate::isofs::cd9660::iso::tests::{test_mnt, test_node};
use crate::kern::vfs_subr::vattr_null;
use crate::sys::vnode::Vattr;

/// `cd9660_setattr` of `vap` on a vnode of type `t`.
fn setattr(t: crate::sys::vnode::Vtype, vap: &mut Vattr) -> Result<(), Errno> {
    let vp: &'static Vnode = std::boxed::Box::leak(std::boxed::Box::new(Vnode::new()));
    vp.v_type.set(t);
    let p = crate::sys::proc::Proc::new();
    let mut a = VopSetattrArgs {
        a_vp: vp,
        a_vap: vap,
        a_cred: NOCRED,
        a_p: &p,
    };
    cd9660_setattr(&mut a)
}

#[test]
fn setattr_refuses_everything_but_device_sizes() {
    let mut va = Vattr::new();
    vattr_null(&mut va);
    assert_eq!(setattr(VREG, &mut va), Err(Errno::EINVAL));
    va.va_size = 0;
    assert_eq!(setattr(VDIR, &mut va), Err(Errno::EISDIR));
    assert_eq!(setattr(VREG, &mut va), Err(Errno::EROFS));
    assert_eq!(setattr(VLNK, &mut va), Err(Errno::EROFS));
    assert_eq!(setattr(VCHR, &mut va), Ok(()));
    assert_eq!(setattr(VFIFO, &mut va), Ok(()));
    vattr_null(&mut va);
    va.va_uid = 0;
    assert_eq!(setattr(VCHR, &mut va), Err(Errno::EROFS));
    vattr_null(&mut va);
    va.va_vaflags |= VA_UTIMES_CHANGE;
    assert_eq!(setattr(VCHR, &mut va), Err(Errno::EROFS));
}

#[test]
fn pathconf_depends_on_rock_ridge() {
    for (ftype, name_max) in [(ISO_FTYPE_RRIP, 255), (ISO_FTYPE_DEFAULT, 37)] {
        let (_ip, vp) = test_node(test_mnt(ftype));
        let mut v: Register = 0;
        let mut ask = |name: i32| {
            let mut a = VopPathconfArgs {
                a_vp: vp,
                a_name: name,
                a_retval: &mut v,
            };
            cd9660_pathconf(&mut a).map(|()| v)
        };
        assert_eq!(ask(_PC_NAME_MAX), Ok(name_max));
        assert_eq!(ask(_PC_LINK_MAX), Ok(1));
        assert_eq!(ask(_PC_NO_TRUNC), Ok(1));
        assert_eq!(ask(_PC_TIMESTAMP_RESOLUTION), Ok(1_000_000_000));
        assert_eq!(ask(crate::sys::unistd::_PC_PIPE_BUF), Err(Errno::EINVAL));
    }
}

#[test]
fn filters_report_writability_and_vnode_events() {
    use crate::sys::event::{NOTE_DELETE, NOTE_WRITE};
    let kn = Knote::new();
    assert!(filt_cd9660write(&kn, 0));
    assert_eq!(kn.kn_data().get(), 0);
    kn.kn_sfflags.set(NOTE_WRITE);
    assert!(!filt_cd9660vnode(&kn, i64::from(NOTE_DELETE)));
    assert!(filt_cd9660vnode(&kn, i64::from(NOTE_WRITE)));
    assert!(filt_cd9660vnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
}

#[test]
fn readdir_ships_an_associated_file_before_its_file() {
    let mut buf = [0u8; 512];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 512,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut idp = Isoreaddir {
        saveent: dirent_zeroed(),
        assocent: dirent_zeroed(),
        current: dirent_zeroed(),
        saveoff: 0,
        assocoff: 0,
        curroff: 0,
        uio: &mut uio,
        uio_off: 0,
        eofflag: 1,
    };
    for (i, name) in [&b"FOO"[..], b"=FOO", b"BAR"].into_iter().enumerate() {
        idp.curroff = 100 * (i as Off + 1);
        idp.current.d_fileno = i as u64 + 1;
        idp.current.d_name[..name.len()].copy_from_slice(name);
        idp.current.d_namlen = name.len() as u8;
        iso_shipdir(&mut idp).unwrap();
    }
    idp.current.d_namlen = 0;
    iso_shipdir(&mut idp).unwrap();
    assert_eq!((idp.uio_off, idp.eofflag), (300, 1));
    let resid = idp.uio.uio_resid;

    let mut entries: Vec<(u64, i64, Vec<u8>)> = Vec::new();
    let mut off = 0;
    while off < 512 - resid {
        let d = Dirent::from_bytes(&buf[off..]).unwrap();
        let name = buf[off + 24..off + 24 + usize::from(d.d_namlen)].to_vec();
        assert_eq!(buf[off + 24 + usize::from(d.d_namlen)], 0);
        entries.push((d.d_fileno, d.d_off, name));
        off += usize::from(d.d_reclen);
    }
    assert_eq!(
        entries,
        [
            (2, 200, b"=FOO".to_vec()),
            (1, 100, b"FOO".to_vec()),
            (3, 300, b"BAR".to_vec())
        ]
    );
}

#[test]
fn readdir_stops_when_the_buffer_is_full_and_refuses_slashes() {
    let mut buf = [0u8; 40];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 40,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut idp = Isoreaddir {
        saveent: dirent_zeroed(),
        assocent: dirent_zeroed(),
        current: dirent_zeroed(),
        saveoff: 0,
        assocoff: 0,
        curroff: 0,
        uio: &mut uio,
        uio_off: 0,
        eofflag: 1,
    };
    idp.current.d_name[..3].copy_from_slice(b"a/b");
    idp.current.d_namlen = 3;
    assert_eq!(
        iso_uiodir(&mut idp, Ent::Current, 1),
        Err(Some(Errno::EINVAL))
    );
    idp.current.d_name[..3].copy_from_slice(b"abc");
    assert_eq!(iso_uiodir(&mut idp, Ent::Current, 1), Ok(()));
    assert_eq!(iso_uiodir(&mut idp, Ent::Current, 2), Err(None));
    assert_eq!((idp.uio_off, idp.eofflag), (1, 0));
}
