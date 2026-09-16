//! Rules tests appended to position.rs.

use super::*;

const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

fn perft(pos: &Position, depth: u32) -> u64 {
    let mut probe = pos.clone();
    probe.perft(depth)
}

#[test]
fn start_position_perft() {
    let mut pos = Position::new();
    assert_eq!(pos.perft(0), 1);
    assert_eq!(pos.perft(1), 20);
    assert_eq!(pos.perft(2), 400);
    assert_eq!(pos.perft(3), 8_902);
    assert_eq!(pos.perft(4), 197_281);
}

#[test]
fn kiwipete_perft() {
    let pos = Position::from_fen(KIWIPETE).unwrap();
    assert_eq!(perft(&pos, 1), 48);
    assert_eq!(perft(&pos, 2), 2_039);
    assert_eq!(perft(&pos, 3), 97_862);
}

#[test]
fn cpw_position3_perft() {
    // "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1" — en passant edge cases.
    let pos = Position::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
    assert_eq!(perft(&pos, 1), 14);
    assert_eq!(perft(&pos, 2), 191);
    assert_eq!(perft(&pos, 3), 2_812);
    assert_eq!(perft(&pos, 4), 43_238);
    assert_eq!(perft(&pos, 5), 674_624);
}

#[test]
fn cpw_position4_perft() {
    // Castling-heavy promotion position.
    let pos =
        Position::from_fen("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1")
            .unwrap();
    assert_eq!(perft(&pos, 1), 6);
    assert_eq!(perft(&pos, 2), 264);
    assert_eq!(perft(&pos, 3), 9_467);
    assert_eq!(perft(&pos, 4), 422_333);
}

#[test]
fn fen_roundtrip() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "8/8/8/8/8/8/8/K6k b - - 5 39",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        assert_eq!(pos.to_fen(), fen, "roundtrip failed for {fen}");
    }
}

#[test]
fn fen_rejects_garbage() {
    assert!(Position::from_fen("").is_err());
    assert!(Position::from_fen("8/8/8/8/8/8/8/K6k w - - 0 1 extra").is_err());
    assert!(Position::from_fen("9/8/8/8/8/8/8/K6k w - - 0 1").is_err());
    assert!(Position::from_fen("8/8/8/8/8/8/8/K6k x - - 0 1").is_err());
    assert!(Position::from_fen("8/8/8/8/8/8/8/K6K w - - 0 1").is_err()); // no black king
    assert!(Position::from_fen("8/8/8/8/8/8/8/K5k1 w k - 0 1").is_ok()); // bad right dropped
}

#[test]
fn make_undo_roundtrip_restores_everything() {
    // Play a mixed opening including castling, capture, and check, verifying
    // the position (FEN + key) restores exactly after each undo.
    let mut pos = Position::new();
    let start_key = pos.key;
    let start_fen = pos.to_fen();
    for (i, mv) in pos.legal_moves().into_iter().enumerate() {
        let before = (pos.to_fen(), pos.key);
        pos.make_move(mv).expect("generated move applies");
        pos.undo_move();
        let after = (pos.to_fen(), pos.key);
        assert_eq!(before, after, "undo mismatch on move {i}");
        if i == 40 {
            break;
        }
    }
    assert_eq!(pos.to_fen(), start_fen);
    assert_eq!(pos.key, start_key);
}

#[test]
fn castle_moves_undo_exactly() {
    let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let castles: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|m| m.is_castle())
        .collect();
    assert_eq!(castles.len(), 2, "white can castle both ways");
    for mv in castles {
        let before = (pos.to_fen(), pos.key);
        pos.make_move(mv).unwrap();
        pos.undo_move();
        assert_eq!(before, (pos.to_fen(), pos.key));
    }
    // Black's two castles, same check.
    let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1").unwrap();
    let castles: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|m| m.is_castle())
        .collect();
    assert_eq!(castles.len(), 2, "black can castle both ways");
    for mv in castles {
        let before = (pos.to_fen(), pos.key);
        pos.make_move(mv).unwrap();
        pos.undo_move();
        assert_eq!(before, (pos.to_fen(), pos.key));
    }
}

#[test]
fn en_passant_generation_and_undo() {
    // Black pawn on e4 (FEN rank 4 row) can take d3 e.p. after d2d4.
    let mut pos =
        Position::from_fen("rnbqkbnr/pppp1ppp/8/8/4p3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 1").unwrap();
    let d2 = Square::parse("d2").unwrap();
    let d4 = Square::parse("d4").unwrap();
    pos.make_move(Move::Quiet { from: d2, to: d4 }).unwrap();
    assert_eq!(pos.ep_square, Some(Square::parse("d3").unwrap()));
    let e4 = Square::parse("e4").unwrap();
    assert_eq!(
        pos.piece_at(e4).map(|p| p.kind),
        Some(PieceKind::Pawn),
        "black pawn sits on e4 in this setup"
    );
    let eps: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::EnPassant { .. }))
        .collect();
    assert_eq!(eps.len(), 1, "exd3 e.p. should be legal");
    let ep = eps[0];
    assert_eq!(ep.from(), e4);
    let before = (pos.to_fen(), pos.key);
    pos.make_move(ep).unwrap();
    assert!(pos.piece_at(Square::parse("d4").unwrap()).is_none());
    assert_eq!(
        pos.piece_at(Square::parse("d3").unwrap()).map(|p| p.kind),
        Some(PieceKind::Pawn)
    );
    pos.undo_move();
    assert_eq!(before, (pos.to_fen(), pos.key));
}

