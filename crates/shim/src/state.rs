//! Process-wide shim state (libTAS `Global`, `GlobalState`, `ThreadManager` bits).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

static MAIN_TID: AtomicI32 = AtomicI32::new(0);
static FASTFORWARD: AtomicBool = AtomicBool::new(false);
static EXITING: AtomicBool = AtomicBool::new(false);

thread_local! {
    static OWN_CODE: Cell<u32> = const { Cell::new(0) };
}

pub fn gettid() -> i32 {
    unsafe { libc::syscall(libc::SYS_gettid) as i32 }
}

/// libTAS `ThreadManager::setMainThread`.
pub fn set_main_thread() {
    MAIN_TID.store(gettid(), Ordering::Relaxed);
}

/// libTAS `ThreadManager::isMainThread`.
pub fn is_main_thread() -> bool {
    MAIN_TID.load(Ordering::Relaxed) == gettid()
}

pub fn fastforward() -> bool {
    FASTFORWARD.load(Ordering::Relaxed)
}

pub fn set_fastforward(on: bool) {
    FASTFORWARD.store(on, Ordering::Relaxed);
}

pub fn is_exiting() -> bool {
    EXITING.load(Ordering::Relaxed)
}

pub fn set_exiting() {
    EXITING.store(true, Ordering::Relaxed);
}

/// libTAS `GlobalState::isOwnCode` / `isNative`: true while the shim itself
/// (or a native call it makes on the game's behalf) is running on this thread.
pub fn is_own_code() -> bool {
    OWN_CODE.try_with(|c| c.get() > 0).unwrap_or(true)
}

/// libTAS `NATIVECALL`: run `f` with hooks passing straight through.
pub fn native<R>(f: impl FnOnce() -> R) -> R {
    let _ = OWN_CODE.try_with(|c| c.set(c.get() + 1));
    let r = f();
    let _ = OWN_CODE.try_with(|c| c.set(c.get() - 1));
    r
}
