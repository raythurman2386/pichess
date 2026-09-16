//! Chess rules on a 0x88 mailbox board: state, move generation, make/undo,
//! endings, and FEN. No UI imports.
//!
//! Board layout: square 0 = a1, each rank is 16 entries, off-board squares
//! have `sq & 0x88 != 0`. Search mutates via `make_move`/`undo_move` (no
//! clone-copy): the undo record carries the captured piece, prior clocks,
//! castling rights, en-passant square, and the pre-move Zobrist key, so an
//! undo restores the exact position including the hash.

use std::fmt;

use crate::piece::{Color, Piece, PieceKind, Square};

/// Castling-rights bitmask.
pub const CASTLE_WK: u8 = 1;
pub const CASTLE_WQ: u8 = 2;
pub const CASTLE_BK: u8 = 4;
pub const CASTLE_BQ: u8 = 8;

/// All 64 on-board squares, a1..h8 order.
pub const SQUARES: [Square; 64] = {
    let mut out = [Square(0); 64];
    let _i = 0;
    let mut rank = 0u8;
    while rank < 8 {
        let mut file = 0u8;
        while file < 8 {
            out[(rank * 8 + file) as usize] = Square(rank * 16 + file);
            file += 1;
        }
        rank += 1;
    }
    out
};

/// Direction offsets in raw 0x88 index deltas.
const KNIGHT_DIRS: [i32; 8] = [33, 31, 14, 18, -33, -31, -14, -18];
const BISHOP_DIRS: [i32; 4] = [17, 15, -17, -15];
const ROOK_DIRS: [i32; 4] = [16, -16, 1, -1];
const KING_DIRS: [i32; 8] = [17, 16, 15, 1, -1, -15, -16, -17];

/// A move: from, to, and what it becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Move {
    /// Any non-castling move without promotion (including pawn pushes and
    /// captures).
    Quiet { from: Square, to: Square },
    /// A pawn move onto the en-passant target, capturing the pawn behind it.
    EnPassant { from: Square, to: Square },
    /// King castles to `to` (g1/c1/g8/c8); the home rook follows.
    Castle { from: Square, to: Square },
    Promotion {
        from: Square,
        to: Square,
        kind: PieceKind,
    },
}

impl Move {
    pub fn from(self) -> Square {
        match self {
            Move::Quiet { from, .. }
            | Move::EnPassant { from, .. }
            | Move::Castle { from, .. }
            | Move::Promotion { from, .. } => from,
        }
    }

    pub fn to(self) -> Square {
        match self {
            Move::Quiet { to, .. }
            | Move::EnPassant { to, .. }
            | Move::Castle { to, .. }
            | Move::Promotion { to, .. } => to,
        }
    }

    pub fn promotion(self) -> Option<PieceKind> {
        match self {
            Move::Promotion { kind, .. } => Some(kind),
            _ => None,
        }
    }

    pub fn is_capture(self, board: &[Option<Piece>; 128]) -> bool {
        match self {
            Move::EnPassant { .. } => true,
            _ => board[self.to().index()].is_some(),
        }
    }

    pub fn is_castle(self) -> bool {
        matches!(self, Move::Castle { .. })
    }
}

impl fmt::Display for Move {
    /// Coordinate notation (e2e4, e7e8q) — engine internals and save files.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.from().name(), self.to().name())?;
        if let Some(kind) = self.promotion() {
            write!(f, "{}", kind.letter())?;
        }
        Ok(())
    }
}

/// Why the game ended, when it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOver {
    Checkmate(Color),
    Stalemate,
    FiftyMove,
    ThreefoldRepetition,
    InsufficientMaterial,
}

impl GameOver {
    pub fn label(self) -> String {
        match self {
            GameOver::Checkmate(winner) => format!("Checkmate — {} wins", winner.label()),
            GameOver::Stalemate => "Stalemate — draw".into(),
            GameOver::FiftyMove => "Draw — fifty-move rule".into(),
            GameOver::ThreefoldRepetition => "Draw — threefold repetition".into(),
            GameOver::InsufficientMaterial => "Draw — insufficient material".into(),
        }
    }

