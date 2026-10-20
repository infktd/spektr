//! `spektr module …` (MODULES §9): scaffold, test, check, install, list,
//! info, enable and disable exec modules.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use spektr_engine::exec::ExecModule;
use spektr_engine::pin::{self, PinState};
use spektr_engine::{
    ActionOutcome, CaseInit, Engine, EngineAction, EngineConfig, EventKind, JobState, Query,
    QueryResult,
};
use spektr_model::EntityRef;
use spektr_protocol::Manifest;

use crate::cli::{ModuleCmd, ModuleLang};
use crate::wiring::Ctx;

const TPL_MANIFEST: &str = include_str!("../../templates/module/module.toml.tmpl");
const TPL_PYTHON: &str = include_str!("../../templates/module/python.py.tmpl");
const TPL_SHELL: &str = include_str!("../../templates/module/shell.sh.tmpl");
const TPL_CARGO: &str = include_str!("../../templates/module/Cargo.toml.tmpl");
const TPL_RUST: &str = include_str!("../../templates/module/main.rs.tmpl");
const TPL_README: &str = include_str!("../../templates/module/README.md.tmpl");
const TPL_FIXTURE: &str = include_str!("../../templates/module/fixture.jsonl.tmpl");

/// Every technique, no automatic expansion: `module test` runs only the
/// module under test, whatever its class.
const TEST_PROFILE: &str = r#"[profile]
name = "module-test"
description = "spektr module test: every technique, no automatic expansion"
techniques = ["passive", "probing", "active"]
default_depth = 0
auto = false
"#;

pub async fn run(ctx: &Ctx, cmd: ModuleCmd) -> Result<bool> {
    match cmd {
        ModuleCmd::New { name, lang, dir } => new(&name, lang, dir.as_deref()).map(|()| true),
        ModuleCmd::Test {
            name,
            input,
            record,
            fixture,
        } => test(ctx, &name, &input, record, fixture.as_deref()).await,
        ModuleCmd::Check { name } => check(ctx, &name).await,
        ModuleCmd::Install {
            source,
            update,
            yes,
        } => install(ctx, &source, update, yes).map(|()| true),
        ModuleCmd::List => list(ctx).map(|()| true),
        ModuleCmd::Info { name } => info(ctx, &name).map(|()| true),
        ModuleCmd::Enable { name } => toggle(ctx, &name, false).map(|()| true),
        ModuleCmd::Disable { name } => toggle(ctx, &name, true).map(|()| true),
        ModuleCmd::ReplayFixture { file } => replay(&file).map(|()| true),
    }
}

