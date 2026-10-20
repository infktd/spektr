//! Image previews (TUI_SPEC §9): the protocol is picked once at startup;
//! decoding, resizing and encoding run off the render path; encoded images
//! are cached per `(sha256, area size)`, up to 64 entries.

use std::collections::{HashMap, VecDeque};

use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

/// Encoded images kept.
pub const CACHE: usize = 64;

/// `images` in config.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Halfblocks,
    Off,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Mode::Auto,
            "kitty" => Mode::Kitty,
            "sixel" => Mode::Sixel,
            "iterm2" => Mode::Iterm2,
            "halfblocks" => Mode::Halfblocks,
            "off" => Mode::Off,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Kitty => "kitty",
            Mode::Sixel => "sixel",
            Mode::Iterm2 => "iterm2",
            Mode::Halfblocks => "halfblocks",
            Mode::Off => "off",
        }
    }
}

fn protocol_name(p: ProtocolType) -> &'static str {
    match p {
        ProtocolType::Halfblocks => "halfblocks",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Kitty => "kitty",
        ProtocolType::Iterm2 => "iterm2",
    }
}

/// Pick the protocol: query the terminal for `auto` (or to learn the font
/// size for a forced protocol). Call before raw mode is entered; the query
/// manages raw mode itself. `None` when images are off.
pub fn detect(mode: Mode) -> (Option<Picker>, String) {
    let forced = match mode {
        Mode::Off => return (None, "off".into()),
        Mode::Halfblocks => return (Some(Picker::halfblocks()), "halfblocks".into()),
        Mode::Auto => None,
        Mode::Kitty => Some(ProtocolType::Kitty),
        Mode::Sixel => Some(ProtocolType::Sixel),
        Mode::Iterm2 => Some(ProtocolType::Iterm2),
    };
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if let Some(p) = forced {
        picker.set_protocol_type(p);
    }
    let name = protocol_name(picker.protocol_type());
    let name = if picker.tmux_detected() {
        format!("{name} (tmux)")
    } else {
        name.to_string()
    };
    (Some(picker), name)
}

/// Cache key: the blob and the cells it's fitted into.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub sha: String,
    pub w: u16,
    pub h: u16,
}

/// An encoded image and the cells it covers (aspect preserved).
pub struct Ready {
    pub proto: Protocol,
    pub w: u16,
    pub h: u16,
}

impl std::fmt::Debug for Ready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ready({}×{})", self.w, self.h)
    }
}

#[derive(Debug)]
pub enum Slot {
    Loading,
    Ready(Ready),
    Failed(String),
}

/// Decode, fit and encode `bytes` for a `w × h` cell area. Blocking.
pub fn encode(picker: &Picker, bytes: &[u8], w: u16, h: u16) -> Result<Ready, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let proto = picker
        .new_protocol(img, Size::new(w, h), Resize::Fit(None))
        .map_err(|e| e.to_string())?;
    let size = proto.size();
    Ok(Ready {
        proto,
        w: size.width,
        h: size.height,
    })
}

#[derive(Debug, Default)]
pub struct Images {
    /// The protocol in use (`kitty`, `halfblocks (tmux)`, `off`), for the
    /// launcher's doctor panel.
    pub protocol: String,
    pub enabled: bool,
    slots: HashMap<Key, Slot>,
    order: VecDeque<Key>,
}

impl Images {
    pub fn new(protocol: String, enabled: bool) -> Self {
        Self {
            protocol,
            enabled,
            ..Self::default()
        }
    }

    pub fn get(&self, k: &Key) -> Option<&Slot> {
        self.slots.get(k)
    }

    /// Mark `k` loading; `false` when it's already known.
    pub fn request(&mut self, k: &Key) -> bool {
        if !self.enabled || self.slots.contains_key(k) {
            return false;
        }
        self.insert(k.clone(), Slot::Loading);
        true
    }

    pub fn insert(&mut self, k: Key, slot: Slot) {
        if self.slots.insert(k.clone(), slot).is_none() {
            self.order.push_back(k);
        }
        while self.order.len() > CACHE {
            if let Some(old) = self.order.pop_front() {
                self.slots.remove(&old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfblocks_encode_fits_the_area() {
        let mut png = Vec::new();
        let img = image::RgbImage::from_fn(64, 32, |x, _| image::Rgb([x as u8 * 4, 40, 90]));
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let r = encode(&Picker::halfblocks(), &png, 20, 10).unwrap();
        assert!(r.w <= 20 && r.h <= 10 && r.w > 0 && r.h > 0, "{r:?}");
        assert!(encode(&Picker::halfblocks(), b"not an image", 20, 10).is_err());
    }

    #[test]
    fn cache_is_bounded() {
        let mut im = Images::new("halfblocks".into(), true);
        for i in 0..(CACHE + 10) {
            let k = Key {
                sha: i.to_string(),
                w: 1,
                h: 1,
            };
            assert!(im.request(&k));
        }
        assert_eq!(im.slots.len(), CACHE);
        assert!(
            im.get(&Key {
                sha: "0".into(),
                w: 1,
                h: 1
            })
            .is_none()
        );
    }
}
