//! Tactics puzzles: a stored line from a real game, graded without an engine.
//!
//! The line follows the Lichess convention. The FEN is the position before
//! the setup move, `moves[0]` is the opponent's move (played for you), and
//! every later odd ply is the only accepted solver move. Nothing here asks
//! the game engine whether another move also wins.

use std::collections::HashSet;

use serde::Deserialize;

use crate::piece::Color;
use crate::position::{Move, Position};

/// One puzzle, trimmed from a Lichess database row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Puzzle {
    pub id: String,
    pub fen: String,
    pub moves: Vec<String>,
    pub rating: i32,
    pub popularity: i32,
    pub themes: Vec<String>,
}

/// A puzzle checked against the rules: the starting position and the line
/// that has to be played, including the setup move at index 0.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub start: Position,
    pub line: Vec<Move>,
    /// The side that has to find the moves (to move after the setup ply).
    pub solver: Color,
}

/// What the solver's move did to the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Right move, and the opponent still has a reply in the line.
    Continue,
    /// Right move, and the line is over.
    Solved,
    /// Not the move in the line.
    Wrong,
}

#[derive(Debug, Deserialize)]
struct Row {
    id: String,
    fen: String,
    moves: String,
    rating: i32,
    #[serde(default)]
    popularity: i32,
    #[serde(default)]
    themes: String,
}

/// Parse JSON-lines. A bad line is skipped so one corrupt row cannot blank the pack.
pub fn parse_pack(text: &str) -> Vec<Puzzle> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let row: Row = serde_json::from_str(line).ok()?;
            let moves: Vec<String> = row.moves.split_whitespace().map(str::to_string).collect();
            if row.id.is_empty() || moves.len() < 2 {
                return None;
            }
            Some(Puzzle {
                id: row.id,
                fen: row.fen,
                moves,
                rating: row.rating,
                popularity: row.popularity,
                themes: row.themes.split_whitespace().map(str::to_string).collect(),
            })
        })
        .collect()
}

/// The pack shipped with the app. CC0 data from the Lichess puzzle database;
/// see `assets/puzzles/SOURCE.md`.
pub fn starter_pack() -> Vec<Puzzle> {
    parse_pack(include_str!("../assets/puzzles/starter.jsonl"))
}

/// Play the line on a copy of the FEN. `None` if a move is illegal, so a bad
/// row never reaches the board.
pub fn prepare(puzzle: &Puzzle) -> Option<Prepared> {
    if puzzle.moves.len() < 2 {
        return None;
    }
    let start = Position::from_fen(&puzzle.fen).ok()?;
    let mut pos = start.clone();
    let mut line = Vec::with_capacity(puzzle.moves.len());
    for uci in &puzzle.moves {
        let mv = pos.parse_uci(uci)?;
        if !pos.legal_moves().contains(&mv) {
            return None;
        }
        pos.make_move(mv).ok()?;
        line.push(mv);
    }
    let mut after_setup = start.clone();
    after_setup.make_move(line[0]).ok()?;
    Some(Prepared {
        start,
        line,
        solver: after_setup.turn,
    })
}

/// Grade `played` against the line at `index`. The setup move and the
/// opponent's replies are part of the line; the caller only submits when it
/// is the solver's turn.
pub fn grade(line: &[Move], index: usize, played: Move) -> Verdict {
    if index >= line.len() || line[index] != played {
        return Verdict::Wrong;
    }
    if index + 1 >= line.len() {
        Verdict::Solved
    } else {
        Verdict::Continue
    }
}

/// One-player Elo step toward the puzzle's rating. K is 24, clamped so a
/// long session cannot walk the number off the scale of the pack.
pub fn adjust_rating(rating: i32, puzzle_rating: i32, solved: bool) -> i32 {
    let expected = 1.0 / (1.0 + 10f64.powf((f64::from(puzzle_rating) - f64::from(rating)) / 400.0));
    let score = if solved { 1.0 } else { 0.0 };
    let next = (f64::from(rating) + 24.0 * (score - expected)).round() as i32;
    next.clamp(400, 2800)
}

