//! Host tests for pf's interface layer: kif creation and lookup, reference counting,
//! `pfi_kif_match` against interfaces and groups, the `set skip` flags and the address
//! buffer.

use std::sync::MutexGuard;

use super::*;
use crate::net::if_::tests::{setup_net, test_ifnet};
use crate::net::if_::{IFG_HEAD, if_addgroup, if_attach};
use crate::netinet::in_::InAddr;

/// The network test lock (`pfi_ifs` and the interfaces are global), with pf initialised and
/// `pf_lock` held until the guard drops.
struct PfGuard {
    _net: MutexGuard<'static, ()>,
}

impl Drop for PfGuard {
    fn drop(&mut self) {
        pf_unlock();
    }
}

fn setup() -> PfGuard {
    let net = setup_net();
    pfi_initialize();
    pf_lock();
    PfGuard { _net: net }
}

/// The group named `name`, created by `if_addgroup`.
fn group(name: &[u8]) -> &'static IfgGroup {
    IFG_HEAD
        .0
        .iter()
        .find(|g| pf_cstr(&g.ifg_group) == name)
        .expect("group")
}

#[test]
fn kifs_are_created_once_and_found_by_name() {
    let _g = setup();

    let all = pfi_all().expect("pfi_all");
    assert_eq!(pf_cstr(&all.pfik_name), b"all");
    assert!(ptr::eq(pfi_kif_find(IFG_ALL).expect("all"), all));

    assert!(pfi_kif_find(b"tkif0").is_none());
    let k = pfi_kif_get(b"tkif0", None).expect("created");
    assert_eq!(pf_cstr(&k.pfik_name), b"tkif0");
    assert!(k.pfik_tzero.get() != 0 || gettime() == 0);
    assert!(ptr::eq(
        pfi_kif_get(b"tkif0\0junk", None).expect("found"),
        k
    ));
    assert!(ptr::eq(pfi_kif_find(b"tkif0").expect("found"), k));

    // `any` gets the flag in both sets.
    let any = pfi_kif_alloc(b"any", M_WAITOK).expect("alloc");
    assert_ne!(any.pfik_flags.get() & PFI_IFLAG_ANY, 0);
    assert_ne!(any.pfik_flags_new.get() & PFI_IFLAG_ANY, 0);
    pfi_kif_free(Some(any));

    // A preallocated buffer is used, and taken, when the name is new...
    let pre = pfi_kif_alloc(b"tkif1", M_WAITOK).expect("alloc");
    let mut prealloc = Some(pre);
    let k1 = pfi_kif_get(b"tkif1", Some(&mut prealloc)).expect("created");
    assert!(ptr::eq(k1, pre));
    assert!(prealloc.is_none());
    // ... and left to the caller when it is not.
    let pre = pfi_kif_alloc(b"tkif1", M_WAITOK).expect("alloc");
    let mut prealloc = Some(pre);
    assert!(ptr::eq(
        pfi_kif_get(b"tkif1", Some(&mut prealloc)).expect("found"),
        k1
    ));
    assert!(prealloc.is_some());
    pfi_kif_free(prealloc);

    // Unreferenced, unattached kifs go away on the next unref.
    pfi_kif_unref(Some(k), PFI_KIF_REF_NONE);
    pfi_kif_unref(Some(k1), PFI_KIF_REF_NONE);
    assert!(pfi_kif_find(b"tkif0").is_none());
    assert!(pfi_kif_find(b"tkif1").is_none());
}

#[test]
fn references_keep_a_kif_until_the_last_is_dropped() {
    let _g = setup();

    let k = pfi_kif_get(b"tref0", None).expect("created");
    pfi_kif_ref(k, PFI_KIF_REF_RULE);
    pfi_kif_ref(k, PFI_KIF_REF_RULE);
    pfi_kif_ref(k, PFI_KIF_REF_STATE);
    assert_eq!(k.pfik_rules.get(), 2);
    assert_eq!(k.pfik_states.get(), 1);

    // An unref of a kind it does not hold is refused (and logged) without underflow.
    pfi_kif_unref(Some(k), PFI_KIF_REF_ROUTE);
    assert_eq!(k.pfik_routes.get(), 0);

    pfi_kif_unref(Some(k), PFI_KIF_REF_RULE);
    pfi_kif_unref(Some(k), PFI_KIF_REF_STATE);
    assert!(ptr::eq(
        pfi_kif_find(b"tref0").expect("still referenced"),
        k
    ));
    pfi_kif_unref(Some(k), PFI_KIF_REF_RULE);
    assert!(
        pfi_kif_find(b"tref0").is_none(),
        "freed with the last reference"
    );

    // pfi_all is never freed.
    let all = pfi_all().expect("all");
    pfi_kif_ref(all, PFI_KIF_REF_STATE);
    pfi_kif_unref(Some(all), PFI_KIF_REF_STATE);
    assert!(pfi_kif_find(IFG_ALL).is_some());

    pfi_kif_unref(None, PFI_KIF_REF_RULE);
}

