//! Host tests for bio(4): registering, looking up by name and by cookie and unregistering
//! controllers, `bioioctl`'s `BIOCLOCATE` against a fake controller and the delegation of the
//! other commands, and `bio_status`'s message buffer.

use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::ffi::CString;
use std::string::String;
use std::sync::atomic::{AtomicU64, Ordering};
use std::vec::Vec;
use std::{assert, assert_eq, format, vec};

use super::*;
use crate::dev::biovar::{BIO_STATUS_ERROR, BiocInq};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::types::makedev;

/// A zeroed device called `name`, leaked (an all-zero `Device` is an unattached one).
fn device(name: &str) -> &'static Device {
    // SAFETY: a fresh zeroed allocation of the layout of `Device`, leaked; the all-zero bit
    // pattern is a valid unattached `Device` (empty cells and lists, a zero reference count).
    let dev = unsafe { &*alloc_zeroed(Layout::new::<Device>()).cast::<Device>() };
    let mut xname = [0u8; 16];
    xname[..name.len()].copy_from_slice(name.as_bytes());
    dev.dv_xname.set(xname);
    dev
}

fn proc() -> &'static Proc {
    Box::leak(Box::new(Proc::new()))
}

/// The last command a fake controller was handed.
static LAST_CMD: AtomicU64 = AtomicU64::new(0);

/// The marker the fake controller writes behind the `struct bio` of its argument.
const MARK: u8 = 0x5a;

/// A controller: records the command, marks the argument, refuses `BIOCALARM`.
fn fake_ioctl(dev: &Device, cmd: u64, addr: &mut [u8]) -> Result<(), Errno> {
    assert!(dev.xname().starts_with("fake"));
    LAST_CMD.store(cmd, Ordering::Relaxed);
    if cmd == BIOCALARM {
        return Err(Errno::EOPNOTSUPP);
    }
    addr[size_of::<Bio>()] = MARK;
    Ok(())
}

/// The names of the registered controllers, in tree order.
fn registered() -> Vec<String> {
    BIOS.0
        .iter()
        .map(|bm| bm.bm_dev.map_or(String::new(), |d| d.xname().into()))
        .collect()
}

/// A `struct bioc_inq`-sized argument whose `struct bio` carries `cookie`.
fn inq_arg(cookie: usize) -> Vec<u8> {
    let mut arg = vec![0u8; size_of::<BiocInq>()];
    arg[offset_of!(Bio, bio_cookie)..][..size_of::<usize>()].copy_from_slice(&cookie.to_ne_bytes());
    arg
}

/// `BIOCLOCATE`'s argument naming the controller at the user address of `name`.
fn locate_arg(name: &CString) -> Vec<u8> {
    let mut arg = vec![0u8; size_of::<BioLocate>()];
    arg[offset_of!(BioLocate, bl_name)..][..size_of::<usize>()]
        .copy_from_slice(&(name.as_ptr() as usize).to_ne_bytes());
    arg
}

/// The cookie `BIOCLOCATE` left in `arg`.
fn cookie_of(arg: &[u8]) -> usize {
    arg_usize(arg, offset_of!(Bio, bio_cookie)).expect("cookie")
}

