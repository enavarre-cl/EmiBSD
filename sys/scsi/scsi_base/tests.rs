//! Host tests for `scsi_base.rs`: sense decoding and interpretation, the error and retry
//! logic, the iopool run queue, transfer allocation, and the synchronous commands against a
//! fake adapter that completes every command at once (as an emulating HBA under
//! `SCSI_POLL` does).

use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::string::String;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, format, vec};

use super::*;
use crate::kern::subr_pool::pool_destroy;

/// What the fake adapter does with the next command.
enum Reply {
    /// Copies the bytes in (`scsi_copy_internal_data`; nothing for an empty reply).
    Data(Vec<u8>),
    /// Ends the command with this `XS_*` error.
    Error(i32),
    /// Ends the command with `XS_SENSE` and this sense data.
    Sense(ScsiSenseData),
}

std::thread_local! {
    /// The fake adapter's script, and the opcodes it was given.
    static FAKE: RefCell<(VecDeque<Reply>, Vec<u8>)> =
        const { RefCell::new((VecDeque::new(), Vec::new())) };
    /// The CDB, `cmdlen`, `timeout` and data length of every command sent.
    static SENT: RefCell<Vec<(Vec<u8>, i32, i32, i32)>> = const { RefCell::new(Vec::new()) };
    /// The cookies of the I/O handlers that got an opening, in order.
    static SERVED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// The fake adapter's `scsi_cmd`: completes `xs` at once with the next scripted reply.
fn fake_cmd(xs: &'static ScsiXfer) {
    let reply = FAKE.with(|f| {
        let mut f = f.borrow_mut();
        f.1.push(xs.cmd.get().opcode);
        f.0.pop_front()
    });
    SENT.with(|s| {
        let cmd = xs.cmd.get();
        let len = xs.cmdlen.get();
        s.borrow_mut().push((
            cmd.as_bytes()[..len as usize].to_vec(),
            len,
            xs.timeout.get(),
            xs.datalen(),
        ));
    });
    match reply.unwrap_or(Reply::Data(Vec::new())) {
        Reply::Data(d) => {
            if !d.is_empty() {
                scsi_copy_internal_data(xs, &d);
            }
        }
        Reply::Error(e) => xs.error.set(e),
        Reply::Sense(s) => {
            xs.sense.set(s);
            xs.error.set(XS_SENSE);
        }
    }
    scsi_done(xs);
}

static FAKE_ADAPTER: ScsiAdapter = ScsiAdapter {
    scsi_cmd: fake_cmd,
    dev_minphys: None,
    dev_probe: None,
    dev_free: None,
    ioctl: None,
};

fn script(replies: Vec<Reply>) {
    FAKE.with(|f| {
        let mut f = f.borrow_mut();
        f.0 = replies.into();
        f.1.clear();
    });
    SENT.with(|s| s.borrow_mut().clear());
}

/// The CDB, command length, timeout and data length of each command sent since `script`.
fn sent() -> Vec<(Vec<u8>, i32, i32, i32)> {
    SENT.with(|s| s.borrow().clone())
}

fn opcodes() -> Vec<u8> {
    FAKE.with(|f| f.borrow().1.clone())
}

/// A zeroed bus (a softc as `config_attach` hands it out) on the fake adapter.
fn test_bus() -> &'static ScsibusSoftc {
    // SAFETY: a fresh zeroed allocation of the softc's layout, leaked; all-zero is a valid
    // `ScsibusSoftc` (its `Softc` impl).
    let sb = unsafe { &*alloc_zeroed(Layout::new::<ScsibusSoftc>()).cast::<ScsibusSoftc>() };
    sb.sb_adapter.set(Some(&FAKE_ADAPTER));
    sb
}

/// A link with its own default iopool, as `scsi_probe_link` makes one when the adapter has
/// no pool.
fn test_link(openings: u16) -> &'static ScsiLink {
    let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
    let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
    // SAFETY: the default allocator ignores its cookie.
    unsafe {
        scsi_iopool_init(
            pool,
            ptr::from_ref(link).cast_mut().cast(),
            scsi_default_get,
            scsi_default_put,
        );
    }
    link.pool.set(Some(pool));
    link.bus.set(Some(test_bus()));
    link.openings.set(openings);
    link
}

/// A transfer outside the pool, for the functions that only read and write its members.
fn test_xs(link: &'static ScsiLink, flags: i32) -> &'static ScsiXfer {
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    xs.sc_link.set(Some(link));
    xs.flags.set(flags);
    xs.retries.set(SCSI_RETRIES);
    xs
}

/// Real memory and a fresh `scsi_xfer_pool`.
fn setup() -> MutexGuard<'static, ()> {
    let g = crate::kern::subr_pool::tests::setup_real_memory();
    pool_init(
        &SCSI_XFER_POOL,
        size_of::<ScsiXfer>(),
        0,
        IPL_BIO,
        0,
        "scxspl",
        None,
    );
    g
}

