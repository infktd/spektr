//! `AppState`: everything the render functions read. Rendering is a pure
//! function of this state (TUI_SPEC §11); `update` is the only writer.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;

use jiff::Timestamp;
use ratatui::layout::Rect;
use spektr_engine::{CaseStatus, EgoParams, EgoView, EntityDetail, EntitySummary, ModuleInfo};
use spektr_model::{EdgeId, EntityId, EntityType};

use crate::ego::{Col, Item};
use crate::glyph::{GlyphSet, Marker};
use crate::host::{DoctorLine, RecentCase, Settings};
use crate::keymap::{KeyPress, Keymap};
use crate::theme::{Theme, Token};

/// Activity rail capacity (TUI_SPEC §4.5).
pub const ACTIVITY_CAP: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pane {
    Entities,
    Graph,
    Inspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Ego,
    Timeline,
    Path,
}

/// The mode pill (TUI_SPEC §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
    Filter,
    Insert,
    Review,
    Findings,
    Checklist,
    Tutor,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Command => "COMMAND",
            Mode::Filter => "FILTER",
            Mode::Insert => "INSERT",
            Mode::Review => "REVIEW",
            Mode::Findings => "FINDINGS",
            Mode::Checklist => "CHECKLIST",
            Mode::Tutor => "TUTOR",
        }
    }

    pub fn token(self) -> Token {
        match self {
            Mode::Normal | Mode::Review | Mode::Findings | Mode::Checklist => Token::Accent,
            Mode::Command => Token::Warn,
            Mode::Filter | Mode::Insert => Token::Fg,
            Mode::Tutor => Token::Ok,
        }
    }
}

/// What a mouse event can land on. The render functions return these with
/// their rectangles; later entries are drawn on top (TUI_SPEC §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hit {
    Pane(Pane),
    EntRow(usize),
    Ego(Col, usize),
    Card,
    Crumb(usize),
    InspRow(usize),
    Activity,
    Tab(Tab),
    Pill,
    LauncherRow(usize),
    PaletteItem(usize),
    MenuItem(usize),
    SubItem(usize),
    DialogItem(usize),
    /// Inside an overlay but on nothing actionable: absorbs the click.
    Overlay,
}

pub struct AppState {
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub theme: Theme,
    pub glyphs: GlyphSet,
    pub mouse: bool,
    pub keymap: Keymap,
    pub settings: Settings,
    pub pending: Vec<KeyPress>,
    pub size: (u16, u16),
    pub now: Timestamp,
    pub hits: Vec<(Rect, Hit)>,
    pub hover: Option<Hit>,
    pub last_click: Option<(u64, Hit)>,
    /// `ctrl-c` once arms force-quit.
    pub force_quit_armed: bool,
    pub quit: bool,
    /// Something changed since the last render.
    pub dirty: bool,
    /// A one-line message in the status line: `(text, token, until)`.
    pub toast: Option<(String, Token, Timestamp)>,
    /// Palette commands run this session, newest first.
    pub history: Vec<String>,
    pub profiles: Vec<String>,
    pub warnings: Vec<String>,
    /// A verify the user asked for: its result is shown as a toast.
    pub verify_requested: Option<PathBuf>,
}

pub enum Screen {
    Launcher(Launcher),
    /// A case is opening.
    Opening(PathBuf),
    Workspace(Box<Workspace>),
}

#[derive(Debug, Default)]
pub struct Launcher {
    pub recent: Option<Vec<RecentCase>>,
    pub cursor: usize,
    pub doctor: Option<Vec<DoctorLine>>,
    pub error: Option<String>,
}

/// One row of the entities pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntItem {
    Header {
        ty: EntityType,
        count: usize,
        expanded: bool,
    },
    /// Index into `Workspace::entities`.
    Row(usize),
    More {
        ty: EntityType,
        hidden: usize,
    },
    /// `⊘ N out of scope · hidden`.
    Hidden(usize),
    Blank,
}

#[derive(Debug, Clone, Default)]
pub struct FilterState {
    pub text: String,
    /// Still typing (FILTER mode) rather than kept.
    pub editing: bool,
    pub matches: usize,
}

