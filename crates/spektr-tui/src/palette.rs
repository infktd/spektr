//! The command palette (TUI_SPEC §5, §6 "Palette commands"): candidate
//! items, fuzzy matching, and parsing a command line.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use spektr_engine::{CostClass, EntitySummary, ModuleInfo};
use spektr_model::{CoreType, EntityRef};

use crate::glyph::GlyphSet;
use crate::state::{PaletteItem, Section};

/// Palette commands: `(template, description, available)`. A template that
/// ends in a space takes an argument.
pub const COMMANDS: &[(&str, &str, bool)] = &[
    (
        "seed ",
        "Add a seed: seed <type>:<value>, then expand it",
        true,
    ),
    ("run ", "Run a module on the cursor entity", true),
    ("rerun ", "Run a module again, ignoring the cache", true),
    (
        "expand",
        "Run every auto module on the cursor entity, even a hub",
        true,
    ),
    ("depth ", "Set the auto-expansion depth: depth <n>", true),
    (
        "scope add ",
        "Add a scope rule: scope add [deny] <rule>",
        true,
    ),
    ("scope list", "List the scope rules", true),
    ("rules ", "Switch the rules profile: rules <profile>", true),
    (
        "merge",
        "Merge the cursor entity with another (picker)",
        true,
    ),
    ("unmerge", "Undo the cursor entity's last merge", false),
    ("confirm", "Confirm the edge under the cursor", true),
    ("reject", "Reject the edge under the cursor", true),
    (
        "path ",
        "Shortest path from the focus: path <type>:<value>",
        true,
    ),
    (
        "note ",
        "Add a note to the cursor entity: note <text>",
        true,
    ),
    ("finding new", "New finding from the cursor entity", true),
    ("capture ", "Capture a page: capture <url>", true),
    ("attach ", "Attach a file: attach <path>", true),
    (
        "open ",
        "Open a pivot launcher on the cursor entity: open <launcher>",
        true,
    ),
    ("export ", "Export the case: export json|md|tracelabs", true),
    ("verify", "Re-hash the audit chain and blobs", true),
    (
        "playbook ",
        "Run a playbook on the focused entity: playbook <name>",
        true,
    ),
    ("playbook stop", "Stop the running playbook", true),
    ("checklist", "Open the checklist and playbook runs", true),
    (
        "reload",
        "Re-read modules, playbooks, profiles and launchers",
        true,
    ),
    (
        "region ",
        "Default region for phone numbers: region <CC>",
        true,
    ),
    ("mouse on", "Turn mouse capture on", true),
    (
        "mouse off",
        "Turn mouse capture off (plain terminal selection)",
        true,
    ),
    (
        "images ",
        "Image protocol: images auto|kitty|sixel|iterm2|halfblocks|off",
        true,
    ),
    ("glyphs unicode", "Unicode glyphs", true),
    ("glyphs ascii", "ASCII glyphs", true),
    ("tutor", "Start or resume the tutor", true),
    ("tutor reset", "Restart the tutor", true),
    ("help", "Every key binding", true),
];

/// Shown for commands whose feature isn't built yet.
pub const NOT_YET: &str = "not available yet";

/// A parsed palette command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Seed(EntityRef),
    Run {
        module: String,
        rerun: bool,
    },
    Expand,
    Depth(u32),
    ScopeAdd(String),
    ScopeList,
    Rules(String),
    Merge,
    Confirm,
    Reject,
    Note(String),
    Verify,
    Region(String),
    Mouse(bool),
    Glyphs(GlyphSet),
    Help,
    Path(EntityRef),
    /// Without a URL: the cursor entity's.
    Capture(Option<String>),
    Attach(String),
    Open(String),
    Playbook(String),
    PlaybookStop,
    Checklist,
    Reload,
    Images(crate::images::Mode),
    FindingNew,
    Export(spektr_engine::export::Format),
    /// `tutor` (false) or `tutor reset` (true).
    Tutor(bool),
}