fn teardown() {
    assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 0);
    pool_destroy(&SCSI_XFER_POOL);
}

fn decoded(sense: &ScsiSenseData, flag: i32) -> String {
    String::from_utf8_lossy(cstr(&scsi_decode_sense(sense, flag))).into_owned()
}

fn sense(key: u8, asc: u8, ascq: u8) -> ScsiSenseData {
    let mut s = ScsiSenseData::new();
    s.error_code = SSD_ERRCODE_CURRENT;
    s.flags = key;
    s.add_sense_code = asc;
    s.add_sense_code_qual = ascq;
    s
}

#[test]
fn asc2ascii_fixed_dynamic_and_unknown() {
    let mut buf = [0u8; 132];
    asc2ascii(0x3a, 0x00, &mut buf);
    assert_eq!(cstr(&buf), b"Medium Not Present");
    asc2ascii(0x40, 0x81, &mut buf);
    assert_eq!(cstr(&buf), b"Diagnostic Failure on Component 0x81");
    asc2ascii(0x4d, 0x07, &mut buf);
    assert_eq!(cstr(&buf), b"Tagged Overlapped Commands (0x07 = TASK TAG)");
    asc2ascii(0x70, 0x2a, &mut buf);
    assert_eq!(
        cstr(&buf),
        b"Decompression Exception Short Algorithm ID OF 0x2a"
    );
    asc2ascii(0xfe, 0xdc, &mut buf);
    assert_eq!(cstr(&buf), b"ASC 0xfe ASCQ 0xdc");
    // A short buffer truncates, as strlcpy and snprintf do.
    let mut small = [0u8; 7];
    asc2ascii(0x3a, 0x00, &mut small);
    assert_eq!(cstr(&small), b"Medium");
}

#[test]
fn decode_sense_parts() {
    let mut s = sense(SKEY_NOT_READY, 0x04, 0x01);
    assert_eq!(decoded(&s, DECODE_SENSE_KEY), "Not Ready");
    assert_eq!(
        decoded(&s, DECODE_ASC_ASCQ),
        "Logical Unit Is in Process Of Becoming Ready"
    );
    // No sense key specific information without SKSV and enough extra bytes.
    assert_eq!(decoded(&s, DECODE_SKSV), "");
    s.extra_len = 10;
    s.sense_key_spec_1 = SSD_SCS_VALID;
    s.sense_key_spec_2 = 0x01;
    s.sense_key_spec_3 = 0x02;
    assert_eq!(decoded(&s, DECODE_SKSV), "Progress Indicator: 258");
    s.flags = SKEY_MEDIUM_ERROR;
    assert_eq!(decoded(&s, DECODE_SKSV), "Actual Retry Count: 258");
    s.flags = SKEY_ILLEGAL_REQUEST;
    s.sense_key_spec_1 = SSD_SCS_VALID | SSD_SCS_CDB_ERROR | SSD_SCS_VALID_BIT_INDEX | 3;
    assert_eq!(decoded(&s, DECODE_SKSV), "Error in CDB, Offset 258, bit 3");
    s.sense_key_spec_1 = SSD_SCS_VALID;
    assert_eq!(decoded(&s, DECODE_SKSV), "Error in Parameters, Offset 258");
    s.flags = SKEY_UNIT_ATTENTION;
    assert_eq!(decoded(&s, DECODE_SKSV), "");
    assert_eq!(decoded(&s, 0), "");
    for (key, name) in SENSE_KEYS.iter().enumerate() {
        s.flags = key as u8;
        assert_eq!(decoded(&s, DECODE_SENSE_KEY), *name);
    }
}