#[derive(Debug, Clone)]
pub struct ActRow {
    pub at: Timestamp,
    pub marker: Marker,
    pub token: Token,
    pub module: String,
    pub module_token: Token,
    pub spans: Vec<(String, Token)>,
}

#[derive(Debug, Clone, Default)]
pub struct Stale {
    pub status: bool,
    pub entities: bool,
    pub ego: bool,
    pub detail: bool,
}

impl Stale {
    pub fn any(&self) -> bool {
        self.status || self.entities || self.ego || self.detail
    }

    pub fn all() -> Self {
        Self {
            status: true,
            entities: true,
            ego: true,
            detail: true,
        }
    }
}

pub struct Workspace {
    pub path: PathBuf,
    pub chain: crate::host::Chain,
    pub status: Option<CaseStatus>,
    pub modules: Vec<ModuleInfo>,
    /// Root entities, sorted for the entities pane.
    pub entities: Vec<EntitySummary>,
    pub ent_items: Vec<EntItem>,
    pub ent_cursor: usize,
    /// Groups the user toggled, by type name: `true` expanded.
    pub group_open: BTreeMap<String, bool>,
    /// Groups whose `··· more` was expanded in place.
    pub group_all: BTreeSet<String>,
    pub filter: Option<FilterState>,

    pub focus: Option<EntityId>,
    /// The pivot history, oldest first; the last is the focus.
    pub crumbs: Vec<EntitySummary>,
    pub ego: Option<EgoView>,
    pub ego_params: EgoParams,
    pub ego_gen: u64,
    pub ego_col: Col,
    pub ego_idx: usize,

    /// The entity the inspector shows (follows the cursor).
    pub inspect: Option<EntityId>,
    pub detail: Option<EntityDetail>,
    pub insp_cursor: usize,
    pub insp_overlay: bool,

    pub activity: VecDeque<ActRow>,
    pub act_scroll: usize,
    /// Finished jobs from before this session, until the rail is seeded.
    pub past_jobs: Option<Vec<spektr_engine::JobView>>,
    /// `(entity, module)` → last finished run.
    pub last_run: HashMap<(EntityId, String), Timestamp>,
    pub running: u64,
    pub queued: u64,
    pub done: u64,
    pub failed: u64,

    pub pane: Pane,
    pub tab: Tab,
    /// Entities added this session that the cursor hasn't visited (`•`).
    pub fresh: BTreeSet<EntityId>,
    pub stale: Stale,
    pub refresh_scheduled: bool,
}

impl Workspace {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            chain: crate::host::Chain::Unknown,
            status: None,
            modules: Vec::new(),
            entities: Vec::new(),
            ent_items: Vec::new(),
            ent_cursor: 0,
            group_open: BTreeMap::new(),
            group_all: BTreeSet::new(),
            filter: None,
            focus: None,
            crumbs: Vec::new(),
            ego: None,
            ego_params: EgoParams::default(),
            ego_gen: 0,
            ego_col: Col::Right,
            ego_idx: 0,
            inspect: None,
            detail: None,
            insp_cursor: 0,
            insp_overlay: false,
            activity: VecDeque::new(),
            act_scroll: 0,
            past_jobs: None,
            last_run: HashMap::new(),
            running: 0,
            queued: 0,
            done: 0,
            failed: 0,
            pane: Pane::Graph,
            tab: Tab::Ego,
            fresh: BTreeSet::new(),
            stale: Stale::default(),
            refresh_scheduled: false,
        }
    }

    pub fn entity(&self, id: EntityId) -> Option<&EntitySummary> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// The entity under the entities-pane cursor.
    pub fn ent_cursor_entity(&self) -> Option<&EntitySummary> {
        match self.ent_items.get(self.ent_cursor)? {
            EntItem::Row(i) => self.entities.get(*i),
            _ => None,
        }
    }

    pub fn ego_item(&self) -> Option<Item> {
        let ego = self.ego.as_ref()?;
        crate::ego::column_items(ego, self.ego_col)
            .get(self.ego_idx)
            .copied()
    }

    /// The entity under the graph-pane cursor (the focus on the card).
    pub fn ego_cursor_entity(&self) -> Option<&EntitySummary> {
        let ego = self.ego.as_ref()?;
        if self.ego_col == Col::Card {
            return Some(&ego.focus);
        }
        crate::ego::row(ego, self.ego_item()?).map(|(_, r)| &r.entity)
    }

    /// The entity actions apply to: the cursor in the focused pane.
    pub fn cursor_entity(&self) -> Option<&EntitySummary> {
        match self.pane {
            Pane::Entities => self.ent_cursor_entity(),
            Pane::Graph => self.ego_cursor_entity(),
            Pane::Inspector => self
                .inspect
                .and_then(|id| self.entity(id))
                .or_else(|| self.detail.as_ref().map(|d| &d.entity.summary)),
        }
    }

    /// The edge under the cursor (graph rows only).
    pub fn cursor_edge(&self) -> Option<EdgeId> {
        if self.pane != Pane::Graph || self.ego_col == Col::Card {
            return None;
        }
        let ego = self.ego.as_ref()?;
        crate::ego::row(ego, self.ego_item()?).and_then(|(_, r)| r.edges.first().copied())
    }

    pub fn profile(&self) -> &str {
        self.status.as_ref().map_or("", |s| s.profile.as_str())
    }
}

