//! Testable chess core, no UI imports. The binary owns the GPUI Kit window.

pub mod cli;
pub mod eval;
pub mod icons;
pub mod piece;
pub mod position;
pub mod puzzle;
pub mod san;
pub mod search;
pub mod stats;
pub mod theme;

pub use cli::{parse_cli, CliAction};
pub use piece::{Color, Piece, PieceKind, Square};
pub use position::{GameOver, Move, Position};
pub use san::{format_san, to_pgn};
pub use search::{best_move, Level, SearchOutcome};
pub use theme::{parse_hex_color, OmarchyPalette, RgbaColor};
