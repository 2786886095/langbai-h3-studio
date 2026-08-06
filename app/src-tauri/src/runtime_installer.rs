//! 托管 ComfyUI Runtime 的声明式、本地归档安装核心。
//!
//! 本模块只校验和解压数据文件；不会运行归档中的 Python、批处理或其它脚本。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub archive_format: ArchiveFormat,
    pub expected_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    Zip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhase {
    Preparing,
    Verifying,
    Extracting,
    Validating,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRuntime {
    pub staging_path: PathBuf,
    pub version: String,
    pub sha256: String,
}

pub type InstallError = String;

/// 校验本地下载完成的归档并安全安装到 `runtime_root/staging/<version>`。
///
/// 下载由上层负责（可复用 `download.rs`）；刻意不在此处执行归档内任何文件。
pub fn install_local_archive<F>(
    manifest: &RuntimeManifest,
    archive_path: &Path,
    runtime_root: &Path,
    mut emit: F,
) -> Result<InstalledRuntime, InstallError>
where
    F: FnMut(InstallProgress),
{
    validate_manifest(manifest)?;
    let total = fs::metadata(archive_path)
        .map_err(|e| format!("读取 Runtime 归档失败：{e}"))?
        .len();
    emit(event(InstallPhase::Preparing, 0, total, None));
    emit(event(InstallPhase::Verifying, 0, total, None));
    let actual = sha256_file(archive_path)?;
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(format!(
            "SHA-256 校验失败：期望 {}，实际 {actual}",
            manifest.sha256
        ));
    }

    let staging_root = runtime_root.join("staging");
    fs::create_dir_all(&staging_root).map_err(|e| format!("创建 staging 目录失败：{e}"))?;
    let destination = safe_child(&staging_root, Path::new(&manifest.version))?;
    let temporary = staging_root.join(format!(".{}.installing", manifest.version));
    remove_path_if_exists(&temporary)?;
    fs::create_dir_all(&temporary).map_err(|e| format!("创建临时安装目录失败：{e}"))?;

    let result = match manifest.archive_format {
        ArchiveFormat::Zip => extract_zip(archive_path, &temporary, total, &mut emit),
    }
    .and_then(|_| {
        emit(event(InstallPhase::Validating, total, total, None));
        validate_installation(&temporary, &manifest.expected_files)
    });
    if let Err(error) = result {
        let _ = remove_path_if_exists(&temporary);
        return Err(error);
    }

    remove_path_if_exists(&destination)?;
    fs::rename(&temporary, &destination).map_err(|e| format!("提交 Runtime 安装失败：{e}"))?;
    emit(event(InstallPhase::Completed, total, total, None));
    Ok(InstalledRuntime {
        staging_path: destination,
        version: manifest.version.clone(),
        sha256: actual,
    })
}

fn extract_zip<F>(
    archive_path: &Path,
    destination: &Path,
    total: u64,
    emit: &mut F,
) -> Result<(), InstallError>
where
    F: FnMut(InstallProgress),
{
    let file = File::open(archive_path).map_err(|e| format!("打开 ZIP 失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 ZIP 失败：{e}"))?;
    let mut completed = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("读取 ZIP 条目失败：{e}"))?;
        // enclosed_name 同时拒绝绝对路径、盘符及 `..`，避免 Zip Slip。
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP 包含不安全路径：{}", entry.name()))?
            .to_path_buf();
        validate_relative(&relative)?;
        let output_path = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(|e| format!("创建解压目录失败：{e}"))?;
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建解压目录失败：{e}"))?;
            }
            let mut output =
                File::create(&output_path).map_err(|e| format!("创建解压文件失败：{e}"))?;
            let copied =
                io::copy(&mut entry, &mut output).map_err(|e| format!("解压文件失败：{e}"))?;
            output
                .flush()
                .map_err(|e| format!("写入解压文件失败：{e}"))?;
            completed = completed.saturating_add(copied);
        }
        emit(event(
            InstallPhase::Extracting,
            completed.min(total),
            total,
            Some(relative.to_string_lossy().into_owned()),
        ));
    }
    Ok(())
}

fn validate_installation(root: &Path, expected: &[PathBuf]) -> Result<(), InstallError> {
    let mut required = vec![PathBuf::from("python"), PathBuf::from("ComfyUI/main.py")];
    required.extend(expected.iter().cloned());
    for relative in required {
        let path = safe_child(root, &relative)?;
        if !path.exists() {
            return Err(format!("Runtime 缺少必需文件：{}", relative.display()));
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &RuntimeManifest) -> Result<(), InstallError> {
    if manifest.version.trim().is_empty() {
        return Err("Runtime 版本不能为空".into());
    }
    validate_relative(Path::new(&manifest.version))?;
    if manifest.sha256.len() != 64 || !manifest.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("SHA-256 必须是 64 位十六进制字符串".into());
    }
    for path in &manifest.expected_files {
        validate_relative(path)?;
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), InstallError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("不安全的相对路径：{}", path.display()));
    }
    Ok(())
}

fn safe_child(root: &Path, relative: &Path) -> Result<PathBuf, InstallError> {
    validate_relative(relative)?;
    Ok(root.join(relative))
}

fn sha256_file(path: &Path) -> Result<String, InstallError> {
    let mut file = File::open(path).map_err(|e| format!("打开归档失败：{e}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("读取归档失败：{e}"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn remove_path_if_exists(path: &Path) -> Result<(), InstallError> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|e| format!("清理旧安装失败：{e}"))
}

fn event(
    phase: InstallPhase,
    completed: u64,
    total: u64,
    current_file: Option<String>,
) -> InstallProgress {
    InstallProgress {
        phase,
        completed_bytes: completed,
        total_bytes: total,
        progress_percent: if total == 0 {
            100.0
        } else {
            completed as f64 * 100.0 / total as f64
        },
        current_file,
    }
}
