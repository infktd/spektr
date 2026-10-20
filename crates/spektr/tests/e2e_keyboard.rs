//! e2e_keyboard (QA.md §2): every action in the registry, triggered by its
//! default key in its context, changes what's on screen. Generated from
//! `spektr_tui::keymap::BINDINGS`; each binding runs in a fresh process on
//! its own copy of the fixture case.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use spektr_tui::keymap::{BINDINGS, Binding, Ctx};
use support::Home;
use support::term::{Size, Term};

/// How to reach a binding's context: `(args, size, setup keys, text that
/// shows the context is ready)`.
struct Plan {
    launcher: bool,
    /// The case: `demo`, or `pb` (with a person-from-username run).
    case: &'static str,
    size: (u16, u16),
    setup: &'static str,
    ready: &'static str,
}

fn ws(setup: &'static str, ready: &'static str) -> Plan {
    Plan {
        launcher: false,
        case: "demo",
        size: (170, 48),
        setup,
        ready,
    }
}

/// On the case with a playbook run (checklist items, review items).
fn pb(setup: &'static str, ready: &'static str) -> Plan {
    Plan {
        case: "pb",
        ..ws(setup, ready)
    }
}

fn launcher(setup: &'static str, ready: &'static str) -> Plan {
    Plan {
        launcher: true,
        case: "demo",
        size: (132, 40),
        setup,
        ready,
    }
}

/// Screens that arrive in Phase 5 (Findings).
const NOT_BUILT: [Ctx; 1] = [Ctx::Findings];

fn plan(b: &Binding) -> Option<Plan> {
    if NOT_BUILT.contains(&b.ctx) {
        return None;
    }
    Some(match b.name {
        "tab-ego" => ws("2", "[2 timeline]"),
        "escape" => ws("/ g ↵", " match"),
        "up" => ws("j", "has_account"),
        "right" => ws("h", "has_account"),
        "top" => ws("j j", "has_account"),
        "half-up" => ws("G", "has_account"),
        "prev-page" => ws("]", "2/2"),
        "back" => ws("↵", "links_to"),
        "inspector" => Plan {
            size: (120, 36),
            ..ws("", "has_account")
        },
        // launcher
        "open" | "new-case" | "open-path" | "verify" => launcher("", "Recent cases"),
        // palette
        "palette-up" => ws(": ↓", "COMMAND"),
        "palette-complete" | "palette-down" | "palette-run" | "palette-close" => ws(":", "COMMAND"),
        // menu
        "menu-up" | "submenu-open" => ws("space ↓", "Pivot here"),
        "submenu-back" | "run-all" => ws("space ↓ →", "Modules for"),
        "menu-down" | "menu-run" | "menu-close" => ws("space", "Pivot here"),
        // filter
        "filter-up" => ws("/ text:a ↓", "FILTER"),
        "filter-down" | "filter-accept" | "filter-clear" => ws("/ text:a", "FILTER"),
        // dialogs
        "dialog-ok" => ws("n text:hello", "Note on"),
        "dialog-yes" => ws("x", "Reject edge"),
        "dialog-cancel" => ws("n", "Note on"),
        "dialog-down" => ws("m", "Merge"),
        "dialog-up" => ws("m ↓", "Merge"),
        "dialog-next" => launcher("n", "New case"),
        "dialog-prev-choice" | "dialog-next-choice" => launcher("n tab", "New case"),
        // help
        "help-search" | "help-close" => ws("?", "Navigation"),
        // checklist: the runs pane, then the items pane
        "item-playbook" | "stop-run" | "checklist-pane" => pb("C", "Playbooks 1 run"),
        "item-done" | "item-skip" | "item-note" | "item-open" | "item-finding" | "item-go" => {
            pb("C tab", "Reverse image search")
        }
        // review
        "review-accept" | "review-reject" | "review-later" | "review-open" => {
            pb("R", "[pending 5]")
        }
        _ => match b.ctx {
            Ctx::Global | Ctx::Nav | Ctx::Entity => ws("", "has_account"),
            other => panic!("no plan for {} in {other:?}", b.name),
        },
    })
}

