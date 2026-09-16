//! SAN move rendering and PGN export. Pure functions over a position +
//! played-move history. `format_san` needs the position *before* the move
//! to compute disambiguation and check/mate suffixes.

use std::fmt::Write as _;

use crate::piece::{PieceKind, Square};
use crate::position::{GameOver, Move, Position};

/// SAN for `mv` in `pos` (the position before the move). The caller applies
/// the move to the position afterwards.
pub fn format_san(pos: &Position, mv: Move) -> String {
    let piece = pos
        .piece_at(mv.from())
        .expect("SAN needs the moving piece on the from square");
    let after = {
        let mut probe = pos.clone();
        probe.make_move(mv).expect("SAN move must be legal");
        probe
    };
    let is_check = after.in_check(after.turn);
    let suffix = if is_check {
        if after.legal_moves().is_empty() {
            "#"
        } else {
            "+"
        }
    } else {
        ""
    };
    match mv {
        Move::Castle { to, .. } => {
            let token = if to.file() == 6 { "O-O" } else { "O-O-O" };
            return format!("{token}{suffix}");
        }
        Move::EnPassant { from, to } => {
            // SAN for e.p. is the same as a pawn capture: <from file>x<to>.
            let mut ep = String::new();
            ep.push(file_letter(from));
            ep.push('x');
            let _ = write!(ep, "{}{suffix}", to.name());
            return ep;
        }
        _ => {}
    }
    let is_capture = mv.is_capture(&pos.board);
    let mut out = String::new();
    if let Some(letter) = piece.kind.san_letter() {
        out.push(letter);
        if piece.kind != PieceKind::King {
            out.push_str(&disambiguation(pos, mv, piece.kind));
        }
    }
    if is_capture {
        if piece.kind == PieceKind::Pawn {
            out.push(file_letter(mv.from()));
        }
        out.push('x');
    }
    let _ = write!(out, "{}", mv.to().name());
    if let Some(kind) = mv.promotion() {
        let _ = write!(out, "={}", kind.san_letter().unwrap_or('Q'));
    }
    out.push_str(suffix);
    out
}

fn file_letter(sq: Square) -> char {
    (b'a' + sq.file()) as char
}

/// Minimal disambiguation: another piece of the same kind+color reaching
/// `mv.to()`? Prefer file letter, then rank, then both.
fn disambiguation(pos: &Position, mv: Move, kind: PieceKind) -> String {
    let others: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|other| {
            other.to() == mv.to()
                && other.from() != mv.from()
                && pos
                    .piece_at(other.from())
                    .map(|p| p.kind == kind)
                    .unwrap_or(false)
        })
        .collect();
    if others.is_empty() {
        return String::new();
    }
    let same_file = others.iter().any(|o| o.from().file() == mv.from().file());
    let same_rank = others.iter().any(|o| o.from().rank() == mv.from().rank());
    if !same_file {
        ((b'a' + mv.from().file()) as char).to_string()
    } else if !same_rank {
        ((b'1' + mv.from().rank()) as char).to_string()
    } else {
        mv.from().name()
    }
}

/// A played game: the moves with their SAN, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRecord {
    pub mv: Move,
    pub san: String,
}

/// Apply `mv` to `pos`, recording its SAN.
pub fn push_san(pos: &mut Position, records: &mut Vec<MoveRecord>, mv: Move) {
    let san = format_san(pos, mv);
    pos.make_move(mv).expect("recorded move must be legal");
    records.push(MoveRecord { mv, san });
}

/// Export a game as PGN. `records` are the SAN records in order; `result`
/// is "*" for an ongoing game.
pub fn to_pgn(
    records: &[MoveRecord],
    result: &str,
    white: &str,
    black: &str,
    date: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Event \"Pi Suite Chess\"]");
    let _ = writeln!(out, "[Site \"Local\"]");
    let _ = writeln!(out, "[Date \"{date}\"]");
    let _ = writeln!(out, "[White \"{white}\"]");
    let _ = writeln!(out, "[Black \"{black}\"]");
    let _ = writeln!(out, "[Result \"{result}\"]");
    out.push('\n');
    let mut line = String::new();
    for (i, record) in records.iter().enumerate() {
        let token = if i % 2 == 0 {
            format!("{}. {}", i / 2 + 1, record.san)
        } else {
            record.san.clone()
        };
        if line.chars().count() + token.chars().count() + 1 > 80 {
            out.push_str(line.trim_end());
            out.push('\n');
            line.clear();
        }
        let _ = write!(line, "{token} ");
    }
    let _ = write!(line, "{result}");
    out.push_str(line.trim_end());
    out.push('\n');
    out
}

/// Game result from the position, "*" while still playing.
pub fn result_token(pos: &Position) -> String {
    match pos.game_over() {
        Some(over) => over.result().to_string(),
        None => "*".into(),
    }
}

/// Player names for the PGN header from the game mode.
pub fn player_names(computer_plays: Option<crate::piece::Color>) -> (String, String) {
    use crate::piece::Color;
    match computer_plays {
        Some(Color::White) => ("Pichess".into(), "You".into()),
        Some(Color::Black) => ("You".into(), "Pichess".into()),
        None => ("White".into(), "Black".into()),
    }
}

