//! Audit actors (DATA_MODEL §9).

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Actor {
    Analyst,
    Engine,
    Module { name: String, version: String },
    Correlator(String),
    Playbook { name: String, version: String },
    Tutor,
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::Analyst => f.write_str("analyst"),
            Actor::Engine => f.write_str("engine"),
            Actor::Module { name, version } => write!(f, "module:{name}@{version}"),
            Actor::Correlator(rule) => write!(f, "correlator:{rule}"),
            Actor::Playbook { name, version } => write!(f, "playbook:{name}@{version}"),
            Actor::Tutor => f.write_str("tutor"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown actor `{0}`")]
pub struct ParseActorError(String);

impl FromStr for Actor {
    type Err = ParseActorError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseActorError(s.to_string());
        let name_version = |rest: &str| -> Result<(String, String), ParseActorError> {
            let (n, v) = rest.split_once('@').ok_or_else(err)?;
            if n.is_empty() || v.is_empty() {
                return Err(err());
            }
            Ok((n.to_string(), v.to_string()))
        };
        match s {
            "analyst" => Ok(Actor::Analyst),
            "engine" => Ok(Actor::Engine),
            "tutor" => Ok(Actor::Tutor),
            _ => {
                if let Some(rest) = s.strip_prefix("module:") {
                    let (name, version) = name_version(rest)?;
                    Ok(Actor::Module { name, version })
                } else if let Some(rest) = s.strip_prefix("playbook:") {
                    let (name, version) = name_version(rest)?;
                    Ok(Actor::Playbook { name, version })
                } else if let Some(rule) = s.strip_prefix("correlator:") {
                    if rule.is_empty() {
                        return Err(err());
                    }
                    Ok(Actor::Correlator(rule.to_string()))
                } else {
                    Err(err())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for s in [
            "analyst",
            "engine",
            "tutor",
            "module:github@0.4.0",
            "correlator:avatar_phash",
            "playbook:person-from-username@1.0.0",
        ] {
            assert_eq!(s.parse::<Actor>().unwrap().to_string(), s);
        }
        assert!("module:github".parse::<Actor>().is_err());
        assert!("bob".parse::<Actor>().is_err());
    }
}
