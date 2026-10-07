use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::net::if_::tests::{test_ifnet, zeroed_static};
use crate::sys::sockio::SIOCSIFADDR;

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/if_media.h");
    let ours = crate::reftest::assert_defines!(defs; IFM_ETHER, IFM_10_T, IFM_10_2, IFM_10_5, IFM_100_TX, IFM_100_FX, IFM_100_T4, IFM_100_VG, IFM_100_T2, IFM_1000_SX, IFM_10_STP, IFM_10_FL, IFM_1000_LX, IFM_1000_CX, IFM_1000_T, IFM_HPNA_1, IFM_10G_LR, IFM_10G_SR, IFM_10G_CX4, IFM_2500_SX, IFM_10G_T, IFM_10G_SFP_CU, IFM_10G_LRM, IFM_40G_CR4, IFM_40G_SR4, IFM_40G_LR4, IFM_1000_KX, IFM_10G_KX4, IFM_10G_KR, IFM_10G_CR1, IFM_20G_KR2, IFM_2500_KX, IFM_2500_T, IFM_5000_T, IFM_1000_SGMII, IFM_10G_SFI, IFM_40G_XLPPI, IFM_1000_CX_SGMII, IFM_40G_KR4, IFM_10G_ER, IFM_100G_CR4, IFM_100G_SR4, IFM_100G_KR4, IFM_100G_LR4, IFM_56G_R4, IFM_25G_CR, IFM_25G_KR, IFM_25G_SR, IFM_50G_CR2, IFM_50G_KR2, IFM_25G_LR, IFM_25G_ER, IFM_10G_AOC, IFM_25G_AOC, IFM_40G_AOC, IFM_100G_AOC, IFM_ETH_MASTER, IFM_ETH_RXPAUSE, IFM_ETH_TXPAUSE, IFM_FDDI, IFM_FDDI_SMF, IFM_FDDI_MMF, IFM_FDDI_UTP, IFM_FDDI_DA, IFM_IEEE80211, IFM_IEEE80211_FH1, IFM_IEEE80211_FH2, IFM_IEEE80211_DS2, IFM_IEEE80211_DS5, IFM_IEEE80211_DS11, IFM_IEEE80211_DS1, IFM_IEEE80211_DS22, IFM_IEEE80211_OFDM6, IFM_IEEE80211_OFDM9, IFM_IEEE80211_OFDM12, IFM_IEEE80211_OFDM18, IFM_IEEE80211_OFDM24, IFM_IEEE80211_OFDM36, IFM_IEEE80211_OFDM48, IFM_IEEE80211_OFDM54, IFM_IEEE80211_OFDM72, IFM_IEEE80211_HT_MCS0, IFM_IEEE80211_HT_MCS1, IFM_IEEE80211_HT_MCS2, IFM_IEEE80211_HT_MCS3, IFM_IEEE80211_HT_MCS4, IFM_IEEE80211_HT_MCS5, IFM_IEEE80211_HT_MCS6, IFM_IEEE80211_HT_MCS7, IFM_IEEE80211_HT_MCS8, IFM_IEEE80211_HT_MCS9, IFM_IEEE80211_HT_MCS10, IFM_IEEE80211_HT_MCS11, IFM_IEEE80211_HT_MCS12, IFM_IEEE80211_HT_MCS13, IFM_IEEE80211_HT_MCS14, IFM_IEEE80211_HT_MCS15, IFM_IEEE80211_HT_MCS16, IFM_IEEE80211_HT_MCS17, IFM_IEEE80211_HT_MCS18, IFM_IEEE80211_HT_MCS19, IFM_IEEE80211_HT_MCS20, IFM_IEEE80211_HT_MCS21, IFM_IEEE80211_HT_MCS22, IFM_IEEE80211_HT_MCS23, IFM_IEEE80211_HT_MCS24, IFM_IEEE80211_HT_MCS25, IFM_IEEE80211_HT_MCS26, IFM_IEEE80211_HT_MCS27, IFM_IEEE80211_HT_MCS28, IFM_IEEE80211_HT_MCS29, IFM_IEEE80211_HT_MCS30, IFM_IEEE80211_HT_MCS31, IFM_IEEE80211_HT_MCS32, IFM_IEEE80211_HT_MCS33, IFM_IEEE80211_HT_MCS34, IFM_IEEE80211_HT_MCS35, IFM_IEEE80211_HT_MCS36, IFM_IEEE80211_HT_MCS37, IFM_IEEE80211_HT_MCS38, IFM_IEEE80211_HT_MCS39, IFM_IEEE80211_HT_MCS40, IFM_IEEE80211_HT_MCS41, IFM_IEEE80211_HT_MCS42, IFM_IEEE80211_HT_MCS43, IFM_IEEE80211_HT_MCS44, IFM_IEEE80211_HT_MCS45, IFM_IEEE80211_HT_MCS46, IFM_IEEE80211_HT_MCS47, IFM_IEEE80211_HT_MCS48, IFM_IEEE80211_HT_MCS49, IFM_IEEE80211_HT_MCS50, IFM_IEEE80211_HT_MCS51, IFM_IEEE80211_HT_MCS52, IFM_IEEE80211_HT_MCS53, IFM_IEEE80211_HT_MCS54, IFM_IEEE80211_HT_MCS55, IFM_IEEE80211_HT_MCS56, IFM_IEEE80211_HT_MCS57, IFM_IEEE80211_HT_MCS58, IFM_IEEE80211_HT_MCS59, IFM_IEEE80211_HT_MCS60, IFM_IEEE80211_HT_MCS61, IFM_IEEE80211_HT_MCS62, IFM_IEEE80211_HT_MCS63, IFM_IEEE80211_HT_MCS64, IFM_IEEE80211_HT_MCS65, IFM_IEEE80211_HT_MCS66, IFM_IEEE80211_HT_MCS67, IFM_IEEE80211_HT_MCS68, IFM_IEEE80211_HT_MCS69, IFM_IEEE80211_HT_MCS70, IFM_IEEE80211_HT_MCS71, IFM_IEEE80211_HT_MCS72, IFM_IEEE80211_HT_MCS73, IFM_IEEE80211_HT_MCS74, IFM_IEEE80211_HT_MCS75, IFM_IEEE80211_HT_MCS76, IFM_IEEE80211_VHT_MCS0, IFM_IEEE80211_VHT_MCS1, IFM_IEEE80211_VHT_MCS2, IFM_IEEE80211_VHT_MCS3, IFM_IEEE80211_VHT_MCS4, IFM_IEEE80211_VHT_MCS5, IFM_IEEE80211_VHT_MCS6, IFM_IEEE80211_VHT_MCS7, IFM_IEEE80211_VHT_MCS8, IFM_IEEE80211_VHT_MCS9, IFM_IEEE80211_HE_MCS0, IFM_IEEE80211_HE_MCS1, IFM_IEEE80211_HE_MCS2, IFM_IEEE80211_HE_MCS3, IFM_IEEE80211_HE_MCS4, IFM_IEEE80211_HE_MCS5, IFM_IEEE80211_HE_MCS6, IFM_IEEE80211_HE_MCS7, IFM_IEEE80211_HE_MCS8, IFM_IEEE80211_HE_MCS9, IFM_IEEE80211_HE_MCS10, IFM_IEEE80211_HE_MCS11, IFM_IEEE80211_ADHOC, IFM_IEEE80211_HOSTAP, IFM_IEEE80211_IBSS, IFM_IEEE80211_IBSSMASTER, IFM_IEEE80211_MONITOR, IFM_IEEE80211_11A, IFM_IEEE80211_11B, IFM_IEEE80211_11G, IFM_IEEE80211_FH, IFM_IEEE80211_11N, IFM_IEEE80211_11AC, IFM_IEEE80211_11AX, IFM_TDM, IFM_TDM_T1, IFM_TDM_T1_AMI, IFM_TDM_E1, IFM_TDM_E1_G704, IFM_TDM_E1_AMI, IFM_TDM_E1_AMI_G704, IFM_TDM_T3, IFM_TDM_T3_M13, IFM_TDM_E3, IFM_TDM_E3_G751, IFM_TDM_E3_G832, IFM_TDM_E1_G704_CRC4, IFM_TDM_HDLC_CRC16, IFM_TDM_PPP, IFM_TDM_FR_ANSI, IFM_TDM_FR_CISCO, IFM_TDM_FR_ITU, IFM_TDM_MASTER, IFM_CARP, IFM_AUTO, IFM_MANUAL, IFM_NONE, IFM_FDX, IFM_HDX, IFM_FLOW, IFM_FLAG0, IFM_FLAG1, IFM_FLAG2, IFM_LOOP, IFM_NMASK, IFM_NSHIFT, IFM_TMASK, IFM_TSHIFT, IFM_IMASK, IFM_ISHIFT, IFM_OMASK, IFM_OSHIFT, IFM_MMASK, IFM_MSHIFT, IFM_GMASK, IFM_GSHIFT, IFM_AVALID, IFM_ACTIVE,
        IFM_1000_TX, IFM_ETH_FMASK, IFM_NMIN, IFM_NMAX, IFM_STATUS_VALID);
    let mut all = ours;
    // IFM_INST_ANY is `((uint64_t) -1)` and IFM_INST_MAX a macro call (checked above); the tables are below or left out (the deviations).
    all.extend([
        "IFM_INST_ANY",
        "IFM_INST_MAX",
        "IFM_BAUDRATE_DESCRIPTIONS",
        "IFM_TYPE_DESCRIPTIONS",
        "IFM_SUBTYPE_DESCRIPTIONS",
        "IFM_MODE_DESCRIPTIONS",
        "IFM_OPTION_DESCRIPTIONS",
        "IFM_STATUS_DESCRIPTIONS",
        "IFM_STATUS_VALID_LIST",
    ]);
    assert_eq!(IFM_INST_MAX, 0xff);
    crate::reftest::assert_complete(&defs, "IFM_", &all);
}

