//! Design tokens (TUI_SPEC §2). **No other file may name a color.**
//!
//! Three modes: truecolor, 256-color (auto-detected from `COLORTERM`,
//! overridable in config) and monochrome (`NO_COLOR`), where focus is bold
//! and the cursor row and mode pill are reverse video.

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMode {
    TrueColor,
    Indexed,
    Mono,
}

impl ColorMode {
    /// `NO_COLOR` wins; then an explicit config value (`truecolor`, `256`,
    /// `none`); then `COLORTERM`.
    pub fn detect(no_color: bool, config: Option<&str>, colorterm: Option<&str>) -> Self {
        if no_color {
            return ColorMode::Mono;
        }
        match config {
            Some("truecolor" | "24bit") => return ColorMode::TrueColor,
            Some("256") => return ColorMode::Indexed,
            Some("none" | "mono") => return ColorMode::Mono,
            _ => {}
        }
        match colorterm {
            Some(c) if c.contains("truecolor") || c.contains("24bit") => ColorMode::TrueColor,
            _ => ColorMode::Indexed,
        }
    }
}

/// The semantic tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    /// Ground.
    Bg,
    /// Overlays: palette, menus, dialogs.
    BgRaised,
    /// Cursor row background.
    Selection,
    /// Pane borders, rules.
    Border,
    /// Tree glyphs, connectors. Decoration only, never text.
    Faint,
    /// Metadata, labels, hints; the lightest color allowed for text.
    FgMuted,
    /// Content.
    Fg,
    /// Focus border, cursor bar, live state, correlated edges, mode pill.
    Accent,
    /// Done, verified, `confirmed` strength.
    Ok,
    /// Hub, needs review, after-last-seen, COMMAND pill.
    Warn,
    /// Failure, broken chain.
    Hostile,
}

impl Token {
    pub const ALL: [Token; 11] = [
        Token::Bg,
        Token::BgRaised,
        Token::Selection,
        Token::Border,
        Token::Faint,
        Token::FgMuted,
        Token::Fg,
        Token::Accent,
        Token::Ok,
        Token::Warn,
        Token::Hostile,
    ];

    /// `(truecolor, 256-color index)`.
    pub const fn values(self) -> ((u8, u8, u8), u8) {
        match self {
            Token::Bg => ((0x0D, 0x0E, 0x10), 233),
            Token::BgRaised => ((0x11, 0x13, 0x17), 234),
            Token::Selection => ((0x1B, 0x20, 0x28), 235),
            Token::Border => ((0x2A, 0x2E, 0x35), 236),
            Token::Faint => ((0x4A, 0x4D, 0x53), 239),
            Token::FgMuted => ((0x8E, 0x8A, 0x83), 245),
            Token::Fg => ((0xD8, 0xD5, 0xCE), 252),
            Token::Accent => ((0x8E, 0xC5, 0xFF), 117),
            Token::Ok => ((0x8F, 0xCB, 0x9B), 114),
            Token::Warn => ((0xE3, 0xB4, 0x67), 179),
            Token::Hostile => ((0xF0, 0x8A, 0x7E), 210),
        }
    }
}

/// The launcher's spectral strip: the only multicolor element in the app
/// (TUI_SPEC §5). Truecolor and 256-color values.
pub const SPECTRUM: [((u8, u8, u8), u8); 6] = [
    ((0xF0, 0x8A, 0x7E), 210),
    ((0xE3, 0xB4, 0x67), 179),
    ((0xD9, 0xD2, 0x7A), 186),
    ((0x8F, 0xCB, 0x9B), 114),
    ((0x8E, 0xC5, 0xFF), 117),
    ((0xB8, 0xA4, 0xF0), 147),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub mode: ColorMode,
}

impl Theme {
    pub fn new(mode: ColorMode) -> Self {
        Self { mode }
    }

    pub fn color(&self, t: Token) -> Color {
        let ((r, g, b), idx) = t.values();
        match self.mode {
            ColorMode::TrueColor => Color::Rgb(r, g, b),
            ColorMode::Indexed => Color::Indexed(idx),
            ColorMode::Mono => Color::Reset,
        }
    }

    pub fn spectrum(&self, i: usize) -> Color {
        let ((r, g, b), idx) = SPECTRUM[i % SPECTRUM.len()];
        match self.mode {
            ColorMode::TrueColor => Color::Rgb(r, g, b),
            ColorMode::Indexed => Color::Indexed(idx),
            ColorMode::Mono => Color::Reset,
        }
    }

    pub fn fg(&self, t: Token) -> Style {
        Style::new().fg(self.color(t))
    }

