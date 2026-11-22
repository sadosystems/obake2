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
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement,
    ObjectFit, PathPromptOptions, Render, RenderImage, SharedString, Styled, StyledImage as _,
    Task, Window, div, img,
};
use obake_controller::{LaunchSpec, Run, RunState, RunStatus};
use project::DirectoryLister;
use ui::{Color, IconButton, IconButtonShape, IconName, IconSize, Label, Tooltip, LabelCommon as _, LabelSize, prelude::*};
use workspace::{
    Item, OpenPathPromptMode, Toast, Workspace, item::ItemEvent, notifications::NotificationId,
};
use obake_actions::obake::{LaunchGame, StepFrame, StopGame, TogglePause};

pub fn init(cx: &mut App) {
    // Obake's in-app file browser, used when system dialogs are off or fail.
    // Its registration lived in a crate removed when pruning.
    cx.observe_new(open_path_prompt::OpenPathPrompt::register)
        .detach();
    cx.observe_new(open_path_prompt::OpenPathPrompt::register_new_path)
        .detach();
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        // Dev hook: OBAKE_AUTOLAUNCH=<binary> launches it in the first window.
        static AUTOLAUNCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if let (Some(window), Ok(path)) = (window, std::env::var("OBAKE_AUTOLAUNCH")) {
            if !AUTOLAUNCHED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                cx.spawn_in(window, async move |ws, cx| {
                    ws.update_in(cx, |ws, window, cx| open_run(ws, path.into(), window, cx)).ok();
                })
                .detach();
            }
        }
        workspace.register_action(launch_game);
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

fn launch_game(
    workspace: &mut Workspace,
    _: &LaunchGame,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let lister = DirectoryLister::Local(workspace.project().clone(), workspace.app_state().fs.clone());
    let paths = workspace.prompt_for_open_path(
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Launch".into()),
        },
        lister,
        OpenPathPromptMode::SelectPath,
        window,
        cx,
    );
    cx.spawn_in(window, async move |workspace, cx| {
        let Some(path) = paths
            .await
            .ok()
            .flatten()
            .and_then(|r| r.paths.into_iter().next())
        else {
            return;
        };
        workspace
            .update_in(cx, |workspace, window, cx| {
                open_run(workspace, path, window, cx);
            })
            .ok();
    })
    .detach();
}

/// Launches `path` and shows it in the active pane.
pub fn open_run(
    workspace: &mut Workspace,
    path: PathBuf,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    match Run::launch(LaunchSpec::for_binary(&path)) {
        Ok(run) => {
            // Dev hook: OBAKE_AUTOLAUNCH_FF=1 starts in fast-forward.
            if std::env::var("OBAKE_AUTOLAUNCH_FF").is_ok() {
                run.set_fastforward(true);
            }
            let view = cx.new(|cx| GameView::new(run, path, cx));
            workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
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
    _poll: Task<()>,
}

impl GameView {
    fn new(run: Run, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let poll_run = run.clone();
        let poll = cx.spawn(async move |this, cx| {
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
                        (d, run.latest_frame(seen_seq))
                    })
                    .await;
                seen_draws = draws;
                if let Some(f) = &frame {
                    seen_seq = f.seq;
                }
                let alive = this.update(cx, |this, cx| {
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
                    {
                        this.status = s;
                        cx.notify();
                    }
                    !matches!(this.status.state, RunState::Exited(_))
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        });
        Self {
            focus_handle: cx.focus_handle(),
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
        self.title.clone()
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
