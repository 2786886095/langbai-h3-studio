use serde::Serialize;
use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use sysinfo::System;
use tauri::Emitter;
use url::Url;

mod autodl_deploy;
mod autodl_remote;
mod benchmark;
mod comfy;
mod comfy_transport;
mod download;
mod h3_patch;
mod h3_workflow;
mod job_store;
mod managed_nodes;
mod minimax_api;
mod model_bundle;
mod model_store;
mod plugin_manager;
mod runtime_installer;
mod runtime_manager;
mod ssh_tunnel;
mod update_manager;
mod workflow_registry;

fn runtime_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("LangbaiH3Studio").join("runtime").join("comfy")
}

fn benchmark_root() -> PathBuf {
    runtime_root()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("benchmarks")
}

#[tauri::command]
fn benchmark_save(report: benchmark::CompatibilityReport) -> Result<String, String> {
    benchmark::save_report(&benchmark_root(), &report)
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
fn benchmark_list() -> Result<Vec<benchmark::CompatibilityReport>, String> {
    benchmark::list_reports(&benchmark_root())
}

#[tauri::command]
fn benchmark_export_anonymous(destination: String) -> Result<String, String> {
    let exported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间早于 UNIX 纪元".to_string())?
        .as_secs();
    benchmark::export_anonymous(&benchmark_root(), &PathBuf::from(destination), exported_at)
        .map(|path| path.to_string_lossy().into_owned())
}

const RUNTIME_NVIDIA_MANIFEST: &str =
    include_str!("../resources/runtime/manifests/comfyui-v0.30.0-nvidia.json");
const RUNTIME_CU126_MANIFEST: &str =
    include_str!("../resources/runtime/manifests/comfyui-v0.30.0-nvidia-cu126.json");
const H3_PREVIEW_PATCH_MANIFEST: &str =
    include_str!("../resources/runtime/manifests/comfyui-h3-preview-patch.json");

fn builtin_runtime_manifest(variant: &str) -> Result<runtime_installer::RuntimeManifest, String> {
    let source = match variant {
        "nvidia" => RUNTIME_NVIDIA_MANIFEST,
        "nvidia-cu126" => RUNTIME_CU126_MANIFEST,
        _ => return Err("未知的 Runtime 版本".into()),
    };
    serde_json::from_str(source).map_err(|e| format!("内置 Runtime 清单损坏：{e}"))
}

#[tauri::command]
fn model_bundles() -> Result<Vec<model_bundle::ModelBundle>, String> {
    model_bundle::builtins()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelAssociation {
    config_path: String,
    root: String,
    categories: std::collections::BTreeMap<String, Vec<String>>,
    file_count: usize,
}

fn h3_model_category(path: &std::path::Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if !name.ends_with(".safetensors") {
        return None;
    }
    if name.contains("qwen") || name.contains("text_encoder") {
        Some("text_encoders")
    } else if name.contains("vae") {
        Some("vae")
    } else if name.contains("minimax_h3") || name.contains("fl2va") || name.contains("ref2va") {
        Some("diffusion_models")
    } else {
        None
    }
}

#[tauri::command]
fn associate_local_h3_models(root: String) -> Result<ModelAssociation, String> {
    let scan = model_store::scan_model_directory(&root, Some(8))?;
    let canonical_root =
        std::fs::canonicalize(&scan.root).map_err(|e| format!("读取模型根目录失败：{e}"))?;
    let mut categories =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<PathBuf>>::new();
    let mut file_count = 0usize;
    for model in &scan.models {
        for file in &model.files {
            let path = PathBuf::from(&file.path);
            if let Some(category) = h3_model_category(&path) {
                let parent = path
                    .parent()
                    .ok_or_else(|| "模型文件缺少父目录".to_string())?;
                let canonical =
                    std::fs::canonicalize(parent).map_err(|e| format!("读取模型目录失败：{e}"))?;
                if !canonical.starts_with(&canonical_root) {
                    return Err("模型文件超出所选根目录".into());
                }
                categories
                    .entry(category.into())
                    .or_default()
                    .insert(canonical);
                file_count += 1;
            }
        }
    }
    if !categories.contains_key("diffusion_models") {
        return Err("没有找到 MiniMax-H3 diffusion model 权重".into());
    }
    let current = runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "请先安装托管 ComfyUI 运行环境".to_string())?;
    let config_path = current.profile_dir.join("extra_model_paths.yaml");
    let runtime_categories = categories
        .iter()
        .map(|(key, paths)| (key.clone(), paths.iter().cloned().collect::<Vec<_>>()))
        .collect::<std::collections::BTreeMap<_, _>>();
    runtime_manager::write_extra_model_paths(&config_path, &canonical_root, &runtime_categories)
        .map_err(|e| e.to_string())?;
    Ok(ModelAssociation {
        config_path: config_path.to_string_lossy().into_owned(),
        root: canonical_root.to_string_lossy().into_owned(),
        categories: categories
            .into_iter()
            .map(|(key, paths)| {
                (
                    key,
                    paths
                        .into_iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect(),
                )
            })
            .collect(),
        file_count,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledModelBundle {
    id: String,
    model_root: String,
    total_size: u64,
    files: Vec<String>,
}

#[tauri::command]
async fn download_h3_bundle(
    app: tauri::AppHandle,
    bundle_id: String,
    license_accepted: bool,
) -> Result<InstalledModelBundle, String> {
    if !license_accepted {
        return Err("下载前需要阅读并接受 MiniMax H3 Community License".into());
    }
    let bundle = model_bundle::select(&bundle_id)?;
    let current = runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "请先安装托管 ComfyUI 运行环境".to_string())?;
    let model_root = current
        .profile_dir
        .join("ComfyUI_windows_portable")
        .join("ComfyUI")
        .join("models");
    std::fs::create_dir_all(&model_root).map_err(|e| format!("创建模型目录失败：{e}"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 60 * 24))
        .build()
        .map_err(|e| format!("创建模型下载客户端失败：{e}"))?;
    let mut installed = Vec::new();
    for (index, file) in bundle.files.iter().enumerate() {
        let _=app.emit("model-bundle-file",serde_json::json!({"bundleId":bundle.id,"index":index,"count":bundle.files.len(),"relativePath":file.relative_path,"size":file.size}));
        let request = download::DownloadRequest {
            source_url: bundle.download_url(file),
            relative_path: std::path::PathBuf::from(&file.relative_path),
            expected_sha256: file.sha256.clone(),
        };
        let path = file.relative_path.clone();
        download::download_model(&client, &model_root, &request, |progress| {
            let _ = app.emit(
                "model-download-progress",
                serde_json::json!({"relativePath":path,"progress":progress}),
            );
        })
        .await?;
        installed.push(file.relative_path.clone());
    }
    let acceptance = runtime_root().join("license-acceptance.json");
    if let Some(parent) = acceptance.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&acceptance,serde_json::to_vec_pretty(&serde_json::json!({"license":bundle.license,"url":bundle.license_url,"accepted":true,"bundleId":bundle.id,"timestamp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()})).map_err(|e|e.to_string())?).map_err(|e|format!("保存许可确认失败：{e}"))?;
    let total_size = bundle.total_size();
    Ok(InstalledModelBundle {
        id: bundle.id,
        model_root: model_root.to_string_lossy().into_owned(),
        total_size,
        files: installed,
    })
}

#[tauri::command]
fn runtime_manifests() -> Result<Vec<runtime_installer::RuntimeManifest>, String> {
    Ok(vec![
        builtin_runtime_manifest("nvidia")?,
        builtin_runtime_manifest("nvidia-cu126")?,
    ])
}

#[tauri::command]
fn h3_preview_patch_manifest() -> Result<serde_json::Value, String> {
    serde_json::from_str(H3_PREVIEW_PATCH_MANIFEST)
        .map_err(|e| format!("内置 H3 补丁清单损坏：{e}"))
}

#[tauri::command]
fn build_h3_workflow(request: h3_workflow::H3WorkflowRequest) -> Result<serde_json::Value, String> {
    h3_workflow::build_h3_prompt(&request)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalH3Asset {
    path: String,
    mime: String,
    kind: h3_workflow::H3AssetKind,
    role: h3_workflow::H3AssetRole,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartH3GenerationInput {
    base_url: String,
    mode: h3_workflow::H3Mode,
    prompt: String,
    width: u32,
    height: u32,
    duration_seconds: f32,
    seed: u64,
    steps: u32,
    reference_image_size: String,
    output_directory: String,
    assets: Vec<LocalH3Asset>,
    #[serde(default)]
    acceleration: h3_workflow::H3Acceleration,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartedH3Generation {
    prompt_id: String,
    queue_number: Option<u64>,
    uploaded_assets: usize,
    filename_prefix: String,
    output_directory: String,
}

#[tauri::command]
async fn start_h3_generation(input: StartH3GenerationInput) -> Result<StartedH3Generation, String> {
    let output_directory = validate_output_path(input.output_directory)?;
    let transport =
        comfy_transport::ComfyTransport::new(&input.base_url, Duration::from_secs(60 * 60 * 2))?;
    let object_info = transport.get_object_info().await?;
    let available = object_info
        .as_object()
        .ok_or_else(|| "ComfyUI 节点能力响应格式无效".to_string())?;
    let job_token = format!(
        "{:x}{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::random::<u32>()
    );
    let subfolder = format!("langbai-h3/{job_token}");
    let mut uploaded = Vec::with_capacity(input.assets.len());
    for asset in input.assets {
        let canonical =
            std::fs::canonicalize(&asset.path).map_err(|e| format!("读取素材失败：{e}"))?;
        let (_, expected_mime) =
            asset_type(&canonical).ok_or_else(|| "素材格式不受支持".to_string())?;
        if asset.mime != expected_mime {
            return Err("素材类型与扩展名不一致".into());
        }
        let receipt = transport
            .upload_input_path(&canonical, expected_mime, Some(&subfolder), false)
            .await?;
        let remote_path = if receipt.subfolder.is_empty() {
            receipt.name
        } else {
            format!("{}/{}", receipt.subfolder.replace('\\', "/"), receipt.name)
        };
        uploaded.push(h3_workflow::UploadedAsset {
            remote_path,
            kind: asset.kind,
            role: asset.role,
        });
    }
    let filename_prefix = format!("video/Langbai_H3_{job_token}");
    let request = h3_workflow::H3WorkflowRequest {
        mode: input.mode,
        prompt: input.prompt,
        width: input.width,
        height: input.height,
        duration_seconds: input.duration_seconds,
        seed: input.seed,
        steps: input.steps,
        reference_image_size: input.reference_image_size,
        assets: uploaded,
        filename_prefix: filename_prefix.clone(),
        acceleration: input.acceleration,
    };
    let workflow = h3_workflow::build_h3_prompt(&request)?;
    let missing = workflow
        .as_object()
        .into_iter()
        .flat_map(|nodes| nodes.values())
        .filter_map(|node| node.get("class_type").and_then(serde_json::Value::as_str))
        .filter(|node_type| !available.contains_key(*node_type))
        .collect::<std::collections::BTreeSet<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "当前 ComfyUI 缺少工作流节点：{}",
            missing.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    let receipt = transport.post_prompt(workflow, &job_token).await?;
    if receipt.validation_error_count > 0 {
        return Err(format!(
            "ComfyUI 拒绝了工作流：{} 个节点参数未通过校验",
            receipt.validation_error_count
        ));
    }
    Ok(StartedH3Generation {
        prompt_id: receipt.prompt_id,
        queue_number: receipt.queue_number,
        uploaded_assets: request.assets.len(),
        filename_prefix,
        output_directory,
    })
}

#[tauri::command]
async fn runtime_install_h3_preview_patch(
    app: tauri::AppHandle,
) -> Result<h3_patch::PatchReceipt, String> {
    let value: serde_json::Value = serde_json::from_str(H3_PREVIEW_PATCH_MANIFEST)
        .map_err(|e| format!("内置 H3 补丁清单损坏：{e}"))?;
    let manifest: h3_patch::H3PatchManifest =
        serde_json::from_value(value.clone()).map_err(|e| format!("H3 补丁清单字段无效：{e}"))?;
    let url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "H3 补丁清单缺少下载地址".to_string())?;
    let current = runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "请先安装托管 ComfyUI 基础运行环境".to_string())?;
    let request = download::DownloadRequest {
        source_url: url.into(),
        relative_path: PathBuf::from("downloads").join(format!("{}.zip", manifest.id)),
        expected_sha256: manifest.sha256.clone(),
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 30))
        .build()
        .map_err(|e| format!("创建 H3 补丁下载客户端失败：{e}"))?;
    let archive = download::download_model(&client, &runtime_root(), &request, |progress| {
        let _ = app.emit("h3-patch-download-progress", progress);
    })
    .await?
    .path;
    let receipt = h3_patch::install_h3_patch(&manifest, &archive, &current.profile_dir)?;
    let portable = current.profile_dir.join("ComfyUI_windows_portable");
    let python = portable.join("python_embeded").join("python.exe");
    let requirements = portable.join("ComfyUI").join("requirements.txt");
    let output = Command::new(&python)
        .args(["-s", "-m", "pip", "install", "-r"])
        .arg(&requirements)
        .output()
        .map_err(|e| format!("启动 H3 依赖安装失败：{e}"));
    match output {
        Ok(result) if result.status.success() => Ok(receipt),
        Ok(result) => {
            let detail = String::from_utf8_lossy(&result.stderr);
            let _ = h3_patch::rollback_h3_patch(&receipt.receipt_path);
            Err(format!(
                "H3 依赖安装失败，源码补丁已回滚：{}",
                detail.chars().take(600).collect::<String>()
            ))
        }
        Err(error) => {
            let _ = h3_patch::rollback_h3_patch(&receipt.receipt_path);
            Err(error)
        }
    }
}

#[tauri::command]
fn runtime_rollback_h3_preview_patch() -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(H3_PREVIEW_PATCH_MANIFEST)
        .map_err(|e| format!("内置 H3 补丁清单损坏：{e}"))?;
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "H3 补丁清单缺少 ID".to_string())?;
    let current = runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "尚未安装托管 ComfyUI".to_string())?;
    h3_patch::rollback_h3_patch(
        &current
            .profile_dir
            .join(".h3-patches")
            .join(id)
            .join("receipt.json"),
    )
}

fn current_plugin_manager() -> Result<plugin_manager::PluginManager, String> {
    let current = runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "请先安装托管 ComfyUI 运行环境".to_string())?;
    Ok(plugin_manager::PluginManager::new(current.profile_dir))
}

async fn comfy_node_set(base_url: &str) -> Result<std::collections::BTreeSet<String>, String> {
    let info = comfy_transport::ComfyTransport::new(base_url, Duration::from_secs(20))?
        .get_object_info()
        .await?;
    info.as_object()
        .map(|value| value.keys().cloned().collect())
        .ok_or_else(|| "ComfyUI 节点能力响应格式无效".into())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginInspectionResult {
    package: plugin_manager::PackageInspection,
    compatibility: plugin_manager::CompatibilityReport,
}

#[tauri::command]
fn plugin_list() -> Result<plugin_manager::PluginLock, String> {
    current_plugin_manager()?
        .read_lock()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn plugin_inspect(path: String, base_url: String) -> Result<PluginInspectionResult, String> {
    let path = std::path::Path::new(&path);
    if path.extension().and_then(|value| value.to_str()) != Some("h3plugin") {
        return Err("请选择 .h3plugin 插件包".into());
    }
    let package = plugin_manager::inspect_package(path, None).map_err(|e| e.to_string())?;
    let manager = current_plugin_manager()?;
    let compatibility = manager
        .compatibility(
            &package,
            env!("CARGO_PKG_VERSION"),
            &comfy_node_set(&base_url).await?,
        )
        .map_err(|e| e.to_string())?;
    Ok(PluginInspectionResult {
        package,
        compatibility,
    })
}

#[tauri::command]
async fn plugin_install(
    path: String,
    expected_sha256: String,
    base_url: String,
) -> Result<plugin_manager::LockedPlugin, String> {
    let manager = current_plugin_manager()?;
    manager
        .install(
            std::path::Path::new(&path),
            Some(&expected_sha256),
            env!("CARGO_PKG_VERSION"),
            &comfy_node_set(&base_url).await?,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn plugin_set_enabled(id: String, enabled: bool) -> Result<(), String> {
    current_plugin_manager()?
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn plugin_uninstall(id: String) -> Result<(), String> {
    current_plugin_manager()?
        .uninstall(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn runtime_download_install_activate(
    app: tauri::AppHandle,
    variant: String,
) -> Result<runtime_manager::CurrentRuntime, String> {
    let manifest = builtin_runtime_manifest(&variant)?;
    let runtime_root = runtime_root();
    let archive_name = format!("{}.7z", manifest.version);
    let request = download::DownloadRequest {
        source_url: manifest.url.clone(),
        relative_path: std::path::PathBuf::from("downloads").join(&archive_name),
        expected_sha256: manifest.sha256.clone(),
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 60 * 12))
        .build()
        .map_err(|e| format!("创建 Runtime 下载客户端失败：{e}"))?;
    let downloaded = download::download_model(&client, &runtime_root, &request, |progress| {
        let _ = app.emit("runtime-download-progress", progress);
    })
    .await?;
    runtime_installer::install_local_archive(
        &manifest,
        &downloaded.path,
        &runtime_root,
        |progress| {
            let _ = app.emit("runtime-install-progress", progress);
        },
    )?;
    runtime_manager::RuntimeManager::new(runtime_root)
        .activate_staged(&manifest.version)
        .map_err(|e| e.to_string())
}

struct ManagedRuntimeProcess {
    child: Child,
    endpoint: String,
    started_at: u64,
}

#[derive(Default)]
struct ManagedRuntimeState(Mutex<Option<ManagedRuntimeProcess>>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedRuntimeStatus {
    running: bool,
    pid: Option<u32>,
    endpoint: Option<String>,
    started_at: Option<u64>,
    exit_code: Option<i32>,
}

#[tauri::command]
fn runtime_start(
    state: tauri::State<'_, ManagedRuntimeState>,
    memory_profile: Option<runtime_manager::MemoryProfile>,
) -> Result<ManagedRuntimeStatus, String> {
    let manager = runtime_manager::RuntimeManager::new(runtime_root());
    let mut plan = manager
        .launch_plan_with_memory_profile(
            "ComfyUI_windows_portable\\python_embeded\\python.exe",
            "ComfyUI_windows_portable\\ComfyUI\\main.py",
            memory_profile.unwrap_or(runtime_manager::MemoryProfile::Auto),
        )
        .map_err(|e| e.to_string())?;
    let extra_models = manager
        .current()
        .map_err(|e| e.to_string())?
        .map(|current| current.profile_dir.join("extra_model_paths.yaml"));
    if let Some(config) = extra_models.filter(|path| path.is_file()) {
        plan.args.push("--extra-model-paths-config".into());
        plan.args.push(config.to_string_lossy().into_owned());
    }
    if !plan.program.is_file() {
        return Err("托管 Runtime 缺少 Python，可在运行环境页面执行修复".into());
    }
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "Runtime 状态锁异常".to_string())?;
    if let Some(process) = slot.as_mut() {
        if process
            .child
            .try_wait()
            .map_err(|e| format!("读取 Runtime 状态失败：{e}"))?
            .is_none()
        {
            return Ok(ManagedRuntimeStatus {
                running: true,
                pid: Some(process.child.id()),
                endpoint: Some(process.endpoint.clone()),
                started_at: Some(process.started_at),
                exit_code: None,
            });
        }
    }
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .current_dir(&plan.working_dir)
        .envs(&plan.environment);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("启动托管 ComfyUI 失败：{e}"))?;
    let pid = child.id();
    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    *slot = Some(ManagedRuntimeProcess {
        child,
        endpoint: plan.endpoint.clone(),
        started_at,
    });
    Ok(ManagedRuntimeStatus {
        running: true,
        pid: Some(pid),
        endpoint: Some(plan.endpoint),
        started_at: Some(started_at),
        exit_code: None,
    })
}

#[tauri::command]
fn runtime_status(
    state: tauri::State<'_, ManagedRuntimeState>,
) -> Result<ManagedRuntimeStatus, String> {
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "Runtime 状态锁异常".to_string())?;
    let Some(process) = slot.as_mut() else {
        return Ok(ManagedRuntimeStatus {
            running: false,
            pid: None,
            endpoint: None,
            started_at: None,
            exit_code: None,
        });
    };
    match process
        .child
        .try_wait()
        .map_err(|e| format!("读取 Runtime 状态失败：{e}"))?
    {
        None => Ok(ManagedRuntimeStatus {
            running: true,
            pid: Some(process.child.id()),
            endpoint: Some(process.endpoint.clone()),
            started_at: Some(process.started_at),
            exit_code: None,
        }),
        Some(status) => {
            let result = ManagedRuntimeStatus {
                running: false,
                pid: Some(process.child.id()),
                endpoint: Some(process.endpoint.clone()),
                started_at: Some(process.started_at),
                exit_code: status.code(),
            };
            *slot = None;
            Ok(result)
        }
    }
}

#[tauri::command]
fn runtime_stop(
    state: tauri::State<'_, ManagedRuntimeState>,
) -> Result<ManagedRuntimeStatus, String> {
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "Runtime 状态锁异常".to_string())?;
    let Some(mut process) = slot.take() else {
        return Ok(ManagedRuntimeStatus {
            running: false,
            pid: None,
            endpoint: None,
            started_at: None,
            exit_code: None,
        });
    };
    let pid = process.child.id();
    process
        .child
        .kill()
        .map_err(|e| format!("停止托管 ComfyUI 失败：{e}"))?;
    let status = process
        .child
        .wait()
        .map_err(|e| format!("等待 Runtime 退出失败：{e}"))?;
    Ok(ManagedRuntimeStatus {
        running: false,
        pid: Some(pid),
        endpoint: Some(process.endpoint),
        started_at: Some(process.started_at),
        exit_code: status.code(),
    })
}

#[tauri::command]
fn runtime_get_current() -> Result<Option<runtime_manager::CurrentRuntime>, String> {
    runtime_manager::RuntimeManager::new(runtime_root())
        .current()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn runtime_prepare_staging(version: String) -> Result<String, String> {
    runtime_manager::RuntimeManager::new(runtime_root())
        .prepare_staging(&version)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn runtime_activate_staged(version: String) -> Result<runtime_manager::CurrentRuntime, String> {
    runtime_manager::RuntimeManager::new(runtime_root())
        .activate_staged(&version)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn runtime_launch_plan() -> Result<runtime_manager::LaunchPlan, String> {
    runtime_manager::RuntimeManager::new(runtime_root())
        .launch_plan(
            "ComfyUI_windows_portable\\python_embeded\\python.exe",
            "ComfyUI_windows_portable\\ComfyUI\\main.py",
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn runtime_install_archive(
    app: tauri::AppHandle,
    manifest: runtime_installer::RuntimeManifest,
    archive_path: String,
) -> Result<runtime_installer::InstalledRuntime, String> {
    runtime_installer::install_local_archive(
        &manifest,
        std::path::Path::new(&archive_path),
        &runtime_root(),
        |progress| {
            let _ = app.emit("runtime-install-progress", progress);
        },
    )
}

#[tauri::command]
fn workflow_list() -> Result<serde_json::Value, String> {
    workflow_registry::verify_official_reference_workflows().map_err(|e| e.to_string())?;
    serde_json::to_value(serde_json::json!({
        "adapters": workflow_registry::registered_workflows(),
        "officialReferences": workflow_registry::official_reference_workflows()
    }))
    .map_err(|e| format!("序列化工作流清单失败：{e}"))
}

#[tauri::command]
fn workflow_capabilities(
    mode: workflow_registry::WorkflowMode,
    reachable: bool,
    node_types: Vec<String>,
) -> Result<workflow_registry::CapabilityReport, String> {
    let descriptor = workflow_registry::select_workflow(mode).map_err(|e| e.to_string())?;
    let probe = comfy::ProbeResult {
        reachable,
        node_types: node_types.into_iter().collect(),
    };
    descriptor
        .capability_report(&probe)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_check_manifest(
    current_version: String,
    manifest_json: String,
    include_prerelease: bool,
) -> Result<Option<update_manager::Release>, String> {
    let releases = update_manager::parse_releases(&manifest_json).map_err(|e| format!("{e:?}"))?;
    let channel = if include_prerelease {
        update_manager::UpdateChannel::PreRelease
    } else {
        update_manager::UpdateChannel::Stable
    };
    update_manager::select_update(&current_version, &releases, channel)
        .map(|value| value.cloned())
        .map_err(|e| format!("{e:?}"))
}

#[tauri::command]
fn update_verify_file(path: String, sha256: String) -> Result<(), String> {
    update_manager::verify_sha256(std::path::Path::new(&path), &sha256)
        .map_err(|e| format!("更新包校验失败：{e:?}"))
}

fn validate_github_download_url(raw: &str) -> Result<(), String> {
    let url = Url::parse(raw).map_err(|_| "更新下载地址格式无效".to_string())?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if url.scheme() != "https"
        || !(host == "github.com"
            || host.ends_with(".githubusercontent.com")
            || host.ends_with(".github-releases.githubusercontent.com"))
    {
        return Err("更新包只能从 GitHub HTTPS 地址下载".into());
    }
    Ok(())
}

#[tauri::command]
async fn update_check_github(
    include_pre_release: bool,
) -> Result<Option<update_manager::UpdateCandidate>, String> {
    let releases = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Langbai-H3-Studio-Updater")
        .build()
        .map_err(|e| format!("创建更新客户端失败：{e}"))?
        .get("https://api.github.com/repos/2786886095/langbai-h3-studio/releases?per_page=20")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("检查 GitHub 更新失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("检查 GitHub 更新失败：{e}"))?
        .text()
        .await
        .map_err(|e| format!("读取 GitHub 更新失败：{e}"))?;
    let channel = if include_pre_release {
        update_manager::UpdateChannel::PreRelease
    } else {
        update_manager::UpdateChannel::Stable
    };
    update_manager::select_windows_candidate(env!("CARGO_PKG_VERSION"), &releases, channel)
        .map_err(|e| format!("解析更新信息失败：{e}"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadedUpdate {
    version: String,
    installer_path: String,
    sha256: String,
}

#[tauri::command]
async fn update_download_candidate(
    app: tauri::AppHandle,
    candidate: update_manager::UpdateCandidate,
) -> Result<DownloadedUpdate, String> {
    validate_github_download_url(&candidate.download_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 60))
        .user_agent("Langbai-H3-Studio-Updater")
        .build()
        .map_err(|e| format!("创建更新下载客户端失败：{e}"))?;
    let sha256 = if let Some(value) = candidate.sha256.clone() {
        value
    } else {
        let url = candidate
            .sha256_url
            .as_deref()
            .ok_or_else(|| "更新缺少 SHA-256 校验文件".to_string())?;
        validate_github_download_url(url)?;
        let text = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("下载更新校验文件失败：{e}"))?
            .error_for_status()
            .map_err(|e| format!("下载更新校验文件失败：{e}"))?
            .text()
            .await
            .map_err(|e| format!("读取更新校验文件失败：{e}"))?;
        text.split_whitespace()
            .find(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| "更新校验文件中没有有效 SHA-256".to_string())?
            .to_ascii_lowercase()
    };
    let request = download::DownloadRequest {
        source_url: candidate.download_url.clone(),
        relative_path: PathBuf::from("updates").join(&candidate.file_name),
        expected_sha256: sha256.clone(),
    };
    let result = download::download_model(&client, &runtime_root(), &request, |progress| {
        let _ = app.emit("update-download-progress", progress);
    })
    .await?;
    Ok(DownloadedUpdate {
        version: candidate.version,
        installer_path: result.path.to_string_lossy().into_owned(),
        sha256,
    })
}

#[tauri::command]
fn update_launch_installer(app: tauri::AppHandle, installer_path: String) -> Result<(), String> {
    let path =
        std::fs::canonicalize(installer_path).map_err(|e| format!("读取更新安装包失败：{e}"))?;
    let allowed = std::fs::canonicalize(runtime_root().join("updates"))
        .map_err(|e| format!("读取更新目录失败：{e}"))?;
    if !path.starts_with(&allowed)
        || path.extension().and_then(|value| value.to_str()) != Some("exe")
    {
        return Err("更新安装包路径无效".into());
    }
    Command::new(&path)
        .spawn()
        .map_err(|e| format!("启动更新安装包失败：{e}"))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn comfy_submit_prompt(
    base_url: String,
    workflow: serde_json::Value,
    client_id: String,
) -> Result<comfy_transport::PromptReceipt, String> {
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(30))?
        .post_prompt(workflow, &client_id)
        .await
}

#[tauri::command]
async fn comfy_get_queue(base_url: String) -> Result<serde_json::Value, String> {
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(15))?
        .get_queue()
        .await
}

#[tauri::command]
async fn comfy_get_history(
    base_url: String,
    prompt_id: String,
) -> Result<serde_json::Value, String> {
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(15))?
        .get_history(&prompt_id)
        .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationPoll {
    status: String,
    prompt_id: String,
    queue_position: Option<usize>,
    outputs: Vec<comfy::OutputAsset>,
    error: Option<String>,
}

#[tauri::command]
async fn comfy_poll_generation(
    base_url: String,
    prompt_id: String,
) -> Result<GenerationPoll, String> {
    let transport = comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(15))?;
    let history = transport.get_history(&prompt_id).await?;
    if let Some(entry) = comfy::parse_history(&history, &prompt_id).map_err(|e| e.to_string())? {
        let status = if entry.error.is_some() {
            "failed"
        } else if entry.completed {
            "completed"
        } else {
            "running"
        };
        return Ok(GenerationPoll {
            status: status.into(),
            prompt_id,
            queue_position: None,
            outputs: entry.outputs,
            error: entry.error,
        });
    }
    let queue = transport.get_queue().await?;
    let snapshot = comfy::QueueSnapshot::parse(&queue).map_err(|e| e.to_string())?;
    if snapshot
        .running
        .iter()
        .any(|item| item.prompt_id == prompt_id)
    {
        return Ok(GenerationPoll {
            status: "running".into(),
            prompt_id,
            queue_position: None,
            outputs: Vec::new(),
            error: None,
        });
    }
    let queue_position = snapshot
        .pending
        .iter()
        .position(|item| item.prompt_id == prompt_id)
        .map(|index| index + 1);
    Ok(GenerationPoll {
        status: if queue_position.is_some() {
            "queued"
        } else {
            "unknown"
        }
        .into(),
        prompt_id,
        queue_position,
        outputs: Vec::new(),
        error: None,
    })
}

#[tauri::command]
async fn comfy_save_output(
    base_url: String,
    asset: comfy::OutputAsset,
    output_directory: String,
) -> Result<String, String> {
    let output_directory = validate_output_path(output_directory)?;
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(60 * 60))?
        .download_output(
            &asset.filename,
            &asset.subfolder,
            "output",
            std::path::Path::new(&output_directory),
        )
        .await
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn comfy_interrupt(base_url: String) -> Result<(), String> {
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(15))?
        .interrupt()
        .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompiledWorkflow {
    plan: comfy::ExecutionPlan,
    prompt_request: comfy::PromptRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAssetMeta {
    path: String,
    name: String,
    size: u64,
    mime: String,
    kind: String,
}

fn asset_type(path: &std::path::Path) -> Option<(&'static str, &'static str)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => ("image", "image/png"),
        "jpg" | "jpeg" => ("image", "image/jpeg"),
        "webp" => ("image", "image/webp"),
        "mp4" => ("video", "video/mp4"),
        "webm" => ("video", "video/webm"),
        "mov" => ("video", "video/quicktime"),
        "wav" => ("audio", "audio/wav"),
        "mp3" => ("audio", "audio/mpeg"),
        "flac" => ("audio", "audio/flac"),
        "m4a" => ("audio", "audio/mp4"),
        _ => return None,
    })
}

#[cfg(test)]
mod local_asset_tests {
    use super::{asset_type, h3_model_category};
    use std::path::Path;

    #[test]
    fn recognizes_supported_media_case_insensitively() {
        assert_eq!(
            asset_type(Path::new("frame.PNG")),
            Some(("image", "image/png"))
        );
        assert_eq!(
            asset_type(Path::new("clip.MP4")),
            Some(("video", "video/mp4"))
        );
        assert_eq!(
            asset_type(Path::new("voice.FLAC")),
            Some(("audio", "audio/flac"))
        );
    }

    #[test]
    fn rejects_unknown_or_missing_extensions() {
        assert_eq!(asset_type(Path::new("payload.exe")), None);
        assert_eq!(asset_type(Path::new("README")), None);
    }

    #[test]
    fn classifies_official_h3_model_components() {
        assert_eq!(
            h3_model_category(Path::new(
                "minimax_h3_fl2va_pruned_int8_convrot.safetensors"
            )),
            Some("diffusion_models")
        );
        assert_eq!(
            h3_model_category(Path::new("qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors")),
            Some("text_encoders")
        );
        assert_eq!(
            h3_model_category(Path::new("minimax_h3_video_vae_fp16.safetensors")),
            Some("vae")
        );
        assert_eq!(h3_model_category(Path::new("unrelated.safetensors")), None);
    }
}

#[tauri::command]
fn inspect_input_files(paths: Vec<String>) -> Result<Vec<LocalAssetMeta>, String> {
    if paths.is_empty() || paths.len() > 12 {
        return Err("一次请选择 1–12 个素材文件".into());
    }
    paths
        .into_iter()
        .map(|raw| {
            let path = std::fs::canonicalize(&raw).map_err(|e| format!("读取素材失败：{e}"))?;
            let metadata =
                std::fs::metadata(&path).map_err(|e| format!("读取素材信息失败：{e}"))?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err("素材必须是非空文件".into());
            }
            let (kind, mime) = asset_type(&path)
                .ok_or_else(|| "仅支持 PNG/JPG/WebP、MP4/WebM/MOV、WAV/MP3/FLAC/M4A".to_string())?;
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "素材文件名必须是有效 Unicode".to_string())?
                .to_owned();
            Ok(LocalAssetMeta {
                path: path.to_string_lossy().into_owned(),
                name,
                size: metadata.len(),
                mime: mime.into(),
                kind: kind.into(),
            })
        })
        .collect()
}

#[tauri::command]
async fn comfy_upload_input(
    base_url: String,
    path: String,
    mime: String,
    subfolder: String,
) -> Result<comfy_transport::UploadReceipt, String> {
    let canonical = std::fs::canonicalize(&path).map_err(|e| format!("读取素材失败：{e}"))?;
    let (_, expected_mime) =
        asset_type(&canonical).ok_or_else(|| "素材格式不受支持".to_string())?;
    if mime != expected_mime {
        return Err("素材类型与扩展名不一致".into());
    }
    comfy_transport::ComfyTransport::new(&base_url, Duration::from_secs(60 * 60 * 2))?
        .upload_input_path(&canonical, expected_mime, Some(&subfolder), false)
        .await
}

#[tauri::command]
fn compile_workflow(
    template_json: String,
    request: comfy::GenerateRequest,
    probe: comfy::ProbeResult,
    client_id: String,
) -> Result<CompiledWorkflow, String> {
    let template = comfy::WorkflowTemplate::from_json(&template_json).map_err(|e| e.to_string())?;
    let plan = template
        .build_plan(&request, &probe)
        .map_err(|e| e.to_string())?;
    let prompt_request = plan.prompt_body(client_id);
    Ok(CompiledWorkflow {
        plan,
        prompt_request,
    })
}

#[tauri::command]
async fn download_model_file(
    app: tauri::AppHandle,
    model_root: String,
    request: download::DownloadRequest,
) -> Result<download::DownloadResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 30))
        .build()
        .map_err(|e| format!("创建下载客户端失败：{e}"))?;
    download::download_model(
        &client,
        std::path::Path::new(&model_root),
        &request,
        |progress| {
            let _ = app.emit("model-download-progress", progress);
        },
    )
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuInfo {
    name: String,
    driver_version: String,
    memory_total_mb: u64,
    memory_used_mb: u64,
    temperature_c: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemProbe {
    os_name: String,
    os_version: String,
    cpu_name: String,
    cpu_threads: usize,
    memory_total_mb: u64,
    memory_used_mb: u64,
    gpu: Option<GpuInfo>,
    cuda_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComfyProbe {
    reachable: bool,
    base_url: String,
    node_count: usize,
    h3_related_nodes: Vec<String>,
    latency_ms: u128,
    message: String,
}

fn run_nvidia_smi() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,memory.total,memory.used,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let row = String::from_utf8_lossy(&output.stdout);
    let values: Vec<_> = row.lines().next()?.split(',').map(str::trim).collect();
    if values.len() < 5 {
        return None;
    }
    Some(GpuInfo {
        name: values[0].to_string(),
        driver_version: values[1].to_string(),
        memory_total_mb: values[2].parse().ok()?,
        memory_used_mb: values[3].parse().unwrap_or(0),
        temperature_c: values[4].parse().ok(),
    })
}

#[tauri::command]
fn probe_system() -> SystemProbe {
    let mut system = System::new_all();
    system.refresh_all();
    let gpu = run_nvidia_smi();
    SystemProbe {
        os_name: System::name().unwrap_or_else(|| "Windows".into()),
        os_version: System::os_version().unwrap_or_default(),
        cpu_name: system
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default(),
        cpu_threads: system.cpus().len(),
        memory_total_mb: system.total_memory() / 1024 / 1024,
        memory_used_mb: system.used_memory() / 1024 / 1024,
        cuda_available: gpu.is_some(),
        gpu,
    }
}

fn normalize_loopback_url(input: &str) -> Result<Url, String> {
    let mut url = Url::parse(input).map_err(|_| "请输入有效的 ComfyUI 地址".to_string())?;
    let host = url.host_str().unwrap_or_default();
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err("首版仅允许探测本机 ComfyUI 地址".into());
    }
    url.set_path("/object_info");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

#[tauri::command]
async fn probe_comfyui(base_url: String) -> Result<ComfyProbe, String> {
    let endpoint = normalize_loopback_url(&base_url)?;
    let started = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|e| format!("创建连接失败：{e}"))?;
    let response = client
        .get(endpoint)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("ComfyUI 返回 HTTP {}", response.status()));
    }
    let object_info: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "节点信息格式无效".to_string())?;
    let nodes = object_info
        .as_object()
        .ok_or_else(|| "节点信息不是对象".to_string())?;
    let h3_related_nodes = nodes
        .keys()
        .filter(|name| {
            let n = name.to_ascii_lowercase();
            n.contains("minimax") || n.contains("h3")
        })
        .cloned()
        .collect();
    Ok(ComfyProbe {
        reachable: true,
        base_url,
        node_count: nodes.len(),
        h3_related_nodes,
        latency_ms: started.elapsed().as_millis(),
        message: "ComfyUI 已连接".into(),
    })
}

#[tauri::command]
fn validate_output_path(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return Err("请选择保存路径".into());
    }
    std::fs::create_dir_all(&path).map_err(|e| format!("创建目录失败：{e}"))?;
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("读取目录失败：{e}"))?;
    let probe = canonical.join(".langbai-write-test");
    std::fs::write(&probe, b"ok").map_err(|e| format!("目录不可写：{e}"))?;
    let _ = std::fs::remove_file(probe);
    Ok(canonical.to_string_lossy().to_string())
}

