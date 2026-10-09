/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `ext2fs_bswap.c`: the byte swappers of the ext2fs structures, for a machine whose byte order
//! is not that of the disk (ext2 metadata is little-endian).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_bswap.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C compiles these functions only `#if BYTE_ORDER == BIG_ENDIAN`; they are always
//!   compiled here, so that the host tests run them, and are called by `e2fs_sbload` & co.
//!   only on a big-endian target. On the little-endian amd64 and arm64 they are not reached.
//! - The functions take `&Ext2fs`, `&mut Ext2fs`, ... where the C takes two pointers, which
//!   may be the same one: the swap of a field is then in place, as it is in the C.
//!   Fields that the C does not touch are left as they are in `new` (`e2fs_sb_bswap` starts
//!   from a copy of `old`, as the C's `memcpy`).
//! - `e2fs_cg_bswap` takes `size` in bytes, as the C does, and swaps `size / sizeof(struct
//!   ext2_gd)` descriptors.
//! - `e2fs_i_bswap` copies the block pointers raw (the C does too: a short symbolic link
//!   keeps its bytes) and does not swap the fields from `e2di_version_lo` on, nor the
//!   large-inode ones other than `e2di_isize`, as the C does not.

use core::mem::size_of;

use crate::ufs::ext2fs::ext2fs::{Ext2Gd, Ext2fs, MExt2fs};
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_REV0_DINODE_SIZE, Ext2fsDinode};

/// `e2fs_sb_bswap(old, new)`: swaps the byte order of the super block `old` into `new`.
pub fn e2fs_sb_bswap(old: &Ext2fs, new: &mut Ext2fs) {
    // preserve unused fields
    *new = *old;
    new.e2fs_icount = old.e2fs_icount.swap_bytes();
    new.e2fs_bcount = old.e2fs_bcount.swap_bytes();
    new.e2fs_rbcount = old.e2fs_rbcount.swap_bytes();
    new.e2fs_fbcount = old.e2fs_fbcount.swap_bytes();
    new.e2fs_ficount = old.e2fs_ficount.swap_bytes();
    new.e2fs_first_dblock = old.e2fs_first_dblock.swap_bytes();
    new.e2fs_log_bsize = old.e2fs_log_bsize.swap_bytes();
    new.e2fs_log_fsize = old.e2fs_log_fsize.swap_bytes();
    new.e2fs_bpg = old.e2fs_bpg.swap_bytes();
    new.e2fs_fpg = old.e2fs_fpg.swap_bytes();
    new.e2fs_ipg = old.e2fs_ipg.swap_bytes();
    new.e2fs_mtime = old.e2fs_mtime.swap_bytes();
    new.e2fs_wtime = old.e2fs_wtime.swap_bytes();
    new.e2fs_mnt_count = old.e2fs_mnt_count.swap_bytes();
    new.e2fs_max_mnt_count = old.e2fs_max_mnt_count.swap_bytes();
    new.e2fs_magic = old.e2fs_magic.swap_bytes();
    new.e2fs_state = old.e2fs_state.swap_bytes();
    new.e2fs_beh = old.e2fs_beh.swap_bytes();
    new.e2fs_minrev = old.e2fs_minrev.swap_bytes();
    new.e2fs_lastfsck = old.e2fs_lastfsck.swap_bytes();
    new.e2fs_fsckintv = old.e2fs_fsckintv.swap_bytes();
    new.e2fs_creator = old.e2fs_creator.swap_bytes();
    new.e2fs_rev = old.e2fs_rev.swap_bytes();
    new.e2fs_ruid = old.e2fs_ruid.swap_bytes();
    new.e2fs_rgid = old.e2fs_rgid.swap_bytes();
    new.e2fs_first_ino = old.e2fs_first_ino.swap_bytes();
    new.e2fs_inode_size = old.e2fs_inode_size.swap_bytes();
    new.e2fs_block_group_nr = old.e2fs_block_group_nr.swap_bytes();
    new.e2fs_features_compat = old.e2fs_features_compat.swap_bytes();
    new.e2fs_features_incompat = old.e2fs_features_incompat.swap_bytes();
    new.e2fs_features_rocompat = old.e2fs_features_rocompat.swap_bytes();
    new.e2fs_algo = old.e2fs_algo.swap_bytes();

    // SOME journaling-related fields.
    new.e2fs_journal_ino = old.e2fs_journal_ino.swap_bytes();
    new.e2fs_journal_dev = old.e2fs_journal_dev.swap_bytes();
    new.e2fs_last_orphan = old.e2fs_last_orphan.swap_bytes();
    new.e2fs_gdesc_size = old.e2fs_gdesc_size.swap_bytes();
    new.e2fs_default_mount_opts = old.e2fs_default_mount_opts.swap_bytes();
    new.e2fs_first_meta_bg = old.e2fs_first_meta_bg.swap_bytes();
    new.e2fs_mkfs_time = old.e2fs_mkfs_time.swap_bytes();
}