pub fn parse(line: &str) -> Result<Cmd, String> {
    let line = line.trim();
    let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    let need = |what: &str| {
        if rest.is_empty() {
            Err(format!("{head} needs {what}"))
        } else {
            Ok(rest.to_string())
        }
    };
    Ok(match head {
        "seed" => {
            let arg = need("<type>:<value>")?;
            let r = arg.parse::<EntityRef>().map_err(|e| e.to_string())?;
            Cmd::Seed(r)
        }
        "run" | "rerun" => Cmd::Run {
            module: need("a module name")?,
            rerun: head == "rerun",
        },
        "expand" => Cmd::Expand,
        "depth" => Cmd::Depth(
            need("a number")?
                .parse()
                .map_err(|_| format!("depth `{rest}` is not a number"))?,
        ),
        "scope" => match rest.split_once(' ') {
            Some(("add", rule)) if !rule.trim().is_empty() => Cmd::ScopeAdd(rule.trim().into()),
            _ if rest == "list" => Cmd::ScopeList,
            _ => return Err("scope add [deny] <rule> · scope list".into()),
        },
        "rules" => Cmd::Rules(need("a profile name")?),
        "merge" => Cmd::Merge,
        "confirm" => Cmd::Confirm,
        "reject" => Cmd::Reject,
        "note" => Cmd::Note(need("some text")?),
        "verify" => Cmd::Verify,
        "region" => {
            let r = need("a two-letter country code")?.to_ascii_uppercase();
            if r.len() != 2 || !r.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(format!("region `{rest}` must be a two-letter country code"));
            }
            Cmd::Region(r)
        }
        "mouse" => match rest {
            "on" => Cmd::Mouse(true),
            "off" => Cmd::Mouse(false),
            _ => return Err("mouse on|off".into()),
        },
        "glyphs" => Cmd::Glyphs(GlyphSet::parse(rest).ok_or("glyphs unicode|ascii")?),
        "help" => Cmd::Help,
        "path" => Cmd::Path(
            need("<type>:<value>")?
                .parse::<EntityRef>()
                .map_err(|e| e.to_string())?,
        ),
        "capture" => Cmd::Capture(Some(rest.to_string()).filter(|r| !r.is_empty())),
        "attach" => Cmd::Attach(need("a file path")?),
        "open" => Cmd::Open(need("a launcher id")?),
        "playbook" if rest == "stop" => Cmd::PlaybookStop,
        "playbook" => Cmd::Playbook(need("a playbook name")?),
        "checklist" => Cmd::Checklist,
        "finding" if rest == "new" => Cmd::FindingNew,
        "export" => Cmd::Export(
            spektr_engine::export::Format::parse(rest).ok_or("export json|md|tracelabs")?,
        ),
        "tutor" if rest.is_empty() => Cmd::Tutor(false),
        "tutor" if rest == "reset" => Cmd::Tutor(true),
        "reload" => Cmd::Reload,
        "images" => Cmd::Images(
            crate::images::Mode::parse(rest)
                .ok_or("images auto|kitty|sixel|iterm2|halfblocks|off")?,
        ),
        "" => return Err("type a command".into()),
        other => {
            if COMMANDS
                .iter()
                .any(|(t, _, ok)| !ok && t.split(' ').next() == Some(other))
            {
                return Err(format!("{other}: {NOT_YET}"));
            }
            return Err(format!("unknown command `{other}`"));
        }
    })
}

