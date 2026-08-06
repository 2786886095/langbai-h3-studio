//! Official MiniMax Hailuo cloud API adapter, separate from local/open-source H3.
//! Secrets live in the OS credential vault and never appear in command results.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, Url, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const API_ORIGIN: &str = "https://api.minimax.io";
const SERVICE: &str = "io.langbai.h3-studio.minimax";
const ACCOUNT: &str = "hailuo-api-key";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HailuoVideoRequest {
    pub model: String,
    pub prompt: String,
    pub first_frame_image: Option<String>,
    pub last_frame_image: Option<String>,
    pub duration: u8,
    pub resolution: String,
    #[serde(default = "yes")]
    pub prompt_optimizer: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskResult {
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAsset {
    pub path: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStartInput {
    pub prompt: String,
    pub mode: String,
    pub model: String,
    pub resolution: String,
    pub duration_seconds: u8,
    #[serde(default)]
    pub assets: Vec<CloudAsset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudPollResult {
    pub task_id: String,
    pub status: String,
    pub progress: u8,
    pub file_id: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoTaskStatus {
    pub task_id: String,
    pub status: String,
    pub file_id: Option<String>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub error_message: Option<String>,
}
#[derive(Debug, Deserialize)]
struct BaseResponse {
    status_code: i64,
    status_msg: String,
}
#[derive(Debug, Deserialize)]
struct CreateResponse {
    task_id: Option<String>,
    base_resp: BaseResponse,
}
#[derive(Debug, Deserialize)]
struct QueryResponse {
    task_id: String,
    status: String,
    file_id: Option<String>,
    video_width: Option<u32>,
    video_height: Option<u32>,
    error_message: Option<String>,
    base_resp: BaseResponse,
}
#[derive(Debug, Deserialize)]
struct RetrieveResponse {
    file: RetrievedFile,
    base_resp: BaseResponse,
}
#[derive(Debug, Deserialize)]
struct RetrievedFile {
    download_url: String,
    filename: Option<String>,
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("初始化系统凭据存储失败：{e}"))
}
fn secret() -> Result<String, String> {
    let key = entry()?
        .get_password()
        .map_err(|_| "尚未保存 MiniMax API 密钥".to_string())?;
    if key.trim().is_empty() {
        Err("MiniMax API 密钥为空".into())
    } else {
        Ok(key)
    }
}
#[tauri::command]
pub fn minimax_set_api_key(api_key: String) -> Result<(), String> {
    let key = api_key.trim();
    if key.len() < 16 || key.chars().any(char::is_whitespace) {
        return Err("MiniMax API 密钥格式无效".into());
    }
    entry()?
        .set_password(key)
        .map_err(|e| format!("保存 MiniMax API 密钥失败：{e}"))
}
#[tauri::command]
pub fn minimax_has_api_key() -> Result<bool, String> {
    match entry()?.get_password() {
        Ok(v) => Ok(!v.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(format!("读取系统凭据状态失败：{e}")),
    }
}
#[tauri::command]
pub fn minimax_delete_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除 MiniMax API 密钥失败：{e}")),
    }
}

fn image_data_url(path: &str) -> Result<String, String> {
    let path = std::fs::canonicalize(path).map_err(|e| format!("读取云端参考图片失败：{e}"))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return Err("云端参考图片仅支持 JPG、PNG 或 WebP".into()),
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("读取云端参考图片失败：{e}"))?;
    if bytes.is_empty() || bytes.len() >= 20 * 1024 * 1024 {
        return Err("云端参考图片必须小于 20 MB 且内容不能为空".into());
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub fn minimax_api_key_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "configured": minimax_has_api_key()? }))
}

#[tauri::command]
pub fn minimax_api_key_set(api_key: String) -> Result<(), String> {
    minimax_set_api_key(api_key)
}

#[tauri::command]
pub fn minimax_api_key_delete() -> Result<(), String> {
    minimax_delete_api_key()
}