// ---- overlays

pub enum Overlay {
    Palette(Palette),
    Menu(Menu),
    Help(Help),
    Dialog(Dialog),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Modules,
    Commands,
    Recent,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Modules => "Modules",
            Section::Commands => "Commands",
            Section::Recent => "Recent",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub section: Section,
    /// The command line this item runs; a trailing space means it needs an
    /// argument, so `↵` completes it instead of running it.
    pub line: String,
    /// Consumes → produces glyphs, for modules.
    pub signature: String,
    /// Right-hand note: cost, last run, or why it's unavailable.
    pub info: String,
    /// Detail pane: `(title, version · subtitle, body)`.
    pub detail: (String, String, String),
    pub disabled: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub input: String,
    pub sel: usize,
    pub items: Vec<PaletteItem>,
    /// Total candidates before filtering (`3 of 14`).
    pub total: usize,
    pub target: Option<EntitySummary>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAct {
    Key(crate::keymap::Action),
    RunModule(String),
    RunAll,
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    /// Right-aligned: the hotkey, a cost, or a disabled reason.
    pub right: String,
    pub act: MenuAct,
    pub disabled: Option<String>,
    pub submenu: bool,
    /// Draw a rule above this item (a new section).
    pub rule: bool,
}

#[derive(Debug, Clone)]
pub struct Submenu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub sel: usize,
}

#[derive(Debug, Clone)]
pub struct Menu {
    /// The click cell, or the right end of the row.
    pub anchor: (u16, u16),
    pub target: EntitySummary,
    pub edge: Option<EdgeId>,
    /// `subdomain · 2 observations`.
    pub subtitle: String,
    pub items: Vec<MenuItem>,
    pub sel: usize,
    pub sub: Option<Submenu>,
}

#[derive(Debug, Clone, Default)]
pub struct Help {
    pub scroll: usize,
    pub search: String,
    pub searching: bool,
}

#[derive(Debug, Clone)]
pub enum Field {
    Text {
        label: String,
        value: String,
        hint: String,
    },
    Choice {
        label: String,
        options: Vec<String>,
        idx: usize,
    },
}

impl Field {
    pub fn label(&self) -> &str {
        match self {
            Field::Text { label, .. } | Field::Choice { label, .. } => label,
        }
    }

