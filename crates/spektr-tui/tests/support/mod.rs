//! A fictional identity case shaped like artboard 08, built directly as view
//! models (no engine), for snapshot tests.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use jiff::Timestamp;
use spektr_engine::{
    CaseStatus, CostClass, Degree, Direction, EgoGroup, EgoRow, EgoView, EntityDetail,
    EntityRowView, EntitySummary, FindingView, GroupKey, ModuleInfo, ObservationView, Runtime,
    Technique,
};
use spektr_model::{
    ArtifactKind, Attr, AttrValue, CorePredicate, CoreType, EdgeId, EntityFlags, EntityType, IdGen,
    ObservationId, Predicate, Strength,
};
use spektr_tui::host::{Chain, DoctorLine, Level, RecentCase, Settings};
use spektr_tui::state::{AppState, Launcher, Pane, Screen, Workspace};
use spektr_tui::theme::ColorMode;

pub const NOW: &str = "2026-10-20T18:14:22Z";

pub fn now() -> Timestamp {
    NOW.parse().unwrap()
}

pub struct Ids(IdGen);

impl Ids {
    pub fn new() -> Self {
        Self(IdGen::seeded(1))
    }

    pub fn ent(&self, ty: CoreType, value: &str, flags: EntityFlags, depth: u32) -> EntitySummary {
        EntitySummary {
            id: self.0.entity(now()),
            ty: EntityType::Core(ty),
            key: value.to_string(),
            value: value.to_string(),
            flags,
            depth,
        }
    }

    pub fn edge(&self) -> EdgeId {
        EdgeId(self.0.next(now()))
    }

    pub fn obs(&self) -> ObservationId {
        ObservationId(self.0.next(now()))
    }
}

fn row(ids: &Ids, e: &EntitySummary, s: Option<Strength>) -> EgoRow {
    EgoRow {
        edges: vec![ids.edge()],
        entity: e.clone(),
        strength: s,
        confirmed_at: None,
        rejected: false,
        sources: 1,
        artifact_value: None,
    }
}

fn group(
    p: CorePredicate,
    d: Direction,
    artifact: Option<ArtifactKind>,
    total: usize,
    pages: usize,
    rows: Vec<EgoRow>,
) -> EgoGroup {
    EgoGroup {
        key: GroupKey {
            predicate: Predicate::Core(p),
            direction: d,
            artifact,
        },
        total,
        page: 0,
        pages,
        rows,
    }
}

pub fn settings(mode: ColorMode) -> Settings {
    Settings {
        mouse: true,
        colors: mode,
        version: "0.1.0".into(),
        home: Some("/home/jm".into()),
        ..Settings::default()
    }
}

pub fn module(
    name: &str,
    consumes: &[&str],
    produces: &[(&str, &str)],
    cost: CostClass,
    blocked: Option<&str>,
) -> ModuleInfo {
    ModuleInfo {
        name: name.into(),
        version: "1.0.0".into(),
        description: format!("{name} lookups."),
        runtime: Runtime::Native,
        technique: Technique::Passive,
        auto: true,
        consumes: consumes.iter().map(|s| s.to_string()).collect(),
        produces: produces
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        cost,
        daily_budget: None,
        missing_secrets: Vec::new(),
        hosts: Vec::new(),
        blocked: blocked.map(str::to_string),
    }
}

