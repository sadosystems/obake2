//! libTAS movies (src/program/movie): one input line per frame, packed with a
//! `config.ini` header into a gzipped tar (`.ltm`). obake only produces
//! keyboard input, but sections it doesn't know (mouse, controllers, flags,
//! framerate, realtime, events) are kept verbatim so a libTAS movie survives a
//! load/save round trip.
//!
//! Frame `n` holds the inputs delivered at the end of frame boundary `n`
//! (libTAS `framecount` after the increment), applied while the game runs
//! frame `n + 1`. Frame 0 is the blank input between startup and the first
//! boundary, which can't be set.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// libTAS `AllInputs::MAXKEYS`.
pub const MAXKEYS: usize = 16;

/// libTAS `SharedConfig::recording`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recording {
    /// No movie selected: inputs still go into a movie, saved as a backup
    /// when the game exits.
    None,
    /// Inputs are recorded into the movie, truncating anything after them.
    Write,
    /// Inputs come from the movie (read-only); live input is ignored.
    Read,
}

/// libTAS `Config::on_movie_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnMovieEnd {
    /// Stay read-only: past the end, no keys are pressed.
    Read,
    /// Switch to recording at the last frame.
    Write,
}

/// One frame of inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameInputs {
    /// Pressed X11 keysyms.
    pub keys: Vec<u32>,
    /// Other sections of the line, verbatim (without the `|` separators).
    pub other: Vec<String>,
}

impl FrameInputs {
    /// libTAS `InputSerialization::readFrame`, keyboard only.
    pub fn parse(line: &str) -> Option<Self> {
        let body = line.trim_end().strip_prefix('|')?;
        let mut f = FrameInputs::default();
        for sec in body.split('|').filter(|s| !s.is_empty()) {
            match sec.strip_prefix('K') {
                Some(keys) => {
                    for k in keys.split(':').filter(|k| !k.is_empty()) {
                        f.keys.push(u32::from_str_radix(k, 16).ok()?);
                    }
                }
                None => f.other.push(sec.to_string()),
            }
        }
        Some(f)
    }

    /// libTAS `InputSerialization::writeFrame`.
    pub fn line(&self) -> String {
        let mut s = String::new();
        // A line of raw events carries the whole frame.
        if !self.other.iter().any(|o| o.starts_with('E')) && !self.keys.is_empty() {
            s.push_str("|K");
            for (i, k) in self.keys.iter().enumerate() {
                let _ = write!(s, "{}{k:x}", if i > 0 { ":" } else { "" });
            }
        }
        for o in &self.other {
            s.push('|');
            s.push_str(o);
        }
        s.push('|');
        s
    }
}

/// A libTAS movie: inputs plus the `config.ini` header fields obake uses.
#[derive(Clone, Debug)]
pub struct Movie {
    pub frames: Vec<FrameInputs>,
    pub framerate: (u32, u32),
    pub rerecord_count: u32,
    pub authors: String,
    pub game_name: String,
    /// md5 of the game executable (`md5sum`), empty if unknown.
    pub md5: String,
    /// libTAS `initial_time_*` / `initial_monotonic_time_*` (sec, nsec).
    pub initial_time: (i64, i64),
    pub initial_monotonic_time: (i64, i64),
    /// Changed since it was last saved or loaded.
    pub modified: bool,
}

impl Movie {
    pub fn new(framerate: (u32, u32)) -> Self {
        Movie {
            frames: Vec::new(),
            framerate,
            rerecord_count: 0,
            authors: String::new(),
            game_name: String::new(),
            md5: String::new(),
            initial_time: (1, 0),
            initial_monotonic_time: (1, 0),
            modified: false,
        }
    }

    pub fn len(&self) -> u64 {
        self.frames.len() as u64
    }

    /// libTAS `MovieFileInputs::setInputs(inputs, pos, keep_inputs=false)`:
    /// write frame `pos` and drop everything after it.
    pub fn set_inputs(&mut self, pos: u64, inputs: FrameInputs) {
        let pos = pos as usize;
        // libTAS refuses a gap; pad with blank frames instead.
        if self.frames.len() < pos {
            self.frames.resize(pos, FrameInputs::default());
        }
        self.frames.truncate(pos);
        self.frames.push(inputs);
        self.modified = true;
    }