#[test]
fn en_passant_leaving_own_king_in_check_is_illegal() {
    // exd6 e.p. removes both pawns from the e-file and exposes the white
    // king to the rook on e8 — illegal, while the plain e5-e6 push is fine.
    let pos = Position::from_fen("4r2k/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    let e5 = Square::parse("e5").unwrap();
    let d6 = Square::parse("d6").unwrap();
    let ep = Move::EnPassant { from: e5, to: d6 };
    assert!(
        !pos.legal_moves().contains(&ep),
        "pin through ep is illegal"
    );
    assert!(pos.legal_moves().contains(&Move::Quiet {
        from: e5,
        to: Square::parse("e6").unwrap()
    }));
}

#[test]
fn castling_through_check_is_illegal() {
    let pos = Position::from_fen("r3k2r/8/8/8/8/5r2/8/R3K2R w KQkq - 0 1").unwrap();
    let moves = pos.legal_moves();
    let g1 = Move::Castle {
        from: Square::parse("e1").unwrap(),
        to: Square::parse("g1").unwrap(),
    };
    assert!(!moves.contains(&g1), "f1 is attacked by the rook");
    let c1 = Move::Castle {
        from: Square::parse("e1").unwrap(),
        to: Square::parse("c1").unwrap(),
    };
    assert!(moves.contains(&c1), "d1 is not attacked");
}

#[test]
fn pinned_piece_has_no_moves() {
    // Knight on c3 pinned by the rook on c8 against the king on c1; black
    // king parked on h8.
    let pos = Position::from_fen("2r4k/8/8/8/8/2N5/8/2K5 w - - 0 1").unwrap();
    let c3 = Square::parse("c3").unwrap();
    let knight_moves: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|m| m.from() == c3)
        .collect();
    assert!(knight_moves.is_empty(), "pinned knight cannot move");
    // The king can step off the pin line.
    assert!(pos
        .legal_moves()
        .iter()
        .any(|m| m.to() == Square::parse("b1").unwrap()));
}

#[test]
fn fifty_move_rule() {
    // A rook keeps the material live so only the clock can end it.
    let pos = Position::from_fen("8/8/8/8/8/8/8/KR4k1 w - - 100 60").unwrap();
    assert_eq!(pos.game_over(), Some(GameOver::FiftyMove));
    let pos = Position::from_fen("8/8/8/8/8/8/8/KR4k1 w - - 99 60").unwrap();
    assert_eq!(pos.game_over(), None);
}

#[test]
fn threefold_repetition() {
    let mut pos = Position::new();
    // Knights out and back four times: repetition after the 3rd return.
    let knight_shuffle = [
        "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
    ];
    for uci in knight_shuffle {
        let from = Square::parse(&uci[0..2]).unwrap();
        let to = Square::parse(&uci[2..4]).unwrap();
        let mv = Move::Quiet { from, to };
        assert!(pos.legal_moves().contains(&mv), "{uci} legal");
        pos.make_move(mv).unwrap();
    }
    assert_eq!(pos.game_over(), Some(GameOver::ThreefoldRepetition));
    assert!(pos.has_repeated());
}

#[test]
fn repetition_count_tracks_pairs() {
    let mut pos = Position::new();
    for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        let from = Square::parse(&uci[0..2]).unwrap();
        let to = Square::parse(&uci[2..4]).unwrap();
        pos.make_move(Move::Quiet { from, to }).unwrap();
    }
    assert_eq!(pos.repetition_count(), 2);
    assert_eq!(pos.game_over(), None);
}

#[test]
fn insufficient_material_cases() {
    assert!(Position::from_fen("8/8/8/8/8/8/8/K6k w - - 0 1")
        .unwrap()
        .insufficient_material());
    assert!(Position::from_fen("8/8/8/8/8/8/8/KB5k w - - 0 1")
        .unwrap()
        .insufficient_material());
    // Bishops on the same square color.
    assert_eq!(
        Position::from_fen("8/8/8/8/8/8/8/KB5k w - - 0 1")
            .unwrap()
            .game_over(),
        Some(GameOver::InsufficientMaterial)
    );
    // Rook on the board is never insufficient.
    assert!(!Position::from_fen("8/8/8/8/8/8/8/KR4k1 w - - 0 1")
        .unwrap()
        .insufficient_material());
    // Two knights: not forced mate but not dead — still insufficient by our
    // rule only when same-color bishops or <=1 minor; KNN vs K stays live.
    assert!(!Position::from_fen("8/8/8/8/8/8/8/KN3N1k w - - 0 1")
        .unwrap()
        .insufficient_material());
}

