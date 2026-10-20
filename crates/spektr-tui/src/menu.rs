//! Context menus (TUI_SPEC §7): built from the action registry, filtered by
//! the target's type and state.

use spektr_engine::{EntitySummary, LauncherView, ModuleInfo, PlaybookInfo};
use spektr_model::EdgeId;

use crate::keymap::{Action, Ctx as KCtx, Keymap};
use crate::palette::{cost_label, module_block};
use crate::state::{MenuAct, MenuItem, Submenu};

fn item(km: &Keymap, action: Action, disabled: Option<&str>, rule: bool) -> MenuItem {
    let (key, label) = km
        .hint_for(action, KCtx::Entity)
        .or_else(|| km.hint_for(action, KCtx::Nav))
        .unwrap_or_default();
    MenuItem {
        label: label.to_string(),
        right: disabled.map_or(key, str::to_string),
        act: MenuAct::Key(action),
        disabled: disabled.map(str::to_string),
        submenu: false,
        rule,
    }
}

/// Whether an entity has a URL to open in a browser.
pub fn has_url(e: &EntitySummary) -> bool {
    e.ty.is(spektr_model::CoreType::Url)
}

/// What the menu needs to know about its target beyond the entity.
pub struct Info<'a> {
    pub edge: Option<EdgeId>,
    pub is_focus: bool,
    /// Launchers, once the inspector's detail for the target is loaded.
    pub launchers: Option<&'a [LauncherView]>,
    pub playbooks: &'a [PlaybookInfo],
    pub observations: usize,
    /// The URL `O` opens: a url's own, an account's profile, an image's
    /// source.
    pub url: Option<&'a str>,
}

/// Playbooks whose seed takes the target's type.
pub fn playbooks_for<'a>(target: &EntitySummary, all: &'a [PlaybookInfo]) -> Vec<&'a PlaybookInfo> {
    all.iter()
        .filter(|p| p.seed.iter().any(|t| t == target.ty.as_str()))
        .collect()
}

/// The menu for an entity (and the edge under the cursor, if any).
pub fn build(km: &Keymap, target: &EntitySummary, c: &Info<'_>) -> Vec<MenuItem> {
    let mut v = Vec::new();
    // navigate
    let mut pivot = item(
        km,
        Action::Activate,
        c.is_focus.then_some("already the focus"),
        false,
    );
    pivot.label = "Pivot here".into();
    v.push(pivot);
    let mut run = item(km, Action::RunModule, None, false);
    run.submenu = true;
    run.right = format!("{} ▸", run.right);
    v.push(run);
    let pbs = playbooks_for(target, c.playbooks);
    let mut pb = item(
        km,
        Action::RunPlaybook,
        pbs.is_empty().then_some("none for this type"),
        false,
    );
    pb.label = "Run playbook".into();
    if !pbs.is_empty() {
        pb.submenu = true;
        pb.right = format!("{} ▸", pb.right);
    }
    v.push(pb);
    let launchers = c.launchers.map_or(0, <[LauncherView]>::len);
    let mut url = item(
        km,
        Action::OpenUrl,
        (c.url.is_none() && launchers == 0).then_some("no URL or launcher"),
        false,
    );
    url.label = "Open in browser".into();
    if c.url.is_some() || launchers > 0 {
        url.submenu = true;
        url.right = format!("{} ▸", url.right);
    }
    v.push(url);
    let mut copy = item(km, Action::Copy, None, false);
    copy.label = "Copy value".into();
    v.push(copy);
    // scope
    v.push(item(km, Action::ScopeAdd, None, true));
    v.push(item(km, Action::ScopeExclude, None, false));
    // analysis
    let mut finding = item(km, Action::NewFinding, None, true);
    finding.label = "Mark as finding".into();
    v.push(finding);
    v.push(item(km, Action::Note, None, false));
    v.push(item(km, Action::Merge, None, false));
    if c.edge.is_some() {
        let mut e = item(km, Action::Confirm, None, false);
        e.label = "Confirm edge".into();
        v.push(e);
        let mut r = item(km, Action::Reject, None, false);
        r.label = "Reject edge".into();
        v.push(r);
    }
    let mut raw = item(
        km,
        Action::OpenEvidence,
        (c.observations == 0).then_some("no evidence"),
        false,
    );
    raw.label = "Open raw response".into();
    v.push(raw);
    v
}

