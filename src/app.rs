//! GPUI Kit chess window. Rules, SAN, and the engine live in the lib.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::label::Label;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Icon, IconName, Root, Sizable as _, Theme, ThemeMode,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pichess::piece::{Color, Piece, PieceKind, Square};
use pichess::position::{Move, Position};
use pichess::san::{ending_label, player_names, push_san, to_pgn, MoveRecord};
use pichess::search::{search_with, Level, ThreadRng};
use pichess::stats::{store_paths, Mode, SavedGame, Settings, Store};

actions!(
    pichess_actions,
    [
        SelectOrMove,
        Cancel,
        Undo,
        NewGame,
        Resign,
        ExportPgn,
        EngineHint,
        FlipBoard,
        Level1,
        Level2,
        Level3,
        ModeHumanWhite,
        ModeHumanBlack,
        ModeTwoPlayers,
        PromoteQueen,
        PromoteRook,
        PromoteBishop,
        PromoteKnight,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        ToggleHelp,
        ToggleFullscreen,
        Quit
    ]
);

pub fn init(cx: &mut App) {
    load_fonts(cx);
    cx.bind_keys([
        KeyBinding::new("space", SelectOrMove, None),
        KeyBinding::new("enter", SelectOrMove, None),
        KeyBinding::new("escape", Cancel, None),
        KeyBinding::new("u", Undo, None),
        KeyBinding::new("n", NewGame, None),
        KeyBinding::new("r", Resign, None),
        KeyBinding::new("e", ExportPgn, None),
        KeyBinding::new("a", EngineHint, None),
        KeyBinding::new("v", FlipBoard, None),
        KeyBinding::new("1", Level1, None),
        KeyBinding::new("2", Level2, None),
        KeyBinding::new("3", Level3, None),
        KeyBinding::new("q", PromoteQueen, None),
        KeyBinding::new("r2", PromoteRook, None),
        KeyBinding::new("b", PromoteBishop, None),
        KeyBinding::new("n2", PromoteKnight, None),
        KeyBinding::new("left", MoveLeft, None),
        KeyBinding::new("h", MoveLeft, None),
        KeyBinding::new("right", MoveRight, None),
        KeyBinding::new("l", MoveRight, None),
        KeyBinding::new("up", MoveUp, None),
        KeyBinding::new("k", MoveUp, None),
        KeyBinding::new("down", MoveDown, None),
        KeyBinding::new("j", MoveDown, None),
        KeyBinding::new("shift-/", ToggleHelp, None),
        KeyBinding::new("f11", ToggleFullscreen, None),
        KeyBinding::new("super-f", ToggleFullscreen, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
}

fn load_fonts(cx: &mut App) {
    let fonts: [&'static [u8]; 4] = [
        include_bytes!("../fonts/iAWriterMonoS-Regular.ttf"),
        include_bytes!("../fonts/iAWriterMonoS-Italic.ttf"),
        include_bytes!("../fonts/iAWriterMonoS-Bold.ttf"),
        include_bytes!("../fonts/iAWriterMonoS-BoldItalic.ttf"),
    ];
    let blobs = fonts.into_iter().map(Cow::Borrowed).collect::<Vec<_>>();
    let _ = cx.text_system().add_fonts(blobs);
}

pub fn open_window(cx: &AsyncApp) -> anyhow::Result<WindowHandle<Root>> {
    cx.open_window(window_options(), move |window, cx| {
        let view: Entity<PichessApp> = cx.new(|cx| PichessApp::new(window, cx));
        cx.new(|cx| {
            let any_view: AnyView = view.into();
            Root::new(any_view, window, cx)
        })
    })
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn window_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(80.), px(60.)),
            size: size(px(1020.), px(700.)),
        })),
        window_min_size: Some(size(px(760.), px(560.))),
        titlebar: Some(TitlebarOptions {
            title: Some("Pichess".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(9.))),
        }),
        app_id: Some("pichess".into()),
        ..Default::default()
    }
}

struct ThemeWatch {
    events: Arc<Mutex<Vec<std::path::PathBuf>>>,
    watcher: Option<RecommendedWatcher>,
    watched: Vec<std::path::PathBuf>,
}

impl ThemeWatch {
    fn new() -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let tx = events.clone();
        let watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Create(_)
                    ) {
                        if let Ok(mut queue) = tx.lock() {
                            queue.extend(event.paths);
                        }
                    }
                }
            },
            notify::Config::default(),
        )
        .ok();
        let mut this = Self {
            events,
            watcher,
            watched: Vec::new(),
        };
        this.watch_all(pichess::theme::omarchy_watch_paths());
        this
    }

    fn watch_all(&mut self, paths: impl IntoIterator<Item = std::path::PathBuf>) {
        self.unwatch_all();
        for path in paths {
            self.watch_path(&path);
        }
    }

    fn watch_path(&mut self, path: &std::path::Path) {
        if !path.exists() || self.watched.iter().any(|p| p == path) {
            return;
        }
        if let Some(watcher) = self.watcher.as_mut() {
            if watcher.watch(path, RecursiveMode::NonRecursive).is_ok() {
                self.watched.push(path.to_path_buf());
            }
        }
    }

    fn unwatch_all(&mut self) {
        if let Some(watcher) = self.watcher.as_mut() {
            for path in self.watched.drain(..) {
                let _ = watcher.unwatch(&path);
            }
        } else {
            self.watched.clear();
        }
    }

    fn drain(&self) -> Vec<std::path::PathBuf> {
        self.events
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    fn needs_rearm(&self) -> bool {
        self.watched.iter().any(|path| !path.exists())
    }
}

/// A pawn move to the last rank awaits the promotion choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingPromotion {
    from: Square,
    to: Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    None,
    Square(Square),
    Promoting(PendingPromotion),
}

/// Pause before the engine plays its move, so the human's move has a
/// moment to register on the board.
const ENGINE_MOVE_DELAY: Duration = Duration::from_millis(650);

