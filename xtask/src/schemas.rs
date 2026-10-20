//! `cargo xtask schemas [--check]`: generate `schemas/exec-protocol.v1.json`
//! `schemas/manifest.v1.json` and `schemas/playbook.v1.json` from the Rust
//! types (MODULES §3, PLAYBOOKS §2).

use std::path::Path;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use spektr_protocol::{HostMessage, ManifestFile, ModuleMessage, PlaybookFile};

/// One line of the exec protocol, in either direction.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum ExecProtocolLine {
    Host(HostMessage),
    Module(ModuleMessage),
}

fn render<T: JsonSchema>(title: &str, description: &str) -> Result<String> {
    let mut schema = serde_json::to_value(schemars::schema_for!(T))?;
    if let Some(obj) = schema.as_object_mut() {
        obj.insert("title".into(), title.into());
        obj.insert("description".into(), description.into());
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&schema)?))
}

pub fn run(root: &Path, check: bool) -> Result<()> {
    let files = [
        (
            "schemas/exec-protocol.v1.json",
            render::<ExecProtocolLine>(
                "spektr exec protocol v1",
                "One JSON object per line over a module's stdin (host → module: hello, run, cancel, shutdown) and stdout (module → host). See docs/MODULES.md §3.",
            )?,
        ),
        (
            "schemas/manifest.v1.json",
            render::<ManifestFile>(
                "spektr module manifest v1",
                "module.toml. See docs/MODULES.md §2.",
            )?,
        ),
        (
            "schemas/playbook.v1.json",
            render::<PlaybookFile>(
                "spektr playbook v1",
                "A playbook TOML file. See docs/PLAYBOOKS.md §1.",
            )?,
        ),
    ];
    let mut stale = Vec::new();
    for (rel, text) in &files {
        let path = root.join(rel);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if check {
            if &current != text {
                stale.push(*rel);
            }
        } else {
            std::fs::create_dir_all(path.parent().context("schema path has no parent")?)?;
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
            println!("wrote {rel}");
        }
    }
    if !stale.is_empty() {
        bail!(
            "out of date: {} (run `cargo xtask schemas`)",
            stale.join(", ")
        );
    }
    if check {
        println!("schemas: up to date");
    }
    Ok(())
}
