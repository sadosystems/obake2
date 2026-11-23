//! obake: launching a game and controlling it from a pane.
//!
//! `obake::LaunchGame` asks for a game binary in a file browser, starts it
//! through `obake_controller` (shim preloaded) and opens a [`GameView`] for
//! the run. The game draws on the GPU in its own window; the pane shows the
//! run's state and holds the transport controls.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement,
    ObjectFit, PathPromptOptions, PromptLevel, Render, RenderImage, SharedString, Styled,
    StyledImage as _, Subscription, Task, Window, div, img,
};
use obake_controller::{LaunchSpec, MovieSpec, Recording, Run, RunState, RunStatus};
use project::DirectoryLister;
use ui::{Color, IconButton, IconButtonShape, IconName, IconSize, Label, Tooltip, LabelCommon as _, LabelSize, prelude::*};
use workspace::{
    Item, OpenPathPromptMode, Toast, Workspace, item::ItemEvent, notifications::NotificationId,
};
use obake_actions::obake::{
    LaunchGame, OpenReplay, SaveReplay, StepFrame, StopGame, TogglePause, ToggleRecording,
};

pub fn init(cx: &mut App) {
    // Obake's in-app file browser, used when system dialogs are off or fail.
    // Its registration lived in a crate removed when pruning.
    cx.observe_new(open_path_prompt::OpenPathPrompt::register)
        .detach();
    cx.observe_new(open_path_prompt::OpenPathPrompt::register_new_path)
        .detach();
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        // Dev hook: OBAKE_AUTOLAUNCH=<binary or .ltm> launches it in the first window.
        static AUTOLAUNCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if let (Some(window), Ok(path)) = (window, std::env::var("OBAKE_AUTOLAUNCH")) {
            if !AUTOLAUNCHED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                cx.spawn_in(window, async move |ws, cx| {
                    // A .ltm opens the movie on the game that recorded it.
                    let path = PathBuf::from(path);
                    let (game, spec) = if path.extension().is_some_and(|e| e == "ltm") {
                        let game = obake_controller::read_movie(&path).ok().and_then(|m| m.game_path);
                        (game, MovieSpec { recording: Recording::Read, path: Some(path), ..Default::default() })
                    } else {
                        (Some(path), MovieSpec::default())
                    };
                    if let Some(game) = game {
                        ws.update_in(cx, |ws, window, cx| open_run(ws, game, spec, false, window, cx)).ok();
                    }
                })
                .detach();
            }
        }
        workspace.register_action(launch_game);
        workspace.register_action(open_replay);
        workspace.register_action(save_replay);
        workspace.register_action(|ws, a: &ToggleRecording, window, cx| {
            with_active_game(ws, cx, |v, cx| v.toggle_recording(a, window, cx))
        });
        // Transport actions reach the active game pane even when focus is
        // elsewhere in the window (menus, docks, title bar).
        workspace.register_action(|ws, a: &TogglePause, window, cx| {
            with_active_game(ws, cx, |v, cx| v.toggle_pause(a, window, cx))
        });
        workspace.register_action(|ws, a: &StepFrame, window, cx| {
            with_active_game(ws, cx, |v, cx| v.step(a, window, cx))
        });
        workspace.register_action(|ws, a: &StopGame, window, cx| {
            with_active_game(ws, cx, |v, cx| v.stop(a, window, cx))
        });
    })
    .detach();
}

fn with_active_game(
    workspace: &mut Workspace,
    cx: &mut Context<Workspace>,
    f: impl FnOnce(&mut GameView, &mut Context<GameView>),
) {
    if let Some(view) = workspace.active_item_as::<GameView>(cx) {
        view.update(cx, f);
    }
}

