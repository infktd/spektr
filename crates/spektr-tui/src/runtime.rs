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

use crate::host::{External, Host, Settings};
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
    /// Show a file in the pager: the loop hands it the terminal.
    Pager(PathBuf),
    /// Edit a finding's text in `$EDITOR`, likewise.
    Edit(i64, PathBuf),
}

/// Type a test-mode key script (`Settings::script`) into the loop.
fn play_script(script: String, tx: mpsc::UnboundedSender<Internal>) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    tokio::spawn(async move {
        for tok in script.split_whitespace() {
            if let Some(ms) = tok
                .strip_prefix("wait:")
                .and_then(|m| m.parse::<u64>().ok())
            {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                continue;
            }
            let code = match tok {
                "enter" => KeyCode::Enter,
                "esc" => KeyCode::Esc,
                "tab" => KeyCode::Tab,
                "space" => KeyCode::Char(' '),
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                t => match t.chars().next() {
                    Some(c) if t.chars().count() == 1 => KeyCode::Char(c),
                    _ => continue,
                },
            };
            let mods = match code {
                KeyCode::Char(c) if c.is_ascii_uppercase() => KeyModifiers::SHIFT,
                _ => KeyModifiers::NONE,
            };
            let _ = tx.send(Internal::Msg(Msg::Key(KeyEvent::new(code, mods))));
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    });
}

/// Attachments read from disk stop here (the capture limit).
const MAX_ATTACH: u64 = spektr_engine::MAX_CAPTURE;

/// Where evidence opened outside the TUI is written.
fn evidence_file(sha: &str, name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("spektr-evidence");
    std::fs::create_dir_all(&dir)?;
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(dir.join(format!("{}-{safe}", &sha[..sha.len().min(12)])))
}

/// Runtime state the effects use.
struct Rt<'a> {
    host: &'a Arc<dyn Host>,
    engine: &'a Option<EngineHandle>,
    tx: &'a mpsc::UnboundedSender<Internal>,
    refresh_at: &'a mut Option<tokio::time::Instant>,
    /// The picker in use, and the one detected at startup (its font size
    /// lets `:images` switch protocol without querying again).
    picker: Option<ratatui_image::picker::Picker>,
    detected: Option<ratatui_image::picker::Picker>,
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
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    // The image-protocol query runs before raw mode (it manages its own).
    let images = crate::images::detect(settings.images);
    enable_raw_mode()?;
    execute!(out, EnableBracketedPaste, cursor::Hide)?;
    if settings.mouse {
        execute!(out, EnableMouseCapture)?;
    }
    let result = run_loop(host, settings, start, images).await;
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

/// Give the terminal to a pager, then take it back.
/// Hand the terminal to a pager or an editor. `Ok(Err(e))`: it failed;
/// `Ok(Ok(Some(t)))`: test mode recorded it.
fn with_terminal(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    host: &Arc<dyn Host>,
    what: External,
    mouse: bool,
) -> io::Result<Result<Option<String>, String>> {
    if !host.pager_takes_terminal() {
        return Ok(host.external(&what));
    }
    restore();
    let r = host.external(&what);
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableBracketedPaste,
        cursor::Hide
    )?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    term.clear()?;
    Ok(r)
}

