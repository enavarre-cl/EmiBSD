/* <LICENSES> */
/* </LICENSES> */

/* <CODE> */
//! `<sys/cdio.h>`: the CD-ROM and DVD ioctls, shared between kernel and process: the audio
//! play, table of contents, sub-channel and volume requests of `cd(4)` and the DVD
//! structure and authentication requests.
//!
//! Upstream: sys/sys/cdio.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above); every notice in the reference tree is accepted (the user's rule of
//! 2026-10-04).
//!
//! ## Deviations
//! - Every structure is `#[repr(C)]` with the C's LP64 size, made of integers and byte
//!   arrays only, with the compiler's holes named `_pad*`, so that it is [`AbiPod`] (read
//!   from and written to an `ioctl` buffer as plain bytes); sizes are asserted at the end.
//! - `union msf_lba` is [`MsfLba`], four bytes with the `msf`, `lba` and `addr` views as
//!   methods (`lba` in the CPU's byte order, as the C union shares its bytes).
//! - The bit-fields (`addr_type:4`, `control:4`, `mc_valid:1`, `ti_valid:1`) are the byte
//!   that holds them, with accessor methods; both archs are little-endian, so `control` is the
//!   low nibble (the `_BYTE_ORDER == _LITTLE_ENDIAN` arm of the C).
//! - The `union`s (`what` of `struct cd_sub_channel_info`, `union dvd_struct`, `union
//!   dvd_authinfo`) are Rust unions of those plain structures; their accessor methods read a
//!   member through the union, which is sound because every member is plain integers and
//!   byte arrays and the unions are only built by `zeroed` or from bytes.
//! - `struct ioc_read_subchannel`'s and `struct ioc_read_toc_entry`'s `data` pointers are
//!   `usize` user addresses, only handed to `copyout` (`docs/C_TO_RUST.md`); the hole before
//!   `data_len` and `data` is `_pad0`.
//! - `u_char`/`u_int8_t` are `u8`, `u_short` is `u16`, `int` is `i32`.

use core::mem::{offset_of, size_of};

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};

/// `union msf_lba`: a track address as minute, second and frame, or as a logical block
/// address, or as four bytes (`addr`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MsfLba {
    /// `addr`: the four bytes all the views share.
    pub addr: [u8; 4],
}

impl MsfLba {
    /// `msf.unused`.
    pub const fn unused(&self) -> u8 {
        self.addr[0]
    }

    /// `msf.minute`.
    pub const fn minute(&self) -> u8 {
        self.addr[1]
    }

    /// `msf.second`.
    pub const fn second(&self) -> u8 {
        self.addr[2]
    }

    /// `msf.frame`.
    pub const fn frame(&self) -> u8 {
        self.addr[3]
    }

    /// `lba`: the same four bytes as a `u_int32_t` in the CPU's byte order.
    pub const fn lba(&self) -> u32 {
        u32::from_ne_bytes(self.addr)
    }

    /// `lba = v`.
    pub const fn set_lba(&mut self, v: u32) {
        self.addr = v.to_ne_bytes();
    }
}

// SAFETY: `#[repr(C)]`, four bytes: no padding, any pattern valid.
unsafe impl AbiPod for MsfLba {}

/// `struct cd_toc_entry`: one track of the table of contents. `addr_type` and `control` are
/// the two nibbles of `ac`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdTocEntry {
    /// `nothing1`.
    pub nothing1: u8,
    /// The byte holding the bit-fields `control:4` (low) and `addr_type:4` (high).
    pub ac: u8,
    /// `track`.
    pub track: u8,
    /// `nothing2`.
    pub nothing2: u8,
    /// `addr`.
    pub addr: MsfLba,
}

impl CdTocEntry {
    /// `control`.
    pub const fn control(&self) -> u8 {
        self.ac & 0x0f
    }

    /// `addr_type`.
    pub const fn addr_type(&self) -> u8 {
        self.ac >> 4
    }

    /// `control = v`.
    pub const fn set_control(&mut self, v: u8) {
        self.ac = (self.ac & 0xf0) | (v & 0x0f);
    }

    /// `addr_type = v`.
    pub const fn set_addr_type(&mut self, v: u8) {
        self.ac = (self.ac & 0x0f) | ((v & 0x0f) << 4);
    }
}

// SAFETY: `#[repr(C)]`, four bytes and an `MsfLba`: eight bytes, no padding.
unsafe impl AbiPod for CdTocEntry {}

/// `CD_AS_AUDIO_INVALID`.
pub const CD_AS_AUDIO_INVALID: u8 = 0x00;
/// `CD_AS_PLAY_IN_PROGRESS`.
pub const CD_AS_PLAY_IN_PROGRESS: u8 = 0x11;
/// `CD_AS_PLAY_PAUSED`.
pub const CD_AS_PLAY_PAUSED: u8 = 0x12;
/// `CD_AS_PLAY_COMPLETED`.
pub const CD_AS_PLAY_COMPLETED: u8 = 0x13;
/// `CD_AS_PLAY_ERROR`.
pub const CD_AS_PLAY_ERROR: u8 = 0x14;
/// `CD_AS_NO_STATUS`.
pub const CD_AS_NO_STATUS: u8 = 0x15;

