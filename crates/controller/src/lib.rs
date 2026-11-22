//! obake controller: the only owner of run behavior. The CLI, the MCP server
//! and the GUI are all clients of this crate.
//!
//! A [`Run`] is one game process launched with the shim preloaded and a Unix
//! socket to talk to it. The shim reports every frame boundary and blocks until
//! the controller ends it, so pausing is the controller withholding that reply
//! (libTAS's model: the game parks inside its own buffer swap).

pub mod movie;

pub use movie::{FrameInputs, Movie, MovieSpec, OnMovieEnd, Recording};
use obake_protocol::*;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const SHIM_FILE: &str = "libobake_shim.so";
const BUILD_HINT: &str = "build it with `cargo build --release -p obake-shim` in the obake2 repo, or set OBAKE_SHIM";

/// Locates the shim library: `$OBAKE_SHIM`, next to the current executable,
/// then the obake2 repo's `target/{release,debug}`.
pub fn find_shim() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("OBAKE_SHIM") {
        let p = PathBuf::from(p);
        return if p.is_file() { Ok(p) } else { Err(format!("OBAKE_SHIM={} does not exist", p.display())) };
    }
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(SHIM_FILE));
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    candidates.push(repo.join("target/release").join(SHIM_FILE));
    candidates.push(repo.join("target/debug").join(SHIM_FILE));
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.canonicalize().unwrap_or(p))
        .ok_or_else(|| format!("{SHIM_FILE} not found; {BUILD_HINT}"))
}

/// What to launch.
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// `num/den` frames per second of virtual time. Playing a movie uses
    /// the movie's framerate instead.
    pub framerate: (u32, u32),
    pub movie: MovieSpec,
}

