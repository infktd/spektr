//! Account platforms and their handle rules (DATA_MODEL §1.4).
//!
//! Built-in platforms live in [`BUILTIN`]. Every platform in the
//! username-sweep site list must appear here (a test in `spektr-modules`
//! enforces it). Modules may declare more platforms in their manifest; unknown
//! platforms fall back to [`fallback_handle`].

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::text::{has_control, nfkc};

/// How a handle is written on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleForm {
    /// A single name; characters limited to ASCII letters, digits and
    /// `extra`.
    Plain { extra: &'static str, max: usize },
    /// Any non-whitespace, non-control text (declared or fallback platforms).
    Any,
    /// `user@instance`, e.g. Mastodon. The instance is a hostname.
    UserAtInstance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub id: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub case_sensitive: bool,
    pub form: HandleForm,
    /// `{handle}`, or `{user}` and `{instance}` for `UserAtInstance`.
    pub profile_url: Option<Cow<'static, str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandleError {
    #[error("empty handle")]
    Empty,
    #[error("handle longer than {0} characters")]
    TooLong(usize),
    #[error("character `{0}` is not allowed in {1} handles")]
    BadChar(char, String),
    #[error("{0} handles look like user@instance")]
    NeedsInstance(String),
    #[error("invalid instance host `{0}`")]
    BadInstance(String),
    #[error("platform id must match [a-z0-9.-]{{1,32}}: `{0}`")]
    BadPlatformId(String),
}

impl Platform {
    /// The normalized handle (the part of the account key after `platform:`).
    pub fn normalize_handle(&self, raw: &str) -> Result<String, HandleError> {
        let trimmed = raw.trim();
        let trimmed = trimmed.strip_prefix('@').unwrap_or(trimmed);
        if trimmed.is_empty() {
            return Err(HandleError::Empty);
        }
        let fold = |s: &str| {
            if self.case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            }
        };
        match self.form {
            HandleForm::Plain { extra, max } => {
                if let Some(c) = trimmed
                    .chars()
                    .find(|c| !(c.is_ascii_alphanumeric() || extra.contains(*c)))
                {
                    return Err(HandleError::BadChar(c, self.name.to_string()));
                }
                if trimmed.chars().count() > max {
                    return Err(HandleError::TooLong(max));
                }
                Ok(fold(trimmed))
            }
            HandleForm::Any => {
                let h = nfkc(trimmed);
                if let Some(c) = h.chars().find(|c| c.is_whitespace() || c.is_control()) {
                    return Err(HandleError::BadChar(c, self.name.to_string()));
                }
                Ok(fold(&h))
            }
            HandleForm::UserAtInstance => {
                let (user, instance) = trimmed
                    .split_once('@')
                    .ok_or_else(|| HandleError::NeedsInstance(self.name.to_string()))?;
                let user = nfkc(user);
                if user.is_empty() {
                    return Err(HandleError::Empty);
                }
                if let Some(c) = user
                    .chars()
                    .find(|c| c.is_whitespace() || c.is_control() || *c == '@')
                {
                    return Err(HandleError::BadChar(c, self.name.to_string()));
                }
                let instance = crate::normalize::normalize_hostname(instance)
                    .map_err(|_| HandleError::BadInstance(instance.to_string()))?
                    .ascii;
                Ok(format!("{}@{}", fold(&user), instance))
            }
        }
    }

    /// The profile URL for a normalized handle, if the platform has one.
    pub fn profile_url(&self, handle: &str) -> Option<String> {
        let template = self.profile_url.as_deref()?;
        match self.form {
            HandleForm::UserAtInstance => {
                let (user, instance) = handle.split_once('@')?;
                Some(
                    template
                        .replace("{user}", user)
                        .replace("{instance}", instance),
                )
            }
            _ => Some(template.replace("{handle}", handle)),
        }
    }
}

/// The fallback for platforms nobody declared: the platform id must match
/// `[a-z0-9.-]{1,32}`; the handle is trimmed, NFKC-normalized and lowercased.
pub fn fallback_handle(platform: &str, raw: &str) -> Result<String, HandleError> {
    if !valid_platform_id(platform) {
        return Err(HandleError::BadPlatformId(platform.to_string()));
    }
    let h = nfkc(raw.trim());
    let h = h.strip_prefix('@').unwrap_or(&h);
    if h.is_empty() {
        return Err(HandleError::Empty);
    }
    if has_control(h) || h.chars().any(char::is_whitespace) {
        return Err(HandleError::BadChar(
            h.chars()
                .find(|c| c.is_whitespace() || c.is_control())
                .unwrap_or(' '),
            platform.to_string(),
        ));
    }
    Ok(h.to_lowercase())
}

