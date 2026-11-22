# obake2 plan

## What it is
A functional RE agent harness for games (see README). It runs game binaries under
full control so that humans and agents can record, replay, rewind, instrument and
compare them. Correctness means matching behavior. Implementation details are free
to match or differ.

The first target is Downwell. We start from scratch: the existing downwell_rs is
treated as nonexistent.

## Decisions so far
- **Any two binaries.** obake compares whatever two binaries you give it. It doesn't
  care which pairing is in play: original vs clone, clone vs previous clone, patched
  vs unpatched are all the same operation.
- **Agents see everything**, including tools that look directly at the binary
  (disassembly, memory, symbols, hooks at addresses).
- **Bring your own harness.** obake never forces an agent harness and never calls a
  model itself. Any harness (Claude Code, Codex, pi, opencode, …) uses obake through
  its MCP server and CLI however it chooses. A faster, more tightly wired path for a
  specific harness may come later as an option, never as a requirement.
- **Human GUI: vendored Zed, then pruned.**
  - Zed's crates are copied into the obake2 repo, with no git link to upstream.
    Unneeded parts (collab, extension store, etc.) are deleted.
  - It is expected to diverge heavily from Zed. Roughly weekly, an AI compares the
    vendored crates with upstream and ports over ideas worth having; nothing is
    merged.
  - Zed's agent panel and ACP client are kept, so external harnesses can run inside
    the GUI. Zed's own built-in agent harness (`NativeAgentConnection`, the
    `crates/agent` tools) is not obake's agent path and is a pruning candidate.
  - The GPL/AGPL license notices of the vendored crates are kept.
- **License: GPLv3.** Lift libTAS code freely, line for line where it helps.
- **Language: "Crust"**, meaning C-style Rust. The shim uses `unsafe`, raw pointers
  and `extern "C"` hooks so that libTAS translates nearly line for line. The rest is
  ordinary Rust.
- **Rendering:** GPU, on one PC. Pixels have to match only on that machine. Across
  machines, the behavior trace (state, RNG, inputs) is what has to match.
- **No forced Xvfb and no forced software rendering.** Both are per-run options.
- **Live play runs in lockstep.** Both binaries advance frame by frame together and
  the movie is always recorded.
- **Audio comparison is out of scope.** Audio *determinism* is still required:
  Downwell's game logic depends on `audio_is_playing`. See "Findings" below.
- **The clone's tech is not obake's concern.** obake works with whatever the clone is
  built with, and the clone needs nothing special for it. The current clone stack
  (Crust, SDL3, probably Vulkan) only decides which shim modules come first.
- **No Windows/Wine support**, probably ever.
- **Room to grow.** The design keeps the option of supporting everything libTAS does,
  but only the Downwell pieces get implemented now.

## Architecture
```
 obake process (vendored Zed fork)          obake --headless
 +--------------------------------------+   +-------------------------+
 | GUI panes: frames, timeline, diff... |   | (no GUI)                |
 | agent panel / ACP  <-> agents        |   |                         |
 | MCP server (in-process)              |   | MCP server, CLI         |
 | controller (linked in-process)       |   | controller              |
 +--------------------------------------+   +-------------------------+
          - controller: runs, lockstep, movies, traces, compare, seek
          - frames are rendered inside the obake process
                 | one socket per game run (parallel runs ok)
          shim (Crust cdylib, LD_PRELOAD) inside each game process
          - core models: time, frame, input, video, audio, fs, threads, rng
          - API modules: X11, GLX, OpenAL (runner) | SDL3, Vulkan (clone) | later: more
```

### Seams to set up on day one
1. Core models are separate from the API hook modules. Adding an API means adding a
   module.
2. Where a frame ends is a pluggable rule (swap buffers, present, per-game config).
3. Input is a typed event stream that names its device; keyboard only for now.
4. The movie format and the shim–controller protocol are versioned from the first
   commit.
5. Time travel goes through `seek / checkpoint / restore`. Version 1 is replay from
   frame 0; libTAS's DMTCP-derived checkpointing gets ported later.
6. Unhooked calls pass through to the real function and are logged, so a new game
   shows what it needs.
7. The controller is the only owner of behavior. The CLI, MCP and GUI are all its
   clients.
8. Agent-facing results follow REA's contract: each result says what was observed,
   derived, inferred or unknown; answers are minimal and paged; full evidence is kept
   on disk behind an ID; tool descriptions are written so an agent can pick the right
   one from the opening lines.

## Milestones (Downwell from scratch)
- **M0 Skeleton.** Cargo workspace (shim, controller, cli). Build against host glibc.
  A `doctor` command that fails fast with a clear message.