pub struct PichessApp {
    palette: pichess::theme::OmarchyPalette,
    text_scale: f32,
    theme_watch: ThemeWatch,
    focus_handle: FocusHandle,
    position: Position,
    /// SAN records aligned with the moves played from the start position.
    records: Vec<MoveRecord>,
    mode: Mode,
    level: Level,
    flipped: bool,
    cursor: Square,
    selection: Selection,
    /// The engine is computing on a background task.
    thinking: bool,
    /// Bumped on new game / undo / mode change; stale results are dropped.
    generation: u64,
    pending_engine: Option<Task<()>>,
    status: String,
    resigned: Option<Color>,
    show_help: bool,
    settings: Settings,
    store: Store,
    rng: ThreadRng,
    _appearance_sub: Subscription,
    _poll_task: Task<()>,
}

impl Focusable for PichessApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PichessApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dark = pichess::theme::detect_system_dark();
        let palette = pichess::theme::OmarchyPalette::load(dark);
        let text_scale = pichess::theme::detect_text_scale();
        let focus_handle = cx.focus_handle();
        let (settings_path, game_path, _) = store_paths();
        let store = Store::new(settings_path, game_path);
        let settings = store.load_settings();
        let mode = Mode::parse(&settings.mode);
        let level = Level::parse(&settings.level);
        let flipped = settings.flipped;
        // Resume the saved game when it parses; corrupt saves reset silently.
        let resumed = store.load_game().and_then(|saved| restore_game(&saved));
        let (position, records) = resumed.unwrap_or_else(|| (Position::new(), Vec::new()));

        apply_palette(&palette, Some(window), cx);
        window.set_window_title("Pichess");

        let (appearance_sub, poll_task) = Self::start_theme_poll(window, cx);

        let mut app = Self {
            palette,
            text_scale,
            theme_watch: ThemeWatch::new(),
            focus_handle,
            position,
            records,
            mode,
            level,
            flipped,
            cursor: Square::new(4, 4),
            selection: Selection::None,
            thinking: false,
            generation: 0,
            pending_engine: None,
            status: String::new(),
            resigned: None,
            show_help: false,
            settings,
            store,
            rng: ThreadRng::default(),
            _appearance_sub: appearance_sub,
            _poll_task: poll_task,
        };
        app.refresh_status();
        if app.engine_to_move() && !app.game_ended() {
            app.spawn_engine_move(cx);
        }
        let handle = app.focus_handle.clone();
        window.focus(&handle, cx);
        app
    }

    fn start_theme_poll(window: &mut Window, cx: &mut Context<Self>) -> (Subscription, Task<()>) {
        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            this.poll_theme(window, cx);
        });
        let poll = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            if this
                .update_in(cx, |this, window, cx| this.poll_theme(window, cx))
                .is_err()
            {
                break;
            }
        });
        (appearance, poll)
    }

    fn poll_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events = self.theme_watch.drain();
        if !events.is_empty() || self.theme_watch.needs_rearm() {
            self.theme_watch
                .watch_all(pichess::theme::omarchy_watch_paths());
        }
        let palette = pichess::theme::OmarchyPalette::load(pichess::theme::detect_system_dark());
        if palette != self.palette {
            self.palette = palette;
            apply_palette(&self.palette, Some(window), cx);
            cx.notify();
        }
        let scale = pichess::theme::detect_text_scale();
        if (scale - self.text_scale).abs() > f32::EPSILON {
            self.text_scale = scale;
            apply_palette(&self.palette, Some(window), cx);
            cx.notify();
        }
    }

    fn computer_plays(&self) -> Option<Color> {
        self.mode.computer_plays()
    }

    fn engine_to_move(&self) -> bool {
        self.computer_plays() == Some(self.position.turn) && !self.thinking
    }

    fn game_ended(&self) -> bool {
        self.resigned.is_some() || self.position.game_over().is_some()
    }

    fn human_controls(&self, color: Color) -> bool {
        self.computer_plays() != Some(color)
    }

    fn refresh_status(&mut self) {
        self.status = if let Some(resigner) = self.resigned {
            format!("Resignation — {} wins", resigner.opposite().label())
        } else if let Some(over) = self.position.game_over() {
            ending_label(over)
        } else if self.thinking {
            format!("{} is thinking…", self.position.turn.label())
        } else if self.position.in_check_self() {
            format!("Check! {} to move", self.position.turn.label())
        } else {
            format!("{} to move", self.position.turn.label())
        };
    }

    fn save_game(&self) {
        let moves: Vec<String> = self.records.iter().map(|r| r.mv.to_string()).collect();
        self.store.save_game(&SavedGame {
            moves,
            mode: self.mode.id().into(),
            level: self.level.id().into(),
            flipped: self.flipped,
        });
    }

    fn save_settings(&mut self) {
        self.settings.mode = self.mode.id().into();
        self.settings.level = self.level.id().into();
        self.settings.flipped = self.flipped;
        self.store.save_settings(&self.settings);
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        self.cancel_engine();
        self.position = Position::new();
        self.records.clear();
        self.selection = Selection::None;
        self.resigned = None;
        self.generation += 1;
        self.refresh_status();
        self.store.clear_game();
        if self.engine_to_move() {
            self.spawn_engine_move(cx);
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.cancel_engine();
        let side_changed = matches!(
            (self.mode, mode),
            (Mode::HumanWhite, Mode::HumanBlack) | (Mode::HumanBlack, Mode::HumanWhite)
        );
        self.mode = mode;
        // Switching seats turns the board to face the new side.
        if side_changed {
            self.flipped = mode == Mode::HumanBlack;
            self.selection = Selection::None;
        }
        self.save_settings();
        self.refresh_status();
        if self.engine_to_move() && !self.game_ended() {
            self.spawn_engine_move(cx);
        }
        cx.notify();
    }

    fn set_level(&mut self, level: Level, cx: &mut Context<Self>) {
        if self.level == level {
            return;
        }
        self.level = level;
        self.save_settings();
        // A change mid-think applies to the move in progress.
        if self.thinking {
            self.cancel_engine();
            if self.engine_to_move() && !self.game_ended() {
                self.spawn_engine_move(cx);
            }
        }
        cx.notify();
    }

    fn flip(&mut self, cx: &mut Context<Self>) {
        self.flipped = !self.flipped;
        self.save_settings();
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if self.thinking {
            return;
        }
        // In engine games take back the engine's reply too; in two-player
        // mode one ply at a time.
        let plies = if self.computer_plays().is_some() {
            2
        } else {
            1
        };
        let mut undone = 0;
        while undone < plies && self.position.undo_stack_len() > 0 {
            self.position.undo_move();
            self.records.pop();
            undone += 1;
        }
        self.resigned = None;
        self.generation += 1;
        self.selection = Selection::None;
        self.refresh_status();
        self.save_game();
        if self.engine_to_move() && !self.game_ended() {
            self.spawn_engine_move(cx);
        }
        cx.notify();
    }

    fn resign(&mut self, cx: &mut Context<Self>) {
        if self.game_ended() {
            return;
        }
        self.resigned = Some(self.position.turn);
        self.cancel_engine();
        self.record_result(pichess::position::GameOver::Checkmate(
            self.resigned.expect("just set").opposite(),
        ));
        self.refresh_status();
        self.store.clear_game();
        cx.notify();
    }

    fn record_result(&mut self, over: pichess::position::GameOver) {
        let Some(computer) = self.computer_plays() else {
            return;
        };
        let record = self.settings.record_mut(self.level);
        match over {
            pichess::position::GameOver::Checkmate(winner) => {
                if winner == computer {
                    record.losses += 1;
                } else {
                    record.wins += 1;
                }
            }
            _ => record.draws += 1,
        }
        self.store.save_settings(&self.settings);
    }

    fn apply_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        push_san(&mut self.position, &mut self.records, mv);
        self.selection = Selection::None;
        self.cursor = mv.to();
        self.refresh_status();
        self.save_game();
        if let Some(over) = self.position.game_over() {
            self.record_result(over);
        }
        cx.notify();
        if self.engine_to_move() && !self.game_ended() {
            self.spawn_engine_move(cx);
        }
    }

    fn cancel_engine(&mut self) {
        // Dropping the task aborts the background search; the generation
        // counter makes a racing completion a no-op.
        self.pending_engine = None;
        self.thinking = false;
        self.generation += 1;
    }

    fn spawn_engine_move(&mut self, cx: &mut Context<Self>) {
        if !self.engine_to_move() || self.game_ended() {
            return;
        }
        self.thinking = true;
        self.generation += 1;
        let generation = self.generation;
        let probe = self.position.clone();
        let level = self.level;
        let rng_state = self.rng.state();
        self.refresh_status();
        let search_task = cx.background_executor().spawn(async move {
            let mut rng = ThreadRng::from_state(rng_state);
            search_with(&probe, level, &mut rng)
        });
        let task = cx.spawn(async move |this, cx| {
            let outcome = search_task.await;
            // A short beat before playing so the human's move registers —
            // instant replies are easy to miss. Cancelled with the rest by
            // the generation check.
            cx.background_executor().timer(ENGINE_MOVE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.pending_engine = None;
                    this.thinking = false;
                    if this.position.legal_moves().contains(&outcome.mv) {
                        this.apply_move(outcome.mv, cx);
                    }
                }
            });
        });
        self.pending_engine = Some(task);
    }

    /// `a`: the engine plays one move for the side to move (a hint, or auto
    /// play in two-player mode).
    fn engine_hint(&mut self, cx: &mut Context<Self>) {
        if self.thinking || self.game_ended() {
            return;
        }
        self.selection = Selection::None;
        self.thinking = true;
        self.generation += 1;
        let generation = self.generation;
        let probe = self.position.clone();
        let level = self.level;
        let rng_state = self.rng.state();
        let search_task = cx.background_executor().spawn(async move {
            let mut rng = ThreadRng::from_state(rng_state);
            search_with(&probe, level, &mut rng)
        });
        let task = cx.spawn(async move |this, cx| {
            let outcome = search_task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.pending_engine = None;
                    this.thinking = false;
                    if this.position.legal_moves().contains(&outcome.mv) {
                        this.apply_move(outcome.mv, cx);
                    }
                }
            });
        });
        self.pending_engine = Some(task);
    }

    // ── selection and moves ─────────────────────────────────────────────

    fn legal_moves_from(&self, from: Square) -> Vec<Move> {
        self.position
            .legal_moves()
            .into_iter()
            .filter(|mv| mv.from() == from)
            .collect()
    }

    /// Click (or cursor-select) a square: pick up, drop, or deselect.
    fn square_clicked(&mut self, sq: Square, cx: &mut Context<Self>) {
        self.cursor = sq;
        match self.selection {
            Selection::Promoting(_) => {}
            Selection::Square(from) => {
                if from == sq {
                    self.selection = Selection::None;
                    cx.notify();
                    return;
                }
                let candidates: Vec<Move> = self
                    .legal_moves_from(from)
                    .into_iter()
                    .filter(|mv| mv.to() == sq)
                    .collect();
                if candidates.is_empty() {
                    self.pick_up(sq, cx);
                } else if candidates.len() == 1 {
                    let mv = candidates[0];
                    self.finish_move(mv, cx);
                } else {
                    // Promotion ambiguity (4 moves to one square).
                    self.selection = Selection::Promoting(PendingPromotion { from, to: sq });
                    cx.notify();
                }
            }
            Selection::None => self.pick_up(sq, cx),
        }
    }

    fn pick_up(&mut self, sq: Square, cx: &mut Context<Self>) {
        let Some(piece) = self.position.piece_at(sq) else {
            self.selection = Selection::None;
            cx.notify();
            return;
        };
        if piece.color != self.position.turn
            || !self.human_controls(piece.color)
            || self.game_ended()
            || self.thinking
        {
            self.selection = Selection::None;
            cx.notify();
            return;
        }
        if self.legal_moves_from(sq).is_empty() {
            self.selection = Selection::None;
            cx.notify();
            return;
        }
        self.selection = Selection::Square(sq);
        cx.notify();
    }

    fn finish_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        // Promotions route through the picker.
        if mv.promotion().is_some() && !matches!(self.selection, Selection::Promoting(_)) {
            self.selection = Selection::Promoting(PendingPromotion {
                from: mv.from(),
                to: mv.to(),
            });
            cx.notify();
            return;
        }
        self.apply_move(mv, cx);
    }

    fn select_or_move(&mut self, cx: &mut Context<Self>) {
        let sq = self.cursor;
        self.square_clicked(sq, cx);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection::None;
        cx.notify();
    }

    fn choose_promotion(&mut self, kind: PieceKind, cx: &mut Context<Self>) {
        if let Selection::Promoting(pending) = self.selection {
            let mv = Move::Promotion {
                from: pending.from,
                to: pending.to,
                kind,
            };
            if self.position.legal_moves().contains(&mv) {
                self.apply_move(mv, cx);
            } else {
                self.selection = Selection::None;
                cx.notify();
            }
        }
    }

    fn export_pgn(&mut self, cx: &mut Context<Self>) {
        if self.records.is_empty() {
            self.status = "No moves to export yet".into();
            cx.notify();
            return;
        }
        let result = pichess::san::result_token(&self.position);
        let (white, black) = player_names(self.computer_plays());
        let date = today_string();
        let pgn = to_pgn(&self.records, &result, &white, &black, &date);
        let exports = self.store.exports_dir();
        let _ = std::fs::create_dir_all(&exports);
        let mut n = 1u32;
        let path = loop {
            let candidate = exports.join(format!("{date}-{n}.pgn"));
            if !candidate.exists() {
                break candidate;
            }
            n += 1;
        };
        match std::fs::write(&path, pgn) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(err) => self.status = format!("Export failed: {err}"),
        }
        cx.notify();
    }

    /// Move the cursor relative to what the eyes see: `df`/`dr` are screen
    /// directions (right/up positive). With the board flipped for Black the
    /// board axes are mirrored, so the deltas flip with it.
    fn move_cursor_screen(&mut self, df: i32, dr: i32, cx: &mut Context<Self>) {
        let (df, dr) = cursor_screen_delta(self.flipped, df, dr);
        let rank = (self.cursor.rank() as i32 + dr).clamp(0, 7);
        let file = (self.cursor.file() as i32 + df).clamp(0, 7);
        self.cursor = Square::new(rank as u8, file as u8);
        cx.notify();
    }
}