/// `struct cd_sub_channel_header`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdSubChannelHeader {
    /// `nothing1`.
    pub nothing1: u8,
    /// `audio_status`: `CD_AS_*`.
    pub audio_status: u8,
    /// `data_len`: big-endian.
    pub data_len: [u8; 2],
}

// SAFETY: `#[repr(C)]`, four bytes.
unsafe impl AbiPod for CdSubChannelHeader {}

/// `struct cd_sub_channel_q_data`. `control` and `addr_type` share `ac` as in
/// [`CdTocEntry`]; `mc_valid` and `ti_valid` are bit 7 of `mc_flags` and `ti_flags`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdSubChannelQData {
    /// `data_format`.
    pub data_format: u8,
    /// `control:4` (low) and `addr_type:4` (high).
    pub ac: u8,
    /// `track_number`.
    pub track_number: u8,
    /// `index_number`.
    pub index_number: u8,
    /// `absaddr`.
    pub absaddr: [u8; 4],
    /// `reladdr`.
    pub reladdr: [u8; 4],
    /// The byte of `:7` and `mc_valid:1` (bit 7).
    pub mc_flags: u8,
    /// `mc_number`.
    pub mc_number: [u8; 15],
    /// The byte of `:7` and `ti_valid:1` (bit 7).
    pub ti_flags: u8,
    /// `ti_number`.
    pub ti_number: [u8; 15],
}

// SAFETY: `#[repr(C)]`, bytes and byte arrays only (44 bytes, no padding).
unsafe impl AbiPod for CdSubChannelQData {}

/// `struct cd_sub_channel_position_data`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdSubChannelPositionData {
    /// `data_format`.
    pub data_format: u8,
    /// `control:4` (low) and `addr_type:4` (high).
    pub ac: u8,
    /// `track_number`.
    pub track_number: u8,
    /// `index_number`.
    pub index_number: u8,
    /// `absaddr`.
    pub absaddr: MsfLba,
    /// `reladdr`.
    pub reladdr: MsfLba,
}

// SAFETY: `#[repr(C)]`, four bytes and two `MsfLba`s (12 bytes, no padding).
unsafe impl AbiPod for CdSubChannelPositionData {}

/// `struct cd_sub_channel_media_catalog`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdSubChannelMediaCatalog {
    /// `data_format`.
    pub data_format: u8,
    /// `nothing1`.
    pub nothing1: u8,
    /// `nothing2`.
    pub nothing2: u8,
    /// `nothing3`.
    pub nothing3: u8,
    /// The byte of `:7` and `mc_valid:1` (bit 7).
    pub mc_flags: u8,
    /// `mc_number`.
    pub mc_number: [u8; 15],
}

// SAFETY: `#[repr(C)]`, bytes and a byte array (20 bytes, no padding).
unsafe impl AbiPod for CdSubChannelMediaCatalog {}

/// `struct cd_sub_channel_track_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CdSubChannelTrackInfo {
    /// `data_format`.
    pub data_format: u8,
    /// `nothing1`.
    pub nothing1: u8,
    /// `track_number`.
    pub track_number: u8,
    /// `nothing2`.
    pub nothing2: u8,
    /// The byte of `:7` and `ti_valid:1` (bit 7).
    pub ti_flags: u8,
    /// `ti_number`.
    pub ti_number: [u8; 15],
}

// SAFETY: `#[repr(C)]`, bytes and a byte array (20 bytes, no padding).
unsafe impl AbiPod for CdSubChannelTrackInfo {}

/// The anonymous union `what` of `struct cd_sub_channel_info`: one of four formats, the
/// largest being 44 bytes. All members are plain bytes: reading any of them is sound.
#[repr(C)]
#[derive(Clone, Copy)]
pub union CdSubChannelWhat {
    /// `q_data`.
    pub q_data: CdSubChannelQData,
    /// `position`.
    pub position: CdSubChannelPositionData,
    /// `media_catalog`.
    pub media_catalog: CdSubChannelMediaCatalog,
    /// `track_info`.
    pub track_info: CdSubChannelTrackInfo,
}

/// `struct cd_sub_channel_info`: the reply of READ SUB-CHANNEL.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CdSubChannelInfo {
    /// `header`.
    pub header: CdSubChannelHeader,
    /// `what`.
    pub what: CdSubChannelWhat,
}

impl CdSubChannelInfo {
    /// All zeros (`PR_ZERO`; the whole union is initialised through its largest member).
    pub const fn zeroed() -> Self {
        Self {
            header: CdSubChannelHeader {
                nothing1: 0,
                audio_status: 0,
                data_len: [0; 2],
            },
            what: CdSubChannelWhat {
                q_data: CdSubChannelQData {
                    data_format: 0,
                    ac: 0,
                    track_number: 0,
                    index_number: 0,
                    absaddr: [0; 4],
                    reladdr: [0; 4],
                    mc_flags: 0,
                    mc_number: [0; 15],
                    ti_flags: 0,
                    ti_number: [0; 15],
                },
            },
        }
    }

