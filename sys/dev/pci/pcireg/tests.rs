//! Host tests of `pcireg.h`'s macros, and the constants against the C header.

use super::*;

#[test]
fn bar_sizes_decode_from_the_all_ones_readback() {
    // A 4 KiB 32-bit memory BAR reads back 0xfffff000 (plus its type bits).
    assert_eq!(pci_mapreg_mem_size(0xffff_f000), 0x1000);
    assert_eq!(pci_mapreg_mem_size(0xffff_f008), 0x1000);
    assert_eq!(pci_mapreg_mem_addr(0xfebd_1008), 0xfebd_1000);
    assert!(pci_mapreg_mem_prefetchable(0xfebd_1008));
    // A 32-byte I/O BAR reads back 0xffffffe1.
    assert_eq!(pci_mapreg_io_size(0xffff_ffe1), 0x20);
    assert_eq!(pci_mapreg_io_addr(0xc041), 0xc040);
    // A 16 KiB 64-bit BAR: both halves.
    assert_eq!(pci_mapreg_mem64_size(0xffff_ffff_ffff_c00c), 0x4000);
    assert_eq!(pci_mapreg_mem64_addr(0x0000_0008_0000_000c), 0x8_0000_0000);
    // An unimplemented BAR reads back 0: no size.
    assert_eq!(pci_mapreg_mem_size(0), 0);
    assert_eq!(pci_rom_size(0xffff_8001), 0x8000);
    // The type bits.
    assert_eq!(_pci_mapreg_typebits(0xc041), PCI_MAPREG_TYPE_IO);
    assert_eq!(
        _pci_mapreg_typebits(0xfe00_000c),
        PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT
    );
    assert_eq!(PCI_MAPREG_TYPE(0xc041), PCI_MAPREG_TYPE_IO);
    assert_eq!(pci_mapreg_mem_type(0xfe00_0004), PCI_MAPREG_MEM_TYPE_64BIT);
}