/// End label for the footer, e.g. "Checkmate — White wins (1-0)".
pub fn ending_label(over: GameOver) -> String {
    format!("{} ({})", over.label(), over.result())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::Color;

    fn apply(pos: &mut Position, records: &mut Vec<MoveRecord>, uci: &str) {
        let from = Square::parse(&uci[0..2]).unwrap();
        let to = Square::parse(&uci[2..4]).unwrap();
        let mv = if uci.len() > 4 {
            Move::Promotion {
                from,
                to,
                kind: PieceKind::from_letter(uci.chars().nth(4).unwrap()).unwrap(),
            }
        } else {
            Move::Quiet { from, to }
        };
        push_san(pos, records, mv);
    }

    #[test]
    fn opening_sans() {
        let mut pos = Position::new();
        let mut recs = Vec::new();
        apply(&mut pos, &mut recs, "e2e4");
        apply(&mut pos, &mut recs, "e7e5");
        apply(&mut pos, &mut recs, "g1f3");
        apply(&mut pos, &mut recs, "b8c6");
        assert_eq!(
            recs.iter().map(|r| r.san.clone()).collect::<Vec<_>>(),
            vec!["e4", "e5", "Nf3", "Nc6"]
        );
    }

    #[test]
    fn captures_and_check_suffixes() {
        let mut pos = Position::new();
        let mut recs = Vec::new();
        for uci in [
            "e2e4", "d7d5", "e4d5", "d8d5", "b1c3", "g8f6", "f1c4", "f6d5",
        ] {
            apply(&mut pos, &mut recs, uci);
        }
        assert_eq!(recs[2].san, "exd5");
        assert_eq!(recs[7].san, "Nxd5");
    }

    #[test]
    fn knight_disambiguation() {
        // Two knights can reach d2: Nb1-d2 and Nf3-d2? Craft the position.
        let pos = Position::from_fen("k7/8/8/8/8/5N2/8/KN6 w - - 0 1").unwrap();
        let d2 = Square::parse("d2").unwrap();
        let sans: Vec<String> = pos
            .legal_moves()
            .into_iter()
            .filter(|m| m.to() == d2)
            .map(|m| format_san(&pos, m))
            .collect();
        // Nb1d2 and Nf3d2 share the file — needs rank disambiguation.
        assert_eq!(sans, vec!["Nbd2", "Nfd2"]);
    }

    #[test]
    fn rook_disambiguation_by_file() {
        // Rooks a1 + f1 both reach d1; the king on h2 keeps the position
        // sound (no checks to the h8 king).
        let pos = Position::from_fen("7k/8/8/8/8/8/7K/R4R2 w - - 0 1").unwrap();
        let d1 = Square::parse("d1").unwrap();
        let sans: Vec<String> = pos
            .legal_moves()
            .into_iter()
            .filter(|m| m.to() == d1)
            .map(|m| format_san(&pos, m))
            .collect();
        assert_eq!(sans, vec!["Rad1", "Rfd1"]);
    }

    #[test]
    fn castling_sans() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let e1 = Square::parse("e1").unwrap();
        let g1 = Square::parse("g1").unwrap();
        let c1 = Square::parse("c1").unwrap();
        assert_eq!(format_san(&pos, Move::Castle { from: e1, to: g1 }), "O-O");
        assert_eq!(format_san(&pos, Move::Castle { from: e1, to: c1 }), "O-O-O");
    }

    #[test]
    fn promotion_and_mate_suffixes() {
        // White pawn e7 promotes to the empty e8; the new queen checks the
        // black king a8 along the 8th rank. Black rook h8 is out of the way.
        let pos = Position::from_fen("k6r/4P3/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let e7 = Square::parse("e7").unwrap();
        let e8 = Square::parse("e8").unwrap();
        let san = format_san(
            &pos,
            Move::Promotion {
                from: e7,
                to: e8,
                kind: PieceKind::Queen,
            },
        );
        assert_eq!(san, "e8=Q+");
        // Mate: scholar's mate position, white Qxf7#.
        let pos = Position::from_fen(
            "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 1",
        )
        .unwrap();
        let f3 = Square::parse("f3").unwrap();
        let f7 = Square::parse("f7").unwrap();
        let san = format_san(&pos, Move::Quiet { from: f3, to: f7 });
        assert_eq!(san, "Qxf7#");
    }

    #[test]
    fn en_passant_san() {
        let mut pos = Position::new();
        let mut recs = Vec::new();
        apply(&mut pos, &mut recs, "e2e4");
        apply(&mut pos, &mut recs, "d7d5");
        apply(&mut pos, &mut recs, "e4e5");
        apply(&mut pos, &mut recs, "f7f5");
        let exf6 = Move::EnPassant {
            from: Square::parse("e5").unwrap(),
            to: Square::parse("f6").unwrap(),
        };
        assert!(pos.legal_moves().contains(&exf6));
        assert_eq!(format_san(&pos, exf6), "exf6");
    }

    #[test]
    fn pgn_export_shape() {
        let mut pos = Position::new();
        let mut recs = Vec::new();
        apply(&mut pos, &mut recs, "e2e4");
        apply(&mut pos, &mut recs, "e7e5");
        let pgn = to_pgn(&recs, "*", "You", "Pichess", "2026.09.16");
        assert!(pgn.contains("[Event \"Pi Suite Chess\"]"));
        assert!(pgn.contains("[Result \"*\"]"));
        assert!(pgn.contains("1. e4 e5 *"));
    }

    #[test]
    fn pgn_result_tokens() {
        assert_eq!(GameOver::Checkmate(Color::White).result(), "1-0");
        assert_eq!(GameOver::Stalemate.result(), "1/2-1/2");
    }

    #[test]
    fn player_names_by_mode() {
        assert_eq!(
            player_names(Some(Color::White)),
            ("Pichess".into(), "You".into())
        );
        assert_eq!(
            player_names(Some(Color::Black)),
            ("You".into(), "Pichess".into())
        );
        assert_eq!(player_names(None), ("White".into(), "Black".into()));
    }
}
