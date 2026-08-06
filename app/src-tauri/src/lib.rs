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

mod comfy;
mod comfy_transport;
mod download;
mod job_store;
mod model_bundle;
mod model_store;
mod runtime_installer;
mod runtime_manager;
mod update_manager;
mod workflow_registry;

fn runtime_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("LangbaiH3Studio").join("runtime").join("comfy")
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
) -> Result<ManagedRuntimeStatus, String> {
    let manager = runtime_manager::RuntimeManager::new(runtime_root());
    let plan = manager
        .launch_plan(
            "ComfyUI_windows_portable\\python_embeded\\python.exe",
            "ComfyUI_windows_portable\\ComfyUI\\main.py",
        )
        .map_err(|e| e.to_string())?;
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
    use super::asset_type;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            probe_system,
            probe_comfyui,
            validate_output_path,
            compile_workflow,
            download_model_file,
            model_store::scan_local_models,
            model_bundles,
            download_h3_bundle,
            runtime_get_current,
            runtime_manifests,
            h3_preview_patch_manifest,
            runtime_download_install_activate,
            runtime_prepare_staging,
            runtime_activate_staged,
            runtime_launch_plan,
            runtime_install_archive,
            workflow_list,
            workflow_capabilities,
            update_check_manifest,
            update_verify_file,
            runtime_start,
            runtime_status,
            runtime_stop,
            comfy_submit_prompt,
            comfy_get_queue,
            comfy_get_history,
            comfy_poll_generation,
            comfy_interrupt,
            inspect_input_files,
            comfy_upload_input,
            job_store::create_job,
            job_store::list_jobs,
            job_store::update_job
        ])
        .manage(ManagedRuntimeState::default())
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
}
