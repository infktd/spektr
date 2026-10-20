//! The TUI loop (ARCHITECTURE §3): `tokio::select!` over terminal input,
//! the engine broadcast, effect results, a 250 ms tick and the refresh
//! debounce. Renders only when the state is dirty, at most every 16 ms.

use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use spektr_engine::{EngineEvent, EngineHandle};
use tokio::sync::{broadcast, mpsc};

use crate::host::{Host, Settings};
use crate::msg::{Effect, Msg};
use crate::state::{AppState, Screen};
use crate::update::{init, update};

/// Refresh debounce after engine events.
const REFRESH: Duration = Duration::from_millis(100);
/// Frame budget: at most 60 renders a second.
const FRAME: Duration = Duration::from_millis(16);
const TICK: Duration = Duration::from_millis(250);

/// Where the TUI starts.
pub enum Start {
    Launcher,
    Open(PathBuf),
}

enum Internal {
    Msg(Msg),
    Engine(PathBuf, EngineHandle),
}

fn restore() {
    let _ = disable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::Show
    );
}

/// Run the TUI until the user quits. Returns the open case's engine, so the
/// caller can flush it.
pub async fn run(
    host: Arc<dyn Host>,
    settings: Settings,
    start: Start,
) -> io::Result<Option<EngineHandle>> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableBracketedPaste,
        cursor::Hide
    )?;
    if settings.mouse {
        execute!(out, EnableMouseCapture)?;
    }
    let result = run_loop(host, settings, start).await;
    restore();
    result
}

async fn next_engine(rx: &mut Option<broadcast::Receiver<EngineEvent>>) -> Msg {
    match rx {
        Some(r) => match r.recv().await {
            Ok(ev) => Msg::Engine(Box::new(ev)),
            Err(broadcast::error::RecvError::Lagged(_)) => Msg::Lagged,
            Err(broadcast::error::RecvError::Closed) => {
                *rx = None;
                std::future::pending().await
            }
        },
        None => std::future::pending().await,
    }
}

async fn sleep_until(at: Option<tokio::time::Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

async fn run_loop(
    host: Arc<dyn Host>,
    settings: Settings,
    start: Start,
) -> io::Result<Option<EngineHandle>> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend)?;
    let size = term.size()?;
    let mut s = AppState::new(settings, host.now());
    s.size = (size.width, size.height);
    if let Start::Open(p) = start {
        s.screen = Screen::Opening(p);
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Internal>();
    let mut engine: Option<EngineHandle> = None;
    let mut engine_rx: Option<broadcast::Receiver<EngineEvent>> = None;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(TICK);
    let mut refresh_at: Option<tokio::time::Instant> = None;
    let mut frame_at: Option<tokio::time::Instant> = None;
    let mut last_draw = Instant::now() - FRAME;
    let t0 = Instant::now();
    let fx = init(&mut s);
    run_effects(fx, &host, &engine, &tx, &mut refresh_at, &mut term);
    loop {
        if s.quit {
            break;
        }
        if s.dirty {
            if last_draw.elapsed() >= FRAME {
                draw(&mut term, &mut s)?;
                last_draw = Instant::now();
                frame_at = None;
            } else if frame_at.is_none() {
                frame_at = Some(tokio::time::Instant::from_std(last_draw + FRAME));
            }
        }
        let msg = tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => match ev {
                    Event::Key(k) => Some(Msg::Key(k)),
                    Event::Mouse(m) => Some(Msg::Mouse(m, t0.elapsed().as_millis() as u64)),
                    Event::Paste(p) => Some(Msg::Paste(p)),
                    Event::Resize(w, h) => {
                        term.autoresize()?;
                        Some(Msg::Resize(w, h))
                    }
                    _ => None,
                },
                Some(Err(e)) => return Err(e),
                None => break,
            },
            Some(i) = rx.recv() => match i {
                Internal::Msg(m) => Some(m),
                Internal::Engine(path, h) => {
                    engine_rx = Some(h.subscribe());
                    engine = Some(h);
                    host.opened(&path);
                    Some(Msg::Opened(path, Ok(())))
                }
            },
            m = next_engine(&mut engine_rx) => Some(m),
            _ = tick.tick() => Some(Msg::Tick(host.now())),
            _ = sleep_until(refresh_at) => {
                refresh_at = None;
                Some(Msg::Refresh)
            }
            _ = sleep_until(frame_at) => {
                frame_at = None;
                None
            }
        };
        if let Some(m) = msg {
            let fx = update(&mut s, m);
            run_effects(fx, &host, &engine, &tx, &mut refresh_at, &mut term);
        }
    }
    Ok(engine)
}

