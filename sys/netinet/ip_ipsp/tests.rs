//! Host tests for the SA database: TDB lookups by SPI and destination, by destination and by
//! source, deletion, the tables growing past their first size, SPI reservation, and the
//! shared identities.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::kern_malloc::malloc;
use crate::netinet::in_::{IPPROTO_AH, IPPROTO_ESP};
use crate::netinet::ip_input::tests::sin;
use crate::sys::endian::htons;
use crate::sys::malloc::M_WAITOK;

/// Memory, timeouts and the network (as the IPv4 tests set them up), and an empty SA database.
fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let g = crate::netinet::ip_input::tests::setup();
    ipsp_reset();
    g
}

/// A `sockaddr_in` union for `a`.
fn su(a: [u8; 4]) -> SockaddrUnion {
    SockaddrUnion::from_sin(&sin(a))
}

/// A valid TDB in the tables: `spi` (host order) from `src` to `dst` with `sproto`.
fn tdb(spi: u32, src: [u8; 4], dst: [u8; 4], sproto: i32) -> &'static Tdb {
    let t = tdb_alloc(0);
    t.tdb_spi.set(htonl(spi));
    t.tdb_src.set(su(src));
    t.tdb_dst.set(su(dst));
    t.tdb_sproto.set(sproto as u8);
    puttdb(t);
    t
}

const A: [u8; 4] = [10, 0, 2, 15];
const B: [u8; 4] = [10, 0, 2, 2];
const C: [u8; 4] = [192, 168, 77, 2];

#[test]
fn tdbs_are_found_by_spi_destination_and_protocol() {
    let _g = setup();
    let esp = tdb(0x1000, A, B, IPPROTO_ESP);
    let ah = tdb(0x1000, A, B, IPPROTO_AH);
    let other = tdb(0x2000, A, C, IPPROTO_ESP);

    let found = gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).expect("the ESP SA");
    assert!(ptr::eq(found, esp));
    assert_eq!(
        found.tdb_refcnt.r_refs.load(Ordering::Relaxed),
        2,
        "the lookup's reference"
    );
    tdb_unref(Some(found));
    let found = gettdb(0, htonl(0x1000), &su(B), IPPROTO_AH as u8).expect("the AH SA");
    assert!(ptr::eq(found, ah));
    tdb_unref(Some(found));

    assert!(
        gettdb(0, htonl(0x1001), &su(B), IPPROTO_ESP as u8).is_none(),
        "other SPI"
    );
    assert!(
        gettdb(0, htonl(0x1000), &su(C), IPPROTO_ESP as u8).is_none(),
        "other dst"
    );
    assert!(
        gettdb(1, htonl(0x1000), &su(B), IPPROTO_ESP as u8).is_none(),
        "other rdomain"
    );

    // By destination and by source, for the SPD.
    let by_dst = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, None, None).expect("by dst");
    assert!(ptr::eq(by_dst, other));
    tdb_unref(Some(by_dst));
    let by_src = gettdbbysrc(0, &su(A), IPPROTO_AH as u8, None, None, None).expect("by src");
    assert!(ptr::eq(by_src, ah));
    tdb_unref(Some(by_src));
    let both = gettdbbysrcdst(0, 0, &su(A), &su(C), IPPROTO_ESP as u8).expect("by src and dst");
    assert!(ptr::eq(both, other));
    tdb_unref(Some(both));

    // An invalid (larval) SA is found by SPI only.
    esp.set_flags(TDBF_INVALID);
    assert!(gettdbbydst(0, &su(B), IPPROTO_ESP as u8, None, None, None).is_none());
    let larval = gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).expect("by SPI");
    tdb_unref(Some(larval));
    esp.clr_flags(TDBF_INVALID);

    // A filter must match exactly when the SA has one.
    let mut f = SockaddrEncap::new();
    f.set_sen_type(SENT_IP4);
    f.set_sen_ip_dst(sin(C).sin_addr);
    other.tdb_filter.set(f);
    let mut g = f;
    g.set_sen_proto(1);
    assert!(gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&g), Some(&g)).is_none());
    let zero = SockaddrEncap::new();
    let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&zero))
        .expect("the filter and the (zero) masks match");
    tdb_unref(Some(t));
    let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&f));
    assert!(t.is_none(), "the SA's mask is zero, the policy's is not");
    other.tdb_filtermask.set(f);
    let t = gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, Some(&f), Some(&f)).expect("match");
    tdb_unref(Some(t));

    // Deleted SAs are gone from every table (and freed with their last reference).
    for t in [esp, ah, other] {
        tdb_delete(t);
    }
    assert!(gettdb(0, htonl(0x1000), &su(B), IPPROTO_ESP as u8).is_none());
    assert!(gettdbbysrc(0, &su(A), IPPROTO_AH as u8, None, None, None).is_none());
    assert!(gettdbbydst(0, &su(C), IPPROTO_ESP as u8, None, None, None).is_none());
}