#[test]
fn register_lookup_validate_unregister() {
    let _g = setup_real_memory();
    let (a, b) = (device("fake0"), device("fake1"));

    assert!(registered().is_empty());
    bio_register(a, fake_ioctl).expect("register a");
    bio_register(b, fake_ioctl).expect("register b");
    assert_eq!(registered().len(), 2);

    let (ma, mb) = (
        bio_lookup(b"fake0\0").expect("a by name"),
        bio_lookup(b"fake1").expect("b by name"),
    );
    assert!(ptr::eq(ma.bm_dev.expect("device"), a));
    assert!(ptr::eq(mb.bm_dev.expect("device"), b));
    assert_ne!(ma.bm_cookie.get(), mb.bm_cookie.get());
    assert!(bio_lookup(b"fake").is_none());
    assert!(bio_lookup(b"fake10").is_none());
    assert!(bio_lookup(b"").is_none());

    // The cookie finds the mapping; any other number does not.
    let found = bio_validate(ma.bm_cookie.get()).expect("a by cookie");
    assert!(ptr::eq(found, ma));
    assert!(ptr::eq(
        bio_validate(mb.bm_cookie.get()).expect("b by cookie"),
        mb
    ));
    let stranger = (0..).find(|&n| n != ma.bm_cookie.get() && n != mb.bm_cookie.get());
    assert!(bio_validate(stranger.expect("a free cookie")).is_none());

    // Unregistering a device forgets only its mapping (the freed `ma` is not touched again).
    let (cookie_a, cookie_b) = (ma.bm_cookie.get(), mb.bm_cookie.get());
    bio_unregister(a);
    assert!(bio_lookup(b"fake0").is_none());
    assert!(bio_validate(cookie_a).is_none());
    assert!(bio_validate(cookie_b).is_some());
    assert!(bio_lookup(b"fake1").is_some());
    bio_unregister(a); // already gone: nothing happens
    assert_eq!(registered(), ["fake1"]);

    bio_unregister(b);
    assert!(registered().is_empty());
}

#[test]
fn many_controllers_keep_distinct_cookies() {
    let _g = setup_real_memory();
    let devs: Vec<&'static Device> = (0..40).map(|i| device(&format!("fake{i}"))).collect();

    for d in &devs {
        bio_register(d, fake_ioctl).expect("register");
    }
    let mut cookies: Vec<usize> = BIOS.0.iter().map(|bm| bm.bm_cookie.get()).collect();
    // The tree is ordered by cookie, descending, as `bio_cookie_cmp` has it.
    assert!(cookies.windows(2).all(|w| w[0] > w[1]));
    cookies.dedup();
    assert_eq!(cookies.len(), devs.len());
    for d in &devs {
        let bm = bio_lookup(d.xname().as_bytes()).expect("by name");
        assert!(ptr::eq(
            bio_validate(bm.bm_cookie.get()).expect("by cookie"),
            bm
        ));
    }

    for d in &devs {
        bio_unregister(d);
    }
    assert!(registered().is_empty());
}

#[test]
fn biolocate_gives_the_cookie_and_the_cookie_reaches_the_controller() {
    let _g = setup_real_memory();
    let (dev, p) = (makedev(79, 0), proc());
    let ctl = device("fake0");
    bio_register(ctl, fake_ioctl).expect("register");

    bioopen(dev, 0, 0, p).expect("open");

    // BIOCLOCATE by name.
    let name = CString::new("fake0").expect("name");
    let mut arg = locate_arg(&name);
    bioioctl(dev, BIOCLOCATE, &mut arg, 0, p).expect("locate");
    let cookie = cookie_of(&arg);
    assert_eq!(
        cookie,
        bio_lookup(b"fake0").expect("mapping").bm_cookie.get()
    );

    // The cookie in a `struct bio` routes the other commands to the controller.
    for cmd in [
        BIOCINQ,
        BIOCDISK,
        BIOCVOL,
        BIOCBLINK,
        BIOCSETSTATE,
        BIOCCREATERAID,
        BIOCDELETERAID,
        BIOCDISCIPLINE,
        BIOCINSTALLBOOT,
        BIOCPATROL,
    ] {
        let mut data = inq_arg(cookie);
        LAST_CMD.store(0, Ordering::Relaxed);
        bioioctl(dev, cmd, &mut data, 0, p).expect("delegated");
        assert_eq!(LAST_CMD.load(Ordering::Relaxed), cmd);
        assert_eq!(data[size_of::<Bio>()], MARK);
    }

    // The controller's error is the ioctl's.
    let mut data = inq_arg(cookie);
    assert_eq!(
        bioioctl(dev, BIOCALARM, &mut data, 0, p),
        Err(Errno::EOPNOTSUPP)
    );

    // An unknown cookie, an unknown name and an unknown command.
    let mut data = inq_arg(cookie.wrapping_add(1));
    if bio_validate(cookie.wrapping_add(1)).is_none() {
        assert_eq!(bioioctl(dev, BIOCINQ, &mut data, 0, p), Err(Errno::ENOENT));
    }
    let nobody = CString::new("fake9").expect("name");
    let mut arg = locate_arg(&nobody);
    assert_eq!(
        bioioctl(dev, BIOCLOCATE, &mut arg, 0, p),
        Err(Errno::ENOENT)
    );
    let mut data = inq_arg(cookie);
    assert_eq!(
        bioioctl(dev, 0x2000_7401, &mut data, 0, p),
        Err(Errno::ENOTTY)
    );
    // A name that does not fit the 16-byte buffer fails in copyinstr.
    let long = CString::new("a-controller-name-too-long").expect("name");
    let mut arg = locate_arg(&long);
    assert!(bioioctl(dev, BIOCLOCATE, &mut arg, 0, p).is_err());
    // An argument too short to hold a `struct bio` is refused.
    assert_eq!(
        bioioctl(dev, BIOCINQ, &mut [0u8; 4], 0, p),
        Err(Errno::EINVAL)
    );

    // After the controller is gone its cookie is dead.
    bio_unregister(ctl);
    let mut data = inq_arg(cookie);
    assert_eq!(bioioctl(dev, BIOCINQ, &mut data, 0, p), Err(Errno::ENOENT));
    bioclose(dev, 0, 0, Some(p)).expect("close");
}

