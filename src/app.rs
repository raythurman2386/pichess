//! GPUI Kit chess window. Rules, SAN, and the engine live in the lib.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::label::Label;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Icon, Root, Sizable as _, Theme, ThemeMode,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pichess::piece::{Color, Piece, PieceKind, Square};
use pichess::position::{Move, Position};
use pichess::puzzle::{
    self, daily_index, grade, prepare, rush_floor, rush_step, PickFilter, Puzzle, SessionKind,
    Theme as PuzzleTheme, Verdict,
};
use pichess::san::{ending_label, format_san, player_names, push_san, to_pgn, MoveRecord};
use pichess::search::{search_with, Level, ThreadRng};
use pichess::sound::{self, Outcome, Sound};
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
/// moment to register on the board. Puzzle setup moves use the same beat.
const ENGINE_MOVE_DELAY: Duration = Duration::from_millis(650);

/// The game that was on the board when a puzzle session started.
struct ParkedGame {
    position: Position,
    records: Vec<MoveRecord>,
    flipped: bool,
    cursor: Square,
    resigned: Option<Color>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PuzzlePhase {
    /// Opponent's move is about to play itself.
    Busy,
    YourTurn,
    Solved,
    Failed,
}

struct PuzzlePlay {
    parked: ParkedGame,
    puzzle: Puzzle,
    line: Vec<Move>,
    /// Index of the next line move. Even indexes are the opponent's.
    next: usize,
    phase: PuzzlePhase,
    kind: SessionKind,
    theme: PuzzleTheme,
    streak: u32,
    /// The current puzzle has already been scored.
    rated: bool,
    /// A retry rewound this attempt, so finishing the replay must not score.
    locked: bool,
    task: Option<Task<()>>,
    /// Rush clock. Dropped when the run ends or the session changes.
    clock: Option<Task<()>>,
    /// Delayed advance to the next rush puzzle after a solve.
    advance: Option<Task<()>>,
    /// Bumped to cancel a pending rush advance.
    epoch: u64,
    rush_score: u32,
    rush_strikes: u8,
    rush_over: bool,
    rush_seen: Vec<String>,
    deadline: Option<Instant>,
    /// Last rush-clock second that already chimed, so the tick plays once.
    rush_chime: Option<u64>,
}

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
    /// Shipped tactics. Empty only if the pack failed to parse.
    pack: Vec<Puzzle>,
    /// Set while a puzzle session is on top of the parked game.
    puzzle: Option<PuzzlePlay>,
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
        let mut settings = store.load_settings();
        let mode = Mode::parse(&settings.mode);
        let level = Level::parse(&settings.level);
        let flipped = settings.flipped;
        // Resume the saved game when it parses; corrupt saves reset silently.
        let resumed = store.load_game().and_then(|saved| restore_game(&saved));
        let (position, records) = resumed.unwrap_or_else(|| (Position::new(), Vec::new()));

        apply_palette(&palette, Some(window), cx);
        window.set_window_title("Pichess");

        let (appearance_sub, poll_task) = Self::start_theme_poll(window, cx);