/// A directory removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let p = std::env::temp_dir().join(format!(
            "spektr-module-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&p)?;
        Ok(Self(p))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fill(tpl: &str, vars: &[(&str, &str)]) -> String {
    let mut s = tpl.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `module new <name> --lang …`: a working module in `./<name>/`.
fn new(name: &str, lang: ModuleLang, dir: Option<&Path>) -> Result<()> {
    if !valid_name(name) {
        bail!("a module name is lowercase letters, digits and `-`, starting with a letter");
    }
    let root = dir.map_or_else(|| PathBuf::from(name), |d| d.join(name));
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    std::fs::create_dir_all(root.join("fixtures"))?;
    let (command, lang_name) = match lang {
        ModuleLang::Python => (
            r#"["python3", "main.py"]"#.to_string(),
            "Python, standard library only",
        ),
        ModuleLang::Shell => (
            r#"["sh", "main.sh"]"#.to_string(),
            "shell, with jq and curl",
        ),
        ModuleLang::Rust => (
            r#"["cargo", "run", "--quiet", "--release", "--"]"#.to_string(),
            "Rust, on spektr-protocol",
        ),
    };
    let vars = [
        ("name", name),
        ("command", command.as_str()),
        ("lang", lang_name),
        ("protocol_version", env!("CARGO_PKG_VERSION")),
    ];
    std::fs::write(root.join("module.toml"), fill(TPL_MANIFEST, &vars))?;
    std::fs::write(root.join("README.md"), fill(TPL_README, &vars))?;
    std::fs::write(
        root.join("fixtures/username_example.jsonl"),
        fill(TPL_FIXTURE, &vars),
    )?;
    match lang {
        ModuleLang::Python => std::fs::write(root.join("main.py"), fill(TPL_PYTHON, &vars))?,
        ModuleLang::Shell => std::fs::write(root.join("main.sh"), fill(TPL_SHELL, &vars))?,
        ModuleLang::Rust => {
            std::fs::create_dir_all(root.join("src"))?;
            std::fs::write(root.join("Cargo.toml"), fill(TPL_CARGO, &vars))?;
            std::fs::write(root.join("src/main.rs"), fill(TPL_RUST, &vars))?;
        }
    }
    // the scaffold must be a valid module
    let m = read_manifest(&root)?;
    println!("created {} ({lang_name})", root.display());
    println!(
        "  {} · consumes {} · technique {}",
        m.name,
        m.consumes.join(", "),
        m.technique.as_str()
    );
    println!("  next: edit PROFILE_URL and `hosts`, then");
    println!(
        "        spektr module test {} --input username:<handle>",
        root.display()
    );
    Ok(())
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    let p = dir.join("module.toml");
    let src = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    Manifest::from_toml(&src).with_context(|| p.display().to_string())
}

/// A path to a module directory, or an installed module's name.
fn resolve(ctx: &Ctx, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.join("module.toml").exists() {
        return Ok(p.to_path_buf());
    }
    let installed = ctx.paths.modules_dir().join(name);
    if installed.join("module.toml").exists() {
        return Ok(installed);
    }
    bail!("no module `{name}` (a directory with module.toml, or an installed module)")
}

/// A throwaway case with only `module` registered, under the test profile.
async fn harness(ctx: &Ctx, module: ExecModule) -> Result<(spektr_engine::EngineHandle, TempDir)> {
    let tmp = TempDir::new()?;
    let cfg_dir = tmp.path().join("config");
    std::fs::create_dir_all(cfg_dir.join("profiles"))?;
    std::fs::write(cfg_dir.join("profiles/module-test.toml"), TEST_PROFILE)?;
    let mut cfg = EngineConfig::new(ctx.store_options()?);
    cfg.config_dir = Some(cfg_dir);
    cfg.secrets = ctx.engine_config()?.secrets;
    cfg.module_config = ctx.config.module_config();
    let mut init = CaseInit::named("module-test");
    init.rules_profile = "module-test".into();
    let engine = Engine::create(&tmp.path().join("module-test.spektr"), &init, cfg).await?;
    engine.register(Box::new(module))?;
    Ok((engine.start(), tmp))
}

/// Run `module` once on `input` through the engine's validation; print what
/// was accepted and rejected. `false` when anything was rejected or failed.
async fn validate(ctx: &Ctx, module: ExecModule, input: &EntityRef, quiet: bool) -> Result<bool> {
    let name = module_name(&module);
    let (h, _tmp) = harness(ctx, module).await?;
    let mut rx = h.subscribe();
    let seeded = h
        .act(EngineAction::Seed {
            entity: input.clone(),
        })
        .await?;
    let ActionOutcome::Seeded { entity, .. } = seeded else {
        bail!("couldn't seed {input}");
    };
    h.act(EngineAction::Run {
        module: name.clone(),
        entity: entity.id,
        rerun: true,
    })
    .await?;
    h.wait_idle(Duration::from_secs(120)).await?;
    let mut rejected = Vec::new();
    let mut failure = None;
    let mut done = false;
    while let Ok(ev) = rx.try_recv() {
        match ev.kind {
            EventKind::MessageRejected { reason, .. } => rejected.push(reason),
            EventKind::JobFinished {
                state, error, note, ..
            } => match state {
                JobState::Done => {
                    done = true;
                    if let Some(n) = note
                        && !quiet
                    {
                        println!("  note       {n}");
                    }
                }
                JobState::Failed => failure = Some(error.unwrap_or_else(|| "failed".into())),
                _ => {}
            },
            EventKind::JobBlocked { reason, .. } => failure = Some(format!("blocked: {reason}")),
            _ => {}
        }
    }
    let QueryResult::Entities(ents) = h.query(Query::Entities).await? else {
        bail!("unexpected reply");
    };
    let QueryResult::Ego(ego) = h
        .query(Query::Ego {
            focus: entity.id,
            params: Default::default(),
        })
        .await?
    else {
        bail!("unexpected reply");
    };
    if !quiet {
        for e in ents.iter().filter(|e| e.id != entity.id) {
            println!("  ✓ entity   {}:{}", e.ty.as_str(), e.value);
        }
        for g in &ego.groups {
            for r in &g.rows {
                println!(
                    "  ✓ edge     {input} {} {}:{}",
                    g.key.predicate.as_str(),
                    r.entity.ty.as_str(),
                    r.entity.value
                );
            }
        }
        for r in &rejected {
            println!("  ✕ rejected {r}");
        }
    }
    match (&failure, done) {
        (Some(f), _) => println!("✕ {name} failed: {f}"),
        (None, true) if rejected.is_empty() => println!(
            "✓ {name}: {} entities, {} edges, every message valid",
            ents.len().saturating_sub(1),
            ego.groups.iter().map(|g| g.rows.len()).sum::<usize>()
        ),
        (None, true) => println!("✕ {name}: {} message(s) rejected", rejected.len()),
        (None, false) => println!("✕ {name}: the job didn't finish"),
    }
    drop(h);
    Ok(failure.is_none() && done && rejected.is_empty())
}

fn module_name(m: &ExecModule) -> String {
    use spektr_engine::Module;
    m.manifest().name.clone()
}

fn exec(ctx: &Ctx, dir: &Path, manifest: Manifest) -> Result<ExecModule> {
    let env: std::collections::BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| {
            matches!(
                k.as_str(),
                "PATH" | "HOME" | "LANG" | "TMPDIR" | "CARGO_HOME" | "RUSTUP_HOME"
            )
        })
        .collect();
    let secrets = ctx.engine_config()?.secrets.into_values().collect();
    Ok(ExecModule::new(manifest, dir.to_path_buf(), env, secrets))
}

