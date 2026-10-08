//! Host tests of the keyboard: the keysym translation (modifiers, locks, control and meta,
//! the keypad, dead keys and compose) through the console path and through an attached
//! keyboard's map, the LEDs, event mode, the bell, repeat and encoding ioctls, and binding
//! to a display.

use core::sync::atomic::AtomicI32;
use std::boxed::Box;

use super::*;
use crate::dev::usb::ukbdmap::UKBD_KEYDESCTAB;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::time::Timespec;

/// The LEDs the fake driver was last told to light.
static LEDS: AtomicI32 = AtomicI32::new(-1);
/// How many times the fake driver was enabled minus disabled.
static ENABLED: AtomicI32 = AtomicI32::new(0);

fn fake_enable(_v: *mut c_void, on: i32) -> i32 {
    ENABLED.fetch_add(if on != 0 { 1 } else { -1 }, Ordering::Relaxed);
    0
}

fn fake_set_leds(_v: *mut c_void, leds: i32) {
    LEDS.store(leds, Ordering::Relaxed);
}

fn fake_ioctl(
    _v: *mut c_void,
    cmd: u64,
    _data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    Ok(cmd == WSKBDIO_COMPLEXBELL)
}

static FAKE_ACCESSOPS: WskbdAccessops = WskbdAccessops {
    enable: fake_enable,
    set_leds: fake_set_leds,
    ioctl: fake_ioctl,
};

/// A translation state over the USB layouts, as the console keyboard's before it attaches.
fn state(layout: KbdT) -> WskbdInternal {
    let id = WskbdInternal::new();
    id.t_keydesc.set(&UKBD_KEYDESCTAB);
    wskbd_update_layout(&id, layout);
    id
}

/// An attached keyboard over the fake driver, with the map of `layout` and no key repeat.
pub(crate) fn keyboard(layout: KbdT) -> &'static WskbdSoftc {
    // SAFETY: all-zero is a valid `WskbdSoftc` (its `Softc` contract); leaked for good.
    let sc: &'static WskbdSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
    let id: &'static WskbdInternal = Box::leak(Box::new(state(layout)));
    sc.id.set(Some(NonNull::from(id)));
    id.t_sc.set(Some(NonNull::from(sc)));
    sc.sc_base.me_ops.set(Some(&WSKBD_SRCOPS));
    sc.sc_accessops.set(Some(&FAKE_ACCESSOPS));
    sc.sc_translating.set(1);
    sc.sc_ledstate.set(-1);
    // SAFETY: the defaults, read on the test's thread.
    sc.sc_bell_data
        .set(unsafe { WSKBD_DEFAULT_BELL_DATA.read() });
    let (map, len) = wskbd_load_keymap(&id.keymap(), layout).unwrap();
    wskbd_set_keymap(sc, map.as_ptr(), len as i32);
    sc
}

/// Presses and releases key `kc`: the keysyms the press produced.
fn tap(id: &WskbdInternal, kc: i32) -> std::vec::Vec<KeysymT> {
    let n = wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, kc);
    let syms = id.t_symbols.get()[..n as usize].to_vec();
    assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_UP, kc), 0);
    syms
}

fn down(id: &WskbdInternal, kc: i32) {
    assert_eq!(
        wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, kc),
        0,
        "a modifier"
    );
}

fn up(id: &WskbdInternal, kc: i32) {
    assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_UP, kc), 0);
}

// USB key codes (usage page 7).
const A: i32 = 4;
const C: i32 = 6;
const E: i32 = 8;
const Y: i32 = 28;
const ONE: i32 = 30;
const TWO: i32 = 31;
const THREE: i32 = 32;
const RET: i32 = 40;
const DEAD_ACUTE_DE: i32 = 46;
const CAPS: i32 = 57;
const NUMLOCK: i32 = 83;
const KP1: i32 = 89;
const CTRL_L: i32 = 224;
const SHIFT_L: i32 = 225;
const ALT_L: i32 = 226;
const ALT_R: i32 = 230;

#[test]
fn plain_and_shifted_keys() {
    let id = state(KB_US | KB_DEFAULT);
    assert_eq!(tap(&id, A), [KS_a]);
    assert_eq!(tap(&id, ONE), [KS_1]);
    assert_eq!(tap(&id, RET), [KS_Return]);
    down(&id, SHIFT_L);
    assert_eq!(tap(&id, A), [KS_A]);
    assert_eq!(tap(&id, ONE), [KS_exclam]);
    up(&id, SHIFT_L);
    assert_eq!(tap(&id, A), [KS_a]);
    assert_eq!(tap(&id, 1000), [], "a code no layout has");
}