        if settings.puzzle_rating == 0 {
            settings.puzzle_rating = 1200;
        }
        if settings.puzzle_seed == 0 {
            settings.puzzle_seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                | 1;
        }

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
            pack: puzzle::starter_pack(),
            puzzle: None,
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
        self.puzzle.is_none() && self.computer_plays() == Some(self.position.turn) && !self.thinking
    }

    fn game_ended(&self) -> bool {
        self.resigned.is_some() || self.position.game_over().is_some()
    }

    fn human_controls(&self, color: Color) -> bool {
        if let Some(play) = &self.puzzle {
            return !play.rush_over
                && play.phase == PuzzlePhase::YourTurn
                && color == self.position.turn;
        }
        self.computer_plays() != Some(color)
    }

    fn refresh_status(&mut self) {
        if self.puzzle.is_some() {
            self.refresh_puzzle_status();
            return;
        }
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
        if self.puzzle.is_some() {
            return;
        }
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
        self.settings.flipped = self
            .puzzle
            .as_ref()
            .map(|play| play.parked.flipped)
            .unwrap_or(self.flipped);
        self.store.save_settings(&self.settings);
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        if self.puzzle.is_some() {
            self.next_puzzle(cx);
            return;
        }
        sound::play(Sound::Click);
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
        let leaving = self.puzzle.is_some();
        if leaving {
            self.leave_puzzles(cx);
        }
        if self.mode == mode {
            if leaving {
                sound::play(Sound::Click);
            }
            cx.notify();
            return;
        }
        self.cancel_engine();
        let side_changed = matches!(
            (self.mode, mode),
            (Mode::HumanWhite, Mode::HumanBlack) | (Mode::HumanBlack, Mode::HumanWhite)
        );
        self.mode = mode;
        sound::play(Sound::Toggle);
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
        sound::play(Sound::Toggle);
        self.save_settings();
        if self.puzzle.is_some() {
            cx.notify();
            return;
        }
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
        sound::play(Sound::Click);
        if self.puzzle.is_none() {
            self.save_settings();
        }
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if self.puzzle.is_some() {
            sound::play(Sound::Back);
            self.retry_puzzle(cx);
            return;
        }
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
        if undone > 0 {
            sound::play(Sound::Back);
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
        if self.puzzle.is_some() {
            self.give_up(cx);
            return;
        }
        if self.game_ended() {
            return;
        }
        sound::play(Sound::Down);
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
        if self.puzzle.is_some() {
            self.apply_puzzle_move(mv, cx);
            return;
        }
        let captures = mv.is_capture(&self.position.board);
        push_san(&mut self.position, &mut self.records, mv);
        self.play_landed(mv, captures, self.outcome_now());
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

    fn play_landed(&self, mv: Move, captures: bool, outcome: Outcome) {
        let (body, accent) = sound::cue_for_move(mv, captures, outcome);
        sound::play_landed(body, accent);
    }

    fn outcome_now(&self) -> Outcome {
        match self.position.game_over() {
            Some(pichess::position::GameOver::Checkmate(winner)) => Outcome::Mate {
                winner_is_computer: self.computer_plays() == Some(winner),
            },
            Some(_) => Outcome::Draw,
            None => Outcome::Play {
                check: self.position.in_check_self(),
            },
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
        if self.puzzle.is_some() {
            return;
        }
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
                    sound::play(Sound::Back);
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
            if !matches!(self.selection, Selection::None) {
                sound::play(Sound::Back);
            }
            self.selection = Selection::None;
            cx.notify();
            return;
        };
        let movable = piece.color == self.position.turn
            && self.human_controls(piece.color)
            && !self.game_ended()
            && !self.thinking
            && !self.legal_moves_from(sq).is_empty();
        if !movable {
            let complain =
                self.human_controls(self.position.turn) && !self.game_ended() && !self.thinking;
            self.selection = Selection::None;
            if complain {
                sound::play(Sound::Error);
            }
            cx.notify();
            return;
        }
        self.selection = Selection::Square(sq);
        sound::play(Sound::Select);
        cx.notify();
    }

    fn finish_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        // Promotions route through the picker.
        if mv.promotion().is_some() && !matches!(self.selection, Selection::Promoting(_)) {
            self.selection = Selection::Promoting(PendingPromotion {
                from: mv.from(),
                to: mv.to(),
            });
            sound::play(Sound::Open);
            cx.notify();
            return;
        }
        self.apply_move(mv, cx);
    }

    fn select_or_move(&mut self, cx: &mut Context<Self>) {
        let sq = self.cursor;
        self.square_clicked(sq, cx);
    }

    fn set_help(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.show_help == open {
            return;
        }
        self.show_help = open;
        sound::play(if open { Sound::Open } else { Sound::Back });
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.selection, Selection::None) {
            sound::play(Sound::Back);
        }
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
        if self.puzzle.is_some() {
            self.status = "Leave puzzles before exporting the game".into();
            sound::play(Sound::Error);
            cx.notify();
            return;
        }
        if self.records.is_empty() {
            self.status = "No moves to export yet".into();
            sound::play(Sound::Error);
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
            Ok(()) => {
                self.status = format!("Exported {}", path.display());
                sound::play(Sound::Confirm);
            }
            Err(err) => {
                self.status = format!("Export failed: {err}");
                sound::play(Sound::Error);
            }
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

    // ── puzzles ─────────────────────────────────────────────────────────

    fn enter_puzzles(&mut self, cx: &mut Context<Self>) {
        if self.puzzle.is_some() {
            return;
        }
        if self.pack.is_empty() {
            self.status = "No puzzles in the pack".into();
            sound::play(Sound::Error);
            cx.notify();
            return;
        }
        self.cancel_engine();
        let parked = ParkedGame {
            position: self.position.clone(),
            records: self.records.clone(),
            flipped: self.flipped,
            cursor: self.cursor,
            resigned: self.resigned,
        };
        let Some(puzzle) = self.choose_puzzle() else {
            self.status = "No puzzles in the pack".into();
            sound::play(Sound::Error);
            cx.notify();
            return;
        };
        self.puzzle = Some(PuzzlePlay {
            parked,
            puzzle: puzzle.clone(),
            line: Vec::new(),
            next: 0,
            phase: PuzzlePhase::Busy,
            kind: SessionKind::Rated,
            theme: PuzzleTheme::Fork,
            streak: 0,
            rated: false,
            locked: false,
            task: None,
            clock: None,
            advance: None,
            epoch: 0,
            rush_score: 0,
            rush_strikes: 0,
            rush_over: false,
            rush_seen: Vec::new(),
            deadline: None,
            rush_chime: None,
        });
        self.present_puzzle(puzzle, cx);
    }

    fn leave_puzzles(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let Some(play) = self.puzzle.take() else {
            return;
        };
        self.position = play.parked.position;
        self.records = play.parked.records;
        self.flipped = play.parked.flipped;
        self.cursor = play.parked.cursor;
        self.resigned = play.parked.resigned;
        self.selection = Selection::None;
        self.thinking = false;
        self.refresh_status();
        if self.engine_to_move() && !self.game_ended() {
            self.spawn_engine_move(cx);
        }
        cx.notify();
    }

    fn choose_puzzle(&mut self) -> Option<Puzzle> {
        if self.pack.is_empty() {
            return None;
        }
        let kind = self
            .puzzle
            .as_ref()
            .map(|play| play.kind)
            .unwrap_or(SessionKind::Rated);
        if kind == SessionKind::Daily {
            let index = daily_index(&self.pack, puzzle_day_key())?;
            return Some(self.pack[index].clone());
        }
        let seed = self.settings.puzzle_seed;
        let rating = self.settings.puzzle_rating;
        let theme = self.puzzle.as_ref().map(|play| play.theme);
        let (filter, rush_seen) = match kind {
            SessionKind::Rush => {
                let solved = self
                    .puzzle
                    .as_ref()
                    .map(|play| play.rush_score)
                    .unwrap_or(0);
                (
                    PickFilter {
                        theme: None,
                        floor: Some(rush_floor(rating, solved)),
                    },
                    true,
                )
            }
            SessionKind::Theme => (
                PickFilter {
                    theme: theme.map(PuzzleTheme::tag),
                    floor: None,
                },
                false,
            ),
            SessionKind::Rated | SessionKind::Daily => (PickFilter::default(), false),
        };
        let seen_owned: Vec<String> = if rush_seen {
            self.puzzle
                .as_ref()
                .map(|play| play.rush_seen.clone())
                .unwrap_or_default()
        } else {
            self.settings.puzzle_seen.clone()
        };
        let seen: std::collections::HashSet<&str> = seen_owned.iter().map(String::as_str).collect();
        let index = match puzzle::pick_index(&self.pack, rating, &seen, seed, filter) {
            Some(index) => index,
            None if rush_seen => {
                if let Some(play) = self.puzzle.as_mut() {
                    play.rush_seen.clear();
                }
                let seen = std::collections::HashSet::new();
                puzzle::pick_index(&self.pack, rating, &seen, seed, filter)?
            }
            None if filter.theme.is_some() => {
                let tag = filter.theme.unwrap_or("");
                self.forget_theme_seen(tag);
                let seen: std::collections::HashSet<&str> = self
                    .settings
                    .puzzle_seen
                    .iter()
                    .map(String::as_str)
                    .collect();
                puzzle::pick_index(&self.pack, rating, &seen, seed, filter)?
            }
            None => {
                self.settings.puzzle_seen.clear();
                let seen = std::collections::HashSet::new();
                puzzle::pick_index(&self.pack, rating, &seen, seed, filter).unwrap_or(0)
            }
        };
        self.settings.puzzle_seed = seed.wrapping_add(1);
        Some(self.pack[index].clone())
    }

    fn forget_theme_seen(&mut self, tag: &str) {
        let drop: std::collections::HashSet<&str> = self
            .pack
            .iter()
            .filter(|puzzle| puzzle.themes.iter().any(|theme| theme == tag))
            .map(|puzzle| puzzle.id.as_str())
            .collect();
        self.settings
            .puzzle_seen
            .retain(|id| !drop.contains(id.as_str()));
    }

    fn present_puzzle(&mut self, puzzle: Puzzle, cx: &mut Context<Self>) {
        let Some(prepared) = prepare(&puzzle) else {
            self.status = format!("Puzzle {} could not be played", puzzle.id);
            cx.notify();
            return;
        };
        let streak = self.puzzle.as_ref().map(|play| play.streak).unwrap_or(0);
        let puzzle_id = puzzle.id.clone();
        if let Some(play) = self.puzzle.as_mut() {
            play.puzzle = puzzle;
            play.line = prepared.line;
            play.next = 0;
            play.phase = PuzzlePhase::Busy;
            play.streak = streak;
            play.task = None;
            if play.kind == SessionKind::Rush && !play.rush_seen.iter().any(|id| id == &puzzle_id) {
                play.rush_seen.push(puzzle_id);
            }
        }
        self.position = prepared.start;
        self.records.clear();
        self.selection = Selection::None;
        self.resigned = None;
        self.flipped = prepared.solver == Color::Black;
        self.cursor = Square::new(3, 3);
        self.generation += 1;
        self.refresh_puzzle_status();
        self.schedule_reply(cx);
    }

    fn next_puzzle(&mut self, cx: &mut Context<Self>) {
        if self.puzzle.is_none() {
            return;
        }
        if self.puzzle.as_ref().is_some_and(|play| play.rush_over) {
            sound::play(Sound::Click);
            self.restart_rush(cx);
            return;
        }
        if self
            .puzzle
            .as_ref()
            .is_some_and(|play| play.kind == SessionKind::Daily)
        {
            self.status = "That's the puzzle for today".into();
            sound::play(Sound::Error);
            cx.notify();
            return;
        }
        let skipping = self.puzzle.as_ref().is_some_and(|play| {
            play.kind == SessionKind::Rush
                && !play.rush_over
                && !play.rated
                && play.phase == PuzzlePhase::YourTurn
        });
        if self.record_rush_skip(cx) {
            return;
        }
        sound::play(if skipping { Sound::Error } else { Sound::Click });
        self.load_another(cx);
    }

    fn load_another(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if let Some(play) = self.puzzle.as_mut() {
            play.rated = false;
            play.locked = false;
            play.epoch = play.epoch.wrapping_add(1);
        }
        let Some(puzzle) = self.choose_puzzle() else {
            return;
        };
        self.present_puzzle(puzzle, cx);
    }

    /// A rush skip on your turn counts as a miss. Returns true when that
    /// miss ended the run.
    fn record_rush_skip(&mut self, cx: &mut Context<Self>) -> bool {
        let skipping = self.puzzle.as_ref().is_some_and(|play| {
            play.kind == SessionKind::Rush
                && !play.rush_over
                && !play.rated
                && play.phase == PuzzlePhase::YourTurn
        });
        if !skipping {
            return false;
        }
        let (score, strikes) = self
            .puzzle
            .as_ref()
            .map(|play| (play.rush_score, play.rush_strikes))
            .unwrap_or((0, 0));
        let (score, strikes, over) = rush_step(score, strikes, false);
        if let Some(play) = self.puzzle.as_mut() {
            play.rush_score = score;
            play.rush_strikes = strikes;
            play.rated = true;
        }
        if over {
            self.end_rush(cx);
        }
        over
    }

    fn retry_puzzle(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.puzzle.as_ref().map(|play| {
            (
                play.puzzle.clone(),
                play.kind,
                play.rated,
                play.phase,
                play.rush_over,
                play.rush_score,
                play.rush_strikes,
            )
        }) else {
            return;
        };
        let (puzzle, kind, rated, phase, rush_over, score, strikes) = snapshot;
        let unfinished =
            !rated && !rush_over && !matches!(phase, PuzzlePhase::Solved | PuzzlePhase::Failed);
        if unfinished {
            if let Some(play) = self.puzzle.as_mut() {
                play.locked = true;
            }
            if kind == SessionKind::Rush {
                let (score, strikes, over) = rush_step(score, strikes, false);
                if let Some(play) = self.puzzle.as_mut() {
                    play.rush_score = score;
                    play.rush_strikes = strikes;
                    play.rated = true;
                }
                if over {
                    self.end_rush(cx);
                    return;
                }
            }
        }
        self.generation += 1;
        self.present_puzzle(puzzle, cx);
    }

    fn schedule_reply(&mut self, cx: &mut Context<Self>) {
        let (next, len) = {
            let Some(play) = &self.puzzle else {
                return;
            };
            (play.next, play.line.len())
        };
        if next >= len || next % 2 == 1 {
            if let Some(play) = self.puzzle.as_mut() {
                play.phase = PuzzlePhase::YourTurn;
            }
            self.refresh_puzzle_status();
            cx.notify();
            return;
        }
        if let Some(play) = self.puzzle.as_mut() {
            play.phase = PuzzlePhase::Busy;
        }
        self.generation += 1;
        let generation = self.generation;
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENGINE_MOVE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.play_scheduled(cx);
            });
        });
        if let Some(play) = self.puzzle.as_mut() {
            play.task = Some(task);
        }
        self.refresh_puzzle_status();
        cx.notify();
    }

    fn play_scheduled(&mut self, cx: &mut Context<Self>) {
        let mv = {
            let Some(play) = &self.puzzle else {
                return;
            };
            if play.next >= play.line.len() {
                return;
            }
            play.line[play.next]
        };
        let captures = mv.is_capture(&self.position.board);
        push_san(&mut self.position, &mut self.records, mv);
        self.play_landed(
            mv,
            captures,
            Outcome::Play {
                check: self.position.in_check_self(),
            },
        );
        if let Some(play) = self.puzzle.as_mut() {
            play.next += 1;
            play.task = None;
            play.phase = PuzzlePhase::YourTurn;
        }
        self.cursor = mv.to();
        self.selection = Selection::None;
        self.refresh_puzzle_status();
        cx.notify();
    }

    fn apply_puzzle_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        let (index, expected, line) = {
            let Some(play) = &self.puzzle else {
                return;
            };
            if play.phase != PuzzlePhase::YourTurn || play.next >= play.line.len() {
                return;
            }
            (play.next, play.line[play.next], play.line.clone())
        };
        let captures = mv.is_capture(&self.position.board);
        match grade(&line, index, mv) {
            Verdict::Wrong => {
                let key = format_san(&self.position, expected);
                push_san(&mut self.position, &mut self.records, mv);
                self.play_landed(mv, captures, Outcome::PuzzleMiss);
                self.cursor = mv.to();
                self.finish_puzzle(false, key, cx);
            }
            Verdict::Continue => {
                push_san(&mut self.position, &mut self.records, mv);
                self.play_landed(
                    mv,
                    captures,
                    Outcome::Play {
                        check: self.position.in_check_self(),
                    },
                );
                self.cursor = mv.to();
                self.selection = Selection::None;
                if let Some(play) = self.puzzle.as_mut() {
                    play.next += 1;
                }
                self.schedule_reply(cx);
            }
            Verdict::Solved => {
                push_san(&mut self.position, &mut self.records, mv);
                self.play_landed(mv, captures, Outcome::PuzzleSolved);
                self.cursor = mv.to();
                self.selection = Selection::None;
                self.finish_puzzle(true, String::new(), cx);
            }
        }
    }

    fn give_up(&mut self, cx: &mut Context<Self>) {
        let expected = {
            let Some(play) = &self.puzzle else {
                return;
            };
            if play.phase != PuzzlePhase::YourTurn || play.next >= play.line.len() {
                return;
            }
            play.line[play.next]
        };
        let key = format_san(&self.position, expected);
        sound::play(Sound::Error);
        self.finish_puzzle(false, key, cx);
    }

    fn finish_puzzle(&mut self, solved: bool, key: String, cx: &mut Context<Self>) {
        let Some(summary) = self.puzzle.as_ref().map(|play| {
            (
                play.rated,
                play.locked,
                play.kind,
                play.puzzle.rating,
                play.puzzle.id.clone(),
                play.streak,
            )
        }) else {
            return;
        };
        let (already_rated, locked, kind, puzzle_rating, id, streak) = summary;
        if already_rated || locked {
            self.mark_practice(solved, &key, cx);
            return;
        }
        match kind {
            SessionKind::Rush => self.finish_rush(solved, key, cx),
            SessionKind::Daily => self.finish_daily(solved, key, puzzle_rating, id, cx),
            SessionKind::Rated | SessionKind::Theme => {
                self.finish_rated(solved, key, puzzle_rating, id, streak, cx);
            }
        }
    }

    fn mark_practice(&mut self, solved: bool, key: &str, cx: &mut Context<Self>) {
        if let Some(play) = self.puzzle.as_mut() {
            play.phase = if solved {
                PuzzlePhase::Solved
            } else {
                PuzzlePhase::Failed
            };
            play.task = None;
        }
        self.selection = Selection::None;
        self.generation += 1;
        self.status = if solved {
            "Solved.".into()
        } else {
            format!("Missed. The move was {key}.")
        };
        cx.notify();
    }

    fn finish_rated(
        &mut self,
        solved: bool,
        key: String,
        puzzle_rating: i32,
        id: String,
        streak: u32,
        cx: &mut Context<Self>,
    ) {
        let streak = if solved { streak + 1 } else { 0 };
        if let Some(play) = self.puzzle.as_mut() {
            play.rated = true;
            play.streak = streak;
            play.phase = if solved {
                PuzzlePhase::Solved
            } else {
                PuzzlePhase::Failed
            };
            play.task = None;
        }
        self.selection = Selection::None;
        self.generation += 1;
        self.settings.puzzle_rating =
            puzzle::adjust_rating(self.settings.puzzle_rating, puzzle_rating, solved);
        self.remember_seen(&id);
        self.save_settings();
        self.status = if solved {
            if streak > 1 {
                format!("Solved. Streak {streak}.")
            } else {
                "Solved.".into()
            }
        } else {
            format!("Missed. The move was {key}.")
        };
        cx.notify();
    }

    fn remember_seen(&mut self, id: &str) {
        if !self.settings.puzzle_seen.iter().any(|seen| seen == id) {
            self.settings.puzzle_seen.push(id.to_string());
        }
    }

    fn finish_daily(
        &mut self,
        solved: bool,
        key: String,
        puzzle_rating: i32,
        id: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(play) = self.puzzle.as_mut() {
            play.rated = true;
            play.phase = if solved {
                PuzzlePhase::Solved
            } else {
                PuzzlePhase::Failed
            };
            play.task = None;
        }
        self.selection = Selection::None;
        self.generation += 1;
        let today = today_string();
        self.remember_seen(&id);
        if self.settings.puzzle_daily != today {
            self.settings.puzzle_rating =
                puzzle::adjust_rating(self.settings.puzzle_rating, puzzle_rating, solved);
            self.settings.puzzle_daily = today;
            self.save_settings();
        }
        self.status = if solved {
            "Today's puzzle is solved.".into()
        } else {
            format!("Missed. The move was {key}.")
        };
        cx.notify();
    }

    fn finish_rush(&mut self, solved: bool, key: String, cx: &mut Context<Self>) {
        let (score, strikes) = self
            .puzzle
            .as_ref()
            .map(|play| (play.rush_score, play.rush_strikes))
            .unwrap_or((0, 0));
        let (score, strikes, over) = rush_step(score, strikes, solved);
        if let Some(play) = self.puzzle.as_mut() {
            play.rush_score = score;
            play.rush_strikes = strikes;
            play.rated = true;
            play.phase = if solved {
                PuzzlePhase::Solved
            } else {
                PuzzlePhase::Failed
            };
            play.task = None;
        }
        self.selection = Selection::None;
        self.generation += 1;
        if over {
            self.end_rush(cx);
            return;
        }
        if solved {
            self.status = format!("Correct. {score} solved.");
            self.arm_rush_advance(cx);
        } else {
            self.status = format!("Missed. The move was {key}. Strike {strikes} of 3.");
        }
        cx.notify();
    }

    fn set_session(&mut self, kind: SessionKind, cx: &mut Context<Self>) {
        let fresh = self.puzzle.is_none();
        if fresh {
            self.enter_puzzles(cx);
        }
        let Some(play) = self.puzzle.as_ref() else {
            return;
        };
        if play.kind == kind && kind != SessionKind::Theme {
            if fresh {
                sound::play(Sound::Toggle);
            }
            return;
        }
        sound::play(Sound::Toggle);
        let cycle = kind == SessionKind::Theme && play.kind == SessionKind::Theme;
        if let Some(play) = self.puzzle.as_mut() {
            if cycle {
                play.theme = play.theme.cycle();
            }
            play.kind = kind;
            play.streak = 0;
            play.rush_score = 0;
            play.rush_strikes = 0;
            play.rush_over = false;
            play.rush_seen.clear();
            play.rated = false;
            play.locked = false;
            play.deadline = None;
            play.clock = None;
            play.advance = None;
            play.epoch = play.epoch.wrapping_add(1);
        }
        if kind == SessionKind::Rush {
            self.start_rush_clock(cx);
        }
        self.load_another(cx);
    }

    fn restart_rush(&mut self, cx: &mut Context<Self>) {
        if let Some(play) = self.puzzle.as_mut() {
            play.kind = SessionKind::Rush;
            play.rush_score = 0;
            play.rush_strikes = 0;
            play.rush_over = false;
            play.rush_seen.clear();
            play.rated = false;
            play.locked = false;
            play.clock = None;
            play.advance = None;
            play.epoch = play.epoch.wrapping_add(1);
        }
        self.start_rush_clock(cx);
        self.load_another(cx);
    }

    fn start_rush_clock(&mut self, cx: &mut Context<Self>) {
        let deadline = Instant::now() + Duration::from_secs(puzzle::RUSH_SECS);
        if let Some(play) = self.puzzle.as_mut() {
            play.deadline = Some(deadline);
            play.rush_over = false;
            play.rush_chime = None;
        }
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let stop = this
                .update(cx, |this, cx| this.on_rush_tick(cx))
                .unwrap_or(true);
            if stop {
                break;
            }
        });
        if let Some(play) = self.puzzle.as_mut() {
            play.clock = Some(task);
        }
    }

    fn on_rush_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let snapshot = self
            .puzzle
            .as_ref()
            .map(|play| (play.kind, play.rush_over, play.deadline, play.phase));
        let Some((kind, rush_over, deadline, phase)) = snapshot else {
            return true;
        };
        if kind != SessionKind::Rush || rush_over {
            return true;
        }
        let Some(deadline) = deadline else {
            return true;
        };
        if Instant::now() >= deadline {
            self.end_rush(cx);
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let already = self.puzzle.as_ref().and_then(|play| play.rush_chime);
        if let Some(sec) = sound::rush_chime(left.as_secs(), already) {
            if let Some(play) = self.puzzle.as_mut() {
                play.rush_chime = Some(sec);
            }
            sound::play(Sound::Tick);
        }
        if !matches!(phase, PuzzlePhase::Solved | PuzzlePhase::Failed) {
            self.refresh_puzzle_status();
        }
        cx.notify();
        false
    }

    fn end_rush(&mut self, cx: &mut Context<Self>) {
        let Some(score) = self.puzzle.as_ref().and_then(|play| {
            if play.rush_over {
                None
            } else {
                Some(play.rush_score)
            }
        }) else {
            return;
        };
        if let Some(play) = self.puzzle.as_mut() {
            play.rush_over = true;
            play.phase = PuzzlePhase::Failed;
            play.epoch = play.epoch.wrapping_add(1);
            play.task = None;
            play.advance = None;
        }
        let best = self.settings.puzzle_rush_best.max(score);
        self.settings.puzzle_rush_best = best;
        self.save_settings();
        self.selection = Selection::None;
        self.generation += 1;
        self.status = format!("Rush over. You solved {score}. Best is {best}.");
        sound::play(Sound::Down);
        cx.notify();
    }

    fn arm_rush_advance(&mut self, cx: &mut Context<Self>) {
        let epoch = {
            let Some(play) = self.puzzle.as_mut() else {
                return;
            };
            play.epoch = play.epoch.wrapping_add(1);
            play.epoch
        };
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENGINE_MOVE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                let ready = this.puzzle.as_ref().is_some_and(|play| {
                    play.epoch == epoch && play.kind == SessionKind::Rush && !play.rush_over
                });
                if ready {
                    this.load_another(cx);
                }
            });
        });
        if let Some(play) = self.puzzle.as_mut() {
            play.advance = Some(task);
        }
    }

    fn refresh_puzzle_status(&mut self) {
        let Some((phase, rush_over)) = self
            .puzzle
            .as_ref()
            .map(|play| (play.phase, play.rush_over))
        else {
            return;
        };
        if rush_over || matches!(phase, PuzzlePhase::Solved | PuzzlePhase::Failed) {
            return;
        }
        self.status = match phase {
            PuzzlePhase::Busy => "Watch this move.".into(),
            PuzzlePhase::YourTurn => "Find the best move.".into(),
            PuzzlePhase::Solved | PuzzlePhase::Failed => return,
        };
    }
}