fn type_glyphs(types: impl Iterator<Item = String>, g: GlyphSet) -> String {
    let mut out: Vec<char> = Vec::new();
    for t in types {
        let c = match (CoreType::parse(&t), g) {
            (Some(c), GlyphSet::Unicode) => c.glyph(),
            (Some(c), GlyphSet::Ascii) => c.ascii(),
            (None, GlyphSet::Unicode) => '·',
            (None, GlyphSet::Ascii) => 'x',
        };
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out.iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Consumes → produces glyphs: `◆ ◇ → ⊟ ◇`.
pub fn signature(m: &ModuleInfo, g: GlyphSet) -> String {
    let consumes = type_glyphs(m.consumes.iter().cloned(), g);
    let produces = type_glyphs(m.produces.iter().map(|(t, _)| t.clone()), g);
    format!("{consumes} {} {produces}", g.m(crate::glyph::Marker::Out))
}

pub fn cost_label(m: &ModuleInfo) -> String {
    match m.cost {
        CostClass::Free => "free".into(),
        CostClass::Local => "local".into(),
        CostClass::Keyed => "keyed".into(),
        CostClass::Metered => match m.daily_budget {
            Some(b) => format!("metered · {b}/day"),
            None => "metered".into(),
        },
    }
}

/// Why a module can't run on `target`, or `None` when it can.
pub fn module_block(m: &ModuleInfo, target: Option<&EntitySummary>) -> Option<String> {
    if let Some(why) = &m.blocked {
        return Some(why.clone());
    }
    if let Some(s) = m.missing_secrets.first() {
        return Some(format!("needs {s}"));
    }
    match target {
        Some(t) if !m.consumes.iter().any(|c| c == t.ty.as_str()) => {
            Some(format!("doesn't take {}", t.ty.as_str()))
        }
        None => Some("no entity under the cursor".into()),
        _ => None,
    }
}

pub struct Candidates<'a> {
    pub modules: &'a [ModuleInfo],
    pub target: Option<&'a EntitySummary>,
    pub history: &'a [String],
    pub glyphs: GlyphSet,
    /// `(entity, module)` → `HH:MM` of the last run on the target.
    pub last_run: &'a dyn Fn(&str) -> Option<String>,
}

/// Every candidate, unfiltered.
pub fn candidates(c: &Candidates<'_>) -> Vec<PaletteItem> {
    let mut out = Vec::new();
    for m in c.modules {
        let applies = c
            .target
            .is_some_and(|t| m.consumes.iter().any(|x| x == t.ty.as_str()));
        if c.target.is_some() && !applies {
            continue;
        }
        let block = module_block(m, c.target);
        let last = (c.last_run)(&m.name);
        let info = match (&block, &last) {
            (Some(why), _) => why.clone(),
            (None, Some(at)) => format!("ran {at} · rerun"),
            (None, None) => cost_label(m),
        };
        let verb = if last.is_some() { "rerun" } else { "run" };
        let target = c
            .target
            .map(|t| format!(" on {}", crate::glyph::display_value(t, c.glyphs)))
            .unwrap_or_default();
        out.push(PaletteItem {
            section: Section::Modules,
            line: format!("{verb} {}", m.name),
            signature: signature(m, c.glyphs),
            info,
            detail: (
                m.name.clone(),
                m.version.clone(),
                format!(
                    "{}{target}. {}{}",
                    m.description.trim_end_matches('.'),
                    match m.cost {
                        CostClass::Local => "Runs locally; no network.",
                        _ => "Results are cached; a rerun ignores the cache.",
                    },
                    if m.auto {
                        " Runs automatically on new entities."
                    } else {
                        ""
                    }
                ),
            ),
            disabled: block,
        });
    }
    for (t, desc, ok) in COMMANDS {
        out.push(PaletteItem {
            section: Section::Commands,
            line: (*t).to_string(),
            signature: String::new(),
            info: if *ok { String::new() } else { NOT_YET.into() },
            detail: (t.trim_end().to_string(), String::new(), (*desc).to_string()),
            disabled: if *ok { None } else { Some(NOT_YET.into()) },
        });
    }
    for h in c.history.iter().take(5) {
        out.push(PaletteItem {
            section: Section::Recent,
            line: h.clone(),
            signature: String::new(),
            info: String::new(),
            detail: (h.clone(), String::new(), "Run again".into()),
            disabled: None,
        });
    }
    out
}

/// Filter by fuzzy match on the line. An empty input shows recent
/// commands, then modules, then commands.
pub fn filter(items: Vec<PaletteItem>, input: &str) -> Vec<PaletteItem> {
    if input.trim().is_empty() {
        let mut v = items;
        v.sort_by_key(|i| match i.section {
            Section::Recent => 0,
            Section::Modules => 1,
            Section::Commands => 2,
        });
        return v;
    }
    let pattern = Pattern::parse(input, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, PaletteItem)> = items
        .into_iter()
        .filter_map(|i| {
            let s = pattern.score(Utf32Str::new(&i.line, &mut buf), &mut matcher)?;
            Some((s, i))
        })
        .collect();
    let section = |s: Section| match s {
        Section::Modules => 0,
        Section::Commands => 1,
        Section::Recent => 2,
    };
    scored.sort_by(|a, b| {
        section(a.1.section)
            .cmp(&section(b.1.section))
            .then(b.0.cmp(&a.0))
    });
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse("depth 3"), Ok(Cmd::Depth(3)));
        assert_eq!(
            parse("scope add deny domain:cdn.kestrel.example"),
            Ok(Cmd::ScopeAdd("deny domain:cdn.kestrel.example".into()))
        );
        assert_eq!(parse("mouse off"), Ok(Cmd::Mouse(false)));
        assert!(matches!(
            parse("seed username:jmarlow.wav"),
            Ok(Cmd::Seed(_))
        ));
        assert_eq!(parse("tutor"), Ok(Cmd::Tutor(false)));
        assert_eq!(parse("tutor reset"), Ok(Cmd::Tutor(true)));
        assert_eq!(
            parse("export md"),
            Ok(Cmd::Export(spektr_engine::export::Format::Md))
        );
        assert!(parse("export pdf").is_err());
        assert_eq!(parse("bogus"), Err("unknown command `bogus`".into()));
        assert_eq!(parse("region gb"), Ok(Cmd::Region("GB".into())));
    }
}
