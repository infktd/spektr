//! The action registry (TUI_SPEC §6). **Every key binding lives here.**
//!
//! Each entry has an action, default keys, a context, a description, a
//! status-line hint, and its palette command name. The help overlay, status
//! hints, menu hotkey labels and palette entries are all generated from this
//! table, and `[keys]` in config.toml can rebind actions.
//!
//! Context precedence, highest first: text input; overlays (palette, menu,
//! dialog, help); screens (launcher); navigation; entity actions; global. A
//! key in a higher context shadows the same key in a lower one; every such
//! shadowing must be listed in the binding's `overrides` (the conflict test
//! checks this). Text-input contexts capture every printable key and `⌫`;
//! only `esc`, `↵`, `tab`, `↑`, `↓` and `ctrl-` chords keep their meaning
//! there. No binding needs the Kitty keyboard protocol.

use std::collections::BTreeMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ctx {
    /// A focused text input (palette, filter, dialog fields). Has no
    /// bindings of its own: it only captures printable keys.
    Text,
    Palette,
    Menu,
    Dialog,
    Help,
    /// The entities-pane filter line.
    Filter,
    Launcher,
    Findings,
    Checklist,
    Review,
    Nav,
    Entity,
    Global,
}

impl Ctx {
    /// Lower is higher precedence (TUI_SPEC §6).
    pub fn rank(self) -> u8 {
        match self {
            Ctx::Text => 0,
            Ctx::Palette | Ctx::Menu | Ctx::Dialog | Ctx::Help | Ctx::Filter => 1,
            Ctx::Launcher | Ctx::Findings | Ctx::Checklist | Ctx::Review => 2,
            Ctx::Nav => 3,
            Ctx::Entity => 4,
            Ctx::Global => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ctx::Text => "Text input",
            Ctx::Palette => "Command palette",
            Ctx::Menu => "Context menu",
            Ctx::Dialog => "Dialogs",
            Ctx::Help => "Help",
            Ctx::Filter => "Filter",
            Ctx::Launcher => "Launcher",
            Ctx::Findings => "Findings",
            Ctx::Checklist => "Checklist",
            Ctx::Review => "Review",
            Ctx::Nav => "Navigation (lists and ego view)",
            Ctx::Entity => "Entity actions (on the cursor entity)",
            Ctx::Global => "Global",
        }
    }

    /// Help-overlay order.
    pub const ALL: [Ctx; 13] = [
        Ctx::Global,
        Ctx::Nav,
        Ctx::Entity,
        Ctx::Launcher,
        Ctx::Palette,
        Ctx::Menu,
        Ctx::Filter,
        Ctx::Dialog,
        Ctx::Help,
        Ctx::Findings,
        Ctx::Checklist,
        Ctx::Review,
        Ctx::Text,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    // global
    Quit,
    ForceQuit,
    Help,
    Palette,
    Filter,
    NextPane,
    PrevPane,
    TabEgo,
    TabTimeline,
    TabPath,
    Findings,
    Review,
    Checklist,
    Escape,
    // navigation
    Down,
    Up,
    Left,
    Right,
    Top,
    Bottom,
    HalfDown,
    HalfUp,
    PrevPage,
    NextPage,
    Activate,
    Back,
    EventFilter,
    // entity
    Menu,
    RunModule,
    Capture,
    NewFinding,
    Attach,
    Note,
    Copy,
    Confirm,
    Unconfirm,
    Reject,
    Merge,
    Unmerge,
    ScopeAdd,
    ScopeExclude,
    OpenEvidence,
    OpenUrl,
    ReviewUnsure,
    RunPlaybook,
    ToggleInspector,
    // launcher
    LauncherOpen,
    LauncherNew,
    LauncherOpenPath,
    LauncherVerify,
    // overlays
    Select,
    SelectUp,
    SelectDown,
    Complete,
    SubmenuOpen,
    SubmenuBack,
    RunAll,
    Yes,
    HelpSearch,
    // findings screen
    FindingNew,
    FindingEdit,
    FindingEditExternal,
    FindingAttach,
    FindingReady,
    FindingSent,
    FindingExport,
    FindingDelete,
    // checklist screen
    ItemDone,
    ItemSkip,
    ItemNote,
    ItemOpen,
    ItemFinding,
    PlaybookOnSeed,
    StopRun,
    SwitchPane,
    // review screen
    ReviewAccept,
    ReviewReject,
    ReviewLater,
}

impl Action {
    /// The name used in `[keys]` and shown in help.
    pub fn name(self) -> &'static str {
        BINDINGS
            .iter()
            .find(|b| b.action == self)
            .map_or("unknown", |b| b.name)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        BINDINGS.iter().find(|b| b.name == name).map(|b| b.action)
    }
}

