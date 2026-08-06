//! ComfyUI 的语义适配层。
//!
//! UI 只需要理解 [`GenerateRequest`]、[`ExecutionPlan`] 与 [`ExecutionEvent`]。
//! 工作流中的节点编号只存在于模板绑定和提交载荷中，不会出现在可序列化的 UI 类型里。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, path::Path};

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    Io(String),
    InvalidTemplate(String),
    MissingCapability(Vec<String>),
    MissingValue(String),
    InvalidResponse(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message)
            | Self::InvalidTemplate(message)
            | Self::MissingValue(message)
            | Self::InvalidResponse(message) => f.write_str(message),
            Self::MissingCapability(nodes) => write!(f, "缺少工作流所需节点：{}", nodes.join("、")),
        }
    }
}

impl std::error::Error for AdapterError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub fps: f32,
    pub steps: u32,
    pub guidance: f32,
    pub seed: i64,
    pub model: String,
    pub output_directory: String,
    #[serde(default)]
    pub acceleration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Asset {
    Image { path: String, role: AssetRole },
    Video { path: String, role: AssetRole },
    Audio { path: String, role: AssetRole },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetRole {
    StartFrame,
    EndFrame,
    Reference,
    MotionReference,
    AudioReference,
}

/// 可安全返回 UI 的执行摘要，不含 ComfyUI 节点编号。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlan {
    pub workflow_id: String,
    pub workflow_version: u32,
    pub title: String,
    pub required_node_types: Vec<String>,
    pub asset_count: usize,
    pub estimated_frames: u32,
    pub output_directory: String,
    pub acceleration: Option<String>,
    #[serde(skip)]
    prompt: Map<String, Value>,
}

