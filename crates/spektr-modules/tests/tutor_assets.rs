//! Binary fixtures for the tutor handle, generated here so they're
//! reproducible and their properties are checked: three avatars that are the
//! same picture (dHash within 6, so `avatar_phash` correlates them), one that
//! isn't, and the tutor photo with EXIF device, time and GPS. All of it is
//! drawn from code: fictional, with no real people or places photographed.
//!
//! `SPEKTR_REGEN_FIXTURES=1 cargo nextest run -p spektr-modules --test
//! tutor_assets` rewrites the files; otherwise the committed copies must
//! match.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;
use std::path::PathBuf;

use base64::Engine as _;
use image::{ImageFormat, Rgb, RgbImage};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// A head-and-shoulders silhouette on a warm gradient: the "photo".
fn portrait(w: u32, h: u32, shift: i32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let mut c = [
            (180.0 + 60.0 * fx) as i32,
            (120.0 + 70.0 * fy) as i32,
            (90.0 + 40.0 * (1.0 - fx)) as i32,
        ];
        let head = ((fx - 0.5).powi(2) + (fy - 0.38).powi(2)).sqrt() < 0.18;
        let shoulders = fy > 0.62 && ((fx - 0.5).abs() < 0.42 - (fy - 0.62) * 0.2);
        if head || shoulders {
            c = [48, 52, 70];
        }
        Rgb(c.map(|v| (v + shift).clamp(0, 255) as u8))
    })
}

/// Diagonal stripes: someone else's avatar.
fn stripes(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        if ((x + 2 * y) / 9) % 2 == 0 {
            Rgb([30, 140, 200])
        } else {
            Rgb([235, 230, 210])
        }
    })
}

