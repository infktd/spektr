//! `SPEKTR_TEST_KEYS` (test mode only): keys the TUI types after the case
//! opens, which the real-terminal QA uses to reach the image previews.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Size, Term};

const T: Duration = Duration::from_secs(20);

#[test]
fn the_key_script_is_typed_after_the_case_opens() {
    let h = Home::new();
    h.demo_case();
    let mut env = h.env();
    env.push((
        "SPEKTR_TEST_KEYS".into(),
        "wait:1500 h h j j wait:300 C".into(),
    ));
    let mut t = Term::spawn(
        &["open", "demo.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &env,
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("CHECKLIST", T).unwrap();
    t.keys("esc").unwrap();
    // `h h j j` left the cursor on a ≈ row: the inspector compares the edge.
    t.wait_for_text("dHash distance", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    // refused outside test mode
    let out = h
        .cmd(&["doctor"])
        .env_remove("SPEKTR_TEST_MODE")
        .env_remove("SPEKTR_TRANSPORT")
        .env_remove("SPEKTR_CLOCK")
        .env_remove("SPEKTR_SEED")
        .env("SPEKTR_TEST_KEYS", "q")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        support::stderr(&out).contains("SPEKTR_TEST_KEYS is a testing variable"),
        "{}",
        support::stderr(&out)
    );
}
