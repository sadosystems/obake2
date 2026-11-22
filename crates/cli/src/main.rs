//! obake CLI: a thin client of the controller.

use obake_controller::{doctor, LaunchSpec, Run, RunState, RunStatus};
use std::io::BufRead;
use std::time::Duration;

const USAGE: &str = "usage:
  obake doctor                 check the environment
  obake run <binary> [args..]  launch under the shim; stdin commands:
                               p = pause/play, s = step, f = fast-forward, q = quit
  obake smoke <binary>         launch, then verify pause / step / play / stop";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("doctor") => cmd_doctor(),
        Some("run") if args.len() >= 2 => cmd_run(&args[1], &args[2..]),
        Some("smoke") if args.len() >= 2 => cmd_smoke(&args[1]),
        Some("framebench") if args.len() >= 2 => cmd_framebench(&args[1]),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn cmd_doctor() -> i32 {
    let mut ok = true;
    for (name, r) in doctor() {
        match r {
            Ok(v) => println!("ok    {name}: {v}"),
            Err(e) => {
                ok = false;
                println!("FAIL  {name}: {e}");
            }
        }
    }
    if ok { 0 } else { 1 }
}

fn launch(bin: &str, args: &[String]) -> Result<Run, i32> {
    let mut spec = LaunchSpec::for_binary(bin);
    spec.args = args.to_vec();
    Run::launch(spec).map_err(|e| {
        eprintln!("error: {e}");
        1
    })
}

fn line(s: &RunStatus) -> String {
    format!(
        "{:?} frame={} draws={} t={:.3}s pid={}",
        s.state,
        s.frame,
        s.draw_frames,
        s.virtual_ns as f64 / 1e9,
        s.game_pid
    )
}

fn cmd_run(bin: &str, args: &[String]) -> i32 {
    let run = match launch(bin, args) {
        Ok(r) => r,
        Err(c) => return c,
    };
    eprintln!("game log: {}", run.status().log_path.display());
    let r = run.clone();
    std::thread::spawn(move || {
        for l in std::io::stdin().lock().lines() {
            match l.unwrap_or_default().trim() {
                "p" => r.toggle_pause(),
                "s" => r.step(),
                "q" => r.stop(),
                "f" => r.set_fastforward(!r.status().fastforward),
                _ => {}
            }
            eprintln!("{}", line(&r.status()));
        }
    });
    loop {
        let s = run.wait_for(Duration::from_secs(1), |s| matches!(s.state, RunState::Exited(_)));
        if let RunState::Exited(code) = s.state {
            eprintln!("{}", line(&s));
            return code.unwrap_or(1);
        }
    }
}

fn cmd_smoke(bin: &str) -> i32 {
    let run = match launch(bin, &[]) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let fail = |msg: &str, s: &RunStatus| {
        eprintln!("FAIL: {msg}: {}", line(s));
        eprintln!("game log: {}", s.log_path.display());
        run.kill();
        1
    };
    let s = run.wait_for(Duration::from_secs(30), |s| s.frame >= 120 || matches!(s.state, RunState::Exited(_)));
    if s.frame < 120 {
        return fail("did not reach frame 120", &s);
    }
    println!("running:  {}", line(&s));

    run.pause();
    let s = run.wait_for(Duration::from_secs(2), |s| s.state == RunState::Paused);
    if s.state != RunState::Paused {
        return fail("did not pause", &s);
    }
    std::thread::sleep(Duration::from_secs(1));
    let s2 = run.status();
    if s2.frame != s.frame || s2.virtual_ns != s.virtual_ns {
        return fail("frame or virtual time moved while paused", &s2);
    }
    println!("paused:   {}", line(&s2));

    match run.latest_frame(0) {
        Some(f) => {
            let ppm = run.dir.join("paused.ppm");
            let mut out = format!("P6 {} {} 255\n", f.width, f.height).into_bytes();
            for px in f.bgra.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            }
            let _ = std::fs::write(&ppm, out);
            println!("frame:    {}x{} game frame {} -> {}", f.width, f.height, f.framecount, ppm.display());
        }
        None => return fail("no captured frame", &s2),
    }

    for _ in 0..3 {
        run.step();
    }
    let target = s2.frame + 3;
    let s3 = run.wait_for(Duration::from_secs(5), |s| s.frame >= target);
    std::thread::sleep(Duration::from_millis(300));
    let s3b = run.status();
    if s3.frame != target || s3b.frame != target {
        return fail(&format!("expected exactly frame {target} after 3 steps"), &s3b);
    }
    println!("stepped:  {}", line(&s3b));

    run.play();
    let s4 = run.wait_for(Duration::from_secs(5), |s| s.frame >= target + 60);
    if s4.frame < target + 60 {
        return fail("did not resume", &s4);
    }
    println!("resumed:  {}", line(&s4));

    run.stop();
    let s5 = run.wait_for(Duration::from_secs(5), |s| matches!(s.state, RunState::Exited(_)));
    if !matches!(s5.state, RunState::Exited(_)) {
        return fail("did not exit", &s5);
    }
    println!("stopped:  {}", line(&s5));
    println!("smoke ok");
    0
}

/// Reads frames the way the GUI does and reports how many arrive per second.
fn cmd_framebench(bin: &str) -> i32 {
    let run = match launch(bin, &[]) {
        Ok(r) => r,
        Err(c) => return c,
    };
    run.wait_for(Duration::from_secs(30), |s| s.frame >= 300);
    let (mut draws, mut seq, mut got, mut missed) = (run.status().draw_frames, 0, 0, 0);
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        draws = run.wait_draw(draws, Duration::from_millis(100));
        match run.latest_frame(seq) {
            Some(f) => {
                seq = f.seq;
                got += 1;
            }
            None => missed += 1,
        }
    }
    println!("frames read: {:.1}/s, misses: {missed}", got as f64 / 3.0);
    run.kill();
    0
}