/// `IFM_BAUDRATE_DESCRIPTIONS` row by row against the header's text.
#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn the_baudrate_table_matches_the_c_header() {
    let path = crate::reftest::openbsd_src().join("sys/net/if_media.h");
    let text = std::fs::read_to_string(path).unwrap();
    let defs = crate::reftest::defines("sys/net/if_media.h");
    let start = text.find("#define\tIFM_BAUDRATE_DESCRIPTIONS").unwrap();
    let body = &text[start..];
    let body = &body[..body.find("{ 0, 0 }").unwrap()];
    let mut rows = std::vec::Vec::new();
    for line in body.lines().skip(1) {
        let line = line.trim().trim_end_matches('\\').trim();
        let Some(inner) = line.strip_prefix('{') else {
            continue;
        };
        let inner = inner.split('}').next().unwrap();
        let (word, rate) = inner.split_once(',').unwrap();
        let w: i64 = word
            .split('|')
            .map(|n| crate::reftest::int(&defs, n.trim()).unwrap())
            .fold(0, |a, b| a | b);
        let rate = rate.trim();
        let (unit, n) = rate.split_once('(').unwrap();
        let n: u64 = n.trim_end_matches(')').parse().unwrap();
        let r = match unit {
            "IF_Kbps" => if_kbps(n),
            "IF_Mbps" => if_mbps(n),
            "IF_Gbps" => if_gbps(n),
            u => panic!("unit {u}"),
        };
        rows.push((w as u64, r));
    }
    assert_eq!(rows.len() + 1, IFMEDIA_BAUDRATE_DESCRIPTIONS.len());
    for (i, (w, r)) in rows.iter().enumerate() {
        assert_eq!(IFMEDIA_BAUDRATE_DESCRIPTIONS[i].ifmb_word, *w, "row {i}");
        assert_eq!(
            IFMEDIA_BAUDRATE_DESCRIPTIONS[i].ifmb_baudrate, *r,
            "row {i}"
        );
    }
}

