//! `spektr setup capture`: download Chrome for Testing's headless shell
//! into the data dir, so capture can take screenshots without a system
//! Chrome. It runs only when asked, and uses the system `curl` and `unzip`.

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::wiring::Ctx;

const INDEX: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

fn platform() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "x86_64") => "linux64",
        (os, arch) => bail!(
            "Chrome for Testing has no headless shell for {os}/{arch}; install Chromium and spektr will find it"
        ),
    })
}

fn run(cmd: &mut std::process::Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("running {:?}", cmd.get_program()))?;
    if !status.success() {
        bail!("{:?} failed ({status})", cmd.get_program());
    }
    Ok(())
}

pub fn capture(ctx: &Ctx) -> Result<()> {
    if ctx.test.transport != crate::testenv::TransportSpec::Live {
        bail!("setup capture downloads from the network; it doesn't run in test mode");
    }
    let plat = platform()?;
    let dir = ctx.paths.data.join("chrome");
    std::fs::create_dir_all(&dir)?;
    println!("finding the current headless shell for {plat}…");
    let out = std::process::Command::new("curl")
        .args(["-fsSL", INDEX])
        .output()
        .context("running curl")?;
    if !out.status.success() {
        bail!("couldn't fetch {INDEX}");
    }
    let index: Value =
        serde_json::from_slice(&out.stdout).context("the Chrome for Testing index")?;
    let stable = &index["channels"]["Stable"];
    let version = stable["version"].as_str().unwrap_or("unknown");
    let url = stable["downloads"]["chrome-headless-shell"]
        .as_array()
        .and_then(|a| a.iter().find(|d| d["platform"] == plat))
        .and_then(|d| d["url"].as_str())
        .context("no headless shell download for this platform")?;
    let zip = dir.join(format!("{version}.zip"));
    println!("downloading chrome-headless-shell {version}…");
    run(std::process::Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(&zip)
        .arg(url))?;
    let dest = dir.join(version);
    let _ = std::fs::remove_dir_all(&dest);
    run(std::process::Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&zip)
        .arg("-d")
        .arg(&dest))?;
    let _ = std::fs::remove_file(&zip);
    match spektr_modules::find_browser(Some(&ctx.paths.data)) {
        Some(p) => println!("✓ {}", crate::paths::tilde(&p)),
        None => bail!(
            "downloaded, but the headless shell wasn't found under {}",
            dest.display()
        ),
    }
    Ok(())
}
