//! Piece and button cues.
//!
//! Samples are Kenney's Interface Sounds and Impact Sounds (CC0 — see
//! `assets/sounds/CREDITS.txt`), embedded as WAV. Playback is the system
//! player (`paplay`, `pw-play`, `aplay`, `ffplay`, `mpv` on Linux), so the
//! crate takes no audio dependency. A missing player is ignored.
//! `PICHESS_DISABLE_SOUND` silences every cue.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::position::Move;

const DISABLE_ENV: &str = "PICHESS_DISABLE_SOUND";

static MOVE_0: &[u8] = include_bytes!("../assets/sounds/move-0.wav");
static MOVE_1: &[u8] = include_bytes!("../assets/sounds/move-1.wav");
static MOVE_2: &[u8] = include_bytes!("../assets/sounds/move-2.wav");
static TAKE_0: &[u8] = include_bytes!("../assets/sounds/take-0.wav");
static TAKE_1: &[u8] = include_bytes!("../assets/sounds/take-1.wav");
static CLICK: &[u8] = include_bytes!("../assets/sounds/click.wav");
static SELECT: &[u8] = include_bytes!("../assets/sounds/select.wav");
static BACK: &[u8] = include_bytes!("../assets/sounds/back.wav");
static TOGGLE: &[u8] = include_bytes!("../assets/sounds/toggle.wav");
static OPEN: &[u8] = include_bytes!("../assets/sounds/open.wav");
static ERROR: &[u8] = include_bytes!("../assets/sounds/error.wav");
static CONFIRM: &[u8] = include_bytes!("../assets/sounds/confirm.wav");
static DOWN: &[u8] = include_bytes!("../assets/sounds/down.wav");
static CHECK: &[u8] = include_bytes!("../assets/sounds/check.wav");
static PROMOTE: &[u8] = include_bytes!("../assets/sounds/promote.wav");
static TICK: &[u8] = include_bytes!("../assets/sounds/tick.wav");

static QUIET_I: AtomicUsize = AtomicUsize::new(0);
static CAPTURE_I: AtomicUsize = AtomicUsize::new(0);
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const QUIET: &[&[u8]] = &[MOVE_0, MOVE_1, MOVE_2];
const CAPTURES: &[&[u8]] = &[TAKE_0, TAKE_1];

/// How a piece lands. Castle plays two quiet knocks a moment apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveBody {
    Quiet,
    Capture,
    Castle,
    Promotion,
}

/// What the landing means, beyond the piece itself. An ending or a puzzle
/// result replaces a bare check, so a mate does not also ring the bell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAccent {
    None,
    Check,
    Solved,
    Miss,
    Win,
    Loss,
}

/// Button and overlay cues. Piece landings go through [`play_landed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Click,
    Back,
    Toggle,
    Open,
    Error,
    Confirm,
    Down,
    Select,
    Tick,
}

/// Why the move that just landed should accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The game goes on. `check` is the opponent to move being in check.
    Play {
        check: bool,
    },
    /// Checkmate. `winner_is_computer` is false in a two-player game.
    Mate {
        winner_is_computer: bool,
    },
    Draw,
    PuzzleSolved,
    PuzzleMiss,
}

/// Body from the move, accent from how the position (or puzzle) came out.
pub fn cue_for_move(mv: Move, captures: bool, outcome: Outcome) -> (MoveBody, MoveAccent) {
    let body = match mv {
        Move::Castle { .. } => MoveBody::Castle,
        Move::Promotion { .. } => MoveBody::Promotion,
        Move::EnPassant { .. } => MoveBody::Capture,
        Move::Quiet { .. } if captures => MoveBody::Capture,
        Move::Quiet { .. } => MoveBody::Quiet,
    };
    let accent = match outcome {
        Outcome::PuzzleMiss => MoveAccent::Miss,
        Outcome::PuzzleSolved => MoveAccent::Solved,
        Outcome::Mate {
            winner_is_computer: true,
        } => MoveAccent::Loss,
        Outcome::Mate {
            winner_is_computer: false,
        } => MoveAccent::Win,
        Outcome::Draw => MoveAccent::None,
        Outcome::Play { check: true } => MoveAccent::Check,
        Outcome::Play { check: false } => MoveAccent::None,
    };
    (body, accent)
}