#[test]
fn media_word_macros() {
    let w = ifm_makeword(IFM_ETHER, IFM_100_TX, IFM_FDX, 3);
    assert_eq!(ifm_type(w), IFM_ETHER);
    assert_eq!(ifm_subtype(w), IFM_100_TX);
    assert_eq!(ifm_inst(w), 3);
    assert_eq!(ifm_options(w), IFM_FDX);
    assert_eq!(ifm_mode(w | IFM_IEEE80211_11G), IFM_IEEE80211_11G);
    assert!(ifm_type_match(IFM_AUTO, w));
    assert!(ifm_type_match(IFM_ETHER | IFM_10_T, w));
    assert!(!ifm_type_match(IFM_IEEE80211 | IFM_10_T, w));
}

#[test]
fn baudrate_of_known_and_unknown_words() {
    assert_eq!(
        ifmedia_baudrate(IFM_ETHER | IFM_100_TX | IFM_FDX),
        if_mbps(100)
    );
    assert_eq!(ifmedia_baudrate(IFM_ETHER | IFM_1000_T), if_mbps(1000));
    assert_eq!(
        ifmedia_baudrate(IFM_IEEE80211 | IFM_IEEE80211_HT_MCS0),
        if_kbps(6500)
    );
    assert_eq!(ifmedia_baudrate(IFM_ETHER | IFM_AUTO), 0);
    assert_eq!(ifmedia_baudrate(IFM_ETHER | IFM_100G_SR4), 0);
}

