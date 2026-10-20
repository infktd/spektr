//! `cargo xtask deps-check`: enforces the crate-boundary and banned-dependency
//! rules of ARCHITECTURE §2 and DISPATCH ("Never"), by inspecting
//! `cargo metadata`.
//!
//! The resolved graph is checked twice, once with default features and once
//! with `--all-features`, so an optional feature can't sneak a banned crate or
//! `serde_json/preserve_order` in through feature unification. Both runs are
//! filtered to the four shipping targets (ARCHITECTURE §9), so wasm- or
//! Windows-only dependencies of upstream crates don't count.
//!
//! Rules:
//!
//! 0. The workspace has exactly the seven spektr crates plus `xtask`.
//! 1. `spektr-engine` does not depend on `spektr-modules` or `spektr-tui`.
//! 2. `spektr-tui` does not depend on `spektr-store` or `spektr-modules`.
//! 3. `spektr-model` and `spektr-protocol` stay light: no async runtime, no
//!    I/O crates and no heavier spektr crate as direct dependencies, and no
//!    package in their normal dependency closure that builds C (a build
//!    dependency on `cc`, `cmake`, `pkg-config` or `bindgen`).
//! 4. Nowhere in the resolved graph: `sqlx`, `async-trait`, Chrome DevTools
//!    crates, or the aws-lc stack that reqwest's default TLS pulls in; and
//!    `reqwest` is never built with its default features.
//! 5. `serde_json`'s `preserve_order` feature is never enabled (the audit
//!    chain's canonical JSON relies on sorted maps, DATA_MODEL §9).
//! 6. Every dependency in a member manifest is `{ workspace = true }`, so each
//!    version lives in exactly one place (DEPENDENCIES.md).
//!
//! Rules 1–3 look at **direct** dependencies (the TUI necessarily reaches the
//! store through the engine); rules 4–5 look at the whole resolved graph.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The shipping targets (ARCHITECTURE §9).
const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

/// The workspace members, exactly (ARCHITECTURE §2).
const MEMBERS: [&str; 8] = [
    "spektr-model",
    "spektr-protocol",
    "spektr-store",
    "spektr-engine",
    "spektr-modules",
    "spektr-tui",
    "spektr",
    "xtask",
];

/// Rules 1 and 2, plus rule 3's ban on heavier spektr crates:
/// `(crate, crates it must not depend on directly, rule number)`.
const BOUNDARIES: &[(&str, &[&str], u8)] = &[
    ("spektr-engine", &["spektr-modules", "spektr-tui"], 1),
    ("spektr-tui", &["spektr-store", "spektr-modules"], 2),
    (
        "spektr-model",
        &[
            "spektr-protocol",
            "spektr-store",
            "spektr-engine",
            "spektr-modules",
            "spektr-tui",
            "spektr",
        ],
        3,
    ),
    (
        "spektr-protocol",
        &[
            "spektr-store",
            "spektr-engine",
            "spektr-modules",
            "spektr-tui",
            "spektr",
        ],
        3,
    ),
];

/// Rule 3: crates that must stay light.
const LIGHT_CRATES: [&str; 2] = ["spektr-model", "spektr-protocol"];

/// Rule 3: async runtimes and I/O crates a light crate may not use directly.
const LIGHT_FORBIDDEN: &[&str] = &[
    "tokio",
    "tokio-util",
    "async-std",
    "smol",
    "async-executor",
    "futures-executor",
    "mio",
    "socket2",
    "hyper",
    "reqwest",
    "ureq",
    "rusqlite",
    "libsqlite3-sys",
    "hickory-proto",
    "crossterm",
    "ratatui",
];

/// Rule 3: a build dependency on one of these means the package compiles C.
/// (`links` alone is not a signal: `wasm-bindgen-shared` and `defmt` use it
/// for symbol uniqueness.)
const C_BUILD_TOOLS: &[&str] = &["cc", "cmake", "pkg-config", "bindgen"];