/// Second to chime on a rush clock, once, while ten or fewer seconds remain.
/// `None` means stay quiet. The `Some` value is what the caller remembers.
pub fn rush_chime(secs_left: u64, already: Option<u64>) -> Option<u64> {
    if !(1..=10).contains(&secs_left) || already == Some(secs_left) {
        return None;
    }
    Some(secs_left)
}

/// Play a button cue. No-op when sound is disabled.
pub fn play(sound: Sound) {
    let Some(data) = sound_bytes(sound) else {
        return;
    };
    spawn_play(data);
}

/// Play the landing, and any accent over the top of it.
pub fn play_landed(body: MoveBody, accent: MoveAccent) {
    if disabled() {
        return;
    }
    let first = body_sample(body);
    let second = if body == MoveBody::Castle {
        Some(body_sample(MoveBody::Quiet))
    } else {
        None
    };
    std::thread::spawn(move || {
        if let Some(second) = second {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                let _ = play_bytes(second);
            });
        }
        let _ = play_bytes(first);
    });
    if let Some(data) = accent_bytes(accent) {
        spawn_play(data);
    }
}

fn sound_bytes(sound: Sound) -> Option<&'static [u8]> {
    if disabled() {
        return None;
    }
    Some(match sound {
        Sound::Click => CLICK,
        Sound::Back => BACK,
        Sound::Toggle => TOGGLE,
        Sound::Open => OPEN,
        Sound::Error => ERROR,
        Sound::Confirm => CONFIRM,
        Sound::Down => DOWN,
        Sound::Select => SELECT,
        Sound::Tick => TICK,
    })
}

fn body_sample(body: MoveBody) -> &'static [u8] {
    match body {
        MoveBody::Quiet | MoveBody::Castle => next_of(QUIET, &QUIET_I),
        MoveBody::Capture => next_of(CAPTURES, &CAPTURE_I),
        MoveBody::Promotion => PROMOTE,
    }
}

fn accent_bytes(accent: MoveAccent) -> Option<&'static [u8]> {
    Some(match accent {
        MoveAccent::None => return None,
        MoveAccent::Check => CHECK,
        MoveAccent::Solved | MoveAccent::Win => CONFIRM,
        MoveAccent::Miss => ERROR,
        MoveAccent::Loss => DOWN,
    })
}

fn next_of<'a>(samples: &[&'a [u8]], cursor: &AtomicUsize) -> &'a [u8] {
    let i = cursor.fetch_add(1, Ordering::Relaxed);
    samples[i % samples.len()]
}

fn disabled() -> bool {
    std::env::var_os(DISABLE_ENV).is_some()
}

fn spawn_play(data: &'static [u8]) {
    std::thread::spawn(move || {
        let _ = play_bytes(data);
    });
}