fn restore_game(saved: &SavedGame) -> Option<(Position, Vec<MoveRecord>)> {
    let mut position = Position::new();
    let mut records = Vec::new();
    for uci in &saved.moves {
        if uci.len() < 4 {
            return None;
        }
        let from = Square::parse(&uci[0..2])?;
        let to = Square::parse(&uci[2..4])?;
        let mv = if uci.len() > 4 {
            let kind = PieceKind::from_letter(uci.chars().nth(4)?)?;
            Move::Promotion { from, to, kind }
        } else {
            match position.piece_at(from) {
                Some(Piece {
                    kind: PieceKind::King,
                    ..
                }) if (to.0 as i32 - from.0 as i32).abs() == 2 => Move::Castle { from, to },
                _ => {
                    if position.piece_at(to).is_some()
                        || position.ep_square == Some(to)
                            && position.piece_at(from).map(|p| p.kind) == Some(PieceKind::Pawn)
                    {
                        // Quiet move or en passant: the engine decides via
                        // legality; try en passant first when it applies.
                        if position.ep_square == Some(to) {
                            Move::EnPassant { from, to }
                        } else {
                            Move::Quiet { from, to }
                        }
                    } else {
                        Move::Quiet { from, to }
                    }
                }
            }
        };
        if !position.legal_moves().contains(&mv) {
            return None;
        }
        push_san(&mut position, &mut records, mv);
    }
    Some((position, records))
}