/// One row of the registry.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub action: Action,
    /// Stable name for `[keys]` overrides.
    pub name: &'static str,
    pub keys: &'static [&'static str],
    pub ctx: Ctx,
    pub desc: &'static str,
    /// Short label for hints, footers and menu items; empty for none.
    pub hint: &'static str,
    /// Position in the status line's hints (1 first); 0 keeps it out.
    pub status: u8,
    /// Palette command name, if the action is also a command.
    pub palette: Option<&'static str>,
    /// Keys of lower-precedence contexts this binding deliberately shadows.
    pub overrides: &'static [&'static str],
}

const fn b(
    action: Action,
    name: &'static str,
    keys: &'static [&'static str],
    ctx: Ctx,
    desc: &'static str,
) -> Binding {
    Binding {
        action,
        name,
        keys,
        ctx,
        desc,
        hint: "",
        status: 0,
        palette: None,
        overrides: &[],
    }
}

impl Binding {
    /// A short label without a status-line slot.
    const fn label(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }

    /// A short label shown in the status line at position `rank`.
    const fn hint(mut self, hint: &'static str, rank: u8) -> Self {
        self.hint = hint;
        self.status = rank;
        self
    }

    const fn palette(mut self, cmd: &'static str) -> Self {
        self.palette = Some(cmd);
        self
    }

    const fn shadows(mut self, keys: &'static [&'static str]) -> Self {
        self.overrides = keys;
        self
    }
}

use Action as A;
use Ctx as C;