/// Asks for existing files.
fn pick_files(
    workspace: &mut Workspace,
    prompt: &str,
    multiple: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Task<Vec<PathBuf>> {
    let lister = DirectoryLister::Local(workspace.project().clone(), workspace.app_state().fs.clone());
    let paths = workspace.prompt_for_open_path(
        PathPromptOptions {
            files: true,
            directories: false,
            multiple,
            prompt: Some(prompt.to_string().into()),
        },
        lister,
        OpenPathPromptMode::SelectPath,
        window,
        cx,
    );
    cx.background_spawn(async move { paths.await.ok().flatten().map(|r| r.paths).unwrap_or_default() })
}

/// Picks game binaries and launches each with `movie`, one pane apiece.
fn pick_and_launch(
    workspace: &mut Workspace,
    movie: MovieSpec,
    multiple: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let games = pick_files(workspace, "Launch", multiple, window, cx);
    cx.spawn_in(window, async move |workspace, cx| {
        let games = games.await;
        workspace
            .update_in(cx, |workspace, window, cx| {
                for (i, game) in games.into_iter().enumerate() {
                    open_run(workspace, game, movie.clone(), i > 0, window, cx);
                }
            })
            .ok();
    })
    .detach();
}

/// Every run records its input; there is no separate "record" launch.
fn launch_game(workspace: &mut Workspace, _: &LaunchGame, window: &mut Window, cx: &mut Context<Workspace>) {
    pick_and_launch(workspace, MovieSpec::default(), false, window, cx);
}

/// Opens a replay in its own pane. It is data on its own: nothing
/// runs until you pick games to play it on.
fn open_replay(workspace: &mut Workspace, _: &OpenReplay, window: &mut Window, cx: &mut Context<Workspace>) {
    let picked = pick_files(workspace, "Open", false, window, cx);
    cx.spawn_in(window, async move |workspace, cx| {
        let Some(path) = picked.await.into_iter().next() else { return };
        let movie = cx.background_spawn({
            let path = path.clone();
            async move { obake_controller::read_movie(&path) }
        });
        let movie = movie.await;
        workspace
            .update_in(cx, |workspace, window, cx| match movie {
                Ok(movie) => {
                    let ws = cx.entity().downgrade();
                    let view = cx.new(|cx| ReplayView::new(path, movie, ws, cx));
                    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
                }
                Err(e) => {
                    struct BadReplay;
                    workspace.show_toast(
                        Toast::new(NotificationId::unique::<BadReplay>(), format!("{}: {e}", path.display())),
                        cx,
                    );
                }
            })
            .ok();
    })
    .detach();
}

/// Saves the focused game's movie, asking where the first time.
fn save_replay(workspace: &mut Workspace, _: &SaveReplay, window: &mut Window, cx: &mut Context<Workspace>) {
    if let Some(view) = workspace.active_item_as::<GameView>(cx) {
        save_view(workspace, view, window, cx);
    }
}

/// Saves a game's recording, asking for a file the first time.
fn save_view(workspace: &mut Workspace, view: Entity<GameView>, window: &mut Window, cx: &mut Context<Workspace>) {
    if view.read(cx).run.status().movie_path.is_some() {
        view.update(cx, |v, cx| v.save_to(None, cx));
        return;
    }
    let lister = DirectoryLister::Local(workspace.project().clone(), workspace.app_state().fs.clone());
    let suggested = format!("{}.ltm", view.read(cx).title);
    let path = workspace.prompt_for_new_path(lister, Some(suggested), window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let Some(path) = path.await.ok().flatten().and_then(|p| p.into_iter().next()) else { return };
        view.update(cx, |v, cx| v.save_to(Some(path), cx));
    })
    .detach();
}

/// Launches `path` and shows it in the active pane, or in a new pane to the
/// right with `split`.
pub fn open_run(
    workspace: &mut Workspace,
    path: PathBuf,
    movie: MovieSpec,
    split: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let mut spec = LaunchSpec::for_binary(&path);
    spec.movie = movie;
    match Run::launch(spec) {
        Ok(run) => {
            // libTAS alerts (movie failed to load, hash mismatch).
            if let Some(warning) = run.status().warning {
                struct LaunchWarning;
                workspace.show_toast(Toast::new(NotificationId::unique::<LaunchWarning>(), warning), cx);
            }
            // Dev hook: OBAKE_AUTOLAUNCH_FF=1 starts in fast-forward.
            if std::env::var("OBAKE_AUTOLAUNCH_FF").is_ok() {
                run.set_fastforward(true);
            }
            let ws = cx.entity().downgrade();
            let view = cx.new(|cx| GameView::new(run, path, ws, window, cx));
            if split {
                workspace.split_item(workspace::SplitDirection::Right, Box::new(view.clone()), window, cx);
            } else {
                workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
            }
            window.focus(&view.focus_handle(cx), cx);
        }
        Err(err) => {
            struct LaunchFailed;
            workspace.show_toast(
                Toast::new(
                    NotificationId::unique::<LaunchFailed>(),
                    format!("Could not launch {}: {err}", path.display()),
                ),
                cx,
            );
        }
    }
}

/// Locks the game's frame pacing to this window's frame loop.
///
/// The game and the window each run at ~60 Hz off their own timers; left
/// alone the two drift through each other and the pane periodically shows a
/// frame twice then skips one. Instead, while the window's frame interval
/// divides the game's, each (nth) window frame releases exactly one game
/// frame. At other refresh rates the shim's timer keeps pacing.
#[derive(Default)]
struct DisplayPacer {
    last: Option<std::time::Instant>,
    intervals: std::collections::VecDeque<f64>,
    phase: u32,
}

