/* <LICENSES> */
/*
 * Copyright (c) 1994, 1995, 1997 Charles M. Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles M. Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
 */

/*
 * Originally written by Julian Elischer (julian@tfs.com)
 * for TRW Financial Systems for use under the MACH(2.5) operating system.
 *
 * TRW Financial Systems, in accordance with their agreement with Carnegie
 * Mellon University, makes this software available to CMU to distribute
 * or use in any manner that they see fit as long as this message is kept with
 * the software. For this reason TFS also grants any other persons or
 * organisations permission to use or modify this software.
 *
 * TFS supplies this software to be publicly redistributed
 * on the understanding that TFS is not responsible for the correct
 * functioning of this software in any circumstances.
 *
 * Ported to run under 386BSD by Julian Elischer (julian@tfs.com) Sept 1992
 */
/*
 * Written by Julian Elischer (julian@tfs.com)
 * for TRW Financial Systems.
 *
 * TRW Financial Systems, in accordance with their agreement with Carnegie
 * Mellon University, makes this software available to CMU to distribute
 * or use in any manner that they see fit as long as this message is kept with
 * the software. For this reason TFS also grants any other persons or
 * organisations permission to use or modify this software.
 *
 * TFS supplies this software to be publicly redistributed
 * on the understanding that TFS is not responsible for the correct
 * functioning of this software in any circumstances.
 *
 * Ported to run under 386BSD by Julian Elischer (julian@tfs.com) Sept 1992
 */
/* </LICENSES> */

/* <CODE> */
//! cd(4): the SCSI CD-ROM driver, and `<scsi/cd.h>`, the commands and mode pages of CD and
//! DVD drives. It attaches to every CD-ROM, WORM and ATAPI optical device a `scsibus`
//! finds, reads its capacity, fabricates a disk label (`cdgetdisklabel`, with the label
//! `readdisklabel` spoofs from an ISO 9660 volume) and turns the buffers of the block and
//! character devices into READ commands on its link.
//!
//! Upstream: sys/scsi/cd.c @ 3ce1f3f79392, sys/scsi/cd.h @ 3ce1f3f79392
//!
//! `cdattach` takes the link (at most `CDOUTSTANDING` commands at once), attaches the disk
//! with `DKF_NOLABELREAD` and leaves the media alone; `cdopen` spins the drive up, locks
//! the tray, loads the capacity (`cd_get_parms`) and fabricates the label. The block
//! (`bdevsw[6]`) and character (`cdevsw[15]`) entries queue buffers on the softc's `bufq`
//! (`cdstrategy`), and `cdstart`, the link's transfer handler, makes one SCSI transfer of
//! each, completed by `cd_buf_done`. `cdioctl` handles the disk label, the audio play,
//! table of contents, sub-channel and volume requests of `<sys/cdio.h>`, tray control and
//! the DVD structure and authentication requests, and hands the rest to `scsi_do_ioctl`.
//!
//! ## Deviations
//! - `dma_alloc(9)` (`kern/dma_alloc.c`) is not ported. Small fixed-size buffers (the table
//!   of contents, the sub-channel reply, the READ CAPACITY replies, the mode sense buffer)
//!   are locals, as in `sd.rs`: every command here is synchronous (`scsi_xs_sync`), so they
//!   outlive the transfer. The DVD buffers (up to 2052 bytes) come from `malloc(9)` through
//!   `scsiconf.rs`'s `DmaBuf`, freed when dropped. The allocation failures the C tests for
//!   on the locals cannot happen.
//! - The C's `-1`-less errno returns are `Result<(), Errno>`. `cd_getvol` keeps the C's
//!   `return 0` after a failed mode sense (the C ignores `error`).
//! - The in-core label is handled as in `sd.rs`: `cdgetdisklabel` builds the label in a
//!   local (a Rust `&mut` may not alias the label `cdstrategy` reads), first publishing the
//!   initialised label when no partition is open (what the C's in-core label holds while
//!   `readdisklabel` reads into it), and `cdopen`/`DIOCRLDINFO` install the result.
//!   `DIOCGPDINFO` builds its answer in a local and copies it into the ioctl buffer.
//! - `cdsize` opens nothing: it returns -1 as the C does ("CD-ROMs are read-only").
//! - `cd_buf_done` reads the buffer from `xs->bp`, which `cdstart` sets to the same buffer
//!   as `xs->cookie` (the C reads the cookie).
//! - `struct cd_toc` is [`CdToc`], the 804 bytes the device writes with `header()`/`entry()`
//!   decoding them, because the C overlays a structure on a DMA buffer. `th->len` and
//!   `cte->addr.lba` are converted in place by the C (`betoh16`, `betoh32`; `letoh16` for
//!   `ADEV_LITTLETOC` drives); here the decoded values are locals or written back to the
//!   [`CdToc`]. The `swap16_multi` of a big-endian `ADEV_LITTLETOC` is not needed: both
//!   archs are little-endian.
//! - `cd_load_toc` clamps the length it asks for to the 804 bytes of [`CdToc`] (the C asks
//!   for 8 bytes more when the disc has 99 tracks starting at 0, overrunning its buffer);
//!   `CDIOREADTOCENTRYS` skips an entry index past the table (the C reads 8 bytes beyond
//!   it); `cd_play_tracks` fails with `EINVAL` where the C would read past the table.
//!   `CDIOREADTOCENTRYS` takes `min(len, th->len - 2)` over unsigned integers, as the C's
//!   `min(9)` does (a `th->len` below 2 copies `len` bytes).
//! - `dvd_read_bca` returns `EIO` on a bad length, as the C (which leaks the buffer there;
//!   the `DmaBuf` is freed).
//! - The ioctl arguments are read from and written back to the kernel copy as byte slices
//!   (`ioctl_arg`/`ioctl_ret`), where the C changes them in place; the DVD unions are
//!   [`DvdStruct`] and [`DvdAuthinfo`].
//! - `SCSIDEBUG` is not configured: the `SC_DEBUG` sites are comments. `cd_eject` is
//!   `__macppc__` only and not ported (no such arch).
//! - `NCD` is 1 (config's `cd.h`), as `NSD` is for sd; the C's `cdlookup` is `disk_lookup`.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use crate::kern::kern_bufq::{
    bufq_dequeue, bufq_destroy, bufq_drain, bufq_init, bufq_peek, bufq_queue,
};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::subr_autoconf::device_unref;
use crate::kern::subr_disk::{
    bounds_check_with_label, disk_attach, disk_busy, disk_closepart, disk_detach, disk_gone,
    disk_lock, disk_lock_nointr, disk_lookup, disk_openpart, disk_unbusy, disk_unlock, dkcksum,
    initdisklabel, setdisklabel,
};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::biodone;
use crate::machine::copy::copyout;
use crate::machine::disklabel::readdisklabel;
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsi_all::{
    PR_ALLOW, PR_PREVENT, SENSE_NOT_READY_BECOMING_READY, SKEY_NOT_READY, SMS_PAGE_CTRL_CHANGEABLE,
    SMS_PF, SSD_ERRCODE, SSD_ERRCODE_CURRENT, SSD_ERRCODE_DEFERRED, SSD_KEY, ScsiGeneric,
    ScsiModeSenseBuf, ScsiReadCapData, ScsiReadCapData16, ScsiWire, T_CDROM, T_DIRECT, T_FIXED,
    T_REMOV, T_WORM, asc_ascq, wire_mut,
};
use crate::scsi::scsi_base::{
    scsi_delay, scsi_do_mode_sense, scsi_interpret_sense, scsi_mode_select, scsi_mode_select_big,
    scsi_prevent, scsi_read_cap_10, scsi_read_cap_16, scsi_start, scsi_test_unit_ready,
    scsi_xs_exec, scsi_xs_get, scsi_xs_put, scsi_xs_sync, scsi_xsh_add, scsi_xsh_del, scsi_xsh_set,
};
use crate::scsi::scsi_debug::{SDEV_DB1, SDEV_DB2};
use crate::scsi::scsi_disk::{
    READ_10, READ_12, READ_COMMAND, SSS_LOEJ, SSS_START, SSS_STOP, ScsiRw, ScsiRw10, ScsiRw12,
    WRITE_10, WRITE_12, WRITE_COMMAND,
};
use crate::scsi::scsi_ioctl::scsi_do_ioctl;
use crate::scsi::scsiconf::{
    _2btol, _4btol, _8btol, _lto2b, _lto3b, _lto4b, ADEV_LITTLETOC, ADEV_NOCAPACITY, DmaBuf,
    SCSI_AUTOCONF, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_IGNORE_ILLEGAL_REQUEST,
    SCSI_IGNORE_MEDIA_CHANGE, SCSI_IGNORE_NOT_READY, SCSI_RESET, SCSI_REV_2, SCSI_REV_SPC,
    SCSI_SILENT, SDEV_ATAPI, SDEV_EJECTING, SDEV_MEDIA_LOADED, SDEV_OPEN, SDEV_UMASS,
    ScsiAttachArgs, ScsiInquiryPattern, ScsiLink, ScsiXfer, ScsiXshandler, TEST_READY_RETRIES,
    XS_BUSY, XS_NOERROR, XS_SENSE, XS_SHORTSENSE, XS_TIMEOUT, scsi_inqmatch, sid_ansii_rev,
};
use crate::sys::buf::{B_ERROR, B_READ, B_WRITE, BUFQ_DEFAULT, Buf, Bufq};
use crate::sys::cdio::{
    CD_LBA_FORMAT, CD_MSF_FORMAT, CDIOCALLOW, CDIOCCLOSE, CDIOCCLRDEBUG, CDIOCEJECT, CDIOCGETVOL,
    CDIOCLOADUNLOAD, CDIOCPAUSE, CDIOCPLAYBLOCKS, CDIOCPLAYMSF, CDIOCPLAYTRACKS, CDIOCPREVENT,
    CDIOCREADSUBCHANNEL, CDIOCRESET, CDIOCRESUME, CDIOCSETDEBUG, CDIOCSETLEFT, CDIOCSETMONO,
    CDIOCSETMUTE, CDIOCSETPATCH, CDIOCSETRIGHT, CDIOCSETSTEREO, CDIOCSETVOL, CDIOCSTART, CDIOCSTOP,
    CDIOREADMSADDR, CDIOREADTOCENTRYS, CDIOREADTOCHEADER, CdSubChannelHeader, CdSubChannelInfo,
    CdTocEntry, DVD_AUTH, DVD_AUTH_ESTABLISHED, DVD_AUTH_FAILURE, DVD_CHALLENGE_SIZE,
    DVD_HOST_SEND_CHALLENGE, DVD_HOST_SEND_KEY2, DVD_HOST_SEND_RPC_STATE, DVD_INVALIDATE_AGID,
    DVD_KEY_SIZE, DVD_LU_SEND_AGID, DVD_LU_SEND_ASF, DVD_LU_SEND_CHALLENGE, DVD_LU_SEND_KEY1,
    DVD_LU_SEND_RPC_STATE, DVD_LU_SEND_TITLE_KEY, DVD_READ_STRUCT, DVD_STRUCT_BCA,
    DVD_STRUCT_COPYRIGHT, DVD_STRUCT_DISCKEY, DVD_STRUCT_MANUFACT, DVD_STRUCT_PHYSICAL,
    DvdAuthinfo, DvdStruct, GPCMD_READ_DVD_STRUCTURE, GPCMD_REPORT_KEY, GPCMD_SEND_KEY,
    IocLoadUnload, IocPatch, IocPlayBlocks, IocPlayMsf, IocPlayTrack, IocReadSubchannel,
    IocReadTocEntry, IocTocHeader, IocVol,
};
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DISK, DVACT_DEACTIVATE, DVACT_RESUME, Device, Softc,
};
use crate::sys::disk::{DKF_NOLABELREAD, Disk};
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DTYPE_ATAPI, DTYPE_SCSI, Disklabel, Partinfo, RAW_PART,
    disklabeldev, diskpart, diskunit, dl_blktosec, dl_getpoffset, dl_setdsize,
};
use crate::sys::dkio::{
    DIOCEJECT, DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCLOCK, DIOCRLDINFO, DIOCSDINFO, DIOCWDINFO,
};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::M_WAITOK;
use crate::sys::mtio::{MTIOCTOP, MTOFFL, MTRETEN};
use crate::sys::param::howmany;
use crate::sys::proc::Proc;
use crate::sys::scsiio::{SCIOCCOMMAND, SCIOCDEBUG, SCIOCIDENTIFY, SCIOCRESET};
use crate::sys::stat::S_IFCHR;
use crate::sys::types::{Daddr, Dev};
use crate::sys::uio::Uio;

// `<scsi/cd.h>`

/// `CD_RELADDR`: two bits always in the same place in byte 2 (flag byte).
pub const CD_RELADDR: u8 = 0x01;
/// `CD_MSF`.
pub const CD_MSF: u8 = 0x02;