/// The default bindings (TUI_SPEC §6).
pub const BINDINGS: &[Binding] = &[
    // ---- global
    b(
        A::Quit,
        "quit",
        &["q"],
        C::Global,
        "Quit (asks if jobs are running)",
    )
    .label("quit"),
    b(
        A::ForceQuit,
        "force-quit",
        &["ctrl-c"],
        C::Global,
        "Force quit (press twice)",
    ),
    b(A::Help, "help", &["?"], C::Global, "Help overlay")
        .hint("help", 9)
        .palette("help"),
    b(A::Palette, "palette", &[":"], C::Global, "Command palette").hint("command", 1),
    b(
        A::Filter,
        "filter",
        &["/"],
        C::Global,
        "Filter the focused list",
    )
    .hint("filter", 2),
    b(A::NextPane, "next-pane", &["tab"], C::Global, "Next pane").hint("pane", 3),
    b(
        A::PrevPane,
        "prev-pane",
        &["shift-tab"],
        C::Global,
        "Previous pane",
    ),
    b(A::TabEgo, "tab-ego", &["1"], C::Global, "Graph tab: ego").label("ego"),
    b(
        A::TabTimeline,
        "tab-timeline",
        &["2"],
        C::Global,
        "Graph tab: timeline",
    )
    .label("timeline"),
    b(A::TabPath, "tab-path", &["3"], C::Global, "Graph tab: path").label("path"),
    b(A::Findings, "findings", &["F"], C::Global, "Findings").hint("findings", 4),
    b(
        A::Review,
        "review",
        &["R"],
        C::Global,
        "Review queue (pending count shown in the top bar)",
    )
    .label("review"),
    b(
        A::Checklist,
        "checklist",
        &["C"],
        C::Global,
        "Checklist and playbook runs",
    )
    .label("checklist")
    .palette("checklist"),
    b(
        A::Escape,
        "escape",
        &["esc"],
        C::Global,
        "Close the overlay, clear the filter, or go back to the workspace",
    )
    .label("close"),
    // ---- navigation
    b(A::Down, "down", &["j", "↓"], C::Nav, "Move the cursor down"),
    b(A::Up, "up", &["k", "↑"], C::Nav, "Move the cursor up"),
    b(
        A::Left,
        "left",
        &["h", "←"],
        C::Nav,
        "Move between ego columns (inbound · focus card · outbound)",
    ),
    b(
        A::Right,
        "right",
        &["l", "→"],
        C::Nav,
        "Move between ego columns",
    ),
    b(A::Top, "top", &["gg"], C::Nav, "Top"),
    b(A::Bottom, "bottom", &["G"], C::Nav, "Bottom"),
    b(
        A::HalfDown,
        "half-down",
        &["ctrl-d"],
        C::Nav,
        "Half page down",
    ),
    b(A::HalfUp, "half-up", &["ctrl-u"], C::Nav, "Half page up"),
    b(
        A::PrevPage,
        "prev-page",
        &["["],
        C::Nav,
        "Previous page of the current group",
    ),
    b(
        A::NextPage,
        "next-page",
        &["]"],
        C::Nav,
        "Next page of the current group",
    ),
    b(
        A::Activate,
        "activate",
        &["↵"],
        C::Nav,
        "Pivot to the cursor entity (ego), expand ··· more, or open the item",
    )
    .label("pivot"),
    b(A::Back, "back", &["⌫"], C::Nav, "Back in the pivot history").label("back"),
    b(
        A::EventFilter,
        "event-filter",
        &["e"],
        C::Nav,
        "Timeline: filter by event kind",
    ),
    // ---- entity actions
    b(A::Menu, "menu", &["space"], C::Entity, "Context menu").hint("menu", 5),
    b(
        A::RunModule,
        "run-module",
        &["r"],
        C::Entity,
        "Run module ▸ (A inside it runs all eligible)",
    )
    .label("Run module")
    .palette("run"),
    b(
        A::Capture,
        "capture",
        &["c"],
        C::Entity,
        "Capture (url, account, or image with a URL)",
    )
    .label("capture")
    .palette("capture"),
    b(
        A::NewFinding,
        "new-finding",
        &["f"],
        C::Entity,
        "New finding from this entity",
    )
    .label("new finding")
    .palette("finding new"),
    b(
        A::Attach,
        "attach",
        &["a"],
        C::Entity,
        "Attach the entity to the current finding (picker if none is open)",
    )
    .label("attach"),
    b(A::Note, "note", &["n"], C::Entity, "Add a note")
        .label("Add note")
        .palette("note"),
    b(
        A::Copy,
        "copy",
        &["y"],
        C::Entity,
        "Copy the value (OSC 52 clipboard)",
    )
    .label("copy"),
    b(
        A::Confirm,
        "confirm",
        &["v"],
        C::Entity,
        "Confirm the edge under the cursor (strength → confirmed)",
    )
    .label("confirm")
    .palette("confirm"),
    b(
        A::Unconfirm,
        "unconfirm",
        &["V"],
        C::Entity,
        "Un-confirm the edge under the cursor",
    )
    .label("unconfirm"),
    b(
        A::Reject,
        "reject",
        &["x"],
        C::Entity,
        "Reject the edge under the cursor (asks first)",
    )
    .label("reject")
    .palette("reject"),
    b(A::Merge, "merge", &["m"], C::Entity, "Merge with… (picker)")
        .label("Merge with…")
        .palette("merge"),
    b(A::Unmerge, "unmerge", &["M"], C::Entity, "Unmerge")
        .label("Unmerge")
        .palette("unmerge"),
    b(A::ScopeAdd, "scope-add", &["s"], C::Entity, "Add to scope").label("Add to scope"),
    b(
        A::ScopeExclude,
        "scope-exclude",
        &["S"],
        C::Entity,
        "Exclude from scope",
    )
    .label("Exclude from scope"),
    b(
        A::OpenEvidence,
        "open-evidence",
        &["o"],
        C::Entity,
        "Open evidence: raw response in $PAGER, a capture or image in the system viewer",
    )
    .label("open"),
    b(
        A::OpenUrl,
        "open-url",
        &["O"],
        C::Entity,
        "Open the URL in the browser",
    )
    .label("open url"),
    b(
        A::ReviewUnsure,
        "review-unsure",
        &["u"],
        C::Entity,
        "Review the username-sweep's unsure hits for this entity",
    )
    .label("review unsure"),
    b(
        A::RunPlaybook,
        "run-playbook",
        &["p"],
        C::Entity,
        "Run playbook ▸ (playbooks whose seed matches)",
    )
    .label("Run playbook"),
    b(
        A::ToggleInspector,
        "inspector",
        &["i"],
        C::Entity,
        "Toggle the inspector overlay (narrow terminals only)",
    )
    .label("inspector"),
    // ---- launcher
    b(
        A::LauncherOpen,
        "open",
        &["↵"],
        C::Launcher,
        "Open the selected case",
    )
    .hint("open", 1)
    .shadows(&["↵"]),
    b(A::LauncherNew, "new-case", &["n"], C::Launcher, "New case")
        .hint("new case", 2)
        .shadows(&["n"]),
    b(
        A::LauncherOpenPath,
        "open-path",
        &["o"],
        C::Launcher,
        "Open a case by path",
    )
    .hint("open path", 3)
    .shadows(&["o"]),
    b(
        A::LauncherVerify,
        "verify",
        &["v"],
        C::Launcher,
        "Verify the selected case's chain",
    )
    .hint("verify chain", 4)
    .palette("verify")
    .shadows(&["v"]),
    // ---- palette
    b(
        A::Complete,
        "palette-complete",
        &["tab"],
        C::Palette,
        "Complete",
    )
    .hint("complete", 1)
    .shadows(&["tab"]),
    b(A::SelectUp, "palette-up", &["↑"], C::Palette, "Select up")
        .hint("select", 2)
        .shadows(&["↑"]),
    b(
        A::SelectDown,
        "palette-down",
        &["↓"],
        C::Palette,
        "Select down",
    )
    .shadows(&["↓"]),
    b(
        A::Select,
        "palette-run",
        &["↵"],
        C::Palette,
        "Run the selected command",
    )
    .hint("run", 3)
    .shadows(&["↵"]),
    b(A::Escape, "palette-close", &["esc"], C::Palette, "Close")
        .hint("close", 4)
        .shadows(&["esc"]),
    // ---- context menu (TUI_SPEC §7; entity hotkeys keep working)
    b(A::SelectUp, "menu-up", &["↑"], C::Menu, "Select up")
        .hint("select", 1)
        .shadows(&["↑"]),
    b(A::SelectDown, "menu-down", &["↓"], C::Menu, "Select down").shadows(&["↓"]),
    b(
        A::SubmenuOpen,
        "submenu-open",
        &["→"],
        C::Menu,
        "Open the submenu",
    )
    .hint("open submenu", 2)
    .shadows(&["→"]),
    b(
        A::SubmenuBack,
        "submenu-back",
        &["←"],
        C::Menu,
        "Back to the parent menu",
    )
    .hint("back", 3)
    .shadows(&["←"]),
    b(
        A::Select,
        "menu-run",
        &["↵"],
        C::Menu,
        "Run the selected item",
    )
    .hint("run", 4)
    .shadows(&["↵"]),
    b(
        A::RunAll,
        "run-all",
        &["A"],
        C::Menu,
        "Run all eligible (inside Run module ▸)",
    )
    .label("Run all eligible"),
    b(A::Escape, "menu-close", &["esc"], C::Menu, "Close")
        .hint("close", 5)
        .shadows(&["esc"]),
    // ---- filter line
    b(
        A::SelectUp,
        "filter-up",
        &["↑"],
        C::Filter,
        "Previous match",
    )
    .hint("select", 1)
    .shadows(&["↑"]),
    b(
        A::SelectDown,
        "filter-down",
        &["↓"],
        C::Filter,
        "Next match",
    )
    .shadows(&["↓"]),
    b(
        A::Select,
        "filter-accept",
        &["↵"],
        C::Filter,
        "Keep the filter and return to the list",
    )
    .hint("accept", 2)
    .shadows(&["↵"]),
    b(
        A::Escape,
        "filter-clear",
        &["esc"],
        C::Filter,
        "Clear the filter",
    )
    .hint("clear", 3)
    .shadows(&["esc"]),
    // ---- dialogs
    b(A::Select, "dialog-ok", &["↵"], C::Dialog, "Confirm")
        .hint("confirm", 1)
        .shadows(&["↵"]),
    b(
        A::Yes,
        "dialog-yes",
        &["y"],
        C::Dialog,
        "Yes (destructive confirmations)",
    )
    .shadows(&["y"]),
    b(A::Escape, "dialog-cancel", &["esc"], C::Dialog, "Cancel")
        .hint("cancel", 3)
        .shadows(&["esc"]),
    b(
        A::SelectUp,
        "dialog-up",
        &["↑"],
        C::Dialog,
        "Previous field, or the previous pick",
    )
    .shadows(&["↑"]),
    b(
        A::SelectDown,
        "dialog-down",
        &["↓"],
        C::Dialog,
        "Next field, or the next pick",
    )
    .hint("field", 2)
    .shadows(&["↓"]),
    b(
        A::Complete,
        "dialog-next",
        &["tab"],
        C::Dialog,
        "Next field",
    )
    .shadows(&["tab"]),
    b(
        A::SubmenuBack,
        "dialog-prev-choice",
        &["←"],
        C::Dialog,
        "Previous choice in a choice field",
    )
    .shadows(&["←"]),
    b(
        A::SubmenuOpen,
        "dialog-next-choice",
        &["→"],
        C::Dialog,
        "Next choice in a choice field",
    )
    .shadows(&["→"]),
    // ---- help
    b(
        A::HelpSearch,
        "help-search",
        &["/"],
        C::Help,
        "Search the help",
    )
    .hint("search", 1)
    .shadows(&["/"]),
    b(A::Escape, "help-close", &["esc"], C::Help, "Close")
        .hint("close", 3)
        .shadows(&["esc"]),
    // ---- findings screen (Phase 5)
    b(
        A::FindingNew,
        "finding-new",
        &["n"],
        C::Findings,
        "New finding",
    )
    .shadows(&["n"]),
    b(
        A::FindingEdit,
        "finding-edit",
        &["e"],
        C::Findings,
        "Edit inline",
    )
    .shadows(&["e"]),
    b(
        A::FindingEditExternal,
        "finding-edit-external",
        &["E"],
        C::Findings,
        "Edit in $EDITOR",
    ),
    b(
        A::FindingAttach,
        "finding-attach",
        &["a"],
        C::Findings,
        "Attach evidence",
    )
    .shadows(&["a"]),
    b(
        A::FindingReady,
        "finding-ready",
        &["r"],
        C::Findings,
        "Mark ready",
    )
    .shadows(&["r"]),
    b(
        A::FindingSent,
        "finding-sent",
        &["s"],
        C::Findings,
        "Mark sent",
    )
    .shadows(&["s"]),
    b(
        A::FindingExport,
        "finding-export",
        &["x"],
        C::Findings,
        "Export ▸ (submission text to clipboard, Markdown, JSON)",
    )
    .shadows(&["x"]),
    b(
        A::Activate,
        "finding-go",
        &["↵"],
        C::Findings,
        "On a supporting-entity row: go to that entity in the workspace",
    )
    .shadows(&["↵"]),
    b(
        A::FindingDelete,
        "finding-delete",
        &["d"],
        C::Findings,
        "Delete (asks first)",
    ),
    // ---- checklist screen (Phase 4)
    b(
        A::ItemDone,
        "item-done",
        &["d"],
        C::Checklist,
        "Mark the item done (prompts for an optional note)",
    ),
    b(
        A::ItemSkip,
        "item-skip",
        &["s"],
        C::Checklist,
        "Skip the item (prompts for a reason)",
    )
    .shadows(&["s"]),
    b(
        A::ItemNote,
        "item-note",
        &["n"],
        C::Checklist,
        "Add or edit the note",
    )
    .shadows(&["n"]),
    b(
        A::ItemOpen,
        "item-open",
        &["o"],
        C::Checklist,
        "Open a launcher ▸",
    )
    .shadows(&["o"]),
    b(
        A::ItemFinding,
        "item-finding",
        &["f"],
        C::Checklist,
        "New finding from this item",
    )
    .shadows(&["f"]),
    b(
        A::Activate,
        "item-go",
        &["↵"],
        C::Checklist,
        "Go to the item's entity, or open its screen",
    )
    .shadows(&["↵"]),
    b(
        A::PlaybookOnSeed,
        "item-playbook",
        &["p"],
        C::Checklist,
        "Run a playbook on the selected run's seed",
    )
    .shadows(&["p"]),
    b(
        A::StopRun,
        "stop-run",
        &["X"],
        C::Checklist,
        "Stop the selected run (asks first)",
    ),
    b(
        A::SwitchPane,
        "checklist-pane",
        &["tab"],
        C::Checklist,
        "Switch between the runs and checklist panes",
    )
    .shadows(&["tab"]),
    // ---- review screen (Phase 4)
    b(
        A::ReviewAccept,
        "review-accept",
        &["a"],
        C::Review,
        "Accept (a sweep hit becomes an account; same-person opens the merge confirmation)",
    )
    .shadows(&["a"]),
    b(
        A::ReviewReject,
        "review-reject",
        &["x"],
        C::Review,
        "Reject",
    )
    .shadows(&["x"]),
    b(A::ReviewLater, "review-later", &["s"], C::Review, "Later").shadows(&["s"]),
    b(
        A::OpenUrl,
        "review-open",
        &["O"],
        C::Review,
        "Open in the browser",
    )
    .shadows(&["O"]),
];