    /// The structure as bytes, for a transfer to read into.
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: `#[repr(C)]` made of integers and byte arrays only, 48 bytes with no
        // padding (the sizes asserted below add up); `zeroed` initialised every byte and
        // every pattern is valid.
        unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::from_mut(self).cast::<u8>(),
                size_of::<Self>(),
            )
        }
    }

    /// The structure as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: as in `as_bytes_mut`.
        unsafe {
            core::slice::from_raw_parts(core::ptr::from_ref(self).cast::<u8>(), size_of::<Self>())
        }
    }
}

// SAFETY: `#[repr(C)]`, a four-byte header and a union of plain structures, 48 bytes with no
// padding; the only constructor, `zeroed`, initialises every byte and all patterns are valid.
unsafe impl AbiPod for CdSubChannelInfo {}

/// `struct ioc_play_track`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocPlayTrack {
    /// `start_track`.
    pub start_track: u8,
    /// `start_index`.
    pub start_index: u8,
    /// `end_track`.
    pub end_track: u8,
    /// `end_index`.
    pub end_index: u8,
}

// SAFETY: `#[repr(C)]`, four bytes.
unsafe impl AbiPod for IocPlayTrack {}

/// `CDIOCPLAYTRACKS`.
pub const CDIOCPLAYTRACKS: u64 = _iow::<IocPlayTrack>(b'c', 1);

/// `struct ioc_play_blocks`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocPlayBlocks {
    /// `blk`.
    pub blk: i32,
    /// `len`.
    pub len: i32,
}

// SAFETY: `#[repr(C)]`, two `int`s.
unsafe impl AbiPod for IocPlayBlocks {}

/// `CDIOCPLAYBLOCKS`.
pub const CDIOCPLAYBLOCKS: u64 = _iow::<IocPlayBlocks>(b'c', 2);

/// `CD_LBA_FORMAT`: `address_format` of the sub-channel and table of contents requests.
pub const CD_LBA_FORMAT: u8 = 1;
/// `CD_MSF_FORMAT`.
pub const CD_MSF_FORMAT: u8 = 2;
/// `CD_SUBQ_DATA`: `data_format` of `struct ioc_read_subchannel`.
pub const CD_SUBQ_DATA: u8 = 0;
/// `CD_CURRENT_POSITION`.
pub const CD_CURRENT_POSITION: u8 = 1;
/// `CD_MEDIA_CATALOG`.
pub const CD_MEDIA_CATALOG: u8 = 2;
/// `CD_TRACK_INFO`.
pub const CD_TRACK_INFO: u8 = 3;

/// `struct ioc_read_subchannel`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocReadSubchannel {
    /// `address_format`: `CD_LBA_FORMAT` or `CD_MSF_FORMAT`.
    pub address_format: u8,
    /// `data_format`: `CD_SUBQ_DATA` ...
    pub data_format: u8,
    /// `track`.
    pub track: u8,
    /// The C compiler's padding before `data_len`.
    pub _pad0: u8,
    /// `data_len`.
    pub data_len: i32,
    /// `data`: user address of a `struct cd_sub_channel_info`.
    pub data: usize,
}

// SAFETY: `#[repr(C)]`, integers with the one hole named `_pad0` (16 bytes, no padding).
unsafe impl AbiPod for IocReadSubchannel {}

/// `CDIOCREADSUBCHANNEL`.
pub const CDIOCREADSUBCHANNEL: u64 = _iowr::<IocReadSubchannel>(b'c', 3);

/// `struct ioc_toc_header`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocTocHeader {
    /// `len`.
    pub len: u16,
    /// `starting_track`.
    pub starting_track: u8,
    /// `ending_track`.
    pub ending_track: u8,
}

// SAFETY: `#[repr(C)]`, a `u_short` and two bytes: four bytes, no padding.
unsafe impl AbiPod for IocTocHeader {}

/// `CDIOREADTOCHEADER`.
pub const CDIOREADTOCHEADER: u64 = _ior::<IocTocHeader>(b'c', 4);

/// `CD_TRACK_LEADOUT`: `starting_track` of the lead-out.
pub const CD_TRACK_LEADOUT: u8 = 0xaa;

/// `struct ioc_read_toc_entry`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocReadTocEntry {
    /// `address_format`.
    pub address_format: u8,
    /// `starting_track`.
    pub starting_track: u8,
    /// `data_len`.
    pub data_len: u16,
    /// The C compiler's padding before `data`.
    pub _pad0: u32,
    /// `data`: user address of the `struct cd_toc_entry` array.
    pub data: usize,
}

// SAFETY: `#[repr(C)]`, integers with the one hole named `_pad0` (16 bytes, no padding).
unsafe impl AbiPod for IocReadTocEntry {}

/// `CDIOREADTOCENTRIES`.
pub const CDIOREADTOCENTRIES: u64 = _iowr::<IocReadTocEntry>(b'c', 5);
/// `CDIOREADTOCENTRYS`.
pub const CDIOREADTOCENTRYS: u64 = CDIOREADTOCENTRIES;

/// `CDIOREADMSADDR`: read LBA start of a given session; 0=last, others not yet supported.
pub const CDIOREADMSADDR: u64 = _iowr::<i32>(b'c', 6);

/// `struct ioc_patch`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocPatch {
    /// `patch`: one for each channel.
    pub patch: [u8; 4],
}