#[test]
fn caps_lock_changes_letters_only() {
    let id = state(KB_US);
    assert_eq!(tap(&id, CAPS), []);
    assert_eq!(id.t_modifiers.get() & MOD_CAPSLOCK, MOD_CAPSLOCK);
    assert_eq!(tap(&id, A), [KS_A]);
    assert_eq!(tap(&id, ONE), [KS_1]);
    down(&id, SHIFT_L);
    assert_eq!(tap(&id, A), [KS_A], "shift and caps: the shifted symbol");
    assert_eq!(tap(&id, ONE), [KS_exclam]);
    up(&id, SHIFT_L);
    assert_eq!(tap(&id, CAPS), []);
    assert_eq!(tap(&id, A), [KS_a]);
}

#[test]
fn control_and_meta() {
    let id = state(KB_US);
    down(&id, CTRL_L);
    assert_eq!(tap(&id, C), [0x03]);
    assert_eq!(tap(&id, TWO), [0x00]);
    assert_eq!(tap(&id, THREE), [KS_Escape]);
    up(&id, CTRL_L);

    down(&id, ALT_L);
    assert_eq!(tap(&id, A), [KS_a | 0x80]);
    up(&id, ALT_L);

    let id = state(KB_US | KB_METAESC);
    down(&id, ALT_L);
    assert_eq!(tap(&id, A), [KS_Escape, KS_a]);
    // All keys up drops the held modifiers.
    assert_eq!(wskbd_translate(&id, WSCONS_EVENT_ALL_KEYS_UP, 0), 0);
    assert_eq!(tap(&id, A), [KS_a]);
}

#[test]
fn num_lock_picks_the_keypad_digit() {
    let id = state(KB_US);
    assert_eq!(tap(&id, KP1), [KS_KP_End]);
    assert_eq!(tap(&id, NUMLOCK), []);
    assert_eq!(tap(&id, KP1), [KS_KP_1]);
    down(&id, SHIFT_L);
    assert_eq!(tap(&id, KP1), [KS_KP_End]);
}

#[test]
fn dead_accent_and_compose() {
    // German: the acute dead key, then e.
    let id = state(KB_DE);
    assert_eq!(tap(&id, Y), [KS_z]);
    assert_eq!(tap(&id, DEAD_ACUTE_DE), []);
    assert_eq!(id.t_composelen.get(), 1);
    // The C passes the type 1 (`WSCONS_EVENT_KEY_UP`) to `update_modifier` when it starts
    // a compose sequence, so `MOD_COMPOSE` (and its LED) stays off: kept as is.
    assert_eq!(id.t_modifiers.get() & MOD_COMPOSE, 0);
    assert_eq!(tap(&id, E), [KS_eacute]);
    assert_eq!(id.t_composelen.get(), 0);

    // US: shifted right Alt is Multi_key; a a composes to @.
    let id = state(KB_US);
    down(&id, SHIFT_L);
    assert_eq!(tap(&id, ALT_R), []);
    up(&id, SHIFT_L);
    assert_eq!(id.t_composelen.get(), 2);
    assert_eq!(tap(&id, A), []);
    assert_eq!(tap(&id, A), [KS_at]);
}

#[test]
fn attached_keyboard_uses_its_map_and_lights_its_leds() {
    let _g = setup_real_memory();
    let sc = keyboard(KB_US | KB_DEFAULT);
    let id = sc.id();
    assert_eq!(sc.sc_maplen.get(), 237);
    assert_eq!(tap(id, A), [KS_a]);
    assert_eq!(
        wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, 237),
        0,
        "out of the map"
    );
    assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, -1), 0);

    assert_eq!(tap(id, CAPS), []);
    assert_eq!(LEDS.load(Ordering::Relaxed), WSKBD_LED_CAPS);
    assert_eq!(tap(id, CAPS), []);
    assert_eq!(LEDS.load(Ordering::Relaxed), 0);
    assert_eq!(tap(id, NUMLOCK), []);
    assert_eq!(LEDS.load(Ordering::Relaxed), WSKBD_LED_NUM);
    assert_eq!(tap(id, NUMLOCK), []);

    // Ctrl_L carries KS_Cmd1, Alt_L KS_Cmd2: both held is command mode, where a key that
    // is no command is swallowed.
    down(id, CTRL_L);
    down(id, ALT_L);
    assert_eq!(
        id.t_modifiers.get() & (MOD_COMMAND1 | MOD_COMMAND2),
        MOD_COMMAND1 | MOD_COMMAND2
    );
    up(id, ALT_L);
    up(id, CTRL_L);
    assert_eq!(id.t_modifiers.get() & (MOD_COMMAND1 | MOD_COMMAND2), 0);
}

