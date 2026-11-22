//! Port of libTAS `DeterministicTimer` (src/library/DeterministicTimer.cpp).
//!
//! Virtual time advances only at frame boundaries (by exactly one frame
//! length) and through sleeps of the main thread (`add_delay`). Real time is
//! used only to pace frames at the target framerate while running.
//!
//! Not ported yet: per-thread fake ticks for softlocked secondary threads
//! (`startThreadedDelay`/`endThreadedDelay`), the busy-loop detector, and the
//! nondeterministic-timer debug mode.

use crate::state;
use crate::timeholder::TimeHolder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// libTAS `SharedConfig::TimeCallType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum TimeCallType {
    UntrackedRealtime = -2,
    UntrackedMonotonic = -1,
    Time = 0,
    GetTimeOfDay = 1,
    ClockGetTimeRealtime = 3,
    ClockGetTimeMonotonic = 4,
}

pub const NUM_TRACKED: usize = 5;

impl TimeCallType {
    fn is_monotonic(self) -> bool {
        !matches!(
            self,
            TimeCallType::UntrackedRealtime
                | TimeCallType::Time
                | TimeCallType::GetTimeOfDay
                | TimeCallType::ClockGetTimeRealtime
        )
    }

    pub fn from_clock(clk: libc::clockid_t) -> Self {
        match clk {
            libc::CLOCK_REALTIME | libc::CLOCK_REALTIME_ALARM | libc::CLOCK_REALTIME_COARSE | libc::CLOCK_TAI => {
                TimeCallType::ClockGetTimeRealtime
            }
            _ => TimeCallType::ClockGetTimeMonotonic,
        }
    }
}

pub struct DetTimer {
    inited: bool,
    ticks: TimeHolder,
    realtime_delta: TimeHolder,
    added_delay: TimeHolder,
    fake_extra_ticks: TimeHolder,
    base_time_increment: TimeHolder,
    framerate_num: u32,
    framerate_den: u32,
    fractional_increment: u32,
    fractional_part: u32,
    last_enter_time: TimeHolder,
    main_gettimes: [i32; NUM_TRACKED],
    sec_gettimes: [i32; NUM_TRACKED],
    /// libTAS `main_gettimes_threshold` / `sec_gettimes_threshold`; -1 disables.
    pub main_gettimes_threshold: [i32; NUM_TRACKED],
    pub sec_gettimes_threshold: [i32; NUM_TRACKED],
}

static TIMER: Mutex<DetTimer> = Mutex::new(DetTimer {
    inited: false,
    ticks: TimeHolder::ZERO,
    realtime_delta: TimeHolder::ZERO,
    added_delay: TimeHolder::ZERO,
    fake_extra_ticks: TimeHolder::ZERO,
    base_time_increment: TimeHolder::ZERO,
    framerate_num: 0,
    framerate_den: 0,
    fractional_increment: 0,
    fractional_part: 0,
    last_enter_time: TimeHolder::ZERO,
    main_gettimes: [0; NUM_TRACKED],
    sec_gettimes: [0; NUM_TRACKED],
    main_gettimes_threshold: [-1; NUM_TRACKED],
    sec_gettimes_threshold: [-1; NUM_TRACKED],
});

static INSIDE_FRAME_BOUNDARY: AtomicBool = AtomicBool::new(false);

fn lock() -> std::sync::MutexGuard<'static, DetTimer> {
    TIMER.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_inited() -> bool {
    lock().inited
}

pub fn is_inside_frame_boundary() -> bool {
    INSIDE_FRAME_BOUNDARY.load(Ordering::Relaxed)
}

/// libTAS `DeterministicTimer::initialize`.
pub fn initialize(
    monotonic: TimeHolder,
    realtime: TimeHolder,
    framerate: (u32, u32),
    main_threshold: [i32; NUM_TRACKED],
) {
    let mut t = lock();
    t.main_gettimes_threshold = main_threshold;
    t.ticks = monotonic;
    t.realtime_delta = realtime - monotonic;
    t.set_framerate(framerate.0, framerate.1);
    t.last_enter_time = TimeHolder::now();
    t.main_gettimes = [0; NUM_TRACKED];
    t.sec_gettimes = [0; NUM_TRACKED];
    t.added_delay = TimeHolder::ZERO;
    t.fake_extra_ticks = TimeHolder::ZERO;
    t.inited = true;
}

impl DetTimer {
    fn set_framerate(&mut self, num: u32, den: u32) {
        if self.framerate_num != num || self.framerate_den != den {
            self.framerate_num = num;
            self.framerate_den = den;
            self.base_time_increment = TimeHolder::new(
                (den / num) as i64,
                (1_000_000_000u64 * (den % num) as u64 / num as u64) as i64,
            );
            self.fractional_increment = (1_000_000_000u64 * (den % num) as u64 % num as u64) as u32;
            self.fractional_part = 0;
        }
    }
}