static CHANGES: AtomicU32 = AtomicU32::new(0);

fn change_ok(_ifp: &'static Ifnet) -> Result<(), Errno> {
    CHANGES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn change_fails(_ifp: &'static Ifnet) -> Result<(), Errno> {
    Err(Errno::EIO)
}

fn status(_ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    ifmr.ifm_status = IFM_AVALID | IFM_ACTIVE;
    ifmr.ifm_active |= IFM_FDX;
}

/// A zeroed `struct ifmedia` with the 10/100 copper words and autoselect, set to autoselect.
fn copper(change: IfmChangeCbT) -> &'static Ifmedia {
    // SAFETY: the all-zero `Ifmedia` is valid (`Cell`s of integers, null pointers, `None`s).
    let ifm: &'static Ifmedia = unsafe { zeroed_static() };
    ifmedia_init(ifm, 0, change, status);
    ifmedia_add(ifm, IFM_ETHER | IFM_10_T, 1, ptr::null_mut());
    ifmedia_add(ifm, IFM_ETHER | IFM_100_TX | IFM_FDX, 2, ptr::null_mut());
    ifmedia_add(ifm, IFM_ETHER | IFM_AUTO, 3, ptr::null_mut());
    ifmedia_set(ifm, IFM_ETHER | IFM_AUTO);
    ifm
}

#[test]
fn add_set_match_and_delete() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let ifm = copper(change_ok);
    assert_eq!(ifm.ifm_nwords(), 3);
    assert_eq!(ifm.ifm_cur().map(|e| e.ifm_data), Some(3));
    assert!(ifmedia_match(ifm, IFM_ETHER | IFM_100_TX | IFM_FDX, 0));
    assert!(!ifmedia_match(ifm, IFM_ETHER | IFM_100_TX, 0));
    assert!(ifmedia_match(ifm, IFM_ETHER | IFM_100_TX, IFM_FDX));
    let mut words = std::vec::Vec::new();
    ifm.for_each(|e| words.push(e.ifm_media));
    assert_eq!(
        words,
        [
            IFM_ETHER | IFM_10_T,
            IFM_ETHER | IFM_100_TX | IFM_FDX,
            IFM_ETHER | IFM_AUTO
        ]
    );

    // A word not on the list falls back to IFM_NONE, which is added.
    ifmedia_set(ifm, IFM_ETHER | IFM_1000_T);
    assert_eq!(ifm.ifm_nwords(), 4);
    assert_eq!(
        ifm.ifm_cur().map(|e| e.ifm_media),
        Some(IFM_ETHER | IFM_NONE)
    );

    ifmedia_add(
        ifm,
        ifm_makeword(IFM_ETHER, IFM_10_T, 0, 1),
        0,
        ptr::null_mut(),
    );
    ifmedia_delete_instance(ifm, 1);
    assert_eq!(ifm.ifm_nwords(), 4);
    assert!(ifm.ifm_cur().is_none());
    ifmedia_delete_instance(ifm, IFM_INST_ANY);
    assert_eq!(ifm.ifm_nwords(), 0);
    assert!(!ifmedia_match(ifm, IFM_ETHER | IFM_AUTO, 0));
}

