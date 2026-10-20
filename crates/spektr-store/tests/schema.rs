//! Schema creation, pragmas, identity and version checks (DATA_MODEL §8).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spektr_model::EntityFlags;
use spektr_store::{APPLICATION_ID, CaseInit, Db, SCHEMA_VERSION, StoreError};
use support::*;

#[test]
fn create_sets_pragmas_tables_and_meta() {
    let mut init = CaseInit::named("tl-ctf-1101");
    init.subject = Some("Jordan Marlow".into());
    init.last_seen = Some("2026-09-28".parse().unwrap());
    init.rules_profile = "tracelabs".into();
    let c = new_case_with(&init);
    let conn = c.db.raw_connection();
    let pragma = |p: &str| {
        conn.pragma_query_value(None, p, |r| r.get::<_, rusqlite::types::Value>(0))
            .unwrap()
    };
    assert_eq!(
        pragma("journal_mode"),
        rusqlite::types::Value::Text("wal".into())
    );
    assert_eq!(pragma("foreign_keys"), rusqlite::types::Value::Integer(1));
    assert_eq!(pragma("synchronous"), rusqlite::types::Value::Integer(1)); // NORMAL
    assert_eq!(
        pragma("busy_timeout"),
        rusqlite::types::Value::Integer(5000)
    );
    assert_eq!(
        pragma("application_id"),
        rusqlite::types::Value::Integer(i64::from(APPLICATION_ID))
    );
    assert_eq!(
        pragma("user_version"),
        rusqlite::types::Value::Integer(SCHEMA_VERSION)
    );

    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables.sort();
    assert_eq!(
        tables,
        [
            "audit",
            "blob",
            "case_meta",
            "checklist_item",
            "edge",
            "edge_evidence",
            "entity",
            "finding",
            "finding_entity",
            "finding_evidence",
            "job",
            "merge",
            "module_setting",
            "note",
            "observation",
            "playbook_run",
            "playbook_step",
            "review_item",
            "scope_rule",
        ]
    );

    let r = c.db.reader();
    let meta = r.meta_all().unwrap();
    for key in [
        "name",
        "created_at",
        "subject_entity",
        "last_seen",
        "rules_profile",
        "default_depth",
        "default_region",
        "hub_threshold",
        "schema_version",
        "spektr_version",
        "audit_tip",
    ] {
        assert!(meta.contains_key(key), "missing case_meta key {key}");
    }
    assert_eq!(meta["name"], "tl-ctf-1101");
    assert_eq!(meta["last_seen"], "2026-09-28");
    let subject_id: spektr_model::EntityId =
        meta["subject_entity"].as_str().unwrap().parse().unwrap();
    let subject = r.entity(subject_id).unwrap().unwrap();
    assert_eq!(subject.value, "Jordan Marlow");
    assert!(subject.key.starts_with("uuid:"));
    assert!(subject.flags.contains(EntityFlags::SUBJECT));
}

#[test]
fn reopen_and_verify() {
    let c = new_case();
    let path = c.path.clone();
    drop(c.db);
    let mut db = Db::open(&path, opts()).unwrap();
    assert!(db.verify().unwrap().ok());
    let ro = Db::open_read_only(&path, opts()).unwrap();
    assert_eq!(ro.reader().meta("name").unwrap().unwrap(), "test-case");
}

#[test]
fn refuses_existing_missing_foreign_and_newer_files() {
    let c = new_case();
    assert!(matches!(
        Db::create(&c.path, &CaseInit::named("x"), opts()),
        Err(StoreError::Exists(_))
    ));
    assert!(matches!(
        Db::open(&c.dir.path().join("nope.spektr"), opts()),
        Err(StoreError::NotFound(_))
    ));

    let foreign = c.dir.path().join("other.db");
    rusqlite::Connection::open(&foreign)
        .unwrap()
        .execute_batch("CREATE TABLE t (x)")
        .unwrap();
    assert!(matches!(
        Db::open(&foreign, opts()),
        Err(StoreError::NotACase(_))
    ));

    c.db.raw_connection()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let path = c.path.clone();
    drop(c.db);
    let err = Db::open(&path, opts()).unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::NewerSchema {
                found: 99,
                supported: 1,
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("newer spektr"), "{err}");
    assert!(matches!(
        Db::open_read_only(&path, opts()),
        Err(StoreError::NewerSchema { .. })
    ));
}

#[test]
fn corrupt_file_refuses_to_open() {
    let c = new_case();
    let path = c.path.clone();
    c.db.raw_connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(c.db);
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 8192);
    for b in &mut bytes[4096..8192] {
        *b = 0xA5;
    }
    std::fs::write(&path, bytes).unwrap();
    let err = Db::open(&path, opts()).unwrap_err();
    // Either quick_check reports it, or SQLite refuses the page outright.
    let msg = err.to_string();
    assert!(
        matches!(err, StoreError::Corrupt { .. })
            || msg.contains("malformed")
            || msg.contains("not a database"),
        "{msg}"
    );
}