impl ExecutionPlan {
    /// 构造 ComfyUI `POST /prompt` 的 JSON；该值只应由后端 HTTP 客户端使用。
    pub fn prompt_body(&self, client_id: impl Into<String>) -> PromptRequest {
        PromptRequest {
            client_id: client_id.into(),
            prompt: Value::Object(self.prompt.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptRequest {
    pub prompt: Value,
    pub client_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplate {
    pub id: String,
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub required_node_types: Vec<String>,
    pub workflow: Map<String, Value>,
    #[serde(default)]
    pub bindings: Vec<RoleBinding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoleBinding {
    pub role: ParameterRole,
    /// 内部模板字段；不要将 `WorkflowTemplate` 返回给 UI。
    pub node: String,
    pub input: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterRole {
    Prompt,
    NegativePrompt,
    Width,
    Height,
    Frames,
    Fps,
    Steps,
    Guidance,
    Seed,
    Model,
    OutputDirectory,
    Acceleration,
    StartFrame,
    EndFrame,
    ReferenceImage,
    ReferenceVideo,
    ReferenceAudio,
}

impl WorkflowTemplate {
    pub fn from_json(json_text: &str) -> Result<Self> {
        let template: Self = serde_json::from_str(json_text)
            .map_err(|e| AdapterError::InvalidTemplate(format!("工作流模板 JSON 格式错误：{e}")))?;
        template.validate()?;
        Ok(template)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| AdapterError::Io(format!("读取工作流模板失败：{e}")))?;
        Self::from_json(&text)
    }

    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() || self.title.trim().is_empty() || self.version == 0 {
            return Err(AdapterError::InvalidTemplate(
                "模板 id、标题和版本必须有效".into(),
            ));
        }
        for binding in &self.bindings {
            let node = self
                .workflow
                .get(&binding.node)
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    AdapterError::InvalidTemplate("参数绑定指向了不存在的节点".into())
                })?;
            if !node.get("inputs").is_some_and(Value::is_object) {
                return Err(AdapterError::InvalidTemplate(
                    "参数绑定节点缺少 inputs 对象".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn build_plan(
        &self,
        request: &GenerateRequest,
        probe: &ProbeResult,
    ) -> Result<ExecutionPlan> {
        let capability = probe.check(&self.required_node_types);
        if !capability.compatible {
            return Err(AdapterError::MissingCapability(
                capability.missing_node_types,
            ));
        }
        let mut prompt = self.workflow.clone();
        for binding in &self.bindings {
            let value = resolve_role(&binding.role, request);
            match value {
                Some(value) => {
                    prompt
                        .get_mut(&binding.node)
                        .and_then(Value::as_object_mut)
                        .and_then(|node| node.get_mut("inputs"))
                        .and_then(Value::as_object_mut)
                        .expect("模板已验证")
                        .insert(binding.input.clone(), value);
                }
                None if binding.required => {
                    return Err(AdapterError::MissingValue(format!(
                        "生成请求缺少必需参数：{:?}",
                        binding.role
                    )));
                }
                None => {}
            }
        }
        Ok(ExecutionPlan {
            workflow_id: self.id.clone(),
            workflow_version: self.version,
            title: self.title.clone(),
            required_node_types: self.required_node_types.clone(),
            asset_count: request.assets.len(),
            estimated_frames: request.frames,
            output_directory: request.output_directory.clone(),
            acceleration: request.acceleration.clone(),
            prompt,
        })
    }
}

fn resolve_role(role: &ParameterRole, request: &GenerateRequest) -> Option<Value> {
    let path_for = |wanted: AssetRole| {
        request.assets.iter().find_map(|asset| match asset {
            Asset::Image { path, role }
            | Asset::Video { path, role }
            | Asset::Audio { path, role }
                if *role == wanted =>
            {
                Some(json!(path))
            }
            _ => None,
        })
    };
    Some(match role {
        ParameterRole::Prompt => json!(request.prompt),
        ParameterRole::NegativePrompt => json!(request.negative_prompt),
        ParameterRole::Width => json!(request.width),
        ParameterRole::Height => json!(request.height),
        ParameterRole::Frames => json!(request.frames),
        ParameterRole::Fps => json!(request.fps),
        ParameterRole::Steps => json!(request.steps),
        ParameterRole::Guidance => json!(request.guidance),
        ParameterRole::Seed => json!(request.seed),
        ParameterRole::Model => json!(request.model),
        ParameterRole::OutputDirectory => json!(request.output_directory),
        ParameterRole::Acceleration => json!(request.acceleration.as_ref()?),
        ParameterRole::StartFrame => path_for(AssetRole::StartFrame)?,
        ParameterRole::EndFrame => path_for(AssetRole::EndFrame)?,
        ParameterRole::ReferenceImage => path_for(AssetRole::Reference)?,
        ParameterRole::ReferenceVideo => path_for(AssetRole::MotionReference)?,
        ParameterRole::ReferenceAudio => path_for(AssetRole::AudioReference)?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub reachable: bool,
    #[serde(default)]
    pub node_types: BTreeSet<String>,
}

impl ProbeResult {
    pub fn from_object_info(value: &Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| AdapterError::InvalidResponse("object_info 必须是对象".into()))?;
        Ok(Self {
            reachable: true,
            node_types: object.keys().cloned().collect(),
        })
    }

    pub fn check(&self, required: &[String]) -> CapabilityCheck {
        let missing_node_types = required
            .iter()
            .filter(|node| !self.node_types.contains(*node))
            .cloned()
            .collect::<Vec<_>>();
        CapabilityCheck {
            compatible: self.reachable && missing_node_types.is_empty(),
            missing_node_types,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCheck {
    pub compatible: bool,
    pub missing_node_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptResponse {
    pub prompt_id: String,
    #[serde(default)]
    pub number: Option<f64>,
    #[serde(default)]
    pub node_errors: Value,
}

impl PromptResponse {
    pub fn parse(value: Value) -> Result<Self> {
        serde_json::from_value(value)
            .map_err(|e| AdapterError::InvalidResponse(format!("/prompt 响应格式错误：{e}")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub running: Vec<QueueItem>,
    pub pending: Vec<QueueItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub sequence: i64,
    pub prompt_id: String,
}

impl QueueSnapshot {
    pub fn parse(value: &Value) -> Result<Self> {
        fn items(value: Option<&Value>) -> Vec<QueueItem> {
            value
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    let row = row.as_array()?;
                    Some(QueueItem {
                        sequence: row.first()?.as_i64()?,
                        prompt_id: row.get(1)?.as_str()?.to_owned(),
                    })
                })
                .collect()
        }
        let object = value
            .as_object()
            .ok_or_else(|| AdapterError::InvalidResponse("/queue 响应必须是对象".into()))?;
        Ok(Self {
            running: items(object.get("queue_running")),
            pending: items(object.get("queue_pending")),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub prompt_id: String,
    pub completed: bool,
    pub outputs: Vec<OutputAsset>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputAsset {
    pub filename: String,
    pub subfolder: String,
    pub media_type: String,
}

pub fn parse_history(value: &Value, prompt_id: &str) -> Result<Option<HistoryEntry>> {
    let Some(entry) = value.get(prompt_id) else {
        return Ok(None);
    };
    let mut outputs = Vec::new();
    if let Some(nodes) = entry.get("outputs").and_then(Value::as_object) {
        for node_output in nodes.values().filter_map(Value::as_object) {
            for (kind, media_type) in [("images", "image"), ("videos", "video"), ("audio", "audio")]
            {
                for item in node_output
                    .get(kind)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(filename) = item.get("filename").and_then(Value::as_str) {
                        outputs.push(OutputAsset {
                            filename: filename.into(),
                            subfolder: item
                                .get("subfolder")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                            media_type: media_type.into(),
                        });
                    }
                }
            }
        }
    }
    let status = entry.get("status");
    let completed = status
        .and_then(|s| s.get("completed"))
        .and_then(Value::as_bool)
        .unwrap_or(!outputs.is_empty());
    let error = status
        .and_then(|s| s.get("messages"))
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages
                .iter()
                .rev()
                .find(|m| m.get(0).and_then(Value::as_str) == Some("execution_error"))
                .and_then(|m| m.get(1))
                .and_then(|d| d.get("exception_message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    Ok(Some(HistoryEntry {
        prompt_id: prompt_id.into(),
        completed,
        outputs,
        error,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionEvent {
    Status {
        queue_remaining: u64,
    },
    Started {
        prompt_id: String,
    },
    Progress {
        prompt_id: Option<String>,
        value: u64,
        max: u64,
    },
    PreviewReady {
        prompt_id: Option<String>,
    },
    Completed {
        prompt_id: String,
    },
    Failed {
        prompt_id: String,
        message: String,
    },
    Ignored,
}

pub fn parse_progress_event(text: &str) -> Result<ExecutionEvent> {
    let envelope: Value = serde_json::from_str(text)
        .map_err(|e| AdapterError::InvalidResponse(format!("进度事件 JSON 格式错误：{e}")))?;
    let event_type = envelope.get("type").and_then(Value::as_str).unwrap_or("");
    let data = envelope.get("data").unwrap_or(&Value::Null);
    let prompt_id = || {
        data.get("prompt_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Ok(match event_type {
        "status" => ExecutionEvent::Status {
            queue_remaining: data
                .pointer("/status/exec_info/queue_remaining")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
        "execution_start" => ExecutionEvent::Started {
            prompt_id: prompt_id().ok_or_else(|| {
                AdapterError::InvalidResponse("execution_start 缺少 prompt_id".into())
            })?,
        },
        "progress" => ExecutionEvent::Progress {
            prompt_id: prompt_id(),
            value: data.get("value").and_then(Value::as_u64).unwrap_or(0),
            max: data.get("max").and_then(Value::as_u64).unwrap_or(0),
        },
        "executed" => ExecutionEvent::PreviewReady {
            prompt_id: prompt_id(),
        },
        "execution_success" => ExecutionEvent::Completed {
            prompt_id: prompt_id().ok_or_else(|| {
                AdapterError::InvalidResponse("execution_success 缺少 prompt_id".into())
            })?,
        },
        "execution_error" => ExecutionEvent::Failed {
            prompt_id: prompt_id().unwrap_or_default(),
            message: data
                .get("exception_message")
                .and_then(Value::as_str)
                .unwrap_or("ComfyUI 执行失败")
                .into(),
        },
        _ => ExecutionEvent::Ignored,
    })
}