#[test]
fn interpret_sense_maps_keys_to_errnos() {
    let link = test_link(1);
    let xs = test_xs(link, SCSI_SILENT | SCSI_NOSLEEP);
    let check = |s: ScsiSenseData| {
        xs.sense.set(s);
        scsi_interpret_sense(xs)
    };

    // An error code that is neither current nor deferred: no key to go by.
    let mut s = sense(SKEY_NO_SENSE, 0, 0);
    s.error_code = 0x00;
    assert_eq!(check(s), Err(EIO));

    // NO SENSE: not a short read if nothing was transferred.
    let mut buf = [0u8; 8];
    // SAFETY: `buf` outlives the transfer's use of it; nothing else touches it.
    unsafe { xs.set_data(buf.as_mut_ptr(), 8) };
    xs.resid.set(8);
    assert_eq!(check(sense(SKEY_NO_SENSE, 0, 0)), Ok(()));
    assert_eq!(xs.resid.get(), 0);
    xs.clear_data();

    assert_eq!(check(sense(SKEY_BLANK_CHECK, 0, 0)), Ok(()));

    // NOT READY.
    assert_eq!(check(sense(SKEY_NOT_READY, 0x04, 0x01)), Err(ERESTART));
    xs.retries.set(0);
    assert_eq!(check(sense(SKEY_NOT_READY, 0x04, 0x01)), Err(EIO));
    xs.retries.set(1);
    link.flags.set(SDEV_MEDIA_LOADED);
    assert_eq!(check(sense(SKEY_NOT_READY, 0x3a, 0x00)), Err(ENOMEDIUM));
    assert_eq!(link.flags.get() & SDEV_MEDIA_LOADED, 0);
    xs.flags.set(xs.flags.get() | SCSI_IGNORE_NOT_READY);
    assert_eq!(check(sense(SKEY_NOT_READY, 0x3a, 0x00)), Ok(()));
    xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

    // MEDIUM ERROR.
    assert_eq!(check(sense(SKEY_MEDIUM_ERROR, 0x3a, 0x02)), Err(ENOMEDIUM));
    assert_eq!(
        check(sense(SKEY_MEDIUM_ERROR, 0x30, 0x00)),
        Err(EMEDIUMTYPE)
    );
    assert_eq!(check(sense(SKEY_MEDIUM_ERROR, 0x11, 0x00)), Err(EIO));

    // ILLEGAL REQUEST.
    assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x24, 0x00)), Err(EINVAL));
    assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x53, 0x02)), Err(EBUSY));
    xs.flags.set(xs.flags.get() | SCSI_IGNORE_ILLEGAL_REQUEST);
    assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x24, 0x00)), Ok(()));
    xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

    // UNIT ATTENTION: resets are retried; a media change fails on removable media only.
    assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x29, 0x01)), Err(ERESTART));
    assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(ERESTART));
    link.flags.set(SDEV_REMOVABLE | SDEV_MEDIA_LOADED);
    assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(EIO));
    assert_eq!(link.flags.get(), SDEV_REMOVABLE);
    xs.flags.set(xs.flags.get() | SCSI_IGNORE_MEDIA_CHANGE);
    assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(ERESTART));
    xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

    assert_eq!(check(sense(SKEY_WRITE_PROTECT, 0, 0)), Err(EROFS));
    assert_eq!(check(sense(SKEY_ABORTED_COMMAND, 0, 0)), Err(ERESTART));
    assert_eq!(check(sense(SKEY_VOLUME_OVERFLOW, 0, 0)), Err(ENOSPC));
    assert_eq!(
        check(sense(SKEY_HARDWARE_ERROR, 0x52, 0x00)),
        Err(EMEDIUMTYPE)
    );
    assert_eq!(check(sense(SKEY_HARDWARE_ERROR, 0x44, 0x00)), Err(EIO));
    assert_eq!(check(sense(SKEY_MISCOMPARE, 0, 0)), Err(EIO));

    // Without SCSI_SILENT the sense is printed (through sc_print_addr) and the result is
    // the same.
    xs.flags.set(SCSI_NOSLEEP);
    let mut s = sense(SKEY_MEDIUM_ERROR, 0x11, 0x00);
    s.extra_len = 10;
    s.info = [0, 0, 0x12, 0x34];
    s.fru = 3;
    assert_eq!(check(s), Err(EIO));
}

#[test]
fn delay_by_flags() {
    let link = test_link(1);
    let xs = test_xs(link, SCSI_NOSLEEP);
    assert_eq!(scsi_delay(xs, 1), Err(ERESTART));
    xs.flags.set(SCSI_AUTOCONF);
    assert_eq!(scsi_delay(xs, 1), Err(EIO));
}

#[test]
fn xs_error_categories_and_retries() {
    let link = test_link(1);
    let xs = test_xs(link, SCSI_SILENT | SCSI_NOSLEEP);

    xs.error.set(XS_NOERROR);
    assert_eq!(scsi_xs_error(xs), Ok(()));
    xs.error.set(XS_DRIVER_STUFFUP);
    assert_eq!(scsi_xs_error(xs), Err(EIO));
    xs.error.set(XS_SELTIMEOUT);
    assert_eq!(scsi_xs_error(xs), Err(EIO));
    xs.error.set(0x77);
    assert_eq!(scsi_xs_error(xs), Err(EIO));

    // ERESTART while retries last, then EIO.
    xs.retries.set(1);
    xs.error.set(XS_TIMEOUT);
    assert_eq!(scsi_xs_error(xs), Err(ERESTART));
    xs.error.set(XS_RESET);
    assert_eq!(scsi_xs_error(xs), Err(EIO));

    // The link's own interpret_sense decides about sense data.
    fn always_busy(_: &'static ScsiXfer) -> Result<(), Errno> {
        Err(EBUSY)
    }
    link.interpret_sense.set(always_busy);
    xs.error.set(XS_SENSE);
    assert_eq!(scsi_xs_error(xs), Err(EBUSY));

    link.state.set(SDEV_S_DYING);
    xs.error.set(XS_NOERROR);
    assert_eq!(scsi_xs_error(xs), Err(ENXIO));
}

