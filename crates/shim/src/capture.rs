//! Frame capture for display inside obake (cf. libTAS
//! `screencapture/ScreenCaptureGL.cpp`). The game keeps rendering on the GPU;
//! at each swap the default framebuffer's back buffer is read back with
//! `glReadPixels` and published to a shared frame file the controller reads.
//!
//! Frame file layout (see `obake_protocol::frames`): a 32-byte header then
//! `width * height * 4` BGRA bytes, top row first.

use crate::{log, real};
use obake_protocol::frames::*;
use std::sync::Mutex;

type GetIntegerv = unsafe extern "C" fn(u32, *mut i32);
type BindFramebuffer = unsafe extern "C" fn(u32, u32);
type ReadBuffer = unsafe extern "C" fn(u32);
type PixelStorei = unsafe extern "C" fn(u32, i32);
type ReadPixels = unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut libc::c_void);
type QueryDrawable = unsafe extern "C" fn(*mut libc::c_void, libc::c_ulong, i32, *mut u32);

const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
const GL_READ_BUFFER: u32 = 0x0C02;
const GL_BACK: u32 = 0x0405;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_PACK_ROW_LENGTH: u32 = 0x0D02;
const GL_PIXEL_PACK_BUFFER: u32 = 0x88EB;
const GL_PIXEL_PACK_BUFFER_BINDING: u32 = 0x88ED;
const GL_BGRA: u32 = 0x80E1;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GLX_WIDTH: i32 = 0x801D;
const GLX_HEIGHT: i32 = 0x801E;

struct Gl {
    get_integerv: GetIntegerv,
    bind_framebuffer: Option<BindFramebuffer>,
    bind_buffer: Option<BindFramebuffer>,
    read_buffer: ReadBuffer,
    pixel_storei: PixelStorei,
    read_pixels: ReadPixels,
    query_drawable: QueryDrawable,
}

struct Capture {
    gl: Option<Gl>,
    fd: libc::c_int,
    buf: Vec<u8>,
    seq: u64,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

unsafe fn proc(name: &[u8]) -> *mut libc::c_void {
    real::glx_get_proc_address_arb(name.as_ptr())
}

unsafe fn load_gl() -> Option<Gl> {
    let p = |n: &[u8]| {
        let f = proc(n);
        (!f.is_null()).then_some(f)
    };
    let qd = libc::dlsym(libc::RTLD_NEXT, c"glXQueryDrawable".as_ptr());
    if qd.is_null() {
        return None;
    }
    Some(Gl {
        get_integerv: std::mem::transmute(p(b"glGetIntegerv\0")?),
        bind_framebuffer: p(b"glBindFramebuffer\0").map(|f| std::mem::transmute(f)),
        bind_buffer: p(b"glBindBuffer\0").map(|f| std::mem::transmute(f)),
        read_buffer: std::mem::transmute(p(b"glReadBuffer\0")?),
        pixel_storei: std::mem::transmute(p(b"glPixelStorei\0")?),
        read_pixels: std::mem::transmute(p(b"glReadPixels\0")?),
        query_drawable: std::mem::transmute(qd),
    })
}

/// Reads the back buffer of `drawable` (call right before the real swap) and
/// publishes it. No-op when the controller didn't ask for frames.
/// Returns whether a frame was published.
pub unsafe fn capture_glx(dpy: *mut libc::c_void, drawable: libc::c_ulong, framecount: u64) -> bool {
    let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let Some(path) = crate::env_var(ENV_FRAMES) else { return false };
        let Ok(cpath) = std::ffi::CString::new(path) else { return false };
        let fd = libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC, 0o600);
        if fd < 0 {
            log::warn!("cannot open frame file");
            return false;
        }
        let gl = load_gl();
        if gl.is_none() {
            log::warn!("GL entry points for capture not found; frames won't be shown in obake");
        }
        *guard = Some(Capture { gl, fd, buf: Vec::new(), seq: 0 });
    }
    let c = guard.as_mut().unwrap();
    let Some(gl) = &c.gl else { return false };

    let (mut w, mut h) = (0u32, 0u32);
    (gl.query_drawable)(dpy, drawable, GLX_WIDTH, &mut w);
    (gl.query_drawable)(dpy, drawable, GLX_HEIGHT, &mut h);
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return false;
    }

    // Save the state we touch, read from the default framebuffer's back
    // buffer, then restore.
    let mut read_fb = 0;
    let mut read_buf = 0;
    let mut pack_align = 0;
    let mut pack_row = 0;
    let mut pack_pbo = 0;
    (gl.get_integerv)(GL_READ_FRAMEBUFFER_BINDING, &mut read_fb);
    (gl.get_integerv)(GL_READ_BUFFER, &mut read_buf);
    (gl.get_integerv)(GL_PACK_ALIGNMENT, &mut pack_align);
    (gl.get_integerv)(GL_PACK_ROW_LENGTH, &mut pack_row);
    (gl.get_integerv)(GL_PIXEL_PACK_BUFFER_BINDING, &mut pack_pbo);
    if let Some(b) = gl.bind_framebuffer {
        b(GL_READ_FRAMEBUFFER, 0);
    }
    if let Some(b) = gl.bind_buffer {
        b(GL_PIXEL_PACK_BUFFER, 0);
    }
    (gl.read_buffer)(GL_BACK);
    (gl.pixel_storei)(GL_PACK_ALIGNMENT, 4);
    (gl.pixel_storei)(GL_PACK_ROW_LENGTH, 0);

    let row = w as usize * 4;
    let size = row * h as usize;
    c.buf.resize(HEADER_SIZE + size, 0);
    (gl.read_pixels)(0, 0, w as i32, h as i32, GL_BGRA, GL_UNSIGNED_BYTE, c.buf[HEADER_SIZE..].as_mut_ptr().cast());

    if let Some(b) = gl.bind_framebuffer {
        b(GL_READ_FRAMEBUFFER, read_fb as u32);
    }
    if let Some(b) = gl.bind_buffer {
        b(GL_PIXEL_PACK_BUFFER, pack_pbo as u32);
    }
    (gl.read_buffer)(read_buf as u32);
    (gl.pixel_storei)(GL_PACK_ALIGNMENT, pack_align);
    (gl.pixel_storei)(GL_PACK_ROW_LENGTH, pack_row);

    // GL rows are bottom-up; store top row first.
    let pixels = &mut c.buf[HEADER_SIZE..];
    for y in 0..(h as usize / 2) {
        let (a, b) = pixels.split_at_mut((h as usize - 1 - y) * row);
        a[y * row..y * row + row].swap_with_slice(&mut b[..row]);
    }

    // Seqlock: odd while writing, even when complete.
    c.seq += 1;
    write_header(&mut c.buf[..HEADER_SIZE], c.seq * 2 + 1, w, h, framecount);
    libc::pwrite(c.fd, c.buf.as_ptr().cast(), HEADER_SIZE, 0);
    libc::pwrite(c.fd, c.buf[HEADER_SIZE..].as_ptr().cast(), size, HEADER_SIZE as i64);
    write_header(&mut c.buf[..HEADER_SIZE], c.seq * 2 + 2, w, h, framecount);
    libc::pwrite(c.fd, c.buf.as_ptr().cast(), HEADER_SIZE, 0);
    true
}
