//! Blobs: raw evidence bytes, zstd-compressed (level 3), keyed by the SHA-256
//! of the uncompressed bytes (DATA_MODEL §8, §10).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Digest;

use crate::error::{StoreError, StoreResult, invalid};

pub const ZSTD_LEVEL: i32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256(pub [u8; 32]);

impl Sha256 {
    pub fn from_slice(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(Self)
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `9a3f…11`, as in the inspector's evidence list.
    pub fn short(&self) -> String {
        let h = self.hex();
        format!("{}…{}", &h[..4], &h[62..])
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({})", self.hex())
    }
}

impl FromStr for Sha256 {
    type Err = StoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.len() != 64 {
            return Err(invalid(format!("expected 64 hex digits, got `{s}`")));
        }
        let mut out = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex = std::str::from_utf8(chunk).map_err(|_| invalid("bad hex"))?;
            out[i] = u8::from_str_radix(hex, 16).map_err(|_| invalid(format!("bad hex `{s}`")))?;
        }
        Ok(Self(out))
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

pub fn sha256(bytes: &[u8]) -> Sha256 {
    let mut out = [0u8; 32];
    out.copy_from_slice(&sha2::Sha256::digest(bytes));
    Sha256(out)
}

/// Stored blob metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobInfo {
    pub sha256: Sha256,
    pub size: u64,
    pub compression: String,
}

pub(crate) fn compress(bytes: &[u8]) -> StoreResult<Vec<u8>> {
    Ok(zstd::bulk::compress(bytes, ZSTD_LEVEL)?)
}

pub(crate) fn decompress(data: &[u8], compression: &str, size: u64) -> StoreResult<Vec<u8>> {
    match compression {
        "zstd" => {
            let cap = usize::try_from(size).map_err(|_| invalid("blob size overflows usize"))?;
            let out = zstd::bulk::decompress(data, cap)?;
            if out.len() != cap {
                return Err(invalid(format!(
                    "blob decompressed to {} bytes, expected {cap}",
                    out.len()
                )));
            }
            Ok(out)
        }
        "none" => Ok(data.to_vec()),
        other => Err(invalid(format!("unknown blob compression `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let data = b"hello hello hello hello".repeat(100);
        let c = compress(&data).unwrap();
        assert!(c.len() < data.len());
        assert_eq!(decompress(&c, "zstd", data.len() as u64).unwrap(), data);
        let h = sha256(&data);
        assert_eq!(h.hex().parse::<Sha256>().unwrap(), h);
        assert_eq!(
            sha256(b"").hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