    /// The ground: every screen starts from this.
    pub fn base(&self) -> Style {
        Style::new()
            .fg(self.color(Token::Fg))
            .bg(self.color(Token::Bg))
    }

    pub fn raised(&self) -> Style {
        Style::new()
            .fg(self.color(Token::Fg))
            .bg(self.color(Token::BgRaised))
    }

    pub fn text(&self) -> Style {
        self.fg(Token::Fg)
    }

    pub fn muted(&self) -> Style {
        self.fg(Token::FgMuted)
    }

    /// Decoration only (tree glyphs, connectors, rules).
    pub fn faint(&self) -> Style {
        self.fg(Token::Faint)
    }

    pub fn accent(&self) -> Style {
        self.fg(Token::Accent)
    }

    pub fn ok(&self) -> Style {
        self.fg(Token::Ok)
    }

    pub fn warn(&self) -> Style {
        self.fg(Token::Warn)
    }

    pub fn hostile(&self) -> Style {
        self.fg(Token::Hostile)
    }

    /// The one thing you're looking at (focused entity name, finding title).
    pub fn focus_text(&self) -> Style {
        self.text().add_modifier(Modifier::BOLD)
    }

    pub fn border(&self, focused: bool) -> Style {
        if focused {
            let s = self.accent();
            if self.mode == ColorMode::Mono {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        } else {
            self.fg(Token::Border)
        }
    }

    pub fn title(&self, focused: bool) -> Style {
        if focused {
            let s = self.accent();
            if self.mode == ColorMode::Mono {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        } else {
            self.muted()
        }
    }

    /// The cursor row's background (`selection`), or reverse video in mono.
    pub fn cursor_row(&self) -> Style {
        match self.mode {
            ColorMode::Mono => Style::new().add_modifier(Modifier::REVERSED),
            _ => Style::new().bg(self.color(Token::Selection)),
        }
    }

    /// Pointer hover: the same subtle background as the cursor row.
    pub fn hover_row(&self) -> Style {
        match self.mode {
            ColorMode::Mono => Style::new().add_modifier(Modifier::UNDERLINED.complement()),
            _ => Style::new().bg(self.color(Token::Selection)),
        }
    }

    /// The text-input caret: a reversed cell.
    pub fn caret(&self) -> Style {
        self.text().add_modifier(Modifier::REVERSED)
    }

    /// The `▌` cursor bar.
    pub fn cursor_bar(&self) -> Style {
        self.accent()
    }

    /// The mode pill: the only filled block of color on screen.
    pub fn pill(&self, t: Token) -> Style {
        match self.mode {
            ColorMode::Mono => Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            _ => Style::new()
                .fg(self.color(Token::Bg))
                .bg(self.color(t))
                .add_modifier(Modifier::BOLD),
        }
    }

    /// The workspace behind an overlay: muted and faint only.
    pub fn dimmed(&self, s: Style) -> Style {
        if self.mode == ColorMode::Mono {
            return s
                .remove_modifier(Modifier::BOLD | Modifier::REVERSED)
                .add_modifier(Modifier::DIM);
        }
        let fg = match s.fg {
            Some(c)
                if c == self.color(Token::Fg)
                    || c == self.color(Token::Accent)
                    || c == self.color(Token::Ok)
                    || c == self.color(Token::Warn)
                    || c == self.color(Token::Hostile) =>
            {
                self.color(Token::FgMuted)
            }
            Some(c) if c == self.color(Token::FgMuted) => self.color(Token::Faint),
            Some(c) => c,
            None => self.color(Token::FgMuted),
        };
        let bg = match s.bg {
            Some(c) if c == self.color(Token::Selection) => self.color(Token::Bg),
            other => other.unwrap_or(self.color(Token::Bg)),
        };
        Style::new().fg(fg).bg(bg).remove_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        assert_eq!(
            ColorMode::detect(true, Some("truecolor"), Some("truecolor")),
            ColorMode::Mono
        );
        assert_eq!(
            ColorMode::detect(false, None, Some("truecolor")),
            ColorMode::TrueColor
        );
        assert_eq!(ColorMode::detect(false, None, None), ColorMode::Indexed);
        assert_eq!(
            ColorMode::detect(false, Some("256"), Some("truecolor")),
            ColorMode::Indexed
        );
    }

    #[test]
    fn tokens_match_the_spec_table() {
        let t = Theme::new(ColorMode::TrueColor);
        assert_eq!(t.color(Token::Accent), Color::Rgb(0x8E, 0xC5, 0xFF));
        assert_eq!(
            Theme::new(ColorMode::Indexed).color(Token::Hostile),
            Color::Indexed(210)
        );
        assert_eq!(Theme::new(ColorMode::Mono).color(Token::Fg), Color::Reset);
    }
}