/// A pool of `free` openings, numbered from 1.
struct FakeIo {
    free: Cell<usize>,
    next: Cell<usize>,
}

unsafe fn fake_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the cookie is the test's `FakeIo`.
    let f = unsafe { &*cookie.cast::<FakeIo>() };
    if f.free.get() == 0 {
        return None;
    }
    f.free.set(f.free.get() - 1);
    f.next.set(f.next.get() + 1);
    NonNull::new(ptr::without_provenance_mut(f.next.get()))
}

unsafe fn fake_put(cookie: *mut c_void, _io: ScsiIo) {
    // SAFETY: as in `fake_get`.
    let f = unsafe { &*cookie.cast::<FakeIo>() };
    f.free.set(f.free.get() + 1);
}

/// Records which handler (its cookie) got an opening.
unsafe fn record(cookie: *mut c_void, io: Option<ScsiIo>) {
    assert!(io.is_some());
    SERVED.with(|s| s.borrow_mut().push(cookie as usize));
}

#[test]
fn iopool_serves_handlers_in_queue_order() {
    let f: &'static FakeIo = Box::leak(Box::new(FakeIo {
        free: Cell::new(0),
        next: Cell::new(0),
    }));
    let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
    // SAFETY: `fake_get`/`fake_put` take the `FakeIo`, which is leaked.
    unsafe { scsi_iopool_init(pool, ptr::from_ref(f).cast_mut().cast(), fake_get, fake_put) };
    SERVED.with(|s| s.borrow_mut().clear());

    let iohs: Vec<&'static ScsiIohandler> = (1..=4)
        .map(|i| {
            let ioh: &'static ScsiIohandler = Box::leak(Box::new(ScsiIohandler::new()));
            // SAFETY: `record` reads no cookie, it only records it.
            unsafe { scsi_ioh_set(ioh, pool, record, ptr::without_provenance_mut(i)) };
            ioh
        })
        .collect();

    // Nothing free: they all queue, in order; adding again is a no-op.
    for ioh in &iohs {
        assert!(scsi_ioh_add(ioh));
    }
    assert!(!scsi_ioh_add(iohs[0]));
    assert!(SERVED.with(|s| s.borrow().is_empty()));
    assert_eq!(scsi_io_get(pool, SCSI_NOSLEEP), None);

    // Taking one off the queue skips it.
    assert!(scsi_ioh_del(iohs[1]));
    assert!(!scsi_ioh_del(iohs[1]));

    // Each opening given back goes to the head of the queue.
    let io = NonNull::new(ptr::without_provenance_mut(100)).unwrap_or(SCSI_IOPOOL_POISON);
    scsi_io_put(pool, io);
    assert_eq!(SERVED.with(|s| s.borrow().clone()), vec![1]);
    scsi_io_put(pool, io);
    scsi_io_put(pool, io);
    assert_eq!(SERVED.with(|s| s.borrow().clone()), vec![1, 3, 4]);
    assert!(pool.queue.is_empty());

    // With the queue empty, a returned opening stays free.
    scsi_io_put(pool, io);
    assert_eq!(f.free.get(), 1);
    assert!(scsi_io_get(pool, SCSI_NOSLEEP).is_some());
    assert_eq!(f.free.get(), 0);
}

#[test]
fn pending_start_and_finish() {
    let mtx = Mutex::new(IPL_BIO);
    let running = Cell::new(0);
    assert!(scsi_pending_start(&mtx, &running));
    // A second entry asks the running one for another round.
    assert!(!scsi_pending_start(&mtx, &running));
    assert!(!scsi_pending_finish(&mtx, &running));
    assert_eq!(running.get(), 1);
    assert!(scsi_pending_finish(&mtx, &running));
    assert_eq!(running.get(), 0);
}

