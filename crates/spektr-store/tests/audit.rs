//! The audit chain: one row per mutation, canonical payloads, and `verify`
//! naming the location of any tampering (Phase 1 exit criterion).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spektr_model::{Actor, Attr, AttrValue, CorePredicate};
use spektr_store::{Db, VerifyLocation, chain_hash, sha256};
use support::*;

/// A case with entities, edges, evidence, attributes, blobs and a merge.
fn populated() -> Case {
    let mut c = new_case();
    c.db.write(|tx| {
        let o1 = obs(tx, "github", None, "api.github.com");
        let blob = tx
            .put_blob(&Actor::Analyst, b"raw response bytes, fictional")
            .unwrap();
        let o2 = tx
            .add_observation(
                &module("gitlab"),
                spektr_store::NewObservation {
                    job: None,
                    module: "gitlab".into(),
                    module_version: "1.0.0".into(),
                    manifest_hash: "x".into(),
                    input_entity: None,
                    source_url: Some("https://gitlab.com/api/v4/users".into()),
                    content_type: Some("application/json".into()),
                    blob: Some(blob),
                    summary: serde_json::json!({"n": 1}),
                    kind: spektr_store::ObservationKind::Module,
                },
            )
            .unwrap();
        let u = ent(tx, "username", "jmarlow.wav", 0, o1, "github.com");
        let a = ent(tx, "account", "github:jmarlow-wav", 1, o1, "github.com");
        let b = ent(tx, "account", "gitlab:jmarlow", 1, o2, "gitlab.com");
        edge(
            tx,
            u,
            CorePredicate::HasAccount,
            a,
            &[(o1, "github.com"), (o2, "gitlab.com")],
        );
        edge(tx, u, CorePredicate::HasAccount, b, &[(o2, "gitlab.com")]);
        tx.add_attr(
            &Actor::Analyst,
            a,
            "display_name",
            Attr {
                value: AttrValue::Text("Jordan M.".into()),
                observation: o1,
                event: false,
            },
        )
        .unwrap();
        let u2 = ent(tx, "username", "jm4rlow", 1, o1, "github.com");
        tx.merge(&Actor::Analyst, u, u2).unwrap();
        Ok(())
    })
    .unwrap();
    c
}

#[test]
fn chain_is_valid_and_formula_matches_spec() {
    let mut c = populated();
    let report = c.db.verify().unwrap();
    assert!(report.ok(), "{:?}", report.failure);
    assert!(report.audit_rows > 20, "{report:?}");
    assert_eq!(report.blobs, 1);

    let rows = c.db.reader().audit_rows(1, 1000).unwrap();
    assert_eq!(rows[0].prev_hash, [0u8; 32]);
    let expected = chain_hash(
        &[0u8; 32],
        1,
        &rows[0].ts,
        &rows[0].actor,
        &rows[0].action,
        &rows[0].payload,
    );
    assert_eq!(rows[0].hash, expected);
    for w in rows.windows(2) {
        assert_eq!(w[1].prev_hash, w[0].hash);
        assert_eq!(w[1].seq, w[0].seq + 1);
    }
    // payloads are canonical: compact, keys sorted
    for r in &rows {
        let v: serde_json::Value = serde_json::from_str(&r.payload).unwrap();
        assert_eq!(serde_json::to_string(&v).unwrap(), r.payload);
        assert!(!r.payload.contains(": "));
    }
    assert_eq!(rows[0].action, "case.create");
    assert_eq!(rows[0].ts, "2026-10-20T18:14:22.000Z");
}