/// The module, but its stdout replays `fixture` (job ids rewritten).
fn replaying(ctx: &Ctx, dir: &Path, mut manifest: Manifest, fixture: &Path) -> Result<ExecModule> {
    let me = std::env::current_exe()?;
    let fixture = std::fs::canonicalize(fixture).with_context(|| fixture.display().to_string())?;
    manifest.command = vec![
        me.display().to_string(),
        "module".into(),
        "replay-fixture".into(),
        fixture.display().to_string(),
    ];
    manifest.serve = false;
    exec(ctx, dir, manifest)
}

async fn test(
    ctx: &Ctx,
    name: &str,
    input: &str,
    record: bool,
    fixture: Option<&Path>,
) -> Result<bool> {
    let dir = resolve(ctx, name)?;
    let manifest = read_manifest(&dir)?;
    let input: EntityRef = input
        .parse()
        .with_context(|| format!("--input `{input}`"))?;
    if !manifest.consumes.iter().any(|c| c == input.ty.as_str()) {
        bail!("{} doesn't consume {}", manifest.name, input.ty.as_str());
    }
    if let Some(f) = fixture {
        println!(
            "{} · replaying {} through validation",
            manifest.name,
            f.display()
        );
        let m = replaying(ctx, &dir, manifest, f)?;
        return validate(ctx, m, &input, false).await;
    }
    if ctx.test.test_mode {
        bail!("a live module test runs the module against the network; in test mode use --fixture");
    }
    if record {
        let lines = record_run(&dir, &manifest, &input)?;
        let file = dir.join("fixtures").join(format!(
            "{}_{}.jsonl",
            input.ty.as_str(),
            input.value.replace(['/', ':', '@', ' '], "_")
        ));
        std::fs::create_dir_all(dir.join("fixtures"))?;
        std::fs::write(&file, lines.join("\n") + "\n")?;
        println!("recorded {} message(s) to {}", lines.len(), file.display());
        let m = replaying(ctx, &dir, manifest, &file)?;
        return validate(ctx, m, &input, false).await;
    }
    println!("{} · live run on {input}", manifest.name);
    let m = exec(ctx, &dir, manifest)?;
    validate(ctx, m, &input, false).await
}