#[test]
fn the_tables_grow_and_keep_every_tdb() {
    let _g = setup();
    let all: Vec<&'static Tdb> = (0..200u32)
        .map(|i| tdb(0x100 + i, A, [10, 1, (i >> 8) as u8, i as u8], IPPROTO_ESP))
        .collect();

    mtx_enter(&TDB_SADB_MTX);
    let (mask, count) = (tables().tdb_hashmask, tables().tdb_count);
    mtx_leave(&TDB_SADB_MTX);
    assert!(mask > TDB_HASHSIZE_INIT - 1, "the tables were rehashed");
    assert_eq!(count, 200);

    for (i, t) in all.iter().enumerate() {
        let i = i as u32;
        let dst = su([10, 1, (i >> 8) as u8, i as u8]);
        let f = gettdb(0, htonl(0x100 + i), &dst, IPPROTO_ESP as u8).expect("found");
        assert!(ptr::eq(f, *t));
        tdb_unref(Some(f));
        let f = gettdbbydst(0, &dst, IPPROTO_ESP as u8, None, None, None).expect("by dst");
        assert!(ptr::eq(f, *t));
        tdb_unref(Some(f));
    }

    let mut walked = 0;
    net_lock();
    tdb_walk(0, |_, _| {
        walked += 1;
        Ok(())
    })
    .expect("walk");
    net_unlock();
    assert_eq!(walked, 200);

    for t in all {
        tdb_delete(t);
    }
}

#[test]
fn reserve_spi_takes_free_spis_out_of_the_range() {
    let _g = setup();

    assert_eq!(
        reserve_spi(0, 1, 255, &su(A), &su(B), IPPROTO_ESP as u8),
        Err(Errno::EINVAL),
        "only reserved SPIs"
    );
    assert_eq!(
        reserve_spi(0, 5000, 4000, &su(A), &su(B), IPPROTO_ESP as u8),
        Err(Errno::EINVAL),
        "an empty range"
    );

    let spi = reserve_spi(0, 0x4242, 0x4242, &su(A), &su(B), IPPROTO_ESP as u8).expect("free");
    assert_eq!(spi, htonl(0x4242));
    let t = gettdb(0, spi, &su(B), IPPROTO_ESP as u8).expect("reserved");
    assert!(t.has_flags(TDBF_INVALID), "larval until SADB_UPDATE");
    assert_eq!(t.tdb_satype.get(), SADB_SATYPE_UNSPEC);
    assert!(t.has_flags(TDBF_TIMER), "with the embryonic timeout");
    assert_eq!(
        reserve_spi(0, 0x4242, 0x4242, &su(A), &su(B), IPPROTO_ESP as u8),
        Err(Errno::EEXIST)
    );

    let other = reserve_spi(0, 256, 0xffff_ffff, &su(A), &su(B), IPPROTO_ESP as u8)
        .expect("a random free SPI");
    assert!(ntohl(other) > SPI_RESERVED_MAX && other != spi);
    let o = gettdb(0, other, &su(B), IPPROTO_ESP as u8).expect("reserved");

    tdb_delete(t);
    tdb_unref(Some(t));
    tdb_delete(o);
    tdb_unref(Some(o));
}

