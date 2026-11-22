//! Pointers to the real (next-in-chain) implementations of hooked functions,
//! resolved lazily with `dlsym(RTLD_NEXT, ...)` (libTAS `LINK_NAMESPACE`).
//!
//! The shim must never call a hooked function through the `libc` crate: inside
//! this cdylib those names resolve to our own hooks. Always go through here.

use std::sync::atomic::{AtomicPtr, Ordering};

macro_rules! real_fn {
    ($name:ident, $sym:literal, fn($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        #[allow(clippy::missing_safety_doc)]
        pub unsafe fn $name($($arg: $ty),*) -> $ret {
            static PTR: AtomicPtr<libc::c_void> = AtomicPtr::new(std::ptr::null_mut());
            let mut p = PTR.load(Ordering::Relaxed);
            if p.is_null() {
                p = libc::dlsym(libc::RTLD_NEXT, concat!($sym, "\0").as_ptr().cast());
                if p.is_null() {
                    crate::log::fatal(concat!("cannot resolve real ", $sym));
                }
                PTR.store(p, Ordering::Relaxed);
            }
            let f: unsafe extern "C" fn($($ty),*) -> $ret = std::mem::transmute(p);
            f($($arg),*)
        }
    };
}

real_fn!(clock_gettime, "clock_gettime", fn(clk: libc::clockid_t, tp: *mut libc::timespec) -> libc::c_int);
real_fn!(gettimeofday, "gettimeofday", fn(tv: *mut libc::timeval, tz: *mut libc::c_void) -> libc::c_int);
real_fn!(time, "time", fn(t: *mut libc::time_t) -> libc::time_t);
real_fn!(nanosleep, "nanosleep", fn(req: *const libc::timespec, rem: *mut libc::timespec) -> libc::c_int);
real_fn!(clock_nanosleep, "clock_nanosleep", fn(clk: libc::clockid_t, flags: libc::c_int, req: *const libc::timespec, rem: *mut libc::timespec) -> libc::c_int);
real_fn!(sched_yield, "sched_yield", fn() -> libc::c_int);
real_fn!(glx_swap_buffers, "glXSwapBuffers", fn(dpy: *mut libc::c_void, drawable: libc::c_ulong) -> ());
real_fn!(glx_get_proc_address_arb, "glXGetProcAddressARB", fn(name: *const u8) -> *mut libc::c_void);
real_fn!(glx_get_proc_address, "glXGetProcAddress", fn(name: *const u8) -> *mut libc::c_void);

