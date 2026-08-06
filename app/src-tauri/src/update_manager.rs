//! Update manifest parsing, selection, verification, and handoff primitives.
//! Private signing keys never belong in the application or repository.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    PreRelease,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}
impl Version {
    pub fn parse(s: &str) -> Result<Self, UpdateError> {
        let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let s = s.split_once('+').map_or(s, |x| x.0);
        let (core, pre) = s.split_once('-').map_or((s, ""), |x| x);
        let mut p = core.split('.');
        let n = |v: Option<&str>| {
            v.and_then(|x| x.parse().ok())
                .ok_or_else(|| UpdateError::InvalidVersion(s.into()))
        };
        let v = Self {
            major: n(p.next())?,
            minor: n(p.next())?,
            patch: n(p.next())?,
            pre: if pre.is_empty() {
                vec![]
            } else {
                pre.split('.').map(str::to_owned).collect()
            },
        };
        if p.next().is_some() || v.pre.iter().any(|x| x.is_empty()) {
            Err(UpdateError::InvalidVersion(s.into()))
        } else {
            Ok(v)
        }
    }
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}
impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            _ => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(alias = "browser_download_url", alias = "url")]
    pub download_url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub signature: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    #[serde(alias = "tag_name")]
    pub version: String,
    #[serde(default, alias = "prerelease")]
    pub pre_release: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// Accepts a GitHub release object/array and a custom `{ "releases": [...] }` manifest.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, UpdateError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    let r = if let Some(x) = v.get("releases") {
        serde_json::from_value(x.clone())
    } else if v.is_array() {
        serde_json::from_value(v)
    } else {
        serde_json::from_value(v).map(|x| vec![x])
    };
    r.map_err(|e| UpdateError::Manifest(e.to_string()))
}
pub fn select_update<'a>(
    current: &str,
    releases: &'a [Release],
    channel: UpdateChannel,
) -> Result<Option<&'a Release>, UpdateError> {
    let current = Version::parse(current)?;
    let mut best: Option<(&Release, Version)> = None;
    for r in releases.iter().filter(|r| !r.draft) {
        let v = Version::parse(&r.version)?;
        if channel == UpdateChannel::Stable && (r.pre_release || v.is_prerelease()) || v <= current
        {
            continue;
        }
        if best.as_ref().is_none_or(|x| v > x.1) {
            best = Some((r, v))
        }
    }
    Ok(best.map(|x| x.0))
}
pub fn select_asset<'a>(
    release: &'a Release,
    names: &[&str],
) -> Result<&'a ReleaseAsset, UpdateError> {
    names
        .iter()
        .find_map(|n| {
            release
                .assets
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(n))
        })
        .ok_or_else(|| UpdateError::AssetNotFound(names.join(", ")))
}
pub fn part_path(final_path: &Path) -> Result<PathBuf, UpdateError> {
    let n = final_path
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or(UpdateError::UnsafePath)?;
    if n.is_empty() {
        return Err(UpdateError::UnsafePath);
    }
    Ok(final_path.with_file_name(format!("{n}.part")))
}
pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), UpdateError> {
    let expected = expected.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(UpdateError::InvalidHash);
    }
    let actual = format!(
        "{:x}",
        Sha256::digest(fs::read(path).map_err(UpdateError::Io)?)
    );
    if actual == expected {
        Ok(())
    } else {
        Err(UpdateError::HashMismatch { expected, actual })
    }
}

/// Provider-neutral Ed25519 verification interface. Bind this to ed25519-dalek in the shell.
pub trait Ed25519Verifier {
    fn verify(&self, public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool;
}
pub fn verify_ed25519<V: Ed25519Verifier>(
    v: &V,
    key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), UpdateError> {
    v.verify(key, message, signature)
        .then_some(())
        .ok_or(UpdateError::SignatureMismatch)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterPlan {
    pub version: String,
    pub verified_installer: PathBuf,
    pub sha256: String,
    pub restart_executable: PathBuf,
}
/// Called only after both verifications; the independent updater consumes this atomic plan.
pub fn write_updater_plan(path: &Path, plan: &UpdaterPlan) -> Result<(), UpdateError> {
    if !plan.verified_installer.is_absolute() || !plan.restart_executable.is_absolute() {
        return Err(UpdateError::UnsafePath);
    }
    let tmp = path.with_extension("json.tmp");
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(UpdateError::Io)?
    }
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(plan).map_err(|e| UpdateError::Manifest(e.to_string()))?,
    )
    .map_err(UpdateError::Io)?;
    fs::rename(tmp, path).map_err(UpdateError::Io)
}

#[derive(Debug)]
pub enum UpdateError {
    InvalidVersion(String),
    Manifest(String),
    AssetNotFound(String),
    InvalidHash,
    HashMismatch { expected: String, actual: String },
    SignatureMismatch,
    UnsafePath,
    Io(std::io::Error),
}
impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for UpdateError {}