// SAFETY: `#[repr(C)]`, four bytes.
unsafe impl AbiPod for IocPatch {}

/// `CDIOCSETPATCH`.
pub const CDIOCSETPATCH: u64 = _iow::<IocPatch>(b'c', 9);

/// `struct ioc_vol`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocVol {
    /// `vol`: one for each channel.
    pub vol: [u8; 4],
}

// SAFETY: `#[repr(C)]`, four bytes.
unsafe impl AbiPod for IocVol {}

/// `CDIOCGETVOL`.
pub const CDIOCGETVOL: u64 = _ior::<IocVol>(b'c', 10);
/// `CDIOCSETVOL`.
pub const CDIOCSETVOL: u64 = _iow::<IocVol>(b'c', 11);
/// `CDIOCSETMONO`.
pub const CDIOCSETMONO: u64 = _io(b'c', 12);
/// `CDIOCSETSTEREO`.
pub const CDIOCSETSTEREO: u64 = _io(b'c', 13);
/// `CDIOCSETMUTE`.
pub const CDIOCSETMUTE: u64 = _io(b'c', 14);
/// `CDIOCSETLEFT`.
pub const CDIOCSETLEFT: u64 = _io(b'c', 15);
/// `CDIOCSETRIGHT`.
pub const CDIOCSETRIGHT: u64 = _io(b'c', 16);
/// `CDIOCSETDEBUG`.
pub const CDIOCSETDEBUG: u64 = _io(b'c', 17);
/// `CDIOCCLRDEBUG`.
pub const CDIOCCLRDEBUG: u64 = _io(b'c', 18);
/// `CDIOCPAUSE`.
pub const CDIOCPAUSE: u64 = _io(b'c', 19);
/// `CDIOCRESUME`.
pub const CDIOCRESUME: u64 = _io(b'c', 20);
/// `CDIOCRESET`.
pub const CDIOCRESET: u64 = _io(b'c', 21);
/// `CDIOCSTART`.
pub const CDIOCSTART: u64 = _io(b'c', 22);
/// `CDIOCSTOP`.
pub const CDIOCSTOP: u64 = _io(b'c', 23);
/// `CDIOCEJECT`.
pub const CDIOCEJECT: u64 = _io(b'c', 24);
/// `CDIOCALLOW`.
pub const CDIOCALLOW: u64 = _io(b'c', 25);
/// `CDIOCPREVENT`.
pub const CDIOCPREVENT: u64 = _io(b'c', 26);
/// `CDIOCCLOSE`.
pub const CDIOCCLOSE: u64 = _io(b'c', 27);

/// `struct ioc_play_msf`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocPlayMsf {
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
}

// SAFETY: `#[repr(C)]`, six bytes.
unsafe impl AbiPod for IocPlayMsf {}

/// `CDIOCPLAYMSF`.
pub const CDIOCPLAYMSF: u64 = _iow::<IocPlayMsf>(b'c', 25);

/// `CD_LU_ABORT`: these are the same as the ATAPI op values for the LOAD_UNLOAD command.
pub const CD_LU_ABORT: u8 = 0x1;
/// `CD_LU_UNLOAD`.
pub const CD_LU_UNLOAD: u8 = 0x2;
/// `CD_LU_LOAD`.
pub const CD_LU_LOAD: u8 = 0x3;

/// `struct ioc_load_unload`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IocLoadUnload {
    /// `options`: `CD_LU_*`.
    pub options: u8,
    /// `slot`.
    pub slot: u8,
}

// SAFETY: `#[repr(C)]`, two bytes.
unsafe impl AbiPod for IocLoadUnload {}

/// `CDIOCLOADUNLOAD`.
pub const CDIOCLOADUNLOAD: u64 = _iow::<IocLoadUnload>(b'c', 26);

// DVD definitions

/// `DVD_READ_STRUCT`: DVD-ROM specific ioctl.
pub const DVD_READ_STRUCT: u64 = _iowr::<DvdStruct>(b'd', 0);
/// `DVD_WRITE_STRUCT`.
pub const DVD_WRITE_STRUCT: u64 = _iowr::<DvdStruct>(b'd', 1);
/// `DVD_AUTH`.
pub const DVD_AUTH: u64 = _iowr::<DvdAuthinfo>(b'd', 2);

/// `GPCMD_READ_DVD_STRUCTURE`.
pub const GPCMD_READ_DVD_STRUCTURE: u8 = 0xad;
/// `GPCMD_SEND_DVD_STRUCTURE`.
pub const GPCMD_SEND_DVD_STRUCTURE: u8 = 0xad;
/// `GPCMD_REPORT_KEY`.
pub const GPCMD_REPORT_KEY: u8 = 0xa4;
/// `GPCMD_SEND_KEY`.
pub const GPCMD_SEND_KEY: u8 = 0xa3;

/// `DVD_STRUCT_PHYSICAL`: DVD struct types.
pub const DVD_STRUCT_PHYSICAL: u8 = 0x00;
/// `DVD_STRUCT_COPYRIGHT`.
pub const DVD_STRUCT_COPYRIGHT: u8 = 0x01;
/// `DVD_STRUCT_DISCKEY`.
pub const DVD_STRUCT_DISCKEY: u8 = 0x02;
/// `DVD_STRUCT_BCA`.
pub const DVD_STRUCT_BCA: u8 = 0x03;
/// `DVD_STRUCT_MANUFACT`.
pub const DVD_STRUCT_MANUFACT: u8 = 0x04;