fn msg(bs: &BioStatus, i: usize) -> &[u8] {
    let m = &bs.bs_msgs[i].bm_msg;
    &m[..m.iter().position(|&b| b == 0).expect("terminated")]
}

#[test]
fn bio_status_collects_messages_up_to_the_limit() {
    let ctl = device("fake0");
    let mut bs = BioStatus {
        bs_controller: [0xff; 16],
        bs_status: BIO_STATUS_ERROR,
        bs_msg_count: 3,
        bs_msgs: [BioMsg {
            bm_type: 9,
            bm_msg: [0xff; BIO_MSG_LEN],
        }; BIO_MSG_COUNT],
    };

    bio_status_init(&mut bs, ctl);
    assert_eq!(&bs.bs_controller[..6], b"fake0\0");
    assert_eq!(bs.bs_status, BIO_STATUS_UNKNOWN);
    assert_eq!(bs.bs_msg_count, 0);
    assert!(
        bs.bs_msgs
            .iter()
            .all(|m| m.bm_type == 0 && m.bm_msg[0] == 0)
    );

    bio_info(&mut bs, false, format_args!("volume {} is {}", 3, "fine"));
    bio_warn(&mut bs, false, format_args!("degraded"));
    bio_error(&mut bs, false, format_args!("{:#x}", 255));
    bio_status(&mut bs, true, 77, format_args!("custom"));
    assert_eq!(bs.bs_msg_count, 4);
    assert_eq!(msg(&bs, 0), b"volume 3 is fine");
    assert_eq!(bs.bs_msgs[0].bm_type, BIO_MSG_INFO);
    assert_eq!(msg(&bs, 1), b"degraded");
    assert_eq!(bs.bs_msgs[1].bm_type, BIO_MSG_WARN);
    assert_eq!(msg(&bs, 2), b"0xff");
    assert_eq!(bs.bs_msgs[2].bm_type, BIO_MSG_ERROR);
    assert_eq!(msg(&bs, 3), b"custom");
    assert_eq!(bs.bs_msgs[3].bm_type, 77);

    // The fifth fits, the sixth is dropped (the controller prints that it did).
    let long = "x".repeat(300);
    bio_info(&mut bs, false, format_args!("{long}"));
    assert_eq!(bs.bs_msg_count, BIO_MSG_COUNT as i32);
    assert_eq!(msg(&bs, 4).len(), BIO_MSG_LEN - 1);
    bio_error(&mut bs, true, format_args!("one too many"));
    assert_eq!(bs.bs_msg_count, BIO_MSG_COUNT as i32);
    assert_eq!(msg(&bs, 0), b"volume 3 is fine");
    assert_eq!(bs.bs_msgs[4].bm_type, BIO_MSG_INFO);
}

#[test]
fn bioattach_and_the_pseudo_device_count() {
    bioattach(NBIO);
    assert_eq!(NBIO, 1);
}
