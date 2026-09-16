//! Board evaluation: material + piece-square tables + light pawn/bishop
//! terms. Values from the Chess Programming Wiki's standard simplified
//! tables (public domain technique, typed out from the published tables).

use crate::piece::{Color, PieceKind};
use crate::position::Position;

pub const PAWN: i32 = 100;
pub const KNIGHT: i32 = 320;
pub const BISHOP: i32 = 330;
pub const ROOK: i32 = 500;
pub const QUEEN: i32 = 900;
pub const KING: i32 = 0; // kings always coexist; their value is not material

pub fn material(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => PAWN,
        PieceKind::Knight => KNIGHT,
        PieceKind::Bishop => BISHOP,
        PieceKind::Rook => ROOK,
        PieceKind::Queen => QUEEN,
        PieceKind::King => KING,
    }
}

/// Piece-square tables for white, rank 8 row first (matching the standard
/// published layout); black indexes the mirrored row.
pub type Pst = [i32; 64];

/// From the mover's point of view: index 0 = a8 ... 63 = h1.
pub const PAWN_PST: Pst = [
    0, 0, 0, 0, 0, 0, 0, 0, //
    50, 50, 50, 50, 50, 50, 50, 50, //
    10, 10, 20, 30, 30, 20, 10, 10, //
    5, 5, 10, 25, 25, 10, 5, 5, //
    0, 0, 0, 20, 20, 0, 0, 0, //
    5, -5, -10, 0, 0, -10, -5, 5, //
    5, 10, 10, -20, -20, 10, 10, 5, //
    0, 0, 0, 0, 0, 0, 0, 0,
];

pub const KNIGHT_PST: Pst = [
    -50, -40, -30, -30, -30, -30, -40, -50, //
    -40, -20, 0, 0, 0, 0, -20, -40, //
    -30, 0, 10, 15, 15, 10, 0, -30, //
    -30, 5, 15, 20, 20, 15, 5, -30, //
    -30, 0, 15, 20, 20, 15, 0, -30, //
    -30, 5, 10, 15, 15, 10, 5, -30, //
    -40, -20, 0, 5, 5, 0, -20, -40, //
    -50, -40, -30, -30, -30, -30, -40, -50,
];

pub const BISHOP_PST: Pst = [
    -20, -10, -10, -10, -10, -10, -10, -20, //
    -10, 0, 0, 0, 0, 0, 0, -10, //
    -10, 0, 5, 10, 10, 5, 0, -10, //
    -10, 5, 5, 10, 10, 5, 5, -10, //
    -10, 0, 10, 10, 10, 10, 0, -10, //
    -10, 10, 10, 10, 10, 10, 10, -10, //
    -10, 5, 0, 0, 0, 0, 5, -10, //
    -20, -10, -10, -10, -10, -10, -10, -20,
];

pub const ROOK_PST: Pst = [
    0, 0, 0, 0, 0, 0, 0, 0, //
    5, 10, 10, 10, 10, 10, 10, 5, //
    -5, 0, 0, 0, 0, 0, 0, -5, //
    -5, 0, 0, 0, 0, 0, 0, -5, //
    -5, 0, 0, 0, 0, 0, 0, -5, //
    -5, 0, 0, 0, 0, 0, 0, -5, //
    -5, 0, 0, 0, 0, 0, 0, -5, //
    0, 0, 0, 5, 5, 0, 0, 0,
];

pub const QUEEN_PST: Pst = [
    -20, -10, -10, -5, -5, -10, -10, -20, //
    -10, 0, 0, 0, 0, 0, 0, -10, //
    -10, 0, 5, 5, 5, 5, 0, -10, //
    -5, 0, 5, 5, 5, 5, 0, -5, //
    0, 0, 5, 5, 5, 5, 0, -5, //
    -10, 5, 5, 5, 5, 5, 0, -10, //
    -10, 0, 5, 0, 0, 0, 0, -10, //
    -20, -10, -10, -5, -5, -10, -10, -20,
];

pub const KING_MID_PST: Pst = [
    -30, -40, -40, -50, -50, -40, -40, -30, //
    -30, -40, -40, -50, -50, -40, -40, -30, //
    -30, -40, -40, -50, -50, -40, -40, -30, //
    -30, -40, -40, -50, -50, -40, -40, -30, //
    -20, -30, -30, -40, -40, -30, -30, -30, //
    -20, -30, -30, -40, -40, -30, -30, -20, //
    -10, -20, -20, -20, -20, -20, -20, -10, //
    20, 20, 0, 0, 0, 0, 20, 20,
];

