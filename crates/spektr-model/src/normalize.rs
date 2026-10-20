//! Normalization: input value → identity key (DATA_MODEL §1). Two inputs that
//! normalize to the same key are the same entity.
//!
//! Unparseable input is a hard error returned to the caller. Normalizing a
//! hostname decides `domain` vs `subdomain`, so the returned type may differ
//! from the requested one. Edges created at insert (`subdomain_of`,
//! `at_domain`, `on_host`, an account's `links_to`) are returned as
//! [`Derived`] targets, already normalized.

use std::fmt;
use std::net::IpAddr;

use uuid::Uuid;

use crate::extension::{ExtNormalize, Vocabulary};
use crate::platforms::fallback_handle;
use crate::predicates::CorePredicate;
use crate::text::{casefold, collapse_ws, has_control, nfkc};
use crate::types::{CoreType, EntityType};

/// Longest accepted input, in bytes.
pub const MAX_INPUT: usize = 8192;

/// Context a normalizer may need.
#[derive(Debug, Clone, Copy)]
pub struct NormalizeCtx<'a> {
    pub vocab: &'a Vocabulary,
    /// ISO 3166 region for phone numbers without a `+` prefix (case meta
    /// `default_region`).
    pub default_region: Option<&'a str>,
}

/// Identity that doesn't come from the value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyHint<'a> {
    None,
    /// A fresh person/organization: key `uuid:<v7>`.
    NewUuid(Uuid),
    /// A person/organization with a stable source id: `dedup:<module>:<id>`.
    Dedup {
        module: &'a str,
        id: &'a str,
    },
    /// Image bytes' SHA-256 (hex), computed by the store.
    Sha256(&'a str),
    /// An existing key, re-validated (person, organization, image,
    /// certificate).
    Key(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub ty: EntityType,
    pub key: String,
    /// Display form: the input, trimmed.
    pub value: String,
    pub derived: Vec<Derived>,
}

/// An edge `this —predicate→ target` created at insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub predicate: CorePredicate,
    pub target: Normalized,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct NormalizeError {
    pub ty: String,
    pub input: String,
    pub reason: String,
}

impl fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown: String = self.input.chars().take(80).collect();
        if self.input.chars().count() > 80 {
            shown.push('…');
        }
        write!(f, "invalid {} `{shown}`: {}", self.ty, self.reason)
    }
}

fn err(ty: &EntityType, input: &str, reason: impl Into<String>) -> NormalizeError {
    NormalizeError {
        ty: ty.to_string(),
        input: input.to_string(),
        reason: reason.into(),
    }
}