#[test]
fn register_fields() {
    let id = pci_id_code(0x8086, 0x29c0);
    assert_eq!(id, 0x29c0_8086);
    assert_eq!((pci_vendor(id), pci_product(id)), (0x8086, 0x29c0));
    let class = 0x0601_0002;
    assert_eq!(pci_class(class), PCI_CLASS_BRIDGE);
    assert_eq!(pci_subclass(class), PCI_SUBCLASS_BRIDGE_ISA);
    assert_eq!(pci_revision(class), 2);
    let bhlc = 0x0080_0000;
    assert_eq!(pci_hdrtype_type(bhlc), 0);
    assert!(pci_hdrtype_multifn(bhlc));
    assert_eq!(pci_interrupt_pin(0x0000_010b), PCI_INTERRUPT_PIN_A);
    assert_eq!(pci_interrupt_line(0x0000_010b), 11);
    assert_eq!(pci_caplist_next(0x0000_6005), 0x60);
    assert_eq!(pci_caplist_cap(0x0000_6005), PCI_CAP_MSI as u32);
    assert_eq!(pci_msix_vc(2), 44);
    assert_eq!(pci_pcie_ecap_next(0x1401_0001), 0x140);
    assert_eq!(pci_ht_cap(0xa800_0000), PCI_HT_CAP_MSI as u32);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/pci/pcireg.h");
    let ours: &[(&str, i64)] = &[
        ("PCI_CONFIG_SPACE_SIZE", i64::from(PCI_CONFIG_SPACE_SIZE)),
        ("PCIE_CONFIG_SPACE_SIZE", i64::from(PCIE_CONFIG_SPACE_SIZE)),
        ("PCI_ID_REG", i64::from(PCI_ID_REG)),
        ("PCI_VENDOR_SHIFT", i64::from(PCI_VENDOR_SHIFT)),
        ("PCI_VENDOR_MASK", i64::from(PCI_VENDOR_MASK)),
        ("PCI_PRODUCT_SHIFT", i64::from(PCI_PRODUCT_SHIFT)),
        ("PCI_PRODUCT_MASK", i64::from(PCI_PRODUCT_MASK)),
        ("PCI_COMMAND_STATUS_REG", i64::from(PCI_COMMAND_STATUS_REG)),
        ("PCI_COMMAND_IO_ENABLE", i64::from(PCI_COMMAND_IO_ENABLE)),
        ("PCI_COMMAND_MEM_ENABLE", i64::from(PCI_COMMAND_MEM_ENABLE)),
        (
            "PCI_COMMAND_MASTER_ENABLE",
            i64::from(PCI_COMMAND_MASTER_ENABLE),
        ),
        (
            "PCI_COMMAND_SPECIAL_ENABLE",
            i64::from(PCI_COMMAND_SPECIAL_ENABLE),
        ),
        (
            "PCI_COMMAND_INVALIDATE_ENABLE",
            i64::from(PCI_COMMAND_INVALIDATE_ENABLE),
        ),
        (
            "PCI_COMMAND_PALETTE_ENABLE",
            i64::from(PCI_COMMAND_PALETTE_ENABLE),
        ),
        (
            "PCI_COMMAND_PARITY_ENABLE",
            i64::from(PCI_COMMAND_PARITY_ENABLE),
        ),
        (
            "PCI_COMMAND_STEPPING_ENABLE",
            i64::from(PCI_COMMAND_STEPPING_ENABLE),
        ),
        (
            "PCI_COMMAND_SERR_ENABLE",
            i64::from(PCI_COMMAND_SERR_ENABLE),
        ),
        (
            "PCI_COMMAND_BACKTOBACK_ENABLE",
            i64::from(PCI_COMMAND_BACKTOBACK_ENABLE),
        ),
        (
            "PCI_COMMAND_INTERRUPT_DISABLE",
            i64::from(PCI_COMMAND_INTERRUPT_DISABLE),
        ),
        (
            "PCI_STATUS_CAPLIST_SUPPORT",
            i64::from(PCI_STATUS_CAPLIST_SUPPORT),
        ),
        (
            "PCI_STATUS_66MHZ_SUPPORT",
            i64::from(PCI_STATUS_66MHZ_SUPPORT),
        ),
        ("PCI_STATUS_UDF_SUPPORT", i64::from(PCI_STATUS_UDF_SUPPORT)),
        (
            "PCI_STATUS_BACKTOBACK_SUPPORT",
            i64::from(PCI_STATUS_BACKTOBACK_SUPPORT),
        ),
        (
            "PCI_STATUS_PARITY_ERROR",
            i64::from(PCI_STATUS_PARITY_ERROR),
        ),
        ("PCI_STATUS_DEVSEL_FAST", i64::from(PCI_STATUS_DEVSEL_FAST)),
        (
            "PCI_STATUS_DEVSEL_MEDIUM",
            i64::from(PCI_STATUS_DEVSEL_MEDIUM),
        ),
        ("PCI_STATUS_DEVSEL_SLOW", i64::from(PCI_STATUS_DEVSEL_SLOW)),
        ("PCI_STATUS_DEVSEL_MASK", i64::from(PCI_STATUS_DEVSEL_MASK)),
        (
            "PCI_STATUS_TARGET_TARGET_ABORT",
            i64::from(PCI_STATUS_TARGET_TARGET_ABORT),
        ),
        (
            "PCI_STATUS_MASTER_TARGET_ABORT",
            i64::from(PCI_STATUS_MASTER_TARGET_ABORT),
        ),
        (
            "PCI_STATUS_MASTER_ABORT",
            i64::from(PCI_STATUS_MASTER_ABORT),
        ),
        (
            "PCI_STATUS_SPECIAL_ERROR",
            i64::from(PCI_STATUS_SPECIAL_ERROR),
        ),
        (
            "PCI_STATUS_PARITY_DETECT",
            i64::from(PCI_STATUS_PARITY_DETECT),
        ),
        ("PCI_CLASS_REG", i64::from(PCI_CLASS_REG)),
        ("PCI_CLASS_SHIFT", i64::from(PCI_CLASS_SHIFT)),
        ("PCI_CLASS_MASK", i64::from(PCI_CLASS_MASK)),
        ("PCI_SUBCLASS_SHIFT", i64::from(PCI_SUBCLASS_SHIFT)),
        ("PCI_SUBCLASS_MASK", i64::from(PCI_SUBCLASS_MASK)),
        ("PCI_INTERFACE_SHIFT", i64::from(PCI_INTERFACE_SHIFT)),
        ("PCI_INTERFACE_MASK", i64::from(PCI_INTERFACE_MASK)),
        ("PCI_REVISION_SHIFT", i64::from(PCI_REVISION_SHIFT)),
        ("PCI_REVISION_MASK", i64::from(PCI_REVISION_MASK)),
        ("PCI_CLASS_PREHISTORIC", i64::from(PCI_CLASS_PREHISTORIC)),
        ("PCI_CLASS_MASS_STORAGE", i64::from(PCI_CLASS_MASS_STORAGE)),
        ("PCI_CLASS_NETWORK", i64::from(PCI_CLASS_NETWORK)),
        ("PCI_CLASS_DISPLAY", i64::from(PCI_CLASS_DISPLAY)),
        ("PCI_CLASS_MULTIMEDIA", i64::from(PCI_CLASS_MULTIMEDIA)),
        ("PCI_CLASS_MEMORY", i64::from(PCI_CLASS_MEMORY)),
        ("PCI_CLASS_BRIDGE", i64::from(PCI_CLASS_BRIDGE)),
        (
            "PCI_CLASS_COMMUNICATIONS",
            i64::from(PCI_CLASS_COMMUNICATIONS),
        ),
        ("PCI_CLASS_SYSTEM", i64::from(PCI_CLASS_SYSTEM)),
        ("PCI_CLASS_INPUT", i64::from(PCI_CLASS_INPUT)),
        ("PCI_CLASS_DOCK", i64::from(PCI_CLASS_DOCK)),
        ("PCI_CLASS_PROCESSOR", i64::from(PCI_CLASS_PROCESSOR)),
        ("PCI_CLASS_SERIALBUS", i64::from(PCI_CLASS_SERIALBUS)),
        ("PCI_CLASS_WIRELESS", i64::from(PCI_CLASS_WIRELESS)),
        ("PCI_CLASS_I2O", i64::from(PCI_CLASS_I2O)),
        ("PCI_CLASS_SATCOM", i64::from(PCI_CLASS_SATCOM)),
        ("PCI_CLASS_CRYPTO", i64::from(PCI_CLASS_CRYPTO)),
        ("PCI_CLASS_DASP", i64::from(PCI_CLASS_DASP)),
        ("PCI_CLASS_ACCELERATOR", i64::from(PCI_CLASS_ACCELERATOR)),
        (
            "PCI_CLASS_INSTRUMENTATION",
            i64::from(PCI_CLASS_INSTRUMENTATION),
        ),
        ("PCI_CLASS_UNDEFINED", i64::from(PCI_CLASS_UNDEFINED)),
        (
            "PCI_SUBCLASS_PREHISTORIC_MISC",
            i64::from(PCI_SUBCLASS_PREHISTORIC_MISC),
        ),
        (
            "PCI_SUBCLASS_PREHISTORIC_VGA",
            i64::from(PCI_SUBCLASS_PREHISTORIC_VGA),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_SCSI",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_SCSI),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_IDE",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_IDE),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_FLOPPY",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_FLOPPY),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_IPI",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_IPI),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_RAID",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_RAID),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_ATA",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_ATA),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_SATA",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_SATA),
        ),
        (
            "PCI_INTERFACE_SATA_AHCI10",
            i64::from(PCI_INTERFACE_SATA_AHCI10),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_SAS",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_SAS),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_NVM",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_NVM),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_UFS",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_UFS),
        ),
        (
            "PCI_SUBCLASS_MASS_STORAGE_MISC",
            i64::from(PCI_SUBCLASS_MASS_STORAGE_MISC),
        ),
        (
            "PCI_SUBCLASS_NETWORK_ETHERNET",
            i64::from(PCI_SUBCLASS_NETWORK_ETHERNET),
        ),
        (
            "PCI_SUBCLASS_NETWORK_TOKENRING",
            i64::from(PCI_SUBCLASS_NETWORK_TOKENRING),
        ),
        (
            "PCI_SUBCLASS_NETWORK_FDDI",
            i64::from(PCI_SUBCLASS_NETWORK_FDDI),
        ),
        (
            "PCI_SUBCLASS_NETWORK_ATM",
            i64::from(PCI_SUBCLASS_NETWORK_ATM),
        ),
        (
            "PCI_SUBCLASS_NETWORK_ISDN",
            i64::from(PCI_SUBCLASS_NETWORK_ISDN),
        ),
        (
            "PCI_SUBCLASS_NETWORK_WORLDFIP",
            i64::from(PCI_SUBCLASS_NETWORK_WORLDFIP),
        ),
        (
            "PCI_SUBCLASS_NETWORK_PCIMGMULTICOMP",
            i64::from(PCI_SUBCLASS_NETWORK_PCIMGMULTICOMP),
        ),
        (
            "PCI_SUBCLASS_NETWORK_INFINIBAND",
            i64::from(PCI_SUBCLASS_NETWORK_INFINIBAND),
        ),
        (
            "PCI_SUBCLASS_NETWORK_MISC",
            i64::from(PCI_SUBCLASS_NETWORK_MISC),
        ),
        (
            "PCI_SUBCLASS_DISPLAY_VGA",
            i64::from(PCI_SUBCLASS_DISPLAY_VGA),
        ),
        (
            "PCI_SUBCLASS_DISPLAY_XGA",
            i64::from(PCI_SUBCLASS_DISPLAY_XGA),
        ),
        (
            "PCI_SUBCLASS_DISPLAY_3D",
            i64::from(PCI_SUBCLASS_DISPLAY_3D),
        ),
        (
            "PCI_SUBCLASS_DISPLAY_MISC",
            i64::from(PCI_SUBCLASS_DISPLAY_MISC),
        ),
        (
            "PCI_SUBCLASS_MULTIMEDIA_VIDEO",
            i64::from(PCI_SUBCLASS_MULTIMEDIA_VIDEO),
        ),
        (
            "PCI_SUBCLASS_MULTIMEDIA_AUDIO",
            i64::from(PCI_SUBCLASS_MULTIMEDIA_AUDIO),
        ),
        (
            "PCI_SUBCLASS_MULTIMEDIA_TELEPHONY",
            i64::from(PCI_SUBCLASS_MULTIMEDIA_TELEPHONY),
        ),
        (
            "PCI_SUBCLASS_MULTIMEDIA_HDAUDIO",
            i64::from(PCI_SUBCLASS_MULTIMEDIA_HDAUDIO),
        ),
        (
            "PCI_SUBCLASS_MULTIMEDIA_MISC",
            i64::from(PCI_SUBCLASS_MULTIMEDIA_MISC),
        ),
        (
            "PCI_SUBCLASS_MEMORY_RAM",
            i64::from(PCI_SUBCLASS_MEMORY_RAM),
        ),
        (
            "PCI_SUBCLASS_MEMORY_FLASH",
            i64::from(PCI_SUBCLASS_MEMORY_FLASH),
        ),
        (
            "PCI_SUBCLASS_MEMORY_MISC",
            i64::from(PCI_SUBCLASS_MEMORY_MISC),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_HOST",
            i64::from(PCI_SUBCLASS_BRIDGE_HOST),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_ISA",
            i64::from(PCI_SUBCLASS_BRIDGE_ISA),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_EISA",
            i64::from(PCI_SUBCLASS_BRIDGE_EISA),
        ),
        ("PCI_SUBCLASS_BRIDGE_MC", i64::from(PCI_SUBCLASS_BRIDGE_MC)),
        (
            "PCI_SUBCLASS_BRIDGE_PCI",
            i64::from(PCI_SUBCLASS_BRIDGE_PCI),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_PCMCIA",
            i64::from(PCI_SUBCLASS_BRIDGE_PCMCIA),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_NUBUS",
            i64::from(PCI_SUBCLASS_BRIDGE_NUBUS),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_CARDBUS",
            i64::from(PCI_SUBCLASS_BRIDGE_CARDBUS),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_RACEWAY",
            i64::from(PCI_SUBCLASS_BRIDGE_RACEWAY),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_STPCI",
            i64::from(PCI_SUBCLASS_BRIDGE_STPCI),
        ),
        (
            "PCI_SUBCLASS_BRIDGE_INFINIBAND",
            i64::from(PCI_SUBCLASS_BRIDGE_INFINIBAND),
        ),
        ("PCI_SUBCLASS_BRIDGE_AS", i64::from(PCI_SUBCLASS_BRIDGE_AS)),
        (
            "PCI_SUBCLASS_BRIDGE_MISC",
            i64::from(PCI_SUBCLASS_BRIDGE_MISC),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_SERIAL",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_SERIAL),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_PARALLEL",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_PARALLEL),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_MPSERIAL",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_MPSERIAL),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_MODEM",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_MODEM),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_GPIB",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_GPIB),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_SMARTCARD",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_SMARTCARD),
        ),
        (
            "PCI_SUBCLASS_COMMUNICATIONS_MISC",
            i64::from(PCI_SUBCLASS_COMMUNICATIONS_MISC),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_PIC",
            i64::from(PCI_SUBCLASS_SYSTEM_PIC),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_DMA",
            i64::from(PCI_SUBCLASS_SYSTEM_DMA),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_TIMER",
            i64::from(PCI_SUBCLASS_SYSTEM_TIMER),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_RTC",
            i64::from(PCI_SUBCLASS_SYSTEM_RTC),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_PCIHOTPLUG",
            i64::from(PCI_SUBCLASS_SYSTEM_PCIHOTPLUG),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_SDHC",
            i64::from(PCI_SUBCLASS_SYSTEM_SDHC),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_IOMMU",
            i64::from(PCI_SUBCLASS_SYSTEM_IOMMU),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_ROOTCOMPEVENT",
            i64::from(PCI_SUBCLASS_SYSTEM_ROOTCOMPEVENT),
        ),
        (
            "PCI_SUBCLASS_SYSTEM_MISC",
            i64::from(PCI_SUBCLASS_SYSTEM_MISC),
        ),
        (
            "PCI_SUBCLASS_INPUT_KEYBOARD",
            i64::from(PCI_SUBCLASS_INPUT_KEYBOARD),
        ),
        (
            "PCI_SUBCLASS_INPUT_DIGITIZER",
            i64::from(PCI_SUBCLASS_INPUT_DIGITIZER),
        ),
        (
            "PCI_SUBCLASS_INPUT_MOUSE",
            i64::from(PCI_SUBCLASS_INPUT_MOUSE),
        ),
        (
            "PCI_SUBCLASS_INPUT_SCANNER",
            i64::from(PCI_SUBCLASS_INPUT_SCANNER),
        ),
        (
            "PCI_SUBCLASS_INPUT_GAMEPORT",
            i64::from(PCI_SUBCLASS_INPUT_GAMEPORT),
        ),
        (
            "PCI_SUBCLASS_INPUT_MISC",
            i64::from(PCI_SUBCLASS_INPUT_MISC),
        ),
        (
            "PCI_SUBCLASS_DOCK_GENERIC",
            i64::from(PCI_SUBCLASS_DOCK_GENERIC),
        ),
        ("PCI_SUBCLASS_DOCK_MISC", i64::from(PCI_SUBCLASS_DOCK_MISC)),
        (
            "PCI_SUBCLASS_PROCESSOR_386",
            i64::from(PCI_SUBCLASS_PROCESSOR_386),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_486",
            i64::from(PCI_SUBCLASS_PROCESSOR_486),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_PENTIUM",
            i64::from(PCI_SUBCLASS_PROCESSOR_PENTIUM),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_ALPHA",
            i64::from(PCI_SUBCLASS_PROCESSOR_ALPHA),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_POWERPC",
            i64::from(PCI_SUBCLASS_PROCESSOR_POWERPC),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_MIPS",
            i64::from(PCI_SUBCLASS_PROCESSOR_MIPS),
        ),
        (
            "PCI_SUBCLASS_PROCESSOR_COPROC",
            i64::from(PCI_SUBCLASS_PROCESSOR_COPROC),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_FIREWIRE",
            i64::from(PCI_SUBCLASS_SERIALBUS_FIREWIRE),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_ACCESS",
            i64::from(PCI_SUBCLASS_SERIALBUS_ACCESS),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_SSA",
            i64::from(PCI_SUBCLASS_SERIALBUS_SSA),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_USB",
            i64::from(PCI_SUBCLASS_SERIALBUS_USB),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_FIBER",
            i64::from(PCI_SUBCLASS_SERIALBUS_FIBER),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_SMBUS",
            i64::from(PCI_SUBCLASS_SERIALBUS_SMBUS),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_INFINIBAND",
            i64::from(PCI_SUBCLASS_SERIALBUS_INFINIBAND),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_IPMI",
            i64::from(PCI_SUBCLASS_SERIALBUS_IPMI),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_SERCOS",
            i64::from(PCI_SUBCLASS_SERIALBUS_SERCOS),
        ),
        (
            "PCI_SUBCLASS_SERIALBUS_CANBUS",
            i64::from(PCI_SUBCLASS_SERIALBUS_CANBUS),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_IRDA",
            i64::from(PCI_SUBCLASS_WIRELESS_IRDA),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_CONSUMERIR",
            i64::from(PCI_SUBCLASS_WIRELESS_CONSUMERIR),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_RF",
            i64::from(PCI_SUBCLASS_WIRELESS_RF),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_BLUETOOTH",
            i64::from(PCI_SUBCLASS_WIRELESS_BLUETOOTH),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_BROADBAND",
            i64::from(PCI_SUBCLASS_WIRELESS_BROADBAND),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_802_11A",
            i64::from(PCI_SUBCLASS_WIRELESS_802_11A),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_802_11B",
            i64::from(PCI_SUBCLASS_WIRELESS_802_11B),
        ),
        (
            "PCI_SUBCLASS_WIRELESS_MISC",
            i64::from(PCI_SUBCLASS_WIRELESS_MISC),
        ),
        (
            "PCI_SUBCLASS_I2O_STANDARD",
            i64::from(PCI_SUBCLASS_I2O_STANDARD),
        ),
        ("PCI_SUBCLASS_SATCOM_TV", i64::from(PCI_SUBCLASS_SATCOM_TV)),
        (
            "PCI_SUBCLASS_SATCOM_AUDIO",
            i64::from(PCI_SUBCLASS_SATCOM_AUDIO),
        ),
        (
            "PCI_SUBCLASS_SATCOM_VOICE",
            i64::from(PCI_SUBCLASS_SATCOM_VOICE),
        ),
        (
            "PCI_SUBCLASS_SATCOM_DATA",
            i64::from(PCI_SUBCLASS_SATCOM_DATA),
        ),
        (
            "PCI_SUBCLASS_CRYPTO_NETCOMP",
            i64::from(PCI_SUBCLASS_CRYPTO_NETCOMP),
        ),
        (
            "PCI_SUBCLASS_CRYPTO_ENTERTAINMENT",
            i64::from(PCI_SUBCLASS_CRYPTO_ENTERTAINMENT),
        ),
        (
            "PCI_SUBCLASS_CRYPTO_MISC",
            i64::from(PCI_SUBCLASS_CRYPTO_MISC),
        ),
        ("PCI_SUBCLASS_DASP_DPIO", i64::from(PCI_SUBCLASS_DASP_DPIO)),
        (
            "PCI_SUBCLASS_DASP_TIMEFREQ",
            i64::from(PCI_SUBCLASS_DASP_TIMEFREQ),
        ),
        ("PCI_SUBCLASS_DASP_SYNC", i64::from(PCI_SUBCLASS_DASP_SYNC)),
        ("PCI_SUBCLASS_DASP_MGMT", i64::from(PCI_SUBCLASS_DASP_MGMT)),
        ("PCI_SUBCLASS_DASP_MISC", i64::from(PCI_SUBCLASS_DASP_MISC)),
        ("PCI_BHLC_REG", i64::from(PCI_BHLC_REG)),
        ("PCI_BIST_SHIFT", i64::from(PCI_BIST_SHIFT)),
        ("PCI_BIST_MASK", i64::from(PCI_BIST_MASK)),
        ("PCI_HDRTYPE_SHIFT", i64::from(PCI_HDRTYPE_SHIFT)),
        ("PCI_HDRTYPE_MASK", i64::from(PCI_HDRTYPE_MASK)),
        ("PCI_LATTIMER_SHIFT", i64::from(PCI_LATTIMER_SHIFT)),
        ("PCI_LATTIMER_MASK", i64::from(PCI_LATTIMER_MASK)),
        ("PCI_CACHELINE_SHIFT", i64::from(PCI_CACHELINE_SHIFT)),
        ("PCI_CACHELINE_MASK", i64::from(PCI_CACHELINE_MASK)),
        ("PCI_MAPS", i64::from(PCI_MAPS)),
        ("PCI_CARDBUSCIS", i64::from(PCI_CARDBUSCIS)),
        ("PCI_SUBVEND_0", i64::from(PCI_SUBVEND_0)),
        ("PCI_SUBDEV_0", i64::from(PCI_SUBDEV_0)),
        ("PCI_EXROMADDR_0", i64::from(PCI_EXROMADDR_0)),
        ("PCI_INTLINE", i64::from(PCI_INTLINE)),
        ("PCI_INTPIN", i64::from(PCI_INTPIN)),
        ("PCI_MINGNT", i64::from(PCI_MINGNT)),
        ("PCI_MAXLAT", i64::from(PCI_MAXLAT)),
        ("PCI_SECSTAT_1", i64::from(PCI_SECSTAT_1)),
        ("PCI_PRIBUS_1", i64::from(PCI_PRIBUS_1)),
        ("PCI_SECBUS_1", i64::from(PCI_SECBUS_1)),
        ("PCI_SUBBUS_1", i64::from(PCI_SUBBUS_1)),
        ("PCI_SECLAT_1", i64::from(PCI_SECLAT_1)),
        ("PCI_IOBASEL_1", i64::from(PCI_IOBASEL_1)),
        ("PCI_IOLIMITL_1", i64::from(PCI_IOLIMITL_1)),
        ("PCI_IOBASEH_1", i64::from(PCI_IOBASEH_1)),
        ("PCI_IOLIMITH_1", i64::from(PCI_IOLIMITH_1)),
        ("PCI_MEMBASE_1", i64::from(PCI_MEMBASE_1)),
        ("PCI_MEMLIMIT_1", i64::from(PCI_MEMLIMIT_1)),
        ("PCI_PMBASEL_1", i64::from(PCI_PMBASEL_1)),
        ("PCI_PMLIMITL_1", i64::from(PCI_PMLIMITL_1)),
        ("PCI_PMBASEH_1", i64::from(PCI_PMBASEH_1)),
        ("PCI_PMLIMITH_1", i64::from(PCI_PMLIMITH_1)),
        ("PCI_BRIDGECTL_1", i64::from(PCI_BRIDGECTL_1)),
        ("PCI_SUBVEND_1", i64::from(PCI_SUBVEND_1)),
        ("PCI_SUBDEV_1", i64::from(PCI_SUBDEV_1)),
        ("PCI_EXROMADDR_1", i64::from(PCI_EXROMADDR_1)),
        ("PCI_SECSTAT_2", i64::from(PCI_SECSTAT_2)),
        ("PCI_PRIBUS_2", i64::from(PCI_PRIBUS_2)),
        ("PCI_SECBUS_2", i64::from(PCI_SECBUS_2)),
        ("PCI_SUBBUS_2", i64::from(PCI_SUBBUS_2)),
        ("PCI_SECLAT_2", i64::from(PCI_SECLAT_2)),
        ("PCI_MEMBASE0_2", i64::from(PCI_MEMBASE0_2)),
        ("PCI_MEMLIMIT0_2", i64::from(PCI_MEMLIMIT0_2)),
        ("PCI_MEMBASE1_2", i64::from(PCI_MEMBASE1_2)),
        ("PCI_MEMLIMIT1_2", i64::from(PCI_MEMLIMIT1_2)),
        ("PCI_IOBASE0_2", i64::from(PCI_IOBASE0_2)),
        ("PCI_IOLIMIT0_2", i64::from(PCI_IOLIMIT0_2)),
        ("PCI_IOBASE1_2", i64::from(PCI_IOBASE1_2)),
        ("PCI_IOLIMIT1_2", i64::from(PCI_IOLIMIT1_2)),
        ("PCI_BRIDGECTL_2", i64::from(PCI_BRIDGECTL_2)),
        ("PCI_SUBVEND_2", i64::from(PCI_SUBVEND_2)),
        ("PCI_SUBDEV_2", i64::from(PCI_SUBDEV_2)),
        ("PCI_PCCARDIF_2", i64::from(PCI_PCCARDIF_2)),
        ("PCI_MAPREG_START", i64::from(PCI_MAPREG_START)),
        ("PCI_MAPREG_END", i64::from(PCI_MAPREG_END)),
        ("PCI_MAPREG_PPB_END", i64::from(PCI_MAPREG_PPB_END)),
        ("PCI_MAPREG_PCB_END", i64::from(PCI_MAPREG_PCB_END)),
        ("PCI_MAPREG_TYPE_MASK", i64::from(PCI_MAPREG_TYPE_MASK)),
        ("PCI_MAPREG_TYPE_MEM", i64::from(PCI_MAPREG_TYPE_MEM)),
        ("PCI_MAPREG_TYPE_IO", i64::from(PCI_MAPREG_TYPE_IO)),
        (
            "PCI_MAPREG_MEM_TYPE_MASK",
            i64::from(PCI_MAPREG_MEM_TYPE_MASK),
        ),
        (
            "PCI_MAPREG_MEM_TYPE_32BIT",
            i64::from(PCI_MAPREG_MEM_TYPE_32BIT),
        ),
        (
            "PCI_MAPREG_MEM_TYPE_32BIT_1M",
            i64::from(PCI_MAPREG_MEM_TYPE_32BIT_1M),
        ),
        (
            "PCI_MAPREG_MEM_TYPE_64BIT",
            i64::from(PCI_MAPREG_MEM_TYPE_64BIT),
        ),
        (
            "PCI_MAPREG_MEM_PREFETCHABLE_MASK",
            i64::from(PCI_MAPREG_MEM_PREFETCHABLE_MASK),
        ),
        (
            "PCI_MAPREG_MEM_ADDR_MASK",
            i64::from(PCI_MAPREG_MEM_ADDR_MASK),
        ),
        (
            "PCI_MAPREG_MEM64_ADDR_MASK",
            PCI_MAPREG_MEM64_ADDR_MASK as i64,
        ),
        (
            "PCI_MAPREG_IO_ADDR_MASK",
            i64::from(PCI_MAPREG_IO_ADDR_MASK),
        ),
        ("PCI_CARDBUS_CIS_REG", i64::from(PCI_CARDBUS_CIS_REG)),
        ("PCI_SUBSYS_ID_REG", i64::from(PCI_SUBSYS_ID_REG)),
        ("PCI_ROM_REG", i64::from(PCI_ROM_REG)),
        ("PCI_ROM_ENABLE", i64::from(PCI_ROM_ENABLE)),
        ("PCI_ROM_ADDR_MASK", i64::from(PCI_ROM_ADDR_MASK)),
        ("PCI_CAPLISTPTR_REG", i64::from(PCI_CAPLISTPTR_REG)),
        (
            "PCI_CARDBUS_CAPLISTPTR_REG",
            i64::from(PCI_CARDBUS_CAPLISTPTR_REG),
        ),
        ("PCI_CAP_RESERVED", i64::from(PCI_CAP_RESERVED)),
        ("PCI_CAP_PWRMGMT", i64::from(PCI_CAP_PWRMGMT)),
        ("PCI_CAP_AGP", i64::from(PCI_CAP_AGP)),
        ("PCI_CAP_VPD", i64::from(PCI_CAP_VPD)),
        ("PCI_CAP_SLOTID", i64::from(PCI_CAP_SLOTID)),
        ("PCI_CAP_MSI", i64::from(PCI_CAP_MSI)),
        ("PCI_CAP_CPCI_HOTSWAP", i64::from(PCI_CAP_CPCI_HOTSWAP)),
        ("PCI_CAP_PCIX", i64::from(PCI_CAP_PCIX)),
        ("PCI_CAP_HT", i64::from(PCI_CAP_HT)),
        ("PCI_CAP_VENDSPEC", i64::from(PCI_CAP_VENDSPEC)),
        ("PCI_CAP_DEBUGPORT", i64::from(PCI_CAP_DEBUGPORT)),
        ("PCI_CAP_CPCI_RSRCCTL", i64::from(PCI_CAP_CPCI_RSRCCTL)),
        ("PCI_CAP_HOTPLUG", i64::from(PCI_CAP_HOTPLUG)),
        ("PCI_CAP_AGP8", i64::from(PCI_CAP_AGP8)),
        ("PCI_CAP_SECURE", i64::from(PCI_CAP_SECURE)),
        ("PCI_CAP_PCIEXPRESS", i64::from(PCI_CAP_PCIEXPRESS)),
        ("PCI_CAP_MSIX", i64::from(PCI_CAP_MSIX)),
        ("PCI_CAP_SATA", i64::from(PCI_CAP_SATA)),
        ("PCI_VPD_ADDRESS_MASK", i64::from(PCI_VPD_ADDRESS_MASK)),
        ("PCI_VPD_ADDRESS_SHIFT", i64::from(PCI_VPD_ADDRESS_SHIFT)),
        ("PCI_VPD_OPFLAG", i64::from(PCI_VPD_OPFLAG)),
        ("PCI_MSI_MC", i64::from(PCI_MSI_MC)),
        ("PCI_MSI_MC_PVMASK", i64::from(PCI_MSI_MC_PVMASK)),
        ("PCI_MSI_MC_C64", i64::from(PCI_MSI_MC_C64)),
        ("PCI_MSI_MC_MME_MASK", i64::from(PCI_MSI_MC_MME_MASK)),
        ("PCI_MSI_MC_MME_SHIFT", i64::from(PCI_MSI_MC_MME_SHIFT)),
        ("PCI_MSI_MC_MMC_MASK", i64::from(PCI_MSI_MC_MMC_MASK)),
        ("PCI_MSI_MC_MMC_SHIFT", i64::from(PCI_MSI_MC_MMC_SHIFT)),
        ("PCI_MSI_MC_MSIE", i64::from(PCI_MSI_MC_MSIE)),
        ("PCI_MSI_MA", i64::from(PCI_MSI_MA)),
        ("PCI_MSI_MAU32", i64::from(PCI_MSI_MAU32)),
        ("PCI_MSI_MD32", i64::from(PCI_MSI_MD32)),
        ("PCI_MSI_MD64", i64::from(PCI_MSI_MD64)),
        ("PCI_MSI_MASK32", i64::from(PCI_MSI_MASK32)),
        ("PCI_MSI_MASK64", i64::from(PCI_MSI_MASK64)),
        ("PCI_PMCSR", i64::from(PCI_PMCSR)),
        ("PCI_PMCSR_STATE_MASK", i64::from(PCI_PMCSR_STATE_MASK)),
        ("PCI_PMCSR_STATE_D0", i64::from(PCI_PMCSR_STATE_D0)),
        ("PCI_PMCSR_STATE_D1", i64::from(PCI_PMCSR_STATE_D1)),
        ("PCI_PMCSR_STATE_D2", i64::from(PCI_PMCSR_STATE_D2)),
        ("PCI_PMCSR_STATE_D3", i64::from(PCI_PMCSR_STATE_D3)),
        ("PCI_PMCSR_PME_STATUS", i64::from(PCI_PMCSR_PME_STATUS)),
        ("PCI_PMCSR_PME_EN", i64::from(PCI_PMCSR_PME_EN)),
        ("PCI_HT_CAP_SLAVE", i64::from(PCI_HT_CAP_SLAVE)),
        ("PCI_HT_CAP_HOST", i64::from(PCI_HT_CAP_HOST)),
        ("PCI_HT_CAP_INTR", i64::from(PCI_HT_CAP_INTR)),
        ("PCI_HT_CAP_MSI", i64::from(PCI_HT_CAP_MSI)),
        ("PCI_HT_MSI_ENABLED", i64::from(PCI_HT_MSI_ENABLED)),
        ("PCI_HT_MSI_FIXED", i64::from(PCI_HT_MSI_FIXED)),
        ("PCI_HT_MSI_FIXED_ADDR", PCI_HT_MSI_FIXED_ADDR as i64),
        ("PCI_HT_MSI_ADDR", i64::from(PCI_HT_MSI_ADDR)),
        ("PCI_HT_MSI_ADDR_HI32", i64::from(PCI_HT_MSI_ADDR_HI32)),
        ("PCI_HT_INTR_DATA", i64::from(PCI_HT_INTR_DATA)),
        ("PCI_PCIE_XCAP", i64::from(PCI_PCIE_XCAP)),
        ("PCI_PCIE_XCAP_SI", i64::from(PCI_PCIE_XCAP_SI)),
        ("PCI_PCIE_XCAP_TYPE_RP", i64::from(PCI_PCIE_XCAP_TYPE_RP)),
        (
            "PCI_PCIE_XCAP_TYPE_DOWN",
            i64::from(PCI_PCIE_XCAP_TYPE_DOWN),
        ),
        (
            "PCI_PCIE_XCAP_TYPE_PCI2PCIE",
            i64::from(PCI_PCIE_XCAP_TYPE_PCI2PCIE),
        ),
        ("PCI_PCIE_DCAP", i64::from(PCI_PCIE_DCAP)),
        ("PCI_PCIE_DCSR", i64::from(PCI_PCIE_DCSR)),
        ("PCI_PCIE_DCSR_ERO", i64::from(PCI_PCIE_DCSR_ERO)),
        ("PCI_PCIE_DCSR_ENS", i64::from(PCI_PCIE_DCSR_ENS)),
        ("PCI_PCIE_DCSR_MPS", i64::from(PCI_PCIE_DCSR_MPS)),
        ("PCI_PCIE_DCSR_CEE", i64::from(PCI_PCIE_DCSR_CEE)),
        ("PCI_PCIE_DCSR_NFE", i64::from(PCI_PCIE_DCSR_NFE)),
        ("PCI_PCIE_DCSR_FEE", i64::from(PCI_PCIE_DCSR_FEE)),
        ("PCI_PCIE_DCSR_URE", i64::from(PCI_PCIE_DCSR_URE)),
        ("PCI_PCIE_LCAP", i64::from(PCI_PCIE_LCAP)),
        ("PCI_PCIE_LCAP_ASPM_L0S", i64::from(PCI_PCIE_LCAP_ASPM_L0S)),
        ("PCI_PCIE_LCAP_ASPM_L1", i64::from(PCI_PCIE_LCAP_ASPM_L1)),
        ("PCI_PCIE_LCSR", i64::from(PCI_PCIE_LCSR)),
        ("PCI_PCIE_LCSR_ASPM_L0S", i64::from(PCI_PCIE_LCSR_ASPM_L0S)),
        ("PCI_PCIE_LCSR_ASPM_L1", i64::from(PCI_PCIE_LCSR_ASPM_L1)),
        ("PCI_PCIE_LCSR_RL", i64::from(PCI_PCIE_LCSR_RL)),
        ("PCI_PCIE_LCSR_CCC", i64::from(PCI_PCIE_LCSR_CCC)),
        ("PCI_PCIE_LCSR_ES", i64::from(PCI_PCIE_LCSR_ES)),
        ("PCI_PCIE_LCSR_ECPM", i64::from(PCI_PCIE_LCSR_ECPM)),
        ("PCI_PCIE_LCSR_CLS", i64::from(PCI_PCIE_LCSR_CLS)),
        ("PCI_PCIE_LCSR_CLS_2_5", i64::from(PCI_PCIE_LCSR_CLS_2_5)),
        ("PCI_PCIE_LCSR_CLS_5", i64::from(PCI_PCIE_LCSR_CLS_5)),
        ("PCI_PCIE_LCSR_CLS_8", i64::from(PCI_PCIE_LCSR_CLS_8)),
        ("PCI_PCIE_LCSR_CLS_16", i64::from(PCI_PCIE_LCSR_CLS_16)),
        ("PCI_PCIE_LCSR_CLS_32", i64::from(PCI_PCIE_LCSR_CLS_32)),
        ("PCI_PCIE_LCSR_LT", i64::from(PCI_PCIE_LCSR_LT)),
        ("PCI_PCIE_LCSR_SCC", i64::from(PCI_PCIE_LCSR_SCC)),
        ("PCI_PCIE_SLCAP", i64::from(PCI_PCIE_SLCAP)),
        ("PCI_PCIE_SLCAP_ABP", i64::from(PCI_PCIE_SLCAP_ABP)),
        ("PCI_PCIE_SLCAP_PCP", i64::from(PCI_PCIE_SLCAP_PCP)),
        ("PCI_PCIE_SLCAP_MSP", i64::from(PCI_PCIE_SLCAP_MSP)),
        ("PCI_PCIE_SLCAP_AIP", i64::from(PCI_PCIE_SLCAP_AIP)),
        ("PCI_PCIE_SLCAP_PIP", i64::from(PCI_PCIE_SLCAP_PIP)),
        ("PCI_PCIE_SLCAP_HPS", i64::from(PCI_PCIE_SLCAP_HPS)),
        ("PCI_PCIE_SLCAP_HPC", i64::from(PCI_PCIE_SLCAP_HPC)),
        ("PCI_PCIE_SLCSR", i64::from(PCI_PCIE_SLCSR)),
        ("PCI_PCIE_SLCSR_ABE", i64::from(PCI_PCIE_SLCSR_ABE)),
        ("PCI_PCIE_SLCSR_PFE", i64::from(PCI_PCIE_SLCSR_PFE)),
        ("PCI_PCIE_SLCSR_MSE", i64::from(PCI_PCIE_SLCSR_MSE)),
        ("PCI_PCIE_SLCSR_PDE", i64::from(PCI_PCIE_SLCSR_PDE)),
        ("PCI_PCIE_SLCSR_CCE", i64::from(PCI_PCIE_SLCSR_CCE)),
        ("PCI_PCIE_SLCSR_HPE", i64::from(PCI_PCIE_SLCSR_HPE)),
        ("PCI_PCIE_SLCSR_ABP", i64::from(PCI_PCIE_SLCSR_ABP)),
        ("PCI_PCIE_SLCSR_PFD", i64::from(PCI_PCIE_SLCSR_PFD)),
        ("PCI_PCIE_SLCSR_MSC", i64::from(PCI_PCIE_SLCSR_MSC)),
        ("PCI_PCIE_SLCSR_PDC", i64::from(PCI_PCIE_SLCSR_PDC)),
        ("PCI_PCIE_SLCSR_CC", i64::from(PCI_PCIE_SLCSR_CC)),
        ("PCI_PCIE_SLCSR_MS", i64::from(PCI_PCIE_SLCSR_MS)),
        ("PCI_PCIE_SLCSR_PDS", i64::from(PCI_PCIE_SLCSR_PDS)),
        ("PCI_PCIE_SLCSR_LACS", i64::from(PCI_PCIE_SLCSR_LACS)),
        ("PCI_PCIE_RCSR", i64::from(PCI_PCIE_RCSR)),
        ("PCI_PCIE_DCSR2", i64::from(PCI_PCIE_DCSR2)),
        ("PCI_PCIE_DCSR2_LTREN", i64::from(PCI_PCIE_DCSR2_LTREN)),
        ("PCI_PCIE_LCAP2", i64::from(PCI_PCIE_LCAP2)),
        ("PCI_PCIE_LCSR2", i64::from(PCI_PCIE_LCSR2)),
        ("PCI_PCIE_LCSR2_TLS", i64::from(PCI_PCIE_LCSR2_TLS)),
        ("PCI_PCIE_LCSR2_TLS_2_5", i64::from(PCI_PCIE_LCSR2_TLS_2_5)),
        ("PCI_PCIE_LCSR2_TLS_5", i64::from(PCI_PCIE_LCSR2_TLS_5)),
        ("PCI_PCIE_LCSR2_TLS_8", i64::from(PCI_PCIE_LCSR2_TLS_8)),
        ("PCI_PCIE_LCSR2_TLS_16", i64::from(PCI_PCIE_LCSR2_TLS_16)),
        ("PCI_PCIE_LCSR2_TLS_32", i64::from(PCI_PCIE_LCSR2_TLS_32)),
        ("PCI_PCIE_ECAP", i64::from(PCI_PCIE_ECAP)),
        ("PCI_PCIE_ECAP_LAST", i64::from(PCI_PCIE_ECAP_LAST)),
        ("PCI_MSIX_MC_MSIXE", i64::from(PCI_MSIX_MC_MSIXE)),
        ("PCI_MSIX_MC_FM", i64::from(PCI_MSIX_MC_FM)),
        ("PCI_MSIX_MC_TBLSZ_MASK", i64::from(PCI_MSIX_MC_TBLSZ_MASK)),
        (
            "PCI_MSIX_MC_TBLSZ_SHIFT",
            i64::from(PCI_MSIX_MC_TBLSZ_SHIFT),
        ),
        ("PCI_MSIX_TABLE", i64::from(PCI_MSIX_TABLE)),
        ("PCI_MSIX_TABLE_BIR", i64::from(PCI_MSIX_TABLE_BIR)),
        ("PCI_MSIX_VC_MASK", i64::from(PCI_MSIX_VC_MASK)),
        ("PCI_INTERRUPT_REG", i64::from(PCI_INTERRUPT_REG)),
        (
            "PCI_INTERRUPT_PIN_SHIFT",
            i64::from(PCI_INTERRUPT_PIN_SHIFT),
        ),
        ("PCI_INTERRUPT_PIN_MASK", i64::from(PCI_INTERRUPT_PIN_MASK)),
        (
            "PCI_INTERRUPT_LINE_SHIFT",
            i64::from(PCI_INTERRUPT_LINE_SHIFT),
        ),
        (
            "PCI_INTERRUPT_LINE_MASK",
            i64::from(PCI_INTERRUPT_LINE_MASK),
        ),
        ("PCI_MIN_GNT_SHIFT", i64::from(PCI_MIN_GNT_SHIFT)),
        ("PCI_MIN_GNT_MASK", i64::from(PCI_MIN_GNT_MASK)),
        ("PCI_MAX_LAT_SHIFT", i64::from(PCI_MAX_LAT_SHIFT)),
        ("PCI_MAX_LAT_MASK", i64::from(PCI_MAX_LAT_MASK)),
        ("PCI_INTERRUPT_PIN_NONE", i64::from(PCI_INTERRUPT_PIN_NONE)),
        ("PCI_INTERRUPT_PIN_A", i64::from(PCI_INTERRUPT_PIN_A)),
        ("PCI_INTERRUPT_PIN_B", i64::from(PCI_INTERRUPT_PIN_B)),
        ("PCI_INTERRUPT_PIN_C", i64::from(PCI_INTERRUPT_PIN_C)),
        ("PCI_INTERRUPT_PIN_D", i64::from(PCI_INTERRUPT_PIN_D)),
        ("PCI_INTERRUPT_PIN_MAX", i64::from(PCI_INTERRUPT_PIN_MAX)),
        (
            "PCI_VPDRES_TYPE_COMPATIBLE_DEVICE_ID",
            i64::from(PCI_VPDRES_TYPE_COMPATIBLE_DEVICE_ID),
        ),
        (
            "PCI_VPDRES_TYPE_VENDOR_DEFINED",
            i64::from(PCI_VPDRES_TYPE_VENDOR_DEFINED),
        ),
        (
            "PCI_VPDRES_TYPE_END_TAG",
            i64::from(PCI_VPDRES_TYPE_END_TAG),
        ),
        (
            "PCI_VPDRES_TYPE_IDENTIFIER_STRING",
            i64::from(PCI_VPDRES_TYPE_IDENTIFIER_STRING),
        ),
        ("PCI_VPDRES_TYPE_VPD", i64::from(PCI_VPDRES_TYPE_VPD)),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}