- **M1 Time and frames.** Shim loads into `runner`. Virtual clock, including the
  rule that lets time pass when the game busy-waits (the startup softlock fix).
  Frame boundary at `glXSwapBuffers`. A hash of every frame. Exit criterion: the
  game run against itself twice gives 0 differences over an idle menu.
- **M2 Input and movies.** Insert X11 key events (`XPending`, `XNextEvent`,
  `XEventsQueued`, `XPeekEvent`, `XLookupString`). Movie format v1. Record and
  replay. Exit criterion: a recorded play-through replays identically, including on
  another machine as judged by the behavior trace.
- **M3 Audio determinism.**
  - Port libTAS's audio core near-exactly (`AudioContext`, `AudioSource`,
    `AudioBuffer`, `AudioConverterSwr`, `AudioPlayerAlsa`) plus its `openal/`
    module. This gives a virtual mixer that advances with frames and real sound
    output during live play. The one new dependency is libswresample. Other
    backends (ALSA, PulseAudio, SDL, FMOD, cubeb…) come later as modules on the
    same core.
  - GameMaker's mixer thread made to step in lockstep with frames: the number of
    512-sample steps that fit into each frame's virtual time.
  - Exit criterion: a long recorded run (with landings and recharge sounds) replays
    identically.
- **M4 Two binaries in lockstep.** Controller daemon. Compare two runs and report
  the first divergence (frame, bounding box, trace excerpt). SDL3 and Vulkan
  modules for the clone side, with `vkQueuePresentKHR` as its frame boundary.
- **M5 Instrumentation.** In-process memory reads, hooks at function addresses
  (e.g. log every GameMaker RNG draw with its caller), a per-frame trace format,
  binary-inspection tools for agents.
- **M6 MCP server** following the REA-style contract above. It is hosted in-process
  by the controller's owner (GUI or headless), so agents and the human act on the
  same live runs.
- **M7 Zed fork.**
  - Vendor and prune Zed, and link the controller in-process.
  - Panes: frame viewer, timeline and scrubbing, the two runs side by side, live
    lockstep play from the keyboard, agent annotations on the timeline.
  - External harnesses run in Zed's agent panel through ACP. Zed already forwards
    configured MCP servers to them (`mcp_servers_for_project` in
    `crates/agent_servers`), so obake's in-process MCP server is registered there.
- **M8 Real snapshots (maybe never).** Port libTAS checkpointing only if
  replay-seek proves too slow. libTAS's own FAQ says its savestates need software
  rendering, and we render on the GPU.

## Findings (2026-11-21, from a 10s strace of `runner` plus its strings)
- **OpenAL surface.** `runner` loads OpenAL at runtime (`dlopen`) and references only
  about 20 OpenAL functions. Most are device and context setup. The rest is a
  streaming queue: `alSourceQueueBuffers`, `alSourceUnqueueBuffers`, `alGetSourcei`,
  `alSourcePlay`, `alSourceStop`. GameMaker mixes audio itself.
- **No OpenAL on this machine.** `libopenal.so` isn't installed, so `runner` runs
  silent natively. Under libTAS it got libTAS's OpenAL, so behavior differed with and
  without libTAS.
- **Threads:**
  - Main thread: the game loop, paced by `clock_nanosleep` of 1ms in a loop.
  - An unnamed thread that wakes every ~11.67ms (512 samples at 44.1kHz). Inferred
    to be GameMaker's audio mixer. It runs even with no audio device and is paced by
    the real clock, which makes `audio_is_playing` depend on real time.
    *Hypothesis:* libTAS only virtualized the main thread's time, so this thread
    raced the game loop. That could explain the old `my-run` drift.
  - Mesa GPU driver threads (`disk$0`, `sh0`, `traceq0`, `gdrv0`). Not game logic.
  - One child process and two threads that exit at startup.
- **Snapshots.** Replay-seek runs at roughly 250fps (old figure, to be re-measured on
  the GPU): about 8s to reach frame 2,000, about 3.5min to reach the end of a
  15-minute run. libTAS savestates require software rendering per its FAQ, so they
  are deferred.
- **libTAS defaults to software rendering** (`opengl_soft = true` in
  `SharedConfig.h`). The old obake inherited that default.

## Open questions
- **The faster wired-harness option (later).** What it looks like, e.g. shared-memory
  frames or a direct in-process tool path, gets decided when a real bottleneck
  shows up.
- **What to prune from Zed.** Decide crate by crate when vendoring. Probably cut:
  collab/server, extension store, auto-update, telemetry.
- **Confirm the mixer thread's identity** with a backtrace (the binary is stripped,
  so find it by address) before M3.
- **Measure replay speed** on the GPU with obake's frame pacing removed.