#[tauri::command]
pub async fn minimax_cloud_start(input: CloudStartInput) -> Result<CreateTaskResult, String> {
    if !matches!(
        input.mode.as_str(),
        "text" | "first_frame" | "first_last_frames"
    ) {
        return Err("云端模式字段无效".into());
    }
    let first = input
        .assets
        .first()
        .map(|asset| image_data_url(&asset.path))
        .transpose()?;
    let last = if input.mode == "first_last_frames" {
        input
            .assets
            .get(1)
            .map(|asset| image_data_url(&asset.path))
            .transpose()?
    } else {
        None
    };
    if input.mode != "text" && first.is_none() {
        return Err("云端图片生成视频需要首帧图片".into());
    }
    if input.mode == "first_last_frames" && last.is_none() {
        return Err("云端首尾帧生成需要两张图片".into());
    }
    let model = match input.model.as_str() {
        "Hailuo-2.3" => "MiniMax-Hailuo-2.3",
        "Hailuo-02" => "MiniMax-Hailuo-02",
        value => value,
    };
    minimax_create_video_task(HailuoVideoRequest {
        model: model.into(),
        prompt: input.prompt,
        first_frame_image: first,
        last_frame_image: last,
        duration: input.duration_seconds,
        resolution: input.resolution,
        prompt_optimizer: true,
    })
    .await
}

#[tauri::command]
pub async fn minimax_cloud_poll(task_id: String) -> Result<CloudPollResult, String> {
    let result = minimax_query_video_task(task_id).await?;
    let (status, progress) = match result.status.as_str() {
        "Preparing" | "Queueing" => ("queued", 10),
        "Processing" => ("running", 50),
        "Success" => ("completed", 100),
        "Fail" => ("failed", 100),
        _ => ("queued", 0),
    };
    Ok(CloudPollResult {
        task_id: result.task_id,
        status: status.into(),
        progress,
        file_id: result.file_id,
        error: result.error_message,
    })
}

#[tauri::command]
pub async fn minimax_cloud_save(
    file_id: String,
    output_directory: String,
) -> Result<String, String> {
    minimax_fetch_video(file_id, output_directory).await
}

fn client() -> Result<Client, String> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("创建 MiniMax 客户端失败：{e}"))
}
pub fn validate_api_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https"
        || url.host_str() != Some("api.minimax.io")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        Err("MiniMax API 地址必须是 https://api.minimax.io".into())
    } else {
        Ok(())
    }
}
pub fn validate_download_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|e| format!("视频下载地址无效：{e}"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        Err("视频下载地址必须是非本机、无凭据的 HTTPS URL".into())
    } else {
        Ok(url)
    }
}
pub fn validate_request(r: &HailuoVideoRequest) -> Result<(), String> {
    if !matches!(
        r.model.as_str(),
        "MiniMax-Hailuo-2.3" | "MiniMax-Hailuo-2.3-Fast" | "MiniMax-Hailuo-02"
    ) {
        return Err("云端仅支持 Hailuo 2.3/02；H3 是本地运行模型".into());
    }
    if r.prompt.trim().is_empty() || r.prompt.chars().count() > 2000 {
        return Err("提示词需为 1 至 2000 个字符".into());
    }
    if !matches!(r.duration, 6 | 10)
        || !matches!(r.resolution.as_str(), "768P" | "1080P")
        || (r.duration == 10 && r.resolution == "1080P")
    {
        return Err("视频时长或分辨率组合无效".into());
    }
    if r.last_frame_image.is_some() && r.model != "MiniMax-Hailuo-02" {
        return Err("首尾帧仅支持 MiniMax-Hailuo-02".into());
    }
    Ok(())
}
fn endpoint(path: &str) -> Result<Url, String> {
    let url = Url::parse(&format!("{API_ORIGIN}{path}")).map_err(|e| e.to_string())?;
    validate_api_url(&url)?;
    Ok(url)
}
fn base(b: BaseResponse) -> Result<(), String> {
    if b.status_code == 0 {
        Ok(())
    } else {
        Err(format!(
            "MiniMax API 错误 {}：{}",
            b.status_code, b.status_msg
        ))
    }
}

