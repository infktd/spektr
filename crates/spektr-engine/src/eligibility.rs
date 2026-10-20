//! Whether a job runs (MODULES §5). A pure function, so the full
//! rule × trigger matrix can be tested directly.

use spektr_model::ObservationId;
use spektr_protocol::Manifest;
use spektr_store::Trigger;

use crate::profile::RulesProfile;

/// What eligibility needs to know about the module.
#[derive(Debug, Clone)]
pub struct ModuleFacts<'a> {
    pub manifest: &'a Manifest,
    pub enabled: bool,
    /// Required secrets that aren't set.
    pub missing_secrets: Vec<String>,
}

/// What eligibility needs to know about the input entity.
#[derive(Debug, Clone)]
pub struct InputFacts<'a> {
    pub ty: &'a str,
    /// `account` inputs: the platform.
    pub platform: Option<&'a str>,
    pub depth: u32,
    pub hub: bool,
    pub in_scope: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetFacts {
    pub used: u32,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Run,
    /// Recorded as a `blocked` job, in the activity rail and the audit log.
    Blocked(String),
    /// Recorded as a `cached` job pointing at the observation.
    Cached(ObservationId),
    /// Not a job at all (an automatic candidate that doesn't apply). For a
    /// manual request this is reported back to the analyst.
    Ineligible(String),
}

#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub module: ModuleFacts<'a>,
    pub input: InputFacts<'a>,
    pub trigger: Trigger,
    /// An explicit rerun skips the cache.
    pub rerun: bool,
    pub profile: &'a RulesProfile,
    /// The case's (or this run's) depth limit.
    pub case_depth: u32,
    pub cached: Option<ObservationId>,
    pub budget: BudgetFacts,
}

/// MODULES §5, in order: enabled and secrets; consumes; rules profile;
/// trigger conditions (automatic: auto flag, depth, scope, hubs; playbook:
/// scope and hubs); cache; budget.
pub fn check(r: &Request<'_>) -> Verdict {
    let name = &r.module.manifest.name;
    let auto = r.trigger == Trigger::Auto;
    let soft = |reason: String| {
        if auto {
            Verdict::Ineligible(reason)
        } else {
            Verdict::Blocked(reason)
        }
    };

    // 1. Enabled, required secrets present.
    if !r.module.enabled {
        return soft(format!("{name} is disabled in this case"));
    }
    if let Some(s) = r.module.missing_secrets.first() {
        return soft(format!("missing secret {s}"));
    }
    // 2. Consumes.
    if !r
        .module
        .manifest
        .consumes_entity(r.input.ty, r.input.platform)
    {
        let what = match r.input.platform {
            Some(p) => format!("account:{p}"),
            None => r.input.ty.to_string(),
        };
        return Verdict::Ineligible(format!("{name} does not accept {what}"));
    }
    // 3. Rules profile: always recorded, whatever the trigger.
    if let Some(reason) = r.profile.block_reason(name, r.module.manifest.technique) {
        return Verdict::Blocked(reason);
    }
    // 4. Trigger.
    match r.trigger {
        Trigger::Auto => {
            if !r.profile.auto {
                return Verdict::Ineligible(format!(
                    "automatic expansion is off in {}",
                    r.profile.name
                ));
            }
            if !r.module.manifest.auto {
                return Verdict::Ineligible(format!("{name} only runs when asked"));
            }
            if r.input.depth > r.case_depth {
                return Verdict::Ineligible(format!(
                    "depth {} is past the limit {}",
                    r.input.depth, r.case_depth
                ));
            }
            if !r.input.in_scope {
                return Verdict::Ineligible("out of scope".into());
            }
            if r.input.hub {
                return Verdict::Ineligible("hub: not expanded automatically".into());
            }
        }
        Trigger::Playbook => {
            if !r.input.in_scope {
                return Verdict::Blocked("out of scope".into());
            }
            if r.input.hub {
                return Verdict::Blocked("hub: not expanded by playbooks".into());
            }
        }
        Trigger::Manual | Trigger::Tutor => {}
    }
    // 5. Cache.
    if !r.rerun
        && let Some(obs) = r.cached
    {
        return Verdict::Cached(obs);
    }
    // 6. Budget.
    if let Some(limit) = r.budget.limit
        && r.budget.used >= limit
    {
        return soft(format!(
            "daily budget exhausted ({}/{limit})",
            r.budget.used
        ));
    }
    Verdict::Run
}