fn today_string() -> String {
    // Local date in PGN form YYYY.MM.DD. `date` is present on every Linux
    // desktop; fall back to epoch seconds formatting if it fails.
    if let Ok(output) = std::process::Command::new("date").arg("+%Y.%m.%d").output() {
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !raw.is_empty() {
                return raw;
            }
        }
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}.{m:02}.{d:02}")
}

/// Squares in screen order. Unflipped is White's view: black's back rank on
/// top, `a1` in the bottom-left, files a→h left to right. Flipped mirrors
/// it for Black.
pub fn board_display_order(flipped: bool) -> Vec<Square> {
    let mut squares = Vec::with_capacity(64);
    for row in 0..8u8 {
        for col in 0..8u8 {
            squares.push(if flipped {
                Square::new(row, 7 - col)
            } else {
                Square::new(7 - row, col)
            });
        }
    }
    squares
}

fn apply_palette(
    palette: &pichess::theme::OmarchyPalette,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    Theme::change(
        if palette.dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        window,
        cx,
    );
    let theme = Theme::global_mut(cx);
    if let Some(bg) = hex_to_hsla(&palette.background) {
        theme.colors.background = bg;
    }
    if let Some(fg) = hex_to_hsla(&palette.foreground) {
        theme.colors.foreground = fg;
    }
    if let Some(accent) = hex_to_hsla(&palette.accent) {
        theme.colors.primary = accent;
        theme.colors.accent = accent;
    }
    if let Some(surface) = hex_to_hsla(&palette.surface) {
        theme.colors.secondary = surface;
    }
    if let Some(muted) = hex_to_hsla(&palette.muted) {
        theme.colors.muted_foreground = muted;
    }
    // The client-decorated title bar picks these up on Linux.
    let border = hex_to_hsla(&palette.background)
        .map(|bg| bg.blend(theme.colors.foreground.opacity(0.08)))
        .unwrap_or_else(|| theme.colors.title_bar_border);
    theme.colors.title_bar = hex_to_hsla(&palette.surface).unwrap_or(theme.colors.secondary);
    theme.colors.title_bar_border = border;
    theme.mono_font_family = "iA Writer Mono S".into();
    theme.mono_font_size = px(16.);
    Theme::sync_base(cx);
}

