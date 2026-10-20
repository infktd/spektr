//! Cell-width helpers. Every string the TUI draws is measured in terminal
//! cells, never bytes or chars.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate to at most `max` cells, ending in `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Truncate, then pad with spaces to exactly `w` cells.
pub fn fit(s: &str, w: usize) -> String {
    let mut t = truncate(s, w);
    let tw = width(&t);
    t.extend(std::iter::repeat_n(' ', w.saturating_sub(tw)));
    t
}

/// The end of `s` in at most `w` cells, with a leading `…` when cut (an
/// input being typed into: the caret end stays visible).
pub fn tail(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out: Vec<char> = Vec::new();
    let mut used = 1;
    for c in s.chars().rev() {
        let cw = width(c.encode_utf8(&mut [0; 4]));
        if used + cw > w {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out.into_iter().rev().collect()
}

/// Right-align in `w` cells (truncating on the left side's terms).
pub fn right(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let pad = w.saturating_sub(width(&t));
    format!("{}{t}", " ".repeat(pad))
}

/// `~/…` for paths under `home`.
pub fn tilde(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home {
        Some(h) if path.starts_with(h) => match path.strip_prefix(h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        },
        _ => path.display().to_string(),
    }
}

/// `1.8 MB`, `412 KB`, `96 B`.
pub fn bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    if f >= K * K * K {
        format!("{:.1} GB", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.1} MB", f / (K * K))
    } else if f >= K {
        format!("{:.0} KB", f / K)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails() {
        assert_eq!(tail("on 2026-10-16.", 20), "on 2026-10-16.");
        assert_eq!(tail("active on 2026-10-16.", 12), "…2026-10-16.");
        assert_eq!(width(&tail("◎ Jordan Marlow", 6)), 6);
    }

    #[test]
    fn widths() {
        assert_eq!(truncate("jordan.marlow@mail.example", 10), "jordan.ma…");
        assert_eq!(width(&truncate("◎ Jordan", 4)), 4);
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(right("7", 3), "  7");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(bytes(1_887_437), "1.8 MB");
    }
}