/// Normalize `value` as an entity of type `ty`.
pub fn normalize(
    ty: &EntityType,
    value: &str,
    hint: KeyHint<'_>,
    ctx: NormalizeCtx<'_>,
) -> Result<Normalized, NormalizeError> {
    if value.len() > MAX_INPUT {
        return Err(err(ty, value, format!("longer than {MAX_INPUT} bytes")));
    }
    let e = |reason: String| err(ty, value, reason);
    let trimmed = value.trim();
    let plain = |t: CoreType, key: String| Normalized {
        ty: t.into(),
        key,
        value: trimmed.to_string(),
        derived: Vec::new(),
    };

    let core = match ty {
        EntityType::Core(c) => *c,
        EntityType::Ext(name) => {
            let rule = ctx
                .vocab
                .ext_rule(name)
                .ok_or_else(|| e("extension type not declared by any installed module".into()))?;
            let key = ext_key(rule, value).map_err(e)?;
            let shown = if rule == ExtNormalize::Exact {
                value
            } else {
                trimmed
            };
            return Ok(Normalized {
                ty: ty.clone(),
                key,
                value: shown.to_string(),
                derived: Vec::new(),
            });
        }
    };

    match core {
        CoreType::Person | CoreType::Organization => {
            if trimmed.is_empty() || has_control(trimmed) {
                return Err(e("needs a name with no control characters".into()));
            }
            let key = identity_key(hint).map_err(e)?;
            Ok(plain(core, key))
        }
        CoreType::Username => Ok(plain(core, normalize_username(value).map_err(e)?)),
        CoreType::Account => normalize_account(value, ctx.vocab).map_err(e),
        CoreType::Email => {
            let (key, host) = normalize_email(value).map_err(e)?;
            let mut n = plain(core, key);
            n.derived.push(Derived {
                predicate: CorePredicate::AtDomain,
                target: host_entity(&host, &host.ascii),
            });
            Ok(n)
        }
        CoreType::Phone => Ok(plain(
            core,
            normalize_phone(value, ctx.default_region).map_err(e)?,
        )),
        CoreType::Image => {
            let key = match hint {
                KeyHint::Sha256(hex) => sha256_key(hex),
                KeyHint::Key(k) => k.strip_prefix("sha256:").and_then(sha256_key),
                _ => None,
            }
            .ok_or_else(|| e("an image's key is the SHA-256 of its bytes; none given".into()))?;
            let mut n = plain(core, key);
            if n.value.is_empty() {
                n.value = n.key.clone();
            }
            Ok(n)
        }
        CoreType::Location => Ok(plain(core, normalize_location(value).map_err(e)?)),
        CoreType::Url => {
            let (key, host) = normalize_url(value).map_err(e)?;
            let mut n = plain(core, key);
            if let Some(host) = host {
                n.derived.push(Derived {
                    predicate: CorePredicate::OnHost,
                    target: host_entity(&host, &host.ascii),
                });
            }
            Ok(n)
        }
        CoreType::Domain | CoreType::Subdomain => {
            let host = normalize_hostname(value).map_err(e)?;
            Ok(host_entity(&host, trimmed))
        }
        CoreType::Ip => Ok(plain(core, normalize_ip(value).map_err(e)?)),
        CoreType::Asn => Ok(plain(core, normalize_asn(value).map_err(e)?)),
        CoreType::Certificate => {
            let key = match hint {
                KeyHint::Key(k) => certificate_key(k),
                KeyHint::Sha256(hex) => sha256_key(hex),
                _ => certificate_key(trimmed),
            }
            .ok_or_else(|| {
                e("a certificate's key is sha256:<DER fingerprint> or crtsh:<id>".into())
            })?;
            Ok(plain(core, key))
        }
        CoreType::Breach => Ok(plain(core, normalize_breach(value).map_err(e)?)),
    }
}

// ---------------------------------------------------------------- identity

fn identity_key(hint: KeyHint<'_>) -> Result<String, String> {
    match hint {
        KeyHint::NewUuid(u) => Ok(format!("uuid:{u}")),
        KeyHint::Dedup { module, id } => dedup_key(module, id),
        KeyHint::Key(k) => {
            if let Some(u) = k.strip_prefix("uuid:") {
                let u = Uuid::parse_str(u).map_err(|_| format!("bad uuid key `{k}`"))?;
                Ok(format!("uuid:{u}"))
            } else if let Some(rest) = k.strip_prefix("dedup:") {
                let (module, id) = rest
                    .split_once(':')
                    .ok_or_else(|| format!("bad dedup key `{k}`"))?;
                dedup_key(module, id)
            } else {
                Err(format!("bad person/organization key `{k}`"))
            }
        }
        KeyHint::None | KeyHint::Sha256(_) => Err(
            "people and organizations need a uuid or a dedup_id; they never merge by name".into(),
        ),
    }
}

fn dedup_key(module: &str, id: &str) -> Result<String, String> {
    let module_ok = (2..=32).contains(&module.len())
        && module
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !module_ok {
        return Err(format!("bad module name `{module}` in dedup key"));
    }
    let id = id.trim();
    if id.is_empty() || has_control(id) {
        return Err("empty or invalid dedup_id".into());
    }
    Ok(format!("dedup:{module}:{id}"))
}

fn sha256_key(hex: &str) -> Option<String> {
    let hex = hex.trim().to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("sha256:{hex}"))
}

fn certificate_key(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(fp) = s.strip_prefix("sha256:") {
        sha256_key(&fp.replace(':', ""))
    } else if let Some(id) = s.strip_prefix("crtsh:") {
        id.trim().parse::<u64>().ok().map(|n| format!("crtsh:{n}"))
    } else {
        None
    }
}

// ---------------------------------------------------------------- identity types

