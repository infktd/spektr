//! Media helpers (ARCHITECTURE §2): EXIF, the avatar dHash, the favicon
//! MurmurHash3. Pure functions over bytes; no I/O.

use image_hasher::{HashAlg, HasherConfig};

/// The 64-bit difference hash (dHash, 8×8 gradient) of an image, as 16 hex
/// digits. `None` when the bytes don't decode as an image.
pub fn dhash(bytes: &[u8]) -> Option<String> {
    let img = image::load_from_memory(bytes).ok()?;
    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::Gradient)
        .hash_size(8, 8)
        .to_hasher();
    let h = hasher.hash_image(&img);
    Some(h.as_bytes().iter().map(|b| format!("{b:02x}")).collect())
}

/// Hamming distance between two dHash hex strings.
pub fn dhash_distance(a: &str, b: &str) -> Option<u32> {
    let pa = u64::from_str_radix(a, 16).ok()?;
    let pb = u64::from_str_radix(b, 16).ok()?;
    Some((pa ^ pb).count_ones())
}

/// MurmurHash3 x86 32-bit.
pub fn mmh3_32(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let mut h = seed;
    let (blocks, tail) = data.as_chunks::<4>();
    for c in blocks {
        let mut k = u32::from_le_bytes(*c);
        k = k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        h ^= k;
        h = h.rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64);
    }
    let mut k: u32 = 0;
    for (i, b) in tail.iter().enumerate() {
        k |= u32::from(*b) << (8 * i);
    }
    if !tail.is_empty() {
        k = k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        h ^= k;
    }
    h ^= data.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

/// The favicon hash search engines use: MurmurHash3 (signed 32-bit) of the
/// MIME base64 of the bytes (76-column lines, each ending in `\n`).
pub fn favicon_hash(bytes: &[u8]) -> i32 {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut mime = String::with_capacity(b64.len() + b64.len() / 76 + 1);
    for chunk in b64.as_bytes().chunks(76) {
        mime.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        mime.push('\n');
    }
    mmh3_32(mime.as_bytes(), 0) as i32
}

/// What `exif` reads from an image.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExifInfo {
    pub device: Option<String>,
    pub software: Option<String>,
    /// RFC 3339, with the image's offset when it records one, else UTC.
    pub taken_at: Option<String>,
    pub gps: Option<(f64, f64)>,
}

/// EXIF `2026:10:03 14:22:10` (+ an optional `+02:00` offset) → RFC 3339.
fn exif_time(raw: &str, offset: Option<&str>) -> Option<String> {
    let raw = raw.trim().trim_matches('\0');
    let (date, time) = raw.split_once(' ').or_else(|| raw.split_once('T'))?;
    let date = date.replace(':', "-");
    let time = time.get(..8)?;
    let off = offset
        .map(str::trim)
        .filter(|o| o.len() == 6)
        .unwrap_or("Z");
    let s = format!("{date}T{time}{off}");
    s.parse::<jiff::Timestamp>().ok().map(|t| t.to_string())
}

pub fn exif(bytes: &[u8]) -> Result<ExifInfo, String> {
    use nom_exif::{ExifTag, MediaParser, MediaSource};
    let ms = MediaSource::from_memory(bytes.to_vec()).map_err(|e| e.to_string())?;
    let mut parser = MediaParser::new();
    let iter = parser.parse_exif(ms).map_err(|e| e.to_string())?;
    let ex: nom_exif::Exif = iter.into();
    let text = |t: ExifTag| {
        ex.get(t)
            .map(|v| v.to_string().trim().trim_matches('\0').to_string())
            .filter(|s| !s.is_empty())
    };
    let device = match (text(ExifTag::Make), text(ExifTag::Model)) {
        (Some(make), Some(model)) if model.starts_with(&make) => Some(model),
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (a, b) => a.or(b),
    };
    let offset = text(ExifTag::OffsetTimeOriginal);
    let taken_at = text(ExifTag::DateTimeOriginal).and_then(|t| exif_time(&t, offset.as_deref()));
    let gps = ex
        .gps_info()
        .and_then(|g| Some((g.latitude_decimal()?, g.longitude_decimal()?)));
    Ok(ExifInfo {
        device,
        software: text(ExifTag::Software),
        taken_at,
        gps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmh3_reference_values() {
        // Reference vectors for MurmurHash3_x86_32.
        assert_eq!(mmh3_32(b"", 0), 0);
        assert_eq!(mmh3_32(b"", 1), 0x514e_28b7);
        assert_eq!(mmh3_32(b"hello", 0), 0x248b_fa47);
        assert_eq!(mmh3_32(b"Hello, world!", 1234), 0xfaf6_cdb3);
    }

    #[test]
    fn exif_times() {
        assert_eq!(
            exif_time("2026:10:03 14:22:10", None).as_deref(),
            Some("2026-10-03T14:22:10Z")
        );
        assert_eq!(
            exif_time("2026:10:03 14:22:10", Some("-07:00")).as_deref(),
            Some("2026-10-03T21:22:10Z")
        );
        assert_eq!(exif_time("garbage", None), None);
    }

    #[test]
    fn dhash_distances() {
        assert_eq!(
            dhash_distance("ff00ff00ff00ff00", "ff00ff00ff00ff01"),
            Some(1)
        );
        assert_eq!(dhash(b"not an image"), None);
    }
}