#[test]
fn event_mode_queues_the_raw_key_codes() {
    let _g = setup_real_memory();
    let sc = keyboard(KB_US);
    let evar = &sc.sc_base.me_evar;
    wsevent_init(evar).unwrap();
    ENABLED.store(0, Ordering::Relaxed);
    wskbd_do_open(sc, evar).unwrap();
    assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
    assert_eq!(sc.sc_translating.get(), 0);
    assert_eq!(wskbd_do_open(sc, evar), Err(Errno::EBUSY));

    wskbd_input(&sc.sc_base.me_dv, WSCONS_EVENT_KEY_DOWN, A);
    wskbd_input(&sc.sc_base.me_dv, WSCONS_EVENT_KEY_UP, A);
    assert_eq!(evar.ws_put.get(), 2);
    // SAFETY: two events were queued, nobody else writes the ring.
    let q = unsafe { evar.q_events(0, 2) };
    let ev = |i: usize| -> WsconsEvent { ioctl_arg(&q[i * 24..]) };
    assert_eq!((ev(0).type_, ev(0).value), (WSCONS_EVENT_KEY_DOWN, A));
    assert_eq!((ev(1).type_, ev(1).value), (WSCONS_EVENT_KEY_UP, A));
    assert_ne!(ev(0).time, Timespec::default());

    wskbd_mux_close(&sc.sc_base).unwrap();
    assert_eq!(ENABLED.load(Ordering::Relaxed), 0);
    assert_eq!(sc.sc_translating.get(), 1);
    wsevent_fini(evar);
}

#[test]
fn bell_repeat_and_encoding_ioctls() {
    let _g = setup_real_memory();
    let sc = keyboard(KB_US | KB_DEFAULT);
    let ioctl =
        |cmd, data: &mut [u8], flag| wskbd_displayioctl_sc(sc, cmd, data, flag, None, false);

    let mut b = [0u8; size_of::<WskbdBellData>()];
    assert_eq!(ioctl(WSKBDIO_GETBELL, &mut b, FREAD), Ok(true));
    let bell: WskbdBellData = ioctl_arg(&b);
    assert_eq!(
        (bell.which, bell.pitch, bell.period, bell.volume),
        (WSKBD_BELL_DOALL, 400, 100, 50)
    );
    ioctl_ret(
        &mut b,
        &WskbdBellData {
            which: WSKBD_BELL_DOPITCH,
            pitch: 1000,
            period: 1,
            volume: 1,
        },
    );
    assert_eq!(ioctl(WSKBDIO_SETBELL, &mut b, FREAD), Err(Errno::EACCES));
    assert_eq!(ioctl(WSKBDIO_SETBELL, &mut b, FWRITE), Ok(true));
    assert_eq!(ioctl(WSKBDIO_GETBELL, &mut b, FREAD), Ok(true));
    let bell: WskbdBellData = ioctl_arg(&b);
    assert_eq!((bell.pitch, bell.period, bell.volume), (1000, 100, 50));
    assert_eq!(
        ioctl(WSKBDIO_BELL, &mut [], FWRITE),
        Ok(true),
        "the driver rings it"
    );
    assert_eq!(
        ioctl(WSKBDIO_SETDEFAULTBELL, &mut b, FWRITE),
        Err(Errno::EPERM)
    );

    let mut r = [0u8; size_of::<WskbdKeyrepeatData>()];
    assert_eq!(ioctl(WSKBDIO_GETKEYREPEAT, &mut r, FREAD), Ok(true));
    let rep: WskbdKeyrepeatData = ioctl_arg(&r);
    assert_eq!(
        (rep.which, rep.del1, rep.delN),
        (WSKBD_KEYREPEAT_DOALL, 0, 0)
    );
    assert_eq!(ioctl(WSKBDIO_GETDEFAULTKEYREPEAT, &mut r, FREAD), Ok(true));
    let rep: WskbdKeyrepeatData = ioctl_arg(&r);
    assert_eq!((rep.del1, rep.delN), (400, 100));

    let mut e = [0u8; 4];
    assert_eq!(ioctl(WSKBDIO_GETENCODING, &mut e, FREAD), Ok(true));
    assert_eq!(ioctl_arg::<KbdT>(&e), KB_US, "without KB_DEFAULT");
    ioctl_ret(&mut e, &KB_DE);
    assert_eq!(ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE), Ok(true));
    assert_eq!(sc.id().t_layout.get(), KB_DE);
    assert_eq!(tap(sc.id(), Y), [KS_z], "the German map is loaded");
    ioctl_ret(&mut e, &(KB_HU | KB_APPLE));
    assert_eq!(
        ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE),
        Err(Errno::EINVAL)
    );
    ioctl_ret(&mut e, &KB_USER);
    assert_eq!(
        ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE),
        Err(Errno::EINVAL)
    );

    // Not wskbd's and not the driver's.
    assert_eq!(ioctl(0x1234, &mut e, FREAD), Ok(false));
    assert_eq!(
        wskbd_do_ioctl_sc(sc, 0x1234, &mut e, FREAD, None, false),
        Err(Errno::ENOTTY)
    );
    // FIOASYNC needs an open queue.
    assert_eq!(
        wskbd_do_ioctl_sc(sc, FIOASYNC, &mut e, FREAD, None, false),
        Err(Errno::EINVAL)
    );
}

