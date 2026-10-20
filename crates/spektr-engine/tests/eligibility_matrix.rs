//! The eligibility matrix (MODULES §5): every rule × every trigger
//! (Phase 2 exit criterion).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::vec_init_then_push)]

use spektr_engine::Profiles;
use spektr_engine::eligibility::{BudgetFacts, InputFacts, ModuleFacts, Request, Verdict, check};
use spektr_model::ObservationId;
use spektr_protocol::{Manifest, Technique};
use spektr_store::Trigger;

const TRIGGERS: [Trigger; 4] = [
    Trigger::Auto,
    Trigger::Manual,
    Trigger::Playbook,
    Trigger::Tutor,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Run,
    Blocked,
    Cached,
    Ineligible,
}

fn kind(v: &Verdict) -> Expect {
    match v {
        Verdict::Run => Expect::Run,
        Verdict::Blocked(_) => Expect::Blocked,
        Verdict::Cached(_) => Expect::Cached,
        Verdict::Ineligible(_) => Expect::Ineligible,
    }
}

fn manifest() -> Manifest {
    Manifest::from_toml(
        r#"[module]
name = "probe-free"
version = "1.0.0"
description = "test"
runtime = "native"
technique = "passive"
auto = true
consumes = ["username", "account:github"]
produces = [["has_account", "account"]]
cost = { class = "keyed", daily_budget = 10 }
secrets = ["SOME_KEY"]
"#,
    )
    .unwrap()
}

/// Everything passes; each rule below breaks exactly one thing.
struct Case {
    manifest: Manifest,
    enabled: bool,
    missing: Vec<String>,
    ty: &'static str,
    platform: Option<&'static str>,
    depth: u32,
    hub: bool,
    in_scope: bool,
    profile: &'static str,
    profile_auto: bool,
    blocked_module: bool,
    cached: Option<ObservationId>,
    used: u32,
    rerun: bool,
}

impl Case {
    fn ok() -> Self {
        Case {
            manifest: manifest(),
            enabled: true,
            missing: vec![],
            ty: "username",
            platform: None,
            depth: 1,
            hub: false,
            in_scope: true,
            profile: "tracelabs",
            profile_auto: true,
            blocked_module: false,
            cached: None,
            used: 3,
            rerun: false,
        }
    }

    fn verdict(&self, trigger: Trigger) -> Verdict {
        let profiles = Profiles::builtin().unwrap();
        let mut profile = profiles.get(self.profile).unwrap().clone();
        profile.auto = self.profile_auto;
        if self.blocked_module {
            profile.blocked_modules.push(self.manifest.name.clone());
        }
        check(&Request {
            module: ModuleFacts {
                manifest: &self.manifest,
                enabled: self.enabled,
                missing_secrets: self.missing.clone(),
            },
            input: InputFacts {
                ty: self.ty,
                platform: self.platform,
                depth: self.depth,
                hub: self.hub,
                in_scope: self.in_scope,
            },
            trigger,
            rerun: self.rerun,
            profile: &profile,
            case_depth: 2,
            cached: self.cached,
            budget: BudgetFacts {
                used: self.used,
                limit: self.manifest.cost.daily_budget,
            },
        })
    }
}

