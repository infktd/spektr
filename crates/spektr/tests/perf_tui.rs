//! Input-to-render latency on the 2,000-entity case (DISPATCH Phase 3 exit,
//! TUI_SPEC §11): from a key event to a drawn frame, with any queries the
//! key causes answered by the real engine. Meaningful in release builds:
//!
//!     cargo test --release -p spektr --test perf_tui -- --ignored --nocapture

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use spektr_engine::{Engine, EngineConfig, EngineHandle, OpenOptions};
use spektr_model::{Clock, IdGen};
use spektr_tui::msg::{Effect, Msg};
use spektr_tui::state::{AppState, Screen};
use spektr_tui::update::{init, update};
use support::Home;

/// Run effects that need the engine (queries) until none are left; others
/// are dropped. Returns the effects that ran.
async fn drive(s: &mut AppState, eng: &EngineHandle, mut fx: Vec<Effect>) -> usize {
    let mut n = 0;
    while let Some(e) = fx.pop() {
        if let Effect::Query(tag, q) = e {
            n += 1;
            let r = eng.query(q).await.map_err(|e| e.to_string());
            fx.extend(update(s, Msg::Query(tag, r)));
        }
    }
    n
}

fn draw(term: &mut Terminal<TestBackend>, s: &mut AppState) {
    let mut hits = Vec::new();
    term.draw(|f| hits = spektr_tui::view::render(f, s))
        .unwrap();
    s.hits = hits;
    s.dirty = false;
}

fn key(c: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(c, KeyModifiers::NONE))
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "release-mode measurement; see the module docs"]
async fn input_to_render_on_2000_entities() {
    let h = Home::new();
    let synth = h.synth_case("big", 2000);
    let opts = OpenOptions::new(Clock::System, IdGen::random()).unwrap();
    let eng = Engine::open(&h.case("big"), EngineConfig::new(opts))
        .await
        .unwrap()
        .start();

    let mut s = AppState::new(spektr_tui::Settings::default(), jiff::Timestamp::now());
    s.size = (170, 48);
    s.screen = Screen::Opening(h.case("big"));
    let _ = init(&mut s);
    let fx = update(&mut s, Msg::Opened(h.case("big"), Ok(())));
    drive(&mut s, &eng, fx).await;
    let mut term = Terminal::new(TestBackend::new(170, 48)).unwrap();
    draw(&mut term, &mut s);
    let w = s.workspace().unwrap();
    println!(
        "case: {} entities, focus has {} ego groups",
        synth.entities,
        w.ego.as_ref().map_or(0, |e| e.groups.len())
    );

    // Each sample: one key, its queries (if any) answered by the engine,
    // one frame. Keys cycle through local moves and engine round-trips.
    let script: Vec<(&str, Vec<KeyCode>)> = vec![
        ("j (cursor)", vec![KeyCode::Char('j')]),
        ("k (cursor)", vec![KeyCode::Char('k')]),
        ("] (next page: ego query)", vec![KeyCode::Char(']')]),
        ("[ (prev page: ego query)", vec![KeyCode::Char('[')]),
        ("tab (pane)", vec![KeyCode::Tab]),
        ("shift-tab (pane)", vec![KeyCode::BackTab]),
        ("↵ (pivot: ego + detail queries)", vec![KeyCode::Enter]),
        ("⌫ (back: ego query)", vec![KeyCode::Backspace]),
    ];
    let mut all = Vec::new();
    for (label, keys) in &script {
        let mut v = Vec::new();
        for _ in 0..40 {
            for k in keys {
                let t0 = Instant::now();
                let fx = update(&mut s, key(*k));
                drive(&mut s, &eng, fx).await;
                draw(&mut term, &mut s);
                v.push(t0.elapsed());
            }
        }
        let (p50, p95, max) = (
            pct(&mut v, 0.5),
            pct(&mut v, 0.95),
            *v.iter().max().unwrap(),
        );
        println!(
            "{label:<34} p50 {:>6.2} ms  p95 {:>6.2} ms  max {:>6.2} ms",
            p50.as_secs_f64() * 1e3,
            p95.as_secs_f64() * 1e3,
            max.as_secs_f64() * 1e3
        );
        all.extend(v);
    }
    let (p50, p95, max) = (
        pct(&mut all, 0.5),
        pct(&mut all, 0.95),
        *all.iter().max().unwrap(),
    );
    println!(
        "{:<34} p50 {:>6.2} ms  p95 {:>6.2} ms  max {:>6.2} ms  ({} samples)",
        "all",
        p50.as_secs_f64() * 1e3,
        p95.as_secs_f64() * 1e3,
        max.as_secs_f64() * 1e3,
        all.len()
    );
    if !cfg!(debug_assertions) {
        assert!(
            p95 < Duration::from_millis(16),
            "p95 input-to-render {p95:?} ≥ 16 ms"
        );
    }
}
