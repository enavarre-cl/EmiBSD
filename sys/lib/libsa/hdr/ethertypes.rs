/* <CODE> */
//! `<net/ethertypes.h>` for libsa (through `<netinet/if_ether.h>`): the two Ethernet types
//! the network code sends and receives.

/// `ETHERTYPE_IP`: IP protocol.
pub const ETHERTYPE_IP: u16 = 0x0800;
/// `ETHERTYPE_ARP`: address resolution protocol.
pub const ETHERTYPE_ARP: u16 = 0x0806;
/* </CODE> */