/// NFKC, strip one leading `@`, lowercase.
pub fn normalize_username(input: &str) -> Result<String, String> {
    let s = nfkc(input.trim());
    let s = s.trim();
    let s = s.strip_prefix('@').unwrap_or(s);
    let s = nfkc(&s.to_lowercase());
    if s.is_empty() {
        return Err("empty username".into());
    }
    if let Some(c) = s.chars().find(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("username contains {c:?}"));
    }
    Ok(s)
}

fn normalize_account(input: &str, vocab: &Vocabulary) -> Result<Normalized, String> {
    let trimmed = input.trim();
    let (platform, handle) = trimmed
        .split_once(':')
        .ok_or_else(|| "expected platform:handle, e.g. github:jmarlow-wav".to_string())?;
    let platform = platform.trim().to_ascii_lowercase();
    let (handle, profile) = match vocab.platforms.get(&platform) {
        Some(p) => {
            let h = p.normalize_handle(handle).map_err(|e| e.to_string())?;
            let url = p.profile_url(&h);
            (h, url)
        }
        None => (
            fallback_handle(&platform, handle).map_err(|e| e.to_string())?,
            None,
        ),
    };
    let mut derived = Vec::new();
    if let Some(url) = profile
        && let Ok((key, host)) = normalize_url(&url)
    {
        let mut target = Normalized {
            ty: CoreType::Url.into(),
            key: key.clone(),
            value: key,
            derived: Vec::new(),
        };
        if let Some(host) = host {
            target.derived.push(Derived {
                predicate: CorePredicate::OnHost,
                target: host_entity(&host, &host.ascii),
            });
        }
        derived.push(Derived {
            predicate: CorePredicate::LinksTo,
            target,
        });
    }
    Ok(Normalized {
        ty: CoreType::Account.into(),
        key: format!("{platform}:{handle}"),
        value: trimmed.to_string(),
        derived,
    })
}

/// Trim, lowercase, IDNA domain → punycode. Returns the key and the host.
pub fn normalize_email(input: &str) -> Result<(String, Host), String> {
    let s = input.trim();
    let (local, domain) = s.rsplit_once('@').ok_or("missing @")?;
    if local.is_empty() {
        return Err("empty local part".into());
    }
    if let Some(c) = local
        .chars()
        .find(|c| c.is_whitespace() || c.is_control() || *c == '@')
    {
        return Err(format!("local part contains {c:?}"));
    }
    let host = normalize_hostname(domain)?;
    Ok((format!("{}@{}", local.to_lowercase(), host.ascii), host))
}

/// E.164 via libphonenumber metadata.
pub fn normalize_phone(input: &str, default_region: Option<&str>) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("empty phone number".into());
    }
    let region = if s.starts_with('+') {
        None
    } else {
        let r = default_region
            .ok_or("no country code and no default region (set one with --region or :region)")?;
        Some(
            r.trim()
                .to_ascii_uppercase()
                .parse::<phonenumber::country::Id>()
                .map_err(|_| format!("unknown region `{r}`"))?,
        )
    };
    let n = phonenumber::parse(region, s).map_err(|e| e.to_string())?;
    Ok(n.format().mode(phonenumber::Mode::E164).to_string())
}

/// `geo:<lat>,<lon>` to 5 decimals, or `place:<casefolded text>`.
pub fn normalize_location(input: &str) -> Result<String, String> {
    let s = input.trim();
    let geo_src = s.strip_prefix("geo:").unwrap_or(s);
    if let Some((a, b)) = geo_src.split_once(',')
        && let (Ok(lat), Ok(lon)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
    {
        if !(lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon))
        {
            return Err("coordinates out of range".into());
        }
        return Ok(format!("geo:{},{}", fixed5(lat), fixed5(lon)));
    }
    if s.starts_with("geo:") {
        return Err("expected geo:<lat>,<lon>".into());
    }
    let text = s.strip_prefix("place:").unwrap_or(s);
    let text = collapse_ws(&casefold(&nfkc(text)));
    if text.is_empty() || has_control(&text) {
        return Err("empty place".into());
    }
    Ok(format!("place:{text}"))
}

fn fixed5(x: f64) -> String {
    let s = format!("{x:.5}");
    if s == "-0.00000" { "0.00000".into() } else { s }
}

// ---------------------------------------------------------------- web & infrastructure

/// A normalized hostname and its registrable domain (eTLD+1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// Lowercase punycode, no trailing dot.
    pub ascii: String,
    pub registrable: String,
}

