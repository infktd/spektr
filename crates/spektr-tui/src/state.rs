//! `AppState`: everything the render functions read. Rendering is a pure
//! function of this state (TUI_SPEC §11); `update` is the only writer.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;

use jiff::Timestamp;
use ratatui::layout::Rect;
use spektr_engine::{
    CaseStatus, EdgeDetail, EgoParams, EgoView, EntityDetail, EntitySummary, ItemView, ModuleInfo,
    PathView, PlaybookInfo, ReviewView, RunView, TimelineEvent,
};
use spektr_model::{ChecklistItemId, EdgeId, EntityId, EntityType, ReviewItemId};

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
    /// Checklist screen: a run row, an item row, a filter tab.
    RunRow(usize),
    ItemRow(usize),
    ItemFilterTab(ItemFilter),
    /// Review screen: a queue row, a filter tab.
    ReviewRow(usize),
    ReviewFilterTab(ReviewFilter),
    /// Timeline tab row.
    TimeRow(usize),
    /// Findings screen: a list row, a filter tab, a detail row.
    FindingRow(usize),
    FindingFilterTab(FindingFilter),
    FindingDetailRow(usize),
    /// The editor's fields.
    EditorField(usize),
}

/// What fills the workspace: the panes, or a full screen (TUI_SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Main,
    Checklist,
    Review,
    Findings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FindingFilter {
    #[default]
    All,
    Draft,
    Ready,
    Sent,
}

impl FindingFilter {
    pub const ALL: [FindingFilter; 4] = [
        FindingFilter::All,
        FindingFilter::Draft,
        FindingFilter::Ready,
        FindingFilter::Sent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FindingFilter::All => "all",
            FindingFilter::Draft => "draft",
            FindingFilter::Ready => "ready",
            FindingFilter::Sent => "sent",
        }
    }

    pub fn accepts(self, status: &str) -> bool {
        self == FindingFilter::All || self.label() == status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindingsPane {
    #[default]
    List,
    /// The supporting entities and evidence of the selected finding.
    Detail,
}

/// One row of the findings list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingRowItem {
    Category { id: String, open: bool },
    Finding(i64),
}

/// The Findings screen (artboard 09).
#[derive(Debug, Clone, Default)]
pub struct FindingsScreen {
    pub view: Option<spektr_engine::findings::FindingsView>,
    pub detail: Option<spektr_engine::findings::FindingDetail>,
    pub filter: FindingFilter,
    pub pane: FindingsPane,
    /// Index into the selectable rows.
    pub sel: usize,
    /// Index into the detail's entities, then evidence.
    pub dsel: usize,
    /// Categories the user toggled: `true` open.
    pub open: BTreeMap<String, bool>,
    /// Select this finding once the list arrives.
    pub want: Option<i64>,
    /// The finding `a` (attach) adds to: the last one selected.
    pub current: Option<i64>,
}

impl FindingsScreen {
    /// The list's rows: each category (with findings or not), then its
    /// findings when open. Empty categories start closed.
    pub fn rows(&self) -> Vec<FindingRowItem> {
        let Some(v) = &self.view else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cats: Vec<String> = v.categories.iter().map(|c| c.id.clone()).collect();
        for f in &v.findings {
            if !cats.contains(&f.category) {
                cats.push(f.category.clone());
            }
        }
        // Categories with findings first, then by points, highest first
        // (artboard 09); ties keep the profile's order.
        let has = |c: &str| {
            v.findings
                .iter()
                .any(|f| f.category == c && self.filter.accepts(&f.status))
        };
        let pts = |c: &str| {
            v.categories
                .iter()
                .find(|x| x.id == c)
                .and_then(|x| x.points)
                .unwrap_or(0)
        };
        cats.sort_by_key(|c| (!has(c), std::cmp::Reverse(pts(c))));
        for c in cats {
            let mine: Vec<i64> = v
                .findings
                .iter()
                .filter(|f| f.category == c && self.filter.accepts(&f.status))
                .map(|f| f.id)
                .collect();
            let open = self.open.get(&c).copied().unwrap_or(!mine.is_empty());
            out.push(FindingRowItem::Category { id: c, open });
            if open {
                out.extend(mine.into_iter().map(FindingRowItem::Finding));
            }
        }
        out
    }

    pub fn selected_row(&self) -> Option<FindingRowItem> {
        self.rows().get(self.sel).cloned()
    }

    pub fn selected_finding(&self) -> Option<i64> {
        match self.selected_row()? {
            FindingRowItem::Finding(id) => Some(id),
            FindingRowItem::Category { .. } => None,
        }
    }
}

