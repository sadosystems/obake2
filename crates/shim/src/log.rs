//! Heap-free stderr logging, safe to use inside hooks.

use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};

pub static ENABLED: AtomicBool = AtomicBool::new(false);

struct StackBuf {
    buf: [u8; 512],
    len: usize,
}

impl Write for StackBuf {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

pub fn write_args(args: std::fmt::Arguments) {
    let mut b = StackBuf { buf: [0; 512], len: 0 };
    let _ = write!(b, "[obake-shim {}] ", unsafe { libc::getpid() });
    let _ = b.write_fmt(args);
    let _ = b.write_str("\n");
    unsafe { libc::write(2, b.buf.as_ptr().cast(), b.len) };
}

macro_rules! debug {
    ($($t:tt)*) => {
        if $crate::log::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            $crate::log::write_args(format_args!($($t)*));
        }
    };
}
pub(crate) use debug;

macro_rules! warn_ {
    ($($t:tt)*) => { $crate::log::write_args(format_args!($($t)*)) };
}
pub(crate) use warn_ as warn;

pub fn fatal(msg: &str) -> ! {
    write_args(format_args!("fatal: {msg}"));
    unsafe { libc::_exit(1) }
}

pub fn init() {
    ENABLED.store(crate::env_var(obake_protocol::ENV_LOG).is_some(), Ordering::Relaxed);
}
