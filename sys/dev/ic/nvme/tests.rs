use std::boxed::Box;

use super::*;
use crate::dev::ic::nvmereg::NvmNamespaceFormat;

/// A zeroed namespace identify page on the heap (4 KiB).
fn namespace() -> Box<NvmIdentifyNamespace> {
    // SAFETY: `NvmIdentifyNamespace` is integers and arrays of them: all-zero is valid.
    Box::new(unsafe { core::mem::zeroed() })
}

#[test]
fn size_is_capacity_only_when_thin_and_smaller() {
    let mut ns = namespace();
    ns.nsze = 131_072u64.to_le();
    ns.ncap = 65_536u64.to_le();
    assert_eq!(nvme_scsi_size(&ns), 131_072);
    ns.nsfeat = NVME_ID_NS_NSFEAT_THIN_PROV;
    assert_eq!(nvme_scsi_size(&ns), 65_536);
    ns.ncap = 262_144u64.to_le();
    assert_eq!(nvme_scsi_size(&ns), 131_072);
}

#[test]
fn read_capacity_replies() {
    let mut ns = namespace();
    ns.nsze = 131_072u64.to_le();
    ns.ncap = ns.nsze;
    ns.flbas = 1;
    ns.lbaf[1] = NvmNamespaceFormat {
        ms: 0,
        lbads: 12,
        rp: 0,
    };
    let rcd = nvme_read_cap_data(&ns);
    assert_eq!(rcd.addr, [0x00, 0x01, 0xff, 0xff]);
    assert_eq!(rcd.length, [0, 0, 0x10, 0]);
    let rcd16 = nvme_read_cap_data_16(&ns);
    assert_eq!(rcd16.addr, [0, 0, 0, 0, 0, 0x01, 0xff, 0xff]);
    assert_eq!(rcd16.length, [0, 0, 0x10, 0]);
    assert_eq!(rcd16.lowest_aligned, [0x80, 0x00]);

    // More than 2^32 blocks: READ CAPACITY (10) says 0xffffffff.
    ns.nsze = (1u64 << 33).to_le();
    assert_eq!(nvme_read_cap_data(&ns).addr, [0xff; 4]);
}

#[test]
fn inquiry_names_the_model_and_firmware() {
    let mut mn = [b' '; 40];
    mn[..14].copy_from_slice(b"QEMU NVMe Ctrl");
    let fr = *b"11.1.0  ";
    let inq = nvme_inquiry_data(&mn, &fr);
    assert_eq!(inq.device, T_DIRECT);
    assert_eq!(inq.version, SCSI_REV_SPC4);
    assert_eq!(&inq.vendor, b"NVMe    ");
    assert_eq!(&inq.product, b"QEMU NVMe Ctrl  ");
    assert_eq!(&inq.revision, b"11.1");
    assert_eq!(inq.flags & SID_CmdQue, SID_CmdQue);
}

#[test]
fn formatted_lba_size_index() {
    let mut ns = namespace();
    ns.flbas = 0x23;
    ns.nlbaf = 3;
    assert_eq!(nvme_ns_lbaf(&ns, 16), 3);
    ns.nlbaf = 17;
    assert_eq!(nvme_ns_lbaf(&ns, 16), 3 | 0x11);
    ns.lbaf[3].lbads = 9;
    assert_eq!(nvme_ns_lbads(&ns, 3), 9);
    assert_eq!(nvme_ns_lbads(&ns, 40), 0);
}

#[test]
fn poll_state_marks_completion() {
    let mut st = NvmePollState {
        s: NvmeSqe::zeroed(),
        c: NvmeCqe::default(),
    };
    st.s.opcode = NVM_ADMIN_IDENTIFY;
    assert_eq!(st.c.flags & NVME_CQE_PHASE, 0);
    st.c.flags = (NVME_CQE_SC_SUCCESS) | NVME_CQE_PHASE;
    assert_eq!(st.c.flags & !NVME_CQE_PHASE, 0);
}

#[test]
fn sqe_write_zeroes_a_long_slot() {
    let mut slot = [0xffu64; 16]; // a 128-byte slot (Apple T2)
    let mut sqe = NvmeSqe::zeroed();
    sqe.opcode = NVM_CMD_READ;
    sqe.cid = 7;
    // SAFETY: a 128-byte, 8-aligned buffer of our own.
    unsafe { nvme_sqe_write(slot.as_mut_ptr().cast(), 128, &sqe) };
    assert_eq!(slot[0] & 0xff, u64::from(NVM_CMD_READ));
    assert_eq!((slot[0] >> 16) & 0xffff, 7);
    assert!(slot[8..].iter().all(|&w| w == 0));
}
