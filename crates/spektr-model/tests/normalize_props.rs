//! Property tests for every normalizer (Phase 1 exit criterion):
//! idempotent (normalizing a key yields the same key) and stable across case
//! and Unicode variants (full-width forms, composed vs decomposed accents,
//! surrounding whitespace).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test file

use proptest::prelude::*;
use spektr_model::{EntityType, KeyHint, NormalizeCtx, Vocabulary, normalize};

fn vocab() -> Vocabulary {
    Vocabulary::builtin()
}

fn norm(ty: &str, value: &str) -> Result<spektr_model::Normalized, spektr_model::NormalizeError> {
    let v = vocab();
    let ctx = NormalizeCtx {
        vocab: &v,
        default_region: Some("US"),
    };
    normalize(
        &ty.parse::<EntityType>().unwrap(),
        value,
        KeyHint::None,
        ctx,
    )
}

fn norm_hint(
    ty: &str,
    value: &str,
    hint: KeyHint<'_>,
) -> Result<String, spektr_model::NormalizeError> {
    let v = vocab();
    let ctx = NormalizeCtx {
        vocab: &v,
        default_region: Some("US"),
    };
    normalize(&ty.parse::<EntityType>().unwrap(), value, hint, ctx).map(|n| n.key)
}

/// Normalizing the key again gives the same key and type.
fn assert_idempotent(ty: &str, input: &str) -> Result<(), TestCaseError> {
    if let Ok(first) = norm(ty, input) {
        let again = norm(first.ty.as_str(), &first.key).map_err(|e| {
            TestCaseError::fail(format!("key `{}` no longer normalizes: {e}", first.key))
        })?;
        prop_assert_eq!(&again.key, &first.key, "input {:?}", input);
        prop_assert_eq!(&again.ty, &first.ty);
    }
    Ok(())
}

/// Flip the case of ASCII letters according to `mask`.
fn flip_case(s: &str, mask: &[bool]) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            if mask.get(i % mask.len().max(1)).copied().unwrap_or(false) {
                if c.is_ascii_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            } else {
                c
            }
        })
        .collect()
}

