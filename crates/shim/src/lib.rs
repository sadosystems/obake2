//! obake shim: LD_PRELOADed into each game process. "Crust": a near
//! line-for-line port of the relevant parts of libTAS's `src/library`.
//!
//! Core models live in `det_timer`/`frame`; API hook modules in `hooks`.
//! The shim works without a controller too (virtual time, real-time pacing).
//!
//! Derived from libTAS, Copyright 2015-2026 Clément Gallet, GPLv3+.

#![allow(clippy::missing_safety_doc)]

mod capture;
mod det_timer;
mod frame;
mod hooks;
mod log;
mod real;
mod socket;
mod state;
mod timeholder;
mod x11;

use timeholder::TimeHolder;

pub(crate) fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// libTAS `__attribute__((constructor)) init()`.
extern "C" fn init() {
    log::init();
    state::set_main_thread();

    let framerate = env_var(obake_protocol::ENV_FRAMERATE)
        .and_then(|s| obake_protocol::parse_framerate(&s))
        .unwrap_or((60, 1));
    // Time tracking (libTAS `main_gettimes_threshold`): after this many
    // main-thread calls of one kind within a frame, time advances by 1ms.
    // libTAS disables it by default; GameMaker's runner busy-waits on
    // clock_gettime(CLOCK_MONOTONIC) at startup and softlocks without it.
    let threshold = env_var(obake_protocol::ENV_TIME_TRACKING)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(100);
    let mut main_threshold = [-1; det_timer::NUM_TRACKED];
    main_threshold[det_timer::TimeCallType::ClockGetTimeMonotonic as usize] = threshold;
    main_threshold[det_timer::TimeCallType::ClockGetTimeRealtime as usize] = threshold;
    // libTAS SharedConfig defaults: initial_monotonic_time = initial_time = 1s.
    det_timer::initialize(TimeHolder::new(1, 0), TimeHolder::new(1, 0), framerate, main_threshold);
    state::set_fastforward(env_var("OBAKE_FASTFORWARD").is_some());
    log::debug!("initialized, framerate {}/{}", framerate.0, framerate.1);
}

#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = init;