    /// PGN result token.
    pub fn result(self) -> &'static str {
        match self {
            GameOver::Checkmate(Color::White) => "1-0",
            GameOver::Checkmate(Color::Black) => "0-1",
            _ => "1/2-1/2",
        }
    }
}

/// Everything needed to restore the position after a move.
#[derive(Debug, Clone, Copy)]
pub struct Undo {
    from: Square,
    to: Square,
    moved: Piece,
    captured: Option<Piece>,
    captured_sq: Option<Square>,
    castling: u8,
    ep_square: Option<Square>,
    halfmove: u16,
    fullmove: u16,
    key: u64,
    rook_from: Option<Square>,
    rook_to: Option<Square>,
}

#[derive(Debug, Clone)]
pub struct Position {
    pub board: [Option<Piece>; 128],
    pub turn: Color,
    pub castling: u8,
    pub ep_square: Option<Square>,
    pub halfmove: u16,
    pub fullmove: u16,
    pub key: u64,
    /// Zobrist key of every position played so far (including the current
    /// one), for repetition detection.
    pub history: Vec<u64>,
    undo_stack: Vec<Undo>,
}

/// Splitmix64-seeded Zobrist tables, built once.
struct Zobrist {
    pieces: [[u64; 128]; 12],
    castling: [u64; 16],
    ep_file: [u64; 8],
    black_to_move: u64,
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn build_zobrist() -> Zobrist {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut pieces = [[0u64; 128]; 12];
    for entry in pieces.iter_mut() {
        for v in entry.iter_mut() {
            *v = splitmix64(&mut state);
        }
    }
    let mut castling = [0u64; 16];
    for v in castling.iter_mut() {
        *v = splitmix64(&mut state);
    }
    let mut ep_file = [0u64; 8];
    for v in ep_file.iter_mut() {
        *v = splitmix64(&mut state);
    }
    Zobrist {
        pieces,
        castling,
        ep_file,
        black_to_move: splitmix64(&mut state),
    }
}

fn zobrist() -> &'static Zobrist {
    use std::sync::OnceLock;
    static TABLES: OnceLock<Zobrist> = OnceLock::new();
    TABLES.get_or_init(build_zobrist)
}

fn piece_key(color: Color, kind: PieceKind) -> usize {
    let color_offset = match color {
        Color::White => 0,
        Color::Black => 6,
    };
    let kind_offset = match kind {
        PieceKind::Pawn => 0,
        PieceKind::Knight => 1,
        PieceKind::Bishop => 2,
        PieceKind::Rook => 3,
        PieceKind::Queen => 4,
        PieceKind::King => 5,
    };
    color_offset + kind_offset
}

impl Position {
    pub fn new() -> Position {
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
            .expect("start position FEN is valid")
    }