#[test]
fn rules_match_their_interface_its_groups_and_any() {
    let _g = setup();

    // if_attach and if_creategroup call pfi_attach_ifnet and pfi_attach_ifgroup (NPF > 0),
    // which take pf_lock themselves: the interfaces come and go without it, as in the kernel.
    pf_unlock();
    let ifp = test_ifnet(b"tmatch0");
    if_attach(ifp);
    let other = test_ifnet(b"tmatch1");
    if_attach(other);
    let lo = test_ifnet(b"tmatchlo0");
    lo.if_flags.set(IFF_LOOPBACK);
    if_attach(lo);
    assert_eq!(if_addgroup(ifp, b"tmgrp"), Ok(()));
    assert_eq!(if_addgroup(other, b"tmother"), Ok(()));
    pf_lock();

    let kif = kif_of(ifp.if_pf_kif.get()).expect("attached");
    let okif = kif_of(other.if_pf_kif.get()).expect("attached");
    let lokif = kif_of(lo.if_pf_kif.get()).expect("attached");
    assert!(ptr::eq(kif.pfik_ifp().expect("ifp"), ifp));
    assert!(!kif.pfik_ah_cookie.get().is_null());
    let gkif = pfi_kif_find(b"tmgrp").expect("group kif");
    assert!(ptr::eq(gkif.pfik_group().expect("group"), group(b"tmgrp")));

    assert!(pfi_kif_match(None, Some(kif)), "no interface: any packet");
    assert!(pfi_kif_match(Some(kif), Some(kif)));
    assert!(!pfi_kif_match(Some(okif), Some(kif)));
    assert!(
        pfi_kif_match(Some(gkif), Some(kif)),
        "a group of the interface"
    );
    assert!(!pfi_kif_match(Some(gkif), Some(okif)));
    let any = pfi_kif_get(b"any", None).expect("any");
    assert!(pfi_kif_match(Some(any), Some(kif)));
    assert!(
        !pfi_kif_match(Some(any), Some(lokif)),
        "any is not a loopback"
    );

    // A skip filter by interface or group name.
    assert!(!pfi_skip_if(b"", kif));
    assert!(!pfi_skip_if(b"tmatch0", kif));
    assert!(!pfi_skip_if(b"tmgrp", kif));
    assert!(pfi_skip_if(b"tmother", kif));
    assert!(
        pfi_skip_if(b"tmatch1", kif),
        "names ending in a digit are no groups"
    );

    // DIOCIGETIFACES: the kifs of the group, copied out.
    let mut buf = std::vec![0u8; 4 * size_of::<PfiKif>()];
    let mut size = 4;
    pfi_get_ifaces(b"tmgrp", &mut buf, &mut size);
    assert_eq!(size, 2, "the group and its member");
    let names: std::vec::Vec<&[u8]> = buf
        .as_chunks::<{ size_of::<PfiKif>() }>()
        .0
        .iter()
        .take(2)
        .map(|c| pf_cstr(&c[..IFNAMSIZ]))
        .collect();
    assert_eq!(names, [&b"tmatch0"[..], &b"tmgrp"[..]]);

    // Detaching drops the hook; the kif stays while a rule refers to it.
    pfi_kif_ref(kif, PFI_KIF_REF_RULE);
    pf_unlock();
    pfi_detach_ifnet(ifp);
    pf_lock();
    assert!(ifp.if_pf_kif.get().is_null());
    assert!(kif.pfik_ifp().is_none());
    assert!(ptr::eq(pfi_kif_find(b"tmatch0").expect("referenced"), kif));
    pfi_kif_unref(Some(kif), PFI_KIF_REF_RULE);
    assert!(pfi_kif_find(b"tmatch0").is_none());
    pfi_kif_unref(Some(any), PFI_KIF_REF_NONE);
}