fn format_clock(left: Duration) -> String {
    let secs = left.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn puzzle_day_key() -> u64 {
    today_string()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn restore_game(saved: &SavedGame) -> Option<(Position, Vec<MoveRecord>)> {
    let mut position = Position::new();
    let mut records = Vec::new();
    for uci in &saved.moves {
        let mv = position.parse_uci(uci)?;
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

struct SideTone {
    page: Hsla,
    ink: Hsla,
    muted: Hsla,
    surface: Hsla,
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
            .flex_none()
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
            .w_full()
            .flex_1()
            .gap_1()
            .overflow_scroll()
            .children(rows)
            .when(self.records.is_empty(), |list| {
                list.child(
                    Label::new(if self.puzzle.is_some() {
                        "The line will show here."
                    } else {
                        "No moves yet."
                    })
                    .text_size(px(13. * scale))
                    .text_color(muted),
                )
            })
            .into_any_element()
    }

    /// `board_side` is the board's outer edge. The panel uses that height so
    /// a tall window cannot stretch it.
    fn render_side(
        &mut self,
        tone: SideTone,
        scale: f32,
        board_side: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let SideTone {
            page,
            ink,
            muted,
            surface,
            accent,
        } = tone;
        let in_puzzle = self.puzzle.is_some();
        let headline = self.status.clone();
        let terminal = self.game_ended()
            || self.puzzle.as_ref().is_some_and(|play| {
                play.rush_over || matches!(play.phase, PuzzlePhase::Solved | PuzzlePhase::Failed)
            });
        let last = self
            .records
            .last()
            .map(|record| (self.position.turn.opposite().label(), record.san.clone()));
        let (phase, rush_over, attempt_scored, session, theme_label, facts) =
            if let Some(play) = &self.puzzle {
                let mut facts = vec![
                    fact(
                        "Puzzle rating",
                        play.puzzle.rating.to_string(),
                        muted,
                        ink,
                        scale,
                    ),
                    fact(
                        "Your rating",
                        self.settings.puzzle_rating.to_string(),
                        muted,
                        ink,
                        scale,
                    ),
                ];
                if play.kind == SessionKind::Theme {
                    facts.push(fact("Motif", play.theme.label().into(), muted, ink, scale));
                }
                if play.kind == SessionKind::Rush {
                    if let Some(deadline) = play.deadline.filter(|_| !play.rush_over) {
                        let left = deadline.saturating_duration_since(Instant::now());
                        facts.push(fact("Time left", format_clock(left), muted, ink, scale));
                    }
                    facts.push(fact(
                        "Solved",
                        play.rush_score.to_string(),
                        muted,
                        ink,
                        scale,
                    ));
                    facts.push(fact(
                        "Misses",
                        format!("{} of 3", play.rush_strikes),
                        muted,
                        ink,
                        scale,
                    ));
                    facts.push(fact(
                        "Best",
                        self.settings.puzzle_rush_best.to_string(),
                        muted,
                        ink,
                        scale,
                    ));
                } else if play.streak > 0 {
                    facts.push(fact("Streak", play.streak.to_string(), muted, ink, scale));
                }
                (
                    Some(play.phase),
                    play.rush_over,
                    play.rated,
                    Some(play.kind),
                    play.theme.label(),
                    facts,
                )
            } else {
                (None, false, false, None, "Fork", Vec::new())
            };
        let detail = if in_puzzle {
            puzzle_detail(
                phase.unwrap_or(PuzzlePhase::Busy),
                rush_over,
                session == Some(SessionKind::Daily),
                last.as_ref().map(|(who, san)| (*who, san.as_str())),
            )
        } else {
            seat_line(self.mode).to_string()
        };
        let live_rush = session == Some(SessionKind::Rush)
            && !rush_over
            && !attempt_scored
            && phase == Some(PuzzlePhase::YourTurn);
        let next_label = if rush_over {
            "New rush"
        } else if live_rush {
            "Skip"
        } else if in_puzzle {
            "Next"
        } else {
            "New game"
        };
        let next_tip = if live_rush {
            "Skip. This counts as a miss."
        } else if in_puzzle {
            "Next puzzle"
        } else {
            "New game"
        };
        let mut next = Button::new("side-next")
            .small()
            .label(next_label)
            .tooltip_with_action(next_tip, &NewGame, Some("pichess"))
            .on_click(cx.listener(|this, _, _, cx| this.new_game(cx)));
        if terminal && in_puzzle {
            next = next.primary();
        } else {
            next = next.ghost();
        }

        v_flex()
            .id("side")
            .flex_none()
            .w(px(320.))
            .h(board_side)
            .overflow_scroll()
            .gap_3()
            .p_4()
            .rounded_lg()
            .bg(surface)
            .border_1()
            .border_color(page.blend(ink.opacity(0.08)))
            .child(
                Label::new(headline)
                    .text_size(px(22. * scale))
                    .text_color(if terminal { accent } else { ink }),
            )
            .child(
                Label::new(detail)
                    .text_size(px(14. * scale))
                    .text_color(muted),
            )
            .children(facts)
            .when(in_puzzle, |panel| {
                panel.child(
                    v_flex()
                        .gap_1()
                        .child(
                            Label::new("Session")
                                .text_size(px(12. * scale))
                                .text_color(muted),
                        )
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_0p5()
                                .child(segment_button(
                                    "pz-rated",
                                    "Rated",
                                    "Endless puzzles near your rating",
                                    session == Some(SessionKind::Rated),
                                    None,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_session(SessionKind::Rated, cx);
                                    }),
                                ))
                                .child(segment_button(
                                    "pz-rush",
                                    "Rush",
                                    "Three minutes. Three misses ends the run.",
                                    session == Some(SessionKind::Rush),
                                    None,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_session(SessionKind::Rush, cx);
                                    }),
                                ))
                                .child(segment_button(
                                    "pz-daily",
                                    "Daily",
                                    "One puzzle for today",
                                    session == Some(SessionKind::Daily),
                                    None,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_session(SessionKind::Daily, cx);
                                    }),
                                ))
                                .child(segment_button(
                                    "pz-theme",
                                    theme_label,
                                    "Click again to change the motif",
                                    session == Some(SessionKind::Theme),
                                    None,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_session(SessionKind::Theme, cx);
                                    }),
                                )),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(next)
                    .child(
                        Button::new("side-undo")
                            .ghost()
                            .small()
                            .label(if in_puzzle { "Retry" } else { "Undo" })
                            .tooltip_with_action(
                                if in_puzzle {
                                    "Retry this puzzle"
                                } else {
                                    "Undo"
                                },
                                &Undo,
                                Some("pichess"),
                            )
                            .disabled(self.thinking && !in_puzzle)
                            .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                    )
                    .when(in_puzzle, |row| {
                        row.child(
                            Button::new("side-solution")
                                .ghost()
                                .small()
                                .label("Show the move")
                                .tooltip_with_action("Show the move", &Resign, Some("pichess"))
                                .disabled(terminal)
                                .on_click(cx.listener(|this, _, _, cx| this.resign(cx))),
                        )
                    })
                    .when(!in_puzzle, |row| {
                        row.child(
                            Button::new("side-resign")
                                .ghost()
                                .small()
                                .label("Resign")
                                .tooltip_with_action("Resign", &Resign, Some("pichess"))
                                .disabled(self.game_ended())
                                .on_click(cx.listener(|this, _, _, cx| this.resign(cx))),
                        )
                        .child(
                            Button::new("side-hint")
                                .ghost()
                                .small()
                                .label("Hint")
                                .tooltip_with_action(
                                    "Engine plays this move",
                                    &EngineHint,
                                    Some("pichess"),
                                )
                                .disabled(self.thinking || self.game_ended())
                                .on_click(cx.listener(|this, _, _, cx| this.engine_hint(cx))),
                        )
                        .child(
                            Button::new("side-pgn")
                                .ghost()
                                .small()
                                .label("Export")
                                .tooltip_with_action(
                                    "Export game as PGN",
                                    &ExportPgn,
                                    Some("pichess"),
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.export_pgn(cx))),
                        )
                    })
                    .child(
                        Button::new("side-flip")
                            .ghost()
                            .small()
                            .label("Flip")
                            .tooltip_with_action("Flip board", &FlipBoard, Some("pichess"))
                            .on_click(cx.listener(|this, _, _, cx| this.flip(cx))),
                    ),
            )
            .child(
                Label::new(if in_puzzle { "Line" } else { "Moves" })
                    .text_size(px(12. * scale))
                    .text_color(muted),
            )
            .child(self.render_move_list(ink, muted, accent, scale))
            .into_any_element()
    }
}