/// One key press, normalized: shifted letters are their uppercase char with
/// no SHIFT modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyPress {
    pub code: KeyCodeOrd,
    pub ctrl: bool,
    pub alt: bool,
}

/// `KeyCode` subset that's orderable and hashable for our tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCodeOrd {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Delete,
}

impl KeyPress {
    pub fn from_event(e: &KeyEvent) -> Option<Self> {
        if e.kind == KeyEventKind::Release {
            return None;
        }
        let code = match e.code {
            KeyCode::Char(c) => KeyCodeOrd::Char(c),
            KeyCode::Enter => KeyCodeOrd::Enter,
            KeyCode::Esc => KeyCodeOrd::Esc,
            KeyCode::Tab if e.modifiers.contains(KeyModifiers::SHIFT) => KeyCodeOrd::BackTab,
            KeyCode::Tab => KeyCodeOrd::Tab,
            KeyCode::BackTab => KeyCodeOrd::BackTab,
            KeyCode::Backspace => KeyCodeOrd::Backspace,
            KeyCode::Up => KeyCodeOrd::Up,
            KeyCode::Down => KeyCodeOrd::Down,
            KeyCode::Left => KeyCodeOrd::Left,
            KeyCode::Right => KeyCodeOrd::Right,
            KeyCode::PageUp => KeyCodeOrd::PageUp,
            KeyCode::PageDown => KeyCodeOrd::PageDown,
            KeyCode::Home => KeyCodeOrd::Home,
            KeyCode::End => KeyCodeOrd::End,
            KeyCode::Delete => KeyCodeOrd::Delete,
            _ => return None,
        };
        let ctrl = e.modifiers.contains(KeyModifiers::CONTROL);
        let alt = e.modifiers.contains(KeyModifiers::ALT);
        // Terminals report ctrl-letter as a lowercase char with CONTROL.
        let code = match code {
            KeyCodeOrd::Char(c) if ctrl => KeyCodeOrd::Char(c.to_ascii_lowercase()),
            other => other,
        };
        Some(Self { code, ctrl, alt })
    }

