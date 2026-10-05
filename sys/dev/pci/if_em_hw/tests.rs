use core::ptr::NonNull;

use super::*;
use crate::dev::pci::pci_map::tests::attach_args;
use crate::machine::bus::{BusSpaceTag, bus_space_map};

fn no_mwi(_: &EmHw) {}

fn no_cfg_read(_: &EmHw, _: u32, v: &mut u16) {
    *v = 0;
}

fn no_cfg_write(_: &EmHw, _: u32, _: &u16) {}

fn no_cap(_: &EmHw, _: u32, _: &mut u16) -> Result<(), i32> {
    Err(-E1000_NOT_IMPLEMENTED)
}

/// What if_em.c will pass: the C's `em_read_pcie_cap_reg` is "not implemented".
static TEST_PCI_OPS: EmPciOps = EmPciOps {
    em_pci_set_mwi: no_mwi,
    em_pci_clear_mwi: no_mwi,
    em_read_pci_cfg: no_cfg_read,
    em_write_pci_cfg: no_cfg_write,
    em_read_pcie_cap_reg: no_cap,
};

/// A softc's osdep over the host's fake bus space (every register reads 0).
fn osdep() -> Box<EmOsdep> {
    let t = BusSpaceTag::default();
    // SAFETY: the host bus space maps nothing; its handles only carry the address.
    let h = unsafe { bus_space_map(t, 0, 0x20000, 0) }.expect("host map");
    Box::new(EmOsdep {
        mem_bus_space_tag: t,
        mem_bus_space_handle: h,
        io_bus_space_tag: t,
        io_bus_space_handle: h,
        flash_bus_space_tag: t,
        flash_bus_space_handle: h,
        dev: None,
        em_pa: attach_args(0, 3, 0),
        em_memsize: 0x20000,
        em_membase: 0,
        em_iosize: 0,
        em_iobase: 0,
        em_flashsize: 0,
        em_flashbase: 0,
        em_flashoffset: 0,
    })
}

/// An `EmHw` for `device_id`/`revision_id`, its osdep kept alive beside it.
fn hw_for(device_id: u16, revision_id: u8) -> (Box<EmOsdep>, EmHw) {
    let mut o = osdep();
    let back = NonNull::from(&mut *o);
    // SAFETY: the osdep is boxed, lives as long as the returned pair, and is never written.
    let mut hw = unsafe { EmHw::new(back, &TEST_PCI_OPS) };
    hw.device_id = device_id;
    hw.revision_id = revision_id;
    (o, hw)
}

#[test]
fn mac_type_from_device_id() {
    for (id, rev, mac) in [
        (E1000_DEV_ID_82540EM, 0, em_82540),
        (E1000_DEV_ID_82574L, 0, em_82574),
        (E1000_DEV_ID_82583V, 0, em_82574),
        (E1000_DEV_ID_82576, 0, em_82576),
        (E1000_DEV_ID_82542, E1000_82542_2_0_REV_ID, em_82542_rev2_0),
        (E1000_DEV_ID_82542, E1000_82542_2_1_REV_ID, em_82542_rev2_1),
        (E1000_DEV_ID_I210_COPPER, 0, em_i210),
        (E1000_DEV_ID_PCH_SPT_I219_LM, 0, em_pch_spt),
        (E1000_DEV_ID_EP80579_LAN_5, 0, em_icp_xxxx),
    ] {
        let (_o, mut hw) = hw_for(id, rev);
        assert_eq!(em_set_mac_type(&mut hw), Ok(()), "{id:#x}");
        assert_eq!(hw.mac_type, mac, "{id:#x}");
    }
    let (_o, mut hw) = hw_for(E1000_DEV_ID_82542, 7);
    assert_eq!(em_set_mac_type(&mut hw), Err(-E1000_ERR_MAC_TYPE));
    let (_o, mut hw) = hw_for(0xffff, 0);
    assert_eq!(em_set_mac_type(&mut hw), Err(-E1000_ERR_MAC_TYPE));
}

#[test]
fn mac_type_semaphore_flags() {
    // The C's fall-through: 82576 has all three, 82574 the EEPROM semaphore and ASF.
    let (_o, mut hw) = hw_for(E1000_DEV_ID_82576, 0);
    em_set_mac_type(&mut hw).expect("82576");
    assert_eq!(
        (
            hw.swfw_sync_present,
            hw.eeprom_semaphore_present,
            hw.asf_firmware_present
        ),
        (1, 1, 1)
    );
    assert!(hw.initialize_hw_bits_disable);
    let (_o, mut hw) = hw_for(E1000_DEV_ID_82574L, 0);
    em_set_mac_type(&mut hw).expect("82574");
    assert_eq!(
        (
            hw.swfw_sync_present,
            hw.eeprom_semaphore_present,
            hw.asf_firmware_present
        ),
        (0, 1, 1)
    );
    let (_o, mut hw) = hw_for(E1000_DEV_ID_82540EM, 0);
    em_set_mac_type(&mut hw).expect("82540");
    assert_eq!(
        (
            hw.swfw_sync_present,
            hw.eeprom_semaphore_present,
            hw.asf_firmware_present
        ),
        (0, 0, 0)
    );
    let (_o, mut hw) = hw_for(E1000_DEV_ID_ICH9_BM, 0);
    em_set_mac_type(&mut hw).expect("ich9");
    assert_eq!(
        (hw.swfwhw_semaphore_present, hw.asf_firmware_present),
        (1, 1)
    );
    assert!(is_ich8(hw.mac_type));
}

#[test]
fn multicast_hash() {
    // The C's comment example: 01 AA 00 12 34 56.
    let addr = [0x01, 0xAA, 0x00, 0x12, 0x34, 0x56];
    let (_o, mut hw) = hw_for(E1000_DEV_ID_82540EM, 0);
    hw.mac_type = em_82540;
    // Type 2 is 0x58D: the C comment's 0x5D8 is a typo of its own code, `(0x34 >> 2) | (0x56 << 6)`.
    for (t, v) in [(0, 0x563), (1, 0xAC6), (2, 0x58D), (3, 0x634)] {
        hw.mc_filter_type = t;
        assert_eq!(em_hash_mc_addr(&hw, &addr), v, "type {t}");
    }
    hw.mac_type = em_ich8lan;
    for (t, v) in [(0, 0x158), (1, 0x2B1), (2, 0x163), (3, 0x18D)] {
        hw.mc_filter_type = t;
        assert_eq!(em_hash_mc_addr(&hw, &addr), v, "ich8 type {t}");
    }
}

#[test]
fn translate_82542() {
    assert_eq!(em_translate_82542_register(e1000_rdbal(0)), 0x00110);
    assert_eq!(em_translate_82542_register(e1000_rdt(1)), 0x00150);
    assert_eq!(em_translate_82542_register(e1000_tdt(0)), 0x00438);
    assert_eq!(em_translate_82542_register(E1000_MTA), 0x00200);
    assert_eq!(em_translate_82542_register(E1000_CTRL), E1000_CTRL);
    assert_eq!(em_translate_82542_register(e1000_rdbal(4)), e1000_rdbal(4));
}

#[test]
fn register_macros() {
    assert_eq!(e1000_rdbal(0), 0x02800);
    assert_eq!(e1000_rdbal(4), 0x0C000 + 4 * 0x40);
    assert_eq!(e1000_txdctl(1), 0x03928);
    assert_eq!(e1000_byte_swap_word(0x1234), 0x3412);
    assert_eq!(nvm_82580_lan_func_offset(0), 0);
    assert_eq!(nvm_82580_lan_func_offset(2), 0xC0);
    assert_eq!(eeprom_ia_start_icp_xxxx(1), 0x22);
    // BM PHY addresses: page and register come back out.
    let r = bm_phy_reg(BM_WUC_PAGE, 17);
    assert_eq!(u32::from(bm_phy_reg_page(r)), BM_WUC_PAGE);
    assert_eq!(bm_phy_reg_num(r), 17);
    assert_eq!(phy_reg(769, 17), (769 << PHY_PAGE_SHIFT) | 17);
    assert_eq!(gg82563_reg(194, 18), GG82563_PHY_INBAND_CTRL);
}

#[test]
fn mng_checksum_and_cookie() {
    let bytes = [1u8, 2, 3, 250, 7];
    let c = em_calculate_mng_checksum(&bytes);
    let sum = bytes.iter().fold(c, |a, b| a.wrapping_add(*b));
    assert_eq!(sum, 0);
    let mut b = [0u8; 16];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8 * 3 + 1;
    }
    let cookie = EmHostMngDhcpCookie::from_bytes(&b);
    assert_eq!(cookie.signature, u32::from_le_bytes([1, 4, 7, 10]));
    assert_eq!(cookie.checksum, 46);
    assert_eq!(cookie.to_bytes(), b);
}

#[test]
fn flash_register_fields() {
    let mut s = Ich8HwsFlashStatus { regval: 0x4000 };
    assert_eq!(s.fldesvalid(), 1);
    s.set_flcerr(1);
    s.set_dael(1);
    assert_eq!(s.regval, 0x4006);
    s.set_berasesz(3);
    assert_eq!(s.berasesz(), 3);
    let mut c = Ich8HwsFlashCtrl::default();
    c.set_fldbcount(1);
    c.set_flcycle(ICH_CYCLE_ERASE as u16);
    c.set_flcgo(1);
    assert_eq!(c.regval, 0x0100 | (3 << 1) | 1);
    let f = SfpE1000Flags { regval: 0x08 };
    assert_eq!((f.e1000_base_t(), f.e1000_base_sx()), (1, 0));
}

#[test]
fn bus_width_from_pcie() {
    assert_eq!(EmBusWidth::from_pcie_link_width(1), em_bus_width_pciex_1);
    assert_eq!(EmBusWidth::from_pcie_link_width(4), em_bus_width_pciex_4);
    assert_eq!(EmBusWidth::from_pcie_link_width(8), em_bus_width_reserved);
}