/// libTAS `DeterministicTimer::getTicks(type)`.
pub fn get_ticks(ty: TimeCallType) -> TimeHolder {
    let main_t = state::is_main_thread();
    let mut ticks_extra = 0;
    {
        let mut t = lock();
        if !t.inited {
            return TimeHolder::new(1, 0);
        }
        let base = t.ticks + t.fake_extra_ticks;
        match ty {
            TimeCallType::UntrackedMonotonic => return base,
            TimeCallType::UntrackedRealtime => return base + t.realtime_delta,
            _ => {}
        }
        if state::is_own_code() {
            return if ty.is_monotonic() { base } else { base + t.realtime_delta };
        }

        let idx = ty as usize;
        let threshold = if main_t { t.main_gettimes_threshold[idx] } else { t.sec_gettimes_threshold[idx] };
        if !is_inside_frame_boundary() && threshold >= 0 {
            let count = if main_t { &mut t.main_gettimes[idx] } else { &mut t.sec_gettimes[idx] };
            *count += 1;
            if *count > threshold {
                crate::log::debug!("force-advancing time of type {:?}", ty);
                ticks_extra += 1;
                t.main_gettimes = [0; NUM_TRACKED];
                t.sec_gettimes = [0; NUM_TRACKED];
            }
        }
    }

    if ticks_extra != 0 {
        // Delay by ticksExtra ms. Arbitrary (libTAS).
        add_delay(TimeHolder::new(0, ticks_extra * 1_000_000));
    }

    let t = lock();
    let r = t.ticks + t.fake_extra_ticks;
    if ty.is_monotonic() { r } else { r + t.realtime_delta }
}

/// libTAS `DeterministicTimer::addDelay`.
pub fn add_delay(delay: TimeHolder) {
    if state::is_own_code() {
        return;
    }
    let base;
    {
        let mut t = lock();
        t.added_delay += delay;
        t.ticks += delay;
        base = t.base_time_increment;
    }

    if !state::fastforward() {
        // Sleep, because the caller would have yielded at least a little.
        unsafe { crate::real::sched_yield() };
    }

    if state::is_exiting() {
        return;
    }

    // Only the main thread may trigger a frame boundary.
    if state::is_main_thread() && !is_inside_frame_boundary() {
        let max_deferred_delay = base;
        while lock().added_delay > max_deferred_delay {
            // Too much delay built up: enter a non-draw frame boundary to
            // advance time. This decrements addedDelay by one frame.
            crate::frame::frame_boundary(None);
        }
    }
}

/// libTAS `DeterministicTimer::enterFrameBoundary`. Returns the time increment.
pub fn enter_frame_boundary() -> TimeHolder {
    INSIDE_FRAME_BOUNDARY.store(true, Ordering::Relaxed);
    let mut t = lock();
    t.fake_extra_ticks = TimeHolder::ZERO;

    let mut time_increment = t.base_time_increment;
    t.fractional_part += t.fractional_increment;
    while t.fractional_part >= t.framerate_num {
        time_increment.nsec += 1;
        t.fractional_part -= t.framerate_num;
    }
    time_increment.normalize();

    if time_increment > t.added_delay {
        let delta = time_increment - t.added_delay;
        t.ticks += delta;
        t.added_delay = TimeHolder::ZERO;
    } else {
        let d = t.added_delay - time_increment;
        t.added_delay = d;
    }
    time_increment
}

/// libTAS `DeterministicTimer::exitFrameBoundary`: sleep so that the game runs
/// at normal speed when `running`.
pub fn exit_frame_boundary(running: bool) {
    let (desired, threshold) = {
        let mut t = lock();
        t.main_gettimes = [0; NUM_TRACKED];
        t.sec_gettimes = [0; NUM_TRACKED];
        let speed_divisor = 1;
        (
            t.last_enter_time + t.base_time_increment.mul(speed_divisor),
            // Arbitrary threshold (libTAS): beyond 10 frames late, stop catching up.
            t.last_enter_time + t.base_time_increment.mul(speed_divisor * 10),
        )
    };

    if running && !state::fastforward() {
        let current_time = TimeHolder::now();
        let ts = desired.to_ts();
        unsafe {
            crate::real::clock_nanosleep(libc::CLOCK_MONOTONIC, libc::TIMER_ABSTIME, &ts, std::ptr::null_mut())
        };
        let mut t = lock();
        t.last_enter_time = if current_time > threshold { current_time } else { desired };
    } else {
        // Frame advance / paused: next frame is paced from now.
        lock().last_enter_time = TimeHolder::now();
    }

    INSIDE_FRAME_BOUNDARY.store(false, Ordering::Relaxed);
}

/// Current virtual monotonic time without tracking (for reporting).
pub fn ticks() -> TimeHolder {
    get_ticks(TimeCallType::UntrackedMonotonic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framerate_increment() {
        let mut t = DetTimer {
            inited: true,
            ticks: TimeHolder::ZERO,
            realtime_delta: TimeHolder::ZERO,
            added_delay: TimeHolder::ZERO,
            fake_extra_ticks: TimeHolder::ZERO,
            base_time_increment: TimeHolder::ZERO,
            framerate_num: 0,
            framerate_den: 0,
            fractional_increment: 0,
            fractional_part: 0,
            last_enter_time: TimeHolder::ZERO,
            main_gettimes: [0; NUM_TRACKED],
            sec_gettimes: [0; NUM_TRACKED],
            main_gettimes_threshold: [-1; NUM_TRACKED],
            sec_gettimes_threshold: [-1; NUM_TRACKED],
        };
        t.set_framerate(60, 1);
        assert_eq!(t.base_time_increment, TimeHolder::new(0, 16_666_666));
        assert_eq!(t.fractional_increment, 40);
    }
}