/// A pair of identities as `import_identities` makes them.
fn ids(local: &[u8], remote: &[u8]) -> &'static IpsecIds {
    let id = |data: &[u8]| {
        let p = malloc(size_of::<IpsecId>() + data.len(), M_CREDENTIALS, M_WAITOK)
            .expect("malloc")
            .cast::<IpsecId>();
        // SAFETY: a fresh allocation of the header and the data.
        unsafe {
            p.as_ptr().write(IpsecId {
                type_: IPSP_IDENTITY_FQDN,
                len: data.len() as i16,
            });
            ptr::copy_nonoverlapping(data.as_ptr(), p.as_ptr().add(1).cast::<u8>(), data.len());
        }
        p
    };
    let p = malloc(size_of::<IpsecIds>(), M_CREDENTIALS, M_WAITOK)
        .expect("malloc")
        .cast::<IpsecIds>();
    // SAFETY: a fresh allocation of an `IpsecIds`.
    unsafe {
        p.as_ptr().write(IpsecIds::new(id(local), id(remote)));
        &*p.as_ptr()
    }
}

#[test]
fn identities_are_shared_by_value_and_found_by_flow() {
    let _g = setup();

    let a = ipsp_ids_insert(ids(b"left\0", b"right\0")).expect("inserted");
    assert_eq!(a.id_refcount.get(), 1);
    let flow = a.id_flow.get();
    assert_ne!(flow, 0);

    // The same identities give the same pair, with one more reference.
    let b = ipsp_ids_insert(ids(b"left\0", b"right\0")).expect("found");
    assert!(ptr::eq(a, b));
    assert_eq!(a.id_refcount.get(), 2);

    // Other identities get another flow number.
    let c = ipsp_ids_insert(ids(b"left\0", b"elsewhere\0")).expect("inserted");
    assert!(!ptr::eq(a, c));
    assert_ne!(c.id_flow.get(), flow);

    let l = ipsp_ids_lookup(flow).expect("by flow");
    assert!(ptr::eq(l, a));
    assert_eq!(a.id_refcount.get(), 3);
    assert!(ipsp_ids_lookup(0x7777).is_none());

    // Unreferenced pairs wait for the garbage collector and are not looked up meanwhile.
    ipsp_ids_free(Some(c));
    assert!(ipsp_ids_lookup(c.id_flow.get()).is_none());
    // Taking it again cancels its collection.
    let c2 = ipsp_ids_insert(ids(b"left\0", b"elsewhere\0")).expect("found");
    assert!(ptr::eq(c, c2));
    assert_eq!(c.id_refcount.get(), 1);

    for p in [a, a, a, c] {
        ipsp_ids_free(Some(p));
    }
    assert_eq!(a.id_refcount.get(), 0);
}

#[test]
fn sockaddr_encap_accessors_are_at_the_c_offsets() {
    let mut e = SockaddrEncap::new();
    e.set_sen_len(SENT_LEN as u8);
    e.set_sen_family(crate::sys::socket::PF_KEY);
    e.set_sen_type(SENT_IP4);
    e.set_sen_direction(IPSP_DIRECTION_OUT);
    e.set_sen_ip_src(sin([10, 77, 1, 0]).sin_addr);
    e.set_sen_ip_dst(sin([10, 77, 2, 0]).sin_addr);
    e.set_sen_proto(17);
    e.set_sen_sport(htons(500));
    e.set_sen_dport(htons(4500));
    let b = e.as_bytes();
    assert_eq!(b[0], 48);
    assert_eq!(b[1], crate::sys::socket::PF_KEY);
    assert_eq!(u16::from_ne_bytes([b[2], b[3]]), SENT_IP4);
    assert_eq!(b[4], IPSP_DIRECTION_OUT);
    assert_eq!(&b[8..12], &[10, 77, 1, 0]);
    assert_eq!(&b[12..16], &[10, 77, 2, 0]);
    assert_eq!(b[16], 17);
    assert_eq!(&b[18..20], &500u16.to_be_bytes());
    assert_eq!(&b[20..22], &4500u16.to_be_bytes());
    assert!(b[22..].iter().all(|&x| x == 0));
}

