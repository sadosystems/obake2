//! Exported hooks (interposed via LD_PRELOAD). Ports of libTAS
//! `general/timewrappers.cpp`, `general/sleepwrappers.cpp` and
//! `rendering/glxwrappers.cpp` (glXSwapBuffers only).

use crate::det_timer::{self, TimeCallType};
use crate::timeholder::TimeHolder;
use crate::{frame, real, state, x11};
use std::sync::atomic::Ordering;

fn passthrough() -> bool {
    state::is_own_code() || !det_timer::is_inited()
}

// ---------------------------------------------------------------- time

#[no_mangle]
pub unsafe extern "C" fn clock_gettime(clk: libc::clockid_t, tp: *mut libc::timespec) -> libc::c_int {
    if passthrough() {
        return real::clock_gettime(clk, tp);
    }
    if tp.is_null() {
        return 0;
    }
    *tp = det_timer::get_ticks(TimeCallType::from_clock(clk)).to_ts();
    0
}

#[no_mangle]
pub unsafe extern "C" fn gettimeofday(tv: *mut libc::timeval, tz: *mut libc::c_void) -> libc::c_int {
    if passthrough() {
        return real::gettimeofday(tv, tz);
    }
    if !tv.is_null() {
        let t = det_timer::get_ticks(TimeCallType::GetTimeOfDay);
        (*tv).tv_sec = t.sec;
        (*tv).tv_usec = t.nsec / 1000;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn time(t: *mut libc::time_t) -> libc::time_t {
    if passthrough() {
        return real::time(t);
    }
    let s = det_timer::get_ticks(TimeCallType::Time).sec;
    if !t.is_null() {
        *t = s;
    }
    s
}

// ---------------------------------------------------------------- sleep

/// libTAS `transfer_sleep` with `sleep_handling = SLEEP_MAIN` (its default).
unsafe fn transfer_sleep(ts: TimeHolder, rem: *mut libc::timespec) {
    // Skip sleeping when main thread and fast-forwarding.
    if !state::is_main_thread() || !state::fastforward() {
        if ts > TimeHolder::ZERO {
            let r = ts.to_ts();
            real::nanosleep(&r, rem);
        }
    }
    if ts <= TimeHolder::ZERO {
        return;
    }
    if !state::is_main_thread() {
        return;
    }
    det_timer::add_delay(ts);
    real::sched_yield();
}

#[no_mangle]
pub unsafe extern "C" fn nanosleep(req: *const libc::timespec, rem: *mut libc::timespec) -> libc::c_int {
    if passthrough() || req.is_null() {
        return real::nanosleep(req, rem);
    }
    transfer_sleep(TimeHolder::from_ts(&*req), rem);
    0
}

#[no_mangle]
pub unsafe extern "C" fn clock_nanosleep(
    clk: libc::clockid_t,
    flags: libc::c_int,
    req: *const libc::timespec,
    rem: *mut libc::timespec,
) -> libc::c_int {
    if passthrough() || req.is_null() {
        return real::clock_nanosleep(clk, flags, req, rem);
    }
    let mut sleeptime = TimeHolder::from_ts(&*req);
    if flags & libc::TIMER_ABSTIME != 0 {
        let now = det_timer::get_ticks(match TimeCallType::from_clock(clk) {
            TimeCallType::ClockGetTimeRealtime => TimeCallType::UntrackedRealtime,
            _ => TimeCallType::UntrackedMonotonic,
        });
        sleeptime = sleeptime - now;
    }
    transfer_sleep(sleeptime, rem);
    0
}

#[no_mangle]
pub unsafe extern "C" fn usleep(usec: libc::useconds_t) -> libc::c_int {
    let ts = TimeHolder::new((usec / 1_000_000) as i64, ((usec % 1_000_000) * 1000) as i64);
    if passthrough() {
        let r = ts.to_ts();
        return real::nanosleep(&r, std::ptr::null_mut());
    }
    transfer_sleep(ts, std::ptr::null_mut());
    0
}

#[no_mangle]
pub unsafe extern "C" fn sleep(secs: libc::c_uint) -> libc::c_uint {
    let ts = TimeHolder::new(secs as i64, 0);
    if passthrough() {
        let r = ts.to_ts();
        real::nanosleep(&r, std::ptr::null_mut());
        return 0;
    }
    transfer_sleep(ts, std::ptr::null_mut());
    0
}

// ---------------------------------------------------------------- GLX

#[no_mangle]
pub unsafe extern "C" fn glXSwapBuffers(dpy: *mut libc::c_void, drawable: libc::c_ulong) {
    if passthrough() {
        return real::glx_swap_buffers(dpy, drawable);
    }
    x11::GAME_DISPLAY.store(dpy, Ordering::Relaxed);
    x11::GAME_WINDOW.store(drawable as u64, Ordering::Relaxed);
    // Start the frame boundary and pass the function to draw.
    frame::frame_boundary(Some(&|| {
        let captured = crate::capture::capture_glx(dpy, drawable, frame::FRAMECOUNT.load(Ordering::Relaxed));
        // While obake shows the game, its own window is parked off-screen and
        // never needs presenting; presenting there can block indefinitely
        // (NVIDIA PRIME offload), so the readback is the only output.
        if !(captured && hide_window()) {
            real::glx_swap_buffers(dpy, drawable)
        }
    }));
}

// libTAS glXSwapInterval*: under the deterministic timer the shim paces
// frames itself, so vsync is always disabled (it would cap fast-forward at
// the monitor's refresh rate).

unsafe fn real_gl(name: &[u8]) -> *mut libc::c_void {
    let mut p = libc::dlsym(libc::RTLD_NEXT, name.as_ptr().cast());
    if p.is_null() {
        p = real::glx_get_proc_address_arb(name.as_ptr());
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapIntervalEXT(dpy: *mut libc::c_void, drawable: libc::c_ulong, _interval: libc::c_int) {
    let f = real_gl(b"glXSwapIntervalEXT\0");
    if !f.is_null() {
        let f: unsafe extern "C" fn(*mut libc::c_void, libc::c_ulong, libc::c_int) = std::mem::transmute(f);
        f(dpy, drawable, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapIntervalSGI(_interval: libc::c_int) -> libc::c_int {
    let f = real_gl(b"glXSwapIntervalSGI\0");
    if f.is_null() {
        return 0;
    }
    let f: unsafe extern "C" fn(libc::c_int) -> libc::c_int = std::mem::transmute(f);
    f(0)
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapIntervalMESA(_interval: libc::c_uint) -> libc::c_int {
    let f = real_gl(b"glXSwapIntervalMESA\0");
    if f.is_null() {
        return 0;
    }
    let f: unsafe extern "C" fn(libc::c_uint) -> libc::c_int = std::mem::transmute(f);
    f(0)
}

/// Hand out our hooks to games that look GLX entry points up dynamically.
unsafe fn proc_override(name: *const u8) -> Option<*mut libc::c_void> {
    if name.is_null() {
        return None;
    }
    let n = std::ffi::CStr::from_ptr(name.cast()).to_bytes();
    match n {
        b"glXSwapBuffers" => Some(glXSwapBuffers as *mut libc::c_void),
        b"glXSwapIntervalEXT" => Some(glXSwapIntervalEXT as *mut libc::c_void),
        b"glXSwapIntervalSGI" => Some(glXSwapIntervalSGI as *mut libc::c_void),
        b"glXSwapIntervalMESA" => Some(glXSwapIntervalMESA as *mut libc::c_void),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddressARB(name: *const u8) -> *mut libc::c_void {
    proc_override(name).unwrap_or_else(|| real::glx_get_proc_address_arb(name))
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddress(name: *const u8) -> *mut libc::c_void {
    proc_override(name).unwrap_or_else(|| real::glx_get_proc_address(name))
}

// ---------------------------------------------------------------- X input
// Ports of libTAS inputs/xkeyboard.cpp and inputs/xpointer.cpp, reduced: the
// game must never grab the real keyboard/pointer or move focus, or obake's
// own window stops receiving input. libTAS additionally redirects grabs and
// focus inside its emulated event queue; that arrives with input (M2).

const GRAB_SUCCESS: libc::c_int = 0;

#[no_mangle]
pub unsafe extern "C" fn XGrabKeyboard(
    _dpy: *mut libc::c_void,
    _w: libc::c_ulong,
    _owner_events: libc::c_int,
    _pointer_mode: libc::c_int,
    _keyboard_mode: libc::c_int,
    _time: libc::c_ulong,
) -> libc::c_int {
    GRAB_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn XUngrabKeyboard(_dpy: *mut libc::c_void, _time: libc::c_ulong) -> libc::c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn XGrabPointer(
    _dpy: *mut libc::c_void,
    _w: libc::c_ulong,
    _owner_events: libc::c_int,
    _event_mask: libc::c_uint,
    _pointer_mode: libc::c_int,
    _keyboard_mode: libc::c_int,
    _confine_to: libc::c_ulong,
    _cursor: libc::c_ulong,
    _time: libc::c_ulong,
) -> libc::c_int {
    GRAB_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn XUngrabPointer(_dpy: *mut libc::c_void, _time: libc::c_ulong) -> libc::c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn XSetInputFocus(
    _dpy: *mut libc::c_void,
    _focus: libc::c_ulong,
    _revert_to: libc::c_int,
    _time: libc::c_ulong,
) -> libc::c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn XWarpPointer(
    _dpy: *mut libc::c_void,
    _src_w: libc::c_ulong,
    _dest_w: libc::c_ulong,
    _src_x: libc::c_int,
    _src_y: libc::c_int,
    _src_width: libc::c_uint,
    _src_height: libc::c_uint,
    _dest_x: libc::c_int,
    _dest_y: libc::c_int,
) -> libc::c_int {
    0
}

// The game's top-level window is mapped (drivers only track the size of
// mapped windows; NVIDIA otherwise keeps the creation-size back buffer) but
// unmanaged (override-redirect) and parked off-screen: obake shows the frames
// and feeds input itself. OBAKE_SHOW_WINDOW leaves the window alone.

const OFFSCREEN: libc::c_int = -30000;
const CW_OVERRIDE_REDIRECT: libc::c_ulong = 1 << 9;
const CW_X: libc::c_uint = 1 << 0;
const CW_Y: libc::c_uint = 1 << 1;

fn hide_window() -> bool {
    crate::env_var(obake_protocol::ENV_SHOW_WINDOW).is_none()
}

/// Top-level windows the game created (parked off-screen).
static PARKED: std::sync::Mutex<Vec<libc::c_ulong>> = std::sync::Mutex::new(Vec::new());

fn is_parked(w: libc::c_ulong) -> bool {
    PARKED.lock().unwrap_or_else(|e| e.into_inner()).contains(&w)
}

unsafe fn next(name: &[u8]) -> *mut libc::c_void {
    let p = libc::dlsym(libc::RTLD_NEXT, name.as_ptr().cast());
    if p.is_null() {
        crate::log::fatal("cannot resolve an Xlib function");
    }
    p
}

/// `XSetWindowAttributes` (112 bytes); `override_redirect` is at offset 88.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSetWindowAttributes {
    words: [u64; 14],
}

#[no_mangle]
pub unsafe extern "C" fn XCreateWindow(
    dpy: *mut libc::c_void,
    parent: libc::c_ulong,
    x: libc::c_int,
    y: libc::c_int,
    width: libc::c_uint,
    height: libc::c_uint,
    border_width: libc::c_uint,
    depth: libc::c_int,
    class: libc::c_uint,
    visual: *mut libc::c_void,
    valuemask: libc::c_ulong,
    attributes: *mut XSetWindowAttributes,
) -> libc::c_ulong {
    type F = unsafe extern "C" fn(
        *mut libc::c_void, libc::c_ulong, libc::c_int, libc::c_int, libc::c_uint, libc::c_uint,
        libc::c_uint, libc::c_int, libc::c_uint, *mut libc::c_void, libc::c_ulong,
        *mut XSetWindowAttributes,
    ) -> libc::c_ulong;
    let real: F = std::mem::transmute(next(b"XCreateWindow\0"));
    let root: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_ulong =
        std::mem::transmute(next(b"XDefaultRootWindow\0"));
    if !hide_window() || parent != root(dpy) {
        return real(dpy, parent, x, y, width, height, border_width, depth, class, visual, valuemask, attributes);
    }
    let mut attrs = if attributes.is_null() { XSetWindowAttributes { words: [0; 14] } } else { *attributes };
    // override_redirect = True (a Bool at byte offset 88, i.e. word 11).
    attrs.words[11] = 1;
    let w = real(
        dpy, parent, OFFSCREEN, OFFSCREEN, width, height, border_width, depth, class, visual,
        valuemask | CW_OVERRIDE_REDIRECT, &mut attrs,
    );
    PARKED.lock().unwrap_or_else(|e| e.into_inner()).push(w);
    w
}

#[no_mangle]
pub unsafe extern "C" fn XMoveWindow(dpy: *mut libc::c_void, w: libc::c_ulong, x: libc::c_int, y: libc::c_int) -> libc::c_int {
    if is_parked(w) {
        return 0;
    }
    let real: unsafe extern "C" fn(*mut libc::c_void, libc::c_ulong, libc::c_int, libc::c_int) -> libc::c_int =
        std::mem::transmute(next(b"XMoveWindow\0"));
    real(dpy, w, x, y)
}

#[no_mangle]
pub unsafe extern "C" fn XConfigureWindow(
    dpy: *mut libc::c_void,
    w: libc::c_ulong,
    mut mask: libc::c_uint,
    changes: *mut libc::c_void,
) -> libc::c_int {
    if is_parked(w) {
        mask &= !(CW_X | CW_Y);
    }
    let real: unsafe extern "C" fn(*mut libc::c_void, libc::c_ulong, libc::c_uint, *mut libc::c_void) -> libc::c_int =
        std::mem::transmute(next(b"XConfigureWindow\0"));
    real(dpy, w, mask, changes)
}