#[test]
fn skip_flags_are_set_cleared_and_committed() {
    let _g = setup();

    assert_eq!(pfi_set_flags(b"1bad", PFI_IFLAG_SKIP), Err(Errno::EINVAL));
    assert_eq!(
        pfi_set_flags(b"waytoolongifname0", PFI_IFLAG_SKIP),
        Err(Errno::EINVAL)
    );

    // A new kif made by `set skip` holds a flag reference.
    assert_eq!(pfi_set_flags(b"tskip0", PFI_IFLAG_SKIP), Ok(()));
    let k = pfi_kif_find(b"tskip0").expect("created");
    assert_eq!(k.pfik_flagrefs.get(), 1);
    assert_eq!(k.pfik_flags.get() & PFI_IFLAG_SKIP, 0, "not committed yet");
    // A second `set skip` takes no second reference.
    assert_eq!(pfi_set_flags(b"tskip0", PFI_IFLAG_SKIP), Ok(()));
    assert_eq!(k.pfik_flagrefs.get(), 1);

    pfi_xcommit();
    assert_ne!(k.pfik_flags.get() & PFI_IFLAG_SKIP, 0);
    // Setting it again on a committed kif takes no reference either.
    assert_eq!(pfi_set_flags(b"tskip0", PFI_IFLAG_SKIP), Ok(()));
    assert_eq!(k.pfik_flagrefs.get(), 1);

    assert_eq!(
        pfi_clear_flags(b"tnosuch0", PFI_IFLAG_SKIP),
        Err(Errno::ESRCH)
    );
    // Clearing drops the reference and with it the kif, which nothing else holds.
    assert_eq!(pfi_clear_flags(b"tskip0", PFI_IFLAG_SKIP), Ok(()));
    assert!(pfi_kif_find(b"tskip0").is_none());

    // Without a name: every kif.
    let a = pfi_kif_get(b"tskip1", None).expect("created");
    pfi_kif_ref(a, PFI_KIF_REF_RULE);
    assert_eq!(pfi_set_flags(b"", 0x40), Ok(()));
    assert_eq!(a.pfik_flags_new.get() & 0x40, 0x40);
    pfi_xcommit();
    assert_eq!(a.pfik_flags.get() & 0x40, 0x40);
    assert_eq!(pfi_clear_flags(b"", 0x40), Ok(()));
    pfi_xcommit();
    assert_eq!(a.pfik_flags.get() & 0x40, 0);
    pfi_kif_unref(Some(a), PFI_KIF_REF_RULE);
}

#[test]
fn addresses_are_masked_into_the_buffer() {
    let _g = setup();

    let sin = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes([10, 1, 2, 3]),
        },
        sin_zero: [0; 8],
    };
    // Each test setup starts a fresh malloc arena: a buffer from an earlier one could not be
    // freed when it grows. Start from a new one of 64 entries, as `pfi_initialize` does.
    let buf = mallocarray(64, size_of::<PfrAddr>(), PFI_MTYPE, M_WAITOK).expect("buffer");
    PFI_BUFFER.pfi_buffer.set(buf.as_ptr().cast());
    PFI_BUFFER.pfi_buffer_max.set(64);
    PFI_BUFFER.pfi_buffer_cnt.set(0);
    let sa = ptr::from_ref(&sin).cast::<Sockaddr>();
    // SAFETY: `sa` is a `sockaddr_in` on this stack.
    unsafe {
        pfi_address_add(sa, AF_INET, 24);
        pfi_address_add(sa, AF_INET, 20);
        pfi_address_add(sa, AF_INET, 33);
    }
    assert_eq!(PFI_BUFFER.pfi_buffer_cnt.get(), 3);
    // SAFETY: three entries were written.
    let got = unsafe { core::slice::from_raw_parts(PFI_BUFFER.pfi_buffer.get(), 3) };
    assert_eq!(got[0].pfra_af, AF_INET);
    assert_eq!(got[0].pfra_net, 24);
    assert_eq!(&got[0].pfra_u.addr8[..4], &[10, 1, 2, 0]);
    assert_eq!(&got[1].pfra_u.addr8[..4], &[10, 1, 0, 0]);
    assert_eq!(got[2].pfra_net, 128, "more than 32 bits of IPv4 is a host");
    assert_eq!(&got[2].pfra_u.addr8[..4], &[10, 1, 2, 3]);

    // The buffer grows past its first 64 entries.
    PFI_BUFFER.pfi_buffer_cnt.set(0);
    for _ in 0..100 {
        // SAFETY: as above.
        unsafe { pfi_address_add(sa, AF_INET, 32) };
    }
    assert_eq!(PFI_BUFFER.pfi_buffer_cnt.get(), 100);
    assert!(PFI_BUFFER.pfi_buffer_max.get() >= 100);
    PFI_BUFFER.pfi_buffer_cnt.set(0);
}

#[test]
fn unmask_counts_the_prefix() {
    let mut m = PfAddr::zeroed();
    assert_eq!(pfi_unmask(&m), 0);
    m.addr8[..4].copy_from_slice(&[255, 255, 255, 0]);
    assert_eq!(pfi_unmask(&m), 24);
    m.addr8[..4].copy_from_slice(&[255, 255, 240, 0]);
    assert_eq!(pfi_unmask(&m), 20);
    m.addr8[..4].copy_from_slice(&[255; 4]);
    assert_eq!(pfi_unmask(&m), 32);
    m.addr8 = [255; 16];
    assert_eq!(pfi_unmask(&m), 128);
}
