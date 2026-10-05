use super::*;

#[test]
fn capability_and_configuration_fields() {
    // CAP of QEMU's nvme: MQES 2047, CQR, TO 15 (7.5 s), DSTRD 0, CSS NVM, MPSMIN 0,
    // MPSMAX 4.
    let cap: u64 = 0x0040_0020_0f01_07ff;
    assert_eq!(nvme_cap_mqes(cap), 2048);
    assert!(nvme_cap_cqr(cap));
    assert_eq!(nvme_cap_to(cap), 7500);
    assert_eq!(nvme_cap_dstrd(cap), 4);
    assert_eq!(nvme_cap_css(cap) & NVME_CAP_CSS_NVM, NVME_CAP_CSS_NVM);
    assert_eq!(nvme_cap_mpsmin(cap), 12);
    assert_eq!(nvme_cap_mpsmax(cap), 16);
    assert!(!nvme_cap_nssrs(cap));

    let cc = nvme_cc_iosqes(6) | nvme_cc_iocqes(4) | nvme_cc_mps(12) | NVME_CC_EN;
    assert_eq!(cc, 0x0046_0001);
    assert_eq!(nvme_cc_iosqes_r(cc), 6);
    assert_eq!(nvme_cc_iocqes_r(cc), 4);
    assert_eq!(nvme_cc_mps_r(cc), 12);
    assert_eq!(nvme_aqa_acqs(128) | nvme_aqa_asqs(128), 0x007f_007f);
    assert_eq!(nvme_vs_mjr(0x0001_0400), 1);
    assert_eq!(nvme_vs_mnr(0x0001_0400), 4);
}

#[test]
fn doorbells_and_status() {
    assert_eq!(nvme_sqtdbl(NVME_ADMIN_Q, 4), 0x1000);
    assert_eq!(nvme_cqhdbl(NVME_ADMIN_Q, 4), 0x1004);
    assert_eq!(nvme_sqtdbl(1, 4), 0x1008);
    assert_eq!(nvme_cqhdbl(1, 4), 0x100c);
    assert_eq!(nvme_cqe_sc(0x8001 | (0x02 << 1)), NVME_CQE_SC_INVALID_FIELD);
    assert_eq!(nvme_cqe_sct(0x0201), NVME_CQE_SCT_COMMAND);
}

