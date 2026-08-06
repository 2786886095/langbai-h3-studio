//! Managed ComfyUI runtime layout and lifecycle planning.
//!
//! This module deliberately does not download or spawn ComfyUI.  It owns the
//! durable, testable part of runtime management; the Tauri command layer may
//! execute the returned plans and report observations through `ProcessHealth`.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const LOOPBACK: &str = "127.0.0.1";

#[derive(Debug)]
pub enum RuntimeError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidVersion,
    MissingStaging(PathBuf),
    VersionAlreadyInstalled(PathBuf),
    NoCurrentRuntime,
    UnsafeExecutable(PathBuf),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "runtime I/O error: {e}"),
            Self::Json(e) => write!(f, "runtime metadata error: {e}"),
            Self::InvalidVersion => write!(f, "runtime version is invalid"),
            Self::MissingStaging(p) => write!(f, "staging runtime is missing: {}", p.display()),
            Self::VersionAlreadyInstalled(p) => {
                write!(f, "runtime already exists: {}", p.display())
            }
            Self::NoCurrentRuntime => write!(f, "no current runtime is selected"),
            Self::UnsafeExecutable(p) => {
                write!(f, "runtime executable escapes its profile: {}", p.display())
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for RuntimeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurrentRuntime {
    pub version: String,
    pub profile_dir: PathBuf,
    pub activated_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessHealth {
    Stopped,
    Starting,
    Healthy { pid: u32, endpoint: String },
    Unhealthy { pid: Option<u32>, reason: String },
    Stopping { pid: u32 },
    Exited { code: Option<i32> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub endpoint: String,
    pub port: u16,
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StopPlan {
    pub pid: u32,
    pub graceful_timeout_ms: u64,
    pub force_after_timeout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestartPlan {
    pub stop: StopPlan,
    pub start: LaunchPlan,
}

#[derive(Debug, Clone)]
pub struct RuntimeManager {
    root: PathBuf,
}

impl RuntimeManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn staging_dir(&self, version: &str) -> Result<PathBuf, RuntimeError> {
        validate_version(version)?;
        Ok(self.root.join("staging").join(version))
    }
    pub fn profile_dir(&self, version: &str) -> Result<PathBuf, RuntimeError> {
        validate_version(version)?;
        Ok(self.versions_dir().join(version))
    }
    pub fn current_file(&self) -> PathBuf {
        self.root.join("current.json")
    }

    pub fn prepare_staging(&self, version: &str) -> Result<PathBuf, RuntimeError> {
        let path = self.staging_dir(version)?;
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn current(&self) -> Result<Option<CurrentRuntime>, RuntimeError> {
        let path = self.current_file();
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
    }

    /// Promote a fully prepared staging directory and switch `current.json`.
    /// If metadata activation fails, the promoted directory is moved back to
    /// staging and the previous current metadata is restored.
    pub fn activate_staged(&self, version: &str) -> Result<CurrentRuntime, RuntimeError> {
        let staging = self.staging_dir(version)?;
        if !staging.is_dir() {
            return Err(RuntimeError::MissingStaging(staging));
        }
        let profile = self.profile_dir(version)?;
        if profile.exists() {
            return Err(RuntimeError::VersionAlreadyInstalled(profile));
        }
        fs::create_dir_all(self.versions_dir())?;
        fs::rename(&staging, &profile)?;
        let record = CurrentRuntime {
            version: version.to_owned(),
            profile_dir: profile.clone(),
            activated_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        };
        if let Err(error) = self.write_current(&record) {
            let _ = fs::create_dir_all(staging.parent().unwrap_or(&self.root));
            let _ = fs::rename(&profile, &staging);
            return Err(error);
        }
        Ok(record)
    }

    pub fn select_installed(&self, version: &str) -> Result<CurrentRuntime, RuntimeError> {
        let profile = self.profile_dir(version)?;
        if !profile.is_dir() {
            return Err(RuntimeError::NoCurrentRuntime);
        }
        let record = CurrentRuntime {
            version: version.into(),
            profile_dir: profile,
            activated_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        };
        self.write_current(&record)?;
        Ok(record)
    }

    fn write_current(&self, record: &CurrentRuntime) -> Result<(), RuntimeError> {
        fs::create_dir_all(&self.root)?;
        let current = self.current_file();
        let temp = self.root.join("current.json.new");
        let backup = self.root.join("current.json.rollback");
        fs::write(&temp, serde_json::to_vec_pretty(record)?)?;
        if backup.exists() {
            fs::remove_file(&backup)?;
        }
        if current.exists() {
            fs::rename(&current, &backup)?;
        }
        if let Err(e) = fs::rename(&temp, &current) {
            if backup.exists() {
                let _ = fs::rename(&backup, &current);
            }
            return Err(e.into());
        }
        if backup.exists() {
            fs::remove_file(backup)?;
        }
        Ok(())
    }

    pub fn launch_plan(
        &self,
        python_relative: impl AsRef<Path>,
        main_relative: impl AsRef<Path>,
    ) -> Result<LaunchPlan, RuntimeError> {
        let current = self.current()?.ok_or(RuntimeError::NoCurrentRuntime)?;
        let program = safe_child(&current.profile_dir, python_relative.as_ref())?;
        let main = safe_child(&current.profile_dir, main_relative.as_ref())?;
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
        let port = listener.local_addr()?.port();
        drop(listener);
        Ok(LaunchPlan {
            program,
            args: vec![
                main.to_string_lossy().into_owned(),
                "--listen".into(),
                LOOPBACK.into(),
                "--port".into(),
                port.to_string(),
            ],
            working_dir: current.profile_dir,
            endpoint: format!("http://{LOOPBACK}:{port}"),
            port,
            environment: BTreeMap::from([("PYTHONUTF8".into(), "1".into())]),
        })
    }

    pub fn stop_plan(pid: u32) -> StopPlan {
        StopPlan {
            pid,
            graceful_timeout_ms: 10_000,
            force_after_timeout: true,
        }
    }
    pub fn restart_plan(
        &self,
        pid: u32,
        python: impl AsRef<Path>,
        main: impl AsRef<Path>,
    ) -> Result<RestartPlan, RuntimeError> {
        Ok(RestartPlan {
            stop: Self::stop_plan(pid),
            start: self.launch_plan(python, main)?,
        })
    }
}

fn validate_version(version: &str) -> Result<(), RuntimeError> {
    if version.is_empty()
        || version == "."
        || version == ".."
        || version.contains(['/', '\\'])
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(RuntimeError::InvalidVersion);
    }
    Ok(())
}

fn safe_child(root: &Path, relative: &Path) -> Result<PathBuf, RuntimeError> {
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(RuntimeError::UnsafeExecutable(relative.to_path_buf()));
    }
    Ok(root.join(relative))
}

/// Create ComfyUI's model path configuration without adding a YAML dependency.
pub fn render_extra_model_paths(
    base_path: &Path,
    categories: &BTreeMap<String, Vec<PathBuf>>,
) -> String {
    let mut out = format!(
        "langbai_h3_studio:\n  base_path: {}\n",
        yaml_quote(base_path)
    );
    for (category, paths) in categories {
        out.push_str(&format!("  {}:\n", category));
        for path in paths {
            out.push_str(&format!("    - {}\n", yaml_quote(path)));
        }
    }
    out
}

pub fn write_extra_model_paths(
    path: &Path,
    base_path: &Path,
    categories: &BTreeMap<String, Vec<PathBuf>>,
) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, render_extra_model_paths(base_path, categories))?;
    Ok(())
}

fn yaml_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('"', "\\\"")
    )
}
