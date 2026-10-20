//! QA scenarios (QA.md §3): run `qa/scenarios/*.toml` through the PTY
//! harness and save frames. Ignored by default; `cargo xtask qa` runs it
//! with SPEKTR_QA_SCENARIOS and SPEKTR_QA_OUT set. The agent then reviews
//! every frame and answers each scenario's checklist in PHASE_N_QA.md.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use support::Home;
use support::term::{Size, Term};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    name: String,
    #[serde(default = "default_size")]
    size: [u16; 2],
    /// An artboard to compare the frames with.
    #[serde(default)]
    compare: Option<String>,
    #[serde(default)]
    checklist: Vec<String>,
    /// Written to config.toml before the first step.
    #[serde(default)]
    config: Option<String>,
    /// Generated cases: `[name, entities]`.
    #[serde(default)]
    synth: Vec<(String, u64)>,
    #[serde(rename = "step")]
    steps: Vec<Step>,
}

fn default_size() -> [u16; 2] {
    [120, 36]
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    /// `spektr` arguments.
    run: Vec<String>,
    /// Keys for the key DSL, sent after `wait`.
    #[serde(default)]
    keys: Option<String>,
    /// Text to wait for before keys / the frame.
    #[serde(default)]
    wait: Option<String>,
    /// Save a frame with this name once the step is done.
    #[serde(default)]
    frame: Option<String>,
    /// Expected exit code (default 0).
    #[serde(default)]
    exit: Option<u32>,
    /// An interactive session: actions run in order after `wait`/`keys`.
    /// `wait <text>`, `keys <dsl>`, `settle`, `frame <name>`, `click x y`,
    /// `dclick x y`, `rclick x y`, `hover x y`, `wheel x y up|down`,
    /// `resize w h`, `sleep <ms>`.
    #[serde(default, rename = "do")]
    actions: Vec<String>,
    /// Extra environment (an empty value unsets the variable).
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    /// Terminal size for this step (default: the scenario's).
    #[serde(default)]
    size: Option<[u16; 2]>,
    /// Break this case's audit chain (row 1) after the step: `name`.
    #[serde(default)]
    tamper: Option<String>,
}

fn act(t: &mut Term, a: &str, sdir: &std::path::Path, frames: &mut Vec<String>) {
    let (verb, rest) = a.split_once(' ').unwrap_or((a, ""));
    let nums: Vec<u16> = rest
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    match verb {
        "wait" => {
            t.wait_for_text(rest, Duration::from_secs(20))
                .unwrap_or_else(|e| panic!("{a}: {e:#}"));
        }
        "keys" => t.keys(rest).unwrap(),
        "settle" => {
            t.settle(Duration::from_millis(400), Duration::from_secs(20))
                .unwrap_or_else(|e| panic!("{a}: {e:#}"));
        }
        "frame" => {
            t.settle(Duration::from_millis(400), Duration::from_secs(20))
                .unwrap_or_else(|e| panic!("{a}: {e:#}"));
            t.save_frame(&sdir.join(rest)).unwrap();
            frames.push(rest.to_string());
        }
        "click" => t.mouse_click(nums[0], nums[1]).unwrap(),
        "dclick" => t.double_click(nums[0], nums[1]).unwrap(),
        "rclick" => t.right_click(nums[0], nums[1]).unwrap(),
        "hover" => t.hover(nums[0], nums[1]).unwrap(),
        "wheel" => t.wheel(nums[0], nums[1], rest.ends_with("down")).unwrap(),
        "resize" => t.resize(nums[0], nums[1]).unwrap(),
        "sleep" => std::thread::sleep(Duration::from_millis(u64::from(nums[0]))),
        other => panic!("unknown action `{other}`"),
    }
}

#[test]
#[ignore = "run with `cargo xtask qa`"]
fn qa_scenarios() {
    let dir = PathBuf::from(std::env::var("SPEKTR_QA_SCENARIOS").expect("SPEKTR_QA_SCENARIOS"));
    let out = PathBuf::from(std::env::var("SPEKTR_QA_OUT").expect("SPEKTR_QA_OUT"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut index = String::from("# QA frames\n\n");
    for file in files {
        let stem = file.file_stem().unwrap().to_string_lossy().into_owned();
        let sc: Scenario = toml::from_str(&std::fs::read_to_string(&file).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let home = Home::new();
        if let Some(c) = &sc.config {
            home.write_config(c);
        }
        for (name, n) in &sc.synth {
            home.synth_case(name, *n);
        }
        let size = Size {
            cols: sc.size[0],
            rows: sc.size[1],
        };
        let sdir = out.join(&stem);
        let _ = std::fs::remove_dir_all(&sdir);
        std::fs::create_dir_all(&sdir).unwrap();
        let mut frames = Vec::new();
        for (i, step) in sc.steps.iter().enumerate() {
            let args: Vec<&str> = step.run.iter().map(String::as_str).collect();
            let mut env = home.env();
            env.retain(|(k, _)| !step.env.contains_key(k));
            env.extend(step.env.iter().map(|(k, v)| (k.clone(), v.clone())));
            let size = step.size.map_or(size, |[cols, rows]| Size { cols, rows });
            let mut t = Term::spawn(&args, size, &env, &home.work()).unwrap();
            if let Some(w) = &step.wait {
                t.wait_for_text(w, Duration::from_secs(20))
                    .unwrap_or_else(|e| panic!("{stem} step {i}: {e:#}"));
            }
            if let Some(k) = &step.keys {
                t.keys(k).unwrap();
            }
            for a in &step.actions {
                act(&mut t, a, &sdir, &mut frames);
            }
            let code = t
                .wait_exit(Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("{stem} step {i}: {e:#}"));
            assert_eq!(
                code,
                step.exit.unwrap_or(0),
                "{stem} step {i} (`spektr {}`) exit code; screen:\n{}",
                step.run.join(" "),
                t.screen().contents
            );
            if let Some(f) = &step.frame {
                t.save_frame(&sdir.join(f)).unwrap();
                frames.push(f.clone());
            }
            if let Some(c) = &step.tamper {
                home.tamper(c, 1);
            }
        }
        let mut md = format!(
            "# {}\n\nScenario `{}` at {}×{}.\n\n",
            sc.name,
            file.display(),
            size.cols,
            size.rows
        );
        if let Some(c) = &sc.compare {
            md.push_str(&format!("Compare with `{c}`.\n\n"));
        }
        md.push_str("## Frames\n\n");
        for f in &frames {
            md.push_str(&format!("- `{f}.png` / `{f}.txt`\n"));
        }
        md.push_str("\n## Checklist (answer after looking at every frame)\n\n");
        for c in &sc.checklist {
            md.push_str(&format!("- [ ] {c}\n"));
        }
        std::fs::write(sdir.join("checklist.md"), &md).unwrap();
        index.push_str(&format!(
            "- {} ({}): {} frames\n",
            sc.name,
            stem,
            frames.len()
        ));
        println!("qa: {stem}: {} frames", frames.len());
    }
    std::fs::write(out.join("index.md"), index).unwrap();
}