/// `struct dvd_layer`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLayer {
    /// `book_version`.
    pub book_version: u8,
    /// `book_type`.
    pub book_type: u8,
    /// `min_rate`.
    pub min_rate: u8,
    /// `disc_size`.
    pub disc_size: u8,
    /// `layer_type`.
    pub layer_type: u8,
    /// `track_path`.
    pub track_path: u8,
    /// `nlayers`.
    pub nlayers: u8,
    /// `track_density`.
    pub track_density: u8,
    /// `linear_density`.
    pub linear_density: u8,
    /// `bca`.
    pub bca: u8,
    /// The C compiler's padding before `start_sector`.
    pub _pad0: [u8; 2],
    /// `start_sector`.
    pub start_sector: u32,
    /// `end_sector`.
    pub end_sector: u32,
    /// `end_sector_l0`.
    pub end_sector_l0: u32,
}

// SAFETY: `#[repr(C)]`, integers with the one hole named `_pad0` (24 bytes, no padding).
unsafe impl AbiPod for DvdLayer {}

/// `struct dvd_physical`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdPhysical {
    /// `type`.
    pub r#type: u8,
    /// `layer_num`.
    pub layer_num: u8,
    /// The C compiler's padding before `layer`.
    pub _pad0: [u8; 2],
    /// `layer`.
    pub layer: [DvdLayer; 4],
}

// SAFETY: `#[repr(C)]`, integers and `DvdLayer`s with the hole named `_pad0` (100 bytes).
unsafe impl AbiPod for DvdPhysical {}

/// `struct dvd_copyright`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdCopyright {
    /// `type`.
    pub r#type: u8,
    /// `layer_num`.
    pub layer_num: u8,
    /// `cpst`.
    pub cpst: u8,
    /// `rmi`.
    pub rmi: u8,
}

// SAFETY: `#[repr(C)]`, four bytes.
unsafe impl AbiPod for DvdCopyright {}

/// `struct dvd_disckey`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DvdDisckey {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `value`.
    pub value: [u8; 2048],
}

// SAFETY: `#[repr(C)]`, bytes (2050 bytes, no padding).
unsafe impl AbiPod for DvdDisckey {}

/// `struct dvd_bca`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DvdBca {
    /// `type`.
    pub r#type: u8,
    /// The C compiler's padding before `len`.
    pub _pad0: [u8; 3],
    /// `len`.
    pub len: i32,
    /// `value`.
    pub value: [u8; 188],
}

// SAFETY: `#[repr(C)]`, integers and bytes with the hole named `_pad0` (196 bytes).
unsafe impl AbiPod for DvdBca {}

/// `struct dvd_manufact`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DvdManufact {
    /// `type`.
    pub r#type: u8,
    /// `layer_num`.
    pub layer_num: u8,
    /// The C compiler's padding before `len`.
    pub _pad0: [u8; 2],
    /// `len`.
    pub len: i32,
    /// `value`.
    pub value: [u8; 2048],
}

// SAFETY: `#[repr(C)]`, integers and bytes with the hole named `_pad0` (2056 bytes).
unsafe impl AbiPod for DvdManufact {}

/// `union dvd_struct`: the argument of `DVD_READ_STRUCT`. Every member starts with `type`.
/// All members are plain integers and byte arrays: any bytes are a valid member.
#[repr(C)]
#[derive(Clone, Copy)]
pub union DvdStruct {
    /// `type`.
    pub r#type: u8,
    /// `physical`.
    pub physical: DvdPhysical,
    /// `copyright`.
    pub copyright: DvdCopyright,
    /// `disckey`.
    pub disckey: DvdDisckey,
    /// `bca`.
    pub bca: DvdBca,
    /// `manufact`.
    pub manufact: DvdManufact,
}

impl DvdStruct {
    /// All zeros (every byte of the largest member initialised).
    pub const fn zeroed() -> Self {
        Self {
            manufact: DvdManufact {
                r#type: 0,
                layer_num: 0,
                _pad0: [0; 2],
                len: 0,
                value: [0; 2048],
            },
        }
    }

    /// `s->type`.
    pub fn r#type(&self) -> u8 {
        // SAFETY: every member starts with the byte `type`, and all bytes are valid.
        unsafe { self.r#type }
    }

    /// `s->physical`.
    pub fn physical(&mut self) -> &mut DvdPhysical {
        // SAFETY: plain integers: any initialised bytes are a valid `DvdPhysical`; the union
        // is only built by `zeroed` or from bytes, so all bytes are initialised.
        unsafe { &mut self.physical }
    }

    /// `s->copyright`.
    pub fn copyright(&mut self) -> &mut DvdCopyright {
        // SAFETY: as in `physical`.
        unsafe { &mut self.copyright }
    }

    /// `s->disckey`.
    pub fn disckey(&mut self) -> &mut DvdDisckey {
        // SAFETY: as in `physical`.
        unsafe { &mut self.disckey }
    }