impl LaunchSpec {
    /// Launch a game binary the way it expects to be started from its own
    /// folder: working directory is the binary's directory, and a sibling
    /// `libs/` directory (bundled shared libraries) is put on the library path.
    pub fn for_binary(program: impl Into<PathBuf>) -> Self {
        let program: PathBuf = program.into();
        let program = program.canonicalize().unwrap_or(program);
        let cwd = program.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let mut env = Vec::new();
        let libs = cwd.join("libs");
        if libs.is_dir() {
            let mut v = libs.display().to_string();
            if let Ok(old) = std::env::var("LD_LIBRARY_PATH") {
                if !old.is_empty() {
                    v = format!("{v}:{old}");
                }
            }
            env.push(("LD_LIBRARY_PATH".into(), v));
        }
        // Render on the discrete NVIDIA GPU when there is one (PRIME render
        // offload). The integrated GPU caps Downwell below 60fps.
        // OBAKE_GPU=integrated keeps the default GPU.
        let integrated = std::env::var("OBAKE_GPU").is_ok_and(|v| v == "integrated");
        if !integrated && Path::new("/proc/driver/nvidia/version").exists() {
            env.push(("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()));
            env.push(("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()));
            env.push(("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()));
        }
        Self { program, args: Vec::new(), cwd, env, framerate: (60, 1), movie: MovieSpec::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Process started, no frame reached yet.
    Starting,
    Running,
    /// Parked in a frame boundary.
    Paused,
    /// Process ended (exit code, or `None` if killed by a signal).
    Exited(Option<i32>),
    /// Something went wrong talking to the game.
    Failed,
}

#[derive(Clone, Debug)]
pub struct RunStatus {
    pub state: RunState,
    /// Frames completed (libTAS `framecount`), including non-draw frames.
    pub frame: u64,
    pub draw_frames: u64,
    /// Virtual monotonic time after the last frame, in nanoseconds.
    pub virtual_ns: u128,
    /// Whether the user asked for pause (takes effect at the next boundary).
    pub pause_requested: bool,
    /// Run as fast as possible instead of at the game's framerate.
    pub fastforward: bool,
    pub game_pid: u32,
    pub log_path: PathBuf,
    pub error: Option<String>,
    /// Something the user should know that didn't stop the run (libTAS
    /// alerts: movie failed to load, game hash mismatch, ...).
    pub warning: Option<String>,
    pub recording: Recording,
    /// Frames in the movie (libTAS `movie_framecount`).
    pub movie_frames: u64,
    /// Movie changed since last saved.
    pub movie_modified: bool,
    /// Where the movie was saved when the game exited without one selected.
    pub backup_movie: Option<PathBuf>,
}

/// One captured game frame.
pub struct Frame {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub framecount: u64,
    pub bgra: Vec<u8>,
}

struct Shared {
    status: RunStatus,
    /// Keys held in the frontend right now (X11 keysyms). Sampled at each
    /// frame boundary into that frame's inputs, as libTAS samples the
    /// keyboard state.
    held: BTreeSet<u32>,
    /// Keys the game currently sees pressed.
    game_keys: BTreeSet<u32>,
    movie: Movie,
    movie_path: Option<PathBuf>,
    on_movie_end: OnMovieEnd,
    pending_steps: u32,
    quit: bool,
    /// Display pacing: the frontend grants one game frame per tick of its own
    /// frame loop (see [`Run::display_tick`]).
    display_credit: bool,
    last_display_tick: Option<Instant>,
}

/// Without a display tick for this long, frames go back to being paced by the
/// shim's own timer (frontend hidden, stalled, or not pacing at all).
const DISPLAY_TICK_STALE: Duration = Duration::from_millis(50);

struct Inner {
    shared: Mutex<Shared>,
    cv: Condvar,
    child: Mutex<Option<Child>>,
}

/// Handle to one game run. Cheap to clone.
#[derive(Clone)]
pub struct Run {
    inner: Arc<Inner>,
    pub spec: Arc<LaunchSpec>,
    pub dir: PathBuf,
}

fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("obake")
}

impl Run {
    /// Starts the game with the shim preloaded. Returns immediately; the game
    /// connects on its first frame boundary.
    pub fn launch(spec: LaunchSpec) -> Result<Run, String> {
        if !spec.program.is_file() {
            return Err(format!("{} is not a file", spec.program.display()));
        }
        let shim = find_shim()?;
        let mut spec = spec;
        let md5 = movie::md5_file(&spec.program);
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = runtime_dir().join(format!("run-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let sock_path = dir.join("shim.sock");
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).map_err(|e| format!("bind {}: {e}", sock_path.display()))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;

        // libTAS GameLoop::init: open the movie before starting the game, so
        // the game runs with the movie's settings.
        let mut warning = None;
        let mut recording = spec.movie.recording;
        let mut mv = Movie::new(spec.framerate);
        if recording == Recording::Read {
            let path = spec.movie.path.clone().unwrap_or_default();
            match Movie::load(&path, &dir.join("movie")) {
                Ok(m) => {
                    if !m.md5.is_empty() && m.md5 != md5 {
                        warning = Some("Game executable hash does not match with the hash stored in the movie!".into());
                    }
                    spec.framerate = m.framerate;
                    mv = m;
                }
                Err(e) => {
                    warning = Some(e);
                    recording = Recording::None;
                }
            }
        }
        if recording != Recording::Read {
            mv.md5 = md5;
            mv.game_name = spec.program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // libTAS adds one blank frame for the inputs between startup and
            // the first frame boundary, which can't be set.
            mv.set_inputs(0, FrameInputs::default());
            mv.modified = false;
        }

        let log_path = dir.join("game.log");
        let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let log2 = log.try_clone().map_err(|e| e.to_string())?;

        let mut preload = shim.display().to_string();
        if let Ok(old) = std::env::var("LD_PRELOAD") {
            if !old.is_empty() {
                preload = format!("{preload}:{old}");
            }
        }
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .env("LD_PRELOAD", preload)
            .env(ENV_SOCKET, &sock_path)
            .env(frames::ENV_FRAMES, dir.join("frames.bin"))
            .env(ENV_FRAMERATE, format!("{}/{}", spec.framerate.0, spec.framerate.1))
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log2);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().map_err(|e| format!("spawn {}: {e}", spec.program.display()))?;
        let pid = child.id();

        let inner = Arc::new(Inner {
            shared: Mutex::new(Shared {
                status: RunStatus {
                    state: RunState::Starting,
                    frame: 0,
                    draw_frames: 0,
                    virtual_ns: 0,
                    pause_requested: false,
                    fastforward: false,
                    game_pid: pid,
                    log_path,
                    error: None,
                    warning,
                    recording,
                    movie_frames: mv.len(),
                    movie_modified: false,
                    backup_movie: None,
                },
                pending_steps: 0,
                held: BTreeSet::new(),
                game_keys: BTreeSet::new(),
                movie_path: spec.movie.path.clone(),
                on_movie_end: spec.movie.on_movie_end,
                movie: mv,
                quit: false,
                display_credit: false,
                last_display_tick: None,
            }),
            cv: Condvar::new(),
            child: Mutex::new(Some(child)),
        });
        let run = Run { inner, spec: Arc::new(spec), dir };

        let r = run.clone();
        std::thread::Builder::new()
            .name(format!("obake-run-{pid}"))
            .spawn(move || r.serve(listener))
            .map_err(|e| e.to_string())?;
        let r = run.clone();
        std::thread::Builder::new()
            .name(format!("obake-wait-{pid}"))
            .spawn(move || r.reap())
            .map_err(|e| e.to_string())?;
        Ok(run)
    }

    /// The newest frame the game published, if newer than `after_seq`.
    /// Pixels are BGRA, top row first.
    pub fn latest_frame(&self, after_seq: u64) -> Option<Frame> {
        use std::os::unix::fs::FileExt;
        let f = std::fs::File::open(self.dir.join("frames.bin")).ok()?;
        let mut h = [0u8; frames::HEADER_SIZE];
        f.read_exact_at(&mut h, 0).ok()?;
        let hdr = frames::read_header(&h)?;
        if hdr.seq % 2 == 1 || hdr.seq <= after_seq {
            return None;
        }
        let mut bgra = vec![0u8; hdr.width as usize * hdr.height as usize * 4];
        f.read_exact_at(&mut bgra, frames::HEADER_SIZE as u64).ok()?;
        f.read_exact_at(&mut h, 0).ok()?;
        // Writer started another frame while we read: drop this torn copy.
        if frames::read_header(&h)?.seq != hdr.seq {
            return None;
        }
        Some(Frame { seq: hdr.seq, width: hdr.width, height: hdr.height, framecount: hdr.framecount, bgra })
    }

    pub fn status(&self) -> RunStatus {
        self.inner.shared.lock().unwrap().status.clone()
    }

    fn update(&self, f: impl FnOnce(&mut Shared)) {
        let mut s = self.inner.shared.lock().unwrap();
        f(&mut s);
        self.inner.cv.notify_all();
    }

    /// Pause at the next frame boundary.
    pub fn pause(&self) {
        self.update(|s| s.status.pause_requested = true);
    }

    /// Resume real-time play.
    pub fn play(&self) {
        self.update(|s| {
            s.status.pause_requested = false;
            s.pending_steps = 0;
        });
    }

    pub fn set_fastforward(&self, on: bool) {
        self.update(|s| s.status.fastforward = on);
    }

    /// Lets the game run one frame, in step with the frontend's frame loop.
    ///
    /// Call once per game-frame interval from the loop that presents the
    /// game's frames. While ticks keep coming the game is paced by them
    /// instead of by the shim's timer, so every presented frame gets exactly
    /// one new game frame; two free-running 60 Hz clocks instead drift
    /// through each other and periodically repeat/drop frames.
    pub fn display_tick(&self) {
        self.update(|s| {
            s.display_credit = true;
            s.last_display_tick = Some(Instant::now());
        });
    }

    /// Blocks until more than `after_draws` frames have been drawn, the run
    /// ends, or `timeout` passes. Returns the drawn-frame count.
    pub fn wait_draw(&self, after_draws: u64, timeout: Duration) -> u64 {
        self.wait_for(timeout, |s| s.draw_frames > after_draws || matches!(s.state, RunState::Exited(_)))
            .draw_frames
    }

    pub fn toggle_pause(&self) {
        if self.status().pause_requested { self.play() } else { self.pause() }
    }

    /// A key went down or up in the frontend (X11 keysym). The game sees the
    /// keys held at each frame boundary.
    pub fn key(&self, keysym: u32, pressed: bool) {
        self.update(|s| {
            if pressed {
                s.held.insert(keysym);
            } else {
                s.held.remove(&keysym);
            }
        });
    }

    /// The frontend lost keyboard focus: nothing is held any more (libTAS
    /// takes no input while the game window is unfocused).
    pub fn release_all_keys(&self) {
        self.update(|s| s.held.clear());
    }

    /// libTAS's read-only toggle: switch a movie between playback and
    /// recording. Recording from the middle of a movie truncates it there.
    pub fn set_read_only(&self, read_only: bool) {
        self.update(|s| {
            if s.status.recording != Recording::None {
                s.status.recording = if read_only { Recording::Read } else { Recording::Write };
            }
        });
    }

    /// Save the movie to its file.
    pub fn save_movie(&self) -> Result<PathBuf, String> {
        let path = self.inner.shared.lock().unwrap().movie_path.clone().ok_or("no movie file selected")?;
        self.save_movie_as(&path)?;
        Ok(path)
    }

    /// Save the movie to `path`.
    pub fn save_movie_as(&self, path: &Path) -> Result<(), String> {
        let mut s = self.inner.shared.lock().unwrap();
        let r = s.movie.save(path, &self.dir.join("movie"));
        s.status.movie_modified = s.movie.modified;
        self.inner.cv.notify_all();
        r
    }

    /// While paused, advance exactly one frame.
    pub fn step(&self) {
        self.update(|s| {
            s.status.pause_requested = true;
            s.pending_steps += 1;
        });
    }

    /// Ask the game to quit at its next frame boundary; kill it if it doesn't
    /// go away within two seconds.
    pub fn stop(&self) {
        self.update(|s| s.quit = true);
        let r = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if matches!(r.status().state, RunState::Exited(_)) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            r.kill();
        });
    }

    pub fn kill(&self) {
        let pid = self.status().game_pid as libc::pid_t;
        if !matches!(self.status().state, RunState::Exited(_)) {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }

    /// Blocks until the run reaches `pred` or `timeout` elapses.
    pub fn wait_for(&self, timeout: Duration, pred: impl Fn(&RunStatus) -> bool) -> RunStatus {
        let deadline = Instant::now() + timeout;
        let mut s = self.inner.shared.lock().unwrap();
        loop {
            if pred(&s.status) {
                return s.status.clone();
            }
            let now = Instant::now();
            if now >= deadline {
                return s.status.clone();
            }
            s = self.inner.cv.wait_timeout(s, deadline - now).unwrap().0;
        }
    }

    fn reap(&self) {
        let child = self.inner.child.lock().unwrap().take();
        if let Some(mut child) = child {
            let code = child.wait().ok().and_then(|st| st.code());
            // libTAS GameLoop::loopExit: without a movie, silently keep a backup.
            if self.status().recording == Recording::None {
                let backup = self.dir.join("backup.ltm");
                if self.save_movie_as(&backup).is_ok() {
                    self.update(|s| s.status.backup_movie = Some(backup));
                }
            }
            self.update(|s| s.status.state = RunState::Exited(code));
        }
    }

    fn fail(&self, msg: String) {
        self.update(|s| {
            if !matches!(s.status.state, RunState::Exited(_)) {
                s.status.state = RunState::Failed;
            }
            s.status.error = Some(msg);
        });
        self.kill();
    }

    fn exited(&self) -> bool {
        matches!(self.status().state, RunState::Exited(_))
    }

    /// Socket thread: accept the shim, then drive frame boundaries.
    fn serve(&self, listener: UnixListener) {
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if self.exited() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return self.fail(format!("accept: {e}")),
            }
        };
        drop(listener);
        let _ = stream.set_nonblocking(false);

        match read_msg(&mut stream) {
            Ok((MSGB_HELLO, p)) => {
                let mut r = Reader::new(&p);
                let version = r.u32().unwrap_or(0);
                if version != PROTOCOL_VERSION {
                    return self.fail(format!("shim protocol v{version}, controller v{PROTOCOL_VERSION}"));
                }
            }
            Ok((tag, _)) => return self.fail(format!("expected hello, got message {tag}")),
            Err(e) => return self.fail(format!("handshake: {e}")),
        }

        loop {
            let (tag, payload) = match read_msg(&mut stream) {
                Ok(m) => m,
                // EOF: the game exited; the reaper records how.
                Err(_) => return,
            };
            match tag {
                MSGB_START_FRAMEBOUNDARY => {
                    let mut r = Reader::new(&payload);
                    let frame = r.u64().unwrap_or(0);
                    let draw = r.u32().unwrap_or(0) != 0;
                    let sec = r.u64().unwrap_or(0) as u128;
                    let nsec = r.u64().unwrap_or(0) as u128;
                    let reply = self.end_frame_boundary(frame, draw, sec * 1_000_000_000 + nsec);
                    let ok = match reply {
                        Reply::Continue { running, fastforward, keys } => {
                            for (sym, pressed) in keys {
                                let mut p = Payload::<8>::new();
                                p.put_u32(sym).put_u32(pressed as u32);
                                let _ = write_msg(&mut stream, MSGN_KEY, p.bytes());
                            }
                            let mut p = Payload::<8>::new();
                            p.put_u32(running as u32).put_u32(fastforward as u32);
                            write_msg(&mut stream, MSGN_END_FRAMEBOUNDARY, p.bytes())
                        }
                        Reply::Quit => write_msg(&mut stream, MSGN_USERQUIT, &[]),
                    };
                    if ok.is_err() {
                        return;
                    }
                }
                MSGB_UNHOOKED => {}
                other => return self.fail(format!("unexpected message {other}")),
            }
        }
    }

    /// Records the frame and blocks while paused (the game stays parked).
    fn end_frame_boundary(&self, frame: u64, draw: bool, virtual_ns: u128) -> Reply {
        let mut s = self.inner.shared.lock().unwrap();
        s.status.frame = frame;
        s.status.virtual_ns = virtual_ns;
        if draw {
            s.status.draw_frames += 1;
            self.inner.cv.notify_all();
        }
        loop {
            if s.quit {
                return Reply::Quit;
            }
            if !s.status.pause_requested {
                if s.status.state != RunState::Running {
                    s.status.state = RunState::Running;
                    self.inner.cv.notify_all();
                }
                // Display-paced: hold the game until the frontend's next tick.
                let since_tick = s.last_display_tick.map(|t| t.elapsed());
                let display_paced =
                    !s.status.fastforward && since_tick.is_some_and(|d| d < DISPLAY_TICK_STALE);
                if display_paced && !s.display_credit {
                    let left = DISPLAY_TICK_STALE - since_tick.unwrap_or_default();
                    s = self.inner.cv.wait_timeout(s, left).unwrap().0;
                    continue;
                }
                s.display_credit = false;
                let keys = self.process_inputs(&mut s, frame);
                // `running: false` = the shim must not sleep to pace this
                // frame; we already did the waiting here.
                return Reply::Continue { running: !display_paced, fastforward: s.status.fastforward, keys };
            }
            if s.pending_steps > 0 {
                s.pending_steps -= 1;
                s.status.state = RunState::Paused;
                self.inner.cv.notify_all();
                let keys = self.process_inputs(&mut s, frame);
                return Reply::Continue { running: false, fastforward: s.status.fastforward, keys };
            }
            if s.status.state != RunState::Paused {
                s.status.state = RunState::Paused;
                self.inner.cv.notify_all();
            }
            s = self.inner.cv.wait(s).unwrap();
        }
    }
}

impl Run {
    /// libTAS `GameLoop::processInputs` plus the end-of-movie pause: the
    /// inputs for frame `frame`, recorded into or read from the movie, as key
    /// events relative to what the game already has pressed.
    fn process_inputs(&self, s: &mut Shared, frame: u64) -> Vec<(u32, bool)> {
        let inputs = match s.status.recording {
            Recording::None | Recording::Write => {
                let inputs = FrameInputs { keys: s.held.iter().copied().take(movie::MAXKEYS).collect(), other: vec![] };
                s.movie.set_inputs(frame, inputs.clone());
                inputs
            }
            Recording::Read => {
                if frame + 1 == s.movie.len() && s.on_movie_end == OnMovieEnd::Write {
                    s.status.recording = Recording::Write;
                }
                s.movie.inputs(frame)
            }
        };
        // Pause when the movie's last frame has been played.
        if s.status.recording == Recording::Read && s.movie.len() == frame + 1 {
            s.status.pause_requested = true;
            s.status.fastforward = false;
        }
        s.status.movie_frames = s.movie.len();
        s.status.movie_modified = s.movie.modified;
        self.inner.cv.notify_all();

        let now: BTreeSet<u32> = inputs.keys.into_iter().collect();
        let mut events: Vec<(u32, bool)> = s.game_keys.difference(&now).map(|&k| (k, false)).collect();
        events.extend(now.difference(&s.game_keys).map(|&k| (k, true)));
        s.game_keys = now;
        events
    }
}

enum Reply {
    Continue { running: bool, fastforward: bool, keys: Vec<(u32, bool)> },
    Quit,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn read_msg(s: &mut UnixStream) -> std::io::Result<(u32, Vec<u8>)> {
    let mut h = [0u8; HEADER_LEN];
    s.read_exact(&mut h)?;
    let (tag, len) = decode_header(&h);
    let mut p = vec![0u8; len as usize];
    s.read_exact(&mut p)?;
    Ok((tag, p))
}

fn write_msg(s: &mut UnixStream, tag: u32, payload: &[u8]) -> std::io::Result<()> {
    s.write_all(&encode_header(tag, payload.len() as u32))?;
    s.write_all(payload)
}

/// `obake doctor`: fail fast with a clear message.
pub fn doctor() -> Vec<(String, Result<String, String>)> {
    let mut out = Vec::new();
    out.push(("shim".into(), find_shim().map(|p| p.display().to_string())));
    let display = std::env::var("DISPLAY").ok().filter(|d| !d.is_empty());
    out.push((
        "X display".into(),
        display.map(|d| format!("DISPLAY={d}")).ok_or_else(|| "DISPLAY is not set (games render through X11/GLX)".into()),
    ));
    out
}
