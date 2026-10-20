//! `spektr doctor`: config, secrets, binaries, terminal, DNS, tmux mouse,
//! modules.

use anyhow::Result;
use serde_json::json;

use crate::cli::DoctorArgs;
use crate::wiring::Ctx;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Check {
    pub name: String,
    pub level: Level,
    pub detail: String,
}

fn check(name: &str, level: Level, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        level,
        detail: detail.into(),
    }
}

use crate::paths::tilde;

pub fn run(ctx: &Ctx, a: DoctorArgs) -> Result<()> {
    let out = checks(ctx);
    if a.json {
        for c in &out {
            println!("{}", json!(c));
        }
    } else {
        println!("Doctor");
        for c in &out {
            let g = match c.level {
                Level::Ok => "✓",
                Level::Warn => "!",
                Level::Fail => "✕",
            };
            println!("{g} {:<12} {}", c.name, c.detail);
        }
    }
    Ok(())
}

/// Every check, in display order.
pub fn checks(ctx: &Ctx) -> Vec<Check> {
    let mut out = Vec::new();
    let cfg = ctx.paths.config_file();
    out.push(if cfg.exists() {
        check("config", Level::Ok, tilde(&cfg))
    } else {
        check(
            "config",
            Level::Ok,
            format!("{} · not created; using defaults", tilde(&cfg)),
        )
    });
    out.push(check("state", Level::Ok, tilde(&ctx.paths.state)));

    // DNS
    let net = ctx.net();
    match net.as_ref().map(|n| n.dns.nameservers()) {
        Ok(Ok(ns)) => out.push(check(
            "resolver",
            Level::Ok,
            ns.iter()
                .map(|s| s.ip().to_string())
                .collect::<Vec<_>>()
                .join(" · "),
        )),
        Ok(Err(e)) => out.push(check("resolver", Level::Fail, e.to_string())),
        Err(e) => out.push(check("network", Level::Fail, e.to_string())),
    }
    match &ctx.test.transport {
        crate::testenv::TransportSpec::Live => {}
        crate::testenv::TransportSpec::Replay(d) => out.push(check(
            "transport",
            Level::Warn,
            format!("replay · {} · test mode", tilde(d)),
        )),
        crate::testenv::TransportSpec::Record(d) => out.push(check(
            "transport",
            Level::Warn,
            format!("record · {} · test mode", tilde(d)),
        )),
    }

    // Secrets: names only, never values.
    let net_ok = net.ok();
    let modules = net_ok
        .map(|n| {
            spektr_modules::builtins_with(
                n,
                spektr_modules::BuiltinOptions {
                    config_dir: Some(ctx.paths.config.clone()),
                },
            )
        })
        .unwrap_or_default();
    for m in &modules {
        let man = m.manifest();
        for s in &man.secrets {
            out.push(if ctx.secrets.contains_key(s) {
                check(&format!("{} key", man.name), Level::Ok, format!("{s} set"))
            } else {
                check(
                    &format!("{} key", man.name),
                    Level::Warn,
                    format!("{s} not set · module off"),
                )
            });
        }
        for s in &man.optional_secrets {
            out.push(check(
                &format!("{} key", man.name),
                Level::Ok,
                if ctx.secrets.contains_key(s) {
                    format!("{s} set")
                } else {
                    format!("{s} not set (optional)")
                },
            ));
        }
    }

    // Exec modules in the config dir.
    if let Ok(entries) = std::fs::read_dir(ctx.paths.modules_dir()) {
        for e in entries.flatten() {
            let toml_path = e.path().join("module.toml");
            match std::fs::read_to_string(&toml_path)
                .map_err(|e| e.to_string())
                .and_then(|s| spektr_protocol::Manifest::from_toml(&s).map_err(|e| e.to_string()))
            {
                Ok(m) => {
                    let missing: Vec<&String> = m
                        .secrets
                        .iter()
                        .filter(|s| !ctx.secrets.contains_key(*s))
                        .collect();
                    let bin = m.command.first().cloned().unwrap_or_default();
                    let found = bin.contains('/') || which::which(&bin).is_ok();
                    let level = if !found || !missing.is_empty() {
                        Level::Warn
                    } else {
                        Level::Ok
                    };
                    let mut detail = format!(
                        "{} {} · {}",
                        m.version,
                        m.technique.as_str(),
                        if found {
                            format!("{bin} found")
                        } else {
                            format!("{bin} not found · module off")
                        }
                    );
                    if !missing.is_empty() {
                        detail.push_str(&format!(
                            " · missing {}",
                            missing
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    out.push(check(&m.name, level, detail));
                }
                Err(err) => out.push(check(&e.file_name().to_string_lossy(), Level::Fail, err)),
            }
        }
    }

    // Binaries.
    let chrome = ["google-chrome", "chromium", "chromium-browser", "chrome"]
        .iter()
        .find_map(|b| which::which(b).ok())
        .or_else(|| {
            let mac = std::path::Path::new(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            );
            mac.exists().then(|| mac.to_path_buf())
        });
    out.push(match chrome {
        Some(p) => check("chrome", Level::Ok, p.display().to_string()),
        None => check(
            "chrome",
            Level::Warn,
            "not found · captures without screenshots (spektr setup capture)",
        ),
    });
    out.push(match which::which("python3") {
        Ok(p) => check("python3", Level::Ok, p.display().to_string()),
        Err(_) => check(
            "python3",
            Level::Warn,
            "not found · Python exec modules won't run",
        ),
    });

    // Terminal.
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    let color = if std::env::var_os("NO_COLOR").is_some() {
        "NO_COLOR"
    } else if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        "truecolor"
    } else {
        "256-color"
    };
    let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".into());
    out.push(check(
        "terminal",
        Level::Ok,
        format!("{color} · TERM={term}"),
    ));
    out.push(check(
        "images",
        Level::Ok,
        "protocol detection arrives with image previews (Phase 4)",
    ));
    if std::env::var_os("TMUX").is_some() {
        let mouse = std::process::Command::new("tmux")
            .args(["show", "-gv", "mouse"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        out.push(match mouse.as_deref() {
            Some("on") => check("tmux", Level::Ok, "mouse on"),
            _ => check(
                "tmux",
                Level::Warn,
                "mouse off · run `tmux set -g mouse on` for clicks and scrolling",
            ),
        });
    }

    out
}