/// Start decoding the images the current frame wants.
fn request_images(s: &mut AppState, rt: &Rt<'_>) {
    let (Some(picker), Some(eng)) = (rt.picker.clone(), rt.engine.clone()) else {
        return;
    };
    for key in crate::view::wanted_images(s) {
        if !s.images.request(&key) {
            continue;
        }
        let (tx, eng, picker) = (rt.tx.clone(), eng.clone(), picker.clone());
        tokio::spawn(async move {
            let r = match eng.query(spektr_engine::Query::Blob(key.sha.clone())).await {
                Ok(spektr_engine::QueryResult::Blob(Some(bytes))) => {
                    let (w, h) = (key.w, key.h);
                    tokio::task::spawn_blocking(move || {
                        crate::images::encode(&picker, &bytes, w, h)
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
                }
                Ok(_) => Err("not in the case".into()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(Internal::Msg(Msg::Image(key, r)));
        });
    }
}

async fn run_loop(
    host: Arc<dyn Host>,
    settings: Settings,
    start: Start,
    images: (Option<ratatui_image::picker::Picker>, String),
) -> io::Result<Option<EngineHandle>> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend)?;
    let size = term.size()?;
    let mut s = AppState::new(settings, host.now());
    s.size = (size.width, size.height);
    let (picker, protocol) = images;
    s.images = crate::images::Images::new(protocol, picker.is_some());
    let detected = picker.clone();
    let mut picker = picker;
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
    {
        let mut rt = Rt {
            host: &host,
            engine: &engine,
            tx: &tx,
            refresh_at: &mut refresh_at,
            picker: picker.take(),
            detected: detected.clone(),
        };
        run_effects(fx, &mut rt, &mut s, &mut term);
        picker = rt.picker;
    }
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
                    if let Some(script) = s.settings.script.take() {
                        play_script(script, tx.clone());
                    }
                    Some(Msg::Opened(path, Ok(())))
                }
                Internal::Edit(id, path) => {
                    let before = std::fs::read_to_string(&path).unwrap_or_default();
                    let r = with_terminal(&mut term, &host, External::Editor(path.clone()), s.mouse)?;
                    s.dirty = true;
                    let after = std::fs::read_to_string(&path).unwrap_or_default();
                    let _ = std::fs::remove_file(&path);
                    match r {
                        Ok(Some(t)) => Some(Msg::WouldOpen(t)),
                        Ok(None) if after != before => Some(Msg::Edited(id, after)),
                        Ok(None) => None,
                        Err(e) => Some(Msg::Acted { what: "editor".into(), result: Err(e) }),
                    }
                }
                Internal::Pager(path) => {
                    let r = with_terminal(&mut term, &host, External::Pager(path), s.mouse)?;
                    s.dirty = true;
                    match r {
                        Ok(Some(t)) => Some(Msg::WouldOpen(t)),
                        Ok(None) => None,
                        Err(e) => Some(Msg::Acted { what: "pager".into(), result: Err(e) }),
                    }
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
            let mut rt = Rt {
                host: &host,
                engine: &engine,
                tx: &tx,
                refresh_at: &mut refresh_at,
                picker: picker.take(),
                detected: detected.clone(),
            };
            run_effects(fx, &mut rt, &mut s, &mut term);
            request_images(&mut s, &rt);
            picker = rt.picker;
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
    rt: &mut Rt<'_>,
    s: &mut AppState,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
) {
    let (host, engine, tx) = (rt.host, rt.engine, rt.tx);
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
                *rt.refresh_at = Some(tokio::time::Instant::now() + REFRESH);
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
                let h = host.clone();
                tokio::task::spawn_blocking(move || match h.external(&External::Url(u)) {
                    Ok(Some(t)) => send(Msg::WouldOpen(t)),
                    Ok(None) => {}
                    Err(e) => send(Msg::Acted {
                        what: "open".into(),
                        result: Err(e),
                    }),
                });
            }
            Effect::OpenBlob { sha, name, reveal } => {
                if let Some(eng) = engine.clone() {
                    let h = host.clone();
                    tokio::spawn(async move {
                        match open_blob(&eng, &h, &sha, &name, reveal).await {
                            Ok(Some(t)) => send(Msg::WouldOpen(t)),
                            Ok(None) => {}
                            Err(e) => send(Msg::Acted {
                                what: "open".into(),
                                result: Err(e),
                            }),
                        }
                    });
                }
            }
            Effect::OpenEvidence(obs) => {
                if let Some(eng) = engine.clone() {
                    let h = host.clone();
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        match open_evidence(&eng, &h, &tx2, obs).await {
                            Ok(Some(t)) => send(Msg::WouldOpen(t)),
                            Ok(None) => {}
                            Err(e) => send(Msg::Acted {
                                what: "open".into(),
                                result: Err(e),
                            }),
                        }
                    });
                }
            }
            Effect::AttachFile { path, entity } => {
                if let Some(eng) = engine.clone() {
                    tokio::spawn(async move {
                        let name = path.file_name().map_or_else(
                            || "attachment".into(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        let read =
                            tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                                let len = std::fs::metadata(&path)
                                    .map_err(|e| format!("{}: {e}", path.display()))?
                                    .len();
                                if len > MAX_ATTACH {
                                    return Err(format!(
                                        "{} is over the 25 MB limit",
                                        path.display()
                                    ));
                                }
                                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
                            })
                            .await
                            .unwrap_or_else(|e| Err(e.to_string()));
                        let result = match read {
                            Ok(bytes) => eng
                                .act(spektr_engine::EngineAction::Attach {
                                    entity,
                                    name,
                                    bytes,
                                    note: None,
                                })
                                .await
                                .map_err(|e| e.to_string()),
                            Err(e) => Err(e),
                        };
                        send(Msg::Acted {
                            what: "attach".into(),
                            result,
                        });
                    });
                }
            }
            Effect::WriteFile(path, text) => {
                tokio::task::spawn_blocking(move || {
                    let r = std::fs::write(&path, text)
                        .map(|()| path.clone())
                        .map_err(|e| format!("{}: {e}", path.display()));
                    send(Msg::Wrote(r));
                });
            }
            Effect::EditExternal(id, text) => {
                let dir = std::env::temp_dir().join("spektr-edit");
                let path = dir.join(format!("finding-{id}.md"));
                match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
                    Ok(()) => {
                        let _ = tx.send(Internal::Edit(id, path));
                    }
                    Err(e) => send(Msg::Acted {
                        what: "editor".into(),
                        result: Err(e.to_string()),
                    }),
                }
            }
            Effect::SetImages(mode) => {
                use ratatui_image::picker::{Picker, ProtocolType};
                let base = || rt.detected.clone().unwrap_or_else(Picker::halfblocks);
                let forced = |p: ProtocolType| {
                    let mut x = base();
                    x.set_protocol_type(p);
                    x
                };
                rt.picker = match mode {
                    crate::images::Mode::Off => None,
                    crate::images::Mode::Auto => rt.detected.clone(),
                    crate::images::Mode::Halfblocks => Some(Picker::halfblocks()),
                    crate::images::Mode::Kitty => Some(forced(ProtocolType::Kitty)),
                    crate::images::Mode::Sixel => Some(forced(ProtocolType::Sixel)),
                    crate::images::Mode::Iterm2 => Some(forced(ProtocolType::Iterm2)),
                };
                let name = rt.picker.as_ref().map_or("off".to_string(), |p| {
                    format!("{:?}", p.protocol_type()).to_lowercase()
                });
                s.images = crate::images::Images::new(name, rt.picker.is_some());
                s.dirty = true;
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

async fn open_blob(
    eng: &EngineHandle,
    host: &Arc<dyn Host>,
    sha: &str,
    name: &str,
    reveal: bool,
) -> Result<Option<String>, String> {
    let bytes = match eng.query(spektr_engine::Query::Blob(sha.to_string())).await {
        Ok(spektr_engine::QueryResult::Blob(Some(b))) => b,
        Ok(_) => return Err("not in the case".into()),
        Err(e) => return Err(e.to_string()),
    };
    let path = evidence_file(sha, name).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let h = host.clone();
    tokio::task::spawn_blocking(move || {
        h.external(&if reveal {
            External::Reveal(path)
        } else {
            External::Open(path)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A capture's screenshot or an image in the viewer; anything else (a raw
/// response, a capture's HTML without a screenshot) in the pager.
async fn open_evidence(
    eng: &EngineHandle,
    host: &Arc<dyn Host>,
    tx: &mpsc::UnboundedSender<Internal>,
    obs: spektr_model::ObservationId,
) -> Result<Option<String>, String> {
    let raw = match eng.query(spektr_engine::Query::Raw(obs)).await {
        Ok(spektr_engine::QueryResult::Raw(Some(r))) => r,
        Ok(_) => return Err("no such observation".into()),
        Err(e) => return Err(e.to_string()),
    };
    if let Some(shot) = raw.summary["screenshot"].as_str() {
        return open_blob(eng, host, shot, "screenshot.png", false).await;
    }
    let Some(bytes) = raw.bytes else {
        return Err(format!(
            "{} kept no bytes for this observation",
            raw.observation.module
        ));
    };
    let sha = raw
        .observation
        .blob
        .clone()
        .unwrap_or_else(|| obs.to_string());
    if spektr_engine::is_image(&bytes) {
        return open_blob(eng, host, &sha, "image", false).await;
    }
    let ext = match raw.observation.content_type.as_deref() {
        Some(c) if c.contains("html") => "html",
        Some(c) if c.contains("json") => "json",
        _ => "txt",
    };
    let path = evidence_file(&sha, &format!("{}.{ext}", raw.observation.module))
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let _ = tx.send(Internal::Pager(path));
    Ok(None)
}