/// ASCII letters and digits → their full-width forms (NFKC maps them back).
fn fullwidth(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// Precomposed é/É → e/E + U+0301 (NFKC recomposes).
fn decompose(s: &str) -> String {
    s.replace('é', "e\u{301}").replace('É', "E\u{301}")
}

fn pad(s: &str, ws: u8) -> String {
    let w = [" ", "\t", "  ", "\u{3000}"][usize::from(ws % 4)];
    format!("{w}{s}{w}")
}

fn handle() -> impl Strategy<Value = String> {
    "[a-z0-9][a-z0-9._-]{0,14}"
}

fn label() -> impl Strategy<Value = String> {
    "[a-z0-9]([a-z0-9-]{0,10}[a-z0-9])?"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // ---- idempotence over arbitrary text, every type

    #[test]
    fn idempotent_on_arbitrary_input(s in "\\PC{0,40}") {
        for ty in ["username", "account", "email", "phone", "location", "url", "domain", "subdomain", "ip", "asn", "breach"] {
            assert_idempotent(ty, &s)?;
        }
    }

    #[test]
    fn idempotent_on_plausible_input(h in handle(), l1 in label(), l2 in label(), n in 0u32..4_000_000_000) {
        assert_idempotent("username", &format!("@{h}"))?;
        assert_idempotent("account", &format!("github:{}", h.replace(['.', '_'], "-")))?;
        assert_idempotent("account", &format!("mastodon:{h}@{l1}.example"))?;
        assert_idempotent("account", &format!("somesite:{h}"))?;
        assert_idempotent("email", &format!("{h}@{l1}.{l2}.example"))?;
        assert_idempotent("domain", &format!("{l1}.{l2}.example"))?;
        assert_idempotent("url", &format!("https://{l1}.example/{h}?q={n}#frag"))?;
        assert_idempotent("asn", &format!("as{n}"))?;
        assert_idempotent("ip", &format!("{}.{}.{}.{}", n >> 24, (n >> 16) & 255, (n >> 8) & 255, n & 255))?;
        assert_idempotent("breach", &format!("hibp:{h}"))?;
        assert_idempotent("location", &format!("place:{h} {l1}"))?;
    }

    #[test]
    fn idempotent_identity_keys(module in "[a-z][a-z0-9-]{1,10}", id in "[A-Za-z0-9-]{1,12}", hex in "[0-9a-fA-F]{64}", crt in 0u64..1_000_000_000) {
        let k = norm_hint("person", "Jordan Marlow", KeyHint::Dedup { module: &module, id: &id }).unwrap();
        prop_assert_eq!(norm_hint("person", "x", KeyHint::Key(&k)).unwrap(), k.clone());
        let k = norm_hint("organization", "Kestrel Labs", KeyHint::NewUuid(uuid::Uuid::from_u128(u128::from(crt)))).unwrap();
        prop_assert_eq!(norm_hint("organization", "x", KeyHint::Key(&k)).unwrap(), k.clone());
        let k = norm_hint("image", "a.png", KeyHint::Sha256(&hex)).unwrap();
        prop_assert_eq!(norm_hint("image", "a.png", KeyHint::Key(&k)).unwrap(), k.clone());
        let k = norm_hint("certificate", "c", KeyHint::Key(&format!("sha256:{hex}"))).unwrap();
        prop_assert_eq!(norm_hint("certificate", "c", KeyHint::Key(&k)).unwrap(), k.clone());
        let k = norm_hint("certificate", "c", KeyHint::Key(&format!("crtsh:{crt}"))).unwrap();
        prop_assert_eq!(norm_hint("certificate", &k, KeyHint::None).unwrap(), k.clone());
    }

    // ---- stability across case and Unicode variants

    #[test]
    fn username_variants(h in handle(), mask in prop::collection::vec(any::<bool>(), 1..8), ws in any::<u8>(), at in any::<bool>()) {
        let base = norm("username", &h).unwrap().key;
        let v = if at { format!("@{h}") } else { h.clone() };
        prop_assert_eq!(&norm("username", &pad(&flip_case(&v, &mask), ws)).unwrap().key, &base);
        prop_assert_eq!(&norm("username", &fullwidth(&flip_case(&v, &mask))).unwrap().key, &base);
    }

    #[test]
    fn username_accents(h in "[a-z]{1,6}", mask in prop::collection::vec(any::<bool>(), 1..8)) {
        let s = format!("{h}é{h}");
        let base = norm("username", &s).unwrap().key;
        prop_assert_eq!(&norm("username", &decompose(&s)).unwrap().key, &base);
        prop_assert_eq!(&norm("username", &decompose(&flip_case(&s, &mask).replace('é', "É"))).unwrap().key, &base);
    }

    #[test]
    fn email_variants(h in handle(), l in label(), mask in prop::collection::vec(any::<bool>(), 1..8), ws in any::<u8>()) {
        let s = format!("{h}@{l}.example");
        let base = norm("email", &s).unwrap().key;
        prop_assert_eq!(&norm("email", &pad(&flip_case(&s, &mask), ws)).unwrap().key, &base);
        // Unicode domain vs its punycode
        let u = format!("{h}@bücher-{l}.example");
        let base = norm("email", &u).unwrap().key;
        let puny = base.split('@').nth(1).unwrap().to_string();
        prop_assert_eq!(&norm("email", &format!("{h}@{puny}")).unwrap().key, &base);
        prop_assert_eq!(&norm("email", &flip_case(&u, &mask).replace('ü', "Ü")).unwrap().key, &base);
    }

    #[test]
    fn hostname_variants(l1 in label(), l2 in label(), mask in prop::collection::vec(any::<bool>(), 1..8), ws in any::<u8>(), dot in any::<bool>()) {
        let s = format!("{l1}.{l2}.example");
        let base = norm("domain", &s).unwrap();
        let v = if dot { format!("{s}.") } else { s.clone() };
        let other = norm("subdomain", &pad(&flip_case(&v, &mask), ws)).unwrap();
        prop_assert_eq!(&other.key, &base.key);
        prop_assert_eq!(&other.ty, &base.ty);
        prop_assert_eq!(&norm("domain", &fullwidth(&s)).unwrap().key, &base.key);
    }

    #[test]
    fn url_variants(l in label(), path in "[a-z0-9/]{0,12}", mask in prop::collection::vec(any::<bool>(), 1..8), port in any::<bool>(), frag in "[a-z]{0,5}") {
        let s = format!("https://{l}.example/{path}");
        let base = norm("url", &s).unwrap().key;
        let host_upper = format!("{}://{}.example{}/{path}#{frag}",
            flip_case("https", &mask), flip_case(&l, &mask), if port { ":443" } else { "" });
        prop_assert_eq!(&norm("url", &host_upper).unwrap().key, &base);
    }

    #[test]
    fn ip_variants(a in any::<[u16; 8]>(), v4 in any::<[u8; 4]>(), upper in any::<bool>()) {
        let v6 = std::net::Ipv6Addr::from(a);
        let full = format!("{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}", a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
        let full = if upper { full.to_uppercase() } else { full };
        prop_assert_eq!(norm("ip", &full).unwrap().key, norm("ip", &v6.to_string()).unwrap().key);
        let v4s = std::net::Ipv4Addr::from(v4).to_string();
        prop_assert_eq!(norm("ip", &format!("::ffff:{v4s}")).unwrap().key, v4s.clone());
        prop_assert_eq!(norm("ip", &format!(" {v4s} ")).unwrap().key, v4s);
    }

    #[test]
    fn asn_variants(n in any::<u32>(), prefix in 0u8..4) {
        let s = match prefix { 0 => format!("{n}"), 1 => format!("AS{n}"), 2 => format!("as{n}"), _ => format!(" As{n} ") };
        prop_assert_eq!(norm("asn", &s).unwrap().key, format!("AS{n}"));
    }

    #[test]
    fn phone_variants(line in 100u32..200, sep in 0u8..4, plus in any::<bool>()) {
        // 555-01XX numbers are reserved for fiction.
        let digits = format!("555555{:04}", line);
        let (a, b, c) = (&digits[..3], &digits[3..6], &digits[6..]);
        let s = match sep { 0 => format!("{a}{b}{c}"), 1 => format!("({a}) {b}-{c}"), 2 => format!("{a}.{b}.{c}"), _ => format!("{a} {b} {c}") };
        let s = if plus { format!("+1 {s}") } else { s };
        prop_assert_eq!(norm("phone", &s).unwrap().key, format!("+1{digits}"));
    }

    #[test]
    fn location_variants(lat in -90.0f64..90.0, lon in -180.0f64..180.0, place in "[a-z]{1,8}( [a-z]{1,8})?", mask in prop::collection::vec(any::<bool>(), 1..8), ws in any::<u8>()) {
        let g = norm("location", &format!("{lat},{lon}")).unwrap().key;
        prop_assert_eq!(&norm("location", &format!("geo:{lat}, {lon}")).unwrap().key, &g);
        prop_assert_eq!(&norm("location", &format!(" {lat} , {lon} ")).unwrap().key, &g);
        let p = norm("location", &place).unwrap().key;
        prop_assert_eq!(&norm("location", &pad(&flip_case(&place, &mask), ws)).unwrap().key, &p);
        prop_assert_eq!(&norm("location", &fullwidth(&place.replace(' ', "  "))).unwrap().key, &p);
    }

    #[test]
    fn account_variants(h in "[a-z0-9]([a-z0-9-]{0,10}[a-z0-9])?", mask in prop::collection::vec(any::<bool>(), 1..8), at in any::<bool>(), ws in any::<u8>()) {
        let base = norm("account", &format!("github:{h}")).unwrap().key;
        let v = format!("{}:{}{}", flip_case("github", &mask), if at { "@" } else { "" }, flip_case(&h, &mask));
        prop_assert_eq!(&norm("account", &pad(&v, ws)).unwrap().key, &base);
        let m = norm("account", &format!("mastodon:{h}@mastodon.social")).unwrap().key;
        prop_assert_eq!(&norm("account", &format!("mastodon:@{}@Mastodon.Social", flip_case(&h, &mask))).unwrap().key, &m);
    }

    #[test]
    fn breach_and_certificate_variants(slug in "[a-z0-9-]{1,12}", hex in "[0-9a-f]{64}", mask in prop::collection::vec(any::<bool>(), 1..8)) {
        let b = norm("breach", &format!("hibp:{slug}")).unwrap().key;
        prop_assert_eq!(&norm("breach", &flip_case(&format!(" hibp:{slug} "), &mask)).unwrap().key, &b);
        let c = norm_hint("certificate", "c", KeyHint::Key(&format!("sha256:{hex}"))).unwrap();
        let colons: String = hex.as_bytes().chunks(2).map(|p| std::str::from_utf8(p).unwrap()).collect::<Vec<_>>().join(":");
        prop_assert_eq!(&norm_hint("certificate", "c", KeyHint::Key(&format!("sha256:{}", flip_case(&colons, &mask)))).unwrap(), &c);
    }
}