    pub fn value(&self) -> String {
        match self {
            Field::Text { value, .. } => value.clone(),
            Field::Choice { options, idx, .. } => options.get(*idx).cloned().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    NewCase,
    OpenPath,
    Note(EntityId),
    Quit,
    RejectEdge(EdgeId),
    Merge { into: EntityId },
}

#[derive(Debug, Clone)]
pub struct Dialog {
    pub title: String,
    pub body: Vec<String>,
    pub fields: Vec<Field>,
    pub focus: usize,
    /// Picker rows (merge target), filtered by the first text field.
    pub picks: Vec<EntitySummary>,
    pub pick_sel: usize,
    /// Destructive: needs `y`.
    pub destructive: bool,
    pub purpose: Purpose,
    pub error: Option<String>,
}

impl Dialog {
    pub fn confirm(title: &str, body: Vec<String>, destructive: bool, purpose: Purpose) -> Self {
        Self {
            title: title.into(),
            body,
            fields: Vec::new(),
            focus: 0,
            picks: Vec::new(),
            pick_sel: 0,
            destructive,
            purpose,
            error: None,
        }
    }

    pub fn input(title: &str, fields: Vec<Field>, purpose: Purpose) -> Self {
        Self {
            fields,
            ..Self::confirm(title, Vec::new(), false, purpose)
        }
    }

    pub fn has_text_focus(&self) -> bool {
        matches!(self.fields.get(self.focus), Some(Field::Text { .. }))
    }
}

impl AppState {
    pub fn new(settings: Settings, now: Timestamp) -> Self {
        let (keymap, warnings) = Keymap::with_overrides(&settings.keys);
        Self {
            screen: Screen::Launcher(Launcher::default()),
            overlay: None,
            theme: Theme::new(settings.colors),
            glyphs: settings.glyphs,
            mouse: settings.mouse,
            keymap,
            settings,
            pending: Vec::new(),
            size: (0, 0),
            now,
            hits: Vec::new(),
            hover: None,
            last_click: None,
            force_quit_armed: false,
            quit: false,
            dirty: true,
            toast: None,
            history: Vec::new(),
            profiles: Vec::new(),
            warnings,
            verify_requested: None,
        }
    }

    pub fn workspace(&self) -> Option<&Workspace> {
        match &self.screen {
            Screen::Workspace(w) => Some(w),
            _ => None,
        }
    }

    pub fn workspace_mut(&mut self) -> Option<&mut Workspace> {
        match &mut self.screen {
            Screen::Workspace(w) => Some(w),
            _ => None,
        }
    }

    /// The mode pill.
    pub fn mode(&self) -> Mode {
        match &self.overlay {
            Some(Overlay::Palette(_)) => return Mode::Command,
            Some(Overlay::Dialog(d)) if d.has_text_focus() => return Mode::Command,
            _ => {}
        }
        if let Some(w) = self.workspace()
            && w.filter.as_ref().is_some_and(|f| f.editing)
        {
            return Mode::Filter;
        }
        Mode::Normal
    }

    /// Active key contexts, highest precedence first.
    pub fn contexts(&self) -> Vec<crate::keymap::Ctx> {
        use crate::keymap::Ctx;
        let mut c = Vec::new();
        match &self.overlay {
            Some(Overlay::Palette(_)) => c.extend([Ctx::Text, Ctx::Palette]),
            Some(Overlay::Menu(_)) => c.extend([Ctx::Menu, Ctx::Entity]),
            Some(Overlay::Help(h)) => {
                if h.searching {
                    c.push(Ctx::Text);
                }
                c.extend([Ctx::Help, Ctx::Nav]);
            }
            Some(Overlay::Dialog(d)) => {
                if d.has_text_focus() {
                    c.push(Ctx::Text);
                }
                c.push(Ctx::Dialog);
                // A dialog captures everything except force-quit.
                return c;
            }
            None => match &self.screen {
                Screen::Launcher(_) | Screen::Opening(_) => c.extend([Ctx::Launcher, Ctx::Nav]),
                Screen::Workspace(w) => {
                    if w.filter.as_ref().is_some_and(|f| f.editing) {
                        c.extend([Ctx::Text, Ctx::Filter]);
                    } else {
                        c.extend([Ctx::Nav, Ctx::Entity]);
                    }
                }
            },
        }
        c.push(Ctx::Global);
        c
    }
}