#[test]
fn entry_views_share_the_bytes() {
    let mut sqe = NvmeSqe::zeroed();
    sqe.as_q_mut().qid = 1;
    sqe.as_q_mut().qflags = NVM_SQE_CQ_IEN | NVM_SQE_Q_PC;
    assert_eq!(sqe.cdw10 & 0xffff, 1);
    assert_eq!(sqe.cdw11 & 0xff, 3);
    sqe.as_io_mut().slba = 0x1234;
    assert_eq!(sqe.cdw10, 0x1234);
    sqe.entry.set_prp(1, 0xdead_0000);
    assert_eq!(sqe.as_io_mut().entry.prp(1), 0xdead_0000);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/ic/nvmereg.h");
    let names = crate::reftest::assert_defines!(defs;
        NVME_CAP, NVME_CAP_LO, NVME_CAP_HI, NVME_VS, NVME_INTMS, NVME_INTMC, NVME_CC,
        NVME_CSTS, NVME_NSSR, NVME_AQA, NVME_ASQ, NVME_ACQ, NVME_ADMIN_Q,
        NVME_CC_SHN_NONE, NVME_CC_SHN_NORMAL, NVME_CC_SHN_ABRUPT, NVME_CC_AMS_RR,
        NVME_CC_AMS_WRR_U, NVME_CC_AMS_VENDOR, NVME_CC_MPS_MASK, NVME_CC_CSS_NVM,
        NVME_CC_EN, NVME_CSTS_SHST_MASK, NVME_CSTS_SHST_NONE, NVME_CSTS_SHST_WAIT,
        NVME_CSTS_SHST_DONE, NVME_CSTS_CFS, NVME_CSTS_RDY, NVME_CAP_CSS_NVM,
        NVME_CAP_AMS_WRR, NVME_CAP_AMS_VENDOR,
        NVM_SQE_SQ_QPRIO_URG, NVM_SQE_SQ_QPRIO_HI, NVM_SQE_SQ_QPRIO_MED,
        NVM_SQE_SQ_QPRIO_LOW, NVM_SQE_CQ_IEN, NVM_SQE_Q_PC,
        NVME_CQE_DNR, NVME_CQE_M, NVME_CQE_SCT_GENERIC, NVME_CQE_SCT_COMMAND,
        NVME_CQE_SCT_MEDIAERR, NVME_CQE_SCT_VENDOR, NVME_CQE_SC_SUCCESS,
        NVME_CQE_SC_INVALID_OPCODE, NVME_CQE_SC_INVALID_FIELD, NVME_CQE_SC_CID_CONFLICT,
        NVME_CQE_SC_DATA_XFER_ERR, NVME_CQE_SC_ABRT_BY_NO_PWR,
        NVME_CQE_SC_INTERNAL_DEV_ERR, NVME_CQE_SC_CMD_ABRT_REQD,
        NVME_CQE_SC_CMD_ABDR_SQ_DEL, NVME_CQE_SC_CMD_ABDR_FUSE_ERR,
        NVME_CQE_SC_CMD_ABDR_FUSE_MISS, NVME_CQE_SC_INVALID_NS, NVME_CQE_SC_CMD_SEQ_ERR,
        NVME_CQE_SC_INVALID_LAST_SGL, NVME_CQE_SC_INVALID_NUM_SGL,
        NVME_CQE_SC_DATA_SGL_LEN, NVME_CQE_SC_MDATA_SGL_LEN,
        NVME_CQE_SC_SGL_TYPE_INVALID, NVME_CQE_SC_LBA_RANGE, NVME_CQE_SC_CAP_EXCEEDED,
        NVME_CQE_NS_NOT_RDY, NVME_CQE_RSV_CONFLICT, NVME_CQE_PHASE,
        NVM_ADMIN_DEL_IOSQ, NVM_ADMIN_ADD_IOSQ, NVM_ADMIN_GET_LOG_PG, NVM_ADMIN_DEL_IOCQ,
        NVM_ADMIN_ADD_IOCQ, NVM_ADMIN_IDENTIFY, NVM_ADMIN_ABORT, NVM_ADMIN_SET_FEATURES,
        NVM_ADMIN_GET_FEATURES, NVM_ADMIN_ASYNC_EV_REQ, NVM_ADMIN_FW_ACTIVATE,
        NVM_ADMIN_FW_DOWNLOAD, NVM_ADMIN_SELFTEST,
        NVM_CMD_FLUSH, NVM_CMD_WRITE, NVM_CMD_READ, NVM_CMD_WR_UNCOR, NVM_CMD_COMPARE,
        NVM_CMD_DSM, NVM_ID_CTRL_LPA_PE, NVM_ID_CTRL_FNA_CRYPTOFORMAT,
        NVM_ID_CTRL_VWC_PRESENT, NVME_ID_NS_NSFEAT_THIN_PROV, NVME_ID_NS_FLBAS_MD,
        NVME_ID_NS_DPS_PIP, NVM_LOG_PAGE_SMART_HEALTH,
        NVM_HEALTH_CW_SPARE, NVM_HEALTH_CW_TEMP, NVM_HEALTH_CW_MEDIA,
        NVM_HEALTH_CW_READONLY, NVM_HEALTH_CW_VOLATILE, NVM_HEALTH_CW_PMR,
    );
    // The function-like macros and the `%b` strings are not simple defines.
    let mut names = names;
    names.extend([
        "NVME_CC_IOCQES_MASK",
        "NVME_CC_IOSQES_MASK",
        "NVME_CC_SHN_MASK",
        "NVME_CC_AMS_MASK",
        "NVME_CC_CSS_MASK",
        "NVM_ID_CTRL_CTRATT_FMT",
        "NVM_ID_CTRL_OACS_FMT",
        "NVM_ID_CTRL_SANICAP_FMT",
        "NVM_ID_CTRL_ONCS_FMT",
        "NVME_ID_NS_NSFEAT_FMT",
    ]);
    crate::reftest::assert_complete(&defs, "NVM", &names);
}