/// Spawn the module directly and keep its stdout lines for one job.
fn record_run(dir: &Path, m: &Manifest, input: &EntityRef) -> Result<Vec<String>> {
    let (prog, args) = m.command.split_first().context("empty command")?;
    let mut child = Command::new(prog)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("starting `{}`", m.command.join(" ")))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let job = "j_record";
    let hello = json!({ "type": "hello", "protocol": 1, "spektr": env!("CARGO_PKG_VERSION") });
    let run = json!({
        "type": "run", "job": job,
        "input": { "type": input.ty.as_str(), "key": input.value, "value": input.value, "attrs": {} },
        "config": {}, "secrets": {}, "deadline": jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_secs(120)).map(|t| t.to_string()).unwrap_or_default(),
    });
    writeln!(stdin, "{hello}")?;
    writeln!(stdin, "{run}")?;
    let out = child.stdout.take().context("no stdout")?;
    let mut lines = Vec::new();
    for line in BufReader::new(out).lines() {
        let line = line?;
        let kind = serde_json::from_str::<serde_json::Value>(&line)
            .ok()
            .and_then(|v| v["type"].as_str().map(str::to_string));
        lines.push(line);
        if matches!(kind.as_deref(), Some("done" | "error")) {
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    Ok(lines)
}

/// Hidden: act as a module whose stdout is a fixture, rewriting job ids.
fn replay(file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file)?;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let msg: serde_json::Value = serde_json::from_str(&line?)?;
        if msg["type"] == "run" {
            let job = msg["job"].clone();
            for l in text.lines().filter(|l| !l.trim().is_empty()) {
                let mut v: serde_json::Value = serde_json::from_str(l)?;
                if v.get("job").is_some() {
                    v["job"] = job.clone();
                }
                writeln!(out, "{v}")?;
            }
            out.flush()?;
            break;
        }
    }
    Ok(())
}

async fn check(ctx: &Ctx, name: &str) -> Result<bool> {
    let dir = resolve(ctx, name)?;
    let mut ok = true;
    let mark = |good: bool| if good { "✓" } else { "✕" };
    // manifest
    let manifest = match read_manifest(&dir) {
        Ok(m) => {
            println!(
                "✓ manifest    {} {} · {} · consumes {}",
                m.name,
                m.version,
                m.technique.as_str(),
                m.consumes.join(", ")
            );
            m
        }
        Err(e) => {
            println!("✕ manifest    {e:#}");
            return Ok(false);
        }
    };
    if manifest.runtime != spektr_protocol::Runtime::Exec {
        println!("✕ runtime     only exec modules can be installed");
        return Ok(false);
    }
    // vocabulary: registering validates types, predicates and produces
    let probe = exec(ctx, &dir, manifest.clone())?;
    match harness(ctx, probe).await {
        Ok((h, _t)) => {
            println!("✓ vocabulary  declared types, predicates and produces are valid");
            drop(h);
        }
        Err(e) => {
            println!("✕ vocabulary  {e:#}");
            ok = false;
        }
    }
    // fixtures replay through validation
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(dir.join("fixtures"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                .collect()
        })
        .unwrap_or_default();
    fixtures.sort();
    if fixtures.is_empty() {
        println!("! fixtures    none in fixtures/ (spektr module test … --record)");
    }
    for f in &fixtures {
        let input = fixture_input(f, &manifest);
        let Some(input) = input else {
            println!(
                "! fixture     {}: name it <type>_<value>.jsonl",
                f.display()
            );
            continue;
        };
        let m = replaying(ctx, &dir, manifest.clone(), f)?;
        print!(
            "  fixture     {} ",
            f.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        );
        std::io::stdout().flush()?;
        let good = validate(ctx, m, &input, true).await?;
        ok &= good;
    }
    // secrets never in fixtures
    let secrets: Vec<String> = ctx
        .engine_config()?
        .secrets
        .iter()
        .filter(|(k, _)| manifest.secrets.contains(k) || manifest.optional_secrets.contains(k))
        .map(|(_, v)| v.clone())
        .filter(|v| v.len() >= 4)
        .collect();
    let leaked: Vec<String> = fixtures
        .iter()
        .filter(|f| {
            let t = std::fs::read_to_string(f).unwrap_or_default();
            secrets.iter().any(|s| t.contains(s.as_str()))
        })
        .map(|f| f.display().to_string())
        .collect();
    println!(
        "{} secrets     {}",
        mark(leaked.is_empty()),
        if leaked.is_empty() {
            "no secret values in fixtures".to_string()
        } else {
            format!("a secret value appears in {}", leaked.join(", "))
        }
    );
    ok &= leaked.is_empty();
    // the process exits once the host closes stdin
    if manifest.command.first().is_some_and(|c| c == "cargo") {
        println!("! exit        skipped for cargo-run modules (build first)");
    } else {
        let (prog, args) = manifest.command.split_first().context("empty command")?;
        let started = Command::new(prog)
            .args(args)
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match started {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = writeln!(
                        stdin,
                        "{}",
                        json!({ "type": "hello", "protocol": 1, "spektr": env!("CARGO_PKG_VERSION") })
                    );
                }
                let t0 = Instant::now();
                let exited = loop {
                    if child.try_wait()?.is_some() {
                        break true;
                    }
                    if t0.elapsed() > Duration::from_secs(2) {
                        let _ = child.kill();
                        break false;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                };
                println!(
                    "{} exit        {}",
                    mark(exited),
                    if exited {
                        "exits within 2 s once stdin closes"
                    } else {
                        "still running 2 s after stdin closed (the host kills it)"
                    }
                );
                ok &= exited;
            }
            Err(e) => {
                println!(
                    "✕ exit        can't start `{}`: {e}",
                    manifest.command.join(" ")
                );
                ok = false;
            }
        }
    }
    Ok(ok)
}