fn fact(label: &'static str, value: String, muted: Hsla, ink: Hsla, scale: f32) -> AnyElement {
    h_flex()
        .w_full()
        .justify_between()
        .child(
            Label::new(label)
                .text_size(px(13. * scale))
                .text_color(muted),
        )
        .child(Label::new(value).text_size(px(13. * scale)).text_color(ink))
        .into_any_element()
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
        // Size every square from the space under the mode bar, so the eighth
        // rank and its file letters stay inside the window.
        let viewport = window.viewport_size();
        let cell_px = board_cell(f32::from(viewport.height), f32::from(viewport.width), scale);
        let cell = px(cell_px);
        let board_side = px(cell_px * 8.0 + 3.0 * scale * 2.0 + 2.0 * scale * 7.0 + 2.0);
        let order = board_display_order(self.flipped);
        let status = self.status.clone();
        let level = self.level;
        let mode = self.mode;
        let thinking = self.thinking;
        let in_puzzle = self.puzzle.is_some();
        let selection = self.selection;
        window.set_window_title(&format!("Pichess — {status}"));

        let header = h_flex()
            .id("hud")
            .w_full()
            .px_3()
            .py_1()
            .gap_2()
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
                        "Two Players",
                        "Hotseat: both sides by hand",
                        !in_puzzle && mode == Mode::TwoPlayers,
                        Some("ctrl-shift-2"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::TwoPlayers, cx)),
                    ))
                    .child(segment_button(
                        "mode-w",
                        "Play White",
                        "You play White, the engine answers",
                        !in_puzzle && mode == Mode::HumanWhite,
                        Some("ctrl-shift-1"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::HumanWhite, cx)),
                    ))
                    .child(segment_button(
                        "mode-b",
                        "Play Black",
                        "You play Black, the engine opens",
                        !in_puzzle && mode == Mode::HumanBlack,
                        Some("ctrl-shift-3"),
                        cx.listener(|this, _, _, cx| this.set_mode(Mode::HumanBlack, cx)),
                    ))
                    .child(segment_button(
                        "mode-puzzles",
                        "Solve Puzzles",
                        "Tactics from real games. Leave by picking a play mode",
                        in_puzzle,
                        None,
                        cx.listener(|this, _, _, cx| {
                            let fresh = this.puzzle.is_none();
                            this.enter_puzzles(cx);
                            if fresh && this.puzzle.is_some() {
                                sound::play(Sound::Toggle);
                            }
                        }),
                    )),
            )
            .when(!in_puzzle, |bar| {
                bar.child(
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
            })
            .child(div().flex_1())
            .child(
                Button::new("keys")
                    .ghost()
                    .small()
                    .label("Keys")
                    .tooltip_with_action("Keyboard shortcuts", &ToggleHelp, Some("pichess"))
                    .on_click(cx.listener(|this, _, _, cx| this.set_help(!this.show_help, cx))),
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
            .relative()
            .flex_none()
            .w(board_side)
            .h(board_side)
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
                        .occlude()
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
                                        promo_button(PieceKind::Queen, mover_color, "q", cx),
                                        promo_button(PieceKind::Rook, mover_color, "r", cx),
                                        promo_button(PieceKind::Bishop, mover_color, "b", cx),
                                        promo_button(PieceKind::Knight, mover_color, "n", cx),
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
                    cx.listener(|this, _, _, cx| this.set_help(false, cx)),
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
                this.set_help(!this.show_help, cx);
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
                    .justify_center()
                    .items_center()
                    .gap_3()
                    .p(px(12.))
                    .overflow_hidden()
                    .child(board)
                    .child(self.render_side(
                        SideTone {
                            page,
                            ink,
                            muted,
                            surface,
                            accent,
                        },
                        scale,
                        board_side,
                        cx,
                    )),
            )
            .children(overlay)
    }
}