impl DisplayPacer {
    const SAMPLES: usize = 15;

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Called once per window frame; true when a game frame is due.
    fn frame(&mut self, game_frame: f64) -> bool {
        let now = std::time::Instant::now();
        if let Some(last) = self.last.replace(now) {
            let dt = now.duration_since(last).as_secs_f64();
            if dt > 0.1 {
                self.intervals.clear();
            } else {
                if self.intervals.len() == Self::SAMPLES {
                    self.intervals.pop_front();
                }
                self.intervals.push_back(dt);
            }
        }
        if self.intervals.len() < Self::SAMPLES {
            return false;
        }
        let mut sorted: Vec<f64> = self.intervals.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let refresh = sorted[sorted.len() / 2];
        let n = (game_frame / refresh).round();
        if n < 1.0 || (refresh * n - game_frame).abs() > game_frame * 0.03 {
            return false;
        }
        self.phase += 1;
        if self.phase >= n as u32 {
            self.phase = 0;
            true
        } else {
            false
        }
    }
}

pub struct GameView {
    focus_handle: FocusHandle,
    run: Run,
    workspace: gpui::WeakEntity<Workspace>,
    path: PathBuf,
    title: SharedString,
    status: RunStatus,
    frame: Option<Arc<RenderImage>>,
    frame_seq: u64,
    shown: u32,
    fps_since: std::time::Instant,
    display_fps: f64,
    /// Size of the area the frame is fitted into, as of the last paint.
    stage: std::rc::Rc<std::cell::Cell<gpui::Size<gpui::Pixels>>>,
    /// Game frames per real second, and the frame count it is measured from.
    game_fps: f64,
    fps_frame: u64,
    /// Replaced frames whose GPU atlas entries must be released.
    stale_frames: Vec<Arc<RenderImage>>,
    display_pacer: DisplayPacer,
    /// Last movie save result, shown in the status bar.
    notice: Option<(String, Color)>,
    frame_log: Option<std::rc::Rc<std::cell::RefCell<FrameLog>>>,
    _poll: Task<()>,
    _focus: [Subscription; 2],
}