const BANDS: [i32; 5] = [150, 300, 500, 800, 100_000];
const RUSH_SPAN: [i32; 4] = [250, 500, 1000, 100_000];

/// How a puzzle session chooses its next row. The grader does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PickFilter {
    /// Keep puzzles tagged with this Lichess theme (`"fork"`, `"endgame"`, …).
    pub theme: Option<&'static str>,
    /// Rush: prefer puzzles at or above this rating, and climb as it rises.
    pub floor: Option<i32>,
}

/// Which puzzle session is on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// Endless, near the local rating.
    Rated,
    /// Three minutes, difficulty climbs, three misses end the run.
    Rush,
    /// Rated queue limited to one motif.
    Theme,
    /// One puzzle for the civil day, same one until tomorrow.
    Daily,
}

/// Motifs the pack actually contains, in the order the theme button cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Fork,
    Pin,
    Skewer,
    Discovered,
    MateIn2,
    Endgame,
}

impl Theme {
    pub const ALL: [Theme; 6] = [
        Theme::Fork,
        Theme::Pin,
        Theme::Skewer,
        Theme::Discovered,
        Theme::MateIn2,
        Theme::Endgame,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Theme::Fork => "fork",
            Theme::Pin => "pin",
            Theme::Skewer => "skewer",
            Theme::Discovered => "discoveredAttack",
            Theme::MateIn2 => "mateIn2",
            Theme::Endgame => "endgame",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Fork => "Fork",
            Theme::Pin => "Pin",
            Theme::Skewer => "Skewer",
            Theme::Discovered => "Discovered",
            Theme::MateIn2 => "Mate in 2",
            Theme::Endgame => "Endgame",
        }
    }

    pub fn cycle(self) -> Theme {
        let index = Self::ALL
            .iter()
            .position(|theme| *theme == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

/// Misses that end a rush, and how long a rush lasts.
pub const RUSH_STRIKES: u8 = 3;
pub const RUSH_SECS: u64 = 180;

/// Score and strikes after one rush puzzle. `over` is the third miss.
pub fn rush_step(score: u32, strikes: u8, solved: bool) -> (u32, u8, bool) {
    if solved {
        (score.saturating_add(1), strikes, false)
    } else {
        let strikes = strikes.saturating_add(1);
        (score, strikes, strikes >= RUSH_STRIKES)
    }
}

/// Rating floor for the next rush puzzle. Starts a step below the player
/// and climbs with each solve, so the run gets harder without a new grader.
pub fn rush_floor(player_rating: i32, solved: u32) -> i32 {
    let base = (player_rating - 150).clamp(600, 2200);
    (base + solved as i32 * 80).min(2400)
}

/// Index of the next puzzle near `rating`, skipping ids in `seen`.
/// Bands widen until something matches. `None` means every eligible puzzle
/// has been seen — the caller clears `seen` and asks again.
///
/// `filter.floor` switches the band to "at least this hard" for a rush.
/// `filter.theme` keeps only puzzles carrying that tag.
pub fn pick_index(
    pack: &[Puzzle],
    rating: i32,
    seen: &HashSet<&str>,
    seed: u64,
    filter: PickFilter,
) -> Option<usize> {
    if let Some(floor) = filter.floor {
        for span in RUSH_SPAN {
            let found = collect(pack, seen, seed, filter.theme, |puzzle| {
                puzzle.rating + 50 >= floor && puzzle.rating <= floor + span
            });
            if found.is_some() {
                return found;
            }
        }
        return None;
    }
    for band in BANDS {
        let found = collect(pack, seen, seed, filter.theme, |puzzle| {
            (puzzle.rating - rating).abs() <= band
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The puzzle for `day`. Sorted by id, so the same pack and the same day
/// always land on the same row. `day` is any integer that changes once a day.
pub fn daily_index(pack: &[Puzzle], day: u64) -> Option<usize> {
    let mut idxs: Vec<usize> = pack
        .iter()
        .enumerate()
        .filter(|(_, puzzle)| puzzle.popularity >= 60)
        .map(|(index, _)| index)
        .collect();
    if idxs.is_empty() {
        return None;
    }
    idxs.sort_by(|&a, &b| pack[a].id.cmp(&pack[b].id));
    let slot = day.wrapping_mul(0x9E37_79B9_7F4A_7C15) as usize % idxs.len();
    Some(idxs[slot])
}

fn collect(
    pack: &[Puzzle],
    seen: &HashSet<&str>,
    seed: u64,
    theme: Option<&str>,
    band: impl Fn(&Puzzle) -> bool,
) -> Option<usize> {
    let mut idxs: Vec<usize> = pack
        .iter()
        .enumerate()
        .filter(|(_, puzzle)| {
            puzzle.popularity >= 60
                && !seen.contains(puzzle.id.as_str())
                && theme.is_none_or(|tag| puzzle.themes.iter().any(|theme| theme == tag))
                && band(puzzle)
        })
        .map(|(index, _)| index)
        .collect();
    if idxs.is_empty() {
        return None;
    }
    shuffle(&mut idxs, seed);
    Some(idxs[0])
}

fn shuffle(indices: &mut [usize], seed: u64) {
    let mut state = seed | 1;
    for i in (1..indices.len()).rev() {
        state = state.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        let j = (state >> 33) as usize % (i + 1);
        indices.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mate_in_two() -> Puzzle {
        Puzzle {
            id: "00sHx".into(),
            fen: "q3k1nr/1pp1nQpp/3p4/1P2p3/4P3/B1PP1b2/B5PP/5K2 b k - 0 17".into(),
            moves: vec!["e8d7".into(), "a2e6".into(), "d7d8".into(), "f7f8".into()],
            rating: 1760,
            popularity: 83,
            themes: vec!["mate".into(), "mateIn2".into()],
        }
    }

    fn promotion_line() -> Puzzle {
        Puzzle {
            id: "promo".into(),
            fen: "4k3/4P3/8/8/8/8/8/4K3 b - - 0 1".into(),
            moves: vec!["e8d7".into(), "e7e8q".into()],
            rating: 900,
            popularity: 90,
            themes: vec!["promotion".into()],
        }
    }

    #[test]
    fn mate_line_grades_only_the_key_moves() {
        let prepared = prepare(&mate_in_two()).expect("line plays");
        assert_eq!(prepared.solver, Color::White);
        assert_eq!(prepared.line.len(), 4);
        let user = prepared.line[1];
        assert_eq!(grade(&prepared.line, 1, user), Verdict::Continue);
        let other = prepared.line[3];
        assert_eq!(grade(&prepared.line, 1, other), Verdict::Wrong);
        assert_eq!(grade(&prepared.line, 3, prepared.line[3]), Verdict::Solved);
    }

    #[test]
    fn promotion_uci_is_the_graded_move() {
        let prepared = prepare(&promotion_line()).expect("promotion plays");
        assert!(prepared.line[1].promotion().is_some());
        assert_eq!(grade(&prepared.line, 1, prepared.line[1]), Verdict::Solved);
        let rook = match prepared.line[1] {
            Move::Promotion { from, to, .. } => Move::Promotion {
                from,
                to,
                kind: crate::piece::PieceKind::Rook,
            },
            other => other,
        };
        assert_eq!(grade(&prepared.line, 1, rook), Verdict::Wrong);
    }

    #[test]
    fn short_or_illegal_line_is_rejected() {
        let mut puzzle = mate_in_two();
        puzzle.moves.truncate(1);
        assert!(prepare(&puzzle).is_none());
        puzzle.moves = vec!["e8d7".into(), "a2a3".into()];
        assert!(prepare(&puzzle).is_none());
    }

    #[test]
    fn parse_pack_skips_a_bad_line() {
        let text = "\n{\"id\":\"a\",\"fen\":\"4k3/4P3/8/8/8/8/8/4K3 b - - 0 1\",\"moves\":\"e8d7 e7e8q\",\"rating\":900,\"popularity\":90,\"themes\":\"promotion\"}\nnot json\n{\"id\":\"\",\"fen\":\"x\",\"moves\":\"a2a3\",\"rating\":1}\n";
        let pack = parse_pack(text);
        assert_eq!(pack.len(), 1);
        assert_eq!(pack[0].id, "a");
        assert_eq!(pack[0].themes, vec!["promotion".to_string()]);
    }

    #[test]
    fn pick_skips_seen_and_widens() {
        let pack = vec![
            Puzzle {
                id: "near".into(),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 1200,
                popularity: 80,
                themes: Vec::new(),
            },
            Puzzle {
                id: "far".into(),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 2200,
                popularity: 80,
                themes: Vec::new(),
            },
        ];
        let mut seen = HashSet::new();
        let filter = PickFilter::default();
        assert_eq!(pick_index(&pack, 1200, &seen, 1, filter), Some(0));
        seen.insert("near");
        assert_eq!(pick_index(&pack, 1200, &seen, 1, filter), Some(1));
        seen.insert("far");
        assert_eq!(pick_index(&pack, 1200, &seen, 1, filter), None);
    }

    #[test]
    fn theme_filter_and_rush_floor_narrow_the_pack() {
        let pack = vec![
            Puzzle {
                id: "fork-easy".into(),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 900,
                popularity: 80,
                themes: vec!["fork".into()],
            },
            Puzzle {
                id: "fork-hard".into(),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 1600,
                popularity: 80,
                themes: vec!["fork".into()],
            },
            Puzzle {
                id: "pin".into(),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 1600,
                popularity: 80,
                themes: vec!["pin".into()],
            },
        ];
        let seen = HashSet::new();
        let themed = PickFilter {
            theme: Some("fork"),
            floor: None,
        };
        assert_eq!(pick_index(&pack, 1600, &seen, 1, themed), Some(1));
        let rush = PickFilter {
            theme: None,
            floor: Some(1500),
        };
        let index = pick_index(&pack, 1200, &seen, 1, rush).unwrap();
        assert_ne!(pack[index].id, "fork-easy");
    }

    #[test]
    fn daily_index_is_stable_for_a_day() {
        let pack: Vec<Puzzle> = (0..8)
            .map(|n| Puzzle {
                id: format!("p{n}"),
                fen: String::new(),
                moves: vec!["a".into(), "b".into()],
                rating: 1200,
                popularity: 80,
                themes: Vec::new(),
            })
            .collect();
        assert_eq!(daily_index(&pack, 20_000), daily_index(&pack, 20_000));
        assert_ne!(daily_index(&pack, 20_000), daily_index(&pack, 20_001));
    }

    #[test]
    fn rush_step_ends_on_the_third_miss() {
        let (score, strikes, over) = rush_step(4, 2, true);
        assert_eq!((score, strikes, over), (5, 2, false));
        let (score, strikes, over) = rush_step(4, 2, false);
        assert_eq!((score, strikes, over), (4, 3, true));
        assert_eq!(rush_floor(1200, 0), 1050);
        assert_eq!(rush_floor(1200, 2), 1210);
        assert_eq!(rush_floor(400, 0), 600);
        assert_eq!(rush_floor(1200, 100), 2400);
    }

    #[test]
    fn starter_pack_covers_the_theme_button() {
        let pack = starter_pack();
        for theme in Theme::ALL {
            assert!(
                pack.iter()
                    .any(|puzzle| puzzle.themes.iter().any(|tag| tag == theme.tag())),
                "no {}",
                theme.tag()
            );
        }
    }

    #[test]
    fn rating_moves_toward_the_puzzle() {
        let up = adjust_rating(1200, 1800, true);
        let down = adjust_rating(1200, 1800, false);
        assert!(up > 1200);
        assert!(down < 1200);
        assert_eq!(adjust_rating(400, 2500, false), 400);
    }

    #[test]
    fn starter_pack_lines_all_play() {
        let pack = starter_pack();
        assert!(pack.len() >= 100, "starter pack has {}", pack.len());
        for puzzle in &pack {
            assert!(
                prepare(puzzle).is_some(),
                "puzzle {} does not play",
                puzzle.id
            );
        }
    }
}