/// An IPv6 address from its eight 16-bit words.
#[cfg(feature = "inet6")]
fn a6(w: [u16; 8]) -> In6Addr {
    let mut a = [0u8; 16];
    for (i, w) in w.iter().enumerate() {
        a[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
    }
    In6Addr::new(a)
}

#[cfg(feature = "inet6")]
#[test]
fn sockaddr_encap_ipv6_accessors_are_at_the_c_offsets() {
    let mut e = SockaddrEncap::new();
    e.set_sen_len(SENT_LEN as u8);
    e.set_sen_family(crate::sys::socket::PF_KEY);
    e.set_sen_type(SENT_IP6);
    e.set_sen_ip6_direction(IPSP_DIRECTION_OUT);
    e.set_sen_ip6_src(a6([0xfd77, 1, 0, 0, 0, 0, 0, 1]));
    e.set_sen_ip6_dst(a6([0xfd77, 2, 0, 0, 0, 0, 0, 2]));
    e.set_sen_ip6_proto(17);
    e.set_sen_ip6_sport(htons(500));
    e.set_sen_ip6_dport(htons(4500));
    let b = e.as_bytes();
    assert_eq!(b[0], 48);
    assert_eq!(u16::from_ne_bytes([b[2], b[3]]), SENT_IP6);
    assert_eq!(b[4], IPSP_DIRECTION_OUT);
    assert_eq!(&b[8..12], &[0xfd, 0x77, 0, 1]);
    assert_eq!(b[23], 1);
    assert_eq!(&b[24..28], &[0xfd, 0x77, 0, 2]);
    assert_eq!(b[39], 2);
    assert_eq!(b[40], 17);
    assert_eq!(&b[42..44], &500u16.to_be_bytes());
    assert_eq!(&b[44..46], &4500u16.to_be_bytes());
    assert_eq!(e.sen_ip6_src(), a6([0xfd77, 1, 0, 0, 0, 0, 0, 1]));
    assert_eq!(e.sen_ip6_dst(), a6([0xfd77, 2, 0, 0, 0, 0, 0, 2]));
    assert_eq!(e.sen_ip6_proto(), 17);
    assert_eq!(e.sen_ip6_dport(), htons(4500));
}

#[cfg(feature = "inet6")]
#[test]
fn sockaddr_union_holds_a_sockaddr_in6() {
    let sin6 = SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: crate::sys::socket::AF_INET6,
        sin6_port: htons(500),
        sin6_addr: a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        sin6_scope_id: 3,
        ..SockaddrIn6::default()
    };
    let s6 = SockaddrUnion::from_sin6(&sin6);
    assert_eq!(s6.sa_family(), crate::sys::socket::AF_INET6);
    assert_eq!(usize::from(s6.sa_len()), 28);
    assert_eq!(s6.sa_bytes().len(), 28, "memcmp over sa_len bytes");
    assert_eq!(s6.sin6(), sin6);
    assert_eq!(s6.sin6_addr(), sin6.sin6_addr);
    // The same address is equal as bytes, another one is not.
    assert!(s6 == SockaddrUnion::from_sin6(&sin6));
    let mut other = sin6;
    other.sin6_addr = a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]);
    assert!(s6 != SockaddrUnion::from_sin6(&other));

    // The unspecified address, and the one of no family, count as unspecified; others not.
    assert!(!ipsp_is_unspecified(s6));
    other.sin6_addr = In6Addr::default();
    assert!(ipsp_is_unspecified(SockaddrUnion::from_sin6(&other)));
    assert!(ipsp_is_unspecified(SockaddrUnion::new()));
    assert!(!ipsp_is_unspecified(su(A)));
    assert!(ipsp_is_unspecified(su([0; 4])));

    // The TDB lookups work over an IPv6 destination: the SA is found by it and not by the
    // IPv4 one with the same SPI.
    let _g = setup();
    let su_v6 = s6;
    let t = tdb_alloc(0);
    t.tdb_spi.set(htonl(0x2000));
    t.tdb_src.set(SockaddrUnion::from_sin6(&other));
    t.tdb_dst.set(su_v6);
    t.tdb_sproto.set(IPPROTO_ESP as u8);
    puttdb(t);
    let found = gettdb(0, htonl(0x2000), &su_v6, IPPROTO_ESP as u8).expect("found by IPv6");
    assert!(core::ptr::eq(found, t));
    tdb_unref(Some(found));
    assert!(gettdb(0, htonl(0x2000), &su(A), IPPROTO_ESP as u8).is_none());
}