impl GameView {
    fn new(
        run: Run,
        path: PathBuf,
        workspace: gpui::WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let poll_run = run.clone();
        let frame_log = FrameLog::from_env();
        let render_log = frame_log.clone();
        let poll = cx.spawn_in(window, async move |this, cx| {
            let mut seen_draws = 0;
            let mut seen_seq = 0;
            loop {
                // Wait for the next drawn frame off the UI thread, and read it
                // there too, so the pane keeps up with the game at any speed.
                let run = poll_run.clone();
                let (draws, frame) = cx
                    .background_executor()
                    .spawn(async move {
                        let d = run.wait_draw(seen_draws, Duration::from_millis(100));
                        let t = std::time::Instant::now();
                        let f = run.latest_frame(seen_seq);
                        (d, f.map(|f| (f, t.elapsed())))
                    })
                    .await;
                let frame = frame.map(|(f, read)| {
                    if let Some(log) = &frame_log {
                        log.borrow_mut().arrived(read, f.framecount);
                    }
                    f
                });
                seen_draws = draws;
                if let Some(f) = &frame {
                    seen_seq = f.seq;
                }
                let alive = this.update_in(cx, |this, window, cx| {
                    if let Some(f) = frame {
                        this.frame_seq = f.seq;
                        this.shown += 1;
                        let now = std::time::Instant::now();
                        let dt = now.duration_since(this.fps_since);
                        if dt >= Duration::from_millis(500) {
                            this.display_fps = this.shown as f64 / dt.as_secs_f64();
                            this.game_fps = f.framecount.saturating_sub(this.fps_frame) as f64
                                / dt.as_secs_f64();
                            this.fps_frame = f.framecount;
                            this.shown = 0;
                            this.fps_since = now;
                        }
                        if let Some(buf) = image::ImageBuffer::from_raw(f.width, f.height, f.bgra) {
                            let new = Arc::new(
                                RenderImage::new(vec![image::Frame::new(buf)]).with_nearest_filter(),
                            );
                            if let Some(old) = this.frame.replace(new) {
                                this.stale_frames.push(old);
                            }
                            cx.notify();
                        }
                    }
                    let s = this.run.status();
                    if s.state != this.status.state
                        || s.frame != this.status.frame
                        || s.pause_requested != this.status.pause_requested
                        || s.fastforward != this.status.fastforward
                        || s.recording != this.status.recording
                        || s.movie_frames != this.status.movie_frames
                        || s.movie_modified != this.status.movie_modified
                    {
                        let exited = matches!(s.state, RunState::Exited(_))
                            && !matches!(this.status.state, RunState::Exited(_));
                        this.status = s;
                        if exited {
                            this.ask_to_save(window, cx);
                        }
                        cx.notify();
                    }
                    !matches!(this.status.state, RunState::Exited(_))
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        });
        let focus_handle = cx.focus_handle();
        // libTAS takes no input while the game doesn't have focus: release
        // everything held when the pane or the window loses it.
        let focus = [
            cx.on_blur(&focus_handle, window, |this, _, _| this.run.release_all_keys()),
            cx.observe_window_activation(window, |this, window, _| {
                if !window.is_window_active() {
                    this.run.release_all_keys();
                }
            }),
        ];
        Self {
            focus_handle,
            notice: None,
            frame_log: render_log,
            _focus: focus,
            status: run.status(),
            frame: None,
            frame_seq: 0,
            shown: 0,
            fps_since: std::time::Instant::now(),
            display_fps: 0.0,
            stage: Default::default(),
            game_fps: 0.0,
            fps_frame: 0,
            stale_frames: Vec::new(),
            display_pacer: DisplayPacer::default(),
            run,
            workspace,
            path,
            title,
            _poll: poll,
        }
    }

    fn toggle_pause(&mut self, _: &TogglePause, _: &mut Window, cx: &mut Context<Self>) {
        self.run.toggle_pause();
        self.status = self.run.status();
        cx.notify();
    }

    fn step(&mut self, _: &StepFrame, _: &mut Window, cx: &mut Context<Self>) {
        self.run.step();
        self.status = self.run.status();
        cx.notify();
    }

    fn stop(&mut self, _: &StopGame, _: &mut Window, cx: &mut Context<Self>) {
        self.run.stop();
        cx.notify();
    }

    /// Saves the movie to `path`, or to its own file.
    fn save_to(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.notice = None;
        let r = match path {
            Some(p) => self.run.save_movie_as(&p),
            None => self.run.save_movie().map(|_| ()),
        };
        // Success shows in the strip ("saved as ..."); only failures need words.
        if let Err(e) = r {
            self.notice = Some((format!("Could not save: {e}"), Color::Error));
        }
        self.status = self.run.status();
        cx.notify();
    }

    fn toggle_recording(&mut self, _: &ToggleRecording, _: &mut Window, cx: &mut Context<Self>) {
        self.run.set_recording(self.status.recording == Recording::Read);
        self.status = self.run.status();
        cx.notify();
    }

    /// libTAS on game exit: offer to save a modified movie.
    fn ask_to_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.recording == Recording::Off || !self.status.movie_modified {
            return;
        }
        let question = if self.status.movie_path.is_some() {
            "Save the changes to this replay?"
        } else {
            "Save this recording as a replay?"
        };
        let answer = window.prompt(PromptLevel::Info, question, None, &["Save", "Don't Save"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |this, window, cx| this.save(window, cx)).ok();
            }
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (ws, me) = (self.workspace.clone(), cx.entity());
        window.defer(cx, move |window, cx| {
            ws.update(cx, |ws, cx| save_view(ws, me, window, cx)).ok();
        });
    }

    /// Inputs only replay from frame 0, so recording restarts the game with
    /// recording on, in place of this pane.
    fn record(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run.stop();
        let (ws, me, path) = (self.workspace.clone(), cx.entity(), self.path.clone());
        window.defer(cx, move |window, cx| {
            ws.update(cx, |ws, cx| {
                let pane = ws.pane_for(&me);
                let spec = MovieSpec { recording: Recording::Write, ..Default::default() };
                open_run(ws, path, spec, false, window, cx);
                if let Some(pane) = pane {
                    pane.update(cx, |p, cx| p.remove_item(me.entity_id(), false, false, window, cx));
                }
            })
            .ok();
        });
    }

    // Mode colours are fixed rather than themed: the default theme is
    // monochrome, and recording vs playback must be told apart at a glance.
    /// The movie line under the game: what is happening, in words, and the
    /// one thing you'd want to do next.
    fn movie_strip(&self, alive: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.status;
        let movie_name = s
            .movie_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        let small = |t: String, c: Color| Label::new(t).size(LabelSize::Small).color(c);
        let red_lead = |text: String| {
            h_flex()
                .gap_1p5()
                .child(div().size(gpui::px(8.)).rounded_full().bg(RED))
                .child(div().text_sm().text_color(RED).child(text))
                .into_any_element()
        };
        let (lead, info, action): (gpui::AnyElement, String, Option<(&'static str, StripAction)>) = match s.recording {
            Recording::Off => (
                small("Live".into(), Color::Muted).into_any_element(),
                String::new(),
                Some(("● Record", StripAction::Record)),
            ),
            Recording::Write => {
                let unsaved = if s.movie_modified && movie_name.is_some() { "  ·  unsaved" } else { "" };
                let lead = match &movie_name {
                    Some(n) => red_lead(format!("Recording into {n}")),
                    None => red_lead("Recording".into()),
                };
                let action = match &movie_name {
                    None => Some(("Save replay…", StripAction::Save)),
                    Some(_) => s.movie_modified.then_some(("Save", StripAction::Save)),
                };
                (lead, format!("{} frames{unsaved}", s.movie_frames), action)
            }
            Recording::Read if s.frame >= s.movie_frames => (
                div().text_sm().text_color(AMBER).child("Replay ended").into_any_element(),
                format!("{}  ·  {} frames", movie_name.unwrap_or_default(), s.movie_frames),
                Some(("Take control", StripAction::TakeControl)),
            ),
            Recording::Read => (
                div()
                    .text_sm()
                    .text_color(GREEN)
                    .child(format!("▶ Playing {}", movie_name.unwrap_or_default()))
                    .into_any_element(),
                format!("{} / {}", s.frame, s.movie_frames),
                Some(("Take control", StripAction::TakeControl)),
            ),
        };
        h_flex()
            .px_2()
            .pt_1()
            .gap_3()
            .child(lead)
            .child(small(info, Color::Muted))
            .when_some(self.notice.clone(), |d, (t, c)| d.child(small(t, c)))
            .child(div().flex_1())
            .when_some(action.filter(|_| alive), |d, (label, act)| {
                d.child(
                    ui::Button::new("movie-action", label)
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(move |this, _, window, cx| match act {
                            StripAction::Save => this.save(window, cx),
                            StripAction::Record => this.record(window, cx),
                            StripAction::TakeControl => this.toggle_recording(&ToggleRecording, window, cx),
                        })),
                )
            })
    }

    fn state_text(&self) -> (String, Color) {
        match self.status.state {
            RunState::Starting => ("Starting…".into(), Color::Muted),
            RunState::Running if self.status.pause_requested => ("Pausing…".into(), Color::Warning),
            RunState::Running => ("Playing".into(), Color::Success),
            RunState::Paused => ("Paused".into(), Color::Warning),
            RunState::Exited(Some(code)) => (format!("Exited ({code})"), Color::Muted),
            RunState::Exited(None) => ("Killed".into(), Color::Muted),
            RunState::Failed => (
                format!("Failed: {}", self.status.error.clone().unwrap_or_default()),
                Color::Error,
            ),
        }
    }
}

impl Drop for GameView {
    fn drop(&mut self) {
        // Closing the pane ends the run.
        self.run.stop();
    }
}

impl EventEmitter<ItemEvent> for GameView {}

impl Focusable for GameView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for GameView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _: &App) -> SharedString {
        let star = if self.status.movie_modified && self.status.movie_path.is_some() { "*" } else { "" };
        match self.status.movie_path.as_ref().and_then(|p| p.file_name()) {
            Some(m) => format!("{} · {}{star}", self.title, m.to_string_lossy()).into(),
            None => self.title.clone(),
        }
    }