    /// `s->bca`.
    pub fn bca(&mut self) -> &mut DvdBca {
        // SAFETY: as in `physical`.
        unsafe { &mut self.bca }
    }

    /// `s->manufact`.
    pub fn manufact(&mut self) -> &mut DvdManufact {
        // SAFETY: as in `physical`.
        unsafe { &mut self.manufact }
    }
}

// SAFETY: `#[repr(C)]` union of `AbiPod` structures, 2056 bytes; `zeroed` and byte copies
// initialise all of them, and every pattern is valid for every member.
unsafe impl AbiPod for DvdStruct {}

// Authentication states

/// `DVD_LU_SEND_AGID`.
pub const DVD_LU_SEND_AGID: u8 = 0;
/// `DVD_HOST_SEND_CHALLENGE`.
pub const DVD_HOST_SEND_CHALLENGE: u8 = 1;
/// `DVD_LU_SEND_KEY1`.
pub const DVD_LU_SEND_KEY1: u8 = 2;
/// `DVD_LU_SEND_CHALLENGE`.
pub const DVD_LU_SEND_CHALLENGE: u8 = 3;
/// `DVD_HOST_SEND_KEY2`.
pub const DVD_HOST_SEND_KEY2: u8 = 4;

// Termination states

/// `DVD_AUTH_ESTABLISHED`.
pub const DVD_AUTH_ESTABLISHED: u8 = 5;
/// `DVD_AUTH_FAILURE`.
pub const DVD_AUTH_FAILURE: u8 = 6;

// Other functions

/// `DVD_LU_SEND_TITLE_KEY`.
pub const DVD_LU_SEND_TITLE_KEY: u8 = 7;
/// `DVD_LU_SEND_ASF`.
pub const DVD_LU_SEND_ASF: u8 = 8;
/// `DVD_INVALIDATE_AGID`.
pub const DVD_INVALIDATE_AGID: u8 = 9;
/// `DVD_LU_SEND_RPC_STATE`.
pub const DVD_LU_SEND_RPC_STATE: u8 = 10;
/// `DVD_HOST_SEND_RPC_STATE`.
pub const DVD_HOST_SEND_RPC_STATE: u8 = 11;

/// `DVD_KEY_SIZE`.
pub const DVD_KEY_SIZE: usize = 5;
/// `DVD_CHALLENGE_SIZE`.
pub const DVD_CHALLENGE_SIZE: usize = 10;

/// `struct dvd_lu_send_agid`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLuSendAgid {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
}

/// `struct dvd_host_send_challenge`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdHostSendChallenge {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `chal`.
    pub chal: [u8; DVD_CHALLENGE_SIZE],
}

/// `struct dvd_send_key`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdSendKey {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `key`.
    pub key: [u8; DVD_KEY_SIZE],
}

/// `struct dvd_lu_send_challenge`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLuSendChallenge {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `chal`.
    pub chal: [u8; DVD_CHALLENGE_SIZE],
}

/// `DVD_CPM_NO_COPYRIGHT`.
pub const DVD_CPM_NO_COPYRIGHT: u8 = 0;
/// `DVD_CPM_COPYRIGHTED`.
pub const DVD_CPM_COPYRIGHTED: u8 = 1;

/// `DVD_CP_SEC_NONE`.
pub const DVD_CP_SEC_NONE: u8 = 0;
/// `DVD_CP_SEC_EXIST`.
pub const DVD_CP_SEC_EXIST: u8 = 1;

/// `DVD_CGMS_UNRESTRICTED`.
pub const DVD_CGMS_UNRESTRICTED: u8 = 0;
/// `DVD_CGMS_SINGLE`.
pub const DVD_CGMS_SINGLE: u8 = 2;
/// `DVD_CGMS_RESTRICTED`.
pub const DVD_CGMS_RESTRICTED: u8 = 3;

/// `struct dvd_lu_send_title_key`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLuSendTitleKey {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `title_key`.
    pub title_key: [u8; DVD_KEY_SIZE],
    /// The C compiler's padding before `lba`.
    pub _pad0: u8,
    /// `lba`.
    pub lba: i32,
    /// `cpm`.
    pub cpm: u8,
    /// `cp_sec`.
    pub cp_sec: u8,
    /// `cgms`.
    pub cgms: u8,
    /// The C compiler's trailing padding.
    pub _pad1: u8,
}

/// `struct dvd_lu_send_asf`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLuSendAsf {
    /// `type`.
    pub r#type: u8,
    /// `agid`.
    pub agid: u8,
    /// `asf`.
    pub asf: u8,
}

/// `struct dvd_host_send_rpcstate`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdHostSendRpcstate {
    /// `type`.
    pub r#type: u8,
    /// `pdrc`.
    pub pdrc: u8,
}

/// `struct dvd_lu_send_rpcstate`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DvdLuSendRpcstate {
    /// `type`.
    pub r#type: u8,
    /// `vra`.
    pub vra: u8,
    /// `ucca`.
    pub ucca: u8,
    /// `region_mask`.
    pub region_mask: u8,
    /// `rpc_scheme`.
    pub rpc_scheme: u8,
}