pub const KING_END_PST: Pst = [
    -50, -40, -30, -20, -20, -30, -40, -50, //
    -30, -20, -10, 0, 0, -10, -20, -30, //
    -30, -10, 20, 30, 30, 20, -10, -30, //
    -30, -10, 30, 40, 40, 30, -10, -30, //
    -30, -10, 30, 40, 40, 30, -10, -30, //
    -30, -10, 20, 30, 30, 20, -10, -30, //
    -30, -30, 0, 0, 0, 0, -30, -30, //
    -50, -30, -30, -30, -30, -30, -30, -50,
];

/// The published tables are written white-to-black (row 0 = rank 8). A
/// white piece on 0x88 square maps to table index
/// `(7 - rank) * 8 + file`; a black piece to `rank * 8 + file`.
pub fn pst_index(color: Color, rank: u8, file: u8) -> usize {
    match color {
        Color::White => (7 - rank) as usize * 8 + file as usize,
        Color::Black => rank as usize * 8 + file as usize,
    }
}

fn pawn_structure(pos: &Position) -> i32 {
    let mut score = 0;
    // Count pawns per file per color.
    let mut white_files = [0u8; 8];
    let mut black_files = [0u8; 8];
    for sq in crate::position::SQUARES {
        if let Some(piece) = pos.board[sq.index()] {
            if piece.kind == PieceKind::Pawn {
                match piece.color {
                    Color::White => white_files[sq.file() as usize] += 1,
                    Color::Black => black_files[sq.file() as usize] += 1,
                }
            }
        }
    }
    // Doubled pawns: -12 each beyond the first on a file.
    for count in white_files {
        if count > 1 {
            score -= 12 * (count - 1) as i32;
        }
    }
    for count in black_files {
        if count > 1 {
            score += 12 * (count - 1) as i32;
        }
    }
    // Isolated pawns: -15 when no friendly pawn is on an adjacent file.
    for (file, count) in white_files.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let left = file > 0 && white_files[file - 1] > 0;
        let right = file < 7 && white_files[file + 1] > 0;
        if !left && !right {
            score -= 15 * *count as i32;
        }
    }
    for (file, count) in black_files.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        let left = file > 0 && black_files[file - 1] > 0;
        let right = file < 7 && black_files[file + 1] > 0;
        if !left && !right {
            score += 15 * *count as i32;
        }
    }
    score
}

