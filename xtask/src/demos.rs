//! `cargo xtask demos`: render every `demos/*.tape` with VHS into
//! `demos/out/<name>.gif`, on the fictional fixture case: test mode, a fixed
//! clock, network replayed from `crates/spektr-modules/fixtures`.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run(root: &Path) -> Result<()> {
    if Command::new("vhs").arg("--version").output().is_err() {
        bail!("VHS isn't installed (https://github.com/charmbracelet/vhs; it needs ttyd and ffmpeg)");
    }
    let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args(["build", "--release", "-q", "-p", "spektr"])
        .status()?;
    if !st.success() {
        bail!("cargo build --release failed");
    }
    let bin_dir = root.join("target/release");
    let out = root.join("demos/out");
    std::fs::create_dir_all(&out)?;
    let mut tapes: Vec<_> = std::fs::read_dir(root.join("demos"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "tape"))
        .collect();
    tapes.sort();
    if tapes.is_empty() {
        bail!("no demos/*.tape");
    }
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for tape in &tapes {
        let name = tape.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let work = std::env::temp_dir().join(format!("spektr-demo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let home = work.join("home");
        std::fs::create_dir_all(&home)?;
        let st = Command::new("vhs")
            .arg(tape)
            .current_dir(&work)
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("SPEKTR_TEST_MODE", "1")
            .env(
                "SPEKTR_TRANSPORT",
                format!("replay:{}", root.join("crates/spektr-modules/fixtures").display()),
            )
            .env("SPEKTR_CLOCK", "2026-10-20T18:14:22Z")
            .env("SPEKTR_SEED", "1")
            .env("COLORTERM", "truecolor")
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .status()
            .with_context(|| format!("running vhs {}", tape.display()))?;
        if !st.success() {
            bail!("vhs {} failed", tape.display());
        }
        let gif = work.join(format!("{name}.gif"));
        let size = std::fs::metadata(&gif)
            .with_context(|| format!("{} wrote no {name}.gif", tape.display()))?
            .len();
        if size == 0 {
            bail!("{name}.gif is empty");
        }
        std::fs::copy(&gif, out.join(format!("{name}.gif")))?;
        let _ = std::fs::remove_dir_all(&work);
        println!("demos/out/{name}.gif · {} KB", size / 1024);
    }
    Ok(())
}