fn png(img: &RgbImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

// ---- a minimal EXIF (TIFF, little-endian) builder

enum Val {
    Ascii(&'static str),
    Long(u32),
    Bytes(Vec<u8>),
    Rationals(Vec<(u32, u32)>),
}

fn ifd(entries: &[(u16, Val)], at: u32, next: u32) -> (Vec<u8>, Vec<u8>) {
    // Returns (ifd bytes, data bytes); data follows the IFD.
    let n = entries.len() as u32;
    let data_start = at + 2 + n * 12 + 4;
    let mut head = Vec::new();
    let mut data = Vec::new();
    head.extend((n as u16).to_le_bytes());
    for (tag, v) in entries {
        let (ty, count, bytes): (u16, u32, Vec<u8>) = match v {
            Val::Ascii(s) => {
                let mut b = s.as_bytes().to_vec();
                b.push(0);
                (2, b.len() as u32, b)
            }
            Val::Long(l) => (4, 1, l.to_le_bytes().to_vec()),
            Val::Bytes(b) => (1, b.len() as u32, b.clone()),
            Val::Rationals(r) => (
                5,
                r.len() as u32,
                r.iter()
                    .flat_map(|(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes()))
                    .collect(),
            ),
        };
        head.extend(tag.to_le_bytes());
        head.extend(ty.to_le_bytes());
        head.extend(count.to_le_bytes());
        if bytes.len() <= 4 {
            let mut b = bytes.clone();
            b.resize(4, 0);
            head.extend(b);
        } else {
            head.extend((data_start + data.len() as u32).to_le_bytes());
            data.extend(&bytes);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    head.extend(next.to_le_bytes());
    (head, data)
}

/// Degrees → (deg, min, sec×100) rationals.
fn dms(v: f64) -> Vec<(u32, u32)> {
    let v = v.abs();
    let d = v.trunc();
    let m = ((v - d) * 60.0).trunc();
    let s = ((v - d) * 60.0 - m) * 60.0;
    vec![
        (d as u32, 1),
        (m as u32, 1),
        ((s * 100.0).round() as u32, 100),
    ]
}

fn exif_app1() -> Vec<u8> {
    // Layout: header(8) | IFD0 | data0 | ExifIFD | dataE | GPSIFD | dataG
    let ifd0_at = 8u32;
    let ifd0_len = |extra: u32| 2 + 5 * 12 + 4 + extra;
    // First pass with placeholder pointers to learn sizes.
    let build = |exif_at: u32, gps_at: u32| {
        ifd(
            &[
                (0x010f, Val::Ascii("Google")),
                (0x0110, Val::Ascii("Pixel 8")),
                (0x0131, Val::Ascii("HDR+ 1.0.612")),
                (0x8769, Val::Long(exif_at)),
                (0x8825, Val::Long(gps_at)),
            ],
            ifd0_at,
            0,
        )
    };
    let (h0, d0) = build(0, 0);
    let exif_at = ifd0_at + h0.len() as u32 + d0.len() as u32;
    let exif_entries = [
        (0x9003, Val::Ascii("2026:10:03 14:22:10")),
        (0x9011, Val::Ascii("-07:00")),
    ];
    let (he, de) = ifd(&exif_entries, exif_at, 0);
    let gps_at = exif_at + he.len() as u32 + de.len() as u32;
    let (h0, d0) = build(exif_at, gps_at);
    let _ = ifd0_len;
    let gps_entries = [
        (0x0000, Val::Bytes(vec![2, 3, 0, 0])),
        (0x0001, Val::Ascii("N")),
        (0x0002, Val::Rationals(dms(45.52))),
        (0x0003, Val::Ascii("W")),
        (0x0004, Val::Rationals(dms(-122.68))),
    ];
    let (hg, dg) = ifd(&gps_entries, gps_at, 0);
    let mut tiff = b"II*\0".to_vec();
    tiff.extend(ifd0_at.to_le_bytes());
    for part in [h0, d0, he, de, hg, dg] {
        tiff.extend(part);
    }
    let mut seg = vec![0xFF, 0xE1];
    let len = (2 + 6 + tiff.len()) as u16;
    seg.extend(len.to_be_bytes());
    seg.extend(b"Exif\0\0");
    seg.extend(tiff);
    seg
}

fn tutor_photo() -> Vec<u8> {
    // A dusk skyline of blocks over water, drawn from code.
    let img = RgbImage::from_fn(320, 240, |x, y| {
        let sky = [
            (250 - y / 2) as u8,
            (150 - y / 4) as u8,
            (110 + y / 3) as u8,
        ];
        let tower = (x / 23) % 3 != 1 && y > 90 + (x * 7 % 50);
        if y > 190 {
            Rgb([40, 70, 110])
        } else if tower {
            Rgb([35, 35, 45])
        } else {
            Rgb(sky)
        }
    });
    let mut jpg = Cursor::new(Vec::new());
    img.write_to(&mut jpg, ImageFormat::Jpeg).unwrap();
    let jpg = jpg.into_inner();
    // SOI, then our APP1, then the rest.
    let mut out = jpg[..2].to_vec();
    out.extend(exif_app1());
    out.extend(&jpg[2..]);
    out
}

fn http_line(url: &str, ct: &str, body: &[u8]) -> String {
    serde_json::json!({
        "kind": "http", "method": "GET", "url": url, "status": 200,
        "headers": { "content-type": ct },
        "body_b64": base64::engine::general_purpose::STANDARD.encode(body),
    })
    .to_string()
}

/// The avatar fixture lines: `(url, bytes)`.
fn avatars() -> Vec<(&'static str, Vec<u8>)> {
    let base = portrait(96, 96, 0);
    let bigger = image::imageops::resize(&base, 128, 128, image::imageops::FilterType::Triangle);
    vec![
        ("https://avatars.example/u/1001.png", png(&base)),
        (
            "https://cdn.bsky.example/img/avatar/jmarlow.png",
            png(&bigger),
        ),
        (
            "https://gitlab.example/uploads/user/avatar/7001/avatar.png",
            png(&portrait(96, 96, 12)),
        ),
        (
            "https://files.mastodon.example/accounts/avatars/jm.png",
            png(&stripes(96, 96)),
        ),
    ]
}

fn avatar_jsonl() -> String {
    let mut s = String::from(
        "// Generated by tests/tutor_assets.rs: the tutor handle's avatars (fictional, drawn from code).\n",
    );
    for (url, bytes) in avatars() {
        s.push_str(&http_line(url, "image/png", &bytes));
        s.push('\n');
    }
    s
}

fn check_or_write(path: PathBuf, want: &[u8]) {
    if std::env::var_os("SPEKTR_REGEN_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, want).unwrap();
        return;
    }
    let have = std::fs::read(&path).unwrap_or_default();
    assert!(
        have == want,
        "{} is stale; regenerate with SPEKTR_REGEN_FIXTURES=1",
        path.display()
    );
}

#[test]
fn avatars_correlate_three_and_not_the_fourth() {
    let a = avatars();
    let h: Vec<String> = a
        .iter()
        .map(|(_, b)| spektr_modules::media::dhash(b).unwrap())
        .collect();
    let d = |i: usize, j: usize| spektr_modules::media::dhash_distance(&h[i], &h[j]).unwrap();
    for (i, j) in [(0, 1), (0, 2), (1, 2)] {
        assert!(
            d(i, j) <= 6,
            "avatars {i} and {j} should match: distance {} ({:?})",
            d(i, j),
            h
        );
    }
    for i in 0..3 {
        assert!(
            d(i, 3) > 12,
            "avatar {i} vs the stripes: distance {}",
            d(i, 3)
        );
    }
    check_or_write(
        dir().join("avatar-hash/jmarlow.wav.jsonl"),
        avatar_jsonl().as_bytes(),
    );
}

#[test]
fn tutor_photo_has_exif_time_and_gps() {
    let photo = tutor_photo();
    let info = spektr_modules::media::exif(&photo).unwrap();
    assert_eq!(info.device.as_deref(), Some("Google Pixel 8"));
    assert_eq!(info.software.as_deref(), Some("HDR+ 1.0.612"));
    assert_eq!(info.taken_at.as_deref(), Some("2026-10-03T21:22:10Z"));
    let (lat, lon) = info.gps.unwrap();
    assert!(
        (lat - 45.52).abs() < 1e-3 && (lon + 122.68).abs() < 1e-3,
        "{lat} {lon}"
    );
    check_or_write(dir().join("tutor/IMG_2214.jpg"), &photo);
}
