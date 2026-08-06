//! Versioned workflow asset registry.
//!
//! Provenance is part of the API: bundled fixtures can never be presented as
//! official assets. A future official workflow must carry an immutable source
//! URL and a digest computed over the exact bundled bytes.

use crate::comfy::{ProbeResult, WorkflowTemplate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const T2V_JSON: &str = include_str!("../resources/workflows/h3_t2v_fixture.json");
const REF2VA_JSON: &str = include_str!("../resources/workflows/h3_ref2va_fixture.json");
const OFFICIAL_T2V_JSON: &[u8] =
    include_bytes!("../resources/workflows/official/video_minimax_h3_t2v.json");
const OFFICIAL_R2V_JSON: &[u8] =
    include_bytes!("../resources/workflows/official/video_minimax_h3_r2v.json");

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialReferenceWorkflow {
    pub mode: WorkflowMode,
    pub filename: &'static str,
    pub source_url: &'static str,
    pub sha256: &'static str,
    pub byte_len: usize,
}

pub fn official_reference_workflows() -> [OfficialReferenceWorkflow; 2] {
    [
        OfficialReferenceWorkflow {
            mode: WorkflowMode::T2v,
            filename: "video_minimax_h3_t2v.json",
            source_url: "https://raw.githubusercontent.com/Comfy-Org/workflow_templates/main/templates/video_minimax_h3_t2v.json",
            sha256: "31ab33fdb053a7834cc866bd7aa08b887518fc656e4a796c89779c6b5e1786e6",
            byte_len: OFFICIAL_T2V_JSON.len(),
        },
        OfficialReferenceWorkflow {
            mode: WorkflowMode::Ref2va,
            filename: "video_minimax_h3_r2v.json",
            source_url: "https://raw.githubusercontent.com/Comfy-Org/workflow_templates/main/templates/video_minimax_h3_r2v.json",
            sha256: "099d24eda6263854818975c7209db6f29ebfd0339936c928f12293d5ab029ffb",
            byte_len: OFFICIAL_R2V_JSON.len(),
        },
    ]
}

pub fn verify_official_reference_workflows() -> Result<(), RegistryError> {
    for (descriptor, bytes) in official_reference_workflows()
        .into_iter()
        .zip([OFFICIAL_T2V_JSON, OFFICIAL_R2V_JSON])
    {
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual != descriptor.sha256 {
            return Err(RegistryError::DigestMismatch {
                id: descriptor.filename.into(),
                expected: descriptor.sha256.into(),
                actual,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowMode {
    T2v,
    Ref2va,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowProvenance {
    Official,
    ProjectFixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDescriptor {
    pub id: &'static str,
    pub mode: WorkflowMode,
    pub version: u32,
    pub title: &'static str,
    pub sha256: &'static str,
    pub provenance: WorkflowProvenance,
    pub source: &'static str,
    pub source_url: Option<&'static str>,
    #[serde(skip)]
    json: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    pub workflow_id: String,
    pub reachable: bool,
    pub compatible: bool,
    pub missing_node_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    NotFound(WorkflowMode),
    DigestMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    InvalidTemplate(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(mode) => write!(f, "未注册工作流模式：{mode:?}"),
            Self::DigestMismatch {
                id,
                expected,
                actual,
            } => write!(
                f,
                "工作流 {id} 完整性校验失败：期望 {expected}，实际 {actual}"
            ),
            Self::InvalidTemplate(message) => write!(f, "工作流模板无效：{message}"),
        }
    }
}

impl std::error::Error for RegistryError {}

const WORKFLOWS: [WorkflowDescriptor; 2] = [
    WorkflowDescriptor {
        id: "h3-t2v-fixture",
        mode: WorkflowMode::T2v,
        version: 1,
        title: "H3 文生视频（开发夹具）",
        sha256: "c36c77da71fa0345e65b9fac9faf12d26505453a794d1d34aba4d64ef102dde3",
        provenance: WorkflowProvenance::ProjectFixture,
        source: "Langbai H3 Studio project-authored development fixture",
        source_url: None,
        json: T2V_JSON,
    },
    WorkflowDescriptor {
        id: "h3-ref2va-fixture",
        mode: WorkflowMode::Ref2va,
        version: 1,
        title: "H3 全模态参考视频（开发夹具）",
        sha256: "eb09fcdd08ff12e93405a3044c05aeab842447fdea9fd1e5c1c8d415e6f2567f",
        provenance: WorkflowProvenance::ProjectFixture,
        source: "Langbai H3 Studio project-authored development fixture",
        source_url: None,
        json: REF2VA_JSON,
    },
];

/// Returns immutable metadata for every bundled workflow.
pub fn registered_workflows() -> &'static [WorkflowDescriptor] {
    &WORKFLOWS
}

/// Selects the newest registered template for a generation mode.
pub fn select_workflow(mode: WorkflowMode) -> Result<&'static WorkflowDescriptor, RegistryError> {
    WORKFLOWS
        .iter()
        .filter(|item| item.mode == mode)
        .max_by_key(|item| item.version)
        .ok_or(RegistryError::NotFound(mode))
}

impl WorkflowDescriptor {
    pub fn bundled_json(&self) -> &'static str {
        self.json
    }

    pub fn actual_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.json.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Verifies the exact resource bytes before parsing the semantic template.
    pub fn load(&self) -> Result<WorkflowTemplate, RegistryError> {
        let actual = self.actual_sha256();
        if actual != self.sha256 {
            return Err(RegistryError::DigestMismatch {
                id: self.id.into(),
                expected: self.sha256.into(),
                actual,
            });
        }
        let template = WorkflowTemplate::from_json(self.json)
            .map_err(|error| RegistryError::InvalidTemplate(error.to_string()))?;
        if template.id != self.id || template.version != self.version {
            return Err(RegistryError::InvalidTemplate(format!(
                "注册元数据与模板不一致：{} v{}",
                template.id, template.version
            )));
        }
        Ok(template)
    }

    pub fn capability_report(
        &self,
        probe: &ProbeResult,
    ) -> Result<CapabilityReport, RegistryError> {
        let template = self.load()?;
        let check = probe.check(&template.required_node_types);
        Ok(CapabilityReport {
            workflow_id: self.id.into(),
            reachable: probe.reachable,
            compatible: check.compatible,
            missing_node_types: check.missing_node_types,
        })
    }
}