    pub fn char(c: char) -> Self {
        Self {
            code: KeyCodeOrd::Char(c),
            ctrl: false,
            alt: false,
        }
    }

    /// Typed into a text input (printable, no ctrl/alt).
    pub fn printable(&self) -> Option<char> {
        match self.code {
            KeyCodeOrd::Char(c) if !self.ctrl && !self.alt => Some(c),
            _ => None,
        }
    }

    /// Keys that keep their meaning inside text inputs.
    pub fn passes_text_capture(&self) -> bool {
        self.ctrl
            || matches!(
                self.code,
                KeyCodeOrd::Esc
                    | KeyCodeOrd::Enter
                    | KeyCodeOrd::Tab
                    | KeyCodeOrd::Up
                    | KeyCodeOrd::Down
            )
    }
}

/// Parse a key spec: `q`, `G`, `gg` (sequence), `ctrl-d`, `shift-tab`, `↵`,
/// `esc`, `tab`, `⌫`, `space`, `↑ ↓ ← →`, `enter`, `up`, …
pub fn parse_key(s: &str) -> Option<Vec<KeyPress>> {
    let named = |n: &str| -> Option<KeyCodeOrd> {
        Some(match n {
            "↵" | "enter" => KeyCodeOrd::Enter,
            "esc" => KeyCodeOrd::Esc,
            "tab" => KeyCodeOrd::Tab,
            "shift-tab" => KeyCodeOrd::BackTab,
            "⌫" | "backspace" => KeyCodeOrd::Backspace,
            "space" => KeyCodeOrd::Char(' '),
            "↑" | "up" => KeyCodeOrd::Up,
            "↓" | "down" => KeyCodeOrd::Down,
            "←" | "left" => KeyCodeOrd::Left,
            "→" | "right" => KeyCodeOrd::Right,
            "pgup" => KeyCodeOrd::PageUp,
            "pgdn" => KeyCodeOrd::PageDown,
            "home" => KeyCodeOrd::Home,
            "end" => KeyCodeOrd::End,
            "del" => KeyCodeOrd::Delete,
            _ => return None,
        })
    };
    if let Some(code) = named(s) {
        return Some(vec![KeyPress {
            code,
            ctrl: false,
            alt: false,
        }]);
    }
    if let Some(rest) = s.strip_prefix("ctrl-") {
        let mut chars = rest.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        return Some(vec![KeyPress {
            code: KeyCodeOrd::Char(c.to_ascii_lowercase()),
            ctrl: true,
            alt: false,
        }]);
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() || chars.len() > 2 {
        return None;
    }
    Some(chars.into_iter().map(KeyPress::char).collect())
}

impl fmt::Display for KeyPress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("ctrl-")?;
        }
        match self.code {
            KeyCodeOrd::Char(' ') => f.write_str("space"),
            KeyCodeOrd::Char(c) => write!(f, "{c}"),
            KeyCodeOrd::Enter => f.write_str("↵"),
            KeyCodeOrd::Esc => f.write_str("esc"),
            KeyCodeOrd::Tab => f.write_str("tab"),
            KeyCodeOrd::BackTab => f.write_str("shift-tab"),
            KeyCodeOrd::Backspace => f.write_str("⌫"),
            KeyCodeOrd::Up => f.write_str("↑"),
            KeyCodeOrd::Down => f.write_str("↓"),
            KeyCodeOrd::Left => f.write_str("←"),
            KeyCodeOrd::Right => f.write_str("→"),
            KeyCodeOrd::PageUp => f.write_str("pgup"),
            KeyCodeOrd::PageDown => f.write_str("pgdn"),
            KeyCodeOrd::Home => f.write_str("home"),
            KeyCodeOrd::End => f.write_str("end"),
            KeyCodeOrd::Delete => f.write_str("del"),
        }
    }
}

