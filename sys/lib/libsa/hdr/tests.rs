//! The layouts libsa declares again agree with the kernel's ports of the same headers.

use core::mem::{offset_of, size_of};

use super::dinode::{Ufs1Dinode, Ufs2Dinode};
use super::disklabel::{Disklabel, Partition};
use super::fs::Fs;

#[test]
fn layouts_match_the_kernel() {
    use bsd::sys::disklabel as k_dl;
    use bsd::ufs::ffs::fs as k_fs;
    use bsd::ufs::ufs::dinode as k_di;

    assert_eq!(size_of::<Fs>(), size_of::<k_fs::Fs>());
    assert_eq!(offset_of!(Fs, fs_magic), offset_of!(k_fs::Fs, fs_magic));
    assert_eq!(offset_of!(Fs, fs_bsize), offset_of!(k_fs::Fs, fs_bsize));
    assert_eq!(offset_of!(Fs, fs_inopb), offset_of!(k_fs::Fs, fs_inopb));
    assert_eq!(offset_of!(Fs, fs_qbmask), offset_of!(k_fs::Fs, fs_qbmask));
    assert_eq!(
        offset_of!(Fs, fs_maxsymlinklen),
        offset_of!(k_fs::Fs, fs_maxsymlinklen)
    );
    assert_eq!(size_of::<Ufs1Dinode>(), size_of::<k_di::Ufs1Dinode>());
    assert_eq!(size_of::<Ufs2Dinode>(), size_of::<k_di::Ufs2Dinode>());
    assert_eq!(
        offset_of!(Ufs1Dinode, di_db),
        offset_of!(k_di::Ufs1Dinode, di_db)
    );
    assert_eq!(
        offset_of!(Ufs2Dinode, di_db),
        offset_of!(k_di::Ufs2Dinode, di_db)
    );
    assert_eq!(size_of::<Disklabel>(), size_of::<k_dl::Disklabel>());
    assert_eq!(size_of::<Partition>(), size_of::<k_dl::Partition>());
    assert_eq!(
        offset_of!(Disklabel, d_uid),
        offset_of!(k_dl::Disklabel, d_uid)
    );
    assert_eq!(
        offset_of!(Disklabel, d_partitions),
        offset_of!(k_dl::Disklabel, d_partitions)
    );
    assert_eq!(super::disklabel::DISKMAGIC, k_dl::DISKMAGIC);
    assert_eq!(super::fs::FS_UFS2_MAGIC, k_fs::FS_UFS2_MAGIC);
}

#[test]
fn network_layouts_match_the_kernel() {
    use super::endian::{htonl, htons, ntohl, ntohs};
    use super::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
    use super::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, ARPOP_REQUEST, Arphdr};
    use super::if_ether::{
        ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_CRC_LEN, ETHER_HDR_LEN, EtherArp, EtherHeader,
    };
    use super::in_::{INADDR_ANY, INADDR_BROADCAST, IP_TTL, IPPROTO_UDP, InAddr};
    use super::ip::{IPVERSION, Ip};
    use super::ip_var::Ipovly;
    use super::udp::Udphdr;
    use super::udp_var::Udpiphdr;
    use bsd::net::{ethertypes as k_et, if_arp as k_arp};
    use bsd::netinet::{if_ether as k_eth, in_ as k_in, ip as k_ip, ip_var as k_ipv};
    use bsd::netinet::{udp as k_udp, udp_var as k_udpv};
    use bsd::sys::endian as k_end;

    assert_eq!(size_of::<InAddr>(), size_of::<k_in::InAddr>());
    assert_eq!(size_of::<EtherHeader>(), size_of::<k_eth::EtherHeader>());
    assert_eq!(
        offset_of!(EtherHeader, ether_type),
        offset_of!(k_eth::EtherHeader, ether_type)
    );
    assert_eq!(size_of::<Arphdr>(), size_of::<k_arp::Arphdr>());
    assert_eq!(offset_of!(Arphdr, ar_op), offset_of!(k_arp::Arphdr, ar_op));
    assert_eq!(size_of::<EtherArp>(), size_of::<k_eth::EtherArp>());
    assert_eq!(
        offset_of!(EtherArp, arp_tpa),
        offset_of!(k_eth::EtherArp, arp_tpa)
    );
    assert_eq!(size_of::<Ip>(), size_of::<k_ip::Ip>());
    assert_eq!(offset_of!(Ip, ip_sum), offset_of!(k_ip::Ip, ip_sum));
    assert_eq!(offset_of!(Ip, ip_dst), offset_of!(k_ip::Ip, ip_dst));
    assert_eq!(size_of::<Ipovly>(), size_of::<k_ipv::Ipovly>());
    assert_eq!(
        offset_of!(Ipovly, ih_len),
        offset_of!(k_ipv::Ipovly, ih_len)
    );
    assert_eq!(size_of::<Udphdr>(), size_of::<k_udp::Udphdr>());
    assert_eq!(
        offset_of!(Udphdr, uh_sum),
        offset_of!(k_udp::Udphdr, uh_sum)
    );
    assert_eq!(size_of::<Udpiphdr>(), size_of::<k_udpv::Udpiphdr>());
    assert_eq!(
        offset_of!(Udpiphdr, ui_u),
        offset_of!(k_udpv::Udpiphdr, ui_u)
    );

    assert_eq!(ETHER_ADDR_LEN, k_eth::ETHER_ADDR_LEN);
    assert_eq!(ETHER_HDR_LEN, k_eth::ETHER_HDR_LEN);
    assert_eq!(ETHER_CRC_LEN, k_eth::ETHER_CRC_LEN);
    assert_eq!(ETHER_ALIGN, k_eth::ETHER_ALIGN);
    assert_eq!(ETHERTYPE_IP, k_et::ETHERTYPE_IP);
    assert_eq!(ETHERTYPE_ARP, k_et::ETHERTYPE_ARP);
    assert_eq!(ARPHRD_ETHER, k_arp::ARPHRD_ETHER);
    assert_eq!(ARPOP_REQUEST, k_arp::ARPOP_REQUEST);
    assert_eq!(ARPOP_REPLY, k_arp::ARPOP_REPLY);
    assert_eq!(i32::from(IPPROTO_UDP), k_in::IPPROTO_UDP);
    assert_eq!(i32::from(IP_TTL), k_in::IP_TTL);
    assert_eq!(INADDR_ANY, k_in::INADDR_ANY);
    assert_eq!(INADDR_BROADCAST, k_in::INADDR_BROADCAST);
    assert_eq!(IPVERSION, k_ip::IPVERSION);
    assert_eq!(htons(0x1234), k_end::htons(0x1234));
    assert_eq!(ntohs(0x1234), k_end::ntohs(0x1234));
    assert_eq!(htonl(0x1234_5678), k_end::htonl(0x1234_5678));
    assert_eq!(ntohl(0x1234_5678), k_end::ntohl(0x1234_5678));
}