#[tauri::command]
fn default_output_path() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|e| {
        format!(
            "\u{8bfb}\u{53d6}\u{8f6f}\u{4ef6}\u{6839}\u{76ee}\u{5f55}\u{5931}\u{8d25}\u{ff1a}{e}"
        )
    })?;
    let output = output_path_for_executable(&executable)?;
    validate_output_path(output.to_string_lossy().to_string())
}

fn output_path_for_executable(executable: &std::path::Path) -> Result<PathBuf, String> {
    executable
        .parent()
        .map(|root| root.join("output"))
        .ok_or_else(|| "\u{8f6f}\u{4ef6}\u{6839}\u{76ee}\u{5f55}\u{65e0}\u{6548}".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            probe_system,
            benchmark_save,
            benchmark_list,
            benchmark_export_anonymous,
            probe_comfyui,
            validate_output_path,
            default_output_path,
            compile_workflow,
            download_model_file,
            model_store::scan_local_models,
            associate_local_h3_models,
            model_bundles,
            download_h3_bundle,
            runtime_get_current,
            runtime_manifests,
            h3_preview_patch_manifest,
            build_h3_workflow,
            start_h3_generation,
            runtime_install_h3_preview_patch,
            runtime_rollback_h3_preview_patch,
            plugin_list,
            plugin_inspect,
            plugin_install,
            plugin_set_enabled,
            plugin_uninstall,
            runtime_download_install_activate,
            runtime_prepare_staging,
            runtime_activate_staged,
            runtime_launch_plan,
            runtime_install_archive,
            workflow_list,
            workflow_capabilities,
            update_check_manifest,
            update_verify_file,
            update_check_github,
            update_download_candidate,
            update_launch_installer,
            runtime_start,
            runtime_status,
            runtime_stop,
            ssh_tunnel::ssh_tunnel_start,
            ssh_tunnel::ssh_tunnel_status,
            ssh_tunnel::ssh_tunnel_stop,
            autodl_remote::autodl_remote_probe,
            autodl_deploy::autodl_deploy_preflight,
            autodl_deploy::autodl_deploy_prepare,
            autodl_deploy::autodl_deploy_status,
            autodl_deploy::autodl_deploy_rollback,
            autodl_deploy::autodl_model_download_start,
            autodl_deploy::autodl_model_download_cancel,
            comfy_submit_prompt,
            comfy_get_queue,
            comfy_get_history,
            comfy_poll_generation,
            comfy_save_output,
            comfy_interrupt,
            inspect_input_files,
            comfy_upload_input,
            job_store::create_job,
            job_store::list_jobs,
            job_store::update_job,
            minimax_api::minimax_set_api_key,
            minimax_api::minimax_has_api_key,
            minimax_api::minimax_delete_api_key,
            minimax_api::minimax_create_video_task,
            minimax_api::minimax_query_video_task,
            minimax_api::minimax_fetch_video,
            minimax_api::minimax_api_key_status,
            minimax_api::minimax_api_key_set,
            minimax_api::minimax_api_key_delete,
            minimax_api::minimax_cloud_start,
            minimax_api::minimax_cloud_poll,
            minimax_api::minimax_cloud_save,
            managed_nodes::managed_nodes_catalog,
            managed_nodes::managed_nodes_status,
            managed_nodes::managed_nodes_install,
            managed_nodes::managed_nodes_uninstall
        ])
        .manage(ManagedRuntimeState::default())
        .manage(ssh_tunnel::SshTunnelState::default())
        .run(tauri::generate_context!())
        .expect("启动 Langbai H3 Studio 失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_remote_comfyui_hosts() {
        assert!(normalize_loopback_url("http://example.com:8188").is_err());
    }

    #[test]
    fn normalizes_local_comfyui_endpoint() {
        let url = normalize_loopback_url("http://127.0.0.1:8188/").unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8188/object_info");
    }

    #[test]
    fn default_output_is_beside_the_executable() {
        let path = output_path_for_executable(std::path::Path::new(r"C:\Apps\Langbai\studio.exe"))
            .unwrap();
        assert_eq!(path, std::path::PathBuf::from(r"C:\Apps\Langbai\output"));
    }
}
