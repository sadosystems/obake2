//! Port of libTAS `frameBoundary` (src/library/frame.cpp), reduced to what
//! obake needs now: advance the deterministic timer, report the frame to the
//! controller, draw, and block until the controller ends the boundary. Pausing
//! is the controller withholding `MSGN_END_FRAMEBOUNDARY`.

use crate::{det_timer, log, socket, state, x11};
use obake_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// libTAS `framecount`.
pub static FRAMECOUNT: AtomicU64 = AtomicU64::new(0);

/// `draw` performs the real buffer swap; `None` for a non-draw frame
/// (triggered by accumulated sleep delay).
pub fn frame_boundary(draw: Option<&dyn Fn()>) {
    state::set_main_thread();

    // Update the deterministic timer.
    let _time_increment = det_timer::enter_frame_boundary();

    if state::is_exiting() {
        if let Some(d) = draw {
            state::native(d);
        }
        det_timer::exit_frame_boundary(true);
        return;
    }

    let framecount = FRAMECOUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if framecount % 60 == 0 || framecount < 5 {
        let t = det_timer::ticks();
        let r = crate::timeholder::TimeHolder::now();
        log::debug!("frame {framecount} draw={} ticks={}.{:09} real={}.{:03}", draw.is_some(), t.sec, t.nsec, r.sec, r.nsec / 1_000_000);
    }

    // libTAS draws after the controller acknowledges the start of the
    // boundary; with no OSD to composite, drawing first is equivalent and
    // means the paused screen already shows this frame.
    if let Some(d) = draw {
        state::native(d);
    }

    let mut running = true;
    if let Some(conn) = socket::get() {
        let ticks = det_timer::ticks();
        let mut p = Payload::<32>::new();
        p.put_u64(framecount)
            .put_u32(draw.is_some() as u32)
            .put_u64(ticks.sec as u64)
            .put_u64(ticks.nsec as u64);
        if conn.send(MSGB_START_FRAMEBOUNDARY, p.bytes()).is_err() {
            log::warn!("controller went away; exiting");
            unsafe { libc::_exit(1) };
        }
        running = receive_messages(conn);
    }

    det_timer::exit_frame_boundary(running);
}

/// libTAS `receive_messages`: wait for the end of the frame boundary while
/// keeping the game window responsive. Returns the `running` flag.
fn receive_messages(conn: &socket::Conn) -> bool {
    loop {
        match conn.recv_timeout(50) {
            Ok(Some((tag, payload))) => match tag {
                MSGN_END_FRAMEBOUNDARY => {
                    x11::push_keys();
                    let mut r = Reader::new(&payload[..]);
                    let running = r.u32().unwrap_or(1) != 0;
                    state::set_fastforward(r.u32().unwrap_or(0) != 0);
                    return running;
                }
                MSGN_KEY => {
                    let mut r = Reader::new(&payload[..]);
                    if let (Some(keysym), Some(pressed)) = (r.u32(), r.u32()) {
                        x11::queue_key(keysym, pressed != 0);
                    }
                }
                MSGN_USERQUIT => {
                    state::set_exiting();
                    log::debug!("user quit");
                    unsafe { libc::_exit(0) };
                }
                other => log::warn!("unknown message {other}"),
            },
            Ok(None) => {
                // Answer window-manager pings so a paused game isn't flagged
                // as unresponsive (libTAS pushNativeXlibEvents).
                x11::answer_pings();
            }
            Err(()) => {
                log::warn!("controller socket closed; exiting");
                unsafe { libc::_exit(1) };
            }
        }
    }
}