    /// Parse a FEN string, validating placement, side, castling, en-passant,
    /// and clocks. Castling rights whose king or rook is not on its home
    /// square are dropped rather than rejected (lenient, like most tools).
    pub fn from_fen(fen: &str) -> Result<Position, String> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() != 6 {
            return Err(format!("FEN needs 6 fields, got {}", parts.len()));
        }
        let (placement, side, castling, ep, halfmove, fullmove) =
            (parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]);

        let ranks: Vec<&str> = placement.split('/').collect();
        if ranks.len() != 8 {
            return Err(format!("FEN placement needs 8 ranks, got {}", ranks.len()));
        }

        let mut board = [None; 128];
        let mut kings = [0u8; 2];
        for (rank_idx, rank_str) in ranks.iter().enumerate() {
            // FEN rank 8 comes first; 0x88 rank 0 is rank 1.
            let rank = 7 - rank_idx as u8;
            let mut file = 0u8;
            for c in rank_str.chars() {
                if let Some(skip) = c.to_digit(10) {
                    if !(1..=8).contains(&skip) {
                        return Err(format!("invalid skip digit {c}"));
                    }
                    file += skip as u8;
                } else {
                    let kind =
                        PieceKind::from_letter(c).ok_or_else(|| format!("invalid piece {c}"))?;
                    let color = if c.is_ascii_uppercase() {
                        Color::White
                    } else {
                        Color::Black
                    };
                    if file > 7 {
                        return Err(format!("rank {rank_str} overflows the board"));
                    }
                    if kind == PieceKind::King {
                        kings[color.index()] += 1;
                    }
                    board[Square::new(rank, file).index()] = Some(Piece::new(kind, color));
                    file += 1;
                }
            }
            if file != 8 {
                return Err(format!("rank {rank_str} fills {} files, need 8", file));
            }
        }
        if kings[Color::White.index()] != 1 || kings[Color::Black.index()] != 1 {
            return Err("FEN needs exactly one king per side".into());
        }

        let turn = match side {
            "w" => Color::White,
            "b" => Color::Black,
            _other => return Err(format!("invalid side to move {side:?}")),
        };

        let mut castling_rights = 0u8;
        if castling != "-" {
            for c in castling.chars() {
                match c {
                    'K' => castling_rights |= CASTLE_WK,
                    'Q' => castling_rights |= CASTLE_WQ,
                    'k' => castling_rights |= CASTLE_BK,
                    'q' => castling_rights |= CASTLE_BQ,
                    _ => return Err(format!("invalid castling flag {c}")),
                }
            }
        }

        let ep_square = if ep == "-" {
            None
        } else {
            Some(Square::parse(ep).ok_or_else(|| format!("invalid en-passant square {ep:?}"))?)
        };

        let halfmove: u16 = halfmove
            .parse()
            .map_err(|_| format!("invalid halfmove clock {halfmove:?}"))?;
        let fullmove: u16 = fullmove
            .parse()
            .map_err(|_| format!("invalid fullmove number {fullmove:?}"))?;
        if halfmove > 1000 || fullmove < 1 {
            return Err("clocks out of range".into());
        }

        let mut position = Position {
            board,
            turn,
            castling: castling_rights,
            ep_square,
            halfmove,
            fullmove,
            key: 0,
            history: Vec::new(),
            undo_stack: Vec::new(),
        };
        position.castling &= Self::home_castling_mask(&position.board, Color::White)
            | Self::home_castling_mask(&position.board, Color::Black);
        position.rebuild_key();
        position.history.push(position.key);
        Ok(position)
    }

    /// Castling bits that are consistent with king/rook home squares.
    fn home_castling_mask(board: &[Option<Piece>; 128], color: Color) -> u8 {
        let king_home = Square::new(color.back_rank(), 4);
        let mut mask = 0;
        if board[king_home.index()] == Some(Piece::new(PieceKind::King, color)) {
            let kingside = Square::new(color.back_rank(), 7);
            let queenside = Square::new(color.back_rank(), 0);
            if board[kingside.index()] == Some(Piece::new(PieceKind::Rook, color)) {
                mask |= match color {
                    Color::White => CASTLE_WK,
                    Color::Black => CASTLE_BK,
                };
            }
            if board[queenside.index()] == Some(Piece::new(PieceKind::Rook, color)) {
                mask |= match color {
                    Color::White => CASTLE_WQ,
                    Color::Black => CASTLE_BQ,
                };
            }
        }
        mask
    }

    pub fn to_fen(&self) -> String {
        let mut placement = String::new();
        for rank in (0u8..8).rev() {
            let mut empty = 0;
            for file in 0u8..8 {
                match self.board[Square::new(rank, file).index()] {
                    Some(piece) => {
                        if empty > 0 {
                            placement.push_str(&empty.to_string());
                            empty = 0;
                        }
                        placement.push(piece.fen_letter());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                placement.push_str(&empty.to_string());
            }
            if rank > 0 {
                placement.push('/');
            }
        }
        let side = match self.turn {
            Color::White => "w",
            Color::Black => "b",
        };
        let mut castling = String::new();
        for (bit, letter) in [
            (CASTLE_WK, 'K'),
            (CASTLE_WQ, 'Q'),
            (CASTLE_BK, 'k'),
            (CASTLE_BQ, 'q'),
        ] {
            if self.castling & bit != 0 {
                castling.push(letter);
            }
        }
        let castling = if castling.is_empty() {
            "-".into()
        } else {
            castling
        };
        let ep = self
            .ep_square
            .map(|s| s.name())
            .unwrap_or_else(|| "-".into());
        format!(
            "{placement} {side} {castling} {ep} {} {}",
            self.halfmove, self.fullmove
        )
    }

    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        if !sq.on_board() {
            return None;
        }
        self.board[sq.index()]
    }

    pub fn king_square(&self, color: Color) -> Option<Square> {
        let target = Piece::new(PieceKind::King, color);
        self.board
            .iter()
            .position(|p| *p == Some(target))
            .map(Square::from_index)
    }

    fn rebuild_key(&mut self) {
        let z = zobrist();
        let mut key = 0u64;
        for sq in SQUARES {
            if let Some(piece) = self.board[sq.index()] {
                key ^= z.pieces[piece_key(piece.color, piece.kind)][sq.index()];
            }
        }
        key ^= z.castling[(self.castling & 0x0f) as usize];
        if let Some(ep) = self.ep_square {
            key ^= z.ep_file[ep.file() as usize];
        }
        if self.turn == Color::Black {
            key ^= z.black_to_move;
        }
        self.key = key;
    }

    fn toggle_piece(&mut self, piece: Piece, sq: Square) {
        self.key ^= zobrist().pieces[piece_key(piece.color, piece.kind)][sq.index()];
    }

    fn set_castling(&mut self, new: u8) {
        if new != self.castling {
            self.key ^= zobrist().castling[(self.castling & 0x0f) as usize];
            self.key ^= zobrist().castling[(new & 0x0f) as usize];
            self.castling = new;
        }
    }

    fn set_ep(&mut self, new: Option<Square>) {
        if let Some(ep) = self.ep_square {
            self.key ^= zobrist().ep_file[ep.file() as usize];
        }
        if let Some(ep) = new {
            self.key ^= zobrist().ep_file[ep.file() as usize];
        }
        self.ep_square = new;
    }

    fn toggle_turn(&mut self) {
        if self.turn == Color::Black {
            self.key ^= zobrist().black_to_move;
        }
    }

    /// Is `sq` attacked by any piece of `by`?
    pub fn is_attacked(&self, sq: Square, by: Color) -> bool {
        let base = sq.0 as i32;
        // Pawns: a `by` pawn stands on sq - by.pawn_dir() ± 1.
        for df in [-1i32, 1] {
            let cand = base - by.pawn_dir() + df;
            if cand & 0x88 == 0 {
                if let Some(p) = self.board[cand as usize] {
                    if p.color == by && p.kind == PieceKind::Pawn {
                        return true;
                    }
                }
            }
        }
        // Knights.
        for delta in KNIGHT_DIRS {
            let cand = base + delta;
            if cand & 0x88 == 0 {
                if let Some(p) = self.board[cand as usize] {
                    if p.color == by && p.kind == PieceKind::Knight {
                        return true;
                    }
                }
            }
        }
        // Kings.
        for delta in KING_DIRS {
            let cand = base + delta;
            if cand & 0x88 == 0 {
                if let Some(p) = self.board[cand as usize] {
                    if p.color == by && p.kind == PieceKind::King {
                        return true;
                    }
                }
            }
        }
        // Sliding: bishop/queen on diagonals, rook/queen on straights.
        for (dirs, matches) in [
            (&BISHOP_DIRS, [PieceKind::Bishop, PieceKind::Queen]),
            (&ROOK_DIRS, [PieceKind::Rook, PieceKind::Queen]),
        ] {
            for step in dirs {
                let mut cand = base + step;
                while cand & 0x88 == 0 {
                    match self.board[cand as usize] {
                        Some(p) => {
                            if p.color == by && (p.kind == matches[0] || p.kind == matches[1]) {
                                return true;
                            }
                            break;
                        }
                        None => cand += step,
                    }
                }
            }
        }
        false
    }

    pub fn in_check(&self, color: Color) -> bool {
        self.king_square(color)
            .map(|k| self.is_attacked(k, color.opposite()))
            .unwrap_or(false)
    }

    pub fn in_check_self(&self) -> bool {
        self.in_check(self.turn)
    }

    /// All moves ignoring king-safety after the move (castling is fully
    /// validated here; pins and checks are filtered by `legal_moves`).
    pub fn pseudo_legal_moves(&self) -> Vec<Move> {
        let mut out = Vec::with_capacity(48);
        let us = self.turn;
        for from in SQUARES {
            let Some(piece) = self.board[from.index()] else {
                continue;
            };
            if piece.color != us {
                continue;
            }
            match piece.kind {
                PieceKind::Pawn => self.gen_pawn_moves(&mut out, from, us),
                PieceKind::Knight => self.gen_step_moves(&mut out, from, us, &KNIGHT_DIRS),
                PieceKind::King => {
                    self.gen_step_moves(&mut out, from, us, &KING_DIRS);
                    self.gen_castles(&mut out, from);
                }
                _ => {
                    let dirs: &[i32] = match piece.kind {
                        PieceKind::Bishop => &BISHOP_DIRS,
                        PieceKind::Rook => &ROOK_DIRS,
                        _ => &KING_DIRS, // queen
                    };
                    for step in dirs {
                        let mut cand = from.0 as i32 + step;
                        while cand & 0x88 == 0 {
                            match self.board[cand as usize] {
                                Some(p) => {
                                    if p.color != us {
                                        out.push(Move::Quiet {
                                            from,
                                            to: Square(cand as u8),
                                        });
                                    }
                                    break;
                                }
                                None => {
                                    out.push(Move::Quiet {
                                        from,
                                        to: Square(cand as u8),
                                    });
                                    cand += step;
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn gen_pawn_moves(&self, out: &mut Vec<Move>, from: Square, us: Color) {
        let dir = us.pawn_dir();
        let base = from.0 as i32;
        // Pushes.
        let one = base + dir;
        if one & 0x88 == 0 && self.board[one as usize].is_none() {
            let one = Square(one as u8);
            if one.rank() == us.promo_rank() {
                for kind in [
                    PieceKind::Queen,
                    PieceKind::Rook,
                    PieceKind::Bishop,
                    PieceKind::Knight,
                ] {
                    out.push(Move::Promotion {
                        from,
                        to: one,
                        kind,
                    });
                }
            } else {
                out.push(Move::Quiet { from, to: one });
                let two = base + 2 * dir;
                if from.rank() == us.pawn_rank() && self.board[two as usize].is_none() {
                    out.push(Move::Quiet {
                        from,
                        to: Square(two as u8),
                    });
                }
            }
        }
        // Captures, including en passant. The target must be occupied by
        // an enemy piece, or be the en-passant target.
        for df in [-1i32, 1] {
            let cand = base + dir + df;
            if cand & 0x88 != 0 {
                continue;
            }
            let target = Square(cand as u8);
            let is_enemy = self.board[target.index()]
                .map(|p| p.color != us)
                .unwrap_or(false);
            let is_ep = self.ep_square == Some(target);
            if !is_enemy && !is_ep {
                continue;
            }
            if target.rank() == us.promo_rank() {
                for kind in [
                    PieceKind::Queen,
                    PieceKind::Rook,
                    PieceKind::Bishop,
                    PieceKind::Knight,
                ] {
                    out.push(Move::Promotion {
                        from,
                        to: target,
                        kind,
                    });
                }
            } else if is_ep {
                out.push(Move::EnPassant { from, to: target });
            } else {
                out.push(Move::Quiet { from, to: target });
            }
        }
    }

    fn gen_step_moves(&self, out: &mut Vec<Move>, from: Square, us: Color, dirs: &[i32]) {
        for delta in dirs {
            let cand = from.0 as i32 + delta;
            if cand & 0x88 != 0 {
                continue;
            }
            if self.board[cand as usize]
                .map(|p| p.color == us)
                .unwrap_or(false)
            {
                continue;
            }
            out.push(Move::Quiet {
                from,
                to: Square(cand as u8),
            });
        }
    }

    /// Castling, validated fully: rights, home squares, empty path, and no
    /// attacked square the king starts on or crosses (the destination is
    /// re-checked by the make/unmake legality filter).
    fn gen_castles(&self, out: &mut Vec<Move>, from: Square) {
        let us = self.turn;
        let opp = us.opposite();
        let rank = us.back_rank();
        let king_home = Square::new(rank, 4);
        if from != king_home {
            return;
        }
        let (kingside, queenside) = match us {
            Color::White => (CASTLE_WK, CASTLE_WQ),
            Color::Black => (CASTLE_BK, CASTLE_BQ),
        };
        if self.castling & kingside != 0 {
            let f = Square::new(rank, 5);
            let g = Square::new(rank, 6);
            let rook_home = Square::new(rank, 7);
            if self.board[f.index()].is_none()
                && self.board[g.index()].is_none()
                && self.board[rook_home.index()] == Some(Piece::new(PieceKind::Rook, us))
                && !self.is_attacked(from, opp)
                && !self.is_attacked(f, opp)
                && !self.is_attacked(g, opp)
            {
                out.push(Move::Castle {
                    from,
                    to: Square::new(rank, 6),
                });
            }
        }
        if self.castling & queenside != 0 {
            let b = Square::new(rank, 1);
            let c = Square::new(rank, 2);
            let d = Square::new(rank, 3);
            let rook_home = Square::new(rank, 0);
            if self.board[b.index()].is_none()
                && self.board[c.index()].is_none()
                && self.board[d.index()].is_none()
                && self.board[rook_home.index()] == Some(Piece::new(PieceKind::Rook, us))
                && !self.is_attacked(from, opp)
                && !self.is_attacked(d, opp)
                && !self.is_attacked(c, opp)
            {
                out.push(Move::Castle {
                    from,
                    to: Square::new(rank, 2),
                });
            }
        }
    }

    /// Moves that are fully legal: the mover's king is never left attacked.
    pub fn legal_moves(&self) -> Vec<Move> {
        let pseudo = self.pseudo_legal_moves();
        let mut probe = self.clone();
        let mut out = Vec::with_capacity(pseudo.len());
        for mv in pseudo {
            if probe.is_legal_fast(mv) {
                out.push(mv);
            }
        }
        out
    }

    /// Is `mv` legal in this position? Checks via make/unmake on a probe.
    pub fn is_legal(&self, mv: Move) -> bool {
        let mut probe = self.clone();
        probe.is_legal_fast(mv)
    }

    /// Make, test the mover's king, undo. Used by legality filtering; leaves
    /// `self` unchanged.
    fn is_legal_fast(&mut self, mv: Move) -> bool {
        let made = self.make_move(mv).is_ok();
        let legal = made && !self.in_check(self.turn.opposite());
        if made {
            self.undo_move();
        }
        legal
    }

    /// Apply a move. Returns Err for moves that do not apply (no piece,
    /// wrong color, malformed en passant / castle).
    pub fn make_move(&mut self, mv: Move) -> Result<(), String> {
        let us = self.turn;
        let from = mv.from();
        let to = mv.to();
        let Some(moved) = self.board[from.index()] else {
            return Err("no piece on the from square".into());
        };
        if moved.color != us {
            return Err("wrong color piece".into());
        }

        let (captured, captured_sq) = match mv {
            Move::EnPassant { to, .. } => {
                // The captured pawn sits behind the target square.
                let cap_sq = Square((to.0 as i32 - us.pawn_dir()) as u8);
                (self.board[cap_sq.index()], Some(cap_sq))
            }
            _ => {
                let existing = self.board[to.index()];
                (existing, if existing.is_some() { Some(to) } else { None })
            }
        };
        let undo = Undo {
            from,
            to,
            moved,
            captured,
            captured_sq,
            castling: self.castling,
            ep_square: self.ep_square,
            halfmove: self.halfmove,
            fullmove: self.fullmove,
            key: self.key,
            rook_from: None,
            rook_to: None,
        };
        self.undo_stack.push(undo);

        self.toggle_piece(moved, from);
        self.board[from.index()] = None;

        match mv {
            Move::Quiet { .. } => {
                if let Some(captured) = captured {
                    self.toggle_piece(captured, to);
                }
                self.toggle_piece(moved, to);
                self.board[to.index()] = Some(moved);
                self.set_ep(None);
                if moved.kind == PieceKind::Pawn && to.0 as i32 - from.0 as i32 == 2 * us.pawn_dir()
                {
                    self.set_ep(Some(Square((from.0 as i32 + us.pawn_dir()) as u8)));
                }
            }
            Move::EnPassant { to, .. } => {
                let captured_sq = Square((to.0 as i32 - us.pawn_dir()) as u8);
                let captured = self.board[captured_sq.index()];
                if captured.map(|p| p.kind) != Some(PieceKind::Pawn)
                    || captured.map(|p| p.color) == Some(us)
                {
                    self.undo_stack.pop();
                    return Err("no enemy pawn behind the en-passant target".into());
                }
                let captured = captured.unwrap();
                self.toggle_piece(captured, captured_sq);
                self.board[captured_sq.index()] = None;
                self.toggle_piece(moved, to);
                self.board[to.index()] = Some(moved);
                self.set_ep(None);
            }
            Move::Castle { to, .. } => {
                let (rook_from_file, rook_to_file) = if to.file() == 6 { (7, 5) } else { (0, 3) };
                let rook_from = Square::new(us.back_rank(), rook_from_file);
                let rook_to = Square::new(us.back_rank(), rook_to_file);
                let rook = self.board[rook_from.index()];
                if rook.map(|p| p.kind) != Some(PieceKind::Rook)
                    || rook.map(|p| p.color) != Some(us)
                {
                    self.undo_stack.pop();
                    return Err("no rook on the castling corner".into());
                }
                let rook = rook.unwrap();
                if let Some(captured) = captured {
                    self.toggle_piece(captured, to);
                }
                self.toggle_piece(moved, to);
                self.board[to.index()] = Some(moved);
                self.toggle_piece(rook, rook_from);
                self.board[rook_from.index()] = None;
                self.toggle_piece(rook, rook_to);
                self.board[rook_to.index()] = Some(rook);
                let undo = self.undo_stack.last_mut().expect("undo pushed above");
                undo.rook_from = Some(rook_from);
                undo.rook_to = Some(rook_to);
                self.set_ep(None);
            }
            Move::Promotion { to, kind, .. } => {
                let promoted = Piece::new(kind, us);
                if let Some(captured) = captured {
                    self.toggle_piece(captured, to);
                }
                self.toggle_piece(promoted, to);
                self.board[to.index()] = Some(promoted);
                self.set_ep(None);
            }
        }

        // Castling rights die on king moves, rook moves, and rook captures.
        let mut new_castling = self.castling;
        if moved.kind == PieceKind::King {
            new_castling &= match us {
                Color::White => !(CASTLE_WK | CASTLE_WQ),
                Color::Black => !(CASTLE_BK | CASTLE_BQ),
            };
        }
        if moved.kind == PieceKind::Rook {
            new_castling &= !Self::rook_castle_bits(from);
        }
        if let Some(captured) = captured {
            if captured.kind == PieceKind::Rook {
                new_castling &= !Self::rook_castle_bits(to);
            }
        }
        self.set_castling(new_castling);

        if us == Color::Black {
            self.fullmove += 1;
        }
        self.turn = us.opposite();
        self.toggle_turn();
        if moved.kind == PieceKind::Pawn || captured.is_some() {
            self.halfmove = 0;
        } else {
            self.halfmove += 1;
        }
        self.history.push(self.key);
        Ok(())
    }

    /// The castling bit tied to a rook standing on `sq` (a1/h1/a8/h8).
    fn rook_castle_bits(sq: Square) -> u8 {
        if sq.rank() != 0 && sq.rank() != 7 {
            return 0;
        }
        match (sq.rank(), sq.file()) {
            (0, 7) => CASTLE_WK,
            (0, 0) => CASTLE_WQ,
            (7, 7) => CASTLE_BK,
            (7, 0) => CASTLE_BQ,
            _ => 0,
        }
    }

    /// How many plies are on the undo stack.
    pub fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// The last move played, in coordinate notation, from the undo stack.
    pub fn last_move(&self) -> Option<(Square, Square)> {
        self.undo_stack.last().map(|u| (u.from, u.to))
    }

    /// Undo the last `make_move`. No-op when nothing to undo.
    pub fn undo_move(&mut self) {
        let Some(undo) = self.undo_stack.pop() else {
            return;
        };
        // Reverse the turn flip first: `us` is the side that moved.
        let us = self.turn.opposite();
        self.turn = us;
        self.toggle_turn();

        self.fullmove = undo.fullmove;
        self.halfmove = undo.halfmove;
        if self.castling != undo.castling {
            self.key ^= zobrist().castling[(self.castling & 0x0f) as usize];
            self.key ^= zobrist().castling[(undo.castling & 0x0f) as usize];
            self.castling = undo.castling;
        }
        if self.ep_square != undo.ep_square {
            if let Some(ep) = self.ep_square {
                self.key ^= zobrist().ep_file[ep.file() as usize];
            }
            if let Some(ep) = undo.ep_square {
                self.key ^= zobrist().ep_file[ep.file() as usize];
            }
            self.ep_square = undo.ep_square;
        }

        match undo.moved.kind {
            PieceKind::King if undo.rook_from.is_some() => {
                // Castling: restore king and rook. The king target and rook
                // target were empty before the move; nothing was captured.
                self.board[undo.to.index()] = None;
                self.board[undo.from.index()] = Some(undo.moved);
                let rook_from = undo.rook_from.expect("checked above");
                let rook_to = undo.rook_to.expect("checked above");
                let rook = self.board[rook_to.index()];
                self.board[rook_to.index()] = None;
                self.board[rook_from.index()] = rook;
            }
            _ => {
                // Remove whatever stands on the destination (the moved or
                // promoted piece), then restore the captured piece (for en
                // passant it sits behind `to`, not on it).
                self.board[undo.to.index()] = None;
                if let Some(captured_sq) = undo.captured_sq {
                    self.board[captured_sq.index()] = undo.captured;
                }
                self.board[undo.from.index()] = Some(undo.moved);
            }
        }

        // Rebuild board-derived zobrist deltas by restoring the saved key.
        self.key = undo.key;
        self.history.pop();
    }

    /// How many times the current position has occurred in the game history
    /// (including right now). Same-side positions only, bounded by the
    /// halfmove clock.
    pub fn repetition_count(&self) -> u32 {
        let mut count = 0u32;
        let start = self
            .history
            .len()
            .saturating_sub(self.halfmove as usize + 1);
        for i in (start..self.history.len()).step_by(2) {
            if self.history[i] == self.key {
                count += 1;
            }
        }
        count
    }

    /// True when the current key occurred before in history (used by the
    /// search to score a repetition as a draw).
    pub fn has_repeated(&self) -> bool {
        self.repetition_count() >= 2
    }

    /// Dead-position material: no pawn/rook/queen and at most one minor, or
    /// exactly two bishops of the same square color.
    pub fn insufficient_material(&self) -> bool {
        let mut minors = Vec::new();
        for sq in SQUARES {
            if let Some(piece) = self.board[sq.index()] {
                match piece.kind {
                    PieceKind::Pawn | PieceKind::Rook | PieceKind::Queen => return false,
                    PieceKind::Bishop | PieceKind::Knight => minors.push((piece.kind, sq)),
                    PieceKind::King => {}
                }
            }
        }
        match minors.len() {
            0 | 1 => true,
            2 => {
                matches!(minors[0].0, PieceKind::Bishop)
                    && matches!(minors[1].0, PieceKind::Bishop)
                    && minors[0].1.square_color() == minors[1].1.square_color()
            }
            _ => false,
        }
    }

    /// The ending reached in this position, if any.
    pub fn game_over(&self) -> Option<GameOver> {
        let moves = self.legal_moves();
        if moves.is_empty() {
            return Some(if self.in_check_self() {
                GameOver::Checkmate(self.turn.opposite())
            } else {
                GameOver::Stalemate
            });
        }
        if self.halfmove >= 100 {
            return Some(GameOver::FiftyMove);
        }
        if self.repetition_count() >= 3 {
            return Some(GameOver::ThreefoldRepetition);
        }
        if self.insufficient_material() {
            return Some(GameOver::InsufficientMaterial);
        }
        None
    }

    /// Perft: count leaf nodes of the legal-move tree to `depth`. The
    /// correctness harness for move generation, make/undo, and castling.
    pub fn perft(&mut self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let moves = self.legal_moves();
        if depth == 1 {
            return moves.len() as u64;
        }
        let mut total = 0u64;
        for mv in moves {
            self.make_move(mv).expect("legal move applies");
            total += self.perft(depth - 1);
            self.undo_move();
        }
        total
    }
}

impl Default for Position {
    fn default() -> Self {
        Position::new()
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
