use super::*;

fn ed(addr: u8, attr: u8, mps: u16, ival: u8) -> UsbEndpointDescriptor {
    let mut e = UsbEndpointDescriptor::zeroed();
    e.bLength = USB_ENDPOINT_DESCRIPTOR_SIZE as u8;
    e.bDescriptorType = UDESC_ENDPOINT;
    e.bEndpointAddress = addr;
    e.bmAttributes = attr;
    usetw(&mut e.wMaxPacketSize, mps);
    e.bInterval = ival;
    e
}

#[test]
fn device_context_index() {
    // Section 4.5.1: control endpoints 2n+1, others 2n + (IN ? 1 : 0).
    assert_eq!(xhci_ed2dci(&ed(0x00, UE_CONTROL, 64, 0)), 1);
    assert_eq!(xhci_ed2dci(&ed(0x81, UE_BULK, 512, 0)), 3);
    assert_eq!(xhci_ed2dci(&ed(0x02, UE_BULK, 512, 0)), 4);
    assert_eq!(xhci_ed2dci(&ed(0x81, UE_INTERRUPT, 8, 10)), 3);
    assert_eq!(xhci_ed2dci(&ed(0x8f, UE_INTERRUPT, 8, 10)), 31);
}

#[test]
fn intervals() {
    // Linear (low/full speed interrupt): fls(ival) - 1.
    assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 0)), 0);
    assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 1)), 0);
    assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 10)), 3);
    assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 255)), 7);
    // Exponential: bInterval - 1, clamped to 1..16.
    assert_eq!(xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 0)), 0);
    assert_eq!(xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 4)), 3);
    assert_eq!(
        xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 200)),
        15
    );
}

#[test]
fn root_hub_descriptors() {
    assert_eq!(XHCI_DEVD.as_bytes().len(), USB_DEVICE_DESCRIPTOR_SIZE);
    assert_eq!(XHCI_DEVD.bcdUSB, [0x00, 0x03]);
    assert_eq!(
        ugetw(XHCI_CONFD.wTotalLength) as usize,
        USB_CONFIG_DESCRIPTOR_SIZE + USB_INTERFACE_DESCRIPTOR_SIZE + USB_ENDPOINT_DESCRIPTOR_SIZE
    );
    assert_eq!(XHCI_ENDPD.bEndpointAddress, 0x81);
    assert_eq!(XHCI_HUBD.bDescriptorType, UDESC_SS_HUB);
}

#[test]
fn command_trb_layout() {
    let t = cmd_trb(0x1234_5000, xhci_trb_set_slot(3) | XHCI_CMD_CONFIG_EP);
    assert_eq!(u64::from_le(t.trb_paddr), 0x1234_5000);
    assert_eq!(t.trb_status, 0);
    assert_eq!(xhci_trb_get_slot(u32::from_le(t.trb_flags)), 3);
    assert_eq!(
        u32::from_le(t.trb_flags) & XHCI_TRB_TYPE_MASK,
        XHCI_CMD_CONFIG_EP
    );
}
