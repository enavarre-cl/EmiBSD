//! `<sys/uuid.h>` for libsa: `struct uuid`.

/// `_UUID_NODE_LEN`.
pub const UUID_NODE_LEN: usize = 6;

/// `struct uuid`.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Uuid {
    /// `time_low`.
    pub time_low: u32,
    /// `time_mid`.
    pub time_mid: u16,
    /// `time_hi_and_version`.
    pub time_hi_and_version: u16,
    /// `clock_seq_hi_and_reserved`.
    pub clock_seq_hi_and_reserved: u8,
    /// `clock_seq_low`.
    pub clock_seq_low: u8,
    /// `node`.
    pub node: [u8; UUID_NODE_LEN],
}

const _: () = assert!(core::mem::size_of::<Uuid>() == 16);