/// `union dvd_authinfo`: the argument of `DVD_AUTH`. Every member starts with `type`. All
/// members are plain integers and byte arrays: any bytes are a valid member.
#[repr(C)]
#[derive(Clone, Copy)]
pub union DvdAuthinfo {
    /// `type`.
    pub r#type: u8,
    /// `lsa`.
    pub lsa: DvdLuSendAgid,
    /// `hsc`.
    pub hsc: DvdHostSendChallenge,
    /// `lsk`.
    pub lsk: DvdSendKey,
    /// `lsc`.
    pub lsc: DvdLuSendChallenge,
    /// `hsk`.
    pub hsk: DvdSendKey,
    /// `lstk`.
    pub lstk: DvdLuSendTitleKey,
    /// `lsasf`.
    pub lsasf: DvdLuSendAsf,
    /// `hrpcs`.
    pub hrpcs: DvdHostSendRpcstate,
    /// `lrpcs`.
    pub lrpcs: DvdLuSendRpcstate,
}

impl DvdAuthinfo {
    /// All zeros (every byte of the largest member initialised).
    pub const fn zeroed() -> Self {
        Self {
            lstk: DvdLuSendTitleKey {
                r#type: 0,
                agid: 0,
                title_key: [0; DVD_KEY_SIZE],
                _pad0: 0,
                lba: 0,
                cpm: 0,
                cp_sec: 0,
                cgms: 0,
                _pad1: 0,
            },
        }
    }

    /// `a->type`.
    pub fn r#type(&self) -> u8 {
        // SAFETY: every member starts with the byte `type`, and all bytes are valid.
        unsafe { self.r#type }
    }

    /// `a->type = v`.
    pub fn set_type(&mut self, v: u8) {
        self.r#type = v;
    }

    /// `a->lsa`.
    pub fn lsa(&mut self) -> &mut DvdLuSendAgid {
        // SAFETY: plain integers: any initialised bytes are a valid member; the union is
        // only built by `zeroed` or from bytes, so all bytes are initialised.
        unsafe { &mut self.lsa }
    }

    /// `a->hsc`.
    pub fn hsc(&mut self) -> &mut DvdHostSendChallenge {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.hsc }
    }

    /// `a->lsk`.
    pub fn lsk(&mut self) -> &mut DvdSendKey {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.lsk }
    }

    /// `a->lsc`.
    pub fn lsc(&mut self) -> &mut DvdLuSendChallenge {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.lsc }
    }

    /// `a->hsk`.
    pub fn hsk(&mut self) -> &mut DvdSendKey {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.hsk }
    }

    /// `a->lstk`.
    pub fn lstk(&mut self) -> &mut DvdLuSendTitleKey {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.lstk }
    }

    /// `a->lsasf`.
    pub fn lsasf(&mut self) -> &mut DvdLuSendAsf {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.lsasf }
    }

    /// `a->hrpcs`.
    pub fn hrpcs(&mut self) -> &mut DvdHostSendRpcstate {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.hrpcs }
    }

    /// `a->lrpcs`.
    pub fn lrpcs(&mut self) -> &mut DvdLuSendRpcstate {
        // SAFETY: as in `lsa`.
        unsafe { &mut self.lrpcs }
    }
}

// SAFETY: `#[repr(C)]` union of plain structures, 16 bytes with the holes named; `zeroed` and
// byte copies initialise all of them, and every pattern is valid for every member.
unsafe impl AbiPod for DvdAuthinfo {}