/// Static evaluation from the side-to-move's point of view (negamax
/// convention: positive = the side to move is better).
pub fn evaluate(pos: &Position) -> i32 {
    let mut score = 0; // white-positive
    let mut white_bishops = 0;
    let mut black_bishops = 0;
    let mut non_pawn_material = 0; // both sides, for the endgame phase check
    for sq in crate::position::SQUARES {
        if let Some(piece) = pos.board[sq.index()] {
            if piece.kind != PieceKind::King && piece.kind != PieceKind::Pawn {
                non_pawn_material += material(piece.kind);
            }
        }
    }
    let endgame = non_pawn_material <= ROOK + QUEEN;
    for sq in crate::position::SQUARES {
        let Some(piece) = pos.board[sq.index()] else {
            continue;
        };
        let mut value = material(piece.kind);
        match piece.kind {
            PieceKind::Pawn => value += PAWN_PST[pst_index(piece.color, sq.rank(), sq.file())],
            PieceKind::Knight => value += KNIGHT_PST[pst_index(piece.color, sq.rank(), sq.file())],
            PieceKind::Bishop => {
                value += BISHOP_PST[pst_index(piece.color, sq.rank(), sq.file())];
                match piece.color {
                    Color::White => white_bishops += 1,
                    Color::Black => black_bishops += 1,
                }
            }
            PieceKind::Rook => value += ROOK_PST[pst_index(piece.color, sq.rank(), sq.file())],
            PieceKind::Queen => value += QUEEN_PST[pst_index(piece.color, sq.rank(), sq.file())],
            PieceKind::King => {
                // Endgame kings walk to the center.
                value += if endgame {
                    KING_END_PST[pst_index(piece.color, sq.rank(), sq.file())]
                } else {
                    KING_MID_PST[pst_index(piece.color, sq.rank(), sq.file())]
                };
            }
        }
        match piece.color {
            Color::White => score += value,
            Color::Black => score -= value,
        }
    }
    // Bishop pair.
    if white_bishops >= 2 {
        score += 30;
    }
    if black_bishops >= 2 {
        score -= 30;
    }
    score += pawn_structure(pos);
    // Side to move: negate for black.
    if pos.turn == Color::Black {
        -score
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::Square;
    use crate::position::Move;

    #[test]
    fn start_position_is_balanced() {
        let pos = Position::new();
        assert_eq!(evaluate(&pos), 0);
    }

    #[test]
    fn material_advantage_signs() {
        // White up a knight (white to move → positive; black to move → negative).
        let fen = "8/8/8/8/8/8/8/K5Nk w - - 0 1";
        let pos = Position::from_fen(fen).unwrap();
        let white_to_move = evaluate(&pos);
        assert!(white_to_move > 200);
        let black_view = Position::from_fen("8/8/8/8/8/8/8/K5Nk b - - 0 1").unwrap();
        let black_to_move = evaluate(&black_view);
        assert_eq!(black_to_move, -white_to_move, "negamax symmetry");
    }

    #[test]
    fn pst_symmetric_for_mirrored_knights() {
        // White knight d4 vs black knight d5: PST contributions cancel.
        let pos = Position::from_fen("8/8/8/3n4/3N4/8/8/K6k w - - 0 1").unwrap();
        let score = evaluate(&pos);
        assert!(score.abs() < 5, "mirrored knights nearly cancel: {score}");
    }

    #[test]
    fn pawn_structure_terms() {
        // Doubled white pawns d3+d4 (-12), both isolated (-30), black pawn
        // d5 isolated (+15). Material +100, PST: d3 = 0, d4 = +20 for
        // white, d5 = +20 for black.
        let doubled = Position::from_fen("8/8/8/3p4/3P4/3P4/8/K6k w - - 0 1").unwrap();
        assert_eq!(evaluate(&doubled), 73, "doubled+isolated pawns");
        // Single white pawn d4: isolated (-15), PST +20, material +100;
        // black pawn d5: isolated (+15), PST -20, material -100.
        let isolated = Position::from_fen("8/8/8/3p4/3P4/8/8/K6k w - - 0 1").unwrap();
        assert_eq!(evaluate(&isolated), 0, "single isolated pawn pair");
        // Removing the black pawn: white's isolated pawn still costs 15.
        let alone = Position::from_fen("8/8/8/8/3P4/8/8/K6k w - - 0 1").unwrap();
        // Material +100, PST +20, isolated -15.
        assert_eq!(evaluate(&alone), 105, "isolated white pawn only");
    }

    #[test]
    fn bishop_pair_bonus() {
        // Two white bishops vs bishop+same-material: the pair adds 30.
        let two = Position::from_fen("8/8/8/8/8/8/8/KB1B3k w - - 0 1").unwrap();
        // Bishop+knight: same material (330+320 vs 330+330 → differs by
        // 10), so compare against a knight+rook? Cleaner: two bishops vs
        // two bishops-on-opposite... use rook for equal material: 330*2 =
        // 660 vs 500+160? No clean equal. Instead verify via symmetric
        // positions: WB+WB vs b+b (black pair) cancels; swap one black
        // bishop for a knight and the delta is pair(30) - material(10).
        let knight = Position::from_fen("8/8/8/8/8/8/8/KB1N3k w - - 0 1").unwrap();
        let two_score = evaluate(&two);
        let knight_score = evaluate(&knight);
        // Material delta (330 vs 320) + 30 pair = 40; the b1 bishop sits on
        // a -10 PST square where the knight sits on -30, adding 20.
        assert_eq!(two_score - knight_score, 60, "pair bonus + material + PST");
    }

    #[test]
    fn eval_is_symmetric_between_sides() {
        let pos = Position::new();
        let mut mirrored = pos.clone();
        // After white plays e4, black to move: eval flips sign.
        mirrored
            .make_move(Move::Quiet {
                from: Square::parse("e2").unwrap(),
                to: Square::parse("e4").unwrap(),
            })
            .unwrap();
        let after_e4 = evaluate(&mirrored);
        // White to move in the start: 0. After e4, black to move sees the
        // pawn-push PST gains for white.
        assert!(
            after_e4 < 0,
            "black to move, white slightly better: {after_e4}"
        );
    }
}
