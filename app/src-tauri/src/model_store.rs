//! 本地 H3 模型发现与只读校验。
//!
//! 扫描器只读取目录项和少量 JSON 元数据，不跟随符号链接，也不会移动、
//! 重命名或删除用户文件。

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const DEFAULT_MAX_DEPTH: usize = 5;
const HARD_MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum H3ModelType {
    Fl2Va,
    Ref2Va,
    UnknownH3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelIntegrity {
    Complete,
    Partial,
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub directory: String,
    pub display_name: String,
    pub model_type: H3ModelType,
    pub integrity: ModelIntegrity,
    pub total_size_bytes: u64,
    pub has_model_index: bool,
    pub safetensors_count: usize,
    pub files: Vec<ModelFile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelScanResult {
    pub root: String,
    pub max_depth: usize,
    pub models: Vec<LocalModel>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct Candidate {
    files: Vec<(PathBuf, u64, &'static str)>,
    index: Option<(PathBuf, Value)>,
    warnings: Vec<String>,
    invalid_index: bool,
}

/// 提供给 Tauri 前端的扫描命令。
#[tauri::command]
pub fn scan_local_models(
    root: String,
    max_depth: Option<usize>,
) -> Result<ModelScanResult, String> {
    scan_model_directory(root, max_depth)
}

/// 递归扫描本地模型目录。`max_depth = None` 使用 5 层，最大限制为 12 层。
pub fn scan_model_directory(
    root: impl AsRef<Path>,
    max_depth: Option<usize>,
) -> Result<ModelScanResult, String> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Err(format!("模型目录不存在或不是文件夹：{}", root.display()));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("读取模型目录失败：{e}"))?;
    let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH).min(HARD_MAX_DEPTH);
    let mut candidates = BTreeMap::<PathBuf, Candidate>::new();
    let mut warnings = Vec::new();
    walk(&root, &root, 0, max_depth, &mut candidates, &mut warnings);

    let mut models = candidates
        .into_iter()
        .map(|(directory, candidate)| build_model(directory, candidate))
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.directory.cmp(&b.directory));

    Ok(ModelScanResult {
        root: root.to_string_lossy().into_owned(),
        max_depth,
        models,
        warnings,
    })
}

fn walk(
    root: &Path,
    directory: &Path,
    depth: usize,
    max_depth: usize,
    candidates: &mut BTreeMap<PathBuf, Candidate>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) => {
            warnings.push(format!("跳过不可读取目录 {}：{e}", directory.display()));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) => {
                warnings.push(format!("跳过不可读取路径 {}：{e}", path.display()));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            warnings.push(format!("已跳过链接：{}", path.display()));
            continue;
        }
        if metadata.is_dir() {
            if depth < max_depth {
                walk(root, &path, depth + 1, max_depth, candidates, warnings);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let is_index = name == "model_index.json";
        let is_safetensors = name.ends_with(".safetensors");
        if !is_index && !is_safetensors {
            continue;
        }
        let parent = path.parent().unwrap_or(root).to_path_buf();
        let candidate = candidates.entry(parent).or_default();
        candidate.files.push((
            path.clone(),
            metadata.len(),
            if is_index {
                "modelIndex"
            } else {
                "safetensors"
            },
        ));
        if is_index {
            match fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())
                }) {
                Ok(value) if value.is_object() => candidate.index = Some((path, value)),
                Ok(_) => {
                    candidate.invalid_index = true;
                    candidate
                        .warnings
                        .push("model_index.json 顶层必须是对象".into());
                }
                Err(e) => {
                    candidate.invalid_index = true;
                    candidate
                        .warnings
                        .push(format!("model_index.json 解析失败：{e}"));
                }
            }
        }
    }
}

fn build_model(directory: PathBuf, mut candidate: Candidate) -> LocalModel {
    let safetensors_count = candidate
        .files
        .iter()
        .filter(|(_, _, kind)| *kind == "safetensors")
        .count();
    let has_model_index = candidate.index.is_some() || candidate.invalid_index;
    let mut identity = directory.to_string_lossy().to_ascii_lowercase();
    if let Some((_, value)) = &candidate.index {
        identity.push(' ');
        identity.push_str(&value.to_string().to_ascii_lowercase());
    }
    let model_type = if contains_fl2va(&identity) {
        H3ModelType::Fl2Va
    } else if contains_ref2va(&identity) {
        H3ModelType::Ref2Va
    } else {
        candidate
            .warnings
            .push("未从目录名或 model_index.json 识别出 FL2VA/Ref2VA 类型".into());
        H3ModelType::UnknownH3
    };

    if safetensors_count == 0 {
        candidate.warnings.push("缺少 .safetensors 权重文件".into());
    }
    if !has_model_index {
        candidate
            .warnings
            .push("缺少 model_index.json；可作为单文件权重使用，但结构信息不完整".into());
    }
    if candidate
        .files
        .iter()
        .any(|(_, size, kind)| *kind == "safetensors" && *size == 0)
    {
        candidate
            .warnings
            .push("存在大小为 0 的 safetensors 文件".into());
    }

    let invalid_weights = candidate
        .files
        .iter()
        .any(|(_, size, kind)| *kind == "safetensors" && *size == 0);
    let integrity = if candidate.invalid_index || invalid_weights {
        ModelIntegrity::Invalid
    } else if candidate.index.is_some() && safetensors_count > 0 {
        ModelIntegrity::Complete
    } else {
        ModelIntegrity::Partial
    };
    let total_size_bytes = candidate.files.iter().map(|(_, size, _)| size).sum();
    let mut files = candidate
        .files
        .into_iter()
        .map(|(path, size_bytes, kind)| ModelFile {
            path: path.to_string_lossy().into_owned(),
            size_bytes,
            kind: kind.into(),
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    LocalModel {
        display_name: directory
            .file_name()
            .unwrap_or(directory.as_os_str())
            .to_string_lossy()
            .into_owned(),
        directory: directory.to_string_lossy().into_owned(),
        model_type,
        integrity,
        total_size_bytes,
        has_model_index,
        safetensors_count,
        files,
        warnings: candidate.warnings,
    }
}

fn contains_fl2va(text: &str) -> bool {
    ["fl2va", "fl-2-va", "first.last", "first_last"]
        .iter()
        .any(|v| text.contains(v))
}

fn contains_ref2va(text: &str) -> bool {
    [
        "ref2va",
        "ref-2-va",
        "reference.to.video",
        "reference_to_video",
    ]
    .iter()
    .any(|v| text.contains(v))
}