/// Rule 4: banned anywhere in the resolved graph. An entry ending in `*`
/// matches by prefix.
const BANNED: &[(&str, &str)] = &[
    ("sqlx", "banned: use rusqlite (ARCHITECTURE §6)"),
    ("sqlx-*", "banned: use rusqlite (ARCHITECTURE §6)"),
    (
        "async-trait",
        "banned: use futures::BoxFuture (DEPENDENCIES.md)",
    ),
    (
        "chromiumoxide",
        "banned: screenshots use a Chrome subprocess",
    ),
    (
        "chromiumoxide_*",
        "banned: screenshots use a Chrome subprocess",
    ),
    (
        "headless_chrome",
        "banned: screenshots use a Chrome subprocess",
    ),
    (
        "aws-lc-rs",
        "banned: TLS is rustls + ring (reqwest default TLS pulls aws-lc)",
    ),
    (
        "aws-lc-sys",
        "banned: TLS is rustls + ring (reqwest default TLS pulls aws-lc)",
    ),
];

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: String,
}

#[derive(Debug, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub deps: Vec<NodeDep>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    #[serde(default)]
    pub dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
pub struct DepKind {
    /// `None` is a normal dependency; otherwise `"dev"` or `"build"`.
    pub kind: Option<String>,
}

impl NodeDep {
    /// Normal or build dependency, i.e. one that ships with the crate.
    fn ships(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|k| matches!(k.kind.as_deref(), None | Some("build")))
    }

    fn is_build(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|k| k.kind.as_deref() == Some("build"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub rule: u8,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {}: {}", self.rule, self.message)
    }
}

fn violation(rule: u8, message: impl Into<String>) -> Violation {
    Violation {
        rule,
        message: message.into(),
    }
}

/// Indexed view of a `cargo metadata` result.
struct Graph<'a> {
    packages: HashMap<&'a str, &'a Package>,
    nodes: HashMap<&'a str, &'a Node>,
    members: HashMap<&'a str, &'a str>,
}

impl<'a> Graph<'a> {
    fn new(meta: &'a Metadata) -> Result<Self> {
        let resolve = meta
            .resolve
            .as_ref()
            .context("cargo metadata returned no dependency resolution")?;
        let packages: HashMap<_, _> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
        let nodes = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut members = HashMap::new();
        for id in &meta.workspace_members {
            let pkg = packages
                .get(id.as_str())
                .with_context(|| format!("workspace member {id} missing from packages"))?;
            members.insert(pkg.name.as_str(), id.as_str());
        }
        Ok(Self {
            packages,
            nodes,
            members,
        })
    }

    fn name(&self, id: &str) -> &'a str {
        self.packages
            .get(id)
            .map_or("<unknown>", |p| p.name.as_str())
    }

    /// The resolved node of a workspace member, by crate name.
    fn member_node(&self, name: &str) -> Option<&'a Node> {
        self.members
            .get(name)
            .and_then(|id| self.nodes.get(id).copied())
    }

    /// Every node in the resolved graph.
    fn all_nodes(&self) -> impl Iterator<Item = &'a Node> + '_ {
        self.nodes.values().copied()
    }

    /// Normal + build dependency closure of `root` (including `root`).
    fn shipped_closure(&self, root: &'a Node) -> Vec<&'a Node> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![root];
        let mut out = Vec::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node.id.as_str()) {
                continue;
            }
            out.push(node);
            for dep in node.deps.iter().filter(|d| d.ships()) {
                if let Some(next) = self.nodes.get(dep.pkg.as_str()) {
                    stack.push(next);
                }
            }
        }
        out
    }
}

