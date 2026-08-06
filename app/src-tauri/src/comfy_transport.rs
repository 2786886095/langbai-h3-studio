//! ComfyUI 的最小 HTTP 传输层。
//!
//! 本模块刻意不把 ComfyUI 的节点 id 暴露给调用方；节点级错误会被折叠为
//! 可显示给用户的中文摘要。业务层仍应负责工作流编译与状态持久化。

use reqwest::{Client, Url, multipart};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

#[derive(Clone)]
pub struct ComfyTransport {
    base_url: Url,
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptReceipt {
    pub prompt_id: String,
    pub queue_number: Option<u64>,
    pub validation_error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UploadReceipt {
    pub name: String,
    pub subfolder: String,
    pub kind: String,
}

impl ComfyTransport {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, String> {
        let base_url = validate_loopback_url(base_url)?;
        let client = Client::builder()
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map_err(|e| format!("创建 ComfyUI 网络客户端失败：{e}"))?;
        Ok(Self { base_url, client })
    }

    pub async fn post_prompt(
        &self,
        workflow: Value,
        client_id: &str,
    ) -> Result<PromptReceipt, String> {
        let body = serde_json::json!({ "prompt": workflow, "client_id": client_id });
        let value = self
            .request_json(
                self.client.post(self.endpoint("prompt")?).json(&body),
                "提交生成任务",
            )
            .await?;
        let prompt_id = value
            .get("prompt_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "ComfyUI 返回异常：缺少任务编号".to_string())?
            .to_owned();
        let queue_number = value.get("number").and_then(Value::as_u64);
        let validation_error_count = value
            .get("node_errors")
            .and_then(Value::as_object)
            .map_or(0, |v| v.len());
        Ok(PromptReceipt {
            prompt_id,
            queue_number,
            validation_error_count,
        })
    }

    pub async fn get_queue(&self) -> Result<Value, String> {
        self.request_json(self.client.get(self.endpoint("queue")?), "读取任务队列")
            .await
    }

    pub async fn get_history(&self, prompt_id: &str) -> Result<Value, String> {
        if prompt_id.is_empty()
            || !prompt_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("任务编号格式无效".into());
        }
        self.request_json(
            self.client
                .get(self.endpoint(&format!("history/{prompt_id}"))?),
            "读取任务历史",
        )
        .await
    }

    pub async fn interrupt(&self) -> Result<(), String> {
        let response = self
            .client
            .post(self.endpoint("interrupt")?)
            .send()
            .await
            .map_err(|e| network_error("中断生成任务", e))?;
        ensure_success(response, "中断生成任务").await.map(|_| ())
    }

    pub async fn upload_input(
        &self,
        filename: &str,
        bytes: Vec<u8>,
        mime: &str,
        subfolder: Option<&str>,
        overwrite: bool,
    ) -> Result<UploadReceipt, String> {
        if filename.trim().is_empty() || filename.contains(['/', '\\']) {
            return Err("上传文件名无效".into());
        }
        let part = multipart::Part::bytes(bytes)
            .file_name(filename.to_owned())
            .mime_str(mime)
            .map_err(|_| "上传文件的媒体类型无效".to_string())?;
        let mut form = multipart::Form::new()
            .part("image", part)
            .text("overwrite", overwrite.to_string());
        if let Some(folder) = subfolder.filter(|v| !v.is_empty()) {
            if folder.contains("..") || folder.starts_with(['/', '\\']) {
                return Err("上传子目录无效".into());
            }
            form = form.text("subfolder", folder.to_owned());
        }
        let value = self
            .request_json(
                self.client
                    .post(self.endpoint("upload/image")?)
                    .multipart(form),
                "上传输入文件",
            )
            .await?;
        Ok(UploadReceipt {
            name: required_string(&value, "name", "文件名")?,
            subfolder: value
                .get("subfolder")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            kind: value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("input")
                .to_owned(),
        })
    }

    /// 从磁盘流式上传素材，避免将大型视频或音频完整载入内存。
    pub async fn upload_input_path(
        &self,
        path: &Path,
        mime: &str,
        subfolder: Option<&str>,
        overwrite: bool,
    ) -> Result<UploadReceipt, String> {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "素材文件名无效".to_string())?;
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| format!("读取素材失败：{e}"))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err("素材必须是非空文件".into());
        }
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| format!("打开素材失败：{e}"))?;
        let stream = tokio_util::io::ReaderStream::new(file);
        let part =
            multipart::Part::stream_with_length(reqwest::Body::wrap_stream(stream), metadata.len())
                .file_name(filename.to_owned())
                .mime_str(mime)
                .map_err(|_| "素材媒体类型无效".to_string())?;
        let mut form = multipart::Form::new()
            .part("image", part)
            .text("overwrite", overwrite.to_string());
        if let Some(folder) = subfolder.filter(|value| !value.is_empty()) {
            if folder.contains("..") || folder.starts_with(['/', '\\']) {
                return Err("上传子目录无效".into());
            }
            form = form.text("subfolder", folder.to_owned());
        }
        let value = self
            .request_json(
                self.client
                    .post(self.endpoint("upload/image")?)
                    .multipart(form),
                "上传输入素材",
            )
            .await?;
        Ok(UploadReceipt {
            name: required_string(&value, "name", "文件名")?,
            subfolder: value
                .get("subfolder")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            kind: value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("input")
                .to_owned(),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, String> {
        self.base_url
            .join(path)
            .map_err(|_| "生成 ComfyUI 请求地址失败".into())
    }

    async fn request_json(
        &self,
        request: reqwest::RequestBuilder,
        action: &str,
    ) -> Result<Value, String> {
        let response = request.send().await.map_err(|e| network_error(action, e))?;
        let response = ensure_success(response, action).await?;
        response
            .json::<Value>()
            .await
            .map_err(|e| format!("{action}失败：ComfyUI 返回了无效数据（{e}）"))
    }
}

pub fn validate_loopback_url(raw: &str) -> Result<Url, String> {
    let mut url = Url::parse(raw).map_err(|_| "ComfyUI 地址格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("ComfyUI 地址仅支持 HTTP 或 HTTPS".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("ComfyUI 地址不能包含账号、查询参数或片段".into());
    }
    let loopback = match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
        None => false,
    };
    if !loopback {
        return Err("出于本机运行安全考虑，ComfyUI 地址必须是回环地址".into());
    }
    if !matches!(url.path(), "" | "/") {
        return Err("ComfyUI 基础地址不能包含额外路径".into());
    }
    url.set_path("/");
    Ok(url)
}

fn required_string(value: &Value, key: &str, label: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("ComfyUI 返回异常：缺少{label}"))
}

async fn ensure_success(
    response: reqwest::Response,
    action: &str,
) -> Result<reqwest::Response, String> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    // 不透传可能含节点 id、路径或内部堆栈的服务端正文。
    let _ = response.bytes().await;
    Err(format!("{action}失败：ComfyUI 返回 HTTP {status}"))
}

fn network_error(action: &str, error: reqwest::Error) -> String {
    let detail = error.to_string().to_ascii_lowercase();
    if error.is_timeout()
        || error.is_request()
        || detail.contains("timed out")
        || detail.contains("deadline")
    {
        format!("{action}超时，请确认 ComfyUI 正在运行")
    } else if error.is_connect() {
        format!("{action}失败：无法连接本机 ComfyUI")
    } else {
        format!("{action}失败：网络请求异常（{error}）")
    }
}
