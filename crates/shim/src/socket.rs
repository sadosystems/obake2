//! Connection to the controller (libTAS `sockethelpers`). Opened lazily on the
//! first frame boundary so that wrapper processes (e.g. a launch script that
//! later `exec`s the game) never claim the socket.

use crate::log;
use obake_protocol::*;
use std::sync::OnceLock;

pub struct Conn {
    fd: libc::c_int,
    pid: libc::pid_t,
}

static CONN: OnceLock<Option<Conn>> = OnceLock::new();

/// The controller connection for this process, if any.
pub fn get() -> Option<&'static Conn> {
    let c = CONN.get_or_init(connect).as_ref()?;
    // A forked child shares the fd but must not talk on it.
    (c.pid == unsafe { libc::getpid() }).then_some(c)
}

fn connect() -> Option<Conn> {
    let path = crate::env_var(ENV_SOCKET)?;
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            log::fatal("socket() failed");
        }
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as _;
        let bytes = path.as_bytes();
        if bytes.len() >= addr.sun_path.len() {
            log::fatal("socket path too long");
        }
        for (i, b) in bytes.iter().enumerate() {
            addr.sun_path[i] = *b as _;
        }
        let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        if libc::connect(fd, (&addr as *const libc::sockaddr_un).cast(), len) != 0 {
            log::fatal("cannot connect to the obake controller socket");
        }
        let conn = Conn { fd, pid: libc::getpid() };
        let mut p = Payload::<8>::new();
        p.put_u32(PROTOCOL_VERSION).put_u32(conn.pid as u32);
        if conn.send(MSGB_HELLO, p.bytes()).is_err() {
            log::fatal("hello to controller failed");
        }
        log::debug!("connected to controller");
        Some(conn)
    }
}

impl Conn {
    pub fn send(&self, tag: u32, payload: &[u8]) -> Result<(), ()> {
        self.write_all(&encode_header(tag, payload.len() as u32))?;
        self.write_all(payload)
    }

    fn write_all(&self, mut b: &[u8]) -> Result<(), ()> {
        while !b.is_empty() {
            let n = unsafe { libc::send(self.fd, b.as_ptr().cast(), b.len(), libc::MSG_NOSIGNAL) };
            if n < 0 {
                if errno() == libc::EINTR {
                    continue;
                }
                return Err(());
            }
            b = &b[n as usize..];
        }
        Ok(())
    }

    fn read_exact(&self, mut b: &mut [u8]) -> Result<(), ()> {
        while !b.is_empty() {
            let n = unsafe { libc::recv(self.fd, b.as_mut_ptr().cast(), b.len(), 0) };
            if n == 0 {
                return Err(());
            }
            if n < 0 {
                if errno() == libc::EINTR {
                    continue;
                }
                return Err(());
            }
            b = &mut b[n as usize..];
        }
        Ok(())
    }

    /// Waits up to `timeout_ms` for a message. `Ok(None)` on timeout,
    /// `Err` when the controller has gone away.
    pub fn recv_timeout(&self, timeout_ms: i32) -> Result<Option<(u32, Vec<u8>)>, ()> {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            return if errno() == libc::EINTR { Ok(None) } else { Err(()) };
        }
        if r == 0 {
            return Ok(None);
        }
        let mut h = [0u8; HEADER_LEN];
        self.read_exact(&mut h)?;
        let (tag, len) = decode_header(&h);
        let mut payload = vec![0u8; len as usize];
        self.read_exact(&mut payload)?;
        Ok(Some((tag, payload)))
    }
}

fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
