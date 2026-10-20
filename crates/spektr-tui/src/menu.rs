//! Context menus (TUI_SPEC §7): built from the action registry, filtered by
//! the target's type and state.

use spektr_engine::{EntitySummary, ModuleInfo};
use spektr_model::EdgeId;

use crate::keymap::{Action, Ctx, Keymap};
use crate::palette::{NOT_YET, cost_label, module_block};
use crate::state::{MenuAct, MenuItem, Submenu};

fn item(km: &Keymap, action: Action, disabled: Option<&str>, rule: bool) -> MenuItem {
    let (key, label) = km
        .hint_for(action, Ctx::Entity)
        .or_else(|| km.hint_for(action, Ctx::Nav))
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

/// The menu for an entity (and the edge under the cursor, if any).
pub fn build(
    km: &Keymap,
    target: &EntitySummary,
    edge: Option<EdgeId>,
    is_focus: bool,
) -> Vec<MenuItem> {
    let mut v = Vec::new();
    // navigate
    let mut pivot = item(
        km,
        Action::Activate,
        is_focus.then_some("already the focus"),
        false,
    );
    pivot.label = "Pivot here".into();
    v.push(pivot);
    let mut run = item(km, Action::RunModule, None, false);
    run.submenu = true;
    run.right = format!("{} ▸", run.right);
    v.push(run);
    let mut pb = item(km, Action::RunPlaybook, Some(NOT_YET), false);
    pb.label = "Run playbook".into();
    v.push(pb);
    let mut url = item(
        km,
        Action::OpenUrl,
        (!has_url(target)).then_some("no URL"),
        false,
    );
    url.label = "Open in browser".into();
    v.push(url);
    let mut copy = item(km, Action::Copy, None, false);
    copy.label = "Copy value".into();
    v.push(copy);
    // scope
    v.push(item(km, Action::ScopeAdd, None, true));
    v.push(item(km, Action::ScopeExclude, None, false));
    // analysis
    let mut finding = item(km, Action::NewFinding, Some(NOT_YET), true);
    finding.label = "Mark as finding".into();
    v.push(finding);
    v.push(item(km, Action::Note, None, false));
    v.push(item(km, Action::Merge, None, false));
    if edge.is_some() {
        let mut c = item(km, Action::Confirm, None, false);
        c.label = "Confirm edge".into();
        v.push(c);
        let mut r = item(km, Action::Reject, None, false);
        r.label = "Reject edge".into();
        v.push(r);
    }
    let mut raw = item(km, Action::OpenEvidence, Some(NOT_YET), false);
    raw.label = "Open raw response".into();
    v.push(raw);
    v
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
    let (key, label) = km.hint_for(Action::RunAll, Ctx::Menu).unwrap_or_default();
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
