//! Declarative `.h3plugin` package inspection and profile-local installation.
//!
//! Packages are data adapters, not executable extensions.  The allow-list below
//! is intentionally closed: a package can contain JSON workflow data only.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

#[derive(Debug)]
pub enum PluginError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    Invalid(String),
    Conflict(Vec<String>),
    NotInstalled,
}
impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "plugin I/O error: {e}"),
            Self::Zip(e) => write!(f, "invalid plugin ZIP: {e}"),
            Self::Json(e) => write!(f, "invalid plugin JSON: {e}"),
            Self::Invalid(e) => write!(f, "invalid plugin: {e}"),
            Self::Conflict(v) => write!(f, "plugin conflicts with: {}", v.join(", ")),
            Self::NotInstalled => write!(f, "plugin is not installed"),
        }
    }
}
impl std::error::Error for PluginError {}
impl From<io::Error> for PluginError {
    fn from(v: io::Error) -> Self {
        Self::Io(v)
    }
}
impl From<zip::result::ZipError> for PluginError {
    fn from(v: zip::result::ZipError) -> Self {
        Self::Zip(v)
    }
}
impl From<serde_json::Error> for PluginError {
    fn from(v: serde_json::Error) -> Self {
        Self::Json(v)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub targets: Targets,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub requires: Requirements,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub workflows: Vec<Workflow>,
    pub parameters: Option<String>,
    pub license: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Targets {
    #[serde(default)]
    pub studio: String,
    #[serde(default)]
    pub comfyui: String,
    #[serde(default)]
    pub os: Vec<String>,
    #[serde(default)]
    pub gpu: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Requirements {
    #[serde(default)]
    pub nodes: Vec<NodeRequirement>,
    #[serde(default)]
    pub models: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRequirement {
    #[serde(rename = "class")]
    pub class_name: String,
    #[serde(default)]
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workflow {
    pub capability: String,
    pub template: String,
    pub bindings: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageInspection {
    pub manifest: PluginManifest,
    pub package_sha256: String,
    pub files: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub studio_compatible: bool,
    pub missing_nodes: Vec<String>,
    pub conflicts: Vec<String>,
    pub provides: Vec<String>,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginLock {
    pub plugins: BTreeMap<String, LockedPlugin>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPlugin {
    pub version: String,
    pub enabled: bool,
    pub sha256: String,
    pub provides: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginManager {
    profile: PathBuf,
}
impl PluginManager {
    pub fn new(profile: impl Into<PathBuf>) -> Self {
        Self {
            profile: profile.into(),
        }
    }
    pub fn root(&self) -> PathBuf {
        self.profile.join(".h3plugins")
    }
    pub fn lock_path(&self) -> PathBuf {
        self.root().join("lock.json")
    }
    pub fn read_lock(&self) -> Result<PluginLock, PluginError> {
        let p = self.lock_path();
        if !p.exists() {
            return Ok(PluginLock::default());
        }
        Ok(serde_json::from_slice(&fs::read(p)?)?)
    }

    pub fn compatibility(
        &self,
        p: &PackageInspection,
        studio_version: &str,
        available_nodes: &BTreeSet<String>,
    ) -> Result<CompatibilityReport, PluginError> {
        let lock = self.read_lock()?;
        let studio_ok = matches_range(studio_version, &p.manifest.targets.studio);
        let missing_nodes = p
            .manifest
            .requires
            .nodes
            .iter()
            .filter(|n| !available_nodes.contains(&n.class_name))
            .map(|n| n.class_name.clone())
            .collect::<Vec<_>>();
        let conflicts = p
            .manifest
            .conflicts
            .iter()
            .filter(|id| lock.plugins.get(*id).is_some_and(|v| v.enabled))
            .cloned()
            .collect::<Vec<_>>();
        let mut reasons = Vec::new();
        if !studio_ok {
            reasons.push(format!(
                "Studio {studio_version} 不满足 {}",
                p.manifest.targets.studio
            ));
        }
        if !missing_nodes.is_empty() {
            reasons.push("缺少必需 ComfyUI 节点".into())
        }
        if !conflicts.is_empty() {
            reasons.push("与已启用插件冲突".into())
        }
        Ok(CompatibilityReport {
            compatible: studio_ok && missing_nodes.is_empty() && conflicts.is_empty(),
            studio_compatible: studio_ok,
            missing_nodes,
            conflicts,
            provides: p.manifest.provides.clone(),
            reasons,
        })
    }

    pub fn install(
        &self,
        package: &Path,
        expected_sha256: Option<&str>,
        studio_version: &str,
        nodes: &BTreeSet<String>,
    ) -> Result<LockedPlugin, PluginError> {
        let inspected = inspect_package(package, expected_sha256)?;
        let report = self.compatibility(&inspected, studio_version, nodes)?;
        if !report.conflicts.is_empty() {
            return Err(PluginError::Conflict(report.conflicts));
        }
        if !report.compatible {
            return Err(PluginError::Invalid(report.reasons.join("；")));
        }
        let root = self.root();
        fs::create_dir_all(&root)?;
        let final_dir = root
            .join(&inspected.manifest.id)
            .join(&inspected.manifest.version);
        if final_dir.exists() {
            return Err(PluginError::Invalid("该版本已经安装".into()));
        }
        let stage = root.join(format!(
            ".install-{}-{}.tmp",
            inspected.manifest.id, inspected.manifest.version
        ));
        if stage.exists() {
            fs::remove_dir_all(&stage)?
        }
        fs::create_dir_all(&stage)?;
        let result = (|| {
            extract_declarative(package, &stage)?;
            let mut lock = self.read_lock()?;
            let entry = LockedPlugin {
                version: inspected.manifest.version.clone(),
                enabled: true,
                sha256: inspected.package_sha256.clone(),
                provides: inspected.manifest.provides.clone(),
            };
            lock.plugins
                .insert(inspected.manifest.id.clone(), entry.clone());
            fs::create_dir_all(final_dir.parent().unwrap())?;
            fs::rename(&stage, &final_dir)?;
            if let Err(e) = write_lock_atomic(&self.lock_path(), &lock) {
                let _ = fs::rename(&final_dir, &stage);
                return Err(e);
            }
            Ok(entry)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PluginError> {
        validate_id(id)?;
        let mut l = self.read_lock()?;
        let item = l.plugins.get_mut(id).ok_or(PluginError::NotInstalled)?;
        item.enabled = enabled;
        write_lock_atomic(&self.lock_path(), &l)
    }
    pub fn uninstall(&self, id: &str) -> Result<(), PluginError> {
        validate_id(id)?;
        let mut l = self.read_lock()?;
        let item = l
            .plugins
            .get(id)
            .cloned()
            .ok_or(PluginError::NotInstalled)?;
        let dir = self.root().join(id).join(&item.version);
        let tomb = self
            .root()
            .join(format!(".remove-{id}-{}.tmp", item.version));
        if tomb.exists() {
            fs::remove_dir_all(&tomb)?
        }
        if dir.exists() {
            fs::rename(&dir, &tomb)?
        }
        l.plugins.remove(id);
        if let Err(e) = write_lock_atomic(&self.lock_path(), &l) {
            if tomb.exists() {
                let _ = fs::rename(&tomb, &dir);
            }
            return Err(e);
        }
        if tomb.exists() {
            fs::remove_dir_all(tomb)?
        }
        Ok(())
    }
}

pub fn inspect_package(
    path: &Path,
    expected_sha256: Option<&str>,
) -> Result<PackageInspection, PluginError> {
    let bytes = fs::read(path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if let Some(e) = expected_sha256 {
        if !is_sha256(e) || !digest.eq_ignore_ascii_case(e) {
            return Err(PluginError::Invalid("包 SHA-256 不匹配".into()));
        }
    }
    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut manifest_bytes = None;
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let raw = f.name().replace('\\', "/");
        validate_entry(&raw, f.is_dir())?;
        if f.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        serde_json::from_slice::<serde_json::Value>(&data)
            .map_err(|_| PluginError::Invalid(format!("{raw} 不是有效 JSON")))?;
        if raw == "manifest.json" {
            manifest_bytes = Some(data)
        }
        files.push(raw)
    }
    let manifest: PluginManifest = serde_json::from_slice(
        &manifest_bytes.ok_or_else(|| PluginError::Invalid("缺少 manifest.json".into()))?,
    )?;
    validate_manifest(&manifest, &files)?;
    Ok(PackageInspection {
        manifest,
        package_sha256: digest,
        files,
    })
}

fn validate_manifest(m: &PluginManifest, files: &[String]) -> Result<(), PluginError> {
    if m.schema_version != 1 {
        return Err(PluginError::Invalid("仅支持 schemaVersion=1".into()));
    }
    validate_id(&m.id)?;
    validate_version(&m.version)?;
    if !m
        .targets
        .os
        .iter()
        .any(|x| x.eq_ignore_ascii_case("windows"))
    {
        return Err(PluginError::Invalid("插件不支持 Windows".into()));
    }
    if !m
        .targets
        .gpu
        .iter()
        .any(|x| x.eq_ignore_ascii_case("nvidia"))
    {
        return Err(PluginError::Invalid("插件不支持 NVIDIA".into()));
    }
    for a in &m.artifacts {
        if !is_sha256(&a.sha256) {
            return Err(PluginError::Invalid("artifact SHA-256 格式无效".into()));
        }
    }
    let declared = m
        .workflows
        .iter()
        .flat_map(|w| [&w.template, &w.bindings])
        .chain(m.parameters.iter());
    for p in declared {
        safe_relative(p)?;
        if !files.contains(p) {
            return Err(PluginError::Invalid(format!("声明文件不存在: {p}")));
        }
    }
    Ok(())
}
fn validate_entry(name: &str, is_dir: bool) -> Result<(), PluginError> {
    safe_relative(name)?;
    if is_dir {
        return Ok(());
    }
    let allowed = name == "manifest.json"
        || name == "parameters.schema.json"
        || ["workflows/", "bindings/", "benchmarks/"]
            .iter()
            .any(|p| name.starts_with(p) && name.ends_with(".json"));
    if !allowed {
        return Err(PluginError::Invalid(format!("禁止的包内文件: {name}")));
    }
    Ok(())
}
fn safe_relative(s: &str) -> Result<(), PluginError> {
    let p = Path::new(s);
    if s.is_empty()
        || p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PluginError::Invalid(format!("不安全路径: {s}")));
    }
    Ok(())
}
fn validate_id(s: &str) -> Result<(), PluginError> {
    if s.len() > 128
        || s.is_empty()
        || s.split('.').any(|x| {
            x.is_empty()
                || !x
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    {
        Err(PluginError::Invalid("插件 ID 无效".into()))
    } else {
        Ok(())
    }
}
fn validate_version(s: &str) -> Result<(), PluginError> {
    if s.split(['-', '+']).next().unwrap_or("").split('.').count() != 3
        || parse_version(s).is_none()
    {
        Err(PluginError::Invalid("插件版本必须是 x.y.z".into()))
    } else {
        Ok(())
    }
}
fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
}
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.split(['-', '+']).next()?;
    let mut i = core.split('.');
    Some((
        i.next()?.parse().ok()?,
        i.next().unwrap_or("0").parse().ok()?,
        i.next().unwrap_or("0").parse().ok()?,
    ))
}
fn matches_range(v: &str, range: &str) -> bool {
    if range.trim().is_empty() {
        return true;
    }
    let Some(v) = parse_version(v) else {
        return false;
    };
    range.split_whitespace().all(|t| {
        let (op, n) = if let Some(x) = t.strip_prefix(">=") {
            (">=", x)
        } else if let Some(x) = t.strip_prefix("<=") {
            ("<=", x)
        } else if let Some(x) = t.strip_prefix('>') {
            (">", x)
        } else if let Some(x) = t.strip_prefix('<') {
            ("<", x)
        } else if let Some(x) = t.strip_prefix('=') {
            ("=", x)
        } else {
            ("=", t)
        };
        parse_version(n).is_some_and(|n| match op {
            ">=" => v >= n,
            "<=" => v <= n,
            ">" => v > n,
            "<" => v < n,
            _ => v == n,
        })
    })
}
fn extract_declarative(package: &Path, dest: &Path) -> Result<(), PluginError> {
    let mut z = ZipArchive::new(fs::File::open(package)?)?;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let n = f.name().replace('\\', "/");
        validate_entry(&n, f.is_dir())?;
        if f.is_dir() {
            continue;
        }
        let out = dest.join(&n);
        fs::create_dir_all(out.parent().unwrap())?;
        let mut w = fs::File::create(out)?;
        io::copy(&mut f, &mut w)?;
    }
    Ok(())
}
fn write_lock_atomic(path: &Path, lock: &PluginLock) -> Result<(), PluginError> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.new");
    let bak = path.with_extension("json.rollback");
    fs::write(&tmp, serde_json::to_vec_pretty(lock)?)?;
    if bak.exists() {
        fs::remove_file(&bak)?
    }
    if path.exists() {
        fs::rename(path, &bak)?
    }
    if let Err(e) = fs::rename(&tmp, path) {
        if bak.exists() {
            let _ = fs::rename(&bak, path);
        }
        return Err(e.into());
    }
    if bak.exists() {
        fs::remove_file(bak)?
    }
    Ok(())
}
