/* <CODE> */
//! `<sys/hibernate.h>` for the boot programs: the signature `check_hibernate` looks for.

/// `HIBERNATE_MAGIC`: the first word of `union hibernate_info` in a valid signature block.
pub const HIBERNATE_MAGIC: u32 = 0x0B5D_0B5D;
/* </CODE> */
