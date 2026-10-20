//! `cargo xtask qa [--live]` and `cargo xtask qa-shots` (docs/QA.md §3–4).
//!
//! `qa` runs `qa/scenarios/*.toml` through the PTY harness (the ignored
//! `qa` test in the binary crate) and saves frames to `qa/out/`. `--live`
//! runs real-network smoke checks against `qa/live-targets.toml` only.
//! `qa-shots` renders every saved `.html` frame, and the design artboards,
//! to PNG with headless Chrome.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// `qa/live-targets.toml`: written by Jay, git-ignored. Nothing else is ever
/// queried live.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveTargets {
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub usernames: Vec<String>,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(skip)]
    pub from_file: bool,
}

impl LiveTargets {
    /// The file, or `example.com` alone when it's missing or empty.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("qa/live-targets.toml");
        let mut t = match std::fs::read_to_string(&path) {
            Ok(s) => {
                let mut t: LiveTargets =
                    toml::from_str(&s).with_context(|| format!("reading {}", path.display()))?;
                t.from_file = true;
                t
            }
            Err(_) => LiveTargets::default(),
        };
        if t.domains.is_empty() && t.usernames.is_empty() && t.emails.is_empty() {
            t.domains = vec!["example.com".into()];
        }
        Ok(t)
    }

    pub fn allows(&self, input: &str) -> bool {
        match input.split_once(':') {
            Some(("domain" | "subdomain", d)) => {
                self.domains.iter().any(|x| x.eq_ignore_ascii_case(d))
            }
            Some(("username", u)) => self.usernames.iter().any(|x| x.eq_ignore_ascii_case(u)),
            Some(("email", e)) => self.emails.iter().any(|x| x.eq_ignore_ascii_case(e)),
            _ => false,
        }
    }
}

pub fn run(root: &Path, live: bool) -> Result<()> {
    if live {
        return run_live(root);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = root.join("qa/out");
    std::fs::create_dir_all(&out)?;
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "test",
            "-q",
            "-p",
            "spektr",
            "--test",
            "qa",
            "--",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("SPEKTR_QA_SCENARIOS", root.join("qa/scenarios"))
        .env("SPEKTR_QA_OUT", &out)
        .status()
        .context("running the qa scenarios")?;
    if !status.success() {
        bail!("qa scenarios failed");
    }
    println!(
        "frames in {}; next: cargo xtask qa-shots, then review every PNG against its artboard",
        out.display()
    );
    Ok(())
}

fn chrome() -> Option<PathBuf> {
    for name in ["google-chrome", "chromium", "chromium-browser", "chrome"] {
        if let Ok(out) = Command::new("which").arg(name).output()
            && out.status.success()
        {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(p.into());
            }
        }
    }
    let mac = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    mac.exists().then_some(mac)
}

fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            html_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "html") {
            out.push(p);
        }
    }
}

/// Window size for a frame from its `<meta name="spektr-size" content="CxR">`.
fn frame_window(html: &str) -> Option<(u32, u32)> {
    let i = html.find("name=\"spektr-size\" content=\"")? + "name=\"spektr-size\" content=\"".len();
    let rest = &html[i..];
    let (c, r) = rest[..rest.find('"')?].split_once('x')?;
    let (c, r): (u32, u32) = (c.parse().ok()?, r.parse().ok()?);
    // 13 px JetBrains Mono advances ~7.83 px per column; 20 px per row.
    Some(((f64::from(c) * 7.83).ceil() as u32 + 2, r * 20 + 2))
}