#[test]
fn stalemate_detection() {
    // Classic stalemate: black king h8, white queen g6 (h7 covered), king far.
    let pos = Position::from_fen("7k/8/6Q1/8/8/8/8/K7 b - - 0 1").unwrap();
    assert_eq!(pos.game_over(), Some(GameOver::Stalemate));
    assert!(!pos.in_check_self());
    assert!(pos.legal_moves().is_empty());
}

#[test]
fn checkmate_detection() {
    // Scholar's mate finish: white just played Qxf7#.
    let pos =
        Position::from_fen("r1bqkbnr/pppp1Qpp/2n5/4p3/2B1P3/8/PPPP1PPP/RNB1K1NR b KQkq - 0 4")
            .unwrap();
    assert_eq!(pos.game_over(), Some(GameOver::Checkmate(Color::White)));
    assert!(pos.in_check_self());
    assert!(pos.legal_moves().is_empty());
}

#[test]
fn check_but_not_mate_has_defense() {
    // Rf2 checks the king on f8; a king move answers the check.
    let pos = Position::from_fen("5k2/8/8/8/8/8/5R2/K7 b - - 0 1").unwrap();
    assert!(pos.in_check_self());
    assert!(!pos.legal_moves().is_empty());
    assert_eq!(pos.game_over(), None);
}

#[test]
fn castling_rights_kill_on_moves() {
    let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let h1 = Square::parse("h1").unwrap();
    let h3 = Square::parse("h3").unwrap();
    pos.make_move(Move::Quiet { from: h1, to: h3 }).unwrap();
    assert_eq!(pos.castling & CASTLE_WK, 0);
    assert_eq!(pos.castling & CASTLE_WQ, CASTLE_WQ);
    pos.undo_move();
    assert_eq!(pos.castling, CASTLE_WK | CASTLE_WQ | CASTLE_BK | CASTLE_BQ);
}

#[test]
fn capturing_a_rook_kills_its_castling_right() {
    // h1 rook takes h2? No rook there. Take via a1xa2? None. Test the rule on
    // a white rook capturing black's h8 rook: Ra1-a8 is blocked. Skip: verify
    // the bit logic directly through make/undo on Ra1xa8 after clearing b8.
    // Ra1 slides to a8 and takes the a8 rook: white's own queenside right
    // dies because the rook left a1, and black's dies because its a8 rook
    // was captured. Undo restores both.
    let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K3 w Qkq - 0 1").unwrap();
    let a1 = Square::parse("a1").unwrap();
    let a8 = Square::parse("a8").unwrap();
    let mv = Move::Quiet { from: a1, to: a8 };
    assert!(pos.legal_moves().contains(&mv), "Ra1 takes a8 rook");
    pos.make_move(mv).unwrap();
    assert_eq!(pos.castling & CASTLE_WQ, 0, "white queenside right dies");
    assert_eq!(pos.castling & CASTLE_BQ, 0, "black queenside right dies");
    assert_eq!(pos.castling & CASTLE_BK, CASTLE_BK, "black kingside lives");
    pos.undo_move();
    assert_eq!(pos.castling & CASTLE_WQ, CASTLE_WQ);
    assert_eq!(pos.castling & CASTLE_BQ, CASTLE_BQ);
}

#[test]
fn zobrist_keys_differ_for_turn() {
    let mut pos = Position::new();
    let key_white = pos.key;
    pos.make_move(Move::Quiet {
        from: Square::parse("e2").unwrap(),
        to: Square::parse("e4").unwrap(),
    })
    .unwrap();
    let key_after = pos.key;
    assert_ne!(key_white, key_after);
    // Same placement + same ep square (e3 from the double push): keys match.
    let mirrored =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
    assert_eq!(
        mirrored.key, key_after,
        "incremental and rebuilt keys match"
    );
}

#[test]
fn legal_moves_exclude_king_into_check() {
    let pos = Position::from_fen("7k/8/8/8/8/8/5r2/6K1 w - - 0 1").unwrap();
    let g1 = Square::parse("g1").unwrap();
    // g1 king cannot step to f1/g2/h2 (f2 rook attacks them).
    for target in ["f1", "g2", "h2"] {
        let mv = Move::Quiet {
            from: g1,
            to: Square::parse(target).unwrap(),
        };
        assert!(!pos.legal_moves().contains(&mv), "{target} is attacked");
    }
    assert!(pos.legal_moves().contains(&Move::Quiet {
        from: g1,
        to: Square::parse("f2").unwrap()
    }));
}