pub fn valid_platform_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// Built-in platforms plus any declared by installed modules.
#[derive(Debug, Clone)]
pub struct PlatformRegistry {
    by_id: BTreeMap<String, Platform>,
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeclareError {
    #[error("platform `{0}` is built in and can't be redeclared")]
    Builtin(String),
    #[error(transparent)]
    Handle(#[from] HandleError),
}

impl PlatformRegistry {
    pub fn builtin() -> Self {
        let by_id = BUILTIN
            .iter()
            .map(|d| (d.id.to_string(), d.to_platform()))
            .collect();
        Self { by_id }
    }

    pub fn get(&self, id: &str) -> Option<&Platform> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Platform> {
        self.by_id.values()
    }

    pub fn is_builtin(id: &str) -> bool {
        BUILTIN.iter().any(|d| d.id == id)
    }

    /// A module-declared platform (`[[module.platforms]]`). A profile URL
    /// containing `{instance}` makes handles `user@instance`.
    pub fn declare(
        &mut self,
        id: &str,
        case_sensitive: bool,
        profile_url: Option<&str>,
    ) -> Result<(), DeclareError> {
        if Self::is_builtin(id) {
            return Err(DeclareError::Builtin(id.to_string()));
        }
        if !valid_platform_id(id) {
            return Err(HandleError::BadPlatformId(id.to_string()).into());
        }
        let form = if profile_url.is_some_and(|u| u.contains("{instance}")) {
            HandleForm::UserAtInstance
        } else {
            HandleForm::Any
        };
        self.by_id.insert(
            id.to_string(),
            Platform {
                id: Cow::Owned(id.to_string()),
                name: Cow::Owned(id.to_string()),
                case_sensitive,
                form,
                profile_url: profile_url.map(|u| Cow::Owned(u.to_string())),
            },
        );
        Ok(())
    }
}

/// One row of the built-in table.
struct Def {
    id: &'static str,
    name: &'static str,
    case_sensitive: bool,
    form: HandleForm,
    url: &'static str,
}

impl Def {
    fn to_platform(&self) -> Platform {
        Platform {
            id: Cow::Borrowed(self.id),
            name: Cow::Borrowed(self.name),
            case_sensitive: self.case_sensitive,
            form: self.form,
            profile_url: (!self.url.is_empty()).then_some(Cow::Borrowed(self.url)),
        }
    }
}

const fn p(
    id: &'static str,
    name: &'static str,
    extra: &'static str,
    max: usize,
    url: &'static str,
) -> Def {
    Def {
        id,
        name,
        case_sensitive: false,
        form: HandleForm::Plain { extra, max },
        url,
    }
}

const fn cs(
    id: &'static str,
    name: &'static str,
    extra: &'static str,
    max: usize,
    url: &'static str,
) -> Def {
    Def {
        id,
        name,
        case_sensitive: true,
        form: HandleForm::Plain { extra, max },
        url,
    }
}

const fn fedi(id: &'static str, name: &'static str, url: &'static str) -> Def {
    Def {
        id,
        name,
        case_sensitive: false,
        form: HandleForm::UserAtInstance,
        url,
    }
}

/// Built-in platforms. Handle character sets are deliberately a little
/// generous: the point is a stable key, not enforcing each site's signup
/// rules.
const BUILTIN: &[Def] = &[
    // code & tech
    p("github", "GitHub", "-", 39, "https://github.com/{handle}"),
    p(
        "gitlab",
        "GitLab",
        "-_.",
        255,
        "https://gitlab.com/{handle}",
    ),
    p(
        "bitbucket",
        "Bitbucket",
        "-_",
        30,
        "https://bitbucket.org/{handle}/",
    ),
    p(
        "codeberg",
        "Codeberg",
        "-_.",
        40,
        "https://codeberg.org/{handle}",
    ),
    p(
        "sourceforge",
        "SourceForge",
        "-_",
        32,
        "https://sourceforge.net/u/{handle}/profile",
    ),
    p("npm", "npm", "-_.", 214, "https://www.npmjs.com/~{handle}"),
    p("pypi", "PyPI", "-_.", 50, "https://pypi.org/user/{handle}/"),
    p(
        "dockerhub",
        "Docker Hub",
        "-_",
        30,
        "https://hub.docker.com/u/{handle}",
    ),
    p(
        "huggingface",
        "Hugging Face",
        "-_.",
        42,
        "https://huggingface.co/{handle}",
    ),
    p(
        "kaggle",
        "Kaggle",
        "-_",
        40,
        "https://www.kaggle.com/{handle}",
    ),
    p("replit", "Replit", "-_", 40, "https://replit.com/@{handle}"),
    p(
        "codepen",
        "CodePen",
        "-_",
        40,
        "https://codepen.io/{handle}",
    ),
    p("devto", "DEV", "_", 30, "https://dev.to/{handle}"),
    p(
        "hashnode",
        "Hashnode",
        "-_",
        40,
        "https://hashnode.com/@{handle}",
    ),
    p(
        "hackernews",
        "Hacker News",
        "-_",
        15,
        "https://news.ycombinator.com/user?id={handle}",
    ),
    p(
        "lobsters",
        "Lobsters",
        "-_",
        50,
        "https://lobste.rs/~{handle}",
    ),
    p(
        "stackoverflow",
        "Stack Overflow",
        "-_.",
        40,
        "https://stackoverflow.com/users/{handle}",
    ),
    p(
        "leetcode",
        "LeetCode",
        "-_",
        40,
        "https://leetcode.com/u/{handle}/",
    ),
    p(
        "hackerrank",
        "HackerRank",
        "-_.",
        40,
        "https://www.hackerrank.com/profile/{handle}",
    ),
    p(
        "codewars",
        "Codewars",
        "-_.",
        40,
        "https://www.codewars.com/users/{handle}",
    ),
    p(
        "codeforces",
        "Codeforces",
        "-_.",
        24,
        "https://codeforces.com/profile/{handle}",
    ),
    p(
        "tryhackme",
        "TryHackMe",
        "-_.",
        40,
        "https://tryhackme.com/p/{handle}",
    ),
    p(
        "hackerone",
        "HackerOne",
        "-_",
        40,
        "https://hackerone.com/{handle}",
    ),
    p(
        "bugcrowd",
        "Bugcrowd",
        "-_",
        40,
        "https://bugcrowd.com/{handle}",
    ),
    p("keybase", "Keybase", "_", 16, "https://keybase.io/{handle}"),
    p(
        "launchpad",
        "Launchpad",
        "-.",
        40,
        "https://launchpad.net/~{handle}",
    ),
    p(
        "producthunt",
        "Product Hunt",
        "-_",
        40,
        "https://www.producthunt.com/@{handle}",
    ),
    p(
        "pastebin",
        "Pastebin",
        "-_",
        40,
        "https://pastebin.com/u/{handle}",
    ),
    p(
        "gravatar",
        "Gravatar",
        "-_.",
        64,
        "https://gravatar.com/{handle}",
    ),
    p(
        "wikipedia",
        "Wikipedia",
        "-_.",
        85,
        "https://en.wikipedia.org/wiki/User:{handle}",
    ),
    p(
        "openstreetmap",
        "OpenStreetMap",
        "-_.",
        255,
        "https://www.openstreetmap.org/user/{handle}",
    ),
    // social
    p(
        "bluesky",
        "Bluesky",
        "-.",
        253,
        "https://bsky.app/profile/{handle}",
    ),
    fedi("mastodon", "Mastodon", "https://{instance}/@{user}"),
    fedi("pixelfed", "Pixelfed", "https://{instance}/{user}"),
    p("twitter", "X (Twitter)", "_", 15, "https://x.com/{handle}"),
    p(
        "instagram",
        "Instagram",
        "._",
        30,
        "https://www.instagram.com/{handle}/",
    ),
    p(
        "threads",
        "Threads",
        "._",
        30,
        "https://www.threads.net/@{handle}",
    ),
    p(
        "facebook",
        "Facebook",
        ".",
        50,
        "https://www.facebook.com/{handle}",
    ),
    p(
        "tiktok",
        "TikTok",
        "._",
        24,
        "https://www.tiktok.com/@{handle}",
    ),
    p(
        "snapchat",
        "Snapchat",
        "-_.",
        15,
        "https://www.snapchat.com/add/{handle}",
    ),
    p(
        "reddit",
        "Reddit",
        "-_",
        20,
        "https://www.reddit.com/user/{handle}",
    ),
    p("tumblr", "Tumblr", "-", 32, "https://{handle}.tumblr.com/"),
    p(
        "pinterest",
        "Pinterest",
        "_",
        30,
        "https://www.pinterest.com/{handle}/",
    ),
    p(
        "linkedin",
        "LinkedIn",
        "-",
        100,
        "https://www.linkedin.com/in/{handle}",
    ),
    p(
        "xing",
        "XING",
        "_",
        100,
        "https://www.xing.com/profile/{handle}",
    ),
    p("telegram", "Telegram", "_", 32, "https://t.me/{handle}"),
    p("vk", "VK", "_.", 32, "https://vk.com/{handle}"),
    p("okru", "OK.ru", "_.", 40, "https://ok.ru/{handle}"),
    p("weibo", "Weibo", "_", 30, "https://weibo.com/{handle}"),
    p(
        "quora",
        "Quora",
        "-",
        80,
        "https://www.quora.com/profile/{handle}",
    ),
    p("askfm", "ASKfm", "_", 40, "https://ask.fm/{handle}"),
    p(
        "clubhouse",
        "Clubhouse",
        "_",
        30,
        "https://www.clubhouse.com/@{handle}",
    ),
    p("minds", "Minds", "_", 40, "https://www.minds.com/{handle}/"),
    p(
        "disqus",
        "Disqus",
        "-_",
        30,
        "https://disqus.com/by/{handle}/",
    ),
    p(
        "myspace",
        "Myspace",
        "-_.",
        40,
        "https://myspace.com/{handle}",
    ),
    p(
        "livejournal",
        "LiveJournal",
        "_-",
        15,
        "https://{handle}.livejournal.com/",
    ),
    p(
        "aboutme",
        "about.me",
        "-_.",
        40,
        "https://about.me/{handle}",
    ),
    p(
        "linktree",
        "Linktree",
        "-_.",
        30,
        "https://linktr.ee/{handle}",
    ),
    p("carrd", "Carrd", "-", 40, "https://{handle}.carrd.co/"),
    // writing & publishing
    p("medium", "Medium", "._", 30, "https://medium.com/@{handle}"),
    p(
        "substack",
        "Substack",
        "-",
        40,
        "https://{handle}.substack.com/",
    ),
    p(
        "wordpress",
        "WordPress.com",
        "-",
        50,
        "https://{handle}.wordpress.com/",
    ),
    p(
        "blogger",
        "Blogger",
        "-",
        50,
        "https://{handle}.blogspot.com/",
    ),
    p(
        "wattpad",
        "Wattpad",
        "-_",
        20,
        "https://www.wattpad.com/user/{handle}",
    ),
    p(
        "ao3",
        "Archive of Our Own",
        "_",
        40,
        "https://archiveofourown.org/users/{handle}",
    ),
    p(
        "goodreads",
        "Goodreads",
        "-_",
        60,
        "https://www.goodreads.com/{handle}",
    ),
    p(
        "scribd",
        "Scribd",
        "-_",
        60,
        "https://www.scribd.com/user/{handle}",
    ),
    p(
        "slideshare",
        "SlideShare",
        "-_",
        40,
        "https://www.slideshare.net/{handle}",
    ),
    p(
        "researchgate",
        "ResearchGate",
        "-_",
        100,
        "https://www.researchgate.net/profile/{handle}",
    ),
    p(
        "instructables",
        "Instructables",
        "-_",
        40,
        "https://www.instructables.com/member/{handle}/",
    ),
    p(
        "thingiverse",
        "Thingiverse",
        "-_",
        40,
        "https://www.thingiverse.com/{handle}/designs",
    ),
    // video, streaming, music
    p(
        "youtube",
        "YouTube",
        "-_.",
        30,
        "https://www.youtube.com/@{handle}",
    ),
    p(
        "twitch",
        "Twitch",
        "_",
        25,
        "https://www.twitch.tv/{handle}",
    ),
    p("kick", "Kick", "_", 25, "https://kick.com/{handle}"),
    p("vimeo", "Vimeo", "-_", 40, "https://vimeo.com/{handle}"),
    p(
        "dailymotion",
        "Dailymotion",
        "-_",
        40,
        "https://www.dailymotion.com/{handle}",
    ),
    p(
        "rumble",
        "Rumble",
        "-_",
        40,
        "https://rumble.com/user/{handle}",
    ),
    p(
        "soundcloud",
        "SoundCloud",
        "-_",
        25,
        "https://soundcloud.com/{handle}",
    ),
    p(
        "bandcamp",
        "Bandcamp",
        "-",
        40,
        "https://{handle}.bandcamp.com/",
    ),
    p(
        "mixcloud",
        "Mixcloud",
        "-_",
        40,
        "https://www.mixcloud.com/{handle}/",
    ),
    p(
        "audiomack",
        "Audiomack",
        "-_",
        40,
        "https://audiomack.com/{handle}",
    ),
    cs(
        "spotify",
        "Spotify",
        "-_.",
        40,
        "https://open.spotify.com/user/{handle}",
    ),
    p(
        "lastfm",
        "Last.fm",
        "-_",
        15,
        "https://www.last.fm/user/{handle}",
    ),
    p("genius", "Genius", "-_", 40, "https://genius.com/{handle}"),
    p(
        "discogs",
        "Discogs",
        "-_.",
        40,
        "https://www.discogs.com/user/{handle}",
    ),
    p(
        "rateyourmusic",
        "Rate Your Music",
        "-_",
        40,
        "https://rateyourmusic.com/~{handle}",
    ),
    p(
        "smule",
        "Smule",
        "-_.",
        40,
        "https://www.smule.com/{handle}",
    ),
    // photo & art
    p(
        "flickr",
        "Flickr",
        "-_@",
        50,
        "https://www.flickr.com/people/{handle}/",
    ),
    p("500px", "500px", "-_", 40, "https://500px.com/p/{handle}"),
    p(
        "unsplash",
        "Unsplash",
        "_",
        40,
        "https://unsplash.com/@{handle}",
    ),
    p("vsco", "VSCO", "-_", 40, "https://vsco.co/{handle}/gallery"),
    p(
        "artstation",
        "ArtStation",
        "-_",
        40,
        "https://www.artstation.com/{handle}",
    ),
    p(
        "deviantart",
        "DeviantArt",
        "-",
        20,
        "https://www.deviantart.com/{handle}",
    ),
    p(
        "behance",
        "Behance",
        "-_",
        40,
        "https://www.behance.net/{handle}",
    ),
    p(
        "dribbble",
        "Dribbble",
        "-_",
        40,
        "https://dribbble.com/{handle}",
    ),
    p(
        "redbubble",
        "Redbubble",
        "-_",
        40,
        "https://www.redbubble.com/people/{handle}/shop",
    ),
    // fitness & outdoors
    p(
        "strava",
        "Strava",
        "-_.",
        50,
        "https://www.strava.com/athletes/{handle}",
    ),
    p(
        "untappd",
        "Untappd",
        "-_",
        40,
        "https://untappd.com/user/{handle}",
    ),
    // games
    p(
        "steam",
        "Steam",
        "-_",
        32,
        "https://steamcommunity.com/id/{handle}",
    ),
    p(
        "chesscom",
        "Chess.com",
        "-_",
        25,
        "https://www.chess.com/member/{handle}",
    ),
    p(
        "lichess",
        "Lichess",
        "-_",
        20,
        "https://lichess.org/@/{handle}",
    ),
    p(
        "speedrun",
        "speedrun.com",
        "-_",
        30,
        "https://www.speedrun.com/users/{handle}",
    ),
    p("itchio", "itch.io", "-_", 40, "https://{handle}.itch.io/"),
    p(
        "namemc",
        "NameMC",
        "_",
        16,
        "https://namemc.com/profile/{handle}",
    ),
    p(
        "osu",
        "osu!",
        "-_[]",
        15,
        "https://osu.ppy.sh/users/{handle}",
    ),
    p(
        "boardgamegeek",
        "BoardGameGeek",
        "-_",
        40,
        "https://boardgamegeek.com/user/{handle}",
    ),
    // film, tv, books
    p(
        "letterboxd",
        "Letterboxd",
        "_",
        15,
        "https://letterboxd.com/{handle}/",
    ),
    p(
        "myanimelist",
        "MyAnimeList",
        "-_",
        16,
        "https://myanimelist.net/profile/{handle}",
    ),
    p(
        "anilist",
        "AniList",
        "-_",
        20,
        "https://anilist.co/user/{handle}/",
    ),
    p(
        "trakt",
        "Trakt",
        "-_.",
        40,
        "https://trakt.tv/users/{handle}",
    ),
    // shopping, payments, creators
    p(
        "etsy",
        "Etsy",
        "-_",
        40,
        "https://www.etsy.com/shop/{handle}",
    ),
    p(
        "ebay",
        "eBay",
        "-_.",
        64,
        "https://www.ebay.com/usr/{handle}",
    ),
    p(
        "poshmark",
        "Poshmark",
        "-_",
        40,
        "https://poshmark.com/closet/{handle}",
    ),
    p(
        "depop",
        "Depop",
        "-_.",
        30,
        "https://www.depop.com/{handle}/",
    ),
    p(
        "patreon",
        "Patreon",
        "-_",
        64,
        "https://www.patreon.com/{handle}",
    ),
    p("kofi", "Ko-fi", "-_", 40, "https://ko-fi.com/{handle}"),
    p(
        "buymeacoffee",
        "Buy Me a Coffee",
        "-_",
        40,
        "https://buymeacoffee.com/{handle}",
    ),
    p(
        "gumroad",
        "Gumroad",
        "-_",
        40,
        "https://{handle}.gumroad.com/",
    ),
    p(
        "venmo",
        "Venmo",
        "-_",
        30,
        "https://account.venmo.com/u/{handle}",
    ),
    p(
        "cashapp",
        "Cash App",
        "-_",
        20,
        "https://cash.app/${handle}",
    ),
    p(
        "paypal",
        "PayPal.me",
        "-_",
        20,
        "https://www.paypal.com/paypalme/{handle}",
    ),
    p(
        "fiverr",
        "Fiverr",
        "_",
        30,
        "https://www.fiverr.com/{handle}",
    ),
    // scheduling & misc
    p(
        "calendly",
        "Calendly",
        "-_",
        40,
        "https://calendly.com/{handle}",
    ),
    p("trello", "Trello", "_", 40, "https://trello.com/u/{handle}"),
    p(
        "tripadvisor",
        "Tripadvisor",
        "-_",
        40,
        "https://www.tripadvisor.com/Profile/{handle}",
    ),
    p(
        "duolingo",
        "Duolingo",
        "-_",
        40,
        "https://www.duolingo.com/profile/{handle}",
    ),
    // ---- further username-sweep sites (MODULES §6), generic handle rules
    p(
        "sourcehut",
        "sourcehut",
        "-_.",
        40,
        "https://sr.ht/~{handle}/",
    ),
    p(
        "notabug",
        "NotABug",
        "-_.",
        40,
        "https://notabug.org/{handle}",
    ),
    p("gitee", "Gitee", "-_.", 40, "https://gitee.com/{handle}"),
    p(
        "rubygems",
        "RubyGems",
        "-_.",
        40,
        "https://rubygems.org/profiles/{handle}",
    ),
    p(
        "packagist",
        "Packagist",
        "-_.",
        40,
        "https://packagist.org/users/{handle}/",
    ),
    p(
        "nuget",
        "NuGet",
        "-_.",
        40,
        "https://www.nuget.org/profiles/{handle}",
    ),
    p("hexpm", "Hex", "-_.", 40, "https://hex.pm/users/{handle}"),
    p(
        "anaconda",
        "Anaconda.org",
        "-_.",
        40,
        "https://anaconda.org/{handle}",
    ),
    p(
        "opencollective",
        "Open Collective",
        "-_.",
        40,
        "https://opencollective.com/{handle}",
    ),
    p(
        "wakatime",
        "WakaTime",
        "-_.",
        40,
        "https://wakatime.com/@{handle}",
    ),
    p(
        "observable",
        "Observable",
        "-_.",
        40,
        "https://observablehq.com/@{handle}",
    ),
    p(
        "jsfiddle",
        "JSFiddle",
        "-_.",
        40,
        "https://jsfiddle.net/user/{handle}/",
    ),
    p(
        "codesandbox",
        "CodeSandbox",
        "-_.",
        40,
        "https://codesandbox.io/u/{handle}",
    ),
    p(
        "exercism",
        "Exercism",
        "-_.",
        40,
        "https://exercism.org/profiles/{handle}",
    ),
    p(
        "atcoder",
        "AtCoder",
        "-_.",
        40,
        "https://atcoder.jp/users/{handle}",
    ),
    p(
        "codechef",
        "CodeChef",
        "-_.",
        40,
        "https://www.codechef.com/users/{handle}",
    ),
    p(
        "hackerearth",
        "HackerEarth",
        "-_.",
        40,
        "https://www.hackerearth.com/@{handle}",
    ),
    p(
        "geeksforgeeks",
        "GeeksforGeeks",
        "-_.",
        40,
        "https://www.geeksforgeeks.org/user/{handle}/",
    ),
    p(
        "topcoder",
        "Topcoder",
        "-_.",
        40,
        "https://profiles.topcoder.com/{handle}",
    ),
    p(
        "coderwall",
        "Coderwall",
        "-_.",
        40,
        "https://coderwall.com/{handle}",
    ),
    p(
        "rootme",
        "Root Me",
        "-_.",
        40,
        "https://www.root-me.org/{handle}",
    ),
    p(
        "codecademy",
        "Codecademy",
        "-_.",
        40,
        "https://www.codecademy.com/profiles/{handle}",
    ),
    p(
        "scratch",
        "Scratch",
        "-_.",
        40,
        "https://scratch.mit.edu/users/{handle}/",
    ),
    p(
        "hackster",
        "Hackster.io",
        "-_.",
        40,
        "https://www.hackster.io/{handle}",
    ),
    p(
        "frontendmentor",
        "Frontend Mentor",
        "-_.",
        40,
        "https://www.frontendmentor.io/profile/{handle}",
    ),
    p(
        "dailydev",
        "daily.dev",
        "-_.",
        40,
        "https://app.daily.dev/{handle}",
    ),
    p(
        "awwwards",
        "Awwwards",
        "-_.",
        40,
        "https://www.awwwards.com/{handle}/",
    ),
    p(
        "slidescom",
        "Slides.com",
        "-_.",
        40,
        "https://slides.com/{handle}",
    ),
    p(
        "speakerdeck",
        "Speaker Deck",
        "-_.",
        40,
        "https://speakerdeck.com/{handle}",
    ),
    p(
        "civitai",
        "Civitai",
        "-_.",
        40,
        "https://civitai.com/user/{handle}",
    ),
    p(
        "replicate",
        "Replicate",
        "-_.",
        40,
        "https://replicate.com/{handle}",
    ),
    p(
        "figma",
        "Figma Community",
        "-_.",
        40,
        "https://www.figma.com/@{handle}",
    ),
    p(
        "webflow",
        "Webflow",
        "-_.",
        40,
        "https://webflow.com/@{handle}",
    ),
    p(
        "indiehackers",
        "Indie Hackers",
        "-_.",
        40,
        "https://www.indiehackers.com/{handle}",
    ),
    p(
        "tetrio",
        "TETR.IO",
        "-_.",
        40,
        "https://ch.tetr.io/u/{handle}",
    ),
    p(
        "monkeytype",
        "Monkeytype",
        "-_.",
        40,
        "https://monkeytype.com/profile/{handle}",
    ),
    p(
        "typeracer",
        "TypeRacer",
        "-_.",
        40,
        "https://data.typeracer.com/pit/profile?user={handle}",
    ),
    p(
        "printables",
        "Printables",
        "-_.",
        40,
        "https://www.printables.com/@{handle}",
    ),
    p(
        "cults3d",
        "Cults",
        "-_.",
        40,
        "https://cults3d.com/en/users/{handle}",
    ),
    p(
        "myminifactory",
        "MyMiniFactory",
        "-_.",
        40,
        "https://www.myminifactory.com/users/{handle}",
    ),
    p(
        "sketchfab",
        "Sketchfab",
        "-_.",
        40,
        "https://sketchfab.com/{handle}",
    ),
    p(
        "newgrounds",
        "Newgrounds",
        "-_.",
        40,
        "https://{handle}.newgrounds.com/",
    ),
    p(
        "kongregate",
        "Kongregate",
        "-_.",
        40,
        "https://www.kongregate.com/accounts/{handle}",
    ),
    p(
        "psnprofiles",
        "PSNProfiles",
        "-_.",
        40,
        "https://psnprofiles.com/{handle}",
    ),
    p(
        "trueachievements",
        "TrueAchievements",
        "-_.",
        40,
        "https://www.trueachievements.com/gamer/{handle}",
    ),
    p(
        "backloggd",
        "Backloggd",
        "-_.",
        40,
        "https://www.backloggd.com/u/{handle}/",
    ),
    p(
        "retroachievements",
        "RetroAchievements",
        "-_.",
        40,
        "https://retroachievements.org/user/{handle}",
    ),
    p("gog", "GOG", "-_.", 40, "https://www.gog.com/u/{handle}"),
    p(
        "moddb",
        "ModDB",
        "-_.",
        40,
        "https://www.moddb.com/members/{handle}",
    ),
    p(
        "curseforge",
        "CurseForge",
        "-_.",
        40,
        "https://www.curseforge.com/members/{handle}",
    ),
    p(
        "modrinth",
        "Modrinth",
        "-_.",
        40,
        "https://modrinth.com/user/{handle}",
    ),
    p(
        "planetminecraft",
        "Planet Minecraft",
        "-_.",
        40,
        "https://www.planetminecraft.com/member/{handle}/",
    ),
    p(
        "plurk",
        "Plurk",
        "-_.",
        40,
        "https://www.plurk.com/{handle}",
    ),
    p("hive", "Hive", "-_.", 40, "https://hive.blog/@{handle}"),
    p(
        "steemit",
        "Steemit",
        "-_.",
        40,
        "https://steemit.com/@{handle}",
    ),
    p(
        "warpcast",
        "Warpcast",
        "-_.",
        40,
        "https://warpcast.com/{handle}",
    ),
    p(
        "slashdot",
        "Slashdot",
        "-_.",
        40,
        "https://slashdot.org/~{handle}",
    ),
    p(
        "tildes",
        "Tildes",
        "-_.",
        40,
        "https://tildes.net/user/{handle}",
    ),
    p(
        "lowendtalk",
        "LowEndTalk",
        "-_.",
        40,
        "https://lowendtalk.com/profile/{handle}",
    ),
    p("9gag", "9GAG", "-_.", 40, "https://9gag.com/u/{handle}"),
    p("giphy", "GIPHY", "-_.", 40, "https://giphy.com/{handle}"),
    p(
        "ifunny",
        "iFunny",
        "-_.",
        40,
        "https://ifunny.co/user/{handle}",
    ),
    p(
        "knowyourmeme",
        "Know Your Meme",
        "-_.",
        40,
        "https://knowyourmeme.com/users/{handle}",
    ),
    p(
        "fandom",
        "Fandom",
        "-_.",
        40,
        "https://community.fandom.com/wiki/User:{handle}",
    ),
    p(
        "wikimediacommons",
        "Wikimedia Commons",
        "-_.",
        40,
        "https://commons.wikimedia.org/wiki/User:{handle}",
    ),
    p(
        "inaturalist",
        "iNaturalist",
        "-_.",
        40,
        "https://www.inaturalist.org/people/{handle}",
    ),
    p(
        "weheartit",
        "We Heart It",
        "-_.",
        40,
        "https://weheartit.com/{handle}",
    ),
    p(
        "bitchute",
        "BitChute",
        "-_.",
        40,
        "https://www.bitchute.com/channel/{handle}/",
    ),
    p(
        "douban",
        "Douban",
        "-_.",
        40,
        "https://www.douban.com/people/{handle}/",
    ),
    p(
        "zhihu",
        "Zhihu",
        "-_.",
        40,
        "https://www.zhihu.com/people/{handle}",
    ),
    p(
        "habr",
        "Habr",
        "-_.",
        40,
        "https://habr.com/en/users/{handle}/",
    ),
    p("pikabu", "Pikabu", "-_.", 40, "https://pikabu.ru/@{handle}"),
    p(
        "tistory",
        "Tistory",
        "-_.",
        40,
        "https://{handle}.tistory.com/",
    ),
    p("notecom", "note", "-_.", 40, "https://note.com/{handle}"),
    p("qiita", "Qiita", "-_.", 40, "https://qiita.com/{handle}"),
    p("zenn", "Zenn", "-_.", 40, "https://zenn.dev/{handle}"),
    p(
        "hatena",
        "Hatena",
        "-_.",
        40,
        "https://profile.hatena.ne.jp/{handle}/",
    ),
    p(
        "reverbnation",
        "ReverbNation",
        "-_.",
        40,
        "https://www.reverbnation.com/{handle}",
    ),
    p(
        "musicbrainz",
        "MusicBrainz",
        "-_.",
        40,
        "https://musicbrainz.org/user/{handle}",
    ),
    p(
        "setlistfm",
        "setlist.fm",
        "-_.",
        40,
        "https://www.setlist.fm/user/{handle}",
    ),
    p(
        "freesound",
        "Freesound",
        "-_.",
        40,
        "https://freesound.org/people/{handle}/",
    ),
    p(
        "hearthis",
        "hearthis.at",
        "-_.",
        40,
        "https://hearthis.at/{handle}/",
    ),
    p(
        "ultimateguitar",
        "Ultimate Guitar",
        "-_.",
        40,
        "https://www.ultimate-guitar.com/u/{handle}",
    ),
    p(
        "albumoftheyear",
        "Album of the Year",
        "-_.",
        40,
        "https://www.albumoftheyear.org/user/{handle}/",
    ),
    p(
        "smugmug",
        "SmugMug",
        "-_.",
        40,
        "https://{handle}.smugmug.com/",
    ),
    p(
        "pexels",
        "Pexels",
        "-_.",
        40,
        "https://www.pexels.com/@{handle}",
    ),
    p(
        "picsart",
        "Picsart",
        "-_.",
        40,
        "https://picsart.com/u/{handle}",
    ),
    p(
        "furaffinity",
        "Fur Affinity",
        "-_.",
        40,
        "https://www.furaffinity.net/user/{handle}/",
    ),
    p(
        "weasyl",
        "Weasyl",
        "-_.",
        40,
        "https://www.weasyl.com/~{handle}",
    ),
    p(
        "toyhouse",
        "Toyhouse",
        "-_.",
        40,
        "https://toyhou.se/{handle}",
    ),
    p("cara", "Cara", "-_.", 40, "https://cara.app/{handle}"),
    p(
        "coroflot",
        "Coroflot",
        "-_.",
        40,
        "https://www.coroflot.com/{handle}",
    ),
    p("cargo", "Cargo", "-_.", 40, "https://{handle}.cargo.site/"),
    p(
        "society6",
        "Society6",
        "-_.",
        40,
        "https://society6.com/{handle}",
    ),
    p(
        "threadless",
        "Threadless",
        "-_.",
        40,
        "https://{handle}.threadless.com/",
    ),
    p(
        "zazzle",
        "Zazzle",
        "-_.",
        40,
        "https://www.zazzle.com/store/{handle}",
    ),
    p(
        "teepublic",
        "TeePublic",
        "-_.",
        40,
        "https://www.teepublic.com/user/{handle}",
    ),
    p(
        "storenvy",
        "Storenvy",
        "-_.",
        40,
        "https://{handle}.storenvy.com/",
    ),
    p(
        "bigcartel",
        "Big Cartel",
        "-_.",
        40,
        "https://{handle}.bigcartel.com/",
    ),
    p(
        "carousell",
        "Carousell",
        "-_.",
        40,
        "https://www.carousell.com/u/{handle}/",
    ),
    p(
        "opensea",
        "OpenSea",
        "-_.",
        40,
        "https://opensea.io/{handle}",
    ),
    p(
        "hubpages",
        "HubPages",
        "-_.",
        40,
        "https://hubpages.com/@{handle}",
    ),
    p(
        "vocal",
        "Vocal",
        "-_.",
        40,
        "https://vocal.media/authors/{handle}",
    ),
    p(
        "writeas",
        "Write.as",
        "-_.",
        40,
        "https://write.as/{handle}/",
    ),
    p(
        "svbtle",
        "Svbtle",
        "-_.",
        40,
        "https://{handle}.svbtle.com/",
    ),
    p(
        "buttondown",
        "Buttondown",
        "-_.",
        40,
        "https://buttondown.com/{handle}",
    ),
    p(
        "archiveorg",
        "Internet Archive",
        "-_.",
        40,
        "https://archive.org/details/@{handle}",
    ),
    p("issuu", "Issuu", "-_.", 40, "https://issuu.com/{handle}"),
    p(
        "academia",
        "Academia.edu",
        "-_.",
        40,
        "https://independent.academia.edu/{handle}",
    ),
    p(
        "quizlet",
        "Quizlet",
        "-_.",
        40,
        "https://quizlet.com/user/{handle}/sets",
    ),
    p(
        "fanfiction",
        "FanFiction.net",
        "-_.",
        40,
        "https://www.fanfiction.net/~{handle}",
    ),
    p(
        "inkitt",
        "Inkitt",
        "-_.",
        40,
        "https://www.inkitt.com/{handle}",
    ),
    p("tapas", "Tapas", "-_.", 40, "https://tapas.io/{handle}"),
    p(
        "animeplanet",
        "Anime-Planet",
        "-_.",
        40,
        "https://www.anime-planet.com/users/{handle}",
    ),
    p(
        "librarything",
        "LibraryThing",
        "-_.",
        40,
        "https://www.librarything.com/profile/{handle}",
    ),
    p(
        "storygraph",
        "The StoryGraph",
        "-_.",
        40,
        "https://app.thestorygraph.com/profile/{handle}",
    ),
    p(
        "alltrails",
        "AllTrails",
        "-_.",
        40,
        "https://www.alltrails.com/members/{handle}",
    ),
    p(
        "myfitnesspal",
        "MyFitnessPal",
        "-_.",
        40,
        "https://www.myfitnesspal.com/profile/{handle}",
    ),
    p(
        "ravelry",
        "Ravelry",
        "-_.",
        40,
        "https://www.ravelry.com/people/{handle}",
    ),
    p(
        "houzz",
        "Houzz",
        "-_.",
        40,
        "https://www.houzz.com/user/{handle}",
    ),
    p(
        "vivino",
        "Vivino",
        "-_.",
        40,
        "https://www.vivino.com/users/{handle}",
    ),
    p(
        "biolink",
        "bio.link",
        "-_.",
        40,
        "https://bio.link/{handle}",
    ),
    p(
        "beacons",
        "Beacons",
        "-_.",
        40,
        "https://beacons.ai/{handle}",
    ),
    p("soloto", "solo.to", "-_.", 40, "https://solo.to/{handle}"),
    p(
        "taplink",
        "Taplink",
        "-_.",
        40,
        "https://taplink.cc/{handle}",
    ),
    p("lnkbio", "Lnk.Bio", "-_.", 40, "https://lnk.bio/{handle}"),
    p(
        "allmylinks",
        "AllMyLinks",
        "-_.",
        40,
        "https://allmylinks.com/{handle}",
    ),
    p("contra", "Contra", "-_.", 40, "https://contra.com/{handle}"),
    p(
        "peerlist",
        "Peerlist",
        "-_.",
        40,
        "https://peerlist.io/{handle}",
    ),
    p(
        "wellfound",
        "Wellfound",
        "-_.",
        40,
        "https://wellfound.com/u/{handle}",
    ),
    p("f6s", "F6S", "-_.", 40, "https://www.f6s.com/{handle}"),
    p(
        "freelancer",
        "Freelancer",
        "-_.",
        40,
        "https://www.freelancer.com/u/{handle}",
    ),
    p(
        "guru",
        "Guru",
        "-_.",
        40,
        "https://www.guru.com/freelancers/{handle}",
    ),
    p("calcom", "Cal.com", "-_.", 40, "https://cal.com/{handle}"),
    p(
        "discuss.python.org",
        "Python Discourse",
        "-_.",
        40,
        "https://discuss.python.org/u/{handle}",
    ),
    p(
        "users.rust-lang.org",
        "Rust Users Forum",
        "-_.",
        40,
        "https://users.rust-lang.org/u/{handle}",
    ),
    p(
        "internals.rust-lang.org",
        "Rust Internals",
        "-_.",
        40,
        "https://internals.rust-lang.org/u/{handle}",
    ),
    p(
        "community.cloudflare.com",
        "Cloudflare Community",
        "-_.",
        40,
        "https://community.cloudflare.com/u/{handle}",
    ),
    p(
        "forum.obsidian.md",
        "Obsidian Forum",
        "-_.",
        40,
        "https://forum.obsidian.md/u/{handle}",
    ),
    p(
        "community.home-assistant.io",
        "Home Assistant Community",
        "-_.",
        40,
        "https://community.home-assistant.io/u/{handle}",
    ),
    p(
        "forum.manjaro.org",
        "Manjaro Forum",
        "-_.",
        40,
        "https://forum.manjaro.org/u/{handle}",
    ),
    p(
        "discuss.kotlinlang.org",
        "Kotlin Discussions",
        "-_.",
        40,
        "https://discuss.kotlinlang.org/u/{handle}",
    ),
    p(
        "community.openai.com",
        "OpenAI Community",
        "-_.",
        40,
        "https://community.openai.com/u/{handle}",
    ),
    p(
        "discuss.pytorch.org",
        "PyTorch Forums",
        "-_.",
        40,
        "https://discuss.pytorch.org/u/{handle}",
    ),
    p(
        "forum.godotengine.org",
        "Godot Forum",
        "-_.",
        40,
        "https://forum.godotengine.org/u/{handle}",
    ),
    p(
        "devforum.roblox.com",
        "Roblox DevForum",
        "-_.",
        40,
        "https://devforum.roblox.com/u/{handle}",
    ),
    p(
        "meta.discourse.org",
        "Discourse Meta",
        "-_.",
        40,
        "https://meta.discourse.org/u/{handle}",
    ),
    p(
        "forums.swift.org",
        "Swift Forums",
        "-_.",
        40,
        "https://forums.swift.org/u/{handle}",
    ),
    p(
        "discuss.hashicorp.com",
        "HashiCorp Discuss",
        "-_.",
        40,
        "https://discuss.hashicorp.com/u/{handle}",
    ),
    p(
        "discuss.elastic.co",
        "Elastic Discuss",
        "-_.",
        40,
        "https://discuss.elastic.co/u/{handle}",
    ),
    p(
        "forum.arduino.cc",
        "Arduino Forum",
        "-_.",
        40,
        "https://forum.arduino.cc/u/{handle}",
    ),
    p(
        "forum.syncthing.net",
        "Syncthing Forum",
        "-_.",
        40,
        "https://forum.syncthing.net/u/{handle}",
    ),
    p(
        "forum.level1techs.com",
        "Level1Techs Forum",
        "-_.",
        40,
        "https://forum.level1techs.com/u/{handle}",
    ),
    p(
        "community.letsencrypt.org",
        "Let's Encrypt Community",
        "-_.",
        40,
        "https://community.letsencrypt.org/u/{handle}",
    ),
    p(
        "forum.freecodecamp.org",
        "freeCodeCamp Forum",
        "-_.",
        40,
        "https://forum.freecodecamp.org/u/{handle}",
    ),
    p(
        "discourse.nixos.org",
        "NixOS Discourse",
        "-_.",
        40,
        "https://discourse.nixos.org/u/{handle}",
    ),
    p(
        "forum.cursor.com",
        "Cursor Forum",
        "-_.",
        40,
        "https://forum.cursor.com/u/{handle}",
    ),
    p(
        "discuss.ray.io",
        "Ray Discuss",
        "-_.",
        40,
        "https://discuss.ray.io/u/{handle}",
    ),
    p(
        "community.grafana.com",
        "Grafana Community",
        "-_.",
        40,
        "https://community.grafana.com/u/{handle}",
    ),
    p(
        "forum.babylonjs.com",
        "Babylon.js Forum",
        "-_.",
        40,
        "https://forum.babylonjs.com/u/{handle}",
    ),
    p(
        "discourse.julialang.org",
        "Julia Discourse",
        "-_.",
        40,
        "https://discourse.julialang.org/u/{handle}",
    ),
    p(
        "forum.exercism.org",
        "Exercism Forum",
        "-_.",
        40,
        "https://forum.exercism.org/u/{handle}",
    ),
    p(
        "community.fly.io",
        "Fly.io Community",
        "-_.",
        40,
        "https://community.fly.io/u/{handle}",
    ),
    p(
        "forum.rclone.org",
        "rclone Forum",
        "-_.",
        40,
        "https://forum.rclone.org/u/{handle}",
    ),
    p(
        "discourse.ubuntu.com",
        "Ubuntu Discourse",
        "-_.",
        40,
        "https://discourse.ubuntu.com/u/{handle}",
    ),
    p(
        "discussion.fedoraproject.org",
        "Fedora Discussion",
        "-_.",
        40,
        "https://discussion.fedoraproject.org/u/{handle}",
    ),
    p(
        "forum.endeavouros.com",
        "EndeavourOS Forum",
        "-_.",
        40,
        "https://forum.endeavouros.com/u/{handle}",
    ),
    p(
        "forum.tailscale.com",
        "Tailscale Forum",
        "-_.",
        40,
        "https://forum.tailscale.com/u/{handle}",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_unique_and_valid() {
        let mut seen = std::collections::BTreeSet::new();
        for d in BUILTIN {
            assert!(valid_platform_id(d.id), "{}", d.id);
            assert!(seen.insert(d.id), "duplicate platform {}", d.id);
            if !d.url.is_empty() {
                assert!(d.url.starts_with("https://"), "{}", d.id);
            }
        }
        assert!(BUILTIN.len() >= 290);
    }

    #[test]
    fn github_rules() {
        let r = PlatformRegistry::builtin();
        let gh = r.get("github").unwrap();
        assert_eq!(
            gh.normalize_handle(" @JMarlow-WAV ").unwrap(),
            "jmarlow-wav"
        );
        assert!(gh.normalize_handle("jm marlow").is_err());
        assert!(gh.normalize_handle("").is_err());
        assert_eq!(
            gh.profile_url("jmarlow-wav").unwrap(),
            "https://github.com/jmarlow-wav"
        );
    }

    #[test]
    fn case_sensitive_platform_keeps_case() {
        let r = PlatformRegistry::builtin();
        let sp = r.get("spotify").unwrap();
        assert_eq!(sp.normalize_handle("JMarlow").unwrap(), "JMarlow");
    }

    #[test]
    fn mastodon_user_at_instance() {
        let r = PlatformRegistry::builtin();
        let m = r.get("mastodon").unwrap();
        assert_eq!(
            m.normalize_handle("@JMarlow@Mastodon.Social").unwrap(),
            "jmarlow@mastodon.social"
        );
        assert!(m.normalize_handle("jmarlow").is_err());
        assert_eq!(
            m.profile_url("jmarlow@mastodon.social").unwrap(),
            "https://mastodon.social/@jmarlow"
        );
    }

    #[test]
    fn declared_platforms() {
        let mut r = PlatformRegistry::builtin();
        assert!(matches!(
            r.declare("mastodon", false, None),
            Err(DeclareError::Builtin(_))
        ));
        r.declare("lemmy", false, Some("https://{instance}/@{user}"))
            .unwrap();
        let l = r.get("lemmy").unwrap();
        assert_eq!(
            l.normalize_handle("JMarlow@lemmy.example").unwrap(),
            "jmarlow@lemmy.example"
        );
        assert!(r.declare("Bad Id", false, None).is_err());
    }

    #[test]
    fn fallback() {
        assert_eq!(
            fallback_handle("somesite", " @JMarlow ").unwrap(),
            "jmarlow"
        );
        assert!(fallback_handle("Some Site", "x").is_err());
        assert!(fallback_handle("somesite", "a b").is_err());
    }
}
