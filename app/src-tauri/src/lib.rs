use serde::Serialize;
use std::{path::PathBuf, process::Command, time::Duration};
use sysinfo::System;
use url::Url;

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
            validate_output_path
        ])
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