/// `e2fs_cg_bswap(old, new, size)`: swaps the byte order of the group descriptors in the first
/// `size` bytes of `old` into `new`; the `reserved` members are not copied.
pub fn e2fs_cg_bswap(old: &[Ext2Gd], new: &mut [Ext2Gd], size: usize) {
    for (o, n) in old
        .iter()
        .zip(new.iter_mut())
        .take(size / size_of::<Ext2Gd>())
    {
        n.ext2bgd_b_bitmap = o.ext2bgd_b_bitmap.swap_bytes();
        n.ext2bgd_i_bitmap = o.ext2bgd_i_bitmap.swap_bytes();
        n.ext2bgd_i_tables = o.ext2bgd_i_tables.swap_bytes();
        n.ext2bgd_nbfree = o.ext2bgd_nbfree.swap_bytes();
        n.ext2bgd_nifree = o.ext2bgd_nifree.swap_bytes();
        n.ext2bgd_ndirs = o.ext2bgd_ndirs.swap_bytes();
    }
}

/// `e2fs_i_bswap(fs, old, new)`: swaps the byte order of the inode `old` into `new`.
pub fn e2fs_i_bswap(fs: &MExt2fs, old: &Ext2fsDinode, new: &mut Ext2fsDinode) {
    new.e2di_mode = old.e2di_mode.swap_bytes();
    new.e2di_uid_low = old.e2di_uid_low.swap_bytes();
    new.e2di_gid_low = old.e2di_gid_low.swap_bytes();
    new.e2di_uid_high = old.e2di_uid_high.swap_bytes();
    new.e2di_gid_high = old.e2di_gid_high.swap_bytes();
    new.e2di_nlink = old.e2di_nlink.swap_bytes();
    new.e2di_size = old.e2di_size.swap_bytes();
    new.e2di_atime = old.e2di_atime.swap_bytes();
    new.e2di_ctime = old.e2di_ctime.swap_bytes();
    new.e2di_mtime = old.e2di_mtime.swap_bytes();
    new.e2di_dtime = old.e2di_dtime.swap_bytes();
    new.e2di_nblock = old.e2di_nblock.swap_bytes();
    new.e2di_flags = old.e2di_flags.swap_bytes();
    new.e2di_gen = old.e2di_gen.swap_bytes();
    new.e2di_facl = old.e2di_facl.swap_bytes();
    new.e2di_size_hi = old.e2di_size_hi.swap_bytes();
    new.e2di_faddr = old.e2di_faddr.swap_bytes();
    new.e2di_nblock_hi = old.e2di_nblock_hi.swap_bytes();
    new.e2di_facl_hi = old.e2di_facl_hi.swap_bytes();
    new.e2di_blocks = old.e2di_blocks;

    if fs.dinode_size() <= EXT2_REV0_DINODE_SIZE {
        return;
    }
    new.e2di_isize = old.e2di_isize.swap_bytes();
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_block_swap_keeps_the_unused_fields_and_round_trips() {
        let mut sb = Ext2fs::new();
        sb.e2fs_icount = 0x0102_0304;
        sb.e2fs_magic = 0xef53;
        sb.e2fs_rev = 1;
        sb.e2fs_features_incompat = 0x0002;
        sb.e2fs_mkfs_time = 0xaabb_ccdd;
        sb.e2fs_uuid = [7; 16];
        sb.e2fs_sbchksum = 0x1122_3344;
        let mut sw = Ext2fs::new();
        e2fs_sb_bswap(&sb, &mut sw);
        assert_eq!(sw.e2fs_icount, 0x0403_0201);
        assert_eq!(sw.e2fs_magic, 0x53ef);
        assert_eq!(sw.e2fs_rev, 0x0100_0000);
        assert_eq!(sw.e2fs_features_incompat, 0x0200_0000);
        assert_eq!(sw.e2fs_mkfs_time, 0xddcc_bbaa);
        assert_eq!(sw.e2fs_uuid, [7; 16]);
        assert_eq!(sw.e2fs_sbchksum, 0x1122_3344, "the checksum is not swapped");
        let mut back = Ext2fs::new();
        e2fs_sb_bswap(&sw, &mut back);
        assert_eq!(back.e2fs_icount, sb.e2fs_icount);
        assert_eq!(back.e2fs_magic, sb.e2fs_magic);
        assert_eq!(back.e2fs_mkfs_time, sb.e2fs_mkfs_time);
        assert_eq!(back.e2fs_rev, sb.e2fs_rev);
    }

    #[test]
    fn group_descriptors_swap_by_whole_descriptors_only() {
        let mut old = [Ext2Gd::default(); 3];
        for (i, g) in old.iter_mut().enumerate() {
            g.ext2bgd_b_bitmap = 0x0102_0300 + i as u32;
            g.ext2bgd_i_tables = 0x1000_0000;
            g.ext2bgd_nbfree = 0x0102;
            g.ext2bgd_ndirs = 3;
            g.reserved = 9;
        }
        let mut new = [Ext2Gd::default(); 3];
        e2fs_cg_bswap(&old, &mut new, 2 * 32 + 31);
        assert_eq!(new[0].ext2bgd_b_bitmap, 0x0003_0201);
        assert_eq!(new[1].ext2bgd_b_bitmap, 0x0103_0201);
        assert_eq!(new[1].ext2bgd_i_tables, 0x10);
        assert_eq!(new[1].ext2bgd_nbfree, 0x0201);
        assert_eq!(new[1].ext2bgd_ndirs, 0x0300);
        assert_eq!(new[0].reserved, 0, "reserved is not copied");
        assert_eq!(
            new[2],
            Ext2Gd::default(),
            "a partial descriptor is not swapped"
        );
    }

    #[test]
    fn inode_swap_follows_the_inode_size() {
        let mut old = Ext2fsDinode {
            e2di_mode: 0x81a4,
            e2di_size: 0x0000_1000,
            e2di_isize: 0x0020,
            e2di_x_ctime: 5,
            ..Ext2fsDinode::default()
        };
        old.e2di_blocks[0] = 0x0102_0304;
        let fs = MExt2fs::new();
        fs.set_e2fs_rev(1);
        fs.set_e2fs_inode_size(256);
        let mut new = Ext2fsDinode::default();
        e2fs_i_bswap(&fs, &old, &mut new);
        assert_eq!(new.e2di_mode, 0xa481);
        assert_eq!(new.e2di_size, 0x0010_0000);
        assert_eq!(new.e2di_isize, 0x2000);
        assert_eq!(
            new.e2di_blocks[0], 0x0102_0304,
            "block pointers are copied raw"
        );
        assert_eq!(new.e2di_x_ctime, 0, "the extra fields are not touched");
        fs.set_e2fs_inode_size(128);
        let mut small = Ext2fsDinode::default();
        e2fs_i_bswap(&fs, &old, &mut small);
        assert_eq!(small.e2di_isize, 0);
        assert_eq!(small.e2di_mode, 0xa481);
    }
}
/* </TESTS> */