impl Host {
    pub fn is_subdomain(&self) -> bool {
        self.ascii != self.registrable
    }
}

/// Lowercase, IDNA → punycode, no trailing dot; must sit under a public
/// suffix (unknown TLDs count as one-label suffixes, so `kestrel.example`
/// is a registrable domain).
pub fn normalize_hostname(input: &str) -> Result<Host, String> {
    let s = input.trim();
    let s = s.strip_suffix('.').unwrap_or(s);
    if s.is_empty() {
        return Err("empty hostname".into());
    }
    if s.parse::<IpAddr>().is_ok() || s.starts_with('[') {
        return Err("an IP address, not a hostname".into());
    }
    if let Some(c) = s
        .chars()
        .find(|c| c.is_whitespace() || c.is_control() || "/:@?#*\\%".contains(*c))
    {
        return Err(format!("hostname contains {c:?}"));
    }
    let ascii = idna::domain_to_ascii(s).map_err(|e| format!("IDNA: {e}"))?;
    let ascii = ascii.strip_suffix('.').unwrap_or(&ascii).to_string();
    if ascii.len() > 253 {
        return Err("hostname longer than 253 bytes".into());
    }
    let labels: Vec<&str> = ascii.split('.').collect();
    if labels.len() < 2 {
        return Err("needs at least two labels".into());
    }
    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return Err("empty or over-long label".into());
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(format!("bad label `{label}`"));
        }
    }
    let registrable = psl::domain_str(&ascii)
        .ok_or("is a public suffix, not a registrable domain")?
        .to_string();
    Ok(Host { ascii, registrable })
}

/// The `domain` or `subdomain` entity for a host, with `subdomain_of` derived.
fn host_entity(host: &Host, value: &str) -> Normalized {
    if host.is_subdomain() {
        Normalized {
            ty: CoreType::Subdomain.into(),
            key: host.ascii.clone(),
            value: value.to_string(),
            derived: vec![Derived {
                predicate: CorePredicate::SubdomainOf,
                target: Normalized {
                    ty: CoreType::Domain.into(),
                    key: host.registrable.clone(),
                    value: host.registrable.clone(),
                    derived: Vec::new(),
                },
            }],
        }
    } else {
        Normalized {
            ty: CoreType::Domain.into(),
            key: host.ascii.clone(),
            value: value.to_string(),
            derived: Vec::new(),
        }
    }
}

/// WHATWG parse; http(s) only; lowercase scheme and host; IDNA host; drop the
/// default port and the fragment. Returns the key and the host (when it's a
/// hostname rather than an IP).
pub fn normalize_url(input: &str) -> Result<(String, Option<Host>), String> {
    let mut u = url::Url::parse(input.trim()).map_err(|e| e.to_string())?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported scheme `{}` (http and https only)",
            u.scheme()
        ));
    }
    u.set_fragment(None);
    let host = match u.host() {
        Some(url::Host::Domain(d)) => Some(normalize_hostname(d)?),
        Some(_) => None,
        None => return Err("no host".into()),
    };
    Ok((u.to_string(), host))
}

/// Canonical `IpAddr`; IPv4-mapped IPv6 becomes IPv4; IPv6 compressed.
pub fn normalize_ip(input: &str) -> Result<String, String> {
    let s = input.trim();
    let s = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(s);
    let ip: IpAddr = s.parse().map_err(|_| "not an IP address".to_string())?;
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    Ok(ip.to_string())
}

/// `AS<number>`.
pub fn normalize_asn(input: &str) -> Result<String, String> {
    let s = input.trim();
    let digits = match (s.get(..2), s.get(2..)) {
        (Some(p), Some(rest)) if p.eq_ignore_ascii_case("as") => rest,
        _ => s,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected AS<number>".into());
    }
    let n: u32 = digits.parse().map_err(|_| "ASN out of range".to_string())?;
    Ok(format!("AS{n}"))
}

/// `<source>:<slug>`, lowercase.
pub fn normalize_breach(input: &str) -> Result<String, String> {
    let s = input.trim().to_lowercase();
    let (source, slug) = s.split_once(':').ok_or("expected <source>:<slug>")?;
    let source_ok = (1..=32).contains(&source.len())
        && source
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !source_ok {
        return Err(format!("bad breach source `{source}`"));
    }
    if slug.is_empty() || slug.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("bad breach slug".into());
    }
    Ok(format!("{source}:{slug}"))
}

