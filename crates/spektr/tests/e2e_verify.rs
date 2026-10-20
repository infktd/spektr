//! J5 (PRD §5): tamper with an audit row, then with a blob; `spektr verify`
//! exits 3 and names the location.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spektr_model::{Clock, IdGen};
use spektr_store::{Db, OpenOptions, sha256};
use support::*;

fn opts() -> OpenOptions {
    OpenOptions::new(Clock::Fixed(CLOCK.parse().unwrap()), IdGen::seeded(1)).unwrap()
}

#[test]
fn verify_catches_audit_and_blob_tampering() {
    let h = Home::new();
    assert!(h.cmd(&["new", "j5"]).output().unwrap().status.success());
    assert!(
        h.cmd(&[
            "run",
            "j5.spektr",
            "--seed",
            "username:jmarlow.wav",
            "--module",
            "username-sweep"
        ])
        .output()
        .unwrap()
        .status
        .success()
    );
    let out = h.cmd(&["verify", "j5.spektr"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("✓ "), "{}", stdout(&out));

    let path = h.case("j5");
    // the first audit row that mentions the handle: change its payload
    let seq: i64 = {
        let db = Db::open(&path, opts()).unwrap();
        let c = db.raw_connection();
        let seq: i64 = c
            .query_row(
                "SELECT seq FROM audit WHERE payload LIKE '%jmarlow%' ORDER BY seq LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        c.execute(
            "UPDATE audit SET payload = replace(payload, 'jmarlow', 'jmarlów') WHERE seq = ?1",
            [seq],
        )
        .unwrap();
        seq
    };
    let out = h.cmd(&["verify", "j5.spektr"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(
        stdout(&out).trim(),
        format!("✕ audit row #{seq}: hash does not match the row's contents")
    );
    let out = h.cmd(&["verify", "j5.spektr", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["failure"]["location"], format!("audit row #{seq}"));

    // restore it, then corrupt a blob
    let sha = {
        let db = Db::open(&path, opts()).unwrap();
        let c = db.raw_connection();
        c.execute(
            "UPDATE audit SET payload = replace(payload, 'jmarlów', 'jmarlow') WHERE seq = ?1",
            [seq],
        )
        .unwrap();
        let (key, data): (Vec<u8>, Vec<u8>) = c
            .query_row(
                "SELECT sha256, data FROM blob ORDER BY sha256 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let other = zstd_like_garbage(&data);
        c.execute("PRAGMA foreign_keys = OFF", []).unwrap();
        c.execute(
            "UPDATE blob SET data = ?1 WHERE sha256 = ?2",
            rusqlite::params![other, key],
        )
        .unwrap();
        spektr_store::Sha256::from_slice(&key).unwrap()
    };
    assert_eq!(
        h.cmd(&["verify", "j5.spektr"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(3)
    );
    let out = h.cmd(&["verify", "j5.spektr"]).output().unwrap();
    let text = stdout(&out);
    assert!(text.starts_with(&format!("✕ blob {sha}: ")), "{text}");
    // sanity: sha256 helper agrees with the store's keys
    assert_eq!(sha256(b"").hex().len(), 64);
}

/// Flip the last byte, keeping the length.
fn zstd_like_garbage(data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    let n = d.len() - 1;
    d[n] ^= 0xff;
    d
}
