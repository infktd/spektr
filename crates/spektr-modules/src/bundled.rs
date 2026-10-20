//! Fixtures bundled into the binary, so the tutor runs offline with every
//! module in replay (TUI_SPEC §10). A test keeps this table in step with
//! `fixtures/`.

use crate::net::Fixtures;
use crate::net::fixtures::FixtureError;

/// Every `fixtures/**/*.jsonl`, by path.
pub const FIXTURES: &[(&str, &str)] = &[
    (
        "avatar-hash/jmarlow.wav.jsonl",
        include_str!("../fixtures/avatar-hash/jmarlow.wav.jsonl"),
    ),
    (
        "bluesky/jmarlow.wav.jsonl",
        include_str!("../fixtures/bluesky/jmarlow.wav.jsonl"),
    ),
    (
        "crtsh/jmarlow.wav.jsonl",
        include_str!("../fixtures/crtsh/jmarlow.wav.jsonl"),
    ),
    (
        "dns/jmarlow.wav.jsonl",
        include_str!("../fixtures/dns/jmarlow.wav.jsonl"),
    ),
    (
        "dns/kestrel.example.jsonl",
        include_str!("../fixtures/dns/kestrel.example.jsonl"),
    ),
    (
        "email-check/jmarlow.wav.jsonl",
        include_str!("../fixtures/email-check/jmarlow.wav.jsonl"),
    ),
    (
        "github/jmarlow-wav.jsonl",
        include_str!("../fixtures/github/jmarlow-wav.jsonl"),
    ),
    (
        "github/rl-demo.jsonl",
        include_str!("../fixtures/github/rl-demo.jsonl"),
    ),
    (
        "gitlab/jmarlow.wav.jsonl",
        include_str!("../fixtures/gitlab/jmarlow.wav.jsonl"),
    ),
    (
        "gravatar/jmarlow.wav.jsonl",
        include_str!("../fixtures/gravatar/jmarlow.wav.jsonl"),
    ),
    (
        "http-fingerprint/jmarlow.wav.jsonl",
        include_str!("../fixtures/http-fingerprint/jmarlow.wav.jsonl"),
    ),
    (
        "kestrel/kestrel.example.jsonl",
        include_str!("../fixtures/kestrel/kestrel.example.jsonl"),
    ),
    (
        "mastodon/jmarlow.wav.jsonl",
        include_str!("../fixtures/mastodon/jmarlow.wav.jsonl"),
    ),
    (
        "rdap/bootstrap.jsonl",
        include_str!("../fixtures/rdap/bootstrap.jsonl"),
    ),
    (
        "rdap/jmarlow.wav.jsonl",
        include_str!("../fixtures/rdap/jmarlow.wav.jsonl"),
    ),
    (
        "tutor-infra/mail.example.jsonl",
        include_str!("../fixtures/tutor-infra/mail.example.jsonl"),
    ),
    (
        "username-sweep/jmarlow.wav.jsonl",
        include_str!("../fixtures/username-sweep/jmarlow.wav.jsonl"),
    ),
    (
        "wayback/jmarlow.wav.jsonl",
        include_str!("../fixtures/wayback/jmarlow.wav.jsonl"),
    ),
];

/// The tutor's photo (EXIF with GPS; fictional), for the attach lesson.
pub const TUTOR_PHOTO: &[u8] = include_bytes!("../fixtures/tutor/IMG_2214.jpg");

/// The bundled fixtures, loaded.
pub fn fixtures() -> Result<Fixtures, FixtureError> {
    let mut f = Fixtures::default();
    for (name, text) in FIXTURES {
        f.add_jsonl(name, text)?;
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_matches_the_fixtures_dir() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut on_disk = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "jsonl") {
                    on_disk.push(
                        p.strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        on_disk.sort();
        let bundled: Vec<String> = FIXTURES.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(bundled, on_disk, "regenerate the FIXTURES table");
        assert!(fixtures().is_ok());
    }
}
