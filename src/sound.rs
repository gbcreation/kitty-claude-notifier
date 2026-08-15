//! Notification sounds for state transitions that need your attention:
//! entering Permission/Waiting ("needs your input") and Done ("a turn
//! finished"). Plays via whichever system audio player happens to be
//! installed, with no audio library dependency, matching the rest of this
//! project's philosophy.
//!
//! The two built-in sounds are sourced from herdrdev/herdr (Apache
//! License 2.0); see `assets/sounds/NOTICE.md` for provenance and
//! attribution.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static SOUND_REQUEST: &[u8] = include_bytes!("../assets/sounds/request.mp3");
static SOUND_DONE: &[u8] = include_bytes!("../assets/sounds/done.mp3");
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Entered Permission or Waiting, needs your input.
    Request,
    /// Entered Done.
    Done,
}

/// Plays `sound` in a background thread (never blocks the caller). A
/// custom file path, if given, is tried first and falls back to the
/// built-in default on failure. Never actually attempts playback when
/// compiled for `cargo test`. This must stay a `cfg!(test)` check, not a
/// runtime toggle, so no test run can ever spawn a real audio process
/// regardless of config.
pub fn play(sound: Sound, custom_path: Option<PathBuf>) {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || {
        if let Some(path) = &custom_path
            && play_file(path).is_ok()
        {
            return;
        }
        let data = match sound {
            Sound::Request => SOUND_REQUEST,
            Sound::Done => SOUND_DONE,
        };
        if let Err(e) = play_bytes(data) {
            tracing::warn!(?sound, "sound playback failed: {e}");
        }
    });
}

fn play_file(path: &Path) -> Result<(), String> {
    run_player(path).and_then(check_success)
}

fn play_bytes(data: &[u8]) -> Result<(), String> {
    let tmp = temp_path();
    {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(data).map_err(|e| e.to_string())?;
    }
    let result = run_player(&tmp).and_then(check_success);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn check_success(output: Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        Err(format!("player exited with {}", output.status))
    } else {
        Err(format!("player exited with {}: {stderr}", output.status))
    }
}

fn temp_path() -> PathBuf {
    let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "kitty-claude-notifier-sound-{}-{id}.mp3",
        std::process::id()
    ))
}

#[cfg(target_os = "macos")]
fn run_player(path: &Path) -> Result<Output, String> {
    Command::new("afplay")
        .arg(path)
        .output()
        .map_err(|e| format!("no audio player available: {e}"))
}

#[cfg(not(target_os = "macos"))]
fn run_player(path: &Path) -> Result<Output, String> {
    const PLAYERS: &[(&str, &[&str])] = &[
        ("paplay", &[]),
        ("pw-play", &[]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
        ("mpg123", &["-q"]),
        ("mpv", &["--no-video", "--really-quiet"]),
    ];
    let mut last_err = String::from("no audio player available");
    for (program, args) in PLAYERS {
        match Command::new(program)
            .args(*args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(output) => return Ok(output),
            Err(e) => last_err = format!("{program}: {e}"),
        }
    }
    Err(last_err)
}