#[test]
fn ioctl_sets_media_and_restores_on_failure() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let ifp = test_ifnet(b"tmedia0");
    let ifm = copper(change_ok);
    let mut ifr = Ifreq::zeroed();
    let before = CHANGES.load(Ordering::Relaxed);

    ifr.set_ifr_media(IFM_ETHER | IFM_100_TX | IFM_FDX);
    // SAFETY: SIOCSIFMEDIA with a `struct ifreq`.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifr).cast(), ifm, SIOCSIFMEDIA) };
    assert_eq!(r, Ok(()));
    assert_eq!(ifm.ifm_media.get(), IFM_ETHER | IFM_100_TX | IFM_FDX);
    assert_eq!(ifm.ifm_cur().map(|e| e.ifm_data), Some(2));
    assert!(CHANGES.load(Ordering::Relaxed) > before);

    ifr.set_ifr_media(IFM_ETHER | IFM_1000_T);
    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifr).cast(), ifm, SIOCSIFMEDIA) };
    assert_eq!(r, Err(Errno::EINVAL));

    let ifm2 = copper(change_fails);
    ifr.set_ifr_media(IFM_ETHER | IFM_10_T);
    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifr).cast(), ifm2, SIOCSIFMEDIA) };
    assert_eq!(r, Err(Errno::EIO));
    assert_eq!(ifm2.ifm_cur().map(|e| e.ifm_data), Some(3));
    assert_eq!(ifm2.ifm_media.get(), 0);

    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::null_mut(), ifm2, SIOCSIFMEDIA) };
    assert_eq!(r, Err(Errno::EINVAL));
}

#[test]
fn ioctl_reports_the_status_and_counts_words() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let ifp = test_ifnet(b"tmedia1");
    let ifm = copper(change_ok);
    // SAFETY: the all-zero `ifmediareq` is valid (integers and a null pointer).
    let mut ifmr: Ifmediareq = unsafe { core::mem::zeroed() };

    // SAFETY: SIOCGIFMEDIA with a `struct ifmediareq`.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifmr).cast(), ifm, SIOCGIFMEDIA) };
    assert_eq!(r, Ok(()));
    assert_eq!(ifmr.ifm_count, 3);
    assert_eq!(ifmr.ifm_current, IFM_ETHER | IFM_AUTO);
    assert_eq!(ifmr.ifm_active, IFM_ETHER | IFM_AUTO | IFM_FDX);
    assert_eq!(ifmr.ifm_status, IFM_AVALID | IFM_ACTIVE);

    ifmr.ifm_count = 2;
    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifmr).cast(), ifm, SIOCGIFMEDIA) };
    assert_eq!(r, Err(Errno::E2BIG));
    assert_eq!(ifmr.ifm_count, 3);

    ifmr.ifm_count = -1;
    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifmr).cast(), ifm, SIOCGIFMEDIA) };
    assert_eq!(r, Err(Errno::EINVAL));

    // SAFETY: as above.
    let r = unsafe { ifmedia_ioctl(ifp, ptr::from_mut(&mut ifmr).cast(), ifm, SIOCSIFADDR) };
    assert_eq!(r, Err(Errno::ENOTTY));
}
