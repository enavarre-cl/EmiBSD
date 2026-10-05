use super::*;

#[test]
fn control_data_layout_matches_the_c() {
    // Three lists of 32 descriptors of two 32-bit words.
    assert_eq!(size_of::<AuichDmalist>(), 8);
    assert_eq!(size_of::<AuichCdata>(), 3 * 32 * 8);
    assert_eq!(auich_pcmo_off(0), 0);
    assert_eq!(auich_pcmo_off(31), 31 * 8);
    assert_eq!(auich_pcmi_off(0), 32 * 8);
    assert_eq!(auich_pcmi_off(1), 32 * 8 + 8);
    assert_eq!(auich_mici_off(0), 64 * 8);
    assert_eq!(auich_mici_off(31), 95 * 8);
    // The lists are what the BDBAR registers (8-byte aligned) point at.
    assert_eq!(auich_pcmi_off(0) % 8, 0);
    assert_eq!(AUICH_DMALIST_MAX * AUICH_DMASEG_MAX, 4 << 20);
}

#[test]
fn blocks_are_multiples_of_64_bytes_and_buffers_are_capped() {
    // SAFETY: neither function touches its handle.
    unsafe {
        let nul = ptr::null_mut();
        assert_eq!(auich_round_blocksize(nul, 1), 64);
        assert_eq!(auich_round_blocksize(nul, 64), 64);
        assert_eq!(auich_round_blocksize(nul, 65), 128);
        assert_eq!(auich_round_blocksize(nul, 960), 960);
        assert_eq!(auich_round_blocksize(nul, 961), 1024);

        assert_eq!(auich_round_buffersize(nul, AUMODE_PLAY, 65536), 65536);
        assert_eq!(
            auich_round_buffersize(nul, AUMODE_RECORD, 5 << 20),
            AUICH_DMALIST_MAX * AUICH_DMASEG_MAX
        );
    }
}

#[test]
fn the_device_table_is_the_c_one() {
    assert_eq!(AUICH_DEVICES.len(), 22);
    // QEMU's `-device AC97` is the 82801AA.
    let d = AUICH_DEVICES
        .iter()
        .rev()
        .find(|d| d.product == PCI_PRODUCT_INTEL_82801AA_ACA)
        .unwrap();
    assert_eq!((d.vendor, d.name), (PCI_VENDOR_INTEL, "ICH"));
    // The two nForce2 variants share a name; the SiS one is the special case.
    assert!(AUICH_DEVICES.iter().filter(|d| d.name == "nForce2").count() == 2);
    assert!(
        AUICH_DEVICES
            .iter()
            .any(|d| d.vendor == PCI_VENDOR_SIS && d.product == PCI_PRODUCT_SIS_7012_ACA)
    );
    // Intel's HDA controller (azalia) is not one of them.
    assert!(!AUICH_DEVICES.iter().any(|d| d.product == 0x2668));
}

#[test]
fn bit_descriptions_are_the_c_strings() {
    assert_eq!(AUICH_ISTS_BITS.first(), Some(&0x10));
    assert!(AUICH_ISTS_BITS.ends_with(b"fifoe"));
    assert!(AUICH_GSTS_BITS.ends_with(b"\x12md3"));
}

/// The bytes of a C string literal with octal escapes, as the file spells it.
fn c_string(text: &str) -> std::vec::Vec<u8> {
    let t = text.trim().trim_matches('"').as_bytes().to_vec();
    let mut out = std::vec::Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'\\' {
            let mut n = 0u32;
            let mut j = i + 1;
            while j < t.len() && j < i + 4 && (b'0'..=b'7').contains(&t[j]) {
                n = n * 8 + u32::from(t[j] - b'0');
                j += 1;
            }
            out.push(n as u8);
            i = j;
        } else {
            out.push(t[i]);
            i += 1;
        }
    }
    out
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_file() {
    let defs = crate::reftest::defines("sys/dev/pci/auich.c");
    let mut ours = crate::reftest::assert_defines!(defs;
        AUICH_NAMBAR, AUICH_NABMBAR, AUICH_CFG, AUICH_CFG_IOSE, AUICH_MMBAR, AUICH_MBBAR,
        AUICH_S2CR, AUICH_BDBAR, AUICH_CIV, AUICH_LVI, AUICH_LVI_MASK, AUICH_STS, AUICH_FIFOE,
        AUICH_BCIS, AUICH_LVBCI, AUICH_CELV, AUICH_DCH, AUICH_PICB, AUICH_PIV, AUICH_CTRL,
        AUICH_IOCE, AUICH_FEIE, AUICH_LVBIE, AUICH_RR, AUICH_RPBM, AUICH_PCMI, AUICH_PCMO,
        AUICH_MICI, AUICH_GCTRL, AUICH_SSM_78, AUICH_SSM_69, AUICH_SSM_1011, AUICH_POM16,
        AUICH_POM20, AUICH_PCM246_MASK, AUICH_PCM2, AUICH_PCM4, AUICH_PCM6,
        AUICH_SIS_PCM246_MASK, AUICH_SIS_PCM2, AUICH_SIS_PCM4, AUICH_SIS_PCM6, AUICH_S2RIE,
        AUICH_SRIE, AUICH_PRIE, AUICH_ACLSO, AUICH_WRESET, AUICH_CRESET, AUICH_GIE, AUICH_GSTS,
        AUICH_MD3, AUICH_AD3, AUICH_RCS, AUICH_B3S12, AUICH_B2S12, AUICH_B1S12, AUICH_SRI,
        AUICH_PRI, AUICH_SCR, AUICH_PCR, AUICH_MINT, AUICH_POINT, AUICH_PIINT, AUICH_MOINT,
        AUICH_MIINT, AUICH_GSCI, AUICH_CAS, AUICH_SEMATIMO, AUICH_RESETIMO, ICH_SIS_NV_CTL,
        ICH_SIS_CTL_UNMUTE, AUICH_DMALIST_MAX, AUICH_DMAF_IOC, AUICH_DMAF_BUP,
        AUICH_FIXED_RATE);

    // `AUICH_DMASEG_MAX` is `(65536*2)`, parsed with the operator the reader knows.
    assert_eq!(
        crate::reftest::int(&defs, "AUICH_DMASEG_MAX"),
        Some(AUICH_DMASEG_MAX as i64)
    );
    assert_eq!(c_string(&defs["AUICH_ISTS_BITS"]), AUICH_ISTS_BITS);
    assert_eq!(c_string(&defs["AUICH_GSTS_BITS"]), AUICH_GSTS_BITS);
    // The debug masks belong to `AUICH_DEBUG`, which is not configured.
    ours.extend([
        "AUICH_DMASEG_MAX",
        "AUICH_ISTS_BITS",
        "AUICH_GSTS_BITS",
        "AUICH_DEBUG_CODECIO",
        "AUICH_DEBUG_DMA",
        "AUICH_DEBUG_INTR",
    ]);
    crate::reftest::assert_complete(&defs, "AUICH_", &ours);
    crate::reftest::assert_complete(&defs, "ICH_", &ours);
}
