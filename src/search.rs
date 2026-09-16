//! The computer player: iterative-deepening negamax alpha-beta with
//! quiescence, MVV-LVA + killer + history move ordering, mate-distance
//! scoring, and a hard time budget.

use std::time::{Duration, Instant};

use crate::eval::{self, material};
use crate::piece::{Color, PieceKind};
use crate::position::{Move, Position};

/// Difficulty levels, exposed in the UI as 1/2/3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Depth 1 with a randomized tie-break — beatable by a beginner.
    Easy,
    /// Depth ≤ 3 or 0.5 s, whichever comes first.
    Medium,
    /// Depth ≤ 5 / 1.5 s — decent club strength on Pi-class hardware.
    Hard,
}

impl Level {
    pub fn max_depth(self) -> u32 {
        match self {
            Level::Easy => 1,
            Level::Medium => 3,
            Level::Hard => 5,
        }
    }

    pub fn budget(self) -> Duration {
        match self {
            Level::Easy => Duration::from_millis(100),
            Level::Medium => Duration::from_millis(500),
            Level::Hard => Duration::from_millis(1500),
        }
    }

    pub fn number(self) -> u8 {
        match self {
            Level::Easy => 1,
            Level::Medium => 2,
            Level::Hard => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Level::Easy => "Easy",
            Level::Medium => "Medium",
            Level::Hard => "Hard",
        }
    }

    pub fn parse(id: &str) -> Level {
        match id {
            "medium" => Level::Medium,
            "hard" => Level::Hard,
            _ => Level::Easy,
        }
    }

    pub fn all() -> [Level; 3] {
        [Level::Easy, Level::Medium, Level::Hard]
    }
}

/// Randomness source for the Easy level's tie-break (injectable for tests,
/// the pisweep `Rand` pattern).
pub trait Rand {
    fn next_f64(&mut self) -> f64;
}

pub struct ThreadRng {
    s: u64,
}