/// A binding's first default key, in the harness key DSL.
fn dsl(key: &str) -> String {
    match key {
        "gg" => "g g".into(),
        k => k.into(),
    }
}

fn run_one(base: &Home, b: &Binding) -> Result<(), String> {
    let Some(p) = plan(b) else { return Ok(()) };
    let h = Home::new();
    h.copy_case(base, p.case);
    h.write_recent(&[p.case]);
    let case = format!("{}.spektr", p.case);
    let args: Vec<&str> = if p.launcher {
        Vec::new()
    } else {
        vec!["open", &case]
    };
    let size = Size {
        cols: p.size.0,
        rows: p.size.1,
    };
    let mut t = Term::spawn(&args, size, &h.env(), &h.work()).map_err(|e| e.to_string())?;
    let first = if p.launcher {
        "Recent cases"
    } else {
        "has_account"
    };
    t.wait_for_text(first, Duration::from_secs(15))
        .map_err(|e| format!("{e:#}"))?;
    t.settle(Duration::from_millis(150), Duration::from_secs(5))
        .map_err(|e| format!("{e:#}"))?;
    if !p.setup.is_empty() {
        t.keys(p.setup).map_err(|e| e.to_string())?;
        t.wait_for_text(p.ready, Duration::from_secs(10))
            .map_err(|e| format!("setup `{}`: {e:#}", p.setup))?;
        t.settle(Duration::from_millis(150), Duration::from_secs(5))
            .map_err(|e| format!("{e:#}"))?;
    }
    let before = t.screen();
    let key = dsl(b.keys[0]);
    t.keys(&key).map_err(|e| e.to_string())?;
    if b.name == "quit" {
        let code = t
            .wait_exit(Duration::from_secs(10))
            .map_err(|e| format!("{e:#}"))?;
        return if code == 0 {
            Ok(())
        } else {
            Err(format!("exit {code}"))
        };
    }
    t.wait_changed(&before, Duration::from_secs(5))
        .map(|_| ())
        .map_err(|_| format!("`{key}` changed nothing; screen:\n{}", t.screen().contents))
}

#[test]
fn every_registry_action_changes_the_screen() {
    let base = Home::new();
    base.demo_case();
    for args in [
        &[
            "new",
            "pb",
            "--profile",
            "tracelabs",
            "--subject",
            "Jordan Marlow",
        ][..],
        &[
            "run",
            "pb.spektr",
            "--seed",
            "username:jmarlow.wav",
            "--playbook",
            "person-from-username",
        ],
    ] {
        assert!(
            base.cmd(args).output().unwrap().status.success(),
            "{args:?}"
        );
    }
    let work: Vec<&Binding> = BINDINGS.iter().filter(|b| plan(b).is_some()).collect();
    let skipped: Vec<&str> = BINDINGS
        .iter()
        .filter(|b| plan(b).is_none())
        .map(|b| b.name)
        .collect();
    assert!(
        BINDINGS
            .iter()
            .filter(|b| plan(b).is_none())
            .all(|b| NOT_BUILT.contains(&b.ctx)),
        "only unbuilt screens may be skipped: {skipped:?}"
    );
    let failures = std::sync::Mutex::new(Vec::new());
    let chunks: Vec<Vec<&Binding>> = work
        .chunks(work.len().div_ceil(8))
        .map(<[_]>::to_vec)
        .collect();
    std::thread::scope(|sc| {
        for chunk in &chunks {
            let (base, failures) = (&base, &failures);
            sc.spawn(move || {
                for b in chunk {
                    if let Err(e) = run_one(base, b) {
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("{} ({:?}, `{}`): {e}", b.name, b.ctx, b.keys[0]));
                    }
                }
            });
        }
    });
    let f = failures.into_inner().unwrap();
    println!(
        "{} bindings exercised, {} skipped (Findings is Phase 5: {skipped:?})",
        work.len(),
        skipped.len()
    );
    assert!(
        f.is_empty(),
        "{} of {} bindings failed:\n{}",
        f.len(),
        work.len(),
        f.join("\n\n")
    );
}