/// Marks the wire structures of `<scsi/cd.h>` (see [`ScsiWire`]).
macro_rules! scsi_wire {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` of `u8`s, byte arrays and structures of them only (the size
            // assertions at the end of the file pin that there is no padding).
            unsafe impl ScsiWire for $t {}
        )*
    };
}

/// `struct scsi_blank`: BLANK.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiBlank {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `BLANK_*`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `unused`.
    pub unused: [u8; 5],
    /// `control`.
    pub control: u8,
}

/// `BLANK_DISC`.
pub const BLANK_DISC: u8 = 0;
/// `BLANK_MINIMAL`.
pub const BLANK_MINIMAL: u8 = 1;

/// `struct scsi_close_track`: CLOSE TRACK.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiCloseTrack {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: `CT_IMMED`.
    pub flags: u8,
    /// `closefunc`: `CT_CLOSE_*`.
    pub closefunc: u8,
    /// `unused`.
    pub unused: u8,
    /// `track`.
    pub track: [u8; 2],
    /// `unused1`.
    pub unused1: [u8; 3],
    /// `control`.
    pub control: u8,
}

/// `CT_IMMED`.
pub const CT_IMMED: u8 = 1;
/// `CT_CLOSE_TRACK`.
pub const CT_CLOSE_TRACK: u8 = 1;
/// `CT_CLOSE_SESS`.
pub const CT_CLOSE_SESS: u8 = 2;
/// `CT_CLOSE_BORDER`.
pub const CT_CLOSE_BORDER: u8 = 3;

/// `struct scsi_pause`: PAUSE.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPause {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 6],
    /// `resume`: `PA_PAUSE` or `PA_RESUME`.
    pub resume: u8,
    /// `control`.
    pub control: u8,
}

/// `PA_PAUSE`.
pub const PA_PAUSE: u8 = 1;
/// `PA_RESUME`.
pub const PA_RESUME: u8 = 0;

/// `struct scsi_play_msf`: PLAY MSF.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPlayMsf {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: u8,
    /// `start_m`.
    pub start_m: u8,
    /// `start_s`.
    pub start_s: u8,
    /// `start_f`.
    pub start_f: u8,
    /// `end_m`.
    pub end_m: u8,
    /// `end_s`.
    pub end_s: u8,
    /// `end_f`.
    pub end_f: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_play_track`: PLAY TRACK.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPlayTrack {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `start_track`.
    pub start_track: u8,
    /// `start_index`.
    pub start_index: u8,
    /// `unused1`.
    pub unused1: u8,
    /// `end_track`.
    pub end_track: u8,
    /// `end_index`.
    pub end_index: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_play`: PLAY.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPlay {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `blk_addr`.
    pub blk_addr: [u8; 4],
    /// `unused`.
    pub unused: u8,
    /// `xfer_len`.
    pub xfer_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_play_big`: PLAY (12).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPlayBig {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: same as above.
    pub byte2: u8,
    /// `blk_addr`.
    pub blk_addr: [u8; 4],
    /// `xfer_len`.
    pub xfer_len: [u8; 4],
    /// `unused`.
    pub unused: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_play_rel_big`: PLAY TRACK RELATIVE (12).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPlayRelBig {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: same as above.
    pub byte2: u8,
    /// `blk_addr`.
    pub blk_addr: [u8; 4],
    /// `xfer_len`.
    pub xfer_len: [u8; 4],
    /// `track`.
    pub track: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_read_header`: READ HEADER.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadHeader {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `blk_addr`.
    pub blk_addr: [u8; 4],
    /// `unused`.
    pub unused: u8,
    /// `data_len`.
    pub data_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_read_subchannel`: READ SUB-CHANNEL.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadSubchannel {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `byte3`: `SRS_SUBQ`.
    pub byte3: u8,
    /// `subchan_format`.
    pub subchan_format: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `track`.
    pub track: u8,
    /// `data_len`.
    pub data_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `SRS_SUBQ`.
pub const SRS_SUBQ: u8 = 0x40;

/// `struct scsi_read_toc`: READ TOC.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadToc {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 4],
    /// `from_track`.
    pub from_track: u8,
    /// `data_len`.
    pub data_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_read_track_info`: READ TRACK INFORMATION.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadTrackInfo {
    /// `opcode`.
    pub opcode: u8,
    /// `addrtype`: `RTI_*`.
    pub addrtype: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `unused`.
    pub unused: u8,
    /// `data_len`.
    pub data_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `RTI_LBA`.
pub const RTI_LBA: u8 = 0;
/// `RTI_TRACK`.
pub const RTI_TRACK: u8 = 1;
/// `RTI_BORDER`.
pub const RTI_BORDER: u8 = 2;

/// `struct scsi_load_unload`: LOAD/UNLOAD MEDIUM.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiLoadUnload {
    /// `opcode`.
    pub opcode: u8,
    /// `reserved`: `IMMED`.
    pub reserved: u8,
    /// `reserved2`.
    pub reserved2: [u8; 2],
    /// `options`: `START`, `LOUNLO`.
    pub options: u8,
    /// `reserved4`.
    pub reserved4: [u8; 3],
    /// `slot`.
    pub slot: u8,
    /// `reserved5`.
    pub reserved5: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `IMMED`.
pub const IMMED: u8 = 0x1;
/// `START`.
pub const START: u8 = 0x1;
/// `LOUNLO`.
pub const LOUNLO: u8 = 0x2;

/// `struct scsi_set_cd_speed`: SET CD SPEED.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiSetCdSpeed {
    /// `opcode`.
    pub opcode: u8,
    /// `rotation`: `ROTATE_*`.
    pub rotation: u8,
    /// `read`.
    pub read: [u8; 2],
    /// `write`.
    pub write: [u8; 2],
    /// `reserved`.
    pub reserved: [u8; 5],
    /// `control`.
    pub control: u8,
}

/// `ROTATE_CLV`.
pub const ROTATE_CLV: u8 = 0;
/// `ROTATE_CAV`.
pub const ROTATE_CAV: u8 = 1;

// Opcodes

/// `READ_SUBCHANNEL`: cdrom read Subchannel.
pub const READ_SUBCHANNEL: u8 = 0x42;
/// `READ_TOC`: cdrom read TOC.
pub const READ_TOC: u8 = 0x43;
/// `READ_HEADER`: cdrom read header.
pub const READ_HEADER: u8 = 0x44;
/// `PLAY`: cdrom play 'play audio' mode.
pub const PLAY: u8 = 0x45;
/// `PLAY_MSF`: cdrom play Min,Sec,Frames mode.
pub const PLAY_MSF: u8 = 0x47;
/// `PLAY_TRACK`: cdrom play track/index mode.
pub const PLAY_TRACK: u8 = 0x48;
/// `PLAY_TRACK_REL`: cdrom play track/index mode.
pub const PLAY_TRACK_REL: u8 = 0x49;
/// `PAUSE`: cdrom pause in 'play audio' mode.
pub const PAUSE: u8 = 0x4b;
/// `READ_TRACK_INFO`: read track/rzone info.
pub const READ_TRACK_INFO: u8 = 0x52;
/// `CLOSE_TRACK`: close track/rzone/session/border.
pub const CLOSE_TRACK: u8 = 0x5b;
/// `BLANK`: cdrom blank.
pub const BLANK: u8 = 0xa1;
/// `PLAY_BIG`: cdrom pause in 'play audio' mode.
pub const PLAY_BIG: u8 = 0xa5;
/// `LOAD_UNLOAD`: cdrom load/unload media.
pub const LOAD_UNLOAD: u8 = 0xa6;
/// `PLAY_TRACK_REL_BIG`: cdrom play track/index mode.
pub const PLAY_TRACK_REL_BIG: u8 = 0xa9;
/// `SET_CD_SPEED`: set cdrom read/write speed.
pub const SET_CD_SPEED: u8 = 0xbb;

// Mode pages

/// `ERR_RECOVERY_PAGE`.
pub const ERR_RECOVERY_PAGE: u8 = 0x01;
/// `WRITE_PARAM_PAGE`.
pub const WRITE_PARAM_PAGE: u8 = 0x05;
/// `AUDIO_PAGE`.
pub const AUDIO_PAGE: u8 = 0x0e;
/// `CDVD_CAPABILITIES_PAGE`.
pub const CDVD_CAPABILITIES_PAGE: u8 = 0x2a;

/// `struct port_control` (the anonymous `port[4]` member of `struct cd_audio_page`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PortControl {
    /// `channels`: `CHANNEL*`.
    pub channels: u8,
    /// `volume`.
    pub volume: u8,
}

/// `struct cd_audio_page`: the audio control mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CdAudioPage {
    /// `page_code`: `CD_PAGE_CODE`, `CD_PAGE_PS`.
    pub page_code: u8,
    /// `param_len`.
    pub param_len: u8,
    /// `flags`: `CD_PA_SOTC`, `CD_PA_IMMED`.
    pub flags: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `format_lba`: `CD_PA_FORMAT_LBA`, `CD_PA_APR_VALID`.
    pub format_lba: u8,
    /// `lb_per_sec`.
    pub lb_per_sec: [u8; 2],
    /// `port`.
    pub port: [PortControl; 4],
}

/// `CD_PAGE_CODE`.
pub const CD_PAGE_CODE: u8 = 0x3F;
/// `CD_PAGE_PS`.
pub const CD_PAGE_PS: u8 = 0x80;
/// `CD_PA_SOTC`.
pub const CD_PA_SOTC: u8 = 0x02;
/// `CD_PA_IMMED`.
pub const CD_PA_IMMED: u8 = 0x04;
/// `CD_PA_FORMAT_LBA`.
pub const CD_PA_FORMAT_LBA: u8 = 0x0F;
/// `CD_PA_APR_VALID`.
pub const CD_PA_APR_VALID: u8 = 0x80;
/// `CHANNEL`.
pub const CHANNEL: u8 = 0x0F;
/// `CHANNEL_0`.
pub const CHANNEL_0: u8 = 1;
/// `CHANNEL_1`.
pub const CHANNEL_1: u8 = 2;
/// `CHANNEL_2`.
pub const CHANNEL_2: u8 = 4;
/// `CHANNEL_3`.
pub const CHANNEL_3: u8 = 8;
/// `LEFT_CHANNEL`.
pub const LEFT_CHANNEL: u8 = CHANNEL_0;
/// `RIGHT_CHANNEL`.
pub const RIGHT_CHANNEL: u8 = CHANNEL_1;
/// `MUTE_CHANNEL`.
pub const MUTE_CHANNEL: u8 = 0x0;
/// `BOTH_CHANNEL`: `LEFT_CHANNEL | RIGHT_CHANNEL`.
pub const BOTH_CHANNEL: u8 = LEFT_CHANNEL | RIGHT_CHANNEL;
/// `LEFT_PORT`.
pub const LEFT_PORT: usize = 0;
/// `RIGHT_PORT`.
pub const RIGHT_PORT: usize = 1;

// There are 2352 bytes in a CD digital audio frame. One frame is 1/75 of a second, at 44.1kHz
// sample rate, 16 bits/sample, 2 channels.
//
// The frame data have the two channels interleaved, with the left channel first. Samples
// are little endian 16-bit signed values.

/// `CD_DA_BLKSIZ`: # bytes in CD-DA frame.
pub const CD_DA_BLKSIZ: u32 = 2352;
/// `CD_NORMAL_DENSITY_CODE`: from Toshiba CD-ROM specs.
pub const CD_NORMAL_DENSITY_CODE: u8 = 0x00;
/// `CD_DA_DENSITY_CODE`: from Toshiba CD-ROM specs.
pub const CD_DA_DENSITY_CODE: u8 = 0x82;

/// `struct scsi_read_dvd_structure`: READ DVD STRUCTURE.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadDvdStructure {
    /// `opcode`: `GPCMD_READ_DVD_STRUCTURE`.
    pub opcode: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `address`.
    pub address: [u8; 4],
    /// `layer`.
    pub layer: u8,
    /// `format`.
    pub format: u8,
    /// `length`.
    pub length: [u8; 2],
    /// `agid`: bottom 6 bits reserved.
    pub agid: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_read_dvd_structure_data`: the reply of READ DVD STRUCTURE.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiReadDvdStructureData {
    /// `len`: big-endian length of valid data.
    pub len: [u8; 2],
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `data`.
    pub data: [u8; 2048],
}

scsi_wire!(
    ScsiBlank,
    ScsiCloseTrack,
    ScsiPause,
    ScsiPlayMsf,
    ScsiPlayTrack,
    ScsiPlay,
    ScsiPlayBig,
    ScsiPlayRelBig,
    ScsiReadHeader,
    ScsiReadSubchannel,
    ScsiReadToc,
    ScsiReadTrackInfo,
    ScsiLoadUnload,
    ScsiSetCdSpeed,
    PortControl,
    CdAudioPage,
    ScsiReadDvdStructure,
    ScsiReadDvdStructureData,
);

// `cd.c`

/// `CDOUTSTANDING`: the most commands cd keeps on a link.
pub const CDOUTSTANDING: u16 = 4;

/// `MAXTRACK`.
pub const MAXTRACK: usize = 99;
/// `CD_FRAMES`.
pub const CD_FRAMES: u8 = 75;
/// `CD_SECS`.
pub const CD_SECS: u8 = 60;

/// `NCD`: config(8)'s count for `cd* at scsibus?` (`cd.h`; `needs-flag`).
pub const NCD: i32 = 1;

/// The size of the table of contents `struct cd_toc`: the header and `MAXTRACK + 1` entries
/// (one extra for the lead-out).
pub const CD_TOC_SIZE: usize = size_of::<IocTocHeader>() + (MAXTRACK + 1) * size_of::<CdTocEntry>();

/// `struct cd_toc`: the table of contents as the device writes it, header first.
#[repr(C)]
pub struct CdToc {
    /// The `struct ioc_toc_header` and the `entries[MAXTRACK + 1]` as bytes.
    pub bytes: [u8; CD_TOC_SIZE],
}

impl CdToc {
    /// A zeroed table (`PR_ZERO`).
    pub const fn new() -> Self {
        Self {
            bytes: [0; CD_TOC_SIZE],
        }
    }

    /// `toc->header`, as the device wrote it (`len` still big-endian in the integer).
    pub fn header(&self) -> IocTocHeader {
        ioctl_arg::<IocTocHeader>(&self.bytes)
    }

    /// `toc->header = th`.
    pub fn set_header(&mut self, th: &IocTocHeader) {
        ioctl_ret(&mut self.bytes, th);
    }

    /// `toc->entries[i]`; `None` past the table.
    pub fn entry(&self, i: usize) -> Option<CdTocEntry> {
        if i > MAXTRACK {
            return None;
        }
        let off = size_of::<IocTocHeader>() + i * size_of::<CdTocEntry>();
        Some(ioctl_arg::<CdTocEntry>(&self.bytes[off..]))
    }

    /// `toc->entries[i] = e`; nothing past the table.
    pub fn set_entry(&mut self, i: usize, e: &CdTocEntry) {
        if i > MAXTRACK {
            return;
        }
        let off = size_of::<IocTocHeader>() + i * size_of::<CdTocEntry>();
        ioctl_ret(&mut self.bytes[off..], e);
    }

    /// `toc->entries`, as bytes.
    pub fn entries(&self) -> &[u8] {
        &self.bytes[size_of::<IocTocHeader>()..]
    }
}

impl Default for CdToc {
    fn default() -> Self {
        Self::new()
    }
}

/// `CDF_DYING`: dying, when deactivated.
pub const CDF_DYING: i32 = 0x40;

/// `struct cd_parms`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdParms {
    /// `secsize`.
    pub secsize: u32,
    /// `disksize`: total number sectors.
    pub disksize: u64,
}

/// `struct cd_softc`.
#[repr(C)]
pub struct CdSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,
    /// `sc_flags`: `CDF_*`.
    pub sc_flags: Cell<i32>,
    /// `sc_link`: contains our targ, lun, etc.; set by `cdattach`.
    pub sc_link: Cell<Option<&'static ScsiLink>>,
    /// `params`.
    pub params: Cell<CdParms>,
    /// `sc_bufq`: the buffers `cdstrategy` queued for `cdstart`.
    pub sc_bufq: Bufq,
    /// `sc_xsh`: runs `cdstart` when the link has an opening.
    pub sc_xsh: ScsiXshandler,
}

impl CdSoftc {
    /// `sc->sc_link`: the link `cdattach` stored.
    ///
    /// Panics before `cdattach`, where the C would dereference NULL.
    pub fn link(&self) -> &'static ScsiLink {
        match self.sc_link.get() {
            Some(link) => link,
            None => panic(format_args!("{}: no scsi_link", self.sc_dev.xname())),
        }
    }

    /// `ISSET(sc->sc_flags, f)`.
    pub fn isset(&self, f: i32) -> bool {
        self.sc_flags.get() & f != 0
    }

    /// `SET(sc->sc_flags, f)`.
    pub fn set(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() | f);
    }
}

// SAFETY: `#[repr(C)]` with the device first; the disk, the buffer queue and the transfer
// handler are all-zero valid (`sys/disk.rs`; `sys/buf.rs`: integers, null pointers, `None`s
// and a free mutex; `scsiconf.rs`), and the other members are `Cell`s of an integer, an
// `Option` of a reference or a structure of integers.
unsafe impl Softc for CdSoftc {}

/// `cd_ca`.
pub static CD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<CdSoftc>(),
    ca_match: Some(cdmatch),
    ca_attach: cdattach,
    ca_detach: Some(cddetach),
    ca_activate: Some(cdactivate),
};

/// `cd_cd`.
pub static CD_CD: Cfdriver = Cfdriver::new(b"cd", DV_DISK, CD_COCOVM);

/// `cd_patterns`: the device types cd drives. The `#if 0` Pioneer entry is not ported (dead
/// code in the C as well).
pub static CD_PATTERNS: [ScsiInquiryPattern; 5] = [
    pattern(T_CDROM, T_REMOV, b""),
    pattern(T_CDROM, T_FIXED, b""),
    pattern(T_WORM, T_REMOV, b""),
    pattern(T_WORM, T_FIXED, b""),
    pattern(T_DIRECT, T_REMOV, b"NEC                 CD-ROM DRIVE:260"),
];

/// `{type, removable, vendor, "", ""}`: an entry of [`CD_PATTERNS`].
const fn pattern(r#type: u8, removable: i32, vendor: &'static [u8]) -> ScsiInquiryPattern {
    ScsiInquiryPattern {
        r#type,
        removable,
        vendor,
        product: b"",
        revision: b"",
    }
}

/// `cdlookup(unit)`: the attached unit, referenced (`disk_lookup`).
fn cdlookup(unit: u32) -> Option<NonNull<Device>> {
    disk_lookup(&CD_CD, i32::try_from(unit).ok()?)
}

/// `(struct cd_softc *)dv`.
fn cd_softc(dv: NonNull<Device>) -> &'static CdSoftc {
    // SAFETY: every `cd` device is a `CdSoftc` (`CD_CA.ca_devsize`) that autoconf allocated
    // and frees only after `cddetach`, once the references `cdlookup` takes are dropped; the
    // transfer paths run while the device is attached.
    unsafe { dv.as_ref().softc::<CdSoftc>() }
}