/// Every simple `#define` of `if_em_hw.h`, with its Rust value.
fn header_defines() -> std::vec::Vec<(&'static str, i64)> {
    std::vec![
        ("E1000_HOST_IF_MAX_SIZE", E1000_HOST_IF_MAX_SIZE as i64),
        ("E1000_SUCCESS", E1000_SUCCESS as i64),
        ("E1000_ERR_EEPROM", E1000_ERR_EEPROM as i64),
        ("E1000_ERR_PHY", E1000_ERR_PHY as i64),
        ("E1000_ERR_CONFIG", E1000_ERR_CONFIG as i64),
        ("E1000_ERR_PARAM", E1000_ERR_PARAM as i64),
        ("E1000_ERR_MAC_TYPE", E1000_ERR_MAC_TYPE as i64),
        ("E1000_ERR_PHY_TYPE", E1000_ERR_PHY_TYPE as i64),
        ("E1000_ERR_RESET", E1000_ERR_RESET as i64),
        (
            "E1000_ERR_MASTER_REQUESTS_PENDING",
            E1000_ERR_MASTER_REQUESTS_PENDING as i64
        ),
        (
            "E1000_ERR_HOST_INTERFACE_COMMAND",
            E1000_ERR_HOST_INTERFACE_COMMAND as i64
        ),
        ("E1000_BLK_PHY_RESET", E1000_BLK_PHY_RESET as i64),
        ("E1000_ERR_SWFW_SYNC", E1000_ERR_SWFW_SYNC as i64),
        ("E1000_NOT_IMPLEMENTED", E1000_NOT_IMPLEMENTED as i64),
        ("E1000_DEFER_INIT", E1000_DEFER_INIT as i64),
        (
            "E1000_MNG_DHCP_TX_PAYLOAD_CMD",
            E1000_MNG_DHCP_TX_PAYLOAD_CMD as i64
        ),
        (
            "E1000_HI_MAX_MNG_DATA_LENGTH",
            E1000_HI_MAX_MNG_DATA_LENGTH as i64
        ),
        (
            "E1000_MNG_DHCP_COMMAND_TIMEOUT",
            E1000_MNG_DHCP_COMMAND_TIMEOUT as i64
        ),
        (
            "E1000_MNG_DHCP_COOKIE_OFFSET",
            E1000_MNG_DHCP_COOKIE_OFFSET as i64
        ),
        (
            "E1000_MNG_DHCP_COOKIE_LENGTH",
            E1000_MNG_DHCP_COOKIE_LENGTH as i64
        ),
        ("E1000_MNG_IAMT_MODE", E1000_MNG_IAMT_MODE as i64),
        ("E1000_MNG_ICH_IAMT_MODE", E1000_MNG_ICH_IAMT_MODE as i64),
        ("E1000_IAMT_SIGNATURE", E1000_IAMT_SIGNATURE as i64),
        (
            "E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT",
            E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT as i64
        ),
        (
            "E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT",
            E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT as i64
        ),
        ("E1000_VFTA_ENTRY_SHIFT", E1000_VFTA_ENTRY_SHIFT as i64),
        ("E1000_VFTA_ENTRY_MASK", E1000_VFTA_ENTRY_MASK as i64),
        (
            "E1000_VFTA_ENTRY_BIT_SHIFT_MASK",
            E1000_VFTA_ENTRY_BIT_SHIFT_MASK as i64
        ),
        ("E1000_DEV_ID_82542", E1000_DEV_ID_82542 as i64),
        (
            "E1000_DEV_ID_82543GC_FIBER",
            E1000_DEV_ID_82543GC_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82543GC_COPPER",
            E1000_DEV_ID_82543GC_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82544EI_COPPER",
            E1000_DEV_ID_82544EI_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82544EI_FIBER",
            E1000_DEV_ID_82544EI_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82544GC_COPPER",
            E1000_DEV_ID_82544GC_COPPER as i64
        ),
        ("E1000_DEV_ID_82544GC_LOM", E1000_DEV_ID_82544GC_LOM as i64),
        ("E1000_DEV_ID_82540EM", E1000_DEV_ID_82540EM as i64),
        ("E1000_DEV_ID_82540EM_LOM", E1000_DEV_ID_82540EM_LOM as i64),
        ("E1000_DEV_ID_82540EP_LOM", E1000_DEV_ID_82540EP_LOM as i64),
        ("E1000_DEV_ID_82540EP", E1000_DEV_ID_82540EP as i64),
        ("E1000_DEV_ID_82540EP_LP", E1000_DEV_ID_82540EP_LP as i64),
        (
            "E1000_DEV_ID_82545EM_COPPER",
            E1000_DEV_ID_82545EM_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82545EM_FIBER",
            E1000_DEV_ID_82545EM_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82545GM_COPPER",
            E1000_DEV_ID_82545GM_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82545GM_FIBER",
            E1000_DEV_ID_82545GM_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82545GM_SERDES",
            E1000_DEV_ID_82545GM_SERDES as i64
        ),
        (
            "E1000_DEV_ID_82546EB_COPPER",
            E1000_DEV_ID_82546EB_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82546EB_FIBER",
            E1000_DEV_ID_82546EB_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82546EB_QUAD_COPPER",
            E1000_DEV_ID_82546EB_QUAD_COPPER as i64
        ),
        ("E1000_DEV_ID_82541EI", E1000_DEV_ID_82541EI as i64),
        (
            "E1000_DEV_ID_82541EI_MOBILE",
            E1000_DEV_ID_82541EI_MOBILE as i64
        ),
        ("E1000_DEV_ID_82541ER_LOM", E1000_DEV_ID_82541ER_LOM as i64),
        ("E1000_DEV_ID_82541ER", E1000_DEV_ID_82541ER as i64),
        ("E1000_DEV_ID_82547GI", E1000_DEV_ID_82547GI as i64),
        ("E1000_DEV_ID_82541GI", E1000_DEV_ID_82541GI as i64),
        (
            "E1000_DEV_ID_82541GI_MOBILE",
            E1000_DEV_ID_82541GI_MOBILE as i64
        ),
        ("E1000_DEV_ID_82541GI_LF", E1000_DEV_ID_82541GI_LF as i64),
        (
            "E1000_DEV_ID_82546GB_COPPER",
            E1000_DEV_ID_82546GB_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82546GB_FIBER",
            E1000_DEV_ID_82546GB_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82546GB_SERDES",
            E1000_DEV_ID_82546GB_SERDES as i64
        ),
        (
            "E1000_DEV_ID_82546GB_PCIE",
            E1000_DEV_ID_82546GB_PCIE as i64
        ),
        (
            "E1000_DEV_ID_82546GB_QUAD_COPPER",
            E1000_DEV_ID_82546GB_QUAD_COPPER as i64
        ),
        ("E1000_DEV_ID_82547EI", E1000_DEV_ID_82547EI as i64),
        (
            "E1000_DEV_ID_82547EI_MOBILE",
            E1000_DEV_ID_82547EI_MOBILE as i64
        ),
        (
            "E1000_DEV_ID_82571EB_COPPER",
            E1000_DEV_ID_82571EB_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82571EB_FIBER",
            E1000_DEV_ID_82571EB_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82571EB_SERDES",
            E1000_DEV_ID_82571EB_SERDES as i64
        ),
        (
            "E1000_DEV_ID_82571EB_SERDES_DUAL",
            E1000_DEV_ID_82571EB_SERDES_DUAL as i64
        ),
        (
            "E1000_DEV_ID_82571EB_SERDES_QUAD",
            E1000_DEV_ID_82571EB_SERDES_QUAD as i64
        ),
        (
            "E1000_DEV_ID_82571EB_QUAD_COPPER",
            E1000_DEV_ID_82571EB_QUAD_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82571EB_QUAD_FIBER",
            E1000_DEV_ID_82571EB_QUAD_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82571EB_QUAD_COPPER_LP",
            E1000_DEV_ID_82571EB_QUAD_COPPER_LP as i64
        ),
        (
            "E1000_DEV_ID_82571PT_QUAD_COPPER",
            E1000_DEV_ID_82571PT_QUAD_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82572EI_COPPER",
            E1000_DEV_ID_82572EI_COPPER as i64
        ),
        (
            "E1000_DEV_ID_82572EI_FIBER",
            E1000_DEV_ID_82572EI_FIBER as i64
        ),
        (
            "E1000_DEV_ID_82572EI_SERDES",
            E1000_DEV_ID_82572EI_SERDES as i64
        ),
        ("E1000_DEV_ID_82572EI", E1000_DEV_ID_82572EI as i64),
        ("E1000_DEV_ID_82573E", E1000_DEV_ID_82573E as i64),
        ("E1000_DEV_ID_82573E_IAMT", E1000_DEV_ID_82573E_IAMT as i64),
        ("E1000_DEV_ID_82573L", E1000_DEV_ID_82573L as i64),
        ("E1000_DEV_ID_82574L", E1000_DEV_ID_82574L as i64),
        ("E1000_DEV_ID_82574LA", E1000_DEV_ID_82574LA as i64),
        ("E1000_DEV_ID_82546GB_2", E1000_DEV_ID_82546GB_2 as i64),
        ("E1000_DEV_ID_82571EB_AT", E1000_DEV_ID_82571EB_AT as i64),
        ("E1000_DEV_ID_82571EB_AF", E1000_DEV_ID_82571EB_AF as i64),
        ("E1000_DEV_ID_82573L_PL_1", E1000_DEV_ID_82573L_PL_1 as i64),
        ("E1000_DEV_ID_82573V_PM", E1000_DEV_ID_82573V_PM as i64),
        ("E1000_DEV_ID_82573E_PM", E1000_DEV_ID_82573E_PM as i64),
        ("E1000_DEV_ID_82573L_PL_2", E1000_DEV_ID_82573L_PL_2 as i64),
        (
            "E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3",
            E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3 as i64
        ),
        (
            "E1000_DEV_ID_80003ES2LAN_COPPER_DPT",
            E1000_DEV_ID_80003ES2LAN_COPPER_DPT as i64
        ),
        (
            "E1000_DEV_ID_80003ES2LAN_SERDES_DPT",
            E1000_DEV_ID_80003ES2LAN_SERDES_DPT as i64
        ),
        (
            "E1000_DEV_ID_80003ES2LAN_COPPER_SPT",
            E1000_DEV_ID_80003ES2LAN_COPPER_SPT as i64
        ),
        (
            "E1000_DEV_ID_80003ES2LAN_SERDES_SPT",
            E1000_DEV_ID_80003ES2LAN_SERDES_SPT as i64
        ),
        (
            "E1000_DEV_ID_ICH8_82567V_3",
            E1000_DEV_ID_ICH8_82567V_3 as i64
        ),
        (
            "E1000_DEV_ID_ICH8_IGP_M_AMT",
            E1000_DEV_ID_ICH8_IGP_M_AMT as i64
        ),
        (
            "E1000_DEV_ID_ICH8_IGP_AMT",
            E1000_DEV_ID_ICH8_IGP_AMT as i64
        ),
        ("E1000_DEV_ID_ICH8_IGP_C", E1000_DEV_ID_ICH8_IGP_C as i64),
        ("E1000_DEV_ID_ICH8_IFE", E1000_DEV_ID_ICH8_IFE as i64),
        ("E1000_DEV_ID_ICH8_IFE_GT", E1000_DEV_ID_ICH8_IFE_GT as i64),
        ("E1000_DEV_ID_ICH8_IFE_G", E1000_DEV_ID_ICH8_IFE_G as i64),
        ("E1000_DEV_ID_ICH8_IGP_M", E1000_DEV_ID_ICH8_IGP_M as i64),
        ("E1000_DEV_ID_ICH9_IGP_M", E1000_DEV_ID_ICH9_IGP_M as i64),
        (
            "E1000_DEV_ID_ICH9_IGP_M_AMT",
            E1000_DEV_ID_ICH9_IGP_M_AMT as i64
        ),
        (
            "E1000_DEV_ID_ICH9_IGP_M_V",
            E1000_DEV_ID_ICH9_IGP_M_V as i64
        ),
        (
            "E1000_DEV_ID_ICH9_IGP_AMT",
            E1000_DEV_ID_ICH9_IGP_AMT as i64
        ),
        ("E1000_DEV_ID_ICH9_BM", E1000_DEV_ID_ICH9_BM as i64),
        ("E1000_DEV_ID_ICH9_IGP_C", E1000_DEV_ID_ICH9_IGP_C as i64),
        ("E1000_DEV_ID_ICH9_IFE", E1000_DEV_ID_ICH9_IFE as i64),
        ("E1000_DEV_ID_ICH9_IFE_GT", E1000_DEV_ID_ICH9_IFE_GT as i64),
        ("E1000_DEV_ID_ICH9_IFE_G", E1000_DEV_ID_ICH9_IFE_G as i64),
        (
            "E1000_DEV_ID_ICH10_R_BM_LM",
            E1000_DEV_ID_ICH10_R_BM_LM as i64
        ),
        (
            "E1000_DEV_ID_ICH10_R_BM_LF",
            E1000_DEV_ID_ICH10_R_BM_LF as i64
        ),
        (
            "E1000_DEV_ID_ICH10_R_BM_V",
            E1000_DEV_ID_ICH10_R_BM_V as i64
        ),
        (
            "E1000_DEV_ID_ICH10_D_BM_LM",
            E1000_DEV_ID_ICH10_D_BM_LM as i64
        ),
        (
            "E1000_DEV_ID_ICH10_D_BM_LF",
            E1000_DEV_ID_ICH10_D_BM_LF as i64
        ),
        (
            "E1000_DEV_ID_ICH10_D_BM_V",
            E1000_DEV_ID_ICH10_D_BM_V as i64
        ),
        ("E1000_DEV_ID_PCH_M_HV_LM", E1000_DEV_ID_PCH_M_HV_LM as i64),
        ("E1000_DEV_ID_PCH_M_HV_LC", E1000_DEV_ID_PCH_M_HV_LC as i64),
        ("E1000_DEV_ID_PCH_D_HV_DM", E1000_DEV_ID_PCH_D_HV_DM as i64),
        ("E1000_DEV_ID_PCH_D_HV_DC", E1000_DEV_ID_PCH_D_HV_DC as i64),
        ("E1000_DEV_ID_PCH2_LV_LM", E1000_DEV_ID_PCH2_LV_LM as i64),
        ("E1000_DEV_ID_PCH2_LV_V", E1000_DEV_ID_PCH2_LV_V as i64),
        (
            "E1000_DEV_ID_PCH_LPT_I217_LM",
            E1000_DEV_ID_PCH_LPT_I217_LM as i64
        ),
        (
            "E1000_DEV_ID_PCH_LPT_I217_V",
            E1000_DEV_ID_PCH_LPT_I217_V as i64
        ),
        (
            "E1000_DEV_ID_PCH_LPTLP_I218_LM",
            E1000_DEV_ID_PCH_LPTLP_I218_LM as i64
        ),
        (
            "E1000_DEV_ID_PCH_LPTLP_I218_V",
            E1000_DEV_ID_PCH_LPTLP_I218_V as i64
        ),
        (
            "E1000_DEV_ID_PCH_I218_LM2",
            E1000_DEV_ID_PCH_I218_LM2 as i64
        ),
        ("E1000_DEV_ID_PCH_I218_V2", E1000_DEV_ID_PCH_I218_V2 as i64),
        (
            "E1000_DEV_ID_PCH_I218_LM3",
            E1000_DEV_ID_PCH_I218_LM3 as i64
        ),
        ("E1000_DEV_ID_PCH_I218_V3", E1000_DEV_ID_PCH_I218_V3 as i64),
        (
            "E1000_DEV_ID_PCH_SPT_I219_LM",
            E1000_DEV_ID_PCH_SPT_I219_LM as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_V",
            E1000_DEV_ID_PCH_SPT_I219_V as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_LM2",
            E1000_DEV_ID_PCH_SPT_I219_LM2 as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_V2",
            E1000_DEV_ID_PCH_SPT_I219_V2 as i64
        ),
        (
            "E1000_DEV_ID_PCH_LBG_I219_LM3",
            E1000_DEV_ID_PCH_LBG_I219_LM3 as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_LM4",
            E1000_DEV_ID_PCH_SPT_I219_LM4 as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_V4",
            E1000_DEV_ID_PCH_SPT_I219_V4 as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_LM5",
            E1000_DEV_ID_PCH_SPT_I219_LM5 as i64
        ),
        (
            "E1000_DEV_ID_PCH_SPT_I219_V5",
            E1000_DEV_ID_PCH_SPT_I219_V5 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CNP_I219_LM6",
            E1000_DEV_ID_PCH_CNP_I219_LM6 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CNP_I219_V6",
            E1000_DEV_ID_PCH_CNP_I219_V6 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CNP_I219_LM7",
            E1000_DEV_ID_PCH_CNP_I219_LM7 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CNP_I219_V7",
            E1000_DEV_ID_PCH_CNP_I219_V7 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ICP_I219_LM8",
            E1000_DEV_ID_PCH_ICP_I219_LM8 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ICP_I219_V8",
            E1000_DEV_ID_PCH_ICP_I219_V8 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ICP_I219_LM9",
            E1000_DEV_ID_PCH_ICP_I219_LM9 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ICP_I219_V9",
            E1000_DEV_ID_PCH_ICP_I219_V9 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_LM10",
            E1000_DEV_ID_PCH_CMP_I219_LM10 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_V10",
            E1000_DEV_ID_PCH_CMP_I219_V10 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_LM11",
            E1000_DEV_ID_PCH_CMP_I219_LM11 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_V11",
            E1000_DEV_ID_PCH_CMP_I219_V11 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_LM12",
            E1000_DEV_ID_PCH_CMP_I219_LM12 as i64
        ),
        (
            "E1000_DEV_ID_PCH_CMP_I219_V12",
            E1000_DEV_ID_PCH_CMP_I219_V12 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_LM13",
            E1000_DEV_ID_PCH_TGP_I219_LM13 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_V13",
            E1000_DEV_ID_PCH_TGP_I219_V13 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_LM14",
            E1000_DEV_ID_PCH_TGP_I219_LM14 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_V14",
            E1000_DEV_ID_PCH_TGP_I219_V14 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_LM15",
            E1000_DEV_ID_PCH_TGP_I219_LM15 as i64
        ),
        (
            "E1000_DEV_ID_PCH_TGP_I219_V15",
            E1000_DEV_ID_PCH_TGP_I219_V15 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ADP_I219_LM16",
            E1000_DEV_ID_PCH_ADP_I219_LM16 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ADP_I219_V16",
            E1000_DEV_ID_PCH_ADP_I219_V16 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ADP_I219_LM17",
            E1000_DEV_ID_PCH_ADP_I219_LM17 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ADP_I219_V17",
            E1000_DEV_ID_PCH_ADP_I219_V17 as i64
        ),
        (
            "E1000_DEV_ID_PCH_MTP_I219_LM18",
            E1000_DEV_ID_PCH_MTP_I219_LM18 as i64
        ),
        (
            "E1000_DEV_ID_PCH_MTP_I219_V18",
            E1000_DEV_ID_PCH_MTP_I219_V18 as i64
        ),
        (
            "E1000_DEV_ID_PCH_MTP_I219_LM19",
            E1000_DEV_ID_PCH_MTP_I219_LM19 as i64
        ),
        (
            "E1000_DEV_ID_PCH_MTP_I219_V19",
            E1000_DEV_ID_PCH_MTP_I219_V19 as i64
        ),
        (
            "E1000_DEV_ID_PCH_LNP_I219_LM20",
            E1000_DEV_ID_PCH_LNP_I219_LM20 as i64
        ),
        (
            "E1000_DEV_ID_PCH_LNP_I219_V20",
            E1000_DEV_ID_PCH_LNP_I219_V20 as i64
        ),
        (
            "E1000_DEV_ID_PCH_LNP_I219_LM21",
            E1000_DEV_ID_PCH_LNP_I219_LM21 as i64
        ),
        (
            "E1000_DEV_ID_PCH_LNP_I219_V21",
            E1000_DEV_ID_PCH_LNP_I219_V21 as i64
        ),
        (
            "E1000_DEV_ID_PCH_RPL_I219_LM22",
            E1000_DEV_ID_PCH_RPL_I219_LM22 as i64
        ),
        (
            "E1000_DEV_ID_PCH_RPL_I219_V22",
            E1000_DEV_ID_PCH_RPL_I219_V22 as i64
        ),
        (
            "E1000_DEV_ID_PCH_RPL_I219_LM23",
            E1000_DEV_ID_PCH_RPL_I219_LM23 as i64
        ),
        (
            "E1000_DEV_ID_PCH_RPL_I219_V23",
            E1000_DEV_ID_PCH_RPL_I219_V23 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ARL_I219_LM24",
            E1000_DEV_ID_PCH_ARL_I219_LM24 as i64
        ),
        (
            "E1000_DEV_ID_PCH_ARL_I219_V24",
            E1000_DEV_ID_PCH_ARL_I219_V24 as i64
        ),
        (
            "E1000_DEV_ID_PCH_PTP_I219_LM25",
            E1000_DEV_ID_PCH_PTP_I219_LM25 as i64
        ),
        (
            "E1000_DEV_ID_PCH_PTP_I219_V25",
            E1000_DEV_ID_PCH_PTP_I219_V25 as i64
        ),
        (
            "E1000_DEV_ID_PCH_WCL_I219_LM27",
            E1000_DEV_ID_PCH_WCL_I219_LM27 as i64
        ),
        (
            "E1000_DEV_ID_PCH_WCL_I219_V27",
            E1000_DEV_ID_PCH_WCL_I219_V27 as i64
        ),
        ("E1000_DEV_ID_82575EB_PT", E1000_DEV_ID_82575EB_PT as i64),
        ("E1000_DEV_ID_82575EB_PF", E1000_DEV_ID_82575EB_PF as i64),
        ("E1000_DEV_ID_82575GB_QP", E1000_DEV_ID_82575GB_QP as i64),
        (
            "E1000_DEV_ID_82575GB_QP_PM",
            E1000_DEV_ID_82575GB_QP_PM as i64
        ),
        ("E1000_DEV_ID_82576", E1000_DEV_ID_82576 as i64),
        ("E1000_DEV_ID_82576_FIBER", E1000_DEV_ID_82576_FIBER as i64),
        (
            "E1000_DEV_ID_82576_SERDES",
            E1000_DEV_ID_82576_SERDES as i64
        ),
        (
            "E1000_DEV_ID_82576_QUAD_COPPER",
            E1000_DEV_ID_82576_QUAD_COPPER as i64
        ),
        ("E1000_DEV_ID_82576_NS", E1000_DEV_ID_82576_NS as i64),
        ("E1000_DEV_ID_82583V", E1000_DEV_ID_82583V as i64),
        (
            "E1000_DEV_ID_82576_NS_SERDES",
            E1000_DEV_ID_82576_NS_SERDES as i64
        ),
        (
            "E1000_DEV_ID_82576_SERDES_QUAD",
            E1000_DEV_ID_82576_SERDES_QUAD as i64
        ),
        (
            "E1000_DEV_ID_82580_COPPER",
            E1000_DEV_ID_82580_COPPER as i64
        ),
        ("E1000_DEV_ID_82580_FIBER", E1000_DEV_ID_82580_FIBER as i64),
        (
            "E1000_DEV_ID_82580_SERDES",
            E1000_DEV_ID_82580_SERDES as i64
        ),
        ("E1000_DEV_ID_82580_SGMII", E1000_DEV_ID_82580_SGMII as i64),
        (
            "E1000_DEV_ID_82580_COPPER_DUAL",
            E1000_DEV_ID_82580_COPPER_DUAL as i64
        ),
        (
            "E1000_DEV_ID_82580_QUAD_FIBER",
            E1000_DEV_ID_82580_QUAD_FIBER as i64
        ),
        (
            "E1000_DEV_ID_DH89XXCC_SGMII",
            E1000_DEV_ID_DH89XXCC_SGMII as i64
        ),
        (
            "E1000_DEV_ID_DH89XXCC_SERDES",
            E1000_DEV_ID_DH89XXCC_SERDES as i64
        ),
        (
            "E1000_DEV_ID_DH89XXCC_BACKPLANE",
            E1000_DEV_ID_DH89XXCC_BACKPLANE as i64
        ),
        (
            "E1000_DEV_ID_DH89XXCC_SFP",
            E1000_DEV_ID_DH89XXCC_SFP as i64
        ),
        ("E1000_DEV_ID_I350_COPPER", E1000_DEV_ID_I350_COPPER as i64),
        ("E1000_DEV_ID_I350_FIBER", E1000_DEV_ID_I350_FIBER as i64),
        ("E1000_DEV_ID_I350_SERDES", E1000_DEV_ID_I350_SERDES as i64),
        ("E1000_DEV_ID_I350_SGMII", E1000_DEV_ID_I350_SGMII as i64),
        (
            "E1000_DEV_ID_82576_QUAD_CU_ET2",
            E1000_DEV_ID_82576_QUAD_CU_ET2 as i64
        ),
        ("E1000_DEV_ID_I210_COPPER", E1000_DEV_ID_I210_COPPER as i64),
        (
            "E1000_DEV_ID_I210_COPPER_OEM1",
            E1000_DEV_ID_I210_COPPER_OEM1 as i64
        ),
        (
            "E1000_DEV_ID_I210_COPPER_IT",
            E1000_DEV_ID_I210_COPPER_IT as i64
        ),
        ("E1000_DEV_ID_I210_FIBER", E1000_DEV_ID_I210_FIBER as i64),
        ("E1000_DEV_ID_I210_SERDES", E1000_DEV_ID_I210_SERDES as i64),
        ("E1000_DEV_ID_I210_SGMII", E1000_DEV_ID_I210_SGMII as i64),
        (
            "E1000_DEV_ID_I210_COPPER_FLASHLESS",
            E1000_DEV_ID_I210_COPPER_FLASHLESS as i64
        ),
        (
            "E1000_DEV_ID_I210_SERDES_FLASHLESS",
            E1000_DEV_ID_I210_SERDES_FLASHLESS as i64
        ),
        ("E1000_DEV_ID_I211_COPPER", E1000_DEV_ID_I211_COPPER as i64),
        ("E1000_DEV_ID_I350_DA4", E1000_DEV_ID_I350_DA4 as i64),
        (
            "E1000_DEV_ID_I354_BACKPLANE_1GBPS",
            E1000_DEV_ID_I354_BACKPLANE_1GBPS as i64
        ),
        ("E1000_DEV_ID_I354_SGMII", E1000_DEV_ID_I354_SGMII as i64),
        (
            "E1000_DEV_ID_I354_BACKPLANE_2_5GBPS",
            E1000_DEV_ID_I354_BACKPLANE_2_5GBPS as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_1",
            E1000_DEV_ID_EP80579_LAN_1 as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_2",
            E1000_DEV_ID_EP80579_LAN_2 as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_3",
            E1000_DEV_ID_EP80579_LAN_3 as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_4",
            E1000_DEV_ID_EP80579_LAN_4 as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_5",
            E1000_DEV_ID_EP80579_LAN_5 as i64
        ),
        (
            "E1000_DEV_ID_EP80579_LAN_6",
            E1000_DEV_ID_EP80579_LAN_6 as i64
        ),
        ("NODE_ADDRESS_SIZE", NODE_ADDRESS_SIZE as i64),
        ("ETH_LENGTH_OF_ADDRESS", ETH_LENGTH_OF_ADDRESS as i64),
        ("MAC_DECODE_SIZE", MAC_DECODE_SIZE as i64),
        ("E1000_82542_2_0_REV_ID", E1000_82542_2_0_REV_ID as i64),
        ("E1000_82542_2_1_REV_ID", E1000_82542_2_1_REV_ID as i64),
        ("E1000_REVISION_0", E1000_REVISION_0 as i64),
        ("E1000_REVISION_1", E1000_REVISION_1 as i64),
        ("E1000_REVISION_2", E1000_REVISION_2 as i64),
        ("E1000_REVISION_3", E1000_REVISION_3 as i64),
        ("SPEED_10", SPEED_10 as i64),
        ("SPEED_100", SPEED_100 as i64),
        ("SPEED_1000", SPEED_1000 as i64),
        ("HALF_DUPLEX", HALF_DUPLEX as i64),
        ("FULL_DUPLEX", FULL_DUPLEX as i64),
        ("ENET_HEADER_SIZE", ENET_HEADER_SIZE as i64),
        (
            "MAXIMUM_ETHERNET_FRAME_SIZE",
            MAXIMUM_ETHERNET_FRAME_SIZE as i64
        ),
        (
            "MINIMUM_ETHERNET_FRAME_SIZE",
            MINIMUM_ETHERNET_FRAME_SIZE as i64
        ),
        ("ETHERNET_FCS_SIZE", ETHERNET_FCS_SIZE as i64),
        (
            "MAXIMUM_ETHERNET_PACKET_SIZE",
            MAXIMUM_ETHERNET_PACKET_SIZE as i64
        ),
        (
            "MINIMUM_ETHERNET_PACKET_SIZE",
            MINIMUM_ETHERNET_PACKET_SIZE as i64
        ),
        ("CRC_LENGTH", CRC_LENGTH as i64),
        ("MAX_JUMBO_FRAME_SIZE", MAX_JUMBO_FRAME_SIZE as i64),
        ("VLAN_TAG_SIZE", VLAN_TAG_SIZE as i64),
        ("ETHERNET_IEEE_VLAN_TYPE", ETHERNET_IEEE_VLAN_TYPE as i64),
        ("ETHERNET_IP_TYPE", ETHERNET_IP_TYPE as i64),
        ("ETHERNET_ARP_TYPE", ETHERNET_ARP_TYPE as i64),
        ("IP_PROTOCOL_TCP", IP_PROTOCOL_TCP as i64),
        ("IP_PROTOCOL_UDP", IP_PROTOCOL_UDP as i64),
        ("POLL_IMS_ENABLE_MASK", POLL_IMS_ENABLE_MASK as i64),
        ("IMS_ENABLE_MASK", IMS_ENABLE_MASK as i64),
        ("IMS_ICH8LAN_ENABLE_MASK", IMS_ICH8LAN_ENABLE_MASK as i64),
        ("E1000_RAR_ENTRIES", E1000_RAR_ENTRIES as i64),
        (
            "E1000_RAR_ENTRIES_ICH8LAN",
            E1000_RAR_ENTRIES_ICH8LAN as i64
        ),
        ("E1000_RAR_ENTRIES_82575", E1000_RAR_ENTRIES_82575 as i64),
        ("E1000_RAR_ENTRIES_82576", E1000_RAR_ENTRIES_82576 as i64),
        ("E1000_RAR_ENTRIES_82580", E1000_RAR_ENTRIES_82580 as i64),
        ("E1000_RAR_ENTRIES_I350", E1000_RAR_ENTRIES_I350 as i64),
        (
            "MIN_NUMBER_OF_DESCRIPTORS",
            MIN_NUMBER_OF_DESCRIPTORS as i64
        ),
        (
            "MAX_NUMBER_OF_DESCRIPTORS",
            MAX_NUMBER_OF_DESCRIPTORS as i64
        ),
        ("MAX_PS_BUFFERS", MAX_PS_BUFFERS as i64),
        ("E1000_RXD_STAT_DD", E1000_RXD_STAT_DD as i64),
        ("E1000_RXD_STAT_EOP", E1000_RXD_STAT_EOP as i64),
        ("E1000_RXD_STAT_IXSM", E1000_RXD_STAT_IXSM as i64),
        ("E1000_RXD_STAT_VP", E1000_RXD_STAT_VP as i64),
        ("E1000_RXD_STAT_UDPCS", E1000_RXD_STAT_UDPCS as i64),
        ("E1000_RXD_STAT_TCPCS", E1000_RXD_STAT_TCPCS as i64),
        ("E1000_RXD_STAT_IPCS", E1000_RXD_STAT_IPCS as i64),
        ("E1000_RXD_STAT_PIF", E1000_RXD_STAT_PIF as i64),
        ("E1000_RXD_STAT_IPIDV", E1000_RXD_STAT_IPIDV as i64),
        ("E1000_RXD_STAT_UDPV", E1000_RXD_STAT_UDPV as i64),
        ("E1000_RXD_STAT_ACK", E1000_RXD_STAT_ACK as i64),
        ("E1000_RXD_STAT_STRIPCRC", E1000_RXD_STAT_STRIPCRC as i64),
        ("E1000_RXD_ERR_CE", E1000_RXD_ERR_CE as i64),
        ("E1000_RXD_ERR_SE", E1000_RXD_ERR_SE as i64),
        ("E1000_RXD_ERR_SEQ", E1000_RXD_ERR_SEQ as i64),
        ("E1000_RXD_ERR_CXE", E1000_RXD_ERR_CXE as i64),
        ("E1000_RXD_ERR_TCPE", E1000_RXD_ERR_TCPE as i64),
        ("E1000_RXD_ERR_IPE", E1000_RXD_ERR_IPE as i64),
        ("E1000_RXD_ERR_RXE", E1000_RXD_ERR_RXE as i64),
        ("E1000_RXD_SPC_VLAN_MASK", E1000_RXD_SPC_VLAN_MASK as i64),
        ("E1000_RXD_SPC_PRI_MASK", E1000_RXD_SPC_PRI_MASK as i64),
        ("E1000_RXD_SPC_PRI_SHIFT", E1000_RXD_SPC_PRI_SHIFT as i64),
        ("E1000_RXD_SPC_CFI_MASK", E1000_RXD_SPC_CFI_MASK as i64),
        ("E1000_RXD_SPC_CFI_SHIFT", E1000_RXD_SPC_CFI_SHIFT as i64),
        ("E1000_RXDEXT_STATERR_CE", E1000_RXDEXT_STATERR_CE as i64),
        ("E1000_RXDEXT_STATERR_SE", E1000_RXDEXT_STATERR_SE as i64),
        ("E1000_RXDEXT_STATERR_SEQ", E1000_RXDEXT_STATERR_SEQ as i64),
        ("E1000_RXDEXT_STATERR_CXE", E1000_RXDEXT_STATERR_CXE as i64),
        (
            "E1000_RXDEXT_STATERR_TCPE",
            E1000_RXDEXT_STATERR_TCPE as i64
        ),
        ("E1000_RXDEXT_STATERR_IPE", E1000_RXDEXT_STATERR_IPE as i64),
        ("E1000_RXDEXT_STATERR_RXE", E1000_RXDEXT_STATERR_RXE as i64),
        (
            "E1000_RXDPS_HDRSTAT_HDRSP",
            E1000_RXDPS_HDRSTAT_HDRSP as i64
        ),
        (
            "E1000_RXDPS_HDRSTAT_HDRLEN_MASK",
            E1000_RXDPS_HDRSTAT_HDRLEN_MASK as i64
        ),
        (
            "E1000_RXD_ERR_FRAME_ERR_MASK",
            E1000_RXD_ERR_FRAME_ERR_MASK as i64
        ),
        (
            "E1000_RXDEXT_ERR_FRAME_ERR_MASK",
            E1000_RXDEXT_ERR_FRAME_ERR_MASK as i64
        ),
        ("E1000_TXD_DTYP_D", E1000_TXD_DTYP_D as i64),
        ("E1000_TXD_DTYP_C", E1000_TXD_DTYP_C as i64),
        ("E1000_TXD_POPTS_IXSM", E1000_TXD_POPTS_IXSM as i64),
        ("E1000_TXD_POPTS_TXSM", E1000_TXD_POPTS_TXSM as i64),
        ("E1000_TXD_CMD_EOP", E1000_TXD_CMD_EOP as i64),
        ("E1000_TXD_CMD_IFCS", E1000_TXD_CMD_IFCS as i64),
        ("E1000_TXD_CMD_IC", E1000_TXD_CMD_IC as i64),
        ("E1000_TXD_CMD_RS", E1000_TXD_CMD_RS as i64),
        ("E1000_TXD_CMD_RPS", E1000_TXD_CMD_RPS as i64),
        ("E1000_TXD_CMD_DEXT", E1000_TXD_CMD_DEXT as i64),
        ("E1000_TXD_CMD_VLE", E1000_TXD_CMD_VLE as i64),
        ("E1000_TXD_CMD_IDE", E1000_TXD_CMD_IDE as i64),
        ("E1000_TXD_STAT_DD", E1000_TXD_STAT_DD as i64),
        ("E1000_TXD_STAT_EC", E1000_TXD_STAT_EC as i64),
        ("E1000_TXD_STAT_LC", E1000_TXD_STAT_LC as i64),
        ("E1000_TXD_STAT_TU", E1000_TXD_STAT_TU as i64),
        ("E1000_TXD_CMD_TCP", E1000_TXD_CMD_TCP as i64),
        ("E1000_TXD_CMD_IP", E1000_TXD_CMD_IP as i64),
        ("E1000_TXD_CMD_TSE", E1000_TXD_CMD_TSE as i64),
        ("E1000_TXD_STAT_TC", E1000_TXD_STAT_TC as i64),
        ("E1000_NUM_UNICAST", E1000_NUM_UNICAST as i64),
        ("E1000_MC_TBL_SIZE", E1000_MC_TBL_SIZE as i64),
        (
            "E1000_VLAN_FILTER_TBL_SIZE",
            E1000_VLAN_FILTER_TBL_SIZE as i64
        ),
        (
            "E1000_NUM_UNICAST_ICH8LAN",
            E1000_NUM_UNICAST_ICH8LAN as i64
        ),
        (
            "E1000_MC_TBL_SIZE_ICH8LAN",
            E1000_MC_TBL_SIZE_ICH8LAN as i64
        ),
        ("E1000_NUM_MTA_REGISTERS", E1000_NUM_MTA_REGISTERS as i64),
        (
            "E1000_NUM_MTA_REGISTERS_ICH8LAN",
            E1000_NUM_MTA_REGISTERS_ICH8LAN as i64
        ),
        (
            "E1000_WAKEUP_IP_ADDRESS_COUNT_MAX",
            E1000_WAKEUP_IP_ADDRESS_COUNT_MAX as i64
        ),
        ("E1000_IP4AT_SIZE", E1000_IP4AT_SIZE as i64),
        ("E1000_IP4AT_SIZE_ICH8LAN", E1000_IP4AT_SIZE_ICH8LAN as i64),
        ("E1000_IP6AT_SIZE", E1000_IP6AT_SIZE as i64),
        (
            "E1000_FLEXIBLE_FILTER_COUNT_MAX",
            E1000_FLEXIBLE_FILTER_COUNT_MAX as i64
        ),
        (
            "E1000_FLEXIBLE_FILTER_SIZE_MAX",
            E1000_FLEXIBLE_FILTER_SIZE_MAX as i64
        ),
        ("E1000_FFLT_SIZE", E1000_FFLT_SIZE as i64),
        ("E1000_FFMT_SIZE", E1000_FFMT_SIZE as i64),
        ("E1000_FFVT_SIZE", E1000_FFVT_SIZE as i64),
        (
            "E1000_DISABLE_SERDES_LOOPBACK",
            E1000_DISABLE_SERDES_LOOPBACK as i64
        ),
        ("E1000_CTRL", E1000_CTRL as i64),
        ("E1000_CTRL_DUP", E1000_CTRL_DUP as i64),
        ("E1000_STATUS", E1000_STATUS as i64),
        ("E1000_EECD", E1000_EECD as i64),
        ("E1000_EERD", E1000_EERD as i64),
        ("E1000_CTRL_EXT", E1000_CTRL_EXT as i64),
        ("E1000_FLA", E1000_FLA as i64),
        ("E1000_MDIC", E1000_MDIC as i64),
        ("E1000_MDICNFG", E1000_MDICNFG as i64),
        ("E1000_SCTL", E1000_SCTL as i64),
        ("E1000_FEXTNVM", E1000_FEXTNVM as i64),
        ("E1000_FEXTNVM3", E1000_FEXTNVM3 as i64),
        ("E1000_FEXTNVM4", E1000_FEXTNVM4 as i64),
        ("E1000_FEXTNVM6", E1000_FEXTNVM6 as i64),
        ("E1000_FEXTNVM12", E1000_FEXTNVM12 as i64),
        ("E1000_FCAL", E1000_FCAL as i64),
        ("E1000_FCAH", E1000_FCAH as i64),
        ("E1000_FCT", E1000_FCT as i64),
        ("E1000_CONNSW", E1000_CONNSW as i64),
        ("E1000_VET", E1000_VET as i64),
        ("E1000_ICR", E1000_ICR as i64),
        ("E1000_ITR", E1000_ITR as i64),
        ("E1000_ICS", E1000_ICS as i64),
        ("E1000_IMS", E1000_IMS as i64),
        ("E1000_IMC", E1000_IMC as i64),
        ("E1000_IAM", E1000_IAM as i64),
        ("E1000_RCTL", E1000_RCTL as i64),
        ("E1000_GPIE", E1000_GPIE as i64),
        ("E1000_EICS", E1000_EICS as i64),
        ("E1000_EIMS", E1000_EIMS as i64),
        ("E1000_EIMC", E1000_EIMC as i64),
        ("E1000_EIAC", E1000_EIAC as i64),
        ("E1000_EIAM", E1000_EIAM as i64),
        ("E1000_EICR", E1000_EICR as i64),
        ("E1000_IVAR0", E1000_IVAR0 as i64),
        ("E1000_IVAR_MISC", E1000_IVAR_MISC as i64),
        ("E1000_RDTR1", E1000_RDTR1 as i64),
        ("E1000_RDBAL1", E1000_RDBAL1 as i64),
        ("E1000_RDBAH1", E1000_RDBAH1 as i64),
        ("E1000_RDLEN1", E1000_RDLEN1 as i64),
        ("E1000_FCTTV", E1000_FCTTV as i64),
        ("E1000_TXCW", E1000_TXCW as i64),
        ("E1000_RXCW", E1000_RXCW as i64),
        ("E1000_TCTL", E1000_TCTL as i64),
        ("E1000_TCTL_EXT", E1000_TCTL_EXT as i64),
        ("E1000_TIPG", E1000_TIPG as i64),
        ("E1000_TBT", E1000_TBT as i64),
        ("E1000_AIT", E1000_AIT as i64),
        ("E1000_LEDCTL", E1000_LEDCTL as i64),
        ("E1000_EXTCNF_CTRL", E1000_EXTCNF_CTRL as i64),
        ("E1000_EXTCNF_SIZE", E1000_EXTCNF_SIZE as i64),
        ("E1000_PHY_CTRL", E1000_PHY_CTRL as i64),
        ("FEXTNVM_SW_CONFIG", FEXTNVM_SW_CONFIG as i64),
        ("FEXTNVM_SW_CONFIG_ICH8M", FEXTNVM_SW_CONFIG_ICH8M as i64),
        ("E1000_PBA", E1000_PBA as i64),
        ("E1000_PBS", E1000_PBS as i64),
        ("E1000_IOSFPC", E1000_IOSFPC as i64),
        ("E1000_EEMNGCTL", E1000_EEMNGCTL as i64),
        ("E1000_FLASH_UPDATES", E1000_FLASH_UPDATES as i64),
        ("E1000_EEARBC", E1000_EEARBC as i64),
        ("E1000_FLASHT", E1000_FLASHT as i64),
        ("E1000_EEWR", E1000_EEWR as i64),
        ("E1000_FLSWCTL", E1000_FLSWCTL as i64),
        ("E1000_FLSWDATA", E1000_FLSWDATA as i64),
        ("E1000_FLSWCNT", E1000_FLSWCNT as i64),
        ("E1000_FLOP", E1000_FLOP as i64),
        ("E1000_I2CCMD", E1000_I2CCMD as i64),
        ("E1000_ERT", E1000_ERT as i64),
        ("E1000_FCRTL", E1000_FCRTL as i64),
        ("E1000_FCRTH", E1000_FCRTH as i64),
        ("E1000_PSRCTL", E1000_PSRCTL as i64),
        ("E1000_RDTR", E1000_RDTR as i64),
        ("E1000_RDTR0", E1000_RDTR0 as i64),
        ("E1000_RADV", E1000_RADV as i64),
        ("E1000_RSRPD", E1000_RSRPD as i64),
        ("E1000_RAID", E1000_RAID as i64),
        ("E1000_TXDMAC", E1000_TXDMAC as i64),
        ("E1000_KABGTXD", E1000_KABGTXD as i64),
        ("E1000_TDFH", E1000_TDFH as i64),
        ("E1000_TDFT", E1000_TDFT as i64),
        ("E1000_TDFHS", E1000_TDFHS as i64),
        ("E1000_TDFTS", E1000_TDFTS as i64),
        ("E1000_TDFPC", E1000_TDFPC as i64),
        ("E1000_TIDV", E1000_TIDV as i64),
        ("E1000_TADV", E1000_TADV as i64),
        ("E1000_TSPMT", E1000_TSPMT as i64),
        ("E1000_TARC0", E1000_TARC0 as i64),
        ("E1000_TDBAL1", E1000_TDBAL1 as i64),
        ("E1000_TDBAH1", E1000_TDBAH1 as i64),
        ("E1000_TDLEN1", E1000_TDLEN1 as i64),
        ("E1000_TDH1", E1000_TDH1 as i64),
        ("E1000_TDT1", E1000_TDT1 as i64),
        ("E1000_TARC1", E1000_TARC1 as i64),
        ("E1000_CRCERRS", E1000_CRCERRS as i64),
        ("E1000_ALGNERRC", E1000_ALGNERRC as i64),
        ("E1000_SYMERRS", E1000_SYMERRS as i64),
        ("E1000_RXERRC", E1000_RXERRC as i64),
        ("E1000_MPC", E1000_MPC as i64),
        ("E1000_SCC", E1000_SCC as i64),
        ("E1000_ECOL", E1000_ECOL as i64),
        ("E1000_MCC", E1000_MCC as i64),
        ("E1000_LATECOL", E1000_LATECOL as i64),
        ("E1000_COLC", E1000_COLC as i64),
        ("E1000_DC", E1000_DC as i64),
        ("E1000_TNCRS", E1000_TNCRS as i64),
        ("E1000_SEC", E1000_SEC as i64),
        ("E1000_CEXTERR", E1000_CEXTERR as i64),
        ("E1000_RLEC", E1000_RLEC as i64),
        ("E1000_XONRXC", E1000_XONRXC as i64),
        ("E1000_XONTXC", E1000_XONTXC as i64),
        ("E1000_XOFFRXC", E1000_XOFFRXC as i64),
        ("E1000_XOFFTXC", E1000_XOFFTXC as i64),
        ("E1000_FCRUC", E1000_FCRUC as i64),
        ("E1000_PRC64", E1000_PRC64 as i64),
        ("E1000_PRC127", E1000_PRC127 as i64),
        ("E1000_PRC255", E1000_PRC255 as i64),
        ("E1000_PRC511", E1000_PRC511 as i64),
        ("E1000_PRC1023", E1000_PRC1023 as i64),
        ("E1000_PRC1522", E1000_PRC1522 as i64),
        ("E1000_GPRC", E1000_GPRC as i64),
        ("E1000_BPRC", E1000_BPRC as i64),
        ("E1000_MPRC", E1000_MPRC as i64),
        ("E1000_GPTC", E1000_GPTC as i64),
        ("E1000_GORCL", E1000_GORCL as i64),
        ("E1000_GORCH", E1000_GORCH as i64),
        ("E1000_GOTCL", E1000_GOTCL as i64),
        ("E1000_GOTCH", E1000_GOTCH as i64),
        ("E1000_RNBC", E1000_RNBC as i64),
        ("E1000_RUC", E1000_RUC as i64),
        ("E1000_RFC", E1000_RFC as i64),
        ("E1000_ROC", E1000_ROC as i64),
        ("E1000_RJC", E1000_RJC as i64),
        ("E1000_MGTPRC", E1000_MGTPRC as i64),
        ("E1000_MGTPDC", E1000_MGTPDC as i64),
        ("E1000_MGTPTC", E1000_MGTPTC as i64),
        ("E1000_TORL", E1000_TORL as i64),
        ("E1000_TORH", E1000_TORH as i64),
        ("E1000_TOTL", E1000_TOTL as i64),
        ("E1000_TOTH", E1000_TOTH as i64),
        ("E1000_TPR", E1000_TPR as i64),
        ("E1000_TPT", E1000_TPT as i64),
        ("E1000_PTC64", E1000_PTC64 as i64),
        ("E1000_PTC127", E1000_PTC127 as i64),
        ("E1000_PTC255", E1000_PTC255 as i64),
        ("E1000_PTC511", E1000_PTC511 as i64),
        ("E1000_PTC1023", E1000_PTC1023 as i64),
        ("E1000_PTC1522", E1000_PTC1522 as i64),
        ("E1000_MPTC", E1000_MPTC as i64),
        ("E1000_BPTC", E1000_BPTC as i64),
        ("E1000_TSCTC", E1000_TSCTC as i64),
        ("E1000_TSCTFC", E1000_TSCTFC as i64),
        ("E1000_IAC", E1000_IAC as i64),
        ("E1000_RPTHC", E1000_RPTHC as i64),
        ("E1000_ICRXPTC", E1000_ICRXPTC as i64),
        ("E1000_ICRXATC", E1000_ICRXATC as i64),
        ("E1000_ICTXPTC", E1000_ICTXPTC as i64),
        ("E1000_ICTXATC", E1000_ICTXATC as i64),
        ("E1000_ICTXQEC", E1000_ICTXQEC as i64),
        ("E1000_ICTXQMTC", E1000_ICTXQMTC as i64),
        ("E1000_ICRXDMTC", E1000_ICRXDMTC as i64),
        ("E1000_ICRXOC", E1000_ICRXOC as i64),
        ("E1000_SDPC", E1000_SDPC as i64),
        ("E1000_PCS_CFG0", E1000_PCS_CFG0 as i64),
        ("E1000_PCS_LCTL", E1000_PCS_LCTL as i64),
        ("E1000_PCS_LSTAT", E1000_PCS_LSTAT as i64),
        ("E1000_RXCSUM", E1000_RXCSUM as i64),
        ("E1000_RFCTL", E1000_RFCTL as i64),
        ("E1000_MTA", E1000_MTA as i64),
        ("E1000_RA", E1000_RA as i64),
        ("E1000_VFTA", E1000_VFTA as i64),
        ("E1000_WUC", E1000_WUC as i64),
        ("E1000_WUFC", E1000_WUFC as i64),
        ("E1000_WUS", E1000_WUS as i64),
        ("E1000_MANC", E1000_MANC as i64),
        ("E1000_IPAV", E1000_IPAV as i64),
        ("E1000_IP4AT", E1000_IP4AT as i64),
        ("E1000_IP6AT", E1000_IP6AT as i64),
        ("E1000_WUPL", E1000_WUPL as i64),
        ("E1000_WUPM", E1000_WUPM as i64),
        ("E1000_FFLT", E1000_FFLT as i64),
        ("E1000_FCRTV_PCH", E1000_FCRTV_PCH as i64),
        ("E1000_CRC_OFFSET", E1000_CRC_OFFSET as i64),
        ("E1000_HOST_IF", E1000_HOST_IF as i64),
        ("E1000_FFMT", E1000_FFMT as i64),
        ("E1000_FFVT", E1000_FFVT as i64),
        ("E1000_KUMCTRLSTA", E1000_KUMCTRLSTA as i64),
        ("E1000_MDPHYA", E1000_MDPHYA as i64),
        ("E1000_MANC2H", E1000_MANC2H as i64),
        ("E1000_SW_FW_SYNC", E1000_SW_FW_SYNC as i64),
        ("E1000_GCR", E1000_GCR as i64),
        ("E1000_GSCL_1", E1000_GSCL_1 as i64),
        ("E1000_GSCL_2", E1000_GSCL_2 as i64),
        ("E1000_GSCL_3", E1000_GSCL_3 as i64),
        ("E1000_GSCL_4", E1000_GSCL_4 as i64),
        ("E1000_FACTPS", E1000_FACTPS as i64),
        ("E1000_SWSM", E1000_SWSM as i64),
        ("E1000_H2ME", E1000_H2ME as i64),
        ("E1000_FWSM", E1000_FWSM as i64),
        ("E1000_FFLT_DBG", E1000_FFLT_DBG as i64),
        ("E1000_HICR", E1000_HICR as i64),
        ("E1000_CPUVEC", E1000_CPUVEC as i64),
        ("E1000_MRQC", E1000_MRQC as i64),
        ("E1000_RSSIM", E1000_RSSIM as i64),
        ("E1000_RSSIR", E1000_RSSIR as i64),
        ("E1000_B2OSPC", E1000_B2OSPC as i64),
        ("E1000_B2OGPRC", E1000_B2OGPRC as i64),
        ("E1000_O2BGPTC", E1000_O2BGPTC as i64),
        ("E1000_O2BSPC", E1000_O2BSPC as i64),
        ("E1000_PHPM", E1000_PHPM as i64),
        ("E1000_PHPM_SPD_EN", E1000_PHPM_SPD_EN as i64),
        ("E1000_PHPM_D0LPLU", E1000_PHPM_D0LPLU as i64),
        ("E1000_PHPM_LPLU", E1000_PHPM_LPLU as i64),
        ("E1000_PHPM_DIS_1000_ND0", E1000_PHPM_DIS_1000_ND0 as i64),
        ("E1000_PHPM_LINK_ED", E1000_PHPM_LINK_ED as i64),
        ("E1000_PHPM_GOLINK_DISC", E1000_PHPM_GOLINK_DISC as i64),
        ("E1000_PHPM_DIS_1000", E1000_PHPM_DIS_1000 as i64),
        ("E1000_PHPM_SPD_B2B_EN", E1000_PHPM_SPD_B2B_EN as i64),
        ("E1000_PHPM_RST_COMPL", E1000_PHPM_RST_COMPL as i64),
        ("E1000_PHPM_DIS_100_ND0", E1000_PHPM_DIS_100_ND0 as i64),
        ("E1000_IPCNFG", E1000_IPCNFG as i64),
        ("E1000_LTRC", E1000_LTRC as i64),
        ("E1000_EEER", E1000_EEER as i64),
        ("E1000_EEE_SU", E1000_EEE_SU as i64),
        ("E1000_TLPIC", E1000_TLPIC as i64),
        ("E1000_RLPIC", E1000_RLPIC as i64),
        (
            "E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK",
            E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK as i64
        ),
        (
            "E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC",
            E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC as i64
        ),
        (
            "E1000_FEXTNVM4_BEACON_DURATION_MASK",
            E1000_FEXTNVM4_BEACON_DURATION_MASK as i64
        ),
        (
            "E1000_FEXTNVM4_BEACON_DURATION_8USEC",
            E1000_FEXTNVM4_BEACON_DURATION_8USEC as i64
        ),
        (
            "E1000_FEXTNVM4_BEACON_DURATION_16USEC",
            E1000_FEXTNVM4_BEACON_DURATION_16USEC as i64
        ),
        (
            "E1000_FEXTNVM6_REQ_PLL_CLK",
            E1000_FEXTNVM6_REQ_PLL_CLK as i64
        ),
        (
            "E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION",
            E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION as i64
        ),
        (
            "E1000_FEXTNVM12_PHYPD_CTRL_MASK",
            E1000_FEXTNVM12_PHYPD_CTRL_MASK as i64
        ),
        (
            "E1000_FEXTNVM12_PHYPD_CTRL_P1",
            E1000_FEXTNVM12_PHYPD_CTRL_P1 as i64
        ),
        ("E1000_EEPROM_SWDPIN0", E1000_EEPROM_SWDPIN0 as i64),
        ("E1000_EEPROM_LED_LOGIC", E1000_EEPROM_LED_LOGIC as i64),
        ("E1000_EEPROM_RW_REG_DATA", E1000_EEPROM_RW_REG_DATA as i64),
        ("E1000_EEPROM_RW_REG_DONE", E1000_EEPROM_RW_REG_DONE as i64),
        (
            "E1000_EEPROM_RW_REG_START",
            E1000_EEPROM_RW_REG_START as i64
        ),
        (
            "E1000_EEPROM_RW_ADDR_SHIFT",
            E1000_EEPROM_RW_ADDR_SHIFT as i64
        ),
        ("E1000_EEPROM_POLL_WRITE", E1000_EEPROM_POLL_WRITE as i64),
        ("E1000_EEPROM_POLL_READ", E1000_EEPROM_POLL_READ as i64),
        ("E1000_CTRL_FD", E1000_CTRL_FD as i64),
        ("E1000_CTRL_BEM", E1000_CTRL_BEM as i64),
        ("E1000_CTRL_PRIOR", E1000_CTRL_PRIOR as i64),
        (
            "E1000_CTRL_GIO_MASTER_DISABLE",
            E1000_CTRL_GIO_MASTER_DISABLE as i64
        ),
        ("E1000_CTRL_LRST", E1000_CTRL_LRST as i64),
        ("E1000_CTRL_TME", E1000_CTRL_TME as i64),
        ("E1000_CTRL_SLE", E1000_CTRL_SLE as i64),
        ("E1000_CTRL_ASDE", E1000_CTRL_ASDE as i64),
        ("E1000_CTRL_SLU", E1000_CTRL_SLU as i64),
        ("E1000_CTRL_ILOS", E1000_CTRL_ILOS as i64),
        ("E1000_CTRL_SPD_SEL", E1000_CTRL_SPD_SEL as i64),
        ("E1000_CTRL_SPD_10", E1000_CTRL_SPD_10 as i64),
        ("E1000_CTRL_SPD_100", E1000_CTRL_SPD_100 as i64),
        ("E1000_CTRL_SPD_1000", E1000_CTRL_SPD_1000 as i64),
        ("E1000_CTRL_BEM32", E1000_CTRL_BEM32 as i64),
        ("E1000_CTRL_FRCSPD", E1000_CTRL_FRCSPD as i64),
        ("E1000_CTRL_FRCDPX", E1000_CTRL_FRCDPX as i64),
        ("E1000_CTRL_D_UD_EN", E1000_CTRL_D_UD_EN as i64),
        ("E1000_CTRL_D_UD_POLARITY", E1000_CTRL_D_UD_POLARITY as i64),
        (
            "E1000_CTRL_FORCE_PHY_RESET",
            E1000_CTRL_FORCE_PHY_RESET as i64
        ),
        (
            "E1000_CTRL_LANPHYPC_OVERRIDE",
            E1000_CTRL_LANPHYPC_OVERRIDE as i64
        ),
        (
            "E1000_CTRL_LANPHYPC_VALUE",
            E1000_CTRL_LANPHYPC_VALUE as i64
        ),
        ("E1000_CTRL_EXT_DPG_EN", E1000_CTRL_EXT_DPG_EN as i64),
        (
            "E1000_CTRL_EXT_FORCE_SMBUS",
            E1000_CTRL_EXT_FORCE_SMBUS as i64
        ),
        ("E1000_CTRL_EXT_PHYPDEN", E1000_CTRL_EXT_PHYPDEN as i64),
        (
            "E1000_I2CCMD_REG_ADDR_SHIFT",
            E1000_I2CCMD_REG_ADDR_SHIFT as i64
        ),
        (
            "E1000_I2CCMD_PHY_ADDR_SHIFT",
            E1000_I2CCMD_PHY_ADDR_SHIFT as i64
        ),
        ("E1000_I2CCMD_OPCODE_READ", E1000_I2CCMD_OPCODE_READ as i64),
        (
            "E1000_I2CCMD_OPCODE_WRITE",
            E1000_I2CCMD_OPCODE_WRITE as i64
        ),
        ("E1000_I2CCMD_READY", E1000_I2CCMD_READY as i64),
        ("E1000_I2CCMD_ERROR", E1000_I2CCMD_ERROR as i64),
        (
            "E1000_MAX_SGMII_PHY_REG_ADDR",
            E1000_MAX_SGMII_PHY_REG_ADDR as i64
        ),
        ("E1000_I2CCMD_PHY_TIMEOUT", E1000_I2CCMD_PHY_TIMEOUT as i64),
        ("E1000_CTRL_SWDPIN0", E1000_CTRL_SWDPIN0 as i64),
        ("E1000_CTRL_SWDPIN1", E1000_CTRL_SWDPIN1 as i64),
        ("E1000_CTRL_SWDPIN2", E1000_CTRL_SWDPIN2 as i64),
        ("E1000_CTRL_SWDPIN3", E1000_CTRL_SWDPIN3 as i64),
        ("E1000_CTRL_SWDPIO0", E1000_CTRL_SWDPIO0 as i64),
        ("E1000_CTRL_SWDPIO1", E1000_CTRL_SWDPIO1 as i64),
        ("E1000_CTRL_SWDPIO2", E1000_CTRL_SWDPIO2 as i64),
        ("E1000_CTRL_SWDPIO3", E1000_CTRL_SWDPIO3 as i64),
        ("E1000_CTRL_RST", E1000_CTRL_RST as i64),
        ("E1000_CTRL_RFCE", E1000_CTRL_RFCE as i64),
        ("E1000_CTRL_TFCE", E1000_CTRL_TFCE as i64),
        ("E1000_CTRL_RTE", E1000_CTRL_RTE as i64),
        ("E1000_CTRL_DEV_RST", E1000_CTRL_DEV_RST as i64),
        ("E1000_CTRL_VME", E1000_CTRL_VME as i64),
        ("E1000_CTRL_PHY_RST", E1000_CTRL_PHY_RST as i64),
        ("E1000_CTRL_SW2FW_INT", E1000_CTRL_SW2FW_INT as i64),
        ("E1000_CTRL_I2C_ENA", E1000_CTRL_I2C_ENA as i64),
        ("E1000_CONNSW_ENRGSRC", E1000_CONNSW_ENRGSRC as i64),
        ("E1000_PCS_CFG_PCS_EN", E1000_PCS_CFG_PCS_EN as i64),
        ("E1000_PCS_LCTL_FSV_1000", E1000_PCS_LCTL_FSV_1000 as i64),
        ("E1000_PCS_LCTL_FDV_FULL", E1000_PCS_LCTL_FDV_FULL as i64),
        ("E1000_PCS_LCTL_FSD", E1000_PCS_LCTL_FSD as i64),
        (
            "E1000_PCS_LCTL_FORCE_FCTRL",
            E1000_PCS_LCTL_FORCE_FCTRL as i64
        ),
        ("E1000_PCS_LSTS_LINK_OK", E1000_PCS_LSTS_LINK_OK as i64),
        ("E1000_PCS_LSTS_SPEED_100", E1000_PCS_LSTS_SPEED_100 as i64),
        (
            "E1000_PCS_LSTS_SPEED_1000",
            E1000_PCS_LSTS_SPEED_1000 as i64
        ),
        (
            "E1000_PCS_LSTS_DUPLEX_FULL",
            E1000_PCS_LSTS_DUPLEX_FULL as i64
        ),
        ("E1000_PCS_LSTS_SYNK_OK", E1000_PCS_LSTS_SYNK_OK as i64),
        ("E1000_STATUS_FD", E1000_STATUS_FD as i64),
        ("E1000_STATUS_LU", E1000_STATUS_LU as i64),
        ("E1000_STATUS_FUNC_MASK", E1000_STATUS_FUNC_MASK as i64),
        ("E1000_STATUS_FUNC_SHIFT", E1000_STATUS_FUNC_SHIFT as i64),
        ("E1000_STATUS_FUNC_0", E1000_STATUS_FUNC_0 as i64),
        ("E1000_STATUS_FUNC_1", E1000_STATUS_FUNC_1 as i64),
        ("E1000_STATUS_TXOFF", E1000_STATUS_TXOFF as i64),
        ("E1000_STATUS_TBIMODE", E1000_STATUS_TBIMODE as i64),
        ("E1000_STATUS_SPEED_MASK", E1000_STATUS_SPEED_MASK as i64),
        ("E1000_STATUS_SPEED_10", E1000_STATUS_SPEED_10 as i64),
        ("E1000_STATUS_SPEED_100", E1000_STATUS_SPEED_100 as i64),
        ("E1000_STATUS_SPEED_1000", E1000_STATUS_SPEED_1000 as i64),
        (
            "E1000_STATUS_LAN_INIT_DONE",
            E1000_STATUS_LAN_INIT_DONE as i64
        ),
        ("E1000_STATUS_ASDV", E1000_STATUS_ASDV as i64),
        ("E1000_STATUS_DOCK_CI", E1000_STATUS_DOCK_CI as i64),
        (
            "E1000_STATUS_GIO_MASTER_ENABLE",
            E1000_STATUS_GIO_MASTER_ENABLE as i64
        ),
        ("E1000_STATUS_MTXCKOK", E1000_STATUS_MTXCKOK as i64),
        ("E1000_STATUS_PCI66", E1000_STATUS_PCI66 as i64),
        ("E1000_STATUS_BUS64", E1000_STATUS_BUS64 as i64),
        ("E1000_STATUS_PCIX_MODE", E1000_STATUS_PCIX_MODE as i64),
        ("E1000_STATUS_PCIX_SPEED", E1000_STATUS_PCIX_SPEED as i64),
        ("E1000_STATUS_BMC_SKU_0", E1000_STATUS_BMC_SKU_0 as i64),
        ("E1000_STATUS_DEV_RST_SET", E1000_STATUS_DEV_RST_SET as i64),
        ("E1000_STATUS_BMC_SKU_1", E1000_STATUS_BMC_SKU_1 as i64),
        ("E1000_STATUS_BMC_SKU_2", E1000_STATUS_BMC_SKU_2 as i64),
        ("E1000_STATUS_BMC_CRYPTO", E1000_STATUS_BMC_CRYPTO as i64),
        ("E1000_STATUS_BMC_LITE", E1000_STATUS_BMC_LITE as i64),
        (
            "E1000_STATUS_RGMII_ENABLE",
            E1000_STATUS_RGMII_ENABLE as i64
        ),
        ("E1000_STATUS_FUSE_8", E1000_STATUS_FUSE_8 as i64),
        ("E1000_STATUS_FUSE_9", E1000_STATUS_FUSE_9 as i64),
        ("E1000_STATUS_SERDES0_DIS", E1000_STATUS_SERDES0_DIS as i64),
        ("E1000_STATUS_SERDES1_DIS", E1000_STATUS_SERDES1_DIS as i64),
        (
            "E1000_STATUS_PCIX_SPEED_66",
            E1000_STATUS_PCIX_SPEED_66 as i64
        ),
        (
            "E1000_STATUS_PCIX_SPEED_100",
            E1000_STATUS_PCIX_SPEED_100 as i64
        ),
        (
            "E1000_STATUS_PCIX_SPEED_133",
            E1000_STATUS_PCIX_SPEED_133 as i64
        ),
        ("E1000_EECD_SK", E1000_EECD_SK as i64),
        ("E1000_EECD_CS", E1000_EECD_CS as i64),
        ("E1000_EECD_DI", E1000_EECD_DI as i64),
        ("E1000_EECD_DO", E1000_EECD_DO as i64),
        ("E1000_EECD_FWE_MASK", E1000_EECD_FWE_MASK as i64),
        ("E1000_EECD_FWE_DIS", E1000_EECD_FWE_DIS as i64),
        ("E1000_EECD_FWE_EN", E1000_EECD_FWE_EN as i64),
        ("E1000_EECD_FWE_SHIFT", E1000_EECD_FWE_SHIFT as i64),
        ("E1000_EECD_REQ", E1000_EECD_REQ as i64),
        ("E1000_EECD_GNT", E1000_EECD_GNT as i64),
        ("E1000_EECD_PRES", E1000_EECD_PRES as i64),
        ("E1000_EECD_SIZE", E1000_EECD_SIZE as i64),
        ("E1000_EECD_ADDR_BITS", E1000_EECD_ADDR_BITS as i64),
        ("E1000_EECD_TYPE", E1000_EECD_TYPE as i64),
        (
            "E1000_EEPROM_GRANT_ATTEMPTS",
            E1000_EEPROM_GRANT_ATTEMPTS as i64
        ),
        ("E1000_EECD_AUTO_RD", E1000_EECD_AUTO_RD as i64),
        ("E1000_EECD_SIZE_EX_MASK", E1000_EECD_SIZE_EX_MASK as i64),
        ("E1000_EECD_SIZE_EX_SHIFT", E1000_EECD_SIZE_EX_SHIFT as i64),
        ("E1000_EECD_NVADDS", E1000_EECD_NVADDS as i64),
        ("E1000_EECD_SELSHAD", E1000_EECD_SELSHAD as i64),
        ("E1000_EECD_INITSRAM", E1000_EECD_INITSRAM as i64),
        ("E1000_EECD_FLUPD", E1000_EECD_FLUPD as i64),
        ("E1000_EECD_AUPDEN", E1000_EECD_AUPDEN as i64),
        ("E1000_EECD_SHADV", E1000_EECD_SHADV as i64),
        ("E1000_EECD_SEC1VAL", E1000_EECD_SEC1VAL as i64),
        (
            "E1000_EECD_SEC1VAL_VALID_MASK",
            E1000_EECD_SEC1VAL_VALID_MASK as i64
        ),
        ("E1000_EECD_SECVAL_SHIFT", E1000_EECD_SECVAL_SHIFT as i64),
        ("E1000_STM_OPCODE", E1000_STM_OPCODE as i64),
        ("E1000_HICR_FW_RESET", E1000_HICR_FW_RESET as i64),
        ("E1000_SHADOW_RAM_WORDS", E1000_SHADOW_RAM_WORDS as i64),
        ("E1000_ICH_NVM_SIG_WORD", E1000_ICH_NVM_SIG_WORD as i64),
        ("E1000_ICH_NVM_SIG_MASK", E1000_ICH_NVM_SIG_MASK as i64),
        (
            "E1000_ICH_NVM_VALID_SIG_MASK",
            E1000_ICH_NVM_VALID_SIG_MASK as i64
        ),
        ("E1000_ICH_NVM_SIG_VALUE", E1000_ICH_NVM_SIG_VALUE as i64),
        ("E1000_EERD_START", E1000_EERD_START as i64),
        ("E1000_EERD_DONE", E1000_EERD_DONE as i64),
        ("E1000_EERD_ADDR_SHIFT", E1000_EERD_ADDR_SHIFT as i64),
        ("E1000_EERD_ADDR_MASK", E1000_EERD_ADDR_MASK as i64),
        ("E1000_EERD_DATA_SHIFT", E1000_EERD_DATA_SHIFT as i64),
        ("E1000_EERD_DATA_MASK", E1000_EERD_DATA_MASK as i64),
        ("EEPROM_STATUS_RDY_SPI", EEPROM_STATUS_RDY_SPI as i64),
        ("EEPROM_STATUS_WEN_SPI", EEPROM_STATUS_WEN_SPI as i64),
        ("EEPROM_STATUS_BP0_SPI", EEPROM_STATUS_BP0_SPI as i64),
        ("EEPROM_STATUS_BP1_SPI", EEPROM_STATUS_BP1_SPI as i64),
        ("EEPROM_STATUS_WPEN_SPI", EEPROM_STATUS_WPEN_SPI as i64),
        ("E1000_CTRL_EXT_GPI0_EN", E1000_CTRL_EXT_GPI0_EN as i64),
        ("E1000_CTRL_EXT_GPI1_EN", E1000_CTRL_EXT_GPI1_EN as i64),
        ("E1000_CTRL_EXT_PHYINT_EN", E1000_CTRL_EXT_PHYINT_EN as i64),
        ("E1000_CTRL_EXT_GPI2_EN", E1000_CTRL_EXT_GPI2_EN as i64),
        ("E1000_CTRL_EXT_LPCD", E1000_CTRL_EXT_LPCD as i64),
        ("E1000_CTRL_EXT_GPI3_EN", E1000_CTRL_EXT_GPI3_EN as i64),
        ("E1000_CTRL_EXT_SDP4_DATA", E1000_CTRL_EXT_SDP4_DATA as i64),
        ("E1000_CTRL_EXT_SDP5_DATA", E1000_CTRL_EXT_SDP5_DATA as i64),
        ("E1000_CTRL_EXT_PHY_INT", E1000_CTRL_EXT_PHY_INT as i64),
        ("E1000_CTRL_EXT_SDP6_DATA", E1000_CTRL_EXT_SDP6_DATA as i64),
        ("E1000_CTRL_EXT_SDP7_DATA", E1000_CTRL_EXT_SDP7_DATA as i64),
        ("E1000_CTRL_EXT_SDP3_DATA", E1000_CTRL_EXT_SDP3_DATA as i64),
        ("E1000_CTRL_EXT_SDP4_DIR", E1000_CTRL_EXT_SDP4_DIR as i64),
        ("E1000_CTRL_EXT_SDP5_DIR", E1000_CTRL_EXT_SDP5_DIR as i64),
        ("E1000_CTRL_EXT_SDP6_DIR", E1000_CTRL_EXT_SDP6_DIR as i64),
        ("E1000_CTRL_EXT_SDP7_DIR", E1000_CTRL_EXT_SDP7_DIR as i64),
        ("E1000_CTRL_EXT_ASDCHK", E1000_CTRL_EXT_ASDCHK as i64),
        ("E1000_CTRL_EXT_EE_RST", E1000_CTRL_EXT_EE_RST as i64),
        ("E1000_CTRL_EXT_IPS", E1000_CTRL_EXT_IPS as i64),
        ("E1000_CTRL_EXT_SPD_BYPS", E1000_CTRL_EXT_SPD_BYPS as i64),
        ("E1000_CTRL_EXT_RO_DIS", E1000_CTRL_EXT_RO_DIS as i64),
        (
            "E1000_CTRL_EXT_LINK_MODE_MASK",
            E1000_CTRL_EXT_LINK_MODE_MASK as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_GMII",
            E1000_CTRL_EXT_LINK_MODE_GMII as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_TBI",
            E1000_CTRL_EXT_LINK_MODE_TBI as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_KMRN",
            E1000_CTRL_EXT_LINK_MODE_KMRN as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES",
            E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_1000BASE_KX",
            E1000_CTRL_EXT_LINK_MODE_1000BASE_KX as i64
        ),
        (
            "E1000_CTRL_EXT_LINK_MODE_SGMII",
            E1000_CTRL_EXT_LINK_MODE_SGMII as i64
        ),
        (
            "E1000_CTRL_EXT_WR_WMARK_MASK",
            E1000_CTRL_EXT_WR_WMARK_MASK as i64
        ),
        (
            "E1000_CTRL_EXT_WR_WMARK_256",
            E1000_CTRL_EXT_WR_WMARK_256 as i64
        ),
        (
            "E1000_CTRL_EXT_WR_WMARK_320",
            E1000_CTRL_EXT_WR_WMARK_320 as i64
        ),
        (
            "E1000_CTRL_EXT_WR_WMARK_384",
            E1000_CTRL_EXT_WR_WMARK_384 as i64
        ),
        (
            "E1000_CTRL_EXT_WR_WMARK_448",
            E1000_CTRL_EXT_WR_WMARK_448 as i64
        ),
        ("E1000_CTRL_EXT_EXT_VLAN", E1000_CTRL_EXT_EXT_VLAN as i64),
        ("E1000_CTRL_EXT_DRV_LOAD", E1000_CTRL_EXT_DRV_LOAD as i64),
        ("E1000_CTRL_EXT_IAME", E1000_CTRL_EXT_IAME as i64),
        (
            "E1000_CTRL_EXT_INT_TIMER_CLR",
            E1000_CTRL_EXT_INT_TIMER_CLR as i64
        ),
        ("E1000_CRTL_EXT_PB_PAREN", E1000_CRTL_EXT_PB_PAREN as i64),
        ("E1000_CTRL_EXT_DF_PAREN", E1000_CTRL_EXT_DF_PAREN as i64),
        (
            "E1000_CTRL_EXT_GHOST_PAREN",
            E1000_CTRL_EXT_GHOST_PAREN as i64
        ),
        ("E1000_MDIC_DATA_MASK", E1000_MDIC_DATA_MASK as i64),
        ("E1000_MDIC_REG_MASK", E1000_MDIC_REG_MASK as i64),
        ("E1000_MDIC_REG_SHIFT", E1000_MDIC_REG_SHIFT as i64),
        ("E1000_MDIC_PHY_MASK", E1000_MDIC_PHY_MASK as i64),
        ("E1000_MDIC_PHY_SHIFT", E1000_MDIC_PHY_SHIFT as i64),
        ("E1000_MDIC_OP_WRITE", E1000_MDIC_OP_WRITE as i64),
        ("E1000_MDIC_OP_READ", E1000_MDIC_OP_READ as i64),
        ("E1000_MDIC_READY", E1000_MDIC_READY as i64),
        ("E1000_MDIC_INT_EN", E1000_MDIC_INT_EN as i64),
        ("E1000_MDIC_ERROR", E1000_MDIC_ERROR as i64),
        ("E1000_MDIC_DEST", E1000_MDIC_DEST as i64),
        ("E1000_KUMCTRLSTA_MASK", E1000_KUMCTRLSTA_MASK as i64),
        ("E1000_KUMCTRLSTA_OFFSET", E1000_KUMCTRLSTA_OFFSET as i64),
        (
            "E1000_KUMCTRLSTA_OFFSET_SHIFT",
            E1000_KUMCTRLSTA_OFFSET_SHIFT as i64
        ),
        ("E1000_KUMCTRLSTA_REN", E1000_KUMCTRLSTA_REN as i64),
        (
            "E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL",
            E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_CTRL",
            E1000_KUMCTRLSTA_OFFSET_CTRL as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_INB_CTRL",
            E1000_KUMCTRLSTA_OFFSET_INB_CTRL as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_DIAG",
            E1000_KUMCTRLSTA_OFFSET_DIAG as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_TIMEOUTS",
            E1000_KUMCTRLSTA_OFFSET_TIMEOUTS as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_INB_PARAM",
            E1000_KUMCTRLSTA_OFFSET_INB_PARAM as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_HD_CTRL",
            E1000_KUMCTRLSTA_OFFSET_HD_CTRL as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_M2P_SERDES",
            E1000_KUMCTRLSTA_OFFSET_M2P_SERDES as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_M2P_MODES",
            E1000_KUMCTRLSTA_OFFSET_M2P_MODES as i64
        ),
        (
            "E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS",
            E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS as i64
        ),
        (
            "E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS",
            E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS as i64
        ),
        (
            "E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT",
            E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT as i64
        ),
        (
            "E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING",
            E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING as i64
        ),
        (
            "E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT",
            E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT as i64
        ),
        (
            "E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT",
            E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT as i64
        ),
        (
            "E1000_KUMCTRLSTA_OFFSET_K0S_CTRL",
            E1000_KUMCTRLSTA_OFFSET_K0S_CTRL as i64
        ),
        (
            "E1000_KUMCTRLSTA_DIAG_FELPBK",
            E1000_KUMCTRLSTA_DIAG_FELPBK as i64
        ),
        (
            "E1000_KUMCTRLSTA_DIAG_NELPBK",
            E1000_KUMCTRLSTA_DIAG_NELPBK as i64
        ),
        (
            "E1000_KUMCTRLSTA_K0S_100_EN",
            E1000_KUMCTRLSTA_K0S_100_EN as i64
        ),
        (
            "E1000_KUMCTRLSTA_K0S_GBE_EN",
            E1000_KUMCTRLSTA_K0S_GBE_EN as i64
        ),
        (
            "E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK",
            E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK as i64
        ),
        ("E1000_KABGTXD_BGSQLBIAS", E1000_KABGTXD_BGSQLBIAS as i64),
        ("E1000_PHY_CTRL_SPD_EN", E1000_PHY_CTRL_SPD_EN as i64),
        ("E1000_PHY_CTRL_D0A_LPLU", E1000_PHY_CTRL_D0A_LPLU as i64),
        (
            "E1000_PHY_CTRL_NOND0A_LPLU",
            E1000_PHY_CTRL_NOND0A_LPLU as i64
        ),
        (
            "E1000_PHY_CTRL_NOND0A_GBE_DISABLE",
            E1000_PHY_CTRL_NOND0A_GBE_DISABLE as i64
        ),
        (
            "E1000_PHY_CTRL_GBE_DISABLE",
            E1000_PHY_CTRL_GBE_DISABLE as i64
        ),
        ("E1000_PHY_CTRL_B2B_EN", E1000_PHY_CTRL_B2B_EN as i64),
        ("E1000_PHY_CTRL_LOOPBACK", E1000_PHY_CTRL_LOOPBACK as i64),
        (
            "E1000_LEDCTL_LED0_MODE_MASK",
            E1000_LEDCTL_LED0_MODE_MASK as i64
        ),
        (
            "E1000_LEDCTL_LED0_MODE_SHIFT",
            E1000_LEDCTL_LED0_MODE_SHIFT as i64
        ),
        (
            "E1000_LEDCTL_LED0_BLINK_RATE",
            E1000_LEDCTL_LED0_BLINK_RATE as i64
        ),
        ("E1000_LEDCTL_LED0_IVRT", E1000_LEDCTL_LED0_IVRT as i64),
        ("E1000_LEDCTL_LED0_BLINK", E1000_LEDCTL_LED0_BLINK as i64),
        (
            "E1000_LEDCTL_LED1_MODE_MASK",
            E1000_LEDCTL_LED1_MODE_MASK as i64
        ),
        (
            "E1000_LEDCTL_LED1_MODE_SHIFT",
            E1000_LEDCTL_LED1_MODE_SHIFT as i64
        ),
        (
            "E1000_LEDCTL_LED1_BLINK_RATE",
            E1000_LEDCTL_LED1_BLINK_RATE as i64
        ),
        ("E1000_LEDCTL_LED1_IVRT", E1000_LEDCTL_LED1_IVRT as i64),
        ("E1000_LEDCTL_LED1_BLINK", E1000_LEDCTL_LED1_BLINK as i64),
        (
            "E1000_LEDCTL_LED2_MODE_MASK",
            E1000_LEDCTL_LED2_MODE_MASK as i64
        ),
        (
            "E1000_LEDCTL_LED2_MODE_SHIFT",
            E1000_LEDCTL_LED2_MODE_SHIFT as i64
        ),
        (
            "E1000_LEDCTL_LED2_BLINK_RATE",
            E1000_LEDCTL_LED2_BLINK_RATE as i64
        ),
        ("E1000_LEDCTL_LED2_IVRT", E1000_LEDCTL_LED2_IVRT as i64),
        ("E1000_LEDCTL_LED2_BLINK", E1000_LEDCTL_LED2_BLINK as i64),
        (
            "E1000_LEDCTL_LED3_MODE_MASK",
            E1000_LEDCTL_LED3_MODE_MASK as i64
        ),
        (
            "E1000_LEDCTL_LED3_MODE_SHIFT",
            E1000_LEDCTL_LED3_MODE_SHIFT as i64
        ),
        (
            "E1000_LEDCTL_LED3_BLINK_RATE",
            E1000_LEDCTL_LED3_BLINK_RATE as i64
        ),
        ("E1000_LEDCTL_LED3_IVRT", E1000_LEDCTL_LED3_IVRT as i64),
        ("E1000_LEDCTL_LED3_BLINK", E1000_LEDCTL_LED3_BLINK as i64),
        (
            "E1000_LEDCTL_MODE_LINK_10_1000",
            E1000_LEDCTL_MODE_LINK_10_1000 as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_100_1000",
            E1000_LEDCTL_MODE_LINK_100_1000 as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_UP",
            E1000_LEDCTL_MODE_LINK_UP as i64
        ),
        (
            "E1000_LEDCTL_MODE_ACTIVITY",
            E1000_LEDCTL_MODE_ACTIVITY as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_ACTIVITY",
            E1000_LEDCTL_MODE_LINK_ACTIVITY as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_10",
            E1000_LEDCTL_MODE_LINK_10 as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_100",
            E1000_LEDCTL_MODE_LINK_100 as i64
        ),
        (
            "E1000_LEDCTL_MODE_LINK_1000",
            E1000_LEDCTL_MODE_LINK_1000 as i64
        ),
        (
            "E1000_LEDCTL_MODE_PCIX_MODE",
            E1000_LEDCTL_MODE_PCIX_MODE as i64
        ),
        (
            "E1000_LEDCTL_MODE_FULL_DUPLEX",
            E1000_LEDCTL_MODE_FULL_DUPLEX as i64
        ),
        (
            "E1000_LEDCTL_MODE_COLLISION",
            E1000_LEDCTL_MODE_COLLISION as i64
        ),
        (
            "E1000_LEDCTL_MODE_BUS_SPEED",
            E1000_LEDCTL_MODE_BUS_SPEED as i64
        ),
        (
            "E1000_LEDCTL_MODE_BUS_SIZE",
            E1000_LEDCTL_MODE_BUS_SIZE as i64
        ),
        ("E1000_LEDCTL_MODE_PAUSED", E1000_LEDCTL_MODE_PAUSED as i64),
        ("E1000_LEDCTL_MODE_LED_ON", E1000_LEDCTL_MODE_LED_ON as i64),
        (
            "E1000_LEDCTL_MODE_LED_OFF",
            E1000_LEDCTL_MODE_LED_OFF as i64
        ),
        ("E1000_RAH_AV", E1000_RAH_AV as i64),
        ("E1000_ICR_TXDW", E1000_ICR_TXDW as i64),
        ("E1000_ICR_TXQE", E1000_ICR_TXQE as i64),
        ("E1000_ICR_LSC", E1000_ICR_LSC as i64),
        ("E1000_ICR_RXSEQ", E1000_ICR_RXSEQ as i64),
        ("E1000_ICR_RXDMT0", E1000_ICR_RXDMT0 as i64),
        ("E1000_ICR_RXO", E1000_ICR_RXO as i64),
        ("E1000_ICR_RXT0", E1000_ICR_RXT0 as i64),
        ("E1000_ICR_MDAC", E1000_ICR_MDAC as i64),
        ("E1000_ICR_RXCFG", E1000_ICR_RXCFG as i64),
        ("E1000_ICR_GPI_EN0", E1000_ICR_GPI_EN0 as i64),
        ("E1000_ICR_GPI_EN1", E1000_ICR_GPI_EN1 as i64),
        ("E1000_ICR_GPI_EN2", E1000_ICR_GPI_EN2 as i64),
        ("E1000_ICR_GPI_EN3", E1000_ICR_GPI_EN3 as i64),
        ("E1000_ICR_TXD_LOW", E1000_ICR_TXD_LOW as i64),
        ("E1000_ICR_SRPD", E1000_ICR_SRPD as i64),
        ("E1000_ICR_ACK", E1000_ICR_ACK as i64),
        ("E1000_ICR_MNG", E1000_ICR_MNG as i64),
        ("E1000_ICR_DOCK", E1000_ICR_DOCK as i64),
        ("E1000_ICR_INT_ASSERTED", E1000_ICR_INT_ASSERTED as i64),
        ("E1000_ICR_RXD_FIFO_PAR0", E1000_ICR_RXD_FIFO_PAR0 as i64),
        ("E1000_ICR_TXD_FIFO_PAR0", E1000_ICR_TXD_FIFO_PAR0 as i64),
        ("E1000_ICR_HOST_ARB_PAR", E1000_ICR_HOST_ARB_PAR as i64),
        ("E1000_ICR_PB_PAR", E1000_ICR_PB_PAR as i64),
        ("E1000_ICR_RXD_FIFO_PAR1", E1000_ICR_RXD_FIFO_PAR1 as i64),
        ("E1000_ICR_TXD_FIFO_PAR1", E1000_ICR_TXD_FIFO_PAR1 as i64),
        ("E1000_ICR_ALL_PARITY", E1000_ICR_ALL_PARITY as i64),
        ("E1000_ICR_DSW", E1000_ICR_DSW as i64),
        ("E1000_ICR_PHYINT", E1000_ICR_PHYINT as i64),
        ("E1000_ICR_EPRST", E1000_ICR_EPRST as i64),
        ("E1000_ICR_DRSTA", E1000_ICR_DRSTA as i64),
        ("E1000_ICS_TXDW", E1000_ICS_TXDW as i64),
        ("E1000_ICS_TXQE", E1000_ICS_TXQE as i64),
        ("E1000_ICS_LSC", E1000_ICS_LSC as i64),
        ("E1000_ICS_RXSEQ", E1000_ICS_RXSEQ as i64),
        ("E1000_ICS_RXDMT0", E1000_ICS_RXDMT0 as i64),
        ("E1000_ICS_RXO", E1000_ICS_RXO as i64),
        ("E1000_ICS_RXT0", E1000_ICS_RXT0 as i64),
        ("E1000_ICS_MDAC", E1000_ICS_MDAC as i64),
        ("E1000_ICS_RXCFG", E1000_ICS_RXCFG as i64),
        ("E1000_ICS_GPI_EN0", E1000_ICS_GPI_EN0 as i64),
        ("E1000_ICS_GPI_EN1", E1000_ICS_GPI_EN1 as i64),
        ("E1000_ICS_GPI_EN2", E1000_ICS_GPI_EN2 as i64),
        ("E1000_ICS_GPI_EN3", E1000_ICS_GPI_EN3 as i64),
        ("E1000_ICS_TXD_LOW", E1000_ICS_TXD_LOW as i64),
        ("E1000_ICS_SRPD", E1000_ICS_SRPD as i64),
        ("E1000_ICS_ACK", E1000_ICS_ACK as i64),
        ("E1000_ICS_MNG", E1000_ICS_MNG as i64),
        ("E1000_ICS_DOCK", E1000_ICS_DOCK as i64),
        ("E1000_ICS_RXD_FIFO_PAR0", E1000_ICS_RXD_FIFO_PAR0 as i64),
        ("E1000_ICS_TXD_FIFO_PAR0", E1000_ICS_TXD_FIFO_PAR0 as i64),
        ("E1000_ICS_HOST_ARB_PAR", E1000_ICS_HOST_ARB_PAR as i64),
        ("E1000_ICS_PB_PAR", E1000_ICS_PB_PAR as i64),
        ("E1000_ICS_RXD_FIFO_PAR1", E1000_ICS_RXD_FIFO_PAR1 as i64),
        ("E1000_ICS_TXD_FIFO_PAR1", E1000_ICS_TXD_FIFO_PAR1 as i64),
        ("E1000_ICS_DSW", E1000_ICS_DSW as i64),
        ("E1000_ICS_PHYINT", E1000_ICS_PHYINT as i64),
        ("E1000_ICS_EPRST", E1000_ICS_EPRST as i64),
        ("E1000_ICS_DRSTA", E1000_ICS_DRSTA as i64),
        ("E1000_IMS_TXDW", E1000_IMS_TXDW as i64),
        ("E1000_IMS_TXQE", E1000_IMS_TXQE as i64),
        ("E1000_IMS_LSC", E1000_IMS_LSC as i64),
        ("E1000_IMS_RXSEQ", E1000_IMS_RXSEQ as i64),
        ("E1000_IMS_RXDMT0", E1000_IMS_RXDMT0 as i64),
        ("E1000_IMS_RXO", E1000_IMS_RXO as i64),
        ("E1000_IMS_RXT0", E1000_IMS_RXT0 as i64),
        ("E1000_IMS_MDAC", E1000_IMS_MDAC as i64),
        ("E1000_IMS_RXCFG", E1000_IMS_RXCFG as i64),
        ("E1000_IMS_GPI_EN0", E1000_IMS_GPI_EN0 as i64),
        ("E1000_IMS_GPI_EN1", E1000_IMS_GPI_EN1 as i64),
        ("E1000_IMS_GPI_EN2", E1000_IMS_GPI_EN2 as i64),
        ("E1000_IMS_GPI_EN3", E1000_IMS_GPI_EN3 as i64),
        ("E1000_IMS_TXD_LOW", E1000_IMS_TXD_LOW as i64),
        ("E1000_IMS_SRPD", E1000_IMS_SRPD as i64),
        ("E1000_IMS_ACK", E1000_IMS_ACK as i64),
        ("E1000_IMS_MNG", E1000_IMS_MNG as i64),
        ("E1000_IMS_DOCK", E1000_IMS_DOCK as i64),
        ("E1000_IMS_RXD_FIFO_PAR0", E1000_IMS_RXD_FIFO_PAR0 as i64),
        ("E1000_IMS_TXD_FIFO_PAR0", E1000_IMS_TXD_FIFO_PAR0 as i64),
        ("E1000_IMS_HOST_ARB_PAR", E1000_IMS_HOST_ARB_PAR as i64),
        ("E1000_IMS_PB_PAR", E1000_IMS_PB_PAR as i64),
        ("E1000_IMS_RXD_FIFO_PAR1", E1000_IMS_RXD_FIFO_PAR1 as i64),
        ("E1000_IMS_TXD_FIFO_PAR1", E1000_IMS_TXD_FIFO_PAR1 as i64),
        ("E1000_IMS_DSW", E1000_IMS_DSW as i64),
        ("E1000_IMS_PHYINT", E1000_IMS_PHYINT as i64),
        ("E1000_IMS_EPRST", E1000_IMS_EPRST as i64),
        ("E1000_IMS_DRSTA", E1000_IMS_DRSTA as i64),
        ("E1000_IMC_TXDW", E1000_IMC_TXDW as i64),
        ("E1000_IMC_TXQE", E1000_IMC_TXQE as i64),
        ("E1000_IMC_LSC", E1000_IMC_LSC as i64),
        ("E1000_IMC_RXSEQ", E1000_IMC_RXSEQ as i64),
        ("E1000_IMC_RXDMT0", E1000_IMC_RXDMT0 as i64),
        ("E1000_IMC_RXO", E1000_IMC_RXO as i64),
        ("E1000_IMC_RXT0", E1000_IMC_RXT0 as i64),
        ("E1000_IMC_MDAC", E1000_IMC_MDAC as i64),
        ("E1000_IMC_RXCFG", E1000_IMC_RXCFG as i64),
        ("E1000_IMC_GPI_EN0", E1000_IMC_GPI_EN0 as i64),
        ("E1000_IMC_GPI_EN1", E1000_IMC_GPI_EN1 as i64),
        ("E1000_IMC_GPI_EN2", E1000_IMC_GPI_EN2 as i64),
        ("E1000_IMC_GPI_EN3", E1000_IMC_GPI_EN3 as i64),
        ("E1000_IMC_TXD_LOW", E1000_IMC_TXD_LOW as i64),
        ("E1000_IMC_SRPD", E1000_IMC_SRPD as i64),
        ("E1000_IMC_ACK", E1000_IMC_ACK as i64),
        ("E1000_IMC_MNG", E1000_IMC_MNG as i64),
        ("E1000_IMC_DOCK", E1000_IMC_DOCK as i64),
        ("E1000_IMC_RXD_FIFO_PAR0", E1000_IMC_RXD_FIFO_PAR0 as i64),
        ("E1000_IMC_TXD_FIFO_PAR0", E1000_IMC_TXD_FIFO_PAR0 as i64),
        ("E1000_IMC_HOST_ARB_PAR", E1000_IMC_HOST_ARB_PAR as i64),
        ("E1000_IMC_PB_PAR", E1000_IMC_PB_PAR as i64),
        ("E1000_IMC_RXD_FIFO_PAR1", E1000_IMC_RXD_FIFO_PAR1 as i64),
        ("E1000_IMC_TXD_FIFO_PAR1", E1000_IMC_TXD_FIFO_PAR1 as i64),
        ("E1000_IMC_DSW", E1000_IMC_DSW as i64),
        ("E1000_IMC_PHYINT", E1000_IMC_PHYINT as i64),
        ("E1000_IMC_EPRST", E1000_IMC_EPRST as i64),
        ("E1000_IMC_DRSTA", E1000_IMC_DRSTA as i64),
        ("E1000_RCTL_RST", E1000_RCTL_RST as i64),
        ("E1000_RCTL_EN", E1000_RCTL_EN as i64),
        ("E1000_RCTL_SBP", E1000_RCTL_SBP as i64),
        ("E1000_RCTL_UPE", E1000_RCTL_UPE as i64),
        ("E1000_RCTL_MPE", E1000_RCTL_MPE as i64),
        ("E1000_RCTL_LPE", E1000_RCTL_LPE as i64),
        ("E1000_RCTL_LBM_NO", E1000_RCTL_LBM_NO as i64),
        ("E1000_RCTL_LBM_MAC", E1000_RCTL_LBM_MAC as i64),
        ("E1000_RCTL_LBM_SLP", E1000_RCTL_LBM_SLP as i64),
        ("E1000_RCTL_LBM_TCVR", E1000_RCTL_LBM_TCVR as i64),
        ("E1000_RCTL_DTYP_MASK", E1000_RCTL_DTYP_MASK as i64),
        ("E1000_RCTL_DTYP_PS", E1000_RCTL_DTYP_PS as i64),
        ("E1000_RCTL_RDMTS_HALF", E1000_RCTL_RDMTS_HALF as i64),
        ("E1000_RCTL_RDMTS_QUAT", E1000_RCTL_RDMTS_QUAT as i64),
        ("E1000_RCTL_RDMTS_EIGTH", E1000_RCTL_RDMTS_EIGTH as i64),
        ("E1000_RCTL_RDMTS_HEX", E1000_RCTL_RDMTS_HEX as i64),
        ("E1000_RCTL_MO_SHIFT", E1000_RCTL_MO_SHIFT as i64),
        ("E1000_RCTL_MO_0", E1000_RCTL_MO_0 as i64),
        ("E1000_RCTL_MO_1", E1000_RCTL_MO_1 as i64),
        ("E1000_RCTL_MO_2", E1000_RCTL_MO_2 as i64),
        ("E1000_RCTL_MO_3", E1000_RCTL_MO_3 as i64),
        ("E1000_RCTL_MDR", E1000_RCTL_MDR as i64),
        ("E1000_RCTL_BAM", E1000_RCTL_BAM as i64),
        ("E1000_RCTL_SZ_2048", E1000_RCTL_SZ_2048 as i64),
        ("E1000_RCTL_SZ_1024", E1000_RCTL_SZ_1024 as i64),
        ("E1000_RCTL_SZ_512", E1000_RCTL_SZ_512 as i64),
        ("E1000_RCTL_SZ_256", E1000_RCTL_SZ_256 as i64),
        ("E1000_RCTL_SZ_16384", E1000_RCTL_SZ_16384 as i64),
        ("E1000_RCTL_SZ_8192", E1000_RCTL_SZ_8192 as i64),
        ("E1000_RCTL_SZ_4096", E1000_RCTL_SZ_4096 as i64),
        ("E1000_RCTL_VFE", E1000_RCTL_VFE as i64),
        ("E1000_RCTL_CFIEN", E1000_RCTL_CFIEN as i64),
        ("E1000_RCTL_CFI", E1000_RCTL_CFI as i64),
        ("E1000_RCTL_DPF", E1000_RCTL_DPF as i64),
        ("E1000_RCTL_PMCF", E1000_RCTL_PMCF as i64),
        ("E1000_RCTL_BSEX", E1000_RCTL_BSEX as i64),
        ("E1000_RCTL_SECRC", E1000_RCTL_SECRC as i64),
        ("E1000_RCTL_FLXBUF_MASK", E1000_RCTL_FLXBUF_MASK as i64),
        ("E1000_RCTL_FLXBUF_SHIFT", E1000_RCTL_FLXBUF_SHIFT as i64),
        ("E1000_PSRCTL_BSIZE0_MASK", E1000_PSRCTL_BSIZE0_MASK as i64),
        ("E1000_PSRCTL_BSIZE1_MASK", E1000_PSRCTL_BSIZE1_MASK as i64),
        ("E1000_PSRCTL_BSIZE2_MASK", E1000_PSRCTL_BSIZE2_MASK as i64),
        ("E1000_PSRCTL_BSIZE3_MASK", E1000_PSRCTL_BSIZE3_MASK as i64),
        (
            "E1000_PSRCTL_BSIZE0_SHIFT",
            E1000_PSRCTL_BSIZE0_SHIFT as i64
        ),
        (
            "E1000_PSRCTL_BSIZE1_SHIFT",
            E1000_PSRCTL_BSIZE1_SHIFT as i64
        ),
        (
            "E1000_PSRCTL_BSIZE2_SHIFT",
            E1000_PSRCTL_BSIZE2_SHIFT as i64
        ),
        (
            "E1000_PSRCTL_BSIZE3_SHIFT",
            E1000_PSRCTL_BSIZE3_SHIFT as i64
        ),
        ("E1000_SWFW_EEP_SM", E1000_SWFW_EEP_SM as i64),
        ("E1000_SWFW_PHY0_SM", E1000_SWFW_PHY0_SM as i64),
        ("E1000_SWFW_PHY1_SM", E1000_SWFW_PHY1_SM as i64),
        ("E1000_SWFW_MAC_CSR_SM", E1000_SWFW_MAC_CSR_SM as i64),
        ("E1000_SWFW_PHY2_SM", E1000_SWFW_PHY2_SM as i64),
        ("E1000_SWFW_PHY3_SM", E1000_SWFW_PHY3_SM as i64),
        ("E1000_RDT_DELAY", E1000_RDT_DELAY as i64),
        ("E1000_RDT_FPDB", E1000_RDT_FPDB as i64),
        ("E1000_RDLEN_LEN", E1000_RDLEN_LEN as i64),
        ("E1000_RDH_RDH", E1000_RDH_RDH as i64),
        ("E1000_RDT_RDT", E1000_RDT_RDT as i64),
        ("E1000_FCRTH_RTH", E1000_FCRTH_RTH as i64),
        ("E1000_FCRTH_XFCE", E1000_FCRTH_XFCE as i64),
        ("E1000_FCRTL_RTL", E1000_FCRTL_RTL as i64),
        ("E1000_FCRTL_XONE", E1000_FCRTL_XONE as i64),
        ("E1000_FC_NONE", E1000_FC_NONE as i64),
        ("E1000_FC_RX_PAUSE", E1000_FC_RX_PAUSE as i64),
        ("E1000_FC_TX_PAUSE", E1000_FC_TX_PAUSE as i64),
        ("E1000_FC_FULL", E1000_FC_FULL as i64),
        ("E1000_FC_DEFAULT", E1000_FC_DEFAULT as i64),
        ("E1000_RFCTL_ISCSI_DIS", E1000_RFCTL_ISCSI_DIS as i64),
        (
            "E1000_RFCTL_ISCSI_DWC_MASK",
            E1000_RFCTL_ISCSI_DWC_MASK as i64
        ),
        (
            "E1000_RFCTL_ISCSI_DWC_SHIFT",
            E1000_RFCTL_ISCSI_DWC_SHIFT as i64
        ),
        ("E1000_RFCTL_NFSW_DIS", E1000_RFCTL_NFSW_DIS as i64),
        ("E1000_RFCTL_NFSR_DIS", E1000_RFCTL_NFSR_DIS as i64),
        ("E1000_RFCTL_NFS_VER_MASK", E1000_RFCTL_NFS_VER_MASK as i64),
        (
            "E1000_RFCTL_NFS_VER_SHIFT",
            E1000_RFCTL_NFS_VER_SHIFT as i64
        ),
        ("E1000_RFCTL_IPV6_DIS", E1000_RFCTL_IPV6_DIS as i64),
        (
            "E1000_RFCTL_IPV6_XSUM_DIS",
            E1000_RFCTL_IPV6_XSUM_DIS as i64
        ),
        ("E1000_RFCTL_ACK_DIS", E1000_RFCTL_ACK_DIS as i64),
        ("E1000_RFCTL_ACKD_DIS", E1000_RFCTL_ACKD_DIS as i64),
        ("E1000_RFCTL_IPFRSP_DIS", E1000_RFCTL_IPFRSP_DIS as i64),
        ("E1000_RFCTL_EXTEN", E1000_RFCTL_EXTEN as i64),
        ("E1000_RFCTL_IPV6_EX_DIS", E1000_RFCTL_IPV6_EX_DIS as i64),
        (
            "E1000_RFCTL_NEW_IPV6_EXT_DIS",
            E1000_RFCTL_NEW_IPV6_EXT_DIS as i64
        ),
        ("E1000_RXDCTL_PTHRESH", E1000_RXDCTL_PTHRESH as i64),
        ("E1000_RXDCTL_HTHRESH", E1000_RXDCTL_HTHRESH as i64),
        ("E1000_RXDCTL_WTHRESH", E1000_RXDCTL_WTHRESH as i64),
        (
            "E1000_RXDCTL_THRESH_UNIT_DESC",
            E1000_RXDCTL_THRESH_UNIT_DESC as i64
        ),
        (
            "E1000_RXDCTL_QUEUE_ENABLE",
            E1000_RXDCTL_QUEUE_ENABLE as i64
        ),
        ("E1000_EITR_ITR_INT_MASK", E1000_EITR_ITR_INT_MASK as i64),
        ("E1000_EITR_CNT_IGNR", E1000_EITR_CNT_IGNR as i64),
        ("E1000_EITR_INTERVAL", E1000_EITR_INTERVAL as i64),
        ("E1000_TXDCTL_PTHRESH", E1000_TXDCTL_PTHRESH as i64),
        ("E1000_TXDCTL_HTHRESH", E1000_TXDCTL_HTHRESH as i64),
        ("E1000_TXDCTL_WTHRESH", E1000_TXDCTL_WTHRESH as i64),
        ("E1000_TXDCTL_GRAN", E1000_TXDCTL_GRAN as i64),
        ("E1000_TXDCTL_LWTHRESH", E1000_TXDCTL_LWTHRESH as i64),
        (
            "E1000_TXDCTL_FULL_TX_DESC_WB",
            E1000_TXDCTL_FULL_TX_DESC_WB as i64
        ),
        ("E1000_TXDCTL_COUNT_DESC", E1000_TXDCTL_COUNT_DESC as i64),
        (
            "E1000_TXDCTL_QUEUE_ENABLE",
            E1000_TXDCTL_QUEUE_ENABLE as i64
        ),
        ("E1000_TXCW_FD", E1000_TXCW_FD as i64),
        ("E1000_TXCW_HD", E1000_TXCW_HD as i64),
        ("E1000_TXCW_PAUSE", E1000_TXCW_PAUSE as i64),
        ("E1000_TXCW_ASM_DIR", E1000_TXCW_ASM_DIR as i64),
        ("E1000_TXCW_PAUSE_MASK", E1000_TXCW_PAUSE_MASK as i64),
        ("E1000_TXCW_RF", E1000_TXCW_RF as i64),
        ("E1000_TXCW_NP", E1000_TXCW_NP as i64),
        ("E1000_TXCW_CW", E1000_TXCW_CW as i64),
        ("E1000_TXCW_TXC", E1000_TXCW_TXC as i64),
        ("E1000_TXCW_ANE", E1000_TXCW_ANE as i64),
        ("E1000_RXCW_CW", E1000_RXCW_CW as i64),
        ("E1000_RXCW_NC", E1000_RXCW_NC as i64),
        ("E1000_RXCW_IV", E1000_RXCW_IV as i64),
        ("E1000_RXCW_CC", E1000_RXCW_CC as i64),
        ("E1000_RXCW_C", E1000_RXCW_C as i64),
        ("E1000_RXCW_SYNCH", E1000_RXCW_SYNCH as i64),
        ("E1000_RXCW_ANC", E1000_RXCW_ANC as i64),
        ("E1000_TCTL_RST", E1000_TCTL_RST as i64),
        ("E1000_TCTL_EN", E1000_TCTL_EN as i64),
        ("E1000_TCTL_BCE", E1000_TCTL_BCE as i64),
        ("E1000_TCTL_PSP", E1000_TCTL_PSP as i64),
        ("E1000_TCTL_CT", E1000_TCTL_CT as i64),
        ("E1000_TCTL_COLD", E1000_TCTL_COLD as i64),
        ("E1000_TCTL_SWXOFF", E1000_TCTL_SWXOFF as i64),
        ("E1000_TCTL_PBE", E1000_TCTL_PBE as i64),
        ("E1000_TCTL_RTLC", E1000_TCTL_RTLC as i64),
        ("E1000_TCTL_NRTU", E1000_TCTL_NRTU as i64),
        ("E1000_TCTL_MULR", E1000_TCTL_MULR as i64),
        ("E1000_TCTL_EXT_BST_MASK", E1000_TCTL_EXT_BST_MASK as i64),
        ("E1000_TCTL_EXT_GCEX_MASK", E1000_TCTL_EXT_GCEX_MASK as i64),
        (
            "DEFAULT_80003ES2LAN_TCTL_EXT_GCEX",
            DEFAULT_80003ES2LAN_TCTL_EXT_GCEX as i64
        ),
        ("E1000_RXCSUM_PCSS_MASK", E1000_RXCSUM_PCSS_MASK as i64),
        ("E1000_RXCSUM_IPOFL", E1000_RXCSUM_IPOFL as i64),
        ("E1000_RXCSUM_TUOFL", E1000_RXCSUM_TUOFL as i64),
        ("E1000_RXCSUM_IPV6OFL", E1000_RXCSUM_IPV6OFL as i64),
        ("E1000_RXCSUM_IPPCSE", E1000_RXCSUM_IPPCSE as i64),
        ("E1000_RXCSUM_PCSD", E1000_RXCSUM_PCSD as i64),
        ("E1000_ADVTXD_DTYP_CTXT", E1000_ADVTXD_DTYP_CTXT as i64),
        ("E1000_ADVTXD_DTYP_DATA", E1000_ADVTXD_DTYP_DATA as i64),
        ("E1000_ADVTXD_DCMD_IFCS", E1000_ADVTXD_DCMD_IFCS as i64),
        ("E1000_ADVTXD_DCMD_DEXT", E1000_ADVTXD_DCMD_DEXT as i64),
        ("E1000_ADVTXD_DCMD_VLE", E1000_ADVTXD_DCMD_VLE as i64),
        ("E1000_ADVTXD_DCMD_TSE", E1000_ADVTXD_DCMD_TSE as i64),
        (
            "E1000_ADVTXD_PAYLEN_SHIFT",
            E1000_ADVTXD_PAYLEN_SHIFT as i64
        ),
        (
            "E1000_ADVTXD_MACLEN_SHIFT",
            E1000_ADVTXD_MACLEN_SHIFT as i64
        ),
        ("E1000_ADVTXD_VLAN_SHIFT", E1000_ADVTXD_VLAN_SHIFT as i64),
        ("E1000_ADVTXD_TUCMD_IPV4", E1000_ADVTXD_TUCMD_IPV4 as i64),
        ("E1000_ADVTXD_TUCMD_IPV6", E1000_ADVTXD_TUCMD_IPV6 as i64),
        (
            "E1000_ADVTXD_TUCMD_L4T_UDP",
            E1000_ADVTXD_TUCMD_L4T_UDP as i64
        ),
        (
            "E1000_ADVTXD_TUCMD_L4T_TCP",
            E1000_ADVTXD_TUCMD_L4T_TCP as i64
        ),
        ("E1000_ADVTXD_L4LEN_SHIFT", E1000_ADVTXD_L4LEN_SHIFT as i64),
        ("E1000_ADVTXD_MSS_SHIFT", E1000_ADVTXD_MSS_SHIFT as i64),
        ("E1000_MRQC_ENABLE_MASK", E1000_MRQC_ENABLE_MASK as i64),
        ("E1000_MRQC_ENABLE_RSS_2Q", E1000_MRQC_ENABLE_RSS_2Q as i64),
        (
            "E1000_MRQC_ENABLE_RSS_INT",
            E1000_MRQC_ENABLE_RSS_INT as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_MASK",
            E1000_MRQC_RSS_FIELD_MASK as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV4_TCP",
            E1000_MRQC_RSS_FIELD_IPV4_TCP as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV4",
            E1000_MRQC_RSS_FIELD_IPV4 as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6_TCP_EX",
            E1000_MRQC_RSS_FIELD_IPV6_TCP_EX as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6_EX",
            E1000_MRQC_RSS_FIELD_IPV6_EX as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6",
            E1000_MRQC_RSS_FIELD_IPV6 as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6_TCP",
            E1000_MRQC_RSS_FIELD_IPV6_TCP as i64
        ),
        ("E1000_WUC_APME", E1000_WUC_APME as i64),
        ("E1000_WUC_PME_EN", E1000_WUC_PME_EN as i64),
        ("E1000_WUC_PME_STATUS", E1000_WUC_PME_STATUS as i64),
        ("E1000_WUC_APMPME", E1000_WUC_APMPME as i64),
        ("E1000_WUC_SPM", E1000_WUC_SPM as i64),
        ("E1000_WUFC_LNKC", E1000_WUFC_LNKC as i64),
        ("E1000_WUFC_MAG", E1000_WUFC_MAG as i64),
        ("E1000_WUFC_EX", E1000_WUFC_EX as i64),
        ("E1000_WUFC_MC", E1000_WUFC_MC as i64),
        ("E1000_WUFC_BC", E1000_WUFC_BC as i64),
        ("E1000_WUFC_ARP", E1000_WUFC_ARP as i64),
        ("E1000_WUFC_IPV4", E1000_WUFC_IPV4 as i64),
        ("E1000_WUFC_IPV6", E1000_WUFC_IPV6 as i64),
        ("E1000_WUFC_IGNORE_TCO", E1000_WUFC_IGNORE_TCO as i64),
        ("E1000_WUFC_FLX0", E1000_WUFC_FLX0 as i64),
        ("E1000_WUFC_FLX1", E1000_WUFC_FLX1 as i64),
        ("E1000_WUFC_FLX2", E1000_WUFC_FLX2 as i64),
        ("E1000_WUFC_FLX3", E1000_WUFC_FLX3 as i64),
        ("E1000_WUFC_ALL_FILTERS", E1000_WUFC_ALL_FILTERS as i64),
        ("E1000_WUFC_FLX_OFFSET", E1000_WUFC_FLX_OFFSET as i64),
        ("E1000_WUFC_FLX_FILTERS", E1000_WUFC_FLX_FILTERS as i64),
        ("E1000_WUS_LNKC", E1000_WUS_LNKC as i64),
        ("E1000_WUS_MAG", E1000_WUS_MAG as i64),
        ("E1000_WUS_EX", E1000_WUS_EX as i64),
        ("E1000_WUS_MC", E1000_WUS_MC as i64),
        ("E1000_WUS_BC", E1000_WUS_BC as i64),
        ("E1000_WUS_ARP", E1000_WUS_ARP as i64),
        ("E1000_WUS_IPV4", E1000_WUS_IPV4 as i64),
        ("E1000_WUS_IPV6", E1000_WUS_IPV6 as i64),
        ("E1000_WUS_FLX0", E1000_WUS_FLX0 as i64),
        ("E1000_WUS_FLX1", E1000_WUS_FLX1 as i64),
        ("E1000_WUS_FLX2", E1000_WUS_FLX2 as i64),
        ("E1000_WUS_FLX3", E1000_WUS_FLX3 as i64),
        ("E1000_WUS_FLX_FILTERS", E1000_WUS_FLX_FILTERS as i64),
        (
            "E1000_TARC0_CB_MULTIQ_2_REQ",
            E1000_TARC0_CB_MULTIQ_2_REQ as i64
        ),
        (
            "E1000_TARC0_CB_MULTIQ_3_REQ",
            E1000_TARC0_CB_MULTIQ_3_REQ as i64
        ),
        ("E1000_MANC_SMBUS_EN", E1000_MANC_SMBUS_EN as i64),
        ("E1000_MANC_ASF_EN", E1000_MANC_ASF_EN as i64),
        ("E1000_MANC_R_ON_FORCE", E1000_MANC_R_ON_FORCE as i64),
        ("E1000_MANC_RMCP_EN", E1000_MANC_RMCP_EN as i64),
        ("E1000_MANC_0298_EN", E1000_MANC_0298_EN as i64),
        ("E1000_MANC_IPV4_EN", E1000_MANC_IPV4_EN as i64),
        ("E1000_MANC_IPV6_EN", E1000_MANC_IPV6_EN as i64),
        ("E1000_MANC_SNAP_EN", E1000_MANC_SNAP_EN as i64),
        ("E1000_MANC_ARP_EN", E1000_MANC_ARP_EN as i64),
        ("E1000_MANC_NEIGHBOR_EN", E1000_MANC_NEIGHBOR_EN as i64),
        ("E1000_MANC_ARP_RES_EN", E1000_MANC_ARP_RES_EN as i64),
        ("E1000_MANC_TCO_RESET", E1000_MANC_TCO_RESET as i64),
        ("E1000_MANC_RCV_TCO_EN", E1000_MANC_RCV_TCO_EN as i64),
        ("E1000_MANC_REPORT_STATUS", E1000_MANC_REPORT_STATUS as i64),
        ("E1000_MANC_RCV_ALL", E1000_MANC_RCV_ALL as i64),
        (
            "E1000_MANC_BLK_PHY_RST_ON_IDE",
            E1000_MANC_BLK_PHY_RST_ON_IDE as i64
        ),
        (
            "E1000_MANC_EN_MAC_ADDR_FILTER",
            E1000_MANC_EN_MAC_ADDR_FILTER as i64
        ),
        ("E1000_MANC_EN_MNG2HOST", E1000_MANC_EN_MNG2HOST as i64),
        (
            "E1000_MANC_EN_IP_ADDR_FILTER",
            E1000_MANC_EN_IP_ADDR_FILTER as i64
        ),
        (
            "E1000_MANC_EN_XSUM_FILTER",
            E1000_MANC_EN_XSUM_FILTER as i64
        ),
        ("E1000_MANC_BR_EN", E1000_MANC_BR_EN as i64),
        ("E1000_MANC_SMB_REQ", E1000_MANC_SMB_REQ as i64),
        ("E1000_MANC_SMB_GNT", E1000_MANC_SMB_GNT as i64),
        ("E1000_MANC_SMB_CLK_IN", E1000_MANC_SMB_CLK_IN as i64),
        ("E1000_MANC_SMB_DATA_IN", E1000_MANC_SMB_DATA_IN as i64),
        ("E1000_MANC_SMB_DATA_OUT", E1000_MANC_SMB_DATA_OUT as i64),
        ("E1000_MANC_SMB_CLK_OUT", E1000_MANC_SMB_CLK_OUT as i64),
        (
            "E1000_MANC_SMB_DATA_OUT_SHIFT",
            E1000_MANC_SMB_DATA_OUT_SHIFT as i64
        ),
        (
            "E1000_MANC_SMB_CLK_OUT_SHIFT",
            E1000_MANC_SMB_CLK_OUT_SHIFT as i64
        ),
        ("E1000_SWSM_SMBI", E1000_SWSM_SMBI as i64),
        ("E1000_SWSM_SWESMBI", E1000_SWSM_SWESMBI as i64),
        ("E1000_SWSM_WMNG", E1000_SWSM_WMNG as i64),
        ("E1000_SWSM_DRV_LOAD", E1000_SWSM_DRV_LOAD as i64),
        ("E1000_H2ME_ULP", E1000_H2ME_ULP as i64),
        (
            "E1000_H2ME_ENFORCE_SETTINGS",
            E1000_H2ME_ENFORCE_SETTINGS as i64
        ),
        ("E1000_FWSM_MODE_MASK", E1000_FWSM_MODE_MASK as i64),
        ("E1000_FWSM_MODE_SHIFT", E1000_FWSM_MODE_SHIFT as i64),
        ("E1000_FWSM_ULP_CFG_DONE", E1000_FWSM_ULP_CFG_DONE as i64),
        ("E1000_FWSM_FW_VALID", E1000_FWSM_FW_VALID as i64),
        ("E1000_FWSM_RSPCIPHY", E1000_FWSM_RSPCIPHY as i64),
        ("E1000_FWSM_DISSW", E1000_FWSM_DISSW as i64),
        ("E1000_FWSM_SKUSEL_MASK", E1000_FWSM_SKUSEL_MASK as i64),
        ("E1000_FWSM_SKUEL_SHIFT", E1000_FWSM_SKUEL_SHIFT as i64),
        ("E1000_FWSM_SKUSEL_EMB", E1000_FWSM_SKUSEL_EMB as i64),
        ("E1000_FWSM_SKUSEL_CONS", E1000_FWSM_SKUSEL_CONS as i64),
        (
            "E1000_FWSM_SKUSEL_PERF_100",
            E1000_FWSM_SKUSEL_PERF_100 as i64
        ),
        (
            "E1000_FWSM_SKUSEL_PERF_GBE",
            E1000_FWSM_SKUSEL_PERF_GBE as i64
        ),
        ("E1000_FFLT_DBG_INVC", E1000_FFLT_DBG_INVC as i64),
        ("E1000_HICR_EN", E1000_HICR_EN as i64),
        ("E1000_HICR_C", E1000_HICR_C as i64),
        ("E1000_HICR_SV", E1000_HICR_SV as i64),
        ("E1000_HICR_FWR", E1000_HICR_FWR as i64),
        ("E1000_HI_MAX_DATA_LENGTH", E1000_HI_MAX_DATA_LENGTH as i64),
        (
            "E1000_HI_MAX_BLOCK_BYTE_LENGTH",
            E1000_HI_MAX_BLOCK_BYTE_LENGTH as i64
        ),
        (
            "E1000_HI_MAX_BLOCK_DWORD_LENGTH",
            E1000_HI_MAX_BLOCK_DWORD_LENGTH as i64
        ),
        ("E1000_HI_COMMAND_TIMEOUT", E1000_HI_COMMAND_TIMEOUT as i64),
        ("E1000_HSMC0R_CLKIN", E1000_HSMC0R_CLKIN as i64),
        ("E1000_HSMC0R_DATAIN", E1000_HSMC0R_DATAIN as i64),
        ("E1000_HSMC0R_DATAOUT", E1000_HSMC0R_DATAOUT as i64),
        ("E1000_HSMC0R_CLKOUT", E1000_HSMC0R_CLKOUT as i64),
        ("E1000_HSMC1R_CLKIN", E1000_HSMC1R_CLKIN as i64),
        ("E1000_HSMC1R_DATAIN", E1000_HSMC1R_DATAIN as i64),
        ("E1000_HSMC1R_DATAOUT", E1000_HSMC1R_DATAOUT as i64),
        ("E1000_HSMC1R_CLKOUT", E1000_HSMC1R_CLKOUT as i64),
        ("E1000_FWSTS_FWS_MASK", E1000_FWSTS_FWS_MASK as i64),
        ("E1000_WUPL_LENGTH_MASK", E1000_WUPL_LENGTH_MASK as i64),
        ("E1000_MDALIGN", E1000_MDALIGN as i64),
        ("E1000_MDICNFG_EXT_MDIO", E1000_MDICNFG_EXT_MDIO as i64),
        ("E1000_MDICNFG_COM_MDIO", E1000_MDICNFG_COM_MDIO as i64),
        ("E1000_MDICNFG_PHY_MASK", E1000_MDICNFG_PHY_MASK as i64),
        ("E1000_MDICNFG_PHY_SHIFT", E1000_MDICNFG_PHY_SHIFT as i64),
        ("E1000_IPCNFG_EEE_1G_AN", E1000_IPCNFG_EEE_1G_AN as i64),
        ("E1000_IPCNFG_EEE_100M_AN", E1000_IPCNFG_EEE_100M_AN as i64),
        ("E1000_EEER_TX_LPI_EN", E1000_EEER_TX_LPI_EN as i64),
        ("E1000_EEER_RX_LPI_EN", E1000_EEER_RX_LPI_EN as i64),
        ("E1000_EEER_LPI_FC", E1000_EEER_LPI_FC as i64),
        ("E1000_EEER_EEE_NEG", E1000_EEER_EEE_NEG as i64),
        ("E1000_EEER_RX_LPI_STATUS", E1000_EEER_RX_LPI_STATUS as i64),
        ("E1000_EEER_TX_LPI_STATUS", E1000_EEER_TX_LPI_STATUS as i64),
        ("E1000_GCR_RXD_NO_SNOOP", E1000_GCR_RXD_NO_SNOOP as i64),
        (
            "E1000_GCR_RXDSCW_NO_SNOOP",
            E1000_GCR_RXDSCW_NO_SNOOP as i64
        ),
        (
            "E1000_GCR_RXDSCR_NO_SNOOP",
            E1000_GCR_RXDSCR_NO_SNOOP as i64
        ),
        ("E1000_GCR_TXD_NO_SNOOP", E1000_GCR_TXD_NO_SNOOP as i64),
        (
            "E1000_GCR_TXDSCW_NO_SNOOP",
            E1000_GCR_TXDSCW_NO_SNOOP as i64
        ),
        (
            "E1000_GCR_TXDSCR_NO_SNOOP",
            E1000_GCR_TXDSCR_NO_SNOOP as i64
        ),
        (
            "E1000_GCR_CMPL_TMOUT_MASK",
            E1000_GCR_CMPL_TMOUT_MASK as i64
        ),
        (
            "E1000_GCR_CMPL_TMOUT_10ms",
            E1000_GCR_CMPL_TMOUT_10ms as i64
        ),
        (
            "E1000_GCR_CMPL_TMOUT_RESEND",
            E1000_GCR_CMPL_TMOUT_RESEND as i64
        ),
        ("E1000_GCR_CAP_VER2", E1000_GCR_CAP_VER2 as i64),
        ("PCI_EX_NO_SNOOP_ALL", PCI_EX_NO_SNOOP_ALL as i64),
        ("PCI_EX_82566_SNOOP_ALL", PCI_EX_82566_SNOOP_ALL as i64),
        (
            "E1000_GCR_L1_ACT_WITHOUT_L0S_RX",
            E1000_GCR_L1_ACT_WITHOUT_L0S_RX as i64
        ),
        (
            "E1000_FACTPS_FUNC0_POWER_STATE_MASK",
            E1000_FACTPS_FUNC0_POWER_STATE_MASK as i64
        ),
        ("E1000_FACTPS_LAN0_VALID", E1000_FACTPS_LAN0_VALID as i64),
        (
            "E1000_FACTPS_FUNC0_AUX_EN",
            E1000_FACTPS_FUNC0_AUX_EN as i64
        ),
        (
            "E1000_FACTPS_FUNC1_POWER_STATE_MASK",
            E1000_FACTPS_FUNC1_POWER_STATE_MASK as i64
        ),
        (
            "E1000_FACTPS_FUNC1_POWER_STATE_SHIFT",
            E1000_FACTPS_FUNC1_POWER_STATE_SHIFT as i64
        ),
        ("E1000_FACTPS_LAN1_VALID", E1000_FACTPS_LAN1_VALID as i64),
        (
            "E1000_FACTPS_FUNC1_AUX_EN",
            E1000_FACTPS_FUNC1_AUX_EN as i64
        ),
        (
            "E1000_FACTPS_FUNC2_POWER_STATE_MASK",
            E1000_FACTPS_FUNC2_POWER_STATE_MASK as i64
        ),
        (
            "E1000_FACTPS_FUNC2_POWER_STATE_SHIFT",
            E1000_FACTPS_FUNC2_POWER_STATE_SHIFT as i64
        ),
        ("E1000_FACTPS_IDE_ENABLE", E1000_FACTPS_IDE_ENABLE as i64),
        (
            "E1000_FACTPS_FUNC2_AUX_EN",
            E1000_FACTPS_FUNC2_AUX_EN as i64
        ),
        (
            "E1000_FACTPS_FUNC3_POWER_STATE_MASK",
            E1000_FACTPS_FUNC3_POWER_STATE_MASK as i64
        ),
        (
            "E1000_FACTPS_FUNC3_POWER_STATE_SHIFT",
            E1000_FACTPS_FUNC3_POWER_STATE_SHIFT as i64
        ),
        ("E1000_FACTPS_SP_ENABLE", E1000_FACTPS_SP_ENABLE as i64),
        (
            "E1000_FACTPS_FUNC3_AUX_EN",
            E1000_FACTPS_FUNC3_AUX_EN as i64
        ),
        (
            "E1000_FACTPS_FUNC4_POWER_STATE_MASK",
            E1000_FACTPS_FUNC4_POWER_STATE_MASK as i64
        ),
        (
            "E1000_FACTPS_FUNC4_POWER_STATE_SHIFT",
            E1000_FACTPS_FUNC4_POWER_STATE_SHIFT as i64
        ),
        ("E1000_FACTPS_IPMI_ENABLE", E1000_FACTPS_IPMI_ENABLE as i64),
        (
            "E1000_FACTPS_FUNC4_AUX_EN",
            E1000_FACTPS_FUNC4_AUX_EN as i64
        ),
        ("E1000_FACTPS_MNGCG", E1000_FACTPS_MNGCG as i64),
        (
            "E1000_FACTPS_LAN_FUNC_SEL",
            E1000_FACTPS_LAN_FUNC_SEL as i64
        ),
        (
            "E1000_FACTPS_PM_STATE_CHANGED",
            E1000_FACTPS_PM_STATE_CHANGED as i64
        ),
        ("E1000_IVAR_VALID", E1000_IVAR_VALID as i64),
        ("E1000_GPIE_NSICR", E1000_GPIE_NSICR as i64),
        ("E1000_GPIE_MSIX_MODE", E1000_GPIE_MSIX_MODE as i64),
        ("E1000_GPIE_EIAME", E1000_GPIE_EIAME as i64),
        ("E1000_GPIE_PBA", E1000_GPIE_PBA as i64),
        ("E1000_MRQC_ENABLE_RSS_4Q", E1000_MRQC_ENABLE_RSS_4Q as i64),
        ("E1000_MRQC_ENABLE_VMDQ", E1000_MRQC_ENABLE_VMDQ as i64),
        (
            "E1000_MRQC_ENABLE_VMDQ_RSS_2Q",
            E1000_MRQC_ENABLE_VMDQ_RSS_2Q as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV4_UDP",
            E1000_MRQC_RSS_FIELD_IPV4_UDP as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6_UDP",
            E1000_MRQC_RSS_FIELD_IPV6_UDP as i64
        ),
        (
            "E1000_MRQC_RSS_FIELD_IPV6_UDP_EX",
            E1000_MRQC_RSS_FIELD_IPV6_UDP_EX as i64
        ),
        ("E1000_MRQC_ENABLE_RSS_8Q", E1000_MRQC_ENABLE_RSS_8Q as i64),
        (
            "E1000_SRRCTL_BSIZEPKT_SHIFT",
            E1000_SRRCTL_BSIZEPKT_SHIFT as i64
        ),
        (
            "E1000_SRRCTL_BSIZEHDRSIZE_MASK",
            E1000_SRRCTL_BSIZEHDRSIZE_MASK as i64
        ),
        (
            "E1000_SRRCTL_BSIZEHDRSIZE_SHIFT",
            E1000_SRRCTL_BSIZEHDRSIZE_SHIFT as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_LEGACY",
            E1000_SRRCTL_DESCTYPE_LEGACY as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_ADV_ONEBUF",
            E1000_SRRCTL_DESCTYPE_ADV_ONEBUF as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_HDR_SPLIT",
            E1000_SRRCTL_DESCTYPE_HDR_SPLIT as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS",
            E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_HDR_REPLICATION",
            E1000_SRRCTL_DESCTYPE_HDR_REPLICATION as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT",
            E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT as i64
        ),
        (
            "E1000_SRRCTL_DESCTYPE_MASK",
            E1000_SRRCTL_DESCTYPE_MASK as i64
        ),
        ("E1000_SRRCTL_TIMESTAMP", E1000_SRRCTL_TIMESTAMP as i64),
        ("E1000_SRRCTL_DROP_EN", E1000_SRRCTL_DROP_EN as i64),
        ("E1000_WUFC_FLEX_HQ", E1000_WUFC_FLEX_HQ as i64),
        ("PCI_EX_LINK_STATUS", PCI_EX_LINK_STATUS as i64),
        ("PCI_EX_LINK_WIDTH_MASK", PCI_EX_LINK_WIDTH_MASK as i64),
        ("PCI_EX_LINK_WIDTH_SHIFT", PCI_EX_LINK_WIDTH_SHIFT as i64),
        ("PCI_EX_DEVICE_CONTROL2", PCI_EX_DEVICE_CONTROL2 as i64),
        (
            "PCI_EX_DEVICE_CONTROL2_16ms",
            PCI_EX_DEVICE_CONTROL2_16ms as i64
        ),
        (
            "EEPROM_READ_OPCODE_MICROWIRE",
            EEPROM_READ_OPCODE_MICROWIRE as i64
        ),
        (
            "EEPROM_WRITE_OPCODE_MICROWIRE",
            EEPROM_WRITE_OPCODE_MICROWIRE as i64
        ),
        (
            "EEPROM_ERASE_OPCODE_MICROWIRE",
            EEPROM_ERASE_OPCODE_MICROWIRE as i64
        ),
        (
            "EEPROM_EWEN_OPCODE_MICROWIRE",
            EEPROM_EWEN_OPCODE_MICROWIRE as i64
        ),
        (
            "EEPROM_EWDS_OPCODE_MICROWIRE",
            EEPROM_EWDS_OPCODE_MICROWIRE as i64
        ),
        ("EEPROM_MAX_RETRY_SPI", EEPROM_MAX_RETRY_SPI as i64),
        ("EEPROM_READ_OPCODE_SPI", EEPROM_READ_OPCODE_SPI as i64),
        ("EEPROM_WRITE_OPCODE_SPI", EEPROM_WRITE_OPCODE_SPI as i64),
        ("EEPROM_A8_OPCODE_SPI", EEPROM_A8_OPCODE_SPI as i64),
        ("EEPROM_WREN_OPCODE_SPI", EEPROM_WREN_OPCODE_SPI as i64),
        ("EEPROM_WRDI_OPCODE_SPI", EEPROM_WRDI_OPCODE_SPI as i64),
        ("EEPROM_RDSR_OPCODE_SPI", EEPROM_RDSR_OPCODE_SPI as i64),
        ("EEPROM_WRSR_OPCODE_SPI", EEPROM_WRSR_OPCODE_SPI as i64),
        (
            "EEPROM_ERASE4K_OPCODE_SPI",
            EEPROM_ERASE4K_OPCODE_SPI as i64
        ),
        (
            "EEPROM_ERASE64K_OPCODE_SPI",
            EEPROM_ERASE64K_OPCODE_SPI as i64
        ),
        (
            "EEPROM_ERASE256_OPCODE_SPI",
            EEPROM_ERASE256_OPCODE_SPI as i64
        ),
        ("EEPROM_WORD_SIZE_SHIFT", EEPROM_WORD_SIZE_SHIFT as i64),
        (
            "EEPROM_WORD_SIZE_SHIFT_MAX",
            EEPROM_WORD_SIZE_SHIFT_MAX as i64
        ),
        ("EEPROM_SIZE_SHIFT", EEPROM_SIZE_SHIFT as i64),
        ("EEPROM_SIZE_MASK", EEPROM_SIZE_MASK as i64),
        ("EEPROM_MAC_ADDR_WORD0", EEPROM_MAC_ADDR_WORD0 as i64),
        ("EEPROM_MAC_ADDR_WORD1", EEPROM_MAC_ADDR_WORD1 as i64),
        ("EEPROM_MAC_ADDR_WORD2", EEPROM_MAC_ADDR_WORD2 as i64),
        ("EEPROM_COMPAT", EEPROM_COMPAT as i64),
        ("EEPROM_ID_LED_SETTINGS", EEPROM_ID_LED_SETTINGS as i64),
        ("EEPROM_VERSION", EEPROM_VERSION as i64),
        ("EEPROM_SERDES_AMPLITUDE", EEPROM_SERDES_AMPLITUDE as i64),
        ("EEPROM_PHY_CLASS_WORD", EEPROM_PHY_CLASS_WORD as i64),
        ("EEPROM_INIT_CONTROL1_REG", EEPROM_INIT_CONTROL1_REG as i64),
        ("EEPROM_INIT_CONTROL2_REG", EEPROM_INIT_CONTROL2_REG as i64),
        (
            "EEPROM_SWDEF_PINS_CTRL_PORT_1",
            EEPROM_SWDEF_PINS_CTRL_PORT_1 as i64
        ),
        ("EEPROM_INIT_CONTROL4_REG", EEPROM_INIT_CONTROL4_REG as i64),
        (
            "EEPROM_INIT_CONTROL3_PORT_B",
            EEPROM_INIT_CONTROL3_PORT_B as i64
        ),
        ("EEPROM_INIT_3GIO_3", EEPROM_INIT_3GIO_3 as i64),
        ("EEPROM_LED_1_CFG", EEPROM_LED_1_CFG as i64),
        ("EEPROM_LED_0_2_CFG", EEPROM_LED_0_2_CFG as i64),
        (
            "EEPROM_SWDEF_PINS_CTRL_PORT_0",
            EEPROM_SWDEF_PINS_CTRL_PORT_0 as i64
        ),
        (
            "EEPROM_INIT_CONTROL3_PORT_A",
            EEPROM_INIT_CONTROL3_PORT_A as i64
        ),
        ("EEPROM_CFG", EEPROM_CFG as i64),
        ("EEPROM_FLASH_VERSION", EEPROM_FLASH_VERSION as i64),
        ("EEPROM_CHECKSUM_REG", EEPROM_CHECKSUM_REG as i64),
        ("EEPROM_COMPAT_VALID_CSUM", EEPROM_COMPAT_VALID_CSUM as i64),
        ("EEPROM_FUTURE_INIT_WORD1", EEPROM_FUTURE_INIT_WORD1 as i64),
        (
            "EEPROM_FUTURE_INIT_WORD1_VALID_CSUM",
            EEPROM_FUTURE_INIT_WORD1_VALID_CSUM as i64
        ),
        (
            "E1000_NVM_CFG_DONE_PORT_0",
            E1000_NVM_CFG_DONE_PORT_0 as i64
        ),
        (
            "E1000_NVM_CFG_DONE_PORT_1",
            E1000_NVM_CFG_DONE_PORT_1 as i64
        ),
        (
            "E1000_NVM_CFG_DONE_PORT_2",
            E1000_NVM_CFG_DONE_PORT_2 as i64
        ),
        (
            "E1000_NVM_CFG_DONE_PORT_3",
            E1000_NVM_CFG_DONE_PORT_3 as i64
        ),
        ("NVM_WORD24_COM_MDIO", NVM_WORD24_COM_MDIO as i64),
        ("NVM_WORD24_EXT_MDIO", NVM_WORD24_EXT_MDIO as i64),
        ("ID_LED_RESERVED_0000", ID_LED_RESERVED_0000 as i64),
        ("ID_LED_RESERVED_FFFF", ID_LED_RESERVED_FFFF as i64),
        ("ID_LED_RESERVED_82573", ID_LED_RESERVED_82573 as i64),
        ("ID_LED_DEFAULT_82573", ID_LED_DEFAULT_82573 as i64),
        ("ID_LED_DEFAULT", ID_LED_DEFAULT as i64),
        ("ID_LED_DEFAULT_ICH8LAN", ID_LED_DEFAULT_ICH8LAN as i64),
        ("ID_LED_DEF1_DEF2", ID_LED_DEF1_DEF2 as i64),
        ("ID_LED_DEF1_ON2", ID_LED_DEF1_ON2 as i64),
        ("ID_LED_DEF1_OFF2", ID_LED_DEF1_OFF2 as i64),
        ("ID_LED_ON1_DEF2", ID_LED_ON1_DEF2 as i64),
        ("ID_LED_ON1_ON2", ID_LED_ON1_ON2 as i64),
        ("ID_LED_ON1_OFF2", ID_LED_ON1_OFF2 as i64),
        ("ID_LED_OFF1_DEF2", ID_LED_OFF1_DEF2 as i64),
        ("ID_LED_OFF1_ON2", ID_LED_OFF1_ON2 as i64),
        ("ID_LED_OFF1_OFF2", ID_LED_OFF1_OFF2 as i64),
        ("IGP_ACTIVITY_LED_MASK", IGP_ACTIVITY_LED_MASK as i64),
        ("IGP_ACTIVITY_LED_ENABLE", IGP_ACTIVITY_LED_ENABLE as i64),
        ("IGP_LED3_MODE", IGP_LED3_MODE as i64),
        (
            "EEPROM_SERDES_AMPLITUDE_MASK",
            EEPROM_SERDES_AMPLITUDE_MASK as i64
        ),
        ("EEPROM_PHY_CLASS_A", EEPROM_PHY_CLASS_A as i64),
        ("EEPROM_WORD0A_ILOS", EEPROM_WORD0A_ILOS as i64),
        ("EEPROM_WORD0A_SWDPIO", EEPROM_WORD0A_SWDPIO as i64),
        ("EEPROM_WORD0A_LRST", EEPROM_WORD0A_LRST as i64),
        ("EEPROM_WORD0A_FD", EEPROM_WORD0A_FD as i64),
        ("EEPROM_WORD0A_66MHZ", EEPROM_WORD0A_66MHZ as i64),
        ("EEPROM_WORD0F_PAUSE_MASK", EEPROM_WORD0F_PAUSE_MASK as i64),
        ("EEPROM_WORD0F_PAUSE", EEPROM_WORD0F_PAUSE as i64),
        ("EEPROM_WORD0F_ASM_DIR", EEPROM_WORD0F_ASM_DIR as i64),
        ("EEPROM_WORD0F_ANE", EEPROM_WORD0F_ANE as i64),
        ("EEPROM_WORD0F_SWPDIO_EXT", EEPROM_WORD0F_SWPDIO_EXT as i64),
        ("EEPROM_WORD0F_LPLU", EEPROM_WORD0F_LPLU as i64),
        (
            "EEPROM_WORD1020_GIGA_DISABLE",
            EEPROM_WORD1020_GIGA_DISABLE as i64
        ),
        (
            "EEPROM_WORD1020_GIGA_DISABLE_NON_D0A",
            EEPROM_WORD1020_GIGA_DISABLE_NON_D0A as i64
        ),
        ("EEPROM_WORD1A_ASPM_MASK", EEPROM_WORD1A_ASPM_MASK as i64),
        ("EEPROM_SUM", EEPROM_SUM as i64),
        (
            "EEPROM_NODE_ADDRESS_BYTE_0",
            EEPROM_NODE_ADDRESS_BYTE_0 as i64
        ),
        ("EEPROM_PBA_BYTE_1", EEPROM_PBA_BYTE_1 as i64),
        ("EEPROM_RESERVED_WORD", EEPROM_RESERVED_WORD as i64),
        ("PBA_SIZE", PBA_SIZE as i64),
        (
            "E1000_COLLISION_THRESHOLD",
            E1000_COLLISION_THRESHOLD as i64
        ),
        ("E1000_CT_SHIFT", E1000_CT_SHIFT as i64),
        ("E1000_COLLISION_DISTANCE", E1000_COLLISION_DISTANCE as i64),
        (
            "E1000_COLLISION_DISTANCE_82542",
            E1000_COLLISION_DISTANCE_82542 as i64
        ),
        (
            "E1000_FDX_COLLISION_DISTANCE",
            E1000_FDX_COLLISION_DISTANCE as i64
        ),
        (
            "E1000_HDX_COLLISION_DISTANCE",
            E1000_HDX_COLLISION_DISTANCE as i64
        ),
        ("E1000_COLD_SHIFT", E1000_COLD_SHIFT as i64),
        (
            "REQ_TX_DESCRIPTOR_MULTIPLE",
            REQ_TX_DESCRIPTOR_MULTIPLE as i64
        ),
        (
            "REQ_RX_DESCRIPTOR_MULTIPLE",
            REQ_RX_DESCRIPTOR_MULTIPLE as i64
        ),
        ("DEFAULT_82542_TIPG_IPGT", DEFAULT_82542_TIPG_IPGT as i64),
        (
            "DEFAULT_82543_TIPG_IPGT_FIBER",
            DEFAULT_82543_TIPG_IPGT_FIBER as i64
        ),
        (
            "DEFAULT_82543_TIPG_IPGT_COPPER",
            DEFAULT_82543_TIPG_IPGT_COPPER as i64
        ),
        ("E1000_TIPG_IPGT_MASK", E1000_TIPG_IPGT_MASK as i64),
        ("E1000_TIPG_IPGR1_MASK", E1000_TIPG_IPGR1_MASK as i64),
        ("E1000_TIPG_IPGR2_MASK", E1000_TIPG_IPGR2_MASK as i64),
        ("DEFAULT_82542_TIPG_IPGR1", DEFAULT_82542_TIPG_IPGR1 as i64),
        ("DEFAULT_82543_TIPG_IPGR1", DEFAULT_82543_TIPG_IPGR1 as i64),
        ("E1000_TIPG_IPGR1_SHIFT", E1000_TIPG_IPGR1_SHIFT as i64),
        ("DEFAULT_82542_TIPG_IPGR2", DEFAULT_82542_TIPG_IPGR2 as i64),
        ("DEFAULT_82543_TIPG_IPGR2", DEFAULT_82543_TIPG_IPGR2 as i64),
        (
            "DEFAULT_80003ES2LAN_TIPG_IPGR2",
            DEFAULT_80003ES2LAN_TIPG_IPGR2 as i64
        ),
        ("E1000_TIPG_IPGR2_SHIFT", E1000_TIPG_IPGR2_SHIFT as i64),
        (
            "DEFAULT_80003ES2LAN_TIPG_IPGT_10_100",
            DEFAULT_80003ES2LAN_TIPG_IPGT_10_100 as i64
        ),
        (
            "DEFAULT_80003ES2LAN_TIPG_IPGT_1000",
            DEFAULT_80003ES2LAN_TIPG_IPGT_1000 as i64
        ),
        ("E1000_TXDMAC_DPP", E1000_TXDMAC_DPP as i64),
        ("TX_THRESHOLD_START", TX_THRESHOLD_START as i64),
        ("TX_THRESHOLD_INCREMENT", TX_THRESHOLD_INCREMENT as i64),
        ("TX_THRESHOLD_DECREMENT", TX_THRESHOLD_DECREMENT as i64),
        ("TX_THRESHOLD_STOP", TX_THRESHOLD_STOP as i64),
        ("TX_THRESHOLD_DISABLE", TX_THRESHOLD_DISABLE as i64),
        ("TX_THRESHOLD_TIMER_MS", TX_THRESHOLD_TIMER_MS as i64),
        ("MIN_NUM_XMITS", MIN_NUM_XMITS as i64),
        ("IFS_MAX", IFS_MAX as i64),
        ("IFS_STEP", IFS_STEP as i64),
        ("IFS_MIN", IFS_MIN as i64),
        ("IFS_RATIO", IFS_RATIO as i64),
        (
            "E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE",
            E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE as i64
        ),
        (
            "E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE",
            E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE as i64
        ),
        (
            "E1000_EXTCNF_CTRL_D_UD_ENABLE",
            E1000_EXTCNF_CTRL_D_UD_ENABLE as i64
        ),
        (
            "E1000_EXTCNF_CTRL_D_UD_LATENCY",
            E1000_EXTCNF_CTRL_D_UD_LATENCY as i64
        ),
        (
            "E1000_EXTCNF_CTRL_D_UD_OWNER",
            E1000_EXTCNF_CTRL_D_UD_OWNER as i64
        ),
        (
            "E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP",
            E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP as i64
        ),
        (
            "E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP",
            E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP as i64
        ),
        (
            "E1000_EXTCNF_CTRL_EXT_CNF_POINTER",
            E1000_EXTCNF_CTRL_EXT_CNF_POINTER as i64
        ),
        (
            "E1000_EXTCNF_SIZE_EXT_PHY_LENGTH",
            E1000_EXTCNF_SIZE_EXT_PHY_LENGTH as i64
        ),
        (
            "E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH",
            E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH as i64
        ),
        (
            "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH",
            E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH as i64
        ),
        (
            "E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE",
            E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE as i64
        ),
        ("E1000_EXTCNF_CTRL_SWFLAG", E1000_EXTCNF_CTRL_SWFLAG as i64),
        (
            "E1000_EXTCNF_CTRL_GATE_PHY_CFG",
            E1000_EXTCNF_CTRL_GATE_PHY_CFG as i64
        ),
        ("E1000_PBA_8K", E1000_PBA_8K as i64),
        ("E1000_PBA_10K", E1000_PBA_10K as i64),
        ("E1000_PBA_12K", E1000_PBA_12K as i64),
        ("E1000_PBA_14K", E1000_PBA_14K as i64),
        ("E1000_PBA_16K", E1000_PBA_16K as i64),
        ("E1000_PBA_20K", E1000_PBA_20K as i64),
        ("E1000_PBA_22K", E1000_PBA_22K as i64),
        ("E1000_PBA_24K", E1000_PBA_24K as i64),
        ("E1000_PBA_26K", E1000_PBA_26K as i64),
        ("E1000_PBA_30K", E1000_PBA_30K as i64),
        ("E1000_PBA_32K", E1000_PBA_32K as i64),
        ("E1000_PBA_34K", E1000_PBA_34K as i64),
        ("E1000_PBA_38K", E1000_PBA_38K as i64),
        ("E1000_PBA_40K", E1000_PBA_40K as i64),
        ("E1000_PBA_48K", E1000_PBA_48K as i64),
        ("E1000_PBS_16K", E1000_PBS_16K as i64),
        ("FLOW_CONTROL_ADDRESS_LOW", FLOW_CONTROL_ADDRESS_LOW as i64),
        (
            "FLOW_CONTROL_ADDRESS_HIGH",
            FLOW_CONTROL_ADDRESS_HIGH as i64
        ),
        ("FLOW_CONTROL_TYPE", FLOW_CONTROL_TYPE as i64),
        ("FC_DEFAULT_HI_THRESH", FC_DEFAULT_HI_THRESH as i64),
        ("FC_DEFAULT_LO_THRESH", FC_DEFAULT_LO_THRESH as i64),
        ("FC_DEFAULT_TX_TIMER", FC_DEFAULT_TX_TIMER as i64),
        ("PCIX_COMMAND_REGISTER", PCIX_COMMAND_REGISTER as i64),
        ("PCIX_STATUS_REGISTER_LO", PCIX_STATUS_REGISTER_LO as i64),
        ("PCIX_STATUS_REGISTER_HI", PCIX_STATUS_REGISTER_HI as i64),
        ("PCIX_COMMAND_MMRBC_MASK", PCIX_COMMAND_MMRBC_MASK as i64),
        ("PCIX_COMMAND_MMRBC_SHIFT", PCIX_COMMAND_MMRBC_SHIFT as i64),
        (
            "PCIX_STATUS_HI_MMRBC_MASK",
            PCIX_STATUS_HI_MMRBC_MASK as i64
        ),
        (
            "PCIX_STATUS_HI_MMRBC_SHIFT",
            PCIX_STATUS_HI_MMRBC_SHIFT as i64
        ),
        ("PCIX_STATUS_HI_MMRBC_4K", PCIX_STATUS_HI_MMRBC_4K as i64),
        ("PCIX_STATUS_HI_MMRBC_2K", PCIX_STATUS_HI_MMRBC_2K as i64),
        ("PAUSE_SHIFT", PAUSE_SHIFT as i64),
        ("SWDPIO_SHIFT", SWDPIO_SHIFT as i64),
        ("SWDPIO__EXT_SHIFT", SWDPIO__EXT_SHIFT as i64),
        ("ILOS_SHIFT", ILOS_SHIFT as i64),
        (
            "RECEIVE_BUFFER_ALIGN_SIZE",
            RECEIVE_BUFFER_ALIGN_SIZE as i64
        ),
        ("LINK_UP_TIMEOUT", LINK_UP_TIMEOUT as i64),
        ("MASTER_DISABLE_TIMEOUT", MASTER_DISABLE_TIMEOUT as i64),
        ("AUTO_READ_DONE_TIMEOUT", AUTO_READ_DONE_TIMEOUT as i64),
        ("PHY_CFG_TIMEOUT", PHY_CFG_TIMEOUT as i64),
        ("SW_FLAG_TIMEOUT", SW_FLAG_TIMEOUT as i64),
        ("E1000_TX_BUFFER_SIZE", E1000_TX_BUFFER_SIZE as i64),
        ("CARRIER_EXTENSION", CARRIER_EXTENSION as i64),
        ("E1000_CTRL_PHY_RESET_DIR", E1000_CTRL_PHY_RESET_DIR as i64),
        ("E1000_CTRL_PHY_RESET", E1000_CTRL_PHY_RESET as i64),
        ("E1000_CTRL_MDIO_DIR", E1000_CTRL_MDIO_DIR as i64),
        ("E1000_CTRL_MDIO", E1000_CTRL_MDIO as i64),
        ("E1000_CTRL_MDC_DIR", E1000_CTRL_MDC_DIR as i64),
        ("E1000_CTRL_MDC", E1000_CTRL_MDC as i64),
        (
            "E1000_CTRL_PHY_RESET_DIR4",
            E1000_CTRL_PHY_RESET_DIR4 as i64
        ),
        ("E1000_CTRL_PHY_RESET4", E1000_CTRL_PHY_RESET4 as i64),
        ("PHY_CTRL", PHY_CTRL as i64),
        ("PHY_STATUS", PHY_STATUS as i64),
        ("PHY_ID1", PHY_ID1 as i64),
        ("PHY_ID2", PHY_ID2 as i64),
        ("PHY_AUTONEG_ADV", PHY_AUTONEG_ADV as i64),
        ("PHY_LP_ABILITY", PHY_LP_ABILITY as i64),
        ("PHY_AUTONEG_EXP", PHY_AUTONEG_EXP as i64),
        ("PHY_NEXT_PAGE_TX", PHY_NEXT_PAGE_TX as i64),
        ("PHY_LP_NEXT_PAGE", PHY_LP_NEXT_PAGE as i64),
        ("PHY_1000T_CTRL", PHY_1000T_CTRL as i64),
        ("PHY_1000T_STATUS", PHY_1000T_STATUS as i64),
        ("PHY_EXT_STATUS", PHY_EXT_STATUS as i64),
        ("MAX_PHY_REG_ADDRESS", MAX_PHY_REG_ADDRESS as i64),
        ("MAX_PHY_MULTI_PAGE_REG", MAX_PHY_MULTI_PAGE_REG as i64),
        ("M88E1000_PHY_SPEC_CTRL", M88E1000_PHY_SPEC_CTRL as i64),
        ("M88E1000_PHY_SPEC_STATUS", M88E1000_PHY_SPEC_STATUS as i64),
        ("M88E1000_INT_ENABLE", M88E1000_INT_ENABLE as i64),
        ("M88E1000_INT_STATUS", M88E1000_INT_STATUS as i64),
        (
            "M88E1000_EXT_PHY_SPEC_CTRL",
            M88E1000_EXT_PHY_SPEC_CTRL as i64
        ),
        ("M88E1000_RX_ERR_CNTR", M88E1000_RX_ERR_CNTR as i64),
        ("M88E1000_PHY_EXT_CTRL", M88E1000_PHY_EXT_CTRL as i64),
        ("M88E1000_PHY_PAGE_SELECT", M88E1000_PHY_PAGE_SELECT as i64),
        ("M88E1000_PHY_GEN_CONTROL", M88E1000_PHY_GEN_CONTROL as i64),
        (
            "M88E1000_PHY_VCO_REG_BIT8",
            M88E1000_PHY_VCO_REG_BIT8 as i64
        ),
        (
            "M88E1000_PHY_VCO_REG_BIT11",
            M88E1000_PHY_VCO_REG_BIT11 as i64
        ),
        ("M88E1543_PAGE_ADDR", M88E1543_PAGE_ADDR as i64),
        ("M88E1543_EEE_CTRL_1", M88E1543_EEE_CTRL_1 as i64),
        ("M88E1543_EEE_CTRL_1_MS", M88E1543_EEE_CTRL_1_MS as i64),
        ("M88E1512_CFG_REG_1", M88E1512_CFG_REG_1 as i64),
        ("M88E1512_CFG_REG_2", M88E1512_CFG_REG_2 as i64),
        ("M88E1512_CFG_REG_3", M88E1512_CFG_REG_3 as i64),
        ("M88E1512_MODE", M88E1512_MODE as i64),
        (
            "BME1000_PSCR_ENABLE_DOWNSHIFT",
            BME1000_PSCR_ENABLE_DOWNSHIFT as i64
        ),
        ("BM_PHY_PAGE_SELECT", BM_PHY_PAGE_SELECT as i64),
        ("BM_REG_BIAS1", BM_REG_BIAS1 as i64),
        ("BM_REG_BIAS2", BM_REG_BIAS2 as i64),
        ("BM_PORT_CTRL_PAGE", BM_PORT_CTRL_PAGE as i64),
        (
            "IGP01E1000_IEEE_REGS_PAGE",
            IGP01E1000_IEEE_REGS_PAGE as i64
        ),
        (
            "IGP01E1000_IEEE_RESTART_AUTONEG",
            IGP01E1000_IEEE_RESTART_AUTONEG as i64
        ),
        (
            "IGP01E1000_IEEE_FORCE_GIGA",
            IGP01E1000_IEEE_FORCE_GIGA as i64
        ),
        (
            "IGP01E1000_PHY_PORT_CONFIG",
            IGP01E1000_PHY_PORT_CONFIG as i64
        ),
        (
            "IGP01E1000_PHY_PORT_STATUS",
            IGP01E1000_PHY_PORT_STATUS as i64
        ),
        ("IGP01E1000_PHY_PORT_CTRL", IGP01E1000_PHY_PORT_CTRL as i64),
        (
            "IGP01E1000_PHY_LINK_HEALTH",
            IGP01E1000_PHY_LINK_HEALTH as i64
        ),
        ("IGP01E1000_GMII_FIFO", IGP01E1000_GMII_FIFO as i64),
        (
            "IGP01E1000_PHY_CHANNEL_QUALITY",
            IGP01E1000_PHY_CHANNEL_QUALITY as i64
        ),
        (
            "IGP02E1000_PHY_POWER_MGMT",
            IGP02E1000_PHY_POWER_MGMT as i64
        ),
        (
            "IGP01E1000_PHY_PAGE_SELECT",
            IGP01E1000_PHY_PAGE_SELECT as i64
        ),
        ("IGP01E1000_PHY_AGC_A", IGP01E1000_PHY_AGC_A as i64),
        ("IGP01E1000_PHY_AGC_B", IGP01E1000_PHY_AGC_B as i64),
        ("IGP01E1000_PHY_AGC_C", IGP01E1000_PHY_AGC_C as i64),
        ("IGP01E1000_PHY_AGC_D", IGP01E1000_PHY_AGC_D as i64),
        ("IGP02E1000_PHY_AGC_A", IGP02E1000_PHY_AGC_A as i64),
        ("IGP02E1000_PHY_AGC_B", IGP02E1000_PHY_AGC_B as i64),
        ("IGP02E1000_PHY_AGC_C", IGP02E1000_PHY_AGC_C as i64),
        ("IGP02E1000_PHY_AGC_D", IGP02E1000_PHY_AGC_D as i64),
        ("IGP01E1000_PHY_DSP_RESET", IGP01E1000_PHY_DSP_RESET as i64),
        ("IGP01E1000_PHY_DSP_SET", IGP01E1000_PHY_DSP_SET as i64),
        ("IGP01E1000_PHY_DSP_FFE", IGP01E1000_PHY_DSP_FFE as i64),
        (
            "IGP01E1000_PHY_CHANNEL_NUM",
            IGP01E1000_PHY_CHANNEL_NUM as i64
        ),
        (
            "IGP02E1000_PHY_CHANNEL_NUM",
            IGP02E1000_PHY_CHANNEL_NUM as i64
        ),
        (
            "IGP01E1000_PHY_AGC_PARAM_A",
            IGP01E1000_PHY_AGC_PARAM_A as i64
        ),
        (
            "IGP01E1000_PHY_AGC_PARAM_B",
            IGP01E1000_PHY_AGC_PARAM_B as i64
        ),
        (
            "IGP01E1000_PHY_AGC_PARAM_C",
            IGP01E1000_PHY_AGC_PARAM_C as i64
        ),
        (
            "IGP01E1000_PHY_AGC_PARAM_D",
            IGP01E1000_PHY_AGC_PARAM_D as i64
        ),
        (
            "IGP01E1000_PHY_EDAC_MU_INDEX",
            IGP01E1000_PHY_EDAC_MU_INDEX as i64
        ),
        (
            "IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS",
            IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS as i64
        ),
        (
            "IGP01E1000_PHY_ANALOG_TX_STATE",
            IGP01E1000_PHY_ANALOG_TX_STATE as i64
        ),
        (
            "IGP01E1000_PHY_ANALOG_CLASS_A",
            IGP01E1000_PHY_ANALOG_CLASS_A as i64
        ),
        (
            "IGP01E1000_PHY_FORCE_ANALOG_ENABLE",
            IGP01E1000_PHY_FORCE_ANALOG_ENABLE as i64
        ),
        (
            "IGP01E1000_PHY_DSP_FFE_CM_CP",
            IGP01E1000_PHY_DSP_FFE_CM_CP as i64
        ),
        (
            "IGP01E1000_PHY_DSP_FFE_DEFAULT",
            IGP01E1000_PHY_DSP_FFE_DEFAULT as i64
        ),
        (
            "IGP01E1000_PHY_PCS_INIT_REG",
            IGP01E1000_PHY_PCS_INIT_REG as i64
        ),
        (
            "IGP01E1000_PHY_PCS_CTRL_REG",
            IGP01E1000_PHY_PCS_CTRL_REG as i64
        ),
        (
            "IGP01E1000_ANALOG_REGS_PAGE",
            IGP01E1000_ANALOG_REGS_PAGE as i64
        ),
        ("I82580_ADDR_REG", I82580_ADDR_REG as i64),
        ("I82580_CFG_REG", I82580_CFG_REG as i64),
        (
            "I82580_CFG_ASSERT_CRS_ON_TX",
            I82580_CFG_ASSERT_CRS_ON_TX as i64
        ),
        (
            "I82580_CFG_ENABLE_DOWNSHIFT",
            I82580_CFG_ENABLE_DOWNSHIFT as i64
        ),
        ("I82580_CTRL_REG", I82580_CTRL_REG as i64),
        (
            "I82580_CTRL_DOWNSHIFT_MASK",
            I82580_CTRL_DOWNSHIFT_MASK as i64
        ),
        ("GG82563_PAGE_SHIFT", GG82563_PAGE_SHIFT as i64),
        ("GG82563_MIN_ALT_REG", GG82563_MIN_ALT_REG as i64),
        ("GG82563_PHY_SPEC_CTRL", GG82563_PHY_SPEC_CTRL as i64),
        ("GG82563_PHY_SPEC_STATUS", GG82563_PHY_SPEC_STATUS as i64),
        ("GG82563_PHY_INT_ENABLE", GG82563_PHY_INT_ENABLE as i64),
        (
            "GG82563_PHY_SPEC_STATUS_2",
            GG82563_PHY_SPEC_STATUS_2 as i64
        ),
        ("GG82563_PHY_RX_ERR_CNTR", GG82563_PHY_RX_ERR_CNTR as i64),
        ("GG82563_PHY_PAGE_SELECT", GG82563_PHY_PAGE_SELECT as i64),
        ("GG82563_PHY_SPEC_CTRL_2", GG82563_PHY_SPEC_CTRL_2 as i64),
        (
            "GG82563_PHY_PAGE_SELECT_ALT",
            GG82563_PHY_PAGE_SELECT_ALT as i64
        ),
        (
            "GG82563_PHY_TEST_CLK_CTRL",
            GG82563_PHY_TEST_CLK_CTRL as i64
        ),
        (
            "GG82563_PHY_MAC_SPEC_CTRL",
            GG82563_PHY_MAC_SPEC_CTRL as i64
        ),
        (
            "GG82563_PHY_MAC_SPEC_CTRL_2",
            GG82563_PHY_MAC_SPEC_CTRL_2 as i64
        ),
        ("GG82563_PHY_DSP_DISTANCE", GG82563_PHY_DSP_DISTANCE as i64),
        (
            "GG82563_PHY_KMRN_MODE_CTRL",
            GG82563_PHY_KMRN_MODE_CTRL as i64
        ),
        ("GG82563_PHY_PORT_RESET", GG82563_PHY_PORT_RESET as i64),
        ("GG82563_PHY_REVISION_ID", GG82563_PHY_REVISION_ID as i64),
        ("GG82563_PHY_DEVICE_ID", GG82563_PHY_DEVICE_ID as i64),
        (
            "GG82563_PHY_PWR_MGMT_CTRL",
            GG82563_PHY_PWR_MGMT_CTRL as i64
        ),
        (
            "GG82563_PHY_RATE_ADAPT_CTRL",
            GG82563_PHY_RATE_ADAPT_CTRL as i64
        ),
        (
            "GG82563_PHY_KMRN_FIFO_CTRL_STAT",
            GG82563_PHY_KMRN_FIFO_CTRL_STAT as i64
        ),
        ("GG82563_PHY_KMRN_CTRL", GG82563_PHY_KMRN_CTRL as i64),
        ("GG82563_PHY_INBAND_CTRL", GG82563_PHY_INBAND_CTRL as i64),
        (
            "GG82563_PHY_KMRN_DIAGNOSTIC",
            GG82563_PHY_KMRN_DIAGNOSTIC as i64
        ),
        ("GG82563_PHY_ACK_TIMEOUTS", GG82563_PHY_ACK_TIMEOUTS as i64),
        ("GG82563_PHY_ADV_ABILITY", GG82563_PHY_ADV_ABILITY as i64),
        (
            "GG82563_PHY_LINK_PARTNER_ADV_ABILITY",
            GG82563_PHY_LINK_PARTNER_ADV_ABILITY as i64
        ),
        (
            "GG82563_PHY_ADV_NEXT_PAGE",
            GG82563_PHY_ADV_NEXT_PAGE as i64
        ),
        (
            "GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE",
            GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE as i64
        ),
        ("GG82563_PHY_KMRN_MISC", GG82563_PHY_KMRN_MISC as i64),
        ("I82577_PHY_ADDR_REG", I82577_PHY_ADDR_REG as i64),
        ("I82577_PHY_CFG_REG", I82577_PHY_CFG_REG as i64),
        ("I82577_PHY_CTRL_REG", I82577_PHY_CTRL_REG as i64),
        (
            "I82577_PHY_CFG_ENABLE_CRS_ON_TX",
            I82577_PHY_CFG_ENABLE_CRS_ON_TX as i64
        ),
        (
            "I82577_PHY_CFG_ENABLE_DOWNSHIFT",
            I82577_PHY_CFG_ENABLE_DOWNSHIFT as i64
        ),
        ("I82578_PHY_ADDR_REG", I82578_PHY_ADDR_REG as i64),
        (
            "I82578_EPSCR_DOWNSHIFT_ENABLE",
            I82578_EPSCR_DOWNSHIFT_ENABLE as i64
        ),
        (
            "I82578_EPSCR_DOWNSHIFT_COUNTER_MASK",
            I82578_EPSCR_DOWNSHIFT_COUNTER_MASK as i64
        ),
        ("MII_CR_SPEED_SELECT_MSB", MII_CR_SPEED_SELECT_MSB as i64),
        ("MII_CR_COLL_TEST_ENABLE", MII_CR_COLL_TEST_ENABLE as i64),
        ("MII_CR_FULL_DUPLEX", MII_CR_FULL_DUPLEX as i64),
        ("MII_CR_RESTART_AUTO_NEG", MII_CR_RESTART_AUTO_NEG as i64),
        ("MII_CR_ISOLATE", MII_CR_ISOLATE as i64),
        ("MII_CR_POWER_DOWN", MII_CR_POWER_DOWN as i64),
        ("MII_CR_AUTO_NEG_EN", MII_CR_AUTO_NEG_EN as i64),
        ("MII_CR_SPEED_SELECT_LSB", MII_CR_SPEED_SELECT_LSB as i64),
        ("MII_CR_LOOPBACK", MII_CR_LOOPBACK as i64),
        ("MII_CR_RESET", MII_CR_RESET as i64),
        ("MII_SR_EXTENDED_CAPS", MII_SR_EXTENDED_CAPS as i64),
        ("MII_SR_JABBER_DETECT", MII_SR_JABBER_DETECT as i64),
        ("MII_SR_LINK_STATUS", MII_SR_LINK_STATUS as i64),
        ("MII_SR_AUTONEG_CAPS", MII_SR_AUTONEG_CAPS as i64),
        ("MII_SR_REMOTE_FAULT", MII_SR_REMOTE_FAULT as i64),
        ("MII_SR_AUTONEG_COMPLETE", MII_SR_AUTONEG_COMPLETE as i64),
        ("MII_SR_PREAMBLE_SUPPRESS", MII_SR_PREAMBLE_SUPPRESS as i64),
        ("MII_SR_EXTENDED_STATUS", MII_SR_EXTENDED_STATUS as i64),
        ("MII_SR_100T2_HD_CAPS", MII_SR_100T2_HD_CAPS as i64),
        ("MII_SR_100T2_FD_CAPS", MII_SR_100T2_FD_CAPS as i64),
        ("MII_SR_10T_HD_CAPS", MII_SR_10T_HD_CAPS as i64),
        ("MII_SR_10T_FD_CAPS", MII_SR_10T_FD_CAPS as i64),
        ("MII_SR_100X_HD_CAPS", MII_SR_100X_HD_CAPS as i64),
        ("MII_SR_100X_FD_CAPS", MII_SR_100X_FD_CAPS as i64),
        ("MII_SR_100T4_CAPS", MII_SR_100T4_CAPS as i64),
        ("NWAY_AR_SELECTOR_FIELD", NWAY_AR_SELECTOR_FIELD as i64),
        ("NWAY_AR_10T_HD_CAPS", NWAY_AR_10T_HD_CAPS as i64),
        ("NWAY_AR_10T_FD_CAPS", NWAY_AR_10T_FD_CAPS as i64),
        ("NWAY_AR_100TX_HD_CAPS", NWAY_AR_100TX_HD_CAPS as i64),
        ("NWAY_AR_100TX_FD_CAPS", NWAY_AR_100TX_FD_CAPS as i64),
        ("NWAY_AR_100T4_CAPS", NWAY_AR_100T4_CAPS as i64),
        ("NWAY_AR_PAUSE", NWAY_AR_PAUSE as i64),
        ("NWAY_AR_ASM_DIR", NWAY_AR_ASM_DIR as i64),
        ("NWAY_AR_REMOTE_FAULT", NWAY_AR_REMOTE_FAULT as i64),
        ("NWAY_AR_NEXT_PAGE", NWAY_AR_NEXT_PAGE as i64),
        ("NWAY_LPAR_SELECTOR_FIELD", NWAY_LPAR_SELECTOR_FIELD as i64),
        ("NWAY_LPAR_10T_HD_CAPS", NWAY_LPAR_10T_HD_CAPS as i64),
        ("NWAY_LPAR_10T_FD_CAPS", NWAY_LPAR_10T_FD_CAPS as i64),
        ("NWAY_LPAR_100TX_HD_CAPS", NWAY_LPAR_100TX_HD_CAPS as i64),
        ("NWAY_LPAR_100TX_FD_CAPS", NWAY_LPAR_100TX_FD_CAPS as i64),
        ("NWAY_LPAR_100T4_CAPS", NWAY_LPAR_100T4_CAPS as i64),
        ("NWAY_LPAR_PAUSE", NWAY_LPAR_PAUSE as i64),
        ("NWAY_LPAR_ASM_DIR", NWAY_LPAR_ASM_DIR as i64),
        ("NWAY_LPAR_REMOTE_FAULT", NWAY_LPAR_REMOTE_FAULT as i64),
        ("NWAY_LPAR_ACKNOWLEDGE", NWAY_LPAR_ACKNOWLEDGE as i64),
        ("NWAY_LPAR_NEXT_PAGE", NWAY_LPAR_NEXT_PAGE as i64),
        ("NWAY_ER_LP_NWAY_CAPS", NWAY_ER_LP_NWAY_CAPS as i64),
        ("NWAY_ER_PAGE_RXD", NWAY_ER_PAGE_RXD as i64),
        ("NWAY_ER_NEXT_PAGE_CAPS", NWAY_ER_NEXT_PAGE_CAPS as i64),
        (
            "NWAY_ER_LP_NEXT_PAGE_CAPS",
            NWAY_ER_LP_NEXT_PAGE_CAPS as i64
        ),
        ("NWAY_ER_PAR_DETECT_FAULT", NWAY_ER_PAR_DETECT_FAULT as i64),
        ("NPTX_MSG_CODE_FIELD", NPTX_MSG_CODE_FIELD as i64),
        ("NPTX_TOGGLE", NPTX_TOGGLE as i64),
        ("NPTX_ACKNOWLDGE2", NPTX_ACKNOWLDGE2 as i64),
        ("NPTX_MSG_PAGE", NPTX_MSG_PAGE as i64),
        ("NPTX_NEXT_PAGE", NPTX_NEXT_PAGE as i64),
        ("LP_RNPR_MSG_CODE_FIELD", LP_RNPR_MSG_CODE_FIELD as i64),
        ("LP_RNPR_TOGGLE", LP_RNPR_TOGGLE as i64),
        ("LP_RNPR_ACKNOWLDGE2", LP_RNPR_ACKNOWLDGE2 as i64),
        ("LP_RNPR_MSG_PAGE", LP_RNPR_MSG_PAGE as i64),
        ("LP_RNPR_ACKNOWLDGE", LP_RNPR_ACKNOWLDGE as i64),
        ("LP_RNPR_NEXT_PAGE", LP_RNPR_NEXT_PAGE as i64),
        ("CR_1000T_ASYM_PAUSE", CR_1000T_ASYM_PAUSE as i64),
        ("CR_1000T_HD_CAPS", CR_1000T_HD_CAPS as i64),
        ("CR_1000T_FD_CAPS", CR_1000T_FD_CAPS as i64),
        ("CR_1000T_REPEATER_DTE", CR_1000T_REPEATER_DTE as i64),
        ("CR_1000T_MS_VALUE", CR_1000T_MS_VALUE as i64),
        ("CR_1000T_MS_ENABLE", CR_1000T_MS_ENABLE as i64),
        (
            "CR_1000T_TEST_MODE_NORMAL",
            CR_1000T_TEST_MODE_NORMAL as i64
        ),
        ("CR_1000T_TEST_MODE_1", CR_1000T_TEST_MODE_1 as i64),
        ("CR_1000T_TEST_MODE_2", CR_1000T_TEST_MODE_2 as i64),
        ("CR_1000T_TEST_MODE_3", CR_1000T_TEST_MODE_3 as i64),
        ("CR_1000T_TEST_MODE_4", CR_1000T_TEST_MODE_4 as i64),
        ("SR_1000T_IDLE_ERROR_CNT", SR_1000T_IDLE_ERROR_CNT as i64),
        ("SR_1000T_ASYM_PAUSE_DIR", SR_1000T_ASYM_PAUSE_DIR as i64),
        ("SR_1000T_LP_HD_CAPS", SR_1000T_LP_HD_CAPS as i64),
        ("SR_1000T_LP_FD_CAPS", SR_1000T_LP_FD_CAPS as i64),
        (
            "SR_1000T_REMOTE_RX_STATUS",
            SR_1000T_REMOTE_RX_STATUS as i64
        ),
        ("SR_1000T_LOCAL_RX_STATUS", SR_1000T_LOCAL_RX_STATUS as i64),
        ("SR_1000T_MS_CONFIG_RES", SR_1000T_MS_CONFIG_RES as i64),
        ("SR_1000T_MS_CONFIG_FAULT", SR_1000T_MS_CONFIG_FAULT as i64),
        (
            "SR_1000T_REMOTE_RX_STATUS_SHIFT",
            SR_1000T_REMOTE_RX_STATUS_SHIFT as i64
        ),
        (
            "SR_1000T_LOCAL_RX_STATUS_SHIFT",
            SR_1000T_LOCAL_RX_STATUS_SHIFT as i64
        ),
        (
            "SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT",
            SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT as i64
        ),
        (
            "FFE_IDLE_ERR_COUNT_TIMEOUT_20",
            FFE_IDLE_ERR_COUNT_TIMEOUT_20 as i64
        ),
        (
            "FFE_IDLE_ERR_COUNT_TIMEOUT_100",
            FFE_IDLE_ERR_COUNT_TIMEOUT_100 as i64
        ),
        ("IEEE_ESR_1000T_HD_CAPS", IEEE_ESR_1000T_HD_CAPS as i64),
        ("IEEE_ESR_1000T_FD_CAPS", IEEE_ESR_1000T_FD_CAPS as i64),
        ("IEEE_ESR_1000X_HD_CAPS", IEEE_ESR_1000X_HD_CAPS as i64),
        ("IEEE_ESR_1000X_FD_CAPS", IEEE_ESR_1000X_FD_CAPS as i64),
        ("PHY_TX_POLARITY_MASK", PHY_TX_POLARITY_MASK as i64),
        ("PHY_TX_NORMAL_POLARITY", PHY_TX_NORMAL_POLARITY as i64),
        ("AUTO_POLARITY_DISABLE", AUTO_POLARITY_DISABLE as i64),
        (
            "M88E1000_PSCR_JABBER_DISABLE",
            M88E1000_PSCR_JABBER_DISABLE as i64
        ),
        (
            "M88E1000_PSCR_POLARITY_REVERSAL",
            M88E1000_PSCR_POLARITY_REVERSAL as i64
        ),
        ("M88E1000_PSCR_SQE_TEST", M88E1000_PSCR_SQE_TEST as i64),
        (
            "M88E1000_PSCR_CLK125_DISABLE",
            M88E1000_PSCR_CLK125_DISABLE as i64
        ),
        (
            "M88E1000_PSCR_MDI_MANUAL_MODE",
            M88E1000_PSCR_MDI_MANUAL_MODE as i64
        ),
        (
            "M88E1000_PSCR_MDIX_MANUAL_MODE",
            M88E1000_PSCR_MDIX_MANUAL_MODE as i64
        ),
        (
            "M88E1000_PSCR_AUTO_X_1000T",
            M88E1000_PSCR_AUTO_X_1000T as i64
        ),
        (
            "M88E1000_PSCR_AUTO_X_MODE",
            M88E1000_PSCR_AUTO_X_MODE as i64
        ),
        (
            "M88E1000_PSCR_10BT_EXT_DIST_ENABLE",
            M88E1000_PSCR_10BT_EXT_DIST_ENABLE as i64
        ),
        (
            "M88E1000_PSCR_MII_5BIT_ENABLE",
            M88E1000_PSCR_MII_5BIT_ENABLE as i64
        ),
        (
            "M88E1000_PSCR_SCRAMBLER_DISABLE",
            M88E1000_PSCR_SCRAMBLER_DISABLE as i64
        ),
        (
            "M88E1000_PSCR_FORCE_LINK_GOOD",
            M88E1000_PSCR_FORCE_LINK_GOOD as i64
        ),
        (
            "M88E1000_PSCR_ASSERT_CRS_ON_TX",
            M88E1000_PSCR_ASSERT_CRS_ON_TX as i64
        ),
        (
            "M88E1000_PSCR_POLARITY_REVERSAL_SHIFT",
            M88E1000_PSCR_POLARITY_REVERSAL_SHIFT as i64
        ),
        (
            "M88E1000_PSCR_AUTO_X_MODE_SHIFT",
            M88E1000_PSCR_AUTO_X_MODE_SHIFT as i64
        ),
        (
            "M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT",
            M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT as i64
        ),
        ("M88E1000_PSSR_JABBER", M88E1000_PSSR_JABBER as i64),
        (
            "M88E1000_PSSR_REV_POLARITY",
            M88E1000_PSSR_REV_POLARITY as i64
        ),
        ("M88E1000_PSSR_DOWNSHIFT", M88E1000_PSSR_DOWNSHIFT as i64),
        ("M88E1000_PSSR_MDIX", M88E1000_PSSR_MDIX as i64),
        (
            "M88E1000_PSSR_CABLE_LENGTH",
            M88E1000_PSSR_CABLE_LENGTH as i64
        ),
        ("M88E1000_PSSR_LINK", M88E1000_PSSR_LINK as i64),
        (
            "M88E1000_PSSR_SPD_DPLX_RESOLVED",
            M88E1000_PSSR_SPD_DPLX_RESOLVED as i64
        ),
        ("M88E1000_PSSR_PAGE_RCVD", M88E1000_PSSR_PAGE_RCVD as i64),
        ("M88E1000_PSSR_DPLX", M88E1000_PSSR_DPLX as i64),
        ("M88E1000_PSSR_SPEED", M88E1000_PSSR_SPEED as i64),
        ("M88E1000_PSSR_10MBS", M88E1000_PSSR_10MBS as i64),
        ("M88E1000_PSSR_100MBS", M88E1000_PSSR_100MBS as i64),
        ("M88E1000_PSSR_1000MBS", M88E1000_PSSR_1000MBS as i64),
        (
            "M88E1000_PSSR_REV_POLARITY_SHIFT",
            M88E1000_PSSR_REV_POLARITY_SHIFT as i64
        ),
        (
            "M88E1000_PSSR_DOWNSHIFT_SHIFT",
            M88E1000_PSSR_DOWNSHIFT_SHIFT as i64
        ),
        ("M88E1000_PSSR_MDIX_SHIFT", M88E1000_PSSR_MDIX_SHIFT as i64),
        (
            "M88E1000_PSSR_CABLE_LENGTH_SHIFT",
            M88E1000_PSSR_CABLE_LENGTH_SHIFT as i64
        ),
        (
            "M88E1000_EPSCR_FIBER_LOOPBACK",
            M88E1000_EPSCR_FIBER_LOOPBACK as i64
        ),
        (
            "M88E1000_EPSCR_DOWN_NO_IDLE",
            M88E1000_EPSCR_DOWN_NO_IDLE as i64
        ),
        (
            "M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK",
            M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK as i64
        ),
        (
            "M88E1000_EPSCR_MASTER_DOWNSHIFT_1X",
            M88E1000_EPSCR_MASTER_DOWNSHIFT_1X as i64
        ),
        (
            "M88E1000_EPSCR_MASTER_DOWNSHIFT_2X",
            M88E1000_EPSCR_MASTER_DOWNSHIFT_2X as i64
        ),
        (
            "M88E1000_EPSCR_MASTER_DOWNSHIFT_3X",
            M88E1000_EPSCR_MASTER_DOWNSHIFT_3X as i64
        ),
        (
            "M88E1000_EPSCR_MASTER_DOWNSHIFT_4X",
            M88E1000_EPSCR_MASTER_DOWNSHIFT_4X as i64
        ),
        (
            "M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK",
            M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK as i64
        ),
        (
            "M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS",
            M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS as i64
        ),
        (
            "M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X",
            M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X as i64
        ),
        (
            "M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X",
            M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X as i64
        ),
        (
            "M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X",
            M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X as i64
        ),
        (
            "M88E1000_EPSCR_TX_CLK_2_5",
            M88E1000_EPSCR_TX_CLK_2_5 as i64
        ),
        ("M88E1000_EPSCR_TX_CLK_25", M88E1000_EPSCR_TX_CLK_25 as i64),
        ("M88E1000_EPSCR_TX_CLK_0", M88E1000_EPSCR_TX_CLK_0 as i64),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X as i64
        ),
        (
            "M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X",
            M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X as i64
        ),
        (
            "M88E1000_EPSCR_TX_TIME_CTRL",
            M88E1000_EPSCR_TX_TIME_CTRL as i64
        ),
        (
            "M88E1000_EPSCR_RX_TIME_CTRL",
            M88E1000_EPSCR_RX_TIME_CTRL as i64
        ),
        (
            "IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT",
            IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT as i64
        ),
        ("IGP01E1000_PSCFR_PRE_EN", IGP01E1000_PSCFR_PRE_EN as i64),
        (
            "IGP01E1000_PSCFR_SMART_SPEED",
            IGP01E1000_PSCFR_SMART_SPEED as i64
        ),
        (
            "IGP01E1000_PSCFR_DISABLE_TPLOOPBACK",
            IGP01E1000_PSCFR_DISABLE_TPLOOPBACK as i64
        ),
        (
            "IGP01E1000_PSCFR_DISABLE_JABBER",
            IGP01E1000_PSCFR_DISABLE_JABBER as i64
        ),
        (
            "IGP01E1000_PSCFR_DISABLE_TRANSMIT",
            IGP01E1000_PSCFR_DISABLE_TRANSMIT as i64
        ),
        (
            "IGP01E1000_PSSR_AUTONEG_FAILED",
            IGP01E1000_PSSR_AUTONEG_FAILED as i64
        ),
        (
            "IGP01E1000_PSSR_POLARITY_REVERSED",
            IGP01E1000_PSSR_POLARITY_REVERSED as i64
        ),
        (
            "IGP01E1000_PSSR_CABLE_LENGTH",
            IGP01E1000_PSSR_CABLE_LENGTH as i64
        ),
        (
            "IGP01E1000_PSSR_FULL_DUPLEX",
            IGP01E1000_PSSR_FULL_DUPLEX as i64
        ),
        ("IGP01E1000_PSSR_LINK_UP", IGP01E1000_PSSR_LINK_UP as i64),
        ("IGP01E1000_PSSR_MDIX", IGP01E1000_PSSR_MDIX as i64),
        (
            "IGP01E1000_PSSR_SPEED_MASK",
            IGP01E1000_PSSR_SPEED_MASK as i64
        ),
        (
            "IGP01E1000_PSSR_SPEED_10MBPS",
            IGP01E1000_PSSR_SPEED_10MBPS as i64
        ),
        (
            "IGP01E1000_PSSR_SPEED_100MBPS",
            IGP01E1000_PSSR_SPEED_100MBPS as i64
        ),
        (
            "IGP01E1000_PSSR_SPEED_1000MBPS",
            IGP01E1000_PSSR_SPEED_1000MBPS as i64
        ),
        (
            "IGP01E1000_PSSR_CABLE_LENGTH_SHIFT",
            IGP01E1000_PSSR_CABLE_LENGTH_SHIFT as i64
        ),
        (
            "IGP01E1000_PSSR_MDIX_SHIFT",
            IGP01E1000_PSSR_MDIX_SHIFT as i64
        ),
        (
            "IGP01E1000_PSCR_TP_LOOPBACK",
            IGP01E1000_PSCR_TP_LOOPBACK as i64
        ),
        (
            "IGP01E1000_PSCR_CORRECT_NC_SCMBLR",
            IGP01E1000_PSCR_CORRECT_NC_SCMBLR as i64
        ),
        (
            "IGP01E1000_PSCR_TEN_CRS_SELECT",
            IGP01E1000_PSCR_TEN_CRS_SELECT as i64
        ),
        (
            "IGP01E1000_PSCR_FLIP_CHIP",
            IGP01E1000_PSCR_FLIP_CHIP as i64
        ),
        (
            "IGP01E1000_PSCR_AUTO_MDIX",
            IGP01E1000_PSCR_AUTO_MDIX as i64
        ),
        (
            "IGP01E1000_PSCR_FORCE_MDI_MDIX",
            IGP01E1000_PSCR_FORCE_MDI_MDIX as i64
        ),
        (
            "IGP01E1000_PLHR_SS_DOWNGRADE",
            IGP01E1000_PLHR_SS_DOWNGRADE as i64
        ),
        (
            "IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR",
            IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR as i64
        ),
        (
            "IGP01E1000_PLHR_MASTER_FAULT",
            IGP01E1000_PLHR_MASTER_FAULT as i64
        ),
        (
            "IGP01E1000_PLHR_MASTER_RESOLUTION",
            IGP01E1000_PLHR_MASTER_RESOLUTION as i64
        ),
        (
            "IGP01E1000_PLHR_GIG_REM_RCVR_NOK",
            IGP01E1000_PLHR_GIG_REM_RCVR_NOK as i64
        ),
        (
            "IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW",
            IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW as i64
        ),
        (
            "IGP01E1000_PLHR_DATA_ERR_1",
            IGP01E1000_PLHR_DATA_ERR_1 as i64
        ),
        (
            "IGP01E1000_PLHR_DATA_ERR_0",
            IGP01E1000_PLHR_DATA_ERR_0 as i64
        ),
        (
            "IGP01E1000_PLHR_AUTONEG_FAULT",
            IGP01E1000_PLHR_AUTONEG_FAULT as i64
        ),
        (
            "IGP01E1000_PLHR_AUTONEG_ACTIVE",
            IGP01E1000_PLHR_AUTONEG_ACTIVE as i64
        ),
        (
            "IGP01E1000_PLHR_VALID_CHANNEL_D",
            IGP01E1000_PLHR_VALID_CHANNEL_D as i64
        ),
        (
            "IGP01E1000_PLHR_VALID_CHANNEL_C",
            IGP01E1000_PLHR_VALID_CHANNEL_C as i64
        ),
        (
            "IGP01E1000_PLHR_VALID_CHANNEL_B",
            IGP01E1000_PLHR_VALID_CHANNEL_B as i64
        ),
        (
            "IGP01E1000_PLHR_VALID_CHANNEL_A",
            IGP01E1000_PLHR_VALID_CHANNEL_A as i64
        ),
        ("IGP01E1000_MSE_CHANNEL_D", IGP01E1000_MSE_CHANNEL_D as i64),
        ("IGP01E1000_MSE_CHANNEL_C", IGP01E1000_MSE_CHANNEL_C as i64),
        ("IGP01E1000_MSE_CHANNEL_B", IGP01E1000_MSE_CHANNEL_B as i64),
        ("IGP01E1000_MSE_CHANNEL_A", IGP01E1000_MSE_CHANNEL_A as i64),
        ("IGP02E1000_PM_SPD", IGP02E1000_PM_SPD as i64),
        ("IGP02E1000_PM_D3_LPLU", IGP02E1000_PM_D3_LPLU as i64),
        ("IGP02E1000_PM_D0_LPLU", IGP02E1000_PM_D0_LPLU as i64),
        ("DSP_RESET_ENABLE", DSP_RESET_ENABLE as i64),
        ("DSP_RESET_DISABLE", DSP_RESET_DISABLE as i64),
        ("E1000_MAX_DSP_RESETS", E1000_MAX_DSP_RESETS as i64),
        (
            "IGP01E1000_AGC_LENGTH_SHIFT",
            IGP01E1000_AGC_LENGTH_SHIFT as i64
        ),
        (
            "IGP02E1000_AGC_LENGTH_SHIFT",
            IGP02E1000_AGC_LENGTH_SHIFT as i64
        ),
        (
            "IGP02E1000_AGC_LENGTH_MASK",
            IGP02E1000_AGC_LENGTH_MASK as i64
        ),
        (
            "IGP01E1000_AGC_LENGTH_TABLE_SIZE",
            IGP01E1000_AGC_LENGTH_TABLE_SIZE as i64
        ),
        (
            "IGP02E1000_AGC_LENGTH_TABLE_SIZE",
            IGP02E1000_AGC_LENGTH_TABLE_SIZE as i64
        ),
        ("IGP01E1000_AGC_RANGE", IGP01E1000_AGC_RANGE as i64),
        ("IGP02E1000_AGC_RANGE", IGP02E1000_AGC_RANGE as i64),
        (
            "IGP01E1000_PHY_POLARITY_MASK",
            IGP01E1000_PHY_POLARITY_MASK as i64
        ),
        ("IGP01E1000_GMII_FLEX_SPD", IGP01E1000_GMII_FLEX_SPD as i64),
        ("IGP01E1000_GMII_SPD", IGP01E1000_GMII_SPD as i64),
        (
            "IGP01E1000_ANALOG_SPARE_FUSE_STATUS",
            IGP01E1000_ANALOG_SPARE_FUSE_STATUS as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_STATUS",
            IGP01E1000_ANALOG_FUSE_STATUS as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_CONTROL",
            IGP01E1000_ANALOG_FUSE_CONTROL as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_BYPASS",
            IGP01E1000_ANALOG_FUSE_BYPASS as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_POLY_MASK",
            IGP01E1000_ANALOG_FUSE_POLY_MASK as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_FINE_MASK",
            IGP01E1000_ANALOG_FUSE_FINE_MASK as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_COARSE_MASK",
            IGP01E1000_ANALOG_FUSE_COARSE_MASK as i64
        ),
        (
            "IGP01E1000_ANALOG_SPARE_FUSE_ENABLED",
            IGP01E1000_ANALOG_SPARE_FUSE_ENABLED as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL",
            IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_COARSE_THRESH",
            IGP01E1000_ANALOG_FUSE_COARSE_THRESH as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_COARSE_10",
            IGP01E1000_ANALOG_FUSE_COARSE_10 as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_FINE_1",
            IGP01E1000_ANALOG_FUSE_FINE_1 as i64
        ),
        (
            "IGP01E1000_ANALOG_FUSE_FINE_10",
            IGP01E1000_ANALOG_FUSE_FINE_10 as i64
        ),
        (
            "GG82563_PSCR_DISABLE_JABBER",
            GG82563_PSCR_DISABLE_JABBER as i64
        ),
        (
            "GG82563_PSCR_POLARITY_REVERSAL_DISABLE",
            GG82563_PSCR_POLARITY_REVERSAL_DISABLE as i64
        ),
        ("GG82563_PSCR_POWER_DOWN", GG82563_PSCR_POWER_DOWN as i64),
        (
            "GG82563_PSCR_COPPER_TRANSMITER_DISABLE",
            GG82563_PSCR_COPPER_TRANSMITER_DISABLE as i64
        ),
        (
            "GG82563_PSCR_CROSSOVER_MODE_MASK",
            GG82563_PSCR_CROSSOVER_MODE_MASK as i64
        ),
        (
            "GG82563_PSCR_CROSSOVER_MODE_MDI",
            GG82563_PSCR_CROSSOVER_MODE_MDI as i64
        ),
        (
            "GG82563_PSCR_CROSSOVER_MODE_MDIX",
            GG82563_PSCR_CROSSOVER_MODE_MDIX as i64
        ),
        (
            "GG82563_PSCR_CROSSOVER_MODE_AUTO",
            GG82563_PSCR_CROSSOVER_MODE_AUTO as i64
        ),
        (
            "GG82563_PSCR_ENALBE_EXTENDED_DISTANCE",
            GG82563_PSCR_ENALBE_EXTENDED_DISTANCE as i64
        ),
        (
            "GG82563_PSCR_ENERGY_DETECT_MASK",
            GG82563_PSCR_ENERGY_DETECT_MASK as i64
        ),
        (
            "GG82563_PSCR_ENERGY_DETECT_OFF",
            GG82563_PSCR_ENERGY_DETECT_OFF as i64
        ),
        (
            "GG82563_PSCR_ENERGY_DETECT_RX",
            GG82563_PSCR_ENERGY_DETECT_RX as i64
        ),
        (
            "GG82563_PSCR_ENERGY_DETECT_RX_TM",
            GG82563_PSCR_ENERGY_DETECT_RX_TM as i64
        ),
        (
            "GG82563_PSCR_FORCE_LINK_GOOD",
            GG82563_PSCR_FORCE_LINK_GOOD as i64
        ),
        (
            "GG82563_PSCR_DOWNSHIFT_ENABLE",
            GG82563_PSCR_DOWNSHIFT_ENABLE as i64
        ),
        (
            "GG82563_PSCR_DOWNSHIFT_COUNTER_MASK",
            GG82563_PSCR_DOWNSHIFT_COUNTER_MASK as i64
        ),
        (
            "GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT",
            GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT as i64
        ),
        ("GG82563_PSSR_JABBER", GG82563_PSSR_JABBER as i64),
        ("GG82563_PSSR_POLARITY", GG82563_PSSR_POLARITY as i64),
        ("GG82563_PSSR_LINK", GG82563_PSSR_LINK as i64),
        (
            "GG82563_PSSR_ENERGY_DETECT",
            GG82563_PSSR_ENERGY_DETECT as i64
        ),
        ("GG82563_PSSR_DOWNSHIFT", GG82563_PSSR_DOWNSHIFT as i64),
        (
            "GG82563_PSSR_CROSSOVER_STATUS",
            GG82563_PSSR_CROSSOVER_STATUS as i64
        ),
        (
            "GG82563_PSSR_RX_PAUSE_ENABLED",
            GG82563_PSSR_RX_PAUSE_ENABLED as i64
        ),
        (
            "GG82563_PSSR_TX_PAUSE_ENABLED",
            GG82563_PSSR_TX_PAUSE_ENABLED as i64
        ),
        ("GG82563_PSSR_LINK_UP", GG82563_PSSR_LINK_UP as i64),
        (
            "GG82563_PSSR_SPEED_DUPLEX_RESOLVED",
            GG82563_PSSR_SPEED_DUPLEX_RESOLVED as i64
        ),
        (
            "GG82563_PSSR_PAGE_RECEIVED",
            GG82563_PSSR_PAGE_RECEIVED as i64
        ),
        ("GG82563_PSSR_DUPLEX", GG82563_PSSR_DUPLEX as i64),
        ("GG82563_PSSR_SPEED_MASK", GG82563_PSSR_SPEED_MASK as i64),
        (
            "GG82563_PSSR_SPEED_10MBPS",
            GG82563_PSSR_SPEED_10MBPS as i64
        ),
        (
            "GG82563_PSSR_SPEED_100MBPS",
            GG82563_PSSR_SPEED_100MBPS as i64
        ),
        (
            "GG82563_PSSR_SPEED_1000MBPS",
            GG82563_PSSR_SPEED_1000MBPS as i64
        ),
        ("GG82563_PSSR2_JABBER", GG82563_PSSR2_JABBER as i64),
        (
            "GG82563_PSSR2_POLARITY_CHANGED",
            GG82563_PSSR2_POLARITY_CHANGED as i64
        ),
        (
            "GG82563_PSSR2_ENERGY_DETECT_CHANGED",
            GG82563_PSSR2_ENERGY_DETECT_CHANGED as i64
        ),
        (
            "GG82563_PSSR2_DOWNSHIFT_INTERRUPT",
            GG82563_PSSR2_DOWNSHIFT_INTERRUPT as i64
        ),
        (
            "GG82563_PSSR2_MDI_CROSSOVER_CHANGE",
            GG82563_PSSR2_MDI_CROSSOVER_CHANGE as i64
        ),
        (
            "GG82563_PSSR2_FALSE_CARRIER",
            GG82563_PSSR2_FALSE_CARRIER as i64
        ),
        (
            "GG82563_PSSR2_SYMBOL_ERROR",
            GG82563_PSSR2_SYMBOL_ERROR as i64
        ),
        (
            "GG82563_PSSR2_LINK_STATUS_CHANGED",
            GG82563_PSSR2_LINK_STATUS_CHANGED as i64
        ),
        (
            "GG82563_PSSR2_AUTO_NEG_COMPLETED",
            GG82563_PSSR2_AUTO_NEG_COMPLETED as i64
        ),
        (
            "GG82563_PSSR2_PAGE_RECEIVED",
            GG82563_PSSR2_PAGE_RECEIVED as i64
        ),
        (
            "GG82563_PSSR2_DUPLEX_CHANGED",
            GG82563_PSSR2_DUPLEX_CHANGED as i64
        ),
        (
            "GG82563_PSSR2_SPEED_CHANGED",
            GG82563_PSSR2_SPEED_CHANGED as i64
        ),
        (
            "GG82563_PSSR2_AUTO_NEG_ERROR",
            GG82563_PSSR2_AUTO_NEG_ERROR as i64
        ),
        (
            "GG82563_PSCR2_10BT_POLARITY_FORCE",
            GG82563_PSCR2_10BT_POLARITY_FORCE as i64
        ),
        (
            "GG82563_PSCR2_1000MB_TEST_SELECT_MASK",
            GG82563_PSCR2_1000MB_TEST_SELECT_MASK as i64
        ),
        (
            "GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL",
            GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL as i64
        ),
        (
            "GG82563_PSCR2_1000MB_TEST_SELECT_112NS",
            GG82563_PSCR2_1000MB_TEST_SELECT_112NS as i64
        ),
        (
            "GG82563_PSCR2_1000MB_TEST_SELECT_16NS",
            GG82563_PSCR2_1000MB_TEST_SELECT_16NS as i64
        ),
        (
            "GG82563_PSCR2_REVERSE_AUTO_NEG",
            GG82563_PSCR2_REVERSE_AUTO_NEG as i64
        ),
        (
            "GG82563_PSCR2_1000BT_DISABLE",
            GG82563_PSCR2_1000BT_DISABLE as i64
        ),
        (
            "GG82563_PSCR2_TRANSMITER_TYPE_MASK",
            GG82563_PSCR2_TRANSMITER_TYPE_MASK as i64
        ),
        (
            "GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B",
            GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B as i64
        ),
        (
            "GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A",
            GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A as i64
        ),
        ("GG82563_MSCR_TX_CLK_MASK", GG82563_MSCR_TX_CLK_MASK as i64),
        (
            "GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ",
            GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ as i64
        ),
        (
            "GG82563_MSCR_TX_CLK_100MBPS_25MHZ",
            GG82563_MSCR_TX_CLK_100MBPS_25MHZ as i64
        ),
        (
            "GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ",
            GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ as i64
        ),
        (
            "GG82563_MSCR_TX_CLK_1000MBPS_25MHZ",
            GG82563_MSCR_TX_CLK_1000MBPS_25MHZ as i64
        ),
        (
            "GG82563_MSCR_ASSERT_CRS_ON_TX",
            GG82563_MSCR_ASSERT_CRS_ON_TX as i64
        ),
        (
            "GG82563_DSPD_CABLE_LENGTH",
            GG82563_DSPD_CABLE_LENGTH as i64
        ),
        ("GG82563_KMCR_PHY_LEDS_EN", GG82563_KMCR_PHY_LEDS_EN as i64),
        (
            "GG82563_KMCR_FORCE_LINK_UP",
            GG82563_KMCR_FORCE_LINK_UP as i64
        ),
        (
            "GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT",
            GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT as i64
        ),
        (
            "GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK",
            GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK as i64
        ),
        (
            "GG82563_KMCR_MDIO_BUS_SPEED_SELECT",
            GG82563_KMCR_MDIO_BUS_SPEED_SELECT as i64
        ),
        (
            "GG82563_KMCR_PASS_FALSE_CARRIER",
            GG82563_KMCR_PASS_FALSE_CARRIER as i64
        ),
        (
            "GG82563_PMCR_ENABLE_ELECTRICAL_IDLE",
            GG82563_PMCR_ENABLE_ELECTRICAL_IDLE as i64
        ),
        (
            "GG82563_PMCR_DISABLE_PORT",
            GG82563_PMCR_DISABLE_PORT as i64
        ),
        (
            "GG82563_PMCR_DISABLE_SERDES",
            GG82563_PMCR_DISABLE_SERDES as i64
        ),
        (
            "GG82563_PMCR_REVERSE_AUTO_NEG",
            GG82563_PMCR_REVERSE_AUTO_NEG as i64
        ),
        (
            "GG82563_PMCR_DISABLE_1000_NON_D0",
            GG82563_PMCR_DISABLE_1000_NON_D0 as i64
        ),
        (
            "GG82563_PMCR_DISABLE_1000",
            GG82563_PMCR_DISABLE_1000 as i64
        ),
        (
            "GG82563_PMCR_REVERSE_AUTO_NEG_D0A",
            GG82563_PMCR_REVERSE_AUTO_NEG_D0A as i64
        ),
        (
            "GG82563_PMCR_FORCE_POWER_STATE",
            GG82563_PMCR_FORCE_POWER_STATE as i64
        ),
        (
            "GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK",
            GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK as i64
        ),
        (
            "GG82563_PMCR_PROGRAMMED_POWER_STATE_DR",
            GG82563_PMCR_PROGRAMMED_POWER_STATE_DR as i64
        ),
        (
            "GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U",
            GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U as i64
        ),
        (
            "GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A",
            GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A as i64
        ),
        (
            "GG82563_PMCR_PROGRAMMED_POWER_STATE_D3",
            GG82563_PMCR_PROGRAMMED_POWER_STATE_D3 as i64
        ),
        ("GG82563_ICR_DIS_PADDING", GG82563_ICR_DIS_PADDING as i64),
        ("M88_VENDOR", M88_VENDOR as i64),
        ("M88E1000_E_PHY_ID", M88E1000_E_PHY_ID as i64),
        ("M88E1000_I_PHY_ID", M88E1000_I_PHY_ID as i64),
        ("M88E1011_I_PHY_ID", M88E1011_I_PHY_ID as i64),
        ("IGP01E1000_I_PHY_ID", IGP01E1000_I_PHY_ID as i64),
        ("M88E1000_12_PHY_ID", M88E1000_12_PHY_ID as i64),
        ("M88E1000_14_PHY_ID", M88E1000_14_PHY_ID as i64),
        ("M88E1011_I_REV_4", M88E1011_I_REV_4 as i64),
        ("M88E1111_I_PHY_ID", M88E1111_I_PHY_ID as i64),
        ("M88E1112_E_PHY_ID", M88E1112_E_PHY_ID as i64),
        ("I347AT4_E_PHY_ID", I347AT4_E_PHY_ID as i64),
        ("L1LXT971A_PHY_ID", L1LXT971A_PHY_ID as i64),
        ("GG82563_E_PHY_ID", GG82563_E_PHY_ID as i64),
        ("BME1000_E_PHY_ID", BME1000_E_PHY_ID as i64),
        ("BME1000_E_PHY_ID_R2", BME1000_E_PHY_ID_R2 as i64),
        ("M88E1543_E_PHY_ID", M88E1543_E_PHY_ID as i64),
        ("I82577_E_PHY_ID", I82577_E_PHY_ID as i64),
        ("I82578_E_PHY_ID", I82578_E_PHY_ID as i64),
        ("I82579_E_PHY_ID", I82579_E_PHY_ID as i64),
        ("I217_E_PHY_ID", I217_E_PHY_ID as i64),
        ("I82580_I_PHY_ID", I82580_I_PHY_ID as i64),
        ("I350_I_PHY_ID", I350_I_PHY_ID as i64),
        ("I210_I_PHY_ID", I210_I_PHY_ID as i64),
        ("IGP04E1000_E_PHY_ID", IGP04E1000_E_PHY_ID as i64),
        ("M88E1141_E_PHY_ID", M88E1141_E_PHY_ID as i64),
        ("M88E1512_E_PHY_ID", M88E1512_E_PHY_ID as i64),
        ("PHY_PAGE_SHIFT", PHY_PAGE_SHIFT as i64),
        ("IGP3_PHY_PORT_CTRL", IGP3_PHY_PORT_CTRL as i64),
        ("IGP3_PHY_RATE_ADAPT_CTRL", IGP3_PHY_RATE_ADAPT_CTRL as i64),
        (
            "IGP3_KMRN_FIFO_CTRL_STATS",
            IGP3_KMRN_FIFO_CTRL_STATS as i64
        ),
        ("IGP3_KMRN_POWER_MNG_CTRL", IGP3_KMRN_POWER_MNG_CTRL as i64),
        ("IGP3_KMRN_INBAND_CTRL", IGP3_KMRN_INBAND_CTRL as i64),
        ("IGP3_KMRN_DIAG", IGP3_KMRN_DIAG as i64),
        (
            "IGP3_KMRN_DIAG_PCS_LOCK_LOSS",
            IGP3_KMRN_DIAG_PCS_LOCK_LOSS as i64
        ),
        ("IGP3_KMRN_ACK_TIMEOUT", IGP3_KMRN_ACK_TIMEOUT as i64),
        ("IGP3_VR_CTRL", IGP3_VR_CTRL as i64),
        ("IGP3_VR_CTRL_MODE_SHUT", IGP3_VR_CTRL_MODE_SHUT as i64),
        ("IGP3_VR_CTRL_MODE_MASK", IGP3_VR_CTRL_MODE_MASK as i64),
        ("IGP3_CAPABILITY", IGP3_CAPABILITY as i64),
        ("IGP3_CAP_INITIATE_TEAM", IGP3_CAP_INITIATE_TEAM as i64),
        ("IGP3_CAP_WFM", IGP3_CAP_WFM as i64),
        ("IGP3_CAP_ASF", IGP3_CAP_ASF as i64),
        ("IGP3_CAP_LPLU", IGP3_CAP_LPLU as i64),
        ("IGP3_CAP_DC_AUTO_SPEED", IGP3_CAP_DC_AUTO_SPEED as i64),
        ("IGP3_CAP_SPD", IGP3_CAP_SPD as i64),
        ("IGP3_CAP_MULT_QUEUE", IGP3_CAP_MULT_QUEUE as i64),
        ("IGP3_CAP_RSS", IGP3_CAP_RSS as i64),
        ("IGP3_CAP_8021PQ", IGP3_CAP_8021PQ as i64),
        ("IGP3_CAP_AMT_CB", IGP3_CAP_AMT_CB as i64),
        ("IGP3_PPC_JORDAN_EN", IGP3_PPC_JORDAN_EN as i64),
        (
            "IGP3_PPC_JORDAN_GIGA_SPEED",
            IGP3_PPC_JORDAN_GIGA_SPEED as i64
        ),
        (
            "IGP3_KMRN_PMC_EE_IDLE_LINK_DIS",
            IGP3_KMRN_PMC_EE_IDLE_LINK_DIS as i64
        ),
        (
            "IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK",
            IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK as i64
        ),
        (
            "IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA",
            IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA as i64
        ),
        (
            "IGP3_KMRN_PMC_K0S_MODE1_EN_100",
            IGP3_KMRN_PMC_K0S_MODE1_EN_100 as i64
        ),
        ("IGP3E1000_PHY_MISC_CTRL", IGP3E1000_PHY_MISC_CTRL as i64),
        (
            "IGP3_PHY_MISC_DUPLEX_MANUAL_SET",
            IGP3_PHY_MISC_DUPLEX_MANUAL_SET as i64
        ),
        ("IGP3_KMRN_EXT_CTRL", IGP3_KMRN_EXT_CTRL as i64),
        ("IGP3_KMRN_EC_DIS_INBAND", IGP3_KMRN_EC_DIS_INBAND as i64),
        ("IGP03E1000_E_PHY_ID", IGP03E1000_E_PHY_ID as i64),
        ("IFE_E_PHY_ID", IFE_E_PHY_ID as i64),
        ("IFE_PLUS_E_PHY_ID", IFE_PLUS_E_PHY_ID as i64),
        ("IFE_C_E_PHY_ID", IFE_C_E_PHY_ID as i64),
        (
            "IFE_PHY_EXTENDED_STATUS_CONTROL",
            IFE_PHY_EXTENDED_STATUS_CONTROL as i64
        ),
        ("IFE_PHY_SPECIAL_CONTROL", IFE_PHY_SPECIAL_CONTROL as i64),
        (
            "IFE_PHY_RCV_FALSE_CARRIER",
            IFE_PHY_RCV_FALSE_CARRIER as i64
        ),
        ("IFE_PHY_RCV_DISCONNECT", IFE_PHY_RCV_DISCONNECT as i64),
        ("IFE_PHY_RCV_ERROT_FRAME", IFE_PHY_RCV_ERROT_FRAME as i64),
        ("IFE_PHY_RCV_SYMBOL_ERR", IFE_PHY_RCV_SYMBOL_ERR as i64),
        ("IFE_PHY_PREM_EOF_ERR", IFE_PHY_PREM_EOF_ERR as i64),
        ("IFE_PHY_RCV_EOF_ERR", IFE_PHY_RCV_EOF_ERR as i64),
        ("IFE_PHY_TX_JABBER_DETECT", IFE_PHY_TX_JABBER_DETECT as i64),
        ("IFE_PHY_EQUALIZER", IFE_PHY_EQUALIZER as i64),
        (
            "IFE_PHY_SPECIAL_CONTROL_LED",
            IFE_PHY_SPECIAL_CONTROL_LED as i64
        ),
        ("IFE_PHY_MDIX_CONTROL", IFE_PHY_MDIX_CONTROL as i64),
        ("IFE_PHY_HWI_CONTROL", IFE_PHY_HWI_CONTROL as i64),
        (
            "IFE_PESC_REDUCED_POWER_DOWN_DISABLE",
            IFE_PESC_REDUCED_POWER_DOWN_DISABLE as i64
        ),
        (
            "IFE_PESC_100BTX_POWER_DOWN",
            IFE_PESC_100BTX_POWER_DOWN as i64
        ),
        (
            "IFE_PESC_10BTX_POWER_DOWN",
            IFE_PESC_10BTX_POWER_DOWN as i64
        ),
        (
            "IFE_PESC_POLARITY_REVERSED",
            IFE_PESC_POLARITY_REVERSED as i64
        ),
        ("IFE_PESC_PHY_ADDR_MASK", IFE_PESC_PHY_ADDR_MASK as i64),
        ("IFE_PESC_SPEED", IFE_PESC_SPEED as i64),
        ("IFE_PESC_DUPLEX", IFE_PESC_DUPLEX as i64),
        (
            "IFE_PESC_POLARITY_REVERSED_SHIFT",
            IFE_PESC_POLARITY_REVERSED_SHIFT as i64
        ),
        (
            "IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN",
            IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN as i64
        ),
        ("IFE_PSC_FORCE_POLARITY", IFE_PSC_FORCE_POLARITY as i64),
        (
            "IFE_PSC_AUTO_POLARITY_DISABLE",
            IFE_PSC_AUTO_POLARITY_DISABLE as i64
        ),
        (
            "IFE_PSC_JABBER_FUNC_DISABLE",
            IFE_PSC_JABBER_FUNC_DISABLE as i64
        ),
        (
            "IFE_PSC_FORCE_POLARITY_SHIFT",
            IFE_PSC_FORCE_POLARITY_SHIFT as i64
        ),
        (
            "IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT",
            IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT as i64
        ),
        ("IFE_PMC_AUTO_MDIX", IFE_PMC_AUTO_MDIX as i64),
        ("IFE_PMC_FORCE_MDIX", IFE_PMC_FORCE_MDIX as i64),
        ("IFE_PMC_MDIX_STATUS", IFE_PMC_MDIX_STATUS as i64),
        (
            "IFE_PMC_AUTO_MDIX_COMPLETE",
            IFE_PMC_AUTO_MDIX_COMPLETE as i64
        ),
        ("IFE_PMC_MDIX_MODE_SHIFT", IFE_PMC_MDIX_MODE_SHIFT as i64),
        (
            "IFE_PHC_MDIX_RESET_ALL_MASK",
            IFE_PHC_MDIX_RESET_ALL_MASK as i64
        ),
        ("IFE_PHC_HWI_ENABLE", IFE_PHC_HWI_ENABLE as i64),
        ("IFE_PHC_ABILITY_CHECK", IFE_PHC_ABILITY_CHECK as i64),
        ("IFE_PHC_TEST_EXEC", IFE_PHC_TEST_EXEC as i64),
        ("IFE_PHC_HIGHZ", IFE_PHC_HIGHZ as i64),
        ("IFE_PHC_LOWZ", IFE_PHC_LOWZ as i64),
        ("IFE_PHC_LOW_HIGH_Z_MASK", IFE_PHC_LOW_HIGH_Z_MASK as i64),
        ("IFE_PHC_DISTANCE_MASK", IFE_PHC_DISTANCE_MASK as i64),
        ("IFE_PHC_RESET_ALL_MASK", IFE_PHC_RESET_ALL_MASK as i64),
        ("IFE_PSCL_PROBE_MODE", IFE_PSCL_PROBE_MODE as i64),
        ("IFE_PSCL_PROBE_LEDS_OFF", IFE_PSCL_PROBE_LEDS_OFF as i64),
        ("IFE_PSCL_PROBE_LEDS_ON", IFE_PSCL_PROBE_LEDS_ON as i64),
        (
            "ICH_FLASH_COMMAND_TIMEOUT",
            ICH_FLASH_COMMAND_TIMEOUT as i64
        ),
        ("ICH_FLASH_ERASE_TIMEOUT", ICH_FLASH_ERASE_TIMEOUT as i64),
        (
            "ICH_FLASH_CYCLE_REPEAT_COUNT",
            ICH_FLASH_CYCLE_REPEAT_COUNT as i64
        ),
        ("ICH_FLASH_SEG_SIZE_256", ICH_FLASH_SEG_SIZE_256 as i64),
        ("ICH_FLASH_SEG_SIZE_4K", ICH_FLASH_SEG_SIZE_4K as i64),
        ("ICH_FLASH_SEG_SIZE_8K", ICH_FLASH_SEG_SIZE_8K as i64),
        ("ICH_FLASH_SEG_SIZE_64K", ICH_FLASH_SEG_SIZE_64K as i64),
        ("ICH_CYCLE_READ", ICH_CYCLE_READ as i64),
        ("ICH_CYCLE_RESERVED", ICH_CYCLE_RESERVED as i64),
        ("ICH_CYCLE_WRITE", ICH_CYCLE_WRITE as i64),
        ("ICH_CYCLE_ERASE", ICH_CYCLE_ERASE as i64),
        ("ICH_FLASH_GFPREG", ICH_FLASH_GFPREG as i64),
        ("ICH_FLASH_HSFSTS", ICH_FLASH_HSFSTS as i64),
        ("ICH_FLASH_HSFCTL", ICH_FLASH_HSFCTL as i64),
        ("ICH_FLASH_FADDR", ICH_FLASH_FADDR as i64),
        ("ICH_FLASH_FDATA0", ICH_FLASH_FDATA0 as i64),
        ("ICH_FLASH_FRACC", ICH_FLASH_FRACC as i64),
        ("ICH_FLASH_FREG0", ICH_FLASH_FREG0 as i64),
        ("ICH_FLASH_FREG1", ICH_FLASH_FREG1 as i64),
        ("ICH_FLASH_FREG2", ICH_FLASH_FREG2 as i64),
        ("ICH_FLASH_FREG3", ICH_FLASH_FREG3 as i64),
        ("ICH_FLASH_FPR0", ICH_FLASH_FPR0 as i64),
        ("ICH_FLASH_FPR1", ICH_FLASH_FPR1 as i64),
        ("ICH_FLASH_SSFSTS", ICH_FLASH_SSFSTS as i64),
        ("ICH_FLASH_SSFCTL", ICH_FLASH_SSFCTL as i64),
        ("ICH_FLASH_PREOP", ICH_FLASH_PREOP as i64),
        ("ICH_FLASH_OPTYPE", ICH_FLASH_OPTYPE as i64),
        ("ICH_FLASH_OPMENU", ICH_FLASH_OPMENU as i64),
        ("ICH_FLASH_REG_MAPSIZE", ICH_FLASH_REG_MAPSIZE as i64),
        ("ICH_FLASH_SECTOR_SIZE", ICH_FLASH_SECTOR_SIZE as i64),
        ("ICH_GFPREG_BASE_MASK", ICH_GFPREG_BASE_MASK as i64),
        (
            "ICH_FLASH_LINEAR_ADDR_MASK",
            ICH_FLASH_LINEAR_ADDR_MASK as i64
        ),
        (
            "ICH_FLASH_SECT_ADDR_SHIFT",
            ICH_FLASH_SECT_ADDR_SHIFT as i64
        ),
        ("PHY_PREAMBLE", PHY_PREAMBLE as i64),
        ("PHY_SOF", PHY_SOF as i64),
        ("PHY_OP_READ", PHY_OP_READ as i64),
        ("PHY_OP_WRITE", PHY_OP_WRITE as i64),
        ("PHY_TURNAROUND", PHY_TURNAROUND as i64),
        ("PHY_PREAMBLE_SIZE", PHY_PREAMBLE_SIZE as i64),
        ("MII_CR_SPEED_1000", MII_CR_SPEED_1000 as i64),
        ("MII_CR_SPEED_100", MII_CR_SPEED_100 as i64),
        ("MII_CR_SPEED_10", MII_CR_SPEED_10 as i64),
        ("E1000_PHY_ADDRESS", E1000_PHY_ADDRESS as i64),
        ("PHY_AUTO_NEG_TIME", PHY_AUTO_NEG_TIME as i64),
        ("PHY_FORCE_TIME", PHY_FORCE_TIME as i64),
        ("PHY_REVISION_MASK", PHY_REVISION_MASK as i64),
        ("DEVICE_SPEED_MASK", DEVICE_SPEED_MASK as i64),
        ("REG4_SPEED_MASK", REG4_SPEED_MASK as i64),
        ("REG9_SPEED_MASK", REG9_SPEED_MASK as i64),
        ("ADVERTISE_10_HALF", ADVERTISE_10_HALF as i64),
        ("ADVERTISE_10_FULL", ADVERTISE_10_FULL as i64),
        ("ADVERTISE_100_HALF", ADVERTISE_100_HALF as i64),
        ("ADVERTISE_100_FULL", ADVERTISE_100_FULL as i64),
        ("ADVERTISE_1000_HALF", ADVERTISE_1000_HALF as i64),
        ("ADVERTISE_1000_FULL", ADVERTISE_1000_FULL as i64),
        (
            "AUTONEG_ADVERTISE_SPEED_DEFAULT",
            AUTONEG_ADVERTISE_SPEED_DEFAULT as i64
        ),
        (
            "AUTONEG_ADVERTISE_10_100_ALL",
            AUTONEG_ADVERTISE_10_100_ALL as i64
        ),
        ("AUTONEG_ADVERTISE_10_ALL", AUTONEG_ADVERTISE_10_ALL as i64),
        (
            "EEPROM_CHECKSUM_REG_ICP_xxxx",
            EEPROM_CHECKSUM_REG_ICP_xxxx as i64
        ),
        ("PCI_CAP_ID_ST", PCI_CAP_ID_ST as i64),
        ("PCI_ST_SMIA_OFFSET", PCI_ST_SMIA_OFFSET as i64),
        ("E1000_IMC1", E1000_IMC1 as i64),
        ("E1000_IMC2", E1000_IMC2 as i64),
        ("E1000_82542_IMC1", E1000_82542_IMC1 as i64),
        ("E1000_82542_IMC2", E1000_82542_IMC2 as i64),
        ("E1000_NVM_K1_CONFIG", E1000_NVM_K1_CONFIG as i64),
        ("E1000_NVM_K1_ENABLE", E1000_NVM_K1_ENABLE as i64),
        ("E1000_KMRNCTRLSTA_OFFSET", E1000_KMRNCTRLSTA_OFFSET as i64),
        (
            "E1000_KMRNCTRLSTA_OFFSET_SHIFT",
            E1000_KMRNCTRLSTA_OFFSET_SHIFT as i64
        ),
        ("E1000_KMRNCTRLSTA_REN", E1000_KMRNCTRLSTA_REN as i64),
        (
            "E1000_KMRNCTRLSTA_DIAG_OFFSET",
            E1000_KMRNCTRLSTA_DIAG_OFFSET as i64
        ),
        (
            "E1000_KMRNCTRLSTA_TIMEOUTS",
            E1000_KMRNCTRLSTA_TIMEOUTS as i64
        ),
        (
            "E1000_KMRNCTRLSTA_INBAND_PARAM",
            E1000_KMRNCTRLSTA_INBAND_PARAM as i64
        ),
        (
            "E1000_KMRNCTRLSTA_DIAG_NELPBK",
            E1000_KMRNCTRLSTA_DIAG_NELPBK as i64
        ),
        (
            "E1000_KMRNCTRLSTA_K1_CONFIG",
            E1000_KMRNCTRLSTA_K1_CONFIG as i64
        ),
        (
            "E1000_KMRNCTRLSTA_K1_ENABLE",
            E1000_KMRNCTRLSTA_K1_ENABLE as i64
        ),
        (
            "E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE",
            E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE as i64
        ),
        (
            "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK",
            E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK as i64
        ),
        (
            "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT",
            E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT as i64
        ),
        (
            "E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK",
            E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK as i64
        ),
        (
            "E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT",
            E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT as i64
        ),
        ("CV_SMB_CTRL", CV_SMB_CTRL as i64),
        ("CV_SMB_CTRL_FORCE_SMBUS", CV_SMB_CTRL_FORCE_SMBUS as i64),
        ("I218_ULP_CONFIG1", I218_ULP_CONFIG1 as i64),
        ("I218_ULP_CONFIG1_START", I218_ULP_CONFIG1_START as i64),
        ("I218_ULP_CONFIG1_IND", I218_ULP_CONFIG1_IND as i64),
        (
            "I218_ULP_CONFIG1_STICKY_ULP",
            I218_ULP_CONFIG1_STICKY_ULP as i64
        ),
        (
            "I218_ULP_CONFIG1_INBAND_EXIT",
            I218_ULP_CONFIG1_INBAND_EXIT as i64
        ),
        (
            "I218_ULP_CONFIG1_WOL_HOST",
            I218_ULP_CONFIG1_WOL_HOST as i64
        ),
        (
            "I218_ULP_CONFIG1_RESET_TO_SMBUS",
            I218_ULP_CONFIG1_RESET_TO_SMBUS as i64
        ),
        (
            "I218_ULP_CONFIG1_EN_ULP_LANPHYPC",
            I218_ULP_CONFIG1_EN_ULP_LANPHYPC as i64
        ),
        (
            "I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST",
            I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST as i64
        ),
        (
            "I218_ULP_CONFIG1_DISABLE_SMB_PERST",
            I218_ULP_CONFIG1_DISABLE_SMB_PERST as i64
        ),
        ("HV_INTC_FC_PAGE_START", HV_INTC_FC_PAGE_START as i64),
        ("HV_SCC_UPPER", HV_SCC_UPPER as i64),
        ("HV_SCC_LOWER", HV_SCC_LOWER as i64),
        ("HV_ECOL_UPPER", HV_ECOL_UPPER as i64),
        ("HV_ECOL_LOWER", HV_ECOL_LOWER as i64),
        ("HV_MCC_UPPER", HV_MCC_UPPER as i64),
        ("HV_MCC_LOWER", HV_MCC_LOWER as i64),
        ("HV_LATECOL_UPPER", HV_LATECOL_UPPER as i64),
        ("HV_LATECOL_LOWER", HV_LATECOL_LOWER as i64),
        ("HV_COLC_UPPER", HV_COLC_UPPER as i64),
        ("HV_COLC_LOWER", HV_COLC_LOWER as i64),
        ("HV_DC_UPPER", HV_DC_UPPER as i64),
        ("HV_DC_LOWER", HV_DC_LOWER as i64),
        ("HV_TNCRS_UPPER", HV_TNCRS_UPPER as i64),
        ("HV_TNCRS_LOWER", HV_TNCRS_LOWER as i64),
        ("HV_OEM_BITS", HV_OEM_BITS as i64),
        ("HV_OEM_BITS_LPLU", HV_OEM_BITS_LPLU as i64),
        ("HV_OEM_BITS_GBE_DIS", HV_OEM_BITS_GBE_DIS as i64),
        ("HV_OEM_BITS_RESTART_AN", HV_OEM_BITS_RESTART_AN as i64),
        ("HV_MUX_DATA_CTRL", HV_MUX_DATA_CTRL as i64),
        (
            "HV_MUX_DATA_CTRL_GEN_TO_MAC",
            HV_MUX_DATA_CTRL_GEN_TO_MAC as i64
        ),
        (
            "HV_MUX_DATA_CTRL_FORCE_SPEED",
            HV_MUX_DATA_CTRL_FORCE_SPEED as i64
        ),
        ("HV_KMRN_MODE_CTRL", HV_KMRN_MODE_CTRL as i64),
        ("HV_KMRN_MDIO_SLOW", HV_KMRN_MDIO_SLOW as i64),
        ("HV_PM_CTRL", HV_PM_CTRL as i64),
        ("HV_PM_CTRL_K1_CLK_REQ", HV_PM_CTRL_K1_CLK_REQ as i64),
        ("HV_PM_CTRL_K1_ENABLE", HV_PM_CTRL_K1_ENABLE as i64),
        ("I2_DFT_CTRL", I2_DFT_CTRL as i64),
        ("I2_SMBUS_CTRL", I2_SMBUS_CTRL as i64),
        ("I2_MODE_CTRL", I2_MODE_CTRL as i64),
        ("I2_PCIE_POWER_CTRL", I2_PCIE_POWER_CTRL as i64),
        ("E1000_FEXTNVM7", E1000_FEXTNVM7 as i64),
        (
            "E1000_FEXTNVM7_SIDE_CLK_UNGATE",
            E1000_FEXTNVM7_SIDE_CLK_UNGATE as i64
        ),
        (
            "E1000_FEXTNVM7_DISABLE_SMB_PERST",
            E1000_FEXTNVM7_DISABLE_SMB_PERST as i64
        ),
        ("E1000_FEXTNVM9", E1000_FEXTNVM9 as i64),
        (
            "E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS",
            E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS as i64
        ),
        (
            "E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS",
            E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS as i64
        ),
        ("E1000_FEXTNVM11", E1000_FEXTNVM11 as i64),
        (
            "E1000_FEXTNVM11_DISABLE_MULR_FIX",
            E1000_FEXTNVM11_DISABLE_MULR_FIX as i64
        ),
        ("BM_PCIE_PAGE", BM_PCIE_PAGE as i64),
        ("BM_WUC_PAGE", BM_WUC_PAGE as i64),
        ("BM_WUC_ADDRESS_OPCODE", BM_WUC_ADDRESS_OPCODE as i64),
        ("BM_WUC_DATA_OPCODE", BM_WUC_DATA_OPCODE as i64),
        ("BM_WUC_ENABLE_PAGE", BM_WUC_ENABLE_PAGE as i64),
        ("BM_WUC_ENABLE_REG", BM_WUC_ENABLE_REG as i64),
        ("BM_WUC_ENABLE_BIT", BM_WUC_ENABLE_BIT as i64),
        ("BM_WUC_HOST_WU_BIT", BM_WUC_HOST_WU_BIT as i64),
        ("BM_CS_STATUS", BM_CS_STATUS as i64),
        (
            "BM_CS_STATUS_ENERGY_DETECT",
            BM_CS_STATUS_ENERGY_DETECT as i64
        ),
        ("BM_CS_STATUS_LINK_UP", BM_CS_STATUS_LINK_UP as i64),
        ("BM_CS_STATUS_RESOLVED", BM_CS_STATUS_RESOLVED as i64),
        ("BM_CS_STATUS_SPEED_MASK", BM_CS_STATUS_SPEED_MASK as i64),
        ("BM_CS_STATUS_SPEED_1000", BM_CS_STATUS_SPEED_1000 as i64),
        ("HV_M_STATUS", HV_M_STATUS as i64),
        (
            "HV_M_STATUS_AUTONEG_COMPLETE",
            HV_M_STATUS_AUTONEG_COMPLETE as i64
        ),
        ("HV_M_STATUS_SPEED_MASK", HV_M_STATUS_SPEED_MASK as i64),
        ("HV_M_STATUS_SPEED_1000", HV_M_STATUS_SPEED_1000 as i64),
        ("HV_M_STATUS_LINK_UP", HV_M_STATUS_LINK_UP as i64),
        ("I217_INBAND_CTRL", I217_INBAND_CTRL as i64),
        (
            "I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK",
            I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK as i64
        ),
        (
            "I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT",
            I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT as i64
        ),
        ("E1000_PHY_TIMEOUTS_REG", E1000_PHY_TIMEOUTS_REG as i64),
        (
            "E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK",
            E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK as i64
        ),
        ("I82579_LPI_CTRL", I82579_LPI_CTRL as i64),
        (
            "I82579_LPI_CTRL_ENABLE_MASK",
            I82579_LPI_CTRL_ENABLE_MASK as i64
        ),
        (
            "I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT",
            I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT as i64
        ),
        ("I82579_EMI_ADDR", I82579_EMI_ADDR as i64),
        ("I82579_EMI_DATA", I82579_EMI_DATA as i64),
        ("I82579_LPI_UPDATE_TIMER", I82579_LPI_UPDATE_TIMER as i64),
        ("I82579_MSE_THRESHOLD", I82579_MSE_THRESHOLD as i64),
        ("I82579_MSE_LINK_DOWN", I82579_MSE_LINK_DOWN as i64),
        ("INVM_SIZE", INVM_SIZE as i64),
        (
            "NVM_INIT_CTRL_2_DEFAULT_I211",
            NVM_INIT_CTRL_2_DEFAULT_I211 as i64
        ),
        (
            "NVM_INIT_CTRL_4_DEFAULT_I211",
            NVM_INIT_CTRL_4_DEFAULT_I211 as i64
        ),
        (
            "NVM_LED_1_CFG_DEFAULT_I211",
            NVM_LED_1_CFG_DEFAULT_I211 as i64
        ),
        (
            "NVM_LED_0_2_CFG_DEFAULT_I211",
            NVM_LED_0_2_CFG_DEFAULT_I211 as i64
        ),
        ("NVM_RESERVED_WORD", NVM_RESERVED_WORD as i64),
        (
            "INVM_UNINITIALIZED_STRUCTURE",
            INVM_UNINITIALIZED_STRUCTURE as i64
        ),
        (
            "INVM_WORD_AUTOLOAD_STRUCTURE",
            INVM_WORD_AUTOLOAD_STRUCTURE as i64
        ),
        (
            "INVM_CSR_AUTOLOAD_STRUCTURE",
            INVM_CSR_AUTOLOAD_STRUCTURE as i64
        ),
        (
            "INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE",
            INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE as i64
        ),
        (
            "INVM_RSA_KEY_SHA256_STRUCTURE",
            INVM_RSA_KEY_SHA256_STRUCTURE as i64
        ),
        (
            "INVM_INVALIDATED_STRUCTURE",
            INVM_INVALIDATED_STRUCTURE as i64
        ),
        (
            "INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS",
            INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS as i64
        ),
        (
            "INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS",
            INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS as i64
        ),
        ("PHY_UPPER_SHIFT", PHY_UPPER_SHIFT as i64),
        (
            "E1000_SFF_IDENTIFIER_OFFSET",
            E1000_SFF_IDENTIFIER_OFFSET as i64
        ),
        ("E1000_SFF_IDENTIFIER_SFF", E1000_SFF_IDENTIFIER_SFF as i64),
        ("E1000_SFF_IDENTIFIER_SFP", E1000_SFF_IDENTIFIER_SFP as i64),
        (
            "E1000_SFF_ETH_FLAGS_OFFSET",
            E1000_SFF_ETH_FLAGS_OFFSET as i64
        ),
        (
            "E1000_SFF_VENDOR_OUI_TYCO",
            E1000_SFF_VENDOR_OUI_TYCO as i64
        ),
        ("E1000_SFF_VENDOR_OUI_FTL", E1000_SFF_VENDOR_OUI_FTL as i64),
        (
            "E1000_SFF_VENDOR_OUI_AVAGO",
            E1000_SFF_VENDOR_OUI_AVAGO as i64
        ),
        (
            "E1000_SFF_VENDOR_OUI_INTEL",
            E1000_SFF_VENDOR_OUI_INTEL as i64
        ),
    ]
}