/// `(rule, how it's broken, expected verdict for [auto, manual, playbook, tutor])`.
fn matrix() -> Vec<(&'static str, Case, [Expect; 4])> {
    use Expect::*;
    let obs = ObservationId(uuid::Uuid::from_u128(7));
    let mut rows = vec![];
    rows.push(("eligible", Case::ok(), [Run, Run, Run, Run]));
    rows.push((
        "1 disabled",
        Case {
            enabled: false,
            ..Case::ok()
        },
        [Ineligible, Blocked, Blocked, Blocked],
    ));
    rows.push((
        "1 missing secret",
        Case {
            missing: vec!["SOME_KEY".into()],
            ..Case::ok()
        },
        [Ineligible, Blocked, Blocked, Blocked],
    ));
    rows.push((
        "2 wrong type",
        Case {
            ty: "domain",
            ..Case::ok()
        },
        [Ineligible, Ineligible, Ineligible, Ineligible],
    ));
    rows.push((
        "2 wrong platform",
        Case {
            ty: "account",
            platform: Some("gitlab"),
            ..Case::ok()
        },
        [Ineligible, Ineligible, Ineligible, Ineligible],
    ));
    rows.push((
        "2 right platform",
        Case {
            ty: "account",
            platform: Some("github"),
            ..Case::ok()
        },
        [Run, Run, Run, Run],
    ));
    let mut probing = Case::ok();
    probing.manifest.technique = Technique::Probing;
    rows.push((
        "3 technique forbidden",
        probing,
        [Blocked, Blocked, Blocked, Blocked],
    ));
    let mut active = Case {
        profile: "default",
        ..Case::ok()
    };
    active.manifest.technique = Technique::Active;
    rows.push((
        "3 active forbidden (default)",
        active,
        [Blocked, Blocked, Blocked, Blocked],
    ));
    rows.push((
        "3 module blocked by name",
        Case {
            blocked_module: true,
            ..Case::ok()
        },
        [Blocked, Blocked, Blocked, Blocked],
    ));
    let mut manual_only = Case::ok();
    manual_only.manifest.auto = false;
    rows.push((
        "4 module not auto",
        manual_only,
        [Ineligible, Run, Run, Run],
    ));
    rows.push((
        "4 profile auto off",
        Case {
            profile_auto: false,
            ..Case::ok()
        },
        [Ineligible, Run, Run, Run],
    ));
    rows.push((
        "4 past depth",
        Case {
            depth: 3,
            ..Case::ok()
        },
        [Ineligible, Run, Run, Run],
    ));
    rows.push((
        "4 at depth limit",
        Case {
            depth: 2,
            ..Case::ok()
        },
        [Run, Run, Run, Run],
    ));
    rows.push((
        "4 out of scope",
        Case {
            in_scope: false,
            ..Case::ok()
        },
        [Ineligible, Run, Blocked, Run],
    ));
    rows.push((
        "4 hub",
        Case {
            hub: true,
            ..Case::ok()
        },
        [Ineligible, Run, Blocked, Run],
    ));
    rows.push((
        "5 cached",
        Case {
            cached: Some(obs),
            ..Case::ok()
        },
        [Cached, Cached, Cached, Cached],
    ));
    rows.push((
        "5 cached, explicit rerun",
        Case {
            cached: Some(obs),
            rerun: true,
            ..Case::ok()
        },
        [Run, Run, Run, Run],
    ));
    rows.push((
        "6 budget exhausted",
        Case {
            used: 10,
            ..Case::ok()
        },
        [Ineligible, Blocked, Blocked, Blocked],
    ));
    // precedence: rules beat the cache and the budget; consumes beats rules
    let mut probing_cached = Case {
        cached: Some(obs),
        used: 10,
        ..Case::ok()
    };
    probing_cached.manifest.technique = Technique::Probing;
    rows.push((
        "3 before 5 and 6",
        probing_cached,
        [Blocked, Blocked, Blocked, Blocked],
    ));
    let mut probing_wrong_type = Case {
        ty: "email",
        ..Case::ok()
    };
    probing_wrong_type.manifest.technique = Technique::Probing;
    rows.push((
        "2 before 3",
        probing_wrong_type,
        [Ineligible, Ineligible, Ineligible, Ineligible],
    ));
    rows.push((
        "1 before 2",
        Case {
            enabled: false,
            ty: "email",
            ..Case::ok()
        },
        [Ineligible, Blocked, Blocked, Blocked],
    ));
    rows
}

#[test]
fn every_rule_times_every_trigger() {
    let mut checked = 0;
    let mut table = String::new();
    for (rule, case, expected) in matrix() {
        for (trigger, want) in TRIGGERS.iter().zip(expected) {
            let v = case.verdict(*trigger);
            assert_eq!(
                kind(&v),
                want,
                "rule `{rule}`, trigger {trigger}: got {v:?}"
            );
            checked += 1;
        }
        let got: Vec<String> = TRIGGERS
            .iter()
            .map(|t| format!("{:?}", kind(&case.verdict(*t))))
            .collect();
        table.push_str(&format!("{rule:<30} {}\n", got.join(" ")));
    }
    println!("{table}");
    assert_eq!(checked, 21 * 4);
}

#[test]
fn reasons_are_specific() {
    let mut probing = Case::ok();
    probing.manifest.technique = Technique::Probing;
    assert_eq!(
        probing.verdict(Trigger::Auto),
        Verdict::Blocked("tracelabs forbids probing".into())
    );
    assert_eq!(
        Case {
            missing: vec!["SOME_KEY".into()],
            ..Case::ok()
        }
        .verdict(Trigger::Playbook),
        Verdict::Blocked("missing secret SOME_KEY".into())
    );
    assert_eq!(
        Case {
            used: 10,
            ..Case::ok()
        }
        .verdict(Trigger::Manual),
        Verdict::Blocked("daily budget exhausted (10/10)".into())
    );
    assert!(
        matches!(Case { hub: true, ..Case::ok() }.verdict(Trigger::Auto), Verdict::Ineligible(r) if r.starts_with("hub"))
    );
}