/// `link->device_softc`: the softc of the cd that `cdattach` made the link's driver.
fn cd_link_softc(link: &ScsiLink) -> &'static CdSoftc {
    match link.device_softc.get() {
        Some(dv) => cd_softc(dv),
        None => panic(format_args!("cd: scsi_link {:p} has no softc", link)),
    }
}

/// `ISSET(link->flags, f)`.
fn link_isset(link: &ScsiLink, f: u16) -> bool {
    link.flags.get() & f != 0
}

/// `SET(link->flags, f)`.
fn link_set(link: &ScsiLink, f: u16) {
    link.flags.set(link.flags.get() | f);
}

/// `CLR(link->flags, f)`.
fn link_clr(link: &ScsiLink, f: u16) {
    link.flags.set(link.flags.get() & !f);
}

/// `cdmatch`: the priority of the best [`CD_PATTERNS`] match for the device.
pub fn cdmatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: scsibus attaches its children with a `struct scsi_attach_args` as `aux`.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
    let inq = sa.sa_sc_link.inqdata.get();

    let (_, priority) = scsi_inqmatch(&inq, &CD_PATTERNS);

    priority
}

/// `cdattach`: the routine called by the low level scsi routine when it discovers a device
/// suitable for this driver.
pub fn cdattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = cd_softc(NonNull::from(self_));
    // SAFETY: scsibus attaches its children with a `struct scsi_attach_args` as `aux`.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
    let link = sa.sa_sc_link;

    // SC_DEBUG(link, SDEV_DB2, ("cdattach:\n")): SCSIDEBUG is not configured.

    // Store information needed to contact our base driver.
    sc.sc_link.set(Some(link));
    link.interpret_sense.set(cd_interpret_sense);
    link.device_softc.set(Some(NonNull::from(self_)));
    if link.openings.get() > CDOUTSTANDING {
        link.openings.set(CDOUTSTANDING);
    }

    // Initialize disk structures.
    let mut name = [0u8; 16];
    let xname = sc.sc_dev.xname().as_bytes();
    let n = xname.len().min(name.len());
    name[..n].copy_from_slice(&xname[..n]);
    sc.sc_dk.dk_name.set(name);
    let _ = bufq_init(&sc.sc_bufq, BUFQ_DEFAULT);

    printf(format_args!("\n"));

    scsi_xsh_set(&sc.sc_xsh, link, cdstart);

    // Attach disk.
    sc.sc_dk.dk_flags.set(DKF_NOLABELREAD);
    disk_attach(Some(&sc.sc_dev), &sc.sc_dk);
}

/// `cdactivate`.
pub fn cdactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = cd_softc(NonNull::from(self_));

    match act {
        DVACT_RESUME => {
            // When resuming, hardware may have forgotten we locked it. So if there are any
            // open partitions, lock the CD.
            if sc.sc_dk.dk_openmask.get() != 0 {
                let _ = scsi_prevent(
                    sc.link(),
                    i32::from(PR_PREVENT),
                    SCSI_IGNORE_ILLEGAL_REQUEST
                        | SCSI_IGNORE_MEDIA_CHANGE
                        | SCSI_SILENT
                        | SCSI_AUTOCONF,
                );
            }
        }
        DVACT_DEACTIVATE => {
            sc.set(CDF_DYING);
            scsi_xsh_del(&sc.sc_xsh);
        }
        _ => {}
    }
    Ok(())
}

/// `cddetach`.
pub fn cddetach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = cd_softc(NonNull::from(self_));

    bufq_drain(&sc.sc_bufq);

    disk_gone(cdopen, self_.dv_unit.get() as u32);

    // Detach disk.
    bufq_destroy(&sc.sc_bufq);
    disk_detach(&sc.sc_dk);

    Ok(())
}

/// `cdopen`: opens the device. Make sure the partition info is as up-to-date as can be.
pub fn cdopen(dev: Dev, flag: i32, fmt: i32, _p: &Proc) -> Result<(), Errno> {
    cdopen_noproc(dev, flag, fmt)
}

/// [`cdopen`] without the thread, which it never reads.
pub fn cdopen_noproc(dev: Dev, _flag: i32, fmt: i32) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let part = diskpart(dev);

    let rawopen = part == RAW_PART && fmt == S_IFCHR as i32;

    let dv = cdlookup(unit).ok_or(ENXIO)?;
    let sc = cd_softc(dv);
    let unref = || {
        // SAFETY: the reference `cdlookup` took.
        unsafe { device_unref(dv) }
    };
    if sc.isset(CDF_DYING) {
        unref();
        return Err(ENXIO);
    }
    let link = sc.link();

    // SC_DEBUG(link, SDEV_DB1, ("cdopen: dev=0x%x (unit %d (of %d), partition %d)\n", ...)):
    // SCSIDEBUG is not configured.

    if let Err(e) = disk_lock(&sc.sc_dk) {
        unref();
        return Err(e);
    }

    // `Ok(true)`: go to `out`; `Ok(false)`: go to `bad` with `error`.
    let mut error = Ok(());
    let out = if sc.sc_dk.dk_openmask.get() != 0 {
        // If any partition is open, but the disk has been invalidated, disallow further
        // opens.
        if !link_isset(link, SDEV_MEDIA_LOADED) {
            if rawopen {
                true
            } else {
                error = Err(EIO);
                false
            }
        } else {
            true
        }
    } else {
        // Check that it is still responding and ok. Drive can be in progress of loading
        // media so use increased retries number and don't ignore NOT_READY.

        // Use cd_interpret_sense() now.
        link_set(link, SDEV_OPEN);

        error = scsi_test_unit_ready(
            link,
            TEST_READY_RETRIES,
            (if rawopen { SCSI_SILENT } else { 0 })
                | SCSI_IGNORE_ILLEGAL_REQUEST
                | SCSI_IGNORE_MEDIA_CHANGE,
        );

        // Start the cd spinning if necessary.
        if error == Err(EIO) {
            error = scsi_start(
                link,
                i32::from(SSS_START),
                SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE | SCSI_SILENT,
            );
        }

        if error.is_err() {
            if rawopen {
                error = Ok(());
                true
            } else {
                false
            }
        } else {
            // Lock the cd in.
            error = scsi_prevent(
                link,
                i32::from(PR_PREVENT),
                SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE | SCSI_SILENT,
            );
            if error.is_err() {
                false
            } else {
                // Load the physical device parameters.
                link_set(link, SDEV_MEDIA_LOADED);
                if cd_get_parms(
                    sc,
                    (if rawopen { SCSI_SILENT } else { 0 })
                        | SCSI_IGNORE_ILLEGAL_REQUEST
                        | SCSI_IGNORE_MEDIA_CHANGE,
                )
                .is_err()
                {
                    link_clr(link, SDEV_MEDIA_LOADED);
                    error = Err(ENXIO);
                    false
                } else {
                    // SC_DEBUG(link, SDEV_DB3, ("Params loaded\n")).

                    // Fabricate a disk label.
                    let mut lp = Disklabel::zeroed();
                    let _ = cdgetdisklabel(dev, sc, &mut lp, false);
                    // SAFETY: under the disk lock, with no partition open: nobody else
                    // holds the in-core label.
                    if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                        *dl = lp;
                    }
                    // SC_DEBUG(link, SDEV_DB3, ("Disklabel fabricated\n")).
                    true
                }
            }
        }
    };

    if out {
        // out:
        error = disk_openpart(&sc.sc_dk, part, fmt, true);
        if error.is_ok() {
            link_set(link, SDEV_OPEN);
            // SC_DEBUG(link, SDEV_DB3, ("open complete\n")).
        }

        // It's OK to fall through because dk_openmask is now non-zero.
    }

    // bad:
    if sc.sc_dk.dk_openmask.get() == 0 {
        let _ = scsi_prevent(
            link,
            i32::from(PR_ALLOW),
            SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE | SCSI_SILENT,
        );
        link_clr(link, SDEV_OPEN | SDEV_MEDIA_LOADED);
    }

    disk_unlock(&sc.sc_dk);
    unref();
    error
}

/// `cdclose`: closes the device. Only called if we are the last occurrence of an open
/// device.
pub fn cdclose(dev: Dev, _flag: i32, fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let part = diskpart(dev);

    let dv = cdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let sc = cd_softc(dv);
    let unref = || {
        // SAFETY: the reference `cdlookup` took.
        unsafe { device_unref(dv) }
    };
    if sc.isset(CDF_DYING) {
        unref();
        return Err(ENXIO);
    }
    let link = sc.link();

    disk_lock_nointr(&sc.sc_dk);

    disk_closepart(&sc.sc_dk, part, fmt);

    if sc.sc_dk.dk_openmask.get() == 0 {
        // XXXX Must wait for I/O to complete!

        let _ = scsi_prevent(
            link,
            i32::from(PR_ALLOW),
            SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_NOT_READY | SCSI_SILENT,
        );
        link_clr(link, SDEV_OPEN | SDEV_MEDIA_LOADED);

        if link_isset(link, SDEV_EJECTING) {
            let _ = scsi_start(link, i32::from(SSS_STOP | SSS_LOEJ), 0);

            link_clr(link, SDEV_EJECTING);
        }

        scsi_xsh_del(&sc.sc_xsh);
    }

    disk_unlock(&sc.sc_dk);

    unref();
    Ok(())
}

/// The zeroed label the I/O paths read before `disk_attach` allocated one.
static ZERO_LABEL: Disklabel = Disklabel::zeroed();

/// `cdstrategy`: actually translates the requested transfer into one the physical driver
/// can understand. The transfer is described by a buf and will include only one physical
/// transfer.
pub fn cdstrategy(bp: &'static Buf) {
    let dv = cdlookup(diskunit(bp.b_dev.get()));

    'done: {
        let error = 'bad: {
            let Some(dv) = dv else {
                break 'bad ENXIO;
            };
            let sc = cd_softc(dv);
            if sc.isset(CDF_DYING) {
                break 'bad ENXIO;
            }

            // SC_DEBUG(sc->sc_link, SDEV_DB2, ("cdstrategy: %ld bytes @ blk %lld\n", ...)).

            // If the device has been made invalid, error out; maybe the media changed, or
            // no media loaded.
            if !link_isset(sc.link(), SDEV_MEDIA_LOADED) {
                break 'bad EIO;
            }

            // Validate the request.
            let ok = sc
                .sc_dk
                .with_label(|lp| bounds_check_with_label(bp, lp.unwrap_or(&ZERO_LABEL)));
            if !ok {
                break 'done;
            }

            // Place it in the queue of disk activities for this disk.
            bufq_queue(&sc.sc_bufq, bp);

            // Tell the device to get going on the transfer if it's not doing anything,
            // otherwise just wait for completion.
            scsi_xsh_add(&sc.sc_xsh);

            // SAFETY: the reference `cdlookup` took.
            unsafe { device_unref(dv) };
            return;
        };

        // bad:
        bp.b_error.set(Some(error));
        bp.set(B_ERROR);
        bp.b_resid
            .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
    }

    // done:
    let s = splbio();
    biodone(bp);
    splx(s);
    if let Some(dv) = dv {
        // SAFETY: the reference `cdlookup` took.
        unsafe { device_unref(dv) };
    }
}

/// `cd_cmd_rw6`: READ (6) or WRITE (6) of `nsecs` sectors at `secno` into `generic`;
/// returns the command's length.
pub fn cd_cmd_rw6(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_COMMAND } else { WRITE_COMMAND };
    _lto3b(secno as u32, &mut cmd.addr);
    cmd.length = (nsecs & 0xff) as u8;

    size_of::<ScsiRw>() as i32
}

/// `cd_cmd_rw10`: READ (10) or WRITE (10).
pub fn cd_cmd_rw10(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw10 = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_10 } else { WRITE_10 };
    _lto4b(secno as u32, &mut cmd.addr);
    _lto2b(nsecs, &mut cmd.length);

    size_of::<ScsiRw10>() as i32
}

/// `cd_cmd_rw12`: READ (12) or WRITE (12).
pub fn cd_cmd_rw12(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw12 = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_12 } else { WRITE_12 };
    _lto4b(secno as u32, &mut cmd.addr);
    _lto4b(nsecs, &mut cmd.length);

    size_of::<ScsiRw12>() as i32
}