#[tauri::command]
pub async fn minimax_create_video_task(
    request: HailuoVideoRequest,
) -> Result<CreateTaskResult, String> {
    validate_request(&request)?;
    let r: CreateResponse = client()?
        .post(endpoint("/v1/video_generation")?)
        .bearer_auth(secret()?)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("创建 Hailuo 任务失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("MiniMax HTTP 错误：{e}"))?
        .json()
        .await
        .map_err(|e| format!("解析 MiniMax 响应失败：{e}"))?;
    base(r.base_resp)?;
    Ok(CreateTaskResult {
        task_id: r
            .task_id
            .filter(|v| !v.is_empty())
            .ok_or("MiniMax 未返回任务 ID")?,
    })
}
#[tauri::command]
pub async fn minimax_query_video_task(task_id: String) -> Result<VideoTaskStatus, String> {
    valid_id(&task_id, "任务")?;
    let r: QueryResponse = client()?
        .get(endpoint("/v1/query/video_generation")?)
        .bearer_auth(secret()?)
        .query(&[("task_id", &task_id)])
        .send()
        .await
        .map_err(|e| format!("查询 Hailuo 任务失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("MiniMax HTTP 错误：{e}"))?
        .json()
        .await
        .map_err(|e| format!("解析 MiniMax 响应失败：{e}"))?;
    base(r.base_resp)?;
    Ok(VideoTaskStatus {
        task_id: r.task_id,
        status: r.status,
        file_id: r.file_id,
        video_width: r.video_width,
        video_height: r.video_height,
        error_message: r.error_message,
    })
}
fn valid_id(id: &str, label: &str) -> Result<(), String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        Err(format!("MiniMax {label} ID 格式无效"))
    } else {
        Ok(())
    }
}
pub fn safe_output_path(dir: &Path, name: Option<&str>, file_id: &str) -> Result<PathBuf, String> {
    valid_id(file_id, "文件")?;
    let name = name
        .filter(|n| {
            Path::new(n).file_name().and_then(|v| v.to_str()) == Some(*n)
                && n.to_ascii_lowercase().ends_with(".mp4")
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("hailuo-{file_id}.mp4"));
    Ok(dir.join(name))
}
#[tauri::command]
pub async fn minimax_fetch_video(
    file_id: String,
    output_directory: String,
) -> Result<String, String> {
    valid_id(&file_id, "文件")?;
    let dir = PathBuf::from(output_directory);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("创建输出目录失败：{e}"))?;
    let m: RetrieveResponse = client()?
        .get(endpoint("/v1/files/retrieve")?)
        .bearer_auth(secret()?)
        .query(&[("file_id", &file_id)])
        .send()
        .await
        .map_err(|e| format!("获取文件信息失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("MiniMax HTTP 错误：{e}"))?
        .json()
        .await
        .map_err(|e| format!("解析文件信息失败：{e}"))?;
    base(m.base_resp)?;
    let url = validate_download_url(&m.file.download_url)?;
    let final_path = safe_output_path(&dir, m.file.filename.as_deref(), &file_id)?;
    let part = final_path.with_extension("mp4.part");
    let mut response = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载视频失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("视频下载 HTTP 错误：{e}"))?;
    let mut out = tokio::fs::File::create(&part)
        .await
        .map_err(|e| format!("创建视频文件失败：{e}"))?;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取视频失败：{e}"))?
    {
        out.write_all(&chunk)
            .await
            .map_err(|e| format!("写入视频失败：{e}"))?
    }
    out.flush()
        .await
        .map_err(|e| format!("刷新视频失败：{e}"))?;
    drop(out);
    tokio::fs::rename(&part, &final_path)
        .await
        .map_err(|e| format!("完成视频保存失败：{e}"))?;
    Ok(final_path.to_string_lossy().into_owned())
}