const HELP_ROWS: [&str; 12] = [
    "click / space   pick up & drop",
    "arrows / hjkl  cursor",
    "u              undo, or retry a puzzle",
    "n              new game, or next puzzle",
    "r              resign, or show the puzzle move",
    "e              export PGN",
    "a              engine plays this move (not during a puzzle)",
    "v              flip board",
    "1 2 3          easy / medium / hard",
    "Puzzles        rated, rush, daily, or a theme",
    "F11 / Super+F  fullscreen",
    "Ctrl+Q         quit",
];

/// The board stops growing once the window is this large. Anything beyond
/// that stays empty background. The window itself is not resized: Omarchy
/// tiles the frame, and pulling it back from inside the app fights the layout.
const LAYOUT_MAX_W: f32 = 1120.0;
const LAYOUT_MAX_H: f32 = 800.0;

/// Square size that keeps all eight ranks on screen.
///
/// The reserved height is the mode bar (padding, one button row, border),
/// the padding around the board, the board's own frame and gaps, and a
/// little slack so the file letters are not flush with the window edge.
fn board_cell(viewport_h: f32, viewport_w: f32, text_scale: f32) -> f32 {
    let viewport_h = viewport_h.min(LAYOUT_MAX_H);
    let viewport_w = viewport_w.min(LAYOUT_MAX_W);
    let button = 32.0 * text_scale.max(1.0);
    let header = 8.0 + button + 8.0 + 1.0;
    let main_pad = 12.0 * 2.0;
    let board_pad = 3.0 * text_scale * 2.0;
    let gaps = 2.0 * text_scale * 7.0;
    let slack = 12.0;
    let from_h = (viewport_h - header - main_pad - board_pad - gaps - slack) / 8.0;
    // The side panel keeps at least this much; the rest of the width is squares.
    let panel = 320.0;
    let row_gap = 12.0;
    let from_w = (viewport_w - panel - main_pad - row_gap - board_pad - gaps - 2.0) / 8.0;
    from_h.min(from_w).clamp(24.0, 72.0 * text_scale)
}