#[test]
fn xs_get_and_put_account_for_openings_and_pool_items() {
    let _g = setup();
    let link = test_link(2);

    let xs1 = scsi_xs_get(link, SCSI_NOSLEEP | SCSI_DATA_IN);
    let xs2 = scsi_xs_get(link, SCSI_NOSLEEP);
    let (Some(xs1), Some(xs2)) = (xs1, xs2) else {
        panic!("two openings, two transfers");
    };
    assert_eq!(link.pending.get(), 2);
    assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 2);
    // The link has no third opening, and SCSI_NOSLEEP does not wait for one.
    assert!(scsi_xs_get(link, SCSI_NOSLEEP).is_none());
    assert_eq!(link.pending.get(), 2);

    assert_eq!(xs1.flags.get(), SCSI_NOSLEEP | SCSI_DATA_IN);
    assert_eq!(xs1.retries.get(), SCSI_RETRIES);
    assert_eq!(xs1.timeout.get(), 10000);
    assert_eq!(xs1.io.get(), Some(SCSI_IOPOOL_POISON));
    assert!(ptr::eq(xs1.link(), link));
    assert!(xs1.done.get().is_none() && xs1.cookie.get().is_null());

    scsi_xs_put(xs1);
    assert_eq!(link.pending.get(), 1);
    assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 1);
    scsi_xs_put(xs2);
    assert_eq!(link.pending.get(), 0);

    // A dying link hands out nothing.
    link.state.set(SDEV_S_DYING);
    assert!(scsi_xs_get(link, SCSI_NOSLEEP).is_none());
    link.state.set(0);

    // A link shutdown with nothing outstanding returns at once.
    scsi_link_shutdown(link);
    teardown();
}

#[test]
fn xsh_handler_gets_a_transfer() {
    let _g = setup();
    let link = test_link(1);

    std::thread_local! {
        static GOT: RefCell<Vec<&'static ScsiXfer>> = const { RefCell::new(Vec::new()) };
    }
    fn handler(xs: &'static ScsiXfer) {
        GOT.with(|g| g.borrow_mut().push(xs));
    }
    let xsh: &'static ScsiXshandler = Box::leak(Box::new(ScsiXshandler::new()));
    scsi_xsh_set(xsh, link, handler);

    // The link's one opening is taken: the handler waits on the link.
    let Some(xs) = scsi_xs_get(link, SCSI_NOSLEEP) else {
        panic!("the link has an opening");
    };
    assert!(scsi_xsh_add(xsh));
    assert!(!scsi_xsh_add(xsh));
    assert!(GOT.with(|g| g.borrow().is_empty()));

    // Putting the transfer back frees the opening, and the handler gets a new transfer.
    scsi_xs_put(xs);
    let got = GOT.with(|g| g.borrow_mut().pop());
    let Some(xs) = got else {
        panic!("the handler ran");
    };
    assert_eq!(xs.flags.get(), SCSI_NOSLEEP);
    assert_eq!(link.pending.get(), 1);
    assert!(!scsi_xsh_del(xsh));
    scsi_xs_put(xs);

    // Queued and deleted before an opening came: never runs.
    let Some(xs) = scsi_xs_get(link, SCSI_NOSLEEP) else {
        panic!("the link has an opening");
    };
    assert!(scsi_xsh_add(xsh));
    assert!(scsi_xsh_del(xsh));
    scsi_xs_put(xs);
    assert!(GOT.with(|g| g.borrow().is_empty()));
    assert_eq!(link.pending.get(), 0);
    teardown();
}

/// The standard INQUIRY data of a disk, `extra` bytes past the SCSI-2 36.
fn inquiry_reply(extra: u8, len: usize) -> Vec<u8> {
    let mut d = vec![0u8; len];
    d[0] = T_DIRECT;
    d[2] = SCSI_REV_SPC3;
    d[4] = (SID_SCSI2_ALEN as u8) + extra;
    d[8..16].copy_from_slice(b"VirtIO  ");
    d
}