impl Default for ThreadRng {
    fn default() -> Self {
        // Nanos alone barely advance between constructions (the xorshift
        // output lands in a narrow band), so mix in a process-wide counter
        // and warm the state up with three splitmix rounds.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        let mut s = nanos
            ^ COUNTER
                .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        s = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        s = (s ^ (s >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        s ^= s >> 31;
        s = s.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        s ^= s >> 31;
        Self { s: s | 1 }
    }
}

impl ThreadRng {
    /// Build from a captured state (used to keep the UI's rng and the
    /// engine's share a stream).
    pub fn from_state(s: u64) -> Self {
        Self { s: s | 1 }
    }

    pub fn state(&self) -> u64 {
        self.s
    }
}

impl Rand for ThreadRng {
    fn next_f64(&mut self) -> f64 {
        let mut s = self.s;
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        self.s = s;
        (s as f64) / (u64::MAX as f64)
    }
}

/// Deterministic shuffle-free RNG for tests.
pub struct FixedRng(pub f64);

impl Rand for FixedRng {
    fn next_f64(&mut self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStop {
    Completed,
    TimeOut,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOutcome {
    pub mv: Move,
    pub score: i32,
    pub depth: u32,
    pub nodes: u64,
    pub stopped: SearchStop,
}

const MATE: i32 = 30_000;
const INF: i32 = 32_000;

pub struct Searcher {
    deadline: Instant,
    pub nodes: u64,
    pub stopped: bool,
    killers: [[Option<Move>; 2]; 64],
    history: [[u16; 128]; 12],
}

impl Searcher {
    pub fn new(budget: Duration) -> Self {
        Self {
            deadline: Instant::now() + budget,
            nodes: 0,
            stopped: false,
            killers: [Default::default(); 64],
            history: [[0; 128]; 12],
        }
    }

    fn check_time(&mut self) {
        self.nodes += 1;
        if self.nodes.is_multiple_of(1024) && Instant::now() >= self.deadline {
            self.stopped = true;
        }
    }

    fn key_for(&self, pos: &Position, mv: Move) -> usize {
        // History table key: piece color+kind × destination.
        let from = mv.from();
        let color_offset = match pos.turn {
            Color::White => 0,
            Color::Black => 6,
        };
        match pos.board[from.index()] {
            Some(piece) => color_offset + piece_key_offset(piece.kind) as usize,
            None => 0,
        }
    }

    pub fn order(
        &self,
        pos: &Position,
        moves: &[Move],
        ply: usize,
        tt_move: Option<Move>,
    ) -> Vec<(i32, Move)> {
        let mut scored: Vec<(i32, Move)> = Vec::with_capacity(moves.len());
        for &mv in moves {
            let score;
            if Some(mv) == tt_move {
                score = 10_000;
            } else if matches!(
                mv,
                crate::position::Move::Promotion {
                    kind: PieceKind::Queen,
                    ..
                }
            ) {
                score = 9_000;
            } else if mv.is_capture(&pos.board) {
                let victim = pos.board[mv.to().index()]
                    .map(|p| material(p.kind))
                    .unwrap_or(crate::eval::PAWN);
                let attacker = pos.board[mv.from().index()]
                    .map(|p| material(p.kind))
                    .unwrap_or(0);
                // MVV-LVA: big victim, small attacker.
                score = 1_000 + victim * 10 - attacker.min(QUEEN_CEIL);
            } else if self.killers[ply.min(63)][0] == Some(mv)
                || self.killers[ply.min(63)][1] == Some(mv)
            {
                score = 800;
            } else {
                score = self.history[self.key_for(pos, mv)][mv.to().index()] as i32;
            }
            scored.push((score, mv));
        }
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored
    }
}

const QUEEN_CEIL: i32 = 900;

fn piece_key_offset(kind: PieceKind) -> u8 {
    match kind {
        PieceKind::Pawn => 0,
        PieceKind::Knight => 1,
        PieceKind::Bishop => 2,
        PieceKind::Rook => 3,
        PieceKind::Queen => 4,
        PieceKind::King => 5,
    }
}

impl Searcher {
    pub fn negamax(
        &mut self,
        pos: &mut Position,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
    ) -> i32 {
        self.check_time();
        if self.stopped {
            return 0;
        }
        // In-search repetition and 50-move awareness: a move that repeats
        // scores as a draw — the engine avoids (or seeks) repetitions.
        if pos.halfmove >= 100 || (ply > 0 && pos.has_repeated()) {
            return 0;
        }
        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply, 8);
        }
        let moves = pos.legal_moves();
        if moves.is_empty() {
            return if pos.in_check_self() {
                // Prefer faster mates: deeper remaining depth = closer mate.
                -MATE + ply as i32
            } else {
                0
            };
        }
        let ordered = self.order(pos, &moves, ply, None);
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        let mut first = true;
        for (_, mv) in ordered {
            pos.make_move(mv).expect("legal move applies");
            let score = if first {
                first = false;
                -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1)
            } else {
                // Zero-window, re-search on fail-high.
                let z = -self.negamax(pos, depth - 1, -alpha - 1, -alpha, ply + 1);
                if z > alpha && z < beta {
                    -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1)
                } else {
                    z
                }
            };
            pos.undo_move();
            if self.stopped {
                return if best == -INF { 0 } else { best };
            }
            if score > best {
                best = score;
                best_move = Some(mv);
                if score > alpha {
                    alpha = score;
                    if alpha >= beta {
                        if !mv.is_capture(&pos.board) {
                            let slot = self.killers[ply.min(63)];
                            if slot[0] != Some(mv) {
                                self.killers[ply.min(63)] = [Some(mv), slot[0]];
                            }
                        }
                        break;
                    }
                }
            }
        }
        let _ = best_move;
        best
    }

    fn quiescence(
        &mut self,
        pos: &mut Position,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        depth: i32,
    ) -> i32 {
        self.check_time();
        if self.stopped {
            return 0;
        }
        // Standing pat is only valid when not in check: a stand-pat cutoff
        // while in check would hide a possible mate.
        let in_check = pos.in_check_self();
        let stand_pat = if in_check {
            -INF + 1
        } else {
            eval::evaluate(pos)
        };
        if !in_check {
            if stand_pat >= beta {
                return stand_pat;
            }
            if depth == 0 {
                return stand_pat;
            }
            if stand_pat > alpha {
                alpha = stand_pat;
            }
        }
        let moves = pos.legal_moves();
        let tactical: Vec<Move> = moves
            .into_iter()
            .filter(|mv| in_check || mv.is_capture(&pos.board) || mv.promotion().is_some())
            .collect();
        if tactical.is_empty() {
            if in_check {
                // Mate in quiescence (no evasions found).
                return -MATE + ply as i32;
            }
            return stand_pat;
        }
        // Delta pruning: skip captures that cannot bring the eval to alpha.
        let ordered = self.order(pos, &tactical, ply, None);
        for (_, mv) in ordered {
            let gain = pos.board[mv.to().index()]
                .map(|p| material(p.kind))
                .unwrap_or(0)
                + mv.promotion()
                    .map(|k| material(k) - crate::eval::PAWN)
                    .unwrap_or(0);
            if !in_check && stand_pat + gain + 200 < alpha {
                continue;
            }
            pos.make_move(mv).expect("generated move applies");
            let score = -self.quiescence(pos, -beta, -alpha, ply + 1, depth - 1);
            pos.undo_move();
            if self.stopped {
                return if in_check {
                    -MATE + ply as i32
                } else {
                    stand_pat
                };
            }
            if score >= beta {
                return score;
            }
            if score > alpha {
                alpha = score;
            }
        }
        alpha
    }
}

/// Root: iterative deepening from depth 1 to the level's cap, keeping the
/// best move completed so far when time runs out mid-iteration.
pub fn search(pos: &Position, level: Level) -> SearchOutcome {
    let rng = &mut ThreadRng::default();
    search_with(pos, level, rng)
}

pub fn search_with(pos: &Position, level: Level, rng: &mut dyn Rand) -> SearchOutcome {
    let moves = pos.legal_moves();
    if moves.is_empty() {
        panic!("search called on a finished position");
    }
    if moves.len() == 1 {
        return SearchOutcome {
            mv: moves[0],
            score: 0,
            depth: 0,
            nodes: 0,
            stopped: SearchStop::Completed,
        };
    }
    let mut searcher = Searcher::new(level.budget());
    let mut best_mv = moves[0];
    let mut best_score = -INF;
    let mut completed_depth = 0;
    let mut nodes_used = searcher.nodes;
    for depth in 1..=level.max_depth() {
        let was_stopped = searcher.stopped;
        let ordered = searcher.order(pos, &moves, 0, Some(best_mv));
        let mut iter_best = best_mv;
        let mut iter_score = -INF;
        let mut iter_alpha = -INF;
        for (_, mv) in &ordered {
            let mut probe = pos.clone();
            probe.make_move(*mv).expect("generated move applies");
            let score =
                -searcher.negamax(&mut probe, depth as i32 - 1, -INF, -iter_alpha.max(-INF), 1);
            if searcher.stopped {
                break;
            }
            if score > iter_score {
                iter_score = score;
                iter_best = *mv;
            }
            if score > iter_alpha {
                iter_alpha = score;
            }
        }
        if searcher.stopped && !was_stopped {
            break;
        }
        best_mv = iter_best;
        best_score = iter_score;
        completed_depth = depth;
        nodes_used = searcher.nodes;
        if searcher.stopped {
            break;
        }
    }
    let stopped = if searcher.stopped {
        SearchStop::TimeOut
    } else {
        SearchStop::Completed
    };
    // Easy level: with some probability swap in a random legal move that
    // scores within 100cp of the best (or the move list is short), so the
    // weakest level varies without blundering on purpose.
    if level == Level::Easy && rng.next_f64() < 0.4 {
        let mut pool = pos.legal_moves();
        // Score each candidate at depth 1 with a fresh searcher so ties are
        // found without disturbing the main search.
        let mut scorer = Searcher::new(Duration::from_millis(200));
        let mut near: Vec<Move> = Vec::new();
        for mv in pool.drain(..) {
            let mut probe = pos.clone();
            probe.make_move(mv).expect("generated move applies");
            let score = -scorer.negamax(&mut probe, 0, -INF, INF, 1);
            if scorer.stopped || score >= best_score - 100 {
                near.push(mv);
            }
        }
        if near.len() > 1 {
            let idx = (rng.next_f64() * near.len() as f64).floor() as usize;
            if let Some(mv) = near.get(idx.min(near.len().saturating_sub(1))) {
                best_mv = *mv;
            }
        }
    }
    SearchOutcome {
        mv: best_mv,
        score: best_score,
        depth: completed_depth,
        nodes: nodes_used,
        stopped,
    }
}

/// One-shot helper for the UI and tests.
pub fn best_move(pos: &Position, level: Level) -> SearchOutcome {
    search(pos, level)
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