/// A binding with its (possibly overridden) parsed keys.
#[derive(Debug, Clone)]
pub struct Bound {
    pub binding: Binding,
    pub keys: Vec<(String, Vec<KeyPress>)>,
}

/// What a key sequence resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Action(Action),
    /// A prefix of a longer binding (e.g. the first `g` of `gg`).
    Pending,
    None,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    pub bound: Vec<Bound>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::with_overrides(&BTreeMap::new()).0
    }
}

impl Keymap {
    /// The defaults with `[keys]` overrides applied. An override that names
    /// an unknown action or key, or that would collide in its context, is
    /// skipped with a warning.
    pub fn with_overrides(overrides: &BTreeMap<String, Vec<String>>) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut bound: Vec<Bound> = BINDINGS
            .iter()
            .map(|b| Bound {
                binding: *b,
                keys: b
                    .keys
                    .iter()
                    .filter_map(|k| parse_key(k).map(|p| (k.to_string(), p)))
                    .collect(),
            })
            .collect();
        for (name, keys) in overrides {
            let Some(idx) = bound.iter().position(|b| b.binding.name == name) else {
                warnings.push(format!("[keys] unknown action `{name}`"));
                continue;
            };
            let mut parsed = Vec::new();
            for k in keys {
                match parse_key(k) {
                    Some(p) => parsed.push((k.clone(), p)),
                    None => warnings.push(format!("[keys] {name}: can't parse key `{k}`")),
                }
            }
            if parsed.is_empty() {
                continue;
            }
            let ctx = bound[idx].binding.ctx;
            let clash = bound.iter().enumerate().any(|(i, b)| {
                i != idx
                    && b.binding.ctx == ctx
                    && b.keys
                        .iter()
                        .any(|(_, kp)| parsed.iter().any(|(_, p)| p == kp))
            });
            if clash {
                warnings.push(format!("[keys] {name}: the new key is already bound in that context; keeping the default"));
                continue;
            }
            bound[idx].keys = parsed;
        }
        (Self { bound }, warnings)
    }

    /// Resolve a key sequence against the active contexts, highest
    /// precedence first. In a text context only esc, ↵, tab, ↑, ↓ and ctrl
    /// chords are looked up at all.
    pub fn resolve(&self, seq: &[KeyPress], active: &[Ctx]) -> Resolved {
        let Some(last) = seq.last() else {
            return Resolved::None;
        };
        if active.contains(&Ctx::Text) && !last.passes_text_capture() {
            return Resolved::None;
        }
        let mut ctxs: Vec<Ctx> = active.to_vec();
        ctxs.sort_by_key(|c| c.rank());
        let mut pending = false;
        for ctx in ctxs {
            for b in self.bound.iter().filter(|b| b.binding.ctx == ctx) {
                for (_, keys) in &b.keys {
                    if keys.as_slice() == seq {
                        return Resolved::Action(b.binding.action);
                    }
                    if keys.len() > seq.len() && keys.starts_with(seq) {
                        pending = true;
                    }
                }
            }
        }
        if pending {
            Resolved::Pending
        } else {
            Resolved::None
        }
    }

    /// The first key bound to `action` in `ctx` (for menus and hints).
    pub fn key_for(&self, action: Action, ctx: Ctx) -> Option<&str> {
        self.bound
            .iter()
            .find(|b| b.binding.action == action && b.binding.ctx == ctx)
            .and_then(|b| b.keys.first())
            .map(|(s, _)| s.as_str())
    }

    /// `(key label, short label)` for an action in a context, for footers
    /// and menu hotkeys.
    pub fn hint_for(&self, action: Action, ctx: Ctx) -> Option<(String, &'static str)> {
        self.bound
            .iter()
            .find(|b| b.binding.action == action && b.binding.ctx == ctx)
            .and_then(|b| b.keys.first().map(|(k, _)| (k.clone(), b.binding.hint)))
    }

    /// Status-line hints from `ctxs`, ordered by their status rank:
    /// `(key, hint)`, at most 7. `↑` and `↓` show as one `↑↓` hint.
    pub fn hints(&self, ctxs: &[Ctx]) -> Vec<(String, &'static str)> {
        let mut rows: Vec<(u8, u8, String, &'static str)> = self
            .bound
            .iter()
            .filter(|b| ctxs.contains(&b.binding.ctx) && b.binding.status > 0)
            .filter_map(|b| {
                let key = if b.binding.action == Action::SelectUp {
                    "↑↓".to_string()
                } else {
                    b.keys.first()?.0.clone()
                };
                Some((b.binding.status, b.binding.ctx.rank(), key, b.binding.hint))
            })
            .collect();
        rows.sort_by_key(|r| (r.0, r.1));
        let mut seen = std::collections::BTreeSet::new();
        rows.into_iter()
            .filter(|r| seen.insert(r.3))
            .take(7)
            .map(|(_, _, k, h)| (k, h))
            .collect()
    }

    /// Every binding, grouped by context, for the help overlay.
    pub fn help(&self) -> Vec<(Ctx, Vec<(String, &'static str)>)> {
        Ctx::ALL
            .iter()
            .filter(|c| **c != Ctx::Text)
            .map(|ctx| {
                let rows = self
                    .bound
                    .iter()
                    .filter(|b| b.binding.ctx == *ctx)
                    .map(|b| {
                        (
                            b.keys
                                .iter()
                                .map(|(k, _)| k.as_str())
                                .collect::<Vec<_>>()
                                .join(" "),
                            b.binding.desc,
                        )
                    })
                    .collect();
                (*ctx, rows)
            })
            .collect()
    }

    /// Problems with the table: a key bound twice in one context, an
    /// unlisted override of a lower context, or a printable key bound in
    /// the text context. Empty for a valid keymap.
    pub fn conflicts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ctx in Ctx::ALL {
            let mut seen: BTreeMap<Vec<KeyPress>, &str> = BTreeMap::new();
            for b in self.bound.iter().filter(|b| b.binding.ctx == ctx) {
                for (label, keys) in &b.keys {
                    if let Some(prev) = seen.insert(keys.clone(), b.binding.name) {
                        out.push(format!(
                            "{}: `{label}` bound to both {prev} and {}",
                            ctx.label(),
                            b.binding.name
                        ));
                    }
                }
            }
        }
        for b in &self.bound {
            for (label, keys) in &b.keys {
                let lower = self.bound.iter().any(|o| {
                    o.binding.ctx.rank() > b.binding.ctx.rank()
                        && o.keys.iter().any(|(_, k)| k == keys)
                });
                if lower && !b.binding.overrides.contains(&label.as_str()) {
                    out.push(format!("{}: `{label}` ({}) shadows a lower context without listing it in overrides", b.binding.ctx.label(), b.binding.name));
                }
                if b.binding.ctx == Ctx::Text && keys.iter().any(|k| !k.passes_text_capture()) {
                    out.push(format!("text input binds printable `{label}`"));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Vec<KeyPress> {
        parse_key(s).unwrap()
    }

    /// The conflict test (TUI_SPEC §6): fails the build if a key is bound
    /// twice in one context, or an override isn't declared.
    #[test]
    fn no_conflicts() {
        let km = Keymap::default();
        let c = km.conflicts();
        assert!(c.is_empty(), "keymap conflicts:\n{}", c.join("\n"));
    }

    #[test]
    fn every_key_parses_and_no_modified_enter() {
        for b in BINDINGS {
            for key in b.keys {
                assert!(parse_key(key).is_some(), "{}: can't parse `{key}`", b.name);
                assert!(
                    !key.contains("shift-↵") && !key.contains("ctrl-↵"),
                    "{}: modified Enter needs the Kitty protocol",
                    b.name
                );
            }
        }
        let names: std::collections::BTreeSet<_> = BINDINGS.iter().map(|b| b.name).collect();
        assert_eq!(names.len(), BINDINGS.len(), "binding names are unique");
    }

    #[test]
    fn precedence_and_sequences() {
        let km = Keymap::default();
        let ws = [Ctx::Nav, Ctx::Entity, Ctx::Global];
        assert_eq!(km.resolve(&k("j"), &ws), Resolved::Action(Action::Down));
        assert_eq!(km.resolve(&k("g"), &ws), Resolved::Pending);
        assert_eq!(km.resolve(&k("gg"), &ws), Resolved::Action(Action::Top));
        assert_eq!(km.resolve(&k("n"), &ws), Resolved::Action(Action::Note));
        let launcher = [Ctx::Launcher, Ctx::Nav, Ctx::Global];
        assert_eq!(
            km.resolve(&k("n"), &launcher),
            Resolved::Action(Action::LauncherNew)
        );
        let menu = [Ctx::Menu, Ctx::Entity, Ctx::Global];
        assert_eq!(
            km.resolve(&k("↓"), &menu),
            Resolved::Action(Action::SelectDown)
        );
        assert_eq!(
            km.resolve(&k("j"), &menu),
            Resolved::None,
            "the menu doesn't move the list underneath"
        );
        assert_eq!(
            km.resolve(&k("y"), &menu),
            Resolved::Action(Action::Copy),
            "hotkeys work while the menu is open"
        );
    }

    /// Text-input contexts capture printable keys (TUI_SPEC §6).
    #[test]
    fn text_input_captures_printable_keys() {
        let km = Keymap::default();
        let palette = [Ctx::Text, Ctx::Palette, Ctx::Global];
        for key in [
            "q", "?", ":", "/", "F", "R", "C", "1", "2", "3", "j", "k", "g", "space", "⌫",
        ] {
            assert_eq!(
                km.resolve(&k(key), &palette),
                Resolved::None,
                "`{key}` must type, not act"
            );
        }
        assert_eq!(
            km.resolve(&k("esc"), &palette),
            Resolved::Action(Action::Escape)
        );
        assert_eq!(
            km.resolve(&k("↵"), &palette),
            Resolved::Action(Action::Select)
        );
        assert_eq!(
            km.resolve(&k("tab"), &palette),
            Resolved::Action(Action::Complete)
        );
        assert_eq!(
            km.resolve(&k("ctrl-c"), &palette),
            Resolved::Action(Action::ForceQuit)
        );
    }

    #[test]
    fn overrides() {
        let mut o = BTreeMap::new();
        o.insert("quit".to_string(), vec!["Q".to_string()]);
        o.insert("bogus".to_string(), vec!["z".to_string()]);
        o.insert("help".to_string(), vec![":".to_string()]); // clashes with palette
        let (km, w) = Keymap::with_overrides(&o);
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(
            km.resolve(&k("Q"), &[Ctx::Global]),
            Resolved::Action(Action::Quit)
        );
        assert_eq!(km.resolve(&k("q"), &[Ctx::Global]), Resolved::None);
        assert_eq!(
            km.resolve(&k("?"), &[Ctx::Global]),
            Resolved::Action(Action::Help)
        );
    }

    #[test]
    fn hints_are_generated() {
        let km = Keymap::default();
        let h: Vec<String> = km
            .hints(&[Ctx::Entity, Ctx::Global])
            .into_iter()
            .map(|(k, h)| format!("{k} {h}"))
            .collect();
        // The order drawn in artboard 08's status line.
        assert_eq!(
            h,
            [
                ": command",
                "/ filter",
                "tab pane",
                "F findings",
                "space menu",
                "? help"
            ]
        );
        let m: Vec<String> = km
            .hints(&[Ctx::Menu])
            .into_iter()
            .map(|(k, h)| format!("{k} {h}"))
            .collect();
        assert_eq!(
            m,
            [
                "↑↓ select",
                "→ open submenu",
                "← back",
                "↵ run",
                "esc close"
            ]
        );
        let p: Vec<String> = km
            .hints(&[Ctx::Palette])
            .into_iter()
            .map(|(k, h)| format!("{k} {h}"))
            .collect();
        assert_eq!(p, ["tab complete", "↑↓ select", "↵ run", "esc close"]);
    }
}