/// Every define of the header against the C (`just test-ref`): the value where the reader
/// can evaluate it (literals, other defines, `|`, `<<`, `+`, `-`, `*`), and that nothing in
/// the header is left out.
#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn defines_match_reference() {
    let defs = crate::reftest::defines("sys/dev/pci/if_em_hw.h");
    let ours = header_defines();
    let mut checked = 0;
    for (name, value) in &ours {
        let text = defs
            .get(*name)
            .unwrap_or_else(|| panic!("{name} is not in the header"));
        match crate::reftest::int(&defs, name) {
            Some(v) => {
                assert_eq!(v, *value, "{name}");
                checked += 1;
            }
            // Multi-line defines, casts and the PHY_REG()/GG82563_REG() forms are beyond
            // the reader; the host tests check the macros themselves.
            None => assert!(
                text.contains('(')
                    || text.ends_with('\\')
                    || text.is_empty()
                    || defs.contains_key(text.as_str()),
                "{name} = {text} was not evaluated"
            ),
        }
    }
    assert!(checked > 2000, "only {checked} defines checked");
    for name in defs.keys() {
        let known = ours.iter().any(|(n, _)| n == name)
            || matches!(
                name.as_str(),
                "_EM_HW_H_" | "E1000_RDBAL0" | "E1000_RDBAH0" | "E1000_RDLEN0"
            );
        assert!(known, "{name} is not ported");
    }
}

/// The two IGP cable length tables against the C's initialisers.
#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn cable_length_tables_match_reference() {
    let path = crate::reftest::openbsd_src().join("sys/dev/pci/if_em_hw.c");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let table = |name: &str| -> std::vec::Vec<u16> {
        let at = text
            .find(&std::format!("{name}["))
            .unwrap_or_else(|| panic!("{name}"));
        let open = at + text[at..].find('{').expect("{");
        let close = open + text[open..].find('}').expect("}");
        text[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().expect("number"))
            .collect()
    };
    assert_eq!(
        table("em_igp_cable_length_table"),
        EM_IGP_CABLE_LENGTH_TABLE.to_vec()
    );
    assert_eq!(
        table("em_igp_2_cable_length_table"),
        EM_IGP_2_CABLE_LENGTH_TABLE.to_vec()
    );
}