fn screenshot(chrome: &Path, html: &Path, png: &Path, (w, h): (u32, u32)) -> Result<()> {
    let url = format!("file://{}", html.canonicalize()?.display());
    let status = Command::new(chrome)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--force-device-scale-factor=2",
            "--virtual-time-budget=3000",
        ])
        .arg(format!("--window-size={w},{h}"))
        .arg(format!("--screenshot={}", png.display()))
        .arg(url)
        .output()
        .context("running Chrome")?;
    if !status.status.success() {
        bail!(
            "Chrome failed on {}: {}",
            html.display(),
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

pub fn shots(root: &Path) -> Result<()> {
    let Some(chrome) = chrome() else {
        bail!("no Chrome or Chromium found (install Chrome, or `spektr setup capture` in Phase 4)");
    };
    let out = root.join("qa/out");
    let mut frames = Vec::new();
    html_files(&out, &mut frames);
    frames.retain(|p| !p.starts_with(out.join("artboards")));
    frames.sort();
    for f in &frames {
        let html = std::fs::read_to_string(f)?;
        let size = frame_window(&html).unwrap_or((1340, 962));
        screenshot(&chrome, f, &f.with_extension("png"), size)?;
        println!(
            "{}",
            f.with_extension("png")
                .strip_prefix(root)
                .unwrap_or(f)
                .display()
        );
    }
    let boards = out.join("artboards");
    let srcs = boards.join("src");
    std::fs::create_dir_all(&srcs)?;
    let mut designs: Vec<PathBuf> = std::fs::read_dir(root.join("docs/design"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .collect();
    designs.sort();
    for d in &designs {
        let name = d
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("artboard")
            .replace(".dc.html", "");
        let html = std::fs::read_to_string(d)?;
        let size = artboard_size(&html).unwrap_or((1440, 900));
        let copy = srcs.join(format!("{name}.html"));
        std::fs::write(&copy, prepare_artboard(&html))?;
        let png = boards.join(format!("{name}.png"));
        screenshot(&chrome, &copy, &png, size)?;
        println!("{}", png.strip_prefix(root).unwrap_or(&png).display());
    }
    println!("{} frames, {} artboards", frames.len(), designs.len());
    Ok(())
}

/// The artboards' canvas size: the first `width: Npx; height: Mpx`.
fn artboard_size(html: &str) -> Option<(u32, u32)> {
    let i = html.find("<div style=\"")? + "<div style=\"".len();
    let style = &html[i..i + html[i..].find('"')?];
    let px = |prop: &str| -> Option<u32> {
        let j = style.find(&format!("{prop}: "))? + prop.len() + 2;
        let rest = &style[j..];
        rest[..rest.find("px")?].trim().parse().ok()
    };
    Some((px("width")?, px("height")?))
}

/// The design canvas relies on a `support.js` (not in the repo) that drops
/// whitespace-only text between tags; inside `white-space: pre` containers
/// those newlines would otherwise render as blank rows. Do the same here.
fn prepare_artboard(html: &str) -> String {
    let html = html.replace("<script src=\"./support.js\"></script>", "");
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(i) = rest.find('>') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        let ws = rest.len() - rest.trim_start().len();
        if rest[..ws].contains('\n') && rest[ws..].starts_with('<') {
            rest = &rest[ws..];
        }
    }
    out.push_str(rest);
    out
}

/// One module's live result.
struct LiveResult {
    module: String,
    target: String,
    ok: bool,
    latency_ms: u128,
    detail: String,
    shape: String,
}

fn run_live(root: &Path) -> Result<()> {
    let targets = LiveTargets::load(root)?;
    let bin = crate::record::build_spektr(root)?;
    let work = root.join("qa/out/live");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let home = work.join("home");
    std::fs::create_dir_all(&home)?;
    let spektr = |args: &[&str]| -> Command {
        let mut c = Command::new(&bin);
        c.current_dir(&work)
            .args(args)
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CONFIG_HOME", home.join("config"));
        c.env_remove("SPEKTR_TEST_MODE")
            .env_remove("SPEKTR_TRANSPORT");
        c
    };
    let mut results = Vec::new();
    let mut plan: Vec<(String, String)> = Vec::new();
    for d in &targets.domains {
        plan.push(("dns".into(), format!("domain:{d}")));
    }
    for u in &targets.usernames {
        plan.push(("github".into(), format!("username:{u}")));
        plan.push(("username-sweep".into(), format!("username:{u}")));
    }
    for (i, (module, input)) in plan.iter().enumerate() {
        if !targets.allows(input) {
            bail!("refusing {input}: not in qa/live-targets.toml");
        }
        let name = format!("live-{i}");
        let st = spektr(&["new", &name, "--profile", "tracelabs"]).status()?;
        if !st.success() {
            bail!("spektr new failed");
        }
        let case = format!("{name}.spektr");
        let t = Instant::now();
        let out = spektr(&[
            "run",
            &case,
            "--seed",
            input,
            "--module",
            module,
            "--depth",
            "0",
            "--json",
            "--timeout",
            "15m",
        ])
        .output()?;
        let latency_ms = t.elapsed().as_millis();
        let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let finished = lines.iter().find(|l| {
            l["kind"]["event"] == "job_finished" && l["kind"]["module"] == module.as_str()
        });
        let state = finished
            .and_then(|f| f["kind"]["state"].as_str())
            .unwrap_or("none")
            .to_string();
        let note = finished
            .and_then(|f| f["kind"]["note"].as_str())
            .unwrap_or("")
            .to_string();
        let error = finished
            .and_then(|f| f["kind"]["error"].as_str())
            .unwrap_or("")
            .to_string();
        // Shape check: what the module wrote, compared with what its fixtures produce.
        let q = spektr(&["query", &case, "--neighbors", input, "--json"]).output()?;
        let rows: Vec<serde_json::Value> = String::from_utf8_lossy(&q.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let mut preds: BTreeMap<String, usize> = BTreeMap::new();
        for r in &rows {
            *preds
                .entry(r["predicate"].as_str().unwrap_or("?").to_string())
                .or_default() += 1;
        }
        let shape = match module.as_str() {
            "dns" if preds.contains_key("resolves_to") => {
                "matches fixtures (resolves_to ip)".to_string()
            }
            "dns" => {
                "no resolves_to edges — compare with crates/spektr-modules/fixtures/dns".to_string()
            }
            _ if state == "done" => format!("edges: {preds:?}"),
            _ => "n/a".into(),
        };
        let detail = if !error.is_empty() {
            error
        } else if !note.is_empty() {
            note
        } else {
            state.clone()
        };
        results.push(LiveResult {
            module: module.clone(),
            target: input.clone(),
            ok: state == "done",
            latency_ms,
            detail,
            shape,
        });
    }
    let mut md = String::from("# Live smoke run\n\n");
    md.push_str(&format!(
        "Targets: {} (from {}).\n\n",
        plan.iter()
            .map(|(_, t)| t.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", "),
        if targets.from_file {
            "qa/live-targets.toml"
        } else {
            "the default: qa/live-targets.toml is missing or empty, so example.com only"
        }
    ));
    md.push_str("| Module | Target | Result | Latency | Detail | Response shape |\n|---|---|---|---|---|---|\n");
    for r in &results {
        md.push_str(&format!(
            "| {} | {} | {} | {} ms | {} | {} |\n",
            r.module,
            r.target,
            if r.ok { "ok" } else { "failed" },
            r.latency_ms,
            r.detail.replace('|', "\\|"),
            r.shape
        ));
    }
    if targets.emails.is_empty() {
        md.push_str("\nNo emails approved; no Phase 2 module consumes email anyway.\n");
    }
    if targets.usernames.is_empty() {
        md.push_str("No usernames approved: `github` and `username-sweep` were not run live.\n");
    }
    std::fs::write(work.join("report.md"), &md)?;
    print!("{md}");
    if results.iter().any(|r| !r.ok) {
        bail!("live smoke run had failures (see qa/out/live/report.md)");
    }
    Ok(())
}

/// Window id and owner pid of every on-screen window of `owner`.
const WINDOWS_SWIFT: &str = r#"
import CoreGraphics
if let list = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] {
  for w in list where (w[kCGWindowOwnerName as String] as? String) == CommandLine.arguments[1] {
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    print(w[kCGWindowNumber as String] ?? 0, w[kCGWindowOwnerPID as String] ?? 0, b["Width"] ?? 0, b["Height"] ?? 0)
  }
}
"#;

/// `(window id, pid, width, height)` for `owner`'s windows.
fn windows(script: &Path, owner: &str) -> Vec<(u64, u32, f64, f64)> {
    let Ok(out) = Command::new("swift").arg(script).arg(owner).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            Some((
                v.first()?.parse().ok()?,
                v.get(1)?.parse().ok()?,
                v.get(2)?.parse().ok()?,
                v.get(3)?.parse().ok()?,
            ))
        })
        .collect()
}

/// QA.md §5: run the TUI in Ghostty on the Phase 2 fixture case and capture
/// the window with `screencapture -l`. Ghostty asks once whether it may run
/// the session script; the capture waits for that.
pub fn real_terminal(root: &Path) -> Result<()> {
    let app = Path::new("/Applications/Ghostty.app");
    if !app.exists() {
        bail!("Ghostty isn't installed in /Applications (iTerm2 support: not yet)");
    }
    let bin_status = Command::new("cargo")
        .args(["build", "-q", "-p", "spektr"])
        .current_dir(root)
        .status()?;
    if !bin_status.success() {
        bail!("cargo build failed");
    }
    let bin = root.join("target/debug/spektr");
    let out = root.join("qa/out/real-terminal");
    let _ = std::fs::remove_dir_all(&out);
    let work = out.join("work");
    let home = out.join("home");
    std::fs::create_dir_all(&work)?;
    std::fs::create_dir_all(&home)?;
    let fixtures = root.join("crates/spektr-modules/fixtures");
    let env = format!(
        "export HOME={h} XDG_CONFIG_HOME={h}/.config XDG_STATE_HOME={h}/.local/state XDG_DATA_HOME={h}/.local/share\n\
         export SPEKTR_TEST_MODE=1 SPEKTR_TRANSPORT=replay:{f} SPEKTR_CLOCK=2026-10-20T18:14:22Z SPEKTR_SEED=1 COLORTERM=truecolor\n\
         cd {w}\n",
        h = home.display(),
        f = fixtures.display(),
        w = work.display()
    );
    let setup = format!(
        "{env}{b} new demo-missing-01 --profile tracelabs --subject 'Jordan Marlow' --last-seen 2026-09-28 >/dev/null && \
         {b} run demo-missing-01.spektr --seed username:jmarlow.wav >/dev/null\n",
        b = bin.display()
    );
    let st = Command::new("/bin/sh").arg("-c").arg(&setup).status()?;
    if !st.success() {
        bail!("building the fixture case failed");
    }
    let run_sh = out.join("run.sh");
    std::fs::write(
        &run_sh,
        format!("{env}exec {} open demo-missing-01.spektr\n", bin.display()),
    )?;
    let swift = out.join("windows.swift");
    std::fs::write(&swift, WINDOWS_SWIFT)?;
    let before: Vec<u32> = windows(&swift, "Ghostty")
        .into_iter()
        .map(|w| w.1)
        .collect();
    Command::new("open")
        .args([
            "-na",
            "/Applications/Ghostty.app",
            "--args",
            "--window-width=170",
            "--window-height=48",
        ])
        .args([
            "--font-family=JetBrains Mono",
            "--font-size=13",
            "--window-decoration=false",
            "-e",
            "/bin/sh",
        ])
        .arg(&run_sh)
        .status()?;
    println!(
        "Ghostty may ask to run /bin/sh: click Allow. Waiting up to 2 minutes for the window…"
    );
    let start = std::time::Instant::now();
    let found = loop {
        let ours: Vec<_> = windows(&swift, "Ghostty")
            .into_iter()
            .filter(|w| !before.contains(&w.1))
            .collect();
        if let Some(w) = ours.iter().find(|w| w.2 > 800.0) {
            break Some(*w);
        }
        if start.elapsed() > std::time::Duration::from_secs(120) {
            break ours.first().copied();
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    let Some((id, pid, w, h)) = found else {
        bail!("no Ghostty window appeared");
    };
    if w <= 800.0 {
        let _ = Command::new("kill").arg(pid.to_string()).status();
        bail!("only Ghostty's permission prompt appeared ({w}×{h}); run again and click Allow");
    }
    std::thread::sleep(std::time::Duration::from_secs(4));
    let png = out.join("ghostty-workspace.png");
    let st = Command::new("screencapture")
        .args(["-x", "-o", "-l", &id.to_string()])
        .arg(&png)
        .status()?;
    let _ = Command::new("kill").arg(pid.to_string()).status();
    if !st.success() || !png.exists() {
        bail!("screencapture failed (Screen Recording permission for this terminal?)");
    }
    println!("captured {} ({w}×{h} pt)", png.display());
    Ok(())
}