#[test]
fn every_mutation_writes_exactly_one_row_and_no_ops_write_none() {
    let mut c = new_case();
    let count = |db: &Db| db.reader().audit_count().unwrap();
    let before = count(&c.db);
    let (o, u) =
        c.db.write(|tx| {
            let o = obs(tx, "github", None, "api.github.com");
            let u = ent(tx, "username", "jmarlow.wav", 0, o, "github.com");
            Ok((o, u))
        })
        .unwrap();
    assert_eq!(count(&c.db), before + 2, "observation + entity");

    let n = count(&c.db);
    c.db.write(|tx| {
        // same key, same depth: nothing changes
        ent(tx, "username", "JMarlow.WAV", 0, o, "github.com");
        Ok(())
    })
    .unwrap();
    assert_eq!(count(&c.db), n);

    let n = count(&c.db);
    let a =
        c.db.write(|tx| Ok(ent(tx, "account", "github:jmarlow-wav", 1, o, "github.com")))
            .unwrap();
    // account + derived url + derived domain + links_to + on_host edges + 2 evidence
    assert_eq!(count(&c.db), n + 7);

    let n = count(&c.db);
    let e =
        c.db.write(|tx| Ok(edge(tx, u, CorePredicate::HasAccount, a, &[])))
            .unwrap();
    assert_eq!(count(&c.db), n + 1);
    c.db.write(|tx| {
        edge(tx, u, CorePredicate::HasAccount, a, &[]);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        count(&c.db),
        n + 1,
        "re-reporting an edge is not a mutation"
    );
    c.db.write(|tx| {
        tx.add_evidence(&Actor::Analyst, e, o, "github.com")
            .map(|_| ())
    })
    .unwrap();
    c.db.write(|tx| {
        tx.add_evidence(&Actor::Analyst, e, o, "github.com")
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(count(&c.db), n + 2);
    c.db.write(|tx| tx.confirm_edge(&Actor::Analyst, e))
        .unwrap();
    assert_eq!(count(&c.db), n + 3);
    // A failed transaction writes nothing.
    let err = c.db.write(|tx| {
        tx.confirm_edge(&Actor::Analyst, e)?;
        Err::<(), _>(spektr_store::StoreError::Invalid("boom".into()))
    });
    assert!(err.is_err());
    assert_eq!(count(&c.db), n + 3);
    assert!(c.db.verify().unwrap().ok());
}

/// Every column of every audit row, tampered one at a time.
#[test]
fn tampering_any_audit_row_is_detected_at_that_row() {
    let mut c = populated();
    let rows = c.db.reader().audit_rows(1, 10_000).unwrap();
    assert!(rows.len() > 20);
    for row in &rows {
        let tampers: [(&str, rusqlite::types::Value); 6] = [
            (
                "ts",
                rusqlite::types::Value::Text("2026-10-20T18:14:23.000Z".into()),
            ),
            (
                "actor",
                rusqlite::types::Value::Text("module:evil@6.6.6".into()),
            ),
            (
                "action",
                rusqlite::types::Value::Text("entity.delete".into()),
            ),
            (
                "payload",
                rusqlite::types::Value::Text(row.payload.replacen('{', "{\"x\":1,", 1)),
            ),
            ("prev_hash", rusqlite::types::Value::Blob(vec![7u8; 32])),
            ("hash", rusqlite::types::Value::Blob(vec![9u8; 32])),
        ];
        for (col, value) in tampers {
            let original: rusqlite::types::Value =
                c.db.raw_connection()
                    .query_row(
                        &format!("SELECT {col} FROM audit WHERE seq = ?1"),
                        [row.seq],
                        |r| r.get(0),
                    )
                    .unwrap();
            c.db.raw_connection()
                .execute(
                    &format!("UPDATE audit SET {col} = ?1 WHERE seq = ?2"),
                    rusqlite::params![value, row.seq],
                )
                .unwrap();
            let report = spektr_store::Db::verify(&mut c.db).unwrap();
            let failure = report
                .failure
                .unwrap_or_else(|| panic!("tampering {col} of row {} went unnoticed", row.seq));
            assert_eq!(
                failure.location,
                VerifyLocation::AuditRow(row.seq),
                "{col} of row {}: {failure}",
                row.seq
            );
            let conn = c.db.raw_connection();
            conn.execute(
                &format!("UPDATE audit SET {col} = ?1 WHERE seq = ?2"),
                rusqlite::params![original, row.seq],
            )
            .unwrap();
        }
    }
    assert!(c.db.verify().unwrap().ok());
}

#[test]
fn deleting_rows_is_detected() {
    let mut c = populated();
    let n = c.db.reader().audit_count().unwrap() as i64;
    let conn = c.db.raw_connection();
    conn.execute("DELETE FROM audit WHERE seq = 5", []).unwrap();
    let f = c.db.verify().unwrap().failure.unwrap();
    assert_eq!(f.location, VerifyLocation::AuditRow(5), "{f}");

    let mut c = populated();
    c.db.raw_connection()
        .execute("DELETE FROM audit WHERE seq = ?1", [n])
        .unwrap();
    let f = c.db.verify().unwrap().failure.unwrap();
    assert_eq!(f.location, VerifyLocation::AuditTip, "{f}");
    assert!(f.to_string().contains(&format!("#{}", n - 1)), "{f}");
}

#[test]
fn tampering_a_blob_is_detected_at_that_blob() {
    let mut c = populated();
    let sha = sha256(b"raw response bytes, fictional");
    let conn = c.db.raw_connection();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();

    // flip a byte of the stored (compressed) data
    let data: Vec<u8> = conn
        .query_row(
            "SELECT data FROM blob WHERE sha256 = ?1",
            [&sha.0[..]],
            |r| r.get(0),
        )
        .unwrap();
    let mut bad = data.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0x01;
    conn.execute(
        "UPDATE blob SET data = ?1 WHERE sha256 = ?2",
        rusqlite::params![bad, &sha.0[..]],
    )
    .unwrap();
    let f = c.db.verify().unwrap().failure.unwrap();
    assert_eq!(f.location, VerifyLocation::Blob(sha), "{f}");

    // replace the content with different, validly compressed bytes
    let other = zstd::bulk::compress(b"raw response bytes, fictionaL", 3).unwrap();
    let conn = c.db.raw_connection();
    conn.execute(
        "UPDATE blob SET data = ?1 WHERE sha256 = ?2",
        rusqlite::params![other, &sha.0[..]],
    )
    .unwrap();
    let f = c.db.verify().unwrap().failure.unwrap();
    assert_eq!(f.location, VerifyLocation::Blob(sha), "{f}");
    assert!(f.reason.contains("SHA-256"), "{f}");

    // a missing blob is reported at the observation that needs it
    let conn = c.db.raw_connection();
    conn.execute("DELETE FROM blob WHERE sha256 = ?1", [&sha.0[..]])
        .unwrap();
    let f = c.db.verify().unwrap().failure.unwrap();
    assert!(matches!(f.location, VerifyLocation::Observation(_)), "{f}");
}
