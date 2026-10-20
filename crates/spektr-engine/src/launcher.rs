//! Pivot launchers (MODULES §8): hand-off searches opened in the system
//! browser for things spektr shouldn't automate. Built-ins are embedded;
//! `<config>/launchers.toml` adds to them or overrides them by id.

use std::path::Path;

use serde::{Deserialize, Serialize};
use spektr_model::{AttrValue, CoreType};
use spektr_store::EntityRow;

const BUILTIN: &str = include_str!("../data/launchers.toml");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub id: String,
    pub name: String,
    /// Entity types it applies to.
    #[serde(rename = "for")]
    pub for_types: Vec<String>,
    /// `{value}` (URL-encoded) and `{url}` are filled in.
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default, rename = "launcher")]
    launchers: Vec<Launcher>,
}

#[derive(Debug, Clone, Default)]
pub struct Launchers {
    list: Vec<Launcher>,
    /// Ids that came from the user's file.
    user: Vec<String>,
}

/// Percent-encode a query value (RFC 3986 unreserved characters pass).
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn first_url(e: &EntityRow, name: &str) -> Option<String> {
    e.attrs.get(name)?.iter().find_map(|a| match &a.value {
        AttrValue::Url(u) | AttrValue::Text(u) => Some(u.clone()),
        _ => None,
    })
}

impl Launchers {
    pub fn builtin() -> Result<Self, String> {
        let f: File = toml::from_str(BUILTIN).map_err(|e| format!("built-in launchers: {e}"))?;
        Ok(Self {
            list: f.launchers,
            user: Vec::new(),
        })
    }

    /// Merge `<config>/launchers.toml`; a missing file is fine.
    pub fn load_file(&mut self, path: &Path) -> Vec<String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => return vec![format!("{}: {e}", path.display())],
        };
        match toml::from_str::<File>(&text) {
            Ok(f) => {
                for l in f.launchers {
                    self.user.push(l.id.clone());
                    match self.list.iter_mut().find(|x| x.id == l.id) {
                        Some(x) => *x = l,
                        None => self.list.push(l),
                    }
                }
                Vec::new()
            }
            Err(e) => vec![format!("{}: {e}", path.display())],
        }
    }

    pub fn get(&self, id: &str) -> Option<&Launcher> {
        self.list.iter().find(|l| l.id == id)
    }

    pub fn all(&self) -> &[Launcher] {
        &self.list
    }

    pub fn is_user(&self, id: &str) -> bool {
        self.user.iter().any(|u| u == id)
    }

    /// The launchers that apply to an entity's type.
    pub fn for_entity<'a>(&'a self, e: &'a EntityRow) -> impl Iterator<Item = &'a Launcher> + 'a {
        self.list
            .iter()
            .filter(move |l| l.for_types.iter().any(|t| t == e.ty.as_str()))
    }

    /// The URL a launcher opens for an entity, or why it can't open.
    pub fn url(l: &Launcher, e: &EntityRow) -> Result<String, String> {
        let value = match e.ty.core() {
            // the handle, not `platform:handle`
            Some(CoreType::Account) => e
                .key
                .split_once(':')
                .map_or(e.key.as_str(), |(_, h)| h)
                .to_string(),
            // E.164
            Some(CoreType::Phone) => e.key.clone(),
            _ => e.value.clone(),
        };
        let mut url = l.url.replace("{value}", &encode(&value));
        if url.contains("{url}") {
            let src = match e.ty.core() {
                Some(CoreType::Url) => Some(e.key.clone()),
                Some(CoreType::Image) => first_url(e, "source_url"),
                _ => first_url(e, "profile_url"),
            };
            let Some(src) = src else {
                return Err("needs a source URL: upload the image yourself".into());
            };
            url = url.replace("{url}", &encode(&src));
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_cover_the_playbooks() {
        let l = Launchers::builtin().unwrap();
        for id in [
            "google-exact",
            "bing-exact",
            "duckduckgo-exact",
            "lens",
            "yandex-images",
            "tineye",
            "bing-visual",
            "google-site",
            "wayback-web",
        ] {
            assert!(l.get(id).is_some(), "{id}");
        }
        assert_eq!(
            encode("jordan.marlow@mail.example"),
            "jordan.marlow%40mail.example"
        );
    }
}
