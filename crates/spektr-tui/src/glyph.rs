//! Glyphs, markers and display strings (TUI_SPEC §3).

use spektr_engine::EntitySummary;
use spektr_model::{CoreType, EntityFlags, EntityType, Strength};

use crate::theme::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphSet {
    #[default]
    Unicode,
    Ascii,
}

impl GlyphSet {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unicode" => Some(GlyphSet::Unicode),
            "ascii" => Some(GlyphSet::Ascii),
            _ => None,
        }
    }

    pub fn entity(self, ty: &EntityType) -> char {
        match self {
            GlyphSet::Unicode => ty.glyph(),
            GlyphSet::Ascii => ty.ascii(),
        }
    }

    pub fn strength(self, s: Strength) -> &'static str {
        match (self, s) {
            (GlyphSet::Unicode, s) => s.bar(),
            (GlyphSet::Ascii, Strength::Single) => "#--",
            (GlyphSet::Ascii, Strength::Corroborated) => "##-",
            (GlyphSet::Ascii, Strength::Confirmed) => "###",
        }
    }

    /// A marker by name; the ASCII set swaps every non-ASCII glyph.
    pub fn m(self, m: Marker) -> &'static str {
        let (u, a) = match m {
            Marker::Cursor => ("▌", ">"),
            Marker::New => ("•", "*"),
            Marker::Seed => ("⌂", "^"),
            Marker::Hub => ("⊛", "H"),
            Marker::Blocked => ("⊘", "/"),
            Marker::Correlated => ("≈", "="),
            Marker::Running => ("▸", ">"),
            Marker::Ok => ("✓", "v"),
            Marker::Failed => ("✕", "x"),
            Marker::Attention => ("!", "!"),
            Marker::AfterLastSeen => ("▲", "^"),
            Marker::Expanded => ("▾", "v"),
            Marker::Collapsed => ("▸", ">"),
            Marker::More => ("···", "..."),
            Marker::TreeMid => ("├", "|"),
            Marker::TreeEnd => ("└", "`"),
            Marker::In => ("←", "<"),
            Marker::Out => ("→", ">"),
            Marker::Sym => ("↔", "-"),
            Marker::Crumb => ("›", ">"),
            Marker::Dot => ("·", "."),
            Marker::Bullet => ("●", "*"),
            Marker::Pending => ("○", "o"),
            Marker::Enter => ("↵", "enter"),
            Marker::Backspace => ("⌫", "bksp"),
            Marker::Rule => ("─", "-"),
            Marker::VRule => ("│", "|"),
            Marker::Link => ("↗", "^"),
        };
        match self {
            GlyphSet::Unicode => u,
            GlyphSet::Ascii => a,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Cursor,
    New,
    Seed,
    Hub,
    Blocked,
    Correlated,
    Running,
    Ok,
    Failed,
    Attention,
    AfterLastSeen,
    Expanded,
    Collapsed,
    More,
    TreeMid,
    TreeEnd,
    In,
    Out,
    Sym,
    Crumb,
    Dot,
    Bullet,
    Pending,
    Enter,
    Backspace,
    Rule,
    VRule,
    Link,
}

/// The token a strength bar is drawn in: single muted, corroborated fg,
/// confirmed ok.
pub fn strength_token(s: Strength) -> Token {
    match s {
        Strength::Single => Token::FgMuted,
        Strength::Corroborated => Token::Fg,
        Strength::Confirmed => Token::Ok,
    }
}

/// How an entity's value is shown: an account `github:jmarlow-wav` reads
/// `github · jmarlow-wav`.
pub fn display_value(e: &EntitySummary, g: GlyphSet) -> String {
    if e.ty.is(CoreType::Account)
        && let Some((platform, handle)) = e.value.split_once(':')
    {
        return format!("{platform} {} {handle}", g.m(Marker::Dot));
    }
    e.value.clone()
}

/// An account's `(platform, handle)`, for column-aligned groups.
pub fn account_parts(e: &EntitySummary) -> Option<(&str, &str)> {
    if e.ty.is(CoreType::Account) {
        e.value.split_once(':')
    } else {
        None
    }
}

/// The right-aligned marker for an entity row: `subject`, `⌂`, `⊛ hub`,
/// `⊘`.
pub fn row_marker(flags: EntityFlags, g: GlyphSet) -> Option<(String, Token)> {
    if flags.contains(EntityFlags::SUBJECT) {
        Some(("subject".into(), Token::FgMuted))
    } else if flags.contains(EntityFlags::SEED) {
        Some((g.m(Marker::Seed).into(), Token::FgMuted))
    } else if flags.contains(EntityFlags::HUB) {
        Some((format!("{} hub", g.m(Marker::Hub)), Token::Warn))
    } else if flags.contains(EntityFlags::OUT_OF_SCOPE) {
        Some((g.m(Marker::Blocked).into(), Token::FgMuted))
    } else {
        None
    }
}