fn selection_promotion(selection: Selection) -> Option<PendingPromotion> {
    match selection {
        Selection::Promoting(pending) => Some(pending),
        _ => None,
    }
}

/// What the side panel says under the headline during a puzzle.
fn puzzle_detail(
    phase: PuzzlePhase,
    rush_over: bool,
    daily: bool,
    last: Option<(&str, &str)>,
) -> String {
    if rush_over {
        return "Press New rush to start again.".into();
    }
    match phase {
        PuzzlePhase::YourTurn => match last {
            Some((who, san)) => format!("{who} played {san}. Only one reply works."),
            None => "Only one reply works.".into(),
        },
        PuzzlePhase::Busy => match last {
            Some((who, san)) => format!("{who} played {san}."),
            None => "A position from a real game is about to appear.".into(),
        },
        PuzzlePhase::Solved => "That was the line.".into(),
        PuzzlePhase::Failed if daily => {
            "Retry plays this one again. A new daily arrives tomorrow.".into()
        }
        PuzzlePhase::Failed => "Next deals another. Retry plays this one again.".into(),
    }
}

fn seat_line(mode: Mode) -> &'static str {
    match mode {
        Mode::TwoPlayers => "Both sides are played by hand.",
        Mode::HumanWhite => "You have White. The computer answers.",
        Mode::HumanBlack => "You have Black. The computer has White.",
    }
}

