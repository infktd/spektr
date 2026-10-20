//! The tutor (TUI_SPEC §10): ten lessons on a bundled case, offline. Each
//! step completes on a semantic event (a pivot, a module run, a confirmed
//! edge…), never on a keystroke. A step's `keys` are what a person would
//! press; `e2e_tutor` drives the whole tutor with them.

use spektr_model::CoreType;

use crate::state::{AppState, Overlay, Pane, Tab, View, Workspace};

/// What happened, as far as the tutor cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Pane(Pane),
    HelpOpened,
    Seeded(String),
    /// The case has this many accounts.
    Accounts(usize),
    PlaybookStarted(String),
    PlaybookFinished(String),
    /// The focus moved to an entity of this type.
    Pivot(CoreType),
    Back,
    /// The inspector shows this entity (its key).
    Inspect(String),
    /// The inspector compares an avatar match.
    AvatarEdge,
    EdgeConfirmed,
    Tab(Tab),
    /// The timeline cursor is on activity after the last-seen date.
    LateActivity,
    Captured,
    LauncherOpened,
    ItemResolved,
    FindingCreated,
    /// The open finding passes its checks, with a claim and a relevance.
    FindingPasses,
    FindingReady,
    Exported,
}

/// What completes a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    Pane(Pane),
    Help,
    Seeded(&'static str),
    Accounts(usize),
    Started(&'static str),
    Finished(&'static str),
    Pivot(CoreType),
    Back,
    Inspect(&'static str),
    AvatarEdge,
    EdgeConfirmed,
    Tab(Tab),
    LateActivity,
    Captured,
    LauncherOpened,
    ItemResolved,
    FindingCreated,
    FindingPasses,
    FindingReady,
    Exported,
}

impl Want {
    fn met(self, e: &Event) -> bool {
        match (self, e) {
            (Want::Pane(p), Event::Pane(q)) => p == *q,
            (Want::Help, Event::HelpOpened) => true,
            (Want::Seeded(v), Event::Seeded(x)) => x == v,
            (Want::Accounts(n), Event::Accounts(m)) => *m >= n,
            (Want::Started(p), Event::PlaybookStarted(x)) => x == p,
            (Want::Finished(p), Event::PlaybookFinished(x)) => x == p,
            (Want::Pivot(t), Event::Pivot(u)) => t == *u,
            (Want::Back, Event::Back) => true,
            (Want::Inspect(k), Event::Inspect(x)) => x == k,
            (Want::AvatarEdge, Event::AvatarEdge) => true,
            (Want::EdgeConfirmed, Event::EdgeConfirmed) => true,
            (Want::Tab(t), Event::Tab(u)) => t == *u,
            (Want::LateActivity, Event::LateActivity) => true,
            (Want::Captured, Event::Captured) => true,
            (Want::LauncherOpened, Event::LauncherOpened) => true,
            (Want::ItemResolved, Event::ItemResolved) => true,
            (Want::FindingCreated, Event::FindingCreated) => true,
            (Want::FindingPasses, Event::FindingPasses) => true,
            (Want::FindingReady, Event::FindingReady) => true,
            (Want::Exported, Event::Exported) => true,
            _ => false,
        }
    }
}

pub struct Step {
    /// What to do, in a sentence.
    pub goal: &'static str,
    /// The keys, in the harness key DSL (`text:`, `space`, `↵`…); `~` is a
    /// pause for something loading, and after `esc` (an escape followed at
    /// once by a key reads as Alt-<key>). `e2e_tutor` waits there.
    pub keys: &'static str,
    pub want: Want,
}

pub struct Lesson {
    pub title: &'static str,
    pub steps: &'static [Step],
}

const fn s(goal: &'static str, keys: &'static str, want: Want) -> Step {
    Step { goal, keys, want }
}

/// The lessons (about 15 minutes).
pub static LESSONS: &[Lesson] = &[
    Lesson {
        title: "Move and focus",
        steps: &[
            s(
                "tab moves the focus between the panes: focus the Inspector on the right.",
                "tab",
                Want::Pane(Pane::Inspector),
            ),
            s(
                "? lists every key, by context. Open it, then esc closes it.",
                "? ~ esc",
                Want::Help,
            ),
        ],
    },
    Lesson {
        title: "Seed a username",
        steps: &[
            s(
                "Jordan Marlow went missing; their handle is jmarlow.wav. Press : and run  seed username:jmarlow.wav",
                ": text:seed space text:username:jmarlow.wav ↵",
                Want::Seeded("jmarlow.wav"),
            ),
            s(
                "The username sweep runs on its own: watch the accounts stream into the graph.",
                "",
                Want::Accounts(3),
            ),
        ],
    },
    Lesson {
        title: "Run a playbook",
        steps: &[
            s(
                "/ filters the entities: type jmarlow.wav, ↵. Then p opens Run playbook ▸ and ↵ runs person-from-username.",
                "/ text:jmarlow.wav ↵ ~ p ↵",
                Want::Started("person-from-username"),
            ),
            s(
                "The rail at the bottom shows each step. Wait for the playbook to finish.",
                "",
                Want::Finished("person-from-username"),
            ),
        ],
    },
    Lesson {
        title: "Pivot and go back",
        steps: &[
            s(
                "esc clears the filter; tab to the graph; l l moves to the accounts on the right; ↵ pivots to one.",
                "esc ~ tab l l ↵",
                Want::Pivot(CoreType::Account),
            ),
            s("⌫ goes back to the username.", "⌫", Want::Back),
        ],
    },
    Lesson {
        title: "Read strength",
        steps: &[s(
            "▰▱▱ one source · ▰▰▱ corroborated · ▰▰▰ confirmed by you. / find jmarlow-wav, ↵, and read its strength in the inspector.",
            "~ / text:jmarlow-wav ↵",
            Want::Inspect("github:jmarlow-wav"),
        )],
    },
    Lesson {
        title: "Review an avatar match",
        steps: &[
            s(
                "esc; tab to the graph; h h moves to the left column, j into ≈ same_avatar: the inspector puts the avatars side by side.",
                "esc ~ tab h h ~ j",
                Want::AvatarEdge,
            ),
            s(
                "They match (dHash distance 0/64). v confirms it: strength becomes ▰▰▰.",
                "v",
                Want::EdgeConfirmed,
            ),
        ],
    },
    Lesson {
        title: "Timeline",
        steps: &[
            s(
                "2 opens the timeline: every dated fact, with a line at the last-seen date.",
                "2",
                Want::Tab(Tab::Timeline),
            ),
            s(
                "Rows marked ▲ are activity after the subject was last seen. Put the cursor on one (G jumps to the end, k moves up).",
                "~ G k k",
                Want::LateActivity,
            ),
        ],
    },
    Lesson {
        title: "Capture and the checklist",
        steps: &[
            s(
                "1 back to the graph; / find jmarlow-wav, ↵; c captures the GitHub profile, ↵ to confirm.",
                "1 / text:jmarlow-wav ↵ ~ c ~ ↵",
                Want::Captured,
            ),
            s(
                "C opens the checklist the playbook left. tab to the items; o on an avatar opens a launcher (the tutor records it instead).",
                "C ~ tab o ↵",
                Want::LauncherOpened,
            ),
            s(
                "d marks the item done: add a note, or just ↵.",
                "d ↵",
                Want::ItemResolved,
            ),
        ],
    },
    Lesson {
        title: "Write a finding",
        steps: &[
            s(
                "esc back; / find jmarlow-wav, ↵; f files a finding: → to Advancing the Timeline, ↓, write the claim, ↵.",
                "esc ~ / text:jmarlow-wav ↵ ~ f ~ → → → → → → → ↓ text:GitHub space text:active space text:after space text:the space text:last-seen space text:date ↵",
                Want::FindingCreated,
            ),
            s(
                "e edits it: ↓ to the relevance, say why it matters, ↵ saves. The checks on the right turn ✓.",
                "~ e ↓ text:Same space text:avatar space text:as space text:Bluesky ↵",
                Want::FindingPasses,
            ),
        ],
    },
    Lesson {
        title: "Pass the checks and export",
        steps: &[
            s(
                "r marks the finding ready: only possible once every check passes.",
                "r",
                Want::FindingReady,
            ),
            s(
                "x → Submission text copies it, ready to paste into the Trace Labs form.",
                "~ x ↵",
                Want::Exported,
            ),
        ],
    },
];

/// Tutor progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tutor {
    pub lesson: usize,
    pub step: usize,
}

impl Tutor {
    pub fn done(&self) -> bool {
        self.lesson >= LESSONS.len()
    }

    pub fn current(&self) -> Option<(&'static Lesson, &'static Step)> {
        let l = LESSONS.get(self.lesson)?;
        Some((l, l.steps.get(self.step)?))
    }

    /// Feed an event; `true` when it completed the current step.
    pub fn observe(&mut self, e: &Event) -> bool {
        let Some((l, step)) = self.current() else {
            return false;
        };
        if !step.want.met(e) {
            return false;
        }
        self.step += 1;
        if self.step >= l.steps.len() {
            self.lesson += 1;
            self.step = 0;
        }
        true
    }
}

/// What the tutor can see from the state, compared before and after each
/// message to find the events no single handler reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pane: Option<Pane>,
    tab: Option<Tab>,
    help: bool,
    focus_ty: Option<CoreType>,
    focus: Option<spektr_model::EntityId>,
    crumbs: usize,
    inspect: Option<String>,
    avatar_edge: bool,
    late: bool,
    accounts: usize,
}

fn late_activity(w: &Workspace) -> bool {
    if w.tab != Tab::Timeline || w.view != View::Main {
        return false;
    }
    let Some(ls) = w
        .status
        .as_ref()
        .and_then(|s| s.last_seen.as_deref())
        .and_then(|d| d.parse::<jiff::civil::Date>().ok())
    else {
        return false;
    };
    w.timeline
        .visible()
        .get(w.timeline.cursor)
        .is_some_and(|e| {
            crate::tabs::marks_activity(&e.attr)
                && e.at.to_zoned(jiff::tz::TimeZone::UTC).date() > ls
        })
}

pub fn probe(s: &AppState) -> Probe {
    let help = matches!(s.overlay, Some(Overlay::Help(_)));
    let Some(w) = s.workspace() else {
        return Probe {
            help,
            ..Probe::default()
        };
    };
    Probe {
        pane: Some(w.pane),
        tab: Some(w.tab),
        help,
        focus_ty: w.crumbs.last().and_then(|e| e.ty.core()),
        focus: w.focus,
        crumbs: w.crumbs.len(),
        inspect: w
            .detail
            .as_ref()
            .filter(|d| Some(d.entity.summary.id) == w.inspect && w.inspect_edge.is_none())
            .map(|d| d.entity.summary.key.clone()),
        avatar_edge: w.inspect_edge.is_some()
            && w.edge
                .as_ref()
                .is_some_and(|e| e.artifact == Some(spektr_model::ArtifactKind::AvatarPhash)),
        late: late_activity(w),
        accounts: w
            .entities
            .iter()
            .filter(|e| e.ty.is(CoreType::Account))
            .count(),
    }
}

/// The events between two probes.
pub fn diff(a: &Probe, b: &Probe) -> Vec<Event> {
    let mut out = Vec::new();
    if b.pane != a.pane
        && let Some(p) = b.pane
    {
        out.push(Event::Pane(p));
    }
    if b.tab != a.tab
        && let Some(t) = b.tab
    {
        out.push(Event::Tab(t));
    }
    if b.help && !a.help {
        out.push(Event::HelpOpened);
    }
    if b.crumbs < a.crumbs && b.focus != a.focus {
        out.push(Event::Back);
    } else if b.focus != a.focus
        && let Some(t) = b.focus_ty
    {
        out.push(Event::Pivot(t));
    }
    if b.inspect != a.inspect
        && let Some(k) = &b.inspect
    {
        out.push(Event::Inspect(k.clone()));
    }
    if b.avatar_edge && !a.avatar_edge {
        out.push(Event::AvatarEdge);
    }
    if b.late && !a.late {
        out.push(Event::LateActivity);
    }
    if b.accounts != a.accounts {
        out.push(Event::Accounts(b.accounts));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_advance_on_their_events_only() {
        let mut t = Tutor::default();
        assert!(!t.observe(&Event::HelpOpened), "not yet");
        assert!(t.observe(&Event::Pane(Pane::Inspector)));
        assert!(t.observe(&Event::HelpOpened));
        assert_eq!((t.lesson, t.step), (1, 0));
        assert!(!t.observe(&Event::Seeded("someone-else".into())));
        assert!(t.observe(&Event::Seeded("jmarlow.wav".into())));
        assert!(!t.observe(&Event::Accounts(2)));
        assert!(t.observe(&Event::Accounts(8)));
        assert_eq!((t.lesson, t.step), (2, 0));
    }

    #[test]
    fn the_script_has_ten_lessons_with_keys() {
        assert_eq!(LESSONS.len(), 10);
        let steps: usize = LESSONS.iter().map(|l| l.steps.len()).sum();
        assert!(steps >= 18, "{steps}");
        let mut t = Tutor::default();
        for l in LESSONS {
            for st in l.steps {
                assert!(!st.goal.is_empty());
                let _ = st;
            }
            let _ = l;
        }
        t.lesson = LESSONS.len();
        assert!(t.done());
        assert!(t.current().is_none());
    }
}