/// `cdstart`: looks to see if there is a buf waiting for the device and that the device is
/// not already busy. If both are true, it dequeues the buf and creates a scsi command to
/// perform the transfer in the buf. The transfer request will call `scsi_done` on
/// completion, which will in turn call this routine again so that the next queued transfer
/// is performed. The bufs are queued by the strategy routine (`cdstrategy`).
///
/// This routine is also called after other non-queued requests have been made of the scsi
/// driver, to ensure that the queue continues to be drained.
///
/// Must be called at the correct (highish) spl level: `cdstart()` is called at splbio from
/// `cdstrategy` and `scsi_done`.
pub fn cdstart(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = cd_link_softc(link);

    // SC_DEBUG(link, SDEV_DB2, ("cdstart\n")).

    if sc.isset(CDF_DYING) {
        scsi_xs_put(xs);
        return;
    }

    if !link_isset(link, SDEV_MEDIA_LOADED) {
        bufq_drain(&sc.sc_bufq);
        scsi_xs_put(xs);
        return;
    }

    let Some(bp) = bufq_dequeue(&sc.sc_bufq) else {
        scsi_xs_put(xs);
        return;
    };
    let read = bp.isset(B_READ);

    xs.flags
        .set(xs.flags.get() | if read { SCSI_DATA_IN } else { SCSI_DATA_OUT });
    xs.timeout.set(30000);
    // SAFETY: `b_data` maps `b_bcount` bytes of the busy buffer, which nobody else touches
    // until `cd_buf_done` hands it back with `biodone` after the transfer completed.
    unsafe { xs.set_data(bp.b_data.get(), bp.b_bcount.get() as i32) };
    xs.done.set(Some(cd_buf_done));
    xs.cookie.set(ptr::from_ref(bp).cast_mut().cast());
    xs.bp.set(Some(bp));

    let (secno, nsecs) = sc.sc_dk.with_label(|lp| {
        let lp = lp.unwrap_or(&ZERO_LABEL);
        let p = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
        let secno = dl_getpoffset(p) + dl_blktosec(lp, bp.b_blkno.get() as u64);
        let nsecs = howmany(
            usize::try_from(bp.b_bcount.get()).unwrap_or(0),
            lp.d_secsize as usize,
        ) as u32;
        (secno, nsecs)
    });

    let cmdlen = xs.with_cmd(|cmd: &mut ScsiGeneric| {
        if !link_isset(link, SDEV_ATAPI | SDEV_UMASS)
            && sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_2
            && (secno & 0x1f_ffff) == secno
            && (nsecs & 0xff) == nsecs
        {
            cd_cmd_rw6(cmd, read, secno, nsecs)
        } else if (secno & 0xffff_ffff) == secno && (nsecs & 0xffff) == nsecs {
            cd_cmd_rw10(cmd, read, secno, nsecs)
        } else {
            cd_cmd_rw12(cmd, read, secno, nsecs)
        }
    });
    xs.cmdlen.set(cmdlen);

    disk_busy(&sc.sc_dk);
    scsi_xs_exec(xs);

    // Move onto the next io.
    if bufq_peek(&sc.sc_bufq) {
        scsi_xsh_add(&sc.sc_xsh);
    }
}

/// `cd_buf_done`: the completion of a `cdstart` transfer: retries it, or finishes its
/// buffer.
pub fn cd_buf_done(xs: &'static ScsiXfer) {
    let sc = cd_link_softc(xs.link());
    let Some(bp) = xs.bp.get() else {
        panic(format_args!("cd_buf_done: xs {:p} has no buf", xs));
    };

    let ok = |bp: &Buf| {
        bp.b_error.set(None);
        bp.clr(B_ERROR);
        bp.b_resid.set(xs.resid.get());
    };

    // `None`: done; `Some(true)`: go to `retry`; `Some(false)`: the default case.
    let retry = match xs.error.get() {
        XS_NOERROR => {
            ok(bp);
            None
        }

        XS_SENSE | XS_SHORTSENSE => {
            // SC_DEBUG_SENSE(xs): SCSIDEBUG is not configured.
            match cd_interpret_sense(xs) {
                Ok(()) => {
                    ok(bp);
                    None
                }
                Err(error) => {
                    if error != ERESTART {
                        xs.retries.set(0);
                    }
                    Some(true)
                }
            }
        }

        XS_BUSY => {
            if xs.retries.get() != 0 && scsi_delay(xs, 1) != Err(ERESTART) {
                xs.retries.set(0);
            }
            Some(true)
        }

        XS_TIMEOUT => Some(true),

        _ => Some(false),
    };

    if let Some(retry) = retry {
        if retry {
            // retry:
            let retries = xs.retries.get();
            xs.retries.set(retries - 1);
            if retries != 0 {
                scsi_xs_exec(xs);
                return;
            }
        }
        // FALLTHROUGH, default:
        bp.b_error.set(Some(EIO));
        bp.set(B_ERROR);
        bp.b_resid
            .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
    }

    disk_unbusy(
        &sc.sc_dk,
        bp.b_bcount.get() - xs.resid.get() as i64,
        bp.b_blkno.get(),
        bp.isset(B_READ),
    );

    let s = splbio();
    biodone(bp);
    splx(s);
    scsi_xs_put(xs);
}

/// `cdminphys`: trims a transfer to what the device and its adapter can do.
pub fn cdminphys(bp: &Buf) {
    let Some(dv) = cdlookup(diskunit(bp.b_dev.get())) else {
        return;
    };
    let sc = cd_softc(dv);
    let link = sc.link();

    // If the device is ancient, we want to make sure that the transfer fits into a 6-byte
    // cdb.
    //
    // XXX Note that the SCSI-I spec says that 256-block transfers are allowed in a 6-byte
    // read/write, and are specified by setting the "length" to 0. However, we're
    // conservative here, allowing only 255-block transfers in case an ancient device gets
    // confused by length == 0. A length of 0 in a 10-byte read/write actually means 0
    // blocks.
    if !link_isset(link, SDEV_ATAPI | SDEV_UMASS) && sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_2
    {
        let secsize = sc.sc_dk.with_label(|lp| lp.map_or(0, |lp| lp.d_secsize));
        let max = i64::from(secsize) * 0xff;

        if bp.b_bcount.get() > max {
            bp.b_bcount.set(max);
        }
    }

    match link.bus().adapter().dev_minphys {
        Some(dev_minphys) => dev_minphys(bp, link),
        None => minphys(bp),
    }

    // SAFETY: the reference `cdlookup` took.
    unsafe { device_unref(dv) };
}

/// `cdread`: the raw device's read, straight into the user's buffer (physio(9)).
pub fn cdread(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(cdstrategy, dev, B_READ, cdminphys, uio)
}

/// `cdwrite`: the raw device's write, straight from the user's buffer (physio(9)).
pub fn cdwrite(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(cdstrategy, dev, B_WRITE, cdminphys, uio)
}

/// `cdioctl`: performs special action on behalf of the user. Knows about the internals of
/// this device.
pub fn cdioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, _p: &Proc) -> Result<(), Errno> {
    let part = diskpart(dev);

    let dv = cdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let sc = cd_softc(dv);
    let result = if sc.isset(CDF_DYING) {
        Err(ENXIO)
    } else {
        cdioctl_locked(sc, dev, part, cmd, addr, flag)
    };

    // exit:
    // SAFETY: the reference `cdlookup` took.
    unsafe { device_unref(dv) };
    result
}

/// `case CDIOCCLOSE:` (also `MTIOCTOP`'s `MTRETEN`, the C's `close_tray`).
fn cd_close_tray(link: &'static ScsiLink) -> Result<(), Errno> {
    scsi_start(
        link,
        i32::from(SSS_START | SSS_LOEJ),
        SCSI_IGNORE_NOT_READY | SCSI_IGNORE_MEDIA_CHANGE,
    )
}

/// The body of [`cdioctl`], between the lookup and the `exit` label.
fn cdioctl_locked(
    sc: &'static CdSoftc,
    dev: Dev,
    part: u32,
    cmd: u64,
    addr: &mut [u8],
    flag: i32,
) -> Result<(), Errno> {
    let link = sc.link();

    // SC_DEBUG(sc->sc_link, SDEV_DB2, ("cdioctl 0x%lx\n", cmd)).

    // If the device is not valid.. abandon ship.
    if !link_isset(link, SDEV_MEDIA_LOADED) {
        let raw_ok = matches!(
            cmd,
            DIOCLOCK
                | DIOCEJECT
                | SCIOCIDENTIFY
                | SCIOCCOMMAND
                | SCIOCDEBUG
                | CDIOCLOADUNLOAD
                | SCIOCRESET
                | CDIOCGETVOL
                | CDIOCSETVOL
                | CDIOCSETMONO
                | CDIOCSETSTEREO
                | CDIOCSETMUTE
                | CDIOCSETLEFT
                | CDIOCSETRIGHT
                | CDIOCCLOSE
                | CDIOCEJECT
                | CDIOCALLOW
                | CDIOCPREVENT
                | CDIOCSETDEBUG
                | CDIOCCLRDEBUG
                | CDIOCRESET
                | DVD_AUTH
                | DVD_READ_STRUCT
                | MTIOCTOP
        ) && part == RAW_PART;
        if !raw_ok {
            return Err(if !link_isset(link, SDEV_OPEN) {
                ENODEV
            } else {
                EIO
            });
        }
    }

    match cmd {
        DIOCRLDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = cdgetdisklabel(dev, sc, &mut lp, false);
            // SAFETY: the driver's own label; no other reference to it is live.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = lp;
            }
            Ok(())
        }

        DIOCGPDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = cdgetdisklabel(dev, sc, &mut lp, true);
            copyout_label(&lp, addr);
            Ok(())
        }

        DIOCGDINFO => {
            if let Some(lp) = sc.sc_dk.label() {
                copyout_label(&lp, addr);
            }
            Ok(())
        }

        DIOCGPART => {
            if let Some(lp) = sc.sc_dk.dk_label.get() {
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[part as usize] },
                };
                pi.store(addr);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }

            disk_lock(&sc.sc_dk)?;

            let mut nlp = Disklabel::from_bytes(addr);
            // SAFETY: under the disk lock; the borrow ends with the call.
            let error = match unsafe { sc.sc_dk.label_mut() } {
                Some(olp) => setdisklabel(olp, &mut nlp, sc.sc_dk.dk_openmask.get()),
                None => Err(ENXIO),
            };
            // The C's `if (error == 0) { }`: cd(4) never writes the label back.

            disk_unlock(&sc.sc_dk);
            error
        }

        CDIOCPLAYTRACKS => {
            let args = ioctl_arg::<IocPlayTrack>(addr);

            cd_set_pa_immed(sc, 0)?;
            cd_play_tracks(
                sc,
                i32::from(args.start_track),
                i32::from(args.start_index),
                i32::from(args.end_track),
                i32::from(args.end_index),
            )
        }

        CDIOCPLAYMSF => {
            let args = ioctl_arg::<IocPlayMsf>(addr);

            cd_set_pa_immed(sc, 0)?;
            cd_play_msf(
                sc,
                args.start_m,
                args.start_s,
                args.start_f,
                args.end_m,
                args.end_s,
                args.end_f,
            )
        }

        CDIOCPLAYBLOCKS => {
            let args = ioctl_arg::<IocPlayBlocks>(addr);

            cd_set_pa_immed(sc, 0)?;
            cd_play(sc, args.blk, args.len)
        }

        CDIOCREADSUBCHANNEL => {
            let args = ioctl_arg::<IocReadSubchannel>(addr);
            let len = args.data_len;

            if len > size_of::<CdSubChannelInfo>() as i32
                || len < size_of::<CdSubChannelHeader>() as i32
            {
                return Err(EINVAL);
            }
            let mut data = CdSubChannelInfo::zeroed();
            cd_read_subchannel(
                sc,
                args.address_format,
                args.data_format,
                args.track,
                data.as_bytes_mut(),
                len as usize,
            )?;
            let len = (len as u32)
                .min(_2btol(&data.header.data_len) + size_of::<CdSubChannelHeader>() as u32)
                as usize;
            copyout(&data.as_bytes()[..len], args.data)
        }

        CDIOREADTOCHEADER => {
            let mut th = [0u8; size_of::<IocTocHeader>()];

            cd_read_toc(sc, 0, 0, &mut th, size_of::<IocTocHeader>(), 0)?;
            let mut hdr = ioctl_arg::<IocTocHeader>(&th);
            hdr.len = toc_len(link, hdr.len);
            if hdr.len > 0 {
                ioctl_ret(addr, &hdr);
                Ok(())
            } else {
                Err(EIO)
            }
        }

        CDIOREADTOCENTRYS => {
            let te = ioctl_arg::<IocReadTocEntry>(addr);
            let mut toc = CdToc::new();
            let mut len = usize::from(te.data_len);

            if len > toc.entries().len() || len < size_of::<CdTocEntry>() {
                return Err(EINVAL);
            }
            cd_read_toc(
                sc,
                i32::from(te.address_format),
                i32::from(te.starting_track),
                &mut toc.bytes,
                len + size_of::<IocTocHeader>(),
                0,
            )?;
            let mut th = toc.header();
            if te.address_format == CD_LBA_FORMAT {
                let mut ntracks = i32::from(th.ending_track) - i32::from(th.starting_track) + 1;
                while ntracks >= 0 {
                    // An index past the table is skipped (the C reads beyond it).
                    if let Some(mut cte) = toc.entry(ntracks as usize) {
                        cte.set_addr_type(CD_LBA_FORMAT);
                        if !link_quirk(link, ADEV_LITTLETOC) {
                            let lba = cte.addr.lba();
                            cte.addr.set_lba(u32::from_be(lba));
                        }
                        toc.set_entry(ntracks as usize, &cte);
                    }
                    ntracks -= 1;
                }
            }
            th.len = toc_len(link, th.len);
            // `min(len, th->len - 2)`: unsigned, as min(9).
            let l = (len as u32).min(i32::from(th.len).wrapping_sub(2) as u32);
            len = l as usize;

            copyout(&toc.entries()[..len.min(toc.entries().len())], te.data)
        }

        CDIOREADMSADDR => {
            let sessno = ioctl_arg::<i32>(addr);
            let mut toc = CdToc::new();

            if sessno != 0 {
                return Err(EINVAL);
            }

            // 0x40: control word for "get MS info".
            cd_read_toc(
                sc,
                0,
                0,
                &mut toc.bytes,
                size_of::<IocTocHeader>() + size_of::<CdTocEntry>(),
                0x40,
            )?;

            let mut cte = toc.entry(0).unwrap_or_default();
            if !link_quirk(link, ADEV_LITTLETOC) {
                let lba = cte.addr.lba();
                cte.addr.set_lba(u32::from_be(lba));
            }
            let th_len = toc_len(link, toc.header().len);

            let msaddr = if th_len >= 10 && cte.track > 1 {
                cte.addr.lba() as i32
            } else {
                0
            };
            ioctl_ret(addr, &msaddr);
            Ok(())
        }

        CDIOCSETPATCH => {
            let arg = ioctl_arg::<IocPatch>(addr);

            cd_setchan(
                sc,
                arg.patch[0],
                arg.patch[1],
                arg.patch[2],
                arg.patch[3],
                0,
            )
        }

        CDIOCGETVOL => {
            let mut arg = ioctl_arg::<IocVol>(addr);

            let error = cd_getvol(sc, &mut arg, 0);
            ioctl_ret(addr, &arg);
            error
        }

        CDIOCSETVOL => {
            let arg = ioctl_arg::<IocVol>(addr);

            cd_setvol(sc, &arg, 0)
        }

        CDIOCSETMONO => cd_setchan(
            sc,
            BOTH_CHANNEL,
            BOTH_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0,
        ),

        CDIOCSETSTEREO => cd_setchan(
            sc,
            LEFT_CHANNEL,
            RIGHT_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0,
        ),

        CDIOCSETMUTE => cd_setchan(
            sc,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0,
        ),

        CDIOCSETLEFT => cd_setchan(
            sc,
            LEFT_CHANNEL,
            LEFT_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0,
        ),

        CDIOCSETRIGHT => cd_setchan(
            sc,
            RIGHT_CHANNEL,
            RIGHT_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0,
        ),

        CDIOCRESUME => cd_pause(sc, 1),

        CDIOCPAUSE => cd_pause(sc, 0),

        CDIOCSTART => scsi_start(link, i32::from(SSS_START), 0),

        CDIOCSTOP => scsi_start(link, i32::from(SSS_STOP), 0),

        CDIOCCLOSE => cd_close_tray(link),

        MTIOCTOP | CDIOCEJECT | DIOCEJECT => {
            if cmd == MTIOCTOP {
                let op = mtop_op(addr);
                if op == MTRETEN {
                    return cd_close_tray(link);
                }
                if op != MTOFFL {
                    return Err(EIO);
                }
                // FALLTHROUGH
            }
            // case CDIOCEJECT: FALLTHROUGH, case DIOCEJECT:
            link_set(link, SDEV_EJECTING);
            Ok(())
        }

        CDIOCALLOW => scsi_prevent(link, i32::from(PR_ALLOW), 0),

        CDIOCPREVENT => scsi_prevent(link, i32::from(PR_PREVENT), 0),

        DIOCLOCK => {
            let r#type = if ioctl_arg::<i32>(addr) != 0 {
                PR_PREVENT
            } else {
                PR_ALLOW
            };
            scsi_prevent(link, i32::from(r#type), 0)
        }

        CDIOCSETDEBUG => {
            link_set(link, SDEV_DB1 | SDEV_DB2);
            Ok(())
        }

        CDIOCCLRDEBUG => {
            link_clr(link, SDEV_DB1 | SDEV_DB2);
            Ok(())
        }

        CDIOCRESET | SCIOCRESET => cd_reset(sc),

        CDIOCLOADUNLOAD => {
            let args = ioctl_arg::<IocLoadUnload>(addr);

            cd_load_unload(sc, i32::from(args.options), i32::from(args.slot))
        }

        DVD_AUTH => {
            let mut a = ioctl_arg::<DvdAuthinfo>(addr);

            let error = dvd_auth(sc, &mut a);
            ioctl_ret(addr, &a);
            error
        }

        DVD_READ_STRUCT => {
            let mut s = ioctl_arg::<DvdStruct>(addr);

            let error = dvd_read_struct(sc, &mut s);
            ioctl_ret(addr, &s);
            error
        }

        _ => {
            if part != RAW_PART {
                return Err(ENOTTY);
            }
            scsi_do_ioctl(link, cmd, addr, flag)
        }
    }
}