/// The identity workspace: subject, seed, accounts, variants, a correlated
/// avatar group, an inspector on the github account.
pub fn workspace(mode: ColorMode, size: (u16, u16)) -> AppState {
    let ids = Ids::new();
    let subject = ids.ent(CoreType::Person, "Jordan Marlow", EntityFlags::SUBJECT, 0);
    let seed = ids.ent(CoreType::Username, "jmarlow.wav", EntityFlags::SEED, 0);
    let v1 = ids.ent(CoreType::Username, "jordanmarlow", EntityFlags::NONE, 1);
    let v2 = ids.ent(CoreType::Username, "jm4rlow", EntityFlags::NONE, 1);
    let accts: Vec<EntitySummary> = [
        "github:jmarlow-wav",
        "bluesky:jmarlow.wav",
        "soundcloud:jmarlow",
        "reddit:jmarlow_wav",
        "strava:jmarlow",
        "lastfm:jmarlow",
        "keybase:jmarlow",
    ]
    .iter()
    .map(|v| ids.ent(CoreType::Account, v, EntityFlags::NONE, 1))
    .collect();
    let email1 = ids.ent(
        CoreType::Email,
        "jordan.marlow@mail.example",
        EntityFlags::NONE,
        2,
    );
    let email2 = ids.ent(
        CoreType::Email,
        "jm4rlow@mail.example",
        EntityFlags::NONE,
        2,
    );
    let url = ids.ent(
        CoreType::Url,
        "https://jmarlow.example",
        EntityFlags::NONE,
        2,
    );
    let hub = ids.ent(CoreType::Domain, "mail.example", EntityFlags::HUB, 3);

    let mut entities = vec![subject.clone(), seed.clone(), v1.clone(), v2.clone()];
    entities.extend(accts.iter().cloned());
    entities.extend([email1, email2.clone(), url, hub]);
    spektr_tui::entities::sort(&mut entities);

    let ego = EgoView {
        focus: seed.clone(),
        degree: Degree {
            inbound: 1,
            outbound: 7,
            symmetric: 5,
            total: 13,
        },
        observations: 4,
        groups: vec![
            group(
                CorePredicate::HasAccount,
                Direction::Out,
                None,
                7,
                2,
                vec![
                    row(&ids, &accts[0], Some(Strength::Confirmed)),
                    row(&ids, &accts[1], Some(Strength::Corroborated)),
                    row(&ids, &accts[2], Some(Strength::Corroborated)),
                    row(&ids, &accts[3], Some(Strength::Single)),
                    row(&ids, &accts[4], Some(Strength::Single)),
                ],
            ),
            group(
                CorePredicate::UsesHandle,
                Direction::In,
                None,
                1,
                1,
                vec![row(&ids, &subject, Some(Strength::Corroborated))],
            ),
            group(
                CorePredicate::VariantOf,
                Direction::Sym,
                None,
                2,
                1,
                vec![
                    row(&ids, &v1, Some(Strength::Single)),
                    row(&ids, &v2, Some(Strength::Single)),
                ],
            ),
            group(
                CorePredicate::SharesArtifact,
                Direction::Sym,
                Some(ArtifactKind::AvatarPhash),
                3,
                1,
                vec![
                    row(&ids, &accts[0], Some(Strength::Single)),
                    row(&ids, &accts[1], Some(Strength::Single)),
                    row(&ids, &accts[2], Some(Strength::Single)),
                ],
            ),
        ],
    };

    let gh = &accts[0];
    let (o1, o2, o3) = (ids.obs(), ids.obs(), ids.obs());
    let mut attrs: spektr_model::Attrs = BTreeMap::new();
    let a = |v, o, event| {
        vec![Attr {
            value: v,
            observation: o,
            event,
        }]
    };
    attrs.insert(
        "display_name".into(),
        a(AttrValue::Text("Jordan M.".into()), o2, false),
    );
    attrs.insert(
        "joined".into(),
        a(AttrValue::Date("2019-03-01".parse().unwrap()), o2, false),
    );
    attrs.insert(
        "last_active".into(),
        a(
            AttrValue::Time("2026-10-16T09:12:00Z".parse().unwrap()),
            o2,
            true,
        ),
    );
    attrs.insert(
        "bio".into(),
        a(AttrValue::Text("synths, trails, coffee".into()), o2, false),
    );
    attrs.insert(
        "links".into(),
        a(AttrValue::Url("https://jmarlow.example".into()), o2, false),
    );
    let ov = |id, module: &str, ver: &str, at: &str, blob: &str| ObservationView {
        id,
        module: module.into(),
        module_version: ver.into(),
        kind: "http".into(),
        source_url: None,
        content_type: None,
        blob: Some(blob.into()),
        created_at: at.parse().unwrap(),
    };
    let detail = EntityDetail {
        entity: EntityRowView {
            summary: gh.clone(),
            attrs,
            created_at: now(),
            created_by: "github".into(),
        },
        strength: Some(Strength::Confirmed),
        degree: Degree {
            inbound: 2,
            outbound: 2,
            symmetric: 2,
            total: 6,
        },
        observations: vec![
            ov(
                o1,
                "capture",
                "html+png",
                "2026-10-20T18:11:00Z",
                "9a3f00000000000000000000000000000000000000000000000000000000ab11",
            ),
            ov(
                o2,
                "github",
                "0.4.0",
                "2026-10-20T18:06:00Z",
                "77d000000000000000000000000000000000000000000000000000000000c4c4",
            ),
            ov(
                o3,
                "username-sweep",
                "1.0.0",
                "2026-10-20T18:05:00Z",
                "2c7100000000000000000000000000000000000000000000000000000000aeae",
            ),
        ],
        checklist: Vec::new(),
        launchers: Vec::new(),
        image: None,
        findings: vec![FindingView {
            id: 3,
            title: "Advancing the Timeline".into(),
            category: "timeline".into(),
            status: "draft".into(),
        }],
        notes: Vec::new(),
    };

    let mut s = AppState::new(settings(mode), now());
    s.size = size;
    let mut w = Workspace::new("/home/jm/cases/demo-missing-01.spektr".into());
    w.chain = Chain::Ok { rows: 206 };
    w.status = Some(CaseStatus {
        name: "demo-missing-01".into(),
        path: "/home/jm/cases/demo-missing-01.spektr".into(),
        profile: "tracelabs".into(),
        profile_summary: "passive only · no probing".into(),
        depth: 2,
        region: "US".into(),
        last_seen: Some("2026-09-28".into()),
        subject: Some(subject.clone()),
        entities: entities.len() as u64,
        edges: 20,
        observations: 14,
        audit_rows: 206,
        findings: 4,
        review_pending: 5,
        checklist_open: 0,
        jobs: BTreeMap::from([
            ("running".into(), 1),
            ("queued".into(), 2),
            ("done".into(), 23),
        ]),
    });
    w.running = 1;
    w.queued = 2;
    w.done = 23;
    w.modules = vec![
        module(
            "github",
            &["username", "account"],
            &[("account", "has_account")],
            CostClass::Free,
            None,
        ),
        module(
            "username-sweep",
            &["username"],
            &[("account", "has_account")],
            CostClass::Free,
            None,
        ),
        module(
            "email-account-probe",
            &["email"],
            &[("account", "has_account")],
            CostClass::Free,
            Some("tracelabs forbids probing"),
        ),
    ];
    w.entities = entities;
    w.fresh.insert(v2.id);
    w.fresh.insert(email2.id);
    w.focus = Some(seed.id);
    w.crumbs = vec![subject, seed];
    w.ego = Some(ego);
    w.ego_col = spektr_tui::ego::Col::Right;
    w.ego_idx = 0;
    w.inspect = Some(gh.id);
    w.detail = Some(detail);
    w.pane = Pane::Graph;
    let rows = [
        ("18:12:40", "exif", "▨ IMG_2214.jpg  → ⌖ 45.52, -122.68"),
        ("18:11:02", "capture", "github.com/jmarlow-wav · html + png"),
        ("18:06:12", "github", "+1 @ email  +1 ↗ url"),
        (
            "18:05:30",
            "username-sweep",
            "300 sites · 7 found · 5 unsure",
        ),
    ];
    for (at, m, text) in rows {
        w.activity.push_back(spektr_tui::state::ActRow {
            at: format!("2026-10-20T{at}Z").parse().unwrap(),
            marker: spektr_tui::glyph::Marker::Ok,
            token: spektr_tui::theme::Token::Ok,
            module: m.into(),
            module_token: spektr_tui::theme::Token::Fg,
            spans: vec![(text.into(), spektr_tui::theme::Token::Fg)],
        });
    }
    spektr_tui::update::rebuild_entities(&mut w);
    if let Some(i) = w
        .ent_items
        .iter()
        .position(|it| matches!(it, spektr_tui::state::EntItem::Row(i) if w.entities[*i].id == w.focus.unwrap()))
    {
        w.ent_cursor = i;
    }
    s.screen = Screen::Workspace(Box::new(w));
    s
}