    fn tab_tooltip_text(&self, _: &App) -> Option<SharedString> {
        Some(self.path.display().to_string().into())
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }
}

impl Render for GameView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(log) = &self.frame_log {
            log.borrow_mut().painted();
        }
        for old in self.stale_frames.drain(..) {
            window.drop_image(old).ok();
        }
        // Pace the game off this window's frames while it plays at its own
        // framerate (uncapped it just runs flat out).
        if self.status.state == RunState::Running
            && !self.status.pause_requested
            && !self.status.fastforward
        {
            window.request_animation_frame();
            let (num, den) = self.run.spec.framerate;
            if self.display_pacer.frame(den as f64 / num as f64) {
                self.run.display_tick();
            }
        } else {
            self.display_pacer.reset();
        }
        let (state, state_color) = self.state_text();
        let alive = !matches!(self.status.state, RunState::Exited(_) | RunState::Failed);
        let paused = self.status.pause_requested;
        let s = &self.status;
        // Frame number, the game's own clock (what the game believes has
        // elapsed; it stops while paused), and how fast it is running.
        let secs = s.virtual_ns / 1_000_000_000;
        let clock = format!("{}:{:02}.{:02}", secs / 60, secs % 60, (s.virtual_ns / 10_000_000) % 100);
        let speed = if self.status.state == RunState::Running {
            format!("{:.0} fps", self.game_fps)
        } else {
            "0 fps".into()
        };
        let details = format!("Frame {}  ·  Game clock {clock}  ·  {speed}", s.frame);

        div()
            .key_context("GameView")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::toggle_pause))
            .on_action(cx.listener(Self::step))
            .on_action(cx.listener(Self::stop))
            .on_action(cx.listener(Self::toggle_recording))
            // Keys typed in the pane are the game's input. Chords with
            // ctrl/alt/super are left for app keybindings (ctrl-k splits etc).
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                let m = &e.keystroke.modifiers;
                // Also yield the second key of a pending chord (ctrl-k up).
                if m.control || m.alt || m.platform || window.has_pending_keystrokes() {
                    return;
                }
                if !e.is_held {
                    if let Some(sym) = keysym(&e.keystroke.key) {
                        this.run.key(sym, true);
                        cx.stop_propagation();
                    }
                }
            }))
            .on_key_up(cx.listener(|this, e: &gpui::KeyUpEvent, _, cx| {
                if let Some(sym) = keysym(&e.keystroke.key) {
                    this.run.key(sym, false);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    // The game sits inset on the pane background, framed by a
                    // hairline, so the chrome stays continuous whatever the
                    // game's own colors are.
                    .relative()
                    .p_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .map(|d| match self.frame.clone() {
                        Some(frame) => {
                            // Fit the frame inside the stage measured last paint
                            // (contain: as large as possible, aspect preserved).
                            let size = frame.size(0);
                            let (fw, fh) = (size.width.0 as f32, size.height.0.max(1) as f32);
                            let stage = self.stage.get();
                            let (sw, sh) = (f32::from(stage.width) - 16.0, f32::from(stage.height) - 16.0);
                            let scale = (sw / fw).min(sh / fh).max(0.0);
                            d.child(
                                div()
                                    .flex_none()
                                    .w(gpui::px(fw * scale + 2.0))
                                    .h(gpui::px(fh * scale + 2.0))
                                    .border_1()
                                    .border_color(cx.theme().colors().border)
                                    .bg(gpui::black())
                                    .child(
                                        img(frame)
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    ),
                            )
                        }
                        None => d.child(Label::new("Waiting for the first frame…").color(Color::Muted)),
                    })
                    .child({
                        let stage = self.stage.clone();
                        gpui::canvas(
                            move |bounds, _, _| stage.set(bounds.size),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    }),
            )
            .child(self.movie_strip(alive, cx))
            .child(
                h_flex()
                    .p_2()
                    .gap_3()
                    // Playing/paused is shown by the transport buttons; only
                    // other states (starting, exited, failed) get a label.
                    .when(
                        !matches!(self.status.state, RunState::Running | RunState::Paused),
                        |d| d.child(Label::new(state).color(state_color)),
                    )
                    .child(Label::new(details).size(LabelSize::Small).color(Color::Muted))
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                IconButton::new(
                                    "toggle-pause",
                                    if paused { IconName::TransportPlay } else { IconName::TransportPause },
                                )
                                .icon_size(IconSize::Medium)
                                .shape(IconButtonShape::Square)
                                .disabled(!alive)
                                .tooltip(Tooltip::text(if paused { "Play" } else { "Pause" }))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_pause(&TogglePause, window, cx)
                                })),
                            )
                            .child(
                                IconButton::new("step", IconName::TransportStep)
                                    .icon_size(IconSize::Medium)
                                    .shape(IconButtonShape::Square)
                                    .disabled(!alive || !paused)
                                    .tooltip(Tooltip::text("Step one frame"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.step(&StepFrame, window, cx)
                                    })),
                            )
                            .child(
                                IconButton::new("stop", IconName::TransportStop)
                                    .icon_size(IconSize::Medium)
                                    .icon_color(Color::Error)
                                    .shape(IconButtonShape::Square)
                                    .disabled(!alive)
                                    .tooltip(Tooltip::text("Stop"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.stop(&StopGame, window, cx)
                                    })),
                            )
                            .child(div().w_px().h_4().mx_1().bg(cx.theme().colors().border))
                            .child(
                                IconButton::new("uncap", IconName::TransportUncapped)
                                    .icon_size(IconSize::Medium)
                                    .shape(IconButtonShape::Square)
                                    .toggle_state(s.fastforward)
                                    .disabled(!alive)
                                    .tooltip(Tooltip::text(if s.fastforward {
                                        "FPS cap off (click to cap at 60)"
                                    } else {
                                        "FPS capped at 60 (click to uncap)"
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run.set_fastforward(!this.status.fastforward);
                                        this.status = this.run.status();
                                        cx.notify();
                                    })),
                            )
                    ),
            )
    }
}