/// `ISSET(link->quirks, q)`.
fn link_quirk(link: &ScsiLink, q: u16) -> bool {
    link.quirks.get() & q != 0
}

/// `th->len = letoh16(th->len)` for `ADEV_LITTLETOC` drives, `betoh16(th->len)` for the
/// others: the length as the device wrote it (an integer read in the CPU's order) in CPU
/// order.
fn toc_len(link: &ScsiLink, len: u16) -> u16 {
    if link_quirk(link, ADEV_LITTLETOC) {
        u16::from_le(len)
    } else {
        u16::from_be(len)
    }
}

/// `((struct mtop *)addr)->mt_op`.
fn mtop_op(addr: &[u8]) -> i16 {
    let mut b = [0u8; 2];
    let n = addr.len().min(b.len());
    b[..n].copy_from_slice(&addr[..n]);
    i16::from_ne_bytes(b)
}

/// `*(struct disklabel *)addr = *lp`: the label into an `ioctl` buffer.
fn copyout_label(lp: &Disklabel, addr: &mut [u8]) {
    let n = addr.len().min(DISKLABEL_SIZE);
    addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
}

/// `strncpy(dst, src, sizeof(dst))`: `src`, then NULs to the end of `dst`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `cdgetdisklabel`: loads the label information on the named device. Actually fabricates a
/// disklabel.
///
/// EVENTUALLY take information about different data tracks from the TOC and put it in the
/// disklabel.
pub fn cdgetdisklabel(
    dev: Dev,
    sc: &CdSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    let dp = sc.params.get();

    *lp = Disklabel::zeroed();

    lp.d_secsize = dp.secsize;
    lp.d_ntracks = 1;
    lp.d_nsectors = 100;
    lp.d_secpercyl = 100;
    lp.d_ncylinders = ((dp.disksize / 100) + 1) as u32;

    if link_isset(sc.link(), SDEV_ATAPI) {
        strncpy(&mut lp.d_typename, b"ATAPI CD-ROM");
        lp.d_type = DTYPE_ATAPI;
    } else {
        strncpy(&mut lp.d_typename, b"SCSI CD-ROM");
        lp.d_type = DTYPE_SCSI;
    }

    strncpy(&mut lp.d_packname, b"fictitious");
    dl_setdsize(lp, dp.disksize);
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);

    let mut toc = CdToc::new();
    let mut audioonly = true;
    if cd_load_toc(sc, &mut toc, CD_LBA_FORMAT).is_err() {
        audioonly = false; // No valid TOC found == not an audio CD.
    } else {
        let th = toc.header();
        let n = i32::from(th.ending_track) - i32::from(th.starting_track) + 1;
        for tocidx in 0..n.max(0) as usize {
            if toc.entry(tocidx).is_some_and(|e| e.control() & 4 != 0) {
                audioonly = false; // Found a non-audio track.
                break;
            }
        }
    }

    if audioonly {
        return Ok(());
    }

    // The label cdstrategy checks the reads below against (see the module's deviations).
    if sc.sc_dk.dk_openmask.get() == 0 {
        let mut incore = *lp;
        if initdisklabel(&mut incore).is_ok() {
            // SAFETY: no partition is open and the caller serialises label changes (the
            // disk lock in `cdopen`): nobody else holds the in-core label.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = incore;
            }
        }
    }

    readdisklabel(disklabeldev(dev), cdstrategy, lp, spoofonly)
}

/// Senses the audio mode page `pg_code` (`AUDIO_PAGE`, possibly with a page control):
/// the mode sense buffer, the page's offset in it and whether the reply is the big format.
/// `EIO` when the device has no such page (the C's `error == 0 && audio == NULL`).
fn cd_sense_audio(
    sc: &CdSoftc,
    pg_code: i32,
    flags: i32,
) -> Result<(ScsiModeSenseBuf, usize, bool), Errno> {
    let mut data = ScsiModeSenseBuf::new();

    let (audio, big) = scsi_do_mode_sense(
        sc.link(),
        pg_code,
        &mut data,
        size_of::<CdAudioPage>() as i32,
        flags,
    )?;
    let audio = audio.ok_or(EIO)?;
    Ok((data, audio, big))
}

/// `scsi_mode_select(_big)(link, SMS_PF, &data->hdr(_big), flags, 20000)`.
fn cd_mode_select(
    sc: &CdSoftc,
    data: &mut ScsiModeSenseBuf,
    big: bool,
    flags: i32,
) -> Result<(), Errno> {
    if big {
        scsi_mode_select_big(sc.link(), i32::from(SMS_PF), &mut data.buf, flags, 20000)
    } else {
        scsi_mode_select(sc.link(), i32::from(SMS_PF), &mut data.buf, flags, 20000)
    }
}

/// `cd_setchan`: routes the audio channels of the four ports.
pub fn cd_setchan(sc: &CdSoftc, p0: u8, p1: u8, p2: u8, p3: u8, flags: i32) -> Result<(), Errno> {
    let (mut data, off, big) = cd_sense_audio(sc, i32::from(AUDIO_PAGE), flags)?;

    let audio: &mut CdAudioPage = wire_mut(&mut data.buf[off..]);
    audio.port[LEFT_PORT].channels = p0;
    audio.port[RIGHT_PORT].channels = p1;
    audio.port[2].channels = p2;
    audio.port[3].channels = p3;
    cd_mode_select(sc, &mut data, big, flags)
}

/// `cd_getvol`: the volume of the four ports. As the C, it returns success even when the
/// mode sense failed (and leaves `arg` alone).
pub fn cd_getvol(sc: &CdSoftc, arg: &mut IocVol, flags: i32) -> Result<(), Errno> {
    if let Ok((data, off, _big)) = cd_sense_audio(sc, i32::from(AUDIO_PAGE), flags) {
        let audio = *crate::scsi::scsi_all::wire_ref::<CdAudioPage>(&data.buf[off..]);
        arg.vol[0] = audio.port[0].volume;
        arg.vol[1] = audio.port[1].volume;
        arg.vol[2] = audio.port[2].volume;
        arg.vol[3] = audio.port[3].volume;
    }

    Ok(())
}

/// `cd_setvol`: sets the volume of the four ports, as far as the device lets it change.
pub fn cd_setvol(sc: &CdSoftc, arg: &IocVol, flags: i32) -> Result<(), Errno> {
    let (data, off, _big) =
        cd_sense_audio(sc, i32::from(AUDIO_PAGE | SMS_PAGE_CTRL_CHANGEABLE), flags)?;

    let audio = *crate::scsi::scsi_all::wire_ref::<CdAudioPage>(&data.buf[off..]);
    let mask_volume = [
        audio.port[0].volume,
        audio.port[1].volume,
        audio.port[2].volume,
        audio.port[3].volume,
    ];

    let (mut data, off, big) = cd_sense_audio(sc, i32::from(AUDIO_PAGE), flags)?;

    let audio: &mut CdAudioPage = wire_mut(&mut data.buf[off..]);
    audio.port[0].volume = arg.vol[0] & mask_volume[0];
    audio.port[1].volume = arg.vol[1] & mask_volume[1];
    audio.port[2].volume = arg.vol[2] & mask_volume[2];
    audio.port[3].volume = arg.vol[3] & mask_volume[3];

    cd_mode_select(sc, &mut data, big, flags)
}

