//! `email-check`: syntax, MX presence and provider class, from DNS only.
//! Never SMTP, signup or reset probing (MODULES §6).

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, manifest, net_err};
use crate::net::{Net, RType};

const MANIFEST: &str = r#"
[module]
name        = "email-check"
version     = "1.0.0"
description = "Email syntax, MX presence and provider class (freemail, corporate, disposable) from DNS"
runtime     = "native"
concurrency = 8
timeout     = "30s"
technique   = "passive"
auto        = true
consumes    = ["email"]
produces    = []
cache_ttl   = "7d"
cost        = { class = "free" }
"#;

/// Common free-mail providers (by mail domain).
const FREEMAIL: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "msn.com",
    "yahoo.com",
    "ymail.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "aol.com",
    "proton.me",
    "protonmail.com",
    "gmx.com",
    "gmx.de",
    "gmx.net",
    "web.de",
    "mail.com",
    "yandex.com",
    "yandex.ru",
    "zoho.com",
    "fastmail.com",
    "tutanota.com",
    "tuta.io",
    "mail.ru",
    "qq.com",
    "163.com",
    "126.com",
    "hey.com",
    "pm.me",
    "posteo.de",
    "mailbox.org",
];

/// Disposable-address providers (a short, conservative list).
const DISPOSABLE: &[&str] = &[
    "mailinator.com",
    "guerrillamail.com",
    "10minutemail.com",
    "tempmail.com",
    "temp-mail.org",
    "trashmail.com",
    "yopmail.com",
    "sharklasers.com",
    "getnada.com",
    "dispostable.com",
    "maildrop.cc",
    "throwawaymail.com",
    "fakeinbox.com",
    "mintemail.com",
];

pub struct EmailCheck {
    manifest: Manifest,
    net: Net,
}

impl EmailCheck {
    pub fn new(net: Net) -> Self {
        Self {
            manifest: manifest(MANIFEST),
            net,
        }
    }
}

/// The provider class for a mail domain.
pub fn provider_class(domain: &str) -> &'static str {
    let d = domain.to_ascii_lowercase();
    if DISPOSABLE
        .iter()
        .any(|x| d == *x || d.ends_with(&format!(".{x}")))
    {
        "disposable"
    } else if FREEMAIL.contains(&d.as_str()) {
        "freemail"
    } else {
        "corporate"
    }
}

impl Module for EmailCheck {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let email = input.entity.key.clone();
            // The model already normalized it, so syntax is valid when it parses.
            let Some((_, domain)) = email.rsplit_once('@') else {
                out.observation(
                    "o1",
                    None,
                    None,
                    None,
                    Some(json!({ "syntax_valid": false })),
                )
                .await?;
                out.attr(
                    "$input",
                    "syntax_valid",
                    AttrValue::Bool(false),
                    false,
                    "o1",
                )
                .await?;
                return Ok(());
            };
            let mx = self
                .net
                .dns
                .lookup("email-check", domain, RType::Mx)
                .await
                .map_err(net_err)?;
            // A null MX (`0 .`, RFC 7505) means the domain takes no mail.
            let present = mx.answers.iter().any(|a| {
                a.split_once(' ')
                    .is_some_and(|(_, h)| !h.is_empty() && h != ".")
            });
            let class = provider_class(domain);
            let raw = json!({ "domain": domain, "MX": mx });
            out.observation(
                "o1",
                None,
                Some("application/json"),
                Some(raw.to_string()),
                Some(json!({
                    "syntax_valid": true, "mx_present": present, "provider_class": class,
                })),
            )
            .await?;
            out.attr("$input", "syntax_valid", AttrValue::Bool(true), false, "o1")
                .await?;
            out.attr(
                "$input",
                "mx_present",
                AttrValue::Bool(present),
                false,
                "o1",
            )
            .await?;
            out.attr(
                "$input",
                "provider_class",
                AttrValue::Text(class.into()),
                false,
                "o1",
            )
            .await?;
            out.progress(
                100,
                &format!("{class} · {}", if present { "MX present" } else { "no MX" }),
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn classes() {
        assert_eq!(super::provider_class("gmail.com"), "freemail");
        assert_eq!(super::provider_class("mailinator.com"), "disposable");
        assert_eq!(super::provider_class("kestrel.example"), "corporate");
    }
}