/// OBAKE_FRAME_LOG=1: every 2s, print to stderr where frame time goes in the
/// pane (read = copying the frame out of the shared file, arrive = interval
/// between new frames reaching the UI thread, paint = interval between
/// renders) and how many game frames were never shown.
struct FrameLog {
    read: Vec<f64>,
    arrive: Vec<f64>,
    paint: Vec<f64>,
    last_arrive: Option<std::time::Instant>,
    last_paint: Option<std::time::Instant>,
    last_framecount: u64,
    skipped: u64,
    since: std::time::Instant,
}

impl FrameLog {
    fn from_env() -> Option<std::rc::Rc<std::cell::RefCell<Self>>> {
        std::env::var_os("OBAKE_FRAME_LOG")?;
        Some(std::rc::Rc::new(std::cell::RefCell::new(FrameLog {
            read: Vec::new(),
            arrive: Vec::new(),
            paint: Vec::new(),
            last_arrive: None,
            last_paint: None,
            last_framecount: 0,
            skipped: 0,
            since: std::time::Instant::now(),
        })))
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    fn arrived(&mut self, read: Duration, framecount: u64) {
        let now = std::time::Instant::now();
        self.read.push(Self::ms(read));
        if let Some(l) = self.last_arrive.replace(now) {
            self.arrive.push(Self::ms(now - l));
        }
        if self.last_framecount != 0 && framecount > self.last_framecount + 1 {
            self.skipped += framecount - self.last_framecount - 1;
        }
        self.last_framecount = framecount;
    }

    fn painted(&mut self) {
        let now = std::time::Instant::now();
        if let Some(l) = self.last_paint.replace(now) {
            self.paint.push(Self::ms(now - l));
        }
        if now - self.since >= Duration::from_secs(2) {
            let stats = |v: &mut Vec<f64>| {
                if v.is_empty() {
                    return "-".to_string();
                }
                v.sort_by(f64::total_cmp);
                let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
                let r = format!("p50 {:.1} p95 {:.1} max {:.1} (n {})", q(0.5), q(0.95), q(1.0), v.len());
                v.clear();
                r
            };
            eprintln!(
                "obake frames: read [{}] arrive [{}] paint [{}] skipped {}",
                stats(&mut self.read),
                stats(&mut self.arrive),
                stats(&mut self.paint),
                self.skipped
            );
            self.skipped = 0;
            self.since = now;
        }
    }
}

#[derive(Clone, Copy)]
enum StripAction {
    Save,
    Record,
    TakeControl,
}

const RED: gpui::Rgba = gpui::Rgba { r: 0.898, g: 0.282, b: 0.302, a: 1.0 };
const GREEN: gpui::Rgba = gpui::Rgba { r: 0.247, g: 0.725, b: 0.380, a: 1.0 };
const AMBER: gpui::Rgba = gpui::Rgba { r: 0.945, g: 0.659, b: 0.204, a: 1.0 };

/// A replay (libTAS .ltm) on its own: what's in it, and buttons to play
/// it on one or more game binaries, each in its own pane.
pub struct ReplayView {
    focus_handle: FocusHandle,
    path: PathBuf,
    movie: obake_controller::Movie,
    /// Runs of identical frames: (first frame, last frame, keys).
    spans: Arc<Vec<(u64, u64, String)>>,
    workspace: gpui::WeakEntity<Workspace>,
    /// Why the builds list couldn't be saved into the file.
    notice: Option<String>,
}

impl ReplayView {
    fn new(
        path: PathBuf,
        movie: obake_controller::Movie,
        workspace: gpui::WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut spans: Vec<(u64, u64, String)> = Vec::new();
        for (i, f) in movie.frames.iter().enumerate() {
            let keys = if f.keys.is_empty() {
                String::new()
            } else {
                f.keys.iter().map(|&k| key_name(k)).collect::<Vec<_>>().join(" + ")
            };
            match spans.last_mut() {
                Some(last) if last.2 == keys => last.1 = i as u64,
                _ => spans.push((i as u64, i as u64, keys)),
            }
        }
        Self { focus_handle: cx.focus_handle(), path, movie, spans: Arc::new(spans), workspace, notice: None }
    }

    fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Runs the inputs on `games`, each in a new pane to the right.
    fn run_on(&self, games: Vec<PathBuf>, window: &mut Window, cx: &mut App) {
        let spec = MovieSpec { recording: Recording::Read, path: Some(self.path.clone()), ..Default::default() };
        self.workspace
            .update(cx, |workspace, cx| {
                for game in games {
                    open_run(workspace, game, spec.clone(), true, window, cx);
                }
            })
            .ok();
    }

    /// The builds this replay is played on: saved with it, falling back to
    /// the one that recorded it.
    fn builds(&self) -> Vec<PathBuf> {
        if self.movie.builds.is_empty() {
            self.movie.game_path.iter().cloned().collect()
        } else {
            self.movie.builds.clone()
        }
    }

    fn set_builds(&mut self, builds: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.movie.builds = builds;
        self.notice = obake_controller::write_movie(&self.path, &mut self.movie).err();
        cx.notify();
    }

    fn add_builds(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else { return };
        let picked = workspace.update(cx, |ws, cx| pick_files(ws, "Add", true, window, cx));
        cx.spawn_in(window, async move |this, cx| {
            let games = picked.await;
            this.update(cx, |this, cx| {
                let mut builds = this.builds();
                for g in games {
                    if !builds.contains(&g) {
                        builds.push(g);
                    }
                }
                this.set_builds(builds, cx);
            })
            .ok();
        })
        .detach();
    }
}

impl EventEmitter<ItemEvent> for ReplayView {}

impl Focusable for ReplayView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for ReplayView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _: &App) -> SharedString {
        self.name().into()
    }

    fn tab_tooltip_text(&self, _: &App) -> Option<SharedString> {
        Some(self.path.display().to_string().into())
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }
}

