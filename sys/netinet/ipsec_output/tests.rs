//! Host tests for `ipsec_hdrsz`: the bytes an SA adds to a packet, with the IPv6 tunnel header
//! of an SA whose destination is IPv6.

use super::*;
use crate::crypto::xform::auth_hash_hmac_sha1_96;
use crate::netinet::in_::SockaddrIn;
use crate::netinet::ip_ipsp::SockaddrUnion;
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::sys::socket::AF_INET6;

fn su6() -> SockaddrUnion {
    SockaddrUnion::from_sin6(&SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: AF_INET6,
        sin6_addr: In6Addr::new([0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]),
        ..SockaddrIn6::default()
    })
}

#[test]
fn an_ipv6_tunnel_adds_an_ipv6_header() {
    let t = Tdb::new();
    t.tdb_dst.set(su6());
    t.tdb_sproto.set(IPPROTO_AH as u8);
    t.tdb_authalgxform.set(Some(&auth_hash_hmac_sha1_96));

    // AH: its header, a word, the authenticator.
    let ah = (AH_FLENGTH + 4 + usize::from(auth_hash_hmac_sha1_96.authsize)) as isize;
    assert_eq!(ipsec_hdrsz(&t), ah);

    // A tunnel adds the outer header of the SA's family.
    t.set_flags(TDBF_TUNNELING);
    assert_eq!(ipsec_hdrsz(&t), ah + 40);
    t.tdb_dst.set(SockaddrUnion::from_sin(&SockaddrIn {
        sin_len: 16,
        sin_family: AF_INET,
        ..SockaddrIn::default()
    }));
    assert_eq!(ipsec_hdrsz(&t), ah + 20);

    // IP-in-IP alone adds only the tunnel header, and an unknown protocol has no size.
    t.tdb_dst.set(su6());
    t.tdb_sproto.set(IPPROTO_IPIP as u8);
    assert_eq!(ipsec_hdrsz(&t), 40);
    t.tdb_sproto.set(99);
    assert_eq!(ipsec_hdrsz(&t), -1);
}