#[test]
fn sync_commands_against_a_completing_adapter() {
    let _g = setup();
    let link = test_link(1);

    // TEST UNIT READY: one command, no data.
    script(vec![]);
    assert_eq!(
        scsi_test_unit_ready(link, TEST_READY_RETRIES, SCSI_NOSLEEP),
        Ok(())
    );
    assert_eq!(opcodes(), vec![TEST_UNIT_READY]);

    // A timeout is retried; a driver failure is not.
    script(vec![Reply::Error(XS_TIMEOUT), Reply::Data(vec![])]);
    assert_eq!(scsi_test_unit_ready(link, 2, SCSI_NOSLEEP), Ok(()));
    assert_eq!(opcodes(), vec![TEST_UNIT_READY, TEST_UNIT_READY]);
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(scsi_test_unit_ready(link, 2, SCSI_NOSLEEP), Err(EIO));
    // Retries run out.
    script(vec![
        Reply::Error(XS_TIMEOUT),
        Reply::Error(XS_TIMEOUT),
        Reply::Error(XS_TIMEOUT),
    ]);
    assert_eq!(scsi_test_unit_ready(link, 1, SCSI_NOSLEEP), Err(EIO));
    assert_eq!(opcodes().len(), 2);
    // Sense data goes through the link's interpret_sense.
    script(vec![Reply::Sense(sense(SKEY_WRITE_PROTECT, 0x27, 0))]);
    assert_eq!(
        scsi_test_unit_ready(link, 1, SCSI_NOSLEEP | SCSI_SILENT),
        Err(EROFS)
    );

    // PREVENT ALLOW, unless the quirk says the door does not lock.
    script(vec![]);
    assert_eq!(
        scsi_prevent(link, i32::from(PR_PREVENT), SCSI_NOSLEEP),
        Ok(())
    );
    assert_eq!(opcodes(), vec![PREVENT_ALLOW]);
    link.quirks.set(ADEV_NODOORLOCK);
    script(vec![]);
    assert_eq!(
        scsi_prevent(link, i32::from(PR_PREVENT), SCSI_NOSLEEP),
        Ok(())
    );
    assert!(opcodes().is_empty());
    link.quirks.set(0);

    // INQUIRY: the basic 36 bytes are enough.
    let mut inq = ScsiInquiryData::new();
    script(vec![Reply::Data(inquiry_reply(0, 36))]);
    assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
    assert_eq!(opcodes(), vec![INQUIRY]);
    assert_eq!(&inq.vendor, b"VirtIO  ");
    assert_eq!(usize::from(inq.additional_length), SID_SCSI2_ALEN);

    // The device has more: asked for again, once, for everything.
    let mut inq = ScsiInquiryData::new();
    script(vec![
        Reply::Data(inquiry_reply(20, 36)),
        Reply::Data(inquiry_reply(20, 56)),
    ]);
    assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
    assert_eq!(opcodes(), vec![INQUIRY, INQUIRY]);
    assert_eq!(usize::from(inq.additional_length), SID_SCSI2_ALEN + 20);

    // ... and if it still sends less, the length is cut to what came.
    let mut inq = ScsiInquiryData::new();
    script(vec![
        Reply::Data(inquiry_reply(40, 36)),
        Reply::Data(inquiry_reply(40, 50)),
    ]);
    assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
    assert_eq!(usize::from(inq.additional_length), 50 - SID_SCSI2_HDRLEN);

    // Too little for a header.
    script(vec![Reply::Data(vec![0; 4])]);
    assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Err(EINVAL));

    // VPD pages are not asked of UMASS devices.
    let mut vpd = ScsiVpdSerial::default();
    script(vec![Reply::Data(vec![
        0,
        SI_PG_SERIAL,
        0,
        4,
        b'S',
        b'N',
        b'0',
        b'1',
    ])]);
    assert_eq!(
        scsi_inquire_vpd(link, vpd.as_bytes_mut(), SI_PG_SERIAL, SCSI_NOSLEEP),
        Ok(())
    );
    assert_eq!(&vpd.serial[..4], b"SN01");
    link.flags.set(SDEV_UMASS);
    assert_eq!(
        scsi_inquire_vpd(link, vpd.as_bytes_mut(), SI_PG_SERIAL, SCSI_NOSLEEP),
        Err(EJUSTRETURN)
    );
    link.flags.set(0);

    assert_eq!(link.pending.get(), 0);
    teardown();
}