impl Render for ReplayView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.movie;
        let summary = format!("{} frames", m.len());
        let builds = self.builds();
        let playable: Vec<PathBuf> = builds.iter().filter(|b| b.is_file()).cloned().collect();
        let spans = self.spans.clone();
        let rows = builds.into_iter().enumerate().map(|(i, build)| {
            let exists = build.is_file();
            let name = build.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = build.parent().map(|d| d.display().to_string()).unwrap_or_default();
            h_flex()
                .gap_3()
                .py_0p5()
                .child(
                    ui::Button::new(("play-build", i), "▶ Play")
                        .label_size(LabelSize::Small)
                        .disabled(!exists)
                        .on_click(cx.listener({
                            let build = build.clone();
                            move |this, _, window, cx| this.run_on(vec![build.clone()], window, cx)
                        })),
                )
                .child(Label::new(name).color(if exists { Color::Default } else { Color::Disabled }))
                .child(
                    Label::new(if exists { dir } else { format!("{dir}  (missing)") })
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(div().flex_1())
                .child(
                    IconButton::new(("remove-build", i), IconName::Close)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Remove from this replay"))
                        .on_click(cx.listener({
                            let build = build.clone();
                            move |this, _, _, cx| {
                                let builds = this.builds().into_iter().filter(|b| *b != build).collect();
                                this.set_builds(builds, cx);
                            }
                        })),
                )
        });
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .p_4()
            .gap_3()
            .child(Label::new(self.name()).size(LabelSize::Large))
            .child(Label::new(summary).size(LabelSize::Small).color(Color::Muted))
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new("Builds").color(Color::Muted))
                    .child(div().flex_1())
                    .when(playable.len() > 1, |d| {
                        d.child(
                            ui::Button::new("play-all", "▶ Play all side by side")
                                .style(ui::ButtonStyle::Filled)
                                .on_click(cx.listener({
                                    let playable = playable.clone();
                                    move |this, _, window, cx| this.run_on(playable.clone(), window, cx)
                                })),
                        )
                    })
                    .child(
                        ui::Button::new("add-build", "+ Add build…")
                            .on_click(cx.listener(|this, _, window, cx| this.add_builds(window, cx))),
                    ),
            )
            .child(v_flex().children(rows))
            .when_some(self.notice.clone(), |d, n| {
                d.child(Label::new(format!("Couldn't save the builds list: {n}")).size(LabelSize::Small).color(Color::Error))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().colors().border_variant)
                    .pt_2()
                    .child(
                        gpui::uniform_list("spans", spans.len(), move |range, _, _| {
                            range
                                .map(|i| {
                                    let (a, b, keys) = &spans[i];
                                    let frames = if a == b { format!("{a}") } else { format!("{a}–{b}") };
                                    h_flex()
                                        .gap_4()
                                        .child(div().w(gpui::px(110.)).child(
                                            Label::new(frames).size(LabelSize::Small).color(Color::Muted),
                                        ))
                                        .child(if keys.is_empty() {
                                            Label::new("—").size(LabelSize::Small).color(Color::Disabled)
                                        } else {
                                            Label::new(keys.clone()).size(LabelSize::Small)
                                        })
                                })
                                .collect()
                        })
                        .size_full(),
                    ),
            )
    }
}

/// X11 keysym -> short readable name.
fn key_name(sym: u32) -> String {
    match sym {
        0xff51 => "←".into(),
        0xff52 => "↑".into(),
        0xff53 => "→".into(),
        0xff54 => "↓".into(),
        0xff0d => "Enter".into(),
        0xff1b => "Esc".into(),
        0xff08 => "Backspace".into(),
        0xff09 => "Tab".into(),
        0x20 => "Space".into(),
        0xffbe..=0xffc9 => format!("F{}", sym - 0xffbe + 1),
        0x21..=0x7e => (sym as u8 as char).to_ascii_uppercase().to_string(),
        _ => format!("0x{sym:x}"),
    }
}

/// gpui key name -> X11 keysym.
fn keysym(key: &str) -> Option<u32> {
    Some(match key {
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "enter" => 0xff0d,
        "escape" => 0xff1b,
        "backspace" => 0xff08,
        "tab" => 0xff09,
        "space" => 0x20,
        k if k.len() == 1 && k.is_ascii() => k.as_bytes()[0].to_ascii_lowercase() as u32,
        k if k.starts_with('f') && k.len() <= 3 => 0xffbe + k[1..].parse::<u32>().ok()?.checked_sub(1)?,
        _ => return None,
    })
}