/// `Run playbook ▸`: playbooks for the target's type; ones needing another
/// rules profile, or with problems, stay visible but disabled.
pub fn playbook_submenu(target: &EntitySummary, all: &[PlaybookInfo], profile: &str) -> Submenu {
    let items = playbooks_for(target, all)
        .into_iter()
        .map(|p| {
            let disabled = if !p.errors.is_empty() {
                Some(format!("{} problems", p.errors.len()))
            } else if !p.requires_profile.is_empty() && p.requires_profile != profile {
                Some(format!("needs {}", p.requires_profile))
            } else {
                None
            };
            MenuItem {
                label: p.name.clone(),
                right: disabled
                    .clone()
                    .unwrap_or_else(|| format!("{} steps", p.steps.len())),
                act: MenuAct::RunPlaybook(p.name.clone()),
                disabled,
                submenu: false,
                rule: false,
            }
        })
        .collect();
    Submenu {
        title: format!("Playbooks for {} {}", target.ty.glyph(), target.ty.as_str()),
        items,
        sel: 0,
    }
}

/// `Open in browser ▸`: the URL itself, then the launchers.
pub fn launcher_submenu(
    km: &Keymap,
    target: &EntitySummary,
    url: Option<&str>,
    launchers: &[LauncherView],
) -> Submenu {
    let mut items = Vec::new();
    if let Some(u) = url {
        let label = if has_url(target) {
            "This URL"
        } else if target.ty.is(spektr_model::CoreType::Account) {
            "Profile page"
        } else {
            "Source page"
        };
        let (key, _) = km
            .hint_for(Action::OpenUrl, KCtx::Entity)
            .unwrap_or_default();
        items.push(MenuItem {
            label: label.into(),
            right: key,
            act: MenuAct::Url(u.to_string()),
            disabled: None,
            submenu: false,
            rule: false,
        });
    }
    for (i, l) in launchers.iter().enumerate() {
        items.push(MenuItem {
            label: l.name.clone(),
            right: l.disabled.clone().unwrap_or_default(),
            act: MenuAct::Launch {
                launcher: l.id.clone(),
                item: None,
            },
            disabled: l.disabled.clone(),
            submenu: false,
            rule: i == 0 && url.is_some(),
        });
    }
    Submenu {
        title: "Open in browser".into(),
        items,
        sel: 0,
    }
}

/// `Run module ▸`: modules that take the target's type, with their cost
/// or last run, disabled ones showing why.
pub fn run_submenu(
    km: &Keymap,
    target: &EntitySummary,
    modules: &[ModuleInfo],
    last_run: &dyn Fn(&str) -> Option<String>,
) -> Submenu {
    let mut items: Vec<MenuItem> = modules
        .iter()
        .filter(|m| m.consumes.iter().any(|c| c == target.ty.as_str()))
        .map(|m| {
            let block = module_block(m, Some(target));
            let right = match (&block, last_run(&m.name)) {
                (Some(why), _) => why.clone(),
                (None, Some(at)) => format!("ran {at} · rerun"),
                (None, None) => cost_label(m),
            };
            MenuItem {
                label: m.name.clone(),
                right,
                act: MenuAct::RunModule(m.name.clone()),
                disabled: block,
                submenu: false,
                rule: false,
            }
        })
        .collect();
    if items.is_empty() {
        items.push(MenuItem {
            label: format!("no module takes {}", target.ty.as_str()),
            right: String::new(),
            act: MenuAct::RunAll,
            disabled: Some("nothing to run".into()),
            submenu: false,
            rule: false,
        });
    }
    let (key, label) = km.hint_for(Action::RunAll, KCtx::Menu).unwrap_or_default();
    items.push(MenuItem {
        label: label.to_string(),
        right: key,
        act: MenuAct::RunAll,
        disabled: None,
        submenu: false,
        rule: true,
    });
    Submenu {
        title: format!("Modules for {} {}", target.ty.glyph(), target.ty.as_str()),
        items,
        sel: 0,
    }
}