#[test]
fn read_capacity_and_start_stop_send_their_cdbs() {
    let _g = setup();
    let link = test_link(1);

    // READ CAPACITY (10): the data lands in the caller's structure.
    let mut rc = ScsiReadCapData::default();
    script(vec![Reply::Data(vec![0, 0, 0x0f, 0xff, 0, 0, 2, 0])]);
    assert_eq!(scsi_read_cap_10(link, &mut rc, SCSI_NOSLEEP), Ok(()));
    assert_eq!(
        sent(),
        vec![(vec![READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10, 20000, 8)]
    );
    assert_eq!(_4btol(&rc.addr), 0x0fff);
    assert_eq!(_4btol(&rc.length), 512);

    // READ CAPACITY (16): the service action and the allocation length.
    let mut rc16 = ScsiReadCapData16::default();
    let mut reply = vec![0u8; 32];
    reply[7] = 0xff;
    reply[11] = 0x10;
    script(vec![Reply::Data(reply)]);
    assert_eq!(scsi_read_cap_16(link, &mut rc16, SCSI_NOSLEEP), Ok(()));
    let mut cdb = vec![READ_CAPACITY_16, SRC16_SERVICE_ACTION];
    cdb.extend_from_slice(&[0; 8]);
    cdb.extend_from_slice(&[0, 0, 0, 32, 0, 0]);
    assert_eq!(sent(), vec![(cdb, 16, 20000, 32)]);
    assert_eq!(_8btol(&rc16.addr), 0xff);
    assert_eq!(_4btol(&rc16.length), 0x10);

    // A failed transfer comes back as the error.
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(scsi_read_cap_10(link, &mut rc, SCSI_NOSLEEP), Err(EIO));

    // START STOP UNIT: the timeout depends on the action.
    script(vec![]);
    assert_eq!(scsi_start(link, i32::from(SSS_START), SCSI_NOSLEEP), Ok(()));
    assert_eq!(scsi_start(link, i32::from(SSS_LOEJ), SCSI_NOSLEEP), Ok(()));
    assert_eq!(
        sent(),
        vec![
            (vec![START_STOP, 0, 0, 0, SSS_START, 0], 6, 30000, 0),
            (vec![START_STOP, 0, 0, 0, SSS_LOEJ, 0], 6, 10000, 0),
        ]
    );

    assert_eq!(link.pending.get(), 0);
    teardown();
}

/// A generic command holding the CDB `bytes`.
fn generic(bytes: &[u8]) -> ScsiGeneric {
    let mut g = ScsiGeneric::zeroed();
    g.as_bytes_mut()[..bytes.len()].copy_from_slice(bytes);
    g
}

#[test]
fn rw_decode_reads_every_cdb_size() {
    // READ (6) and WRITE (6): a 21-bit address, a length of 0 meaning 256.
    for op in [READ_COMMAND, WRITE_COMMAND] {
        assert_eq!(
            scsi_cmd_rw_decode(&generic(&[op, 0xff, 0x12, 0x34, 8, 0])),
            (0x1f1234, 8)
        );
        assert_eq!(
            scsi_cmd_rw_decode(&generic(&[op, 0, 0, 5, 0, 0])),
            (5, 0x100)
        );
    }
    // READ (10) and WRITE (10).
    for op in [READ_10, WRITE_10] {
        assert_eq!(
            scsi_cmd_rw_decode(&generic(&[op, 0, 0x89, 0xab, 0xcd, 0xef, 0, 1, 2, 0])),
            (0x89ab_cdef, 0x0102)
        );
    }
    // READ (12) and WRITE (12).
    for op in [READ_12, WRITE_12] {
        assert_eq!(
            scsi_cmd_rw_decode(&generic(&[op, 0, 0x89, 0xab, 0xcd, 0xef, 1, 2, 3, 4, 0, 0])),
            (0x89ab_cdef, 0x0102_0304)
        );
    }
    // READ (16) and WRITE (16).
    for op in [READ_16, WRITE_16] {
        assert_eq!(
            scsi_cmd_rw_decode(&generic(&[
                op, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0x0a, 0x0b, 0x0c, 0x0d, 0, 0
            ])),
            (0x0102_0304_0506_0708, 0x0a0b_0c0d)
        );
    }
}

/// A MODE SENSE (6) reply: header, one direct-access block descriptor, and `page`.
fn mode_sense_reply(page: &[u8]) -> Vec<u8> {
    let mut d = vec![0u8, 0, 0, 8];
    d.extend_from_slice(&[0, 0, 0x10, 0, 0, 0, 0x02, 0]); // 4096 blocks of 512 bytes
    d.extend_from_slice(page);
    d[0] = (d.len() - 1) as u8;
    d
}

#[test]
fn mode_sense_finds_the_page_and_the_block_descriptor() {
    let _g = setup();
    let link = test_link(1);
    let mut inq = ScsiInquiryData::new();
    inq.version = SCSI_REV_SPC3;
    link.inqdata.set(inq);

    let page = [
        8u8, 0x12, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut buf = ScsiModeSenseBuf::new();
    script(vec![Reply::Data(mode_sense_reply(&page))]);
    let r = scsi_do_mode_sense(link, 8, &mut buf, 20 - 4, SCSI_NOSLEEP);
    assert_eq!(r, Ok((Some(12), false)));
    assert_eq!(opcodes(), vec![MODE_SENSE]);
    assert_eq!(buf.buf[12 + 2], 0x04);

    let (mut density, mut count, mut size) = (99u32, 0u64, 0u32);
    scsi_parse_blkdesc(
        link,
        &buf,
        false,
        Some(&mut density),
        Some(&mut count),
        Some(&mut size),
    );
    assert_eq!((density, count, size), (0, 4096, 512));

    // Another page than asked for, or too short a page: no page, but no error.
    script(vec![Reply::Data(mode_sense_reply(&page))]);
    assert_eq!(
        scsi_do_mode_sense(link, 4, &mut buf, 16, SCSI_NOSLEEP),
        Ok((None, false))
    );
    script(vec![Reply::Data(mode_sense_reply(&page))]);
    assert_eq!(
        scsi_do_mode_sense(link, 8, &mut buf, 64, SCSI_NOSLEEP),
        Ok((None, false))
    );

    // MODE SENSE (6) fails: a SCSI-2 device is asked with MODE SENSE (10).
    let mut big = vec![0u8, 0, 0, 0, 0, 0, 0, 0];
    big.extend_from_slice(&page);
    big[1] = (big.len() - 2) as u8;
    script(vec![Reply::Error(XS_DRIVER_STUFFUP), Reply::Data(big)]);
    let r = scsi_do_mode_sense(link, 8, &mut buf, 16, SCSI_NOSLEEP);
    assert_eq!(r, Ok((Some(8), true)));
    assert_eq!(opcodes(), vec![MODE_SENSE, MODE_SENSE_BIG]);

    // ... but a SCSI-1 device is not.
    inq.version = SCSI_REV_1;
    link.inqdata.set(inq);
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(
        scsi_do_mode_sense(link, 8, &mut buf, 16, SCSI_NOSLEEP),
        Err(EIO)
    );
    assert_eq!(opcodes(), vec![MODE_SENSE]);

    // A reply without a valid header is an error.
    script(vec![Reply::Data(vec![2, 0, 0, 0])]);
    assert_eq!(scsi_mode_sense(link, 8, &mut buf, SCSI_NOSLEEP), Err(EIO));

    // MODE SELECT sends what the header says and zeroes its length.
    let mut sel = [5u8, 0, 0, 0, 8, 2];
    script(vec![]);
    assert_eq!(
        scsi_mode_select(link, i32::from(SMS_PF), &mut sel, SCSI_NOSLEEP, 1000),
        Ok(())
    );
    assert_eq!(sel[0], 0);
    assert_eq!(opcodes(), vec![MODE_SELECT]);

    assert_eq!(link.pending.get(), 0);
    teardown();
}

#[test]
fn mode_page_offsets_stay_in_the_buffer() {
    let mut buf = ScsiModeSenseBuf::new();
    // The header claims a block descriptor that ends past the buffer.
    buf.buf[0] = 255;
    buf.buf[3] = 250;
    assert_eq!(scsi_mode_sense_page(&buf, 0, 1), None);
    buf.buf[3] = 0;
    buf.buf[4] = 0x45;
    assert_eq!(scsi_mode_sense_page(&buf, 5, 4), Some(4));
    assert_eq!(scsi_mode_sense_page(&buf, 5, 300), None);
    // MODE SENSE (10): header of 8 bytes.
    let mut buf = ScsiModeSenseBuf::new();
    buf.buf[1] = 20;
    buf.buf[8] = 0x08;
    assert_eq!(scsi_mode_sense_big_page(&buf, 8, 10), Some(8));
    buf.buf[6] = 0xff; // blk_desc_len 0xff00
    assert_eq!(scsi_mode_sense_big_page(&buf, 8, 10), None);
}

#[test]
fn alt_hex_is_printf_sharp_x() {
    assert_eq!(format!("{}", AltHex(0)), "0");
    assert_eq!(format!("{}", AltHex(0x70)), "0x70");
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn adesc_matches_the_c_table() {
    let path = crate::reftest::openbsd_src().join("sys/scsi/scsi_base.c");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let start = text.find("} adesc[] = {").unwrap_or(0);
    let end = text[start..].find("{ 0x00, 0x00, NULL }").unwrap_or(0) + start;
    let mut c = Vec::new();
    for line in text[start..end].lines() {
        let line = line.trim();
        if !line.starts_with("{ 0x") {
            continue;
        }
        let mut parts = line.trim_start_matches('{').splitn(3, ',');
        let asc = parts.next().unwrap_or_default().trim();
        let ascq = parts.next().unwrap_or_default().trim();
        let desc = parts.next().unwrap_or_default().trim();
        let desc = desc.trim_end_matches(',').trim_end_matches('}').trim();
        let desc = desc.trim_matches('"');
        let num = |s: &str| u8::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0);
        c.push((num(asc), num(ascq), String::from(desc)));
    }
    assert_eq!(c.len(), ADESC.len());
    for (ours, theirs) in ADESC.iter().zip(&c) {
        assert_eq!(
            (ours.0, ours.1, ours.2),
            (theirs.0, theirs.1, theirs.2.as_str())
        );
    }
}
