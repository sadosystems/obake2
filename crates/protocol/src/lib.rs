//! Shim <-> controller wire protocol.
//!
//! One Unix stream socket per game run. Every message is a fixed 8-byte header
//! (`tag: u32`, `len: u32`, little endian) followed by `len` payload bytes.
//! Payload fields are little-endian integers in the order documented on each
//! tag. The first message on a connection is always `MSGB_HELLO` from the shim,
//! carrying `PROTOCOL_VERSION`; the controller refuses a mismatched version.
//!
//! Naming follows libTAS (`shared/messages.h`): `MSGB_*` go from the game
//! (shim) to the program (controller), `MSGN_*` go from the program to the game.
//!
//! Deliberately `std`-only and allocation-light so the shim can use it from
//! inside hooked libc calls.

/// Bumped on any incompatible change to tags or payload layouts.
pub const PROTOCOL_VERSION: u32 = 1;

/// Environment variable carrying the controller socket path into the game.
pub const ENV_SOCKET: &str = "OBAKE_SOCKET";
/// Environment variable carrying the initial framerate as `num/den` (default `60/1`).
pub const ENV_FRAMERATE: &str = "OBAKE_FRAMERATE";
/// Main-thread `clock_gettime` calls per frame before time is force-advanced
/// by 1ms (libTAS time tracking). Default 100; -1 disables.
pub const ENV_TIME_TRACKING: &str = "OBAKE_TIME_TRACKING";
/// Set to any value to let the game map its own window (by default only obake
/// shows it).
pub const ENV_SHOW_WINDOW: &str = "OBAKE_SHOW_WINDOW";
/// Set to any value to make the shim log to stderr.
pub const ENV_LOG: &str = "OBAKE_LOG";

// ---- shim -> controller ----

/// `version: u32, pid: u32`
pub const MSGB_HELLO: u32 = 1;
/// Start of a frame boundary.
/// `framecount: u64, draw: u32 (1 = frame was drawn, 0 = non-draw frame),
///  monotonic_sec: u64, monotonic_nsec: u64` (virtual time after this frame).
/// The shim then blocks until it receives `MSGN_END_FRAMEBOUNDARY`.
pub const MSGB_START_FRAMEBOUNDARY: u32 = 2;
/// Hooked call that has no model yet and was passed through. `name: utf8 bytes`.
pub const MSGB_UNHOOKED: u32 = 3;

// ---- controller -> shim ----

/// Ends the current frame boundary; the game runs until the next one.
/// `running: u32, fastforward: u32` (fastforward 1 = never sleep to pace).
/// `running` (1 = the shim sleeps to pace frames at real-time speed, 0 = don't
/// sleep to pace this frame: frame advance, or the controller paced it).
pub const MSGN_END_FRAMEBOUNDARY: u32 = 101;
/// The game must exit now.
pub const MSGN_USERQUIT: u32 = 102;
/// Keyboard input for the coming frame, sent before `MSGN_END_FRAMEBOUNDARY`.
/// `keysym: u32 (X11 keysym), pressed: u32 (1 = press, 0 = release)`.
pub const MSGN_KEY: u32 = 103;

pub const HEADER_LEN: usize = 8;

/// Shared frame file: the shim publishes each drawn frame here so obake can
/// show the game inside its own window.
pub mod frames {
    /// Environment variable carrying the frame file path into the game.
    pub const ENV_FRAMES: &str = "OBAKE_FRAMES";
    pub const MAGIC: u32 = 0x4f42_4b46; // "OBKF"
    /// `magic: u32, version: u32, seq: u64 (odd while being written),
    ///  width: u32, height: u32, framecount: u64`, then BGRA pixels, top row first.
    pub const HEADER_SIZE: usize = 32;