fn ext_key(rule: ExtNormalize, input: &str) -> Result<String, String> {
    let key = match rule {
        ExtNormalize::Exact => input.to_string(),
        ExtNormalize::Trim => input.trim().to_string(),
        ExtNormalize::Lowercase => input.trim().to_lowercase(),
        ExtNormalize::Casefold => casefold(&nfkc(input.trim())),
        ExtNormalize::Url => normalize_url(input)?.0,
    };
    if key.is_empty() {
        return Err("empty value".into());
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(v: &Vocabulary) -> NormalizeCtx<'_> {
        NormalizeCtx {
            vocab: v,
            default_region: Some("US"),
        }
    }

    fn n(ty: &str, value: &str) -> Result<Normalized, NormalizeError> {
        let v = Vocabulary::builtin();
        normalize(&ty.parse().unwrap(), value, KeyHint::None, ctx(&v))
    }

    fn key(ty: &str, value: &str) -> String {
        n(ty, value).unwrap().key
    }

    #[test]
    fn spec_examples() {
        assert_eq!(key("username", " @JMarlow.WAV "), "jmarlow.wav");
        assert_eq!(key("account", "github:JMarlow-WAV"), "github:jmarlow-wav");
        assert_eq!(
            key("email", "Jordan.Marlow@Mail.Example"),
            "jordan.marlow@mail.example"
        );
        assert_eq!(key("phone", "+1 555-555-0142"), "+15555550142");
        assert_eq!(key("phone", "(555) 555-0142"), "+15555550142");
        assert_eq!(key("location", "45.52, -122.68"), "geo:45.52000,-122.68000");
        assert_eq!(
            key("location", "  Portland,  Oregon "),
            "place:portland, oregon"
        );
        assert_eq!(
            key("url", "HTTPS://JMarlow.Example:443/#top"),
            "https://jmarlow.example/"
        );
        assert_eq!(key("domain", "Kestrel.Example."), "kestrel.example");
        assert_eq!(key("ip", "::ffff:203.0.113.24"), "203.0.113.24");
        assert_eq!(key("ip", "2001:DB8:0:0:0:0:0:1"), "2001:db8::1");
        assert_eq!(key("asn", "as64500"), "AS64500");
        assert_eq!(
            key("breach", "HIBP:ExampleBreach2024"),
            "hibp:examplebreach2024"
        );
    }

    #[test]
    fn hostname_typing_is_automatic() {
        let d = n("domain", "vpn.kestrel.example").unwrap();
        assert_eq!(d.ty, "subdomain".parse().unwrap());
        assert_eq!(d.derived.len(), 1);
        assert_eq!(d.derived[0].predicate, CorePredicate::SubdomainOf);
        assert_eq!(d.derived[0].target.key, "kestrel.example");
        let d = n("subdomain", "kestrel.example").unwrap();
        assert_eq!(d.ty, "domain".parse().unwrap());
        assert!(d.derived.is_empty());
        // public suffixes are respected
        assert_eq!(key("domain", "jmarlow.github.io"), "jmarlow.github.io");
        assert_eq!(
            n("domain", "jmarlow.github.io").unwrap().ty,
            "domain".parse().unwrap()
        );
        assert!(n("domain", "co.uk").is_err());
        assert!(n("domain", "localhost").is_err());
        assert!(n("domain", "203.0.113.24").is_err());
        // IDN
        assert_eq!(key("domain", "Bücher.example"), "xn--bcher-kva.example");
        assert_eq!(
            key("domain", "xn--bcher-kva.example"),
            "xn--bcher-kva.example"
        );
    }

    #[test]
    fn derived_edges() {
        let e = n("email", "ops@mail.kestrel.example").unwrap();
        assert_eq!(e.derived[0].predicate, CorePredicate::AtDomain);
        assert_eq!(e.derived[0].target.key, "mail.kestrel.example");
        assert_eq!(e.derived[0].target.derived[0].target.key, "kestrel.example");
        let u = n("url", "https://jmarlow.example/about").unwrap();
        assert_eq!(u.derived[0].predicate, CorePredicate::OnHost);
        assert_eq!(u.derived[0].target.key, "jmarlow.example");
        assert!(n("url", "http://203.0.113.24/").unwrap().derived.is_empty());
        let a = n("account", "github:jmarlow-wav").unwrap();
        assert_eq!(a.derived[0].predicate, CorePredicate::LinksTo);
        assert_eq!(a.derived[0].target.key, "https://github.com/jmarlow-wav");
        assert_eq!(a.derived[0].target.derived[0].target.key, "github.com");
        assert!(n("account", "somesite:jm").unwrap().derived.is_empty());
    }

    #[test]
    fn hard_errors() {
        for (ty, v) in [
            ("username", "  "),
            ("username", "jordan marlow"),
            ("email", "nobody"),
            ("email", "@mail.example"),
            ("url", "not a url"),
            ("url", "ftp://files.example/"),
            ("url", "mailto:a@b.example"),
            ("ip", "203.0.113.256"),
            ("asn", "ASX"),
            ("breach", "nocolon"),
            ("location", "geo:95,10"),
            ("account", "nocolon"),
            ("account", "github:bad handle"),
            ("certificate", "*.kestrel.example · R11"),
            ("person", "Jordan Marlow"),
        ] {
            assert!(n(ty, v).is_err(), "{ty}:{v} should fail");
        }
        let v = Vocabulary::builtin();
        let no_region = NormalizeCtx {
            vocab: &v,
            default_region: None,
        };
        let e = normalize(
            &"phone".parse().unwrap(),
            "555-555-0142",
            KeyHint::None,
            no_region,
        )
        .unwrap_err();
        assert!(e.to_string().contains("default region"), "{e}");
    }

    #[test]
    fn identity_hints() {
        let v = Vocabulary::builtin();
        let c = ctx(&v);
        let person: EntityType = "person".parse().unwrap();
        let u = Uuid::nil();
        let p = normalize(&person, " Jordan Marlow ", KeyHint::NewUuid(u), c).unwrap();
        assert_eq!(p.key, format!("uuid:{u}"));
        assert_eq!(p.value, "Jordan Marlow");
        let o = normalize(
            &"organization".parse().unwrap(),
            "Kestrel Labs LLC",
            KeyHint::Dedup {
                module: "rdap",
                id: "KL-1234",
            },
            c,
        )
        .unwrap();
        assert_eq!(o.key, "dedup:rdap:KL-1234");
        let again = normalize(&person, "x", KeyHint::Key(&p.key), c).unwrap();
        assert_eq!(again.key, p.key);
        let hex = "AB".repeat(32);
        let img = normalize(
            &"image".parse().unwrap(),
            "IMG_2214.jpg",
            KeyHint::Sha256(&hex),
            c,
        )
        .unwrap();
        assert_eq!(img.key, format!("sha256:{}", "ab".repeat(32)));
        let cert = normalize(
            &"certificate".parse().unwrap(),
            "*.kestrel.example · R11",
            KeyHint::Key("crtsh:00042"),
            c,
        )
        .unwrap();
        assert_eq!(cert.key, "crtsh:42");
        assert_eq!(cert.value, "*.kestrel.example · R11");
    }

    #[test]
    fn extension_rules() {
        let mut v = Vocabulary::builtin();
        v.declare_type("lemmy", "x.lemmy.instance", ExtNormalize::Lowercase)
            .unwrap();
        v.declare_type("lemmy", "x.lemmy.tag", ExtNormalize::Casefold)
            .unwrap();
        v.declare_type("lemmy", "x.lemmy.raw", ExtNormalize::Exact)
            .unwrap();
        let c = ctx(&v);
        let k = |t: &str, val: &str| {
            normalize(&t.parse().unwrap(), val, KeyHint::None, c)
                .unwrap()
                .key
        };
        assert_eq!(k("x.lemmy.instance", " Lemmy.World "), "lemmy.world");
        assert_eq!(k("x.lemmy.tag", "STRASSE"), k("x.lemmy.tag", "straße"));
        assert_eq!(k("x.lemmy.raw", " A "), " A ");
        assert!(normalize(&"x.other.thing".parse().unwrap(), "a", KeyHint::None, c).is_err());
    }
}