pub fn launcher(size: (u16, u16)) -> AppState {
    let mut s = AppState::new(settings(ColorMode::TrueColor), now());
    s.size = size;
    let t = |h: i64| {
        now()
            .checked_sub(jiff::SignedDuration::from_hours(h))
            .unwrap()
    };
    let rc = |name: &str, e, ed, opened, chain| RecentCase {
        path: format!("/home/jm/cases/{name}.spektr").into(),
        name: name.into(),
        entities: Some(e),
        edges: Some(ed),
        last_opened: Some(opened),
        chain,
        size: Some(1_887_437),
        seeds: vec![(
            EntityType::Core(CoreType::Email),
            "ops@kestrel.example".into(),
        )],
    };
    s.screen = Screen::Launcher(Launcher {
        recent: Some(vec![
            rc(
                "tl-ctf-1019",
                92,
                214,
                now()
                    .checked_sub(jiff::SignedDuration::from_mins(2))
                    .unwrap(),
                Chain::Ok { rows: 418 },
            ),
            rc("kestrel-recon", 61, 133, t(24), Chain::Ok { rows: 300 }),
            rc(
                "tl-ctf-0927",
                318,
                901,
                t(24 * 23),
                Chain::Ok { rows: 1200 },
            ),
            rc(
                "scratch",
                4,
                3,
                t(24 * 38),
                Chain::Broken { at: "#77".into() },
            ),
        ]),
        cursor: 0,
        doctor: Some(vec![
            DoctorLine {
                name: "config".into(),
                level: Level::Ok,
                detail: "~/.config/spektr/config.toml".into(),
            },
            DoctorLine {
                name: "resolver".into(),
                level: Level::Ok,
                detail: "9.9.9.9 · 1.1.1.1".into(),
            },
            DoctorLine {
                name: "hibp key".into(),
                level: Level::Ok,
                detail: "set · 38/100 today".into(),
            },
            DoctorLine {
                name: "subfinder".into(),
                level: Level::Warn,
                detail: "binary not found · module off".into(),
            },
            DoctorLine {
                name: "python3".into(),
                level: Level::Ok,
                detail: "/usr/bin/python3".into(),
            },
        ]),
        error: None,
    });
    s
}