fn play_bytes(data: &[u8]) -> Result<(), String> {
    let tmp = temp_path();
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let result = run_player(&tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn temp_path() -> PathBuf {
    let id = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pichess-sound-{}-{id}.wav", std::process::id()))
}

#[cfg(target_os = "macos")]
fn run_player(path: &Path) -> Result<(), String> {
    run_checked("afplay", &[], path)
}

#[cfg(windows)]
fn run_player(path: &Path) -> Result<(), String> {
    let script = format!(
        "(New-Object Media.SoundPlayer '{}').PlaySync()",
        path.display()
    );
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|e| format!("powershell failed: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("powershell exited with {}", output.status))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn run_player(path: &Path) -> Result<(), String> {
    let players: &[(&str, &[&str])] = &[
        ("paplay", &[]),
        ("pw-play", &[]),
        ("aplay", &["-q"]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
        ("mpv", &["--no-video", "--really-quiet"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in players {
        match run_checked(program, args, path) {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(err),
        }
    }
    Err(format!("no audio player available: {}", errors.join("; ")))
}

#[cfg(not(windows))]
fn run_checked(program: &str, args: &[&str], path: &Path) -> Result<(), String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{program} exited with {status}")),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{program} timed out"));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program}: {err}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::{PieceKind, Square};

    fn quiet() -> Move {
        Move::Quiet {
            from: Square::new(1, 4),
            to: Square::new(3, 4),
        }
    }

    #[test]
    fn quiet_move_is_only_a_knock() {
        let (body, accent) = cue_for_move(quiet(), false, Outcome::Play { check: false });
        assert_eq!(body, MoveBody::Quiet);
        assert_eq!(accent, MoveAccent::None);
    }

    #[test]
    fn capture_with_check_uses_both() {
        let (body, accent) = cue_for_move(quiet(), true, Outcome::Play { check: true });
        assert_eq!(body, MoveBody::Capture);
        assert_eq!(accent, MoveAccent::Check);
    }

    #[test]
    fn mate_replaces_the_check_bell() {
        let (_, win) = cue_for_move(
            quiet(),
            true,
            Outcome::Mate {
                winner_is_computer: false,
            },
        );
        let (_, loss) = cue_for_move(
            quiet(),
            false,
            Outcome::Mate {
                winner_is_computer: true,
            },
        );
        assert_eq!(win, MoveAccent::Win);
        assert_eq!(loss, MoveAccent::Loss);
    }

    #[test]
    fn castle_en_passant_and_promotion_pick_their_body() {
        let castle = Move::Castle {
            from: Square::new(0, 4),
            to: Square::new(0, 6),
        };
        let ep = Move::EnPassant {
            from: Square::new(4, 4),
            to: Square::new(5, 3),
        };
        let promo = Move::Promotion {
            from: Square::new(6, 0),
            to: Square::new(7, 1),
            kind: PieceKind::Queen,
        };
        assert_eq!(
            cue_for_move(castle, false, Outcome::Play { check: false }).0,
            MoveBody::Castle
        );
        assert_eq!(cue_for_move(ep, true, Outcome::Draw).0, MoveBody::Capture);
        let (body, accent) = cue_for_move(promo, true, Outcome::PuzzleSolved);
        assert_eq!(body, MoveBody::Promotion);
        assert_eq!(accent, MoveAccent::Solved);
    }

    #[test]
    fn a_puzzle_miss_outranks_check() {
        let (_, accent) = cue_for_move(quiet(), false, Outcome::PuzzleMiss);
        assert_eq!(accent, MoveAccent::Miss);
    }

    #[test]
    fn a_draw_keeps_the_landing_only() {
        let (_, accent) = cue_for_move(quiet(), false, Outcome::Draw);
        assert_eq!(accent, MoveAccent::None);
    }

    #[test]
    fn rush_clock_chimes_once_per_second_under_ten() {
        assert_eq!(rush_chime(11, None), None);
        assert_eq!(rush_chime(10, None), Some(10));
        assert_eq!(rush_chime(10, Some(10)), None);
        assert_eq!(rush_chime(9, Some(10)), Some(9));
        assert_eq!(rush_chime(0, Some(1)), None);
    }

    #[test]
    fn samples_cycle() {
        let cursor = AtomicUsize::new(0);
        let samples = [b"a".as_slice(), b"b".as_slice()];
        assert_eq!(next_of(&samples, &cursor), b"a");
        assert_eq!(next_of(&samples, &cursor), b"b");
        assert_eq!(next_of(&samples, &cursor), b"a");
    }

    #[test]
    fn embedded_cues_are_wav() {
        for data in [
            MOVE_0, MOVE_1, MOVE_2, TAKE_0, TAKE_1, CLICK, SELECT, BACK, TOGGLE, OPEN, ERROR,
            CONFIRM, DOWN, CHECK, PROMOTE, TICK,
        ] {
            assert!(data.len() > 44);
            assert_eq!(&data[..4], b"RIFF");
            assert_eq!(&data[8..12], b"WAVE");
        }
    }

    #[test]
    fn temp_paths_are_unique() {
        assert_ne!(temp_path(), temp_path());
    }
}
