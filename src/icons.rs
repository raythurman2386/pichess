//! Chess piece and HUD icons served from `assets/icons/`.
//!
//! Pieces are the Cburnett set (Wikimedia Commons, CC BY-SA 3.0 — see
//! README). HUD icons follow the pisweep Lucide pattern.

use std::borrow::Cow;

use gpui_kit::component::IconNamed;
use gpui_kit::{AssetSource, SharedString};
use rust_embed::RustEmbed;

use crate::piece::{Color, Piece, PieceKind};

/// Serves `icons/*.svg` from `assets/`, delegating everything else to
/// gpui-kit's bundled set (the main binary registers this as the app
/// asset source).
#[derive(RustEmbed)]
#[folder = "assets/"]
#[include = "icons/*.svg"]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path.starts_with("icons/") && path.ends_with(".svg") {
            if let Some(data) = Self::get(path) {
                return Ok(Some(data.data));
            }
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChessIcon {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

impl ChessIcon {
    pub fn kind(self) -> PieceKind {
        match self {
            ChessIcon::King => PieceKind::King,
            ChessIcon::Queen => PieceKind::Queen,
            ChessIcon::Rook => PieceKind::Rook,
            ChessIcon::Bishop => PieceKind::Bishop,
            ChessIcon::Knight => PieceKind::Knight,
            ChessIcon::Pawn => PieceKind::Pawn,
        }
    }

    pub fn from_kind(kind: PieceKind) -> ChessIcon {
        match kind {
            PieceKind::King => ChessIcon::King,
            PieceKind::Queen => ChessIcon::Queen,
            PieceKind::Rook => ChessIcon::Rook,
            PieceKind::Bishop => ChessIcon::Bishop,
            PieceKind::Knight => ChessIcon::Knight,
            PieceKind::Pawn => ChessIcon::Pawn,
        }
    }

    /// The icon for a piece: white pieces stay unflipped; black pieces use
    /// the black SVG set.
    pub fn of(piece: Piece) -> ChessIcon {
        ChessIcon::from_kind(piece.kind)
    }

    /// Path served by `AppAssets`. gpui renders SVGs as monochrome alpha
    /// masks, so only the white-set shapes are needed; side colors come
    /// from the tint applied at render time.
    pub fn asset_path(piece: Piece) -> SharedString {
        let path = match piece.kind {
            PieceKind::King => "icons/piece-white-king.svg",
            PieceKind::Queen => "icons/piece-white-queen.svg",
            PieceKind::Rook => "icons/piece-white-rook.svg",
            PieceKind::Bishop => "icons/piece-white-bishop.svg",
            PieceKind::Knight => "icons/piece-white-knight.svg",
            PieceKind::Pawn => "icons/piece-white-pawn.svg",
        };
        path.into()
    }
}

impl IconNamed for ChessIcon {
    /// Neutral path: the white set's alpha mask. Side colors come from the
    /// tint applied at render time.
    fn path(self) -> SharedString {
        Self::asset_path(Piece::new(self.kind(), Color::White))
    }
}

/// HUD icons shared with the rest of the suite (Lucide set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudIcon {
    Flag,
    X,
    Lightbulb,
    Trophy,
    Timer,
    Info,
}

impl IconNamed for HudIcon {
    fn path(self) -> SharedString {
        match self {
            HudIcon::Flag => "icons/flag.svg",
            HudIcon::X => "icons/x.svg",
            HudIcon::Lightbulb => "icons/lightbulb.svg",
            HudIcon::Trophy => "icons/trophy.svg",
            HudIcon::Timer => "icons/timer.svg",
            HudIcon::Info => "icons/info.svg",
        }
        .into()
    }
}