fn find(w: &Workspace, value: &str) -> EntitySummary {
    w.entities
        .iter()
        .find(|e| e.value == value)
        .cloned()
        .unwrap()
}

fn info(title: &str, kind: &str, detail: &str, on: &str) -> spektr_engine::StepInfo {
    spektr_engine::StepInfo {
        title: title.into(),
        kind: kind.into(),
        detail: detail.into(),
        on: on.into(),
    }
}

fn stepv(
    n: u32,
    i: spektr_engine::StepInfo,
    state: &str,
    selected: u32,
    result: serde_json::Value,
) -> spektr_engine::StepView {
    spektr_engine::StepView {
        step: n,
        info: i,
        state: state.into(),
        selected,
        capped: 0,
        result,
    }
}

fn pb(name: &str, seed: &str, source: &str, profile: &str) -> spektr_engine::PlaybookInfo {
    spektr_engine::PlaybookInfo {
        name: name.into(),
        version: "1.0.0".into(),
        description: format!("{name}."),
        seed: vec![seed.into()],
        requires_profile: profile.into(),
        source: source.into(),
        steps: Vec::new(),
        errors: Vec::new(),
        warnings: Vec::new(),
    }
}

fn lv(id: &str, name: &str) -> spektr_engine::LauncherView {
    spektr_engine::LauncherView {
        id: id.into(),
        name: name.into(),
        url: Some(format!("https://search.example/{id}")),
        disabled: None,
    }
}

