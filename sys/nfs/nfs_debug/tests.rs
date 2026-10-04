use std::cell::RefCell;
use std::string::String;

use super::*;

std::thread_local! {
    static OUT: RefCell<String> = const { RefCell::new(String::new()) };
}

fn capture(args: fmt::Arguments<'_>) -> usize {
    use std::fmt::Write;
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        let before = o.len();
        let _ = o.write_fmt(args);
        o.len() - before
    })
}

fn take() -> String {
    OUT.with(|o| core::mem::take(&mut *o.borrow_mut()))
}

#[test]
fn request_print_short_and_full() {
    let rep = NfsReq::new();
    rep.r_xid.set(0x1234abcd);
    rep.r_flags.set(0x22);
    rep.r_rexmit.set(3);
    rep.r_procnum.set(6);
    rep.r_timer.set(7);
    rep.r_rtt.set(-1);

    nfs_request_print(ptr::from_ref(&rep).cast(), false, capture);
    let s = take();
    assert!(
        s.starts_with("xid 0x1234abcd flags 0x22 rexmit 3 procnum 6 proc 0x0\n"),
        "{s}"
    );
    assert_eq!(s.lines().count(), 1);

    nfs_request_print(ptr::from_ref(&rep).cast(), true, capture);
    let s = take();
    let second = s.lines().nth(1).expect("a second line with /f");
    assert!(
        second.starts_with("mreq 0x0 mrep 0x0 md 0x0 nfsmount 0x0 vnode 0x0 timer 7 rtt -1"),
        "{second}"
    );
}

#[test]
fn node_print_short_and_full() {
    let np = NfsNode::new();
    np.n_size.set(12345);
    np.n_flag.set(5);
    np.n_accstamp.set(99);
    np.n_pushedlo.set(1);
    np.n_pushedhi.set(2);
    np.n_pushlo.set(3);
    np.n_pushhi.set(4);
    np.n_commitflags.set(8);

    nfs_node_print(ptr::from_ref(&np).cast(), false, capture);
    assert_eq!(take(), "size 12345 flag 5 vnode 0x0 accstamp 99\n");

    nfs_node_print(ptr::from_ref(&np).cast(), true, capture);
    assert_eq!(
        take(),
        "size 12345 flag 5 vnode 0x0 accstamp 99\n\
         pushedlo 1 pushedhi 2 pushlo 3 pushhi 4\n\
         commitflags 8\n"
    );
}
