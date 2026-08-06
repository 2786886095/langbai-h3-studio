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
        .launch_plan("python\\python.exe", "ComfyUI\\main.py")
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
        .launch_plan("python\\python.exe", "ComfyUI\\main.py")
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
        .invoke_handler(tauri::generate_handler![
            probe_system,
            probe_comfyui,
            validate_output_path,
            compile_workflow,
            download_model_file,
            model_store::scan_local_models,
            runtime_get_current,
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
            comfy_interrupt,
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