/// `<type>_<value>.jsonl` → the fixture's input.
fn fixture_input(f: &Path, m: &Manifest) -> Option<EntityRef> {
    let stem = f.file_stem()?.to_string_lossy().into_owned();
    let (ty, value) = stem.split_once('_')?;
    if !m.consumes.iter().any(|c| c == ty) {
        return None;
    }
    format!("{ty}:{value}").parse().ok()
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if matches!(
            name.to_str(),
            Some(".git" | "target" | "__pycache__" | pin::PIN_FILE)
        ) {
            continue;
        }
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_dir(&e.path(), &to.join(&name))?;
        } else if ft.is_file() {
            std::fs::copy(e.path(), to.join(&name))?;
        }
    }
    Ok(())
}

fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        bail!("{question} needs confirmation: run it in a terminal, or pass --yes");
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut a = String::new();
    std::io::stdin().read_line(&mut a)?;
    Ok(matches!(a.trim(), "y" | "Y" | "yes"))
}

fn describe(m: &Manifest) {
    println!("{} {} · {}", m.name, m.version, m.description);
    println!(
        "  technique {} · consumes {} · produces {}",
        m.technique.as_str(),
        m.consumes.join(", "),
        m.produces
            .iter()
            .map(|(p, t)| format!("{p} {t}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  command   {}", m.command.join(" "));
    println!(
        "  hosts     {}",
        if m.hosts.is_empty() {
            "none declared".into()
        } else {
            m.hosts.join(", ")
        }
    );
    let mut secrets: Vec<String> = m.secrets.clone();
    secrets.extend(m.optional_secrets.iter().map(|s| format!("{s} (optional)")));
    println!(
        "  secrets   {}",
        if secrets.is_empty() {
            "none".into()
        } else {
            secrets.join(", ")
        }
    );
}

fn install(ctx: &Ctx, source: &str, update: bool, yes: bool) -> Result<()> {
    let modules = ctx.paths.modules_dir();
    if update {
        let dir = modules.join(source);
        let m = read_manifest(&dir)?;
        describe(&m);
        let state = pin::pin_state(&dir);
        println!(
            "  files     {}",
            match state {
                PinState::Changed => "changed since install",
                PinState::Pinned => "unchanged",
                PinState::Unpinned => "not pinned yet",
            }
        );
        println!(
            "\nExec modules run with your permissions: approving one means trusting its code."
        );
        if !confirm(&format!("Approve {} as it is now?", m.name), yes)? {
            bail!("not approved");
        }
        let h = pin::write_pin(&dir)?;
        println!("✓ {} approved · tree {}", m.name, &h[..12]);
        return Ok(());
    }
    // a path, or a git URL cloned into a temp dir
    let tmp;
    let src: PathBuf =
        if source.starts_with("https://") || source.starts_with("git@") || source.ends_with(".git")
        {
            if ctx.test.test_mode {
                bail!("installing from git fetches from the network; it doesn't run in test mode");
            }
            tmp = TempDir::new()?;
            let dest = tmp.path().join("module");
            let st = Command::new("git")
                .args(["clone", "--depth", "1", source])
                .arg(&dest)
                .status()
                .context("running git")?;
            if !st.success() {
                bail!("git clone {source} failed");
            }
            dest
        } else {
            PathBuf::from(source)
        };
    let m = read_manifest(&src)?;
    if m.runtime != spektr_protocol::Runtime::Exec {
        bail!("only exec modules can be installed");
    }
    let dest = modules.join(&m.name);
    if dest.exists() {
        bail!(
            "{} is already installed (spektr module install --update {} re-approves its files)",
            m.name,
            m.name
        );
    }
    describe(&m);
    println!("\nExec modules run with your permissions: installing one means trusting its code.");
    if !confirm(&format!("Install {}?", m.name), yes)? {
        bail!("not installed");
    }
    copy_dir(&src, &dest)?;
    let h = pin::write_pin(&dest)?;
    println!(
        "✓ installed {} to {} · tree {}",
        m.name,
        crate::paths::tilde(&dest),
        &h[..12]
    );
    Ok(())
}

fn list(ctx: &Ctx) -> Result<()> {
    let off = pin::disabled(&ctx.paths.modules_dir());
    let net = spektr_modules::net::Net::replay(spektr_modules::net::Fixtures::default());
    for m in spektr_modules::builtins_with(net, Default::default()) {
        let mf = m.manifest();
        let state = if off.contains(&mf.name) {
            "disabled"
        } else {
            "enabled"
        };
        println!(
            "{:<20} {:<8} built-in  {:<8} {state}",
            mf.name,
            mf.version,
            mf.technique.as_str()
        );
    }
    let dir = ctx.paths.modules_dir();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.join("module.toml").exists())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    for d in dirs {
        match read_manifest(&d) {
            Ok(m) => {
                let state = if off.contains(&m.name) {
                    "disabled"
                } else {
                    match pin::pin_state(&d) {
                        PinState::Pinned => "enabled",
                        PinState::Unpinned => "enabled · not pinned",
                        PinState::Changed => "changed · needs install --update",
                    }
                };
                println!(
                    "{:<20} {:<8} exec      {:<8} {state}",
                    m.name,
                    m.version,
                    m.technique.as_str()
                );
            }
            Err(e) => println!(
                "{:<20} ✕ {e:#}",
                d.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            ),
        }
    }
    Ok(())
}

fn info(ctx: &Ctx, name: &str) -> Result<()> {
    if let Ok(dir) = resolve(ctx, name) {
        let m = read_manifest(&dir)?;
        describe(&m);
        println!("  dir       {}", crate::paths::tilde(&dir));
        return Ok(());
    }
    let net = spektr_modules::net::Net::replay(spektr_modules::net::Fixtures::default());
    let m = spektr_modules::builtins_with(net, Default::default())
        .into_iter()
        .find(|m| m.manifest().name == name)
        .with_context(|| format!("no module `{name}`"))?;
    describe(m.manifest());
    println!("  built-in");
    Ok(())
}

fn toggle(ctx: &Ctx, name: &str, off: bool) -> Result<()> {
    let known_exec = ctx
        .paths
        .modules_dir()
        .join(name)
        .join("module.toml")
        .exists();
    let net = spektr_modules::net::Net::replay(spektr_modules::net::Fixtures::default());
    let known_builtin = spektr_modules::builtins_with(net, Default::default())
        .iter()
        .any(|m| m.manifest().name == name);
    if !known_exec && !known_builtin {
        bail!("no module `{name}`");
    }
    pin::set_disabled(&ctx.paths.modules_dir(), name, off)?;
    println!("{} {name}", if off { "disabled" } else { "enabled" });
    Ok(())
}