/// Playbook runs, the checklist, review items, the timeline, a path and an
/// avatar edge comparison, on top of `workspace` (artboards 05, 08, 10).
pub fn playbook_screens(s: &mut AppState) {
    use serde_json::json;
    use spektr_model::{ChecklistItemId, PlaybookRunId, ReviewItemId};
    let ids = Ids(IdGen::seeded(7));
    let at = |t: &str| -> Timestamp { format!("2026-10-20T{t}Z").parse().unwrap() };
    let Screen::Workspace(w) = &mut s.screen else {
        panic!()
    };
    let seed = find(w, "jmarlow.wav");
    let email = find(w, "jordan.marlow@mail.example");
    let subject = find(w, "Jordan Marlow");
    let gh = find(w, "github:jmarlow-wav");
    let bsky = find(w, "bluesky:jmarlow.wav");
    let img = |v: &str| {
        let mut e = ids.ent(CoreType::Image, v, EntityFlags::NONE, 2);
        e.key = format!("sha256:{:0>64}", v.len());
        e
    };
    let (a1, a2, a3, photo) = (
        img("avatar-github.png"),
        img("avatar-bluesky.png"),
        img("avatar-soundcloud.png"),
        img("IMG_2214.jpg"),
    );
    let (r1, r2, r3) = (
        PlaybookRunId(ids.0.next(now())),
        PlaybookRunId(ids.0.next(now())),
        PlaybookRunId(ids.0.next(now())),
    );
    let run = |id,
               playbook: &str,
               seed: &EntitySummary,
               state: &str,
               cur,
               total,
               started: &str,
               steps| spektr_engine::RunView {
        id,
        playbook: playbook.into(),
        seed: Some(seed.clone()),
        parent: None,
        state: state.into(),
        current_step: cur,
        total_steps: total,
        started_at: at(started),
        finished_at: None,
        steps,
    };
    let steps = vec![
        stepv(
            1,
            info("Sweep the handle", "run", "run username-sweep", "seed"),
            "done",
            1,
            json!({"jobs": {"done": 1}}),
        ),
        stepv(
            2,
            info(
                "Profile details",
                "run",
                "run github bluesky gitlab mastodon hackernews",
                "seed",
            ),
            "done",
            1,
            json!({"jobs": {"done": 3, "cached": 2}}),
        ),
        stepv(
            3,
            info(
                "Avatar hashes",
                "run",
                "run avatar-hash",
                "account from run",
            ),
            "done",
            9,
            json!({"jobs": {"done": 9}}),
        ),
        stepv(
            4,
            info(
                "Linked sites",
                "run",
                "run http-fingerprint wayback",
                "url from run, max 25",
            ),
            "running",
            4,
            json!({"jobs": {"done": 2, "running": 2}}),
        ),
        stepv(
            5,
            info(
                "Reverse image search",
                "check",
                "Reverse image search the avatar.",
                "image from run",
            ),
            "pending",
            3,
            json!({}),
        ),
        stepv(
            6,
            info(
                "Review unsure sweep hits",
                "check",
                "Review the unsure hits.",
                "—",
            ),
            "pending",
            0,
            json!({}),
        ),
    ];
    w.checklist.runs = vec![
        run(
            r1,
            "person-from-username",
            &seed,
            "running",
            4,
            6,
            "18:20:00",
            steps,
        ),
        run(
            r2,
            "person-from-email",
            &email,
            "done",
            5,
            5,
            "18:08:00",
            Vec::new(),
        ),
        run(
            r3,
            "image-triage",
            &photo,
            "done",
            3,
            3,
            "18:02:00",
            Vec::new(),
        ),
    ];
    w.checklist.playbooks = vec![
        pb("person-from-username", "username", "built-in", ""),
        pb("person-from-email", "email", "built-in", ""),
        pb("person-from-phone", "phone", "built-in", ""),
        pb("image-triage", "image", "built-in", ""),
        pb("domain-recon", "domain", "built-in", ""),
        pb("tracelabs-kickoff", "person", "built-in", "tracelabs"),
        pb(
            "forum-sweep",
            "username",
            "/home/jm/.config/spektr/playbooks/forum-sweep.toml",
            "",
        ),
    ];
    let item = |run,
                playbook: &str,
                step,
                title: &str,
                e: Option<&EntitySummary>,
                text: &str,
                state: &str,
                note: Option<&str>,
                launchers: Vec<spektr_engine::LauncherView>,
                screen: Option<&str>| spektr_engine::ItemView {
        id: ChecklistItemId(ids.0.next(now())),
        run,
        playbook: playbook.into(),
        step,
        step_title: title.into(),
        entity: e.cloned(),
        text: text.into(),
        launchers,
        screen: screen.map(str::to_string),
        state: state.into(),
        note: note.map(str::to_string),
        resolved_at: (state != "open").then(|| at("18:22:00")),
        local_image: None,
        label: e.filter(|e| e.value.starts_with("avatar-")).map(|e| {
            let site = e
                .value
                .trim_start_matches("avatar-")
                .trim_end_matches(".png");
            format!("avatar · {site} · jmarlow")
        }),
    };
    let rev = || {
        vec![
            lv("lens", "Google Lens"),
            lv("yandex-images", "Yandex Images"),
            lv("tineye", "TinEye"),
        ]
    };
    let text = "Reverse image search the GitHub avatar for jmarlow-wav. Look for the same photo on other profiles, shops, forums or news.";
    w.checklist.items = vec![
        item(
            r1,
            "person-from-username",
            5,
            "Reverse image search",
            Some(&a1),
            text,
            "open",
            None,
            rev(),
            None,
        ),
        item(
            r1,
            "person-from-username",
            5,
            "Reverse image search",
            Some(&a2),
            text,
            "open",
            None,
            rev(),
            None,
        ),
        item(
            r1,
            "person-from-username",
            5,
            "Reverse image search",
            Some(&a3),
            text,
            "done",
            Some("same photo on a forum"),
            rev(),
            None,
        ),
        item(
            r1,
            "person-from-username",
            6,
            "Review unsure sweep hits",
            Some(&seed),
            "Five sites didn't answer clearly.",
            "open",
            None,
            Vec::new(),
            Some("review"),
        ),
        item(
            r2,
            "person-from-email",
            4,
            "Search the exact address",
            Some(&email),
            "Search the exact address in quotes.",
            "done",
            None,
            vec![lv("google-exact", "Google")],
            None,
        ),
        item(
            r3,
            "image-triage",
            3,
            "Geolocate if no GPS",
            Some(&photo),
            "Geolocate the photo by hand.",
            "skipped",
            Some("has GPS"),
            Vec::new(),
            None,
        ),
    ];
    w.checklist.loaded = true;
    if let Some(st) = &mut w.status {
        st.checklist_open = 3;
    }
    // review
    let rv = |kind: &str, payload: serde_json::Value| spektr_engine::ReviewView {
        id: ReviewItemId(ids.0.next(now())),
        kind: kind.into(),
        state: "pending".into(),
        payload,
    };
    let sweep = |site: &str, name: &str, platform: &str, reason: &str| {
        rv(
            "sweep_unsure",
            json!({
                "module": "username-sweep", "input": seed.id, "evidence": ["o1"],
                "detail": {"site": site, "name": name, "username": "jmarlow.wav", "platform": platform,
                           "handle": "jmarlow.wav", "url": format!("https://{site}.example/u/jmarlow.wav"), "reason": reason},
            }),
        )
    };
    w.review.items = vec![
        sweep("forum", "Example Forum", "forum", "login wall"),
        sweep("photos", "Photo Site", "photos", "HTTP 403"),
        rv(
            "same_person",
            json!({"a": subject.id, "b": gh.id, "via": seed.id, "a_value": "Jordan Marlow", "b_value": "J. Marlow", "via_value": "jmarlow.wav", "via_type": "username"}),
        ),
    ];
    w.review.loaded = true;
    // timeline
    let te = |t: &str, e: &EntitySummary, attr: &str, module: &str| spektr_engine::TimelineEvent {
        at: format!("2026-{t}:00Z").parse().unwrap(),
        entity: e.clone(),
        attr: attr.into(),
        value: String::new(),
        observation: ids.obs(),
        module: module.into(),
    };
    w.timeline.events = Some(vec![
        te("03-01T10:00", &gh, "joined", "github"),
        te("08-14T21:40", &bsky, "last_post", "bluesky"),
        te("09-27T19:05", &photo, "taken_at", "exif"),
        te("10-16T09:12", &gh, "last_active", "github"),
    ]);
    // path
    w.path_tab.from = Some(seed.clone());
    w.path_tab.target = Some(email.clone());
    w.path_tab.view = Some(Some(spektr_engine::PathView {
        steps: vec![
            spektr_engine::PathStep {
                entity: seed.clone(),
                via: None,
            },
            spektr_engine::PathStep {
                entity: gh.clone(),
                via: Some((
                    ids.edge(),
                    Predicate::Core(CorePredicate::HasAccount),
                    Direction::Out,
                    Some(Strength::Confirmed),
                )),
            },
            spektr_engine::PathStep {
                entity: email.clone(),
                via: Some((
                    ids.edge(),
                    Predicate::Core(CorePredicate::HasEmail),
                    Direction::Out,
                    Some(Strength::Corroborated),
                )),
            },
        ],
    }));
    // the avatar edge
    let ev = ObservationView {
        id: ids.obs(),
        module: "avatar_phash".into(),
        module_version: "0.1.0".into(),
        kind: "correlator".into(),
        source_url: None,
        content_type: None,
        blob: None,
        created_at: at("18:07:00"),
    };
    w.edge = Some(spektr_engine::EdgeDetail {
        edge: ids.edge(),
        a: gh.clone(),
        b: bsky.clone(),
        predicate: "shares_artifact".into(),
        artifact: Some(ArtifactKind::AvatarPhash),
        value: Some("00ff~00fe".into()),
        distance: Some(3),
        a_image: Some(a1.key.trim_start_matches("sha256:").into()),
        b_image: Some(a2.key.trim_start_matches("sha256:").into()),
        strength: Some(Strength::Single),
        state: "active".into(),
        confirmed_at: None,
        evidence: vec![ev],
        a_sources: Vec::new(),
        b_sources: Vec::new(),
    });
}