/// Rules 0–5, over one resolved graph.
pub fn check_graph(meta: &Metadata) -> Result<Vec<Violation>> {
    let g = Graph::new(meta)?;
    let mut out = Vec::new();

    // Rule 0: exact workspace shape.
    let actual: BTreeSet<&str> = g.members.keys().copied().collect();
    let expected: BTreeSet<&str> = MEMBERS.into_iter().collect();
    for extra in actual.difference(&expected) {
        out.push(violation(
            0,
            format!("unexpected workspace member `{extra}`: seven crates plus xtask, use Rust modules instead"),
        ));
    }
    for missing in expected.difference(&actual) {
        out.push(violation(
            0,
            format!("workspace member `{missing}` is missing"),
        ));
    }

    // Rules 1–3: direct boundaries (every dependency kind).
    for (krate, forbidden, rule) in BOUNDARIES {
        let Some(node) = g.member_node(krate) else {
            continue;
        };
        for dep in &node.deps {
            let dep_name = g.name(&dep.pkg);
            if forbidden.contains(&dep_name) {
                out.push(violation(
                    *rule,
                    format!("`{krate}` must not depend on `{dep_name}`"),
                ));
            }
        }
    }

    // Rule 3: light crates.
    for krate in LIGHT_CRATES {
        let Some(node) = g.member_node(krate) else {
            continue;
        };
        for dep in node.deps.iter().filter(|d| d.ships()) {
            let dep_name = g.name(&dep.pkg);
            if LIGHT_FORBIDDEN.contains(&dep_name) {
                out.push(violation(
                    3,
                    format!(
                        "`{krate}` must stay light: no async runtime or I/O crate (`{dep_name}`)"
                    ),
                ));
            }
        }
        for member in g.shipped_closure(node) {
            let name = g.name(&member.id);
            for dep in member.deps.iter().filter(|d| d.is_build()) {
                let tool = g.name(&dep.pkg);
                if C_BUILD_TOOLS.contains(&tool) {
                    out.push(violation(
                        3,
                        format!(
                            "`{krate}` must have no C dependencies: `{name}` builds with `{tool}`"
                        ),
                    ));
                }
            }
        }
    }

    // Rules 4–5: the whole resolved graph.
    for node in g.all_nodes() {
        let name = g.name(&node.id);
        for (pattern, why) in BANNED {
            let hit = match pattern.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == *pattern,
            };
            if hit {
                out.push(violation(
                    4,
                    format!("`{name}` is in the dependency graph: {why}"),
                ));
            }
        }
        if name == "reqwest" && node.features.iter().any(|f| f == "default") {
            out.push(violation(
                4,
                "`reqwest` is built with default features (pulls aws-lc); set default-features = false",
            ));
        }
        if name == "serde_json" && node.features.iter().any(|f| f == "preserve_order") {
            out.push(violation(
                5,
                "`serde_json/preserve_order` is enabled; audit canonical JSON relies on sorted maps",
            ));
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// Rule 6: every dependency in a member manifest is `{ workspace = true }`.
pub fn check_manifest(crate_name: &str, manifest: &str) -> Result<Vec<Violation>> {
    let doc: toml::Table = manifest
        .parse()
        .with_context(|| format!("parsing the manifest of `{crate_name}`"))?;
    let mut tables: Vec<(String, &toml::Table)> = Vec::new();
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(t) = doc.get(key).and_then(toml::Value::as_table) {
            tables.push((key.to_string(), t));
        }
    }
    if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
        for (cfg, body) in targets {
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(t) = body.get(key).and_then(toml::Value::as_table) {
                    tables.push((format!("target.{cfg}.{key}"), t));
                }
            }
        }
    }

    let mut out = Vec::new();
    for (section, table) in tables {
        for (dep, spec) in table {
            let inherited = spec
                .as_table()
                .and_then(|t| t.get("workspace"))
                .and_then(toml::Value::as_bool)
                == Some(true);
            if !inherited {
                out.push(violation(
                    6,
                    format!("`{crate_name}` [{section}] `{dep}` must use `{{ workspace = true }}` (declare the version in [workspace.dependencies])"),
                ));
            }
        }
    }
    Ok(out)
}

fn cargo_metadata(root: &Path, all_features: bool) -> Result<Metadata> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root)
        .args(["metadata", "--format-version", "1", "--locked"]);
    for target in TARGETS {
        cmd.args(["--filter-platform", target]);
    }
    if all_features {
        cmd.arg("--all-features");
    }
    let output = cmd.output().context("running cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing cargo metadata output")
}