/// The inline finding editor (`e`, mode INSERT).
pub struct Editor {
    pub id: i64,
    /// Title, claim, relevance.
    pub fields: Vec<ratatui_textarea::TextArea<'static>>,
    pub focus: usize,
}

impl Editor {
    pub const LABELS: [&'static str; 3] = ["title", "claim", "relevance"];

    pub fn new(id: i64, title: &str, claim: &str, relevance: &str) -> Self {
        let field = |t: &str| {
            let mut a = ratatui_textarea::TextArea::new(vec![t.replace('\n', " ")]);
            a.set_wrap_mode(ratatui_textarea::WrapMode::Word);
            a.move_cursor(ratatui_textarea::CursorMove::End);
            a
        };
        Self {
            id,
            fields: vec![field(title), field(claim), field(relevance)],
            focus: 1,
        }
    }

    pub fn value(&self, i: usize) -> String {
        self.fields
            .get(i)
            .map(|f| f.lines().join(" ").trim().to_string())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ItemFilter {
    #[default]
    Open,
    Done,
    All,
}

impl ItemFilter {
    pub const ALL: [ItemFilter; 3] = [ItemFilter::Open, ItemFilter::Done, ItemFilter::All];

    pub fn label(self) -> &'static str {
        match self {
            ItemFilter::Open => "open",
            ItemFilter::Done => "done",
            ItemFilter::All => "all",
        }
    }

    pub fn accepts(self, state: &str) -> bool {
        match self {
            ItemFilter::Open => state == "open",
            ItemFilter::Done => state != "open",
            ItemFilter::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChecklistPane {
    #[default]
    Runs,
    Items,
}

/// The Checklist screen (artboard 10).
#[derive(Debug, Clone, Default)]
pub struct ChecklistScreen {
    /// Runs in display order: each top-level run, then its nested runs.
    pub runs: Vec<RunView>,
    pub playbooks: Vec<PlaybookInfo>,
    /// Every item; the filter applies when listing.
    pub items: Vec<ItemView>,
    pub filter: ItemFilter,
    pub pane: ChecklistPane,
    pub run_sel: usize,
    /// Index into the filtered items.
    pub item_sel: usize,
    /// Jump to this item once the items arrive (from the inspector).
    pub want_item: Option<ChecklistItemId>,
    pub loaded: bool,
}

impl ChecklistScreen {
    /// Top-level runs first (newest first), each followed by its children.
    pub fn order(mut runs: Vec<RunView>) -> Vec<RunView> {
        runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));
        let mut out = Vec::new();
        for top in runs.iter().filter(|r| r.parent.is_none()) {
            out.push(top.clone());
            let mut kids: Vec<_> = runs
                .iter()
                .filter(|r| r.parent == Some(top.id))
                .cloned()
                .collect();
            kids.sort_by_key(|r| r.started_at);
            out.extend(kids);
        }
        out
    }

    pub fn visible(&self) -> Vec<&ItemView> {
        self.items
            .iter()
            .filter(|i| self.filter.accepts(&i.state))
            .collect()
    }

    pub fn selected_item(&self) -> Option<&ItemView> {
        self.visible().get(self.item_sel).copied()
    }

    pub fn selected_run(&self) -> Option<&RunView> {
        self.runs.get(self.run_sel)
    }

    pub fn open_count(&self) -> usize {
        self.items.iter().filter(|i| i.state == "open").count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReviewFilter {
    #[default]
    Pending,
    Accepted,
    Rejected,
    Later,
}

impl ReviewFilter {
    pub const ALL: [ReviewFilter; 4] = [
        ReviewFilter::Pending,
        ReviewFilter::Accepted,
        ReviewFilter::Rejected,
        ReviewFilter::Later,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ReviewFilter::Pending => "pending",
            ReviewFilter::Accepted => "accepted",
            ReviewFilter::Rejected => "rejected",
            ReviewFilter::Later => "later",
        }
    }
}

/// The Review screen (artboard 05's layout).
#[derive(Debug, Clone, Default)]
pub struct ReviewScreen {
    pub items: Vec<ReviewView>,
    pub filter: ReviewFilter,
    pub sel: usize,
    /// Side-by-side details for a `same_person` item: `(a, b)`.
    pub compare: Option<(ReviewItemId, Option<EntityDetail>, Option<EntityDetail>)>,
    pub loaded: bool,
    /// Open the queue on this entity's items only (`u`).
    pub only: Option<EntityId>,
}

impl ReviewScreen {
    pub fn visible(&self) -> Vec<&ReviewView> {
        self.items
            .iter()
            .filter(|i| i.state == self.filter.label())
            .filter(|i| match self.only {
                Some(e) => {
                    let id = serde_json_id(&i.payload["input"]);
                    id == Some(e)
                        || serde_json_id(&i.payload["a"]) == Some(e)
                        || serde_json_id(&i.payload["b"]) == Some(e)
                }
                None => true,
            })
            .collect()
    }

    pub fn selected(&self) -> Option<&ReviewView> {
        self.visible().get(self.sel).copied()
    }

    pub fn count(&self, f: ReviewFilter) -> usize {
        self.items.iter().filter(|i| i.state == f.label()).count()
    }
}

/// An entity id stored in a JSON payload.
pub fn serde_json_id(v: &serde_json::Value) -> Option<EntityId> {
    v.as_str().and_then(|s| s.parse().ok())
}

/// The timeline tab.
#[derive(Debug, Clone, Default)]
pub struct TimelineState {
    pub events: Option<Vec<TimelineEvent>>,
    pub cursor: usize,
    /// `e` cycles through the event kinds (attribute names).
    pub kind: Option<String>,
}

impl TimelineState {
    pub fn kinds(&self) -> Vec<String> {
        let mut k: Vec<String> = self
            .events
            .iter()
            .flatten()
            .map(|e| e.attr.clone())
            .collect();
        k.sort();
        k.dedup();
        k
    }

    pub fn visible(&self) -> Vec<&TimelineEvent> {
        self.events
            .iter()
            .flatten()
            .filter(|e| self.kind.as_ref().is_none_or(|k| &e.attr == k))
            .collect()
    }
}

/// The path tab: from the focus to `target`.
#[derive(Debug, Clone, Default)]
pub struct PathState {
    pub from: Option<EntitySummary>,
    pub target: Option<EntitySummary>,
    /// `Some(None)`: no path within the hop limit.
    pub view: Option<Option<PathView>>,
    /// Index into the path's steps.
    pub cursor: usize,
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
    /// Encoded image previews (TUI_SPEC §9).
    pub images: crate::images::Images,
    /// Tutor progress, when this session is the tutor.
    pub tutor: Option<crate::tutor::Tutor>,
    /// Semantic events for the tutor, drained after each message.
    pub tutor_events: Vec<crate::tutor::Event>,
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
    /// The open screen (Checklist, Review) or tab (timeline, path).
    pub screen: bool,
}

impl Stale {
    pub fn any(&self) -> bool {
        self.status || self.entities || self.ego || self.detail || self.screen
    }

    pub fn all() -> Self {
        Self {
            status: true,
            entities: true,
            ego: true,
            detail: true,
            screen: true,
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
    /// The correlated edge under the cursor, compared in the inspector.
    pub inspect_edge: Option<EdgeId>,
    pub edge: Option<EdgeDetail>,
    pub insp_cursor: usize,
    pub insp_overlay: bool,

    pub view: View,
    pub checklist: ChecklistScreen,
    pub review: ReviewScreen,
    pub findings: FindingsScreen,
    pub timeline: TimelineState,
    pub path_tab: PathState,

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
    /// `playbook person-from-username · step 4/6` while a run is going.
    pub playbook_line: Option<String>,
    /// The running top-level playbook run; its end clears the line.
    pub top_run: Option<spektr_model::PlaybookRunId>,

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
            inspect_edge: None,
            edge: None,
            insp_cursor: 0,
            insp_overlay: false,
            view: View::Main,
            checklist: ChecklistScreen::default(),
            review: ReviewScreen::default(),
            findings: FindingsScreen::default(),
            timeline: TimelineState::default(),
            path_tab: PathState::default(),
            activity: VecDeque::new(),
            act_scroll: 0,
            past_jobs: None,
            last_run: HashMap::new(),
            running: 0,
            queued: 0,
            done: 0,
            failed: 0,
            playbook_line: None,
            top_run: None,
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

    /// The entity under the timeline cursor.
    pub fn timeline_entity(&self) -> Option<&EntitySummary> {
        self.timeline
            .visible()
            .get(self.timeline.cursor)
            .map(|e| &e.entity)
    }

    /// The entity actions apply to: the cursor in the focused pane.
    pub fn cursor_entity(&self) -> Option<&EntitySummary> {
        match self.pane {
            Pane::Graph if self.tab == Tab::Timeline => self.timeline_entity(),
            Pane::Graph if self.tab == Tab::Path => self
                .path_tab
                .view
                .as_ref()
                .and_then(|v| v.as_ref())
                .and_then(|v| v.steps.get(self.path_tab.cursor))
                .map(|s| &s.entity)
                .or(self.path_tab.target.as_ref()),
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
        if self.pane != Pane::Graph || self.tab != Tab::Ego || self.ego_col == Col::Card {
            return None;
        }
        let ego = self.ego.as_ref()?;
        crate::ego::row(ego, self.ego_item()?).and_then(|(_, r)| r.edges.first().copied())
    }

    /// The correlated edge under the cursor (a `≈` group row), which the
    /// inspector compares (TUI_SPEC §4.4).
    pub fn cursor_correlation(&self) -> Option<EdgeId> {
        if self.pane != Pane::Graph || self.tab != Tab::Ego || self.ego_col == Col::Card {
            return None;
        }
        let ego = self.ego.as_ref()?;
        let (g, r) = crate::ego::row(ego, self.ego_item()?)?;
        if g.key.is_correlated() {
            r.edges.first().copied()
        } else {
            None
        }
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
    Editor(Box<Editor>),
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
    RunPlaybook(String),
    /// Export the findings in a format (`tracelabs` copies to the
    /// clipboard; `md` and `json` ask for a path).
    Export {
        format: spektr_engine::export::Format,
        findings: Vec<i64>,
    },
    /// Open this URL (the entity's own, profile or source).
    Url(String),
    /// A launcher, optionally for a checklist item.
    Launch {
        launcher: String,
        item: Option<ChecklistItemId>,
    },
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
    Merge {
        into: EntityId,
    },
    /// Checklist: done with an optional note, skip with a reason, a note.
    ItemDone(ChecklistItemId),
    ItemSkip(ChecklistItemId),
    ItemNote(ChecklistItemId),
    StopRun,
    /// Review: accept a same-person item by merging `b` into `a`.
    ReviewMerge {
        id: ReviewItemId,
        a: EntityId,
        b: EntityId,
    },
    /// `:path` picker.
    PathTo,
    /// `:capture` / `c` with a URL to confirm or type.
    Capture(Option<EntityId>),
    /// `:attach` / `a`: a local path.
    Attach(Option<EntityId>),
    /// A new finding: category (a choice) and claim. From an entity, a
    /// checklist item (its note becomes the relevance) or the Findings
    /// screen.
    NewFinding {
        entities: Vec<EntityId>,
        relevance: String,
    },
    /// Attach an entity to a finding chosen in the dialog.
    AttachTo(EntityId),
    /// Attach evidence (an observation chosen in the dialog) to a finding.
    AttachEvidence(i64, Vec<spektr_model::ObservationId>),
    DeleteFinding(i64),
    /// Export to a file (md, json): the path is the field.
    ExportTo {
        format: spektr_engine::export::Format,
        findings: Vec<i64>,
    },
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
        let tutor = settings
            .tutor
            .map(|(lesson, step)| crate::tutor::Tutor { lesson, step });
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
            images: crate::images::Images::default(),
            tutor,
            tutor_events: Vec::new(),
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
            Some(Overlay::Editor(_)) => return Mode::Insert,
            Some(Overlay::Dialog(d)) if d.has_text_focus() => return Mode::Command,
            _ => {}
        }
        if let Some(w) = self.workspace() {
            if w.filter.as_ref().is_some_and(|f| f.editing) {
                return Mode::Filter;
            }
            match w.view {
                View::Checklist => return Mode::Checklist,
                View::Review => return Mode::Review,
                View::Findings => return Mode::Findings,
                View::Main => {}
            }
        }
        if self.tutor.is_some() {
            return Mode::Tutor;
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
            // The editor is a dialog over text areas.
            Some(Overlay::Editor(_)) => return vec![Ctx::Text, Ctx::Dialog],
            None => match &self.screen {
                Screen::Launcher(_) | Screen::Opening(_) => c.extend([Ctx::Launcher, Ctx::Nav]),
                Screen::Workspace(w) => match w.view {
                    View::Checklist => c.extend([Ctx::Checklist, Ctx::Nav]),
                    View::Review => c.extend([Ctx::Review, Ctx::Nav]),
                    View::Findings => c.extend([Ctx::Findings, Ctx::Nav]),
                    View::Main => {
                        if w.filter.as_ref().is_some_and(|f| f.editing) {
                            c.extend([Ctx::Text, Ctx::Filter]);
                        } else {
                            c.extend([Ctx::Nav, Ctx::Entity]);
                        }
                    }
                },
            },
        }
        c.push(Ctx::Global);
        c
    }
}