    /// Inputs for frame `pos`, or none past the end.
    pub fn inputs(&self, pos: u64) -> FrameInputs {
        self.frames.get(pos as usize).cloned().unwrap_or_default()
    }

    /// libTAS `MovieFileInputs::updateLength` (constant framerate).
    fn length(&self) -> (u64, u64) {
        let (num, den) = (self.framerate.0.max(1) as u64, self.framerate.1 as u64);
        let n = self.len();
        (n * den / num, (1_000_000_000 * n * den / num) % 1_000_000_000)
    }

    /// libTAS `MovieFile::loadMovie`.
    pub fn load(path: &Path, scratch: &Path) -> Result<Movie, String> {
        if !path.is_file() {
            return Err("Could not find the movie file".into());
        }
        let _ = std::fs::remove_dir_all(scratch);
        std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
        // Piping gzip -> tar as libTAS does, to avoid gzip warnings on old movies.
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut gzip = Command::new("gzip")
            .arg("-dq")
            .stdin(file)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("gzip: {e}"))?;
        let tar = Command::new("tar")
            .args(["-xUf", "-", "-C"])
            .arg(scratch)
            .stdin(gzip.stdout.take().unwrap())
            .status()
            .map_err(|e| format!("tar: {e}"))?;
        let _ = gzip.wait();
        if !tar.success() {
            return Err("The movie file could not be extracted".into());
        }
        let config = std::fs::read_to_string(scratch.join("config.ini"))
            .map_err(|_| "The movie file does not contain the config file".to_string())?;
        let inputs = std::fs::read_to_string(scratch.join("inputs"))
            .map_err(|_| "The movie file does not contain the inputs file".to_string())?;

        let ini = parse_ini(&config);
        let get = |k: &str| ini.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        let num = |k: &str| get(k).and_then(|v| v.parse::<i64>().ok());

        let mut m = Movie::new((60, 1));
        let fr_num = num("framerate_num").or_else(|| num("framerate")).unwrap_or(60) as u32;
        let fr_den = num("framerate_den").unwrap_or(1) as u32;
        m.framerate = (fr_num.max(1), fr_den.max(1));
        m.rerecord_count = num("rerecord_count").unwrap_or(0) as u32;
        m.authors = get("authors").unwrap_or_default().to_string();
        m.game_name = get("game_name").unwrap_or_default().to_string();
        m.md5 = get("md5").unwrap_or_default().to_string();
        m.initial_time = (num("initial_time_sec").unwrap_or(1), num("initial_time_nsec").unwrap_or(0));
        m.initial_monotonic_time =
            (num("initial_monotonic_time_sec").unwrap_or(1), num("initial_monotonic_time_nsec").unwrap_or(0));
        for line in inputs.lines().filter(|l| l.starts_with('|')) {
            match FrameInputs::parse(line) {
                Some(f) => m.frames.push(f),
                None => break,
            }
        }
        Ok(m)
    }

    /// libTAS `MovieFile::saveMovie`.
    pub fn save(&mut self, path: &Path, scratch: &Path) -> Result<(), String> {
        let _ = std::fs::remove_dir_all(scratch);
        std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
        let mut inputs = String::new();
        for f in &self.frames {
            inputs.push_str(&f.line());
            inputs.push('\n');
        }
        let (len_sec, len_nsec) = self.length();
        let mut c = String::from("[General]\n");
        let mut kv = |k: &str, v: &dyn std::fmt::Display| {
            let _ = writeln!(c, "{k}={v}");
        };
        kv("game_name", &ini_string(&self.game_name));
        kv("frame_count", &self.len());
        kv("mouse_support", &false);
        kv("nb_controllers", &0);
        kv("initial_time_sec", &self.initial_time.0);
        kv("initial_time_nsec", &self.initial_time.1);
        kv("initial_monotonic_time_sec", &self.initial_monotonic_time.0);
        kv("initial_monotonic_time_nsec", &self.initial_monotonic_time.1);
        kv("length_sec", &len_sec);
        kv("length_nsec", &len_nsec);
        kv("framerate_num", &self.framerate.0);
        kv("framerate_den", &self.framerate.1);
        kv("rerecord_count", &self.rerecord_count);
        kv("authors", &ini_string(&self.authors));
        // The libTAS version whose movie format this follows.
        kv("libtas_major_version", &1);
        kv("libtas_minor_version", &4);
        kv("libtas_patch_version", &8);
        kv("savestate_frame_count", &self.len());
        kv("auto_restart", &false);
        kv("variable_framerate", &false);
        if !self.md5.is_empty() {
            kv("md5", &self.md5);
        }
        // obake doesn't track main-thread time calls yet; -1 = off, as libTAS.
        c.push_str("\n[mainthread_timetrack]\n");
        for k in [
            "time",
            "gettimeofday",
            "clock",
            "clock_gettime_real",
            "clock_gettime_monotonic",
            "sdl_getticks",
            "sdl_getperformancecounter",
            "GetTickCount",
            "GetTickCount64",
            "QueryPerformanceCounter",
        ] {
            let _ = writeln!(c, "{k}=-1");
        }
        let w = |name: &str, data: &str| {
            std::fs::write(scratch.join(name), data).map_err(|e| format!("write {name}: {e}"))
        };
        w("inputs", &inputs)?;
        w("config.ini", &c)?;
        w("editor.ini", "")?;
        w("annotations.txt", "")?;
        let st = Command::new("tar")
            .arg("-czUf")
            .arg(path)
            .arg("-C")
            .arg(scratch)
            .args(["inputs", "config.ini", "editor.ini", "annotations.txt"])
            .status()
            .map_err(|e| format!("tar: {e}"))?;
        if !st.success() {
            return Err(format!("could not write {}", path.display()));
        }
        self.modified = false;
        Ok(())
    }
}