pub fn run(root: &Path) -> Result<()> {
    let mut violations = BTreeSet::new();
    let mut package_count = 0;
    for all_features in [false, true] {
        let meta = cargo_metadata(root, all_features)?;
        package_count = package_count.max(meta.resolve.as_ref().map_or(0, |r| r.nodes.len()));
        violations.extend(check_graph(&meta)?);
        if !all_features {
            for id in &meta.workspace_members {
                let Some(pkg) = meta.packages.iter().find(|p| &p.id == id) else {
                    continue;
                };
                let manifest = std::fs::read_to_string(&pkg.manifest_path)
                    .with_context(|| format!("reading {}", pkg.manifest_path))?;
                violations.extend(check_manifest(&pkg.name, &manifest)?);
            }
        }
    }

    if violations.is_empty() {
        println!(
            "deps-check: ok ({} members, {package_count} resolved packages; default and all features)",
            MEMBERS.len()
        );
        Ok(())
    } else {
        for v in &violations {
            eprintln!("deps-check: {v}");
        }
        bail!("{} dependency rule violation(s)", violations.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `(dependency name, kind)`; kind `None` is a normal dependency.
    type Dep<'a> = (&'a str, Option<&'a str>);
    /// `(package name, its dependencies, its enabled features)`.
    type Row<'a> = (&'a str, &'a [Dep<'a>], &'a [&'a str]);

    /// Build a resolved graph from rows.
    /// Every workspace member in `MEMBERS` is present unless `members` overrides.
    fn graph(members: &[&str], rows: &[Row<'_>]) -> Metadata {
        let mut packages = Vec::new();
        let mut nodes = Vec::new();
        let mut names: BTreeSet<&str> = members.iter().copied().collect();
        for (name, deps, _) in rows {
            names.insert(name);
            for (d, _) in *deps {
                names.insert(d);
            }
        }
        for name in &names {
            let row = rows.iter().find(|r| r.0 == *name);
            packages.push(json!({
                "id": format!("{name}-id"),
                "name": name,
                "manifest_path": format!("/x/{name}/Cargo.toml"),
            }));
            let deps: Vec<_> = row
                .map(|r| r.1)
                .unwrap_or(&[])
                .iter()
                .map(|(d, kind)| json!({"pkg": format!("{d}-id"), "dep_kinds": [{"kind": kind}]}))
                .collect();
            let features = row.map(|r| r.2).unwrap_or(&[]);
            nodes.push(json!({"id": format!("{name}-id"), "deps": deps, "features": features}));
        }
        let ws: Vec<_> = members.iter().map(|m| format!("{m}-id")).collect();
        serde_json::from_value(json!({
            "packages": packages,
            "workspace_members": ws,
            "resolve": {"nodes": nodes},
        }))
        .unwrap()
    }

    fn rules(v: &[Violation]) -> Vec<u8> {
        v.iter().map(|v| v.rule).collect()
    }

    #[test]
    fn clean_graph_passes() {
        let meta = graph(
            &MEMBERS,
            &[
                (
                    "spektr-engine",
                    &[("spektr-store", None), ("tokio", None)],
                    &[],
                ),
                ("spektr-tui", &[("spektr-engine", None)], &[]),
                ("spektr-model", &[("url", None)], &[]),
                ("spektr-protocol", &[("serde_json", None)], &[]),
                ("serde_json", &[], &["std"]),
                ("reqwest", &[], &["json", "rustls-no-provider"]),
            ],
        );
        assert!(check_graph(&meta).unwrap().is_empty());
    }

    #[test]
    fn workspace_shape_is_exact() {
        let mut members: Vec<&str> = MEMBERS.to_vec();
        members.retain(|m| *m != "spektr-store");
        members.push("spektr-net");
        let v = check_graph(&graph(&members, &[])).unwrap();
        assert_eq!(rules(&v), vec![0, 0]);
        assert!(v.iter().any(|v| v.message.contains("spektr-net")));
        assert!(v.iter().any(|v| v.message.contains("spektr-store")));
    }

    #[test]
    fn engine_must_not_depend_on_modules_or_tui() {
        let meta = graph(
            &MEMBERS,
            &[(
                "spektr-engine",
                &[("spektr-modules", None), ("spektr-tui", Some("dev"))],
                &[],
            )],
        );
        assert_eq!(rules(&check_graph(&meta).unwrap()), vec![1, 1]);
    }

    #[test]
    fn tui_must_not_depend_on_store_or_modules() {
        let meta = graph(
            &MEMBERS,
            &[(
                "spektr-tui",
                &[("spektr-store", None), ("spektr-modules", None)],
                &[],
            )],
        );
        assert_eq!(rules(&check_graph(&meta).unwrap()), vec![2, 2]);
    }

    #[test]
    fn tui_reaching_store_transitively_is_allowed() {
        let meta = graph(
            &MEMBERS,
            &[
                ("spektr-tui", &[("spektr-engine", None)], &[]),
                ("spektr-engine", &[("spektr-store", None)], &[]),
            ],
        );
        assert!(check_graph(&meta).unwrap().is_empty());
    }

    #[test]
    fn light_crates_reject_runtimes_io_and_heavier_spektr_crates() {
        let meta = graph(
            &MEMBERS,
            &[
                (
                    "spektr-model",
                    &[("tokio", None), ("spektr-store", None)],
                    &[],
                ),
                ("spektr-protocol", &[("reqwest", Some("build"))], &[]),
            ],
        );
        let v = check_graph(&meta).unwrap();
        assert_eq!(rules(&v), vec![3, 3, 3]);
    }

    #[test]
    fn light_crates_allow_dev_only_helpers() {
        let meta = graph(
            &MEMBERS,
            &[("spektr-model", &[("tokio", Some("dev"))], &[])],
        );
        assert!(check_graph(&meta).unwrap().is_empty());
    }

    #[test]
    fn light_crates_reject_transitive_c_code() {
        let meta = graph(
            &MEMBERS,
            &[
                ("spektr-model", &[("wrapper", None)], &[]),
                ("wrapper", &[("libfoo-sys", None)], &[]),
                ("libfoo-sys", &[("cc", Some("build"))], &[]),
            ],
        );
        let v = check_graph(&meta).unwrap();
        assert_eq!(rules(&v), vec![3]);
        assert!(v[0].message.contains("`libfoo-sys` builds with `cc`"));
    }

    #[test]
    fn c_code_is_fine_outside_light_crates() {
        let meta = graph(
            &MEMBERS,
            &[
                ("spektr-store", &[("libsqlite3-sys", None)], &[]),
                ("libsqlite3-sys", &[("cc", Some("build"))], &[]),
            ],
        );
        assert!(check_graph(&meta).unwrap().is_empty());
    }

    #[test]
    fn banned_crates_anywhere_in_the_graph() {
        let meta = graph(
            &MEMBERS,
            &[
                ("spektr-modules", &[("deep", None)], &[]),
                (
                    "deep",
                    &[
                        ("async-trait", None),
                        ("sqlx-core", None),
                        ("chromiumoxide_cdp", None),
                        ("headless_chrome", None),
                        ("aws-lc-sys", None),
                    ],
                    &[],
                ),
            ],
        );
        assert_eq!(rules(&check_graph(&meta).unwrap()), vec![4, 4, 4, 4, 4]);
    }

    #[test]
    fn reqwest_default_features_are_banned() {
        let meta = graph(&MEMBERS, &[("reqwest", &[], &["default", "json"])]);
        assert_eq!(rules(&check_graph(&meta).unwrap()), vec![4]);
    }

    #[test]
    fn serde_json_preserve_order_is_banned() {
        let meta = graph(&MEMBERS, &[("serde_json", &[], &["std", "preserve_order"])]);
        assert_eq!(rules(&check_graph(&meta).unwrap()), vec![5]);
    }

    #[test]
    fn manifests_must_inherit_workspace_versions() {
        let manifest = r#"
            [package]
            name = "spektr-x"
            [dependencies]
            serde.workspace = true
            url = "2.5"
            [dev-dependencies]
            proptest = { version = "1" }
            [target.'cfg(unix)'.dependencies]
            libc = { workspace = true }
        "#;
        let v = check_manifest("spektr-x", manifest).unwrap();
        assert_eq!(rules(&v), vec![6, 6]);
        assert!(v.iter().any(|v| v.message.contains("`url`")));
        assert!(v.iter().any(|v| v.message.contains("`proptest`")));
    }
}