fn hex_to_hsla(value: &str) -> Option<Hsla> {
    let hex = value.trim().trim_start_matches('#');
    let expanded = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_string()
    };
    let n = u32::from_str_radix(&expanded, 16).ok()?;
    Some(rgb(n).into())
}

/// The theme colors a render pass needs, resolved once per frame.
#[derive(Clone, Copy)]
struct RenderColors {
    page: Hsla,
    ink: Hsla,
    accent: Hsla,
}

impl PichessApp {
    fn last_move(&self) -> Option<(Square, Square)> {
        self.position.last_move()
    }

    fn render_square(
        &self,
        sq: Square,
        cell: f32,
        colors: RenderColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page = colors.page;
        let ink = colors.ink;
        let accent = colors.accent;
        let selected =
            matches!(self.selection, Selection::Square(from) if from == sq) || self.cursor == sq;
        let is_target = match self.selection {
            Selection::Square(from) => self.legal_moves_from(from).iter().any(|mv| mv.to() == sq),
            _ => false,
        };
        let is_last = self
            .last_move()
            .map(|(from, to)| from == sq || to == sq)
            .unwrap_or(false);
        let light = sq.is_light();
        let base = if light {
            page.blend(ink.opacity(0.06))
        } else {
            page.blend(ink.opacity(0.22))
        };
        let in_check = self.position.in_check(self.position.turn)
            && self.position.king_square(self.position.turn) == Some(sq);
        let fill = if in_check {
            Hsla::from(rgb(0xef4444)).blend(page.opacity(0.72))
        } else if is_last {
            page.blend(accent.opacity(0.28))
        } else {
            base
        };
        let piece = self.position.piece_at(sq);
        let id = SharedString::from(format!("sq-{}", sq.name()));
        let xx = sq.file() as i32;
        let yy = sq.rank() as i32;
        let announce = self.square_announcement(sq);
        let dot_or_ring = is_target.then(|| {
            let occupied = self.position.piece_at(sq).is_some();
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(if occupied {
                    div()
                        .rounded_full()
                        .size(px(cell * 0.9))
                        .border_2()
                        .border_color(accent)
                } else {
                    div().rounded_full().size(px(cell * 0.18)).bg(accent)
                })
        });

        let mut square_div = div()
            .id(id)
            .relative()
            .w(px(cell))
            .h(px(cell))
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .cursor_pointer()
            .aria_label(announce.clone())
            .hover(|el| el.bg(page.blend(accent.opacity(0.14))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev, _window, cx| {
                    let sq = pichess::piece::Square::new(yy as u8, xx as u8);
                    this.square_clicked(sq, cx);
                }),
            );
        if selected {
            square_div = square_div.border_2().border_color(accent);
        }
        let mut square_div = square_div.child(match piece {
            Some(piece) => {
                // gpui renders SVGs as monochrome alpha masks tinted by the
                // element's text color, so one piece path + a per-color tint
                // is how the two sides are distinguished.
                let tint: Hsla = match piece.color {
                    Color::White => rgb(0xf5f2e9).into(),
                    Color::Black => rgb(0x1c1c1c).into(),
                };
                Icon::default()
                    .path(pichess::icons::ChessIcon::asset_path(Piece::new(
                        piece.kind,
                        Color::White,
                    )))
                    .text_color(tint)
                    .with_size(px(cell * 0.86))
                    .into_any_element()
            }
            None => div().into_any_element(),
        });
        if let Some(marker) = dot_or_ring {
            square_div = square_div.child(marker);
        }
        square_div.into_any_element()
    }

    fn square_announcement(&self, sq: Square) -> String {
        let pos_label = sq.name();
        match self.position.piece_at(sq) {
            Some(piece) => format!(
                "{} {} on {}{}",
                piece.color.label(),
                piece_name(piece.kind),
                pos_label,
                if self.cursor == sq { ", selected" } else { "" }
            ),
            None => format!(
                "empty {}{}",
                pos_label,
                if self.cursor == sq { ", selected" } else { "" }
            ),
        }
    }

    fn render_move_list(&self, ink: Hsla, muted: Hsla, accent: Hsla, scale: f32) -> AnyElement {
        let rows: Vec<AnyElement> = self
            .records
            .chunks(2)
            .enumerate()
            .map(|(i, pair)| {
                let white = pair[0].san.clone();
                let black = pair.get(1).map(|r| r.san.clone()).unwrap_or_default();
                let is_last = i == self.records.chunks(2).count() - 1;
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("{}.", i + 1))
                            .text_size(px(13. * scale))
                            .text_color(muted),
                    )
                    .child(Label::new(white).text_size(px(14. * scale)).text_color(
                        if is_last && pair.len() == 1 {
                            accent
                        } else {
                            ink
                        },
                    ))
                    .child(Label::new(black).text_size(px(14. * scale)).text_color(
                        if is_last && pair.len() == 2 {
                            accent
                        } else {
                            ink
                        },
                    ))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("moves")
            .w(px(190. * scale))
            .flex_1()
            .gap_1()
            .p_3()
            .overflow_scroll()
            .children(rows)
            .when(self.records.is_empty(), |list| {
                list.child(
                    Label::new("No moves yet")
                        .text_size(px(13. * scale))
                        .text_color(muted),
                )
            })
            .into_any_element()
    }
}

fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::King => "king",
        PieceKind::Queen => "queen",
        PieceKind::Rook => "rook",
        PieceKind::Bishop => "bishop",
        PieceKind::Knight => "knight",
        PieceKind::Pawn => "pawn",
    }
}

