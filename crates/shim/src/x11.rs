//! Minimal Xlib use from inside the shim: answering `_NET_WM_PING` while the
//! game's main thread is parked in a frame boundary. Xlib is resolved at
//! runtime from the game's own libX11 (no link-time dependency).

use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;

/// The Display the game last swapped buffers on.
pub static GAME_DISPLAY: AtomicPtr<libc::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The window the game last swapped (its GLX drawable).
pub static GAME_WINDOW: AtomicU64 = AtomicU64::new(0);

/// Keys received during this frame boundary, injected when it ends.
static PENDING_KEYS: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());

type Display = libc::c_void;
type Atom = libc::c_ulong;
type Window = libc::c_ulong;

const CLIENT_MESSAGE: libc::c_int = 33;
const SUBSTRUCTURE_NOTIFY_MASK: libc::c_long = 1 << 19;
const SUBSTRUCTURE_REDIRECT_MASK: libc::c_long = 1 << 20;

/// `XEvent` is a union of 24 longs.
#[repr(C)]
#[derive(Clone, Copy)]
struct XEvent {
    pad: [libc::c_long; 24],
}

/// Field view of `XClientMessageEvent`.
#[repr(C)]
struct XClientMessageEvent {
    ty: libc::c_int,
    serial: libc::c_ulong,
    send_event: libc::c_int,
    display: *mut Display,
    window: Window,
    message_type: Atom,
    format: libc::c_int,
    data: [libc::c_long; 5],
}

unsafe fn sym(name: &[u8]) -> *mut libc::c_void {
    libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr().cast())
}

pub fn answer_pings() {
    let dpy = GAME_DISPLAY.load(Ordering::Relaxed);
    if dpy.is_null() {
        return;
    }
    unsafe {
        let check = sym(b"XCheckTypedEvent\0");
        let put_back = sym(b"XPutBackEvent\0");
        let send = sym(b"XSendEvent\0");
        let intern = sym(b"XInternAtom\0");
        let root = sym(b"XDefaultRootWindow\0");
        let flush = sym(b"XFlush\0");
        if [check, put_back, send, intern, root, flush].iter().any(|p| p.is_null()) {
            return;
        }
        let check: unsafe extern "C" fn(*mut Display, libc::c_int, *mut XEvent) -> libc::c_int = std::mem::transmute(check);
        let put_back: unsafe extern "C" fn(*mut Display, *mut XEvent) -> libc::c_int = std::mem::transmute(put_back);
        let send: unsafe extern "C" fn(*mut Display, Window, libc::c_int, libc::c_long, *mut XEvent) -> libc::c_int =
            std::mem::transmute(send);
        let intern: unsafe extern "C" fn(*mut Display, *const libc::c_char, libc::c_int) -> Atom = std::mem::transmute(intern);
        let root: unsafe extern "C" fn(*mut Display) -> Window = std::mem::transmute(root);
        let flush: unsafe extern "C" fn(*mut Display) -> libc::c_int = std::mem::transmute(flush);

        let wm_protocols = intern(dpy, c"WM_PROTOCOLS".as_ptr(), 1);
        let net_wm_ping = intern(dpy, c"_NET_WM_PING".as_ptr(), 1);
        let root_win = root(dpy);

        // Pull every ClientMessage, answer pings, and give the rest back to
        // the game in their original order.
        let mut kept: [XEvent; 32] = [XEvent { pad: [0; 24] }; 32];
        let mut nkept = 0;
        let mut ev = XEvent { pad: [0; 24] };
        while nkept < kept.len() && check(dpy, CLIENT_MESSAGE, &mut ev) != 0 {
            let cm = &mut *(&mut ev as *mut XEvent as *mut XClientMessageEvent);
            if wm_protocols != 0 && cm.message_type == wm_protocols && cm.data[0] as Atom == net_wm_ping {
                cm.window = root_win;
                send(dpy, root_win, 0, SUBSTRUCTURE_NOTIFY_MASK | SUBSTRUCTURE_REDIRECT_MASK, &mut ev);
            } else {
                kept[nkept] = ev;
                nkept += 1;
            }
        }
        for e in kept[..nkept].iter_mut().rev() {
            put_back(dpy, e);
        }
        flush(dpy);
    }
}

pub fn queue_key(keysym: u32, pressed: bool) {
    PENDING_KEYS.lock().unwrap_or_else(|e| e.into_inner()).push((keysym, pressed));
}

/// `XKeyEvent` field view.
#[repr(C)]
struct XKeyEvent {
    ty: libc::c_int,
    serial: libc::c_ulong,
    send_event: libc::c_int,
    display: *mut Display,
    window: Window,
    root: Window,
    subwindow: Window,
    time: libc::c_ulong,
    x: libc::c_int,
    y: libc::c_int,
    x_root: libc::c_int,
    y_root: libc::c_int,
    state: libc::c_uint,
    keycode: libc::c_uint,
    same_screen: libc::c_int,
}

const KEY_PRESS: libc::c_int = 2;
const KEY_RELEASE: libc::c_int = 3;

/// Inserts the queued key events into the game's Xlib event queue, in order,
/// ahead of anything already queued (libTAS generateInputEvents, reduced).
/// Runs on the game's main thread inside the frame boundary.
pub fn push_keys() {
    let keys: Vec<(u32, bool)> = std::mem::take(&mut *PENDING_KEYS.lock().unwrap_or_else(|e| e.into_inner()));
    if keys.is_empty() {
        return;
    }
    let dpy = GAME_DISPLAY.load(Ordering::Relaxed);
    let win = GAME_WINDOW.load(Ordering::Relaxed) as Window;
    if dpy.is_null() || win == 0 {
        return;
    }
    unsafe {
        let put_back = sym(b"XPutBackEvent\0");
        let to_code = sym(b"XKeysymToKeycode\0");
        let root = sym(b"XDefaultRootWindow\0");
        if put_back.is_null() || to_code.is_null() || root.is_null() {
            return;
        }
        let put_back: unsafe extern "C" fn(*mut Display, *mut XEvent) -> libc::c_int = std::mem::transmute(put_back);
        let to_code: unsafe extern "C" fn(*mut Display, libc::c_ulong) -> u8 = std::mem::transmute(to_code);
        let root: unsafe extern "C" fn(*mut Display) -> Window = std::mem::transmute(root);
        let root_win = root(dpy);
        let time = crate::det_timer::ticks();
        let time_ms = (time.sec * 1000 + time.nsec / 1_000_000) as libc::c_ulong;
        // XPutBackEvent prepends, so insert in reverse.
        for &(keysym, pressed) in keys.iter().rev() {
            let code = to_code(dpy, keysym as libc::c_ulong);
            if code == 0 {
                continue;
            }
            let mut ev = XEvent { pad: [0; 24] };
            let k = &mut *(&mut ev as *mut XEvent as *mut XKeyEvent);
            k.ty = if pressed { KEY_PRESS } else { KEY_RELEASE };
            k.display = dpy;
            k.window = win;
            k.root = root_win;
            k.time = time_ms;
            k.keycode = code as libc::c_uint;
            k.same_screen = 1;
            put_back(dpy, &mut ev);
        }
    }
}