fn draw(term: &mut Terminal<CrosstermBackend<Stdout>>, s: &mut AppState) -> io::Result<()> {
    let mut hits = Vec::new();
    term.draw(|f| {
        hits = crate::view::render(f, s);
    })?;
    s.hits = hits;
    s.dirty = false;
    Ok(())
}

fn run_effects(
    fx: Vec<Effect>,
    host: &Arc<dyn Host>,
    engine: &Option<EngineHandle>,
    tx: &mpsc::UnboundedSender<Internal>,
    refresh_at: &mut Option<tokio::time::Instant>,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
) {
    for e in fx {
        let sender = tx.clone();
        let send = move |m: Msg| {
            let _ = sender.send(Internal::Msg(m));
        };
        match e {
            Effect::LoadRecent => {
                let h = host.clone();
                tokio::task::spawn_blocking(move || send(Msg::Recent(h.recent())));
            }
            Effect::LoadDoctor => {
                let h = host.clone();
                tokio::task::spawn_blocking(move || send(Msg::Doctor(h.doctor())));
            }
            Effect::LoadProfiles => {
                let h = host.clone();
                tokio::task::spawn_blocking(move || send(Msg::Profiles(h.profiles())));
            }
            Effect::Verify(p) => {
                let h = host.clone();
                tokio::task::spawn_blocking(move || {
                    let c = h.verify(&p);
                    send(Msg::Verified(p, c));
                });
            }
            Effect::Open(p) => {
                let h = host.clone();
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    match h.open(p.clone()).await {
                        Ok(handle) => {
                            let _ = tx2.send(Internal::Engine(p, handle));
                        }
                        Err(e) => {
                            let _ = tx2.send(Internal::Msg(Msg::Opened(p, Err(e))));
                        }
                    }
                });
            }
            Effect::Create(req) => {
                let h = host.clone();
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    match h.create(req).await {
                        Ok((p, handle)) => {
                            let _ = tx2.send(Internal::Engine(p, handle));
                        }
                        Err(e) => send(Msg::Created(Err(e))),
                    }
                });
            }
            Effect::Query(tag, q) => {
                if let Some(eng) = engine.clone() {
                    tokio::spawn(async move {
                        let r = eng.query(q).await.map_err(|e| e.to_string());
                        send(Msg::Query(tag, r));
                    });
                }
            }
            Effect::Act { action, what } => {
                if let Some(eng) = engine.clone() {
                    tokio::spawn(async move {
                        let r = eng.act(action).await.map_err(|e| e.to_string());
                        send(Msg::Acted { what, result: r });
                    });
                }
            }
            Effect::ScheduleRefresh => {
                *refresh_at = Some(tokio::time::Instant::now() + REFRESH);
            }
            Effect::Copy(text) => {
                let out = term.backend_mut();
                let _ = execute!(
                    out,
                    crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
                );
                let _ = out.flush();
            }
            Effect::OpenUrl(u) => {
                let _ = open::that_detached(u);
            }
            Effect::SetMouse(on) => {
                let out = term.backend_mut();
                let _ = if on {
                    execute!(out, EnableMouseCapture)
                } else {
                    execute!(out, DisableMouseCapture)
                };
            }
            Effect::Quit => {}
        }
    }
}