const _: () = {
    assert!(size_of::<MsfLba>() == 4);
    assert!(size_of::<CdTocEntry>() == 8);
    assert!(size_of::<CdSubChannelHeader>() == 4);
    assert!(size_of::<CdSubChannelQData>() == 44);
    assert!(size_of::<CdSubChannelPositionData>() == 12);
    assert!(size_of::<CdSubChannelMediaCatalog>() == 20);
    assert!(size_of::<CdSubChannelTrackInfo>() == 20);
    assert!(size_of::<CdSubChannelInfo>() == 48);
    assert!(size_of::<IocPlayTrack>() == 4);
    assert!(size_of::<IocPlayBlocks>() == 8);
    assert!(size_of::<IocReadSubchannel>() == 16);
    assert!(offset_of!(IocReadSubchannel, data_len) == 4);
    assert!(offset_of!(IocReadSubchannel, data) == 8);
    assert!(size_of::<IocTocHeader>() == 4);
    assert!(size_of::<IocReadTocEntry>() == 16);
    assert!(offset_of!(IocReadTocEntry, data_len) == 2);
    assert!(offset_of!(IocReadTocEntry, data) == 8);
    assert!(size_of::<IocPatch>() == 4);
    assert!(size_of::<IocVol>() == 4);
    assert!(size_of::<IocPlayMsf>() == 6);
    assert!(size_of::<IocLoadUnload>() == 2);
    assert!(size_of::<DvdLayer>() == 24);
    assert!(offset_of!(DvdLayer, start_sector) == 12);
    assert!(size_of::<DvdPhysical>() == 100);
    assert!(offset_of!(DvdPhysical, layer) == 4);
    assert!(size_of::<DvdCopyright>() == 4);
    assert!(size_of::<DvdDisckey>() == 2050);
    assert!(size_of::<DvdBca>() == 196);
    assert!(offset_of!(DvdBca, len) == 4);
    assert!(size_of::<DvdManufact>() == 2056);
    assert!(offset_of!(DvdManufact, len) == 4);
    assert!(size_of::<DvdStruct>() == 2056);
    assert!(size_of::<DvdLuSendAgid>() == 2);
    assert!(size_of::<DvdHostSendChallenge>() == 12);
    assert!(size_of::<DvdSendKey>() == 7);
    assert!(size_of::<DvdLuSendTitleKey>() == 16);
    assert!(offset_of!(DvdLuSendTitleKey, lba) == 8);
    assert!(size_of::<DvdLuSendRpcstate>() == 5);
    assert!(size_of::<DvdAuthinfo>() == 16);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdioreadmsaddr_is_an_inout_int_request() {
        // _IOWR('c', 6, int): IOC_INOUT | sizeof(int) << 16 | 'c' << 8 | 6.
        assert_eq!(CDIOREADMSADDR, 0xc004_6306);
    }

    #[test]
    fn ioctl_numbers_encode_the_argument_sizes() {
        assert_eq!(CDIOCPLAYTRACKS, 0x8004_6301);
        assert_eq!(CDIOCPLAYBLOCKS, 0x8008_6302);
        assert_eq!(CDIOCREADSUBCHANNEL, 0xc010_6303);
        assert_eq!(CDIOREADTOCHEADER, 0x4004_6304);
        assert_eq!(CDIOREADTOCENTRIES, 0xc010_6305);
        assert_eq!(CDIOCEJECT, 0x2000_6318);
        assert_eq!(CDIOCALLOW, 0x2000_6319);
        assert_eq!(CDIOCPLAYMSF, 0x8006_6319);
        assert_eq!(CDIOCLOADUNLOAD, 0x8002_631a);
        assert_eq!(DVD_READ_STRUCT, 0xc808_6400);
        assert_eq!(DVD_AUTH, 0xc010_6402);
    }

    #[test]
    fn toc_entry_nibbles_and_msf_views() {
        let mut e = CdTocEntry::default();
        e.set_control(4);
        e.set_addr_type(1);
        assert_eq!((e.control(), e.addr_type(), e.ac), (4, 1, 0x14));
        e.addr.set_lba(0x0102_0304);
        assert_eq!(e.addr.lba(), 0x0102_0304);
        let m = MsfLba {
            addr: [0, 12, 34, 56],
        };
        assert_eq!((m.minute(), m.second(), m.frame()), (12, 34, 56));
    }

    #[test]
    fn unions_start_with_the_type_byte() {
        let mut s = DvdStruct::zeroed();
        s.disckey().r#type = DVD_STRUCT_DISCKEY;
        assert_eq!(s.r#type(), DVD_STRUCT_DISCKEY);
        let mut a = DvdAuthinfo::zeroed();
        a.set_type(DVD_LU_SEND_AGID);
        a.lsa().agid = 3;
        assert_eq!(a.r#type(), DVD_LU_SEND_AGID);
        assert_eq!(a.lsa().agid, 3);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/cdio.h");
        crate::reftest::assert_defines!(defs;
            CD_AS_AUDIO_INVALID, CD_AS_PLAY_IN_PROGRESS, CD_AS_PLAY_PAUSED,
            CD_AS_PLAY_COMPLETED, CD_AS_PLAY_ERROR, CD_AS_NO_STATUS, CD_LBA_FORMAT,
            CD_MSF_FORMAT, CD_SUBQ_DATA, CD_CURRENT_POSITION, CD_MEDIA_CATALOG, CD_TRACK_INFO,
            CD_TRACK_LEADOUT, CD_LU_ABORT, CD_LU_UNLOAD, CD_LU_LOAD, GPCMD_READ_DVD_STRUCTURE,
            GPCMD_SEND_DVD_STRUCTURE, GPCMD_REPORT_KEY, GPCMD_SEND_KEY, DVD_STRUCT_PHYSICAL,
            DVD_STRUCT_COPYRIGHT, DVD_STRUCT_DISCKEY, DVD_STRUCT_BCA, DVD_STRUCT_MANUFACT,
            DVD_LU_SEND_AGID, DVD_HOST_SEND_CHALLENGE, DVD_LU_SEND_KEY1, DVD_LU_SEND_CHALLENGE,
            DVD_HOST_SEND_KEY2, DVD_AUTH_ESTABLISHED, DVD_AUTH_FAILURE, DVD_LU_SEND_TITLE_KEY,
            DVD_LU_SEND_ASF, DVD_INVALIDATE_AGID, DVD_LU_SEND_RPC_STATE,
            DVD_HOST_SEND_RPC_STATE, DVD_KEY_SIZE, DVD_CHALLENGE_SIZE, DVD_CPM_NO_COPYRIGHT,
            DVD_CPM_COPYRIGHTED, DVD_CP_SEC_NONE, DVD_CP_SEC_EXIST, DVD_CGMS_UNRESTRICTED,
            DVD_CGMS_SINGLE, DVD_CGMS_RESTRICTED,
        );
    }
}
/* </TESTS> */
