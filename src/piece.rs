//! Chess pieces, colors, and squares.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    /// 0 for white, 1 for black — indexes into two-slot arrays.
    pub fn index(self) -> usize {
        match self {
            Color::White => 0,
            Color::Black => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Color::White => "White",
            Color::Black => "Black",
        }
    }

    /// +1 for white, -1 for black, the sign of a material advantage.
    pub fn sign(self) -> i32 {
        match self {
            Color::White => 1,
            Color::Black => -1,
        }
    }

    /// Forward direction in 0x88 terms: +16 for white, -16 for black.
    pub fn pawn_dir(self) -> i32 {
        match self {
            Color::White => 16,
            Color::Black => -16,
        }
    }

    /// The rank a pawn starts on.
    pub fn pawn_rank(self) -> u8 {
        match self {
            Color::White => 1,
            Color::Black => 6,
        }
    }

    /// The rank a pawn promotes on.
    pub fn promo_rank(self) -> u8 {
        match self {
            Color::White => 7,
            Color::Black => 0,
        }
    }

    /// The home rank of the king.
    pub fn back_rank(self) -> u8 {
        match self {
            Color::White => 0,
            Color::Black => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    pub fn letter(self) -> char {
        match self {
            PieceKind::Pawn => 'p',
            PieceKind::Knight => 'n',
            PieceKind::Bishop => 'b',
            PieceKind::Rook => 'r',
            PieceKind::Queen => 'q',
            PieceKind::King => 'k',
        }
    }

    /// SAN letter (upper for pieces, none for pawns).
    pub fn san_letter(self) -> Option<char> {
        match self {
            PieceKind::Pawn => None,
            PieceKind::Knight => Some('N'),
            PieceKind::Bishop => Some('B'),
            PieceKind::Rook => Some('R'),
            PieceKind::Queen => Some('Q'),
            PieceKind::King => Some('K'),
        }
    }

    pub fn from_letter(c: char) -> Option<PieceKind> {
        match c {
            'p' | 'P' => Some(PieceKind::Pawn),
            'n' | 'N' => Some(PieceKind::Knight),
            'b' | 'B' => Some(PieceKind::Bishop),
            'r' | 'R' => Some(PieceKind::Rook),
            'q' | 'Q' => Some(PieceKind::Queen),
            'k' | 'K' => Some(PieceKind::King),
            _ => None,
        }
    }

    pub fn all() -> [PieceKind; 6] {
        [
            PieceKind::Pawn,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Rook,
            PieceKind::Queen,
            PieceKind::King,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub kind: PieceKind,
    pub color: Color,
}

impl Piece {
    pub fn new(kind: PieceKind, color: Color) -> Self {
        Self { kind, color }
    }

    /// FEN letter: upper for white, lower for black.
    pub fn fen_letter(self) -> char {
        let letter = self.kind.letter();
        match self.color {
            Color::White => letter.to_ascii_uppercase(),
            Color::Black => letter,
        }
    }
}

/// A square in 0x88 indexing: `sq = rank * 16 + file`, 0..128, off-board
/// squares have `sq & 0x88 != 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Square(pub u8);

impl Square {
    pub fn new(rank: u8, file: u8) -> Square {
        Square(rank * 16 + file)
    }

    pub fn from_index(i: usize) -> Square {
        Square(i as u8)
    }

    pub fn rank(self) -> u8 {
        self.0 >> 4
    }

    pub fn file(self) -> u8 {
        self.0 & 0x0f
    }

    pub fn on_board(self) -> bool {
        self.0 & 0x88 == 0
    }

    pub fn is_light(self) -> bool {
        (self.rank() + self.file()) % 2 == 1
    }

    /// Algebraic name, `a1`..`h8`.
    pub fn name(self) -> String {
        let file = (b'a' + self.file()) as char;
        let rank = (b'1' + self.rank()) as char;
        format!("{file}{rank}")
    }

    /// Parse `a1`..`h8`.
    pub fn parse(s: &str) -> Option<Square> {
        let mut chars = s.chars();
        let file = chars.next()?;
        let rank = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let file = file.to_ascii_lowercase();
        if !('a'..='h').contains(&file) || !('1'..='8').contains(&rank) {
            return None;
        }
        Some(Square::new(rank as u8 - b'1', file as u8 - b'a'))
    }

    /// The color of the square a bishop on `self` would fight for.
    pub fn square_color(self) -> Color {
        if self.is_light() {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_names_roundtrip() {
        for file in 0u8..8 {
            for rank in 0u8..8 {
                let sq = Square::new(rank, file);
                let name = sq.name();
                assert_eq!(Square::parse(&name), Some(sq));
            }
        }
        assert_eq!(Square::parse("a1"), Some(Square::new(0, 0)));
        assert_eq!(Square::parse("H8"), Some(Square::new(7, 7)));
        assert_eq!(Square::parse("i1"), None);
        assert_eq!(Square::parse("a9"), None);
        assert_eq!(Square::parse("a"), None);
    }

    #[test]
    fn off_board_detection() {
        assert!(Square::new(0, 0).on_board());
        assert!(Square::new(7, 7).on_board());
        assert!(!Square(0x08).on_board());
        assert!(!Square(0x80).on_board());
        assert!(!Square(0x88).on_board());
        assert!(!Square(127).on_board());
    }

    #[test]
    fn square_colors() {
        assert!(!Square::new(0, 0).is_light());
        assert!(!Square::new(7, 7).is_light());
        assert!(Square::new(0, 1).is_light());
        assert!(Square::parse("e4").unwrap().is_light());
    }

    #[test]
    fn color_helpers() {
        assert_eq!(Color::White.opposite(), Color::Black);
        assert_eq!(Color::White.pawn_dir(), 16);
        assert_eq!(Color::Black.pawn_dir(), -16);
        assert_eq!(Color::White.promo_rank(), 7);
        assert_eq!(Color::Black.promo_rank(), 0);
        assert_eq!(Color::Black.sign(), -1);
    }

    #[test]
    fn piece_fen_letters() {
        assert_eq!(
            Piece::new(PieceKind::Knight, Color::White).fen_letter(),
            'N'
        );
        assert_eq!(Piece::new(PieceKind::Pawn, Color::Black).fen_letter(), 'p');
        assert_eq!(
            Piece::new(PieceKind::Queen, Color::White).kind.san_letter(),
            Some('Q')
        );
        assert_eq!(
            Piece::new(PieceKind::Pawn, Color::White).kind.san_letter(),
            None
        );
        assert_eq!(PieceKind::from_letter('K'), Some(PieceKind::King));
        assert_eq!(PieceKind::from_letter('z'), None);
    }
}