#[test]
fn display_binding() {
    let _g = setup_real_memory();
    let sc = keyboard(KB_US);
    // SAFETY: all-zero is a valid `Device`; leaked for good.
    let disp: &'static Device = Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }));
    ENABLED.store(0, Ordering::Relaxed);

    assert_eq!(
        wskbd_set_display(&sc.sc_base.me_dv, None),
        Err(Errno::ENXIO)
    );
    assert_eq!(wskbd_set_display(&sc.sc_base.me_dv, Some(disp)), Ok(()));
    assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
    assert!(sc.displaydv().is_some_and(|d| ptr::eq(d, disp)));
    assert_eq!(
        wskbd_set_display(&sc.sc_base.me_dv, Some(disp)),
        Err(Errno::EBUSY)
    );
    // A display's keyboard stays on whatever its users do.
    assert_eq!(wskbd_enable(sc, 0), Ok(()));
    assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
    assert_eq!(wskbd_set_display(&sc.sc_base.me_dv, None), Ok(()));
    assert_eq!(ENABLED.load(Ordering::Relaxed), 0);
    assert!(sc.displaydv().is_none());

    sc.sc_isconsole.set(1);
    assert_eq!(
        wskbd_set_display(&sc.sc_base.me_dv, Some(disp)),
        Err(Errno::EBUSY)
    );
}

#[test]
fn attach_locators_match_like_the_c() {
    static CF_ANY: crate::sys::device::Cfdata =
        crate::sys::device::Cfdata::new(&WSKBD_CA, &WSKBD_CD, 0, 0, &[-1, 1], 0, &[], 0, 0);
    static CF_CONS: crate::sys::device::Cfdata =
        crate::sys::device::Cfdata::new(&WSKBD_CA, &WSKBD_CD, 0, 0, &[1, 1], 0, &[], 0, 0);
    static NO_MAP: WskbdMapdata = WskbdMapdata::new(&[], 0);
    let mut a = WskbddevAttachArgs {
        console: 0,
        keymap: &NO_MAP,
        accessops: &FAKE_ACCESSOPS,
        accesscookie: ptr::null_mut(),
        audiocookie: ptr::null_mut(),
    };
    let aux = ptr::from_mut(&mut a).cast();
    assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_ANY), aux), 1);
    assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_CONS), aux), 0);
    a.console = 1;
    let aux = ptr::from_mut(&mut a).cast();
    assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_CONS), aux), 10);
    assert_eq!(wskbddevprint(ptr::null_mut(), Some(b"ukbd0")), UNCONF);
}

/// The defaults and `MOD_*` bits against `wskbd.c`.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn constants_match_the_c() {
    let defs = crate::reftest::defines("sys/dev/wscons/wskbd.c");
    let mut ours = crate::reftest::assert_defines!(defs;
        MOD_SHIFT_L, MOD_SHIFT_R, MOD_SHIFTLOCK, MOD_CAPSLOCK, MOD_CONTROL_L, MOD_CONTROL_R,
        MOD_META_L, MOD_META_R, MOD_MODESHIFT, MOD_NUMLOCK, MOD_COMPOSE, MOD_HOLDSCREEN,
        MOD_COMMAND, MOD_COMMAND1, MOD_COMMAND2, MOD_MODELOCK, MOD_ANYSHIFT, MOD_ANYCONTROL,
        MOD_ANYMETA, WSKBD_DEFAULT_BELL_PITCH, WSKBD_DEFAULT_BELL_PERIOD,
        WSKBD_DEFAULT_BELL_VOLUME, WSKBD_DEFAULT_KEYREPEAT_DEL1, WSKBD_DEFAULT_KEYREPEAT_DELN,
        WSKFL_METAESC, MAXKEYSYMSPERKEY,
    );
    // `MOD_ANYLED` spans two lines (a `\` continuation), which the parser does not read.
    assert_eq!(MOD_ANYLED, 3596);
    ours.push("MOD_ANYLED");
    crate::reftest::assert_complete(&defs, "MOD_", &ours);
}
