//! The terminal harness (QA.md §1): run the real `spektr` binary in a
//! pseudo-terminal and read the screen back as a cell grid, with colors.
//!
//! ```ignore
//! let mut t = Term::spawn(&["open", case], Size { cols: 170, rows: 48 }, &env, &cwd)?;
//! t.wait_for_text("Entities", Duration::from_secs(5))?;
//! t.keys("j j ↵")?;
//! t.mouse_click(42, 10)?;
//! let s = t.screen();
//! t.save_frame("qa/out/j2-step3")?; // .txt, .ansi, .html
//! ```

#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// Frame colors for cells using the terminal default: the TUI's `fg` and
/// `bg` tokens, from `theme.rs`.
pub const DEFAULT_FG: (u8, u8, u8) = spektr_tui::theme::Token::Fg.values().0;
pub const DEFAULT_BG: (u8, u8, u8) = spektr_tui::theme::Token::Bg.values().0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for Color {
    fn from(c: vt100::Color) -> Self {
        match c {
            vt100::Color::Default => Color::Default,
            vt100::Color::Idx(i) => Color::Idx(i),
            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

impl Color {
    /// The RGB a terminal would show (xterm palette for indexed colors).
    pub fn rgb(self, default: (u8, u8, u8)) -> (u8, u8, u8) {
        match self {
            Color::Default => default,
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Idx(i) => xterm(i),
        }
    }
}

fn xterm(i: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match i {
        0..=15 => BASE[usize::from(i)],
        16..=231 => {
            let n = i - 16;
            let v = |c: u8| if c == 0 { 0 } else { 55 + c * 40 };
            (v(n / 36), v((n / 6) % 6), v(n % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            (g, g, g)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub inverse: bool,
}

/// A snapshot of the terminal screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<Cell>>,
    pub cursor: (u16, u16),
    pub contents: String,
    pub formatted: Vec<u8>,
}

impl Screen {
    pub fn row_text(&self, row: u16) -> String {
        self.cells
            .get(usize::from(row))
            .map(|r| {
                r.iter()
                    .map(|c| {
                        if c.text.is_empty() {
                            " "
                        } else {
                            c.text.as_str()
                        }
                    })
                    .collect::<String>()
            })
            .unwrap_or_default()
    }

    pub fn contains(&self, text: &str) -> bool {
        (0..self.rows).any(|r| self.row_text(r).contains(text))
    }

    /// Where `text` first appears: `(col, row)`.
    pub fn find(&self, text: &str) -> Option<(u16, u16)> {
        (0..self.rows).find_map(|r| {
            let line = self.row_text(r);
            line.find(text)
                .map(|byte| (u16::try_from(line[..byte].chars().count()).unwrap_or(0), r))
        })
    }

    pub fn cell(&self, col: u16, row: u16) -> Option<&Cell> {
        self.cells
            .get(usize::from(row))
            .and_then(|r| r.get(usize::from(col)))
    }

    pub fn fg_at(&self, col: u16, row: u16) -> Option<Color> {
        self.cell(col, row).map(|c| c.fg)
    }

    pub fn bg_at(&self, col: u16, row: u16) -> Option<Color> {
        self.cell(col, row).map(|c| c.bg)
    }

    /// Render the grid as HTML in JetBrains Mono 13px / 20px, the artboards'
    /// metrics (QA.md §1).
    pub fn html(&self, title: &str) -> String {
        let mut body = String::new();
        for row in &self.cells {
            body.push_str("<div class=\"r\">");
            let mut run = String::new();
            let mut style = String::new();
            // Every span is exactly its cell count wide, so a glyph drawn
            // from a fallback font can't push the rest of the row.
            let mut cells = 0usize;
            for c in row {
                let (mut fg, mut bg) = (c.fg.rgb(DEFAULT_FG), c.bg.rgb(DEFAULT_BG));
                if c.inverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let s = format!(
                    "color:#{:02x}{:02x}{:02x};background:#{:02x}{:02x}{:02x}{}",
                    fg.0,
                    fg.1,
                    fg.2,
                    bg.0,
                    bg.1,
                    bg.2,
                    if c.bold { ";font-weight:700" } else { "" }
                );
                if s != style && !run.is_empty() {
                    body.push_str(&format!(
                        "<span style=\"{style};width:{cells}ch\">{run}</span>"
                    ));
                    run.clear();
                    cells = 0;
                }
                cells += 1;
                style = s;
                let t = if c.text.is_empty() {
                    " "
                } else {
                    c.text.as_str()
                };
                run.push_str(
                    &t.replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;"),
                );
            }
            if !run.is_empty() {
                body.push_str(&format!(
                    "<span style=\"{style};width:{cells}ch\">{run}</span>"
                ));
            }
            body.push_str("</div>");
        }
        let (r, g, b) = DEFAULT_BG;
        format!(
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"><meta name=\"spektr-size\" content=\"{}x{}\"><title>{title}</title>\
<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap\">\
<style>html,body{{margin:0;background:#{r:02x}{g:02x}{b:02x}}}\
.t{{font-family:'JetBrains Mono',monospace;font-size:13px;line-height:20px;white-space:pre;padding:0}}\
.r{{height:20px;white-space:pre}}.r span{{display:inline-block;height:20px;overflow:hidden;vertical-align:top}}</style></head>\
<body><div class=\"t\">{body}</div></body></html>\n",
            self.cols, self.rows
        )
    }
}

pub struct Term {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    size: Size,
}

fn nz(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n.max(1)).unwrap_or(std::num::NonZeroU16::MIN)
}

impl Term {
    /// Run `spektr <args>` in a PTY of `size`.
    pub fn spawn(args: &[&str], size: Size, env: &[(String, String)], cwd: &Path) -> Result<Self> {
        let bin = assert_cmd::cargo::cargo_bin("spektr");
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("openpty: {e}"))?;
        let mut cmd = CommandBuilder::new(bin);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env_clear();
        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            if v.is_empty() {
                cmd.env_remove(k);
            } else {
                cmd.env(k, v);
            }
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow!("spawn: {e}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow!("reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow!("writer: {e}"))?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            nz(size.rows),
            nz(size.cols),
            0,
        )));
        let p = Arc::clone(&parser);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => p
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .process(&buf[..n]),
                }
            }
        });
        Ok(Self {
            master: pair.master,
            writer,
            child,
            parser,
            size,
        })
    }

    pub fn screen(&self) -> Screen {
        let parser = self.parser.lock().unwrap_or_else(|e| e.into_inner());
        let s = parser.screen();
        let (rows, cols) = (self.size.rows, self.size.cols);
        let cells = (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| match s.cell(r, c) {
                        Some(cell) => Cell {
                            text: cell.contents().to_string(),
                            fg: cell.fgcolor().into(),
                            bg: cell.bgcolor().into(),
                            bold: cell.bold(),
                            inverse: cell.inverse(),
                        },
                        None => Cell {
                            text: String::new(),
                            fg: Color::Default,
                            bg: Color::Default,
                            bold: false,
                            inverse: false,
                        },
                    })
                    .collect()
            })
            .collect();
        Screen {
            cols,
            rows,
            cells,
            cursor: s.cursor_position(),
            contents: s.contents(),
            formatted: s.contents_formatted().into(),
        }
    }

    pub fn wait_for(
        &self,
        timeout: Duration,
        mut pred: impl FnMut(&Screen) -> bool,
    ) -> Result<Screen> {
        let start = Instant::now();
        loop {
            let s = self.screen();
            if pred(&s) {
                return Ok(s);
            }
            if start.elapsed() > timeout {
                bail!("timed out after {timeout:?}; screen:\n{}", s.contents);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Wait until the screen (text or attributes) differs from `before`.
    pub fn wait_changed(&self, before: &Screen, timeout: Duration) -> Result<Screen> {
        self.wait_for(timeout, |s| s.formatted != before.formatted)
            .context("waiting for the screen to change")
    }

    /// Wait until the screen hasn't changed for `quiet`.
    pub fn settle(&self, quiet: Duration, timeout: Duration) -> Result<Screen> {
        let start = Instant::now();
        let mut last = self.screen().formatted;
        let mut since = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(20));
            let now = self.screen().formatted;
            if now != last {
                last = now;
                since = Instant::now();
            } else if since.elapsed() >= quiet {
                return Ok(self.screen());
            }
            if start.elapsed() > timeout {
                bail!("screen didn't settle within {timeout:?}");
            }
        }
    }

    pub fn wait_for_text(&self, text: &str, timeout: Duration) -> Result<Screen> {
        self.wait_for(timeout, |s| s.contains(text))
            .with_context(|| format!("waiting for {text:?}"))
    }

    /// Write input. Input to a process that has already exited is discarded,
    /// as a real terminal would (macOS reports EIO for it).
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let r = self
            .writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush());
        match r {
            Ok(()) => Ok(()),
            Err(e) if self.child.try_wait()?.is_some() => {
                let _ = e;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The key DSL: whitespace-separated keys. Named keys: `↵`/`enter`,
    /// `esc`, `tab`, `shift-tab`, `⌫`/`bs`, `space`, `↑ ↓ ← →`/`up down left
    /// right`, `pgup`, `pgdn`, `home`, `end`, `ctrl-<c>`; `text:<s>` types a
    /// string; anything else is typed as-is.
    pub fn keys(&mut self, dsl: &str) -> Result<()> {
        for tok in dsl.split_whitespace() {
            let bytes: Vec<u8> = match tok {
                "↵" | "enter" => b"\r".to_vec(),
                "esc" => b"\x1b".to_vec(),
                "tab" => b"\t".to_vec(),
                "shift-tab" => b"\x1b[Z".to_vec(),
                "⌫" | "bs" => b"\x7f".to_vec(),
                "space" => b" ".to_vec(),
                "↑" | "up" => b"\x1b[A".to_vec(),
                "↓" | "down" => b"\x1b[B".to_vec(),
                "→" | "right" => b"\x1b[C".to_vec(),
                "←" | "left" => b"\x1b[D".to_vec(),
                "pgup" => b"\x1b[5~".to_vec(),
                "pgdn" => b"\x1b[6~".to_vec(),
                "home" => b"\x1b[H".to_vec(),
                "end" => b"\x1b[F".to_vec(),
                t if t.starts_with("ctrl-") && t.len() == 6 => {
                    let c = t.as_bytes()[5].to_ascii_lowercase();
                    vec![c & 0x1f]
                }
                t if t.starts_with("text:") => t.as_bytes()[5..].to_vec(),
                t => t.as_bytes().to_vec(),
            };
            self.write(&bytes)?;
            std::thread::sleep(Duration::from_millis(15));
        }
        Ok(())
    }

    fn sgr(&mut self, button: u8, col: u16, row: u16, press: bool) -> Result<()> {
        let s = format!(
            "\x1b[<{button};{};{}{}",
            col + 1,
            row + 1,
            if press { 'M' } else { 'm' }
        );
        self.write(s.as_bytes())
    }

    /// SGR (1006) mouse sequences; `col`/`row` are 0-based cells.
    pub fn mouse_click(&mut self, col: u16, row: u16) -> Result<()> {
        self.sgr(0, col, row, true)?;
        self.sgr(0, col, row, false)
    }

    pub fn double_click(&mut self, col: u16, row: u16) -> Result<()> {
        self.mouse_click(col, row)?;
        self.mouse_click(col, row)
    }

    pub fn right_click(&mut self, col: u16, row: u16) -> Result<()> {
        self.sgr(2, col, row, true)?;
        self.sgr(2, col, row, false)
    }

    pub fn wheel(&mut self, col: u16, row: u16, down: bool) -> Result<()> {
        self.sgr(if down { 65 } else { 64 }, col, row, true)
    }

    pub fn hover(&mut self, col: u16, row: u16) -> Result<()> {
        self.sgr(35, col, row, true)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.size = Size { cols, rows };
        // Resize the parser first and hold its lock across the PTY resize:
        // otherwise the app's redraw at the new size can reach the parser
        // while it still has the old size, and scroll off the top rows.
        let mut parser = self.parser.lock().unwrap_or_else(|e| e.into_inner());
        parser.set_size(nz(rows), nz(cols));
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("resize: {e}"))?;
        drop(parser);
        Ok(())
    }

    /// Wait for the process to exit; returns its exit code.
    pub fn wait_exit(&mut self, timeout: Duration) -> Result<u32> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                // let the reader thread drain the last output
                std::thread::sleep(Duration::from_millis(50));
                return Ok(status.exit_code());
            }
            if start.elapsed() > timeout {
                let _ = self.child.kill();
                bail!(
                    "process didn't exit within {timeout:?}; screen:\n{}",
                    self.screen().contents
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Write `<stem>.txt` (plain), `<stem>.ansi` (colored) and `<stem>.html`
    /// (rendered).
    pub fn save_frame(&self, stem: &Path) -> Result<()> {
        let s = self.screen();
        save_screen(&s, stem)
    }
}

pub fn save_screen(s: &Screen, stem: &Path) -> Result<()> {
    if let Some(dir) = stem.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = stem
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut txt = String::new();
    for r in 0..s.rows {
        txt.push_str(s.row_text(r).trim_end());
        txt.push('\n');
    }
    std::fs::write(stem.with_extension("txt"), txt)?;
    std::fs::write(stem.with_extension("ansi"), &s.formatted)?;
    std::fs::write(stem.with_extension("html"), s.html(&name))?;
    Ok(())
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