    pub fn write_header(h: &mut [u8], seq: u64, w: u32, height: u32, framecount: u64) {
        h[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        h[4..8].copy_from_slice(&1u32.to_le_bytes());
        h[8..16].copy_from_slice(&seq.to_le_bytes());
        h[16..20].copy_from_slice(&w.to_le_bytes());
        h[20..24].copy_from_slice(&height.to_le_bytes());
        h[24..32].copy_from_slice(&framecount.to_le_bytes());
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Header {
        pub seq: u64,
        pub width: u32,
        pub height: u32,
        pub framecount: u64,
    }

    pub fn read_header(h: &[u8]) -> Option<Header> {
        if h.len() < HEADER_SIZE || u32::from_le_bytes(h[0..4].try_into().ok()?) != MAGIC {
            return None;
        }
        Some(Header {
            seq: u64::from_le_bytes(h[8..16].try_into().ok()?),
            width: u32::from_le_bytes(h[16..20].try_into().ok()?),
            height: u32::from_le_bytes(h[20..24].try_into().ok()?),
            framecount: u64::from_le_bytes(h[24..32].try_into().ok()?),
        })
    }
}

pub fn encode_header(tag: u32, len: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&tag.to_le_bytes());
    h[4..8].copy_from_slice(&len.to_le_bytes());
    h
}

pub fn decode_header(h: &[u8; HEADER_LEN]) -> (u32, u32) {
    (
        u32::from_le_bytes([h[0], h[1], h[2], h[3]]),
        u32::from_le_bytes([h[4], h[5], h[6], h[7]]),
    )
}

/// Fixed-capacity payload writer (no heap), used by the shim.
pub struct Payload<const N: usize> {
    pub buf: [u8; N],
    pub len: usize,
}

impl<const N: usize> Payload<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }
    pub fn put_u32(&mut self, v: u32) -> &mut Self {
        self.put_bytes(&v.to_le_bytes())
    }
    pub fn put_u64(&mut self, v: u64) -> &mut Self {
        self.put_bytes(&v.to_le_bytes())
    }
    pub fn put_bytes(&mut self, b: &[u8]) -> &mut Self {
        let n = b.len().min(N - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
        self
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// Sequential little-endian reader over a received payload.
pub struct Reader<'a> {
    b: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b }
    }
    pub fn u32(&mut self) -> Option<u32> {
        let (h, t) = self.b.split_first_chunk::<4>()?;
        self.b = t;
        Some(u32::from_le_bytes(*h))
    }
    pub fn u64(&mut self) -> Option<u64> {
        let (h, t) = self.b.split_first_chunk::<8>()?;
        self.b = t;
        Some(u64::from_le_bytes(*h))
    }
    pub fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.b)
    }
}

/// Parses `num/den` (or a bare `num`) into a framerate, rejecting zeros.
pub fn parse_framerate(s: &str) -> Option<(u32, u32)> {
    let (n, d) = match s.split_once('/') {
        Some((n, d)) => (n.trim().parse().ok()?, d.trim().parse().ok()?),
        None => (s.trim().parse().ok()?, 1),
    };
    (n != 0 && d != 0).then_some((n, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = encode_header(MSGB_START_FRAMEBOUNDARY, 28);
        assert_eq!(decode_header(&h), (MSGB_START_FRAMEBOUNDARY, 28));
    }

    #[test]
    fn payload_reader_roundtrip() {
        let mut p = Payload::<32>::new();
        p.put_u64(7).put_u32(1).put_u64(u64::MAX);
        let mut r = Reader::new(p.bytes());
        assert_eq!(r.u64(), Some(7));
        assert_eq!(r.u32(), Some(1));
        assert_eq!(r.u64(), Some(u64::MAX));
        assert_eq!(r.u32(), None);
    }

    #[test]
    fn framerate() {
        assert_eq!(parse_framerate("60"), Some((60, 1)));
        assert_eq!(parse_framerate("30000/1001"), Some((30000, 1001)));
        assert_eq!(parse_framerate("0/1"), None);
    }
}