/// md5 of a file, as libTAS computes it for `md5` in the movie.
pub fn md5_file(path: &Path) -> String {
    Command::new("md5sum")
        .arg("-b")
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.chars().take(32).collect())
        .unwrap_or_default()
}

/// `QSettings` IniFormat, enough for libTAS's `config.ini`: `[General]` keys
/// unprefixed, others as `group/key`.
fn parse_ini(s: &str) -> Vec<(String, String)> {
    let mut group = String::new();
    let mut out = Vec::new();
    for line in s.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            group = line[1..line.len() - 1].to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let key = if group.is_empty() || group == "General" { k.to_string() } else { format!("{group}/{k}") };
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
            out.push((key, v.replace("\\\"", "\"").replace("\\\\", "\\")));
        }
    }
    out
}

/// Quote a string value the way QSettings does when it needs to.
fn ini_string(s: &str) -> String {
    if s.chars().any(|c| matches!(c, ',' | ';' | '=' | '"' | '\\') || c.is_whitespace()) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Where a run's movie lives and how it's used.
#[derive(Clone, Debug)]
pub struct MovieSpec {
    pub recording: Recording,
    /// The movie file; `None` with [`Recording::None`].
    pub path: Option<PathBuf>,
    pub on_movie_end: OnMovieEnd,
}

impl Default for MovieSpec {
    fn default() -> Self {
        MovieSpec { recording: Recording::None, path: None, on_movie_end: OnMovieEnd::Read }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_round_trip() {
        for l in ["|", "|K61:ff52|", "|K20|C1 0:0:0:0:0:0:...............|", "|E1:2:3|E4:5:6|", "|M1:2:A:1....:0|"] {
            assert_eq!(FrameInputs::parse(l).unwrap().line(), l, "{l}");
        }
        assert_eq!(FrameInputs::parse("|K61:ff52|").unwrap().keys, vec![0x61, 0xff52]);
    }

    #[test]
    fn set_inputs_truncates() {
        let mut m = Movie::new((60, 1));
        for i in 0..5 {
            m.set_inputs(i, FrameInputs { keys: vec![i as u32], other: vec![] });
        }
        m.set_inputs(2, FrameInputs::default());
        assert_eq!(m.len(), 3);
        m.set_inputs(6, FrameInputs::default());
        assert_eq!(m.len(), 7);
    }

    #[test]
    fn save_load() {
        let dir = std::env::temp_dir().join(format!("obake-movie-test-{}", std::process::id()));
        let mut m = Movie::new((30, 1));
        m.game_name = "a game".into();
        m.set_inputs(0, FrameInputs::default());
        m.set_inputs(1, FrameInputs { keys: vec![0x61], other: vec![] });
        let path = dir.with_extension("ltm");
        m.save(&path, &dir.join("w")).unwrap();
        let l = Movie::load(&path, &dir.join("r")).unwrap();
        assert_eq!(l.frames, m.frames);
        assert_eq!(l.framerate, (30, 1));
        assert_eq!(l.game_name, "a game");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }
}