/// `cd_load_unload`: LOAD/UNLOAD MEDIUM.
pub fn cd_load_unload(sc: &CdSoftc, options: i32, slot: i32) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), 0).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiLoadUnload>() as i32);
    xs.timeout.set(200000);

    xs.with_cmd(|cmd: &mut ScsiLoadUnload| {
        cmd.opcode = LOAD_UNLOAD;
        cmd.options = options as u8; // ioctl uses ATAPI values
        cmd.slot = slot as u8;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_set_pa_immed`: makes the audio page's play commands return at once (`CD_PA_IMMED`,
/// not `CD_PA_SOTC`).
pub fn cd_set_pa_immed(sc: &CdSoftc, flags: i32) -> Result<(), Errno> {
    if link_isset(sc.link(), SDEV_ATAPI) {
        // XXX Noop?
        return Ok(());
    }

    let (mut data, off, big) = cd_sense_audio(sc, i32::from(AUDIO_PAGE), flags)?;

    let audio: &mut CdAudioPage = wire_mut(&mut data.buf[off..]);
    let oflags = audio.flags;
    audio.flags &= !CD_PA_SOTC;
    audio.flags |= CD_PA_IMMED;
    if audio.flags != oflags {
        return cd_mode_select(sc, &mut data, big, flags);
    }
    Ok(())
}

/// `cd_play`: gets the scsi driver to send a "start playing" command.
pub fn cd_play(sc: &CdSoftc, secno: i32, nsecs: i32) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), 0).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiPlay>() as i32);
    xs.timeout.set(200000);

    xs.with_cmd(|cmd: &mut ScsiPlay| {
        cmd.opcode = PLAY;
        _lto4b(secno as u32, &mut cmd.blk_addr);
        _lto2b(nsecs as u32, &mut cmd.xfer_len);
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// The MSF address one frame before `(m, s, f)`: where a track ends, one frame before the
/// next begins. `EINVAL` before 00:00:00.
pub fn cd_msf_prev(m: u8, s: u8, f: u8) -> Result<(u8, u8, u8), Errno> {
    let (mut endm, mut ends, mut endf) = (m, s, f);

    let borrow_f = endf == 0;
    endf = endf.wrapping_sub(1);
    if borrow_f {
        endf = CD_FRAMES - 1;
        let borrow_s = ends == 0;
        ends = ends.wrapping_sub(1);
        if borrow_s {
            ends = CD_SECS - 1;
            let borrow_m = endm == 0;
            endm = endm.wrapping_sub(1);
            if borrow_m {
                return Err(EINVAL);
            }
        }
    }
    Ok((endm, ends, endf))
}

/// `cd_play_tracks`: gets the scsi driver to send a "start playing" command for tracks.
pub fn cd_play_tracks(
    sc: &CdSoftc,
    mut strack: i32,
    _sindex: i32,
    mut etrack: i32,
    _eindex: i32,
) -> Result<(), Errno> {
    if etrack == 0 {
        return Err(EIO);
    }
    if strack > etrack {
        return Err(EINVAL);
    }

    let mut toc = CdToc::new();

    cd_load_toc(sc, &mut toc, CD_MSF_FORMAT)?;
    let th = toc.header();

    etrack += 1;
    if etrack > i32::from(th.ending_track) + 1 {
        etrack = i32::from(th.ending_track) + 1;
    }

    strack -= i32::from(th.starting_track);
    etrack -= i32::from(th.starting_track);
    if strack < 0 {
        return Err(EINVAL);
    }

    // The track ends one frame before the next begins. The last track is taken care of by
    // the leadoff track. An index past the table is `EINVAL` (the C reads beyond it).
    let end = toc.entry(etrack as usize).ok_or(EINVAL)?;
    let start = toc.entry(strack as usize).ok_or(EINVAL)?;
    let (endm, ends, endf) = cd_msf_prev(end.addr.minute(), end.addr.second(), end.addr.frame())?;

    cd_play_msf(
        sc,
        start.addr.minute(),
        start.addr.second(),
        start.addr.frame(),
        endm,
        ends,
        endf,
    )
}

/// `cd_play_msf`: gets the scsi driver to send a "play msf" command.
pub fn cd_play_msf(
    sc: &CdSoftc,
    startm: u8,
    starts: u8,
    startf: u8,
    endm: u8,
    ends: u8,
    endf: u8,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), 0).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiPlayMsf>() as i32);
    xs.timeout.set(20000);

    xs.with_cmd(|cmd: &mut ScsiPlayMsf| {
        cmd.opcode = PLAY_MSF;
        cmd.start_m = startm;
        cmd.start_s = starts;
        cmd.start_f = startf;
        cmd.end_m = endm;
        cmd.end_s = ends;
        cmd.end_f = endf;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_pause`: gets the scsi driver to send a "start up" command.
pub fn cd_pause(sc: &CdSoftc, go: i32) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), 0).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiPause>() as i32);
    xs.timeout.set(2000);

    xs.with_cmd(|cmd: &mut ScsiPause| {
        cmd.opcode = PAUSE;
        cmd.resume = go as u8;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_reset`: gets the scsi driver to send a "RESET" command.
pub fn cd_reset(sc: &CdSoftc) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), SCSI_RESET).ok_or(ENOMEM)?;

    xs.timeout.set(2000);

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_read_subchannel`: reads the sub-channel into `data` (`len` bytes of it).
pub fn cd_read_subchannel(
    sc: &CdSoftc,
    mode: u8,
    format: u8,
    track: u8,
    data: &mut [u8],
    len: usize,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), SCSI_DATA_IN | SCSI_SILENT).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiReadSubchannel>() as i32);
    let len = len.min(data.len());
    // SAFETY: `data` stays borrowed, hence valid for `len` bytes and untouched by anything
    // else, until `scsi_xs_sync` has returned below.
    unsafe { xs.set_data(data.as_mut_ptr(), len as i32) };
    xs.timeout.set(5000);

    xs.with_cmd(|cmd: &mut ScsiReadSubchannel| {
        cmd.opcode = READ_SUBCHANNEL;
        if mode == CD_MSF_FORMAT {
            cmd.byte2 |= CD_MSF;
        }
        cmd.byte3 = SRS_SUBQ;
        cmd.subchan_format = format;
        cmd.track = track;
        _lto2b(len as u32, &mut cmd.data_len);
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_read_toc`: reads the table of contents into `data` (`len` bytes of it, zeroed first).
pub fn cd_read_toc(
    sc: &CdSoftc,
    mode: i32,
    start: i32,
    data: &mut [u8],
    len: usize,
    control: u8,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), SCSI_DATA_IN | SCSI_IGNORE_ILLEGAL_REQUEST).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiReadToc>() as i32);
    let len = len.min(data.len());
    // SAFETY: `data` stays borrowed, hence valid for `len` bytes and untouched by anything
    // else, until `scsi_xs_sync` has returned below.
    unsafe { xs.set_data(data.as_mut_ptr(), len as i32) };
    xs.timeout.set(5000);

    data[..len].fill(0);

    xs.with_cmd(|cmd: &mut ScsiReadToc| {
        cmd.opcode = READ_TOC;

        if mode == i32::from(CD_MSF_FORMAT) {
            cmd.byte2 |= CD_MSF;
        }
        cmd.from_track = start as u8;
        _lto2b(len as u32, &mut cmd.data_len);
        cmd.control = control;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `cd_load_toc`: reads the header, then the whole table of contents in format `fmt`.
pub fn cd_load_toc(sc: &CdSoftc, toc: &mut CdToc, fmt: u8) -> Result<(), Errno> {
    cd_read_toc(sc, 0, 0, &mut toc.bytes, size_of::<IocTocHeader>(), 0)?;

    let th = toc.header();
    if th.ending_track < th.starting_track {
        return Err(EIO);
    }
    // +2 to account for leading out track.
    let n = usize::from(th.ending_track - th.starting_track) + 2;
    let len = (n * size_of::<CdTocEntry>() + size_of::<IocTocHeader>()).min(CD_TOC_SIZE);
    cd_read_toc(sc, i32::from(fmt), 0, &mut toc.bytes, len, 0)
}

/// `cd_get_parms`: gets the scsi driver to send a full inquiry to the device and uses the
/// results to fill out the disk parameter structure.
pub fn cd_get_parms(sc: &CdSoftc, flags: i32) -> Result<(), Errno> {
    // Reasonable defaults for drives that don't support READ_CAPACITY.
    let mut dp = CdParms {
        secsize: 2048,
        disksize: 400000,
    };
    sc.params.set(dp);

    if link_quirk(sc.link(), ADEV_NOCAPACITY) {
        return Ok(());
    }

    let mut secsize = dp.secsize;
    dp.disksize = cd_size(sc.link(), flags, Some(&mut secsize));
    dp.secsize = secsize;

    if dp.secsize < 512 || (dp.secsize & 511) != 0 {
        dp.secsize = 2048; // some drives lie !
    }

    if dp.disksize < 100 {
        dp.disksize = 400000;
    }

    sc.params.set(dp);
    Ok(())
}

/// `cdsize`: CD-ROMs are read-only, so there is no swap: -1.
pub fn cdsize(_dev: Dev) -> Daddr {
    -1
}

/// `cddump`: not implemented.
pub fn cddump(_dev: Dev, _blkno: Daddr, _va: *mut u8, _size: usize) -> Result<(), Errno> {
    Err(ENXIO)
}

/// `DVD_AUTH_BUFSIZE`.
pub const DVD_AUTH_BUFSIZE: usize = 20;

/// `dvd_auth`: a step of the DVD authentication (`DVD_AUTH`).
pub fn dvd_auth(sc: &CdSoftc, a: &mut DvdAuthinfo) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_AUTH_BUFSIZE, M_WAITOK).ok_or(ENOMEM)?;
    let buf_ptr = buf.bytes().as_mut_ptr();

    let xs = scsi_xs_get(sc.link(), 0).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiGeneric>() as i32);
    xs.timeout.set(30000);
    // SAFETY: `buf` is a `DVD_AUTH_BUFSIZE` allocation, dropped only after the transfer
    // (`scsi_xs_sync`, then `scsi_xs_put`) finished below; the length is set per command, 0
    // until then.
    unsafe { xs.set_data(buf_ptr, 0) };

    // Each case sets the command, runs it, puts `xs`, and reads the reply from `buf`.
    let run = |xs: &'static ScsiXfer, len: i32, flags: i32| -> Result<(), Errno> {
        // SAFETY: as above; `len` is at most `DVD_AUTH_BUFSIZE`.
        unsafe { xs.set_data(buf_ptr, len) };
        xs.flags.set(xs.flags.get() | flags);
        let error = scsi_xs_sync(xs);
        scsi_xs_put(xs);
        error
    };

    match a.r#type() {
        DVD_LU_SEND_AGID => {
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[8] = 8;
                cmd.bytes[9] = 0; // 0 | (0 << 6)
            });
            let error = run(xs, 8, SCSI_DATA_IN);

            if error.is_ok() {
                a.lsa().agid = buf.bytes()[7] >> 6;
            }
            error
        }

        DVD_LU_SEND_CHALLENGE => {
            let agid = a.lsc().agid;
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[8] = 16;
                cmd.bytes[9] = 1 | (agid << 6);
            });
            let error = run(xs, 16, SCSI_DATA_IN);
            if error.is_ok() {
                a.lsc()
                    .chal
                    .copy_from_slice(&buf.bytes()[4..4 + DVD_CHALLENGE_SIZE]);
            }
            error
        }

        DVD_LU_SEND_KEY1 => {
            let agid = a.lsk().agid;
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[8] = 12;
                cmd.bytes[9] = 2 | (agid << 6);
            });
            let error = run(xs, 12, SCSI_DATA_IN);

            if error.is_ok() {
                a.lsk()
                    .key
                    .copy_from_slice(&buf.bytes()[4..4 + DVD_KEY_SIZE]);
            }
            error
        }

        DVD_LU_SEND_TITLE_KEY => {
            let (lba, agid) = {
                let lstk = a.lstk();
                (lstk.lba, lstk.agid)
            };
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                _lto4b(lba as u32, &mut cmd.bytes[1..5]);
                cmd.bytes[8] = 12;
                cmd.bytes[9] = 4 | (agid << 6);
            });
            let error = run(xs, 12, SCSI_DATA_IN);

            if error.is_ok() {
                let b = buf.bytes();
                let lstk = a.lstk();
                lstk.cpm = (b[4] >> 7) & 1;
                lstk.cp_sec = (b[4] >> 6) & 1;
                lstk.cgms = (b[4] >> 4) & 3;
                lstk.title_key.copy_from_slice(&b[5..5 + DVD_KEY_SIZE]);
            }
            error
        }

        DVD_LU_SEND_ASF => {
            let agid = a.lsasf().agid;
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[8] = 8;
                cmd.bytes[9] = 5 | (agid << 6);
            });
            let error = run(xs, 8, SCSI_DATA_IN);

            if error.is_ok() {
                a.lsasf().asf = buf.bytes()[7] & 1;
            }
            error
        }

        DVD_HOST_SEND_CHALLENGE => {
            let (agid, chal) = {
                let hsc = a.hsc();
                (hsc.agid, hsc.chal)
            };
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_SEND_KEY;
                cmd.bytes[8] = 16;
                cmd.bytes[9] = 1 | (agid << 6);
            });
            {
                let b = buf.bytes();
                b[1] = 14;
                b[4..4 + DVD_CHALLENGE_SIZE].copy_from_slice(&chal);
            }
            let error = run(xs, 16, SCSI_DATA_OUT);

            if error.is_ok() {
                a.set_type(DVD_LU_SEND_KEY1);
            }
            error
        }

        DVD_HOST_SEND_KEY2 => {
            let (agid, key) = {
                let hsk = a.hsk();
                (hsk.agid, hsk.key)
            };
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_SEND_KEY;
                cmd.bytes[8] = 12;
                cmd.bytes[9] = 3 | (agid << 6);
            });
            {
                let b = buf.bytes();
                b[1] = 10;
                b[4..4 + DVD_KEY_SIZE].copy_from_slice(&key);
            }
            let error = run(xs, 12, SCSI_DATA_OUT);

            if error.is_ok() {
                a.set_type(DVD_AUTH_ESTABLISHED);
            } else {
                a.set_type(DVD_AUTH_FAILURE);
            }
            error
        }

        DVD_INVALIDATE_AGID => {
            let agid = a.lsa().agid;
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[9] = 0x3f | (agid << 6);
            });
            xs.clear_data();

            let error = scsi_xs_sync(xs);
            scsi_xs_put(xs);
            error
        }

        DVD_LU_SEND_RPC_STATE => {
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_REPORT_KEY;
                cmd.bytes[8] = 8;
                cmd.bytes[9] = 8; // 8 | (0 << 6)
            });
            let error = run(xs, 8, SCSI_DATA_IN);

            if error.is_ok() {
                let b = buf.bytes();
                let lrpcs = a.lrpcs();
                lrpcs.r#type = (b[4] >> 6) & 3;
                lrpcs.vra = (b[4] >> 3) & 7;
                lrpcs.ucca = b[4] & 7;
                lrpcs.region_mask = b[5];
                lrpcs.rpc_scheme = b[6];
            }
            error
        }

        DVD_HOST_SEND_RPC_STATE => {
            let pdrc = a.hrpcs().pdrc;
            xs.with_cmd(|cmd: &mut ScsiGeneric| {
                cmd.opcode = GPCMD_SEND_KEY;
                cmd.bytes[8] = 8;
                cmd.bytes[9] = 6; // 6 | (0 << 6)
            });
            {
                let b = buf.bytes();
                b[1] = 6;
                b[4] = pdrc;
            }
            run(xs, 8, SCSI_DATA_OUT)
        }

        _ => {
            scsi_xs_put(xs);
            Err(ENOTTY)
        }
    }
}

/// `DVD_READ_PHYSICAL_BUFSIZE`.
pub const DVD_READ_PHYSICAL_BUFSIZE: usize = 4 + 4 * 20;

/// Runs a READ DVD STRUCTURE (a generic CDB, `cmd->bytes[6]` the structure type, `[7]` the
/// length, `[5]` the layer) with `buf` as its `len` bytes of data.
fn dvd_read_generic(
    sc: &CdSoftc,
    buf: &mut DmaBuf,
    len: usize,
    r#type: u8,
    layer: Option<u8>,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), SCSI_DATA_IN).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiGeneric>() as i32);
    let b = buf.bytes();
    // SAFETY: `buf` is borrowed until `scsi_xs_sync` has returned below and holds at least
    // `len` bytes (the callers size it).
    unsafe { xs.set_data(b.as_mut_ptr(), len as i32) };
    xs.timeout.set(30000);

    xs.with_cmd(|cmd: &mut ScsiGeneric| {
        cmd.opcode = GPCMD_READ_DVD_STRUCTURE;
        cmd.bytes[6] = r#type;
        _lto2b(len as u32, &mut cmd.bytes[7..9]);

        if let Some(layer) = layer {
            cmd.bytes[5] = layer;
        }
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);
    error
}

/// `dvd_read_physical`: the physical format information of the four layers.
pub fn dvd_read_physical(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_READ_PHYSICAL_BUFSIZE, M_WAITOK).ok_or(ENOMEM)?;

    let layer_num = s.physical().layer_num;
    dvd_read_generic(
        sc,
        &mut buf,
        DVD_READ_PHYSICAL_BUFSIZE,
        s.r#type(),
        Some(layer_num),
    )?;

    let b = buf.bytes();
    let physical = s.physical();
    for (i, layer) in physical.layer.iter_mut().enumerate() {
        let bufp = &b[4 + i * 20..4 + (i + 1) * 20];

        *layer = crate::sys::cdio::DvdLayer::default();
        layer.book_version = bufp[0] & 0xf;
        layer.book_type = bufp[0] >> 4;
        layer.min_rate = bufp[1] & 0xf;
        layer.disc_size = bufp[1] >> 4;
        layer.layer_type = bufp[2] & 0xf;
        layer.track_path = (bufp[2] >> 4) & 1;
        layer.nlayers = (bufp[2] >> 5) & 3;
        layer.track_density = bufp[3] & 0xf;
        layer.linear_density = bufp[3] >> 4;
        layer.start_sector = _4btol(&bufp[4..8]);
        layer.end_sector = _4btol(&bufp[8..12]);
        layer.end_sector_l0 = _4btol(&bufp[12..16]);
        layer.bca = bufp[16] >> 7;
    }
    Ok(())
}

/// `DVD_READ_COPYRIGHT_BUFSIZE`.
pub const DVD_READ_COPYRIGHT_BUFSIZE: usize = 8;

/// `dvd_read_copyright`: the copyright information.
pub fn dvd_read_copyright(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_READ_COPYRIGHT_BUFSIZE, M_WAITOK).ok_or(ENOMEM)?;

    let layer_num = s.copyright().layer_num;
    dvd_read_generic(
        sc,
        &mut buf,
        DVD_READ_COPYRIGHT_BUFSIZE,
        s.r#type(),
        Some(layer_num),
    )?;

    let b = buf.bytes();
    let copyright = s.copyright();
    copyright.cpst = b[4];
    copyright.rmi = b[5];
    Ok(())
}

/// `sizeof(struct scsi_read_dvd_structure_data)`.
const DVD_STRUCTURE_DATA_SIZE: usize = size_of::<ScsiReadDvdStructureData>();

/// Runs a READ DVD STRUCTURE with the 12-byte CDB `scsi_read_dvd_structure` into `buf`, the
/// data reply `struct scsi_read_dvd_structure_data`.
fn dvd_read_structure_cmd(
    sc: &CdSoftc,
    buf: &mut DmaBuf,
    format: u8,
    agid: Option<u8>,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(sc.link(), SCSI_DATA_IN).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiReadDvdStructure>() as i32);
    let b = buf.bytes();
    // SAFETY: `buf` is a `DVD_STRUCTURE_DATA_SIZE` allocation borrowed until `scsi_xs_sync`
    // has returned below.
    unsafe { xs.set_data(b.as_mut_ptr(), DVD_STRUCTURE_DATA_SIZE as i32) };
    xs.timeout.set(30000);

    xs.with_cmd(|cmd: &mut ScsiReadDvdStructure| {
        cmd.opcode = GPCMD_READ_DVD_STRUCTURE;
        cmd.format = format;
        if let Some(agid) = agid {
            cmd.agid = agid << 6;
        }
        _lto2b(DVD_STRUCTURE_DATA_SIZE as u32, &mut cmd.length);
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);
    error
}

/// `dvd_read_disckey`: the disc key.
pub fn dvd_read_disckey(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_STRUCTURE_DATA_SIZE, M_WAITOK).ok_or(ENOMEM)?;

    let agid = s.disckey().agid;
    dvd_read_structure_cmd(sc, &mut buf, s.r#type(), Some(agid))?;

    let data = buf.wire::<ScsiReadDvdStructureData>();
    let value = &mut s.disckey().value;
    let n = value.len();
    value.copy_from_slice(&data.data[..n]);
    Ok(())
}

/// `DVD_READ_BCA_BUFLEN`.
pub const DVD_READ_BCA_BUFLEN: usize = 4 + 188;

/// `dvd_read_bca`: the burst cutting area.
pub fn dvd_read_bca(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_READ_BCA_BUFLEN, M_WAITOK).ok_or(ENOMEM)?;

    dvd_read_generic(sc, &mut buf, DVD_READ_BCA_BUFLEN, s.r#type(), None)?;

    let b = buf.bytes();
    let len = _2btol(&b[0..2]);
    s.bca().len = len as i32;
    if !(12..=188).contains(&len) {
        return Err(EIO);
    }
    let len = len as usize;
    s.bca().value[..len].copy_from_slice(&b[4..4 + len]);
    Ok(())
}

/// `dvd_read_manufact`: the manufacturing information.
pub fn dvd_read_manufact(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    let mut buf = DmaBuf::new(DVD_STRUCTURE_DATA_SIZE, M_WAITOK).ok_or(ENOMEM)?;

    dvd_read_structure_cmd(sc, &mut buf, s.r#type(), None)?;

    let data = buf.wire::<ScsiReadDvdStructureData>();
    let len = _2btol(&data.len) as i32;
    s.manufact().len = len;
    if (0..=2048).contains(&len) {
        let len = len as usize;
        s.manufact().value[..len].copy_from_slice(&data.data[..len]);
        Ok(())
    } else {
        Err(EIO)
    }
}

/// `dvd_read_struct`: `DVD_READ_STRUCT`, by the structure's type.
pub fn dvd_read_struct(sc: &CdSoftc, s: &mut DvdStruct) -> Result<(), Errno> {
    match s.r#type() {
        DVD_STRUCT_PHYSICAL => dvd_read_physical(sc, s),
        DVD_STRUCT_COPYRIGHT => dvd_read_copyright(sc, s),
        DVD_STRUCT_DISCKEY => dvd_read_disckey(sc, s),
        DVD_STRUCT_BCA => dvd_read_bca(sc, s),
        DVD_STRUCT_MANUFACT => dvd_read_manufact(sc, s),
        _ => Err(EINVAL),
    }
}

/// `cd_interpret_sense`: checks errors. Lets the generic code handle everything except the
/// unit becoming ready on open devices.
pub fn cd_interpret_sense(xs: &'static ScsiXfer) -> Result<(), Errno> {
    let sense = xs.sense.get();
    let link = xs.link();
    let skey = sense.flags & SSD_KEY;
    let serr = sense.error_code & SSD_ERRCODE;

    if !link_isset(link, SDEV_OPEN) || (serr != SSD_ERRCODE_CURRENT && serr != SSD_ERRCODE_DEFERRED)
    {
        return scsi_interpret_sense(xs);
    }

    // We do custom processing in cd for the unit becoming ready case. We do not allow
    // xs->retries to be decremented on the "Unit Becoming Ready" case. This is because CD
    // drives report "Unit Becoming Ready" when loading media and can take a long time.
    // Rather than having a massive timeout for all operations (which would cause other
    // problems), we allow operations to wait (but be interruptible with Ctrl-C) forever as
    // long as the drive is reporting that it is becoming ready. All other cases of not
    // being ready are handled by the default handler.
    if skey == SKEY_NOT_READY {
        if xs.flags.get() & SCSI_IGNORE_NOT_READY != 0 {
            return Ok(());
        }
        if asc_ascq(&sense) == SENSE_NOT_READY_BECOMING_READY {
            // SC_DEBUG(link, SDEV_DB1, ("not ready: busy (%#x)\n", ...)).
            // don't count this as a retry
            xs.retries.set(xs.retries.get() + 1);
            return scsi_delay(xs, 1);
        }
        // XXX more to come here for a few other cases
    }
    scsi_interpret_sense(xs)
}

/// `cd_size`: finds out from the device what its capacity is: the number of sectors (0 when
/// it cannot tell), and the sector size in `blksize`.
pub fn cd_size(link: &'static ScsiLink, flags: i32, mut blksize: Option<&mut u32>) -> u64 {
    if let Some(b) = blksize.as_deref_mut() {
        *b = 0;
    }

    let flags = flags & !SCSI_IGNORE_ILLEGAL_REQUEST;

    // Start with a READ CAPACITY(10).
    let mut rdcap = ScsiReadCapData::zeroed();
    if scsi_read_cap_10(link, &mut rdcap, flags).is_err() {
        return 0;
    }

    let mut max_addr = u64::from(_4btol(&rdcap.addr));
    if let Some(b) = blksize.as_deref_mut() {
        *b = _4btol(&rdcap.length);
    }

    // pre-SPC (i.e. pre-SCSI-3) devices reporting less than 2^32-1 sectors can stop here.
    let stop = sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_SPC && max_addr != 0xffff_ffff;

    if !stop {
        let mut rdcap16 = ScsiReadCapData16::zeroed();
        if scsi_read_cap_16(link, &mut rdcap16, flags).is_ok() {
            max_addr = _8btol(&rdcap16.addr);
            if let Some(b) = blksize.as_deref_mut() {
                *b = _4btol(&rdcap16.length);
            }
            // XXX The other READ CAPACITY(16) info could be stored away.

            return max_addr + 1;
        }
    }

    // exit: return READ CAPACITY 10 values.
    if max_addr != 0xffff_ffff {
        return max_addr + 1;
    } else if let Some(b) = blksize {
        *b = 0;
    }
    0
}

const _: () = {
    assert!(size_of::<ScsiBlank>() == 12);
    assert!(size_of::<ScsiCloseTrack>() == 10);
    assert!(size_of::<ScsiPause>() == 10);
    assert!(size_of::<ScsiPlayMsf>() == 10);
    assert!(size_of::<ScsiPlayTrack>() == 10);
    assert!(size_of::<ScsiPlay>() == 10);
    assert!(size_of::<ScsiPlayBig>() == 12);
    assert!(size_of::<ScsiPlayRelBig>() == 12);
    assert!(size_of::<ScsiReadHeader>() == 10);
    assert!(size_of::<ScsiReadSubchannel>() == 10);
    assert!(size_of::<ScsiReadToc>() == 10);
    assert!(size_of::<ScsiReadTrackInfo>() == 10);
    assert!(size_of::<ScsiLoadUnload>() == 12);
    assert!(size_of::<ScsiSetCdSpeed>() == 12);
    assert!(size_of::<PortControl>() == 2);
    assert!(size_of::<CdAudioPage>() == 16);
    assert!(offset_of!(CdAudioPage, port) == 8);
    assert!(size_of::<ScsiReadDvdStructure>() == 12);
    assert!(size_of::<ScsiReadDvdStructureData>() == 2052);
    assert!(CD_TOC_SIZE == 804);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `cd.rs`: the READ/WRITE CDBs, the MSF arithmetic of `cd_play_tracks`,
    // `cd_size`/`cd_get_parms` (capacity, the defaults and the lying drives), the table of
    // contents and the play, mode page and DVD commands against a fake adapter that completes
    // every command at once, recording its CDB.

    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::subr_pool::{pool_destroy, pool_init};
    use crate::machine::intr::IPL_BIO;
    use crate::scsi::scsi_all::{MODE_SELECT, MODE_SENSE, SKEY_UNIT_ATTENTION, ScsiSenseData};
    use crate::scsi::scsi_base::{
        SCSI_XFER_POOL, scsi_copy_internal_data, scsi_default_get, scsi_default_put, scsi_done,
        scsi_iopool_init,
    };
    use crate::scsi::scsiconf::{ScsiAdapter, ScsiIopool, ScsibusSoftc, XS_DRIVER_STUFFUP};
    use crate::sys::cdio::{CD_LU_LOAD, DvdAuthinfo, DvdStruct};

    /// What the fake adapter does with the next command.
    enum Reply {
        /// Copies the bytes in (`scsi_copy_internal_data`; nothing for an empty reply).
        Data(Vec<u8>),
        /// Ends the command with this `XS_*` error.
        Error(i32),
    }

    std::thread_local! {
        /// The fake adapter's script.
        static SCRIPT: RefCell<VecDeque<Reply>> = const { RefCell::new(VecDeque::new()) };
        /// The CDB (`cmdlen` bytes) and data length of every command sent.
        static SENT: RefCell<Vec<(Vec<u8>, i32)>> = const { RefCell::new(Vec::new()) };
    }

    /// The fake adapter's `scsi_cmd`: completes `xs` at once with the next scripted reply (an
    /// empty data reply when the script is out).
    fn fake_cmd(xs: &'static ScsiXfer) {
        let reply = SCRIPT.with(|s| s.borrow_mut().pop_front());
        SENT.with(|s| {
            let cmd = xs.cmd.get();
            let len = xs.cmdlen.get() as usize;
            s.borrow_mut()
                .push((cmd.as_bytes()[..len].to_vec(), xs.datalen()));
        });
        match reply.unwrap_or(Reply::Data(Vec::new())) {
            Reply::Data(d) => {
                if !d.is_empty() {
                    scsi_copy_internal_data(xs, &d);
                }
            }
            Reply::Error(e) => xs.error.set(e),
        }
        scsi_done(xs);
    }

    static FAKE_ADAPTER: ScsiAdapter = ScsiAdapter {
        scsi_cmd: fake_cmd,
        dev_minphys: None,
        dev_probe: None,
        dev_free: None,
        ioctl: None,
    };

    fn script(replies: Vec<Reply>) {
        SCRIPT.with(|s| *s.borrow_mut() = replies.into());
        SENT.with(|s| s.borrow_mut().clear());
    }

    fn sent() -> Vec<(Vec<u8>, i32)> {
        SENT.with(|s| s.borrow().clone())
    }

    fn opcodes() -> Vec<u8> {
        sent().iter().map(|(cdb, _)| cdb[0]).collect()
    }

    /// Real memory and a fresh `scsi_xfer_pool`.
    fn setup() -> MutexGuard<'static, ()> {
        let g = crate::kern::subr_pool::tests::setup_real_memory();
        pool_init(
            &SCSI_XFER_POOL,
            size_of::<ScsiXfer>(),
            0,
            IPL_BIO,
            0,
            "scxspl",
            None,
        );
        g
    }

    fn teardown() {
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 0);
        pool_destroy(&SCSI_XFER_POOL);
    }

    /// A zeroed `T` (an `M_ZERO` allocation), leaked.
    ///
    /// # Safety
    ///
    /// All-zero must be a valid `T`.
    unsafe fn leak_zeroed<T>() -> &'static T {
        // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
        unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
    }

    /// A link on the fake adapter, with its own default iopool, for a CD-ROM of SCSI revision
    /// `version`.
    fn test_link(version: u8) -> &'static ScsiLink {
        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
        // SAFETY: the default allocator ignores its cookie.
        unsafe {
            scsi_iopool_init(
                pool,
                ptr::from_ref(link).cast_mut().cast(),
                scsi_default_get,
                scsi_default_put,
            );
        }
        // SAFETY: all-zero is a valid `ScsibusSoftc` (its `Softc` impl).
        let sb: &'static ScsibusSoftc = unsafe { leak_zeroed() };
        sb.sb_adapter.set(Some(&FAKE_ADAPTER));
        link.pool.set(Some(pool));
        link.bus.set(Some(sb));
        link.openings.set(1);
        let mut inq = link.inqdata.get();
        inq.device = T_CDROM;
        inq.version = version;
        link.inqdata.set(inq);
        link
    }

    /// A cd softc driving `link`, as `cdattach` leaves it (without the commands).
    fn test_cd(link: &'static ScsiLink) -> &'static CdSoftc {
        // SAFETY: all-zero is a valid `CdSoftc` (its `Softc` impl).
        let sc: &'static CdSoftc = unsafe { leak_zeroed() };
        sc.sc_link.set(Some(link));
        link.device_softc.set(Some(NonNull::from(&sc.sc_dev)));
        sc
    }

    /// READ CAPACITY (10) data: last block `last`, `secsize` bytes per block.
    fn cap10(last: u32, secsize: u32) -> Reply {
        let mut d = vec![0u8; 8];
        _lto4b(last, &mut d[0..4]);
        _lto4b(secsize, &mut d[4..8]);
        Reply::Data(d)
    }

    /// READ CAPACITY (16) data.
    fn cap16(last: u64, secsize: u32) -> Reply {
        let mut d = vec![0u8; 32];
        crate::scsi::scsiconf::_lto8b(last, &mut d[0..8]);
        _lto4b(secsize, &mut d[8..12]);
        Reply::Data(d)
    }

    /// The bytes of a table of contents with tracks `first..=last` plus the lead-out, each
    /// entry's address the `(m, s, f)` of `msf[i]` (in the layout the device writes: header
    /// length in big-endian).
    fn toc_bytes(first: u8, last: u8, msf: &[(u8, u8, u8)]) -> Vec<u8> {
        let n = usize::from(last - first) + 2;
        let mut d = vec![0u8; size_of::<IocTocHeader>() + n * size_of::<CdTocEntry>()];
        let len = (d.len() - 2) as u16;
        d[0..2].copy_from_slice(&len.to_be_bytes());
        d[2] = first;
        d[3] = last;
        for (i, &(m, s, f)) in msf.iter().enumerate().take(n) {
            let off = size_of::<IocTocHeader>() + i * size_of::<CdTocEntry>();
            d[off + 1] = 0x10; // control 0 (audio), addr_type 1
            d[off + 2] = first + i as u8;
            d[off + 4..off + 8].copy_from_slice(&[0, m, s, f]);
        }
        d
    }

    #[test]
    fn rw_cdbs() {
        let mut g = ScsiGeneric::zeroed();
        assert_eq!(cd_cmd_rw6(&mut g, true, 0x12_3456, 0x80), 6);
        assert_eq!(
            &g.as_bytes()[..6],
            &[READ_COMMAND, 0x12, 0x34, 0x56, 0x80, 0]
        );

        let mut g = ScsiGeneric::zeroed();
        assert_eq!(cd_cmd_rw10(&mut g, false, 0x0102_0304, 0x0506), 10);
        assert_eq!(&g.as_bytes()[..10], &[WRITE_10, 0, 1, 2, 3, 4, 0, 5, 6, 0]);

        let mut g = ScsiGeneric::zeroed();
        assert_eq!(cd_cmd_rw12(&mut g, true, 7, 0x0001_0000), 12);
        assert_eq!(
            &g.as_bytes()[..12],
            &[READ_12, 0, 0, 0, 0, 7, 0, 1, 0, 0, 0, 0]
        );
    }

    #[test]
    fn msf_prev_borrows_across_frames_seconds_and_minutes() {
        assert_eq!(cd_msf_prev(1, 2, 3), Ok((1, 2, 2)));
        assert_eq!(cd_msf_prev(1, 2, 0), Ok((1, 1, 74)));
        assert_eq!(cd_msf_prev(1, 0, 0), Ok((0, 59, 74)));
        assert_eq!(cd_msf_prev(0, 0, 0), Err(EINVAL));
    }

    #[test]
    fn toc_accessors_decode_the_device_layout() {
        let mut toc = CdToc::new();
        toc.bytes[..12].copy_from_slice(&toc_bytes(1, 1, &[(0, 2, 0), (3, 4, 5)])[..12]);
        let th = toc.header();
        assert_eq!((th.starting_track, th.ending_track), (1, 1));
        assert_eq!(u16::from_be(th.len), 18);
        let e = toc.entry(0).unwrap();
        assert_eq!((e.control(), e.addr_type(), e.track), (0, 1, 1));
        assert_eq!(e.addr.second(), 2);
        assert!(toc.entry(MAXTRACK).is_some() && toc.entry(MAXTRACK + 1).is_none());

        let mut e = CdTocEntry::default();
        e.set_addr_type(CD_LBA_FORMAT);
        e.addr.set_lba(0x1234);
        toc.set_entry(3, &e);
        assert_eq!(toc.entry(3), Some(e));
        toc.set_entry(MAXTRACK + 1, &e);
        assert_eq!(toc.entries().len(), (MAXTRACK + 1) * 8);
    }

    #[test]
    fn audio_page_has_the_ports_after_eight_bytes() {
        let mut page = [0u8; 16];
        let p: &mut CdAudioPage = wire_mut(&mut page);
        p.port[LEFT_PORT].channels = LEFT_CHANNEL;
        p.port[RIGHT_PORT].volume = 0x7f;
        assert_eq!(page[8], 1);
        assert_eq!(page[11], 0x7f);
    }

    #[test]
    fn cd_size_reads_capacity_10_then_16_for_spc_devices() {
        let _g = setup();

        // SCSI-2 device: READ CAPACITY (10) and done.
        let link = test_link(SCSI_REV_2);
        script(vec![cap10(0x0002_bf1f, 2048)]);
        let mut secsize = 0;
        assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0x2_bf20);
        assert_eq!(secsize, 2048);
        assert_eq!(opcodes(), vec![crate::scsi::scsi_disk::READ_CAPACITY]);

        // SPC device: then READ CAPACITY (16), whose values win.
        let link = test_link(SCSI_REV_SPC);
        script(vec![cap10(0x0002_bf1f, 2048), cap16(0x0002_bf1f, 2048)]);
        let mut secsize = 0;
        assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0x2_bf20);
        assert_eq!(
            opcodes(),
            vec![
                crate::scsi::scsi_disk::READ_CAPACITY,
                crate::scsi::scsi_disk::READ_CAPACITY_16
            ]
        );

        // SPC device whose READ CAPACITY (16) fails: the (10) values stand.
        script(vec![cap10(99, 512), Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(cd_size(link, 0, Some(&mut secsize)), 100);
        assert_eq!(secsize, 512);

        // A device that cannot say: 0.
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(cd_size(link, 0, None), 0);

        // More than 2^32 - 1 sectors and no (16): nothing.
        script(vec![
            cap10(0xffff_ffff, 2048),
            Reply::Error(XS_DRIVER_STUFFUP),
        ]);
        assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0);
        assert_eq!(secsize, 0);
        teardown();
    }

    #[test]
    fn get_parms_has_defaults_for_lying_drives() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);

        script(vec![cap10(0x0002_bf1f, 2048)]);
        assert_eq!(cd_get_parms(sc, 0), Ok(()));
        assert_eq!(
            sc.params.get(),
            CdParms {
                secsize: 2048,
                disksize: 0x2_bf20
            }
        );

        // A sector size that is not a multiple of 512 is a lie; so is a tiny disc.
        script(vec![cap10(5, 300)]);
        assert_eq!(cd_get_parms(sc, 0), Ok(()));
        assert_eq!(
            sc.params.get(),
            CdParms {
                secsize: 2048,
                disksize: 400000
            }
        );

        // A failed READ CAPACITY leaves the defaults.
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(cd_get_parms(sc, 0), Ok(()));
        assert_eq!(
            sc.params.get(),
            CdParms {
                secsize: 2048,
                disksize: 400000
            }
        );

        // ADEV_NOCAPACITY: no command at all.
        link.quirks.set(ADEV_NOCAPACITY);
        script(vec![]);
        assert_eq!(cd_get_parms(sc, 0), Ok(()));
        assert!(sent().is_empty());
        teardown();
    }

    #[test]
    fn load_toc_asks_for_the_header_then_every_entry() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);
        let full = toc_bytes(1, 2, &[(0, 2, 0), (3, 0, 0), (7, 30, 10)]);

        script(vec![
            Reply::Data(full[..4].to_vec()),
            Reply::Data(full.clone()),
        ]);
        let mut toc = CdToc::new();
        assert_eq!(cd_load_toc(sc, &mut toc, CD_MSF_FORMAT), Ok(()));
        let cmds = sent();
        assert_eq!(cmds.len(), 2);
        // READ TOC: opcode, MSF bit (second command only), from_track, data_len.
        assert_eq!(cmds[0].0[0], READ_TOC);
        assert_eq!(cmds[0].0[1], 0);
        assert_eq!(cmds[0].1, 4);
        assert_eq!(cmds[1].0[1], CD_MSF);
        assert_eq!(cmds[1].1, 4 + 3 * 8);
        assert_eq!(&cmds[1].0[7..9], &[0, 28]);
        assert_eq!(toc.entry(2).unwrap().addr.minute(), 7);

        // An ending track before the starting one: EIO after the first command.
        let mut bad = toc_bytes(1, 1, &[]);
        bad[2] = 5;
        bad[3] = 4;
        script(vec![Reply::Data(bad[..4].to_vec())]);
        assert_eq!(cd_load_toc(sc, &mut toc, CD_LBA_FORMAT), Err(EIO));
        assert_eq!(sent().len(), 1);
        teardown();
    }

    #[test]
    fn play_tracks_ends_one_frame_before_the_next_track() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);
        let full = toc_bytes(1, 2, &[(0, 2, 0), (3, 0, 0), (7, 30, 10)]);

        // Tracks 1 to 1: from entry 0 to just before entry 1.
        script(vec![
            Reply::Data(full[..4].to_vec()),
            Reply::Data(full.clone()),
        ]);
        assert_eq!(cd_play_tracks(sc, 1, 0, 1, 0), Ok(()));
        let cmds = sent();
        let play = &cmds[2].0;
        assert_eq!(play[0], PLAY_MSF);
        assert_eq!(&play[3..9], &[0, 2, 0, 2, 59, 74]);

        // No end track, a backwards range, and a start before the first track.
        assert_eq!(cd_play_tracks(sc, 1, 0, 0, 0), Err(EIO));
        assert_eq!(cd_play_tracks(sc, 3, 0, 2, 0), Err(EINVAL));
        script(vec![Reply::Data(full[..4].to_vec()), Reply::Data(full)]);
        assert_eq!(cd_play_tracks(sc, 0, 0, 2, 0), Err(EINVAL));
        teardown();
    }

    #[test]
    fn simple_commands_have_their_cdbs() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);

        script(vec![]);
        assert_eq!(cd_play(sc, 0x0102_0304, 0x0506), Ok(()));
        assert_eq!(cd_pause(sc, 1), Ok(()));
        assert_eq!(cd_load_unload(sc, i32::from(CD_LU_LOAD), 3), Ok(()));
        assert_eq!(cd_play_msf(sc, 1, 2, 3, 4, 5, 6), Ok(()));
        assert_eq!(cd_reset(sc), Ok(()));
        let cmds = sent();
        assert_eq!(&cmds[0].0[..10], &[PLAY, 0, 1, 2, 3, 4, 0, 5, 6, 0]);
        assert_eq!(cmds[1].0[0], PAUSE);
        assert_eq!(cmds[1].0[8], 1);
        assert_eq!(cmds[2].0[0], LOAD_UNLOAD);
        assert_eq!((cmds[2].0[4], cmds[2].0[8]), (CD_LU_LOAD, 3));
        assert_eq!(&cmds[3].0[..10], &[PLAY_MSF, 0, 0, 1, 2, 3, 4, 5, 6, 0]);
        teardown();
    }

    #[test]
    fn setchan_reads_the_audio_page_and_selects_it_back() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);

        // MODE SENSE (6) reply: header (no block descriptor) and the audio page.
        let mut page = vec![0u8; size_of::<CdAudioPage>()];
        page[0] = AUDIO_PAGE;
        page[1] = 14;
        let mut d = vec![0u8, 0, 0, 0];
        d.extend_from_slice(&page);
        d[0] = (d.len() - 1) as u8;
        script(vec![Reply::Data(d)]);
        assert_eq!(
            cd_setchan(
                sc,
                BOTH_CHANNEL,
                BOTH_CHANNEL,
                MUTE_CHANNEL,
                MUTE_CHANNEL,
                0
            ),
            Ok(())
        );
        assert_eq!(opcodes(), vec![MODE_SENSE, MODE_SELECT]);

        // A device without the page: EIO, no select.
        script(vec![Reply::Data(vec![3, 0, 0, 0])]);
        assert_eq!(cd_setchan(sc, 1, 2, 0, 0, 0), Err(EIO));
        assert_eq!(opcodes(), vec![MODE_SENSE]);

        // cd_getvol answers success even then.
        let mut v = IocVol::default();
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(cd_getvol(sc, &mut v, 0), Ok(()));
        teardown();
    }

    #[test]
    fn dvd_requests_check_their_type_and_parse_the_replies() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        let sc = test_cd(link);

        // Unknown types: EINVAL for a structure, ENOTTY for an authentication step, no command.
        script(vec![]);
        let mut s = DvdStruct::zeroed();
        s.physical().r#type = 0x40;
        assert_eq!(dvd_read_struct(sc, &mut s), Err(EINVAL));
        let mut a = DvdAuthinfo::zeroed();
        a.set_type(0x40);
        assert_eq!(dvd_auth(sc, &mut a), Err(ENOTTY));
        assert!(sent().is_empty());

        // REPORT KEY, AGID: the AGID is in the top two bits of byte 7.
        let mut reply = vec![0u8; 8];
        reply[7] = 2 << 6;
        script(vec![Reply::Data(reply)]);
        let mut a = DvdAuthinfo::zeroed();
        a.set_type(DVD_LU_SEND_AGID);
        assert_eq!(dvd_auth(sc, &mut a), Ok(()));
        assert_eq!(a.lsa().agid, 2);
        let cmds = sent();
        assert_eq!(cmds[0].0[0], GPCMD_REPORT_KEY);
        // cmd->bytes[8] is the length (8), bytes[9] the AGID/format byte (0).
        assert_eq!((cmds[0].0[9], cmds[0].0[10], cmds[0].1), (8, 0, 8));

        // The challenge goes out with its length bytes set; success moves the state on.
        script(vec![]);
        let mut a = DvdAuthinfo::zeroed();
        a.set_type(DVD_HOST_SEND_CHALLENGE);
        a.hsc().agid = 1;
        a.hsc().chal = [9; DVD_CHALLENGE_SIZE];
        assert_eq!(dvd_auth(sc, &mut a), Ok(()));
        assert_eq!(a.r#type(), DVD_LU_SEND_KEY1);
        assert_eq!(sent()[0].0[0], GPCMD_SEND_KEY);

        // A failed KEY2 exchange ends in DVD_AUTH_FAILURE.
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        let mut a = DvdAuthinfo::zeroed();
        a.set_type(DVD_HOST_SEND_KEY2);
        assert!(dvd_auth(sc, &mut a).is_err());
        assert_eq!(a.r#type(), DVD_AUTH_FAILURE);

        // READ DVD STRUCTURE, physical: the layers' fields come out of the 20-byte records.
        let mut reply = vec![0u8; DVD_READ_PHYSICAL_BUFSIZE];
        reply[4] = 0x21; // book type 2, version 1
        reply[4 + 4..4 + 8].copy_from_slice(&0x0003_0000u32.to_be_bytes());
        script(vec![Reply::Data(reply)]);
        let mut s = DvdStruct::zeroed();
        s.physical().r#type = DVD_STRUCT_PHYSICAL;
        s.physical().layer_num = 0;
        assert_eq!(dvd_read_struct(sc, &mut s), Ok(()));
        assert_eq!(s.physical().layer[0].book_type, 2);
        assert_eq!(s.physical().layer[0].book_version, 1);
        assert_eq!(s.physical().layer[0].start_sector, 0x3_0000);
        assert_eq!(sent()[0].0[6], DVD_STRUCT_PHYSICAL);

        // BCA: a length outside 12..=188 is EIO.
        let mut reply = vec![0u8; DVD_READ_BCA_BUFLEN];
        reply[1] = 4;
        script(vec![Reply::Data(reply)]);
        let mut s = DvdStruct::zeroed();
        s.bca().r#type = DVD_STRUCT_BCA;
        assert_eq!(dvd_read_struct(sc, &mut s), Err(EIO));
        teardown();
    }

    #[test]
    fn interpret_sense_counts_becoming_ready_as_no_retry() {
        let _g = setup();
        let link = test_link(SCSI_REV_2);
        link.flags.set(link.flags.get() | SDEV_OPEN);

        let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
        xs.sc_link.set(Some(link));
        xs.flags.set(crate::scsi::scsiconf::SCSI_NOSLEEP);
        xs.retries.set(2);
        xs.sense.set(ScsiSenseData {
            error_code: SSD_ERRCODE_CURRENT,
            flags: SKEY_NOT_READY,
            extra_len: 10,
            add_sense_code: 0x04,
            add_sense_code_qual: 0x01,
            ..ScsiSenseData::new()
        });
        // `retries` is incremented (the caller's decrement cancels it) and the command retried.
        assert_eq!(cd_interpret_sense(xs), Err(ERESTART));
        assert_eq!(xs.retries.get(), 3);

        // SCSI_IGNORE_NOT_READY: success.
        xs.flags.set(SCSI_IGNORE_NOT_READY);
        assert_eq!(cd_interpret_sense(xs), Ok(()));

        // Anything else is the generic code's (a unit attention is retried).
        xs.sense.set(ScsiSenseData {
            error_code: SSD_ERRCODE_CURRENT,
            flags: SKEY_UNIT_ATTENTION,
            ..ScsiSenseData::new()
        });
        xs.flags.set(0);
        assert_eq!(cd_interpret_sense(xs), scsi_interpret_sense(xs));
        teardown();
    }
}
/* </TESTS> */