/// Screen-direction delta to board delta: the flipped view mirrors both
/// board axes, so a screen step is the opposite board step.
fn cursor_screen_delta(flipped: bool, df: i32, dr: i32) -> (i32, i32) {
    if flipped {
        (-df, -dr)
    } else {
        (df, dr)
    }
}

fn promo_button(
    kind: PieceKind,
    color: Color,
    key: &'static str,
    cx: &mut Context<PichessApp>,
) -> Button {
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
        .on_click(cx.listener(move |this, _, _, cx| this.choose_promotion(kind, cx)))
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
    fn board_cell_fits_the_window_min_size() {
        // Window minimum is 760×560. Every rank, plus the frame around it,
        // has to land inside that.
        let cell = super::board_cell(560.0, 760.0, 1.0);
        let board_h = cell * 8.0 + 2.0 * 7.0 + 3.0 * 2.0 + 2.0;
        let header = 8.0 + 32.0 + 8.0 + 1.0;
        let used = header + 12.0 * 2.0 + board_h + 12.0;
        assert!(used <= 560.0, "used {used} cell {cell}");
        let row_w = board_h + 12.0 + 320.0 + 24.0;
        assert!(row_w <= 760.0, "row {row_w} cell {cell}");
        assert!(cell >= 24.0);
        let huge = super::board_cell(1600.0, 2000.0, 1.0);
        let capped = super::board_cell(super::LAYOUT_MAX_H, super::LAYOUT_MAX_W, 1.0);
        assert!(huge <= 72.0);
        assert_eq!(huge, capped);
    }

    #[test]
    fn puzzle_panel_says_who_just_moved() {
        use super::{puzzle_detail, seat_line, PuzzlePhase};
        use pichess::stats::Mode;
        assert_eq!(
            puzzle_detail(PuzzlePhase::YourTurn, false, false, Some(("White", "Qe7"))),
            "White played Qe7. Only one reply works."
        );
        assert_eq!(
            puzzle_detail(PuzzlePhase::Busy, false, false, None),
            "A position from a real game is about to appear."
        );
        assert_eq!(
            puzzle_detail(PuzzlePhase::Failed, true, false, None),
            "Press New rush to start again."
        );
        assert_eq!(
            puzzle_detail(PuzzlePhase::Failed, false, true, None),
            "Retry plays this one again. A new daily arrives tomorrow."
        );
        assert_eq!(
            seat_line(Mode::HumanWhite),
            "You have White. The computer answers."
        );
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
