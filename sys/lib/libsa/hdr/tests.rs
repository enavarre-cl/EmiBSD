//! The layouts libsa declares again agree with the kernel's ports of the same headers.

use core::mem::{offset_of, size_of};

use super::dinode::{Ufs1Dinode, Ufs2Dinode};
use super::disklabel::{Disklabel, Partition};
use super::fs::Fs;

#[test]
fn layouts_match_the_kernel() {
    use bsd::sys::disklabel as k_dl;
    use bsd::ufs::ffs::fs as k_fs;
    use bsd::ufs::ufs::dinode as k_di;

    assert_eq!(size_of::<Fs>(), size_of::<k_fs::Fs>());
    assert_eq!(offset_of!(Fs, fs_magic), offset_of!(k_fs::Fs, fs_magic));
    assert_eq!(offset_of!(Fs, fs_bsize), offset_of!(k_fs::Fs, fs_bsize));
    assert_eq!(offset_of!(Fs, fs_inopb), offset_of!(k_fs::Fs, fs_inopb));
    assert_eq!(offset_of!(Fs, fs_qbmask), offset_of!(k_fs::Fs, fs_qbmask));
    assert_eq!(
        offset_of!(Fs, fs_maxsymlinklen),
        offset_of!(k_fs::Fs, fs_maxsymlinklen)
    );
    assert_eq!(size_of::<Ufs1Dinode>(), size_of::<k_di::Ufs1Dinode>());
    assert_eq!(size_of::<Ufs2Dinode>(), size_of::<k_di::Ufs2Dinode>());
    assert_eq!(
        offset_of!(Ufs1Dinode, di_db),
        offset_of!(k_di::Ufs1Dinode, di_db)
    );
    assert_eq!(
        offset_of!(Ufs2Dinode, di_db),
        offset_of!(k_di::Ufs2Dinode, di_db)
    );
    assert_eq!(size_of::<Disklabel>(), size_of::<k_dl::Disklabel>());
    assert_eq!(size_of::<Partition>(), size_of::<k_dl::Partition>());
    assert_eq!(
        offset_of!(Disklabel, d_uid),
        offset_of!(k_dl::Disklabel, d_uid)
    );
    assert_eq!(
        offset_of!(Disklabel, d_partitions),
        offset_of!(k_dl::Disklabel, d_partitions)
    );
    assert_eq!(super::disklabel::DISKMAGIC, k_dl::DISKMAGIC);
    assert_eq!(super::fs::FS_UFS2_MAGIC, k_fs::FS_UFS2_MAGIC);
}