impl Render for PichessApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = hex_to_hsla(&self.palette.background).unwrap_or_else(|| cx.theme().background);
        let ink = hex_to_hsla(&self.palette.foreground).unwrap_or_else(|| cx.theme().foreground);
        let muted = hex_to_hsla(&self.palette.muted).unwrap_or_else(|| cx.theme().muted_foreground);
        let surface = hex_to_hsla(&self.palette.surface).unwrap_or_else(|| cx.theme().secondary);
        let accent = hex_to_hsla(&self.palette.accent).unwrap_or_else(|| cx.theme().accent);
        let scale = self.text_scale;
        // Fit the board to the window: share the viewport height with the
        // header/status bars and leave room for the move-list sidebar.
        let viewport = window.viewport_size();
        let chrome_h = px(96. * scale); // header + status + paddings
        let sidebar_w = px(200. * scale);
        let cell_from_height = (viewport.height - chrome_h - px(12. * scale)) / 8.5;
        let cell_from_width = (viewport.width - sidebar_w - px(60. * scale)) / 8.5;
        let cell = cell_from_height
            .min(cell_from_width)
            .min(px(74. * scale))
            .max(px(34.));
        let order = board_display_order(self.flipped);
        let status = self.status.clone();
        let level = self.level;
        let mode = self.mode;
        let thinking = self.thinking;
        let ended = self.game_ended();
        let selection = self.selection;
        window.set_window_title(&format!("Pichess — {status}"));

        let header = h_flex()
            .id("hud")
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .flex_wrap()
            .bg(surface)
            .border_b_1()
            .border_color(page.blend(ink.opacity(0.08)))
            .child(
                h_flex()
                    .gap_0p5()
                    .p_0p5()
                    .rounded_md()
                    .bg(page.blend(ink.opacity(0.06)))
                    .child(segment_button(
                        "mode-2p",
                        "Two players",
                        "Hotseat: both sides by hand",
                        mode == Mode::TwoPlayers,
                        Some("ctrl-shift-2"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::TwoPlayers, cx)),
                    ))
                    .child(segment_button(
                        "mode-w",
                        "You: White",
                        "You play White, the engine answers",
                        mode == Mode::HumanWhite,
                        Some("ctrl-shift-1"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::HumanWhite, cx)),
                    ))
                    .child(segment_button(
                        "mode-b",
                        "You: Black",
                        "You play Black, the engine opens",
                        mode == Mode::HumanBlack,
                        Some("ctrl-shift-3"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::HumanBlack, cx)),
                    )),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .p_0p5()
                    .rounded_md()
                    .bg(page.blend(ink.opacity(0.06)))
                    .child(segment_button(
                        "lv1",
                        "Easy",
                        "Level 1 — quick and fallible",
                        level == Level::Easy,
                        Some("1"),
                        cx.listener(|this, _, _, cx| this.set_level(Level::Easy, cx)),
                    ))
                    .child(segment_button(
                        "lv2",
                        "Medium",
                        "Level 2 — depth 3, half a second",
                        level == Level::Medium,
                        Some("2"),
                        cx.listener(|this, _, _, cx| this.set_level(Level::Medium, cx)),
                    ))
                    .child(segment_button(
                        "lv3",
                        "Hard",
                        "Level 3 — depth 5, 1.5 seconds",
                        level == Level::Hard,
                        Some("3"),
                        cx.listener(|this, _, _, cx| this.set_level(Level::Hard, cx)),
                    )),
            )
            .child(div().flex_1())
            .child(
                Button::new("new")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip_with_action("New game", &NewGame, Some("pichess"))
                    .on_click(cx.listener(|this, _, _, cx| this.new_game(cx))),
            )
            .child(
                Button::new("undo")
                    .ghost()
                    .small()
                    .icon(IconName::Undo2)
                    .tooltip_with_action("Undo (engine reply too)", &Undo, Some("pichess"))
                    .disabled(thinking)
                    .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
                Button::new("resign")
                    .ghost()
                    .small()
                    .icon(IconName::TriangleAlert)
                    .tooltip_with_action("Resign", &Resign, Some("pichess"))
                    .disabled(ended)
                    .on_click(cx.listener(|this, _, _, cx| this.resign(cx))),
            )
            .child(
                Button::new("flip")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .tooltip_with_action("Flip board", &FlipBoard, Some("pichess"))
                    .on_click(cx.listener(|this, _, _, cx| this.flip(cx))),
            )
            .child(
                Button::new("pgn")
                    .ghost()
                    .small()
                    .icon(IconName::FileText)
                    .tooltip_with_action("Export game as PGN", &ExportPgn, Some("pichess"))
                    .on_click(cx.listener(|this, _, _, cx| this.export_pgn(cx))),
            )
            .child(
                Button::new("hint")
                    .ghost()
                    .small()
                    .icon(IconName::Star)
                    .tooltip_with_action("Engine plays this move", &EngineHint, Some("pichess"))
                    .disabled(thinking || ended)
                    .on_click(cx.listener(|this, _, _, cx| this.engine_hint(cx))),
            )
            .when(thinking, |bar| {
                bar.child(
                    h_flex()
                        .gap_1p5()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(accent.opacity(0.14))
                        .child(Spinner::new().small().color(accent))
                        .child(
                            Label::new("thinking")
                                .text_size(px(12. * scale))
                                .text_color(accent),
                        ),
                )
            });

        let squares = order
            .iter()
            .enumerate()
            .map(|(i, sq)| {
                // Display coordinates: row 0 is the top row on screen.
                let display_row = i / 8;
                let display_col = i % 8;
                let square = self.render_square(
                    *sq,
                    f32::from(cell),
                    RenderColors { page, ink, accent },
                    cx,
                );
                // File letters along the bottom display row; rank numbers
                // along the left display column.
                let is_file_label = display_row == 7;
                let is_rank_label = display_col == 0;
                let mut stack = v_flex().relative();
                if is_rank_label {
                    stack = stack.child(
                        div().absolute().left(px(2.)).top(px(1.)).child(
                            Label::new(((b'1' + sq.rank()) as char).to_string())
                                .text_size(px(8. * scale))
                                .text_color(muted.opacity(0.65)),
                        ),
                    );
                }
                stack = stack.child(square);
                if is_file_label {
                    stack = stack.child(
                        div().absolute().right(px(2.)).bottom(px(0.)).child(
                            Label::new(((b'a' + sq.file()) as char).to_string())
                                .text_size(px(8. * scale))
                                .text_color(muted.opacity(0.65)),
                        ),
                    );
                }
                stack.into_any_element()
            })
            .collect::<Vec<_>>();

        let board = div()
            .id("board")
            .grid()
            .grid_cols(8)
            .gap(px(2. * scale))
            .p(px(3. * scale))
            .rounded_lg()
            .bg(surface)
            .border_1()
            .border_color(page.blend(ink.opacity(0.08)))
            .children(squares)
            .when_some(selection_promotion(selection), |board, pending| {
                board.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(page.opacity(0.72))
                        .rounded_lg()
                        .child(
                            v_flex()
                                .gap_2()
                                .p_4()
                                .rounded_lg()
                                .bg(surface)
                                .border_1()
                                .border_color(accent.opacity(0.4))
                                .child(
                                    Label::new("Promote to")
                                        .text_size(px(18. * scale))
                                        .text_color(ink),
                                )
                                .child({
                                    let mover_color = self
                                        .position
                                        .piece_at(pending.from)
                                        .map(|piece| piece.color)
                                        .unwrap_or(Color::White);
                                    h_flex().gap_2().children([
                                        promo_button(PieceKind::Queen, mover_color, "q"),
                                        promo_button(PieceKind::Rook, mover_color, "r"),
                                        promo_button(PieceKind::Bishop, mover_color, "b"),
                                        promo_button(PieceKind::Knight, mover_color, "n"),
                                    ])
                                }),
                        ),
                )
            });

        let overlay = self.show_help.then(|| {
            div()
                .id("help")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(page.opacity(0.72))
                .aria_label("Keyboard shortcuts")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.show_help = false;
                        cx.notify();
                    }),
                )
                .child(
                    v_flex()
                        .id("help-card")
                        .w(px(430. * scale))
                        .p_5()
                        .gap_2()
                        .rounded_lg()
                        .bg(surface)
                        .border_1()
                        .border_color(accent.opacity(0.4))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Icon::new(pichess::icons::HudIcon::Info)
                                        .small()
                                        .text_color(accent),
                                )
                                .child(
                                    Label::new("Keys")
                                        .text_size(px(20. * scale))
                                        .text_color(ink),
                                ),
                        )
                        .children(
                            HELP_ROWS
                                .iter()
                                .map(|row| {
                                    Label::new(*row)
                                        .text_sm()
                                        .text_color(muted)
                                        .into_any_element()
                                })
                                .collect::<Vec<_>>(),
                        ),
                )
        });

        v_flex()
            .id("pichess")
            .size_full()
            .relative()
            .bg(page)
            .font_family("iA Writer Mono S")
            .text_size(px(16. * scale))
            .key_context("pichess")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &SelectOrMove, _, cx| this.select_or_move(cx)))
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.cancel(cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &NewGame, _, cx| this.new_game(cx)))
            .on_action(cx.listener(|this, _: &Resign, _, cx| this.resign(cx)))
            .on_action(cx.listener(|this, _: &ExportPgn, _, cx| this.export_pgn(cx)))
            .on_action(cx.listener(|this, _: &EngineHint, _, cx| this.engine_hint(cx)))
            .on_action(cx.listener(|this, _: &FlipBoard, _, cx| this.flip(cx)))
            .on_action(cx.listener(|this, _: &Level1, _, cx| this.set_level(Level::Easy, cx)))
            .on_action(cx.listener(|this, _: &Level2, _, cx| this.set_level(Level::Medium, cx)))
            .on_action(cx.listener(|this, _: &Level3, _, cx| this.set_level(Level::Hard, cx)))
            .on_action(cx.listener(|this, _: &ModeTwoPlayers, _, cx| {
                this.set_mode(Mode::TwoPlayers, cx);
            }))
            .on_action(cx.listener(|this, _: &ModeHumanWhite, _, cx| {
                this.set_mode(Mode::HumanWhite, cx);
            }))
            .on_action(cx.listener(|this, _: &ModeHumanBlack, _, cx| {
                this.set_mode(Mode::HumanBlack, cx);
            }))
            .on_action(cx.listener(|this, _: &PromoteQueen, _, cx| {
                this.choose_promotion(PieceKind::Queen, cx);
            }))
            .on_action(cx.listener(|this, _: &PromoteRook, _, cx| {
                this.choose_promotion(PieceKind::Rook, cx);
            }))
            .on_action(cx.listener(|this, _: &PromoteBishop, _, cx| {
                this.choose_promotion(PieceKind::Bishop, cx);
            }))
            .on_action(cx.listener(|this, _: &PromoteKnight, _, cx| {
                this.choose_promotion(PieceKind::Knight, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                this.move_cursor_screen(-1, 0, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| {
                this.move_cursor_screen(1, 0, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_cursor_screen(0, 1, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_cursor_screen(0, -1, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleHelp, _, cx| {
                this.show_help = !this.show_help;
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| {
                window.toggle_fullscreen();
            }))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .child(header)
            .child(
                h_flex()
                    .id("main")
                    .flex_1()
                    .items_start()
                    .justify_center()
                    .gap_4()
                    .p_4()
                    .overflow_scroll()
                    .child(board)
                    .child(self.render_move_list(ink, muted, accent, scale)),
            )
            .child(
                h_flex()
                    .id("status")
                    .w_full()
                    .px_3()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .bg(surface)
                    .border_t_1()
                    .border_color(page.blend(ink.opacity(0.08)))
                    .child(Label::new(status).text_sm().text_color(if ended {
                        accent
                    } else {
                        ink
                    }))
                    .child(
                        Label::new(KEY_HINTS)
                            .text_xs()
                            .text_color(muted.opacity(0.85)),
                    ),
            )
            .children(overlay)
    }
}

const HELP_ROWS: [&str; 11] = [
    "click / space   pick up & drop",
    "arrows / hjkl  cursor",
    "u              undo",
    "n              new game",
    "r              resign",
    "e              export PGN",
    "a              engine plays this move",
    "v              flip board",
    "1 2 3          easy / medium / hard",
    "F11 / Super+F  fullscreen",
    "Ctrl+Q         quit",
];

fn selection_promotion(selection: Selection) -> Option<PendingPromotion> {
    match selection {
        Selection::Promoting(pending) => Some(pending),
        _ => None,
    }
}

const KEY_HINTS: &str = "click move · u undo · n new · r resign · e pgn · a hint · v flip · ? help";

/// Screen-direction delta to board delta: the flipped view mirrors both
/// board axes, so a screen step is the opposite board step.
fn cursor_screen_delta(flipped: bool, df: i32, dr: i32) -> (i32, i32) {
    if flipped {
        (-df, -dr)
    } else {
        (df, dr)
    }
}

fn promo_button(kind: PieceKind, color: Color, key: &'static str) -> Button {
    let icon_path = pichess::icons::ChessIcon::asset_path(Piece::new(kind, color));
    let label = SharedString::from(match kind {
        PieceKind::Queen => "Queen",
        PieceKind::Rook => "Rook",
        PieceKind::Bishop => "Bishop",
        PieceKind::Knight => "Knight",
        _ => "Pawn",
    });
    Button::new(SharedString::from(format!("promo-{}", kind.letter())))
        .ghost()
        .label(label.clone())
        .accessibility_label(format!("{label} (press {key})"))
        .child(Icon::default().path(icon_path).with_size(px(34.)))
}

/// One option inside a segmented control: filled accent when chosen, quiet
/// ghost otherwise.
fn segment_button(
    id: &'static str,
    label: &'static str,
    tip: &'static str,
    active: bool,
    key: Option<&'static str>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let mut button = Button::new(id)
        .small()
        .label(label)
        .tooltip(tip)
        .rounded(ButtonRounded::Small)
        .on_click(on_click);
    if active {
        button = button.primary();
    } else {
        button = button.ghost();
    }
    if let Some(key) = key {
        button = button.accessibility_label(format!("{label} ({key})"));
    }
    button
}

#[cfg(test)]
mod tests {
    use super::{board_display_order, restore_game, today_string};
    use pichess::stats::SavedGame;

    #[test]
    fn hex_to_hsla_parses_three_and_six_digit_colors() {
        assert!(super::hex_to_hsla("#101010").is_some());
        assert!(super::hex_to_hsla("#fff").is_some());
        assert!(super::hex_to_hsla("nope").is_none());
    }

    #[test]
    fn today_string_is_pgn_shaped() {
        let today = today_string();
        assert_eq!(today.len(), 10);
        assert_eq!(today.as_bytes()[4], b'.');
        assert_eq!(today.as_bytes()[7], b'.');
    }

    #[test]
    fn board_order_flips() {
        // White's view: black back rank on top, a1 in the bottom-left.
        let normal = board_display_order(false);
        assert_eq!(normal[0], pichess::piece::Square::new(7, 0), "a8 top-left");
        assert_eq!(normal[7], pichess::piece::Square::new(7, 7), "h8 top-right");
        assert_eq!(
            normal[56],
            pichess::piece::Square::new(0, 0),
            "a1 bottom-left"
        );
        assert_eq!(
            normal[63],
            pichess::piece::Square::new(0, 7),
            "h1 bottom-right"
        );
        // Black's view mirrors both axes.
        let flipped = board_display_order(true);
        assert_eq!(flipped[0], pichess::piece::Square::new(0, 7), "h1 top-left");
        assert_eq!(
            flipped[63],
            pichess::piece::Square::new(7, 0),
            "a8 bottom-right"
        );
        assert_eq!(normal.len(), flipped.len());
    }

    #[test]
    fn cursor_deltas_flip_with_the_board() {
        use super::cursor_screen_delta;
        // dr = +1 means screen-up. Unflipped (White view, rank 8 on top):
        // screen right = board +file, screen up = board +rank.
        assert_eq!(cursor_screen_delta(false, 1, 0), (1, 0));
        assert_eq!(cursor_screen_delta(false, 0, 1), (0, 1));
        // Flipped (Black view, rank 1 on top, files h→a): both axes mirror.
        assert_eq!(cursor_screen_delta(true, 1, 0), (-1, 0));
        assert_eq!(cursor_screen_delta(true, 0, 1), (0, -1));
        assert_eq!(cursor_screen_delta(true, -1, 0), (1, 0));
        assert_eq!(cursor_screen_delta(true, 0, -1), (0, 1));
    }

    #[test]
    fn restore_replays_a_short_game() {
        let saved = SavedGame {
            moves: vec!["e2e4".into(), "e7e5".into()],
            mode: "two-players".into(),
            level: "easy".into(),
            flipped: false,
        };
        let (position, records) = restore_game(&saved).expect("game restores");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].san, "e4");
        assert!(position
            .piece_at(pichess::piece::Square::parse("e4").unwrap())
            .is_some());
    }

    #[test]
    fn restore_rejects_illegal_moves() {
        let saved = SavedGame {
            moves: vec!["e2e5".into()],
            mode: "two-players".into(),
            level: "easy".into(),
            flipped: false,
        };
        assert!(restore_game(&saved).is_none());
    }

    #[test]
    fn restore_replays_castling() {
        let saved = SavedGame {
            moves: vec![
                "e2e4".into(),
                "e7e5".into(),
                "g1f3".into(),
                "b8c6".into(),
                "f1c4".into(),
                "g8f6".into(),
                "e1g1".into(),
            ],
            mode: "two-players".into(),
            level: "easy".into(),
            flipped: false,
        };
        let (position, records) = restore_game(&saved).expect("castle game restores");
        assert_eq!(records.len(), 7);
        assert_eq!(records[6].san, "O-O");
        let g1 = pichess::piece::Square::parse("g1").unwrap();
        assert_eq!(
            position.piece_at(g1).map(|p| p.kind),
            Some(pichess::piece::PieceKind::King)
        );
    }
}
